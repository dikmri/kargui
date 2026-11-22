//! 色の計算（色の選択で使う）: RGB と HSV の変換、`#RRGGBB` の文字、色の選択の濃淡の絵。
//! 色は sRGB の値のまま扱う（色の選択の部品は、ほかの OS やアプリの色の選択と同じく sRGB の上の HSV で動く）。

use crate::geom::Color;

/// The words a color picker shows and screen readers hear ([`Ui::color_picker`](crate::Ui::color_picker)).
/// English by default; change them with [`Ui::set_color_text`](crate::Ui::set_color_text).
///
/// ```
/// # kargui::test::example(|ui| {
/// ui.set_color_text(kargui::ColorText::japanese());
/// # });
/// assert_eq!(kargui::ColorText::japanese().hue, "色相");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ColorText {
    /// The name of the square where saturation (left to right) and brightness (bottom to top) are picked.
    pub area: &'static str,
    /// Saturation, as screen readers hear it in the square's value.
    pub saturation: &'static str,
    /// Brightness, as screen readers hear it in the square's value.
    pub brightness: &'static str,
    /// The name of the hue bar.
    pub hue: &'static str,
    /// The name of the opacity bar.
    pub opacity: &'static str,
    /// The name of the field for the color code (`#RRGGBB`).
    pub code: &'static str,
    /// The name of the group of ready-made colors.
    pub swatches: &'static str,
}

impl ColorText {
    /// English (the default).
    ///
    /// ```
    /// assert_eq!(kargui::ColorText::english().opacity, "Opacity");
    /// ```
    pub fn english() -> ColorText {
        ColorText {
            area: "Saturation and brightness",
            saturation: "Saturation",
            brightness: "Brightness",
            hue: "Hue",
            opacity: "Opacity",
            code: "Color code",
            swatches: "Colors",
        }
    }

    /// Japanese.
    ///
    /// ```
    /// assert_eq!(kargui::ColorText::japanese().code, "色の番号");
    /// ```
    pub fn japanese() -> ColorText {
        ColorText {
            area: "彩度と明るさ",
            saturation: "彩度",
            brightness: "明るさ",
            hue: "色相",
            opacity: "不透明度",
            code: "色の番号",
            swatches: "色の見本",
        }
    }
}

impl Default for ColorText {
    fn default() -> Self {
        ColorText::english()
    }
}

/// 色の選択の見本の既定（灰色 4 つと、色相を回った 12 色）
pub(crate) const SWATCHES: [u32; 16] = [
    0x000000, 0x5F6368, 0xBDC1C6, 0xFFFFFF, 0xD93025, 0xF57C00, 0xF9AB00, 0x7CB342, 0x188038, 0x00897B, 0x0097A7, 0x1A73E8, 0x3949AB, 0x8E24AA, 0xD81B60,
    0x795548,
];

/// 開いている色の選択の状態（灰色にしても色相を覚えておくため、HSV で持つ）
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PickerState {
    pub hsv: [f32; 3],
    pub alpha: f32,
    /// 色の番号の欄の文字（打っている途中は、読めるまで色を変えない）
    pub code: String,
}

impl PickerState {
    pub fn of(color: Color, with_alpha: bool) -> PickerState {
        PickerState { hsv: rgb_to_hsv([color[0], color[1], color[2]]), alpha: color[3], code: hex(color, with_alpha) }
    }

    pub fn color(&self) -> Color {
        let [r, g, b] = hsv_to_rgb(self.hsv[0], self.hsv[1], self.hsv[2]);
        [r, g, b, self.alpha]
    }
}

/// 色の選択の作り手が決めて、次のフレームから効く設定（.alpha() と .swatches()）
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PickerOptions {
    pub alpha: bool,
    pub swatches: Option<Vec<Color>>,
}

/// HSV（色相 0〜360、彩度 0〜1、明るさ 0〜1）→ RGB（0〜1）
pub(crate) fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 60.0;
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

/// RGB（0〜1）→ HSV。灰色（彩度 0）の色相は決まらないので 0
pub(crate) fn rgb_to_hsv(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|c| c.clamp(0.0, 1.0));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max <= f32::EPSILON { 0.0 } else { d / max };
    [h, s, max]
}

fn byte(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `#RRGGBB`（不透明でない時は `#RRGGBBAA`）
pub(crate) fn hex(color: Color, with_alpha: bool) -> String {
    let [r, g, b, a] = color.map(byte);
    if with_alpha && a != 255 { format!("#{r:02X}{g:02X}{b:02X}{a:02X}") } else { format!("#{r:02X}{g:02X}{b:02X}") }
}

/// `#RGB`・`#RRGGBB`・`#RRGGBBAA`（`#` は無くてもよい）を色にする。読めなければ None
pub(crate) fn parse_hex(text: &str) -> Option<Color> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    if !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = |s: &str| u8::from_str_radix(s, 16).ok().map(|v| v as f32 / 255.0);
    match t.len() {
        3 => {
            let d: Vec<f32> = t.chars().map(|c| u8::from_str_radix(&c.to_string(), 16).map(|v| (v * 17) as f32 / 255.0)).collect::<Result<_, _>>().ok()?;
            Some([d[0], d[1], d[2], 1.0])
        }
        6 => Some([n(&t[0..2])?, n(&t[2..4])?, n(&t[4..6])?, 1.0]),
        8 => Some([n(&t[0..2])?, n(&t[2..4])?, n(&t[4..6])?, n(&t[6..8])?]),
        _ => None,
    }
}

/// 2 つの色が、8 ビットに丸めると同じか
pub(crate) fn same_bytes(a: Color, b: Color) -> bool {
    a.map(byte) == b.map(byte)
}

/// 彩度と明るさの面の絵（左から右へ彩度 0→1、上から下へ明るさ 1→0）。w×h の RGBA
pub(crate) fn sv_pixels(hue: f32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let v = 1.0 - y as f32 / (h.max(2) - 1) as f32;
        for x in 0..w {
            let s = x as f32 / (w.max(2) - 1) as f32;
            let [r, g, b] = hsv_to_rgb(hue, s, v);
            out.extend_from_slice(&[byte(r), byte(g), byte(b), 255]);
        }
    }
    out
}

/// 色相の帯の絵（左から右へ 0→360）
pub(crate) fn hue_pixels(w: u32, h: u32) -> Vec<u8> {
    let row: Vec<u8> = (0..w)
        .flat_map(|x| {
            let [r, g, b] = hsv_to_rgb(360.0 * x as f32 / w.max(1) as f32, 1.0, 1.0);
            [byte(r), byte(g), byte(b), 255]
        })
        .collect();
    row.repeat(h as usize)
}

/// 不透明度の帯の絵（左から右へ透明→不透明）。透明は市松模様（明るい灰と白）の上に重ねて見せる
pub(crate) fn alpha_pixels(rgb: [f32; 3], w: u32, h: u32, cell: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    let cell = cell.max(1);
    for y in 0..h {
        for x in 0..w {
            let a = x as f32 / (w.max(2) - 1) as f32;
            let checker = if (x / cell + y / cell).is_multiple_of(2) { 1.0 } else { 0.8 };
            let mix = |c: f32| byte(c * a + checker * (1.0 - a));
            out.extend_from_slice(&[mix(rgb[0]), mix(rgb[1]), mix(rgb[2]), 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn hsv_and_rgb_convert_both_ways() {
        assert!(close(hsv_to_rgb(0.0, 1.0, 1.0), [1.0, 0.0, 0.0]));
        assert!(close(hsv_to_rgb(120.0, 1.0, 1.0), [0.0, 1.0, 0.0]));
        assert!(close(hsv_to_rgb(240.0, 1.0, 0.5), [0.0, 0.0, 0.5]));
        assert!(close(hsv_to_rgb(360.0, 1.0, 1.0), [1.0, 0.0, 0.0]), "360 度は 0 度");
        assert!(close(hsv_to_rgb(200.0, 0.0, 0.6), [0.6, 0.6, 0.6]), "彩度 0 は灰色");
        for rgb in [[0.2, 0.4, 0.8], [0.9, 0.1, 0.3], [0.5, 0.5, 0.1], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]] {
            let [h, s, v] = rgb_to_hsv(rgb);
            assert!(close(hsv_to_rgb(h, s, v), rgb), "{rgb:?} → {h} {s} {v}");
        }
        assert_eq!(rgb_to_hsv([0.5, 0.5, 0.5])[1], 0.0);
    }

    #[test]
    fn hex_text_reads_and_writes_colors() {
        assert_eq!(hex([0.0, 0.5, 1.0, 1.0], false), "#0080FF");
        assert_eq!(hex([1.0, 0.0, 0.0, 0.5], true), "#FF000080");
        assert_eq!(hex([1.0, 0.0, 0.0, 1.0], true), "#FF0000", "不透明なら 6 桁");
        assert_eq!(parse_hex("#0080ff"), Some([0.0, 128.0 / 255.0, 1.0, 1.0]));
        assert_eq!(parse_hex("f00"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_hex(" #FF000080 ").map(|c| hex(c, true)), Some("#FF000080".to_string()));
        for bad in ["", "#", "#12345", "#GG0000", "12 34 56", "#1234567890"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        assert!(same_bytes([0.4, 0.4, 0.4, 1.0], [0.401, 0.399, 0.4, 1.0]));
        assert!(!same_bytes([0.4, 0.4, 0.4, 1.0], [0.41, 0.4, 0.4, 1.0]));
    }

    #[test]
    fn gradient_pictures_have_the_right_corners() {
        let p = sv_pixels(0.0, 4, 3);
        assert_eq!(&p[0..4], &[255, 255, 255, 255], "左上は白");
        assert_eq!(&p[12..16], &[255, 0, 0, 255], "右上は色相そのもの");
        assert_eq!(&p[p.len() - 4..], &[0, 0, 0, 255], "下は黒");
        let hue = hue_pixels(6, 1);
        assert_eq!(&hue[0..4], &[255, 0, 0, 255]);
        assert_eq!(&hue[8..12], &[0, 255, 0, 255], "3 分の 1 で緑");
        let a = alpha_pixels([0.0, 0.0, 0.0], 3, 1, 8);
        assert_eq!(&a[0..4], &[255, 255, 255, 255], "左端は透明（白い市松の上）");
        assert_eq!(&a[8..12], &[0, 0, 0, 255], "右端は不透明");
    }
}
