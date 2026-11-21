// 表（kargui の例）。1 万件の品物を、列の見出しを押して並べ替え、名前で絞り込み、行を選んで右に詳しく出す。
// 選んだ品物は id で覚えるので、並べ替えや絞り込みをしても同じ品物が選ばれたまま。
// --items N で件数を変えられる（既定 1 万件）。
#![windows_subsystem = "windows"]

use std::time::Instant;

use kargui::{Align, App, Frame, Options, Size, Ui};
use taskcore::{args, group, log};

const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/table.log");
const CATEGORY_W: f32 = 110.0;
const PRICE_W: f32 = 90.0;
const STOCK_W: f32 = 70.0;

struct Item {
    id: u32,
    name: String,
    category: &'static str,
    price: u32,
    stock: u32,
}

/// 決まった並びの見本の品物（毎回同じ）
fn sample(n: usize) -> Vec<Item> {
    const ADJ: [&str; 8] = ["青い", "小さな", "丈夫な", "軽い", "静かな", "古い", "新しい", "丸い"];
    const NOUN: [(&str, &str); 10] = [
        ("ノート", "文房具"),
        ("ペン", "文房具"),
        ("カップ", "台所"),
        ("皿", "台所"),
        ("ランプ", "照明"),
        ("電球", "照明"),
        ("椅子", "家具"),
        ("棚", "家具"),
        ("時計", "雑貨"),
        ("傘", "雑貨"),
    ];
    let mut seed: u64 = 20261121;
    let mut next = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as u32
    };
    (0..n)
        .map(|i| {
            let (noun, category) = NOUN[next() as usize % NOUN.len()];
            Item {
                id: i as u32 + 1,
                name: format!("{}{} {:04}", ADJ[next() as usize % ADJ.len()], noun, i + 1),
                category,
                price: 100 + next() % 9900,
                stock: next() % 500,
            }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Col {
    Name,
    Category,
    Price,
    Stock,
}

struct Table {
    items: Vec<Item>,
    /// 見えている並び（items の添字）
    order: Vec<usize>,
    sort: Col,
    ascending: bool,
    query: String,
    /// 選んだ品物の id と、見えている並びの中の位置
    selected_id: Option<u32>,
    selected_row: Option<usize>,
    started: Instant,
    ready: bool,
}

impl Table {
    fn new(n: usize) -> Self {
        let mut t = Self {
            items: sample(n),
            order: Vec::new(),
            sort: Col::Name,
            ascending: true,
            query: String::new(),
            selected_id: None,
            selected_row: None,
            started: Instant::now(),
            ready: false,
        };
        t.refresh("startup");
        t
    }

    /// 絞り込みと並べ替えをやり直し、選んだ品物の位置を探し直す
    fn refresh(&mut self, why: &str) {
        let t = Instant::now();
        let q = self.query.trim();
        let mut order: Vec<usize> = (0..self.items.len()).filter(|&i| q.is_empty() || self.items[i].name.contains(q)).collect();
        let items = &self.items;
        order.sort_by(|&a, &b| {
            let (a, b) = (&items[a], &items[b]);
            let o = match self.sort {
                Col::Name => a.name.cmp(&b.name),
                Col::Category => a.category.cmp(b.category).then(a.name.cmp(&b.name)),
                Col::Price => a.price.cmp(&b.price).then(a.id.cmp(&b.id)),
                Col::Stock => a.stock.cmp(&b.stock).then(a.id.cmp(&b.id)),
            };
            if self.ascending { o } else { o.reverse() }
        });
        self.order = order;
        self.selected_row = self.selected_id.and_then(|id| self.order.iter().position(|&i| self.items[i].id == id));
        log::event(
            "refresh",
            &format!("why={why} q={q} sort={:?} asc={}", self.sort, self.ascending),
            &format!("rows={} ms={:.2}", self.order.len(), t.elapsed().as_secs_f64() * 1000.0),
        );
    }

    /// 列の見出し。押すとその列で並べ、もう一度押すと逆にする
    fn header(&mut self, ui: &mut Ui, title: &str, col: Col, width: Option<f32>) {
        let mark = if self.sort != col {
            ""
        } else if self.ascending {
            " ▲"
        } else {
            " ▼"
        };
        let b = ui.button(format!("{title}{mark}")).small().label(format!("{title}で並べ替え"));
        let b = match width {
            Some(w) => b.width(Size::Fixed(w)),
            None => b.grow(),
        };
        if b.clicked() {
            self.ascending = if self.sort == col { !self.ascending } else { true };
            self.sort = col;
            self.refresh("sort");
        }
    }
}

impl App for Table {
    fn view(&mut self, ui: &mut Ui) {
        ui.row(|ui| {
            ui.heading("品物の表");
            ui.fill();
            if ui.text_input(&mut self.query).hint("名前で絞り込み").width(Size::Fixed(220.0)).changed() {
                self.refresh("query");
            }
            ui.label(format!("{} 件中 {} 件", group(self.items.len()), group(self.order.len()))).weak();
        });
        ui.row(|ui| {
            ui.column(|ui| {
                // 見出しの行は、一覧の行と同じ余白（左右 12）と間（8）にして、列をそろえる
                ui.row(|ui| {
                    self.header(ui, "名前", Col::Name, None);
                    self.header(ui, "分類", Col::Category, Some(CATEGORY_W));
                    self.header(ui, "値段", Col::Price, Some(PRICE_W));
                    self.header(ui, "在庫", Col::Stock, Some(STOCK_W));
                })
                .padding(12.0, 0.0);
                let (items, order) = (&self.items, &self.order);
                ui.list(order.len(), 30.0).grow().selection(&mut self.selected_row).show_keyed(
                    |row| items[order[row]].id,
                    |ui, row| {
                        let it = &items[order[row]];
                        ui.label(&it.name).grow();
                        ui.label(it.category).width(Size::Fixed(CATEGORY_W));
                        ui.label(format!("{} 円", group(it.price as usize))).right().width(Size::Fixed(PRICE_W));
                        ui.label(group(it.stock as usize)).right().width(Size::Fixed(STOCK_W));
                    },
                );
            })
            .grow();
            // 選んだ品物の詳しい所
            let id = self.selected_row.map(|r| self.items[self.order[r]].id);
            if id != self.selected_id {
                self.selected_id = id;
                log::event("select", &format!("row={:?}", self.selected_row), &format!("id={id:?}"));
            }
            ui.column(|ui| {
                ui.label("詳しく").bold();
                match self.selected_row.map(|r| self.order[r]) {
                    Some(i) => {
                        let it = &mut self.items[i];
                        ui.label(it.name.as_str());
                        ui.label(format!("分類: {}", it.category)).weak();
                        ui.label(format!("値段: {} 円", group(it.price as usize))).weak();
                        ui.label(format!("在庫: {}", group(it.stock as usize))).weak();
                        ui.row(|ui| {
                            if ui.button("1 つ売る").clicked() && it.stock > 0 {
                                it.stock -= 1;
                                log::event("sell", &format!("id={}", it.id), &format!("stock={}", it.stock));
                            }
                            if ui.button("10 入れる").clicked() {
                                it.stock += 10;
                                log::event("restock", &format!("id={}", it.id), &format!("stock={}", it.stock));
                            }
                        });
                    }
                    None => {
                        ui.label("行を選ぶと、ここに出ます").weak();
                    }
                }
            })
            .width(Size::Fixed(210.0));
        })
        .align(Align::Stretch)
        .grow();
    }

    fn presented(&mut self, _frame: &Frame) {
        if !self.ready {
            self.ready = true;
            log::event("ready", &format!("app=table items={}", self.items.len()), &format!("since_main_ms={}", self.started.elapsed().as_millis()));
        }
    }
}

fn main() {
    let a = args::from_env();
    let path = log::init("kargui-table", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    let n = if a.items > 0 { a.items } else { 10_000 };
    log::event("startup", &format!("app=table items={n}"), &format!("log={path:?}"));
    let mut options = Options::new("品物の表（kargui）");
    options.size = (900.0, 600.0);
    options.position = a.pos;
    options.activate = !a.no_focus;
    if let Err(e) = kargui::run(options, Table::new(n)) {
        log::event("error", "run", &e.to_string());
    }
    log::event("exit", "", "ok");
}

#[cfg(test)]
mod tests {
    use super::*;
    use kargui::Key;
    use kargui::test::Harness;

    fn app(n: usize) -> Harness<Table> {
        let mut h = Harness::with_size(Table::new(n), 900.0, 600.0, 1.0);
        h.frame();
        h
    }

    #[test]
    fn headers_sort_and_reverse() {
        let mut h = app(500);
        h.click("値段で並べ替え");
        let prices = |h: &Harness<Table>| h.app().order.iter().map(|&i| h.app().items[i].price).collect::<Vec<_>>();
        assert!(prices(&h).windows(2).all(|w| w[0] <= w[1]), "安い順");
        h.click("値段で並べ替え");
        assert!(prices(&h).windows(2).all(|w| w[0] >= w[1]), "高い順");
    }

    #[test]
    fn the_selection_follows_the_item_through_sorting_and_filtering() {
        let mut h = app(500);
        let first = h.app().items[h.app().order[0]].name.clone();
        h.click(&first);
        let id = h.app().selected_id.unwrap();
        h.press(Key::Down);
        let second = h.app().selected_id.unwrap();
        assert_ne!(id, second);
        // 在庫で並べ替えても、同じ品物が選ばれたまま
        h.click("在庫で並べ替え");
        assert_eq!(h.app().selected_id, Some(second));
        let row = h.app().selected_row.unwrap();
        assert_eq!(h.app().items[h.app().order[row]].id, second);
        // 詳しい所で売ると在庫が減る
        let i = h.app().order[row];
        let stock = h.app().items[i].stock;
        h.click("10 入れる");
        assert_eq!(h.app().items[i].stock, stock + 10);
        // 絞り込みで見えなくなると、選んだ行は無くなる
        h.click("名前で絞り込み").type_text("存在しない名前");
        assert_eq!(h.app().order.len(), 0);
        assert_eq!(h.app().selected_row, None);
    }

    #[test]
    fn ten_thousand_rows_only_build_the_visible_ones() {
        let h = app(10_000);
        let rows = h.names().iter().filter(|n| n.ends_with(" 円") || n.contains("円 ")).count();
        assert!(rows < 40, "見えている行だけを作る: {rows}");
    }
}
