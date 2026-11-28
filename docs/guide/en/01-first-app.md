<!-- source: ja/01-first-app.md fnv=1d91396b47316ec7 -->
<!-- covers: App Options run Ui::heading Ui::label Ui::button Ui::row sig=c5a08b17950f69d8 -->
# Your first app

A kargui app is two things: your own struct that holds the state, and `view`, which describes its screen.

## Setup

Add kargui to `Cargo.toml` (or run `cargo add kargui`).

```toml
[dependencies]
kargui = "0.5"
```

For what is newer than the crates.io release, write `kargui = { git = "https://github.com/dikmri/kargui" }`; for a kargui on your disk, write `kargui = { path = "../kargui/crates/kargui" }`. Building on Linux needs the fontconfig development files (`libfontconfig1-dev` and `pkg-config` on Debian and Ubuntu).

## A counter

```rust,no_run
use kargui::{App, Options, Ui};

struct Counter {
    count: u32,
}

impl App for Counter {
    fn view(&mut self, ui: &mut Ui) {
        ui.heading("Counter");
        ui.row(|ui| {
            ui.label(format!("Count: {}", self.count));
            if ui.button("Add 1").clicked() {
                self.count += 1;
            }
        });
    }
}

fn main() {
    kargui::run(Options::new("Counter"), Counter { count: 0 }).unwrap();
}
```

## How it works

- kargui calls `view` when there is input or the window size changes. While nothing happens it does not call it and uses no CPU.
- `view` describes the whole screen from the top every time. You do not need to remember the previous screen.
- Keep the state (here `count`) in your own struct. Pass it to widgets with `&mut`, or read it to show it.
- Answers such as `clicked()` are decided with the widget positions from the previous frame, so you can use them in an `if` right away in the same `view`.
- Drawing uses the CPU by default and only redraws what changed since the previous screen, so it is fast even on PCs without a GPU.

## Window settings

`Options` sets the window's title, size, position and light or dark look.

```rust,no_run
# use kargui::{App, Options, Ui};
# struct Counter { count: u32 }
# impl App for Counter { fn view(&mut self, _ui: &mut Ui) {} }
let mut options = Options::new("Counter");
options.size = (480.0, 320.0); // logical pixels
options.dark = Some(true); // None follows the operating system
kargui::run(options, Counter { count: 0 }).unwrap();
```
