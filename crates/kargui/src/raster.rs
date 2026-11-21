//! CPU の道の描き方。描く物の一覧を、画素の並び（BGRA、1 画素 = u32）に描く。
//! 描くのは region の中だけ。層ごとに、その層の切り取り範囲とも重ねて切る。
//! 角の丸い四角の計算は、GPU のシェーダー（shaders/ui.hlsl）と同じ。

use crate::geom::{Color, IRect};
use crate::paint::{DrawList, Quad};

pub(crate) struct Canvas<'a> {
    pub px: &'a mut [u32],
    pub width: i32,
    pub height: i32,
}

/// 色を 0〜255 の BGR と不透明度にする
fn channels(c: &Color) -> (u32, u32, u32, u32) {
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    (to(c[2]), to(c[1]), to(c[0]), to(c[3]))
}

/// dst に (b, g, r) を不透明度 a（0〜255）で重ねる
#[inline]
fn blend(dst: u32, b: u32, g: u32, r: u32, a: u32) -> u32 {
    if a >= 255 {
        return 0xff00_0000 | (r << 16) | (g << 8) | b;
    }
    if a == 0 {
        return dst;
    }
    let inv = 255 - a;
    let ob = (b * a + (dst & 0xff) * inv + 127) / 255;
    let og = (g * a + ((dst >> 8) & 0xff) * inv + 127) / 255;
    let or = (r * a + ((dst >> 16) & 0xff) * inv + 127) / 255;
    0xff00_0000 | (or << 16) | (og << 8) | ob
}

/// 角の丸い四角の、画素の中心 (px, py) での覆い具合（0〜1）。四角の左上が (0, 0)
pub(crate) fn rounded_coverage(px: f32, py: f32, w: f32, h: f32, radius: f32) -> f32 {
    let hw = w * 0.5;
    let hh = h * 0.5;
    let qx = (px - hw).abs() - (hw - radius);
    let qy = (py - hh).abs() - (hh - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    (0.5 - (outside + inside - radius)).clamp(0.0, 1.0)
}

impl Canvas<'_> {
    fn bounds(&self) -> IRect {
        IRect::new(0, 0, self.width, self.height)
    }

    /// region の中を背景の色で塗り、一覧の物を層ごとに描く
    pub fn draw(&mut self, list: &DrawList, atlas: &[u8], atlas_size: u32, region: IRect, background: Color) {
        let Some(region) = region.intersect(&self.bounds()) else { return };
        let (b, g, r, _) = channels(&background);
        let solid = 0xff00_0000 | (r << 16) | (g << 8) | b;
        for y in region.y..region.bottom() {
            let row = (y * self.width) as usize;
            self.px[row + region.x as usize..row + region.right() as usize].fill(solid);
        }
        for layer in &list.layers {
            let Some(clip) = layer.clip.intersect(&region) else { continue };
            for q in &list.quads[layer.start..layer.end] {
                let Some(area) = IRect::covering(q.bounds()).intersect(&clip) else { continue };
                if let Some(i) = q.image_index() {
                    self.image(q, area, &list.images[i]);
                } else if q.is_glyph() {
                    self.glyph(q, area, atlas, atlas_size);
                } else {
                    self.rounded(q, area);
                }
            }
        }
    }

    fn rounded(&mut self, q: &Quad, area: IRect) {
        let (b, g, r, a) = channels(&q.color);
        let [qx, qy, qw, qh] = q.rect;
        let radius = q.extra[0];
        for y in area.y..area.bottom() {
            let row = (y * self.width) as usize;
            let ly = y as f32 + 0.5 - qy;
            // 角にかからない行は、左右の端まで一様に塗る
            let corner_row = radius > 0.0 && (ly < radius || ly > qh - radius);
            if !corner_row && a >= 255 {
                let solid = 0xff00_0000 | (r << 16) | (g << 8) | b;
                self.px[row + area.x as usize..row + area.right() as usize].fill(solid);
                continue;
            }
            for x in area.x..area.right() {
                let cov = if corner_row {
                    let lx = x as f32 + 0.5 - qx;
                    if lx < radius || lx > qw - radius { rounded_coverage(lx, ly, qw, qh, radius) } else { 1.0 }
                } else {
                    1.0
                };
                let alpha = (a as f32 * cov + 0.5) as u32;
                let i = row + x as usize;
                self.px[i] = blend(self.px[i], b, g, r, alpha);
            }
        }
    }

    /// 画像を q.rect に引き伸ばして重ねる（乗算済みの色。拡大縮小は 4 点の線形補間、等倍はそのまま写す）
    fn image(&mut self, q: &Quad, area: IRect, image: &crate::image::Image) {
        let data = &image.0;
        let (iw, ih) = (data.width as i32, data.height as i32);
        let [qx, qy, qw, qh] = q.rect;
        let px = &data.pixels;
        let texel = |x: i32, y: i32| -> [u32; 4] {
            let i = ((y.clamp(0, ih - 1) * iw + x.clamp(0, iw - 1)) * 4) as usize;
            [px[i] as u32, px[i + 1] as u32, px[i + 2] as u32, px[i + 3] as u32]
        };
        let exact = qw as i32 == iw && qh as i32 == ih && qx.fract() == 0.0 && qy.fract() == 0.0;
        let (sx, sy) = (iw as f32 / qw, ih as f32 / qh);
        for y in area.y..area.bottom() {
            let row = (y * self.width) as usize;
            for x in area.x..area.right() {
                let [r, g, b, a] = if exact {
                    texel(x - qx as i32, y - qy as i32)
                } else {
                    // 画素の真ん中の位置を画像の座標にして、まわりの 4 つを混ぜる
                    let u = (x as f32 + 0.5 - qx) * sx - 0.5;
                    let v = (y as f32 + 0.5 - qy) * sy - 0.5;
                    let (x0, y0) = (u.floor() as i32, v.floor() as i32);
                    let (fx, fy) = (u - u.floor(), v - v.floor());
                    let (t00, t10, t01, t11) = (texel(x0, y0), texel(x0 + 1, y0), texel(x0, y0 + 1), texel(x0 + 1, y0 + 1));
                    let mut out = [0u32; 4];
                    for c in 0..4 {
                        let top = t00[c] as f32 * (1.0 - fx) + t10[c] as f32 * fx;
                        let bottom = t01[c] as f32 * (1.0 - fx) + t11[c] as f32 * fx;
                        out[c] = (top * (1.0 - fy) + bottom * fy + 0.5) as u32;
                    }
                    out
                };
                if a == 0 {
                    continue;
                }
                let i = row + x as usize;
                let dst = self.px[i];
                let inv = 255 - a;
                let ob = (b + ((dst & 0xff) * inv + 127) / 255).min(255);
                let og = (g + (((dst >> 8) & 0xff) * inv + 127) / 255).min(255);
                let or = (r + (((dst >> 16) & 0xff) * inv + 127) / 255).min(255);
                self.px[i] = 0xff00_0000 | (or << 16) | (og << 8) | ob;
            }
        }
    }

    fn glyph(&mut self, q: &Quad, area: IRect, atlas: &[u8], atlas_size: u32) {
        let (b, g, r, a) = channels(&q.color);
        let ox = q.rect[0] as i32;
        let oy = q.rect[1] as i32;
        let ax = q.uv[0] as i32;
        let ay = q.uv[1] as i32;
        for y in area.y..area.bottom() {
            let row = (y * self.width) as usize;
            let src = ((ay + y - oy) as u32 * atlas_size) as usize;
            for x in area.x..area.right() {
                let m = atlas[src + (ax + x - ox) as usize] as u32;
                if m == 0 {
                    continue;
                }
                let alpha = (m * a + 127) / 255;
                let i = row + x as usize;
                self.px[i] = blend(self.px[i], b, g, r, alpha);
            }
        }
    }

    /// region の中の絵を dy 画素だけ上へずらす（dy が負なら下へ）。
    /// 空いた帯は描き直しが要るので、その範囲を返す。|dy| が region の高さ以上なら region 全体を返す。
    pub fn scroll_up(&mut self, region: IRect, dy: i32) -> IRect {
        let Some(region) = region.intersect(&self.bounds()) else { return IRect::default() };
        if dy == 0 {
            return IRect::new(region.x, region.y, region.w, 0);
        }
        if dy.abs() >= region.h {
            return region;
        }
        let (x0, x1) = (region.x as usize, region.right() as usize);
        let w = self.width as usize;
        if dy > 0 {
            for y in region.y..region.bottom() - dy {
                let dst = y as usize * w;
                let src = (y + dy) as usize * w;
                self.px.copy_within(src + x0..src + x1, dst + x0);
            }
            IRect::new(region.x, region.bottom() - dy, region.w, dy)
        } else {
            let d = -dy;
            for y in (region.y + d..region.bottom()).rev() {
                let dst = y as usize * w;
                let src = (y - d) as usize * w;
                self.px.copy_within(src + x0..src + x1, dst + x0);
            }
            IRect::new(region.x, region.y, region.w, d)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Rect, rgb};

    const WHITE: u32 = 0xffff_ffff;
    const BLACK: u32 = 0xff00_0000;

    fn canvas(px: &mut [u32], w: i32, h: i32) -> Canvas<'_> {
        Canvas { px, width: w, height: h }
    }

    #[test]
    fn coverage_is_full_inside_and_zero_outside_corner() {
        assert_eq!(rounded_coverage(5.0, 5.0, 10.0, 10.0, 4.0), 1.0);
        assert_eq!(rounded_coverage(0.5, 0.5, 10.0, 10.0, 4.0), 0.0);
        let edge = rounded_coverage(5.0, 0.0, 10.0, 10.0, 4.0);
        assert!((edge - 0.5).abs() < 1e-6, "{edge}");
    }

    #[test]
    fn draw_respects_region_and_layer_clip() {
        let mut px = vec![0u32; 8 * 4];
        let mut c = canvas(&mut px, 8, 4);
        let mut list = DrawList::default();
        list.begin(IRect::new(0, 0, 8, 4));
        list.push_clip(IRect::new(0, 0, 4, 4), None);
        list.fill(Rect::new(0.0, 0.0, 8.0, 4.0), rgb(0x000000), 0.0);
        list.pop_clip();
        list.finish();
        c.draw(&list, &[], 0, IRect::new(2, 1, 6, 2), rgb(0xffffff));
        for y in 0..4 {
            for x in 0..8 {
                let in_region = (2..8).contains(&x) && (1..3).contains(&y);
                let expect = if !in_region { 0 } else if x < 4 { BLACK } else { WHITE };
                assert_eq!(px[y * 8 + x], expect, "x={x} y={y}");
            }
        }
    }

    #[test]
    fn glyph_mask_blends_by_coverage() {
        let atlas = [255u8, 0, 0, 0];
        let mut px = vec![WHITE; 4];
        let mut c = canvas(&mut px, 4, 1);
        let mut list = DrawList::default();
        list.begin(IRect::new(0, 0, 4, 1));
        list.glyph(1.0, 0.0, 2.0, 1.0, 0.0, 0.0, rgb(0x000000));
        list.finish();
        c.draw(&list, &atlas, 2, IRect::new(0, 0, 4, 1), rgb(0xffffff));
        assert_eq!(px, vec![WHITE, BLACK, WHITE, WHITE]);
    }

    #[test]
    fn scroll_up_moves_rows_and_returns_the_strip() {
        let mut px: Vec<u32> = (0..5).collect();
        let mut c = canvas(&mut px, 1, 5);
        assert_eq!(c.scroll_up(IRect::new(0, 1, 1, 4), 2), IRect::new(0, 3, 1, 2));
        assert_eq!(&px[..3], &[0, 3, 4]);
        let mut px: Vec<u32> = (0..5).collect();
        let mut c = canvas(&mut px, 1, 5);
        assert_eq!(c.scroll_up(IRect::new(0, 0, 1, 5), -1), IRect::new(0, 0, 1, 1));
        assert_eq!(&px[1..], &[0, 1, 2, 3]);
    }
}
