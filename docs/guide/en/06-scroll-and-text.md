<!-- source: ja/06-scroll-and-text.md fnv=1735e52f18557486 -->
<!-- covers: Ui::scroll Label::wrap Label::size Handle::scroll_into_view sig=ca0d715071c2fdd1 -->
# Scrolling and long text

## The scroll container

Put screens that get taller than the window (a list of settings, a long text) in `ui.scroll`. The widgets inside are placed from top to bottom.
By default it takes the remaining height. It always keeps room for the scroll bar on the right.

```rust
# kargui::test::example(|ui| {
ui.heading("Settings");
ui.scroll(|ui| {
    for i in 0..50 {
        ui.label(format!("Line {i}"));
    }
});
ui.button("Save");
# });
```

Tab also reaches widgets that are out of view, and the container scrolls to show the focused widget.

## Wrapping text

`wrap()` wraps text to fit the width. In a reading view, letting the user change the text size with `size(..)` makes it easier to read.

```rust
# let font = 16.0;
# kargui::test::example(|ui| {
ui.scroll(|ui| {
    ui.label("Chapter 1").bold().size(font * 1.35);
    ui.label("A long paragraph wraps to fit the width of the container.").size(font).wrap();
});
# });
```

## Jumping from a table of contents

Calling `scroll_into_view()` when you create a heading brings it to the top of the scroll container once the frame is laid out.

```rust
# let mut jump_to: Option<usize> = Some(2);
# kargui::test::example(|ui| {
ui.scroll(|ui| {
    for chapter in 0..5 {
        let heading = ui.heading(format!("Chapter {chapter}"));
        if jump_to == Some(chapter) {
            heading.scroll_into_view();
        }
        ui.label("...").wrap();
    }
});
jump_to = None;
# });
```

The table of contents and the headings may have the same names. `examples/reader` is a reading view with a table of contents, previous and next chapter buttons, and a text size slider.
