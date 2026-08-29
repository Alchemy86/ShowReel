# The render budget: bounded, machine-aware concurrency

The other half of the 2026-08-29 OOM incident (`docs/clip-streaming.md` bounded one
clip's own decode memory; `docs/segmented-rendering.md` made a killed render resumable).
Neither stopped the actual reboot: two concurrent `showreel render` invocations over a
large source (a 9248x1568 canvas, ~400MB 60fps footage) drove the kernel OOM killer,
which killed 45 processes — the whole desktop session, not just the render. The same
evening, a *single* camera pass measured at ~60% of one core on a 20-core box:
`showreel` was simultaneously capable of starving the machine of memory and leaving
nearly all its CPU idle, because nothing owned a resource budget.

`src/budget.rs` is that budget; `src/segments.rs`'s `render_segmented` is the one
caller. See both modules' own doc comments for the full design — this is the
verification record: what was measured, on what machine, and what it showed.

## What it does, in one paragraph

A render's first pending segment always renders serially, exactly as before — this
both preserves old behaviour when there's nothing to parallelise and gives an honest,
*measured* per-worker memory cost (system-wide `/proc/meminfo` availability, tightened
by the enclosing cgroup's own ceiling when there is one — not `showreel`'s own RSS,
which would miss the `ffmpeg` children that are most of a render's real footprint).
That measurement, the machine's real core count and available memory, and a small
cross-process ledger under the system temp directory (so a second concurrent
`showreel render` sees what the first has already claimed) together plan a worker
count — capped by memory, by cores, and by `--max-workers` if given. A worker stops
picking up further segments mid-render if headroom turns critical. Every render prints
what it decided and, at the end, what it actually used.

## Machine

All measurements below: Fedora Linux 44, 20 cores, 30 GiB RAM, on a shared host that —
per the captain's own caution before this work began — had another `showreel` render
actively running for part of this session. That is called out explicitly wherever it
confounds a number; it is not hidden.

## Proof 1: it degrades instead of dying

A real render (960x540, 120s/3600 frames, 15 segments, a genuine `ffmpeg`-decoded
streaming source — a synthesised 91MB clip, not a trivial one) under a hard cgroup
memory cap:

```
$ systemd-run --scope -p MemoryMax=3G --user -- showreel render sd.film.jsonc \
    -A assets -o constrained-out.mp4 --no-mobile --crf 23
sd.film.jsonc: 1 scenes, 120.00s, 960x540 at 30fps — 3600 frames
  segments 15 of ~240 frames (8s) each, none done yet
  segment 1/15 rendered (frames 0..240)
  budget  running serial, only 1.4 GB headroom (738 MB measured per worker, 0.0 GB already claimed by other renders)
  segment 2/15 rendered (frames 240..480)
  ...
  segment 15/15 rendered (frames 3360..3600)
  budget  peak 867 MB used, 1 worker at once, 33.6s wall
  3600 frames at 960x540 in 33.59s ...
  master  constrained-out.mp4
$ echo $?
0
```

`journalctl --user -u <that scope>` afterward: `Consumed 2min 14.998s CPU time over
33.825s wall clock time, 1019.9M memory peak` — no kernel OOM message, a clean exit,
comfortably under the 3 GiB cap. **This would not have shown up as a bound at all
against `/proc/meminfo` alone** — a `systemd-run --scope -p MemoryMax=`/container cap
changes nothing that file reports, only the cgroup's own `memory.max`/`memory.current`
(`/sys/fs/cgroup` under this process's own leaf, from `/proc/self/cgroup`) — which is
why `MachineState::probe`/`available_bytes_now` read both and take the tighter.

## Proof 2: cross-process safety

Two `showreel render` processes (different output files, same small five-segment film)
launched at the same instant:

```
A:  budget  4 workers (~298 MB each, measured; 7.1 GB headroom, 1.2 GB claimed elsewhere)
B:  budget  4 workers (~298 MB each, measured; 8.3 GB headroom, 0.0 GB claimed elsewhere)
```

`A` saw `B`'s live reservation in the shared ledger (`B` had reserved first); `B`, a
few milliseconds ahead, saw nothing yet from `A`. Both completed cleanly with 4 workers
each. This is the exact two-invocations-at-once shape the incident was — a budget that
only bounded one process would not have prevented it.

## Proof 3: the speedup is real

**A genuinely large, real-CLI comparison**, run back-to-back on the same busy machine
against the pre-change binary (`git archive` of the parent commit, built separately):
a 1280x720/30fps/200s film (6000 frames, 25 segments) — a title, a gradient and an
animated counter every frame, deliberately CPU-bound raster work with no clip decode,
chosen so it wouldn't also contend on disk I/O with the other render active on this
host at the time.

| | wall clock | workers |
|---|---|---|
| pre-change (strictly serial) | **60.26s** | 1 |
| this change (auto-planned) | **28.27s** | 20 |

**2.13x**, measured while a second, unrelated, real `showreel` render was actively
decoding a large source on the same 20-core box — the achievable speedup on a quiet
machine is very likely higher; this number is the conservative, contended-machine one,
not a cherry-picked quiet one.

Output correctness, not just speed: both files are 6000 frames / 200.00s / 1280x720 —
identical by `ffprobe`. Decoded pixel content at five points across the film (5s, 50s,
100s, 150s, 195s) differs by **0.0000 to 0.0413 / 255** mean absolute — the lossy-codec
noise floor, not a content difference; `src/segments.rs`'s own test
(`concurrent_and_forced_serial_segment_rendering_produce_the_same_picture`) proves the
same thing at unit-test speed and explains why exact *byte* equality is not the bar —
see the next section.

### A finding worth recording honestly: the naive version was *slower*

The first working version of concurrent segments (no ffmpeg thread capping) was tried
against a 1920x1080/96s film and came out **slower** than serial — 102.08s against a
47.70s baseline, CPU utilisation actually *lower* (240% vs 350%) and involuntary
context switches roughly 2.4x higher. `ffmpeg` defaults to using every core it can see
for both its decoder and its encoder; N concurrent workers each starting such
processes oversubscribes the machine by roughly Nx on top of the raster work `rayon` is
already doing. The fix — capping each worker's own `ffmpeg` children to a fair share of
the cores (`-threads`, both the decode side in `src/assets/clip.rs` via a thread-local
hint, and the encode side in `EncodeOptions.threads`) — is what turned the concurrency
into the 2.13x above rather than a regression. Left as a real finding rather than
smoothed over, the same way `docs/segmented-rendering.md` records its own GOP-boundary
investigation.

## Why encoded bytes aren't guaranteed identical, but frames are

x264's own encoded bitstream is sensitive to its thread count — a documented, expected
property of frame-parallel encoding, not a bug — so a segment encoded by a 4-worker run
is not guaranteed byte-for-byte identical to the same segment encoded serially. This
crate already made and measured exactly this trade for segmenting itself
(`docs/segmented-rendering.md`'s GOP-boundary finding: segmenting changes encoded
bytes, not decoded pixels, and the SSIM/PSNR bar is what's actually load-bearing).
The guarantee that matters is unaffected: `render_segment` always calls the same
deterministic `Renderer::render_range` on the same film and assets regardless of which
worker calls it (`render.rs`'s own
`sequential_render_ranges_over_a_streaming_clip_match_one_continuous_pass` already
proves sub-range rendering is pixel-exact), and every measurement above confirms it
holds through the encoder too, within noise.

## The mobile cut's frame rate

`showreel render`'s mobile cut used to hard-code 30fps — a real, documented delivery
constraint (Telegram's `sendVideo` rejects 60fps outright), but one that silently
destroyed a Game Boy walk cycle's readability on every film rendered above 30fps. It is
now `--mobile-fps <N>`, defaulting to the film's own fps (no motion lost) rather than a
fixed number; `--mobile-fps 30` restores the old, Telegram-safe behaviour explicitly.
Verified directly, not just built: a 48fps film's mobile cut is 48fps by default
(`ffprobe`: `r_frame_rate=48/1`) and exactly 30fps with the flag
(`r_frame_rate=30/1`). `MobileOptions::default()` itself is unchanged (still 30, still
what `studio`/`mcp` use, since neither has a film's own fps to default against the same
way the CLI does) — see `src/encode.rs`.

## Reproducing this

None of the source films above are committed (large, synthetic, disposable — the same
convention `examples/burst_demo.assets.md` follows for its own synthetic clips):

```sh
ffmpeg -f lavfi -i "testsrc2=size=1920x1080:rate=30:duration=96" \
  -c:v libx264 -preset veryfast -crf 20 -pix_fmt yuv420p source.mp4
```

then a film whose one layer is `{"type": "clip", "asset": "source.mp4", "fit": "cover",
"audio": {"muted": true}, "camera": {...}}` at matching width/height/fps. The
CPU-bound comparison film needs no assets at all — a `title`/`gradient`/`counter`
layer stack is enough; see `src/segments.rs`'s own `solid_film` test helper for the
smallest version of the same idea.
