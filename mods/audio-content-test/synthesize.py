"""Regenerate the audio content test mod's sounds (48 kHz mono PCM16, self-made: no game audio).

    python synthesize.py

bed.wav: 20 s of soft filtered noise with a slow swell (replaces a zone bed), beep.wav: a 0.4 s
two-tone beep at 22.05 kHz (replaces an emitter bank's samples: another rate and length than the
retail ones, so the rebuilt sample headers are exercised), warn.wav: a short falling tone (a speech
take), siren.wav: a 3 s rising-falling tone (an added location-set bank), fade.wav: 2 s of looping
band-passed noise with a slow flutter (a mod crossfade bank's sample, played by a declared layout).
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


def beep(rate: int) -> list[float]:
    return [math.sin(2 * math.pi * (880 if i < rate * 0.2 else 660) * i / rate) * min(1.0, (rate * 0.4 - i) / (rate * 0.02)) for i in range(int(rate * 0.4))]


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


if __name__ == "__main__":
    write("bed.wav", bed())
    write("beep.wav", beep(22050), 22050)
    write("warn.wav", warn())
    write("siren.wav", siren())
    write("fade.wav", fade())
    print("wrote", sorted(p.name for p in HERE.glob("*.wav")))
