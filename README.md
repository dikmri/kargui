# kargui

A light native GUI framework for Rust (Windows 10/11; version 0.2). The name comes from
the Japanese word *karui* (light), Rust and GUI. [日本語の README](README.ja.md) (the original; this is
a translation).

kargui grew out of an experiment that built the same app (a task book) with Tauri, egui and GPUI and
measured them ([report, in Japanese](https://dikmri.github.io/ai-coding-notes/reports/rust-gui/)). Each
of the three does something well: egui is easy to write, GPUI uses little memory, and Tauri draws
smoothly even on a PC without a GPU. kargui aims to bring those together, and it is measured with the
same tools and conditions as it grows. How it was built is written up in
[a second report (in Japanese)](https://dikmri.github.io/ai-coding-notes/reports/kargui/). kargui
learned a lot from the three frameworks and from their authors and contributors.

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
cargo run --release -p taskbook-kargui -- --items 10000   # the task book (same app as the comparison)
cargo run --release -p settings-kargui                    # a settings screen
cargo run --release -p reader-kargui                      # a long-text reader
cargo run --release -p table-kargui                       # a 10,000-row table
cargo run --release -p notepad-kargui                     # a text editor (menus, file dialogs, asking before closing)
cargo run --release -p expenses-kargui                    # a household budget (form, table, tabs, chart)
cargo run --release -p files-kargui                       # a file browser (tree, split panes, another window)
cargo run --release -p board-kargui                       # a card board (drag and drop, with a "Move" menu too)
```

Each app writes one line per event to `logs/<app>.log` (time, event, arguments, result).

```text
2026-11-21T12:17:48.204Z t_ms=1795263468204 app=kargui-table event=ready args="app=table items=10000" result="since_main_ms=34"
```

## Documentation

- Guide: [English](docs/guide/en/README.md) (translation), [日本語](docs/guide/ja/README.md) (original)
- API: `cargo doc -p kargui --open`
- For AI: [docs/llms.txt](docs/llms.txt)
- Working on kargui: [AGENTS.md](AGENTS.md)

## Checks

```powershell
cargo test --workspace --exclude kargui-spike   # unit tests and the examples in the docs, guide and README
cargo run -p xtask -- check                      # doc and example coverage, widget pictures, the AI index, guide freshness
```

The input method checks type into the foreground window, so they run only while the author is away
(`tools/away-run.ps1`).

## Status and next steps

0.1 was the first release. 0.2 adds the widgets and mechanisms most everyday desktop apps need
(changes and how to upgrade: [CHANGELOG.md](CHANGELOG.md)). As a check, three ordinary apps were
written with kargui alone: a text editor, a household budget and a file browser (`examples/notepad`,
`examples/expenses`, `examples/files`); what they showed was fixed in kargui. Next: the items below,
and windows on Linux and macOS. Each addition is measured with the same tools to keep kargui light.

Not there yet:

- **Widgets:** rich text (disabled widgets, popups, in-window dialogs, tooltips,
  icons, images, multi-line text input, radio buttons, progress, cards and groups, expanders, links,
  passwords, drop-downs, menus (menu bar, menu buttons, submenus and context menus), tabs, number
  inputs, toolbars, grids, split panes, drawing surfaces, trees, tables with headings and date
  pickers were added in 0.2; images are not drawn on the Direct3D 11 renderer yet).
- **App mechanisms:** a tray icon and system notifications (shortcuts, timers, waking from other
  threads, multiple windows, open and save file dialogs, the window icon, a minimum window size and
  asking before closing were added in 0.2; drag and drop inside the window, dropping files from
  other apps, animation helpers, choosing fonts, and color and time pickers are on the main branch).
- **Platforms:** windows open on Windows 10/11 only. The Direct3D 11 renderer (not the default)
  occasionally stalls for a long frame. Screen reading has been checked with Narrator but not with
  NVDA. Apps cannot start a reconversion themselves.

### Linux and macOS

They are planned. Everything that depends on the operating system (window, input, input method,
text, putting pixels on screen, clipboard, screen-reader adapter) is kept in five files:
`platform.rs`, `gdi.rs`, `gpu.rs`, `text/dwrite.rs` and `wic.rs` (reading image files). The rest (widgets, layout, drawing, the
test harness) builds on Linux and macOS as well, with text laid out and drawn by parley and swash.
On Linux, building needs the fontconfig development files (`libfontconfig1-dev` and `pkg-config` on
Debian and Ubuntu); fontique, inside parley, uses fontconfig to find the system fonts.
Every push runs the tests on Windows, Linux and macOS (GitHub Actions). Apps cannot open windows
there yet, so `kargui::run` returns an `Err` saying the system is not supported. Linux comes first.

The API may change during 0.x; changes are listed in [CHANGELOG.md](CHANGELOG.md).

## License

MIT ([LICENSE](LICENSE)).
