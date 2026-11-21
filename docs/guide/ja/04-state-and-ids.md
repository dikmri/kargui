<!-- covers: Ui::keyed List::show_keyed Ui::request_frame App::presented Handle::id sig=eb58365da527b924 -->
# 状態と id

## 状態はアプリが持つ

kargui はアプリの状態を持ちません。入力欄の文字・チェックの入り切り・選んだ行は、すべて自分の構造体に置きます。
`view` で部品に渡すと、部品がそれを読み、操作されたら書き換えます。

kargui が持つのは、キャレットの位置・スクロールの位置・焦点のような、画面のための小さな状態だけです。

## 部品の id

部品の状態は、部品の id ごとに覚えます。id は、部品を作ったソースの場所（ファイル・行・列）と、同じ場所で何回目に作ったかで決まります。
そのため、ふつうは id を気にする必要がありません。

ループで同じ部品を作り、項目が足されたり消えたりする時は、項目の id で区別してください。
そうしないと、先頭に項目を足した時に、焦点やキャレットが別の項目へずれます。

```rust
# struct Task { id: u32, text: String }
# let tasks = vec![Task { id: 7, text: "牛乳".into() }, Task { id: 3, text: "パン".into() }];
# kargui::test::example(|ui| {
for task in &tasks {
    ui.keyed(task.id, |ui| {
        ui.label(&task.text);
    });
}
# });
```

一覧（`ui.list`）では `show_keyed` を使います（[一覧と表](05-lists-and-tables.md)）。

## 描き直しを頼む

入力が無いと `view` は呼ばれません。時間で変わる物（時計やアニメーション）を出す時は、`ui.request_frame()` で次のフレームを頼みます。

```rust
# kargui::test::example(|ui| {
let animating = true;
if animating {
    ui.request_frame();
}
# });
```

## 画面に出た後

`App::presented` は、フレームが画面に出た後に呼ばれます。最初の呼び出しは「起動して表示できた」時なので、起動の時間を測るのに使えます。
