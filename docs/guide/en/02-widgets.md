<!-- source: ja/02-widgets.md fnv=6b1b1f4b2901a2d1 -->
<!-- covers: Ui::label Ui::heading Ui::button Ui::checkbox Ui::toggle Ui::segmented Ui::text_input Ui::slider Label::weak Label::small Label::bold Label::size Button::primary Button::small Toggled::changed Toggled::strike Slider::changed sig=347ae262fe4371ee -->
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
