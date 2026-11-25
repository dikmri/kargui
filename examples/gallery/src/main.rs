// 部品の見本（kargui の例）。左の一覧で部品を選ぶと、右にその部品の見本が出る（1 部品 1 ページ）。どのページも、
// ふつうの状態と押せない状態を並べ、使う関数の名前を見出しの下に出す。一覧は文字を打つと飛べる（List::type_ahead）。
#![windows_subsystem = "windows"]

use std::time::{Duration, Instant};

use kargui::ui::{TableColumn, TreeBuilder};
use kargui::{App, Color, Date, Dock, DockSide, DockTo, FileDialog, Frame, Icon, Image, Notification, NotificationAction, Options, Rect, Size, Time, Ui, rgb};
use taskcore::{args, log};

const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/gallery.log");

/// ページ（名前と、使う関数）
const PAGES: [(&str, &str); 25] = [
    ("ボタン", "button · icon_button · toggle_button · link"),
    ("チェックと切り替え", "checkbox · toggle · dark_mode_switch"),
    ("ひとつを選ぶ", "radio_group · segmented · dropdown"),
    ("数", "slider · number"),
    ("文字の入力", "text_input · password · text_area"),
    ("進み具合", "progress · progress_busy · spinner"),
    ("画像とアイコン", "image · icon"),
    ("文字", "label · heading · rich_text"),
    ("入れ物", "card · group · separator · row · column"),
    ("開閉する節", "expander"),
    ("一覧", "list"),
    ("表", "table"),
    ("木", "tree"),
    ("日付と時刻", "date_picker · time_picker"),
    ("色", "color_picker"),
    ("タブ", "tabs"),
    ("格子と仕切り", "grid · split_left_right"),
    ("メニューとツールバー", "menu_button · toolbar · context_menu"),
    ("浮き出しとダイアログ", "popup · dialog · tooltip"),
    ("ファイルのダイアログ", "file_dialog · chosen_file"),
    ("自由に描く面", "canvas"),
    ("引っ張って落とす", "drag_source · dropped · animate"),
    ("ドッキング", "dock · Dock"),
    ("トレイ", "tray · hide_window · show_window"),
    ("通知", "notify · notification_clicked"),
];

struct Gallery {
    page: Option<usize>,
    split: f32,
    dark: Option<bool>,
    started: Instant,
    // ページの部品の値
    clicks: u32,
    bold: bool,
    check: bool,
    on: bool,
    size: u8,
    view: u8,
    fruit: u8,
    volume: f32,
    copies: i32,
    name: String,
    secret: String,
    notes: String,
    open: bool,
    row: Option<usize>,
    table_sort: Option<(usize, bool)>,
    table_row: Option<usize>,
    node: Option<&'static str>,
    date: Option<Date>,
    time: Option<Time>,
    color: Color,
    tab: u8,
    inner_split: f32,
    popup: bool,
    dialog: bool,
    chosen: Option<String>,
    dots: Vec<(f32, f32)>,
    boxes: [Vec<&'static str>; 2],
    wide: bool,
    picture: Image,
    dock: Dock<&'static str>,
    saved_dock: Option<String>,
    // トレイ（24 ページ目）: アイコンを出すか、閉じるボタンで隠れるか、押した回数、次のビューで行う事
    tray_on: bool,
    close_to_tray: bool,
    tray_clicks: u32,
    hide_now: bool,
    show_now: bool,
    // 通知（25 ページ目）: 出した数と、最後に押された答え
    notified: u32,
    notify_answer: String,
}

impl Gallery {
    /// トレイのアイコン（説明に押した回数を入れる）とメニュー
    fn tray(&mut self, ui: &mut Ui) {
        let (mut show, mut quit, mut page) = (false, false, None);
        let before = ui.dark();
        let mut dark = before;
        let picture = self.picture.clone();
        let clicks = self.tray_clicks;
        ui.tray(|t| {
            t.icon(&picture);
            t.tooltip(format!("部品の見本（押した回数 {clicks}）"));
            if t.clicked() {
                show = true;
            }
            t.menu(|m| {
                show |= m.item("窓を出す").clicked();
                m.check(&mut dark, "暗い色");
                m.separator();
                m.submenu("ページ", |m| {
                    for (i, (name, _)) in PAGES.iter().enumerate().take(4) {
                        if m.item(*name).clicked() {
                            page = Some(i);
                        }
                    }
                });
                m.item("押せない項目").enabled(false);
                m.separator();
                quit = m.item("見本を終える").clicked();
            });
        });
        if show {
            self.tray_clicks += 1;
            log::event("gallery.tray_click", "", &self.tray_clicks.to_string());
            self.show_now = true;
        }
        if dark != before {
            self.dark = Some(dark);
            log::event("gallery.dark", "tray", &dark.to_string());
        }
        if let Some(p) = page {
            log::event("gallery.tray_page", PAGES[p].0, "");
            self.page = Some(p);
            self.show_now = true;
        }
        if quit {
            log::event("gallery.tray_quit", "", "");
            ui.quit();
        }
    }
}

/// ドッキングの最初の並べ方: 左にファイル、右上に編集とメモ、右下に出力
fn default_dock() -> Dock<&'static str> {
    let mut dock = Dock::new(["編集", "メモ"]);
    dock.insert("ファイル", DockTo::Edge { side: DockSide::Left, share: 0.3 });
    dock.insert("出力", DockTo::Split { of: "編集", side: DockSide::Bottom, share: 0.35 });
    dock
}

/// ドッキングのパネルの名前（保存した文字から戻す）
const DOCK_PANELS: [&str; 4] = ["ファイル", "編集", "メモ", "出力"];

/// 見本の画像（64×64 の色の移り変わりと、透ける丸）
fn picture() -> Image {
    let mut px = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64u32 {
        for x in 0..64u32 {
            let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
            let inside = dx * dx + dy * dy <= 30.0 * 30.0;
            let a = if inside { 255 } else { 0 };
            px.extend_from_slice(&[(x * 4) as u8, (y * 4) as u8, 200, a]);
        }
    }
    Image::from_rgba(64, 64, px).expect("64×64")
}

impl Gallery {
    fn new(started: Instant) -> Self {
        Self {
            page: Some(0),
            split: 220.0,
            dark: None,
            started,
            clicks: 0,
            bold: false,
            check: true,
            on: false,
            size: 1,
            view: 0,
            fruit: 0,
            volume: 40.0,
            copies: 1,
            name: String::new(),
            secret: String::new(),
            notes: String::new(),
            open: false,
            row: None,
            table_sort: None,
            table_row: None,
            node: None,
            date: None,
            time: None,
            color: rgb(0x1a73e8),
            tab: 0,
            inner_split: 160.0,
            popup: false,
            dialog: false,
            chosen: None,
            dots: Vec::new(),
            boxes: [vec!["りんご", "みかん", "ぶどう"], Vec::new()],
            wide: false,
            picture: picture(),
            dock: default_dock(),
            saved_dock: None,
            tray_on: false,
            close_to_tray: true,
            tray_clicks: 0,
            hide_now: false,
            show_now: false,
            notified: 0,
            notify_answer: String::new(),
        }
    }

    /// 押せない状態の見本（同じ部品を enabled(false) の中に作る）
    fn disabled(ui: &mut Ui, f: impl FnOnce(&mut Ui)) {
        ui.label("押せない状態").weak().small();
        ui.enabled(false, f);
    }

    /// ドッキング（23 ページ目と、別の窓に出したまとまりの窓で同じ物を使う）
    fn dock_panels(&mut self, ui: &mut Ui) {
        let notes = &mut self.notes;
        let closed = ui.dock(&mut self.dock).grow().closable().show(|p| p.to_string(), |ui, p| match *p {
            "ファイル" => {
                ui.label("main.rs");
                ui.label("dock.rs");
            }
            "編集" => {
                ui.label("fn main() {").monospace();
                ui.label("    kargui::run(..)").monospace();
                ui.label("}").monospace();
            }
            "メモ" => {
                ui.text_area(notes).hint("メモ").grow();
            }
            _ => {
                ui.label("終わりました（0.8 秒）").weak();
            }
        });
        if let Some(p) = closed {
            self.dock.remove(&p);
            log::event("gallery.dock_close", p, "removed");
        }
    }

    fn page(&mut self, ui: &mut Ui, page: usize) {
        match page {
            0 => {
                ui.row(|ui| {
                    if ui.button("ふつう").clicked() {
                        self.clicks += 1;
                    }
                    if ui.button("主な操作").primary().clicked() {
                        self.clicks += 1;
                    }
                    ui.button("追加").icon(Icon::Plus);
                    ui.icon_button(Icon::Search, "検索");
                    ui.toggle_button(&mut self.bold, "太字");
                });
                ui.label(format!("押した回数: {}", self.clicks));
                if ui.link("リンク（押せる文字）").clicked() {
                    self.clicks += 1;
                }
                Self::disabled(ui, |ui| {
                    ui.row(|ui| {
                        ui.button("ふつう");
                        ui.button("主な操作").primary();
                        ui.icon_button(Icon::Search, "検索");
                    });
                });
            }
            1 => {
                ui.checkbox(&mut self.check, "メールで知らせる");
                ui.toggle(&mut self.on, "自動で保存する");
                ui.dark_mode_switch("暗い表示");
                Self::disabled(ui, |ui| {
                    let mut c = true;
                    ui.checkbox(&mut c, "メールで知らせる");
                });
            }
            2 => {
                ui.radio_group(&mut self.size, &[(0, "小"), (1, "中"), (2, "大")]);
                ui.segmented(&mut self.view, &[(0, "一覧"), (1, "格子"), (2, "地図")]);
                ui.dropdown(&mut self.fruit, &[(0, "りんご"), (1, "みかん"), (2, "ぶどう")]).label("果物");
                Self::disabled(ui, |ui| {
                    let mut f = 0u8;
                    ui.dropdown(&mut f, &[(0, "りんご")]).label("果物（押せない）");
                });
            }
            3 => {
                ui.slider(&mut self.volume, 0.0..=100.0).label("音量");
                ui.number(&mut self.copies, 1..=99).label("部数");
                ui.label(format!("音量 {:.0}・部数 {}（小さいボタンは押したままで進む）", self.volume, self.copies)).weak().wrap();
            }
            4 => {
                ui.text_input(&mut self.name).hint("名前").grow();
                ui.password(&mut self.secret).hint("パスワード（日本語入力は切れる）").grow();
                ui.text_area(&mut self.notes).hint("メモ（複数行）").rows(5).grow();
            }
            5 => {
                ui.progress(0.4).label("ダウンロード");
                ui.progress_busy().label("準備中");
                ui.row(|ui| {
                    ui.spinner().label("読み込み中");
                    ui.label("読み込み中");
                });
            }
            6 => {
                ui.row(|ui| {
                    ui.image(&self.picture).label("色の移り変わり");
                    ui.image(&self.picture).size(128.0, 128.0).label("2 倍");
                });
                ui.row(|ui| {
                    for icon in Icon::ALL {
                        ui.icon(icon);
                    }
                });
            }
            7 => {
                ui.heading("見出し");
                ui.label("ふつうの文字");
                ui.label("薄い文字").weak();
                ui.label("小さい文字").small();
                ui.label("等幅の文字 let x = 1;").monospace();
                ui.rich_text(|t| {
                    t.text("書式付きの文字: ");
                    t.text("太字").bold();
                    t.text("・");
                    t.text("斜体").italic();
                    t.text("・");
                    t.text("コード").code();
                    t.text("・");
                    if t.link("リンク").clicked() {
                        self.clicks += 1;
                    }
                });
            }
            8 => {
                ui.card(|ui| {
                    ui.heading("カード");
                    ui.label("中身をまとめる入れ物");
                });
                ui.group("題付きの枠", |ui| {
                    ui.label("中身");
                });
                ui.separator();
                ui.row(|ui| {
                    ui.label("横に");
                    ui.label("並べる");
                    ui.fill();
                    ui.label("右の端");
                });
            }
            9 => {
                ui.expander(&mut self.open, "くわしい設定", |ui| {
                    ui.label("開いた時だけ作る中身");
                });
            }
            10 => {
                ui.list(1000, 28.0).height(Size::Fixed(240.0)).selection(&mut self.row).type_ahead(|i| format!("{i} 行目")).show(|ui, i| {
                    ui.label(format!("{i} 行目"));
                });
                ui.label("1000 行（見えている行だけを作る）。数字を打つと、その数字で始まる行へ飛ぶ").weak().wrap();
            }
            11 => {
                const ROWS: [(&str, &str, &str); 5] = [("東京", "13,960,000", "関東"), ("大阪", "8,780,000", "近畿"), ("名古屋", "2,330,000", "中部"), ("札幌", "1,970,000", "北海道"), ("福岡", "1,640,000", "九州")];
                let columns = [TableColumn::new("都市").fill(), TableColumn::new("人口").width(120.0).right(), TableColumn::new("地方").width(100.0)];
                let mut order: Vec<usize> = (0..ROWS.len()).collect();
                if let Some((col, asc)) = self.table_sort {
                    let people = |i: usize| ROWS[i].1.replace(',', "").parse::<u64>().unwrap_or(0);
                    order.sort_by(|&a, &b| {
                        let o = match col {
                            1 => people(a).cmp(&people(b)),
                            2 => ROWS[a].2.cmp(ROWS[b].2),
                            _ => ROWS[a].0.cmp(ROWS[b].0),
                        };
                        if asc { o } else { o.reverse() }
                    });
                }
                ui.table(&columns, ROWS.len(), 28.0).sort(&mut self.table_sort).selection(&mut self.table_row).type_ahead(|i| ROWS[order[i]].0.to_string()).label("都市").show(|row, i| {
                    let r = ROWS[order[i]];
                    row.text(r.0);
                    row.text(r.1);
                    row.text(r.2);
                });
                ui.label("見出しを押すと並べ替える。見出しに焦点を置いて Shift+←→ で列の幅").weak().wrap();
            }
            12 => {
                ui.tree(&mut self.node, |t: &mut TreeBuilder<&'static str>| {
                    t.branch("docs", "文書", |t| {
                        t.leaf("report", "報告書.pdf");
                        t.leaf("memo", "メモ.txt");
                    });
                    t.branch("pics", "写真", |t| {
                        t.leaf("sea", "海.jpg");
                    });
                })
                .label("ファイル");
            }
            13 => {
                ui.date_picker(&mut self.date).label("日付");
                ui.time_picker(&mut self.time).label("時刻");
            }
            14 => {
                ui.color_picker(&mut self.color).label("強調の色");
            }
            15 => {
                ui.tabs(&mut self.tab, &[(0, "一般"), (1, "表示"), (2, "詳しい設定")], |ui, t| {
                    ui.label(format!("{} 番目のタブの中身", t + 1));
                });
            }
            16 => {
                ui.grid(&[Size::Auto, Size::Fill(1.0)], |ui| {
                    ui.label("名前");
                    ui.text_input(&mut self.name).grow();
                    ui.label("部数");
                    ui.number(&mut self.copies, 1..=99);
                });
                let mut s = self.inner_split;
                ui.split_left_right(&mut s, |ui, pane| {
                    ui.label(if pane == 0 { "左" } else { "右（仕切りを引っ張る）" });
                })
                .height(Size::Fixed(120.0));
                self.inner_split = s;
            }
            17 => {
                ui.toolbar(|tb| {
                    if tb.button(Icon::Plus, "追加").clicked() {
                        self.clicks += 1;
                    }
                    tb.toggle(&mut self.bold, Icon::Check, "固定");
                    tb.separator();
                    tb.more(|m| {
                        m.item("設定");
                    });
                })
                .label("ツールバー");
                ui.menu_button("メニュー", |m| {
                    m.item("開く");
                    m.item("保存");
                });
                ui.label("右クリックでメニュー").context_menu(|m| {
                    if m.item("写す").clicked() {
                        self.clicks += 1;
                    }
                });
            }
            18 => {
                let more = ui.button("浮き出しを開く").tooltip("押すと下に浮き出しを開く");
                let anchor = more.id();
                if more.clicked() {
                    self.popup = !self.popup;
                }
                ui.popup(&mut self.popup, anchor, |ui| {
                    ui.button("名前を変える");
                    ui.button("消す");
                });
                if ui.button("ダイアログを開く").clicked() {
                    self.dialog = true;
                }
                ui.dialog(&mut self.dialog, "確かめ", |ui| {
                    ui.label("ダイアログの中身。Esc で閉じる");
                });
            }
            19 => {
                if ui.button("ファイルを開く…").clicked() {
                    ui.file_dialog("open", FileDialog::open().filter("文字", &["txt", "md"]));
                }
                if let Some(path) = ui.chosen_file("open") {
                    self.chosen = Some(path.display().to_string());
                }
                ui.label(self.chosen.as_deref().unwrap_or("まだ選んでいない")).weak();
            }
            20 => {
                let dots = &self.dots;
                let picture = &self.picture;
                let canvas = ui.canvas(320.0, 200.0, |d| {
                    d.rounded_rect(Rect::new(0.0, 0.0, 320.0, 200.0), 8.0, rgb(0xf2f4f7));
                    d.line((20.0, 180.0), (300.0, 20.0), 2.0, rgb(0x1a73e8));
                    d.image(Rect::new(240.0, 120.0, 64.0, 64.0), picture);
                    for &(x, y) in dots {
                        d.circle((x, y), 5.0, rgb(0xd93025));
                    }
                });
                if let Some(at) = canvas.label("点を打つ面").clicked_at() {
                    self.dots.push(at);
                }
                ui.label("押した所に点を打つ（画像も描ける）").weak().wrap();
            }
            21 => {
                let mut moved = None;
                ui.row(|ui| {
                    for (b, items) in self.boxes.iter().enumerate() {
                        let mut column = ui
                            .card(|ui| {
                                ui.heading(if b == 0 { "左の箱" } else { "右の箱" });
                                for (k, name) in items.iter().enumerate() {
                                    ui.card(|ui| {
                                        ui.label(*name);
                                    })
                                    .drag_source((b, k));
                                }
                            })
                            .width(Size::Fill(1.0))
                            .label(if b == 0 { "左の箱" } else { "右の箱" });
                        if let Some(from) = column.dropped::<(usize, usize)>() {
                            moved = Some((from, b));
                        }
                    }
                });
                if let Some(((from, k), to)) = moved
                    && from != to
                    && k < self.boxes[from].len()
                {
                    let item = self.boxes[from].remove(k);
                    self.boxes[to].push(item);
                }
                ui.toggle(&mut self.wide, "広げる（動き）");
                let w = ui.animate("wide", if self.wide { 300.0 } else { 120.0 }, Duration::from_millis(250));
                ui.card(|ui| {
                    ui.label("動く幅");
                })
                .width(Size::Fixed(w));
            }
            22 => {
                ui.column(|ui| self.dock_panels(ui)).height(Size::Fixed(320.0));
                ui.row(|ui| {
                    if ui.button("最初の並べ方に戻す").clicked() {
                        self.dock = default_dock();
                        log::event("gallery.dock_reset", "", "ok");
                    }
                    if ui.button("並べ方を保存").clicked() {
                        let text = self.dock.save(|p| p.to_string());
                        log::event("gallery.dock_save", "", &text.replace('\n', "|"));
                        self.saved_dock = Some(text);
                    }
                    let saved = self.saved_dock.clone();
                    if ui.button("保存した並べ方に戻す").enabled(saved.is_some()).clicked()
                        && let Some(text) = saved
                    {
                        let back = Dock::load(&text, |name| DOCK_PANELS.into_iter().find(|p| *p == name));
                        log::event("gallery.dock_load", "", &format!("ok={}", back.is_some()));
                        if let Some(d) = back {
                            self.dock = d;
                        }
                    }
                });
                ui.label("タブを引っ張ると、落ちる所が薄く塗られる（まとまりの真ん中はタブ、端は分ける、全体の端の帯は外側）。タブの右クリック（Shift+F10）でも動かせる。F6 でまとまりを回る").weak().wrap();
                ui.label("右クリックの Float で、パネルをほかのまとまりの上に浮かせる。浮いた枠は、タブの左の点を引くと動き、右下の点を引くと大きさが変わる。右クリックの Dock で元の場所へ戻る").weak().wrap();
                ui.label("右クリックの Move to a new window で、パネルを別の窓に出す（Windows と X11）。窓を閉じると元の場所へ戻る").weak().wrap();
            }
            23 => {
                if ui.checkbox(&mut self.tray_on, "通知領域にアイコンを出す").changed() {
                    log::event("gallery.tray", "", &self.tray_on.to_string());
                }
                let on = self.tray_on;
                ui.enabled(on, |ui| {
                    ui.checkbox(&mut self.close_to_tray, "閉じるボタンでトレイに隠れる");
                    if ui.button("窓を隠す").clicked() {
                        self.hide_now = true;
                    }
                });
                ui.label(format!("トレイのアイコンを押した回数: {}", self.tray_clicks));
                ui.label("アイコンを押すと窓が戻る。右クリックのメニューで、窓を出す・明暗・ページを選ぶ・見本を終えるができる").weak().wrap();
                ui.label("Linux は StatusNotifierItem を表示する帯（KDE・Waybar・Quickshell・拡張機能を入れた GNOME）に出る").weak().wrap();
            }
            24 => {
                let mut asked = None;
                ui.row(|ui| {
                    if ui.button("通知を出す").clicked() {
                        asked = Some(Notification::new("部品の見本").body("通知の本文を押すと、見本の窓が前に出る"));
                    }
                    if ui.button("音の無い通知").clicked() {
                        asked = Some(Notification::new("部品の見本").body("音を鳴らさずに出した通知").silent());
                    }
                    if ui.button("ボタン付きの通知").clicked() {
                        asked = Some(Notification::new("部品の見本").body("ボタンを押すと、答えがページに出る").button("later", "あとで").button("open", "開く"));
                    }
                });
                if let Some(n) = asked {
                    self.notified += 1;
                    log::event("gallery.notify", "", &self.notified.to_string());
                    ui.notify("gallery", n);
                }
                ui.label(format!("出した通知: {}", self.notified));
                let answer = if self.notify_answer.is_empty() { "まだ無い" } else { self.notify_answer.as_str() };
                ui.label(format!("最後に押された所: {answer}"));
                ui.label("同じキーで出すので、新しい通知は前の通知を置き換える。Windows は最初の通知の時に、見本の名前をレジストリ（HKEY_CURRENT_USER\\Software\\Classes\\AppUserModelId\\gallery-kargui）に書く").weak().wrap();
            }
            _ => {}
        }
    }
}

impl App for Gallery {
    // ドッキングの別の窓（23 ページ目のパネルを、右クリックの Move to a new window で出した物）
    fn window(&mut self, key: &str, ui: &mut Ui) {
        if let Some(d) = self.dark {
            ui.set_dark(d);
        }
        if self.dock.has_window(key) {
            self.dock_panels(ui);
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        if let Some(d) = self.dark {
            ui.set_dark(d);
        }
        if self.tray_on {
            self.tray(ui);
        }
        if std::mem::take(&mut self.hide_now) {
            log::event("gallery.hide", "", "");
            ui.hide_window();
        }
        // 通知の答え（25 ページ目で出した通知。どのページを開いていても受け取る）
        if let Some(a) = ui.notification_clicked("gallery") {
            log::event("gallery.notify_answer", "", &format!("{a:?}"));
            self.notify_answer = match a {
                NotificationAction::Body => {
                    self.show_now = true;
                    "本文".into()
                }
                NotificationAction::Button(id) if id == "later" => "あとで".into(),
                NotificationAction::Button(id) if id == "open" => "開く".into(),
                NotificationAction::Button(id) => id,
            };
        }
        if std::mem::take(&mut self.show_now) {
            log::event("gallery.show", "", "");
            ui.show_window();
        }
        // kargui が描く題の帯（Options::custom_title_bar）に、見本の名前と明暗の切り替えを置く。帯を描かない OS では
        // 窓のいちばん上の行になる
        ui.title_bar(|ui| {
            ui.label("部品の見本").bold();
            let mut dark = ui.dark();
            if ui.toggle_button(&mut dark, "暗い色").changed() {
                self.dark = Some(dark);
                log::event("gallery.dark", "", &dark.to_string());
            }
        });
        let mut split = self.split;
        ui.split_left_right(&mut split, |ui, pane| {
            if pane == 0 {
                ui.heading("部品");
                ui.list(PAGES.len(), 30.0).grow().selection(&mut self.page).type_ahead(|i| PAGES[i].0.to_string()).show(|ui, i| {
                    ui.label(PAGES[i].0);
                });
            } else {
                let page = self.page.unwrap_or(0).min(PAGES.len() - 1);
                ui.heading(PAGES[page].0);
                ui.label(PAGES[page].1).weak().monospace();
                ui.separator();
                ui.scroll(|ui| self.page(ui, page)).grow();
            }
        })
        .label("部品の一覧の幅")
        .grow();
        self.split = split;
    }

    // トレイを出していて「閉じるボタンでトレイに隠れる」なら、終わらずに隠れる
    fn close_requested(&mut self) -> bool {
        let hide = self.tray_on && self.close_to_tray;
        log::event("gallery.close", "", if hide { "hide to the tray" } else { "close" });
        self.hide_now |= hide;
        !hide
    }

    fn presented(&mut self, frame: &Frame) {
        if frame.number == 0 {
            log::event("ready", "app=gallery", &format!("since_main_ms={}", self.started.elapsed().as_millis()));
        }
    }
}

fn main() {
    let started = Instant::now();
    let a = args::from_env();
    let path = log::init("kargui-gallery", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    log::event("startup", "app=gallery", &format!("log={path:?}"));
    let mut options = Options::new("部品の見本（kargui）");
    options.size = (900.0, 620.0);
    options.min_size = Some((600.0, 400.0));
    options.position = a.pos;
    options.activate = !a.no_focus;
    // 題の帯は kargui が描く（Windows。GNOME はいつも kargui が描く）
    options.custom_title_bar = true;
    // --high-contrast は OS の設定に関わらずハイコントラストで描く（作者の PC の設定を変えずに確かめる）
    if a.high_contrast {
        options.high_contrast = Some(true);
    }
    options.default_icon = !a.no_default_icon;
    if matches!(a.renderer.as_deref(), Some("d3d11" | "gpu")) {
        options.renderer = kargui::Renderer::Gpu;
    }
    let mut app = Gallery::new(started);
    app.dark = a.dark;
    // 写真を撮る時に、始めのページを決める（GALLERY_PAGE=6 なら画像とアイコン）
    if let Some(p) = std::env::var("GALLERY_PAGE").ok().and_then(|v| v.parse::<usize>().ok()) {
        app.page = Some(p.min(PAGES.len() - 1));
    }
    if let Err(e) = kargui::run(options, app) {
        log::event("error", "run", &e.to_string());
    }
    log::event("exit", "", "ok");
}

#[cfg(test)]
mod tests {
    use super::*;
    use kargui::Key;
    use kargui::test::Harness;

    #[test]
    fn every_page_opens_and_shows_its_widgets() {
        let mut h = Harness::with_size(Gallery::new(Instant::now()), 900.0, 620.0, 1.0);
        h.frame();
        for (i, (name, _)) in PAGES.iter().enumerate() {
            h.app_mut().page = Some(i);
            h.frame();
            assert!(h.find(name).is_some(), "{name} の見出し");
        }
        // いくつかのページの部品
        h.app_mut().page = Some(0);
        h.frame();
        h.click("ふつう");
        assert_eq!(h.app().clicks, 1);
        h.app_mut().page = Some(11);
        h.frame();
        assert!(h.find("東京").is_some() && h.find("13,960,000").is_some());
    }

    #[test]
    fn the_tray_page_puts_an_icon_that_hides_and_shows_the_window() {
        let mut h = Harness::with_size(Gallery::new(Instant::now()), 900.0, 620.0, 1.0);
        h.app_mut().page = Some(23);
        h.frame();
        assert!(!h.tray_shown());
        h.click("通知領域にアイコンを出す");
        assert!(h.tray_shown());
        assert_eq!(h.tray_tooltip(), Some("部品の見本（押した回数 0）"));
        assert_eq!(h.tray_menu(), ["窓を出す", "[ ] 暗い色", "-", "ページ >", "  ボタン", "  チェックと切り替え", "  ひとつを選ぶ", "  数", "押せない項目 (disabled)", "-", "見本を終える"]);
        // 閉じるボタンで隠れ、トレイを押すと戻る
        h.request_close();
        assert!(!h.is_closed() && !h.window_shown());
        h.tray_click();
        assert!(h.window_shown());
        assert_eq!(h.tray_tooltip(), Some("部品の見本（押した回数 1）"));
        // メニューで暗い色とページを選ぶ
        h.tray_choose("暗い色");
        assert_eq!(h.app().dark, Some(true));
        h.tray_choose("数");
        assert_eq!(h.app().page, Some(3));
        h.tray_choose("見本を終える");
        assert!(h.is_closed());
    }

    #[test]
    fn the_notification_page_shows_notifications_and_their_answers() {
        let mut h = Harness::with_size(Gallery::new(Instant::now()), 900.0, 620.0, 1.0);
        h.app_mut().page = Some(24);
        h.frame();
        h.click("ボタン付きの通知");
        assert_eq!(h.notifications(), ["gallery: 部品の見本 | ボタンを押すと、答えがページに出る [later: あとで] [open: 開く]"]);
        h.click_notification("gallery", Some("open"));
        assert!(h.find("最後に押された所: 開く").is_some());
        // 同じキーなので置き換える
        h.click("通知を出す");
        h.click("音の無い通知");
        assert_eq!(h.notifications(), ["gallery: 部品の見本 | 音を鳴らさずに出した通知 (silent)"]);
        h.click_notification("gallery", None);
        assert!(h.find("最後に押された所: 本文").is_some());
    }

    #[test]
    fn arrows_in_the_list_move_between_pages() {
        let mut h = Harness::with_size(Gallery::new(Instant::now()), 900.0, 620.0, 1.0);
        h.frame();
        h.click("ボタン");
        h.press(Key::Down).press(Key::Down);
        assert_eq!(h.app().page, Some(2));
        assert!(h.find("ひとつを選ぶ").is_some());
    }
}
