#!/usr/bin/env python3
"""Kokoro narration driver — the *only* Python ShowReel shells out to.

This is deliberately the narrowest possible surface: it turns text into speech
and reports where each word lands. It does NOT insert pauses, concatenate
lines, apply gain, or mix — all of that is done in Rust (see `src/narration.rs`
and `src/narrate.rs`), so the directed timing and the assembly stay testable
without a Python stack behind them. One job in, per-line WAVs + word timings
out.

Word timings are REAL, not a heuristic: Kokoro's model predicts a duration for
every phoneme (`pred_dur`), and `KPipeline.join_timestamps` turns those into a
start/end time per token. We read `.start_ts`/`.end_ts` straight off the
tokens. See `KPipeline.join_timestamps` in the installed kokoro package.

# Job format (JSON, path given as argv[1], or on stdin)

    {
      "voice": "bm_george",       # a Kokoro voice id
      "lang": "b",                # 'b' British, 'a' American (derived from voice)
      "sample_rate": 24000,       # Kokoro's native rate
      "result": "/abs/out/timings.json",   # where to write the result JSON
      "lines": [
        {"id": 0, "text": "...", "speed": 1.0, "wav": "/abs/out/line-0.wav"},
        ...
      ]
    }

# Result (JSON written to the job's `result` path)

    {
      "sample_rate": 24000,
      "lines": [
        {"id": 0, "duration": 4.13,
         "words": [{"text": "Everything", "start": 0.02, "end": 0.51}, ...]},
        ...
      ]
    }

The result is written to a *file* (`result`), never stdout — kokoro and its
dependencies print progress and repo-id warnings to stdout, which would
otherwise corrupt the JSON. Each line's audio is written to its `wav` path as
24 kHz mono 16-bit PCM. The caller (Rust) verifies every one of those files for
corruption before using it.

# Environment gotchas (documented, and handled defensively here)

  * VIRTUAL_ENV must be unset, or kokoro's first-run spaCy install throws a
    confusing "No virtual environment found". The Rust caller unsets it; we
    pop it again here so the script is also correct when run by hand.
  * The venv needs `pip` inside it — kokoro shells out to install the spaCy
    model on first use.
  * numpy is pinned to 1.26.4, which has no wheel for Python 3.14; the venv is
    Python 3.12 for exactly this reason. Don't recreate it against system
    Python.
"""
import json
import os
import sys

# Must happen before kokoro/torch import triggers any subprocess of its own.
os.environ.pop("VIRTUAL_ENV", None)


def load_job(argv):
    if len(argv) > 1 and argv[1] != "-":
        with open(argv[1]) as f:
            return json.load(f)
    return json.load(sys.stdin)


def synth_line(pipeline, voice, text, speed, sample_rate):
    """Synthesise one line. A line is normally one segment, but Kokoro may
    split a long one; we concatenate the audio and stitch the word timings,
    offsetting each later segment by the audio already emitted."""
    import numpy as np

    audio_parts = []
    words = []
    offset = 0.0  # seconds of audio emitted by earlier segments of this line
    for result in pipeline(text, voice=voice, speed=speed):
        if result.audio is None:
            continue
        chunk = result.audio.detach().cpu().numpy()
        if result.tokens:
            for t in result.tokens:
                w = (t.text or "").strip()
                if not w or t.start_ts is None or t.end_ts is None:
                    continue
                words.append(
                    {
                        "text": w,
                        "start": round(offset + float(t.start_ts), 3),
                        "end": round(offset + float(t.end_ts), 3),
                    }
                )
        audio_parts.append(chunk)
        offset += len(chunk) / sample_rate

    if not audio_parts:
        return np.zeros(0, dtype=np.float32), []
    return np.concatenate(audio_parts), words


def main():
    job = load_job(sys.argv)
    voice = job["voice"]
    lang = job.get("lang") or ("b" if voice.startswith("b") else "a")
    sample_rate = int(job.get("sample_rate", 24000))

    import numpy as np
    import soundfile as sf
    from kokoro import KPipeline

    pipeline = KPipeline(lang_code=lang)

    out_lines = []
    for line in job["lines"]:
        audio, words = synth_line(
            pipeline, voice, line["text"], float(line.get("speed", 1.0)), sample_rate
        )
        # Write 16-bit PCM mono so the Rust side reads it with the same tiny
        # WAV reader it uses for every other track.
        sf.write(line["wav"], audio, sample_rate, subtype="PCM_16")
        out_lines.append(
            {
                "id": line["id"],
                "duration": round(len(audio) / sample_rate, 3),
                "words": words,
            }
        )
        print(
            f"line {line['id']}: {len(audio)/sample_rate:.2f}s, {len(words)} words",
            file=sys.stderr,
        )

    result = {"sample_rate": sample_rate, "lines": out_lines}
    # Write to a file, not stdout: kokoro prints warnings to stdout that would
    # otherwise corrupt the JSON the caller parses.
    result_path = job.get("result")
    if result_path:
        with open(result_path, "w") as f:
            json.dump(result, f)
    else:
        json.dump(result, sys.stdout)
        sys.stdout.flush()


if __name__ == "__main__":
    main()
