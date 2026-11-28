//! 部品の大きさを測り、描く物の一覧を作る。次のフレームの当たり判定に使う情報（Prev）もここで作る。

use crate::geom::{Color, IRect, Rect};
use crate::paint::DrawList;
use crate::text::{FontId, GLYPH_CHUNK, Laid, Placed, TextStyle};
use crate::ime::ImeField;
use crate::ui::context::{Context, Hit, ListInfo, OverlayInfo, Prev};
use crate::ui::edit;
use crate::ui::layout::{self, Dir, Size};
use crate::ui::dock::{DockGeom, GroupGeom};
use crate::ui::{Kind, MenuMark, Overlay, OverlayKind, Place, TextKind, Tree};
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
    TextStyle { size: (kind.size() * cx.scale).round(), bold, font: cx.font }
}

/// 部品が書体を決めていればその書体、無ければ窓の書体
fn styled(cx: &Context, kind: TextKind, bold: bool, font: Option<FontId>) -> TextStyle {
    TextStyle { font: font.unwrap_or(cx.font), ..style(cx, kind, bold) }
}

fn px(cx: &Context, v: f32) -> f32 {
    (v * cx.scale).round()
}

/// 葉の部品の中身の大きさを測る
fn intrinsic(cx: &mut Context, kind: &Kind, max_w: Option<f32>) -> Option<(f32, f32)> {
    let s = cx.scale;
    let p = |v: f32| (v * s).round();
    Some(match kind {
        Kind::Label { text, size, bold, wrap, font, .. } => {
            let st = styled(cx, *size, *bold, *font);
            let laid = cx.text.layout(text, st, if *wrap { max_w } else { None });
            (if *wrap { 0.0 } else { laid.width.ceil() }, laid.height.ceil())
        }
        // 書式付きの文字は折り返すラベルと同じく、幅は置き場所で決まり、高さはその幅で並べて測る
        Kind::RichText { text, spans } => {
            let base = super::rich::base_style(cx);
            let runs = super::rich::runs(spans, base, cx.scale);
            let laid = cx.text.layout_runs(text, base, &runs, max_w);
            (0.0, laid.height.ceil())
        }
        Kind::CaptionButton { .. } => (p(46.0), p(super::TITLE_BAR_H)),
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
        Kind::MenuButton { text, icon, .. } => {
            if text.is_empty() && icon.is_some() {
                return Some((p(sz::BUTTON_H), p(sz::BUTTON_H)));
            }
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            ((laid.width.ceil() + p(sz::BUTTON_PAD) + p(sz::ICON_GAP) + p(16.0) + p(10.0)).max(p(sz::BUTTON_MIN_W)), p(sz::BUTTON_H))
        }
        Kind::MenuBarItem { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            (laid.width.ceil() + 2.0 * p(10.0), p(30.0))
        }
        Kind::MenuItem { text, shortcut, submenu, gutter, .. } => {
            let st = style(cx, TextKind::Body, false);
            let mut w = cx.text.layout(text, st, None).width.ceil() + 2.0 * p(12.0);
            if *gutter {
                w += p(16.0) + p(10.0);
            }
            if let Some(k) = shortcut {
                w += p(32.0) + cx.text.layout(k, st, None).width.ceil();
            }
            if *submenu {
                w += p(8.0) + p(16.0);
            }
            (w.max(p(140.0)), p(30.0))
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
        Kind::ToggleButton { text, icon, .. } => {
            // アイコンだけのボタンは正方形
            if text.is_empty() && icon.is_some() {
                return Some((p(sz::BUTTON_H), p(sz::BUTTON_H)));
            }
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            ((laid.width.ceil() + 2.0 * p(sz::BUTTON_PAD)).max(p(sz::BUTTON_MIN_W)), p(sz::BUTTON_H))
        }
        Kind::Tab { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, true), None);
            (laid.width.ceil() + 2.0 * p(12.0), p(38.0))
        }
        Kind::SpinButton { .. } => (p(28.0), p(24.0)),
        // 大きさは作る時に決める
        Kind::Splitter { .. } | Kind::Canvas { .. } | Kind::ColumnGrip | Kind::DockGrip { .. } => (0.0, 0.0),
        Kind::DateField { .. } => (p(160.0), p(sz::INPUT_H)),
        Kind::TimeField { .. } => (p(120.0), p(sz::INPUT_H)),
        Kind::CalendarDay { .. } => (p(36.0), p(32.0)),
        Kind::ClockCell { .. } => (p(40.0), p(32.0)),
        Kind::ColorField { .. } => (p(150.0), p(sz::INPUT_H)),
        Kind::ColorArea { .. } => (p(232.0), p(150.0)),
        Kind::HueBar { .. } | Kind::AlphaBar { .. } => (p(232.0), p(16.0)),
        Kind::Swatch { .. } => (p(24.0), p(24.0)),
        Kind::ColumnHeader { text, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Small, true), None);
            (laid.width.ceil() + p(28.0), p(32.0))
        }
        Kind::TreeItem { text, depth, .. } => {
            let laid = cx.text.layout(text, style(cx, TextKind::Body, false), None);
            (laid.width.ceil() + p(8.0) + *depth as f32 * p(20.0) + p(20.0) + p(12.0), p(30.0))
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
        Kind::TextArea { rows, font, .. } => {
            let line_h = cx.text.layout("あ", styled(cx, TextKind::Body, false, *font), None).height.ceil();
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
    // 重ねて出す物（ダイアログ・ポップアップ）の中はまだ置かれていない（幅 0）ので、ここでは測り直さない。
    // place_overlays が、置いてから測り直す
    let mut in_overlay = vec![false; tree.info.len()];
    for o in &tree.overlays {
        for i in subtree(tree, o.idx) {
            in_overlay[i] = true;
        }
    }
    let mut again = false;
    for (i, &overlaid) in in_overlay.iter().enumerate() {
        if overlaid {
            continue;
        }
        if let Kind::Label { wrap: true, .. } | Kind::RichText { .. } = tree.info[i].kind {
            let w = tree.layout[i].rect.w;
            if let Some(size) = intrinsic(cx, &tree.info[i].kind, Some(w))
                && (size.1 - tree.layout[i].intrinsic.1).abs() > 0.5 {
                    tree.layout[i].intrinsic = size;
                    again = true;
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
    if let Some(target) = cx.scroll_into_view.take()
        && let Some(t) = tree.info.iter().position(|n| n.id == target) {
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
    place_overlays(tree, cx, window);
    // 書式付きの文字の部分（子）の四角は、置いた後の幅で並べた結果から決める
    super::rich::place_spans(tree, cx);
}

/// 重ねて出す物の中で折り返す文の、望む幅の上限（論理ピクセル。読みやすい長さの 1 行）
const OVERLAY_TEXT_W: f32 = 480.0;

/// 重ねて出す物を置く（ふつうの配置の後。出す元の部品の場所が決まってから）
fn place_overlays(tree: &mut Tree, cx: &mut Context, window: Rect) {
    let (margin, gap) = (px(cx, 8.0), px(cx, 4.0));
    // ダイアログは、kargui が描く題の帯の下の真ん中に出す
    let body = tree.info.iter().position(|n| matches!(n.kind, Kind::TitleBar { .. })).map_or(window, |i| {
        let top = tree.layout[i].rect.bottom();
        Rect::new(window.x, top, window.w, (window.bottom() - top).max(0.0))
    });
    let max_text = px(cx, OVERLAY_TEXT_W).min((window.w - 2.0 * margin).max(0.0));
    for o in tree.overlays.clone() {
        // 重ねて出す物の幅は中身で決まる。折り返す文は幅 0 と数えると 1 語ずつ折り返して縦に長くなるので、
        // 1 行の幅（max_text まで）を望む幅にし、その幅での高さで測る
        let wrapped: Vec<usize> = subtree(tree, o.idx).into_iter().filter(|&i| matches!(tree.info[i].kind, Kind::Label { wrap: true, .. } | Kind::RichText { .. })).collect();
        for &i in &wrapped {
            let w = one_line_width(cx, &tree.info[i].kind).min(max_text);
            if let Some((_, h)) = intrinsic(cx, &tree.info[i].kind, Some(w)) {
                tree.layout[i].intrinsic = (w, h);
            }
        }
        place_overlay(tree, &o, window, body, margin, gap);
        // 置いた幅が望む幅と違えば（入れ物の幅が決まっている時など）、その幅で高さを測り直して置き直す
        let mut again = false;
        for &i in &wrapped {
            let w = tree.layout[i].rect.w;
            if let Some((_, h)) = intrinsic(cx, &tree.info[i].kind, Some(w))
                && (h - tree.layout[i].intrinsic.1).abs() > 0.5 {
                    tree.layout[i].intrinsic.1 = h;
                    again = true;
                }
        }
        if again {
            place_overlay(tree, &o, window, body, margin, gap);
        }
    }
}

/// 重ねて出す物 1 つの大きさを測り、場所を決めて中を並べる
fn place_overlay(tree: &mut Tree, o: &Overlay, window: Rect, body: Rect, margin: f32, gap: f32) {
    let (w, h) = layout::measure(&mut tree.layout, o.idx);
    let w = w.min((window.w - 2.0 * margin).max(0.0));
    let h = h.min((window.h - 2.0 * margin).max(0.0));
    let anchor = o.anchor.and_then(|a| tree.info.iter().position(|n| n.id == a)).map(|i| tree.layout[i].rect);
    let rect = match (o.kind, o.place, anchor) {
        (OverlayKind::Dialog, ..) => layout::place_center(w, h.min((body.h - 2.0 * margin).max(0.0)), body),
        (_, Place::At(x, y), _) => layout::place_near(Rect::new(x, y, 0.0, 0.0), w, h, window, margin, 0.0),
        (_, Place::Beside, Some(a)) => layout::place_beside(a, w, h, window, margin, gap),
        (_, _, Some(a)) => layout::place_near(a, w, h, window, margin, gap),
        _ => layout::place_center(w, h, window),
    };
    layout::arrange(&mut tree.layout, o.idx, rect);
}

/// 部品 i とその下の部品
fn subtree(tree: &Tree, i: usize) -> Vec<usize> {
    let mut out = vec![i];
    let mut k = 0;
    while k < out.len() {
        out.extend(tree.layout[out[k]].children.iter().copied());
        k += 1;
    }
    out
}

/// 折り返す文を 1 行に並べた時の幅
fn one_line_width(cx: &mut Context, kind: &Kind) -> f32 {
    match kind {
        Kind::Label { text, size, bold, font, .. } => {
            let st = styled(cx, *size, *bold, *font);
            cx.text.layout(text, st, None).width.ceil()
        }
        Kind::RichText { text, spans } => {
            let base = super::rich::base_style(cx);
            let runs = super::rich::runs(spans, base, cx.scale);
            cx.text.layout_runs(text, base, &runs, None).width.ceil()
        }
        _ => 0.0,
    }
}

/// a の下に t があるか（a 自身を含まない）
fn contains(tree: &Tree, a: usize, t: usize) -> bool {
    tree.layout[a].children.iter().any(|&c| c == t || contains(tree, c, t))
}

/// 並べた字のうち、切り取り範囲 clip と重なる物と、その形の左上（整数の画素）。x, y は並べた範囲の左上の位置。
/// 重ならない字は描いても画素にならないので、描く物に入れない（200 万字の入力欄で、見えない字まで毎フレーム
/// 描く物に入れ、何も変わらないフレームでも 160 ms かかった。2026-11-28 の X11 の記録）。
/// 字のまとまりごとの範囲（Laid::chunks）で、重ならないまとまりを丸ごと飛ばす
fn visible_glyphs(laid: &Laid, x: f32, y: f32, clip: IRect) -> impl Iterator<Item = (&Placed, f32, f32)> {
    // 並べた範囲の座標にした切り取り範囲。字の位置は画素に丸めるので 1 画素広げる
    let (x0, y0) = (clip.x as f32 - x - 1.0, clip.y as f32 - y - 1.0);
    let (x1, y1) = (clip.right() as f32 - x + 1.0, clip.bottom() as f32 - y + 1.0);
    laid.chunks
        .iter()
        .enumerate()
        .filter(move |(_, c)| c[0] < x1 && c[2] > x0 && c[1] < y1 && c[3] > y0)
        .flat_map(move |(k, _)| &laid.glyphs[k * GLYPH_CHUNK..((k + 1) * GLYPH_CHUNK).min(laid.glyphs.len())])
        .filter_map(move |g| {
            let gx = (x + g.x).round() + g.slot.left as f32;
            let gy = (y + g.y).round() + g.slot.top as f32;
            IRect::new(gx as i32, gy as i32, g.slot.w as i32, g.slot.h as i32).intersect(&clip).map(|_| (g, gx, gy))
        })
}

fn put(d: &mut DrawList, laid: &Laid, x: f32, y: f32, color: Color) {
    for (g, gx, gy) in visible_glyphs(laid, x, y, d.clip()) {
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
    /// 今描いているのがツールバーの中（ボタンが左右の矢印を受ける）
    toolbar: bool,
}

/// 画面の木を描く。次のフレームの当たり判定の情報を cx.prev に入れる
pub(crate) fn paint(tree: &Tree, cx: &mut Context, d: &mut DrawList) {
    let window = IRect::new(0, 0, cx.width as i32, cx.height as i32);
    d.begin(window);
    cx.caret = None;
    cx.preedit_rect = None;
    cx.input_rect = None;
    cx.ime_field = None;
    let mut p = Painter { cx, d, prev: Prev::default(), clip: window.to_rect(), row: None, scroller: None, toolbar: false };
    p.node(tree, 0);
    for o in &tree.overlays {
        p.overlay(tree, o);
    }
    p.drag_ghost();
    // kargui が題の帯を描く浮いた窓は、合成が影も縁も描かないので、窓の縁に線を引く（重なった窓の境が分かるように）
    if p.cx.csd.as_ref().is_some_and(|c| !c.tiled && !c.windows) {
        let (line, color) = (p.line(), p.cx.theme.border);
        p.outline(window.to_rect(), color, line);
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

/// 2 つの色の間の色（p が 0 なら a、1 なら b）
fn mix(a: Color, b: Color, p: f32) -> Color {
    [a[0] + (b[0] - a[0]) * p, a[1] + (b[1] - a[1]) * p, a[2] + (b[2] - a[2]) * p, a[3] + (b[3] - a[3]) * p]
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
        let arrows = enabled
            && matches!(
                tree.info[i].kind,
                Kind::Slider { .. }
                    | Kind::Dropdown { .. }
                    | Kind::ListOption { .. }
                    | Kind::MenuButton { .. }
                    | Kind::MenuBarItem { .. }
                    | Kind::MenuItem { .. }
                    | Kind::Tab { .. }
                    | Kind::Splitter { .. }
                    | Kind::TreeItem { .. }
                    | Kind::ColumnHeader { .. }
                    | Kind::CalendarDay { .. }
                    | Kind::ClockCell { .. }
                    | Kind::ColorArea { .. }
                    | Kind::HueBar { .. }
                    | Kind::AlphaBar { .. }
            )
            || (enabled && focusable && self.toolbar);
        let vertical = matches!(tree.info[i].kind, Kind::TextArea { .. } | Kind::NumberInput { .. });
        // 引っ張りを自分で使う部品（この上からはドラッグ＆ドロップを始めない）
        let drags = text_input
            || matches!(
                tree.info[i].kind,
                Kind::Slider { .. }
                    | Kind::Splitter { .. }
                    | Kind::Canvas { .. }
                    | Kind::ColumnGrip
                    | Kind::DockGrip { .. }
                    | Kind::TextArea { .. }
                    | Kind::NumberInput { .. }
                    | Kind::ColorArea { .. }
                    | Kind::HueBar { .. }
                    | Kind::AlphaBar { .. }
            );
        let link = matches!(tree.info[i].kind, Kind::Link { .. } | Kind::RichSpan { link: true, .. });
        self.prev.hits.push(Hit { id, rect: r, slop, focusable, text_input, arrows, vertical, enabled, drags, link });
        // メニューバー・タブ・ツールバーは、まとまりの中の 1 つにだけ Tab で止まる
        if focusable && !tree.info[i].skip_tab && !matches!(tree.info[i].kind, Kind::MenuBarItem { tab_stop: false, .. }) {
            self.prev.focus_order.push(id);
        }
    }

    /// アイコンを (x, y) から s × s 画素で描く（形の表に入れて、文字と同じく色を付けて描く）
    fn icon(&mut self, icon: Icon, x: f32, y: f32, s: f32, color: Color) {
        if let Some(slot) = self.cx.icon_slot(icon, s as u32) {
            self.d.glyph(x, y, slot.w as f32, slot.h as f32, slot.x as f32, slot.y as f32, color);
        }
    }

    /// 一覧やメニューの項目の背景（bg が無ければ塗らない）と、キーボードの焦点の枠（内側に 2 本分）。
    /// 枠の内側は背景の色で塗り直す（boxed は外側を枠の色で塗るので、透明は渡せない）
    /// 色の選択の濃淡の絵（同じ鍵の絵は作り直さず、最近の 8 枚を覚える）
    fn color_image(&mut self, key: String, w: u32, h: u32, make: impl FnOnce() -> Vec<u8>) -> Option<crate::image::Image> {
        let cache = &mut self.cx.color_images;
        if let Some(k) = cache.iter().position(|(c, _)| *c == key) {
            let hit = cache.remove(k);
            let img = hit.1.clone();
            cache.push(hit);
            return Some(img);
        }
        let img = crate::image::Image::from_rgba(w, h, make()).ok()?;
        if cache.len() >= 8 {
            cache.remove(0);
        }
        cache.push((key, img.clone()));
        Some(img)
    }

    /// 四角の縁（中は塗らない）
    fn outline(&mut self, r: Rect, color: Color, w: f32) {
        for e in [Rect::new(r.x, r.y, r.w, w), Rect::new(r.x, r.bottom() - w, r.w, w), Rect::new(r.x, r.y, w, r.h), Rect::new(r.right() - w, r.y, w, r.h)] {
            self.d.fill(e, color, 0.0);
        }
    }

    /// 色の見本の四角。透けている色は、市松模様の上に重ねて見せる
    fn swatch_box(&mut self, r: Rect, color: Color, border: Color, radius: f32) {
        let line = self.line();
        self.d.fill(r, border, radius);
        let inner = r.inset(line);
        if color[3] < 1.0 {
            self.d.fill(inner, [1.0, 1.0, 1.0, 1.0], (radius - line).max(0.0));
            let cell = (inner.w / 4.0).max(1.0).round();
            let mut y = inner.y;
            let mut row = 0;
            while y < inner.bottom() {
                let mut x = inner.x + if row % 2 == 0 { 0.0 } else { cell };
                while x < inner.right() {
                    let c = Rect::new(x, y, cell.min(inner.right() - x), cell.min(inner.bottom() - y));
                    self.d.fill(c, [0.8, 0.8, 0.8, 1.0], 0.0);
                    x += 2.0 * cell;
                }
                y += cell;
                row += 1;
            }
        }
        self.d.fill(inner, color, (radius - line).max(0.0));
    }

    /// 色の選択のつまみ（白い輪と暗い縁。どの色の上でも見える）
    fn color_knob(&mut self, x: f32, y: f32, color: Color) {
        let (o, m, n) = (self.px(9.0), self.px(8.0), self.px(5.5));
        let at = |rad: f32| Rect::new((x - rad).round(), (y - rad).round(), (2.0 * rad).round(), (2.0 * rad).round());
        self.d.fill(at(o), [0.0, 0.0, 0.0, 0.45], o);
        self.d.fill(at(m), [1.0, 1.0, 1.0, 1.0], m);
        self.d.fill(at(n), color, n);
    }

    /// 色相と不透明度の帯: 絵と、位置 f（0〜1）の縦のつまみ
    fn bar_with_knob(&mut self, tree: &Tree, i: usize, r: Rect, img: Option<crate::image::Image>, f: f32) {
        let radius = self.px(4.0);
        self.focus_ring(tree, i, radius);
        if let Some(img) = img {
            self.d.image(r, &img);
        }
        let t = self.cx.theme.clone();
        let line = self.line();
        self.outline(r, t.border, line);
        let (w, over) = (self.px(6.0), self.px(3.0));
        let x = (r.x + f.clamp(0.0, 1.0) * (r.w - 1.0) - w / 2.0).round();
        let knob = Rect::new(x, r.y - over, w, r.h + 2.0 * over);
        self.d.boxed(knob, [1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 0.55], line, self.px(2.0));
        self.hit(tree, i, true, false);
    }

    fn item_bg(&mut self, r: Rect, bg: Option<Color>, focused: bool, radius: f32) {
        let line = self.line();
        let (accent, base) = (self.cx.theme.accent, self.cx.theme.background);
        if focused {
            self.d.boxed(r.inset(line), bg.unwrap_or(base), accent, 2.0 * line, radius);
        } else if let Some(b) = bg {
            self.d.fill(r, b, radius);
        }
    }

    /// 自由に描く面の中身（面の外は描かない）
    fn canvas(&mut self, r: Rect, ops: &[crate::ui::DrawOp]) {
        use crate::ui::DrawOp;
        let s = self.cx.scale;
        let at = |(x, y): (f32, f32)| (r.x + x * s, r.y + y * s);
        let rect = |q: &Rect| Rect::new(r.x + q.x * s, r.y + q.y * s, q.w * s, q.h * s);
        self.d.push_clip(IRect::covering(r));
        for op in ops {
            match op {
                DrawOp::Rect { r: q, color, radius } => self.d.fill(rect(q), *color, radius * s),
                DrawOp::Line { points, width, color } => {
                    let pts: Vec<(f32, f32)> = points.iter().map(|&p| at(p)).collect();
                    let polys = crate::vector::stroke(&pts, (width * s).max(1.0));
                    self.mask(&polys, *color);
                }
                DrawOp::Polygon { points, color } => {
                    let pts: Vec<(f32, f32)> = points.iter().map(|&p| at(p)).collect();
                    self.mask(&[crate::vector::solid(pts)], *color);
                }
                DrawOp::Text { at: p, text, color } => {
                    let laid = self.cx.text.layout(text, style(self.cx, TextKind::Body, false), None);
                    let (x, y) = at(*p);
                    put(self.d, &laid, x.round(), y.round(), *color);
                }
                DrawOp::Image { r: q, image } => self.d.image(rect(q), image),
            }
        }
        self.d.pop_clip();
    }

    /// 多角形の集まりを濃さの形に塗って形の表に入れ、color で描く（斜めの線と多角形。CPU と GPU のどちらでも描ける）。
    /// 同じ形（位置の端数を含む）は表の同じ場所を使う
    fn mask(&mut self, polys: &[Vec<(f32, f32)>], color: Color) {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in polys.iter().flatten() {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        if x0 > x1 {
            return;
        }
        let (x0, y0) = (x0.floor(), y0.floor());
        let (w, h) = ((x1.ceil() - x0) as u32 + 1, (y1.ceil() - y0) as u32 + 1);
        // 大きすぎる形は描かない（表に入らない）
        if w > 2048 || h > 2048 {
            crate::log::event("canvas.mask", &format!("w={w} h={h}"), "too large; skipped");
            return;
        }
        let local: Vec<Vec<(f32, f32)>> = polys.iter().map(|p| p.iter().map(|&(x, y)| (x - x0, y - y0)).collect()).collect();
        let key = super::hash_of((w, h, local.iter().map(|p| p.iter().map(|&(x, y)| ((x * 16.0) as i32, (y * 16.0) as i32)).collect::<Vec<_>>()).collect::<Vec<_>>()));
        let make = || {
            let mut ras = crate::vector::Raster::new(w as usize, h as usize);
            for p in &local {
                ras.polygon(p);
            }
            ras.coverage()
        };
        if let Some(slot) = self.cx.mask_slot(key, w, h, make) {
            self.d.glyph(x0, y0, w as f32, h as f32, slot.x as f32, slot.y as f32, color);
        }
    }

    fn focus_ring(&mut self, tree: &Tree, i: usize, radius: f32) {
        if self.cx.focus_visible && self.cx.focus == Some(tree.info[i].id) {
            self.ring(tree.layout[i].rect, radius);
        }
    }

    /// 焦点の枠を r のまわりに描く（主の色の外枠と、背景の色の内枠）
    fn ring(&mut self, r: Rect, radius: f32) {
        let w = self.px(2.0);
        let t = &self.cx.theme;
        let (accent, bg) = (t.accent, t.background);
        self.d.fill(r.inset(-2.0 * w), accent, radius + 2.0 * w);
        self.d.fill(r.inset(-w), bg, radius + w);
    }

    /// 書式付きの文字: 1 つの段落に並べ、部分ごとに色・下線・取り消し線・コードの背景を付けて描く。
    /// リンクの部分（子）には、行ごとの当たりと焦点の枠を入れる
    fn rich_text(&mut self, tree: &Tree, i: usize, text: &str, spans: &[super::rich::SpanStyle]) {
        use super::rich;
        let r = tree.layout[i].rect;
        let line = self.line();
        let t = self.cx.theme.clone();
        let off = tree.info[i].disabled;
        let base = rich::base_style(self.cx);
        let laid = self.cx.text.layout_runs(text, base, &rich::runs(spans, base, self.cx.scale), Some(r.w));
        let children = &tree.layout[i].children;
        let color_of = |s: &rich::SpanStyle| -> Color {
            if off {
                t.done_text
            } else if s.link {
                t.accent
            } else if let Some(c) = s.color {
                c
            } else if s.weak {
                t.weak_text
            } else {
                t.text
            }
        };
        let text16: Vec<u16> = text.encode_utf16().collect();
        let segs: Vec<Vec<(usize, f32, f32)>> = spans.iter().map(|s| rich::segments(&laid, &text16, s.start, s.end)).collect();
        let place = |n: usize, x0: f32, x1: f32| {
            let l = laid.lines[n];
            Rect::new((r.x + x0).floor(), (r.y + l.top).floor(), (x1 - x0).ceil(), l.height.ceil())
        };
        // コードの背景（文字の下に、左右に少し広げて）。文字の色を薄く重ねる（テーマのボタンの色は白の上で見えないほど薄い）
        let pad = self.px(3.0);
        let code_bg = [t.text[0], t.text[1], t.text[2], if self.cx.dark { 0.12 } else { 0.08 }];
        for (_, seg) in spans.iter().zip(&segs).filter(|(s, _)| s.code) {
            for &(n, x0, x1) in seg {
                let b = place(n, x0, x1);
                self.d.fill(Rect::new(b.x - pad, b.y, b.w + 2.0 * pad, b.h), code_bg, self.px(3.0));
            }
        }
        // 焦点のあるリンクの枠（行ごと）
        for (k, &c) in children.iter().enumerate() {
            if spans.get(k).is_some_and(|s| s.link) && self.cx.focus_visible && self.cx.focus == Some(tree.info[c].id) {
                for &(n, x0, x1) in &segs[k] {
                    self.ring(place(n, x0, x1), self.px(2.0));
                }
            }
        }
        // 文字の形（部分ごとの色。切り取り範囲と重なる字だけ）
        for (g, gx, gy) in visible_glyphs(&laid, r.x, r.y, self.d.clip()) {
            let color = rich::span_at(spans, g.pos as usize).map_or(if off { t.done_text } else { t.text }, |k| color_of(&spans[k]));
            self.d.glyph(gx, gy, g.slot.w as f32, g.slot.h as f32, g.slot.x as f32, g.slot.y as f32, color);
        }
        // 下線（リンクは乗った時に太く）と取り消し線。どちらもベースラインから測る
        for (k, s) in spans.iter().enumerate() {
            if !(s.underline || s.strike) {
                continue;
            }
            let hovered = s.link && !off && children.get(k).is_some_and(|&c| self.cx.hover == Some(tree.info[c].id));
            let size = s.size.map_or(base.size, |kind| (kind.size() * self.cx.scale).round());
            let color = color_of(s);
            for &(n, x0, x1) in &segs[k] {
                let b = place(n, x0, x1);
                let baseline = r.y + laid.lines[n].baseline;
                if s.underline {
                    let thick = if hovered { 2.0 * line } else { line };
                    let y = (baseline + (size * 0.12).max(line)).round();
                    self.d.fill(Rect::new(b.x, y, b.w, thick), color, 0.0);
                }
                if s.strike {
                    let y = (baseline - size * 0.3).round();
                    self.d.fill(Rect::new(b.x, y, b.w, line), color, 0.0);
                }
            }
        }
        // リンクの当たり: 最初の行は子の四角（Tab の順番にも入る）、2 行目からは同じ id の四角を足す
        for (k, &c) in children.iter().enumerate() {
            if !spans.get(k).is_some_and(|s| s.link) {
                continue;
            }
            self.hit(tree, c, true, false);
            let enabled = !tree.info[c].disabled;
            for &(n, x0, x1) in segs[k].iter().skip(1) {
                if let Some(v) = intersect(place(n, x0, x1), self.clip) {
                    let id = tree.info[c].id;
                    self.prev.hits.push(Hit {
                        id,
                        rect: v,
                        slop: v,
                        focusable: enabled,
                        text_input: false,
                        arrows: false,
                        vertical: false,
                        enabled,
                        drags: false,
                        link: true,
                    });
                }
            }
        }
    }

    /// 部品を描く。引っ張っている荷物を受け取れる部品の上にポインターがあれば、中身の上に強調を重ねる
    fn node(&mut self, tree: &Tree, i: usize) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        // 引っ張り出せる部品と落とせる部品は、見えている範囲と、ここまでの当たりの数を覚える（後から上に描いた物に
        // 隠れているかを見るため）
        let (source, target) = (tree.info[i].drag_source, tree.info[i].drop_type);
        if (source || target.is_some())
            && let Some(c) = intersect(r, self.clip)
        {
            let order = self.prev.hits.len();
            if source {
                self.prev.drag_sources.push((id, c, super::a11y::drag_text(tree, i), order));
            }
            if let Some(t) = target {
                self.prev.drop_targets.push((id, c, t, order));
            }
        }
        let regions = self.prev.context_regions.len();
        // ハイコントラストの押せない部品は、縁・文字・印をどれも GrayText で描く。描く間だけ配色を差し替える
        let keep = (self.cx.high_contrast && tree.info[i].disabled).then(|| {
            let off = self.cx.theme.disabled_high_contrast();
            std::mem::replace(&mut self.cx.theme, off)
        });
        if tree.layout[i].reveal.is_some() || tree.layout[i].clip {
            // 一部だけ見せる物と切り取る物（表の升）: 範囲の外を切り取る。外に出た子は押せず、Tab でも移らない
            self.d.push_clip(IRect::covering(r));
            let outer = self.clip;
            self.clip = intersect(r, outer).unwrap_or(Rect::new(r.x, r.y, 0.0, 0.0));
            self.node_body(tree, i);
            self.clip = outer;
            self.d.pop_clip();
        } else {
            self.node_body(tree, i);
        }
        if let Some(t) = keep {
            self.cx.theme = t;
        }
        // 右クリックの範囲は、中身を描き終えた時の当たりの数を持つ（中に浮いたまとまりがあっても、外側の範囲は隠れない）
        if tree.info[i].context_menu
            && let Some(region) = self.prev.context_regions.get_mut(regions).filter(|r| r.0 == id)
        {
            region.2 = self.prev.hits.len();
        }
        if let Some((at, source)) = self.cx.dnd.as_ref().filter(|d| d.over == Some(id)).map(|d| (d.at, d.source))
            && let Some(c) = intersect(r, self.clip)
        {
            // ドッキングは全体を囲まず、落とす先の的と落ちる所を描く
            if matches!(tree.info[i].kind, Kind::Dock) {
                self.dock_overlay(&dock_geom(tree, i, self.cx.window_key.is_some()), at, source);
            } else {
                self.drop_highlight(c);
            }
        }
    }

    /// ドッキングにタブを引っ張っている間: 落ちる所を薄く塗る（落とす先はポインターのいる場所で決まるので、的は描かない）
    fn dock_overlay(&mut self, geom: &DockGeom, at: (f32, f32), source: crate::ui::Id) {
        let scale = self.cx.scale;
        let a = self.cx.theme.accent;
        let line = self.line();
        let zone = geom.zone_for(at.0, at.1, scale, source);
        // ポインターの下のまとまりと落とす先が変わったら記録する（どこを通ったか、記録から分かるように）
        let now = format!("group={:?} zone={zone:?}", geom.group_at(at.0, at.1));
        if self.cx.dock_zone_log.as_deref() != Some(now.as_str()) {
            crate::log::event("dock.zone", &format!("at={:.0},{:.0}", at.0, at.1), &now);
            self.cx.dock_zone_log = Some(now);
        }
        if let Some(r) = zone.and_then(|z| geom.preview(z)) {
            // 半透明の塗りと枠は別々に描く（boxed は枠の色の上に中を重ねるので、中が透けると枠の色で埋まる）
            self.d.fill(r, [a[0], a[1], a[2], 0.18], self.px(4.0));
            self.outline(r, a, 2.0 * line);
        }
    }

    /// 荷物を受け取れる部品の強調: 薄い主の色で塗り、主の色の枠を内側に 2 本分
    fn drop_highlight(&mut self, r: Rect) {
        let a = self.cx.theme.accent;
        let w = 2.0 * self.line();
        self.d.fill(r, [a[0], a[1], a[2], 0.10], self.px(4.0));
        for e in [Rect::new(r.x, r.y, r.w, w), Rect::new(r.x, r.bottom() - w, r.w, w), Rect::new(r.x, r.y, w, r.h), Rect::new(r.right() - w, r.y, w, r.h)] {
            self.d.fill(e, a, 0.0);
        }
    }

    /// 引っ張っている物の影: 元の部品の大きさ（大きすぎれば縮める）の箱に名前を書き、つかんだ所がポインターの下に来るように
    /// 描く。重ねて出した物よりも上
    fn drag_ghost(&mut self) {
        // ほかのアプリからの引っ張りは、OS（IDropTargetHelper）が引っ張っている物の絵を描く
        let Some(d) = self.cx.dnd.as_ref().filter(|d| !d.external) else { return };
        let (w, h) = (d.rect.w.min(self.px(360.0)), d.rect.h.min(self.px(120.0)));
        let (gx, gy) = (d.grab.0.min(w), d.grab.1.min(h));
        let r = Rect::new((d.at.0 - gx).round(), (d.at.1 - gy).round(), w, h);
        let text = d.text.clone();
        let t = self.cx.theme.clone();
        let (radius, sh) = (self.px(4.0), self.px(3.0));
        // ハイコントラストでは影を描かない（薄い黒は配色の外の色。境は縁で見せる）
        if !self.cx.high_contrast {
            self.d.fill(Rect::new(r.x - sh, r.y, r.w + 2.0 * sh, r.h + 2.0 * sh), [0.0, 0.0, 0.0, 0.12], radius + sh);
        }
        self.d.fill(r, t.accent, radius);
        let line = self.line();
        let bg = t.background;
        self.d.fill(r.inset(line), [bg[0], bg[1], bg[2], 0.94], (radius - line).max(0.0));
        self.d.push_clip(IRect::covering(r));
        let st = style(self.cx, TextKind::Body, false);
        text_mid(self.d, self.cx, &text, st, r.x + self.px(8.0), r, t.text);
        self.d.pop_clip();
    }

    fn node_body(&mut self, tree: &Tree, i: usize) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let line = self.line();
        // 右クリックのメニューを持つ部品は、見えている範囲を覚える（右クリックの場所から部品を探す）
        if tree.info[i].context_menu
            && let Some(c) = intersect(r, self.clip) {
                self.prev.context_regions.push((id, c, 0));
            }
        match &tree.info[i].kind {
            // 重ねる物は親に付かないので、ここには来ない（Painter::overlay で描く）
            Kind::Overlay { .. } => {}
            Kind::Dock => {
                // 全体とまとまりの場所を覚える（次のフレームで、タブを落とした先を決める）
                self.prev.docks.insert(id, dock_geom(tree, i, self.cx.window_key.is_some()));
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::DockFloat => {
                // 影（少し下にずらした薄い黒）と背景の箱。下の部品へ押すのが抜けないように、範囲全体を押せない当たりに
                // 入れ、先に描いた一覧・スクロールの棒・右クリックの範囲を、この範囲では隠す
                let t = self.cx.theme.clone();
                let (radius, sh) = (self.px(6.0), self.px(3.0));
                if !self.cx.high_contrast {
                    self.d.fill(Rect::new(r.x - sh, r.y, r.w + 2.0 * sh, r.h + 2.0 * sh), [0.0, 0.0, 0.0, 0.10], radius + sh);
                }
                self.d.boxed(r, t.background, t.border, line, radius);
                if let Some(c) = intersect(r, self.clip) {
                    self.prev.occluders.push((c, self.prev.hits.len()));
                    self.prev.hits.push(Hit { id, rect: c, slop: c, focusable: false, text_input: false, arrows: false, vertical: false, enabled: false, drags: false, link: false });
                }
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::DockGrip { resize } => {
                // 動かす印は 2 列 3 段の点、大きさを変える印は右下の隅に斜めに並べた点。押している・乗っている時は主の色
                self.prev.shapes.insert(id, if *resize { super::context::PointerShape::NwseResize } else { super::context::PointerShape::Move });
                let t = self.cx.theme.clone();
                let hot = self.cx.pressed == Some(id) || self.cx.hover == Some(id);
                let color = if hot { t.accent } else { t.weak_text };
                let (dot, step) = (self.px(2.0).round().max(1.0), self.px(4.0).round().max(2.0));
                let spots: Vec<(f32, f32)> = if *resize {
                    let x0 = r.right() - self.px(3.0).round() - 2.0 * step - dot;
                    let y0 = r.bottom() - self.px(3.0).round() - 2.0 * step - dot;
                    (0..3).flat_map(|gy| (0..3).map(move |gx| (gx, gy))).filter(|(gx, gy)| gx + gy >= 2).map(|(gx, gy)| (x0 + gx as f32 * step, y0 + gy as f32 * step)).collect()
                } else {
                    let x0 = (r.x + (r.w - step - dot) / 2.0).round();
                    let y0 = (r.y + (r.h - 2.0 * step - dot) / 2.0).round();
                    (0..3).flat_map(|gy| (0..2).map(move |gx| (x0 + gx as f32 * step, y0 + gy as f32 * step))).collect()
                };
                for (x, y) in spots {
                    self.d.fill(Rect::new(x, y, dot, dot), color, dot / 2.0);
                }
                // 動かす印の下にも、隣の見出しの列と同じ線を引く（線が印の所で切れないように）
                if !*resize {
                    self.d.fill(Rect::new(r.x, r.bottom() - line, r.w, line), t.border, 0.0);
                }
                self.hit(tree, i, false, false);
            }
            Kind::Root | Kind::Row | Kind::Column | Kind::Stack | Kind::Space | Kind::MenuBar | Kind::Tabs | Kind::DockGroup | Kind::TabPanel | Kind::Grid | Kind::Tree | Kind::Table { .. } | Kind::DayGrid => {
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::Label { text, size, bold, weak, strike, wrap, right, font } => {
                let st = styled(self.cx, *size, *bold, *font);
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
            Kind::RichText { text, spans } => self.rich_text(tree, i, text, spans),
            // 部分は親（RichText）が描く
            Kind::RichSpan { .. } => {}
            Kind::Item { selected, tab_stop } => {
                let t = self.cx.theme.clone();
                let enabled = !tree.info[i].disabled;
                if *selected && self.cx.high_contrast {
                    self.d.boxed(r, t.selected, t.accent, self.px(2.0), 0.0);
                } else if *selected {
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
                    self.prev.hits.push(Hit { id, rect: v, slop: v, focusable: enabled, text_input: false, arrows: enabled, vertical: false, enabled, drags: false, link: false });
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
                // ハイコントラストの押せない主のボタンは塗らない（塗ると押せるボタンより目立つ）
                let filled = *primary && !(off && self.cx.high_contrast);
                if filled {
                    // 押せない主のボタンは、アクセントの色の代わりに枠の色で塗る
                    let mut bg = if off { t.border } else { t.accent };
                    if !off && (self.cx.pressed == Some(id) || self.cx.hover == Some(id)) {
                        bg = [bg[0] * 0.88, bg[1] * 0.88, bg[2] * 0.88, 1.0];
                    }
                    self.d.fill(r, bg, radius);
                } else if self.toolbar {
                    // ツールバーの中は枠を描かず、乗った時と押した時だけ塗る
                    if !off && (self.cx.pressed == Some(id) || self.cx.hover == Some(id)) {
                        self.d.fill(r, state_bg, radius);
                    }
                } else {
                    self.d.boxed(r, state_bg, t.border, line, radius);
                }
                let st = style(self.cx, if *small { TextKind::Small } else { TextKind::Body }, *primary);
                let laid = self.cx.text.layout(text, st, None);
                let color = if filled {
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
                    self.prev.hits.push(Hit { id, rect: v, slop: v, focusable: enabled, text_input: false, arrows: enabled, vertical: false, enabled, drags: false, link: false });
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
                let bg = if *selected {
                    Some(t.selected)
                } else if hot {
                    Some(t.button_hover)
                } else {
                    None
                };
                self.item_bg(r, bg, self.cx.focus_visible && self.cx.focus == Some(id), radius);
                if *selected {
                    // 選んでいる項目は左に短い縦の線
                    let bar = Rect::new(r.x, (r.y + r.h * 0.25).round(), self.px(3.0), (r.h * 0.5).round());
                    self.d.fill(bar, t.accent, bar.w / 2.0);
                }
                text_mid(self.d, self.cx, text, style(self.cx, TextKind::Body, false), r.x + self.px(12.0), r, t.text);
                self.hit(tree, i, true, false);
            }
            Kind::MenuButton { text, open, icon } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let bg = if off {
                    t.button
                } else if *open || self.cx.pressed == Some(id) {
                    t.button_pressed
                } else if self.cx.hover == Some(id) {
                    t.button_hover
                } else {
                    t.button
                };
                if !self.toolbar {
                    self.d.boxed(r, bg, t.border, line, radius);
                } else if !off && (*open || self.cx.pressed == Some(id) || self.cx.hover == Some(id)) {
                    self.d.fill(r, bg, radius);
                }
                let color = if off { t.done_text } else { t.text };
                if let (true, Some(ic)) = (text.is_empty(), icon) {
                    // アイコンだけ（ツールバーの「ほか」）: 下向きの印は付けない
                    let s = self.px(button_icon(false));
                    self.icon(*ic, (r.x + (r.w - s) / 2.0).round(), (r.y + (r.h - s) / 2.0).round(), s, color);
                } else {
                    text_mid(self.d, self.cx, text, style(self.cx, TextKind::Body, false), r.x + self.px(sz::BUTTON_PAD), r, color);
                    let s = self.px(16.0);
                    self.icon(Icon::ChevronDown, r.right() - self.px(10.0) - s, (r.y + (r.h - s) / 2.0).round(), s, if off { t.done_text } else { t.weak_text });
                }
                self.hit(tree, i, true, false);
            }
            Kind::TabList => {
                // 見出しの列の下に、横いっぱいの線
                let t = &self.cx.theme;
                let border = t.border;
                self.d.fill(Rect::new(r.x, r.bottom() - line, r.w, line), border, 0.0);
                // 帯の中のボタン（ドッキングの閉じるボタン）は、ツールバーと同じく枠を描かない
                let was = self.toolbar;
                self.toolbar = true;
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
                self.toolbar = was;
            }
            Kind::Tab { text, selected } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(4.0);
                let body = Rect::new(r.x, r.y + self.px(2.0), r.w, r.h - self.px(6.0));
                let bg = if !off && self.cx.hover == Some(id) { Some(t.button_hover) } else { None };
                self.item_bg(body, bg, self.cx.focus_visible && self.cx.focus == Some(id), radius);
                let st = style(self.cx, TextKind::Body, *selected);
                let laid = self.cx.text.layout(text, st, None);
                let color = if off {
                    t.done_text
                } else if *selected {
                    t.text
                } else {
                    t.weak_text
                };
                put(self.d, &laid, (r.x + (r.w - laid.width) / 2.0).round(), (body.y + (body.h - laid.height) / 2.0).round(), color);
                if *selected {
                    // 選んでいるタブは、下に丸い端の短い線
                    let (w, h) = ((laid.width * 0.6).max(self.px(16.0)).round(), self.px(3.0));
                    self.d.fill(Rect::new((r.x + (r.w - w) / 2.0).round(), r.bottom() - h, w, h), if off { t.border } else { t.accent }, h / 2.0);
                }
                self.hit(tree, i, true, false);
            }
            Kind::NumberInput { text, step, .. } => {
                self.prev.steps.insert(id, *step);
                let reserve = self.px(2.0 * 28.0 + 3.0);
                self.text_input_with(tree, i, text, "", false, reserve);
                for &c in &tree.layout[i].children {
                    if matches!(tree.info[c].kind, Kind::SpinButton { .. }) {
                        self.prev.spins.insert(tree.info[c].id, id);
                    }
                    self.node(tree, c);
                }
            }
            Kind::SpinButton { up } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(4.0);
                if !off && self.cx.pressed == Some(id) {
                    self.d.fill(r, t.button_pressed, radius);
                } else if !off && self.cx.hover == Some(id) {
                    self.d.fill(r, t.button_hover, radius);
                }
                let s = self.px(14.0);
                let icon = if *up { Icon::ChevronUp } else { Icon::ChevronDown };
                self.icon(icon, (r.x + (r.w - s) / 2.0).round(), (r.y + (r.h - s) / 2.0).round(), s, if off { t.done_text } else { t.weak_text });
                self.hit(tree, i, false, false);
            }
            Kind::Split { stacked } => {
                for &c in &tree.layout[i].children {
                    if matches!(tree.info[c].kind, Kind::Splitter { .. }) {
                        let span = if *stacked { (r.y, r.h) } else { (r.x, r.w) };
                        self.prev.splits.insert(tree.info[c].id, span);
                    }
                    self.node(tree, c);
                }
            }
            Kind::Splitter { stacked, .. } => {
                self.prev.split_bars.insert(id, *stacked);
                let t = self.cx.theme.clone();
                self.focus_ring(tree, i, self.px(2.0));
                let off = tree.info[i].disabled;
                let hot = !off && (self.cx.pressed == Some(id) || self.cx.hover == Some(id));
                let w = if hot { self.px(3.0) } else { line };
                let bar = if *stacked {
                    Rect::new(r.x, (r.y + (r.h - w) / 2.0).round(), r.w, w)
                } else {
                    Rect::new((r.x + (r.w - w) / 2.0).round(), r.y, w, r.h)
                };
                self.d.fill(bar, if hot { t.accent } else { t.border }, 0.0);
                self.hit(tree, i, true, false);
            }
            Kind::DateField { text, empty, open } | Kind::TimeField { text, empty, open } => {
                let picture = if matches!(tree.info[i].kind, Kind::TimeField { .. }) { Icon::Clock } else { Icon::Calendar };
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let bg = if off {
                    t.button
                } else if self.cx.hover == Some(id) {
                    t.button_hover
                } else {
                    t.field
                };
                self.d.boxed(r, bg, if *open { t.accent } else { t.border }, line, radius);
                let color = if off { t.done_text } else if *empty { t.weak_text } else { t.text };
                let pad = self.px(sz::INPUT_PAD);
                text_mid(self.d, self.cx, text, style(self.cx, TextKind::Body, false), r.x + pad, r, color);
                let s = self.px(16.0);
                self.icon(picture, r.right() - pad - s, (r.y + (r.h - s) / 2.0).round(), s, if off { t.done_text } else { t.weak_text });
                self.hit(tree, i, true, false);
            }
            Kind::ColorField { color, text, open } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(sz::RADIUS);
                self.focus_ring(tree, i, radius);
                let bg = if off {
                    t.button
                } else if self.cx.hover == Some(id) {
                    t.button_hover
                } else {
                    t.field
                };
                self.d.boxed(r, bg, if *open { t.accent } else { t.border }, line, radius);
                let pad = self.px(sz::INPUT_PAD);
                let s = self.px(18.0);
                let sw = Rect::new(r.x + pad, (r.y + (r.h - s) / 2.0).round(), s, s);
                self.swatch_box(sw, *color, t.border, self.px(3.0));
                let st = style(self.cx, TextKind::Body, false);
                text_mid(self.d, self.cx, text, st, sw.right() + self.px(8.0), r, if off { t.done_text } else { t.text });
                let k = self.px(16.0);
                self.icon(Icon::ChevronDown, r.right() - pad - k, (r.y + (r.h - k) / 2.0).round(), k, if off { t.done_text } else { t.weak_text });
                self.hit(tree, i, true, false);
            }
            Kind::ColorArea { hue, sat, val } => {
                let key = format!("sv:{:.1}:{}x{}", hue, r.w as u32, r.h as u32);
                let (hue, w, h) = (*hue, r.w.max(1.0) as u32, r.h.max(1.0) as u32);
                let img = self.color_image(key, w, h, || crate::color::sv_pixels(hue, w, h));
                let radius = self.px(4.0);
                self.focus_ring(tree, i, radius);
                if let Some(img) = img {
                    self.d.image(r, &img);
                }
                let t = self.cx.theme.clone();
                self.outline(r, t.border, line);
                let (x, y) = (r.x + sat * (r.w - 1.0), r.y + (1.0 - val) * (r.h - 1.0));
                let [cr, cg, cb] = crate::color::hsv_to_rgb(hue, *sat, *val);
                self.color_knob(x, y, [cr, cg, cb, 1.0]);
                self.hit(tree, i, true, false);
            }
            Kind::HueBar { hue } => {
                let key = format!("hue:{}x{}", r.w as u32, r.h as u32);
                let (w, h) = (r.w.max(1.0) as u32, r.h.max(1.0) as u32);
                let img = self.color_image(key, w, h, || crate::color::hue_pixels(w, h));
                self.bar_with_knob(tree, i, r, img, hue / 360.0);
            }
            Kind::AlphaBar { rgb, alpha } => {
                let b = rgb.map(|c| (c * 255.0).round() as u32);
                let key = format!("alpha:{}:{}:{}:{}x{}", b[0], b[1], b[2], r.w as u32, r.h as u32);
                let (w, h, rgb, cell) = (r.w.max(1.0) as u32, r.h.max(1.0) as u32, *rgb, self.px(4.0) as u32);
                let img = self.color_image(key, w, h, || crate::color::alpha_pixels(rgb, w, h, cell));
                self.bar_with_knob(tree, i, r, img, *alpha);
            }
            Kind::Swatch { color, selected } => {
                let t = self.cx.theme.clone();
                let radius = self.px(4.0);
                let focused = self.cx.focus_visible && self.cx.focus == Some(id);
                if *selected || focused || self.cx.hover == Some(id) {
                    // 選んでいる色は主の色の輪で囲む（輪と色の間に背景の色のすき間）
                    let ring = if *selected || focused { t.accent } else { t.border };
                    let w = self.px(2.0);
                    self.d.boxed(Rect::new(r.x - w, r.y - w, r.w + 2.0 * w, r.h + 2.0 * w), t.background, ring, w, radius + w);
                }
                self.swatch_box(r.inset(if *selected { self.px(2.0) } else { 0.0 }), *color, t.border, radius);
                self.hit(tree, i, true, false);
            }
            Kind::ClockCell { text, selected } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(sz::RADIUS);
                let hot = !off && (self.cx.pressed == Some(id) || self.cx.hover == Some(id));
                let focused = self.cx.focus_visible && self.cx.focus == Some(id);
                let base = if *selected {
                    Some(t.accent)
                } else if hot {
                    Some(t.button_hover)
                } else {
                    None
                };
                self.item_bg(r, base, focused, radius);
                let color = if *selected { t.on_accent } else if off { t.done_text } else { t.text };
                let laid = self.cx.text.layout(text, style(self.cx, TextKind::Body, *selected), None);
                put(self.d, &laid, (r.x + (r.w - laid.width) / 2.0).round(), (r.y + (r.h - laid.height) / 2.0).round(), color);
                self.hit(tree, i, true, false);
            }
            Kind::CalendarDay { day, in_month, today, selected } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = (r.h.min(r.w) / 2.0).round();
                let hot = !off && (self.cx.pressed == Some(id) || self.cx.hover == Some(id));
                let focused = self.cx.focus_visible && self.cx.focus == Some(id);
                let base = if *selected {
                    Some(t.accent)
                } else if hot {
                    Some(t.button_hover)
                } else {
                    None
                };
                self.item_bg(r, base, focused, radius);
                if *today && !*selected {
                    let ring = r.inset(if focused { 3.0 * line } else { line });
                    self.d.boxed(ring, if hot { t.button_hover } else { t.background }, t.accent, line, radius);
                }
                let color = if *selected {
                    t.on_accent
                } else if off || !*in_month {
                    t.done_text
                } else if *today {
                    t.accent
                } else {
                    t.text
                };
                let laid = self.cx.text.layout(&day.to_string(), style(self.cx, TextKind::Body, *today || *selected), None);
                put(self.d, &laid, (r.x + (r.w - laid.width) / 2.0).round(), (r.y + (r.h - laid.height) / 2.0).round(), color);
                self.hit(tree, i, true, false);
            }
            Kind::TableHeader => {
                // 見出しの行の下に、横いっぱいの線
                let border = self.cx.theme.border;
                self.d.fill(Rect::new(r.x, r.bottom() - line, r.w, line), border, 0.0);
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
            }
            Kind::ColumnHeader { text, sort, right, sortable, focusable, .. } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(4.0);
                let bg = if *sortable && !off && (self.cx.pressed == Some(id) || self.cx.hover == Some(id)) { Some(t.button_hover) } else { None };
                self.item_bg(r, bg, self.cx.focus_visible && self.cx.focus == Some(id), radius);
                let st = style(self.cx, TextKind::Small, true);
                let laid = self.cx.text.layout(text, st, None);
                let s = self.px(12.0);
                let arrow = if sort.is_some() { s + self.px(4.0) } else { 0.0 };
                let pad = self.px(crate::ui::CELL_PAD);
                let x = if *right { (r.right() - pad - laid.width - arrow).round() } else { r.x + pad };
                let color = if off { t.done_text } else { t.weak_text };
                put(self.d, &laid, x, (r.y + (r.h - laid.height) / 2.0).round(), color);
                if let Some(asc) = sort {
                    let icon = if *asc { Icon::ArrowUp } else { Icon::ArrowDown };
                    self.icon(icon, (x + laid.width + self.px(4.0)).round(), (r.y + (r.h - s) / 2.0).round(), s, color);
                }
                self.hit(tree, i, *focusable, false);
            }
            Kind::ColumnGrip => {
                self.prev.split_bars.insert(id, false);
                let t = self.cx.theme.clone();
                let hot = self.cx.pressed == Some(id) || self.cx.hover == Some(id);
                let w = if hot { self.px(3.0) } else { line };
                let h = if hot { r.h } else { (r.h * 0.5).round() };
                let bar = Rect::new((r.x + (r.w - w) / 2.0).round(), (r.y + (r.h - h) / 2.0).round(), w, h);
                self.d.fill(bar, if hot { t.accent } else { t.border }, 0.0);
                self.hit(tree, i, false, false);
            }
            Kind::TreeItem { text, depth, open, selected } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let bg = if *selected {
                    Some(t.selected)
                } else if !off && self.cx.hover == Some(id) {
                    Some(t.button_hover)
                } else {
                    None
                };
                self.item_bg(r, bg, self.cx.focus_visible && self.cx.focus == Some(id), self.px(4.0));
                let x0 = r.x + self.px(8.0) + *depth as f32 * self.px(20.0);
                let color = if off { t.done_text } else { t.text };
                if let Some(o) = open {
                    let s = self.px(16.0);
                    let icon = if *o { Icon::ChevronDown } else { Icon::ChevronRight };
                    self.icon(icon, x0, (r.y + (r.h - s) / 2.0).round(), s, if off { t.done_text } else { t.weak_text });
                }
                text_mid(self.d, self.cx, text, style(self.cx, TextKind::Body, false), x0 + self.px(20.0), r, color);
                self.hit(tree, i, true, false);
            }
            Kind::Canvas { ops } => {
                self.prev.canvases.insert(id, r);
                self.canvas(r, ops);
                self.hit(tree, i, false, false);
            }
            // 合成が題の帯を描かない時に kargui が描く帯。焦点が無い窓は背景と同じ色にする（GNOME と同じ）。
            // ボタンはツールバーと同じく平らに描く。何も無い所は窓を動かす場所なので四角を覚える
            Kind::TitleBar { focused } => {
                let t = self.cx.theme.clone();
                self.d.fill(r, if *focused { t.button } else { t.background }, 0.0);
                self.d.fill(Rect::new(r.x, r.y + r.h - line, r.w, line), t.separator, 0.0);
                self.prev.title_bar = Some(r);
                let was = self.toolbar;
                self.toolbar = true;
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
                self.toolbar = was;
            }
            // Windows の形の題の帯のボタン。乗った時と押した時だけ塗り、閉じるは赤に白の印。最大化のボタンの四角は、
            // 窓の処理が Windows 11 のスナップレイアウトのために OS に答える（HTMAXBUTTON）
            Kind::CaptionButton { button, maximized } => {
                use super::context::CsdButton as B;
                let t = self.cx.theme.clone();
                let (hover, pressed) = (self.cx.hover == Some(id), self.cx.pressed == Some(id));
                let close = *button == B::Close;
                if hover || pressed {
                    let hc = self.cx.high_contrast;
                    let bg = match (close, pressed) {
                        (true, _) if hc => t.accent,
                        (true, false) => [0.769, 0.169, 0.110, 1.0],
                        (true, true) => [0.78, 0.26, 0.20, 1.0],
                        (false, false) => t.button_hover,
                        (false, true) => t.button_pressed,
                    };
                    self.d.fill(r, bg, 0.0);
                }
                let color = match (close && (hover || pressed), self.cx.high_contrast) {
                    (true, true) => t.on_accent,
                    (true, false) => [1.0, 1.0, 1.0, 1.0],
                    (false, _) => t.text,
                };
                let icon = match (button, maximized) {
                    (B::Minimize, _) => Icon::Minus,
                    (B::Maximize, true) => Icon::Restore,
                    (B::Maximize, false) => Icon::Maximize,
                    (B::Close, _) => Icon::Close,
                };
                let s = self.px(16.0);
                self.icon(icon, (r.x + (r.w - s) / 2.0).round(), (r.y + (r.h - s) / 2.0).round(), s, color);
                if *button == B::Maximize {
                    self.prev.caption_max = Some(r);
                }
                self.hit(tree, i, false, false);
            }
            Kind::Toolbar => {
                let was = self.toolbar;
                self.toolbar = true;
                for &c in &tree.layout[i].children {
                    self.node(tree, c);
                }
                self.toolbar = was;
            }
            Kind::MenuBarItem { text, open, .. } => {
                self.prev.menubar.push(id);
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(4.0);
                let bg = if !off && (*open || self.cx.pressed == Some(id)) {
                    Some(t.button_pressed)
                } else if !off && self.cx.hover == Some(id) {
                    Some(t.button_hover)
                } else {
                    None
                };
                self.item_bg(r, bg, self.cx.focus_visible && self.cx.focus == Some(id), radius);
                let laid = self.cx.text.layout(text, style(self.cx, TextKind::Body, false), None);
                let color = if off { t.done_text } else { t.text };
                put(self.d, &laid, (r.x + (r.w - laid.width) / 2.0).round(), (r.y + (r.h - laid.height) / 2.0).round(), color);
                self.hit(tree, i, true, false);
            }
            Kind::MenuItem { text, shortcut, icon, mark, submenu, open, gutter } => {
                let t = self.cx.theme.clone();
                let off = tree.info[i].disabled;
                let radius = self.px(4.0);
                let hot = !off && (self.cx.hover == Some(id) || (self.cx.focus_visible && self.cx.focus == Some(id)));
                let bg = if *open {
                    Some(t.selected)
                } else if hot {
                    Some(t.button_hover)
                } else {
                    None
                };
                self.item_bg(r, bg, self.cx.focus_visible && self.cx.focus == Some(id), radius);
                let color = if off { t.done_text } else { t.text };
                let weak = if off { t.done_text } else { t.weak_text };
                let pad = self.px(12.0);
                let s = self.px(16.0);
                let iy = (r.y + (r.h - s) / 2.0).round();
                let mut x = r.x + pad;
                if *gutter {
                    match (mark, icon) {
                        (MenuMark::Check(true), _) => self.icon(Icon::Check, x, iy, s, color),
                        (MenuMark::Radio(true), _) => {
                            let d = self.px(6.0);
                            self.d.fill(Rect::new(x + ((s - d) / 2.0).round(), (r.y + (r.h - d) / 2.0).round(), d, d), color, d / 2.0);
                        }
                        (_, Some(ic)) => self.icon(*ic, x, iy, s, color),
                        _ => {}
                    }
                    x += s + self.px(10.0);
                }
                let st = style(self.cx, TextKind::Body, false);
                text_mid(self.d, self.cx, text, st, x, r, color);
                let mut right = r.right() - pad;
                if *submenu {
                    self.icon(Icon::ChevronRight, right - s, iy, s, weak);
                    right -= s + self.px(8.0);
                }
                if let Some(k) = shortcut {
                    let laid = self.cx.text.layout(k, st, None);
                    put(self.d, &laid, (right - laid.width).round(), (r.y + (r.h - laid.height) / 2.0).round(), weak);
                }
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
            Kind::ToggleButton { on, text, icon } => {
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
                if !self.toolbar {
                    self.d.boxed(r, bg, border, line, radius);
                } else if !off && (*on || self.cx.pressed == Some(id) || self.cx.hover == Some(id)) {
                    self.d.fill(r, bg, radius);
                }
                if let (true, Some(ic)) = (text.is_empty(), icon) {
                    let s = self.px(button_icon(false));
                    self.icon(*ic, (r.x + (r.w - s) / 2.0).round(), (r.y + (r.h - s) / 2.0).round(), s, color);
                } else {
                    let laid = self.cx.text.layout(text, style(self.cx, TextKind::Body, *on), None);
                    put(self.d, &laid, (r.x + (r.w - laid.width) / 2.0).round(), (r.y + (r.h - laid.height) / 2.0).round(), color);
                }
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
            Kind::Toggle { text, pos, .. } => {
                let t = self.cx.theme.clone();
                let (tw, th) = (self.px(sz::TRACK_W), self.px(sz::TRACK_H));
                let track = Rect::new(r.x, (r.y + (r.h - th) / 2.0).round(), tw, th);
                if self.cx.focus_visible && self.cx.focus == Some(id) {
                    let w = self.px(2.0);
                    self.d.fill(track.inset(-2.0 * w), t.accent, th);
                    self.d.fill(track.inset(-w), t.background, th);
                }
                let off = tree.info[i].disabled;
                // 切り替えの途中（pos が 0 と 1 の間）は、つまみの位置と色を間の値にする
                let p = pos.clamp(0.0, 1.0);
                let (track_off, track_on) = if off { (t.separator, t.border) } else { (t.border, t.accent) };
                let track_color = mix(track_off, track_on, p);
                self.d.fill(track, track_color, th / 2.0);
                let k = th - self.px(6.0);
                let (x0, x1) = (track.x + self.px(3.0), track.right() - self.px(3.0) - k);
                let kx = (x0 + (x1 - x0) * p).round();
                self.d.fill(Rect::new(kx, track.y + self.px(3.0), k, k), mix(t.field, t.on_accent, p), k / 2.0);
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
            Kind::TextInput { value, hint, password, .. } => self.text_input(tree, i, value, hint, *password),
            Kind::TextArea { value, hint, font, .. } => self.text_area(tree, i, value, hint, *font),
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
            p.prev.hits.push(Hit { id: o.id, rect, slop: rect, focusable: false, text_input: false, arrows: false, vertical: false, enabled: false, drags: false, link: false })
        };
        let radius = self.px(if o.kind == OverlayKind::Tooltip { 4.0 } else { 8.0 });
        if o.kind == OverlayKind::Dialog {
            // kargui が描く題の帯は暗くせず押せるままにする（OS が描く題の帯と同じく、窓を動かす・閉じるに使える）
            let below = self.prev.title_bar.map_or(window, |b| Rect::new(0.0, b.bottom(), window.w, (window.h - b.bottom()).max(0.0)));
            self.d.fill(below, [0.0, 0.0, 0.0, if self.cx.dark { 0.55 } else { 0.35 }], 0.0);
            block(self, below);
        }
        // 影（少し下にずらした薄い黒）と、背景の箱
        if o.kind != OverlayKind::Tooltip {
            let sh = self.px(3.0);
            if !self.cx.high_contrast {
                self.d.fill(Rect::new(r.x - sh, r.y, r.w + 2.0 * sh, r.h + 2.0 * sh), [0.0, 0.0, 0.0, 0.10], radius + sh);
            }
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
        } else if tree.info[i].kind.interactive() && !tree.info[i].skip_tab && !matches!(tree.info[i].kind, Kind::Radio { tab_stop: false, .. } | Kind::MenuBarItem { tab_stop: false, .. }) {
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
        // 押せる範囲は、つまみの左に余白、右は部品の右の端からさらに余白（端のすぐ外を押しても棒に当たる）
        let slop = self.px(sz::SCROLLBAR_SLOP);
        let bar = Rect::new(x - slop, r.y, r.right() - x + 2.0 * slop, r.h);
        (Some(thumb), Some(bar))
    }

    /// つまみを描く。ポインターが棒の上にある時とつかんでいる間は濃くする（つかめることが見て分かるように）
    fn draw_thumb(&mut self, id: crate::ui::Id, thumb: Rect) {
        let (dragged, hovered) = (self.cx.drag.is_some_and(|d| d.list == id), self.cx.bar_hover == Some(id));
        let mut c = self.cx.theme.weak_text;
        c[3] = if dragged {
            0.85
        } else if hovered {
            0.65
        } else {
            0.45
        };
        // ハイコントラストでは透かさない（透かすと配色にない灰色になる）。乗った時と引っ張る時は強調の色
        if self.cx.high_contrast {
            c = if dragged || hovered { self.cx.theme.accent } else { self.cx.theme.text };
        }
        self.d.fill(Rect::new(thumb.x, thumb.y.round(), thumb.w, thumb.h.round()), c, thumb.w / 2.0);
    }

    /// スクロールの入れ物。見える範囲で切り取り、見えない子は描かない
    fn scroll_area(&mut self, tree: &Tree, i: usize) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let l = &tree.layout[i];
        // 中の一覧より先に入れる（ホイールと棒は、後に入れた内側の物から探す）
        let (thumb, bar) = self.scrollbar_geometry(r, l.content, l.offset);
        let k = self.prev.lists.len();
        self.prev.lists.push(ListInfo { id, rect: r, content_h: l.content, thumb, bar, order: 0 });
        self.d.push_scroll(IRect::covering(r), id.0, l.offset as i32);
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
        // 中身を描き終えた時の当たりの数（中に浮いたまとまりがあっても、外側の入れ物は隠れない）
        self.prev.lists[k].order = self.prev.hits.len();
        (self.clip, self.scroller) = outer;
        self.d.pop_clip();
        if let Some(t) = thumb {
            self.draw_thumb(id, t);
        }
    }

    fn text_input(&mut self, tree: &Tree, i: usize, value: &str, hint: &str, password: bool) {
        self.text_input_with(tree, i, value, hint, password, 0.0);
    }

    /// 1 行の入力欄。reserve は文字を並べない右の幅（数の入力欄の小さいボタンの分）
    fn text_input_with(&mut self, tree: &Tree, i: usize, value: &str, hint: &str, password: bool, reserve: f32) {
        let r = tree.layout[i].rect;
        let id = tree.info[i].id;
        let t = self.cx.theme.clone();
        let line = self.line();
        let focused = self.cx.focus == Some(id);
        let off = tree.info[i].disabled;
        let radius = self.px(sz::RADIUS);
        self.d.boxed(r, if off { t.button } else { t.field }, if focused { t.accent } else { t.border }, if focused { 2.0 * line } else { line }, radius);
        let pad = self.px(sz::INPUT_PAD);
        let inner = Rect::new(r.x + pad, r.y, (r.w - 2.0 * pad - reserve).max(0.0), r.h);
        // 書体は入力欄が決めた物（数の入力欄は窓の書体）
        let font = if let Kind::TextInput { font, .. } = &tree.info[i].kind { *font } else { None };
        let st = styled(self.cx, TextKind::Body, false, font);
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
        self.d.push_clip(IRect::covering(inner));
        let mut picked = None;
        if cursor != anchor && preedit.is_empty() {
            let (a, b) = (cursor.min(anchor), cursor.max(anchor));
            let xa = laid.x_of(edit::utf16_of(&shown, a));
            let xb = laid.x_of(edit::utf16_of(&shown, b));
            let sel = Rect::new(x0 + xa, ty, xb - xa, line_h);
            self.d.fill(sel, t.selection, 0.0);
            picked = Some(sel);
        }
        if shown.is_empty() {
            let h = self.cx.text.layout(hint, st, None);
            put(self.d, &h, x0, ty, if off || self.cx.high_contrast { t.done_text } else { t.weak_text });
        } else {
            put(self.d, &laid, x0, ty, if off { t.done_text } else { t.text });
            // ハイコントラストの選んだ範囲は Highlight の上なので、文字を HighlightText で描き直す
            if let Some(sel) = picked.filter(|_| self.cx.high_contrast) {
                self.d.push_clip(IRect::covering(sel));
                put(self.d, &laid, x0, ty, t.on_accent);
                self.d.pop_clip();
            }
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
        if password {
            self.prev.passwords.insert(id);
        }
        self.prev.inputs.insert(id, (r, x0));
        self.hit(tree, i, true, true);
    }

    /// 複数行の入力欄。折り返して並べ、縦にスクロールする（スクロールの棒とホイールは一覧と同じ仕組み）。
    /// 選んだ範囲と変換中の文字の下線は、行ごとの四角に分けて描く
    fn text_area(&mut self, tree: &Tree, i: usize, value: &str, hint: &str, font: Option<FontId>) {
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
        let st = styled(self.cx, TextKind::Body, false, font);
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
        self.d.push_clip(IRect::covering(view));
        // 選んだ範囲
        let mut picked = Vec::new();
        if cursor != anchor && preedit.is_empty() {
            let (a, b) = (edit::utf16_of(&shown, cursor.min(anchor)), edit::utf16_of(&shown, cursor.max(anchor)));
            for rr in range_rects(&laid, a, b, self.px(4.0)) {
                let sel = Rect::new(ox + rr.x, oy + rr.y, rr.w, rr.h);
                self.d.fill(sel, t.selection, 0.0);
                picked.push(sel);
            }
        }
        if shown.is_empty() {
            let h = self.cx.text.layout(hint, st, Some(inner.w));
            put(self.d, &h, ox, oy, if off || self.cx.high_contrast { t.done_text } else { t.weak_text });
        } else {
            put(self.d, &laid, ox, oy, if off { t.done_text } else { t.text });
            // ハイコントラストの選んだ範囲は Highlight の上なので、行ごとに文字を HighlightText で描き直す
            if self.cx.high_contrast {
                for sel in picked {
                    self.d.push_clip(IRect::covering(sel));
                    put(self.d, &laid, ox, oy, t.on_accent);
                    self.d.pop_clip();
                }
            }
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
        self.prev.lists.push(ListInfo { id, rect: view, content_h, thumb, bar, order: self.prev.hits.len() });
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
        let k = self.prev.lists.len();
        self.prev.lists.push(ListInfo { id, rect: r, content_h, thumb, bar, order: 0 });
        self.d.push_scroll(IRect::covering(r), id.0, l.offset as i32);
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
        // 行を描き終えた時の当たりの数（浮いたまとまりに隠れているかは、描き終えた後に重ねた物で決める）
        self.prev.lists[k].order = self.prev.hits.len();
        self.clip = outer;
        self.d.pop_clip();
        // スクロールの位置を示す細い棒（中身が一覧より高いときだけ）
        if let Some(t) = thumb {
            self.draw_thumb(id, t);
        }
    }
}

/// ドッキングの入れ物 i の、全体とまとまりの場所（今のフレームの配置から。まとまりは並べ方の木の左・上が先で、その後に
/// 浮いたまとまりを下にある物から）。in_window（別の窓の中）なら、窓いっぱいのまとまりも浮いたまとまりとして扱う
/// （分けない。タブとして入れるだけ）
pub(crate) fn dock_geom(tree: &Tree, i: usize, in_window: bool) -> DockGeom {
    fn walk(tree: &Tree, i: usize, floating: bool, out: &mut Vec<GroupGeom>) {
        for &c in &tree.layout[i].children {
            match tree.info[c].kind {
                Kind::DockGroup => {
                    // まとまりの最初の子が見出しの帯（TabList）、その中の Tab が見出し。浮いたまとまりの帯は、動かす印と
                    // TabList の行。浮いたまとまりの場所は枠の全体
                    let head = tree.layout[c].children.first().copied();
                    let is_list = |k: usize| matches!(tree.info[k].kind, Kind::TabList);
                    let list = head.and_then(|h| if is_list(h) { Some(h) } else { tree.layout[h].children.iter().copied().find(|&k| is_list(k)) });
                    let heads: Vec<usize> = list.map_or(Vec::new(), |s| {
                        tree.layout[s].children.iter().copied().filter(|&t| matches!(tree.info[t].kind, Kind::Tab { .. })).collect()
                    });
                    let tabs = heads.iter().map(|&t| tree.layout[t].rect).collect();
                    let alone = match heads[..] {
                        [t] => Some(tree.info[t].id),
                        _ => None,
                    };
                    let rect = if floating { tree.layout[i].rect } else { tree.layout[c].rect };
                    out.push(GroupGeom { rect, strip: head.map_or(Rect::default(), |s| tree.layout[s].rect), tabs, floating, alone });
                }
                Kind::DockFloat => walk(tree, c, true, out),
                _ => walk(tree, c, floating, out),
            }
        }
    }
    let mut g = DockGeom { root: tree.layout[i].rect, groups: Vec::new() };
    walk(tree, i, in_window, &mut g.groups);
    g
}

/// 一覧が見える範囲を決めるのに使った高さと、決まった高さがずれていたら true（もう 1 フレーム描く）
pub(crate) fn list_needs_refresh(tree: &Tree) -> bool {
    tree.info.iter().zip(&tree.layout).any(|(n, l)| matches!(n.kind, Kind::List { view_h, .. } if (view_h - l.rect.h).abs() > 0.5))
}

#[allow(dead_code)]
fn _unused(_: Dir, _: Size) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Text;

    /// 字を put した時の四角（put と同じ丸め方）を、全部の字から総当たりで出す
    fn every_glyph_box(laid: &Laid, x: f32, y: f32) -> Vec<[f32; 4]> {
        laid.glyphs
            .iter()
            .map(|g| [(x + g.x).round() + g.slot.left as f32, (y + g.y).round() + g.slot.top as f32, g.slot.w as f32, g.slot.h as f32])
            .collect()
    }

    fn put_into(laid: &Laid, x: f32, y: f32, clip: IRect) -> Vec<[f32; 4]> {
        let mut d = DrawList::default();
        d.begin(IRect::new(-100_000, -100_000, 300_000, 300_000));
        d.push_clip(clip);
        put(&mut d, laid, x, y, [0.0, 0.0, 0.0, 1.0]);
        d.pop_clip();
        d.finish();
        d.quads.iter().map(|q| q.rect).collect()
    }

    /// 描く物に入れるのは、切り抜きと重なる字だけ。重なる字は 1 つも落とさない（端で一部だけ見える字も入れる）。
    /// 家計簿のメモに 200 万字を貼ると、見えるのは 60 字ほどなのに、全部の字を毎フレーム描く物に入れていた
    /// （quads=2000194、何も変わらないフレームでも 160 ms。2026-11-28 の X11 の記録）
    #[test]
    fn put_keeps_exactly_the_glyphs_that_touch_the_clip() {
        let mut t = Text::new().unwrap();
        let st = TextStyle { size: 14.0, bold: false, font: FontId(0) };
        let one_line = "a".repeat(3000) + "設定😀" + &"b".repeat(3000);
        let many_lines: String = (0..400).map(|i| format!("行{i} abcdefghij 設定\n")).collect();
        for (text, width, x, y) in [(one_line.as_str(), None, -9876.4, 30.0), (many_lines.as_str(), Some(160.0), 12.3, -2345.6)] {
            let laid = t.layout(text, st, width);
            for clip in [IRect::new(40, 20, 120, 90), IRect::new(0, 0, 900, 640), IRect::new(500, 600, 3, 3)] {
                let want: Vec<[f32; 4]> = every_glyph_box(&laid, x, y)
                    .into_iter()
                    .filter(|r| IRect::covering(Rect::new(r[0], r[1], r[2], r[3])).intersect(&clip).is_some())
                    .collect();
                let got = put_into(&laid, x, y, clip);
                assert_eq!(got, want, "{clip:?}");
                assert!(want.len() < laid.glyphs.len() / 10, "見えるのは一部: {} / {}", want.len(), laid.glyphs.len());
            }
            // 切り抜きが全部を含めば、全部の字を入れる
            assert_eq!(put_into(&laid, 0.0, 0.0, IRect::new(-100_000, -100_000, 300_000, 300_000)), every_glyph_box(&laid, 0.0, 0.0));
        }
    }
}
