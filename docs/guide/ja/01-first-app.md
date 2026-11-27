<!-- covers: App Options run Ui::heading Ui::label Ui::button Ui::row sig=c5a08b17950f69d8 -->
# 最初のアプリ

kargui のアプリは、状態を持つ自分の構造体と、その画面を書く `view` の 2 つでできています。

## 準備

`Cargo.toml` に kargui を足します（`cargo add kargui` でも足せます）。

```toml
[dependencies]
kargui = "0.4"
```

crates.io に出す前の新しい物を使う時は `kargui = { git = "https://github.com/dikmri/kargui" }`、手元の kargui を使う時は `kargui = { path = "../kargui/crates/kargui" }` と書きます。Linux で組み立てるには、fontconfig の開発用ファイルが要ります（Debian と Ubuntu では `libfontconfig1-dev` と `pkg-config`）。

## 数を数えるアプリ

```rust,no_run
use kargui::{App, Options, Ui};

struct Counter {
    count: u32,
}

impl App for Counter {
    fn view(&mut self, ui: &mut Ui) {
        ui.heading("数を数える");
        ui.row(|ui| {
            ui.label(format!("今の数: {}", self.count));
            if ui.button("1 足す").clicked() {
                self.count += 1;
            }
        });
    }
}

fn main() {
    kargui::run(Options::new("数を数える"), Counter { count: 0 }).unwrap();
}
```

## 動き方

- kargui は、入力があった時や窓の大きさが変わった時に `view` を呼びます。何も起きていない間は呼ばず、CPU も使いません。
- `view` は毎回、画面全体を上から書きます。前の画面を覚えておく必要はありません。
- 状態（ここでは `count`）は自分の構造体に持ちます。部品には `&mut` で渡すか、値を読んで表示します。
- `clicked()` のような答えは、前のフレームで決まった部品の場所を使って決まります。そのため、1 回の `view` の中でそのまま `if` で使えます。
- 描くのは既定で CPU です。前の画面から変わった所だけを描き直すので、GPU の無い PC でも速く動きます。

## 窓の設定

`Options` で窓の題・大きさ・場所・明暗を決めます。

```rust,no_run
# use kargui::{App, Options, Ui};
# struct Counter { count: u32 }
# impl App for Counter { fn view(&mut self, _ui: &mut Ui) {} }
let mut options = Options::new("数を数える");
options.size = (480.0, 320.0); // 論理ピクセル
options.dark = Some(true); // None なら OS の設定に従う
kargui::run(options, Counter { count: 0 }).unwrap();
```
