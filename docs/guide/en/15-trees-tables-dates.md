<!-- source: ja/15-trees-tables-dates.md fnv=5add5f8d1405a104 -->
<!-- covers: Ui::tree TreeBuilder::leaf TreeBuilder::branch TreeView::changed Ui::table Table::sort Table::selection Table::label Table::grow Table::show Table::show_keyed TableRow::cell TableRow::text TableRow::context_menu TableColumn::new TableColumn::width TableColumn::fill TableColumn::right Ui::date_picker DatePicker::changed Ui::set_date_text Ui::today Date::new Date::today Date::year Date::month Date::day Date::weekday Date::add_days Date::add_months Date::days_in_month Date::from_unix_days Date::unix_days DateText::english DateText::japanese sig=1e24c244697ffaa8 -->
# Trees, tables and dates

## Trees

Items that open and close, like folders or the levels of an outline.

```rust
# let mut selected = None;
# kargui::test::example(|ui| {
ui.tree(&mut selected, |t| {
    t.branch("docs", "Documents", |t| {
        t.leaf("report", "report.pdf");
        t.branch("photos", "Photos", |t| {
            t.leaf("cat", "cat.jpg");
        });
    });
    t.leaf("notes", "notes.txt");
})
.label("Files");
# });
```

`t.branch(key, text, |t| ..)` is an item with children, and `t.leaf(key, text)` an item without. Items are told apart by their keys, and `selected` holds the selected item's key. Branches start closed, and kargui remembers which are open. Children of closed branches are not created, so large trees stay light.

Clicking an item selects it, and clicking the arrow at its left opens or closes it. With the keyboard, Tab stops once, on the selected item. Up, Down, Home, End and letters move and select. Right opens a closed branch or moves to the first child of an open one. Left closes an open branch or moves to the parent. Enter and Space open and close.

Screen readers hear a tree, and items with their level, whether they are open, and whether they are selected.

## Tables

Rows under column headings. As in a list, only the visible rows are created.

```rust
use kargui::ui::TableColumn;
# struct File { name: String, size: u64 }
# let mut files = vec![File { name: "b.txt".into(), size: 20 }, File { name: "a.png".into(), size: 300 }];
# let (mut sort, mut selected) = (None, None);
# kargui::test::example(|ui| {
// sorting is up to the app
match sort {
    Some((0, asc)) => files.sort_by(|a, b| if asc { a.name.cmp(&b.name) } else { b.name.cmp(&a.name) }),
    Some((1, asc)) => files.sort_by(|a, b| if asc { a.size.cmp(&b.size) } else { b.size.cmp(&a.size) }),
    _ => {}
}
let columns = [TableColumn::new("Name").width(220.0), TableColumn::new("Size").width(90.0).right(), TableColumn::new("Notes").fill()];
ui.table(&columns, files.len(), 30.0).sort(&mut sort).selection(&mut selected).label("Files").grow().show(|row, i| {
    row.text(&files[i].name);
    row.text(format!("{} KB", files[i].size));
    row.cell(|ui| {
        ui.link("Open");
    });
});
# });
```

- `TableColumn::new(heading)` makes a column. `.width(width)` sets its starting width, `.fill()` gives it the remaining width, and `.right()` aligns it to the right (for numbers)
- With `.sort(&mut order)`, each click on a heading switches between `Some((column, true))` (ascending) and `Some((column, false))`. Sorting the data is up to the app
- `.selection(&mut selected_row)` makes rows selectable (as `selection` on a list)
- In a row, add the cells in column order with `row.text(text)` or `row.cell(|ui| ..)`
- `row.context_menu(|m| ..)` adds a menu that opens when the row is right-clicked

Drag the small grip at the right of a heading to change the column's width (kargui remembers it). Tab stops once on the headings; Left and Right move between them, and Enter or Space sorts. Without `.grow()`, the table is 8 rows tall and scrolls.

Screen readers hear a table, a heading row (with the sort order), rows and cells, with their row and column numbers.

## Dates

A date field that opens a month calendar below it when clicked.

```rust
# let mut due: Option<kargui::Date> = None;
# kargui::test::example(|ui| {
ui.set_date_text(kargui::DateText::japanese());
if ui.date_picker(&mut due).label("Due date").changed() {
    // save the picked date
}
# });
```

The date is a `kargui::Date`, or `None` while none has been picked. Clicking a day, or Enter or Space in the calendar, picks it and closes. Escape or a click outside closes without picking.

In the calendar, the arrow keys move by a day or a week, Home and End to the start and end of the week, and Page Up and Page Down by a month (by a year with Shift). With the focus on the field, Enter, Space, Alt+Down and F4 open the calendar.

The field shows the date as `2026-11-22`. The names of days and months come from `ui.set_date_text(..)` (English by default, Japanese with `DateText::japanese()`). Screen readers hear a date field with its value, and a calendar grid whose days have full names (such as November 22, 2026).

`Date` holds a year, a month and a day. Make one with `Date::new(year, month, day)` (`None` if there is no such day), and calculate with `add_days`, `add_months` and `weekday`. From a file's modification time and the like, `Date::from_unix_days(seconds / 86400)` gives the day in UTC (and `unix_days` goes back). `ui.today()` is today's date (always 2026-11-22 in tests).
