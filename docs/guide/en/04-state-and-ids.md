<!-- source: ja/04-state-and-ids.md fnv=ba75450c4560c172 -->
<!-- covers: Ui::keyed List::show_keyed Ui::request_frame App::presented Handle::id sig=eb58365da527b924 -->
# State and ids

## The app owns its state

kargui does not hold your app's state. The text of a field, whether a box is checked, the selected row: all of it lives in your own struct.
Pass it to a widget in `view`; the widget reads it and writes to it when the user changes it.

kargui only keeps small state for the screen itself, such as the caret position, scroll positions and focus.

## Widget ids

Widget state is remembered per widget id. The id comes from where the widget was created in the source (file, line, column) and how many times that spot was reached in this frame.
So you usually do not need to think about ids.

When a loop creates the same widgets and items are added or removed, tell them apart by the item's id.
Otherwise, adding an item at the top moves focus and the caret to a different item.

```rust
# struct Task { id: u32, text: String }
# let tasks = vec![Task { id: 7, text: "Milk".into() }, Task { id: 3, text: "Bread".into() }];
# kargui::test::example(|ui| {
for task in &tasks {
    ui.keyed(task.id, |ui| {
        ui.label(&task.text);
    });
}
# });
```

For lists (`ui.list`), use `show_keyed` (see [Lists and tables](05-lists-and-tables.md)).

## Asking for another frame

Without input, `view` is not called. To show something that changes over time (a clock, an animation), ask for the next frame with `ui.request_frame()`.

```rust
# kargui::test::example(|ui| {
let animating = true;
if animating {
    ui.request_frame();
}
# });
```

## After the frame is shown

`App::presented` is called after a frame is on the screen. The first call happens when the app has started and its window is shown, so it is a good place to measure startup time.
