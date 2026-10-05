"""Compare retail's active location set (recomp trace WPPOS / WPSET / WPFIRE lines) with our region lookup.

usage: py -3.13 check_sets_vs_trace.py <trace.tsv> [stop=District ...]

Each stop is checked against ITS district only (the game loads one district; district maps overlap in
space). Stops of the Challenge Map > Locations lists are known below; others can be given as
`Name=District`; positions before the first stop use `first=District`.

Reads the audio manifest of the current dev install (regions, random_sets) and, for every WPPOS line,
looks the position up in each district's `audio_emitters` region layer. Prints per teleport stop
(MARK "at <name>") the retail set, our set, the agreement rate, and the sounds retail fired.
"""
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO))
from tools.asset_pipeline.audio_formats import region_key  # noqa: E402


STOP_DISTRICT = {
    **{s: 'DownTown' for s in ('Aletown', 'All_Mart', 'Carverton_Memorial_Park', 'Crystal_Towers', 'Hotel_District',
                                'Kube_Tower', 'Midtown_Promenade', 'Park_n_Play', 'Plaza_of_Solitude', 'Rippon_Towers',
                                'Rosalita_Skate_Park', 'Slappys_Car_Lot', 'Tresoutta_Plaza', 'Uptown_Promenade')},
    **{s: 'Industrial' for s in ('Carverton_Quarry', 'Drydocks', 'Factory_Roofs', 'Ghetto_Spot', 'Haystings_Park',
                                  'Loading_Docks', 'Miracle_Bowl', 'Pipe_Works', 'Spillway_Entrance', 'The_Old_Factory',
                                  'The_Tanker', '2nd_and_Navy')},
    **{s: 'University' for s in ('Campus_Entrance', 'Chan_Center', 'Clock_Tower', 'Daly_Estates', 'Hartley_Stadium',
                                  'PCU_Library', 'Peterson_Pavilion', 'Super_Ultra_Mega_Park', 'The_Carvatron',
                                  'The_Observatory')},
}


def main(trace: Path, extra: dict[str, str]) -> None:
    marker = json.loads((REPO / 'data/installation.json').read_text())
    manifest = json.loads((REPO / 'data' / marker['directory'] / 'assets/private/audio/audio_manifest.json').read_text())
    sets = manifest.get('random_sets', {})
    name = lambda key: (sets.get(key) or {}).get('name') or key
    tiles = {district: [{'box': t['box'], 'nodes': t['nodes'], 'keys': [int(k, 16) for k in t['keys']]}
                        for t in layers.get('audio_emitters', [])]
             for district, layers in manifest.get('regions', {}).items()}
    sound_bank = {}
    for s in sets.values():
        for sound in s['sounds']:
            sound_bank[sound['sound_id']] = sound['bank']

    stop = 'before first stop'
    stale = last_key = None
    by_stop = defaultdict(lambda: {'agree': 0, 'total': 0, 'stale': 0, 'retail': Counter(), 'ours': Counter(), 'fired': [], 'pos': None})
    for line in trace.open(encoding='utf-8', errors='replace'):
        f = line.rstrip('\n').split('\t')
        if f[0] == 'MARK' and len(f) > 2 and f[2].startswith('at '):
            stop = f[2][3:].split('#')[0].strip()
            # The recomp ends its loading screen before the new district's streams load, so the
            # previous area's set lingers for a few seconds (real hardware waits them out).
            stale = last_key
        elif f[0] == 'WPPOS' and len(f) >= 4:
            x, _, z = (float(v) for v in f[2].split())
            retail = f[3]
            if retail == '0000000000000000':
                continue  # loading / menu
            if stale is not None and retail == stale:
                by_stop[stop]['stale'] += 1
                continue
            stale = None
            last_key = retail
            district = extra.get(stop) or STOP_DISTRICT.get(stop) or extra.get('first' if stop == 'before first stop' else '')
            ours = None
            for tile in tiles.get(district or '', []):
                key = region_key(tile, x, z)
                if key is not None:
                    ours = f'{key:016X}'
                    break
            entry = by_stop[stop]
            entry['total'] += 1
            entry['agree'] += ours == retail
            entry['retail'][name(retail)] += 1
            entry['ours'][name(ours) if ours else 'none'] += 1
            entry['pos'] = (round(x, 1), round(z, 1))
        elif f[0] == 'WPFIRE' and len(f) >= 3:
            by_stop[stop]['fired'].append(sound_bank.get(f[2], f[2]))

    total = agree = 0
    for stop, e in by_stop.items():
        if not e['total']:
            continue
        total += e['total']
        agree += e['agree']
        print(f"{stop:28s} at {e['pos']}  retail {dict(e['retail'])}  ours {dict(e['ours'])}  agree {e['agree']}/{e['total']} (skipped {e['stale']} stale)")
        print(f"{'':28s} fired {len(e['fired'])}: {', '.join(e['fired'])}")
    print(f'overall agreement {agree}/{total}')


if __name__ == '__main__':
    main(Path(sys.argv[1]), dict(a.split('=', 1) for a in sys.argv[2:]))
