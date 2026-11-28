//! 文字の Windows 以外の道（Linux・macOS と、Windows で `portable-text` の時）。parley で並べ（行の折り返し・
//! 代わりのフォント・当たり判定）、swash で形を作る。フォントは OS に入っている物を fontique（parley の中）で探す。

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use parley::editing::Cursor;
use parley::fontique::{Blob, Language};
use parley::layout::Affinity;
use parley::style::{FontFamily, FontStyle, FontWeight, LineHeight, StyleProperty};
use parley::{Alignment, AlignmentOptions, FontContext, FontData, Layout, LayoutContext, PositionedLayoutItem};
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Angle, Format, Transform};

use super::{ATLAS_SIZE, Atlas, Font, FontId, FontTable, LayoutCache, LayoutKey, Line, Placed, Run, Slot, TextStyle, add_trailing_line, glyph_chunks};
use crate::log;

/// UI の文字に使うフォント（CSS の font-family の形）。先にある物から使い、無い文字は代わりのフォントで出す
#[cfg(target_os = "macos")]
const FONTS: &str = "\"Hiragino Sans\", system-ui, sans-serif";
#[cfg(windows)]
const FONTS: &str = "\"Yu Gothic UI\", \"Segoe UI\", sans-serif";
#[cfg(not(any(windows, target_os = "macos")))]
const FONTS: &str =
    "\"Noto Sans CJK JP\", \"Noto Sans JP\", \"Source Han Sans JP\", \"IPAexGothic\", \"IPAPGothic\", \"TakaoPGothic\", \"VL PGothic\", sans-serif";

/// 等幅の書体（Font::Monospace）。日本語の幅が英字のちょうど 2 つ分になる書体を先に置く
#[cfg(target_os = "macos")]
const MONOSPACE: &str = "\"Osaka-Mono\", Menlo, monospace, \"Hiragino Sans\"";
#[cfg(windows)]
const MONOSPACE: &str = "\"BIZ UDGothic\", Consolas, \"MS Gothic\", monospace";
#[cfg(not(any(windows, target_os = "macos")))]
const MONOSPACE: &str =
    "\"Noto Sans Mono CJK JP\", \"TakaoGothic\", \"IPAGothic\", \"VL Gothic\", \"DejaVu Sans Mono\", monospace";

/// 並べ終えた文字列
pub(crate) struct Laid {
    pub glyphs: Vec<Placed>,
    /// 字のまとまりごとの範囲（glyph_chunks）
    pub chunks: Vec<[f32; 4]>,
    pub width: f32,
    pub height: f32,
    /// 行（1 行の文字列も 1 行。改行で終わる文字列は最後に空の行）
    pub lines: Vec<Line>,
    /// 当たり判定のために、元の文字列と parley の並べた結果を持つ
    text: String,
    layout: Option<Layout<()>>,
    /// 聞かれた位置（UTF-16）ごとのカーソルの四角（cursor_box）
    carets: RefCell<HashMap<usize, (f32, f32, f32)>>,
}

/// 覚えておくカーソルの四角の数の上限。超えたら全部忘れる
const CARET_MEMO_LIMIT: usize = 4096;

impl Laid {
    /// UTF-16 で index 文字目の前のカーソルの位置（x, 行の上端, 行の高さ）
    pub fn caret(&self, index: usize) -> (f32, f32, f32) {
        let Some(l) = &self.layout else { return (0.0, 0.0, self.height) };
        // 最後の改行の後ろは、parley では前の行の終わりになるので、空の最後の行の左端にする
        if let Some(last) = self.lines.last().filter(|last| index >= last.start && last.start == last.end && self.text.ends_with('\n')) {
            return (0.0, last.top, last.height);
        }
        self.cursor_box(l, index)
    }

    /// UTF-16 で index 文字目の前の、parley のカーソルの四角（x, 上端, 高さ）。聞かれた位置の答えは覚える。
    /// parley はカーソルの x を、行の頭から字の幅を足して出す（Cluster::visual_offset）。200 万字の 1 行の終わりでは
    /// 1 回に約 80 ms かかり、1 行の入力欄は毎フレーム同じ位置を聞くので、何も変わらないフレームが 70 ms になった
    /// （2026-11-28 の X11 の記録）。並べた結果は変わらないので、同じ位置には同じ答えを返せる
    fn cursor_box(&self, l: &Layout<()>, index: usize) -> (f32, f32, f32) {
        if let Some(&b) = self.carets.borrow().get(&index) {
            return b;
        }
        let byte = byte_of_utf16(&self.text, index);
        let g = Cursor::from_byte_index(l, byte, Affinity::Downstream).geometry(l, 0.0);
        let b = (g.x0 as f32, g.y0 as f32, (g.y1 - g.y0) as f32);
        let mut carets = self.carets.borrow_mut();
        if carets.len() >= CARET_MEMO_LIMIT {
            carets.clear();
        }
        carets.insert(index, b);
        b
    }

    /// 点 (x, y) に一番近い文字の境目（UTF-16 の位置）
    pub fn index_at_point(&self, x: f32, y: f32) -> usize {
        let Some(l) = &self.layout else { return 0 };
        if let Some(last) = self.lines.last().filter(|last| last.start == last.end && y >= last.top && self.text.ends_with('\n')) {
            return last.start;
        }
        utf16_of_byte(&self.text, Cursor::from_point(l, x, y).index())
    }

    /// UTF-16 で index 文字目の前の位置（並べた範囲の左からの x）
    pub fn x_of(&self, index: usize) -> f32 {
        let Some(l) = &self.layout else { return 0.0 };
        self.cursor_box(l, index).0
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
    /// 斜体の書体が無い時に傾ける角度（度、ビットの形）
    skew: u32,
}

pub(crate) struct Text {
    font_cx: FontContext,
    /// 使った書体の一覧（部品と書式は番号で持つ）
    pub fonts: FontTable,
    /// 書体の番号ごとに決めた、parley に渡すファミリーの並び（CSS の font-family の形）
    families: HashMap<u16, String>,
    /// ファミリーの並び・大きさ・太字ごとの、1 行の書体どおりの高さ
    natural_lines: HashMap<(String, u32, bool), f32>,
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
            font_cx: FontContext::new(),
            fonts: FontTable::new(),
            families: HashMap::new(),
            natural_lines: HashMap::new(),
            layout_cx: LayoutContext::new(),
            scale: ScaleContext::new(),
            glyphs: HashMap::new(),
            layouts: LayoutCache::new(),
            atlas: Atlas::new(ATLAS_SIZE),
            layouts_made: 0,
            glyphs_made: 0,
        })
    }

    /// フレームの終わり。このフレームで使わなかった長い文字列の並べた結果を忘れる
    pub fn end_frame(&mut self) {
        self.layouts.end_frame();
    }

    /// 拡大率が変わったときなどに、作った物を全部忘れる
    pub fn reset(&mut self) {
        self.glyphs.clear();
        self.layouts.clear();
        self.atlas = self.atlas.renewed();
    }

    /// アプリが渡した書体のファイル（Options::fonts）を読み込み、名前で選べるようにする。読めたファミリーの数を返す
    pub fn load_fonts(&mut self, files: &[Cow<'static, [u8]>]) -> usize {
        let mut families = 0;
        for data in files {
            let blob = Blob::new(Arc::new(data.clone()));
            families += self.font_cx.collection.register_fonts(blob, None).len();
        }
        if !files.is_empty() {
            self.families.clear();
            self.layouts.clear();
            let bytes: usize = files.iter().map(|f| f.len()).sum();
            log::event("font.load", &format!("files={} bytes={bytes}", files.len()), &format!("families={families}"));
        }
        families
    }

    /// 試験用: 書体から決めたファミリーの並び（parley は OS とアプリの書体を 1 つの一覧で探すので、2 つ目はいつも false）。
    /// 等幅は、並びの中で入っている最初の書体の名前を返す（並びをそのまま返すと、入っていない BIZ UD ゴシックが先頭に
    /// あるだけで「日本語の等幅がある」と見なし、試験を飛ばせなかった。2026-11-22 の CI の Windows）
    #[cfg(test)]
    pub fn resolved(&mut self, font: &Font) -> (String, bool) {
        let id = self.fonts.id(font);
        let family = self.family(id);
        if *font == Font::Monospace {
            let installed = family.split(',').map(|n| n.trim().trim_matches('"')).find(|n| self.font_cx.collection.family_id(n).is_some());
            return (installed.unwrap_or("monospace").to_string(), false);
        }
        (family, false)
    }

    /// 書体の番号から、parley に渡すファミリーの並びを決める（決めた物は覚える）
    fn family(&mut self, id: FontId) -> String {
        if let Some(f) = self.families.get(&id.0) {
            return f.clone();
        }
        let font = self.fonts.get(id).clone();
        let found = match &font {
            Font::Ui => FONTS.to_string(),
            Font::Monospace => MONOSPACE.to_string(),
            Font::Family(name) => {
                if self.font_cx.collection.family_id(name).is_some() {
                    // 無い文字は UI の書体で出す
                    format!("\"{}\", {FONTS}", name.replace('"', ""))
                } else {
                    log::event("font.missing", &format!("family={name}"), "fallback=ui");
                    FONTS.to_string()
                }
            }
        };
        log::event("font.resolve", &format!("font={font:?}"), &format!("family={found}"));
        self.families.insert(id.0, found.clone());
        found
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
        let laid = Rc::new(self.make_layout(s, style, runs, max_width));
        self.layouts.insert(key, laid.clone());
        laid
    }

    fn make_layout(&mut self, s: &str, style: TextStyle, runs: &[Run], max_width: Option<f32>) -> Laid {
        // 空の文字列は行が無く高さ 0 になるので、1 行分の高さだけを持つ（DirectWrite と同じ）
        if s.is_empty() {
            let probe = self.make_layout(" ", style, &[], None);
            let baseline = probe.lines[0].baseline;
            let lines = vec![Line { start: 0, end: 0, top: 0.0, height: probe.height, width: 0.0, baseline }];
            return Laid { glyphs: Vec::new(), chunks: Vec::new(), width: 0.0, height: probe.height, lines, text: String::new(), layout: None, carets: RefCell::default() };
        }
        // 範囲の書体の名前は、借りの前に決めておく
        let run_families: Vec<Option<String>> = runs.iter().map(|r| (r.style.font != style.font).then(|| self.family(r.style.font))).collect();
        self.layouts_made += 1;
        let family = self.family(style.font);
        // 行の間が UI の書体より狭い書体（BIZ UD ゴシックなど）は、UI の書体の行の高さに広げる（字は行の真ん中に来る）
        let line = if style.font != FontId(0) {
            let (ui_h, h) = (self.natural_line(FONTS, style.size, style.bold), self.natural_line(&family, style.size, style.bold));
            (h + 0.01 < ui_h).then_some(ui_h)
        } else {
            None
        };
        let mut b = self.layout_cx.ranged_builder(&mut self.font_cx, s, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::Source(family.into())));
        b.push_default(StyleProperty::FontSize(style.size));
        b.push_default(StyleProperty::Locale(Language::parse("ja-JP").ok()));
        if style.bold {
            b.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
        }
        // 行の詰まった書体を広げた時でも、大きい範囲があれば書体どおりの行の間にする（行からはみ出さないように）
        let bigger = runs.iter().any(|r| r.style.size > style.size + 0.01);
        if let Some(h) = line.filter(|_| !bigger) {
            b.push_default(StyleProperty::LineHeight(LineHeight::Absolute(h)));
        }
        for (r, family) in runs.iter().zip(run_families) {
            if r.end <= r.start {
                continue;
            }
            let range = byte_of_utf16(s, r.start)..byte_of_utf16(s, r.end);
            b.push(StyleProperty::FontWeight(if r.style.bold { FontWeight::BOLD } else { FontWeight::NORMAL }), range.clone());
            b.push(StyleProperty::FontStyle(if r.italic { FontStyle::Italic } else { FontStyle::Normal }), range.clone());
            b.push(StyleProperty::FontSize(r.style.size), range.clone());
            if let Some(f) = family {
                b.push(StyleProperty::FontFamily(FontFamily::Source(f.into())), range);
            }
        }
        let mut layout: Layout<()> = b.build(s);
        layout.break_all_lines(max_width);
        layout.align(Alignment::Start, AlignmentOptions::default());
        // バイトの位置 → UTF-16 の位置（形ごとに数え直さないように、先に表にする）
        let mut utf16 = vec![0u32; s.len() + 1];
        let mut n = 0u32;
        for (b, ch) in s.char_indices() {
            utf16[b] = n;
            n += ch.len_utf16() as u32;
        }
        utf16[s.len()] = n;
        let mut glyphs = Vec::new();
        for line in layout.lines() {
            let mid = line.metrics().baseline;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let r = run.run();
                let font = r.font().clone();
                let size = r.font_size();
                let coords: Vec<i16> = r.normalized_coords().iter().map(|c| c.to_bits()).collect();
                let synthesis = r.synthesis();
                let (embolden, skew) = (synthesis.embolden(), synthesis.skew().unwrap_or(0.0));
                // 形ごとの文字の位置: 並びのクラスターを見た目の順にたどる（数が合わない時は、形の位置から探す）
                let from_clusters: Vec<u32> =
                    r.visual_clusters().flat_map(|c| std::iter::repeat_n(utf16[c.text_range().start], c.glyphs().count())).collect();
                let placed: Vec<_> = run.positioned_glyphs().collect();
                let same = from_clusters.len() == placed.len();
                for (k, g) in placed.iter().enumerate() {
                    let pos = if same {
                        from_clusters[k]
                    } else {
                        utf16[Cursor::from_point(&layout, g.x + g.advance * 0.25, mid).index().min(s.len())]
                    };
                    if let Some(slot) = self.glyph(&font, g.id, size, &coords, embolden, skew) {
                        glyphs.push(Placed { x: g.x, y: g.y, slot, pos });
                    }
                }
            }
        }
        // 行の一覧（改行は行の終わりに入れない）。UTF-16 の位置は先に作った表から引く（行ごとに文字列の頭から
        // 数えると、行の多い文字列で時間が字数の 2 乗になった。2 MB・4 万行で 66 秒）
        let mut lines = Vec::new();
        for line in layout.lines() {
            let r = line.text_range();
            let m = line.metrics();
            let end = if s[r.start..r.end].ends_with('\n') { r.end - 1 } else { r.end };
            let (top, height) = (m.block_min_coord, m.block_max_coord - m.block_min_coord);
            lines.push(Line { start: utf16[r.start] as usize, end: utf16[end] as usize, top, height, width: m.advance, baseline: m.baseline });
        }
        add_trailing_line(s, &mut lines);
        let height = lines.last().map_or(layout.height(), |l| layout.height().max(l.top + l.height));
        // 幅は行末の空白も含める（入力欄のキャレットが空白の後ろへ進むように。DirectWrite と同じ）
        Laid { chunks: glyph_chunks(&glyphs), glyphs, width: layout.full_width(), height, lines, text: s.to_string(), layout: Some(layout), carets: RefCell::default() }
    }

    /// そのファミリーの並びの、1 行の書体どおりの高さ（覚えておく）。英字と日本語で測って低い方を使う
    /// （書体に無い字は代わりの書体で並び、その行は代わりの書体の高さになるため）
    fn natural_line(&mut self, family: &str, size: f32, bold: bool) -> f32 {
        let key = (family.to_string(), (size * 64.0) as u32, bold);
        if let Some(h) = self.natural_lines.get(&key) {
            return *h;
        }
        let mut lowest = f32::MAX;
        for sample in ["Ag", "あ"] {
            let mut b = self.layout_cx.ranged_builder(&mut self.font_cx, sample, 1.0, true);
            b.push_default(StyleProperty::FontFamily(FontFamily::Source(family.to_string().into())));
            b.push_default(StyleProperty::FontSize(size));
            b.push_default(StyleProperty::Locale(Language::parse("ja-JP").ok()));
            if bold {
                b.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
            }
            let mut layout: Layout<()> = b.build(sample);
            layout.break_all_lines(None);
            lowest = lowest.min(layout.height());
        }
        self.natural_lines.insert(key, lowest);
        lowest
    }

    fn glyph(&mut self, font: &FontData, glyph: u32, size: f32, coords: &[i16], embolden: bool, skew: f32) -> Option<Slot> {
        let key = GlyphKey { font: font.data.id(), index: font.index, glyph, size: (size * 64.0) as u32, embolden, skew: skew.to_bits() };
        if let Some(s) = self.glyphs.get(&key) {
            return *s;
        }
        let slot = self.rasterize(font, glyph, size, coords, embolden, skew);
        self.glyphs.insert(key, slot);
        slot
    }

    fn rasterize(&mut self, font: &FontData, glyph: u32, size: f32, coords: &[i16], embolden: bool, skew: f32) -> Option<Slot> {
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
        if skew != 0.0 {
            // 斜体の書体が無い時は、輪郭を右へ傾ける（形の座標は上向きが正なので、上ほど右へずらす）
            render.transform(Some(Transform::skew(Angle::from_degrees(skew), Angle::from_degrees(0.0))));
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
