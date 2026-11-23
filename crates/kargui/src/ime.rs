//! IME の問い合わせ（WM_IME_REQUEST）に答える中身を作る、画面を持たない処理。
//! 窓の処理（platform.rs）が、描いた時点の入力欄の写し（ImeField）から答えを作る。
//!
//! - 文字の位置（IMR_QUERYCHARPOSITION）: 変換中の文字の境目ごとの x を描く時に覚えておき、そのまま答える
//! - 前後の文字（IMR_DOCUMENTFEED）: 入力欄の文字（変換中の文字をキャレットの位置に入れた物）と変換中の範囲。
//!   IME はこれを見て、前後の文に合った候補を選ぶ。変換中でなく選んでいる文字があれば、その範囲を答える
//! - 再変換（IMR_RECONVERTSTRING と IMR_CONFIRMRECONVERTSTRING）: 選んでいる文字を変換し直す。
//!   IME が範囲を直したら、その範囲を選び直す
//!
//! 位置はすべて UTF-16 の単位（IME の決まり）。

use crate::input::Event;
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

/// Wayland の text-input-v3 の preedit_string（範囲は変換中の文字の中の UTF-8 のバイト。-1 はカーソルを出さない）を文節にする。
/// v3 には文節の区切りが無いので、IME が強調した範囲（始まり < 終わり）を今変換している文節とし、その前と後を別の文節にする。
/// 始まり = 終わりなら、そこがカーソルの位置
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn wayland_clauses(text: &str, begin: i32, end: i32) -> Clauses {
    let len = text.encode_utf16().count();
    if begin < 0 || end < 0 {
        return Clauses { bounds: vec![0, len], target: None, cursor: None };
    }
    // バイトの位置を UTF-16 の位置にする（文字の途中・長すぎる位置は、手前の文字の境目に寄せる）
    let at = |b: i32| {
        let mut b = (b as usize).min(text.len());
        while !text.is_char_boundary(b) {
            b -= 1;
        }
        utf16_of(text, b)
    };
    let (a, b) = (at(begin.min(end)), at(begin.max(end)));
    if a == b {
        return Clauses { bounds: vec![0, len], target: None, cursor: Some(a) };
    }
    let mut bounds = vec![0, a, b, len];
    bounds.dedup();
    Clauses { bounds, target: Some((a, b)), cursor: Some(b) }
}

/// text-input-v3 の done で、たまった変換中の文字と確定の文字を kargui の入力にする。
/// `shown` は今見せている変換中の文字。確定を先に渡し、それから新しい変換中の文字を渡す（v3 の決まりの順）。
/// done で変換中の文字が届かなかったら、変換中の文字は空になった
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn text_input_done(shown: &str, preedit: &str, begin: i32, end: i32, commit: &str) -> Vec<Event> {
    let mut out = Vec::new();
    if !commit.is_empty() {
        out.push(Event::ImePreedit(String::new()));
        out.push(Event::ImeCommit(commit.to_string()));
    }
    if !preedit.is_empty() {
        let Clauses { bounds, target, cursor } = wayland_clauses(preedit, begin, end);
        out.push(Event::ImePreedit(preedit.to_string()));
        out.push(Event::ImeClauses { bounds, target, cursor });
    } else if commit.is_empty() && !shown.is_empty() {
        out.push(Event::ImePreedit(String::new()));
    }
    out
}

/// fcitx5 の書式の旗の「強調」（TextFormatFlag::HighLight）。今変換している文節に付く
const FCITX_HIGHLIGHT: i32 = 1 << 4;

/// fcitx5 の D-Bus の UpdateFormattedPreedit（書式付きの区切りの並びと、変換中の文字の中の UTF-8 のバイトのカーソル。
/// 負ならカーソルを出さない）を、変換中の文字と文節にする。区切りの境目を文節の境目にし、強調の付いた区切りを
/// 今変換している文節にする
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn fcitx_clauses(segments: &[(String, i32)], cursor: i32) -> (String, Clauses) {
    let text: String = segments.iter().map(|(t, _)| t.as_str()).collect();
    let len = text.encode_utf16().count();
    let mut bounds = vec![0];
    let mut target: Option<(usize, usize)> = None;
    let mut at = 0;
    for (t, flags) in segments {
        let end = at + t.encode_utf16().count();
        if flags & FCITX_HIGHLIGHT != 0 && end > at {
            target = Some(target.map_or((at, end), |(a, b)| (a.min(at), b.max(end))));
        }
        bounds.push(end);
        at = end;
    }
    bounds.dedup();
    if bounds.len() < 2 {
        bounds = vec![0, len];
    }
    let cursor = (cursor >= 0).then(|| {
        let mut b = (cursor as usize).min(text.len());
        while !text.is_char_boundary(b) {
            b -= 1;
        }
        utf16_of(&text, b)
    });
    (text, Clauses { bounds, target, cursor })
}

/// fcitx5 の UpdateFormattedPreedit を kargui の入力にする。shown は今見せている変換中の文字。
/// 変換中の文字が空になったら（取り消し・確定の後）、見せていた時だけ消す
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn fcitx_preedit_events(shown: &str, segments: &[(String, i32)], cursor: i32) -> Vec<Event> {
    let (text, Clauses { bounds, target, cursor }) = fcitx_clauses(segments, cursor);
    if text.is_empty() {
        return if shown.is_empty() { Vec::new() } else { vec![Event::ImePreedit(String::new())] };
    }
    vec![Event::ImePreedit(text), Event::ImeClauses { bounds, target, cursor }]
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

/// IMR_DOCUMENTFEED の答え。変換中でなく選んでいる文字があれば、その範囲を変換する範囲として知らせる
/// （Chromium と同じ。Google 日本語入力はこの答えから再変換する文字を知る）
pub(crate) fn document_feed(f: &ImeField, preedit: &str) -> Reconvert {
    if let Some(r) = reconvert(f, preedit) {
        return r;
    }
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
    fn document_feed_tells_the_selection_when_not_composing() {
        // 「牛乳」を選んでいる。IME（Google 日本語入力など）はこの答えから再変換する文字を知る
        let value = "牛乳を買う";
        let f = field(value, "牛乳".len(), 0);
        let r = document_feed(&f, "");
        assert_eq!(s(&r.text), value);
        assert_eq!(r.comp, (0, 2));
        assert_eq!(r.target, (0, 2));
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

    #[test]
    fn fcitx_segments_become_clauses_and_the_highlight_is_the_target() {
        let seg = |t: &str, f: i32| (t.to_string(), f);
        // fcitx5 の UpdateFormattedPreedit: 「明日は|東京に|行く」の「東京に」を変換中（下線 8、強調 16）。カーソルは 18 バイト目
        let segs = [seg("明日は", 8), seg("東京に", 8 | 16), seg("行く", 8)];
        let (text, c) = fcitx_clauses(&segs, 18);
        assert_eq!(text, "明日は東京に行く");
        assert_eq!(c, Clauses { bounds: vec![0, 3, 6, 8], target: Some((3, 6)), cursor: Some(6) });
        // かなを打っている途中（文節 1 つ、強調なし）。カーソルは「あい|う」
        let (_, c) = fcitx_clauses(&[seg("あいう", 8)], 6);
        assert_eq!(c, Clauses { bounds: vec![0, 3], target: None, cursor: Some(2) });
        // カーソルを出さない（-1）・文字の途中の位置・空の区切りも止まらない
        let (_, c) = fcitx_clauses(&[seg("", 8), seg("😀あ", 16)], -1);
        assert_eq!(c, Clauses { bounds: vec![0, 3], target: Some((0, 3)), cursor: None });
        let (_, c) = fcitx_clauses(&[seg("あいう", 8)], 4);
        assert_eq!(c.cursor, Some(1));
    }

    #[test]
    fn fcitx_preedit_events_clear_an_empty_composition_once() {
        let seg = |t: &str, f: i32| (t.to_string(), f);
        let ev = fcitx_preedit_events("", &[seg("あ", 8)], 3);
        assert_eq!(ev, vec![Event::ImePreedit("あ".into()), Event::ImeClauses { bounds: vec![0, 1], target: None, cursor: Some(1) }]);
        // 変換中の文字が空になった（取り消し）。見せていた時だけ消す
        assert_eq!(fcitx_preedit_events("あ", &[], 0), vec![Event::ImePreedit(String::new())]);
        assert_eq!(fcitx_preedit_events("", &[], 0), Vec::<Event>::new());
    }

    #[test]
    fn wayland_clauses_turn_the_highlight_into_the_clause_being_converted() {
        // 「明日は東京に行く」の「東京に」を変換中（1 文字 3 バイト。9..18 バイト = 3..6 文字目）
        let c = wayland_clauses("明日は東京に行く", 9, 18);
        assert_eq!(c, Clauses { bounds: vec![0, 3, 6, 8], target: Some((3, 6)), cursor: Some(6) });
        // 全体を変換中なら、文節は 1 つ
        let c = wayland_clauses("東京", 0, 6);
        assert_eq!(c, Clauses { bounds: vec![0, 2], target: Some((0, 2)), cursor: Some(2) });
    }

    #[test]
    fn wayland_clauses_show_a_line_cursor_when_begin_equals_end() {
        // かなを打っている途中（変換の前）。カーソルは「あい|う」
        let c = wayland_clauses("あいう", 6, 6);
        assert_eq!(c, Clauses { bounds: vec![0, 3], target: None, cursor: Some(2) });
    }

    #[test]
    fn wayland_clauses_hide_the_cursor_for_minus_one() {
        let c = wayland_clauses("あいう", -1, -1);
        assert_eq!(c, Clauses { bounds: vec![0, 3], target: None, cursor: None });
    }

    #[test]
    fn wayland_clauses_count_in_utf16_and_stay_on_character_boundaries() {
        // 😀 は 4 バイトで UTF-16 の 2 単位
        let c = wayland_clauses("😀あ", 4, 4);
        assert_eq!(c.cursor, Some(2));
        // 文字の途中や長すぎる位置は、文字の境目に寄せる（IME の誤りで止まらない）
        let c = wayland_clauses("あいう", 1, 99);
        assert_eq!(c, Clauses { bounds: vec![0, 3], target: Some((0, 3)), cursor: Some(3) });
        // 前後が逆でも並べ直す
        let c = wayland_clauses("あいう", 6, 3);
        assert_eq!(c.target, Some((1, 2)));
    }

    #[test]
    fn text_input_done_sends_the_commit_before_the_new_composition() {
        // 前の変換を確定して、続けて次の変換が始まった
        let ev = text_input_done("とうきょう", "あ", 3, 3, "東京");
        assert_eq!(
            ev,
            vec![
                Event::ImePreedit(String::new()),
                Event::ImeCommit("東京".into()),
                Event::ImePreedit("あ".into()),
                Event::ImeClauses { bounds: vec![0, 1], target: None, cursor: Some(1) },
            ]
        );
    }

    #[test]
    fn text_input_done_clears_the_composition_only_when_one_is_shown() {
        // 変換中の文字を全部消した（Backspace）
        assert_eq!(text_input_done("あ", "", 0, 0, ""), vec![Event::ImePreedit(String::new())]);
        // 何も見せていない時に空の知らせが来ても、何も渡さない
        assert_eq!(text_input_done("", "", 0, 0, ""), vec![]);
        // 確定だけ（直接入力の 1 文字など）
        assert_eq!(text_input_done("", "", 0, 0, "a"), vec![Event::ImePreedit(String::new()), Event::ImeCommit("a".into())]);
    }
}
