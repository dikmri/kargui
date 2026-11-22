//! 説明を古くしないための道具。
//!
//! 使い方（このリポジトリの最上位で）:
//!   cargo run -p xtask -- doc-check     部品を作る関数に、説明と動く例があるか
//!   cargo run -p xtask -- shots         部品の写真（明・暗）を docs/shots/ に作り直す
//!   cargo run -p xtask -- shots --check 写真が今の見た目と同じか（違えば落とす）
//!   cargo run -p xtask -- llms          AI 向けの索引 docs/llms.txt を作り直す
//!   cargo run -p xtask -- llms --check  索引が今のソースと同じか（違えば落とす）
//!   cargo run -p xtask -- coverage      公開項目の全部に説明と動く例があるか（rustdoc の数え方）
//!   cargo run -p xtask -- guide-check   ガイドの扱う API と訳が古くないか（guide.rs）
//!   cargo run -p xtask -- guide-check --update  見直した後に、ガイドの値を入れ直す
//!   cargo run -p xtask -- check         上の検査を全部（取り込みの前に走らせる）
//!
//! 説明そのものが無い公開項目は、kargui の #![deny(missing_docs)] で組み立ての時点で落ちる。
//! 説明の中の例は cargo test --doc で動かす（試験台 kargui::test::Harness の上で動く）。

mod guide;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kargui::test::Harness;
use kargui::{App, Size, Ui};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// ソースの中の、ある impl の中の公開の関数と、その前の説明のコメント
struct Item {
    name: String,
    docs: Vec<String>,
    signature: String,
}

/// impl_header で始まる impl の中の `pub fn` を、説明のコメントと一緒に集める
fn items(source: &str, impl_header: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let Some(start) = source.find(impl_header) else { return out };
    let mut depth = 0i32;
    let mut docs: Vec<String> = Vec::new();
    let mut started = false;
    for line in source[start..].lines() {
        let t = line.trim();
        if started && depth == 1 {
            if let Some(d) = t.strip_prefix("///") {
                docs.push(d.strip_prefix(' ').unwrap_or(d).to_string());
            } else if t.starts_with("pub fn ") {
                let name = t["pub fn ".len()..].split(|c: char| c == '(' || c == '<').next().unwrap_or("").to_string();
                out.push(Item { name, docs: std::mem::take(&mut docs), signature: t.trim_end_matches('{').trim().to_string() });
            } else if !t.starts_with("#[") {
                docs.clear();
            }
        }
        // コメントの行と文字列の中の { } は数えない（説明の中の例にも { がある）
        if !t.starts_with("//") {
            let code: String = t.split('"').step_by(2).collect();
            depth += code.matches('{').count() as i32 - code.matches('}').count() as i32;
        }
        if depth > 0 {
            started = true;
        }
        if started && depth == 0 {
            break;
        }
    }
    out
}

fn ui_source() -> String {
    fs::read_to_string(root().join("crates/kargui/src/ui/mod.rs")).expect("ui/mod.rs")
}

/// 部品を作る関数（Ui の公開の関数）に、説明と動く例（```）があるか
fn doc_check() -> bool {
    let list = items(&ui_source(), "impl<'a> Ui<'a> {");
    let mut ok = true;
    for it in &list {
        let has_text = it.docs.iter().any(|d| !d.trim().is_empty() && !d.starts_with("```"));
        let has_example = it.docs.iter().any(|d| d.starts_with("```"));
        if !has_text || !has_example {
            ok = false;
            println!("doc-check: Ui::{} に {}", it.name, if !has_text { "説明が無い" } else { "動く例（```）が無い" });
        }
    }
    println!("doc-check: Ui の公開の関数 {} 個を確かめた: {}", list.len(), if ok { "ok" } else { "不足あり" });
    ok
}

/// 部品の写真 1 枚分: 名前、大きさ（論理ピクセル）、画面
struct Shot {
    name: &'static str,
    size: (f32, f32),
    view: fn(&mut Ui),
    /// 写す前に試験台で行う操作（入力欄に焦点を当てて IME の変換中にする、など）
    setup: fn(&mut Harness<ShotApp>),
}

struct ShotApp {
    view: fn(&mut Ui),
    dark: bool,
}

impl App for ShotApp {
    fn view(&mut self, ui: &mut Ui) {
        ui.set_dark(self.dark);
        (self.view)(ui);
    }
}

fn no_setup(_: &mut Harness<ShotApp>) {}

fn shots() -> Vec<Shot> {
    vec![
        Shot {
            name: "label",
            size: (320.0, 90.0),
            view: |ui| {
                ui.heading("Tasks");
                ui.label("10 tasks, 3 done").weak();
            },
            setup: no_setup,
        },
        Shot {
            name: "button",
            size: (360.0, 60.0),
            view: |ui| {
                ui.row(|ui| {
                    ui.button("Cancel");
                    ui.button("Save").primary();
                    ui.button("Delete").small();
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "checkbox",
            size: (240.0, 80.0),
            view: |ui| {
                ui.checkbox(&mut true, "Done");
                ui.checkbox(&mut false, "Not yet");
            },
            setup: no_setup,
        },
        Shot {
            name: "toggle",
            size: (240.0, 56.0),
            view: |ui| {
                ui.toggle(&mut true, "Dark mode");
            },
            setup: no_setup,
        },
        Shot {
            name: "segmented",
            size: (320.0, 60.0),
            view: |ui| {
                let mut v = 1;
                ui.segmented(&mut v, &[(0, "All"), (1, "Open"), (2, "Done")]);
            },
            setup: no_setup,
        },
        Shot {
            name: "text_input",
            size: (360.0, 110.0),
            view: |ui| {
                ui.text_input(&mut String::from("Hello, kargui")).grow();
                ui.text_input(&mut String::new()).hint("Your name").grow();
            },
            setup: no_setup,
        },
        Shot {
            name: "text_input_ime",
            size: (360.0, 60.0),
            view: |ui| {
                ui.text_input(&mut String::from("買う物: ")).hint("New task").grow();
            },
            setup: |h| {
                h.click("New task").press(kargui::Key::End).ime_compose("ぎゅうにゅう");
            },
        },
        Shot {
            name: "list",
            size: (360.0, 180.0),
            view: |ui| {
                ui.list(1000, 32.0).grow().show(|ui, i| {
                    let done = i % 3 == 2;
                    ui.checkbox(&mut done.clone(), format!("Item {i}")).strike(done);
                    ui.fill();
                    ui.button("Delete").small().width(Size::Auto);
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "list_selection",
            size: (360.0, 150.0),
            view: |ui| {
                let mut selected = Some(1);
                ui.list(1000, 30.0).grow().selection(&mut selected).show(|ui, i| {
                    ui.label(format!("report-{i:03}.pdf")).grow();
                    ui.label(format!("{} KB", 120 + i * 7)).weak().right().width(Size::Fixed(70.0));
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "slider",
            size: (360.0, 90.0),
            view: |ui| {
                ui.row(|ui| {
                    ui.label("Font size");
                    ui.slider(&mut 14, 10..=24).label("Font size");
                    ui.label("14 pt");
                });
                ui.row(|ui| {
                    ui.label("Volume");
                    ui.slider(&mut 0.3_f32, 0.0..=1.0).label("Volume");
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "scroll",
            size: (360.0, 160.0),
            view: |ui| {
                ui.scroll(|ui| {
                    ui.label("Settings").bold();
                    for (i, name) in ["Spell check", "Auto save", "Notifications", "Sounds", "Statistics"].iter().enumerate() {
                        ui.checkbox(&mut (i % 2 == 0), *name);
                    }
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "disabled",
            size: (520.0, 130.0),
            view: |ui| {
                ui.checkbox(&mut false, "Notifications");
                ui.enabled(false, |ui| {
                    ui.checkbox(&mut true, "Play a sound");
                    ui.row(|ui| {
                        ui.slider(&mut 6, 0..=10).label("Volume");
                        ui.button("Test").primary();
                        ui.text_input(&mut String::new()).hint("Sound file");
                    });
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "popup",
            size: (320.0, 200.0),
            view: |ui| {
                ui.row(|ui| {
                    let more = ui.button("More");
                    let anchor = more.id();
                    ui.popup(&mut true, anchor, |ui| {
                        ui.button("Rename");
                        ui.button("Duplicate");
                        ui.button("Delete");
                    });
                    ui.label("report-2026.pdf");
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "dialog",
            size: (440.0, 240.0),
            view: |ui| {
                ui.heading("Files");
                ui.label("report-2026.pdf");
                ui.button("Rename");
                ui.dialog(&mut true, "Rename", |ui| {
                    ui.text_input(&mut String::from("report-2026.pdf")).width(Size::Fixed(260.0));
                    ui.row(|ui| {
                        ui.fill();
                        ui.button("OK").primary();
                        ui.button("Cancel");
                    });
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "text_area",
            size: (380.0, 150.0),
            view: |ui| {
                let mut text = String::from("Buy milk and bread\nCall the office about the meeting room for Thursday afternoon\nPack");
                ui.text_area(&mut text).hint("Notes").rows(4).grow();
            },
            // 最後の行から 2 行分を選ぶ（行をまたぐ選択）
            setup: |h| {
                h.click("Notes")
                    .press_with(kargui::Key::End, kargui::Mods::CTRL)
                    .press_with(kargui::Key::Up, kargui::Mods::SHIFT)
                    .press_with(kargui::Key::Up, kargui::Mods::SHIFT);
            },
        },
        Shot {
            name: "text_area_ime",
            size: (380.0, 120.0),
            view: |ui| {
                let mut text = String::from("今日の予定:\n");
                ui.text_area(&mut text).hint("Notes").rows(3).grow();
            },
            setup: |h| {
                h.click("Notes").press_with(kargui::Key::End, kargui::Mods::CTRL).ime_compose("かいぎしつをよやくする");
            },
        },
        Shot {
            name: "dropdown",
            size: (360.0, 230.0),
            view: |ui| {
                let (mut theme, mut rows) = (0, 50);
                ui.heading("Appearance");
                ui.dropdown(&mut theme, &[(0, "Use the system setting"), (1, "Light"), (2, "Dark")]).label("Theme");
                ui.dropdown(&mut rows, &[(10, "10 rows"), (50, "50 rows"), (100, "100 rows")]).label("Rows per page");
            },
            setup: |h| {
                h.click("Theme");
            },
        },
        Shot {
            name: "menus",
            size: (520.0, 260.0),
            view: |ui| {
                use kargui::{Key, Mods};
                let mut wrap = true;
                ui.menu_bar(|bar| {
                    bar.menu("File", |m| {
                        m.item("New").shortcut(Key::N, Mods::CTRL);
                        m.item("Save").shortcut(Key::S, Mods::CTRL);
                        m.submenu("Open recent", |m| {
                            m.item("notes.txt");
                            m.item("plan.md");
                            m.item("report-2026.pdf");
                        });
                        m.separator();
                        m.item("Print").shortcut(Key::P, Mods::CTRL).enabled(false);
                        m.item("Exit");
                    });
                    bar.menu("Edit", |m| {
                        m.item("Undo");
                    });
                    bar.menu("View", |m| {
                        m.check(&mut wrap, "Word wrap");
                    });
                });
                ui.label("notes.txt").weak();
            },
            setup: |h| {
                h.click("File").click("Open recent");
            },
        },
        Shot {
            name: "context_menu",
            size: (420.0, 230.0),
            view: |ui| {
                use kargui::{Icon, Key, Mods};
                let mut pinned = true;
                for name in ["notes.txt", "plan.md", "report-2026.pdf"] {
                    ui.label(name).context_menu(|m| {
                        m.item("Open").icon(Icon::ArrowRight);
                        m.item("Rename").shortcut(Key::F2, Mods::NONE);
                        m.check(&mut pinned, "Pinned");
                        m.separator();
                        m.item("Delete").icon(Icon::Close).shortcut(Key::Delete, Mods::NONE);
                    });
                }
            },
            // 開いた後に下の矢印: キーボードの焦点の見た目も写す
            setup: |h| {
                h.right_click("plan.md").press(kargui::Key::Down);
            },
        },
        Shot {
            name: "tabs",
            size: (420.0, 170.0),
            view: |ui| {
                let (mut page, mut sound, mut vibrate) = (1, true, false);
                ui.tabs(&mut page, &[(0, "General"), (1, "Notifications"), (2, "About")], |ui, _| {
                    ui.checkbox(&mut sound, "Play a sound");
                    ui.checkbox(&mut vibrate, "Vibrate");
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "toolbar",
            size: (480.0, 140.0),
            view: |ui| {
                use kargui::Icon;
                let (mut pinned, mut copies, mut zoom) = (true, 3, 1.25);
                ui.toolbar(|tb| {
                    tb.button(Icon::Plus, "Add");
                    tb.toggle(&mut pinned, Icon::Check, "Pin");
                    tb.button(Icon::Search, "Find");
                    tb.separator();
                    tb.button(Icon::Close, "Remove").enabled(false);
                    tb.more(|m| {
                        m.item("Settings");
                    });
                })
                .label("Tasks");
                ui.row(|ui| {
                    ui.label("Copies");
                    ui.number(&mut copies, 1..=99).label("Number of copies");
                    ui.label("Zoom");
                    ui.number(&mut zoom, 0.5..=4.0).step(0.25).label("Zoom level");
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "grid_split",
            size: (560.0, 220.0),
            view: |ui| {
                use kargui::Size;
                let mut left = 150.0;
                let (mut name, mut email, mut team) = (String::from("Aiko Tanaka"), String::new(), 1);
                ui.split_left_right(&mut left, |ui, pane| {
                    if pane == 0 {
                        ui.heading("People");
                        ui.label("Aiko Tanaka");
                        ui.label("Ben Clark").weak();
                        ui.label("Chen Wei").weak();
                    } else {
                        ui.grid(&[Size::Auto, Size::Fill(1.0)], |ui| {
                            ui.label("Name");
                            ui.text_input(&mut name).label("Name");
                            ui.label("E-mail address");
                            ui.text_input(&mut email).hint("name@example.com").label("E-mail address");
                            ui.label("Team");
                            ui.dropdown(&mut team, &[(0, "Design"), (1, "Engineering"), (2, "Sales")]).label("Team");
                        });
                    }
                })
                .label("Resize the list")
                .grow();
            },
            setup: no_setup,
        },
        Shot {
            name: "canvas",
            size: (360.0, 220.0),
            view: |ui| {
                use kargui::{Rect, rgb};
                let values = [12.0, 30.0, 22.0, 48.0, 40.0, 62.0, 58.0];
                ui.canvas(320.0, 180.0, |d| {
                    d.rounded_rect(Rect::new(0.0, 0.0, 320.0, 180.0), 8.0, rgb(0xf3f5f8));
                    for k in 0..4 {
                        let y = 150.0 - k as f32 * 40.0;
                        d.rect(Rect::new(36.0, y, 270.0, 1.0), rgb(0xdde2ea));
                        d.text((8.0, y - 9.0), format!("{}", k * 20), rgb(0x6b7280));
                    }
                    let pts: Vec<(f32, f32)> = values.iter().enumerate().map(|(k, &v)| (48.0 + k as f32 * 42.0, 150.0 - v * 2.0)).collect();
                    let mut area = pts.clone();
                    area.push((pts[pts.len() - 1].0, 150.0));
                    area.push((pts[0].0, 150.0));
                    d.polygon(&area, [0.2, 0.45, 0.9, 0.15]);
                    d.polyline(&pts, 2.5, rgb(0x2f6fd6));
                    for &p in &pts {
                        d.circle(p, 4.0, rgb(0x2f6fd6));
                    }
                })
                .label("Weekly visits");
            },
            setup: no_setup,
        },
        Shot {
            name: "drag_and_drop",
            size: (440.0, 200.0),
            view: |ui| {
                ui.row(|ui| {
                    for (title, cards) in [("To do", &["Write the guide", "Take the pictures"][..]), ("Done", &["Release 0.2.0"][..])] {
                        let mut column = ui
                            .card(|ui| {
                                ui.heading(title);
                                for (k, card) in cards.iter().enumerate() {
                                    ui.card(|ui| {
                                        ui.label(*card);
                                    })
                                    .drag_source(k);
                                }
                            })
                            .width(kargui::Size::Fill(1.0))
                            .height(kargui::Size::Fill(1.0));
                        let _ = column.dropped::<usize>();
                    }
                })
                .gap(12.0)
                .grow();
            },
            // 引っ張ったまま止める: 受け取る列の強調と、ポインターについて動く箱を写す
            setup: |h| {
                h.drag_over("Take the pictures", "Release 0.2.0");
            },
        },
        Shot {
            name: "tree",
            size: (320.0, 240.0),
            view: |ui| {
                let mut sel = Some(4);
                ui.tree(&mut sel, |t| {
                    t.branch(1, "Documents", |t| {
                        t.leaf(2, "report-2026.pdf");
                        t.branch(3, "Photos", |t| {
                            t.leaf(4, "cat.jpg");
                            t.leaf(5, "sea.jpg");
                        });
                    });
                    t.branch(6, "Music", |t| {
                        t.leaf(7, "song.mp3");
                    });
                    t.leaf(8, "notes.txt");
                })
                .label("Files");
            },
            // Documents と Photos を開く（開閉の状態は kargui が持つ）
            setup: |h| {
                use kargui::Key;
                h.press(Key::Tab).press(Key::Up).press(Key::Up).press(Key::Up).press(Key::Up).press(Key::Right).press(Key::Down).press(Key::Down).press(Key::Right).press(Key::Down);
            },
        },
        Shot {
            name: "table",
            size: (520.0, 210.0),
            view: |ui| {
                use kargui::ui::TableColumn;
                let mut sort = Some((1, false));
                let mut sel = Some(1);
                let rows = [("photos-2026.zip", "1.2 GB", "Archive"), ("report-2026.pdf", "3.4 MB", "PDF"), ("notes.txt", "12 KB", "Text"), ("plan.md", "4 KB", "Markdown")];
                let cols = [TableColumn::new("Name").width(200.0), TableColumn::new("Size").width(90.0).right(), TableColumn::new("Kind").fill()];
                ui.table(&cols, rows.len(), 30.0).sort(&mut sort).selection(&mut sel).label("Files").grow().show(|row, i| {
                    row.text(rows[i].0);
                    row.text(rows[i].1);
                    row.text(rows[i].2);
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "date_picker",
            size: (340.0, 360.0),
            view: |ui| {
                let mut due = kargui::Date::new(2026, 11, 25);
                ui.date_picker(&mut due).label("Due date");
            },
            setup: |h| {
                h.click("Due date");
            },
        },
        Shot {
            name: "time_picker",
            size: (580.0, 250.0),
            view: |ui| {
                let mut start = kargui::Time::new(9, 30);
                ui.time_picker(&mut start).label("Start");
            },
            setup: |h| {
                h.click("Start");
            },
        },
        Shot {
            name: "color_picker",
            size: (300.0, 360.0),
            view: |ui| {
                let mut fill = kargui::rgb(0x1a73e8);
                ui.color_picker(&mut fill).label("Fill");
            },
            setup: |h| {
                h.click("Fill");
            },
        },
        Shot {
            name: "more_widgets",
            size: (560.0, 330.0),
            view: |ui| {
                ui.row(|ui| {
                    ui.column(|ui| {
                        let mut size = 1;
                        ui.radio_group(&mut size, &[(0, "Small"), (1, "Medium"), (2, "Large")]);
                        ui.separator();
                        ui.checkbox(&mut false, "Select all").mixed(true);
                        ui.row(|ui| {
                            ui.toggle_button(&mut true, "Bold");
                            ui.toggle_button(&mut false, "Italic");
                        });
                        ui.link("How do backups work?");
                    })
                    .width(Size::Fixed(220.0));
                    ui.separator();
                    ui.column(|ui| {
                        ui.group("Upload", |ui| {
                            ui.progress(0.4).label("Upload");
                            ui.row(|ui| {
                                ui.spinner().label("Loading");
                                ui.label("Connecting…").weak();
                            });
                        });
                        let mut open = true;
                        ui.expander(&mut open, "Advanced", |ui| {
                            ui.password(&mut String::from("hunter2")).hint("Password").grow();
                        });
                    })
                    .grow();
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "icons",
            size: (520.0, 110.0),
            view: |ui| {
                ui.row(|ui| {
                    for icon in kargui::Icon::ALL {
                        ui.icon(icon);
                    }
                });
                ui.row(|ui| {
                    ui.button("New task").icon(kargui::Icon::Plus).primary();
                    ui.button("Search").icon(kargui::Icon::Search);
                    ui.icon_button(kargui::Icon::More, "More");
                    ui.icon_button(kargui::Icon::Close, "Close");
                    ui.icon(kargui::Icon::Info).size(20.0).weak();
                    ui.label("Saved").weak();
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "image",
            size: (360.0, 120.0),
            view: |ui| {
                // 左上が赤、右下が青の、だんだん変わる 32×16 の絵（試しの画像。ファイルを読まずに作る）
                let (w, h) = (32u32, 16u32);
                let mut px = Vec::with_capacity((w * h * 4) as usize);
                for y in 0..h {
                    for x in 0..w {
                        px.extend([(255 - x * 8) as u8, (y * 16) as u8, (x * 8) as u8, 255]);
                    }
                }
                let img = kargui::Image::from_rgba(w, h, px).unwrap();
                ui.row(|ui| {
                    ui.image(&img);
                    ui.image(&img).size(128.0, 64.0).label("Gradient");
                    ui.label("32 × 16 and 4×");
                });
            },
            setup: no_setup,
        },
        Shot {
            name: "tooltip",
            size: (320.0, 100.0),
            view: |ui| {
                ui.row(|ui| {
                    ui.button("Save").tooltip("Saves the file (Ctrl+S)");
                    ui.button("Share");
                });
            },
            // キーボードで焦点を当てると、すぐに出る
            setup: |h| {
                h.press(kargui::Key::Tab);
            },
        },
    ]
}

/// 写真と、その中で文字の形を描いた四角
fn render(shot: &Shot, dark: bool) -> (kargui::test::Image, Vec<[i32; 4]>) {
    let mut h = Harness::with_size(ShotApp { view: shot.view, dark }, shot.size.0, shot.size.1, 1.0);
    h.frame();
    (shot.setup)(&mut h);
    (h.render(), h.glyph_boxes())
}

/// 部品の写真を作り直す（check なら、保存してある物と比べるだけ）
fn make_shots(check: bool) -> bool {
    let dir = root().join("docs/shots");
    fs::create_dir_all(&dir).unwrap();
    // 写真は手元の Windows で作る。CI の Windows は書体や DirectWrite の版が違い、同じ文字でも
    // 形が数画素ずれることがある（色の選択の「#1A73E8」で起きた）。そこで CI では、違いが全部
    // 文字の形の中なら通す。文字の外（色・枠・配置・絵）は 1 画素でも落とす。手元は完全一致
    let lenient = std::env::var_os("CI").is_some();
    let mut ok = true;
    let mut gallery = String::from("# Widgets\n\nGenerated by `cargo run -p xtask -- shots`. Light and dark mode, 100% scale.\n\n");
    for shot in shots() {
        gallery.push_str(&format!("## {}\n\n", shot.name));
        for (dark, suffix) in [(false, "light"), (true, "dark")] {
            let file = format!("{}-{suffix}.png", shot.name);
            let path = dir.join(&file);
            let (img, glyphs) = render(&shot, dark);
            if check {
                let tmp = std::env::temp_dir().join(format!("kargui-shot-{file}"));
                img.save_png(&tmp).unwrap();
                let (now, saved) = (fs::read(&tmp).ok(), fs::read(&path).ok());
                if now != saved {
                    let diff = match (&now, &saved) {
                        (Some(now), Some(saved)) => describe_diff(now, saved, &glyphs),
                        _ => Diff { lines: vec!["（保存してある写真が無い）".into()], only_in_text: false },
                    };
                    if lenient && diff.only_in_text {
                        println!("shots: {file} は文字の形だけ違う（CI の書体の版の差として通す）");
                    } else {
                        ok = false;
                        println!("shots: {file} が今の見た目と違う（cargo run -p xtask -- shots で作り直す）");
                    }
                    for line in diff.lines {
                        println!("shots:   {line}");
                    }
                }
            } else {
                img.save_png(&path).unwrap();
            }
            gallery.push_str(&format!("![{} ({suffix})](shots/{file})\n", shot.name));
        }
        gallery.push('\n');
    }
    let gallery_path = root().join("docs/widgets.md");
    if check {
        if fs::read_to_string(&gallery_path).ok().as_deref() != Some(gallery.as_str()) {
            ok = false;
            println!("shots: docs/widgets.md が古い");
        }
    } else {
        fs::write(&gallery_path, gallery).unwrap();
    }
    println!("shots: {}", if ok { "ok" } else { "違いあり" });
    ok
}

/// kargui の save_png が書いた PNG（圧縮しない 1 つの IDAT、各行は 0 の後に RGB）を読み戻す
fn read_shot(png: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let (mut i, mut size, mut z) = (8, None, Vec::new());
    while i + 8 <= png.len() {
        let (len, kind) = (be(&png[i..]), &png[i + 4..i + 8]);
        let data = png.get(i + 8..i + 8 + len)?;
        match kind {
            b"IHDR" => size = Some((be(data), be(&data[4..]))),
            b"IDAT" => z.extend_from_slice(data),
            _ => {}
        }
        i += 12 + len;
    }
    let (w, h) = size?;
    let (mut raw, mut j) = (Vec::new(), 2);
    loop {
        let last = *z.get(j)? & 1 == 1;
        let n = u16::from_le_bytes([*z.get(j + 1)?, *z.get(j + 2)?]) as usize;
        raw.extend_from_slice(z.get(j + 5..j + 5 + n)?);
        j += 5 + n;
        if last {
            break;
        }
    }
    let rgb = raw.chunks(1 + w * 3).take(h).flat_map(|row| row[1..].to_vec()).collect::<Vec<u8>>();
    (rgb.len() == w * h * 3).then_some((w, h, rgb))
}

struct Diff {
    lines: Vec<String>,
    /// 違う画素が全部、文字の形の四角（まわり GLYPH_MARGIN 画素まで）の中にある
    only_in_text: bool,
}

/// 文字の形の四角を広げる幅。版の違う書体では、形の大きさや字間が 1〜2 画素変わる
const GLYPH_MARGIN: i32 = 3;

/// 2 枚の写真の違う所を、8 画素の升でまとめて、つながった範囲ごとに 1 行で言う。
/// glyphs は今の写真で文字の形を描いた四角
fn describe_diff(now: &[u8], saved: &[u8], glyphs: &[[i32; 4]]) -> Diff {
    let (Some((w, h, a)), Some((w2, h2, b))) = (read_shot(now), read_shot(saved)) else {
        return Diff { lines: vec!["（読み戻せないので画素では比べられない）".into()], only_in_text: false };
    };
    if (w, h) != (w2, h2) {
        return Diff { lines: vec![format!("大きさが違う: 今 {w}×{h}、保存 {w2}×{h2}")], only_in_text: false };
    }
    let in_text = |x: usize, y: usize| {
        let (x, y, m) = (x as i32, y as i32, GLYPH_MARGIN);
        glyphs.iter().any(|g| x >= g[0] - m && x < g[0] + g[2] + m && y >= g[1] - m && y < g[1] + g[3] + m)
    };
    const T: usize = 8;
    let (tw, th) = (w.div_ceil(T), h.div_ceil(T));
    let mut tiles = vec![false; tw * th];
    let (mut count, mut outside, mut most) = (0, 0, 0u8);
    let mut out_box = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let k = (y * w + x) * 3;
            let d = (0..3).map(|c| a[k + c].abs_diff(b[k + c])).max().unwrap();
            if d > 0 {
                count += 1;
                most = most.max(d);
                tiles[(y / T) * tw + x / T] = true;
                if !in_text(x, y) {
                    outside += 1;
                    out_box = (out_box.0.min(x), out_box.1.min(y), out_box.2.max(x), out_box.3.max(y));
                }
            }
        }
    }
    let mut out = vec![format!("違う画素 {count} 個（うち文字の形の外 {outside} 個、色の差は最大 {most}）")];
    if outside > 0 {
        out.push(format!("文字の形の外の違いは x {}〜{}、y {}〜{}", out_box.0, out_box.2, out_box.1, out_box.3));
    }
    let mut seen = vec![false; tw * th];
    for start in 0..tiles.len() {
        if !tiles[start] || seen[start] {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(t) = stack.pop() {
            let (tx, ty) = (t % tw, t / tw);
            (x0, y0, x1, y1) = (x0.min(tx), y0.min(ty), x1.max(tx), y1.max(ty));
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (nx, ny) = (tx as i32 + dx, ty as i32 + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < tw && (ny as usize) < th {
                    let n = ny as usize * tw + nx as usize;
                    if tiles[n] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        out.push(format!("範囲 x {}〜{}、y {}〜{}", x0 * T, ((x1 + 1) * T).min(w) - 1, y0 * T, ((y1 + 1) * T).min(h) - 1));
    }
    Diff { lines: out, only_in_text: outside == 0 }
}

/// AI 向けの索引。ソースの説明のコメントから作るので、手で書いた別の文書と食い違わない
fn make_llms(check: bool) -> bool {
    let src = ui_source();
    let module_doc: Vec<&str> =
        src.lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!").strip_prefix(' ').unwrap_or("")).collect();
    let test_src = fs::read_to_string(root().join("crates/kargui/src/test.rs")).unwrap();
    let test_doc: Vec<&str> =
        test_src.lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!").strip_prefix(' ').unwrap_or("")).collect();
    let mut out = String::new();
    out.push_str("# kargui\n\n");
    out.push_str("> A light native GUI framework for Rust (Windows 10/11). Immediate-mode code, CPU rendering that only redraws what changed, input methods (IME) and screen readers built in.\n\n");
    out.push_str("This file is generated from the source by `cargo run -p xtask -- llms`. Do not edit by hand.\n\n");
    out.push_str("## Writing a screen\n\n");
    out.push_str(&module_doc.join("\n"));
    out.push_str("\n\n## Widgets and layout (methods of `Ui`)\n\n");
    for it in items(&src, "impl<'a> Ui<'a> {") {
        out.push_str(&format!("### `{}`\n\n", it.signature));
        // 例の中の「# 」で始まる隠した行は、AI にも見せない（人の読む説明と同じにする）
        let doc: Vec<&str> = it.docs.iter().map(String::as_str).filter(|l| !l.starts_with("# ")).collect();
        out.push_str(&doc.join("\n"));
        out.push_str("\n\n");
    }
    // 部品を作った後に続けて呼ぶメソッド（List::selection、TextInput::submitted など）も、AI が見つけられるように載せる
    for (title, header) in [
        ("Every widget (methods of `Handle`; most widgets also have `width`, `grow`, `label`, `id`, `scroll_into_view`)", "impl Handle<'_> {"),
        ("Text (`Label`, from `ui.label` and `ui.heading`)", "impl Label<'_> {"),
        ("Buttons (`Button`)", "impl Button<'_> {"),
        ("Check boxes and switches (`Toggled`)", "impl Toggled<'_> {"),
        ("Sliders (`Slider`)", "impl Slider<'_> {"),
        ("Text fields (`TextInput`)", "impl TextInput<'_> {"),
        ("Lists (`List`, from `ui.list`)", "impl<'u> List<'u, '_> {"),
    ] {
        out.push_str(&format!("## {title}\n\n"));
        for it in items(&src, header) {
            out.push_str(&format!("### `{}`\n\n", it.signature));
            let doc: Vec<&str> = it.docs.iter().map(String::as_str).filter(|l| !l.starts_with("# ")).collect();
            out.push_str(&doc.join("\n"));
            out.push_str("\n\n");
        }
    }
    out.push_str("## Testing without a window\n\n");
    out.push_str(&test_doc.join("\n"));
    out.push('\n');
    let path = root().join("docs/llms.txt");
    let ok = if check {
        let same = fs::read_to_string(&path).ok().as_deref() == Some(out.as_str());
        if !same {
            println!("llms: docs/llms.txt が古い（cargo run -p xtask -- llms で作り直す）");
        }
        same
    } else {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, out).unwrap();
        true
    };
    println!("llms: {}", if ok { "ok" } else { "違いあり" });
    ok
}

/// rustdoc の --show-coverage の JSON が書かれたファイル。Rust 1.95 までは JSON を標準出力に出すので None。
/// 1.100 からはファイル（target/doc/kargui.json）に書き、標準出力には `Generated output into "<場所>"` を
/// Debug の形（Windows では \ が二重）で出す
fn coverage_file(stdout: &str) -> Option<PathBuf> {
    let line = stdout.lines().find(|l| l.starts_with("Generated output into "))?;
    let (start, end) = (line.find('"')?, line.rfind('"')?);
    (end > start).then(|| PathBuf::from(line[start + 1..end].replace("\\\\", "\\").replace("\\\"", "\"")))
}

/// 公開項目の全部に説明と例があるか。rustdoc の --show-coverage（stable では RUSTC_BOOTSTRAP=1）の JSON を数える
fn coverage() -> bool {
    let out = std::process::Command::new("cargo")
        .current_dir(root())
        .env("RUSTC_BOOTSTRAP", "1")
        .args(["rustdoc", "-q", "-p", "kargui", "--offline", "--", "-Z", "unstable-options", "--show-coverage", "--output-format", "json"])
        .output()
        .expect("cargo rustdoc");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let json = match coverage_file(&stdout) {
        Some(path) => fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        None => stdout,
    };
    let sum = |key: &str| -> u64 {
        json.match_indices(&format!("\"{key}\":")).filter_map(|(i, m)| json[i + m.len()..].split(|c: char| !c.is_ascii_digit()).next()?.parse::<u64>().ok()).sum()
    };
    let (total, docs, ex_total, ex) = (sum("total"), sum("with_docs"), sum("total_examples"), sum("with_examples"));
    let ok = out.status.success() && total > 0 && docs == total && ex == ex_total;
    println!("coverage: 説明 {docs}/{total}、例 {ex}/{ex_total}: {}", if ok { "ok" } else { "不足あり" });
    if !out.status.success() {
        println!("{}", String::from_utf8_lossy(&out.stderr));
    }
    ok
}

/// dir の下の .rs を全部
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn md_files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.ends_with(".md")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// ガイドの検査（guide.rs を参照）。update なら、扱う API の値と訳の元の値を入れ直す
fn guide_check(update: bool) -> bool {
    let mut files = Vec::new();
    rust_files(&root().join("crates/kargui/src"), &mut files);
    let mut api = BTreeMap::new();
    for f in &files {
        guide::api(&fs::read_to_string(f).unwrap(), &mut api);
    }
    let dir = root().join("docs/guide");
    let mut ok = true;
    let mut pages = 0;
    // 日本語を先に（英語の訳の元の値は、日本語のページから作る）
    for lang in ["ja", "en"] {
        let names = md_files(&dir.join(lang));
        if lang == "en" {
            for ja in md_files(&dir.join("ja")) {
                if !names.contains(&ja) {
                    ok = false;
                    println!("guide-check: en/{ja} が無い（ja/{ja} の訳）");
                }
            }
        }
        for name in names {
            pages += 1;
            let rel = format!("{lang}/{name}");
            let path = dir.join(lang).join(&name);
            let text = fs::read_to_string(&path).unwrap();
            let mut new = text.clone();
            match guide::covers(&text) {
                Some((covered, sig)) => match guide::signature_hash(&covered, &api) {
                    Err(missing) => {
                        ok = false;
                        println!("guide-check: {rel}: ソースに無い API {missing:?}");
                    }
                    Ok(h) if sig.as_deref() != Some(h.as_str()) => {
                        if update {
                            new = guide::stamp(&new, "covers:", "sig", &h);
                        } else {
                            ok = false;
                            println!("guide-check: {rel}: 扱う API の宣言が変わった（ページを見直してから guide-check --update）");
                        }
                    }
                    Ok(_) => {}
                },
                None if name != "README.md" => {
                    ok = false;
                    println!("guide-check: {rel}: 先頭に <!-- covers: ... --> が無い");
                }
                None => {}
            }
            if lang == "en" {
                match guide::source(&text) {
                    Some((src, h)) => match fs::read_to_string(dir.join(&src)) {
                        Ok(ja) => {
                            let want = guide::source_hash(&ja);
                            if h.as_deref() != Some(want.as_str()) {
                                if update {
                                    new = guide::stamp(&new, "source:", "fnv", &want);
                                } else {
                                    ok = false;
                                    println!("guide-check: {rel}: 元の {src} が変わった（訳を直してから guide-check --update）");
                                }
                            }
                        }
                        Err(_) => {
                            ok = false;
                            println!("guide-check: {rel}: 元の {src} が無い");
                        }
                    },
                    None => {
                        ok = false;
                        println!("guide-check: {rel}: 先頭に <!-- source: ja/... --> が無い");
                    }
                }
            }
            if update && new != text {
                fs::write(&path, new).unwrap();
                println!("guide-check: {rel} の値を入れ直した");
            }
        }
    }
    println!("guide-check: ページ {pages} 枚、公開の API {} 個: {}", api.len(), if ok { "ok" } else { "不足あり" });
    ok
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let ok = match args.first().map(String::as_str) {
        Some("doc-check") => doc_check(),
        Some("shots") => make_shots(check),
        Some("llms") => make_llms(check),
        Some("guide-check") => guide_check(args.iter().any(|a| a == "--update")),
        Some("coverage") => coverage(),
        Some("check") => {
            let a = doc_check();
            let b = make_shots(true);
            let c = make_llms(true);
            let d = guide_check(false);
            let e = coverage();
            a && b && c && d && e
        }
        _ => {
            println!("使い方: cargo run -p xtask -- doc-check | shots [--check] | llms [--check] | guide-check [--update] | coverage | check");
            false
        }
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_reads_docs_and_signatures_inside_one_impl() {
        let src = "impl Other {\n    /// no\n    pub fn other() {}\n}\nimpl<'a> Ui<'a> {\n    /// A button.\n    ///\n    /// ```\n    /// example(|ui| {\n    /// ```\n    #[track_caller]\n    pub fn button(&mut self) -> B {\n        if x { format!(\"{}\") }\n    }\n    fn private() {}\n    pub fn bare(&self) {}\n}\nimpl After { pub fn after() {} }\n";
        let list = items(src, "impl<'a> Ui<'a> {");
        let names: Vec<_> = list.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["button", "bare"]);
        assert_eq!(list[0].docs, ["A button.", "", "```", "example(|ui| {", "```"]);
        assert!(list[1].docs.is_empty());
        assert_eq!(list[0].signature, "pub fn button(&mut self) -> B");
    }

    #[test]
    fn coverage_is_read_from_stdout_or_from_the_file_rustdoc_names() {
        // Rust 1.95 まで: JSON を標準出力に出す（ファイルは無い）
        assert_eq!(coverage_file("{\"a.rs\":{\"total\":1,\"with_docs\":1}}"), None);
        // Rust 1.100 から: ファイルに書き、場所を Debug の形で出す（Windows では \ が二重になる）
        assert_eq!(coverage_file("Generated output into \"D:/k/target\\\\doc\\\\kargui.json\"\n"), Some(PathBuf::from("D:/k/target\\doc\\kargui.json")));
        assert_eq!(coverage_file("Generated output into \"/home/r/target/doc/kargui.json\"\n"), Some(PathBuf::from("/home/r/target/doc/kargui.json")));
    }
}
