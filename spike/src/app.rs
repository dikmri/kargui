//! 試作の画面（タスク帳の形）。状態・配置・描く物の一覧・当たり判定・読み上げの木を作る。
//! 段階 1 では、この部分がフレームワークの書き方（即時モード）に置き換わる。
//! 位置は物理ピクセル。大きさは論理ピクセル × 拡大率で決める。

use accesskit::{Action, Live, Node, NodeId, Rect as AkRect, Role, Toggled, TreeId, TreeInfo, TreeUpdate};
use taskcore::{ROW_HEIGHT, Store, group};

use crate::draw::{Color, DrawList, Rect, rgb};
use crate::text::{Laid, Text};

pub const BG: Color = rgb(0xffffff);
const TEXT: Color = rgb(0x1f2328);
const SUB: Color = rgb(0x59636e);
const BORDER: Color = rgb(0xc8d1da);
const ACCENT: Color = rgb(0x0969da);
const ROW_LINE: Color = rgb(0xeef1f4);
const DONE: Color = rgb(0x8c959f);
const BUTTON: Color = rgb(0xf6f8fa);
const WHITE: Color = rgb(0xffffff);

/// 文字の大きさ（論理ピクセル）
const BODY: f32 = 14.0;
const SMALL: f32 = 12.5;
const HEADING: f32 = 21.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Input,
    Add,
}

/// クリックや読み上げの操作の行き先
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Input,
    Add,
    Check(u32),
    Delete(u32),
    List,
    Nothing,
}

pub struct Layout {
    pub heading: Rect,
    pub input: Rect,
    pub add: Rect,
    pub info: Rect,
    pub list: Rect,
    pub status: Rect,
}

pub struct App {
    pub store: Store,
    pub input: String,
    /// 変換中の文字（まだ確定していない）
    pub preedit: String,
    pub focus: Option<Focus>,
    /// 一覧のスクロールの位置（論理ピクセル）
    pub scroll: f32,
    pub message: String,
    pub renderer: String,
    pub scale: f32,
    pub width: f32,
    pub height: f32,
    /// 文字を打つ位置（物理ピクセル、窓の中）。日本語入力の候補の窓を出す位置に使う
    pub caret: Rect,
}

fn put(d: &mut DrawList, laid: &Laid, x: f32, y: f32, color: Color) {
    for g in &laid.glyphs {
        let gx = (x + g.x).round() + g.slot.left as f32;
        let gy = (y + g.y).round() + g.slot.top as f32;
        d.glyph(gx, gy, g.slot.w as f32, g.slot.h as f32, g.slot.x as f32, g.slot.y as f32, color);
    }
}

fn ak(r: Rect) -> AkRect {
    AkRect { x0: r.x as f64, y0: r.y as f64, x1: r.right() as f64, y1: r.bottom() as f64 }
}

pub const ROOT: NodeId = NodeId(1);
const HEADING_ID: NodeId = NodeId(2);
const INPUT_ID: NodeId = NodeId(3);
const ADD_ID: NodeId = NodeId(4);
const INFO_ID: NodeId = NodeId(5);
const LIST_ID: NodeId = NodeId(6);
const STATUS_ID: NodeId = NodeId(7);
const ROW_BASE: u64 = 1_000;

fn row_id(task: u32, part: u64) -> NodeId {
    NodeId(ROW_BASE + task as u64 * 4 + part)
}

/// 読み上げの木の番号から、操作の行き先を求める
pub fn target_of(id: NodeId) -> Target {
    match id {
        INPUT_ID => Target::Input,
        ADD_ID => Target::Add,
        NodeId(n) if n >= ROW_BASE => {
            let task = ((n - ROW_BASE) / 4) as u32;
            match (n - ROW_BASE) % 4 {
                1 => Target::Check(task),
                2 => Target::Delete(task),
                _ => Target::List,
            }
        }
        _ => Target::Nothing,
    }
}

impl App {
    pub fn new(store: Store, renderer: &str) -> Self {
        Self {
            store,
            input: String::new(),
            preedit: String::new(),
            focus: Some(Focus::Input),
            scroll: 0.0,
            message: "準備できました".into(),
            renderer: renderer.into(),
            scale: 1.0,
            width: 0.0,
            height: 0.0,
            caret: Rect::default(),
        }
    }

    pub fn layout(&self) -> Layout {
        let s = self.scale;
        let (w, h) = (self.width, self.height);
        let m = 16.0 * s;
        let add_w = 72.0 * s;
        let input = Rect::new(m, 50.0 * s, (w - 2.0 * m - add_w - 8.0 * s).max(0.0), 34.0 * s);
        let footer = 36.0 * s;
        let top = 122.0 * s;
        Layout {
            heading: Rect::new(m, 10.0 * s, w - 2.0 * m, 34.0 * s),
            add: Rect::new(input.right() + 8.0 * s, input.y, add_w, input.h),
            input,
            info: Rect::new(m, 92.0 * s, w - 2.0 * m, 24.0 * s),
            list: Rect::new(0.0, top, w, (h - top - footer).max(0.0)),
            status: Rect::new(m, h - footer, w - 2.0 * m, footer),
        }
    }

    fn row_h(&self) -> f32 {
        (ROW_HEIGHT * self.scale).round()
    }

    /// 描くときのスクロールの位置（物理ピクセル、整数）
    pub fn scroll_px(&self) -> f32 {
        (self.scroll * self.scale).round()
    }

    pub fn max_scroll(&self) -> f32 {
        let l = self.layout();
        let total = self.store.tasks().len() as f32 * ROW_HEIGHT;
        (total - l.list.h / self.scale).max(0.0)
    }

    pub fn set_scroll(&mut self, v: f32) {
        self.scroll = v.clamp(0.0, self.max_scroll());
    }

    /// 見えている行の範囲（tasks() の添字）
    fn visible(&self, l: &Layout) -> std::ops::Range<usize> {
        let n = self.store.tasks().len();
        let row_h = self.row_h();
        let off = self.scroll_px();
        let first = ((off / row_h).floor() as usize).min(n);
        let last = (((off + l.list.h) / row_h).ceil() as usize).min(n);
        first..last
    }

    fn row_rect(&self, l: &Layout, i: usize) -> Rect {
        let row_h = self.row_h();
        Rect::new(0.0, l.list.y + i as f32 * row_h - self.scroll_px(), self.width, row_h)
    }

    fn check_rect(&self, row: Rect) -> Rect {
        let s = self.scale;
        let size = 16.0 * s;
        Rect::new(26.0 * s, row.y + (row.h - size) / 2.0, size, size)
    }

    fn delete_rect(&self, row: Rect) -> Rect {
        let s = self.scale;
        let (w, h) = (52.0 * s, 24.0 * s);
        Rect::new(row.right() - 24.0 * s - w, row.y + (row.h - h) / 2.0, w, h)
    }

    /// 画面全体の描く物の一覧を作り直す
    pub fn build(&mut self, text: &mut Text, d: &mut DrawList) {
        d.clear();
        let s = self.scale;
        let l = self.layout();
        let line = s.max(1.0).round();

        // 1) 一覧（見えている行だけ）
        for i in self.visible(&l) {
            let task = &self.store.tasks()[i];
            let row = self.row_rect(&l, i);
            d.fill(Rect::new(16.0 * s, row.bottom() - line, self.width - 32.0 * s, line), ROW_LINE, 0.0);
            let cb = self.check_rect(row);
            if task.done {
                d.fill(cb, ACCENT, 3.0 * s);
                let mark = text.layout("✓", 12.0 * s, true);
                put(d, &mark, cb.x + (cb.w - mark.width) / 2.0, cb.y + (cb.h - mark.height) / 2.0, WHITE);
            } else {
                d.boxed(cb, WHITE, BORDER, line, 3.0 * s);
            }
            let laid = text.layout(&task.text, BODY * s, false);
            let tx = 54.0 * s;
            let ty = row.y + (row.h - laid.height) / 2.0;
            put(d, &laid, tx, ty, if task.done { DONE } else { TEXT });
            if task.done {
                d.fill(Rect::new(tx, ty + laid.height * 0.55, laid.width, line), DONE, 0.0);
            }
            let del = self.delete_rect(row);
            d.boxed(del, BUTTON, BORDER, line, 4.0 * s);
            let dl = text.layout("削除", SMALL * s, false);
            put(d, &dl, del.x + (del.w - dl.width) / 2.0, del.y + (del.h - dl.height) / 2.0, TEXT);
        }

        // 2) 上と下の帯（一覧のはみ出しを隠す）と区切りの線
        d.fill(Rect::new(0.0, 0.0, self.width, l.list.y), BG, 0.0);
        d.fill(Rect::new(0.0, l.status.y, self.width, l.status.h), BG, 0.0);
        d.fill(Rect::new(0.0, l.list.y - line, self.width, line), BORDER, 0.0);
        d.fill(Rect::new(0.0, l.status.y, self.width, line), BORDER, 0.0);

        // 3) 見出し
        let title = text.layout("タスク帳", HEADING * s, true);
        put(d, &title, l.heading.x, l.heading.y + (l.heading.h - title.height) / 2.0, TEXT);
        let sub = text.layout(&format!("kargui 試作 ・ {}", self.renderer), SMALL * s, false);
        put(d, &sub, l.heading.x + title.width + 12.0 * s, l.heading.y + (l.heading.h - sub.height) / 2.0 + 2.0 * s, SUB);

        // 4) 入力欄（確定した文字 ＋ 変換中の文字。変換中の文字には下線を引く）
        let focused = self.focus == Some(Focus::Input);
        let border = if focused { ACCENT } else { BORDER };
        d.boxed(l.input, WHITE, border, if focused { 2.0 * line } else { line }, 6.0 * s);
        let pad = 10.0 * s;
        let committed = text.layout(&self.input, BODY * s, false);
        let pre = text.layout(&self.preedit, BODY * s, false);
        let line_h = committed.height.max(pre.height).max(BODY * s * 1.4);
        let ty = l.input.y + (l.input.h - line_h) / 2.0;
        if self.input.is_empty() && self.preedit.is_empty() {
            let hint = text.layout("新しいタスクを入力して Enter", BODY * s, false);
            put(d, &hint, l.input.x + pad, ty, SUB);
        } else {
            put(d, &committed, l.input.x + pad, ty, TEXT);
            let px = l.input.x + pad + committed.width;
            put(d, &pre, px, ty, TEXT);
            if !self.preedit.is_empty() {
                d.fill(Rect::new(px, ty + line_h - 2.0 * line, pre.width, line), TEXT, 0.0);
            }
        }
        let caret_x = (l.input.x + pad + committed.width + pre.width).round();
        self.caret = Rect::new(caret_x, ty.round(), line, line_h.round());
        if focused {
            d.fill(Rect::new(caret_x, ty, (1.5 * s).round().max(1.0), line_h), TEXT, 0.0);
        }

        // 5) 追加ボタン（焦点があるときは外側に輪を描く）
        if self.focus == Some(Focus::Add) {
            d.fill(l.add.inset(-3.0 * s), ACCENT, 9.0 * s);
            d.fill(l.add.inset(-1.0 * s), WHITE, 7.0 * s);
        }
        d.fill(l.add, ACCENT, 6.0 * s);
        let at = text.layout("追加", BODY * s, true);
        put(d, &at, l.add.x + (l.add.w - at.width) / 2.0, l.add.y + (l.add.h - at.height) / 2.0, WHITE);

        // 6) 件数と、下の帯の知らせ
        let c = self.store.counts();
        let info = format!("{} 件（未完了 {} ・ 完了 {}）", group(c.total), group(c.active), group(c.done));
        let il = text.layout(&info, SMALL * s, false);
        put(d, &il, l.info.x, l.info.y + (l.info.h - il.height) / 2.0, SUB);
        let ml = text.layout(&self.message, SMALL * s, false);
        put(d, &ml, l.status.x, l.status.y + (l.status.h - ml.height) / 2.0, SUB);
    }

    /// 窓の中の (x, y) にある物
    pub fn hit(&self, x: f32, y: f32) -> Target {
        let l = self.layout();
        if l.input.contains(x, y) {
            return Target::Input;
        }
        if l.add.contains(x, y) {
            return Target::Add;
        }
        if l.list.contains(x, y) {
            for i in self.visible(&l) {
                let row = self.row_rect(&l, i);
                if !row.contains(x, y) {
                    continue;
                }
                let id = self.store.tasks()[i].id;
                if self.check_rect(row).inset(-6.0 * self.scale).contains(x, y) {
                    return Target::Check(id);
                }
                if self.delete_rect(row).contains(x, y) {
                    return Target::Delete(id);
                }
            }
            return Target::List;
        }
        Target::Nothing
    }

    /// 入力欄の文字でタスクを足す。足したら番号を返す
    pub fn add_from_input(&mut self) -> Option<u32> {
        let text = self.input.trim().to_string();
        let id = self.store.add(&self.input)?;
        self.message = format!("「{text}」を足しました");
        self.input.clear();
        Some(id)
    }

    pub fn toggle(&mut self, id: u32) -> Option<bool> {
        let done = self.store.toggle(id)?;
        self.message = if done { "完了にしました".into() } else { "未完了に戻しました".into() };
        Some(done)
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let ok = self.store.remove(id);
        if ok {
            self.message = "1 件を消しました".into();
            self.set_scroll(self.scroll);
        }
        ok
    }

    /// 読み上げの木（AccessKit）。一覧は見えている行だけを入れ、行ごとに「何件中何件目」を持たせる
    pub fn a11y_tree(&self) -> TreeUpdate {
        let l = self.layout();
        let mut nodes = Vec::new();
        let n = self.store.tasks().len();
        let mut rows = Vec::new();
        for i in self.visible(&l) {
            let task = &self.store.tasks()[i];
            let row = self.row_rect(&l, i);
            let (row_node, check_node, del_node) = (row_id(task.id, 0), row_id(task.id, 1), row_id(task.id, 2));
            let mut item = Node::new(Role::ListItem);
            item.set_label(task.text.clone());
            item.set_bounds(ak(row));
            item.set_size_of_set(n);
            item.set_position_in_set(i + 1);
            item.set_children(vec![check_node, del_node]);
            let mut check = Node::new(Role::CheckBox);
            check.set_label(task.text.clone());
            check.set_toggled(if task.done { Toggled::True } else { Toggled::False });
            check.set_bounds(ak(self.check_rect(row)));
            check.add_action(Action::Click);
            let mut del = Node::new(Role::Button);
            del.set_label(format!("削除: {}", task.text));
            del.set_bounds(ak(self.delete_rect(row)));
            del.add_action(Action::Click);
            nodes.push((row_node, item));
            nodes.push((check_node, check));
            nodes.push((del_node, del));
            rows.push(row_node);
        }
        let mut root = Node::new(Role::Window);
        root.set_label("タスク帳");
        root.set_children(vec![HEADING_ID, INPUT_ID, ADD_ID, INFO_ID, LIST_ID, STATUS_ID]);
        let mut heading = Node::new(Role::Heading);
        heading.set_label("タスク帳");
        heading.set_level(1);
        heading.set_bounds(ak(l.heading));
        let mut input = Node::new(Role::TextInput);
        input.set_label("新しいタスク");
        input.set_placeholder("新しいタスクを入力して Enter");
        input.set_value(self.input.clone());
        input.set_bounds(ak(l.input));
        input.add_action(Action::Focus);
        let mut add = Node::new(Role::Button);
        add.set_label("追加");
        add.set_bounds(ak(l.add));
        add.add_action(Action::Click);
        add.add_action(Action::Focus);
        let c = self.store.counts();
        let mut info = Node::new(Role::Label);
        info.set_value(format!("{} 件（未完了 {} ・ 完了 {}）", group(c.total), group(c.active), group(c.done)));
        info.set_bounds(ak(l.info));
        let mut list = Node::new(Role::List);
        list.set_label("タスクの一覧");
        list.set_bounds(ak(l.list));
        list.set_size_of_set(n);
        list.set_children(rows);
        let mut status = Node::new(Role::Status);
        status.set_value(self.message.clone());
        status.set_live(Live::Polite);
        status.set_bounds(ak(l.status));
        nodes.extend([
            (ROOT, root),
            (HEADING_ID, heading),
            (INPUT_ID, input),
            (ADD_ID, add),
            (INFO_ID, info),
            (LIST_ID, list),
            (STATUS_ID, status),
        ]);
        let focus = match self.focus {
            Some(Focus::Input) => INPUT_ID,
            Some(Focus::Add) => ADD_ID,
            None => ROOT,
        };
        TreeUpdate { nodes, tree: Some(TreeInfo::new(ROOT)), tree_id: TreeId::ROOT, focus }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(n: usize) -> App {
        let mut store = Store::new();
        store.generate(n);
        let mut a = App::new(store, "test");
        a.width = 900.0;
        a.height = 640.0;
        a
    }

    #[test]
    fn hit_finds_controls_and_row_parts() {
        let a = app(100);
        let l = a.layout();
        assert_eq!(a.hit(l.input.x + 5.0, l.input.y + 5.0), Target::Input);
        assert_eq!(a.hit(l.add.x + 5.0, l.add.y + 5.0), Target::Add);
        // 1 行目のチェックと削除
        let row = a.row_rect(&l, 0);
        let cb = a.check_rect(row);
        assert_eq!(a.hit(cb.x + 1.0, cb.y + 1.0), Target::Check(1));
        let del = a.delete_rect(row);
        assert_eq!(a.hit(del.x + 1.0, del.y + 1.0), Target::Delete(1));
        assert_eq!(a.hit(400.0, row.y + 5.0), Target::List);
    }

    #[test]
    fn scrolling_changes_which_row_is_hit_and_is_clamped() {
        let mut a = app(100);
        let l = a.layout();
        a.set_scroll(ROW_HEIGHT * 3.0);
        let row = a.row_rect(&l, 3);
        assert_eq!(row.y, l.list.y);
        let cb = a.check_rect(row);
        assert_eq!(a.hit(cb.x + 1.0, cb.y + 1.0), Target::Check(4));
        a.set_scroll(1.0e9);
        assert_eq!(a.scroll, a.max_scroll());
        a.set_scroll(-5.0);
        assert_eq!(a.scroll, 0.0);
    }

    #[test]
    fn a11y_tree_has_visible_rows_with_position_and_targets_round_trip() {
        let a = app(10_000);
        let tree = a.a11y_tree();
        let items: Vec<_> = tree.nodes.iter().filter(|(_, n)| n.role() == Role::ListItem).collect();
        // 一覧の高さ 482 / 行 32 → 16 行
        assert_eq!(items.len(), 16);
        assert_eq!(items[0].1.size_of_set(), Some(10_000));
        assert_eq!(items[0].1.position_in_set(), Some(1));
        assert_eq!(target_of(row_id(42, 1)), Target::Check(42));
        assert_eq!(target_of(row_id(42, 2)), Target::Delete(42));
        assert_eq!(target_of(INPUT_ID), Target::Input);
        assert_eq!(tree.focus, INPUT_ID);
    }

    #[test]
    fn add_trims_and_rejects_blank() {
        let mut a = app(0);
        a.input = "   ".into();
        assert_eq!(a.add_from_input(), None);
        a.input = " 牛乳を買う ".into();
        assert_eq!(a.add_from_input(), Some(1));
        assert!(a.input.is_empty());
        assert_eq!(a.message, "「牛乳を買う」を足しました");
    }
}
