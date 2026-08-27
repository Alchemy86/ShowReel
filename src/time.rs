//! Time, expressed the way a person describes a film.
//!
//! Remotion makes frames the authoring unit — `durationInFrames={90}` — which
//! means every edit is an arithmetic problem and changing the frame rate
//! rewrites the whole film. ShowReel authors in **seconds** and resolves to
//! exact frames once, at render. Frame-exactness is kept; the arithmetic is
//! not the author's job.

use serde::{Deserialize, Serialize};

/// A point in time, or a duration, in seconds.
///
/// Deserialises from a bare number (`2.5`) or from a string with a unit
/// (`"2.5s"`, `"250ms"`, `"90f"`), so a film written by hand reads well and a
/// film written by a program stays simple.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, Default)]
#[serde(from = "TimeRepr", into = "TimeRepr")]
pub struct Time(pub f64);

impl Time {
    pub const ZERO: Time = Time(0.0);

    pub fn secs(s: f64) -> Self {
        Time(s)
    }

    pub fn ms(ms: f64) -> Self {
        Time(ms / 1000.0)
    }

    /// Seconds as a plain float.
    pub fn as_secs(self) -> f64 {
        self.0
    }

    /// The frame index this instant falls on, at `fps`.
    ///
    /// Rounds rather than truncates so that a cut written at `2.0s` with 60fps
    /// lands on frame 120 and not 119 through float drift.
    pub fn to_frame(self, fps: f64) -> i64 {
        (self.0 * fps).round() as i64
    }

    /// How many frames a span of this length occupies at `fps`.
    pub fn frame_count(self, fps: f64) -> u32 {
        self.to_frame(fps).max(0) as u32
    }

    pub fn from_frame(frame: i64, fps: f64) -> Self {
        Time(frame as f64 / fps)
    }
}

impl std::ops::Add for Time {
    type Output = Time;
    fn add(self, o: Time) -> Time {
        Time(self.0 + o.0)
    }
}

impl std::ops::Sub for Time {
    type Output = Time;
    fn sub(self, o: Time) -> Time {
        Time(self.0 - o.0)
    }
}

impl std::ops::Mul<f64> for Time {
    type Output = Time;
    fn mul(self, k: f64) -> Time {
        Time(self.0 * k)
    }
}

impl From<f64> for Time {
    fn from(v: f64) -> Time {
        Time(v)
    }
}

impl From<i32> for Time {
    fn from(v: i32) -> Time {
        Time(v as f64)
    }
}

/// The wire form of [`Time`]: a number, or a string carrying its unit.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum TimeRepr {
    Secs(f64),
    Tagged(String),
}

impl From<TimeRepr> for Time {
    fn from(r: TimeRepr) -> Time {
        match r {
            TimeRepr::Secs(s) => Time(s),
            TimeRepr::Tagged(s) => parse_time(&s).unwrap_or(Time::ZERO),
        }
    }
}

impl From<Time> for TimeRepr {
    fn from(t: Time) -> TimeRepr {
        TimeRepr::Secs(t.0)
    }
}

/// `"2.5s"`, `"250ms"`, `"90f"` (frames at 60fps reference), or a bare number.
///
/// `f` is deliberately referenced to 60fps rather than the film's own rate:
/// a duration written in frames is almost always copied off a source clip, and
/// silently re-meaning it when the film's rate changes is the trap this module
/// exists to avoid. Prefer seconds.
pub fn parse_time(s: &str) -> Option<Time> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("ms") {
        return v.trim().parse::<f64>().ok().map(Time::ms);
    }
    if let Some(v) = s.strip_suffix('s') {
        return v.trim().parse::<f64>().ok().map(Time);
    }
    if let Some(v) = s.strip_suffix('f') {
        return v.trim().parse::<f64>().ok().map(|f| Time(f / 60.0));
    }
    s.parse::<f64>().ok().map(Time)
}

/// A half-open span `[start, start + duration)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: Time,
    pub duration: Time,
}

impl Span {
    pub fn new(start: impl Into<Time>, duration: impl Into<Time>) -> Self {
        Span { start: start.into(), duration: duration.into() }
    }

    pub fn end(&self) -> Time {
        self.start + self.duration
    }

    pub fn contains(&self, t: Time) -> bool {
        t >= self.start && t < self.end()
    }

    /// Time since this span began — the "local time" a nested element sees.
    pub fn local(&self, t: Time) -> Time {
        t - self.start
    }

    /// Position through the span, 0..=1, clamped.
    pub fn progress(&self, t: Time) -> f64 {
        if self.duration.0 <= 0.0 {
            return 1.0;
        }
        ((t.0 - self.start.0) / self.duration.0).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_rather_than_truncate() {
        // 2.0s at 60fps is frame 120 exactly, even through float multiplication.
        assert_eq!(Time::secs(2.0).to_frame(60.0), 120);
        // 0.1s at 30fps is 3 frames, not 2.
        assert_eq!(Time::secs(0.1).to_frame(30.0), 3);
    }

    #[test]
    fn parses_units() {
        assert_eq!(parse_time("2.5s"), Some(Time(2.5)));
        assert_eq!(parse_time("250ms"), Some(Time(0.25)));
        assert_eq!(parse_time("90f"), Some(Time(1.5)));
        assert_eq!(parse_time("4"), Some(Time(4.0)));
    }

    #[test]
    fn span_local_time_starts_at_zero() {
        let s = Span::new(3.0, 2.0);
        assert_eq!(s.local(Time(3.0)), Time(0.0));
        assert_eq!(s.progress(Time(4.0)), 0.5);
        assert!(s.contains(Time(3.0)));
        assert!(!s.contains(Time(5.0)));
    }
}
