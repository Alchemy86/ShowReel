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
| `Presentation::CrossBlur` blurs every RGBA channel (`canvas::blur_rgba`), not just alpha (`canvas::blur_alpha`, for shadows) — both share the same three-pass box-blur core, `canvas::box_blur3` | `src/canvas.rs`, `src/transition.rs` |
| `Content::Bar`'s `BarSpec` (`from`/`to`/`over`/`easing`) deliberately mirrors `CounterSpec` rather than reusing it — a bar has no digits, grouping or prefix/suffix, and nests under `"progress"` for the same `from`-collides-with-`Layer::from` reason `CounterSpec` nests under `"count"` | `src/layer.rs` |
| `Content::Parallax` (the fake-depth "screenshot" shot, `docs/anarchist-study.md`) is a composition convenience, not new render machinery: one authored `Camera` move, and each plane's own viewport is `Camera::viewport_at`'s *result* blended toward its resting framing by `ParallaxPlane::depth` — position linearly, height geometrically, the same reasoning `Camera::viewport_at` itself uses for zoom | `src/layer.rs` (`draw_parallax`, `parallax_viewport`) |
| ffmpeg is invoked directly rather than reusing `agentgb`'s Python `video.py` | `src/encode.rs` |
| Assets are resolved through `AssetStore`, the seam for MCP/fetching later | `src/assets/mod.rs` |
| Audio hangs off the *film*, not a scene; `Audio` describes, `AudioInput` is resolved | `src/audio.rs` |
| A clip's own soundtrack (`ClipAudio`, on `Content::Clip`) is a level, not a placement — its `at`/`from`/`duration` are the clip's own timing, so `clip_track` builds its `AudioInput` by delegating to `Audio::resolve` rather than re-deriving fade clamping | `src/audio.rs`, `src/layer.rs` (`Layer::clip_audio_track`), `src/timeline.rs` (`Film::clip_audio`) |
| Film files accept a narrow JSONC subset (comments, trailing commas) — deliberately not full JSON5 | `src/timeline.rs` |
| The browser studio polls (the film file's mtime, and `/api/state`) rather than holding a socket open — one `tiny_http` worker thread per held connection is the cost a blocking server can't hide | `src/studio.rs` |
| "API access" means the local HTTP surface, not the (already stable, already documented) Rust library — and it rides the studio's own server rather than a second, stateless one: `/api/render`/`still`/`info`/`check` answer against whatever film the running `showreel studio` instance already has loaded and validated, so there is one code path for "load a film," not two that can drift | `src/studio.rs`, `README.md`'s "API access" |
| The renderer also compiles to `wasm32-unknown-unknown` (no wasm-bindgen — plain `extern "C"` over linear memory, `projects/asciicity`'s pattern) so a film can be scrubbed in someone else's browser with no server. `rayon` and `clap` are optional (`parallel`/`cli` features) so the wasm build pulls in neither; ffmpeg has no browser story, so a clip's frames are pre-decoded natively by `showreel web-pack` and shipped as a `.srclip` JPEG sequence | `src/wasm.rs`, `src/webclip.rs`, `build-wasm.sh` |
| The browser page (`tools/web/`) is a real editor, not just a scrubber: `editor.js` mutates a film's JSON tree directly (it *is* the wire format — see the sharp edge below) and `main.js` reloads it through the same `sr_load_film`/`sr_add_*` wasm calls the boot sequence uses. Adding a clip from the browser needs no ffmpeg either: `clipimport.js` decodes it via a seeked `<video>` element (the platform decoder, reached through the one API surface that already demuxes for you) and `srclip.js` packs the frames into the exact `.srclip` container `sr_add_clip` already reads — no new wasm surface. Exporting a video uses real WebCodecs (`VideoEncoder`, VP8) plus a hand-rolled, ffprobe-verified WebM muxer (`muxer.js`/`test-muxer.mjs`), since no browser ships a demuxer *or* a muxer | `tools/web/editor.js`, `tools/web/main.js`, `tools/web/clipimport.js`, `tools/web/srclip.js`, `tools/web/export.js`, `tools/web/muxer.js` |
| The editor is organised around what a person is doing (trim, add an effect, move/change text), not the data model: the inspector shows named presets and drag widgets first and folds the full field vocabulary under "Advanced"/"Exact position"/"Layer JSON" — the reach is never removed, only deferred. Direct manipulation (drag a clip's trim handles, drag a layer on the preview, drag a callout's ring and label) is real dragging against `geometry.js`'s from-scratch JS mirror of `Placement::resolve` and `CalloutSpec`'s target/label_at, not a data-model change — the wasm renderer and wire format are untouched | `tools/web/geometry.js`, `tools/web/editor.js`, `tools/web/index.html`'s `#stage-overlay` |
| Browser playback has sound via a second, independent Web Audio graph the wasm renderer knows nothing about — see the "Browser playback has sound" sharp edge below for the film-track-vs-clip-audio liveness split | `tools/web/audio.js`, `AudioInput::export_filter` (src/audio.rs), `cmd_web_pack`'s clip-audio extraction (src/bin/showreel.rs) |
| Thumbnails (scene rows, layer rows, the asset list) are recognition over recall the same way YouCut's filmstrip is — generated once per asset and cached, a still via one scaled `createImageBitmap` decode, a clip via a byte-slice out of its `.srclip`'s first frame (no decode at all) | `tools/web/thumbnails.js`, `main.js`'s `ensureThumbnail` |

## Considered, not built

**Rich text runs — bolding or colouring one word inside a `Text`/`Title`/`LowerThird`
string — was investigated and deliberately declined**, rather than half-landed. Today a
layer is one `TextStyle` for its whole string; the investigation found four separate
places that assumption runs through, not one:

- `PositionedGlyph` already carries a `font: FontId` per glyph, so the *data model* is not
  the blocker — a mixed-weight line is already representable.
- `TextLayout::build` shapes and measures an entire line as a single `db.shape`/`db.measure`
  call against one `(font, size, tracking)`. A bold word needs a second `FontId` (a
  different weight, possibly a different file), shaped separately and stitched onto the
  first run's pen position — losing `rustybuzz`'s cross-run kerning at the seam, a real,
  accepted trade-off every such system makes, but worth naming rather than discovering late.
- `wrap()` measures word widths against that one style; wrapping a line with a run boundary
  mid-word needs the wrap algorithm to sum per-run segment widths, not call `measure` once
  per candidate line.
- `text::draw` applies one `style.fill`/`style.stroke` to every glyph in the whole layout —
  the one function every text-bearing `Content` variant funnels through, so even a
  colour-only slice touches code shared by six content kinds, not just `Title`/`LowerThird`.

A colour-only version (no weight/shaping changes, since colour needs no re-shaping) would
have been materially smaller — but the brief that named this named bold and colour as
co-equal examples, and shipping only the easier half is itself a half-landing. Left for a
session that can give the shaping/wrap/draw path the same careful pass the audio pipeline
got before this round of features touched it.

**A global colour grade (lift/gamma/gain, saturation, contrast, vignette) — the other real
gap `docs/anarchist-study.md` found — is deliberately left for its own session, not bundled
into `Content::Parallax`.** It is confirmed absent (nothing in `canvas.rs` tone-maps a
finished frame) and worth building, but the study's own ranking puts it last: a one-stage,
self-contained addition to `render.rs`'s per-frame pipeline that touches no text, camera or
transition code, and — per the study — the smaller lever of the two gaps for how much it
would change what a film can actually look like. Bundling it into this round would have
meant doing it quickly to stay in scope, and a post-composite grade is exactly the kind of
thing (colour is unforgiving, and a vignette that clips wrong is obvious) that deserves its
own measured pass rather than a rider on the parallax work.

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
- **Every `Content::Parallax` plane must be the exact same pixel size as the first.**
  `draw_parallax` checks this at render time and errors with both planes' sizes rather than
  silently misaligning them — the "one authored camera move, blended per plane" math only
  makes sense if every plane shares one coordinate space, the same way real depth-plane
  cutouts of one screenshot share its canvas. A differently-sized plane is an authoring
  mistake to fix (re-export the cutout at the source canvas's size), not a case to support.

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
- **`Content::Clip.speed` picks a scaled *decoded* frame index
  (`local × speed`); it does not touch the clip's own audio.** Same call as
  the loop/hold gap directly above: pitch/tempo-correcting audio for a
  different playback rate needs `ffmpeg`'s `atempo`, which this crate does not
  wire up, so `Layer::clip_audio_track` drops a clip's own soundtrack entirely
  while `speed != 1.0` rather than mix it out of sync with what is on screen.
  A large speed still only ever reaches frames the `trim`/`decode_fps` window
  actually decoded — it does not decode further on its own. See the "A clip's
  own speed" section of `README.md`.
- **A clip's camera is not mip-backed the way a still's is.** `Content::Clip.camera`
  reuses `Camera`'s framing maths (`src/camera.rs`'s `Canvas::draw_pixmap_cropped`),
  but a clip frame is decoded once at `max_width` and a tight framing just
  magnifies that — there is no pyramid to pick a sharper level from. Push a
  camera in close on a clip and raise `max_width` to match, or the footage
  goes soft.
- **`Presentation::CrossBlur` is genuinely expensive, and the cost does not
  fall with a smaller radius.** `canvas::blur_rgba` measured **~170ms a call**
  at 1920×1080 (`canvas::tests::blur_rgba_cost_at_1080p`; an unoptimised debug
  build is several times slower again) — a box blur's cost is the frame's
  pixel count times its fixed 4 channels × 3 passes, not the window size, so
  radius 5 and radius 24 cost the same. `compose` calls it twice a frame (the
  outgoing and incoming sides), so this transition costs on the order of a
  third of a second *per frame*, on top of everything else drawn that frame —
  against the whole example film's own 12ms/frame average
  (`docs/performance.md`). Fine for a transition well under a second; a
  film-length one would not render in a reasonable time. See the "A
  cross-blur dissolve" section of `README.md`.
- **`showreel studio` needs `cargo build --features studio`** — the plain
  binary does not have the subcommand at all, on purpose (`src/studio.rs`).
- **`/api/render` runs ffmpeg and writes a file for whoever can reach the port.**
  `StudioOptions::host` defaults to `127.0.0.1` for exactly this reason; `serve()`
  prints a startup warning naming the risk the moment `--host` is anything else. This
  is not a check to relax later — a render endpoint open to a network is a
  resource-exhaustion (and, depending what else is running, code-execution-adjacent)
  surface, per the brief that added it. Extend the studio server's API further with
  the same default in mind, not just this one endpoint.
- **`showreel new`'s starter film is a hand-written JSONC template
  (`starter_jsonc` in `src/bin/showreel.rs`), not built through the Rust
  builders and re-serialised** — unlike `kanto.film.jsonc`, there is no
  canonical `.rs` source and no drift check, so a schema change (a renamed
  field, a new required one) can silently break it. `the_starter_template_always_parses_and_validates`
  guards this by round-tripping the template through `Film::from_json` +
  `validate()` on every `cargo test`; keep it passing rather than skipping it
  after a schema change. The one field that *is* runtime input — `--title` —
  goes through `serde_json::to_string` before interpolation, not a bare
  `"{title}"`: a title containing a `"` would otherwise corrupt the JSON it is
  spliced into.
- **`[profile.web]` (Cargo.toml) is `opt-level = 3`, not the smaller `"z"`,
  and `build-wasm.sh` compiles it with `RUSTFLAGS="-C target-feature=+simd128"`.**
  Both were tried in isolation before being combined — see the measured
  fps at each combination in the "Browser playback used to render every
  frame..." sharp edge above. `opt-level = "z"` was actively hostile to this
  binary's one real hot loop (pixel compositing, `src/canvas.rs`): it costs
  more than the simd128 flag itself gains, because "z" also skips the
  inlining tiny-skia's own `target_feature = "simd128"` codepaths
  (`tiny-skia`'s `simd` cargo feature, already on by default) need to pay
  off. The wasm binary is about 25% bigger for it (1.8MB -> 2.25MB,
  stripped) — a real cost against the "opens anywhere, no server" download,
  but a >10x compute win for a real-time renderer is the more load-bearing
  trade. `wasm-opt` (binaryen) is wired in as an optional extra pass in
  `build-wasm.sh` (skipped silently if not on `PATH`) but was never itself
  measured in this environment — it was not installed here, and installing
  system packages was out of scope for the session that added this; a
  session with install rights should measure it before assuming the doc
  figure some other project's README gives.
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
  1.0)** — no different from a paused frame, on a canvas that filled most of
  the window: on a 1920-wide screen that is nearly the film's own resolution,
  spent just to show a preview. `sr_load_film`'s `scale` builds the one
  *registered* preview, and a clip layer's decode is registered under a
  `max_width` baked into that same call (`AssetStore::clip`'s cache key — see
  the `.srclip` filename sharp edge above) — so naively asking for a smaller
  preview by re-`scale_film`-ing would shrink a clip's `max_width` right along
  with everything else, and the resulting lookup would miss the one
  `max_width` actually registered — a miss native code answers by calling
  `Clip::load` (ffmpeg), which does not exist in wasm. `scale_keep_clip_decode`
  (`src/wasm.rs`) is `scale_film` with every clip layer's `max_width` put back
  to what the *caller's own base film* already declared it as; both
  `sr_load_film`'s own `scale` and `sr_set_draft_scale`'s further narrowing
  for continuous playback go through it, so **the browser can ask for any
  preview size — to match whatever box the page actually renders it into —
  without ever needing a differently-packed clip asset to match.**
  `tools/web/main.js`'s `PLAYBACK_DRAFT_SCALE` mechanism still exists as a
  fallback for continuous playback narrower than the loaded preview, engaged
  only while `playing`; a scrub or a pause always renders the registered
  preview at full quality. Measured on `examples/kanto.film.jsonc`
  (1920x1080/60fps) on one dev machine, via `wasm.sr_render_at` in a tight
  loop (bypassing rAF, see below) — at the *full* 1920x1080 preview, with
  neither of the two build flags below: ~1.8 raw fps; with only `+simd128`:
  ~3.6fps; with only `opt-level 3`: ~7.6fps; with both: ~20.6fps. At a
  960x540 preview (half width — a quarter of the pixels) with both flags:
  ~67fps, comfortably past this film's own 60fps. `src/preview.rs`'s
  quarter-size contact-sheet number (0.25) reached ~12.5fps under the old
  unoptimized build; draft-scale's old 0.125 floor and its ~26fps number are
  historical baselines from before the build-flag fix below — the actual
  floor a given machine needs should be re-measured against the current
  build, the same way, before being trusted.
- **`requestAnimationFrame` under `chrome-devtools-axi`'s headless Chrome is
  throttled to roughly 1Hz**, independent of how fast a frame actually
  renders — confirmed by counting bare `requestAnimationFrame` ticks with no
  ShowReel code involved at all. Timing the editor's real playback loop
  (`main.js`'s `tick()`) through this harness reports that same ~1fps
  regardless of true render cost, which will misdiagnose a fast renderer as
  still slow. Benchmark real per-frame cost directly instead: call
  `wasm.sr_render_at`/paint back-to-back in a tight loop via `eval`, bypassing
  rAF entirely.
- **Browser playback has sound, via a second, independent mixer — not the
  wasm renderer.** The wasm renderer still only ever produces RGBA pixels;
  nothing there decodes or mixes audio, and never will (`AudioInput::filter`/
  `mix_filter`, src/audio.rs, are ffmpeg-filter-based and have no wasm
  equivalent). `tools/web/audio.js`'s `AudioEngine` is a *separate* Web Audio
  graph driven by the same film-time clock `main.js`'s `tick()`/`renderAt()`
  own: scheduled from scratch on every play/scrub/loop, stopped outright on
  pause. Two different kinds of sound, two different liveness guarantees:
  a film-level `Audio` track (`film.audio`) ships as its own raw source file
  (`showreel web-pack` copies it like a still) and is decoded with
  `decodeAudioData`, with `at`/`from`/duration/gain/fades read live off the
  film object every reload — editing a track in the browser is heard
  immediately. A clip's own baked-in soundtrack is pre-extracted by ffmpeg at
  `web-pack` time (`AudioInput::export_filter`, a no-`adelay` sibling of
  `AudioInput::filter`) into its own small file, listed in `clip-audio.json`
  — a snapshot, the same staleness a `.srclip`'s own decode already has:
  editing a clip's `audio` settings or trim in the browser needs a repackage
  to be heard. `export.js`'s WebM output is still video-only — real audio
  muxing into that export was not attempted this round, an honestly
  documented gap, not a silent one.
- **A browser-dropped clip (no server `web-pack` behind it) has no audio
  preview** — `clip-audio.json` only exists for a real `web-pack` output, for
  the same reason a browser-dropped clip has no `.srclip` either: there is no
  ffmpeg in the browser to extract it from. The picture still imports fine
  (`clipimport.js`); only its sound doesn't follow yet.
- **`timelineEl`'s click listener must check `data-act` before `data-sel`.**
  Every inline row action (a layer's ↑/↓/✕, a scene's ↑/↓/✕, an audio
  track's ✕) is a `data-act` button *inside* its row's own `data-sel`
  element. Checking `data-sel` first lets `e.target.closest('[data-sel]')`
  match the ancestor row on every button click, so the click just re-selects
  the row and returns before ever reaching the action — the exact way
  layer-up/layer-down/layer-del/audio-del silently did nothing for a full
  session of the redesign before this was caught by trying to give scenes
  the same inline reorder affordance and finding the copied pattern didn't
  work either. If a new inline row button is ever added and "does nothing
  when clicked" — check this ordering first.
- **The stage canvas is sized by intrinsic size + `max-width`/`max-height`,
  not `width: 100%; height: 100%; object-fit: contain`.** The latter was
  tried first and gives the canvas *element's own CSS box* the pane's shape
  (tall and narrow) rather than the film's; `object-fit` then letterboxes
  within that wrong-shaped box, leaving the frame small and off-centre with
  most of the pane empty. Leaving width/height `auto` and clamping with
  max-width/max-height lets the canvas's intrinsic size (its `width`/
  `height` attributes, set from the film's own dimensions) drive the aspect
  ratio, and the flex container's `align-items: center; justify-content:
  center` centers it properly. `#stage-overlay` (the direct-manipulation hit
  box) was never sized off the canvas element either way — `stageImageRect()`
  (main.js) computes its own letterbox rect from the overlay's box and the
  film's aspect ratio independently, so it already agrees with either.
- **Thumbnails are generated once per unique asset and cached for the page's
  lifetime**, not recomputed per render — `main.js`'s `ensureThumbnail`
  claims the cache key before the (possibly async) work starts, so a
  debounced reload never redoes it. A still's thumbnail is a real decode
  (`createImageBitmap` with `resizeWidth`/`resizeHeight`, the browser's own
  scaled-decode path — never a full-resolution decode then a manual canvas
  downscale, which matters for something like `kanto.png`'s 48-megapixel
  atlas). A clip's thumbnail costs nothing extra: its first `.srclip` frame
  is already a decodable JPEG, so `thumbnails.js`'s `clipThumbnail` just
  slices those bytes out of the container (`src/webclip.rs`'s layout) rather
  than decoding anything.

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
