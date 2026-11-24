<!-- source: ja/08-accessibility.md fnv=b8278841dea88050 -->
<!-- covers: Handle::label Ui::checkbox Ui::slider List::selection sig=3d4f35766a528b40 -->
# Screen readers and the keyboard

kargui gives every widget to screen readers (Narrator, NVDA). The kind of widget decides its role (button, check box, slider and so on), and its name comes from its text.

## Names

Give a name with `.label(..)` to widgets without text (sliders, buttons with only a symbol) and to widgets that share the same text.

```rust
# let mut volume = 0.5_f32;
# kargui::test::example(|ui| {
ui.slider(&mut volume, 0.0..=1.0).label("Volume");
ui.button("×").label("Close");
# });
```

If a control has no name, the log (wherever you sent it with `set_logger`) gets an `a11y.unlabeled` line.

## The keyboard

- Tab and Shift+Tab move through the controls in screen order. Moving to a control that is out of view in a scroll container or list scrolls it into view.
- Space and Enter press the focused control.
- Sliders move with the arrow keys, Home and End; selectable lists move with the up and down arrows, Home and End.
- The focus ring appears only when focus was moved with the keyboard.
- Clicking a control that can take the focus moves the focus to it. Clicking where there is nothing, or on plain text, leaves the focus where it was (as with Windows' own controls), so you can go on typing in a text field.

## Lists

List rows carry "item N of M". Only the visible rows (and the selected row) go into the screen-reader tree, so a list of ten thousand items stays light.
