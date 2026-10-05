"""After merging branches: list lines a source branch added that the merged result no longer has.

Usage (in any worktree of the repo):
    python check_lines_kept.py <base> <result-rev> <branch>... [-- <file>...]

<base> must be what the branches have in common (the branch they all forked from), NOT main when
they sit on another PR: with main as base, the parent PR's own lines that a child rewrote on purpose
flood the report. Without files, every file that differs between <base> and <result-rev> is checked.
Lines are compared stripped, whitespace-only lines ignored. Expected hits: a line both branches edited
(the merged line combines them) and lines a later branch rewrote; read each one, anything else is a
dropped change.
"""
import collections
import subprocess
import sys


def git(*args):
    return subprocess.run(['git', *args], capture_output=True).stdout.decode('utf-8', 'replace').replace('\r\n', '\n')


def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8')
    args = sys.argv[1:]
    if '--' in args:
        i = args.index('--')
        revs, files = args[:i], args[i + 1:]
    else:
        revs, files = args, []
    base, result, branches = revs[0], revs[1], revs[2:]
    if not files:
        files = [f for f in git('diff', '--name-only', base, result).split('\n') if f]
    if not files:
        print('no files to check (wrong directory or revisions?)')
        sys.exit(2)
    missing = 0
    for f in files:
        have = collections.Counter(l.strip() for l in git('show', f'{result}:{f}').split('\n') if l.strip())
        for b in branches:
            diff = git('diff', '-U0', f'{base}...{b}', '--', f)
            added = {l[1:].strip() for l in diff.split('\n') if l.startswith('+') and not l.startswith('+++') and l[1:].strip()}
            for line in sorted(added):
                if not have[line]:
                    missing += 1
                    print(f'{f} [{b}] missing: {line[:160]}')
    print(f'checked {len(files)} file(s); missing lines: {missing}')


if __name__ == '__main__':
    main()
