//! Linux の窓（Wayland）。Windows の platform.rs と同じく、OS との境目をこのファイルに閉じる。
//!
//! 窓は xdg-shell、絵は CPU の道（cpu_frame）で描いて wl_shm の共有メモリに写す（2 枚を交互に使い、合成が
//! 読み終えた絵だけに描く）。倍率は fractional-scale（1/120 単位）で受け、絵は物理ピクセルで作り、viewporter で
//! 論理の大きさに合わせる。キー配列は libxkbcommon を動く時に読む（xkbcommon-dl。組み立てに開発用の部品が要らない）。
//! 待つのは poll の 1 か所（Wayland の接続・ほかのスレッドから起こす eventfd・時間）。
//!
//! クリップボードは wl_data_device（文字だけ）。地元の日付と時刻は C の localtime_r で読む。
//! 日本語入力は text-input-v3（fcitx5 などの IME が、合成を通して変換中の文字と確定の文字を送る）。
//!
//! まだ無い物（次の区切り）: 読み上げ・ファイルのダイアログ・明暗の設定・複数の窓・ドラッグ＆ドロップ・
//! 題の帯（GNOME の CSD）。

use std::cell::RefCell;
use std::ffi::c_char;
use std::os::fd::{AsFd, OwnedFd};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
use crate::input::{Event, Key, Mods};
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
        let (read_end, write_end) = pipe_with(PipeFlags::CLOEXEC).ok()?;
        offer.receive(mime.to_string(), write_end.as_fd());
        drop(write_end);
        let _ = c.conn.flush();
        // ほかのアプリが書き終える（書く口を閉じる）まで読む。止まったアプリで固まらないよう、待つのは 1 秒まで
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
    fn new(shm: &wl_shm::WlShm, qh: &QueueHandle<State>, width: i32, height: i32) -> Result<Shm, crate::Error> {
        let stride = width * 4;
        let one = (stride * height) as usize;
        let len = one * 2;
        let fd = memfd_create("kargui", MemfdFlags::CLOEXEC).map_err(|e| error("memfd_create", e))?;
        ftruncate(&fd, len as u64).map_err(|e| error("ftruncate", e))?;
        // SAFETY: 作ったばかりの memfd を len の大きさで写す。写した所はこの Shm が持ち、Drop で外す
        let map = unsafe { mmap(std::ptr::null_mut(), len, ProtFlags::READ | ProtFlags::WRITE, MapFlags::SHARED, &fd, 0) }
            .map_err(|e| error("mmap", e))? as *mut u8;
        let pool = shm.create_pool(fd.as_fd(), len as i32, qh, ());
        let slots = [0usize, 1].map(|i| (pool.create_buffer((one * i) as i32, width, height, stride, wl_shm::Format::Xrgb8888, qh, ()), false));
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
    /// 合成が、この窓に文字の入力の焦点を置いている（enter から leave まで）
    entered: bool,
    /// キーボードの焦点がこの窓にある。Hyprland は作業場を移った時に text-input の leave を送らないので、
    /// キーボードの焦点でも有効・無効を決める
    keyboard: bool,
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

struct State {
    rt: Runtime,
    app: Box<dyn App>,
    qh: QueueHandle<State>,
    shm: wl_shm::WlShm,
    surface: wl_surface::WlSurface,
    viewport: Option<wp_viewport::WpViewport>,
    cursor_mgr: Option<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1>,
    cursor: Option<wp_cursor_shape_device_v1::WpCursorShapeDeviceV1>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    xkb: Option<Keyboard>,
    /// クリップボード（WaylandClipboard と分け合う）
    clip: Rc<RefCell<Clip>>,
    /// 日本語入力（合成が text-input-v3 を持っていなければ None）
    text_input: Option<zwp_text_input_v3::ZwpTextInputV3>,
    ime: Ime,
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
    running: bool,
    focused: bool,
    // ポインター（物理ピクセル）
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
    shown: bool,
    warned_windows: bool,
}

impl State {
    fn scale(&self) -> f32 {
        self.scale120 as f32 / 120.0
    }

    fn physical(&self) -> (i32, i32) {
        let s = self.scale120 as i64;
        let f = |v: i32| ((v as i64 * s + 60) / 120).max(1) as i32;
        (f(self.logical.0), f(self.logical.1))
    }

    /// 論理の大きさか倍率が変わった
    fn apply_size(&mut self) {
        let (w, h) = self.physical();
        self.rt.resize(w as f32, h as f32, self.scale());
        self.summary = Summary::default();
        self.full = true;
        self.need_frame = true;
        self.why = "size";
        log::event("window.size", &format!("logical={}x{} scale={:.3}", self.logical.0, self.logical.1, self.scale()), &format!("physical={w}x{h}"));
    }

    fn push(&mut self, e: Event) {
        self.events.push(e);
        self.need_frame = true;
        self.why = "input";
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
        let (x, y) = (self.px, self.py);
        let hit = self.rt.cx.hit(x, y).cloned();
        let shape = match hit.as_ref().and_then(|h| self.rt.cx.prev.split_bars.get(&h.id).copied()) {
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
        let cx = &self.rt.cx;
        let inner_drag = cx.dnd.as_ref().is_some_and(|d| !d.external);
        if cx.drag.is_some() || cx.pressed.is_some() || cx.press_source.is_some() || inner_drag {
            // つまみをつかんでいる・部品を押している・引っ張っている間は、移動を渡す（Windows の platform.rs と同じ）
            if self.left_down {
                self.push(Event::PointerMove { x, y });
                if let Some(ok) = self.rt.cx.drag_cursor(x, y) {
                    self.set_shape(if ok { Shape::Default } else { Shape::NotAllowed });
                }
            } else if inner_drag || self.rt.cx.press_source.is_some() {
                log::event("drag.lost", &format!("x={x} y={y}"), "cancel");
                self.push(Event::PointerCancel);
            } else {
                self.push(Event::PointerUp { x, y });
            }
            return;
        }
        let hover = self.rt.hover_at(x, y);
        let bar = self.rt.cx.scrollbar_at(x, y).map(|l| l.id);
        if hover != self.rt.cx.hover {
            log::event("hover", &format!("x={x} y={y}"), &format!("id={}", hover.map_or("none".to_string(), |h| format!("{:x}", h.0))));
        }
        if hover != self.rt.cx.hover || bar != self.rt.cx.bar_hover {
            self.rt.cx.set_hover(hover);
            self.rt.cx.bar_hover = bar;
            self.need_frame = true;
            self.why = "input";
        }
        self.update_shape();
    }

    fn key_down(&mut self, code: u32, repeat: bool) {
        let Some(kb) = self.xkb.as_ref() else { return };
        let mods = kb.mods();
        let key = key_of(kb.base_sym(code));
        // 文字は Ctrl と Alt の無い時だけ（Windows の WM_CHAR と同じく、ショートカットはキーとして受ける）
        let text = if mods.ctrl || mods.alt { String::new() } else { kb.utf8(code) };
        if let Some(key) = key {
            if !repeat {
                log::event("key", &format!("key={key:?} mods={mods:?}"), "sent");
            }
            self.push(Event::Key { key, mods });
        }
        for c in text.chars().filter(|c| !c.is_control()) {
            if !repeat {
                log::event("char", &format!("u=0x{:04X}", c as u32), "insert");
            }
            self.push(Event::Char(c));
        }
    }

    /// 焦点のある部品が入力欄なら IME を有効に、そうでなければ無効にする。文字を打つ位置が動いたら知らせる。
    /// kargui が変換中の文字を確定した時（入力欄の外を押したなど）は、無効にして有効にし直し、IME の変換を捨てさせる
    fn update_ime(&mut self) {
        let cancel = std::mem::take(&mut self.rt.cx.ime_cancel);
        let Some(ti) = self.text_input.clone() else { return };
        if !self.ime.entered {
            return;
        }
        let focus = self.rt.cx.focus.filter(|f| self.ime.keyboard && self.rt.cx.prev.inputs.contains_key(f));
        let s = self.scale();
        let rect = self.rt.cx.caret.map(|c| ((c.x / s).floor() as i32, (c.y / s).floor() as i32, (c.w / s).ceil().max(1.0) as i32, (c.h / s).ceil().max(1.0) as i32));
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
            let id = |f: Option<Id>| f.map_or("none".to_string(), |f| format!("{:x}", f.0));
            log::event("ime.enable", &format!("why={why} from={} to={} rect={rect:?}", id(was), id(focus)), &format!("commits={}", self.ime.commits));
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
        self.push(Event::ImePreedit(String::new()));
        self.push(Event::ImeCommit(shown));
    }

    /// text-input-v3 の done: たまった変換中の文字と確定の文字を入力にする
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
        for e in events {
            self.push(e);
        }
    }

    /// 1 フレーム: 入力を渡してビューを回し、描いて画面に出す
    fn frame(&mut self) {
        if !self.configured {
            return;
        }
        let why = self.why;
        let t = Instant::now();
        let events = std::mem::take(&mut self.events);
        let out = self.rt.frame(&mut *self.app, events);
        if std::mem::take(&mut self.rt.cx.quit) {
            log::event("window.quit", "", "ok");
            self.running = false;
        }
        if let Some((key, _)) = self.rt.cx.file_dialog_request.take() {
            log::event("file_dialog", &format!("key={key}"), "not supported on linux yet");
        }
        if !self.rt.cx.windows.is_empty() && !self.warned_windows {
            self.warned_windows = true;
            log::event("window.open", &format!("count={}", self.rt.cx.windows.len()), "not supported on linux yet");
        }
        // 文字を打つ位置は、絵と同じ wl_surface.commit で合成に届くように、絵より先に知らせる
        self.update_ime();
        self.wake_at = out.wake_after.map(|d| Instant::now() + d);
        self.animating = out.want_frame;
        self.need_frame = false;
        let t_build = t.elapsed().as_secs_f64() * 1000.0;

        let t_draw = Instant::now();
        let (w, h) = self.physical();
        if self.buffers.as_ref().is_none_or(|b| b.width != w || b.height != h) {
            self.buffers = None;
            match Shm::new(&self.shm, &self.qh, w, h) {
                Ok(b) => self.buffers = Some(b),
                Err(e) => {
                    log::event("surface.init", &format!("size={w}x{h}"), &format!("error={e}"));
                    return;
                }
            }
            self.pixels = vec![0; (w * h) as usize];
            self.summary = Summary::default();
            self.full = true;
        }
        let Some(buffers) = self.buffers.as_mut() else { return };
        let Some(slot) = buffers.free_slot() else {
            // 2 枚とも合成が読んでいる。どちらかが返ってきたら描く
            log::event("render", &format!("why={why}"), "busy (both buffers in use)");
            self.need_frame = true;
            return;
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
            self.surface.frame(&self.qh, ());
            self.frame_callback = true;
        }
        self.surface.commit();
        let t_draw = t_draw.elapsed().as_secs_f64() * 1000.0;
        if !self.animating {
            log::event(
                "render",
                &format!("why={why} {} quads={} slot={slot}", drawn.mode, self.rt.draw.quads.len()),
                &format!("ms={:.2} draw_ms={t_draw:.2}", t_build + t_draw),
            );
        }
        if self.shown {
            let number = self.rt.frames - 1;
            self.app.presented(&Frame { number, since_start: self.started.elapsed() });
        } else {
            self.shown = true;
            log::event("window.shown", &format!("physical={w}x{h} scale={:.3}", self.scale()), &format!("ms={:.1}", self.started.elapsed().as_secs_f64() * 1000.0));
            self.app.presented(&Frame { number: 0, since_start: self.started.elapsed() });
        }
    }
}

/// 窓の app_id（Hyprland の窓の規則などで使う）は、実行ファイルの名前
fn app_id() -> String {
    std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "kargui".into())
}

pub(crate) fn run(options: Options, app: Box<dyn App>) -> Result<(), crate::Error> {
    let started = Instant::now();
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

    let surface = compositor.create_surface(&qh, ());
    let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg_surface.get_toplevel(&qh, ());
    toplevel.set_title(options.title.clone());
    toplevel.set_app_id(app_id());
    if let Some((w, h)) = options.min_size {
        toplevel.set_min_size(w.round() as i32, h.round() as i32);
    }
    // 合成が題の帯を描けるなら頼む（KDE・sway など。Hyprland と GNOME には無い）
    let _decoration = deco_mgr.as_ref().map(|m| {
        let d = m.get_toplevel_decoration(&toplevel, &qh, ());
        d.set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        d
    });
    let viewport = viewporter.as_ref().map(|v| v.get_viewport(&surface, &qh, ()));
    let _fractional = frac_mgr.as_ref().map(|m| m.get_fractional_scale(&surface, &qh, ()));
    surface.commit();

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

    let text = Text::new()?;
    let dark = options.dark.unwrap_or(false);
    let mut rt = Runtime::new(text, dark, Box::new(WaylandClipboard(clip.clone())), options.title.clone());
    // ほかのスレッドから起こされたら、eventfd に書いて poll を起こす
    let wake_fd = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).map_err(|e| error("eventfd", e))?);
    let writer = wake_fd.clone();
    rt.cx.waker = crate::wake::Waker::new(move || {
        let _ = rustix::io::write(&*writer, &1u64.to_ne_bytes());
    });
    rt.cx.text.load_fonts(&options.fonts);
    rt.cx.font = rt.cx.text.fonts.id(&options.font);
    let logical = (options.size.0.round().max(1.0) as i32, options.size.1.round().max(1.0) as i32);
    let local = local_date().zip(local_time()).map_or("unknown".to_string(), |((y, m, d), (h, min))| format!("{y}-{m:02}-{d:02} {h:02}:{min:02}"));
    log::event("window.create", &format!("title={:?} size={}x{} app_id={} local={local}", options.title, logical.0, logical.1, app_id()), "ok");

    let mut st = State {
        rt,
        app,
        qh: qh.clone(),
        shm,
        surface,
        viewport,
        cursor_mgr,
        cursor: None,
        pointer: None,
        keyboard: None,
        xkb: Keyboard::new(),
        clip,
        text_input,
        ime: Ime::default(),
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
        running: true,
        focused: false,
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
        shown: false,
        warned_windows: false,
    };
    if st.xkb.is_none() {
        log::event("keyboard.xkb", "", "error=libxkbcommon.so could not be loaded (keys will not work)");
    }
    st.apply_size();
    // 最初の configure（と、出せれば倍率）を待つ
    queue.roundtrip(&mut st).map_err(|e| error("roundtrip", e))?;

    let _keep = (compositor, wm_base, xdg_surface, toplevel, seat, ti_mgr);
    while st.running {
        queue.dispatch_pending(&mut st).map_err(|e| error("dispatch", e))?;
        let now = Instant::now();
        if st.wake_at.is_some_and(|t| t <= now) {
            st.wake_at = None;
            log::event("timer", "", "frame");
            st.need_frame = true;
            st.why = "timer";
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
        if st.need_frame && st.configured {
            st.frame();
        }
        if !st.running {
            break;
        }
        if let Err(e) = conn.flush() {
            log::event("wayland.flush", "", &format!("error={e}"));
        }
        // 次に起きる時（タイマー・押し続け）。アニメーション中は合成のフレームの知らせで起きる
        let deadline = [st.wake_at, st.xkb.as_ref().and_then(|k| k.repeat.map(|r| r.1))].into_iter().flatten().min();
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
            st.rt.cx.waker.take_woken();
            log::event("wake", "from=waker", "frame");
            st.need_frame = true;
            st.why = "wake";
        }
    }
    log::event("loop.end", "", "ok");
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

impl Dispatch<xdg_surface::XdgSurface, ()> for State {
    fn event(st: &mut Self, xs: &xdg_surface::XdgSurface, event: xdg_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_surface::Event::Configure { serial } = event {
            xs.ack_configure(serial);
            let first = !st.configured;
            st.configured = true;
            if let Some(size) = st.pending_size.take()
                && size != st.logical
            {
                st.logical = size;
                st.apply_size();
            }
            if first {
                log::event("window.configure", &format!("logical={}x{}", st.logical.0, st.logical.1), "first");
            }
            st.need_frame = true;
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for State {
    fn event(st: &mut Self, _: &xdg_toplevel::XdgToplevel, event: xdg_toplevel::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            xdg_toplevel::Event::Configure { width, height, states } => {
                // 状態は u32 の並び（1 最大化・2 全画面・4 前面・5〜8 敷き詰めの辺）
                let states: Vec<u32> = states.as_chunks::<4>().0.iter().map(|c| u32::from_ne_bytes(*c)).collect();
                let has = |s: xdg_toplevel::State| states.contains(&(s as u32));
                use xdg_toplevel::State as S;
                let constrained = [S::Maximized, S::Fullscreen, S::TiledLeft, S::TiledRight, S::TiledTop, S::TiledBottom].into_iter().any(has);
                log::event("toplevel.configure", &format!("size={width}x{height} states={states:?} configured={}", st.configured), &format!("constrained={constrained}"));
                // 最大化・全画面・敷き詰めの時は、言われた大きさに従う。浮いた窓の最初の大きさは勧めにすぎないので、
                // アプリが頼んだ大きさ（Options::size）を使う（Hyprland は浮いた窓に画面いっぱいを勧めてくる）。
                // 2 回目からは、利用者が窓の大きさを変えた結果なので従う
                if width > 0 && height > 0 && (constrained || st.configured) {
                    st.pending_size = Some((width, height));
                } else if width > 0 && height > 0 {
                    log::event("window.configure", &format!("suggested={width}x{height}"), "ignored (floating, first configure: use Options::size)");
                }
                let activated = has(S::Activated);
                if activated != st.focused {
                    st.focused = activated;
                    log::event("window.focus", &format!("focused={activated}"), "ok");
                    st.need_frame = true;
                }
            }
            xdg_toplevel::Event::Close => {
                let ok = st.app.close_requested();
                log::event("window.close_requested", "", &format!("allowed={ok}"));
                if ok {
                    st.running = false;
                } else {
                    st.need_frame = true;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for State {
    fn event(st: &mut Self, _: &wl_surface::WlSurface, event: wl_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        // fractional-scale が無い合成では、整数の倍率で描く
        if let wl_surface::Event::PreferredBufferScale { factor } = event
            && st.viewport.is_none()
            && factor != st.int_scale
        {
            st.int_scale = factor.max(1);
            st.scale120 = st.int_scale as u32 * 120;
            st.apply_size();
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for State {
    fn event(st: &mut Self, _: &wp_fractional_scale_v1::WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event
            && scale != st.scale120
            && st.viewport.is_some()
        {
            st.scale120 = scale.max(1);
            st.apply_size();
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for State {
    fn event(st: &mut Self, buffer: &wl_buffer::WlBuffer, event: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = event
            && let Some(b) = st.buffers.as_mut()
        {
            for slot in b.slots.iter_mut().filter(|s| &s.0 == buffer) {
                slot.1 = false;
            }
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(st: &mut Self, _: &wl_callback::WlCallback, event: wl_callback::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = event {
            st.frame_callback = false;
            if st.animating {
                st.need_frame = true;
                st.why = "animation";
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
        let s = st.scale();
        match event {
            wl_pointer::Event::Enter { serial, surface_x, surface_y, .. } => {
                st.pointer_serial = serial;
                st.shape = None;
                st.pointer_motion(surface_x as f32 * s, surface_y as f32 * s);
                // 押していない時は形を決める（入ってきた時に決めないと、合成の前の形のまま）
                st.update_shape();
            }
            wl_pointer::Event::Leave { .. } => {
                let had = st.rt.cx.hover.is_some();
                let bar = st.rt.cx.bar_hover.take().is_some();
                log::event("pointer.leave", "", &format!("had_hover={had}"));
                st.rt.cx.set_hover(None);
                if had || bar {
                    st.need_frame = true;
                    st.why = "input";
                }
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                st.pointer_motion(surface_x as f32 * s, surface_y as f32 * s);
            }
            wl_pointer::Event::Button { serial, time, button, state } => {
                let pressed = state == WEnum::Value(wl_pointer::ButtonState::Pressed);
                st.clip.borrow_mut().serial = serial;
                let (x, y) = (st.px, st.py);
                if button != BTN_LEFT {
                    log::event("button", &format!("button=0x{button:x} pressed={pressed} x={x} y={y}"), "ok");
                }
                match button {
                    BTN_LEFT if pressed => {
                        st.left_down = true;
                        let target = st.rt.cx.hit(x, y).map(|h| h.id);
                        // 続けて押した何回目か（400 ms の中で、4 論理ピクセルの中を押したら続き。3 回で止める）
                        let d = DOUBLE_CLICK_DIST * s;
                        let count = match st.last_press {
                            Some((t0, x0, y0, n)) if time.wrapping_sub(t0) <= DOUBLE_CLICK_MS && (x - x0).abs() <= d && (y - y0).abs() <= d => (n % 3) + 1,
                            _ => 1,
                        };
                        st.last_press = Some((time, x, y, count));
                        log::event("click", &format!("x={x} y={y} count={count}"), &format!("target={target:?}"));
                        let shift = st.mods().shift;
                        st.push(Event::PointerDown { x, y, shift, count });
                    }
                    BTN_LEFT => {
                        st.left_down = false;
                        st.push(Event::PointerUp { x, y });
                        st.shape = None;
                    }
                    // 右クリックは離した時（Windows の WM_CONTEXTMENU と同じ）
                    BTN_RIGHT if !pressed => {
                        log::event("context_menu", &format!("x={x} y={y}"), "sent");
                        st.push(Event::ContextMenu { x, y });
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
                let dy = if v120 != 0 {
                    v120 as f32 / 120.0 * WHEEL_STEP
                } else {
                    value as f32 * s
                };
                if dy != 0.0 {
                    let (x, y) = (st.px, st.py);
                    log::event("wheel", &format!("source={source:?} value120={v120} value={value:.2} x={x} y={y}"), &format!("dy={dy:.1}"));
                    st.push(Event::Wheel { x, y, dy });
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
            wl_keyboard::Event::Enter { serial, .. } => {
                st.clip.borrow_mut().serial = serial;
                st.ime.keyboard = true;
                st.need_frame = true;
                log::event("keyboard.enter", "", "ok");
            }
            wl_keyboard::Event::Leave { .. } => {
                if let Some(k) = st.xkb.as_mut() {
                    k.stop_repeat("leave");
                }
                st.ime.keyboard = false;
                st.need_frame = true;
                st.commit_shown("keyboard.leave");
                log::event("keyboard.leave", "", "ok");
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
            E::Enter { surface } if surface == st.surface => {
                st.ime.entered = true;
                st.ime.enabled = None;
                st.ime.rect = None;
                st.need_frame = true;
                st.why = "input";
                log::event("ime.enter", "", "ok");
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
            // ほかのアプリからの引っ張り（まだ受け取らない。offer は片付ける）
            wl_data_device::Event::Enter { id: Some(offer), .. } => {
                offer.destroy();
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
