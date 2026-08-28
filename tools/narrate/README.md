# Narration — the Kokoro voice bake

ShowReel's narration feature (see `src/narration.rs`) speaks a film's script
with a neural TTS model. Because that model is a Python stack — not something
that runs in a terminal with nothing behind it — the synthesis is done **once,
ahead of rendering**, by `showreel narrate`, which bakes the voice-over to a
plain WAV asset. From then on `render` needs only ffmpeg. This directory is the
Python side of that bake.

## What's here

- **`kokoro_narrate.py`** — the driver `showreel narrate` shells out to. It is
  deliberately tiny: text in, per-line WAVs and **real per-word timings** out
  (Kokoro's model predicts a duration for every word). Pauses, concatenation,
  gain, verification and the manifest are all done in Rust, so the only thing
  Python does is turn text into speech. The binary embeds a copy of this file
  and writes it to a temp dir at bake time, so `showreel narrate` is
  self-contained — you do **not** need this repo checked out to run a bake, only
  the interpreter below.

## What a stranger needs to bake narration

Rendering a film whose narration is **already baked** (e.g. the committed demo)
needs nothing here — just ffmpeg. You only need this to bake your own.

You need a Python environment with [Kokoro](https://github.com/hexgrad/kokoro)
installed. Kokoro pins an older numpy, so use **Python 3.12** (not 3.13/3.14,
which have no wheel for it). With [`uv`](https://github.com/astral-sh/uv):

```bash
uv venv --python 3.12 ~/.local/share/kokoro-venv
~/.local/share/kokoro-venv/bin/python -m ensurepip        # kokoro shells out to pip on first run
~/.local/share/kokoro-venv/bin/pip install kokoro soundfile
```

Then point `showreel narrate` at it — it defaults to
`~/.local/share/kokoro-venv/bin/python`, or set `$SHOWREEL_KOKORO_PYTHON`, or
pass `--python`:

```bash
showreel narrate my.film.jsonc -A assets -o assets
# or:  SHOWREEL_KOKORO_PYTHON=/path/to/venv/bin/python showreel narrate ...
# or:  showreel narrate my.film.jsonc --python /path/to/venv/bin/python -o assets
```

The first run downloads the ~330 MB model from Hugging Face and installs a
spaCy model; subsequent runs are about real-time on CPU (no GPU needed).

## Two gotchas, both handled for you but worth knowing

- **`VIRTUAL_ENV` must be unset**, or Kokoro's first-run spaCy install throws a
  confusing "No virtual environment found". `showreel narrate` unsets it for the
  child process, and the driver pops it again defensively.
- **The venv needs `pip` inside it** — Kokoro shells out to install the spaCy
  model on first use. `ensurepip` above covers this.

## The voice, and other voices

The captain chose Kokoro `bm_george` (a British male read) on measured pitch
variation; it is the default voice. But **the voice and the engine are film
parameters**, not baked into ShowReel — a film declares `engine` and `voice`,
Kokoro is one engine behind a `SynthEngine` seam (`src/narrate.rs`), and adding
another model is a new engine there plus a driver like this one. List Kokoro's
voices with the model; any `a`-prefixed id is American, `b`-prefixed British.

## Verifying a bake

`showreel narrate` runs a native corruption check (zero-crossing rate of the
loudest window — real speech ~0.13, white noise ~0.49) on every line and on the
finished assembly, and **refuses to write** if it measures as noise. To score
how natural a take sounds, or to check a WAV by hand, use the standalone tools
in `tools/prosody/`.
