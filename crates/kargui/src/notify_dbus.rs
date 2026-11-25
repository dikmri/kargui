//! Linux の通知（org.freedesktop.Notifications）。Wayland と X11 で同じ。
//!
//! 窓の処理は止めない: 通知を出す頼みは 1 本のスレッドが順に Notify で送り、押された知らせ（ActionInvoked）は別の
//! スレッドが受けて列に積み、eventfd に書いて窓の処理を起こす（ime_dbus・sni と同じ形）。同じキーの通知は replaces_id
//! で置き換える。本文を押すと「default」、ボタンは「button:id」の action が届く。

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use zbus::blocking::Connection;
use zbus::zvariant::Value;

use crate::NotificationAction;
use crate::log;
use crate::ui::notify::Notification;

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// 窓の処理から通知のスレッドへの頼み（頼んだ窓の番号, キー, 中身）
type Cmd = (u32, String, Notification);
/// 通知のスレッドから窓の処理への答え（頼んだ窓の番号, キー, 答え）
pub(crate) type Answer = (u32, String, NotificationAction);

/// 窓の処理が持つ通知の係
pub(crate) struct Notifier {
    tx: Sender<Cmd>,
    /// 押された答え（eventfd で起こされた窓の処理が読む）
    pub rx: Receiver<Answer>,
}

impl Notifier {
    pub fn show(&self, win: u32, key: String, n: Notification) {
        let _ = self.tx.send((win, key, n));
    }
}

/// 出した通知（通知の番号 → 頼んだ窓とキー、キー → 番号）
#[derive(Default)]
struct Shown {
    by_id: HashMap<u32, (u32, String)>,
    by_key: HashMap<String, u32>,
}

/// 通知のスレッドを始める。app_name は通知に出すアプリの名前（Options::title）
pub(crate) fn spawn(app_name: String, wake: Arc<OwnedFd>) -> Notifier {
    let (tx, cmds) = channel::<Cmd>();
    let (answer_tx, rx) = channel::<Answer>();
    let spawned = std::thread::Builder::new().name("kargui-notify".into()).spawn(move || run(app_name, cmds, answer_tx, wake));
    if let Err(e) = spawned {
        log::event("notify.error", "spawn", &e.to_string());
    }
    Notifier { tx, rx }
}

/// action の名前を答えにする（本文は「default」、ボタンは「button:id」）
pub(crate) fn action(key: &str) -> Option<NotificationAction> {
    match key {
        "default" => Some(NotificationAction::Body),
        k => k.strip_prefix("button:").map(|id| NotificationAction::Button(id.to_string())),
    }
}

/// Notify の actions（名前と文字を交互に並べる）。本文を押した時の「default」を先に置く
pub(crate) fn actions(n: &Notification) -> Vec<String> {
    let mut a = vec!["default".to_string(), String::new()];
    for (id, label) in &n.buttons {
        a.push(format!("button:{id}"));
        a.push(label.clone());
    }
    a
}

fn run(app_name: String, cmds: Receiver<Cmd>, answers: Sender<Answer>, wake: Arc<OwnedFd>) {
    let conn = match Connection::session() {
        Ok(c) => c,
        Err(e) => {
            log::event("notify.error", "connect", &e.to_string());
            return;
        }
    };
    let shown = Arc::new(Mutex::new(Shown::default()));
    // 押された・閉じた知らせを受けるスレッド
    {
        let (conn, shown) = (conn.clone(), shown.clone());
        let _ = std::thread::Builder::new().name("kargui-notify-signals".into()).spawn(move || listen(conn, shown, answers, wake));
    }
    for (win, key, n) in cmds {
        let replaces = shown.lock().unwrap_or_else(|e| e.into_inner()).by_key.get(&key).copied().unwrap_or(0);
        let mut hints: HashMap<&str, Value> = HashMap::new();
        if n.silent {
            hints.insert("suppress-sound", Value::from(true));
        }
        let body = (app_name.as_str(), replaces, "", n.title.as_str(), n.body.as_str(), actions(&n), hints, -1i32);
        match conn.call_method(Some(DEST), PATH, Some(DEST), "Notify", &body).and_then(|m| m.body().deserialize::<u32>()) {
            Ok(id) => {
                let mut s = shown.lock().unwrap_or_else(|e| e.into_inner());
                if replaces != 0 && replaces != id {
                    s.by_id.remove(&replaces);
                }
                s.by_id.insert(id, (win, key.clone()));
                s.by_key.insert(key.clone(), id);
                log::event("notify.show", &crate::ui::notify::line(&key, &n), &format!("id={id} replaces={replaces}"));
            }
            Err(e) => log::event("notify.show", &crate::ui::notify::line(&key, &n), &format!("error={e}")),
        }
    }
}

/// ActionInvoked と NotificationClosed を受ける（どのアプリの通知の知らせも来るので、出した番号だけを見る）
fn listen(conn: Connection, shown: Arc<Mutex<Shown>>, answers: Sender<Answer>, wake: Arc<OwnedFd>) {
    let rule = format!("type='signal',interface='{DEST}',path='{PATH}'");
    let messages = match zbus::blocking::MessageIterator::for_match_rule(rule.as_str(), &conn, Some(16)) {
        Ok(m) => m,
        Err(e) => {
            log::event("notify.error", "listen", &e.to_string());
            return;
        }
    };
    for m in messages.flatten() {
        let member = m.header().member().map(|s| s.to_string()).unwrap_or_default();
        match member.as_str() {
            "ActionInvoked" => {
                let Ok((id, key)) = m.body().deserialize::<(u32, String)>() else { continue };
                let target = shown.lock().unwrap_or_else(|e| e.into_inner()).by_id.get(&id).cloned();
                if let (Some((win, k)), Some(a)) = (target, action(&key)) {
                    log::event("notify.click", &format!("id={id} action={key}"), &format!("key={k} {a:?}"));
                    let _ = answers.send((win, k, a));
                    let _ = rustix::io::write(&*wake, &1u64.to_ne_bytes());
                }
            }
            "NotificationClosed" => {
                let Ok((id, reason)) = m.body().deserialize::<(u32, u32)>() else { continue };
                let mut s = shown.lock().unwrap_or_else(|e| e.into_inner());
                if let Some((_, k)) = s.by_id.remove(&id) {
                    if s.by_key.get(&k) == Some(&id) {
                        s.by_key.remove(&k);
                    }
                    log::event("notify.closed", &format!("id={id} key={k}"), &format!("reason={reason}"));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_put_the_body_first_and_buttons_get_a_prefix() {
        let n = Notification::new("t").button("default", "Odd id").button("open", "Open");
        assert_eq!(actions(&n), ["default", "", "button:default", "Odd id", "button:open", "Open"]);
        assert_eq!(action("default"), Some(NotificationAction::Body));
        assert_eq!(action("button:default"), Some(NotificationAction::Button("default".into())));
        assert_eq!(action("something-else"), None);
    }
}
