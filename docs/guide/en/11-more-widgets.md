<!-- source: ja/11-more-widgets.md fnv=8be94daca53ab0b3 -->
<!-- covers: Ui::radio_group Ui::separator Ui::progress Ui::progress_busy Ui::spinner Ui::card Ui::group Ui::expander Ui::link Ui::toggle_button Ui::password Toggled::mixed sig=a19e38880539381c -->
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
