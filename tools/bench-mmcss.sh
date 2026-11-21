#!/usr/bin/env bash
# 段階 2: MMCSS のあり・なしを、1 コアのスクロールで交互に測る（回ごとのばらつきが大きいため）
# 使い方（Git Bash）: bash tools/bench-mmcss.sh [組の数]
set -u
ROUNDS=${1:-4}
export RUST_GUI_ROOT=${RUST_GUI_ROOT:-D:/soft/dekiru-html}
BENCH=${BENCH:-D:/soft/dekiru-html/.claude/worktrees/rust-gui-fw-plan-acd5f3/reports/rust-gui/bench/bench.mjs}
cd "$(dirname "$0")/.." || exit 1
LOG=logs/bench-mmcss.log
for r in $(seq 1 "$ROUNDS"); do
  for mode in off on; do
    if [ "$mode" = off ]; then export KARGUI_MMCSS=0; else unset KARGUI_MMCSS; fi
    echo "=== $(date -Iseconds) round=$r mmcss=$mode" | tee -a "$LOG"
    node "$BENCH" --tag "mmcss-$mode-$r" --scenarios scroll_1core --apps kargui-tb-full 2>&1 | grep -E "#[0-9]:" | tee -a "$LOG"
  done
done
echo "=== $(date -Iseconds) done" | tee -a "$LOG"
