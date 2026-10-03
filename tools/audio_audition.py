"""Write a local audition page for the decoded game audio (dev tool).

    py -3.13 tools/audio_audition.py [--assets assets] [--out .local/audio-audition/index.html]

The page lists every cue in crates/skate-game/src/game_audio/cues.rs with its
current samples, then each bank in full, the rolling grains and ambience beds.
Players start at 25% volume. It links the WAVs in the local install; nothing
is copied, and the page belongs in the gitignored .local folder.
"""
import argparse
import html
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CUES = ROOT/'crates/skate-game/src/game_audio/cues.rs'


def cue_table():
    """(name, bank, [indices], comment) for each `Cue` constant in cues.rs."""
    text = CUES.read_text(encoding='utf-8')
    pattern = re.compile(r'((?:///[^\n]*\n)*)pub\(super\) const (\w+): Cue = Cue \{ bank: "(\w+)", samples: ([^,]+(?:, \d+)*\]?), level')
    out = []
    for doc, name, bank, samples in pattern.findall(text):
        found = re.match(r'range!\((\d+)\.\.=(\d+)\)', samples)
        indices = list(range(int(found[1]), int(found[2]) + 1)) if found else [int(n) for n in re.findall(r'\d+', samples)]
        out.append((name, bank, indices, ' '.join(line.strip('/ ') for line in doc.splitlines())))
    return out


def groups(entries):
    """(first, last) runs of consecutive clips with similar length (within 25%)."""
    runs, start = [], 0
    for i in range(1, len(entries) + 1):
        if i == len(entries) or abs(entries[i]['seconds'] - entries[i - 1]['seconds']) > 0.25 * max(entries[i - 1]['seconds'], 0.05):
            runs.append((start, i - 1))
            start = i
    return runs


def player(path: Path, label: str, note: str = '') -> str:
    return (f'<div class="s"><span class="l">{html.escape(label)}</span>'
            f'<audio controls preload="none" src="{path.as_uri()}"></audio>'
            f'<span class="n">{html.escape(note)}</span></div>')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--assets', type=Path, default=ROOT/'assets')
    parser.add_argument('--out', type=Path, default=ROOT/'.local/audio-audition/index.html')
    args = parser.parse_args()
    audio = (args.assets/'private/audio').resolve()
    manifest = json.loads((audio/'audio_manifest.json').read_text(encoding='utf-8'))
    parts = ['<h1>Skate 3 audio audition</h1><p>Players start at 25% volume. Note the bank and index of '
             'the sounds that fit each cue.</p><h2>Current cue picks</h2>']
    for name, bank, indices, doc in cue_table():
        parts.append(f'<h3>{name} <small>{bank} {indices[0]}..{indices[-1]} — {html.escape(doc)}</small></h3>')
        for i in indices:
            entry = manifest['banks'][bank][i]
            parts.append(player(audio/entry['file'], f'{bank} {i}', f"{entry['seconds']:.2f}s"))
    parts.append('<h2>Rolling grains</h2><p>Each surface: speed bands from slow (0) to fast.</p>')
    for name, grain in manifest['grains'].items():
        for band, entry in enumerate(grain['bands']):
            parts.append(player(audio/entry['file'], f'{name} band {band}', f"{entry['seconds']:.1f}s loop"))
    parts.append('<h2>Ambience beds</h2>')
    for name, entry in manifest['ambience'].items():
        parts.append(player(audio/entry['file'], name, f"{entry['seconds']:.0f}s"))
    parts.append('<h2>Bank groups</h2><p>Runs of similar clips (one player each; the range is the '
                 'whole run). Use these to find candidates quickly.</p>')
    for bank, entries in manifest['banks'].items():
        runs = groups(entries)
        parts.append(f'<details><summary>{bank}: {len(runs)} groups</summary>')
        for first, last in runs:
            e = entries[first]
            label = f'{bank} {first}' + (f'–{last}' if last > first else '')
            parts.append(player(audio/e['file'], label, f"{e['seconds']:.2f}s, {last - first + 1} clip(s)"))
        parts.append('</details>')
    parts.append('<h2>Banks (every clip)</h2>')
    for bank, entries in manifest['banks'].items():
        parts.append(f'<details><summary>{bank} ({len(entries)})</summary>')
        parts.extend(player(audio/e['file'], f'{bank} {i}', f"{e['seconds']:.2f}s") for i, e in enumerate(entries))
        parts.append('</details>')
    page = ('<!doctype html><meta charset="utf-8"><title>Audio audition</title><style>'
            'body{font:14px system-ui;margin:24px;background:#111;color:#ddd}h3 small{color:#999;font-weight:normal}'
            '.s{display:flex;gap:12px;align-items:center;margin:2px 0}.l{width:260px}.n{color:#999}'
            'audio{height:28px}summary{cursor:pointer;margin:6px 0}</style>' + ''.join(parts) +
            '<script>for(const a of document.querySelectorAll("audio")){a.volume=0.25;'
            'a.addEventListener("play",()=>{for(const b of document.querySelectorAll("audio"))if(b!==a)b.pause()})}</script>')
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(page, encoding='utf-8')
    print(args.out)


if __name__ == '__main__':
    main()
