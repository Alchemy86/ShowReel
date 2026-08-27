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
| Audio hangs off the *film*, not a scene; `Audio` describes, `AudioInput` is resolved | `src/audio.rs` |
| A clip's own soundtrack (`ClipAudio`, on `Content::Clip`) is a level, not a placement — its `at`/`from`/`duration` are the clip's own timing, so `clip_track` builds its `AudioInput` by delegating to `Audio::resolve` rather than re-deriving fade clamping | `src/audio.rs`, `src/layer.rs` (`Layer::clip_audio_track`), `src/timeline.rs` (`Film::clip_audio`) |
| Film files accept a narrow JSONC subset (comments, trailing commas) — deliberately not full JSON5 | `src/timeline.rs` |
| The browser studio polls (the film file's mtime, and `/api/state`) rather than holding a socket open — one `tiny_http` worker thread per held connection is the cost a blocking server can't hide | `src/studio.rs` |
| The renderer also compiles to `wasm32-unknown-unknown` (no wasm-bindgen — plain `extern "C"` over linear memory, `projects/asciicity`'s pattern) so a film can be scrubbed in someone else's browser with no server. `rayon` and `clap` are optional (`parallel`/`cli` features) so the wasm build pulls in neither; ffmpeg has no browser story, so a clip's frames are pre-decoded natively by `showreel web-pack` and shipped as a `.srclip` JPEG sequence | `src/wasm.rs`, `src/webclip.rs`, `build-wasm.sh` |
| The browser page (`tools/web/`) is a real editor, not just a scrubber: `editor.js` mutates a film's JSON tree directly (it *is* the wire format — see the sharp edge below) and `main.js` reloads it through the same `sr_load_film`/`sr_add_*` wasm calls the boot sequence uses. Adding a clip from the browser needs no ffmpeg either: `clipimport.js` decodes it via a seeked `<video>` element (the platform decoder, reached through the one API surface that already demuxes for you) and `srclip.js` packs the frames into the exact `.srclip` container `sr_add_clip` already reads — no new wasm surface. Exporting a video uses real WebCodecs (`VideoEncoder`, VP8) plus a hand-rolled, ffprobe-verified WebM muxer (`muxer.js`/`test-muxer.mjs`), since no browser ships a demuxer *or* a muxer | `tools/web/editor.js`, `tools/web/main.js`, `tools/web/clipimport.js`, `tools/web/srclip.js`, `tools/web/export.js`, `tools/web/muxer.js` |
| The editor is organised around what a person is doing (trim, add an effect, move/change text), not the data model: the inspector shows named presets and drag widgets first and folds the full field vocabulary under "Advanced"/"Exact position"/"Layer JSON" — the reach is never removed, only deferred. Direct manipulation (drag a clip's trim handles, drag a layer on the preview, drag a callout's ring and label) is real dragging against `geometry.js`'s from-scratch JS mirror of `Placement::resolve` and `CalloutSpec`'s target/label_at, not a data-model change — the wasm renderer and wire format are untouched | `tools/web/geometry.js`, `tools/web/editor.js`, `tools/web/index.html`'s `#stage-overlay` |

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
- **The mobile cut must be checked for audio, not assumed.** It is a *second*
  ffmpeg invocation over the finished master, and it carried `-an` for as long
  as the crate was silent. A film can play perfectly and arrive on the phone
  mute; `tests/render_pipeline.rs` ffprobes both outputs and asserts a level,
  because "has an audio stream" and "is audible" are different claims.
- **`afade` with `d=0` is not a no-op** — it mutes a sample. `AudioInput::filter`
  therefore *omits* a stage rather than passing neutral parameters, and
  `amix` is always given `normalize=0` (its default divides every input by the
  input count, so adding a quiet second track silently halves the first).
- **`Audio::at` is film time, `Audio::from` is source time.** Confusing the two
  is the classic mistake; both are pinned by a test.
- **A clip's own audio does not loop with `ClipLoop::Loop`, and does not hold
  with `ClipLoop::Hold`.** `Layer::clip_audio_track` mixes exactly the source
  window the clip decoded for its on-screen span (clamped to what is left of
  the scene — a layer's declared `duration` cannot pull audio past where it
  is ever drawn); once that runs out the sound simply stops, the same way a
  video frozen on its last frame does not keep making noise. Looping the
  *audio* to match a looping picture would need `aloop` sized in samples,
  which was judged not worth the complexity until a film actually needs it —
  a deliberate gap, not a missed one. Measured on a 3s synthesised clip:
  default gain reached the master at −24.1 dB mean, `.clip_gain(0.3)` at
  −34.6 dB (≈ 20·log₁₀(0.3) quieter, as it should be), `.mute()` produced no
  audio stream at all. See the "A clip's own audio" section of `README.md`.
- **A clip's camera is not mip-backed the way a still's is.** `Content::Clip.camera`
  reuses `Camera`'s framing maths (`src/camera.rs`'s `Canvas::draw_pixmap_cropped`),
  but a clip frame is decoded once at `max_width` and a tight framing just
  magnifies that — there is no pyramid to pick a sharper level from. Push a
  camera in close on a clip and raise `max_width` to match, or the footage
  goes soft.
- **`showreel studio` needs `cargo build --features studio`** — the plain
  binary does not have the subcommand at all, on purpose (`src/studio.rs`).
- **The browser build cannot decode video, at all** — `Clip::load` shells out
  to ffmpeg, and there is neither ffmpeg nor a filesystem in a wasm32 browser
  sandbox. `showreel web-pack` (native, `--features wasm`) runs the ordinary
  ffmpeg-backed decode ahead of time and packs the frames as a `.srclip`
  container the wasm build unpacks with the `image` crate it already has for
  stills. If you are chasing "why is this clip blank in the browser," the
  clip was never pre-decoded, not a wasm-side rendering bug.
- **A `.srclip`'s filename is not just the asset name.** `AssetStore::clip`'s
  cache key is `(reference, fps, max_width, trim)`, and a film can use the
  same source file at several different trims (`examples/kanto.film.jsonc`
  does, six times, over `pixel-chain-run.mp4`). `clip_srclip_name` in
  `src/bin/showreel.rs` and `clipAssetPath` in `tools/web/bridge.js` both
  fold those same four fields into the filename for exactly this reason —
  naming it just `{asset}.srclip` was tried first and silently served every
  trim the same, wrong, frames. The two functions must stay in lock step.
- **A `.srclip`'s all-intraframe JPEG sequence is the reason a packaged
  film's clips dominate its download size** (one real packaging run came out
  51.6 MB, nearly all of it six trims of one source clip). Measured, not
  guessed: the same 6s/640×360/30fps source packed as `.srclip` at the
  default quality is **3.4 MB**; ffmpeg re-encoding the identical window to
  ordinary h264 (`crf 23`) is **48 KB** and to VP8 (`crf 30`) is **147 KB** —
  a 23-70× gap, because JPEG has no motion compensation and re-compresses
  every frame from nothing. The immediate lever needs no code change:
  `showreel web-pack --scale`/`--clip-quality` (`src/bin/showreel.rs`) trade
  this down directly — a `--scale 0.5 --clip-quality 60` pass on the same
  test clip nearly halved it (3.4 MB → 2.7 MB) with no format change. The
  bigger fix this points at, not yet built: ship the clip as ffmpeg's own
  small compressed video (still produced natively, still no browser ffmpeg)
  and decode it client-side the same way `clipimport.js` already decodes a
  captain's dropped file — a seeked `<video>` element, no demuxer needed —
  paying a one-time browser-side decode cost per page load in exchange for
  the ~20-60× smaller download this measurement shows is on the table.
- **`Theme::DISPLAY`/`BODY`'s families are found by scanning system font
  directories (`FontDb::scan_system`), which a browser has none of.** The
  wasm build registers its own bytes via `FontDb::add_bytes` instead, from
  `tools/web/fonts/` — a fixed, small subset of Montserrat/Open Sans weights,
  not every weight the native binary might find installed.
- **The browser editor's film object *is* the wire JSON, always** —
  `tools/web/editor.js` never holds an internal-only shape it converts before
  handing the film to `sr_load_film`. A `Layer`'s `placement` field there is
  exactly `Placement`'s untagged wire form, a `Transition`'s `presentation`
  exactly `Presentation`'s tagged form, and so on. Breaking that (adding a
  JS-only convenience field) means either duplicating a conversion step the
  crate doesn't otherwise need, or a per-layer "advanced JSON" editor showing
  something that isn't actually what gets rendered.
- **`geometry.js`'s on-canvas selection box is exact for some content, a
  guess for others — know which before trusting it.** `Placement::Full`,
  `::Rect` and `::Frac` resolve from the frame size alone (`Placement::resolve`,
  src/layer.rs), so those — and a bare `Content::Text`, whose "natural size"
  is a fixed `frame.w*0.8 x frame.h*0.3` regardless of what the text says —
  are reproduced exactly with zero font engine. `Title`/`LowerThird`/`Counter`
  anchor against their *measured* text plate instead (real font metrics,
  Rust-only), so their default (unset) placement box is a stand-in sized to
  look about right — good enough to click and to start a drag, and the
  moment a drag commits it becomes an explicit `Frac`, which is exact from
  then on. A callout's `target`/`label_at` are already plain `[fx, fy]`
  fractions (`CalloutSpec`, src/layer.rs) with no font dependency at all —
  always exact, the cleanest case in the module. If a selection outline looks
  slightly off around a freshly-added Title, that's this approximation, not a
  renderer bug; drag it once and it locks to a precise box.
- **Direct manipulation covers text-ish layers and callouts, not yet a
  still/clip's camera framing.** A `Camera` (src/camera.rs) is keyframed pan
  and geometric zoom over a mip pyramid, not a single point or box — dragging
  it on the preview is the same "point at where you want it" idea the
  position/callout work above proves out, but it needs its own interaction
  (drag to reframe *and* scrub a timeline of keyframes) rather than reusing
  `geometry.js`'s single-rect drag. Left for a follow-up; today a camera move
  is still Advanced-JSON-only.
- **`Content::Text.style` is a bare `TextStyle`, not `Option<TextStyle>`** —
  every other content kind's `style` is optional (`None` = derive from the
  theme). Sending `"style": null` for a text layer fails with `invalid type:
  null, expected struct TextStyle`, not a helpful "missing field" error. A
  browser-added text layer omits the key entirely rather than nulling it;
  `editor.js`'s `newLayer('text')` has the full explanation.
- **A dropped clip decodes in the browser at roughly 0.5-1 *second* a frame**,
  measured in a headless, GPU-less test environment — seeking a `<video>`
  element and drawing its current frame is one full round trip per frame (see
  `clipimport.js`'s doc comment for why there's no faster route without a
  hand-rolled demuxer). Decoding a 3s clip at a 60fps film's own rate would
  take minutes; `main.js`'s "+ Clip" handler defaults a browser-added clip's
  `decode_fps` to `min(film fps, 12)` for exactly this reason — raise it via
  the layer's "Decode fps override" field only once you know the cost.
- **Browser playback used to render every frame at the loaded scale (always
  1.0)** — no different from a paused frame. `sr_load_film`'s `scale` sets the
  one *registered* preview, including what `max_width` every clip layer's
  decode is registered under (`AssetStore::clip`'s cache key — see the
  `.srclip` filename sharp edge above), so continuous playback can't just
  re-`scale_film` a cheaper copy: a clip layer's `max_width` would shrink
  along with everything else, and the resulting lookup would miss the one
  `max_width` actually registered — a miss native code answers by calling
  `Clip::load` (ffmpeg), which does not exist in wasm. `sr_set_draft_scale`
  (`src/wasm.rs`) scales a *further* copy of the registered preview for
  playback only, via `draft_film`, which restores every clip layer's
  `max_width` back to the registered value afterward — `scale_film` itself
  must never be handed a clip layer whose registered decode you want kept.
  `tools/web/main.js`'s `PLAYBACK_DRAFT_SCALE` engages this only while
  `playing`; a scrub or a pause always renders the registered preview at full
  quality. Measured on `examples/kanto.film.jsonc` (1920x1080/60fps) on one
  dev machine, via `wasm.sr_render_at` in a tight loop (bypassing rAF, see
  below): full quality was ~0.9-1.1 raw fps; `src/preview.rs`'s own
  quarter-size number (0.25) only reached ~12.5fps; 0.125 was needed to clear
  24-30fps (~26fps, 40-sample average) — quarter-size is the right call for a
  *contact sheet*, not necessarily for interactive playback.
- **`requestAnimationFrame` under `chrome-devtools-axi`'s headless Chrome is
  throttled to roughly 1Hz**, independent of how fast a frame actually
  renders — confirmed by counting bare `requestAnimationFrame` ticks with no
  ShowReel code involved at all. Timing the editor's real playback loop
  (`main.js`'s `tick()`) through this harness reports that same ~1fps
  regardless of true render cost, which will misdiagnose a fast renderer as
  still slow. Benchmark real per-frame cost directly instead: call
  `wasm.sr_render_at`/paint back-to-back in a tight loop via `eval`, bypassing
  rAF entirely.
- **No audio in the browser at all yet** — the wasm renderer only ever
  produces RGBA pixels; nothing decodes or mixes a film's `Audio` tracks (that
  is `AudioInput::filter`, native-only, ffmpeg's `amix`). The editor still
  lets you add/edit audio tracks (their fields round-trip to the JSON
  correctly), and `export.js`'s WebM output is video-only — both honestly
  documented gaps, not silent ones.

- **`examples/kanto.film.jsonc` is committed and generated.** `kanto_reel.rs` is
  canonical — regenerate with `cargo run --release --example kanto_reel -- -o
  examples/kanto.film.jsonc`, and `--check` on the same command fails if they
  have drifted. Every asset reference in it must stay a bare name resolved by
  `-A/--assets`; a test rejects absolute paths, because a committed film with a
  machine-specific path is useless to everyone else. It is `.jsonc`, not
  `.json`, because it carries hand-written comments — `to_json` never emits
  them back, so `--check` compares *parsed* films rather than raw text and a
  comment cannot trip the drift guard (nor can it catch one gone stale; see
  `src/timeline.rs`).

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
  the whole film on one page in a couple of seconds, `showreel still --at <t>` is
  milliseconds, and `showreel studio` (needs `--features studio`) is the live, scrubbable
  version of the same thing in a browser. Both bugs in the sharp-edges list above were
  caught by the contact sheet before a single second of video was encoded.
- `ffmpeg` and `ffprobe` must be on `PATH`. Fonts come from the system; `showreel fonts`
  lists what is visible. The default theme wants Montserrat and Open Sans and degrades to
  whatever sans exists.
- **Browser build**: `./build-wasm.sh` needs `rustup target add wasm32-unknown-unknown`
  once, then writes `tools/web/showreel.wasm`. `showreel web-pack <film> -o dist` (needs
  `--features wasm`, and ffmpeg to pre-decode any clips) turns that plus a film into a
  self-contained directory any static file host can serve — open its `index.html` and
  scrub, edit, add clips/stills, and export. `cargo test --features wasm` and `cargo
  clippy --target wasm32-unknown-unknown --no-default-features --features wasm --lib`
  both need to stay clean; see `src/wasm.rs` for what the browser cannot do (encode,
  decode arbitrary video without a `<video>` element, read a filesystem). To iterate on
  the editor itself without re-running `web-pack` each time, serve `tools/web/` directly
  (e.g. `python3 -m http.server`) with a `film.json` (and any `assets/`) dropped next to
  `index.html` — every JS file there is a plain ES module, no build step. `node
  tools/web/test-muxer.mjs` checks the hand-rolled WebM muxer (`tools/web/muxer.js`)
  against real `ffprobe`/`ffmpeg` decode; run it after touching that file.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
