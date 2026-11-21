<!-- source: ja/02-widgets.md fnv=77999a55f8f9e50c -->
<!-- covers: Ui::label Ui::heading Ui::button Ui::checkbox Ui::toggle Ui::dark_mode_switch Ui::segmented Ui::text_input Ui::slider Label::weak Label::small Label::bold Label::size Button::primary Button::small Button::enabled Ui::enabled Ui::popup Ui::dialog Handle::tooltip Toggled::changed Toggled::strike Slider::changed sig=0af435d8eecb0df6 -->
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

The input-method rules are in [Japanese input](07-japanese-input.md).

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
