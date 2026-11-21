//! 文字の Windows 以外の道（Linux・macOS と、Windows で `portable-text` の時）。parley で並べ（行の折り返し・
//! 代わりのフォント・当たり判定）、swash で形を作る。フォントは OS に入っている物を fontique（parley の中）で探す。

use std::collections::HashMap;
use std::rc::Rc;

use parley::editing::Cursor;
use parley::fontique::Language;
use parley::layout::Affinity;
use parley::style::{FontFamily, FontWeight, StyleProperty};
use parley::{Alignment, AlignmentOptions, FontContext, FontData, Layout, LayoutContext, PositionedLayoutItem};
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;

use super::{ATLAS_SIZE, Atlas, LayoutCache, LayoutKey, Placed, Slot, TextStyle};
use crate::log;

/// UI の文字に使うフォント（CSS の font-family の形）。先にある物から使い、無い文字は代わりのフォントで出す
#[cfg(target_os = "macos")]
const FONTS: &str = "\"Hiragino Sans\", system-ui, sans-serif";
#[cfg(windows)]
const FONTS: &str = "\"Yu Gothic UI\", \"Segoe UI\", sans-serif";
#[cfg(not(any(windows, target_os = "macos")))]
const FONTS: &str =
    "\"Noto Sans CJK JP\", \"Noto Sans JP\", \"Source Han Sans JP\", \"IPAexGothic\", \"IPAPGothic\", \"TakaoPGothic\", \"VL PGothic\", sans-serif";

/// 並べ終えた文字列
pub(crate) struct Laid {
    pub glyphs: Vec<Placed>,
    pub width: f32,
    pub height: f32,
    /// 当たり判定のために、元の文字列と parley の並べた結果を持つ
    text: String,
    layout: Option<Layout<()>>,
}

impl Laid {
    /// UTF-16 で index 文字目の前の位置（並べた範囲の左からの x）
    pub fn x_of(&self, index: usize) -> f32 {
        let Some(l) = &self.layout else { return 0.0 };
        let byte = byte_of_utf16(&self.text, index);
        Cursor::from_byte_index(l, byte, Affinity::Downstream).geometry(l, 0.0).x0 as f32
    }

    /// 並べた範囲の左から x の位置に一番近い文字の境目（UTF-16 の位置）
    pub fn index_at(&self, x: f32) -> usize {
        let Some(l) = &self.layout else { return 0 };
        let c = Cursor::from_point(l, x, self.height * 0.5);
        utf16_of_byte(&self.text, c.index())
    }
}

/// UTF-16 の位置 → バイトの位置（サロゲートの途中なら次の文字の頭）
fn byte_of_utf16(s: &str, index16: usize) -> usize {
    let mut n = 0;
    for (b, ch) in s.char_indices() {
        if n >= index16 {
            return b;
        }
        n += ch.len_utf16();
    }
    s.len()
}

/// バイトの位置 → UTF-16 の位置
fn utf16_of_byte(s: &str, byte: usize) -> usize {
    s.char_indices().take_while(|&(b, _)| b < byte).map(|(_, ch)| ch.len_utf16()).sum()
}

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
struct GlyphKey {
    font: u64,
    index: u32,
    glyph: u32,
    size: u32,
    embolden: bool,
}

pub(crate) struct Text {
    fonts: FontContext,
    layout_cx: LayoutContext<()>,
    scale: ScaleContext,
    glyphs: HashMap<GlyphKey, Option<Slot>>,
    layouts: LayoutCache<Laid>,
    pub atlas: Atlas,
    pub layouts_made: u64,
    pub glyphs_made: u64,
}

impl Text {
    pub fn new() -> Result<Self, crate::Error> {
        Ok(Self {
            fonts: FontContext::new(),
            layout_cx: LayoutContext::new(),
            scale: ScaleContext::new(),
            glyphs: HashMap::new(),
            layouts: LayoutCache::new(),
            atlas: Atlas::new(ATLAS_SIZE),
            layouts_made: 0,
            glyphs_made: 0,
        })
    }

    /// 拡大率が変わったときなどに、作った物を全部忘れる
    pub fn reset(&mut self) {
        self.glyphs.clear();
        self.layouts.clear();
        self.atlas = self.atlas.renewed();
    }

    /// 文字列を並べる。max_width を渡すとその幅で折り返す。結果は覚えておき、同じ物には同じ結果を返す
    pub fn layout(&mut self, s: &str, style: TextStyle, max_width: Option<f32>) -> Rc<Laid> {
        let key = LayoutKey::new(s, style, max_width);
        if let Some(l) = self.layouts.get(&key) {
            return l;
        }
        let laid = Rc::new(self.make_layout(s, style, max_width));
        self.layouts.insert(key, laid.clone());
        laid
    }

    fn make_layout(&mut self, s: &str, style: TextStyle, max_width: Option<f32>) -> Laid {
        // 空の文字列は行が無く高さ 0 になるので、1 行分の高さだけを持つ（DirectWrite と同じ）
        if s.is_empty() {
            let probe = self.make_layout(" ", style, None);
            return Laid { glyphs: Vec::new(), width: 0.0, height: probe.height, text: String::new(), layout: None };
        }
        self.layouts_made += 1;
        let mut b = self.layout_cx.ranged_builder(&mut self.fonts, s, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::Source(FONTS.into())));
        b.push_default(StyleProperty::FontSize(style.size));
        b.push_default(StyleProperty::Locale(Language::parse("ja-JP").ok()));
        if style.bold {
            b.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
        }
        let mut layout: Layout<()> = b.build(s);
        layout.break_all_lines(max_width);
        layout.align(Alignment::Start, AlignmentOptions::default());
        let mut glyphs = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let r = run.run();
                let font = r.font().clone();
                let size = r.font_size();
                let coords: Vec<i16> = r.normalized_coords().iter().map(|c| c.to_bits()).collect();
                let embolden = r.synthesis().embolden();
                for g in run.positioned_glyphs() {
                    if let Some(slot) = self.glyph(&font, g.id, size, &coords, embolden) {
                        glyphs.push(Placed { x: g.x, y: g.y, slot });
                    }
                }
            }
        }
        // 幅は行末の空白も含める（入力欄のキャレットが空白の後ろへ進むように。DirectWrite と同じ）
        Laid { glyphs, width: layout.full_width(), height: layout.height(), text: s.to_string(), layout: Some(layout) }
    }

    fn glyph(&mut self, font: &FontData, glyph: u32, size: f32, coords: &[i16], embolden: bool) -> Option<Slot> {
        let key = GlyphKey { font: font.data.id(), index: font.index, glyph, size: (size * 64.0) as u32, embolden };
        if let Some(s) = self.glyphs.get(&key) {
            return *s;
        }
        let slot = self.rasterize(font, glyph, size, coords, embolden);
        self.glyphs.insert(key, slot);
        slot
    }

    fn rasterize(&mut self, font: &FontData, glyph: u32, size: f32, coords: &[i16], embolden: bool) -> Option<Slot> {
        self.glyphs_made += 1;
        let Some(font_ref) = FontRef::from_index(font.data.as_ref(), font.index as usize) else {
            log::event("text.raster", &format!("glyph={glyph} size={size}"), "error=font");
            return None;
        };
        let mut scaler = self.scale.builder(font_ref).size(size).hint(false).normalized_coords(coords).build();
        let mut render = Render::new(&[Source::Outline]);
        render.format(Format::Alpha);
        if embolden {
            // 太字の書体が無い時は、輪郭を太らせる
            render.embolden(size / 32.0);
        }
        let image = render.render(&mut scaler, glyph as u16)?;
        let (w, h) = (image.placement.width, image.placement.height);
        if w == 0 || h == 0 {
            return None;
        }
        let Some((x, y)) = self.atlas.alloc(w, h) else {
            // 表がいっぱいになったら作り直す（次のフレームで形を作り直す）
            log::event("text.atlas", &format!("w={w} h={h}"), "full; reset");
            self.reset();
            return None;
        };
        self.atlas.write(x, y, w, h, &image.data);
        // swash の top はベースラインから上向き。表の形は「ベースラインから形の左上までのずれ」（下向きが正）で持つ
        Some(Slot { x: x as u16, y: y as u16, w: w as u16, h: h as u16, left: image.placement.left as i16, top: -image.placement.top as i16 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_and_byte_positions_convert_both_ways() {
        let s = "ab牛😀c";
        // a(1) b(1) 牛(3 バイト・1) 😀(4 バイト・2) c(1)
        assert_eq!(byte_of_utf16(s, 0), 0);
        assert_eq!(byte_of_utf16(s, 2), 2);
        assert_eq!(byte_of_utf16(s, 3), 5);
        assert_eq!(byte_of_utf16(s, 4), 9, "サロゲートの途中は次の文字の頭");
        assert_eq!(byte_of_utf16(s, 5), 9);
        assert_eq!(byte_of_utf16(s, 99), s.len());
        assert_eq!(utf16_of_byte(s, 5), 3);
        assert_eq!(utf16_of_byte(s, 9), 5);
        assert_eq!(utf16_of_byte(s, s.len()), 6);
    }
}
