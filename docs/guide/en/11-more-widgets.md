<!-- source: ja/11-more-widgets.md fnv=c57d8b11d5c8b064 -->
<!-- covers: Ui::radio_group Ui::separator Ui::progress Ui::progress_busy Ui::spinner Ui::card Ui::group Ui::expander Ui::link Ui::toggle_button Ui::password Toggled::mixed Ui::dropdown Dropdown::changed sig=13e8d8bd7ca74494 -->
# More widgets

## Radio buttons

Pick one of several options. They are placed one below another; for a compact row, use `ui.segmented(..)`.

```rust
#[derive(Clone, Copy, PartialEq)]
enum Size { Small, Medium, Large }
# let mut size = Size::Medium;
# kargui::test::example(|ui| {
if ui.radio_group(&mut size, &[(Size::Small, "Small"), (Size::Medium, "Medium"), (Size::Large, "Large")]) {
    // the selection changed
}
# });
```

Tab stops once in the group (at the selected button), and the arrow keys move through the group, selecting as they go. At an end, they wrap around to the other end.

## Drop-downs

Pick one of several options, showing only the selected one. Use it when there are many options or little room.

```rust
#[derive(Clone, Copy, PartialEq)]
enum Theme { System, Light, Dark }
# let mut theme = Theme::System;
# kargui::test::example(|ui| {
let options = [(Theme::System, "Use the system setting"), (Theme::Light, "Light"), (Theme::Dark, "Dark")];
if ui.dropdown(&mut theme, &options).label("Theme").changed() {
    // switch the theme
}
# });
```

Clicking it opens the list below (above when there is no room). With the focus on the closed field, Up and Down pick the previous or next option. Enter, Space, Alt+Down and F4 open the list. In the open list, Up, Down, Home and End move, and Enter or Space picks. Escape or a click outside closes it without picking. Screen readers hear the name given with `.label(..)` and the selected option.

## Separators, cards and groups

```rust
# let (mut sound, mut vibrate) = (true, false);
# kargui::test::example(|ui| {
ui.card(|ui| {
    ui.heading("Storage");
    ui.label("12 GB of 64 GB used").weak();
});
ui.separator();
ui.group("Notifications", |ui| {
    ui.checkbox(&mut sound, "Play a sound");
    ui.checkbox(&mut vibrate, "Vibrate");
});
# });
```

`ui.separator()` is a horizontal line in a column and a vertical line in a row. `ui.group(title, ..)` is a card with a title; screen readers hear a group named by the title.

## Progress

```rust
# let (done, total) = (3, 10);
# kargui::test::example(|ui| {
ui.progress(done as f32 / total as f32).label("Sending photos");
ui.progress_busy().label("Connecting");
ui.spinner().label("Loading");
# });
```

`ui.progress(fraction)` is a bar from 0.0 to 1.0. When the end is unknown, use a moving stripe with `ui.progress_busy()` or a spinning ring with `ui.spinner()`. The two moving ones draw about 30 times a second while they are on screen. Name them with `.label(..)` so screen readers can say what is progressing.

## Sections that open and close

```rust
# let (mut more, mut proxy) = (false, false);
# kargui::test::example(|ui| {
ui.expander(&mut more, "Advanced", |ui| {
    ui.checkbox(&mut proxy, "Use a proxy");
});
# });
```

Clicking the header (or Enter / Space while it has the focus) opens and closes it. Screen readers also hear whether it is open.

## Links, toggle buttons and partly checked boxes

```rust
# let (mut bold, mut help) = (false, false);
# let items = [true, false, true];
# kargui::test::example(|ui| {
if ui.link("How do backups work?").clicked() {
    help = true;
}
ui.toggle_button(&mut bold, "Bold");
let mut all = items.iter().all(|&c| c);
let some = items.iter().any(|&c| c);
ui.checkbox(&mut all, "Select all").mixed(some && !all);
# });
```

A check box with `.mixed(true)` draws a dash, and screen readers hear "mixed". What a click does is up to the app.

## Passwords

```rust
# let mut password = String::new();
# kargui::test::example(|ui| {
ui.password(&mut password).hint("Password").grow();
# });
```

It shows dots instead of the text, does not copy or cut, and does not give the text to screen readers. Pasting works.
