"""Per-location statistics of a long recomp run (WPFIRE / WPINT / WPPOS / AMBST lines) against our data.

usage: py -3.13 long_run_stats.py <trace.tsv> [...more traces]

For each teleport stop (MARK "at <name>"): the retail location set, fires and their sounds vs the set's
weights, drawn intervals vs the set's min..max, sounds never fired, and the ambience zone retail used vs
our `audio_ambience` region lookup at the same positions. Uses the dev install's audio manifest.
"""
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO))
from tools.asset_pipeline.audio_formats import region_key  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_sets_vs_trace import STOP_DISTRICT  # noqa: E402


def load_manifest() -> dict:
    marker = json.loads((REPO / 'data/installation.json').read_text())
    return json.loads((REPO / 'data' / marker['directory'] / 'assets/private/audio/audio_manifest.json').read_text())


def main(traces: list[Path]) -> None:
    m = load_manifest()
    sets, zones = m['random_sets'], m.get('zones', {})
    tiles = {d: {layer: [{'box': t['box'], 'nodes': t['nodes'], 'keys': [int(k, 16) for k in t['keys']]}
                         for t in tl] for layer, tl in layers.items()} for d, layers in m['regions'].items()}
    sound_bank = {s['sound_id']: s['bank'] for st in sets.values() for s in st['sounds']}
    zname = lambda k: (zones.get(k) or {}).get('name') or k
    total = Counter()
    for trace in traces:
        stop, stale = None, None
        data = defaultdict(lambda: {'set': Counter(), 'fires': [], 'intervals': [], 'amb': Counter(), 'ours_amb': Counter()})
        for line in trace.open(encoding='utf-8', errors='replace'):
            f = line.rstrip('\n').split('\t')
            if f[0] == 'MARK' and len(f) > 2 and f[2].startswith('at '):
                stop = f[2][3:].split('#')[0].strip()
            elif stop is None:
                continue
            elif f[0] == 'WPPOS':
                x, _, z = (float(v) for v in f[2].split())
                if f[3] != '0000000000000000':
                    data[stop]['set'][f[3]] += 1
                district = STOP_DISTRICT.get(stop, '')
                key = next((region_key(t, x, z) for t in tiles.get(district, {}).get('audio_ambience', [])
                            if region_key(t, x, z) is not None), None)
                data[stop]['ours_amb'][zname(f'{key:016X}') if key else 'none'] += 1
            elif f[0] == 'WPFIRE':
                data[stop]['fires'].append(sound_bank.get(f[2], f[2]))
            elif f[0] == 'WPINT':
                data[stop]['intervals'].append(float(f[2]))
            elif f[0] == 'AMBST' and f[2] != '0000000000000000':
                data[stop]['amb'][zname(f[2])] += 1
        print(f'=== {trace.parent.name}')
        for stop, d in data.items():
            if not d['set']:
                continue
            key = d['set'].most_common(1)[0][0]
            s = sets.get(key, {})
            weights = {x['bank']: x['weight'] for x in s.get('sounds', [])}
            fired = Counter(d['fires'])
            lo, hi = s.get('min_interval'), s.get('max_interval')
            ivs = [v for v in d['intervals'] if v > 0]
            outside = [v for v in ivs if lo is not None and not (min(lo, hi) - 1e-3 <= v <= max(lo, hi) + 1e-3)]
            never = sorted(b for b in weights if b not in fired)
            print(f"{stop:24s} set {s.get('name')} ({lo}-{hi} s): {len(d['fires'])} fires, {len(ivs)} intervals"
                  f" {min(ivs, default=0):.2f}-{max(ivs, default=0):.2f} s, {len(outside)} outside range")
            print(f"{'':24s} fired: {', '.join(f'{b}×{n}' for b, n in fired.most_common())}")
            print(f"{'':24s} never fired ({len(never)}/{len(weights)}): {', '.join(never)}")
            print(f"{'':24s} ambience retail {dict(d['amb'])} / ours at positions {dict(d['ours_amb'])}")
            total['fires'] += len(d['fires'])
            total['outside'] += len(outside)
    print('total', dict(total))


if __name__ == '__main__':
    main([Path(a) for a in sys.argv[1:]])
