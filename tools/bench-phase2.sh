#!/usr/bin/env bash
# 段階 2 の終わりの測り直し（段階 1 と同じ場面と相手）。前回の比べの道具（ai-coding-notes の reports/rust-gui/bench/bench.mjs）で、
# kargui の試作と、比べる相手（前回の各条件の最良の版）を同じ回の中で測る。
# 使い方（Git Bash）: bash tools/bench-phase2.sh [tag]
# 結果: $RUST_GUI_ROOT/runtime/rust-gui/results/<tag>-*.json、進み具合はこのリポジトリの logs/bench-phase2.log
set -u
TAG=${1:-kargui2}
export RUST_GUI_ROOT=${RUST_GUI_ROOT:-D:/soft/dekiru-html}
BENCH=${BENCH:-D:/soft/dekiru-html/.claude/worktrees/rust-gui-fw-plan-acd5f3/reports/rust-gui/bench/bench.mjs}
cd "$(dirname "$0")/.." || exit 1
mkdir -p logs
LOG=logs/bench-phase2.log
run() {
  echo "=== $(date -Iseconds) $*" | tee -a "$LOG"
  node "$BENCH" --tag "$TAG" "$@" 2>&1 | tee -a "$LOG"
}
# 起動・メモリ・待機中の CPU（空・1 万件・10 万件）
run --scenarios startup,items10k,items100k --apps kargui-tb,kargui-tb-gpu,gpui,egui-glow
# スクロール（1 フレームの CPU。抑えない設定どうしで比べる）
run --scenarios scroll --apps kargui-tb-full,kargui-tb-gpu-full,kargui-tb,gpui-full,egui
# 低い性能のまね（CPU 2 コア・1 コア）
run --scenarios startup_2core,startup_1core --apps kargui-tb,kargui-tb-gpu,egui-glow,gpui
run --scenarios scroll_1core --apps kargui-tb-full,kargui-tb-gpu-full,egui,gpui-full
# GPU なし（kargui は CPU の道。--software で選ばれる）
run --scenarios startup_soft --apps kargui-tb,gpui-warp,tauri
run --scenarios scroll_soft --apps kargui-tb-full,gpui-warp,tauri
echo "=== $(date -Iseconds) done" | tee -a "$LOG"
