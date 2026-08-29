# Clip decoding: bounded memory, measured

`Clip::load` (`src/assets/clip.rs`) used to decode an entire source video into
`Vec<Pixmap>` and hold every frame in RAM for the render's whole life. A
minute of gameplay footage across a few clips could put a render into
double-digit gigabytes; on 2026-08-29 this OOM-killed the machine mid-render
and cost hours of unsaved work.

This document is the measured evidence behind the fix — see `src/assets/clip.rs`'s
module doc for the design itself (eager-vs-streaming, the frame cache, why
`frame_at` now returns `Result<Option<Arc<Pixmap>>>`).

## Diagnosis, verified

The claim: at 1920×1080, one RGBA frame is `1920*1080*4` = 8,294,400 bytes —
matches the arithmetic exactly. The real question was whether `Clip::load`
actually holds all of them at once. It did — confirmed by reading the old
`Clip::load` (a single `.output()` call materializing the whole decode into
`Vec<Pixmap>`) and by measuring real RSS below, not by taking the arithmetic
on faith.

## Method

Two builds compared: `origin/main` (`4011f74`, pre-fix) and this branch
(post-fix), both `cargo build --release`. Real footage, not synthetic:
`mt-moon-20-25.mp4` (1280×720, 60fps, 138s, from `~/pokemon-run`, not
committed — any real clip reproduces this). Decoded at 640×360, 15fps (kept
deliberately modest: this machine was under real memory pressure — other
renders running, swap nearly full — while these numbers were taken, so the
test sizes were chosen to be conclusive without risking another OOM). Three
`trim` lengths — 10s/30s/90s, a 9x spread — to show the pre-fix number scale
with length and the post-fix number not. Peak RSS via `/usr/bin/time -v`
(`Maximum resident set size`).

Two harnesses:
- `examples/rss_probe.rs`: calls `Clip::load` directly, then walks every
  frame forward once, single-threaded — isolates the clip decoder from the
  renderer.
- A real `showreel render` of a one-scene, one-clip-layer film
  (`--crf 30 --no-mobile`, clip audio muted — the source has no audio track
  and mixing it is not what this measures) — the actual user-facing path,
  full `rayon` parallelism (20 cores on the test machine).

## Results

### `Clip::load` in isolation (`rss_probe`)

| length | frames | pre-fix peak RSS | post-fix peak RSS | pre-fix time | post-fix time |
|---|---|---|---|---|---|
| 10s | 150 | 266 MB | 266 MB *(eager, unchanged)* | 0.54s | 0.55s |
| 30s | 450 | 794 MB | **152 MB** | 1.28s | 2.00s |
| 90s | 1350 | 2,380 MB | **157 MB** | 3.66s | 4.82s |

Pre-fix grows linearly with length (266→794→2,380 MB, tracking frame count
almost exactly). Post-fix is flat past the eager/streaming threshold
(152→157 MB for a 3x longer clip) — the 256 MiB `EAGER_MAX_BYTES` threshold
means the 10s case (150 frames × 640×360×4 ≈ 132 MB) stays on the eager path
either build, unchanged on purpose (see the module doc: a short, reused clip
is legitimately cheaper eager).

The 30s/90s time cost (single-threaded, forward, no reseeks needed) is real:
1.6-1.3x slower, because the eager path pays its whole decode cost once up
front while streaming pays a `read_exact` per frame on demand. This is the
honest trade for the isolated case — the full-render numbers below tell a
different story once real parallelism is in the picture.

### A real render (`showreel render`, 20-core parallel, real ffmpeg encode)

| length | frames | pre-fix peak RSS | post-fix peak RSS | pre-fix time | post-fix time |
|---|---|---|---|---|---|
| 10s | 150 | 460 MB | 460 MB *(eager, unchanged)* | 1.15s | 1.14s |
| 30s | 450 | 1,001 MB | **367 MB** | 2.51s | 2.59s |
| 90s | 1350 | 2,624 MB | **363 MB** | 5.99s | 6.18s |

Same linear-vs-flat shape (367→363 MB across a 3x length change), and this
time render **time is a wash** (within ~3-4%, noise on a shared machine) —
the streaming cache is sized off the render's own parallel chunk width
(`stream_cache_frames`, tied to `rayon::current_num_threads()`), so the
chunk-local out-of-order access `render.rs` actually does mostly lands in
the cache rather than forcing a reseek. The isolated probe's slowdown above
was a single-threaded worst case that the real renderer doesn't hit.

A bonus, unplanned result: `showreel still` at one arbitrary timestamp into
a large clip got **faster**, not slower — it no longer decodes the whole
clip up to that point just to answer one query. At t=89.9s into the 90s
clip: 6.9s pre-fix vs 4.2s post-fix. This also answers the `showreel studio`
scrubbing concern raised during design: random single-point access into a
big clip was exactly the case a naive "drop everything behind a moving
window" design would have handled badly; the seek-and-cache design instead
makes it cheaper.

### Output correctness

`showreel still` at 5 timestamps per length (15 total; including the very
start and within 0.1s of the trim's end, the boundary case Hold-mode
clamping would get wrong first) — pre-fix and post-fix PNGs compared with
`cmp`. **15/15 byte-identical.** `src/assets/clip.rs`'s own
`streaming_matches_eager_frame_for_frame` test covers the same claim at the
unit level, forcing the streaming path on a small synthetic clip and
comparing every frame against the eager path across a deliberately
scrambled, repeated, backward-then-forward time sequence — standing in for
`render.rs`'s out-of-order parallel chunk access.

## What this doesn't cover

- Not tested at the brief's own 1920×1080 example scale — the machine was
  under real memory pressure from other work while these numbers were
  taken, so resolution was kept modest by design (see Method). The
  per-frame byte arithmetic is resolution-independent, and 640×360's 9x
  length spread already proves the flat-vs-linear shape; a follow-up with a
  quiet machine should confirm the same shape at 1080p.
- The streaming cache is sized to `[8, 128]` frames, capped at 512 MiB
  regardless of resolution (`stream_cache_frames` in `src/assets/clip.rs`) —
  a very high core count on a very large frame size is the one case that
  could still push a single streaming clip's memory higher than these
  numbers suggest; the byte cap exists specifically to bound that.
