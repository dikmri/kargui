# AI の書きやすさの試験で、各回の AI が開いたファイルと走らせたコマンドを並べ、読んではいけない所に触れていないかを見る。
# 各回のフォルダの agent.txt に、その回を走らせたエージェントの id を書いておく（やり取りの記録の名前に使う）。
# 使い方: python tools/ai-test/audit.py [回の名前の一部]
import glob
import io
import json
import os
import sys

KARGUI = r'D:\soft\kargui'
RUNS = os.path.join(KARGUI, 'runtime', 'ai-test', 'runs')
TRANSCRIPTS = glob.glob(os.path.expanduser(r'~\.claude\projects\*rust-gui-fw-plan*\*\subagents'))
# 読んではいけない所（試験の外の完成したタスク帳、ほかの回、git の記録）
FORBIDDEN = ['examples\\taskbook', 'examples/taskbook', 'reports\\rust-gui\\src\\egui-app', 'reports/rust-gui/src/egui-app',
             'gpui-app', 'tauri-app', '-pilot-', '.git', 'MyWiki', 'myWiki']


def transcript(agent):
    for d in TRANSCRIPTS:
        p = os.path.join(d, f'agent-{agent}.jsonl')
        if os.path.exists(p):
            return p
    return None


def main():
    pick = sys.argv[1] if len(sys.argv) > 1 else ''
    for run in sorted(glob.glob(os.path.join(RUNS, '*'))):
        name = os.path.basename(run)
        if pick not in name or '-pilot-' in name:
            continue
        idf = os.path.join(run, 'agent.txt')
        if not os.path.exists(idf):
            continue
        agent = io.open(idf, encoding='utf-8').read().strip()
        path = transcript(agent)
        if not path:
            print(f'{name}: 記録が見つからない（{agent}）')
            continue
        uses = []
        for line in io.open(path, encoding='utf-8'):
            try:
                m = json.loads(line).get('message', {})
            except ValueError:
                continue
            if m.get('role') != 'assistant':
                continue
            for c in m.get('content', []):
                if isinstance(c, dict) and c.get('type') == 'tool_use':
                    i = c.get('input', {})
                    uses.append((c.get('name'), str(i.get('file_path') or i.get('command') or i.get('pattern') or i.get('path') or '')))
        # 自分の回のフォルダの名前は除いてから、ほかの回のフォルダを探す
        # cargo を build.ps1 を通さずに呼ぶのも決まりの外
        bad = [(t, s) for t, s in uses if any(f in s for f in FORBIDDEN) or (t == 'Bash' and 'cargo ' in s and 'build.ps1' not in s) or any(
            os.path.basename(r) in s for r in glob.glob(os.path.join(RUNS, '*')) if os.path.basename(r) != name)]
        print(f'{name}: ツール {len(uses)} 回、読んではいけない所 {len(bad)} 回')
        for t, s in bad:
            print(f'   ! {t} {s[:200]}')


if __name__ == '__main__':
    main()
