# Prosody & corruption tools

Two small, dependency-light tools for judging synthesised speech. They are what
ShowReel's narration feature (`src/narration.rs`) was measured against, kept in
the repo because they are useful on their own — for choosing a voice, tuning
per-line pace, or checking any WAV.

## `check.py` — is it speech, or noise?

A neural vocoder can emit audio that passes every duration, codec and decode
check and is still white noise. The zero-crossing rate of the loudest one-second
window separates them outright: real speech ~0.10–0.15, white noise ~0.49.

```bash
python3 tools/prosody/check.py voice.wav    # prints the rate; exit 0 = speech (<0.30), 1 = noise
```

Stdlib-only, so it runs anywhere. ShowReel ports this same check to Rust
(`narration::zero_crossing_rate`) and runs it natively before writing any baked
narration — this script is the standalone reference.

## `prosody.py` — how natural does it sound?

Naturalness is not purely subjective. Its strongest correlate is **pitch
variation**, measured in semitones (perceptual, so it does not depend on whether
a voice is high or low). Read-aloud English narration sits around **2.5–4.5 st**;
monotone TTS comes in near 1.5, over-driven TTS overshoots and reads as
sing-song. The tool also reports speaking rate and how much the pauses vary
(identical gaps are a big part of what makes synthetic speech sound mechanical).

```bash
python3 tools/prosody/prosody.py voice.wav [more.wav ...]
```

Needs only **numpy** (no scipy). It reads the sample rate from the WAV header,
so it measures a 24 kHz Kokoro file and a 22.05 kHz reference alike; the
`pitch_var_st` figure is rate-independent and directly comparable across files.

The captain compared voices on `pitch_var_st` and chose Kokoro `bm_george`
(measured 3.21 st). The committed demo narration measures the same 3.21 st,
inside the natural band — the pipeline reproduces that number.

## Tests

```bash
python3 tools/prosody/test_prosody.py
```

numpy-only, no voice model: each case synthesises a signal with known properties
(a steady tone → near-zero pitch variation; a warbled tone → more; white noise →
fails the corruption gate) and asserts the tools report it.
