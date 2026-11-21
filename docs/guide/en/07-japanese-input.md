<!-- source: ja/07-japanese-input.md fnv=bf7e6e11cab4fe7f -->
<!-- covers: Ui::text_input TextInput::submitted TextInput::changed TextInput::composing TextInput::hint sig=e9d6eeca8077cac8 -->
# Japanese input (IME)

kargui's text fields handle the Windows input methods (IME) for Japanese and other languages from the start. There is very little an app needs to take care of.

## The rules

1. The Enter that commits a composition does not make `submitted()` true. The next Enter, after committing, makes it true once.
2. The text being composed is shown underlined inside the field, but the value (`&mut String`) changes only when it is committed. So work such as filtering does not run until the user commits.
3. Each clause being composed is underlined, and the clause being converted gets a thick underline. The candidate window appears right below the text being typed.
4. If the user clicks another widget in the middle of composing, the composed text is committed to the field it was typed in. It is not lost.
5. Backspace and the left and right arrows move by what looks like one character (an emoji sequence counts as one).
6. Starting to type over selected text replaces the selection with the composed text.

```rust
# let mut query = String::new();
# let mut hits = 0;
# kargui::test::example(|ui| {
let field = ui.text_input(&mut query).hint("Filter");
if field.changed() {
    // only on commit, never while composing
    hits = query.len();
}
if let Some(text) = field.composing() {
    // read the text being composed only when you need it (for example to suggest early)
    let _ = text;
}
# });
```

## Testing

`kargui::test::Harness` can imitate composing and committing (see [Writing tests](09-testing.md)).

```rust
# use kargui::{App, Ui};
# #[derive(Default)]
# struct Notes { input: String, notes: Vec<String> }
# impl App for Notes {
#     fn view(&mut self, ui: &mut Ui) {
#         let field = ui.text_input(&mut self.input).hint("Note");
#         if field.submitted() { self.notes.push(std::mem::take(&mut self.input)); }
#     }
# }
let mut h = kargui::test::Harness::new(Notes::default());
h.click("Note").ime_compose("ぎゅうにゅう");
assert_eq!(h.app().input, ""); // composing does not change the value
h.ime_commit("牛乳").press(kargui::Key::Enter);
assert_eq!(h.app().notes, ["牛乳"]);
```

kargui also answers reconversion requests (selecting committed text and converting it again), but the behavior depends on the input method and its settings. With Google Japanese Input on the author's PC, this has not been confirmed yet.
