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

    /// 文字を入れる（選択していれば置き換える）。改行は入れない（1 行の入力欄）
    pub fn insert(&mut self, s: &mut String, text: &str, typing: bool) -> bool {
        let text: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
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
