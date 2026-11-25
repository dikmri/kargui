//! ドッキングの並べ方の木（アプリが持つ）。分割の木の葉がタブのまとまりで、まとまりは番号ではなく中のパネルで指す
//! （編集しても指す先がずれない）。空のまとまりは作らず、最後のパネルが出たら親の分割ごと畳む。
//! 変更はすべて Dock::insert を通す（マウスで落とした時・メニュー・キーボードが同じ形を作る）。
//! 後半は、タブを引っ張って落とす先の計算（描いた結果の場所から、十字の的・外側の的・タブの帯の間を決める）。

use crate::geom::Rect;
use crate::ui::Id;

/// Which side of a panel group, or of the whole dock, a panel goes to.
///
/// ```
/// use kargui::DockSide;
///
/// assert_ne!(DockSide::Left, DockSide::Right);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockSide {
    /// To the left.
    Left,
    /// To the right.
    Right,
    /// Above.
    Top,
    /// Below.
    Bottom,
}

impl DockSide {
    /// 上下に分けるか（false は左右）
    fn stacked(self) -> bool {
        matches!(self, DockSide::Top | DockSide::Bottom)
    }
    /// 新しいパネルが分割の 1 つ目（左か上）に入るか
    fn first(self) -> bool {
        matches!(self, DockSide::Left | DockSide::Top)
    }
}

/// Where [`Dock::insert`] puts a panel.
///
/// Groups are named by a panel in them, so a place stays right while other panels move.
///
/// ```
/// use kargui::{Dock, DockSide, DockTo};
///
/// let mut dock = Dock::new(["Files"]);
/// dock.insert("Editor", DockTo::Split { of: "Files", side: DockSide::Right, share: 0.7 });
/// dock.insert("Search", DockTo::Tab { with: "Files", index: None });
/// assert_eq!(dock.panels(), vec![&"Files", &"Search", &"Editor"]);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum DockTo<K> {
    /// As a tab in the group that holds `with`, before the tab at `index` (counted as the tabs
    /// are shown before the move), or after the last one when `index` is `None`.
    Tab {
        /// A panel in the group.
        with: K,
        /// The position among the group's tabs.
        index: Option<usize>,
    },
    /// Next to the group that holds `of`, splitting its space: `share` (0.1 to 0.9) is the part
    /// the new panel gets. A floating group is not split.
    Split {
        /// A panel in the group to split.
        of: K,
        /// Which side of that group.
        side: DockSide,
        /// The new panel's part of the space.
        share: f32,
    },
    /// Along a side of the whole dock: `share` (0.1 to 0.9) is the part the new panel gets.
    Edge {
        /// Which side of the dock.
        side: DockSide,
        /// The new panel's part of the space.
        share: f32,
    },
    /// In a floating group of its own, over the other groups. Its top-left corner is `x` and
    /// `y` from the dock's top-left corner, and its size is `w` by `h`, in logical pixels
    /// (at least 120 by 80; it is kept inside the dock when shown). The tabs' menu puts it
    /// back where it was before.
    Float {
        /// From the dock's left edge.
        x: f32,
        /// From the dock's top edge.
        y: f32,
        /// The width.
        w: f32,
        /// The height.
        h: f32,
    },
    /// In a window of its own: its top-left corner (the outside of the window) is at `x` and
    /// `y` on the screen, in the units of [`Options::position`](crate::Options::position), and
    /// the inside of the window is `w` by `h` logical pixels (at least 120 by 80). The app
    /// builds the window with [`Ui::dock`](crate::Ui::dock) in
    /// [`App::window`](crate::App::window) when [`Dock::has_window`] says the window is the
    /// dock's. Where kargui cannot place windows (Wayland), the group floats inside the dock
    /// instead.
    Window {
        /// From the screen's left edge.
        x: f32,
        /// From the screen's top edge.
        y: f32,
        /// The width of the inside.
        w: f32,
        /// The height of the inside.
        h: f32,
    },
}

/// 木の節。葉はタブのまとまり、ほかは 2 つに分ける分割
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node<K> {
    Tabs { panels: Vec<K>, selected: usize },
    /// stacked は上下に分ける（false は左右）。share は 1 つ目（左か上）の割合
    Split { stacked: bool, share: f32, a: Box<Node<K>>, b: Box<Node<K>> },
}

/// 浮いたまとまり（タブのまとまり 1 つを、ほかのまとまりの上に重ねる）
#[derive(Clone, Debug)]
pub(crate) struct Floating<K> {
    /// いつも Node::Tabs
    pub tabs: Node<K>,
    /// 左上と大きさ（dock の左上からの論理ピクセル。整数に丸める）
    pub rect: [f32; 4],
    /// パネルごとの、浮いたまとまりに入る前にいた所（メニューの「Dock」で戻す。保存はしない）
    pub homes: Vec<(K, Home<K>)>,
    /// 浮かせた順の番号（F6 で回る順。上にしても変わらない。保存はせず、読み込んだ時は重なりの順）
    pub seq: u64,
    /// OS の別の窓に出しているか。出していれば rect は窓の外枠の左上（画面。Options::position と同じ形）と中の大きさ
    pub window: bool,
}

/// 戻る先と浮かせた順は覚えているだけなので比べない（保存して読み直した物と同じになるように）
impl<K: PartialEq> PartialEq for Floating<K> {
    fn eq(&self, other: &Self) -> bool {
        self.tabs == other.tabs && self.rect == other.rect && self.window == other.window
    }
}

/// 浮かせたパネルが戻る先。並べ方の木が変わっても使えるように、番号ではなくパネルで覚える
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Home<K> {
    /// with（元のまとまりのほかのパネル）のうち、木に残っている最初の物のまとまりの、index の前
    Tab { with: Vec<K>, index: Option<usize> },
    /// beside（隣にあった部分の木のパネル）のうち木に残っている物をすべて含む、いちばん小さい部分の side 側に share で
    Beside { beside: Vec<K>, side: DockSide, share: f32 },
}

/// 浮いたまとまりの、いちばん小さい大きさ（論理ピクセル）
pub(crate) const FLOAT_MIN: (f32, f32) = (120.0, 80.0);

/// The layout of a dock: panels in tab groups, groups side by side or one above another, and
/// floating groups over them. The app keeps it and shows it with [`Ui::dock`](crate::Ui::dock);
/// the user rearranges it by dragging tabs or with the tabs' menu. `K` is the app's name for a
/// panel, such as an enum.
///
/// ```
/// use kargui::{Dock, DockSide, DockTo};
///
/// #[derive(Clone, Copy, Debug, PartialEq)]
/// enum Panel { Files, Editor, Output }
///
/// let mut dock = Dock::new([Panel::Editor]);
/// dock.insert(Panel::Files, DockTo::Edge { side: DockSide::Left, share: 0.25 });
/// dock.insert(Panel::Output, DockTo::Split { of: Panel::Editor, side: DockSide::Bottom, share: 0.3 });
/// assert_eq!(dock.panels(), vec![&Panel::Files, &Panel::Editor, &Panel::Output]);
/// ```
#[derive(Clone, Debug)]
pub struct Dock<K> {
    pub(crate) root: Option<Node<K>>,
    /// 下にある物が先（最後がいちばん上）。別の窓に出したまとまりも同じ並びに持つ（window）
    pub(crate) floating: Vec<Floating<K>>,
    /// 主の窓で見せた部品の id（別の窓のキーに入れて、ほかの dock の窓と分ける。まだ見せていなければ 0。比べない）
    pub(crate) area: u64,
}

/// 主の窓の部品の id は覚えているだけなので比べない
impl<K: PartialEq> PartialEq for Dock<K> {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.floating == other.floating
    }
}

/// 割合を収める範囲（どちらの区画も消えないように）
const MIN_SHARE: f32 = 0.1;
const MAX_SHARE: f32 = 0.9;

impl<K: Clone + PartialEq> Dock<K> {
    /// A dock with one group holding `panels` as tabs (the first one selected). With no panels,
    /// the dock is empty.
    ///
    /// ```
    /// use kargui::Dock;
    ///
    /// let dock = Dock::new(["Files", "Search"]);
    /// assert_eq!(dock.panels(), vec![&"Files", &"Search"]);
    /// assert_eq!(dock.selected(), vec![&"Files"]);
    /// ```
    pub fn new(panels: impl IntoIterator<Item = K>) -> Self {
        let mut list: Vec<K> = Vec::new();
        for k in panels {
            if !list.contains(&k) {
                list.push(k);
            }
        }
        Dock { root: (!list.is_empty()).then_some(Node::Tabs { panels: list, selected: 0 }), floating: Vec::new(), area: 0 }
    }

    /// タブのまとまりを、並べ方の木（左・上が先）、浮いたまとまり（下にある物が先）の順に並べる。Ui::dock がまとまりを
    /// 数える順と同じ
    pub(crate) fn groups(&self) -> Vec<&Node<K>> {
        let mut out = Vec::new();
        if let Some(r) = &self.root {
            r.walk(&mut |n| {
                if matches!(n, Node::Tabs { .. }) {
                    out.push(n);
                }
            });
        }
        out.extend(self.floating.iter().map(|f| &f.tabs));
        out
    }

    /// Every panel, group by group: left before right, top before bottom, and tabs in order;
    /// then the floating groups, the one at the back first.
    ///
    /// ```
    /// use kargui::Dock;
    ///
    /// assert_eq!(Dock::new([1, 2]).panels(), vec![&1, &2]);
    /// ```
    pub fn panels(&self) -> Vec<&K> {
        let mut out = Vec::new();
        for n in self.groups() {
            if let Node::Tabs { panels, .. } = n {
                out.extend(panels.iter());
            }
        }
        out
    }

    /// The selected panel of each group, in the same order as [`Dock::panels`].
    ///
    /// ```
    /// use kargui::{Dock, DockSide, DockTo};
    ///
    /// let mut dock = Dock::new(["A", "B"]);
    /// dock.insert("C", DockTo::Split { of: "A", side: DockSide::Right, share: 0.5 });
    /// assert_eq!(dock.selected(), vec![&"A", &"C"]);
    /// ```
    pub fn selected(&self) -> Vec<&K> {
        let mut out = Vec::new();
        for n in self.groups() {
            if let Node::Tabs { panels, selected } = n {
                out.extend(panels.get(*selected));
            }
        }
        out
    }

    /// Whether `panel` is in the dock.
    ///
    /// ```
    /// use kargui::Dock;
    ///
    /// let dock = Dock::new(["Files"]);
    /// assert!(dock.contains(&"Files"));
    /// assert!(!dock.contains(&"Search"));
    /// ```
    pub fn contains(&self, panel: &K) -> bool {
        self.panels().contains(&panel)
    }

    /// Whether `panel` is in a floating group (over the other groups, or in a window of its own).
    ///
    /// ```
    /// use kargui::{Dock, DockTo};
    ///
    /// let mut dock = Dock::new(["Files", "Search"]);
    /// dock.insert("Search", DockTo::Float { x: 40.0, y: 40.0, w: 300.0, h: 200.0 });
    /// assert!(dock.is_floating(&"Search"));
    /// assert!(!dock.is_floating(&"Files"));
    /// ```
    pub fn is_floating(&self, panel: &K) -> bool {
        self.floating_of(panel).is_some()
    }

    /// panel を持つ浮いたまとまりの番号
    pub(crate) fn floating_of(&self, panel: &K) -> Option<usize> {
        self.floating.iter().position(|f| f.tabs.group_of(panel).is_some())
    }

    /// Selects `panel` in its group (shows its tab). Returns false if it is not in the dock.
    ///
    /// ```
    /// use kargui::Dock;
    ///
    /// let mut dock = Dock::new(["Files", "Search"]);
    /// assert!(dock.select(&"Search"));
    /// assert_eq!(dock.selected(), vec![&"Search"]);
    /// ```
    pub fn select(&mut self, panel: &K) -> bool {
        let mut found = false;
        let mut pick = |n: &mut Node<K>| {
            if let Node::Tabs { panels, selected } = n
                && let Some(i) = panels.iter().position(|p| p == panel)
            {
                *selected = i;
                found = true;
            }
        };
        if let Some(r) = &mut self.root {
            r.walk_mut(&mut pick);
        }
        for f in &mut self.floating {
            pick(&mut f.tabs);
        }
        found
    }

    /// Takes `panel` out of the dock. A group left without panels disappears, and its
    /// neighbour takes its space. Returns false if the panel was not in the dock.
    ///
    /// ```
    /// use kargui::{Dock, DockSide, DockTo};
    ///
    /// let mut dock = Dock::new(["Files"]);
    /// dock.insert("Editor", DockTo::Split { of: "Files", side: DockSide::Right, share: 0.7 });
    /// assert!(dock.remove(&"Files"));
    /// assert_eq!(dock.panels(), vec![&"Editor"]);
    /// assert!(!dock.remove(&"Files"));
    /// ```
    pub fn remove(&mut self, panel: &K) -> bool {
        if let Some(root) = self.root.take() {
            let (rest, found) = root.without(panel);
            self.root = rest;
            if found {
                return true;
            }
        }
        let Some(i) = self.floating_of(panel) else { return false };
        let f = self.floating.remove(i);
        if let (Some(tabs), _) = f.tabs.without(panel) {
            let homes = f.homes.into_iter().filter(|(k, _)| k != panel).collect();
            self.floating.insert(i, Floating { tabs, rect: f.rect, homes, seq: f.seq, window: f.window });
        }
        true
    }

    /// k を持つまとまりのパネルの並び（浮いたまとまりも探す）
    fn group_of(&self, k: &K) -> Option<&[K]> {
        self.root.as_ref().and_then(|r| r.group_of(k)).or_else(|| self.floating.iter().find_map(|f| f.tabs.group_of(k)))
    }

    /// Puts `panel` at `to` and selects it. If the panel is already in the dock, it moves
    /// there. A place named by a panel that is not in the dock (or, for a moving panel, a
    /// place named by the panel itself when it is alone in its group), and a split of a
    /// floating group, leave the dock as it is and return false. An empty dock takes any panel
    /// as its only group.
    ///
    /// ```
    /// use kargui::{Dock, DockSide, DockTo};
    ///
    /// let mut dock = Dock::new(["A", "B", "C"]);
    /// // Move A after C in the same group
    /// assert!(dock.insert("A", DockTo::Tab { with: "B", index: Some(3) }));
    /// assert_eq!(dock.panels(), vec![&"B", &"C", &"A"]);
    /// // Split C off to the bottom
    /// assert!(dock.insert("C", DockTo::Split { of: "B", side: DockSide::Bottom, share: 0.4 }));
    /// assert_eq!(dock.selected(), vec![&"A", &"C"]);
    /// ```
    pub fn insert(&mut self, panel: K, to: DockTo<K>) -> bool {
        if self.root.is_none() && self.floating.is_empty() {
            match to {
                DockTo::Float { x, y, w, h } => self.floating.push(Floating { tabs: Node::leaf(panel), rect: float_rect(x, y, w, h), homes: Vec::new(), seq: 0, window: false }),
                DockTo::Window { x, y, w, h } => self.floating.push(Floating { tabs: Node::leaf(panel), rect: float_rect(x, y, w, h), homes: Vec::new(), seq: 0, window: true }),
                _ => self.root = Some(Node::leaf(panel)),
            }
            return true;
        }
        // 指す先のまとまりを、動かすパネルを抜く前に決める（指す先が動かすパネル自身なら、同じまとまりのほかのパネル）
        let anchor = |k: &K| -> Option<K> {
            let group = self.group_of(k)?;
            if *k != panel {
                return Some(k.clone());
            }
            group.iter().find(|p| **p != panel).cloned()
        };
        let moving_from = self.group_of(&panel).map(|g| (g.to_vec(), g.iter().position(|p| *p == panel).unwrap_or(0)));
        match to {
            DockTo::Tab { with, index } => {
                let Some(a) = anchor(&with) else {
                    // 自分だけのまとまりの自分の隣: 動かさない（並べ替えるものが無い）
                    return self.group_of(&with).is_some() && with == panel;
                };
                // 同じまとまりの中で後ろへ動かす時は、抜いた分だけ前に詰める
                let index = match (&moving_from, index) {
                    (Some((g, from)), Some(i)) if g.contains(&a) && *from < i => Some(i - 1),
                    (_, i) => i,
                };
                // 浮いたまとまりへ入る時は、入る前にいた所を覚える（浮いたまとまりの中で動く時は、覚えている所のまま）
                let home = self.home_for(&panel);
                self.remove(&panel);
                if let Some(r) = &mut self.root
                    && r.group_of(&a).is_some()
                {
                    return r.put_tab(&a, panel, index);
                }
                let Some(f) = self.floating.iter_mut().find(|f| f.tabs.group_of(&a).is_some()) else { return false };
                if !f.tabs.put_tab(&a, panel.clone(), index) {
                    return false;
                }
                f.homes.extend(home.map(|h| (panel, h)));
                true
            }
            DockTo::Split { of, side, share } => {
                let Some(a) = anchor(&of) else { return false };
                // 浮いたまとまりは分けない（1 つの枠に 1 つのまとまり）
                if self.is_floating(&a) {
                    return false;
                }
                self.remove(&panel);
                let Some(r) = self.root.take() else { return false };
                let (r, ok) = r.split_at(&a, side, share.clamp(MIN_SHARE, MAX_SHARE), panel);
                self.root = Some(r);
                ok
            }
            DockTo::Edge { side, share } => {
                self.remove(&panel);
                let leaf = Node::leaf(panel);
                self.root = Some(match self.root.take() {
                    None => leaf,
                    Some(r) => Node::join(side, share.clamp(MIN_SHARE, MAX_SHARE), leaf, r),
                });
                true
            }
            DockTo::Float { x, y, w, h } => self.put_floating(panel, float_rect(x, y, w, h), false),
            DockTo::Window { x, y, w, h } => self.put_floating(panel, float_rect(x, y, w, h), true),
        }
    }

    /// panel だけの浮いたまとまりを作って、いちばん上に置く（window なら別の窓）。入る前にいた所を覚える
    fn put_floating(&mut self, panel: K, rect: [f32; 4], window: bool) -> bool {
        let homes = self.home_for(&panel).map(|h| (panel.clone(), h)).into_iter().collect();
        let seq = self.next_seq();
        self.remove(&panel);
        self.floating.push(Floating { tabs: Node::leaf(panel), rect, homes, seq, window });
        true
    }

    /// 窓に出したまとまり i の窓のキー（ほかの dock の窓と分けるため、主の窓の部品の id を入れる）
    pub(crate) fn window_key(&self, i: usize) -> String {
        format!("kargui-dock-{:x}-{}", self.area, self.floating[i].seq)
    }

    /// キーが key の、窓に出したまとまりの番号
    pub(crate) fn window_index(&self, key: &str) -> Option<usize> {
        (0..self.floating.len()).find(|&i| self.floating[i].window && self.window_key(i) == key)
    }

    /// Whether `key` names one of this dock's windows, so the app builds that window with
    /// [`Ui::dock`](crate::Ui::dock) in [`App::window`](crate::App::window).
    ///
    /// ```
    /// use kargui::{App, Dock, Ui};
    ///
    /// struct Ide {
    ///     dock: Dock<&'static str>,
    /// }
    ///
    /// impl Ide {
    ///     fn panels(&mut self, ui: &mut Ui) {
    ///         ui.dock(&mut self.dock).grow().show(|p| p.to_string(), |ui, p| {
    ///             ui.label(*p);
    ///         });
    ///     }
    /// }
    ///
    /// impl App for Ide {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         self.panels(ui);
    ///     }
    ///
    ///     fn window(&mut self, key: &str, ui: &mut Ui) {
    ///         // A group the user moved to a new window
    ///         if self.dock.has_window(key) {
    ///             self.panels(ui);
    ///         }
    ///     }
    /// }
    ///
    /// assert!(!Dock::new(["Files"]).has_window("settings"));
    /// ```
    pub fn has_window(&self, key: &str) -> bool {
        self.window_index(key).is_some()
    }

    /// 別の窓に出したまとまりを、全体の中の浮いたまとまりに戻す（窓を置けない所）。at は 1 つ目を置く場所と大きさで、
    /// 2 つ目からは少しずつずらす。戻した物があれば true
    pub(crate) fn windows_to_frames(&mut self, at: [f32; 4]) -> bool {
        let mut n = 0;
        for f in self.floating.iter_mut().filter(|f| f.window) {
            f.window = false;
            let d = 24.0 * n as f32;
            f.rect = float_rect(at[0] + d, at[1] + d, at[2], at[3]);
            n += 1;
        }
        n > 0
    }

    /// 浮いたまとまりのうち、全体の中に描く物（別の窓に出していない物）の番号。重なりの順
    pub(crate) fn frames(&self) -> Vec<usize> {
        (0..self.floating.len()).filter(|&i| !self.floating[i].window).collect()
    }

    /// 次に浮かせるまとまりの、浮かせた順の番号
    fn next_seq(&self) -> u64 {
        self.floating.iter().map(|f| f.seq + 1).max().unwrap_or(0)
    }

    /// 全体の中に描く浮いたまとまりの番号を、浮かせた順に並べる（F6 で回る順。上にしても変わらないので、回る途中で
    /// 飛ばさない。別の窓に出した物は、その窓の中で回る）
    pub(crate) fn float_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = self.frames();
        order.sort_by_key(|&i| self.floating[i].seq);
        order
    }

    /// panel が浮いたまとまりへ入る時に覚える戻る先。木にいれば今の場所（同じまとまりにほかの物がいればそのタブの位置、
    /// 1 つだけなら隣の部分の木の横）、浮いていれば覚えている戻る先
    fn home_for(&self, panel: &K) -> Option<Home<K>> {
        if let Some(r) = &self.root
            && let Some(g) = r.group_of(panel)
        {
            let with: Vec<K> = g.iter().filter(|p| *p != panel).cloned().collect();
            if with.is_empty() {
                return r.beside(panel);
            }
            return Some(Home::Tab { with, index: g.iter().position(|p| p == panel) });
        }
        let f = &self.floating[self.floating_of(panel)?];
        f.homes.iter().find(|(k, _)| k == panel).map(|(_, h)| h.clone())
    }

    /// 浮いたまとまりの panel を、浮いたまとまりに入る前にいた所へ戻す（その所が無くなっていたり、覚えていなかったり
    /// すれば全体の右の端）。浮いていなければ false。同じ浮いたまとまりに残ったパネルのうち、戻る先を覚えていない物は、
    /// この後は戻したパネルのまとまりへ戻る（別々のまとまりに分かれないように）
    pub(crate) fn dock_back(&mut self, panel: &K) -> bool {
        let Some(i) = self.floating_of(panel) else { return false };
        let alone = matches!(&self.floating[i].tabs, Node::Tabs { panels, .. } if panels.len() == 1);
        let home = self.home_for(panel);
        self.remove(panel);
        if !alone {
            let f = &mut self.floating[i];
            let rest: Vec<K> = f.tabs.all_panels().into_iter().filter(|k| !f.homes.iter().any(|(h, _)| h == k)).collect();
            f.homes.extend(rest.into_iter().map(|k| (k, Home::Tab { with: vec![panel.clone()], index: None })));
        }
        let Some(mut root) = self.root.take() else {
            self.root = Some(Node::leaf(panel.clone()));
            return true;
        };
        let in_tree = |k: &&K| root.group_of(k).is_some();
        let tab = match &home {
            Some(Home::Tab { with, index }) => with.iter().find(in_tree).map(|k| (k.clone(), *index)),
            _ => None,
        };
        let beside = match &home {
            Some(Home::Beside { beside, side, share }) => {
                let left: Vec<K> = beside.iter().filter(in_tree).cloned().collect();
                (!left.is_empty()).then_some((left, *side, *share))
            }
            _ => None,
        };
        self.root = Some(if let Some((k, index)) = tab {
            root.put_tab(&k, panel.clone(), index);
            root
        } else if let Some((left, side, share)) = beside {
            root.split_around(&left, side, share, panel.clone())
        } else {
            Node::join(DockSide::Right, EDGE_SHARE, Node::leaf(panel.clone()), root)
        });
        true
    }

    /// 浮いたまとまり i の場所と大きさ（論理ピクセル）を変える
    pub(crate) fn set_float_rect(&mut self, i: usize, rect: [f32; 4]) {
        if let Some(f) = self.floating.get_mut(i) {
            f.rect = float_rect(rect[0], rect[1], rect[2], rect[3]);
        }
    }

    /// 浮いたまとまり i をいちばん上（最後）にする。動いたら true
    pub(crate) fn raise(&mut self, i: usize) -> bool {
        if i + 1 >= self.floating.len() {
            return false;
        }
        let f = self.floating.remove(i);
        self.floating.push(f);
        true
    }

    /// 道（根が 1、子は 2p と 2p+1）の分割の割合を、0.1〜0.9 に収めて変える。分割でなければ false
    pub(crate) fn set_share(&mut self, path: u64, share: f32) -> bool {
        let Some(mut n) = self.root.as_mut() else { return false };
        let depth = 63 - path.leading_zeros();
        for i in (0..depth).rev() {
            let Node::Split { a, b, .. } = n else { return false };
            n = if (path >> i) & 1 == 0 { a } else { b };
        }
        if let Node::Split { share: s, .. } = n {
            *s = share.clamp(MIN_SHARE, MAX_SHARE);
            return true;
        }
        false
    }

    /// The layout as text, to keep between runs: `name` gives each panel's name (names must
    /// be different and should not contain line breaks). Read it back with [`Dock::load`].
    ///
    /// ```
    /// use kargui::{Dock, DockSide, DockTo};
    ///
    /// let mut dock = Dock::new(["Files"]);
    /// dock.insert("Editor", DockTo::Split { of: "Files", side: DockSide::Right, share: 0.7 });
    /// let text = dock.save(|p| p.to_string());
    /// assert_eq!(Dock::load(&text, |name| ["Files", "Editor"].into_iter().find(|p| *p == name)), Some(dock));
    /// ```
    pub fn save(&self, name: impl Fn(&K) -> String) -> String {
        let mut out = String::from("kargui-dock 1\n");
        if let Some(r) = &self.root {
            r.write(&mut out, &name);
        }
        for f in &self.floating {
            let [x, y, w, h] = f.rect;
            let kind = if f.window { "window" } else { "float" };
            out.push_str(&format!("{kind} {x} {y} {w} {h}\n"));
            f.tabs.write(&mut out, &name);
        }
        out
    }

    /// Reads a layout written by [`Dock::save`]. `panel` turns a saved name back into a panel;
    /// names it does not know (`None`) are left out, and groups left empty disappear. Returns
    /// `None` if the text is not a saved layout (or from a newer version of kargui).
    ///
    /// ```
    /// use kargui::Dock;
    ///
    /// let text = Dock::new(["Files", "Old panel"]).save(|p| p.to_string());
    /// let dock = Dock::load(&text, |name| (name == "Files").then_some("Files")).unwrap();
    /// assert_eq!(dock.panels(), vec![&"Files"]);
    /// assert_eq!(Dock::<&str>::load("not a layout", |_| None), None);
    /// ```
    pub fn load(text: &str, panel: impl Fn(&str) -> Option<K>) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()?.trim() != "kargui-dock 1" {
            return None;
        }
        let mut rest = lines.filter(|l| !l.trim().is_empty()).peekable();
        let mut seen = Vec::new();
        let mut dock = Dock { root: None, floating: Vec::new(), area: 0 };
        if rest.peek().is_some_and(|l| !l.starts_with("float ") && !l.starts_with("window ")) {
            dock.root = Node::read(&mut rest, &panel, &mut seen)?;
        }
        // 木の後は浮いたまとまりだけ（「float x y w h」か、別の窓の「window x y w h」の行と、タブのまとまり 1 つ）
        while let Some(line) = rest.next() {
            let (window, nums) = match line.strip_prefix("window ") {
                Some(n) => (true, n),
                None => (false, line.strip_prefix("float ")?),
            };
            let nums = nums.split_whitespace().map(|v| v.parse::<f32>().ok().filter(|v| v.is_finite())).collect::<Option<Vec<f32>>>()?;
            let [x, y, w, h] = nums[..] else { return None };
            match Node::read(&mut rest, &panel, &mut seen)? {
                Some(tabs @ Node::Tabs { .. }) => {
                    let seq = dock.floating.len() as u64;
                    dock.floating.push(Floating { tabs, rect: float_rect(x, y, w, h), homes: Vec::new(), seq, window });
                }
                Some(Node::Split { .. }) => return None,
                None => {}
            }
        }
        Some(dock)
    }
}

/// 浮いたまとまりの場所と大きさ（整数の論理ピクセルに丸め、いちばん小さい大きさより小さくしない。数でない物は 0）
fn float_rect(x: f32, y: f32, w: f32, h: f32) -> [f32; 4] {
    let n = |v: f32| if v.is_finite() { v.round() } else { 0.0 };
    [n(x), n(y), n(w).max(FLOAT_MIN.0), n(h).max(FLOAT_MIN.1)]
}

impl<K: Clone + PartialEq> Node<K> {
    /// 前から順に（左・上が先）すべての節をたどる
    pub(crate) fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node<K>)) {
        f(self);
        if let Node::Split { a, b, .. } = self {
            a.walk(f);
            b.walk(f);
        }
    }

    /// k だけのまとまり
    fn leaf(k: K) -> Node<K> {
        Node::Tabs { panels: vec![k], selected: 0 }
    }

    /// すべてのパネル（左・上が先）
    pub(crate) fn all_panels(&self) -> Vec<K> {
        let mut out = Vec::new();
        self.walk(&mut |n| {
            if let Node::Tabs { panels, .. } = n {
                out.extend(panels.iter().cloned());
            }
        });
        out
    }

    /// panel だけのまとまりの場所を、親の分割のもう片方（隣の部分の木）から見て表す
    fn beside(&self, panel: &K) -> Option<Home<K>> {
        let Node::Split { stacked, share, a, b } = self else { return None };
        let only = |n: &Node<K>| matches!(n, Node::Tabs { panels, .. } if panels.len() == 1 && panels[0] == *panel);
        if only(a) {
            let side = if *stacked { DockSide::Top } else { DockSide::Left };
            return Some(Home::Beside { beside: b.all_panels(), side, share: *share });
        }
        if only(b) {
            let side = if *stacked { DockSide::Bottom } else { DockSide::Right };
            return Some(Home::Beside { beside: a.all_panels(), side, share: 1.0 - *share });
        }
        if a.group_of(panel).is_some() { a.beside(panel) } else { b.beside(panel) }
    }

    /// set のパネル（どれも木の中にある）をすべて含む、いちばん小さい部分の木の side 側に、panel だけのまとまりを置く
    fn split_around(self, set: &[K], side: DockSide, share: f32, panel: K) -> Node<K> {
        let holds = |n: &Node<K>| set.iter().all(|k| n.group_of(k).is_some());
        match self {
            Node::Split { stacked, share: s, a, b } if holds(&a) => {
                Node::Split { stacked, share: s, a: Box::new(a.split_around(set, side, share, panel)), b }
            }
            Node::Split { stacked, share: s, a, b } if holds(&b) => {
                Node::Split { stacked, share: s, a, b: Box::new(b.split_around(set, side, share, panel)) }
            }
            other => Node::join(side, share, Node::leaf(panel), other),
        }
    }

    fn walk_mut(&mut self, f: &mut impl FnMut(&mut Node<K>)) {
        f(self);
        if let Node::Split { a, b, .. } = self {
            a.walk_mut(f);
            b.walk_mut(f);
        }
    }

    /// k を持つまとまりのパネルの並び
    fn group_of(&self, k: &K) -> Option<&[K]> {
        match self {
            Node::Tabs { panels, .. } => panels.contains(k).then_some(panels.as_slice()),
            Node::Split { a, b, .. } => a.group_of(k).or_else(|| b.group_of(k)),
        }
    }

    /// k を抜いた木と、抜けたか。空になったまとまりは畳み、親の分割はもう片方に置き換える
    fn without(self, k: &K) -> (Option<Node<K>>, bool) {
        match self {
            Node::Tabs { mut panels, selected } => {
                let Some(i) = panels.iter().position(|p| p == k) else {
                    return (Some(Node::Tabs { panels, selected }), false);
                };
                panels.remove(i);
                if panels.is_empty() {
                    return (None, true);
                }
                // 選んでいた物より前を抜いたら 1 つ詰める。選んでいた物を抜いたら、同じ位置（端なら前）の物を選ぶ
                let selected = if i < selected { selected - 1 } else { selected.min(panels.len() - 1) };
                (Some(Node::Tabs { panels, selected }), true)
            }
            Node::Split { stacked, share, a, b } => {
                let (a, fa) = a.without(k);
                let (b, fb) = if fa { (Some(*b), false) } else { b.without(k) };
                let node = match (a, b) {
                    (Some(a), Some(b)) => Some(Node::Split { stacked, share, a: Box::new(a), b: Box::new(b) }),
                    (Some(x), None) | (None, Some(x)) => Some(x),
                    (None, None) => None,
                };
                (node, fa || fb)
            }
        }
    }

    /// with を持つまとまりに、panel をタブとして index の前（None は最後）に入れて選ぶ
    fn put_tab(&mut self, with: &K, panel: K, index: Option<usize>) -> bool {
        match self {
            Node::Tabs { panels, selected } => {
                if !panels.contains(with) {
                    return false;
                }
                let i = index.unwrap_or(panels.len()).min(panels.len());
                panels.insert(i, panel);
                *selected = i;
                true
            }
            Node::Split { a, b, .. } => {
                if a.group_of(with).is_some() {
                    a.put_tab(with, panel, index)
                } else {
                    b.put_tab(with, panel, index)
                }
            }
        }
    }

    /// of を持つまとまりを分けて、side に panel だけのまとまりを置く。share は新しいまとまりの割合
    fn split_at(self, of: &K, side: DockSide, share: f32, panel: K) -> (Node<K>, bool) {
        match self {
            Node::Tabs { ref panels, .. } if panels.contains(of) => {
                let leaf = Node::Tabs { panels: vec![panel], selected: 0 };
                (Node::join(side, share, leaf, self), true)
            }
            Node::Tabs { .. } => (self, false),
            Node::Split { stacked, share: s, a, b } => {
                let (a, done) = a.split_at(of, side, share, panel.clone());
                if done {
                    return (Node::Split { stacked, share: s, a: Box::new(a), b }, true);
                }
                let (b, done) = b.split_at(of, side, share, panel);
                (Node::Split { stacked, share: s, a: Box::new(a), b: Box::new(b) }, done)
            }
        }
    }

    /// new を old の side 側に置く分割。share は new の割合
    fn join(side: DockSide, share: f32, new: Node<K>, old: Node<K>) -> Node<K> {
        let stacked = side.stacked();
        if side.first() {
            Node::Split { stacked, share, a: Box::new(new), b: Box::new(old) }
        } else {
            Node::Split { stacked, share: 1.0 - share, a: Box::new(old), b: Box::new(new) }
        }
    }

    fn write(&self, out: &mut String, name: &impl Fn(&K) -> String) {
        match self {
            Node::Tabs { panels, selected } => {
                out.push_str(&format!("tabs {selected} {}\n", panels.len()));
                for p in panels {
                    let n: String = name(p).chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
                    out.push_str(&format!("panel {n}\n"));
                }
            }
            Node::Split { stacked, share, a, b } => {
                out.push_str(&format!("split {} {share:.4}\n", if *stacked { "v" } else { "h" }));
                a.write(out, name);
                b.write(out, name);
            }
        }
    }

    /// 保存した文字から節を 1 つ読む。知らないパネルと重なったパネルは捨て、空のまとまりは None（親の分割を畳む）。
    /// 形が壊れていたら Err（全体を読めない）
    fn read<'a>(lines: &mut impl Iterator<Item = &'a str>, panel: &impl Fn(&str) -> Option<K>, seen: &mut Vec<K>) -> Option<Option<Node<K>>> {
        let line = lines.next()?;
        let mut words = line.splitn(3, ' ');
        match words.next()? {
            "tabs" => {
                let selected: usize = words.next()?.parse().ok()?;
                let count: usize = words.next()?.trim().parse().ok()?;
                let mut panels = Vec::new();
                let mut keep_selected = None;
                for i in 0..count {
                    let l = lines.next()?;
                    let n = l.strip_prefix("panel ")?;
                    if let Some(k) = panel(n).filter(|k| !seen.contains(k)) {
                        seen.push(k.clone());
                        if i == selected {
                            keep_selected = Some(panels.len());
                        }
                        panels.push(k);
                    }
                }
                if panels.is_empty() {
                    return Some(None);
                }
                let selected = keep_selected.unwrap_or(0);
                Some(Some(Node::Tabs { panels, selected }))
            }
            "split" => {
                let stacked = match words.next()? {
                    "v" => true,
                    "h" => false,
                    _ => return None,
                };
                let share: f32 = words.next()?.trim().parse().ok()?;
                let a = Node::read(lines, panel, seen)?;
                let b = Node::read(lines, panel, seen)?;
                Some(match (a, b) {
                    (Some(a), Some(b)) => Some(Node::Split { stacked, share: share.clamp(MIN_SHARE, MAX_SHARE), a: Box::new(a), b: Box::new(b) }),
                    (Some(x), None) | (None, Some(x)) => Some(x),
                    (None, None) => None,
                })
            }
            _ => None,
        }
    }
}

// ---- 引っ張って落とす先 ----

/// 描いた後の、まとまり 1 つの場所（物理ピクセル）。strip は見出しの帯、tabs は見出し 1 つずつ。floating は浮いたまとまり
/// （rect は枠の全体）。alone は、見出しが 1 つだけの時のその見出しの id
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct GroupGeom {
    pub rect: Rect,
    pub strip: Rect,
    pub tabs: Vec<Rect>,
    pub floating: bool,
    pub alone: Option<Id>,
}

/// 描いた後の、ドッキング全体の場所。groups は Dock::groups と同じ順（並べ方の木の左・上が先、その後に浮いたまとまりを
/// 下にある物から）
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DockGeom {
    pub root: Rect,
    pub groups: Vec<GroupGeom>,
}

/// 落とす先。group は DockGeom::groups の番号
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Zone {
    /// まとまりのタブとして、index の前（None は最後）
    Tab { group: usize, index: Option<usize> },
    /// まとまりを分けて side に置く
    Split { group: usize, side: DockSide },
    /// 全体の外側の side に置く
    Edge(DockSide),
}

/// 全体の外側の帯の幅（論理ピクセル）。ここに落とすと全体の端に置く
const EDGE_BAND: f32 = 20.0;
/// まとまりの中身の端からこの割合までに落とすと、その側に分ける（真ん中はタブにする）
const SPLIT_PART: f32 = 0.25;
/// 外側の端に置いた時の割合（Edge の share と同じにする）
pub(crate) const EDGE_SHARE: f32 = 0.3;

impl GroupGeom {
    /// 見出しの帯の下（中身の場所）
    fn body(&self) -> Rect {
        let top = self.strip.bottom().max(self.rect.y);
        Rect::new(self.rect.x, top, self.rect.w, (self.rect.bottom() - top).max(0.0))
    }
}

impl DockGeom {
    /// (x, y) の下のまとまり（浮いたまとまりは、上にある物から先に見る）
    pub(crate) fn group_at(&self, x: f32, y: f32) -> Option<usize> {
        let top = (0..self.groups.len()).rev().find(|&g| self.groups[g].floating && self.groups[g].rect.contains(x, y));
        top.or_else(|| self.groups.iter().position(|g| !g.floating && g.rect.contains(x, y)))
    }

    /// 並べ方の木のまとまり（浮いていない物）
    fn docked(&self) -> impl Iterator<Item = &GroupGeom> {
        self.groups.iter().filter(|g| !g.floating)
    }

    /// (x, y) が全体の外側の帯の中なら、その側。並べ方の木のまとまりが 2 つ以上の時だけ（1 つならまとまりの端と同じ）。
    /// 上の帯は、上のまとまりの見出しの帯のすぐ下に置く（見出しの帯の上はタブの間に入れる所）
    fn edge_at(&self, x: f32, y: f32, scale: f32) -> Option<DockSide> {
        if self.docked().count() < 2 || !self.root.contains(x, y) {
            return None;
        }
        let (b, r) = (EDGE_BAND * scale, self.root);
        let strip = self.docked().next().map_or(r.y, |g| g.strip.bottom());
        if x < r.x + b {
            Some(DockSide::Left)
        } else if x >= r.right() - b {
            Some(DockSide::Right)
        } else if y >= r.bottom() - b {
            Some(DockSide::Bottom)
        } else if y >= strip && y < strip + b {
            Some(DockSide::Top)
        } else {
            None
        }
    }

    /// (x, y) に落とした時の先。見出しの帯（タブの間）、全体の外側の帯、まとまりの中身の順に見る。中身では、端から
    /// 4 分の 1 までなら近い側に分け、真ん中ならタブにする（小さな的を狙わせない。どこに落ちるかは薄い塗りで見せる）。
    /// 浮いたまとまりの上は、下の物より先に見て、帯の外もタブにする（浮いたまとまりは分けない）。並べ方の木が空なら、
    /// 浮いたまとまりの外は全体
    pub(crate) fn zone_at(&self, x: f32, y: f32, scale: f32) -> Option<Zone> {
        let g = self.group_at(x, y);
        if let Some(g) = g
            && (self.groups[g].floating || self.groups[g].strip.contains(x, y))
        {
            // 見出しの真ん中より左なら、その見出しの前
            let index = if self.groups[g].strip.contains(x, y) { self.groups[g].tabs.iter().position(|t| x < t.x + t.w / 2.0) } else { None };
            return Some(Zone::Tab { group: g, index });
        }
        if self.docked().next().is_none() {
            return self.root.contains(x, y).then_some(Zone::Edge(DockSide::Left));
        }
        if let Some(side) = self.edge_at(x, y, scale) {
            return Some(Zone::Edge(side));
        }
        let g = g?;
        let b = self.groups[g].body();
        if b.w <= 0.0 || b.h <= 0.0 {
            return Some(Zone::Tab { group: g, index: None });
        }
        let (fx, fy) = ((x - b.x) / b.w, (y - b.y) / b.h);
        let near = [(fx, DockSide::Left), (1.0 - fx, DockSide::Right), (fy, DockSide::Top), (1.0 - fy, DockSide::Bottom)];
        let (d, side) = near.into_iter().fold((f32::MAX, DockSide::Left), |m, n| if n.0 < m.0 { n } else { m });
        Some(if d < SPLIT_PART { Zone::Split { group: g, side } } else { Zone::Tab { group: g, index: None } })
    }

    /// moving（引っ張っている見出し）を (x, y) に落とした時の先。moving だけのまとまりの上は、タブにしても分けても
    /// 何も変わらないので落とす先にしない（薄く塗らず、落としても何もしない）。全体の端の帯へは落とせる
    pub(crate) fn zone_for(&self, x: f32, y: f32, scale: f32, moving: Id) -> Option<Zone> {
        let zone = self.zone_at(x, y, scale)?;
        let own = match zone {
            Zone::Tab { group, .. } | Zone::Split { group, .. } => self.groups[group].alone == Some(moving),
            Zone::Edge(_) => false,
        };
        (!own).then_some(zone)
    }

    /// 落ちる所（薄く塗る四角）
    pub(crate) fn preview(&self, z: Zone) -> Option<Rect> {
        let half = |r: Rect, side: DockSide, part: f32| match side {
            DockSide::Left => Rect::new(r.x, r.y, r.w * part, r.h),
            DockSide::Right => Rect::new(r.right() - r.w * part, r.y, r.w * part, r.h),
            DockSide::Top => Rect::new(r.x, r.y, r.w, r.h * part),
            DockSide::Bottom => Rect::new(r.x, r.bottom() - r.h * part, r.w, r.h * part),
        };
        Some(match z {
            Zone::Tab { group, .. } => self.groups.get(group)?.rect,
            Zone::Split { group, side } => half(self.groups.get(group)?.rect, side, 0.5),
            // 並べ方の木が空なら、全体に置く
            Zone::Edge(_) if self.docked().next().is_none() => self.root,
            Zone::Edge(side) => half(self.root, side, EDGE_SHARE),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 左右 2 つのまとまり（左は 0〜400、右は 400〜900、どちらも高さ 600、見出しの帯は 30）
    fn two_groups() -> DockGeom {
        let group = |x: f32, w: f32, tabs: usize| GroupGeom {
            rect: Rect::new(x, 0.0, w, 600.0),
            strip: Rect::new(x, 0.0, w, 30.0),
            tabs: (0..tabs).map(|i| Rect::new(x + i as f32 * 80.0, 0.0, 76.0, 30.0)).collect(),
            floating: false,
            alone: None,
        };
        DockGeom { root: Rect::new(0.0, 0.0, 900.0, 600.0), groups: vec![group(0.0, 400.0, 2), group(400.0, 500.0, 1)] }
    }

    #[test]
    fn the_middle_of_a_group_adds_a_tab_and_its_sides_split_it() {
        // 作者の手の確かめ（ドッキングの 2 回目）で、小さな的を 14 秒狙っても、隣の的と入れ替わって落とせなかった。
        // 的を狙わせず、ポインターのいる場所で決める。右のまとまりの中身は x 400〜900、y 30〜600
        let g = two_groups();
        assert_eq!(g.zone_at(650.0, 315.0, 1.0), Some(Zone::Tab { group: 1, index: None }), "真ん中はタブ");
        assert_eq!(g.zone_at(860.0, 315.0, 1.0), Some(Zone::Split { group: 1, side: DockSide::Right }), "右の 4 分の 1");
        assert_eq!(g.zone_at(450.0, 315.0, 1.0), Some(Zone::Split { group: 1, side: DockSide::Left }));
        assert_eq!(g.zone_at(650.0, 560.0, 1.0), Some(Zone::Split { group: 1, side: DockSide::Bottom }));
        assert_eq!(g.zone_at(650.0, 70.0, 1.0), Some(Zone::Split { group: 1, side: DockSide::Top }));
        // 角は近い方の辺（右まで 0.06、上まで 0.09）
        assert_eq!(g.zone_at(870.0, 80.0, 1.0), Some(Zone::Split { group: 1, side: DockSide::Right }));
    }

    #[test]
    fn the_outer_bands_win_and_only_with_two_groups_or_more() {
        let g = two_groups();
        assert_eq!(g.zone_at(10.0, 300.0, 1.0), Some(Zone::Edge(DockSide::Left)));
        assert_eq!(g.zone_at(890.0, 300.0, 1.0), Some(Zone::Edge(DockSide::Right)));
        assert_eq!(g.zone_at(450.0, 590.0, 1.0), Some(Zone::Edge(DockSide::Bottom)));
        assert_eq!(g.zone_at(650.0, 40.0, 1.0), Some(Zone::Edge(DockSide::Top)), "上の帯は見出しの帯のすぐ下");
        assert_eq!(g.zone_at(30.0, 300.0, 1.0), Some(Zone::Split { group: 0, side: DockSide::Left }), "帯の内側はまとまりの左");
        // 2 倍の画面では帯も 2 倍
        assert_eq!(g.zone_at(30.0, 300.0, 2.0), Some(Zone::Edge(DockSide::Left)));
        let mut one = g.clone();
        one.groups.truncate(1);
        assert_eq!(one.zone_at(10.0, 300.0, 1.0), Some(Zone::Split { group: 0, side: DockSide::Left }), "1 つだけなら帯は無い");
    }

    #[test]
    fn the_tab_strip_picks_the_place_between_tabs() {
        let g = two_groups();
        // 見出しは 0〜76 と 80〜156。帯は全体の外側の帯より先に見る
        assert_eq!(g.zone_at(10.0, 15.0, 1.0), Some(Zone::Tab { group: 0, index: Some(0) }));
        assert_eq!(g.zone_at(60.0, 15.0, 1.0), Some(Zone::Tab { group: 0, index: Some(1) }), "1 つ目の右半分は 2 つ目の前");
        assert_eq!(g.zone_at(300.0, 15.0, 1.0), Some(Zone::Tab { group: 0, index: None }), "見出しの後ろの空き");
        // まとまりの外（仕切りの上など）は落とす先ではない
        assert_eq!(g.zone_at(950.0, 300.0, 1.0), None);
    }

    /// two_groups の上に、x 300〜600・y 100〜300 の浮いたまとまり（見出しは 320〜396 の 1 つ）を重ねる
    fn with_float() -> DockGeom {
        let mut g = two_groups();
        g.groups.push(GroupGeom {
            rect: Rect::new(300.0, 100.0, 300.0, 200.0),
            strip: Rect::new(300.0, 100.0, 300.0, 30.0),
            tabs: vec![Rect::new(320.0, 100.0, 76.0, 30.0)],
            floating: true,
            alone: None,
        });
        g
    }

    #[test]
    fn a_tab_alone_in_its_group_cannot_be_dropped_on_its_own_group() {
        // 作者の手の確かめ（ドッキングの 2 段目の 2 回目）で、1 つだけの「出力」を自分のまとまりの上に 2 回落とした。
        // 薄い塗りが出たのに、タブにしても分けても何も変わらなかった。自分だけのまとまりは落とす先にしない
        let me = Id(7);
        let mut g = two_groups();
        g.groups[1].alone = Some(me);
        assert_eq!(g.zone_for(650.0, 315.0, 1.0, me), None, "真ん中");
        assert_eq!(g.zone_for(650.0, 70.0, 1.0, me), None, "上の 4 分の 1");
        assert_eq!(g.zone_for(420.0, 15.0, 1.0, me), None, "見出しの帯");
        assert_eq!(g.zone_for(200.0, 300.0, 1.0, me), Some(Zone::Tab { group: 0, index: None }), "ほかのまとまりへは落とせる");
        assert_eq!(g.zone_for(890.0, 300.0, 1.0, me), Some(Zone::Edge(DockSide::Right)), "全体の端の帯へも落とせる");
        assert_eq!(g.zone_for(650.0, 315.0, 1.0, Id(8)), Some(Zone::Tab { group: 1, index: None }), "ほかのタブは入れられる");
        // 浮いたまとまりも同じ
        let mut f = with_float();
        f.groups[2].alone = Some(me);
        assert_eq!(f.zone_for(450.0, 200.0, 1.0, me), None);
        assert_eq!(f.zone_for(450.0, 200.0, 1.0, Id(8)), Some(Zone::Tab { group: 2, index: None }));
    }

    #[test]
    fn floating_groups_cover_what_is_below_and_only_take_tabs() {
        let g = with_float();
        assert_eq!(g.group_at(350.0, 200.0), Some(2), "上に重なった浮いたまとまり");
        assert_eq!(g.zone_at(350.0, 200.0, 1.0), Some(Zone::Tab { group: 2, index: None }), "中身はどこでもタブ");
        assert_eq!(g.zone_at(590.0, 290.0, 1.0), Some(Zone::Tab { group: 2, index: None }), "端でも分けない");
        assert_eq!(g.zone_at(330.0, 115.0, 1.0), Some(Zone::Tab { group: 2, index: Some(0) }));
        assert_eq!(g.zone_at(380.0, 115.0, 1.0), Some(Zone::Tab { group: 2, index: None }), "見出しの右半分は後ろ");
        // 外は今までどおり（外側の帯は、並べ方の木のまとまりだけを数える）
        assert_eq!(g.zone_at(650.0, 400.0, 1.0), Some(Zone::Tab { group: 1, index: None }));
        assert_eq!(g.zone_at(10.0, 300.0, 1.0), Some(Zone::Edge(DockSide::Left)));
        assert_eq!(g.preview(Zone::Tab { group: 2, index: None }), Some(Rect::new(300.0, 100.0, 300.0, 200.0)));
        let mut one = g.clone();
        one.groups.remove(1);
        assert_eq!(one.zone_at(10.0, 300.0, 1.0), Some(Zone::Split { group: 0, side: DockSide::Left }), "木のまとまりが 1 つなら帯は無い");
    }

    #[test]
    fn with_only_floating_groups_the_rest_of_the_dock_takes_the_panel() {
        let mut g = with_float();
        g.groups.drain(0..2);
        assert_eq!(g.zone_at(50.0, 50.0, 1.0), Some(Zone::Edge(DockSide::Left)));
        assert_eq!(g.preview(Zone::Edge(DockSide::Left)), Some(g.root), "全体に置く");
        assert_eq!(g.zone_at(350.0, 200.0, 1.0), Some(Zone::Tab { group: 0, index: None }));
        assert_eq!(g.zone_at(950.0, 50.0, 1.0), None);
    }

    #[test]
    fn previews_show_where_the_panel_lands() {
        let g = two_groups();
        assert_eq!(g.preview(Zone::Tab { group: 1, index: None }), Some(Rect::new(400.0, 0.0, 500.0, 600.0)));
        assert_eq!(g.preview(Zone::Split { group: 1, side: DockSide::Right }), Some(Rect::new(650.0, 0.0, 250.0, 600.0)));
        assert_eq!(g.preview(Zone::Edge(DockSide::Top)), Some(Rect::new(0.0, 0.0, 900.0, 180.0)));
        assert_eq!(g.preview(Zone::Split { group: 5, side: DockSide::Left }), None);
    }

    /// 木の形を短い文字にする（試験で比べやすいように）。[A* B] はタブ（* が選んでいる物）、h(…|…) は左右、v(…|…) は上下、
    /// 後ろの ~[…] は浮いたまとまり（下にある物が先）、^[…] は別の窓に出したまとまり
    fn shape(d: &Dock<&str>) -> String {
        fn go(n: &Node<&str>) -> String {
            match n {
                Node::Tabs { panels, selected } => {
                    let names: Vec<String> = panels.iter().enumerate().map(|(i, p)| if i == *selected { format!("{p}*") } else { p.to_string() }).collect();
                    format!("[{}]", names.join(" "))
                }
                Node::Split { stacked, share, a, b } => format!("{}{:.1}({}|{})", if *stacked { "v" } else { "h" }, share, go(a), go(b)),
            }
        }
        let mut s = d.root.as_ref().map_or("-".to_string(), go);
        for f in &d.floating {
            s += &format!(" {}{}", if f.window { "^" } else { "~" }, go(&f.tabs));
        }
        s
    }

    fn split(of: &'static str, side: DockSide) -> DockTo<&'static str> {
        DockTo::Split { of, side, share: 0.5 }
    }

    fn float() -> DockTo<&'static str> {
        DockTo::Float { x: 10.0, y: 20.0, w: 300.0, h: 200.0 }
    }

    #[test]
    fn float_takes_a_panel_out_and_dock_puts_it_back_in_its_tab_place() {
        let mut d = Dock::new(["A", "B", "C"]);
        d.select(&"C");
        assert!(d.insert("B", float()));
        assert_eq!(shape(&d), "[A C*] ~[B*]");
        assert_eq!(d.panels(), vec![&"A", &"C", &"B"], "浮いたまとまりは木の後");
        assert_eq!(d.selected(), vec![&"C", &"B"]);
        assert!(d.is_floating(&"B") && !d.is_floating(&"A"));
        assert_eq!(d.floating[0].rect, [10.0, 20.0, 300.0, 200.0]);
        assert!(d.dock_back(&"B"));
        assert_eq!(shape(&d), "[A B* C]", "元の 2 つ目の位置");
        assert!(!d.dock_back(&"B"), "浮いていなければ何もしない");
    }

    /// 別の窓（画面の左の外、もう 1 つの画面にも置ける）
    fn window() -> DockTo<&'static str> {
        DockTo::Window { x: -300.0, y: 40.0, w: 360.0, h: 240.0 }
    }

    #[test]
    fn windows_hold_groups_like_floating_ones() {
        let mut d = Dock::new(["A", "B", "C"]);
        d.area = 0xab;
        assert!(d.insert("B", window()));
        assert_eq!(shape(&d), "[A* C] ^[B*]");
        assert!(d.is_floating(&"B"), "窓も浮いたまとまりの 1 つ");
        assert_eq!(d.floating[0].rect, [-300.0, 40.0, 360.0, 240.0]);
        // 窓のキーは主の窓の部品の id と浮かせた順の番号
        assert!(d.has_window("kargui-dock-ab-0"));
        assert!(!d.has_window("kargui-dock-ab-1") && !d.has_window("settings"));
        // タブは入れられるが、分けられない
        assert!(d.insert("C", DockTo::Tab { with: "B", index: None }));
        assert_eq!(shape(&d), "[A*] ^[B C*]");
        assert!(!d.insert("A", split("B", DockSide::Left)), "窓のまとまりは分けない");
        // パネルごとに、窓へ入る前にいた所へ戻る。最後のパネルが出たら窓のまとまりは無くなる
        assert!(d.dock_back(&"C"));
        assert_eq!(shape(&d), "[A C*] ^[B*]");
        assert!(d.dock_back(&"B"));
        assert_eq!(shape(&d), "[A B* C]");
        assert!(!d.has_window("kargui-dock-ab-0"));
        // 空の dock に窓で入れると、窓のまとまりだけになる
        let mut e: Dock<&str> = Dock::new([]);
        assert!(e.insert("A", window()));
        assert_eq!(shape(&e), "- ^[A*]");
    }

    #[test]
    fn only_groups_inside_the_dock_are_drawn_there_and_reached_by_f6() {
        let mut d = Dock::new(["A", "B", "C", "D"]);
        d.insert("B", float());
        d.insert("C", window());
        d.insert("D", float());
        assert_eq!(shape(&d), "[A*] ~[B*] ^[C*] ~[D*]");
        assert_eq!(d.frames(), vec![0, 2], "全体の中に描くのは浮いた枠だけ（重なりの順）");
        d.raise(0);
        assert_eq!(shape(&d), "[A*] ^[C*] ~[D*] ~[B*]");
        assert_eq!(d.float_order(), vec![2, 1], "F6 は浮かせた順（B、D）。窓は回らない");
        // 窓を置けない所（Wayland）では、窓のまとまりを全体の中の枠に戻す
        assert!(d.windows_to_frames([100.0, 50.0, 300.0, 200.0]));
        assert_eq!(shape(&d), "[A*] ~[C*] ~[D*] ~[B*]");
        assert_eq!(d.floating[0].rect, [100.0, 50.0, 300.0, 200.0]);
        assert!(!d.windows_to_frames([0.0, 0.0, 300.0, 200.0]), "もう窓は無い");
    }

    #[test]
    fn windows_are_saved_with_their_place_on_the_screen() {
        let mut d = Dock::new(["A", "B"]);
        d.insert("B", window());
        let text = d.save(|p| p.to_string());
        assert!(text.ends_with("window -300 40 360 240\ntabs 0 1\npanel B\n"), "{text}");
        let known = |n: &str| ["A", "B"].into_iter().find(|p| *p == n);
        let back = Dock::load(&text, known).unwrap();
        assert_eq!(back, d);
        assert!(back.floating[0].window);
        // 窓だけの並べ方も読める
        let only = Dock::load("kargui-dock 1\nwindow 10 20 300 200\ntabs 0 1\npanel A\n", known).unwrap();
        assert_eq!(shape(&only), "- ^[A*]");
    }

    #[test]
    fn a_panel_alone_in_its_group_goes_back_beside_the_same_neighbours() {
        // 左の F が、上下に分けた E と O の全体の隣にいた。戻すと E と O の全体の左へ（E だけを分けない）
        let mut d = Dock::new(["E"]);
        d.insert("O", DockTo::Split { of: "E", side: DockSide::Bottom, share: 0.3 });
        d.insert("F", DockTo::Edge { side: DockSide::Left, share: 0.2 });
        let before = shape(&d);
        assert_eq!(before, "h0.2([F*]|v0.7([E*]|[O*]))");
        assert!(d.insert("F", float()));
        assert_eq!(shape(&d), "v0.7([E*]|[O*]) ~[F*]");
        assert!(d.dock_back(&"F"));
        assert_eq!(shape(&d), before);
        // 分割の 2 つ目（右・下）にいた物も、同じ割合で戻る
        assert!(d.insert("O", float()));
        assert_eq!(shape(&d), "h0.2([F*]|[E*]) ~[O*]");
        assert!(d.dock_back(&"O"));
        assert_eq!(shape(&d), before);
    }

    #[test]
    fn docking_back_after_the_neighbours_changed() {
        let mut d = Dock::new(["A"]);
        d.insert("B", split("A", DockSide::Right));
        d.insert("C", split("B", DockSide::Bottom));
        d.insert("A", float());
        // 隣にあった物の一部が無くなったら、残っている物の隣
        d.remove(&"B");
        assert!(d.dock_back(&"A"));
        assert_eq!(shape(&d), "h0.5([A*]|[C*])");
        // 木が空になっていたら、木の全体
        d.insert("A", float());
        d.remove(&"C");
        assert!(d.dock_back(&"A"));
        assert_eq!(shape(&d), "[A*]");
        // 元のまとまりが無くなっていたら、全体の右の端
        let mut d = Dock::new(["A", "B"]);
        d.insert("C", split("B", DockSide::Right));
        d.insert("B", float());
        d.remove(&"A");
        assert!(d.dock_back(&"B"));
        assert_eq!(shape(&d), "h0.7([C*]|[B*])");
    }

    #[test]
    fn floating_groups_take_tabs_but_are_not_split() {
        let mut d = Dock::new(["A", "B", "C"]);
        d.insert("B", float());
        assert!(d.insert("C", DockTo::Tab { with: "B", index: Some(0) }));
        assert_eq!(shape(&d), "[A*] ~[C* B]");
        assert!(!d.insert("A", split("B", DockSide::Right)), "浮いたまとまりは分けない");
        assert_eq!(shape(&d), "[A*] ~[C* B]");
        // 木へ引っ張って戻す。最後の 1 つが出ると、浮いたまとまりは消える
        assert!(d.insert("C", split("A", DockSide::Right)));
        assert!(d.insert("B", DockTo::Edge { side: DockSide::Bottom, share: 0.3 }));
        assert_eq!(shape(&d), "v0.7(h0.5([A*]|[C*])|[B*])");
        assert!(d.floating.is_empty());
    }

    #[test]
    fn each_panel_of_a_floating_group_docks_back_to_its_own_place() {
        // 作者の手の確かめ（0.4 の 2 段目）で、浮いたまとまりへ引いて入れた出力を戻すと、先に浮かせたメモの元の場所へ
        // 行った。戻る先はパネルごとに覚える
        let mut d = Dock::new(["A", "B", "C"]);
        d.insert("D", split("A", DockSide::Bottom));
        d.insert("B", float());
        d.insert("D", DockTo::Tab { with: "B", index: None });
        assert_eq!(shape(&d), "[A* C] ~[B D*]");
        assert!(d.dock_back(&"D"), "後から入った D は、入る前にいた A の下へ");
        assert_eq!(shape(&d), "v0.5([A* C]|[D*]) ~[B*]");
        assert!(d.dock_back(&"B"));
        assert_eq!(shape(&d), "v0.5([A B* C]|[D*])", "B は元のタブの位置へ");
        // 浮いたまとまりの中で並べ替えても、戻る先は変わらない
        d.insert("C", float());
        d.insert("A", DockTo::Tab { with: "C", index: None });
        d.insert("A", DockTo::Tab { with: "C", index: Some(0) });
        assert_eq!(shape(&d), "v0.5([B*]|[D*]) ~[A* C]");
        assert!(d.dock_back(&"A"));
        assert!(d.dock_back(&"C"));
        assert_eq!(shape(&d), "v0.5([A B C*]|[D*])");
    }

    #[test]
    fn panels_without_a_place_of_their_own_dock_with_the_first_one_back() {
        // 読み込んだ浮いたまとまりは戻る先を覚えていない: 最初の 1 つは右の端へ、残りはそのまとまりへ
        let mut d = Dock::new(["A", "B", "C"]);
        d.insert("B", float());
        d.insert("C", DockTo::Tab { with: "B", index: None });
        let text = d.save(|k| k.to_string());
        let mut d = Dock::load(&text, |s| ["A", "B", "C"].into_iter().find(|k| *k == s)).unwrap();
        assert_eq!(shape(&d), "[A*] ~[B C*]");
        assert!(d.dock_back(&"C"));
        assert_eq!(shape(&d), "h0.7([A*]|[C*]) ~[B*]");
        assert!(d.dock_back(&"B"));
        assert_eq!(shape(&d), "h0.7([A*]|[C B*])", "先に戻した C のまとまりへ");
    }

    #[test]
    fn floats_raise_move_and_keep_a_usable_size() {
        let mut d = Dock::new(["A", "B", "C"]);
        d.insert("B", DockTo::Float { x: 5.4, y: 6.0, w: 10.0, h: 10.0 });
        assert_eq!(d.floating[0].rect, [5.0, 6.0, FLOAT_MIN.0, FLOAT_MIN.1], "整数に丸め、小さすぎる大きさは広げる");
        d.insert("C", float());
        assert_eq!(shape(&d), "[A*] ~[B*] ~[C*]");
        assert!(d.raise(0));
        assert_eq!(shape(&d), "[A*] ~[C*] ~[B*]", "上にした物が最後");
        assert_eq!(d.float_order(), vec![1, 0], "F6 で回る順は浮かせた順（B、C）のまま");
        assert!(!d.raise(1), "もういちばん上");
        d.set_float_rect(1, [100.0, 50.0, 400.0, f32::NAN]);
        assert_eq!(d.floating[1].rect, [100.0, 50.0, 400.0, FLOAT_MIN.1]);
        // 浮いた物をもう一度浮かせると、場所だけが変わり、戻る先は変わらない（B は元の 2 つ目へ）
        d.insert("B", DockTo::Float { x: 0.0, y: 0.0, w: 200.0, h: 200.0 });
        assert!(d.dock_back(&"B"));
        assert_eq!(shape(&d), "[A B*] ~[C*]");
    }

    #[test]
    fn floats_round_trip_through_save() {
        let mut d = Dock::new(["A", "B", "C"]);
        d.insert("B", float());
        d.insert("C", DockTo::Float { x: 50.0, y: -4.0, w: 320.0, h: 240.0 });
        let text = d.save(|p| p.to_string());
        assert!(text.ends_with("float 50 -4 320 240\ntabs 0 1\npanel C\n"), "{text}");
        let names = ["A", "B", "C"];
        let known = |n: &str| names.iter().copied().find(|p| *p == n);
        assert_eq!(Dock::load(&text, known), Some(d), "戻る先は保存しないが、比べる時も見ない");
        // 浮いたまとまりだけの並べ方
        let only = Dock::load("kargui-dock 1\nfloat 1 2 300 200\ntabs 0 1\npanel A\n", known).unwrap();
        assert_eq!(shape(&only), "- ~[A*]");
        // 知らないパネルだけの浮いたまとまりは捨てる
        let gone = Dock::load("kargui-dock 1\ntabs 0 1\npanel A\nfloat 1 2 300 200\ntabs 0 1\npanel X\n", known).unwrap();
        assert_eq!(shape(&gone), "[A*]");
        let any = |n: &str| Some(n.to_string());
        assert_eq!(Dock::load("kargui-dock 1\ntabs 0 1\npanel A\nfloat 1 2 300 200\nsplit h 0.5\ntabs 0 1\npanel B\ntabs 0 1\npanel C\n", any), None, "浮いたまとまりは分割にしない");
        assert_eq!(Dock::load("kargui-dock 1\ntabs 0 1\npanel A\nfloat 1 2\ntabs 0 1\npanel B\n", any), None, "数が足りない");
        assert_eq!(Dock::load("kargui-dock 1\ntabs 0 1\npanel A\nfloat 1 2 inf 4\ntabs 0 1\npanel B\n", any), None, "数でない");
    }

    #[test]
    fn an_empty_dock_can_start_with_a_float() {
        let mut d = Dock::<&str>::new([]);
        assert!(d.insert("A", float()));
        assert_eq!(shape(&d), "- ~[A*]");
        assert!(d.insert("B", DockTo::Tab { with: "A", index: None }));
        assert_eq!(shape(&d), "- ~[A B*]");
        assert!(d.insert("C", DockTo::Edge { side: DockSide::Left, share: 0.3 }), "木が空でも端には置ける");
        assert_eq!(shape(&d), "[C*] ~[A B*]");
    }

    #[test]
    fn new_keeps_each_panel_once() {
        assert_eq!(shape(&Dock::new(["A", "B", "A"])), "[A* B]");
        assert_eq!(shape(&Dock::<&str>::new([])), "-");
    }

    #[test]
    fn split_puts_the_panel_on_the_side_with_its_share() {
        let mut d = Dock::new(["A"]);
        assert!(d.insert("B", DockTo::Split { of: "A", side: DockSide::Right, share: 0.3 }));
        assert_eq!(shape(&d), "h0.7([A*]|[B*])", "右に 0.3: 1 つ目（左）は 0.7");
        assert!(d.insert("C", DockTo::Split { of: "A", side: DockSide::Top, share: 0.4 }));
        assert_eq!(shape(&d), "h0.7(v0.4([C*]|[A*])|[B*])", "A のまとまりだけを上下に分ける");
        // 割合は 0.1〜0.9 に収める
        assert!(d.insert("D", DockTo::Split { of: "B", side: DockSide::Left, share: 2.0 }));
        assert_eq!(shape(&d), "h0.7(v0.4([C*]|[A*])|h0.9([D*]|[B*]))");
    }

    #[test]
    fn edge_wraps_the_whole_dock() {
        let mut d = Dock::new(["A"]);
        d.insert("B", split("A", DockSide::Right));
        assert!(d.insert("C", DockTo::Edge { side: DockSide::Bottom, share: 0.25 }));
        assert_eq!(shape(&d), "v0.8(h0.5([A*]|[B*])|[C*])");
        assert!(d.insert("D", DockTo::Edge { side: DockSide::Left, share: 0.2 }));
        assert_eq!(shape(&d), "h0.2([D*]|v0.8(h0.5([A*]|[B*])|[C*]))");
    }

    #[test]
    fn removing_the_last_panel_of_a_group_folds_its_split() {
        let mut d = Dock::new(["A"]);
        d.insert("B", split("A", DockSide::Right));
        d.insert("C", split("B", DockSide::Bottom));
        assert_eq!(shape(&d), "h0.5([A*]|v0.5([B*]|[C*]))");
        assert!(d.remove(&"B"));
        assert_eq!(shape(&d), "h0.5([A*]|[C*])", "B のまとまりが消え、C が B の場所を取る");
        assert!(d.remove(&"A"));
        assert_eq!(shape(&d), "[C*]");
        assert!(d.remove(&"C"));
        assert_eq!(shape(&d), "-");
        assert!(!d.remove(&"C"));
    }

    #[test]
    fn removing_keeps_a_sensible_selection() {
        let mut d = Dock::new(["A", "B", "C", "D"]);
        d.select(&"C");
        d.remove(&"A");
        assert_eq!(shape(&d), "[B C* D]", "前を抜いても C のまま");
        d.remove(&"C");
        assert_eq!(shape(&d), "[B D*]", "選んでいた物を抜くと、同じ位置の物");
        d.remove(&"D");
        assert_eq!(shape(&d), "[B*]", "端なら前の物");
    }

    #[test]
    fn tabs_move_within_a_group_and_between_groups() {
        let mut d = Dock::new(["A", "B", "C"]);
        // 後ろへ: index は動かす前の並びで数える（C の後ろ = 3）
        assert!(d.insert("A", DockTo::Tab { with: "B", index: Some(3) }));
        assert_eq!(shape(&d), "[B C A*]");
        // 前へ
        assert!(d.insert("A", DockTo::Tab { with: "C", index: Some(0) }));
        assert_eq!(shape(&d), "[A* B C]");
        // 自分を指しても、同じまとまりのほかのパネルが指す先になる
        assert!(d.insert("A", DockTo::Tab { with: "A", index: Some(2) }));
        assert_eq!(shape(&d), "[B A* C]");
        // ほかのまとまりへ移ると、元のまとまりは残りで続く
        d.insert("D", split("C", DockSide::Right));
        assert!(d.insert("B", DockTo::Tab { with: "D", index: None }));
        assert_eq!(shape(&d), "h0.5([A* C]|[D B*])");
        // まとまりの最後の 1 つを動かすと、元のまとまりは消える
        assert!(d.insert("A", DockTo::Tab { with: "D", index: Some(0) }));
        assert!(d.insert("C", DockTo::Tab { with: "D", index: None }));
        assert_eq!(shape(&d), "[A D B C*]");
    }

    #[test]
    fn splitting_a_panel_off_its_own_group() {
        let mut d = Dock::new(["A", "B"]);
        // 自分のいるまとまりを分けて自分を右へ（ほかに B がいるので B のまとまりを指す）
        assert!(d.insert("A", split("A", DockSide::Right)));
        assert_eq!(shape(&d), "h0.5([B*]|[A*])");
        // 自分だけのまとまりを自分の隣へは動かせない
        assert!(!d.insert("A", split("A", DockSide::Left)));
        assert_eq!(shape(&d), "h0.5([B*]|[A*])");
        // 自分だけのまとまりにタブとして戻すのは、何もしない（成功）
        assert!(d.insert("A", DockTo::Tab { with: "A", index: None }));
        assert_eq!(shape(&d), "h0.5([B*]|[A*])");
    }

    #[test]
    fn places_named_by_missing_panels_change_nothing() {
        let mut d = Dock::new(["A"]);
        assert!(!d.insert("B", split("X", DockSide::Right)));
        assert!(!d.insert("B", DockTo::Tab { with: "X", index: None }));
        assert_eq!(shape(&d), "[A*]");
        // 動かすパネルが指す先を失わないように、抜く前に決めている
        d.insert("B", split("A", DockSide::Right));
        assert!(!d.insert("A", split("X", DockSide::Right)));
        assert_eq!(shape(&d), "h0.5([A*]|[B*])", "失敗した時は A を抜かない");
    }

    #[test]
    fn an_empty_dock_takes_any_panel() {
        let mut d = Dock::<&str>::new([]);
        assert!(d.insert("A", split("X", DockSide::Right)));
        assert_eq!(shape(&d), "[A*]");
        d.remove(&"A");
        assert!(d.insert("B", DockTo::Edge { side: DockSide::Top, share: 0.3 }));
        assert_eq!(shape(&d), "[B*]");
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut d = Dock::new(["A", "B"]);
        d.insert("C", DockTo::Split { of: "A", side: DockSide::Right, share: 0.35 });
        d.insert("D", DockTo::Edge { side: DockSide::Bottom, share: 0.2 });
        d.select(&"B");
        let text = d.save(|p| p.to_string());
        assert!(text.starts_with("kargui-dock 1\n"), "{text}");
        let names = ["A", "B", "C", "D"];
        let back = Dock::load(&text, |n| names.iter().copied().find(|p| *p == n)).unwrap();
        assert_eq!(shape(&back), shape(&d));
        assert_eq!(back.selected(), d.selected());
    }

    #[test]
    fn load_drops_unknown_and_repeated_panels_and_folds_empty_groups() {
        let mut d = Dock::new(["A", "Old"]);
        d.insert("C", split("A", DockSide::Right));
        d.insert("Gone", split("C", DockSide::Bottom));
        d.select(&"Old");
        let text = d.save(|p| p.to_string());
        let known = ["A", "C"];
        let back = Dock::load(&text, |n| known.iter().copied().find(|p| *p == n)).unwrap();
        assert_eq!(shape(&back), "h0.5([A*]|[C*])", "Old が選ばれていたまとまりは最初を選ぶ。Gone のまとまりは畳む");
        // 名前が 2 回出てきたら、2 回目は捨てる
        let twice = "kargui-dock 1\nsplit h 0.5\ntabs 0 1\npanel A\ntabs 0 2\npanel A\npanel C\n";
        let back = Dock::load(twice, |n| known.iter().copied().find(|p| *p == n)).unwrap();
        assert_eq!(shape(&back), "h0.5([A*]|[C*])");
    }

    #[test]
    fn load_refuses_text_that_is_not_a_layout() {
        let any = |n: &str| Some(n.to_string());
        assert_eq!(Dock::load("", any), None);
        assert_eq!(Dock::load("kargui-dock 2\ntabs 0 1\npanel A\n", any), None, "新しい版");
        assert_eq!(Dock::load("kargui-dock 1\ntabs 0 2\npanel A\n", any), None, "パネルが足りない");
        assert_eq!(Dock::load("kargui-dock 1\nsplit x 0.5\ntabs 0 1\npanel A\ntabs 0 1\npanel B\n", any), None, "向きが壊れている");
        assert_eq!(Dock::load("kargui-dock 1\ntabs 0 1\npanel A\ntabs 0 1\npanel B\n", any), None, "余りの行");
        assert_eq!(Dock::load("kargui-dock 1\n", any), Some(Dock { root: None, floating: Vec::new(), area: 0 }), "空の並べ方");
        // 名前の中の空白はそのまま
        let d = Dock::new(["two words".to_string()]);
        assert_eq!(Dock::load(&d.save(|p| p.clone()), any), Some(d));
    }
}
