//! 窓（Win32 を直接使う）、メッセージの輪、日本語入力（IMM32）、読み上げ（AccessKit）、描く道の切り替え。
//!
//! 決まり:
//! - 変化が無い間は GetMessage で眠り、自分から起きない（待機中の CPU を 0 にする）
//! - アニメーション（測定のスクロール）の間だけ、窓のあるモニターの垂直同期に合わせて描く
//! - 前面にない窓は、--full-rate が無ければ 2 回に 1 回だけ描く（約 30 fps）
//! - 変換を確定する Enter は IME が受け取り、アプリには VK_PROCESSKEY として届くので、送信しない
//! - 状態を借りたまま、窓へメッセージを送り返す関数（SetWindowPos・DestroyWindow など）を呼ばない。
//!   それらは Post に入れ、借りを返してから行う

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_windows::{Adapter, QueuedEvents};
use taskcore::args::Args;
use taskcore::bench::{ScrollBench, Step};
use taskcore::{Store, WINDOW_SIZE, log};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, Result, w};

use crate::app::{App, BG, Focus, Target, target_of};
use crate::cpu::Cpu;
use crate::draw::{DrawList, IRect};
use crate::gpu::Gpu;
use crate::raster::Canvas;
use crate::text::Text;

const WM_APP_A11Y: u32 = WM_APP + 1;
/// 変換中に手が止まったら、候補の窓の位置を記録するためのタイマー
const IME_SHOT_TIMER: usize = 7;
const IME_SHOT_DELAY_MS: u32 = 600;
const IME_SHOT_MAX: u32 = 8;
/// WM_GETOBJECT（読み上げソフトなどからの問い合わせ）を受けた回数
static GETOBJECT_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RendererKind {
    D3d11,
    Cpu,
}

impl RendererKind {
    pub fn from_args(a: &Args) -> Self {
        if a.software || a.renderer.as_deref() == Some("cpu") { RendererKind::Cpu } else { RendererKind::D3d11 }
    }
    pub fn name(self) -> &'static str {
        match self {
            RendererKind::D3d11 => "d3d11",
            RendererKind::Cpu => "cpu",
        }
    }
}

enum Renderer {
    Gpu(Gpu),
    Cpu(Cpu),
}

/// 状態の借りを返してから行うこと
enum Post {
    SetWindowPos((HWND, RECT)),
    Destroy(HWND),
    Raise(QueuedEvents),
    /// 日本語入力を使える（true）か使えない（false）か
    AssociateIme((HWND, bool)),
    /// 文字を打つ位置を IME に知らせる（IME からの入れ子のメッセージが届くので、借りの外で行う）
    ImePosition((HWND, IRect)),
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

struct State {
    hwnd: HWND,
    app: App,
    text: Text,
    list: DrawList,
    renderer: Option<Renderer>,
    kind: RendererKind,
    adapter: Option<Adapter>,
    actions: Arc<Mutex<VecDeque<ActionRequest>>>,
    post: Vec<Post>,
    /// 全体を描き直す
    full_dirty: bool,
    /// 最後に描いたときのスクロールの位置（物理ピクセル）
    drawn_scroll: Option<f32>,
    main_started: Instant,
    ready_at: Option<Instant>,
    bench: Option<ScrollBench>,
    full_rate: bool,
    window_focused: bool,
    composing: bool,
    /// UTF-16 の上位の代用文字（WM_CHAR が 2 回に分けて届く文字のため）
    high_surrogate: Option<u16>,
    vblank_count: u64,
    last_caret: Option<IRect>,
    /// 記録の置き場所（候補の窓の写真もここに置く）
    log_dir: PathBuf,
    ime_shots: u32,
    /// 遅いフレームの内訳を記録するための、前のフレームの時間（ミリ秒）
    last_tick_at: Option<Instant>,
    t_build: f64,
    t_draw: f64,
    t_wait: f64,
    slow_frames: u32,
    /// 今の変換の間に届いた IME の問い合わせの数（記録は最初の数回と、変換の終わりの合計だけ）
    ime_requests: u32,
}

/// 変換ごとに記録する IME の問い合わせの数
const IME_REQUEST_LOG_MAX: u32 = 3;

/// これより長くフレームの間が空いたら、内訳を記録する（60 Hz の 1.5 フレーム）
const SLOW_FRAME_MS: f64 = 25.0;

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
            Post::SetWindowPos((hwnd, r)) => unsafe {
                let res = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                log::event("window.setpos", &format!("rect={},{},{},{}", r.left, r.top, r.right, r.bottom), &format!("{res:?}"));
            },
            Post::Destroy(hwnd) => unsafe {
                let res = DestroyWindow(hwnd);
                log::event("window.destroy_call", "", &format!("{res:?}"));
            },
            Post::Raise(events) => events.raise(),
            Post::AssociateIme((hwnd, on)) => unsafe {
                let flags = if on { IACE_DEFAULT } else { 0 };
                let ok = ImmAssociateContextEx(hwnd, HIMC::default(), flags);
                log::event("ime.associate", &format!("on={on}"), &format!("ok={}", ok.as_bool()));
            },
            Post::ImePosition((hwnd, caret)) => set_ime_position(hwnd, caret),
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
        let comp = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT { x: caret.x, y: caret.y },
            rcArea: RECT::default(),
        };
        let cand = CANDIDATEFORM {
            dwIndex: 0,
            dwStyle: CFS_EXCLUDE,
            ptCurrentPos: POINT { x: caret.x, y: caret.bottom() },
            rcArea: RECT { left: caret.x, top: caret.y, right: caret.right(), bottom: caret.bottom() },
        };
        let a = ImmSetCompositionWindow(himc, &comp);
        let b = ImmSetCandidateWindow(himc, &cand);
        let _ = ImmReleaseContext(hwnd, himc);
        let mut screen = POINT { x: caret.x, y: caret.bottom() };
        let _ = ClientToScreen(hwnd, &mut screen);
        log::event(
            "ime.caret",
            &format!("client={},{} h={} screen_bottom_left={},{}", caret.x, caret.y, caret.h, screen.x, screen.y),
            &format!("comp={} cand={}", a.as_bool(), b.as_bool()),
        );
    }
}

fn lo(v: isize) -> i32 {
    (v & 0xffff) as i16 as i32
}
fn hi(v: isize) -> i32 {
    ((v >> 16) & 0xffff) as i16 as i32
}

impl State {
    fn animating(&self) -> bool {
        self.ready_at.is_some() && self.bench.as_ref().is_some_and(|b| !b.is_done())
    }

    fn set_focus(&mut self, f: Option<Focus>, why: &str) {
        if self.app.focus == f {
            return;
        }
        let was_input = self.app.focus == Some(Focus::Input);
        self.app.focus = f;
        let is_input = f == Some(Focus::Input);
        if was_input != is_input {
            self.post.push(Post::AssociateIme((self.hwnd, is_input)));
        }
        log::event("focus", &format!("to={f:?} why={why}"), "ok");
        self.full_dirty = true;
    }

    fn act(&mut self, target: Target, how: &str) {
        let result = match target {
            Target::Input => {
                self.set_focus(Some(Focus::Input), how);
                "focus".to_string()
            }
            Target::Add => {
                self.set_focus(Some(Focus::Add), how);
                match self.app.add_from_input() {
                    Some(id) => format!("added id={id}"),
                    None => "skipped=empty".into(),
                }
            }
            Target::Check(id) => format!("toggled={:?}", self.app.toggle(id)),
            Target::Delete(id) => format!("removed={}", self.app.remove(id)),
            Target::List | Target::Nothing => "nothing".into(),
        };
        log::event("act", &format!("target={target:?} how={how}"), &result);
        self.full_dirty = true;
    }

    fn submit(&mut self, how: &str) {
        let text = self.app.input.clone();
        match self.app.add_from_input() {
            Some(id) => log::event("add", &format!("text={} how={how}", text.trim()), &format!("id={id}")),
            None => log::event("add", &format!("text= how={how}"), "skipped=empty"),
        }
        self.full_dirty = true;
    }

    fn resize(&mut self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.app.width = width as f32;
        self.app.height = height as f32;
        self.app.set_scroll(self.app.scroll);
        let res = match self.renderer.as_mut() {
            Some(Renderer::Gpu(g)) => g.resize(width as u32, height as u32),
            Some(Renderer::Cpu(c)) => c.resize(width, height),
            None => Ok(()),
        };
        if let Err(e) = res {
            log::event("resize", &format!("size={width}x{height}"), &format!("error={e:?}"));
        }
        self.full_dirty = true;
    }

    /// 描く。CPU の道では、スクロールだけが変わったときは絵をずらして帯だけを描く
    fn render(&mut self, why: &str) {
        let t = Instant::now();
        let made_before = (self.text.layouts_made, self.text.glyphs_made);
        self.app.build(&mut self.text, &mut self.list);
        self.t_build = t.elapsed().as_secs_f64() * 1000.0;
        let t_draw = Instant::now();
        let scroll = self.app.scroll_px();
        let full = self.full_dirty || self.drawn_scroll.is_none();
        let mut mode = "none";
        match self.renderer.as_mut() {
            Some(Renderer::Gpu(g)) => {
                if let Err(e) = g.render(&self.list, &mut self.text.atlas, BG, 0) {
                    log::event("render", &format!("why={why}"), &format!("error={e:?}"));
                }
                mode = "full";
            }
            Some(Renderer::Cpu(c)) => {
                let (w, h) = (c.width, c.height);
                let list_rect = IRect::covering(self.app.layout().list);
                let atlas = &self.text.atlas;
                let mut canvas = Canvas { px: c.pixels(), width: w, height: h };
                let rects = if full {
                    canvas.draw(&self.list, &atlas.pixels, atlas.size, IRect::new(0, 0, w, h), BG);
                    mode = "full";
                    vec![IRect::new(0, 0, w, h)]
                } else if self.drawn_scroll != Some(scroll) {
                    let dy = (scroll - self.drawn_scroll.unwrap_or(scroll)) as i32;
                    let strip = canvas.scroll_up(list_rect, dy);
                    canvas.draw(&self.list, &atlas.pixels, atlas.size, strip, BG);
                    mode = "scroll";
                    vec![list_rect]
                } else {
                    vec![]
                };
                c.present(&rects);
            }
            None => return,
        }
        self.t_draw = t_draw.elapsed().as_secs_f64() * 1000.0;
        if self.animating() && self.t_build + self.t_draw > SLOW_FRAME_MS * 0.5 {
            log::event(
                "frame.heavy",
                &format!(
                    "mode={mode} new_layouts={} new_glyphs={}",
                    self.text.layouts_made - made_before.0,
                    self.text.glyphs_made - made_before.1
                ),
                &format!("build_ms={:.2} draw_ms={:.2}", self.t_build, self.t_draw),
            );
        }
        self.drawn_scroll = Some(scroll);
        self.full_dirty = false;
        if self.animating() {
            // 測定の最中は 1 枚ごとの記録をしない（記録の書き込みが測定に入らないように）
        } else {
            log::event(
                "render",
                &format!("why={why} mode={mode} quads={}", self.list.quads.len()),
                &format!("ms={:.2} layouts={} glyphs={}", t.elapsed().as_secs_f64() * 1000.0, self.text.layouts_made, self.text.glyphs_made),
            );
        }
        if let Some(adapter) = self.adapter.as_mut() {
            let app = &self.app;
            if let Some(events) = adapter.update_if_active(|| app.a11y_tree()) {
                self.post.push(Post::Raise(events));
            }
        }
        self.update_ime_position();
    }

    /// 文字を打つ位置を IME に知らせる（候補の窓をそこに出すため）。変わったときだけ
    fn update_ime_position(&mut self) {
        if self.app.focus != Some(Focus::Input) {
            return;
        }
        let c = self.app.caret;
        let caret = IRect::new(c.x as i32, c.y as i32, c.w as i32, c.h as i32);
        if self.last_caret == Some(caret) {
            return;
        }
        self.last_caret = Some(caret);
        self.post.push(Post::ImePosition((self.hwnd, caret)));
    }

    /// IME の「変換中の何文字目は画面のどこか」の問い合わせに答える。
    /// 文字ごとの位置はまだ持っていないので、変換中の文字の範囲を文字数で等分して近い位置を返す
    fn answer_char_position(&self, info: &mut IMECHARPOSITION) {
        let p = self.app.preedit_rect;
        let count = self.app.preedit.encode_utf16().count().max(1) as f32;
        let x = p.x + p.w * (info.dwCharPos as f32 / count).min(1.0);
        let mut pt = POINT { x: x.round() as i32, y: p.y as i32 };
        let r = self.app.input_rect;
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

    /// 測定のスクロール。描くたびに呼ぶ
    fn tick(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_tick_at {
            let gap = (now - prev).as_secs_f64() * 1000.0;
            if gap > SLOW_FRAME_MS {
                self.slow_frames += 1;
                // 前のフレームの内訳: 作る・描く・垂直同期の待ち。残りはメッセージの処理か、OS に止められた時間
                let rest = gap - self.t_build - self.t_draw - self.t_wait;
                log::event(
                    "frame.slow",
                    &format!("n={} gap_ms={gap:.1}", self.slow_frames),
                    &format!(
                        "build_ms={:.2} draw_ms={:.2} wait_ms={:.2} other_ms={rest:.1}",
                        self.t_build, self.t_draw, self.t_wait
                    ),
                );
            }
        }
        self.last_tick_at = Some(now);
        let Some(ready) = self.ready_at else { return };
        let Some(bench) = self.bench.as_mut() else { return };
        match bench.tick(ready.elapsed().as_secs_f32()) {
            Step::Wait => {}
            Step::Scroll { offset, first } => {
                if first {
                    log::event("bench.scroll.start", "", "ok");
                }
                let max = self.app.max_scroll();
                self.app.scroll = offset.min(max);
            }
            Step::Finished { frames, secs, max_gap_ms } => {
                log::event(
                    "bench.scroll.done",
                    &format!("secs={secs:.3}"),
                    &format!("frames={frames} fps={:.1} max_gap_ms={max_gap_ms:.1}", frames as f32 / secs),
                );
            }
            Step::Idle => {}
        }
        self.render("bench");
    }

    fn wait_vblank(&mut self) {
        // 前面にない窓は、--full-rate が無ければ 2 回に 1 回だけ描く（約 30 fps）
        let waits = if self.window_focused || self.full_rate { 1 } else { 2 };
        let t = Instant::now();
        for _ in 0..waits {
            self.vblank_count += 1;
            match self.renderer.as_mut() {
                Some(Renderer::Gpu(g)) => g.wait_vblank(),
                Some(Renderer::Cpu(c)) => c.wait_vblank(),
                None => {}
            }
        }
        self.t_wait = t.elapsed().as_secs_f64() * 1000.0;
    }

    fn forget_output(&mut self) {
        match self.renderer.as_mut() {
            Some(Renderer::Gpu(g)) => g.forget_output(),
            Some(Renderer::Cpu(c)) => c.forget_output(),
            None => {}
        }
    }

    fn apply_a11y_actions(&mut self) {
        let requests: Vec<ActionRequest> = self.actions.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
        for r in requests {
            let target = target_of(r.target_node);
            match r.action {
                accesskit::Action::Click => self.act(target, "a11y.click"),
                accesskit::Action::Focus => match target {
                    Target::Input => self.set_focus(Some(Focus::Input), "a11y.focus"),
                    Target::Add => self.set_focus(Some(Focus::Add), "a11y.focus"),
                    _ => {}
                },
                other => log::event("a11y.action", &format!("action={other:?}"), "ignored"),
            }
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
                // 窓が残っているうちに、GPU の物と読み上げを片付ける（GPUI のエラーを避ける順番）
                self.adapter = None;
                let had = self.renderer.take().is_some();
                log::event("window.destroy", "", &format!("renderer_dropped={had}"));
                unsafe { PostQuitMessage(0) };
                Some(LRESULT(0))
            }
            WM_SIZE => {
                let (w, h) = (lo(lp.0), hi(lp.0));
                log::event("window.size", &format!("size={w}x{h} kind={}", wp.0), "ok");
                self.resize(w, h);
                if self.ready_at.is_some() && w > 0 && h > 0 {
                    self.render("size");
                }
                Some(LRESULT(0))
            }
            WM_PAINT => {
                match self.renderer.as_ref() {
                    Some(Renderer::Cpu(c)) => c.paint(),
                    _ => unsafe {
                        let _ = ValidateRect(Some(self.hwnd), None);
                        self.full_dirty = true;
                    },
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xffff) as f32;
                self.app.scale = dpi / 96.0;
                self.text.reset();
                self.last_caret = None;
                let r = unsafe { *(lp.0 as *const RECT) };
                log::event("window.dpi", &format!("dpi={dpi}"), "ok");
                self.post.push(Post::SetWindowPos((self.hwnd, r)));
                self.full_dirty = true;
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
                self.full_dirty = true;
                None
            }
            WM_LBUTTONDOWN => {
                let (x, y) = (lo(lp.0) as f32, hi(lp.0) as f32);
                let target = self.app.hit(x, y);
                log::event("click", &format!("x={x} y={y}"), &format!("target={target:?}"));
                match target {
                    Target::Input => self.set_focus(Some(Focus::Input), "click"),
                    Target::Add => self.act(Target::Add, "click"),
                    Target::Check(_) | Target::Delete(_) => self.act(target, "click"),
                    Target::List | Target::Nothing => {}
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = hi(wp.0 as isize) as f32;
                let before = self.app.scroll;
                self.app.set_scroll(self.app.scroll - delta / 120.0 * 96.0);
                log::event("wheel", &format!("delta={delta}"), &format!("scroll={}→{}", before, self.app.scroll));
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                let vk = VIRTUAL_KEY(wp.0 as u16);
                let result = match vk {
                    VK_PROCESSKEY => "ime (ignored)",
                    VK_RETURN => match self.app.focus {
                        Some(Focus::Input) if !self.composing => {
                            self.submit("enter");
                            "submit"
                        }
                        Some(Focus::Input) => "composing (ignored)",
                        Some(Focus::Add) => {
                            self.act(Target::Add, "enter");
                            "add"
                        }
                        None => "no focus",
                    },
                    VK_SPACE if self.app.focus == Some(Focus::Add) => {
                        self.act(Target::Add, "space");
                        "add"
                    }
                    VK_TAB => {
                        let next = match self.app.focus {
                            Some(Focus::Input) => Some(Focus::Add),
                            _ => Some(Focus::Input),
                        };
                        self.set_focus(next, "tab");
                        "focus"
                    }
                    _ => "",
                };
                if !result.is_empty() {
                    log::event("key", &format!("vk=0x{:02X} composing={}", vk.0, self.composing), result);
                }
                Some(LRESULT(0))
            }
            WM_CHAR => {
                let unit = wp.0 as u16;
                if self.app.focus != Some(Focus::Input) {
                    return Some(LRESULT(0));
                }
                if (0xD800..0xDC00).contains(&unit) {
                    self.high_surrogate = Some(unit);
                    return Some(LRESULT(0));
                }
                let units: Vec<u16> = match self.high_surrogate.take() {
                    Some(h) if (0xDC00..0xE000).contains(&unit) => vec![h, unit],
                    _ => vec![unit],
                };
                let s = String::from_utf16_lossy(&units);
                let result = match unit {
                    0x08 => {
                        self.app.input.pop();
                        "backspace"
                    }
                    0x0D | 0x0A | 0x09 | 0x1B => "control (ignored)",
                    u if u < 0x20 => "control (ignored)",
                    _ => {
                        self.app.input.push_str(&s);
                        "insert"
                    }
                };
                log::event("char", &format!("u=0x{unit:04X}"), result);
                self.full_dirty = true;
                Some(LRESULT(0))
            }
            WM_IME_STARTCOMPOSITION => {
                self.composing = true;
                self.last_caret = None;
                self.update_ime_position();
                log::event("ime.start", "", "ok");
                // DefWindowProc に渡さない = IME の既定の変換中の窓を出さない（入力欄の中に描く）
                Some(LRESULT(0))
            }
            WM_IME_COMPOSITION => {
                let flags = lp.0 as u32;
                if flags & GCS_RESULTSTR.0 != 0 {
                    if let Some(s) = self.ime_string(GCS_RESULTSTR) {
                        if self.app.focus == Some(Focus::Input) {
                            self.app.input.push_str(&s);
                        }
                        log::event("ime.commit", &format!("text={s}"), &format!("input={}", self.app.input));
                    }
                }
                if flags & GCS_COMPSTR.0 != 0 {
                    if let Some(s) = self.ime_string(GCS_COMPSTR) {
                        log::event("ime.preedit", &format!("text={s}"), "shown");
                        self.app.preedit = s;
                    }
                    // 手が止まったら候補の窓の位置を記録する
                    unsafe { SetTimer(Some(self.hwnd), IME_SHOT_TIMER, IME_SHOT_DELAY_MS, None) };
                } else if flags & GCS_RESULTSTR.0 != 0 {
                    self.app.preedit.clear();
                }
                self.full_dirty = true;
                Some(LRESULT(0))
            }
            WM_IME_ENDCOMPOSITION => {
                self.composing = false;
                self.app.preedit.clear();
                log::event("ime.end", &format!("requests={}", self.ime_requests), "ok");
                self.ime_requests = 0;
                self.full_dirty = true;
                Some(LRESULT(0))
            }
            WM_IME_CHAR => Some(LRESULT(0)),
            WM_IME_NOTIFY => {
                log::event("ime.notify", &format!("imn=0x{:02X}", wp.0), "default");
                None
            }
            WM_IME_REQUEST => {
                let imr = wp.0 as u32;
                self.ime_requests += 1;
                let log_this = self.ime_requests <= IME_REQUEST_LOG_MAX;
                if imr == IMR_QUERYCHARPOSITION && lp.0 != 0 {
                    let info = unsafe { &mut *(lp.0 as *mut IMECHARPOSITION) };
                    self.answer_char_position(info);
                    if log_this {
                        log::event(
                            "ime.request",
                            &format!("imr=QUERYCHARPOSITION char={}", info.dwCharPos),
                            &format!("pt={},{} line_h={}", info.pt.x, info.pt.y, info.cLineHeight),
                        );
                    }
                    return Some(LRESULT(1));
                }
                if log_this {
                    log::event("ime.request", &format!("imr=0x{imr:02X}"), "default");
                }
                None
            }
            WM_TIMER if wp.0 == IME_SHOT_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), IME_SHOT_TIMER);
                }
                if self.composing && self.ime_shots < IME_SHOT_MAX {
                    self.ime_shots += 1;
                    // 候補の窓は変換している文節の頭に出るので、変換中の文字の頭を基準に撮る
                    let c = self.app.caret;
                    let mut p = POINT { x: self.app.preedit_rect.x as i32, y: c.bottom() as i32 };
                    unsafe {
                        let _ = ClientToScreen(self.hwnd, &mut p);
                    }
                    crate::imeshot::shot(self.hwnd, p.x, p.y, &self.log_dir, self.ime_shots);
                }
                Some(LRESULT(0))
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
            // 読み上げソフトなどの問い合わせの数を記録する（最初の 5 回と、以後 100 回ごと）
            let n = GETOBJECT_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n <= 5 || n % 100 == 0 {
                log::event("a11y.getobject", &format!("objid={} n={n}", lp.0 as i32), "received");
            }
            // 借りの中で木を作って渡し、UIA への受け渡し（入れ子のメッセージが起きうる）は借りの外で行う
            let reply = with_state(|st| {
                let mut initial = Initial(Some(st.app.a11y_tree()));
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
            // 状態を借りている最中に届いた入れ子のメッセージ。既定の処理に任せ、記録だけ残す
            if STATE.with(|s| s.try_borrow().is_err()) {
                log::event("wndproc.reentrant", &format!("msg=0x{msg:04X}"), "default");
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}

/// 窓を作って、閉じられるまでメッセージの輪を回す
pub fn run(a: Args, kind: RendererKind, main_started: Instant, log_dir: PathBuf) -> Result<()> {
    let t0 = Instant::now();
    unsafe {
        if let Err(e) = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) {
            log::event("dpi.awareness", "", &format!("error={e:?}"));
        }
    }
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    let class = w!("kargui-spike");
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
    let (lx, ly) = a.pos.map(|(x, y)| (x as i32, y as i32)).unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));
    let monitor = unsafe { MonitorFromPoint(POINT { x: lx.max(-32000), y: ly.max(-32000) }, MONITOR_DEFAULTTONEAREST) };
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    }
    let scale = dx as f32 / 96.0;
    let style = WS_OVERLAPPEDWINDOW;
    // 窓の中の大きさ（論理ピクセル）。既定は前回の比べと同じ 900×640。
    // 大きい窓での重さを測るときは、環境変数 KARGUI_SIZE=1800x1000 のように変える
    let (sw, sh) = std::env::var("KARGUI_SIZE")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))
        })
        .unwrap_or(WINDOW_SIZE);
    let mut r = RECT { left: 0, top: 0, right: (sw * scale) as i32, bottom: (sh * scale) as i32 };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), dx);
    }
    let title = HSTRING::from(format!("タスク帳（kargui 試作 ・ {}）", kind.name()));
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            &title,
            style,
            if a.pos.is_some() { lx } else { CW_USEDEFAULT },
            if a.pos.is_some() { ly } else { CW_USEDEFAULT },
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    let t_window = t0.elapsed().as_secs_f64() * 1000.0;
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client)? };
    let (cw, ch) = (client.right - client.left, client.bottom - client.top);
    log::event(
        "window.create",
        &format!("pos={:?} dpi={dpi} client={cw}x{ch}", a.pos),
        &format!("ms={t_window:.1}"),
    );

    let text = Text::new()?;
    let t_r = Instant::now();
    let renderer = match kind {
        RendererKind::D3d11 => Renderer::Gpu(Gpu::new(hwnd, cw as u32, ch as u32, crate::text::ATLAS_SIZE)?),
        RendererKind::Cpu => Renderer::Cpu(Cpu::new(hwnd, cw, ch)?),
    };
    let t_renderer = t_r.elapsed().as_secs_f64() * 1000.0;

    let mut store = Store::new();
    if a.items > 0 {
        let t = Instant::now();
        store.generate(a.items);
        log::event("generate", &format!("n={}", a.items), &format!("ms={:.1}", t.elapsed().as_secs_f64() * 1000.0));
    }
    let mut app = App::new(store, kind.name());
    app.scale = dpi as f32 / 96.0;
    app.width = cw as f32;
    app.height = ch as f32;

    let actions = Arc::new(Mutex::new(VecDeque::new()));
    let focused = unsafe { GetForegroundWindow() } == hwnd;
    let adapter = Adapter::new(hwnd, focused, ActionSink { hwnd: hwnd.0 as isize, queue: actions.clone() });

    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            hwnd,
            app,
            text,
            list: DrawList::default(),
            renderer: Some(renderer),
            kind,
            adapter: Some(adapter),
            actions,
            post: Vec::new(),
            full_dirty: true,
            drawn_scroll: None,
            main_started,
            ready_at: None,
            bench: a.bench_scroll.map(ScrollBench::new),
            full_rate: a.full_rate,
            window_focused: focused,
            composing: false,
            high_surrogate: None,
            vblank_count: 0,
            last_caret: None,
            log_dir,
            ime_shots: 0,
            last_tick_at: None,
            t_build: 0.0,
            t_draw: 0.0,
            t_wait: 0.0,
            slow_frames: 0,
            ime_requests: 0,
        })
    });

    unsafe {
        let _ = ShowWindow(hwnd, if a.no_focus { SW_SHOWNOACTIVATE } else { SW_SHOW });
    }
    drain_post();
    // 1 枚目を描いて出し、画面の合成が終わるのを待ってから「表示できた」とする
    with_state(|st| st.render("first"));
    drain_post();
    unsafe {
        let _ = DwmFlush();
    }
    with_state(|st| {
        st.ready_at = Some(Instant::now());
        log::event(
            "ready",
            &format!("renderer={} items={}", st.kind.name(), st.app.store.tasks().len()),
            &format!(
                "since_main_ms={} window_ms={t_window:.1} renderer_ms={t_renderer:.1}",
                st.main_started.elapsed().as_millis()
            ),
        );
    });

    let mut msg = MSG::default();
    'outer: loop {
        let animating = with_state(|st| st.animating()).unwrap_or(false);
        if animating {
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                if msg.message == WM_QUIT {
                    break 'outer;
                }
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            with_state(|st| st.tick());
            drain_post();
            with_state(|st| st.wait_vblank());
        } else {
            let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if r.0 == 0 || r.0 == -1 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                if msg.message == WM_QUIT {
                    break 'outer;
                }
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            with_state(|st| {
                let scrolled = st.drawn_scroll != Some(st.app.scroll_px());
                if st.full_dirty || scrolled {
                    st.render(if st.full_dirty { "input" } else { "scroll" });
                }
            });
            drain_post();
        }
    }
    let leftover = STATE.with(|s| s.borrow_mut().take());
    if let Some(st) = leftover {
        log::event("exit.state", &format!("vblanks={}", st.vblank_count), "dropped");
    }
    Ok(())
}
