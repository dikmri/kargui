#!/usr/bin/env bash
# 段階 2: スクロール中の 1 フレームの仕事の時間（アプリが記録する anim.summary の work_ms_per_frame）を版ごとに比べる。
# 測定の道具の CPU の時間は 15.6 ms 単位で粗く、1 ms 前後の差が測れないため。
# 版: now（今）・nommcss（今の版で KARGUI_MMCSS=0）・no-overscan（一覧の見えない行を作らない版）。実行ファイルは runtime/ab/
# 使い方（Git Bash）: bash tools/bench-work.sh [組の数]
set -u
ROUNDS=${1:-3}
export RUST_GUI_ROOT=${RUST_GUI_ROOT:-D:/soft/dekiru-html}
BENCH=${BENCH:-D:/soft/dekiru-html/.claude/worktrees/rust-gui-fw-plan-acd5f3/reports/rust-gui/bench/bench.mjs}
cd "$(dirname "$0")/.." || exit 1
LOG=logs/bench-work.log
for r in $(seq 1 "$ROUNDS"); do
  for v in now nommcss no-overscan; do
    exe=$v; unset KARGUI_MMCSS
    if [ "$v" = nommcss ]; then exe=now; export KARGUI_MMCSS=0; fi
    export KARGUI_TB="$(pwd -W)/runtime/ab/taskbook-$exe.exe"
    node "$BENCH" --tag "work-$v-$r" --scenarios scroll --apps kargui-tb-full > /dev/null 2>&1
    for f in "$RUST_GUI_ROOT"/logs/rust-gui/bench/work-$v-$r-scroll-kargui-tb-full-[1-9].log; do
      echo "$v $(grep -o 'work_ms_per_frame=[0-9.]*' "$f" | tail -1) $(grep -o 'build_ms_per_frame=[0-9.]*' "$f" | tail -1)" | tee -a "$LOG"
    done
  done
done
