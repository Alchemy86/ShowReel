# Internet Anarchist, studied for technique — and what ShowReel would need to match it

The captain named Internet Anarchist specifically: "the transitions and tooling are very
much the display style videos we want to be able to do in ShowReel." This is that study —
technique and tooling, not content. Nothing below quotes, transcribes or reproduces any of
his material; the goal is to name the mechanisms, the same way `docs/remotion-study.md`
named Remotion's primitives and `docs/youcut-study.md` named YouCut's design.

## What this is based on, and what it is not

**I did not watch a full Internet Anarchist video with sound.** This sandbox's browser
automation (`chrome-devtools-axi`) can load YouTube, read page structure and click
controls, but it has no audio pipeline and no speech-to-text — a played video is silent
from this tool's point of view, and none of the pages I opened had captions available
("Subtitles/closed captions unavailable" on every video I checked, first-party and
tutorial alike). So I did not, and could not, watch or listen to his content end to end.
Treat every claim below as resting on one of these, named per-claim:

- **His channel's own public catalogue** — video titles, lengths and view counts, which I
  did browse directly (`youtube.com/@InternetAnarchist/videos`, cookie-walled but
  navigable once the consent dialog is dismissed).
- **Third-party tutorials that reverse-engineer specific shots of his**, published by other
  editors under titles like "Edit Like Internet Anarchist — 3D Screenshot Animation" and
  "— Channel Page Animation" (creator: Mathew Graphic) and "How to Edit like Internet
  Anarchist — Premiere Pro & After Effects" (creator: Matty Love Editing, breaking down a
  specific published video of his). I read these pages' titles, descriptions and comment
  threads, not the tutorial footage itself (again, no audio/transcript access) — so I know
  *what technique each tutorial claims to teach*, not the full step-by-step.
- **One piece of direct, first-party corroboration**: Internet Anarchist commented on the
  Matty Love Editing breakdown himself — "Awesome work Matty! I am looking forward to
  seeing what other breakdowns you do in the future" — which is a real, if thin, signal
  that the breakdown's read of his technique was accurate enough for him to endorse rather
  than correct.
- **General knowledge of the genre** his channel sits in — long-form, narrator-driven
  "internet documentary" essays about creator drama and platform culture, a category that
  also includes the channels his own tutorial-makers and commenters spontaneously compare
  him to (Magnates Media, Alex Hormozi's channel, a creator called "Fern"). Where a claim
  rests on this genre-level knowledge rather than something specific to him, it's marked
  as such below.

Where I say "he does X," read it as *the best-supported account available from outside the
video*, not a frame-by-frame observation. This is a thinner evidence base than the YouCut
study had (that one at least had consistent public reputation and app-store descriptions
to draw on) — I'm flagging that up front rather than writing around it.

## Who he is and what the videos are, concretely

Internet Anarchist is an Australian YouTuber, roughly 1.8–2M subscribers, whose channel is
built entirely around narrated, documentary-style video essays covering the downfall,
controversy or "life falling apart" of other online creators — titles from his own channel
page: *Jack Doherty's Life Is Falling Apart*, *Bobbi Althoff's Life Is Falling Apart*,
*Remember Vitaly? He's Got Worse…*, *The Never-Ending Downfall of iDubbbz*. Runtimes cluster
15–40 minutes. This is **not** an on-camera talking-head format — there is no host in
frame — it's a narrator over an evidence reel: screenshots of tweets, video thumbnails,
chat logs, and clipped footage from the subject's own public output, cut together to
support the narration. That framing matters for everything below: the editing problem he
is solving on every cut is "how do I keep a flat piece of evidence (a screenshot, mostly)
visually alive for the several seconds the narration needs to sit on it," not "how do I
cut between camera angles."

### The signature technique: fake-depth animation of flat evidence

The most concretely documented technique, and the one two independent tutorial-makers
built entire videos around, is turning a **flat screenshot** — a tweet, a channel page, a
thumbnail — into a moving, quasi-3D shot rather than a static insert. One tutorial names it
outright: *"3D Screenshot Animation… using this style you can make more engaging
screenshots, boosting viewer retention."* A second, narrower tutorial applies the same idea
specifically to scrolling through a creator's YouTube channel page as an animated "camera"
move rather than a still capture. The mechanism this implies (standard practice for this
shot type, per the genre generally, since the tutorials describe the *result* rather than
the compositing steps) is: cut the screenshot into 2–3 depth planes — background,
subject, foreground UI chrome — space them apart on a virtual Z axis, and drive a small
camera move (a slow push, or a slight parallax pan) across the stack in a 3D animation
tool. The payoff is a static image that reads as if it has depth and is being looked
*into*, not just displayed.

Both tutorial-makers use After Effects for this, and Premiere Pro for the surrounding cut
— the standard pairing for this genre, and consistent with what the breakdown of a specific
published video also names. **This is what his own tutorial-makers use to replicate the
look; I found no direct statement from Internet Anarchist himself confirming his own
toolchain** — no interview, no software mention in a video description, no pinned comment.
Treat "Premiere Pro + After Effects" as strong inference (it's the default pairing for
essentially every channel in this genre, and it's what two separate people independently
reached for to clone specific shots of his), not a confirmed fact.

### Everything else: genre-level inference, not verified for him specifically

The rest of the "display style" the captain is pointing at — cut rhythm, callouts, sound
design, colour treatment — I could not verify against his output directly (no
audio/transcript access, as above). What I can say honestly is that his channel is grouped,
by his own audience and by other editors, in the same lineage as Magnates Media and
Alex Hormozi's channel — commercially successful, high-production "documentary/explainer"
channels whose shared conventions are well-documented across the wider creator-economy
tutorial space:

- **Punch-ins and camera pushes over stills**, timed to narration beats rather than a fixed
  rhythm — a slow, continuous zoom during a long sentence, a hard cut to a new punch-in at
  a new claim.
- **Kinetic caption/keyword overlays** — a word or short phrase popping onto screen in sync
  with the narrator saying it, usually with a fast, overshooting entrance rather than a
  plain fade, then cutting away rather than animating back out.
- **Callout/annotation graphics over evidence** — a circle, arrow or box drawn onto a
  screenshot to point at the specific detail the narration is currently describing, often
  paired with the evidence lifting slightly off the frame toward camera.
- **Stinger sound effects on cuts** — a whoosh, a click, or a low-end thump timed to a hard
  cut or a punch-in, doing the job a visual transition would otherwise do.
- **A driving instrumental bed under the narration**, ducked under speech and swelling at
  narrative beats, rather than silence-with-narration-only.
- **A slightly desaturated, contrast-boosted grade** — the visual signature of "serious
  documentary" tone that this whole channel category leans on, as distinct from a bright,
  neutral vlog grade.

**None of the six points above are things I verified against an actual Internet Anarchist
frame.** They are the well-established conventions of the specific genre his channel
occupies, named because the captain's brief was explicit that "the display style" is the
target, and because the tutorial ecosystem around him repeatedly frames his work as
belonging to that same genre rather than doing something else. I'm listing them because a
capability study that only covered the one technique I could triangulate (fake-depth
screenshots) would understate what "the Internet Anarchist look" actually asks for — but
every one of them should be re-verified against his real output the moment someone on the
crew can actually watch a video with sound, rather than taken as settled from this pass.

## Reading this against the other two studies

- **Remotion study**: concluded ShowReel's animation primitives (`interpolate`-equivalent
  easing, springs, presentation×timing transitions) are already a match or better than a
  React-based renderer's. Nothing here contradicts that — if anything, this creator's
  reliance on punchy, overshooting text pops and camera pushes over stills is squarely
  inside what `ease::Easing::OutBack`/`OutElastic` and `camera.rs`'s geometric zoom were
  already built for.
- **YouCut study**: its central finding was that YouCut's "obvious" reputation is a design
  property (recognition over recall, direct manipulation, one-tool-one-screen) rather than
  a feature list, and that the same is true in reverse for our own editor's gaps
  (thumbnails, stage sizing). This creator's study lands on the same shape of conclusion
  from the opposite direction: **the "Internet Anarchist look" is also mostly not a feature
  list.** The one concretely documented technique (fake-depth screenshot animation) is a
  compositing *habit* — always split flat evidence into depth planes before it goes on
  screen — applied with total consistency across (per his catalogue) hundreds of videos,
  not a one-off effect. Consistency of application, not availability of a checkbox, is the
  actual mechanism, the same conclusion the YouCut study reached about "obvious."

## Technique by technique: what ShowReel would need

Ranked by how much each would change what the captain can make, most to least.

### 1. Fake-depth ("parallax") animation of a flat still — **ShowReel could nearly do this**

> **Since built — see the proof.** This verdict is now stale: `Content::Parallax`
> was built the round after this study. The gallery has it running —
> [`docs/gallery/parallax.gif`](gallery/parallax.gif), from
> [`examples/gallery/parallax.film.jsonc`](../examples/gallery/parallax.film.jsonc).

This is the one technique with real evidentiary weight behind it, so it's the one worth
being most precise about. ShowReel has no dedicated "parallax still" feature, but it has
every primitive the shot actually needs, because a parallax shot is nothing but several
independent camera moves at different rates stacked on top of each other:

- A `Scene` already holds an ordered stack of `Layer`s (`src/layer.rs`), each an
  independent `Content::Still` or `Content::Clip` with its own `Camera`.
- `Camera` (`src/camera.rs`) already does geometrically-interpolated pan/zoom over a
  mip-backed still, keyframed by `Shot`s — exactly the "push slowly across this plane"
  half of the shot.
- A still with transparency (a cut-out subject with alpha) layered over a background still,
  each given its own `Camera` moving at a different rate, *is* a parallax composite —
  ShowReel already composites layers with alpha and already drives each one's camera
  independently.

What's missing is entirely on the authoring side, not the renderer: nothing turns one
screenshot into 2–3 depth-plane assets automatically (that's a cutout/rotoscoping problem,
not a video-timeline problem — the same category of work Photoshop or an AI background-
remover does, upstream of ShowReel), and there's no single `Content` variant that says "one
source image, N depth planes, one relative-parallax camera" — a person would hand-build it
today as 2–3 separately-authored `Still` layers with hand-tuned `Camera`s, which works but
is fiddly. **Worth building**: a thin `Content::Parallax` (or a camera-linking helper) that
takes N image layers plus a single camera move and derives each layer's own offset as a
fraction of the main move (nearer planes move more) would close this cheaply, without
touching the render core — it's a composition convenience over primitives that already
exist, not a new rendering capability. The actual image-splitting-into-planes step stays a
pre-production task outside the crate's rule ("nothing in the crate may know what its films
are about" — a depth-plane cutout is unavoidably about the specific image).

### 2. Punch-ins and camera pushes over stills and clips, timed to narration — **ShowReel already does this**

> **Proof:** [`docs/gallery/camera.gif`](gallery/camera.gif), from
> [`examples/gallery/camera.film.jsonc`](../examples/gallery/camera.film.jsonc)
> — a push over a still far larger than the frame.

Exactly `Camera`'s job (`src/camera.rs`): keyframed `Shot`s with geometric zoom
interpolation, described in seconds and resolved to frames. The one caveat already on
record in `AGENTS.md`: a *clip's* camera (as opposed to a still's) isn't mip-backed, so
pushing in tight on footage needs `max_width` raised to match or the image goes soft — a
known, documented limitation, not a gap in this study.

### 3. Kinetic caption/keyword pop-ins synced to narration — **ShowReel already does this**

> **Proof:** [`docs/gallery/captions.gif`](gallery/captions.gif), from
> [`examples/gallery/captions.film.jsonc`](../examples/gallery/captions.film.jsonc)
> — words landing one at a time with a spring overshoot (`Motion::Words` +
> `Easing::OutBack`).

`Motion::Chars`/`Words` (per-character or per-word staggered entrance,
`src/motion.rs`) combined with `Easing::OutBack`/`OutElastic`/springs (`src/ease.rs`) is
precisely the "word lands with a snap, slightly overshooting" motion this genre uses for
on-screen keywords. `Title`/`LowerThird`/`Text` all take this motion. The one real
limitation is the one already named and deliberately declined in `AGENTS.md`'s "Considered,
not built" section: a caption where *only one word* changes weight or colour mid-line
(rich text runs) isn't supported — every layer is one `TextStyle` for its whole string.
That gap is orthogonal to this creator's technique, though: the pop-in-per-word effect
above doesn't need mixed styling within a single frame, only staggered timing across
whole words, which is already there.

### 4. Callout/annotation graphics over evidence — **ShowReel already does this**

> **Proof:** [`docs/gallery/callouts.gif`](gallery/callouts.gif), from
> [`examples/gallery/callouts.film.jsonc`](../examples/gallery/callouts.film.jsonc)
> — a ring on a target, a leader line, a label.

`CalloutSpec` (`src/layer.rs`) is a ring drawn at a target point plus a connected label —
the "circle the detail, draw a line to an explanation" idiom this genre uses constantly
over screenshots. `PullUpSpec` (same file) is the more elaborate cousin: lift a region of
whatever is already on screen, dim the rest, optionally draw a tether line back to where it
came from — described in its own doc comment as "the explainer idiom the captain described:
highlight, enlarge, pull forward, annotate," and it works over any layer without knowing
what that layer contains, which is exactly the constraint this study operates under too.

### 5. Stinger sound effects timed to cuts — **ShowReel already does this**

`Audio` (`src/audio.rs`) places any audio file at an exact film-time `at`, with its own
gain and fades — a one-shot whoosh or click dropped at a hard cut's exact frame is not a
different code path from a music bed, just a short file placed precisely. Nothing here
needs new capability; it needs someone to author the film JSON with a `Audio` entry per
stinger, the same way any other sound gets placed.

### 6. A driving music bed, ducked under narration — **ShowReel already does this**

Gain, fades and `amix` mixing are already a tested pipeline (`src/audio.rs`,
`AGENTS.md`'s audio section) — ducking a bed under a narration track is exactly what
per-track gain plus fades already exists for. No missing capability; this is authoring,
same as #5.

### 7. Colour grade (desaturated, contrast-pushed "documentary" look) — **ShowReel cannot do this**

> **Since built — the verdict below is stale.** This was true when written; the
> colour grade it identified as the one real gap was then built (`src/grade.rs`,
> `Grade::documentary()`). The gallery shows it before-and-after —
> [`docs/gallery/grade.gif`](gallery/grade.gif), from
> [`examples/gallery/grade.film.jsonc`](../examples/gallery/grade.film.jsonc).
> The original finding is kept below because a study is only worth reading if it
> records what was actually true at the time.

Confirmed absent by reading the source, not assumed: there is no global grade, LUT,
saturation, contrast or vignette pass anywhere in `src/` — `canvas.rs` composites and
blurs, but nothing tone-maps a finished frame. Every other capability in this list already
existed somewhere in the crate before this study; this is the one clean miss.

**Judgement on whether it's worth building**: yes, but it's small and it's the last thing
on this list to build, not the first. A single post-composite pass — lift/gamma/gain or a
simple contrast+saturation adjustment, applied once per frame after everything else is
drawn, the same place a vignette would hook in — would cover the "serious documentary"
look this whole genre leans on, and it's a self-contained addition (one more stage in
`render.rs`'s per-frame pipeline) that doesn't touch text, camera or transitions. It's
ranked last here because, per the "watch for the trap" framing below, it's the smallest
lever of everything in this list for how much it would change what the captain can
actually make — the fake-depth screenshot technique and the consistency of applying the
existing punch-in/callout/caption toolkit matter far more to "looking like this genre" than
a LUT does.

## The trap, and why this study lands on craft over capability

The brief warned against a list of missing effects that wouldn't actually get the captain
there. Having gone through the technique list above, that's almost exactly what happened:
**six of the seven techniques ShowReel needs for this look are already built**, several of
them (camera, callouts, staggered text, audio mixing) built to a level of polish — real
easing curves, real springs, a tested audio pipeline — well past what a from-scratch
implementation would need to just barely work. The one thing that's actually missing
(colour grade) is real but small, and the one thing that's partially missing (an ergonomic
parallax composite) is a convenience layer over primitives that already exist, not new
rendering machinery.

What separates "a ShowReel film" from "an Internet Anarchist video," on the evidence
available here, is not a checklist gap — it's that his channel applies one technique (split
flat evidence into depth planes, animate the camera across it) with total, unbroken
consistency across an entire catalogue, timed tightly to narration, while ShowReel's own
redesign work (`docs/youcut-study.md`) has been busy making the *editor* more obvious to
use, not enforcing any particular *visual discipline* on what gets made with it. That's the
same conclusion the YouCut study reached from the interface side: the reputation is a
design decision applied consistently, not a feature. Here it's a compositing decision
(depth-plane evidence, always) applied consistently, not a render feature. **Building the
missing grade pass and the parallax convenience closes the remaining capability gap; only
someone actually cutting a film with the discipline of always splitting a screenshot into
planes and timing every push to the voiceover would close the craft gap, and no amount of
new `Content` variants substitutes for that.**

## Recommendations, ranked

1. **A parallax/depth-plane composition convenience** (`Content::Parallax` or a
   camera-linking helper over ordinary layered `Still`s) — the only technique here with
   strong evidence behind it and no direct ShowReel equivalent yet, though every primitive
   it needs (layered alpha stills, independent per-layer `Camera`s, geometric zoom) already
   exists. Highest leverage of anything on this list because it's the one concretely
   documented signature move.
2. **A single post-composite grade pass** (lift/gamma/gain or contrast+saturation, one
   stage in `render.rs`) — the one clean, confirmed-absent capability. Small, self-
   contained, worth doing, but genuinely last-order compared to #1: a correctly-graded
   film with no parallax evidence shots looks less like this genre than an ungraded one
   with them.
3. **Everything else on the technique list needs no engine work** — camera pushes,
   kinetic captions, callouts/pull-ups, stinger SFX and a ducked music bed are all already
   built to a tested standard. The gap there, if the captain finds a ShowReel film still
   doesn't read like this genre once #1 and #2 land, will be in how consistently a film
   author reaches for those existing tools — an authoring/craft question, not a missing
   feature, and worth naming plainly rather than inventing new capability to paper over.
4. **Re-verify the genre-level claims** (cut rhythm, exact caption timing, grade specifics,
   sound design pattern) against real Internet Anarchist footage the first time someone on
   the crew has audio-capable browser access or a way to watch with sound — this study's
   weakest evidence is exactly the six conventions in the "genre-level inference" section,
   and they're doing real work in the recommendations above.
