//! ドッキングの部品（`Ui::dock`）。並べ方は `Dock<K>`（dock.rs）が持ち、ここは毎フレーム木をたどって部品を作る。
//! 押したタブは作る前に選び、キー・メニュー・仕切りの変更は作り終えてから Dock に当てる（変わったらもう一度回す）。
//! 入れ物は重ねる形（Stack）で、1 つ目が並べ方の木、その後に浮いたまとまりの枠を下にある物から重ねる。
//! 別の窓に出したまとまりは、主の窓の Ui::dock が Ui::window で窓を頼み、その窓の App::window の中の Ui::dock が
//! そのまとまりだけを窓いっぱいに描く

use std::hash::Hash;

use super::context::WindowPlace;
use super::dock::{Dock, DockSide, DockTo, EDGE_SHARE, FLOAT_MIN, Node, Zone};
use super::layout::{Align, Dir, Size};
use crate::geom::Rect;
use super::{Id, Kind, Ui, descends, hash_of};
use crate::icon::Icon;
use crate::input::{Event, Key, Mods};

/// 1 回のビューで集めて、作り終えてから当てる変更
enum Change<K> {
    Select(K),
    Move(K, DockTo<K>),
    /// 分割の道（根が 1）と、新しい割合
    Share(u64, f32),
    /// 浮いたまとまりのパネルを、浮かせる前にいた所へ戻す（メニューの「Dock」）
    DockBack(K),
    /// 浮いたまとまり（Dock::floating の番号）をいちばん上にする（F6 で移った時）
    Raise(usize),
}

/// まとまりの置き場所（見出しの帯とメニューが変わる）
#[derive(Clone, Copy, PartialEq)]
enum GroupAt {
    /// 並べ方の木の中
    Docked,
    /// 全体の中の浮いた枠
    Frame,
    /// OS の別の窓
    Window,
}

/// Ui::dock の作り途中の状態
struct Build<'t, K> {
    dock: Id,
    closable: bool,
    title: &'t dyn Fn(&K) -> String,
    changes: Vec<Change<K>>,
    closed: Option<K>,
    /// まとまりごとの、入れ物の部品の番号・パネル・見せているパネルの位置（F6 と Ctrl+Tab に使う）
    groups: Vec<(usize, Vec<K>, usize)>,
    /// まとまりごとの、選んでいるパネルとその題（メニューの「別のまとまりへ」）
    names: Vec<(K, String)>,
    /// メニューの「Float」で浮かせる場所と大きさ（論理ピクセル）
    float_at: [f32; 4],
    /// メニューの「Move to a new window」で出す窓の場所と大きさ（描くまとまりの順）。窓を置けない所と、別の窓の中は None
    window_at: Option<Vec<[f32; 4]>>,
}

/// 引っ張っているタブの荷物（同じ dock の中でだけ受け取る）
struct DockDrag<K> {
    dock: Id,
    panel: K,
}

/// タブの見出しの id（パネルから作るので、並べ方が変わっても同じ）
fn tab_id<K: Hash>(dock: Id, k: &K) -> Id {
    Id(hash_of((dock.0, hash_of(k), "tab")))
}

/// 浮いたまとまりの印の id（まとまりの最初のパネルから作る。resize なら大きさを変える印、ほかは動かす印）
fn grip_id<K: Hash>(dock: Id, k: &K, resize: bool) -> Id {
    Id(hash_of((dock.0, hash_of(k), if resize { "float-size" } else { "float-move" })))
}

/// A dock being built: chain the options, then call [`DockArea::show`]. Made by [`Ui::dock`].
///
/// ```
/// # use kargui::Dock;
/// # let mut dock = Dock::new(["Files"]);
/// # kargui::test::example(|ui| {
/// ui.dock(&mut dock).grow().show(|p| p.to_string(), |ui, p| {
///     ui.label(*p);
/// });
/// # });
/// ```
pub struct DockArea<'u, 'a, 'd, K> {
    ui: &'u mut Ui<'a>,
    dock: &'d mut Dock<K>,
    idx: usize,
    id: Id,
    closable: bool,
}

impl<'a> Ui<'a> {
    /// A dock: panels in tab groups that the user can rearrange. `dock` holds the layout (see
    /// [`Dock`]); kargui changes it when the user rearranges the panels, and the app can save it
    /// with [`Dock::save`].
    ///
    /// Each group shows its panels' titles as tabs over the selected panel. The user can:
    ///
    /// - drag a divider (or move it with the keyboard) to share the space differently;
    /// - choose a tab by clicking it, or with Left, Right, Home and End while a tab has the
    ///   focus; Ctrl+Tab and Ctrl+PageDown (Ctrl+Shift+Tab and Ctrl+PageUp) go to the next
    ///   (previous) tab while the focus is in the group;
    /// - move the focused tab within its group with Ctrl+Shift+PageUp and Ctrl+Shift+PageDown;
    /// - go to the next (previous) group with F6 (Shift+F6), floating groups in the order they
    ///   floated;
    /// - move a panel to a new group beside its own, or into another group, from the tab's menu
    ///   (right click, Shift+F10 or the context menu key);
    /// - float a panel over the others from the tab's menu, move the floating group by the dots
    ///   at the left of its tabs, change its size by the dots at its bottom-right corner, and put
    ///   each of its panels back where it was before it joined the group with Dock in the tab's
    ///   menu;
    /// - move a panel to a window of its own from the tab's menu (on Windows and X11; not on
    ///   Wayland, where apps cannot place windows), and put it back with Dock or by closing the
    ///   window. The app builds such a window by calling `ui.dock` with the same dock in
    ///   [`App::window`](crate::App::window) when [`Dock::has_window`] is true for its key; there,
    ///   `ui.dock` builds only that window's group.
    ///
    /// Screen readers hear each group as a tab list with tabs and a tab panel, a floating group
    /// as a pane, and the dividers as separators with their position in percent.
    ///
    /// ```
    /// use kargui::{Dock, DockSide, DockTo};
    ///
    /// #[derive(Clone, Copy, PartialEq, Hash)]
    /// enum Panel { Files, Editor, Output }
    ///
    /// let mut dock = Dock::new([Panel::Editor]);
    /// dock.insert(Panel::Files, DockTo::Edge { side: DockSide::Left, share: 0.25 });
    /// dock.insert(Panel::Output, DockTo::Split { of: Panel::Editor, side: DockSide::Bottom, share: 0.3 });
    /// # kargui::test::example(|ui| {
    /// let title = |p: &Panel| match p {
    ///     Panel::Files => "Files",
    ///     Panel::Editor => "Editor",
    ///     Panel::Output => "Output",
    /// }
    /// .to_string();
    /// ui.dock(&mut dock).grow().show(title, |ui, p| match p {
    ///     Panel::Files => {
    ///         ui.label("main.rs");
    ///     }
    ///     Panel::Editor => {
    ///         ui.label("fn main() {}");
    ///     }
    ///     Panel::Output => {
    ///         ui.label("Finished");
    ///     }
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn dock<'u, 'd, K: Clone + PartialEq + Hash + 'static>(&'u mut self, dock: &'d mut Dock<K>) -> DockArea<'u, 'a, 'd, K> {
        let (idx, id) = self.add(Kind::Dock, Dir::Stack);
        self.tree.layout[idx].align = Align::Stretch;
        DockArea { ui: self, dock, idx, id, closable: false }
    }

    /// 木の節を 1 つ作る。path は分割の道（根が 1、子は 2p と 2p+1）
    fn dock_node<K: Clone + PartialEq + Hash + 'static>(&mut self, node: &Node<K>, path: u64, st: &mut Build<K>, content: &mut dyn FnMut(&mut Ui, &K)) {
        match node {
            Node::Tabs { panels, selected } => self.dock_group(panels, *selected, GroupAt::Docked, st, content),
            Node::Split { stacked, share, a, b } => {
                let stacked = *stacked;
                let dir = if stacked { Dir::Column } else { Dir::Row };
                let (si, _) = self.push(Id(hash_of((st.dock.0, path, "split"))), Kind::Split { stacked }, dir);
                self.fill_parent(si);
                let bar_id = Id(hash_of((st.dock.0, path, "bar")));
                let (p0, bar, p1) = {
                    let mut ui = self.child(si);
                    let (p0, _) = ui.push(Id(hash_of((st.dock.0, path, "first"))), Kind::Column, Dir::Column);
                    let (bar, _) = ui.push(bar_id, Kind::Splitter { stacked, value: 0.0, min: 10.0, max: 90.0 }, Dir::Leaf);
                    let (p1, _) = ui.push(Id(hash_of((st.dock.0, path, "second"))), Kind::Column, Dir::Column);
                    (p0, bar, p1)
                };
                self.tree.info[bar].label = Some("Resize".to_string());
                let scale = self.cx.scale;
                let bar_len = self.px(12.0);
                let mut s = *share;
                if !self.disabled {
                    // 前のフレームの、分割の入れ物の始まりと長さ（物理ピクセル）
                    let total = self.cx.prev.splits.get(&bar_id).copied().filter(|&(_, len)| len > bar_len);
                    let cx = &mut *self.cx;
                    if (cx.pressed == Some(bar_id) || cx.released == Some(bar_id))
                        && let (Some((px, py)), Some((start, len))) = (cx.pointer, total)
                    {
                        let pos = if stacked { py } else { px };
                        s = (pos - start - bar_len / 2.0) / (len - bar_len);
                    }
                    if cx.focus == Some(bar_id)
                        && let Some((_, len)) = total
                    {
                        // キーは今の仕切りと同じく 10 論理ピクセルずつ
                        let step = 10.0 * scale / (len - bar_len);
                        for e in std::mem::take(&mut cx.keys) {
                            if let Event::Key { key, mods } = e {
                                if mods.ctrl || mods.alt {
                                    continue;
                                }
                                match key {
                                    Key::Left | Key::Up => s -= step,
                                    Key::Right | Key::Down => s += step,
                                    Key::Home => s = 0.1,
                                    Key::End => s = 0.9,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                let s = s.clamp(0.1, 0.9);
                if (s - *share).abs() > 1e-4 {
                    st.changes.push(Change::Share(path, s));
                }
                if let Kind::Splitter { value, .. } = &mut self.tree.info[bar].kind {
                    *value = (s * 100.0).round();
                }
                {
                    let lay = &mut self.tree.layout;
                    let (x, y, z) = (Size::Fill(s), Size::Fixed(bar_len), Size::Fill(1.0 - s));
                    if stacked {
                        (lay[p0].height, lay[bar].height, lay[p1].height) = (x, y, z);
                    } else {
                        (lay[p0].width, lay[bar].width, lay[p1].width) = (x, y, z);
                    }
                    lay[p0].align = Align::Stretch;
                    lay[p1].align = Align::Stretch;
                }
                self.child(p0).dock_node(a, path * 2, st, content);
                self.child(p1).dock_node(b, path * 2 + 1, st, content);
            }
        }
    }

    /// 親の向きに沿って残りを取る（交わる向きは親の Align::Stretch で伸びる）
    fn fill_parent(&mut self, idx: usize) {
        let row = self.parent_dir() == Dir::Row;
        let l = &mut self.tree.layout[idx];
        l.align = Align::Stretch;
        if row {
            l.width = Size::Fill(1.0);
        } else {
            l.height = Size::Fill(1.0);
        }
    }

    /// タブのまとまりを 1 つ作る: 見出しの帯と、選んでいるパネルの中身。全体の中の浮いた枠は、見出しの帯の左に動かす
    /// 印を置く。浮いたまとまり（枠と別の窓）は、はみ出した中身を切り取る
    fn dock_group<K: Clone + PartialEq + Hash + 'static>(&mut self, panels: &[K], selected: usize, at: GroupAt, st: &mut Build<K>, content: &mut dyn FnMut(&mut Ui, &K)) {
        let dock = st.dock;
        let (gi, _) = self.push(Id(hash_of((dock.0, hash_of(&panels[0]), "group"))), Kind::DockGroup, Dir::Column);
        self.fill_parent(gi);
        self.tree.layout[gi].clip = at != GroupAt::Docked;
        let frame = at == GroupAt::Frame;
        let heads: Vec<Id> = panels.iter().map(|k| tab_id(dock, k)).collect();
        let n = panels.len();
        let mut picked = selected.min(n - 1);
        if !self.disabled
            && let Some(at) = self.cx.focus.and_then(|f| heads.iter().position(|&h| h == f))
        {
            // 見出しに焦点がある時: 左右・Home・End で選ぶ（端で回る）
            let mut next = at;
            for e in std::mem::take(&mut self.cx.keys) {
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
            // Ctrl+Shift+PageUp・PageDown で並べ替え、Delete で閉じる（アプリへ届くキーから取る）
            let k = panels[next].clone();
            let mut moves = Vec::new();
            let mut close = false;
            let closable = st.closable;
            self.cx.app_keys.retain(|&(key, m)| {
                let shifted = Mods { ctrl: true, shift: true, alt: false };
                match (key, m) {
                    (Key::PageUp, x) if x == shifted => moves.push(-1),
                    (Key::PageDown, x) if x == shifted => moves.push(1),
                    (Key::Delete, x) if x == Mods::default() && closable => close = true,
                    _ => return true,
                }
                false
            });
            for d in moves {
                let to = if d < 0 { next.checked_sub(1) } else { (next + 2 <= n).then_some(next + 2) };
                if let Some(index) = to {
                    st.changes.push(Change::Move(k.clone(), DockTo::Tab { with: k.clone(), index: Some(index) }));
                }
            }
            if close {
                st.closed = Some(k.clone());
            }
            if next != at {
                picked = next;
                self.cx.focus = Some(heads[next]);
                self.cx.focus_visible = true;
                st.changes.push(Change::Select(k));
            }
        }
        // 「Move to a new window」で出す窓（このまとまりの、描く順の番号で引く）
        let window_at = st.window_at.as_ref().and_then(|w| w.get(st.groups.len()).copied());
        st.groups.push((gi, panels.to_vec(), picked));
        // 見出しの帯（浮いたまとまりは、動かす印と見出しの帯を並べた行）
        let mut menu_moves: Vec<(K, DockTo<K>)> = Vec::new();
        let mut menu_close = None;
        let mut menu_dock = None;
        {
            let mut ui = self.child(gi);
            let head = if frame {
                let (h, _) = ui.add(Kind::Row, Dir::Row);
                ui.tree.layout[h].align = Align::Stretch;
                let mut hu = ui.child(h);
                let (g, _) = hu.push(grip_id(dock, &panels[0], false), Kind::DockGrip { resize: false }, Dir::Leaf);
                hu.tree.layout[g].width = Size::Fixed(hu.px(18.0));
                Some(h)
            } else {
                None
            };
            let mut ui = match head {
                Some(h) => ui.child(h),
                None => ui,
            };
            let (list, _) = ui.add(Kind::TabList, Dir::Row);
            let gap = ui.px(4.0);
            ui.tree.layout[list].gap = gap;
            if frame {
                ui.tree.layout[list].width = Size::Fill(1.0);
            }
            let mut row = ui.child(list);
            for (k, panel) in panels.iter().enumerate() {
                let title = (st.title)(panel);
                let (ti, tid) = row.push(heads[k], Kind::Tab { text: title.clone(), selected: k == picked }, Dir::Leaf);
                // Tab は選んでいるタブにだけ止まる
                row.tree.info[ti].skip_tab = k != picked;
                let others: Vec<(K, String)> = st.names.iter().filter(|(p, _)| !panels.contains(p)).cloned().collect();
                let closable = st.closable;
                let alone = n == 1;
                let p = panel.clone();
                let float_at = st.float_at;
                // 見出しを引っ張ると、ほかのまとまりや端へ動かせる（4 画素動かしてから。押しただけなら選ぶ）
                let payload = DockDrag { dock, panel: panel.clone() };
                row.handle(ti, tid).drag_source(payload).context_menu(|m| {
                    if at != GroupAt::Docked {
                        if m.item("Dock").clicked() {
                            menu_dock = Some(p.clone());
                        }
                    } else {
                        m.submenu("Move to a new group", |m| {
                            for (text, side) in [("Left", DockSide::Left), ("Right", DockSide::Right), ("Above", DockSide::Top), ("Below", DockSide::Bottom)] {
                                if m.item(text).clicked() {
                                    // まとまりにほかのパネルがあれば、そのまとまりの隣。1 つだけなら全体の端
                                    let to = if alone { DockTo::Edge { side, share: 0.3 } } else { DockTo::Split { of: p.clone(), side, share: 0.5 } };
                                    menu_moves.push((p.clone(), to));
                                }
                            }
                        });
                    }
                    if !others.is_empty() {
                        m.submenu("Move into group", |m| {
                            for (with, text) in &others {
                                if m.item(text.clone()).clicked() {
                                    menu_moves.push((p.clone(), DockTo::Tab { with: with.clone(), index: None }));
                                }
                            }
                        });
                    }
                    if at == GroupAt::Docked && m.item("Float").clicked() {
                        let [x, y, w, h] = float_at;
                        menu_moves.push((p.clone(), DockTo::Float { x, y, w, h }));
                    }
                    if let Some([x, y, w, h]) = window_at
                        && m.item("Move to a new window").clicked()
                    {
                        menu_moves.push((p.clone(), DockTo::Window { x, y, w, h }));
                    }
                    if closable {
                        m.separator();
                        if m.item("Close").clicked() {
                            menu_close = Some(p.clone());
                        }
                    }
                });
                if st.closable {
                    let b = row.icon_button(Icon::Close, format!("Close {title}"));
                    let clicked = b.clicked;
                    // 小さい形にし、Tab では止まらない（キーボードは Delete とメニューで閉じる）
                    let info = &mut b.h.tree.info[b.h.idx];
                    info.skip_tab = true;
                    if let Kind::Button { small, .. } = &mut info.kind {
                        *small = true;
                    }
                    if clicked {
                        menu_close = Some(panel.clone());
                    }
                }
            }
        }
        for (k, to) in menu_moves {
            st.changes.push(Change::Move(k, to));
        }
        if let Some(k) = menu_dock {
            st.changes.push(Change::DockBack(k));
        }
        if let Some(k) = menu_close {
            st.closed = Some(k);
        }
        // 中身（選んでいるパネルの題で読み上げる）
        let mut ui = self.child(gi);
        let (p, _) = ui.add(Kind::TabPanel, Dir::Column);
        ui.tree.info[p].label = Some((st.title)(&panels[picked]));
        let (pad, gap) = (ui.px(8.0), ui.px(10.0));
        ui.tree.layout[p].pad = (pad, pad);
        ui.tree.layout[p].gap = gap;
        ui.tree.layout[p].align = Align::Stretch;
        ui.tree.layout[p].height = Size::Fill(1.0);
        content(&mut ui.child(p), &panels[picked]);
    }
}

/// 浮いたまとまりのパネルと見せているパネルの位置
fn float_tabs<K: Clone>(tabs: &Node<K>) -> Option<(Vec<K>, usize)> {
    match tabs {
        Node::Tabs { panels, selected } if !panels.is_empty() => Some((panels.clone(), *selected)),
        _ => None,
    }
}

/// 描くまとまりの最初のパネル。Ui::dock がまとまりを数える順（落とす先の計算と同じ）: 主の窓は並べ方の木、全体の中の
/// 浮いた枠（重なりの順）。別の窓（own）は、その窓のまとまりだけ
fn drawn_firsts<K: Clone + PartialEq>(dock: &Dock<K>, own: Option<usize>) -> Vec<K> {
    let first = |n: &Node<K>| float_tabs(n).map(|(p, _)| p[0].clone());
    match own {
        Some(i) => first(&dock.floating[i].tabs).into_iter().collect(),
        None => {
            let mut out = Vec::new();
            if let Some(r) = &dock.root {
                r.walk(&mut |n| out.extend(first(n)));
            }
            out.extend(dock.frames().into_iter().filter_map(|i| first(&dock.floating[i].tabs)));
            out
        }
    }
}

/// まとまりを新しい窓に出す時の、窓の場所と大きさ。r は前のフレームのまとまりの場所（物理ピクセル）。窓の外枠の左上を
/// まとまりの左上（画面）に合わせ、中の大きさはまとまりの大きさにする。場所が分からなければ主の窓の中の左上の近く
fn window_rect(place: Option<WindowPlace>, r: Option<Rect>, scale: f32) -> [f32; 4] {
    let p = place.unwrap_or(WindowPlace { outer: (0.0, 0.0), client: (0.0, 0.0), unit: 1.0 / scale });
    match r {
        Some(r) => [p.client.0 + r.x * p.unit, p.client.1 + r.y * p.unit, (r.w / scale).max(FLOAT_MIN.0), (r.h / scale).max(FLOAT_MIN.1)],
        None => [p.client.0 + 40.0, p.client.1 + 40.0, 360.0, 260.0],
    }
}

impl<K: Clone + PartialEq + Hash + 'static> DockArea<'_, '_, '_, K> {
    /// Takes the remaining space along the parent's direction (usually what a dock wants).
    ///
    /// ```
    /// # use kargui::Dock;
    /// # let mut dock = Dock::new(["Files"]);
    /// # kargui::test::example(|ui| {
    /// ui.dock(&mut dock).grow().show(|p| p.to_string(), |ui, p| {
    ///     ui.label(*p);
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

    /// Gives each tab a close button (and Close in its menu, and the Delete key). Closing only
    /// asks: [`DockArea::show`] returns the panel, and the app takes it out with
    /// [`Dock::remove`] (or keeps it, for example after asking to save).
    ///
    /// ```
    /// # use kargui::Dock;
    /// # let mut dock = Dock::new(["Files", "Search"]);
    /// # kargui::test::example(|ui| {
    /// if let Some(p) = ui.dock(&mut dock).closable().show(|p| p.to_string(), |ui, p| {
    ///     ui.label(*p);
    /// }) {
    ///     dock.remove(&p);
    /// }
    /// # });
    /// ```
    pub fn closable(mut self) -> Self {
        self.closable = true;
        self
    }

    /// Builds the dock: `title` names each panel (on its tab), and `content` builds the selected
    /// panel of each group. Returns the panel the user asked to close, if any (see
    /// [`DockArea::closable`]).
    ///
    /// ```
    /// # use kargui::Dock;
    /// # let mut dock = Dock::new(["Files"]);
    /// # kargui::test::example(|ui| {
    /// let closed = ui.dock(&mut dock).show(|p| p.to_string(), |ui, p| {
    ///     ui.label(*p);
    /// });
    /// assert!(closed.is_none());
    /// # });
    /// ```
    pub fn show(self, title: impl Fn(&K) -> String, mut content: impl FnMut(&mut Ui, &K)) -> Option<K> {
        let DockArea { ui, dock, idx, id, closable } = self;
        let scale = ui.cx.scale;
        // 別の窓の中なら、その窓に出したまとまり（own）だけを描く。主の窓は、並べ方の木と全体の中の浮いた枠
        let in_window = ui.cx.window_key.clone();
        let own = match &in_window {
            None => {
                dock.area = id.0;
                None
            }
            Some(key) => Some(dock.window_index(key)?),
        };
        // 前のフレームの全体の大きさ（論理ピクセル。浮いたまとまりを中に収める）
        let bounds = ui.cx.prev.docks.get(&id).map(|g| (g.root.w / scale, g.root.h / scale));
        // メニューの「Float」で出す場所: 全体の真ん中に、浮いた枠の数だけずらす
        let (bw, bh) = bounds.unwrap_or((640.0, 480.0));
        let (fw, fh) = ((bw * 0.4).clamp(FLOAT_MIN.0, 480.0), (bh * 0.5).clamp(FLOAT_MIN.1, 360.0));
        let shift = 24.0 * dock.frames().len() as f32;
        let float_at = [((bw - fw) / 2.0 + shift).min(bw - fw).max(0.0), ((bh - fh) / 2.0 + shift).min(bh - fh).max(0.0), fw, fh];
        if own.is_none() {
            // 窓を置けない所（Wayland）では、別の窓に出したまとまり（読み込んだ物など）を全体の中の枠に戻す
            if !ui.cx.places_windows && dock.windows_to_frames(float_at) {
                crate::log::event("dock.windows_to_frames", "", "places_windows=false");
            }
            // 別の窓に出したまとまりの窓を頼む（後ろから。戻すと番号がずれるので）。利用者が窓を閉じたら、そのまとまりの
            // パネルを元の場所へ戻す（パネルは消さない）
            for i in (0..dock.floating.len()).rev() {
                if !dock.floating[i].window {
                    continue;
                }
                let Some((panels, selected)) = float_tabs(&dock.floating[i].tabs) else { continue };
                let key = dock.window_key(i);
                let [x, y, w, h] = dock.floating[i].rect;
                let options = crate::Options { size: (w, h), position: Some((x, y)), ..crate::Options::new(title(&panels[selected])) };
                let mut open = true;
                ui.window(&key, &options, &mut open);
                if !open {
                    crate::log::event("dock.window_closed", &format!("key={key}"), &format!("panels={}", panels.len()));
                    for k in &panels {
                        dock.dock_back(k);
                    }
                }
            }
        }
        // 別の窓の場所と大きさを覚える（利用者が動かしたら、保存する並べ方に入る）
        if let (Some(i), Some(p)) = (own, ui.cx.place)
            && ui.cx.width >= 1.0
            && ui.cx.height >= 1.0
        {
            let before = dock.floating[i].rect;
            dock.set_float_rect(i, [p.outer.0, p.outer.1, ui.cx.width / scale, ui.cx.height / scale]);
            if dock.floating[i].rect != before {
                crate::log::event("dock.window_place", &format!("key={}", in_window.as_deref().unwrap_or_default()), &format!("rect={:?}", dock.floating[i].rect));
            }
        }
        // 落としたタブ: 前のフレームの場所から落とす先を決め、作る前に並べ方を変える（受け取る部品として毎フレーム届け出る）
        let at = ui.cx.drop.as_ref().map(|d| d.at);
        let got = ui.handle(idx, id).dropped::<DockDrag<K>>();
        if let (Some(d), Some(at)) = (got, at)
            && d.dock == id
        {
            let zone = ui.cx.prev.docks.get(&id).and_then(|g| g.zone_for(at.0, at.1, ui.cx.scale, tab_id(id, &d.panel)));
            // まとまりの最初のパネル（落とす先の計算と同じ順）
            let firsts = drawn_firsts(dock, own);
            let to = match zone {
                Some(Zone::Tab { group, index }) => firsts.get(group).map(|w| DockTo::Tab { with: w.clone(), index }),
                Some(Zone::Split { group, side }) => firsts.get(group).map(|w| DockTo::Split { of: w.clone(), side, share: 0.5 }),
                Some(Zone::Edge(side)) => Some(DockTo::Edge { side, share: EDGE_SHARE }),
                None => None,
            };
            let ok = to.is_some_and(|to| dock.insert(d.panel.clone(), to));
            crate::log::event("dock.drop", &format!("tab={:x} at={:.0},{:.0}", tab_id(id, &d.panel).0, at.0, at.1), &format!("zone={zone:?} ok={ok}"));
            if ok {
                ui.cx.focus = Some(tab_id(id, &d.panel));
            }
        }
        // 押したタブを選ぶ（タブの id はパネルから作るので、前のフレームと同じ）
        if !ui.disabled
            && let Some(c) = ui.cx.clicked
            && let Some(k) = dock.panels().into_iter().find(|k| tab_id(id, *k) == c).cloned()
            && dock.select(&k)
        {
            crate::log::event("dock.select", &format!("tab={:x}", c.0), "click");
        }
        if !ui.disabled && own.is_none() {
            // 浮いた枠の上を押したら、いちばん上にする
            if let Some(p) = ui.cx.press_now
                && let Some(geom) = ui.cx.prev.docks.get(&id)
                && let Some(g) = geom.group_at(p.0, p.1).filter(|&g| geom.groups[g].floating)
            {
                let docked = geom.groups.iter().filter(|g| !g.floating).count();
                if let Some(&fi) = dock.frames().get(g - docked)
                    && dock.raise(fi)
                {
                    crate::log::event("dock.raise", &format!("at={:.0},{:.0}", p.0, p.1), &format!("float={fi}"));
                }
            }
            // 印を押している間: 押した時の場所と大きさに、ポインターの動いた分を足す（全体の中に収める）
            for fi in dock.frames() {
                let Some((panels, _)) = float_tabs(&dock.floating[fi].tabs) else { continue };
                for resize in [false, true] {
                    let gid = grip_id(id, &panels[0], resize);
                    let cx = &mut *ui.cx;
                    if cx.pressed != Some(gid) && cx.released != Some(gid) {
                        continue;
                    }
                    let Some(p) = cx.pointer else { continue };
                    let (start, from) = match cx.float_grab {
                        Some((g, r, at)) if g == gid => (r, at),
                        _ => {
                            let grab = (dock.floating[fi].rect, cx.press_at.unwrap_or(p));
                            cx.float_grab = Some((gid, grab.0, grab.1));
                            crate::log::event(if resize { "dock.float_resize_start" } else { "dock.float_move_start" }, &format!("float={fi}"), &format!("rect={:?}", grab.0));
                            grab
                        }
                    };
                    let (dx, dy) = ((p.0 - from.0) / scale, (p.1 - from.1) / scale);
                    let [x, y, w, h] = start;
                    let mut r = if resize { [x, y, w + dx, h + dy] } else { [x + dx, y + dy, w, h] };
                    if let Some((bw, bh)) = bounds {
                        if resize {
                            r[2] = r[2].min(bw - x);
                            r[3] = r[3].min(bh - y);
                        } else {
                            r[0] = r[0].min(bw - w).max(0.0);
                            r[1] = r[1].min(bh - h).max(0.0);
                        }
                    }
                    dock.set_float_rect(fi, r);
                    if cx.released == Some(gid) {
                        cx.float_grab = None;
                        crate::log::event(if resize { "dock.float_resize" } else { "dock.float_move" }, &format!("float={fi}"), &format!("rect={:?}", dock.floating[fi].rect));
                    }
                }
            }
        }
        let root = dock.root.clone();
        // 全体の中に描く浮いた枠（重なりの順。別の窓の中では無し）
        let frames: Vec<usize> = if own.is_some() { Vec::new() } else { dock.frames() };
        let floats: Vec<(Vec<K>, usize, [f32; 4])> = frames.iter().filter_map(|&i| float_tabs(&dock.floating[i].tabs).map(|(p, s)| (p, s, dock.floating[i].rect))).collect();
        let names = dock.groups().into_iter().filter_map(float_tabs).map(|(p, s)| (p[s].clone(), title(&p[s]))).collect();
        // 「Move to a new window」で出す窓の場所（主の窓で、窓を置ける時だけ）。まとまりの前のフレームの場所から
        let window_at = (own.is_none() && ui.cx.places_windows).then(|| {
            let geom = ui.cx.prev.docks.get(&id);
            (0..drawn_firsts(dock, None).len()).map(|g| window_rect(ui.cx.place, geom.and_then(|gm| gm.groups.get(g)).map(|gg| gg.rect), scale)).collect()
        });
        let mut st = Build { dock: id, closable, title: &title, changes: Vec::new(), closed: None, groups: Vec::new(), names, float_at, window_at };
        // 並べ方の木（重ねる入れ物いっぱい）と、その上に浮いたまとまりの枠。別の窓の中は、その窓のまとまりを窓いっぱいに
        let (tree_box, _) = ui.child(idx).push(Id(hash_of((id.0, "docked"))), Kind::Column, Dir::Column);
        match own {
            Some(i) => {
                if let Some((panels, selected)) = float_tabs(&dock.floating[i].tabs) {
                    ui.child(tree_box).dock_group(&panels, selected, GroupAt::Window, &mut st, &mut content);
                }
            }
            None => {
                if let Some(r) = &root {
                    ui.child(tree_box).dock_node(r, 1, &mut st, &mut content);
                }
            }
        }
        let docked = st.groups.len();
        let line = scale.max(1.0).round();
        for (panels, selected, [x, y, w, h]) in &floats {
            let (fr, _) = ui.child(idx).push(Id(hash_of((id.0, hash_of(&panels[0]), "float"))), Kind::DockFloat, Dir::Stack);
            {
                let l = &mut ui.tree.layout[fr];
                l.at = Some((x * scale, y * scale));
                l.width = Size::Fixed((w * scale).round());
                l.height = Size::Fixed((h * scale).round());
                l.pad = (line, line);
                l.align = Align::Stretch;
            }
            let mut fu = ui.child(fr);
            fu.dock_group(panels, *selected, GroupAt::Frame, &mut st, &mut content);
            // 大きさを変える印は右下の隅（位置を大きな数にすると、枠の中の右下へ寄る）
            let (g, _) = fu.push(grip_id(id, &panels[0], true), Kind::DockGrip { resize: true }, Dir::Leaf);
            let s = fu.px(16.0);
            let l = &mut fu.tree.layout[g];
            (l.width, l.height, l.at) = (Size::Fixed(s), Size::Fixed(s), Some((f32::MAX, f32::MAX)));
        }
        // 焦点のあるまとまり（中身を作り終えたので、中身の部品の中かも分かる）
        let focused = if ui.disabled { None } else { ui.cx.focus.and_then(|f| st.groups.iter().position(|(gi, _, _)| descends(ui.tree, *gi, f))) };
        if let Some(g) = focused {
            // Ctrl+Tab・Ctrl+PageDown で次のタブ、Ctrl+Shift+Tab・Ctrl+PageUp で前のタブ（中の部品が使わなかった時）
            let mut tab_step = 0i32;
            // F6・Shift+F6 で次（前）のまとまりの、見せているタブへ
            let mut group_step = 0i32;
            ui.cx.app_keys.retain(|&(key, m)| {
                let (c, s, a) = (m.ctrl, m.shift, m.alt);
                match (key, c, s, a) {
                    (Key::Tab, true, false, false) | (Key::PageDown, true, false, false) => tab_step += 1,
                    (Key::Tab, true, true, false) | (Key::PageUp, true, false, false) => tab_step -= 1,
                    (Key::F6, false, false, false) => group_step += 1,
                    (Key::F6, false, true, false) => group_step -= 1,
                    _ => return true,
                }
                false
            });
            let (_, panels, picked) = &st.groups[g];
            if tab_step != 0 && panels.len() > 1 {
                let next = (*picked as i32 + tab_step).rem_euclid(panels.len() as i32) as usize;
                let k = panels[next].clone();
                ui.cx.focus = Some(tab_id(id, &k));
                ui.cx.focus_visible = true;
                st.changes.push(Change::Select(k));
                ui.cx.again = true;
            }
            if group_step != 0 && st.groups.len() > 1 {
                // 回る順は、木のまとまり、浮いたまとまり（浮かせた順）。移った浮いたまとまりを上にすると重なりの順が
                // 変わるので、重なりの順で回るとまだ回っていない物を飛ばす
                let order: Vec<usize> = (0..docked).chain(dock.float_order().into_iter().filter_map(|fi| frames.iter().position(|&x| x == fi)).map(|j| docked + j)).collect();
                let n = order.len() as i32;
                let at = order.iter().position(|&x| x == g).unwrap_or(0) as i32;
                let to = order[(at + group_step).rem_euclid(n) as usize];
                let (_, panels, picked) = &st.groups[to];
                ui.cx.focus = Some(tab_id(id, &panels[*picked]));
                ui.cx.focus_visible = true;
                crate::log::event("dock.next_group", &format!("from={g} step={group_step}"), &format!("to={to}"));
                // 浮いた枠へ移ったら、いちばん上にする
                if to >= docked {
                    st.changes.push(Change::Raise(frames[to - docked]));
                }
            }
        }
        // 集めた変更を当てる。並べ方が変わったらもう一度回す（新しい並べ方をこのフレームで見せる）
        let mut moved = false;
        for c in st.changes {
            match c {
                Change::Select(k) => {
                    dock.select(&k);
                }
                Change::Share(path, s) => {
                    dock.set_share(path, s);
                    crate::log::event("dock.share", &format!("path={path}"), &format!("share={s:.3}"));
                }
                Change::Move(k, to) => {
                    let what = match &to {
                        DockTo::Tab { .. } => "tab",
                        DockTo::Split { .. } => "split",
                        DockTo::Edge { .. } => "edge",
                        DockTo::Float { .. } => "float",
                        DockTo::Window { .. } => "window",
                    };
                    let ok = dock.insert(k.clone(), to);
                    crate::log::event("dock.move", &format!("tab={:x} to={what}", tab_id(id, &k).0), &format!("ok={ok}"));
                    if ok {
                        ui.cx.focus = Some(tab_id(id, &k));
                        moved = true;
                    }
                }
                Change::DockBack(k) => {
                    let ok = dock.dock_back(&k);
                    crate::log::event("dock.dock_back", &format!("tab={:x}", tab_id(id, &k).0), &format!("ok={ok}"));
                    if ok {
                        ui.cx.focus = Some(tab_id(id, &k));
                        moved = true;
                    }
                }
                Change::Raise(i) => {
                    moved |= dock.raise(i);
                }
            }
        }
        if moved {
            ui.cx.again = true;
        }
        if let Some(k) = &st.closed {
            crate::log::event("dock.close", &format!("tab={:x}", tab_id(id, k).0), "asked");
        }
        st.closed
    }
}
