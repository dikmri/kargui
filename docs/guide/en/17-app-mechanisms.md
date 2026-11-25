<!-- source: ja/17-app-mechanisms.md fnv=6a596ff636749b84 -->
<!-- covers: Ui::file_dialog Ui::chosen_files Ui::chosen_file Ui::file_dialog_cancelled FileDialog::open FileDialog::save FileDialog::multiple FileDialog::title FileDialog::filter FileDialog::file_name FileDialog::folder FileDialog::is_save FileDialog::allows_multiple App::close_requested Ui::quit Harness::file_dialog_key Harness::file_dialog Harness::choose_files Harness::cancel_file_dialog Harness::request_close Harness::is_closed sig=7e9d34a5ac6cda9a -->
# App mechanisms

## Dialogs to open and save files

Shows the system's file dialog.

```rust
use kargui::FileDialog;
# let mut text = String::new();
# kargui::test::example(|ui| {
if ui.button("Open").clicked() {
    ui.file_dialog("open", FileDialog::open().title("Open a note").filter("Text", &["txt", "md"]));
}
if let Some(path) = ui.chosen_file("open") {
    text = std::fs::read_to_string(path).unwrap_or_default();
}
if ui.button("Save as").clicked() {
    ui.file_dialog("save", FileDialog::save().file_name("note.txt").filter("Text", &["txt"]));
}
if let Some(path) = ui.chosen_file("save") {
    let _ = std::fs::write(path, &text);
}
# });
```

`ui.file_dialog(key, ..)` shows the dialog after the frame is drawn. The chosen file arrives once, in a later frame, from `ui.chosen_file(key)`. When the user cancels, `ui.file_dialog_cancelled(key)` is true once, in the next frame. The app keeps running and drawing while the dialog is open.

- `FileDialog::open()` opens, `FileDialog::save()` saves (and asks before replacing a file with the same name)
- `.filter(name, &[extensions])` sets the kinds of files, `.title(..)` the title, `.file_name(..)` the starting name and `.folder(..)` the starting folder
- `.multiple()` lets the user choose several files; take all of them with `ui.chosen_files(key)`

On Linux, the dialog of xdg-desktop-portal opens (the desktop's own file chooser). On macOS, nothing happens yet.

## Asking before closing

When there are unsaved changes, the app can ask before the window closes.

```rust
use kargui::{App, Ui};

#[derive(Default)]
struct Notes {
    unsaved: bool,
    asking: bool,
}

impl App for Notes {
    fn view(&mut self, ui: &mut Ui) {
        let mut discard = false;
        ui.dialog(&mut self.asking, "Discard your changes?", |ui| {
            discard = ui.button("Discard and close").clicked();
        });
        if discard {
            ui.quit();
        }
    }

    fn close_requested(&mut self) -> bool {
        self.asking = self.unsaved;
        !self.unsaved
    }
}
# let mut h = kargui::test::Harness::new(Notes { unsaved: true, asking: false });
# h.request_close();
# assert!(!h.is_closed());
```

When the user clicks the window's close button (or presses Alt+F4), kargui calls the app's `close_requested`. Returning true closes the window; returning false keeps it open. To close after asking, call `ui.quit()`, which ends the app from anywhere.

## Window icon and minimum size

```rust
use kargui::{Image, Options};
# let png = Image::from_rgba(2, 2, vec![255; 16]).unwrap();
let mut options = Options::new("Notes");
options.icon = Some(png);
options.min_size = Some((360.0, 240.0));
# let _ = options;
```

`icon` is the icon in the title bar and the taskbar. Other windows use the main window's icon unless they have their own. `min_size` is the smallest size of the inside of the window, in logical pixels, that the user can make it.

## Widgets in the title bar

With `options.custom_title_bar = true`, kargui draws the title bar on Windows too, and `ui.title_bar` in the view puts widgets in it, such as a menu, a search field or tabs.

```rust
use kargui::{App, Options, Ui};

struct Notes {
    search: String,
}

impl App for Notes {
    fn view(&mut self, ui: &mut Ui) {
        ui.title_bar(|ui| {
            ui.label("Notes").bold();
            ui.text_input(&mut self.search).hint("Search");
        });
        ui.label("Body");
    }
}

let mut options = Options::new("Notes");
options.custom_title_bar = true;
# let _ = (options, Notes { search: String::new() });
```

- kargui puts the minimize, maximize and close buttons at the bar's right end. On Windows 11, resting the pointer on the maximize button shows the snap layouts.
- The empty part of the bar (including text that cannot be pressed) works like the system's bar: dragging it moves the window, a double click maximizes it, a right click opens the window menu, and the window can be snapped to the screen's edges.
- When `ui.title_bar` adds nothing, the window's title is shown at the bar's left. When it adds widgets, the title is not shown; put it in yourself with `ui.label` if you want it.
- On GNOME (Wayland where the compositor draws no title bars), kargui always draws the bar and puts the `ui.title_bar` widgets in it, whatever `custom_title_bar` says.
- Where the system or compositor draws the bar (X11, Wayland compositors that draw bars, compositors with no bars) and on Windows with `custom_title_bar` false, the `ui.title_bar` widgets make the top row of the window. Call `ui.title_bar` first in the view so this works everywhere.

## Tray (notification area icon)

While the main window's view calls `ui.tray`, the app has an icon in the notification area. The icon has a tooltip, a click, and a menu that opens with a right click. The system draws the menu, so it takes items, check items, separators and submenus.

```rust
use kargui::{App, Ui};

#[derive(Default)]
struct Notes {
    muted: bool,
    hide: bool,
}

impl App for Notes {
    fn view(&mut self, ui: &mut Ui) {
        let (mut show, mut quit) = (false, false);
        ui.tray(|t| {
            t.tooltip("Notes");
            show = t.clicked();
            t.menu(|m| {
                show |= m.item("Show window").clicked();
                m.check(&mut self.muted, "Mute");
                m.separator();
                quit = m.item("Quit").clicked();
            });
        });
        if std::mem::take(&mut self.hide) {
            ui.hide_window();
        }
        if show {
            ui.show_window();
        }
        if quit {
            ui.quit();
        }
        ui.label("Body");
    }

    // the close button hides the window to the tray instead of ending the app
    fn close_requested(&mut self) -> bool {
        self.hide = true;
        false
    }
}
# let _ = Notes::default();
```

- `ui` is in use inside `ui.tray`, so note what was clicked or chosen, and show the window or end the app after it.
- `t.clicked()` and `m.item(..).clicked()` are true only in the frame of the click or choice. `m.check` flips its value when chosen.
- Give the icon's picture with `t.icon(&image)`. Without one, the tray uses the window's icon (`Options::icon`), or a plain kargui icon. Keep the image in the app's state: the same image is not sent to the system again.
- When the view stops calling `ui.tray`, the icon goes away. Only the main window's view makes the tray (`App::window` views are ignored).
- `ui.hide_window()` takes the window off the screen and the taskbar, and `ui.show_window()` brings a hidden or minimized window back to the front. Call them when something happens (a click on the tray, a close), not every frame: calling `ui.show_window()` every frame would undo the user's minimizing.
- On Windows the icon is in the notification area of the taskbar. On Linux it uses the StatusNotifierItem protocol, which KDE, the bars of tiling compositors (Waybar, Quickshell and others) and GNOME with the AppIndicator extension show. Plain GNOME shows nothing.
- Wayland has no request to bring back a minimized window. There, `ui.show_window()` brings back only a window hidden with `ui.hide_window()`.

## Notifications

`ui.notify` shows a notification: on Windows a toast (it also stays in the notification center), on Linux through the desktop's notification service (org.freedesktop.Notifications). A notification has a title, a body and up to three buttons. Read what the user clicked with `ui.notification_clicked` and the same key.

```rust
use kargui::{App, Notification, NotificationAction, Ui};

#[derive(Default)]
struct Timer {
    snoozed: u32,
}

impl App for Timer {
    fn view(&mut self, ui: &mut Ui) {
        if ui.button("Remind me").clicked() {
            ui.notify("time", Notification::new("Time is up").body("The meeting starts").button("snooze", "In 5 minutes"));
        }
        match ui.notification_clicked("time") {
            Some(NotificationAction::Button(id)) if id == "snooze" => self.snoozed += 1,
            Some(_) => ui.show_window(),
            None => {}
        }
    }
}
# let _ = Timer::default();
```

- `ui.notify` shows the notification once, when called. Call it when something happens, such as a click. Showing again with the same key replaces the notification shown before.
- `ui.notification_clicked` returns the answer once when the user clicked the notification itself (`NotificationAction::Body`) or a button (`NotificationAction::Button(id)`). Closing it without a click returns nothing.
- `Notification::silent()` shows it without the sound.
- On Windows, the first notification registers the app's name (`Options::title`) for the user in the registry (`HKEY_CURRENT_USER\Software\Classes\AppUserModelId\<program file name>`); the toast shows this name. Clicks reach the app while it runs; clicking an old notification in the notification center after the app ended does nothing.
- On Linux, notifications show on desktops with org.freedesktop.Notifications, such as GNOME, KDE, mako and dunst. Whether buttons show depends on the notification service.

## Tests

In the test harness, read the requested dialog with `h.file_dialog_key()` and `h.file_dialog()`, and answer it with `h.choose_files(&[..])` or `h.cancel_file_dialog()`. `h.request_close()` acts as if the close button was clicked, and `h.is_closed()` tells whether the app closed. Read the tray with `h.tray_shown()`, `h.tray_tooltip()` and `h.tray_menu()` (one line per item), click it with `h.tray_click()`, and choose an item with `h.tray_choose("Quit")`. `h.window_shown()` tells whether the window is hidden. Read the notifications with `h.notifications()` (one line each), and click one with `h.click_notification("time", Some("snooze"))` for a button or `None` for its body.
