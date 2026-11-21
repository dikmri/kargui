<!-- source: ja/03-layout.md fnv=5181bf67fc925dac -->
<!-- covers: Ui::row Ui::column Ui::stack Ui::fill Ui::space Handle::width Handle::height Handle::grow Handle::gap Handle::padding Handle::align Size Align Ui::set_padding Ui::set_gap sig=6ec8a790ae06df48 -->
# Layout

The outermost level of the screen is a column that goes from top to bottom. Use `row` to place things side by side and `column` to place them one below another.

```rust
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.heading("Tasks");
    ui.fill(); // takes the rest of the width and pushes the following widgets to the right
    ui.button("Settings");
});
ui.column(|ui| {
    ui.label("Line 1");
    ui.label("Line 2");
});
# });
```

## Sizes

Sizes are set with `Size`:

- `Size::Auto`: as large as the content (the default)
- `Size::Fixed(120.0)`: a set length (logical pixels)
- `Size::Fill(1.0)`: share the rest by weight

`grow()` is short for "take the rest along the direction things are placed": the width in a row, the height in a column.

```rust
# use kargui::Size;
# let mut query = String::new();
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.label("Name").width(Size::Fixed(100.0));
    ui.text_input(&mut query).grow();
});
# });
```

## Gaps and padding

```rust
# use kargui::Align;
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.button("Yes");
    ui.button("No");
})
.gap(16.0) // between the children
.padding(12.0, 8.0) // left/right and top/bottom
.align(Align::Start); // align to the top inside the row
# });
```

Change the padding and gap of the whole screen with `ui.set_padding(x, y)` and `ui.set_gap(..)`.

## Layering

`stack` places its children on top of each other, centered by default.

```rust
# kargui::test::example(|ui| {
ui.stack(|ui| {
    ui.label("Below");
    ui.label("Above");
});
# });
```

## Two-pass layout

kargui first measures sizes from the bottom up, then places things from the top down. Centering something whose size is not known in advance, and sharing the remaining width, are decided correctly within one frame.
