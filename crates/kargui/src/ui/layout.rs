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
}

#[derive(Clone, Debug)]
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

fn measure(nodes: &mut [LayoutNode], i: usize) -> (f32, f32) {
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
    nodes[i].measured = m;
    m
}

/// 主軸の長さを子に分ける。Fixed と Auto はそのまま、Fill は残りを重みで分ける
fn distribute(sizes: &[Size], measured: &[f32], avail: f32) -> Vec<f32> {
    let basis: Vec<f32> = sizes.iter().zip(measured).map(|(s, &m)| fixed_or(*s, m)).collect();
    let weights: f32 = sizes.iter().map(|s| if let Size::Fill(w) = s { w.max(0.0) } else { 0.0 }).sum();
    let free = avail - basis.iter().sum::<f32>();
    sizes
        .iter()
        .zip(basis)
        .map(|(s, b)| match s {
            Size::Fill(w) if weights > 0.0 && free > 0.0 => b + free * w.max(0.0) / weights,
            _ => b,
        })
        .collect()
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

fn arrange(nodes: &mut [LayoutNode], i: usize, rect: Rect) {
    nodes[i].rect = rect;
    let n = nodes[i].clone();
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
    }
}

/// root を rect に置き、すべての子の場所を決める
pub(crate) fn layout(nodes: &mut [LayoutNode], root: usize, rect: Rect) {
    measure(nodes, root);
    arrange(nodes, root, rect);
}

#[cfg(test)]
mod tests {
    use super::*;

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
