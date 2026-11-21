# AI の書きやすさの試験（計画 4.2）の 1 回分のフォルダを作る。
# 使い方: python tools/ai-test/prepare.py <kargui|egui|gpui|tauri> <sonnet|haiku> <回>
# runtime/ai-test/runs/<版>-<モデル>-<回>/ に、ひな形・build.ps1・prompt.txt（AI に渡す文）を作る。
# 組み立ての置き場所は runtime/ai-test/target-<版>（回をまたいで使い回し、依存の組み立てを 1 回で済ませる）。
# 比べる相手の依存の版と Cargo.lock は、前回の比べ（ai-coding-notes の reports/rust-gui/src）と同じにする。
import glob
import io
import os
import shutil
import sys

KARGUI = r'D:\soft\kargui'
REPORT = r'D:\soft\dekiru-html\reports\rust-gui\src'
REG = glob.glob(os.path.expanduser(r'~\.cargo\registry\src\index.crates.io-*'))[0]
RUNS = os.path.join(KARGUI, 'runtime', 'ai-test', 'runs')


def cargo_toml(name, deps, extra=''):
    return f'''[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
publish = false

{extra}[dependencies]
{deps}
[workspace]
'''


STUB = '// Write the Task Book app here.\nfn main() {}\n'

FW = {
    'kargui': dict(
        name='kargui',
        taskcore=KARGUI + r'\taskcore',
        crate='',
        entry='`Cargo.toml` and `src\\main.rs`',
        docs=[
            KARGUI + r'\docs\guide\en\ (the guide; start with README.md)',
            KARGUI + r'\docs\llms.txt (a summary written for AI)',
            KARGUI + r'\crates\kargui\src\ (the API documentation is in the doc comments)',
        ],
        cargo=cargo_toml('taskbook-ai', 'kargui = { path = "D:/soft/kargui/crates/kargui" }\ntaskcore = { path = "D:/soft/kargui/taskcore" }\n'),
        lock=None,
    ),
    'egui': dict(
        name='egui (with eframe)',
        taskcore=REPORT + r'\common',
        crate='',
        entry='`Cargo.toml` and `src\\main.rs`',
        docs=[
            REG + r'\egui-0.37.0\ (README.md and src\; the API documentation is in the doc comments)',
            REG + r'\eframe-0.37.0\ (README.md and src\)',
        ],
        cargo=cargo_toml(
            'taskbook-ai',
            'eframe = { version = "=0.37.0", default-features = false, features = ["accesskit", "clipboard", "default_fonts", "links", "system_fonts", "wgpu"] }\n'
            'taskcore = { path = "D:/soft/dekiru-html/reports/rust-gui/src/common" }\n',
        ),
        lock=REPORT + r'\egui-app\Cargo.lock',
    ),
    'gpui': dict(
        name='GPUI (gpui-kit, which re-exports gpui-pre)',
        taskcore=REPORT + r'\common',
        crate='',
        entry='`Cargo.toml` and `src\\main.rs`',
        docs=[
            REG + r'\gpui-kit-0.10.0\ (README.md, TESTING.md and src\)',
            REG + r'\gpui-pre-0.3.12\ (README.md, docs\, examples\ and src\)',
        ],
        cargo=cargo_toml(
            'taskbook-ai',
            'gpui-kit = "=0.10.0"\ntaskcore = { path = "D:/soft/dekiru-html/reports/rust-gui/src/common" }\nlog = "0.4"\n',
        ),
        lock=REPORT + r'\gpui-app\Cargo.lock',
    ),
    'tauri': dict(
        name='Tauri 2',
        taskcore=REPORT + r'\common',
        crate='src-tauri',
        entry='`src-tauri\\` (Rust; `tauri.conf.json` serves the page from `..\\ui` and turns on `withGlobalTauri`) and `ui\\index.html`',
        docs=[
            REG + r'\tauri-2.12.7\ (README.md, src\ and scripts\; the API documentation is in the doc comments)',
            REG + r'\tauri-build-2.7.6\ (README.md and src\)',
        ],
        cargo=None,
        lock=None,
    ),
}


def write(path, text, bom=False):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    io.open(path, 'w', encoding='utf-8-sig' if bom else 'utf-8', newline='\n').write(text)


def main():
    fw, model, n = sys.argv[1], sys.argv[2], sys.argv[3]
    f = FW[fw]
    run = os.path.join(RUNS, f'{fw}-{model}-{n}')
    if os.path.exists(run):
        sys.exit(f'{run} がもうある')
    if fw == 'tauri':
        src = os.path.join(REPORT, 'tauri-app', 'src-tauri')
        dst = os.path.join(run, 'src-tauri')
        os.makedirs(dst)
        for name in ['build.rs', 'tauri.conf.json', 'Cargo.lock']:
            shutil.copy(os.path.join(src, name), dst)
        for d in ['capabilities', 'icons']:
            if os.path.isdir(os.path.join(src, d)):
                shutil.copytree(os.path.join(src, d), os.path.join(dst, d))
        write(os.path.join(dst, 'Cargo.toml'), cargo_toml(
            'taskbook-ai',
            'tauri = { version = "=2.12.7", features = [] }\ntaskcore = { path = "D:/soft/dekiru-html/reports/rust-gui/src/common" }\n',
            extra='[build-dependencies]\ntauri-build = { version = "=2.7.6", features = [] }\n\n'))
        write(os.path.join(dst, 'src', 'main.rs'), STUB)
        write(os.path.join(run, 'ui', 'index.html'), '<!doctype html>\n<title>Task Book</title>\n<!-- Write the page here. -->\n')
    else:
        write(os.path.join(run, 'Cargo.toml'), f['cargo'])
        write(os.path.join(run, 'src', 'main.rs'), STUB)
        if f['lock']:
            shutil.copy(f['lock'], os.path.join(run, 'Cargo.lock'))
    target = os.path.join(KARGUI, 'runtime', 'ai-test', f'target-{fw}')
    crate = os.path.join(run, f['crate']) if f['crate'] else run
    write(os.path.join(run, 'build.ps1'), f'''# AI の書きやすさの試験の組み立て（cargo build --offline）。毎回 attempts.log に 1 行書き、出力を build-N.txt に残す
$env:CARGO_TARGET_DIR = '{target}'
Set-Location '{crate}'
$n = 1 + @(Get-Content '{run}\\attempts.log' -ErrorAction SilentlyContinue).Count
$out = & cargo build --offline 2>&1 | Out-String
$code = $LASTEXITCODE
$errors = ([regex]::Matches($out, '(?m)^error(\\[E\\d+\\])?:')).Count
"$(Get-Date -Format o) attempt=$n exit=$code errors=$errors" | Add-Content '{run}\\attempts.log'
$out | Set-Content '{run}\\build-$n.txt'
$out
exit $code
''', bom=True)
    task = io.open(os.path.join(KARGUI, 'tools', 'ai-test', 'task.md'), encoding='utf-8').read()
    task = task.replace('{FRAMEWORK}', f['name']).replace('{TASKCORE}', f['taskcore'])
    prompt = io.open(os.path.join(KARGUI, 'tools', 'ai-test', 'prompt.md'), encoding='utf-8').read()
    prompt = (prompt.replace('{RUN}', run).replace('{FRAMEWORK}', f['name']).replace('{TASKCORE}', f['taskcore'])
              .replace('{ENTRY}', f['entry']).replace('{DOCS}', '\n'.join('- ' + d for d in f['docs'])).replace('{TASK}', task))
    write(os.path.join(run, 'prompt.txt'), prompt)
    print(run)


if __name__ == '__main__':
    main()
