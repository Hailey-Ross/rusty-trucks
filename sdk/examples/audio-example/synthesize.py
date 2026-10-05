"""Regenerate the audio example's sounds (48 kHz mono PCM16, self-made: no game audio).

    python synthesize.py

chime_0..2.wav: soft bell tones (a location-set bank the example adds), tick.wav: a short click
the example layers on the game's own pop sound.
"""
from __future__ import annotations

import math
import struct
import wave
from pathlib import Path

SR = 48000
HERE = Path(__file__).resolve().parent / "audio"


def write(name: str, samples: list[float]) -> None:
    peak = max(abs(s) for s in samples) or 1.0
    scale = min(0.8 / peak, 1.0)
    pcm = b"".join(struct.pack("<h", int(max(-32767, min(32767, s * scale * 32767.0)))) for s in samples)
    HERE.mkdir(parents=True, exist_ok=True)
    with wave.open(str(HERE / name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm)


def bell(freq: float, seconds: float) -> list[float]:
    """A bell-like tone: a fundamental and two inharmonic partials with exponential decays."""
    out = []
    for i in range(int(SR * seconds)):
        t = i / SR
        attack = min(1.0, t / 0.004)
        s = (math.sin(2 * math.pi * freq * t) * math.exp(-t * 2.2)
             + 0.45 * math.sin(2 * math.pi * freq * 2.76 * t) * math.exp(-t * 4.0)
             + 0.2 * math.sin(2 * math.pi * freq * 5.4 * t) * math.exp(-t * 7.0))
        out.append(attack * s)
    return out


def tick() -> list[float]:
    """A 40 ms click: a decaying 2.2 kHz burst over a softer 700 Hz body."""
    out = []
    for i in range(int(SR * 0.04)):
        t = i / SR
        out.append(math.sin(2 * math.pi * 2200 * t) * math.exp(-t * 160) + 0.5 * math.sin(2 * math.pi * 700 * t) * math.exp(-t * 90))
    return out


if __name__ == "__main__":
    for n, f in enumerate((660.0, 880.0, 990.0)):
        write(f"chime_{n}.wav", bell(f, 1.6))
    write("tick.wav", tick())
    print("wrote", sorted(p.name for p in HERE.glob("*.wav")))
