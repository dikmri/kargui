<!-- source: ja/21-color-and-time.md fnv=005cb1fcbd4eead4 -->
<!-- covers: Ui::time_picker TimePicker::changed TimePicker::minute_step Time::new Time::now Time::parse Time::hour Time::minute Time::minutes Time::from_minutes Time::add_minutes Ui::color_picker ColorPicker::changed ColorPicker::alpha ColorPicker::swatches Ui::set_color_text ColorText::english ColorText::japanese sig=e317c6ca600891c4 -->
# Color and time pickers

## Time picker

`ui.time_picker(&mut time)` is a field for a time of day (`Time`). Clicking it opens a grid of hours and a grid of minutes below it. The time is an `Option<Time>`, `None` until one is picked.

```rust
use kargui::Time;

# let mut start: Option<Time> = Time::new(9, 30);
# kargui::test::example(|ui| {
if ui.time_picker(&mut start).label("Start").minute_step(15).changed() {
    // save the new time
}
# });
```

- Picking an hour moves to the minutes; picking a minute closes the grids. Picking an hour keeps the minute.
- Minutes go in steps of 5. `.minute_step(15)` offers 00, 15, 30 and 45, `.minute_step(1)` all 60. A time that is not on a step (such as 9:37) is kept as it is.
- Keyboard: Enter, Space, Alt+Down or F4 on the field opens it. In the grids the arrows move and Enter or Space picks. Escape or a click outside closes without picking.
- With no time yet, the grids open with the focus on the current hour (12 in tests).
- The field's text ("Pick a time") and the names of the grids follow `ui.set_date_text`, as the date picker does. Screen readers hear a time input and two grids named for hours and minutes.

`Time` holds hours and minutes. Arithmetic goes round the 24-hour clock.

```rust
use kargui::Time;

let t = Time::new(23, 50).unwrap();
assert_eq!(t.add_minutes(20).to_string(), "00:10");
assert_eq!(Time::parse("7:05"), Time::new(7, 5));
assert_eq!(Time::from_minutes(90).hour(), 1);
assert_eq!(Time::new(1, 30).unwrap().minutes(), 90);
assert_eq!(Time::new(18, 45).unwrap().minute(), 45);
assert!(Time::now().hour() < 24);
```

## Color picker

`ui.color_picker(&mut color)` is a field for a color (`Color`). Clicking it opens, below it:

- a square for saturation (left to right) and brightness (bottom to top)
- a hue bar
- a field for the color code (`#RRGGBB`)
- 16 ready-made colors

```rust
# let mut accent = kargui::rgb(0x1a73e8);
# kargui::test::example(|ui| {
if ui.color_picker(&mut accent).label("Accent color").changed() {
    // redraw with the new color
}
# });
```

- Dragging in the square or a bar, pressing an arrow key, finishing a 6-digit code, or clicking a ready-made color changes the color at once. Escape or a click outside closes.
- A 3-digit code (such as `#F80`) is read when Enter is pressed, so a code half typed or half deleted (such as `#FF9`) does not change the color.
- In the square and the bars the arrow keys move by 1% (1° for hue), by 10 with Shift.
- While it is open the hue is remembered: lowering the saturation to 0 (grey) and raising it again comes back to the same hue.
- A ready-made color or a typed code is returned exactly, so it compares equal (`==`) with the app's own colors.
- The square and the bars are drawn as pictures (images), which the Direct3D 11 renderer (not the default) does not draw yet.

To pick opacity too, add `.alpha()`: an opacity bar appears and codes become `#RRGGBBAA`. Without it, opacity stays as it was. `.swatches(&[colors])` replaces the ready-made colors (8 per row).

```rust
use kargui::rgb;

# let mut shade = [0.0, 0.0, 0.0, 0.4];
# let mut label = rgb(0xd93025);
# kargui::test::example(|ui| {
ui.color_picker(&mut shade).alpha();
let brand = [rgb(0xd93025), rgb(0x188038), rgb(0x1a73e8)];
ui.color_picker(&mut label).swatches(&brand);
# });
```

The names of the square and the bars ("Hue", "Opacity" and so on) follow `ui.set_color_text`. Screen readers hear a color well, sliders for the square and the bars, a text field for the code and buttons for the ready-made colors.

```rust
use kargui::ColorText;

# kargui::test::example(|ui| {
ui.set_color_text(ColorText::japanese());
# });
assert_eq!(ColorText::english().hue, "Hue");
```

## Tests

Color and time pickers are clicked by name like other widgets. The cells are named `09 Hours` and `30 Minutes` (`09 時` and `30 分` in Japanese), and the ready-made colors by their codes (`#D93025`).

```rust
use kargui::{App, Time, Ui};
use kargui::test::Harness;

struct Form {
    start: Option<Time>,
    color: kargui::Color,
}

impl App for Form {
    fn view(&mut self, ui: &mut Ui) {
        ui.time_picker(&mut self.start).label("Start");
        ui.color_picker(&mut self.color).label("Color");
    }
}

let mut h = Harness::new(Form { start: None, color: kargui::rgb(0x000000) });
h.click("Start").click("09 Hours").click("30 Minutes");
assert_eq!(h.app().start, Time::new(9, 30));
h.click("Color").click("#D93025");
assert_eq!(h.app().color, kargui::rgb(0xd93025));
```
