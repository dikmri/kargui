<!-- source: ja/05-lists-and-tables.md fnv=6a1077f6d9705062 -->
<!-- covers: Ui::list List::show List::show_keyed List::selection List::grow List::separators List::scroll_to List::scroll_offset Label::right sig=a3205d9c04291788 -->
# Lists and tables

`ui.list(count, row_height)` is a list that only creates the visible rows. With ten thousand or a million items, one frame creates only the twenty or so rows on screen.
All rows have the same height.

```rust
# let items: Vec<String> = (0..10_000).map(|i| format!("Item {i}")).collect();
# kargui::test::example(|ui| {
ui.list(items.len(), 32.0).grow().show(|ui, i| {
    ui.label(&items[i]);
});
# });
```

Users scroll with the wheel, the scroll bar (drag the thumb, or click the empty part to move one page), or Tab.

## Telling rows apart by item id

`show` tells rows apart by their position from the top. If items are added or removed, pass the item's id with `show_keyed`.
Focus and check state then stay with the item, and screen readers do not see every row change.

```rust
# struct Task { id: u32, text: String, done: bool }
# let mut tasks = vec![Task { id: 7, text: "Milk".into(), done: false }];
# kargui::test::example(|ui| {
let ids: Vec<u32> = tasks.iter().map(|t| t.id).collect();
ui.list(tasks.len(), 32.0).grow().show_keyed(|i| ids[i], |ui, i| {
    let t = &mut tasks[i];
    ui.checkbox(&mut t.done, t.text.clone());
});
# });
```

## Selecting a row

With `selection`, users select a row by clicking it. The up and down arrows move the selection, Home and End jump to the ends, and the list scrolls to show the selected row.
The whole list is one stop for the Tab key.

```rust
# let files = vec!["a.txt", "b.txt", "c.txt"];
# let mut selected: Option<usize> = None;
# kargui::test::example(|ui| {
ui.list(files.len(), 28.0).grow().selection(&mut selected).show(|ui, i| {
    ui.label(files[i]);
});
# });
```

In apps where sorting or filtering changes the order of rows, remember the selected item by its id and look up its row each time (see `examples/table`).

## Making a table

Put labels with set widths in each row to make a table. Align numbers to the right with `right()`.
List rows have 12 pixels of padding on the left and right, so a header row above the list lines up when it also has `padding(12.0, 0.0)`.

```rust
# use kargui::Size;
# struct Item { name: String, price: u32 }
# let items = vec![Item { name: "Notebook".into(), price: 280 }];
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.label("Name").bold().grow();
    ui.label("Price").bold().right().width(Size::Fixed(80.0));
})
.padding(12.0, 0.0);
ui.list(items.len(), 30.0).grow().show(|ui, i| {
    ui.label(&items[i].name).grow();
    ui.label(format!("{} yen", items[i].price)).right().width(Size::Fixed(80.0));
});
# });
```
