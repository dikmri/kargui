//! IME の問い合わせ（WM_IME_REQUEST）に答える中身を作る、画面を持たない処理。
//! 窓の処理（platform.rs）が、描いた時点の入力欄の写し（ImeField）から答えを作る。
//!
//! - 文字の位置（IMR_QUERYCHARPOSITION）: 変換中の文字の境目ごとの x を描く時に覚えておき、そのまま答える
//! - 前後の文字（IMR_DOCUMENTFEED）: 入力欄の文字（変換中の文字をキャレットの位置に入れた物）と変換中の範囲。
//!   IME はこれを見て、前後の文に合った候補を選ぶ
//! - 再変換（IMR_RECONVERTSTRING と IMR_CONFIRMRECONVERTSTRING）: 選んでいる文字を変換し直す。
//!   IME が範囲を直したら、その範囲を選び直す
//!
//! 位置はすべて UTF-16 の単位（IME の決まり）。

use crate::ui::Id;
use crate::ui::edit::{byte_of, utf16_of};

/// 焦点のある入力欄の、描いた時点の写し
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ImeField {
    pub id: Id,
    /// 入力欄の文字（変換中の文字は入れない）
    pub value: String,
    /// キャレットと選択の始まり（UTF-8 の位置）
    pub cursor: usize,
    pub anchor: usize,
    /// 変換中の文字の UTF-16 の境目ごとの x（窓の座標、物理ピクセル）。境目は文字数 + 1 個。変換中でなければ空
    pub preedit_xs: Vec<f32>,
}

/// RECONVERTSTRING に入れる中身（位置と長さは UTF-16 の単位）
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reconvert {
    pub text: Vec<u16>,
    /// 変換する範囲（始まり, 長さ）
    pub comp: (usize, usize),
    /// 注目している範囲（始まり, 長さ）
    pub target: (usize, usize),
}

/// 変換中の文字の文節（位置は変換中の文字の中の UTF-16 の位置）
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Clauses {
    /// 文節の境目。先頭は 0、最後は文字数
    pub bounds: Vec<usize>,
    /// 今変換している文節（始まり, 終わり）
    pub target: Option<(usize, usize)>,
    /// キャレットの位置（IME が示した位置。無ければ変換中の文字の終わり）
    pub cursor: Option<usize>,
}

/// GCS_COMPATTR（文字ごとの属性）から、今変換している文節の範囲（始まり, 終わり）を取り出す。
/// 属性 1（ATTR_TARGET_CONVERTED）と 3（ATTR_TARGET_NOTCONVERTED）が変換中の文節
pub(crate) fn target_range(attrs: &[u8]) -> Option<(usize, usize)> {
    let is_target = |a: &u8| *a == 1 || *a == 3;
    let start = attrs.iter().position(is_target)?;
    let end = attrs.iter().rposition(is_target)? + 1;
    Some((start, end))
}

/// GCS_COMPCLAUSE（境目の並び）を確かめる。先頭が 0 で、増え続け、最後が文字数なら使い、そうでなければ全体を 1 つの文節にする
pub(crate) fn clause_bounds(raw: &[u32], len: usize) -> Vec<usize> {
    let v: Vec<usize> = raw.iter().map(|&x| x as usize).collect();
    let ok = v.len() >= 2 && v[0] == 0 && *v.last().unwrap() == len && v.windows(2).all(|w| w[0] < w[1]);
    if ok { v } else { vec![0, len] }
}

/// RECONVERTSTRING の頭の大きさ（DWORD が 8 個）
pub(crate) const HEADER: usize = 32;

impl Reconvert {
    /// 頭と文字（終わりの 0 を含む）を合わせた大きさ（バイト）
    pub fn size(&self) -> usize {
        HEADER + (self.text.len() + 1) * 2
    }
}

/// 変換中の dwCharPos 文字目の左の x。覚えた境目が無ければ None
pub(crate) fn char_x(f: &ImeField, char_pos: usize) -> Option<f32> {
    f.preedit_xs.get(char_pos).or(f.preedit_xs.last()).copied()
}

/// IMR_DOCUMENTFEED の答え
pub(crate) fn document_feed(f: &ImeField, preedit: &str) -> Reconvert {
    let at = f.cursor.min(f.value.len());
    let shown = format!("{}{}{}", &f.value[..at], preedit, &f.value[at..]);
    let start = utf16_of(&f.value, at);
    let len = preedit.encode_utf16().count();
    Reconvert { text: shown.encode_utf16().collect(), comp: (start, len), target: (start, len) }
}

/// IMR_RECONVERTSTRING の答え。選んでいる文字が無い時と、変換中の時は答えない
pub(crate) fn reconvert(f: &ImeField, preedit: &str) -> Option<Reconvert> {
    if !preedit.is_empty() || f.cursor == f.anchor {
        return None;
    }
    let (a, b) = (f.cursor.min(f.anchor), f.cursor.max(f.anchor));
    let start = utf16_of(&f.value, a);
    let len = utf16_of(&f.value, b) - start;
    Some(Reconvert { text: f.value.encode_utf16().collect(), comp: (start, len), target: (start, len) })
}

/// IMR_CONFIRMRECONVERTSTRING で IME が決めた範囲（始まりはバイト、長さは文字数）を、
/// 入力欄の文字の中の UTF-8 の範囲にする（文字の途中にはしない）
pub(crate) fn confirmed_range(value: &str, comp_offset_bytes: u32, comp_len: u32) -> (usize, usize) {
    let start = comp_offset_bytes as usize / 2;
    let end = start + comp_len as usize;
    (byte_of(value, start), byte_of(value, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_range_finds_the_clause_being_converted() {
        // 「明日は|東京に|行く」で 2 つ目の文節を変換中。属性 2 = 変換済み、1 = 変換中の文節
        assert_eq!(target_range(&[2, 2, 2, 1, 1, 1, 2, 2]), Some((3, 6)));
        // 変換の前（かなのまま、属性 0）は変換中の文節が無い
        assert_eq!(target_range(&[0, 0, 0]), None);
        // まだ変換していない文節を選んでいる（属性 3）
        assert_eq!(target_range(&[3, 3, 0]), Some((0, 2)));
    }

    #[test]
    fn clause_bounds_fall_back_to_one_clause_when_broken() {
        assert_eq!(clause_bounds(&[0, 3, 6, 8], 8), vec![0, 3, 6, 8]);
        // 終わりが文字数と合わない、増えていない、空 → 全体を 1 つの文節にする
        assert_eq!(clause_bounds(&[0, 3, 9], 8), vec![0, 8]);
        assert_eq!(clause_bounds(&[0, 3, 3, 8], 8), vec![0, 8]);
        assert_eq!(clause_bounds(&[], 4), vec![0, 4]);
    }

    fn field(value: &str, cursor: usize, anchor: usize) -> ImeField {
        ImeField { id: Id(1), value: value.into(), cursor, anchor, preedit_xs: Vec::new() }
    }

    fn s(units: &[u16]) -> String {
        String::from_utf16(units).unwrap()
    }

    #[test]
    fn char_position_uses_each_boundary() {
        let mut f = field("", 0, 0);
        assert_eq!(char_x(&f, 0), None);
        f.preedit_xs = vec![100.0, 114.0, 130.0, 146.0];
        assert_eq!(char_x(&f, 0), Some(100.0));
        assert_eq!(char_x(&f, 2), Some(130.0));
        // 範囲の外を聞かれたら、最後の境目（変換中の文字の右の端）
        assert_eq!(char_x(&f, 9), Some(146.0));
    }

    #[test]
    fn document_feed_puts_the_composition_at_the_caret() {
        // 「明日は」と「に行く」の間で「とうきょう」を変換中
        let value = "明日はに行く";
        let f = field(value, "明日は".len(), "明日は".len());
        let r = document_feed(&f, "とうきょう");
        assert_eq!(s(&r.text), "明日はとうきょうに行く");
        assert_eq!(r.comp, (3, 5));
        assert_eq!(r.target, (3, 5));
        // 変換中でなければ、キャレットの位置で長さ 0
        let r = document_feed(&f, "");
        assert_eq!(s(&r.text), value);
        assert_eq!(r.comp, (3, 0));
    }

    #[test]
    fn document_feed_counts_surrogate_pairs_as_two() {
        // 𠮷（サロゲートペア）の後ろにキャレット
        let f = field("𠮷野家", "𠮷".len(), "𠮷".len());
        let r = document_feed(&f, "の");
        assert_eq!(r.comp, (2, 1));
        assert_eq!(r.size(), HEADER + (5 + 1) * 2);
    }

    #[test]
    fn reconvert_returns_the_selection_only_when_there_is_one() {
        let value = "牛乳を買う";
        assert_eq!(reconvert(&field(value, 0, 0), ""), None);
        // 「牛乳」を後ろから選んだ（キャレットが前）
        let f = field(value, 0, "牛乳".len());
        let r = reconvert(&f, "").unwrap();
        assert_eq!(s(&r.text), value);
        assert_eq!(r.comp, (0, 2));
        // 変換中は答えない
        assert_eq!(reconvert(&f, "か"), None);
    }

    #[test]
    fn confirmed_range_turns_bytes_into_text_positions() {
        let value = "牛乳を買う";
        // IME が「牛乳を」（3 文字）に広げた。始まりは 0 バイト
        assert_eq!(confirmed_range(value, 0, 3), (0, "牛乳を".len()));
        // 「買う」（始まりは 3 文字目 = 6 バイト）
        assert_eq!(confirmed_range(value, 6, 2), ("牛乳を".len(), value.len()));
        // 長すぎる範囲は終わりで止まる
        assert_eq!(confirmed_range(value, 6, 99), ("牛乳を".len(), value.len()));
    }
}
