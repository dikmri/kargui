//! 部品の大きさを測り、描く物の一覧を作る。次のフレームの当たり判定に使う情報（Prev）もここで作る。

use crate::geom::{Color, IRect, Rect};
use crate::paint::{DrawList, Scroll};
use crate::text::{Laid, TextStyle};
use crate::ime::ImeField;
use crate::ui::context::{Context, Hit, ListInfo, Prev};
use crate::ui::edit;
use crate::ui::layout::{self, Dir, Size};
use crate::ui::{Kind, TextKind, Tree};

/// 論理ピクセルの大きさ（拡大率を掛ける前）
mod sz {
    pub const BUTTON_H: f32 = 32.0;
    pub const BUTTON_SMALL_H: f32 = 26.0;
    pub const BUTTON_PAD: f32 = 14.0;
    pub const BUTTON_SMALL_PAD: f32 = 10.0;
    pub const BUTTON_MIN_W: f32 = 56.0;
    pub const BOX: f32 = 16.0;
    pub const BOX_GAP: f32 = 8.0;
    pub const TRACK_W: f32 = 36.0;
    pub const TRACK_H: f32 = 20.0;
    pub const SEGMENT_H: f32 = 30.0;
    pub const SEGMENT_PAD: f32 = 12.0;
    pub const INPUT_H: f32 = 34.0;
    pub const INPUT_W: f32 = 200.0;
    pub const INPUT_PAD: f32 = 10.0;
    pub const RADIUS: f32 = 6.0;
    pub const SLIDER_W: f32 = 200.0;
    pub const SLIDER_H: f32 = 24.0;
    pub const KNOB: f32 = 18.0;
    pub const SLIDER_TRACK: f32 = 4.0;
    pub const SCROLLBAR_W: f32 = 6.0;
    /// スクロールの棒の左に足す、押せる幅（棒は細いので）
    pub const SCROLLBAR_SLOP: f32 = 4.0;
    /// チェックとスイッチの当たりの余白（左右・上下）
    pub const SLOP_X: f32 = 12.0;
    pub const SLOP_Y: f32 = 8.0;
}

fn style(cx: &Context, kind: TextKind, bold: bool) -> TextStyle {
    TextStyle { size: (kind.size() * cx.scale).round(), bold }
}

fn px(cx: &Context, v: f32) -> f32 {
    (v * cx.scale).round()
}

/// 葉の部品の中身の大きさを測る
fn intrinsic(cx: &mut Context, kind: &Kind, max_w: Option<f32>) -> Option<(f32, f32)> {
    let s = cx.scale;
    let p = |v: f32| (v * s).round();
    Some(match kind {
        Kind::Label { text, size, bold, wrap, .. } => {
            let st = style(cx, *size, *bold);
            let laid = cx.text.layout(text, st, if *wrap { max_w } else { None });
            (if *wrap { 0.0 } else { laid.width.ceil() }, laid.height.ceil())
        }
        Kind::Button { text, small, .. } => {
            let st = style(cx, if *small { TextKind::Small } else { TextKind::Body }, false);
            let laid = cx.text.layout(text, st, None);
            let pad = p(if *small { sz::BUTTON_SMALL_PAD } else { sz::BUTTON_PAD });
            let min = p(if *small { sz::BUTTON_MIN_W * 0.8 } else { sz::BUTTON_MIN_W });
            ((laid.width.ceil() + 2.0 * pad).max(min), p(if *small { sz::BUTTON_SMALL_H } else { sz::BUTTON_H }))
        }
        Kind::Checkbox { text, .. } | Kind::Toggle { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            let lead = p(if matches!(kind, Kind::Toggle { .. }) { sz::TRACK_W } else { sz::BOX });
            let gap = if text.is_empty() { 0.0 } else { p(sz::BOX_GAP) };
            (lead + gap + laid.width.ceil(), laid.height.ceil().max(p(sz::TRACK_H)))
        }
        Kind::Segment { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            (laid.width.ceil() + 2.0 * p(sz::SEGMENT_PAD), p(sz::SEGMENT_H))
        }
        Kind::TextInput { .. } => (p(sz::INPUT_W), p(sz::INPUT_H)),
        Kind::Slider { .. } => (p(sz::SLIDER_W), p(sz::SLIDER_H)),
        _ => return None,
    })
}

/// スクロールの入れ物の右に空ける幅（論理ピクセル。棒の幅と、棒の左右のすき間）
pub(crate) const SCROLL_GUTTER: f32 = sz::SCROLLBAR_W + 3.0 + 4.0;

/// スライダーの溝の左端と長さ（つまみの半分ずつ内側）。押した位置から値を決める時も同じ値を使う
pub(crate) fn slider_track(r: Rect, scale: f32) -> (f32, f32) {
    let knob = (sz::KNOB * scale).round();
    (r.x + knob / 2.0, (r.w - knob).max(0.0))
}

/// 大きさを測って場所を決める。折り返す文字があれば、決まった幅で測り直してもう 1 回決める
pub(crate) fn layout(tree: &mut Tree, cx: &mut Context) {
    for i in 0..tree.info.len() {
        if let Some(size) = intrinsic(cx, &tree.info[i].kind, None) {
            tree.layout[i].intrinsic = size;
        }
    }
    let window = Rect::new(0.0, 0.0, cx.width, cx.height);
    layout::layout(&mut tree.layout, 0, window);
    let mut again = false;
    for i in 0..tree.info.len() {
        if let Kind::Label { wrap: true, .. } = tree.info[i].kind {
            let w = tree.layout[i].rect.w;
            if let Some(size) = intrinsic(cx, &tree.info[i].kind, Some(w)) {
                if (size.1 - tree.layout[i].intrinsic.1).abs() > 0.5 {
                    tree.layout[i].intrinsic = size;
                    again = true;
                }
            }
        }
    }
    if again {
        layout::layout(&mut tree.layout, 0, window);
    }
    // 中身が短くなってスクロールの位置が端を越えたら、端に戻してもう一度置く
    let mut clamped = false;
    for i in 0..tree.info.len() {
        if matches!(tree.info[i].kind, Kind::Scroll) {
            let l = &mut tree.layout[i];
            let max = (l.content - l.rect.h).max(0.0).round();
            if l.offset > max {
                l.offset = max;
                cx.scrolls.insert(tree.info[i].id, max / cx.scale);
                clamped = true;
            }
        }
    }
    if clamped {
        layout::layout(&mut tree.layout, 0, window);
    }
    // 頼まれた部品を、いちばん近いスクロールの入れ物の上端へ持ってくる
    if let Some(target) = cx.scroll_into_view.take() {
        if let Some(t) = tree.info.iter().position(|n| n.id == target) {
            let scroller = (0..tree.info.len()).rev().find(|&s| matches!(tree.info[s].kind, Kind::Scroll) && contains(tree, s, t));
            if let Some(s) = scroller {
                let l = &tree.layout[s];
                let max = (l.content - l.rect.h).max(0.0).round();
                let want = (l.offset + tree.layout[t].rect.y - (l.rect.y + l.pad.1)).clamp(0.0, max).round();
                if (want - l.offset).abs() > 0.5 {
                    tree.layout[s].offset = want;
                    cx.scrolls.insert(tree.info[s].id, want / cx.scale);
                    layout::layout(&mut tree.layout, 0, window);
                }
            }
        }
    }
}

/// a の下に t があるか（a 自身を含まない）
fn contains(tree: &Tree, a: usize, t: usize) -> bool {
    tree.layout[a].children.iter().any(|&c| c == t || contains(tree, c, t))
}

fn put(d: &mut DrawList, laid: &Laid, x: f32, y: f32, color: Color) {
    for g in &laid.glyphs {
        let gx = (x + g.x).round() + g.slot.left as f32;
        let gy = (y + g.y).round() + g.slot.top as f32;
        d.glyph(gx, gy, g.slot.w as f32, g.slot.h as f32, g.slot.x as f32, g.slot.y as f32, color);
    }
}

/// 文字を縦の真ん中にそろえて描き、並べた結果を返す
fn text_mid(d: &mut DrawList, cx: &mut Context, s: &str, st: TextStyle, x: f32, r: Rect, color: Color) -> std::rc::Rc<Laid> {
    let laid = cx.text.layout(s, st, None);
    put(d, &laid, x, (r.y + (r.h - laid.height) / 2.0).round(), color);
    laid
}

struct Painter<'a> {
    cx: &'a mut Context,
    d: &'a mut DrawList,
    prev: Prev,
    /// 今の切り取り範囲（当たり判定の範囲を切るため）
    clip: Rect,
    /// 今描いている一覧の行（一覧の id と行の範囲）
    row: Option<(crate::ui::Id, Rect)>,
    /// 今描いているスクロールの入れ物（焦点が移った時に、見えるようにスクロールするため）
    scroller: Option<crate::ui::Id>,
}

/// 画面の木を描く。次のフレームの当たり判定の情報を cx.prev に入れる
pub(crate) fn paint(tree: &Tree, cx: &mut Context, d: &mut DrawList) {
    let window = IRect::new(0, 0, cx.width as i32, cx.height as i32);
    d.begin(window);
    cx.caret = None;
    cx.preedit_rect = None;
    cx.input_rect = None;
    cx.ime_field = None;
    let mut p = Painter { cx, d, prev: Prev::default(), clip: window.to_rect(), row: None, scroller: None };
    p.node(tree, 0);
    p.d.finish();
    let prev = std::mem::take(&mut p.prev);
    cx.prev = prev;
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

impl Painter<'_> {
    fn px(&self, v: f32) -> f32 {
        px(self.cx, v)
    }

    fn line(&self) -> f32 {
        self.cx.scale.max(1.0).round()
    }

    fn hit(&mut self, tree: &Tree, i: usize, focusable: bool, text_input: bool) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        if focusable {
            if let Some(row) = self.row {
                self.prev.rows.insert(id, row);
            } else if let Some(s) = self.scroller {
                self.prev.rows.insert(id, (s, r));
            }
        }
        let Some(r) = intersect(r, self.clip) else {
            // 一覧の見えない行（上下に 1 行ずつ作る）とスクロールの外の部品は、押せないが Tab では移れる
            if focusable && (self.row.is_some() || self.scroller.is_some()) {
                self.prev.focus_order.push(id);
            }
            return;
        };
        // チェックとスイッチは小さいので、少し外れたクリックも当てる（切り取り範囲の外には広げない）
        let small = matches!(tree.info[i].kind, Kind::Checkbox { .. } | Kind::Toggle { .. });
        let (sx, sy) = if small { (self.px(sz::SLOP_X), self.px(sz::SLOP_Y)) } else { (0.0, 0.0) };
        let slop = intersect(Rect::new(r.x - sx, r.y - sy, r.w + 2.0 * sx, r.h + 2.0 * sy), self.clip).unwrap_or(r);
        let arrows = matches!(tree.info[i].kind, Kind::Slider { .. });
        self.prev.hits.push(Hit { id, rect: r, slop, focusable, text_input, arrows });
        if focusable {
            self.prev.focus_order.push(id);
        }
    }

    fn focus_ring(&mut self, tree: &Tree, i: usize, radius: f32) {
        if self.cx.focus_visible && self.cx.focus == Some(tree.info[i].id) {
            let r = tree.layout[i].rect;
            let w = self.px(2.0);
            let t = &self.cx.theme;
            let (accent, bg) = (t.accent, t.background);
            self.d.fill(r.inset(-2.0 * w), accent, radius + 2.0 * w);
            self.d.fill(r.inset(-w), bg, radius + w);
        }
    }

    fn node(&mut self, tree: &Tree, i: usize) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let line = self.line();
        match &tree.info[i].kind {
            Kind::Root | Kind::Row | Kind::Column | Kind::Stack | Kind::Space => {
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::Label { text, size, bold, weak, strike, wrap, right } => {
                let st = style(self.cx, *size, *bold);
                let color = if *weak { self.cx.theme.weak_text } else { self.cx.theme.text };
                let laid = self.cx.text.layout(text, st, if *wrap { Some(r.w) } else { None });
                let y = if *wrap { r.y } else { (r.y + (r.h - laid.height) / 2.0).round() };
                let x = if *right && !*wrap { (r.right() - laid.width).round() } else { r.x };
                put(self.d, &laid, x, y, color);
                if *strike {
                    let color = self.cx.theme.done_text;
                    self.d.fill(Rect::new(x, (y + laid.height * 0.55).round(), laid.width, line), color, 0.0);
                }
            }
            Kind::Item { selected, tab_stop } => {
                let t = self.cx.theme.clone();
                if *selected {
                    self.d.fill(r, t.selected, 0.0);
                } else if self.cx.hover == Some(id) {
                    self.d.fill(r, t.button_hover, 0.0);
                }
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    let bg = if *selected { t.selected } else { t.background };
                    self.d.boxed(r.inset(line), bg, t.accent, 2.0 * line, self.px(4.0));
                }
                // 行そのものの当たり（中の部品は後に入るので、部品が先に当たる）
                if let Some((list, row)) = self.row {
                    self.prev.items.insert(id, (list, tree.layout[i].row_index));
                    self.prev.rows.insert(id, (list, row));
                }
                if let Some(v) = intersect(r, self.clip) {
                    self.prev.hits.push(Hit { id, rect: v, slop: v, focusable: true, text_input: false, arrows: true });
                }
                if *tab_stop {
                    self.prev.focus_order.push(id);
                }
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::Button { text, primary, small } => {
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let t = self.cx.theme.clone();
                let state_bg = if self.cx.pressed == Some(id) {
                    t.button_pressed
                } else if self.cx.hover == Some(id) {
                    t.button_hover
                } else {
                    t.button
                };
                if *primary {
                    let mut bg = t.accent;
                    if self.cx.pressed == Some(id) || self.cx.hover == Some(id) {
                        bg = [bg[0] * 0.88, bg[1] * 0.88, bg[2] * 0.88, 1.0];
                    }
                    self.d.fill(r, bg, radius);
                } else {
                    self.d.boxed(r, state_bg, t.border, line, radius);
                }
                let st = style(self.cx, if *small { TextKind::Small } else { TextKind::Body }, *primary);
                let laid = self.cx.text.layout(text, st, None);
                let color = if *primary { t.on_accent } else { t.text };
                put(self.d, &laid, (r.x + (r.w - laid.width) / 2.0).round(), (r.y + (r.h - laid.height) / 2.0).round(), color);
                self.hit(tree, i, true, false);
            }
            Kind::Checkbox { checked, text, strike } => {
                let t = self.cx.theme.clone();
                let b = self.px(sz::BOX);
                let bx = Rect::new(r.x, (r.y + (r.h - b) / 2.0).round(), b, b);
                let radius = self.px(3.0);
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    let w = self.px(2.0);
                    self.d.fill(bx.inset(-2.0 * w), t.accent, radius + 2.0 * w);
                    self.d.fill(bx.inset(-w), t.background, radius + w);
                }
                if *checked {
                    self.d.fill(bx, t.accent, radius);
                    let mark = self.cx.text.layout("✓", TextStyle { size: (13.0 * self.cx.scale).round(), bold: true }, None);
                    put(self.d, &mark, (bx.x + (bx.w - mark.width) / 2.0).round(), (bx.y + (bx.h - mark.height) / 2.0).round(), t.on_accent);
                } else {
                    let border = if self.cx.hover == Some(id) { t.accent } else { t.border };
                    self.d.boxed(bx, t.field, border, line, radius);
                }
                if !text.is_empty() {
                    let st = style(self.cx, TextKind::Body, false);
                    let x = r.x + b + self.px(sz::BOX_GAP);
                    let color = if *strike { t.done_text } else { t.text };
                    let laid = text_mid(self.d, self.cx, text, st, x, r, color);
                    if *strike {
                        let y = (r.y + (r.h - laid.height) / 2.0 + laid.height * 0.55).round();
                        self.d.fill(Rect::new(x, y, laid.width, line), t.done_text, 0.0);
                    }
                }
                self.hit(tree, i, true, false);
            }
            Kind::Toggle { on, text } => {
                let t = self.cx.theme.clone();
                let (tw, th) = (self.px(sz::TRACK_W), self.px(sz::TRACK_H));
                let track = Rect::new(r.x, (r.y + (r.h - th) / 2.0).round(), tw, th);
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    let w = self.px(2.0);
                    self.d.fill(track.inset(-2.0 * w), t.accent, th);
                    self.d.fill(track.inset(-w), t.background, th);
                }
                self.d.fill(track, if *on { t.accent } else { t.border }, th / 2.0);
                let k = th - self.px(6.0);
                let kx = if *on { track.right() - self.px(3.0) - k } else { track.x + self.px(3.0) };
                self.d.fill(Rect::new(kx, track.y + self.px(3.0), k, k), if *on { t.on_accent } else { t.field }, k / 2.0);
                if !text.is_empty() {
                    let st = style(self.cx, TextKind::Body, false);
                    text_mid(self.d, self.cx, text, st, r.x + tw + self.px(sz::BOX_GAP), r, t.text);
                }
                self.hit(tree, i, true, false);
            }
            Kind::Segment { text, selected, .. } => {
                let t = self.cx.theme.clone();
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let (bg, border, color) = if *selected {
                    (t.selected, t.accent, t.accent)
                } else if self.cx.hover == Some(id) {
                    (t.button_hover, t.border, t.text)
                } else {
                    (t.button, t.border, t.text)
                };
                self.d.boxed(r, bg, border, line, radius);
                let laid = self.cx.text.layout(text, style(self.cx, TextKind::Body, *selected), None);
                put(self.d, &laid, (r.x + (r.w - laid.width) / 2.0).round(), (r.y + (r.h - laid.height) / 2.0).round(), color);
                self.hit(tree, i, true, false);
            }
            Kind::TextInput { value, hint } => self.text_input(tree, i, value, hint),
            Kind::List { count, separators, .. } => self.list(tree, i, *count, *separators),
            Kind::Scroll => self.scroll_area(tree, i),
            Kind::Slider { value, min, max, .. } => {
                let t = self.cx.theme.clone();
                let knob = self.px(sz::KNOB);
                let th = self.px(sz::SLIDER_TRACK);
                let (tx, tw) = slider_track(r, self.cx.scale);
                let ty = (r.y + (r.h - th) / 2.0).round();
                let f = if max > min { ((value - min) / (max - min)).clamp(0.0, 1.0) as f32 } else { 0.0 };
                let k = Rect::new((tx + f * tw - knob / 2.0).round(), (r.y + (r.h - knob) / 2.0).round(), knob, knob);
                self.d.fill(Rect::new(tx, ty, tw, th), t.border, th / 2.0);
                self.d.fill(Rect::new(tx, ty, (f * tw).round(), th), t.accent, th / 2.0);
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    let w = self.px(2.0);
                    self.d.fill(k.inset(-2.0 * w), t.accent, knob / 2.0 + 2.0 * w);
                    self.d.fill(k.inset(-w), t.background, knob / 2.0 + w);
                }
                // つまみ: 白い丸の中にアクセントの色の丸。押している・乗っている時は中の丸を大きくする
                let active = self.cx.pressed == Some(id) || self.cx.hover == Some(id);
                self.d.boxed(k, t.field, t.border, line, knob / 2.0);
                let inner = (knob * if active { 0.6 } else { 0.5 }).round();
                let c = Rect::new(k.x + ((knob - inner) / 2.0).round(), k.y + ((knob - inner) / 2.0).round(), inner, inner);
                self.d.fill(c, t.accent, inner / 2.0);
                self.hit(tree, i, true, false);
            }
        }
    }

    /// 見えない所にある部品は描かずに、Tab で移れるように覚えるだけにする
    fn hidden(&mut self, tree: &Tree, i: usize) {
        let id = tree.info[i].id;
        if tree.info[i].kind.interactive() {
            if let Some(s) = self.scroller {
                self.prev.rows.insert(id, (s, tree.layout[i].rect));
            }
            self.prev.focus_order.push(id);
        }
        for &c in &tree.layout[i].children {
            self.hidden(tree, c);
        }
    }

    /// スクロールの棒のつまみと押せる範囲（中身が見える範囲より高いときだけ）
    fn scrollbar_geometry(&self, r: Rect, content_h: f32, offset: f32) -> (Option<Rect>, Option<Rect>) {
        if content_h <= r.h + 0.5 {
            return (None, None);
        }
        let w = self.px(sz::SCROLLBAR_W);
        let h = (r.h * r.h / content_h).max(self.px(24.0)).min(r.h);
        let y = r.y + (offset / (content_h - r.h)).clamp(0.0, 1.0) * (r.h - h);
        let x = r.right() - w - self.px(3.0);
        let thumb = Rect::new(x, y, w, h);
        let bar = Rect::new(x - self.px(sz::SCROLLBAR_SLOP), r.y, r.right() - x + self.px(sz::SCROLLBAR_SLOP), r.h);
        (Some(thumb), Some(bar))
    }

    /// つまみを描く。ポインターが棒の上にある時とつかんでいる間は濃くする（つかめることが見て分かるように）
    fn draw_thumb(&mut self, id: crate::ui::Id, thumb: Rect) {
        let mut c = self.cx.theme.weak_text;
        c[3] = if self.cx.drag.is_some_and(|d| d.list == id) {
            0.85
        } else if self.cx.bar_hover == Some(id) {
            0.65
        } else {
            0.45
        };
        self.d.fill(Rect::new(thumb.x, thumb.y.round(), thumb.w, thumb.h.round()), c, thumb.w / 2.0);
    }

    /// スクロールの入れ物。見える範囲で切り取り、見えない子は描かない
    fn scroll_area(&mut self, tree: &Tree, i: usize) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let l = &tree.layout[i];
        // 中の一覧より先に入れる（ホイールと棒は、後に入れた内側の物から探す）
        let (thumb, bar) = self.scrollbar_geometry(r, l.content, l.offset);
        self.prev.lists.push(ListInfo { id, rect: r, content_h: l.content, thumb, bar });
        self.d.push_clip(IRect::covering(r), Some(Scroll { id: id.0, offset: l.offset as i32 }));
        let outer = (self.clip, self.scroller);
        self.clip = intersect(r, outer.0).unwrap_or(Rect::new(r.x, r.y, 0.0, 0.0));
        self.scroller = Some(id);
        for &c in &l.children {
            if intersect(tree.layout[c].rect, self.clip).is_some() {
                self.node(tree, c);
            } else {
                self.hidden(tree, c);
            }
        }
        (self.clip, self.scroller) = outer;
        self.d.pop_clip();
        if let Some(t) = thumb {
            self.draw_thumb(id, t);
        }
    }

    fn text_input(&mut self, tree: &Tree, i: usize, value: &str, hint: &str) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let t = self.cx.theme.clone();
        let line = self.line();
        let focused = self.cx.focus == Some(id);
        let radius = self.px(sz::RADIUS);
        self.d.boxed(r, t.field, if focused { t.accent } else { t.border }, if focused { 2.0 * line } else { line }, radius);
        let pad = self.px(sz::INPUT_PAD);
        let inner = Rect::new(r.x + pad, r.y, (r.w - 2.0 * pad).max(0.0), r.h);
        let st = style(self.cx, TextKind::Body, false);
        let (cursor, anchor) = self.cx.edits.get(&id).map_or((value.len(), value.len()), |e| (e.cursor.min(value.len()), e.anchor.min(value.len())));
        let preedit = if focused { self.cx.preedit.clone() } else { String::new() };
        // 変換中の文字はキャレットの位置に差し込んで描く（値にはまだ入れない）
        let shown = if preedit.is_empty() { value.to_string() } else { format!("{}{}{}", &value[..cursor], preedit, &value[cursor..]) };
        let laid = self.cx.text.layout(&shown, st, None);
        let line_h = laid.height.max(st.size * 1.3);
        let ty = (r.y + (r.h - line_h) / 2.0).round();
        // 変換中の下線は画素にそろえて行の下端に引く（太い下線は 2 本分で、下端まで）
        let underline_y = (ty + line_h).round() - 2.0 * line;
        // IME に答える行の高さ。候補の窓はこの下に出るので、太い下線の下に 1 本分あける
        let ime_h = underline_y + 3.0 * line - ty;
        let caret_byte = cursor + preedit.len();
        // 変換中は、IME が示した位置にキャレットを置く（無ければ変換中の文字の終わり）
        let clauses = if preedit.is_empty() { None } else { self.cx.preedit_clauses.clone() };
        let start16 = edit::utf16_of(&shown, cursor);
        let n16 = preedit.encode_utf16().count();
        let caret_x = match clauses.as_ref().and_then(|c| c.cursor).filter(|&c| c <= n16) {
            Some(c) => laid.x_of(start16 + c),
            None => laid.x_of(edit::utf16_of(&shown, caret_byte)),
        };
        // キャレットが見えるように横にずらす
        let mut scroll = self.cx.edit_scroll.get(&id).copied().unwrap_or(0.0);
        if caret_x - scroll > inner.w - line {
            scroll = caret_x - inner.w + line;
        }
        if caret_x - scroll < 0.0 {
            scroll = caret_x;
        }
        if laid.width <= inner.w {
            scroll = 0.0;
        }
        self.cx.edit_scroll.insert(id, scroll);
        let x0 = inner.x - scroll;
        self.d.push_clip(IRect::covering(inner), None);
        if cursor != anchor && preedit.is_empty() {
            let (a, b) = (cursor.min(anchor), cursor.max(anchor));
            let xa = laid.x_of(edit::utf16_of(&shown, a));
            let xb = laid.x_of(edit::utf16_of(&shown, b));
            self.d.fill(Rect::new(x0 + xa, ty, xb - xa, line_h), t.selection, 0.0);
        }
        if shown.is_empty() {
            let h = self.cx.text.layout(hint, st, None);
            put(self.d, &h, x0, ty, t.weak_text);
        } else {
            put(self.d, &laid, x0, ty, t.text);
        }
        if !preedit.is_empty() {
            let xa = laid.x_of(start16);
            let xb = laid.x_of(start16 + n16);
            // 文節ごとに下線を引き、今変換している文節は太くする（文節の間は少しあける）
            let bounds = clauses.as_ref().map(|c| c.bounds.clone()).filter(|b| b.last() == Some(&n16)).unwrap_or_else(|| vec![0, n16]);
            let target = clauses.as_ref().and_then(|c| c.target);
            let gap = (2.0 * self.cx.scale).round();
            for w in bounds.windows(2) {
                let (a, b) = (w[0], w[1]);
                let thick = target.is_some_and(|(t0, t1)| a < t1 && t0 < b);
                let x1 = laid.x_of(start16 + a);
                let x2 = laid.x_of(start16 + b) - if b < n16 { gap } else { 0.0 };
                let h = if thick { 2.0 * line } else { line };
                self.d.fill(Rect::new((x0 + x1).round(), underline_y, (x2 - x1).max(0.0).round(), h), t.text, 0.0);
            }
            self.cx.preedit_rect = Some(Rect::new((x0 + xa).round(), ty, xb - xa, ime_h));
        }
        let caret = Rect::new((x0 + caret_x).round(), ty, (1.5 * self.cx.scale).round().max(1.0), line_h.round());
        if focused {
            // IME の問い合わせに答えるための写し。変換中の文字の境目ごとの x も覚える
            let preedit_xs = if preedit.is_empty() { Vec::new() } else { (0..=n16).map(|k| (x0 + laid.x_of(start16 + k)).round()).collect() };
            self.cx.ime_field = Some(ImeField { id, value: value.to_string(), cursor, anchor, preedit_xs });
            self.d.fill(caret, t.text, 0.0);
            self.cx.caret = Some(caret);
            self.cx.input_rect = Some(r);
            if preedit.is_empty() {
                self.cx.preedit_rect = Some(Rect::new(caret.x, caret.y, 0.0, ime_h));
            }
        }
        self.d.pop_clip();
        self.prev.inputs.insert(id, (r, x0));
        self.hit(tree, i, true, true);
    }

    fn list(&mut self, tree: &Tree, i: usize, count: usize, separators: bool) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let l = &tree.layout[i];
        let content_h = count as f32 * l.row_h;
        let line = self.line();
        let sep = self.cx.theme.separator;
        let (thumb, bar) = self.scrollbar_geometry(r, content_h, l.offset);
        self.prev.lists.push(ListInfo { id, rect: r, content_h, thumb, bar });
        self.d.push_clip(IRect::covering(r), Some(Scroll { id: id.0, offset: l.offset as i32 }));
        let outer = self.clip;
        self.clip = intersect(r, outer).unwrap_or(Rect::new(r.x, r.y, 0.0, 0.0));
        for &c in &l.children {
            let row = tree.layout[c].rect;
            if separators {
                let inset = tree.layout[c].pad.0;
                self.d.fill(Rect::new(row.x + inset, row.bottom() - line, (row.w - 2.0 * inset).max(0.0), line), sep, 0.0);
            }
            self.row = Some((id, row));
            self.node(tree, c);
            self.row = None;
        }
        self.clip = outer;
        self.d.pop_clip();
        // スクロールの位置を示す細い棒（中身が一覧より高いときだけ）
        if let Some(t) = thumb {
            self.draw_thumb(id, t);
        }
    }
}

/// 一覧が見える範囲を決めるのに使った高さと、決まった高さがずれていたら true（もう 1 フレーム描く）
pub(crate) fn list_needs_refresh(tree: &Tree) -> bool {
    tree.info.iter().zip(&tree.layout).any(|(n, l)| matches!(n.kind, Kind::List { view_h, .. } if (view_h - l.rect.h).abs() > 0.5))
}

#[allow(dead_code)]
fn _unused(_: Dir, _: Size) {}
