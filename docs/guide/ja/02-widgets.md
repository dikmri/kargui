<!-- covers: Ui::label Ui::heading Ui::button Ui::checkbox Ui::toggle Ui::segmented Ui::text_input Ui::slider Label::weak Label::small Label::bold Label::size Button::primary Button::small Toggled::changed Toggled::strike Slider::changed sig=347ae262fe4371ee -->
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
