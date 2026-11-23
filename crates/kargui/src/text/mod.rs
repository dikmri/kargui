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
    pub(crate) fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
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

    pub(crate) fn write(&mut self, x: u32, y: u32, w: u32, h: u32, src: &[u8]) {
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
    /// どの文字の形か（UTF-16 の位置。合字などで何文字かが 1 つの形になる時は、その頭の位置）
    pub pos: u32,
}

/// 並べた文字列の 1 行。位置は UTF-16 で、end は改行の手前。改行で終わる文字列は、最後に空の行を持つ
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Line {
    pub start: usize,
    pub end: usize,
    /// 行の上端と高さ（並べた範囲の左上が原点）
    pub top: f32,
    pub height: f32,
    /// 行の文字の幅（行末の空白を含む）
    pub width: f32,
    /// ベースラインの位置（並べた範囲の上から）
    pub baseline: f32,
}

/// 改行で終わる文字列の最後の空の行を足す（並べる道具が作らない時）
fn add_trailing_line(s: &str, lines: &mut Vec<Line>) {
    let len16 = s.encode_utf16().count();
    if !s.ends_with('\n') {
        return;
    }
    if let Some(&last) = lines.last()
        && last.start != len16 {
            let top = last.top + last.height;
            lines.push(Line { start: len16, end: len16, top, height: last.height, width: 0.0, baseline: top + (last.baseline - last.top) });
        }
}

/// A typeface for text.
///
/// The font of the whole window is [`Options::font`](crate::Options::font) (changed while the
/// app runs with [`Ui::set_font`](crate::Ui::set_font)); labels and text fields can have their own
/// with `.font(…)` or `.monospace()`. Characters a font lacks are drawn with the system's
/// fallback fonts, so Japanese stays readable in a Latin-only font.
///
/// ```
/// use kargui::Font;
///
/// # kargui::test::example(|ui| {
/// ui.label("let total = 1_280;").font(Font::Monospace);
/// ui.label("お知らせ").font(Font::family("BIZ UDPGothic"));
/// # });
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Font {
    /// The system's font for user interfaces: Yu Gothic UI on Windows, Hiragino Sans on macOS
    /// and a Japanese sans-serif such as Noto Sans CJK JP on Linux. The default.
    #[default]
    Ui,
    /// A fixed-width font for code and columns of numbers. Where the font has both, a Japanese
    /// character is exactly as wide as two Latin letters: BIZ UDGothic on Windows (or Consolas,
    /// then MS Gothic), and Noto Sans Mono CJK JP, TakaoGothic or DejaVu Sans Mono on Linux.
    Monospace,
    /// A font family by name: one installed on the system, or one whose file the app gave in
    /// [`Options::fonts`](crate::Options::fonts). If no font has the name, the UI font is used
    /// (and the log says `font.missing`).
    Family(String),
}

impl Font {
    /// A font family by name. See [`Font::Family`].
    ///
    /// ```
    /// use kargui::Font;
    ///
    /// assert_eq!(Font::family("Meiryo"), Font::Family("Meiryo".to_string()));
    /// ```
    pub fn family(name: impl Into<String>) -> Self {
        Font::Family(name.into())
    }
}

/// 書体の番号（FontTable の位置）。0 はいつも UI の書体
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub(crate) struct FontId(pub u16);

/// 使った書体の一覧。部品と書式は書体を番号で持つ（毎フレーム文字列を比べないため）
pub(crate) struct FontTable(Vec<Font>);

impl FontTable {
    pub fn new() -> Self {
        Self(vec![Font::Ui])
    }

    /// 書体の番号（初めての書体は一覧の最後に足す）
    pub fn id(&mut self, font: &Font) -> FontId {
        if let Some(k) = self.0.iter().position(|f| f == font) {
            return FontId(k as u16);
        }
        self.0.push(font.clone());
        FontId((self.0.len() - 1) as u16)
    }

    pub fn get(&self, id: FontId) -> &Font {
        self.0.get(id.0 as usize).unwrap_or(&Font::Ui)
    }
}

/// 文字の書式
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextStyle {
    /// 大きさ（物理ピクセル）
    pub size: f32,
    pub bold: bool,
    pub font: FontId,
}

/// 書式の範囲（書式付きの文字）。start..end は UTF-16 の位置で、範囲の外は基の書式で並べる
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Run {
    pub start: usize,
    pub end: usize,
    pub style: TextStyle,
    pub italic: bool,
}

/// 並べた結果を覚えるための鍵
#[derive(Hash, Eq, PartialEq, Clone)]
struct LayoutKey {
    text: String,
    size: u32,
    bold: bool,
    font: u16,
    max_width: u32,
    /// 書式の範囲（start, end, 大きさ, 太字, 斜体, 書体）。ふつうの文字は空
    runs: Vec<(u32, u32, u32, bool, bool, u16)>,
}

impl LayoutKey {
    fn new(s: &str, style: TextStyle, max_width: Option<f32>) -> Self {
        Self {
            text: s.to_string(),
            size: (style.size * 64.0) as u32,
            bold: style.bold,
            font: style.font.0,
            max_width: max_width.map_or(u32::MAX, |w| w.max(0.0).round() as u32),
            runs: Vec::new(),
        }
    }

    fn with_runs(s: &str, style: TextStyle, runs: &[Run], max_width: Option<f32>) -> Self {
        let runs = runs
            .iter()
            .map(|r| (r.start as u32, r.end as u32, (r.style.size * 64.0) as u32, r.style.bold, r.italic, r.style.font.0))
            .collect();
        Self { runs, ..Self::new(s, style, max_width) }
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
        let style = TextStyle { size: 14.0, bold: false, font: FontId(0) };
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
        let style = TextStyle { size: 14.0, bold: false, font: FontId(0) };
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
        ink_of(t, &laid.glyphs)
    }

    /// 形の一覧の濃さの合計
    fn ink_of(t: &Text, glyphs: &[Placed]) -> u64 {
        let mut sum = 0u64;
        for g in glyphs {
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
    fn lines_carets_and_points_in_multiline_text() {
        let mut t = Text::new().unwrap();
        let st = TextStyle { size: 14.0, bold: false, font: FontId(0) };
        let laid = t.layout("ab\ncd\n", st, None);
        let spans: Vec<(usize, usize)> = laid.lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(spans, [(0, 2), (3, 5), (6, 6)], "改行で終わると最後に空の行");
        let h = laid.lines[0].height;
        assert!(h > 10.0 && laid.lines[1].top >= h - 0.5 && laid.lines[2].top >= 2.0 * h - 1.0, "{:?}", laid.lines);
        assert!(laid.height >= 3.0 * h - 1.0, "高さは 3 行分: {}", laid.height);
        // 4 の前（d の前）は 2 行目
        let (x, top, ch) = laid.caret(4);
        assert!(x > 0.0 && (top - laid.lines[1].top).abs() < 0.5 && ch > 10.0, "{x} {top} {ch}");
        // 最後の改行の後ろは、空の最後の行の左端
        let (x, top, _) = laid.caret(6);
        assert!(x.abs() < 0.5 && (top - laid.lines[2].top).abs() < 0.5, "{x} {top}");
        // 2 行目の右の外を押すと 2 行目の終わり、左の端なら 2 行目の始め
        assert_eq!(laid.index_at_point(1000.0, laid.lines[1].top + 2.0), 5);
        assert_eq!(laid.index_at_point(0.0, laid.lines[1].top + 2.0), 3);
        // 折り返した行は、すき間なく並び、幅に収まる
        let long = "これは日本語の折り返しを確かめるための文です。";
        let w = t.layout(long, st, Some(100.0));
        assert!(w.lines.len() >= 3, "{:?}", w.lines);
        assert_eq!(w.lines[0].start, 0);
        assert_eq!(w.lines.last().unwrap().end, long.encode_utf16().count());
        for pair in w.lines.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
            assert!(pair[1].top > pair[0].top);
        }
        for l in &w.lines {
            assert!(l.width <= 101.0, "{l:?}");
        }
        // 1 行の文字列と空の文字列も 1 行
        assert_eq!(t.layout("abc", st, None).lines.len(), 1);
        let empty = t.layout("", st, None);
        assert_eq!(empty.lines.len(), 1);
        assert_eq!(empty.caret(0).1, 0.0);
    }

    fn styled(t: &mut Text, font: Font) -> TextStyle {
        TextStyle { size: 14.0, bold: false, font: t.fonts.id(&font) }
    }

    #[test]
    fn font_table_gives_each_font_one_number_and_zero_is_the_ui_font() {
        let mut f = FontTable::new();
        assert_eq!(f.id(&Font::Ui), FontId(0));
        let mono = f.id(&Font::Monospace);
        assert_eq!(f.id(&Font::family("Meiryo")), FontId(2));
        assert_eq!(f.id(&Font::Monospace), mono, "同じ書体は同じ番号");
        assert_eq!(f.get(FontId(2)), &Font::family("Meiryo"));
        assert_eq!(f.get(FontId(99)), &Font::Ui, "知らない番号は UI の書体");
    }

    /// 行の間が狭い書体（BIZ UD ゴシックは行の高さが文字の大きさと同じ）も、UI の書体より詰めない。
    /// 作者がメモ帳を等幅にした時、行の高さが 19 から 14 ピクセルになり、行がくっついた（2026-11-22 の記録）
    #[test]
    fn fonts_with_tight_lines_get_at_least_the_ui_fonts_line_height() {
        let mut t = Text::new().unwrap();
        let ui = styled(&mut t, Font::Ui);
        let step = |l: &Laid| l.lines[1].top - l.lines[0].top;
        let ui_step = step(&t.layout("a\n漢", ui, None));
        // 英字だけの等幅の書体（BIZ UD ゴシックの無い CI の Windows では、等幅がこれになった。2026-11-22 の CI）
        let latin_mono = if cfg!(windows) {
            "Consolas"
        } else if cfg!(target_os = "macos") {
            "Menlo"
        } else {
            "DejaVu Sans Mono"
        };
        for font in [Font::Monospace, Font::family("BIZ UDPGothic"), Font::family(latin_mono)] {
            let st = styled(&mut t, font.clone());
            let laid = t.layout("a\n漢", st, None);
            assert!(step(&laid) >= ui_step - 0.5, "{font:?}: 行の間 {} < UI の書体の {ui_step}", step(&laid));
            assert!(laid.height >= 2.0 * ui_step - 1.0, "{font:?}: 高さ {}", laid.height);
            // 字は広げた行の中に収まる（上下にはみ出さない）
            for g in &laid.glyphs {
                let (top, bottom) = (g.y + g.slot.top as f32, g.y + g.slot.top as f32 + g.slot.h as f32);
                assert!(top >= -1.0 && bottom <= laid.height + 1.0, "{font:?}: 字 {top}..{bottom} 高さ {}", laid.height);
            }
        }
    }

    /// 等幅の書体では、細い字も太い字も同じ幅。UI の書体では違う
    #[test]
    fn monospace_letters_all_have_the_same_width() {
        let mut t = Text::new().unwrap();
        let (mono, ui) = (styled(&mut t, Font::Monospace), styled(&mut t, Font::Ui));
        let (i, m) = (t.layout("iiiiiiii", mono, None).width, t.layout("MMMMMMMM", mono, None).width);
        assert!((i - m).abs() < 0.5, "等幅: i {i} M {m}（{:?}）", t.resolved(&Font::Monospace));
        let (i, m) = (t.layout("iiiiiiii", ui, None).width, t.layout("MMMMMMMM", ui, None).width);
        assert!(m > i * 1.5, "UI の書体: i {i} M {m}");
    }

    /// 日本語も持つ等幅の書体（BIZ UD ゴシックなど）では、日本語 1 字が英字 2 字とちょうど同じ幅
    #[test]
    fn in_a_japanese_monospace_font_a_kanji_is_two_letters_wide() {
        let mut t = Text::new().unwrap();
        let (family, _) = t.resolved(&Font::Monospace);
        let japanese = ["BIZ UDGothic", "Noto Sans Mono CJK JP", "TakaoGothic", "IPAGothic", "VL Gothic", "MS Gothic"];
        if !japanese.iter().any(|f| family.starts_with(&format!("\"{f}\"")) || family == *f) {
            eprintln!("日本語の等幅の書体が無いので飛ばす: {family}");
            return;
        }
        let mono = styled(&mut t, Font::Monospace);
        let (kanji, latin) = (t.layout("漢字漢字", mono, None).width, t.layout("abcdefgh", mono, None).width);
        assert!((kanji - latin).abs() < 1.0, "漢字 4 字 {kanji}、英字 8 字 {latin}（{family}）");
    }

    /// 無い名前の書体は UI の書体で出す（文字が消えない）
    #[test]
    fn an_unknown_family_falls_back_to_the_ui_font() {
        let mut t = Text::new().unwrap();
        let (missing, ui) = (styled(&mut t, Font::family("No Such Font 4cb3")), styled(&mut t, Font::Ui));
        let (a, b) = (t.layout("設定 Settings", missing, None).width, t.layout("設定 Settings", ui, None).width);
        assert!((a - b).abs() < 0.5, "{a} {b}");
        assert_eq!(t.resolved(&Font::family("No Such Font 4cb3")), t.resolved(&Font::Ui));
    }

    /// アプリが渡した書体のファイルを読み込むと、その名前で選べる（Windows では OS の書体より先に探す）
    #[test]
    fn font_files_from_the_app_are_found_by_family_name() {
        let (path, family) = if cfg!(windows) {
            ("C:\\Windows\\Fonts\\consola.ttf", "Consolas")
        } else {
            ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", "DejaVu Sans Mono")
        };
        let Ok(data) = std::fs::read(path) else {
            eprintln!("{path} が無いので飛ばす");
            return;
        };
        let mut t = Text::new().unwrap();
        assert_eq!(t.load_fonts(&[]), 0, "渡さなければ何も読まない");
        assert!(t.load_fonts(&[data.into()]) >= 1);
        let (name, from_app) = t.resolved(&Font::family(family));
        assert!(name.contains(family), "{name}");
        if cfg!(all(windows, not(feature = "portable-text"))) {
            assert!(from_app, "読み込んだファイルから見つける");
        }
        let st = styled(&mut t, Font::family(family));
        let (i, m) = (t.layout("iiii", st, None).width, t.layout("MMMM", st, None).width);
        assert!((i - m).abs() < 0.5, "読み込んだ等幅の書体で並べる: {i} {m}");
    }

    #[test]
    fn bold_draws_more_ink_and_empty_text_has_height() {
        let mut t = Text::new().unwrap();
        let normal = t.layout("Settings 設定", TextStyle { size: 14.0, bold: false, font: FontId(0) }, None);
        let bold = t.layout("Settings 設定", TextStyle { size: 14.0, bold: true, font: FontId(0) }, None);
        let (a, b) = (ink(&t, &normal), ink(&t, &bold));
        assert!(b as f64 > a as f64 * 1.1, "普通 {a}、太字 {b}");
        assert!(bold.width >= normal.width, "{} {}", bold.width, normal.width);
        let empty = t.layout("", TextStyle { size: 14.0, bold: false, font: FontId(0) }, None);
        assert_eq!(empty.width, 0.0);
        assert!(empty.height > 10.0, "{}", empty.height);
    }

    const BASE: TextStyle = TextStyle { size: 14.0, bold: false, font: FontId(0) };

    fn run(start: usize, end: usize) -> Run {
        Run { start, end, style: BASE, italic: false }
    }

    /// 並べた形は、どの文字の形か（UTF-16 の位置）を持つ。書式付きの文字は、この位置で範囲ごとの色を決める。
    /// 行は、ベースラインの位置（並べた範囲の上から）を持つ。下線と取り消し線はこの位置から引く
    #[test]
    fn glyphs_know_their_text_position_and_lines_their_baseline() {
        let mut t = Text::new().unwrap();
        let laid = t.layout("ab牛乳😀c", BASE, None);
        let pos: Vec<u32> = laid.glyphs.iter().map(|g| g.pos).collect();
        assert_eq!(pos, [0, 1, 2, 3, 4, 6], "絵文字は 2 つ分");
        let two = t.layout("ab\ncd", BASE, None);
        assert_eq!(two.glyphs.iter().map(|g| g.pos).collect::<Vec<_>>(), [0, 1, 3, 4]);
        for l in &two.lines {
            assert!(l.baseline > l.top + l.height * 0.5 && l.baseline < l.top + l.height, "{l:?}");
        }
        // 形のペンの位置はベースラインの上にある
        for g in &two.glyphs {
            let line = two.lines.iter().find(|l| (g.pos as usize) >= l.start && (g.pos as usize) <= l.end).unwrap();
            assert!((g.y - line.baseline).abs() < 0.5, "{} {}", g.y, line.baseline);
        }
    }

    /// 範囲ごとに太字・大きさ・書体を変えて、1 つの段落に並べる（範囲の外は基の書式）
    #[test]
    fn runs_mix_bold_size_and_font_in_one_paragraph() {
        let mut t = Text::new().unwrap();
        let s = "Save 設定 now";
        let plain = t.layout(s, BASE, None);
        // 太字の範囲（「設定」）は濃くなり、ほかの字は同じ形
        let bold = t.layout_runs(s, BASE, &[Run { style: TextStyle { bold: true, ..BASE }, ..run(5, 7) }], None);
        let ink_in = |t: &Text, l: &Laid, range: std::ops::Range<u32>| {
            let only: Vec<Placed> = l.glyphs.iter().copied().filter(|g| range.contains(&g.pos)).collect();
            ink_of(t, &only)
        };
        assert!(ink_in(&t, &bold, 5..7) as f64 > ink_in(&t, &plain, 5..7) as f64 * 1.1, "太字の範囲は濃い");
        assert_eq!(ink_in(&t, &bold, 0..4), ink_in(&t, &plain, 0..4), "範囲の外は同じ");
        // 大きい範囲があると、行が高くなり、ベースラインも下がる
        let big = t.layout_runs(s, BASE, &[Run { style: TextStyle { size: 28.0, ..BASE }, ..run(5, 7) }], None);
        assert!(big.height > plain.height * 1.5, "{} {}", big.height, plain.height);
        assert!(big.lines[0].baseline > plain.lines[0].baseline + 5.0, "{:?} {:?}", big.lines[0], plain.lines[0]);
        // 等幅の範囲（「now」を等幅に）は、UI の書体と幅が変わる
        let mono = t.fonts.id(&Font::Monospace);
        let code = t.layout_runs("iii MMM", BASE, &[Run { style: TextStyle { font: mono, ..BASE }, ..run(0, 3) }], None);
        let ui = t.layout("iii MMM", BASE, None);
        assert!(code.x_of(3) > ui.x_of(3) + 3.0, "等幅の i は広い: {} {}", code.x_of(3), ui.x_of(3));
        // 同じ文字列と範囲は覚えた結果を返し、範囲が違えば別に並べる
        let again = t.layout_runs(s, BASE, &[Run { style: TextStyle { bold: true, ..BASE }, ..run(5, 7) }], None);
        assert!(std::rc::Rc::ptr_eq(&bold, &again));
        let other = t.layout_runs(s, BASE, &[Run { style: TextStyle { bold: true, ..BASE }, ..run(0, 4) }], None);
        assert!(!std::rc::Rc::ptr_eq(&bold, &other));
    }

    /// 斜体の範囲は傾いた形で描く。日本語の書体には斜体の書体が無いので、形を傾けて作る
    #[test]
    fn italic_runs_lean_even_without_an_italic_font() {
        let mut t = Text::new().unwrap();
        let s = "Il 設定";
        let plain = t.layout(s, BASE, None);
        let italic = t.layout_runs(s, BASE, &[Run { italic: true, ..run(0, s.encode_utf16().count()) }], None);
        assert_eq!(plain.glyphs.len(), italic.glyphs.len());
        // 縦の棒（I と l）は、傾くと形の幅が広がる
        for k in 0..2 {
            let (a, b) = (plain.glyphs[k].slot, italic.glyphs[k].slot);
            assert!(b.w > a.w, "{k}: 普通 {a:?}、斜体 {b:?}");
        }
    }

    /// 折り返しても、形の位置は文字の順で、どの行にも入る
    #[test]
    fn wrapped_runs_keep_positions_in_order() {
        let mut t = Text::new().unwrap();
        let s = "これは書式の付いた文字を折り返して確かめるための長い文です。";
        let laid = t.layout_runs(s, BASE, &[Run { style: TextStyle { bold: true, ..BASE }, ..run(3, 9) }], Some(120.0));
        assert!(laid.lines.len() >= 3, "{:?}", laid.lines);
        let pos: Vec<u32> = laid.glyphs.iter().map(|g| g.pos).collect();
        assert!(pos.windows(2).all(|w| w[1] > w[0]), "{pos:?}");
        assert_eq!(pos.len(), s.chars().count());
        for l in &laid.lines {
            assert!(l.width <= 121.0, "{l:?}");
        }
    }
}
