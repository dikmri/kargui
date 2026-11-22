<!-- source: ja/19-animation.md fnv=c069d3bb7fdabddc -->
<!-- covers: Ui::animate Ui::animate_bool Ui::reduce_motion Handle::reveal Harness::reduce_motion sig=fed43c178b5db00c -->
# Motion

## Moving a value smoothly

`ui.animate(key, target, duration)` returns a number that, when the target changes, goes from where it is to the new target over `duration`. Use the number for whatever you draw: a width, a height, how strong a color is.

```rust
use std::time::Duration;
use kargui::Size;

# let mut wide = false;
# kargui::test::example(|ui| {
if ui.button("Wider").clicked() {
    wide = !wide;
}
let width = ui.animate("panel", if wide { 320.0 } else { 160.0 }, Duration::from_millis(250));
ui.column(|ui| {
    ui.label("Panel");
})
.width(Size::Fixed(width));
# });
```

- It moves fast at first and slows down at the end (a cubic ease-out).
- The first call returns the target as it is. A panel does not grow from 0 when the screen opens.
- If the target changes while the value is moving, the value goes from where it is to the new target. It does not jump.
- Frames are drawn only while something moves. Once the value reaches its target, drawing stops, so a still window uses no CPU.
- The key tells moving values apart inside the same container (the same idea as `ui.keyed`). In a loop, use the row's index or id as the key.

## Animating things that appear and disappear

`ui.animate_bool(key, on)` returns 0.0 while `on` is `false` and 1.0 while it is `true`, and moves between them over 250 ms when it changes. 250 ms is the normal duration of WinUI's controls (ControlNormalAnimationDuration).

```rust
# let mut details = true;
# kargui::test::example(|ui| {
let shown = ui.animate_bool("details", details);
if shown > 0.0 {
    // appears from the top when details turns true, and goes back up when it turns false
    ui.column(|ui| {
        ui.label("Created 2026-11-22").weak();
        ui.label("Changed 2026-11-23").weak();
    })
    .reveal(shown);
}
# });
```

`.reveal(amount)` shows only the top `amount` (0.0 to 1.0) of a widget's height and cuts off the rest. In its parent it takes only the height it shows, so the widgets below move with it. The widgets inside keep their own sizes, so text is not squeezed and does not wrap differently. Widgets in the part that is cut off cannot be clicked, and Tab skips those that are cut off entirely. Growing a widget with `.height(Size::Fixed(…))` instead does not cut anything off, so its content overlaps the widgets below while it grows.

Call `animate` and `animate_bool` on every frame, whether the thing is shown or not. A value that is not called in a frame is dropped at the end of that frame. If `animate_bool` in the example above were inside `if details { … }`, the hiding motion would not show: the next call counts as a first call and starts at the target.

kargui's own widgets move the same way. The thumb of a switch (`ui.toggle`) slides to the other end in 167 ms (WinUI's fast duration). The content of an expander (`ui.expander`) slides in and out from the top in 250 ms.

## When the user turns motion off

For people who turned motion off in the system, `animate` and `animate_bool` return the target at once. On Windows this is Settings > Accessibility > Visual effects > Animation effects. Motion in kargui's own widgets stops the same way.

- The setting is read when a window is created. If it changes while the app is open, it is read again when the system says so (`WM_SETTINGCHANGE`).
- For motion you make yourself without `animate` (drawing something over time on a canvas, say), check `ui.reduce_motion()` and do not move things when it is `true`.
- Motion on screen makes some people feel sick. WCAG 2.1 success criterion 2.3.3 (Animation from Interactions, AAA) asks that motion which is not essential can be turned off.

## Tests

The test harness starts with motion turned off. The screen after an action is already its end state without moving the clock, so a test can check that an expander's content is gone right after closing it.

To test motion itself, turn it on with `Harness::reduce_motion(false)`. The harness's clock moves only when you call `Harness::wait(time)`, so values in the middle of a motion can be checked as exact numbers.

```rust
use std::time::Duration;
use kargui::{App, Ui};
use kargui::test::Harness;

struct Drawer {
    open: bool,
    shown: f32,
}

impl App for Drawer {
    fn view(&mut self, ui: &mut Ui) {
        if ui.button("Open").clicked() {
            self.open = true;
        }
        self.shown = ui.animate_bool("drawer", self.open);
    }
}

// motion is off at first, so a click opens it all the way at once
let mut h = Harness::new(Drawer { open: false, shown: 0.0 });
h.click("Open");
assert_eq!(h.app().shown, 1.0);

// with motion on, it moves as far as the clock is moved
let mut h = Harness::new(Drawer { open: false, shown: 0.0 });
h.reduce_motion(false).click("Open");
assert_eq!(h.app().shown, 0.0, "the frame of the click is where it starts");
h.wait(Duration::from_millis(125));
assert!(h.app().shown > 0.5, "slowing down, so past half in half the time");
h.wait(Duration::from_millis(125));
assert_eq!(h.app().shown, 1.0);
```
