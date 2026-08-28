# What Remotion gets right, what it gets wrong, and what ShowReel does instead

Read from Remotion's public documentation and API reference only — the Player
page, the templates gallery, the fundamentals, `<Sequence>`, `<Series>`,
`interpolate`, `spring`, `Easing`, `@remotion/transitions`, `@remotion/fonts`,
`@remotion/layout-utils`, parametrized rendering, and the render CLI. **No
Remotion source was read and nothing here is derived from their code.** The
ideas are theirs; the implementation is ours.

## The primitives, and what each is for

| Primitive | For |
|---|---|
| `<Composition>` | Registers a component with `width`, `height`, `fps`, `durationInFrames`. The unit that can be rendered. |
| `useCurrentFrame()` | The frame number, and the only legitimate clock. Their docs are blunt that animating by any other means flickers under render. |
| `useVideoConfig()` | `width`, `height`, `fps`, `durationInFrames` from context. |
| `<Sequence from durationInFrames>` | Time-shifts children. Children see a frame number that *starts at zero*, and nesting cascades (60 inside 30 begins at 90). |
| `<Series>` / `<Series.Sequence>` | Lays sequences back to back without cumulative frame arithmetic; `offset` nudges or overlaps. |
| `<AbsoluteFill>` | A full-bleed positioned div. Layering is CSS stacking. |
| `<Loop>`, `<Freeze>` | Repeat, and hold at a frame. |
| `interpolate(v, in[], out[], opts)` | Multi-keyframe value mapping with per-segment easing and extrapolation. |
| `spring({frame, fps, config})` | Physics-based motion; `durationInFrames` stretches the curve. |
| `Easing.*` | `linear`, `quad`, `cubic`, `poly`, `sin`, `circle`, `exp`, `elastic`, `back`, `bounce`, `bezier`, `step0/1`, composed with `in`/`out`/`inOut`. |
| `<Img>`, `<Video>`, `<OffthreadVideo>`, `<Audio>`, `staticFile()` | Media, referenced by path out of `public/`. |
| `<TransitionSeries>` + `.Sequence` / `.Transition` | Scene-to-scene transitions. |
| presentations × timings | `fade`, `slide`, `wipe`, `flip`, `clockWipe`, `none`, crossed with `linearTiming` / `springTiming`. |
| `@remotion/layout-utils` | `measureText`, `fitText`, `fillTextBox` — text measurement and fitting. |
| `<Player>` | Embeds a composition in a React app with real playback. |

## How a film is described

A film is a **React component tree evaluated once per frame**. The mental model
they lead with is exact and worth stealing verbatim: *a video is a function of
images over time*. You are handed a frame number and you draw.

Time is expressed in **frames**, everywhere — `from={30}`, `durationInFrames={90}`.
Composition is by nesting: a `<Sequence>` shifts its children's clock and
`<Series>` chains them. Data reaches a film through `defaultProps` / `inputProps`
(overridable from the CLI with `--props`, optionally validated with a Zod
schema), and `calculateMetadata()` can compute duration or dimensions from that
data before rendering.

## How animation is expressed

Two calls, driven off `useCurrentFrame()`, whose results are assigned to CSS:

```
const opacity = interpolate(frame, [0, 20], [0, 1], {extrapolateRight: 'clamp'});
const scale   = spring({frame, fps});
```

`interpolate` is the good one. One call takes a whole keyframe curve, takes an
easing per segment, and says explicitly what happens outside the range.

## How transitions are declared

```
<TransitionSeries>
  <TransitionSeries.Sequence durationInFrames={40}>…</TransitionSeries.Sequence>
  <TransitionSeries.Transition timing={springTiming(...)} presentation={fade()} />
  <TransitionSeries.Sequence durationInFrames={60}>…</TransitionSeries.Sequence>
</TransitionSeries>
```

Two ideas, both good:

1. **Presentation and timing are orthogonal.** How it looks is one object, how
   it is paced is another, and any pair composes.
2. **A transition overlaps its neighbours.** Both scenes render during it and
   the total shortens: 40 + 60 − 30 = 70 frames, not 100.

## How text and overlays are handled

Text is DOM and CSS, which is Remotion's strongest ground — kerning, wrapping,
web fonts and flexbox come free. Fonts load through `@remotion/fonts` or
`@remotion/google-fonts`, and rendering is held back with
`delayRender()`/`continueRender()` until the face is ready. `@remotion/layout-utils`
adds `measureText` and `fitText` for the case CSS cannot answer: *make this
headline as large as fits*.

The templates gallery is the best evidence of what people actually build with
it, and it is almost entirely typography and data: **Audiogram** and **Music
Visualization** (waveform plus text), **TikTok** (word-by-word captions),
**Code Hike** (animated code), **Stargazer** (a repository milestone),
**Watercolor Map** (2D map animation for travel films), **Overlay** (graphics to
lay over externally-edited video). Almost none of it is video effects. It is
titles, captions, counters and reveals.

## What the Player gives that a plain render does not

The captain pointed at this page specifically, and it is the sharpest part of
their offer. `<Player>` is real playback with `controls`, `loop`, `autoPlay`,
`playbackRate`, `inFrame`/`outFrame`, and an imperative ref: `play()`, `pause()`,
`seekTo(frame)`, `getCurrentFrame()`, `isPlaying()`, `requestFullscreen()`, plus
events (`frameupdate`, `seeked`, `timeupdate`, `ended`, …). Alongside it,
Remotion Studio gives a scrubbable timeline, hot reload, and a props editor.

The value is **the loop, not the pixels**: you change a number and see the
consequence immediately, and you can scrub to the beat you are unsure about
instead of rendering three minutes to check four seconds.

## What is genuinely awkward

1. **Frames as the authoring unit.** `durationInFrames={90}` means doing
   seconds × fps in your head constantly, and changing the frame rate rewrites
   every number in the film.
2. **`interpolate` has accumulated too many knobs.** `extrapolateLeft` and
   `extrapolateRight` as separate stringly options, plus
   `output: 'perceptual-scale'` and `posterize`, on one function.
3. **The transition rules are runtime errors.** A transition cannot be first or
   last, two cannot be adjacent, and one cannot be longer than its neighbours —
   all of which are perfectly writable, and none of which you learn until you
   run it.
4. **`layout="absolute-fill" | "none"`** — a CSS implementation detail surfacing
   in the timeline API.
5. **Fonts need an async escape hatch.** `delayRender()`/`continueRender()`
   exists because the renderer might screenshot before the browser has the font.
   That is a browser problem leaking into the film API.
6. **The browser itself.** Every frame is a DOM layout, a paint and a
   screenshot, and their own performance docs warn that high resolutions get
   slow and suggest `--scale`. It also means a ~300 MB Chromium download and a
   second toolchain.

## What ShowReel does with all of this

> **Transitions, seen:** [`docs/gallery/transitions.gif`](gallery/transitions.gif),
> from [`examples/gallery/transitions.film.jsonc`](../examples/gallery/transitions.film.jsonc)
> — a cross-blur dissolve then a wipe, each the "presentation × timing" split
> this table records as taken.

| Idea | Verdict | Where it lives |
|---|---|---|
| A video is a function of frames | **Taken wholesale** | `render.rs` |
| Local time starting at zero, cascading | **Taken** | `time::Span`, layers, `camera` |
| `interpolate` with multi-keyframe and per-segment easing | **Taken, simplified** — one `Extrapolate` enum, `Clamp` by default | `ease.rs` |
| Springs as a first-class primitive | **Taken** — fixed 1 ms timestep, so a spring looks the same at 24 and 60 fps | `ease::Spring` |
| Presentation × timing | **Taken** | `transition.rs` |
| Transition overlap arithmetic | **Taken** | `timeline::Timeline::placements` |
| `Series` chaining without frame maths | **Taken** — it *is* the timeline | `timeline.rs` |
| `measureText` / `fitText` | **Taken** — `TextLayout::fit`, one argument rather than a separate package | `text/mod.rs` |
| Data-driven films (`inputProps`, schemas) | **Taken** — serde *is* the film; JSON needs no separate concept | `timeline::Film` |
| Frames as the authoring unit | **Rejected** — seconds, resolved to exact frames once | `time.rs` |
| Transition rules as runtime errors | **Rejected** — `Scene (Transition Scene)*` makes three of the four unrepresentable | `timeline::Timeline` |
| `layout` prop | **Rejected** | — |
| `delayRender` for fonts | **Rejected** — we own the rasteriser; fonts are loaded before drawing | `text/font.rs` |
| A browser | **Rejected** — `tiny-skia` and `rustybuzz`, no DOM | — |
| A scrubbable Player | **Not matched; approached from the other side** — stills in ~100 ms, a labelled contact sheet of the whole film in ~3 s, and a genuinely scaled-down pass | `preview.rs`, `scale.rs` |

## Why a data tree and builders, and not a macro DSL

Rust has no JSX and no hooks, so the question is what the honest local
equivalent of a declarative timeline is. Four candidates:

1. **A serde data tree** — you *describe*, you do not execute. Declarative in
   the strict sense.
2. **Builders returning `Self`** — ergonomic construction of that tree.
3. **Traits at the extension points** — `Present` for transitions, `Resolver`
   for assets.
4. **A `macro_rules!`/proc-macro DSL** that apes JSX.

ShowReel is 1 + 2 + 3, and deliberately not 4. A macro DSL would buy a little
syntax and cost error messages, rust-analyzer completion and the ability to read
the film back out. Whereas the data tree pays three times over:

- **the JSON film format is free**, so timing changes need no recompile — which
  is exactly what makes the preview loop fast, and the preview loop is the gap
  we were told not to concede;
- **determinism and caching become easy**, because a film is a value that can be
  hashed and compared;
- **it is the clean seam for MCP and asset fetching**: a tool that emits JSON is
  a first-class film author, not an integration.

The builders then give the "considered API" feel without inventing syntax:

```rust
Film::new(1920, 1080, 60.0)
    .open(Scene::new(5.0).layer(Layer::title("…").entering(Motion::chars(0.5, 0.02))))
    .then(Transition::dissolve(0.8), Scene::new(11.0).layer(Layer::camera("map.png", camera)))
```

`Film::new` returns a `FilmSpec`, not a `Film`, and only `.open(scene)` produces
a `Film` — because a film with no scenes is not a thing, and the type should say
so. That is the same instinct as Remotion's transition rules, honoured with the
type system instead of an error message.
