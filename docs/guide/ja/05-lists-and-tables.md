<!-- covers: Ui::list List::show List::show_keyed List::selection Ui::fill List::grow List::separators List::scroll_to List::scroll_offset Label::right sig=3b692e2a30af5317 -->
# 一覧と表

`ui.list(件数, 行の高さ)` は、見えている行だけを作る一覧です。1 万件でも 100 万件でも、1 フレームで作るのは見えている 20 行ほどです。
行の高さはどの行も同じです。

```rust
# let items: Vec<String> = (0..10_000).map(|i| format!("項目 {i}")).collect();
# kargui::test::example(|ui| {
ui.list(items.len(), 32.0).grow().show(|ui, i| {
    ui.label(&items[i]);
});
# });
```

利用者は、ホイール・スクロールの棒（つかむ、空いた所を押して 1 ページ）・Tab でスクロールできます。

## 行を項目の id で区別する

`show` は行を「上から何行目か」で区別します。項目が足されたり消えたりするなら、`show_keyed` で項目の id を渡してください。
焦点やチェックの状態が項目についていき、読み上げソフトにも全部の行が変わったようには見えません。

```rust
# struct Task { id: u32, text: String, done: bool }
# let mut tasks = vec![Task { id: 7, text: "牛乳".into(), done: false }];
# kargui::test::example(|ui| {
let ids: Vec<u32> = tasks.iter().map(|t| t.id).collect();
ui.list(tasks.len(), 32.0).grow().show_keyed(|i| ids[i], |ui, i| {
    let t = &mut tasks[i];
    ui.checkbox(&mut t.done, t.text.clone());
});
# });
```

## 行の中の並べ方と、データを変える所

一覧の行は、もう横の並びです。部品をそのまま並べ、`ui.fill()` でボタンを右端へ寄せます。
行の関数の中では、データを変えません。変えると、後ろの行が古い番号でデータを読みます。押された物を覚えておき、一覧の後で変えます。

```rust
# struct Task { id: u32, text: String, done: bool }
# let mut tasks = vec![Task { id: 7, text: "牛乳".into(), done: false }];
# kargui::test::example(|ui| {
let (mut toggled, mut removed) = (None, None);
ui.list(tasks.len(), 32.0).grow().show_keyed(|i| tasks[i].id, |ui, i| {
    let task = &tasks[i];
    let mut done = task.done;
    if ui.checkbox(&mut done, task.text.as_str()).strike(task.done).changed() {
        toggled = Some(task.id);
    }
    ui.fill();
    if ui.button("削除").small().clicked() {
        removed = Some(task.id);
    }
});
if let Some(id) = toggled {
    if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
        t.done = !t.done;
    }
}
if let Some(id) = removed {
    tasks.retain(|t| t.id != id);
}
# });
```

## 行を選ぶ

`selection` を付けると、行を押して選べます。↑↓ で選ぶ行が動き、Home と End で端へ移ります。選んだ行は見えるようにスクロールします。
一覧全体で、Tab で止まる所は 1 つです。

```rust
# let files = vec!["a.txt", "b.txt", "c.txt"];
# let mut selected: Option<usize> = None;
# kargui::test::example(|ui| {
ui.list(files.len(), 28.0).grow().selection(&mut selected).show(|ui, i| {
    ui.label(files[i]);
});
# });
```

並べ替えや絞り込みで行の順番が変わるアプリでは、選んだ項目を id で覚え、行の番号は毎回探し直します（`examples/table` を参照）。

## 表にする

行の中に、幅を決めたラベルを並べると表になります。数字は `right()` で右にそろえます。
一覧の行は左右に 12 の余白を持つので、上に置く見出しの行も `padding(12.0, 0.0)` にすると列がそろいます。

```rust
# use kargui::Size;
# struct Item { name: String, price: u32 }
# let items = vec![Item { name: "ノート".into(), price: 280 }];
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.label("名前").bold().grow();
    ui.label("値段").bold().right().width(Size::Fixed(80.0));
})
.padding(12.0, 0.0);
ui.list(items.len(), 30.0).grow().show(|ui, i| {
    ui.label(&items[i].name).grow();
    ui.label(format!("{} 円", items[i].price)).right().width(Size::Fixed(80.0));
});
# });
```
