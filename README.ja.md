# kargui

Rust で書く、軽いネイティブ GUI フレームワーク（Windows 10/11。0.1 は最初の版）。名前は「軽い」「Rust」「GUI」から。[English README](README.md)（この文の訳）。

Tauri・egui・GPUI で同じアプリ（タスク帳）を作って比べた実験（[AI コーディングノートの実験レポート番外編](https://dikmri.github.io/ai-coding-notes/reports/rust-gui/)）から生まれた。3 つにはそれぞれ優れた所がある。egui は書きやすく、GPUI はメモリが少なく、Tauri は GPU が無い PC でも滑らかに描く。kargui はそれらを 1 つにまとめることを目指し、前回と同じ道具・同じ条件で測りながら作っている。作った過程は[実験レポート番外編 2](https://dikmri.github.io/ai-coding-notes/reports/kargui/)にまとめた。3 つのフレームワークと、その作者・貢献者から多くを学んだ。

## 特徴

- **即時モードの書き方。** 状態は自分の構造体に持ち、`view` で毎回画面全体を書く（egui と同じ書きやすさ）。
- **変わった所だけ描く。** 中で前の画面と比べ、既定の CPU の道で変わった所だけを描き直す。待機中は CPU を使わない。GPU の無い PC でも速い。
- **日本語入力。** 変換を確定する Enter で送信しない、変換中は値を変えない、文節ごとの下線、候補の窓を文字の真下に出す、焦点を移しても変換中の文字を失わない。
- **読み上げとキーボード。** AccessKit で全部の部品を UI オートメーションに渡す。名前の無い部品は記録で警告する。Tab で全部に届く。
- **説明が古くならない。** 公開項目すべてに説明と動く例があり、ガイドのコードも試験で動かす。API が変わったのにガイドを直していないと、検査で落ちる。

## 部品

見出し・ラベル（折り返し、大きさ、右寄せ）・ボタン・チェック・スイッチ・ダークモードのスイッチ・選択（segmented）・入力欄・スライダー・一覧（見えている行だけ作る、行の選択、行を id で区別）・スクロールの入れ物。並べ方は行・列・重ね・残りを分ける（2 回で決める配置）。写真は [docs/widgets.md](docs/widgets.md)。

```rust,no_run
use kargui::{App, Options, Ui};

struct Counter {
    count: u32,
}

impl App for Counter {
    fn view(&mut self, ui: &mut Ui) {
        ui.row(|ui| {
            ui.label(format!("回数: {}", self.count));
            if ui.button("1 足す").clicked() {
                self.count += 1;
            }
        });
    }
}

fn main() {
    kargui::run(Options::new("数える"), Counter { count: 0 }).unwrap();
}
```

## 測った数字（タスク帳。前回の比べと同じ道具・同じ PC）

| 項目 | kargui（CPU の道） | Tauri・egui・GPUI の最良 | 目標 |
|---|---|---|---|
| 起動（空の一覧） | 42 ms | 344 ms（egui、glow） | 330 ms |
| メモリ（1 万件・待機） | 5.4 MB | 17.9 MB（GPUI） | 12.5 MB |
| スクロール中の 1 フレームの CPU（測定の道具） | 0.36 ms | 1.97 ms（egui） | 1.2 ms |
| 待機中の CPU | 0 | 0（egui） | 0.16% |
| 実行ファイルの大きさ | 1.0 MB | 6.7 MB（egui） | 4.5 MB |
| タスク帳の画面のコード | 198 行 | 316 行（egui） | 220 行 |

測定の道具は CPU の時間を 15.6 ms 単位で数えるので、1 ms 前後の 1 フレームの値は回ごとにばらつく。kargui 自身の時計では、スクロール中の 1 フレームの仕事は約 0.58 ms。

## 動かし方

```powershell
cargo run --release -p taskbook-kargui -- --items 10000   # タスク帳（前回の比べと同じアプリ）
cargo run --release -p settings-kargui                    # 設定画面
cargo run --release -p reader-kargui                      # 長い文を読む画面
cargo run --release -p table-kargui                       # 1 万件の表
```

記録は `logs/<アプリ>.log` に 1 行ずつ書く（時刻・出来事・引数・結果）。

```text
2026-11-21T12:17:48.204Z t_ms=1795263468204 app=kargui-table event=ready args="app=table items=10000" result="since_main_ms=34"
```

## 説明

- ガイド: [日本語](docs/guide/ja/README.md)（元）・[English](docs/guide/en/README.md)（訳）
- API: `cargo doc -p kargui --open`
- AI 向けの索引: [docs/llms.txt](docs/llms.txt)
- kargui の開発に加わる時: [AGENTS.md](AGENTS.md)

## 検査

```powershell
cargo test --workspace --exclude kargui-spike   # 単体の試験・説明とガイドと README の例
cargo run -p xtask -- check                      # 説明と例の割合・部品の写真・AI 向けの索引・ガイドの古さ
```

日本語入力の確かめは前面の窓へキー入力をまねるので、作者が席を外している間だけ走らせる（`tools/away-run.ps1`）。

## 今の状態と今後

0.1 は最初の版で、ふつうのアプリを作るには足りない物がまだ多い。これまで試したのは、タスク帳と、形の違うアプリ 3 本（設定画面・長い文を読む画面・1 万件の表）だけ。今後は、実際のアプリを作りながら足りない部品と仕組みを調べ、よく使う物から足して完成度を上げていく。足すたびに、同じ道具で軽さが保たれているかを測る。

まだ無い主な物:

- **部品:** 複数行の入力欄、パスワードの欄、ドロップダウン、ラジオボタン、メニュー（メニューバー・右クリック）、ファイルを開く・保存するダイアログ、タブ、木の表示、列の見出しがある表、進み具合の棒、自由に描く面、リンク（押せない状態、浮き出し、窓の中のダイアログ、ツールチップ、アイコン、画像は 0.1 の後に足した。画像は GPU の道ではまだ描かない）
- **アプリの仕組み:** 複数の窓、ドラッグ＆ドロップ、書体の指定、窓のアイコン・最小の大きさ・閉じる前の確認、アニメーションの道具（ショートカット・タイマー・ほかのスレッドから描き直す仕組みは 0.1 の後に足した）
- **環境:** 窓を開けるのは Windows 10/11 だけ。GPU の道（既定ではない）では、まれに長い途切れが出る。読み上げはナレーターで確かめ、NVDA では確かめていない。アプリから再変換を始める仕組みが無い

### Linux と macOS

対応する予定。OS に頼る所（窓・入力・日本語入力・文字・画面に出す・クリップボード・読み上げの橋渡し）は、`platform.rs`・`gdi.rs`・`gpu.rs`・`text/dwrite.rs`・`wic.rs`（画像のファイルを読む）の 5 つのファイルにまとめてある。それ以外（部品・並べ方・描き方・試験台）は Linux と macOS でも組み立てられ、文字は parley と swash で並べて描く。送るたびに、Windows・Linux・macOS で試験を走らせる（GitHub Actions）。窓はまだ開けないので、Windows 以外で `kargui::run` を呼ぶと、対応していないことを伝える `Err` を返す。移植は Linux から行う。

0.x の間は API が変わることがある。変わった所は [CHANGELOG.md](CHANGELOG.md) に書く。

## ライセンス

MIT（[LICENSE](LICENSE)）。
