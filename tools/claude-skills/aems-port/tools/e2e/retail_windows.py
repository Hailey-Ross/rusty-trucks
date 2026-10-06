"""End-to-end check against retail: straight-rolling windows of a recomp session's capture
(audio.f32 = the recomp host's stereo fold 0.4·(L + Ls + 0.5·C) per side, CAPTURE-aligned) against
our / the PoC's renders folded the same way.

Windows: SKATEB samples with the four wheels down (no truck/deck contact), the horizontal deck
speed steady (±12 %) for >= 1.0 s and no SPLC line from the board contacts inside (collision-manager
one-shots stay in: they ride along with rolling in retail). Per
5 km/h bin: median RMS (dBFS), octave bands (dB rel. total) and the standing-still floor (speed
< 0.3 m/s windows: ambience + emitters, what the capture holds besides the board).

usage: py -3.13 retail_windows.py SESSION_DIR [E2E_DIR]
"""
import math
import os
import sys
from bisect import bisect_left
from collections import defaultdict
from pathlib import Path

import numpy as np

RATE = 48000
# SPLC callers inside SFXObj_Contacts (pops, landings, touchdowns, foot taps): a window holding one
# of these is not plain rolling. Collision-manager one-shots (824D2…) stay in (they ride along).
CONTACT_CALLERS = ('824B8', '824B9', '824BA', '824BB')
BANDS = [63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]


def load_trace(path):
    sk, splc, caps = [], [], []
    # SKATEB interleaves every skater's board: the player's is the one logged most.
    from collections import Counter
    boards = Counter(l.split('\t')[2] for l in path.open(encoding='utf-8', errors='replace') if l.startswith('SKATEB	') and l.count('\t') > 3)
    player = boards.most_common(1)[0][0] if boards else None
    for raw in path.open(encoding='utf-8', errors='replace'):
        f = raw.rstrip('\n').split('\t')
        if len(f) < 3:
            continue
        try:
            ms = float(f[1])
        except ValueError:
            continue
        if f[0] == 'SKATEB' and len(f) >= 6:
            if f[2] != player:
                continue
            try:
                v = [float(x) for x in f[5].split()]
                sk.append((ms, f[3], math.hypot(v[0], v[2])))
            except (ValueError, IndexError):
                pass
        elif f[0] == 'SPLC' and len(f) >= 5 and f[4][:5] in CONTACT_CALLERS:
            splc.append(ms)
        elif f[0] == 'CAPTURE':
            try:
                caps.append((ms, int(f[2])))
            except ValueError:
                pass
    return sk, sorted(splc), caps


def capture(session, caps, a, b):
    ms = np.array([c[0] for c in caps])
    frames = np.array([c[1] for c in caps])
    fa, fb = (int(np.interp(t, ms, frames)) for t in (a, b))
    data = np.memmap(session / 'audio.f32', dtype='<f4', mode='r')
    return np.asarray(data[2 * fa:2 * fb]).reshape(-1, 2)


def metrics(x):
    mono = x.mean(1)
    rms = 10 * math.log10(max(float(np.mean(x * x)), 1e-20))
    spec = np.abs(np.fft.rfft(mono * np.hanning(len(mono)))) ** 2
    freqs = np.fft.rfftfreq(len(mono), 1 / RATE)
    total = spec.sum() + 1e-30
    bands = [10 * math.log10(max(spec[(freqs >= f / math.sqrt(2)) & (freqs < f * math.sqrt(2))].sum() / total, 1e-20)) for f in BANDS]
    return rms, bands


def windows(sk, splc, min_s=1.0):
    out = []
    i = 0
    while i < len(sk):
        ms, contacts, v = sk[i]
        if contacts[:4] != '1111' or contacts[4:] != '000':
            i += 1
            continue
        j = i
        while j + 1 < len(sk) and sk[j + 1][1] == contacts and abs(sk[j + 1][2] - v) <= 0.12 * max(v, 0.3) and sk[j + 1][0] - sk[j][0] < 400:
            j += 1
        a, b = ms, sk[j][0]
        if b - a >= min_s * 1000:
            k = bisect_left(splc, a)
            if not (k < len(splc) and splc[k] <= b):
                out.append((a, b, float(np.mean([s[2] for s in sk[i:j + 1]]))))
        i = j + 1
    return out


def fold_capture(path):
    d = np.fromfile(path, dtype='<f4')
    d = d[: len(d) // 6 * 6].reshape(-1, 6)  # L, R, C, LFE, Ls, Rs
    left = 0.4 * (d[:, 0] + d[:, 4] + 0.5 * d[:, 2])
    right = 0.4 * (d[:, 1] + d[:, 5] + 0.5 * d[:, 2])
    return np.stack([left, right], 1)[RATE:]


def main():
    session = Path(sys.argv[1])
    e2e = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(os.environ.get('E2E_DIR') or Path(__file__).resolve().parents[5] / '.local' / 'audio-re' / 'e2e')
    sk, splc, caps = load_trace(session / 'trace.tsv')
    ws = windows(sk, splc)
    bins = defaultdict(list)
    for a, b, v in ws:
        x = capture(session, caps, a, b)
        if len(x) < RATE // 2:
            continue
        kmh = v * 3.6
        key = 0 if kmh < 1 else int(round(kmh / 5) * 5)
        bins[key].append(metrics(x))
    print(f'{session.name}: {len(ws)} straight four-wheel windows')
    print('bin km/h   n   retail RMS p50 (p10..p90)   octave bands p50 (63..16k)')
    retail = {}
    for k in sorted(bins):
        r = sorted(m[0] for m in bins[k])
        b = np.median(np.array([m[1] for m in bins[k]]), axis=0)
        retail[k] = (r[len(r) // 2], b)
        print(f'{k:4d}     {len(r):3d}   {r[len(r) // 2]:6.1f} ({r[len(r) // 10]:6.1f}..{r[(len(r) * 9) // 10]:6.1f})   ' + ' '.join(f'{x:5.1f}' for x in b))
    print('\nrenders in the capture fold (straight roll scenarios, after the settle second):')
    for name, kmh in (('roll10', 10), ('roll20', 20), ('roll30', 30), ('roll45', 45)):
        for side in ('ours', 'poc'):
            p = e2e / f'{name}.{side}.f32'
            if not p.exists():
                continue
            rms, bands = metrics(fold_capture(p))
            ref = retail.get(kmh)
            delta = f'{rms - ref[0]:+5.1f} dB vs retail' if ref else 'no retail bin'
            bd = ' '.join(f'{x - y:+5.1f}' for x, y in zip(bands, ref[1])) if ref else ''
            print(f'{name:7s} {side:5s} RMS {rms:6.1f}  ({delta})   bands d {bd}')


if __name__ == '__main__':
    main()
