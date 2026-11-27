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
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSMarkedClauseSegmentAttributeName, NSModalResponseOK, NSOpenPanel, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSPasteboard, NSPasteboardTypeFileURL, NSUnderlineColorAttributeName, NSUnderlineStyleAttributeName, NSPasteboardTypeString, NSResponder, NSSavePanel, NSTextInputClient, NSTextInputContext, NSView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_core_foundation::CFData;
use objc2_core_graphics::{CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSDate, NSDefaultRunLoopMode, NSNotFound, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString, NSUInteger, NSURL,
};

use crate::app::{App, Frame, Options};
use crate::cpu_frame;
use crate::damage::Summary;
use crate::input::{A11yAction, Event, Key, Mods};
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
    /// 開いている窓。0 番が主の窓、ほかは Ui::window で頼まれた窓（key を持つ）
    windows: Vec<Window>,
    app: Box<dyn App>,
    started: Instant,
    /// アプリが終わりを頼んだ（呼び返しの中のフレームで頼まれた時も、輪が読んで終わる）
    quit: bool,
    /// アプリが渡した書体のファイル（別の窓の Runtime にも読ませる）
    fonts: Vec<std::borrow::Cow<'static, [u8]>>,
}

impl State {
    /// この委任先を持つ窓
    fn index_of(&self, d: &WindowDelegate) -> Option<usize> {
        self.windows.iter().position(|w| std::ptr::eq(&*w.delegate, d))
    }
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
    /// OS の明暗かハイコントラストの設定が変わった（輪が読み直す）
    system_changed: Cell<bool>,
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

    impl WindowDelegate {
        // OS の明暗（AppleInterfaceThemeChangedNotification）とハイコントラスト（NSWorkspace の
        // AccessibilityDisplayOptionsDidChange）が変わった。輪が読み直す
        #[unsafe(method(systemSettingsChanged:))]
        fn system_settings_changed(&self, notification: &NSNotification) {
            log::event("system.changed", &format!("name={}", notification.name()), "reread on the next loop");
            self.ivars().system_changed.set(true);
            post_wake();
        }
    }

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
                let Some(i) = s.index_of(self) else { return };
                let w = &mut s.windows[i];
                if w.left_down {
                    if let Some(k) = w.events.iter().rposition(|e| matches!(e, Event::PointerDown { .. })) {
                        w.events.remove(k);
                    }
                    w.left_down = false;
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
                let Some(i) = s.index_of(self) else { return };
                let State { windows, app, started, quit, .. } = s;
                let w = &mut windows[i];
                if w.sync_size() {
                    w.why = "live_resize";
                    if !frame(w, &mut **app, *started) {
                        *quit = true;
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
    /// IME（入力の受け手の handleEvent）の中にいる（外から来た insertText の時に残っている変換中の文字は置き去り）
    in_ime: Cell<bool>,
    /// 今のキーの文字（NSEvent の characters）
    key_chars: RefCell<String>,
    /// IME（入力の受け手の handleEvent）が今のキーを受け取った
    handled: Cell<bool>,
    /// 入力の受け手が使っている入力ソース
    source: RefCell<String>,
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
                // 入力ソースは IME に渡した後で読む（先に読んだ回は、参考のアプリが変換できた同じ Mac で、kargui だけ
                // 英字のままだった。1 例なので確かではない）
                iv.key_chars.replace(event.characters().map(|s| s.to_string()).unwrap_or_default());
                iv.in_ime.set(true);
                let handled = ctx.as_ref().is_some_and(|c| c.handleEvent(event));
                iv.in_ime.set(false);
                iv.handled.set(handled);
                let source = ctx.as_ref().and_then(|c| c.selectedKeyboardInputSource()).map(|s| s.to_string()).unwrap_or_default();
                self.note_source(&source);
                log::event("ime.key", &format!("code={} source={source:?}", event.keyCode()), &format!("handled_by_input_method={handled}"));
                if !handled {
                    self.interpretKeyEvents(&NSArray::from_slice(&[event]));
                }
            }
        }
    }

    // SAFETY: NSObjectProtocol に決まりは無い
    unsafe impl NSObjectProtocol for KarguiView {}

    // ほかのアプリ（Finder など）からファイルを引っ張って落とす。届け出る（registerForDraggedTypes）のは、ファイルを
    // 受け取る部品が初めて現れた時（Windows の版と同じ）。知らせは待ち行列に積み、輪を起こす（State は借りない）
    // SAFETY: 名前と引数の形は AppKit の NSDraggingDestination と同じ
    unsafe impl NSDraggingDestination for KarguiView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            let (x, y) = self.drag_point(sender);
            let paths = dragged_paths(sender);
            log::event("drag.enter", &format!("x={x:.0} y={y:.0}"), &format!("files={}", paths.len()));
            if paths.is_empty() {
                return NSDragOperation::None;
            }
            self.ivars().queue.borrow_mut().push(Event::FileDragEnter { x, y, paths });
            self.wake_if_outside_key();
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            let (x, y) = self.drag_point(sender);
            self.ivars().queue.borrow_mut().push(Event::FileDragOver { x, y });
            self.wake_if_outside_key();
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            log::event("drag.leave", "", "ok");
            self.ivars().queue.borrow_mut().push(Event::FileDragLeave);
            self.wake_if_outside_key();
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let (x, y) = self.drag_point(sender);
            log::event("drag.drop", &format!("x={x:.0} y={y:.0}"), "ok");
            self.ivars().queue.borrow_mut().push(Event::FileDrop { x, y });
            self.wake_if_outside_key();
            true
        }
    }

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
            self.wake_if_outside_key();
        }

        // 変換中の文字をそのまま確定する（Apple の決まり）
        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            let iv = self.ivars();
            let text = iv.marked.take();
            log::event("ime.unmark", &format!("text={text:?}"), if text.is_empty() { "nothing" } else { "commit" });
            if !text.is_empty() {
                iv.queue.borrow_mut().extend([Event::ImePreedit(String::new()), Event::ImeCommit(text)]);
                self.wake_if_outside_key();
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
            let marked = iv.marked.take();
            // キーの文字そのもの（英字の配列へ切り替えた後の「o」など）は確定ではない。入力の受け手は英字の配列でも
            // handleEvent の中で insertText するので、IME の中かどうかでは見分けられなかった
            let key_text = !marked.is_empty() && iv.via_appkit.get() && text != marked && *iv.key_chars.borrow() == text;
            let left_over = self.left_over_marked() || key_text;
            let mut q = iv.queue.borrow_mut();
            if !marked.is_empty() && !left_over {
                // 変換の確定（IME の中から、またはキーの外で後から届いた）。確定に使ったキー（Enter など）はアプリへ渡さない
                log::event("ime.commit", &format!("text={text:?}"), "commit");
                q.push(Event::ImePreedit(String::new()));
                q.push(Event::ImeCommit(text));
            } else {
                // IME を使わない打ち方（英字の配列、日本語入力の英数）: キーと文字。日本語入力から英字の配列へ切り替えた
                // 後に変換中の文字が残っていたら、先にそのまま確定する（この insertText を確定と取り違えると消える）
                if !marked.is_empty() {
                    log::event("ime.commit", &format!("text={marked:?}"), "commit (left over; the input method is no longer active)");
                    q.push(Event::ImePreedit(String::new()));
                    q.push(Event::ImeCommit(marked));
                }
                if let Some((key, mods)) = key {
                    q.push(Event::Key { key, mods });
                }
                for c in text.chars().filter(|c| typed(*c)) {
                    log::event("char", &format!("u=0x{:04X}", c as u32), "insert");
                    q.push(Event::Char(c));
                }
            }
            iv.answered.set(true);
            self.wake_if_outside_key();
        }

        // IME が使わなかったキー（矢印・Enter・Backspace・Tab・Esc など）。アプリへ渡す
        #[unsafe(method(doCommandBySelector:))]
        fn do_command_by_selector(&self, selector: Sel) {
            let iv = self.ivars();
            // 英字の配列へ切り替えた後に置き去りの変換中の文字があれば、先にそのまま確定する
            if self.left_over_marked() {
                let marked = iv.marked.take();
                if !marked.is_empty() {
                    log::event("ime.commit", &format!("text={marked:?}"), "commit (left over; the input method is no longer active)");
                    iv.queue.borrow_mut().extend([Event::ImePreedit(String::new()), Event::ImeCommit(marked)]);
                }
            }
            if let Some((key, mods)) = iv.key.take() {
                log::event("key", &format!("key={key:?} mods={mods:?} command={}", selector.name().to_string_lossy()), "sent");
                iv.queue.borrow_mut().push(Event::Key { key, mods });
            }
            iv.answered.set(true);
            self.wake_if_outside_key();
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
        iv.handled.set(false);
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
            if iv.handled.get() {
                // IME が受け取った。結果は後から届くことがある（CI の Mac では、変換中の Enter の確定が届かなかった）。
                // アプリへ渡すと、変換中に Enter などが欄の操作として効いてしまう
                log::event("ime.key", &format!("key={key:?}"), "consumed by the input method (no answer yet)");
            } else {
                log::event("key", &format!("key={key:?} mods={mods:?}"), "sent (the input method returned nothing)");
                out.push(Event::Key { key, mods });
            }
        }
        iv.key.set(None);
        out
    }

    /// 入力の受け手が使っている入力ソースを覚えて記録する（確定には使わない。入力の受け手は入力ソースを遅れて知らせる）。
    /// 変わったら true
    fn note_source(&self, source: &str) -> bool {
        let iv = self.ivars();
        if source.is_empty() || *iv.source.borrow() == source {
            return false;
        }
        let old = iv.source.replace(source.to_string());
        log::event("ime.source", &format!("from={old:?}"), &format!("to={source:?}"));
        true
    }

    /// 今の呼び返しは、キーの中なのに IME（handleEvent）の外から来た（英字の配列のキーの割り当て）。その時に残っている
    /// 変換中の文字は置き去り（入力の受け手は入力ソースを遅れて知らせるので、入力ソースでは見分けられなかった）
    fn left_over_marked(&self) -> bool {
        let iv = self.ivars();
        iv.via_appkit.get() && !iv.in_ime.get()
    }

    /// IME の呼び返しがキーの外で届いた（後から届く確定など）時、輪を起こして待ち行列を取り出させる
    fn wake_if_outside_key(&self) {
        if !self.ivars().via_appkit.get() {
            log::event("ime.async", "", "a callback outside a key; wake the loop");
            post_wake();
        }
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

    /// 引っ張っている所（窓の座標、左下が原点）→ kargui の座標（左上が原点、物理ピクセル）
    fn drag_point(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> (f32, f32) {
        let v = self.convertPoint_fromView(sender.draggingLocation(), None);
        let h = self.bounds().size.height;
        let scale = self.window().map_or(1.0, |w| w.backingScaleFactor());
        ((v.x * scale) as f32, ((h - v.y) * scale) as f32)
    }
}

/// 引っ張りの板（pasteboard）の項目のうち、ファイルの URL を場所にする
fn dragged_paths(sender: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<std::path::PathBuf> {
    let Some(items) = sender.draggingPasteboard().pasteboardItems() else { return Vec::new() };
    items
        .iter()
        // SAFETY: NSPasteboardTypeFileURL は AppKit の定数
        .filter_map(|item| item.stringForType(unsafe { NSPasteboardTypeFileURL }))
        .filter_map(|s| NSURL::URLWithString(&s))
        .filter_map(|u| u.path())
        .map(|p| std::path::PathBuf::from(p.to_string()))
        .collect()
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

/// 読み上げ（VoiceOver）からの頼みの受け手。呼ばれるのは主のスレッドで、AppKit の呼び返しの中なので State は借りず、
/// 待ち行列に積んで輪を起こす（X11 の版の A11ySink と同じ形）
#[derive(Clone, Default)]
struct MacA11ySink {
    actions: Rc<RefCell<Vec<accesskit::ActionRequest>>>,
}

impl accesskit::ActivationHandler for MacA11ySink {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        // 木は次のフレームで渡す（update_if_active に全部の木を渡す）
        log::event("a11y.activate", "", "pending (tree on next frame)");
        post_wake();
        None
    }
}

impl accesskit::ActionHandler for MacA11ySink {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        log::event("a11y.action", &format!("action={:?} node={:?}", request.action, request.target_node), "queued");
        self.actions.borrow_mut().push(request);
        post_wake();
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

/// 好みの言語の並び（システム設定の言語と地域。「ja-JP」「en-US」など）。locale.rs が OS の言語を決めるのに使う
pub(crate) fn preferred_languages() -> Vec<String> {
    objc2_foundation::NSLocale::preferredLanguages().iter().map(|s| s.to_string()).collect()
}

/// 知らせの時に OS が暗いテーマか（ユーザーの設定 AppleInterfaceStyle。知らせの直後は NSApp の effectiveAppearance が
/// まだ変わっていないことがあるので、設定を直接読む）
fn system_dark_setting() -> bool {
    objc2_foundation::NSUserDefaults::standardUserDefaults().stringForKey(&NSString::from_str("AppleInterfaceStyle")).is_some_and(|s| s.to_string() == "Dark")
}

/// OS のハイコントラスト（システム設定 > アクセシビリティ > ディスプレイ > コントラストを上げる）
fn system_high_contrast() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast()
}

/// アプリのメニュー（隠す ⌘H・終了 ⌘Q）。終了は主の窓を閉じる（閉じる前の確認 App::close_requested を通る）
fn install_menu(mtm: MainThreadMarker, app: &NSApplication, window: &NSWindow, japanese: bool) {
    let name = objc2_app_kit::NSRunningApplication::currentApplication().localizedName().map_or_else(|| "kargui".to_string(), |s| s.to_string());
    let item = |title: String, action: Sel, key: &str| {
        // SAFETY: NSMenuItem の init の形（題・動き・キー）
        unsafe { objc2_app_kit::NSMenuItem::initWithTitle_action_keyEquivalent(objc2_app_kit::NSMenuItem::alloc(mtm), &NSString::from_str(&title), Some(action), &NSString::from_str(key)) }
    };
    let (hide, quit) = if japanese { (format!("{name}を隠す"), format!("{name}を終了")) } else { (format!("Hide {name}"), format!("Quit {name}")) };
    let menu = objc2_app_kit::NSMenu::new(mtm);
    menu.addItem(&item(hide, objc2::sel!(hide:), "h"));
    let quit = item(quit, objc2::sel!(performClose:), "q");
    // SAFETY: 窓は輪が終わるまで生きる
    unsafe { quit.setTarget(Some(window)) };
    menu.addItem(&quit);
    let app_item = objc2_app_kit::NSMenuItem::new(mtm);
    app_item.setSubmenu(Some(&menu));
    let bar = objc2_app_kit::NSMenu::new(mtm);
    bar.addItem(&app_item);
    app.setMainMenu(Some(&bar));
    log::event("menu.install", &format!("name={name:?} japanese={japanese}"), "hide, quit");
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
    /// None は主の窓。ほかの窓は Ui::window の鍵
    key: Option<String>,
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
    /// 出すのを待っているファイルのダイアログ（頼んだ時の鍵と中身）
    pending_dialog: Option<(String, crate::file_dialog::FileDialog)>,
    /// 読み上げの橋渡し（KarguiView を動的に派生させて NSAccessibility の関数を足す）と、読み上げからの頼み
    a11y: accesskit_macos::SubclassingAdapter,
    a11y_actions: Rc<RefCell<Vec<accesskit::ActionRequest>>>,
    /// ファイルを落とせる窓だと OS に届け出た
    files_registered: bool,
    /// OS の明暗に従う（アプリが Options::dark で決めていない）
    follow_dark: bool,
}

impl Window {
    /// 記録に付ける窓の名前（主の窓は空）
    fn tag(&self) -> String {
        self.key.as_ref().map_or(String::new(), |k| format!(" win={k}"))
    }

    /// フレームの前に、大きさ・時刻・起こし・IME の後から届く入力・読み上げの頼み・前面の変化を読む
    fn before_frame(&mut self, app_active: bool) {
        if self.sync_size() {
            self.need_frame = true;
            self.why = "resize";
        }
        if self.wake_at.is_some_and(|t| t <= Instant::now()) {
            self.wake_at = None;
            self.need_frame = true;
            self.why = "timer";
        }
        if self.rt.cx.waker.take_woken() {
            self.need_frame = true;
            self.why = "wake";
        }
        // キーの外で届いた IME の入力（後から届く確定など）と、読み上げからの頼み
        for e in self.view.ivars().queue.take() {
            self.push(e);
        }
        self.apply_a11y_actions();
        if let Some(focused) = self.delegate.ivars().focus.take() {
            log::event("window.focus", &format!("focused={focused}{}", self.tag()), &format!("app_active={app_active}"));
            if let Some(ev) = self.a11y.update_view_focus_state(focused) {
                ev.raise();
            }
        }
    }

    /// 読み上げからの頼み（押す・焦点・増減）を kargui の入力にする
    fn apply_a11y_actions(&mut self) {
        let requests = std::mem::take(&mut *self.a11y_actions.borrow_mut());
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

    /// フレームの後に、IME を向ける入力欄と、候補の窓を出す場所を KarguiView に写す
    fn sync_ime(&mut self) {
        let target = self.rt.cx.ime_target();
        if target != self.ime_target {
            self.view.discard_marked();
            log::event("ime.focus", &format!("to={}", target.map_or("none".to_string(), |t| format!("{:x}", t.0))), "ok");
            self.ime_target = target;
        }
        // 入力の受け手が今使っている入力ソース（日本語入力・英字など）。変わったら記録する
        if let Some(source) = self.view.inputContext().and_then(|ctx| ctx.selectedKeyboardInputSource()).map(|s| s.to_string())
            && self.view.note_source(&source)
        {
            for e in self.view.ivars().queue.take() {
                self.push(e);
            }
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

/// 窓の Runtime を作る（主の窓と、Ui::window で頼まれた窓）
fn new_runtime(title: &str, dark: bool, font: &crate::Font, fonts: &[std::borrow::Cow<'static, [u8]>]) -> Result<Runtime, crate::Error> {
    let text = Text::new()?;
    let mut rt = Runtime::new(text, dark, Box::new(MacClipboard), title.to_string());
    rt.cx.waker = crate::wake::Waker::new(post_wake);
    rt.cx.text.load_fonts(fonts);
    rt.cx.font = rt.cx.text.fonts.id(font);
    crate::locale::apply_to(&mut rt.cx, title);
    // OS のハイコントラスト（アプリの Options::high_contrast が先。kargui の配色を使う）
    rt.cx.set_system_high_contrast(system_high_contrast(), None);
    Ok(rt)
}

/// NSWindow・委任先・中身の NSView・読み上げの橋渡しを作り、Window にする（まだ出さない）
fn new_window(mtm: MainThreadMarker, key: Option<String>, title: &str, size: (f32, f32), min_size: Option<(f32, f32)>, rt: Runtime, follow_dark: bool) -> Window {
    let (lw, lh) = (size.0 as f64, size.1 as f64);
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable | NSWindowStyleMask::Resizable;
    // SAFETY: 窓を作る AppKit の関数。閉じた時に放さない（下の setReleasedWhenClosed）ので、Retained の持ち方と合う
    let ns = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(lw, lh)), style, NSBackingStoreType::Buffered, false) };
    // SAFETY: Retained で持つので、閉じた時に AppKit に放させない
    unsafe { ns.setReleasedWhenClosed(false) };
    ns.setTitle(&NSString::from_str(title));
    if let Some((mw, mh)) = min_size {
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
    // 読み上げの橋渡し。窓を出す前に付ける（accesskit_macos の決まり）
    let sink = MacA11ySink::default();
    let a11y_actions = sink.actions.clone();
    // SAFETY: view は生きている NSView（Window が持ち続ける）。窓を出す前で、まだ誰も KVO で見ていない
    let a11y = unsafe { accesskit_macos::SubclassingAdapter::new(Retained::as_ptr(&view) as *mut std::ffi::c_void, sink.clone(), sink) };
    let mut w = Window {
        key,
        ns,
        view,
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
        pending_dialog: None,
        a11y,
        a11y_actions,
        follow_dark,
        files_registered: false,
    };
    w.sync_size();
    w.sync_title_appearance();
    log::event(
        "window.create",
        &format!("title={title:?} size={lw:.0}x{lh:.0}{}", w.tag()),
        &format!("physical={}x{} scale={} first_responder={first_responder} a11y=SubclassingAdapter", w.size.0, w.size.1, w.scale),
    );
    w
}

/// 主の窓のビューが頼んだ窓（Ui::window）を開き、頼まれなくなった窓を閉じる（X11 の版の sync_windows と同じ）
fn sync_windows(s: &mut State, mtm: MainThreadMarker) {
    let requested = s.windows[0].rt.cx.windows.clone();
    let new: Vec<_> = requested.iter().filter(|r| !s.windows.iter().any(|w| w.key.as_deref() == Some(r.key.as_str()))).cloned().collect();
    for r in new {
        let main = &s.windows[0];
        let (dark, font, follow) = (main.rt.cx.dark, main.rt.cx.text.fonts.get(main.rt.cx.font).clone(), main.follow_dark);
        let main_frame = main.ns.frame();
        let rt = match new_runtime(&r.title, dark, &font, &s.fonts) {
            Ok(rt) => rt,
            Err(e) => {
                log::event("window.open_other", &format!("key={}", r.key), &format!("error={e}"));
                continue;
            }
        };
        let w = new_window(mtm, Some(r.key.clone()), &r.title, r.size, r.min_size, rt, follow);
        // 置く場所: 頼まれた場所（画面の左上から。macOS の画面の座標は左下が原点）か、主の窓から右下へずらす
        match (r.position, w.ns.screen()) {
            (Some((x, y)), Some(screen)) => {
                let sf = screen.frame();
                w.ns.setFrameTopLeftPoint(NSPoint::new(sf.origin.x + x as f64, sf.origin.y + sf.size.height - y as f64));
            }
            _ => {
                let n = s.windows.len() as f64;
                w.ns.setFrameTopLeftPoint(NSPoint::new(main_frame.origin.x + 30.0 * n, main_frame.origin.y + main_frame.size.height - 30.0 * n));
            }
        }
        if r.activate {
            w.ns.makeKeyAndOrderFront(None);
        } else {
            w.ns.orderFront(None);
        }
        log::event("window.open_other", &format!("key={} size={:.0}x{:.0} position={:?}", r.key, r.size.0, r.size.1, r.position), &format!("activate={}", r.activate));
        s.windows.push(w);
    }
    let mut i = 1;
    while i < s.windows.len() {
        let gone = s.windows[i].key.as_ref().is_some_and(|k| !requested.iter().any(|r| &r.key == k));
        if gone {
            let w = s.windows.remove(i);
            w.ns.orderOut(None);
            log::event("window.close_other", &format!("key={}", w.key.as_deref().unwrap_or("")), "ok");
        } else {
            i += 1;
        }
    }
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

    let rt = new_runtime(&options.title, dark, &options.font, &options.fonts)?;
    log::event("theme.contrast", &format!("high={}", rt.cx.os_high_contrast), &format!("system forced={:?}", crate::theme::option()));
    let japanese = matches!(rt.cx.locale, crate::Locale::Japanese);
    let w = new_window(mtm, None, &options.title, options.size, options.min_size, rt, options.dark.is_none());
    match options.position {
        // 画面の座標は左下が原点。kargui の位置は左上なので、ここでは真ん中に置く（位置の対応は後で）
        Some(_) | None => w.ns.center(),
    }
    // OS の明暗とハイコントラストの変化を、主の窓の委任先で聞く
    // SAFETY: 委任先は主の窓（Window）が輪の終わりまで持つ。知らせの名前は Apple の物
    unsafe {
        let observer: &AnyObject = w.delegate.as_ref();
        objc2_foundation::NSDistributedNotificationCenter::defaultCenter().addObserver_selector_name_object(
            observer,
            objc2::sel!(systemSettingsChanged:),
            Some(&NSString::from_str("AppleInterfaceThemeChangedNotification")),
            None,
        );
        objc2_app_kit::NSWorkspace::sharedWorkspace().notificationCenter().addObserver_selector_name_object(
            observer,
            objc2::sel!(systemSettingsChanged:),
            Some(objc2_app_kit::NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
            None,
        );
    }
    install_menu(mtm, &ns_app, &w.ns, japanese);
    w.ns.makeKeyAndOrderFront(None);
    if options.activate {
        // 端末などから起動したアプリは、前面のアプリが譲らないと activate（macOS 14 からの穏やかな頼み方）では前面に
        // ならない（CI の Mac では Finder が前面のまま、窓は灰色の題の帯だった）。winit と同じく、ほかのアプリより先に
        // 前面にする古い頼み方を使う
        #[allow(deprecated)]
        ns_app.activateIgnoringOtherApps(true);
        log::event("app.activate", "ignoring_other_apps=true", &format!("active_now={}", ns_app.isActive()));
    }
    // アプリの束の名前（cargo が作った裸の実行ファイルでは無い）
    let bundle = objc2_foundation::NSBundle::mainBundle();
    log::event("app.bundle", &format!("path={:?}", bundle.bundlePath().to_string()), &format!("identifier={:?}", bundle.bundleIdentifier().map(|s| s.to_string())));

    let state = Rc::new(RefCell::new(State { windows: vec![w], app, started, quit: false, fonts: options.fonts.clone() }));

    let shot = std::env::var("KARGUI_SHOT").ok().filter(|s| !s.is_empty());
    let shot_at = started + Duration::from_millis(std::env::var("KARGUI_SHOT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1500));
    let mut last_press: Option<(Instant, f32, f32, u8)> = None;
    loop {
        // 新しく開いた窓の委任先にも、窓と App の弱い参照を渡す（引っ張って大きさを変える間の呼び返しが使う）
        {
            let s = state.borrow();
            for w in &s.windows {
                if w.delegate.ivars().state.borrow().upgrade().is_none() {
                    w.delegate.ivars().state.replace(Rc::downgrade(&state));
                }
            }
        }
        // 頼まれたファイルのダイアログを、State を借りずに出す。答え（取り消しは空）は Ui::chosen_file が読む
        let dialog = state.borrow_mut().windows.iter_mut().enumerate().find_map(|(i, w)| w.pending_dialog.take().map(|d| (i, d)));
        if let Some((i, (key, d))) = dialog {
            let files = show_file_dialog(mtm, &d);
            log::event("file_dialog.answer", &format!("key={key} save={}", d.save), &format!("files={}", files.len()));
            let mut guard = state.borrow_mut();
            if let Some(w) = guard.windows.get_mut(i) {
                w.rt.cx.file_results.insert(key, files);
                w.need_frame = true;
                w.why = "file_dialog";
            }
        }
        let date = {
            let mut guard = state.borrow_mut();
            let s = &mut *guard;
            // OS の明暗・ハイコントラストが変わった（主の窓の委任先が聞く）。全部の窓に入れる
            if s.windows[0].delegate.ivars().system_changed.replace(false) {
                let (dark, hc) = (system_dark_setting(), system_high_contrast());
                log::event("theme.system", &format!("dark={dark}"), if s.windows[0].follow_dark { "changed" } else { "ignored (the app chose)" });
                for w in &mut s.windows {
                    if w.follow_dark {
                        w.rt.cx.set_dark(dark);
                    }
                    if w.rt.cx.set_system_high_contrast(hc, None) && w.key.is_none() {
                        log::event("theme.contrast", &format!("high={hc}"), &format!("changed forced={:?}", crate::theme::option()));
                    }
                    w.need_frame = true;
                    w.why = "theme";
                }
            }
            let active = ns_app.isActive();
            for w in &mut s.windows {
                w.before_frame(active);
            }
            // 閉じるボタン: 主の窓はアプリに聞き、ほかの窓はアプリに閉じたことを知らせる（主の窓のビューが頼むのを
            // やめたら閉じる）
            let mut close_main = false;
            for i in 0..s.windows.len() {
                if !s.windows[i].delegate.ivars().close_asked.replace(false) {
                    continue;
                }
                match s.windows[i].key.clone() {
                    None => {
                        let ok = s.app.close_requested();
                        log::event("window.close_requested", "", &format!("allowed={ok}"));
                        close_main |= ok;
                        s.windows[0].need_frame = true;
                        s.windows[0].why = "close_asked";
                    }
                    Some(key) => {
                        log::event("window.close_requested", &format!("key={key}"), "closed by the user");
                        s.windows[0].rt.cx.closed_windows.insert(key);
                        s.windows[0].need_frame = true;
                        s.windows[0].why = "input";
                    }
                }
            }
            if close_main {
                break;
            }
            // 主の窓を先に回す（ほかの窓を開くか閉じるかは、主の窓のビューが決める）
            for i in 0..s.windows.len() {
                let State { windows, app, started, quit, .. } = &mut *s;
                if windows[i].need_frame && !frame(&mut windows[i], &mut **app, *started) {
                    *quit = true;
                }
            }
            if s.quit {
                break;
            }
            sync_windows(s, mtm);
            if let Some(path) = shot.as_ref().filter(|_| Instant::now() >= shot_at) {
                save_shot(&s.windows[0], path);
                break;
            }
            // 次に起きる時刻まで知らせを待つ（描く物が残っていれば待たない）
            let now = Instant::now();
            let mut until: Option<Instant> = shot.as_ref().map(|_| shot_at);
            for w in &s.windows {
                let t = if w.need_frame || w.pending_dialog.is_some() { Some(now) } else { w.wake_at };
                until = match (until, t) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
            }
            match until {
                Some(t) => NSDate::dateWithTimeIntervalSinceNow(t.saturating_duration_since(Instant::now()).as_secs_f64()),
                None => NSDate::distantFuture(),
            }
        };
        // SAFETY: NSDefaultRunLoopMode は Foundation の定数
        let ev = ns_app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&date), unsafe { NSDefaultRunLoopMode }, true);
        let Some(ev) = ev else { continue };
        // 知らせを窓に振り分ける（起こす知らせなど、窓の無い知らせは輪を起こすだけ）
        let target = ev.window(mtm).and_then(|win| state.borrow().windows.iter().position(|w| std::ptr::eq(&*w.ns, &*win)));
        let pass = match target {
            Some(i) => handle(&mut state.borrow_mut().windows[i], &ev, &mut last_press),
            None => !(ev.r#type() == NSEventType::ApplicationDefined && ev.subtype().0 == WAKE_SUBTYPE),
        };
        if pass {
            // 借りずに渡す（AppKit が中で自分の輪を回し、委任先が State を借りることがある）
            ns_app.sendEvent(&ev);
            // IME に通したキーの結果
            if let Some(i) = target {
                let mut guard = state.borrow_mut();
                if let Some(w) = guard.windows.get_mut(i) {
                    for e in w.view.finish_key() {
                        w.push(e);
                    }
                }
            }
        }
    }
    log::event("window.close", "", "ok");
    for w in &state.borrow().windows {
        w.ns.orderOut(None);
    }
    Ok(())
}

/// 1 フレームを回して出す。アプリが終わりを頼んだら false
fn frame(w: &mut Window, app: &mut dyn App, started: Instant) -> bool {
    let why = w.why;
    let events = std::mem::take(&mut w.events);
    // 主の窓は App::view、ほかの窓は App::window で描く
    let out = match w.key.clone() {
        None => w.rt.frame(app, events),
        Some(k) => w.rt.frame_with(&mut |ui: &mut crate::ui::Ui| app.window(&k, ui), events),
    };
    let quit = std::mem::take(&mut w.rt.cx.quit);
    w.wake_at = out.wake_after.map(|d| Instant::now() + d);
    if out.want_frame {
        w.wake_at = Some(w.wake_at.map_or(Instant::now() + ANIMATION_STEP, |t| t.min(Instant::now() + ANIMATION_STEP)));
    }
    w.need_frame = false;
    // ファイルのダイアログは、輪の頭で State を借りずに出す（runModal の間は AppKit が自分の輪を回す）
    if let Some((key, d)) = w.rt.cx.file_dialog_request.take() {
        log::event("file_dialog.request", &format!("key={key} save={} multiple={}", d.save, d.multiple), "show at the top of the loop");
        w.pending_dialog = Some((key, d));
    }
    // まだ作っていない仕組みの頼みは、記録して捨てる
    for (key, _) in std::mem::take(&mut w.rt.cx.notify_requests) {
        log::event("notify.request", &format!("key={key}"), "not on macOS yet (M4)");
    }
    w.draw_and_present(why);
    w.sync_title_appearance();
    w.sync_ime();
    // ファイルを受け取る部品が初めて現れたら、OS にファイルを落とせる窓だと届け出る（1 回だけ。Windows の版と同じ）
    if w.rt.cx.wants_files && !w.files_registered {
        w.files_registered = true;
        // SAFETY: NSPasteboardTypeFileURL は AppKit の定数
        w.view.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        log::event("drag.register", &format!("file URLs{}", w.tag()), "ok");
    }
    // 読み上げの木（読み上げが使われ始めてから。初めの木は request_initial_tree の後のこのフレームで渡す）
    let rt = &mut w.rt;
    if let Some(ev) = w.a11y.update_if_active(|| rt.a11y().0) {
        ev.raise();
    }
    let first = !w.shown;
    w.shown = true;
    if first {
        log::event("window.shown", &format!("physical={}x{} scale={}{}", w.size.0, w.size.1, w.scale, w.tag()), &format!("ms={:.1}", w.created.elapsed().as_secs_f64() * 1000.0));
    }
    if w.key.is_none() {
        let number = if first { 0 } else { w.rt.frames.saturating_sub(1) };
        app.presented(&Frame { number, since_start: started.elapsed() });
    }
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
            // Command の近道は、先にアプリのメニュー（隠す ⌘H・終了 ⌘Q）に聞く。メニューに無ければ Ctrl としてアプリへ
            if ev.modifierFlags().contains(NSEventModifierFlags::Command)
                && NSApplication::sharedApplication(MainThreadMarker::from(&*w.ns)).mainMenu().is_some_and(|m| m.performKeyEquivalent(ev))
            {
                log::event("menu.key", &format!("code={code}"), "handled by the menu");
                return false;
            }
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

/// システムのファイルのダイアログ（NSOpenPanel・NSSavePanel）を出して、選んだファイルを返す（取り消しは空）。
/// 選べる種類は、全部の種類の拡張子をまとめて渡す（setAllowedFileTypes。新しい allowedContentTypes は UTType の依存が
/// 増えるので使わない）。保存は、名前に拡張子が無ければ最初の拡張子を足し、同じ名前があれば上書きしてよいか聞く
fn show_file_dialog(mtm: MainThreadMarker, d: &crate::file_dialog::FileDialog) -> Vec<std::path::PathBuf> {
    let exts: Vec<Retained<NSString>> = d.filters.iter().flat_map(|(_, e)| e.iter()).map(|e| NSString::from_str(e)).collect();
    let types = (!exts.is_empty()).then(|| NSArray::from_retained_slice(&exts));
    let common = |p: &NSSavePanel| {
        if let Some(t) = &d.title {
            p.setTitle(Some(&NSString::from_str(t)));
            p.setMessage(Some(&NSString::from_str(t)));
        }
        if let Some(f) = &d.folder {
            p.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&f.to_string_lossy()))));
        }
        #[allow(deprecated)]
        p.setAllowedFileTypes(types.as_deref());
    };
    let path_of = |u: &NSURL| u.path().map(|s| std::path::PathBuf::from(s.to_string()));
    if d.save {
        let p = NSSavePanel::savePanel(mtm);
        common(&p);
        if let Some(n) = &d.file_name {
            p.setNameFieldStringValue(&NSString::from_str(n));
        }
        if p.runModal() != NSModalResponseOK {
            return Vec::new();
        }
        p.URL().and_then(|u| path_of(&u)).into_iter().collect()
    } else {
        let p = NSOpenPanel::openPanel(mtm);
        common(&p);
        p.setCanChooseFiles(true);
        p.setCanChooseDirectories(false);
        p.setAllowsMultipleSelection(d.multiple);
        if p.runModal() != NSModalResponseOK {
            return Vec::new();
        }
        p.URLs().iter().filter_map(|u| path_of(&u)).collect()
    }
}

/// 窓に出した絵を PNG で保存する（KARGUI_SHOT）
fn save_shot(w: &Window, path: &str) {
    let (pw, ph) = w.size;
    let img = crate::test::Image { width: pw as u32, height: ph as u32, pixels: w.pixels.clone() };
    let result = img.save_png(path);
    log::event("shot", &format!("path={path:?} size={pw}x{ph}"), &format!("{result:?}"));
}
