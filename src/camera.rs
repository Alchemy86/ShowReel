//! The camera: zoom, pan and hold over a source larger than the frame.
//!
//! This is the capability the scout report named as genuinely missing from
//! everything we already own, and the one performance question worth answering
//! first. It is answered in [`crate::assets::still`], with numbers.
//!
//! A camera is a list of [`Shot`]s — where to be, and when. Between two shots
//! the viewport is interpolated, with the *zoom interpolated geometrically*:
//! ramping the viewport width linearly from 300px to 7000px crawls for the
//! first half and then lurches, because what the eye reads is the *rate of
//! change of scale*, not of width. Geometric interpolation makes that rate
//! constant, and is the difference between a camera move and a bug.

use crate::assets::Still;
use crate::ease::{Easing, interpolate_geometric};
use crate::geom::Rect;
use crate::time::Time;
use serde::{Deserialize, Serialize};

/// Where the camera is looking, in source-image pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "kebab-case")]
pub enum Framing {
    /// The whole source, letterboxed to the frame's aspect ratio.
    Whole,
    /// An explicit rect of the source.
    Rect { x: f64, y: f64, w: f64, h: f64 },
    /// Centred on a point, showing `height` source pixels vertically.
    Point { x: f64, y: f64, height: f64 },
    /// Centred on a point given as a *fraction* of the source, so a framing
    /// survives the source being re-rendered at another scale.
    At { fx: f64, fy: f64, height: f64 },
}

impl Framing {
    /// Centre on a point, showing `height` source pixels tall.
    pub fn point(x: f64, y: f64, height: f64) -> Self {
        Framing::Point { x, y, height }
    }

    /// Centre on a fractional position within the source.
    pub fn at(fx: f64, fy: f64, height: f64) -> Self {
        Framing::At { fx, fy, height }
    }

    pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Self {
        Framing::Rect { x, y, w, h }
    }

    /// Resolve to a viewport rect of the right aspect ratio for the frame.
    ///
    /// The result is *not* clamped to the source here — [`Camera::viewport_at`]
    /// does that after interpolation, because clamping the endpoints first
    /// would bend the path between them.
    pub fn resolve(&self, source: (u32, u32), frame_aspect: f64) -> Rect {
        let (sw, sh) = (source.0 as f64, source.1 as f64);
        let r = match *self {
            Framing::Whole => Rect::from_size(sw, sh),
            Framing::Rect { x, y, w, h } => Rect::new(x, y, w, h),
            Framing::Point { x, y, height } => {
                Rect::centred(x, y, height * frame_aspect, height)
            }
            Framing::At { fx, fy, height } => {
                Rect::centred(sw * fx, sh * fy, height * frame_aspect, height)
            }
        };
        r.to_aspect(frame_aspect)
    }
}

/// One held position, and how the camera got there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shot {
    /// When the camera arrives here, measured from the start of the layer.
    pub at: Time,
    pub framing: Framing,
    /// The curve used on the way *to* this shot. Ignored on the first shot.
    #[serde(default = "default_ease")]
    pub ease: Easing,
}

fn default_ease() -> Easing {
    Easing::InOutCubic
}

impl Shot {
    pub fn new(at: impl Into<Time>, framing: Framing) -> Self {
        Shot { at: at.into(), framing, ease: Easing::InOutCubic }
    }

    pub fn eased(mut self, e: Easing) -> Self {
        self.ease = e;
        self
    }
}

/// A camera move over one source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Camera {
    pub shots: Vec<Shot>,
    /// Keep the viewport inside the source. On by default: a camera that
    /// wanders off the edge of the map shows background, which is never what
    /// was meant.
    #[serde(default = "default_true")]
    pub clamp_to_source: bool,
}

fn default_true() -> bool {
    true
}

impl Camera {
    pub fn new() -> Self {
        Camera { shots: Vec::new(), clamp_to_source: true }
    }

    /// Hold one framing for the whole layer.
    pub fn hold(framing: Framing) -> Self {
        Camera { shots: vec![Shot::new(0.0, framing)], clamp_to_source: true }
    }

    /// The move the captain asked for: start tight on a point, end on the whole
    /// source.
    pub fn pull_back(from: Framing, over: impl Into<Time>) -> Self {
        Camera {
            shots: vec![Shot::new(0.0, from), Shot::new(over, Framing::Whole)],
            clamp_to_source: true,
        }
    }

    /// The reverse: open wide, close in on a detail.
    pub fn push_in(to: Framing, over: impl Into<Time>) -> Self {
        Camera {
            shots: vec![Shot::new(0.0, Framing::Whole), Shot::new(over, to)],
            clamp_to_source: true,
        }
    }

    pub fn shot(mut self, s: Shot) -> Self {
        self.shots.push(s);
        self.shots.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        self
    }

    /// Arrive at `framing` at time `at`.
    pub fn to(self, at: impl Into<Time>, framing: Framing) -> Self {
        self.shot(Shot::new(at, framing))
    }

    /// Stay where the previous shot left off until `at` — an explicit hold, so
    /// that a pause reads in the description rather than being implied by a
    /// gap.
    pub fn hold_until(mut self, at: impl Into<Time>) -> Self {
        if let Some(last) = self.shots.last().cloned() {
            self.shots.push(Shot { at: at.into(), ..last });
        }
        self
    }

    pub fn no_clamp(mut self) -> Self {
        self.clamp_to_source = false;
        self
    }

    pub fn duration(&self) -> Time {
        self.shots.last().map(|s| s.at).unwrap_or(Time::ZERO)
    }

    /// The viewport at `t` seconds into the layer.
    pub fn viewport_at(&self, t: Time, source: (u32, u32), frame_aspect: f64) -> Rect {
        let bounds = Rect::from_size(source.0 as f64, source.1 as f64);
        if self.shots.is_empty() {
            return bounds.to_aspect(frame_aspect);
        }
        let resolved = |s: &Shot| s.framing.resolve(source, frame_aspect);

        let first = &self.shots[0];
        if t <= first.at || self.shots.len() == 1 {
            return self.finish(resolved(first), &bounds);
        }
        let last = self.shots.last().unwrap();
        if t >= last.at {
            return self.finish(resolved(last), &bounds);
        }

        let i = self
            .shots
            .windows(2)
            .position(|w| t >= w[0].at && t < w[1].at)
            .unwrap_or(self.shots.len() - 2);
        let (a, b) = (&self.shots[i], &self.shots[i + 1]);
        let span = (b.at - a.at).as_secs();
        let p = if span <= 0.0 { 1.0 } else { ((t - a.at).as_secs() / span).clamp(0.0, 1.0) };

        let (ra, rb) = (resolved(a), resolved(b));
        // Zoom geometrically, position linearly *in the eased frame*: the two
        // have to share one eased parameter or the pan and the zoom arrive at
        // different times and the move wobbles.
        let e = b.ease.apply(p);
        let h = interpolate_geometric(p, ra.h, rb.h, b.ease);
        let w = h * frame_aspect;
        let (cax, cay) = ra.centre();
        let (cbx, cby) = rb.centre();
        let cx = cax + (cbx - cax) * e;
        let cy = cay + (cby - cay) * e;
        self.finish(Rect::centred(cx, cy, w, h), &bounds)
    }

    fn finish(&self, r: Rect, bounds: &Rect) -> Rect {
        if self.clamp_to_source { r.clamp_within(bounds) } else { r }
    }

    /// Draw the source through this camera into `dst_rect` of `canvas`.
    ///
    /// Picks a pyramid level for the zoom, then lets tiny-skia do one
    /// transformed blit. There is no intermediate buffer: the crop, the scale
    /// and the composite are the same operation.
    pub fn draw(
        &self,
        canvas: &mut crate::canvas::Canvas,
        still: &Still,
        t: Time,
        dst_rect: Rect,
        opacity: f64,
    ) {
        if dst_rect.w <= 0.0 || dst_rect.h <= 0.0 || opacity <= 0.0 {
            return;
        }
        let vp = self.viewport_at(t, still.size(), dst_rect.aspect());
        draw_viewport(canvas, still, vp, dst_rect, opacity);
    }
}

/// Draw viewport `vp` of `still` into `dst` on `canvas`.
///
/// Shared with layers that frame a still without a camera move.
pub fn draw_viewport(
    canvas: &mut crate::canvas::Canvas,
    still: &Still,
    vp: Rect,
    dst: Rect,
    opacity: f64,
) {
    let (level, div) = still.level_for(&vp, dst.w);
    // The viewport in the chosen level's own coordinates.
    let vp_level = Rect::new(vp.x / div, vp.y / div, vp.w / div, vp.h / div);

    // Clip to the destination rect unless it is the whole canvas, so an inset
    // camera cannot paint over its neighbours.
    let full = dst.x <= 0.0 && dst.y <= 0.0
        && dst.right() >= canvas.width() as f64
        && dst.bottom() >= canvas.height() as f64;
    let mask = if full {
        None
    } else {
        crate::canvas::round_rect_path(dst, 0.0).and_then(|p| {
            let mut m = tiny_skia::Mask::new(canvas.width(), canvas.height())?;
            m.fill_path(&p, tiny_skia::FillRule::Winding, true, tiny_skia::Transform::identity());
            Some(m)
        })
    };
    canvas.draw_pixmap_cropped(level, vp_level, dst, opacity, mask.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: (u32, u32) = (6832, 7024);
    const ASPECT: f64 = 16.0 / 9.0;

    #[test]
    fn framing_always_matches_the_frame_aspect() {
        for f in [
            Framing::Whole,
            Framing::point(100.0, 100.0, 288.0),
            Framing::rect(0.0, 0.0, 500.0, 100.0),
            Framing::at(0.5, 0.5, 1000.0),
        ] {
            let r = f.resolve(SRC, ASPECT);
            assert!((r.aspect() - ASPECT).abs() < 1e-9, "{f:?} -> {r:?}");
        }
        // Whole must still show everything, so it grows rather than crops.
        let w = Framing::Whole.resolve(SRC, ASPECT);
        assert!(w.w >= SRC.0 as f64 - 1e-6 && w.h >= SRC.1 as f64 - 1e-6);
    }

    #[test]
    fn pull_back_starts_tight_and_ends_wide() {
        let cam = Camera::pull_back(Framing::point(1000.0, 5600.0, 288.0), 4.0);
        let a = cam.viewport_at(Time(0.0), SRC, ASPECT);
        let b = cam.viewport_at(Time(4.0), SRC, ASPECT);
        assert!(a.h < 300.0, "starts tight: {a:?}");
        assert!(b.h >= SRC.1 as f64 - 1.0, "ends showing the whole height: {b:?}");
        // And it is monotonic on the way.
        let mut prev = 0.0;
        for i in 0..=40 {
            let h = cam.viewport_at(Time(i as f64 * 0.1), SRC, ASPECT).h;
            assert!(h >= prev - 1e-6, "zoom must not reverse at t={}", i as f64 * 0.1);
            prev = h;
        }
    }

    #[test]
    fn zoom_is_geometric_not_linear() {
        // Halfway through a pull-back the viewport should be near the
        // *geometric* mean of the endpoints, not the arithmetic one.
        let cam = Camera {
            shots: vec![
                Shot { at: Time(0.0), framing: Framing::point(3416.0, 3512.0, 100.0), ease: Easing::Linear },
                Shot { at: Time(1.0), framing: Framing::point(3416.0, 3512.0, 10000.0), ease: Easing::Linear },
            ],
            clamp_to_source: false,
        };
        let mid = cam.viewport_at(Time(0.5), SRC, ASPECT).h;
        let geometric = (100.0f64 * 10000.0).sqrt(); // 1000
        let arithmetic = (100.0 + 10000.0) / 2.0; // 5050
        assert!((mid - geometric).abs() < 1.0, "got {mid}, want ~{geometric}");
        assert!((mid - arithmetic).abs() > 1000.0);
    }

    #[test]
    fn clamping_keeps_the_viewport_on_the_source() {
        // A framing hanging off the top-left corner gets pushed back on.
        let cam = Camera::hold(Framing::point(0.0, 0.0, 500.0));
        let r = cam.viewport_at(Time(0.0), SRC, ASPECT);
        assert!(r.x >= -1e-9 && r.y >= -1e-9, "{r:?}");
        // Without clamping it is allowed off the edge.
        let free = Camera::hold(Framing::point(0.0, 0.0, 500.0)).no_clamp();
        assert!(free.viewport_at(Time(0.0), SRC, ASPECT).x < 0.0);
    }

    #[test]
    fn holds_before_the_first_and_after_the_last_shot() {
        let cam = Camera::pull_back(Framing::point(500.0, 500.0, 288.0), 2.0);
        let before = cam.viewport_at(Time(-5.0), SRC, ASPECT);
        let at0 = cam.viewport_at(Time(0.0), SRC, ASPECT);
        let after = cam.viewport_at(Time(99.0), SRC, ASPECT);
        let at2 = cam.viewport_at(Time(2.0), SRC, ASPECT);
        assert_eq!(before, at0);
        assert_eq!(after, at2);
    }

    #[test]
    fn hold_until_freezes_the_previous_framing() {
        let cam = Camera::new()
            .to(0.0, Framing::point(100.0, 100.0, 500.0))
            .hold_until(2.0)
            .to(3.0, Framing::Whole);
        let a = cam.viewport_at(Time(0.5), SRC, ASPECT);
        let b = cam.viewport_at(Time(1.9), SRC, ASPECT);
        assert!((a.h - b.h).abs() < 1e-6, "must not drift during the hold");
        assert!(cam.viewport_at(Time(3.0), SRC, ASPECT).h > a.h);
    }
}
