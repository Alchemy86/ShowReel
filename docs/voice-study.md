# Speech, studied against generated music — what ShowReel would need

The captain's question, verbatim: *"What can we do to create clear, human sound easily. Like
you mentioned before the villains voice on dungeon soup is actually made with elevenlabs or
something. And Geminai talking sounds clear and human too. What can we do to add speaking to
our video platform."* This is that study: technique, licensing and architecture, not code —
a decision document, the same job `docs/anarchist-study.md` and `docs/remotion-study.md` did
for their briefs.

## Why this is read against the music work specifically

`src/music.rs` just closed the "a film is a readable file you commit" gap for a soundtrack: a
`Music` spec resolves to a WAV exactly where a file `asset` resolves to a path, it's pure Rust
so it runs natively and in wasm, and its one real trick — `MusicFit::Film` nudging the tempo so
a whole number of bars spans the film — is a genuine case of *generation timed to the film's own
clock*, not audio bolted on after the fact. Narration is the obvious next line item, and the
brief is right that the same self-contained argument applies to it. But the two problems are not
the same shape, and the difference is the whole substance of this study:

- **Music's hard part was arithmetic.** Three simple voices (a pulse bass, a pulse lead, noise
  drums) were already "good enough" — chiptune is a genre defined by its limitations, so a small
  synth faithfully reproduces it. The actual engineering was fitting a beat grid to a duration
  that's known in advance.
- **Speech's hard part is the opposite of arithmetic.** "Clear, human sound" is not a genre with
  forgiving limitations — it is the frontier problem the two products the captain named
  (ElevenLabs, Gemini) exist to solve, and neither is a weekend's DSP. Nobody is writing a
  competitively human-sounding neural TTS model from scratch for this crate, the way three
  oscillators were enough for a chiptune. That single fact reshapes every recommendation below:
  this study is about *which existing engine to call and how*, not about porting a synthesis
  algorithm the way `chiptune.py` was ported.

## What I verified versus what I'm inferring

I read `src/audio.rs` and `src/music.rs` in full before writing anything below, and I did not
design against a blank page — every claim about ShowReel's own architecture in this document is
read directly from source, cited by file and line. For the external landscape:

- **Verified by fetching the primary page or terms document directly**: ElevenLabs' pricing
  tiers and per-credit cost (`elevenlabs.io/pricing`), its commercial-use/free-plan/attribution
  terms (`elevenlabs.io/terms-of-use` via search-indexed quotes plus the official help-centre
  article "Can I publish the content I generate on the platform?"), its timestamp/forced-alignment
  API (`elevenlabs.io/docs/api-reference/text-to-speech/convert-with-timestamps` and
  `.../forced-alignment/create`), Google's Gemini API terms on output ownership
  (`ai.google.dev/gemini-api/terms`), the Gemini TTS model list and per-model token pricing
  (`ai.google.dev/gemini-api/docs/pricing`), SynthID watermarking being applied automatically to
  Gemini audio output (Google's own `blog.google` and `deepmind.google` posts), the GNU project's
  own FAQ answer on whether a GPL program's *output* is covered by the GPL
  (`gnu.org/licenses/gpl-faq.html`), and the Piper voice weights' MIT licence as stated on their
  Hugging Face model card (`huggingface.co/rhasspy/piper-voices`).
- **Verified by actually running it, on this machine, right now** — the strongest evidence in
  this study and the one thing no pricing page can tell you: Piper is already installed here
  (`~/.local/bin/piper`, `piper-tts` 1.6.0), and I fetched a real voice model, synthesised real
  audio, and measured real timing and real (non-)support for word alignment. See the "Local
  engines, measured" section — every number there is a command I ran and its output, not a
  number I read.
- **Inferred / secondary-sourced, flagged inline where it matters**: Kokoro's benchmark numbers
  (MOS scores, CPU realtime-factor claims) come from third-party benchmark blogs rather than a
  primary Hugging Face model card or my own run — I did not install or run Kokoro in this
  session, for the plain reason that Piper was already present and answered the load-bearing
  questions (licence clarity, offline operation, alignment API) on its own; a full head-to-head
  listening test of Kokoro against Piper is real follow-up work, not done here. ElevenLabs' exact
  effective per-1,000-character rate also varies slightly across sources ($0.05–$0.20/1,000
  chars depending on plan tier and model — Flash/Turbo vs Multilingual v2/v3); I've given the
  range and shown the arithmetic from the official credits table rather than picking one figure.

## The commercial APIs the captain named

### ElevenLabs

Verified from `elevenlabs.io/pricing`: paid tiers run Free ($0, 10,000 credits/mo) → Starter
($6, 30,000) → Creator ($22, 121,000) → Pro ($99, 600,000) → Scale ($299, 1.8M) → Business ($990,
6M) → Enterprise (custom). Text-to-speech spends roughly 1 credit per character, so per-1,000-
character cost falls from about $0.20 on Starter to about $0.165 on Pro/Scale/Business as you buy
in bulk — cheaper "pay-as-you-go" API rates as low as $0.05–$0.10/1,000 characters are quoted for
the fast Flash/Turbo model specifically, separate from the higher-quality Multilingual v2/v3
models. Billing is on *characters sent*, not audio duration, and a 500-character paragraph costs
the same whether the resulting clip is faster or slower.

**Licence position — this is the part that matters most for the captain's actual question.**
Verified: the free plan's outputs **cannot be used commercially and must carry attribution**
("elevenlabs.io" or "11.ai") whenever shared, even non-commercially. A paid plan (Starter and up)
grants commercial rights to audio generated *during* an active subscription, and you keep those
rights on audio already generated even if you later downgrade — but new generations made after a
downgrade revert to the free-tier restriction. In short: **clears the bar for a public GitHub
Pages blog only while a paid plan is active**, and the cheapest tier that clears it is $6/month
for about 30,000 characters. That's an ongoing subscription dependency for a project whose whole
music story this month was "stop gambling on licence and stop depending on a subscription."

ElevenLabs also ships a genuinely useful **timestamp API**: `/v1/text-to-speech/:voice_id/with-
timestamps` returns character-level start/end times per generated clip, and a separate **Forced
Alignment** endpoint (`/v1/forced-alignment`) takes *any* existing audio plus its transcript and
returns the same alignment — including audio that was not generated by ElevenLabs at all. That
second endpoint is a real, if paid, back door to word-level timing for a locally-synthesised
voice; noted here because it resurfaces in the timing section below.

### Google — Gemini's TTS, not Google Cloud's older Text-to-Speech product

The captain's "Geminai" is almost certainly the native-audio Gemini models (2.5 Flash/Pro
Preview TTS, and a newer 3.1 Flash TTS Preview), which are a different product from Google
Cloud's long-standing WaveNet/Neural2 Text-to-Speech API. Verified pricing from
`ai.google.dev/gemini-api/docs/pricing`: Gemini 2.5 Flash Preview TTS is $0.50/M input text
tokens + $10/M output audio tokens on the free-tier-adjacent rate, or $0.25/$5 per million on the
paid batch rate; Gemini 2.5 Pro Preview TTS is $1.00 input / $20.00 output per million tokens
standard, $0.50/$10.00 batch; Gemini 3.1 Flash TTS Preview matches the Pro-tier rate at
$1.00/$20.00 standard. Audio tokens correspond to roughly 25 tokens per second of audio, so a
one-minute narration track (~1,500 audio tokens) costs on the order of $0.02–$0.03 at the
2.5 Flash rate purely in output-token terms — genuinely cheap per-render, in the same
neighbourhood as ElevenLabs' bulk rate and cheaper than its entry tier.

**Licence position, verified from `ai.google.dev/gemini-api/terms`**: Google explicitly disclaims
ownership of your generated output ("Google won't claim ownership over that content") and permits
commercial use through the Paid Services tier (an active Cloud Billing account). There is no
ElevenLabs-style attribution requirement in the terms. The real catch, also verified (Google's
own `blog.google`/`deepmind.google` posts on the 2.5 and 3.1 TTS launches): **every audio output
from these models carries an automatic SynthID watermark, imperceptible but always present, with
no opt-out.** That's not a licence blocker — the terms don't forbid public distribution — but it
is a fact worth putting in front of the captain before he ships a "dungeon soup villain" voice
built on it: the audio is permanently, silently tagged as AI-generated by Google's own detector,
which is a different property than a locally-synthesised WAV has. Gemini TTS also has, verified
by its own docs, **no word-timestamp or timepoint output of any kind** — it's a black-box WAV in,
nothing but audio out, unlike ElevenLabs' with-timestamps endpoint or the classic Google Cloud
TTS product's SSML `<mark>`/timepoint mechanism (which *does* support per-word timepoints, but is
a different, older product than what "Gemini talking" refers to).

**Bottom line on the two commercial options**: both clear the "can I publish this on a public
site" bar in plain English — Gemini more cleanly (no subscription-dependent rights, no
attribution clause) but with a permanent watermark and zero timing hooks; ElevenLabs with real
per-word timing available but commercial rights that lapse the moment the subscription does.
Neither is a *credential-free* story: both need a live network call and an API key at render
time, which is exactly the class of problem the brief calls out as already having burned this
estate twice this week.

## Local engines, measured

This is where "a local model that is merely acceptable may beat a cloud one that is excellent"
gets tested against this actual machine rather than a marketing page.

### Piper — installed, run, and measured here

`piper` is already on this machine's `PATH` (`~/.local/bin/piper`, package `piper-tts` 1.6.0,
`pip3 show piper-tts` confirms `Home-page: http://github.com/OHF-voice/piper1-gpl`). A voice
model was already cached at `~/.local/share/piper-voices/en_GB-alba-medium.onnx` (60.3 MB ONNX +
a small JSON config). I ran it for real:

```
$ time piper -m en_GB-alba-medium.onnx -f sample.wav -i sample.txt
13.58s user 0.29s system 1433% cpu 0.968 total
$ ffprobe -show_entries format=duration sample.wav
duration=8.986122
```

138 characters of input text → 8.99 seconds of audio, synthesised in 0.968 seconds wall-clock on
this 20-core machine (`nproc` → 20) using ~14 threads in parallel — a real-time factor of about
**9.3×**. A second, shorter run through the Python API alone (no process-spawn or model-load
overhead) hit **31×** real-time: 0.117s to synthesise 3.65s of audio. Model load itself was well
under a second (0.87s). None of this needs a GPU; `--cuda` exists as an option but was not
touched. This confirms the general reputation (Piper targets a Raspberry Pi 4 as a baseline
device) with an actual number on actual hardware rather than a doc figure.

**Licence — this is a genuinely more tangled story than "MIT" or "GPL", and worth getting exactly
right rather than picking the convenient half.** The original engine, `rhasspy/piper`, was MIT
licensed and is now archived (went read-only in October 2025). Active development moved to
`OHF-Voice/piper1-gpl` — the Home Assistant project's fork — under **GPL-3.0-or-later**, and that
GPL fork is what `pip install piper-tts` actually installs (confirmed above: `License:
GPL-3.0-or-later` is what's on this machine right now). The **voice weights are a separate
artifact from the engine** and remain MIT-licensed on their Hugging Face card
(`huggingface.co/rhasspy/piper-voices`, verified). GPL-3.0 is a licence on the *program*, not
automatically on what it produces: the GNU project's own FAQ, verified directly, states plainly
that "the output of a program is not, in general, covered by the copyright on the code of the
program," with the one carve-out being when the program's *own* creative content — its art, its
music, its authored words — ends up embedded in the output. A voice's timbre lives in the
separately-MIT-licensed model weights, not in the GPL'd inference code, so the strongest reading
is that synthesised speech is unencumbered either way. But there is no need to lean on that
reading at all, because ShowReel already has prior art for exactly this situation: **it already
shells out to `ffmpeg` as an external process** (`src/encode.rs`), and a stock ffmpeg build
commonly includes GPL components without that making ShowReel's own crate GPL — invoking an
external GPL binary as a subprocess, rather than linking it, is the established pattern this
project already relies on. Two ways to get to zero ambiguity, cheapest first: (a) pin to the
frozen MIT-licensed original `rhasspy/piper` release — it no longer receives updates, but for a
project that already values deterministic, reproducible renders, a tool that has stopped moving
under you is arguably a feature, and it sidesteps the whole GPL question by construction; or
(b) use the current `piper1-gpl` build (what's already installed here, actively maintained, and
the one with the alignment API described below) as a subprocess exactly the way `ffmpeg` already
is. Either is publishable on a public GitHub Pages blog with no subscription and no attribution
line. I'd default to (a) for the same "don't gamble" instinct that produced the music work, and
only reach for (b) if a specific voice or the alignment feature (below) is worth the very small
residual ambiguity.

**Word-level timing — real, but conditional, and I tested this rather than assumed it.** The
piper1-gpl Python API's `PiperVoice.synthesize()` takes an `include_alignments=True` flag and,
per its own docstring, returns per-phoneme alignment "if the model supports it." I called it:

```python
chunks = list(voice.synthesize(text, include_alignments=True))
chunks[0].phoneme_alignments  # -> None
```

Reading the source (`piper.voice.PiperVoice.phoneme_ids_to_audio`) explains why: the ONNX model
must expose a *second* output tensor (per-phoneme duration, from a duration predictor) for
alignment to be extractable at all, and the one voice already cached on this machine — a
standard "medium" quality export — only exposes the audio tensor, so the call silently and
correctly returns `None` rather than fabricating a timestamp. The installed `piper` CLI doesn't
expose this at all (its `--help` has no alignment flag; an `--alignment-data` flag exists only in
old PRs against the now-archived C++ `rhasspy/piper`, a different codebase). **The honest
conclusion: Piper's newer engine has real, first-party phoneme-alignment machinery, but it is not
yet a property of the general voice catalogue** — it needs voices exported with the extra output
head, which is not (yet, as far as this session found) the default across the community's
existing `.onnx` files. This is the single most important "what it would take" finding in this
whole study for the timing question, and it's a gap I verified by hitting it, not one I inferred.

### Kokoro — not run this session, worth a real listening pass before deciding

Apache-2.0 licensed, ~82M parameters (~327MB of weights), 54 voices across 8 languages,
maintained on Hugging Face by `hexgrad`. Secondary benchmark sources report a Mean Opinion Score
around 4.45 and roughly 6× real-time on a CPU with no GPU — both plausible and both consistent
with the general "small model, StyleTTS2 lineage, punches above its parameter count" reputation
it has, but **I did not install or run it here**, so treat those two numbers as inferred rather
than measured, unlike every Piper number above. Kokoro's licence is the cleanest of anything in
this study — Apache-2.0 top to bottom, no separate voice-weight question, no GPL fork history —
and it's worth a real side-by-side listening test against Piper before choosing between them; I'd
weight that test on ear, not benchmark score, given how close a call it looks from outside.

### Coqui / XTTS — a licence trap, named so it doesn't get picked by accident

This is the model most tutorials point to for voice cloning, and it is the wrong one for this
project. XTTS v2's weights are released under the **Coqui Public Model License (CPML)**, which is
**non-commercial only**. Coqui the company shut down in January 2024; a 2023 post mentioned a
paid commercial licence path at $365/year, but there is now no company, no sales team, and no
portal to buy one — so "non-commercial only, with literally no route to a commercial licence at
any price" is the permanent state of this model, not a temporary gap. This is exactly the shape
of trap the brief is worried about: a model that sounds excellent, shows up first in every
tutorial, and is quietly unusable on a public site. **Do not build on it.**

## The licence verdict, plainly

| Option | Clears "publish on a public GitHub Pages blog" | Cost/ongoing dependency |
|---|---|---|
| ElevenLabs | Yes, but **only while a paid plan stays active** — rights lapse on downgrade | Subscription, from $6/mo |
| Gemini TTS | Yes, no attribution clause — but **every clip is permanently SynthID-watermarked** | Pay-per-use, ~$0.02–0.03/min at Flash rate |
| Piper (frozen MIT original) | Yes, unambiguously, forever | None — one-time download, runs offline |
| Piper (current `piper1-gpl`) | Yes in substance (GPL doesn't reach program output; subprocess use already precedented by ffmpeg) | None — one-time download, runs offline |
| Kokoro (Apache-2.0) | Yes, unambiguously, forever | None — one-time download, runs offline |
| Coqui / XTTS v2 | **No** — non-commercial licence, no purchase path exists | N/A — do not use |

## The design question: how would narration be authored?

`Audio.music: Option<Music>` (`src/audio.rs:74-79`) is the direct precedent: a track is *either*
a file `asset` or generated `music`, and from `Audio::resolve` on it's a plain located WAV — the
same fades, gain and mixing machinery, no special case downstream (`src/audio.rs:211-226`,
`src/timeline.rs:448-469`). Narration fits the same slot for the same reason: the crate rule is
that nothing in `src/` may know what a film is *about*, but the words a narrator says are
unavoidably about the film, exactly the way a `Title`'s string already is — so putting the actual
line of dialogue in the film JSON is not a violation, it's consistent with every other text
content kind already in `src/layer.rs`.

The natural shape, mirroring `Music`'s bare-word-or-object idiom (`src/music.rs`'s module docs,
"`\"music\": \"funk\"`… or an object for full control"):

```jsonc
// shorthand — the common case
"speech": "In 1996, a game changed everything."

// full control
"speech": { "text": "In 1996, a game changed everything.", "voice": "en_US-ryan-high" }
```

Resolved by a new `Speech::render_to_temp` sitting exactly beside `Music::render_to_temp` in the
same `resolve_audio_tracks` match arm (`src/timeline.rs:460-465`) — content-addressed cache key
of `(text, voice, engine version)` in place of Music's `(spec, duration, SYNTH_VERSION)`, same
reasoning: re-rendering the same film should reuse the WAV, and a changed line should never
silently serve stale audio. Where `Music::render_samples` is pure Rust with **no** filesystem or
subprocess (`src/music.rs`'s doc comment: "Pure and deterministic — no filesystem, no ffmpeg —
so it compiles and runs on wasm32 as readily as natively"), a `Speech` spec's resolver would need
to shell out — to Piper, exactly the way `src/encode.rs` already shells out to `ffmpeg` — because
nothing in this crate is going to out-synthesise Piper/Kokoro/ElevenLabs from scratch. That
subprocess dependency is the one real architectural difference from Music, and it's the reason
the wasm answer below is different too.

### Timing — genuinely the interesting part, and genuinely harder than music's version

Music's timing trick worked because the *film's duration was already fixed* and the tempo bent to
fit it (`MusicFit::Film`, `src/music.rs:effective_bpm`). Speech inverts that relationship: **you
cannot know how long a sentence takes to say until you've synthesised it** — there's no equivalent
of "nudge the tempo" for prose without time-stretching audio into something that stops sounding
human, which defeats the entire point of this study. So two different, separable questions:

1. **Can a caption pop in sync with the spoken word?** Concretely buildable, and the highest-value
   thing in this whole study. Today, `Motion::Words`/`Motion::Chars` (`src/motion.rs:31-33`)
   stagger words apart by a fixed, uniform interval — it has no notion of *actual* per-word timing,
   only an authored guess at rhythm. Real word-synced captions need an irregular list of
   `(word, start_time)` cues instead of a uniform stagger, and — this is the part only this
   session's own testing settles — there are three real sources for that list: Piper's own
   alignment API (real, but conditional on voice export, per above), a local forced-aligner run
   over the finished WAV (Montreal Forced Aligner or WhisperX, both real open tools, run entirely
   offline, no licence question, and MFA in particular is reported to out-perform WhisperX on raw
   word-boundary accuracy), or ElevenLabs' Forced Alignment endpoint if the captain is already
   paying for ElevenLabs for the voice itself. Any of the three produces the same shape of output:
   a small per-word timing table the layer content can consume. The clean addition is a new
   `Motion` variant — call it `Motion::Cues { words: Vec<(String, Time)> }` — that a caption layer
   reads instead of guessing a stagger, populated either by hand (an author who timed it by ear)
   or by a companion step that turns an alignment file into that same shape. This does **not**
   require solving question 2 below to be worth doing on its own.

2. **Can a cut land when a sentence ends, or a shot hold until the line finishes?** This is the
   harder one, and I'd rather say so plainly than half-solve it. `Film::duration()` today is built
   bottom-up from author-declared `Scene`/`Timeline` durations (`src/timeline.rs`), and
   `resolve_audio_tracks` runs *after* that shape is fixed, using it as an input
   (`self.duration()` at `src/timeline.rs:452`) — the same order Music's resolver relies on to
   know how long to render. Letting a scene's duration instead be *derived from* how long its own
   narration took to synthesise means resolving speech **before** scene/timeline duration is known,
   which is a real ordering change to `Film`'s resolve pipeline, not a new field. It's exactly the
   shape of thing `AGENTS.md` already names as deliberately deferred for `MusicFit` ("the general
   per-cut-time solver is a documented next step, not half-built") — and I'd make the same call
   here: name it as the real prize, and leave it unbuilt rather than land a version that only
   half-derives cut timing and quietly breaks the moment an author's scene declares a duration that
   disagrees with what the narration actually needs.

### WASM — an honest answer, and it's "desktop-only," which the brief already said was acceptable

A cloud API call (ElevenLabs or Gemini) inside the pure wasm render path is worse than merely
impossible — it's the wrong shape of impossible, because doing it would mean embedding a live API
key in client-side JavaScript, handed to every visitor of a public page. That's precisely the
credential-exposure class of mistake the brief names as something this estate has already been
burned by twice this week; it should stay off the table regardless of whether a browser fetch is
technically reachable from wasm.

A local model is the *architecturally* right shape but the *practically* wrong size for this
project's existing wasm story. Compare: the chiptune compiles into wasm because
`Music::render_samples` is genuinely pure — no filesystem, no subprocess, a few hundred lines of
DSP (`src/music.rs`'s own doc comment says this explicitly). A neural TTS model is the opposite of
that: Piper's smallest voices are tens of MB, "high" quality voices and Kokoro's weights run to
hundreds of MB, and running either needs an ONNX/tensor runtime compiled into the wasm binary —
none of which resembles the ~2MB, `wasm-opt`-tuned renderer binary this project has been careful
to keep small (`AGENTS.md`'s wasm sharp edges). This is the same shape of gap `.srclip` already
solved for video: **a clip can't be decoded live in a browser sandbox, so `showreel web-pack`
pre-decodes it natively and ships a snapshot.** Narration should follow exactly that precedent —
synthesise the WAV (and, if available, its word-cue table) natively at `web-pack` time, list it
in a manifest the way `music.json` and `clip-audio.json` already work
(`AGENTS.md`'s "Generated music needs the wasm blob rebuilt" and "Browser playback has sound"
entries), and let the browser's existing separate Web Audio graph (`tools/web/audio.js`) play it
back like any other pre-rendered cue. **The honest framing the brief invited: narration generation
is desktop-only, exactly like ffmpeg-backed clip decode already is** — not a gap, a category this
project already has a working answer for.

### Does this belong in the crate, or as a pre-step that hands the film a WAV?

Music earned its place in `src/` because generation could be *timed to the film* while staying
pure Rust — no new external dependency, and a real, load-bearing coupling (bar-aligned cuts) that
only works if the synthesiser and the timeline share one clock. Speech only partially clears that
bar today: word-synced captions are a real, timeline-aware coupling worth building *in* the crate
(section above), but the harder coupling — cut timing genuinely derived from narration length —
isn't built yet anywhere, in this study or otherwise, so it can't be the argument for landing
speech generation *now*. What's left is a plainer case: bringing `Speech` in as a resolved-asset
idiom (parallel to `Music`, a subprocess call instead of pure synthesis) keeps the film file
readable and diffable — `"speech": "the line to say"` is genuinely more reviewable in a PR than a
checked-in binary WAV would be, the same self-contained argument that motivated Music — without
overselling a cut-timing story that isn't real yet. I'd build it for that reason: the readability
win is real even before the harder timing win is, and it costs nothing that ffmpeg-shelling
doesn't already cost this crate. What I would *not* do is wire ElevenLabs or Gemini in as a
first-class in-crate resolver — see the recommendations below for why.

## Recommendations, ranked

1. **Land a `Speech` spec resolving through a local, unambiguously-licensed engine — Piper,
   pinned to the frozen MIT original `rhasspy/piper`** (or the current `piper1-gpl` fork used as a
   subprocess, the same pattern already used for ffmpeg, if a specific voice or the alignment
   feature is worth the very small residual GPL-on-output ambiguity this study found is
   effectively nil anyway). **Argument**: this is the only option that fully matches what the
   music work already proved out — no subscription, no credential, no per-render cost, offline,
   reproducible, publishable on a public site without a single caveat. **Effort**: medium — a
   `Speech` type beside `Music` (`src/audio.rs`/new `src/speech.rs`), a subprocess wrapper the
   shape of `src/encode.rs`'s ffmpeg calls, `AssetUse`/`validate` wiring, and — separately — the
   `web-pack` snapshot path for the browser. **Licence**: clean, verified, zero ongoing
   dependency.

2. **Ship word-synced captions as a companion, not a prerequisite** — a `Motion::Cues` variant
   consuming a `(word, start_time)` list, sourced from Piper's alignment API where the voice
   supports it, or a local forced-aligner (Montreal Forced Aligner) run over the finished WAV
   where it doesn't. **Argument**: this is the concrete, buildable half of "timing is the point,"
   and it's genuinely the single most differentiating feature this study found — it's the thing
   that makes narration *ShowReel's* narration rather than an audio file with a caption
   guessed alongside it. **Effort**: medium, mostly on the alignment-to-cue-table plumbing rather
   than the renderer (the motion/easing machinery this needs already exists per
   `docs/anarchist-study.md`'s finding that kinetic-caption motion is already built). **Licence**:
   MFA and WhisperX are both open and offline; no new licence surface at all.

3. **A real listening comparison of Piper vs. Kokoro before committing to one as the shipped
   default** — both clear the licence bar outright, and this study only ran one of them.
   **Argument**: quality is the one axis this study could not settle from a pricing page or a
   benchmark blog; it needs ears, on real narration-length text, not a benchmark's MOS number.
   **Effort**: small — an afternoon, not a build. **Licence**: both clean (MIT-adjacent / Apache-
   2.0).

4. **Document, but do not build, a "bring your own cloud WAV" path** — if the captain wants a
   specific ElevenLabs voice for a one-off video (the "dungeon soup villain" case named in the
   brief), generate it externally and reference the resulting file as an ordinary
   `Audio::track("narration.wav")` — zero crate changes, works today. **Argument**: this is
   explicitly *not* a recommendation to wire ElevenLabs or Gemini into the crate as a resolver;
   it's naming the escape hatch that already exists so nobody reinvents it, while keeping the
   crate itself free of a live-network, credentialed dependency. **Effort**: zero — it's a README
   paragraph. **Licence**: exactly ElevenLabs'/Gemini's own terms above, the captain's call per
   render, made with eyes open rather than assumed.

## What I would not do, and why

- **Would not write a from-scratch neural TTS model in Rust.** This is not chiptune. A synth
  faithful to three oscillators and a step sequencer is a weekend; a model that clears "clear,
  human sound" the way ElevenLabs or Gemini do is a research problem this project has no reason
  to take on, and pretending otherwise would produce something worse than either the cloud APIs
  or Piper while costing far more to build.
- **Would not wire a cloud API key into the renderer as a first-class resolver**, in either the
  native or wasm build. It reintroduces exactly the credential-exposure and licence-ambiguity risk
  class this estate has already been burned by twice this week, and it breaks the "a committed
  film builds for anyone who clones the repo, no secrets required" property the music work spent
  effort earning. The documented pre-step escape hatch (recommendation 4) gets the captain the
  same voice without that cost.
- **Would not touch Coqui/XTTS at all.** Named explicitly above so it doesn't get reached for by
  accident — it is the model every tutorial recommends and the one licence in this whole study
  with genuinely no legitimate path to commercial use.
- **Would not attempt to make narration drive scene-cut timing in this pass.** It's the real
  architectural prize and I named exactly what it would take (resolving speech before scene
  duration, not after) — but landing a version that only half-derives it would produce a film
  format where a scene's declared duration can silently disagree with what its own narration
  needs, which is a worse failure mode than not having the feature. Left as a next step, the same
  restraint `MusicFit`'s own general solver was given in `AGENTS.md`.
- **Would not pick Kokoro or Piper as the sole answer without the listening pass in
  recommendation 3.** Both look right on paper; I only ran one of them, and licence clarity alone
  isn't the whole decision the captain needs to make.
