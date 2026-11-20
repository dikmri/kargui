//! 説明を古くしないための道具。
//!
//! 使い方（このリポジトリの最上位で）:
//!   cargo run -p xtask -- doc-check     部品を作る関数に、説明と動く例があるか
//!   cargo run -p xtask -- shots         部品の写真（明・暗）を docs/shots/ に作り直す
//!   cargo run -p xtask -- shots --check 写真が今の見た目と同じか（違えば落とす）
//!   cargo run -p xtask -- llms          AI 向けの索引 docs/llms.txt を作り直す
//!   cargo run -p xtask -- llms --check  索引が今のソースと同じか（違えば落とす）
//!   cargo run -p xtask -- check         上の検査を全部（取り込みの前に走らせる）
//!
//! 説明そのものが無い公開項目は、kargui の #![deny(missing_docs)] で組み立ての時点で落ちる。
//! 説明の中の例は cargo test --doc で動かす（試験台 kargui::test::Harness の上で動く）。

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
                    ui.checkbox(&mut (i % 3 == 2), "").label(format!("Item {i}"));
                    ui.label(format!("Item {i}")).grow();
                    ui.button("Delete").small().width(Size::Auto);
                });
            },
            setup: no_setup,
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let ok = match args.first().map(String::as_str) {
        Some("doc-check") => doc_check(),
        Some("shots") => make_shots(check),
        Some("llms") => make_llms(check),
        Some("check") => {
            let a = doc_check();
            let b = make_shots(true);
            let c = make_llms(true);
            a && b && c
        }
        _ => {
            println!("使い方: cargo run -p xtask -- doc-check | shots [--check] | llms [--check] | check");
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
