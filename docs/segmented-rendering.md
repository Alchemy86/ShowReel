# Resumable rendering

The other half of the 2026-08-29 OOM fix (`docs/clip-streaming.md` is the
memory half): a long render killed at 90% used to lose everything. `showreel
render`'s default whole-film path now renders in fixed-size segments, each
its own short-lived `ffmpeg` process, tracked in a small on-disk manifest —
see `src/segments.rs`'s module doc for the design (why `.ts` segments, why
audio is mixed in exactly once at the end rather than per segment, how the
manifest invalidates).

This only covers the default path: `showreel render <film>` with no
`--frames` sub-range and no `--png` dump. Those are debugging/inspection
tools, not the "long render that dies at 90%" case this exists for, so they
keep the old single-pass behaviour.

## Proof it actually resumes

A real render, killed with `SIGKILL` partway through:

```
$ showreel render film.jsonc -A assets -o out.mp4 --no-mobile &
  segments 4 of ~120 frames (8s) each, none done yet
  segment 1/4 rendered (frames 0..120)
  segment 2/4 rendered (frames 120..240)
  segment 3/4 rendered (frames 240..360)
[killed here, mid-segment-4]

$ showreel render film.jsonc -A assets -o out.mp4 --no-mobile
  resume  3/4 segments already rendered (unchanged film/assets/settings); redoing 1
  segment 4/4 rendered (frames 360..450)
  450 frames at 640x360 in 1.57s — 3.5 ms/frame, 286.3 fps, 41 MB of assets
  master  out.mp4
```

3 of 4 segments survived the kill (their `.ts` files and the manifest
checkpoint after each). The rerun redid only the missing one and produced a
complete, correctly-timed 30.0s output (verified via `ffprobe`).

## Proof the output is right

**Invalidation is real, not decorative** — `render_segmented`'s own tests
(`src/segments.rs`) prove a killed-and-resumed render matches a clean one,
and that changing the film's content changes the fingerprint (so it can't
silently resume onto stale content).

**Composability is the load-bearing invariant, and it's proven at the pixel
level, not just by trusting the design.** Splitting one continuous render
into several sequential `render_range` calls over the same shared,
*streaming* `Clip` (see `docs/clip-streaming.md`) only works if that produces
byte-identical raw frames to one continuous pass — this is genuinely a new
access pattern the streaming design had not been exercised against before
this feature. `Renderer`'s own test suite
(`sequential_render_ranges_over_a_streaming_clip_match_one_continuous_pass`,
`src/render.rs`) proves it directly, comparing raw RGB24 frames (no encoder
involved) across several unaligned sub-ranges against one continuous pass —
**0 differences**.

**A real finding worth recording honestly**: an early check compared full
`showreel render` outputs (segmented vs. a forced single-pass run via
`--frames 0-449`) by extracting frames from each finished `.mp4` and diffing
them — and found ~84% of frames differed, including one where an on-screen
counter in the source footage read a visibly different number (438 vs 436).
That looked like a real bug. It wasn't: re-run at the render layer only (no
video encode at all, the composability test above), the two paths produced
**zero** differing frames. The discrepancy was entirely downstream, in
`ffmpeg`'s H.264 encode — segmenting introduces GOP boundaries a continuous
encode wouldn't have, and at the aggressive `--crf 30` used to keep that
first test's files small, those boundaries measurably hurt fine detail
(SSIM 0.977, PSNR ~15dB, low enough that the on-screen counter's digits
became genuinely misleading). At the real default (`--crf 17`, `slow`
preset), the same comparison gives **SSIM 0.997, PSNR 46.6dB average** — the
counter reads identically, and the two outputs are visually indistinguishable
side by side. This is the expected, honestly-quantified cost of segmenting a
lossy codec (independent per-segment rate control, no B-frame lookahead
across the seam) at realistic quality settings, not a correctness defect —
and it would shrink further on a real multi-scene film, where segment
boundaries would often land near an actual scene cut rather than mid-motion
the way this single continuous test scene forced them to.

## What this doesn't cover

- The mobile cut (`mobile_cut`) is a second, already-fast pass over the
  finished master and is not itself segmented/resumable — losing it to a
  kill just means re-running that one cheap pass, a reasonable scope cut
  given the frame-render is what actually takes long and holds memory.
- Segment boundaries are fixed 8-second wall-clock chunks, not aligned to
  scene cuts. Aligning them would reduce the GOP-boundary cost noted above
  further; not built this round.
- A partial `--frames` render and a `--png` dump keep the old, unsegmented
  behaviour — a deliberate scope cut (see above), not an oversight.
