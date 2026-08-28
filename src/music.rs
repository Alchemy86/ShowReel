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
const SYNTH_VERSION: u32 = 1;

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
}

impl Mood {
    fn parse(s: &str) -> Mood {
        match s.trim().to_ascii_lowercase().as_str() {
            // "chiptune" reads as "the default chiptune" for someone who does
            // not yet know the mood names — it resolves to the reference tune.
            "funk" | "chiptune" | "" => Mood::Funk,
            "dreamy" => Mood::Dreamy,
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
        }
    }

    /// Whether `s` names a mood this synth knows — so a typo is reported at
    /// load rather than silently swapped for the default.
    pub fn is_known(s: &str) -> bool {
        matches!(s.trim().to_ascii_lowercase().as_str(), "funk" | "chiptune" | "dreamy")
    }

    fn default_bpm(self) -> f64 {
        match self {
            Mood::Funk => 128.0,
            Mood::Dreamy => 96.0,
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

/// A generated chiptune. Written in a film file as a bare mood word —
/// `"music": "funk"` — or an object for full control:
///
/// ```jsonc
/// { "music": { "mood": "funk", "key": "C", "bpm": 132, "fit": "film" } }
/// ```
///
/// The bare-word form mirrors [`crate::grade::Grade`]'s `"documentary"`: the
/// common case is the word an author wants to type, not `{"mood": "funk"}`.
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

const DEFAULT_SEED: u64 = 0xC17;

impl Default for Music {
    fn default() -> Self {
        Music { mood: Mood::Funk, key: 0, bpm: Mood::Funk.default_bpm(), fit: MusicFit::Free, seed: DEFAULT_SEED }
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
        errs
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
        let bpm = self.effective_bpm(duration);
        let beat = 60.0 / bpm;
        let step = beat / 4.0; // a sixteenth note
        let bar_len = 16.0 * step;
        let loop_len = voicing.bars.len() as f64 * bar_len;

        // A one-second tail so a note struck near the end still has room to ring
        // before the buffer is truncated — the same pad chiptune.py used.
        let total = n_out + SAMPLE_RATE as usize;
        let mut mix = vec![0.0f32; total];
        let mut rng = Rng::new(self.seed);

        let nloops = ((duration / loop_len).ceil() as usize) + 1;
        for l in 0..nloops {
            for (b, bar) in voicing.bars.iter().enumerate() {
                let bar_t = l as f64 * loop_len + b as f64 * bar_len;
                if bar_t >= duration + 1.0 {
                    break;
                }
                for s in 0..16 {
                    let at = bar_t + s as f64 * step;
                    // Bass: root dropped an octave, punchy short pulse.
                    if voicing.bass_gate[s] == b'x' {
                        let f = midi(bar.root - 12 + self.key);
                        let sig = pulse(f, step * 0.9, voicing.bass_duty, 0.0, sr);
                        place(&mut mix, &sig, at, sr, voicing.bass_gain);
                    }
                    // Lead: bright arpeggio, thin duty, a little vibrato.
                    let tone = bar.tones[voicing.lead_pat[s] % bar.tones.len()];
                    let f = midi(tone + voicing.lead_octave + self.key);
                    let sig = pulse(f, step * 0.95, voicing.lead_duty, voicing.lead_vib, sr);
                    place(&mut mix, &sig, at, sr, voicing.lead_gain);
                    // Drums: kick on the beat, hat on the offbeat eighths.
                    if s % 4 == 0 {
                        let sig = kick(&mut rng, sr);
                        place(&mut mix, &sig, at, sr, voicing.kick_gain);
                    }
                    if s % 2 == 1 {
                        let sig = noise(0.04, 120.0, &mut rng, sr);
                        place(&mut mix, &sig, at, sr, voicing.hat_gain);
                    }
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
        MusicRepr::from(*self).serialize(s)
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
