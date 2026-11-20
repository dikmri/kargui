//! 文字。DirectWrite で並べ（行の折り返し・代わりのフォント）、形（ラスタライズ）も DirectWrite で作る。
//! 作った形は 1 枚の表（アトラス）に詰め、GPU の道と CPU の道の両方で使う。
//! 段階 0 の試作なので、文字の位置は整数の画素にそろえる（半端な位置の形は作らない）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::time::Instant;

use taskcore::log;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::{BOOL, ComObject, IUnknown, Interface, Ref, Result, implement, w};

/// 文字の形の表の一辺（画素）
pub const ATLAS_SIZE: u32 = 1024;

/// 文字の形の表。棚に左から詰めていく。
pub struct Atlas {
    pub pixels: Vec<u8>,
    pub size: u32,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    /// GPU に送り直す行の範囲（上端, 下端）
    pub dirty: Option<(u32, u32)>,
}

impl Atlas {
    fn new(size: u32) -> Self {
        Self { pixels: vec![0; (size * size) as usize], size, shelf_x: 0, shelf_y: 0, shelf_h: 0, dirty: None }
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
pub struct Slot {
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
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub slot: Slot,
}

/// 並べ終えた文字列
#[derive(Debug, Default)]
pub struct Laid {
    pub glyphs: Vec<Placed>,
    pub width: f32,
    pub height: f32,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
struct GlyphKey {
    face: usize,
    index: u16,
    size: u32,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct LayoutKey {
    text: String,
    size: u32,
    bold: bool,
}

struct RunGlyph {
    face: IDWriteFontFace,
    size: f32,
    index: u16,
    x: f32,
    y: f32,
}

/// DirectWrite が並べた結果を受け取る係。描かずに、形の番号と位置だけを集める。
#[implement(IDWriteTextRenderer)]
struct Collector {
    runs: RefCell<Vec<RunGlyph>>,
}

impl IDWritePixelSnapping_Impl for Collector_Impl {
    fn IsPixelSnappingDisabled(&self, _context: *const c_void) -> Result<BOOL> {
        Ok(false.into())
    }
    fn GetCurrentTransform(&self, _context: *const c_void, transform: *mut DWRITE_MATRIX) -> Result<()> {
        unsafe { *transform = DWRITE_MATRIX { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 } };
        Ok(())
    }
    fn GetPixelsPerDip(&self, _context: *const c_void) -> Result<f32> {
        Ok(1.0)
    }
}

impl IDWriteTextRenderer_Impl for Collector_Impl {
    fn DrawGlyphRun(
        &self,
        _context: *const c_void,
        origin_x: f32,
        origin_y: f32,
        _mode: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        _description: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _effect: Ref<IUnknown>,
    ) -> Result<()> {
        let run = unsafe { &*run };
        let Some(face) = run.fontFace.as_ref() else { return Ok(()) };
        let n = run.glyphCount as usize;
        if n == 0 {
            return Ok(());
        }
        let indices = unsafe { std::slice::from_raw_parts(run.glyphIndices, n) };
        let advances = (!run.glyphAdvances.is_null()).then(|| unsafe { std::slice::from_raw_parts(run.glyphAdvances, n) });
        let offsets = (!run.glyphOffsets.is_null()).then(|| unsafe { std::slice::from_raw_parts(run.glyphOffsets, n) });
        let rtl = run.bidiLevel & 1 == 1;
        let mut pen = origin_x;
        let mut out = self.runs.borrow_mut();
        for i in 0..n {
            let advance = advances.map_or(0.0, |a| a[i]);
            if rtl {
                pen -= advance;
            }
            let (ox, oy) = offsets.map_or((0.0, 0.0), |o| (o[i].advanceOffset, o[i].ascenderOffset));
            out.push(RunGlyph { face: face.clone(), size: run.fontEmSize, index: indices[i], x: pen + ox, y: origin_y - oy });
            if !rtl {
                pen += advance;
            }
        }
        Ok(())
    }
    fn DrawUnderline(&self, _: *const c_void, _: f32, _: f32, _: *const DWRITE_UNDERLINE, _: Ref<IUnknown>) -> Result<()> {
        Ok(())
    }
    fn DrawStrikethrough(&self, _: *const c_void, _: f32, _: f32, _: *const DWRITE_STRIKETHROUGH, _: Ref<IUnknown>) -> Result<()> {
        Ok(())
    }
    fn DrawInlineObject(&self, _: *const c_void, _: f32, _: f32, _: Ref<IDWriteInlineObject>, _: BOOL, _: BOOL, _: Ref<IUnknown>) -> Result<()> {
        Ok(())
    }
}

pub struct Text {
    factory: IDWriteFactory2,
    formats: HashMap<(u32, bool), IDWriteTextFormat>,
    glyphs: HashMap<GlyphKey, Option<Slot>>,
    /// 形の鍵にフォントのポインタを使うので、使ったフォントを持ち続ける
    faces: Vec<IDWriteFontFace>,
    layouts: HashMap<LayoutKey, Rc<Laid>>,
    pub atlas: Atlas,
    collector: ComObject<Collector>,
    renderer: IDWriteTextRenderer,
    /// 並べ直した回数と、形を作った回数（記録用）
    pub layouts_made: u64,
    pub glyphs_made: u64,
}

/// 並べた結果を覚えておく数の上限。超えたら全部忘れる（試作なので単純にする）
const LAYOUT_CACHE_LIMIT: usize = 2048;

impl Text {
    pub fn new() -> Result<Self> {
        let t = Instant::now();
        let factory: IDWriteFactory2 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let collector = ComObject::new(Collector { runs: RefCell::new(Vec::new()) });
        let renderer: IDWriteTextRenderer = collector.to_interface();
        log::event("text.init", "dwrite factory2", &format!("ms={:.1}", t.elapsed().as_secs_f64() * 1000.0));
        Ok(Self {
            factory,
            formats: HashMap::new(),
            glyphs: HashMap::new(),
            faces: Vec::new(),
            layouts: HashMap::new(),
            atlas: Atlas::new(ATLAS_SIZE),
            collector,
            renderer,
            layouts_made: 0,
            glyphs_made: 0,
        })
    }

    /// 大きさが変わったとき（窓の拡大率の変更）に、作った物を忘れる
    pub fn reset(&mut self) {
        self.formats.clear();
        self.glyphs.clear();
        self.faces.clear();
        self.layouts.clear();
        self.atlas = Atlas::new(ATLAS_SIZE);
        self.atlas.dirty = Some((0, ATLAS_SIZE));
    }

    fn format(&mut self, size: f32, bold: bool) -> Result<IDWriteTextFormat> {
        let key = ((size * 64.0) as u32, bold);
        if let Some(f) = self.formats.get(&key) {
            return Ok(f.clone());
        }
        let weight = if bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
        let format = unsafe {
            self.factory.CreateTextFormat(
                w!("Yu Gothic UI"),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("ja-jp"),
            )?
        };
        unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)? };
        self.formats.insert(key, format.clone());
        Ok(format)
    }

    /// 1 行の文字列を並べる。size は画素。結果は覚えておき、同じ文字列には同じ物を返す
    pub fn layout(&mut self, s: &str, size: f32, bold: bool) -> Rc<Laid> {
        let key = LayoutKey { text: s.to_string(), size: (size * 64.0) as u32, bold };
        if let Some(l) = self.layouts.get(&key) {
            return l.clone();
        }
        if self.layouts.len() >= LAYOUT_CACHE_LIMIT {
            self.layouts.clear();
        }
        let laid = match self.make_layout(s, size, bold) {
            Ok(l) => Rc::new(l),
            Err(e) => {
                log::event("text.layout", &format!("text={s} size={size}"), &format!("error={e:?}"));
                Rc::new(Laid::default())
            }
        };
        self.layouts.insert(key, laid.clone());
        laid
    }

    fn make_layout(&mut self, s: &str, size: f32, bold: bool) -> Result<Laid> {
        self.layouts_made += 1;
        let format = self.format(size, bold)?;
        let wide: Vec<u16> = s.encode_utf16().collect();
        let layout = unsafe { self.factory.CreateTextLayout(&wide, &format, 100_000.0, 10_000.0)? };
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut m)? };
        self.collector.runs.borrow_mut().clear();
        unsafe { layout.Draw(None, &self.renderer, 0.0, 0.0)? };
        let runs = std::mem::take(&mut *self.collector.runs.borrow_mut());
        let mut glyphs = Vec::with_capacity(runs.len());
        for g in runs {
            if let Some(slot) = self.glyph(&g.face, g.index, g.size) {
                glyphs.push(Placed { x: g.x, y: g.y, slot });
            }
        }
        Ok(Laid { glyphs, width: m.widthIncludingTrailingWhitespace, height: m.height })
    }

    fn glyph(&mut self, face: &IDWriteFontFace, index: u16, size: f32) -> Option<Slot> {
        let key = GlyphKey { face: face.as_raw() as usize, index, size: (size * 64.0) as u32 };
        if let Some(s) = self.glyphs.get(&key) {
            return *s;
        }
        if !self.faces.iter().any(|f| f.as_raw() == face.as_raw()) {
            self.faces.push(face.clone());
        }
        let slot = self.rasterize(face, index, size).unwrap_or_else(|e| {
            log::event("text.raster", &format!("glyph={index} size={size}"), &format!("error={e:?}"));
            None
        });
        self.glyphs.insert(key, slot);
        slot
    }

    fn rasterize(&mut self, face: &IDWriteFontFace, index: u16, size: f32) -> Result<Option<Slot>> {
        self.glyphs_made += 1;
        let advance = 0.0f32;
        let offset = DWRITE_GLYPH_OFFSET::default();
        let mut run = DWRITE_GLYPH_RUN {
            fontFace: ManuallyDrop::new(Some(face.clone())),
            fontEmSize: size,
            glyphCount: 1,
            glyphIndices: &index,
            glyphAdvances: &advance,
            glyphOffsets: &offset,
            isSideways: false.into(),
            bidiLevel: 0,
        };
        let analysis = unsafe {
            self.factory.CreateGlyphRunAnalysis(
                &run,
                None,
                DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_GRID_FIT_MODE_DEFAULT,
                DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                0.0,
                0.0,
            )
        };
        unsafe { ManuallyDrop::drop(&mut run.fontFace) };
        let analysis = analysis?;
        let b = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_ALIASED_1x1)? };
        let (w, h) = ((b.right - b.left) as u32, (b.bottom - b.top) as u32);
        if w == 0 || h == 0 {
            return Ok(None);
        }
        let mut buf = vec![0u8; (w * h) as usize];
        unsafe { analysis.CreateAlphaTexture(DWRITE_TEXTURE_ALIASED_1x1, &b, &mut buf)? };
        let Some((x, y)) = self.atlas.alloc(w, h) else {
            log::event("text.atlas", &format!("w={w} h={h}"), "full");
            return Ok(None);
        };
        self.atlas.write(x, y, w, h, &buf);
        Ok(Some(Slot { x: x as u16, y: y as u16, w: w as u16, h: h as u16, left: b.left as i16, top: b.top as i16 }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_packs_on_shelves_and_tracks_dirty_rows() {
        let mut a = Atlas::new(16);
        assert_eq!(a.alloc(7, 3), Some((0, 0)));
        assert_eq!(a.alloc(7, 5), Some((8, 0)));
        // 棚の残りが足りないので次の棚へ。棚の高さは一番高い形 + 1
        assert_eq!(a.alloc(4, 2), Some((0, 6)));
        // 大きすぎる物は入らない
        assert_eq!(a.alloc(16, 1), None);
        a.write(8, 0, 2, 2, &[1, 2, 3, 4]);
        assert_eq!(&a.pixels[8..10], &[1, 2]);
        assert_eq!(&a.pixels[16 + 8..16 + 10], &[3, 4]);
        a.write(0, 6, 1, 1, &[9]);
        assert_eq!(a.dirty, Some((0, 7)));
    }
}
