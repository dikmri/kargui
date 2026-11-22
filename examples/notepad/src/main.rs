// メモ帳（kargui の例。ふつうのアプリで確かめる 1 本目）。メニューバー・複数行の入力欄・ファイルを開く・保存する
// ダイアログ・閉じる前の確認・窓のアイコンと最小の大きさを使う。
#![windows_subsystem = "windows"]

use std::path::{Path, PathBuf};
use std::time::Instant;

use kargui::{App, FileDialog, Frame, Image, Key, Mods, Options, Ui};
use taskcore::{args, log};

const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/notepad.log");

/// メニューとショートカットの命令
#[derive(Clone, Copy, PartialEq, Debug)]
enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    Exit,
    About,
}

/// 保存していない変更を捨ててよいかを聞いた後で、続けてする事
#[derive(Clone, Copy, PartialEq, Debug)]
enum Then {
    Close,
    New,
    Open,
}

struct Notepad {
    text: String,
    path: Option<PathBuf>,
    dirty: bool,
    /// 変更を保存するかを聞いている（答えの後でする事）
    asking: Option<Then>,
    /// 名前を付けて保存が終わったらする事（「保存」を選んだのに名前が無かった時）
    after_save: Option<Then>,
    about: bool,
    message: String,
    started: Instant,
    ready: bool,
}

impl Notepad {
    fn new(started: Instant) -> Self {
        Self { text: String::new(), path: None, dirty: false, asking: None, after_save: None, about: false, message: String::new(), started, ready: false }
    }

    fn name(&self) -> String {
        self.path.as_deref().and_then(Path::file_name).map_or_else(|| "無題".to_string(), |n| n.to_string_lossy().into_owned())
    }

    fn load(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(t) => {
                // Windows のメモ帳で作った CR LF は、入力欄と同じく LF にする
                self.text = t.replace("\r\n", "\n");
                self.path = Some(path);
                self.dirty = false;
                self.message = "開きました".into();
                log::event("open", &self.name(), "ok");
            }
            Err(e) => {
                self.message = format!("開けませんでした: {e}");
                log::event("open", &path.display().to_string(), &format!("error={e}"));
            }
        }
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        match std::fs::write(&path, &self.text) {
            Ok(()) => {
                self.path = Some(path);
                self.dirty = false;
                self.message = "保存しました".into();
                log::event("save", &self.name(), "ok");
                true
            }
            Err(e) => {
                self.message = format!("保存できませんでした: {e}");
                log::event("save", &path.display().to_string(), &format!("error={e}"));
                false
            }
        }
    }

    /// 変更を確かめてから then をする（変更があれば先に聞く）
    fn guard(&mut self, ui: &mut Ui, then: Then) {
        if self.dirty {
            self.asking = Some(then);
        } else {
            self.go(ui, then);
        }
    }

    fn go(&mut self, ui: &mut Ui, then: Then) {
        match then {
            Then::Close => ui.quit(),
            Then::New => {
                self.text.clear();
                self.path = None;
                self.dirty = false;
                self.message.clear();
            }
            Then::Open => ui.file_dialog("open", FileDialog::open().title("メモを開く").filter("テキスト", &["txt", "md"]).filter("すべてのファイル", &["*"])),
        }
    }

    fn save_as(&self, ui: &mut Ui) {
        ui.file_dialog("save", FileDialog::save().title("名前を付けて保存").file_name(self.name() + if self.path.is_none() { ".txt" } else { "" }).filter("テキスト", &["txt"]));
    }

    fn run(&mut self, ui: &mut Ui, cmd: Cmd) {
        log::event("command", &format!("{cmd:?}"), "run");
        match cmd {
            Cmd::New => self.guard(ui, Then::New),
            Cmd::Open => self.guard(ui, Then::Open),
            Cmd::Save => match self.path.clone() {
                Some(p) => {
                    self.save_to(p);
                }
                None => self.save_as(ui),
            },
            Cmd::SaveAs => self.save_as(ui),
            Cmd::Exit => self.guard(ui, Then::Close),
            Cmd::About => self.about = true,
        }
    }
}

impl App for Notepad {
    fn view(&mut self, ui: &mut Ui) {
        let mut cmd = None;
        ui.menu_bar(|bar| {
            bar.menu("ファイル", |m| {
                if m.item("新規").shortcut(Key::N, Mods::CTRL).clicked() {
                    cmd = Some(Cmd::New);
                }
                if m.item("開く…").shortcut(Key::O, Mods::CTRL).clicked() {
                    cmd = Some(Cmd::Open);
                }
                if m.item("保存").shortcut(Key::S, Mods::CTRL).clicked() {
                    cmd = Some(Cmd::Save);
                }
                if m.item("名前を付けて保存…").shortcut(Key::S, Mods::CTRL | Mods::SHIFT).clicked() {
                    cmd = Some(Cmd::SaveAs);
                }
                m.separator();
                if m.item("終了").shortcut(Key::Q, Mods::CTRL).clicked() {
                    cmd = Some(Cmd::Exit);
                }
            });
            bar.menu("ヘルプ", |m| {
                if m.item("このアプリについて").clicked() {
                    cmd = Some(Cmd::About);
                }
            });
        });
        if let Some(c) = cmd {
            self.run(ui, c);
        }
        // ファイルのダイアログの答え
        if let Some(path) = ui.chosen_file("open") {
            self.load(path);
        }
        if let Some(path) = ui.chosen_file("save") {
            if self.save_to(path) {
                if let Some(then) = self.after_save.take() {
                    self.go(ui, then);
                }
            }
        } else if ui.file_dialog_cancelled("save") {
            self.after_save = None;
        }
        if ui.text_area(&mut self.text).hint("ここに書きます").grow().changed() {
            self.dirty = true;
        }
        let name = self.name();
        let (lines, chars) = (self.text.lines().count().max(1), self.text.chars().count());
        ui.row(|ui| {
            ui.label(if self.dirty { format!("{name}（変更あり）") } else { name.clone() }).weak().small();
            ui.label(&self.message).weak().small();
            ui.fill();
            ui.label(format!("{lines} 行・{chars} 文字")).weak().small();
        });
        // 変更を保存するか
        let mut open = self.asking.is_some();
        let mut answer = None;
        ui.dialog(&mut open, "変更を保存しますか？", |ui| {
            ui.label(format!("「{name}」には保存していない変更があります。"));
            ui.row(|ui| {
                ui.fill();
                if ui.button("保存").primary().clicked() {
                    answer = Some(0);
                }
                if ui.button("保存しない").clicked() {
                    answer = Some(1);
                }
                if ui.button("キャンセル").clicked() {
                    answer = Some(2);
                }
            });
        });
        if let Some(then) = self.asking {
            match answer {
                Some(0) => {
                    self.asking = None;
                    match self.path.clone() {
                        Some(p) => {
                            if self.save_to(p) {
                                self.go(ui, then);
                            }
                        }
                        None => {
                            self.after_save = Some(then);
                            self.save_as(ui);
                        }
                    }
                }
                Some(1) => {
                    self.asking = None;
                    self.dirty = false;
                    self.go(ui, then);
                }
                // キャンセルか Esc
                Some(_) => self.asking = None,
                None if !open => self.asking = None,
                None => {}
            }
        }
        ui.dialog(&mut self.about, "このアプリについて", |ui| {
            ui.label("メモ帳（kargui の例）");
            ui.label("ふつうのアプリを kargui だけで書けるかを確かめるための見本です。").weak();
        });
    }

    fn close_requested(&mut self) -> bool {
        if self.dirty {
            self.asking = Some(Then::Close);
        }
        !self.dirty
    }

    fn presented(&mut self, _frame: &Frame) {
        if !self.ready {
            self.ready = true;
            log::event("ready", "app=notepad", &format!("since_main_ms={}", self.started.elapsed().as_millis()));
        }
    }
}

/// 窓のアイコン: 白い紙に青い線（32×32）
fn icon() -> Image {
    let mut px = vec![0u8; 32 * 32 * 4];
    for y in 0..32usize {
        for x in 0..32usize {
            let paper = (5..27).contains(&x) && (3..29).contains(&y);
            let line = paper && (9..23).contains(&x) && y >= 9 && y < 25 && (y - 9) % 5 == 0;
            let edge = paper && (x == 5 || x == 26 || y == 3 || y == 28);
            let c: [u8; 4] = if line {
                [0x2f, 0x6f, 0xd6, 0xff]
            } else if edge {
                [0x60, 0x68, 0x78, 0xff]
            } else if paper {
                [0xff, 0xff, 0xff, 0xff]
            } else {
                [0, 0, 0, 0]
            };
            px[(y * 32 + x) * 4..][..4].copy_from_slice(&c);
        }
    }
    Image::from_rgba(32, 32, px).expect("32×32")
}

fn main() {
    let started = Instant::now();
    let a = args::from_env();
    let path = log::init("kargui-notepad", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    log::event("startup", "app=notepad", &format!("log={path:?}"));
    let mut options = Options::new("メモ帳（kargui）");
    options.size = (640.0, 480.0);
    options.min_size = Some((360.0, 240.0));
    options.icon = Some(icon());
    options.position = a.pos;
    options.activate = !a.no_focus;
    if let Err(e) = kargui::run(options, Notepad::new(started)) {
        log::event("error", "run", &e.to_string());
    }
    log::event("exit", "", "ok");
}

#[cfg(test)]
mod tests {
    use super::*;
    use kargui::test::Harness;

    fn app() -> Harness<Notepad> {
        Harness::with_size(Notepad::new(Instant::now()), 640.0, 480.0, 1.0)
    }

    fn temp(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("kargui-notepad-test");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn opening_editing_and_saving_with_shortcuts() {
        let file = temp("open.txt", "一行目\r\n二行目");
        let mut h = app();
        h.press_with(Key::O, Mods::CTRL);
        assert_eq!(h.file_dialog_key().as_deref(), Some("open"));
        h.choose_files(&[file.to_str().unwrap()]);
        assert_eq!(h.app().text, "一行目\n二行目", "CR LF は LF にする");
        assert!(h.find("open.txt").is_some());
        h.click("ここに書きます").press_with(Key::End, Mods::CTRL).type_text("！");
        assert!(h.app().dirty);
        assert!(h.find("open.txt（変更あり）").is_some());
        h.press_with(Key::S, Mods::CTRL);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "一行目\n二行目！", "名前があればそのまま保存");
        assert!(!h.app().dirty);
    }

    #[test]
    fn new_asks_about_unsaved_changes() {
        let mut h = app();
        h.click("ここに書きます").type_text("下書き");
        h.press_with(Key::N, Mods::CTRL);
        assert!(h.find("変更を保存しますか？").is_some());
        h.click("キャンセル");
        assert_eq!(h.app().text, "下書き", "キャンセルなら何もしない");
        h.press_with(Key::N, Mods::CTRL).click("保存しない");
        assert_eq!(h.app().text, "", "保存しないなら新しくする");
    }

    #[test]
    fn closing_with_changes_saves_under_a_new_name_first() {
        let target = std::env::temp_dir().join("kargui-notepad-test").join("closed.txt");
        let _ = std::fs::remove_file(&target);
        let mut h = app();
        h.click("ここに書きます").type_text("消さないで");
        h.request_close();
        assert!(!h.is_closed(), "変更があれば閉じない");
        h.click("保存");
        assert_eq!(h.file_dialog_key().as_deref(), Some("save"), "名前が無いので名前を聞く");
        let _ = std::fs::create_dir_all(target.parent().unwrap());
        h.choose_files(&[target.to_str().unwrap()]);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "消さないで");
        assert!(h.is_closed(), "保存したら閉じる");
    }

    #[test]
    fn the_menu_bar_has_the_commands() {
        let mut h = app();
        h.click("ファイル");
        for name in ["新規", "開く…", "保存", "名前を付けて保存…", "終了"] {
            assert!(h.find(name).is_some(), "{name}");
        }
        h.click("終了");
        assert!(h.is_closed());
    }

    #[test]
    fn ctrl_q_exits_and_still_asks_about_unsaved_changes() {
        // 作者が実際の窓で Ctrl+Q を押して終わろうとした（2026-11-22 の記録）
        let mut h = app();
        h.click("ここに書きます").type_text("書きかけ");
        h.press_with(Key::Q, Mods::CTRL);
        assert!(h.find("変更を保存しますか？").is_some(), "変更があれば先に聞く");
        assert!(!h.is_closed());
        h.click("保存しない");
        assert!(h.is_closed());
        let mut h = app();
        h.press_with(Key::Q, Mods::CTRL);
        assert!(h.is_closed(), "変更が無ければすぐ閉じる");
    }
}
