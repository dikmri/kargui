<!-- covers: Ui::label Ui::heading Ui::button Ui::checkbox Ui::toggle Ui::dark_mode_switch Ui::segmented Ui::text_input Ui::slider Label::weak Label::small Label::bold Label::size Button::primary Button::small Button::enabled Ui::enabled Ui::popup Ui::dialog Handle::tooltip Ui::icon Ui::icon_button Button::icon Ui::image Image::from_rgba Image::open Toggled::changed Toggled::strike Slider::changed sig=611901922cf6e15f -->
# 部品

部品は `ui` のメソッドで作ります。値を変える部品には `&mut` で状態を渡し、戻り値で何が起きたかを聞きます。

## 文字とボタン

```rust
# kargui::test::example(|ui| {
ui.heading("見出し");
ui.label("ふつうの文字");
ui.label("目立たない文字").weak();
ui.label("小さい文字").small();
ui.label("太い文字").bold();
ui.label("大きさを決めた文字").size(18.0);
if ui.button("保存").primary().clicked() {
    // 保存する
}
ui.button("小さいボタン").small();
# });
```

`primary()` は、画面でいちばん大事なボタン（1 つだけ）に付けます。

## 切り替え

```rust
# let (mut done, mut wifi) = (false, true);
# kargui::test::example(|ui| {
if ui.checkbox(&mut done, "牛乳を買う").strike(done).changed() {
    // done が切り替わった
}
ui.toggle(&mut wifi, "Wi-Fi");
# });
```

`strike(true)` は、終わったタスクのように文字に線を引きます。押せるのは変わりません。

明暗の切り替えには、`ui.dark_mode_switch("ダーク")` を使います。kargui 自身の明暗（初めは OS の設定）を示して切り替えるので、アプリが値を持つ必要がなく、スイッチが見た目と食い違いません。

```rust
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.heading("タスク");
    ui.fill();
    ui.dark_mode_switch("ダーク");
});
# });
```

## いくつかから 1 つ選ぶ

```rust
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Light,
    Dark,
}
# let mut look = Look::Light;
# kargui::test::example(|ui| {
if ui.segmented(&mut look, &[(Look::Light, "ライト"), (Look::Dark, "ダーク")]) {
    // 選んだ物が変わった
}
# });
```

## 文字の入力

```rust
# let mut name = String::new();
# kargui::test::example(|ui| {
let field = ui.text_input(&mut name).hint("名前").grow();
if field.submitted() {
    // Enter が押された（変換を確定する Enter は数えない）
}
# });
```

日本語入力の決まりは [日本語入力](07-japanese-input.md) にあります。

## 数を選ぶ

```rust
# let (mut font_size, mut volume) = (14, 0.5_f32);
# kargui::test::example(|ui| {
ui.slider(&mut font_size, 10..=24).label("文字の大きさ");
if ui.slider(&mut volume, 0.0..=1.0).label("音量").changed() {
    // 音量を変える
}
# });
```

整数は 1 ずつ、小数はポインターで自由に、キーで範囲の 100 分の 1 ずつ動きます。スライダーは文字を持たないので、読み上げの名前を `.label(..)` で付けます。

## 押せなくする

条件がそろうまで使えない部品は、`ui.enabled(条件, |ui| ..)` の中に作ります。中の部品は薄く描かれ、ポインターもキーも受けず、Tab でも止まりません。読み上げソフトには「使用不可」と伝わります。ボタン 1 つだけなら `.enabled(条件)` を付けます。

```rust
# let (mut notify, mut sound, mut volume) = (false, true, 5);
# let name = String::new();
# kargui::test::example(|ui| {
ui.checkbox(&mut notify, "通知する");
ui.enabled(notify, |ui| {
    ui.checkbox(&mut sound, "音を鳴らす");
    ui.slider(&mut volume, 0..=10).label("音量");
});
if ui.button("送る").enabled(!name.is_empty()).clicked() {
    // 送る
}
# });
```

焦点のある部品が押せなくなると、焦点は外れます。`ui.enabled` は行や列を作らないので、中の部品は今の行（列）にそのまま並びます。

## 重ねて出す

ボタンの近くに出す浮き出しは `ui.popup(&mut 開いているか, 出す元の id, |ui| ..)`、窓の真ん中に出す確認の画面は `ui.dialog(&mut 開いているか, 題, |ui| ..)` で作ります。どちらも、開くと中の最初の部品へ焦点が移り、Esc で閉じ、閉じると焦点が開く前の部品へ戻ります。浮き出しは外を押しても閉じます（そのクリックは下の部品に届きません）。ダイアログが出ている間は後ろの部品を使えず、Tab はダイアログの中だけを回ります。

```rust
# let (mut menu, mut confirm) = (false, false);
# kargui::test::example(|ui| {
let more = ui.button("その他");
let anchor = more.id();
if more.clicked() {
    menu = !menu;
}
ui.popup(&mut menu, anchor, |ui| {
    if ui.button("削除").clicked() {
        confirm = true;
    }
});
let mut close = false;
ui.dialog(&mut confirm, "削除しますか？", |ui| {
    ui.label("この操作は元に戻せません。");
    ui.row(|ui| {
        ui.fill();
        close |= ui.button("削除").primary().clicked();
        close |= ui.button("やめる").clicked();
    });
});
if close {
    confirm = false;
}
# });
```

浮き出しは出す元の下に出し、下に入らなければ上に出します。重ねる物は窓の中に描くので、窓の外にははみ出しません。

部品の説明は `.tooltip("説明")` で付けます。ポインターが 0.5 秒止まった時と、キーボードで焦点が来た時に、部品の下に出ます。読み上げソフトには、部品の説明として伝わります。

```rust
# kargui::test::example(|ui| {
ui.button("保存").tooltip("今の内容をファイルに保存します");
# });
```

## アイコンと画像

kargui は、よく使う 18 個のアイコン（`Icon::Check`・`Icon::Close`・`Icon::ChevronDown`・`Icon::Search` など）を持っています。どの大きさでもくっきり描き、色は文字に合わせます。ボタンの文字の前に付ける時は `.icon(..)`、アイコンだけのボタンは `ui.icon_button(アイコン, 名前)` です。アイコンだけのボタンには文字が無いので、名前（読み上げの名前）を必ず渡します。

```rust
use kargui::Icon;
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.button("新しいタスク").icon(Icon::Plus).primary();
    ui.icon_button(Icon::Close, "閉じる");
    ui.icon(Icon::Info).weak();
    ui.label("自動で保存します");
});
# });
```

画像は `Image` に読み込んでから `ui.image(&画像)` で出します。読み込みは時間がかかるので、`view` の中ではなく、アプリの状態を作る時に 1 回だけ行います。大きさは画像の画素数（論理ピクセル）で、`.size(幅, 高さ)` で変えられます。

```rust
use kargui::{App, Image, Ui};

struct Album {
    photo: Image,
}

impl App for Album {
    fn view(&mut self, ui: &mut Ui) {
        ui.image(&self.photo).size(320.0, 240.0).label("夜の事務所");
    }
}

// ファイルからは Image::open("photo.jpg")（Windows）。ここでは画素から作る
let photo = Image::from_rgba(4, 3, vec![120; 48]).unwrap();
let h = kargui::test::Harness::new(Album { photo });
assert!(h.find("夜の事務所").is_some());
```

アイコンも画像も、名前を付けない時は飾りとして扱い、読み上げソフトは読みません。内容を伝える画像には `.label(..)` で説明を付けます。画像のファイルを読めるのは今は Windows だけで（PNG・JPEG・GIF・BMP など）、ほかの OS では `Image::from_rgba` で画素から作ります。GPU の道（既定ではない）では、画像はまだ描きません。
