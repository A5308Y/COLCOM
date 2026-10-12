#!/usr/bin/env python3
"""Synthesises the placeholder sounds in assets/sounds/ (mono 16-bit WAV).

They're simple stand-ins made from filtered noise, decaying tones and pitch
sweeps, so the game has sound before real samples are chosen. Replace any of
them by pointing assets/sounds.ron at other files (WAV or OGG).

    python3 tools/make_sounds.py

Only the standard library is used. The output is deterministic (fixed seed).
"""

import math
import os
import random
import struct
import wave

RATE = 22050
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "sounds")
rng = random.Random(1234)


# ---------- building blocks ----------

def silence(secs):
    return [0.0] * int(secs * RATE)


def noise(secs):
    return [rng.uniform(-1.0, 1.0) for _ in range(int(secs * RATE))]


def lowpass(x, cutoff):
    a = 1.0 - math.exp(-2.0 * math.pi * cutoff / RATE)
    y, out = 0.0, []
    for v in x:
        y += a * (v - y)
        out.append(y)
    return out


def highpass(x, cutoff):
    low = lowpass(x, cutoff)
    return [v - l for v, l in zip(x, low)]


def bandpass(x, lo, hi):
    return lowpass(highpass(x, lo), hi)


def env(n, attack, decay):
    """Quick attack, exponential decay (times in seconds)."""
    a = max(1, int(attack * RATE))
    out = []
    for i in range(n):
        if i < a:
            out.append(i / a)
        else:
            out.append(math.exp(-(i - a) / (decay * RATE)))
    return out


def tone(secs, f0, f1=None, decay=None, phase=0.0, vibrato=0.0, vib_rate=5.0):
    """A sine sweeping from f0 to f1, optionally decaying."""
    n = int(secs * RATE)
    f1 = f0 if f1 is None else f1
    out, ph = [], phase
    for i in range(n):
        t = i / n
        f = f0 * (f1 / f0) ** t
        f *= 1.0 + vibrato * math.sin(2 * math.pi * vib_rate * i / RATE)
        ph += 2 * math.pi * f / RATE
        v = math.sin(ph)
        if decay:
            v *= math.exp(-i / (decay * RATE))
        out.append(v)
    return out


def mul(x, e):
    return [a * b for a, b in zip(x, e)]


def mix(*parts):
    """Sum of (signal, gain, start_seconds) tuples."""
    n = max(int(s * RATE) + len(x) for x, g, s in parts)
    out = [0.0] * n
    for x, g, s in parts:
        o = int(s * RATE)
        for i, v in enumerate(x):
            out[o + i] += v * g
    return out


def shaped_noise(secs, lo, hi, attack, decay):
    x = bandpass(noise(secs), lo, hi)
    return mul(x, env(len(x), attack, decay))


def normalise(x, peak=0.85):
    m = max(1e-9, max(abs(v) for v in x))
    return [v * peak / m for v in x]


def fade_out(x, secs=0.01):
    n = min(len(x), int(secs * RATE))
    for i in range(n):
        x[len(x) - n + i] *= 1.0 - i / n
    return x


def loop(make, secs, cross=0.6):
    """A seamless loop: make secs + cross, fold the extra onto the start."""
    x = make(secs + cross)
    n, c = int(secs * RATE), int(cross * RATE)
    out = x[:n]
    for i in range(c):
        t = i / c
        out[i] = x[i] * t + x[n + i] * (1.0 - t)
    return out


def write(name, x, peak=0.85):
    os.makedirs(OUT, exist_ok=True)
    x = fade_out(normalise(x, peak))
    with wave.open(os.path.join(OUT, name + ".wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1.0, min(1.0, v)) * 32767)) for v in x))


def variants(name, count, make):
    for k in range(count):
        write(f"{name}_{k + 1}", make(k))


def j(v, spread=0.08):
    """Jitter a value a little, so variants differ."""
    return v * rng.uniform(1.0 - spread, 1.0 + spread)


# ---------- effects ----------

def step_grass(k):
    return mix((shaped_noise(0.12, 200, j(1400), 0.004, 0.035), 1.0, 0),
               (tone(0.08, j(85), 60, decay=0.03), 0.4, 0))


def step_stone(k):
    return mix((shaped_noise(0.08, 900, j(5000), 0.001, 0.018), 1.0, 0),
               (tone(0.05, j(320), 250, decay=0.012), 0.3, 0))


def step_wood(k):
    return mix((tone(0.14, j(150), 120, decay=0.04), 1.0, 0),
               (shaped_noise(0.06, 300, 2500, 0.001, 0.015), 0.5, 0))


def chop(k):
    return mix((shaped_noise(0.03, 400, 6000, 0.0005, 0.006), 0.9, 0),
               (tone(0.3, j(115), 70, decay=0.09), 1.0, 0),
               (tone(0.15, j(430), 380, decay=0.04), 0.5, 0))


def metal(freqs, secs, decay, k):
    parts = [(tone(secs, j(f, 0.03), decay=j(decay)), 1.0 / (i + 1), 0) for i, f in enumerate(freqs)]
    parts.append((shaped_noise(0.03, 2000, 8000, 0.0005, 0.005), 0.8, 0))
    return mix(*parts)


def mine(k):
    return mix((metal([1240, 2870, 4110, 5530], 0.35, 0.07, k), 1.0, 0),
               (shaped_noise(0.25, 300, 2500, 0.002, 0.06), 0.6, 0.005))


def dig(k):
    x = bandpass(noise(0.3), 150, j(900))
    crunch = [1.0 if rng.random() < 0.3 else 0.3 for _ in x]
    return mul(mul(x, lowpass(crunch, 60)), env(len(x), 0.01, 0.09))


def hammer(k):
    return mix((tone(0.15, j(260), 240, decay=0.035), 1.0, 0),
               (tone(0.1, j(610), 590, decay=0.02), 0.6, 0),
               (shaped_noise(0.03, 800, 6000, 0.0005, 0.004), 0.7, 0))


def anvil(k):
    return mix((metal([880, 2210, 3360, 4980], 0.6, 0.16, k), 1.0, 0),
               (tone(0.1, 180, decay=0.03), 0.5, 0))


def chisel(k):
    return mix((metal([1900, 3400, 5100], 0.2, 0.035, k), 0.7, 0),
               (shaped_noise(0.15, 1200, 6000, 0.001, 0.04), 0.8, 0))


def rustle(k):
    x = bandpass(noise(0.45), 1200, j(5000))
    bumps = [0.0] * len(x)
    for _ in range(6):
        c, w = rng.randrange(len(x)), rng.uniform(0.02, 0.06) * RATE
        for i in range(len(x)):
            bumps[i] += math.exp(-((i - c) / w) ** 2)
    return mul(x, mul(bumps, env(len(x), 0.05, 0.25)))


def sizzle(k):
    x = highpass(noise(0.9), j(3000))
    pops = [0.0] * len(x)
    for _ in range(25):
        c = rng.randrange(len(x) - 200)
        for i in range(120):
            pops[c + i] += math.exp(-i / 20.0) * rng.uniform(0.5, 1.5)
    x = [v * 0.5 + v * p for v, p in zip(x, pops)]
    return mul(x, env(len(x), 0.08, 0.5))


def saw(k):
    x = bandpass(noise(0.7), 900, 4500)
    stroke = [0.5 - 0.5 * math.cos(2 * math.pi * 3.0 * i / RATE) for i in range(len(x))]
    return mul(mul(x, stroke), env(len(x), 0.03, 0.5))


def craft(k):
    return mix((shaped_noise(0.05, 500, 3500, 0.001, 0.012), 1.0, 0),
               (tone(0.06, j(900), 800, decay=0.015), 0.5, 0),
               (shaped_noise(0.05, 500, 3500, 0.001, 0.012), 0.6, j(0.12)))


def furnace(k):
    x = lowpass(noise(1.0), 380)
    flick = lowpass([rng.uniform(0.5, 1.5) for _ in x], 8)
    return mul(mul(x, flick), env(len(x), 0.15, 0.6))


def tree_fall(k):
    creak = mul(bandpass([math.copysign(1, v) for v in tone(0.7, 220, 110, vibrato=0.04, vib_rate=11)], 150, 1500),
                env(int(0.7 * RATE), 0.2, 0.3))
    crash = mix((shaped_noise(1.0, 60, 700, 0.005, 0.3), 1.0, 0),
                (tone(0.5, 70, 45, decay=0.15), 0.8, 0))
    return mix((creak, 0.5, 0), (crash, 1.0, 0.55))


def rock_break(k):
    parts = [(tone(0.3, j(75), 50, decay=0.08), 0.8, 0)]
    for b in range(5):
        parts.append((shaped_noise(0.12, 200, j(1800), 0.001, 0.03), rng.uniform(0.4, 1.0), b * j(0.06, 0.4)))
    return mix(*parts)


def built(k):
    return mix((hammer(0), 1.0, 0), (hammer(1), 0.8, 0.13), (tone(0.4, 523, decay=0.12), 0.25, 0.26),
               (tone(0.4, 784, decay=0.12), 0.2, 0.26))


def gunshot(k, pitch=1.0, length=1.0):
    return mix((shaped_noise(0.6 * length, 100, 9000 * pitch, 0.0005, j(0.06) * length), 1.0, 0),
               (tone(0.4 * length, j(80 * pitch), 40, decay=0.08 * length), 0.9, 0),
               (shaped_noise(0.8 * length, 80, 600, 0.02, 0.25 * length), 0.3, 0.02))


def swing(k):
    x = noise(0.3)
    out, y = [], 0.0
    for i, v in enumerate(x):
        t = i / len(x)
        cutoff = 300 + 3000 * math.sin(math.pi * t)
        y += (1.0 - math.exp(-2 * math.pi * cutoff / RATE)) * (v - y)
        out.append(y * math.sin(math.pi * t))
    return out


def hit(k):
    return mix((tone(0.15, j(95), 60, decay=0.04), 1.0, 0),
               (shaped_noise(0.05, 200, 1500, 0.001, 0.015), 0.7, 0))


def glance(k):
    return metal([2500, 3700, 5200], 0.3, 0.06, k)


def thud(k):
    return mix((tone(0.35, j(65), 45, decay=0.09), 1.0, 0),
               (shaped_noise(0.2, 40, 350, 0.003, 0.05), 0.8, 0))


def death(k):
    return mix((tone(0.9, 200, 95, decay=0.4, vibrato=0.01), 0.5, 0),
               (shaped_noise(0.6, 100, 500, 0.05, 0.25), 0.3, 0),
               (thud(0), 0.7, 0.35))


def door(k):
    x = [math.copysign(1, v) * 0.6 + v * 0.4 for v in tone(0.35, j(190), j(150), vibrato=0.06, vib_rate=17)]
    return mul(bandpass(x, 200, 2200), env(len(x), 0.04, 0.12))


def horn(k):
    secs = 1.8
    parts = [(tone(secs, f, vibrato=0.006, vib_rate=4.5), g, 0) for f, g in [(147, 1.0), (294, 0.5), (441, 0.3), (588, 0.15)]]
    x = mix(*parts)
    swell = [min(1.0, i / (0.4 * RATE)) * min(1.0, (len(x) - i) / (0.5 * RATE)) for i in range(len(x))]
    return mul(x, swell)


def bell(f, secs=0.9):
    return mix((tone(secs, f, decay=0.3), 1.0, 0), (tone(secs, f * 2.01, decay=0.15), 0.3, 0),
               (tone(secs, f * 3.02, decay=0.08), 0.15, 0))


def joined(k):
    return mix((bell(659), 1.0, 0), (bell(880), 0.9, 0.18))


def level_up(k):
    return mix((bell(523, 0.6), 0.8, 0), (bell(659, 0.6), 0.8, 0.09), (bell(784, 0.7), 0.9, 0.18))


def pods(k):
    return mix((thud(0), 1.0, 0), (metal([700, 1650, 2900], 0.4, 0.1, k), 0.4, 0.02))


def ui_click(k):
    return tone(0.03, 2100, 1800, decay=0.006)


def ui_place(k):
    return mul(tone(0.07, 520, 820), env(int(0.07 * RATE), 0.004, 0.025))


def ui_open(k):
    return mix((mul(tone(0.09, 700, 1100), env(int(0.09 * RATE), 0.01, 0.03)), 0.6, 0),
               (shaped_noise(0.08, 2000, 6000, 0.01, 0.02), 0.2, 0))


# ---------- ambience (seamless loops) ----------
# Sparse sounds over silence: no continuous noise bed, which came across as
# an annoying wind or waterfall.

def amb_day(secs):
    x = silence(secs)
    n = len(x)
    t = 0.8
    while t < secs - 0.5:
        f = rng.uniform(2600, 4400)
        for c in range(rng.randint(2, 4)):
            chirp = mul(tone(0.07, f, f * rng.uniform(0.8, 1.3)), env(int(0.07 * RATE), 0.01, 0.025))
            o = int((t + c * 0.11) * RATE)
            for i, v in enumerate(chirp):
                if o + i < n:
                    x[o + i] += v * rng.uniform(0.5, 1.0)
        t += rng.uniform(1.8, 4.5)
    return x


def amb_night(secs):
    x = silence(secs)
    n = len(x)
    for cricket in range(3):
        f = rng.uniform(4000, 4800)
        period = rng.uniform(0.6, 1.0)
        t = rng.uniform(0, period)
        g = rng.uniform(0.5, 1.0)
        while t < secs - 0.2:
            for p in range(3):
                chirp = mul(tone(0.025, f), env(int(0.025 * RATE), 0.004, 0.01))
                o = int((t + p * 0.04) * RATE)
                for i, v in enumerate(chirp):
                    if o + i < n:
                        x[o + i] += v * g
            t += period * rng.uniform(0.9, 1.1)
    return x


def amb_cave(secs):
    x = silence(secs)
    n = len(x)
    t = 0.5
    while t < secs - 0.6:
        f = rng.uniform(800, 1300)
        drip = mul(tone(0.08, f, f * 1.6), env(int(0.08 * RATE), 0.002, 0.02))
        for echo, g in [(0.0, 1.0), (0.23, 0.3), (0.47, 0.1)]:
            o = int((t + echo) * RATE)
            for i, v in enumerate(drip):
                if o + i < n:
                    x[o + i] += v * g
        t += rng.uniform(1.5, 3.5)
    return x


def main():
    variants("step_grass", 4, step_grass)
    variants("step_stone", 4, step_stone)
    variants("step_wood", 4, step_wood)
    variants("chop", 3, chop)
    variants("mine", 3, mine)
    variants("dig", 2, dig)
    variants("hammer", 3, hammer)
    variants("anvil", 2, anvil)
    variants("chisel", 2, chisel)
    variants("rustle", 3, rustle)
    variants("sizzle", 2, sizzle)
    variants("saw", 2, saw)
    variants("craft", 3, craft)
    variants("furnace", 1, furnace)
    variants("tree_fall", 1, tree_fall)
    variants("rock_break", 2, rock_break)
    variants("built", 1, built)
    variants("rifle", 3, lambda k: gunshot(k))
    variants("pistol", 3, lambda k: gunshot(k, pitch=1.3, length=0.7))
    variants("swing", 2, swing)
    variants("hit", 3, hit)
    variants("glance", 2, glance)
    variants("death", 1, death)
    variants("thud", 2, thud)
    variants("door", 2, door)
    variants("raid", 1, horn)
    variants("joined", 1, joined)
    variants("level_up", 1, level_up)
    variants("pods", 1, pods)
    variants("ui_click", 1, ui_click)
    variants("ui_place", 1, ui_place)
    variants("ui_open", 1, ui_open)
    write("amb_day", loop(amb_day, 12.0), peak=0.2)
    write("amb_night", loop(amb_night, 12.0), peak=0.12)
    write("amb_cave", loop(amb_cave, 12.0), peak=0.25)
    print("wrote", len([f for f in os.listdir(OUT) if f.endswith(".wav")]), "files to", os.path.normpath(OUT))


if __name__ == "__main__":
    main()
