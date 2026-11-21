<!-- covers: Ui::text_input TextInput::submitted TextInput::changed TextInput::composing TextInput::hint sig=e9d6eeca8077cac8 -->
# 日本語入力

kargui の入力欄は、Windows の日本語入力（IME）を最初から扱います。アプリが気をつけることはほとんどありません。

## 決まり

1. 変換を確定する Enter では、`submitted()` になりません。確定した後の Enter で 1 回だけ `submitted()` になります。
2. 変換中の文字は入力欄の中に下線付きで出ますが、値（`&mut String`）は確定した時に変わります。そのため、絞り込みのような処理は確定するまで走りません。
3. 変換中の文節には下線を引き、今変換している文節は太くします。候補の窓は、打っている文字の真下に出ます。
4. 変換の途中で別の部品を押しても、変換中の文字は元の入力欄に確定されます。消えません。
5. Backspace と左右の矢印は、見た目の 1 文字（絵文字の並びなども 1 文字）ずつ動きます。
6. 文字を選んで打ち始めると、選んだ文字が変換中の文字に置き換わります。

```rust
# let mut query = String::new();
# let mut hits = 0;
# kargui::test::example(|ui| {
let field = ui.text_input(&mut query).hint("絞り込み");
if field.changed() {
    // 確定した時だけ来る。変換中は来ない
    hits = query.len();
}
if let Some(text) = field.composing() {
    // 変換中の文字が要る時だけ読む（候補を先に出したい時など）
    let _ = text;
}
# });
```

## 試験

`kargui::test::Harness` で、変換中と確定をまねられます（[試験の書き方](09-testing.md)）。

```rust
# use kargui::{App, Ui};
# #[derive(Default)]
# struct Notes { input: String, notes: Vec<String> }
# impl App for Notes {
#     fn view(&mut self, ui: &mut Ui) {
#         let field = ui.text_input(&mut self.input).hint("メモ");
#         if field.submitted() { self.notes.push(std::mem::take(&mut self.input)); }
#     }
# }
let mut h = kargui::test::Harness::new(Notes::default());
h.click("メモ").ime_compose("ぎゅうにゅう");
assert_eq!(h.app().input, ""); // 変換中は値が変わらない
h.ime_commit("牛乳").press(kargui::Key::Enter);
assert_eq!(h.app().notes, ["牛乳"]);
```

再変換（確定した文字を選んで変換し直す）にも答えますが、IME と設定によって動きが違います。手元の Google 日本語入力では、まだ確かめが終わっていません。
