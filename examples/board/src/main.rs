// カードの板（kargui の例。ドラッグ＆ドロップを確かめる）。3 つの列の間でカードを引っ張って移し、カードの上に落とすと
// そのカードの前に入る（並べ替え）。引っ張れない人とキーボードのために、どのカードにも「移す」メニューを置く。
#![windows_subsystem = "windows"]

use std::time::Instant;

use kargui::{App, Frame, Options, Size, Ui};
use taskcore::{args, log};

const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/board.log");

const COLUMNS: [&str; 3] = ["やること", "作業中", "済み"];

/// 引っ張る荷物: どの列の何番目のカードか
#[derive(Clone, Copy, Debug, PartialEq)]
struct CardRef {
    col: usize,
    idx: usize,
}

struct Board {
    cols: [Vec<String>; 3],
    new_card: String,
    started: Instant,
    ready: bool,
    /// --light・--dark で決めた明暗（None なら OS の設定に従う。写真を撮る時に使う）
    dark: Option<bool>,
}

impl Board {
    fn new(started: Instant) -> Self {
        let cards = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        Self {
            cols: [cards(&["ガイドを書く", "写真を撮る", "試験を足す"]), cards(&["ドラッグ＆ドロップ"]), cards(&["0.2.0 を出す"])],
            new_card: String::new(),
            started,
            ready: false,
            dark: None,
        }
    }

    /// from のカードを、列 to の before 番目のカードの前（None なら最後）へ移す
    fn move_card(&mut self, from: CardRef, to: usize, before: Option<usize>) {
        if from.col >= COLUMNS.len() || from.idx >= self.cols[from.col].len() || to >= COLUMNS.len() {
            log::event("move", &format!("{from:?}"), "error=no such card");
            return;
        }
        let card = self.cols[from.col].remove(from.idx);
        let mut at = before.unwrap_or(self.cols[to].len());
        // 同じ列の前の方から動かした時は、抜いた分だけ前へずれる
        if from.col == to && before.is_some_and(|b| from.idx < b) {
            at -= 1;
        }
        let at = at.min(self.cols[to].len());
        log::event("move", &card, &format!("from={}:{} to={to}:{at}", from.col, from.idx));
        self.cols[to].insert(at, card);
    }
}

impl App for Board {
    fn view(&mut self, ui: &mut Ui) {
        if let Some(d) = self.dark {
            ui.set_dark(d);
        }
        // (動かすカード, 先の列, 前に入れるカード)
        let mut action: Option<(CardRef, usize, Option<usize>)> = None;
        let mut add = false;
        ui.row(|ui| {
            for c in 0..COLUMNS.len() {
                let mut column = ui
                    .card(|ui| {
                        ui.heading(format!("{}（{}）", COLUMNS[c], self.cols[c].len()));
                        for (k, text) in self.cols[c].iter().enumerate() {
                            let me = CardRef { col: c, idx: k };
                            let mut card = ui
                                .card(|ui| {
                                    ui.row(|ui| {
                                        // 折り返すラベルは幅 0 と測られるので、残りの幅を取らせる
                                        ui.label(text).wrap().grow();
                                        // 引っ張れない時（キーボード・読み上げ）の代わり
                                        ui.menu_button("移す", |m| {
                                            for (t, name) in COLUMNS.iter().enumerate().filter(|&(t, _)| t != c) {
                                                if m.item(format!("{name}へ")).clicked() {
                                                    action = Some((me, t, None));
                                                }
                                            }
                                            if k > 0 && m.item("上へ").clicked() {
                                                action = Some((me, c, Some(k - 1)));
                                            }
                                            if k + 1 < self.cols[c].len() && m.item("下へ").clicked() {
                                                action = Some((me, c, Some(k + 2)));
                                            }
                                        })
                                        .label(format!("「{text}」を移す"));
                                    });
                                })
                                .drag_source(me);
                            // カードの上に落としたら、そのカードの前へ
                            if let Some(from) = card.dropped::<CardRef>() {
                                action = Some((from, c, Some(k)));
                            }
                        }
                        if c == 0 {
                            ui.row(|ui| {
                                let input = ui.text_input(&mut self.new_card).hint("新しいカード").grow();
                                add |= input.submitted();
                                add |= ui.button("追加").clicked();
                            });
                        }
                    })
                    .width(Size::Fill(1.0))
                    .height(Size::Fill(1.0))
                    .label(COLUMNS[c]);
                // 列の空いた所に落としたら、列の最後へ
                if let Some(from) = column.dropped::<CardRef>() {
                    action = Some((from, c, None));
                }
            }
        })
        .gap(12.0)
        .padding(12.0, 12.0)
        .grow();
        if let Some((from, to, before)) = action {
            self.move_card(from, to, before);
        }
        let text = self.new_card.trim().to_string();
        if add && !text.is_empty() {
            log::event("add", &text, "ok");
            self.cols[0].push(text);
            self.new_card.clear();
        }
    }

    fn presented(&mut self, _frame: &Frame) {
        if !self.ready {
            self.ready = true;
            log::event("ready", "app=board", &format!("since_main_ms={}", self.started.elapsed().as_millis()));
        }
    }
}

fn main() {
    let started = Instant::now();
    let a = args::from_env();
    let path = log::init("kargui-board", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    log::event("startup", "app=board", &format!("log={path:?}"));
    let mut options = Options::new("カードの板（kargui）");
    options.size = (760.0, 460.0);
    options.min_size = Some((480.0, 300.0));
    options.position = a.pos;
    options.activate = !a.no_focus;
    let mut app = Board::new(started);
    app.dark = a.dark;
    if let Err(e) = kargui::run(options, app) {
        log::event("error", "run", &e.to_string());
    }
    log::event("exit", "", "ok");
}

#[cfg(test)]
mod tests {
    use super::*;
    use kargui::test::Harness;

    fn app() -> Harness<Board> {
        Harness::with_size(Board::new(Instant::now()), 760.0, 460.0, 1.0)
    }

    #[test]
    fn dragging_a_card_to_another_column_puts_it_last() {
        let mut h = app();
        h.drag_to("写真を撮る", "済み（1）");
        assert_eq!(h.app().cols[0], ["ガイドを書く", "試験を足す"]);
        assert_eq!(h.app().cols[2], ["0.2.0 を出す", "写真を撮る"]);
    }

    #[test]
    fn dropping_on_a_card_puts_it_before_that_card() {
        let mut h = app();
        h.drag_to("試験を足す", "ガイドを書く");
        assert_eq!(h.app().cols[0], ["試験を足す", "ガイドを書く", "写真を撮る"], "同じ列の中の並べ替え");
        h.drag_to("ドラッグ＆ドロップ", "写真を撮る");
        assert_eq!(h.app().cols[0], ["試験を足す", "ガイドを書く", "ドラッグ＆ドロップ", "写真を撮る"]);
        assert!(h.app().cols[1].is_empty());
    }

    #[test]
    fn the_move_menu_does_the_same_without_dragging() {
        let mut h = app();
        h.click("「ガイドを書く」を移す").click("済みへ");
        assert_eq!(h.app().cols[2], ["0.2.0 を出す", "ガイドを書く"]);
        h.click("「試験を足す」を移す").click("上へ");
        assert_eq!(h.app().cols[0], ["試験を足す", "写真を撮る"]);
        h.click("「試験を足す」を移す").click("下へ");
        assert_eq!(h.app().cols[0], ["写真を撮る", "試験を足す"]);
    }

    #[test]
    fn adding_a_card() {
        let mut h = app();
        h.click("新しいカード").type_text("買い物").click("追加");
        assert_eq!(h.app().cols[0].last().map(String::as_str), Some("買い物"));
        assert!(h.app().new_card.is_empty());
    }
}
