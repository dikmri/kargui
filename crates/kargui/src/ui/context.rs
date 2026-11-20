//! フレームをまたいで持つ状態と、入力の振り分け。
//!
//! クリックがどの部品に当たったかは、前のフレームの配置（ユーザーが見ていた画面）で決める。
//! 入力はビューの 1 回目（入力を渡す回）で部品に届き、2 回目（描く回）は入力なしで描く。

use std::collections::HashMap;

use crate::geom::Rect;
use crate::input::{A11yAction, Event, Key, Mods};
use crate::text::Text;
use crate::theme::Theme;
use crate::ui::Id;
use crate::ui::edit::EditState;

/// クリップボードの読み書き（窓では OS のもの、試験台では手元の文字列）
pub(crate) trait Clipboard {
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: &str);
}

/// 試験台で使う、手元の文字列だけのクリップボード
#[derive(Default)]
pub(crate) struct MemoryClipboard(pub Option<String>);

impl Clipboard for MemoryClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }
    fn set(&mut self, text: &str) {
        self.0 = Some(text.to_string());
    }
}

/// 押したり焦点を受けたりできる部品の、前のフレームでの場所
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub id: Id,
    pub rect: Rect,
    pub focusable: bool,
    pub text_input: bool,
}

/// 一覧の、前のフレームでの場所と中身の高さ（物理ピクセル）
#[derive(Clone, Debug)]
pub(crate) struct ListInfo {
    pub id: Id,
    pub rect: Rect,
    pub content_h: f32,
}

/// 前のフレームの配置
#[derive(Default)]
pub(crate) struct Prev {
    /// 描いた順。後ろほど上に重なる
    pub hits: Vec<Hit>,
    /// Tab で移る順
    pub focus_order: Vec<Id>,
    pub lists: Vec<ListInfo>,
    /// 入力欄の場所と、文字の左端の x（物理ピクセル）
    pub inputs: HashMap<Id, (Rect, f32)>,
}

pub(crate) struct Context {
    pub scale: f32,
    /// 窓の中の大きさ（物理ピクセル）
    pub width: f32,
    pub height: f32,
    pub dark: bool,
    pub theme: Theme,
    pub text: Text,
    pub prev: Prev,
    pub edits: HashMap<Id, EditState>,
    /// 入力欄の横のスクロール（物理ピクセル）
    pub edit_scroll: HashMap<Id, f32>,
    /// 一覧のスクロールの位置（論理ピクセル）
    pub scrolls: HashMap<Id, f32>,
    pub focus: Option<Id>,
    pub hover: Option<Id>,
    pub pressed: Option<Id>,
    /// キーボードで焦点を動かしたあとだけ、焦点の輪を描く
    pub focus_visible: bool,
    // ---- この回の入力 ----
    pub clicked: Option<Id>,
    /// 焦点のある入力欄へ届ける入力
    pub keys: Vec<Event>,
    /// 入力欄の中のクリック（場所と Shift）
    pub pointer_text: Option<(Id, f32, f32, bool)>,
    /// 変換中の文字（焦点のある入力欄の中に描く）
    pub preedit: String,
    pub want_frame: bool,
    pub clipboard: Box<dyn Clipboard>,
    // ---- 描いた結果 ----
    /// 文字を打つ位置と、変換中の文字の範囲（物理ピクセル）。IME に知らせる
    pub caret: Option<Rect>,
    pub preedit_rect: Option<Rect>,
    pub input_rect: Option<Rect>,
}

impl Context {
    pub fn new(text: Text, dark: bool, clipboard: Box<dyn Clipboard>) -> Self {
        Self {
            scale: 1.0,
            width: 0.0,
            height: 0.0,
            dark,
            theme: if dark { Theme::dark() } else { Theme::light() },
            text,
            prev: Prev::default(),
            edits: HashMap::new(),
            edit_scroll: HashMap::new(),
            scrolls: HashMap::new(),
            focus: None,
            hover: None,
            pressed: None,
            focus_visible: false,
            clicked: None,
            keys: Vec::new(),
            pointer_text: None,
            preedit: String::new(),
            want_frame: false,
            clipboard,
            caret: None,
            preedit_rect: None,
            input_rect: None,
        }
    }

    pub fn set_dark(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.theme = if dark { Theme::dark() } else { Theme::light() };
        }
    }

    /// 前のフレームで (x, y) にあった、押せる部品（上に重なっている物）
    pub fn hit(&self, x: f32, y: f32) -> Option<&Hit> {
        self.prev.hits.iter().rev().find(|h| h.rect.contains(x, y))
    }

    fn is_text_input(&self, id: Id) -> bool {
        self.prev.hits.iter().any(|h| h.id == id && h.text_input)
    }

    fn move_focus(&mut self, back: bool) {
        let order = &self.prev.focus_order;
        if order.is_empty() {
            return;
        }
        let at = self.focus.and_then(|f| order.iter().position(|&o| o == f));
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
        };
        self.focus = Some(order[next]);
    }

    /// 入力を部品へ振り分ける。ビューの 1 回目の前に呼ぶ
    pub fn handle(&mut self, events: Vec<Event>) {
        for e in events {
            match e {
                Event::PointerDown { x, y, shift } => {
                    let hit = self.hit(x, y).cloned();
                    self.pressed = hit.as_ref().map(|h| h.id);
                    self.focus_visible = false;
                    self.focus = hit.as_ref().filter(|h| h.focusable).map(|h| h.id);
                    if let Some(h) = hit.filter(|h| h.text_input) {
                        self.pointer_text = Some((h.id, x, y, shift));
                    }
                }
                Event::PointerUp { x, y } => {
                    let hit = self.hit(x, y).map(|h| h.id);
                    if hit.is_some() && hit == self.pressed {
                        self.clicked = hit;
                    }
                    self.pressed = None;
                }
                Event::Wheel { x, y, dy } => {
                    if let Some(l) = self.prev.lists.iter().rev().find(|l| l.rect.contains(x, y)) {
                        let max = ((l.content_h - l.rect.h) / self.scale).max(0.0);
                        let v = self.scrolls.entry(l.id).or_insert(0.0);
                        *v = (*v + dy).clamp(0.0, max);
                    }
                }
                Event::Key { key: Key::Tab, mods } => {
                    self.focus_visible = true;
                    self.move_focus(mods.shift);
                }
                Event::Key { key: Key::Enter | Key::Space, mods: Mods { ctrl: false, alt: false, .. } }
                    if self.focus.is_some_and(|f| !self.is_text_input(f)) =>
                {
                    self.clicked = self.focus;
                }
                Event::A11y { target, action: A11yAction::Click } => {
                    self.clicked = Some(target);
                }
                Event::A11y { target, action: A11yAction::Focus } => {
                    self.focus = Some(target);
                }
                Event::ImePreedit(s) => self.preedit = s,
                e @ (Event::Key { .. } | Event::Char(_) | Event::ImeCommit(_)) => {
                    if self.focus.is_some_and(|f| self.is_text_input(f)) {
                        if matches!(e, Event::ImeCommit(_)) {
                            self.preedit.clear();
                        }
                        self.keys.push(e);
                    }
                }
            }
        }
    }

    /// ビューの 1 回目が終わったら、使わなかった入力を捨てる
    pub fn end_input(&mut self) {
        self.clicked = None;
        self.keys.clear();
        self.pointer_text = None;
    }
}
