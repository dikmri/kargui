<!-- source: ja/22-rich-text.md fnv=32cfd3d9084fe7c8 -->
<!-- covers: Ui::rich_text RichText::text RichText::link RichLink::clicked RichLink::bold Span::bold Span::italic Span::underline Span::strike Span::weak Span::color Span::small Span::size Span::font Span::monospace Span::code sig=8398e6e20e27a91e -->
# Rich text

`ui.rich_text(|t| …)` lays out text whose parts have their own style as one paragraph. Bold,
italic, colors, code and links can be mixed in the middle of a sentence.

```rust
# let mut show_help = false;
# kargui::test::example(|ui| {
ui.rich_text(|t| {
    t.text("Saving cannot be ");
    t.text("undone").bold();
    t.text(". See ");
    if t.link("Help").clicked() {
        show_help = true;
    }
    t.text(" for details.");
});
# });
```

- The parts are laid out in the order you add them, as one paragraph that wraps to the width.
  In a row (`ui.row`), it takes the remaining width.
- Styles belong to a part. What you add after `t.text("…")` applies only to that part; the other
  parts keep the body style.
- A `\n` in the text starts a new line.
- For text with a single style, `ui.label("…").bold()` is shorter.

## Styles of a part

| Call | Look |
|---|---|
| `.bold()` | Bold |
| `.italic()` | Italic. Fonts without an italic style, such as Japanese fonts, are slanted |
| `.underline()` | A line under it |
| `.strike()` | A line through it (a price before a discount) |
| `.weak()` | The weak text color (notes) |
| `.color(color)` | That color |
| `.small()` | Small text (12.5) |
| `.size(size)` | A size in logical pixels (body text is 14). A larger part makes its line taller |
| `.font(font)`, `.monospace()` | A font (chapter 20) |
| `.code()` | The fixed-width font on a light background (code, key names, file names) |

```rust
use kargui::rgb;

# kargui::test::example(|ui| {
ui.rich_text(|t| {
    t.text("Price ");
    t.text("1,980").strike().weak();
    t.text(" 1,280").bold().color(rgb(0xc42b1c));
    t.text(" (tax included)").small().weak();
});
ui.rich_text(|t| {
    t.text("Save with ");
    t.text("Ctrl+S").code();
    t.text(". A ");
    t.text("draft").italic();
    t.text(" is kept ");
    t.text("every").underline();
    t.text(" time.");
});
# });
```

## Links

`t.link("…")` is a link inside a sentence. It is drawn in the accent color with a line under it.
When the pointer is over it, the pointer becomes a hand and the line gets thicker.

- A click, or Tab to move the focus to it and Enter, makes `.clicked()` true. Opening a web page
  is up to the app.
- A link that wraps onto two lines is one link on both lines. The focus ring is drawn on each line.
- `.bold()` makes it bold.
- A click writes `rich.link text=…` to the log.
- Inside `ui.enabled(false, …)` it cannot be clicked and is grayed out.

A single link outside a sentence can also be written with `ui.link("…")` (chapter 11).

## Screen readers and tests

Screen readers read the parts of the paragraph in order and announce links as links. In tests,
find and click a part by its text.

```rust
use kargui::{App, Ui};
use kargui::test::Harness;

#[derive(Default)]
struct Notice {
    opened: bool,
}

impl App for Notice {
    fn view(&mut self, ui: &mut Ui) {
        ui.rich_text(|t| {
            t.text("For details, see ");
            if t.link("Help").clicked() {
                self.opened = true;
            }
            t.text(".");
        });
    }
}

let mut h = Harness::new(Notice::default());
assert!(h.find("For details, see ").is_some());
h.click("Help");
assert!(h.app().opened);
```
