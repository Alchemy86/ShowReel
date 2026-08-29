//! How a thing arrives and leaves.
//!
//! Remotion expresses an entrance by writing `interpolate` calls inside a
//! component. That is maximally flexible and maximally repetitive — everyone
//! rewrites the same fade-and-rise. ShowReel names the common ones, because
//! "titles animate in and out" was asked for as a capability rather than as a
//! recipe. [`MotionKind::Chars`] and [`MotionKind::Words`] are the kinetic text
//! the brief named; the rest are the vocabulary an explainer actually uses.
//!
//! [`Motion`] only ever carries a layer *into* or *out of* a fixed placement —
//! `enter`/`exit` are boundary states, and the middle of a layer's life always
//! settles to [`MotionState::SETTLED`]. [`Drift`] fills the gap: continuous
//! motion applied for a layer's *whole* active span, not just its edges. One
//! vector plus an optional grow covers a pan, a parallax-adjacent drift, and —
//! several of these staggered across sibling layers, on fanned headings — a
//! burst. See [`crate::layer::Layer::burst`].

use crate::ease::{Easing, Spring};
use crate::time::Time;
use crate::transition::Timing;
use serde::{Deserialize, Serialize};

/// What an entrance or exit does.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MotionKind {
    /// Opacity only.
    Fade,
    /// Fade while travelling up into place. The default for titles: it reads
    /// as deliberate without drawing attention to itself.
    Rise { distance: f64 },
    /// Fade while travelling down into place.
    Drop { distance: f64 },
    /// Travel in from a side.
    SlideIn { dx: f64, dy: f64 },
    /// Grow (or shrink) into place while fading.
    Scale { from: f64 },
    /// Each character arrives in turn, `stagger` seconds apart.
    Chars { stagger: f64, rise: f64 },
    /// Each word arrives in turn.
    Words { stagger: f64, rise: f64 },
    /// No animation — appear.
    None,
}

/// The transform an in-progress motion applies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionState {
    pub dx: f64,
    pub dy: f64,
    pub scale: f64,
    pub alpha: f64,
}

impl MotionState {
    pub const SETTLED: MotionState = MotionState { dx: 0.0, dy: 0.0, scale: 1.0, alpha: 1.0 };
    pub const HIDDEN: MotionState = MotionState { dx: 0.0, dy: 0.0, scale: 1.0, alpha: 0.0 };

    pub fn is_invisible(&self) -> bool {
        self.alpha <= 0.001 || self.scale <= 0.0
    }
}

/// An entrance or an exit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    #[serde(flatten)]
    pub kind: MotionKind,
    pub duration: Time,
    #[serde(default)]
    pub timing: Timing,
}

impl Motion {
    pub fn new(kind: MotionKind, duration: impl Into<Time>) -> Self {
        Motion { kind, duration: duration.into(), timing: Timing::eased(Easing::OutCubic) }
    }

    pub fn fade(d: impl Into<Time>) -> Self {
        Motion::new(MotionKind::Fade, d)
    }

    /// The default title entrance: up 40px, springing gently into place.
    pub fn rise(d: impl Into<Time>) -> Self {
        Motion::new(MotionKind::Rise { distance: 40.0 }, d)
            .timed(Timing::spring(Spring::gentle()))
    }

    pub fn drop(d: impl Into<Time>) -> Self {
        Motion::new(MotionKind::Drop { distance: 40.0 }, d)
            .timed(Timing::spring(Spring::gentle()))
    }

    pub fn scale_up(d: impl Into<Time>) -> Self {
        Motion::new(MotionKind::Scale { from: 0.86 }, d).timed(Timing::spring(Spring::gentle()))
    }

    pub fn slide_in(d: impl Into<Time>, dx: f64, dy: f64) -> Self {
        Motion::new(MotionKind::SlideIn { dx, dy }, d).timed(Timing::spring(Spring::gentle()))
    }

    /// Per-character. `stagger` is the gap between consecutive characters.
    pub fn chars(d: impl Into<Time>, stagger: f64) -> Self {
        Motion::new(MotionKind::Chars { stagger, rise: 26.0 }, d)
            .timed(Timing::spring(Spring::gentle()))
    }

    pub fn words(d: impl Into<Time>, stagger: f64) -> Self {
        Motion::new(MotionKind::Words { stagger, rise: 30.0 }, d)
            .timed(Timing::spring(Spring::gentle()))
    }

    pub fn timed(mut self, t: Timing) -> Self {
        self.timing = t;
        self
    }

    pub fn eased(mut self, e: Easing) -> Self {
        self.timing = Timing::eased(e);
        self
    }

    pub fn distance(mut self, d: f64) -> Self {
        self.kind = match self.kind {
            MotionKind::Rise { .. } => MotionKind::Rise { distance: d },
            MotionKind::Drop { .. } => MotionKind::Drop { distance: d },
            MotionKind::Chars { stagger, .. } => MotionKind::Chars { stagger, rise: d },
            MotionKind::Words { stagger, .. } => MotionKind::Words { stagger, rise: d },
            k => k,
        };
        self
    }

    /// How long the whole motion takes, including every stagger.
    pub fn total_duration(&self, items: usize) -> f64 {
        let base = self.duration.as_secs();
        match self.kind {
            MotionKind::Chars { stagger, .. } | MotionKind::Words { stagger, .. } => {
                base + stagger * items.saturating_sub(1) as f64
            }
            _ => base,
        }
    }

    fn progress(&self, elapsed: f64) -> f64 {
        let d = self.duration.as_secs();
        if d <= 0.0 {
            return 1.0;
        }
        self.timing.progress((elapsed / d).clamp(0.0, 1.0), elapsed)
    }

    /// The whole-block transform, `elapsed` seconds into an *entrance*.
    pub fn enter_state(&self, elapsed: f64) -> MotionState {
        if elapsed < 0.0 {
            return self.at_start();
        }
        let p = self.progress(elapsed);
        self.blend(p)
    }

    /// The whole-block transform for an *exit*, where `elapsed` counts from
    /// the moment the exit begins. The same shapes, run backwards.
    pub fn exit_state(&self, elapsed: f64) -> MotionState {
        let p = 1.0 - self.progress(elapsed);
        self.blend(p)
    }

    /// The transform for item `i` of `n` — a character or a word — when
    /// staggering. Non-staggered motions ignore `i`.
    pub fn enter_state_for(&self, elapsed: f64, i: usize, _n: usize) -> MotionState {
        match self.kind {
            MotionKind::Chars { stagger, .. } | MotionKind::Words { stagger, .. } => {
                self.enter_state(elapsed - stagger * i as f64)
            }
            _ => self.enter_state(elapsed),
        }
    }

    pub fn exit_state_for(&self, elapsed: f64, i: usize, n: usize) -> MotionState {
        match self.kind {
            // Exits stagger from the *end*, so the text leaves the way it
            // arrived rather than unravelling backwards.
            MotionKind::Chars { stagger, .. } | MotionKind::Words { stagger, .. } => {
                let j = n.saturating_sub(1).saturating_sub(i);
                self.exit_state(elapsed - stagger * j as f64)
            }
            _ => self.exit_state(elapsed),
        }
    }

    fn at_start(&self) -> MotionState {
        self.blend(0.0)
    }

    /// Interpolate between "not arrived" (p=0) and "in place" (p=1).
    ///
    /// A spring can push `p` past 1, and that overshoot is the point — it is
    /// what makes the motion feel physical — so nothing here clamps it.
    fn blend(&self, p: f64) -> MotionState {
        let alpha = p.clamp(0.0, 1.0);
        match self.kind {
            MotionKind::None => MotionState::SETTLED,
            MotionKind::Fade => MotionState { alpha, ..MotionState::SETTLED },
            MotionKind::Rise { distance } | MotionKind::Chars { rise: distance, .. }
            | MotionKind::Words { rise: distance, .. } => {
                MotionState { dx: 0.0, dy: distance * (1.0 - p), scale: 1.0, alpha }
            }
            MotionKind::Drop { distance } => {
                MotionState { dx: 0.0, dy: -distance * (1.0 - p), scale: 1.0, alpha }
            }
            MotionKind::SlideIn { dx, dy } => {
                MotionState { dx: dx * (1.0 - p), dy: dy * (1.0 - p), scale: 1.0, alpha }
            }
            MotionKind::Scale { from } => {
                MotionState { dx: 0.0, dy: 0.0, scale: from + (1.0 - from) * p, alpha }
            }
        }
    }
}

/// Continuous motion over a layer's whole active span — see the module doc
/// for how this differs from an [`Motion`] entrance/exit.
///
/// Position travels linearly (`dx`, `dy`, in the same frame pixels
/// `MotionKind::SlideIn` already uses — not resolution-independent, matching
/// the rest of this module) from the layer's resting placement; scale grows
/// (or shrinks) toward `scale_to`. Both ride the same `easing` curve, because
/// a burst clip wants its growth and its travel to read as one motion, not
/// two racing each other.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Drift {
    pub dx: f64,
    pub dy: f64,
    #[serde(default = "one")]
    pub scale_to: f64,
    #[serde(default)]
    pub easing: Easing,
}

fn one() -> f64 {
    1.0
}

impl Drift {
    pub fn new(dx: f64, dy: f64) -> Self {
        Drift { dx, dy, scale_to: 1.0, easing: Easing::OutCubic }
    }

    /// A drift pointed along `heading_deg` (0 = +x/right, 90 = +y/down — the
    /// frame's own axes, degrees clockwise) for `distance` pixels.
    pub fn heading(heading_deg: f64, distance: f64) -> Self {
        let r = heading_deg.to_radians();
        Drift::new(r.cos() * distance, r.sin() * distance)
    }

    pub fn grow_to(mut self, scale: f64) -> Self {
        self.scale_to = scale;
        self
    }

    pub fn eased(mut self, e: Easing) -> Self {
        self.easing = e;
        self
    }

    /// `(dx, dy, scale)` at `elapsed` seconds into a `duration`-second span.
    /// `duration <= 0.0` snaps straight to the far end, the same convention
    /// [`Motion::progress`] uses for a zero-length motion.
    pub fn state_at(&self, elapsed: f64, duration: f64) -> (f64, f64, f64) {
        let p = if duration <= 0.0 { 1.0 } else { (elapsed / duration).clamp(0.0, 1.0) };
        let e = self.easing.apply(p);
        (self.dx * e, self.dy * e, 1.0 + (self.scale_to - 1.0) * e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_motion_starts_invisible_and_settles() {
        let m = Motion::rise(0.5);
        let a = m.enter_state(0.0);
        assert!(a.is_invisible(), "{a:?}");
        assert!(a.dy > 30.0, "starts displaced: {a:?}");
        let b = m.enter_state(3.0);
        assert!((b.alpha - 1.0).abs() < 1e-3, "{b:?}");
        assert!(b.dy.abs() < 0.5, "settles in place: {b:?}");
    }

    #[test]
    fn an_exit_ends_invisible() {
        let m = Motion::fade(0.4);
        assert!((m.exit_state(0.0).alpha - 1.0).abs() < 1e-9);
        assert!(m.exit_state(0.4).is_invisible());
    }

    #[test]
    fn staggering_delays_later_characters() {
        let m = Motion::chars(0.4, 0.05);
        // At 0.02s the first character has begun and the tenth has not.
        assert!(m.enter_state_for(0.02, 0, 12).alpha > 0.0);
        assert!(m.enter_state_for(0.02, 10, 12).is_invisible());
        // Total duration accounts for the whole stagger.
        assert!((m.total_duration(12) - (0.4 + 0.05 * 11.0)).abs() < 1e-9);
    }

    #[test]
    fn non_staggered_motions_ignore_the_index() {
        let m = Motion::fade(0.3);
        assert_eq!(m.enter_state_for(0.1, 0, 10), m.enter_state_for(0.1, 9, 10));
        assert_eq!(m.total_duration(10), 0.3);
    }

    #[test]
    fn scale_motion_grows_from_its_starting_size() {
        let m = Motion::new(MotionKind::Scale { from: 0.5 }, 0.5).eased(Easing::Linear);
        assert!((m.enter_state(0.0).scale - 0.5).abs() < 1e-9);
        assert!((m.enter_state(0.5).scale - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_spring_entrance_may_overshoot() {
        let m = Motion::new(MotionKind::Rise { distance: 100.0 }, 0.3)
            .timed(Timing::spring(Spring::bouncy()));
        // Somewhere in the run it should pass its resting place.
        let overshot = (0..60).any(|i| m.enter_state(i as f64 * 0.01).dy < -0.5);
        assert!(overshot, "a bouncy spring should overshoot");
    }

    #[test]
    fn round_trips_through_json() {
        let m = Motion::chars(0.5, 0.04);
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Motion>(&s).unwrap(), m);
    }

    #[test]
    fn a_drift_starts_at_rest_and_ends_at_its_full_vector() {
        let d = Drift::new(100.0, -40.0).grow_to(2.0).eased(Easing::Linear);
        assert_eq!(d.state_at(0.0, 1.0), (0.0, 0.0, 1.0));
        let (dx, dy, scale) = d.state_at(1.0, 1.0);
        assert!((dx - 100.0).abs() < 1e-9 && (dy + 40.0).abs() < 1e-9);
        assert!((scale - 2.0).abs() < 1e-9);
        // Halfway, linear, is halfway on every axis.
        let (dx, dy, scale) = d.state_at(0.5, 1.0);
        assert!((dx - 50.0).abs() < 1e-9 && (dy + 20.0).abs() < 1e-9);
        assert!((scale - 1.5).abs() < 1e-9);
    }

    #[test]
    fn heading_points_along_the_frames_own_axes() {
        // 0 degrees is straight along +x.
        let right = Drift::heading(0.0, 10.0);
        assert!((right.dx - 10.0).abs() < 1e-9 && right.dy.abs() < 1e-9);
        // 90 degrees is straight down (+y), matching SlideIn's convention.
        let down = Drift::heading(90.0, 10.0);
        assert!(down.dx.abs() < 1e-9 && (down.dy - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_zero_length_drift_snaps_to_its_end_state() {
        let d = Drift::heading(45.0, 50.0);
        let (dx, dy, _) = d.state_at(0.0, 0.0);
        let full = d.state_at(1.0, 1.0);
        assert!((dx - full.0).abs() < 1e-9 && (dy - full.1).abs() < 1e-9);
    }

    #[test]
    fn a_drift_round_trips_through_json() {
        let d = Drift::new(120.0, -80.0).grow_to(1.6).eased(Easing::InCubic);
        let s = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Drift>(&s).unwrap(), d);
    }
}
