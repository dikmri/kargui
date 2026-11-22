<!-- covers: Ui::animate Ui::animate_bool Ui::reduce_motion Handle::reveal Harness::reduce_motion sig=fed43c178b5db00c -->
# 動き

## 値を滑らかに動かす

`ui.animate(名前, 目標, 時間)` は、目標が変わった時に、今の値から新しい目標へ `時間` をかけて動く数を返します。返った数を、幅・高さ・色の濃さなど、描く物に使います。

```rust
use std::time::Duration;
use kargui::Size;

# let mut wide = false;
# kargui::test::example(|ui| {
if ui.button("広げる").clicked() {
    wide = !wide;
}
let width = ui.animate("panel", if wide { 320.0 } else { 160.0 }, Duration::from_millis(250));
ui.column(|ui| {
    ui.label("パネル");
})
.width(Size::Fixed(width));
# });
```

- 動き方は、初めが速く、終わりがゆっくりです（3 次の減速の曲線）。
- 初めて呼んだ時は、目標をそのまま返します。画面を開いた時に、パネルが 0 から広がることはありません。
- 動いている途中で目標が変わったら、その時の値から新しい目標へ動きます。値が飛びません。
- 動いている間だけフレームを描きます。目標に着いたら描くのをやめるので、止まっている窓は CPU を使いません。
- 名前は、同じ入れ物の中で動く値を見分けるのに使います（`ui.keyed` と同じ考え方です）。繰り返しの中では、行の番号や ID を名前にします。

## 出す・消すを動かす

`ui.animate_bool(名前, 出すか)` は、`false` の間は 0.0、`true` の間は 1.0 を返し、切り替わると 250 ms かけて動きます。250 ms は、WinUI の部品の標準の長さ（ControlNormalAnimationDuration）です。

```rust
# let mut details = true;
# kargui::test::example(|ui| {
let shown = ui.animate_bool("details", details);
if shown > 0.0 {
    // details が true になると上から現れ、false になると上へ消える
    ui.column(|ui| {
        ui.label("作った日 2026-11-22").weak();
        ui.label("直した日 2026-11-23").weak();
    })
    .reveal(shown);
}
# });
```

`.reveal(割合)` は、部品の上から `割合`（0.0〜1.0）の高さだけを見せ、残りを切り取ります。親の中では見せる高さだけを取るので、下の部品も一緒に動きます。中の部品は元の大きさのまま並ぶので、文字が詰まったり折り返しが変わったりしません。切り取られた所の部品は押せず、全部隠れた部品へは Tab で移りません。高さを決める `.height(Size::Fixed(…))` で伸ばす書き方は、中身を切り取らないので、伸びる途中で下の部品に重なります。

`animate` と `animate_bool` は、出しているかどうかに関わらず毎フレーム呼んでください。あるフレームで呼ばれなかった値は、そのフレームの終わりに捨てます。上の例で `animate_bool` を `if details { … }` の中に書くと、消す時の動きが見えません。次に呼んだ時は「初めて呼んだ」扱いになり、目標から始まるからです。

kargui の部品も同じ仕組みで動きます。切り替え（`ui.toggle`）の丸は、167 ms（WinUI の速い動きの長さ）で反対の端へ滑ります。開閉する節（`ui.expander`）の中身は、250 ms で上から出し入れします。

## 動きを減らす設定

OS で動きを切っている人には、`animate` と `animate_bool` は、すぐ目標の値を返します。Windows では「設定 > アクセシビリティ > 視覚効果 > アニメーション効果」です。kargui の部品の動きも同じように止まります。

- 設定は窓を作る時に読みます。アプリを開いたまま設定を変えた時も、OS から知らせ（`WM_SETTINGCHANGE`）が来た時に読み直します。
- `animate` を使わずに自分で作る動き（自由に描く面で時間を使って描く物など）では、`ui.reduce_motion()` を見て、`true` なら動かさないでください。
- 画面の動きで気分が悪くなる人がいます。WCAG 2.1 の達成基準 2.3.3（操作から始まる動き、AAA）は、本来の働きに要らない動きを止められるようにすることを求めています。

## 試験

試験台は、動きを切った状態で始まります。押した後の画面は、時計を進めなくても動き終わった形になります。開閉する節を閉じた直後に「中身が消えた」ことを確かめる試験も、そのまま書けます。

動きそのものを試す時は、`Harness::reduce_motion(false)` で動きを入れます。試験台の時計は `Harness::wait(時間)` を呼んだ時だけ進むので、動きの途中の値も決まった数で確かめられます。

```rust
use std::time::Duration;
use kargui::{App, Ui};
use kargui::test::Harness;

struct Drawer {
    open: bool,
    shown: f32,
}

impl App for Drawer {
    fn view(&mut self, ui: &mut Ui) {
        if ui.button("開く").clicked() {
            self.open = true;
        }
        self.shown = ui.animate_bool("drawer", self.open);
    }
}

// 初めは動きを切ってあるので、押すとすぐ開き終わる
let mut h = Harness::new(Drawer { open: false, shown: 0.0 });
h.click("開く");
assert_eq!(h.app().shown, 1.0);

// 動きを入れると、時計を進めた分だけ動く
let mut h = Harness::new(Drawer { open: false, shown: 0.0 });
h.reduce_motion(false).click("開く");
assert_eq!(h.app().shown, 0.0, "押したフレームは動き始め");
h.wait(Duration::from_millis(125));
assert!(h.app().shown > 0.5, "減速の曲線なので、半分の時間で半分より先");
h.wait(Duration::from_millis(125));
assert_eq!(h.app().shown, 1.0);
```
