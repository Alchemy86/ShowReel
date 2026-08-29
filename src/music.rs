//! Generated music under the film — a chiptune, synthesised to the film's clock.
//!
//! # Why this is a source, not a second audio path
//!
//! A film needs music, and licensing a bed is exactly the kind of "reach for a
//! file the reader cannot regenerate" that fights ShowReel's whole pitch — a
//! film is a readable text you commit and rebuild from source. So the music is
//! *generated*, from a tiny description written in the film file. But it is
//! deliberately **not** a new pipeline: a [`Music`] spec resolves to a WAV file
//! exactly the way an `asset` string resolves to a file, and from that point it
//! is an ordinary [`crate::audio::Audio`] track. Placement, gain, fades, mixing
//! with other tracks, and the mobile cut all compose for free, because the
//! encoder never learns the file was synthesised rather than fetched. The seam
//! is [`crate::timeline::Film::resolve_audio_tracks`]: where a file track calls
//! `assets.resolve`, a music track calls [`Music::render_to_temp`] instead, and
//! both hand the resulting path to the same [`crate::audio::Audio::resolve`].
//!
//! # One idiom, not a synthesiser
//!
//! This synthesises exactly one thing: the NES/SID chiptune the reference
//! `chiptune.py` proved — a duty-cycled pulse bass, a pulse-wave arpeggio lead
//! with vibrato, and noise-burst drums. It is not a general instrument system
//! and does not want to become one; a [`Mood`] is a *data* preset (a chord
//! progression and two step patterns), not a new voice. Adding a mood is adding
//! a row to [`Voicing::of`], nothing more.
//!
//! # Timing is the point
//!
//! The reason this lives in ShowReel rather than beside a blog asset is that the
//! music can be generated to *fit the cuts*. Two levels, both real:
//!
//! - **Length** (always): a music track with no explicit `duration` is
//!   synthesised to exactly the film's own length, so it never needs trimming
//!   and never hard-cuts to silence — [`crate::audio::Audio`]'s "to the end of
//!   the film" default, made literal in samples rather than clamped after the
//!   fact.
//! - **Tempo fit** ([`MusicFit::Film`]): the authored `bpm` is treated as a target,
//!   and nudged to the nearest tempo at which a whole number of bars spans the
//!   track. The final downbeat then lands exactly on the film's end, and the
//!   whole bar grid is regular against the film — so an author who spaces scene
//!   cuts a whole number of bars apart gets a chord change on every cut. The
//!   general "align to N arbitrary interior cut times" solver is deliberately
//!   left for later (see the module tests and `README.md`); this is the honest,
//!   exact subset, not a half-built version of the clever one.
//!
//! # Determinism
//!
//! `chiptune.py` seeded its drums from `numpy`'s global RNG, so no two runs of
//! it were bit-identical. That is fine for a one-off asset and wrong for
//! ShowReel, whose renders must be reproducible from source. The drums here run
//! off a small seeded [`Rng`] (`splitmix64`), so the same spec always yields the
//! same bytes — which is also what lets [`Music::render_to_temp`] content-address
//! its cache.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// The synthesis sample rate, matching the reference and the rate every track
/// is resampled to before mixing (`aformat=...:sample_rates=48000`).
pub const SAMPLE_RATE: u32 = 48_000;

/// Bumped whenever the synthesis algorithm changes in a way that alters the
/// output samples. Folded into [`Music::render_to_temp`]'s cache key so a stale
/// WAV from an older algorithm is never reused for a spec that would now sound
/// different. Native only — it exists solely for that on-disk cache, which the
/// browser (no filesystem) has no equivalent of.
#[cfg(not(target_arch = "wasm32"))]
const SYNTH_VERSION: u32 = 2;

/// A mood: the musical content behind the one chiptune idiom.
///
/// Each mood is a chord progression and two step patterns (a bass gate and a
/// lead arpeggio), plus a couple of tone/tempo defaults — *data*, resolved by
/// [`Voicing::of`]. It is not a new instrument: every mood plays the same three
/// voices (pulse bass, pulse lead, noise drums).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mood {
    /// The reference tune: a punchy A-minor funk, `i–VI–III–VII`, 128 BPM.
    #[default]
    Funk,
    /// Gentler and slower: lush seventh chords, a sparse bass, a softer lead —
    /// for a calmer film. Same three voices, tuned down.
    Dreamy,
    /// Bright and hopeful: a major-key I-V-vi-IV progression, a marching pulse
    /// bass and a sparkling arpeggio lead an octave up — the title-screen
    /// character, for an opener that wants to lift rather than groove. Same
    /// three voices as every other mood; an original progression, not a
    /// transcription of any specific game's theme.
    Title,
}

impl Mood {
    fn parse(s: &str) -> Mood {
        match s.trim().to_ascii_lowercase().as_str() {
            // "chiptune" reads as "the default chiptune" for someone who does
            // not yet know the mood names — it resolves to the reference tune.
            "funk" | "chiptune" | "" => Mood::Funk,
            "dreamy" => Mood::Dreamy,
            "title" => Mood::Title,
            // Like `Grade`'s word shorthand, an unrecognised name is the default
            // rather than a hard error: a typo should cost you a mood, not a
            // render. `showreel check` still names it — see [`Music::validate`].
            _ => Mood::Funk,
        }
    }

    /// The mood's canonical name — the word that names it in a film file.
    pub fn name(self) -> &'static str {
        match self {
            Mood::Funk => "funk",
            Mood::Dreamy => "dreamy",
            Mood::Title => "title",
        }
    }

    /// Whether `s` names a mood this synth knows — so a typo is reported at
    /// load rather than silently swapped for the default.
    pub fn is_known(s: &str) -> bool {
        matches!(s.trim().to_ascii_lowercase().as_str(), "funk" | "chiptune" | "dreamy" | "title")
    }

    fn default_bpm(self) -> f64 {
        match self {
            Mood::Funk => 128.0,
            Mood::Dreamy => 96.0,
            Mood::Title => 150.0,
        }
    }
}

/// One bar of the progression: the bass root (MIDI, relative to an A tonic) and
/// the chord tones the lead arpeggiates over it.
struct Bar {
    root: i32,
    tones: &'static [i32],
}

/// The fixed musical content of a [`Mood`]: everything the synth reads.
struct Voicing {
    bars: &'static [Bar],
    /// 16 steps, one bar; `x` plays the bass root, `.` rests.
    bass_gate: &'static [u8; 16],
    /// 16 steps; each indexes into the current bar's chord tones.
    lead_pat: &'static [usize; 16],
    bass_duty: f64,
    bass_gain: f32,
    lead_duty: f64,
    lead_vib: f64,
    lead_gain: f32,
    /// Semitones the lead sits above the written chord tone (the chip "bite").
    lead_octave: i32,
    kick_gain: f32,
    hat_gain: f32,
}

impl Voicing {
    fn of(mood: Mood) -> Voicing {
        match mood {
            // Faithful to chiptune.py: Am F C G, the reference gates/patterns.
            Mood::Funk => Voicing {
                bars: &[
                    Bar { root: 45, tones: &[57, 60, 64, 69] }, // Am : A C E A
                    Bar { root: 41, tones: &[57, 60, 65, 69] }, // F  : A C F A
                    Bar { root: 48, tones: &[60, 64, 67, 72] }, // C  : C E G C
                    Bar { root: 43, tones: &[59, 62, 67, 71] }, // G  : B D G B
                ],
                bass_gate: b"x.x.x..xx.x.x.x.",
                lead_pat: &[0, 2, 1, 3, 2, 3, 1, 2, 0, 2, 3, 2, 1, 3, 2, 3],
                bass_duty: 0.5,
                bass_gain: 0.42,
                lead_duty: 0.30,
                lead_vib: 2.0,
                lead_gain: 0.20,
                lead_octave: 12,
                kick_gain: 0.5,
                hat_gain: 0.14,
            },
            // Lusher sevenths, a sparse dotted bass and a fuller, barely-vibrato
            // lead an octave lower than the funk's — the same voices, calmer.
            Mood::Dreamy => Voicing {
                bars: &[
                    Bar { root: 45, tones: &[57, 60, 64, 67] }, // Am7 : A C E G
                    Bar { root: 41, tones: &[57, 60, 65, 69] }, // Fma7: A C F A
                    Bar { root: 48, tones: &[59, 64, 67, 71] }, // Cma7: B E G B
                    Bar { root: 43, tones: &[57, 62, 66, 69] }, // G   : A D F# A
                ],
                bass_gate: b"x.......x...x...",
                lead_pat: &[0, 1, 2, 3, 2, 1, 0, 1, 2, 3, 2, 3, 1, 2, 3, 2],
                bass_duty: 0.5,
                bass_gain: 0.40,
                lead_duty: 0.5,
                lead_vib: 1.0,
                lead_gain: 0.16,
                lead_octave: 0,
                kick_gain: 0.34,
                hat_gain: 0.08,
            },
            // A-E-F#m-D (I-V-vi-IV), the bright four-chord climb every hopeful
            // title screen leans on — a progression, not a melody, so it is
            // free to reuse. Written on the same A tonic Funk and Dreamy use,
            // so `key: 0` (the default) reads as "A" and actually means it —
            // unlike those two, Title's "A" is a major tonic, not a minor
            // one. A steady marching bass, a wider-duty lead than Funk's
            // (fuller, more "brass") with a touch more vibrato for sparkle,
            // and a harder-hitting kick for a fanfare feel.
            Mood::Title => Voicing {
                bars: &[
                    Bar { root: 45, tones: &[57, 61, 64, 69] }, // A   : A C# E A
                    Bar { root: 40, tones: &[52, 56, 59, 64] }, // E   : E G# B E
                    Bar { root: 42, tones: &[54, 57, 61, 66] }, // F#m : F# A C# F#
                    Bar { root: 38, tones: &[50, 54, 57, 62] }, // D   : D F# A D
                ],
                bass_gate: b"x...x...x...x...",
                lead_pat: &[0, 1, 2, 3, 3, 2, 1, 0, 0, 1, 2, 3, 3, 2, 1, 0],
                bass_duty: 0.5,
                bass_gain: 0.38,
                lead_duty: 0.35,
                lead_vib: 3.0,
                lead_gain: 0.24,
                lead_octave: 12,
                kick_gain: 0.55,
                hat_gain: 0.12,
            },
        }
    }
}

/// How the tune's tempo relates to the film.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MusicFit {
    /// Play at exactly the authored `bpm`; the tune is generated to fill the
    /// film's length but its beats land wherever the tempo puts them.
    #[default]
    Free,
    /// Treat `bpm` as a target and nudge it so a whole number of bars spans the
    /// track — the last downbeat lands on the film's end, and the bar grid is
    /// regular against the film. See the module docs.
    Film,
}

impl MusicFit {
    fn parse(s: &str) -> MusicFit {
        match s.trim().to_ascii_lowercase().as_str() {
            "film" | "fit" => MusicFit::Film,
            _ => MusicFit::Free,
        }
    }

    /// The fit's canonical name — the word that names it in a film file.
    pub fn name(self) -> &'static str {
        match self {
            MusicFit::Free => "free",
            MusicFit::Film => "film",
        }
    }
}

/// How present one voice is in a bar. Not a volume knob — [`Intensity`] decides
/// *whether a voice speaks this bar and how often*, using the mood's own
/// step patterns, so a sparse section still plays the mood's music rather than
/// a quieter copy of the full arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceLevel {
    /// Silent this section.
    #[default]
    Off,
    /// One hit a bar — the downbeat only. On the drums (kick/hat) this is a
    /// lonely repeated *percussive* beat; the drums are noise-burst and
    /// therefore unpitched (see [`kick`] and [`noise`]), so a [`Sparse`]
    /// kick alone carries no tonal content at all — right for a rhythm
    /// building under a tune, wrong for the first thing an opener sounds.
    /// For a lonely *note* to open on, use [`VoiceLevel::Held`] on the bass
    /// or lead instead.
    ///
    /// [`Sparse`]: VoiceLevel::Sparse
    Sparse,
    /// One long, decaying tone a bar — the downbeat rings and fades across
    /// the whole bar rather than clicking on and off. Only meaningful on the
    /// two pitched voices (bass/lead, via [`held_tone`]); on the drums it has
    /// nothing to ring, so it is silent, the same as [`VoiceLevel::Off`].
    /// This is what a slow opener wants to open *on* — a single held pitched
    /// note, not a percussive hit.
    Held,
    /// The mood's own pattern for this voice, unabridged — what every voice
    /// plays when no arrangement is declared at all.
    Full,
}

impl VoiceLevel {
    fn parse(s: &str) -> VoiceLevel {
        match s.trim().to_ascii_lowercase().as_str() {
            "sparse" => VoiceLevel::Sparse,
            "held" => VoiceLevel::Held,
            "full" => VoiceLevel::Full,
            _ => VoiceLevel::Off,
        }
    }

    fn name(self) -> &'static str {
        match self {
            VoiceLevel::Off => "off",
            VoiceLevel::Sparse => "sparse",
            VoiceLevel::Held => "held",
            VoiceLevel::Full => "full",
        }
    }
}

/// Which of the three voices play in a bar, and how densely — the knob an
/// [`Section`] turns. Named presets cover the common shapes; the object form
/// sets each voice independently.
///
/// ```jsonc
/// "intensity": "tone"    // a single held, decaying note — open an opener on this, not a drum
/// "intensity": "kick"    // just the lonely downbeat kick — a rhythm, not a note
/// "intensity": "full"    // the whole mood, unabridged — the default
/// "intensity": { "bass": "held", "kick": "off", "hat": "off" }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intensity {
    pub bass: VoiceLevel,
    pub lead: VoiceLevel,
    pub kick: VoiceLevel,
    pub hat: VoiceLevel,
}

impl Intensity {
    /// Nothing plays — a held silence, useful before the first hit of an
    /// opener or as a dramatic beat between sections.
    pub const SILENCE: Intensity =
        Intensity { bass: VoiceLevel::Off, lead: VoiceLevel::Off, kick: VoiceLevel::Off, hat: VoiceLevel::Off };
    /// Every voice at its mood's own full pattern — what a track with no
    /// arrangement plays throughout, and what the whole tune returns to once
    /// it "really gets going".
    pub const FULL: Intensity =
        Intensity { bass: VoiceLevel::Full, lead: VoiceLevel::Full, kick: VoiceLevel::Full, hat: VoiceLevel::Full };
    /// A single held, decaying note a bar — bass and lead both ring the same
    /// pitch class an octave apart (see [`VoiceLevel::Held`]), no drums at
    /// all. **This is what a slow opener should open on**, not
    /// [`Intensity::KICK`]: the drums are noise-burst and unpitched, so a
    /// kick-only intro carries no tonal content — it reads as a slap, not a
    /// beat. A tone rings and decays across the bar and the theme's own lead
    /// voice later grows out of the same note.
    pub const TONE: Intensity =
        Intensity { bass: VoiceLevel::Held, lead: VoiceLevel::Held, kick: VoiceLevel::Off, hat: VoiceLevel::Off };
    /// Just the kick, once a bar — a lonely percussive pulse with nothing
    /// else playing. Unpitched (the kick is a noise burst, see [`kick`]): a
    /// rhythm, not a note. Reach for [`Intensity::TONE`] instead when an
    /// opener wants something to sing on rather than tap along to.
    pub const KICK: Intensity =
        Intensity { bass: VoiceLevel::Off, lead: VoiceLevel::Off, kick: VoiceLevel::Sparse, hat: VoiceLevel::Off };
    /// The kick joined by a light offbeat hat — a little air added to
    /// [`Intensity::KICK`] without yet bringing in the tune. Still unpitched.
    pub const PULSE: Intensity =
        Intensity { bass: VoiceLevel::Off, lead: VoiceLevel::Off, kick: VoiceLevel::Sparse, hat: VoiceLevel::Sparse };
    /// Bass and kick both present (kick at its full four-per-bar pattern),
    /// still no lead — the rhythm section arriving just ahead of the melody,
    /// for the bar or two right before a tune "really gets going".
    pub const BUILD: Intensity =
        Intensity { bass: VoiceLevel::Sparse, lead: VoiceLevel::Off, kick: VoiceLevel::Full, hat: VoiceLevel::Sparse };

    fn named(s: &str) -> Option<Intensity> {
        match s.trim().to_ascii_lowercase().as_str() {
            "silence" | "off" => Some(Intensity::SILENCE),
            "full" => Some(Intensity::FULL),
            "tone" => Some(Intensity::TONE),
            "kick" => Some(Intensity::KICK),
            "pulse" => Some(Intensity::PULSE),
            "build" => Some(Intensity::BUILD),
            _ => None,
        }
    }
}

impl Default for Intensity {
    fn default() -> Self {
        Intensity::FULL
    }
}

/// Whether `level` triggers the bass at step `s` of a bar, given the mood's
/// own gate pattern for that step. [`VoiceLevel::Held`] triggers once a bar
/// like [`VoiceLevel::Sparse`] — the caller distinguishes the two by
/// synthesising a long, ringing [`held_tone`] rather than a short [`pulse`].
fn bass_on(level: VoiceLevel, gated: bool, s: usize) -> bool {
    match level {
        VoiceLevel::Off => false,
        VoiceLevel::Sparse | VoiceLevel::Held => s == 0,
        VoiceLevel::Full => gated,
    }
}

/// The lead plays every step at [`VoiceLevel::Full`] (a continuous arpeggio);
/// at [`VoiceLevel::Sparse`] it thins to one note a beat; at
/// [`VoiceLevel::Held`] to one note a *bar* (see [`bass_on`]).
fn lead_on(level: VoiceLevel, s: usize) -> bool {
    match level {
        VoiceLevel::Off => false,
        VoiceLevel::Sparse => s.is_multiple_of(4),
        VoiceLevel::Held => s == 0,
        VoiceLevel::Full => true,
    }
}

/// The kick sits on every beat at [`VoiceLevel::Full`] (four a bar); at
/// [`VoiceLevel::Sparse`] it drops to once a bar, on the downbeat — a lonely
/// percussive pulse, not a note (the kick is an unpitched noise burst — see
/// [`kick`]). [`VoiceLevel::Held`] has nothing to ring on a drum, so it is
/// silent, same as [`VoiceLevel::Off`]: reach for a bass/lead voice at
/// [`VoiceLevel::Held`] (or [`Intensity::TONE`]) for something pitched to
/// open on instead.
fn kick_on(level: VoiceLevel, s: usize) -> bool {
    match level {
        VoiceLevel::Off | VoiceLevel::Held => false,
        VoiceLevel::Sparse => s == 0,
        VoiceLevel::Full => s.is_multiple_of(4),
    }
}

/// The hat sits on every offbeat eighth at [`VoiceLevel::Full`] (eight a bar);
/// at [`VoiceLevel::Sparse`] it thins to two. [`VoiceLevel::Held`] is silent
/// on the hat for the same reason it is on the kick — see [`kick_on`].
fn hat_on(level: VoiceLevel, s: usize) -> bool {
    match level {
        VoiceLevel::Off | VoiceLevel::Held => false,
        VoiceLevel::Sparse => s % 4 == 1,
        VoiceLevel::Full => s % 2 == 1,
    }
}

/// One stretch of a [`Music`] track's [`arrangement`](Music::arrangement): a
/// length in bars and how present each voice is over it. A film asks for a
/// build the same way it asks for anything else here — as data:
///
/// ```jsonc
/// "arrangement": [
///   { "name": "intro", "bars": 6, "intensity": "tone" },
///   { "name": "build", "bars": 2, "intensity": "build" },
///   { "name": "theme", "bars": 8, "intensity": "full" }
/// ]
/// ```
///
/// A section may also override the tempo (`"bpm"`) — a real tempo lift into
/// the theme, not just a density change — but most arrangements need only
/// `bars` and `intensity`; the track's own `bpm` covers a section that leaves
/// it unset. The chord progression keeps advancing bar over bar through every
/// section, arrangement or not — a build is a change in who is playing, never
/// a change in the mood's own harmony.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// A label carried into the [`MusicManifest`] for the film crew to key
    /// off — `"intro"`, `"build"`, `"theme"`. Cosmetic to the audio itself.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Length in bars (four beats each). Must be at least 1 — see
    /// [`Music::validate`].
    pub bars: u32,
    /// Tempo override for this section only. Unset plays at the track's own
    /// [`Music::effective_bpm`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "is_full_intensity")]
    pub intensity: Intensity,
}

fn is_full_intensity(i: &Intensity) -> bool {
    *i == Intensity::FULL
}

impl Section {
    pub fn new(name: impl Into<String>, bars: u32, intensity: Intensity) -> Self {
        Section { name: name.into(), bars, bpm: None, intensity }
    }

    pub fn bpm(mut self, bpm: f64) -> Self {
        self.bpm = Some(bpm);
        self
    }
}

// --- serde: intensity as a named word, or an object of per-voice levels -------

impl Serialize for Intensity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        IntensityRepr::from(*self).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Intensity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(IntensityRepr::deserialize(d)?.into())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum IntensityRepr {
    Word(String),
    Full(IntensityFull),
}

#[derive(Serialize, Deserialize)]
struct IntensityFull {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bass: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lead: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kick: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hat: Option<String>,
}

impl From<IntensityRepr> for Intensity {
    fn from(r: IntensityRepr) -> Intensity {
        match r {
            // A named preset, or (mirroring `Mood`'s tolerance for a typo) the
            // safest fallback: silence, not a guess at what was meant.
            IntensityRepr::Word(w) => Intensity::named(&w).unwrap_or(Intensity::SILENCE),
            // Each voice defaults to Off when unset — the object form is for
            // deliberate, explicit control, so a voice left out of it is meant
            // to stay quiet rather than quietly inherit Full.
            IntensityRepr::Full(f) => Intensity {
                bass: f.bass.as_deref().map(VoiceLevel::parse).unwrap_or_default(),
                lead: f.lead.as_deref().map(VoiceLevel::parse).unwrap_or_default(),
                kick: f.kick.as_deref().map(VoiceLevel::parse).unwrap_or_default(),
                hat: f.hat.as_deref().map(VoiceLevel::parse).unwrap_or_default(),
            },
        }
    }
}

impl From<Intensity> for IntensityRepr {
    fn from(i: Intensity) -> IntensityRepr {
        // A recognised preset round-trips as its terse word; anything else
        // (a custom mix) is the full object.
        let word = if i == Intensity::SILENCE {
            Some("silence")
        } else if i == Intensity::FULL {
            Some("full")
        } else if i == Intensity::TONE {
            Some("tone")
        } else if i == Intensity::KICK {
            Some("kick")
        } else if i == Intensity::PULSE {
            Some("pulse")
        } else if i == Intensity::BUILD {
            Some("build")
        } else {
            None
        };
        match word {
            Some(w) => IntensityRepr::Word(w.to_string()),
            None => IntensityRepr::Full(IntensityFull {
                bass: Some(i.bass.name().to_string()),
                lead: Some(i.lead.name().to_string()),
                kick: Some(i.kick.name().to_string()),
                hat: Some(i.hat.name().to_string()),
            }),
        }
    }
}

/// One resolved stretch [`Music::render_samples`] and [`Music::manifest`] both
/// walk — either a borrowed [`Section`]'s fields, or the synthetic single
/// section [`Music::plan`] builds when no arrangement is declared.
struct PlanSection<'a> {
    bars: u32,
    bpm: Option<f64>,
    intensity: Intensity,
    name: &'a str,
}

/// Where every beat, bar and section boundary of a rendered track falls — see
/// [`Music::manifest`]. Written beside a standalone export
/// (`showreel music`) and mirrors the shape
/// [`crate::narration::NarrationManifest`] already established: sections
/// nesting the bars they own, each bar nesting its own beat times, so a film
/// crew reads exactly the granularity it needs without flattening anything
/// itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MusicManifest {
    pub mood: String,
    pub key: String,
    pub bpm: f64,
    pub duration: f64,
    pub sections: Vec<SectionTiming>,
    pub bars: Vec<BarBeat>,
}

/// One arrangement section's span on the rendered track's own clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionTiming {
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub bar_start: usize,
    pub bar_count: usize,
}

/// One bar's downbeat and its four beat times, absolute on the track's clock —
/// what a terminal cursor or a logo drop cues off to land exactly on the beat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarBeat {
    pub index: usize,
    pub start: f64,
    pub section: String,
    pub beats: Vec<f64>,
}

/// A generated chiptune. Written in a film file as a bare mood word —
/// `"music": "funk"` — or an object for full control:
///
/// ```jsonc
/// { "music": { "mood": "funk", "key": "C", "bpm": 132, "fit": "film" } }
/// ```
///
/// The bare-word form mirrors [`crate::grade::Grade`]'s `"documentary"`: the
/// common case is the word an author wants to type, not `{"mood": "funk"}`.
///
/// # Arrangement
///
/// By default a track plays every voice at [`Intensity::FULL`] from its first
/// sample — fine for a bed under a scene, wrong for an opener that wants to
/// build. [`Music::arrangement`] gives it structure over time: named
/// [`Section`]s, each a length in bars and how present each voice is, walked
/// in order and looped if the track needs to run longer than the arrangement's
/// own length. An empty arrangement (the default) is exactly the old
/// single-intensity behaviour — this is capability added, not a new mode to
/// opt out of.
#[derive(Debug, Clone, PartialEq)]
pub struct Music {
    pub mood: Mood,
    /// Tonal centre as semitones above A (the reference tonic). Written as a
    /// note name — `"A"`, `"C#"`, `"F"`. Transposes the whole progression.
    pub key: i32,
    /// Tempo in BPM. With [`MusicFit::Film`] this is a target the fit nudges.
    pub bpm: f64,
    pub fit: MusicFit,
    /// Seeds the drums' noise. A fixed default keeps renders reproducible;
    /// change it only to reroll the drum texture.
    pub seed: u64,
    /// Structure over time — see the "Arrangement" section above. Empty means
    /// no structure: every bar plays at [`Intensity::FULL`], the original
    /// behaviour.
    pub arrangement: Vec<Section>,
}

const DEFAULT_SEED: u64 = 0xC17;

impl Default for Music {
    fn default() -> Self {
        Music {
            mood: Mood::Funk,
            key: 0,
            bpm: Mood::Funk.default_bpm(),
            fit: MusicFit::Free,
            seed: DEFAULT_SEED,
            arrangement: Vec::new(),
        }
    }
}

impl Music {
    /// The reference tune, sized to the film.
    pub fn chiptune() -> Self {
        Music::default()
    }

    /// A tune in the named mood, at that mood's own default tempo.
    pub fn mood(mood: Mood) -> Self {
        Music { mood, bpm: mood.default_bpm(), ..Music::default() }
    }

    pub fn key(mut self, semitones_above_a: i32) -> Self {
        self.key = semitones_above_a.rem_euclid(12);
        self
    }

    pub fn bpm(mut self, bpm: f64) -> Self {
        self.bpm = bpm;
        self
    }

    pub fn fit(mut self, fit: MusicFit) -> Self {
        self.fit = fit;
        self
    }

    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Give the track structure over time — see the "Arrangement" section on
    /// [`Music`]'s own docs.
    pub fn arrangement(mut self, sections: impl IntoIterator<Item = Section>) -> Self {
        self.arrangement = sections.into_iter().collect();
        self
    }

    /// The tempo actually used for a track of `duration` seconds, once the
    /// [`MusicFit`] has been applied. Equal to `bpm` under [`MusicFit::Free`].
    pub fn effective_bpm(&self, duration: f64) -> f64 {
        match self.fit {
            MusicFit::Free => self.bpm,
            MusicFit::Film => {
                if duration <= 0.0 || self.bpm <= 0.0 {
                    return self.bpm;
                }
                // A bar is four beats; `240/bpm` seconds. Choose the whole
                // number of bars closest to what the target tempo would give,
                // then solve for the tempo that makes exactly that many fit.
                let bar_len_target = 240.0 / self.bpm;
                let nbars = (duration / bar_len_target).round().max(1.0);
                240.0 * nbars / duration
            }
        }
    }

    /// Seconds per bar for a track of `duration` — the downbeat grid the film's
    /// cuts can be aligned to. Under [`MusicFit::Film`], `duration` is an exact whole
    /// multiple of this.
    pub fn bar_seconds(&self, duration: f64) -> f64 {
        240.0 / self.effective_bpm(duration)
    }

    /// The rules a type cannot carry. `label` names the track in messages.
    pub fn validate(&self, label: &str) -> Vec<String> {
        let mut errs = Vec::new();
        if self.bpm <= 0.0 {
            errs.push(format!("{label}: bpm must be positive"));
        }
        for (i, s) in self.arrangement.iter().enumerate() {
            if s.bars == 0 {
                errs.push(format!("{label}: arrangement section {i} ({:?}) has 0 bars", s.name));
            }
            if let Some(bpm) = s.bpm
                && bpm <= 0.0
            {
                errs.push(format!("{label}: arrangement section {i} ({:?}) has a non-positive bpm", s.name));
            }
        }
        errs
    }

    /// The track's own natural length in seconds, when it has an
    /// [`arrangement`](Self::arrangement): the arrangement's own bars at the
    /// track's authored `bpm`, section tempo overrides included. `None` with
    /// no arrangement — an unstructured tune has no length of its own, only
    /// however long it is asked to fill (a film's length, or an explicit
    /// `--duration`). Used as the default duration for a standalone `showreel
    /// music` export.
    pub fn natural_duration(&self) -> Option<f64> {
        if self.arrangement.is_empty() {
            return None;
        }
        let mut secs = 0.0;
        for s in &self.arrangement {
            let bpm = s.bpm.unwrap_or(self.bpm);
            if bpm <= 0.0 {
                continue;
            }
            secs += s.bars as f64 * (240.0 / bpm);
        }
        Some(secs)
    }

    /// The concrete plan render and [`manifest`](Self::manifest) both walk:
    /// either the authored [`arrangement`](Self::arrangement), or — when none
    /// is declared — one synthetic section spanning [`Intensity::FULL`] for
    /// long enough to cover `duration`. One function so the two can never
    /// disagree about where a bar or a beat falls.
    fn plan(&self, duration: f64, base_bpm: f64) -> Vec<PlanSection<'_>> {
        if !self.arrangement.is_empty() {
            return self
                .arrangement
                .iter()
                .map(|s| PlanSection { bars: s.bars, bpm: s.bpm, intensity: s.intensity, name: s.name.as_str() })
                .collect();
        }
        let bar_len = if base_bpm > 0.0 { 240.0 / base_bpm } else { 1.0 };
        // Enough bars to cover the padded duration the synth renders (see
        // render_samples' one-second ring-out tail), plus one for headroom.
        let bars = (((duration + 1.0) / bar_len).ceil() as u32 + 1).max(1);
        vec![PlanSection { bars, bpm: None, intensity: Intensity::FULL, name: "" }]
    }

    /// Where every beat, bar and section boundary falls — the sidecar a
    /// standalone export or a film's render writes beside the audio, so a
    /// terminal cursor, a logo drop or a narration cue can land on the beat
    /// exactly rather than by hand-timing against a waveform. Pure — walks
    /// the same [`plan`](Self::plan) [`render_samples`](Self::render_samples)
    /// does, so the two can never drift apart.
    pub fn manifest(&self, duration: f64) -> MusicManifest {
        let base_bpm = self.effective_bpm(duration);
        let plan = self.plan(duration, base_bpm);
        let mut bars = Vec::new();
        let mut sections: Vec<SectionTiming> = Vec::new();
        let mut t = 0.0f64;
        let mut bar_index = 0usize;
        'outer: loop {
            for sec in &plan {
                let bpm = sec.bpm.unwrap_or(base_bpm);
                let beat = if bpm > 0.0 { 60.0 / bpm } else { 0.0 };
                let bar_len = 4.0 * beat;
                if bar_len <= 0.0 {
                    break 'outer;
                }
                let sec_start = t;
                let sec_bar_start = bar_index;
                let mut bars_here = 0u32;
                for _ in 0..sec.bars {
                    if t >= duration {
                        break 'outer;
                    }
                    let beats = (0..4).map(|i| round3(t + i as f64 * beat)).collect();
                    bars.push(BarBeat { index: bar_index, start: round3(t), section: sec.name.to_string(), beats });
                    t += bar_len;
                    bar_index += 1;
                    bars_here += 1;
                }
                if bars_here > 0 {
                    sections.push(SectionTiming {
                        name: sec.name.to_string(),
                        start: round3(sec_start),
                        end: round3(t),
                        bar_start: sec_bar_start,
                        bar_count: bars_here as usize,
                    });
                }
            }
        }
        MusicManifest {
            mood: self.mood.name().to_string(),
            key: format_key(self.key),
            bpm: base_bpm,
            duration: round3(duration),
            sections,
            bars,
        }
    }

    /// Synthesise `duration` seconds of the tune as interleaved stereo `i16`
    /// samples at [`SAMPLE_RATE`]. Pure and deterministic — no filesystem, no
    /// ffmpeg — so it compiles and runs on `wasm32` as readily as natively.
    pub fn render_samples(&self, duration: f64) -> Vec<i16> {
        let sr = SAMPLE_RATE as f64;
        let n_out = ((duration.max(0.0)) * sr).round() as usize;
        if n_out == 0 {
            return Vec::new();
        }
        let voicing = Voicing::of(self.mood);
        let base_bpm = self.effective_bpm(duration);
        let plan = self.plan(duration, base_bpm);

        // A one-second tail so a note struck near the end still has room to ring
        // before the buffer is truncated — the same pad chiptune.py used.
        let total = n_out + SAMPLE_RATE as usize;
        let mut mix = vec![0.0f32; total];
        let mut rng = Rng::new(self.seed);

        let mut t = 0.0f64;
        let mut chord_bar = 0usize;
        'outer: loop {
            for sec in &plan {
                let bpm = sec.bpm.unwrap_or(base_bpm);
                if bpm <= 0.0 {
                    break 'outer;
                }
                let beat = 60.0 / bpm;
                let step = beat / 4.0; // a sixteenth note
                for _ in 0..sec.bars {
                    if t >= duration + 1.0 {
                        break 'outer;
                    }
                    let bar = &voicing.bars[chord_bar % voicing.bars.len()];
                    let bar_len = 16.0 * step;
                    for s in 0..16 {
                        let at = t + s as f64 * step;
                        // Bass: root dropped an octave. A short punchy pulse
                        // normally; at Held, one long tone ringing the whole
                        // bar instead — see `held_tone`.
                        if bass_on(sec.intensity.bass, voicing.bass_gate[s] == b'x', s) {
                            let f = midi(bar.root - 12 + self.key);
                            let sig = if sec.intensity.bass == VoiceLevel::Held {
                                held_tone(f, bar_len, voicing.bass_duty, 0.0, sr)
                            } else {
                                pulse(f, step * 0.9, voicing.bass_duty, 0.0, sr)
                            };
                            place(&mut mix, &sig, at, sr, voicing.bass_gain);
                        }
                        // Lead: bright arpeggio, thin duty, a little vibrato,
                        // normally. At Held — one long tone a bar — the pitch
                        // stays the arpeggio's own written root (no extra
                        // `lead_octave` lift, which is tuned for a fast,
                        // cutting-through line, not a long note) at a rounder
                        // duty and gentler vibrato, so a lonely opening note
                        // rings warm rather than thin.
                        if lead_on(sec.intensity.lead, s) {
                            let tone = bar.tones[voicing.lead_pat[s] % bar.tones.len()];
                            let held = sec.intensity.lead == VoiceLevel::Held;
                            let octave = if held { 0 } else { voicing.lead_octave };
                            let f = midi(tone + octave + self.key);
                            let sig = if held {
                                held_tone(f, bar_len, voicing.bass_duty, voicing.lead_vib * 0.5, sr)
                            } else {
                                pulse(f, step * 0.95, voicing.lead_duty, voicing.lead_vib, sr)
                            };
                            place(&mut mix, &sig, at, sr, voicing.lead_gain);
                        }
                        // Drums: kick on the beat, hat on the offbeat
                        // eighths — unpitched noise bursts, never triggered
                        // at Held (see `kick_on`/`hat_on`).
                        if kick_on(sec.intensity.kick, s) {
                            let sig = kick(&mut rng, sr);
                            place(&mut mix, &sig, at, sr, voicing.kick_gain);
                        }
                        if hat_on(sec.intensity.hat, s) {
                            let sig = noise(0.04, 120.0, &mut rng, sr);
                            place(&mut mix, &sig, at, sr, voicing.hat_gain);
                        }
                    }
                    t += bar_len;
                    chord_bar += 1;
                }
            }
        }
        mix.truncate(n_out);

        // Soft limit into headroom, exactly as the reference: normalise to the
        // peak, push through tanh for a gentle knee, then back off to 0.9.
        let peak = mix.iter().fold(0.0f32, |m, &v| m.max(v.abs())).max(1e-6);
        let mut out = Vec::with_capacity(n_out * 2);
        for &v in &mix {
            let limited = ((v / peak) as f64 * 1.3).tanh() * 0.9;
            let s = (limited * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
            out.push(s); // left
            out.push(s); // right (the reference is centred mono)
        }
        out
    }

    /// The tune as a complete 16-bit stereo WAV file, in memory. Pure — the
    /// wasm-friendly half of [`Music::render_to_temp`].
    pub fn wav_bytes(&self, duration: f64) -> Vec<u8> {
        wav_from_i16(&self.render_samples(duration), 2, SAMPLE_RATE)
    }

    /// Synthesise the tune to a WAV on disk and return its path, so it can be
    /// handed to [`crate::audio::Audio::resolve`] like any located asset.
    ///
    /// The file is content-addressed under the OS temp directory: its name is a
    /// hash of the spec, the duration and [`SYNTH_VERSION`], so re-rendering the
    /// same film reuses it and distinct tunes never collide. Written atomically
    /// (a temp name, then a rename) so two concurrent renders cannot read a
    /// half-written file. Left in place afterwards — it is small and its name is
    /// stable, so it self-dedupes rather than accumulating per render.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_to_temp(&self, duration: f64) -> anyhow::Result<std::path::PathBuf> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        SYNTH_VERSION.hash(&mut h);
        self.mood.name().hash(&mut h);
        self.key.hash(&mut h);
        self.bpm.to_bits().hash(&mut h);
        self.fit.name().hash(&mut h);
        self.seed.hash(&mut h);
        for s in &self.arrangement {
            s.name.hash(&mut h);
            s.bars.hash(&mut h);
            s.bpm.map(f64::to_bits).hash(&mut h);
            s.intensity.bass.name().hash(&mut h);
            s.intensity.lead.name().hash(&mut h);
            s.intensity.kick.name().hash(&mut h);
            s.intensity.hat.name().hash(&mut h);
        }
        // Rounded to the sample the synth will actually produce, so a float
        // wobble in `duration` does not spawn a near-identical second file.
        ((duration * SAMPLE_RATE as f64).round() as i64).hash(&mut h);
        let key = h.finish();

        let dir = std::env::temp_dir().join("showreel-music");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}-{key:016x}.wav", self.mood.name()));
        if std::fs::metadata(&path).map(|m| m.len() > 44).unwrap_or(false) {
            return Ok(path); // a complete file from an earlier identical render
        }
        let bytes = self.wav_bytes(duration);
        let tmp = dir.join(format!("{}-{key:016x}.{}.tmp", self.mood.name(), std::process::id()));
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }
}

/// MIDI note number to frequency in Hz. `midi(69)` is A4 = 440 Hz.
fn midi(n: i32) -> f64 {
    440.0 * 2f64.powf((n as f64 - 69.0) / 12.0)
}

/// Round to milliseconds — the same precision
/// [`crate::narration`]'s manifest timings use, plenty for cueing a visual to
/// a beat.
fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// A duty-cycled pulse (square) wave with a short attack/release so a stepped
/// note does not click, and optional vibrato (a 6 Hz wobble). Faithful to
/// `chiptune.py`'s `pulse`, including its `vib / freq` depth shaping.
fn pulse(freq: f64, dur: f64, duty: f64, vib: f64, sr: f64) -> Vec<f32> {
    let n = (dur * sr).round() as usize;
    if n == 0 {
        return Vec::new();
    }
    // 4 ms ramps, never more than half the note.
    let a = ((0.004 * sr).round() as usize).clamp(1, n.max(1) / 2).max(1);
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let ph = if vib != 0.0 {
            (t * freq + vib * (2.0 * PI * 6.0 * t).sin() / freq).rem_euclid(1.0)
        } else {
            (t * freq).rem_euclid(1.0)
        };
        let wave = if ph < duty { 1.0 } else { -1.0 };
        let env = if i < a {
            i as f64 / a as f64
        } else if i >= n - a {
            (n - 1 - i) as f64 / a as f64
        } else {
            1.0
        };
        *o = (wave * env) as f32;
    }
    out
}

/// A single held, ringing tone — [`VoiceLevel::Held`]'s voice, and what an
/// opener should sound its first note on rather than a drum. Unlike
/// [`pulse`]'s flat sustain-then-cutoff envelope (right for a short arpeggio
/// note, but a *held* note on that envelope either drones flatly or clicks
/// off at the end — exactly the "cheap"/"plucky" failure a slow opener can't
/// afford), this one has a soft ~15 ms attack and then decays exponentially
/// across its whole duration, the way a struck bell or a plucked string
/// actually rings out. By the end of `dur` it has faded to roughly an eighth
/// of its peak — audibly still ringing when the next bar's note lands (a
/// little overlap is what a real ringing note does), rather than sitting at
/// full volume the whole way and then being cut off.
fn held_tone(freq: f64, dur: f64, duty: f64, vib: f64, sr: f64) -> Vec<f32> {
    let n = (dur * sr).round() as usize;
    if n == 0 {
        return Vec::new();
    }
    let a = ((0.015 * sr).round() as usize).clamp(1, n.max(1) / 2).max(1);
    // Decay so the tone is down to ~1/8 peak by the end of its written
    // duration — a real, audible ring-out rather than a sustained drone.
    let decay_rate = -(0.125f64.ln()) / dur.max(1e-6);
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let ph = if vib != 0.0 {
            (t * freq + vib * (2.0 * PI * 6.0 * t).sin() / freq).rem_euclid(1.0)
        } else {
            (t * freq).rem_euclid(1.0)
        };
        let wave = if ph < duty { 1.0 } else { -1.0 };
        let attack = if i < a { i as f64 / a as f64 } else { 1.0 };
        let env = attack * (-decay_rate * t).exp();
        *o = (wave * env) as f32;
    }
    out
}

/// White noise decaying at `exp(-decay * t)`.
fn noise(dur: f64, decay: f64, rng: &mut Rng, sr: f64) -> Vec<f32> {
    let n = (dur * sr).round() as usize;
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f64 / sr;
        *o = (rng.uniform() * (-decay * t).exp()) as f32;
    }
    out
}

/// A kick: a fast-decaying noise burst pitched by a 90 Hz sine punch, exactly
/// as `chiptune.py` shapes it.
fn kick(rng: &mut Rng, sr: f64) -> Vec<f32> {
    let mut sig = noise(0.11, 55.0, rng, sr);
    for (i, s) in sig.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let punch = (2.0 * PI * 90.0 * t).sin() * 0.6 + 1.0;
        *s = (*s as f64 * punch) as f32;
    }
    sig
}

/// Add `sig` into `buf` at `at` seconds, scaled by `gain`, clamped to the end.
fn place(buf: &mut [f32], sig: &[f32], at: f64, sr: f64, gain: f32) {
    let i = (at * sr).round() as usize;
    if i >= buf.len() {
        return;
    }
    let j = (i + sig.len()).min(buf.len());
    for k in i..j {
        buf[k] += sig[k - i] * gain;
    }
}

/// A tiny deterministic PRNG (`splitmix64`) so the drums are reproducible — the
/// one place `chiptune.py` reached for a global, non-reproducible RNG.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Offset so a seed of 0 is not a degenerate all-zero state.
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform sample in `[-1, 1)`.
    fn uniform(&mut self) -> f64 {
        // 53 bits of mantissa precision into [0,1), then span to [-1,1).
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        u * 2.0 - 1.0
    }
}

/// Wrap interleaved 16-bit samples in a canonical PCM WAV container.
fn wav_from_i16(samples: &[i16], channels: u16, sample_rate: u32) -> Vec<u8> {
    let bytes_per_sample = 2u32;
    let block_align = channels as u32 * bytes_per_sample;
    let byte_rate = sample_rate * block_align;
    let data_len = (samples.len() as u32) * bytes_per_sample;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    v.extend_from_slice(&1u16.to_le_bytes()); // audio format 1 = PCM
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&(block_align as u16).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// A note name (`"A"`, `"C#"`, `"Fb"`) as semitones above A, or `None`.
fn parse_key(s: &str) -> Option<i32> {
    let s = s.trim();
    let mut chars = s.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    // Semitones from A within an octave.
    let base: i32 = match letter {
        'A' => 0,
        'B' => 2,
        'C' => 3,
        'D' => 5,
        'E' => 7,
        'F' => 8,
        'G' => 10,
        _ => return None,
    };
    let mut acc = 0;
    for c in chars {
        match c {
            '#' | 's' | 'S' => acc += 1,
            'b' | 'B' => acc -= 1,
            _ => return None,
        }
    }
    Some((base + acc).rem_euclid(12))
}

/// Semitones above A as a note name, normalised to sharps.
fn format_key(semitones: i32) -> String {
    const NAMES: [&str; 12] =
        ["A", "A#", "B", "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#"];
    NAMES[semitones.rem_euclid(12) as usize].to_string()
}

// --- serde: a bare mood word, or a full object --------------------------------

impl Serialize for Music {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        MusicRepr::from(self.clone()).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Music {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(MusicRepr::deserialize(d)?.into())
    }
}

/// The wire form of [`Music`]: either a bare word (a mood) or an object.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum MusicRepr {
    Word(String),
    Full(MusicFull),
}

#[derive(Serialize, Deserialize)]
struct MusicFull {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mood: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    arrangement: Vec<Section>,
}

impl From<MusicRepr> for Music {
    fn from(r: MusicRepr) -> Music {
        match r {
            MusicRepr::Word(w) => {
                let mood = Mood::parse(&w);
                Music::mood(mood)
            }
            MusicRepr::Full(f) => {
                let mood = f.mood.as_deref().map(Mood::parse).unwrap_or_default();
                Music {
                    mood,
                    key: f.key.as_deref().and_then(parse_key).unwrap_or(0),
                    bpm: f.bpm.unwrap_or_else(|| mood.default_bpm()),
                    fit: f.fit.as_deref().map(MusicFit::parse).unwrap_or_default(),
                    seed: f.seed.unwrap_or(DEFAULT_SEED),
                    arrangement: f.arrangement,
                }
            }
        }
    }
}

impl From<Music> for MusicRepr {
    fn from(m: Music) -> MusicRepr {
        // Always the object form, like `Grade`: the word is input sugar. Emit
        // the concrete mood/key/bpm/fit so the value round-trips; the seed only
        // when it differs from the default, to keep the common case terse.
        MusicRepr::Full(MusicFull {
            mood: Some(m.mood.name().to_string()),
            key: Some(format_key(m.key)),
            bpm: Some(m.bpm),
            fit: Some(m.fit.name().to_string()),
            seed: (m.seed != DEFAULT_SEED).then_some(m.seed),
            arrangement: m.arrangement,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bare_word_resolves_a_mood() {
        let m: Music = serde_json::from_str("\"funk\"").unwrap();
        assert_eq!(m.mood, Mood::Funk);
        assert_eq!(m.bpm, 128.0);
        let d: Music = serde_json::from_str("\"dreamy\"").unwrap();
        assert_eq!(d.mood, Mood::Dreamy);
        assert_eq!(d.bpm, 96.0, "a bare mood word takes that mood's own tempo");
    }

    #[test]
    fn chiptune_is_an_alias_for_the_default() {
        let m: Music = serde_json::from_str("\"chiptune\"").unwrap();
        assert_eq!(m, Music::chiptune());
    }

    #[test]
    fn an_unknown_word_is_the_default_not_an_error() {
        // Same reasoning as Grade's word shorthand — a typo costs a mood, not
        // a render — but `Mood::is_known` still lets `check` name it.
        let m: Music = serde_json::from_str("\"nonsense\"").unwrap();
        assert_eq!(m.mood, Mood::Funk);
        assert!(!Mood::is_known("nonsense"));
        assert!(Mood::is_known("dreamy"));
    }

    #[test]
    fn the_object_form_round_trips() {
        let m = Music::mood(Mood::Dreamy).key(3).bpm(132.0).fit(MusicFit::Film).seed(99);
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Music>(&s).unwrap(), m);
    }

    #[test]
    fn a_default_music_round_trips_and_omits_the_seed() {
        let m = Music::chiptune();
        let s = serde_json::to_string(&m).unwrap();
        assert!(!s.contains("seed"), "the default seed stays out of the JSON: {s}");
        assert_eq!(serde_json::from_str::<Music>(&s).unwrap(), m);
    }

    #[test]
    fn keys_parse_and_transpose() {
        assert_eq!(parse_key("A"), Some(0));
        assert_eq!(parse_key("C"), Some(3));
        assert_eq!(parse_key("C#"), Some(4));
        assert_eq!(parse_key("Bb"), Some(1));
        assert_eq!(parse_key("F"), Some(8));
        assert_eq!(parse_key("x"), None);
        // Round-trips through the note-name writer.
        for k in 0..12 {
            assert_eq!(parse_key(&format_key(k)), Some(k));
        }
    }

    #[test]
    fn free_fit_leaves_the_tempo_alone() {
        let m = Music::chiptune().bpm(128.0);
        assert_eq!(m.effective_bpm(52.0), 128.0);
    }

    #[test]
    fn film_fit_lands_a_whole_number_of_bars_on_the_length() {
        let m = Music::chiptune().bpm(128.0).fit(MusicFit::Film);
        let duration = 52.0;
        let bar = m.bar_seconds(duration);
        let nbars = duration / bar;
        // Exactly an integer number of bars fills the film, so the final
        // downbeat lands on its end.
        assert!((nbars - nbars.round()).abs() < 1e-9, "nbars = {nbars}");
        // And the nudge stays near the target tempo, never a jarring jump.
        assert!((m.effective_bpm(duration) - 128.0).abs() < 8.0);
    }

    #[test]
    fn synthesis_is_deterministic() {
        let m = Music::chiptune();
        assert_eq!(m.render_samples(2.0), m.render_samples(2.0));
    }

    #[test]
    fn a_different_seed_changes_the_drums_but_not_the_length() {
        let a = Music::chiptune().seed(1);
        let b = Music::chiptune().seed(2);
        let sa = a.render_samples(2.0);
        let sb = b.render_samples(2.0);
        assert_eq!(sa.len(), sb.len(), "same length regardless of seed");
        assert_ne!(sa, sb, "a different seed must reroll the noise");
    }

    #[test]
    fn the_output_length_matches_the_requested_duration() {
        let m = Music::chiptune();
        let s = m.render_samples(3.0);
        // Interleaved stereo: two samples per frame, one frame per sample tick.
        assert_eq!(s.len(), (3.0 * SAMPLE_RATE as f64) as usize * 2);
    }

    #[test]
    fn it_actually_makes_sound() {
        // Not silence: a real signal that uses the headroom.
        let s = Music::chiptune().render_samples(2.0);
        let peak = s.iter().map(|&v| v.unsigned_abs()).max().unwrap_or(0);
        assert!(peak > 8000, "peak sample {peak} is implausibly quiet");
    }

    #[test]
    fn the_wav_header_is_well_formed() {
        let bytes = Music::chiptune().wav_bytes(0.5);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[36..40], b"data");
        // 16-bit stereo at 48 kHz.
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
        assert_eq!(u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]), SAMPLE_RATE);
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
    }

    #[test]
    fn zero_duration_is_empty_not_a_panic() {
        assert!(Music::chiptune().render_samples(0.0).is_empty());
    }

    #[test]
    fn an_empty_arrangement_is_bit_identical_to_the_old_unstructured_tune() {
        // Capability added, not a new mode to opt into: a track with no
        // arrangement must still be exactly what it always was.
        let plain = Music::chiptune();
        let explicitly_empty = Music::chiptune().arrangement(Vec::new());
        assert_eq!(plain.render_samples(3.0), explicitly_empty.render_samples(3.0));
    }

    #[test]
    fn a_silent_section_renders_true_silence() {
        let m = Music::chiptune().arrangement([Section::new("hush", 4, Intensity::SILENCE)]);
        let duration = m.natural_duration().unwrap();
        let s = m.render_samples(duration);
        assert!(s.iter().all(|&x| x == 0), "a SILENCE section must produce no sound at all");
    }

    #[test]
    fn a_sparse_kick_section_makes_sound_but_less_of_it_than_full() {
        let bars = 4;
        let sparse = Music::chiptune().arrangement([Section::new("intro", bars, Intensity::KICK)]);
        let full = Music::chiptune().arrangement([Section::new("theme", bars, Intensity::FULL)]);
        let duration = sparse.natural_duration().unwrap();
        let s = sparse.render_samples(duration);
        let f = full.render_samples(duration);
        assert!(s.iter().any(|&x| x != 0), "a lonely kick still makes some sound");
        let energy = |v: &[i16]| v.iter().map(|&x| (x as i64).abs()).sum::<i64>();
        assert!(
            energy(&s) < energy(&f) / 2,
            "kick-only ({}) must be markedly sparser than the full band ({})",
            energy(&s),
            energy(&f)
        );
    }

    #[test]
    fn intensity_tone_is_pitched_voices_only_no_drums() {
        // The whole fix: TONE must not be another drum-only preset. Bass and
        // lead ring; the (unpitched, noise-burst) drums stay silent.
        assert_eq!(Intensity::TONE.bass, VoiceLevel::Held);
        assert_eq!(Intensity::TONE.lead, VoiceLevel::Held);
        assert_eq!(Intensity::TONE.kick, VoiceLevel::Off);
        assert_eq!(Intensity::TONE.hat, VoiceLevel::Off);
    }

    #[test]
    fn held_tone_rings_through_most_of_its_duration_rather_than_clicking_off() {
        let sig = held_tone(220.0, 2.0, 0.5, 0.0, SAMPLE_RATE as f64);
        let rms = |w: &[f32]| (w.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / w.len().max(1) as f64).sqrt();
        let sr = SAMPLE_RATE as usize;
        let onset = &sig[sr / 10..sr / 5]; // 0.1s-0.2s in: past the attack ramp
        let near_end = &sig[sig.len() - sr / 5..]; // the last 0.2s of a 2s note
        let (onset_rms, end_rms) = (rms(onset), rms(near_end));
        assert!(end_rms > 0.0, "a held tone must still be audible near the end, not silent");
        assert!(
            end_rms < onset_rms * 0.5,
            "a held tone must audibly decay: onset_rms={onset_rms:.4} end_rms={end_rms:.4}"
        );
        // And it must not simply click to zero: the true final samples are not
        // all zero (a linear-ramp release would end flush at 0).
        assert!(sig[sig.len() - 1] != 0.0 || sig[sig.len() - 2] != 0.0, "must fade, not click to a hard stop");
    }

    #[test]
    fn a_tone_section_rings_through_the_bar_unlike_a_kick_section() {
        // The captain's own complaint, made numeric: a KICK bar's tail is
        // near-silent (the noise burst has fully decayed); a TONE bar's tail
        // is still clearly sounding, because it is a held note, not a slap.
        let bpm = 100.0;
        let tone = Music::chiptune().bpm(bpm).arrangement([Section::new("intro", 1, Intensity::TONE)]);
        let kick = Music::chiptune().bpm(bpm).arrangement([Section::new("intro", 1, Intensity::KICK)]);
        let bar_len = tone.natural_duration().unwrap();
        let ts = tone.render_samples(bar_len);
        let ks = kick.render_samples(bar_len);
        // Last third of the buffer (both — identical — channels; the ratio
        // is unaffected by including both).
        let tail_of = |v: &[i16]| -> i64 {
            let start = v.len() * 2 / 3;
            v[start..].iter().map(|&x| (x as i64).abs()).sum()
        };
        let (tone_tail, kick_tail) = (tail_of(&ts), tail_of(&ks));
        assert!(
            tone_tail > kick_tail * 8,
            "a held tone must still clearly sound in the bar's tail (tone={tone_tail}, kick={kick_tail}); \
             a bar's worth of near-silence there is the original bug"
        );
    }

    #[test]
    fn a_tone_section_still_makes_sound_and_the_manifest_is_unaffected_by_intensity() {
        // Voice choice must never change the timing grid — the whole point of
        // sharing one `plan()` between render_samples and manifest.
        let tone_track = Music::chiptune().bpm(110.0).arrangement([
            Section::new("intro", 4, Intensity::TONE),
            Section::new("theme", 4, Intensity::FULL),
        ]);
        let kick_track = Music::chiptune().bpm(110.0).arrangement([
            Section::new("intro", 4, Intensity::KICK),
            Section::new("theme", 4, Intensity::FULL),
        ]);
        let duration = tone_track.natural_duration().unwrap();
        assert_eq!(duration, kick_track.natural_duration().unwrap());
        let tone_man = tone_track.manifest(duration);
        let kick_man = kick_track.manifest(duration);
        assert_eq!(tone_man.bars.len(), kick_man.bars.len());
        for (a, b) in tone_man.bars.iter().zip(kick_man.bars.iter()) {
            assert_eq!(a.start, b.start);
            assert_eq!(a.beats, b.beats);
        }
        assert_eq!(tone_man.sections, kick_man.sections);
        let peak = tone_track.render_samples(duration).iter().map(|&v| v.unsigned_abs()).max().unwrap_or(0);
        assert!(peak > 8000, "a tone-opened track must still make real sound, peak={peak}");
    }

    #[test]
    fn tone_arrangement_round_trips_through_json() {
        let m = Music::mood(Mood::Title).bpm(104.0).arrangement([
            Section::new("intro", 6, Intensity::TONE),
            Section::new("build", 2, Intensity::BUILD),
            Section::new("theme", 8, Intensity::FULL),
        ]);
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Music>(&s).unwrap(), m);
        assert!(s.contains("\"tone\""), "the TONE preset must round-trip as its terse word: {s}");
    }

    #[test]
    fn natural_duration_sums_the_arrangements_bars_at_the_tracks_bpm() {
        let m = Music::chiptune().bpm(120.0).arrangement([
            Section::new("a", 2, Intensity::KICK),
            Section::new("b", 4, Intensity::FULL),
        ]);
        // 6 bars at 120 bpm: a bar is 4 beats, 240/bpm seconds each = 2s/bar.
        assert_eq!(m.natural_duration(), Some(12.0));
    }

    #[test]
    fn natural_duration_respects_a_sections_own_tempo_override() {
        let m = Music::chiptune().bpm(120.0).arrangement([
            Section::new("slow", 2, Intensity::KICK).bpm(60.0), // 240/60 = 4s/bar
            Section::new("fast", 2, Intensity::FULL),           // 240/120 = 2s/bar
        ]);
        assert_eq!(m.natural_duration(), Some(2.0 * 4.0 + 2.0 * 2.0));
    }

    #[test]
    fn natural_duration_is_none_without_an_arrangement() {
        assert_eq!(Music::chiptune().natural_duration(), None);
    }

    #[test]
    fn an_arrangement_shorter_than_the_requested_duration_loops() {
        // A single one-bar section, asked to fill several times its own length,
        // must keep sounding for the whole requested duration rather than
        // trailing off to silence once its one bar has played.
        let one_bar = Music::chiptune().bpm(120.0).arrangement([Section::new("loop", 1, Intensity::FULL)]);
        let duration = 10.0; // one bar at 120bpm is 2s, so this loops 5x
        let s = one_bar.render_samples(duration);
        let sr = SAMPLE_RATE as usize;
        let tail = &s[s.len() - sr * 2..]; // last second, stereo
        assert!(tail.iter().any(|&x| x != 0), "the arrangement must still be sounding near the end");
    }

    #[test]
    fn the_manifest_agrees_with_the_arrangement_bar_by_bar() {
        let m = Music::chiptune().bpm(120.0).arrangement([
            Section::new("intro", 2, Intensity::KICK),
            Section::new("theme", 3, Intensity::FULL),
        ]);
        let duration = m.natural_duration().unwrap();
        let man = m.manifest(duration);
        assert_eq!(man.bars.len(), 5, "2 + 3 bars");
        assert_eq!(man.sections.len(), 2);
        assert_eq!(man.sections[0].name, "intro");
        assert_eq!(man.sections[0].bar_count, 2);
        assert_eq!(man.sections[1].name, "theme");
        assert_eq!(man.sections[1].bar_count, 3);
        // First bar starts on the downbeat at t=0.
        assert_eq!(man.bars[0].start, 0.0);
        assert_eq!(man.bars[0].beats[0], 0.0);
        // A bar at 120bpm is 2s; each of its 4 beats is 0.5s apart.
        assert_eq!(man.bars[0].beats, vec![0.0, 0.5, 1.0, 1.5]);
        assert_eq!(man.bars[1].start, 2.0);
        // The theme section starts right where the intro's 2 bars end.
        assert_eq!(man.sections[1].start, 4.0);
        assert_eq!(man.sections[0].end, man.sections[1].start);
        assert!((man.duration - duration).abs() < 1e-9);
    }

    #[test]
    fn the_manifest_never_reports_a_bar_at_or_past_the_requested_duration() {
        let m = Music::chiptune().bpm(140.0).arrangement([Section::new("loop", 1, Intensity::FULL)]);
        let man = m.manifest(7.0);
        assert!(man.bars.last().unwrap().start < 7.0);
        for b in &man.bars {
            assert!(b.start < 7.0);
        }
    }

    #[test]
    fn section_bpm_override_reaches_the_manifest() {
        let m = Music::chiptune().bpm(120.0).arrangement([
            Section::new("slow", 1, Intensity::KICK).bpm(60.0), // 4s bar
            Section::new("fast", 1, Intensity::FULL),           // 2s bar (base bpm)
        ]);
        let man = m.manifest(m.natural_duration().unwrap());
        assert_eq!(man.bars[0].beats, vec![0.0, 1.0, 2.0, 3.0], "60bpm: 1s/beat");
        assert_eq!(man.bars[1].start, 4.0);
        assert_eq!(man.bars[1].beats, vec![4.0, 4.5, 5.0, 5.5], "120bpm: 0.5s/beat");
    }

    #[test]
    fn intensity_named_presets_round_trip_to_their_word() {
        for (word, preset) in [
            ("silence", Intensity::SILENCE),
            ("full", Intensity::FULL),
            ("tone", Intensity::TONE),
            ("kick", Intensity::KICK),
            ("pulse", Intensity::PULSE),
            ("build", Intensity::BUILD),
        ] {
            let parsed: Intensity = serde_json::from_str(&format!("{word:?}")).unwrap();
            assert_eq!(parsed, preset, "{word}");
            let s = serde_json::to_string(&preset).unwrap();
            assert_eq!(s, format!("{word:?}"), "{word} must round-trip to its own word");
        }
    }

    #[test]
    fn a_custom_intensity_object_defaults_unset_voices_to_off() {
        let i: Intensity = serde_json::from_str(r#"{"kick":"sparse"}"#).unwrap();
        assert_eq!(i, Intensity::KICK, "an unset voice in the object form stays off, not full");
    }

    #[test]
    fn an_unrecognised_intensity_word_falls_back_to_silence_not_a_guess() {
        let i: Intensity = serde_json::from_str("\"chaos\"").unwrap();
        assert_eq!(i, Intensity::SILENCE);
    }

    #[test]
    fn arrangement_round_trips_through_json() {
        let m = Music::mood(Mood::Title).bpm(150.0).arrangement([
            Section::new("intro", 6, Intensity::KICK),
            Section::new("build", 2, Intensity::BUILD).bpm(120.0),
            Section::new("theme", 8, Intensity::FULL),
        ]);
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Music>(&s).unwrap(), m);
    }

    #[test]
    fn validation_catches_a_zero_length_section_and_a_bad_section_tempo() {
        let m = Music::chiptune()
            .arrangement([Section::new("oops", 0, Intensity::FULL), Section::new("bad", 2, Intensity::FULL).bpm(-5.0)]);
        let errs = m.validate("music");
        assert!(errs.iter().any(|e| e.contains("0 bars")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("non-positive bpm")), "{errs:?}");
    }

    #[test]
    fn the_title_mood_is_a_distinct_bright_progression_and_makes_sound() {
        let s = Music::mood(Mood::Title).render_samples(2.0);
        let peak = s.iter().map(|&v| v.unsigned_abs()).max().unwrap_or(0);
        assert!(peak > 8000, "peak sample {peak} is implausibly quiet");
        assert_ne!(
            Music::mood(Mood::Title).render_samples(2.0),
            Music::mood(Mood::Funk).render_samples(2.0),
            "title must not just be funk with a label change"
        );
        assert!(Mood::is_known("title"));
    }

    // A/B listening aid, not a CI test: writes WAVs to $SR_MUSIC_OUT for a
    // human (or a spectrogram) to compare against `chiptune.py`. Ignored by
    // default. Run: `SR_MUSIC_OUT=/path cargo test --lib write_demo_wavs -- --ignored`.
    #[test]
    #[ignore]
    fn write_demo_wavs() {
        let dir = std::env::var("SR_MUSIC_OUT").unwrap_or_else(|_| ".".into());
        let cases: &[(&str, Music, f64)] = &[
            ("rust_funk", Music::chiptune(), 15.0),
            ("rust_funk_fit", Music::chiptune().fit(MusicFit::Film), 15.0),
            ("rust_funk_C", Music::chiptune().key(3), 15.0),
            ("rust_dreamy", Music::mood(Mood::Dreamy), 15.0),
        ];
        for (name, m, dur) in cases {
            let bytes = m.wav_bytes(*dur);
            let path = format!("{dir}/{name}.wav");
            std::fs::write(&path, &bytes).unwrap();
            println!("wrote {path} ({:.1}s, {} bpm)", dur, m.effective_bpm(*dur).round());
        }
    }
}
