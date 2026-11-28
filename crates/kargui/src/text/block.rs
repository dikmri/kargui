//! 入力欄の文字を、かたまりに分けて並べる（Text::layout_block）。打った時は、前の版と同じかたまりはそのまま使い、
//! 変わったかたまりだけを並べ直す。200 万字の欄で、1 字打つたびに全部を並べ直して約 0.9 秒かかった
//! （2026-11-28 の測定）。
//!
//! - 折り返す欄（複数行）: 改行の所でだけ分け、縦に積む。行の折り返しも、右から左へ書く文字の並び替えも改行を
//!   またがないので、分けて並べても全体を一度に並べた時と同じになる
//! - 折り返さない 1 行の欄: 約 PIECE バイトごとに切り、横に並べる。切れ目では字の形の合わせ（カーニング）が効かない
//!   ので、なるべく空白の後ろで切る。切ると並べ方が変わる字（右から左へ書く文字・結合する文字・絵文字・タブなど）を
//!   含む行は分けない
//! - LONG_TEXT より短い文字は分けない（今までどおり一度に並べる）

use std::rc::Rc;

use super::{LONG_TEXT, Laid, LayoutKey, Line, Text, TextStyle};

/// かたまりの大きさの目安（バイト）
const PIECE: usize = 4096;

/// 分け方
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// 分けない（短い文字と、切ると並べ方が変わる字を含む 1 行）
    Whole,
    /// 改行の所で分け、縦に積む
    Paragraphs,
    /// 1 行を切り、横に並べる
    Row,
}

/// 並べたかたまり 1 つ
struct Piece {
    /// 文字列の中の範囲（バイト）。かたまりの間の改行は入れない
    start: usize,
    end: usize,
    /// 頭の UTF-16 の位置と、UTF-16 の長さ
    start16: usize,
    len16: usize,
    /// 並べた範囲の左上からの位置
    x: f32,
    y: f32,
    /// 字の形が描かれる範囲（左, 上, 右, 下。かたまりの左上が原点）
    ink: [f32; 4],
    laid: Rc<Laid>,
}

impl Piece {
    fn new(start: usize, end: usize, start16: usize, len16: usize, laid: Rc<Laid>) -> Self {
        let ink = laid.chunks.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, c| [b[0].min(c[0]), b[1].min(c[1]), b[2].max(c[2]), b[3].max(c[3])]);
        Self { start, end, start16, len16, x: 0.0, y: 0.0, ink, laid }
    }

    /// 同じ並べた結果を、文字列の中で ds バイト・ds16（UTF-16）ずらした所に置く
    fn shifted(&self, ds: isize, ds16: isize) -> Self {
        let at = |v: usize, d: isize| (v as isize + d) as usize;
        Self { start: at(self.start, ds), end: at(self.end, ds), start16: at(self.start16, ds16), laid: self.laid.clone(), ..*self }
    }
}

/// かたまりに分けて並べた文字列。Laid と同じ問い（キャレット・行・点の位置）に答える
pub(crate) struct Block {
    text: String,
    mode: Mode,
    pieces: Vec<Piece>,
    /// 行（位置は全体の UTF-16、上端とベースラインは並べた範囲の上から）
    pub lines: Vec<Line>,
    pub width: f32,
    pub height: f32,
}

impl Block {
    /// 並べ終えたかたまりを置き、行と大きさを決める
    fn assemble(text: String, mode: Mode, mut pieces: Vec<Piece>) -> Self {
        let mut lines = Vec::new();
        let (mut width, mut height) = (0.0f32, 0.0f32);
        if mode == Mode::Row {
            // 横に並べ、ベースラインをそろえる
            let baseline = |p: &Piece| p.laid.lines.first().map_or(0.0, |l| l.baseline);
            let base = pieces.iter().map(baseline).fold(0.0, f32::max);
            for p in &mut pieces {
                p.x = width;
                p.y = base - baseline(p);
                width += p.laid.width;
                height = height.max(p.y + p.laid.height);
            }
            let len16 = pieces.last().map_or(0, |p| p.start16 + p.len16);
            lines.push(Line { start: 0, end: len16, top: 0.0, height, width, baseline: base });
        } else {
            for p in &mut pieces {
                p.y = height;
                for l in &p.laid.lines {
                    lines.push(Line { start: l.start + p.start16, end: l.end + p.start16, top: l.top + height, baseline: l.baseline + height, ..*l });
                }
                width = width.max(p.laid.width);
                height += p.laid.height;
            }
        }
        Self { text, mode, pieces, lines, width, height }
    }

    /// UTF-16 で index 文字目を持つかたまり（かたまりの終わりの位置は、そのかたまり）
    fn piece16(&self, index: usize) -> &Piece {
        &self.pieces[self.pieces.partition_point(|p| p.start16 <= index).saturating_sub(1)]
    }

    /// UTF-16 で index 文字目の前のカーソルの位置（x, 行の上端, 行の高さ）
    pub fn caret(&self, index: usize) -> (f32, f32, f32) {
        let p = self.piece16(index);
        let (x, top, h) = p.laid.caret(index.saturating_sub(p.start16).min(p.len16));
        (p.x + x, p.y + top, h)
    }

    /// UTF-16 で index 文字目の前の位置（並べた範囲の左からの x）
    pub fn x_of(&self, index: usize) -> f32 {
        let p = self.piece16(index);
        p.x + p.laid.x_of(index.saturating_sub(p.start16).min(p.len16))
    }

    /// 並べた範囲の左から x の位置に一番近い文字の境目（UTF-16 の位置。1 行の欄で使う）
    pub fn index_at(&self, x: f32) -> usize {
        let k = if self.mode == Mode::Row { self.pieces.partition_point(|p| p.x <= x).saturating_sub(1) } else { 0 };
        let p = &self.pieces[k];
        p.start16 + p.laid.index_at(x - p.x)
    }

    /// 点 (x, y) に一番近い文字の境目（UTF-16 の位置）
    pub fn index_at_point(&self, x: f32, y: f32) -> usize {
        if self.mode == Mode::Row {
            return self.index_at(x);
        }
        let p = &self.pieces[self.pieces.partition_point(|p| p.y <= y).saturating_sub(1)];
        p.start16 + p.laid.index_at_point(x - p.x, y - p.y)
    }

    /// 範囲（左, 上, 右, 下。並べた範囲の左上が原点）と字の形が重なるかたまりの、並べた結果と位置
    pub fn visible(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> impl Iterator<Item = (&Laid, f32, f32)> {
        self.pieces
            .iter()
            .filter(move |p| p.x + p.ink[0] < x1 && p.x + p.ink[2] > x0 && p.y + p.ink[1] < y1 && p.y + p.ink[3] > y0)
            .map(|p| (&*p.laid, p.x, p.y))
    }

    /// 試験用: 全部の字の形の四角（描く時と同じ丸め。x, y は並べた範囲の左上の位置）
    #[cfg(test)]
    pub fn glyph_boxes(&self, x: f32, y: f32) -> Vec<[f32; 4]> {
        self.pieces
            .iter()
            .flat_map(|p| {
                p.laid.glyphs.iter().map(move |g| {
                    [(x + p.x + g.x).round() + g.slot.left as f32, (y + p.y + g.y).round() + g.slot.top as f32, g.slot.w as f32, g.slot.h as f32]
                })
            })
            .collect()
    }
}

/// 1 行を切って横に並べてよい字（切れ目の前後で並べ方が変わらない字）。右から左へ書く文字・結合する文字・絵文字・
/// 字の幅の無い字・タブ・改行などは入れない
fn splittable(c: char) -> bool {
    matches!(c as u32,
        0x20..=0x7E // ASCII（タブと制御文字は入れない）
        | 0xA0..=0x24F // ラテン文字（結合する記号は入れない）
        | 0x2010..=0x2027 | 0x2030..=0x205E // 句読点（幅の無い空白・方向の印・結合子・行の区切りは入れない）
        | 0x3000..=0x3029 | 0x3030..=0x3096 | 0x309B..=0x30FF // 記号・ひらがな・カタカナ（結合する濁点と声調の印は入れない）
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF // 漢字
        | 0xAC00..=0xD7A3 // ハングル
        | 0xFF01..=0xFF9D | 0xFFA0..=0xFFEF // 全角・半角の形（半角の濁点は入れない）
    )
}

/// 2 つの文字列の、頭から同じバイト数と、終わりから同じバイト数（重ならないよう、合わせて短い方の長さまで）。
/// どちらも文字の境目にする
fn common(a: &str, b: &str) -> (usize, usize) {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    // 64 バイトずつ比べてから、違うかたまりの中を 1 バイトずつ
    let mut p = x.chunks(64).zip(y.chunks(64)).take_while(|(c, d)| c == d).count() * 64;
    p = p.min(x.len()).min(y.len());
    p += x[p..].iter().zip(&y[p..]).take_while(|(c, d)| c == d).count();
    while !a.is_char_boundary(p) {
        p -= 1;
    }
    let most = x.len().min(y.len()) - p;
    let (xs, ys) = (&x[x.len() - most..], &y[y.len() - most..]);
    let mut q = xs.rchunks(64).zip(ys.rchunks(64)).take_while(|(c, d)| c == d).count() * 64;
    q = q.min(most);
    q += xs[..most - q].iter().rev().zip(ys[..most - q].iter().rev()).take_while(|(c, d)| c == d).count();
    while !a.is_char_boundary(a.len() - q) || !b.is_char_boundary(b.len() - q) {
        q -= 1;
    }
    (p, q)
}

/// 文字列の [from, to) を、改行の所でかたまりに分ける（目安の PIECE バイトを超えた後の最初の改行で切る）。
/// 範囲は改行を入れない。[from, to) が改行で終わる時は、最後に空のかたまりがある
fn cut_paragraphs(s: &str, from: usize, to: usize) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut a = from;
    loop {
        let end = if a + PIECE < to { b[a + PIECE..to].iter().position(|&c| c == b'\n').map_or(to, |k| a + PIECE + k) } else { to };
        out.push((a, end));
        if end >= to {
            return out;
        }
        a = end + 1;
    }
}

/// 1 行の [from, to) を、PIECE バイトほどのかたまりに切る。なるべく空白の後ろで切る
fn cut_row(s: &str, from: usize, to: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut a = from;
    while a < to {
        let mut end = to;
        if to - a > PIECE {
            end = a + PIECE;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            let near = end.saturating_sub(PIECE / 8).max(a + 1);
            if let Some(k) = s.as_bytes()[near..end].iter().rposition(|&c| c == b' ') {
                end = near + k + 1;
            }
        }
        out.push((a, end));
        a = end;
    }
    out
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

impl Text {
    /// 入力欄の文字を並べる。長い文字はかたまりに分けて並べ、前の版と同じかたまりはそのまま使う（このモジュールの
    /// 説明）。結果は覚えておき、同じ物には同じ結果を返す
    pub fn layout_block(&mut self, s: &str, style: TextStyle, max_width: Option<f32>) -> Rc<Block> {
        let key = LayoutKey::new(s, style, max_width);
        if let Some(b) = self.blocks.get(&key) {
            return b;
        }
        let generation = self.atlas.generation;
        let block = Rc::new(self.make_block(s, style, max_width, &key));
        // 作る途中で形の表を作り直した（中の形の場所が古い）時は覚えない。次のフレームで作り直す
        if self.atlas.generation == generation {
            self.blocks.insert(key, block.clone());
        }
        block
    }

    fn make_block(&mut self, s: &str, style: TextStyle, max_width: Option<f32>, key: &LayoutKey) -> Block {
        let mode = if s.len() < LONG_TEXT {
            Mode::Whole
        } else if max_width.is_some() {
            Mode::Paragraphs
        } else if s.chars().all(splittable) {
            Mode::Row
        } else {
            Mode::Whole
        };
        if mode == Mode::Whole {
            let laid = self.layout(s, style, max_width);
            return Block::assemble(s.to_string(), mode, vec![Piece::new(0, s.len(), 0, utf16_len(s), laid)]);
        }
        let sep = usize::from(mode == Mode::Paragraphs);
        let ln = s.len();
        // 前の版: 同じ書式と幅で、頭と終わりが一番多く同じ物
        let old = self
            .blocks
            .similar(key)
            .filter(|b| b.mode == mode)
            .map(|b| {
                let (p, q) = common(&b.text, s);
                (p + q, p, q, b)
            })
            .max_by_key(|t| t.0)
            .map(|(_, p, q, b)| (p, q, b.clone()));
        // 前の版から、頭の側と終わりの側の同じかたまりを残す。間（mid_start..mid_end）は並べ直す
        let (mut front, mut back) = (Vec::new(), Vec::new());
        let (mut mid_start, mut mid_start16, mut mid_end) = (0, 0, ln);
        if let Some((p, q, old)) = &old {
            let (n, lo) = (old.pieces.len(), old.text.len());
            // 頭の側: 後ろの区切りまで同じかたまり（複数行では、最後のかたまりは後ろに区切りが無いので残さない）
            let kept_front = old.pieces.iter().enumerate().take_while(|(i, c)| if sep == 1 { i + 1 < n && c.end < *p } else { c.end <= *p }).count();
            // 終わりの側: 前の区切りから同じかたまり（複数行では、最初のかたまりは前に区切りが無いので残さない）
            let kept_back = old.pieces[kept_front..]
                .iter()
                .rev()
                .take_while(|c| if sep == 1 { c.start > lo - q } else { c.start >= lo - q })
                .count();
            if let Some(c) = old.pieces[..kept_front].last() {
                (mid_start, mid_start16) = (c.end + sep, c.start16 + c.len16 + sep);
            }
            front.extend(old.pieces[..kept_front].iter().map(|c| c.shifted(0, 0)));
            if kept_back > 0 {
                let first = &old.pieces[n - kept_back];
                mid_end = first.start - sep + ln - lo;
                let ds = ln as isize - lo as isize;
                let ds16 = (mid_start16 + utf16_len(&s[mid_start..mid_end]) + sep) as isize - first.start16 as isize;
                back.extend(old.pieces[n - kept_back..].iter().map(|c| c.shifted(ds, ds16)));
            }
        }
        // 間を分けて並べる
        let cuts = if mode == Mode::Paragraphs { cut_paragraphs(s, mid_start, mid_end) } else { cut_row(s, mid_start, mid_end) };
        let mut pieces = front;
        let mut at16 = mid_start16;
        for (a, b) in cuts {
            let len16 = utf16_len(&s[a..b]);
            let laid = Rc::new(self.shape(&s[a..b], style, max_width));
            pieces.push(Piece::new(a, b, at16, len16, laid));
            at16 += len16 + sep;
        }
        pieces.extend(back);
        Block::assemble(s.to_string(), mode, pieces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::FontId;

    const ST: TextStyle = TextStyle { size: 14.0, bold: false, font: FontId(0) };

    /// 段落の多い長い文字列（日本語・英字・空の行・続く空の行・最後の改行）
    fn paragraphs(n: usize) -> String {
        let mut s = String::new();
        for i in 0..n {
            match i % 9 {
                0 | 1 => {}
                3 => s.push_str("これは日本語の折り返しを確かめるための、少し長めの段落です。"),
                5 => s.push_str(&format!("line {i}: the quick brown fox jumps over the lazy dog")),
                _ => s.push_str(&format!("{i} 行目 abc")),
            }
            s.push('\n');
        }
        s
    }

    fn whole_boxes(laid: &Laid) -> Vec<[f32; 4]> {
        laid.glyphs.iter().map(|g| [g.x.round() + g.slot.left as f32, g.y.round() + g.slot.top as f32, g.slot.w as f32, g.slot.h as f32]).collect()
    }

    fn near(a: f32, b: f32, d: f32) -> bool {
        (a - b).abs() <= d
    }

    /// 全体を一度に並べた結果の x と比べる時に許す差。DirectWrite は x が大きいと f32 の誤差が溜まる（10 万字の 1 行の
    /// 終わりで 53 画素。かたまりごとに並べた方は、かたまりの中の小さい x で数えるので溜まりにくい）
    fn x_slack(x: f32) -> f32 {
        1.0 + x.abs() * 1e-4
    }

    /// 字の四角が、数も位置（y は 1 画素、x は x_slack まで）も同じ
    fn same_boxes(a: &[[f32; 4]], b: &[[f32; 4]]) {
        assert_eq!(a.len(), b.len(), "字の数");
        for (k, (p, q)) in a.iter().zip(b).enumerate() {
            assert!(near(p[0], q[0], x_slack(q[0])) && near(p[1], q[1], 1.0) && p[2] == q[2] && p[3] == q[3], "{k}: {p:?} {q:?}");
        }
    }

    /// 複数行の長い文字は改行の所で分けて並べ、全体を一度に並べた時と同じ行・キャレット・当たり・字の位置になる
    #[test]
    fn paragraphs_are_laid_out_in_pieces_like_the_whole_text() {
        let mut t = Text::new().unwrap();
        let s = paragraphs(1500);
        assert!(s.len() > LONG_TEXT * 2, "{}", s.len());
        let w = Some(260.0);
        let whole = t.layout(&s, ST, w);
        let block = t.layout_block(&s, ST, w);
        assert!(block.pieces.len() > 3, "分けて並べる: {}", block.pieces.len());
        assert_eq!(block.lines.len(), whole.lines.len());
        for (a, b) in block.lines.iter().zip(&whole.lines) {
            assert_eq!((a.start, a.end), (b.start, b.end));
            assert!(near(a.top, b.top, 0.5) && near(a.height, b.height, 0.5) && near(a.baseline, b.baseline, 0.5) && near(a.width, b.width, 0.5), "{a:?} {b:?}");
        }
        assert!(near(block.width, whole.width, 0.5) && near(block.height, whole.height, 0.5), "{} {} {} {}", block.width, whole.width, block.height, whole.height);
        let len16 = utf16_len(&s);
        for i in (0..=len16).step_by(53).chain([len16 - 1, len16]) {
            let (a, b) = (block.caret(i), whole.caret(i));
            assert!(near(a.0, b.0, 0.5) && near(a.1, b.1, 0.5) && near(a.2, b.2, 0.5), "{i}: {a:?} {b:?}");
        }
        for k in 0..300 {
            let (x, y) = ((k * 37 % 280) as f32, k as f32 * whole.height / 300.0);
            assert_eq!(block.index_at_point(x, y), whole.index_at_point(x, y), "({x}, {y})");
        }
        same_boxes(&block.glyph_boxes(0.0, 0.0), &whole_boxes(&whole));
    }

    /// 折り返さない長い 1 行は切って横に並べ、全体を一度に並べた時と同じ幅・位置・当たりになる。
    /// 切ると並べ方が変わる字を含む行は分けない
    #[test]
    fn a_long_line_is_cut_into_pieces_like_the_whole_line() {
        let mut t = Text::new().unwrap();
        let s = "abc 設定 xyz ".repeat(1500);
        let whole = t.layout(&s, ST, None);
        let block = t.layout_block(&s, ST, None);
        assert!(block.pieces.len() > 3, "切って並べる: {}", block.pieces.len());
        assert_eq!(block.lines.len(), 1);
        assert!(near(block.width, whole.width, x_slack(whole.width)) && near(block.height, whole.height, 0.5), "{} {} {} {}", block.width, whole.width, block.height, whole.height);
        let len16 = utf16_len(&s);
        for i in (0..=len16).step_by(97).chain([len16]) {
            assert!(near(block.x_of(i), whole.x_of(i), x_slack(whole.x_of(i))), "{i}: {} {}", block.x_of(i), whole.x_of(i));
        }
        // それぞれの x で、境目の 1 画素右を押すと同じ境目（2 つの x は f32 の誤差の分ずれるので、同じ x では比べない）
        for i in (0..len16).step_by(89) {
            assert_eq!(block.index_at(block.x_of(i) + 1.0), whole.index_at(whole.x_of(i) + 1.0), "{i}");
        }
        same_boxes(&block.glyph_boxes(0.0, 0.0), &whole_boxes(&whole));
        for odd in ["😀", "\t", "e\u{301}", "שלום", "\u{200d}"] {
            let b = t.layout_block(&format!("{s}{odd}"), ST, None);
            assert_eq!(b.pieces.len(), 1, "{odd:?} を含む行は分けない");
        }
        // 短い文字は分けない
        assert_eq!(t.layout_block("abc 設定", ST, None).pieces.len(), 1);
        assert_eq!(t.layout_block(&paragraphs(20), ST, Some(260.0)).pieces.len(), 1);
    }

    /// 打った時は、変わったかたまりだけを並べ直す。並べた結果は、全体を一度に並べた時と同じ行になる
    #[test]
    fn typing_lays_out_only_the_pieces_that_changed() {
        for (s, w) in [(paragraphs(1500), Some(260.0)), ("abc 設定 xyz ".repeat(3000), None)] {
            let mut t = Text::new().unwrap();
            t.layout_block(&s, ST, w);
            t.end_frame();
            // 文字列の k バイト目から後ろで、最初の文字の境目
            fn at(s: &str, k: usize) -> usize {
                (k..).find(|&k| s.is_char_boundary(k)).unwrap()
            }
            type Edit = Box<dyn Fn(&str) -> String>;
            let edits: Vec<Edit> = vec![
                // 真ん中に 1 字打つ・その字を消す・頭と終わりに足す・真ん中の 4000 バイトを消す・真ん中に改行を打つ
                Box::new(|s: &str| format!("{}X{}", &s[..at(s, s.len() / 2)], &s[at(s, s.len() / 2)..])),
                Box::new(|s: &str| format!("{}{}", &s[..at(s, s.len() / 2)], &s[at(s, at(s, s.len() / 2) + 1)..])),
                Box::new(|s: &str| format!("頭{s}")),
                Box::new(|s: &str| format!("{s}終わり")),
                Box::new(|s: &str| format!("{}{}", &s[..at(s, s.len() / 2)], &s[at(s, s.len() / 2 + 4000)..])),
                Box::new(|s: &str| if s.contains('\n') { format!("{}\n{}", &s[..at(s, s.len() / 3)], &s[at(s, s.len() / 3)..]) } else { format!("{s} ") }),
            ];
            let mut cur = s.clone();
            for (k, edit) in edits.iter().enumerate() {
                let next = edit(&cur);
                let before = t.bytes_laid;
                let block = t.layout_block(&next, ST, w);
                let laid = t.bytes_laid - before;
                let kind = if w.is_some() { "複数行" } else { "1 行" };
                assert!(laid <= 4 * PIECE as u64, "{kind}の {k} 番目の直し: {laid} バイト並べ直した（全体は {}）", next.len());
                let whole = t.layout(&next, ST, w);
                assert_eq!(block.lines.iter().map(|l| (l.start, l.end)).collect::<Vec<_>>(), whole.lines.iter().map(|l| (l.start, l.end)).collect::<Vec<_>>());
                assert!(near(block.height, whole.height, 0.5) && near(block.width, whole.width, x_slack(whole.width)), "{kind}の {k} 番目の直し: 高さ {} {}、幅 {} {}", block.height, whole.height, block.width, whole.width);
                t.end_frame();
                cur = next;
            }
        }
    }

    #[test]
    fn common_prefix_and_suffix_stop_at_character_boundaries() {
        assert_eq!(common("abcXdef", "abcYYdef"), (3, 3));
        assert_eq!(common("abc", "abc"), (3, 0));
        assert_eq!(common("ab", "abab"), (2, 0));
        // 「設」と「説」は頭の 2 バイトが同じ。文字の途中で止めない
        assert_eq!(common("a設b", "a説b"), (1, 1));
        let long = "x".repeat(200);
        assert_eq!(common(&format!("{long}1{long}"), &format!("{long}22{long}")), (200, 200));
    }
}
