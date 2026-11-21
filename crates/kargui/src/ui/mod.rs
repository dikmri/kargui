//! Building screens.
//!
//! You describe the whole screen in [`App::view`](crate::App::view) every time something
//! changes. Keep your state in your own struct and pass it to widgets:
//!
//! ```
//! use kargui::{App, Ui};
//!
//! struct Counter {
//!     count: u32,
//! }
//!
//! impl App for Counter {
//!     fn view(&mut self, ui: &mut Ui) {
//!         ui.row(|ui| {
//!             ui.label(format!("Count: {}", self.count));
//!             if ui.button("Add").clicked() {
//!                 self.count += 1;
//!             }
//!         });
//!     }
//! }
//!
//! let mut h = kargui::test::Harness::new(Counter { count: 0 });
//! h.click("Add");
//! assert_eq!(h.app().count, 1);
//! ```
//!
//! kargui remembers each widget between frames (text cursor, scroll position, focus) by an
//! [`Id`] derived from where in your code the widget is created. Widgets created in a loop are
//! told apart by their order; use [`Ui::keyed`] when items can move.
//!
//! # Common mistakes
//!
//! These come from apps that AIs wrote with kargui in a measured test:
//!
//! - **Changing data inside a list row.** Rows after it would read the data with old indices.
//!   Remember what was clicked and change the data after the list (see [`Ui::list`]).
//! - **Keeping your own dark-mode flag.** It starts out of step with the operating system's
//!   setting. Use [`Ui::dark_mode_switch`], which shows and changes kargui's own setting.
//! - **Wrapping a list row in another row.** A list row is already a row: put the widgets straight
//!   in and use [`Ui::fill`] to push a button to the right end.
//! - **Handling Enter or input-method events yourself.** [`TextInput::submitted`] already ignores
//!   the Enter that commits a composition, and [`TextInput::changed`] only reports committed text.
//! - **Widgets without a name.** Sliders and symbol-only buttons need `.label("..")` so screen
//!   readers (and tests) can find them.

pub(crate) mod a11y;
pub(crate) mod context;
pub(crate) mod draw;
pub(crate) mod edit;
pub(crate) mod layout;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::RangeInclusive;

use context::Context;
pub use layout::{Align, Size};
use layout::{Dir, LayoutNode};

use crate::input::{Event, Key};
use crate::theme::Theme;

/// Identifies a widget across frames.
///
/// ```
/// # kargui::test::example(|ui| {
/// let ok = ui.button("OK").id();
/// let cancel = ui.button("Cancel").id();
/// assert_ne!(ok, cancel);
/// # });
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Id(pub(crate) u64);

/// 64 ビットの FNV-1a
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

pub(crate) fn hash_of(v: impl Hash) -> u64 {
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    v.hash(&mut h);
    h.finish()
}

/// 文字の大きさの種類
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextKind {
    Body,
    Small,
    Heading,
    /// 大きさを決めた文字（論理ピクセルの 10 倍。Eq のため整数で持つ）
    Custom(u32),
}

impl TextKind {
    /// 論理ピクセル
    pub fn size(self) -> f32 {
        match self {
            TextKind::Body => 14.0,
            TextKind::Small => 12.5,
            TextKind::Heading => 21.0,
            TextKind::Custom(tenths) => tenths as f32 / 10.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Root,
    Row,
    Column,
    Stack,
    Label { text: String, size: TextKind, bold: bool, weak: bool, strike: bool, wrap: bool, right: bool },
    Button { text: String, primary: bool, small: bool },
    Checkbox { checked: bool, text: String, strike: bool },
    Toggle { on: bool, text: String },
    Segment { text: String, selected: bool },
    TextInput { value: String, hint: String },
    Space,
    List { count: usize, separators: bool, view_h: f32 },
    Scroll,
    /// 選べる一覧の行（tab_stop: Tab で移る行。選んだ行、無ければ見えている最初の行）
    Item { selected: bool, tab_stop: bool },
    Slider { value: f64, min: f64, max: f64, step: f64 },
}

impl Kind {
    pub fn interactive(&self) -> bool {
        matches!(
            self,
            Kind::Button { .. }
                | Kind::Checkbox { .. }
                | Kind::Toggle { .. }
                | Kind::Segment { .. }
                | Kind::TextInput { .. }
                | Kind::Slider { .. }
                | Kind::Item { .. }
        )
    }
}

#[derive(PartialEq)]
pub(crate) struct NodeInfo {
    pub id: Id,
    pub kind: Kind,
    /// 読み上げの名前（無ければ部品の文字を使う）
    pub label: Option<String>,
}

/// ビュー 1 回分の画面の木。info と layout は同じ番号で並ぶ。親は子より前に入る
pub(crate) struct Tree {
    pub info: Vec<NodeInfo>,
    pub layout: Vec<LayoutNode>,
    occurrences: HashMap<(u64, u64, &'static str, u32, u32), u32>,
}

impl Tree {
    /// 部品と、その並べ方の決まり（配置の結果の前）が同じか。ビューを回し直して画面が落ち着いたかを見るのに使う
    pub fn same_as(&self, other: &Tree) -> bool {
        self.info == other.info && self.layout == other.layout
    }

    /// 根は縦に並べる。既定の余白は左右 16・上下 12、間は 10（論理ピクセル）
    pub fn new(root: Id, scale: f32) -> Self {
        let mut layout = LayoutNode::new(Dir::Column);
        layout.align = Align::Stretch;
        layout.pad = ((16.0 * scale).round(), (12.0 * scale).round());
        layout.gap = (10.0 * scale).round();
        Self { info: vec![NodeInfo { id: root, kind: Kind::Root, label: None }], layout: vec![layout], occurrences: HashMap::new() }
    }
}

/// The screen being described. See the [module documentation](self).
///
/// ```
/// use kargui::{App, Ui};
///
/// struct Hello;
///
/// impl App for Hello {
///     fn view(&mut self, ui: &mut Ui) {
///         ui.heading("Hello");
///         ui.label("Written every frame.");
///     }
/// }
///
/// let h = kargui::test::Harness::new(Hello);
/// assert!(h.find("Hello").is_some());
/// ```
pub struct Ui<'a> {
    pub(crate) cx: &'a mut Context,
    pub(crate) tree: &'a mut Tree,
    pub(crate) parent: usize,
    pub(crate) salt: u64,
}

impl<'a> Ui<'a> {
    pub(crate) fn new(cx: &'a mut Context, tree: &'a mut Tree) -> Self {
        Self { cx, tree, parent: 0, salt: 0 }
    }

    fn px(&self, logical: f32) -> f32 {
        (logical * self.cx.scale).round()
    }

    #[track_caller]
    fn add(&mut self, kind: Kind, dir: Dir) -> (usize, Id) {
        let loc = std::panic::Location::caller();
        let pid = self.tree.info[self.parent].id.0;
        let key = (pid, self.salt, loc.file(), loc.line(), loc.column());
        let n = self.tree.occurrences.entry(key).or_insert(0);
        let occurrence = *n;
        *n += 1;
        let id = Id(hash_of((key, occurrence)));
        self.push(id, kind, dir)
    }

    fn push(&mut self, id: Id, kind: Kind, dir: Dir) -> (usize, Id) {
        let idx = self.tree.info.len();
        self.tree.info.push(NodeInfo { id, kind, label: None });
        self.tree.layout.push(LayoutNode::new(dir));
        self.tree.layout[self.parent].children.push(idx);
        (idx, id)
    }

    fn child(&mut self, parent: usize) -> Ui<'_> {
        Ui { cx: &mut *self.cx, tree: &mut *self.tree, parent, salt: 0 }
    }

    fn parent_dir(&self) -> Dir {
        self.tree.layout[self.parent].dir
    }

    fn handle(&mut self, idx: usize, id: Id) -> Handle<'_> {
        let parent_dir = self.parent_dir();
        Handle { cx_scroll: &mut self.cx.scroll_into_view, tree: &mut *self.tree, idx, id, scale: self.cx.scale, parent_dir }
    }

    // ---- 入れ物 ----

    /// Places the widgets created in `f` side by side, centered vertically.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Name");
    ///     ui.button("OK");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn row(&mut self, f: impl FnOnce(&mut Ui)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Row, Dir::Row);
        self.tree.layout[idx].gap = self.px(8.0);
        f(&mut self.child(idx));
        self.grow_if_filled(idx);
        self.handle(idx, id)
    }

    /// Places the widgets created in `f` one below another, as wide as the column.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.column(|ui| {
    ///     ui.heading("Settings");
    ///     ui.label("Choose how the app looks.");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn column(&mut self, f: impl FnOnce(&mut Ui)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Column, Dir::Column);
        self.tree.layout[idx].gap = self.px(6.0);
        f(&mut self.child(idx));
        self.grow_if_filled(idx);
        self.handle(idx, id)
    }

    /// 行の中の行（列の中の列）が fill（残りを取る子）を持つなら、その行も親の残りを取る。
    /// 書く人は「fill で右端へ寄せる」つもりなので、入れ子の行が中身の幅で止まらないようにする（AI の試験で見つけた）
    fn grow_if_filled(&mut self, idx: usize) {
        let dir = self.tree.layout[idx].dir;
        if self.parent_dir() != dir {
            return;
        }
        let main = |l: &LayoutNode| if dir == Dir::Row { l.width } else { l.height };
        let filled = self.tree.layout[idx].children.iter().any(|&c| matches!(main(&self.tree.layout[c]), Size::Fill(_)));
        let l = &mut self.tree.layout[idx];
        if filled && main(l) == Size::Auto {
            if dir == Dir::Row {
                l.width = Size::Fill(1.0);
            } else {
                l.height = Size::Fill(1.0);
            }
        }
    }

    /// Places the widgets created in `f` on top of each other.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.stack(|ui| {
    ///     ui.label("Under");
    ///     ui.label("Over");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn stack(&mut self, f: impl FnOnce(&mut Ui)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Stack, Dir::Stack);
        self.tree.layout[idx].align = Align::Center;
        f(&mut self.child(idx));
        self.handle(idx, id)
    }

    /// A container whose content scrolls up and down when it is taller than the space left.
    /// The widgets created in `f` are placed one below another, as in [`Ui::column`].
    /// It takes the rest of the height (change it with [`Handle::height`]). Users scroll it with
    /// the mouse wheel, by dragging or clicking the scroll bar, or by moving the keyboard focus
    /// (Tab) to a widget that is out of view.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.heading("Settings");
    /// ui.scroll(|ui| {
    ///     for i in 0..50 {
    ///         ui.label(format!("Line {i}"));
    ///     }
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn scroll(&mut self, f: impl FnOnce(&mut Ui)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Scroll, Dir::Scroll);
        let gap = self.px(6.0);
        let gutter = self.px(draw::SCROLL_GUTTER);
        let scale = self.cx.scale;
        // 前のフレームの中身の高さで端を決める（中身が変わった時は、配置の後でもう一度そろえる）
        let max = self.cx.prev.lists.iter().find(|l| l.id == id).map(|l| ((l.content_h - l.rect.h) / scale).max(0.0));
        let now = self.cx.scrolls.get(&id).copied().unwrap_or(0.0);
        let offset = max.map_or(now, |m| now.min(m)).max(0.0);
        self.cx.scrolls.insert(id, offset);
        let l = &mut self.tree.layout[idx];
        l.gap = gap;
        l.gutter = gutter;
        l.height = Size::Fill(1.0);
        l.offset = (offset * scale).round();
        f(&mut self.child(idx));
        self.handle(idx, id)
    }

    /// Gives the widgets created in `f` identities based on `key`, so that their state
    /// (text cursor, focus) follows the item when items are added, removed or reordered.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// for (id, name) in [(7, "Milk"), (3, "Bread")] {
    ///     ui.keyed(id, |ui| {
    ///         ui.label(name);
    ///     });
    /// }
    /// # });
    /// ```
    pub fn keyed<K: Hash>(&mut self, key: K, f: impl FnOnce(&mut Ui)) {
        let salt = hash_of((self.salt, key));
        let parent = self.parent;
        f(&mut Ui { cx: &mut *self.cx, tree: &mut *self.tree, parent, salt });
    }

    /// Empty space that takes the rest of the row (or column), pushing the following widgets to the end.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.heading("Tasks");
    ///     ui.fill();
    ///     ui.button("Settings");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn fill(&mut self) {
        let (idx, _) = self.add(Kind::Space, Dir::Leaf);
        let dir = self.parent_dir();
        let l = &mut self.tree.layout[idx];
        if dir == Dir::Column {
            l.height = Size::Fill(1.0);
        } else {
            l.width = Size::Fill(1.0);
        }
    }

    /// Empty space of a fixed length (logical pixels).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.label("Above");
    /// ui.space(24.0);
    /// ui.label("Below");
    /// # });
    /// ```
    #[track_caller]
    pub fn space(&mut self, length: f32) {
        let (idx, _) = self.add(Kind::Space, Dir::Leaf);
        let px = self.px(length);
        self.tree.layout[idx].intrinsic = (px, px);
    }

    // ---- 文字 ----

    /// Text.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.label("10 tasks").weak();
    /// # });
    /// ```
    #[track_caller]
    pub fn label(&mut self, text: impl Into<String>) -> Label<'_> {
        let kind = Kind::Label { text: text.into(), size: TextKind::Body, bold: false, weak: false, strike: false, wrap: false, right: false };
        let (idx, id) = self.add(kind, Dir::Leaf);
        Label(self.handle(idx, id))
    }

    /// A large, bold title. Screen readers announce it as a heading.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.heading("Tasks");
    /// # });
    /// ```
    #[track_caller]
    pub fn heading(&mut self, text: impl Into<String>) -> Label<'_> {
        let kind = Kind::Label { text: text.into(), size: TextKind::Heading, bold: true, weak: false, strike: false, wrap: false, right: false };
        let (idx, id) = self.add(kind, Dir::Leaf);
        Label(self.handle(idx, id))
    }

    // ---- 押す物 ----

    /// A button. Ask [`Button::clicked`] whether it was clicked (by pointer, keyboard or screen reader).
    ///
    /// ```
    /// # struct App { saved: bool }
    /// # let mut app = App { saved: false };
    /// # kargui::test::example(|ui| {
    /// if ui.button("Save").primary().clicked() {
    ///     app.saved = true;
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn button(&mut self, text: impl Into<String>) -> Button<'_> {
        let (idx, id) = self.add(Kind::Button { text: text.into(), primary: false, small: false }, Dir::Leaf);
        let clicked = self.cx.clicked == Some(id);
        Button { h: self.handle(idx, id), clicked }
    }

    /// A check box with a label. Clicking it flips `checked`.
    ///
    /// ```
    /// # let mut done = false;
    /// # kargui::test::example(|ui| {
    /// if ui.checkbox(&mut done, "Done").changed() {
    ///     // save
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn checkbox(&mut self, checked: &mut bool, text: impl Into<String>) -> Toggled<'_> {
        let (idx, id) = self.add(Kind::Checkbox { checked: *checked, text: text.into(), strike: false }, Dir::Leaf);
        let changed = self.cx.clicked == Some(id);
        if changed {
            *checked = !*checked;
            if let Kind::Checkbox { checked: c, .. } = &mut self.tree.info[idx].kind {
                *c = *checked;
            }
        }
        Toggled { h: self.handle(idx, id), changed }
    }

    /// An on/off switch with a label. Clicking it flips `on`.
    /// For a dark mode switch, use [`Ui::dark_mode_switch`] instead: it follows the operating
    /// system's setting from the start.
    ///
    /// ```
    /// # let mut wifi = true;
    /// # kargui::test::example(|ui| {
    /// ui.toggle(&mut wifi, "Wi-Fi");
    /// # });
    /// ```
    #[track_caller]
    pub fn toggle(&mut self, on: &mut bool, text: impl Into<String>) -> Toggled<'_> {
        let (idx, id) = self.add(Kind::Toggle { on: *on, text: text.into() }, Dir::Leaf);
        let changed = self.cx.clicked == Some(id);
        if changed {
            *on = !*on;
            if let Kind::Toggle { on: o, .. } = &mut self.tree.info[idx].kind {
                *o = *on;
            }
        }
        Toggled { h: self.handle(idx, id), changed }
    }

    /// A switch that turns dark mode on and off. It shows and changes kargui's own setting, which
    /// starts as the operating system's, so the app does not need to keep a value for it (and the
    /// switch never disagrees with the look). Read the current state with [`Ui::dark`].
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.heading("Tasks");
    ///     ui.fill();
    ///     ui.dark_mode_switch("Dark");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn dark_mode_switch(&mut self, text: impl Into<String>) -> Toggled<'_> {
        let dark = self.cx.dark;
        let (idx, id) = self.add(Kind::Toggle { on: dark, text: text.into() }, Dir::Leaf);
        let changed = self.cx.clicked == Some(id);
        if changed {
            self.cx.set_dark(!dark);
            if let Kind::Toggle { on, .. } = &mut self.tree.info[idx].kind {
                *on = !dark;
            }
        }
        Toggled { h: self.handle(idx, id), changed }
    }

    /// A row of buttons of which exactly one is selected. Returns whether the selection changed.
    ///
    /// ```
    /// #[derive(Clone, Copy, PartialEq)]
    /// enum Show { All, Open, Done }
    /// # let mut show = Show::All;
    /// # kargui::test::example(|ui| {
    /// ui.segmented(&mut show, &[(Show::All, "All"), (Show::Open, "Open"), (Show::Done, "Done")]);
    /// # });
    /// ```
    #[track_caller]
    pub fn segmented<T: Copy + PartialEq>(&mut self, value: &mut T, options: &[(T, &str)]) -> bool {
        let (row, _) = self.add(Kind::Row, Dir::Row);
        self.tree.layout[row].gap = self.px(4.0);
        let mut changed = false;
        {
            let mut ui = self.child(row);
            for (k, (v, text)) in options.iter().enumerate() {
                ui.salt = hash_of(k);
                let kind = Kind::Segment { text: text.to_string(), selected: *v == *value };
                let (_, id) = ui.add(kind, Dir::Leaf);
                if ui.cx.clicked == Some(id) && *v != *value {
                    *value = *v;
                    changed = true;
                }
            }
        }
        // 押されて選び直したときは、この回の木の見た目も直す
        if changed {
            let children = self.tree.layout[row].children.clone();
            for (k, c) in children.into_iter().enumerate() {
                if let Kind::Segment { selected, .. } = &mut self.tree.info[c].kind {
                    *selected = options[k].0 == *value;
                }
            }
        }
        changed
    }

    // ---- 入力欄 ----

    /// A one-line text field. Typing, the input method (IME), selection, copy and paste, undo and
    /// redo edit `text` directly.
    ///
    /// While the input method is composing (for example kana before conversion), `text` does not
    /// change; it changes when the conversion is committed. The Enter key that commits a conversion
    /// does not count as [`TextInput::submitted`].
    ///
    /// ```
    /// # let mut name = String::new();
    /// # kargui::test::example(|ui| {
    /// let field = ui.text_input(&mut name).hint("Your name").grow();
    /// if field.submitted() {
    ///     // use the name
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn text_input(&mut self, text: &mut String) -> TextInput<'_> {
        let (idx, id) = self.add(Kind::TextInput { value: String::new(), hint: String::new() }, Dir::Leaf);
        let (changed, submitted) = self.edit(id, text);
        if let Kind::TextInput { value, .. } = &mut self.tree.info[idx].kind {
            *value = text.clone();
        }
        let composing = (self.cx.focus == Some(id) && !self.cx.preedit.is_empty()).then(|| self.cx.preedit.clone());
        TextInput { h: self.handle(idx, id), changed, submitted, composing }
    }

    /// 焦点のある入力欄に、この回の入力を当てる。(変わったか, 送信したか)
    fn edit(&mut self, id: Id, s: &mut String) -> (bool, bool) {
        let before = s.clone();
        let mut submitted = false;
        let style = crate::text::TextStyle { size: self.px(TextKind::Body.size()), bold: false };
        self.cx.edits.entry(id).or_default().clamp(s);
        // 焦点が移る前に確定した変換中の文字（押した位置へキャレットを動かすより先に入れる）
        if self.cx.pending_commit.as_ref().is_some_and(|(pid, _)| *pid == id) {
            let (_, t) = self.cx.pending_commit.take().unwrap();
            self.cx.edits.get_mut(&id).unwrap().insert(s, &t, false);
        }
        if let Some((pid, x, _y, shift)) = self.cx.pointer_text {
            if pid == id {
                self.cx.pointer_text = None;
                if let Some(&(_, text_x)) = self.cx.prev.inputs.get(&id) {
                    let scroll = self.cx.edit_scroll.get(&id).copied().unwrap_or(0.0);
                    let laid = self.cx.text.layout(s, style, None);
                    let at = edit::byte_of(s, laid.index_at(x - text_x + scroll));
                    self.cx.edits.get_mut(&id).unwrap().move_to(at, shift);
                }
            }
        }
        if self.cx.focus != Some(id) {
            return (false, false);
        }
        let keys = std::mem::take(&mut self.cx.keys);
        let cx = &mut *self.cx;
        let st = cx.edits.get_mut(&id).unwrap();
        for e in keys {
            match e {
                Event::Char(c) if !c.is_control() => {
                    st.insert(s, c.encode_utf8(&mut [0; 4]), true);
                }
                Event::ImeCommit(t) => {
                    st.insert(s, &t, false);
                }
                // 変換が始まった: 選んでいる文字は変換中の文字に置き換わる（再変換もこの形）
                Event::ImePreedit(_) if st.has_selection() => {
                    st.insert(s, "", false);
                }
                Event::ImeSelect { start, end } => {
                    st.move_to(start, false);
                    st.move_to(end, true);
                    st.clamp(s);
                }
                Event::Key { key, mods } => match key {
                    Key::Enter => submitted = true,
                    Key::Backspace => {
                        st.backspace(s, mods.ctrl);
                    }
                    Key::Delete => {
                        st.delete(s, mods.ctrl);
                    }
                    Key::Left => st.move_left(s, mods.ctrl, mods.shift),
                    Key::Right => st.move_right(s, mods.ctrl, mods.shift),
                    Key::Home => st.move_to(0, mods.shift),
                    Key::End => st.move_to(s.len(), mods.shift),
                    Key::A if mods.ctrl => st.select_all(s),
                    Key::C if mods.ctrl && st.has_selection() => {
                        let t = st.selected_text(s).to_string();
                        cx.clipboard.set(&t);
                    }
                    Key::X if mods.ctrl && st.has_selection() => {
                        let t = st.selected_text(s).to_string();
                        cx.clipboard.set(&t);
                        st.backspace(s, false);
                    }
                    Key::V if mods.ctrl => {
                        if let Some(t) = cx.clipboard.get() {
                            st.insert(s, &t, false);
                        }
                    }
                    Key::Z if mods.ctrl => {
                        st.undo(s);
                    }
                    Key::Y if mods.ctrl => {
                        st.redo(s);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        (*s != before, submitted)
    }

    /// A slider for picking a number in `range` by dragging, clicking, or with the arrow keys
    /// (Home and End jump to the ends). Integer values move in steps of 1; `f32` and `f64` move
    /// freely with the pointer and in hundredths of the range with the keyboard. Give it a name for
    /// screen readers with `.label(..)`.
    ///
    /// ```
    /// # let (mut font_size, mut volume) = (14, 0.8_f32);
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Font size");
    ///     ui.slider(&mut font_size, 10..=24).label("Font size");
    ///     ui.label(font_size.to_string());
    /// });
    /// if ui.slider(&mut volume, 0.0..=1.0).label("Volume").changed() {
    ///     // apply the new volume
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn slider<T: SliderValue>(&mut self, value: &mut T, range: RangeInclusive<T>) -> Slider<'_> {
        let (min, max) = (range.start().to_f64(), range.end().to_f64());
        let step = if T::INTEGER { 1.0 } else { (max - min) / 100.0 };
        let old = value.to_f64();
        let mut v = old;
        let (idx, id) = self.add(Kind::Slider { value: v, min, max, step }, Dir::Leaf);
        // ポインター: 押している間と、押して離した時は、その位置の値にする（場所は前のフレームの配置）
        if self.cx.pressed == Some(id) || self.cx.released == Some(id) {
            let rect = self.cx.prev.hits.iter().find(|h| h.id == id).map(|h| h.rect);
            if let (Some((x, _)), Some(r)) = (self.cx.pointer, rect) {
                let (tx, tw) = draw::slider_track(r, self.cx.scale);
                let f = if tw > 0.0 { ((x - tx) / tw).clamp(0.0, 1.0) as f64 } else { 0.0 };
                v = min + f * (max - min);
            }
        }
        // キー: 焦点がある時だけ
        if self.cx.focus == Some(id) {
            for e in std::mem::take(&mut self.cx.keys) {
                if let Event::Key { key, .. } = e {
                    v = match key {
                        Key::Left | Key::Down => v - step,
                        Key::Right | Key::Up => v + step,
                        Key::Home => min,
                        Key::End => max,
                        _ => v,
                    };
                }
            }
        }
        let v = if T::INTEGER { v.round() } else { v }.clamp(min.min(max), max.max(min));
        let changed = v != old;
        if changed {
            *value = T::from_f64(v);
            if let Kind::Slider { value: k, .. } = &mut self.tree.info[idx].kind {
                *k = value.to_f64();
            }
        }
        Slider { h: self.handle(idx, id), changed }
    }

    // ---- 一覧 ----

    /// A list that only creates the rows that are visible, so it stays fast with any number of
    /// rows. All rows have the same height (logical pixels). Finish with [`List::show`].
    /// Each row is a [`Ui::row`] with 12 logical pixels of padding on the left and right, so a
    /// header row placed above the list lines up when it has the same padding:
    /// `ui.row(..).padding(12.0, 0.0)`.
    /// Users scroll it with the mouse wheel, by dragging the scroll bar, or by clicking the bar
    /// above or below the thumb to move one page.
    ///
    /// ```
    /// # let items: Vec<String> = (0..10_000).map(|i| format!("Item {i}")).collect();
    /// # kargui::test::example(|ui| {
    /// ui.list(items.len(), 32.0).grow().show(|ui, i| {
    ///     ui.label(&items[i]);
    /// });
    /// # });
    /// ```
    ///
    /// Each row is already a row: put the widgets straight in, and use [`Ui::fill`] to push a
    /// button to the right end. Do not change the data inside the row function (rows after it
    /// would read it with old indices). Remember what was clicked, and change the data after the
    /// list:
    ///
    /// ```
    /// # struct Task { id: u32, text: String, done: bool }
    /// # let mut tasks = vec![Task { id: 7, text: "Milk".into(), done: false }];
    /// # kargui::test::example(|ui| {
    /// let (mut toggled, mut removed) = (None, None);
    /// ui.list(tasks.len(), 32.0).grow().show_keyed(|i| tasks[i].id, |ui, i| {
    ///     let task = &tasks[i];
    ///     let mut done = task.done;
    ///     if ui.checkbox(&mut done, task.text.as_str()).strike(task.done).changed() {
    ///         toggled = Some(task.id);
    ///     }
    ///     ui.fill();
    ///     if ui.button("Delete").small().clicked() {
    ///         removed = Some(task.id);
    ///     }
    /// });
    /// if let Some(id) = toggled {
    ///     if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
    ///         t.done = !t.done;
    ///     }
    /// }
    /// if let Some(id) = removed {
    ///     tasks.retain(|t| t.id != id);
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn list(&mut self, count: usize, row_height: f32) -> List<'_, 'a> {
        let (idx, id) = self.add(Kind::List { count, separators: true, view_h: 0.0 }, Dir::List);
        let row_h = self.px(row_height).max(1.0);
        self.tree.layout[idx].row_h = row_h;
        List { ui: self, idx, id, count, row_h, scroll_to: None, selection: None }
    }

    // ---- そのほか ----

    /// Whether dark mode is on. It starts as the operating system's setting
    /// (or [`Options::dark`](crate::Options::dark)).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let label = if ui.dark() { "Light mode" } else { "Dark mode" };
    /// ui.button(label);
    /// # });
    /// ```
    pub fn dark(&self) -> bool {
        self.cx.dark
    }

    /// Switches between dark and light mode.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.set_dark(true);
    /// assert_eq!(ui.theme().background, kargui::Theme::dark().background);
    /// # });
    /// ```
    pub fn set_dark(&mut self, dark: bool) {
        self.cx.set_dark(dark);
    }

    /// The colors in use, for example to match your own drawing to the widgets.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let accent = ui.theme().accent;
    /// assert_eq!(accent[3], 1.0);
    /// # });
    /// ```
    pub fn theme(&self) -> &Theme {
        &self.cx.theme
    }

    /// Space inside the edges of the current container (logical pixels): `x` left and right,
    /// `y` top and bottom. At the top level this is the window's margin (16 and 12 by default).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.set_padding(24.0, 24.0);
    /// ui.heading("Roomy");
    /// # });
    /// ```
    pub fn set_padding(&mut self, x: f32, y: f32) {
        let p = (self.px(x), self.px(y));
        self.tree.layout[self.parent].pad = p;
    }

    /// Space between the children of the current container (logical pixels).
    /// At the top level the default is 10.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.set_gap(4.0);
    /// ui.label("Close");
    /// ui.label("together");
    /// # });
    /// ```
    pub fn set_gap(&mut self, length: f32) {
        let g = self.px(length);
        self.tree.layout[self.parent].gap = g;
    }

    /// Asks for another frame right after this one, for animations.
    /// kargui otherwise draws only when something changes.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.request_frame();
    /// # });
    /// ```
    pub fn request_frame(&mut self) {
        self.cx.want_frame = true;
    }
}

/// Settings shared by all widgets: size, spacing and accessibility name.
///
/// ```
/// # kargui::test::example(|ui| {
/// ui.row(|ui| {
///     ui.label("Left");
///     ui.fill();
///     ui.label("Right");
/// })
/// .padding(8.0, 4.0)
/// .gap(12.0);
/// # });
/// ```
pub struct Handle<'t> {
    cx_scroll: &'t mut Option<Id>,
    tree: &'t mut Tree,
    idx: usize,
    id: Id,
    scale: f32,
    parent_dir: Dir,
}

impl Handle<'_> {
    fn px(&self, s: Size) -> Size {
        match s {
            Size::Fixed(v) => Size::Fixed((v * self.scale).round()),
            s => s,
        }
    }

    /// Sets the width ([`Size::Fixed`] is in logical pixels).
    ///
    /// ```
    /// # use kargui::Size;
    /// # kargui::test::example(|ui| {
    /// ui.column(|ui| {
    ///     ui.label("Narrow");
    /// })
    /// .width(Size::Fixed(200.0));
    /// # });
    /// ```
    pub fn width(self, size: Size) -> Self {
        let s = self.px(size);
        self.tree.layout[self.idx].width = s;
        self
    }

    /// Sets the height ([`Size::Fixed`] is in logical pixels).
    ///
    /// ```
    /// # use kargui::Size;
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Tall");
    /// })
    /// .height(Size::Fixed(48.0));
    /// # });
    /// ```
    pub fn height(self, size: Size) -> Self {
        let s = self.px(size);
        self.tree.layout[self.idx].height = s;
        self
    }

    /// Takes the remaining space along the parent's direction (width in a row, height in a column).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.column(|ui| {
    ///     ui.label("Fills the height that is left.");
    /// })
    /// .grow();
    /// # });
    /// ```
    pub fn grow(self) -> Self {
        match self.parent_dir {
            Dir::Column => self.height(Size::Fill(1.0)),
            _ => self.width(Size::Fill(1.0)),
        }
    }

    /// Space between the children (logical pixels).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.column(|ui| {
    ///     ui.label("One");
    ///     ui.label("Two");
    /// })
    /// .gap(2.0);
    /// # });
    /// ```
    pub fn gap(self, length: f32) -> Self {
        self.tree.layout[self.idx].gap = (length * self.scale).round();
        self
    }

    /// Space inside the edges (logical pixels): `x` left and right, `y` top and bottom.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Indented");
    /// })
    /// .padding(12.0, 0.0);
    /// # });
    /// ```
    pub fn padding(self, x: f32, y: f32) -> Self {
        self.tree.layout[self.idx].pad = ((x * self.scale).round(), (y * self.scale).round());
        self
    }

    /// Where the children go across the main direction.
    ///
    /// ```
    /// # use kargui::Align;
    /// # kargui::test::example(|ui| {
    /// ui.column(|ui| {
    ///     ui.label("Centered");
    ///     ui.button("OK");
    /// })
    /// .align(Align::Center);
    /// # });
    /// ```
    pub fn align(self, align: Align) -> Self {
        self.tree.layout[self.idx].align = align;
        self
    }

    /// The name screen readers use for this widget, when its visible text is not enough
    /// (for example an icon-only button).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.button("+");
    ///     ui.button("-");
    /// })
    /// .label("Zoom");
    /// # });
    /// ```
    pub fn label(self, name: impl Into<String>) -> Self {
        self.tree.info[self.idx].label = Some(name.into());
        self
    }

    /// The widget's identity.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let toolbar = ui.row(|ui| {
    ///     ui.button("Save");
    /// });
    /// let id = toolbar.id();
    /// assert_eq!(id, id);
    /// # });
    /// ```
    pub fn id(&self) -> Id {
        self.id
    }

    /// Scrolls the nearest [`Ui::scroll`] container that holds this widget so that the widget is at
    /// the top (or as close as the content allows), after this frame is laid out. Use it to jump to
    /// a chapter from a table of contents.
    ///
    /// ```
    /// # let mut jump_to: Option<usize> = Some(2);
    /// # kargui::test::example(|ui| {
    /// ui.scroll(|ui| {
    ///     for chapter in 0..5 {
    ///         let heading = ui.heading(format!("Chapter {chapter}"));
    ///         if jump_to == Some(chapter) {
    ///             heading.scroll_into_view();
    ///         }
    ///         ui.label("...").wrap();
    ///     }
    /// });
    /// jump_to = None;
    /// # });
    /// ```
    pub fn scroll_into_view(self) -> Self {
        *self.cx_scroll = Some(self.id);
        self
    }

    fn kind(&mut self) -> &mut Kind {
        &mut self.tree.info[self.idx].kind
    }
}

macro_rules! common {
    // `$setup` is one hidden line of the examples (it declares the variable `$make` uses);
    // `$make` is the call on `ui` that creates the widget.
    ($t:ident, $setup:literal, $make:literal) => {
        impl<'t> $t<'t> {
            /// Sets the width. See [`Handle::width`].
            ///
            /// ```
            /// # use kargui::Size;
            #[doc = $setup]
            /// # kargui::test::example(|ui| {
            #[doc = concat!(" ui.", $make, ".width(Size::Fixed(120.0));")]
            /// # });
            /// ```
            pub fn width(self, size: Size) -> Self {
                Self { h: self.h.width(size), ..self }
            }
            /// Takes the remaining space. See [`Handle::grow`].
            ///
            /// ```
            #[doc = $setup]
            /// # kargui::test::example(|ui| {
            /// ui.row(|ui| {
            #[doc = concat!("     ui.", $make, ".grow();")]
            /// });
            /// # });
            /// ```
            pub fn grow(self) -> Self {
                Self { h: self.h.grow(), ..self }
            }
            /// Sets the accessibility name. See [`Handle::label`].
            ///
            /// ```
            #[doc = $setup]
            /// # kargui::test::example(|ui| {
            #[doc = concat!(" ui.", $make, ".label(\"Settings\");")]
            /// # });
            /// ```
            pub fn label(self, name: impl Into<String>) -> Self {
                Self { h: self.h.label(name), ..self }
            }
            /// The widget's identity.
            ///
            /// ```
            #[doc = $setup]
            /// # kargui::test::example(|ui| {
            #[doc = concat!(" let first = ui.", $make, ".id();")]
            #[doc = concat!(" let second = ui.", $make, ".id();")]
            /// assert_ne!(first, second);
            /// # });
            /// ```
            pub fn id(&self) -> Id {
                self.h.id
            }
            /// Scrolls the nearest scroll container so this widget is at its top. See [`Handle::scroll_into_view`].
            ///
            /// ```
            #[doc = $setup]
            /// # kargui::test::example(|ui| {
            /// ui.scroll(|ui| {
            #[doc = concat!("     ui.", $make, ".scroll_into_view();")]
            /// });
            /// # });
            /// ```
            pub fn scroll_into_view(self) -> Self {
                Self { h: self.h.scroll_into_view(), ..self }
            }
        }
    };
}

/// A text widget. See [`Ui::label`].
///
/// ```
/// # kargui::test::example(|ui| {
/// ui.label("Plain text");
/// ui.label("Quiet text").weak().small();
/// # });
/// ```
pub struct Label<'t>(Handle<'t>);

impl Label<'_> {
    fn text_kind(mut self, f: impl FnOnce(&mut TextKind, &mut bool, &mut bool, &mut bool, &mut bool)) -> Self {
        if let Kind::Label { size, bold, weak, strike, wrap, .. } = self.0.kind() {
            f(size, bold, weak, strike, wrap);
        }
        self
    }
    /// Draws in the less prominent text color.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.label("3 of 10 done").weak();
    /// # });
    /// ```
    pub fn weak(self) -> Self {
        self.text_kind(|_, _, w, _, _| *w = true)
    }
    /// Draws smaller.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.label("Saved a minute ago").small();
    /// # });
    /// ```
    pub fn small(self) -> Self {
        self.text_kind(|s, _, _, _, _| *s = TextKind::Small)
    }
    /// Draws bold.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.label("Total: 1,280").bold();
    /// # });
    /// ```
    pub fn bold(self) -> Self {
        self.text_kind(|_, b, _, _, _| *b = true)
    }
    /// Draws a line through the text (for example for finished tasks) when `on` is true.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let done = true;
    /// ui.label("Buy milk").strike(done);
    /// # });
    /// ```
    pub fn strike(self, on: bool) -> Self {
        self.text_kind(|_, _, w, s, _| {
            *s = on;
            *w |= on;
        })
    }
    /// Wraps the text onto several lines to fit the width. In a row, the text also takes the
    /// remaining width.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.label("A long paragraph that does not fit on one line is wrapped to the width.").wrap();
    /// # });
    /// ```
    pub fn wrap(self) -> Self {
        let l = self.text_kind(|_, _, _, _, w| *w = true);
        // 列の中では幅いっぱいに広がるので、高さを残りいっぱいにしない（下の部品を押し下げないように）
        if l.0.parent_dir == Dir::Row { l.grow() } else { l }
    }
    /// Sets the text size in logical pixels (body text is 14, headings 21). Use it for text the
    /// user can enlarge, such as a reading view.
    ///
    /// ```
    /// # let font_size = 18.0;
    /// # kargui::test::example(|ui| {
    /// ui.label("Large text for reading.").size(font_size).wrap();
    /// # });
    /// ```
    pub fn size(self, size: f32) -> Self {
        let tenths = (size.clamp(4.0, 200.0) * 10.0).round() as u32;
        self.text_kind(|s, _, _, _, _| *s = TextKind::Custom(tenths))
    }
    /// Puts the text at the right end of its space (for example numbers in a table column).
    /// Give the label a width with [`Label::width`] or [`Label::grow`] for this to show.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Apples").grow();
    ///     ui.label("1,280").right().width(kargui::Size::Fixed(80.0));
    /// });
    /// # });
    /// ```
    pub fn right(mut self) -> Self {
        if let Kind::Label { right, .. } = self.0.kind() {
            *right = true;
        }
        self
    }
    /// Takes the remaining space. See [`Handle::grow`].
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Title").grow();
    ///     ui.button("Close");
    /// });
    /// # });
    /// ```
    pub fn grow(self) -> Self {
        Label(self.0.grow())
    }
    /// Sets the width. See [`Handle::width`].
    ///
    /// ```
    /// # use kargui::Size;
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Name").width(Size::Fixed(80.0));
    ///     ui.label("Ada");
    /// });
    /// # });
    /// ```
    pub fn width(self, size: Size) -> Self {
        Label(self.0.width(size))
    }
    /// Scrolls the nearest scroll container so this text is at its top. See [`Handle::scroll_into_view`].
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.scroll(|ui| {
    ///     for i in 0..30 {
    ///         ui.label(format!("Line {i}"));
    ///     }
    ///     ui.label("The end").scroll_into_view();
    /// });
    /// # });
    /// ```
    pub fn scroll_into_view(self) -> Self {
        Label(self.0.scroll_into_view())
    }
}

/// A button. See [`Ui::button`].
///
/// ```
/// # kargui::test::example(|ui| {
/// let button = ui.button("Save");
/// assert!(!button.clicked());
/// # });
/// ```
pub struct Button<'t> {
    h: Handle<'t>,
    clicked: bool,
}

common!(Button, "", "button(\"OK\")");

impl Button<'_> {
    /// Draws in the main color, for the most important action.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.button("Save").primary();
    /// # });
    /// ```
    pub fn primary(mut self) -> Self {
        if let Kind::Button { primary, .. } = self.h.kind() {
            *primary = true;
        }
        self
    }
    /// Draws smaller (for buttons in list rows).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Milk");
    ///     ui.button("Delete").small();
    /// });
    /// # });
    /// ```
    pub fn small(mut self) -> Self {
        if let Kind::Button { small, .. } = self.h.kind() {
            *small = true;
        }
        self
    }
    /// Whether the button was clicked.
    ///
    /// ```
    /// # let mut count = 0;
    /// # kargui::test::example(|ui| {
    /// if ui.button("Add one").clicked() {
    ///     count += 1;
    /// }
    /// # });
    /// ```
    pub fn clicked(&self) -> bool {
        self.clicked
    }
}

/// A check box or switch. See [`Ui::checkbox`] and [`Ui::toggle`].
///
/// ```
/// # let (mut done, mut dark) = (false, false);
/// # kargui::test::example(|ui| {
/// ui.checkbox(&mut done, "Done");
/// ui.toggle(&mut dark, "Dark mode");
/// # });
/// ```
pub struct Toggled<'t> {
    h: Handle<'t>,
    changed: bool,
}

common!(Toggled, " # let mut on = false;", "toggle(&mut on, \"Wi-Fi\")");

impl Toggled<'_> {
    /// Whether it was clicked (and the value flipped).
    ///
    /// ```
    /// # let mut sound = true;
    /// # kargui::test::example(|ui| {
    /// if ui.toggle(&mut sound, "Sound").changed() {
    ///     // save the setting
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
    /// Draws a line through a check box's label and dims it when `on` is true, for example for
    /// finished tasks. The label stays clickable. Has no effect on switches.
    ///
    /// ```
    /// # let mut done = true;
    /// # kargui::test::example(|ui| {
    /// ui.checkbox(&mut done, "Buy milk").strike(done);
    /// # });
    /// ```
    pub fn strike(mut self, on: bool) -> Self {
        if let Kind::Checkbox { strike, .. } = self.h.kind() {
            *strike = on;
        }
        self
    }
}

/// A number that a [`Ui::slider`] can change: `f32`, `f64` and the integer types.
/// Integers move in steps of 1.
///
/// ```
/// # let (mut level, mut ratio) = (3_u8, 0.5_f64);
/// # kargui::test::example(|ui| {
/// ui.slider(&mut level, 0..=10);
/// ui.slider(&mut ratio, 0.0..=1.0);
/// # });
/// ```
pub trait SliderValue: Copy {
    #[doc(hidden)]
    const INTEGER: bool;
    #[doc(hidden)]
    fn to_f64(self) -> f64;
    #[doc(hidden)]
    fn from_f64(v: f64) -> Self;
}

macro_rules! slider_value {
    ($integer:expr; $($t:ty),*) => {
        $(impl SliderValue for $t {
            const INTEGER: bool = $integer;
            fn to_f64(self) -> f64 {
                self as f64
            }
            fn from_f64(v: f64) -> Self {
                v as $t
            }
        })*
    };
}
slider_value!(true; i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
slider_value!(false; f32, f64);

/// A slider. See [`Ui::slider`].
///
/// ```
/// # let mut volume = 40;
/// # kargui::test::example(|ui| {
/// let slider = ui.slider(&mut volume, 0..=100).label("Volume");
/// assert!(!slider.changed());
/// # });
/// ```
pub struct Slider<'t> {
    h: Handle<'t>,
    changed: bool,
}

common!(Slider, " # let mut level = 5;", "slider(&mut level, 0..=10)");

impl Slider<'_> {
    /// Whether the value changed in this frame (by pointer, keyboard or screen reader).
    ///
    /// ```
    /// # let mut volume = 40;
    /// # kargui::test::example(|ui| {
    /// if ui.slider(&mut volume, 0..=100).label("Volume").changed() {
    ///     // apply the new volume
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// A text field. See [`Ui::text_input`].
///
/// ```
/// # let mut name = String::new();
/// # kargui::test::example(|ui| {
/// let field = ui.text_input(&mut name).hint("Your name");
/// assert!(!field.changed());
/// # });
/// ```
pub struct TextInput<'t> {
    h: Handle<'t>,
    changed: bool,
    submitted: bool,
    composing: Option<String>,
}

common!(TextInput, " # let mut text = String::new();", "text_input(&mut text)");

impl TextInput<'_> {
    /// Text shown while the field is empty.
    ///
    /// ```
    /// # let mut query = String::new();
    /// # kargui::test::example(|ui| {
    /// ui.text_input(&mut query).hint("Search");
    /// # });
    /// ```
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        if let Kind::TextInput { hint: h, .. } = self.h.kind() {
            *h = hint.into();
        }
        self
    }
    /// Whether the text changed. Composing with the input method does not count until committed.
    ///
    /// ```
    /// # let mut query = String::new();
    /// # kargui::test::example(|ui| {
    /// if ui.text_input(&mut query).hint("Search").changed() {
    ///     // run the search
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
    /// Whether Enter was pressed (not the Enter that commits an input method conversion).
    ///
    /// ```
    /// # let mut query = String::new();
    /// # kargui::test::example(|ui| {
    /// if ui.text_input(&mut query).submitted() {
    ///     // Enter was pressed
    /// }
    /// # });
    /// ```
    pub fn submitted(&self) -> bool {
        self.submitted
    }
    /// The text the input method is composing, if any (shown underlined in the field, not yet in the value).
    ///
    /// ```
    /// # let mut query = String::new();
    /// # kargui::test::example(|ui| {
    /// let field = ui.text_input(&mut query);
    /// if let Some(kana) = field.composing() {
    ///     // the input method is converting `kana`
    ///     assert!(!kana.is_empty());
    /// }
    /// # });
    /// ```
    pub fn composing(&self) -> Option<&str> {
        self.composing.as_deref()
    }
}

/// A virtual list being set up. See [`Ui::list`].
///
/// ```
/// # kargui::test::example(|ui| {
/// let list = ui.list(100, 28.0);
/// list.show(|ui, i| {
///     ui.label(format!("Row {i}"));
/// });
/// # });
/// ```
pub struct List<'u, 'a> {
    ui: &'u mut Ui<'a>,
    idx: usize,
    id: Id,
    count: usize,
    row_h: f32,
    scroll_to: Option<f32>,
    selection: Option<&'u mut Option<usize>>,
}

impl<'u> List<'u, '_> {
    /// Takes the remaining space. See [`Handle::grow`].
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.heading("Files");
    /// ui.list(100, 28.0).grow().show(|ui, i| {
    ///     ui.label(format!("File {i}"));
    /// });
    /// # });
    /// ```
    pub fn grow(self) -> Self {
        let dir = self.ui.parent_dir();
        let l = &mut self.ui.tree.layout[self.idx];
        if dir == Dir::Row {
            l.width = Size::Fill(1.0);
        } else {
            l.height = Size::Fill(1.0);
        }
        self
    }
    /// Sets the height ([`Size::Fixed`] is in logical pixels).
    ///
    /// ```
    /// # use kargui::Size;
    /// # kargui::test::example(|ui| {
    /// ui.list(100, 28.0).height(Size::Fixed(200.0)).show(|ui, i| {
    ///     ui.label(format!("Row {i}"));
    /// });
    /// # });
    /// ```
    pub fn height(self, size: Size) -> Self {
        let s = match size {
            Size::Fixed(v) => Size::Fixed((v * self.ui.cx.scale).round()),
            s => s,
        };
        self.ui.tree.layout[self.idx].height = s;
        self
    }
    /// Draws lines between rows (on by default).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.list(100, 28.0).separators(false).show(|ui, i| {
    ///     ui.label(format!("Row {i}"));
    /// });
    /// # });
    /// ```
    pub fn separators(self, on: bool) -> Self {
        if let Kind::List { separators, .. } = &mut self.ui.tree.info[self.idx].kind {
            *separators = on;
        }
        self
    }
    /// Scrolls so that the content starts `offset` logical pixels above the top of the list.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// // Jump back to the top.
    /// ui.list(100, 28.0).scroll_to(0.0).show(|ui, i| {
    ///     ui.label(format!("Row {i}"));
    /// });
    /// # });
    /// ```
    pub fn scroll_to(mut self, offset: f32) -> Self {
        self.scroll_to = Some(offset);
        self
    }
    /// The current scroll position in logical pixels.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let list = ui.list(100, 28.0);
    /// assert_eq!(list.scroll_offset(), 0.0);
    /// list.show(|ui, i| {
    ///     ui.label(format!("Row {i}"));
    /// });
    /// # });
    /// ```
    pub fn scroll_offset(&self) -> f32 {
        self.ui.cx.scrolls.get(&self.id).copied().unwrap_or(0.0)
    }

    /// Lets the user select one row: clicking a row selects it, the up and down arrows move the
    /// selection (Home and End jump to the ends), and the list scrolls to show it. `selected` holds
    /// the selected row's index. The whole list is one stop for the Tab key.
    ///
    /// ```
    /// # let files = vec!["a.txt", "b.txt", "c.txt"];
    /// # let mut selected: Option<usize> = None;
    /// # kargui::test::example(|ui| {
    /// ui.list(files.len(), 28.0).grow().selection(&mut selected).show(|ui, i| {
    ///     ui.label(files[i]);
    /// });
    /// if let Some(i) = selected {
    ///     ui.label(format!("Selected: {}", files[i]));
    /// }
    /// # });
    /// ```
    pub fn selection(mut self, selected: &'u mut Option<usize>) -> Self {
        self.selection = Some(selected);
        self
    }

    /// Creates the visible rows. `row` is called with each visible row's index.
    ///
    /// Rows are told apart by their position, so when items are inserted or removed above,
    /// the rows below get new identities. If the items have ids of their own, use
    /// [`List::show_keyed`] instead.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let id = ui.list(100, 28.0).show(|ui, i| {
    ///     ui.label(format!("Row {i}"));
    /// });
    /// assert_eq!(id, id);
    /// # });
    /// ```
    pub fn show(self, row: impl FnMut(&mut Ui, usize)) -> Id {
        self.show_keyed(|i| i, row)
    }

    /// Like [`List::show`], but rows are told apart by `key(index)` (for example a database id).
    /// Focus and other widget state then stay with the item when items are inserted or removed
    /// above it, and screen readers do not see every row change.
    ///
    /// ```
    /// # struct Task { id: u32, text: String }
    /// # let tasks = vec![Task { id: 7, text: "Milk".into() }, Task { id: 3, text: "Bread".into() }];
    /// # kargui::test::example(|ui| {
    /// ui.list(tasks.len(), 32.0).grow().show_keyed(|i| tasks[i].id, |ui, i| {
    ///     ui.label(&tasks[i].text);
    /// });
    /// # });
    /// ```
    pub fn show_keyed<K: Hash>(self, key: impl Fn(usize) -> K, mut row: impl FnMut(&mut Ui, usize)) -> Id {
        let List { ui, idx, id, count, row_h, scroll_to, mut selection } = self;
        let row_id = |i: usize| Id(hash_of((id.0, key(i))));
        let cx = &mut *ui.cx;
        // 見えている範囲は前のフレームの一覧の高さで決める（初めてなら窓の高さ）
        let view_h = cx.prev.lists.iter().find(|l| l.id == id).map_or(cx.height, |l| l.rect.h);
        let scale = cx.scale;
        // 選べる一覧: 押した行を選び、焦点がある時は矢印で選ぶ行を動かして、見えるようにスクロールする
        let mut sel = selection.as_deref().copied().flatten().filter(|&s| s < count);
        if let Some(out) = selection.as_deref_mut() {
            if let Some(&(list, i)) = cx.clicked.and_then(|c| cx.prev.items.get(&c)) {
                if list == id && i < count {
                    sel = Some(i);
                }
            }
            let focused = cx.focus.and_then(|f| cx.prev.items.get(&f)).filter(|(l, _)| *l == id).map(|&(_, i)| i);
            if let (Some(at), true) = (focused, count > 0) {
                let start = sel.unwrap_or(at.min(count - 1));
                let mut next = start;
                for e in std::mem::take(&mut cx.keys) {
                    if let Event::Key { key, .. } = e {
                        next = match key {
                            Key::Up => next.saturating_sub(1),
                            Key::Down => (next + 1).min(count - 1),
                            Key::Home => 0,
                            Key::End => count - 1,
                            _ => next,
                        };
                    }
                }
                if next != start {
                    sel = Some(next);
                    cx.focus = Some(row_id(next));
                    let (rh, view) = (row_h / scale, view_h / scale);
                    let top = next as f32 * rh;
                    let off = cx.scrolls.entry(id).or_insert(0.0);
                    if top < *off {
                        *off = top;
                    } else if top + rh > *off + view {
                        *off = top + rh - view;
                    }
                }
            }
            *out = sel;
        }
        let content_h = count as f32 * row_h;
        let max = ((content_h - view_h) / scale).max(0.0);
        let offset = scroll_to.unwrap_or_else(|| cx.scrolls.get(&id).copied().unwrap_or(0.0)).clamp(0.0, max);
        cx.scrolls.insert(id, offset);
        let off_px = (offset * scale).round();
        ui.tree.layout[idx].offset = off_px;
        if let Kind::List { view_h: v, .. } = &mut ui.tree.info[idx].kind {
            *v = view_h;
        }
        // 見えている行に加えて、上下に 1 行ずつ見えない行も作る。Tab でその行へ移ると、見えるようにスクロールする
        let first = ((off_px / row_h).floor() as usize).min(count).saturating_sub(1);
        let last = ((((off_px + view_h) / row_h).ceil() as usize) + 1).min(count);
        // Tab で移る行: 選んだ行。無ければ見えている最初の行
        let tab_row = sel.unwrap_or(((off_px / row_h).floor() as usize).min(count.saturating_sub(1)));
        for i in first..last {
            let rid = row_id(i);
            let kind = if selection.is_some() { Kind::Item { selected: sel == Some(i), tab_stop: i == tab_row } } else { Kind::Row };
            let mut list_ui = ui.child(idx);
            let (ridx, _) = list_ui.push(rid, kind, Dir::Row);
            list_ui.tree.layout[ridx].row_index = i;
            list_ui.tree.layout[ridx].gap = (8.0 * scale).round();
            // 左右 12: 右端に重なるスクロールの棒（端から 3〜9 px）に、行の中身がかからない
            list_ui.tree.layout[ridx].pad = ((ROW_PAD * scale).round(), 0.0);
            let mut row_ui = list_ui.child(ridx);
            row(&mut row_ui, i);
        }
        id
    }
}

/// 一覧の行の左右の余白（論理ピクセル）
const ROW_PAD: f32 = 12.0;

/// 部品の文字（読み上げの名前の元）
pub(crate) fn visible_text(kind: &Kind) -> Option<&str> {
    match kind {
        Kind::Label { text, .. } | Kind::Button { text, .. } | Kind::Checkbox { text, .. } | Kind::Toggle { text, .. } => Some(text),
        Kind::Segment { text, .. } => Some(text),
        _ => None,
    }
}
