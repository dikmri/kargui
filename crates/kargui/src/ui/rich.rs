//! 書式付きの文字（`ui.rich_text`）。部分ごとに太字・斜体・色・等幅・リンクを混ぜて、1 つの段落に並べる。
//!
//! 親の部品（Kind::RichText）が部分をつないだ文字列と部分ごとの書式を持ち、1 回で並べて描く。部分は子の部品
//! （Kind::RichSpan）にもなり、読み上げの木の中の文字とリンク・試験台の find と click・リンクの当たりと焦点に使う。
//! 子の四角は、並べた結果から決める（レイアウトの仕組みは葉の子を置かないので、place_spans が入れる）。

use super::context::Context;
use super::{Dir, Id, Kind, TextKind, Tree, Ui};
use crate::geom::{Color, Rect};
use crate::log;
use crate::text::{Font, FontId, Laid, Run, TextStyle};

/// 部分の範囲（UTF-16）と書式
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct SpanStyle {
    pub start: usize,
    pub end: usize,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub weak: bool,
    /// 等幅で、薄い背景を敷く（コード）
    pub code: bool,
    pub link: bool,
    pub size: Option<TextKind>,
    pub color: Option<Color>,
    pub font: Option<FontId>,
}

/// 部分の書式から、文字の道に渡す範囲を作る。基と同じ書式で並ぶ部分は渡さない（色や下線は描く時に付ける）
pub(crate) fn runs(spans: &[SpanStyle], base: TextStyle, scale: f32) -> Vec<Run> {
    spans
        .iter()
        .filter(|s| s.end > s.start)
        .map(|s| {
            let size = s.size.map_or(base.size, |k| (k.size() * scale).round());
            let style = TextStyle { size, bold: s.bold || base.bold, font: s.font.unwrap_or(base.font) };
            Run { start: s.start, end: s.end, style, italic: s.italic }
        })
        .filter(|r| r.style != base || r.italic)
        .collect()
}

/// pos（UTF-16）の文字が入っている部分の番号
pub(crate) fn span_at(spans: &[SpanStyle], pos: usize) -> Option<usize> {
    // 部分は前から順に並び、すき間が無い。空の部分は飛ばす
    let k = spans.partition_point(|s| s.end <= pos);
    (k < spans.len() && spans[k].start <= pos).then_some(k)
}

/// 範囲 start..end が各行で占める横の区間（行の番号, 左の x, 右の x）。並べた範囲の左上が原点。
/// text16 は並べた文字列（UTF-16）。折り返した行の終わりにぶら下がる空白は区間に入れない（下線を引かない）
pub(crate) fn segments(laid: &Laid, text16: &[u16], start: usize, end: usize) -> Vec<(usize, f32, f32)> {
    let mut out = Vec::new();
    for (n, l) in laid.lines.iter().enumerate() {
        let (a, mut b) = (start.max(l.start), end.min(l.end));
        if b == l.end {
            while b > a && text16.get(b - 1).is_some_and(|&c| c == 0x20 || c == 0x3000 || c == 0x09) {
                b -= 1;
            }
        }
        if a >= b {
            continue;
        }
        let x0 = if a == l.start { 0.0 } else { laid.caret(a).0 };
        let x1 = if b == l.end { l.width } else { laid.caret(b).0 };
        if x1 > x0 {
            out.push((n, x0, x1));
        }
    }
    out
}

/// 部分の子の四角を、並べた結果から決める（最初の行の区間。読み上げの枠と試験台の click に使う）
pub(crate) fn place_spans(tree: &mut Tree, cx: &mut Context) {
    for i in 0..tree.info.len() {
        let Kind::RichText { text, spans } = &tree.info[i].kind else { continue };
        let r = tree.layout[i].rect;
        let base = base_style(cx);
        let laid = cx.text.layout_runs(text, base, &runs(spans, base, cx.scale), Some(r.w));
        let text16: Vec<u16> = text.encode_utf16().collect();
        let children = tree.layout[i].children.clone();
        for (k, c) in children.into_iter().enumerate() {
            let Some(s) = spans.get(k) else { break };
            let rect = segments(&laid, &text16, s.start, s.end)
                .first()
                .map(|&(n, x0, x1)| {
                    let l = laid.lines[n];
                    Rect::new((r.x + x0).floor(), (r.y + l.top).floor(), (x1 - x0).ceil(), l.height.ceil())
                })
                .unwrap_or(Rect::new(r.x, r.y, 0.0, 0.0));
            tree.layout[c].rect = rect;
        }
    }
}

/// 書式付きの文字の基の書式（本文の大きさ・窓の書体）
pub(crate) fn base_style(cx: &Context) -> TextStyle {
    TextStyle { size: (TextKind::Body.size() * cx.scale).round(), bold: false, font: cx.font }
}

/// Builds the parts of [`Ui::rich_text`]: plain text with [`RichText::text`] and links with
/// [`RichText::link`], one after another in one paragraph that wraps to the width.
///
/// ```
/// # let mut show_help = false;
/// # kargui::test::example(|ui| {
/// ui.rich_text(|t| {
///     t.text("Deleting cannot be ");
///     t.text("undone").bold();
///     t.text(". See ");
///     if t.link("how backups work").clicked() {
///         show_help = true;
///     }
///     t.text(".");
/// });
/// # });
/// ```
pub struct RichText<'a> {
    pub(super) ui: Ui<'a>,
}

impl RichText<'_> {
    /// Adds text. Style it with the methods of [`Span`], such as [`Span::bold`] or [`Span::code`].
    /// A `\n` in the text starts a new line.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("Press ");
    ///     t.text("Ctrl+S").code();
    ///     t.text(" to save.");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn text(&mut self, text: impl Into<String>) -> Span<'_> {
        self.push(text.into(), false).0
    }

    /// Adds a link: drawn in the accent color and underlined, it takes the keyboard focus with
    /// Tab, and Enter or a click on it makes [`RichLink::clicked`] true (opening a web page is
    /// up to the app). Screen readers hear it as a link inside the paragraph.
    ///
    /// ```
    /// # let mut page = "";
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("Read the ");
    ///     if t.link("privacy policy").clicked() {
    ///         page = "privacy";
    ///     }
    ///     t.text(" before you sign up.");
    /// });
    /// # });
    /// ```
    #[track_caller]
    pub fn link(&mut self, text: impl Into<String>) -> RichLink<'_> {
        let text = text.into();
        let enabled = !self.ui.disabled;
        let (span, id) = self.push(text.clone(), true);
        let clicked = enabled && span.cx.clicked == Some(id);
        if clicked {
            log::event("rich.link", &format!("text={text}"), "clicked");
        }
        RichLink { span, clicked }
    }

    #[track_caller]
    fn push(&mut self, text: String, link: bool) -> (Span<'_>, Id) {
        let rich = self.ui.parent;
        let (_, id) = self.ui.add(Kind::RichSpan { text: text.clone(), link }, Dir::Leaf);
        let mut k = 0;
        if let Kind::RichText { text: all, spans } = &mut self.ui.tree.info[rich].kind {
            let start = spans.last().map_or(0, |s| s.end);
            all.push_str(&text);
            let end = start + text.encode_utf16().count();
            spans.push(SpanStyle { start, end, link, underline: link, ..Default::default() });
            k = spans.len() - 1;
        }
        (Span { cx: &mut *self.ui.cx, tree: &mut *self.ui.tree, rich, k }, id)
    }
}

/// One part of a [`Ui::rich_text`] paragraph, returned by [`RichText::text`]. Its methods style
/// only this part; the rest of the paragraph keeps the body style.
///
/// ```
/// # kargui::test::example(|ui| {
/// ui.rich_text(|t| {
///     t.text("Total: ");
///     t.text("1,280").bold();
/// });
/// # });
/// ```
pub struct Span<'a> {
    cx: &'a mut Context,
    tree: &'a mut Tree,
    rich: usize,
    k: usize,
}

impl Span<'_> {
    fn style(self, f: impl FnOnce(&mut SpanStyle)) -> Self {
        if let Kind::RichText { spans, .. } = &mut self.tree.info[self.rich].kind
            && let Some(s) = spans.get_mut(self.k)
        {
            f(s);
        }
        self
    }

    /// Draws this part bold.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("This cannot be ");
    ///     t.text("undone").bold();
    /// });
    /// # });
    /// ```
    pub fn bold(self) -> Self {
        self.style(|s| s.bold = true)
    }

    /// Draws this part slanted (fonts without an italic style, such as Japanese fonts, are
    /// slanted by the system).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("The word ");
    ///     t.text("ephemeral").italic();
    ///     t.text(" means short-lived.");
    /// });
    /// # });
    /// ```
    pub fn italic(self) -> Self {
        self.style(|s| s.italic = true)
    }

    /// Draws a line under this part.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("Fill in ");
    ///     t.text("every").underline();
    ///     t.text(" field.");
    /// });
    /// # });
    /// ```
    pub fn underline(self) -> Self {
        self.style(|s| s.underline = true)
    }

    /// Draws a line through this part (for example an old price).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("1,980").strike();
    ///     t.text(" 1,280").bold();
    /// });
    /// # });
    /// ```
    pub fn strike(self) -> Self {
        self.style(|s| s.strike = true)
    }

    /// Draws this part in the weak text color (for notes that matter less).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("Backup finished ");
    ///     t.text("(3 minutes ago)").weak();
    /// });
    /// # });
    /// ```
    pub fn weak(self) -> Self {
        self.style(|s| s.weak = true)
    }

    /// Draws this part in `color`.
    ///
    /// ```
    /// use kargui::rgb;
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("Status: ");
    ///     t.text("failed").color(rgb(0xc42b1c)).bold();
    /// });
    /// # });
    /// ```
    pub fn color(self, color: Color) -> Self {
        self.style(|s| s.color = Some(color))
    }

    /// Draws this part smaller (12.5 instead of 14 logical pixels).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("Price 1,280");
    ///     t.text(" (tax included)").small().weak();
    /// });
    /// # });
    /// ```
    pub fn small(self) -> Self {
        self.style(|s| s.size = Some(TextKind::Small))
    }

    /// Sets the size of this part in logical pixels (body text is 14). The line grows to fit it.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("1,280").size(28.0).bold();
    ///     t.text(" yen this month");
    /// });
    /// # });
    /// ```
    pub fn size(self, size: f32) -> Self {
        let tenths = (size.clamp(4.0, 200.0) * 10.0).round() as u32;
        self.style(|s| s.size = Some(TextKind::Custom(tenths)))
    }

    /// Draws this part in `font` instead of the window's font. See [`Font`].
    ///
    /// ```
    /// use kargui::Font;
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("見出し").font(Font::family("BIZ UDPGothic"));
    ///     t.text(" と本文");
    /// });
    /// # });
    /// ```
    pub fn font(self, font: Font) -> Self {
        let id = self.cx.text.fonts.id(&font);
        self.style(|s| s.font = Some(id))
    }

    /// Draws this part in the fixed-width font ([`Font::Monospace`]).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("File ");
    ///     t.text("report_2026.csv").monospace();
    ///     t.text(" was saved.");
    /// });
    /// # });
    /// ```
    pub fn monospace(self) -> Self {
        self.font(Font::Monospace)
    }

    /// Draws this part as code: in the fixed-width font on a light background.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("Run ");
    ///     t.text("cargo run").code();
    ///     t.text(" in the project folder.");
    /// });
    /// # });
    /// ```
    pub fn code(self) -> Self {
        self.monospace().style(|s| s.code = true)
    }
}

/// A link in a [`Ui::rich_text`] paragraph, returned by [`RichText::link`].
///
/// ```
/// # let mut open_settings = false;
/// # kargui::test::example(|ui| {
/// ui.rich_text(|t| {
///     t.text("Notifications are off. ");
///     if t.link("Turn them on").clicked() {
///         open_settings = true;
///     }
/// });
/// # });
/// ```
pub struct RichLink<'a> {
    span: Span<'a>,
    clicked: bool,
}

impl RichLink<'_> {
    /// Whether the link was clicked, or activated with Enter while it had the focus, since the
    /// last frame.
    ///
    /// ```
    /// # let mut count = 0;
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     if t.link("Show more").clicked() {
    ///         count += 10;
    ///     }
    /// });
    /// # });
    /// ```
    pub fn clicked(&self) -> bool {
        self.clicked
    }

    /// Draws the link bold.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.rich_text(|t| {
    ///     t.text("Update ready. ");
    ///     t.link("Restart now").bold();
    /// });
    /// # });
    /// ```
    pub fn bold(self) -> Self {
        let clicked = self.clicked;
        RichLink { span: self.span.bold(), clicked }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Text;

    const BASE: TextStyle = TextStyle { size: 14.0, bold: false, font: FontId(0) };

    fn span(start: usize, end: usize) -> SpanStyle {
        SpanStyle { start, end, ..Default::default() }
    }

    /// 並べ方が変わる部分（太字・大きさ・書体・斜体）だけが範囲になる。色や下線は描く時に付ける
    #[test]
    fn only_parts_that_change_the_shapes_become_runs() {
        let spans = [
            span(0, 3),
            SpanStyle { bold: true, ..span(3, 5) },
            SpanStyle { size: Some(TextKind::Custom(280)), ..span(5, 6) },
            SpanStyle { underline: true, color: Some([1.0, 0.0, 0.0, 1.0]), ..span(6, 8) },
            SpanStyle { italic: true, ..span(8, 9) },
            SpanStyle { font: Some(FontId(2)), ..span(9, 10) },
            SpanStyle { bold: true, ..span(10, 10) },
        ];
        let r = runs(&spans, BASE, 1.0);
        let got: Vec<(usize, usize)> = r.iter().map(|r| (r.start, r.end)).collect();
        assert_eq!(got, [(3, 5), (5, 6), (8, 9), (9, 10)], "空の部分と、色・下線だけの部分は入らない");
        assert!(r[0].style.bold && r[1].style.size == 28.0 && r[2].italic && r[3].style.font == FontId(2));
        // 拡大率 2 では、決めた大きさも 2 倍（物理ピクセル）
        let big = TextStyle { size: 28.0, ..BASE };
        assert_eq!(runs(&spans, big, 2.0)[1].style.size, 56.0);
    }

    #[test]
    fn span_at_finds_the_part_of_a_position() {
        let spans = [span(0, 3), span(3, 5), span(5, 5), span(5, 8)];
        let got: Vec<Option<usize>> = (0..=8).map(|p| span_at(&spans, p)).collect();
        assert_eq!(got, [Some(0), Some(0), Some(0), Some(1), Some(1), Some(3), Some(3), Some(3), None], "空の部分は飛ばす");
        assert_eq!(span_at(&[], 0), None);
    }

    /// 2 行にまたがる範囲は 2 つの区間になる（1 行目は範囲の頭から行の終わりまで、2 行目は行の頭から）
    #[test]
    fn segments_split_a_range_by_line() {
        let mut t = Text::new().unwrap();
        let s = "これは書式の付いた文字を折り返して確かめるための長い文です。";
        let laid = t.layout(s, BASE, Some(120.0));
        let s16: Vec<u16> = s.encode_utf16().collect();
        assert!(laid.lines.len() >= 3, "{:?}", laid.lines);
        let end = laid.lines[1].start + 2;
        let seg = segments(&laid, &s16, 2, end);
        assert_eq!(seg.iter().map(|s| s.0).collect::<Vec<_>>(), [0, 1]);
        assert!((seg[0].1 - laid.x_of(2)).abs() < 0.5, "{seg:?}");
        assert!((seg[0].2 - laid.lines[0].width).abs() < 0.5, "{seg:?}");
        assert!(seg[1].1.abs() < 0.5 && seg[1].2 > seg[1].1 + 10.0, "{seg:?}");
        // 1 行の中の範囲は 1 つの区間、空の範囲は区間なし
        assert_eq!(segments(&laid, &s16, 1, 3).len(), 1);
        assert!(segments(&laid, &s16, 3, 3).is_empty());
    }

    /// 折り返した行の終わりの空白には区間を作らない（リンクの下線が空白の下まで伸びない）。行の途中の空白は入れる
    #[test]
    fn segments_leave_out_the_space_hanging_at_a_line_end() {
        let mut t = Text::new().unwrap();
        let s = "Read how backups work";
        let s16: Vec<u16> = s.encode_utf16().collect();
        let one = t.layout(s, BASE, None);
        // 1 行なら、「how backups」の区間は語の間の空白を含めて 1 つ
        let mid = segments(&one, &s16, 5, 16);
        assert_eq!(mid.len(), 1);
        assert!((mid[0].2 - one.x_of(16)).abs() < 0.5, "{mid:?}");
        // 「how 」の後で折り返す幅にすると、1 行目の区間は「how」の右端で終わる
        let wrap_at = one.x_of(9) + 2.0;
        let two = t.layout(s, BASE, Some(wrap_at));
        assert!(two.lines.len() >= 2 && two.lines[0].end == 9, "{:?}", two.lines);
        let seg = segments(&two, &s16, 5, 21);
        assert!((seg[0].2 - two.x_of(8)).abs() < 0.5, "空白の手前で終わる: {seg:?} {}", two.x_of(8));
    }
}
