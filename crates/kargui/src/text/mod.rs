//! 文字。並べる（行の折り返し・代わりのフォント・当たり判定）のと形を作る（ラスタライズ）のは、OS ごとの道に任せる。
//! 作った形は 1 枚の表（アトラス）に詰め、GPU の道と CPU の道の両方で使う。文字の位置は整数の画素にそろえる。
//!
//! - Windows: DirectWrite（dwrite.rs）。段階 0 の比べで、Windows では parley を足しても速さと日本語の折り返しが
//!   変わらず、依存の crate だけが 40 個増えると分かったので、Windows では DirectWrite だけを使う
//! - それ以外の OS と、Windows で `portable-text` の時: parley で並べ、swash で形を作る（portable.rs）
//!
//! どちらの道も、同じ名前の `Text`（作る係）と `Laid`（並べた結果）を出す。

use std::collections::HashMap;

#[cfg(all(windows, not(feature = "portable-text")))]
mod dwrite;
#[cfg(all(windows, not(feature = "portable-text")))]
pub(crate) use dwrite::{Laid, Text};

#[cfg(any(not(windows), feature = "portable-text"))]
mod portable;
#[cfg(any(not(windows), feature = "portable-text"))]
pub(crate) use portable::{Laid, Text};

/// 文字の形の表の一辺（画素）
pub(crate) const ATLAS_SIZE: u32 = 1024;

/// 並べた結果を覚えておく数の上限。超えたら全部忘れる（形の表は残す）
const LAYOUT_CACHE_LIMIT: usize = 4096;

/// 文字の形の表。棚に左から詰めていく。
pub(crate) struct Atlas {
    pub pixels: Vec<u8>,
    pub size: u32,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    /// GPU に送り直す行の範囲（上端, 下端）
    pub dirty: Option<(u32, u32)>,
    /// 中身を作り直した回数（GPU の道が表を全部送り直すかどうかの判断に使う）
    pub generation: u64,
}

impl Atlas {
    fn new(size: u32) -> Self {
        Self { pixels: vec![0; (size * size) as usize], size, shelf_x: 0, shelf_y: 0, shelf_h: 0, dirty: None, generation: 0 }
    }

    /// 中身を空にした新しい表（作り直した回数は続きから数え、GPU には全部送り直させる）
    fn renewed(&self) -> Self {
        let mut a = Atlas::new(self.size);
        a.generation = self.generation + 1;
        a.dirty = Some((0, self.size));
        a
    }

    /// w×h の場所を取る。形どうしがにじまないよう 1 画素あける
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + 1 > self.size || h + 1 > self.size {
            return None;
        }
        if self.shelf_x + w + 1 > self.size {
            self.shelf_y += self.shelf_h;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }
        if self.shelf_y + h + 1 > self.size {
            return None;
        }
        let at = (self.shelf_x, self.shelf_y);
        self.shelf_x += w + 1;
        self.shelf_h = self.shelf_h.max(h + 1);
        Some(at)
    }

    fn write(&mut self, x: u32, y: u32, w: u32, h: u32, src: &[u8]) {
        for row in 0..h {
            let d = ((y + row) * self.size + x) as usize;
            let s = (row * w) as usize;
            self.pixels[d..d + w as usize].copy_from_slice(&src[s..s + w as usize]);
        }
        let (top, bottom) = self.dirty.unwrap_or((y, y + h));
        self.dirty = Some((top.min(y), bottom.max(y + h)));
    }
}

/// 表の中の 1 つの形
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// ペンの位置（ベースライン）から形の左上までのずれ
    pub left: i16,
    pub top: i16,
}

/// 並べた文字 1 つ。x, y はペンの位置（ベースライン上）で、並べた範囲の左上が原点
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    pub x: f32,
    pub y: f32,
    pub slot: Slot,
}

/// 文字の書式
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextStyle {
    /// 大きさ（物理ピクセル）
    pub size: f32,
    pub bold: bool,
}

/// 並べた結果を覚えるための鍵
#[derive(Hash, Eq, PartialEq, Clone)]
struct LayoutKey {
    text: String,
    size: u32,
    bold: bool,
    max_width: u32,
}

impl LayoutKey {
    fn new(s: &str, style: TextStyle, max_width: Option<f32>) -> Self {
        Self {
            text: s.to_string(),
            size: (style.size * 64.0) as u32,
            bold: style.bold,
            max_width: max_width.map_or(u32::MAX, |w| w.max(0.0).round() as u32),
        }
    }
}

/// 並べた結果を覚えておく表（同じ文字列・書式・幅には同じ結果を返す）
struct LayoutCache<L>(HashMap<LayoutKey, std::rc::Rc<L>>);

impl<L> LayoutCache<L> {
    fn new() -> Self {
        Self(HashMap::new())
    }

    fn get(&self, key: &LayoutKey) -> Option<std::rc::Rc<L>> {
        self.0.get(key).cloned()
    }

    fn insert(&mut self, key: LayoutKey, laid: std::rc::Rc<L>) {
        if self.0.len() >= LAYOUT_CACHE_LIMIT {
            self.0.clear();
        }
        self.0.insert(key, laid);
    }

    fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn atlas_packs_on_shelves_and_tracks_dirty_rows() {
        let mut a = Atlas::new(16);
        assert_eq!(a.alloc(7, 3), Some((0, 0)));
        assert_eq!(a.alloc(7, 5), Some((8, 0)));
        assert_eq!(a.alloc(4, 2), Some((0, 6)));
        assert_eq!(a.alloc(16, 1), None);
        a.write(8, 0, 2, 2, &[1, 2, 3, 4]);
        assert_eq!(&a.pixels[8..10], &[1, 2]);
        a.write(0, 6, 1, 1, &[9]);
        assert_eq!(a.dirty, Some((0, 7)));
        let b = a.renewed();
        assert_eq!((b.generation, b.dirty, b.pixels[8]), (1, Some((0, 16)), 0));
    }

    /// どちらの道でも同じことを確かめる（Windows 以外では、日本語のフォントが入っている必要がある）
    #[test]
    fn layout_measures_wraps_and_hit_tests_japanese() {
        let mut t = Text::new().unwrap();
        let style = TextStyle { size: 14.0, bold: false };
        let one = t.layout("資料", style, None);
        assert!(one.width > 20.0 && one.width < 40.0, "{}", one.width);
        assert!(!one.glyphs.is_empty());
        // 2 文字目の前の位置は、1 文字目の幅に近い
        let x1 = one.x_of(1);
        assert!(x1 > 8.0 && x1 < one.width, "{x1}");
        assert_eq!(one.index_at(0.0), 0);
        assert_eq!(one.index_at(one.width + 10.0), 2);
        // 同じ文字列は覚えた結果を返す
        assert!(Rc::ptr_eq(&one, &t.layout("資料", style, None)));
        // 幅を決めると折り返して高くなる
        let long = "これは日本語の折り返しを確かめるための文です。";
        let wrapped = t.layout(long, style, Some(100.0));
        let single = t.layout(long, style, None);
        assert!(wrapped.height > single.height * 2.0, "{} {}", wrapped.height, single.height);
        assert!(wrapped.width <= 100.5, "{}", wrapped.width);
    }

    /// 英字と日本語が混ざった文字列の当たり判定は、UTF-16 の位置で数える（絵文字は 2 つ分）
    #[test]
    fn hit_testing_counts_utf16_positions() {
        let mut t = Text::new().unwrap();
        let style = TextStyle { size: 14.0, bold: false };
        let laid = t.layout("ab牛乳😀c", style, None);
        // a, b, 牛, 乳 は 1 つずつ、😀 は 2 つ、c は 1 つ（全部で 7）
        let xs: Vec<f32> = (0..=7).filter(|&i| i != 5).map(|i| laid.x_of(i)).collect();
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "{xs:?}");
        assert!((laid.x_of(7) - laid.width).abs() < 1.0, "{} {}", laid.x_of(7), laid.width);
        assert_eq!(laid.index_at(laid.width + 5.0), 7);
        assert_eq!(laid.index_at(laid.x_of(2) + 1.0), 2);
    }

    /// 並べた文字の形の濃さの合計（形の表の画素の和）
    fn ink(t: &Text, laid: &Laid) -> u64 {
        let mut sum = 0u64;
        for g in &laid.glyphs {
            let s = g.slot;
            for y in 0..s.h as u32 {
                for x in 0..s.w as u32 {
                    sum += t.atlas.pixels[((s.y as u32 + y) * t.atlas.size + s.x as u32 + x) as usize] as u64;
                }
            }
        }
        sum
    }

    /// 太字は濃く描く。太字の書体が無いフォント（Linux の Takao など）では輪郭を太らせるので、
    /// 幅は変わらないことがある。幅ではなく濃さで確かめる
    #[test]
    fn bold_draws_more_ink_and_empty_text_has_height() {
        let mut t = Text::new().unwrap();
        let normal = t.layout("Settings 設定", TextStyle { size: 14.0, bold: false }, None);
        let bold = t.layout("Settings 設定", TextStyle { size: 14.0, bold: true }, None);
        let (a, b) = (ink(&t, &normal), ink(&t, &bold));
        assert!(b as f64 > a as f64 * 1.1, "普通 {a}、太字 {b}");
        assert!(bold.width >= normal.width, "{} {}", bold.width, normal.width);
        let empty = t.layout("", TextStyle { size: 14.0, bold: false }, None);
        assert_eq!(empty.width, 0.0);
        assert!(empty.height > 10.0, "{}", empty.height);
    }
}
