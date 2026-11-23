//! 多角形を塗って、なめらかな縁の濃さの表（1 画素 1 バイト）を作る。アイコンの形に使う。
//!
//! 面積を数える方式: 辺が画素の行を横切るたびに、その辺の左右の面積の差を表に足し込み、最後に左上から順に
//! 足し上げると、各画素の「形に覆われた割合」になる（font-rs と同じ考え方）。向きの同じ形は合わさり（重なっても
//! 濃さは 1 で止める）、向きが逆の形は差し引かれて穴になる。

/// 点（画素の座標。左上が 0, 0）
pub(crate) type Pt = (f32, f32);

pub(crate) struct Raster {
    w: usize,
    h: usize,
    /// 面積の差。行の終わりからはみ出した分を受けるため、少し長く取る
    acc: Vec<f32>,
}

impl Raster {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, acc: vec![0.0; w * h + w + 4] }
    }

    /// 1 本の辺を足し込む（上から下へ向かう辺は +、下から上は -）
    fn line(&mut self, p0: Pt, p1: Pt) {
        if (p0.1 - p1.1).abs() <= f32::EPSILON {
            return;
        }
        let maxx = (self.w as f32 - 0.001).max(0.0);
        let p0 = (p0.0.clamp(0.0, maxx), p0.1);
        let p1 = (p1.0.clamp(0.0, maxx), p1.1);
        let (dir, p0, p1) = if p0.1 < p1.1 { (1.0, p0, p1) } else { (-1.0, p1, p0) };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        if p0.1 < 0.0 {
            x -= p0.1 * dxdy;
        }
        let y_start = p0.1.max(0.0) as usize;
        let y_end = (p1.1.ceil().max(0.0) as usize).min(self.h);
        for y in y_start..y_end {
            let row = y * self.w;
            let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(p0.1);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let x0f = x0.floor();
            let x0i = x0f as usize;
            let x1c = x1.ceil();
            let x1i = x1c as usize;
            if x1i <= x0i + 1 {
                // 1 つの画素の中で終わる
                let xm = 0.5 * (x + xnext) - x0f;
                self.acc[row + x0i] += d - d * xm;
                self.acc[row + x0i + 1] += d * xm;
            } else {
                let s = (x1 - x0).recip();
                let fx0 = x0 - x0f;
                let a0 = 0.5 * s * (1.0 - fx0) * (1.0 - fx0);
                let fx1 = x1 - x1c + 1.0;
                let am = 0.5 * s * fx1 * fx1;
                self.acc[row + x0i] += d * a0;
                if x1i == x0i + 2 {
                    self.acc[row + x0i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - fx0);
                    self.acc[row + x0i + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.acc[row + xi] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.acc[row + x1i - 1] += d * (1.0 - a2 - am);
                }
                self.acc[row + x1i] += d * am;
            }
            x = xnext;
        }
    }

    /// 閉じた多角形を足し込む（最後の点から最初の点へも辺を引く）
    pub fn polygon(&mut self, pts: &[Pt]) {
        for i in 0..pts.len() {
            self.line(pts[i], pts[(i + 1) % pts.len()]);
        }
    }

    /// 濃さの表（0〜255、w×h）
    pub fn coverage(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h);
        let mut sum = 0.0f32;
        for &a in &self.acc[..self.w * self.h] {
            sum += a;
            out.push((sum.abs().min(1.0) * 255.0 + 0.5) as u8);
        }
        out
    }
}

/// 多角形の符号付きの面積（画面の座標で、時計回りが正）
pub(crate) fn area(pts: &[Pt]) -> f32 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p.0 * q.1 - q.0 * p.1;
    }
    a * 0.5
}

/// 塗る形（向きを時計回りにそろえる）
pub(crate) fn solid(mut pts: Vec<Pt>) -> Vec<Pt> {
    if area(&pts) < 0.0 {
        pts.reverse();
    }
    pts
}

/// 穴の形（向きを反時計回りにそろえる）
pub(crate) fn hole(mut pts: Vec<Pt>) -> Vec<Pt> {
    if area(&pts) > 0.0 {
        pts.reverse();
    }
    pts
}

/// 円（多角形で近似する）
pub(crate) fn circle(c: Pt, r: f32) -> Vec<Pt> {
    let n = ((r * 8.0).ceil() as usize).clamp(16, 64);
    solid((0..n).map(|i| {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        (c.0 + r * a.cos(), c.1 + r * a.sin())
    })
    .collect())
}

/// 太さ w の線（端と角は丸める）。点を結んだ折れ線を、四角と円の和で作る
pub(crate) fn stroke(points: &[Pt], w: f32) -> Vec<Vec<Pt>> {
    let r = w / 2.0;
    let mut shapes = Vec::new();
    for seg in points.windows(2) {
        let (p, q) = (seg[0], seg[1]);
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (nx, ny) = (-dy / len * r, dx / len * r);
        shapes.push(solid(vec![(p.0 + nx, p.1 + ny), (q.0 + nx, q.1 + ny), (q.0 - nx, q.1 - ny), (p.0 - nx, p.1 - ny)]));
    }
    for &p in points {
        shapes.push(circle(p, r));
    }
    shapes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(w: usize, h: usize, polys: &[Vec<Pt>]) -> Vec<u8> {
        let mut r = Raster::new(w, h);
        for p in polys {
            r.polygon(p);
        }
        r.coverage()
    }

    #[test]
    fn a_pixel_aligned_square_is_solid_inside_and_empty_outside() {
        let c = fill(5, 5, &[vec![(1.0, 1.0), (4.0, 1.0), (4.0, 4.0), (1.0, 4.0)]]);
        for y in 0..5 {
            for x in 0..5 {
                let inside = (1..4).contains(&x) && (1..4).contains(&y);
                assert_eq!(c[y * 5 + x], if inside { 255 } else { 0 }, "({x}, {y})");
            }
        }
    }

    #[test]
    fn a_half_covered_pixel_is_half_dark() {
        let c = fill(4, 3, &[vec![(1.5, 0.0), (3.0, 0.0), (3.0, 3.0), (1.5, 3.0)]]);
        assert_eq!(c[1], 128);
        assert_eq!(c[2], 255);
        assert_eq!(c[0], 0);
    }

    #[test]
    fn total_ink_matches_the_area() {
        let tri = vec![(1.0, 1.0), (9.0, 2.0), (3.0, 8.0)];
        let c = fill(10, 10, std::slice::from_ref(&tri));
        let ink: f32 = c.iter().map(|&v| v as f32 / 255.0).sum();
        let want = area(&tri).abs();
        assert!((ink - want).abs() < 0.3, "ink {ink} area {want}");
    }

    #[test]
    fn a_reversed_inner_circle_makes_a_hole() {
        let c = fill(20, 20, &[circle((10.0, 10.0), 8.0), hole(circle((10.0, 10.0), 4.0))]);
        assert_eq!(c[10 * 20 + 10], 0, "真ん中は穴");
        assert_eq!(c[10 * 20 + 3], 255, "輪の上は塗る");
        assert_eq!(c[0], 0);
    }

    #[test]
    fn direction_does_not_change_the_fill_and_overlaps_stay_solid() {
        let cw = fill(8, 8, &[vec![(1.0, 1.0), (7.0, 1.0), (7.0, 7.0), (1.0, 7.0)]]);
        let ccw = fill(8, 8, &[vec![(1.0, 1.0), (1.0, 7.0), (7.0, 7.0), (7.0, 1.0)]]);
        assert_eq!(cw, ccw);
        // 同じ向きの形の重なりは濃いまま（2 倍にならない）
        let both = fill(8, 8, &[solid(vec![(1.0, 1.0), (5.0, 1.0), (5.0, 5.0), (1.0, 5.0)]), solid(vec![(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)])]);
        assert_eq!(both[4 * 8 + 4], 255);
        assert_eq!(both[0], 0);
    }

    #[test]
    fn a_stroke_covers_its_line_with_round_ends() {
        let c = fill(20, 10, &stroke(&[(3.0, 5.0), (17.0, 5.0)], 2.0));
        assert_eq!(c[5 * 20 + 10], 255, "線の上");
        assert_eq!(c[2 * 20 + 10], 0, "線の外");
        assert!(c[5 * 20 + 2] > 0, "丸い端は点の外まで少し出る");
    }
}
