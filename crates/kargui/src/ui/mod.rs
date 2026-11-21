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

use context::{Context, TipState};
pub use layout::{Align, Size};
use layout::{Dir, LayoutNode};

use crate::date::{Date, DateText};
use crate::file_dialog::FileDialog;
use std::path::PathBuf;
use crate::geom::{Color, Rect};
use crate::input::{Event, Key, Mods};
use crate::wake::Waker;
use crate::icon::Icon;
use crate::image::Image;
use std::time::{Duration, Instant};
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
    Button { text: String, primary: bool, small: bool, icon: Option<Icon> },
    /// アイコン（大きさは論理ピクセル）
    Icon { icon: Icon, size: u32, weak: bool },
    /// 画像（大きさは論理ピクセル）
    Image { image: Image, w: u32, h: u32 },
    /// ラジオボタン（tab_stop: グループの中で Tab が止まる 1 つ）
    Radio { selected: bool, text: String, tab_stop: bool },
    /// 区切り線
    Separator,
    /// 進み具合（value が None なら終わりが分からない。ring は輪）
    Progress { value: Option<f32>, ring: bool },
    /// カードと枠（title があれば枠の見出しで、読み上げの名前）
    Card { title: Option<String> },
    /// 開閉する節の見出し
    Expander { open: bool, text: String },
    /// リンク
    Link { text: String },
    /// 押したままにできるボタン（icon だけならツールバーのボタン）
    ToggleButton { on: bool, text: String, icon: Option<Icon> },
    /// ドロップダウン（text は選んでいる項目、widest は一番長い項目。幅を決める）
    Dropdown { text: String, widest: String, open: bool },
    /// 選択の一覧の項目（ドロップダウンの中）
    ListOption { text: String, selected: bool },
    /// メニューを開くボタン（Ui::menu_button。icon だけならツールバーの「ほか」）
    MenuButton { text: String, open: bool, icon: Option<Icon> },
    /// タブの入れ物（見出しの列と中身）
    Tabs,
    /// タブの見出しの列
    TabList,
    /// タブの見出し
    Tab { text: String, selected: bool },
    /// タブの中身
    TabPanel,
    /// 数の入力欄（text は欄に出す文字。value・min・max・step は読み上げにも渡す）
    NumberInput { text: String, value: f64, min: f64, max: f64, step: f64 },
    /// 数の入力欄の右の小さいボタン（読み上げの木には出さない）
    SpinButton { up: bool },
    /// ツールバー
    Toolbar,
    /// 格子の入れ物
    Grid,
    /// 仕切りで分けた 2 つの区画の入れ物（stacked なら上下）
    Split { stacked: bool },
    /// 区画の間の仕切り（value は 1 つ目の区画の長さ、論理ピクセル）
    Splitter { stacked: bool, value: f32, min: f32, max: f32 },
    /// 自由に描く面
    Canvas { ops: Vec<DrawOp> },
    /// 木の表示の入れ物
    Tree,
    /// 表の入れ物（見出しの行と、行の一覧）
    Table { columns: usize, rows: usize },
    /// 表の見出しの行
    TableHeader,
    /// 表の列の見出し（sort は並べ替えている列なら昇順か、sortable は押して並べ替えられるか）
    ColumnHeader { text: String, sort: Option<bool>, right: bool, sortable: bool, column: usize },
    /// 列の幅を変えるつまみ（読み上げの木には出さない）
    ColumnGrip,
    /// 日付の欄（text は出す文字、empty は日付が無い）
    DateField { text: String, empty: bool, open: bool },
    /// 月の暦の日のます目の入れ物（格子。読み上げは grid）
    DayGrid,
    /// 暦の 1 日（in_month はその月の日、today は今日、selected は選んでいる日）
    CalendarDay { day: u32, in_month: bool, today: bool, selected: bool },
    /// 木の表示の 1 行（depth は深さ、open は枝なら開いているか、葉は None）
    TreeItem { text: String, depth: usize, open: Option<bool>, selected: bool },
    /// メニューバー（見出しを横に並べる入れ物）
    MenuBar,
    /// メニューバーの見出し（tab_stop は Tab で止まる 1 つ。最初の見出し）
    MenuBarItem { text: String, open: bool, tab_stop: bool },
    /// メニューの項目（gutter は印かアイコンの列を空けるか。メニューの中で 1 つでもあれば全部が空ける）
    MenuItem { text: String, shortcut: Option<String>, icon: Option<Icon>, mark: MenuMark, submenu: bool, open: bool, gutter: bool },
    Checkbox { checked: bool, text: String, strike: bool, mixed: bool },
    Toggle { on: bool, text: String },
    Segment { text: String, selected: bool },
    TextInput { value: String, hint: String, password: bool },
    /// 複数行の入力欄（rows は見える行の数）
    TextArea { value: String, hint: String, rows: u32 },
    Space,
    /// 一覧（table は表の行の一覧。読み上げで行と升にする）
    List { count: usize, separators: bool, view_h: f32, table: bool },
    Scroll,
    /// 選べる一覧の行（tab_stop: Tab で移る行。選んだ行、無ければ見えている最初の行）
    Item { selected: bool, tab_stop: bool },
    Slider { value: f64, min: f64, max: f64, step: f64 },
    /// 重ねて出す物（浮き出し・ダイアログ・ツールチップ）。親に付けず、Tree::overlays から描く
    Overlay { kind: OverlayKind, title: String },
}

/// 重ねて出す物の種類
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverlayKind {
    /// 出す元の近くに出し、外を押すか Esc で閉じる（Ui::popup）
    Popup,
    /// 窓の真ん中に出し、後ろを押せなくして焦点を中に閉じ込める（Ui::dialog）
    Dialog,
    /// 出す元の下に出す説明（Handle::tooltip）。押せない
    Tooltip,
    /// 選択の一覧（ドロップダウン）。浮き出しと同じく、外を押すか Esc で閉じる
    ListBox,
    /// メニュー。浮き出しと同じく、外を押すか Esc で閉じる
    Menu,
}

impl OverlayKind {
    /// 外を押すと閉じる物（浮き出し・選択の一覧・メニュー）
    pub fn light_dismiss(self) -> bool {
        matches!(self, OverlayKind::Popup | OverlayKind::ListBox | OverlayKind::Menu)
    }
}

/// メニューの項目の印
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuMark {
    None,
    Check(bool),
    Radio(bool),
}

/// 重ねて出す物を置く場所（ダイアログは窓の真ん中）
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Place {
    /// 出す元の下（入らなければ上）
    Below,
    /// 出す元の横（右、入らなければ左）。入れ子のメニュー
    Beside,
    /// 窓の中のこの点（物理ピクセル）の右下。右クリックのメニュー
    At(f32, f32),
}

/// この回の画面で重ねて出す物
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Overlay {
    pub idx: usize,
    pub id: Id,
    /// 近くに出す元の部品（ダイアログと右クリックのメニューは無し）
    pub anchor: Option<Id>,
    pub kind: OverlayKind,
    pub place: Place,
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
                | Kind::TextArea { .. }
                | Kind::Slider { .. }
                | Kind::Item { .. }
                | Kind::Radio { .. }
                | Kind::Expander { .. }
                | Kind::Link { .. }
                | Kind::ToggleButton { .. }
                | Kind::Dropdown { .. }
                | Kind::ListOption { .. }
                | Kind::MenuButton { .. }
                | Kind::MenuBarItem { .. }
                | Kind::MenuItem { .. }
                | Kind::Tab { .. }
                | Kind::NumberInput { .. }
                | Kind::Splitter { .. }
                | Kind::TreeItem { .. }
                | Kind::ColumnHeader { .. }
                | Kind::DateField { .. }
                | Kind::CalendarDay { .. }
        )
    }
}

#[derive(PartialEq)]
pub(crate) struct NodeInfo {
    pub id: Id,
    pub kind: Kind,
    /// 読み上げの名前（無ければ部品の文字を使う）
    pub label: Option<String>,
    /// 押せない（Ui::enabled の範囲の中か、Button::enabled(false)）
    pub disabled: bool,
    /// 読み上げの説明（ツールチップの文）
    pub description: Option<String>,
    /// 右クリックのメニューを持つ（Handle::context_menu）。描く時にその範囲を覚える
    pub context_menu: bool,
    /// Tab で止まらない（焦点は持てる。タブとツールバーは、まとまりの中の 1 つだけが Tab で止まる）
    pub skip_tab: bool,
}

/// ビュー 1 回分の画面の木。info と layout は同じ番号で並ぶ。親は子より前に入る
pub(crate) struct Tree {
    pub info: Vec<NodeInfo>,
    pub layout: Vec<LayoutNode>,
    /// 重ねて出す物（作った順。後ろほど上に重なる）
    pub overlays: Vec<Overlay>,
    occurrences: HashMap<(u64, u64, &'static str, u32, u32), u32>,
}

impl Tree {
    /// 部品と、その並べ方の決まり（配置の結果の前）が同じか。ビューを回し直して画面が落ち着いたかを見るのに使う
    pub fn same_as(&self, other: &Tree) -> bool {
        self.info == other.info && self.layout == other.layout && self.overlays == other.overlays
    }

    /// 根は縦に並べる。既定の余白は左右 16・上下 12、間は 10（論理ピクセル）
    pub fn new(root: Id, scale: f32) -> Self {
        let mut layout = LayoutNode::new(Dir::Column);
        layout.align = Align::Stretch;
        layout.pad = ((16.0 * scale).round(), (12.0 * scale).round());
        layout.gap = (10.0 * scale).round();
        Self {
            info: vec![NodeInfo { id: root, kind: Kind::Root, label: None, disabled: false, description: None, context_menu: false, skip_tab: false }],
            layout: vec![layout],
            overlays: Vec::new(),
            occurrences: HashMap::new(),
        }
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
    /// この Ui で作る部品は押せない（Ui::enabled）
    pub(crate) disabled: bool,
}

impl<'a> Ui<'a> {
    pub(crate) fn new(cx: &'a mut Context, tree: &'a mut Tree) -> Self {
        Self { cx, tree, parent: 0, salt: 0, disabled: false }
    }

    fn px(&self, logical: f32) -> f32 {
        (logical * self.cx.scale).round()
    }

    #[track_caller]
    fn add(&mut self, kind: Kind, dir: Dir) -> (usize, Id) {
        let id = self.id_here(std::panic::Location::caller());
        self.push(id, kind, dir)
    }

    /// 呼んだ場所（ソースの位置）と親と、その場所を呼んだ回数から id を作る
    fn id_here(&mut self, loc: &'static std::panic::Location<'static>) -> Id {
        let pid = self.tree.info[self.parent].id.0;
        let key = (pid, self.salt, loc.file(), loc.line(), loc.column());
        let n = self.tree.occurrences.entry(key).or_insert(0);
        let occurrence = *n;
        *n += 1;
        Id(hash_of((key, occurrence)))
    }

    /// 重ねて出す物の入れ物を作る（親には付けず、Tree::overlays に入れる）
    fn push_overlay(&mut self, id: Id, kind: OverlayKind, title: String, anchor: Option<Id>) -> usize {
        let idx = self.tree.info.len();
        self.tree.info.push(NodeInfo { id, kind: Kind::Overlay { kind, title }, label: None, disabled: self.disabled, description: None, context_menu: false, skip_tab: false });
        self.tree.layout.push(LayoutNode::new(Dir::Column));
        self.tree.overlays.push(Overlay { idx, id, anchor, kind, place: Place::Below });
        idx
    }

    fn push(&mut self, id: Id, kind: Kind, dir: Dir) -> (usize, Id) {
        let idx = self.tree.info.len();
        self.tree.info.push(NodeInfo { id, kind, label: None, disabled: self.disabled, description: None, context_menu: false, skip_tab: false });
        self.tree.layout.push(LayoutNode::new(dir));
        self.tree.layout[self.parent].children.push(idx);
        (idx, id)
    }

    fn child(&mut self, parent: usize) -> Ui<'_> {
        Ui { cx: &mut *self.cx, tree: &mut *self.tree, parent, salt: 0, disabled: self.disabled }
    }

    fn parent_dir(&self) -> Dir {
        self.tree.layout[self.parent].dir
    }

    fn handle(&mut self, idx: usize, id: Id) -> Handle<'_> {
        let parent_dir = self.parent_dir();
        let tip = self.cx.tip_state(id);
        let scale = self.cx.scale;
        Handle { cx: &mut *self.cx, tree: &mut *self.tree, idx, id, scale, parent_dir, tip }
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
        let disabled = self.disabled;
        f(&mut Ui { cx: &mut *self.cx, tree: &mut *self.tree, parent, salt, disabled });
    }

    /// Makes the widgets created in `f` disabled when `enabled` is false: they are drawn dimmed,
    /// ignore the pointer and the keyboard, are skipped by Tab, and screen readers announce them
    /// as unavailable. A widget that had the focus loses it. Nesting keeps the outer setting, so
    /// `enabled(true, ..)` inside a disabled part stays disabled. The widgets are placed in the
    /// current row or column, as if `f`'s code were written here.
    ///
    /// ```
    /// # struct App { notify: bool, sound: bool, volume: u32 }
    /// # let mut app = App { notify: false, sound: true, volume: 5 };
    /// # kargui::test::example(|ui| {
    /// ui.checkbox(&mut app.notify, "Notifications");
    /// ui.enabled(app.notify, |ui| {
    ///     ui.checkbox(&mut app.sound, "Play a sound");
    ///     ui.slider(&mut app.volume, 0..=10).label("Volume");
    /// });
    /// # });
    /// ```
    pub fn enabled(&mut self, enabled: bool, f: impl FnOnce(&mut Ui)) {
        let (parent, salt) = (self.parent, self.salt);
        let disabled = self.disabled || !enabled;
        f(&mut Ui { cx: &mut *self.cx, tree: &mut *self.tree, parent, salt, disabled });
    }

    /// A box shown next to `anchor` (usually the button that opens it) while `*open` is true, above
    /// everything else. The keyboard focus moves into it when it opens. Clicking outside it or
    /// pressing Escape sets `*open` to false; that click does not reach the widget underneath. When
    /// it closes, the focus goes back to where it was. Clicking `anchor` again is left to your code
    /// (toggle `*open`). It opens below `anchor`, or above it when there is no room below.
    ///
    /// ```
    /// # let mut open = false;
    /// # kargui::test::example(|ui| {
    /// let more = ui.button("More");
    /// let anchor = more.id();
    /// if more.clicked() {
    ///     open = !open;
    /// }
    /// ui.popup(&mut open, anchor, |ui| {
    ///     ui.button("Rename");
    ///     ui.button("Delete");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn popup(&mut self, open: &mut bool, anchor: Id, f: impl FnOnce(&mut Ui)) {
        let id = self.id_here(std::panic::Location::caller());
        if self.cx.closing.contains(&id) {
            *open = false;
        }
        if !*open {
            return;
        }
        let idx = self.push_overlay(id, OverlayKind::Popup, String::new(), Some(anchor));
        let (pad, gap) = (self.px(6.0), self.px(4.0));
        let l = &mut self.tree.layout[idx];
        l.pad = (pad, pad);
        l.gap = gap;
        f(&mut self.child(idx));
    }

    /// A modal dialog with a title, in the middle of the window while `*open` is true. The rest of
    /// the window is dimmed and cannot be used; Tab moves only between the dialog's widgets, and the
    /// focus moves into it when it opens. Escape sets `*open` to false. Close it from your code (for
    /// example on OK or Cancel) by setting `*open` to false after this call; the focus then goes
    /// back to where it was. Screen readers announce it as a dialog with the title as its name.
    ///
    /// ```
    /// # let (mut open, mut name) = (true, String::new());
    /// # kargui::test::example(|ui| {
    /// let mut close = false;
    /// ui.dialog(&mut open, "Rename", |ui| {
    ///     ui.text_input(&mut name).hint("New name");
    ///     ui.row(|ui| {
    ///         ui.fill();
    ///         close |= ui.button("OK").primary().clicked();
    ///         close |= ui.button("Cancel").clicked();
    ///     });
    /// });
    /// if close {
    ///     open = false;
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn dialog(&mut self, open: &mut bool, title: impl Into<String>, f: impl FnOnce(&mut Ui)) {
        let id = self.id_here(std::panic::Location::caller());
        if self.cx.closing.contains(&id) {
            *open = false;
        }
        if !*open {
            return;
        }
        let title = title.into();
        let idx = self.push_overlay(id, OverlayKind::Dialog, title.clone(), None);
        let (px, py, gap) = (self.px(24.0), self.px(20.0), self.px(12.0));
        let l = &mut self.tree.layout[idx];
        l.pad = (px, py);
        l.gap = gap;
        let mut ui = self.child(idx);
        ui.heading(title);
        f(&mut ui);
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

    /// A group of radio buttons, one below another, of which exactly one is selected. Returns
    /// whether the selection changed. Tab stops once in the group (at the selected button); the
    /// arrow keys move through the group and select as they go, wrapping at the ends. For a
    /// compact row of choices, see [`Ui::segmented`].
    ///
    /// ```
    /// #[derive(Clone, Copy, PartialEq)]
    /// enum Size { Small, Medium, Large }
    /// # let mut size = Size::Medium;
    /// # kargui::test::example(|ui| {
    /// ui.radio_group(&mut size, &[(Size::Small, "Small"), (Size::Medium, "Medium"), (Size::Large, "Large")]);
    /// # });
    /// ```
    #[track_caller]
    pub fn radio_group<T: Copy + PartialEq>(&mut self, value: &mut T, options: &[(T, &str)]) -> bool {
        let (col, _) = self.add(Kind::Column, Dir::Column);
        self.tree.layout[col].gap = self.px(6.0);
        let disabled = self.disabled;
        let tab = options.iter().position(|(v, _)| v == value).unwrap_or(0);
        let mut changed = false;
        let mut ids = Vec::with_capacity(options.len());
        {
            let mut ui = self.child(col);
            for (k, (v, text)) in options.iter().enumerate() {
                ui.salt = hash_of(k);
                let (_, id) = ui.add(Kind::Radio { selected: *v == *value, text: text.to_string(), tab_stop: k == tab }, Dir::Leaf);
                ids.push(id);
                if !disabled && ui.cx.clicked == Some(id) && *v != *value {
                    *value = *v;
                    changed = true;
                }
            }
        }
        // 矢印: 焦点がこのグループのボタンにある時、選びながら移る（端で折り返す）
        if let (false, Some(at)) = (disabled, self.cx.focus.and_then(|f| ids.iter().position(|&i| i == f))) {
            let n = options.len();
            let mut next = at;
            for e in std::mem::take(&mut self.cx.keys) {
                if let Event::Key { key, .. } = e {
                    next = match key {
                        Key::Down | Key::Right => (next + 1) % n,
                        Key::Up | Key::Left => (next + n - 1) % n,
                        _ => next,
                    };
                }
            }
            if next != at {
                *value = options[next].0;
                changed = true;
                self.cx.focus = Some(ids[next]);
            }
        }
        if changed {
            let children = self.tree.layout[col].children.clone();
            let tab = options.iter().position(|(v, _)| v == value).unwrap_or(0);
            for (k, c) in children.into_iter().enumerate() {
                if let Kind::Radio { selected, tab_stop, .. } = &mut self.tree.info[c].kind {
                    *selected = options[k].0 == *value;
                    *tab_stop = k == tab;
                }
            }
        }
        changed
    }

    /// A thin line between parts of a column (horizontal) or a row (vertical).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.label("Account");
    /// ui.separator();
    /// ui.label("Privacy");
    /// # });
    /// ```
    #[track_caller]
    pub fn separator(&mut self) {
        let (idx, _) = self.add(Kind::Separator, Dir::Leaf);
        let (one, tall) = (Size::Fixed(self.px(1.0).max(1.0)), Size::Fixed(self.px(20.0)));
        let in_row = self.parent_dir() == Dir::Row;
        let l = &mut self.tree.layout[idx];
        if in_row {
            (l.width, l.height) = (one, tall);
        } else {
            l.height = one;
        }
    }

    /// A progress bar from 0.0 (nothing done) to 1.0 (finished). Give it a name with `.label(..)`
    /// so screen readers can say what is progressing.
    ///
    /// ```
    /// # let (done, total) = (3, 10);
    /// # kargui::test::example(|ui| {
    /// ui.progress(done as f32 / total as f32).label("Uploading photos");
    /// # });
    /// ```
    #[track_caller]
    pub fn progress(&mut self, fraction: f32) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Progress { value: Some(fraction.clamp(0.0, 1.0)), ring: false }, Dir::Leaf);
        self.handle(idx, id)
    }

    /// A progress bar for work whose end is unknown: a moving stripe. It draws about 30 times a
    /// second while it is on screen.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.progress_busy().label("Connecting");
    /// # });
    /// ```
    #[track_caller]
    pub fn progress_busy(&mut self) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Progress { value: None, ring: false }, Dir::Leaf);
        self.animate();
        self.handle(idx, id)
    }

    /// A small spinning ring for work whose end is unknown. It draws about 30 times a second
    /// while it is on screen.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.spinner().label("Loading");
    ///     ui.label("Loading…");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn spinner(&mut self) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Progress { value: None, ring: true }, Dir::Leaf);
        let s = Size::Fixed(self.px(20.0));
        (self.tree.layout[idx].width, self.tree.layout[idx].height) = (s, s);
        self.animate();
        self.handle(idx, id)
    }

    /// 動く部品のために、約 30 fps で起こしてもらう（毎フレームは描かない）
    fn animate(&mut self) {
        let t = self.cx.now + Duration::from_millis(33);
        self.cx.wake_by(t);
    }

    /// A card: the widgets created in `f`, one below another, on a box with a border.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.card(|ui| {
    ///     ui.heading("Storage");
    ///     ui.label("12 GB of 64 GB used").weak();
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn card(&mut self, f: impl FnOnce(&mut Ui)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Card { title: None }, Dir::Column);
        self.card_layout(idx);
        f(&mut self.child(idx));
        self.handle(idx, id)
    }

    /// A card with a title, for a group of related settings. Screen readers announce it as a
    /// group named by the title.
    ///
    /// ```
    /// # let (mut sound, mut vibrate) = (true, false);
    /// # kargui::test::example(|ui| {
    /// ui.group("Notifications", |ui| {
    ///     ui.checkbox(&mut sound, "Play a sound");
    ///     ui.checkbox(&mut vibrate, "Vibrate");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn group(&mut self, title: impl Into<String>, f: impl FnOnce(&mut Ui)) -> Handle<'_> {
        let title = title.into();
        let (idx, id) = self.add(Kind::Card { title: Some(title.clone()) }, Dir::Column);
        self.card_layout(idx);
        let mut ui = self.child(idx);
        ui.label(title).bold();
        f(&mut ui);
        self.handle(idx, id)
    }

    fn card_layout(&mut self, idx: usize) {
        let (px, py, gap) = (self.px(16.0), self.px(12.0), self.px(8.0));
        let l = &mut self.tree.layout[idx];
        l.pad = (px, py);
        l.gap = gap;
    }

    /// A section that opens and closes: a header with an arrow, and the widgets created in `f`
    /// below it while `*open` is true. Clicking the header (or Enter / Space on it) flips `*open`.
    /// Returns whether it was flipped. Screen readers hear whether it is expanded.
    ///
    /// ```
    /// # let (mut more, mut proxy) = (false, false);
    /// # kargui::test::example(|ui| {
    /// ui.expander(&mut more, "Advanced", |ui| {
    ///     ui.checkbox(&mut proxy, "Use a proxy");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn expander(&mut self, open: &mut bool, title: impl Into<String>, f: impl FnOnce(&mut Ui)) -> bool {
        let (col, _) = self.add(Kind::Column, Dir::Column);
        self.tree.layout[col].gap = self.px(4.0);
        let indent = self.px(24.0);
        let mut ui = self.child(col);
        let (hidx, hid) = ui.add(Kind::Expander { open: *open, text: title.into() }, Dir::Leaf);
        let flipped = !ui.disabled && ui.cx.clicked == Some(hid);
        if flipped {
            *open = !*open;
            if let Kind::Expander { open: o, .. } = &mut ui.tree.info[hidx].kind {
                *o = *open;
            }
        }
        if *open {
            let (body, _) = ui.add(Kind::Column, Dir::Column);
            ui.tree.layout[body].pad = (indent, 0.0);
            ui.tree.layout[body].gap = ui.px(6.0);
            f(&mut ui.child(body));
        }
        flipped
    }

    /// A text link. Ask [`Button::clicked`] whether it was clicked (to open a page or another
    /// screen; opening web pages is up to the app).
    ///
    /// ```
    /// # let mut help = false;
    /// # kargui::test::example(|ui| {
    /// if ui.link("How do backups work?").clicked() {
    ///     help = true;
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn link(&mut self, text: impl Into<String>) -> Button<'_> {
        let (idx, id) = self.add(Kind::Link { text: text.into() }, Dir::Leaf);
        let clicked = !self.disabled && self.cx.clicked == Some(id);
        Button { h: self.handle(idx, id), clicked }
    }

    /// A button that stays pressed while `*on` is true (for example Bold in a text editor).
    /// Clicking it flips `*on`.
    ///
    /// ```
    /// # let mut bold = false;
    /// # kargui::test::example(|ui| {
    /// ui.toggle_button(&mut bold, "Bold");
    /// # });
    /// ```
    #[track_caller]
    pub fn toggle_button(&mut self, on: &mut bool, text: impl Into<String>) -> Toggled<'_> {
        let (idx, id) = self.add(Kind::ToggleButton { on: *on, text: text.into(), icon: None }, Dir::Leaf);
        let changed = !self.disabled && self.cx.clicked == Some(id);
        if changed {
            *on = !*on;
            if let Kind::ToggleButton { on: o, .. } = &mut self.tree.info[idx].kind {
                *o = *on;
            }
        }
        Toggled { h: self.handle(idx, id), changed }
    }

    /// A one-line field for passwords: it shows dots instead of the text, does not copy or cut,
    /// and does not give the text to screen readers. Pasting works. Otherwise it is like
    /// [`Ui::text_input`].
    ///
    /// ```
    /// # let mut password = String::new();
    /// # kargui::test::example(|ui| {
    /// ui.password(&mut password).hint("Password").grow();
    /// # });
    /// ```
    #[track_caller]
    pub fn password(&mut self, text: &mut String) -> TextInput<'_> {
        let (idx, id) = self.add(Kind::TextInput { value: String::new(), hint: String::new(), password: true }, Dir::Leaf);
        let (changed, submitted) = self.edit(id, text, Field::Password);
        if let Kind::TextInput { value, .. } = &mut self.tree.info[idx].kind {
            *value = text.clone();
        }
        let composing = (self.cx.focus == Some(id) && !self.cx.preedit.is_empty()).then(|| self.cx.preedit.clone());
        TextInput { h: self.handle(idx, id), changed, submitted, composing }
    }

    /// A drop-down: a field showing the selected option; clicking it opens the list of options
    /// below (or above when there is no room). Returns a [`Dropdown`]; ask it whether the selection
    /// changed. With the focus on the closed field, Up and Down pick the previous or next option
    /// directly, and Enter, Space, Alt+Down or F4 open the list. In the open list, Up, Down, Home
    /// and End move, Enter or Space picks, and Escape or a click outside closes without picking.
    /// Give it a name with `.label(..)` for screen readers (they also hear the selected option).
    ///
    /// ```
    /// #[derive(Clone, Copy, PartialEq)]
    /// enum Theme { System, Light, Dark }
    /// # let mut theme = Theme::System;
    /// # kargui::test::example(|ui| {
    /// let options = [(Theme::System, "Use the system setting"), (Theme::Light, "Light"), (Theme::Dark, "Dark")];
    /// if ui.dropdown(&mut theme, &options).label("Theme").changed() {
    ///     // apply the theme
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn dropdown<T: Copy + PartialEq>(&mut self, value: &mut T, options: &[(T, &str)]) -> Dropdown<'_> {
        let current = options.iter().position(|(v, _)| v == value);
        let text = current.map_or("", |k| options[k].1).to_string();
        // 幅は一番長い項目に合わせる（開いても閉じても幅が変わらない）
        let style = crate::text::TextStyle { size: self.px(TextKind::Body.size()), bold: false };
        let widest = options.iter().map(|(_, t)| *t).max_by(|a, b| self.cx.text.layout(a, style, None).width.total_cmp(&self.cx.text.layout(b, style, None).width)).unwrap_or("").to_string();
        let (idx, id) = self.add(Kind::Dropdown { text, widest, open: false }, Dir::Leaf);
        let pop = Id(hash_of((id.0, "dropdown")));
        let mut open = self.cx.open.contains(&id) && !self.cx.closing.contains(&pop);
        let mut picked = None;
        if self.disabled {
            open = false;
        } else {
            if self.cx.clicked == Some(id) {
                open = !open;
            }
            if self.cx.focus == Some(id) && !options.is_empty() {
                // 閉じたまま: 上下で選び直す。Alt+下と F4 で開く
                for e in std::mem::take(&mut self.cx.keys) {
                    if let Event::Key { key: key @ (Key::Up | Key::Down), mods } = e {
                        if !mods.alt {
                            let at = current.unwrap_or(0) as isize + if key == Key::Down { 1 } else { -1 };
                            picked = Some(at.clamp(0, options.len() as isize - 1) as usize);
                        }
                    }
                }
                if self.cx.app_keys.contains(&(Key::Down, Mods::ALT)) || self.cx.app_keys.contains(&(Key::F4, Mods::NONE)) {
                    open = true;
                }
                // 閉じたまま文字を打つと、その文字で始まる次の項目を選ぶ
                if !open {
                    let texts: Vec<&str> = options.iter().map(|(_, t)| *t).collect();
                    for c in take_letters(self.cx) {
                        let from = picked.or(current).unwrap_or(options.len() - 1);
                        if let Some(j) = next_starting(&texts, from, c) {
                            picked = Some(j);
                        }
                    }
                }
            }
        }
        if open {
            let o = self.push_overlay(pop, OverlayKind::ListBox, String::new(), Some(id));
            let pad = self.px(4.0);
            self.tree.layout[o].pad = (pad, pad);
            // 一覧の幅は、欄の幅以上（前のフレームの欄の幅）
            if let Some(w) = self.cx.prev.hits.iter().find(|h| h.id == id).map(|h| h.rect.w) {
                self.tree.layout[o].width = Size::Fixed(w);
            }
            let mut item_ids = Vec::with_capacity(options.len());
            {
                let mut ui = self.child(o);
                for (k, (_, label)) in options.iter().enumerate() {
                    ui.salt = hash_of(k);
                    let (_, iid) = ui.add(Kind::ListOption { text: label.to_string(), selected: Some(k) == current }, Dir::Leaf);
                    item_ids.push(iid);
                    if ui.cx.clicked == Some(iid) {
                        picked = Some(k);
                        open = false;
                    }
                }
            }
            // 開いた時は、選んでいる項目に焦点を置く
            self.cx.overlay_focus_hint.insert(pop, item_ids[current.unwrap_or(0).min(item_ids.len().saturating_sub(1))]);
            move_focus_with_arrows(self.cx, &item_ids);
            let named: Vec<(Id, String)> = item_ids.iter().zip(options).map(|(&i, (_, t))| (i, t.to_string())).collect();
            type_ahead(self.cx, &named);
        }
        if open {
            self.cx.open.insert(id);
        } else {
            self.cx.open.remove(&id);
        }
        let changed = picked.is_some_and(|k| Some(k) != current);
        if let Some(k) = picked {
            *value = options[k].0;
        }
        if let Kind::Dropdown { text, open: o, .. } = &mut self.tree.info[idx].kind {
            *o = open;
            if let Some(k) = picked {
                *text = options[k].1.to_string();
            }
        }
        Dropdown { h: self.handle(idx, id), changed }
    }

    /// A button that opens a menu below it (or above it when there is no room). Build the items in
    /// `f` with [`Menu::item`], [`Menu::check`], [`Menu::radio`], [`Menu::separator`] and
    /// [`Menu::submenu`]. Choosing an item closes the menu; Escape or a click outside closes it
    /// without choosing. Keyboard: Enter, Space or Down opens it at the first item and Up at the
    /// last. In the menu, Up, Down, Home and End move, a letter moves to the next item starting
    /// with it, Right opens a submenu and Left closes it, and Tab closes the menu. `f` also runs
    /// while the menu is closed, without creating anything, so that the items' shortcuts work.
    ///
    /// ```
    /// use kargui::{Key, Mods};
    /// # let (mut doc, mut wrap) = (String::from("text"), false);
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("Edit", |m| {
    ///     if m.item("Clear").shortcut(Key::L, Mods::CTRL).clicked() {
    ///         doc.clear();
    ///     }
    ///     m.separator();
    ///     m.check(&mut wrap, "Word wrap");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn menu_button(&mut self, text: impl Into<String>, f: impl FnOnce(&mut Menu)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::MenuButton { text: text.into(), open: false, icon: None }, Dir::Leaf);
        let pop = Id(hash_of((id.0, "menu")));
        let (open, last) = self.root_menu(id, pop);
        let out = self.menu_body(pop, Some(id), Place::Below, 0, open, false, f);
        let cx = &mut *self.cx;
        if out.done && cx.menus.first() == Some(&pop) {
            cx.menus_truncate(0);
        }
        if let (true, Some(&l)) = (last, out.items.last()) {
            cx.overlay_focus_hint.insert(pop, l);
        }
        let open = cx.menus.first() == Some(&pop);
        if let Kind::MenuButton { open: o, .. } = &mut self.tree.info[idx].kind {
            *o = open;
        }
        self.handle(idx, id)
    }

    /// A menu bar: a row of headings that each open a menu ([`MenuBar::menu`]). F10 moves the
    /// focus to the first heading (and back); Left and Right move between headings, also while a
    /// menu is open; Down, Enter or Space opens the menu. While one menu is open, pointing at
    /// another heading opens that one. After an item is chosen, the focus goes back to where it was
    /// before the menu bar. Tab stops once on the bar.
    ///
    /// ```
    /// use kargui::{Key, Mods};
    /// # let mut saved = 0;
    /// # kargui::test::example(|ui| {
    /// ui.menu_bar(|bar| {
    ///     bar.menu("File", |m| {
    ///         if m.item("Save").shortcut(Key::S, Mods::CTRL).clicked() {
    ///             saved += 1;
    ///         }
    ///     });
    ///     bar.menu("Help", |m| {
    ///         m.item("About");
    ///     });
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn menu_bar(&mut self, f: impl FnOnce(&mut MenuBar)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::MenuBar, Dir::Row);
        let gap = self.px(2.0);
        self.tree.layout[idx].gap = gap;
        let mut bar = MenuBar { ui: self.child(idx), items: Vec::new(), step: None };
        f(&mut bar);
        let MenuBar { items, step, .. } = bar;
        let cx = &mut *self.cx;
        // メニューの中の左右の矢印で、隣の見出しのメニューへ
        if let Some((k, s)) = step.filter(|_| !items.is_empty()) {
            let j = (k as i32 + s).rem_euclid(items.len() as i32) as usize;
            let pop = Id(hash_of((items[j].0, "menu")));
            cx.overlay_focus_hint.remove(&pop);
            cx.menus_open(0, pop);
        }
        // 見出しに焦点がある時（メニューは閉じている）: 左右で見出しを移る。Esc でメニューバーへ入る前の場所へ
        if let Some(at) = cx.focus.and_then(|f| items.iter().position(|&i| i == f)) {
            let n = items.len();
            let mut next = at;
            for e in std::mem::take(&mut cx.keys) {
                if let Event::Key { key, mods } = e {
                    if mods.ctrl || mods.alt {
                        continue;
                    }
                    next = match key {
                        Key::Right => (next + 1) % n,
                        Key::Left => (next + n - 1) % n,
                        Key::Home => 0,
                        Key::End => n - 1,
                        _ => next,
                    };
                }
            }
            if next != at {
                cx.focus = Some(items[next]);
                cx.focus_visible = true;
            }
            let esc = (Key::Escape, Mods::NONE);
            if cx.bar_return.is_some() && cx.app_keys.contains(&esc) {
                cx.app_keys.retain(|k| *k != esc);
                cx.focus = cx.bar_return.take();
            }
        }
        self.handle(idx, id)
    }

    /// 一番外のメニュー（メニューボタン・メニューバーの見出し）を開くか決めて、開いているメニューの列を合わせる。
    /// 出す元を押すと開閉し、焦点がある時は下の矢印で開く（上の矢印なら最後の項目から）。返すのは (開くか, 最後の項目に焦点を置くか)
    fn root_menu(&mut self, owner: Id, pop: Id) -> (bool, bool) {
        let cx = &mut *self.cx;
        let mine = cx.menus.first() == Some(&pop);
        if self.disabled {
            if mine {
                cx.menus_truncate(0);
            }
            return (false, false);
        }
        let mut open = mine && !cx.closing.contains(&pop);
        if cx.clicked == Some(owner) {
            open = !open;
        }
        let mut last = false;
        if cx.focus == Some(owner) {
            if take_key(cx, Key::Down) {
                open = true;
            }
            if take_key(cx, Key::Up) {
                open = true;
                last = !mine;
            }
        }
        if open && !mine {
            cx.overlay_focus_hint.remove(&pop);
            cx.menus_open(0, pop);
        } else if !open && mine {
            cx.menus_truncate(0);
        }
        (open, last)
    }

    /// メニューの中身を作る。開いていれば重ねる物を作って項目を並べ、閉じていれば項目を作らずに
    /// ショートカットだけを見る（f は毎回回す）
    #[allow(clippy::too_many_arguments)]
    fn menu_body(&mut self, pop: Id, anchor: Option<Id>, place: Place, depth: usize, open: bool, in_bar: bool, f: impl FnOnce(&mut Menu)) -> MenuOut {
        let parent = if open {
            let o = self.push_overlay(pop, OverlayKind::Menu, String::new(), anchor);
            if let Some(last) = self.tree.overlays.last_mut() {
                last.place = place;
            }
            let pad = self.px(4.0);
            self.tree.layout[o].pad = (pad, pad);
            o
        } else {
            self.parent
        };
        let mut m = Menu { ui: self.child(parent), open, depth, in_bar, done: false, step: 0 };
        f(&mut m);
        let (done, step) = (m.done, m.step);
        if !open {
            return MenuOut { done, step, items: Vec::new() };
        }
        let (items, keys_step) = self.finish_menu(parent, pop, depth, in_bar);
        MenuOut { done, step: if keys_step != 0 { keys_step } else { step }, items }
    }

    /// 開いたメニューの仕上げ: 印かアイコンの列をそろえ、焦点が中にあれば矢印・Home・End・文字で項目を移る。
    /// 入れ子のメニューは左の矢印で閉じる。メニューバーのメニューは左右で隣へ（返す向き）
    fn finish_menu(&mut self, o: usize, pop: Id, depth: usize, in_bar: bool) -> (Vec<Id>, i32) {
        let kids = self.tree.layout[o].children.clone();
        let gutter = kids
            .iter()
            .any(|&c| matches!(&self.tree.info[c].kind, Kind::MenuItem { mark, icon, .. } if *mark != MenuMark::None || icon.is_some()));
        let mut items = Vec::new();
        for &c in &kids {
            let n = &mut self.tree.info[c];
            if let Kind::MenuItem { gutter: g, text, .. } = &mut n.kind {
                *g = gutter;
                if !n.disabled {
                    items.push((n.id, text.clone()));
                }
            }
        }
        let mut step = 0;
        let cx = &mut *self.cx;
        if let Some(at) = cx.focus.and_then(|f| items.iter().position(|(i, _)| *i == f)) {
            let n = items.len();
            let mut next = at;
            for e in std::mem::take(&mut cx.keys) {
                let Event::Key { key, mods } = e else { continue };
                if mods.ctrl || mods.alt {
                    continue;
                }
                match key {
                    Key::Down => next = (next + 1) % n,
                    Key::Up => next = (next + n - 1) % n,
                    Key::Home => next = 0,
                    Key::End => next = n - 1,
                    // 入れ子のメニューは左で閉じる（焦点は開いた項目へ戻る）
                    Key::Left if depth > 0 => {
                        if cx.menus.get(depth) == Some(&pop) {
                            cx.menus_truncate(depth);
                        }
                    }
                    Key::Left if in_bar => step = -1,
                    Key::Right if in_bar => step = 1,
                    _ => {}
                }
            }
            if next != at {
                cx.focus = Some(items[next].0);
                cx.focus_visible = true;
            }
            type_ahead(cx, &items);
        }
        (items.into_iter().map(|(i, _)| i).collect(), step)
    }

    /// Tabs: a row of tab headings over a panel that shows the selected tab's content. `panel`
    /// gets the selected value and builds the content for it. Clicking a heading selects it.
    /// Keyboard: Tab stops once on the selected heading; Left and Right select the previous or next
    /// tab (wrapping), Home and End the first or last. While the focus is anywhere inside,
    /// Ctrl+Tab and Ctrl+PageDown select the next tab, Ctrl+Shift+Tab and Ctrl+PageUp the previous.
    /// Screen readers hear a tab list, tabs (and which is selected) and a tab panel named after it.
    ///
    /// ```
    /// #[derive(Clone, Copy, PartialEq)]
    /// enum Page { General, Advanced }
    /// # let (mut page, mut sound, mut proxy) = (Page::General, true, false);
    /// # kargui::test::example(|ui| {
    /// ui.tabs(&mut page, &[(Page::General, "General"), (Page::Advanced, "Advanced")], |ui, page| match page {
    ///     Page::General => {
    ///         ui.checkbox(&mut sound, "Play a sound");
    ///     }
    ///     Page::Advanced => {
    ///         ui.checkbox(&mut proxy, "Use a proxy");
    ///     }
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn tabs<T: Copy + PartialEq>(&mut self, value: &mut T, tabs: &[(T, &str)], panel: impl FnOnce(&mut Ui, T)) -> Tabs<'_> {
        let (idx, id) = self.add(Kind::Tabs, Dir::Column);
        self.tree.layout[idx].align = Align::Stretch;
        let current = tabs.iter().position(|(v, _)| v == value);
        let mut picked = None;
        let mut heads = Vec::with_capacity(tabs.len());
        {
            let mut ui = self.child(idx);
            let (list, _) = ui.add(Kind::TabList, Dir::Row);
            let gap = ui.px(4.0);
            ui.tree.layout[list].gap = gap;
            let mut row = ui.child(list);
            for (k, (_, label)) in tabs.iter().enumerate() {
                row.salt = hash_of(k);
                let (ti, tid) = row.add(Kind::Tab { text: label.to_string(), selected: Some(k) == current }, Dir::Leaf);
                // Tab は選んでいるタブにだけ止まる（選んでいる物が無ければ最初）
                row.tree.info[ti].skip_tab = Some(k) != current.or(Some(0));
                heads.push(tid);
                if !row.disabled && row.cx.clicked == Some(tid) {
                    picked = Some(k);
                }
            }
        }
        // 中身（選んでいるタブの名前で読み上げる）
        if let Some(k) = current {
            let mut ui = self.child(idx);
            let (p, _) = ui.add(Kind::TabPanel, Dir::Column);
            ui.tree.info[p].label = Some(tabs[k].1.to_string());
            let (pad, gap) = (ui.px(12.0), ui.px(10.0));
            ui.tree.layout[p].pad = (0.0, pad);
            ui.tree.layout[p].gap = gap;
            ui.tree.layout[p].align = Align::Stretch;
            panel(&mut ui.child(p), tabs[k].0);
        }
        let n = heads.len();
        let cx = &mut *self.cx;
        if n > 0 && !self.disabled {
            // 見出しに焦点がある時: 左右・Home・End で選ぶ（端で回る）
            if let Some(at) = cx.focus.and_then(|f| heads.iter().position(|&h| h == f)) {
                let mut next = at;
                for e in std::mem::take(&mut cx.keys) {
                    if let Event::Key { key, mods } = e {
                        if mods.ctrl || mods.alt {
                            continue;
                        }
                        next = match key {
                            Key::Right => (next + 1) % n,
                            Key::Left => (next + n - 1) % n,
                            Key::Home => 0,
                            Key::End => n - 1,
                            _ => next,
                        };
                    }
                }
                if next != at {
                    picked = Some(next);
                    cx.focus = Some(heads[next]);
                    cx.focus_visible = true;
                }
            }
            // 中に焦点がある時: Ctrl+Tab・Ctrl+PageDown で次、Ctrl+Shift+Tab・Ctrl+PageUp で前
            let within = cx.focus.is_some_and(|f| descends(self.tree, idx, f));
            if within {
                let from = picked.or(current).unwrap_or(0);
                let mut step = 0i32;
                cx.app_keys.retain(|&(k, m)| {
                    let s = match (k, m) {
                        (Key::Tab, Mods { ctrl: true, alt: false, shift: false }) | (Key::PageDown, Mods { ctrl: true, alt: false, shift: false }) => 1,
                        (Key::Tab, Mods { ctrl: true, alt: false, shift: true }) | (Key::PageUp, Mods { ctrl: true, alt: false, shift: false }) => -1,
                        _ => return true,
                    };
                    step += s;
                    false
                });
                if step != 0 {
                    let next = (from as i32 + step).rem_euclid(n as i32) as usize;
                    picked = Some(next);
                    // 中身の部品は消えるので、焦点は新しいタブの見出しへ
                    if !heads.contains(&cx.focus.unwrap_or(Id(0))) {
                        cx.focus = Some(heads[next]);
                        cx.focus_visible = true;
                    }
                }
            }
        }
        let changed = picked.is_some_and(|k| Some(k) != current);
        if let Some(k) = picked {
            *value = tabs[k].0;
        }
        Tabs { h: self.handle(idx, id), changed }
    }

    /// A field for a number between the ends of `range`, with small buttons to step it up and
    /// down. Typing changes the number when Enter is pressed or the focus leaves (text that is not
    /// a number is undone; numbers outside the range are moved to the nearest end; full-width
    /// digits typed with a Japanese input method also work). Up and Down step by
    /// [`NumberInput::step`] (1 by default), Page Up and Page Down by 10 steps.
    /// Screen readers hear a spin button with the value and the range.
    ///
    /// ```
    /// # let (mut copies, mut zoom) = (1, 1.0);
    /// # kargui::test::example(|ui| {
    /// ui.number(&mut copies, 1..=99).label("Copies");
    /// ui.number(&mut zoom, 0.5..=4.0).step(0.25).label("Zoom");
    /// # });
    /// ```
    #[track_caller]
    pub fn number<T: SliderValue>(&mut self, value: &mut T, range: RangeInclusive<T>) -> NumberInput<'_> {
        let (min, max) = (range.start().to_f64(), range.end().to_f64());
        let (idx, id) = self.add(Kind::NumberInput { text: String::new(), value: 0.0, min, max, step: 1.0 }, Dir::Row);
        // 幅 140・高さは 1 行の入力欄と同じ 34（論理ピクセル）
        let (w, h, pad) = (self.px(140.0), self.px(34.0), self.px(3.0));
        {
            let l = &mut self.tree.layout[idx];
            l.width = Size::Fixed(w);
            l.height = Size::Fixed(h);
            l.pad = (pad, pad);
            l.align = Align::Stretch;
        }
        // 右の小さいボタン（読み上げの木には出さない。押しても焦点は欄に残る）
        let (up, down) = {
            let bw = self.px(28.0);
            let mut ui = self.child(idx);
            ui.fill();
            let (u, uid) = ui.add(Kind::SpinButton { up: true }, Dir::Leaf);
            let (d, did) = ui.add(Kind::SpinButton { up: false }, Dir::Leaf);
            for (k, name) in [(u, "Increase"), (d, "Decrease")] {
                ui.tree.layout[k].width = Size::Fixed(bw);
                ui.tree.info[k].label = Some(name.to_string());
            }
            (uid, did)
        };
        let step = self.cx.prev.steps.get(&id).copied().unwrap_or(1.0);
        let fmt = |v: f64| format_number(v, T::INTEGER, step);
        let fit = |v: f64| {
            let v = v.clamp(min.min(max), max.max(min));
            if T::INTEGER { v.round() } else { round_to_step_decimals(v, step) }
        };
        let mut v = value.to_f64();
        let start = v;
        let focused = self.cx.focus == Some(id) && !self.disabled;
        // 打っている途中の文字は、焦点がある間だけ kargui が持つ。焦点が外れたら決める
        let editing = self.cx.number_text.remove(&id);
        let mut text = match (focused, editing) {
            (false, Some(t)) => {
                if let Some(n) = parse_number(&t) {
                    v = fit(n);
                }
                fmt(v)
            }
            (true, Some(t)) => t,
            (_, None) => fmt(v),
        };
        if !self.disabled {
            let mut steps = 0.0;
            if self.cx.clicked == Some(up) {
                steps += 1.0;
            }
            if self.cx.clicked == Some(down) {
                steps -= 1.0;
            }
            if focused {
                self.cx.keys.retain(|e| match e {
                    Event::Key { key, mods } if !mods.ctrl && !mods.alt => {
                        let s = match key {
                            Key::Up => 1.0,
                            Key::Down => -1.0,
                            Key::PageUp => 10.0,
                            Key::PageDown => -10.0,
                            _ => return true,
                        };
                        steps += s;
                        false
                    }
                    _ => true,
                });
            }
            if steps != 0.0 {
                // 打っていた数を先に決めてから進める
                if let Some(n) = parse_number(&text) {
                    v = fit(n);
                }
                v = fit(v + steps * step);
                text = fmt(v);
                if let Some(e) = self.cx.edits.get_mut(&id) {
                    e.cursor = text.len();
                    e.anchor = text.len();
                }
            }
            if focused {
                let (_, submitted) = self.edit(id, &mut text, Field::Line);
                if submitted {
                    if let Some(n) = parse_number(&text) {
                        v = fit(n);
                    }
                    text = fmt(v);
                }
                self.cx.number_text.insert(id, text.clone());
            }
        }
        let changed = v != start;
        if changed {
            *value = T::from_f64(v);
            crate::log::event("number.change", &format!("from={start} to={v}"), "ok");
        }
        if let Kind::NumberInput { text: t, value: val, .. } = &mut self.tree.info[idx].kind {
            *t = text;
            *val = v;
        }
        NumberInput { h: self.handle(idx, id), changed }
    }

    /// A toolbar: a row of icon buttons and other commands. Its buttons show their names as
    /// tooltips. Tab stops once on the toolbar (on the button used last), and Left, Right, Home
    /// and End move between its buttons. Give it a name with `.label(..)`.
    ///
    /// ```
    /// use kargui::Icon;
    /// # let (mut count, mut pinned) = (0, false);
    /// # kargui::test::example(|ui| {
    /// ui.toolbar(|tb| {
    ///     if tb.button(Icon::Plus, "Add").clicked() {
    ///         count += 1;
    ///     }
    ///     tb.toggle(&mut pinned, Icon::Check, "Pin");
    ///     tb.separator();
    ///     tb.more(|m| {
    ///         m.item("Settings");
    ///     });
    /// })
    /// .label("Tasks");
    /// # });
    /// ```
    #[track_caller]
    pub fn toolbar(&mut self, f: impl FnOnce(&mut Toolbar)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Toolbar, Dir::Row);
        let gap = self.px(2.0);
        self.tree.layout[idx].gap = gap;
        f(&mut Toolbar { ui: self.child(idx) });
        // 移れるボタン（使えない物は飛ばす）
        let items: Vec<(usize, Id)> = self.tree.layout[idx]
            .children
            .iter()
            .copied()
            .filter(|&c| !self.tree.info[c].disabled && matches!(self.tree.info[c].kind, Kind::Button { .. } | Kind::ToggleButton { .. } | Kind::MenuButton { .. }))
            .map(|c| (c, self.tree.info[c].id))
            .collect();
        let cx = &mut *self.cx;
        if let Some(at) = cx.focus.and_then(|f| items.iter().position(|&(_, i)| i == f)) {
            let n = items.len();
            let mut next = at;
            for e in std::mem::take(&mut cx.keys) {
                if let Event::Key { key, mods } = e {
                    if mods.ctrl || mods.alt {
                        continue;
                    }
                    next = match key {
                        Key::Right => (next + 1) % n,
                        Key::Left => (next + n - 1) % n,
                        Key::Home => 0,
                        Key::End => n - 1,
                        _ => next,
                    };
                }
            }
            if next != at {
                cx.focus = Some(items[next].1);
                cx.focus_visible = true;
            }
        }
        // 最後にいたボタンを覚えて、Tab はそこにだけ止まる
        if let Some(f) = cx.focus.filter(|f| items.iter().any(|&(_, i)| i == *f)) {
            cx.toolbar_last.insert(id, f);
        }
        let stop = cx.toolbar_last.get(&id).copied().filter(|s| items.iter().any(|&(_, i)| i == *s)).or(items.first().map(|&(_, i)| i));
        for &(c, i) in &items {
            self.tree.info[c].skip_tab = Some(i) != stop;
        }
        self.handle(idx, id)
    }

    /// A grid: the widgets created in `f` go into cells left to right, then top to bottom, wrapping
    /// after `columns.len()` cells. Each column is as wide as its widest cell ([`Size::Auto`]), a
    /// fixed width, or a share of the remaining width ([`Size::Fill`]). Each row is as tall as its
    /// tallest cell, and cells are centered vertically in their row. Good for forms.
    ///
    /// ```
    /// use kargui::Size;
    /// # let (mut name, mut email) = (String::new(), String::new());
    /// # kargui::test::example(|ui| {
    /// ui.grid(&[Size::Auto, Size::Fill(1.0)], |ui| {
    ///     ui.label("Name");
    ///     ui.text_input(&mut name).label("Name");
    ///     ui.label("E-mail address");
    ///     ui.text_input(&mut email).label("E-mail address");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn grid(&mut self, columns: &[Size], f: impl FnOnce(&mut Ui)) -> Handle<'_> {
        let (idx, id) = self.add(Kind::Grid, Dir::Grid);
        let scale = self.cx.scale;
        let gap = self.px(10.0);
        let l = &mut self.tree.layout[idx];
        l.columns = columns.iter().map(|&c| if let Size::Fixed(v) = c { Size::Fixed((v * scale).round()) } else { c }).collect();
        l.gap = gap;
        l.align = Align::Stretch;
        f(&mut self.child(idx));
        self.handle(idx, id)
    }

    /// Two panes side by side with a divider between them that can be dragged, or moved with the
    /// keyboard (Tab stops on it; Left and Right move it by 10, Home and End to the ends).
    /// `*left` is the left pane's width in logical pixels; each pane keeps at least 48. `f` is
    /// called for each pane with its number (0 for the left, 1 for the right). The divider's
    /// screen-reader name is "Resize" until you give it one with `.label(..)`.
    ///
    /// ```
    /// # let mut left = 220.0;
    /// # kargui::test::example(|ui| {
    /// ui.split_left_right(&mut left, |ui, pane| {
    ///     if pane == 0 {
    ///         ui.label("Folders");
    ///     } else {
    ///         ui.label("Files");
    ///     }
    /// })
    /// .label("Resize the folder list")
    /// .grow();
    /// # });
    /// ```
    #[track_caller]
    pub fn split_left_right(&mut self, left: &mut f32, f: impl FnMut(&mut Ui, usize)) -> Split<'_> {
        self.split(left, false, f)
    }

    /// Two panes, one above the other, with a divider between them. Like
    /// [`Ui::split_left_right`], but `*top` is the top pane's height, and Up and Down move the
    /// divider. Pane 0 is the top one.
    ///
    /// ```
    /// # let mut top = 160.0;
    /// # kargui::test::example(|ui| {
    /// ui.split_top_bottom(&mut top, |ui, pane| {
    ///     ui.label(if pane == 0 { "Messages" } else { "Preview" });
    /// })
    /// .grow();
    /// # });
    /// ```
    #[track_caller]
    pub fn split_top_bottom(&mut self, top: &mut f32, f: impl FnMut(&mut Ui, usize)) -> Split<'_> {
        self.split(top, true, f)
    }

    #[track_caller]
    fn split(&mut self, first: &mut f32, stacked: bool, mut f: impl FnMut(&mut Ui, usize)) -> Split<'_> {
        /// どちらの区画も、これより狭くしない（論理ピクセル）
        const MIN: f32 = 48.0;
        let (idx, id) = self.add(Kind::Split { stacked }, if stacked { Dir::Column } else { Dir::Row });
        self.tree.layout[idx].align = Align::Stretch;
        let (p0, bar, bar_id, p1) = {
            let mut ui = self.child(idx);
            let (p0, _) = ui.add(Kind::Column, Dir::Column);
            let (bar, bar_id) = ui.add(Kind::Splitter { stacked, value: 0.0, min: MIN, max: MIN }, Dir::Leaf);
            let (p1, _) = ui.add(Kind::Column, Dir::Column);
            (p0, bar, bar_id, p1)
        };
        self.tree.info[bar].label = Some("Resize".to_string());
        let scale = self.cx.scale;
        // 仕切りの幅（つかめる幅。線は真ん中に 1 本）
        let bar_len = self.px(12.0);
        // 前のフレームの、仕切りの入れ物の始まりと長さ（物理ピクセル）
        let total = self.cx.prev.splits.get(&bar_id).copied();
        let max = total.map_or(f32::MAX, |(_, len)| ((len - bar_len) / scale - MIN).max(MIN));
        let mut v = *first;
        if !self.disabled {
            let cx = &mut *self.cx;
            // つかんで動かす: 仕切りの真ん中がポインターの下に来るように
            if cx.pressed == Some(bar_id) || cx.released == Some(bar_id) {
                if let (Some((px, py)), Some((start, _))) = (cx.pointer, total) {
                    let pos = if stacked { py } else { px };
                    v = (pos - start - bar_len / 2.0) / scale;
                }
            }
            if cx.focus == Some(bar_id) {
                for e in std::mem::take(&mut cx.keys) {
                    if let Event::Key { key, mods } = e {
                        if mods.ctrl || mods.alt {
                            continue;
                        }
                        match key {
                            Key::Left | Key::Up => v -= 10.0,
                            Key::Right | Key::Down => v += 10.0,
                            Key::Home => v = MIN,
                            Key::End => v = max,
                            _ => {}
                        }
                    }
                }
            }
        }
        let v = v.clamp(MIN, max).round();
        let changed = v != *first;
        if changed {
            crate::log::event("split.resize", &format!("from={} to={v}", *first), "ok");
        }
        *first = v;
        let gap = self.px(10.0);
        {
            let lay = &mut self.tree.layout;
            let (a, b) = (Size::Fixed((v * scale).round()), Size::Fixed(bar_len));
            if stacked {
                (lay[p0].height, lay[bar].height, lay[p1].height) = (a, b, Size::Fill(1.0));
            } else {
                (lay[p0].width, lay[bar].width, lay[p1].width) = (a, b, Size::Fill(1.0));
            }
            lay[p0].gap = gap;
            lay[p1].gap = gap;
        }
        if let Kind::Splitter { value, max: m, .. } = &mut self.tree.info[bar].kind {
            *value = v;
            *m = if max == f32::MAX { v } else { max };
        }
        f(&mut self.child(p0), 0);
        f(&mut self.child(p1), 1);
        Split { h: self.handle(idx, id), bar, changed }
    }

    /// A surface of `width` × `height` logical pixels to draw on with the [`Drawing`] given to `f`
    /// (rectangles, circles, lines, polygons, text and images, in the surface's own coordinates
    /// from its top left corner). Ask the returned [`CanvasView`] where the pointer is while the
    /// surface is pressed, and where it was clicked. Name it with `.label(..)` for screen readers.
    ///
    /// ```
    /// use kargui::rgb;
    /// # let mut dots: Vec<(f32, f32)> = Vec::new();
    /// # kargui::test::example(|ui| {
    /// let canvas = ui.canvas(240.0, 160.0, |d| {
    ///     for &(x, y) in &dots {
    ///         d.circle((x, y), 4.0, rgb(0x2060c0));
    ///     }
    /// });
    /// if let Some(at) = canvas.label("Dots").clicked_at() {
    ///     dots.push(at);
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn canvas(&mut self, width: f32, height: f32, f: impl FnOnce(&mut Drawing)) -> CanvasView<'_> {
        let mut d = Drawing { ops: Vec::new() };
        f(&mut d);
        let (idx, id) = self.add(Kind::Canvas { ops: d.ops }, Dir::Leaf);
        let (w, h) = (self.px(width), self.px(height));
        let l = &mut self.tree.layout[idx];
        (l.width, l.height) = (Size::Fixed(w), Size::Fixed(h));
        // ポインターの位置を、面の左上からの論理ピクセルにする（前のフレームの場所を使う）
        let scale = self.cx.scale;
        let origin = self.cx.prev.canvases.get(&id).map(|r| (r.x, r.y));
        let local = |p: (f32, f32)| origin.map(|(ox, oy)| ((p.0 - ox) / scale, (p.1 - oy) / scale));
        let cx = &*self.cx;
        let on = !self.disabled;
        let pointer = if on && (cx.pressed == Some(id) || cx.released == Some(id)) { cx.pointer.and_then(local) } else { None };
        let clicked_at = if on && cx.clicked == Some(id) { cx.pointer.and_then(local) } else { None };
        CanvasView { h: self.handle(idx, id), pointer, clicked_at }
    }

    /// A tree of items that open and close, such as folders or an outline. Build it in `f` with
    /// [`TreeBuilder::branch`] (an item with children; closed at first) and [`TreeBuilder::leaf`].
    /// Items are told apart by their keys. `*selected` is the selected item's key. Clicking an
    /// item selects it, and clicking the arrow at its left opens or closes it. Keyboard: Tab stops
    /// once (on the selected item); Up, Down, Home, End and a letter move and select; Right opens
    /// an item or moves to its first child; Left closes it or moves to its parent; Enter and Space
    /// open and close. Children of closed items are not created, so large trees stay light.
    ///
    /// ```
    /// # let mut selected = None;
    /// # kargui::test::example(|ui| {
    /// ui.tree(&mut selected, |t| {
    ///     t.branch("docs", "Documents", |t| {
    ///         t.leaf("report", "report.pdf");
    ///     });
    ///     t.leaf("notes", "notes.txt");
    /// })
    /// .label("Files");
    /// # });
    /// ```
    #[track_caller]
    pub fn tree<K: Copy + PartialEq + Hash>(&mut self, selected: &mut Option<K>, f: impl FnOnce(&mut TreeBuilder<K>)) -> TreeView<'_> {
        let (idx, id) = self.add(Kind::Tree, Dir::Column);
        self.tree.layout[idx].align = Align::Stretch;
        let mut b = TreeBuilder { ui: self.child(idx), tree: id, depth: 0, parent: None, rows: Vec::new(), selected: *selected, picked: None };
        f(&mut b);
        let TreeBuilder { rows, picked, .. } = b;
        let mut sel = picked.or(*selected);
        let n = rows.len();
        let cx = &mut *self.cx;
        let at = cx.focus.filter(|_| !self.disabled).and_then(|f| rows.iter().position(|r| r.id == f));
        if let (Some(at), true) = (at, n > 0) {
            let mut next = at;
            for e in std::mem::take(&mut cx.keys) {
                let Event::Key { key, mods } = e else { continue };
                if mods.ctrl || mods.alt {
                    continue;
                }
                match key {
                    Key::Down => next = (next + 1).min(n - 1),
                    Key::Up => next = next.saturating_sub(1),
                    Key::Home => next = 0,
                    Key::End => next = n - 1,
                    // 右: 閉じた枝は開く。開いた枝は最初の子へ
                    Key::Right => match rows[next].open {
                        Some(false) => {
                            cx.expanded.insert(rows[next].id, true);
                        }
                        Some(true) if next + 1 < n && rows[next + 1].parent == Some(next) => next += 1,
                        _ => {}
                    },
                    // 左: 開いた枝は閉じる。ほかは親へ
                    Key::Left => match (rows[next].open, rows[next].parent) {
                        (Some(true), _) => {
                            cx.expanded.insert(rows[next].id, false);
                        }
                        (_, Some(p)) => next = p,
                        _ => {}
                    },
                    _ => {}
                }
            }
            let items: Vec<(Id, String)> = rows.iter().map(|r| (r.id, r.text.clone())).collect();
            if next != at {
                cx.focus = Some(rows[next].id);
                cx.focus_visible = true;
            }
            type_ahead(cx, &items);
            if let Some(k) = cx.focus.and_then(|f| rows.iter().position(|r| r.id == f)).filter(|&k| k != at) {
                sel = Some(rows[k].key);
            }
        }
        // Tab は選んでいる行（無ければ最初の行）にだけ止まる
        let stop = rows.iter().position(|r| Some(r.key) == sel).unwrap_or(0);
        for (k, r) in rows.iter().enumerate() {
            self.tree.info[r.idx].skip_tab = k != stop;
            if let Kind::TreeItem { selected, .. } = &mut self.tree.info[r.idx].kind {
                *selected = Some(r.key) == sel;
            }
        }
        let changed = sel != *selected;
        *selected = sel;
        TreeView { h: self.handle(idx, id), changed }
    }

    /// A table: column headings over a virtual list of `count` rows, `row_height` logical pixels
    /// tall, whose cells line up with the headings. Set it up with [`Table::sort`],
    /// [`Table::selection`], [`Table::label`] and [`Table::grow`], then build the rows with
    /// [`Table::show`]. Drag the small grip at the right of a heading to change a column's width
    /// (kargui remembers it). Without `.grow()` the table shows up to 8 rows and scrolls.
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # struct File { name: String, size: u64 }
    /// # let files = vec![File { name: "a.txt".into(), size: 3 }];
    /// # let mut sort = None;
    /// # kargui::test::example(|ui| {
    /// let columns = [TableColumn::new("Name").width(220.0), TableColumn::new("Size").width(90.0).right()];
    /// ui.table(&columns, files.len(), 30.0).sort(&mut sort).label("Files").grow().show(|row, i| {
    ///     row.text(&files[i].name);
    ///     row.text(format!("{} KB", files[i].size));
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn table<'u>(&'u mut self, columns: &'u [TableColumn], count: usize, row_height: f32) -> Table<'u, 'a> {
        let loc = std::panic::Location::caller();
        Table { ui: self, loc, columns, count, row_h: row_height, sort: None, selection: None, label: None, grow: false }
    }

    /// A field for a date that opens a month calendar below it when clicked (or with Enter, Space,
    /// Alt+Down or F4). `*value` is `None` until a date is picked. In the calendar, the arrow keys
    /// move by a day or a week, Home and End to the start or end of the week, Page Up and Page
    /// Down by a month (with Shift, by a year), Enter or Space picks, and Escape or a click outside
    /// closes without picking. The field shows the date as `2026-11-22`; the names of days and
    /// months come from [`Ui::set_date_text`]. Screen readers hear a date input with its value,
    /// and a grid of days, each with its full name.
    ///
    /// ```
    /// # let mut due: Option<kargui::Date> = None;
    /// # kargui::test::example(|ui| {
    /// if ui.date_picker(&mut due).label("Due date").changed() {
    ///     // save the new date
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn date_picker(&mut self, value: &mut Option<Date>) -> DatePicker<'_> {
        let words = self.cx.date_text.clone();
        let text = value.map_or_else(|| words.empty.to_string(), |d| d.to_string());
        let (idx, id) = self.add(Kind::DateField { text, empty: value.is_none(), open: false }, Dir::Leaf);
        let pop = Id(hash_of((id.0, "calendar")));
        let today = self.today();
        let cell = |d: Date| Id(hash_of((pop.0, d.days())));
        let mut open = self.cx.open.contains(&id) && !self.cx.closing.contains(&pop);
        let mut picked = None;
        if self.disabled {
            open = false;
        } else {
            if self.cx.clicked == Some(id) {
                open = !open;
            }
            if self.cx.focus == Some(id) && (self.cx.app_keys.contains(&(Key::Down, Mods::ALT)) || self.cx.app_keys.contains(&(Key::F4, Mods::NONE))) {
                open = true;
            }
        }
        if open {
            let mut cursor = *self.cx.calendar_cursor.entry(id).or_insert(value.unwrap_or(today));
            // 日に焦点がある時のキー（作る前に動かし、動かした先の月を作る）
            if self.cx.focus == Some(cell(cursor)) {
                let cx = &mut *self.cx;
                let mut moved = false;
                for e in std::mem::take(&mut cx.keys) {
                    let Event::Key { key, mods } = e else { continue };
                    if mods.ctrl || mods.alt {
                        continue;
                    }
                    let wd = cursor.weekday() as i64;
                    cursor = match key {
                        Key::Left => cursor.add_days(-1),
                        Key::Right => cursor.add_days(1),
                        Key::Up => cursor.add_days(-7),
                        Key::Down => cursor.add_days(7),
                        Key::Home => cursor.add_days(-wd),
                        Key::End => cursor.add_days(6 - wd),
                        _ => continue,
                    };
                    moved = true;
                }
                // PageUp と PageDown は、部品が受けないキーとしてアプリへ届く
                cx.app_keys.retain(|&(k, m)| {
                    let months = match (k, m) {
                        (Key::PageUp, Mods { shift: false, ctrl: false, alt: false }) => -1,
                        (Key::PageDown, Mods { shift: false, ctrl: false, alt: false }) => 1,
                        (Key::PageUp, Mods { shift: true, ctrl: false, alt: false }) => -12,
                        (Key::PageDown, Mods { shift: true, ctrl: false, alt: false }) => 12,
                        _ => return true,
                    };
                    cursor = cursor.add_months(months);
                    moved = true;
                    false
                });
                if moved {
                    cx.focus = Some(cell(cursor));
                    cx.focus_visible = true;
                }
            }
            let o = self.push_overlay(pop, OverlayKind::Popup, String::new(), Some(id));
            let (pad, gap) = (self.px(8.0), self.px(6.0));
            self.tree.layout[o].pad = (pad, pad);
            self.tree.layout[o].gap = gap;
            let cw = self.px(36.0);
            {
                let mut ui = self.child(o);
                // 月の題と、前・次の月のボタン
                let mut step = 0;
                ui.row(|ui| {
                    if ui.icon_button(Icon::ChevronLeft, words.previous).clicked() {
                        step = -1;
                    }
                    ui.fill();
                    ui.label(words.title(cursor.year(), cursor.month())).bold();
                    ui.fill();
                    if ui.icon_button(Icon::ChevronRight, words.next).clicked() {
                        step = 1;
                    }
                });
                if step != 0 {
                    cursor = cursor.add_months(step);
                }
                let columns = [Size::Fixed(36.0); 7];
                // 曜日の名前（日のます目と同じ列の幅）
                ui.grid(&columns, |ui| {
                    for name in words.weekdays {
                        ui.label(name).weak().small();
                    }
                })
                .gap(0.0)
                .align(Align::Center);
                // 日のます目: 月の 1 日を含む週の日曜日から 6 週
                let first = Date::new(cursor.year(), cursor.month(), 1).unwrap_or(cursor);
                let start = first.add_days(-(first.weekday() as i64));
                let (g, _) = ui.add(Kind::DayGrid, Dir::Grid);
                ui.tree.info[g].label = Some(words.title(cursor.year(), cursor.month()));
                let l = &mut ui.tree.layout[g];
                l.columns = vec![Size::Fixed(cw); 7];
                l.align = Align::Center;
                let mut days = ui.child(g);
                for k in 0..42 {
                    let d = start.add_days(k);
                    let kind = Kind::CalendarDay { day: d.day(), in_month: d.month() == cursor.month(), today: d == today, selected: Some(d) == *value };
                    let (c, cid) = days.push(cell(d), kind, Dir::Leaf);
                    days.tree.info[c].label = Some(words.full(d));
                    days.tree.info[c].skip_tab = d != cursor;
                    if !days.disabled && days.cx.clicked == Some(cid) {
                        picked = Some(d);
                    }
                }
            }
            self.cx.calendar_cursor.insert(id, cursor);
            // 開いた時は、焦点を今の日に置く
            self.cx.overlay_focus_hint.insert(pop, cell(cursor));
        }
        if let Some(d) = picked {
            open = false;
            crate::log::event("date.pick", &format!("date={d}"), "ok");
        }
        let changed = picked.is_some_and(|d| Some(d) != *value);
        if let Some(d) = picked {
            *value = Some(d);
        }
        if open {
            self.cx.open.insert(id);
        } else {
            self.cx.open.remove(&id);
            self.cx.calendar_cursor.remove(&id);
        }
        if let Kind::DateField { text, empty, open: o } = &mut self.tree.info[idx].kind {
            *o = open;
            *empty = value.is_none();
            *text = value.map_or_else(|| words.empty.to_string(), |d| d.to_string());
        }
        DatePicker { h: self.handle(idx, id), changed }
    }

    /// Sets the words date pickers show (names of days and months, the text of an empty field)
    /// for this frame and the following ones. English by default.
    ///
    /// ```
    /// # let mut due = None;
    /// # kargui::test::example(|ui| {
    /// ui.set_date_text(kargui::DateText::japanese());
    /// ui.date_picker(&mut due).label("期限");
    /// # });
    /// ```
    pub fn set_date_text(&mut self, text: DateText) {
        self.cx.date_text = text;
    }

    /// Today's date (in tests, always 2026-11-22, so pictures and results do not change from day
    /// to day).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let today = ui.today();
    /// ui.label(format!("Today is {today}"));
    /// # });
    /// ```
    pub fn today(&self) -> Date {
        self.cx.today.unwrap_or_else(Date::today)
    }

    /// Opens another window while `*open` is true, with `options`' title, size and position; its
    /// content comes from [`App::window`](crate::App::window) with this `key`. When the user
    /// closes it, `*open` becomes false. Setting `*open` to false closes it. Only the main
    /// window's view opens windows.
    ///
    /// ```
    /// use kargui::Options;
    /// # let mut settings = false;
    /// # kargui::test::example(|ui| {
    /// if ui.button("Settings").clicked() {
    ///     settings = true;
    /// }
    /// ui.window("settings", &Options { size: (400.0, 300.0), ..Options::new("Settings") }, &mut settings);
    /// # });
    /// ```
    pub fn window(&mut self, key: &str, options: &crate::Options, open: &mut bool) {
        if self.cx.closed_windows.remove(key) {
            *open = false;
            crate::log::event("window.closed_by_user", &format!("key={key}"), "open=false");
        }
        if *open {
            self.cx.windows.push(context::WindowRequest { key: key.to_string(), title: options.title.clone(), size: options.size, position: options.position, activate: options.activate, icon: options.icon.clone(), min_size: options.min_size });
        }
    }

    /// Shows the system's dialog to open or save files, after this frame (only one at a time; a
    /// new request replaces one not shown yet). The chosen files come back in a later frame
    /// through [`Ui::chosen_files`] or [`Ui::chosen_file`] with the same `key`. The app keeps
    /// running and drawing while the dialog is open; it is modal for the window.
    ///
    /// ```
    /// use kargui::FileDialog;
    /// # let mut text = String::new();
    /// # kargui::test::example(|ui| {
    /// if ui.button("Open…").clicked() {
    ///     ui.file_dialog("open", FileDialog::open().filter("Text", &["txt", "md"]));
    /// }
    /// if let Some(path) = ui.chosen_file("open") {
    ///     text = std::fs::read_to_string(path).unwrap_or_default();
    /// }
    /// # });
    /// ```
    pub fn file_dialog(&mut self, key: &str, dialog: FileDialog) {
        if self.disabled {
            return;
        }
        crate::log::event("file_dialog.request", &format!("key={key} save={}", dialog.save), "queued");
        self.cx.file_dialog_request = Some((key.to_string(), dialog));
    }

    /// The files chosen in the dialog shown with `key`, once, in the frame after it closed.
    /// `None` before that, and when the user cancelled.
    ///
    /// ```
    /// use kargui::FileDialog;
    /// # let mut photos: Vec<std::path::PathBuf> = Vec::new();
    /// # kargui::test::example(|ui| {
    /// if ui.button("Add photos…").clicked() {
    ///     ui.file_dialog("photos", FileDialog::open().filter("Pictures", &["png", "jpg"]).multiple());
    /// }
    /// if let Some(files) = ui.chosen_files("photos") {
    ///     photos.extend(files);
    /// }
    /// # });
    /// ```
    pub fn chosen_files(&mut self, key: &str) -> Option<Vec<PathBuf>> {
        self.cx.file_results.remove(key)
    }

    /// The first file chosen in the dialog shown with `key`. See [`Ui::chosen_files`].
    ///
    /// ```
    /// use kargui::FileDialog;
    /// # let mut target = None;
    /// # kargui::test::example(|ui| {
    /// if ui.button("Save as…").clicked() {
    ///     ui.file_dialog("save", FileDialog::save().file_name("notes.txt"));
    /// }
    /// if let Some(path) = ui.chosen_file("save") {
    ///     target = Some(path);
    /// }
    /// # });
    /// ```
    pub fn chosen_file(&mut self, key: &str) -> Option<PathBuf> {
        self.chosen_files(key).and_then(|f| f.into_iter().next())
    }

    /// Ends the app after this frame: the main window (and every other window) closes, without
    /// asking [`App::close_requested`](crate::App::close_requested).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// if ui.button("Exit").clicked() {
    ///     ui.quit();
    /// }
    /// # });
    /// ```
    pub fn quit(&mut self) {
        crate::log::event("app.quit", "", "requested");
        self.cx.quit = true;
    }

    /// A built-in [`Icon`], 16 logical pixels by default, in the text color. It is decorative:
    /// screen readers skip it unless you give it a name with `.label(..)`.
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.icon(Icon::Info).weak();
    ///     ui.label("Changes are saved automatically.");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn icon(&mut self, icon: Icon) -> IconView<'_> {
        let (idx, id) = self.add(Kind::Icon { icon, size: 16, weak: false }, Dir::Leaf);
        // 決めた大きさにして、伸ばす並べ方（列）の中でも広がらないようにする
        let s = Size::Fixed(self.px(16.0));
        (self.tree.layout[idx].width, self.tree.layout[idx].height) = (s, s);
        IconView(self.handle(idx, id))
    }

    /// A picture, as large as it has pixels (in logical pixels) unless you set `.size(..)`. Screen
    /// readers skip it unless you give it a description with `.label(..)`. On the optional
    /// Direct3D 11 renderer, images are not drawn yet.
    ///
    /// ```
    /// use kargui::Image;
    /// # let photo = Image::from_rgba(4, 2, vec![200; 32]).unwrap();
    /// # kargui::test::example(|ui| {
    /// ui.image(&photo).size(160.0, 80.0).label("The office at night");
    /// # });
    /// ```
    #[track_caller]
    pub fn image(&mut self, image: &Image) -> ImageView<'_> {
        let kind = Kind::Image { image: image.clone(), w: image.width(), h: image.height() };
        let (idx, id) = self.add(kind, Dir::Leaf);
        // 決めた大きさにして、伸ばす並べ方（列）の中でもゆがまないようにする
        self.tree.layout[idx].width = Size::Fixed(self.px(image.width() as f32));
        self.tree.layout[idx].height = Size::Fixed(self.px(image.height() as f32));
        ImageView(self.handle(idx, id))
    }

    // ---- 押す物 ----

    /// A square button that shows only an icon. `name` is what screen readers say and is required,
    /// because the icon has no text. Ask [`Button::clicked`] whether it was clicked.
    ///
    /// ```
    /// use kargui::Icon;
    /// # let mut open = true;
    /// # kargui::test::example(|ui| {
    /// if ui.icon_button(Icon::Close, "Close").clicked() {
    ///     open = false;
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn icon_button(&mut self, icon: Icon, name: impl Into<String>) -> Button<'_> {
        let (idx, id) = self.add(Kind::Button { text: String::new(), primary: false, small: false, icon: Some(icon) }, Dir::Leaf);
        self.tree.info[idx].label = Some(name.into());
        let clicked = !self.disabled && self.cx.clicked == Some(id);
        Button { h: self.handle(idx, id), clicked }
    }

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
        let (idx, id) = self.add(Kind::Button { text: text.into(), primary: false, small: false, icon: None }, Dir::Leaf);
        let clicked = !self.disabled && self.cx.clicked == Some(id);
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
        let (idx, id) = self.add(Kind::Checkbox { checked: *checked, text: text.into(), strike: false, mixed: false }, Dir::Leaf);
        let changed = !self.disabled && self.cx.clicked == Some(id);
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
        let changed = !self.disabled && self.cx.clicked == Some(id);
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
        let changed = !self.disabled && self.cx.clicked == Some(id);
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
                if !ui.disabled && ui.cx.clicked == Some(id) && *v != *value {
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
        let (idx, id) = self.add(Kind::TextInput { value: String::new(), hint: String::new(), password: false }, Dir::Leaf);
        let (changed, submitted) = self.edit(id, text, Field::Line);
        if let Kind::TextInput { value, .. } = &mut self.tree.info[idx].kind {
            *value = text.clone();
        }
        let composing = (self.cx.focus == Some(id) && !self.cx.preedit.is_empty()).then(|| self.cx.preedit.clone());
        TextInput { h: self.handle(idx, id), changed, submitted, composing }
    }

    /// A text field for several lines (notes, messages). Enter starts a new line; Ctrl+Enter is
    /// left to the app (see [`Ui::shortcut`]) for sending. Long text wraps and scrolls; the arrows,
    /// Home/End (line), Ctrl+Home/End (whole text) and Page Up/Down move the caret. Typing, the
    /// input method, selection, copy and paste, undo and redo work as in [`Ui::text_input`]. Pasted
    /// Windows line ends (CR LF) become `\n`.
    ///
    /// ```
    /// use kargui::{Key, Mods};
    /// # let (mut message, mut sent) = (String::new(), Vec::<String>::new());
    /// # kargui::test::example(|ui| {
    /// ui.text_area(&mut message).hint("Message").rows(5).grow();
    /// if ui.shortcut(Key::Enter, Mods::CTRL) && !message.is_empty() {
    ///     sent.push(std::mem::take(&mut message));
    /// }
    /// # });
    /// ```
    #[track_caller]
    pub fn text_area(&mut self, text: &mut String) -> TextArea<'_> {
        let (idx, id) = self.add(Kind::TextArea { value: String::new(), hint: String::new(), rows: 4 }, Dir::Leaf);
        let (changed, _) = self.edit(id, text, Field::Area);
        if let Kind::TextArea { value, .. } = &mut self.tree.info[idx].kind {
            *value = text.clone();
        }
        let composing = (self.cx.focus == Some(id) && !self.cx.preedit.is_empty()).then(|| self.cx.preedit.clone());
        TextArea { h: self.handle(idx, id), changed, composing }
    }

    /// 焦点のある入力欄に、この回の入力を当てる。(変わったか, 送信したか)
    fn edit(&mut self, id: Id, s: &mut String, field: Field) -> (bool, bool) {
        let multiline = field == Field::Area;
        let password = field == Field::Password;
        if self.disabled {
            return (false, false);
        }
        let before = s.clone();
        let mut submitted = false;
        let style = crate::text::TextStyle { size: self.px(TextKind::Body.size()), bold: false };
        self.cx.edits.entry(id).or_default().clamp(s);
        self.cx.edits.get_mut(&id).unwrap().multiline = multiline;
        // 焦点が移る前に確定した変換中の文字（押した位置へキャレットを動かすより先に入れる）
        if self.cx.pending_commit.as_ref().is_some_and(|(pid, _)| *pid == id) {
            let (_, t) = self.cx.pending_commit.take().unwrap();
            self.cx.edits.get_mut(&id).unwrap().insert(s, &t, false);
        }
        if let Some((pid, x, y, shift)) = self.cx.pointer_text {
            if pid == id {
                self.cx.pointer_text = None;
                // 複数行: 押した点の行と位置（前のフレームの、文字の左上と折り返しの幅で並べ直して探す）
                if let (true, Some(&(ox, oy, w, _))) = (multiline, self.cx.prev.areas.get(&id)) {
                    let laid = self.cx.text.layout(s, style, Some(w));
                    let at = edit::byte_of(s, laid.index_at_point(x - ox, y - oy));
                    let st = self.cx.edits.get_mut(&id).unwrap();
                    st.move_to(at, shift);
                    st.goal_x = None;
                    self.cx.reveal = Some(id);
                } else if let Some(&(_, text_x)) = self.cx.prev.inputs.get(&id) {
                    let scroll = self.cx.edit_scroll.get(&id).copied().unwrap_or(0.0);
                    let at = if password {
                        // 画面の ● の何個目か → 元の文字の何文字目か
                        let laid = self.cx.text.layout(&edit::mask(s), style, None);
                        edit::byte_of_char(s, laid.index_at(x - text_x + scroll))
                    } else {
                        let laid = self.cx.text.layout(s, style, None);
                        edit::byte_of(s, laid.index_at(x - text_x + scroll))
                    };
                    self.cx.edits.get_mut(&id).unwrap().move_to(at, shift);
                }
            }
        }
        if self.cx.focus != Some(id) {
            return (false, false);
        }
        let keys = std::mem::take(&mut self.cx.keys);
        let cx = &mut *self.cx;
        if multiline && !keys.is_empty() {
            // 打った・動かした後は、キャレットが見えるようにスクロールする（ホイールで動かした時はしない）
            cx.reveal = Some(id);
        }
        let st = cx.edits.get_mut(&id).unwrap();
        for e in keys {
            // 上下に続けて動く間だけ、横の位置を覚えておく
            if !matches!(e, Event::Key { key: Key::Up | Key::Down | Key::PageUp | Key::PageDown, .. }) {
                st.goal_x = None;
            }
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
                    Key::Enter if multiline => {
                        st.insert(s, "\n", true);
                    }
                    Key::Up | Key::Down | Key::PageUp | Key::PageDown if multiline => {
                        if let Some(&(_, _, w, view_h)) = cx.prev.areas.get(&id) {
                            let laid = cx.text.layout(s, style, Some(w));
                            if let Some(to) = vertical_move(&laid, s, st, key, view_h) {
                                st.move_to(to, mods.shift);
                            }
                        }
                    }
                    Key::Home | Key::End if multiline && !mods.ctrl => {
                        if let Some(&(_, _, w, _)) = cx.prev.areas.get(&id) {
                            let laid = cx.text.layout(s, style, Some(w));
                            let l = laid.lines[edit::line_of(&laid.lines, edit::utf16_of(s, st.cursor))];
                            let to = if key == Key::Home { l.start } else { l.end };
                            st.move_to(edit::byte_of(s, to), mods.shift);
                        }
                    }
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
                    // パスワードは写さない・切り取らない
                    Key::C | Key::X if mods.ctrl && password => {}
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
        if !self.disabled && (self.cx.pressed == Some(id) || self.cx.released == Some(id)) {
            let rect = self.cx.prev.hits.iter().find(|h| h.id == id).map(|h| h.rect);
            if let (Some((x, _)), Some(r)) = (self.cx.pointer, rect) {
                let (tx, tw) = draw::slider_track(r, self.cx.scale);
                let f = if tw > 0.0 { ((x - tx) / tw).clamp(0.0, 1.0) as f64 } else { 0.0 };
                v = min + f * (max - min);
            }
        }
        // キー: 焦点がある時だけ
        if !self.disabled && self.cx.focus == Some(id) {
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
        let (idx, id) = self.add(Kind::List { count, separators: true, view_h: 0.0, table: false }, Dir::List);
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

    /// Whether `key` was pressed with exactly the modifier keys `mods` in this frame, and no widget
    /// used it. The focused widget keeps the keys it needs: a text field keeps letters, digits,
    /// Enter, the editing keys and Ctrl+A/C/V/X/Y/Z, a slider or a selectable list keeps the arrows.
    /// Everything else (Ctrl+S, F5, Escape with nothing to close, ...) comes here. Inside
    /// [`Ui::enabled`]`(false, ..)` it is always false.
    ///
    /// ```
    /// use kargui::{Key, Mods};
    /// # let mut saved = 0;
    /// # kargui::test::example(|ui| {
    /// if ui.shortcut(Key::S, Mods::CTRL) {
    ///     saved += 1;
    /// }
    /// if ui.shortcut(Key::S, Mods::CTRL | Mods::SHIFT) {
    ///     // save as
    /// }
    /// # });
    /// ```
    pub fn shortcut(&self, key: Key, mods: Mods) -> bool {
        !self.disabled && self.cx.app_keys.contains(&(key, mods))
    }

    /// The time of this frame. Use it with [`Ui::request_frame_after`] for timers and countdowns.
    /// In tests, [`Harness::wait`](crate::test::Harness::wait) moves it forward.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let started = ui.now();
    /// assert!(ui.now() >= started);
    /// # });
    /// ```
    pub fn now(&self) -> Instant {
        self.cx.now
    }

    /// Asks for a frame after `after` has passed, without drawing in between (the window sleeps
    /// until then). If it is called several times, the earliest time wins. Use it for timers,
    /// blinking or anything that changes with time but not every frame.
    ///
    /// ```
    /// use std::time::Duration;
    /// # kargui::test::example(|ui| {
    /// // show the clock again in a second
    /// ui.request_frame_after(Duration::from_secs(1));
    /// # });
    /// ```
    pub fn request_frame_after(&mut self, after: Duration) {
        let t = self.cx.now + after;
        self.cx.wake_by(t);
    }

    /// A [`Waker`] that wakes this window from another thread.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let waker = ui.waker();
    /// std::thread::spawn(move || waker.wake());
    /// # });
    /// ```
    pub fn waker(&self) -> Waker {
        self.cx.waker.clone()
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
    cx: &'t mut Context,
    tree: &'t mut Tree,
    idx: usize,
    id: Id,
    scale: f32,
    parent_dir: Dir,
    /// この部品のツールチップを出すか（Handle::tooltip が使う）
    tip: TipState,
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

    /// Shows `text` in a small box below the widget when the pointer rests on it for half a second
    /// or the keyboard moves the focus to it, and gives it to screen readers as the widget's
    /// description. Escape and pressing the widget hide it.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     ui.label("Status");
    /// })
    /// .tooltip("Shows whether the last save worked");
    /// # });
    /// ```
    pub fn tooltip(self, text: impl Into<String>) -> Self {
        let text = text.into();
        self.tree.info[self.idx].description = Some(text.clone());
        match self.tip {
            // 出す時刻に起こしてもらう（それまでは描かない）
            TipState::Waiting(due) => self.cx.wake_by(due),
            TipState::Shown => {
                let (id, scale) = (self.id, self.scale);
                let ov = Id(hash_of((id.0, "tooltip")));
                let idx = self.tree.info.len();
                let kind = Kind::Overlay { kind: OverlayKind::Tooltip, title: String::new() };
                self.tree.info.push(NodeInfo { id: ov, kind, label: None, disabled: false, description: None, context_menu: false, skip_tab: false });
                let mut l = LayoutNode::new(Dir::Column);
                l.pad = ((8.0 * scale).round(), (5.0 * scale).round());
                self.tree.layout.push(l);
                let label = Kind::Label { text, size: TextKind::Small, bold: false, weak: false, strike: false, wrap: false, right: false };
                self.tree.info.push(NodeInfo { id: Id(hash_of((id.0, "tooltip-text"))), kind: label, label: None, disabled: false, description: None, context_menu: false, skip_tab: false });
                self.tree.layout.push(LayoutNode::new(Dir::Leaf));
                self.tree.layout[idx].children.push(idx + 1);
                self.tree.overlays.push(Overlay { idx, id: ov, anchor: Some(id), kind: OverlayKind::Tooltip, place: Place::Below });
            }
            TipState::Hidden => {}
        }
        self
    }

    /// A menu that opens where the widget is right-clicked, or below it with Shift+F10 or the
    /// context menu key while the focus is on it (or on a widget inside it). Build the items in `f`
    /// as in [`Ui::menu_button`]. While the focus is inside the widget, the items' shortcuts work
    /// even when the menu is closed.
    ///
    /// ```
    /// # let mut files = vec!["a.txt".to_string(), "b.txt".to_string()];
    /// # kargui::test::example(|ui| {
    /// let mut remove = None;
    /// for (k, name) in files.iter().enumerate() {
    ///     ui.row(|ui| {
    ///         ui.label(name);
    ///     })
    ///     .context_menu(|m| {
    ///         if m.item("Delete").clicked() {
    ///             remove = Some(k);
    ///         }
    ///     });
    /// }
    /// if let Some(k) = remove {
    ///     files.remove(k);
    /// }
    /// # });
    /// ```
    pub fn context_menu(self, f: impl FnOnce(&mut Menu)) -> Self {
        let (idx, id) = (self.idx, self.id);
        self.tree.info[idx].context_menu = true;
        let disabled = self.tree.info[idx].disabled;
        let pop = Id(hash_of((id.0, "context")));
        let within = self.cx.focus.is_some_and(|f| f == id || descends(self.tree, idx, f));
        let cx = &mut *self.cx;
        if cx.closing.contains(&pop) && cx.menus.first() == Some(&pop) {
            cx.menus_truncate(0);
        }
        if let Some((_, at)) = cx.context_request.filter(|&(t, _)| t == id && !disabled) {
            cx.context_request = None;
            cx.context_at = at;
            cx.overlay_focus_hint.remove(&pop);
            cx.menus_open(0, pop);
        }
        let open = cx.menus.first() == Some(&pop);
        if open || (within && !disabled) {
            let (x, y) = cx.context_at;
            let mut ui = Ui { cx: &mut *self.cx, tree: &mut *self.tree, parent: idx, salt: 0, disabled };
            let out = ui.menu_body(pop, None, Place::At(x, y), 0, open, false, f);
            if out.done && ui.cx.menus.first() == Some(&pop) {
                ui.cx.menus_truncate(0);
            }
        }
        self
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
        self.cx.scroll_into_view = Some(self.id);
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
            /// Shows a tooltip and gives screen readers a description. See [`Handle::tooltip`].
            ///
            /// ```
            #[doc = $setup]
            /// # kargui::test::example(|ui| {
            #[doc = concat!(" ui.", $make, ".tooltip(\"More about this\");")]
            /// # });
            /// ```
            pub fn tooltip(self, text: impl Into<String>) -> Self {
                Self { h: self.h.tooltip(text), ..self }
            }
            /// Adds a menu that opens on a right-click. See [`Handle::context_menu`].
            ///
            /// ```
            #[doc = $setup]
            /// # kargui::test::example(|ui| {
            #[doc = concat!(" ui.", $make, ".context_menu(|m| {")]
            ///     m.item("Copy");
            /// });
            /// # });
            /// ```
            pub fn context_menu(self, f: impl FnOnce(&mut Menu)) -> Self {
                Self { h: self.h.context_menu(f), ..self }
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
    /// Adds a menu that opens on a right-click. See [`Handle::context_menu`].
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.label("report.pdf").context_menu(|m| {
    ///     m.item("Open");
    /// });
    /// # });
    /// ```
    pub fn context_menu(self, f: impl FnOnce(&mut Menu)) -> Self {
        Label(self.0.context_menu(f))
    }
}

/// A picture. See [`Ui::image`].
///
/// ```
/// # let logo = kargui::Image::from_rgba(2, 2, vec![255; 16]).unwrap();
/// # kargui::test::example(|ui| {
/// ui.image(&logo).size(32.0, 32.0).label("Company logo");
/// # });
/// ```
pub struct ImageView<'t>(Handle<'t>);

impl ImageView<'_> {
    /// Shows the picture at `width` × `height` logical pixels (stretched to fit; keep the ratio of
    /// the picture's sides to avoid distortion).
    ///
    /// ```
    /// # let photo = kargui::Image::from_rgba(4, 3, vec![200; 48]).unwrap();
    /// # kargui::test::example(|ui| {
    /// ui.image(&photo).size(120.0, 90.0);
    /// # });
    /// ```
    pub fn size(mut self, width: f32, height: f32) -> Self {
        let (width, height) = (width.max(1.0).round(), height.max(1.0).round());
        if let Kind::Image { w, h, .. } = self.0.kind() {
            *w = width as u32;
            *h = height as u32;
        }
        ImageView(self.0.width(Size::Fixed(width)).height(Size::Fixed(height)))
    }
    /// Describes the picture for screen readers (so it is read instead of skipped).
    ///
    /// ```
    /// # let chart = kargui::Image::from_rgba(2, 2, vec![255; 16]).unwrap();
    /// # kargui::test::example(|ui| {
    /// ui.image(&chart).label("Sales rose 12% in March");
    /// # });
    /// ```
    pub fn label(self, description: impl Into<String>) -> Self {
        ImageView(self.0.label(description))
    }
    /// Shows a tooltip. See [`Handle::tooltip`].
    ///
    /// ```
    /// # let map = kargui::Image::from_rgba(2, 2, vec![255; 16]).unwrap();
    /// # kargui::test::example(|ui| {
    /// ui.image(&map).tooltip("Drawn on 21 November");
    /// # });
    /// ```
    pub fn tooltip(self, text: impl Into<String>) -> Self {
        ImageView(self.0.tooltip(text))
    }
}

/// An icon. See [`Ui::icon`].
///
/// ```
/// use kargui::Icon;
/// # kargui::test::example(|ui| {
/// ui.icon(Icon::Warning).size(20.0).label("Warning");
/// # });
/// ```
pub struct IconView<'t>(Handle<'t>);

impl IconView<'_> {
    /// Sets the size in logical pixels (the default is 16).
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.icon(Icon::Search).size(24.0);
    /// # });
    /// ```
    pub fn size(mut self, px: f32) -> Self {
        let px = px.max(1.0).round();
        if let Kind::Icon { size, .. } = self.0.kind() {
            *size = px as u32;
        }
        IconView(self.0.width(Size::Fixed(px)).height(Size::Fixed(px)))
    }
    /// Draws in the weaker text color.
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.icon(Icon::Info).weak();
    /// # });
    /// ```
    pub fn weak(mut self) -> Self {
        if let Kind::Icon { weak, .. } = self.0.kind() {
            *weak = true;
        }
        self
    }
    /// Gives the icon a name, so screen readers read it as a picture instead of skipping it.
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.icon(Icon::Warning).label("Warning");
    /// # });
    /// ```
    pub fn label(self, name: impl Into<String>) -> Self {
        IconView(self.0.label(name))
    }
    /// Shows a tooltip. See [`Handle::tooltip`].
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.icon(Icon::Info).tooltip("Saved every minute");
    /// # });
    /// ```
    pub fn tooltip(self, text: impl Into<String>) -> Self {
        IconView(self.0.tooltip(text))
    }
}

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
    /// Shows `icon` before the text.
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.button("New task").icon(Icon::Plus).primary();
    /// # });
    /// ```
    pub fn icon(mut self, icon: Icon) -> Self {
        if let Kind::Button { icon: i, .. } = self.h.kind() {
            *i = Some(icon);
        }
        self
    }
    /// Disables the button when `enabled` is false, as [`Ui::enabled`] does for a group: drawn
    /// dimmed, not clickable, skipped by Tab, and announced as unavailable. Call it before
    /// [`Button::clicked`], which then returns false.
    ///
    /// ```
    /// # let mut selected: Option<usize> = None;
    /// # kargui::test::example(|ui| {
    /// if ui.button("Delete").enabled(selected.is_some()).clicked() {
    ///     // delete the selected item
    /// }
    /// # });
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        if !enabled {
            self.clicked = false;
            self.h.tree.info[self.h.idx].disabled = true;
        }
        self
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
    /// Shows a check box as partly checked (a dash) when `mixed` is true, for example for "select
    /// all" when only some items are selected. Screen readers hear "mixed". Clicking still flips
    /// the value; decide in your code what it means.
    ///
    /// ```
    /// # let items = [true, false, true];
    /// # kargui::test::example(|ui| {
    /// let mut all = items.iter().all(|&c| c);
    /// let some = items.iter().any(|&c| c);
    /// ui.checkbox(&mut all, "Select all").mixed(some && !all);
    /// # });
    /// ```
    pub fn mixed(mut self, mixed: bool) -> Self {
        if let Kind::Checkbox { mixed: m, .. } = self.h.kind() {
            *m = mixed;
        }
        self
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

/// A text field for several lines. See [`Ui::text_area`].
///
/// ```
/// # let mut notes = String::new();
/// # kargui::test::example(|ui| {
/// if ui.text_area(&mut notes).hint("Notes").rows(6).changed() {
///     // save the notes
/// }
/// # });
/// ```
pub struct TextArea<'t> {
    h: Handle<'t>,
    changed: bool,
    composing: Option<String>,
}

common!(TextArea, " # let mut text = String::new();", "text_area(&mut text)");

impl TextArea<'_> {
    /// Gray text shown while the field is empty. Screen readers use it as the name.
    ///
    /// ```
    /// # let mut text = String::new();
    /// # kargui::test::example(|ui| {
    /// ui.text_area(&mut text).hint("Describe the problem");
    /// # });
    /// ```
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        if let Kind::TextArea { hint: h, .. } = self.h.kind() {
            *h = hint.into();
        }
        self
    }
    /// How many lines are visible (the height; the default is 4). Longer text scrolls.
    ///
    /// ```
    /// # let mut text = String::new();
    /// # kargui::test::example(|ui| {
    /// ui.text_area(&mut text).rows(10);
    /// # });
    /// ```
    pub fn rows(mut self, rows: u32) -> Self {
        if let Kind::TextArea { rows: r, .. } = self.h.kind() {
            *r = rows.max(1);
        }
        self
    }
    /// Whether the text changed in this frame.
    ///
    /// ```
    /// # let mut text = String::new();
    /// # kargui::test::example(|ui| {
    /// if ui.text_area(&mut text).changed() {
    ///     // mark the document as edited
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
    /// The text the input method is composing (not yet in the value), if any.
    ///
    /// ```
    /// # let mut text = String::new();
    /// # kargui::test::example(|ui| {
    /// let area = ui.text_area(&mut text);
    /// let busy = area.composing().is_some();
    /// # let _ = busy;
    /// # });
    /// ```
    pub fn composing(&self) -> Option<&str> {
        self.composing.as_deref()
    }
}

/// A drop-down. See [`Ui::dropdown`].
///
/// ```
/// # let mut n = 10;
/// # kargui::test::example(|ui| {
/// let rows = ui.dropdown(&mut n, &[(10, "10 rows"), (50, "50 rows"), (100, "100 rows")]).label("Rows per page");
/// if rows.changed() {
///     // show more or fewer rows
/// }
/// # });
/// ```
pub struct Dropdown<'t> {
    h: Handle<'t>,
    changed: bool,
}

common!(Dropdown, " # let mut n = 1;", "dropdown(&mut n, &[(1, \"One\"), (2, \"Two\")])");

impl Dropdown<'_> {
    /// Whether another option was picked in this frame.
    ///
    /// ```
    /// # let mut n = 1;
    /// # kargui::test::example(|ui| {
    /// if ui.dropdown(&mut n, &[(1, "One"), (2, "Two")]).changed() {
    ///     // react to the new choice
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// 焦点が items のどれかにある時、上下・Home・End で焦点を移す（端で折り返す）。選択の一覧とメニューが使う
fn move_focus_with_arrows(cx: &mut Context, items: &[Id]) {
    let Some(at) = cx.focus.and_then(|f| items.iter().position(|&i| i == f)) else { return };
    let n = items.len();
    let mut next = at;
    for e in std::mem::take(&mut cx.keys) {
        if let Event::Key { key, .. } = e {
            next = match key {
                Key::Down => (next + 1) % n,
                Key::Up => (next + n - 1) % n,
                Key::Home => 0,
                Key::End => n - 1,
                _ => next,
            };
        }
    }
    if next != at {
        cx.focus = Some(items[next]);
        cx.focus_visible = true;
    }
}

/// ポインターが入れ子のメニューの項目に止まってから開く（ほかの項目なら閉じる）までの時間
const SUBMENU_DELAY: Duration = Duration::from_millis(400);

/// メニューの中身を作った結果
struct MenuOut {
    /// 項目を選んだ（開いているメニューを全部閉じる）
    done: bool,
    /// メニューバーのメニューで、左右の矢印で隣のメニューへ移る向き（0 なら移らない）
    step: i32,
    /// 使える項目（並んだ順）
    items: Vec<Id>,
}

/// The items of a menu, built in the closure given to [`Ui::menu_button`], [`MenuBar::menu`],
/// [`Menu::submenu`] or `.context_menu(..)`. While the menu is closed, the closure still runs but
/// creates nothing, so that the items' [`MenuItem::shortcut`]s work.
///
/// ```
/// # let mut zoom = 100;
/// # kargui::test::example(|ui| {
/// ui.menu_button("View", |m| {
///     if m.item("Zoom in").clicked() {
///         zoom += 10;
///     }
///     if m.item("Zoom out").clicked() {
///         zoom -= 10;
///     }
/// });
/// # });
/// ```
pub struct Menu<'m> {
    ui: Ui<'m>,
    open: bool,
    /// 入れ子の深さ（一番外が 0）
    depth: usize,
    /// メニューバーのメニュー（左右の矢印で隣へ移る）
    in_bar: bool,
    done: bool,
    step: i32,
}

impl Menu<'_> {
    /// An item that does something when chosen (clicked, or Enter or Space on it). Ask
    /// [`MenuItem::clicked`]; give it keys with [`MenuItem::shortcut`].
    ///
    /// ```
    /// # let mut copies = 0;
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("More", |m| {
    ///     if m.item("Duplicate").clicked() {
    ///         copies += 1;
    ///     }
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn item(&mut self, text: impl Into<String>) -> MenuItem<'_> {
        self.entry(text.into(), MenuMark::None, None)
    }

    /// An item with a check mark that flips `*value` when chosen. Screen readers hear a checkable
    /// menu item.
    ///
    /// ```
    /// # let mut wrap = false;
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("View", |m| {
    ///     m.check(&mut wrap, "Word wrap");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn check<'s>(&'s mut self, value: &'s mut bool, text: impl Into<String>) -> MenuItem<'s> {
        let mark = MenuMark::Check(*value);
        self.entry(text.into(), mark, Some(Box::new(move || *value = !*value)))
    }

    /// One of several items where one is chosen: it shows a dot when `*value == this` and sets
    /// `*value = this` when chosen. Screen readers hear a radio menu item.
    ///
    /// ```
    /// #[derive(Clone, Copy, PartialEq)]
    /// enum Sort { Name, Date }
    /// # let mut sort = Sort::Name;
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("Sort", |m| {
    ///     m.radio(&mut sort, Sort::Name, "By name");
    ///     m.radio(&mut sort, Sort::Date, "By date");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn radio<'s, T: Copy + PartialEq + 's>(&'s mut self, value: &'s mut T, this: T, text: impl Into<String>) -> MenuItem<'s> {
        let mark = MenuMark::Radio(*value == this);
        self.entry(text.into(), mark, Some(Box::new(move || *value = this)))
    }

    /// A line between groups of items.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("File", |m| {
    ///     m.item("Open");
    ///     m.separator();
    ///     m.item("Exit");
    /// });
    /// # });
    /// ```
    pub fn separator(&mut self) {
        if self.open {
            self.ui.space(2.0);
            self.ui.separator();
            self.ui.space(2.0);
        }
    }

    /// An item that opens another menu beside it: when clicked, when the pointer rests on it, or
    /// with Right (Left closes it again).
    ///
    /// ```
    /// # let mut opened = None;
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("File", |m| {
    ///     m.submenu("Open recent", |m| {
    ///         for name in ["notes.txt", "plan.md"] {
    ///             if m.item(name).clicked() {
    ///                 opened = Some(name);
    ///             }
    ///         }
    ///     });
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn submenu(&mut self, text: impl Into<String>, f: impl FnOnce(&mut Menu)) {
        let d = self.depth + 1;
        if !self.open {
            // 閉じている時も、中の項目のショートカットのために回す
            let parent = self.ui.parent;
            let mut m = Menu { ui: self.ui.child(parent), open: false, depth: d, in_bar: self.in_bar, done: false, step: 0 };
            f(&mut m);
            return;
        }
        let kind = Kind::MenuItem { text: text.into(), shortcut: None, icon: None, mark: MenuMark::None, submenu: true, open: false, gutter: false };
        let (idx, id) = self.ui.add(kind, Dir::Leaf);
        let pop = Id(hash_of((id.0, "submenu")));
        let enabled = !self.ui.disabled;
        let cx = &mut *self.ui.cx;
        if cx.closing.contains(&pop) && cx.menus.get(d) == Some(&pop) {
            cx.menus_truncate(d);
        }
        if enabled {
            let key = cx.focus == Some(id) && take_key(cx, Key::Right);
            if cx.clicked == Some(id) || key {
                cx.overlay_focus_hint.remove(&pop);
                cx.menus_open(d, pop);
            }
        }
        if enabled && self.rest(id) {
            self.ui.cx.menus_open(d, pop);
        }
        let open = enabled && self.ui.cx.menus.get(d) == Some(&pop);
        let out = self.ui.menu_body(pop, Some(id), Place::Beside, d, open, self.in_bar, f);
        if out.done {
            self.done = true;
        }
        if out.step != 0 {
            self.step = out.step;
        }
        let open = self.ui.cx.menus.get(d) == Some(&pop);
        if let Kind::MenuItem { open: o, .. } = &mut self.ui.tree.info[idx].kind {
            *o = open;
        }
    }

    #[track_caller]
    fn entry<'s>(&'s mut self, text: String, mark: MenuMark, apply: Option<Box<dyn FnMut() + 's>>) -> MenuItem<'s> {
        let enabled = !self.ui.disabled;
        if !self.open {
            return MenuItem { cx: &mut *self.ui.cx, node: None, clicked: false, keyed: false, enabled, apply };
        }
        let kind = Kind::MenuItem { text, shortcut: None, icon: None, mark, submenu: false, open: false, gutter: false };
        let (idx, id) = self.ui.add(kind, Dir::Leaf);
        let mut apply = apply;
        let clicked = enabled && self.ui.cx.clicked == Some(id);
        if clicked {
            self.done = true;
            if let Some(mut a) = apply.take() {
                a();
            }
        }
        // ポインターが止まったら、開いている入れ子のメニューを閉じる
        if self.rest(id) {
            self.ui.cx.menus_truncate(self.depth + 1);
        }
        MenuItem { cx: &mut *self.ui.cx, node: Some((&mut *self.ui.tree, idx)), clicked, keyed: false, enabled, apply }
    }

    /// ポインターが項目 id の上に SUBMENU_DELAY だけ止まった（1 回だけ true。それまでは起こしてもらう）
    fn rest(&mut self, id: Id) -> bool {
        let cx = &mut *self.ui.cx;
        let Some(since) = cx.hover_since.filter(|_| cx.hover == Some(id)) else { return false };
        if cx.rested == Some((id, since)) {
            return false;
        }
        let due = since + SUBMENU_DELAY;
        if cx.now >= due {
            cx.rested = Some((id, since));
            true
        } else {
            cx.wake_by(due);
            false
        }
    }
}

/// An item of a [`Menu`]. Ask it whether it was chosen with [`MenuItem::clicked`].
///
/// ```
/// use kargui::{Key, Mods};
/// # let mut saved = 0;
/// # kargui::test::example(|ui| {
/// ui.menu_button("File", |m| {
///     if m.item("Save").shortcut(Key::S, Mods::CTRL).clicked() {
///         saved += 1;
///     }
/// });
/// # });
/// ```
pub struct MenuItem<'s> {
    cx: &'s mut Context,
    /// 開いている時の項目（木と番号）
    node: Option<(&'s mut Tree, usize)>,
    clicked: bool,
    /// ショートカットのキーが押された
    keyed: bool,
    enabled: bool,
    /// 選んだ時に値を変える（チェックとラジオ）
    apply: Option<Box<dyn FnMut() + 's>>,
}

impl MenuItem<'_> {
    /// Shows `key` with `mods` at the right of the item and chooses the item when they are pressed,
    /// even while the menu is closed (in a context menu, while the focus is inside its widget).
    /// Keys that the focused widget uses itself (such as Ctrl+C in a text field) stay with it.
    ///
    /// ```
    /// use kargui::{Key, Mods};
    /// # let mut found = false;
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("Edit", |m| {
    ///     if m.item("Find").shortcut(Key::F, Mods::CTRL).clicked() {
    ///         found = true;
    ///     }
    /// });
    /// # });
    /// ```
    pub fn shortcut(mut self, key: Key, mods: Mods) -> Self {
        if let Some((tree, idx)) = &mut self.node {
            if let Kind::MenuItem { shortcut, .. } = &mut tree.info[*idx].kind {
                *shortcut = Some(key_label(key, mods));
            }
        }
        self.keyed |= self.cx.app_keys.contains(&(key, mods));
        self
    }

    /// Makes the item unavailable when `enabled` is false: it is drawn dimmed, cannot be chosen
    /// (its shortcut does nothing either), and the arrow keys skip it.
    ///
    /// ```
    /// # let can_paste = false;
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("Edit", |m| {
    ///     m.item("Paste").enabled(can_paste);
    /// });
    /// # });
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        if !enabled {
            self.enabled = false;
            self.clicked = false;
            if let Some((tree, idx)) = &mut self.node {
                tree.info[*idx].disabled = true;
            }
        }
        self
    }

    /// Shows an icon at the left of the item.
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("More", |m| {
    ///     m.item("Remove").icon(Icon::Close);
    /// });
    /// # });
    /// ```
    pub fn icon(mut self, icon: Icon) -> Self {
        if let Some((tree, idx)) = &mut self.node {
            if let Kind::MenuItem { icon: i, .. } = &mut tree.info[*idx].kind {
                *i = Some(icon);
            }
        }
        self
    }

    /// Whether the item was chosen in this frame (clicked, Enter or Space on it, or its shortcut).
    ///
    /// ```
    /// # let mut count = 0;
    /// # kargui::test::example(|ui| {
    /// ui.menu_button("More", |m| {
    ///     if m.item("Count").clicked() {
    ///         count += 1;
    ///     }
    /// });
    /// # });
    /// ```
    pub fn clicked(&self) -> bool {
        self.clicked || (self.keyed && self.enabled)
    }
}

impl Drop for MenuItem<'_> {
    /// ショートカットで選んだチェックとラジオの値を変える（enabled と shortcut の順番に関わらず、最後に決める）
    fn drop(&mut self) {
        if self.keyed && self.enabled {
            if let Some(mut a) = self.apply.take() {
                a();
            }
        }
    }
}

/// The headings of a menu bar. See [`Ui::menu_bar`].
///
/// ```
/// # kargui::test::example(|ui| {
/// ui.menu_bar(|bar| {
///     bar.menu("File", |m| {
///         m.item("New");
///     });
/// });
/// # });
/// ```
pub struct MenuBar<'m> {
    ui: Ui<'m>,
    items: Vec<Id>,
    /// 左右の矢印で隣のメニューへ移る: (何番目の見出しから, 向き)
    step: Option<(usize, i32)>,
}

impl MenuBar<'_> {
    /// A heading that opens the menu built in `f` (see [`Ui::menu_button`] for the items).
    ///
    /// ```
    /// # let mut about = false;
    /// # kargui::test::example(|ui| {
    /// ui.menu_bar(|bar| {
    ///     bar.menu("Help", |m| {
    ///         if m.item("About").clicked() {
    ///             about = true;
    ///         }
    ///     });
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn menu(&mut self, text: impl Into<String>, f: impl FnOnce(&mut Menu)) {
        let tab_stop = self.items.is_empty();
        let (idx, id) = self.ui.add(Kind::MenuBarItem { text: text.into(), open: false, tab_stop }, Dir::Leaf);
        let pop = Id(hash_of((id.0, "menu")));
        // ほかの見出しのメニューが開いている時は、ポインターを乗せるだけで切り替える
        let cx = &mut *self.ui.cx;
        let bar_open = cx.menus.first().is_some_and(|m| cx.prev.menubar.iter().any(|b| Id(hash_of((b.0, "menu"))) == *m));
        if !self.ui.disabled && bar_open && cx.hover == Some(id) && cx.menus.first() != Some(&pop) {
            cx.overlay_focus_hint.remove(&pop);
            cx.menus_open(0, pop);
        }
        let (open, last) = self.ui.root_menu(id, pop);
        let out = self.ui.menu_body(pop, Some(id), Place::Below, 0, open, true, f);
        let cx = &mut *self.ui.cx;
        if out.done && cx.menus.first() == Some(&pop) {
            cx.menus_truncate(0);
            // 選んだ後は、メニューバーへ入る前の場所へ焦点を戻す
            let back = cx.bar_return.take();
            cx.overlay_after.insert(pop, back);
        }
        if let (true, Some(&l)) = (last, out.items.last()) {
            cx.overlay_focus_hint.insert(pop, l);
        }
        if out.step != 0 {
            self.step = Some((self.items.len(), out.step));
        }
        self.items.push(id);
        let open = cx.menus.first() == Some(&pop);
        if let Kind::MenuBarItem { open: o, .. } = &mut self.ui.tree.info[idx].kind {
            *o = open;
        }
    }
}

/// 焦点のある部品へ届いたキーのうち、key（Ctrl と Alt 無し）を取り出す。あったか
fn take_key(cx: &mut Context, key: Key) -> bool {
    let before = cx.keys.len();
    cx.keys.retain(|e| !matches!(e, Event::Key { key: k, mods } if *k == key && !mods.ctrl && !mods.alt));
    cx.keys.len() != before
}

/// 文字のキーの小文字（A〜Z だけ）
fn letter_of(key: Key) -> Option<char> {
    use Key::*;
    const LETTERS: [Key; 26] = [A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z];
    LETTERS.iter().position(|&k| k == key).map(|i| (b'a' + i as u8) as char)
}

/// アプリへ行くはずだった文字のキー（修飾キー無しか Shift）を取り出す（打った文字で項目へ移るのに使う）
fn take_letters(cx: &mut Context) -> Vec<char> {
    let mut typed = Vec::new();
    cx.app_keys.retain(|&(k, m)| match letter_of(k) {
        Some(c) if !m.ctrl && !m.alt => {
            typed.push(c);
            false
        }
        _ => true,
    });
    typed
}

/// from の次から一周して、c で始まる項目（大文字と小文字は区別しない）
fn next_starting(texts: &[&str], from: usize, c: char) -> Option<usize> {
    let n = texts.len();
    (1..=n).map(|s| (from + s) % n).find(|&j| texts[j].chars().next().is_some_and(|f| f.to_lowercase().eq(c.to_lowercase())))
}

/// 焦点が items のどれかにある時、打った文字で始まる次の項目へ焦点を移す（メニュー・選択の一覧）
fn type_ahead(cx: &mut Context, items: &[(Id, String)]) {
    let Some(mut at) = cx.focus.and_then(|f| items.iter().position(|(i, _)| *i == f)) else { return };
    let typed = take_letters(cx);
    if typed.is_empty() {
        return;
    }
    let texts: Vec<&str> = items.iter().map(|(_, t)| t.as_str()).collect();
    for c in typed {
        if let Some(j) = next_starting(&texts, at, c) {
            at = j;
        }
    }
    cx.focus = Some(items[at].0);
    cx.focus_visible = true;
}

/// ショートカットのキーの書き方（Ctrl+Shift+S）
fn key_label(key: Key, mods: Mods) -> String {
    let mut s = String::new();
    if mods.ctrl {
        s.push_str("Ctrl+");
    }
    if mods.alt {
        s.push_str("Alt+");
    }
    if mods.shift {
        s.push_str("Shift+");
    }
    let name = match key {
        Key::Escape => "Esc".to_string(),
        Key::Delete => "Del".to_string(),
        Key::Insert => "Ins".to_string(),
        Key::PageUp => "PgUp".to_string(),
        Key::PageDown => "PgDn".to_string(),
        Key::ContextMenu => "Menu".to_string(),
        k => {
            let d = format!("{k:?}");
            d.strip_prefix("Num").map(str::to_string).unwrap_or(d)
        }
    };
    s.push_str(&name);
    s
}

/// 部品 a の下に id の部品があるか（a 自身を含まない）
fn descends(tree: &Tree, a: usize, id: Id) -> bool {
    tree.layout[a].children.iter().any(|&c| tree.info[c].id == id || descends(tree, c, id))
}

/// Tabs. See [`Ui::tabs`].
///
/// ```
/// # let mut tab = 0;
/// # kargui::test::example(|ui| {
/// let tabs = ui.tabs(&mut tab, &[(0, "Inbox"), (1, "Sent")], |ui, tab| {
///     ui.label(if tab == 0 { "3 new messages" } else { "Nothing sent yet" });
/// });
/// if tabs.changed() {
///     // load the other folder
/// }
/// # });
/// ```
pub struct Tabs<'t> {
    h: Handle<'t>,
    changed: bool,
}

common!(Tabs, " # let mut tab = 0;", "tabs(&mut tab, &[(0, \"One\"), (1, \"Two\")], |ui, _| { ui.label(\"Text\"); })");

impl Tabs<'_> {
    /// Whether another tab was selected in this frame.
    ///
    /// ```
    /// # let mut tab = 0;
    /// # kargui::test::example(|ui| {
    /// if ui.tabs(&mut tab, &[(0, "Inbox"), (1, "Sent")], |ui, _| { ui.label("Mail"); }).changed() {
    ///     // react to the new tab
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// A number field. See [`Ui::number`].
///
/// ```
/// # let mut copies = 1;
/// # kargui::test::example(|ui| {
/// if ui.number(&mut copies, 1..=99).label("Copies").changed() {
///     // update the price
/// }
/// # });
/// ```
pub struct NumberInput<'t> {
    h: Handle<'t>,
    changed: bool,
}

common!(NumberInput, " # let mut n = 1;", "number(&mut n, 0..=10)");

impl NumberInput<'_> {
    /// How much Up, Down and the small buttons change the number (Page Up and Page Down change it
    /// by 10 times this). 1 by default. The new step is used from the next frame.
    ///
    /// ```
    /// # let mut volume = 0.5;
    /// # kargui::test::example(|ui| {
    /// ui.number(&mut volume, 0.0..=1.0).step(0.1).label("Volume");
    /// # });
    /// ```
    pub fn step(mut self, step: f64) -> Self {
        if let Kind::NumberInput { step: s, .. } = self.h.kind() {
            *s = step.abs().max(f64::MIN_POSITIVE);
        }
        self
    }

    /// Whether the number changed in this frame (typed and confirmed, stepped, or set by a screen
    /// reader).
    ///
    /// ```
    /// # let mut n = 1;
    /// # kargui::test::example(|ui| {
    /// if ui.number(&mut n, 0..=10).changed() {
    ///     // react to the new number
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// The commands of a toolbar. See [`Ui::toolbar`].
///
/// ```
/// use kargui::Icon;
/// # kargui::test::example(|ui| {
/// ui.toolbar(|tb| {
///     tb.button(Icon::Search, "Search");
/// })
/// .label("Tools");
/// # });
/// ```
pub struct Toolbar<'m> {
    ui: Ui<'m>,
}

impl<'m> Toolbar<'m> {
    /// A button that shows only an icon; `name` is its tooltip and screen-reader name.
    ///
    /// ```
    /// use kargui::Icon;
    /// # let mut count = 0;
    /// # kargui::test::example(|ui| {
    /// ui.toolbar(|tb| {
    ///     if tb.button(Icon::Plus, "Add").clicked() {
    ///         count += 1;
    ///     }
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn button(&mut self, icon: Icon, name: impl Into<String>) -> Button<'_> {
        let name = name.into();
        let (idx, id) = self.ui.add(Kind::Button { text: String::new(), primary: false, small: false, icon: Some(icon) }, Dir::Leaf);
        self.ui.tree.info[idx].label = Some(name.clone());
        let clicked = !self.ui.disabled && self.ui.cx.clicked == Some(id);
        Button { h: self.ui.handle(idx, id).tooltip(name), clicked }
    }

    /// A button with an icon that stays pressed while `*on` is true; clicking it flips `*on`.
    ///
    /// ```
    /// use kargui::Icon;
    /// # let mut pinned = false;
    /// # kargui::test::example(|ui| {
    /// ui.toolbar(|tb| {
    ///     tb.toggle(&mut pinned, Icon::Check, "Pin");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn toggle(&mut self, on: &mut bool, icon: Icon, name: impl Into<String>) -> Toggled<'_> {
        let name = name.into();
        let (idx, id) = self.ui.add(Kind::ToggleButton { on: *on, text: String::new(), icon: Some(icon) }, Dir::Leaf);
        self.ui.tree.info[idx].label = Some(name.clone());
        let changed = !self.ui.disabled && self.ui.cx.clicked == Some(id);
        if changed {
            *on = !*on;
            if let Kind::ToggleButton { on: o, .. } = &mut self.ui.tree.info[idx].kind {
                *o = *on;
            }
        }
        Toggled { h: self.ui.handle(idx, id).tooltip(name), changed }
    }

    /// A line between groups of buttons.
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.toolbar(|tb| {
    ///     tb.button(Icon::Plus, "Add");
    ///     tb.separator();
    ///     tb.button(Icon::Close, "Remove");
    /// });
    /// # });
    /// ```
    pub fn separator(&mut self) {
        self.ui.separator();
    }

    /// A "More options" button (three dots) that opens a menu, for commands used less often.
    ///
    /// ```
    /// use kargui::Icon;
    /// # kargui::test::example(|ui| {
    /// ui.toolbar(|tb| {
    ///     tb.button(Icon::Plus, "Add");
    ///     tb.more(|m| {
    ///         m.item("Import");
    ///         m.item("Export");
    ///     });
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn more(&mut self, f: impl FnOnce(&mut Menu)) {
        let name = "More options";
        let (idx, id) = self.ui.add(Kind::MenuButton { text: String::new(), open: false, icon: Some(Icon::More) }, Dir::Leaf);
        self.ui.tree.info[idx].label = Some(name.to_string());
        let pop = Id(hash_of((id.0, "menu")));
        let (open, last) = self.ui.root_menu(id, pop);
        let out = self.ui.menu_body(pop, Some(id), Place::Below, 0, open, false, f);
        let cx = &mut *self.ui.cx;
        if out.done && cx.menus.first() == Some(&pop) {
            cx.menus_truncate(0);
        }
        if let (true, Some(&l)) = (last, out.items.last()) {
            cx.overlay_focus_hint.insert(pop, l);
        }
        let open = cx.menus.first() == Some(&pop);
        if let Kind::MenuButton { open: o, .. } = &mut self.ui.tree.info[idx].kind {
            *o = open;
        }
        let _ = self.ui.handle(idx, id).tooltip(name);
    }

    /// The [`Ui`] of the toolbar, to put any other widget in it (a search field, a drop-down).
    ///
    /// ```
    /// # let mut query = String::new();
    /// # kargui::test::example(|ui| {
    /// ui.toolbar(|tb| {
    ///     tb.ui().text_input(&mut query).hint("Search");
    /// });
    /// # });
    /// ```
    pub fn ui(&mut self) -> &mut Ui<'m> {
        &mut self.ui
    }
}

/// 打った文字を数にする（前後の空白を除く。全角の数字・小数点・マイナスも半角にする）
fn parse_number(text: &str) -> Option<f64> {
    let t: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
            '．' => '.',
            '－' | '−' | 'ー' => '-',
            '＋' => '+',
            c => c,
        })
        .collect();
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// 刻みの小数の桁数（0.25 → 2。最大 6）
fn step_decimals(step: f64) -> usize {
    let s = format!("{step}");
    s.split('.').nth(1).map_or(0, |d| d.len()).min(6)
}

/// 刻みの小数の桁にそろえる（足し算の誤差を消す）
fn round_to_step_decimals(v: f64, step: f64) -> f64 {
    let k = 10f64.powi(step_decimals(step) as i32);
    (v * k).round() / k
}

/// 数を欄に出す文字にする（整数は整数で、小数は刻みの桁まで。末尾の 0 は消す）
fn format_number(v: f64, integer: bool, step: f64) -> String {
    if integer {
        return format!("{}", v.round() as i64);
    }
    let s = format!("{:.*}", step_decimals(step).max(1), v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

/// Two panes with a divider. See [`Ui::split_left_right`].
///
/// ```
/// # let mut left = 200.0;
/// # kargui::test::example(|ui| {
/// let split = ui.split_left_right(&mut left, |ui, pane| {
///     ui.label(if pane == 0 { "List" } else { "Details" });
/// });
/// if split.changed() {
///     // remember the new width
/// }
/// # });
/// ```
pub struct Split<'t> {
    h: Handle<'t>,
    /// 仕切りの番号（名前を付ける）
    bar: usize,
    changed: bool,
}

impl Split<'_> {
    /// The divider's name for screen readers ("Resize" if not set).
    ///
    /// ```
    /// # let mut left = 200.0;
    /// # kargui::test::example(|ui| {
    /// ui.split_left_right(&mut left, |ui, _| { ui.label("Pane"); }).label("Resize the list");
    /// # });
    /// ```
    pub fn label(self, name: impl Into<String>) -> Self {
        self.h.tree.info[self.bar].label = Some(name.into());
        self
    }

    /// Takes the remaining space (usually the rest of the window's height). See [`Handle::grow`].
    ///
    /// ```
    /// # let mut left = 200.0;
    /// # kargui::test::example(|ui| {
    /// ui.split_left_right(&mut left, |ui, _| { ui.label("Pane"); }).grow();
    /// # });
    /// ```
    pub fn grow(self) -> Self {
        Self { h: self.h.grow(), ..self }
    }

    /// Sets the height of both panes. See [`Handle::height`].
    ///
    /// ```
    /// use kargui::Size;
    /// # let mut left = 200.0;
    /// # kargui::test::example(|ui| {
    /// ui.split_left_right(&mut left, |ui, _| { ui.label("Pane"); }).height(Size::Fixed(300.0));
    /// # });
    /// ```
    pub fn height(self, size: Size) -> Self {
        Self { h: self.h.height(size), ..self }
    }

    /// Whether the divider moved in this frame.
    ///
    /// ```
    /// # let mut left = 200.0;
    /// # kargui::test::example(|ui| {
    /// if ui.split_left_right(&mut left, |ui, _| { ui.label("Pane"); }).changed() {
    ///     // save the layout
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// 自由に描く面に描く物（座標は面の左上からの論理ピクセル）
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DrawOp {
    Rect { r: Rect, color: Color, radius: f32 },
    /// 太さのある折れ線（端と角は丸い）
    Line { points: Vec<(f32, f32)>, width: f32, color: Color },
    Polygon { points: Vec<(f32, f32)>, color: Color },
    Text { at: (f32, f32), text: String, color: Color },
    Image { r: Rect, image: Image },
}

/// What to draw on a [`Ui::canvas`]. Coordinates are logical pixels from the surface's top left
/// corner; later shapes are drawn over earlier ones, and nothing is drawn outside the surface.
///
/// ```
/// use kargui::{Rect, rgb};
/// # kargui::test::example(|ui| {
/// ui.canvas(200.0, 100.0, |d| {
///     d.rect(Rect::new(0.0, 0.0, 200.0, 100.0), rgb(0xf4f4f4));
///     d.line((10.0, 90.0), (190.0, 10.0), 2.0, rgb(0x2060c0));
///     d.text((10.0, 10.0), "Sales", rgb(0x000000));
/// });
/// # });
/// ```
pub struct Drawing {
    ops: Vec<DrawOp>,
}

impl Drawing {
    /// A filled rectangle.
    ///
    /// ```
    /// use kargui::{Rect, rgb};
    /// # kargui::test::example(|ui| {
    /// ui.canvas(100.0, 100.0, |d| d.rect(Rect::new(10.0, 10.0, 80.0, 40.0), rgb(0xe04040)));
    /// # });
    /// ```
    pub fn rect(&mut self, r: Rect, color: Color) {
        self.ops.push(DrawOp::Rect { r, color, radius: 0.0 });
    }

    /// A filled rectangle with rounded corners.
    ///
    /// ```
    /// use kargui::{Rect, rgb};
    /// # kargui::test::example(|ui| {
    /// ui.canvas(100.0, 100.0, |d| d.rounded_rect(Rect::new(10.0, 10.0, 80.0, 40.0), 8.0, rgb(0x4040e0)));
    /// # });
    /// ```
    pub fn rounded_rect(&mut self, r: Rect, radius: f32, color: Color) {
        self.ops.push(DrawOp::Rect { r, color, radius });
    }

    /// A filled circle.
    ///
    /// ```
    /// use kargui::rgb;
    /// # kargui::test::example(|ui| {
    /// ui.canvas(100.0, 100.0, |d| d.circle((50.0, 50.0), 20.0, rgb(0x40a040)));
    /// # });
    /// ```
    pub fn circle(&mut self, center: (f32, f32), radius: f32, color: Color) {
        let r = Rect::new(center.0 - radius, center.1 - radius, 2.0 * radius, 2.0 * radius);
        self.ops.push(DrawOp::Rect { r, color, radius });
    }

    /// A straight line `width` pixels thick, with round ends.
    ///
    /// ```
    /// use kargui::rgb;
    /// # kargui::test::example(|ui| {
    /// ui.canvas(100.0, 100.0, |d| d.line((0.0, 0.0), (100.0, 100.0), 2.0, rgb(0x000000)));
    /// # });
    /// ```
    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), width: f32, color: Color) {
        self.ops.push(DrawOp::Line { points: vec![from, to], width, color });
    }

    /// Lines through `points` (a chart's line, a free-hand stroke), with round ends and joins.
    ///
    /// ```
    /// use kargui::rgb;
    /// # kargui::test::example(|ui| {
    /// let points = [(0.0, 80.0), (40.0, 30.0), (80.0, 60.0), (120.0, 10.0)];
    /// ui.canvas(120.0, 90.0, |d| d.polyline(&points, 2.0, rgb(0x2060c0)));
    /// # });
    /// ```
    pub fn polyline(&mut self, points: &[(f32, f32)], width: f32, color: Color) {
        if points.len() >= 2 {
            self.ops.push(DrawOp::Line { points: points.to_vec(), width, color });
        }
    }

    /// A filled shape with corners at `points` (closed from the last point back to the first).
    ///
    /// ```
    /// use kargui::rgb;
    /// # kargui::test::example(|ui| {
    /// ui.canvas(100.0, 100.0, |d| d.polygon(&[(50.0, 10.0), (90.0, 90.0), (10.0, 90.0)], rgb(0xf0a020)));
    /// # });
    /// ```
    pub fn polygon(&mut self, points: &[(f32, f32)], color: Color) {
        if points.len() >= 3 {
            self.ops.push(DrawOp::Polygon { points: points.to_vec(), color });
        }
    }

    /// Text in the body size, with its top left corner at `at`.
    ///
    /// ```
    /// use kargui::rgb;
    /// # kargui::test::example(|ui| {
    /// ui.canvas(100.0, 40.0, |d| d.text((4.0, 4.0), "42 °C", rgb(0x000000)));
    /// # });
    /// ```
    pub fn text(&mut self, at: (f32, f32), text: impl Into<String>, color: Color) {
        self.ops.push(DrawOp::Text { at, text: text.into(), color });
    }

    /// An image stretched to `r` (drawn by the CPU renderer; the Direct3D 11 renderer does not
    /// draw images yet).
    ///
    /// ```
    /// use kargui::{Image, Rect};
    /// # let photo = Image::from_rgba(2, 2, vec![255; 16]).unwrap();
    /// # kargui::test::example(|ui| {
    /// ui.canvas(100.0, 100.0, |d| d.image(Rect::new(0.0, 0.0, 100.0, 100.0), &photo));
    /// # });
    /// ```
    pub fn image(&mut self, r: Rect, image: &Image) {
        self.ops.push(DrawOp::Image { r, image: image.clone() });
    }
}

/// A drawing surface. See [`Ui::canvas`].
///
/// ```
/// # let mut last = None;
/// # kargui::test::example(|ui| {
/// let canvas = ui.canvas(100.0, 100.0, |_| {}).label("Pad");
/// if let Some(at) = canvas.clicked_at() {
///     last = Some(at);
/// }
/// # });
/// ```
pub struct CanvasView<'t> {
    h: Handle<'t>,
    pointer: Option<(f32, f32)>,
    clicked_at: Option<(f32, f32)>,
}

common!(CanvasView, "", "canvas(100.0, 50.0, |d| d.circle((50.0, 25.0), 10.0, kargui::rgb(0x000000)))");

impl CanvasView<'_> {
    /// Where the pointer is while the surface is pressed (also outside it, while dragging), in
    /// the surface's coordinates. `None` when it is not pressed.
    ///
    /// ```
    /// # let mut stroke: Vec<(f32, f32)> = Vec::new();
    /// # kargui::test::example(|ui| {
    /// if let Some(at) = ui.canvas(200.0, 200.0, |_| {}).pointer() {
    ///     stroke.push(at);
    /// }
    /// # });
    /// ```
    pub fn pointer(&self) -> Option<(f32, f32)> {
        self.pointer
    }

    /// Where the surface was clicked in this frame (pressed and released on it), in its
    /// coordinates.
    ///
    /// ```
    /// # let mut marks: Vec<(f32, f32)> = Vec::new();
    /// # kargui::test::example(|ui| {
    /// if let Some(at) = ui.canvas(200.0, 200.0, |_| {}).clicked_at() {
    ///     marks.push(at);
    /// }
    /// # });
    /// ```
    pub fn clicked_at(&self) -> Option<(f32, f32)> {
        self.clicked_at
    }
}

/// 木の表示の 1 行（キーボードで移るために、作った順に覚える）
struct TreeRow<K> {
    idx: usize,
    id: Id,
    key: K,
    text: String,
    /// 親の行（rows の中の番号）
    parent: Option<usize>,
    /// 枝なら開いているか。葉は None
    open: Option<bool>,
}

/// Builds the items of a [`Ui::tree`].
///
/// ```
/// # let mut selected: Option<u32> = None;
/// # kargui::test::example(|ui| {
/// ui.tree(&mut selected, |t| {
///     t.branch(1, "Fruit", |t| {
///         t.leaf(2, "Apple");
///         t.leaf(3, "Pear");
///     });
/// });
/// # });
/// ```
pub struct TreeBuilder<'m, K> {
    ui: Ui<'m>,
    tree: Id,
    depth: usize,
    parent: Option<usize>,
    rows: Vec<TreeRow<K>>,
    selected: Option<K>,
    picked: Option<K>,
}

impl<'m, K: Copy + PartialEq + Hash> TreeBuilder<'m, K> {
    /// An item without children.
    ///
    /// ```
    /// # let mut selected = None;
    /// # kargui::test::example(|ui| {
    /// ui.tree(&mut selected, |t| t.leaf(1, "Inbox"));
    /// # });
    /// ```
    pub fn leaf(&mut self, key: K, text: impl Into<String>) {
        self.row(key, text.into(), None);
    }

    /// An item with children, built in `f` only while it is open (closed at first).
    ///
    /// ```
    /// # let mut selected = None;
    /// # kargui::test::example(|ui| {
    /// ui.tree(&mut selected, |t| {
    ///     t.branch("mail", "Mail", |t| {
    ///         t.leaf("inbox", "Inbox");
    ///         t.leaf("sent", "Sent");
    ///     });
    /// });
    /// # });
    /// ```
    pub fn branch(&mut self, key: K, text: impl Into<String>, f: impl FnOnce(&mut TreeBuilder<'m, K>)) {
        let rid = Id(hash_of((self.tree.0, key)));
        let open = *self.ui.cx.expanded.entry(rid).or_insert(false);
        let k = self.row(key, text.into(), Some(open));
        if self.ui.cx.expanded.get(&rid).copied().unwrap_or(false) {
            let (depth, parent) = (self.depth, self.parent);
            self.depth += 1;
            self.parent = Some(k);
            f(self);
            self.depth = depth;
            self.parent = parent;
        }
    }

    fn row(&mut self, key: K, text: String, open: Option<bool>) -> usize {
        let rid = Id(hash_of((self.tree.0, key)));
        let selected = self.selected == Some(key);
        let depth = self.depth;
        let (idx, _) = self.ui.push(rid, Kind::TreeItem { text: text.clone(), depth, open, selected }, Dir::Leaf);
        let cx = &mut *self.ui.cx;
        if !self.ui.disabled && cx.clicked == Some(rid) {
            self.picked = Some(key);
            // 枝は、Enter・Space か、左の印（シェブロン）を押した時に開閉する
            if let Some(o) = open {
                let scale = cx.scale;
                let on_arrow = match (cx.pointer, cx.prev.hits.iter().find(|h| h.id == rid)) {
                    (Some((x, _)), Some(h)) => x < h.rect.x + ((8.0 + 20.0 * depth as f32 + 20.0) * scale).round(),
                    _ => false,
                };
                if cx.clicked_by_key || on_arrow {
                    cx.expanded.insert(rid, !o);
                }
            }
        }
        self.rows.push(TreeRow { idx, id: rid, key, text, parent: self.parent, open });
        self.rows.len() - 1
    }
}

/// A tree. See [`Ui::tree`].
///
/// ```
/// # let mut selected = None;
/// # kargui::test::example(|ui| {
/// if ui.tree(&mut selected, |t| t.leaf(1, "Inbox")).changed() {
///     // show the selected folder
/// }
/// # });
/// ```
pub struct TreeView<'t> {
    h: Handle<'t>,
    changed: bool,
}

common!(TreeView, " # let mut sel = None;", "tree(&mut sel, |t| t.leaf(1, \"Inbox\"))");

impl TreeView<'_> {
    /// Whether another item was selected in this frame.
    ///
    /// ```
    /// # let mut selected = None;
    /// # kargui::test::example(|ui| {
    /// if ui.tree(&mut selected, |t| t.leaf(1, "Inbox")).changed() {
    ///     // react to the selection
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// A column of a [`Ui::table`]: its heading, width and alignment.
///
/// ```
/// use kargui::ui::TableColumn;
/// let name = TableColumn::new("Name").width(200.0);
/// let size = TableColumn::new("Size").width(80.0).right();
/// let notes = TableColumn::new("Notes").fill();
/// # let _ = (name, size, notes);
/// ```
#[derive(Clone, Debug)]
pub struct TableColumn {
    title: String,
    /// 幅（論理ピクセル）。None なら残りの幅
    width: Option<f32>,
    right: bool,
}

impl TableColumn {
    /// A column with this heading, 120 logical pixels wide.
    ///
    /// ```
    /// let column = kargui::ui::TableColumn::new("Name");
    /// # let _ = column;
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), width: Some(120.0), right: false }
    }

    /// The starting width in logical pixels (the user can drag it wider or narrower).
    ///
    /// ```
    /// let column = kargui::ui::TableColumn::new("Name").width(240.0);
    /// # let _ = column;
    /// ```
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width.max(1.0));
        self
    }

    /// Takes the width that is left (usually the last column). It cannot be dragged.
    ///
    /// ```
    /// let column = kargui::ui::TableColumn::new("Notes").fill();
    /// # let _ = column;
    /// ```
    pub fn fill(mut self) -> Self {
        self.width = None;
        self
    }

    /// Puts the heading and the cells at the right of the column (for numbers).
    ///
    /// ```
    /// let column = kargui::ui::TableColumn::new("Price").right();
    /// # let _ = column;
    /// ```
    pub fn right(mut self) -> Self {
        self.right = true;
        self
    }
}

/// 表の升と見出しの、内側の左右の余白（論理ピクセル）
pub(crate) const CELL_PAD: f32 = 8.0;

/// 列の幅を変えるつまみの幅と、列の狭さの下限（論理ピクセル）
const GRIP_W: f32 = 8.0;
const MIN_COLUMN_W: f32 = 40.0;

/// A table being set up. See [`Ui::table`].
///
/// ```
/// use kargui::ui::TableColumn;
/// # kargui::test::example(|ui| {
/// let columns = [TableColumn::new("Item").fill()];
/// ui.table(&columns, 3, 28.0).show(|row, i| {
///     row.text(format!("Item {i}"));
/// });
/// # });
/// ```
pub struct Table<'u, 'a> {
    ui: &'u mut Ui<'a>,
    loc: &'static std::panic::Location<'static>,
    columns: &'u [TableColumn],
    count: usize,
    row_h: f32,
    sort: Option<&'u mut Option<(usize, bool)>>,
    selection: Option<&'u mut Option<usize>>,
    label: Option<String>,
    grow: bool,
}

impl<'u> Table<'u, '_> {
    /// Makes the headings sortable: clicking a heading (or Enter or Space on it) sets `*sort` to
    /// `Some((column, true))` (ascending), and again to `Some((column, false))`. Sorting the rows is
    /// up to you; the heading shows an arrow and screen readers hear the order.
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # let mut names = vec!["b", "a"];
    /// let mut sort: Option<(usize, bool)> = None;
    /// # kargui::test::example(|ui| {
    /// if let Some((0, ascending)) = sort {
    ///     names.sort();
    ///     if !ascending {
    ///         names.reverse();
    ///     }
    /// }
    /// let columns = [TableColumn::new("Name").fill()];
    /// ui.table(&columns, names.len(), 28.0).sort(&mut sort).show(|row, i| row.text(names[i]));
    /// # });
    /// ```
    pub fn sort(mut self, sort: &'u mut Option<(usize, bool)>) -> Self {
        self.sort = Some(sort);
        self
    }

    /// Makes rows selectable, as in [`List::selection`].
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # let mut selected = None;
    /// # kargui::test::example(|ui| {
    /// let columns = [TableColumn::new("Name").fill()];
    /// ui.table(&columns, 3, 28.0).selection(&mut selected).show(|row, i| row.text(format!("Row {i}")));
    /// # });
    /// ```
    pub fn selection(mut self, selected: &'u mut Option<usize>) -> Self {
        self.selection = Some(selected);
        self
    }

    /// The table's name for screen readers.
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # kargui::test::example(|ui| {
    /// let columns = [TableColumn::new("Name").fill()];
    /// ui.table(&columns, 1, 28.0).label("Contacts").show(|row, _| row.text("Aiko"));
    /// # });
    /// ```
    pub fn label(mut self, name: impl Into<String>) -> Self {
        self.label = Some(name.into());
        self
    }

    /// Takes the remaining space. See [`Handle::grow`].
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # kargui::test::example(|ui| {
    /// let columns = [TableColumn::new("Name").fill()];
    /// ui.table(&columns, 100, 28.0).grow().show(|row, i| row.text(format!("Row {i}")));
    /// # });
    /// ```
    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }

    /// Builds the visible rows: `row` is called with a [`TableRow`] and the row's index, and adds
    /// one cell per column in order. Returns the list's id.
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # kargui::test::example(|ui| {
    /// let columns = [TableColumn::new("Name"), TableColumn::new("Done").fill()];
    /// ui.table(&columns, 2, 28.0).show(|row, i| {
    ///     row.text(format!("Task {i}"));
    ///     row.cell(|ui| {
    ///         ui.label(if i == 0 { "Yes" } else { "No" });
    ///     });
    /// });
    /// # });
    /// ```
    pub fn show(self, row: impl FnMut(&mut TableRow, usize)) -> Id {
        self.show_keyed(|i| i, row)
    }

    /// Like [`Table::show`], with rows told apart by `key(index)`, as in [`List::show_keyed`].
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # let ids = [7u32, 3];
    /// # kargui::test::example(|ui| {
    /// let columns = [TableColumn::new("Id").fill()];
    /// ui.table(&columns, ids.len(), 28.0).show_keyed(|i| ids[i], |row, i| row.text(ids[i].to_string()));
    /// # });
    /// ```
    pub fn show_keyed<K: Hash>(self, key: impl Fn(usize) -> K, mut row: impl FnMut(&mut TableRow, usize)) -> Id {
        let Table { ui, loc, columns, count, row_h, mut sort, selection, label, grow } = self;
        let id = ui.id_here(loc);
        let parent_dir = ui.parent_dir();
        let (idx, _) = ui.push(id, Kind::Table { columns: columns.len(), rows: count }, Dir::Column);
        ui.tree.info[idx].label = label;
        ui.tree.layout[idx].align = Align::Stretch;
        if grow {
            match parent_dir {
                Dir::Column => ui.tree.layout[idx].height = Size::Fill(1.0),
                _ => ui.tree.layout[idx].width = Size::Fill(1.0),
            }
        }
        let scale = ui.cx.scale;
        let grip_w = (GRIP_W * scale).round();
        // 列の幅（論理ピクセル）。つまみをつかんで動かすと変わり、kargui が覚える
        let mut widths: Vec<Option<f32>> = Vec::with_capacity(columns.len());
        for (k, c) in columns.iter().enumerate() {
            let Some(start) = c.width else {
                widths.push(None);
                continue;
            };
            let grip = Id(hash_of((id.0, k, "grip")));
            let cx = &mut *ui.cx;
            let mut w = *cx.column_widths.entry((id, k)).or_insert(start);
            if !ui.disabled && (cx.pressed == Some(grip) || cx.released == Some(grip)) {
                if let (Some((px, _)), Some(h)) = (cx.pointer, cx.prev.hits.iter().find(|h| h.id == grip)) {
                    w = (w + (px - (h.rect.x + h.rect.w / 2.0)) / scale).round().max(MIN_COLUMN_W);
                    cx.column_widths.insert((id, k), w);
                }
            }
            widths.push(Some(w));
        }
        let px_widths: Vec<Option<f32>> = widths.iter().map(|w| w.map(|w| (w * scale).round())).collect();
        let rights: Vec<bool> = columns.iter().map(|c| c.right).collect();
        // 見出しの行: 見出しと、その右の幅を変えるつまみ（行の升の間の 8 の所に置く）
        let sortable = sort.is_some();
        let current = sort.as_deref().copied().flatten();
        let mut heads = Vec::with_capacity(columns.len());
        {
            let mut tui = ui.child(idx);
            let head_id = Id(hash_of((id.0, "head")));
            let (hr, _) = tui.push(head_id, Kind::TableHeader, Dir::Row);
            let l = &mut tui.tree.layout[hr];
            l.pad = ((ROW_PAD * scale).round(), 0.0);
            l.height = Size::Fixed((32.0 * scale).round());
            l.align = Align::Stretch;
            let mut hui = tui.child(hr);
            for (k, c) in columns.iter().enumerate() {
                let hid = Id(hash_of((id.0, k, "heading")));
                let dir = current.filter(|&(col, _)| col == k).map(|(_, asc)| asc);
                let (h, _) = hui.push(hid, Kind::ColumnHeader { text: c.title.clone(), sort: dir, right: c.right, sortable, column: k }, Dir::Leaf);
                hui.tree.layout[h].width = px_widths[k].map_or(Size::Fill(1.0), Size::Fixed);
                hui.tree.info[h].skip_tab = k != 0;
                heads.push(hid);
                if sortable && !hui.disabled && hui.cx.clicked == Some(hid) {
                    if let Some(s) = sort.as_deref_mut() {
                        *s = Some((k, !matches!(*s, Some((col, true)) if col == k)));
                    }
                }
                if px_widths[k].is_some() {
                    let gid = Id(hash_of((id.0, k, "grip")));
                    let (g, _) = hui.push(gid, Kind::ColumnGrip, Dir::Leaf);
                    hui.tree.layout[g].width = Size::Fixed(grip_w);
                    hui.tree.info[g].label = Some("Resize column".to_string());
                }
            }
            // 見出しに焦点がある時: 左右・Home・End で見出しを移る
            let cx = &mut *hui.cx;
            if let Some(at) = cx.focus.and_then(|f| heads.iter().position(|&h| h == f)) {
                let n = heads.len();
                let mut next = at;
                for e in std::mem::take(&mut cx.keys) {
                    if let Event::Key { key, mods } = e {
                        if mods.ctrl || mods.alt {
                            continue;
                        }
                        next = match key {
                            Key::Right => (next + 1).min(n - 1),
                            Key::Left => next.saturating_sub(1),
                            Key::Home => 0,
                            Key::End => n - 1,
                            _ => next,
                        };
                    }
                }
                if next != at {
                    cx.focus = Some(heads[next]);
                    cx.focus_visible = true;
                }
            }
        }
        // 行: 一覧（見えている行だけ作る）
        let mut tui = ui.child(idx);
        let mut list = tui.list(count, row_h);
        if let Kind::List { table, .. } = &mut list.ui.tree.info[list.idx].kind {
            *table = true;
        }
        if grow {
            list = list.grow();
        } else {
            let shown = (count.min(8) as f32 * list.row_h).max(list.row_h);
            list.ui.tree.layout[list.idx].height = Size::Fixed(shown);
        }
        if let Some(s) = selection {
            list = list.selection(s);
        }
        list.show_keyed(key, |rui, i| {
            let mut tr = TableRow { ui: rui, widths: &px_widths, rights: &rights, col: 0 };
            row(&mut tr, i);
        })
    }
}

/// One row of a [`Ui::table`], to add its cells to in column order.
///
/// ```
/// use kargui::ui::TableColumn;
/// # kargui::test::example(|ui| {
/// let columns = [TableColumn::new("City"), TableColumn::new("Population").fill().right()];
/// ui.table(&columns, 1, 28.0).show(|row, _| {
///     row.text("Osaka");
///     row.text("2,750,000");
/// });
/// # });
/// ```
pub struct TableRow<'r, 'a> {
    ui: &'r mut Ui<'a>,
    widths: &'r [Option<f32>],
    rights: &'r [bool],
    col: usize,
}

impl TableRow<'_, '_> {
    /// The next cell, built in `f` (any widgets: a check box, a button, a progress bar).
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # let mut done = [false, true];
    /// # kargui::test::example(|ui| {
    /// let columns = [TableColumn::new("Done").width(60.0), TableColumn::new("Task").fill()];
    /// ui.table(&columns, 2, 30.0).show(|row, i| {
    ///     row.cell(|ui| {
    ///         ui.checkbox(&mut done[i], "").label(format!("Task {i} done"));
    ///     });
    ///     row.text(format!("Task {i}"));
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn cell(&mut self, f: impl FnOnce(&mut Ui)) {
        let k = self.col;
        self.col += 1;
        let (c, _) = self.ui.add(Kind::Column, Dir::Column);
        let pad = (CELL_PAD * self.ui.cx.scale).round();
        let l = &mut self.ui.tree.layout[c];
        l.width = self.widths.get(k).copied().flatten().map_or(Size::Fill(1.0), Size::Fixed);
        // 升の内側の左右の余白（右寄せの列と左寄せの列が詰まらないように。見出しも同じだけ内側に描く）
        l.pad = (pad, 0.0);
        l.align = if self.rights.get(k).copied().unwrap_or(false) { Align::End } else { Align::Start };
        f(&mut self.ui.child(c));
    }

    /// The next cell, with text.
    ///
    /// ```
    /// use kargui::ui::TableColumn;
    /// # kargui::test::example(|ui| {
    /// let columns = [TableColumn::new("Name").fill()];
    /// ui.table(&columns, 1, 28.0).show(|row, _| row.text("Aiko"));
    /// # });
    /// ```
    #[track_caller]
    pub fn text(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.cell(|ui| {
            ui.label(text);
        });
    }
}

/// A date field. See [`Ui::date_picker`].
///
/// ```
/// # let mut day = None;
/// # kargui::test::example(|ui| {
/// if ui.date_picker(&mut day).label("Start").changed() {
///     // react to the new date
/// }
/// # });
/// ```
pub struct DatePicker<'t> {
    h: Handle<'t>,
    changed: bool,
}

common!(DatePicker, " # let mut day = None;", "date_picker(&mut day)");

impl DatePicker<'_> {
    /// Whether a date was picked in this frame.
    ///
    /// ```
    /// # let mut day = None;
    /// # kargui::test::example(|ui| {
    /// if ui.date_picker(&mut day).changed() {
    ///     // react to the new date
    /// }
    /// # });
    /// ```
    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// 入力欄の種類（edit の振る舞いを変える）
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Line,
    Area,
    Password,
}

/// 複数行の入力欄で、上下の矢印と PageUp/PageDown の行き先（UTF-8 の位置）。行が変わらなければ None。
/// 横の位置は、続けて上下に動く間は最初の位置（goal_x）を保つ
fn vertical_move(laid: &crate::text::Laid, s: &str, st: &mut edit::EditState, key: Key, view_h: f32) -> Option<usize> {
    let cur = edit::utf16_of(s, st.cursor);
    let (x, top, h) = laid.caret(cur);
    let gx = *st.goal_x.get_or_insert(x);
    let (first, last) = (*laid.lines.first()?, *laid.lines.last()?);
    let y = match key {
        Key::Up => top - h * 0.5,
        Key::Down => top + h * 1.5,
        Key::PageUp => top + h * 0.5 - view_h.max(h),
        _ => top + h * 0.5 + view_h.max(h),
    };
    let y = y.clamp(first.top + first.height * 0.5, last.top + last.height * 0.5);
    let from = edit::line_of(&laid.lines, cur);
    let to = laid.lines.iter().rposition(|l| l.top <= y).unwrap_or(0);
    if to == from {
        return None;
    }
    Some(edit::byte_of(s, laid.index_at_point(gx, y)))
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
        let disabled = ui.disabled;
        if let Some(out) = selection.as_deref_mut() {
            if let Some(&(list, i)) = cx.clicked.filter(|_| !disabled).and_then(|c| cx.prev.items.get(&c)) {
                if list == id && i < count {
                    sel = Some(i);
                }
            }
            let focused = cx.focus.filter(|_| !disabled).and_then(|f| cx.prev.items.get(&f)).filter(|(l, _)| *l == id).map(|&(_, i)| i);
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
        Kind::Segment { text, .. } | Kind::Radio { text, .. } | Kind::Expander { text, .. } | Kind::Link { text } | Kind::ToggleButton { text, .. } => Some(text),
        Kind::Dropdown { text, .. } | Kind::ListOption { text, .. } => Some(text),
        Kind::MenuButton { text, .. } | Kind::MenuBarItem { text, .. } | Kind::MenuItem { text, .. } | Kind::Tab { text, .. } => Some(text),
        Kind::TreeItem { text, .. } | Kind::ColumnHeader { text, .. } => Some(text),
        _ => None,
    }
}
