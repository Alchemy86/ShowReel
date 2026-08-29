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
| `Layer.drift: Option<Drift>` is the animated-placement primitive `enter`/`exit` deliberately don't provide: `Motion` only ever carries a layer INTO or OUT OF a fixed `placement` (the middle of a layer's life always settles to `MotionState::SETTLED`), where `Drift` runs continuously across the layer's *whole* active span, added onto `motion_at`'s result in `Layer::draw` before any `shift`/`shift_scaled` call — so it composes for free with a still, a clip, a camera or a parallax stack with zero new drawing code. `Layer::burst`/`BurstSpec` builds a burst effect (several clips clustered near a point, fanned onto evenly-spaced-and-jittered headings, staggered launches) purely from `Drift` + `Placement::Frac` + `.framed()` — a convenience, not a fourteenth `Content` kind, and every field it sets is ordinary layer JSON (`examples/burst_demo.film.jsonc` is exactly what it produces). Default easing is `in-quad` (accelerating outward reads as energy; `in-cubic` was tried and sat nearly still for the first 40% of its own travel — a constant speed reads as a slide) | `src/motion.rs` (`Drift`), `src/layer.rs` (`Layer::draw`, `Layer::burst`, `BurstSpec`) |
| `Grade` (the colour-grade pass, `docs/anarchist-study.md`'s other confirmed gap) follows the same idiom as `Content::Parallax`: a composition convenience, not a new render path. It is `Film.grade`/`Scene.grade` — an `Option`, the same override shape `background` already has — applied once per scene by `Renderer::draw_scene` *after* every layer has drawn, so it composes with a still, a clip or a parallax stack with zero knowledge of what any of them contain. Written as a bare word (`"documentary"`) or a full object, the same untagged shorthand `Placement` uses | `src/grade.rs`, `src/canvas.rs` (`apply_grade`), `src/render.rs` |
| `Content::Chart` (animated charts — a plotted function/line or growing bars) is one layer content kind, not a parallel system: `ChartSpec` is data like every other layer, and its one animation is a **reveal sweep** — a single eased 0..1 crossing the plot left-to-right on the film clock, the same `value_at(local)` idiom `BarSpec`/`CounterSpec` use, not a private animation vocabulary. A function series is a string (`"40*log(x+1)"`) parsed once by `src/expr.rs` (a tiny arithmetic evaluator — the four ops, `^`, `x`, a fixed function set, `pi`/`tau`/`e`; anything else is a `validate()` error, never a silent zero). Composition is inherited, not built: the grade lands on the finished pixels, a callout/title is a higher-`z` layer, and "push in on a chart" is `PullUp` (a bitmap lift of the drawn region) — the showreel `Camera` is a still/clip mip feature and is deliberately *not* bolted onto procedural drawing. Audio-mapped-to-curve (the reference short does it) was deliberately **not** started | `src/chart.rs`, `src/expr.rs`, `src/layer.rs` (`Content::Chart`, `draw_content`) |
| A chart reads its series from an external **CSV/JSON** via `Series::Data { file, x, y, bars }`, resolved through `AssetStore` like a still (`AssetStore::data`, cached `DataTable`) — *not* a second mechanism. `ChartSpec::resolve(assets)` expands every `Data` series into a concrete `Line`/`Bars` (borrowed `Cow` when there is none), so `chart::draw` is unchanged below that one call and stays a pure function of `(spec, assets)`. `chart::draw` therefore returns `Result` and the `Content::Chart` arm propagates `?` — that is what makes a bad file/column/row loud in `still`/`render` (no preload) as well as `check` (`Film::resolve_chart_data`). The film names the columns, so it still reads as *x against y* without opening the data | `src/assets/data.rs`, `src/chart.rs` (`Series::Data`, `resolve`), `src/timeline.rs` (`resolve_chart_data`, `AssetUse::Data`) |
| A **plugin** is a new layer kind expressed as *data*, not code: `Content::Custom { use, with }` names a `Plugin` (a parameterised template of ordinary layers) in `Film.plugins`, and `Film::expand_plugins(assets)` substitutes `{{param}}` and replaces each `custom` layer with the concrete layers it denotes. Declarative was chosen over a Rust trait (forces compile-against-us, breaks "a film is JSON") and a dynamic library (unsafe, and `wasm32` has no `dlopen` — would split native/browser); the template runs identically in both builds because expansion is pure. It cannot draw a mark the primitives can't, and parameter arithmetic is deliberately unbuilt (direct substitution only) | `src/plugin.rs`, `src/timeline.rs` (`expand_plugins`), `src/layer.rs` (`Content::Custom`) |
| ffmpeg is invoked directly rather than reusing `agentgb`'s Python `video.py` | `src/encode.rs` |
| A `Clip`'s memory is bounded by a window, not by clip length: below an estimated-size threshold it decodes eagerly (unchanged, cheap for a sting reused across many cuts); above it, frames stream from a running `ffmpeg` through a cache sized to the render's own parallelism (`stream_cache_frames`, tied to `rayon::current_num_threads()` so `render.rs`'s intentionally out-of-order parallel chunk access mostly hits rather than reseeks) and capped by a byte budget regardless of resolution. `frame_at` returns `Result<Option<Arc<Pixmap>>>` rather than the old borrowed `PixmapRef`, because a streaming frame lives behind a `Mutex`-guarded cache and a real decode failure can now surface lazily rather than only at load. Measured before/after in `docs/clip-streaming.md` | `src/assets/clip.rs` |
| `showreel render`'s default whole-film path is segmented and resumable: fixed-size chunks each become a `.ts` file (concatenated + audio-muxed once at the end, never per segment — audio changes must never invalidate an already-rendered video segment), tracked in a small JSON manifest keyed by a fingerprint of the resolved film + every asset's stat + video encode settings. A kill mid-render leaves finished segments on disk; a rerun with the same fingerprint skips them and says so. `--frames`/`--png` keep the old single-pass path — debugging tools, not the "dies at 90%" case this exists for. Proven at the pixel level, not just by design: `render.rs`'s `sequential_render_ranges_over_a_streaming_clip_match_one_continuous_pass` confirms several sequential `render_range` sub-ranges over a *streaming* `Clip` produce byte-identical frames to one continuous pass — the invariant the whole feature depends on. See `docs/segmented-rendering.md`, including a real investigation of an apparent divergence that turned out to be an encoder GOP-boundary artifact at an aggressive test `--crf`, not a bug | `src/segments.rs`, `src/encode.rs` (`finish_segmented_render`) |
| GIF export (`showreel gif`) is a `FrameSink` (`GifSink`), not a second render path — the same renderer's raw `rgb24` frames pipe into one ffmpeg `palettegen`/`paletteuse` filtergraph (palette generated *from the footage*, Lanczos downscale, `fps` decimation, all one pass). A subcommand not a `render` flag, because a GIF is a *window* of the film (`--from`/`--to` in seconds) at its own width/fps. Default dither is **ordered (Bayer)**, not error-diffusion: a GIF loops, and ordered dithering is a fixed function of pixel position so a static background does not crawl | `src/encode.rs` (`GifOptions`, `GifSink`), `src/bin/showreel.rs` (`cmd_gif`) |
| Assets are resolved through `AssetStore`, the seam for MCP/fetching later | `src/assets/mod.rs` |
| Audio hangs off the *film*, not a scene; `Audio` describes, `AudioInput` is resolved | `src/audio.rs` |
| A clip's own soundtrack (`ClipAudio`, on `Content::Clip`) is a level, not a placement — its `at`/`from`/`duration` are the clip's own timing, so `clip_track` builds its `AudioInput` by delegating to `Audio::resolve` rather than re-deriving fade clamping | `src/audio.rs`, `src/layer.rs` (`Layer::clip_audio_track`), `src/timeline.rs` (`Film::clip_audio`) |
| Generated music (`Audio.music: Option<Music>`) is a *source*, not a parallel audio path: a `Music` spec resolves to a synthesised WAV exactly where a file track's `asset` resolves to a path (`Film::resolve_audio_tracks`), so from `Audio::resolve` on it is an ordinary `AudioInput` — fades/gain/mixing/mobile compose for free. One idiom (the NES/SID chiptune the devlog's `chiptune.py` proved, ported faithfully — envelope/spectrum correlation ≈0.9997/0.9999 vs the reference), not a synthesiser: a `Mood` is *data* (a progression + step patterns). Authoring mirrors `Grade`'s bare-word-or-object shorthand. Deterministic (a seeded `splitmix64`, unlike the reference's `numpy` global RNG) so renders reproduce and the temp WAV can be content-addressed. `MusicFit::Film` nudges the tempo so a whole number of bars spans the film (ends on a downbeat); the general per-cut-time solver is a documented next step, not half-built | `src/music.rs`, `src/audio.rs` (`Audio::music`), `src/timeline.rs` (`resolve_audio_tracks`, `resolve_music`) |
| Narration (`Audio.narration: Option<Narration>`) is a *source* like music — a third arm in `resolve_audio_tracks` beside `music`/`asset`, so from `Audio::resolve` on it is an ordinary `AudioInput` — but with one deliberate difference: it does **not** synthesise at render. The voice is Kokoro, a Python model; synthesising per-render would make every `render` need that stack and break wasm. So it follows the clip/`web-pack` pattern instead: an explicit `showreel narrate` bakes it to a WAV **ahead of time**, and render only *finds* that WAV (`resolve_narration`, content-addressed by `Narration::baked_name` → un-findable if the script changed → loud "run narrate", never a stale take). A narration track's natural length is the *speech* (read from the baked WAV header in `resolve_audio_tracks`), not "to end of film". Direction is per-line (`pace`→synth speed, `pause_before`/`pause_after`→inserted silence). The engine **and** voice are film-declared (`engine`,`voice`); Kokoro/`bm_george` is one `SynthEngine` behind `narrate::engine_for`, not the design — a per-engine default voice (`default_voice`) and a `KNOWN_ENGINES` list checked in `validate` | `src/narration.rs` (spec/assembly/corruption check, all pure), `src/narrate.rs` (the `SynthEngine` seam + Kokoro bake), `src/audio.rs` (`Audio::narration`), `src/timeline.rs` (`resolve_narration`) |
| Film files accept a narrow JSONC subset (comments, trailing commas) — deliberately not full JSON5 | `src/timeline.rs` |
| The browser studio polls (the film file's mtime, and `/api/state`) rather than holding a socket open — one `tiny_http` worker thread per held connection is the cost a blocking server can't hide | `src/studio.rs` |
| "API access" means the local HTTP surface, not the (already stable, already documented) Rust library — and it rides the studio's own server rather than a second, stateless one: `/api/render`/`still`/`info`/`check` answer against whatever film the running `showreel studio` instance already has loaded and validated, so there is one code path for "load a film," not two that can drift | `src/studio.rs`, `README.md`'s "API access" |
| The MCP server (`showreel mcp`) uses `rmcp`, the official SDK, not a hand-rolled JSON-RPC framing — and its five tools mirror the CLI's own vocabulary (`new`/`check`/`info`/`still`/`render`) rather than one tool per `Content` variant: a film is JSON an agent already edits with ordinary file tools, so "build" only needs scaffolding, not a bespoke authoring API | `src/mcp.rs` |
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

## Sharp edges

- **Nothing may call `Theme::default()` while drawing.** Use `RenderCtx::theme`, which is
  the *film's* theme. Reaching for the crate default silently ignores both a custom theme
  and `scale::scale_film`, so every preview thumbnail draws 1080p type into a 320px frame.
  `render.rs` has the regression test.
- **A `Layer`'s `placement` is `Option`.** `None` means "wherever this content belongs"
  (`Content::default_placement`) — a lower-third goes bottom-left, a counter top-right. A
  JSON layer with no placement must land where the Rust builder would put it.
- **`Clip::load` no longer holds a whole decoded clip in memory** (it did,
  until an OOM on 2026-08-29 — see `docs/clip-streaming.md` for the measured
  before/after). A clip above `EAGER_MAX_BYTES` (256 MiB estimated) decodes
  lazily from a running `ffmpeg` through a bounded frame cache instead;
  `trim` still narrows what's probed and (for a small clip) decoded, but is
  no longer the only thing standing between a long source and an
  out-of-memory render. `Content::Clip.trim`'s own doc comment still says
  "decoding is bounded by this" — true, just no longer the *whole* story.
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
- **An unmuted `Content::Clip` whose *source file has no audio stream at all*
  fails the whole render, not just that clip.** `Audio::clip_track` builds an
  `AudioInput` from `ClipAudio::default()` (`muted: false`) without checking
  the file actually has an audio stream, so `encode.rs` wires a `[i:a]`
  reference into ffmpeg's mix filtergraph that matches nothing, and ffmpeg
  errors out ("Stream specifier ... matches no streams") rather than the
  crate skipping that one track. This is a real, common shape — confirmed
  against actual screen-captured gameplay footage (`~/pokemon-run/*.mp4`),
  which is video-only — not a synthetic-test-clip edge case. `Layer::burst`
  works around it by defaulting `BurstSpec::mute_audio` to `true`, but the
  underlying gap is in `clip_track`/`encode.rs`, not `layer.rs`; a real fix
  would probe the source (or catch and skip the failing input) before
  building the filtergraph. Found and worked around, not fixed, while adding
  the burst effect — a good next session's task.
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
- **`canvas::apply_grade` is a single per-pixel pass, cheap compared to
  `CrossBlur` above but not free.** Measured at **~34-40ms a call** at
  1920×1080, release build (`canvas::tests::apply_grade_cost_at_1080p`) —
  roughly 3× the whole example film's own 12ms/frame average
  (`docs/performance.md`), once per scene per frame a grade is set on. No
  second buffer and no windowed operation like a box blur needs, so the cost
  is exactly the pixel count times a fixed handful of float ops; it does not
  grow with any of the grade's five knobs. `Grade::default()` (every film's
  default — grading is opt-in) short-circuits to a no-op before touching a
  single pixel. See the "A colour grade" section of `README.md`.
- **`showreel studio` needs `cargo build --features studio`** — the plain
  binary does not have the subcommand at all, on purpose (`src/studio.rs`).
- **`/api/render` runs ffmpeg and writes a file for whoever can reach the port.**
  `StudioOptions::host` defaults to `127.0.0.1` for exactly this reason; `serve()`
  prints a startup warning naming the risk the moment `--host` is anything else. This
  is not a check to relax later — a render endpoint open to a network is a
  resource-exhaustion (and, depending what else is running, code-execution-adjacent)
  surface, per the brief that added it. Extend the studio server's API further with
  the same default in mind, not just this one endpoint.
- **`showreel mcp` needs `--features mcp`** (a plain build has neither the subcommand nor
  `rmcp`/`tokio` in the dependency graph), and its `tokio::runtime::Builder` must call
  `.enable_time()`. Found the hard way: `rmcp` uses a timer internally (request timeouts,
  shutdown draining), and a runtime built without it doesn't fail at startup — every tool
  call answers fine — it panics later, the first time shutdown actually needs the timer.
  `tests/mcp_server.rs`'s `the_server_shuts_down_cleanly_after_real_work` exists
  specifically to catch this regressing; a test that never closes stdin would not.
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

- **`examples/showreel_demo.film.jsonc` is the demo reel, and unlike the kanto
  film it is hand-written, not generated.** It is the tool's own showreel — an
  ~52s film demonstrating the camera, a clip push-in, parallax, the colour
  grade (shown ungraded-then-graded), kinetic text, callouts and a mixed
  soundtrack — authored directly as JSONC (there is no `.rs` source and no
  `--check` drift guard). The rendered cuts are committed at
  `docs/showreel-reel.mp4` (+ `.mobile.mp4`); its assets are *not* committed
  (map + one clip from `~/pokemon-run`, plus synthesised audio and
  ImageMagick-cut parallax/grade stills), and every one is documented with an
  exact regen command in `examples/showreel_demo.assets.md`. Re-render with
  `showreel render examples/showreel_demo.film.jsonc -A <assets> --crf 27 -o
  docs/showreel-reel.mp4` (crf 27, not the default 17, keeps the dithered
  pixel-art master under ~30 MB). If you edit the film, regenerate both cuts by
  hand and re-verify audio on each — nothing regenerates it for you.

- **`examples/gallery.rs` is the canonical source for the README gallery** —
  the eight single-idea proof films (`examples/gallery/*.film.jsonc`) and the
  three procedural stills they need (`examples/gallery/assets/`, ~1.1 MB, kept
  small on purpose — see the poster's "blocky not hatched" comment). Regenerate
  with `cargo run --release --example gallery`, then render every GIF with
  `examples/gallery/render.sh` (needs the release `showreel` binary), which
  writes `docs/gallery/*.gif` (~2.3 MB total) — the images the README's gallery
  and the four studies' "proof" pointers link to. Unlike kanto there is no
  `--check` drift guard; if you edit the `.rs`, rerun both steps and **watch the
  GIFs back** (this is judged by eye — a badly-dithered or illegible GIF is
  worse than none). The two full-frame-motion GIFs (camera, parallax) are the
  only heavy ones and are rendered at 128 colours; the rest are defaults.

- **`examples/data_plugin_demo.film.jsonc` is the proof film for external chart
  data and plugins**, hand-written JSONC with its assets committed beside it
  (`examples/data_plugin_demo/adoption.csv`, `regions.json`; the shareable
  plugin at `examples/stat_card.plugin.json`). Unlike the kanto/showreel films
  its assets *are* committed — they are tiny text files, the whole point being a
  self-contained, runnable demonstration. Rendered cut at
  `docs/data-plugin-demo.mp4` (+ `.mobile.mp4`); re-render with `showreel render
  examples/data_plugin_demo.film.jsonc -A examples --crf 23 -o
  docs/data-plugin-demo.mp4`. It is also the fixture the README's data/plugin
  sections point at.

- **`examples/music_demo.film.jsonc` is the proof film for generated music
  (`src/music.rs`)**, hand-written JSONC and fully self-contained — it ships *no*
  assets (every background is a colour, the only sound is the synthesised
  chiptune), which is the whole point. Rendered cut at `docs/music-demo.mp4`
  (+ `.mobile.mp4`); re-render with `showreel render
  examples/music_demo.film.jsonc -o docs/music-demo.mp4` (no `-A`). Its four
  scenes are bar-aligned at 128 BPM with hard cuts and a `fit: "film"` track, so
  the cuts land on downbeats — verified by onset-detecting the rendered audio
  (~7 ms of a bar line at each cut), not by eye. If you edit it, re-render both
  cuts by hand and re-verify audio (`ffprobe`/`volumedetect` a real level on the
  mobile cut, the same as the showreel reel).

- **`examples/narration_demo.film.jsonc` is the proof film for narration
  (`src/narration.rs`)** — the ASCII-city pitch read by Kokoro `bm_george` over
  that engine's footage, the chiptune ducked underneath, captions cued from the
  bake's word manifest. Its baked narration WAV + `.words.json` **are** committed
  (in `examples/narration_demo/`) — the whole point is that render needs no voice
  model — but the footage `city-film.mp4` is **not** (it is ASCII-city-engine
  output, `.gitignore`d; see `examples/narration_demo.assets.md`). Two steps, in
  order: `showreel narrate examples/narration_demo.film.jsonc -A
  examples/narration_demo -o examples/narration_demo` (needs the Kokoro venv),
  then `showreel render examples/narration_demo.film.jsonc -A
  examples/narration_demo --crf 30 -o docs/narration-demo.mp4`. If you edit the
  narration lines, the WAV's content-addressed name changes — re-bake, commit the
  new WAV/manifest, delete the old, re-render both cuts, and re-verify audio on
  each (measured on the committed cut: master −23.7 dB, voice ~10 dB over the
  ducked bed, corruption ZCR 0.047, prosody `pitch_var_st` 3.21 st).

- **`examples/burst_demo.film.jsonc` is the proof film for the burst effect
  (`Drift`, `Layer::burst` — src/motion.rs, src/layer.rs)**, hand-written
  JSONC. It uses six small synthetic (`ffmpeg testsrc2`, hue-shifted so they
  stay visually distinguishable) clips rather than real footage, on purpose:
  the effect is subject-agnostic, and its own proof film's assets follow the
  same "nothing in the crate may know what its films are about" rule as
  `src/`. Not committed — see `examples/burst_demo.assets.md` to regenerate
  them. Rendered cut at `docs/burst-demo.mp4` (+ `.mobile.mp4`, both silent —
  every clip has `audio.muted: true`, see the sharp edge on an audio-less
  clip below); re-render with `showreel render examples/burst_demo.film.jsonc
  -A examples/burst_demo --crf 23 -o docs/burst-demo.mp4`. The film's own
  header comments record what was actually tuned by watching it (easing,
  `.framed()`'s rounded corners/border/shadow, timing) rather than guessed —
  read those before changing the effect's defaults.

- **`tools/narrate/` and `tools/prosody/` are the narration support tools.**
  `tools/narrate/kokoro_narrate.py` is the thin Kokoro driver `showreel narrate`
  embeds (`narrate::DRIVER` — keep them byte-identical, a test asserts it).
  `tools/prosody/` holds the naturalness harness (`prosody.py`, numpy-only) and
  the standalone corruption check (`check.py`, stdlib-only); `python3
  tools/prosody/test_prosody.py` tests both with synthetic signals, no voice
  model. These are the tools the voice was measured with — see their READMEs.

- **Generated music needs the wasm blob rebuilt.** `Audio.asset` became
  `#[serde(default)]` when `Audio.music` was added, so a film with a music track
  fails to load in a browser running an *older* `tools/web/showreel.wasm`
  (`missing field 'asset'`). Any `Audio`-schema change means `./build-wasm.sh`
  and committing the new blob. In the browser, a music track is a *snapshot*:
  `showreel web-pack` pre-synthesises the WAV natively (the DSP is pure Rust but
  the browser has no filesystem for `Music::render_to_temp`) and lists it in
  `music.json`, which `tools/web/main.js`/`audio.js` play as a cue like a
  film-level track — editing a music track's mood/key/bpm needs a repackage to be
  heard, the same staleness a `.srclip` carries. Inline-in-the-browser synthesis
  (a wasm PCM export → Web Audio) is a documented next step, unbuilt.

- **Narration bakes ahead of render; the render path only *finds* the WAV.**
  Unlike music (pure Rust, synthesised every render), narration needs `showreel
  narrate` run first (Kokoro is Python — see below). `render`/`still`/`check` do
  **not** synthesise; `resolve_narration` looks up the content-addressed
  `Narration::baked_name` on the `-A` asset path and fails loudly ("run
  `showreel narrate`") if absent. So the contract is: bake into your assets dir,
  then render with that same `-A`. The name is a hash of the script (engine,
  voice, every line's text/pace/pauses), so editing a line makes the old bake
  un-findable rather than letting a stale take through — the same freshness
  discipline `.srclip` and `Music::render_to_temp` have. `check` validates the
  *spec* (engine known, voices/lines non-empty) but not that it is baked, exactly
  as it does not synthesise music — the missing bake surfaces at render.

- **The corruption gate is native and runs before any WAV is written.** A neural
  vocoder can emit plausible-looking white noise that passes every codec/decode
  check; zero-crossing rate of the loudest window separates it (speech ~0.13,
  noise ~0.49). `narration::zero_crossing_rate`/`looks_like_speech` (a Rust port
  of `tools/prosody/check.py`, so verification never depends on the Python stack
  that produced the audio) checks **every synthesised line and the assembly**,
  and `bake_narration` bails rather than write a corrupt file. Keep this — the
  captain was sent broken audio twice before this check existed.

- **The Python dependency is at *bake* time only, and the engine is a seam.**
  `showreel narrate` shells out to a venv running Kokoro (`SHOWREEL_KOKORO_PYTHON`
  / `--python` / default `~/.local/share/kokoro-venv/bin/python`); `render` needs
  no Python at all. The driver `tools/narrate/kokoro_narrate.py` is `include_str!`d
  into the binary (`narrate::DRIVER`) so `showreel narrate` is self-contained.
  Two venv gotchas, handled in `run_driver` but worth knowing: **unset
  `VIRTUAL_ENV`** (Kokoro's spaCy install throws a confusing error otherwise),
  and the venv needs **`pip` inside it** (Kokoro shells out to install a model on
  first use). Kokoro pins numpy 1.26/2.x → **Python 3.12** (not 3.14). The engine
  is chosen by `narration.engine` via `narrate::engine_for`; adding a model (e.g.
  voice-cloning) is a new `impl SynthEngine`, one row in `engine_for`, and one in
  `narration::KNOWN_ENGINES` (keep those two in lock step) — `bm_george`/Kokoro is
  a default, not the design. `tools/narrate/README.md` has the full setup.

- **Narration also needs the wasm blob rebuilt (same reason as music), and it
  is a browser *snapshot*.** Adding `Audio.narration` is an `Audio`-schema change,
  so `./build-wasm.sh` and commit the blob — confirmed the hard way: an older blob
  ignores the unknown `narration` field, sees an empty track and fails
  validation. In the browser a narration track plays like music: `web-pack`
  copies the **already-baked** WAV (it does *not* synthesise — no Python in
  `web-pack`) and lists it in `narration.json` with its at/gain/fades + the
  speech's own length; `tools/web/main.js`/`audio.js` play it as a cue through the
  exact same envelope a music cue uses. A missing bake is the same loud failure
  `render` gives. Editing the narration needs a re-bake **and** a re-`web-pack` to
  be heard, the snapshot staleness `.srclip`/`music.json` already carry.

- **`Rect::to_aspect` grows, `Rect::inscribed_aspect` crops.** `Fit::Cover` needs the
  second. Using the first letterboxes a square source into a wide frame — the exact
  opposite of covering it.

- **A `Content::Chart` is either xy or categorical, never both.** The moment any
  series is `Series::Bars` the whole chart is a bar chart — its x-axis is the bars'
  categories — and any non-bar series in it is ignored and flagged by
  `ChartSpec::problems`. A line and bars share no x coordinate system; mixing them
  is an authoring mistake to split into two charts, not a combo to support
  (`src/chart.rs`, `ChartSpec::is_categorical`).

- **A `Series::Function` has no domain of its own** — a string like `"sin(x)"`
  does not say *over what x*. So a function chart with `x.min`/`x.max` unset is a
  `validate()` error, not a guess; `Layer::chart_function(expr, x0, x1)` sets the
  range for you. Explicit `Series::Line` points carry their own x, so they need no
  range (`src/chart.rs`).

- **Chart tick labels are laid out with `TextLayout::build` every frame**, the
  same as `Counter`/`Title` — there is no cross-frame label cache, on purpose
  (frames render in parallel and out of order; a chart must stay a pure function
  of its frame). It is most of the chart's ~9ms/frame at 1080p; a 240-sample
  gradient-stroked filled curve on its own is cheap. Don't reach for a cache
  without solving the determinism/ordering it would break first (`src/chart.rs`).

- **A `custom` layer MUST be expanded before it reaches the renderer.** Every
  render entry (`cmd_render`/`still`/`sheet`, studio's `load`, mcp's
  `render_*`/`check`, wasm's `sr_load_film`) calls `Film::expand_plugins` first;
  a `Content::Custom` that reaches `draw_content` is a wiring bug and *bails
  loudly* rather than drawing nothing. If you add a new render path, expand
  there too. `expand_plugins` returns a derived film (plugins/`custom` gone) and
  leaves the authored film round-trippable — do not expand in place.

- **The "loud at load" contract is split across two seams, keep both.** For
  charts-from-data: structural checks (`file`/`x`/`y` present) live in
  `ChartSpec::problems` (no assets, runs in `validate`); the file/column/row
  checks need the store and run in `ChartSpec::resolve` — surfaced by
  `chart::draw`'s `?` on every render path and by `Film::resolve_chart_data` for
  `check`. Neither alone is enough: `validate` can't open files, and a render
  that skipped `resolve_chart_data` would still be caught by draw, but `check`
  (which never draws) would not. For plugins, `expand_plugins` is the one seam.

- **In tests/plugins, a JSON hex colour (`"#e26"`) closes a `r#"..."#` raw
  string early** — the `"#` sequence is the terminator. Use `r##"..."##`. Bit
  the plugin unit test and the integration test once each; both now use `##`.

- **The browser cannot read a chart's data file** — `AssetStore::data` reads the
  filesystem, which `wasm32` has none of. `web-pack` copies the CSV/JSON into the
  package, but nothing wires `insert_data` through `bridge.js` yet, so a packaged
  chart reading external data is a *known, documented gap* (same shape as a clip
  needing pre-decode). Inline chart data works in the browser; a `Series::Data`
  there needs an `AssetStore::insert_data` call that does not exist on the JS side.

## Working on it

- `cargo test` — unit tests live beside their modules; `tests/render_pipeline.rs` renders a
  film using every layer kind and checks determinism and the JSON round trip. It uses a
  tiny frame on purpose: it is what catches clamp arithmetic that only holds at 1080p.
- **Rebuild with `cargo build --release --bins --examples`.** A bare `cargo build
  --release` does not rebuild examples, and `--examples` does not rebuild the `showreel`
  binary. Rendering with a stale half of the pair produces output that contradicts the
  source and wastes a debugging cycle — this has happened twice.
- **Do not run `cargo fmt` on this tree.** It is not rustfmt-formatted (the pre-land
  check is `cargo clippy`, not `fmt`), so `cargo fmt` reformats ~46 files at once and
  buries a real change under whole-file churn that conflicts with other worktrees. Match
  the surrounding style by hand instead; format only the lines you add if you must.
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
- **MCP server**: `showreel mcp` (needs `--features mcp`, which also needs `cli`) runs an
  MCP server over stdio — see the README's "MCP support" section for the tool list.
  `cargo build --features cli,mcp --bin showreel` builds it; `cargo test --features mcp`
  runs `tests/mcp_server.rs`, which spawns the real binary and speaks real
  newline-delimited JSON-RPC over stdio pipes rather than calling the tool functions
  in-process. `cargo clippy --features cli,parallel,studio,mcp,wasm` is the combined
  check this project runs before landing changes that touch more than one optional
  feature, since features can compile clean individually and still conflict combined.
- **Supply-chain gate** (`tools/supplychain/`, `deny.toml`): three gates against a
  malicious/unreviewed crate — `cargo audit` (known vulns), a stdlib-only build-script
  drift + typosquat guard (`guard.py`, the half that would have fired on the 2026-08-20
  arrayref attack before any advisory), and `cargo deny` (crates.io-only sources,
  licence allow-list, duplicate detection). `tools/supplychain/check.sh` is the fast
  offline gate (~0.09s, wire it as a pre-push hook via `install-hooks.sh`);
  `scan.sh` is the full networked scan. The committed `buildscript-baseline.json` is
  the reviewed state — when a build-script change is legitimate, re-run `guard.py
  --update-baseline` (that is the workflow, not a way to silence it). Every gate is
  proven to fire by `selftest.sh`/`deny-selftest.sh`; `audit.sh` reports exit 3 (loud)
  rather than a false green when the advisory DB can't be reached. See
  `tools/supplychain/README.md`, including its honest "what each does NOT catch".

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
