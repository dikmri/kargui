#!/usr/bin/env bash
# rustdoc の「説明の割合」を 4 つの GUI フレームワークの公開 API で数える台本。
# kargui（このワークスペース）と、egui・gpui-pre・gpui-kit・tauri（手元の registry にある版）を
# `cargo rustdoc --offline -- -Z unstable-options --show-coverage --output-format json` で数え、
# bench/doc-coverage.json に集計する。外の crate は runtime/doc-coverage/<名前>/ に使い捨てのプロジェクトを作る。
# 使い方: bash tools/doc-coverage.sh   （nightly なしで RUSTC_BOOTSTRAP=1 を使う。ダウンロードはしない）
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/runtime/doc-coverage"
export RUSTC_BOOTSTRAP=1
export CARGO_TARGET_DIR="$WORK/target"
mkdir -p "$WORK/out"

# 名前 crate 版 [features]
EXTERNAL=(
  "egui egui 0.37.0 "
  "gpui-pre gpui-pre 0.3.12 "
  "gpui-kit gpui-kit 0.10.0 "
  "tauri tauri 2.12.7 "
)

run_cov() { # dir pkg out
  (cd "$1" && cargo rustdoc -p "$2" --offline -- -Z unstable-options --show-coverage --output-format json >"$3" 2>"$3.err")
}

# kargui は自分のワークスペースの target を使う
( unset CARGO_TARGET_DIR; run_cov "$ROOT" kargui "$WORK/out/kargui.json" ) || echo "kargui: 失敗" >&2

for spec in "${EXTERNAL[@]}"; do
  read -r name crate ver feats <<<"$spec"
  d="$WORK/$name"
  mkdir -p "$d/src"
  [ -f "$d/src/lib.rs" ] || : >"$d/src/lib.rs"
  # features は手で足した場合に消さないよう、Cargo.toml が無いときだけ作る
  if [ ! -f "$d/Cargo.toml" ]; then
    cat >"$d/Cargo.toml" <<TOML
[package]
name = "cov-$name"
version = "0.0.0"
edition = "2021"

[dependencies]
$crate = "=$ver"

[workspace]
TOML
  fi
  run_cov "$d" "$crate" "$WORK/out/$name.json" || echo "$name: 失敗（$WORK/out/$name.json.err を見る）" >&2
done

node - "$ROOT" "$WORK" <<'JS'
const fs = require('fs'), path = require('path');
const [root, work] = process.argv.slice(2);
const versions = { kargui: '0.0.1', egui: '0.37.0', 'gpui-pre': '0.3.12', 'gpui-kit': '0.10.0', tauri: '2.12.7' };
const crates = {};
for (const [name, version] of Object.entries(versions)) {
  const f = path.join(work, 'out', name + '.json');
  let j;
  try { j = JSON.parse(fs.readFileSync(f, 'utf8')); } catch { crates[name] = { version, error: 'not counted' }; continue; }
  let t = 0, d = 0, te = 0, e = 0;
  for (const v of Object.values(j)) { t += v.total; d += v.with_docs; te += v.total_examples; e += v.with_examples; }
  const pct = (a, b) => b ? Math.round(a / b * 1000) / 10 : 0;
  crates[name] = { version, items: t, documented: d, documented_pct: pct(d, t), examples: e, examples_pct: pct(e, te), examples_eligible: te };
}
const note = 'rustdoc --show-coverage の Total。examples_pct は例の対象になる項目（examples_eligible）に対する割合で、全項目に対する割合ではない。'
  + ' 2026-11-21 測定。';
fs.writeFileSync(path.join(root, 'bench', 'doc-coverage.json'), JSON.stringify({ note, crates }, null, 2) + '\n');
console.log(JSON.stringify(crates, null, 2));
JS
