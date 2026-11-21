//! 部品の大きさを測り、描く物の一覧を作る。次のフレームの当たり判定に使う情報（Prev）もここで作る。

use crate::geom::{Color, IRect, Rect};
use crate::paint::{DrawList, Scroll};
use crate::text::{Laid, TextStyle};
use crate::ime::ImeField;
use crate::ui::context::{Context, Hit, ListInfo, OverlayInfo, Prev};
use crate::ui::edit;
use crate::ui::layout::{self, Dir, Size};
use crate::ui::{Kind, Overlay, OverlayKind, TextKind, Tree};
use crate::icon::Icon;

/// 論理ピクセルの大きさ（拡大率を掛ける前）
mod sz {
    pub const BUTTON_H: f32 = 32.0;
    pub const BUTTON_SMALL_H: f32 = 26.0;
    pub const BUTTON_PAD: f32 = 14.0;
    pub const BUTTON_SMALL_PAD: f32 = 10.0;
    pub const BUTTON_MIN_W: f32 = 56.0;
    pub const BOX: f32 = 16.0;
    pub const BOX_GAP: f32 = 8.0;
    /// 複数行の入力欄の上下の余白
    pub const AREA_PAD_Y: f32 = 6.0;
    /// ボタンの中のアイコンと文字の間
    pub const ICON_GAP: f32 = 6.0;
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
        Kind::Button { text, small, icon, .. } => {
            let h = p(if *small { sz::BUTTON_SMALL_H } else { sz::BUTTON_H });
            // アイコンだけのボタンは正方形
            if text.is_empty() && icon.is_some() {
                return Some((h, h));
            }
            let st = style(cx, if *small { TextKind::Small } else { TextKind::Body }, false);
            let laid = cx.text.layout(text, st, None);
            let pad = p(if *small { sz::BUTTON_SMALL_PAD } else { sz::BUTTON_PAD });
            let min = p(if *small { sz::BUTTON_MIN_W * 0.8 } else { sz::BUTTON_MIN_W });
            let lead = icon.map_or(0.0, |_| p(button_icon(*small)) + p(sz::ICON_GAP));
            ((laid.width.ceil() + lead + 2.0 * pad).max(min), h)
        }
        Kind::Icon { size, .. } => (p(*size as f32), p(*size as f32)),
        Kind::Radio { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            (p(sz::BOX) + p(sz::BOX_GAP) + laid.width.ceil(), laid.height.ceil().max(p(sz::TRACK_H)))
        }
        Kind::Separator => (1.0, 1.0),
        Kind::Dropdown { widest, .. } => {
            let laid = cx.text.layout(widest, style(cx, TextKind::Body, false), None);
            ((laid.width.ceil() + 2.0 * p(sz::INPUT_PAD) + p(16.0) + p(sz::ICON_GAP)).max(p(sz::BUTTON_MIN_W)), p(sz::BUTTON_H))
        }
        Kind::ListOption { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            (laid.width.ceil() + 2.0 * p(12.0), p(30.0))
        }
        Kind::Progress { ring: false, .. } => (p(sz::INPUT_W), p(12.0)),
        Kind::Progress { ring: true, .. } => (p(20.0), p(20.0)),
        Kind::Expander { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, true), None);
            (p(16.0) + p(sz::ICON_GAP) + laid.width.ceil() + p(8.0), p(sz::BUTTON_H))
        }
        Kind::Link { text } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            (laid.width.ceil(), laid.height.ceil())
        }
        Kind::ToggleButton { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            ((laid.width.ceil() + 2.0 * p(sz::BUTTON_PAD)).max(p(sz::BUTTON_MIN_W)), p(sz::BUTTON_H))
        }
        Kind::Image { w, h, .. } => (p(*w as f32), p(*h as f32)),
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
        Kind::TextArea { rows, .. } => {
            let line_h = cx.text.layout("あ", style(cx, TextKind::Body, false), None).height.ceil();
            (p(sz::INPUT_W), *rows as f32 * line_h + 2.0 * p(sz::AREA_PAD_Y))
        }
        Kind::Slider { .. } => (p(sz::SLIDER_W), p(sz::SLIDER_H)),
        _ => return None,
    })
}

/// UTF-16 の範囲 [a, b) を、行ごとの四角にする（並べた範囲の左上が原点）。範囲が改行をまたぐ行は、
/// 行の終わりから newline の幅だけ伸ばして、改行も選ばれていることを見せる
fn range_rects(laid: &Laid, a: usize, b: usize, newline: f32) -> Vec<Rect> {
    let mut out = Vec::new();
    if a >= b {
        return out;
    }
    for l in &laid.lines {
        if l.end < a || l.start > b || (l.start == b && b > a && l.start != l.end) {
            continue;
        }
        let x0 = if a > l.start { laid.caret(a).0 } else { 0.0 };
        let (x1, extra) = if b <= l.end { (laid.caret(b).0, 0.0) } else { (l.width, newline) };
        if x1 + extra > x0 {
            out.push(Rect::new(x0, l.top, x1 + extra - x0, l.height));
        }
    }
    out
}

/// ボタンの中のアイコンの大きさ（論理ピクセル）
fn button_icon(small: bool) -> f32 {
    if small { 14.0 } else { 16.0 }
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
    place_overlays(tree, cx, window);
}

/// 重ねて出す物を置く（ふつうの配置の後。出す元の部品の場所が決まってから）
fn place_overlays(tree: &mut Tree, cx: &Context, window: Rect) {
    let (margin, gap) = (px(cx, 8.0), px(cx, 4.0));
    for o in tree.overlays.clone() {
        let (w, h) = layout::measure(&mut tree.layout, o.idx);
        let w = w.min((window.w - 2.0 * margin).max(0.0));
        let h = h.min((window.h - 2.0 * margin).max(0.0));
        let anchor = o.anchor.and_then(|a| tree.info.iter().position(|n| n.id == a)).map(|i| tree.layout[i].rect);
        let rect = match (o.kind, anchor) {
            (OverlayKind::Popup | OverlayKind::Tooltip | OverlayKind::ListBox | OverlayKind::Menu, Some(a)) => layout::place_near(a, w, h, window, margin, gap),
            _ => layout::place_center(w, h, window),
        };
        layout::arrange(&mut tree.layout, o.idx, rect);
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
    for o in &tree.overlays {
        p.overlay(tree, o);
    }
    p.d.finish();
    let prev = std::mem::take(&mut p.prev);
    cx.prev = prev;
    // 焦点のある部品が押せなくなったら、焦点を外す（Enter や Space が効かないように）
    if let Some(f) = cx.focus.filter(|f| cx.prev.disabled.contains(f)) {
        cx.focus = None;
        crate::log::event("focus.lost", &format!("target={:x}", f.0), "disabled");
    }
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
        // 押せない部品は、焦点も文字の入力も受けない
        let enabled = !tree.info[i].disabled;
        let (focusable, text_input) = (focusable && enabled, text_input && enabled);
        if !enabled {
            self.prev.disabled.insert(id);
        }
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
        let arrows = enabled && matches!(tree.info[i].kind, Kind::Slider { .. } | Kind::Dropdown { .. } | Kind::ListOption { .. });
        let multiline = matches!(tree.info[i].kind, Kind::TextArea { .. });
        self.prev.hits.push(Hit { id, rect: r, slop, focusable, text_input, arrows, multiline, enabled });
        if focusable {
            self.prev.focus_order.push(id);
        }
    }

    /// アイコンを (x, y) から s × s 画素で描く（形の表に入れて、文字と同じく色を付けて描く）
    fn icon(&mut self, icon: Icon, x: f32, y: f32, s: f32, color: Color) {
        if let Some(slot) = self.cx.icon_slot(icon, s as u32) {
            self.d.glyph(x, y, slot.w as f32, slot.h as f32, slot.x as f32, slot.y as f32, color);
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
            // 重ねる物は親に付かないので、ここには来ない（Painter::overlay で描く）
            Kind::Overlay { .. } => {}
            Kind::Root | Kind::Row | Kind::Column | Kind::Stack | Kind::Space => {
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::Label { text, size, bold, weak, strike, wrap, right } => {
                let st = style(self.cx, *size, *bold);
                let color = if tree.info[i].disabled {
                    self.cx.theme.done_text
                } else if *weak {
                    self.cx.theme.weak_text
                } else {
                    self.cx.theme.text
                };
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
                let enabled = !tree.info[i].disabled;
                if *selected {
                    self.d.fill(r, t.selected, 0.0);
                } else if enabled && self.cx.hover == Some(id) {
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
                    self.prev.hits.push(Hit { id, rect: v, slop: v, focusable: enabled, text_input: false, arrows: enabled, multiline: false, enabled });
                }
                if !enabled {
                    self.prev.disabled.insert(id);
                }
                if *tab_stop && enabled {
                    self.prev.focus_order.push(id);
                }
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::Button { text, primary, small, icon } => {
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let state_bg = if off {
                    t.button
                } else if self.cx.pressed == Some(id) {
                    t.button_pressed
                } else if self.cx.hover == Some(id) {
                    t.button_hover
                } else {
                    t.button
                };
                if *primary {
                    // 押せない主のボタンは、アクセントの色の代わりに枠の色で塗る
                    let mut bg = if off { t.border } else { t.accent };
                    if !off && (self.cx.pressed == Some(id) || self.cx.hover == Some(id)) {
                        bg = [bg[0] * 0.88, bg[1] * 0.88, bg[2] * 0.88, 1.0];
                    }
                    self.d.fill(r, bg, radius);
                } else {
                    self.d.boxed(r, state_bg, t.border, line, radius);
                }
                let st = style(self.cx, if *small { TextKind::Small } else { TextKind::Body }, *primary);
                let laid = self.cx.text.layout(text, st, None);
                let color = if *primary {
                    t.on_accent
                } else if off {
                    t.done_text
                } else {
                    t.text
                };
                // アイコンと文字をひとまとまりにして、真ん中に置く
                let (is, gap) = match icon {
                    Some(_) => (self.px(button_icon(*small)), if text.is_empty() { 0.0 } else { self.px(sz::ICON_GAP) }),
                    None => (0.0, 0.0),
                };
                let x0 = (r.x + (r.w - (is + gap + laid.width)) / 2.0).round();
                if let Some(ic) = icon {
                    self.icon(*ic, x0, (r.y + (r.h - is) / 2.0).round(), is, color);
                }
                put(self.d, &laid, x0 + is + gap, (r.y + (r.h - laid.height) / 2.0).round(), color);
                self.hit(tree, i, true, false);
            }
            Kind::Image { image, .. } => {
                self.d.image(r, image);
            }
            Kind::Radio { selected, text, tab_stop } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let b = self.px(sz::BOX);
                let c = Rect::new(r.x, (r.y + (r.h - b) / 2.0).round(), b, b);
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    let w = self.px(2.0);
                    self.d.fill(c.inset(-2.0 * w), t.accent, b / 2.0 + 2.0 * w);
                    self.d.fill(c.inset(-w), t.background, b / 2.0 + w);
                }
                let ring = if off {
                    t.border
                } else if *selected || self.cx.hover == Some(id) {
                    t.accent
                } else {
                    t.border
                };
                self.d.boxed(c, t.field, ring, if *selected { (b * 0.28).round() } else { line }, b / 2.0);
                let st = style(self.cx, TextKind::Body, false);
                text_mid(self.d, self.cx, text, st, r.x + b + self.px(sz::BOX_GAP), r, if off { t.done_text } else { t.text });
                // Tab が止まるのはグループの中の 1 つだけ（矢印で移る）
                let enabled = !off;
                if let Some(v) = intersect(r, self.clip) {
                    self.prev.hits.push(Hit { id, rect: v, slop: v, focusable: enabled, text_input: false, arrows: enabled, multiline: false, enabled });
                }
                if !enabled {
                    self.prev.disabled.insert(id);
                } else if *tab_stop {
                    self.prev.focus_order.push(id);
                }
            }
            Kind::Separator => {
                let t = &self.cx.theme;
                self.d.fill(r, t.border, 0.0);
            }
            Kind::Dropdown { text, open, .. } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let bg = if off {
                    t.button
                } else if self.cx.pressed == Some(id) {
                    t.button_pressed
                } else if self.cx.hover == Some(id) {
                    t.button_hover
                } else {
                    t.button
                };
                self.d.boxed(r, bg, if *open { t.accent } else { t.border }, line, radius);
                let color = if off { t.done_text } else { t.text };
                let pad = self.px(sz::INPUT_PAD);
                text_mid(self.d, self.cx, text, style(self.cx, TextKind::Body, false), r.x + pad, r, color);
                let s = self.px(16.0);
                self.icon(Icon::ChevronDown, r.right() - pad - s, (r.y + (r.h - s) / 2.0).round(), s, if off { t.done_text } else { t.weak_text });
                self.hit(tree, i, true, false);
            }
            Kind::ListOption { text, selected } => {
                let t = self.cx.theme.clone();
                let radius = self.px(4.0);
                let hot = self.cx.hover == Some(id) || self.cx.focus == Some(id);
                if *selected {
                    self.d.fill(r, t.selected, radius);
                    // 選んでいる項目は左に短い縦の線
                    let bar = Rect::new(r.x, (r.y + r.h * 0.25).round(), self.px(3.0), (r.h * 0.5).round());
                    self.d.fill(bar, t.accent, bar.w / 2.0);
                } else if hot {
                    self.d.fill(r, t.button_hover, radius);
                }
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    self.d.boxed(r.inset(line), [0.0; 4], t.accent, 2.0 * line, radius);
                }
                text_mid(self.d, self.cx, text, style(self.cx, TextKind::Body, false), r.x + self.px(12.0), r, t.text);
                self.hit(tree, i, true, false);
            }
            Kind::Progress { value, ring } => {
                let t = self.cx.theme.clone();
                let phase = (self.cx.now.saturating_duration_since(self.cx.epoch).as_secs_f32() % 1.2) / 1.2;
                if *ring {
                    // 8 つの点が回る輪（先頭の点ほど濃い）
                    let s = r.w.min(r.h);
                    let (cx_, cy_) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                    let dot = (s * 0.18).max(2.0);
                    let rad = s / 2.0 - dot / 2.0;
                    for k in 0..8 {
                        let a = k as f32 / 8.0 * std::f32::consts::TAU;
                        let age = ((phase * 8.0 - k as f32).rem_euclid(8.0)) / 8.0;
                        let mut col = t.accent;
                        col[3] = 1.0 - age * 0.85;
                        let (x, y) = (cx_ + rad * a.cos() - dot / 2.0, cy_ + rad * a.sin() - dot / 2.0);
                        self.d.fill(Rect::new(x, y, dot, dot), col, dot / 2.0);
                    }
                } else {
                    let h = self.px(4.0);
                    let track = Rect::new(r.x, (r.y + (r.h - h) / 2.0).round(), r.w, h);
                    self.d.fill(track, t.separator, h / 2.0);
                    match value {
                        Some(v) => self.d.fill(Rect::new(track.x, track.y, (track.w * v).round(), h), t.accent, h / 2.0),
                        None => {
                            // 3 割の長さの帯が左から右へ流れる（両端で切る）
                            let w = track.w * 0.3;
                            let x0 = track.x + (phase * 1.3 - 0.3) * track.w;
                            let (a, b) = (x0.max(track.x), (x0 + w).min(track.right()));
                            if b > a {
                                self.d.fill(Rect::new(a.round(), track.y, (b - a).round(), h), t.accent, h / 2.0);
                            }
                        }
                    }
                }
            }
            Kind::Card { .. } => {
                let t = self.cx.theme.clone();
                self.d.boxed(r, t.background, t.border, line, self.px(8.0));
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::Expander { open, text } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                if !off && (self.cx.hover == Some(id) || self.cx.pressed == Some(id)) {
                    self.d.fill(r, t.button_hover, radius);
                }
                let color = if off { t.done_text } else { t.text };
                let s = self.px(16.0);
                let pad = self.px(4.0);
                self.icon(if *open { Icon::ChevronDown } else { Icon::ChevronRight }, r.x + pad, (r.y + (r.h - s) / 2.0).round(), s, color);
                text_mid(self.d, self.cx, text, style(self.cx, TextKind::Body, true), r.x + pad + s + self.px(sz::ICON_GAP), r, color);
                self.hit(tree, i, true, false);
            }
            Kind::Link { text } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                self.focus_ring(tree, i, self.px(2.0));
                let color = if off { t.done_text } else { t.accent };
                let laid = text_mid(self.d, self.cx, text, style(self.cx, TextKind::Body, false), r.x, r, color);
                // 下線（乗った時は太く）
                let thick = if !off && self.cx.hover == Some(id) { 2.0 * line } else { line };
                let y = (r.y + (r.h + laid.height) / 2.0 - thick).round();
                self.d.fill(Rect::new(r.x, y, laid.width.round(), thick), color, 0.0);
                self.hit(tree, i, true, false);
            }
            Kind::ToggleButton { on, text } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let (bg, border, color) = if off {
                    (t.button, t.border, t.done_text)
                } else if *on {
                    (t.selected, t.accent, t.accent)
                } else if self.cx.pressed == Some(id) {
                    (t.button_pressed, t.border, t.text)
                } else if self.cx.hover == Some(id) {
                    (t.button_hover, t.border, t.text)
                } else {
                    (t.button, t.border, t.text)
                };
                self.d.boxed(r, bg, border, line, radius);
                let laid = self.cx.text.layout(text, style(self.cx, TextKind::Body, *on), None);
                put(self.d, &laid, (r.x + (r.w - laid.width) / 2.0).round(), (r.y + (r.h - laid.height) / 2.0).round(), color);
                self.hit(tree, i, true, false);
            }
            Kind::Icon { icon, size, weak } => {
                let t = &self.cx.theme;
                let color = if tree.info[i].disabled {
                    t.done_text
                } else if *weak {
                    t.weak_text
                } else {
                    t.text
                };
                let s = self.px(*size as f32);
                self.icon(*icon, (r.x + (r.w - s) / 2.0).round(), (r.y + (r.h - s) / 2.0).round(), s, color);
            }
            Kind::Checkbox { checked, text, strike, mixed } => {
                let t = self.cx.theme.clone();
                let b = self.px(sz::BOX);
                let bx = Rect::new(r.x, (r.y + (r.h - b) / 2.0).round(), b, b);
                let radius = self.px(3.0);
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    let w = self.px(2.0);
                    self.d.fill(bx.inset(-2.0 * w), t.accent, radius + 2.0 * w);
                    self.d.fill(bx.inset(-w), t.background, radius + w);
                }
                let off = tree.info[i].disabled;
                if *checked || *mixed {
                    self.d.fill(bx, if off { t.border } else { t.accent }, radius);
                    let m = (bx.w * 0.85).round();
                    let mark = if *mixed { Icon::Minus } else { Icon::Check };
                    self.icon(mark, (bx.x + (bx.w - m) / 2.0).round(), (bx.y + (bx.h - m) / 2.0).round(), m, t.on_accent);
                } else {
                    let border = if !off && self.cx.hover == Some(id) { t.accent } else { t.border };
                    self.d.boxed(bx, t.field, border, line, radius);
                }
                if !text.is_empty() {
                    let st = style(self.cx, TextKind::Body, false);
                    let x = r.x + b + self.px(sz::BOX_GAP);
                    let color = if *strike || off { t.done_text } else { t.text };
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
                let off = tree.info[i].disabled;
                let track_color = match (*on, off) {
                    (true, false) => t.accent,
                    (true, true) | (false, false) => t.border,
                    (false, true) => t.separator,
                };
                self.d.fill(track, track_color, th / 2.0);
                let k = th - self.px(6.0);
                let kx = if *on { track.right() - self.px(3.0) - k } else { track.x + self.px(3.0) };
                self.d.fill(Rect::new(kx, track.y + self.px(3.0), k, k), if *on { t.on_accent } else { t.field }, k / 2.0);
                if !text.is_empty() {
                    let st = style(self.cx, TextKind::Body, false);
                    text_mid(self.d, self.cx, text, st, r.x + tw + self.px(sz::BOX_GAP), r, if off { t.done_text } else { t.text });
                }
                self.hit(tree, i, true, false);
            }
            Kind::Segment { text, selected, .. } => {
                let t = self.cx.theme.clone();
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let (bg, border, color) = if tree.info[i].disabled {
                    (if *selected { t.separator } else { t.button }, t.border, t.done_text)
                } else if *selected {
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
            Kind::TextInput { value, hint, password } => self.text_input(tree, i, value, hint, *password),
            Kind::TextArea { value, hint, .. } => self.text_area(tree, i, value, hint),
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
                let off = tree.info[i].disabled;
                let (rail, filled) = if off { (t.separator, t.border) } else { (t.border, t.accent) };
                self.d.fill(Rect::new(tx, ty, tw, th), rail, th / 2.0);
                self.d.fill(Rect::new(tx, ty, (f * tw).round(), th), filled, th / 2.0);
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    let w = self.px(2.0);
                    self.d.fill(k.inset(-2.0 * w), t.accent, knob / 2.0 + 2.0 * w);
                    self.d.fill(k.inset(-w), t.background, knob / 2.0 + w);
                }
                // つまみ: 白い丸の中にアクセントの色の丸。押している・乗っている時は中の丸を大きくする
                let active = !off && (self.cx.pressed == Some(id) || self.cx.hover == Some(id));
                self.d.boxed(k, t.field, t.border, line, knob / 2.0);
                let inner = (knob * if active { 0.6 } else { 0.5 }).round();
                let c = Rect::new(k.x + ((knob - inner) / 2.0).round(), k.y + ((knob - inner) / 2.0).round(), inner, inner);
                self.d.fill(c, filled, inner / 2.0);
                self.hit(tree, i, true, false);
            }
        }
    }

    /// 重ねて出す物を描く。ダイアログは後ろを暗くして押せなくする。浮き出しとダイアログの空いた所も、
    /// 下の部品へクリックが抜けないように当たりに入れる（押せない当たり）
    fn overlay(&mut self, tree: &Tree, o: &Overlay) {
        let r = tree.layout[o.idx].rect;
        let t = self.cx.theme.clone();
        let line = self.line();
        let window = Rect::new(0.0, 0.0, self.cx.width, self.cx.height);
        let start = self.prev.focus_order.len();
        let block = |p: &mut Self, rect: Rect| {
            p.prev.hits.push(Hit { id: o.id, rect, slop: rect, focusable: false, text_input: false, arrows: false, multiline: false, enabled: false })
        };
        let radius = self.px(if o.kind == OverlayKind::Tooltip { 4.0 } else { 8.0 });
        if o.kind == OverlayKind::Dialog {
            self.d.fill(window, [0.0, 0.0, 0.0, if self.cx.dark { 0.55 } else { 0.35 }], 0.0);
            block(self, window);
        }
        // 影（少し下にずらした薄い黒）と、背景の箱
        if o.kind != OverlayKind::Tooltip {
            let sh = self.px(3.0);
            self.d.fill(Rect::new(r.x - sh, r.y, r.w + 2.0 * sh, r.h + 2.0 * sh), [0.0, 0.0, 0.0, 0.10], radius + sh);
            block(self, r);
        }
        let bg = if o.kind == OverlayKind::Tooltip { t.button } else { t.background };
        self.d.boxed(r, bg, t.border, line, radius);
        for &c in &tree.layout[o.idx].children {
            self.node(tree, c);
        }
        let focus = self.prev.focus_order[start..].to_vec();
        self.prev.overlays.push(OverlayInfo { id: o.id, anchor: o.anchor, kind: o.kind, rect: r, focus });
    }

    /// 見えない所にある部品は描かずに、Tab で移れるように覚えるだけにする
    fn hidden(&mut self, tree: &Tree, i: usize) {
        let id = tree.info[i].id;
        if tree.info[i].disabled {
            self.prev.disabled.insert(id);
        } else if tree.info[i].kind.interactive() && !matches!(tree.info[i].kind, Kind::Radio { tab_stop: false, .. }) {
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

    fn text_input(&mut self, tree: &Tree, i: usize, value: &str, hint: &str, password: bool) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let t = self.cx.theme.clone();
        let line = self.line();
        let focused = self.cx.focus == Some(id);
        let off = tree.info[i].disabled;
        let radius = self.px(sz::RADIUS);
        self.d.boxed(r, if off { t.button } else { t.field }, if focused { t.accent } else { t.border }, if focused { 2.0 * line } else { line }, radius);
        let pad = self.px(sz::INPUT_PAD);
        let inner = Rect::new(r.x + pad, r.y, (r.w - 2.0 * pad).max(0.0), r.h);
        let st = style(self.cx, TextKind::Body, false);
        let (cursor, anchor) = self.cx.edits.get(&id).map_or((value.len(), value.len()), |e| (e.cursor.min(value.len()), e.anchor.min(value.len())));
        // パスワードは点にして描く（位置も点の位置に直す）
        let masked;
        let (value, cursor, anchor) = if password {
            masked = edit::mask(value);
            let to = |b: usize| value[..b].chars().count() * edit::DOT.len_utf8();
            (masked.as_str(), to(cursor), to(anchor))
        } else {
            (value, cursor, anchor)
        };
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
            put(self.d, &h, x0, ty, if off { t.done_text } else { t.weak_text });
        } else {
            put(self.d, &laid, x0, ty, if off { t.done_text } else { t.text });
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

    /// 複数行の入力欄。折り返して並べ、縦にスクロールする（スクロールの棒とホイールは一覧と同じ仕組み）。
    /// 選んだ範囲と変換中の文字の下線は、行ごとの四角に分けて描く
    fn text_area(&mut self, tree: &Tree, i: usize, value: &str, hint: &str) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let t = self.cx.theme.clone();
        let line = self.line();
        let focused = self.cx.focus == Some(id);
        let off = tree.info[i].disabled;
        let radius = self.px(sz::RADIUS);
        self.d.boxed(r, if off { t.button } else { t.field }, if focused { t.accent } else { t.border }, if focused { 2.0 * line } else { line }, radius);
        let (pad_x, pad_y) = (self.px(sz::INPUT_PAD), self.px(sz::AREA_PAD_Y));
        let gutter = self.px(SCROLL_GUTTER);
        // 文字を並べる範囲（右にスクロールの棒の分をいつも空け、幅が揺れないようにする）
        let inner = Rect::new(r.x + pad_x, r.y + pad_y, (r.w - 2.0 * pad_x - gutter).max(1.0), (r.h - 2.0 * pad_y).max(1.0));
        let view = Rect::new(r.x + line, r.y + line, (r.w - 2.0 * line).max(0.0), (r.h - 2.0 * line).max(0.0));
        let st = style(self.cx, TextKind::Body, false);
        let (cursor, anchor) = self.cx.edits.get(&id).map_or((value.len(), value.len()), |e| (e.cursor.min(value.len()), e.anchor.min(value.len())));
        let preedit = if focused { self.cx.preedit.clone() } else { String::new() };
        let shown = if preedit.is_empty() { value.to_string() } else { format!("{}{}{}", &value[..cursor], preedit, &value[cursor..]) };
        let laid = self.cx.text.layout(&shown, st, Some(inner.w));
        let start16 = edit::utf16_of(&shown, cursor);
        let n16 = preedit.encode_utf16().count();
        let clauses = if preedit.is_empty() { None } else { self.cx.preedit_clauses.clone() };
        let caret16 = match clauses.as_ref().and_then(|c| c.cursor).filter(|&c| c <= n16) {
            Some(c) => start16 + c,
            None => start16 + n16,
        };
        let (cx_, cy, ch) = laid.caret(caret16);
        // 縦のスクロール（論理ピクセルで持つ）。打った・動かした時だけ、キャレットが見えるようにする
        let scale = self.cx.scale;
        let content_h = laid.height + 2.0 * pad_y;
        let max = (content_h - r.h).max(0.0);
        let mut offset = (self.cx.scrolls.get(&id).copied().unwrap_or(0.0) * scale).clamp(0.0, max);
        if self.cx.reveal == Some(id) {
            self.cx.reveal = None;
            if cy - offset < 0.0 {
                offset = cy;
            }
            if cy + ch + 2.0 * pad_y - offset > r.h {
                offset = cy + ch + 2.0 * pad_y - r.h;
            }
            offset = offset.clamp(0.0, max).round();
        }
        self.cx.scrolls.insert(id, offset / scale);
        let (ox, oy) = (inner.x, (inner.y - offset).round());
        self.d.push_clip(IRect::covering(view), None);
        // 選んだ範囲
        if cursor != anchor && preedit.is_empty() {
            let (a, b) = (edit::utf16_of(&shown, cursor.min(anchor)), edit::utf16_of(&shown, cursor.max(anchor)));
            for rr in range_rects(&laid, a, b, self.px(4.0)) {
                self.d.fill(Rect::new(ox + rr.x, oy + rr.y, rr.w, rr.h), t.selection, 0.0);
            }
        }
        if shown.is_empty() {
            let h = self.cx.text.layout(hint, st, Some(inner.w));
            put(self.d, &h, ox, oy, if off { t.done_text } else { t.weak_text });
        } else {
            put(self.d, &laid, ox, oy, if off { t.done_text } else { t.text });
        }
        if !preedit.is_empty() {
            // 文節ごとに下線を引き、今変換している文節は太くする。行をまたぐ文節は行ごとに引く
            let bounds = clauses.as_ref().map(|c| c.bounds.clone()).filter(|b| b.last() == Some(&n16)).unwrap_or_else(|| vec![0, n16]);
            let target = clauses.as_ref().and_then(|c| c.target);
            let gap = (2.0 * scale).round();
            for w in bounds.windows(2) {
                let (a, b) = (w[0], w[1]);
                let thick = target.is_some_and(|(t0, t1)| a < t1 && t0 < b);
                let h = if thick { 2.0 * line } else { line };
                for rr in range_rects(&laid, start16 + a, start16 + b, 0.0) {
                    let wdt = (rr.w - if b < n16 { gap } else { 0.0 }).max(0.0);
                    self.d.fill(Rect::new((ox + rr.x).round(), (oy + rr.y + rr.h).round() - 2.0 * line, wdt.round(), h), t.text, 0.0);
                }
            }
            let (pa, pt, ph) = laid.caret(start16);
            let (pb, _, _) = laid.caret(start16 + n16);
            self.cx.preedit_rect = Some(Rect::new((ox + pa).round(), oy + pt, (pb - pa).max(0.0), ph + 3.0 * line));
        }
        let caret = Rect::new((ox + cx_).round(), (oy + cy).round(), (1.5 * scale).round().max(1.0), ch.round());
        if focused {
            let preedit_xs = if preedit.is_empty() { Vec::new() } else { (0..=n16).map(|k| (ox + laid.caret(start16 + k).0).round()).collect() };
            self.cx.ime_field = Some(ImeField { id, value: value.to_string(), cursor, anchor, preedit_xs });
            if caret.bottom() > view.y && caret.y < view.bottom() {
                self.d.fill(caret, t.text, 0.0);
            }
            self.cx.caret = Some(caret);
            self.cx.input_rect = Some(r);
            if preedit.is_empty() {
                self.cx.preedit_rect = Some(Rect::new(caret.x, caret.y, 0.0, ch + 3.0 * line));
            }
        }
        self.d.pop_clip();
        // ホイールとスクロールの棒（一覧と同じ仕組み。cx.scrolls を動かす）
        let (thumb, bar) = self.scrollbar_geometry(view, content_h, offset);
        self.prev.lists.push(ListInfo { id, rect: view, content_h, thumb, bar });
        if let Some(th) = thumb {
            self.draw_thumb(id, th);
        }
        self.prev.inputs.insert(id, (r, ox));
        self.prev.areas.insert(id, (ox, oy, inner.w, inner.h));
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
