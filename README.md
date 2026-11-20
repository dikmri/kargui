# kargui

Rust で書く、軽いネイティブ GUI フレームワーク（作成中）。名前は「軽い」「Rust」「GUI」から。

Tauri・egui・GPUI を同じアプリ（タスク帳）で比べた実験（[AI コーディングノートの実験レポート番外編](https://dikmri.github.io/ai-coding-notes/reports/rust-gui/)）を受けて、3 つの長所を残し、短所をなくすことを目指す。性能は前回と同じ道具・同じ条件で測り、各項目で前回の最良より 3 割良いことを目標にする。

## 今の状態

**段階 0（試作）。** フレームワークの形はまだ無い。`spike/` に、タスク帳の形の窓を 2 つの道で描く試作がある。

- GPU の道: Direct3D 11 を直接使う（`--renderer d3d11`、既定）
- CPU の道: CPU で描き、GDI で窓へ写す。スクロールは今の絵をずらし、新しく見えた帯だけを描く（`--renderer cpu` か `--software`）
- 文字: DirectWrite で並べ、形も DirectWrite で作る。形の表は 2 つの道で共有する
- 日本語入力: IMM32。変換を確定する Enter では追加しない。変換中の文字は入力欄の中に下線付きで出す
- 読み上げ: AccessKit で、窓の中の部品を UI オートメーションに渡す

対応する OS は、まず Windows 10/11 だけ。

## 動かし方

```powershell
cargo run --release -p kargui-spike -- --items 10000
```

記録は `logs/spike.log` に 1 行ずつ書く（時刻・出来事・引数・結果）。

```
2026-11-21T04:44:01.060Z t_ms=1795203841060 app=kargui event=ready args="renderer=d3d11 items=10000" result="since_main_ms=443 window_ms=7.7 renderer_ms=361.2"
```

## 測り方

前回の比べの道具（ai-coding-notes の `reports/rust-gui/bench/`）で、比べる相手と同じ回の中で測る。

```bash
bash tools/bench-phase0.sh kargui0   # 起動・メモリ・CPU・スクロール・低い性能のまね
node tools/buildtime.mjs             # ビルドの時間・crate の数・実行ファイルの大きさ
```

読み上げの部品の木は `tools/uia-dump.ps1` で書き出せる。

## ライセンス

MIT OR Apache-2.0（公開のときにライセンスの文書を置く）。
