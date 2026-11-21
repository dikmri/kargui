#!/usr/bin/env bash
# 段階 2: GPU の道の待ち方（変更前: Present のあと WaitForVBlank / 変更後: スワップチェーンの合図を待つ）を交互に測る
# 使い方（Git Bash）: bash tools/bench-gpu-wait.sh [組の数]。実行ファイルは runtime/ab/ に置く
set -u
ROUNDS=${1:-3}
export RUST_GUI_ROOT=${RUST_GUI_ROOT:-D:/soft/dekiru-html}
BENCH=${BENCH:-D:/soft/dekiru-html/.claude/worktrees/rust-gui-fw-plan-acd5f3/reports/rust-gui/bench/bench.mjs}
cd "$(dirname "$0")/.." || exit 1
LOG=logs/bench-gpu-wait.log
for r in $(seq 1 "$ROUNDS"); do
  for v in ${VARIANTS:-before wait}; do
    export KARGUI_TB="$(pwd -W)/runtime/ab/taskbook-gpu-$v.exe"
    echo "=== $(date -Iseconds) round=$r gpu=$v" | tee -a "$LOG"
    node "$BENCH" --tag "gpuwait-$v-$r" --scenarios scroll,scroll_1core --apps kargui-tb-gpu-full 2>&1 | grep -E "#[0-9]:" | tee -a "$LOG"
  done
done
echo "=== $(date -Iseconds) done" | tee -a "$LOG"
