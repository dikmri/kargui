//! Windows のトレイ（通知領域のアイコン）。Shell_NotifyIconW で出し、知らせ（WM_APP_TRAY）は主の窓の処理が受ける。
//! OS への呼び出しはエクスプローラーと知らせをやり取りし、メニューは閉じるまで戻らないので、どちらも窓の処理の
//! 借りの外で行う（platform.rs の Post::Tray と Post::TrayMenu）。

use std::sync::OnceLock;

use windows::Win32::Foundation::*;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, w};

use crate::log;
use crate::ui::tray::TrayNode;

/// トレイのアイコンの知らせ（Shell_NotifyIconW の uCallbackMessage）
pub(crate) const WM_APP_TRAY: u32 = WM_APP + 3;
/// アイコンの番号（主の窓に 1 つだけ）
const ID: u32 = 1;
/// 左のボタンで押した（NOTIFYICON_VERSION_4 の出来事）
pub(crate) const NIN_SELECT: u32 = WM_USER;
/// キーボードで選んで Enter か Space を押した（NIN_SELECT | NINF_KEY）
pub(crate) const NIN_KEYSELECT: u32 = WM_USER | NINF_KEY;

/// OS へ送る事
pub(crate) enum Op {
    /// 出す（説明とアイコン）
    Add { icon: HICON, tip: String },
    /// 変わった物だけを送る
    Modify { icon: Option<HICON>, tip: Option<String> },
    Delete,
}

impl Op {
    fn name(&self) -> &'static str {
        match self {
            Op::Add { .. } => "add",
            Op::Modify { .. } => "modify",
            Op::Delete => "delete",
        }
    }
}

/// エクスプローラーが起動し直した時に、全部の窓へ送られる知らせ（アイコンを入れ直す）
pub(crate) fn taskbar_created() -> u32 {
    static MSG: OnceLock<u32> = OnceLock::new();
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

/// 通知領域のアイコンの大きさ（その窓の DPI の小さいアイコン。物理ピクセル）
pub(crate) fn icon_size(hwnd: HWND) -> u32 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        GetSystemMetricsForDpi(SM_CXSMICON, dpi).max(16) as u32
    }
}

/// 送る。終わったか（OS が受け付けたか）を返す
pub(crate) fn apply(hwnd: HWND, op: &Op) -> bool {
    let mut d = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: ID, ..Default::default() };
    let tip = |d: &mut NOTIFYICONDATAW, text: &str| {
        // 終わりの 0 を入れて 127 文字まで
        for (slot, c) in d.szTip.iter_mut().zip(text.encode_utf16().take(127)) {
            *slot = c;
        }
        d.uFlags |= NIF_TIP | NIF_SHOWTIP;
    };
    let ok = unsafe {
        match op {
            Op::Add { icon, tip: text } => {
                d.uFlags = NIF_MESSAGE | NIF_ICON;
                d.uCallbackMessage = WM_APP_TRAY;
                d.hIcon = *icon;
                tip(&mut d, text);
                let added = Shell_NotifyIconW(NIM_ADD, &d).as_bool();
                // 新しい形の知らせ（押した位置を wParam に、出来事を lParam の下位に入れる）
                d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                added && Shell_NotifyIconW(NIM_SETVERSION, &d).as_bool()
            }
            Op::Modify { icon, tip: text } => {
                if let Some(i) = icon {
                    d.uFlags |= NIF_ICON;
                    d.hIcon = *i;
                }
                if let Some(t) = text {
                    tip(&mut d, t);
                }
                Shell_NotifyIconW(NIM_MODIFY, &d).as_bool()
            }
            Op::Delete => Shell_NotifyIconW(NIM_DELETE, &d).as_bool(),
        }
    };
    log::event("tray.os", &format!("op={}", op.name()), &format!("ok={ok}"));
    ok
}

/// メニューを画面の (x, y) に出し、選んだ項目の番号を返す（閉じただけなら None）。閉じるまで戻らない
pub(crate) fn show_menu(hwnd: HWND, x: i32, y: i32, nodes: &[TrayNode]) -> Option<u32> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        fill(menu, nodes);
        // メニューの外を押した時に閉じるよう、先に前面の窓にする（Shell_NotifyIcon の決まり）
        let _ = SetForegroundWindow(hwnd);
        let align = if GetSystemMetricsForDpi(SM_MENUDROPALIGNMENT, GetDpiForWindow(hwnd).max(96)) != 0 { TPM_RIGHTALIGN } else { TPM_LEFTALIGN };
        let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | align;
        let cmd = TrackPopupMenuEx(menu, flags.0, x, y, hwnd, None);
        // 次に開いた時にすぐ閉じないよう、何もしない知らせを送る（同じく決まり）
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        log::event("tray.menu_closed", &format!("x={x} y={y}"), &format!("cmd={}", cmd.0));
        (cmd.0 > 0).then_some(cmd.0 as u32)
    }
}

/// メニューに項目を入れる（入れ子のメニューは作って入れる。DestroyMenu が中も片付ける）
unsafe fn fill(menu: HMENU, nodes: &[TrayNode]) {
    for n in nodes {
        unsafe {
            match n {
                TrayNode::Separator { .. } => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                TrayNode::Item { id, text, enabled, checked, children } => {
                    // & は次の文字のアクセスキーの印なので、そのまま見せるには重ねる
                    let label = HSTRING::from(text.replace('&', "&&"));
                    let mut flags = MF_STRING;
                    if !enabled {
                        flags |= MF_GRAYED;
                    }
                    if *checked == Some(true) {
                        flags |= MF_CHECKED;
                    }
                    if children.is_empty() {
                        let _ = AppendMenuW(menu, flags, *id as usize, &label);
                    } else if let Ok(sub) = CreatePopupMenu() {
                        fill(sub, children);
                        let _ = AppendMenuW(menu, flags | MF_POPUP, sub.0 as usize, &label);
                    }
                }
            }
        }
    }
}
