# Changelog

## Unreleased

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
- Multi-line text: `Ui::text_area(&mut text)` with `.hint`, `.rows(n)`, `.changed`, `.composing`.
  Enter inserts a new line (Ctrl+Enter reaches the app), long lines wrap, the field scrolls (wheel,
  scroll bar, and to the caret after typing or moving), and Up/Down (keeping the column),
  Home/End per line, Ctrl+Home/End and Page Up/Down move the caret; a click places the caret on
  the clicked line. Selections and the input method's clause underlines are drawn line by line.
  Pasted CR LF and CR become LF. Screen readers see a multi-line text input. The text layer now
  reports lines, caret positions and hit points (both DirectWrite and parley).
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
