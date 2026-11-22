<!-- source: ja/18-drag-and-drop.md fnv=4cb3ce3b0ff68e80 -->
<!-- covers: Handle::drag_source Handle::dropped Harness::drag_to Harness::drag_over Harness::release Harness::drop_files sig=e3964f54e66f4cdf -->
# Drag and drop

## Dragging something across

Add `.drag_source(payload)` to a widget and the pointer can drag it. The payload is a value that
says what is being carried (which card in which column, a file path, and so on). Add
`.dropped::<Type>()` to the widget that takes it: it returns `Some(payload)` when a payload of
that type is dropped on it.

```rust
#[derive(Clone, Copy)]
struct Card(usize);

# let mut todo = vec!["Write the guide".to_string(), "Take the pictures".to_string()];
# let mut done: Vec<String> = Vec::new();
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.column(|ui| {
        ui.heading("To do");
        for (k, name) in todo.iter().enumerate() {
            ui.label(name).drag_source(Card(k));
        }
    });
    let mut done_column = ui.column(|ui| {
        ui.heading("Done");
        for name in &done {
            ui.label(name);
        }
    });
    if let Some(Card(k)) = done_column.dropped::<Card>() {
        done.push(todo.remove(k));
    }
});
# });
```

- A drag starts when the pointer moves more than 4 pixels after pressing. A shorter move is still a
  click (a button inside a draggable row can still be pressed).
- While dragging, a box with the widget's name follows the pointer. A widget that takes the payload
  is outlined in the accent color while the pointer is over it; elsewhere the pointer shows the
  "not allowed" shape.
- Escape cancels. Releasing outside any widget that takes the payload does nothing.
- The payload's type decides what is taken: a widget does not take payloads of other types.
- A drag does not start on widgets that use dragging themselves (text fields, sliders, dividers,
  drawing surfaces), nor on disabled widgets.
- The payload is taken once, when the drag starts. It does not need to be `Clone`.

## Reordering (nested drop targets)

Where widgets that take payloads are nested, the innermost one takes it. That makes reordering
easy to write: dropped on a row, the item goes before that row; dropped where there is no row, it
goes last.

```rust
#[derive(Clone, Copy)]
struct Item(usize);

# let mut items = vec!["One".to_string(), "Two".to_string(), "Three".to_string()];
# kargui::test::example(|ui| {
let mut moved = None;
let mut list = ui.column(|ui| {
    for (k, name) in items.iter().enumerate() {
        let mut row = ui
            .card(|ui| {
                ui.label(name);
            })
            .drag_source(Item(k));
        // Dropped on a row: before that row
        if let Some(Item(from)) = row.dropped::<Item>() {
            moved = Some((from, k));
        }
    }
});
// Dropped where there is no row: last
if let Some(Item(from)) = list.dropped::<Item>() {
    moved = Some((from, items.len()));
}
if let Some((from, before)) = moved {
    let item = items.remove(from);
    let at = if from < before { before - 1 } else { before };
    items.insert(at, item);
}
# });
```

## Dropping files from other apps

When files are dragged from Explorer (or another app) and dropped on the window, a widget that
takes `Vec<PathBuf>` receives their paths. It is written the same way as drag and drop inside the
window.

```rust
use std::path::PathBuf;

# let mut opened: Vec<PathBuf> = Vec::new();
# kargui::test::example(|ui| {
let mut area = ui.column(|ui| {
    ui.heading("Drop files here");
    for path in &opened {
        ui.label(path.display().to_string());
    }
});
if let Some(paths) = area.dropped::<Vec<PathBuf>>() {
    opened.extend(paths);
}
# });
```

- Over a widget that takes the files, the pointer shows the "copy" shape and the widget is outlined
  in the accent color; elsewhere the pointer shows the "not allowed" shape.
- The picture of what is being dragged (the file icons) is drawn by the system.
- A drag inside the window that carries `Vec<PathBuf>` (`.drag_source(vec![path])`) lands on the
  same widget.
- kargui registers the window with the system (OLE `RegisterDragDrop` on Windows) when a widget that
  takes `Vec<PathBuf>` first appears. Apps that never take files do not register, so their startup
  does not slow down.

## For people who cannot drag

Dragging is hard for people who cannot move their hands as they want, and impossible with a
keyboard or a screen reader. WCAG 2.2 success criterion 2.5.7 also asks for a way that does not
need dragging. Always offer the same action through a menu or buttons as well. The example
`examples/board` gives every card a "Move" menu (to another column, up, down).

## Testing

`Harness::drag_to(name, target)` drags and drops: it presses the widget, moves the pointer in a
few steps (one frame each) and releases it over the middle of the target. To check what is shown
while dragging (the highlighted target, for example), `Harness::drag_over(name, target)` stops
there with the button held, and `Harness::release()` releases it. Escape cancels in between.
Dropping files from another app is tested with `Harness::drop_files(target, &[paths])`.

```rust
use kargui::{App, Ui};
use kargui::test::Harness;

struct Shelf {
    books: Vec<&'static str>,
    read: Vec<&'static str>,
}

impl App for Shelf {
    fn view(&mut self, ui: &mut Ui) {
        for (k, book) in self.books.iter().enumerate() {
            ui.label(*book).drag_source(k);
        }
        let mut read = ui.column(|ui| {
            ui.heading("Read");
        });
        if let Some(k) = read.dropped::<usize>() {
            let book = self.books.remove(k);
            self.read.push(book);
        }
    }
}

let mut h = Harness::new(Shelf { books: vec!["Dune", "Emma"], read: vec![] });
h.drag_to("Emma", "Read");
assert_eq!(h.app().read, ["Emma"]);
```
