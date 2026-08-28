#!/usr/bin/env python3
"""Measure how natural a piece of synthesised speech sounds.

Naturalness is not purely subjective. The single strongest correlate of
"flat" delivery is PITCH VARIATION, and it has a known human range. Measured
in semitones (perceptual, so it does not depend on whether the voice is high
or low), read-aloud English narration sits around 2.5-4.5 st standard
deviation. Monotone TTS comes in near 1.5; over-driven TTS overshoots and
reads as sing-song or unstable.

Also measured: speaking rate, and how much the pause lengths vary - a person
does not leave identical gaps, and identical gaps are a big part of why
synthetic narration sounds mechanical.

This is the harness the ShowReel narration feature was tuned against; the
pitch-variation figure is what the captain compared voices on (Kokoro
`bm_george` measured 3.21 st, inside the natural band). It is numpy-only (no
scipy) and reads the sample rate from the WAV header, so it measures a 24 kHz
Kokoro file and a 22.05 kHz reference alike.

    python3 tools/prosody/prosody.py voice.wav [more.wav ...]

The `pitch_var_st` metric is rate-independent (it is a distribution of pitch,
not of time), so it is comparable across files and sample rates; the pause and
rate metrics are in real seconds because the true rate is read, not assumed.
"""
import struct
import sys

import numpy as np


def load(path):
    """Read a canonical PCM WAV: returns (mono float samples in [-1,1), rate)."""
    with open(path, "rb") as f:
        b = f.read()
    if b[0:4] != b"RIFF" or b[8:12] != b"WAVE":
        raise ValueError(f"{path}: not a RIFF/WAVE file")
    rate, channels, data = 22050, 1, b""
    pos = 12
    while pos + 8 <= len(b):
        cid = b[pos:pos + 4]
        size = struct.unpack("<I", b[pos + 4:pos + 8])[0]
        body = b[pos + 8:pos + 8 + size]
        if cid == b"fmt " and len(body) >= 16:
            channels = struct.unpack("<H", body[2:4])[0] or 1
            rate = struct.unpack("<I", body[4:8])[0]
        elif cid == b"data":
            data = body
        pos += 8 + size + (size & 1)
    n = (len(data) // 2) * 2
    x = np.frombuffer(data[:n], dtype="<i2").astype(np.float64) / 32768.0
    if channels > 1:  # downmix to mono
        x = x[: (len(x) // channels) * channels].reshape(-1, channels).mean(axis=1)
    return x, rate


def _medfilt(x, k=5):
    """A width-k median filter, numpy-only (replaces scipy.signal.medfilt)."""
    if len(x) < k:
        return x
    pad = k // 2
    xp = np.pad(x, pad, mode="edge")
    stack = np.stack([xp[i:i + len(x)] for i in range(k)])
    return np.median(stack, axis=0)


def f0_track(x, sr, fmin=60, fmax=300, hop=512, win=1024):
    """Autocorrelation pitch tracker. Returns f0 per frame, 0 = unvoiced."""
    out = []
    lo, hi = int(sr / fmax), int(sr / fmin)
    for s in range(0, len(x) - win, hop):
        w = x[s:s + win]
        e = np.sqrt((w ** 2).mean())
        if e < 0.01:
            out.append(0.0)
            continue
        w = w - w.mean()
        ac = np.correlate(w, w, "full")[win - 1:]
        if ac[0] <= 0:
            out.append(0.0)
            continue
        ac = ac / ac[0]
        seg = ac[lo:hi]
        if len(seg) == 0:
            out.append(0.0)
            continue
        k = int(np.argmax(seg)) + lo
        out.append(sr / k if ac[k] > 0.35 else 0.0)
    return np.array(out)


def pauses(x, sr, thresh_db=-45, minlen=0.12):
    win = int(sr * 0.02)
    fr = np.array([np.sqrt((x[i:i + win] ** 2).mean() + 1e-12) for i in range(0, len(x) - win, win)])
    db = 20 * np.log10(fr + 1e-12)
    quiet = db < thresh_db
    runs = []
    c = 0
    for q in quiet:
        if q:
            c += 1
        else:
            if c * 0.02 >= minlen:
                runs.append(c * 0.02)
            c = 0
    if c * 0.02 >= minlen:
        runs.append(c * 0.02)
    return runs, float(quiet.mean())


def measure(path):
    x, sr = load(path)
    dur = len(x) / sr
    f0 = f0_track(x, sr)
    v = f0[f0 > 0]
    v = _medfilt(v, 5) if len(v) > 5 else v
    if len(v) < 20:
        return None
    # semitones relative to the median -> perceptual, voice-independent
    st = 12 * np.log2(v / np.median(v))
    st = st[np.abs(st) < 12]  # drop octave errors
    p, quiet_frac = pauses(x, sr)
    voiced_s = len(v) * 512 / sr
    return dict(
        dur=dur,
        f0_med=float(np.median(v)),
        pitch_var_st=float(st.std()),
        pitch_range_st=float(np.percentile(st, 90) - np.percentile(st, 10)),
        n_pauses=len(p),
        pause_mean=float(np.mean(p)) if p else 0.0,
        pause_var=float(np.std(p)) if len(p) > 1 else 0.0,
        quiet_frac=quiet_frac,
        speech_rate=voiced_s / dur,
    )


# Targets for read-aloud English narration.
TARGET = dict(
    pitch_var_st=(2.5, 4.5),
    pitch_range_st=(6.0, 11.0),
    pause_var=(0.10, 0.45),
    quiet_frac=(0.18, 0.40),
    speech_rate=(0.40, 0.62),
)


def score(m):
    """Distance from the natural band. 0 = inside every band."""
    tot = 0.0
    detail = {}
    for k, (lo, hi) in TARGET.items():
        val = m[k]
        if val < lo:
            d = (lo - val) / (hi - lo)
        elif val > hi:
            d = (val - hi) / (hi - lo)
        else:
            d = 0.0
        detail[k] = (val, lo, hi, d)
        tot += d
    return tot, detail


if __name__ == "__main__":
    for p in sys.argv[1:]:
        m = measure(p)
        if not m:
            print(f"{p}: too little voiced audio")
            continue
        s, d = score(m)
        print(f"\n=== {p.split('/')[-1]}  ({m['dur']:.1f}s, median pitch {m['f0_med']:.0f} Hz)")
        print(f"  {'metric':<16}{'value':>8}  {'natural band':<14} verdict")
        for k, (val, lo, hi, dd) in d.items():
            v = "OK" if dd == 0 else ("LOW" if val < lo else "HIGH")
            print(f"  {k:<16}{val:>8.2f}  {lo:.2f} - {hi:<8.2f} {v}")
        print(f"  SCORE {s:.2f}  (0 = natural on every measure)")
