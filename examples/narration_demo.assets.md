# `narration_demo` assets

The proof film for narration is `examples/narration_demo.film.jsonc`. What it
uses, and what is / isn't committed:

## Committed (in `examples/narration_demo/`)

- **`narration-bm_george-everything-you-are-<hash>.wav`** — the baked
  voice-over. This is the point of the whole design: the audio is a pre-rendered
  asset, so anyone who clones the repo can render the film's narration with
  **no Python and no voice model** — only ffmpeg. It is 33 s of 24 kHz mono, so
  small enough to commit.
- **`narration-bm_george-everything-you-are-<hash>.words.json`** — the word
  timings the bake wrote (real, model-derived). The film's lower-third captions
  are cued from these.

Regenerate both (needs the Kokoro venv — see `tools/narrate/README.md`):

```bash
showreel narrate examples/narration_demo.film.jsonc -A examples/narration_demo -o examples/narration_demo
```

The WAV is content-addressed by the script, so editing a line changes the
filename; if you edit the narration in the film, re-bake and commit the new
files (and delete the old ones).

## NOT committed

- **`city-film.mp4`** — the on-screen footage. This is output from the ASCII
  city engine (a separate project — a procedurally generated city rendered in a
  terminal, the subject the narration describes). It is a ~5 MB video and not
  part of this repo; the narration feature does not depend on this specific
  clip. `.gitignore` excludes it. To rebuild the demo you need *a* clip at this
  path — any footage works; the film references it as the bare name
  `city-film.mp4` resolved through `-A examples/narration_demo`. The reel used
  here is 720×392, 24 fps, ~32 s, no audio track.

## The rendered cut (committed, under `docs/`)

- **`docs/narration-demo.mp4`** (+ `.mobile.mp4`) — the proof. Re-render after a
  bake with:

```bash
showreel render examples/narration_demo.film.jsonc -A examples/narration_demo --crf 30 -o docs/narration-demo.mp4
```

If you change the film, re-bake (if the script changed), re-render both cuts,
and re-verify audio on each — the mobile cut is a *second* ffmpeg pass and must
be checked for a real level, the same as the other demo reels. Measured on the
committed cut: master mean −23.7 dB, the voice riding ~10 dB over the ducked
chiptune, music fading to silence at the tail; corruption ZCR 0.047 (speech);
narration prosody `pitch_var_st` 3.21 st (inside the 2.5–4.5 natural band).
