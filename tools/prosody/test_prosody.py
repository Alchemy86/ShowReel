#!/usr/bin/env python3
"""Tests for the prosody + corruption tools. numpy-only, no Kokoro.

Run:  python3 tools/prosody/test_prosody.py

Every case synthesises a signal with known properties (a steady tone, a
warbled tone, white noise) and asserts the tools report what they should — so
the harness is checked without a voice model in the loop.
"""
import os
import sys
import tempfile
import unittest
import wave

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check  # noqa: E402
import prosody  # noqa: E402

SR = 24000


def tone(freq, dur=2.0, sr=SR, vib=0.0, amp=0.5):
    """A sine (optionally vibrato'd), as int16 mono."""
    t = np.arange(int(dur * sr)) / sr
    phase = 2 * np.pi * freq * t
    if vib:
        phase += vib * np.sin(2 * np.pi * 5.0 * t)
    return (np.sin(phase) * amp * 32767).astype("<i2")


def white_noise(dur=2.0, sr=SR, amp=0.5, seed=1):
    rng = np.random.default_rng(seed)
    return (rng.uniform(-1, 1, int(dur * sr)) * amp * 32767).astype("<i2")


def write_wav(samples, sr=SR, channels=1):
    fd, path = tempfile.mkstemp(suffix=".wav")
    os.close(fd)
    with wave.open(path, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(samples.tobytes())
    return path


class CorruptionCheck(unittest.TestCase):
    def test_a_steady_tone_reads_as_speech(self):
        # A 150 Hz tone crosses zero ~300 times/sec: ZCR ~0.0125, well under
        # the 0.30 gate.
        z = check.zero_crossing_rate(write_wav(tone(150)))
        self.assertLess(z, 0.05, f"tone ZCR {z}")

    def test_white_noise_reads_as_noise(self):
        z = check.zero_crossing_rate(write_wav(white_noise()))
        self.assertGreater(z, 0.35, f"noise ZCR {z}")
        self.assertGreaterEqual(z, check.THRESHOLD)

    def test_reads_the_header_sample_rate_and_downmixes_stereo(self):
        # A stereo file at 48 kHz must still measure, not throw.
        stereo = np.repeat(tone(150, sr=48000)[:, None], 2, axis=1).reshape(-1)
        z = check.zero_crossing_rate(write_wav(stereo, sr=48000, channels=2))
        self.assertLess(z, 0.05, f"stereo tone ZCR {z}")


class Prosody(unittest.TestCase):
    def test_a_steady_tone_has_near_zero_pitch_variation(self):
        m = prosody.measure(write_wav(tone(150, dur=3.0)))
        self.assertIsNotNone(m)
        self.assertLess(m["pitch_var_st"], 0.8, f"steady pitch_var {m['pitch_var_st']}")

    def test_vibrato_raises_pitch_variation(self):
        steady = prosody.measure(write_wav(tone(150, dur=3.0)))
        warbled = prosody.measure(write_wav(tone(150, dur=3.0, vib=1.2)))
        self.assertIsNotNone(warbled)
        self.assertGreater(
            warbled["pitch_var_st"],
            steady["pitch_var_st"] + 0.3,
            f"vibrato {warbled['pitch_var_st']} vs steady {steady['pitch_var_st']}",
        )

    def test_load_reads_the_real_sample_rate(self):
        x, sr = prosody.load(write_wav(tone(150), sr=24000))
        self.assertEqual(sr, 24000)
        x, sr = prosody.load(write_wav(tone(150), sr=22050))
        self.assertEqual(sr, 22050)


if __name__ == "__main__":
    unittest.main(verbosity=2)
