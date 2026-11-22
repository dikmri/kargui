// 設定画面（kargui の例）。タスク帳と形の違う画面として、窓より高い設定の並びをスクロールの入れ物に入れ、
// 入力欄・切り替え・スライダー・選択を並べる。「保存」と「元に戻す」は、変えた所があるかで動きを変える。
#![windows_subsystem = "windows"]

use std::time::Instant;

use kargui::{App, Color, ColorText, DateText, Font, Frame, Options, Size, Time, Ui};
use taskcore::{args, log};

const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/settings.log");

#[derive(Clone, Copy, PartialEq, Debug)]
enum Look {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Typeface {
    System,
    Readable,
    Mono,
}

impl Typeface {
    fn font(self) -> Font {
        match self {
            Typeface::System => Font::Ui,
            // Windows 10 以降に入っている、読みやすさを考えて作られた書体
            Typeface::Readable => Font::family("BIZ UDPGothic"),
            Typeface::Mono => Font::Monospace,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Startup {
    Blank,
    LastFile,
    Welcome,
}

#[derive(Clone, PartialEq, Debug)]
struct Prefs {
    name: String,
    email: String,
    look: Look,
    typeface: Typeface,
    font_size: i32,
    line_height: f32,
    autosave: bool,
    autosave_minutes: i32,
    spellcheck: bool,
    notifications: bool,
    sounds: bool,
    volume: f32,
    startup: Startup,
    statistics: bool,
    /// 強調の色（保存するだけ。見本は色の選択を見せる）
    accent: Color,
    /// おやすみの時間（開始・終了）に通知を止める
    quiet: bool,
    quiet_from: Option<Time>,
    quiet_to: Option<Time>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            name: String::new(),
            email: String::new(),
            look: Look::System,
            typeface: Typeface::System,
            font_size: 14,
            line_height: 1.5,
            autosave: true,
            autosave_minutes: 5,
            spellcheck: true,
            notifications: true,
            sounds: false,
            volume: 0.6,
            startup: Startup::LastFile,
            statistics: false,
            accent: kargui::rgb(0x1a73e8),
            quiet: false,
            quiet_from: Time::new(22, 0),
            quiet_to: Time::new(7, 0),
        }
    }
}

impl Prefs {
    /// 直すべき所（無ければ None）
    fn problem(&self) -> Option<&'static str> {
        if !self.email.is_empty() && !self.email.contains('@') {
            Some("メールの形が正しくありません（@ がありません）")
        } else {
            None
        }
    }
}

struct Settings {
    saved: Prefs,
    edit: Prefs,
    message: String,
    started: Instant,
    ready: bool,
    /// 統計で送る中身の例を出している（設定ではないので保存しない）
    show_detail: bool,
}

/// 区切りの見出し
fn section(ui: &mut Ui, title: &str) {
    ui.space(10.0);
    ui.label(title).bold();
}

/// 左に名前、右に部品を置いた 1 行
fn field(ui: &mut Ui, name: &str, f: impl FnOnce(&mut Ui)) {
    ui.row(|ui| {
        ui.label(name).width(Size::Fixed(130.0));
        f(ui);
    });
}

impl App for Settings {
    fn view(&mut self, ui: &mut Ui) {
        if self.edit.look != Look::System {
            ui.set_dark(self.edit.look == Look::Dark);
        }
        // 書体は選んだ時からすぐ窓全体に当てる（テーマと同じ）
        ui.set_font(self.edit.typeface.font());
        ui.set_date_text(DateText::japanese());
        ui.set_color_text(ColorText::japanese());
        ui.row(|ui| {
            ui.heading("設定");
            ui.fill();
            ui.label(&self.message).weak();
        });
        let e = &mut self.edit;
        ui.scroll(|ui| {
            section(ui, "アカウント");
            field(ui, "名前", |ui| {
                ui.text_input(&mut e.name).hint("表示する名前").grow();
            });
            field(ui, "メール", |ui| {
                ui.text_input(&mut e.email).hint("name@example.com").grow();
            });
            if let Some(p) = e.problem() {
                ui.label(p).small();
            }

            section(ui, "表示");
            field(ui, "テーマ", |ui| {
                ui.segmented(&mut e.look, &[(Look::System, "システム"), (Look::Light, "ライト"), (Look::Dark, "ダーク")]);
            });
            field(ui, "書体", |ui| {
                let before = e.typeface;
                ui.segmented(&mut e.typeface, &[(Typeface::System, "システム"), (Typeface::Readable, "BIZ UDP ゴシック"), (Typeface::Mono, "等幅")]);
                if e.typeface != before {
                    log::event("typeface", &format!("{before:?}"), &format!("{:?}", e.typeface));
                }
            });
            field(ui, "文字の大きさ", |ui| {
                ui.slider(&mut e.font_size, 10..=24).label("文字の大きさ");
                ui.label(format!("{} pt", e.font_size));
            });
            field(ui, "行の高さ", |ui| {
                ui.slider(&mut e.line_height, 1.0..=2.0).label("行の高さ");
                ui.label(format!("{:.2}", e.line_height));
            });
            field(ui, "強調の色", |ui| {
                if ui.color_picker(&mut e.accent).label("強調の色").changed() {
                    let [r, g, b, _] = e.accent.map(|c| (c * 255.0).round() as u8);
                    log::event("accent", "", &format!("#{r:02X}{g:02X}{b:02X}"));
                }
            });
            // 書式付きの文字の見本（強調の色の部分は、選んだ色で描く）
            ui.rich_text(|t| {
                t.text("見本: ").weak();
                t.text("強調の色").bold().color(e.accent);
                t.text(" の文字、");
                t.text("斜体").italic();
                t.text("、");
                t.text("下線").underline();
                t.text("、");
                t.text("取り消し線").strike();
                t.text("、");
                t.text("Ctrl+S").code();
                t.text("、");
                t.text("小さい字").small().weak();
            });

            section(ui, "編集");
            ui.toggle(&mut e.autosave, "自動で保存する");
            // 保存の間隔は、自動で保存する間だけ上から出す（OS で動きを切っていれば、すぐ出し入れする）
            let shown = ui.animate_bool("autosave-interval", e.autosave);
            if shown > 0.0 {
                ui.column(|ui| {
                    field(ui, "保存の間隔", |ui| {
                        ui.slider(&mut e.autosave_minutes, 1..=30).label("保存の間隔");
                        ui.label(format!("{} 分ごと", e.autosave_minutes));
                    });
                })
                .reveal(shown);
            }
            ui.checkbox(&mut e.spellcheck, "つづりを確かめる");

            section(ui, "通知");
            ui.toggle(&mut e.notifications, "通知を出す");
            ui.toggle(&mut e.sounds, "音を鳴らす");
            field(ui, "音量", |ui| {
                ui.slider(&mut e.volume, 0.0..=1.0).label("音量");
                ui.label(format!("{:.0}%", e.volume * 100.0));
            });
            ui.toggle(&mut e.quiet, "おやすみの時間に通知を止める");
            field(ui, "おやすみの時間", |ui| {
                let from = ui.time_picker(&mut e.quiet_from).label("開始").changed();
                ui.label("から");
                let to = ui.time_picker(&mut e.quiet_to).label("終了").changed();
                if from || to {
                    let show = |t: Option<Time>| t.map_or("-".to_string(), |t| t.to_string());
                    log::event("quiet", &show(e.quiet_from), &show(e.quiet_to));
                }
            });

            section(ui, "起動");
            field(ui, "起動した時", |ui| {
                ui.segmented(&mut e.startup, &[(Startup::Blank, "空の画面"), (Startup::LastFile, "前のファイル"), (Startup::Welcome, "案内")]);
            });
            ui.checkbox(&mut e.statistics, "使い方の統計を送る（名前とメールは送らない）");
            let detail = &mut self.show_detail;
            ui.rich_text(|t| {
                t.text("送るのは ");
                t.text("起動した回数").bold();
                t.text(" と ");
                t.text("使った機能の名前").bold();
                t.text(" だけで、書いた文章やファイルの名前は送りません。");
                let label = if *detail { "送る中身の例を隠す" } else { "送る中身の例を見る" };
                if t.link(label).clicked() {
                    *detail = !*detail;
                    log::event("statistics.detail", label, &format!("shown={detail}"));
                }
            });
            if *detail {
                ui.rich_text(|t| {
                    t.text("例: ").weak();
                    t.text(r#"{"launches": 12, "features": ["save", "search"]}"#).code();
                });
            }
        });

        ui.row(|ui| {
            let changed = self.edit != self.saved;
            ui.label(if changed { "変えた所があります" } else { "保存済み" }).weak();
            ui.fill();
            if ui.button("元に戻す").clicked() {
                if changed {
                    self.edit = self.saved.clone();
                    self.message = "元に戻しました".into();
                }
                log::event("revert", &format!("changed={changed}"), "ok");
            }
            if ui.button("保存").primary().clicked() {
                match self.edit.problem() {
                    Some(p) => {
                        self.message = "直す所があります".into();
                        log::event("save", "", &format!("rejected={p}"));
                    }
                    None => {
                        self.saved = self.edit.clone();
                        self.message = "保存しました".into();
                        log::event("save", &format!("{:?}", self.saved), "ok");
                    }
                }
            }
        });
    }

    fn presented(&mut self, _frame: &Frame) {
        if !self.ready {
            self.ready = true;
            log::event("ready", "app=settings", &format!("since_main_ms={}", self.started.elapsed().as_millis()));
        }
    }
}

fn main() {
    let started = Instant::now();
    let a = args::from_env();
    let path = log::init("kargui-settings", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    log::event("startup", "app=settings", &format!("log={path:?}"));
    let app = Settings { saved: Prefs::default(), edit: Prefs::default(), message: String::new(), started, ready: false, show_detail: false };
    let mut options = Options::new("設定（kargui）");
    options.size = (560.0, 520.0);
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
    use kargui::Key;
    use kargui::test::Harness;

    fn app() -> Harness<Settings> {
        let s = Settings { saved: Prefs::default(), edit: Prefs::default(), message: String::new(), started: Instant::now(), ready: false, show_detail: false };
        let mut h = Harness::with_size(s, 560.0, 520.0, 1.0);
        h.frame();
        h
    }

    #[test]
    fn saving_and_reverting() {
        let mut h = app();
        h.click("表示する名前").type_text("Hana");
        assert!(h.names().contains(&"変えた所があります".to_string()));
        h.click("保存");
        assert_eq!(h.app().saved.name, "Hana");
        h.click("ダーク").click("元に戻す");
        assert_eq!(h.app().edit.look, Look::System);
    }

    #[test]
    fn the_saving_interval_shows_only_while_autosave_is_on() {
        let mut h = app();
        assert!(h.find("保存の間隔").is_some());
        h.click("自動で保存する");
        assert!(!h.app().edit.autosave);
        assert!(h.find("保存の間隔").is_none(), "試験台は動きを切ってあるので、すぐ消える");
        // 動きを入れると、閉じる間も描き、開く時は上から出てくる
        h.reduce_motion(false).click("自動で保存する");
        assert!(h.find("保存の間隔").is_none(), "押したフレームは高さ 0");
        h.wait(std::time::Duration::from_millis(100));
        assert!(h.find("保存の間隔").is_some());
    }

    #[test]
    fn the_typeface_applies_to_the_whole_window_at_once() {
        let mut h = app();
        // 英字を含む表示で測る（日本語だけの「システム」は、英字だけの等幅の書体（BIZ UD ゴシックの無い CI の Windows の
        // Consolas）では代わりの書体で出て、幅が変わらなかった）
        let width = |h: &Harness<Settings>| h.find("14 pt").unwrap().w;
        let before = width(&h);
        h.click("等幅");
        assert_eq!(h.app().edit.typeface, Typeface::Mono);
        assert_ne!(width(&h), before, "窓の書体が変わると、ほかの文字の幅も変わる");
        h.click("元に戻す");
        assert_eq!(width(&h), before);
    }

    #[test]
    fn the_accent_color_and_quiet_hours_are_picked_in_popups() {
        // 通知の節まで見える高さにする（520 では開始の欄が見える範囲の外）
        let s = Settings { saved: Prefs::default(), edit: Prefs::default(), message: String::new(), started: Instant::now(), ready: false, show_detail: false };
        let mut h = Harness::with_size(s, 560.0, 1100.0, 1.0);
        h.click("強調の色").click("#D93025");
        assert_eq!(h.app().edit.accent, kargui::rgb(0xd93025));
        h.press(kargui::Key::Escape);
        h.click("開始").click("23 時").click("30 分");
        assert_eq!(h.app().edit.quiet_from, Time::new(23, 30));
        assert!(h.names().contains(&"変えた所があります".to_string()));
    }

    #[test]
    fn a_wrong_email_is_not_saved() {
        let mut h = app();
        h.click("name@example.com").type_text("abc").click("保存");
        assert_eq!(h.app().saved.email, "");
        assert_eq!(h.app().message, "直す所があります");
    }

    #[test]
    fn every_setting_is_reachable_with_tab_and_the_slider_follows_keys() {
        let mut h = app();
        // 文字の大きさのスライダーまで Tab で移る（名前・メール・テーマ 3 つ・書体 3 つ・スライダー）
        for _ in 0..9 {
            h.press(Key::Tab);
        }
        h.press(Key::Right).press(Key::Right);
        assert_eq!(h.app().edit.font_size, 16);
        // 下の方の設定にも Tab で届き、見えるようにスクロールする（行の高さ・強調の色・自動保存・間隔・つづり・通知・音・
        // 音量・おやすみ・開始・終了・起動 3 つ・統計の 15 回）
        for _ in 0..15 {
            h.press(Key::Tab);
        }
        h.press(Key::Space);
        assert!(h.app().edit.statistics, "最後のチェックまで届く");
        // 統計の説明のリンクは、チェックの次に Tab で届き、Enter で例を出す
        h.press(Key::Tab).press(Key::Enter);
        assert!(h.app().show_detail);
    }

    #[test]
    fn the_statistics_link_shows_and_hides_an_example() {
        let s = Settings { saved: Prefs::default(), edit: Prefs::default(), message: String::new(), started: Instant::now(), ready: false, show_detail: false };
        let mut h = Harness::with_size(s, 560.0, 1200.0, 1.0);
        assert!(h.find("例: ").is_none());
        h.click("送る中身の例を見る");
        assert!(h.app().show_detail && h.find("例: ").is_some());
        h.click("送る中身の例を隠す");
        assert!(!h.app().show_detail && h.find("例: ").is_none());
        assert!(h.app().edit == h.app().saved, "例を出しても設定は変わらない");
    }
}
