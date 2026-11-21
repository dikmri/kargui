<!-- covers: Ui::menu_button Ui::menu_bar MenuBar::menu Menu::item Menu::check Menu::radio Menu::separator Menu::submenu MenuItem::shortcut MenuItem::enabled MenuItem::icon MenuItem::clicked Handle::context_menu Label::context_menu Harness::right_click sig=fca41d6176841f35 -->
# メニュー

## メニューボタン

押すと下にメニューが開くボタンです。項目は、渡した関数の中で作ります。

```rust
use kargui::{Key, Mods};
# let (mut text, mut wrap) = (String::from("メモ"), false);
# kargui::test::example(|ui| {
ui.menu_button("編集", |m| {
    if m.item("すべて消す").shortcut(Key::L, Mods::CTRL).clicked() {
        text.clear();
    }
    m.separator();
    m.check(&mut wrap, "折り返す");
});
# });
```

項目を選ぶとメニューが閉じます。Esc を押すか外を押すと、選ばずに閉じます。

項目の種類は次のとおりです。

- `m.item(文字)`: 選ぶと `.clicked()` が true になる項目
- `m.check(&mut 値, 文字)`: 選ぶたびに値が切り替わる、チェックの付く項目
- `m.radio(&mut 値, この値, 文字)`: いくつかから 1 つを選ぶ項目。選んでいる項目に点が付きます
- `m.separator()`: 項目の間の線
- `m.submenu(文字, |m| ..)`: 横にもう 1 つのメニューを開く項目

`.enabled(false)` を付けた項目は薄く描かれ、選べません。`.icon(..)` で項目の左にアイコンを出せます。

## ショートカット

`.shortcut(キー, 修飾キー)` を付けると、項目の右にキーを表示します。そのキーを押すと、メニューが閉じていても項目を選んだことになります。メニューを作る関数は、閉じている間も毎回呼ばれます。その時は項目を作らずに、キーが押されたかだけを確かめます。

焦点のある部品が自分で使うキーは、その部品に届きます。たとえば入力欄で Ctrl+C を押すと、入力欄が文字を写します。

## メニューバー

```rust
use kargui::{Key, Mods};
# let mut saved = 0;
# kargui::test::example(|ui| {
ui.menu_bar(|bar| {
    bar.menu("ファイル", |m| {
        if m.item("保存").shortcut(Key::S, Mods::CTRL).clicked() {
            saved += 1;
        }
        m.submenu("最近使ったファイル", |m| {
            m.item("メモ.txt");
            m.item("計画.md");
        });
    });
    bar.menu("ヘルプ", |m| {
        m.item("このアプリについて");
    });
});
# });
```

1 つのメニューが開いている時は、別の見出しにポインターを乗せるだけで、そのメニューに切り替わります。入れ子のメニューは、項目を押すか、項目の上でポインターを 0.4 秒止めると開きます。

## キーボード

| キー | 動き |
|---|---|
| F10 | メニューバーの最初の見出しへ移る（もう一度押すと元の場所へ戻る） |
| Enter・Space・下 | メニューを開いて最初の項目へ |
| 上 | メニューを開いて最後の項目へ |
| 上・下・Home・End | メニューの中で項目を移る（使えない項目は飛ばす） |
| 文字 | その文字で始まる次の項目へ |
| 右・左 | 入れ子のメニューを開く・閉じる。メニューバーのメニューでは隣のメニューへ |
| Esc | 一番上のメニューを閉じる |
| Tab | メニューを閉じる |

メニューバーのメニューで項目を選ぶと、焦点はメニューバーへ入る前の場所へ戻ります。メニューバーは Tab で 1 回だけ止まります。

読み上げソフトには、メニューを開くボタン、メニュー、メニューの項目として伝わります。チェックとラジオの項目は選んでいるかどうかも、ショートカットのキーも伝わります。

## 右クリックのメニュー

どの部品にも `.context_menu(|m| ..)` で右クリックのメニューを付けられます。行や列のような入れ物にも付けられます。

```rust
# let mut files = vec!["メモ.txt".to_string(), "計画.md".to_string()];
# kargui::test::example(|ui| {
let mut remove = None;
for (k, name) in files.iter().enumerate() {
    ui.label(name).context_menu(|m| {
        if m.item("削除").clicked() {
            remove = Some(k);
        }
    });
}
if let Some(k) = remove {
    files.remove(k);
}
# });
```

メニューは右クリックした場所に開きます。焦点がその部品（か中の部品）にある時は、Shift+F10 かアプリケーションキーでも開きます。この時は部品の左下に開きます。右クリックのメニューの項目のショートカットは、焦点がその部品の中にある時だけ動きます。

試験では `h.right_click(名前)` で右クリックできます。
