//! Narration under the film — directed voice-over, spoken by Kokoro.
//!
//! # Why this is a *pre-baked source*, not a render-time synth like music
//!
//! [`crate::music`] synthesises its chiptune in pure Rust on every render: it
//! has no dependency, runs on `wasm32`, and a stranger who clones the repo
//! rebuilds the music from source with nothing but `cargo` and `ffmpeg`.
//! Narration cannot be that. The voice is [Kokoro], an 82M-parameter neural TTS
//! model that runs under Python (torch, numpy pinned to 1.26/2.x, a spaCy
//! model) — a stack that is emphatically *not* "works in a terminal with
//! nothing behind it". Synthesising narration on every render would make every
//! `showreel render` of a narrated film require that whole stack, and would not
//! compile to the browser at all.
//!
//! So narration follows the pattern ShowReel already uses for its other heavy
//! external tool — video. A [`crate::layer::Content::Clip`] is *decoded* by
//! ffmpeg, and the browser (which has no ffmpeg) gets a `.srclip` produced
//! ahead of time by `showreel web-pack`. Narration generalises that: the heavy
//! tool is Kokoro, and `showreel narrate` is its `web-pack`. A [`Narration`] is
//! declared in the film as readable text with per-line direction; an explicit,
//! one-time **bake** turns it into a plain WAV asset beside the film; and from
//! that point it is an ordinary [`crate::audio::Audio`] track. `render` needs
//! only ffmpeg — never Python — so the audio is reproducible by anyone who has
//! the baked WAV, exactly the "pre-rendered audio as a first-class asset"
//! property a voice model can actually deliver.
//!
//! The seam is the same one music uses,
//! [`crate::timeline::Film::resolve_audio_tracks`]: where a file track calls
//! `assets.resolve` and a music track synthesises, a narration track resolves
//! to its **content-addressed baked WAV** by name. If that file is not present,
//! resolution fails loudly and names the fix (`showreel narrate`) — it never
//! silently renders a narrated film mute, and because the name is a hash of the
//! script, editing a line makes the old bake un-findable rather than letting a
//! stale take through. That is the same freshness discipline `.srclip` and
//! [`crate::music::Music::render_to_temp`] both rely on.
//!
//! # Direction is the point
//!
//! One `speed` across a whole script sounds flat — measured, by the captain who
//! chose this voice. So direction is *per line*: each [`NarrationLine`] carries
//! its own [`pace`](NarrationLine::pace) (Kokoro's synthesis speed) and its own
//! pauses ([`pause_before`](NarrationLine::pause_before),
//! [`pause_after`](NarrationLine::pause_after)). A script is marked up the way
//! you would direct a voice actor — slow this line down, hold a beat after that
//! one — and the assembly here honours it. The pace is applied inside synthesis
//! (so the word timings reflect it); the pauses are inserted here, in Rust, as
//! silence, so the directed timing is testable with no model in the loop.
//!
//! # The voice and the synthesiser are declared, not baked in
//!
//! A film declares **both** which [`engine`](Narration::engine) synthesises the
//! narration and which [`voice`](Narration::voice) it uses — they are ordinary
//! parameters, the same way [`crate::music::Mood`] is. Kokoro is *one* engine
//! behind that seam (its default voice is the British `bm_george`), not the
//! design: the engine string selects a synthesiser at bake time
//! ([`crate::narrate`]'s engine registry), so a different model — a
//! voice-cloning engine, say — is a new entry there and a one-word change in
//! the film, with nothing in the format or this module to unpick. Everything in
//! this pure module treats the engine and voice as opaque strings.
//!
//! # Word timings are real
//!
//! Kokoro's model predicts a duration for every phoneme, and its pipeline turns
//! those into a start/end time per word. The bake reads them straight off the
//! model output (see `tools/narrate/kokoro_narrate.py`) and this module offsets
//! them by each line's position in the assembled track, producing a
//! [`NarrationManifest`] written beside the WAV. These are genuine
//! model-derived timings — not a characters-per-second guess — which is what
//! lets narration be cued against what is on screen, and captions be built
//! later. Nothing here fakes a timing it does not have.
//!
//! # Verify before writing
//!
//! A neural vocoder can emit plausible-looking white noise that passes every
//! duration/codec/decode check. The one thing that separates it from speech is
//! the zero-crossing rate of the loudest window (speech ~0.13, noise ~0.49), so
//! [`zero_crossing_rate`] is run natively — no Python in the verification path —
//! on every synthesised line *and* on the finished assembly before either is
//! written as final. See [`looks_like_speech`].

use crate::time::Time;
use serde::{Deserialize, Serialize};

/// Kokoro's native output sample rate. The baked WAV is written at this rate;
/// the mix filter resamples every track to 48 kHz before combining, so this
/// need not match music's [`crate::music::SAMPLE_RATE`].
pub const SAMPLE_RATE: u32 = 24_000;

/// The default synthesiser when a film does not name one. Kokoro is the first
/// engine implemented (see [`crate::narrate`]); this being a mere default is the
/// point — a film can name a different engine, and the plumbing here is
/// engine-agnostic. Keep in sync with [`crate::narrate`]'s engine registry and
/// [`known_engine`].
pub const DEFAULT_ENGINE: &str = "kokoro";

/// The engine names this build knows, so an unknown one is reported at
/// `showreel check` rather than only at bake time. Kept in lock step with the
/// registry in [`crate::narrate`] — adding an engine adds a row to both.
pub const KNOWN_ENGINES: &[&str] = &["kokoro"];

/// Whether `name` is an engine this build can bake with.
pub fn known_engine(name: &str) -> bool {
    KNOWN_ENGINES.contains(&name.trim())
}

/// The voice an engine uses when a film names the engine but no voice. This is
/// a per-engine default — Kokoro's happens to be `bm_george`, the British male
/// read measured most natural — not a global "the voice"; another engine brings
/// its own. An unknown engine has no default, so a film using it must name a
/// voice, which [`Narration::validate`] enforces.
pub fn default_voice(engine: &str) -> &'static str {
    match engine.trim() {
        "kokoro" => "bm_george",
        _ => "",
    }
}

/// The default silence after a line that does not set its own `pause_after` —
/// the natural beat between sentences. A line overrides it in either direction:
/// `0` to run straight on, a second or more to hold for effect.
const DEFAULT_GAP: f64 = 0.35;

/// The corruption threshold: a loudest-window zero-crossing rate at or above
/// this is noise, not speech. Real Kokoro speech measures ~0.10–0.15; white
/// noise ~0.49. Matches `tools/prosody/check.py`, kept native so the bake's
/// self-check never depends on the same Python stack that might be broken.
const SPEECH_ZCR_MAX: f64 = 0.30;

/// Bumped when the Rust assembly (pause insertion, concatenation, WAV framing)
/// changes in a way that alters the baked bytes, so a stale WAV from an older
/// assembly is never reused. The Kokoro model version is deliberately *not* in
/// here — Rust cannot see it, and a re-bake is always an explicit step anyway.
const NARRATION_VERSION: u32 = 1;

/// A directed voice-over track: a voice and a script of directed lines.
///
/// Written in a film file three ways, from terse to full, mirroring
/// [`crate::music::Music`]'s bare-word-or-object shorthand:
///
/// ```jsonc
/// // A single line, default engine and voice, default direction:
/// "narration": "Everything you see is built from one number."
///
/// // A script — an array of lines, default direction:
/// "narration": ["First line.", "Second line."]
///
/// // Full control — the synthesiser, the voice, and per-line direction:
/// "narration": {
///   "engine": "kokoro",
///   "voice": "bm_george",
///   "gap": 0.35,
///   "lines": [
///     { "text": "Everything you see is built from one number.", "pace": 0.95 },
///     { "text": "Change the number and the city changes.", "pause_before": 0.4 }
///   ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Narration {
    /// Which synthesiser bakes this narration — see [`DEFAULT_ENGINE`] and
    /// [`crate::narrate`]'s engine registry. An opaque string here: the engine
    /// decides how [`voice`](Self::voice) is interpreted (a Kokoro voice id, a
    /// path to reference audio for a cloning engine, and so on).
    pub engine: String,
    /// The voice, interpreted by the [`engine`](Self::engine). Defaults to that
    /// engine's own default voice ([`default_voice`]).
    pub voice: String,
    /// The default silence after a line — see [`DEFAULT_GAP`]. A line's own
    /// [`pause_after`](NarrationLine::pause_after) overrides it.
    pub gap: Time,
    pub lines: Vec<NarrationLine>,
}

/// One directed line of the script.
#[derive(Debug, Clone, PartialEq)]
pub struct NarrationLine {
    pub text: String,
    /// Kokoro synthesis speed. `1.0` is the voice's natural pace; below 1 is
    /// slower and more deliberate, above 1 quicker. The single biggest lever on
    /// how directed the read sounds, after the voice itself — applied inside
    /// synthesis, so the word timings reflect it.
    pub pace: f64,
    /// Silence inserted *before* this line's speech. Usually 0; a beat here sets
    /// a line apart from the one before it.
    pub pause_before: Time,
    /// Silence inserted *after* this line's speech. Unset falls back to the
    /// narration's [`gap`](Narration::gap); set it to `0` to run straight on, or
    /// long for a held pause.
    pub pause_after: Option<Time>,
}

impl NarrationLine {
    /// A line at natural pace with the default trailing gap.
    pub fn new(text: impl Into<String>) -> Self {
        NarrationLine { text: text.into(), pace: 1.0, pause_before: Time::ZERO, pause_after: None }
    }

    pub fn pace(mut self, p: f64) -> Self {
        self.pace = p;
        self
    }

    pub fn pause_before(mut self, t: impl Into<Time>) -> Self {
        self.pause_before = t.into();
        self
    }

    pub fn pause_after(mut self, t: impl Into<Time>) -> Self {
        self.pause_after = Some(t.into());
        self
    }

    /// The trailing silence actually used, resolving an unset value to `gap`.
    fn resolved_pause_after(&self, gap: f64) -> f64 {
        self.pause_after.map(|t| t.as_secs()).unwrap_or(gap).max(0.0)
    }
}

impl Narration {
    /// A single-line narration in the default engine and voice.
    pub fn line(text: impl Into<String>) -> Self {
        Narration::script([NarrationLine::new(text)])
    }

    /// A narration from a set of already-directed lines, in the default engine
    /// and that engine's default voice.
    pub fn script(lines: impl IntoIterator<Item = NarrationLine>) -> Self {
        Narration {
            engine: DEFAULT_ENGINE.to_string(),
            voice: default_voice(DEFAULT_ENGINE).to_string(),
            gap: Time(DEFAULT_GAP),
            lines: lines.into_iter().collect(),
        }
    }

    /// Choose the synthesiser. If the current voice is still the *previous*
    /// engine's default, it switches to the new engine's default too, so
    /// `Narration::line("…").engine("kokoro")` picks kokoro's own voice.
    pub fn engine(mut self, e: impl Into<String>) -> Self {
        let e = e.into();
        if self.voice == default_voice(&self.engine) {
            self.voice = default_voice(&e).to_string();
        }
        self.engine = e;
        self
    }

    pub fn voice(mut self, v: impl Into<String>) -> Self {
        self.voice = v.into();
        self
    }

    pub fn gap(mut self, g: impl Into<Time>) -> Self {
        self.gap = g.into();
        self
    }

    pub fn line_of(mut self, line: NarrationLine) -> Self {
        self.lines.push(line);
        self
    }

    /// A short label for `info`/`summary` display, where a narration track has
    /// no source filename to show: the engine and voice.
    pub fn source_label(&self) -> String {
        format!("{}:{}", self.engine, self.voice)
    }

    /// The rules a type cannot carry. `label` names the track in messages.
    pub fn validate(&self, label: &str) -> Vec<String> {
        let mut errs = Vec::new();
        if self.engine.trim().is_empty() {
            errs.push(format!("{label}: narration needs an engine"));
        } else if !known_engine(&self.engine) {
            errs.push(format!(
                "{label}: unknown narration engine {:?} (known: {})",
                self.engine,
                KNOWN_ENGINES.join(", ")
            ));
        }
        if self.voice.trim().is_empty() {
            errs.push(format!("{label}: narration needs a voice"));
        }
        if self.lines.is_empty() {
            errs.push(format!("{label}: narration has no lines to speak"));
        }
        for (i, l) in self.lines.iter().enumerate() {
            if l.text.trim().is_empty() {
                errs.push(format!("{label}: line {i} is empty"));
            }
            if l.pace <= 0.0 {
                errs.push(format!("{label}: line {i} has a non-positive pace ({})", l.pace));
            }
            if l.pause_before.as_secs() < 0.0
                || l.pause_after.map(|t| t.as_secs() < 0.0).unwrap_or(false)
            {
                errs.push(format!("{label}: line {i} has a negative pause"));
            }
        }
        errs
    }

    /// A stable hash of everything that affects the baked audio: the voice, the
    /// sample rate, the assembly version, and every line's directed text, pace
    /// and pauses. Two scripts collide only if they would synthesise and
    /// assemble to the same bytes; editing any line changes it, which is what
    /// makes a stale bake un-findable rather than silently reused.
    pub fn content_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        NARRATION_VERSION.hash(&mut h);
        SAMPLE_RATE.hash(&mut h);
        self.engine.hash(&mut h);
        self.voice.hash(&mut h);
        let gap = self.gap.as_secs();
        for l in &self.lines {
            l.text.hash(&mut h);
            l.pace.to_bits().hash(&mut h);
            l.pause_before.as_secs().to_bits().hash(&mut h);
            l.resolved_pause_after(gap).to_bits().hash(&mut h);
        }
        h.finish()
    }

    /// The file name of this narration's baked WAV: a human-recognisable slug
    /// (the voice and the opening words) plus the content hash, so a directory
    /// of bakes is browsable and a changed script never reuses an old file. The
    /// voice is sanitised for a filename — a cloning engine's voice may be a
    /// path — while the hash (which covers the engine) guarantees uniqueness.
    pub fn baked_name(&self) -> String {
        format!("narration-{}-{}-{:016x}.wav", safe(&self.voice), self.slug(), self.content_hash())
    }

    /// The sidecar manifest name — the word timings beside the WAV.
    pub fn manifest_name(&self) -> String {
        format!("narration-{}-{}-{:016x}.words.json", safe(&self.voice), self.slug(), self.content_hash())
    }

    /// A few opening words of the first line, lower-kebab, for the file name.
    fn slug(&self) -> String {
        let first = self.lines.first().map(|l| l.text.as_str()).unwrap_or("");
        let mut out = String::new();
        for word in first.split_whitespace().take(3) {
            for c in word.chars() {
                if c.is_ascii_alphanumeric() {
                    out.push(c.to_ascii_lowercase());
                } else if !out.ends_with('-') && !out.is_empty() {
                    out.push('-');
                }
            }
            out.push('-');
        }
        let trimmed = out.trim_matches('-');
        if trimmed.is_empty() { "line".to_string() } else { trimmed.to_string() }
    }

    /// Assemble synthesised line audio into one track: directed pauses inserted,
    /// lines concatenated, word timings offset onto the track's own clock.
    ///
    /// Pure — no filesystem, no model. `lines` are the per-line results from the
    /// driver, in script order: each is the mono [`SAMPLE_RATE`] samples of one
    /// line and that line's word timings *relative to the line's own start*.
    /// This is the half of the pipeline that is fully testable without Kokoro.
    pub fn assemble(&self, lines: &[SynthLine]) -> Assembled {
        let sr = SAMPLE_RATE as f64;
        let gap = self.gap.as_secs();
        let mut samples: Vec<i16> = Vec::new();
        let mut timings: Vec<LineTiming> = Vec::new();

        for (i, (spec, synth)) in self.lines.iter().zip(lines).enumerate() {
            let before = spec.pause_before.as_secs().max(0.0);
            silence(&mut samples, (before * sr).round() as usize);

            let line_start = samples.len() as f64 / sr;
            samples.extend_from_slice(&synth.samples);
            let line_end = samples.len() as f64 / sr;

            let words = synth
                .words
                .iter()
                .map(|w| WordTiming {
                    text: w.text.clone(),
                    start: round3(line_start + w.start),
                    end: round3(line_start + w.end),
                })
                .collect();
            timings.push(LineTiming {
                index: i,
                text: spec.text.clone(),
                start: round3(line_start),
                end: round3(line_end),
                words,
            });

            let after = spec.resolved_pause_after(gap);
            silence(&mut samples, (after * sr).round() as usize);
        }

        let duration = samples.len() as f64 / sr;
        Assembled { samples, manifest: NarrationManifest { voice: self.voice.clone(), sample_rate: SAMPLE_RATE, duration: round3(duration), lines: timings } }
    }
}

/// One line's synthesised audio and its word timings, as produced by the driver
/// — the input to [`Narration::assemble`]. Timings are relative to the line.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthLine {
    pub samples: Vec<i16>,
    pub words: Vec<WordTiming>,
}

/// The assembled track: the samples to write, and the manifest to write beside
/// them.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembled {
    pub samples: Vec<i16>,
    pub manifest: NarrationManifest,
}

/// Where every word lands on a narration track's own clock — the sidecar the
/// bake writes for cueing and captions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NarrationManifest {
    pub voice: String,
    pub sample_rate: u32,
    pub duration: f64,
    pub lines: Vec<LineTiming>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineTiming {
    pub index: usize,
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub words: Vec<WordTiming>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordTiming {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

fn silence(buf: &mut Vec<i16>, n: usize) {
    buf.extend(std::iter::repeat_n(0i16, n));
}

/// A string reduced to filename-safe characters (alphanumerics, `_`, `-`), so a
/// voice id or reference-audio path can go into a baked file name unharmed.
fn safe(s: &str) -> String {
    let out: String = s
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
        .collect();
    let t = out.trim_matches('-').to_string();
    if t.is_empty() { "voice".to_string() } else { t }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

// --- corruption check ---------------------------------------------------------

/// The zero-crossing rate of the loudest one-second window — the measure that
/// separates real speech from plausible-looking noise. A port of
/// `tools/prosody/check.py`, kept in Rust so the bake verifies its own output
/// with no dependency on the Python stack that produced it.
///
/// The loudest window is used because a mostly-quiet clip with a burst of noise
/// would average out to a speech-like rate; the burst is where corruption
/// shows. Returns 0 for a clip too short to hold a window.
pub fn zero_crossing_rate(samples: &[i16], sample_rate: u32) -> f64 {
    let sr = sample_rate as usize;
    if samples.len() < sr {
        // Too short for a full window: measure the whole thing rather than
        // nothing, so a very short line is still checked.
        return zcr_of(samples);
    }
    let step = (sr / 2).max(1);
    let mut best_energy = -1.0f64;
    let mut best = &samples[0..sr];
    let mut s = 0;
    while s + sr <= samples.len() {
        let w = &samples[s..s + sr];
        let e: f64 = w.iter().map(|&x| (x as f64).abs()).sum::<f64>() / w.len() as f64;
        if e > best_energy {
            best_energy = e;
            best = w;
        }
        s += step;
    }
    zcr_of(best)
}

fn zcr_of(w: &[i16]) -> f64 {
    if w.len() < 2 {
        return 0.0;
    }
    let crossings = w
        .windows(2)
        .filter(|p| (p[0] < 0) != (p[1] < 0))
        .count();
    crossings as f64 / w.len() as f64
}

/// Whether a buffer measures as speech rather than corruption — the gate the
/// bake refuses to write past. See [`zero_crossing_rate`] and [`SPEECH_ZCR_MAX`].
pub fn looks_like_speech(samples: &[i16], sample_rate: u32) -> bool {
    zero_crossing_rate(samples, sample_rate) < SPEECH_ZCR_MAX
}

// --- WAV I/O (mono i16) -------------------------------------------------------

/// Wrap mono 16-bit samples in a canonical PCM WAV container.
pub fn wav_bytes(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let channels = 1u16;
    let bytes_per_sample = 2u32;
    let block_align = channels as u32 * bytes_per_sample;
    let byte_rate = sample_rate * block_align;
    let data_len = (samples.len() as u32) * bytes_per_sample;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&(block_align as u16).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// A parsed WAV: its mono samples and its sample rate. Reads canonical PCM
/// (what soundfile writes for the per-line files) — enough for what the driver
/// produces, not a general WAV reader.
pub struct WavData {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Parse a canonical 16-bit PCM WAV from bytes: locate `fmt ` for the rate and
/// channel count, and `data` for the samples. A file with more than one channel
/// is downmixed to mono by averaging, so a stereo source still verifies and
/// assembles.
pub fn read_wav(bytes: &[u8]) -> anyhow::Result<WavData> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        anyhow::bail!("not a RIFF/WAVE file");
    }
    let mut sample_rate = SAMPLE_RATE;
    let mut channels = 1u16;
    let mut data: Option<&[u8]> = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]]) as usize;
        let body_start = pos + 8;
        let body_end = (body_start + size).min(bytes.len());
        if id == b"fmt " && body_end - body_start >= 16 {
            channels = u16::from_le_bytes([bytes[body_start + 2], bytes[body_start + 3]]).max(1);
            sample_rate = u32::from_le_bytes([
                bytes[body_start + 4],
                bytes[body_start + 5],
                bytes[body_start + 6],
                bytes[body_start + 7],
            ]);
        } else if id == b"data" {
            data = Some(&bytes[body_start..body_end]);
        }
        // Chunks are word-aligned: an odd size is followed by a pad byte.
        pos = body_start + size + (size & 1);
    }
    let data = data.ok_or_else(|| anyhow::anyhow!("no data chunk in WAV"))?;
    let n = (data.len() / 2) * 2;
    let raw: Vec<i16> = data[..n]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    let samples = if channels <= 1 {
        raw
    } else {
        let c = channels as usize;
        raw.chunks(c)
            .map(|frame| {
                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                (sum / frame.len() as i32) as i16
            })
            .collect()
    };
    Ok(WavData { samples, sample_rate, channels: 1 })
}

/// The playing length in seconds of a canonical PCM WAV — read from its header,
/// so the render path can learn a baked narration's true duration without the
/// manifest. See [`crate::timeline::Film::resolve_audio_tracks`] for why a
/// narration's natural length is the speech, not "to the end of the film".
pub fn wav_duration(bytes: &[u8]) -> anyhow::Result<f64> {
    let w = read_wav(bytes)?;
    Ok(w.samples.len() as f64 / w.sample_rate.max(1) as f64)
}

// --- serde: a bare string, an array of strings, or a full object --------------

impl Serialize for Narration {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        NarrationRepr::from(self.clone()).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Narration {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(NarrationRepr::deserialize(d)?.into())
    }
}

impl Serialize for NarrationLine {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        LineRepr::from(self.clone()).serialize(s)
    }
}

impl<'de> Deserialize<'de> for NarrationLine {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(LineRepr::deserialize(d)?.into())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum NarrationRepr {
    /// A single line — `"narration": "text"`.
    One(String),
    /// A whole script of default-direction lines — `["a", "b"]` — or already
    /// directed lines. Distinguished from `Full` by being an array.
    Lines(Vec<LineRepr>),
    Full(NarrationFull),
}

#[derive(Serialize, Deserialize)]
struct NarrationFull {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gap: Option<Time>,
    lines: Vec<LineRepr>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum LineRepr {
    Text(String),
    Full(LineFull),
}

#[derive(Serialize, Deserialize)]
struct LineFull {
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pace: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pause_before: Option<Time>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pause_after: Option<Time>,
}

impl From<LineRepr> for NarrationLine {
    fn from(r: LineRepr) -> NarrationLine {
        match r {
            LineRepr::Text(text) => NarrationLine::new(text),
            LineRepr::Full(f) => NarrationLine {
                text: f.text,
                pace: f.pace.unwrap_or(1.0),
                pause_before: f.pause_before.unwrap_or(Time::ZERO),
                pause_after: f.pause_after,
            },
        }
    }
}

impl From<NarrationLine> for LineRepr {
    fn from(l: NarrationLine) -> LineRepr {
        // Emit the terse string form only for a fully default line; otherwise
        // the object, so direction round-trips.
        if l.pace == 1.0 && l.pause_before.as_secs() == 0.0 && l.pause_after.is_none() {
            LineRepr::Text(l.text)
        } else {
            LineRepr::Full(LineFull {
                text: l.text,
                pace: (l.pace != 1.0).then_some(l.pace),
                pause_before: (l.pause_before.as_secs() != 0.0).then_some(l.pause_before),
                pause_after: l.pause_after,
            })
        }
    }
}

impl From<NarrationRepr> for Narration {
    fn from(r: NarrationRepr) -> Narration {
        match r {
            NarrationRepr::One(text) => Narration::line(text),
            NarrationRepr::Lines(lines) => {
                Narration::script(lines.into_iter().map(NarrationLine::from))
            }
            NarrationRepr::Full(f) => {
                // Resolve engine and voice to concrete values at parse time, so
                // the content hash is over what will actually be synthesised —
                // an unset voice defaults to the (now known) engine's default.
                let engine = f
                    .engine
                    .filter(|e| !e.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_ENGINE.to_string());
                let voice = f
                    .voice
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or_else(|| default_voice(&engine).to_string());
                Narration {
                    engine,
                    voice,
                    gap: f.gap.unwrap_or(Time(DEFAULT_GAP)),
                    lines: f.lines.into_iter().map(NarrationLine::from).collect(),
                }
            }
        }
    }
}

impl From<Narration> for NarrationRepr {
    fn from(n: Narration) -> NarrationRepr {
        // Always the object form: the string/array shorthands are input sugar,
        // and emitting the object keeps voice/gap explicit so a value
        // round-trips exactly. Mirrors `Music`'s serialize.
        NarrationRepr::Full(NarrationFull {
            engine: Some(n.engine),
            voice: Some(n.voice),
            gap: Some(n.gap),
            lines: n.lines.into_iter().map(LineRepr::from).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_string_is_a_single_line_in_the_default_engine_and_voice() {
        let n: Narration = serde_json::from_str("\"Hello there.\"").unwrap();
        assert_eq!(n.engine, DEFAULT_ENGINE);
        assert_eq!(n.voice, default_voice(DEFAULT_ENGINE));
        assert_eq!(n.lines.len(), 1);
        assert_eq!(n.lines[0].text, "Hello there.");
        assert_eq!(n.lines[0].pace, 1.0);
    }

    #[test]
    fn an_array_is_a_script_of_default_lines() {
        let n: Narration = serde_json::from_str(r#"["First.", "Second."]"#).unwrap();
        assert_eq!(n.lines.len(), 2);
        assert_eq!(n.lines[1].text, "Second.");
        assert_eq!(n.engine, DEFAULT_ENGINE);
    }

    #[test]
    fn the_object_form_carries_engine_voice_gap_and_per_line_direction() {
        let n: Narration = serde_json::from_str(
            r#"{ "engine": "kokoro", "voice": "am_adam", "gap": 0.5,
                 "lines": [ {"text": "Slow.", "pace": 0.9, "pause_after": 1.2}, "Fast." ] }"#,
        )
        .unwrap();
        assert_eq!(n.engine, "kokoro");
        assert_eq!(n.voice, "am_adam");
        assert_eq!(n.gap, Time(0.5));
        assert_eq!(n.lines[0].pace, 0.9);
        assert_eq!(n.lines[0].pause_after, Some(Time(1.2)));
        assert_eq!(n.lines[1].pace, 1.0, "the second line is a bare string, default pace");
    }

    #[test]
    fn an_unset_voice_defaults_to_the_engines_default_voice() {
        let n: Narration =
            serde_json::from_str(r#"{ "engine": "kokoro", "lines": ["Hi."] }"#).unwrap();
        assert_eq!(n.voice, "bm_george", "kokoro's default voice fills in");
    }

    #[test]
    fn an_unknown_engine_is_a_check_error_not_a_silent_default() {
        let n = Narration::script([NarrationLine::new("Hi.")]).engine("mystery").voice("x");
        assert!(!known_engine("mystery"));
        assert!(n.validate("n").iter().any(|e| e.contains("unknown narration engine")));
    }

    #[test]
    fn narration_round_trips_through_json() {
        let n = Narration::script([
            NarrationLine::new("One.").pace(0.95),
            NarrationLine::new("Two.").pause_before(0.4).pause_after(1.0),
        ])
        .voice("bm_george")
        .gap(0.4);
        let s = serde_json::to_string(&n).unwrap();
        assert_eq!(serde_json::from_str::<Narration>(&s).unwrap(), n);
    }

    #[test]
    fn a_default_line_round_trips_back_to_the_terse_string_form() {
        let l = NarrationLine::new("Plain.");
        let s = serde_json::to_string(&l).unwrap();
        assert_eq!(s, "\"Plain.\"");
    }

    #[test]
    fn choosing_an_engine_switches_its_default_voice() {
        // The generic builder path: switching engine off the default carries
        // the new engine's default voice, but an explicitly-set voice is kept.
        let n = Narration::line("x").engine("kokoro");
        assert_eq!(n.voice, "bm_george");
        let kept = Narration::line("x").voice("am_adam").engine("kokoro");
        assert_eq!(kept.voice, "am_adam", "an explicit voice survives an engine switch");
    }

    #[test]
    fn the_content_hash_changes_when_any_directed_field_changes() {
        let base = Narration::script([NarrationLine::new("A line.")]);
        let paced = Narration::script([NarrationLine::new("A line.").pace(0.9)]);
        let paused = Narration::script([NarrationLine::new("A line.").pause_after(2.0)]);
        let voiced = base.clone().voice("am_adam");
        let engined = base.clone().voice("v").engine("other");
        assert_ne!(base.content_hash(), paced.content_hash());
        assert_ne!(base.content_hash(), paused.content_hash());
        assert_ne!(base.content_hash(), voiced.content_hash());
        assert_ne!(base.content_hash(), engined.content_hash(), "the engine is in the hash");
        // Identical scripts hash identically — the cache is content-addressed.
        assert_eq!(base.content_hash(), Narration::script([NarrationLine::new("A line.")]).content_hash());
    }

    #[test]
    fn the_baked_name_is_recognisable_and_carries_the_hash() {
        let n = Narration::line("Everything you see is built from one number.");
        let name = n.baked_name();
        assert!(name.starts_with("narration-bm_george-everything-you-see"), "{name}");
        assert!(name.ends_with(".wav"));
        assert!(name.contains(&format!("{:016x}", n.content_hash())));
    }

    #[test]
    fn validation_catches_the_empty_and_the_nonsense() {
        let empty =
            Narration { engine: "kokoro".into(), voice: "bm_george".into(), gap: Time(0.35), lines: vec![] };
        assert!(empty.validate("n").iter().any(|e| e.contains("no lines")));
        let bad = Narration::script([NarrationLine::new("ok").pace(0.0), NarrationLine::new("  ")]);
        let errs = bad.validate("n");
        assert!(errs.iter().any(|e| e.contains("non-positive pace")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("line 1 is empty")), "{errs:?}");
    }

    #[test]
    fn assembly_inserts_directed_pauses_and_offsets_word_timings() {
        // Two lines: the first ends with a 1s pause_after, the second has a
        // 0.5s pause_before. Each "line" is 1s of a constant tone (24000
        // samples). Word timings are relative to the line; after assembly they
        // must be absolute on the track's clock.
        let sr = SAMPLE_RATE as usize;
        let tone: Vec<i16> = (0..sr).map(|i| if (i / 60) % 2 == 0 { 6000 } else { -6000 }).collect();
        let n = Narration::script([
            NarrationLine::new("First line").pause_after(1.0),
            NarrationLine::new("Second line").pause_before(0.5),
        ])
        .gap(0.0);
        let synth = vec![
            SynthLine { samples: tone.clone(), words: vec![WordTiming { text: "First".into(), start: 0.0, end: 0.4 }] },
            SynthLine { samples: tone.clone(), words: vec![WordTiming { text: "Second".into(), start: 0.0, end: 0.4 }] },
        ];
        let a = n.assemble(&synth);
        // Total = 1s speech + 1s pause + 0.5s pause + 1s speech = 3.5s.
        assert!((a.manifest.duration - 3.5).abs() < 0.01, "{}", a.manifest.duration);
        // First word at t=0.
        assert!((a.manifest.lines[0].words[0].start - 0.0).abs() < 0.01);
        // Second line starts at 1s speech + 1s pause_after + 0.5s pause_before = 2.5s,
        // so "Second" lands at ~2.5s — the offset proves absolute timing.
        assert!((a.manifest.lines[1].words[0].start - 2.5).abs() < 0.01, "{}", a.manifest.lines[1].words[0].start);
        assert_eq!(a.samples.len(), (3.5 * sr as f64) as usize);
    }

    #[test]
    fn a_pure_tone_reads_as_speech_and_white_noise_does_not() {
        let sr = SAMPLE_RATE;
        // A ~200 Hz square tone: crosses zero ~twice per 120-sample cycle,
        // ZCR ~0.017 — well under the speech threshold.
        let tone: Vec<i16> = (0..sr).map(|i| if (i / 60) % 2 == 0 { 8000 } else { -8000 }).collect();
        assert!(looks_like_speech(&tone, sr), "zcr {}", zero_crossing_rate(&tone, sr));
        // Deterministic pseudo-white noise: alternating-ish sign flips near half
        // the samples, ZCR ~0.5 — the corruption a vocoder can emit.
        let mut state = 0x1234_5678u32;
        let noise: Vec<i16> = (0..sr)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 16) as i16
            })
            .collect();
        assert!(!looks_like_speech(&noise, sr), "zcr {}", zero_crossing_rate(&noise, sr));
    }

    #[test]
    fn wav_round_trips_through_write_and_read() {
        let samples: Vec<i16> = (0..5000).map(|i| ((i * 7) % 2000 - 1000) as i16).collect();
        let bytes = wav_bytes(&samples, SAMPLE_RATE);
        let w = read_wav(&bytes).unwrap();
        assert_eq!(w.sample_rate, SAMPLE_RATE);
        assert_eq!(w.channels, 1);
        assert_eq!(w.samples, samples);
        assert!((wav_duration(&bytes).unwrap() - 5000.0 / SAMPLE_RATE as f64).abs() < 1e-9);
    }

    #[test]
    fn read_wav_downmixes_stereo_to_mono() {
        // A tiny stereo WAV: L and R differ; the reader averages them.
        let mut bytes = Vec::new();
        let interleaved: Vec<i16> = vec![100, 300, -200, -400]; // 2 frames, L/R
        let data: Vec<u8> = interleaved.iter().flat_map(|s| s.to_le_bytes()).collect();
        let data_len = data.len() as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes()); // 2 channels
        bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        bytes.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.extend_from_slice(&data);
        let w = read_wav(&bytes).unwrap();
        assert_eq!(w.channels, 1);
        assert_eq!(w.samples, vec![200, -300]); // (100+300)/2, (-200+-400)/2
    }
}
