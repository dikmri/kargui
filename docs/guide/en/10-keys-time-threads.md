<!-- source: ja/10-keys-time-threads.md fnv=90bc91ebf0947e89 -->
<!-- covers: Ui::shortcut Ui::now Ui::request_frame_after Ui::waker Waker::wake Harness::hover Harness::wait sig=1dc37a39551057c3 -->
# Keys, time and other threads

kargui sleeps while nothing changes and uses no CPU. Besides input, it runs `view` when the app receives a key, when a set time has passed, and when another thread wakes it.

## Shortcuts

Keys that no widget used reach the app through `ui.shortcut(key, mods)`. Combine `Mods::CTRL`, `Mods::SHIFT`, `Mods::ALT` and `Mods::NONE` with `|`.

```rust
use kargui::{Key, Mods};
# let (mut saved, mut saved_as) = (0, 0);
# kargui::test::example(|ui| {
if ui.shortcut(Key::S, Mods::CTRL) {
    saved += 1;
}
if ui.shortcut(Key::S, Mods::CTRL | Mods::SHIFT) {
    saved_as += 1;
}
# });
```

The focused widget gets the keys it uses first. A text field uses letters, digits, Enter, the deleting keys and Ctrl+A/C/V/X/Y/Z; a slider and a selectable list use the arrows. Everything else (Ctrl+S, F5, Escape when there is nothing to close, ...) reaches the app. Inside `ui.enabled(false, ..)` it is always false.

## Drawing again after some time

`ui.now()` is the time of this frame. `ui.request_frame_after(duration)` asks for `view` to run once that time has passed. Until then, kargui sleeps without drawing.

```rust
use std::time::Duration;
# let mut saved_at: Option<std::time::Instant> = None;
# kargui::test::example(|ui| {
// show "Saved" for 3 seconds
if let Some(at) = saved_at {
    let shown = ui.now().saturating_duration_since(at);
    if shown < Duration::from_secs(3) {
        ui.label("Saved");
        ui.request_frame_after(Duration::from_secs(3) - shown);
    }
}
# });
```

For animations that draw every frame, use `ui.request_frame()`.

## Drawing after work on another thread

Do slow work (loading, computing, networking) on another thread. Send the result through a channel and call `wake()` on the waker from `ui.waker()`. The window wakes up and runs `view`, which reads the channel.

```rust
use std::sync::mpsc::{Receiver, channel};
use kargui::{App, Ui, Waker};

struct Loader {
    rx: Receiver<String>,
    waker: Option<Waker>,
    lines: Vec<String>,
}

impl App for Loader {
    fn view(&mut self, ui: &mut Ui) {
        self.waker.get_or_insert_with(|| ui.waker());
        self.lines.extend(self.rx.try_iter());
        for line in &self.lines {
            ui.label(line);
        }
    }
}

let (tx, rx) = channel();
let mut h = kargui::test::Harness::new(Loader { rx, waker: None, lines: Vec::new() });
let waker = h.app().waker.clone().unwrap();
std::thread::spawn(move || {
    tx.send("Loaded".to_string()).unwrap();
    waker.wake();
})
.join()
.unwrap();
h.frame();
assert!(h.find("Loaded").is_some());
```

A waker can be called from any thread, any number of times. Wakes that arrive before the window reads them are merged.

## Moving time forward in tests

The test harness never really waits. Move time forward with `h.wait(duration)` and put the pointer on a widget with `h.hover("name")`.

```rust
use std::time::Duration;
use kargui::{App, Ui};

struct Bar;

impl App for Bar {
    fn view(&mut self, ui: &mut Ui) {
        ui.button("Save").tooltip("Saves the current content");
    }
}

let mut h = kargui::test::Harness::new(Bar);
h.hover("Save");
assert!(h.find("Saves the current content").is_none());
h.wait(Duration::from_millis(600));
assert!(h.find("Saves the current content").is_some());
```
