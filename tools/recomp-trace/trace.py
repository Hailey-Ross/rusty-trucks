"""Shared reader for skate3recomp research traces (`trace.tsv` from the hooks in src/research/*).

Lines are `KIND <tab> <ms> <tab> fields…`, all on one time base. Special kinds:
- MARK <ms> <label>: script steps, e.g. "at <stop>", "listen <stop>", "done";
- CLOCK <ms> <unix ms>: aligns trace time with wall clock (screenshot names);
- CAPTURE <ms> <frames>: aligns trace time with the float32 stereo capture (`trace.f32`).

    from trace import Trace
    t = Trace('sessions/my_session/trace.tsv')
    for stop, lines in t.by_stop('WATCH'):          # lines are (ms, fields)
        ...
    t.shot_for(ms)    # nearest screenshot path
    t.capture(a_ms, b_ms)  # numpy (n, 2) float32 window
"""
from __future__ import annotations

from bisect import bisect_left
from collections import defaultdict
from pathlib import Path
from typing import Iterator


# Fixed field counts (after KIND and ms) for kinds whose hooks write a fixed layout; `Trace.malformed()` checks
# them. Hook documentation: the header comment above each hook in skate3recomp src/research/hooks_*.cpp.
FIELD_COUNTS = {
    'TREAT': 8,     # +236 +240 +260 +224 +332 +200 object local   (Class_Treatment)
    'SEAMPAT': 11,  # +636 +648 +620 +208 frame_ms w0 w1 w2 w3 ("x y z") object local   (Class_Seams)
    'SEAMHIT': 5,   # wheel single transition object local   (seam hit)
    # Category audiox. Space-separated groups count as one field.
    'GRECX': 7,     # owner w332(hex) slip232 rev690 counter1516 cam("x y z") view("x y z")   (grain-bed owner hook, local player)
    'FIRSTHIT': 6,  # B b16(u8) f24 bail676 end677 why(1 byte 2 float 4 bail/end 8 heartbeat)   (GREC hook)
    'SKID': 7,      # owner holder handle w0..w17(18 ints, or "-" on release) counter1516 slip232 rev690   (skid hook)
    'EMITSLOT': 11, # object state info0(patch) info4(positional) info8(level) info12 info16 mixkey handle w0..w8 out0..out9(hex)   (emitter slot hook)
}


class Trace:
    def __init__(self, path: str | Path):
        self.path = Path(path)
        self.lines: dict[str, list[tuple[float, list[str]]]] = defaultdict(list)
        self.marks: list[tuple[float, str]] = []
        self.clock: float | None = None  # unix ms at trace ms 0
        self.captures: list[tuple[float, int]] = []
        for raw in self.path.open(encoding='utf-8', errors='replace'):
            f = raw.rstrip('\n').split('\t')
            if len(f) < 2:
                continue
            try:
                ms = float(f[1])
            except ValueError:
                continue
            kind = f[0]
            try:  # a few lines can be split/merged by the two trace writers; skip what doesn't parse
                if kind == 'MARK':
                    self.marks.append((ms, f[2] if len(f) > 2 else ''))
                elif kind == 'CLOCK' and self.clock is None and len(f) > 2:
                    self.clock = int(f[2]) - ms
                elif kind == 'CAPTURE' and len(f) > 2:
                    self.captures.append((ms, int(f[2])))
            except ValueError:
                continue
            self.lines[kind].append((ms, f[2:]))

    def malformed(self) -> dict[str, int]:
        """Lines per kind whose field count differs from FIELD_COUNTS (should be all zero)."""
        return {k: sum(1 for _, f in self.lines.get(k, []) if len(f) != n) for k, n in FIELD_COUNTS.items()}

    def kinds(self) -> dict[str, int]:
        return {k: len(v) for k, v in self.lines.items()}

    def stops(self) -> list[tuple[str, float, float]]:
        """(name, start ms, end ms) for each `at <name>` mark, ending at the next `at`/`done` mark."""
        starts = [(ms, label[3:].split('#')[0].strip()) for ms, label in self.marks if label.startswith('at ')]
        ends = [ms for ms, label in self.marks if label.startswith(('at ', 'done'))]
        out = []
        for ms, name in starts:
            later = [e for e in ends if e > ms]
            out.append((name, ms, later[0] if later else float('inf')))
        return out

    def by_stop(self, kind: str) -> Iterator[tuple[str, list[tuple[float, list[str]]]]]:
        for name, a, b in self.stops():
            yield name, [(ms, f) for ms, f in self.lines.get(kind, []) if a <= ms < b]

    def shot_for(self, ms: float) -> Path | None:
        """Nearest screenshot (`<trace stem>_shots/` or `shots/`, `shot_<unix ms>.jpg|png`) to trace time `ms`."""
        if self.clock is None:
            return None
        # scripted runs: <trace stem>_shots/; passive sessions (PLAY_TRACE_*.bat): shots/
        folders = [self.path.with_name(self.path.stem + '_shots'), self.path.with_name('shots')]
        folder = next((f for f in folders if f.exists()), None)
        shots = sorted(folder.glob('shot_*.*')) if folder else []
        if not shots:
            return None
        stamps = [int(p.stem.split('_')[1]) for p in shots]
        target = self.clock + ms
        i = bisect_left(stamps, target)
        best = min((j for j in (i - 1, i) if 0 <= j < len(stamps)), key=lambda j: abs(stamps[j] - target))
        return shots[best]

    def capture(self, a_ms: float, b_ms: float):
        """Stereo float32 frames of `trace.f32` between two trace times (numpy array (n, 2))."""
        import numpy
        ms = numpy.array([c[0] for c in self.captures])
        frames = numpy.array([c[1] for c in self.captures])
        a, b = (int(numpy.interp(t, ms, frames)) for t in (a_ms, b_ms))
        data = numpy.memmap(self.path.with_suffix('.f32'), dtype='<f4', mode='r')
        return numpy.asarray(data[2 * a:2 * b]).reshape(-1, 2)


if __name__ == '__main__':
    import sys
    if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
        sys.exit('usage: py -3.13 tools/recomp-trace/trace.py <trace.tsv>   (line counts per kind, malformed '
                 'fixed-layout lines, MARK stops)')
    t = Trace(sys.argv[1])
    print(t.kinds())
    print('malformed (fixed-layout kinds):', t.malformed())
    for name, a, b in t.stops():
        print(f'{name:28s} {a / 1000:8.1f} .. {b / 1000:8.1f} s')
