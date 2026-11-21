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

pub(crate) mod a11y;
pub(crate) mod context;
pub(crate) mod draw;
pub(crate) mod edit;
pub(crate) mod layout;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use context::Context;
pub use layout::{Align, Size};
use layout::{Dir, LayoutNode};

use crate::input::{Event, Key};
use crate::theme::Theme;

/// Identifies a widget across frames.
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
}

impl TextKind {
    /// 論理ピクセル
    pub fn size(self) -> f32 {
        match self {
            TextKind::Body => 14.0,
            TextKind::Small => 12.5,
            TextKind::Heading => 21.0,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Root,
    Row,
    Column,
    Stack,
    Label { text: String, size: TextKind, bold: bool, weak: bool, strike: bool, wrap: bool },
    Button { text: String, primary: bool, small: bool },
    Checkbox { checked: bool, text: String, strike: bool },
    Toggle { on: bool, text: String },
    Segment { text: String, selected: bool },
    TextInput { value: String, hint: String },
    Space,
    List { count: usize, separators: bool, view_h: f32 },
}

impl Kind {
    pub fn interactive(&self) -> bool {
        matches!(self, Kind::Button { .. } | Kind::Checkbox { .. } | Kind::Toggle { .. } | Kind::Segment { .. } | Kind::TextInput { .. })
    }
}

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
        Handle { tree: &mut *self.tree, idx, id, scale: self.cx.scale, parent_dir }
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
        self.handle(idx, id)
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
        let kind = Kind::Label { text: text.into(), size: TextKind::Body, bold: false, weak: false, strike: false, wrap: false };
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
        let kind = Kind::Label { text: text.into(), size: TextKind::Heading, bold: true, weak: false, strike: false, wrap: false };
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
    ///
    /// ```
    /// # let mut dark = false;
    /// # kargui::test::example(|ui| {
    /// ui.toggle(&mut dark, "Dark mode");
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

    // ---- 一覧 ----

    /// A list that only creates the rows that are visible, so it stays fast with any number of
    /// rows. All rows have the same height (logical pixels). Finish with [`List::show`].
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
    #[track_caller]
    pub fn list(&mut self, count: usize, row_height: f32) -> List<'_, 'a> {
        let (idx, id) = self.add(Kind::List { count, separators: true, view_h: 0.0 }, Dir::List);
        let row_h = self.px(row_height).max(1.0);
        self.tree.layout[idx].row_h = row_h;
        List { ui: self, idx, id, count, row_h, scroll_to: None }
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
pub struct Handle<'t> {
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
    pub fn width(self, size: Size) -> Self {
        let s = self.px(size);
        self.tree.layout[self.idx].width = s;
        self
    }

    /// Sets the height ([`Size::Fixed`] is in logical pixels).
    pub fn height(self, size: Size) -> Self {
        let s = self.px(size);
        self.tree.layout[self.idx].height = s;
        self
    }

    /// Takes the remaining space along the parent's direction (width in a row, height in a column).
    pub fn grow(self) -> Self {
        match self.parent_dir {
            Dir::Column => self.height(Size::Fill(1.0)),
            _ => self.width(Size::Fill(1.0)),
        }
    }

    /// Space between the children (logical pixels).
    pub fn gap(self, length: f32) -> Self {
        self.tree.layout[self.idx].gap = (length * self.scale).round();
        self
    }

    /// Space inside the edges (logical pixels): `x` left and right, `y` top and bottom.
    pub fn padding(self, x: f32, y: f32) -> Self {
        self.tree.layout[self.idx].pad = ((x * self.scale).round(), (y * self.scale).round());
        self
    }

    /// Where the children go across the main direction.
    pub fn align(self, align: Align) -> Self {
        self.tree.layout[self.idx].align = align;
        self
    }

    /// The name screen readers use for this widget, when its visible text is not enough
    /// (for example an icon-only button).
    pub fn label(self, name: impl Into<String>) -> Self {
        self.tree.info[self.idx].label = Some(name.into());
        self
    }

    /// The widget's identity.
    pub fn id(&self) -> Id {
        self.id
    }

    fn kind(&mut self) -> &mut Kind {
        &mut self.tree.info[self.idx].kind
    }
}

macro_rules! common {
    ($t:ident) => {
        impl<'t> $t<'t> {
            /// Sets the width. See [`Handle::width`].
            pub fn width(self, size: Size) -> Self {
                Self { h: self.h.width(size), ..self }
            }
            /// Takes the remaining space. See [`Handle::grow`].
            pub fn grow(self) -> Self {
                Self { h: self.h.grow(), ..self }
            }
            /// Sets the accessibility name. See [`Handle::label`].
            pub fn label(self, name: impl Into<String>) -> Self {
                Self { h: self.h.label(name), ..self }
            }
            /// The widget's identity.
            pub fn id(&self) -> Id {
                self.h.id
            }
        }
    };
}

/// A text widget. See [`Ui::label`].
pub struct Label<'t>(Handle<'t>);

impl Label<'_> {
    fn text_kind(mut self, f: impl FnOnce(&mut TextKind, &mut bool, &mut bool, &mut bool, &mut bool)) -> Self {
        if let Kind::Label { size, bold, weak, strike, wrap, .. } = self.0.kind() {
            f(size, bold, weak, strike, wrap);
        }
        self
    }
    /// Draws in the less prominent text color.
    pub fn weak(self) -> Self {
        self.text_kind(|_, _, w, _, _| *w = true)
    }
    /// Draws smaller.
    pub fn small(self) -> Self {
        self.text_kind(|s, _, _, _, _| *s = TextKind::Small)
    }
    /// Draws bold.
    pub fn bold(self) -> Self {
        self.text_kind(|_, b, _, _, _| *b = true)
    }
    /// Draws a line through the text (for example for finished tasks) when `on` is true.
    pub fn strike(self, on: bool) -> Self {
        self.text_kind(|_, _, w, s, _| {
            *s = on;
            *w |= on;
        })
    }
    /// Wraps the text onto several lines to fit the width.
    pub fn wrap(self) -> Self {
        self.text_kind(|_, _, _, _, w| *w = true).grow()
    }
    /// Takes the remaining space. See [`Handle::grow`].
    pub fn grow(self) -> Self {
        Label(self.0.grow())
    }
    /// Sets the width. See [`Handle::width`].
    pub fn width(self, size: Size) -> Self {
        Label(self.0.width(size))
    }
}

/// A button. See [`Ui::button`].
pub struct Button<'t> {
    h: Handle<'t>,
    clicked: bool,
}

common!(Button);

impl Button<'_> {
    /// Draws in the main color, for the most important action.
    pub fn primary(mut self) -> Self {
        if let Kind::Button { primary, .. } = self.h.kind() {
            *primary = true;
        }
        self
    }
    /// Draws smaller (for buttons in list rows).
    pub fn small(mut self) -> Self {
        if let Kind::Button { small, .. } = self.h.kind() {
            *small = true;
        }
        self
    }
    /// Whether the button was clicked.
    pub fn clicked(&self) -> bool {
        self.clicked
    }
}

/// A check box or switch. See [`Ui::checkbox`] and [`Ui::toggle`].
pub struct Toggled<'t> {
    h: Handle<'t>,
    changed: bool,
}

common!(Toggled);

impl Toggled<'_> {
    /// Whether it was clicked (and the value flipped).
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

/// A text field. See [`Ui::text_input`].
pub struct TextInput<'t> {
    h: Handle<'t>,
    changed: bool,
    submitted: bool,
    composing: Option<String>,
}

common!(TextInput);

impl TextInput<'_> {
    /// Text shown while the field is empty.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        if let Kind::TextInput { hint: h, .. } = self.h.kind() {
            *h = hint.into();
        }
        self
    }
    /// Whether the text changed. Composing with the input method does not count until committed.
    pub fn changed(&self) -> bool {
        self.changed
    }
    /// Whether Enter was pressed (not the Enter that commits an input method conversion).
    pub fn submitted(&self) -> bool {
        self.submitted
    }
    /// The text the input method is composing, if any (shown underlined in the field, not yet in the value).
    pub fn composing(&self) -> Option<&str> {
        self.composing.as_deref()
    }
}

/// A virtual list being set up. See [`Ui::list`].
pub struct List<'u, 'a> {
    ui: &'u mut Ui<'a>,
    idx: usize,
    id: Id,
    count: usize,
    row_h: f32,
    scroll_to: Option<f32>,
}

impl List<'_, '_> {
    /// Takes the remaining space. See [`Handle::grow`].
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
    pub fn height(self, size: Size) -> Self {
        let s = match size {
            Size::Fixed(v) => Size::Fixed((v * self.ui.cx.scale).round()),
            s => s,
        };
        self.ui.tree.layout[self.idx].height = s;
        self
    }
    /// Draws lines between rows (on by default).
    pub fn separators(self, on: bool) -> Self {
        if let Kind::List { separators, .. } = &mut self.ui.tree.info[self.idx].kind {
            *separators = on;
        }
        self
    }
    /// Scrolls so that the content starts `offset` logical pixels above the top of the list.
    pub fn scroll_to(mut self, offset: f32) -> Self {
        self.scroll_to = Some(offset);
        self
    }
    /// The current scroll position in logical pixels.
    pub fn scroll_offset(&self) -> f32 {
        self.ui.cx.scrolls.get(&self.id).copied().unwrap_or(0.0)
    }

    /// Creates the visible rows. `row` is called with each visible row's index.
    pub fn show(self, mut row: impl FnMut(&mut Ui, usize)) -> Id {
        let cx = &mut *self.ui.cx;
        // 見えている範囲は前のフレームの一覧の高さで決める（初めてなら窓の高さ）
        let view_h = cx.prev.lists.iter().find(|l| l.id == self.id).map_or(cx.height, |l| l.rect.h);
        let content_h = self.count as f32 * self.row_h;
        let max = ((content_h - view_h) / cx.scale).max(0.0);
        let offset = self.scroll_to.unwrap_or_else(|| cx.scrolls.get(&self.id).copied().unwrap_or(0.0)).clamp(0.0, max);
        cx.scrolls.insert(self.id, offset);
        let off_px = (offset * cx.scale).round();
        let scale = cx.scale;
        self.ui.tree.layout[self.idx].offset = off_px;
        if let Kind::List { view_h: v, .. } = &mut self.ui.tree.info[self.idx].kind {
            *v = view_h;
        }
        let first = ((off_px / self.row_h).floor() as usize).min(self.count);
        let last = (((off_px + view_h) / self.row_h).ceil() as usize).min(self.count);
        for i in first..last {
            let rid = Id(hash_of((self.id.0, i)));
            let mut list_ui = self.ui.child(self.idx);
            let (ridx, _) = list_ui.push(rid, Kind::Row, Dir::Row);
            list_ui.tree.layout[ridx].row_index = i;
            list_ui.tree.layout[ridx].gap = (8.0 * scale).round();
            list_ui.tree.layout[ridx].pad = ((8.0 * scale).round(), 0.0);
            let mut row_ui = list_ui.child(ridx);
            row(&mut row_ui, i);
        }
        self.id
    }
}

/// 部品の文字（読み上げの名前の元）
pub(crate) fn visible_text(kind: &Kind) -> Option<&str> {
    match kind {
        Kind::Label { text, .. } | Kind::Button { text, .. } | Kind::Checkbox { text, .. } | Kind::Toggle { text, .. } => Some(text),
        Kind::Segment { text, .. } => Some(text),
        _ => None,
    }
}
