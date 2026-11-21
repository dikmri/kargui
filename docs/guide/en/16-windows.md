<!-- source: ja/16-windows.md fnv=4ff66a89e70c9757 -->
<!-- covers: Ui::window App::window Harness::window Harness::main_window Harness::windows Harness::close_window sig=ef71c564feb745c1 -->
# More windows

Settings or details can go in a window of their own.

```rust
use kargui::{App, Options, Ui};

#[derive(Default)]
struct Player {
    settings: bool,
    volume: i32,
}

impl App for Player {
    fn view(&mut self, ui: &mut Ui) {
        if ui.button("Settings").clicked() {
            self.settings = true;
        }
        ui.label(format!("Volume {}", self.volume));
        let options = Options { size: (360.0, 200.0), ..Options::new("Settings") };
        ui.window("settings", &options, &mut self.settings);
    }

    fn window(&mut self, key: &str, ui: &mut Ui) {
        if key == "settings" {
            ui.slider(&mut self.volume, 0..=10).label("Volume");
            if ui.button("Close").clicked() {
                self.settings = false;
            }
        }
    }
}
# let mut h = kargui::test::Harness::new(Player::default());
# h.click("Settings").window("settings");
# assert!(h.find("Volume").is_some());
```

`ui.window(key, &Options, &mut open)` keeps another window open while `open` is true. `Options` sets its title, size and position. The window's content comes from the app's `window(&mut self, key, ui)`. All windows share the app, so a value changed in one window shows in the others at once.

When the user clicks a window's close button, `open` becomes false. When the app sets `open` to false, the window closes too. Only the main window's `view` opens windows. Closing the main window ends the app.

## Tests

In the test harness, `h.window(key)` moves to that window, and the following clicks and key presses go to it. `h.main_window()` moves back to the main window. `h.windows()` lists the keys of the open windows, and `h.close_window(key)` acts as if its close button was clicked.

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
