"""Check a zone ambience bed's retail level against a prediction (the MixMap −11 dB base question).

usage: py -3.13 check_bed_level.py <session dir> <stop name> <bed stream> <zone volume>
  e.g. <session dir> Aletown 06_dt_open 0.75

Measures: the recomp capture (`trace.f32`, stereo float32 48 kHz; `CAPTURE <ms> <frames>` lines align it to
trace time) over the stop's listening window. 1 s RMS blocks; the low percentiles are mostly bed, since
one-shots and emitters only add on top.
Predicts: the bed's original 5-channel stream (decoded with vgmstream into .local/audio-re/amb_level)
× zone volume × base gain, folded the way the recomp host's capture folds it:
0.4·(front + surround + 0.5·centre) per side (aems-voice-graph-spec.md). Both channel orders (L C R Ls Rs /
L R C Ls Rs) are shown, because the stream's order is unverified.
"""
import struct
import subprocess
import sys
import wave
from pathlib import Path

import numpy

REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO))
from tools.owned_game.big import BigArchive  # noqa: E402

WORK = REPO / '.local/audio-re/amb_level'
VGM = REPO / 'data/tools/vgmstream-cli/vgmstream-cli.exe'


def capture_window(session: Path, stop: str) -> numpy.ndarray:
    marks, clock = [], []
    for line in (session / 'trace.tsv').open(encoding='utf-8', errors='replace'):
        f = line.rstrip('\n').split('\t')
        if f[0] == 'MARK':
            marks.append((float(f[1]), f[2]))
        elif f[0] == 'CAPTURE':
            clock.append((float(f[1]), int(f[2])))
    start = next(t for t, label in marks if label.startswith(f'listen {stop}'))
    end = next((t for t, label in marks if t > start and label.startswith(('at ', 'done'))), marks[-1][0])
    ms, frames = numpy.array([c[0] for c in clock]), numpy.array([c[1] for c in clock])
    a, b = (int(numpy.interp(t, ms, frames)) for t in (start + 5000, end - 5000))
    data = numpy.memmap(session / 'trace.f32', dtype='<f4', mode='r')
    return numpy.asarray(data[2 * a:2 * b]).reshape(-1, 2)


def decode_bed(bed: str) -> numpy.ndarray:
    WORK.mkdir(parents=True, exist_ok=True)
    out = WORK / f'{bed}.wav'
    if not out.exists():
        disc = REPO / '.local/skate3-disc/data/audio'
        heads, bodies = BigArchive(disc / 'ambienceresident.big'), BigArchive(disc / 'ambience.big')
        (WORK / f'{bed}.snr').write_bytes(heads.read(next(e for e in heads.entries if e.path.endswith(f'{bed}.snr'))))
        (WORK / f'{bed}.sns').write_bytes(bodies.read(next(e for e in bodies.entries if e.path.endswith(f'{bed}.sns'))))
        subprocess.run([str(VGM), '-o', str(out), str(WORK / f'{bed}.snr')], check=True, capture_output=True)
    with wave.open(str(out)) as w:
        ch, n = w.getnchannels(), w.getnframes()
        pcm = numpy.frombuffer(w.readframes(n), dtype='<i2').reshape(-1, ch).astype(numpy.float32) / 32768.0
    return pcm


def block_db(stereo: numpy.ndarray) -> numpy.ndarray:
    blocks = stereo[: len(stereo) // 48000 * 48000].reshape(-1, 48000, 2)
    power = (blocks ** 2).mean(axis=(1, 2))
    return 10 * numpy.log10(numpy.maximum(power, 1e-12))


def fold(pcm: numpy.ndarray, order: str) -> numpy.ndarray:
    if order == 'LCR':
        fl, c, fr, sl, sr = (pcm[:, i] for i in range(5))
    else:
        fl, fr, c, sl, sr = (pcm[:, i] for i in range(5))
    return numpy.stack([0.4 * (fl + sl + 0.5 * c), 0.4 * (fr + sr + 0.5 * c)], axis=1)


def main() -> None:
    session, stop, bed, volume = Path(sys.argv[1]), sys.argv[2], sys.argv[3], float(sys.argv[4])
    retail = block_db(capture_window(session, stop))
    print(f'retail capture at {stop}: {len(retail)} s; 1 s RMS p10 {numpy.percentile(retail, 10):.1f} dBFS, '
          f'p25 {numpy.percentile(retail, 25):.1f}, median {numpy.median(retail):.1f}')
    pcm = decode_bed(bed)
    print(f'bed {bed}: {pcm.shape[1]} channels, {len(pcm) / 48000:.0f} s')
    for order in ('LCR', 'LRC'):
        raw = block_db(fold(pcm, order))
        for label, base_db in (('no base', 0.0), ('-11 dB base', -11.0)):
            gain_db = 20 * numpy.log10(volume) + base_db
            print(f'  predicted ({order}, {label}): p10 {numpy.percentile(raw, 10) + gain_db:.1f}, '
                  f'p25 {numpy.percentile(raw, 25) + gain_db:.1f}, median {numpy.median(raw) + gain_db:.1f} dBFS')


if __name__ == '__main__':
    main()
