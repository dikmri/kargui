<!-- covers: Ui::tabs Tabs::changed Ui::number NumberInput::step NumberInput::changed Ui::toolbar Toolbar::button Toolbar::toggle Toolbar::separator Toolbar::more Toolbar::ui sig=04b1f1f2e9b03f53 -->
# タブ・数の入力・ツールバー

## タブ

見出しを押すと、下の中身が切り替わる部品です。中身は、渡した関数が選んでいる値を受け取って作ります。

```rust
#[derive(Clone, Copy, PartialEq)]
enum Page { General, Advanced }
# let (mut page, mut sound, mut proxy) = (Page::General, true, false);
# kargui::test::example(|ui| {
let pages = [(Page::General, "全般"), (Page::Advanced, "詳しい設定")];
ui.tabs(&mut page, &pages, |ui, page| match page {
    Page::General => {
        ui.checkbox(&mut sound, "音を鳴らす");
    }
    Page::Advanced => {
        ui.checkbox(&mut proxy, "プロキシを使う");
    }
});
# });
```

Tab は選んでいる見出しに 1 回だけ止まります。見出しに焦点がある時は、左右の矢印で前後のタブを選びます（端では反対の端へ回ります）。Home と End で最初と最後のタブを選びます。中身に焦点がある時は、Ctrl+Tab か Ctrl+PageDown で次のタブ、Ctrl+Shift+Tab か Ctrl+PageUp で前のタブを選びます。

読み上げソフトには、タブの並び・タブ（どれを選んでいるか）・中身（選んでいるタブの名前）として伝わります。

## 数の入力

数を打つ欄です。右に、1 つ増やす・減らす小さいボタンが付きます。

```rust
# let (mut copies, mut zoom) = (1, 1.0);
# kargui::test::example(|ui| {
ui.number(&mut copies, 1..=99).label("部数");
ui.number(&mut zoom, 0.5..=4.0).step(0.25).label("倍率");
# });
```

打った数は、Enter を押すか焦点が外れた時に値になります。数でない文字は元に戻し、範囲の外の数は近い方の端にします。日本語入力で打った全角の数字も読めます。上下の矢印は `.step(..)` の分（決めなければ 1）、PageUp と PageDown はその 10 倍だけ変えます。

読み上げソフトには、値と範囲を持つスピンボタンとして伝わります。読み上げソフトの「増やす・減らす」の操作でも値が変わります。

## ツールバー

アイコンのボタンを横に並べる部品です。ボタンの名前はツールチップにも出ます。

```rust
use kargui::Icon;
# let (mut count, mut pinned) = (0, false);
# kargui::test::example(|ui| {
ui.toolbar(|tb| {
    if tb.button(Icon::Plus, "追加").clicked() {
        count += 1;
    }
    tb.toggle(&mut pinned, Icon::Check, "固定");
    tb.separator();
    tb.more(|m| {
        m.item("設定");
    });
})
.label("タスク");
# });
```

- `tb.button(アイコン, 名前)`: アイコンだけのボタン
- `tb.toggle(&mut 値, アイコン, 名前)`: 押すたびにオンとオフが切り替わるボタン
- `tb.separator()`: ボタンの間の線
- `tb.more(|m| ..)`: 「ほかの操作」のメニューを開くボタン（点が 3 つ）。たまに使う操作を入れます
- `tb.ui()`: ツールバーの `Ui`。検索の欄など、ほかの部品を置く時に使います

Tab はツールバーに 1 回だけ止まります（最後に使ったボタン）。左右の矢印と Home・End で、ボタンの間を移ります。使えないボタンは飛ばします。`.label(..)` でツールバーに名前を付けます。
