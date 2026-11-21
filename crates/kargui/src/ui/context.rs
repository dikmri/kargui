//! フレームをまたいで持つ状態と、入力の振り分け。
//!
//! クリックがどの部品に当たったかは、前のフレームの配置（ユーザーが見ていた画面）で決める。
//! 入力はビューの 1 回目（入力を渡す回）で部品に届き、2 回目（描く回）は入力なしで描く。

use std::collections::HashMap;

use crate::geom::Rect;
use crate::input::{A11yAction, Event, Key, Mods};
use crate::log;
use crate::text::Text;
use crate::theme::Theme;
use crate::ui::Id;
use crate::ime::{Clauses, ImeField};
use crate::ui::edit::EditState;

/// クリップボードの読み書き（窓では OS のもの、試験台では手元の文字列）
pub(crate) trait Clipboard {
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: &str);
}

/// 試験台で使う、手元の文字列だけのクリップボード
#[derive(Default)]
pub(crate) struct MemoryClipboard(pub Option<String>);

impl Clipboard for MemoryClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }
    fn set(&mut self, text: &str) {
        self.0 = Some(text.to_string());
    }
}

/// 押したり焦点を受けたりできる部品の、前のフレームでの場所
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub id: Id,
    pub rect: Rect,
    /// 当たりの余白を含めた範囲（小さい部品だけ rect より広い）
    pub slop: Rect,
    pub focusable: bool,
    pub text_input: bool,
}

/// 点 (x, y) から四角までの距離の 2 乗（中なら 0）
fn distance2(r: &Rect, x: f32, y: f32) -> f32 {
    let dx = (r.x - x).max(0.0).max(x - r.right());
    let dy = (r.y - y).max(0.0).max(y - r.bottom());
    dx * dx + dy * dy
}

/// 一覧の、前のフレームでの場所と中身の高さ（物理ピクセル）
#[derive(Clone, Debug)]
pub(crate) struct ListInfo {
    pub id: Id,
    pub rect: Rect,
    pub content_h: f32,
    /// スクロールの棒のつまみ（中身が一覧より高いときだけ）
    pub thumb: Option<Rect>,
    /// スクロールの棒を押せる範囲（つまみの列。一覧の高さ全部）
    pub bar: Option<Rect>,
}

/// つかんでいるつまみ
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollDrag {
    pub list: Id,
    /// つかんだ所の、つまみの上の端からの距離（物理ピクセル）
    pub grab: f32,
    /// つかんでから届いた移動の数（記録用）
    pub moves: u32,
}

/// 前のフレームの配置
#[derive(Default)]
pub(crate) struct Prev {
    /// 描いた順。後ろほど上に重なる
    pub hits: Vec<Hit>,
    /// Tab で移る順
    pub focus_order: Vec<Id>,
    pub lists: Vec<ListInfo>,
    /// 入力欄の場所と、文字の左端の x（物理ピクセル）
    pub inputs: HashMap<Id, (Rect, f32)>,
    /// 一覧の行の中の、焦点を持てる部品 → (一覧の id, 行の範囲)
    pub rows: HashMap<Id, (Id, Rect)>,
}

pub(crate) struct Context {
    pub scale: f32,
    /// 窓の中の大きさ（物理ピクセル）
    pub width: f32,
    pub height: f32,
    pub dark: bool,
    pub theme: Theme,
    pub text: Text,
    pub prev: Prev,
    pub edits: HashMap<Id, EditState>,
    /// 入力欄の横のスクロール（物理ピクセル）
    pub edit_scroll: HashMap<Id, f32>,
    /// 一覧のスクロールの位置（論理ピクセル）
    pub scrolls: HashMap<Id, f32>,
    pub focus: Option<Id>,
    pub hover: Option<Id>,
    pub pressed: Option<Id>,
    /// ポインターが乗っているスクロールの棒（一覧の id）
    pub bar_hover: Option<Id>,
    /// つかんでいるスクロールの棒のつまみ
    pub drag: Option<ScrollDrag>,
    /// キーボードで焦点を動かしたあとだけ、焦点の輪を描く
    pub focus_visible: bool,
    // ---- この回の入力 ----
    pub clicked: Option<Id>,
    /// 焦点のある入力欄へ届ける入力
    pub keys: Vec<Event>,
    /// 入力欄の中のクリック（場所と Shift）
    pub pointer_text: Option<(Id, f32, f32, bool)>,
    /// 変換中の文字（焦点のある入力欄の中に描く）
    pub preedit: String,
    /// 変換中の文字の文節（IME が知らせた時だけ）
    pub preedit_clauses: Option<Clauses>,
    /// 焦点が移る前に、元の入力欄へ確定する変換中の文字（入力欄の id と文字）
    pub pending_commit: Option<(Id, String)>,
    /// IME の変換を取り消すように頼む（変換中の文字をこちらで確定した時）。窓の処理が読んで戻す
    pub ime_cancel: bool,
    pub want_frame: bool,
    pub clipboard: Box<dyn Clipboard>,
    // ---- 描いた結果 ----
    /// 文字を打つ位置と、変換中の文字の範囲（物理ピクセル）。IME に知らせる
    pub caret: Option<Rect>,
    pub preedit_rect: Option<Rect>,
    pub input_rect: Option<Rect>,
    /// 焦点のある入力欄の写し（IME の問い合わせに答えるため）
    pub ime_field: Option<ImeField>,
}

impl Context {
    pub fn new(text: Text, dark: bool, clipboard: Box<dyn Clipboard>) -> Self {
        Self {
            scale: 1.0,
            width: 0.0,
            height: 0.0,
            dark,
            theme: if dark { Theme::dark() } else { Theme::light() },
            text,
            prev: Prev::default(),
            edits: HashMap::new(),
            edit_scroll: HashMap::new(),
            scrolls: HashMap::new(),
            focus: None,
            hover: None,
            pressed: None,
            bar_hover: None,
            drag: None,
            focus_visible: false,
            clicked: None,
            keys: Vec::new(),
            pointer_text: None,
            preedit: String::new(),
            preedit_clauses: None,
            pending_commit: None,
            ime_cancel: false,
            want_frame: false,
            clipboard,
            caret: None,
            preedit_rect: None,
            input_rect: None,
            ime_field: None,
        }
    }

    pub fn set_dark(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.theme = if dark { Theme::dark() } else { Theme::light() };
        }
    }

    /// 前のフレームで (x, y) にあった、押せる部品（上に重なっている物）。
    /// 本来の範囲で見つからなければ、当たりの余白の中で一番近い部品にする（隣の部品の範囲は奪わない）
    pub fn hit(&self, x: f32, y: f32) -> Option<&Hit> {
        if let Some(h) = self.prev.hits.iter().rev().find(|h| h.rect.contains(x, y)) {
            return Some(h);
        }
        self.prev
            .hits
            .iter()
            .filter(|h| h.slop.contains(x, y))
            .min_by(|a, b| distance2(&a.rect, x, y).total_cmp(&distance2(&b.rect, x, y)))
    }

    /// (x, y) にあるスクロールの棒の一覧。部品の本来の範囲の中なら None（部品が勝つ）
    pub fn scrollbar_at(&self, x: f32, y: f32) -> Option<&ListInfo> {
        if self.prev.hits.iter().any(|h| h.rect.contains(x, y)) {
            return None;
        }
        self.prev.lists.iter().rev().find(|l| l.bar.is_some_and(|b| b.contains(x, y)))
    }

    /// 一覧のスクロールの位置（論理ピクセル）を、端で止めて設定する
    fn set_scroll(&mut self, l: &ListInfo, v: f32) -> f32 {
        let max = ((l.content_h - l.rect.h) / self.scale).max(0.0);
        let v = v.clamp(0.0, max);
        self.scrolls.insert(l.id, v);
        v
    }

    /// スクロールの棒を押した。つまみならつかみ、空いた所ならそちらへ一覧の高さ 1 つ分動かす
    fn press_scrollbar(&mut self, l: &ListInfo, y: f32) {
        let Some(thumb) = l.thumb else { return };
        let now = self.scrolls.get(&l.id).copied().unwrap_or(0.0);
        if y >= thumb.y && y < thumb.bottom() {
            self.drag = Some(ScrollDrag { list: l.id, grab: y - thumb.y, moves: 0 });
            log::event("scrollbar.grab", &format!("y={y} thumb_y={:.0}", thumb.y), &format!("scroll={now:.0}"));
        } else {
            let page = l.rect.h / self.scale;
            let up = y < thumb.y;
            let v = self.set_scroll(l, if up { now - page } else { now + page });
            log::event("scrollbar.page", &format!("y={y} dir={}", if up { "up" } else { "down" }), &format!("scroll={v:.0}"));
        }
    }

    /// つかんだつまみを y まで動かす（つかんだ所がポインターの下に来るように）
    fn drag_scrollbar(&mut self, d: ScrollDrag, y: f32) {
        let Some(l) = self.prev.lists.iter().find(|l| l.id == d.list).cloned() else { return };
        let Some(thumb) = l.thumb else { return };
        let track = l.rect.h - thumb.h;
        let f = if track > 0.0 { ((y - d.grab - l.rect.y) / track).clamp(0.0, 1.0) } else { 0.0 };
        let max = ((l.content_h - l.rect.h) / self.scale).max(0.0);
        self.set_scroll(&l, f * max);
    }

    fn is_text_input(&self, id: Id) -> bool {
        self.prev.hits.iter().any(|h| h.id == id && h.text_input)
    }

    fn move_focus(&mut self, back: bool) {
        let order = &self.prev.focus_order;
        if order.is_empty() {
            return;
        }
        let at = self.focus.and_then(|f| order.iter().position(|&o| o == f));
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
        };
        self.focus = Some(order[next]);
    }

    /// 入力を部品へ振り分ける。ビューの 1 回目の前に呼ぶ
    pub fn handle(&mut self, events: Vec<Event>) {
        for e in events {
            match e {
                Event::PointerDown { x, y, shift } => {
                    self.commit_preedit("pointer");
                    // 部品の本来の範囲が先。次にスクロールの棒、最後に部品の当たりの余白
                    if let Some(l) = self.scrollbar_at(x, y).cloned() {
                        self.pressed = None;
                        self.press_scrollbar(&l, y);
                        continue;
                    }
                    let hit = self.hit(x, y).cloned();
                    self.pressed = hit.as_ref().map(|h| h.id);
                    self.focus_visible = false;
                    self.focus = hit.as_ref().filter(|h| h.focusable).map(|h| h.id);
                    if let Some(h) = hit.filter(|h| h.text_input) {
                        self.pointer_text = Some((h.id, x, y, shift));
                    }
                }
                Event::PointerUp { x, y } => {
                    if let Some(d) = self.drag.take() {
                        let v = self.scrolls.get(&d.list).copied().unwrap_or(0.0);
                        log::event("scrollbar.release", &format!("y={y} moves={}", d.moves), &format!("scroll={v:.0}"));
                        self.pressed = None;
                        continue;
                    }
                    let hit = self.hit(x, y).map(|h| h.id);
                    if hit.is_some() && hit == self.pressed {
                        self.clicked = hit;
                    }
                    self.pressed = None;
                }
                Event::PointerMove { y, .. } => {
                    if let Some(d) = self.drag.as_mut() {
                        d.moves += 1;
                        let d = *d;
                        self.drag_scrollbar(d, y);
                    }
                }
                Event::Wheel { x, y, dy } => {
                    if let Some(l) = self.prev.lists.iter().rev().find(|l| l.rect.contains(x, y)) {
                        let max = ((l.content_h - l.rect.h) / self.scale).max(0.0);
                        let v = self.scrolls.entry(l.id).or_insert(0.0);
                        *v = (*v + dy).clamp(0.0, max);
                    }
                }
                Event::Key { key: Key::Tab, mods } => {
                    self.commit_preedit("tab");
                    self.focus_visible = true;
                    self.move_focus(mods.shift);
                    self.reveal_focus();
                }
                Event::Key { key: Key::Enter | Key::Space, mods: Mods { ctrl: false, alt: false, .. } }
                    if self.focus.is_some_and(|f| !self.is_text_input(f)) =>
                {
                    self.clicked = self.focus;
                }
                Event::A11y { target, action: A11yAction::Click } => {
                    self.commit_preedit("a11y");
                    self.clicked = Some(target);
                }
                Event::A11y { target, action: A11yAction::Focus } => {
                    self.commit_preedit("a11y");
                    self.focus = Some(target);
                    self.reveal_focus();
                }
                Event::ImePreedit(s) => {
                    // 変換が始まった時に入力欄の選択を消すため、入力欄にも届ける
                    if self.preedit.is_empty() && !s.is_empty() && self.focus.is_some_and(|f| self.is_text_input(f)) {
                        self.keys.push(Event::ImePreedit(s.clone()));
                    }
                    self.preedit = s;
                    // 文節の区切りは、新しい変換中の文字のすぐ後に届く。古い区切りは捨てる
                    self.preedit_clauses = None;
                }
                Event::ImeClauses { bounds, target, cursor } => {
                    if !self.preedit.is_empty() {
                        self.preedit_clauses = Some(Clauses { bounds, target, cursor });
                    }
                }
                e @ (Event::Key { .. } | Event::Char(_) | Event::ImeCommit(_) | Event::ImeSelect { .. }) => {
                    if self.focus.is_some_and(|f| self.is_text_input(f)) {
                        if matches!(e, Event::ImeCommit(_)) {
                            self.preedit.clear();
                        }
                        self.keys.push(e);
                    }
                }
            }
        }
    }

    /// 焦点のある部品が一覧の行の中にあって、行が全部は見えていなければ、見えるように一覧をスクロールする
    fn reveal_focus(&mut self) {
        let Some(f) = self.focus else { return };
        let Some(&(list, row)) = self.prev.rows.get(&f) else { return };
        let Some(l) = self.prev.lists.iter().find(|l| l.id == list).cloned() else { return };
        let now = self.scrolls.get(&list).copied().unwrap_or(0.0);
        let dy = if row.bottom() > l.rect.bottom() {
            row.bottom() - l.rect.bottom()
        } else if row.y < l.rect.y {
            row.y - l.rect.y
        } else {
            return;
        };
        self.set_scroll(&l, now + dy / self.scale);
    }

    /// 変換の途中で焦点が動きそうな時（押す・Tab・読み上げソフトの操作）、変換中の文字を今の入力欄へ確定する。
    /// IME の側の変換は取り消す（IME に確定させると、結果が届く前に焦点が移って別の入力欄に入るため）
    fn commit_preedit(&mut self, why: &str) {
        if self.preedit.is_empty() {
            return;
        }
        let Some(f) = self.focus.filter(|&f| self.is_text_input(f)) else { return };
        let t = std::mem::take(&mut self.preedit);
        self.preedit_clauses = None;
        log::event("ime.commit_on_focus", &format!("why={why} text={t}"), "cancel_ime");
        self.pending_commit = Some((f, t));
        self.ime_cancel = true;
    }

    /// ビューの 1 回目が終わったら、使わなかった入力を捨てる
    pub fn end_input(&mut self) {
        self.pending_commit = None;
        self.clicked = None;
        self.keys.clear();
        self.pointer_text = None;
    }
}
