"""Resolve merge-conflict hunks the way a reference branch already resolved them.

Usage (in the worktree with the conflicted merge):
    python resolve_like.py <reference-rev> <file>...

For each hunk it builds "ours + theirs" and "theirs + ours" with the two context lines on each side
and accepts one only if that exact text appears in the reference file. Anything else (one side only,
a hand-rewritten line) stays as a conflict and is reported: resolve those by hand or take the whole
file from a reference that merges exactly the same commits. A looser match once dropped whole hunks
(a doc section, two validate arms), so do not loosen it; always run check_lines_kept.py afterwards.
"""
import re
import subprocess
import sys

HUNK = re.compile(r'<<<<<<< [^\n]*\n(.*?)(?:\|\|\|\|\|\|\| [^\n]*\n.*?)?=======\n(.*?)>>>>>>> [^\n]*\n', re.S)


def context(text, start, end):
    before = text[:start].split('\n')
    pre = '\n'.join(before[-3:])  # the two full lines before the hunk + '' (the hunk starts a line)
    post = '\n'.join(text[end:].split('\n')[:2])
    return pre, post


def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8')
    ref = sys.argv[1]
    total_left = 0
    for path in sys.argv[2:]:
        raw = open(path, encoding='utf-8', newline='').read()
        nl = '\r\n' if '\r\n' in raw else '\n'
        text = raw.replace('\r\n', '\n')
        shown = subprocess.run(['git', 'show', f'{ref}:{path}'], capture_output=True)
        refs = shown.stdout.decode('utf-8', 'replace').replace('\r\n', '\n') if shown.returncode == 0 else ''
        out, pos, left = [], 0, 0
        for m in HUNK.finditer(text):
            ours, theirs = m.group(1), m.group(2)
            pre, post = context(text, m.start(), m.end())
            choice = next((c for c in (ours + theirs, theirs + ours) if refs and pre + c + post in refs), None)
            out.append(text[pos:m.start()])
            if choice is None:
                out.append(m.group(0))
                left += 1
                line = text[:m.start()].count('\n') + 1
                print(f'  {path}:{line}: needs a hand resolution')
            else:
                out.append(choice)
            pos = m.end()
        out.append(text[pos:])
        open(path, 'w', encoding='utf-8', newline='').write(''.join(out).replace('\n', nl))
        print(f'{path}: {left} hunk(s) left')
        total_left += left
    sys.exit(1 if total_left else 0)


if __name__ == '__main__':
    main()
