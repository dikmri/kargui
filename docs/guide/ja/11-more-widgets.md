<!-- covers: Ui::radio_group Ui::separator Ui::progress Ui::progress_busy Ui::spinner Ui::card Ui::group Ui::expander Ui::link Ui::toggle_button Ui::password Toggled::mixed Ui::dropdown Dropdown::changed sig=13e8d8bd7ca74494 -->
# いろいろな部品

## ラジオボタン

いくつかから 1 つを選ぶ部品です。縦に並びます。横に詰めて並べたい時は `ui.segmented(..)` を使います。

```rust
#[derive(Clone, Copy, PartialEq)]
enum Size { Small, Medium, Large }
# let mut size = Size::Medium;
# kargui::test::example(|ui| {
if ui.radio_group(&mut size, &[(Size::Small, "小"), (Size::Medium, "中"), (Size::Large, "大")]) {
    // 選び直した
}
# });
```

Tab はグループに 1 回だけ止まり（選んでいるボタン）、矢印で選びながら移ります。端では反対の端へ回ります。

## ドロップダウン

いくつかから 1 つを選ぶ部品で、選んでいる項目だけを出します。項目が多い時や、場所が狭い時に使います。

```rust
#[derive(Clone, Copy, PartialEq)]
enum Theme { System, Light, Dark }
# let mut theme = Theme::System;
# kargui::test::example(|ui| {
let options = [(Theme::System, "システムの設定に合わせる"), (Theme::Light, "明るい"), (Theme::Dark, "暗い")];
if ui.dropdown(&mut theme, &options).label("テーマ").changed() {
    // テーマを切り替える
}
# });
```

押すと下に一覧が開きます（下に場所が無い時は上）。焦点がある時は、閉じたまま上下の矢印で前後の項目を選べます。Enter・Space・Alt+下・F4 で一覧が開きます。開いた一覧では、上下・Home・End で移り、Enter か Space で選びます。Esc を押すか一覧の外を押すと、選ばずに閉じます。読み上げソフトには、`.label(..)` で付けた名前と、選んでいる項目が伝わります。

## 区切り線・カード・枠

```rust
# let (mut sound, mut vibrate) = (true, false);
# kargui::test::example(|ui| {
ui.card(|ui| {
    ui.heading("容量");
    ui.label("64 GB のうち 12 GB を使っています").weak();
});
ui.separator();
ui.group("通知", |ui| {
    ui.checkbox(&mut sound, "音を鳴らす");
    ui.checkbox(&mut vibrate, "振動させる");
});
# });
```

`ui.separator()` は、列の中では横の線、行の中では縦の線になります。`ui.group(題, ..)` は題の付いたカードで、読み上げソフトには題を名前にした枠として伝わります。

## 進み具合

```rust
# let (done, total) = (3, 10);
# kargui::test::example(|ui| {
ui.progress(done as f32 / total as f32).label("写真を送っています");
ui.progress_busy().label("つないでいます");
ui.spinner().label("読み込んでいます");
# });
```

`ui.progress(割合)` は 0.0〜1.0 の棒です。終わりが分からない時は、流れる帯の `ui.progress_busy()` か、回る輪の `ui.spinner()` を使います。動く 2 つは、画面に出ている間だけ 1 秒に約 30 回描き直します。読み上げのために `.label(..)` で何の進み具合かを付けます。

## 開閉する節

```rust
# let (mut more, mut proxy) = (false, false);
# kargui::test::example(|ui| {
ui.expander(&mut more, "詳しい設定", |ui| {
    ui.checkbox(&mut proxy, "プロキシを使う");
});
# });
```

見出しを押すと（焦点がある時は Enter や Space でも）開閉します。読み上げソフトには、開いているかどうかも伝わります。

## リンク・押したままのボタン・一部だけ選んだチェック

```rust
# let (mut bold, mut help) = (false, false);
# let items = [true, false, true];
# kargui::test::example(|ui| {
if ui.link("バックアップの仕組み").clicked() {
    help = true;
}
ui.toggle_button(&mut bold, "太字");
let mut all = items.iter().all(|&c| c);
let some = items.iter().any(|&c| c);
ui.checkbox(&mut all, "全部を選ぶ").mixed(some && !all);
# });
```

`.mixed(true)` のチェックは横線を描き、読み上げソフトには「一部」と伝わります。押した時に何をするかは、アプリが決めます。

## パスワード

```rust
# let mut password = String::new();
# kargui::test::example(|ui| {
ui.password(&mut password).hint("パスワード").grow();
# });
```

文字の代わりに点を出し、コピーと切り取りをせず、読み上げソフトにも文字を渡しません。貼り付けはできます。
