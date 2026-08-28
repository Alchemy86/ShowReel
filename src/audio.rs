//! Sound under the film.
//!
//! # Why a track is placed, not attached
//!
//! Everything else in ShowReel that has a source file — a still, a clip — hangs
//! off a [`Layer`](crate::layer::Layer), because it is *drawn* and therefore
//! belongs to a scene. Sound is not drawn and does not belong to a scene: a
//! theme that carries the opening and settles under the second shot is one
//! track spanning a cut, and expressing that as a property of either scene
//! would be a lie about what it is. So audio hangs off the [`Film`] and is
//! placed on the film's own clock, exactly the way a scene is.
//!
//! The controls are deliberately the same four an author already knows from
//! trimming a clip — where it starts on the film's clock ([`Audio::at`]), where
//! to start reading inside the source ([`Audio::from`]), how much to use
//! ([`Audio::lasting`]) — plus the one thing a clip does not need:
//!
//! > **A track that is still playing when the film ends does not stop, it
//! > *ends*.** [`Audio::fade_out`] exists because the alternative is a film
//! > whose last frame is a hard cut to silence, which sounds like a fault.
//!
//! # What is resolved when
//!
//! [`Audio`] is the *description*: seconds, an asset reference, no filesystem.
//! [`AudioInput`] is what the encoder needs: a located path and every timing
//! resolved to a plain float — in particular `duration`, which may be written
//! as "to the end of the film" and can only become a number once the film's
//! length is known. [`Audio::resolve`] is the single place that conversion
//! happens, for the same reason [`crate::time`] resolves frames exactly once.
//!
//! # Nothing here knows what the sound *is*
//!
//! Per the crate rule, this module has no idea whether it is carrying a music
//! bed, a voice-over or a sound effect. It takes a file and a position.
//!
//! # A clip's own soundtrack
//!
//! [`Content::Clip`](crate::layer::Content::Clip) draws a decoded video frame,
//! but the source file it was decoded from usually carries sound too, and
//! that sound reaches the mix through [`ClipAudio`] rather than through a
//! film-level [`Audio`] track — it is intrinsic to that one clip, not a
//! separately-placed bed. [`clip_track`] is the seam: it turns the clip's own
//! timing (where it sits on the film's clock, where its decoded window starts
//! in the source) plus a [`ClipAudio`] into an [`AudioInput`], by building an
//! ephemeral [`Audio`] and calling [`Audio::resolve`] — so a clip's fades are
//! clamped by exactly the rule a standalone track's already are, rather than
//! a second copy of that arithmetic.

use crate::time::Time;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A piece of audio laid under the film.
///
/// Build one with [`Audio::track`] and place it with the builder methods:
///
/// ```
/// use showreel::audio::Audio;
/// use showreel::time::Time;
///
/// // The source's first 18 seconds, under the film from the start, easing
/// // in over half a second and away over three.
/// let bed = Audio::track("theme.wav").lasting(18.0).fades(0.5, 3.0);
/// assert_eq!(bed.resolve_duration(Time::from(60.0)).as_secs(), 18.0);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Audio {
    /// Asset reference, resolved through [`AssetStore`](crate::assets::AssetStore)
    /// like any other source file.
    pub asset: String,
    /// Where the track starts on the **film's** clock.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub at: Time,
    /// Where to start reading inside the **source** file.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub from: Time,
    /// How much of the source to use. `None` means "to the end of the film".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Time>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_in: Time,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_out: Time,
    /// Linear gain. `1.0` leaves the source alone, `0.5` is half amplitude.
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub gain: f64,
}

fn is_zero(t: &Time) -> bool {
    t.as_secs() == 0.0
}

fn unity() -> f64 {
    1.0
}

fn is_unity(g: &f64) -> bool {
    (*g - 1.0).abs() < 1e-9
}

impl Audio {
    /// A track that plays from the start of the film to the end of it.
    pub fn track(asset: impl Into<String>) -> Self {
        Audio {
            asset: asset.into(),
            at: Time::ZERO,
            from: Time::ZERO,
            duration: None,
            fade_in: Time::ZERO,
            fade_out: Time::ZERO,
            gain: 1.0,
        }
    }

    /// Where the track starts on the film's clock.
    pub fn at(mut self, t: impl Into<Time>) -> Self {
        self.at = t.into();
        self
    }

    /// Where to start reading inside the source file.
    pub fn from(mut self, t: impl Into<Time>) -> Self {
        self.from = t.into();
        self
    }

    /// How much of the source to use.
    pub fn lasting(mut self, d: impl Into<Time>) -> Self {
        self.duration = Some(d.into());
        self
    }

    pub fn fade_in(mut self, d: impl Into<Time>) -> Self {
        self.fade_in = d.into();
        self
    }

    pub fn fade_out(mut self, d: impl Into<Time>) -> Self {
        self.fade_out = d.into();
        self
    }

    /// Both fades at once — the common case.
    pub fn fades(self, in_: impl Into<Time>, out: impl Into<Time>) -> Self {
        self.fade_in(in_).fade_out(out)
    }

    pub fn gain(mut self, g: f64) -> Self {
        self.gain = g;
        self
    }

    /// How long this track actually runs, given the film's length.
    ///
    /// An unset `duration` means "to the end of the film", which is the only
    /// thing that cannot be known while the film is being described.
    pub fn resolve_duration(&self, film: Time) -> Time {
        match self.duration {
            Some(d) => d,
            None => Time((film - self.at).as_secs().max(0.0)),
        }
    }

    /// When this track ends on the film's clock.
    pub fn end(&self, film: Time) -> Time {
        self.at + self.resolve_duration(film)
    }

    /// Locate the source and pin every timing to a number.
    pub fn resolve(&self, path: impl AsRef<Path>, film: Time) -> AudioInput {
        let duration = self.resolve_duration(film).as_secs();
        // A fade cannot be longer than the thing it is fading; clamping here
        // rather than erroring keeps a shortened film from failing to encode.
        let fade_in = self.fade_in.as_secs().clamp(0.0, duration);
        let fade_out = self.fade_out.as_secs().clamp(0.0, duration);
        AudioInput {
            path: path.as_ref().to_path_buf(),
            at: self.at.as_secs().max(0.0),
            from: self.from.as_secs().max(0.0),
            duration,
            fade_in,
            fade_out,
            gain: self.gain.max(0.0),
        }
    }

    /// The rules a type cannot carry. `label` names the track in messages.
    pub fn validate(&self, label: &str, film: Time) -> Vec<String> {
        let mut errs = Vec::new();
        if self.asset.trim().is_empty() {
            errs.push(format!("{label}: needs an asset"));
        }
        if self.at.as_secs() < 0.0 {
            errs.push(format!("{label}: starts before the film does"));
        }
        if self.from.as_secs() < 0.0 {
            errs.push(format!("{label}: reads from before the start of the source"));
        }
        if let Some(d) = self.duration
            && d.as_secs() <= 0.0
        {
            errs.push(format!("{label}: duration must be positive"));
        }
        if self.gain < 0.0 {
            errs.push(format!("{label}: gain must not be negative"));
        }
        let d = self.resolve_duration(film).as_secs();
        if self.fade_in.as_secs() + self.fade_out.as_secs() > d + 1e-9 {
            errs.push(format!(
                "{label}: its fades overlap ({}s + {}s > {d}s)",
                self.fade_in.as_secs(),
                self.fade_out.as_secs()
            ));
        }
        // Silence is a mistake worth naming: a track placed past the end of
        // the film encodes perfectly and is simply never heard.
        if self.at.as_secs() >= film.as_secs() && film.as_secs() > 0.0 {
            errs.push(format!(
                "{label}: starts at {}s, after the film ends at {}s — it will never be heard",
                self.at.as_secs(),
                film.as_secs()
            ));
        }
        errs
    }
}

/// Whether a clip's own soundtrack joins the mix, and at what level.
///
/// Lives on [`Content::Clip`](crate::layer::Content::Clip) rather than as a
/// standalone [`Audio`] track: its position and length are the clip's own —
/// there is no `at`/`from`/`duration` to author separately, only the level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClipAudio {
    /// Draws silently — decoded and drawn as normal, contributes no sound.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    /// Linear gain. `1.0` leaves the source alone, `0.5` is half amplitude —
    /// the lever for ducking a clip's own sound under a voice-over without
    /// silencing it outright.
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub gain: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_in: Time,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_out: Time,
}

impl Default for ClipAudio {
    fn default() -> Self {
        ClipAudio { muted: false, gain: 1.0, fade_in: Time::ZERO, fade_out: Time::ZERO }
    }
}

impl ClipAudio {
    pub fn muted() -> Self {
        ClipAudio { muted: true, ..Default::default() }
    }

    pub fn gain(g: f64) -> Self {
        ClipAudio { gain: g, ..Default::default() }
    }

    pub fn fades(in_: impl Into<Time>, out: impl Into<Time>) -> Self {
        ClipAudio { fade_in: in_.into(), fade_out: out.into(), ..Default::default() }
    }
}

/// Build the mix input for a clip's own soundtrack, or `None` if it is muted
/// or its on-screen window is empty.
///
/// `film_at`/`window` are where the clip layer sits and how long it is on
/// screen, on the film's clock, already clamped to its scene — the same
/// window the video itself draws for. `source_from` is where in the source
/// file that window's first frame comes from (the clip's `trim` start plus
/// its own playhead offset).
///
/// This does not loop a clip's audio to match [`ClipLoop::Loop`]
/// (crate::assets::ClipLoop): a frozen or looping *picture* has no natural
/// audio analogue, so the sound simply runs out when the decoded source does
/// — like a video frozen on its last frame, not a video looping with it.
pub fn clip_track(
    path: impl AsRef<Path>,
    audio: &ClipAudio,
    film_at: Time,
    source_from: Time,
    window: Time,
) -> Option<AudioInput> {
    if audio.muted || window.as_secs() <= 0.0 {
        return None;
    }
    let a = Audio {
        asset: String::new(),
        at: film_at,
        from: source_from,
        duration: Some(window),
        fade_in: audio.fade_in,
        fade_out: audio.fade_out,
        gain: audio.gain,
    };
    Some(a.resolve(path, window))
}

/// One track, located on disk and with every timing resolved to seconds.
///
/// This is the encoder's view. Produced by [`Audio::resolve`].
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInput {
    pub path: PathBuf,
    pub at: f64,
    pub from: f64,
    pub duration: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    pub gain: f64,
}

impl AudioInput {
    /// The trim/gain/fade stages shared by [`filter`](Self::filter) and
    /// [`export_filter`](Self::export_filter) — everything except the
    /// `at`-positioned delay, which only `filter` (mixing straight into a
    /// render) needs; a caller of `export_filter` positions the result itself.
    ///
    /// Built as a `Vec` of links and joined, so that a stage which would be a
    /// no-op is *absent* rather than present-with-neutral-parameters:
    /// `afade` with `d=0` is not a silent no-op in ffmpeg, it is a zero-length
    /// fade that mutes the first sample.
    fn link_chain(&self) -> Vec<String> {
        let mut links: Vec<String> = Vec::new();
        links.push(format!("atrim=start={}:duration={}", self.from, self.duration));
        // atrim leaves the timestamps where they were in the source; without
        // this every trimmed track would be delayed by its own `from`.
        links.push("asetpts=PTS-STARTPTS".into());
        if !is_unity(&self.gain) {
            links.push(format!("volume={}", self.gain));
        }
        if self.fade_in > 0.0 {
            links.push(format!("afade=t=in:st=0:d={}", self.fade_in));
        }
        if self.fade_out > 0.0 {
            let st = (self.duration - self.fade_out).max(0.0);
            links.push(format!("afade=t=out:st={st}:d={}", self.fade_out));
        }
        // Mixing needs one common layout and rate; sources vary.
        links.push("aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo".into());
        links
    }

    /// This track's ffmpeg filter chain, reading ffmpeg input `index` and
    /// producing the named label `[a{slot}]`, positioned at [`Self::at`] via
    /// a trailing `adelay`.
    pub fn filter(&self, index: usize, slot: usize) -> String {
        let mut links = self.link_chain();
        if self.at > 0.0 {
            // adelay is integer milliseconds; `all=1` applies it to every
            // channel, which is what the bare `N|N` form was always meant to say.
            links.push(format!("adelay={}:all=1", (self.at * 1000.0).round() as i64));
        }
        format!("[{index}:a]{}[a{slot}]", links.join(","))
    }

    /// The trimmed, gained, faded audio for this track alone, with no
    /// `adelay` — for pre-rendering one track to its own small file (`showreel
    /// web-pack`, ahead of a browser that has no ffmpeg to mix with) whose
    /// caller positions the result itself, at [`Self::at`] on its own clock.
    pub fn export_filter(&self, index: usize) -> String {
        format!("[{index}:a]{}[a]", self.link_chain().join(","))
    }
}

/// The whole `-filter_complex` for a set of tracks, and the label to `-map`.
///
/// Returns `None` when there is nothing to mix, so a silent film takes exactly
/// the command line it took before audio existed.
///
/// The output is padded with silence (`apad`) so that a track shorter than the
/// film cannot shorten the film: with `-shortest`, the finite side must be the
/// video, and the video is the thing whose length is authoritative.
pub fn mix_filter(tracks: &[AudioInput], first_index: usize) -> Option<(String, String)> {
    if tracks.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = tracks
        .iter()
        .enumerate()
        .map(|(slot, t)| t.filter(first_index + slot, slot))
        .collect();
    let labels: String = (0..tracks.len()).map(|s| format!("[a{s}]")).collect();
    if tracks.len() == 1 {
        parts.push(format!("{labels}apad[aout]"));
    } else {
        // `normalize=0` matters: amix's default divides every input by the
        // number of inputs, so adding a second quiet track would silently
        // halve the first one. Gain is the author's to set, not ffmpeg's.
        parts.push(format!(
            "{labels}amix=inputs={}:normalize=0:dropout_transition=0[amixed]",
            tracks.len()
        ));
        parts.push("[amixed]apad[aout]".into());
    }
    Some((parts.join(";"), "[aout]".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_duration_runs_to_the_end_of_the_film() {
        let a = Audio::track("t.wav").at(2.0);
        assert_eq!(a.resolve_duration(Time(10.0)).as_secs(), 8.0);
        assert_eq!(a.end(Time(10.0)).as_secs(), 10.0);
    }

    #[test]
    fn an_explicit_duration_wins_and_may_end_early() {
        let a = Audio::track("t.wav").at(1.0).lasting(3.0);
        assert_eq!(a.resolve_duration(Time(30.0)).as_secs(), 3.0);
        assert_eq!(a.end(Time(30.0)).as_secs(), 4.0);
    }

    #[test]
    fn fades_are_clamped_to_the_track_rather_than_failing_the_encode() {
        // Shortening a film must not produce an ffmpeg error at the very end
        // of a long render.
        let r = Audio::track("t.wav").fades(10.0, 10.0).resolve("/tmp/t.wav", Time(4.0));
        assert_eq!(r.duration, 4.0);
        assert!(r.fade_in <= 4.0 && r.fade_out <= 4.0);
    }

    #[test]
    fn a_zero_fade_leaves_no_afade_in_the_chain() {
        // afade with d=0 is not a no-op in ffmpeg — it mutes a sample.
        let f = Audio::track("t.wav").resolve("/tmp/t.wav", Time(5.0)).filter(1, 0);
        assert!(!f.contains("afade"), "{f}");
        let g = Audio::track("t.wav").fade_out(1.0).resolve("/tmp/t.wav", Time(5.0)).filter(1, 0);
        assert!(g.contains("afade=t=out:st=4:d=1"), "{g}");
    }

    #[test]
    fn a_track_at_zero_needs_no_delay() {
        let f = Audio::track("t.wav").resolve("/tmp/t.wav", Time(5.0)).filter(1, 0);
        assert!(!f.contains("adelay"), "{f}");
        let g = Audio::track("t.wav").at(1.5).resolve("/tmp/t.wav", Time(5.0)).filter(1, 0);
        assert!(g.contains("adelay=1500:all=1"), "{g}");
    }

    #[test]
    fn the_source_offset_is_a_trim_not_a_delay() {
        // `from` reads later into the file; `at` moves it later in the film.
        // Confusing the two is the classic mistake, so pin both.
        let f = Audio::track("t.wav").from(12.0).at(2.0).lasting(4.0);
        let r = f.resolve("/tmp/t.wav", Time(30.0));
        let chain = r.filter(1, 0);
        assert!(chain.contains("atrim=start=12:duration=4"), "{chain}");
        assert!(chain.contains("adelay=2000"), "{chain}");
    }

    #[test]
    fn export_filter_carries_gain_and_fades_but_never_a_delay() {
        let t = Audio::track("t.wav").at(2.0).gain(0.5).fades(0.5, 0.5).resolve("/tmp/t.wav", Time(5.0));
        let f = t.export_filter(0);
        assert!(f.contains("volume=0.5") && f.contains("afade=t=in") && f.contains("afade=t=out"), "{f}");
        assert!(!f.contains("adelay"), "{f}");
        assert!(f.starts_with("[0:a]") && f.ends_with("[a]"), "{f}");
    }

    #[test]
    fn no_tracks_means_no_filter_at_all() {
        assert!(mix_filter(&[], 1).is_none());
    }

    #[test]
    fn one_track_is_padded_but_not_mixed() {
        let t = Audio::track("t.wav").resolve("/tmp/t.wav", Time(5.0));
        let (f, label) = mix_filter(&[t], 1).unwrap();
        assert_eq!(label, "[aout]");
        assert!(f.contains("[a0]apad[aout]"), "{f}");
        assert!(!f.contains("amix"), "{f}");
    }

    #[test]
    fn several_tracks_mix_without_ffmpeg_rescaling_anyones_volume() {
        let a = Audio::track("a.wav").resolve("/tmp/a.wav", Time(5.0));
        let b = Audio::track("b.wav").at(1.0).resolve("/tmp/b.wav", Time(5.0));
        let (f, _) = mix_filter(&[a, b], 1).unwrap();
        assert!(f.contains("amix=inputs=2:normalize=0"), "{f}");
        // Each track reads its own ffmpeg input.
        assert!(f.contains("[1:a]") && f.contains("[2:a]"), "{f}");
    }

    #[test]
    fn validation_catches_a_track_that_could_never_be_heard() {
        let errs = Audio::track("t.wav").at(30.0).validate("music", Time(10.0));
        assert!(errs.iter().any(|e| e.contains("never be heard")), "{errs:?}");
    }

    #[test]
    fn validation_catches_overlapping_fades() {
        let errs = Audio::track("t.wav").lasting(2.0).fades(1.5, 1.5).validate("music", Time(10.0));
        assert!(errs.iter().any(|e| e.contains("fades overlap")), "{errs:?}");
    }

    #[test]
    fn a_plain_track_validates_clean() {
        assert!(Audio::track("t.wav").fades(0.5, 2.0).validate("music", Time(10.0)).is_empty());
    }

    #[test]
    fn tracks_round_trip_through_json_and_stay_terse() {
        let a = Audio::track("theme.wav").at(1.0).from(2.0).lasting(8.0).fades(0.5, 3.0).gain(0.7);
        let s = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Audio>(&s).unwrap(), a);
        // A default-shaped track carries nothing but its asset.
        let plain = serde_json::to_string(&Audio::track("t.wav")).unwrap();
        assert_eq!(plain, r#"{"asset":"t.wav"}"#);
    }

    #[test]
    fn a_bare_asset_deserialises_to_the_full_film_defaults() {
        let a: Audio = serde_json::from_str(r#"{"asset":"t.wav"}"#).unwrap();
        assert_eq!(a, Audio::track("t.wav"));
        assert_eq!(a.gain, 1.0, "gain must default to unity, not zero");
    }

    #[test]
    fn a_muted_clip_produces_no_track() {
        let a = ClipAudio::muted();
        assert!(clip_track("/tmp/c.mp4", &a, Time(2.0), Time(1.0), Time(5.0)).is_none());
    }

    #[test]
    fn an_empty_window_produces_no_track() {
        let a = ClipAudio::default();
        assert!(clip_track("/tmp/c.mp4", &a, Time(2.0), Time(1.0), Time(0.0)).is_none());
    }

    #[test]
    fn a_clip_track_carries_its_own_position_and_source_offset() {
        let a = ClipAudio::gain(0.5);
        let t = clip_track("/tmp/c.mp4", &a, Time(3.0), Time(1.5), Time(4.0)).unwrap();
        assert_eq!(t.at, 3.0);
        assert_eq!(t.from, 1.5);
        assert_eq!(t.duration, 4.0);
        assert_eq!(t.gain, 0.5);
    }

    #[test]
    fn clip_track_fades_are_clamped_the_same_way_a_standalone_tracks_are() {
        // Reuses Audio::resolve, so an overlong fade against a short window
        // must clamp exactly like the standalone-track test above.
        let a = ClipAudio::fades(10.0, 10.0);
        let t = clip_track("/tmp/c.mp4", &a, Time(0.0), Time(0.0), Time(4.0)).unwrap();
        assert!(t.fade_in <= 4.0 && t.fade_out <= 4.0);
    }

    #[test]
    fn clip_audio_defaults_to_full_gain_unmuted_and_stays_terse_in_json() {
        let a = ClipAudio::default();
        assert!(!a.muted);
        assert_eq!(a.gain, 1.0);
        assert_eq!(serde_json::to_string(&a).unwrap(), "{}");
    }

    #[test]
    fn times_may_be_written_with_units() {
        let a: Audio =
            serde_json::from_str(r#"{"asset":"t.wav","at":"1.5s","fade_out":"250ms"}"#).unwrap();
        assert_eq!(a.at, Time(1.5));
        assert_eq!(a.fade_out, Time(0.25));
    }
}
