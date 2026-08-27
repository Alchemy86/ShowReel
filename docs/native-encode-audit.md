# Does ShowReel need ffmpeg at all?

The captain asked this while the browser editor (`tools/web/`) was being built,
because that work forced the question of what ffmpeg is actually *for*. It
does four different jobs, and they have four different answers. Every number
below was measured on this machine, not quoted from a benchmark someone else
ran — see each section for exactly what was run.

## The four jobs, one line each

| Job | Where it lives today | Verdict |
|---|---|---|
| **Encode** (frames → compressed video) | `src/encode.rs`, `libx264` | Keep ffmpeg. Measured: no mature pure-Rust encoder is competitive on speed. |
| **Mux** (compressed frames → a playable container) | `src/encode.rs`'s ffmpeg invocation | Replaceable today, cheaply. Already done once, by hand, for the browser (`tools/web/muxer.js`). |
| **Decode** (an arbitrary source clip → frames) | `src/assets/clip.rs`'s `Clip::load` | Keep ffmpeg. It runs once per source clip, not per frame — see below for why that changes the calculus. |
| **Audio** (mix tracks, encode to AAC) | `src/audio.rs`, ffmpeg's `amix`/AAC | Mixing is easy to take back; encoding to a lossy codec at a mature quality bar is not, and isn't worth taking back for what this crate needs. |

## Encode: measured, not guessed

`rav1e` (Mozilla/Xiph's pure-Rust AV1 encoder, one of AOMedia's three reference
implementations) is the serious pure-Rust candidate. It was benchmarked
against this crate's own real settings, on real frames from
[`examples/showcase.rs`](../examples/showcase.rs) (1920×1080, real camera
moves, text and gradients — not synthetic noise).

**Setup**: `showreel render --frames 0-119 --png` produced 120 real frames.
`ffmpeg`/`rav1e` both encoded those *same* frames. `rav1e` was built
`--no-default-features` with the `asm` feature off — this sandbox has no
`nasm` — so **these numbers understate rav1e's real-world speed**; a
production build with its hand-written SIMD kernels would be faster, plausibly
by 2-5×. The comparison is still useful because even that headroom doesn't
close the gap below.

| Encoder | Settings | Frames | Wall time | ms/frame | Output |
|---|---|---|---|---|---|
| **ffmpeg/libx264** (this crate's actual default: `crf 17`, `preset slow`) | multi-threaded (~3.15 cores) | 120 | 1.45 s | 12.1 | 96.1 KB |
| **rav1e**, speed 10 (fastest) | no asm, quantizer 100 | 120 | 43.0 s | 358 | 74.1 KB |
| **rav1e**, speed 8 | no asm, quantizer 100 | 30 | 24.0 s | 800 | 27.5 KB |
| **rav1e**, speed 6 | no asm, quantizer 100 | 10 | 9.9 s | 993 | 4.8 KB |

Even at rav1e's *fastest, lowest-quality* preset — the one nobody would
actually ship — it is **~30× slower** than this crate's real x264 settings on
identical frames. Extrapolated to the README's own benchmark film (2082
frames), that is the difference between a ~25 s final encode and a ~12-minute
one. ShowReel's whole pitch is fast iteration (`docs/performance.md`); a 30×
regression on the one command that produces the actual deliverable would
change what kind of tool this is, not just how it's implemented.

Independent research corroborates the shape of this, not just the number:
production AV1 encoding in 2026 runs overwhelmingly on **SVT-AV1** (Intel/
Netflix, ~95% of practical AV1 production), not rav1e — rav1e's own niche is
memory-constrained/embedded targets where safety matters more than throughput,
not the encode-speed-critical path this crate is on.
[Encoder implementations survey](https://www.forasoft.com/learn/video-encoding/articles/encoder-implementations-x264-x265-svt-av1-libaom-vvenc) ·
[rav1e on GitHub](https://github.com/rust-av/rav1e)

**Is there a mature pure-Rust H.264 encoder instead?** One real candidate
exists — `rusty_h264`, a from-scratch encoder/decoder claiming bit-exact
parity with ffmpeg. Checked directly against crates.io, not assumed: **created
2026-06-27** (two months old at the time of writing), version **0.12.0**
(pre-1.0), **16,094** downloads. That is a genuine, serious-looking effort,
not vapourware — but it has no track record, and "a young crate claims parity"
is not the same claim as "ffmpeg has had a decade of fuzzing and every codec
edge case thrown at it." Worth watching, not worth betting a shipping feature
on yet.

**Would a naive Motion-JPEG-in-a-container output be acceptable instead?**
This crate already has a fast JPEG encoder in the dependency tree (`image`,
used by `src/webclip.rs` for exactly this — see that module), so an MJPEG path
would cost almost no new *codec* code, only a muxer. It is not a real answer
for delivery, though: MJPEG is all-intraframe, so it does not exploit the
temporal redundancy that is most of what a real film compresses away. That gap
is measured directly in the browser-size investigation below — the same
6-second clip is **~70× larger** as a JPEG sequence than as a real h264/VP8
encode. A film's final master output is exactly the case where that 70×
matters (it is the thing that gets shipped), which is why `showreel render`
does not use this approach even though the crate already has every piece it
would need to.

## Decode: the "once, not per-frame" argument, tested

The working hypothesis going in was that ffmpeg's decode might not need
replacing *at all* in practice, because ShowReel generates almost all of its
own frames — camera moves over stills, drawn text and shapes — and the only
place it decodes existing video is pulling in a captain's own source clips
(`Clip::load`, `src/assets/clip.rs`), which already happens **exactly once per
source clip**, ahead of time, converting straight to `Clip`'s owned RGBA frame
representation. `src/webclip.rs`'s `.srclip` container (built for the browser)
is this same "decode once on the way in" idea, already shipped.

That reasoning was checked against the actual code, not just asserted: grepping
every call site of `Clip::load`/`AssetStore::clip` confirms decode happens at
asset-resolution time, cached by `(reference, fps, max_width, trim)` — a
render never re-decodes a source clip per frame, per scene, or per render
pass. **The hypothesis holds.** Decode is a one-time ingest cost with far
looser speed requirements than encode (a captain trims and imports a clip once
per edit session, not once per frame of the finished film), which is exactly
why the browser could get away with a *slow* decode route
(`tools/web/clipimport.js`'s seeked-`<video>` loop, ~0.5-1 s/frame measured —
see `AGENTS.md`) rather than needing a fast one.

What this does **not** mean is "safe to drop ffmpeg for decode." Pure-Rust
video decode is not a mature, general-purpose story in 2026: the most current
result found is `oxideav-vp9`, whose own crates.io description reads *"orphan-
rebuild scaffold pending clean-room re-implementation"* — version 0.0.12,
1,232 downloads, created five months ago. That is not "immature," it is
explicitly unfinished. Nothing comparable to `symphonia` (below) exists yet
for video containers/codecs. Decode stays ffmpeg's job; the one-time-cost
argument just means that job's speed doesn't matter as much as encode's does.

## Audio: split the same way, and it splits the same way

Mixing already-decoded PCM (gain, fades, multiple tracks) is genuinely easy —
`src/audio.rs`'s fade/gain math is a handful of float operations per sample,
already written and tested in this crate. **Decoding** an arbitrary source
audio file in pure Rust is a solved, mature problem:
[`symphonia`](https://crates.io/crates/symphonia) (checked directly:
**11.3 million** total downloads, actively maintained) handles this
comfortably. **Encoding** to a lossy codec at a quality bar every phone and
browser accepts (AAC, what `EncodeOptions::audio_codec` uses) is the harder
half again, for the same reason video encode is: a mature encoder is a
multi-year optimisation project, and reference-quality output at a competitive
bitrate is not a weekend's work. ffmpeg is one command; nothing pure-Rust
matches it here either.

## If we built the missing pieces ourselves — a scoping estimate, not a guess

The captain's follow-up: if nothing existing is a clear winner, how big is
each piece to build?

- **Muxing — small, already proven.** This is not a hypothetical: **it was
  already built**, by hand, for the browser export path this same task added
  — `tools/web/muxer.js`, a complete WebM/EBML muxer (EBML header, Segment,
  Tracks, Cluster/SimpleBlock), verified against real `ffprobe`/`ffmpeg`
  decode (`tools/web/test-muxer.mjs`: encodes a real VP8 stream, re-muxes it,
  and round-trips it through ffmpeg's own decoder). That took a few hundred
  lines and one sitting, in JavaScript with no library support at all beyond
  what's already in this repo. A Rust mp4 muxer for "one video track, one
  audio track, already-encoded frames in" — the exact case ShowReel needs —
  is the same size of job, and several maintained pure-Rust crates already
  exist for it (`mp4e`, `muxide`, the `webm` crate) if writing one from
  scratch isn't even wanted. **Days, not weeks; genuinely low risk.**
- **Encoding — a different kind of job entirely.** A *competent* encoder
  (compresses reasonably, doesn't crash, ships) is a real but bounded project.
  A *competitive* one (rivals x264's rate-distortion decisions, built on
  decades of psychovisual tuning) is not something a from-scratch effort
  reaches in any timeframe this task's scope covers — `rusty_h264` is the
  live proof: a serious team's fresh attempt, two months old, not yet a
  contender. **Months to a year-plus for "competent"; "competitive" is a
  standing research project, not a milestone.**
- **Decode of arbitrary input — the hard one, and the one already sidestepped.**
  As above: nothing pure-Rust is close for general video decode
  (`oxideav-vp9`'s own description is "scaffold pending re-implementation").
  But ShowReel's actual exposure to this is small and one-time per source
  clip, not per frame — which is why the pragmatic answer is "keep using
  ffmpeg for the one-time ingest step" rather than "build a decoder," full
  stop. **Not worth building for this crate's use case, regardless of size.**
- **Audio — mixing is a rounding error to build (already built); encoding
  is the same order of difficulty as video encoding, for the same reason.**

## Recommendation

**Keep ffmpeg for encode and decode. The muxing question is worth revisiting
independently of encode/decode** — it's the one piece here that's both cheap
to replace and already has a working, verified proof of concept, so a future
pass could reasonably ask "does removing ffmpeg's muxing step buy us
anything concrete" (a smaller binary? one fewer subprocess?) rather than
whether it's *possible* — that part is already answered: yes, cheaply. There
is no version of this — measured, not assumed — where dropping ffmpeg for
encode or decode is a win for a tool whose whole value proposition is fast,
deterministic iteration.
