<!-- covers: Handle::drag_source Handle::dropped Harness::drag_to Harness::drag_over Harness::release sig=40cfeefb33560e8e -->
# ドラッグ＆ドロップ

## 引っ張って移す

部品に `.drag_source(荷物)` を付けると、ポインターで引っ張り出せます。荷物は、何を運ぶかを表す値です（どの列の何番目か、ファイルのパスなど）。受け取る部品に `.dropped::<型>()` を付けると、その型の荷物が落とされた時に `Some(荷物)` を返します。

```rust
#[derive(Clone, Copy)]
struct Card(usize);

# let mut todo = vec!["ガイドを書く".to_string(), "写真を撮る".to_string()];
# let mut done: Vec<String> = Vec::new();
# kargui::test::example(|ui| {
ui.row(|ui| {
    ui.column(|ui| {
        ui.heading("やること");
        for (k, name) in todo.iter().enumerate() {
            ui.label(name).drag_source(Card(k));
        }
    });
    let mut done_column = ui.column(|ui| {
        ui.heading("済み");
        for name in &done {
            ui.label(name);
        }
    });
    if let Some(Card(k)) = done_column.dropped::<Card>() {
        done.push(todo.remove(k));
    }
});
# });
```

- 押して 4 ピクセルより動かすと、引っ張り始めます。それより短い動きはクリックのままです（引っ張り出せる行の中のボタンも押せます）。
- 引っ張っている間は、元の部品の名前を書いた箱がポインターについて動きます。受け取れる部品の上では、その部品を主の色で囲みます。受け取れない所では、ポインターが禁止の形になります。
- Esc を押すと取り消します。受け取れる部品の外で離した時も、何も起きません。
- 受け取る物は荷物の型で決まります。違う型の荷物は受け取りません。
- 入力欄・スライダー・仕切り・自由に描く面のように、自分で引っ張りを使う部品の上からは始まりません。押せない部品からも始まりません。
- 荷物は、引っ張り始めた時に 1 回だけ受け取ります。`Clone` は要りません。

## 並べ替え（受け取る部品を入れ子にする）

受け取る部品が重なっている所では、いちばん内側の部品が受け取ります。行の上に落とした時はその行の前へ、行の無い所に落とした時は最後へ、という並べ替えが書けます。

```rust
#[derive(Clone, Copy)]
struct Item(usize);

# let mut items = vec!["一".to_string(), "二".to_string(), "三".to_string()];
# kargui::test::example(|ui| {
let mut moved = None;
let mut list = ui.column(|ui| {
    for (k, name) in items.iter().enumerate() {
        let mut row = ui
            .card(|ui| {
                ui.label(name);
            })
            .drag_source(Item(k));
        // 行の上に落としたら、その行の前へ
        if let Some(Item(from)) = row.dropped::<Item>() {
            moved = Some((from, k));
        }
    }
});
// 行の無い所に落としたら、最後へ
if let Some(Item(from)) = list.dropped::<Item>() {
    moved = Some((from, items.len()));
}
if let Some((from, before)) = moved {
    let item = items.remove(from);
    let at = if from < before { before - 1 } else { before };
    items.insert(at, item);
}
# });
```

## 引っ張れない人のために

引っ張る操作は、手が思うように動かない人には難しく、キーボードや読み上げソフトではできません。WCAG 2.2 の達成基準 2.5.7 も、引っ張らずにできる別のやり方を求めています。同じことができるメニューやボタンを、必ず一緒に置いてください。見本の `examples/board` は、どのカードにも「移す」メニュー（ほかの列へ・上へ・下へ）を置いています。

## 試験

`Harness::drag_to(元の名前, 先の名前)` で、引っ張って落とす操作を試せます。元の部品を押し、何回かに分けて動かして（1 回ごとに 1 フレーム）、先の部品の真ん中で離します。引っ張っている途中の画面（受け取る部品の強調など）を確かめる時は、`Harness::drag_over(元の名前, 先の名前)` で押したまま止め、`Harness::release()` で離します。止めている間に Esc を押すと取り消せます。

```rust
use kargui::{App, Ui};
use kargui::test::Harness;

struct Shelf {
    books: Vec<&'static str>,
    read: Vec<&'static str>,
}

impl App for Shelf {
    fn view(&mut self, ui: &mut Ui) {
        for (k, book) in self.books.iter().enumerate() {
            ui.label(*book).drag_source(k);
        }
        let mut read = ui.column(|ui| {
            ui.heading("読んだ本");
        });
        if let Some(k) = read.dropped::<usize>() {
            let book = self.books.remove(k);
            self.read.push(book);
        }
    }
}

let mut h = Harness::new(Shelf { books: vec!["坊っちゃん", "こころ"], read: vec![] });
h.drag_to("こころ", "読んだ本");
assert_eq!(h.app().read, ["こころ"]);
```
