<!-- covers: Ui::grid Ui::split_left_right Ui::split_top_bottom Split::label Split::grow Split::height Split::changed Ui::canvas Drawing::rect Drawing::rounded_rect Drawing::circle Drawing::line Drawing::polyline Drawing::polygon Drawing::text Drawing::image CanvasView::pointer CanvasView::clicked_at Harness::drag sig=e5b37aca5ba6090b -->
# 格子・仕切り・自由に描く面

## 格子

部品を表の升のように並べます。作った順に左から右へ入り、列の数だけ入ると次の行へ移ります。入力の画面で、名前と欄をそろえて並べる時に使います。

```rust
use kargui::Size;
# let (mut name, mut email) = (String::new(), String::new());
# kargui::test::example(|ui| {
ui.grid(&[Size::Auto, Size::Fill(1.0)], |ui| {
    ui.label("名前");
    ui.text_input(&mut name).label("名前");
    ui.label("メールアドレス");
    ui.text_input(&mut email).label("メールアドレス");
});
# });
```

列の幅は、`Size::Auto` ならその列でいちばん広い部品の幅、`Size::Fixed(..)` なら決めた幅、`Size::Fill(..)` なら残りの幅を分けた幅になります。行の高さは、その行でいちばん高い部品に合わせます。部品は行の中で縦の真ん中にそろいます。

## 仕切り

2 つの区画を並べ、間の仕切りをつかんで広さを変えられるようにします。

```rust
# let mut left = 220.0;
# kargui::test::example(|ui| {
ui.split_left_right(&mut left, |ui, pane| {
    if pane == 0 {
        ui.label("フォルダー");
    } else {
        ui.label("ファイル");
    }
})
.label("フォルダーの一覧の幅")
.grow();
# });
```

`left` は左の区画の幅（論理ピクセル）です。渡した関数は区画ごとに呼ばれ、2 つ目の引数で区画の番号（左が 0、右が 1）を受け取ります。上下に分ける時は `ui.split_top_bottom(&mut 上の高さ, ..)` を使います。どちらの区画も 48 より狭くはなりません。

仕切りには Tab で止まり、矢印で 10 ずつ、Home と End で端まで動かせます。ポインターを乗せると、ポインターの形が左右（か上下）の矢印になります。読み上げソフトには、`.label(..)` で付けた名前（付けなければ "Resize"）と今の幅が伝わります。

## 自由に描く面

グラフや図を描く面です。渡した関数の中で、面の左上からの座標（論理ピクセル）を使って描きます。

```rust
use kargui::{Rect, rgb};
# let values = [12.0, 30.0, 22.0, 48.0];
# kargui::test::example(|ui| {
ui.canvas(240.0, 120.0, |d| {
    d.rect(Rect::new(0.0, 0.0, 240.0, 120.0), rgb(0xf3f5f8));
    let points: Vec<(f32, f32)> = values.iter().enumerate().map(|(k, v)| (20.0 + k as f32 * 60.0, 110.0 - v * 2.0)).collect();
    d.polyline(&points, 2.0, rgb(0x2f6fd6));
    for &p in &points {
        d.circle(p, 4.0, rgb(0x2f6fd6));
    }
    d.text((8.0, 4.0), "訪問数", rgb(0x333333));
})
.label("週ごとの訪問数");
# });
```

描ける物は、四角（`rect`・`rounded_rect`）、円（`circle`）、線（`line`・`polyline`）、塗った多角形（`polygon`）、文字（`text`）、画像（`image`）です。後に描いた物が上に重なり、面の外には描きません。斜めの線と多角形は kargui が自分で塗って文字と同じ表に入れるので、CPU と GPU のどちらの描き方でも同じ絵になります。画像は、今は CPU の描き方だけで描きます。

ポインターの操作は、返ってきた値に聞きます。

```rust
# let mut stroke: Vec<(f32, f32)> = Vec::new();
# let mut marks: Vec<(f32, f32)> = Vec::new();
# kargui::test::example(|ui| {
let pad = ui.canvas(300.0, 200.0, |_| {}).label("手書き");
if let Some(at) = pad.pointer() {
    // 押している間の位置（押したまま面の外へ出ても続く）
    stroke.push(at);
}
if let Some(at) = pad.clicked_at() {
    // 押して離した位置
    marks.push(at);
}
# });
```

試験では `h.drag(名前, 横, 縦)` で、押したまま動かして離す操作ができます。
