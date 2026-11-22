<!-- covers: Ui::rich_text RichText::text RichText::link RichLink::clicked RichLink::bold Span::bold Span::italic Span::underline Span::strike Span::weak Span::color Span::small Span::size Span::font Span::monospace Span::code sig=8398e6e20e27a91e -->
# 書式付きの文字

`ui.rich_text(|t| …)` は、部分ごとに書式の違う文字を、1 つの段落に並べます。太字・斜体・色・コード・リンクを、文の途中で混ぜられます。

```rust
# let mut show_help = false;
# kargui::test::example(|ui| {
ui.rich_text(|t| {
    t.text("保存すると ");
    t.text("元に戻せません").bold();
    t.text("。詳しくは ");
    if t.link("ヘルプ").clicked() {
        show_help = true;
    }
    t.text(" を見てください。");
});
# });
```

- 部分は、書いた順に 1 つの段落に並びます。段落は幅に合わせて折り返します。行（`ui.row`）の中に置くと、残りの幅に広がります。
- 書式は部分ごとです。`t.text("…")` の後に付けた物は、その部分だけに効きます。ほかの部分は本文の書式のままです。
- 文字の中の `\n` で改行します。
- 書式が 1 つだけの文字なら、`ui.label("…").bold()` のほうが短く書けます。

## 部分の書式

| 書き方 | 見た目 |
|---|---|
| `.bold()` | 太字 |
| `.italic()` | 斜体。日本語の書体のように斜体を持たない書体は、傾けて描きます |
| `.underline()` | 下線 |
| `.strike()` | 取り消し線（値引きの前の値段など） |
| `.weak()` | 弱い文字の色（補足） |
| `.color(色)` | その色 |
| `.small()` | 小さい文字（12.5） |
| `.size(大きさ)` | 大きさ（論理ピクセル。本文は 14）。大きい部分があると、その行が高くなります |
| `.font(書体)`・`.monospace()` | 書体（20 章） |
| `.code()` | 等幅の書体と薄い背景（コード・キーの名前・ファイル名） |

```rust
use kargui::rgb;

# kargui::test::example(|ui| {
ui.rich_text(|t| {
    t.text("価格 ");
    t.text("1,980").strike().weak();
    t.text(" 1,280").bold().color(rgb(0xc42b1c));
    t.text("（税込み）").small().weak();
});
ui.rich_text(|t| {
    t.text("保存は ");
    t.text("Ctrl+S").code();
    t.text("。");
    t.text("下書き").italic();
    t.text("は ");
    t.text("毎回").underline();
    t.text("残ります。");
});
# });
```

## リンク

`t.link("…")` は、文の中のリンクです。主の色に下線を付けて描きます。ポインターを乗せると、手の形になり、下線が太くなります。

- 押すか、Tab で焦点を移して Enter を押すと、`.clicked()` が true になります。ウェブのページを開くのはアプリの仕事です。
- 2 行にまたがるリンクは、どちらの行を押しても同じリンクです。焦点の枠も行ごとに出ます。
- `.bold()` で太字にできます。
- 押した時は、記録に `rich.link text=…` が出ます。
- `ui.enabled(false, …)` の中では押せず、灰色になります。

文の外に 1 つだけ置くリンクは、`ui.link("…")`（11 章）でも書けます。

## 読み上げと試験

読み上げソフトは、段落の中の部分を順に読み、リンクはリンクとして伝えます。試験台では、部分の文字を名前にして探し、押せます。

```rust
use kargui::{App, Ui};
use kargui::test::Harness;

#[derive(Default)]
struct Notice {
    opened: bool,
}

impl App for Notice {
    fn view(&mut self, ui: &mut Ui) {
        ui.rich_text(|t| {
            t.text("詳しくは ");
            if t.link("ヘルプ").clicked() {
                self.opened = true;
            }
            t.text(" を見てください。");
        });
    }
}

let mut h = Harness::new(Notice::default());
assert!(h.find("詳しくは ").is_some());
h.click("ヘルプ");
assert!(h.app().opened);
```
