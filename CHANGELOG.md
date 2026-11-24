# Changelog

## Unreleased

- Linux: apps open a Wayland window (first part). `kargui::run` no longer returns an `Err` on
  Linux. The window is an xdg-shell toplevel drawn by the CPU path into shared memory, at the
  compositor's fractional scale (fractional-scale and viewporter; integer scale where they are
  missing). Mouse: move, press (double and triple clicks within 400 ms and 4 pixels), wheel
  (one notch is 96 pixels, as on Windows) and touchpad scrolling, right click, and pointer shapes
  (cursor-shape). Keyboard: the system key map through libxkbcommon, loaded at run time with
  xkbcommon-dl (no development package is needed to build), and key repeat. Copy and paste use
  the system clipboard (text, through wl_data_device; pasting waits up to one second for the other
  app). `Date::today()` and `Time::now()` use the local time zone (they used UTC on Linux). The
  input method works through text-input-v3: it is turned on while a text field has the focus, the
  composition is drawn in the field with the clause being converted, the candidate window opens
  under the caret, and a composition is committed when the field or the window loses the focus
  (checked with fcitx5 and Mozc on Hyprland). Screen readers get the same tree as on Windows
  through AT-SPI (accesskit_unix; checked with Orca). `ui.file_dialog` opens the dialog of
  xdg-desktop-portal (when the app gives no folder, it starts in the folder of the file chosen
  last, as the Windows dialog does), and the window starts in the system's dark or light mode (the portal's
  color-scheme, read while the window is being set up). `ui.window` opens more windows, as on
  Windows: each window has its own surface and runtime and shares the app; input in one window
  redraws the others once; a window's close button sets its `open` flag to false (the position, the
  icon and whether it comes to the front are left to the compositor). Files dragged in from other
  apps arrive as on Windows (`dropped::<Vec<PathBuf>>`; the text/uri-list is read when the drag
  enters, and the compositor is told whether the widget under the pointer takes files). Where the
  compositor does not draw title bars (GNOME has no xdg-decoration), kargui draws one: the title in
  the middle and the buttons that GNOME's button-layout setting names (read from the portal; close
  only by default), a move when the empty part is dragged, maximize on a double click, the window
  menu on a right click, resizing from the window's edges and corners, and a one-pixel border while
  the window floats. The app's size is the size of its content, as on Windows (the title bar is
  added). A dialog darkens only the content, so the title bar can still move or close the window.
  Apps also open X11 windows: kargui uses Wayland when the compositor's socket is there and X11
  (`DISPLAY`) otherwise, and `KARGUI_BACKEND=x11` or `wayland` chooses one. The X11 window draws
  through MIT-SHM (PutImage where it is missing), scales by `Xft.dpi`, reads the server's key map
  with libxkbcommon-x11 (loaded at run time, like libxcb), lets the server repeat keys, and has the
  same clipboard, screen reader tree, file dialogs (now attached to their window), second windows
  (with their position and icon) and files dropped from other apps (XDND) as on Wayland. The input
  method on X11 is fcitx5 or IBus through D-Bus: keys go to the input method first, and only
  the keys it does not use reach the app; the composition, the clause being converted and the
  candidate window under the caret work as on Wayland. kargui uses the one that is running on the
  session bus (it does not start one): fcitx5 when both are (fcitx5 also answers as IBus, so a
  name owned by fcitx5 does not count as IBus), and IBus first when `XMODIFIERS` is `@im=ibus`.
  IBus is reached through its portal (ibus-portal); its own panel draws the candidate window
  (checked with fcitx5 and Mozc, and with IBus 1.5 and ibus-mozc, under XWayland). New Linux-only dependencies: wayland-client,
  wayland-protocols, xkbcommon-dl, rustix, accesskit_unix, zbus and x11rb (89 crates in all; 71 of
  them come with accesskit_unix, that is zbus and AT-SPI).
- Password fields turn the input method off (on Windows, Wayland and X11): what is typed there is
  not sent to the input method's conversion or learning, as with Windows' own password boxes.
- Number fields: holding a small step button steps once at once, then every 60 ms after 400 ms
  (counted from the time held, so slow frames do not slow it down).
- Tables: Shift+Left and Shift+Right on a focused heading change that column's width (by 10).
  Headings of a table with resizable columns can now take the focus even when it does not sort;
  screen readers hear how to resize.
- Lists and tables: `List::type_ahead(|i| text)` and `Table::type_ahead(|i| text)` let the user
  jump to the next row that starts with a typed letter or digit (wrapping, ignoring case). Rows off
  screen are not built, so the app gives their text.
- The Direct3D 11 renderer (`Renderer::Gpu`) now draws images (they were left out before), with
  the same linear filtering as the CPU renderer.
- A new example, `examples/gallery`, shows every widget on its own page with its disabled state.
- Tables: in a narrow table, a cell whose text is wider than its column no longer pushes the
  cells after it to the right (the columns of every row stay under their headings); the text is
  cut at the edge of the cell. Header cells do the same.
- Icons: `Icon::Maximize` (a square) and `Icon::Restore` (two squares). `Icon::ALL` now has 21
  icons.
- Screen readers: text fields and text areas now expose their text as text runs (one per line)
  with the caret and the selection, so a screen reader can read the text and follow the caret.
  On Linux, Orca needs this to read a field at all; on Windows, fields gain the UI Automation
  text pattern next to the value pattern. Password fields still expose nothing.
- Rich text: `ui.rich_text(|t| …)` lays out parts that have their own style as one paragraph that
  wraps to the width. `t.text(…)` returns a `Span` with `.bold()`, `.italic()` (slanted where the
  font has no italic style, as Japanese fonts), `.underline()`, `.strike()`, `.weak()`,
  `.color(c)`, `.small()`, `.size(px)`, `.font(f)`, `.monospace()` and `.code()` (the fixed-width
  font on a light background). `t.link(…)` returns a `RichLink` whose `.clicked()` is true after a
  click, or Enter while it has the focus (Tab stops at links; a link that wraps onto two lines is
  one link on both). Clicks are logged as `rich.link`. Screen readers get a paragraph whose parts
  are text and links, and tests find and click a part by its text. Guide page 22.
- The pointer becomes a hand over a link (`ui.link` and links in rich text).
- Time picker: `ui.time_picker(&mut Option<Time>)` is a field that opens a grid of hours and a
  grid of minutes (picking an hour moves to the minutes, picking a minute closes; arrow keys,
  Enter or Space, Escape). `.minute_step(n)` sets the minutes offered (5 by default). `Time` is a
  new type for a time of day (`Time::new`, `now`, `parse`, `hour`, `minute`, `minutes`,
  `from_minutes`, `add_minutes`, shown as `09:30`). `DateText` has new fields `time_empty`,
  `hours` and `minutes` (code that builds a `DateText` with a struct expression must add them).
  Screen readers hear a time input and grids named for hours and minutes.
- Color picker: `ui.color_picker(&mut Color)` is a field that opens a square for saturation and
  brightness, a hue bar, a color code field (`#RRGGBB`) and 16 ready-made colors. `.alpha()` adds
  an opacity bar and `#RRGGBBAA` codes; `.swatches(&colors)` replaces the ready-made colors. The
  hue is kept while the saturation passes through grey, and a ready-made color or a typed code is
  returned exactly. A typed code takes effect when it has 6 digits (8 with `.alpha()`), or with
  Enter for a 3-digit code. The words come from `ColorText` (`english`, `japanese`) through
  `ui.set_color_text`. The square and the bars are drawn as images, which the Direct3D 11 renderer
  does not draw yet.
- Text fields: dragging with the mouse selects text (before, only the caret moved to the
  press), a double-click selects a word, and a triple-click selects all of a one-line field or
  one line of a multi-line field. Double- and triple-clicks follow the system's double-click time
  and distance. Tests: `Harness::double_click` and `Harness::triple_click`.
- `Icon::Clock`. Guide page 21. The settings example picks an accent color and quiet hours.
- Fonts: `Font` is `Font::Ui` (the system's UI font, the default), `Font::Monospace` (a
  fixed-width font; on Windows BIZ UDGothic, where a Japanese character is exactly two Latin
  letters wide, then Consolas, then MS Gothic) or `Font::family(name)`. `Options::font` sets the
  font of the windows, `ui.set_font` changes it while the app runs and `ui.font()` reads it.
  Labels, text fields and multi-line text fields take `.font(font)` and `.monospace()`; caret
  movement and clicks in a field are measured in its font. `Options::fonts` takes font files the
  app brings along (TrueType or OpenType, for example with `include_bytes!`), chosen by family name
  and found before system fonts of the same name. A name no font has falls back to the UI font
  (logged as `font.missing`). Nothing is read for fonts at startup unless the app uses them.
  Tests: `Harness::with_options(app, &options)`. Guide page 20. The notepad example has View >
  Monospace font, and the settings example a font choice that applies to the whole window.
- `Options` (behaviour change for code that builds it with a struct expression): new fields `font`
  and `fonts`. `Options::new` fills them in.
- Animation helpers: `ui.animate(key, target, duration)` returns a number that moves from where it
  is to `target` over `duration` whenever `target` changes (a cubic ease-out; the first call
  returns `target` at once, and turning back midway starts from where the value is), and
  `ui.animate_bool(key, on)` moves between 0.0 and 1.0 in 250 ms. Frames are drawn only while a
  value moves. When the user turned off Animation effects in Windows (read when a window is created
  and again on `WM_SETTINGCHANGE`), values jump to the target; `ui.reduce_motion()` tells your own
  effects to do the same. `.reveal(amount)` shows only the top part of a widget and cuts off the
  rest (with `animate_bool`, a section that slides open and closed). The switch thumb of
  `ui.toggle` now slides in 167 ms, and the content of `ui.expander` slides in and out in 250 ms.
  Guide page 19.
- Tests (behaviour change): `Harness` now starts with motion turned off, so the screen after an
  action is already its end state (an expander's content is gone right after closing it). Turn
  motion on with `Harness::reduce_motion(false)` and move the clock with `Harness::wait` to test
  motion itself.
- Drag and drop inside the window: `.drag_source(payload)` on any widget lets the pointer carry
  `payload` (any `'static` type; it is taken once, so it need not be `Clone`), and
  `.dropped::<T>()` on a widget returns it when it is released there. A drag starts after the
  pointer moves 4 pixels (shorter moves stay clicks), a box with the widget's name follows the
  pointer, the widget that takes the payload is outlined, other places show the "not allowed"
  pointer, and Escape or losing the mouse capture cancels. Nested targets: the innermost one that
  takes the type wins, which makes reordering easy. Widgets that drag by themselves (text fields,
  sliders, dividers, drawing surfaces) and disabled widgets do not start a drag. Tests:
  `Harness::drag_to`, `Harness::drag_over` and `Harness::release`. Guide page 18 and the example
  `examples/board` (cards moved between columns, with a "Move" menu for people who cannot drag).
- Dropping files from other apps (Explorer): a widget that takes `Vec<PathBuf>` with
  `.dropped::<Vec<PathBuf>>()` receives the paths of files dropped on it, the same way as drag and
  drop inside the window (an in-window drag that carries `Vec<PathBuf>` lands there too). Over such
  a widget the pointer shows "copy" and the widget is outlined; the system draws the picture of the
  files. The window is registered with the system (OLE `RegisterDragDrop`) only when such a widget
  first appears, and shell32 is loaded only when files arrive, so other apps start as fast as
  before. Tests: `Harness::drop_files(target, paths)`. The notepad example opens a file dropped
  anywhere below its menu bar (asking first about unsaved changes).
- Layout (behaviour change): `Size::Fill` now shares the remaining length by weight whatever the
  children's content, like WinUI's star sizing and CSS `flex: 1`, and never makes a child shorter
  than its content. Before, each `Fill` child got its content length plus a share, so children with
  equal weights could get different lengths. A single `Fill` (a text field next to a button) lays
  out as before.

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
