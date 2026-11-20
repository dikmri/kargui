// kargui の試作のビルド時間と、依存している crate の数を測る。前回の比べ（ai-coding-notes の
// reports/rust-gui/bench/buildtime.mjs）と同じ手順: crate のダウンロードは済んだ状態で、
// 何もない所からのリリースビルド・デバッグビルドと、main.rs に 1 行足したあとのデバッグビルドを測る。
// ほかの重い処理と同時に走らせないこと。
// 使い方（このリポジトリの最上位で）: node tools/buildtime.mjs [パッケージ名]
// 結果は runtime/build-times.json に書く。
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const R = (...p) => path.join(ROOT, ...p);
const OUT = R('runtime/build-times.json');
const pkg = process.argv[2] ?? 'kargui-spike';
const MAIN = { 'kargui-spike': 'spike/src/main.rs', 'text-probe': 'probes/text-probe/src/main.rs', 'taskbook-kargui': 'examples/taskbook/src/main.rs' }[pkg];
const results = fs.existsSync(OUT) ? JSON.parse(fs.readFileSync(OUT, 'utf8')) : {};

function cargo(args, targetDir) {
  const t0 = Date.now();
  const r = spawnSync('cargo', [...args, '-p', pkg], {
    cwd: ROOT,
    env: { ...process.env, CARGO_TARGET_DIR: targetDir },
    encoding: 'utf8',
  });
  const sec = (Date.now() - t0) / 1000;
  if (r.status !== 0) throw new Error(`${args.join(' ')} failed\n${r.stderr.slice(-2000)}`);
  return sec;
}

const out = (results[pkg] = { measuredAt: new Date().toISOString() });
const tree = spawnSync('cargo', ['tree', '-p', pkg, '-e', 'normal,build', '--prefix', 'none', '--target', 'x86_64-pc-windows-msvc'], {
  cwd: ROOT,
  encoding: 'utf8',
});
const names = new Set(
  tree.stdout
    .split('\n')
    .map((l) => l.replace(/ \(\*\)$/, '').replace(/ \(proc-macro\)$/, '').trim())
    .filter(Boolean),
);
out.crates = names.size;
out.crateList = [...names].sort();

const rel = R(`runtime/bt/${pkg}-release`);
fs.rmSync(rel, { recursive: true, force: true });
out.cleanReleaseSec = cargo(['build', '--release'], rel);
console.log(`${pkg}: clean release ${out.cleanReleaseSec}s`);
const exe = path.join(rel, 'release', `${pkg}.exe`);
if (fs.existsSync(exe)) out.exeBytes = fs.statSync(exe).size;

const dbg = R(`runtime/bt/${pkg}-debug`);
fs.rmSync(dbg, { recursive: true, force: true });
out.cleanDebugSec = cargo(['build'], dbg);
console.log(`${pkg}: clean debug ${out.cleanDebugSec}s`);

const mainPath = R(MAIN);
const orig = fs.readFileSync(mainPath, 'utf8');
try {
  fs.writeFileSync(mainPath, orig + '\n// build-time probe\n');
  out.incrDebugSec = cargo(['build'], dbg);
} finally {
  fs.writeFileSync(mainPath, orig);
}
console.log(`${pkg}: incremental debug ${out.incrDebugSec}s, crates ${out.crates}, exe ${out.exeBytes} bytes`);
fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, JSON.stringify(results, null, 1));
