"""Regenerate the audio content test mod's sounds (48 kHz mono PCM16, self-made: no game audio).

    python synthesize.py

bed.wav: 20 s of soft filtered noise with a slow swell (replaces a zone bed), warn.wav: a short
falling tone (a speech take), siren.wav: a 3 s rising-falling tone (an added location-set bank),
fade.wav: 2 s of looping band-passed noise with a slow flutter (a mod crossfade bank's sample, played
by a declared layout). Every autotest step with something to hear has its own sound, clearly
different in pitch and character (the listener names the step by its sound):
  bell.wav      a deep church bell "bong" at 22.05 kHz (replaces Baby_Cry_1, the map emitter of
                step 1: another rate and length than the retail samples, so the rebuilt sample
                headers are exercised)
  chime.wav     a two-tone doorbell chime "ding-dong" (replaces Buoy_Bell, the bank the F6 post plays)
  woodblock.wav a wood block "knock-knock" (the F8 one-shot next to the siren)
  triangle.wav  a high triangle "ting" with a long shimmer (replaces clock_bell, the F10 emitter)
  horn.wav      a low buzzy horn "honk" (the F11 landing beacon)
  click.wav     one short dry click (the pop_click rule on every pop)
  whistle.wav   a rising whistle chirp "wheet" (the Digit5 nose rule)
  shaker.wav    a shaker / rattle "chk-chk-chk" (replaces Boat_Horns, the Digit6 orbiting emitter)
  boing.wav     a cartoon spring "boing" (the honk_beep rule above honking cars)
dev.csi: a Csis project of the mod's own (doc 16 L4): the class c_dev_mod, the function f_dev_msg
and the global g_dev_level (default 7); the layout skate-audio's `Project::parse` reads.
"""
from __future__ import annotations

import math
import random
import struct
import wave
from pathlib import Path

HERE = Path(__file__).resolve().parent / "audio"


def write(name: str, samples: list[float], rate: int = 48000) -> None:
    peak = max(abs(s) for s in samples) or 1.0
    scale = min(0.8 / peak, 1.0)
    pcm = b"".join(struct.pack("<h", int(max(-32767, min(32767, s * scale * 32767.0)))) for s in samples)
    HERE.mkdir(parents=True, exist_ok=True)
    with wave.open(str(HERE / name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm)


def bed() -> list[float]:
    rng = random.Random(0x5EED)
    out, lp = [], 0.0
    n = 48000 * 20
    for i in range(n):
        lp += 0.02 * (rng.uniform(-1, 1) - lp)
        swell = 0.6 + 0.4 * math.sin(2 * math.pi * i / n)
        out.append(lp * swell)
    return out


def env(i: int, n: int, attack: float, decay: float, rate: int = 48000) -> float:
    """Linear attack, exponential decay (seconds to -60 dB), a 5 ms fade at the end (no click)."""
    t = i / rate
    a = min(1.0, t / attack) if attack > 0 else 1.0
    return a * math.exp(-6.9 * t / decay) * min(1.0, (n - i) / (rate * 0.005))


def bell(rate: int) -> list[float]:
    # Inharmonic church-bell partials over a 196 Hz strike note (hum, prime, tierce, quint, nominal).
    n = int(rate * 1.6)
    partials = [(0.5, 0.6, 1.6), (1.0, 1.0, 1.4), (1.2, 0.5, 1.0), (1.5, 0.35, 0.8), (2.0, 0.4, 0.7), (2.76, 0.25, 0.5), (5.4, 0.12, 0.25)]
    return [sum(a * math.sin(2 * math.pi * 196 * r * i / rate) * env(i, n, 0.002, d * 1.0, rate) for r, a, d in partials) for i in range(n)]


def chime() -> list[float]:
    # Two pure notes, high then low (E6, then C6), each a sine with a soft octave.
    out = []
    for f in (1318.5, 1046.5):
        n = int(48000 * 0.55)
        out += [(math.sin(2 * math.pi * f * i / 48000) + 0.2 * math.sin(4 * math.pi * f * i / 48000)) * env(i, n, 0.003, 0.9) for i in range(n)]
    return out


def woodblock() -> list[float]:
    # Two hollow knocks: a fast-decaying 900 Hz / 2.3 kHz body with a short noise attack.
    rng = random.Random(0xB10C)
    one = int(48000 * 0.18)
    knock = [(math.sin(2 * math.pi * 900 * i / 48000) + 0.5 * math.sin(2 * math.pi * 2300 * i / 48000)) * env(i, one, 0.0005, 0.07)
             + rng.uniform(-1, 1) * math.exp(-i / 48.0) * 0.6 for i in range(one)]
    return knock + [0.0] * int(48000 * 0.04) + knock


def triangle() -> list[float]:
    # A struck steel triangle: high inharmonic partials, a slow shimmer, 1.5 s ring.
    n = int(48000 * 1.5)
    partials = [(3100, 1.0), (4650, 0.6), (6900, 0.45), (8400, 0.3)]
    return [sum(a * math.sin(2 * math.pi * f * i / 48000) for f, a in partials) * (0.85 + 0.15 * math.sin(2 * math.pi * 5 * i / 48000)) * env(i, n, 0.001, 1.4) for i in range(n)]


def horn() -> list[float]:
    # A low buzzy horn: a band-limited sawtooth at 98 Hz (G2), flat while held, 0.8 s.
    n, phase, out = int(48000 * 0.8), 0.0, []
    for i in range(n):
        phase += 2 * math.pi * 98 * (1 + 0.004 * math.sin(2 * math.pi * 5 * i / 48000)) / 48000
        a = min(1.0, i / 1440) * min(1.0, (n - i) / 4800)
        out.append(sum(math.sin(k * phase) / k for k in range(1, 16)) * a)
    return out


def click() -> list[float]:
    # One dry click: a 2 ms noise tick through a crude high-pass, 25 ms long.
    rng = random.Random(0xC11C)
    n, prev, out = int(48000 * 0.025), 0.0, []
    for i in range(n):
        x = rng.uniform(-1, 1) * math.exp(-i / 96.0)
        out.append(x - prev)
        prev = x
    return out


def whistle() -> list[float]:
    # A rising whistle chirp, 1.2 -> 2.8 kHz in 0.35 s, a little breath noise.
    rng = random.Random(0x3157)
    n, phase, out = int(48000 * 0.35), 0.0, []
    for i in range(n):
        t = i / n
        phase += 2 * math.pi * (1200 + 1600 * t * t) / 48000
        a = math.sin(math.pi * t) ** 0.5
        out.append((math.sin(phase) + 0.05 * rng.uniform(-1, 1)) * a)
    return out


def shaker() -> list[float]:
    # Three shakes of bright noise (a rattle), 0.15 s apart.
    rng = random.Random(0x5A4E)
    out, prev = [], 0.0
    for shake in range(3):
        n = int(48000 * 0.15)
        for i in range(n):
            x = rng.uniform(-1, 1)
            hp = x - prev
            prev = x
            out.append(hp * min(1.0, i / 480) * math.exp(-i / 2400.0))
    return out


def boing() -> list[float]:
    # A cartoon spring: a wobbling tone falling 420 -> 140 Hz, 0.6 s.
    n, phase, out = int(48000 * 0.6), 0.0, []
    for i in range(n):
        t = i / 48000
        f = (140 + 280 * math.exp(-5 * t)) * (1 + 0.12 * math.sin(2 * math.pi * 14 * t) * math.exp(-3 * t))
        phase += 2 * math.pi * f / 48000
        out.append((math.sin(phase) + 0.3 * math.sin(3 * phase)) * env(i, n, 0.003, 0.9))
    return out


def warn() -> list[float]:
    n = int(48000 * 0.6)
    return [math.sin(2 * math.pi * (500 - 200 * i / n) * i / 48000) * math.exp(-3 * i / n) for i in range(n)]


def siren() -> list[float]:
    n, phase, out = 48000 * 3, 0.0, []
    for i in range(n):
        f = 700 + 400 * math.sin(math.pi * i / n)
        phase += 2 * math.pi * f / 48000
        out.append(math.sin(phase) * math.sin(math.pi * i / n))
    return out


def fade() -> list[float]:
    rng = random.Random(0xFADE)
    n, lp, hp, out = 48000 * 2, 0.0, 0.0, []
    for i in range(n):
        lp += 0.08 * (rng.uniform(-1, 1) - lp)
        hp += 0.01 * (lp - hp)
        # A whole number of flutter cycles, so the loop is seamless.
        out.append((lp - hp) * (0.7 + 0.3 * math.sin(2 * math.pi * 4 * i / n)))
    return out


def csi(path: Path) -> None:
    """A MOIR project: header 0x28 (counts at 0x0A / 0x0C / 0x0E, id at 0x10, big-endian), the
    function and class records (12 bytes: name offset at +4, name id at +8), the global records (16
    bytes: default at +4, name offset at +8, name id at +12), then the names."""
    tables = [[("f_dev_msg", 1, 0)], [("c_dev_mod", 1, 0)], [("g_dev_level", 1, 7)]]
    records = sum(len(t) * (16 if i == 2 else 12) for i, t in enumerate(tables))
    head = bytearray(0x28)
    head[0:4] = b"MOIR"
    for i, at in enumerate((0x0A, 0x0C, 0x0E)):
        head[at:at + 2] = struct.pack(">H", len(tables[i]))
    head[0x10:0x12] = struct.pack(">H", 0x4445)
    rec, names = bytearray(), bytearray()
    for i, table in enumerate(tables):
        for name, name_id, default in table:
            off = 0x28 + records + len(names)
            names += name.encode() + b"\0"
            if i == 2:
                rec += b"\0" * 4 + struct.pack(">iIH", default, off, name_id) + b"\0" * 2
            else:
                rec += b"\0" * 4 + struct.pack(">IH", off, name_id) + b"\0" * 2
    path.write_bytes(bytes(head + rec + names))


if __name__ == "__main__":
    write("bed.wav", bed())
    write("bell.wav", bell(22050), 22050)
    write("chime.wav", chime())
    write("woodblock.wav", woodblock())
    write("triangle.wav", triangle())
    write("horn.wav", horn())
    write("click.wav", click())
    write("whistle.wav", whistle())
    write("shaker.wav", shaker())
    write("boing.wav", boing())
    write("warn.wav", warn())
    write("siren.wav", siren())
    write("fade.wav", fade())
    csi(HERE / "dev.csi")
    print("wrote", sorted(p.name for p in HERE.glob("*.wav")), "dev.csi")
