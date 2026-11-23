//! 配置の計算（画面を持たない純粋な処理）。
//!
//! 2 回で決める: 下から大きさを測り（measure）、上から場所を決める（arrange）。
//! 1 回で決める即時モード（egui）と違い、大きさの分からない物の中央寄せや、残りの幅を分けることが 1 フレームでできる。
//!
//! - Row: 子を横に並べる。縦は align（既定は真ん中）
//! - Column: 子を縦に並べる。横は align（既定は幅いっぱい）
//! - Stack: 子を重ねる
//! - List: 見えている行だけを、行の番号とスクロールの位置から置く
//! - Scroll: 子を縦に並べ、スクロールの位置だけ上にずらす。中身の高さ（content）を覚える。
//!   高さが Fill のときは、中身が高くても残りの高さだけを取る（Fill の基準を 0 にする）
//! - Grid: 子を左から右、上から下へ、列の数ごとに折り返して並べる。列の幅は columns（Auto はその列でいちばん広い子、
//!   Fill は残りを分ける）、行の高さはその行でいちばん高い子。子は升の中で横は align、縦は真ん中
//! - Fill の子は、残りの長さを重みで分け合う

use crate::geom::Rect;

/// How long a widget is along one axis.
///
/// ```
/// # use kargui::Size;
/// # let mut name = String::new();
/// # kargui::test::example(|ui| {
/// ui.row(|ui| {
///     ui.label("Name").width(Size::Fixed(80.0));
///     ui.text_input(&mut name).width(Size::Fill(1.0));
///     ui.button("OK").width(Size::Auto);
/// });
/// # });
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// As long as its content.
    Auto,
    /// This many logical pixels.
    Fixed(f32),
    /// Takes a share of the remaining space, by weight (`Fill(1.0)` is the usual choice).
    Fill(f32),
}

/// Where children go on the cross axis.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Top (in a row) or left (in a column).
    Start,
    /// Middle.
    Center,
    /// Bottom (in a row) or right (in a column).
    End,
    /// As long as the container.
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dir {
    Leaf,
    Row,
    Column,
    Stack,
    List,
    Scroll,
    Grid,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutNode {
    pub dir: Dir,
    /// 物理ピクセル（Fixed の値も物理ピクセルに直して入れる）
    pub width: Size,
    pub height: Size,
    /// 左右と上下の余白
    pub pad: (f32, f32),
    pub gap: f32,
    pub align: Align,
    /// 中身の大きさ（Leaf のとき）
    pub intrinsic: (f32, f32),
    pub children: Vec<usize>,
    /// List の行の高さとスクロールの位置（物理ピクセル）
    pub row_h: f32,
    pub offset: f32,
    /// List の子のとき、何行目か
    pub row_index: usize,
    /// Scroll の中身の高さ（余白を含む。物理ピクセル）
    pub content: f32,
    /// Scroll の右に空けておく幅（スクロールの棒の分。中身があふれていなくても空け、幅が変わって折り返しが揺れないようにする）
    pub gutter: f32,
    /// Grid の列の幅（Fixed は物理ピクセル）
    pub columns: Vec<Size>,
    /// 上から何割の高さだけを見せるか（Handle::reveal）。親の中ではその高さだけを取り、子は元の高さで並べる（はみ出した所は
    /// 描くときに切り取る）
    pub reveal: Option<f32>,
    /// reveal の時の、元の高さ（物理ピクセル）
    pub full_h: f32,
    /// 中身を自分の範囲で切り取る（CSS の overflow: hidden）。Fill の向きでは中身より狭くできる（min-width: 0）。表の升
    pub clip: bool,
    // 結果
    pub measured: (f32, f32),
    pub rect: Rect,
}

impl LayoutNode {
    pub fn new(dir: Dir) -> Self {
        Self {
            dir,
            width: Size::Auto,
            height: Size::Auto,
            pad: (0.0, 0.0),
            gap: 0.0,
            align: if dir == Dir::Row { Align::Center } else { Align::Stretch },
            intrinsic: (0.0, 0.0),
            children: Vec::new(),
            row_h: 0.0,
            offset: 0.0,
            row_index: 0,
            content: 0.0,
            gutter: 0.0,
            columns: Vec::new(),
            reveal: None,
            full_h: 0.0,
            clip: false,
            measured: (0.0, 0.0),
            rect: Rect::default(),
        }
    }
}

fn fixed_or(size: Size, measured: f32) -> f32 {
    match size {
        Size::Fixed(v) => v,
        _ => measured,
    }
}

pub(crate) fn measure(nodes: &mut [LayoutNode], i: usize) -> (f32, f32) {
    let children = nodes[i].children.clone();
    let sizes: Vec<(f32, f32)> = children.iter().map(|&c| measure(nodes, c)).collect();
    let n = &nodes[i];
    let gaps = n.gap * sizes.len().saturating_sub(1) as f32;
    let (w, h) = match n.dir {
        Dir::Leaf => n.intrinsic,
        Dir::Row => (sizes.iter().map(|s| s.0).sum::<f32>() + gaps, sizes.iter().map(|s| s.1).fold(0.0, f32::max)),
        Dir::Column => (sizes.iter().map(|s| s.0).fold(0.0, f32::max), sizes.iter().map(|s| s.1).sum::<f32>() + gaps),
        Dir::Stack => (sizes.iter().map(|s| s.0).fold(0.0, f32::max), sizes.iter().map(|s| s.1).fold(0.0, f32::max)),
        Dir::List => (0.0, 0.0),
        Dir::Scroll => (sizes.iter().map(|s| s.0).fold(0.0, f32::max) + n.gutter, sizes.iter().map(|s| s.1).sum::<f32>() + gaps),
        Dir::Grid => {
            let (cw, rh) = grid_tracks(&n.columns, &sizes);
            let gx = n.gap * cw.len().saturating_sub(1) as f32;
            let gy = n.gap * rh.len().saturating_sub(1) as f32;
            let cw: Vec<f32> = cw.iter().zip(grid_columns(&n.columns, cw.len())).map(|(&m, c)| fixed_or(c, m)).collect();
            (cw.iter().sum::<f32>() + gx, rh.iter().sum::<f32>() + gy)
        }
    };
    let (w, h) = if n.dir == Dir::Leaf { (w, h) } else { (w + 2.0 * n.pad.0, h + 2.0 * n.pad.1) };
    let (dir, width, height) = (n.dir, n.width, n.height);
    let m = if dir == Dir::Scroll {
        nodes[i].content = h;
        let own_h = match height {
            Size::Fixed(v) => v,
            Size::Fill(_) => 0.0,
            Size::Auto => h,
        };
        (fixed_or(width, w), own_h)
    } else {
        (fixed_or(width, w), fixed_or(height, h))
    };
    // 切り取る物の Fill の向きは、中身の長さを親に伝えない（親の取り分より広がらない。はみ出す所は描く時に切り取る）
    let m = if nodes[i].clip {
        (if matches!(width, Size::Fill(_)) { 0.0 } else { m.0 }, if matches!(height, Size::Fill(_)) { 0.0 } else { m.1 })
    } else {
        m
    };
    // 一部だけ見せる物は、見せる高さだけを親に伝え、元の高さは並べる時のために覚える
    let m = match nodes[i].reveal {
        Some(t) => {
            nodes[i].full_h = m.1;
            (m.0, (m.1 * t.clamp(0.0, 1.0)).round())
        }
        None => m,
    };
    nodes[i].measured = m;
    m
}

/// 格子の列の決まり（足りない分は Auto）
fn grid_columns(columns: &[Size], n: usize) -> Vec<Size> {
    (0..n).map(|k| columns.get(k).copied().unwrap_or(Size::Auto)).collect()
}

/// 格子の、列ごとのいちばん広い子の幅と、行ごとのいちばん高い子の高さ
fn grid_tracks(columns: &[Size], sizes: &[(f32, f32)]) -> (Vec<f32>, Vec<f32>) {
    let cols = columns.len().max(1);
    let rows = sizes.len().div_ceil(cols);
    let (mut cw, mut rh) = (vec![0.0f32; cols], vec![0.0f32; rows]);
    for (k, s) in sizes.iter().enumerate() {
        cw[k % cols] = cw[k % cols].max(s.0);
        rh[k / cols] = rh[k / cols].max(s.1);
    }
    (cw, rh)
}

/// 主軸の長さを子に分ける。Fixed と Auto はそのまま。Fill は残りを重みで分ける（WinUI の * と CSS の flex: 1 と同じく、
/// 同じ重みなら中身に関わらず同じ長さ）が、中身より短くはしない: 取り分が中身より短い子は中身の長さで止め、残りをほかの
/// Fill で分け直す。全部が入りきらない時は、どれも中身の長さ（はみ出す）
fn distribute(sizes: &[Size], measured: &[f32], avail: f32) -> Vec<f32> {
    let weight = |s: &Size| if let Size::Fill(w) = s { w.max(0.0) } else { 0.0 };
    let mut out: Vec<f32> = sizes.iter().zip(measured).map(|(s, &m)| if weight(s) > 0.0 { 0.0 } else { fixed_or(*s, m) }).collect();
    // 長さが決まった子（Fixed・Auto・重み 0 の Fill と、中身で止めた Fill）
    let mut done: Vec<bool> = sizes.iter().map(|s| weight(s) <= 0.0).collect();
    loop {
        let used: f32 = out.iter().zip(&done).filter(|&(_, &d)| d).map(|(&l, _)| l).sum();
        let weights: f32 = sizes.iter().zip(&done).filter(|&(_, &d)| !d).map(|(s, _)| weight(s)).sum();
        if weights <= 0.0 {
            break;
        }
        let free = (avail - used).max(0.0);
        let mut stopped = false;
        for k in 0..sizes.len() {
            if done[k] {
                continue;
            }
            let share = free * weight(&sizes[k]) / weights;
            if share < measured[k] {
                out[k] = measured[k];
                done[k] = true;
                stopped = true;
            } else {
                out[k] = share;
            }
        }
        if !stopped {
            break;
        }
    }
    out
}

fn cross(size: Size, measured: f32, avail: f32, align: Align) -> (f32, f32) {
    let len = match size {
        Size::Fixed(v) => v,
        Size::Fill(_) => avail,
        Size::Auto if align == Align::Stretch => avail,
        Size::Auto => measured,
    };
    let at = match align {
        Align::Start | Align::Stretch => 0.0,
        Align::Center => ((avail - len) * 0.5).round(),
        Align::End => avail - len,
    };
    (at, len)
}

pub(crate) fn arrange(nodes: &mut [LayoutNode], i: usize, rect: Rect) {
    nodes[i].rect = rect;
    let n = nodes[i].clone();
    // 一部だけ見せる物の子は、元の高さの中に並べる（見せない所は描く時に切り取る）
    let rect = if n.reveal.is_some() { Rect::new(rect.x, rect.y, rect.w, rect.h.max(n.full_h)) } else { rect };
    let inner = Rect::new(rect.x + n.pad.0, rect.y + n.pad.1, (rect.w - 2.0 * n.pad.0).max(0.0), (rect.h - 2.0 * n.pad.1).max(0.0));
    let gaps = n.gap * n.children.len().saturating_sub(1) as f32;
    match n.dir {
        Dir::Leaf => {}
        Dir::Row => {
            let sizes: Vec<Size> = n.children.iter().map(|&c| nodes[c].width).collect();
            let measured: Vec<f32> = n.children.iter().map(|&c| nodes[c].measured.0).collect();
            let lens = distribute(&sizes, &measured, inner.w - gaps);
            let mut x = inner.x;
            for (k, &c) in n.children.iter().enumerate() {
                let (at, h) = cross(nodes[c].height, nodes[c].measured.1, inner.h, n.align);
                arrange(nodes, c, Rect::new(x.round(), inner.y + at, lens[k].round(), h));
                x += lens[k] + n.gap;
            }
        }
        Dir::Column | Dir::Scroll => {
            let sizes: Vec<Size> = n.children.iter().map(|&c| nodes[c].height).collect();
            let measured: Vec<f32> = n.children.iter().map(|&c| nodes[c].measured.1).collect();
            // スクロールの中は、中身の高さと見える高さの大きいほうを分ける
            let avail = if n.dir == Dir::Scroll { inner.h.max(n.content - 2.0 * n.pad.1) } else { inner.h };
            let lens = distribute(&sizes, &measured, avail - gaps);
            let mut y = if n.dir == Dir::Scroll { inner.y - n.offset } else { inner.y };
            let cross_w = if n.dir == Dir::Scroll { (inner.w - n.gutter).max(0.0) } else { inner.w };
            for (k, &c) in n.children.iter().enumerate() {
                let (at, w) = cross(nodes[c].width, nodes[c].measured.0, cross_w, n.align);
                arrange(nodes, c, Rect::new(inner.x + at, y.round(), w, lens[k].round()));
                y += lens[k] + n.gap;
            }
        }
        Dir::Stack => {
            for &c in &n.children {
                let (ax, w) = cross(nodes[c].width, nodes[c].measured.0, inner.w, n.align);
                let (ay, h) = cross(nodes[c].height, nodes[c].measured.1, inner.h, n.align);
                arrange(nodes, c, Rect::new(inner.x + ax, inner.y + ay, w, h));
            }
        }
        Dir::List => {
            for &c in &n.children {
                let y = inner.y + nodes[c].row_index as f32 * n.row_h - n.offset;
                arrange(nodes, c, Rect::new(inner.x, y.round(), inner.w, n.row_h));
            }
        }
        Dir::Grid => {
            let sizes: Vec<(f32, f32)> = n.children.iter().map(|&c| nodes[c].measured).collect();
            let (cm, rh) = grid_tracks(&n.columns, &sizes);
            let cols = cm.len();
            let gx = n.gap * cols.saturating_sub(1) as f32;
            let cw = distribute(&grid_columns(&n.columns, cols), &cm, inner.w - gx);
            let mut y = inner.y;
            for (r, &row_h) in rh.iter().enumerate() {
                let mut x = inner.x;
                for (k, &col_w) in cw.iter().enumerate() {
                    let Some(&c) = n.children.get(r * cols + k) else { break };
                    let (ax, w) = cross(nodes[c].width, nodes[c].measured.0, col_w, n.align);
                    let (ay, h) = cross(nodes[c].height, nodes[c].measured.1, row_h, Align::Center);
                    arrange(nodes, c, Rect::new((x + ax).round(), (y + ay).round(), w.round(), h));
                    x += col_w + n.gap;
                }
                y += row_h + n.gap;
            }
        }
    }
}

/// root を rect に置き、すべての子の場所を決める
pub(crate) fn layout(nodes: &mut [LayoutNode], root: usize, rect: Rect) {
    measure(nodes, root);
    arrange(nodes, root, rect);
}

/// 出す元（anchor）の近くに w×h の重ねる物を置く場所。下に入れば下、入らなければ上、どちらも入らなければ
/// 広い方に出して窓の中に寄せる。横は出す元の左端にそろえ、窓からはみ出すなら内側へ寄せる。
/// margin は窓の端との間、gap は出す元との間（物理ピクセル）
pub(crate) fn place_near(anchor: Rect, w: f32, h: f32, window: Rect, margin: f32, gap: f32) -> Rect {
    let (top, bottom) = (window.y + margin, window.bottom() - margin);
    let below = anchor.bottom() + gap;
    let above = anchor.y - gap - h;
    let y = if below + h <= bottom {
        below
    } else if above >= top {
        above
    } else if bottom - anchor.bottom() >= anchor.y - top {
        below
    } else {
        above
    };
    let y = y.min(bottom - h).max(top);
    let x = anchor.x.min(window.right() - margin - w).max(window.x + margin);
    Rect::new(x.round(), y.round(), w, h)
}

/// 出す元（anchor）の横に w×h を置く場所（入れ子のメニュー）。右に入れば右、入らなければ左。
/// 縦は出す元の上端から pad だけ上（中の最初の項目が出す元と並ぶ）にして、窓の中に収める
pub(crate) fn place_beside(anchor: Rect, w: f32, h: f32, window: Rect, margin: f32, pad: f32) -> Rect {
    let right = anchor.right() + pad;
    let x = if right + w <= window.right() - margin { right } else { anchor.x - pad - w };
    let x = x.min(window.right() - margin - w).max(window.x + margin);
    let y = (anchor.y - pad).min(window.bottom() - margin - h).max(window.y + margin);
    Rect::new(x.round(), y.round(), w, h)
}

/// 窓の真ん中に w×h を置く場所
pub(crate) fn place_center(w: f32, h: f32, window: Rect) -> Rect {
    Rect::new((window.x + (window.w - w) / 2.0).max(window.x).round(), (window.y + (window.h - h) / 2.0).max(window.y).round(), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_size_columns_by_their_widest_cell_and_share_the_rest() {
        let mut nodes = vec![LayoutNode::new(Dir::Grid)];
        nodes[0].columns = vec![Size::Auto, Size::Fill(1.0)];
        nodes[0].gap = 10.0;
        for (w, h) in [(40.0, 20.0), (100.0, 30.0), (80.0, 20.0), (100.0, 30.0), (30.0, 20.0)] {
            let mut leaf = LayoutNode::new(Dir::Leaf);
            leaf.intrinsic = (w, h);
            nodes.push(leaf);
            let k = nodes.len() - 1;
            nodes[0].children.push(k);
        }
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 400.0, 300.0));
        // 1 列目は 80（いちばん広い）、2 列目は残りの 310
        assert_eq!((nodes[1].rect.x, nodes[1].rect.w), (0.0, 80.0));
        assert_eq!((nodes[2].rect.x, nodes[2].rect.w), (90.0, 310.0));
        // 行の高さは 30。低い子は縦の真ん中
        assert_eq!(nodes[1].rect.y, 5.0);
        assert_eq!(nodes[3].rect.y, 40.0 + 5.0);
        // 3 行目は 1 つだけ
        assert_eq!((nodes[5].rect.x, nodes[5].rect.y), (0.0, 80.0));
        assert_eq!(nodes[0].measured, (80.0 + 10.0 + 100.0, 30.0 + 10.0 + 30.0 + 10.0 + 20.0));
    }

    #[test]
    fn placing_beside_prefers_the_right_then_the_left_and_stays_inside() {
        let window = Rect::new(0.0, 0.0, 400.0, 300.0);
        let r = place_beside(Rect::new(20.0, 40.0, 100.0, 30.0), 120.0, 100.0, window, 8.0, 4.0);
        assert_eq!((r.x, r.y), (124.0, 36.0));
        // 右に入らないので左
        let r = place_beside(Rect::new(250.0, 40.0, 100.0, 30.0), 120.0, 100.0, window, 8.0, 4.0);
        assert_eq!(r.x, 250.0 - 4.0 - 120.0);
        // 下にはみ出すなら上へ寄せる
        let r = place_beside(Rect::new(20.0, 250.0, 100.0, 30.0), 120.0, 100.0, window, 8.0, 4.0);
        assert_eq!(r.y, 300.0 - 8.0 - 100.0);
    }

    #[test]
    fn placing_near_an_anchor_prefers_below_then_above_and_stays_inside() {
        let window = Rect::new(0.0, 0.0, 400.0, 300.0);
        // 下に入る
        let r = place_near(Rect::new(20.0, 40.0, 80.0, 30.0), 120.0, 100.0, window, 8.0, 4.0);
        assert_eq!((r.x, r.y), (20.0, 74.0));
        // 下に入らないので上
        let r = place_near(Rect::new(20.0, 220.0, 80.0, 30.0), 120.0, 100.0, window, 8.0, 4.0);
        assert_eq!((r.x, r.y), (20.0, 116.0));
        // 右の端からはみ出すので内側へ
        let r = place_near(Rect::new(350.0, 40.0, 40.0, 30.0), 120.0, 100.0, window, 8.0, 4.0);
        assert_eq!(r.x, 400.0 - 8.0 - 120.0);
        // どちらにも入らない: 広い方（上）に出して、窓の中に収める
        let r = place_near(Rect::new(20.0, 200.0, 80.0, 30.0), 120.0, 250.0, window, 8.0, 4.0);
        assert_eq!(r.y, 8.0);
        // 窓より大きい物は上の端から
        let r = place_near(Rect::new(20.0, 20.0, 80.0, 30.0), 120.0, 400.0, window, 8.0, 4.0);
        assert_eq!(r.y, 8.0);
    }

    #[test]
    fn placing_in_the_center() {
        let r = place_center(100.0, 50.0, Rect::new(0.0, 0.0, 400.0, 300.0));
        assert_eq!((r.x, r.y), (150.0, 125.0));
        let r = place_center(500.0, 50.0, Rect::new(0.0, 0.0, 400.0, 300.0));
        assert_eq!(r.x, 0.0);
    }

    #[test]
    fn reveal_takes_part_of_the_height_and_keeps_the_children_at_full_size() {
        // 縦の並び [上から 4 分の 1 だけ見せる列（中に高さ 40 と 60 の子）][下の部品 20]
        let mut nodes = vec![LayoutNode::new(Dir::Column), LayoutNode::new(Dir::Column), leaf(50.0, 40.0), leaf(50.0, 60.0), leaf(50.0, 20.0)];
        nodes[0].children = vec![1, 4];
        nodes[1].children = vec![2, 3];
        nodes[1].reveal = Some(0.25);
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 200.0, 300.0));
        assert_eq!(nodes[1].rect.h, 25.0, "100 の 4 分の 1");
        assert_eq!(nodes[4].rect.y, 25.0, "下の部品は、見せる高さのすぐ下");
        assert_eq!((nodes[2].rect.h, nodes[3].rect.y, nodes[3].rect.h), (40.0, 40.0, 60.0), "中は元の大きさで並ぶ");
    }

    fn leaf(w: f32, h: f32) -> LayoutNode {
        let mut n = LayoutNode::new(Dir::Leaf);
        n.intrinsic = (w, h);
        n
    }

    #[test]
    fn row_gives_the_rest_to_fill_and_centers_vertically() {
        // [入力欄（Fill）][追加（72×32）]、間 8、余白 16
        let mut nodes = vec![LayoutNode::new(Dir::Row), leaf(0.0, 34.0), leaf(72.0, 32.0)];
        nodes[0].children = vec![1, 2];
        nodes[0].gap = 8.0;
        nodes[0].pad = (16.0, 0.0);
        nodes[1].width = Size::Fill(1.0);
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 900.0, 40.0));
        assert_eq!(nodes[1].rect, Rect::new(16.0, 3.0, 788.0, 34.0));
        assert_eq!(nodes[2].rect, Rect::new(812.0, 4.0, 72.0, 32.0));
    }

    #[test]
    fn column_stretches_width_and_fills_height() {
        let mut nodes = vec![LayoutNode::new(Dir::Column), leaf(100.0, 30.0), leaf(50.0, 0.0), leaf(10.0, 20.0)];
        nodes[0].children = vec![1, 2, 3];
        nodes[0].gap = 10.0;
        nodes[2].height = Size::Fill(1.0);
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 300.0, 200.0));
        assert_eq!(nodes[1].rect, Rect::new(0.0, 0.0, 300.0, 30.0));
        assert_eq!(nodes[2].rect, Rect::new(0.0, 40.0, 300.0, 130.0));
        assert_eq!(nodes[3].rect, Rect::new(0.0, 180.0, 300.0, 20.0));
    }

    #[test]
    fn fills_share_by_weight_and_fixed_overrides_content() {
        let mut nodes = vec![LayoutNode::new(Dir::Row), leaf(0.0, 10.0), leaf(0.0, 10.0), leaf(999.0, 10.0)];
        nodes[0].children = vec![1, 2, 3];
        nodes[1].width = Size::Fill(1.0);
        nodes[2].width = Size::Fill(3.0);
        nodes[3].width = Size::Fixed(100.0);
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 500.0, 10.0));
        assert_eq!(nodes[1].rect.w, 100.0);
        assert_eq!(nodes[2].rect.w, 300.0);
        assert_eq!(nodes[3].rect, Rect::new(400.0, 0.0, 100.0, 10.0));
    }

    #[test]
    fn equal_fills_get_equal_lengths_whatever_their_content() {
        // 中身の幅が違う 3 つの Fill(1.0)（カードの板の列: 1 列目だけ入力欄で中身が広い）。WinUI の * と CSS の flex: 1 と同じく、
        // 同じ重みなら同じ幅にする
        let mut nodes = vec![LayoutNode::new(Dir::Row), leaf(250.0, 10.0), leaf(80.0, 10.0), leaf(80.0, 10.0)];
        nodes[0].children = vec![1, 2, 3];
        for node in &mut nodes[1..=3] {
            node.width = Size::Fill(1.0);
        }
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 900.0, 10.0));
        assert_eq!([nodes[1].rect.w, nodes[2].rect.w, nodes[3].rect.w], [300.0, 300.0, 300.0]);
    }

    #[test]
    fn a_fill_never_gets_shorter_than_its_content_and_the_others_share_the_rest() {
        let mut nodes = vec![LayoutNode::new(Dir::Row), leaf(500.0, 10.0), leaf(80.0, 10.0), leaf(0.0, 10.0)];
        nodes[0].children = vec![1, 2, 3];
        for node in &mut nodes[1..=3] {
            node.width = Size::Fill(1.0);
        }
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 900.0, 10.0));
        assert_eq!([nodes[1].rect.w, nodes[2].rect.w, nodes[3].rect.w], [500.0, 200.0, 200.0], "取り分 300 より広い中身は中身の幅");
        // 全部が入りきらない時は、どれも中身の幅（前と同じくはみ出す）
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 400.0, 10.0));
        assert_eq!([nodes[1].rect.w, nodes[2].rect.w, nodes[3].rect.w], [500.0, 80.0, 0.0]);
    }

    #[test]
    fn a_clipping_fill_can_be_narrower_than_its_content() {
        // 表の升: 中身（500）より狭い取り分でも、取り分の幅にする（はみ出す所は描く時に切り取る）
        let mut nodes = vec![LayoutNode::new(Dir::Row), leaf(500.0, 10.0), leaf(80.0, 10.0)];
        nodes[0].children = vec![1, 2];
        nodes[1].width = Size::Fill(1.0);
        nodes[1].clip = true;
        nodes[2].width = Size::Fixed(80.0);
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 300.0, 10.0));
        assert_eq!((nodes[1].rect.w, nodes[2].rect.x), (220.0, 220.0));
        // 切り取らない物は今までどおり中身の幅
        nodes[1].clip = false;
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 300.0, 10.0));
        assert_eq!(nodes[1].rect.w, 500.0);
    }

    #[test]
    fn measure_adds_padding_and_gaps() {
        let mut nodes = vec![LayoutNode::new(Dir::Row), leaf(30.0, 10.0), leaf(40.0, 20.0)];
        nodes[0].children = vec![1, 2];
        nodes[0].gap = 5.0;
        nodes[0].pad = (8.0, 4.0);
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(nodes[0].measured, (30.0 + 5.0 + 40.0 + 16.0, 20.0 + 8.0));
    }

    #[test]
    fn scroll_takes_the_rest_and_shifts_its_content() {
        // [見出し 30][スクロール（Fill）: 中身 3 つ（各 100、間 10）]、窓の高さ 200
        let mut nodes = vec![LayoutNode::new(Dir::Column), leaf(100.0, 30.0), LayoutNode::new(Dir::Scroll), leaf(50.0, 100.0), leaf(50.0, 100.0), leaf(50.0, 100.0)];
        nodes[0].children = vec![1, 2];
        nodes[2].children = vec![3, 4, 5];
        nodes[2].height = Size::Fill(1.0);
        nodes[2].gap = 10.0;
        nodes[2].offset = 40.0;
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 300.0, 200.0));
        // 中身が高くても、スクロールは残りの高さだけを取る
        assert_eq!(nodes[2].rect, Rect::new(0.0, 30.0, 300.0, 170.0));
        assert_eq!(nodes[2].content, 320.0);
        // 中身は幅いっぱいで、スクロールの位置だけ上にずれる
        assert_eq!(nodes[3].rect, Rect::new(0.0, -10.0, 300.0, 100.0));
        assert_eq!(nodes[5].rect, Rect::new(0.0, 210.0, 300.0, 100.0));
    }

    #[test]
    fn scroll_with_short_content_lets_fill_children_grow() {
        let mut nodes = vec![LayoutNode::new(Dir::Scroll), leaf(10.0, 20.0), leaf(10.0, 0.0)];
        nodes[0].children = vec![1, 2];
        nodes[2].height = Size::Fill(1.0);
        layout(&mut nodes, 0, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(nodes[2].rect, Rect::new(0.0, 20.0, 100.0, 80.0));
    }

    #[test]
    fn list_places_rows_by_index_and_offset() {
        let mut nodes = vec![LayoutNode::new(Dir::List), LayoutNode::new(Dir::Row), LayoutNode::new(Dir::Row)];
        nodes[0].children = vec![1, 2];
        nodes[0].row_h = 32.0;
        nodes[0].offset = 40.0;
        nodes[1].row_index = 1;
        nodes[2].row_index = 2;
        layout(&mut nodes, 0, Rect::new(0.0, 100.0, 300.0, 64.0));
        assert_eq!(nodes[1].rect, Rect::new(0.0, 92.0, 300.0, 32.0));
        assert_eq!(nodes[2].rect, Rect::new(0.0, 124.0, 300.0, 32.0));
    }
}
