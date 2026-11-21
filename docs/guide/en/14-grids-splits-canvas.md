<!-- source: ja/14-grids-splits-canvas.md fnv=f45043ac47f82091 -->
<!-- covers: Ui::grid Ui::split_left_right Ui::split_top_bottom Split::label Split::grow Split::height Split::changed Ui::canvas Drawing::rect Drawing::rounded_rect Drawing::circle Drawing::line Drawing::polyline Drawing::polygon Drawing::text Drawing::image CanvasView::pointer CanvasView::clicked_at Harness::drag sig=e5b37aca5ba6090b -->
# Grids, split panes and drawing surfaces

## Grids

Places widgets in cells, like a table. They go in left to right in the order you create them, and move to the next row after as many cells as there are columns. Use it to line up names and fields on a form.

```rust
use kargui::Size;
# let (mut name, mut email) = (String::new(), String::new());
# kargui::test::example(|ui| {
ui.grid(&[Size::Auto, Size::Fill(1.0)], |ui| {
    ui.label("Name");
    ui.text_input(&mut name).label("Name");
    ui.label("E-mail address");
    ui.text_input(&mut email).label("E-mail address");
});
# });
```

A column is as wide as its widest widget with `Size::Auto`, the given width with `Size::Fixed(..)`, and a share of the remaining width with `Size::Fill(..)`. A row is as tall as its tallest widget, and widgets are centered vertically in their row.

## Split panes

Two panes side by side, with a divider between them that can be dragged to change their sizes.

```rust
# let mut left = 220.0;
# kargui::test::example(|ui| {
ui.split_left_right(&mut left, |ui, pane| {
    if pane == 0 {
        ui.label("Folders");
    } else {
        ui.label("Files");
    }
})
.label("Width of the folder list")
.grow();
# });
```

`left` is the left pane's width in logical pixels. The closure you pass is called for each pane and gets the pane's number as its second argument (0 for the left, 1 for the right). For panes one above the other, use `ui.split_top_bottom(&mut top_height, ..)`. Neither pane gets narrower than 48.

Tab stops on the divider; the arrow keys move it by 10, and Home and End to the ends. Over the divider, the pointer becomes a left-right (or up-down) arrow. Screen readers hear the name given with `.label(..)` ("Resize" if none) and the current width.

## Drawing surfaces

A surface for charts and diagrams. Draw in the closure you pass, in coordinates from the surface's top left corner (logical pixels).

```rust
use kargui::{Rect, rgb};
# let values = [12.0, 30.0, 22.0, 48.0];
# kargui::test::example(|ui| {
ui.canvas(240.0, 120.0, |d| {
    d.rect(Rect::new(0.0, 0.0, 240.0, 120.0), rgb(0xf3f5f8));
    let points: Vec<(f32, f32)> = values.iter().enumerate().map(|(k, v)| (20.0 + k as f32 * 60.0, 110.0 - v * 2.0)).collect();
    d.polyline(&points, 2.0, rgb(0x2f6fd6));
    for &p in &points {
        d.circle(p, 4.0, rgb(0x2f6fd6));
    }
    d.text((8.0, 4.0), "Visits", rgb(0x333333));
})
.label("Visits per week");
# });
```

You can draw rectangles (`rect`, `rounded_rect`), circles (`circle`), lines (`line`, `polyline`), filled polygons (`polygon`), text (`text`) and images (`image`). Later shapes are drawn over earlier ones, and nothing is drawn outside the surface. kargui fills diagonal lines and polygons itself and puts them in the same atlas as text, so the CPU and GPU renderers draw the same picture. Images are drawn only by the CPU renderer for now.

Ask the returned value about the pointer.

```rust
# let mut stroke: Vec<(f32, f32)> = Vec::new();
# let mut marks: Vec<(f32, f32)> = Vec::new();
# kargui::test::example(|ui| {
let pad = ui.canvas(300.0, 200.0, |_| {}).label("Handwriting");
if let Some(at) = pad.pointer() {
    // where the pointer is while pressed (also outside the surface while dragging)
    stroke.push(at);
}
if let Some(at) = pad.clicked_at() {
    // where it was pressed and released
    marks.push(at);
}
# });
```

In tests, `h.drag(name, dx, dy)` presses a widget, moves the pointer and releases it.
