<!-- source: ja/02-widgets.md fnv=ad000fa0b2b2d8eb -->
<!-- covers: Ui::label Ui::heading Ui::button Ui::checkbox Ui::toggle Ui::dark_mode_switch Ui::segmented Ui::text_input Ui::text_area TextArea::hint TextArea::rows Ui::slider Label::weak Label::small Label::bold Label::size Button::primary Button::small Button::enabled Ui::enabled Ui::popup Ui::dialog Handle::tooltip Ui::icon Ui::icon_button Button::icon Ui::image Image::from_rgba Image::open Toggled::changed Toggled::strike Slider::changed sig=a0f2ab3741c86140 -->
# Widgets

Widgets are created with methods on `ui`. Widgets that change a value take the state with `&mut`; ask the return value what happened.

## Text and buttons

```rust
# kargui::test::example(|ui| {
ui.heading("A heading");
ui.label("Normal text");
ui.label("Less prominent text").weak();
ui.label("Small text").small();
ui.label("Bold text").bold();
ui.label("Text with a set size").size(18.0);
if ui.button("Save").primary().clicked() {
    // save
}
ui.button("Small button").small();
# });
```

Use `primary()` for the single most important button on the screen.

## On and off

```rust
# let (mut done, mut wifi) = (false, true);
# kargui::test::example(|ui| {
if ui.checkbox(&mut done, "Buy milk").strike(done).changed() {
    // done was flipped
}
ui.toggle(&mut wifi, "Wi-Fi");
# });
```

`strike(true)` draws a line through the text, as for a finished task. It stays clickable.

For switching between light and dark, use `ui.dark_mode_switch("Dark")`. It shows and changes kargui's own setting (which starts as the operating system's), so the app does not need to keep a value, and the switch never disagrees with the look.

```rust
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.heading("Tasks");
    ui.fill();
    ui.dark_mode_switch("Dark");
});
# });
```

## Picking one of a few

```rust
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Light,
    Dark,
}
# let mut look = Look::Light;
# kargui::test::example(|ui| {
if ui.segmented(&mut look, &[(Look::Light, "Light"), (Look::Dark, "Dark")]) {
    // the choice changed
}
# });
```

## Typing text

```rust
# let mut name = String::new();
# kargui::test::example(|ui| {
let field = ui.text_input(&mut name).hint("Name").grow();
if field.submitted() {
    // Enter was pressed (the Enter that commits an input-method composition does not count)
}
# });
```

For several lines, use `ui.text_area(..)`. Enter starts a new line, long lines wrap, and what does not fit scrolls. The arrows, Home/End (start and end of the line), Ctrl+Home/End (start and end of the text) and Page Up/Down move the caret. Since Enter makes a new line, use a shortcut such as Ctrl+Enter for sending.

```rust
use kargui::{Key, Mods};
# let (mut message, mut sent) = (String::new(), 0);
# kargui::test::example(|ui| {
ui.text_area(&mut message).hint("Message").rows(5).grow();
if ui.shortcut(Key::Enter, Mods::CTRL) && !message.is_empty() {
    message.clear();
    sent += 1;
}
# });
```

The input-method rules are in [Japanese input](07-japanese-input.md). They are the same for both fields.

## Picking a number

```rust
# let (mut font_size, mut volume) = (14, 0.5_f32);
# kargui::test::example(|ui| {
ui.slider(&mut font_size, 10..=24).label("Font size");
if ui.slider(&mut volume, 0.0..=1.0).label("Volume").changed() {
    // change the volume
}
# });
```

Integers move in steps of 1; decimals move freely with the pointer and in hundredths of the range with the keyboard. A slider has no text of its own, so give it a screen-reader name with `.label(..)`.

## Disabling widgets

Create widgets that cannot be used until a condition holds inside `ui.enabled(condition, |ui| ..)`. They are drawn dimmed, ignore the pointer and the keyboard, and are skipped by Tab. Screen readers announce them as unavailable. For a single button, add `.enabled(condition)`.

```rust
# let (mut notify, mut sound, mut volume) = (false, true, 5);
# let name = String::new();
# kargui::test::example(|ui| {
ui.checkbox(&mut notify, "Notify me");
ui.enabled(notify, |ui| {
    ui.checkbox(&mut sound, "Play a sound");
    ui.slider(&mut volume, 0..=10).label("Volume");
});
if ui.button("Send").enabled(!name.is_empty()).clicked() {
    // send
}
# });
```

A widget that has the focus loses it when it becomes disabled. `ui.enabled` does not create a row or column, so the widgets inside are placed in the current row (or column) as they are.

## Layers

Create a popup next to a button with `ui.popup(&mut is_open, anchor_id, |ui| ..)` and a confirmation in the middle of the window with `ui.dialog(&mut is_open, title, |ui| ..)`. Both move the focus to their first widget when they open, close with Escape, and give the focus back to where it was when they close. A popup also closes when you click outside it (that click does not reach the widget underneath). While a dialog is open, the widgets behind it cannot be used and Tab moves only inside the dialog.

```rust
# let (mut menu, mut confirm) = (false, false);
# kargui::test::example(|ui| {
let more = ui.button("More");
let anchor = more.id();
if more.clicked() {
    menu = !menu;
}
ui.popup(&mut menu, anchor, |ui| {
    if ui.button("Delete").clicked() {
        confirm = true;
    }
});
let mut close = false;
ui.dialog(&mut confirm, "Delete this?", |ui| {
    ui.label("This cannot be undone.");
    ui.row(|ui| {
        ui.fill();
        close |= ui.button("Delete").primary().clicked();
        close |= ui.button("Keep").clicked();
    });
});
if close {
    confirm = false;
}
# });
```

A popup opens below its anchor, or above it when there is no room below. Layers are drawn inside the window, so they never extend past its edges.

Give a widget a description with `.tooltip("text")`. It appears below the widget when the pointer rests on it for half a second or the keyboard moves the focus to it, and screen readers read it as the widget's description.

```rust
# kargui::test::example(|ui| {
ui.button("Save").tooltip("Saves the current content to the file");
# });
```

## Icons and images

kargui has 18 common icons (`Icon::Check`, `Icon::Close`, `Icon::ChevronDown`, `Icon::Search` and so on). They are drawn sharply at any size, in the text color. Put one before a button's text with `.icon(..)`; make an icon-only button with `ui.icon_button(icon, name)`. An icon-only button has no text, so always pass a name (what screen readers say).

```rust
use kargui::Icon;
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.button("New task").icon(Icon::Plus).primary();
    ui.icon_button(Icon::Close, "Close");
    ui.icon(Icon::Info).weak();
    ui.label("Saved automatically");
});
# });
```

Load a picture into an `Image`, then show it with `ui.image(&image)`. Loading takes time, so do it once when you create the app's state, not in `view`. Its size is its pixel count (in logical pixels); change it with `.size(width, height)`.

```rust
use kargui::{App, Image, Ui};

struct Album {
    photo: Image,
}

impl App for Album {
    fn view(&mut self, ui: &mut Ui) {
        ui.image(&self.photo).size(320.0, 240.0).label("The office at night");
    }
}

// from a file: Image::open("photo.jpg") (on Windows). Here, from pixels
let photo = Image::from_rgba(4, 3, vec![120; 48]).unwrap();
let h = kargui::test::Harness::new(Album { photo });
assert!(h.find("The office at night").is_some());
```

Icons and images without a name are treated as decoration, and screen readers skip them. Describe pictures that carry information with `.label(..)`. Image files can be read only on Windows for now (PNG, JPEG, GIF, BMP and more); on other systems, create images from pixels with `Image::from_rgba`. The Direct3D 11 renderer (not the default) does not draw images yet.
