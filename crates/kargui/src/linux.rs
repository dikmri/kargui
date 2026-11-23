//! Linux の窓の Wayland（wayland.rs）と X11（x11.rs）で分け合う物: キー配列（libxkbcommon）、地元の日付と時刻、
//! 読み上げの係、OS の設定とファイルのダイアログ（ポータル）を別のスレッドで聞く処理、どちらを使うかの決め方。

use std::ffi::c_char;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use xkbcommon_dl as xkb;

use crate::input::{Key, Mods};
use crate::log;

/// ホイールの 1 刻み（物理ピクセル。Windows の platform.rs と同じ）
pub(crate) const WHEEL_STEP: f32 = 96.0;
/// 2 回押しの時間と距離（論理ピクセル）。Wayland には OS の設定を聞く道が無いので、GNOME の既定に合わせる
pub(crate) const DOUBLE_CLICK_MS: u32 = 400;
pub(crate) const DOUBLE_CLICK_DIST: f32 = 4.0;

/// ポータルから読んだ OS の設定（読まなかった物・読めなかった物は None）
pub(crate) struct PortalSettings {
    pub dark: Option<bool>,
    pub button_layout: Option<String>,
}

/// 地元の日付と時刻（C の localtime_r。/etc/localtime と TZ に従う）
#[repr(C)]
struct Tm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: std::ffi::c_long,
    tm_zone: *const c_char,
}

unsafe extern "C" {
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}

fn local_tm() -> Option<Tm> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let mut tm = Tm { tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0, tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0, tm_isdst: 0, tm_gmtoff: 0, tm_zone: std::ptr::null() };
    // SAFETY: secs と tm はこの関数の中の値。localtime_r は tm に書くだけで、ほかのスレッドと分け合わない
    let ok = unsafe { !localtime_r(&secs, &mut tm).is_null() };
    ok.then_some(tm)
}

/// 地元の今日（年・月・日）
pub(crate) fn local_date() -> Option<(i32, u32, u32)> {
    local_tm().map(|t| (t.tm_year + 1900, (t.tm_mon + 1) as u32, t.tm_mday as u32))
}

/// 地元の今の時刻（時・分）
pub(crate) fn local_time() -> Option<(u32, u32)> {
    local_tm().map(|t| (t.tm_hour as u32, t.tm_min as u32))
}

/// キー配列（libxkbcommon）と押し続け
pub(crate) struct Keyboard {
    xkb: &'static xkb::XkbCommon,
    ctx: *mut xkb::xkb_context,
    keymap: *mut xkb::xkb_keymap,
    state: *mut xkb::xkb_state,
    /// 押し続けの速さ（1 秒あたり）と、始まるまでの時間（ms）
    pub rate: i32,
    pub delay: i32,
    /// 押し続けているキー（xkb のキー番号）と、次に繰り返す時
    pub repeat: Option<(u32, Instant)>,
    /// 今の押し続けで繰り返した回数（記録用）
    pub repeat_count: u32,
}

impl Keyboard {
    pub(crate) fn new() -> Option<Keyboard> {
        let xkb = xkb::xkbcommon_option()?;
        // SAFETY: libxkbcommon の関数を、決まった引数で呼ぶ
        let ctx = unsafe { (xkb.xkb_context_new)(xkb::xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };
        if ctx.is_null() {
            return None;
        }
        Some(Keyboard { xkb, ctx, keymap: std::ptr::null_mut(), state: std::ptr::null_mut(), rate: 25, delay: 600, repeat: None, repeat_count: 0 })
    }

    /// 合成から届いたキー配列（文字列の fd）を読む
    pub(crate) fn set_keymap(&mut self, fd: OwnedFd, size: u32) -> bool {
        // SAFETY: 合成が渡した fd を読むだけで写す（MAP_PRIVATE）。文字列は NUL で終わる（size に NUL を含む）
        unsafe {
            let Ok(p) = mmap(std::ptr::null_mut(), size as usize, ProtFlags::READ, MapFlags::PRIVATE, &fd, 0) else {
                return false;
            };
            let keymap = (self.xkb.xkb_keymap_new_from_string)(
                self.ctx,
                p as *const c_char,
                xkb::xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
                xkb::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            );
            let _ = munmap(p, size as usize);
            if keymap.is_null() {
                return false;
            }
            let state = (self.xkb.xkb_state_new)(keymap);
            if state.is_null() {
                (self.xkb.xkb_keymap_unref)(keymap);
                return false;
            }
            self.drop_keymap();
            self.keymap = keymap;
            self.state = state;
        }
        true
    }

    fn drop_keymap(&mut self) {
        // SAFETY: 作った物を 1 回ずつ外す
        unsafe {
            if !self.state.is_null() {
                (self.xkb.xkb_state_unref)(self.state);
            }
            if !self.keymap.is_null() {
                (self.xkb.xkb_keymap_unref)(self.keymap);
            }
        }
        self.state = std::ptr::null_mut();
        self.keymap = std::ptr::null_mut();
    }

    /// X サーバーのキー配列を読む（libxkbcommon-x11。conn は libxcb の接続）。読み直しにも使う
    pub(crate) fn set_x11_keymap(&mut self, conn: *mut std::ffi::c_void) -> bool {
        let Some(x) = xkb::x11::xkbcommon_x11_option() else { return false };
        let conn = conn as *mut xkb::x11::xcb_connection_t;
        // SAFETY: 生きている libxcb の接続を渡す。作った keymap と state はこの Keyboard が持ち、drop_keymap で外す
        unsafe {
            let device = (x.xkb_x11_get_core_keyboard_device_id)(conn);
            if device < 0 {
                return false;
            }
            let keymap = (x.xkb_x11_keymap_new_from_device)(self.ctx, conn, device, xkb::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS);
            if keymap.is_null() {
                return false;
            }
            let state = (x.xkb_x11_state_new_from_device)(keymap, conn, device);
            if state.is_null() {
                (self.xkb.xkb_keymap_unref)(keymap);
                return false;
            }
            self.drop_keymap();
            self.keymap = keymap;
            self.state = state;
        }
        true
    }

    /// X サーバーが送った修飾キーと配列の組（XKB の StateNotify）を入れる
    pub(crate) fn update_mask(&mut self, mods: (u32, u32, u32), groups: (u32, u32, u32)) {
        if self.ready() {
            // SAFETY: 作った state に、サーバーが送った状態を入れる
            unsafe {
                (self.xkb.xkb_state_update_mask)(self.state, mods.0, mods.1, mods.2, groups.0, groups.1, groups.2);
            }
        }
    }

    pub(crate) fn ready(&self) -> bool {
        !self.state.is_null()
    }

    pub(crate) fn update_mods(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if self.ready() {
            // SAFETY: 作った state に、合成が送った修飾キーの状態を入れる
            unsafe {
                (self.xkb.xkb_state_update_mask)(self.state, depressed, latched, locked, 0, 0, group);
            }
        }
    }

    fn mod_active(&self, name: &[u8]) -> bool {
        // SAFETY: name は NUL で終わる定数（XKB_MOD_NAME_*）
        self.ready() && unsafe { (self.xkb.xkb_state_mod_name_is_active)(self.state, name.as_ptr() as *const c_char, xkb::xkb_state_component::XKB_STATE_MODS_EFFECTIVE) } > 0
    }

    pub(crate) fn mods(&self) -> Mods {
        Mods { shift: self.mod_active(xkb::XKB_MOD_NAME_SHIFT), ctrl: self.mod_active(xkb::XKB_MOD_NAME_CTRL), alt: self.mod_active(xkb::XKB_MOD_NAME_ALT) }
    }

    /// キーの番号（xkb）を、修飾キーを外した段 0 の記号にする（Windows の仮想キーと同じく、配列の文字ではなくキーで決める）
    pub(crate) fn base_sym(&self, code: u32) -> u32 {
        if !self.ready() {
            return 0;
        }
        // SAFETY: 作った keymap と state に、合成が送ったキーの番号を渡す
        unsafe {
            let layout = (self.xkb.xkb_state_key_get_layout)(self.state, code);
            let mut syms: *const u32 = std::ptr::null();
            let n = (self.xkb.xkb_keymap_key_get_syms_by_level)(self.keymap, code, layout, 0, &mut syms);
            if n > 0 && !syms.is_null() { *syms } else { 0 }
        }
    }

    /// 今の修飾キーでのキーの記号（IME に渡す keyval）
    pub(crate) fn sym(&self, code: u32) -> u32 {
        if !self.ready() {
            return 0;
        }
        // SAFETY: 作った state にキーの番号を渡す
        unsafe { (self.xkb.xkb_state_key_get_one_sym)(self.state, code) }
    }

    /// キーの記号の文字（IME が送り返したキー）。文字の無い記号は空
    pub(crate) fn keysym_text(&self, sym: u32) -> String {
        let mut buf = [0 as c_char; 16];
        // SAFETY: buf の大きさを渡す。書いた長さは終わりの 0 を含む
        let n = unsafe { (self.xkb.xkb_keysym_to_utf8)(sym, buf.as_mut_ptr(), buf.len()) };
        if n <= 1 {
            return String::new();
        }
        let bytes: Vec<u8> = buf[..(n as usize - 1).min(buf.len())].iter().map(|&c| c as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// 今の修飾キーで打った文字
    pub(crate) fn utf8(&self, code: u32) -> String {
        if !self.ready() {
            return String::new();
        }
        let mut buf = [0 as c_char; 32];
        // SAFETY: buf の大きさを渡し、書かれた長さだけを読む
        let n = unsafe { (self.xkb.xkb_state_key_get_utf8)(self.state, code, buf.as_mut_ptr(), buf.len()) };
        if n <= 0 {
            return String::new();
        }
        let bytes: Vec<u8> = buf[..(n as usize).min(buf.len() - 1)].iter().map(|&c| c as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub(crate) fn repeats(&self, code: u32) -> bool {
        // SAFETY: 作った keymap にキーの番号を渡す
        self.ready() && self.rate > 0 && unsafe { (self.xkb.xkb_keymap_key_repeats)(self.keymap, code) } != 0
    }

    /// 押し続けをやめる（繰り返していたら、その回数を記録する）
    pub(crate) fn stop_repeat(&mut self, why: &str) {
        if let Some((code, _)) = self.repeat.take()
            && self.repeat_count > 0
        {
            log::event("key.repeat", &format!("code={code} why={why}"), &format!("stopped count={}", self.repeat_count));
        }
        self.repeat_count = 0;
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        self.drop_keymap();
        // SAFETY: new で作った context を外す
        unsafe { (self.xkb.xkb_context_unref)(self.ctx) };
    }
}

/// xkb の記号（段 0）→ kargui のキー
pub(crate) fn key_of(sym: u32) -> Option<Key> {
    const LETTERS: [Key; 26] = [Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z];
    const DIGITS: [Key; 10] = [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
    const FKEYS: [Key; 12] = [Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12];
    Some(match sym {
        0x61..=0x7a => LETTERS[(sym - 0x61) as usize],
        0x41..=0x5a => LETTERS[(sym - 0x41) as usize],
        0x30..=0x39 => DIGITS[(sym - 0x30) as usize],
        0xffbe..=0xffc9 => FKEYS[(sym - 0xffbe) as usize],
        0xff0d | 0xff8d => Key::Enter,
        0xff09 | 0xfe20 => Key::Tab,
        0xff1b => Key::Escape,
        0x20 => Key::Space,
        0xff08 => Key::Backspace,
        0xffff | 0xff9f => Key::Delete,
        0xff51 | 0xff96 => Key::Left,
        0xff53 | 0xff98 => Key::Right,
        0xff52 | 0xff97 => Key::Up,
        0xff54 | 0xff99 => Key::Down,
        0xff50 | 0xff95 => Key::Home,
        0xff57 | 0xff9c => Key::End,
        0xff55 | 0xff9a => Key::PageUp,
        0xff56 | 0xff9b => Key::PageDown,
        0xff63 | 0xff9e => Key::Insert,
        0xff67 => Key::ContextMenu,
        _ => return None,
    })
}

/// 読み上げ（AT-SPI）からの頼みを受ける係。accesskit_unix は 3 つの係をどれも別のスレッドから呼ぶので、
/// 操作は列に積み、eventfd に書いて窓の処理（poll）を起こす。最初の木もここでは作らず（画面の状態は窓の処理の
/// スレッドにある）、起きた後のフレームで update_if_active に全部の木を渡す
#[derive(Clone)]
pub(crate) struct A11ySink {
    pub actions: Arc<Mutex<Vec<ActionRequest>>>,
    pub wake: Arc<OwnedFd>,
}

impl A11ySink {
    fn wake(&self) {
        let _ = rustix::io::write(&*self.wake, &1u64.to_ne_bytes());
    }
}

impl ActivationHandler for A11ySink {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        log::event("a11y.activate", "", "pending (tree on next frame)");
        self.wake();
        None
    }
}

impl ActionHandler for A11ySink {
    fn do_action(&mut self, request: ActionRequest) {
        log::event("a11y.action", &format!("action={:?} node={:?}", request.action, request.target_node), "queued");
        if let Ok(mut q) = self.actions.lock() {
            q.push(request);
        }
        self.wake();
    }
}

impl DeactivationHandler for A11ySink {
    fn deactivate_accessibility(&mut self) {
        log::event("a11y.deactivate", "", "ok");
    }
}

/// 窓の app_id（Hyprland の窓の規則などで使う）は、実行ファイルの名前
pub(crate) fn app_id() -> String {
    std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "kargui".into())
}

/// 窓を開く仕組み
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Wayland,
    X11,
}

/// 環境変数から、窓を開く仕組みを決める。KARGUI_BACKEND（wayland か x11）があればそれに従う。無ければ、Wayland の
/// 合成の口（XDG_RUNTIME_DIR の下の WAYLAND_DISPLAY）があれば Wayland、無くて DISPLAY があれば X11、どちらも
/// 無ければ Wayland（つなげない理由を Err で返す）。exists は口のファイルがあるか
pub(crate) fn choose_backend(var: impl Fn(&str) -> Option<String>, exists: impl Fn(&std::path::Path) -> bool) -> Backend {
    match var("KARGUI_BACKEND").as_deref().map(str::trim) {
        Some("x11") => return Backend::X11,
        Some("wayland") => return Backend::Wayland,
        _ => {}
    }
    let socket = var("WAYLAND_DISPLAY").filter(|d| !d.is_empty()).map(|d| {
        let p = PathBuf::from(&d);
        if p.is_absolute() { p } else { PathBuf::from(var("XDG_RUNTIME_DIR").unwrap_or_default()).join(d) }
    });
    if socket.is_some_and(|p| exists(&p)) {
        return Backend::Wayland;
    }
    if var("DISPLAY").is_some_and(|d| !d.is_empty()) { Backend::X11 } else { Backend::Wayland }
}

/// OS の設定（明暗と、kargui が題の帯を描く時のボタンの並び）を、別のスレッドでポータルに聞き始める。答えが
/// 届いたら eventfd に書いて窓の処理を起こす
pub(crate) fn spawn_settings(want_dark: bool, want_buttons: bool, wake: Arc<OwnedFd>) -> Option<Receiver<PortalSettings>> {
    (want_dark || want_buttons).then(|| {
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new().name("kargui-portal-settings".into()).spawn(move || {
            let t = Instant::now();
            let dark = want_dark.then(crate::portal::read_dark);
            let button_layout = want_buttons.then(crate::portal::read_button_layout);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            match &dark {
                Some(Ok(d)) => log::event("portal.color_scheme", "", &format!("dark={d} ms={ms:.1}")),
                Some(Err(e)) => log::event("portal.color_scheme", "", &format!("error={e} ms={ms:.1}")),
                None => {}
            }
            match &button_layout {
                Some(Ok(l)) => log::event("portal.button_layout", "", &format!("layout={l:?} ms={ms:.1}")),
                Some(Err(e)) => log::event("portal.button_layout", "", &format!("error={e} ms={ms:.1}")),
                None => {}
            }
            let _ = tx.send(PortalSettings { dark: dark.and_then(Result::ok), button_layout: button_layout.and_then(Result::ok) });
            let _ = rustix::io::write(&*wake, &1u64.to_ne_bytes());
        });
        rx
    })
}

/// ファイルのダイアログを別のスレッドで出す（ポータル）。parent は親の窓（X11 は「x11:16 進の番号」、無ければ空）。
/// 答え（窓の番号, キー, ファイル）は tx に送り、eventfd に書いて窓の処理を起こす。スレッドを作れなければ false
pub(crate) fn spawn_file_dialog(win: u32, key: String, d: crate::file_dialog::FileDialog, parent: String, tx: Sender<(u32, String, Vec<PathBuf>)>, wake: Arc<OwnedFd>) -> bool {
    let spawned = std::thread::Builder::new().name("kargui-file-dialog".into()).spawn(move || {
        let files = match crate::portal::choose_files(&d, &parent) {
            Ok(f) => f,
            Err(e) => {
                log::event("file_dialog.error", &format!("key={key}"), &format!("error={e}"));
                Vec::new()
            }
        };
        log::event("file_dialog.answer", &format!("key={key} save={}", d.save), &format!("files={}", files.len()));
        let _ = tx.send((win, key, files));
        let _ = rustix::io::write(&*wake, &1u64.to_ne_bytes());
    });
    if let Err(e) = &spawned {
        log::event("file_dialog.error", "spawn", &format!("error={e}"));
    }
    spawned.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn pick(vars: &[(&str, &str)], sockets: &[&str]) -> Backend {
        let vars: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        choose_backend(|k| vars.get(k).cloned(), |p| sockets.iter().any(|s| p == std::path::Path::new(s)))
    }

    #[test]
    fn backend_follows_the_session_and_the_override() {
        let run = ("XDG_RUNTIME_DIR", "/run/user/1000");
        // Wayland の合成の口があれば Wayland（XWayland の DISPLAY もあるのが普通）
        assert_eq!(pick(&[run, ("WAYLAND_DISPLAY", "wayland-1"), ("DISPLAY", ":0")], &["/run/user/1000/wayland-1"]), Backend::Wayland);
        // 口のファイルが無い（ssh の先などで名前だけ残っている）なら X11
        assert_eq!(pick(&[run, ("WAYLAND_DISPLAY", "wayland-1"), ("DISPLAY", ":0")], &[]), Backend::X11);
        // X11 だけの画面
        assert_eq!(pick(&[("DISPLAY", ":0")], &[]), Backend::X11);
        // 絶対の場所の WAYLAND_DISPLAY
        assert_eq!(pick(&[("WAYLAND_DISPLAY", "/tmp/w0")], &["/tmp/w0"]), Backend::Wayland);
        // KARGUI_BACKEND が一番強い
        assert_eq!(pick(&[run, ("WAYLAND_DISPLAY", "wayland-1"), ("KARGUI_BACKEND", "x11")], &["/run/user/1000/wayland-1"]), Backend::X11);
        assert_eq!(pick(&[("DISPLAY", ":0"), ("KARGUI_BACKEND", "wayland")], &[]), Backend::Wayland);
        // どちらも無ければ Wayland（つなげない理由は run が返す）
        assert_eq!(pick(&[], &[]), Backend::Wayland);
    }
}
