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

fn render(shot: &Shot, dark: bool) -> kargui::test::Image {
    let mut h = Harness::with_size(ShotApp { view: shot.view, dark }, shot.size.0, shot.size.1, 1.0);
    h.frame();
    (shot.setup)(&mut h);
    h.render()
}

/// 部品の写真を作り直す（check なら、保存してある物と比べるだけ）
fn make_shots(check: bool) -> bool {
    let dir = root().join("docs/shots");
    fs::create_dir_all(&dir).unwrap();
    let mut ok = true;
    let mut gallery = String::from("# Widgets\n\nGenerated by `cargo run -p xtask -- shots`. Light and dark mode, 100% scale.\n\n");
    for shot in shots() {
        gallery.push_str(&format!("## {}\n\n", shot.name));
        for (dark, suffix) in [(false, "light"), (true, "dark")] {
            let file = format!("{}-{suffix}.png", shot.name);
            let path = dir.join(&file);
            let img = render(&shot, dark);
            if check {
                let tmp = std::env::temp_dir().join(format!("kargui-shot-{file}"));
                img.save_png(&tmp).unwrap();
                let same = fs::read(&tmp).ok() == fs::read(&path).ok();
                if !same {
                    ok = false;
                    println!("shots: {file} が今の見た目と違う（cargo run -p xtask -- shots で作り直す）");
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

/// 公開項目の全部に説明と例があるか。rustdoc の --show-coverage（stable では RUSTC_BOOTSTRAP=1）の JSON を数える
fn coverage() -> bool {
    let out = std::process::Command::new("cargo")
        .current_dir(root())
        .env("RUSTC_BOOTSTRAP", "1")
        .args(["rustdoc", "-q", "-p", "kargui", "--offline", "--", "-Z", "unstable-options", "--show-coverage", "--output-format", "json"])
        .output()
        .expect("cargo rustdoc");
    let json = String::from_utf8_lossy(&out.stdout);
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
}
