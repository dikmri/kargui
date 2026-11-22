//! 入力欄の編集の計算（画面を持たない純粋な処理）。
//!
//! 位置は UTF-8 のバイトの位置で、いつも文字の境目にある。
//! DirectWrite の当たり判定は UTF-16 の位置を使うので、変換の関数も置く。
//! 1 文字ずつ消す・動かす時は書記素（結合文字や、ZWJ でつないだ絵文字の並びを 1 つとした単位）で数える。

use unicode_segmentation::UnicodeSegmentation;

/// 入力欄 1 つの編集の状態。文字列そのものはアプリが持つ（&mut String で渡される）
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EditState {
    /// キャレットの位置
    pub cursor: usize,
    /// 選択の始まり（cursor と同じなら選択なし）
    pub anchor: usize,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// 続けて打った文字を 1 回の「元に戻す」にまとめるため、直前の操作が文字の入力だったか
    last_was_typing: bool,
    /// 複数行の入力欄（改行を入れる）
    pub multiline: bool,
    /// 上下の矢印で続けて移る時に保つ横の位置（物理ピクセル。ほかの操作で忘れる）
    pub goal_x: Option<f32>,
    /// この欄の書体（.font() で決めた物。作るたびに忘れ、.font() が入れ直す）
    pub font: Option<crate::text::FontId>,
}

/// UTF-16 の位置 i がある行の番号。折り返しの境目（前の行の終わり = 次の行の始め）は次の行にする
pub(crate) fn line_of(lines: &[crate::text::Line], i: usize) -> usize {
    lines.iter().rposition(|l| l.start <= i).unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    text: String,
    cursor: usize,
    anchor: usize,
}

/// 文字の種類（単語の区切りを決めるのに使う）
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Kanji,
    Hiragana,
    Katakana,
    Other,
}

fn class(c: char) -> Class {
    match c {
        c if c.is_whitespace() => Class::Space,
        '\u{3040}'..='\u{309F}' => Class::Hiragana,
        '\u{30A0}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9F}' => Class::Katakana,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' => Class::Kanji,
        c if c.is_alphanumeric() || c == '_' => Class::Word,
        _ => Class::Other,
    }
}

/// i より前の書記素の境目
fn prev_boundary(s: &str, i: usize) -> usize {
    s[..i].grapheme_indices(true).next_back().map_or(0, |(p, _)| p)
}

/// i より後の書記素の境目
fn next_boundary(s: &str, i: usize) -> usize {
    s[i..].graphemes(true).next().map_or(i, |g| i + g.len())
}

/// UTF-8 の位置を UTF-16 の位置にする
pub(crate) fn utf16_of(s: &str, byte: usize) -> usize {
    s[..byte.min(s.len())].encode_utf16().count()
}

/// パスワードの 1 文字の代わりに出す点（U+2022。● より小さく、Windows の標準のパスワードの欄に近い）
pub(crate) const DOT: char = '\u{2022}';

/// パスワードを画面に出す形（1 文字を 1 つの点にする）
pub(crate) fn mask(s: &str) -> String {
    DOT.to_string().repeat(s.chars().count())
}

/// n 文字目の前の UTF-8 の位置（パスワードの ● の位置から元の文字の位置へ）
pub(crate) fn byte_of_char(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(b, _)| b)
}

/// UTF-16 の位置を UTF-8 の位置にする（文字の途中なら手前の境目）
pub(crate) fn byte_of(s: &str, utf16: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= utf16 {
            return i;
        }
        n += c.len_utf16();
        if n > utf16 {
            return i;
        }
    }
    s.len()
}

impl EditState {
    pub fn selection(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }

    pub fn has_selection(&self) -> bool {
        self.cursor != self.anchor
    }

    /// アプリが文字列を書き換えたときに、位置を文字列の中に収める
    pub fn clamp(&mut self, s: &str) {
        let fix = |i: usize| {
            let mut i = i.min(s.len());
            while !s.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        self.cursor = fix(self.cursor);
        self.anchor = fix(self.anchor);
    }

    fn snapshot(&mut self, s: &str, typing: bool) {
        if typing && self.last_was_typing {
            return;
        }
        self.undo.push(Snapshot { text: s.to_string(), cursor: self.cursor, anchor: self.anchor });
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_was_typing = typing;
    }

    /// 選択している部分を消す。消したら true
    fn delete_selection(&mut self, s: &mut String) -> bool {
        let (a, b) = self.selection();
        if a == b {
            return false;
        }
        s.replace_range(a..b, "");
        self.cursor = a;
        self.anchor = a;
        true
    }

    /// 文字を入れる（選択していれば置き換える）。1 行の入力欄は改行を入れない。複数行の入力欄は改行を入れ、
    /// Windows の改行（CR LF）と古い Mac の改行（CR）は LF にそろえる
    pub fn insert(&mut self, s: &mut String, text: &str, typing: bool) -> bool {
        let text: String = if self.multiline {
            text.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            text.chars().filter(|c| *c != '\n' && *c != '\r').collect()
        };
        if text.is_empty() && !self.has_selection() {
            return false;
        }
        self.snapshot(s, typing && !self.has_selection());
        self.delete_selection(s);
        s.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.anchor = self.cursor;
        true
    }

    /// Backspace。選択していれば選択を消す
    pub fn backspace(&mut self, s: &mut String, word: bool) -> bool {
        if self.has_selection() {
            self.snapshot(s, false);
            return self.delete_selection(s);
        }
        if self.cursor == 0 {
            return false;
        }
        self.snapshot(s, false);
        let from = if word { self.word_left(s) } else { prev_boundary(s, self.cursor) };
        s.replace_range(from..self.cursor, "");
        self.cursor = from;
        self.anchor = from;
        true
    }

    /// Delete。選択していれば選択を消す
    pub fn delete(&mut self, s: &mut String, word: bool) -> bool {
        if self.has_selection() {
            self.snapshot(s, false);
            return self.delete_selection(s);
        }
        if self.cursor >= s.len() {
            return false;
        }
        self.snapshot(s, false);
        let to = if word { self.word_right(s) } else { next_boundary(s, self.cursor) };
        s.replace_range(self.cursor..to, "");
        true
    }

    fn word_left(&self, s: &str) -> usize {
        let mut i = self.cursor;
        // 手前の空白を飛ばし、同じ種類の文字が続く所の頭まで戻る
        while i > 0 && class(s[..i].chars().next_back().unwrap()) == Class::Space {
            i = prev_boundary(s, i);
        }
        if i == 0 {
            return 0;
        }
        let k = class(s[..i].chars().next_back().unwrap());
        while i > 0 && class(s[..i].chars().next_back().unwrap()) == k {
            i = prev_boundary(s, i);
        }
        i
    }

    fn word_right(&self, s: &str) -> usize {
        let mut i = self.cursor;
        if i >= s.len() {
            return s.len();
        }
        let k = class(s[i..].chars().next().unwrap());
        while i < s.len() && class(s[i..].chars().next().unwrap()) == k {
            i = next_boundary(s, i);
        }
        while i < s.len() && class(s[i..].chars().next().unwrap()) == Class::Space {
            i = next_boundary(s, i);
        }
        i
    }

    /// キャレットを動かす。extend なら選択を広げる
    pub fn move_left(&mut self, s: &str, word: bool, extend: bool) {
        self.last_was_typing = false;
        if self.has_selection() && !extend {
            let (a, _) = self.selection();
            self.cursor = a;
            self.anchor = a;
            return;
        }
        self.cursor = if word { self.word_left(s) } else { prev_boundary(s, self.cursor) };
        if !extend {
            self.anchor = self.cursor;
        }
    }

    pub fn move_right(&mut self, s: &str, word: bool, extend: bool) {
        self.last_was_typing = false;
        if self.has_selection() && !extend {
            let (_, b) = self.selection();
            self.cursor = b;
            self.anchor = b;
            return;
        }
        self.cursor = if word { self.word_right(s) } else { next_boundary(s, self.cursor) };
        if !extend {
            self.anchor = self.cursor;
        }
    }

    pub fn move_to(&mut self, i: usize, extend: bool) {
        self.last_was_typing = false;
        self.cursor = i;
        if !extend {
            self.anchor = i;
        }
    }

    pub fn select_all(&mut self, s: &str) {
        self.anchor = 0;
        self.cursor = s.len();
    }

    /// i の所の単語を選ぶ（同じ種類の文字が続く所。2 回押し）。i が終わりなら手前の単語
    pub fn select_word(&mut self, s: &str, i: usize) {
        self.last_was_typing = false;
        let i = i.min(s.len());
        let (next, prev) = (s[i..].chars().next().map(class), s[..i].chars().next_back().map(class));
        // 押した所が単語の後ろの端（次が空白か終わりで、手前が空白でない）なら、手前の単語を選ぶ
        let k = match (next, prev) {
            (Some(Class::Space) | None, Some(p)) if p != Class::Space => p,
            (Some(n), _) => n,
            (None, Some(p)) => p,
            (None, None) => {
                self.move_to(i, false);
                return;
            }
        };
        let (mut a, mut b) = (i, i);
        while a > 0 && class(s[..a].chars().next_back().unwrap()) == k {
            a = prev_boundary(s, a);
        }
        while b < s.len() && class(s[b..].chars().next().unwrap()) == k {
            b = next_boundary(s, b);
        }
        self.anchor = a;
        self.cursor = b;
    }

    /// i のある行を選ぶ（改行は含めない。複数行の欄の 3 回押し）
    pub fn select_line(&mut self, s: &str, i: usize) {
        self.last_was_typing = false;
        let i = i.min(s.len());
        self.anchor = s[..i].rfind('\n').map_or(0, |p| p + 1);
        self.cursor = s[i..].find('\n').map_or(s.len(), |p| i + p);
    }

    pub fn selected_text<'a>(&self, s: &'a str) -> &'a str {
        let (a, b) = self.selection();
        &s[a..b]
    }

    /// 元に戻す。戻したら true
    pub fn undo(&mut self, s: &mut String) -> bool {
        let Some(snap) = self.undo.pop() else { return false };
        self.redo.push(Snapshot { text: s.clone(), cursor: self.cursor, anchor: self.anchor });
        *s = snap.text;
        self.cursor = snap.cursor;
        self.anchor = snap.anchor;
        self.last_was_typing = false;
        true
    }

    /// やり直す。やり直したら true
    pub fn redo(&mut self, s: &mut String) -> bool {
        let Some(snap) = self.redo.pop() else { return false };
        self.undo.push(Snapshot { text: s.clone(), cursor: self.cursor, anchor: self.anchor });
        *s = snap.text;
        self.cursor = snap.cursor;
        self.anchor = snap.anchor;
        self.last_was_typing = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(_s: &str, i: usize) -> EditState {
        EditState { cursor: i, anchor: i, ..Default::default() }
    }

    #[test]
    fn double_click_selects_a_word_and_triple_click_a_line() {
        let s = "#FF8800 と 牛乳を買う";
        let mut e = EditState::default();
        let sel = |e: &EditState| &s[e.selection().0..e.selection().1];
        e.select_word(s, 3);
        assert_eq!(sel(&e), "FF8800", "# は単語に入らない");
        e.select_word(s, 0);
        assert_eq!(sel(&e), "#");
        e.select_word(s, "#FF8800".len());
        assert_eq!(sel(&e), "FF8800", "単語の後ろの端を押すと、手前の単語");
        e.select_word(s, "#FF8800 と ".len() + 3);
        assert_eq!(sel(&e), "牛乳", "漢字の続く所");
        e.select_word(s, s.len());
        assert_eq!(sel(&e), "う", "終わりなら手前の、同じ種類の文字が続く所（Ctrl+左右と同じ区切り。送り仮名と漢字は分かれる）");
        let t = "first line\nsecond line\nthird";
        let mut e = EditState::default();
        e.select_line(t, 14);
        assert_eq!(&t[e.selection().0..e.selection().1], "second line", "改行は含めない");
        e.select_line(t, t.len());
        assert_eq!(&t[e.selection().0..e.selection().1], "third");
        e.select_word("", 0);
        assert_eq!(e.selection(), (0, 0), "空の欄でも落ちない");
    }

    #[test]
    fn insert_replaces_selection_and_ignores_newlines() {
        let mut s = String::from("牛乳を買う");
        let mut e = at(&s, 0);
        e.anchor = "牛乳".len();
        e.cursor = 0;
        assert!(e.insert(&mut s, "豆乳\n", true));
        assert_eq!(s, "豆乳を買う");
        assert_eq!(e.cursor, "豆乳".len());
        assert!(!e.has_selection());
    }

    #[test]
    fn multiline_insert_keeps_newlines_and_makes_crlf_lf() {
        let mut s = String::from("ab");
        let mut e = EditState { cursor: 1, anchor: 1, multiline: true, ..Default::default() };
        assert!(e.insert(&mut s, "1\r\n2\r3\n", false));
        assert_eq!(s, "a1\n2\n3\nb");
        assert_eq!(e.cursor, "a1\n2\n3\n".len());
    }

    #[test]
    fn line_of_finds_the_line_of_a_position() {
        use crate::text::Line;
        let l = |start, end, top| Line { start, end, top, height: 10.0, width: 0.0 };
        // "ab\ncd\n" の行: [0, 2) [3, 5) [6, 6)
        let lines = [l(0, 2, 0.0), l(3, 5, 10.0), l(6, 6, 20.0)];
        assert_eq!(line_of(&lines, 0), 0);
        assert_eq!(line_of(&lines, 2), 0, "改行の手前は前の行");
        assert_eq!(line_of(&lines, 3), 1);
        assert_eq!(line_of(&lines, 5), 1);
        assert_eq!(line_of(&lines, 6), 2);
        // 折り返し（改行なし）の境目は、次の行の始め
        let wrapped = [l(0, 4, 0.0), l(4, 8, 10.0)];
        assert_eq!(line_of(&wrapped, 4), 1);
        assert_eq!(line_of(&wrapped, 8), 1);
    }

    #[test]
    fn backspace_delete_and_arrows_treat_a_grapheme_as_one_character() {
        // 家族の絵文字（4 人を ZWJ でつないだ 7 つの文字）、肌の色つきの手（2 つ）、結合文字の「か」＋「゛」（2 つ）
        let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
        let hand = "👍\u{1F3FD}";
        let ga = "か\u{3099}";
        let mut s = format!("a{family}{hand}{ga}");
        let mut e = at(&s, s.len());
        assert!(e.backspace(&mut s, false));
        assert_eq!(s, format!("a{family}{hand}"));
        assert!(e.backspace(&mut s, false));
        assert_eq!(s, format!("a{family}"));
        assert!(e.backspace(&mut s, false));
        assert_eq!(s, "a");
        // Delete と左右の移動も、並び全体を 1 文字として扱う（途中にキャレットを置かない）
        let mut s = format!("{family}b");
        let mut e = at(&s, 0);
        e.move_right(&s, false, false);
        assert_eq!(e.cursor, family.len());
        e.move_left(&s, false, false);
        assert_eq!(e.cursor, 0);
        assert!(e.delete(&mut s, false));
        assert_eq!(s, "b");
    }

    #[test]
    fn backspace_and_delete_by_char_and_word() {
        let mut s = String::from("資料 まとめる");
        let mut e = at(&s, s.len());
        assert!(e.backspace(&mut s, false));
        assert_eq!(s, "資料 まとめ");
        // 単語ごと: ひらがなの並びを消す
        assert!(e.backspace(&mut s, true));
        assert_eq!(s, "資料 ");
        // 空白を飛ばして漢字の並びを消す
        assert!(e.backspace(&mut s, true));
        assert_eq!(s, "");
        assert!(!e.backspace(&mut s, false));
        let mut s = String::from("abc def");
        let mut e = at(&s, 0);
        assert!(e.delete(&mut s, true));
        assert_eq!(s, "def");
    }

    #[test]
    fn moving_collapses_or_extends_the_selection() {
        let s = "あいう";
        let mut e = at(s, s.len());
        e.move_left(s, false, true);
        e.move_left(s, false, true);
        assert_eq!(e.selected_text(s), "いう");
        e.move_left(s, false, false);
        assert_eq!(e.cursor, "あ".len());
        assert!(!e.has_selection());
        e.move_right(s, true, false);
        assert_eq!(e.cursor, s.len());
        e.select_all(s);
        assert_eq!(e.selected_text(s), s);
    }

    #[test]
    fn typing_is_undone_in_one_step_and_redo_restores_it() {
        let mut s = String::new();
        let mut e = EditState::default();
        for c in ["a", "b", "c"] {
            e.insert(&mut s, c, true);
        }
        e.backspace(&mut s, false);
        assert_eq!(s, "ab");
        assert!(e.undo(&mut s));
        assert_eq!(s, "abc");
        assert!(e.undo(&mut s));
        assert_eq!(s, "");
        assert!(!e.undo(&mut s));
        assert!(e.redo(&mut s));
        assert_eq!(s, "abc");
    }

    #[test]
    fn utf16_and_byte_positions_convert_both_ways() {
        let s = "a資😀b";
        assert_eq!(utf16_of(s, 0), 0);
        assert_eq!(utf16_of(s, 1), 1);
        assert_eq!(utf16_of(s, 1 + 3), 2);
        assert_eq!(utf16_of(s, s.len()), 5);
        assert_eq!(byte_of(s, 2), 4);
        // 代用文字の途中は手前の境目
        assert_eq!(byte_of(s, 3), 4);
        assert_eq!(byte_of(s, 4), 8);
        assert_eq!(byte_of(s, 99), s.len());
    }

    #[test]
    fn clamp_keeps_positions_on_boundaries() {
        let mut e = at("", 10);
        e.clamp("資");
        assert_eq!(e.cursor, 3);
        let mut e = at("", 2);
        e.clamp("資");
        assert_eq!(e.cursor, 0);
    }
}
