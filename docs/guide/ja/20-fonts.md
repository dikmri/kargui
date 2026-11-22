<!-- covers: Font Font::family Ui::font Ui::set_font Label::font Label::monospace TextInput::font TextInput::monospace TextArea::font TextArea::monospace Harness::with_options sig=800225c94540e67a -->
# 書体

## 窓の書体

何も決めなければ、文字は OS の UI の書体で出ます。Windows では Yu Gothic UI です。窓全体の書体は `Options::font` で決めます。

```rust
use kargui::{Font, Options};

let mut options = Options::new("帳簿");
options.font = Font::family("BIZ UDPGothic");
```

書体は `Font` で表します。

- `Font::Ui`: OS の UI の書体（既定）。Windows は Yu Gothic UI、macOS は Hiragino Sans、Linux は Noto Sans CJK JP などの日本語のゴシック体
- `Font::Monospace`: 等幅の書体。コード・ファイルの名前・桁をそろえたい数に使います。日本語も持つ書体を先に選ぶので、日本語 1 字が英字 2 字とちょうど同じ幅になります（Windows は BIZ UD ゴシック、無ければ Consolas、MS ゴシックの順）
- `Font::family("名前")`: 名前で選ぶ書体。OS に入っている物か、アプリが渡したファイルの物です

名前の書体が見つからない時は、UI の書体で出します（記録に `font.missing` と書きます）。書体に無い文字は OS の代わりの書体で出るので、英字だけの書体を選んでも日本語は消えません。

アプリを動かしている間に変える時は、`ui.set_font` を使います。設定の画面で書体を選ぶ時などです。そのフレームから、書体を決めていない部品の文字が全部変わります。

```rust
use kargui::Font;

# let mut readable = true;
# kargui::test::example(|ui| {
ui.checkbox(&mut readable, "読みやすい書体にする");
ui.set_font(if readable { Font::family("BIZ UDPGothic") } else { Font::Ui });
assert_eq!(ui.font(), Font::family("BIZ UDPGothic"));
# });
```

## 部品ごとの書体

ラベル・入力欄・複数行の入力欄は、`.font(書体)` で自分の書体を持てます。等幅には `.monospace()` が使えます。

```rust
use kargui::Font;

# let mut code = String::new();
# kargui::test::example(|ui| {
ui.label("請求書").font(Font::family("BIZ UDPGothic"));
ui.label("ID: 4F2A-0019").monospace();
ui.text_area(&mut code).monospace().rows(8);
# });
```

入力欄では、キャレットを上下の行へ動かす時や、押した位置を文字の位置に直す時も、その欄の書体で測ります。

## アプリに書体を入れる

OS に入っていない書体は、ファイルの中身を `Options::fonts` に入れて渡します。TrueType と OpenType（`.ttf`・`.otf`・`.ttc`）が使えます。`include_bytes!` で実行ファイルに入れておけば、どの PC でも同じ書体で出ます。選ぶ時は、書体の中に書かれたファミリーの名前を `Font::family` に渡します。

```rust
use kargui::{Font, Options};

let mut options = Options::new("お店");
// アプリでは: options.fonts.push(include_bytes!("../fonts/ShopSans.ttf").as_slice().into());
if let Ok(data) = std::fs::read("fonts/ShopSans.ttf") {
    options.fonts.push(data.into());
}
options.font = Font::family("Shop Sans");
```

- 渡した書体は、OS に同じ名前の書体があっても先に使います。
- `Options::fonts` が空なら、窓を開く時に書体のデータを読みません。等幅や名前の書体も、初めて使う時に OS の書体の一覧を読みます。UI の書体だけを使うアプリの起動は遅くなりません。
- 日本語の書体のファイルは大きく（ゴシック体 1 つで数 MB〜20 MB）、実行ファイルもその分大きくなります。OS に入っている書体で足りる時は、名前で選んでください。

## 試験

`Harness::with_options(アプリ, &options)` は、アプリと同じ `Options` の大きさ・書体・書体のファイルで試験台を作ります。

```rust
use kargui::{App, Font, Options, Ui};
use kargui::test::Harness;

struct Code {
    text: String,
}

impl App for Code {
    fn view(&mut self, ui: &mut Ui) {
        ui.row(|ui| {
            ui.label("iiii").monospace();
        });
        ui.row(|ui| {
            ui.label("MMMM").monospace();
        });
        ui.text_area(&mut self.text).hint("コード");
    }
}

let mut options = Options::new("コード");
options.font = Font::Monospace;
let h = Harness::with_options(Code { text: String::new() }, &options);
// 等幅なので、細い字も太い字も同じ幅
let (i, m) = (h.find("iiii").unwrap().w, h.find("MMMM").unwrap().w);
assert!((i - m).abs() < 1.0);
```
