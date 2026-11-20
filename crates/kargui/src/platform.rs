//! 窓（Win32 を直接使う）、メッセージの輪、日本語入力（IMM32）、読み上げ（AccessKit）、描く道。
//!
//! 段階 0 の試作（spike/src/win.rs）で確かめた決まりを引き継ぐ:
//! - 変化が無い間は GetMessage で眠り、自分から起きない（待機中の CPU を 0 にする）
//! - アニメーションの間だけ、窓のあるモニターの垂直同期に合わせて描く。前面にない窓は full_rate が無ければ約 30 fps
//! - 変換を確定する Enter は IME が受け取り、アプリには VK_PROCESSKEY として届くので、送信しない
//! - 状態を借りたまま、窓へメッセージを送り返す関数（SetWindowPos・DestroyWindow・Imm*）を呼ばない。Post に入れて借りの外で行う
//! - 閉じる時は、GPU の物と読み上げを片付けてから窓を壊す

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_windows::{Adapter, QueuedEvents};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmFlush, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, w};

use crate::app::{App, Frame, Options, Renderer};
use crate::damage::{self, Summary};
use crate::gdi::Gdi;
use crate::geom::{Color, IRect};
use crate::gpu::Gpu;
use crate::input::{A11yAction, Event, Key, Mods};
use crate::log;
use crate::raster::Canvas;
use crate::runtime::Runtime;
use crate::text::{ATLAS_SIZE, Text};
use crate::ui::Id;
use crate::ui::context::Clipboard;

const WM_APP_A11Y: u32 = WM_APP + 1;
/// これより長くフレームの間が空いたら、内訳を記録する（60 Hz の 1.5 フレーム）
const SLOW_FRAME_MS: f64 = 25.0;
/// 変換ごとに記録する IME の問い合わせの数
const IME_REQUEST_LOG_MAX: u32 = 3;
/// マウスのホイール 1 目盛りで動かす量（論理ピクセル。行の高さ 32 の 3 行分）
const WHEEL_STEP: f32 = 96.0;
/// WM_GETOBJECT（読み上げソフトなどからの問い合わせ）を受けた回数
static GETOBJECT_COUNT: AtomicU32 = AtomicU32::new(0);

enum Surface {
    Gpu(Gpu),
    Cpu(Gdi),
}

/// 状態の借りを返してから行うこと
enum Post {
    SetWindowPos(HWND, RECT),
    Destroy(HWND),
    Raise(QueuedEvents),
    AssociateIme(HWND, bool),
    ImePosition(HWND, IRect),
    DarkTitle(HWND, bool),
}

/// 読み上げの操作を、窓の持ち主のスレッドへ渡す係（AccessKit は別のスレッドから呼ぶことがある）
struct ActionSink {
    hwnd: isize,
    queue: Arc<Mutex<VecDeque<ActionRequest>>>,
}

impl ActionHandler for ActionSink {
    fn do_action(&mut self, request: ActionRequest) {
        log::event("a11y.action", &format!("action={:?} node={:?}", request.action, request.target_node), "queued");
        if let Ok(mut q) = self.queue.lock() {
            q.push_back(request);
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_APP_A11Y, WPARAM(0), LPARAM(0));
        }
    }
}

/// 最初の読み上げの木を渡す係。借りの外へ結果を持ち出せるよう、木を自分で持つ
struct Initial(Option<TreeUpdate>);

impl ActivationHandler for Initial {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        log::event("a11y.activate", "", "initial tree");
        self.0.take()
    }
}

/// OS のクリップボード
struct SystemClipboard {
    hwnd: isize,
}

const CF_UNICODETEXT: u32 = 13;

impl Clipboard for SystemClipboard {
    fn get(&mut self) -> Option<String> {
        unsafe {
            OpenClipboard(Some(HWND(self.hwnd as *mut _))).ok()?;
            let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u16;
                if p.is_null() {
                    return None;
                }
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                let _ = GlobalUnlock(g);
                Some(s)
            });
            let _ = CloseClipboard();
            log::event("clipboard.get", "", &format!("chars={}", text.as_ref().map_or(0, |t| t.chars().count())));
            text
        }
    }

    fn set(&mut self, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            if OpenClipboard(Some(HWND(self.hwnd as *mut _))).is_err() {
                log::event("clipboard.set", "", "error=open");
                return;
            }
            let _ = EmptyClipboard();
            let ok = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok().map(|g| {
                let p = GlobalLock(g) as *mut u16;
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                    let _ = GlobalUnlock(g);
                }
                SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))).is_ok()
            });
            let _ = CloseClipboard();
            log::event("clipboard.set", &format!("chars={}", text.chars().count()), &format!("ok={ok:?}"));
        }
    }
}

/// OS が暗いテーマか（設定 > 個人用設定 > 色 > アプリ モード）
fn system_dark() -> bool {
    let mut value = 1u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    r.is_ok() && value == 0
}

fn key_of(vk: VIRTUAL_KEY) -> Option<Key> {
    Some(match vk {
        VK_RETURN => Key::Enter,
        VK_TAB => Key::Tab,
        VK_ESCAPE => Key::Escape,
        VK_SPACE => Key::Space,
        VK_BACK => Key::Backspace,
        VK_DELETE => Key::Delete,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_A => Key::A,
        VK_C => Key::C,
        VK_V => Key::V,
        VK_X => Key::X,
        VK_Y => Key::Y,
        VK_Z => Key::Z,
        _ => return None,
    })
}

fn mods_now() -> Mods {
    let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk.0 as i32) } < 0;
    Mods { shift: down(VK_SHIFT), ctrl: down(VK_CONTROL), alt: down(VK_MENU) }
}

fn lo(v: isize) -> i32 {
    (v & 0xffff) as i16 as i32
}
fn hi(v: isize) -> i32 {
    ((v >> 16) & 0xffff) as i16 as i32
}

struct State {
    hwnd: HWND,
    app: Box<dyn App>,
    rt: Runtime,
    surface: Option<Surface>,
    adapter: Option<Adapter>,
    actions: Arc<Mutex<VecDeque<ActionRequest>>>,
    post: Vec<Post>,
    events: Vec<Event>,
    /// 次のメッセージの区切りで描く
    need_frame: bool,
    /// アプリがアニメーションを求めている
    animating: bool,
    started: Instant,
    shown: bool,
    full_rate: bool,
    window_focused: bool,
    composing: bool,
    high_surrogate: Option<u16>,
    last_caret: Option<IRect>,
    ime_requests: u32,
    /// CPU の道: 前のフレームの描く物の要約と背景の色
    summary: Summary,
    background: Color,
    dark_title: Option<bool>,
    tracking_leave: bool,
    // 遅いフレームの内訳
    last_tick: Option<Instant>,
    t_build: f64,
    t_draw: f64,
    t_wait: f64,
    slow_frames: u32,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// 状態を借りて f を呼ぶ。すでに借りている（メッセージの入れ子）ときは None
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| {
        let Ok(mut guard) = s.try_borrow_mut() else { return None };
        guard.as_mut().map(f)
    })
}

/// 借りを返したあとで、Post に入れたことを行う
fn drain_post() {
    let posts = with_state(|st| std::mem::take(&mut st.post)).unwrap_or_default();
    for p in posts {
        match p {
            Post::SetWindowPos(hwnd, r) => unsafe {
                let res = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                log::event("window.setpos", &format!("rect={},{},{},{}", r.left, r.top, r.right, r.bottom), &format!("{res:?}"));
            },
            Post::Destroy(hwnd) => unsafe {
                let res = DestroyWindow(hwnd);
                log::event("window.destroy_call", "", &format!("{res:?}"));
            },
            Post::Raise(events) => events.raise(),
            Post::AssociateIme(hwnd, on) => unsafe {
                let ok = ImmAssociateContextEx(hwnd, HIMC::default(), if on { IACE_DEFAULT } else { 0 });
                log::event("ime.associate", &format!("on={on}"), &format!("ok={}", ok.as_bool()));
            },
            Post::ImePosition(hwnd, caret) => set_ime_position(hwnd, caret),
            Post::DarkTitle(hwnd, dark) => unsafe {
                let v = BOOL::from(dark);
                let res = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&v as *const BOOL).cast(), 4);
                log::event("window.dark_title", &format!("dark={dark}"), &format!("{res:?}"));
            },
        }
    }
}

/// 文字を打つ位置を IME に知らせる（候補の窓をそこに出すため）
fn set_ime_position(hwnd: HWND, caret: IRect) {
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.is_invalid() {
            return;
        }
        let comp = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: POINT { x: caret.x, y: caret.y }, rcArea: RECT::default() };
        let cand = CANDIDATEFORM {
            dwIndex: 0,
            dwStyle: CFS_EXCLUDE,
            ptCurrentPos: POINT { x: caret.x, y: caret.bottom() },
            rcArea: RECT { left: caret.x, top: caret.y, right: caret.right(), bottom: caret.bottom() },
        };
        let a = ImmSetCompositionWindow(himc, &comp);
        let b = ImmSetCandidateWindow(himc, &cand);
        let _ = ImmReleaseContext(hwnd, himc);
        log::event("ime.caret", &format!("client={},{} h={}", caret.x, caret.y, caret.h), &format!("comp={} cand={}", a.as_bool(), b.as_bool()));
    }
}

impl State {
    fn size(&self) -> (i32, i32) {
        (self.rt.cx.width as i32, self.rt.cx.height as i32)
    }

    fn resize(&mut self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        let scale = self.rt.cx.scale;
        self.rt.resize(width as f32, height as f32, scale);
        let res = match self.surface.as_mut() {
            Some(Surface::Gpu(g)) => g.resize(width as u32, height as u32),
            Some(Surface::Cpu(c)) => c.resize(width, height),
            None => Ok(()),
        };
        if let Err(e) = res {
            log::event("resize", &format!("size={width}x{height}"), &format!("error={e:?}"));
        }
        self.summary = Summary::default();
        self.need_frame = true;
    }

    /// 1 フレーム: 入力を渡してビューを回し、描いて画面に出す
    fn frame(&mut self, why: &str) {
        let t = Instant::now();
        let events = std::mem::take(&mut self.events);
        let focus_before = self.rt.cx.focus;
        let out = self.rt.frame(&mut *self.app, events);
        self.animating = out.want_frame;
        self.need_frame = false;
        self.t_build = t.elapsed().as_secs_f64() * 1000.0;
        let t_draw = Instant::now();
        let bg = self.rt.cx.theme.background;
        let (w, h) = self.size();
        let mut mode = String::from("full");
        match self.surface.as_mut() {
            Some(Surface::Gpu(g)) => {
                if let Err(e) = g.render(&self.rt.draw, &mut self.rt.cx.text.atlas, bg, 0) {
                    log::event("render", &format!("why={why}"), &format!("error={e:?}"));
                }
            }
            Some(Surface::Cpu(c)) => {
                let cur = damage::summarize(&self.rt.draw, (w, h));
                let mut plan = damage::plan(&self.summary, &cur);
                if bg != self.background {
                    plan.full = true;
                }
                self.background = bg;
                let atlas = &self.rt.cx.text.atlas;
                let mut canvas = Canvas { px: c.pixels(), width: w, height: h };
                let mut present = Vec::new();
                if plan.full {
                    let all = IRect::new(0, 0, w, h);
                    canvas.draw(&self.rt.draw, &atlas.pixels, atlas.size, all, bg);
                    present.push(all);
                } else {
                    for (r, dy) in &plan.blits {
                        canvas.scroll_up(*r, *dy);
                        present.push(*r);
                    }
                    for r in &plan.damage {
                        canvas.draw(&self.rt.draw, &atlas.pixels, atlas.size, *r, bg);
                        if !present.iter().any(|p| p.intersect(r) == Some(*r)) {
                            present.push(*r);
                        }
                    }
                    mode = format!("blits={} damage={}", plan.blits.len(), plan.damage.len());
                }
                c.present(&present);
                self.summary = cur;
            }
            None => return,
        }
        self.t_draw = t_draw.elapsed().as_secs_f64() * 1000.0;
        if !self.animating {
            log::event(
                "render",
                &format!("why={why} {mode} quads={}", self.rt.draw.quads.len()),
                &format!("ms={:.2} draw_ms={:.2}", self.t_build + self.t_draw, self.t_draw),
            );
        }
        // 読み上げ・日本語入力・タイトルバーの色
        if let Some(adapter) = self.adapter.as_mut() {
            let rt = &mut self.rt;
            if let Some(events) = adapter.update_if_active(|| rt.a11y().0) {
                self.post.push(Post::Raise(events));
            }
        }
        let was_input = focus_before.is_some_and(|f| self.rt.cx.prev.inputs.contains_key(&f));
        let is_input = self.rt.cx.focus.is_some_and(|f| self.rt.cx.prev.inputs.contains_key(&f));
        if was_input != is_input || self.rt.frames == 1 {
            self.post.push(Post::AssociateIme(self.hwnd, is_input));
        }
        if let Some(c) = self.rt.cx.caret {
            let caret = IRect::new(c.x as i32, c.y as i32, c.w as i32, c.h as i32);
            if self.last_caret != Some(caret) {
                self.last_caret = Some(caret);
                self.post.push(Post::ImePosition(self.hwnd, caret));
            }
        }
        if self.dark_title != Some(self.rt.cx.dark) {
            self.dark_title = Some(self.rt.cx.dark);
            self.post.push(Post::DarkTitle(self.hwnd, self.rt.cx.dark));
        }
        if self.shown {
            let number = self.rt.frames - 1;
            self.app.presented(&Frame { number, since_start: self.started.elapsed() });
        }
    }

    /// アニメーションの 1 回分。遅いフレームの内訳も記録する
    fn tick(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_tick {
            let gap = (now - prev).as_secs_f64() * 1000.0;
            if gap > SLOW_FRAME_MS {
                self.slow_frames += 1;
                let rest = gap - self.t_build - self.t_draw - self.t_wait;
                log::event(
                    "frame.slow",
                    &format!("n={} gap_ms={gap:.1}", self.slow_frames),
                    &format!("build_ms={:.2} draw_ms={:.2} wait_ms={:.2} other_ms={rest:.1}", self.t_build, self.t_draw, self.t_wait),
                );
            }
        }
        self.last_tick = Some(now);
        self.frame("animation");
    }

    fn wait_vblank(&mut self) {
        // 前面にない窓は、full_rate が無ければ 2 回に 1 回だけ描く（約 30 fps）
        let waits = if self.window_focused || self.full_rate { 1 } else { 2 };
        let t = Instant::now();
        for _ in 0..waits {
            match self.surface.as_mut() {
                Some(Surface::Gpu(g)) => g.wait_vblank(),
                Some(Surface::Cpu(c)) => c.wait_vblank(),
                None => {}
            }
        }
        self.t_wait = t.elapsed().as_secs_f64() * 1000.0;
    }

    fn forget_output(&mut self) {
        match self.surface.as_mut() {
            Some(Surface::Gpu(g)) => g.forget_output(),
            Some(Surface::Cpu(c)) => c.forget_output(),
            None => {}
        }
    }

    fn push(&mut self, e: Event) {
        self.events.push(e);
        self.need_frame = true;
    }

    fn ime_string(&self, kind: IME_COMPOSITION_STRING) -> Option<String> {
        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if himc.is_invalid() {
                return None;
            }
            let bytes = ImmGetCompositionStringW(himc, kind, None, 0);
            let result = if bytes > 0 {
                let mut buf = vec![0u16; bytes as usize / 2];
                ImmGetCompositionStringW(himc, kind, Some(buf.as_mut_ptr().cast()), bytes as u32);
                Some(String::from_utf16_lossy(&buf))
            } else if bytes == 0 {
                Some(String::new())
            } else {
                None
            };
            let _ = ImmReleaseContext(self.hwnd, himc);
            result
        }
    }

    /// IME の「変換中の何文字目は画面のどこか」の問い合わせに答える。
    /// 文字ごとの位置は持っていないので、変換中の文字の範囲を文字数で等分して近い位置を返す
    fn answer_char_position(&self, info: &mut IMECHARPOSITION) -> bool {
        let (Some(p), Some(r)) = (self.rt.cx.preedit_rect, self.rt.cx.input_rect) else { return false };
        let count = self.rt.cx.preedit.encode_utf16().count().max(1) as f32;
        let x = p.x + p.w * (info.dwCharPos as f32 / count).min(1.0);
        let mut pt = POINT { x: x.round() as i32, y: p.y as i32 };
        let mut lt = POINT { x: r.x as i32, y: r.y as i32 };
        let mut rb = POINT { x: r.right() as i32, y: r.bottom() as i32 };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut pt);
            let _ = ClientToScreen(self.hwnd, &mut lt);
            let _ = ClientToScreen(self.hwnd, &mut rb);
        }
        info.pt = pt;
        info.cLineHeight = p.h.max(1.0) as u32;
        info.rcDocument = RECT { left: lt.x, top: lt.y, right: rb.x, bottom: rb.y };
        true
    }

    fn apply_a11y_actions(&mut self) {
        let requests: Vec<ActionRequest> = self.actions.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
        for r in requests {
            let action = match r.action {
                accesskit::Action::Click => A11yAction::Click,
                accesskit::Action::Focus => A11yAction::Focus,
                other => {
                    log::event("a11y.action", &format!("action={other:?}"), "ignored");
                    continue;
                }
            };
            self.push(Event::A11y { target: Id(r.target_node.0), action });
        }
    }

    /// メッセージを受ける。None を返したら DefWindowProc に任せる
    fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_CLOSE => {
                log::event("window.close", "", "requested");
                self.post.push(Post::Destroy(self.hwnd));
                Some(LRESULT(0))
            }
            WM_DESTROY => {
                // 窓が残っているうちに、GPU の物と読み上げを片付ける
                self.adapter = None;
                let had = self.surface.take().is_some();
                log::event("window.destroy", "", &format!("surface_dropped={had}"));
                unsafe { PostQuitMessage(0) };
                Some(LRESULT(0))
            }
            WM_SIZE => {
                let (w, h) = (lo(lp.0), hi(lp.0));
                log::event("window.size", &format!("size={w}x{h} kind={}", wp.0), "ok");
                self.resize(w, h);
                if self.shown && w > 0 && h > 0 {
                    self.frame("size");
                }
                Some(LRESULT(0))
            }
            WM_PAINT => {
                match self.surface.as_ref() {
                    Some(Surface::Cpu(c)) => c.paint(),
                    _ => unsafe {
                        let _ = ValidateRect(Some(self.hwnd), None);
                        self.need_frame = true;
                    },
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xffff) as f32;
                let (w, h) = (self.rt.cx.width, self.rt.cx.height);
                self.rt.resize(w, h, dpi / 96.0);
                self.last_caret = None;
                let r = unsafe { *(lp.0 as *const RECT) };
                log::event("window.dpi", &format!("dpi={dpi}"), "ok");
                self.post.push(Post::SetWindowPos(self.hwnd, r));
                self.summary = Summary::default();
                self.need_frame = true;
                Some(LRESULT(0))
            }
            WM_MOVE | WM_EXITSIZEMOVE | WM_DISPLAYCHANGE => {
                self.forget_output();
                self.last_caret = None;
                None
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                let focused = msg == WM_SETFOCUS;
                self.window_focused = focused;
                log::event("window.focus", &format!("focused={focused}"), "ok");
                if let Some(events) = self.adapter.as_mut().and_then(|a| a.update_window_focus_state(focused)) {
                    self.post.push(Post::Raise(events));
                }
                self.last_caret = None;
                self.need_frame = true;
                None
            }
            WM_MOUSEMOVE => {
                let (x, y) = (lo(lp.0) as f32, hi(lp.0) as f32);
                if !self.tracking_leave {
                    let mut t =
                        TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                    self.tracking_leave = unsafe { TrackMouseEvent(&mut t) }.is_ok();
                }
                let hover = self.rt.hover_at(x, y);
                if hover != self.rt.cx.hover {
                    self.rt.cx.hover = hover;
                    self.need_frame = true;
                }
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.tracking_leave = false;
                if self.rt.cx.hover.take().is_some() {
                    self.need_frame = true;
                }
                Some(LRESULT(0))
            }
            WM_SETCURSOR if (lp.0 & 0xffff) as u32 == HTCLIENT => {
                let mut p = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut p);
                    let _ = ScreenToClient(self.hwnd, &mut p);
                }
                let over_input = self.rt.cx.hit(p.x as f32, p.y as f32).is_some_and(|h| h.text_input);
                if let Ok(c) = unsafe { LoadCursorW(None, if over_input { IDC_IBEAM } else { IDC_ARROW }) } {
                    unsafe { SetCursor(Some(c)) };
                }
                Some(LRESULT(1))
            }
            WM_LBUTTONDOWN => {
                let (x, y) = (lo(lp.0) as f32, hi(lp.0) as f32);
                unsafe { SetCapture(self.hwnd) };
                let target = self.rt.cx.hit(x, y).map(|h| h.id);
                log::event("click", &format!("x={x} y={y}"), &format!("target={target:?}"));
                self.push(Event::PointerDown { x, y, shift: mods_now().shift });
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                let (x, y) = (lo(lp.0) as f32, hi(lp.0) as f32);
                unsafe {
                    let _ = ReleaseCapture();
                }
                self.push(Event::PointerUp { x, y });
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = hi(wp.0 as isize) as f32;
                let mut p = POINT { x: lo(lp.0), y: hi(lp.0) };
                unsafe {
                    let _ = ScreenToClient(self.hwnd, &mut p);
                }
                log::event("wheel", &format!("delta={delta} x={} y={}", p.x, p.y), "ok");
                self.push(Event::Wheel { x: p.x as f32, y: p.y as f32, dy: -delta / 120.0 * WHEEL_STEP });
                Some(LRESULT(0))
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = VIRTUAL_KEY(wp.0 as u16);
                if vk == VK_PROCESSKEY {
                    log::event("key", &format!("vk=0x{:02X} composing={}", vk.0, self.composing), "ime (ignored)");
                    return Some(LRESULT(0));
                }
                match key_of(vk) {
                    Some(key) => {
                        let mods = mods_now();
                        log::event("key", &format!("key={key:?} mods={mods:?} composing={}", self.composing), "sent");
                        self.push(Event::Key { key, mods });
                        Some(LRESULT(0))
                    }
                    None => None,
                }
            }
            WM_CHAR => {
                let unit = wp.0 as u16;
                if (0xD800..0xDC00).contains(&unit) {
                    self.high_surrogate = Some(unit);
                    return Some(LRESULT(0));
                }
                let units: Vec<u16> = match self.high_surrogate.take() {
                    Some(h) if (0xDC00..0xE000).contains(&unit) => vec![h, unit],
                    _ => vec![unit],
                };
                // 制御文字（Backspace・Enter・Tab・Ctrl+文字）は WM_KEYDOWN で受ける
                if let Some(c) = String::from_utf16_lossy(&units).chars().next().filter(|c| !c.is_control()) {
                    log::event("char", &format!("u=0x{unit:04X}"), "insert");
                    self.push(Event::Char(c));
                }
                Some(LRESULT(0))
            }
            WM_IME_STARTCOMPOSITION => {
                self.composing = true;
                self.last_caret = None;
                log::event("ime.start", "", "ok");
                self.need_frame = true;
                // DefWindowProc に渡さない = IME の既定の変換中の窓を出さない（入力欄の中に描く）
                Some(LRESULT(0))
            }
            WM_IME_COMPOSITION => {
                let flags = lp.0 as u32;
                if flags & GCS_RESULTSTR.0 != 0 {
                    if let Some(s) = self.ime_string(GCS_RESULTSTR) {
                        log::event("ime.commit", &format!("text={s}"), "sent");
                        self.push(Event::ImePreedit(String::new()));
                        self.push(Event::ImeCommit(s));
                    }
                }
                if flags & GCS_COMPSTR.0 != 0 {
                    if let Some(s) = self.ime_string(GCS_COMPSTR) {
                        log::event("ime.preedit", &format!("text={s}"), "shown");
                        self.push(Event::ImePreedit(s));
                    }
                }
                Some(LRESULT(0))
            }
            WM_IME_ENDCOMPOSITION => {
                self.composing = false;
                log::event("ime.end", &format!("requests={}", self.ime_requests), "ok");
                self.ime_requests = 0;
                self.push(Event::ImePreedit(String::new()));
                Some(LRESULT(0))
            }
            WM_IME_CHAR => Some(LRESULT(0)),
            WM_IME_REQUEST => {
                let imr = wp.0 as u32;
                self.ime_requests += 1;
                let log_this = self.ime_requests <= IME_REQUEST_LOG_MAX;
                if imr == IMR_QUERYCHARPOSITION && lp.0 != 0 {
                    let info = unsafe { &mut *(lp.0 as *mut IMECHARPOSITION) };
                    let ok = self.answer_char_position(info);
                    if log_this {
                        log::event(
                            "ime.request",
                            &format!("imr=QUERYCHARPOSITION char={}", info.dwCharPos),
                            &format!("answered={ok} pt={},{}", info.pt.x, info.pt.y),
                        );
                    }
                    return ok.then_some(LRESULT(1));
                }
                if log_this {
                    log::event("ime.request", &format!("imr=0x{imr:02X}"), "default");
                }
                None
            }
            WM_APP_A11Y => {
                self.apply_a11y_actions();
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_IME_SETCONTEXT => {
            // IME の既定の変換中の窓を出さない（変換中の文字は入力欄の中に描く）
            let lp = LPARAM(((lp.0 as u32) & !ISC_SHOWUICOMPOSITIONWINDOW) as i32 as isize);
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        }
        WM_GETOBJECT => {
            let n = GETOBJECT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if n <= 5 || n % 100 == 0 {
                log::event("a11y.getobject", &format!("objid={} n={n}", lp.0 as i32), "received");
            }
            // 借りの中で木を作って渡し、UIA への受け渡し（入れ子のメッセージが起きうる）は借りの外で行う
            let reply = with_state(|st| {
                let mut initial = Initial(Some(st.rt.a11y().0));
                st.adapter.as_mut().and_then(|a| a.handle_wm_getobject(wp, lp, &mut initial))
            })
            .flatten();
            if let Some(r) = reply {
                return r.into();
            }
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        }
        _ => {}
    }
    let handled = with_state(|st| st.handle(msg, wp, lp));
    drain_post();
    match handled {
        Some(Some(r)) => r,
        Some(None) => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        None => {
            if STATE.with(|s| s.try_borrow().is_err()) {
                log::event("wndproc.reentrant", &format!("msg=0x{msg:04X}"), "default");
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}

fn dispatch_pending(msg: &mut MSG) -> bool {
    while unsafe { PeekMessageW(msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            return false;
        }
        unsafe {
            let _ = TranslateMessage(msg);
            DispatchMessageW(msg);
        }
    }
    true
}

pub(crate) fn run(options: Options, app: Box<dyn App>) -> Result<(), crate::Error> {
    let started = Instant::now();
    unsafe {
        if let Err(e) = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) {
            log::event("dpi.awareness", "", &format!("error={e:?}"));
        }
    }
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    let class = w!("kargui");
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: class,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&wc) } == 0 {
        log::event("window.class", "", "error=RegisterClassExW");
    }

    // 位置（論理ピクセル）と大きさを、そのモニターの拡大率で物理ピクセルにする
    let (lx, ly) = options.position.map_or((CW_USEDEFAULT, CW_USEDEFAULT), |(x, y)| (x as i32, y as i32));
    let monitor = unsafe { MonitorFromPoint(POINT { x: lx.max(-32000), y: ly.max(-32000) }, MONITOR_DEFAULTTOPRIMARY) };
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    }
    let scale = dx as f32 / 96.0;
    let style = WS_OVERLAPPEDWINDOW;
    let mut r = RECT { left: 0, top: 0, right: (options.size.0 * scale) as i32, bottom: (options.size.1 * scale) as i32 };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), dx);
    }
    let title = HSTRING::from(options.title.as_str());
    let hwnd = unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), class, &title, style, lx, ly, r.right - r.left, r.bottom - r.top, None, None, Some(instance), None)?
    };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client)? };
    let (cw, ch) = (client.right - client.left, client.bottom - client.top);
    log::event(
        "window.create",
        &format!("pos={:?} dpi={dpi} client={cw}x{ch} renderer={:?}", options.position, options.renderer),
        &format!("ms={:.1}", started.elapsed().as_secs_f64() * 1000.0),
    );

    let text = Text::new()?;
    let t_surface = Instant::now();
    let surface = match options.renderer {
        Renderer::Gpu => Surface::Gpu(Gpu::new(hwnd, cw as u32, ch as u32, ATLAS_SIZE)?),
        Renderer::Cpu => Surface::Cpu(Gdi::new(hwnd, cw, ch)?),
    };
    log::event("surface.init", &format!("renderer={:?}", options.renderer), &format!("ms={:.1}", t_surface.elapsed().as_secs_f64() * 1000.0));
    let dark = options.dark.unwrap_or_else(system_dark);
    let mut rt = Runtime::new(text, dark, Box::new(SystemClipboard { hwnd: hwnd.0 as isize }), options.title.clone());
    rt.resize(cw as f32, ch as f32, dpi as f32 / 96.0);

    let actions = Arc::new(Mutex::new(VecDeque::new()));
    let focused = unsafe { GetForegroundWindow() } == hwnd;
    let adapter = Adapter::new(hwnd, focused, ActionSink { hwnd: hwnd.0 as isize, queue: actions.clone() });

    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            hwnd,
            app,
            rt,
            surface: Some(surface),
            adapter: Some(adapter),
            actions,
            post: Vec::new(),
            events: Vec::new(),
            need_frame: true,
            animating: false,
            started,
            shown: false,
            full_rate: options.full_rate,
            window_focused: focused,
            composing: false,
            high_surrogate: None,
            last_caret: None,
            ime_requests: 0,
            summary: Summary::default(),
            background: [0.0; 4],
            dark_title: None,
            tracking_leave: false,
            last_tick: None,
            t_build: 0.0,
            t_draw: 0.0,
            t_wait: 0.0,
            slow_frames: 0,
        })
    });

    unsafe {
        let _ = ShowWindow(hwnd, if options.activate { SW_SHOW } else { SW_SHOWNOACTIVATE });
    }
    drain_post();
    // 1 枚目を描いて出し、画面の合成が終わるのを待ってから、アプリに「表示できた」と知らせる
    with_state(|st| st.frame("first"));
    drain_post();
    unsafe {
        let _ = DwmFlush();
    }
    with_state(|st| {
        st.shown = true;
        st.app.presented(&Frame { number: 0, since_start: st.started.elapsed() });
    });

    let mut msg = MSG::default();
    loop {
        let animating = with_state(|st| st.animating).unwrap_or(false);
        if animating {
            if !dispatch_pending(&mut msg) {
                break;
            }
            with_state(|st| st.tick());
            drain_post();
            with_state(|st| st.wait_vblank());
        } else {
            with_state(|st| st.last_tick = None);
            let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if r.0 == 0 || r.0 == -1 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !dispatch_pending(&mut msg) {
                break;
            }
            with_state(|st| {
                if st.need_frame {
                    st.frame("input");
                }
            });
            drain_post();
        }
    }
    STATE.with(|s| s.borrow_mut().take());
    log::event("exit", "", "ok");
    Ok(())
}
