# Task: build "Task Book" with {FRAMEWORK}

Build a desktop app with the features below. Write only the user interface. The data model is the
`taskcore` crate, which is already a dependency: read `{TASKCORE}\src\lib.rs` for its API
(`Store`, `Task`, `Status`, `Counts`, `group`, `ROW_HEIGHT`, `WINDOW_SIZE`).

1. Window title "Task Book", size `taskcore::WINDOW_SIZE` (logical pixels).
2. Top row: the heading "Task Book", and on the right a switch "Dark" that toggles a dark look.
3. Second row: a text field with the placeholder "New task" that takes the remaining width, and an
   "Add" button. Clicking "Add" or pressing Enter in the field adds the text as a task
   (`Store::add`) and clears the field. The Enter key that commits a Japanese input-method (IME)
   composition must not add a task; only a later Enter does.
4. Third row: a text field with the placeholder "Filter" (about 240 pixels wide), three choices
   "All", "Active", "Done" (`taskcore::Status`), and a label "Showing X of Y" (numbers with
   `taskcore::group`). The list shows `Store::filter(query, status)`. While the user is composing
   with an IME in the Filter field, filtering must not run until the text is committed.
5. The list: one row per shown task (row height `ROW_HEIGHT`), with a check box labeled with the
   task text (clicking it toggles done; done tasks look dimmed or struck through) and a "Delete"
   button at the right end of the row. It must scroll, and stay fast with 10,000 tasks or more
   (only build the visible rows).
6. Bottom row: buttons "Add 10,000" (`Store::generate(10_000)`), "Clear done"
   (`Store::clear_done`) and "Clear all" (`Store::clear`), and a label "Active N · Done M"
   (from `Store::counts`).
