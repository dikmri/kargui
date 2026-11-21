// 測定の結果（bench.mjs が書く results/<tag>-*.json）を、場面と版ごとの中央値にまとめる。
// 段階 1 の bench/phase1-summary.json と同じ形で書く（ウォームアップの回は除く）。
// 使い方: node tools/summarize.mjs <tag> <出力の JSON> [説明]
//   例: node tools/summarize.mjs kargui2 bench/phase2-summary.json "段階 2 の終わり"
import fs from 'node:fs';
import path from 'node:path';

const [tag, out, note = ''] = process.argv.slice(2);
if (!tag || !out) {
  console.error('使い方: node tools/summarize.mjs <tag> <出力の JSON> [説明]');
  process.exit(1);
}
const root = process.env.RUST_GUI_ROOT ?? 'D:/soft/dekiru-html';
const dir = path.join(root, 'runtime/rust-gui/results');
const files = fs.readdirSync(dir).filter((f) => f.startsWith(`${tag}-`) && /^\d+\.json$/.test(f.slice(tag.length + 1)));
if (files.length === 0) {
  console.error(`${dir} に ${tag}-*.json が無い`);
  process.exit(1);
}
const median = (xs) => {
  const v = xs.filter((x) => typeof x === 'number' && Number.isFinite(x)).sort((a, b) => a - b);
  if (v.length === 0) return null;
  const m = Math.floor(v.length / 2);
  return v.length % 2 ? v[m] : (v[m - 1] + v[m]) / 2;
};
const max = (xs) => {
  const v = xs.filter((x) => typeof x === 'number' && Number.isFinite(x));
  return v.length ? Math.max(...v) : null;
};
const round = (x, d) => (x === null ? null : Math.round(x * 10 ** d) / 10 ** d);

const groups = new Map();
let machine = null;
for (const f of files.sort()) {
  const d = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
  machine ??= d.machine;
  for (const r of d.runs) {
    if (r.warmup) continue;
    const key = `${r.scenario}|${r.app}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(r);
  }
}
const results = {};
for (const [key, runs] of groups) {
  results[key] = {
    n: runs.length,
    startupMs: round(median(runs.map((r) => r.startupMs)), 0),
    memMB: round(median(runs.map((r) => r.mem?.pws / 2 ** 20)), 3), // 段階 1 と同じく 1 MB = 1,048,576 バイト
    gpuMB: round(median(runs.map((r) => (r.gpu?.dedicated ?? 0) / 2 ** 20)), 3),
    // 1 コア = 100%。待機中の CPU は中央値ではなく全部の回の最大で見る
    idleCpuPctMax: round(max(runs.map((r) => (r.idleCpuMsPerSec === undefined ? null : r.idleCpuMsPerSec / 10))), 3),
    fps: round(median(runs.map((r) => r.scroll?.fps)), 1),
    cpuMsPerFrame: round(median(runs.map((r) => r.scroll?.cpuMsPerFrame)), 3),
    maxGapMsWorst: round(max(runs.map((r) => r.scroll?.maxGapMs)), 1),
  };
}
const outPath = path.resolve(out);
fs.writeFileSync(outPath, JSON.stringify({ note, machine, results }, null, 1) + '\n');
for (const [k, v] of Object.entries(results)) {
  console.log(k.padEnd(34), JSON.stringify(v));
}
console.log(`→ ${outPath}（${files.length} ファイル）`);
