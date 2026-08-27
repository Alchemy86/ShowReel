# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

ShowReel is a **general** toolset for demo, explainer and animation films: a library plus
the `showreel` CLI. `README.md` covers usage; `src/lib.rs` covers the mental model.

## The rule that shapes everything

**Nothing in the crate may know what its films are about.** Maps, screen captures, game
sprites and swarm runs are *inputs*. If a `if pokemon`-shaped branch appears in `src/`, it
belongs in an example instead — see `examples/kanto_reel.rs`, which does the whole
subject-specific job (map rectangles, clip timestamps) through the public API only.

## Where the load-bearing decisions live, and why

Each is documented at the top of its module; read the module rather than duplicating here.

| Decision | Module |
|---|---|
| Seconds are the authoring unit, resolved to exact frames once | `src/time.rs` |
| A camera over a huge still is mip-backed; zoom interpolates *geometrically* | `src/camera.rs`, `src/assets/still.rs` |
| Glyphs are filled paths, not font-engine blits (so text takes gradients, strokes, shadows) | `src/text/font.rs` |
| Transitions split presentation from timing; the "no leading transition" rule is in the *type* | `src/transition.rs`, `src/timeline.rs` |
| ffmpeg is invoked directly rather than reusing `agentgb`'s Python `video.py` | `src/encode.rs` |
| Assets are resolved through `AssetStore`, the seam for MCP/fetching later | `src/assets/mod.rs` |

## Sharp edges

- **Nothing may call `Theme::default()` while drawing.** Use `RenderCtx::theme`, which is
  the *film's* theme. Reaching for the crate default silently ignores both a custom theme
  and `scale::scale_film`, so every preview thumbnail draws 1080p type into a 320px frame.
  `render.rs` has the regression test.
- **A `Layer`'s `placement` is `Option`.** `None` means "wherever this content belongs"
  (`Content::default_placement`) — a lower-third goes bottom-left, a counter top-right. A
  JSON layer with no placement must land where the Rust builder would put it.
- **`Content::Clip` without `trim` decodes the whole file into memory.** A six-minute
  source at 480px is gigabytes. Always trim to the moment used.
- **`serde(flatten)` + an internally-tagged enum cannot take a default.** That is why
  `Placement` is untagged with a string shorthand (`"centre"`) rather than flattened.
- Counters nest under `"count"` rather than flattening: a counter's `from` is a value and a
  layer's `from` is a time.

- **Any text drawn into a plate must be measured against the room that actually exists**,
  then the plate clamped into the frame. `TextLayout::fit_width` wraps first and shrinks
  only as a fallback, stopping at a legibility floor — so its width guarantee is
  best-effort and the absolute "never off-frame" guarantee is the caller's clamp. A
  caption once ran off the left edge because the plate grew away from its target without
  ever consulting the frame; `layer.rs` has the edge tests.
- **`Rect::to_aspect` grows, `Rect::inscribed_aspect` crops.** `Fit::Cover` needs the
  second. Using the first letterboxes a square source into a wide frame — the exact
  opposite of covering it.

## Working on it

- `cargo test` — unit tests live beside their modules; `tests/render_pipeline.rs` renders a
  film using every layer kind and checks determinism and the JSON round trip. It uses a
  tiny frame on purpose: it is what catches clamp arithmetic that only holds at 1080p.
- **Rebuild with `cargo build --release --bins --examples`.** A bare `cargo build
  --release` does not rebuild examples, and `--examples` does not rebuild the `showreel`
  binary. Rendering with a stale half of the pair produces output that contradicts the
  source and wastes a debugging cycle — this has happened twice.
- `./reel` renders the self-contained tour from a clean clone; it needs no assets.
- **Use the preview path rather than rendering to judge anything**: `showreel sheet` puts
  the whole film on one page in a couple of seconds, and `showreel still --at <t>` is
  milliseconds. Both bugs in the sharp-edges list above were caught by the contact sheet
  before a single second of video was encoded.
- `ffmpeg` and `ffprobe` must be on `PATH`. Fonts come from the system; `showreel fonts`
  lists what is visible. The default theme wants Montserrat and Open Sans and degrades to
  whatever sans exists.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
