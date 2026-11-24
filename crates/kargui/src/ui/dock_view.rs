//! ドッキングの部品（Ui::dock）。並べ方は Dock<K>（dock.rs）が持ち、ここは毎フレーム木をたどって部品を作る。
//! 押したタブは作る前に選び、キー・メニュー・仕切りの変更は作り終えてから Dock に当てる（変わったらもう一度回す）

use std::hash::Hash;

use super::dock::{Dock, DockSide, DockTo, EDGE_SHARE, Node, Zone};
use super::layout::{Align, Dir, Size};
use super::{Id, Kind, Ui, descends, hash_of};
use crate::icon::Icon;
use crate::input::{Event, Key, Mods};

/// 1 回のビューで集めて、作り終えてから当てる変更
enum Change<K> {
    Select(K),
    Move(K, DockTo<K>),
    /// 分割の道（根が 1）と、新しい割合
    Share(u64, f32),
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
    /// - go to the next (previous) group with F6 (Shift+F6);
    /// - move a panel to a new group beside its own, or into another group, from the tab's menu
    ///   (right click, Shift+F10 or the context menu key).
    ///
    /// Screen readers hear each group as a tab list with tabs and a tab panel, and the dividers
    /// as separators with their position in percent.
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
        let (idx, id) = self.add(Kind::Dock, Dir::Column);
        self.tree.layout[idx].align = Align::Stretch;
        DockArea { ui: self, dock, idx, id, closable: false }
    }

    /// 木の節を 1 つ作る。path は分割の道（根が 1、子は 2p と 2p+1）
    fn dock_node<K: Clone + PartialEq + Hash + 'static>(&mut self, node: &Node<K>, path: u64, st: &mut Build<K>, content: &mut dyn FnMut(&mut Ui, &K)) {
        match node {
            Node::Tabs { panels, selected } => self.dock_group(panels, *selected, st, content),
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

    /// タブのまとまりを 1 つ作る: 見出しの帯と、選んでいるパネルの中身
    fn dock_group<K: Clone + PartialEq + Hash + 'static>(&mut self, panels: &[K], selected: usize, st: &mut Build<K>, content: &mut dyn FnMut(&mut Ui, &K)) {
        let dock = st.dock;
        let (gi, _) = self.push(Id(hash_of((dock.0, hash_of(&panels[0]), "group"))), Kind::DockGroup, Dir::Column);
        self.fill_parent(gi);
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
        st.groups.push((gi, panels.to_vec(), picked));
        // 見出しの帯
        let mut menu_moves: Vec<(K, DockTo<K>)> = Vec::new();
        let mut menu_close = None;
        {
            let mut ui = self.child(gi);
            let (list, _) = ui.add(Kind::TabList, Dir::Row);
            let gap = ui.px(4.0);
            ui.tree.layout[list].gap = gap;
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
                // 見出しを引っ張ると、ほかのまとまりや端へ動かせる（4 画素動かしてから。押しただけなら選ぶ）
                let payload = DockDrag { dock, panel: panel.clone() };
                row.handle(ti, tid).drag_source(payload).context_menu(|m| {
                    m.submenu("Move to a new group", |m| {
                        for (text, side) in [("Left", DockSide::Left), ("Right", DockSide::Right), ("Above", DockSide::Top), ("Below", DockSide::Bottom)] {
                            if m.item(text).clicked() {
                                // まとまりにほかのパネルがあれば、そのまとまりの隣。1 つだけなら全体の端
                                let to = if alone { DockTo::Edge { side, share: 0.3 } } else { DockTo::Split { of: p.clone(), side, share: 0.5 } };
                                menu_moves.push((p.clone(), to));
                            }
                        }
                    });
                    if !others.is_empty() {
                        m.submenu("Move into group", |m| {
                            for (with, text) in &others {
                                if m.item(text.clone()).clicked() {
                                    menu_moves.push((p.clone(), DockTo::Tab { with: with.clone(), index: None }));
                                }
                            }
                        });
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
        // 落としたタブ: 前のフレームの場所から落とす先を決め、作る前に並べ方を変える（受け取る部品として毎フレーム届け出る）
        let at = ui.cx.drop.as_ref().map(|d| d.at);
        let got = ui.handle(idx, id).dropped::<DockDrag<K>>();
        if let (Some(d), Some(at)) = (got, at)
            && d.dock == id
        {
            let zone = ui.cx.prev.docks.get(&id).and_then(|g| g.zone_at(at.0, at.1, ui.cx.scale));
            let mut firsts = Vec::new();
            if let Some(r) = &dock.root {
                r.walk(&mut |n| {
                    if let Node::Tabs { panels, .. } = n {
                        firsts.push(panels[0].clone());
                    }
                });
            }
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
        let root = dock.root.clone();
        let mut names = Vec::new();
        if let Some(r) = &root {
            r.walk(&mut |n| {
                if let Node::Tabs { panels, selected } = n {
                    let k = &panels[*selected];
                    names.push((k.clone(), title(k)));
                }
            });
        }
        let mut st = Build { dock: id, closable, title: &title, changes: Vec::new(), closed: None, groups: Vec::new(), names };
        if let Some(r) = &root {
            ui.child(idx).dock_node(r, 1, &mut st, &mut content);
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
                let n = st.groups.len() as i32;
                let to = (g as i32 + group_step).rem_euclid(n) as usize;
                let (_, panels, picked) = &st.groups[to];
                ui.cx.focus = Some(tab_id(id, &panels[*picked]));
                ui.cx.focus_visible = true;
                crate::log::event("dock.next_group", &format!("from={g} step={group_step}"), &format!("to={to}"));
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
                    let ok = dock.insert(k.clone(), to);
                    crate::log::event("dock.move", &format!("tab={:x}", tab_id(id, &k).0), &format!("ok={ok}"));
                    if ok {
                        ui.cx.focus = Some(tab_id(id, &k));
                        moved = true;
                    }
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
