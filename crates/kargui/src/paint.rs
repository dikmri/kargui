//! 描く物の一覧。GPU の道（gpu.rs）も CPU の道（raster.rs）も、同じ一覧を受け取る。
//!
//! 描く物は「層」に分かれる。層は切り取る範囲（clip）を持ち、範囲の外には描かない。
//! スクロールする部品の中身は、スクロールの番号と位置を持つ層に入る。
//! 変わった所だけ描く仕組み（damage.rs）は、この情報でスクロールを絵のずらしで済ませる。
//! 座標は窓の中の物理ピクセル。スクロールする層の物も、スクロールを反映した窓の座標で入れる。

use crate::geom::{Color, IRect, Rect};
use crate::image::Image;

/// 描く物 1 つ。GPU の頂点の入力と同じ並び（64 バイト）。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Quad {
    /// x, y, 幅, 高さ（画素）
    pub rect: [f32; 4],
    /// 色（乗算していない RGBA）
    pub color: [f32; 4],
    /// 文字の形の表の中の位置 x, y, 幅, 高さ（画素）。幅が 0 なら塗りつぶし
    pub uv: [f32; 4],
    /// x: 角の丸み（画素）。y: 画像なら DrawList::images の番号 + 1（0 なら画像ではない）。
    /// z, w: 画像の id の下と上の 32 ビット（変わった所を見つける要約に入れる）
    pub extra: [f32; 4],
}

impl Quad {
    pub fn is_glyph(&self) -> bool {
        self.uv[2] > 0.0 && self.extra[1] == 0.0
    }
    /// 画像なら DrawList::images の番号
    pub fn image_index(&self) -> Option<usize> {
        (self.extra[1] > 0.0).then(|| self.extra[1] as usize - 1)
    }
    pub fn bounds(&self) -> Rect {
        Rect::new(self.rect[0], self.rect[1], self.rect[2], self.rect[3])
    }
}

/// スクロールする層の印
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Scroll {
    /// スクロールする部品の番号（フレームをまたいで同じ部品なら同じ）
    pub id: u64,
    /// スクロールの位置（画素、整数）。中身は窓の座標に -offset して入っている
    pub offset: i32,
    /// 入れ子の切り取りとは別に、スクロール部品自体の見える範囲を覚える
    pub viewport: IRect,
}

/// 同じ切り取り範囲で続けて描く物のまとまり
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layer {
    pub clip: IRect,
    pub start: usize,
    pub end: usize,
    pub scroll: Option<Scroll>,
}

#[derive(Default)]
pub(crate) struct DrawList {
    pub quads: Vec<Quad>,
    pub layers: Vec<Layer>,
    /// このフレームで描く画像（Quad::image_index が指す）
    pub images: Vec<Image>,
    stack: Vec<(IRect, Option<Scroll>)>,
}

impl DrawList {
    /// 描き始める。window は窓全体
    pub fn begin(&mut self, window: IRect) {
        self.quads.clear();
        self.layers.clear();
        self.images.clear();
        self.stack.clear();
        self.stack.push((window, None));
        self.open();
    }

    fn open(&mut self) {
        let (clip, scroll) = *self.stack.last().expect("begin が先");
        self.layers.push(Layer { clip, start: self.quads.len(), end: self.quads.len(), scroll });
    }

    fn close(&mut self) {
        let n = self.quads.len();
        if let Some(l) = self.layers.last_mut() {
            l.end = n;
        }
        // 中身の無い層は捨てる（切り取り範囲を何度も切り替えたときに増えないように）
        if self.layers.last().is_some_and(|l| l.start == l.end) {
            self.layers.pop();
        }
    }

    /// 切り取り範囲を狭め、外側のスクロールを受け継ぐ
    pub fn push_clip(&mut self, clip: IRect) {
        self.push(clip, None);
    }

    /// 切り取り範囲を狭め、その範囲を新しいスクロールの見える範囲にする
    pub fn push_scroll(&mut self, clip: IRect, id: u64, offset: i32) {
        self.push(clip, Some((id, offset)));
    }

    fn push(&mut self, clip: IRect, scroll: Option<(u64, i32)>) {
        self.close();
        let (parent, parent_scroll) = *self.stack.last().expect("begin が先");
        let clip = clip.intersect(&parent).unwrap_or(IRect::new(clip.x, clip.y, 0, 0));
        let scroll = scroll.map(|(id, offset)| Scroll { id, offset, viewport: clip }).or(parent_scroll);
        self.stack.push((clip, scroll));
        self.open();
    }

    pub fn pop_clip(&mut self) {
        self.close();
        if self.stack.len() > 1 {
            self.stack.pop();
        }
        self.open();
    }

    /// 描き終える
    pub fn finish(&mut self) {
        self.close();
    }

    /// 塗りつぶした四角（radius > 0 なら角を丸める）
    pub fn fill(&mut self, r: Rect, color: Color, radius: f32) {
        let r = r.snapped();
        if r.w <= 0.0 || r.h <= 0.0 || color[3] <= 0.0 {
            return;
        }
        let radius = radius.min(r.w * 0.5).min(r.h * 0.5).max(0.0);
        self.quads.push(Quad { rect: [r.x, r.y, r.w, r.h], color, uv: [0.0; 4], extra: [radius, 0.0, 0.0, 0.0] });
    }

    /// 枠付きの四角。外側を枠の色、内側を塗りの色で 2 回描く
    pub fn boxed(&mut self, r: Rect, fill: Color, border: Color, width: f32, radius: f32) {
        self.fill(r, border, radius);
        self.fill(r.inset(width), fill, (radius - width).max(0.0));
    }

    /// 文字の形 1 つ。x, y は形の左上（整数の画素）、ax, ay は表の中の位置
    /// 画像を r に引き伸ばして描く
    pub fn image(&mut self, r: Rect, image: &Image) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        self.images.push(image.clone());
        let n = self.images.len() as f32;
        let id = image.0.id;
        let extra = [0.0, n, f32::from_bits(id as u32), f32::from_bits((id >> 32) as u32)];
        self.quads.push(Quad { rect: [r.x, r.y, r.w, r.h], color: [1.0; 4], uv: [0.0, 0.0, image.width() as f32, image.height() as f32], extra });
    }

    #[allow(clippy::too_many_arguments)] // 字の位置・大きさ・字形の座標・色を、呼ぶ側でそのまま並べる
    pub fn glyph(&mut self, x: f32, y: f32, w: f32, h: f32, ax: f32, ay: f32, color: Color) {
        self.quads.push(Quad { rect: [x, y, w, h], color, uv: [ax, ay, w, h], extra: [0.0; 4] });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::rgb;

    #[test]
    fn layers_split_by_clip_and_drop_empty_ones() {
        let mut d = DrawList::default();
        d.begin(IRect::new(0, 0, 100, 100));
        d.fill(Rect::new(0.0, 0.0, 10.0, 10.0), rgb(0), 0.0);
        d.push_scroll(IRect::new(0, 20, 100, 50), 7, 30);
        d.fill(Rect::new(0.0, 25.0, 10.0, 10.0), rgb(0), 0.0);
        // 入れ子の切り取りはスクロールを受け継ぐ
        d.push_clip(IRect::new(0, 0, 50, 200));
        d.fill(Rect::new(0.0, 30.0, 10.0, 10.0), rgb(0), 0.0);
        d.pop_clip();
        d.pop_clip();
        d.push_clip(IRect::new(0, 0, 1, 1));
        d.pop_clip();
        d.fill(Rect::new(0.0, 90.0, 10.0, 10.0), rgb(0), 0.0);
        d.finish();
        let summary: Vec<_> = d.layers.iter().map(|l| (l.clip, l.start, l.end, l.scroll.map(|s| s.id))).collect();
        assert_eq!(
            summary,
            vec![
                (IRect::new(0, 0, 100, 100), 0, 1, None),
                (IRect::new(0, 20, 100, 50), 1, 2, Some(7)),
                (IRect::new(0, 20, 50, 50), 2, 3, Some(7)),
                (IRect::new(0, 0, 100, 100), 3, 4, None),
            ]
        );
    }
}
