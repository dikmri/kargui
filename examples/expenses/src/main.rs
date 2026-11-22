// 家計簿（kargui の例。ふつうのアプリで確かめる 2 本目）。格子に並べた入力（日付の選択・ドロップダウン・数の入力・
// 入力欄）、タブ、並べ替えと選択のできる表、行の右クリックのメニュー、ツールバー、自由に描く面の棒グラフを使う。
#![windows_subsystem = "windows"]

use std::time::Instant;

use kargui::ui::TableColumn;
use kargui::{App, Date, DateText, Frame, Icon, Options, Rect, Size, Ui, rgb};
use taskcore::{args, log};

const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/expenses.log");

const CATEGORIES: [&str; 5] = ["食費", "日用品", "交通", "趣味", "そのほか"];

#[derive(Clone, PartialEq, Debug)]
struct Entry {
    id: u32,
    date: Date,
    category: usize,
    amount: i64,
    memo: String,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Page {
    List,
    Chart,
}

struct Expenses {
    entries: Vec<Entry>,
    next_id: u32,
    // 入力の欄
    date: Option<Date>,
    category: usize,
    amount: i64,
    memo: String,
    message: String,
    page: Page,
    sort: Option<(usize, bool)>,
    /// 選んでいる行（並べた後の行の番号）
    selected: Option<usize>,
    started: Instant,
    ready: bool,
    /// --light・--dark で決めた明暗（None なら OS の設定に従う。写真を撮る時に使う）
    dark: Option<bool>,
}

/// 金額を「12,345 円」にする
fn yen(v: i64) -> String {
    let digits = v.abs().to_string();
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{}{out} 円", if v < 0 { "-" } else { "" })
}

impl Expenses {
    fn new(started: Instant) -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            date: None,
            category: 0,
            amount: 0,
            memo: String::new(),
            message: String::new(),
            page: Page::List,
            sort: Some((0, false)),
            selected: None,
            started,
            ready: false,
            dark: None,
        }
    }

    fn add(&mut self, date: Date, category: usize, amount: i64, memo: &str) {
        self.entries.push(Entry { id: self.next_id, date, category, amount, memo: memo.to_string() });
        self.next_id += 1;
    }

    /// 並べ替えた順の、entries の番号
    fn order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        if let Some((col, asc)) = self.sort {
            order.sort_by(|&a, &b| {
                let (x, y) = (&self.entries[a], &self.entries[b]);
                let o = match col {
                    0 => x.date.cmp(&y.date),
                    1 => x.category.cmp(&y.category),
                    2 => x.amount.cmp(&y.amount),
                    _ => x.memo.cmp(&y.memo),
                };
                if asc { o } else { o.reverse() }
            });
        }
        order
    }

    fn remove(&mut self, id: u32) {
        self.entries.retain(|e| e.id != id);
        self.selected = None;
        log::event("remove", &format!("id={id}"), "ok");
    }

    fn form(&mut self, ui: &mut Ui) {
        ui.grid(&[Size::Auto, Size::Fixed(200.0), Size::Auto, Size::Fill(1.0)], |ui| {
            ui.label("日付");
            ui.date_picker(&mut self.date).label("日付");
            ui.label("分類");
            ui.dropdown(&mut self.category, &CATEGORIES.iter().copied().enumerate().collect::<Vec<_>>()).label("分類");
            ui.label("金額");
            ui.number(&mut self.amount, 0..=10_000_000).step(100.0).label("金額");
            ui.label("メモ");
            ui.text_input(&mut self.memo).hint("買った物など").label("メモ");
        });
        ui.row(|ui| {
            if ui.button("追加").primary().clicked() {
                match (self.date, self.amount) {
                    (None, _) => self.message = "日付を選んでください".into(),
                    (_, 0) => self.message = "金額を入れてください".into(),
                    (Some(d), a) => {
                        let memo = std::mem::take(&mut self.memo);
                        self.add(d, self.category, a, &memo);
                        self.amount = 0;
                        self.message = "追加しました".into();
                        log::event("add", &format!("date={d} amount={a}"), "ok");
                    }
                }
            }
            ui.label(&self.message).weak();
        });
    }

    fn list(&mut self, ui: &mut Ui) {
        let order = self.order();
        let mut remove = None;
        let picked = self.selected.and_then(|r| order.get(r)).map(|&k| self.entries[k].id);
        ui.toolbar(|tb| {
            if tb.button(Icon::Close, "選んだ行を削除").enabled(picked.is_some()).clicked() {
                remove = picked;
            }
        })
        .label("一覧の操作");
        let columns = [TableColumn::new("日付").width(120.0), TableColumn::new("分類").width(90.0), TableColumn::new("金額").width(110.0).right(), TableColumn::new("メモ").fill()];
        let entries = &self.entries;
        ui.table(&columns, order.len(), 30.0).sort(&mut self.sort).selection(&mut self.selected).label("支出の一覧").grow().show_keyed(
            |r| entries[order[r]].id,
            |row, r| {
                let e = &entries[order[r]];
                row.text(e.date.to_string());
                row.text(CATEGORIES[e.category]);
                row.text(yen(e.amount));
                row.text(&e.memo);
                row.context_menu(|m| {
                    if m.item("削除").clicked() {
                        remove = Some(e.id);
                    }
                });
            },
        );
        if let Some(id) = remove {
            self.remove(id);
        }
    }

    /// 分類ごとの合計の棒グラフ
    fn chart(&self, ui: &mut Ui) {
        let mut sums = [0i64; CATEGORIES.len()];
        for e in &self.entries {
            sums[e.category] += e.amount;
        }
        let max = sums.iter().copied().max().unwrap_or(0).max(1) as f32;
        let (w, h) = (520.0, 240.0);
        let dark = ui.dark();
        let (text, grid) = if dark { (rgb(0xd0d4dc), rgb(0x3a3f48)) } else { (rgb(0x404650), rgb(0xdde2ea)) };
        ui.canvas(w, h, |d| {
            let (left, base, top) = (16.0, h - 36.0, 20.0);
            d.rect(Rect::new(left, base, w - 2.0 * left, 1.0), grid);
            let slot = (w - 2.0 * left) / CATEGORIES.len() as f32;
            for (k, name) in CATEGORIES.iter().enumerate() {
                let bh = (sums[k] as f32 / max * (base - top - 20.0)).round();
                let x = left + slot * k as f32 + slot * 0.2;
                d.rounded_rect(Rect::new(x, base - bh, slot * 0.6, bh), 4.0, rgb(0x2f6fd6));
                d.text((x, base + 8.0), *name, text);
                if sums[k] > 0 {
                    d.text((x, base - bh - 20.0), yen(sums[k]), text);
                }
            }
        })
        .label("分類ごとの合計");
    }
}

impl App for Expenses {
    fn view(&mut self, ui: &mut Ui) {
        if let Some(d) = self.dark {
            ui.set_dark(d);
        }
        ui.set_date_text(DateText::japanese());
        self.form(ui);
        let total: i64 = self.entries.iter().map(|e| e.amount).sum();
        ui.label(format!("合計 {}（{} 件）", yen(total), self.entries.len())).bold();
        let pages = [(Page::List, "一覧"), (Page::Chart, "グラフ")];
        let mut page = self.page;
        ui.tabs(&mut page, &pages, |ui, p| match p {
            Page::List => self.list(ui),
            Page::Chart => self.chart(ui),
        })
        .grow();
        self.page = page;
    }

    fn presented(&mut self, _frame: &Frame) {
        if !self.ready {
            self.ready = true;
            log::event("ready", "app=expenses", &format!("since_main_ms={}", self.started.elapsed().as_millis()));
        }
    }
}

/// 見本のデータ
fn samples(app: &mut Expenses) {
    let d = |m, day| Date::new(2026, m, day).expect("日付");
    app.add(d(11, 2), 0, 1280, "スーパー");
    app.add(d(11, 3), 2, 420, "電車");
    app.add(d(11, 8), 3, 3300, "本");
    app.add(d(11, 12), 1, 860, "洗剤");
    app.add(d(11, 15), 0, 2150, "スーパー");
    app.add(d(11, 20), 4, 1500, "贈り物");
}

fn main() {
    let started = Instant::now();
    let a = args::from_env();
    let path = log::init("kargui-expenses", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    log::event("startup", "app=expenses", &format!("log={path:?}"));
    let mut app = Expenses::new(started);
    samples(&mut app);
    app.dark = a.dark;
    let mut options = Options::new("家計簿（kargui）");
    options.size = (720.0, 600.0);
    options.min_size = Some((560.0, 420.0));
    options.position = a.pos;
    options.activate = !a.no_focus;
    if let Err(e) = kargui::run(options, app) {
        log::event("error", "run", &e.to_string());
    }
    log::event("exit", "", "ok");
}

#[cfg(test)]
mod tests {
    use super::*;
    use kargui::test::Harness;
    use kargui::{Key, Mods};

    fn app(with_samples: bool) -> Harness<Expenses> {
        let mut a = Expenses::new(Instant::now());
        if with_samples {
            samples(&mut a);
        }
        Harness::with_size(a, 720.0, 600.0, 1.0)
    }

    #[test]
    fn adding_an_entry_with_the_form() {
        let mut h = app(false);
        h.click("追加");
        assert_eq!(h.app().message, "日付を選んでください");
        // 見える名前と欄が同じ名前の時、試験台は押せる欄の方を押す
        h.click("日付").click("2026年11月20日");
        assert_eq!(h.app().date, Date::new(2026, 11, 20));
        h.click("分類").click("交通");
        h.click("金額").press_with(Key::A, Mods::CTRL).type_text("1200").press(Key::Enter);
        h.click("メモ").type_text("バス");
        h.click("追加");
        let e = &h.app().entries[0];
        assert_eq!((e.date, e.category, e.amount, e.memo.as_str()), (Date::new(2026, 11, 20).unwrap(), 2, 1200, "バス"));
        assert!(h.find("合計 1,200 円（1 件）").is_some());
        assert!(h.find("バス").is_some(), "一覧に出る");
    }

    #[test]
    fn sorting_selecting_and_removing_rows() {
        let mut h = app(true);
        // 金額の見出しは、入力の「金額」2 つの後
        let r = h.find_all("金額")[2];
        h.click_at(r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert_eq!(h.app().sort, Some((2, true)));
        let first = h.find_all("1,280 円")[0].y;
        assert!(h.find("420 円").unwrap().y < first, "小さい順");
        // 右クリックのメニューで消す
        h.right_click("贈り物").click("削除");
        assert_eq!(h.app().entries.len(), 5);
        assert!(h.find("贈り物").is_none());
        // 行を選ぶと、ツールバーの削除が使える
        assert_eq!(h.enabled("選んだ行を削除"), Some(false));
        h.click("洗剤");
        assert_eq!(h.enabled("選んだ行を削除"), Some(true));
        h.click("選んだ行を削除");
        assert!(h.find("洗剤").is_none());
    }

    #[test]
    fn the_chart_tab_draws_totals() {
        let mut h = app(true);
        h.click("グラフ");
        assert!(h.find("分類ごとの合計").is_some());
        assert!(h.find("支出の一覧").is_none());
    }
}
