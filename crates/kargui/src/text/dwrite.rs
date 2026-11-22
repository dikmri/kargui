//! 文字の Windows の道。DirectWrite で並べ（行の折り返し・代わりのフォント）、形（ラスタライズ）も DirectWrite で作る。

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::rc::Rc;

use windows::Win32::Graphics::DirectWrite::*;
use windows::core::{BOOL, ComObject, HSTRING, IUnknown, Interface, Ref, Result, implement, w};

use super::{ATLAS_SIZE, Atlas, Font, FontId, FontTable, LayoutCache, LayoutKey, Line, Placed, Run, Slot, TextStyle, add_trailing_line};
use crate::log;

/// 並べ終えた文字列
pub(crate) struct Laid {
    pub glyphs: Vec<Placed>,
    pub width: f32,
    pub height: f32,
    /// 行（1 行の文字列も 1 行。改行で終わる文字列は最後に空の行）
    pub lines: Vec<Line>,
    /// 当たり判定のために DirectWrite の並べた結果を持つ
    layout: Option<IDWriteTextLayout>,
}

impl Laid {
    fn empty(height: f32) -> Self {
        let lines = vec![Line { start: 0, end: 0, top: 0.0, height, width: 0.0, baseline: height * 0.8 }];
        Self { glyphs: Vec::new(), width: 0.0, height, lines, layout: None }
    }

    /// UTF-16 で index 文字目の前のカーソルの位置（x, 行の上端, 行の高さ）
    pub fn caret(&self, index: usize) -> (f32, f32, f32) {
        let Some(l) = &self.layout else { return (0.0, 0.0, self.height) };
        let (mut x, mut y) = (0.0f32, 0.0f32);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        if unsafe { l.HitTestTextPosition(index as u32, false, &mut x, &mut y, &mut m) }.is_err() {
            return (self.width, 0.0, self.height);
        }
        (x, m.top, m.height)
    }

    /// 点 (x, y) に一番近い文字の境目（UTF-16 の位置）
    pub fn index_at_point(&self, x: f32, y: f32) -> usize {
        let Some(l) = &self.layout else { return 0 };
        let mut trailing = BOOL::default();
        let mut inside = BOOL::default();
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        if unsafe { l.HitTestPoint(x, y, &mut trailing, &mut inside, &mut m) }.is_err() {
            return 0;
        }
        (m.textPosition + if trailing.as_bool() { m.length } else { 0 }) as usize
    }

    /// UTF-16 で index 文字目の前の位置（並べた範囲の左からの x）
    pub fn x_of(&self, index: usize) -> f32 {
        let Some(l) = &self.layout else { return 0.0 };
        let (mut x, mut y) = (0.0f32, 0.0f32);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        if unsafe { l.HitTestTextPosition(index as u32, false, &mut x, &mut y, &mut m) }.is_err() {
            return self.width;
        }
        x
    }

    /// 並べた範囲の左から x の位置に一番近い文字の境目（UTF-16 の位置）
    pub fn index_at(&self, x: f32) -> usize {
        let Some(l) = &self.layout else { return 0 };
        let mut trailing = BOOL::default();
        let mut inside = BOOL::default();
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        if unsafe { l.HitTestPoint(x, self.height * 0.5, &mut trailing, &mut inside, &mut m) }.is_err() {
            return 0;
        }
        (m.textPosition + if trailing.as_bool() { m.length } else { 0 }) as usize
    }
}

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
struct GlyphKey {
    face: usize,
    index: u16,
    size: u32,
}

struct RunGlyph {
    face: IDWriteFontFace,
    size: f32,
    index: u16,
    x: f32,
    y: f32,
    /// どの文字の形か（UTF-16 の位置）
    pos: u32,
}

/// 並びの中の形ごとの、文字の位置（UTF-16）。cluster_map[j] は j 文字目の形の番号なので、形ごとに最初に出てきた
/// 文字の位置を入れる。文字の無い形（1 文字が何個かの形になる時の 2 つ目から）は、前の形の位置を引き継ぐ
fn glyph_positions(text_position: u32, cluster_map: &[u16], glyphs: usize) -> Vec<u32> {
    let mut pos: Vec<Option<u32>> = vec![None; glyphs];
    for (j, &g) in cluster_map.iter().enumerate() {
        if let Some(p) = pos.get_mut(g as usize).filter(|p| p.is_none()) {
            *p = Some(text_position + j as u32);
        }
    }
    let mut last = text_position;
    pos.into_iter()
        .map(|p| {
            last = p.unwrap_or(last);
            last
        })
        .collect()
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
        description: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _effect: Ref<IUnknown>,
    ) -> Result<()> {
        let run = unsafe { &*run };
        let Some(face) = run.fontFace.as_ref() else { return Ok(()) };
        let n = run.glyphCount as usize;
        if n == 0 {
            return Ok(());
        }
        let positions = match unsafe { description.as_ref() } {
            Some(d) if !d.clusterMap.is_null() => {
                let map = unsafe { std::slice::from_raw_parts(d.clusterMap, d.stringLength as usize) };
                glyph_positions(d.textPosition, map, n)
            }
            Some(d) => vec![d.textPosition; n],
            None => vec![0; n],
        };
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
            out.push(RunGlyph { face: face.clone(), size: run.fontEmSize, index: indices[i], x: pen + ox, y: origin_y - oy, pos: positions[i] });
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

pub(crate) struct Text {
    factory: IDWriteFactory2,
    formats: HashMap<(u32, bool, u16), IDWriteTextFormat>,
    /// 使った書体の一覧（部品と書式は番号で持つ）
    pub fonts: FontTable,
    /// 書体の番号ごとに決めた、ファミリーの名前と探す一覧（None は OS の一覧）
    families: HashMap<u16, (HSTRING, Option<IDWriteFontCollection>)>,
    /// OS に入っている書体の一覧（UI の書体だけを使うアプリは読まない）
    system: Option<IDWriteFontCollection>,
    /// アプリが渡した書体のファイルの一覧（Options::fonts）
    custom: Option<IDWriteFontCollection>,
    glyphs: HashMap<GlyphKey, Option<Slot>>,
    /// 形の鍵にフォントのポインタを使うので、使ったフォントを持ち続ける
    faces: Vec<IDWriteFontFace>,
    layouts: LayoutCache<Laid>,
    pub atlas: Atlas,
    collector: ComObject<Collector>,
    renderer: IDWriteTextRenderer,
    pub layouts_made: u64,
    pub glyphs_made: u64,
}

/// UI の文字に使うフォント
const UI_FONT: &str = "Yu Gothic UI";

/// 等幅の書体の候補（入っている最初の物を使う）。BIZ UD ゴシックは、日本語の幅が英字のちょうど 2 つ分
const MONOSPACE: [&str; 3] = ["BIZ UDGothic", "Consolas", "MS Gothic"];

/// 一覧にそのファミリーがあるか
fn has_family(collection: &IDWriteFontCollection, name: &HSTRING) -> bool {
    let (mut index, mut exists) = (0u32, BOOL::default());
    unsafe { collection.FindFamilyName(name, &mut index, &mut exists) }.is_ok() && exists.as_bool()
}

impl Text {
    pub fn new() -> Result<Self> {
        let factory: IDWriteFactory2 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let collector = ComObject::new(Collector { runs: RefCell::new(Vec::new()) });
        let renderer: IDWriteTextRenderer = collector.to_interface();
        Ok(Self {
            factory,
            formats: HashMap::new(),
            fonts: FontTable::new(),
            families: HashMap::new(),
            system: None,
            custom: None,
            glyphs: HashMap::new(),
            faces: Vec::new(),
            layouts: LayoutCache::new(),
            atlas: Atlas::new(ATLAS_SIZE),
            collector,
            renderer,
            layouts_made: 0,
            glyphs_made: 0,
        })
    }

    /// 拡大率が変わったときなどに、作った物を全部忘れる
    pub fn reset(&mut self) {
        self.formats.clear();
        self.glyphs.clear();
        self.faces.clear();
        self.layouts.clear();
        self.atlas = self.atlas.renewed();
    }

    /// アプリが渡した書体のファイル（Options::fonts）を読み込み、名前で選べるようにする。読めたファミリーの数を返す
    pub fn load_fonts(&mut self, files: &[Cow<'static, [u8]>]) -> usize {
        if files.is_empty() {
            return 0;
        }
        let bytes: usize = files.iter().map(|f| f.len()).sum();
        match self.font_collection(files) {
            Ok((collection, families)) => {
                self.custom = Some(collection);
                // 名前を決め直す（前に OS の一覧で見つからなかった名前も、読み込んだ書体で見つかる）
                self.families.clear();
                self.formats.clear();
                self.layouts.clear();
                log::event("font.load", &format!("files={} bytes={bytes}", files.len()), &format!("families={families}"));
                families
            }
            Err(e) => {
                log::event("font.load", &format!("files={} bytes={bytes}", files.len()), &format!("error={e:?}"));
                0
            }
        }
    }

    /// 書体のファイルの中身から、名前で探せる一覧を作る（中身は DirectWrite が写して持つ）
    fn font_collection(&self, files: &[Cow<'static, [u8]>]) -> Result<(IDWriteFontCollection, usize)> {
        let f5: IDWriteFactory5 = self.factory.cast()?;
        let loader = unsafe { f5.CreateInMemoryFontFileLoader()? };
        unsafe { f5.RegisterFontFileLoader(&loader)? };
        let builder = unsafe { f5.CreateFontSetBuilder()? };
        for data in files {
            let file = unsafe { loader.CreateInMemoryFontFileReference(&f5, data.as_ptr().cast(), data.len() as u32, None)? };
            unsafe { builder.AddFontFile(&file)? };
        }
        let set = unsafe { builder.CreateFontSet()? };
        let collection = unsafe { f5.CreateFontCollectionFromFontSet(&set)? };
        let families = unsafe { collection.GetFontFamilyCount() } as usize;
        Ok((collection.cast()?, families))
    }

    /// 試験用: 書体から決めたファミリーの名前と、アプリが渡したファイルから見つけたか
    #[cfg(test)]
    pub fn resolved(&mut self, font: &Font) -> (String, bool) {
        let id = self.fonts.id(font);
        let (name, from_app) = self.family(id);
        (name.to_string(), from_app.is_some())
    }

    /// OS に入っている書体の一覧（初めて要る時に読む）
    fn system_fonts(&mut self) -> Option<IDWriteFontCollection> {
        if self.system.is_none() {
            let mut c = None;
            if unsafe { self.factory.GetSystemFontCollection(&mut c, false) }.is_ok() {
                self.system = c;
            }
        }
        self.system.clone()
    }

    /// 書体の番号から、DirectWrite に渡すファミリーの名前と探す一覧を決める（決めた物は覚える）
    fn family(&mut self, id: FontId) -> (HSTRING, Option<IDWriteFontCollection>) {
        if let Some(f) = self.families.get(&id.0) {
            return f.clone();
        }
        let ui = (HSTRING::from(UI_FONT), None);
        let font = self.fonts.get(id).clone();
        let found = match &font {
            Font::Ui => ui,
            Font::Monospace => {
                let system = self.system_fonts();
                let first = MONOSPACE.iter().map(|n| HSTRING::from(*n)).find(|n| system.as_ref().is_some_and(|c| has_family(c, n)));
                match first {
                    Some(name) => (name, None),
                    None => {
                        log::event("font.missing", "family=monospace", "fallback=ui");
                        ui
                    }
                }
            }
            Font::Family(name) => {
                let h = HSTRING::from(name.as_str());
                if let Some(c) = self.custom.as_ref().filter(|c| has_family(c, &h)) {
                    (h, Some(c.clone()))
                } else if self.system_fonts().is_some_and(|c| has_family(&c, &h)) {
                    (h, None)
                } else {
                    log::event("font.missing", &format!("family={name}"), "fallback=ui");
                    ui
                }
            }
        };
        log::event("font.resolve", &format!("font={font:?}"), &format!("family={} from_app={}", found.0, found.1.is_some()));
        self.families.insert(id.0, found.clone());
        found
    }

    fn format(&mut self, size: f32, bold: bool, font: FontId) -> Result<IDWriteTextFormat> {
        let key = ((size * 64.0) as u32, bold, font.0);
        if let Some(f) = self.formats.get(&key) {
            return Ok(f.clone());
        }
        let weight = if bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
        let (family, collection) = self.family(font);
        let format = unsafe {
            self.factory.CreateTextFormat(&family, collection.as_ref(), weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("ja-jp"))?
        };
        // 行の間が UI の書体より狭い書体（BIZ UD ゴシックは行の高さが文字の大きさと同じ）は、UI の書体の行の高さに広げ、
        // 字は広げた行の真ん中に置く。書体を変えても、行の間と入力欄の高さが変わらないように
        if font != FontId(0) {
            let ui = self.format(size, bold, FontId(0))?;
            let ((ui_h, _), (h, baseline)) = (self.natural_line(&ui)?, self.natural_line(&format)?);
            if h + 0.01 < ui_h {
                unsafe { format.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, ui_h, baseline + (ui_h - h) / 2.0)? };
                log::event("font.line_height", &format!("family={family} size={size}"), &format!("natural={h:.2} ui={ui_h:.2}"));
            }
        }
        self.formats.insert(key, format.clone());
        Ok(format)
    }

    /// その書式の 1 行の、書体どおりの高さとベースラインの位置。英字と日本語で測って低い方を使う
    /// （書体に無い字は代わりの書体で並び、その行は代わりの書体の高さになる。英字だけの Consolas を「あ」で測ると、
    /// 代わりの書体の高い行を測ってしまい、英字の行が詰まったままになった。2026-11-22 の CI）
    fn natural_line(&self, format: &IDWriteTextFormat) -> Result<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        for sample in ["Ag", "あ"] {
            let wide: Vec<u16> = sample.encode_utf16().collect();
            let layout = unsafe { self.factory.CreateTextLayout(&wide, format, 1.0e6, 1.0e6)? };
            let mut lm = [DWRITE_LINE_METRICS::default()];
            let mut count = 0u32;
            unsafe { layout.GetLineMetrics(Some(&mut lm), &mut count)? };
            if best.is_none_or(|(h, _)| lm[0].height < h) {
                best = Some((lm[0].height, lm[0].baseline));
            }
        }
        Ok(best.unwrap_or((0.0, 0.0)))
    }

    /// 文字列を並べる。max_width を渡すとその幅で折り返す。結果は覚えておき、同じ物には同じ結果を返す
    pub fn layout(&mut self, s: &str, style: TextStyle, max_width: Option<f32>) -> Rc<Laid> {
        self.layout_runs(s, style, &[], max_width)
    }

    /// 書式の範囲を混ぜて並べる（書式付きの文字）。範囲の外は style で並べる
    pub fn layout_runs(&mut self, s: &str, style: TextStyle, runs: &[Run], max_width: Option<f32>) -> Rc<Laid> {
        let key = LayoutKey::with_runs(s, style, runs, max_width);
        if let Some(l) = self.layouts.get(&key) {
            return l;
        }
        let laid = match self.make_layout(s, style, runs, max_width) {
            Ok(l) => Rc::new(l),
            Err(e) => {
                log::event("text.layout", &format!("text={s} size={} runs={}", style.size, runs.len()), &format!("error={e:?}"));
                Rc::new(Laid::empty(style.size * 1.4))
            }
        };
        self.layouts.insert(key, laid.clone());
        laid
    }

    /// 範囲ごとの書式を設定する（太字・斜体・大きさ・書体）
    fn apply_runs(&mut self, layout: &IDWriteTextLayout, style: TextStyle, runs: &[Run]) -> Result<()> {
        for r in runs.iter().filter(|r| r.end > r.start) {
            let range = DWRITE_TEXT_RANGE { startPosition: r.start as u32, length: (r.end - r.start) as u32 };
            let weight = if r.style.bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
            let slant = if r.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL };
            unsafe {
                layout.SetFontWeight(weight, range)?;
                layout.SetFontStyle(slant, range)?;
                layout.SetFontSize(r.style.size, range)?;
            }
            if r.style.font != style.font {
                let (family, collection) = self.family(r.style.font);
                let collection = collection.or_else(|| self.system_fonts());
                unsafe {
                    layout.SetFontFamilyName(&family, range)?;
                    if let Some(c) = &collection {
                        layout.SetFontCollection(c, range)?;
                    }
                }
            }
        }
        // 基の書式が行の間を決めている（行の詰まった書体を広げた）時、大きい範囲があると行からはみ出すので、書体どおりの行の間に戻す
        if runs.iter().any(|r| r.style.size > style.size + 0.01) {
            let (mut method, mut spacing, mut baseline) = (DWRITE_LINE_SPACING_METHOD_DEFAULT, 0.0f32, 0.0f32);
            unsafe { layout.GetLineSpacing(&mut method, &mut spacing, &mut baseline)? };
            if method != DWRITE_LINE_SPACING_METHOD_DEFAULT {
                unsafe { layout.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_DEFAULT, 0.0, 0.0)? };
            }
        }
        Ok(())
    }

    fn make_layout(&mut self, s: &str, style: TextStyle, runs: &[Run], max_width: Option<f32>) -> Result<Laid> {
        self.layouts_made += 1;
        let format = self.format(style.size, style.bold, style.font)?;
        let wide: Vec<u16> = s.encode_utf16().collect();
        let layout = unsafe { self.factory.CreateTextLayout(&wide, &format, max_width.unwrap_or(1.0e6).max(1.0), 1.0e6)? };
        unsafe {
            layout.SetWordWrapping(if max_width.is_some() { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP })?;
        }
        self.apply_runs(&layout, style, runs)?;
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut m)? };
        self.collector.runs.borrow_mut().clear();
        unsafe { layout.Draw(None, &self.renderer, 0.0, 0.0)? };
        let runs = std::mem::take(&mut *self.collector.runs.borrow_mut());
        let mut glyphs = Vec::with_capacity(runs.len());
        for g in runs {
            if let Some(slot) = self.glyph(&g.face, g.index, g.size) {
                glyphs.push(Placed { x: g.x, y: g.y, slot, pos: g.pos });
            }
        }
        let lines = line_metrics(&layout, s)?;
        Ok(Laid { glyphs, width: m.widthIncludingTrailingWhitespace, height: m.height, lines, layout: Some(layout) })
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
            // 表がいっぱいになったら作り直す（次のフレームで形を作り直す）
            log::event("text.atlas", &format!("w={w} h={h}"), "full; reset");
            self.reset();
            return Ok(None);
        };
        self.atlas.write(x, y, w, h, &buf);
        Ok(Some(Slot { x: x as u16, y: y as u16, w: w as u16, h: h as u16, left: b.left as i16, top: b.top as i16 }))
    }
}

/// DirectWrite の行の情報から行の一覧を作る。行の幅は、行の最後の文字の後ろの位置
fn line_metrics(layout: &IDWriteTextLayout, s: &str) -> Result<Vec<Line>> {
    let mut count = 0u32;
    // 1 回目は数を聞くだけ（足りないという失敗が返る）
    let _ = unsafe { layout.GetLineMetrics(None, &mut count) };
    let mut lm = vec![DWRITE_LINE_METRICS::default(); count as usize];
    unsafe { layout.GetLineMetrics(Some(&mut lm), &mut count)? };
    let (mut start, mut top) = (0usize, 0.0f32);
    let mut lines = Vec::with_capacity(lm.len());
    for m in &lm {
        let end = start + m.length as usize - m.newlineLength as usize;
        let width = if end > start {
            let (mut x, mut y) = (0.0f32, 0.0f32);
            let mut hm = DWRITE_HIT_TEST_METRICS::default();
            match unsafe { layout.HitTestTextPosition(end as u32 - 1, true, &mut x, &mut y, &mut hm) } {
                Ok(()) => x,
                Err(_) => 0.0,
            }
        } else {
            0.0
        };
        lines.push(Line { start, end, top, height: m.height, width, baseline: top + m.baseline });
        start += m.length as usize;
        top += m.height;
    }
    add_trailing_line(s, &mut lines);
    Ok(lines)
}
