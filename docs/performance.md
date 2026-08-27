# What a frame costs

Every number here was measured on this machine — a 20-core box — with the
build in this repository. Nothing is quoted from a document.

## The camera over a very large still

This was the one genuinely unknown question when ShowReel was designed: can a
camera move smoothly over a source far bigger than the frame without falling
over? The test is a 120-frame pull-back over PixelGB's atlas of all 226 Pokémon
Blue maps — **6832 × 7024, 48.0 megapixels** — rendered to 1920 × 1080.

| approach | ms/frame | 120 frames |
|---|---|---|
| crop and resample from full resolution, one thread | 142.1 | 17.05 s |
| mip pyramid, one thread | 42.2 | 5.06 s |
| **mip pyramid across 20 threads** | **4.3** | **0.52 s** |

**33× faster than the obvious implementation**, and roughly 8× faster than
real time for the shot.

The reason is the pyramid, in [`src/assets/still.rs`](../src/assets/still.rs).
Resampling straight from level 0 costs time proportional to the *source* area
being read — which, for a wide shot, is the entire image, on every frame.
Choosing a pre-reduced level first makes the cost proportional to the *output*
area instead: near-constant, whatever the zoom. The pyramid is built once at
load (523 ms for this source) and costs 192 MB resident.

Level selection keeps the final resample to a minification of at most 2×,
which is the range where a plain bilinear filter is indistinguishable from an
expensive one. When the camera is magnifying rather than reducing — a Game
Boy screen blown up to fill the frame — it switches to nearest-neighbour, so
pixel art stays crisp instead of turning to mush.

## A whole film

The worked example: 35 seconds, five scenes, four transitions, a 48-megapixel
camera move, five video insets, and text overlays throughout.

| | frames | wall | per frame | throughput | peak RSS |
|---|---|---|---|---|---|
| the 48 MP pull-back alone | 660 | 10.0 s | 15.2 ms | 66 fps | 2.09 GB |
| the whole film | 2082 | 25.0 s | 12.0 ms | 83 fps | 2.16 GB |
| a 0.35-scale preview pass | 2082 | 6.6 s | 3.1 ms | 318 fps | — |

Rendering is 1.4× faster than real time at full quality.

**Memory is dominated by decoded video, not by the picture.** 1156 MB of the
peak is clips held as raw frames. `Content::Clip` decodes only the range a
`trim` names; without one, a six-minute source is decoded whole and will
exhaust a machine. The preview pass scales decode widths down with the frame,
which is why it holds 365 MB rather than 1156 MB.

## Determinism

Frame *n* is a pure function of the description: nothing reads the clock,
draws a random number, or mutates shared state. Verified end to end rather
than asserted —

```
$ showreel render kanto.film.jsonc --frames 600-609 --png det1 -o det1.mp4
$ showreel render kanto.film.jsonc --frames 600-609 --png det2 -o det2.mp4
$ diff -r det1 det2 && md5sum det1.mp4 det2.mp4
```

— gives byte-identical PNG frames *and* byte-identical mp4s. That is what
makes it safe to render frames out of order across twenty threads, and the
test suite checks the parallel and serial paths agree frame for frame.

## Looking before rendering

| | answers | on the example film |
|---|---|---|
| `showreel still --at 4.2s` | what does this moment look like? | ~100 ms |
| `showreel sheet --every 1.5s` | is the pacing right? | 3 s for all 35 s |
| `showreel preview --scale 0.35` | does the motion work? | 6.6 s |
| `showreel render` | the delivery | 25 s |

This is not a nicety. Three real defects in this repository — overlay text
ignoring the film's theme, lower-thirds landing at the origin instead of
bottom-left, and a caption running off the frame — were each found by looking
at a contact sheet, before any video was encoded.
