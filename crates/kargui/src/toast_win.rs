//! Windows の通知（トースト）。WinRT の ToastNotification で出す。アプリの名前は、利用者ごとのレジストリ
//! HKCU\Software\Classes\AppUserModelId\<実行ファイルの名前> の DisplayName に書いて登録する（最初の通知の時に 1 回）。
//!
//! 出すのは窓のスレッド（COM の STA）で、窓の処理の借りの外（platform.rs の Post::Notify。WinRT の呼び出しは、答えを
//! 待つ間に窓の知らせを処理することがある）。押された知らせ（Activated）は WinRT の別のスレッドに来るので、答えを
//! 列に積み、頼んだ窓に WM_APP_NOTIFY を積む。窓の処理が列から取り出して Context に入れる。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{ToastActivatedEventArgs, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification, ToastNotificationManager, ToastNotifier};
use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, IInspectable, Interface};

use crate::NotificationAction;
use crate::log;
use crate::ui::notify::Notification;

/// 押された通知の答えが届いた（列に積んだ答えを、窓の処理が取り出す）
pub(crate) const WM_APP_NOTIFY: u32 = WM_APP + 4;
/// 通知のまとまりの名前（Tag はキー）
const GROUP: &str = "kargui";

/// 押された答え（頼んだ窓, キー, 答え）。WinRT のスレッドが積み、窓のスレッドが取り出す
static ANSWERS: Mutex<Vec<(isize, String, NotificationAction)>> = Mutex::new(Vec::new());

/// 窓のスレッドの持ち物: 通知を出す係と、出した通知（押された知らせを受け続けるため、キーごとに持っておく）
struct Toasts {
    notifier: ToastNotifier,
    shown: HashMap<String, ToastNotification>,
}

thread_local! {
    static TOASTS: RefCell<Option<Toasts>> = const { RefCell::new(None) };
}

/// 窓 hwnd 宛ての答えを取り出す
pub(crate) fn take_answers(hwnd: HWND) -> Vec<(String, NotificationAction)> {
    let mut all = ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
    let (mine, rest): (Vec<_>, Vec<_>) = all.drain(..).partition(|(h, _, _)| *h == hwnd.0 as isize);
    *all = rest;
    mine.into_iter().map(|(_, k, a)| (k, a)).collect()
}

/// アプリの名前を決める（実行ファイルの名前。AppUserModelID に使えない文字は - にする）
fn app_user_model_id() -> String {
    let stem = std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "kargui".into());
    let id: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '-' }).take(120).collect();
    if id.is_empty() { "kargui".into() } else { id }
}

/// アプリの名前（トーストの上に出る）を、利用者ごとのレジストリに書く
fn register(aumid: &str, display_name: &str) {
    let path = HSTRING::from(format!("Software\\Classes\\AppUserModelId\\{aumid}"));
    let mut key = HKEY::default();
    let r = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, &path, &mut key) };
    if r != ERROR_SUCCESS {
        log::event("notify.register", &format!("aumid={aumid}"), &format!("error={r:?}"));
        return;
    }
    let value: Vec<u16> = display_name.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2) };
    let w = unsafe { RegSetValueExW(key, windows::core::w!("DisplayName"), None, REG_SZ, Some(bytes)) };
    let _ = unsafe { RegCloseKey(key) };
    log::event("notify.register", &format!("aumid={aumid} name={display_name:?}"), &format!("{w:?}"));
}

/// XML に入れる文字（& < > " ' を書き換える）
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// トーストの中身（ToastGeneric: 題・本文・ボタン）。本文を押すと引数は空、ボタンは「button:id」
pub(crate) fn xml(n: &Notification) -> String {
    let mut s = String::from("<toast><visual><binding template=\"ToastGeneric\">");
    s.push_str(&format!("<text>{}</text>", esc(&n.title)));
    if !n.body.is_empty() {
        s.push_str(&format!("<text>{}</text>", esc(&n.body)));
    }
    s.push_str("</binding></visual>");
    if !n.buttons.is_empty() {
        s.push_str("<actions>");
        for (id, label) in &n.buttons {
            s.push_str(&format!("<action content=\"{}\" arguments=\"button:{}\" activationType=\"foreground\"/>", esc(label), esc(id)));
        }
        s.push_str("</actions>");
    }
    if n.silent {
        s.push_str("<audio silent=\"true\"/>");
    }
    s.push_str("</toast>");
    s
}

/// 押された時の引数を答えにする
pub(crate) fn action(arguments: &str) -> NotificationAction {
    match arguments.strip_prefix("button:") {
        Some(id) => NotificationAction::Button(id.to_string()),
        None => NotificationAction::Body,
    }
}

/// Tag（64 文字まで）。長いキーは前と後ろを残す
fn tag(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 64 { key.to_string() } else { chars[..32].iter().chain(&chars[chars.len() - 31..]).collect::<String>() + "~" }
}

/// 通知を出す。app_name はトーストの上に出るアプリの名前（最初の 1 回だけ登録する）
pub(crate) fn show(hwnd: HWND, app_name: &str, key: &str, n: &Notification) {
    match show_inner(hwnd, app_name, key, n) {
        Ok(()) => log::event("notify.show", &crate::ui::notify::line(key, n), "ok"),
        Err(e) => log::event("notify.show", &crate::ui::notify::line(key, n), &format!("error={e}")),
    }
}

fn show_inner(hwnd: HWND, app_name: &str, key: &str, n: &Notification) -> windows::core::Result<()> {
    TOASTS.with(|t| {
        let mut t = t.borrow_mut();
        if t.is_none() {
            let aumid = app_user_model_id();
            register(&aumid, app_name);
            let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&aumid))?;
            *t = Some(Toasts { notifier, shown: HashMap::new() });
        }
        let toasts = t.as_mut().expect("just made");
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml(n)))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        toast.SetTag(&HSTRING::from(tag(key)))?;
        toast.SetGroup(&HSTRING::from(GROUP))?;
        let (target, owned_key) = (hwnd.0 as isize, key.to_string());
        toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, args| {
            let arguments = args.ok()?.cast::<ToastActivatedEventArgs>()?.Arguments()?.to_string();
            let a = action(&arguments);
            log::event("notify.click", &format!("key={owned_key} arguments={arguments:?}"), &format!("{a:?}"));
            ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).push((target, owned_key.clone(), a));
            unsafe {
                let _ = PostMessageW(Some(HWND(target as *mut _)), WM_APP_NOTIFY, WPARAM(0), LPARAM(0));
            }
            Ok(())
        }))?;
        let dismissed_key = key.to_string();
        toast.Dismissed(&TypedEventHandler::<ToastNotification, ToastDismissedEventArgs>::new(move |_, args| {
            let reason = args.ok()?.Reason()?;
            log::event("notify.dismissed", &format!("key={dismissed_key}"), &format!("reason={}", reason.0));
            Ok(())
        }))?;
        let failed_key = key.to_string();
        toast.Failed(&TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(move |_, args| {
            let code = args.ok()?.ErrorCode()?;
            log::event("notify.failed", &format!("key={failed_key}"), &format!("error={code:?}"));
            Ok(())
        }))?;
        toasts.notifier.Show(&toast)?;
        toasts.shown.insert(key.to_string(), toast);
        Ok(())
    })
}

/// 主の窓の題（トーストの上に出すアプリの名前）
pub(crate) fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_xml_escapes_text_and_buttons_carry_their_ids() {
        let n = Notification::new("A & B").body("<b>").button("snooze", "Later \"5\"");
        assert_eq!(
            xml(&n),
            "<toast><visual><binding template=\"ToastGeneric\"><text>A &amp; B</text><text>&lt;b&gt;</text></binding></visual>\
             <actions><action content=\"Later &quot;5&quot;\" arguments=\"button:snooze\" activationType=\"foreground\"/></actions></toast>"
        );
        assert!(xml(&Notification::new("t").silent()).ends_with("</visual><audio silent=\"true\"/></toast>"));
        assert_eq!(action(""), NotificationAction::Body);
        assert_eq!(action("button:snooze"), NotificationAction::Button("snooze".into()));
        assert_eq!(tag("short"), "short");
        assert_eq!(tag(&"x".repeat(100)).chars().count(), 64);
    }
}
