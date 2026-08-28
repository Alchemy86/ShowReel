#!/usr/bin/env python3
"""The corruption check: is a WAV real speech, or plausible-looking noise?

A neural vocoder can emit audio that passes every duration, codec and decode
check and is still white noise. The one measure that separates them outright is
the ZERO-CROSSING RATE of the loudest one-second window: real speech comes in
around 0.10-0.15, white noise around 0.49. This is the gate that stopped broken
audio being shipped, and ShowReel runs the same check natively before it writes
any baked narration (`narration::zero_crossing_rate`) — this script is the
standalone reference, stdlib-only, runnable anywhere.

    python3 tools/prosody/check.py voice.wav
    # prints the zero-crossing rate; exits 0 if speech (< 0.30), 1 if noise.

Reads the sample rate from the WAV header so the window really is one second,
and downmixes a stereo file to mono, so it measures a 24 kHz Kokoro line and a
48 kHz mixed export alike.
"""
import array
import struct
import sys

THRESHOLD = 0.30  # >= this is noise, not speech


def read_wav(path):
    with open(path, "rb") as f:
        b = f.read()
    rate, channels, data = 22050, 1, b""
    if b[0:4] == b"RIFF" and b[8:12] == b"WAVE":
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
    else:  # headerless: treat the whole thing as PCM
        i = b.find(b"data")
        data = b[i + 8:] if i >= 0 else b
    n = (len(data) // 2) * 2
    a = array.array("h")
    a.frombytes(data[:n])
    if channels > 1:  # downmix
        a = array.array("h", (sum(a[i:i + channels]) // channels for i in range(0, len(a) - channels + 1, channels)))
    return a, rate


def zero_crossing_rate(path):
    a, sr = read_wav(path)
    if len(a) < 2:
        return 0.0
    win = max(2, sr)
    step = max(1, sr // 2)
    best = (0, 0)  # (energy, start)
    for s in range(0, max(1, len(a) - win), step):
        w = a[s:s + win]
        if not w:
            continue
        e = sum(abs(x) for x in w) / len(w)
        if e > best[0]:
            best = (e, s)
    w = a[best[1]:best[1] + win]
    if len(w) < 2:
        w = a
    z = sum(1 for i in range(1, len(w)) if (w[i - 1] < 0) != (w[i] < 0)) / len(w)
    return z


if __name__ == "__main__":
    z = zero_crossing_rate(sys.argv[1])
    print(f"{z:.3f}")
    sys.exit(0 if z < THRESHOLD else 1)
