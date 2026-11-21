<!-- covers: test::Harness Harness::new Harness::with_size Harness::click Harness::type_text Harness::press Harness::ime_compose Harness::ime_commit Harness::find Harness::find_all Harness::names Harness::render Harness::app Harness::app_mut Harness::frame sig=cd306a0d6f61ebbd -->
# 試験の書き方

`kargui::test::Harness` は、窓を開かずにアプリを動かします。入力をまねて、結果を確かめます。
部品は読み上げの名前で探すので、試験を書くと、すべての部品に名前があることも確かめられます。

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
            let field = ui.text_input(&mut self.input).hint("新しい項目").grow();
            if field.submitted() || ui.button("追加").clicked() {
                self.items.push(std::mem::take(&mut self.input));
            }
        });
        for item in &self.items {
            ui.label(item);
        }
    }
}

let mut h = Harness::new(Todo::default());
h.click("新しい項目").type_text("牛乳").press(Key::Enter);
assert_eq!(h.app().items, ["牛乳"]);
assert!(h.names().contains(&"牛乳".to_string()));
```

## よく使う操作

| 操作 | メソッド |
|---|---|
| 押す | `click("名前")` |
| 文字を打つ | `type_text("abc")` |
| キー | `press(Key::Tab)`、修飾キー付きは `press_with(Key::A, Mods { ctrl: true, ..Default::default() })` |
| 日本語入力 | `ime_compose("へんかんちゅう")`、`ime_commit("確定")` |
| ホイール | `scroll("名前", 96.0)` |
| 場所 | `find("名前")`、同じ名前が何個もある時は `find_all("名前")` |
| 絵 | `render()` で CPU で描いた絵を返す。`save_png(..)` で保存できる |
| 状態 | `app()`、`app_mut()`。状態を直接変えたら `frame()` で描き直す |

窓の大きさと拡大率は `Harness::with_size(app, 幅, 高さ, 拡大率)` で決めます。

```rust
# use kargui::{App, Ui};
# struct Hello;
# impl App for Hello { fn view(&mut self, ui: &mut Ui) { ui.heading("こんにちは"); } }
let mut h = kargui::test::Harness::with_size(Hello, 320.0, 200.0, 1.5);
let image = h.render();
assert_eq!((image.width, image.height), (480, 300));
```
