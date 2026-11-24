<!-- source: ja/17-app-mechanisms.md fnv=2af021f6df3c20bb -->
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

## Tests

In the test harness, read the requested dialog with `h.file_dialog_key()` and `h.file_dialog()`, and answer it with `h.choose_files(&[..])` or `h.cancel_file_dialog()`. `h.request_close()` acts as if the close button was clicked, and `h.is_closed()` tells whether the app closed.
