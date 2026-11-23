//! Linux の窓（Wayland）。Windows の platform.rs と同じく、OS との境目をこのファイルに閉じる。
//!
//! 窓は xdg-shell、絵は CPU の道（cpu_frame）で描いて wl_shm の共有メモリに写す（2 枚を交互に使い、合成が
//! 読み終えた絵だけに描く）。倍率は fractional-scale（1/120 単位）で受け、絵は物理ピクセルで作り、viewporter で
//! 論理の大きさに合わせる。キー配列は libxkbcommon を動く時に読む（xkbcommon-dl。組み立てに開発用の部品が要らない）。
//! 待つのは poll の 1 か所（Wayland の接続・ほかのスレッドから起こす eventfd・時間）。
//!
//! クリップボードは wl_data_device（文字だけ）。地元の日付と時刻は C の localtime_r で読む。
//! 日本語入力は text-input-v3（fcitx5 などの IME が、合成を通して変換中の文字と確定の文字を送る）。
//! 読み上げは accesskit_unix（AT-SPI を D-Bus で話す）。Windows と同じ木を毎フレーム渡し、違いは AccessKit が見つける。
//! ファイルのダイアログと OS の明暗の設定は xdg-desktop-portal（portal.rs。別のスレッドで呼ぶ）。
//! ほかのアプリから引っ張ってきたファイルは wl_data_device の引っ張りの知らせ（text/uri-list を入った時に受け取る）。
//!
//! 窓は Win（窓ごとに面・xdg の窓・絵・Runtime・読み上げ）、座席の物（マウス・キーボード・日本語入力・クリップボード・
//! 引っ張り）は State に持つ。面と xdg の窓の知らせには窓の番号（WinId）を添え、マウス・キーボード・日本語入力・引っ張りは
//! enter の面で窓を決める。2 つ目からの窓は、Windows と同じく主の窓のフレームの後で、頼まれた窓とそろえる。
//!
//! まだ無い物（次の区切り）: 題の帯（GNOME の CSD）・X11。

use std::cell::RefCell;
use std::ffi::{OsString, c_char};
use std::os::unix::ffi::OsStringExt;
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use rustix::pipe::{PipeFlags, pipe_with};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_keyboard, wl_pointer, wl_registry, wl_seat,
    wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop, event_created_child};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{self, Shape};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1;
use wayland_protocols::wp::fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1};
use wayland_protocols::wp::text_input::zv3::client::{zwp_text_input_manager_v3, zwp_text_input_v3};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::decoration::zv1::client::{zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use xkbcommon_dl as xkb;

use crate::app::{App, Frame, Options};
use crate::cpu_frame;
use crate::damage::Summary;
use crate::geom::Color;
use crate::ime;
use crate::input::{A11yAction, Event, Key, Mods};
use crate::log;
use crate::runtime::Runtime;
use crate::text::Text;
use crate::ui::Id;
use crate::ui::context::Clipboard;

/// ホイールの 1 刻み（物理ピクセル。Windows の platform.rs と同じ）
const WHEEL_STEP: f32 = 96.0;
/// 2 回押しの時間と距離（論理ピクセル）。Wayland には OS の設定を聞く道が無いので、GNOME の既定に合わせる
const DOUBLE_CLICK_MS: u32 = 400;
const DOUBLE_CLICK_DIST: f32 = 4.0;
/// マウスのボタン（linux/input-event-codes.h）
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

fn error(what: &str, e: impl std::fmt::Display) -> crate::Error {
    crate::Error(format!("wayland: {what}: {e}"))
}

/// 文字の形（mime）。上ほど優先する
const TEXT_MIMES: [&str; 4] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "TEXT"];
/// ほかのアプリが貼る物を書き終えるのを待つ長さ
const PASTE_TIMEOUT: Duration = Duration::from_secs(1);
/// ほかのアプリから引っ張ってくるファイルの一覧の形
const URI_LIST: &str = "text/uri-list";

/// 窓（合成からの知らせ）とクリップボードの係が分け合う状態
struct Clip {
    conn: Connection,
    qh: QueueHandle<State>,
    manager: Option<wl_data_device_manager::WlDataDeviceManager>,
    device: Option<wl_data_device::WlDataDevice>,
    /// 最後の入力の番号。選択を取る時に合成へ見せる（キー・ボタン・キーボードが入った時の知らせから）
    serial: u32,
    /// 自分が出している物。ほかのアプリが選択を取ると、合成が cancelled を送るので消す
    ours: Option<(wl_data_source::WlDataSource, String)>,
    /// 今の選択（ほかのアプリの物も自分の物も、合成が offer として知らせる）
    offer: Option<wl_data_offer::WlDataOffer>,
    /// wl_data_device が無い合成では、アプリの中だけで持つ
    local: Option<String>,
}

/// offer の中身を mime の形で受け取る（貼る時と、ほかのアプリからファイルを引っ張ってきた時）。ほかのアプリが
/// 書き終える（書く口を閉じる）まで読む。止まったアプリで固まらないよう、待つのは 1 秒まで。（中身, 時間切れか）
fn receive_offer(conn: &Connection, offer: &wl_data_offer::WlDataOffer, mime: &str) -> Option<(Vec<u8>, bool)> {
    let t = Instant::now();
    let (read_end, write_end) = pipe_with(PipeFlags::CLOEXEC).ok()?;
    offer.receive(mime.to_string(), write_end.as_fd());
    drop(write_end);
    let _ = conn.flush();
    let mut bytes = Vec::new();
    let mut timed_out = false;
    loop {
        let left = PASTE_TIMEOUT.saturating_sub(t.elapsed());
        let ts = Timespec { tv_sec: left.as_secs() as _, tv_nsec: left.subsec_nanos() as _ };
        let mut fds = [PollFd::new(&read_end, PollFlags::IN)];
        match poll(&mut fds, Some(&ts)) {
            Ok(0) => {
                timed_out = true;
                break;
            }
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => break,
        }
        let mut chunk = [0u8; 8192];
        match rustix::io::read(&read_end, &mut chunk) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => break,
        }
    }
    Some((bytes, timed_out))
}

/// ほかのアプリから引っ張ってきている物（wl_data_device の enter から leave か drop まで）
struct Drag {
    /// 入ってきた窓
    win: WinId,
    offer: wl_data_offer::WlDataOffer,
    /// enter の番号（accept に添える）
    serial: u32,
    /// ファイル（text/uri-list）を持っていて、場所を受け取れた
    files: bool,
    /// 位置（物理ピクセル）
    at: (f32, f32),
    /// 最後に合成へ答えた「受け取れるか」
    accepting: Option<bool>,
}

/// OS のクリップボード（文字だけ）
struct WaylandClipboard(Rc<RefCell<Clip>>);

impl Clipboard for WaylandClipboard {
    fn set(&mut self, text: &str) {
        let mut c = self.0.borrow_mut();
        let (Some(manager), Some(device)) = (c.manager.clone(), c.device.clone()) else {
            log::event("clipboard.set", &format!("chars={}", text.chars().count()), "local only (no wl_data_device)");
            c.local = Some(text.to_string());
            return;
        };
        let source = manager.create_data_source(&c.qh, ());
        for mime in TEXT_MIMES {
            source.offer(mime.to_string());
        }
        device.set_selection(Some(&source), c.serial);
        if let Some((old, _)) = c.ours.replace((source, text.to_string())) {
            old.destroy();
        }
        let _ = c.conn.flush();
        log::event("clipboard.set", &format!("chars={} serial={}", text.chars().count(), c.serial), "selection");
    }

    fn get(&mut self) -> Option<String> {
        let c = self.0.borrow();
        // 自分が出している物は、合成を通さずに返す（自分あての送れの知らせを、読む間は扱えないため）
        if let Some((_, text)) = &c.ours {
            log::event("clipboard.get", &format!("chars={}", text.chars().count()), "own selection");
            return Some(text.clone());
        }
        let Some(offer) = c.offer.as_ref() else {
            log::event("clipboard.get", "", if c.local.is_some() { "local" } else { "empty" });
            return c.local.clone();
        };
        let mimes = offer.data::<Mutex<Vec<String>>>().map(|m| m.lock().map(|v| v.clone()).unwrap_or_default()).unwrap_or_default();
        let Some(mime) = TEXT_MIMES.into_iter().find(|t| mimes.iter().any(|m| m == t)) else {
            log::event("clipboard.get", &format!("mimes={mimes:?}"), "no text");
            return None;
        };
        let t = Instant::now();
        let (bytes, timed_out) = receive_offer(&c.conn, offer, mime)?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        log::event(
            "clipboard.get",
            &format!("mime={mime} chars={}", text.chars().count()),
            &format!("ms={:.1}{}", t.elapsed().as_secs_f64() * 1000.0, if timed_out { " timeout" } else { "" }),
        );
        Some(text)
    }
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

/// 共有メモリの絵 2 枚。合成が読んでいる（busy）間は描かない
struct Shm {
    pool: wl_shm_pool::WlShmPool,
    map: *mut u8,
    len: usize,
    _fd: OwnedFd,
    width: i32,
    height: i32,
    slots: [(wl_buffer::WlBuffer, bool); 2],
}

impl Shm {
    fn new(shm: &wl_shm::WlShm, qh: &QueueHandle<State>, width: i32, height: i32, win: WinId) -> Result<Shm, crate::Error> {
        let stride = width * 4;
        let one = (stride * height) as usize;
        let len = one * 2;
        let fd = memfd_create("kargui", MemfdFlags::CLOEXEC).map_err(|e| error("memfd_create", e))?;
        ftruncate(&fd, len as u64).map_err(|e| error("ftruncate", e))?;
        // SAFETY: 作ったばかりの memfd を len の大きさで写す。写した所はこの Shm が持ち、Drop で外す
        let map = unsafe { mmap(std::ptr::null_mut(), len, ProtFlags::READ | ProtFlags::WRITE, MapFlags::SHARED, &fd, 0) }
            .map_err(|e| error("mmap", e))? as *mut u8;
        let pool = shm.create_pool(fd.as_fd(), len as i32, qh, ());
        let slots = [0usize, 1].map(|i| (pool.create_buffer((one * i) as i32, width, height, stride, wl_shm::Format::Xrgb8888, qh, win), false));
        Ok(Shm { pool, map, len, _fd: fd, width, height, slots })
    }

    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| !s.1)
    }

    fn pixels(&mut self, i: usize) -> &mut [u32] {
        let one = (self.width * self.height) as usize;
        // SAFETY: map は 2 枚分（len）を写しており、i は 0 か 1。&mut self の間はほかから触らない
        unsafe { std::slice::from_raw_parts_mut((self.map as *mut u32).add(one * i), one) }
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        for (b, _) in &self.slots {
            b.destroy();
        }
        self.pool.destroy();
        // SAFETY: new で写した所を、同じ長さで外す
        unsafe {
            let _ = munmap(self.map as *mut _, self.len);
        }
    }
}

/// キー配列（libxkbcommon）と押し続け
struct Keyboard {
    xkb: &'static xkb::XkbCommon,
    ctx: *mut xkb::xkb_context,
    keymap: *mut xkb::xkb_keymap,
    state: *mut xkb::xkb_state,
    /// 押し続けの速さ（1 秒あたり）と、始まるまでの時間（ms）
    rate: i32,
    delay: i32,
    /// 押し続けているキー（xkb のキー番号）と、次に繰り返す時
    repeat: Option<(u32, Instant)>,
    /// 今の押し続けで繰り返した回数（記録用）
    repeat_count: u32,
}

impl Keyboard {
    fn new() -> Option<Keyboard> {
        let xkb = xkb::xkbcommon_option()?;
        // SAFETY: libxkbcommon の関数を、決まった引数で呼ぶ
        let ctx = unsafe { (xkb.xkb_context_new)(xkb::xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };
        if ctx.is_null() {
            return None;
        }
        Some(Keyboard { xkb, ctx, keymap: std::ptr::null_mut(), state: std::ptr::null_mut(), rate: 25, delay: 600, repeat: None, repeat_count: 0 })
    }

    /// 合成から届いたキー配列（文字列の fd）を読む
    fn set_keymap(&mut self, fd: OwnedFd, size: u32) -> bool {
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

    fn ready(&self) -> bool {
        !self.state.is_null()
    }

    fn update_mods(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
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

    fn mods(&self) -> Mods {
        Mods { shift: self.mod_active(xkb::XKB_MOD_NAME_SHIFT), ctrl: self.mod_active(xkb::XKB_MOD_NAME_CTRL), alt: self.mod_active(xkb::XKB_MOD_NAME_ALT) }
    }

    /// キーの番号（xkb）を、修飾キーを外した段 0 の記号にする（Windows の仮想キーと同じく、配列の文字ではなくキーで決める）
    fn base_sym(&self, code: u32) -> u32 {
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

    /// 今の修飾キーで打った文字
    fn utf8(&self, code: u32) -> String {
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

    fn repeats(&self, code: u32) -> bool {
        // SAFETY: 作った keymap にキーの番号を渡す
        self.ready() && self.rate > 0 && unsafe { (self.xkb.xkb_keymap_key_repeats)(self.keymap, code) } != 0
    }

    /// 押し続けをやめる（繰り返していたら、その回数を記録する）
    fn stop_repeat(&mut self, why: &str) {
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
fn key_of(sym: u32) -> Option<Key> {
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

/// 日本語入力（text-input-v3）の状態
#[derive(Default)]
struct Ime {
    /// 合成が、どれかの窓に文字の入力の焦点を置いている（enter から leave まで。どの窓かは State::ime_win）。
    /// Hyprland は作業場を移った時に text-input の leave を送らないので、キーボードの焦点（State::keyboard_win）
    /// が同じ窓にある時だけ有効にする
    entered: bool,
    /// 有効にした入力欄（None = 無効にしている）
    enabled: Option<Id>,
    /// 送った commit の数（done の serial と比べる）
    commits: u32,
    /// この数より前の serial の done は、有効・無効を切り替える前の変換の残り（こちらで確定した変換を IME が
    /// 確定し直した物など）なので捨てる
    stale_before: u32,
    /// 最後に知らせた文字を打つ位置（論理ピクセル）
    rect: Option<(i32, i32, i32, i32)>,
    /// done までにたまった、変換中の文字（強調の範囲。UTF-8 のバイト）と確定の文字と消す長さ
    preedit: Option<(String, i32, i32)>,
    commit: Option<String>,
    delete: (u32, u32),
    /// 今見せている変換中の文字
    shown: String,
}

/// 読み上げ（AT-SPI）からの頼みを受ける係。accesskit_unix は 3 つの係をどれも別のスレッドから呼ぶので、
/// 操作は列に積み、eventfd に書いて窓の処理（poll）を起こす。最初の木もここでは作らず（画面の状態は窓の処理の
/// スレッドにある）、起きた後のフレームで update_if_active に全部の木を渡す
#[derive(Clone)]
struct A11ySink {
    actions: Arc<Mutex<Vec<ActionRequest>>>,
    wake: Arc<OwnedFd>,
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

/// 窓の番号。面や xdg の窓の知らせに添えて、どの窓の物かを分ける
type WinId = u32;

/// 窓 1 つ分（主の窓と、Ui::window で開いた窓）。Windows の platform.rs と同じく、窓ごとに Runtime を持ち、
/// アプリは窓どうしで分け合う（State::app）
struct Win {
    id: WinId,
    /// None は主の窓
    key: Option<String>,
    rt: Runtime,
    surface: wl_surface::WlSurface,
    xdg_surface: xdg_surface::XdgSurface,
    toplevel: xdg_toplevel::XdgToplevel,
    viewport: Option<wp_viewport::WpViewport>,
    fractional: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    decoration: Option<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1>,
    /// 読み上げ。読み上げソフトが動いていない間、AccessKit は木を作らせない（update_if_active が何もしない）
    a11y: accesskit_unix::Adapter,
    a11y_actions: Arc<Mutex<Vec<ActionRequest>>>,
    buffers: Option<Shm>,
    /// CPU の道の絵（前のフレームの絵。変わった所だけ描き直す）と、その要約と背景の色
    pixels: Vec<u32>,
    summary: Summary,
    background: Color,
    /// 論理の大きさと、倍率（1/120 単位。fractional-scale が無ければ整数の倍率 × 120）
    logical: (i32, i32),
    scale120: u32,
    /// fractional-scale が無い時の整数の倍率（wl_surface の preferred_buffer_scale）
    int_scale: i32,
    configured: bool,
    pending_size: Option<(i32, i32)>,
    events: Vec<Event>,
    need_frame: bool,
    why: &'static str,
    /// 次は全部描き直す（大きさ・倍率が変わった）
    full: bool,
    animating: bool,
    frame_callback: bool,
    wake_at: Option<Instant>,
    focused: bool,
    shown: bool,
    /// 窓を作った時（主の窓はアプリの起動の時。出るまでの時間を記録に書く）
    created: Instant,
}

impl Drop for Win {
    fn drop(&mut self) {
        // 作った順の逆に壊す（バッファは Shm の Drop が壊す）
        self.buffers = None;
        if let Some(d) = self.decoration.take() {
            d.destroy();
        }
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
        self.toplevel.destroy();
        self.xdg_surface.destroy();
        self.surface.destroy();
    }
}

impl Win {
    fn scale(&self) -> f32 {
        self.scale120 as f32 / 120.0
    }

    fn physical(&self) -> (i32, i32) {
        let s = self.scale120 as i64;
        let f = |v: i32| ((v as i64 * s + 60) / 120).max(1) as i32;
        (f(self.logical.0), f(self.logical.1))
    }

    /// 記録に添える窓の名前（主の窓は空。ほかの窓は「 win=キー」）
    fn tag(&self) -> String {
        self.key.as_ref().map_or(String::new(), |k| format!(" win={k}"))
    }

    /// 論理の大きさか倍率が変わった
    fn apply_size(&mut self) {
        let (w, h) = self.physical();
        self.rt.resize(w as f32, h as f32, self.scale());
        self.summary = Summary::default();
        self.full = true;
        self.need_frame = true;
        self.why = "size";
        log::event("window.size", &format!("logical={}x{} scale={:.3}{}", self.logical.0, self.logical.1, self.scale(), self.tag()), &format!("physical={w}x{h}"));
    }

    fn push(&mut self, e: Event) {
        self.events.push(e);
        self.need_frame = true;
        self.why = "input";
    }

    /// 読み上げソフトから届いた操作を、kargui の入力に直す（Windows の platform.rs と同じ対応）
    fn apply_a11y_actions(&mut self) {
        let requests: Vec<ActionRequest> = self.a11y_actions.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for r in requests {
            let action = match r.action {
                accesskit::Action::Click => A11yAction::Click,
                accesskit::Action::Focus => A11yAction::Focus,
                accesskit::Action::Increment => A11yAction::Increment,
                accesskit::Action::Decrement => A11yAction::Decrement,
                other => {
                    log::event("a11y.action", &format!("action={other:?}"), "ignored");
                    continue;
                }
            };
            self.push(Event::A11y { target: Id(r.target_node.0), action });
        }
    }

    /// 描いて画面に出す（ビューは回した後）。出せたら true
    fn draw(&mut self, shm: &wl_shm::WlShm, qh: &QueueHandle<State>, why: &str, t_build: f64) -> bool {
        let t_draw = Instant::now();
        let (w, h) = self.physical();
        if self.buffers.as_ref().is_none_or(|b| b.width != w || b.height != h) {
            self.buffers = None;
            match Shm::new(shm, qh, w, h, self.id) {
                Ok(b) => self.buffers = Some(b),
                Err(e) => {
                    log::event("surface.init", &format!("size={w}x{h}{}", self.tag()), &format!("error={e}"));
                    return false;
                }
            }
            self.pixels = vec![0; (w * h) as usize];
            self.summary = Summary::default();
            self.full = true;
        }
        let Some(buffers) = self.buffers.as_mut() else { return false };
        let Some(slot) = buffers.free_slot() else {
            // 2 枚とも合成が読んでいる。どちらかが返ってきたら描く
            log::event("render", &format!("why={why}{}", self.tag()), "busy (both buffers in use)");
            self.need_frame = true;
            return false;
        };
        let bg = self.rt.cx.theme.background;
        let full = self.full || bg != self.background;
        let drawn = cpu_frame::draw(&mut self.pixels, w, h, &self.rt.draw, &self.rt.cx.text.atlas, bg, &self.summary, full);
        self.summary = drawn.summary;
        self.background = bg;
        self.full = false;
        // 2 枚を交互に使うので、写す絵はいつも全体（変わった所だけを合成に知らせる）
        buffers.pixels(slot).copy_from_slice(&self.pixels);
        buffers.slots[slot].1 = true;
        self.surface.attach(Some(&buffers.slots[slot].0), 0, 0);
        for r in &drawn.present {
            self.surface.damage_buffer(r.x, r.y, r.w, r.h);
        }
        match &self.viewport {
            Some(v) => v.set_destination(self.logical.0, self.logical.1),
            None => self.surface.set_buffer_scale(self.int_scale),
        }
        if self.animating && !self.frame_callback {
            self.surface.frame(qh, self.id);
            self.frame_callback = true;
        }
        self.surface.commit();
        let t_draw = t_draw.elapsed().as_secs_f64() * 1000.0;
        if !self.animating {
            log::event(
                "render",
                &format!("why={why} {} quads={} slot={slot}{}", drawn.mode, self.rt.draw.quads.len(), self.tag()),
                &format!("ms={:.2} draw_ms={t_draw:.2}", t_build + t_draw),
            );
        }
        true
    }
}

struct State {
    /// アプリ（どの窓も同じ物を使う）
    app: Box<dyn App>,
    qh: QueueHandle<State>,
    shm: wl_shm::WlShm,
    compositor: wl_compositor::WlCompositor,
    wm_base: xdg_wm_base::XdgWmBase,
    viewporter: Option<wp_viewporter::WpViewporter>,
    frac_mgr: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    deco_mgr: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1>,
    /// 開いている窓。0 番はいつも主の窓
    windows: Vec<Win>,
    next_id: WinId,
    /// アプリが持ってきた書体のファイル（ほかの窓にも読ませる）
    font_files: Vec<std::borrow::Cow<'static, [u8]>>,
    cursor_mgr: Option<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1>,
    cursor: Option<wp_cursor_shape_device_v1::WpCursorShapeDeviceV1>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    xkb: Option<Keyboard>,
    /// クリップボード（WaylandClipboard と分け合う）
    clip: Rc<RefCell<Clip>>,
    /// 日本語入力（合成が text-input-v3 を持っていなければ None）と、文字の入力の焦点がある窓
    text_input: Option<zwp_text_input_v3::ZwpTextInputV3>,
    ime: Ime,
    ime_win: Option<WinId>,
    /// ほかのスレッドから窓の処理を起こす eventfd
    wake: Arc<OwnedFd>,
    /// ファイルのダイアログ（ポータル）の答え（頼んだ窓, キー, ファイル）。開いている間は dialog_open
    dialog_tx: Sender<(WinId, String, Vec<PathBuf>)>,
    dialog_rx: Receiver<(WinId, String, Vec<PathBuf>)>,
    dialog_open: bool,
    /// 最後にダイアログで選んだファイルのフォルダ（この起動の間だけ覚える）
    last_folder: Option<PathBuf>,
    /// ほかのアプリから引っ張ってきている物
    drag: Option<Drag>,
    /// OS の明暗の設定の答え（Options::dark が無い時だけ聞く。届いたら None にする）
    dark_rx: Option<Receiver<Option<bool>>>,
    running: bool,
    /// ポインターとキーボードの焦点がある窓
    pointer_win: Option<WinId>,
    keyboard_win: Option<WinId>,
    // ポインター（その窓の物理ピクセル）
    px: f32,
    py: f32,
    left_down: bool,
    pointer_serial: u32,
    shape: Option<Shape>,
    /// 前に押した時（ms, x, y, 何回目か）
    last_press: Option<(u32, f32, f32, u8)>,
    /// ホイール・タッチパッドの 1 まとまり（wl_pointer.frame まで）の源と量。まとまりの中の順番は決まっていないので、
    /// frame でまとめて決める（源が量の後に来ても指の送りを捨てない）
    axis_source: Option<wl_pointer::AxisSource>,
    axis_value: f64,
    axis_value120: i32,
    started: Instant,
}

impl State {
    fn index(&self, id: WinId) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    fn win(&mut self, id: WinId) -> Option<&mut Win> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// 面から窓を探す（ポインター・キーボード・日本語入力・引っ張りの enter は面で届く）
    fn win_of(&self, surface: &wl_surface::WlSurface) -> Option<WinId> {
        self.windows.iter().find(|w| &w.surface == surface).map(|w| w.id)
    }

    fn pointer_index(&self) -> Option<usize> {
        self.pointer_win.and_then(|id| self.index(id))
    }

    fn pointer_scale(&self) -> f32 {
        self.pointer_index().map_or(1.0, |i| self.windows[i].scale())
    }

    /// 窓を作る（主の窓は run、ほかの窓は主の窓のフレームの後）。最初の configure が来てから描く
    fn new_window(&mut self, key: Option<String>, title: &str, size: (f32, f32), min_size: Option<(f32, f32)>, rt: Runtime) -> WinId {
        let id = self.next_id;
        self.next_id += 1;
        let qh = &self.qh;
        let surface = self.compositor.create_surface(qh, id);
        let xdg_surface = self.wm_base.get_xdg_surface(&surface, qh, id);
        let toplevel = xdg_surface.get_toplevel(qh, id);
        toplevel.set_title(title.to_string());
        toplevel.set_app_id(app_id());
        if let Some((w, h)) = min_size {
            toplevel.set_min_size(w.round() as i32, h.round() as i32);
        }
        // 合成が題の帯を描けるなら頼む（KDE・sway など。Hyprland と GNOME には無い）
        let decoration = self.deco_mgr.as_ref().map(|m| {
            let d = m.get_toplevel_decoration(&toplevel, qh, ());
            d.set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
            d
        });
        let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(&surface, qh, ()));
        let fractional = self.frac_mgr.as_ref().map(|m| m.get_fractional_scale(&surface, qh, id));
        surface.commit();
        // 読み上げ。AT-SPI の接続は accesskit_unix が自分のスレッドで行うので、起動を待たせない
        let a11y_actions = Arc::new(Mutex::new(Vec::new()));
        let sink = A11ySink { actions: a11y_actions.clone(), wake: self.wake.clone() };
        let a11y = accesskit_unix::Adapter::new(sink.clone(), sink.clone(), sink);
        let logical = (size.0.round().max(1.0) as i32, size.1.round().max(1.0) as i32);
        let mut w = Win {
            id,
            key,
            rt,
            surface,
            xdg_surface,
            toplevel,
            viewport,
            fractional,
            decoration,
            a11y,
            a11y_actions,
            buffers: None,
            pixels: Vec::new(),
            summary: Summary::default(),
            background: [0.0; 4],
            logical,
            scale120: 120,
            int_scale: 1,
            configured: false,
            pending_size: None,
            events: Vec::new(),
            need_frame: true,
            why: "first",
            full: true,
            animating: false,
            frame_callback: false,
            wake_at: None,
            focused: false,
            shown: false,
            created: if self.windows.is_empty() { self.started } else { Instant::now() },
        };
        w.apply_size();
        self.windows.push(w);
        id
    }

    /// 窓ごとの Runtime を作る（日付の書き方などは窓ごと。書体・明暗は主の窓と同じにする）
    fn runtime(&self, title: &str, dark: bool, font: &crate::Font) -> Result<Runtime, crate::Error> {
        let text = Text::new()?;
        let mut rt = Runtime::new(text, dark, Box::new(WaylandClipboard(self.clip.clone())), title.to_string());
        let writer = self.wake.clone();
        rt.cx.waker = crate::wake::Waker::new(move || {
            let _ = rustix::io::write(&*writer, &1u64.to_ne_bytes());
        });
        rt.cx.text.load_fonts(&self.font_files);
        rt.cx.font = rt.cx.text.fonts.id(font);
        Ok(rt)
    }

    /// Ui::window で頼まれた窓を開く。Wayland では、置く場所（position）と前面に出すか（activate）は合成が決める。
    /// 窓のアイコンも、まだ合成に渡していない
    fn open_window(&mut self, r: crate::ui::context::WindowRequest) {
        let main = &self.windows[0].rt;
        let (dark, font) = (main.cx.dark, main.cx.text.fonts.get(main.cx.font).clone());
        match self.runtime(&r.title, dark, &font) {
            Ok(rt) => {
                let id = self.new_window(Some(r.key.clone()), &r.title, r.size, r.min_size, rt);
                log::event("window.open_other", &format!("key={} size={:.0}x{:.0}", r.key, r.size.0, r.size.1), &format!("id={id}"));
            }
            Err(e) => log::event("window.open_other", &format!("key={}", r.key), &format!("error={e}")),
        }
    }

    fn close_window(&mut self, id: WinId) {
        let Some(i) = self.index(id) else { return };
        let w = self.windows.remove(i);
        log::event("window.close_other", &format!("key={}", w.key.as_deref().unwrap_or("")), "ok");
        drop(w);
        for slot in [&mut self.pointer_win, &mut self.keyboard_win, &mut self.ime_win] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        if self.drag.as_ref().is_some_and(|d| d.win == id)
            && let Some(d) = self.drag.take()
        {
            d.offer.destroy();
        }
    }

    /// 主の窓のフレームの後: 頼まれた窓と開いている窓をそろえる（Windows の platform.rs と同じ）
    fn sync_windows(&mut self, requested: Vec<crate::ui::context::WindowRequest>) {
        let new: Vec<_> = requested.iter().filter(|r| !self.windows.iter().any(|w| w.key.as_deref() == Some(r.key.as_str()))).cloned().collect();
        for r in new {
            self.open_window(r);
        }
        let gone: Vec<WinId> = self.windows.iter().filter(|w| w.key.as_ref().is_some_and(|k| !requested.iter().any(|r| &r.key == k))).map(|w| w.id).collect();
        for id in gone {
            self.close_window(id);
        }
    }

    /// ファイルのダイアログをポータルに頼む。ダイアログの間も窓は描き続けたいので、別のスレッドで待ち、答えは
    /// dialog_rx と eventfd で受け取る（Windows と同じく、取り消しやポータルが無い時は空の答え）
    fn open_file_dialog(&mut self, win: WinId, key: String, mut d: crate::file_dialog::FileDialog) {
        if self.dialog_open {
            log::event("file_dialog", &format!("key={key}"), "ignored (a dialog is already open)");
            return;
        }
        self.dialog_open = true;
        // アプリがフォルダを決めていなければ、最後に選んだファイルのフォルダから始める（Windows のダイアログは
        // アプリが最後に使ったフォルダから始まる。GTK のダイアログは何も渡さないとホームから始まる）
        if d.folder.is_none() {
            d.folder = self.last_folder.clone();
        }
        log::event(
            "file_dialog",
            &format!("key={key} save={} multiple={} filters={} folder={:?}", d.save, d.allows_multiple(), d.filters.len(), d.folder),
            "portal",
        );
        let (tx, wake) = (self.dialog_tx.clone(), self.wake.clone());
        let spawned = std::thread::Builder::new().name("kargui-file-dialog".into()).spawn(move || {
            let files = match crate::portal::choose_files(&d) {
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
        if let Err(e) = spawned {
            log::event("file_dialog.error", "spawn", &format!("error={e}"));
            self.dialog_open = false;
        }
    }

    /// 別のスレッドから届いた答え（ファイルのダイアログ・OS の明暗）を受け取る
    fn take_answers(&mut self) {
        while let Ok((win, key, files)) = self.dialog_rx.try_recv() {
            if let Some(dir) = files.first().and_then(|f| f.parent()) {
                self.last_folder = Some(dir.to_path_buf());
            }
            self.dialog_open = false;
            if let Some(w) = self.win(win) {
                w.rt.cx.file_results.insert(key, files);
                w.need_frame = true;
            }
        }
        if let Some(rx) = &self.dark_rx
            && let Ok(dark) = rx.try_recv()
        {
            self.dark_rx = None;
            if let Some(d) = dark {
                log::event("theme.system", &format!("dark={d}"), "late (after the first frame)");
                for w in &mut self.windows {
                    w.rt.cx.set_dark(d);
                    w.need_frame = true;
                }
            }
        }
    }

    /// ほかのアプリから引っ張ってきている物を受け取れるかを、合成に答える。Windows の IDropTarget と同じく、
    /// 前のフレームの配置でその場で決める（受け取れる時だけ text/uri-list と「写す」を返す）
    fn update_drag_accept(&mut self) {
        let Some(d) = self.drag.as_mut() else { return };
        let ok = d.files && self.windows.iter().find(|w| w.id == d.win).is_some_and(|w| w.rt.cx.accepts_files_at(d.at.0, d.at.1));
        if d.accepting == Some(ok) {
            return;
        }
        d.accepting = Some(ok);
        d.offer.accept(d.serial, ok.then(|| URI_LIST.to_string()));
        if d.offer.version() >= 3 {
            let action = if ok { wl_data_device_manager::DndAction::Copy } else { wl_data_device_manager::DndAction::empty() };
            d.offer.set_actions(action, action);
        }
        log::event("drag.accept", &format!("at={:.0},{:.0}", d.at.0, d.at.1), &format!("accept={ok}"));
    }

    fn mods(&self) -> Mods {
        self.xkb.as_ref().map_or(Mods::NONE, |k| k.mods())
    }

    fn set_shape(&mut self, shape: Shape) {
        if self.shape == Some(shape) {
            return;
        }
        if let Some(c) = &self.cursor {
            c.set_shape(self.pointer_serial, shape);
            self.shape = Some(shape);
            log::event("cursor.shape", &format!("x={} y={}", self.px, self.py), &format!("{shape:?}"));
        }
    }

    /// ポインターの下の部品から形を決める（Windows の WM_SETCURSOR と同じ）
    fn update_shape(&mut self) {
        let Some(i) = self.pointer_index() else { return };
        let (x, y) = (self.px, self.py);
        let cx = &self.windows[i].rt.cx;
        let hit = cx.hit(x, y).cloned();
        let shape = match hit.as_ref().and_then(|h| cx.prev.split_bars.get(&h.id).copied()) {
            Some(true) => Shape::NsResize,
            Some(false) => Shape::EwResize,
            None if hit.as_ref().is_some_and(|h| h.text_input) => Shape::Text,
            None if hit.as_ref().is_some_and(|h| h.link && h.enabled) => Shape::Pointer,
            None => Shape::Default,
        };
        self.set_shape(shape);
    }

    fn pointer_motion(&mut self, x: f32, y: f32) {
        self.px = x;
        self.py = y;
        let Some(i) = self.pointer_index() else { return };
        let left_down = self.left_down;
        let w = &mut self.windows[i];
        let cx = &w.rt.cx;
        let inner_drag = cx.dnd.as_ref().is_some_and(|d| !d.external);
        if cx.drag.is_some() || cx.pressed.is_some() || cx.press_source.is_some() || inner_drag {
            // つまみをつかんでいる・部品を押している・引っ張っている間は、移動を渡す（Windows の platform.rs と同じ）
            let mut shape = None;
            if left_down {
                w.push(Event::PointerMove { x, y });
                if let Some(ok) = w.rt.cx.drag_cursor(x, y) {
                    shape = Some(if ok { Shape::Default } else { Shape::NotAllowed });
                }
            } else if inner_drag || w.rt.cx.press_source.is_some() {
                log::event("drag.lost", &format!("x={x} y={y}"), "cancel");
                w.push(Event::PointerCancel);
            } else {
                w.push(Event::PointerUp { x, y });
            }
            if let Some(s) = shape {
                self.set_shape(s);
            }
            return;
        }
        let hover = w.rt.hover_at(x, y);
        let bar = w.rt.cx.scrollbar_at(x, y).map(|l| l.id);
        if hover != w.rt.cx.hover {
            log::event("hover", &format!("x={x} y={y}"), &format!("id={}", hover.map_or("none".to_string(), |h| format!("{:x}", h.0))));
        }
        if hover != w.rt.cx.hover || bar != w.rt.cx.bar_hover {
            w.rt.cx.set_hover(hover);
            w.rt.cx.bar_hover = bar;
            w.need_frame = true;
            w.why = "input";
        }
        self.update_shape();
    }

    fn key_down(&mut self, code: u32, repeat: bool) {
        let Some(kb) = self.xkb.as_ref() else { return };
        let mods = kb.mods();
        let key = key_of(kb.base_sym(code));
        // 文字は Ctrl と Alt の無い時だけ（Windows の WM_CHAR と同じく、ショートカットはキーとして受ける）
        let text = if mods.ctrl || mods.alt { String::new() } else { kb.utf8(code) };
        let Some(w) = self.keyboard_win.and_then(|id| self.windows.iter_mut().find(|w| w.id == id)) else { return };
        if let Some(key) = key {
            if !repeat {
                log::event("key", &format!("key={key:?} mods={mods:?}"), "sent");
            }
            w.push(Event::Key { key, mods });
        }
        for c in text.chars().filter(|c| !c.is_control()) {
            if !repeat {
                log::event("char", &format!("u=0x{:04X}", c as u32), "insert");
            }
            w.push(Event::Char(c));
        }
    }

    /// 窓 i で、焦点のある部品が入力欄なら IME を有効に、そうでなければ無効にする。文字を打つ位置が動いたら知らせる。
    /// kargui が変換中の文字を確定した時（入力欄の外を押したなど）は、無効にして有効にし直し、IME の変換を捨てさせる
    fn update_ime(&mut self, i: usize) {
        let id = self.windows[i].id;
        let cancel = std::mem::take(&mut self.windows[i].rt.cx.ime_cancel);
        let Some(ti) = self.text_input.clone() else { return };
        if !self.ime.entered || self.ime_win != Some(id) {
            return;
        }
        let keyboard = self.keyboard_win == Some(id);
        let w = &self.windows[i];
        let focus = w.rt.cx.focus.filter(|f| keyboard && w.rt.cx.prev.inputs.contains_key(f));
        let s = w.scale();
        let rect = w.rt.cx.caret.map(|c| ((c.x / s).floor() as i32, (c.y / s).floor() as i32, (c.w / s).ceil().max(1.0) as i32, (c.h / s).ceil().max(1.0) as i32));
        if focus != self.ime.enabled || (cancel && focus.is_some()) {
            let was = self.ime.enabled;
            if was.is_some() {
                ti.disable();
                ti.commit();
                self.ime.commits += 1;
            }
            if focus.is_some() {
                ti.enable();
                ti.set_content_type(zwp_text_input_v3::ContentHint::empty(), zwp_text_input_v3::ContentPurpose::Normal);
                if let Some((x, y, w, h)) = rect {
                    ti.set_cursor_rectangle(x, y, w, h);
                }
                ti.commit();
                self.ime.commits += 1;
            }
            self.ime.stale_before = self.ime.commits;
            self.ime.enabled = focus;
            self.ime.rect = rect;
            let why = if cancel { "cancel" } else { "focus" };
            let fmt = |f: Option<Id>| f.map_or("none".to_string(), |f| format!("{:x}", f.0));
            log::event("ime.enable", &format!("why={why} from={} to={} rect={rect:?}{}", fmt(was), fmt(focus), self.windows[i].tag()), &format!("commits={}", self.ime.commits));
        } else if focus.is_some()
            && rect != self.ime.rect
            && let Some((x, y, w, h)) = rect
        {
            ti.set_cursor_rectangle(x, y, w, h);
            ti.commit();
            self.ime.commits += 1;
            self.ime.rect = rect;
            log::event("ime.rect", &format!("x={x} y={y} w={w} h={h}"), &format!("commits={}", self.ime.commits));
        }
    }

    /// 変換の途中で窓を離れたら、見せている変換中の文字を確定する（Windows の IME と同じ。fcitx5 は捨ててしまう）。
    /// IME がこの後に送る前の変換の残りは、次に有効・無効を切り替えるまで捨てる
    fn commit_shown(&mut self, why: &str) {
        let shown = std::mem::take(&mut self.ime.shown);
        if shown.is_empty() {
            return;
        }
        self.ime.stale_before = self.ime.commits + 1;
        log::event("ime.commit_on_leave", &format!("why={why} text={shown}"), "sent");
        if let Some(w) = self.ime_win.and_then(|id| self.windows.iter_mut().find(|w| w.id == id)) {
            w.push(Event::ImePreedit(String::new()));
            w.push(Event::ImeCommit(shown));
        }
    }

    /// text-input-v3 の done: たまった変換中の文字と確定の文字を、文字の入力の焦点がある窓の入力にする
    fn ime_done(&mut self, serial: u32) {
        let (preedit, begin, end) = self.ime.preedit.take().unwrap_or_default();
        let commit = self.ime.commit.take().unwrap_or_default();
        let delete = std::mem::take(&mut self.ime.delete);
        let what = format!("serial={serial} commits={} preedit={preedit:?} range={begin}..{end} commit={commit:?}", self.ime.commits);
        if serial < self.ime.stale_before {
            log::event("ime.done", &what, "dropped (from before enable/disable)");
            return;
        }
        if delete != (0, 0) {
            // 前後の文字を送っていないので、IME が消す長さを頼むことは無いはず
            log::event("ime.delete_surrounding", &format!("before={} after={}", delete.0, delete.1), "ignored");
        }
        let events = ime::text_input_done(&self.ime.shown, &preedit, begin, end, &commit);
        let clauses = events.iter().find_map(|e| match e {
            Event::ImeClauses { bounds, target, cursor } => Some(format!(" clauses={bounds:?} target={target:?} cursor={cursor:?}")),
            _ => None,
        });
        log::event("ime.done", &what, &format!("events={}{}", events.len(), clauses.unwrap_or_default()));
        self.ime.shown = preedit;
        if let Some(w) = self.ime_win.and_then(|id| self.windows.iter_mut().find(|w| w.id == id)) {
            for e in events {
                w.push(e);
            }
        }
    }

    /// 窓 i の 1 フレーム: 入力を渡してビューを回し、描いて画面に出す
    fn frame(&mut self, i: usize) {
        if !self.windows[i].configured {
            return;
        }
        let t = Instant::now();
        let app = &mut self.app;
        let w = &mut self.windows[i];
        let why = w.why;
        let events = std::mem::take(&mut w.events);
        let out = match w.key.clone() {
            None => w.rt.frame(&mut **app, events),
            Some(k) => w.rt.frame_with(&mut |ui: &mut crate::ui::Ui| app.window(&k, ui), events),
        };
        let (id, is_main) = (w.id, w.key.is_none());
        let quit = std::mem::take(&mut w.rt.cx.quit);
        let dialog = w.rt.cx.file_dialog_request.take();
        let requested = is_main.then(|| w.rt.cx.windows.clone());
        w.wake_at = out.wake_after.map(|d| Instant::now() + d);
        w.animating = out.want_frame;
        w.need_frame = false;
        if quit {
            log::event("window.quit", "", "ok");
            self.running = false;
        }
        if let Some((key, d)) = dialog {
            self.open_file_dialog(id, key, d);
        }
        // 文字を打つ位置は、絵と同じ wl_surface.commit で合成に届くように、絵より先に知らせる
        self.update_ime(i);
        // 読み上げの木（読み上げソフトが聞いている時だけ作る。2 つの絵が両方使われていて描けない時も渡す）
        let w = &mut self.windows[i];
        let rt = &mut w.rt;
        w.a11y.update_if_active(|| rt.a11y().0);
        let t_build = t.elapsed().as_secs_f64() * 1000.0;
        let drawn = w.draw(&self.shm, &self.qh, why, t_build);
        if drawn {
            let w = &mut self.windows[i];
            let first = !w.shown;
            w.shown = true;
            if first {
                let (pw, ph) = w.physical();
                log::event("window.shown", &format!("physical={pw}x{ph} scale={:.3}{}", w.scale(), w.tag()), &format!("ms={:.1}", w.created.elapsed().as_secs_f64() * 1000.0));
            }
            if is_main {
                let number = if first { 0 } else { w.rt.frames - 1 };
                self.app.presented(&Frame { number, since_start: self.started.elapsed() });
            }
        }
        // 入力で描いたら、ほかの窓も描き直す（アプリを共有するため。描き直しはそれ以上広げない）
        if why == "input" {
            for o in self.windows.iter_mut().filter(|o| o.id != id && !o.need_frame) {
                o.need_frame = true;
                o.why = "sync";
            }
        }
        // 主の窓: 頼まれた窓と開いている窓をそろえる
        if let Some(requested) = requested {
            self.sync_windows(requested);
        }
    }
}

/// 窓の app_id（Hyprland の窓の規則などで使う）は、実行ファイルの名前
fn app_id() -> String {
    std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "kargui".into())
}

pub(crate) fn run(options: Options, app: Box<dyn App>) -> Result<(), crate::Error> {
    let started = Instant::now();
    // ほかのスレッドから起こされたら、eventfd に書いて poll を起こす
    let wake_fd = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).map_err(|e| error("eventfd", e))?);
    // OS の明暗の設定は、窓の準備と並べてポータルに聞く（最初の絵の前に少しだけ待つ）
    let dark_rx = options.dark.is_none().then(|| {
        let (tx, rx) = channel();
        let wake = wake_fd.clone();
        let _ = std::thread::Builder::new().name("kargui-portal-settings".into()).spawn(move || {
            let t = Instant::now();
            let r = crate::portal::read_dark();
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            match &r {
                Ok(d) => log::event("portal.color_scheme", "", &format!("dark={d} ms={ms:.1}")),
                Err(e) => log::event("portal.color_scheme", "", &format!("error={e} ms={ms:.1}")),
            }
            let _ = tx.send(r.ok());
            let _ = rustix::io::write(&*wake, &1u64.to_ne_bytes());
        });
        rx
    });
    let conn = Connection::connect_to_env().map_err(|e| error("connect (WAYLAND_DISPLAY)", e))?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| error("registry", e))?;
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=6, ()).map_err(|e| error("wl_compositor", e))?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).map_err(|e| error("wl_shm", e))?;
    let wm_base: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).map_err(|e| error("xdg_wm_base", e))?;
    let seat: Option<wl_seat::WlSeat> = globals.bind(&qh, 5..=8, ()).ok();
    let viewporter: Option<wp_viewporter::WpViewporter> = globals.bind(&qh, 1..=1, ()).ok();
    let frac_mgr: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let cursor_mgr: Option<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let deco_mgr: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let data_mgr: Option<wl_data_device_manager::WlDataDeviceManager> = globals.bind(&qh, 1..=3, ()).ok();
    let ti_mgr: Option<zwp_text_input_manager_v3::ZwpTextInputManagerV3> = globals.bind(&qh, 1..=1, ()).ok();
    log::event(
        "wayland.globals",
        &format!(
            "seat={} viewporter={} fractional_scale={} cursor_shape={} decoration={} data_device={} text_input={}",
            seat.is_some(),
            viewporter.is_some(),
            frac_mgr.is_some(),
            cursor_mgr.is_some(),
            deco_mgr.is_some(),
            data_mgr.is_some(),
            ti_mgr.is_some()
        ),
        &format!("ms={:.1}", started.elapsed().as_secs_f64() * 1000.0),
    );

    // クリップボード（座席ごとに 1 つの data device）
    let device = match (&data_mgr, &seat) {
        (Some(m), Some(s)) => Some(m.get_data_device(s, &qh, ())),
        _ => None,
    };
    let clip = Rc::new(RefCell::new(Clip { conn: conn.clone(), qh: qh.clone(), manager: data_mgr, device, serial: 0, ours: None, offer: None, local: None }));
    // 日本語入力（座席ごとに 1 つ。焦点が入力欄にある間だけ有効にする）
    let text_input = match (&ti_mgr, &seat) {
        (Some(m), Some(s)) => Some(m.get_text_input(s, &qh, ())),
        _ => None,
    };
    let (dialog_tx, dialog_rx) = channel();
    let mut st = State {
        app,
        qh: qh.clone(),
        shm,
        compositor,
        wm_base,
        viewporter,
        frac_mgr,
        deco_mgr,
        windows: Vec::new(),
        next_id: 1,
        font_files: options.fonts.clone(),
        cursor_mgr,
        cursor: None,
        pointer: None,
        keyboard: None,
        xkb: Keyboard::new(),
        clip,
        text_input,
        ime: Ime::default(),
        ime_win: None,
        wake: wake_fd.clone(),
        dialog_tx,
        dialog_rx,
        dialog_open: false,
        last_folder: None,
        drag: None,
        dark_rx,
        running: true,
        pointer_win: None,
        keyboard_win: None,
        px: 0.0,
        py: 0.0,
        left_down: false,
        pointer_serial: 0,
        shape: None,
        last_press: None,
        axis_source: None,
        axis_value: 0.0,
        axis_value120: 0,
        started,
    };
    if st.xkb.is_none() {
        log::event("keyboard.xkb", "", "error=libxkbcommon.so could not be loaded (keys will not work)");
    }
    // 主の窓
    let rt = st.runtime(&options.title, options.dark.unwrap_or(false), &options.font)?;
    let local = local_date().zip(local_time()).map_or("unknown".to_string(), |((y, m, d), (h, min))| format!("{y}-{m:02}-{d:02} {h:02}:{min:02}"));
    log::event("window.create", &format!("title={:?} size={:.0}x{:.0} app_id={} local={local}", options.title, options.size.0, options.size.1, app_id()), "ok");
    st.new_window(None, &options.title, options.size, options.min_size, rt);
    // 最初の configure（と、出せれば倍率）を待つ
    queue.roundtrip(&mut st).map_err(|e| error("roundtrip", e))?;
    // OS の明暗の答えを、最初の絵の前に 150 ms まで待つ。間に合わなければ明るい方で描き、届いた時に切り替える
    let answer = st.dark_rx.as_ref().map(|rx| rx.recv_timeout(Duration::from_millis(150)));
    match answer {
        Some(Ok(dark)) => {
            st.dark_rx = None;
            if let Some(d) = dark {
                st.windows[0].rt.cx.set_dark(d);
            }
            log::event("theme.system", &format!("dark={dark:?}"), &format!("waited_ms={:.1}", started.elapsed().as_secs_f64() * 1000.0));
        }
        Some(Err(_)) => log::event("theme.system", "", "not answered in 150 ms (light until it is)"),
        None => {}
    }

    let _keep = (seat, ti_mgr);
    while st.running {
        queue.dispatch_pending(&mut st).map_err(|e| error("dispatch", e))?;
        let now = Instant::now();
        for w in &mut st.windows {
            if w.wake_at.is_some_and(|t| t <= now) {
                w.wake_at = None;
                log::event("timer", &w.tag(), "frame");
                w.need_frame = true;
                w.why = "timer";
            }
        }
        let due = st.xkb.as_ref().and_then(|k| k.repeat).filter(|(_, t)| *t <= now);
        if let Some((code, _)) = due {
            st.key_down(code, true);
            if let Some(k) = st.xkb.as_mut() {
                let gap = Duration::from_millis((1000 / k.rate.max(1)) as u64);
                k.repeat = Some((code, now + gap));
                if k.repeat_count == 0 {
                    log::event("key.repeat", &format!("code={code} rate={} delay={}", k.rate, k.delay), "started");
                }
                k.repeat_count += 1;
            }
        }
        // 窓ごとに描く（主の窓のフレームが窓を開け閉めするので、番号でたどる）
        let ids: Vec<WinId> = st.windows.iter().map(|w| w.id).collect();
        for id in ids {
            if let Some(i) = st.index(id)
                && st.windows[i].need_frame
                && st.windows[i].configured
            {
                st.frame(i);
            }
            if !st.running {
                break;
            }
        }
        if !st.running {
            break;
        }
        if let Err(e) = conn.flush() {
            log::event("wayland.flush", "", &format!("error={e}"));
        }
        // 次に起きる時（タイマー・押し続け）。アニメーション中は合成のフレームの知らせで起きる
        let deadline = st.windows.iter().filter_map(|w| w.wake_at).chain(st.xkb.as_ref().and_then(|k| k.repeat.map(|r| r.1))).min();
        let timeout = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        let Some(guard) = queue.prepare_read() else { continue };
        let (conn_ready, woke) = {
            let cfd = guard.connection_fd();
            let mut fds = [PollFd::new(&cfd, PollFlags::IN), PollFd::new(&*wake_fd, PollFlags::IN)];
            let ts = timeout.map(|d| Timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos() as _ });
            match poll(&mut fds, ts.as_ref()) {
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(e) => return Err(error("poll", e)),
            }
            let conn_ready = fds[0].revents().intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP);
            (conn_ready, fds[1].revents().contains(PollFlags::IN))
        };
        if conn_ready {
            if let Err(e) = guard.read()
                && !matches!(e, wayland_client::backend::WaylandError::Io(ref io) if io.kind() == std::io::ErrorKind::WouldBlock)
            {
                return Err(error("read", e));
            }
        } else {
            drop(guard);
        }
        if woke {
            let mut buf = [0u8; 8];
            let _ = rustix::io::read(&*wake_fd, &mut buf);
            log::event("wake", "from=waker", "frame");
            // 読み上げソフトの操作・ファイルのダイアログ・明暗の答えも同じ eventfd で起きる
            for w in &mut st.windows {
                w.rt.cx.waker.take_woken();
                w.need_frame = true;
                w.why = "wake";
                w.apply_a11y_actions();
            }
            st.take_answers();
        }
    }
    log::event("loop.end", "", "ok");
    // 窓は主の窓から順に壊す
    st.windows.clear();
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_noop!(State: wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: wl_shm_pool::WlShmPool);
delegate_noop!(State: wp_viewporter::WpViewporter);
delegate_noop!(State: wp_viewport::WpViewport);
delegate_noop!(State: wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(State: wp_cursor_shape_manager_v1::WpCursorShapeManagerV1);
delegate_noop!(State: wp_cursor_shape_device_v1::WpCursorShapeDeviceV1);
delegate_noop!(State: zxdg_decoration_manager_v1::ZxdgDecorationManagerV1);
delegate_noop!(State: ignore zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(_: &mut Self, base: &xdg_wm_base::XdgWmBase, event: xdg_wm_base::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, WinId> for State {
    fn event(st: &mut Self, xs: &xdg_surface::XdgSurface, event: xdg_surface::Event, id: &WinId, _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_surface::Event::Configure { serial } = event {
            xs.ack_configure(serial);
            let Some(w) = st.win(*id) else { return };
            let first = !w.configured;
            w.configured = true;
            if let Some(size) = w.pending_size.take()
                && size != w.logical
            {
                w.logical = size;
                w.apply_size();
            }
            if first {
                log::event("window.configure", &format!("logical={}x{}{}", w.logical.0, w.logical.1, w.tag()), "first");
            }
            w.need_frame = true;
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, WinId> for State {
    fn event(st: &mut Self, _: &xdg_toplevel::XdgToplevel, event: xdg_toplevel::Event, id: &WinId, _: &Connection, _: &QueueHandle<Self>) {
        match event {
            xdg_toplevel::Event::Configure { width, height, states } => {
                let Some(w) = st.win(*id) else { return };
                // 状態は u32 の並び（1 最大化・2 全画面・4 前面・5〜8 敷き詰めの辺）
                let states: Vec<u32> = states.as_chunks::<4>().0.iter().map(|c| u32::from_ne_bytes(*c)).collect();
                let has = |s: xdg_toplevel::State| states.contains(&(s as u32));
                use xdg_toplevel::State as S;
                let constrained = [S::Maximized, S::Fullscreen, S::TiledLeft, S::TiledRight, S::TiledTop, S::TiledBottom].into_iter().any(has);
                log::event("toplevel.configure", &format!("size={width}x{height} states={states:?} configured={}{}", w.configured, w.tag()), &format!("constrained={constrained}"));
                // 最大化・全画面・敷き詰めの時は、言われた大きさに従う。浮いた窓の最初の大きさは勧めにすぎないので、
                // アプリが頼んだ大きさ（Options::size・Ui::window の大きさ）を使う（Hyprland は浮いた窓に画面いっぱいを
                // 勧めてくる）。2 回目からは、利用者が窓の大きさを変えた結果なので従う
                if width > 0 && height > 0 && (constrained || w.configured) {
                    w.pending_size = Some((width, height));
                } else if width > 0 && height > 0 {
                    log::event("window.configure", &format!("suggested={width}x{height}{}", w.tag()), "ignored (floating, first configure: use the requested size)");
                }
                let activated = has(S::Activated);
                if activated != w.focused {
                    w.focused = activated;
                    log::event("window.focus", &format!("focused={activated}{}", w.tag()), "ok");
                    w.a11y.update_window_focus_state(activated);
                    w.need_frame = true;
                }
            }
            xdg_toplevel::Event::Close => {
                let key = st.win(*id).and_then(|w| w.key.clone());
                match key {
                    // 主の窓: アプリに聞いてから終える
                    None => {
                        let ok = st.app.close_requested();
                        log::event("window.close_requested", "", &format!("allowed={ok}"));
                        if ok {
                            st.running = false;
                        } else if let Some(w) = st.windows.first_mut() {
                            w.need_frame = true;
                        }
                    }
                    // ほかの窓: 閉じるボタンは、Ui::window の open を false にする（窓は主の窓のフレームの後で閉じる）
                    Some(key) => {
                        log::event("window.close_requested", &format!("key={key}"), "closed by the user");
                        if let Some(main) = st.windows.first_mut() {
                            main.rt.cx.closed_windows.insert(key);
                            main.need_frame = true;
                            main.why = "input";
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_surface::WlSurface, WinId> for State {
    fn event(st: &mut Self, _: &wl_surface::WlSurface, event: wl_surface::Event, id: &WinId, _: &Connection, _: &QueueHandle<Self>) {
        // fractional-scale が無い合成では、整数の倍率で描く
        if let wl_surface::Event::PreferredBufferScale { factor } = event
            && let Some(w) = st.win(*id)
            && w.viewport.is_none()
            && factor != w.int_scale
        {
            w.int_scale = factor.max(1);
            w.scale120 = w.int_scale as u32 * 120;
            w.apply_size();
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, WinId> for State {
    fn event(st: &mut Self, _: &wp_fractional_scale_v1::WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, id: &WinId, _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event
            && let Some(w) = st.win(*id)
            && scale != w.scale120
            && w.viewport.is_some()
        {
            w.scale120 = scale.max(1);
            w.apply_size();
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, WinId> for State {
    fn event(st: &mut Self, buffer: &wl_buffer::WlBuffer, event: wl_buffer::Event, id: &WinId, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = event
            && let Some(b) = st.win(*id).and_then(|w| w.buffers.as_mut())
        {
            for slot in b.slots.iter_mut().filter(|s| &s.0 == buffer) {
                slot.1 = false;
            }
        }
    }
}

impl Dispatch<wl_callback::WlCallback, WinId> for State {
    fn event(st: &mut Self, _: &wl_callback::WlCallback, event: wl_callback::Event, id: &WinId, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = event
            && let Some(w) = st.win(*id)
        {
            w.frame_callback = false;
            if w.animating {
                w.need_frame = true;
                w.why = "animation";
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(st: &mut Self, seat: &wl_seat::WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event {
            if caps.contains(wl_seat::Capability::Pointer) && st.pointer.is_none() {
                let p = seat.get_pointer(qh, ());
                st.cursor = st.cursor_mgr.as_ref().map(|m| m.get_pointer(&p, qh, ()));
                st.pointer = Some(p);
            }
            if caps.contains(wl_seat::Capability::Keyboard) && st.keyboard.is_none() {
                st.keyboard = Some(seat.get_keyboard(qh, ()));
            }
            log::event("seat.capabilities", &format!("pointer={} keyboard={}", st.pointer.is_some(), st.keyboard.is_some()), "ok");
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(st: &mut Self, _: &wl_pointer::WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_pointer::Event::Enter { serial, surface, surface_x, surface_y } => {
                st.pointer_win = st.win_of(&surface);
                st.pointer_serial = serial;
                st.shape = None;
                let s = st.pointer_scale();
                st.pointer_motion(surface_x as f32 * s, surface_y as f32 * s);
                // 押していない時は形を決める（入ってきた時に決めないと、合成の前の形のまま）
                st.update_shape();
            }
            wl_pointer::Event::Leave { surface, .. } => {
                if let Some(w) = st.win_of(&surface).and_then(|id| st.win(id)) {
                    let had = w.rt.cx.hover.is_some();
                    let bar = w.rt.cx.bar_hover.take().is_some();
                    log::event("pointer.leave", &w.tag(), &format!("had_hover={had}"));
                    w.rt.cx.set_hover(None);
                    if had || bar {
                        w.need_frame = true;
                        w.why = "input";
                    }
                }
                if st.pointer_win == st.win_of(&surface) {
                    st.pointer_win = None;
                }
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                let s = st.pointer_scale();
                st.pointer_motion(surface_x as f32 * s, surface_y as f32 * s);
            }
            wl_pointer::Event::Button { serial, time, button, state } => {
                let pressed = state == WEnum::Value(wl_pointer::ButtonState::Pressed);
                st.clip.borrow_mut().serial = serial;
                let (x, y) = (st.px, st.py);
                let s = st.pointer_scale();
                if button != BTN_LEFT {
                    log::event("button", &format!("button=0x{button:x} pressed={pressed} x={x} y={y}"), "ok");
                }
                let shift = st.mods().shift;
                let last_press = st.last_press;
                let Some(i) = st.pointer_index() else { return };
                match button {
                    BTN_LEFT if pressed => {
                        st.left_down = true;
                        let w = &mut st.windows[i];
                        let target = w.rt.cx.hit(x, y).map(|h| h.id);
                        // 続けて押した何回目か（400 ms の中で、4 論理ピクセルの中を押したら続き。3 回で止める）
                        let d = DOUBLE_CLICK_DIST * s;
                        let count = match last_press {
                            Some((t0, x0, y0, n)) if time.wrapping_sub(t0) <= DOUBLE_CLICK_MS && (x - x0).abs() <= d && (y - y0).abs() <= d => (n % 3) + 1,
                            _ => 1,
                        };
                        log::event("click", &format!("x={x} y={y} count={count}{}", w.tag()), &format!("target={target:?}"));
                        w.push(Event::PointerDown { x, y, shift, count });
                        st.last_press = Some((time, x, y, count));
                    }
                    BTN_LEFT => {
                        st.left_down = false;
                        st.windows[i].push(Event::PointerUp { x, y });
                        st.shape = None;
                    }
                    // 右クリックは離した時（Windows の WM_CONTEXTMENU と同じ）
                    BTN_RIGHT if !pressed => {
                        log::event("context_menu", &format!("x={x} y={y}"), "sent");
                        st.windows[i].push(Event::ContextMenu { x, y });
                    }
                    _ => {}
                }
            }
            wl_pointer::Event::AxisSource { axis_source: WEnum::Value(source) } => {
                st.axis_source = Some(source);
            }
            // マウスのホイール（1 刻みが 120）
            wl_pointer::Event::AxisValue120 { axis: WEnum::Value(wl_pointer::Axis::VerticalScroll), value120 } => {
                st.axis_value120 += value120;
            }
            // なめらかな量（論理ピクセル）。ホイールでも送られる（その時は value120 を使う）
            wl_pointer::Event::Axis { axis: WEnum::Value(wl_pointer::Axis::VerticalScroll), value, .. } => {
                st.axis_value += value;
            }
            // 1 まとまりの終わり: ホイールは刻み（1 刻み 96）、タッチパッドはなめらかな量を物理ピクセルにして渡す
            wl_pointer::Event::Frame => {
                let source = st.axis_source.take();
                let (v120, value) = (std::mem::take(&mut st.axis_value120), std::mem::take(&mut st.axis_value));
                let s = st.pointer_scale();
                let dy = if v120 != 0 { v120 as f32 / 120.0 * WHEEL_STEP } else { value as f32 * s };
                if dy != 0.0
                    && let Some(i) = st.pointer_index()
                {
                    let (x, y) = (st.px, st.py);
                    log::event("wheel", &format!("source={source:?} value120={v120} value={value:.2} x={x} y={y}"), &format!("dy={dy:.1}"));
                    st.windows[i].push(Event::Wheel { x, y, dy });
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(st: &mut Self, _: &wl_keyboard::WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_keyboard::Event::Keymap { format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1), fd, size } => {
                let ok = st.xkb.as_mut().is_some_and(|k| k.set_keymap(fd, size));
                log::event("keyboard.keymap", &format!("size={size}"), if ok { "ok" } else { "error" });
            }
            wl_keyboard::Event::Enter { serial, surface, .. } => {
                st.clip.borrow_mut().serial = serial;
                st.keyboard_win = st.win_of(&surface);
                if let Some(w) = st.keyboard_win.and_then(|id| st.win(id)) {
                    w.need_frame = true;
                    log::event("keyboard.enter", &w.tag(), "ok");
                }
            }
            wl_keyboard::Event::Leave { surface, .. } => {
                if let Some(k) = st.xkb.as_mut() {
                    k.stop_repeat("leave");
                }
                if let Some(w) = st.win_of(&surface).and_then(|id| st.win(id)) {
                    w.need_frame = true;
                    log::event("keyboard.leave", &w.tag(), "ok");
                }
                if st.keyboard_win == st.win_of(&surface) {
                    st.keyboard_win = None;
                }
                st.commit_shown("keyboard.leave");
            }
            wl_keyboard::Event::Modifiers { mods_depressed, mods_latched, mods_locked, group, .. } => {
                if let Some(k) = st.xkb.as_mut() {
                    k.update_mods(mods_depressed, mods_latched, mods_locked, group);
                }
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                if let Some(k) = st.xkb.as_mut() {
                    k.rate = rate;
                    k.delay = delay;
                }
                log::event("keyboard.repeat_info", &format!("rate={rate} delay={delay}"), "ok");
            }
            wl_keyboard::Event::Key { serial, key, state, .. } => {
                st.clip.borrow_mut().serial = serial;
                // xkb のキーの番号は、evdev の番号 + 8
                let code = key + 8;
                if state == WEnum::Value(wl_keyboard::KeyState::Pressed) {
                    st.key_down(code, false);
                    // 押し続けは kargui が繰り返す（Hyprland のように合成が押した知らせを繰り返して送る時は、知らせの
                    // たびに待ちがやり直されるので、二重には繰り返さない）
                    if let Some(k) = st.xkb.as_mut() {
                        k.stop_repeat("other key");
                        k.repeat = k.repeats(code).then(|| (code, Instant::now() + Duration::from_millis(k.delay.max(0) as u64)));
                    }
                } else if let Some(k) = st.xkb.as_mut()
                    && k.repeat.is_some_and(|r| r.0 == code)
                {
                    k.stop_repeat("released");
                }
            }
            _ => {}
        }
    }
}

delegate_noop!(State: wl_data_device_manager::WlDataDeviceManager);
delegate_noop!(State: zwp_text_input_manager_v3::ZwpTextInputManagerV3);

impl Dispatch<zwp_text_input_v3::ZwpTextInputV3, ()> for State {
    fn event(st: &mut Self, _: &zwp_text_input_v3::ZwpTextInputV3, event: zwp_text_input_v3::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use zwp_text_input_v3::Event as E;
        match event {
            // 文字の入力の焦点が窓に来た（キーボードの焦点と一緒に動く）。次のフレームで、入力欄なら有効にする
            E::Enter { surface } => {
                let Some(id) = st.win_of(&surface) else { return };
                st.ime.entered = true;
                st.ime_win = Some(id);
                st.ime.enabled = None;
                st.ime.rect = None;
                if let Some(w) = st.win(id) {
                    w.need_frame = true;
                    w.why = "input";
                    log::event("ime.enter", &w.tag(), "ok");
                }
            }
            // 焦点が離れた。合成はこの後の頼みを聞かないので、有効・無効は次の enter でやり直す。
            // 変換中の文字は、キーボードの焦点が離れた時と同じく確定する
            E::Leave { .. } => {
                st.ime.entered = false;
                st.ime.enabled = None;
                st.ime.rect = None;
                st.ime.preedit = None;
                st.ime.commit = None;
                st.commit_shown("ime.leave");
                st.ime_win = None;
                log::event("ime.leave", "", "ok");
            }
            E::PreeditString { text, cursor_begin, cursor_end } => {
                st.ime.preedit = Some((text.unwrap_or_default(), cursor_begin, cursor_end));
            }
            E::CommitString { text } => {
                st.ime.commit = Some(text.unwrap_or_default());
            }
            E::DeleteSurroundingText { before_length, after_length } => {
                st.ime.delete = (before_length, after_length);
            }
            E::Done { serial } => st.ime_done(serial),
            _ => {}
        }
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for State {
    fn event(st: &mut Self, _: &wl_data_device::WlDataDevice, event: wl_data_device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            // 選択が変わった（ほかのアプリが写した、自分が写した、空になった）。前の offer は片付ける
            wl_data_device::Event::Selection { id } => {
                let mut c = st.clip.borrow_mut();
                let mimes = id.as_ref().and_then(|o| o.data::<Mutex<Vec<String>>>()).map(|m| m.lock().map(|v| v.len()).unwrap_or(0)).unwrap_or(0);
                if let Some(old) = std::mem::replace(&mut c.offer, id) {
                    old.destroy();
                }
                log::event("clipboard.selection", &format!("mimes={mimes} ours={}", c.ours.is_some()), if c.offer.is_some() { "offer" } else { "empty" });
            }
            // ほかのアプリからの引っ張り。ファイル（text/uri-list）なら入った時に場所の一覧を受け取り、Windows と同じ
            // FileDragEnter にする（受け取る部品を決めるのは共通の部分）
            wl_data_device::Event::Enter { serial, surface, x, y, id: Some(offer) } => {
                if let Some(old) = st.drag.take() {
                    old.offer.destroy();
                }
                let Some(win) = st.win_of(&surface) else {
                    offer.destroy();
                    return;
                };
                let s = st.windows[st.index(win).unwrap_or(0)].scale();
                let at = (x as f32 * s, y as f32 * s);
                let mimes = offer.data::<Mutex<Vec<String>>>().map(|m| m.lock().map(|v| v.clone()).unwrap_or_default()).unwrap_or_default();
                let mut paths: Vec<PathBuf> = Vec::new();
                let mut how = String::from("no files");
                if mimes.iter().any(|m| m == URI_LIST) {
                    let conn = st.clip.borrow().conn.clone();
                    let t = Instant::now();
                    match receive_offer(&conn, &offer, URI_LIST) {
                        Some((bytes, timed_out)) => {
                            paths = crate::file_dialog::uri_list(&String::from_utf8_lossy(&bytes)).into_iter().map(|b| PathBuf::from(OsString::from_vec(b))).collect();
                            how = format!("files={} ms={:.1}{}", paths.len(), t.elapsed().as_secs_f64() * 1000.0, if timed_out { " timeout" } else { "" });
                        }
                        None => how = "pipe error".into(),
                    }
                }
                log::event("drag.enter", &format!("mimes={} at={:.0},{:.0}", mimes.len(), at.0, at.1), &how);
                let files = !paths.is_empty();
                if files && let Some(w) = st.win(win) {
                    w.push(Event::FileDragEnter { x: at.0, y: at.1, paths });
                }
                st.drag = Some(Drag { win, offer, serial, files, at, accepting: None });
                st.update_drag_accept();
            }
            wl_data_device::Event::Motion { x, y, .. } => {
                let Some(win) = st.drag.as_ref().map(|d| d.win) else { return };
                let s = st.index(win).map_or(1.0, |i| st.windows[i].scale());
                let at = (x as f32 * s, y as f32 * s);
                let files = match st.drag.as_mut() {
                    Some(d) => {
                        d.at = at;
                        d.files
                    }
                    None => return,
                };
                if files && let Some(w) = st.win(win) {
                    w.push(Event::FileDragOver { x: at.0, y: at.1 });
                }
                st.update_drag_accept();
            }
            wl_data_device::Event::Leave => {
                if let Some(d) = st.drag.take() {
                    if d.files
                        && let Some(w) = st.win(d.win)
                    {
                        w.push(Event::FileDragLeave);
                    }
                    d.offer.destroy();
                    log::event("drag.leave", "", "ok");
                }
            }
            // 落とした。場所は入った時に受け取ってある。受け取れると答えていた時だけ、終わりを知らせる（finish）
            wl_data_device::Event::Drop => {
                if let Some(d) = st.drag.take() {
                    let ok = d.files && d.accepting == Some(true);
                    let (x, y) = d.at;
                    if let Some(w) = st.win(d.win) {
                        if ok {
                            w.push(Event::FileDrop { x, y });
                        } else if d.files {
                            w.push(Event::FileDragLeave);
                        }
                    }
                    if ok && d.offer.version() >= 3 {
                        d.offer.finish();
                    }
                    d.offer.destroy();
                    log::event("drag.drop", &format!("at={x:.0},{y:.0}"), if ok { "dropped" } else { "not accepted" });
                }
            }
            _ => {}
        }
    }

    event_created_child!(State, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, Mutex::new(Vec::<String>::new())),
    ]);
}

impl Dispatch<wl_data_offer::WlDataOffer, Mutex<Vec<String>>> for State {
    fn event(_: &mut Self, _: &wl_data_offer::WlDataOffer, event: wl_data_offer::Event, mimes: &Mutex<Vec<String>>, _: &Connection, _: &QueueHandle<Self>) {
        // offer が持てる形（mime）を貯める（selection の知らせより先に届く）
        if let wl_data_offer::Event::Offer { mime_type } = event
            && let Ok(mut m) = mimes.lock()
        {
            m.push(mime_type);
        }
    }
}

impl Dispatch<wl_data_source::WlDataSource, ()> for State {
    fn event(st: &mut Self, source: &wl_data_source::WlDataSource, event: wl_data_source::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            // ほかのアプリが貼る: 渡された口へ文字を書いて閉じる
            wl_data_source::Event::Send { mime_type, fd } => {
                let c = st.clip.borrow();
                let text = c.ours.as_ref().filter(|(s, _)| s == source).map(|(_, t)| t.clone()).unwrap_or_default();
                let mut rest = text.as_bytes();
                while !rest.is_empty() {
                    match rustix::io::write(&fd, rest) {
                        Ok(n) => rest = &rest[n..],
                        Err(rustix::io::Errno::INTR) => {}
                        Err(_) => break,
                    }
                }
                log::event("clipboard.send", &format!("mime={mime_type} chars={}", text.chars().count()), if rest.is_empty() { "ok" } else { "error" });
            }
            // ほかのアプリが選択を取った（または自分で取り直した）
            wl_data_source::Event::Cancelled => {
                let mut c = st.clip.borrow_mut();
                if c.ours.as_ref().is_some_and(|(s, _)| s == source) {
                    c.ours = None;
                    log::event("clipboard.cancelled", "", "another app took the selection");
                }
                source.destroy();
            }
            _ => {}
        }
    }
}
