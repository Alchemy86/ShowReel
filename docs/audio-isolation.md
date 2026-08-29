# Audio isolation: the licence gate, and what was actually measured

`showreel isolate` (see `src/isolate.rs`'s module doc for the design) splits a
video or audio file's soundtrack into a speech stem and a music/effects stem,
and can mux a chosen stem back against the original picture. This document is
the licence research behind which model it defaults to, and the measured
evidence — not an assertion — for how well it actually separates.

## The licence gate

Source separation is a solved problem with several mature pretrained models.
The question that matters is not which sounds best; it is which one we are
actually allowed to use — **code and model-weight licences are independent
facts, checked separately for each model**. This project has been caught by
exactly that split before: XTTS-v2's code was MIT, its weights were
non-commercial, which ruled it out.

| Model | Code licence | Pretrained-weights licence | Verdict |
|---|---|---|---|
| **Spleeter** (Deezer) | MIT | **MIT.** Deezer trained on its own private catalogue — the project's own paper: *"we cannot release the training data for copyright reasons... sharing pre-trained models were the only way to make these results available to the community"* — and released the resulting weights under the same MIT licence as the code. Confirmed by fetching `github.com/deezer/spleeter`'s `LICENSE` directly: one file, no carve-out for models. That is Deezer's call to make, since it owns the training data. | **Usable anywhere, commercial included.** |
| **Demucs** (Meta, `htdemucs` family) | MIT | **Not covered by the MIT licence.** The maintainer, on the record, in [facebookresearch/demucs#327](https://github.com/facebookresearch/demucs/issues/327): *"The model weights are not covered by the MIT license, and are provided only for scientific purposes."* Every released weight set (`htdemucs`, `htdemucs_ft`, `mdx_extra`, ...) is trained wholly or partly on MUSDB18(-HQ), whose own terms restrict it to academic/research use (confirmed via the dataset's own licensing page); a second maintainer comment in the same issue makes the causal link explicit: *"The models are trained using MusDB dataset, which requires the result model can only be used for research purpose."* The restriction traces to the training data, not a Meta-specific clause. | **Research/non-commercial use only.** Best measured quality of the three — see below. |
| **Open-Unmix** | MIT | `umxl`: explicit **CC BY-NC-SA 4.0** (non-commercial), stated directly in the project's own README. `umx`/`umxhq`: no explicit commercial grant found, and both are trained on the same MUSDB18(-HQ) that gates Demucs — no cleaner than Demucs, so not investigated further. | Not used. |

**`Model::Spleeter` is the default for exactly this reason: it is the only
option whose weights carry the same permissive terms as its code.**
`Model::Demucs` is offered as `--model demucs` because it is real and
measurably better on this kind of source (below) — but every invocation
prints a loud warning naming the research-only restriction (`src/isolate.rs`),
the same way a corrupt narration bake refuses silently to become a working
one.

**What this does and does not affect.** The restriction is on *distributing
or shipping* something built on the `htdemucs` weights without a licence from
Meta — a commercial product, a public release, anything leaving the room. It
does not reach backwards: a prior task on this machine
(`data/voice-clone-qwert/report.md`) already installed Demucs and ran it
locally to clean a voice reference before feeding it to a separate cloning
step — private, internal use, nothing distributed — which is squarely within
"provided for scientific purposes" and is not in tension with the finding
here. The gate matters the moment `showreel isolate --model demucs`'s output
leaves internal/research use.

## Setup

Each model gets its **own** venv — Spleeter pins TensorFlow 2.12.1 on Python
`<3.12`; Demucs wants a current PyTorch on Python 3.12 — forcing them into one
environment would pin one against the other for no reason. Point
`showreel isolate --model <name>` at either with `--python`, or
`$SHOWREEL_SPLEETER_PYTHON`/`$SHOWREEL_DEMUCS_PYTHON`, or just create them at
the default paths below.

```bash
# Spleeter — needs Python <3.12
uv venv --python 3.11 ~/.local/share/spleeter-venv
~/.local/share/spleeter-venv/bin/python -m ensurepip
~/.local/share/spleeter-venv/bin/python -m pip install spleeter soundfile

# Demucs — needs numpy/soundfile installed explicitly; demucs's own
# dependency list does not pull them in (found the hard way: `import
# demucs.pretrained` fails with `ModuleNotFoundError: No module named
# 'numpy'` right after a clean `pip install demucs`).
uv venv --python 3.12 ~/.local/share/demucs-venv
~/.local/share/demucs-venv/bin/python -m ensurepip
~/.local/share/demucs-venv/bin/python -m pip install demucs numpy soundfile
```

Both models cache their pretrained weights out-of-tree and persistently —
Spleeter under `MODEL_PATH`, which `run_separation` pins to the venv's own
directory (left unset, Spleeter defaults to a *relative* `pretrained_models/`
in whatever directory `showreel` happens to be invoked from — it landed in
this repo's root once, before this was caught); Demucs under its own
Hugging Face Hub cache (`~/.cache/huggingface/hub`), no override needed. The
first run of either downloads its weights (Spleeter ~80 MB, Demucs's
`htdemucs` ~80 MB); after that both run offline.

## Verified quality: real narrated material, not synthetic

The film's own `docs/narration-demo.mp4` (35.5s, Kokoro `bm_george` narration
ducked under a generated chiptune bed — see `examples/narration_demo/`) is a
genuine speech-over-music test case, and it comes with something better than
a listen: the **exact pre-mix narration WAV** the film was built from
(`examples/narration_demo/narration-bm_george-*.wav`) is committed. That
means "how close is the separated speech stem to the truth" is not a guess —
it is a direct waveform comparison against the file the mix started from,
narration starts at the film's own `"at": 1.4` offset.

Method: `showreel isolate docs/narration-demo.mp4 --model <m>`, then compare
the `speech.wav`/`music.wav` it wrote against the ground-truth WAV with a
small numpy script (normalized cross-correlation, envelope correlation, RMS
windows) — not a single SDR figure, several separate measurements each
checking a specific, named kind of leakage:

| Check | Spleeter | Demucs (`htdemucs`) | What it means |
|---|---|---|---|
| Waveform correlation, speech stem vs. pristine pre-mix reference | 0.879 | **0.981** | Demucs reconstructs the actual voice far more faithfully — the gap the model's real-world reputation predicts. |
| RMS of the **speech** stem during the film's music-only intro (0–1.4s, no dialogue at all) | **−101.4 dBFS** (essentially digital silence) | −77.9 dBFS (very quiet, but a real, measurable residue) | Spleeter suppresses a no-speech passage more completely; Demucs leaves a faint, almost certainly inaudible, trace of the music in the speech stem. |
| Envelope correlation, ground-truth narration timing vs. **music** stem energy | −0.142 | −0.015 | Both near zero: neither model's music stem visibly "pulses" in time with the dialogue — speech is not audibly ghosting into the music track on this source. |

Reading this honestly: **Demucs is the better separator on the actual
voice** (0.98 vs 0.88 correlation is not marginal), which matches the
`voice-clone-qwert` precedent measuring it cleaning a reference clip. Its one
measured weakness here is a small, quiet residue of music left in the speech
stem during a fully instrumental passage — audible only as a very faint bed
under near-silence, not as intelligible bleed-through. Spleeter's older,
mask-based approach achieves cleaner silence in that specific case but
reconstructs the voice itself less faithfully throughout. Neither model
showed the dialogue ghosting into the music stem on this source; that check
would need a louder, busier bed than this film's calm 0.22-gain chiptune to
stress properly — a real limit of this test source, named rather than
papered over.

Both stems pass the crate's own speech/noise gate
(`narration::looks_like_speech`, zero-crossing rate < 0.30 — the same check a
narration bake refuses to write past): Spleeter's speech stem measured 0.036,
Demucs's 0.047. Not a quality score — it only confirms the output is speech
at all, not noise.

## Speed and memory — measured, this machine

`/usr/bin/time -v` around the whole `showreel isolate` invocation (extraction
+ separation + stem write), 35.5s of input audio, CPU only, no GPU used by
either model:

| Model | Wall clock | Peak RSS | Real-time factor |
|---|---|---|---|
| Spleeter | 13.2s | 1.60 GB | ~2.7× faster than real time |
| Demucs (`htdemucs`) | 17.2s | 1.52 GB | ~2.1× faster than real time |

Both comfortably fit the ~11 GB the machine had free when this was measured;
neither is the memory risk the render path's own clip decoding was. Demucs's
first run also pays a one-time ~80 MB weight download.

## What was not built

- **Neither model is bundled or downloaded automatically.** Consistent with
  narration's Kokoro bake: a real model needs an explicit, pinned venv the
  operator sets up once, not something `cargo build` or `showreel isolate`
  reaches for on its own.
- **No GPU path.** Both ran CPU-only above; a CUDA/MPS device would speed
  either up further but was not available to measure here, so it is not
  claimed.
- **No streaming/incremental separation.** A whole file is processed as one
  batch, matching every use case the brief named (a finished clip, not a live
  feed).
