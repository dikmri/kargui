<!-- covers: Ui::file_dialog Ui::chosen_files Ui::chosen_file Ui::file_dialog_cancelled FileDialog::open FileDialog::save FileDialog::multiple FileDialog::title FileDialog::filter FileDialog::file_name FileDialog::folder FileDialog::is_save FileDialog::allows_multiple App::close_requested Ui::quit Harness::file_dialog_key Harness::file_dialog Harness::choose_files Harness::cancel_file_dialog Harness::request_close Harness::is_closed sig=7e9d34a5ac6cda9a -->
# アプリの仕組み

## ファイルを開く・保存するダイアログ

システムのファイルのダイアログを出します。

```rust
use kargui::FileDialog;
# let mut text = String::new();
# kargui::test::example(|ui| {
if ui.button("開く").clicked() {
    ui.file_dialog("open", FileDialog::open().title("メモを開く").filter("テキスト", &["txt", "md"]));
}
if let Some(path) = ui.chosen_file("open") {
    text = std::fs::read_to_string(path).unwrap_or_default();
}
if ui.button("名前を付けて保存").clicked() {
    ui.file_dialog("save", FileDialog::save().file_name("メモ.txt").filter("テキスト", &["txt"]));
}
if let Some(path) = ui.chosen_file("save") {
    let _ = std::fs::write(path, &text);
}
# });
```

`ui.file_dialog(キー, ..)` で頼むと、そのフレームを描いた後でダイアログを出します。選んだファイルは、後のフレームで `ui.chosen_file(キー)` から 1 回だけ受け取れます。利用者が取り消した時は、次のフレームで `ui.file_dialog_cancelled(キー)` が 1 回だけ true を返します。ダイアログの間も、アプリは止まらずに描き続けます。

- `FileDialog::open()` は開く、`FileDialog::save()` は保存する（同じ名前のファイルがあれば、置き換えてよいかを聞く）
- `.filter(名前, &[拡張子])` で選べる種類、`.title(..)` で題、`.file_name(..)` で始めの名前、`.folder(..)` で始めのフォルダー
- `.multiple()` で複数のファイルを選べるようにし、`ui.chosen_files(キー)` で全部を受け取る

Linux では、xdg-desktop-portal のダイアログ（デスクトップのファイルを選ぶ画面）が開きます。macOS では、まだ何も起きません。

## 閉じる前の確認

保存していない変更がある時に、閉じる前に聞けます。

```rust
use kargui::{App, Ui};

#[derive(Default)]
struct Notes {
    unsaved: bool,
    asking: bool,
}

impl App for Notes {
    fn view(&mut self, ui: &mut Ui) {
        let mut discard = false;
        ui.dialog(&mut self.asking, "変更を捨てますか？", |ui| {
            discard = ui.button("捨てて閉じる").clicked();
        });
        if discard {
            ui.quit();
        }
    }

    fn close_requested(&mut self) -> bool {
        self.asking = self.unsaved;
        !self.unsaved
    }
}
# let mut h = kargui::test::Harness::new(Notes { unsaved: true, asking: false });
# h.request_close();
# assert!(!h.is_closed());
```

利用者が窓の閉じるボタン（か Alt+F4）を押すと、アプリの `close_requested` が呼ばれます。true を返すと閉じ、false を返すと閉じません。聞いた後で閉じる時は `ui.quit()` を呼びます。`ui.quit()` は、どこからでもアプリを終えます。

## 窓のアイコンと最小の大きさ

```rust
use kargui::{Image, Options};
# let png = Image::from_rgba(2, 2, vec![255; 16]).unwrap();
let mut options = Options::new("メモ");
options.icon = Some(png);
options.min_size = Some((360.0, 240.0));
# let _ = options;
```

`icon` は題の帯とタスクバーのアイコンです。ほかの窓は、自分のアイコンが無ければ主の窓のアイコンを使います。`min_size` は、利用者が小さくできる中の大きさの下限（論理ピクセル）です。

## 題の帯に部品を置く

`options.custom_title_bar = true` の窓では、Windows でも kargui が題の帯を描きます。ビューの `ui.title_bar` で、帯の中にメニュー・検索・タブなどを置けます。

```rust
use kargui::{App, Options, Ui};

struct Notes {
    search: String,
}

impl App for Notes {
    fn view(&mut self, ui: &mut Ui) {
        ui.title_bar(|ui| {
            ui.label("メモ").bold();
            ui.text_input(&mut self.search).hint("探す");
        });
        ui.label("本文");
    }
}

let mut options = Options::new("メモ");
options.custom_title_bar = true;
# let _ = (options, Notes { search: String::new() });
```

- kargui は、帯の右端に最小化・最大化・閉じるのボタンを置きます。Windows 11 では、最大化のボタンに乗るとスナップレイアウト（窓の並べ方の候補）が出ます。
- 帯の何も無い所（押せない文字の上を含む）は、OS の題の帯と同じく、引っ張ると窓が動き、2 回押すと最大化し、右クリックで窓のメニューが出ます。画面の端へ寄せる操作も使えます。
- `ui.title_bar` に何も入れなければ、帯の左に窓の題を出します。入れた時は題を出さないので、出したい時はアプリが `ui.label` で置いてください。
- GNOME（合成が題の帯を描かない Wayland）では、`custom_title_bar` にかかわらず kargui が帯を描き、`ui.title_bar` の部品を帯に入れます。
- OS や合成が帯を描く所（X11、帯を描く Wayland、帯の無い合成）と、`custom_title_bar` が false の Windows では、`ui.title_bar` の部品は窓のいちばん上の行になります。どの OS でも使えるように、`ui.title_bar` はビューの最初に呼んでください。

## 試験

試験台では、`h.file_dialog_key()` と `h.file_dialog()` で頼まれたダイアログを読み、`h.choose_files(&[..])` か `h.cancel_file_dialog()` で答えます。`h.request_close()` は閉じるボタンを押したことにし、`h.is_closed()` で閉じたかを確かめます。
