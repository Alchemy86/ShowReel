//! Easing, interpolation and springs.
//!
//! The shape of [`interpolate`] is taken from Remotion's `interpolate()`, which
//! is the best idea in their API: one call maps a value through a multi-point
//! keyframe curve with per-segment easing. What is *not* taken is the pile of
//! options bolted onto it — `extrapolateLeft`/`extrapolateRight` as separate
//! stringly knobs, `output: 'perceptual-scale'`, `posterize`. ShowReel has one
//! [`Extrapolate`] enum, and it defaults to `Clamp`, which is what an animation
//! wants essentially every time.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// What a curve does outside its input range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Extrapolate {
    /// Hold the boundary value. The default, and almost always right.
    #[default]
    Clamp,
    /// Keep going along the last segment's slope.
    Extend,
    /// Loop back to the start of the range.
    Wrap,
}

/// A named easing curve.
///
/// Remotion exposes these as `Easing.in(Easing.cubic)` style combinators. In
/// Rust an enum is the honest equivalent: it is serialisable (so a film file
/// can name a curve), exhaustively matched, and needs no closures crossing
/// thread boundaries when frames render in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Easing {
    Linear,
    #[default]
    /// The workhorse. Gentle start, gentle stop — what a camera move wants.
    InOutCubic,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InQuart,
    OutQuart,
    InOutQuart,
    InExpo,
    OutExpo,
    InOutExpo,
    InCirc,
    OutCirc,
    InOutCirc,
    InSine,
    OutSine,
    InOutSine,
    /// Overshoots slightly before settling. Good for text arriving.
    InBack,
    OutBack,
    InOutBack,
    OutElastic,
    OutBounce,
    /// Hold the start value until the very end, then jump. Useful for cuts.
    Step,
    /// A cubic Bézier, as CSS `cubic-bezier(x1,y1,x2,y2)`.
    Bezier(f64, f64, f64, f64),
}

impl Easing {
    /// Map linear progress `t` (0..=1) through the curve.
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        use Easing::*;
        match self {
            Linear => t,
            InQuad => t * t,
            OutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            InOutQuad => {
                if t < 0.5 { 2.0 * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(2) / 2.0 }
            }
            InCubic => t * t * t,
            OutCubic => 1.0 - (1.0 - t).powi(3),
            InOutCubic => {
                if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
            }
            InQuart => t.powi(4),
            OutQuart => 1.0 - (1.0 - t).powi(4),
            InOutQuart => {
                if t < 0.5 { 8.0 * t.powi(4) } else { 1.0 - (-2.0 * t + 2.0).powi(4) / 2.0 }
            }
            InExpo => if t == 0.0 { 0.0 } else { (2.0f64).powf(10.0 * t - 10.0) },
            OutExpo => if t == 1.0 { 1.0 } else { 1.0 - (2.0f64).powf(-10.0 * t) },
            InOutExpo => {
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else if t < 0.5 {
                    (2.0f64).powf(20.0 * t - 10.0) / 2.0
                } else {
                    (2.0 - (2.0f64).powf(-20.0 * t + 10.0)) / 2.0
                }
            }
            InCirc => 1.0 - (1.0 - t * t).max(0.0).sqrt(),
            OutCirc => (1.0 - (t - 1.0).powi(2)).max(0.0).sqrt(),
            InOutCirc => {
                if t < 0.5 {
                    (1.0 - (1.0 - (2.0 * t).powi(2)).max(0.0).sqrt()) / 2.0
                } else {
                    ((1.0 - (-2.0 * t + 2.0).powi(2)).max(0.0).sqrt() + 1.0) / 2.0
                }
            }
            InSine => 1.0 - ((t * PI) / 2.0).cos(),
            OutSine => ((t * PI) / 2.0).sin(),
            InOutSine => -((PI * t).cos() - 1.0) / 2.0,
            InBack => {
                const C1: f64 = 1.70158;
                const C3: f64 = C1 + 1.0;
                C3 * t * t * t - C1 * t * t
            }
            OutBack => {
                const C1: f64 = 1.70158;
                const C3: f64 = C1 + 1.0;
                1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
            }
            InOutBack => {
                const C2: f64 = 1.70158 * 1.525;
                if t < 0.5 {
                    ((2.0 * t).powi(2) * ((C2 + 1.0) * 2.0 * t - C2)) / 2.0
                } else {
                    ((2.0 * t - 2.0).powi(2) * ((C2 + 1.0) * (t * 2.0 - 2.0) + C2) + 2.0) / 2.0
                }
            }
            OutElastic => {
                const C4: f64 = 2.0 * PI / 3.0;
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    (2.0f64).powf(-10.0 * t) * ((t * 10.0 - 0.75) * C4).sin() + 1.0
                }
            }
            OutBounce => out_bounce(t),
            Step => if t >= 1.0 { 1.0 } else { 0.0 },
            Bezier(x1, y1, x2, y2) => cubic_bezier(t, x1, y1, x2, y2),
        }
    }
}

fn out_bounce(t: f64) -> f64 {
    const N1: f64 = 7.5625;
    const D1: f64 = 2.75;
    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / D1;
        N1 * t * t + 0.984375
    }
}

/// Solve a CSS-style cubic Bézier for y at a given x, by Newton then bisection.
fn cubic_bezier(x: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    fn bez(t: f64, a: f64, b: f64) -> f64 {
        let mt = 1.0 - t;
        3.0 * mt * mt * t * a + 3.0 * mt * t * t * b + t * t * t
    }
    fn bez_prime(t: f64, a: f64, b: f64) -> f64 {
        let mt = 1.0 - t;
        3.0 * mt * mt * a + 6.0 * mt * t * (b - a) + 3.0 * t * t * (1.0 - b)
    }
    let mut t = x;
    for _ in 0..8 {
        let err = bez(t, x1, x2) - x;
        if err.abs() < 1e-7 {
            return bez(t, y1, y2);
        }
        let d = bez_prime(t, x1, x2);
        if d.abs() < 1e-9 {
            break;
        }
        t -= err / d;
    }
    // Newton did not converge (a near-vertical control polygon); bisect.
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let mut t = x;
    for _ in 0..24 {
        let v = bez(t, x1, x2);
        if (v - x).abs() < 1e-7 {
            break;
        }
        if v < x { lo = t } else { hi = t }
        t = (lo + hi) / 2.0;
    }
    bez(t, y1, y2)
}

/// Map `value` through a keyframe curve.
///
/// `input` and `output` must be the same length (>= 2) and `input` must be
/// ascending. `easing` may be one curve for the whole ramp, or one per segment.
///
/// ```
/// # use showreel::ease::{interpolate, Easing, Extrapolate};
/// // fade in over the first 20 frames, hold, fade out over the last 20
/// let o = interpolate(10.0, &[0.0, 20.0, 100.0, 120.0], &[0.0, 1.0, 1.0, 0.0],
///                     &[Easing::Linear], Extrapolate::Clamp);
/// assert!((o - 0.5).abs() < 1e-9);
/// ```
pub fn interpolate(
    value: f64,
    input: &[f64],
    output: &[f64],
    easing: &[Easing],
    extrapolate: Extrapolate,
) -> f64 {
    debug_assert_eq!(input.len(), output.len(), "interpolate: range length mismatch");
    if input.len() < 2 {
        return output.first().copied().unwrap_or(0.0);
    }
    let (lo, hi) = (input[0], input[input.len() - 1]);

    let value = match extrapolate {
        Extrapolate::Clamp => value.clamp(lo, hi),
        Extrapolate::Wrap => {
            let span = hi - lo;
            if span <= 0.0 { lo } else { lo + (value - lo).rem_euclid(span) }
        }
        Extrapolate::Extend => value,
    };

    // Find the segment. Extend uses the end segments' slopes beyond the range.
    let mut seg = 0usize;
    while seg + 2 < input.len() && value >= input[seg + 1] {
        seg += 1;
    }
    let (x0, x1) = (input[seg], input[seg + 1]);
    let (y0, y1) = (output[seg], output[seg + 1]);
    if (x1 - x0).abs() < f64::EPSILON {
        return y1;
    }
    let t = (value - x0) / (x1 - x0);

    let curve = if easing.is_empty() {
        Easing::Linear
    } else if easing.len() == 1 {
        easing[0]
    } else {
        easing[seg.min(easing.len() - 1)]
    };

    // Easing is only meaningful inside the segment; outside, Extend must stay
    // linear or the curve doubles back on itself.
    let e = if (0.0..=1.0).contains(&t) { curve.apply(t) } else { t };
    y0 + (y1 - y0) * e
}

/// Interpolate geometrically rather than additively.
///
/// A zoom from 2% to 100% is not perceived evenly when the *width* is ramped
/// linearly — it crawls at the start and lurches at the end. Interpolating the
/// logarithm makes the zoom rate constant, which is why a pull-back done this
/// way looks like a camera and a linear one looks like a bug. Used by
/// [`crate::camera`].
pub fn interpolate_geometric(t: f64, from: f64, to: f64, easing: Easing) -> f64 {
    if from <= 0.0 || to <= 0.0 {
        return from + (to - from) * easing.apply(t);
    }
    from * (to / from).powf(easing.apply(t.clamp(0.0, 1.0)))
}

/// A physical spring, as Remotion's `spring()`.
///
/// Kept because a spring is genuinely the nicest way to make text arrive: it
/// has one intuitive dial (bounciness) and it never looks mechanical. The
/// simulation is deterministic — a fixed timestep from rest — so frame N is the
/// same number on every machine.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Spring {
    pub mass: f64,
    pub stiffness: f64,
    pub damping: f64,
    /// Stop the curve exceeding 1.0.
    pub clamp: bool,
}

impl Default for Spring {
    fn default() -> Self {
        Spring { mass: 1.0, stiffness: 100.0, damping: 10.0, clamp: false }
    }
}

impl Spring {
    /// A firm, barely-overshooting spring — the default for text entrances.
    pub fn gentle() -> Self {
        Spring { mass: 1.0, stiffness: 120.0, damping: 20.0, clamp: false }
    }

    /// Visibly bouncy.
    pub fn bouncy() -> Self {
        Spring { mass: 1.0, stiffness: 180.0, damping: 12.0, clamp: false }
    }

    /// No overshoot at all.
    pub fn stiff() -> Self {
        Spring { mass: 1.0, stiffness: 200.0, damping: 30.0, clamp: true }
    }

    /// Value at `t` seconds after release, from 0 towards 1.
    pub fn at(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        // Fixed 1ms steps: independent of the film's frame rate, so the same
        // spring looks the same at 24fps and 60fps.
        const DT: f64 = 0.001;
        let steps = (t / DT).round() as u64;
        let (mut x, mut v) = (0.0f64, 0.0f64);
        for _ in 0..steps.min(60_000) {
            let f = self.stiffness * (1.0 - x) - self.damping * v;
            v += (f / self.mass) * DT;
            x += v * DT;
        }
        if self.clamp { x.min(1.0) } else { x }
    }

    /// How long until the spring has settled within `threshold` of 1.
    pub fn settle_time(&self, threshold: f64) -> f64 {
        const DT: f64 = 0.001;
        let (mut x, mut v) = (0.0f64, 0.0f64);
        for i in 0..20_000u64 {
            if (1.0 - x).abs() < threshold && v.abs() < threshold {
                return i as f64 * DT;
            }
            let f = self.stiffness * (1.0 - x) - self.damping * v;
            v += (f / self.mass) * DT;
            x += v * DT;
        }
        20.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easings_are_normalised() {
        for e in [
            Easing::Linear, Easing::InOutCubic, Easing::OutBack, Easing::OutBounce,
            Easing::OutElastic, Easing::InOutExpo, Easing::Bezier(0.4, 0.0, 0.2, 1.0),
        ] {
            assert!((e.apply(0.0)).abs() < 1e-6, "{e:?} must start at 0");
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6, "{e:?} must end at 1");
        }
    }

    #[test]
    fn multi_keyframe_fade_in_hold_out() {
        let inp = [0.0, 20.0, 100.0, 120.0];
        let out = [0.0, 1.0, 1.0, 0.0];
        let f = |v| interpolate(v, &inp, &out, &[Easing::Linear], Extrapolate::Clamp);
        assert!((f(0.0) - 0.0).abs() < 1e-9);
        assert!((f(10.0) - 0.5).abs() < 1e-9);
        assert!((f(60.0) - 1.0).abs() < 1e-9);
        assert!((f(110.0) - 0.5).abs() < 1e-9);
        assert!((f(120.0) - 0.0).abs() < 1e-9);
        // Clamped outside the range.
        assert!((f(-50.0) - 0.0).abs() < 1e-9);
        assert!((f(500.0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn geometric_zoom_is_even() {
        // Halfway through a 1 -> 100 zoom, geometric interpolation is at 10,
        // not 50.5. That is the whole point.
        let m = interpolate_geometric(0.5, 1.0, 100.0, Easing::Linear);
        assert!((m - 10.0).abs() < 1e-6, "got {m}");
    }

    #[test]
    fn spring_settles_at_one() {
        let s = Spring::gentle();
        assert!(s.at(0.0).abs() < 1e-9);
        assert!((s.at(3.0) - 1.0).abs() < 1e-3, "got {}", s.at(3.0));
        assert!(Spring::stiff().at(2.0) <= 1.0);
    }

    #[test]
    fn spring_is_deterministic() {
        let s = Spring::bouncy();
        assert_eq!(s.at(0.4).to_bits(), s.at(0.4).to_bits());
    }
}
