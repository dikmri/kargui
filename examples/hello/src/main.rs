// 最小の kargui のアプリ（見出し 1 つ）。窓の枠や起動の確かめに使う
#![windows_subsystem = "windows"]

use kargui::{App, Options, Ui};

struct Hello;

impl App for Hello {
    fn view(&mut self, ui: &mut Ui) {
        ui.heading("Hello, kargui");
    }
}

fn main() {
    let mut o = Options::new("Hello");
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--gpu") {
        o.renderer = kargui::Renderer::Gpu;
    }
    o.position = Some((-1410.0, 200.0));
    o.activate = false;
    let _ = kargui::run(o, Hello);
}
