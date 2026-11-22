<!-- source: ja/20-fonts.md fnv=52689713caef4211 -->
<!-- covers: Font Font::family Ui::font Ui::set_font Label::font Label::monospace TextInput::font TextInput::monospace TextArea::font TextArea::monospace Harness::with_options sig=800225c94540e67a -->
# Fonts

## The window's font

If you choose nothing, text is drawn in the system's UI font: Yu Gothic UI on Windows. The font of the whole window is set with `Options::font`.

```rust
use kargui::{Font, Options};

let mut options = Options::new("Ledger");
options.font = Font::family("BIZ UDPGothic");
```

A font is a `Font`.

- `Font::Ui`: the system's UI font (the default). Yu Gothic UI on Windows, Hiragino Sans on macOS, and a Japanese sans-serif such as Noto Sans CJK JP on Linux
- `Font::Monospace`: a fixed-width font, for code, file names and numbers whose digits should line up. Fonts that also have Japanese come first, so a Japanese character is exactly as wide as two Latin letters (on Windows: BIZ UDGothic, then Consolas, then MS Gothic)
- `Font::family("name")`: a font chosen by name, either installed on the system or from a file the app brings along

If no font has the name, the UI font is used (and the log says `font.missing`). Characters a font lacks come from the system's fallback fonts, so Japanese does not disappear when you choose a Latin-only font.

To change the font while the app runs, use `ui.set_font`, for example for a font setting in the app. From that frame on, all text whose widget has no font of its own changes.

```rust
use kargui::Font;

# let mut readable = true;
# kargui::test::example(|ui| {
ui.checkbox(&mut readable, "Use an easy-to-read font");
ui.set_font(if readable { Font::family("BIZ UDPGothic") } else { Font::Ui });
assert_eq!(ui.font(), Font::family("BIZ UDPGothic"));
# });
```

## A font for one widget

Labels, text fields and multi-line text fields can have their own font with `.font(font)`; `.monospace()` is the short form for the fixed-width one.

```rust
use kargui::Font;

# let mut code = String::new();
# kargui::test::example(|ui| {
ui.label("Invoice").font(Font::family("BIZ UDPGothic"));
ui.label("ID: 4F2A-0019").monospace();
ui.text_area(&mut code).monospace().rows(8);
# });
```

In text fields, moving the caret to the line above or below and turning a click into a position in the text are measured in the field's own font as well.

## Bringing a font with the app

For a font the system does not have, put the contents of its file in `Options::fonts`. TrueType and OpenType (`.ttf`, `.otf`, `.ttc`) work. With `include_bytes!` the font is inside the executable, so it looks the same on every PC. To choose it, pass the family name written inside the font to `Font::family`.

```rust
use kargui::{Font, Options};

let mut options = Options::new("Shop");
// In an app: options.fonts.push(include_bytes!("../fonts/ShopSans.ttf").as_slice().into());
if let Ok(data) = std::fs::read("fonts/ShopSans.ttf") {
    options.fonts.push(data.into());
}
options.font = Font::family("Shop Sans");
```

- A font the app brings is used before a system font of the same name.
- While `Options::fonts` is empty, no font data is read when the window opens, and the list of system fonts is read only when a monospace or named font is first used. Apps that use only the UI font start as fast as before.
- Japanese font files are large (several MB to 20 MB for one sans-serif), and the executable grows by that much. When a system font is enough, choose it by name.

## Tests

`Harness::with_options(app, &options)` makes a test harness with the size, font and font files of the app's `Options`.

```rust
use kargui::{App, Font, Options, Ui};
use kargui::test::Harness;

struct Code {
    text: String,
}

impl App for Code {
    fn view(&mut self, ui: &mut Ui) {
        ui.row(|ui| {
            ui.label("iiii").monospace();
        });
        ui.row(|ui| {
            ui.label("MMMM").monospace();
        });
        ui.text_area(&mut self.text).hint("Code");
    }
}

let mut options = Options::new("Code");
options.font = Font::Monospace;
let h = Harness::with_options(Code { text: String::new() }, &options);
// fixed width: thin and wide letters take the same space
let (i, m) = (h.find("iiii").unwrap().w, h.find("MMMM").unwrap().w);
assert!((i - m).abs() < 1.0);
```
