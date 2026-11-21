# kargui

A light native GUI framework for Rust (Windows 10/11, work in progress). The name comes from the
Japanese word *karui* (light), Rust and GUI. [日本語の README](README.ja.md) (the original; this is a translation).

kargui started from an experiment that built the same app (a task book) with Tauri, egui and GPUI and
measured them ([report, in Japanese](https://dikmri.github.io/ai-coding-notes/reports/rust-gui/)). It aims
to keep their strengths and drop their weaknesses, and it is measured with the same tools and
conditions, aiming to be 30% better than the best of the three on every axis.

## Features

- **Immediate-mode code.** Keep your state in your own struct and describe the whole screen in
  `view` every time (as easy to write as egui).
- **Redraws only what changed.** kargui compares with the previous screen and, on the default CPU
  renderer, redraws only the changed parts. It uses no CPU while idle and is fast without a GPU.
- **Input methods (IME).** The Enter that commits a composition does not submit, the value does not
  change while composing, each clause is underlined, the candidate window appears right below the
  text, and moving focus never loses the composed text.
- **Screen readers and the keyboard.** Every widget is given to UI Automation through AccessKit;
  widgets without a name are reported in the log; Tab reaches everything.
- **Documentation that stays current.** Every public item has a doc comment and a runnable example,
  the guide's code runs as tests, and the checks fail when the API changes without the guide being
  updated.

## Widgets

Heading, label (wrapping, size, right alignment), button, check box, switch, dark mode switch,
segmented choice, text input, slider, list (builds only the visible rows, row selection, rows keyed by
your ids) and scroll container. Layout: row, column, stack, and sharing the remaining space (two-pass
layout). Pictures: [docs/widgets.md](docs/widgets.md).

```rust,no_run
use kargui::{App, Options, Ui};

struct Counter {
    count: u32,
}

impl App for Counter {
    fn view(&mut self, ui: &mut Ui) {
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

## Measured (task book, same tools and PC as the earlier comparison)

| Axis | kargui (CPU renderer) | Best of Tauri, egui, GPUI | Target |
|---|---|---|---|
| Startup (empty list) | 42 ms | 344 ms (egui, glow) | 330 ms |
| Memory (10,000 items, idle) | 5.4 MB | 17.9 MB (GPUI) | 12.5 MB |
| CPU per frame while scrolling (measuring tool) | 0.36 ms | 1.97 ms (egui) | 1.2 ms |
| Idle CPU | 0 | 0 (egui) | 0.16% |
| Executable size | 1.0 MB | 6.7 MB (egui) | 4.5 MB |
| Task book UI code | 198 lines | 316 lines (egui) | 220 lines |

The measuring tool counts CPU time in 15.6 ms ticks, so per-frame values of about 1 ms vary between
runs; kargui's own timer reports about 0.58 ms of work per scrolling frame.

## Run the examples

```powershell
cargo run --release -p taskbook-kargui -- --items 10000
cargo run --release -p settings-kargui
cargo run --release -p reader-kargui
cargo run --release -p table-kargui
```

## Documentation

- Guide: [English](docs/guide/en/README.md) (translation), [日本語](docs/guide/ja/README.md) (original)
- API: `cargo doc -p kargui --open`
- For AI: [docs/llms.txt](docs/llms.txt)
- Working on kargui: [AGENTS.md](AGENTS.md)

## License

MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
