# Changelog

## Unreleased

- Disabled widgets: `Ui::enabled(bool, |ui| ..)` disables every widget created inside, and
  `Button::enabled(bool)` disables one button. Disabled widgets are drawn dimmed, ignore the
  pointer, the keyboard and screen-reader actions, are skipped by Tab, lose the focus, and are
  announced as unavailable. `Harness::enabled(name)` tells whether a widget can be used.
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
