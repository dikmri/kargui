<!-- source: ja/13-tabs-numbers-toolbars.md fnv=dd0bb9fbc5c42051 -->
<!-- covers: Ui::tabs Tabs::changed Ui::number NumberInput::step NumberInput::changed Ui::toolbar Toolbar::button Toolbar::toggle Toolbar::separator Toolbar::more Toolbar::ui sig=04b1f1f2e9b03f53 -->
# Tabs, number inputs and toolbars

## Tabs

Clicking a heading switches the content below it. The closure you pass gets the selected value and builds the content for it.

```rust
#[derive(Clone, Copy, PartialEq)]
enum Page { General, Advanced }
# let (mut page, mut sound, mut proxy) = (Page::General, true, false);
# kargui::test::example(|ui| {
let pages = [(Page::General, "General"), (Page::Advanced, "Advanced")];
ui.tabs(&mut page, &pages, |ui, page| match page {
    Page::General => {
        ui.checkbox(&mut sound, "Play a sound");
    }
    Page::Advanced => {
        ui.checkbox(&mut proxy, "Use a proxy");
    }
});
# });
```

Tab stops once, on the selected heading. With the focus on a heading, Left and Right select the previous or next tab (wrapping around at the ends), and Home and End the first or last. With the focus in the content, Ctrl+Tab or Ctrl+PageDown selects the next tab, and Ctrl+Shift+Tab or Ctrl+PageUp the previous one.

Screen readers hear a tab list, tabs (and which one is selected) and a tab panel named after the selected tab.

## Number inputs

A field for typing a number, with small buttons at the right that step it up and down.

```rust
# let (mut copies, mut zoom) = (1, 1.0);
# kargui::test::example(|ui| {
ui.number(&mut copies, 1..=99).label("Copies");
ui.number(&mut zoom, 0.5..=4.0).step(0.25).label("Zoom");
# });
```

A typed number becomes the value when Enter is pressed or the focus leaves. Text that is not a number is undone, and a number outside the range is moved to the nearest end. Full-width digits typed with a Japanese input method also work. Up and Down change the value by `.step(..)` (1 if not set), and Page Up and Page Down by 10 times that.

Screen readers hear a spin button with its value and range. Their increase and decrease actions change the value too.

## Toolbars

A row of icon buttons. The buttons' names also show as tooltips.

```rust
use kargui::Icon;
# let (mut count, mut pinned) = (0, false);
# kargui::test::example(|ui| {
ui.toolbar(|tb| {
    if tb.button(Icon::Plus, "Add").clicked() {
        count += 1;
    }
    tb.toggle(&mut pinned, Icon::Check, "Pin");
    tb.separator();
    tb.more(|m| {
        m.item("Settings");
    });
})
.label("Tasks");
# });
```

- `tb.button(icon, name)`: a button that shows only an icon
- `tb.toggle(&mut value, icon, name)`: a button that switches on and off each time it is pressed
- `tb.separator()`: a line between buttons
- `tb.more(|m| ..)`: a button (three dots) that opens a menu of other commands, for commands used less often
- `tb.ui()`: the toolbar's `Ui`, to put other widgets in it, such as a search field

Tab stops once on the toolbar (on the button used last). Left, Right, Home and End move between its buttons, skipping unavailable ones. Name the toolbar with `.label(..)`.
