//! Transitions between scenes.
//!
//! The factoring is Remotion's, and it is the right one: **how a transition
//! looks** ([`Presentation`]) is independent of **how it is paced**
//! ([`Timing`]), so any curve composes with any effect. Their overlap
//! semantics are taken too — a transition consumes time from both neighbours,
//! so `A(4s) + transition(1s) + B(4s)` is a 7 second film, not 9.
//!
//! What is *not* taken is how Remotion enforces the rules. There, "a transition
//! cannot be first or last" and "two transitions cannot be adjacent" are
//! runtime errors you discover after a render. Here the timeline is
//! `Scene, (Transition, Scene)*` — see [`crate::timeline::Timeline`] — so those
//! mistakes cannot be written down in the first place. That is the honest Rust
//! improvement on the idea: push the invariant into the type and delete the
//! error message.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::ease::{Easing, Spring};
use crate::geom::{Direction, Rect};
use crate::time::Time;
use serde::{Deserialize, Serialize};
use tiny_skia::{FillRule, Mask, PathBuilder, Transform};

/// How the transition is paced.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Timing {
    Eased { easing: Easing },
    Spring { spring: Spring },
}

impl Default for Timing {
    fn default() -> Self {
        Timing::Eased { easing: Easing::InOutCubic }
    }
}

impl Timing {
    pub fn eased(e: Easing) -> Self {
        Timing::Eased { easing: e }
    }

    pub fn spring(s: Spring) -> Self {
        Timing::Spring { spring: s }
    }

    /// Progress 0..1 from linear progress `t`, and the seconds elapsed.
    pub fn progress(&self, t: f64, elapsed: f64) -> f64 {
        match self {
            Timing::Eased { easing } => easing.apply(t.clamp(0.0, 1.0)),
            // A spring is defined in real seconds, not normalised progress, so
            // it keeps its physical character whatever the duration.
            Timing::Spring { spring } => spring.at(elapsed).clamp(0.0, 1.2),
        }
    }
}

/// Anything that can composite an outgoing and an incoming frame.
///
/// The enum below covers the built-ins; the trait exists so a film can bring
/// its own without the crate needing to know about it.
pub trait Present: Send + Sync {
    /// `out` is the outgoing scene, `incoming` the arriving one, `p` the
    /// progress 0..1. Write the composite into `dst`.
    fn compose(&self, out: &Canvas, incoming: &Canvas, p: f64, dst: &mut Canvas);
}

/// The built-in looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Presentation {
    /// No blend at all — the incoming scene replaces the outgoing one at the
    /// midpoint. Present so that a hard cut is still a timeline element and
    /// can be re-paced into something softer without restructuring the film.
    Cut,
    /// Cross-fade. The workhorse, and the default.
    #[default]
    Dissolve,
    /// Fade out through a colour, then in. Reads as a bigger break than a
    /// dissolve, which is what you want between chapters.
    Fade {
        #[serde(default = "black")]
        through: Color,
    },
    /// A moving edge reveals the incoming scene. `softness` in pixels; 0 is a
    /// hard edge.
    Wipe {
        #[serde(default)]
        direction: Direction,
        #[serde(default)]
        softness: f64,
    },
    /// The incoming scene slides in on top; the outgoing one stays put.
    Slide {
        #[serde(default)]
        direction: Direction,
    },
    /// Both scenes move together, as if on one strip of film.
    Push {
        #[serde(default)]
        direction: Direction,
    },
    /// A circle opens (or closes) over the frame.
    Iris {
        /// Centre as a fraction of the frame.
        #[serde(default = "half")]
        cx: f64,
        #[serde(default = "half")]
        cy: f64,
        #[serde(default)]
        softness: f64,
    },
    /// The incoming scene expands from small to full while fading in — the
    /// "reveal" beat in an explainer.
    ZoomIn {
        #[serde(default = "zoom_from")]
        from: f64,
    },
}

fn black() -> Color {
    Color::BLACK
}

fn half() -> f64 {
    0.5
}

fn zoom_from() -> f64 {
    0.86
}

impl Present for Presentation {
    fn compose(&self, out: &Canvas, incoming: &Canvas, p: f64, dst: &mut Canvas) {
        let p = p.clamp(0.0, 1.0);
        let frame = dst.rect();
        match self {
            Presentation::Cut => {
                let src = if p < 0.5 { out } else { incoming };
                dst.draw_canvas(src, frame, 1.0);
            }
            Presentation::Dissolve => {
                dst.draw_canvas(out, frame, 1.0);
                dst.draw_canvas(incoming, frame, p);
            }
            Presentation::Fade { through } => {
                // First half fades the outgoing scene down to the colour, the
                // second half brings the incoming one up out of it.
                if p < 0.5 {
                    dst.clear(*through);
                    dst.draw_canvas(out, frame, 1.0 - p * 2.0);
                } else {
                    dst.clear(*through);
                    dst.draw_canvas(incoming, frame, (p - 0.5) * 2.0);
                }
            }
            Presentation::Wipe { direction, softness } => {
                dst.draw_canvas(out, frame, 1.0);
                if let Some(mask) = wipe_mask(frame, *direction, p, *softness) {
                    dst.draw_pixmap_masked(incoming.as_ref(), frame, 1.0, &mask);
                } else {
                    dst.draw_canvas(incoming, frame, p);
                }
            }
            Presentation::Slide { direction } => {
                dst.draw_canvas(out, frame, 1.0);
                let (dx, dy) = direction.vector();
                // Travel from fully off-frame to home.
                let k = 1.0 - p;
                let r = Rect::new(
                    frame.x - dx * frame.w * k,
                    frame.y - dy * frame.h * k,
                    frame.w,
                    frame.h,
                );
                dst.draw_canvas(incoming, r, 1.0);
            }
            Presentation::Push { direction } => {
                let (dx, dy) = direction.vector();
                let k = 1.0 - p;
                dst.draw_canvas(
                    out,
                    Rect::new(frame.x + dx * frame.w * p, frame.y + dy * frame.h * p, frame.w, frame.h),
                    1.0,
                );
                dst.draw_canvas(
                    incoming,
                    Rect::new(frame.x - dx * frame.w * k, frame.y - dy * frame.h * k, frame.w, frame.h),
                    1.0,
                );
            }
            Presentation::Iris { cx, cy, softness } => {
                dst.draw_canvas(out, frame, 1.0);
                let centre = (frame.w * cx, frame.h * cy);
                // Reach the far corner at p = 1 so the circle always finishes.
                let max_r = ((frame.w - centre.0).max(centre.0)).hypot((frame.h - centre.1).max(centre.1));
                if let Some(mask) = iris_mask(frame, centre, max_r * p, *softness) {
                    dst.draw_pixmap_masked(incoming.as_ref(), frame, 1.0, &mask);
                } else {
                    dst.draw_canvas(incoming, frame, p);
                }
            }
            Presentation::ZoomIn { from } => {
                dst.draw_canvas(out, frame, 1.0);
                let s = from + (1.0 - from) * p;
                let (cx, cy) = frame.centre();
                dst.draw_canvas(incoming, Rect::centred(cx, cy, frame.w * s, frame.h * s), p);
            }
        }
    }
}

/// A hard or soft straight edge sweeping across the frame.
fn wipe_mask(frame: Rect, dir: Direction, p: f64, softness: f64) -> Option<Mask> {
    let (w, h) = (frame.w as u32, frame.h as u32);
    let mut pm = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let soft = softness.max(0.0);
    // The revealed band grows from the edge the wipe comes *from*.
    let (start, end) = match dir {
        // Travelling right: reveal from the left edge.
        Direction::Right => ((0.0, 0.0), (frame.w, 0.0)),
        Direction::Left => ((frame.w, 0.0), (0.0, 0.0)),
        Direction::Down => ((0.0, 0.0), (0.0, frame.h)),
        Direction::Up => ((0.0, frame.h), (0.0, 0.0)),
    };
    // Overshoot by the softness at both ends so the edge fully clears the
    // frame rather than leaving a permanent grey fringe.
    let extent = ((end.0 - start.0).hypot(end.1 - start.1)).max(1.0);
    let travel = p * (extent + soft * 2.0) - soft;
    let ux = (end.0 - start.0) / extent;
    let uy = (end.1 - start.1) / extent;
    let edge = (start.0 + ux * travel, start.1 + uy * travel);

    let shader = if soft <= 0.5 {
        tiny_skia::Shader::SolidColor(tiny_skia::Color::WHITE)
    } else {
        tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy((edge.0 - ux * soft / 2.0) as f32, (edge.1 - uy * soft / 2.0) as f32),
            tiny_skia::Point::from_xy((edge.0 + ux * soft / 2.0) as f32, (edge.1 + uy * soft / 2.0) as f32),
            vec![
                tiny_skia::GradientStop::new(0.0, tiny_skia::Color::WHITE),
                tiny_skia::GradientStop::new(1.0, tiny_skia::Color::TRANSPARENT),
            ],
            tiny_skia::SpreadMode::Pad,
            Transform::identity(),
        )?
    };

    if soft <= 0.5 {
        // A hard edge is just a rectangle of the revealed region.
        let r = revealed_rect(frame, dir, travel);
        if r.w > 0.0 && r.h > 0.0
            && let Some(sk) = tiny_skia::Rect::from_xywh(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
        {
            let paint = tiny_skia::Paint { shader, ..Default::default() };
            pm.fill_rect(sk, &paint, Transform::identity(), None);
        }
    } else {
        let paint = tiny_skia::Paint { anti_alias: true, shader, ..Default::default() };
        let sk = tiny_skia::Rect::from_xywh(0.0, 0.0, frame.w as f32, frame.h as f32)?;
        pm.fill_rect(sk, &paint, Transform::identity(), None);
    }
    pixmap_to_mask(&pm)
}

fn revealed_rect(frame: Rect, dir: Direction, travel: f64) -> Rect {
    match dir {
        Direction::Right => Rect::new(0.0, 0.0, travel, frame.h),
        Direction::Left => Rect::new(frame.w - travel, 0.0, travel, frame.h),
        Direction::Down => Rect::new(0.0, 0.0, frame.w, travel),
        Direction::Up => Rect::new(0.0, frame.h - travel, frame.w, travel),
    }
}

fn iris_mask(frame: Rect, centre: (f64, f64), radius: f64, softness: f64) -> Option<Mask> {
    let (w, h) = (frame.w as u32, frame.h as u32);
    let mut pm = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    if radius > 0.0 {
        let mut pb = PathBuilder::new();
        pb.push_circle(centre.0 as f32, centre.1 as f32, radius as f32);
        if let Some(path) = pb.finish() {
            let mut paint = tiny_skia::Paint { anti_alias: true, ..Default::default() };
            paint.shader = tiny_skia::Shader::SolidColor(tiny_skia::Color::WHITE);
            pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
        if softness > 0.5 {
            crate::canvas::blur_alpha(&mut pm, softness / 2.0);
        }
    }
    pixmap_to_mask(&pm)
}

fn pixmap_to_mask(pm: &tiny_skia::Pixmap) -> Option<Mask> {
    let mut m = Mask::new(pm.width(), pm.height())?;
    let d = m.data_mut();
    for (i, px) in pm.pixels().iter().enumerate() {
        d[i] = px.alpha();
    }
    Some(m)
}

/// A transition: what it looks like, how it is paced, how long it lasts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub duration: Time,
    #[serde(default)]
    pub presentation: Presentation,
    #[serde(default)]
    pub timing: Timing,
}

impl Transition {
    pub fn new(duration: impl Into<Time>, presentation: Presentation) -> Self {
        Transition { duration: duration.into(), presentation, timing: Timing::default() }
    }

    pub fn cut() -> Self {
        Transition::new(0.0, Presentation::Cut)
    }

    pub fn dissolve(d: impl Into<Time>) -> Self {
        Transition::new(d, Presentation::Dissolve)
    }

    pub fn fade(d: impl Into<Time>, through: Color) -> Self {
        Transition::new(d, Presentation::Fade { through })
    }

    pub fn fade_black(d: impl Into<Time>) -> Self {
        Transition::fade(d, Color::BLACK)
    }

    pub fn wipe(d: impl Into<Time>, direction: Direction) -> Self {
        Transition::new(d, Presentation::Wipe { direction, softness: 64.0 })
    }

    pub fn slide(d: impl Into<Time>, direction: Direction) -> Self {
        Transition::new(d, Presentation::Slide { direction })
    }

    pub fn push(d: impl Into<Time>, direction: Direction) -> Self {
        Transition::new(d, Presentation::Push { direction })
    }

    pub fn iris(d: impl Into<Time>) -> Self {
        Transition::new(d, Presentation::Iris { cx: 0.5, cy: 0.5, softness: 24.0 })
    }

    pub fn zoom_in(d: impl Into<Time>) -> Self {
        Transition::new(d, Presentation::ZoomIn { from: 0.86 })
    }

    pub fn timed(mut self, t: Timing) -> Self {
        self.timing = t;
        self
    }

    pub fn eased(mut self, e: Easing) -> Self {
        self.timing = Timing::eased(e);
        self
    }

    /// Composite two scene frames, `elapsed` seconds into the transition.
    pub fn compose(&self, out: &Canvas, incoming: &Canvas, elapsed: f64, dst: &mut Canvas) {
        let d = self.duration.as_secs();
        let linear = if d <= 0.0 { 1.0 } else { (elapsed / d).clamp(0.0, 1.0) };
        let p = self.timing.progress(linear, elapsed);
        self.presentation.compose(out, incoming, p, dst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(c: Color) -> Canvas {
        Canvas::filled(64, 36, c).unwrap()
    }

    fn sample(cv: &Canvas, fx: f64, fy: f64) -> (u8, u8, u8) {
        let x = (fx * (cv.width() - 1) as f64) as u32;
        let y = (fy * (cv.height() - 1) as f64) as u32;
        let p = cv.as_ref().pixels()[(y * cv.width() + x) as usize];
        (p.red(), p.green(), p.blue())
    }

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    #[test]
    fn every_presentation_ends_on_the_incoming_scene() {
        let (a, b) = (scene(RED), scene(BLUE));
        for pres in [
            Presentation::Cut,
            Presentation::Dissolve,
            Presentation::Fade { through: Color::BLACK },
            Presentation::Wipe { direction: Direction::Right, softness: 0.0 },
            Presentation::Wipe { direction: Direction::Left, softness: 8.0 },
            Presentation::Slide { direction: Direction::Right },
            Presentation::Push { direction: Direction::Up },
            Presentation::Iris { cx: 0.5, cy: 0.5, softness: 0.0 },
            Presentation::ZoomIn { from: 0.8 },
        ] {
            let mut dst = scene(Color::rgb(0, 255, 0));
            pres.compose(&a, &b, 1.0, &mut dst);
            let (r, _g, bl) = sample(&dst, 0.5, 0.5);
            assert!(bl > 200 && r < 60, "{pres:?} at p=1 gave {:?}", sample(&dst, 0.5, 0.5));
        }
    }

    #[test]
    fn every_presentation_starts_on_the_outgoing_scene() {
        let (a, b) = (scene(RED), scene(BLUE));
        for pres in [
            Presentation::Cut,
            Presentation::Dissolve,
            Presentation::Wipe { direction: Direction::Right, softness: 0.0 },
            Presentation::Slide { direction: Direction::Right },
            Presentation::Push { direction: Direction::Up },
            Presentation::Iris { cx: 0.5, cy: 0.5, softness: 0.0 },
            Presentation::ZoomIn { from: 0.8 },
        ] {
            let mut dst = scene(Color::rgb(0, 255, 0));
            pres.compose(&a, &b, 0.0, &mut dst);
            let (r, _g, bl) = sample(&dst, 0.5, 0.5);
            assert!(r > 200 && bl < 60, "{pres:?} at p=0 gave {:?}", sample(&dst, 0.5, 0.5));
        }
    }

    #[test]
    fn dissolve_is_halfway_at_halfway() {
        let mut dst = scene(Color::BLACK);
        Presentation::Dissolve.compose(&scene(RED), &scene(BLUE), 0.5, &mut dst);
        let (r, _, b) = sample(&dst, 0.5, 0.5);
        assert!((100..=160).contains(&r), "red {r}");
        assert!((100..=160).contains(&b), "blue {b}");
    }

    #[test]
    fn fade_through_black_is_dark_at_the_midpoint() {
        let mut dst = scene(Color::rgb(0, 255, 0));
        Presentation::Fade { through: Color::BLACK }.compose(&scene(RED), &scene(BLUE), 0.5, &mut dst);
        let (r, g, b) = sample(&dst, 0.5, 0.5);
        assert!(r < 40 && g < 40 && b < 40, "midpoint should be near black, got {r},{g},{b}");
    }

    #[test]
    fn a_rightward_wipe_reveals_from_the_left() {
        let mut dst = scene(Color::BLACK);
        Presentation::Wipe { direction: Direction::Right, softness: 0.0 }
            .compose(&scene(RED), &scene(BLUE), 0.5, &mut dst);
        assert!(sample(&dst, 0.1, 0.5).2 > 200, "left edge should be the incoming scene");
        assert!(sample(&dst, 0.9, 0.5).0 > 200, "right edge should still be outgoing");
    }

    #[test]
    fn iris_opens_from_the_centre() {
        let mut dst = scene(Color::BLACK);
        Presentation::Iris { cx: 0.5, cy: 0.5, softness: 0.0 }
            .compose(&scene(RED), &scene(BLUE), 0.35, &mut dst);
        assert!(sample(&dst, 0.5, 0.5).2 > 200, "centre revealed");
        assert!(sample(&dst, 0.02, 0.02).0 > 200, "corner not yet");
    }

    #[test]
    fn spring_timing_reaches_the_end() {
        let t = Transition::dissolve(0.6).timed(Timing::spring(Spring::stiff()));
        let mut dst = scene(Color::BLACK);
        t.compose(&scene(RED), &scene(BLUE), 5.0, &mut dst);
        assert!(sample(&dst, 0.5, 0.5).2 > 200);
    }

    #[test]
    fn zero_duration_transition_is_immediately_complete() {
        let t = Transition::cut();
        let mut dst = scene(Color::BLACK);
        t.compose(&scene(RED), &scene(BLUE), 0.0, &mut dst);
        assert!(sample(&dst, 0.5, 0.5).2 > 200);
    }

    #[test]
    fn presentations_round_trip_through_json() {
        let t = Transition::wipe(0.7, Direction::Up).eased(Easing::OutExpo);
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Transition>(&s).unwrap(), t);
        assert!(s.contains("\"wipe\""), "{s}");
    }
}
