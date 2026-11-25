//! kargui の既定のアイコン: 濃い紺の角丸の地に、光の管で描いた小文字の k（縦の線と、少し離したくの字の腕）。
//! 線の色は水色から青紫、桃色へ移る。アプリがアイコンを渡さない時に、窓とトレイに出す（Options::default_icon）。
//!
//! 画像のファイルを読まず、大きさごとに計算で描く（どの OS でも同じ絵になり、小さい大きさでもにじまない）。
//! 1 画素を n × n の点に分けて数え、縁をなめらかにする。色は sRGB の値のまま混ぜる。

use std::sync::atomic::{AtomicBool, Ordering};

/// アプリの Options::default_icon（run が起動の時に入れる）
static ENABLED: AtomicBool = AtomicBool::new(true);

pub(crate) fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// アプリがアイコンを渡さない時に、kargui のアイコンを出すか
pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

type C = [f32; 3];

const fn hex(v: u32) -> C {
    [((v >> 16) & 0xff) as f32 / 255.0, ((v >> 8) & 0xff) as f32 / 255.0, (v & 0xff) as f32 / 255.0]
}

/// 線の色: 水色・青紫・桃色
const NEON: [C; 3] = [hex(0x38e8ff), hex(0x707aff), hex(0xff5fd1)];
/// 地の色: 上と下
const TOP: C = hex(0x1b2244);
const BOTTOM: C = hex(0x080b16);
/// 左上の淡い光
const SHINE: C = hex(0x4a5ad8);
/// 地の半分の大きさ（絵の幅を 1 とする）
const HALF: f32 = 0.46;
/// 線の太さの半分
const STROKE: f32 = 0.058;
/// 縦の線と、くの字の腕（始めと終わり）
const STEM: [(f32, f32); 2] = [(0.35, 0.19), (0.35, 0.81)];
const JOINT: (f32, f32) = (0.50, 0.585);
const UPPER: (f32, f32) = (0.70, 0.385);
const LOWER: (f32, f32) = (0.71, 0.81);

fn lerp(a: C, b: C, t: f32) -> C {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn add(a: C, b: C, k: f32) -> C {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn grad(t: f32) -> C {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { lerp(NEON[0], NEON[1], t * 2.0) } else { lerp(NEON[1], NEON[2], t * 2.0 - 1.0) }
}

/// 点 p から線分 ab までの距離
fn seg(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py, bx, by) = (p.0 - a.0, p.1 - a.1, b.0 - a.0, b.1 - a.1);
    let h = ((px * bx + py * by) / (bx * bx + by * by)).clamp(0.0, 1.0);
    ((px - bx * h).powi(2) + (py - by * h).powi(2)).sqrt()
}

/// 絵の (u, v)（どちらも 0〜1）の色。地の外は None
fn shade(u: f32, v: f32) -> Option<C> {
    // 地: 角の丸みがなめらかな四角（超楕円、指数 5）。edge は縁までのおよその距離
    let (x, y) = (((u - 0.5) / HALF).abs(), ((v - 0.5) / HALF).abs());
    let f = (x.powi(5) + y.powi(5)).powf(0.2);
    if f > 1.0 {
        return None;
    }
    let edge = (1.0 - f) * HALF;
    let mut c = lerp(TOP, BOTTOM, v);
    c = add(c, SHINE, 0.22 * (-((u - 0.22).powi(2) + (v - 0.12).powi(2)) / 0.09).exp());
    // 縁の細い光（上と左ほど明るい）
    c = add(c, grad(u * 0.6 + v * 0.4), smooth(0.016, 0.0, edge) * (0.25 + 0.35 * (1.0 - v)));
    // k: 線の周りの光と、芯の白い管
    let p = (u, v);
    let d = seg(p, STEM[0], STEM[1]).min(seg(p, JOINT, UPPER)).min(seg(p, JOINT, LOWER));
    let col = grad(((u - 0.3) * 0.75 + (v - 0.18) * 0.55) / 0.62);
    c = add(c, col, 0.5 * (-(d - STROKE).max(0.0) / 0.045).exp());
    if d < STROKE {
        c = lerp(col, [1.0, 1.0, 1.0], smooth(STROKE * 0.75, 0.0, d) * 0.6);
    }
    Some(c)
}

/// size × size 画素の kargui のアイコン
pub(crate) fn icon(size: u32) -> crate::Image {
    let size = size.clamp(1, 1024);
    // 小さい時ほど細かく数える（16 画素でも線がにじまない）
    let n = if size <= 48 { 8 } else { 4 };
    let (s, k) = (size as f32, 1.0 / (n * n) as f32);
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..n {
                for sx in 0..n {
                    let u = (x as f32 + (sx as f32 + 0.5) / n as f32) / s;
                    let v = (y as f32 + (sy as f32 + 0.5) / n as f32) / s;
                    if let Some(c) = shade(u, v) {
                        for i in 0..3 {
                            acc[i] += c[i].clamp(0.0, 1.0);
                        }
                        acc[3] += 1.0;
                    }
                }
            }
            // 乗算済み（数えた点の色の和を、点の数で割る）
            px.extend(acc.map(|v| (v * k * 255.0 + 0.5) as u8));
        }
    }
    crate::Image::premultiplied(size, size, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_a_dark_rounded_square_with_a_bright_k() {
        let img = icon(64);
        let d = &img.0;
        assert_eq!((d.width, d.height, d.pixels.len()), (64, 64, 64 * 64 * 4));
        let at = |x: u32, y: u32| {
            let i = ((y * 64 + x) * 4) as usize;
            [d.pixels[i], d.pixels[i + 1], d.pixels[i + 2], d.pixels[i + 3]]
        };
        // 角は透明、真ん中より右下の地は不透明で暗い
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(63, 63)[3], 0);
        let ground = at(52, 30);
        assert_eq!(ground[3], 255);
        assert!(ground[..3].iter().all(|&c| c < 90), "dark ground {ground:?}");
        // 縦の線（x = 0.35）の上は明るい
        let stem = at((0.35 * 64.0) as u32, 32);
        assert!(stem[..3].iter().map(|&c| c as u32).sum::<u32>() > 450, "bright stem {stem:?}");
        // 乗算済み: 色はどれも不透明度を超えない
        assert!(d.pixels.as_chunks::<4>().0.iter().all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
    }

    #[test]
    fn small_icons_keep_the_k() {
        // 16 画素でも、縦の線の列は地より明るい
        let img = icon(16);
        let d = &img.0;
        let lum = |x: u32, y: u32| {
            let i = ((y * 16 + x) * 4) as usize;
            d.pixels[i] as u32 + d.pixels[i + 1] as u32 + d.pixels[i + 2] as u32
        };
        let stem_x = (0.35 * 16.0) as u32;
        assert!(lum(stem_x, 8) > lum(stem_x + 3, 4) + 150, "stem {} ground {}", lum(stem_x, 8), lum(stem_x + 3, 4));
    }
}
