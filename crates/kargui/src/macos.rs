//! OS との境目の macOS の版（AppKit）。M1: 主の窓を 1 つ開き、CPU の絵を窓の層（CALayer）に写す。マウス・ホイール・
//! キー・文字・拡大率（Retina）・明暗・閉じる前の確認・クリップボード（文字）。日本語入力（M2）、読み上げ・ファイルの
//! ダイアログ・複数の窓・引っ張って落とす（M3）、題の帯・トレイ・通知など（M4）はまだ。
//!
//! 輪は自前: NSApplication の nextEventMatchingMask で、次に起きる時刻まで待つ。届いた知らせを kargui の入力に直して
//! から sendEvent で AppKit にも渡す（題の帯で窓を動かす・大きさを変える・閉じるボタンは AppKit がする）。キーの
//! 知らせは AppKit に渡さない（受け手が無いと警告の音が鳴る）。別のスレッドからは ApplicationDefined の知らせを積んで
//! 起こす。
//!
//! 確かめの道具: 環境変数 KARGUI_SHOT にファイルの場所を入れると、起動から KARGUI_SHOT_MS（既定 1500）ミリ秒の後に、
//! 窓に出した絵を PNG で保存して終わる（CI の Mac で写真を残すため。OS の撮影の許可に左右されない）。

use std::cell::Cell;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadOnly, class, define_class, msg_send};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSPasteboard, NSPasteboardTypeString, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_core_foundation::CFData;
use objc2_core_graphics::{CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_foundation::{MainThreadMarker, NSArray, NSDate, NSDefaultRunLoopMode, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::app::{App, Frame, Options};
use crate::cpu_frame;
use crate::damage::Summary;
use crate::input::Event;
use crate::log;
use crate::mac_keys::{key_of, mods_of, typed};
use crate::runtime::Runtime;
use crate::text::Text;
use crate::ui::context::Clipboard;

/// アニメーションの 1 コマの間（60 Hz）
const ANIMATION_STEP: Duration = Duration::from_millis(16);
/// ホイールの 1 刻み（細かい量を持たないマウス）。Windows と Linux と同じ 96 論理ピクセル
const WHEEL_STEP: f32 = 96.0;
/// 起こす知らせの印（ApplicationDefined の subtype）
const WAKE_SUBTYPE: i16 = 0x6b67; // "kg"

#[derive(Default)]
struct DelegateIvars {
    /// 閉じるボタンが押された（輪が App::close_requested に聞く）
    close_asked: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject には派生の決まりが無く、Drop も持たない
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "KarguiWindowDelegate"]
    #[ivars = DelegateIvars]
    struct WindowDelegate;

    // SAFETY: NSObjectProtocol に決まりは無い
    unsafe impl NSObjectProtocol for WindowDelegate {}

    // SAFETY: 名前と引数の形は AppKit の windowShouldClose: と同じ
    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            // 閉じるかはアプリが決める（App::close_requested）。ここでは閉じずに、輪に知らせる
            self.ivars().close_asked.set(true);
            false
        }
    }
);

impl WindowDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars::default());
        // SAFETY: NSObject の init の形
        unsafe { msg_send![super(this), init] }
    }
}

/// OS のクリップボード（文字だけ）
struct MacClipboard;

impl Clipboard for MacClipboard {
    fn get(&mut self) -> Option<String> {
        let board = NSPasteboard::generalPasteboard();
        // SAFETY: NSPasteboardTypeString は AppKit の定数
        let text = board.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string());
        log::event("clipboard.get", "", &format!("chars={}", text.as_ref().map_or(0, |t| t.chars().count())));
        text
    }

    fn set(&mut self, text: &str) {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        // SAFETY: NSPasteboardTypeString は AppKit の定数
        let ok = board.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
        log::event("clipboard.set", &format!("chars={}", text.chars().count()), &format!("ok={ok}"));
    }
}

/// OS が暗いテーマか（システム設定 > 外観）
fn system_dark(app: &NSApplication) -> bool {
    let appearance = app.effectiveAppearance();
    // SAFETY: 外観の名前は AppKit の定数
    let names = NSArray::from_slice(&[unsafe { NSAppearanceNameAqua }, unsafe { NSAppearanceNameDarkAqua }]);
    let best = appearance.bestMatchFromAppearancesWithNames(&names);
    // SAFETY: 同上
    best.is_some_and(|b| &*b == unsafe { NSAppearanceNameDarkAqua })
}

/// 別のスレッドから輪を起こす（空の ApplicationDefined の知らせを積む）。postEvent:atStart: はどのスレッドから
/// 呼んでもよい（Apple の説明）
fn post_wake() {
    let ev = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        WAKE_SUBTYPE,
        0,
        0,
    );
    if let Some(ev) = ev {
        // SAFETY: 共有の NSApplication に、作った知らせを渡すだけ（どのスレッドからでもよい）
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, postEvent: &*ev, atStart: false];
        }
    }
}

struct Window {
    ns: Retained<NSWindow>,
    view: Retained<NSView>,
    delegate: Retained<WindowDelegate>,
    rt: Runtime,
    /// 窓の中の大きさ（物理ピクセル）と拡大率
    size: (i32, i32),
    scale: f64,
    pixels: Vec<u32>,
    summary: Summary,
    background: crate::geom::Color,
    full: bool,
    events: Vec<Event>,
    need_frame: bool,
    why: &'static str,
    wake_at: Option<Instant>,
    shown: bool,
    created: Instant,
    left_down: bool,
}

impl Window {
    fn push(&mut self, e: Event) {
        self.events.push(e);
        self.need_frame = true;
        self.why = "input";
    }

    /// 窓の中の大きさと拡大率を読み直す。変わったら true
    fn sync_size(&mut self) -> bool {
        let scale = self.ns.backingScaleFactor();
        let r = self.view.bounds();
        let size = ((r.size.width * scale).round().max(1.0) as i32, (r.size.height * scale).round().max(1.0) as i32);
        if size == self.size && (scale - self.scale).abs() < f64::EPSILON {
            return false;
        }
        log::event("window.size", &format!("logical={:.0}x{:.0} scale={scale}", r.size.width, r.size.height), &format!("physical={}x{}", size.0, size.1));
        self.size = size;
        self.scale = scale;
        self.rt.resize(size.0 as f32, size.1 as f32, scale as f32);
        self.full = true;
        true
    }

    /// 窓の座標（左下が原点、論理ピクセル）→ kargui の座標（左上が原点、物理ピクセル）
    fn to_physical(&self, p: NSPoint) -> (f32, f32) {
        let v = self.view.convertPoint_fromView(p, None);
        let h = self.view.bounds().size.height;
        ((v.x * self.scale) as f32, ((h - v.y) * self.scale) as f32)
    }

    /// CPU の絵を描き直して、窓の層に渡す
    fn draw_and_present(&mut self, why: &str) {
        let t = Instant::now();
        let (pw, ph) = self.size;
        if self.pixels.len() != (pw * ph) as usize {
            self.pixels = vec![0; (pw * ph) as usize];
            self.summary = Summary::default();
            self.full = true;
        }
        let bg = self.rt.cx.theme.background;
        let full = self.full || bg != self.background;
        let drawn = cpu_frame::draw(&mut self.pixels, pw, ph, &self.rt.draw, &self.rt.cx.text.atlas, bg, &self.summary, full);
        self.summary = drawn.summary;
        self.background = bg;
        self.full = false;
        let ok = present(&self.view, &self.pixels, pw, ph);
        log::event("render", &format!("why={why} size={pw}x{ph}"), &format!("{} ok={ok} ms={:.1}", drawn.mode, t.elapsed().as_secs_f64() * 1000.0));
    }
}

/// 絵（0x00RRGGBB の並び）を CGImage にして、窓の層の contents にする。暗黙の動き（0.25 秒のふわっと切り替え）は切る
fn present(view: &NSView, pixels: &[u32], w: i32, h: i32) -> bool {
    // 0x00RRGGBB の u32 を小さい方から並べると B G R X。CoreGraphics には「32 ビットの小さい方から・先頭を飛ばす」と伝える
    // SAFETY: u32 の並びを同じ長さのバイトの並びとして読むだけ
    let bytes = unsafe { std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), pixels.len() * 4) };
    let data = CFData::from_bytes(bytes);
    let Some(provider) = CGDataProvider::with_cf_data(Some(&data)) else { return false };
    let Some(space) = CGColorSpace::new_device_rgb() else { return false };
    let info = CGBitmapInfo(CGImageAlphaInfo::NoneSkipFirst.0 | CGImageByteOrderInfo::Order32Little.0);
    // SAFETY: 大きさと行の長さは pixels と合っている
    let image = unsafe { CGImage::new(w as usize, h as usize, 8, 32, w as usize * 4, Some(&space), info, Some(&provider), std::ptr::null(), false, CGColorRenderingIntent::RenderingIntentDefault) };
    let Some(image) = image else { return false };
    // SAFETY: 層は wantsLayer で作った CALayer。CGImage は contents にそのまま渡せる（CF の型は Objective-C の物として扱える）
    unsafe {
        let Some(layer) = view.layer() else { return false };
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
        let contents = (&*image as *const CGImage).cast::<AnyObject>();
        let _: () = msg_send![&*layer, setContents: contents];
        let _: () = msg_send![class!(CATransaction), commit];
    }
    true
}

/// 主の窓を開いて、閉じるまで輪を回す
pub(crate) fn run(options: Options, mut app: Box<dyn App>) -> Result<(), crate::Error> {
    let started = Instant::now();
    let mtm = MainThreadMarker::new().ok_or_else(|| crate::Error("kargui must run on the main thread on macOS".into()))?;
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    ns_app.finishLaunching();
    let dark = options.dark.unwrap_or_else(|| system_dark(&ns_app));
    log::event("theme.system", &format!("dark={dark}"), if options.dark.is_some() { "app" } else { "system" });

    let (lw, lh) = (options.size.0 as f64, options.size.1 as f64);
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable | NSWindowStyleMask::Resizable;
    // SAFETY: 窓を作る AppKit の関数。閉じた時に放さない（下の setReleasedWhenClosed）ので、Retained の持ち方と合う
    let ns = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(lw, lh)), style, NSBackingStoreType::Buffered, false) };
    // SAFETY: Retained で持つので、閉じた時に AppKit に放させない
    unsafe { ns.setReleasedWhenClosed(false) };
    ns.setTitle(&NSString::from_str(&options.title));
    if let Some((mw, mh)) = options.min_size {
        ns.setContentMinSize(NSSize::new(mw as f64, mh as f64));
    }
    ns.setAcceptsMouseMovedEvents(true);
    let delegate = WindowDelegate::new(mtm);
    ns.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let view = ns.contentView().ok_or_else(|| crate::Error("the window has no content view".into()))?;
    view.setWantsLayer(true);
    match options.position {
        // 画面の座標は左下が原点。kargui の位置は左上なので、ここでは真ん中に置く（位置の対応は後で）
        Some(_) | None => ns.center(),
    }

    let text = Text::new()?;
    let mut rt = Runtime::new(text, dark, Box::new(MacClipboard), options.title.clone());
    rt.cx.waker = crate::wake::Waker::new(post_wake);
    rt.cx.text.load_fonts(&options.fonts);
    rt.cx.font = rt.cx.text.fonts.id(&options.font);
    crate::locale::apply_to(&mut rt.cx, &options.title);
    let mut w = Window {
        ns: ns.clone(),
        view: view.clone(),
        delegate,
        rt,
        size: (0, 0),
        scale: 0.0,
        pixels: Vec::new(),
        summary: Summary::default(),
        background: [0.0; 4],
        full: true,
        events: Vec::new(),
        need_frame: true,
        why: "first",
        wake_at: None,
        shown: false,
        created: Instant::now(),
        left_down: false,
    };
    w.sync_size();
    log::event("window.create", &format!("title={:?} size={lw:.0}x{lh:.0}", options.title), &format!("physical={}x{} scale={}", w.size.0, w.size.1, w.scale));
    ns.makeKeyAndOrderFront(None);
    if options.activate {
        ns_app.activate();
    }

    let shot = std::env::var("KARGUI_SHOT").ok().filter(|s| !s.is_empty());
    let shot_at = started + Duration::from_millis(std::env::var("KARGUI_SHOT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1500));
    let mut last_press: Option<(Instant, f32, f32, u8)> = None;
    let mut running = true;
    while running {
        if w.sync_size() {
            w.need_frame = true;
            w.why = "resize";
        }
        if w.wake_at.is_some_and(|t| t <= Instant::now()) {
            w.wake_at = None;
            w.need_frame = true;
            w.why = "timer";
        }
        if w.rt.cx.waker.take_woken() {
            w.need_frame = true;
            w.why = "wake";
        }
        if w.need_frame {
            running &= frame(&mut w, &mut *app, started);
        }
        if let Some(path) = shot.as_ref().filter(|_| Instant::now() >= shot_at) {
            save_shot(&w, path);
            break;
        }
        if w.delegate.ivars().close_asked.replace(false) {
            let ok = app.close_requested();
            log::event("window.close_requested", "", &format!("allowed={ok}"));
            if ok {
                break;
            }
            w.need_frame = true;
            w.why = "close_asked";
        }
        // 次に起きる時刻まで知らせを待つ（描く物が残っていれば待たない）
        let until = if w.need_frame {
            Some(Instant::now())
        } else {
            let mut t = w.wake_at;
            if shot.is_some() {
                t = Some(t.map_or(shot_at, |x| x.min(shot_at)));
            }
            t
        };
        let date = match until {
            Some(t) => NSDate::dateWithTimeIntervalSinceNow(t.saturating_duration_since(Instant::now()).as_secs_f64()),
            None => NSDate::distantFuture(),
        };
        // SAFETY: NSDefaultRunLoopMode は Foundation の定数
        let ev = ns_app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&date), unsafe { NSDefaultRunLoopMode }, true);
        let Some(ev) = ev else { continue };
        let pass = handle(&mut w, &ev, &mut last_press);
        if pass {
            ns_app.sendEvent(&ev);
        }
    }
    log::event("window.close", "", "ok");
    ns.orderOut(None);
    Ok(())
}

/// 1 フレームを回して出す。アプリが終わりを頼んだら false
fn frame(w: &mut Window, app: &mut dyn App, started: Instant) -> bool {
    let why = w.why;
    let events = std::mem::take(&mut w.events);
    let out = w.rt.frame(app, events);
    let quit = std::mem::take(&mut w.rt.cx.quit);
    w.wake_at = out.wake_after.map(|d| Instant::now() + d);
    if out.want_frame {
        w.wake_at = Some(w.wake_at.map_or(Instant::now() + ANIMATION_STEP, |t| t.min(Instant::now() + ANIMATION_STEP)));
    }
    w.need_frame = false;
    // まだ作っていない仕組みの頼みは、記録して捨てる
    if let Some((key, _)) = w.rt.cx.file_dialog_request.take() {
        log::event("file_dialog.request", &format!("key={key}"), "not on macOS yet (M3)");
    }
    for (key, _) in std::mem::take(&mut w.rt.cx.notify_requests) {
        log::event("notify.request", &format!("key={key}"), "not on macOS yet (M4)");
    }
    w.draw_and_present(why);
    let first = !w.shown;
    w.shown = true;
    if first {
        log::event("window.shown", &format!("physical={}x{} scale={}", w.size.0, w.size.1, w.scale), &format!("ms={:.1}", w.created.elapsed().as_secs_f64() * 1000.0));
    }
    let number = if first { 0 } else { w.rt.frames.saturating_sub(1) };
    app.presented(&Frame { number, since_start: started.elapsed() });
    if quit {
        log::event("window.quit", "", "ok");
    }
    !quit
}

/// 知らせを kargui の入力に直す。AppKit にも渡す時は true
fn handle(w: &mut Window, ev: &NSEvent, last_press: &mut Option<(Instant, f32, f32, u8)>) -> bool {
    let ty = ev.r#type();
    let mods = mods_of(ev.modifierFlags().0);
    match ty {
        NSEventType::ApplicationDefined if ev.subtype().0 == WAKE_SUBTYPE => {
            w.need_frame = true;
            w.why = "wake";
            false
        }
        NSEventType::LeftMouseDown => {
            let (x, y) = w.to_physical(ev.locationInWindow());
            if inside(w, x, y) {
                let count = (ev.clickCount().clamp(1, 3)) as u8;
                let target = w.rt.cx.hit(x, y).map(|h| h.id);
                log::event("click", &format!("x={x:.0} y={y:.0} count={count}"), &format!("target={target:?}"));
                w.push(Event::PointerDown { x, y, shift: mods.shift, count });
                w.left_down = true;
                *last_press = Some((Instant::now(), x, y, count));
            }
            true
        }
        NSEventType::LeftMouseUp => {
            let (x, y) = w.to_physical(ev.locationInWindow());
            if w.left_down {
                w.push(Event::PointerUp { x, y });
                w.left_down = false;
            }
            true
        }
        NSEventType::RightMouseUp => {
            let (x, y) = w.to_physical(ev.locationInWindow());
            if inside(w, x, y) {
                log::event("context_menu", &format!("x={x:.0} y={y:.0}"), "sent");
                w.push(Event::ContextMenu { x, y });
            }
            true
        }
        NSEventType::MouseMoved | NSEventType::LeftMouseDragged => {
            let (x, y) = w.to_physical(ev.locationInWindow());
            if w.left_down {
                w.push(Event::PointerMove { x, y });
            }
            let hover = w.rt.hover_at(x, y);
            let bar = w.rt.cx.scrollbar_at(x, y).map(|l| l.id);
            if hover != w.rt.cx.hover || bar != w.rt.cx.bar_hover {
                log::event("hover", &format!("x={x:.0} y={y:.0}"), &format!("id={}", hover.map_or("none".to_string(), |h| format!("{:x}", h.0))));
                w.rt.cx.set_hover(hover);
                w.rt.cx.bar_hover = bar;
                w.need_frame = true;
                w.why = "input";
            }
            true
        }
        NSEventType::ScrollWheel => {
            let (x, y) = w.to_physical(ev.locationInWindow());
            // 上へ回す（指を下へ）と正の量が来る。kargui は下へが正（論理ピクセル）
            let dy = if ev.hasPreciseScrollingDeltas() { -ev.scrollingDeltaY() as f32 } else { -ev.deltaY() as f32 * WHEEL_STEP };
            if dy != 0.0 && inside(w, x, y) {
                log::event("wheel", &format!("x={x:.0} y={y:.0} precise={}", ev.hasPreciseScrollingDeltas()), &format!("dy={dy:.1}"));
                w.push(Event::Wheel { x, y, dy });
            }
            true
        }
        NSEventType::KeyDown => {
            let code = ev.keyCode();
            let repeat = ev.isARepeat();
            if let Some(key) = key_of(code) {
                if !repeat {
                    log::event("key", &format!("code={code} key={key:?} mods={mods:?}"), "sent");
                }
                w.push(Event::Key { key, mods });
            }
            // 文字は Command・Control が無い時だけ（Command+C などの近道は文字を入れない）
            if !mods.ctrl
                && let Some(chars) = ev.characters()
            {
                for c in chars.to_string().chars().filter(|c| typed(*c)) {
                    if !repeat {
                        log::event("char", &format!("u=0x{:04X}", c as u32), "insert");
                    }
                    w.push(Event::Char(c));
                }
            }
            // AppKit に渡すと、受け手の無いキーで警告の音が鳴る
            false
        }
        NSEventType::KeyUp | NSEventType::FlagsChanged => false,
        _ => true,
    }
}

/// 窓の中の点か（題の帯や窓の外の知らせを、部品へ渡さない）
fn inside(w: &Window, x: f32, y: f32) -> bool {
    x >= 0.0 && y >= 0.0 && x < w.size.0 as f32 && y < w.size.1 as f32
}

/// 窓に出した絵を PNG で保存する（KARGUI_SHOT）
fn save_shot(w: &Window, path: &str) {
    let (pw, ph) = w.size;
    let img = crate::test::Image { width: pw as u32, height: ph as u32, pixels: w.pixels.clone() };
    let result = img.save_png(path);
    log::event("shot", &format!("path={path:?} size={pw}x{ph}"), &format!("{result:?}"));
}
