//! OS との境目の macOS の版（AppKit）。M1: 主の窓を 1 つ開き、CPU の絵を窓の層（CALayer）に写す。マウス・ホイール・
//! キー・文字・拡大率（Retina）・明暗・閉じる前の確認・クリップボード（文字）。M2: 日本語入力（NSTextInputClient）。
//! 読み上げ・ファイルのダイアログ・複数の窓・引っ張って落とす（M3）、題の帯・トレイ・通知など（M4）はまだ。
//!
//! 日本語入力: 窓の中身は自前の NSView（KarguiView）で、NSTextInputClient を持つ。入力欄に焦点がある時だけ、キーを
//! sendEvent で AppKit に渡し、窓を通って届いた KarguiView の keyDown: が interpretKeyEvents で IME に通す。IME は setMarkedText（変換中）・insertText（確定、または IME を使わない打ち方の
//! 文字）・doCommandBySelector（IME が使わなかったキー。矢印・Enter など）で返すので、それを kargui の入力にして
//! KarguiView の待ち行列に積み、輪が取り出す（呼び返しは State を借りない）。候補の窓を出す場所は、フレームの後に
//! 変換中の範囲かキャレットの場所を KarguiView に写しておき、firstRectForCharacterRange で答える。Command・Control の
//! 近道、パスワードの欄、入力欄が無い時は IME を通さない。
//!
//! 輪は自前: NSApplication の nextEventMatchingMask で、次に起きる時刻まで待つ。届いた知らせを kargui の入力に直して
//! から sendEvent で AppKit にも渡す（題の帯で窓を動かす・大きさを変える・閉じるボタンは AppKit がする）。キーの
//! 知らせは AppKit に渡さない（受け手が無いと警告の音が鳴る）。別のスレッドからは ApplicationDefined の知らせを積んで
//! 起こす。窓の縁を引っ張って大きさを変える間は、AppKit が sendEvent の中で自分の輪を回すので、その間の描き直しは
//! 窓の委任先の windowDidResize: から回す（窓と App は State にまとめ、輪と委任先の両方から借りる）。
//!
//! 確かめの道具: 環境変数 KARGUI_SHOT にファイルの場所を入れると、起動から KARGUI_SHOT_MS（既定 1500）ミリ秒の後に、
//! 窓に出した絵を PNG で保存して終わる（CI の Mac で写真を残すため。OS の撮影の許可に左右されない）。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{ClassType, DefinedClass, MainThreadOnly, class, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSMarkedClauseSegmentAttributeName, NSPasteboard, NSUnderlineColorAttributeName, NSUnderlineStyleAttributeName, NSPasteboardTypeString, NSResponder, NSTextInputClient, NSTextInputContext, NSView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_core_foundation::CFData;
use objc2_core_graphics::{CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSDate, NSDefaultRunLoopMode, NSNotFound, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString, NSUInteger,
};

use crate::app::{App, Frame, Options};
use crate::cpu_frame;
use crate::damage::Summary;
use crate::input::{Event, Key, Mods};
use crate::log;
use crate::mac_keys::{key_of, mods_of, typed};
use crate::runtime::Runtime;
use crate::text::Text;
use crate::ui::Id;
use crate::ui::context::Clipboard;

/// アニメーションの 1 コマの間（60 Hz）
const ANIMATION_STEP: Duration = Duration::from_millis(16);
/// ホイールの 1 刻み（細かい量を持たないマウス）。Windows と Linux と同じ 96 論理ピクセル
const WHEEL_STEP: f32 = 96.0;
/// 起こす知らせの印（ApplicationDefined の subtype）
const WAKE_SUBTYPE: i16 = 0x6b67; // "kg"

/// 窓と App。kargui の輪のほかに、AppKit が自分の輪を回している間の呼び返し（引っ張って大きさを変える間の
/// windowDidResize: など）からも使うので、Rc<RefCell<…>> で持つ。kargui の輪は sendEvent の間は借りない
struct State {
    w: Window,
    app: Box<dyn App>,
    started: Instant,
    /// アプリが終わりを頼んだ（呼び返しの中のフレームで頼まれた時も、輪が読んで終わる）
    quit: bool,
}

#[derive(Default)]
struct DelegateIvars {
    /// 閉じるボタンが押された（輪が App::close_requested に聞く）
    close_asked: Cell<bool>,
    /// 窓が前面（キーを受ける窓）になった・外れた。輪が読んで記録する
    focus: Cell<Option<bool>>,
    /// 引っ張って大きさを変えている間か
    live_resize: Cell<bool>,
    /// 引っ張って大きさを変えている間に描き直した回数
    live_frames: Cell<u32>,
    /// 窓と App（弱い参照。窓が委任先を持つので、強い参照にすると輪になる）
    state: RefCell<Weak<RefCell<State>>>,
}

impl DelegateIvars {
    /// 窓と App を借りる。kargui の輪が借りている間（kargui 自身の処理から AppKit が呼び返した時）は None
    fn with_state(&self, f: impl FnOnce(&mut State)) {
        let Some(state) = self.state.borrow().upgrade() else { return };
        let Ok(mut s) = state.try_borrow_mut() else { return };
        f(&mut s);
    }
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

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            self.ivars().focus.set(Some(true));
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            self.ivars().focus.set(Some(false));
        }

        // 引っ張って大きさを変える間は、AppKit が sendEvent の中で自分の輪を回し、kargui の輪は止まる。その間の
        // 描き直しは windowDidResize: から回す
        #[unsafe(method(windowWillStartLiveResize:))]
        fn window_will_start_live_resize(&self, _notification: &NSNotification) {
            let iv = self.ivars();
            iv.live_resize.set(true);
            iv.live_frames.set(0);
            log::event("window.live_resize", "start", "ok");
            // 窓の中身の角を押して始まった時、その押下は kargui に渡さない（放した知らせは AppKit の輪が受け取るので
            // kargui に届かず、kargui が押したままだと思い続ける）
            iv.with_state(|s| {
                if s.w.left_down {
                    if let Some(i) = s.w.events.iter().rposition(|e| matches!(e, Event::PointerDown { .. })) {
                        s.w.events.remove(i);
                    }
                    s.w.left_down = false;
                    log::event("pointer.taken_by_window", "live_resize", "ok");
                }
            });
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            let iv = self.ivars();
            // 引っ張っている間でなければ（タイル配置の動きなど）、kargui の輪が読む
            if !iv.live_resize.get() {
                return;
            }
            iv.with_state(|s| {
                if s.w.sync_size() {
                    s.w.why = "live_resize";
                    if !frame(&mut s.w, &mut *s.app, s.started) {
                        s.quit = true;
                    }
                    iv.live_frames.set(iv.live_frames.get() + 1);
                }
            });
        }

        #[unsafe(method(windowDidEndLiveResize:))]
        fn window_did_end_live_resize(&self, _notification: &NSNotification) {
            let iv = self.ivars();
            iv.live_resize.set(false);
            log::event("window.live_resize", "end", &format!("frames={}", iv.live_frames.get()));
        }

        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            log::event("window.move", "", "ok");
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

#[derive(Default)]
struct ViewIvars {
    /// 今の変換中の文字（IME が setMarkedText で渡した物。空なら変換していない）
    marked: RefCell<String>,
    /// IME の呼び返しが作った入力（輪が interpretKeyEvents の後に取り出す）
    queue: RefCell<Vec<Event>>,
    /// 今 IME に通しているキー（IME が使わなかった時と、IME を使わない打ち方の時に、アプリへ渡す）
    key: Cell<Option<(Key, Mods)>>,
    /// IME が何か返した（変換中の文字・確定・使わなかったキー）
    answered: Cell<bool>,
    /// 今のキーを sendEvent で AppKit に渡し、keyDown: で IME に通す
    via_appkit: Cell<bool>,
    /// keyDown: に届いた
    reached: Cell<bool>,
    /// 候補の窓を出す場所（窓の中身の座標。論理ピクセル、左下が原点）
    ime_rect: Cell<NSRect>,
    /// IME が setMarkedText で選んだ範囲（変換中の文字の中の UTF-16 の位置と長さ）
    selected: Cell<NSRange>,
}

define_class!(
    // SAFETY: NSView の派生の決まり（init は initWithFrame:）に従う。Drop は持たない
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KarguiView"]
    #[ivars = ViewIvars]
    struct KarguiView;

    impl KarguiView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        // 入力欄に焦点がある時のキーは、輪が sendEvent で AppKit に渡し、窓を通ってここに届く（窓を通すと、AppKit が
        // この NSView の入力の受け手を今の受け手にする。輪から直接 interpretKeyEvents を呼ぶと、入力ソースが日本語でも
        // IME が変換を始めず、打った英字がそのまま入った）。ほかのキーは輪が直接読み、ここでは何もしない（受け手が
        // 無いと警告の音が鳴る）
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let iv = self.ivars();
            if iv.via_appkit.get() {
                iv.reached.set(true);
                // interpretKeyEvents は「入力の受け手の handleEvent（IME）→ 使わなければキーの割り当て（insertText・
                // doCommandBySelector）」の順。IME が受け取ったかを記録するため、handleEvent を先に呼ぶ
                let ctx = self.inputContext();
                let handled = ctx.as_ref().is_some_and(|c| c.handleEvent(event));
                log::event(
                    "ime.key",
                    &format!("code={} source={:?}", event.keyCode(), ctx.as_ref().and_then(|c| c.selectedKeyboardInputSource()).map(|s| s.to_string())),
                    &format!("handled_by_input_method={handled}"),
                );
                if !handled {
                    self.interpretKeyEvents(&NSArray::from_slice(&[event]));
                }
            }
        }
    }

    // SAFETY: NSObjectProtocol に決まりは無い
    unsafe impl NSObjectProtocol for KarguiView {}

    // SAFETY: 名前と引数の形は AppKit の NSTextInputClient と同じ（位置と長さは UTF-16 の単位）
    unsafe impl NSTextInputClient for KarguiView {
        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            let has = !self.ivars().marked.borrow().is_empty();
            log::event("ime.query", "hasMarkedText", &format!("{has}"));
            has
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            let n = self.ivars().marked.borrow().encode_utf16().count();
            log::event("ime.query", "markedRange", &format!("0+{n}"));
            if n > 0 { NSRange::new(0, n) } else { NSRange::new(NSNotFound as NSUInteger, 0) }
        }

        // IME から見た文書は変換中の文字だけにする（入力欄の前後の文字は渡さない。Wayland の版と同じ）。キャレットは
        // 長さ 0 の選択なので、「選択が無い」（NSNotFound）ではなく位置を答える（NSNotFound と答えると、テキスト
        // エディットでは変換できる同じ手順で、日本語入力が変換せずに英字をそのまま確定した）
        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            let iv = self.ivars();
            let r = if iv.marked.borrow().is_empty() { NSRange::new(0, 0) } else { iv.selected.get() };
            log::event("ime.query", "selectedRange", &format!("{}+{}", r.location, r.length));
            r
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(&self, string: &AnyObject, selected: NSRange, _replacement: NSRange) {
            let (text, starts) = marked_parts(string);
            let iv = self.ivars();
            iv.selected.set(selected);
            let shown = iv.marked.replace(text.clone());
            let clauses = crate::ime::mac_clauses(&text, &starts, (selected.location, selected.length));
            log::event(
                "ime.preedit",
                &format!("text={text:?} selected={}+{} segments={starts:?}", selected.location, selected.length),
                &format!("clauses={:?} target={:?} cursor={:?}", clauses.bounds, clauses.target, clauses.cursor),
            );
            iv.queue.borrow_mut().extend(crate::ime::preedit_events(&shown, text, clauses));
            iv.answered.set(true);
        }

        // 変換中の文字をそのまま確定する（Apple の決まり）
        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            let iv = self.ivars();
            let text = iv.marked.take();
            log::event("ime.unmark", &format!("text={text:?}"), if text.is_empty() { "nothing" } else { "commit" });
            if !text.is_empty() {
                iv.queue.borrow_mut().extend([Event::ImePreedit(String::new()), Event::ImeCommit(text)]);
            }
            if let Some(ctx) = self.inputContext() {
                ctx.discardMarkedText();
            }
        }

        // 変換中の文字に IME が付けてよい属性。変換中の文字は下線で表すので、下線と下線の色を受け付けると答える
        // （Chromium と同じ）。文節の区切りだけと答えると、日本語入力（Kotoeri）はキーを受け取っても変換中の文字を
        // 作らず、打った英字をそのまま確定した。kargui は下線を自分で描くので、受け取った属性のうち使うのは文節の区切りだけ
        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            log::event("ime.query", "validAttributesForMarkedText", "underline, underline color, clause segment");
            // SAFETY: AppKit の定数
            unsafe { NSArray::from_slice(&[NSUnderlineStyleAttributeName, NSUnderlineColorAttributeName, NSMarkedClauseSegmentAttributeName]) }
        }

        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring_for_proposed_range(&self, range: NSRange, _actual: *mut NSRange) -> Option<Retained<NSAttributedString>> {
            log::event("ime.query", &format!("attributedSubstringForProposedRange {}+{}", range.location, range.length), "none");
            None
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index_for_point(&self, _point: NSPoint) -> NSUInteger {
            0
        }

        // 候補の窓を出す場所（画面の座標）
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect_for_character_range(&self, range: NSRange, _actual: *mut NSRange) -> NSRect {
            let r = self.ivars().ime_rect.get();
            let screen = match self.window() {
                Some(win) => win.convertRectToScreen(self.convertRect_toView(r, None)),
                None => r,
            };
            log::event(
                "ime.first_rect",
                &format!("range={}+{}", range.location, range.length),
                &format!("screen=({:.0}, {:.0}, {:.0}, {:.0})", screen.origin.x, screen.origin.y, screen.size.width, screen.size.height),
            );
            screen
        }

        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _replacement: NSRange) {
            let (text, _) = marked_parts(string);
            let iv = self.ivars();
            let key = iv.key.take();
            let was_marked = !iv.marked.take().is_empty();
            let mut q = iv.queue.borrow_mut();
            if was_marked {
                // 変換の確定。確定に使ったキー（Enter など）はアプリへ渡さない
                log::event("ime.commit", &format!("text={text:?}"), "commit");
                q.push(Event::ImePreedit(String::new()));
                q.push(Event::ImeCommit(text));
            } else {
                // IME を使わない打ち方（英字の配列、日本語入力の英数）: キーと文字
                if let Some((key, mods)) = key {
                    q.push(Event::Key { key, mods });
                }
                for c in text.chars().filter(|c| typed(*c)) {
                    log::event("char", &format!("u=0x{:04X}", c as u32), "insert");
                    q.push(Event::Char(c));
                }
            }
            iv.answered.set(true);
        }

        // IME が使わなかったキー（矢印・Enter・Backspace・Tab・Esc など）。アプリへ渡す
        #[unsafe(method(doCommandBySelector:))]
        fn do_command_by_selector(&self, selector: Sel) {
            let iv = self.ivars();
            if let Some((key, mods)) = iv.key.take() {
                log::event("key", &format!("key={key:?} mods={mods:?} command={}", selector.name().to_string_lossy()), "sent");
                iv.queue.borrow_mut().push(Event::Key { key, mods });
            }
            iv.answered.set(true);
        }
    }
);

impl KarguiView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars::default());
        // SAFETY: NSView の init の形
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// このキーを IME に通す用意をする（輪は続けて sendEvent でキーを AppKit に渡し、keyDown: が IME に通す）
    fn begin_key(&self, key: Option<(Key, Mods)>) {
        let iv = self.ivars();
        // AppKit の今の入力の受け手が、この NSView の物か（違えば受け手にする）
        let ours = self.inputContext();
        let current = NSTextInputContext::currentInputContext(MainThreadMarker::from(self));
        let same = matches!((&ours, &current), (Some(a), Some(b)) if std::ptr::eq::<NSTextInputContext>(&**a, &**b));
        if !same {
            log::event("ime.context", &format!("current_is_ours=false current={}", current.is_some()), "activate");
            if let Some(ctx) = &ours {
                ctx.activate();
            }
        }
        iv.key.set(key);
        iv.answered.set(false);
        iv.reached.set(false);
        iv.via_appkit.set(true);
    }

    /// sendEvent の後に、IME が返した入力を取り出す。IME が何も返さなければ、キーをそのままアプリへ渡す
    fn finish_key(&self) -> Vec<Event> {
        let iv = self.ivars();
        if !iv.via_appkit.replace(false) {
            return Vec::new();
        }
        if !iv.reached.get() {
            log::event("ime.key", "", "keyDown: was not reached (AppKit kept the key)");
        }
        let mut out = iv.queue.take();
        if !iv.answered.get()
            && let Some((key, mods)) = iv.key.get()
        {
            log::event("key", &format!("key={key:?} mods={mods:?}"), "sent (the input method returned nothing)");
            out.push(Event::Key { key, mods });
        }
        iv.key.set(None);
        out
    }

    /// 入力欄から焦点が外れた（別の欄へ移った）時、変換中の文字を捨てる
    fn discard_marked(&self) {
        if self.ivars().marked.take().is_empty() {
            return;
        }
        if let Some(ctx) = self.inputContext() {
            ctx.discardMarkedText();
        }
        log::event("ime.discard", "focus moved", "ok");
    }
}

/// IME が渡す文字（NSString か NSAttributedString）から、文字と、文節の区切りの始まり（UTF-16 の位置）を取り出す
fn marked_parts(string: &AnyObject) -> (String, Vec<usize>) {
    if let Some(attr) = string.downcast_ref::<NSAttributedString>() {
        let len = attr.length();
        let mut starts = Vec::new();
        let mut i = 0;
        while i < len {
            let mut range = NSRange::new(0, 0);
            // SAFETY: i は文字の中、range は書き込める場所。属性の名前は AppKit の定数
            let value = unsafe { attr.attribute_atIndex_longestEffectiveRange_inRange(NSMarkedClauseSegmentAttributeName, i, &mut range, NSRange::new(0, len)) };
            if value.is_some() {
                starts.push(range.location);
            }
            i = (range.location + range.length).max(i + 1);
        }
        (attr.string().to_string(), starts)
    } else if let Some(s) = string.downcast_ref::<NSString>() {
        (s.to_string(), Vec::new())
    } else {
        (String::new(), Vec::new())
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
    view: Retained<KarguiView>,
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
    /// 題の帯に頼んだ明暗（None はまだ頼んでいない）
    dark_title: Option<bool>,
    /// 前のフレームで IME を向けていた入力欄
    ime_target: Option<Id>,
    /// 入力の受け手が使っている入力ソース（記録のため）
    ime_source: String,
}

impl Window {
    /// フレームの後に、IME を向ける入力欄と、候補の窓を出す場所を KarguiView に写す
    fn sync_ime(&mut self) {
        let target = self.rt.cx.ime_target();
        if target != self.ime_target {
            self.view.discard_marked();
            log::event("ime.focus", &format!("to={}", target.map_or("none".to_string(), |t| format!("{:x}", t.0))), "ok");
            self.ime_target = target;
        }
        // 入力の受け手が今使っている入力ソース（日本語入力・英字など）。変わった時に記録する
        if let Some(source) = self.view.inputContext().and_then(|ctx| ctx.selectedKeyboardInputSource()).map(|s| s.to_string())
            && source != self.ime_source
        {
            log::event("ime.source", &format!("from={:?}", self.ime_source), &format!("to={source:?}"));
            self.ime_source = source;
        }
        // 変換中なら変換中の範囲、そうでなければキャレット（物理ピクセル、左上が原点）
        let Some(c) = self.rt.cx.preedit_rect.or(self.rt.cx.caret) else { return };
        let s = self.scale.max(1.0);
        let h = self.view.bounds().size.height;
        let r = NSRect::new(NSPoint::new(c.x as f64 / s, h - (c.y + c.h) as f64 / s), NSSize::new((c.w as f64 / s).max(1.0), c.h as f64 / s));
        let iv = self.view.ivars();
        if r != iv.ime_rect.get() {
            iv.ime_rect.set(r);
            if let Some(ctx) = self.view.inputContext() {
                ctx.invalidateCharacterCoordinates();
            }
        }
    }

    /// 題の帯の明暗を kargui の明暗に合わせる（アプリが OS と違う明暗を選んでも、題の帯が中身と同じになる）
    fn sync_title_appearance(&mut self) {
        let want = self.rt.cx.dark;
        if self.dark_title == Some(want) {
            return;
        }
        self.dark_title = Some(want);
        // SAFETY: 外観の名前は AppKit の定数
        let name = if want { unsafe { NSAppearanceNameDarkAqua } } else { unsafe { NSAppearanceNameAqua } };
        let appearance = NSAppearance::appearanceNamed(name);
        let ok = appearance.is_some();
        self.ns.setAppearance(appearance.as_deref());
        log::event("window.dark_title", &format!("dark={want}"), &format!("ok={ok}"));
    }

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
pub(crate) fn run(options: Options, app: Box<dyn App>) -> Result<(), crate::Error> {
    let started = Instant::now();
    let mtm = MainThreadMarker::new().ok_or_else(|| crate::Error("kargui must run on the main thread on macOS".into()))?;
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // [NSApp run] は通らない（起動の時に一度 run を通る GLFW の形と CI で比べ、日本語入力の振る舞いは変わらなかった）
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
    // 中身は自前の NSView（日本語入力の受け手）。窓の大きさに合わせて AppKit が広げる
    let view = KarguiView::new(mtm, NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(lw, lh)));
    ns.setContentView(Some(view.as_super()));
    view.setWantsLayer(true);
    let first_responder = ns.makeFirstResponder(Some(view.as_super().as_super()));
    log::event("view.first_responder", "KarguiView", &format!("ok={first_responder}"));
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
        dark_title: None,
        ime_target: None,
        ime_source: String::new(),
    };
    w.sync_size();
    w.sync_title_appearance();
    log::event("window.create", &format!("title={:?} size={lw:.0}x{lh:.0}", options.title), &format!("physical={}x{} scale={}", w.size.0, w.size.1, w.scale));
    ns.makeKeyAndOrderFront(None);
    if options.activate {
        // 端末などから起動したアプリは、前面のアプリが譲らないと activate（macOS 14 からの穏やかな頼み方）では前面に
        // ならない（CI の Mac では Finder が前面のまま、窓は灰色の題の帯だった）。winit と同じく、ほかのアプリより先に
        // 前面にする古い頼み方を使う
        #[allow(deprecated)]
        ns_app.activateIgnoringOtherApps(true);
        log::event("app.activate", "ignoring_other_apps=true", &format!("active_now={}", ns_app.isActive()));
    }
    // アプリの束の名前（cargo が作った裸の実行ファイルでは無い。IME がつながるかに関わるかもしれないので残す）
    let bundle = objc2_foundation::NSBundle::mainBundle();
    log::event("app.bundle", &format!("path={:?}", bundle.bundlePath().to_string()), &format!("identifier={:?}", bundle.bundleIdentifier().map(|s| s.to_string())));

    let state = Rc::new(RefCell::new(State { w, app, started, quit: false }));
    state.borrow().w.delegate.ivars().state.replace(Rc::downgrade(&state));

    let shot = std::env::var("KARGUI_SHOT").ok().filter(|s| !s.is_empty());
    let shot_at = started + Duration::from_millis(std::env::var("KARGUI_SHOT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1500));
    let mut last_press: Option<(Instant, f32, f32, u8)> = None;
    loop {
        let date = {
            let mut guard = state.borrow_mut();
            let s = &mut *guard;
            let w = &mut s.w;
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
            if w.need_frame && !frame(w, &mut *s.app, s.started) {
                s.quit = true;
            }
            if s.quit {
                break;
            }
            if let Some(path) = shot.as_ref().filter(|_| Instant::now() >= shot_at) {
                save_shot(w, path);
                break;
            }
            if let Some(focused) = w.delegate.ivars().focus.take() {
                log::event("window.focus", &format!("focused={focused}"), &format!("app_active={}", ns_app.isActive()));
            }
            if w.delegate.ivars().close_asked.replace(false) {
                let ok = s.app.close_requested();
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
            match until {
                Some(t) => NSDate::dateWithTimeIntervalSinceNow(t.saturating_duration_since(Instant::now()).as_secs_f64()),
                None => NSDate::distantFuture(),
            }
        };
        // SAFETY: NSDefaultRunLoopMode は Foundation の定数
        let ev = ns_app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&date), unsafe { NSDefaultRunLoopMode }, true);
        let Some(ev) = ev else { continue };
        let pass = handle(&mut state.borrow_mut().w, &ev, &mut last_press);
        if pass {
            // 借りずに渡す（AppKit が中で自分の輪を回し、委任先が State を借りることがある）
            ns_app.sendEvent(&ev);
            // IME に通したキーの結果
            let mut guard = state.borrow_mut();
            let w = &mut guard.w;
            for e in w.view.finish_key() {
                w.push(e);
            }
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
    w.sync_title_appearance();
    w.sync_ime();
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
            } else {
                // 題の帯や窓の縁（大きさを変える所）。AppKit に任せる
                log::event("pointer.outside", &format!("x={x:.0} y={y:.0}"), "to AppKit");
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
            // 入力欄に焦点があり、近道（Command・Control）でなければ、AppKit に渡して IME に通す（KarguiView の keyDown:）
            if !mods.ctrl && w.rt.cx.ime_target().is_some() {
                w.view.begin_key(key_of(code).map(|k| (k, mods)));
                return true;
            }
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
