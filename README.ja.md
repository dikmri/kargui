# kargui

Rust で書く、軽いネイティブ GUI フレームワーク（作成中、Windows 10/11）。名前は「軽い」「Rust」「GUI」から。[English README](README.md)（この文の訳）。

Tauri・egui・GPUI を同じアプリ（タスク帳）で比べた実験（[AI コーディングノートの実験レポート番外編](https://dikmri.github.io/ai-coding-notes/reports/rust-gui/)）を受けて、3 つの長所を残し、短所をなくすことを目指す。性能は前回と同じ道具・同じ条件で測り、各項目で前回の最良より 3 割良いことを目標にする。

## 特徴

- **即時モードの書き方。** 状態は自分の構造体に持ち、`view` で毎回画面全体を書く（egui と同じ書きやすさ）。
- **変わった所だけ描く。** 中で前の画面と比べ、既定の CPU の道で変わった所だけを描き直す。待機中は CPU を使わない。GPU の無い PC でも速い。
- **日本語入力。** 変換を確定する Enter で送信しない、変換中は値を変えない、文節ごとの下線、候補の窓を文字の真下に出す、焦点を移しても変換中の文字を失わない。
- **読み上げとキーボード。** AccessKit で全部の部品を UI オートメーションに渡す。名前の無い部品は記録で警告する。Tab で全部に届く。
- **説明が古くならない。** 公開項目すべてに説明と動く例があり、ガイドのコードも試験で動かす。API が変わったのにガイドを直していないと、検査で落ちる。

## 部品

見出し・ラベル（折り返し、大きさ、右寄せ）・ボタン・チェック・スイッチ・選択（segmented）・入力欄・スライダー・一覧（見えている行だけ作る、行の選択、行を id で区別）・スクロールの入れ物。並べ方は行・列・重ね・残りを分ける（2 回で決める配置）。写真は [docs/widgets.md](docs/widgets.md)。

## 動かし方

```powershell
cargo run --release -p taskbook-kargui -- --items 10000   # タスク帳（前回の比べと同じアプリ）
cargo run --release -p settings-kargui                    # 設定画面
cargo run --release -p reader-kargui                      # 長い文を読む画面
cargo run --release -p table-kargui                       # 1 万件の表
```

記録は `logs/<アプリ>.log` に 1 行ずつ書く（時刻・出来事・引数・結果）。

```
2026-11-21T12:17:48.204Z t_ms=1795263468204 app=kargui-table event=ready args="app=table items=10000" result="since_main_ms=34"
```

## 説明

- ガイド: [日本語](docs/guide/ja/README.md)（元）・[English](docs/guide/en/README.md)（訳）
- API: `cargo doc -p kargui --open`
- AI 向けの索引: [docs/llms.txt](docs/llms.txt)

## 検査

```powershell
cargo test --workspace --exclude kargui-spike   # 単体の試験・説明とガイドの例
cargo run -p xtask -- check                      # 説明と例の割合・部品の写真・AI 向けの索引・ガイドの古さ
```

日本語入力の確かめは前面の窓へキー入力をまねるので、作者が席を外している間だけ走らせる（`tools/away-run.ps1`）。

## 今の状態

段階 3（書きやすさと説明）を終えた。計画と測った数字は作者の MyWiki（rust-gui-framework）にある。GitHub への公開は段階 4。

## ライセンス

MIT OR Apache-2.0（[LICENSE-MIT](LICENSE-MIT)、[LICENSE-APACHE](LICENSE-APACHE)）。
