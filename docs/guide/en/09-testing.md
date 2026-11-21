<!-- source: ja/09-testing.md fnv=f2083561d9048ae4 -->
<!-- covers: test::Harness Harness::new Harness::with_size Harness::click Harness::type_text Harness::press Harness::ime_compose Harness::ime_commit Harness::find Harness::find_all Harness::names Harness::render Harness::app Harness::app_mut Harness::frame sig=cd306a0d6f61ebbd -->
# Writing tests

`kargui::test::Harness` runs your app without a window. Imitate input, then check the result.
Widgets are found by their screen-reader names, so writing tests also checks that every widget has a name.

```rust
use kargui::test::Harness;
use kargui::{App, Key, Ui};

#[derive(Default)]
struct Todo {
    input: String,
    items: Vec<String>,
}

impl App for Todo {
    fn view(&mut self, ui: &mut Ui) {
        ui.row(|ui| {
            let field = ui.text_input(&mut self.input).hint("New item").grow();
            if field.submitted() || ui.button("Add").clicked() {
                self.items.push(std::mem::take(&mut self.input));
            }
        });
        for item in &self.items {
            ui.label(item);
        }
    }
}

let mut h = Harness::new(Todo::default());
h.click("New item").type_text("Milk").press(Key::Enter);
assert_eq!(h.app().items, ["Milk"]);
assert!(h.names().contains(&"Milk".to_string()));
```

## Common actions

| Action | Method |
|---|---|
| Click | `click("name")` |
| Type text | `type_text("abc")` |
| Keys | `press(Key::Tab)`; with modifiers `press_with(Key::A, Mods { ctrl: true, ..Default::default() })` |
| Input method | `ime_compose("composing")`, `ime_commit("committed")` |
| Wheel | `scroll("name", 96.0)` |
| Positions | `find("name")`; when several widgets share a name, `find_all("name")` |
| Image | `render()` returns the screen drawn by the CPU renderer; save it with `save_png(..)` |
| State | `app()`, `app_mut()`. After changing the state directly, redraw with `frame()` |

Set the window size and scale with `Harness::with_size(app, width, height, scale)`.

```rust
# use kargui::{App, Ui};
# struct Hello;
# impl App for Hello { fn view(&mut self, ui: &mut Ui) { ui.heading("Hello"); } }
let mut h = kargui::test::Harness::with_size(Hello, 320.0, 200.0, 1.5);
let image = h.render();
assert_eq!((image.width, image.height), (480, 300));
```
