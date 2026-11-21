<!-- covers: Ui::window App::window Harness::window Harness::main_window Harness::windows Harness::close_window sig=ef71c564feb745c1 -->
# 複数の窓

設定や詳しい表示を、別の窓に出せます。

```rust
use kargui::{App, Options, Ui};

#[derive(Default)]
struct Player {
    settings: bool,
    volume: i32,
}

impl App for Player {
    fn view(&mut self, ui: &mut Ui) {
        if ui.button("設定").clicked() {
            self.settings = true;
        }
        ui.label(format!("音量 {}", self.volume));
        let options = Options { size: (360.0, 200.0), ..Options::new("設定") };
        ui.window("settings", &options, &mut self.settings);
    }

    fn window(&mut self, key: &str, ui: &mut Ui) {
        if key == "settings" {
            ui.slider(&mut self.volume, 0..=10).label("音量");
            if ui.button("閉じる").clicked() {
                self.settings = false;
            }
        }
    }
}
# let mut h = kargui::test::Harness::new(Player::default());
# h.click("設定").window("settings");
# assert!(h.find("音量").is_some());
```

`ui.window(キー, &Options, &mut open)` は、`open` が true の間、別の窓を開いておきます。窓の題・大きさ・場所は `Options` で決めます。窓の中身は、アプリの `window(&mut self, key, ui)` で作ります。どの窓も同じアプリを使うので、ある窓で変えた値は、ほかの窓にもすぐに出ます。

利用者が窓の閉じるボタンを押すと、`open` が false になります。アプリが `open` を false にした時も、窓を閉じます。窓を開けるのは、元の窓の `view` だけです。元の窓を閉じると、アプリが終わります。

## 試験

試験台では、`h.window(キー)` でその窓へ移り、以後のクリックやキーはその窓に届きます。`h.main_window()` で元の窓へ戻ります。`h.windows()` は開いている窓のキー、`h.close_window(キー)` は閉じるボタンを押したことにします。

```rust
# use kargui::{App, Options, Ui};
# #[derive(Default)]
# struct Player { settings: bool, volume: i32 }
# impl App for Player {
#     fn view(&mut self, ui: &mut Ui) {
#         if ui.button("Settings").clicked() { self.settings = true; }
#         ui.window("settings", &Options::new("Settings"), &mut self.settings);
#     }
#     fn window(&mut self, _key: &str, ui: &mut Ui) {
#         ui.slider(&mut self.volume, 0..=10).label("Volume");
#     }
# }
use kargui::Key;
let mut h = kargui::test::Harness::new(Player::default());
h.click("Settings");
assert_eq!(h.windows(), ["settings"]);
h.window("settings").press(Key::Tab).press(Key::Right);
assert_eq!(h.app().volume, 1);
h.close_window("settings");
assert!(h.windows().is_empty());
```
