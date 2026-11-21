# AI の書きやすさの試験の結果を数える。runtime/ai-test/runs/ の全部の回から、
# 1 回目の組み立てが通ったか・通るまでの回数・最後に通ったか・画面のコードの行数（空行とコメントを除く。前回と同じ数え方）を出し、
# bench/ai-test-summary.json に書く。
# 使い方: python tools/ai-test/score.py
import glob
import io
import json
import os
import re

KARGUI = r'D:\soft\kargui'
RUNS = os.path.join(KARGUI, 'runtime', 'ai-test', 'runs')


def code_lines(path):
    """空行とコメントを除いた行数"""
    n = 0
    in_block = False
    ext = os.path.splitext(path)[1]
    for line in io.open(path, encoding='utf-8', errors='replace'):
        t = line.strip()
        if in_block:
            if '*/' in t or '-->' in t:
                in_block = False
            continue
        if not t:
            continue
        if ext in ('.rs', '.js') and t.startswith('//'):
            continue
        if t.startswith('/*') or (ext == '.html' and t.startswith('<!--')):
            if not ('*/' in t or '-->' in t):
                in_block = True
            continue
        n += 1
    return n


def ui_files(run):
    if os.path.isdir(os.path.join(run, 'src-tauri')):
        files = glob.glob(os.path.join(run, 'src-tauri', 'src', '*.rs'))
        for ext in ('js', 'html', 'css', 'mjs'):
            files += glob.glob(os.path.join(run, 'ui', '**', f'*.{ext}'), recursive=True)
        return files
    return glob.glob(os.path.join(run, 'src', '**', '*.rs'), recursive=True)


def main():
    runs = {}
    for run in sorted(glob.glob(os.path.join(RUNS, '*'))):
        name = os.path.basename(run)
        fw, model, n = name.split('-')
        attempts = []
        log = os.path.join(run, 'attempts.log')
        if os.path.exists(log):
            for line in io.open(log, encoding='utf-8-sig'):
                m = re.search(r'exit=(-?\d+) errors=(\d+)', line)
                if m:
                    attempts.append((int(m.group(1)), int(m.group(2))))
        ok_at = next((i + 1 for i, (code, _) in enumerate(attempts) if code == 0), None)
        reply = None
        rp = os.path.join(run, 'reply.json')
        if os.path.exists(rp):
            reply = json.load(io.open(rp, encoding='utf-8'))
        runs[name] = {
            'framework': fw,
            'model': model,
            'attempts': len(attempts),
            'first_build_ok': bool(attempts) and attempts[0][0] == 0,
            'ok_at_attempt': ok_at,
            'final_ok': bool(attempts) and attempts[-1][0] == 0,
            'first_errors': attempts[0][1] if attempts else None,
            'lines': sum(code_lines(f) for f in ui_files(run)),
            'reply': reply,
        }
    by = {}
    for r in runs.values():
        key = f"{r['framework']}|{r['model']}"
        b = by.setdefault(key, {'runs': 0, 'first_build_ok': 0, 'final_ok': 0, 'attempts': [], 'lines': []})
        b['runs'] += 1
        b['first_build_ok'] += r['first_build_ok']
        b['final_ok'] += r['final_ok']
        b['attempts'].append(r['ok_at_attempt'])
        b['lines'].append(r['lines'])
    out = {'runs': runs, 'summary': by}
    path = os.path.join(KARGUI, 'bench', 'ai-test-summary.json')
    io.open(path, 'w', encoding='utf-8', newline='\n').write(json.dumps(out, ensure_ascii=False, indent=1) + '\n')
    for k, b in sorted(by.items()):
        print(f"{k:16} runs={b['runs']} first_ok={b['first_build_ok']} final_ok={b['final_ok']} ok_at={b['attempts']} lines={b['lines']}")
    print('->', path)


if __name__ == '__main__':
    main()
