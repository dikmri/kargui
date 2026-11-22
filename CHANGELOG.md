# Changelog

## 0.2.0 (2026-11-22)

Everyday desktop apps can now be written with kargui alone: menus, tabs, number inputs, toolbars,
grids, split panes, drawing surfaces, trees, tables, date pickers, more windows, open and save file
dialogs, asking before closing, and more. As a check, three ordinary apps were written with kargui
only (`examples/notepad`, `examples/expenses`, `examples/files`), and the real file dialogs were
tried by hand.

Upgrading from 0.1 (compared with the published 0.1.0; nothing was removed):

- `Key` is now `#[non_exhaustive]` and has many more keys, so a `match` that lists every key needs
  a `_ =>` arm.
- `Options` has two new fields, `icon` and `min_size`. Code that builds it by listing every field
  must add them, or use `..Options::new(title)`.
- Ctrl+Tab no longer moves the focus like Tab. Inside tabs it switches tabs; elsewhere the app
  receives it, for example as a shortcut.
- `App` has two new methods, `window` and `close_requested`. Both have default bodies, so existing
  apps keep compiling.

- The crate now states its minimum Rust version (1.95, `rust-version`). Building on Linux needs the
  fontconfig development files (`libfontconfig1-dev` and `pkg-config` on Debian and Ubuntu); the
  README says so.

- Disabled widgets: `Ui::enabled(bool, |ui| ..)` disables every widget created inside, and
  `Button::enabled(bool)` disables one button. Disabled widgets are drawn dimmed, ignore the
  pointer, the keyboard and screen-reader actions, are skipped by Tab, lose the focus, and are
  announced as unavailable. `Harness::enabled(name)` tells whether a widget can be used.
- Layers drawn above everything else, inside the window: `Ui::popup` (next to an anchor widget,
  below it or above it when there is no room; closes on a click outside or Escape), `Ui::dialog`
  (modal, in the middle of the window: dims and blocks the rest, keeps Tab inside, closes on
  Escape) and `.tooltip(text)` on any widget (shown after the pointer rests for half a second or
  when the keyboard focuses the widget; also the widget's screen-reader description). Opening moves
  the focus inside; closing gives it back. Screen readers see them as a pane, a modal dialog with
  its title and a tooltip.
- Keys, time and threads: `Ui::shortcut(key, mods)` receives the keys no widget used (the focused
  text field keeps letters and its editing keys, sliders and lists keep the arrows); `Key` now has
  every letter, digits, F1-F12, Page Up/Down and Insert, and is `#[non_exhaustive]` (breaking for
  exhaustive matches); `Mods::NONE/SHIFT/CTRL/ALT` combine with `|`. `Ui::now` and
  `Ui::request_frame_after(duration)` wake the window at a time without drawing in between (on
  Windows a timer); tooltips now wait this way instead of drawing every frame. `Ui::waker` returns a
  `Waker` that wakes the window from any thread (merged until the window reads it). Alt+F4 and F10
  keep their Windows behaviour, and Alt+letter no longer beeps. Tests: `Harness::hover(name)` and
  `Harness::wait(duration)` (time moves without sleeping).
- Icons: 18 built-in vector icons (`Icon`, `Icon::ALL`) drawn sharply at any size by kargui's own
  rasterizer into the text atlas (so both renderers draw them), shown with `Ui::icon`,
  `Button::icon` and `Ui::icon_button(icon, name)`. The check box mark is now this icon instead of a
  font glyph. Unnamed icons are hidden from screen readers.
- Images: `Image` (`from_rgba`, `from_bytes`, `open`, `width`, `height`) and `Ui::image(&image)`
  with `.size(..)` and `.label(..)`. Files are decoded with Windows Imaging Component (no new
  dependencies); on other systems `from_bytes` returns an error for now. Drawn by the CPU renderer
  (bilinear scaling); the Direct3D 11 renderer does not draw images yet.
- More widgets: `Ui::radio_group` (one Tab stop, arrows select and wrap), `Ui::separator`,
  `Ui::progress`, `Ui::progress_busy` and `Ui::spinner` (the moving ones wake about 30 times a second
  with a timer, not every frame), `Ui::card`, `Ui::group` (a titled group for screen readers),
  `Ui::expander` (reports expanded), `Ui::link`, `Ui::toggle_button`, `Ui::password` (dots, no copy
  or cut, no value for screen readers) and `Toggled::mixed` for partly checked boxes.
- Multi-line text: `Ui::text_area(&mut text)` with `.hint`, `.rows(n)`, `.changed`, `.composing`.
  Enter inserts a new line (Ctrl+Enter reaches the app), long lines wrap, the field scrolls (wheel,
  scroll bar, and to the caret after typing or moving), and Up/Down (keeping the column),
  Home/End per line, Ctrl+Home/End and Page Up/Down move the caret; a click places the caret on
  the clicked line. Selections and the input method's clause underlines are drawn line by line.
  Pasted CR LF and CR become LF. Screen readers see a multi-line text input. The text layer now
  reports lines, caret positions and hit points (both DirectWrite and parley).
- Drop-downs: `Ui::dropdown(&mut value, &[(value, label), ..])` returns a `Dropdown` with
  `.changed()`. The list opens below the field (above when there is no room) as a light-dismiss
  layer with the focus on the selected option. Closed, Up/Down pick the previous or next option;
  Enter, Space, Alt+Down and F4 open it; open, Up/Down/Home/End move (wrapping), Enter or Space
  picks, Escape or a click outside closes. Screen readers see a combo box with its value and
  whether it is expanded, and a list box of options.
- Menus: `Ui::menu_button`, `Ui::menu_bar` with `MenuBar::menu`, and `.context_menu(..)` on any
  widget (right-click, Shift+F10 or the context menu key). Items: `Menu::item`, `Menu::check`,
  `Menu::radio`, `Menu::separator` and `Menu::submenu` (opens beside, on click, Right or a 0.4 s
  rest of the pointer); `MenuItem::shortcut` shows the keys and chooses the item even while the
  menu is closed (in a context menu, while the focus is inside its widget), `MenuItem::enabled`,
  `MenuItem::icon`. Keyboard as in the WAI-ARIA menu patterns: arrows, Home/End, a letter jumps to
  the next item starting with it, Right/Left for submenus and neighbouring menu bar menus, Escape
  and Tab close, F10 moves to the menu bar and back. Only one menu chain is open at a time; while
  a menu bar menu is open, pointing at another heading switches. Screen readers see menu buttons,
  a menu bar, menus, and menu items (checkable and radio items with their state, and shortcuts).
  New: `Key::ContextMenu`, `Harness::right_click`. Drop-downs now also pick by typing a letter.
- Tabs: `Ui::tabs(&mut value, &[(value, label), ..], |ui, value| ..)` returns `Tabs` with
  `.changed()`. Tab stops once on the selected heading; Left/Right (wrapping), Home and End select;
  Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageDown and Ctrl+PageUp switch tabs while the focus is inside
  (Ctrl+Tab no longer moves the focus). Screen readers see a tab list, tabs and a tab panel named
  after the selected tab.
- Number inputs: `Ui::number(&mut value, range)` with `.step(..)` and `.changed()`, for every
  integer and float type. Typed text becomes the value on Enter or when the focus leaves (not a
  number: undone; out of range: moved to the nearest end; full-width digits work). Up/Down step,
  Page Up/Page Down step 10 times; small up and down buttons keep the focus in the field. Screen
  readers see a spin button with value, range and step, and can increase and decrease it.
- Toolbars: `Ui::toolbar(|tb| ..)` with `Toolbar::button(icon, name)`, `Toolbar::toggle`,
  `Toolbar::separator`, `Toolbar::more(|m| ..)` (a "More options" menu) and `Toolbar::ui()`.
  Buttons are drawn without borders and show their names as tooltips. Tab stops once (on the button
  used last); Left/Right/Home/End move between buttons, skipping unavailable ones.
- Grids: `Ui::grid(&[Size, ..], |ui| ..)` places widgets in cells left to right, wrapping after
  the number of columns. Columns are `Auto` (widest cell), `Fixed` or `Fill` (share of the rest);
  rows are as tall as their tallest cell, with cells centered vertically.
- Split panes: `Ui::split_left_right(&mut width, |ui, pane| ..)` and `Ui::split_top_bottom`
  return a `Split` with `.label`, `.grow`, `.height` and `.changed`. The divider can be dragged,
  stops Tab, moves by 10 with the arrow keys and to the ends with Home and End, keeps both panes at
  least 48 wide, shows a resize pointer on Windows, and is a splitter with its value for screen
  readers.
- Drawing surfaces: `Ui::canvas(w, h, |d| ..)` with `Drawing::rect`, `rounded_rect`, `circle`,
  `line`, `polyline`, `polygon`, `text` and `image`, in the surface's own coordinates and clipped to
  it. kargui fills diagonal lines and polygons itself into the text atlas, so both renderers draw
  them (images: CPU renderer only for now). `CanvasView::pointer` (while pressed) and
  `CanvasView::clicked_at`. Screen readers see a canvas with its name.
- Tests: `Harness::drag(name, dx, dy)`.
- Trees: `Ui::tree(&mut selected, |t| ..)` with `TreeBuilder::branch(key, text, |t| ..)` and
  `TreeBuilder::leaf`; kargui keeps which branches are open (closed at first; children of closed
  branches are not created). Click selects, the arrow opens and closes; Tab stops once, Up/Down/
  Home/End/letters move and select, Right opens or goes to the first child, Left closes or goes to
  the parent, Enter and Space open and close. Screen readers see a tree and items with level,
  expanded and selected.
- Tables: `Ui::table(&[TableColumn], count, row_height)` with `.sort(&mut Option<(column, ascending)>)`,
  `.selection`, `.label`, `.grow` and `.show(|row, i| ..)` / `.show_keyed`; `TableRow::text` and
  `TableRow::cell`; `TableColumn::new(..).width(..).fill().right()`. Rows are the virtual list's
  rows; cells line up with the headings; grips between headings resize columns (remembered by
  kargui, at least 40); headings are one Tab stop with Left/Right and sort with Enter/Space. Screen
  readers see a table with row and column counts, a heading row with sort directions, rows and
  cells with their indices.
- Dates: `Date` (a calendar date with `new`, `today`, `year`, `month`, `day`, `weekday`,
  `add_days`, `add_months`, `days_in_month`, shown as `2026-11-22`; no new dependencies) and
  `Ui::date_picker(&mut Option<Date>)`: a field that opens a month calendar (arrows by day and week,
  Home/End, Page Up/Down by month, with Shift by year, Enter/Space pick, Escape closes; today is
  ringed, the picked day filled). `Ui::set_date_text(DateText::japanese())` sets the names of days
  and months (English by default). `Ui::today()` (fixed at 2026-11-22 in tests). Screen readers see
  a date input with its value and a grid of days with full names.
- More windows: `Ui::window(key, &Options, &mut open)` keeps another window open while `open` is
  true; its content comes from the new provided method `App::window(&mut self, key, ui)`. All
  windows share the app; after input in one window, the others draw again. The close button sets
  `open` to false; setting it to false closes the window; closing the main window ends the app. On
  Windows each window has its own renderer, screen-reader tree, input method state and timers.
  Tests: `Harness::window(key)`, `Harness::main_window`, `Harness::windows`,
  `Harness::close_window`.
- File dialogs: `Ui::file_dialog(key, FileDialog::open()/save())` with `.title`, `.filter`,
  `.file_name`, `.folder` and `.multiple`; the system's dialog is shown after the frame (on Windows,
  IFileOpenDialog and IFileSaveDialog; the window thread becomes a COM single-threaded apartment the
  first time a dialog is shown, and shell32 is loaded only then, so startup does not slow down),
  and the chosen files arrive once through `Ui::chosen_files` / `Ui::chosen_file` (a cancel once
  through `Ui::file_dialog_cancelled`). Tests:
  `Harness::file_dialog_key`, `file_dialog`, `choose_files`, `cancel_file_dialog`.
- Closing: `App::close_requested` (return false to keep the main window open, for example to ask
  about unsaved changes) and `Ui::quit`. Tests: `Harness::request_close`, `Harness::is_closed`.
- `Options::icon` (the title bar and taskbar icon; other windows inherit it) and
  `Options::min_size` (the smallest inside size the user can resize to). `Options` has two new
  fields, so code that builds it with every field listed must add them (`..Options::new(..)`
  avoids this).
- Three ordinary apps as the final check that apps can be written with kargui alone:
  `examples/notepad` (menu bar with shortcuts, multi-line text, open and save dialogs, asking before
  closing, window icon), `examples/expenses` (a form on a grid with a date picker, drop-down and
  number input, tabs, a sortable table with row context menus, a toolbar, a bar chart on a canvas)
  and `examples/files` (a folder tree next to a file table in split panes, properties in another
  window). Writing them found and fixed: `Ui::file_dialog_cancelled`, `TableRow::context_menu`, tree
  keys only need `Clone` (paths work), `Date::from_unix_days` / `Date::unix_days`, grown tabs give
  their panel the remaining height, and the test harness clicks what a user would see: among
  widgets with the same name, one in the open menu or dialog first, then a usable one on top, then
  any interactive one (a field rather than its label). `Harness::click_at(x, y)`.
- Fix: when a popup, dialog, list or menu opens or closes, the focus that moves into it (or back)
  is drawn at once. Before, the frame was drawn before the focus moved, and the old focus stayed on
  screen until the next input.
- Linux and macOS (preparation): kargui now builds on them, and its tests (widgets, layout,
  drawing, the test harness) pass there. Text on these systems is laid out with parley and drawn
  with swash; Windows keeps DirectWrite and gains no dependencies. Windows cannot be opened there
  yet: `kargui::run` returns an error. The `portable-text` feature uses the parley/swash text on
  Windows too, to test that path. GitHub Actions runs the tests on all three systems.

## 0.1.0 (2026-11-21)

The first version: the framework, four example apps, the guide (Japanese and English) and the checks
that keep the documentation current. Licensed under MIT. What is still missing is listed in the README
("Status and next steps").

- Widgets: heading, label (wrap, size, right alignment, strike), button (primary, small), check box,
  switch, dark mode switch, segmented choice, text input (input methods, undo, clipboard), slider,
  virtual list (rows keyed by ids, selection, separators), scroll container.
- Layout: row, column, stack, fill, space, fixed and shared sizes, padding, gap, alignment.
- Rendering: CPU renderer that redraws only what changed (default), Direct3D 11 renderer (optional).
- Input methods (IMM32): composition shown in the field with clause underlines, candidate window
  position, document feed and reconversion requests, committing on focus changes.
- Accessibility: AccessKit / UI Automation tree with roles, names, states and list positions.
- Testing: `kargui::test::Harness` runs apps without a window (pointer, keys, input method, wheel,
  screen-reader names, rendered pictures).
