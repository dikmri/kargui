<!-- covers: Ui::row Ui::column Ui::stack Ui::fill Ui::space Handle::width Handle::height Handle::grow Handle::gap Handle::padding Handle::align Size Align Ui::set_padding Ui::set_gap sig=6ec8a790ae06df48 -->
# 並べ方

画面の一番外は、上から下へ並ぶ列です。横に並べたい時は `row`、縦に並べたい時は `column` を使います。

```rust
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.heading("タスク");
    ui.fill(); // 残りの幅を取り、後ろの部品を右端へ寄せる
    ui.button("設定");
});
ui.column(|ui| {
    ui.label("1 行目");
    ui.label("2 行目");
});
# });
```

## 大きさ

大きさは `Size` で決めます。

- `Size::Auto`: 中身の大きさ（既定）
- `Size::Fixed(120.0)`: 決まった長さ（論理ピクセル）
- `Size::Fill(1.0)`: 残りを重みで分け合う

`grow()` は「並ぶ向きの残りを取る」の省略です。行の中なら幅、列の中なら高さを取ります。

```rust
# use kargui::Size;
# let mut query = String::new();
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.label("名前").width(Size::Fixed(100.0));
    ui.text_input(&mut query).grow();
});
# });
```

## すき間と余白

```rust
# use kargui::Align;
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.button("はい");
    ui.button("いいえ");
})
.gap(16.0) // 子の間
.padding(12.0, 8.0) // 左右と上下の余白
.align(Align::Start); // 行の中で縦に上へそろえる
# });
```

画面全体の余白と間は `ui.set_padding(x, y)` と `ui.set_gap(..)` で変えます。

## 重ねる

`stack` は子を同じ場所に重ねます。真ん中にそろえるのが既定です。

```rust
# kargui::test::example(|ui| {
ui.stack(|ui| {
    ui.label("下");
    ui.label("上");
});
# });
```

## 2 回で決まる配置

kargui は、まず下から大きさを測り、次に上から場所を決めます。大きさの分からない物の真ん中寄せや、残りの幅を分けることが、1 フレームで正しく決まります。
