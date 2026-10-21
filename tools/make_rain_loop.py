#!/usr/bin/env python3
"""Generate assets/sounds/amb_rain.wav: a seamless rain loop.

Steady rain is a soft, low-passed hiss with many small droplet ticks on
top. Both are made from seeded noise (so the file regenerates exactly), and
the end is crossfaded into the start so the loop has no seam.

    python3 -B tools/make_rain_loop.py
"""

import math
import random
import struct
import wave
from pathlib import Path

RATE = 22050
SECONDS = 12.0
FADE = 1.0  # seconds crossfaded from the end into the start
OUT = Path(__file__).resolve().parent.parent / "assets" / "sounds" / "amb_rain.wav"


def one_pole(samples, cutoff):
    """Low-pass filter: each sample moves part of the way to the input."""
    k = 1.0 - math.exp(-2.0 * math.pi * cutoff / RATE)
    out, y = [], 0.0
    for x in samples:
        y += (x - y) * k
        out.append(y)
    return out


def main():
    rng = random.Random(7)
    n = int((SECONDS + FADE) * RATE)
    white = [rng.uniform(-1.0, 1.0) for _ in range(n)]
    # The hiss: two low-passes, a darker body and a brighter sheen.
    body = one_pole(one_pole(white, 900.0), 900.0)
    sheen = one_pole(white, 4000.0)
    sheen = [s - b for s, b in zip(sheen, one_pole(sheen, 1500.0))]
    # Droplets: short decaying ticks at random times, each its own pitch.
    drops = [0.0] * n
    t = 0
    while t < n:
        t += int(rng.expovariate(140.0) * RATE) + 1
        amp = rng.uniform(0.15, 0.6)
        freq = rng.uniform(1800.0, 5200.0)
        decay = rng.uniform(0.002, 0.008)
        for j in range(int(decay * 6 * RATE)):
            if t + j >= n:
                break
            drops[t + j] += amp * math.exp(-j / (decay * RATE)) * math.sin(2 * math.pi * freq * j / RATE)
    mix = [1.6 * b + 0.5 * s + 0.35 * d for b, s, d in zip(body, sheen, drops)]
    # Slow swells, so it doesn't sound like a fan.
    mix = [x * (0.85 + 0.15 * math.sin(2 * math.pi * i / (RATE * 4.0))) for i, x in enumerate(mix)]
    loop_n, fade_n = int(SECONDS * RATE), int(FADE * RATE)
    loop = mix[:loop_n]
    for i in range(fade_n):
        a = i / fade_n
        loop[i] = loop[i] * a + mix[loop_n + i] * (1.0 - a)
    peak = max(abs(x) for x in loop)
    frames = b"".join(struct.pack("<h", int(x / peak * 0.8 * 32767)) for x in loop)
    with wave.open(str(OUT), "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(RATE)
        f.writeframes(frames)
    print(f"wrote {OUT} ({SECONDS:.0f} s)")


if __name__ == "__main__":
    main()
