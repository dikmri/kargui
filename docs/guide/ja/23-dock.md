<!-- covers: Ui::dock DockArea::show DockArea::closable DockArea::grow Dock::new Dock::insert Dock::remove Dock::select Dock::panels Dock::selected Dock::contains Dock::save Dock::load sig=d57dfe3282e8411b -->
# ドッキング

`ui.dock(&mut dock)` は、パネルをタブのまとまりに入れて並べる部品です。利用者は、タブを引っ張ってほかのまとまりや端へ動かしたり、仕切りで広さを変えたりできます。開発の道具のように、利用者が自分の使いやすい並べ方にするアプリに向いています。

```rust
use kargui::{Dock, DockSide, DockTo};

#[derive(Clone, Copy, PartialEq, Hash)]
enum Panel { Files, Editor, Output }

let mut dock = Dock::new([Panel::Editor]);
dock.insert(Panel::Files, DockTo::Edge { side: DockSide::Left, share: 0.25 });
dock.insert(Panel::Output, DockTo::Split { of: Panel::Editor, side: DockSide::Bottom, share: 0.3 });
# kargui::test::example(|ui| {
let title = |p: &Panel| match p {
    Panel::Files => "ファイル",
    Panel::Editor => "編集",
    Panel::Output => "出力",
}
.to_string();
ui.dock(&mut dock).grow().show(title, |ui, p| match p {
    Panel::Files => {
        ui.label("main.rs");
    }
    Panel::Editor => {
        ui.label("fn main() {}");
    }
    Panel::Output => {
        ui.label("終わりました");
    }
});
# });
```

![左にファイル、右上に編集とメモのタブ、右下に出力のまとまりが並び、タブの横に閉じるボタンがある](../../shots/dock-light.png)

## 並べ方はアプリが持つ

並べ方は `Dock<K>` が持ちます。`K` はパネルを指すアプリの値で、上の例の `enum` のように `Clone`・`PartialEq`・`Hash` を持つ物にします。kargui は利用者の操作に合わせて `Dock` を書き換えるので、アプリは同じ `Dock` を毎フレーム渡します。

まとまりは、中のパネルで指します。番号で指さないので、ほかのパネルが動いても指す先は変わりません。

- `Dock::new(パネル)` は、パネルを 1 つのまとまりのタブにした並べ方を作ります。
- `dock.insert(パネル, 置き場所)` は、パネルを置きます。もう入っているパネルなら、そこへ動かします。置き場所は `DockTo` で、次の 3 つです。
  - `DockTo::Tab { with, index }`: `with` のまとまりのタブにする（`index` の前。`None` は最後）
  - `DockTo::Split { of, side, share }`: `of` のまとまりを分けて、`side` 側に置く（`share` は新しい方の割合）
  - `DockTo::Edge { side, share }`: 全体の外側の `side` に置く
- `dock.remove(&パネル)` は、パネルを外します。空になったまとまりは消え、隣のまとまりがその広さを取ります。
- `dock.select(&パネル)` はタブを選び、`dock.panels()` と `dock.selected()` は今のパネルと、まとまりごとの選んでいるパネルを返します。

## 利用者の操作

- タブを押すと選びます。タブを 4 画素より大きく引っ張ると、動かし始めます。
- 引っ張っている間は、ポインターのいる場所で落とす先が決まり、落ちる所が薄く塗られます。小さな的を狙う必要はありません。
  - まとまりの中身の真ん中: そのまとまりのタブにする
  - まとまりの中身の端から 4 分の 1 まで: 近い側（左・右・上・下）に分ける
  - 見出しの帯の上: タブの間に入れる
  - 全体の左・右・下の端の帯（幅 20 画素）と、上のまとまりの見出しの帯のすぐ下の帯: 全体の外側に置く（まとまりが 2 つ以上の時）
- Esc で取り消します。
- 仕切りを引くと広さが変わります。広さは割合で持つので、窓の大きさを変えても比が保たれます。
- タブを右クリックすると（Shift+F10 か、アプリのキーでも）、「新しいまとまりへ」（左・右・上・下）、「別のまとまりへ」、「閉じる」のメニューが出ます。まとまりに 1 つだけのパネルを「新しいまとまりへ」動かすと、全体の外側へ置きます。

引っ張るのが難しい人と読み上げソフトの人は、メニューとキーボードで同じことができます。

| キー | 働き |
|---|---|
| ←・→・Home・End（タブに焦点） | まとまりの中でタブを選ぶ |
| Ctrl+Tab・Ctrl+PageDown（まとまりの中に焦点） | 次のタブ（Shift か PageUp で前のタブ） |
| Ctrl+Shift+PageUp・PageDown（タブに焦点） | タブを前（後ろ）へ並べ替える |
| F6・Shift+F6 | 次（前）のまとまりのタブへ焦点を移す |
| Delete（タブに焦点） | 閉じる（`.closable()` の時） |
| ←→↑↓・Home・End（仕切りに焦点） | 仕切りを動かす |

読み上げソフトには、まとまりごとにタブの並び・タブ・タブの中身として伝わり、仕切りは位置を百分率で伝えます。

## 閉じる

`.closable()` を付けると、タブに閉じるボタンが付きます（メニューの「閉じる」と Delete も使えます）。kargui は閉じず、`show` が閉じたいパネルを返すだけです。アプリが `dock.remove` で外します。保存していない変更があれば、外す前に確かめられます。

```rust
# use kargui::Dock;
# let mut dock = Dock::new(["メモ", "下書き"]);
# kargui::test::example(|ui| {
if let Some(p) = ui.dock(&mut dock).closable().grow().show(|p| p.to_string(), |ui, p| {
    ui.label(*p);
}) {
    dock.remove(&p);
}
# });
```

## 並べ方を保存する

`dock.save(名前)` は、並べ方を文字にします。次に起動した時に `Dock::load(文字, 戻し方)` で戻します。知らない名前（アプリから無くなったパネル）は捨てられ、空になったまとまりは消えます。文字の形が違う時（kargui の新しい版で保存した物など）は `None` を返すので、決まった並べ方に戻してください。

```rust
use kargui::Dock;

let dock = Dock::new(["メモ", "下書き"]);
let text = dock.save(|p| p.to_string());
let back = Dock::load(&text, |name| ["メモ", "下書き"].into_iter().find(|p| *p == name)).unwrap_or(dock.clone());
assert_eq!(back, dock);
```

## まだ無いこと

まとまりを別の窓に出すことは、まだできません。端に隠す（オートハイド）、名前を付けた並べ方、まとまりごと引っ張ることも、後の版の候補です。
