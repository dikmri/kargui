//! 部品の大きさを測り、描く物の一覧を作る。次のフレームの当たり判定に使う情報（Prev）もここで作る。

use crate::geom::{Color, IRect, Rect};
use crate::paint::{DrawList, Scroll};
use crate::text::{Laid, TextStyle};
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
    pub const SCROLLBAR_W: f32 = 6.0;
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
        _ => return None,
    })
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
}

/// 画面の木を描く。次のフレームの当たり判定の情報を cx.prev に入れる
pub(crate) fn paint(tree: &Tree, cx: &mut Context, d: &mut DrawList) {
    let window = IRect::new(0, 0, cx.width as i32, cx.height as i32);
    d.begin(window);
    cx.caret = None;
    cx.preedit_rect = None;
    cx.input_rect = None;
    let mut p = Painter { cx, d, prev: Prev::default(), clip: window.to_rect() };
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
        if let Some(r) = intersect(r, self.clip) {
            let id = tree.info[i].id;
            self.prev.hits.push(Hit { id, rect: r, focusable, text_input });
            if focusable {
                self.prev.focus_order.push(id);
            }
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
            Kind::Label { text, size, bold, weak, strike, wrap } => {
                let st = style(self.cx, *size, *bold);
                let color = if *weak { self.cx.theme.weak_text } else { self.cx.theme.text };
                let laid = self.cx.text.layout(text, st, if *wrap { Some(r.w) } else { None });
                let y = if *wrap { r.y } else { (r.y + (r.h - laid.height) / 2.0).round() };
                put(self.d, &laid, r.x, y, color);
                if *strike {
                    let color = self.cx.theme.done_text;
                    self.d.fill(Rect::new(r.x, (y + laid.height * 0.55).round(), laid.width, line), color, 0.0);
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
        let caret_byte = cursor + preedit.len();
        let caret_x = laid.x_of(edit::utf16_of(&shown, caret_byte));
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
            let xa = laid.x_of(edit::utf16_of(&shown, cursor));
            let xb = laid.x_of(edit::utf16_of(&shown, caret_byte));
            self.d.fill(Rect::new(x0 + xa, ty + line_h - 2.0 * line, xb - xa, line), t.text, 0.0);
            self.cx.preedit_rect = Some(Rect::new((x0 + xa).round(), ty, xb - xa, line_h));
        }
        let caret = Rect::new((x0 + caret_x).round(), ty, (1.5 * self.cx.scale).round().max(1.0), line_h.round());
        if focused {
            self.d.fill(caret, t.text, 0.0);
            self.cx.caret = Some(caret);
            self.cx.input_rect = Some(r);
            if preedit.is_empty() {
                self.cx.preedit_rect = Some(Rect::new(caret.x, caret.y, 0.0, caret.h));
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
        self.d.push_clip(IRect::covering(r), Some(Scroll { id: id.0, offset: l.offset as i32 }));
        let outer = self.clip;
        self.clip = intersect(r, outer).unwrap_or(Rect::new(r.x, r.y, 0.0, 0.0));
        for &c in &l.children {
            let row = tree.layout[c].rect;
            if separators {
                let inset = tree.layout[c].pad.0;
                self.d.fill(Rect::new(row.x + inset, row.bottom() - line, (row.w - 2.0 * inset).max(0.0), line), sep, 0.0);
            }
            self.node(tree, c);
        }
        self.clip = outer;
        self.d.pop_clip();
        // スクロールの位置を示す細い棒（中身が一覧より高いときだけ）
        if content_h > r.h + 0.5 {
            let w = self.px(sz::SCROLLBAR_W);
            let h = (r.h * r.h / content_h).max(self.px(24.0)).min(r.h);
            let y = r.y + (l.offset / (content_h - r.h)).clamp(0.0, 1.0) * (r.h - h);
            let mut c = self.cx.theme.weak_text;
            c[3] = 0.45;
            self.d.fill(Rect::new(r.right() - w - self.px(3.0), y.round(), w, h.round()), c, w / 2.0);
        }
        self.prev.lists.push(ListInfo { id, rect: r, content_h });
    }
}

/// 一覧が見える範囲を決めるのに使った高さと、決まった高さがずれていたら true（もう 1 フレーム描く）
pub(crate) fn list_needs_refresh(tree: &Tree) -> bool {
    tree.info.iter().zip(&tree.layout).any(|(n, l)| matches!(n.kind, Kind::List { view_h, .. } if (view_h - l.rect.h).abs() > 0.5))
}

#[allow(dead_code)]
fn _unused(_: Dir, _: Size) {}
