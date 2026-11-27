<!-- covers: Ui::time_picker TimePicker::changed TimePicker::minute_step Time::new Time::now Time::parse Time::hour Time::minute Time::minutes Time::from_minutes Time::add_minutes Ui::color_picker ColorPicker::changed ColorPicker::alpha ColorPicker::swatches Ui::set_color_text ColorText::english ColorText::japanese sig=e317c6ca600891c4 -->
# 色と時刻の選択

## 時刻の選択

`ui.time_picker(&mut 時刻)` は、時刻（`Time`）の欄です。押すと、下に時と分のます目が出ます。時刻は `Option<Time>` で持ち、選ぶまでは `None` です。

```rust
use kargui::Time;

# let mut start: Option<Time> = Time::new(9, 30);
# kargui::test::example(|ui| {
if ui.time_picker(&mut start).label("開始").minute_step(15).changed() {
    // 新しい時刻を保存する
}
# });
```

- 時を選ぶと、分のます目へ移ります。分を選ぶと閉じます。時を選んだ時、分は今の分のままです。
- 分は 5 分ずつ並びます。`.minute_step(15)` なら 00・15・30・45、`.minute_step(1)` なら 60 個です。刻みに乗らない時刻（9:37 など）もそのまま持てます。
- キーボード: 欄で Enter・Space・Alt+下・F4 を押すと開きます。ます目の中は矢印で動き、Enter か Space で選びます。Esc か外を押すと、選ばずに閉じます。
- 時刻が無い時は、今の時刻の時に焦点を置いて開きます（試験台では 12 時）。
- 欄の文字（「時刻を選ぶ」など）とます目の名前は、日付の選択と同じく kargui の言語に合わせて決まり、`ui.set_date_text` で決め直せます。読み上げソフトには、時刻の入力欄と、「時」「分」という名前の 2 つのます目として伝わります。

`Time` は時と分を持つ値です。日をまたぐ計算は、24 時間で回ります。

```rust
use kargui::Time;

let t = Time::new(23, 50).unwrap();
assert_eq!(t.add_minutes(20).to_string(), "00:10");
assert_eq!(Time::parse("7:05"), Time::new(7, 5));
assert_eq!(Time::from_minutes(90).hour(), 1);
assert_eq!(Time::new(1, 30).unwrap().minutes(), 90);
assert_eq!(Time::new(18, 45).unwrap().minute(), 45);
assert!(Time::now().hour() < 24);
```

## 色の選択

`ui.color_picker(&mut 色)` は、色（`Color`）の欄です。押すと、下に次の物が出ます。

- 彩度（左から右）と明るさ（下から上）を選ぶ面
- 色相の帯
- 色の番号（`#RRGGBB`）の欄
- 見本の色 16 個

```rust
# let mut accent = kargui::rgb(0x1a73e8);
# kargui::test::example(|ui| {
if ui.color_picker(&mut accent).label("強調の色").changed() {
    // 新しい色で描き直す
}
# });
```

- 面や帯を引っ張る・矢印を押す・6 桁の番号を打ち終える・見本を押す、のどれでも、その時に色が変わります。Esc か外を押すと閉じます。
- 3 桁の番号（`#F80` など）は、Enter を押した時に読みます。打ったり消したりする途中の 3 桁（`#FF9` など）で色が変わらないようにするためです。
- 面と帯は、矢印で 1%（色相は 1 度）ずつ、Shift を押しながらなら 10 ずつ動きます。
- 開いている間は色相を覚えています。彩度を 0（灰色）まで下げてから戻しても、元の色相に戻ります。
- 見本を押した時と番号を打った時は、その色をそのまま返します。アプリの見本の色と `==` で比べられます。
- 面と帯は、絵（画像）で描きます。Direct3D 11 の描き方（既定ではない）は、まだ画像を描きません。

不透明度も選ぶ時は `.alpha()` を付けます。不透明度の帯が増え、番号は `#RRGGBBAA` になります。付けなければ、不透明度は元の値のままです。見本の色は `.swatches(&[色])` で替えられます（1 行に 8 個）。

```rust
use kargui::rgb;

# let mut shade = [0.0, 0.0, 0.0, 0.4];
# let mut label = rgb(0xd93025);
# kargui::test::example(|ui| {
ui.color_picker(&mut shade).alpha();
let brand = [rgb(0xd93025), rgb(0x188038), rgb(0x1a73e8)];
ui.color_picker(&mut label).swatches(&brand);
# });
```

面や帯の名前（「色相」「不透明度」など）は、kargui の言語に合わせて決まり、`ui.set_color_text` で決め直せます（`ColorText::japanese()` で日本語）。読み上げソフトには、色の欄と、面と帯のスライダー、番号の入力欄、見本のボタンとして伝わります。

```rust
use kargui::ColorText;

# kargui::test::example(|ui| {
ui.set_color_text(ColorText::japanese());
# });
assert_eq!(ColorText::english().hue, "Hue");
```

## 試験

色と時刻の選択は、ほかの部品と同じく名前で押して試せます。ます目の名前は「09 時」「30 分」（英語なら `09 Hours`・`30 Minutes`）、見本の名前は色の番号（`#D93025`）です。

```rust
use kargui::{App, ColorText, DateText, Time, Ui};
use kargui::test::Harness;

struct Form {
    start: Option<Time>,
    color: kargui::Color,
}

impl App for Form {
    fn view(&mut self, ui: &mut Ui) {
        ui.set_date_text(DateText::japanese());
        ui.set_color_text(ColorText::japanese());
        ui.time_picker(&mut self.start).label("開始");
        ui.color_picker(&mut self.color).label("色");
    }
}

let mut h = Harness::new(Form { start: None, color: kargui::rgb(0x000000) });
h.click("開始").click("09 時").click("30 分");
assert_eq!(h.app().start, Time::new(9, 30));
h.click("色").click("#D93025");
assert_eq!(h.app().color, kargui::rgb(0xd93025));
```
