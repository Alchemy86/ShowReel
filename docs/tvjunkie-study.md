# TV Junkie, studied for technique — and what ShowReel would need to match it

The captain sent one video directly: TV Junkie's *"How Much Would It Cost To Run Vacuum
Guy's Business in Real Life?"* (`youtu.be/FWrJrgadaU0`), with a specific hook — "has things
like a bankroll counter to demo what's being talked about." This is that study, in the same
shape as `docs/anarchist-study.md`, `docs/youcut-study.md` and `docs/remotion-study.md`:
technique and tooling, not content.

## What this is based on, and what it is not

**I watched the real video, not a description of it.** Using `chrome-devtools-axi` I opened
`youtu.be/FWrJrgadaU0` in a real Chrome tab and captured actual decoded frames from the
`<video>` element at scattered points across its full 30:02 runtime — confirmed, distinct
frames exist at roughly 0:05, 0:15–0:30, 0:60, 0:90, 0:120, and two more later in the film
(see below). Every technique claim in this study is anchored to one of those frames, not a
guess from the title.

**I could not get reliable, dense coverage of the whole runtime, and I am saying so plainly
rather than inventing what I couldn't see.** Three real problems compounded:

- **Captions/transcript are unavailable for this video** — the same limitation
  `docs/anarchist-study.md` hit. YouTube's own "Show transcript" panel exists in the UI but
  returns no segments, and the CC button itself reports "Subtitles/closed captions
  unavailable." So nothing here rests on narration content or dialogue — everything is what
  is visibly on screen, described in my own words.
- **The video carries an unusually dense ad load.** Nearly every attempt to pause and seek
  to a new timestamp landed on a pre-roll or mid-roll ad instead of the main content — this
  is a real, monetized 30-minute video, and YouTube inserts breaks aggressively on it. Nine
  separate seek attempts aimed at the 400–1800s range returned ad creative (for a phone
  cable, a wristwatch, a Roblox game, a beach holiday, a dust-mop) instead of the film.
- **Headless Chrome's video compositor went stale under repeated seeking**, and twice the
  player itself hard-crashed ("Something went wrong. Refresh or try again later.") after a
  handful of seeks in one page session, needing a full reload to recover. Several capture
  attempts returned the *previous* frame relabelled with a new (correct-looking)
  `currentTime` reading rather than a new picture — caught by comparing file bytes across
  nominally-different timestamps, not assumed away.

Net effect: I have **six confirmed, distinct, directly-observed frames** spread across
roughly the first two minutes and two points later in the film (approximate location only —
the staleness bug means the exact second isn't trustworthy for the later two), which is
enough to characterise the film's visual grammar with real confidence. I do **not** have a
frame that shows the bankroll counter itself on screen. Where I reason about the counter
below, I say exactly what that reasoning rests on (the captain's own description, plus one
specific, checkable piece of audience evidence) rather than describing a graphic I never
actually saw.

## Who they are and what the video is, concretely

TV Junkie is a 24.1K-subscriber channel whose visible catalogue (from the video's own
sidebar of related uploads) is entirely "How Much Would It Cost To *do the criminal thing a
TV/film character did*, In Real Life?" essays — this video sits beside sibling uploads
literally titled *How Much Would It Cost To Build Gus Fring's Meth Lab in Real Life?* and
*…Gus Fring's Meth Empire in Real Life?*, each with a thumbnail built around the same
formula: the character's photo, bold "HOW MUCH WOULD IT COST?" cover type, and cash imagery.
This video applies that same premise to "Ed," the vacuum-repair-shop-owning identity-broker
character from *Better Call Saul*/*El Camino* — the pinned comment states plainly that the
video's whole premise ("costing out Ed's operation") was a viewer-submitted request the
channel then made. It is narrator-over-evidence, the same format `docs/anarchist-study.md`
described for a different channel: no host on camera, footage and stills cut together under
voice-over.

### What I directly observed, frame by frame

1. **~0:05, cold open.** A desaturated, angled map of the continental US reaching into
   southern Canada, several small circular pin markers scattered across it (a cluster in the
   upper Midwest, one near the Great Lakes/New York, one out toward the Yukon), with a real
   photo of a roadside/parking-lot scene bleeding through faintly underneath — an
   establishing "here is the map of this operation" graphic before the title even lands.
2. **~0:15–0:30, title stamp.** A soft-focus, blurred portrait of a bearded man against a
   plain sky background, a bold red-and-white "CRIMINAL" banner stamped diagonally across
   his eyes with a soft red glow — a tabloid-redaction-style reveal, not a plain title card.
3. **~0:20–0:30, split-screen kinetic caption.** Two video stills side by side, filling the
   left and right halves of the frame independently (a moving truck being loaded on the
   left, a dim interior room on the right), with a large two-line kinetic caption stacked
   bottom-left — the first line in white, the second in yellow — reading as one running
   phrase that continues past what's on screen. A second instance of the *identical*
   structural device appears later in the film: two different stills split left/right, the
   same two-line white-then-yellow caption stacked over them, reading a different phrase.
   This is clearly a **recurring structural device** — the split-comparison-stills-plus-
   kinetic-caption pairing shows up at least twice, not once, which is the strongest single
   signal in this study about what actually carries this channel's "look."
4. **~0:90, plain footage insert.** A tight shot of a Breaking Bad clip with no graphic
   overlay at all in the captured instant — confirming that not every beat gets a caption;
   some moments are just the footage, held.
5. **Somewhere in the first several minutes (exact second unreliable — see the staleness
   note above), a name-and-place card.** A big two-part name in two colours — "ED" in
   yellow, "Galbraith" in white, overlapping — set over a photo-real vacuum-repair-shop
   interior (branded shelving: Hoover, Dyson, Bissell, Miele, Kirby; a "Rental Equipment"
   sign), with a small recurring "Breaking Bad"-chemistry-placard logo bug in the corner —
   this last element reads as the channel's own franchise/episode watermark, not a
   per-scene graphic.

Six confirmed observations, four distinct techniques: a cold-open map-with-pins
establishing shot; a stamped-word title-card reveal; a split-screen-plus-two-colour-kinetic-
caption device used at least twice; and a two-colour name/place card. None of these needed
sound to identify — they're all textual and compositional, exactly the kind of claim this
study can stand behind.

## The captain's hook: the bankroll counter

I did not capture a frame showing it. I want to be exact about what that means and what it
doesn't.

What I *do* have, beyond the captain's own description, is one specific, checkable piece of
audience evidence: a reply comment timestamped at **29:42** in the video (the very end of its
runtime) reads, in full: *"You left out the profits from his vacuum repair shop. He could be
raking in 10s of dollars every year from that"* — 258 likes, several replies. A comment
correcting an omission from a running sum, landing at the video's final minutes, is
consistent with exactly the "bankroll counter" conceit the captain named: the video builds a
running dollar figure across its runtime and a chunk of the audience treats that figure as
a real, arguable total worth correcting line-by-line. That is real, if indirect, evidence the
mechanism exists — it is not proof of *how it's drawn on screen* (persistent HUD in a
corner vs. a periodic full-screen tally card vs. something else), and I'm not going to
describe a widget style I never saw.

Given that honest gap, the rest of this study reasons about **both plausible presentations**
this genre commonly uses for a running total — a persistent corner counter, and a periodic
"tally so far" full-screen card — and shows ShowReel already covers both, rather than
picking one and hoping.

## Technique by technique: what ShowReel would need

Ranked by how much each would change what the captain can make, most to least.

### 1. The bankroll counter (both plausible forms) — **ShowReel already does this**

> **Proof:** [`docs/gallery/counter.gif`](gallery/counter.gif), from
> [`examples/gallery/counter.film.jsonc`](../examples/gallery/counter.film.jsonc)
> — a bankroll ticking up, tabular figures, grouped thousands.

`Content::Counter` (`src/layer.rs`) is a value that eases from `from` to `to` over `over`
seconds, with grouping, decimals, and a prefix/suffix — built for exactly "a dollar figure
that ticks upward," and it already has the one detail that matters for a running-total
counter specifically: **tabular figures**, so the digits don't jitter sideways as they tick
(`README.md`'s "Text as geometry" line; `src/text/font.rs`). Whether the real video holds it
in a fixed corner for the whole runtime or brings it in fresh at each new figure, both are
already authorable:

```jsonc
// A persistent corner counter, re-triggered at each new total by giving
// each stage its own layer, entering when the scene's own moment arrives.
{ "type": "counter",
  "count": { "from": 0, "to": 47250, "over": 1.4, "prefix": "$", "group": true },
  "label": "estimated startup cost",
  "placement": { "anchor": "top-right", "pad": 48.0 } }
```

```jsonc
// A periodic full-screen "tally so far" card: a Solid backdrop, a big
// counter, held for a few seconds, then the scene transitions on.
{ "duration": 4.0,
  "layers": [
    { "type": "solid", "fill": "#0c0c0c" },
    { "type": "counter",
      "count": { "from": 12000, "to": 47250, "over": 2.0, "prefix": "$", "group": true },
      "label": "running total",
      "placement": "centre",
      "enter": { "kind": "scale", "from": 0.7, "duration": 0.4 } }
  ] }
```

The only thing to watch is exactly the caveat already on record in `README.md`'s "Known
limits": if the real figure holds for several seconds once it lands (rather than ticking the
whole time it's on screen), the counter should reach `to` well before the hold ends —
`over` is the *animation* duration, not the on-screen duration, and `Layer::duration` /
`Layer::from` control the latter independently.

### 2. The split-screen + two-colour kinetic caption — **ShowReel already does this**

This is the device with the most evidentiary weight (seen twice, independently), and it
decomposes cleanly into primitives that already exist, per a direct source check
(`src/timeline.rs:125-129`, `src/render.rs:159-166`, `src/layer.rs`): a `Scene`'s layers
draw in `z` order onto one canvas with no coupling between one layer's placement and
another's, so two `Content::Clip`/`Content::Still` layers, each given a `Placement::Frac`
covering its own half of the frame, sit side by side as a literal split screen — no
dedicated "split-screen" `Content` variant needed:

```jsonc
{ "type": "clip", "source": "truck.mp4", "placement": { "fx": 0.0, "fy": 0.0, "fw": 0.5, "fh": 1.0 } },
{ "type": "clip", "source": "room.mp4",  "placement": { "fx": 0.5, "fy": 0.0, "fw": 0.5, "fh": 1.0 } }
```

The two-colour, two-line caption over it needs one honest workaround: `TextStyle` is one
fill for a whole layer's text (`AGENTS.md`'s "Considered, not built" — no rich text runs),
so "As Their" (white) / "Absolute" (yellow) is **two stacked `Text` layers**, not one string
— which the crate already supports cleanly, each with its own staggered-word entrance:

```jsonc
{ "type": "text", "text": "As Their", "placement": { "fx": 0.06, "fy": 0.55, "fw": 0.5, "fh": 0.12 },
  "style": { "fill": "#ffffff", "size": 96, "weight": 800 },
  "enter": { "kind": "words", "stagger": 0.06, "rise": 22.0, "duration": 0.4 } },
{ "type": "text", "text": "Absolute", "from": 0.15,
  "placement": { "fx": 0.06, "fy": 0.66, "fw": 0.5, "fh": 0.12 },
  "style": { "fill": "#ffd147", "size": 96, "weight": 800 },
  "enter": { "kind": "words", "stagger": 0.06, "rise": 22.0, "duration": 0.4 } }
```

### 3. The name/place card — **ShowReel already does this**

`Content::Title` already carries two independently-styled text fields — title and subtitle
— which is exactly the shape of "ED" (yellow, one style) over "Galbraith" (white, a second
style) on a `Still` backdrop:

```jsonc
{ "type": "still", "source": "shop-interior.png" },
{ "type": "title", "text": "Galbraith", "subtitle": "ED",
  "style": { "fill": "#ffffff", "size": 140, "weight": 900 },
  "subtitle_style": { "fill": "#ffd147", "size": 140, "weight": 900 } }
```

### 4. Cold-open map with pin markers — **ShowReel already does this**

The map image itself is a `Still` — the same "prepare the subject-specific asset upstream"
idiom `docs/anarchist-study.md` names for a parallax cutout, since drawing a real map is
squarely "what the film is about" and belongs outside `src/` per the crate's one rule.
But the *pins themselves don't need to be baked into that image at all*: `CalloutSpec`
(`src/layer.rs`) already draws a ring at an arbitrary `target` fraction of the frame with an
optional label — an empty-label callout is just a marker ring, so each pin on the map is one
`Callout` layer, positioned independently, no image-editing round-trip needed for the part
that actually varies per-film:

```jsonc
{ "type": "callout", "target": [0.32, 0.18], "label_at": [0.32, 0.18], "text": "", "ring": 10.0, "accent": "#ffd147" }
```

The desaturated grade on the map is `Grade::documentary()` or hand-tuned knobs, already
built and already the confirmed subject of `docs/anarchist-study.md`'s one real capability
gap from the round before this one.

### 5. The blurred-portrait title stamp — **mostly authoring, one real small gap**

The soft-focus background photo is upstream prep (pre-blur the source image once, the same
"prepare the asset, ShowReel composites it" idiom as #4 above) — a `Still` layer with no
special handling needed. Confirmed absent, though, by a direct source check: **ShowReel has
no per-layer rotation.** `canvas::blur_rgba`/`blur_alpha` exist and are real, but every call
site is either `Presentation::CrossBlur` (a transition between two scenes) or the shadow
blur behind text (`text::draw`) — there is no standing, author-facing "blur this still's own
pixels as a look" filter, and no `Placement`/`Layer` field tilts a layer's content to an
arbitrary angle. So the *diagonal* stamp ("CRIMINAL" banner tilted across the eyes) can't be
authored as a rotated text layer today; the closest honest workaround is baking the whole
stamp graphic (band, glow and tilted word together) into a pre-rendered PNG and placing it
as an ordinary `Still` layer — which works, composes with everything else, and costs nothing
new, but is authoring around a gap rather than the gap not existing. See "Genuinely missing"
below.

## Reading this against the other three studies

This study lands in the same place all three others did, from a new angle: **the pieces of
this look with real evidentiary weight are already built**, several of them (the counter's
tabular figures, `Title`'s two-style name card, `Frac` placement composing into a literal
split screen with zero new render code) built more carefully than a from-scratch
implementation would need to just work. The recurring device here — split screen plus a
two-colour kinetic caption, used at least twice — is, like the anarchist study's fake-depth
screenshot habit, a **compositing habit applied consistently**, not a checkbox feature; nothing
in this study found a rendering capability that doesn't already exist for it. The one
confirmed-absent primitive (per-layer rotation) is small, self-contained, and — like the
anarchist study's colour grade before it was built — the honest last-order item on this list,
not the thing standing between ShowReel and this look.

## Genuinely missing, ranked

1. **Per-layer rotation.** Confirmed absent by source check, not assumed: no `Placement`
   variant or `Layer` field takes an angle; the only place "angle" appears in the type system
   is `Paint::Linear`'s gradient direction, unrelated to laying out a whole layer at a tilt.
   Small, self-contained addition — a rotation angle on `Placement` (or a dedicated transform
   struct wrapping it) that `Layer::draw` applies before compositing, touching no other
   system (camera, transitions, text shaping all stay untouched; a rotated text/image layer
   is drawn to its own buffer and rotated on composite, the same shape `PullUp`'s bitmap-lift
   already uses). Would close the diagonal-stamp technique and anything else that wants a
   tilted lower-third, sticker, or stamp graphic — a small but real, recurring want in this
   genre's title cards. Not urgent: every technique observed in this specific video still
   composes without it, via a pre-rendered stamp asset.
2. **A standing "blur this layer" filter**, distinct from the transition-only `CrossBlur`
   and the text-shadow blur — would remove the "pre-blur the background upstream" workaround
   for a soft-focus portrait. Smaller than #1 and lower priority: the workaround costs one
   image-prep step per asset, not per film, and `canvas::blur_rgba`'s own measured cost
   (~170ms/frame at 1080p, `README.md`'s "Known limits") means a *cheap* standing blur would
   need a different, faster algorithm than what's already in the crate — not just exposing
   the existing one, which argues for leaving this alone until a film genuinely needs it.
3. **Everything else on this list needs no engine work** — the counter (both plausible
   presentations), the split-screen composition, the two-colour name card, and marker
   callouts over a map are all already built to a tested standard. If a ShowReel film in
   this style still doesn't read right once #1 lands, the gap will be in how consistently a
   film author reaches for the split-screen-plus-kinetic-caption pairing specifically —
   authoring/craft, the same conclusion every study in this series reaches, worth naming
   rather than papering over with a new `Content` variant nobody needed.

## The cheapest wins near the counter and chart work

> **Proof of the chart draw-in:** [`docs/gallery/chart.gif`](gallery/chart.gif),
> from [`examples/gallery/chart.film.jsonc`](../examples/gallery/chart.film.jsonc)
> — a plotted function revealing itself left to right. (Still a different job
> from the bankroll counter, as this section argues — the counter has its own
> proof under #1 above.)

The brief asked specifically whether this video does anything the recent chart/counter work
*almost* does but not quite. Two real, cheap observations:

- **`CounterSpec` already has everything a "runs across a whole video, restated at several
  points" bankroll figure needs** — `prefix`/`group`/`decimals` for currency formatting, and
  independent `from`/`to` per instance so a later scene's counter can start from the *previous*
  scene's ending total rather than zero (the second JSON example under "1." above does
  exactly this: `"from": 12000, "to": 47250`). Nothing to build; worth calling out in the
  README's counter section as a named pattern ("a running total across several scenes")
  since it's one line of authoring guidance, not a code change, and it's exactly the shape
  this specific hook needs.
- **`Content::Chart`'s bar-growth reveal is a close but not quite literal match for "a
  bankroll ticking up," and that's fine — they're different jobs.** A chart's reveal sweep
  (`src/chart.rs`) draws a *plotted series* filling in left-to-right; a bankroll counter is a
  single scalar animating its own digits. Reaching for a one-bar `Series::Bars` chart to fake
  a counter would be the wrong tool — `Content::Counter` is already the right one and is not
  missing anything a video like this needs. Named here only because the brief asked, and
  because it's worth being explicit that this is not a gap: it's confirmation the crate
  already has the correctly-shaped primitive for this exact hook.
