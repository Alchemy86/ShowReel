//! Rendering a film smaller than it was written.
//!
//! The preview loop is the thing Remotion's Studio genuinely buys you, and the
//! scout report named its absence as the gap we would otherwise concede. Half
//! the answer is being able to render the whole film at a quarter size in a
//! fraction of the time and still have it *look* like the film — which means
//! type, padding, corner radii and shadows all have to come down with the
//! frame. A film scaled here is the same film, smaller; only the source-space
//! quantities (camera framings, clip trims, every [`Time`]) are left alone,
//! because they are not measured in frame pixels.
//!
//! [`Time`]: crate::time::Time

use crate::layer::{Content, Layer, Placement};
use crate::text::TextStyle;
use crate::theme::Theme;
use crate::timeline::{Film, Scene};

fn scale_style(s: &mut TextStyle, k: f64) {
    s.size *= k;
    if let Some(sh) = &mut s.shadow {
        sh.blur *= k;
        sh.dx *= k;
        sh.dy *= k;
    }
    // `tracking` and `line_height` are ratios, so they are already scale-free.
}

fn scale_opt(s: &mut Option<TextStyle>, k: f64) {
    if let Some(s) = s {
        scale_style(s, k);
    }
}

fn scale_placement(p: &mut Placement, k: f64) {
    match p {
        Placement::Anchored { pad, .. } => *pad *= k,
        Placement::Rect { x, y, w, h } => {
            *x *= k;
            *y *= k;
            *w *= k;
            *h *= k;
        }
        // Full and Frac are already relative to the frame.
        Placement::Full | Placement::Frac { .. } => {}
    }
}

fn scale_layer(l: &mut Layer, k: f64) {
    if let Some(p) = l.placement.as_mut() {
        scale_placement(p, k);
    }
    // Motion distances are frame-space pixels too.
    for m in [l.enter.as_mut(), l.exit.as_mut()].into_iter().flatten() {
        m.kind = match m.kind {
            crate::motion::MotionKind::Rise { distance } => {
                crate::motion::MotionKind::Rise { distance: distance * k }
            }
            crate::motion::MotionKind::Drop { distance } => {
                crate::motion::MotionKind::Drop { distance: distance * k }
            }
            crate::motion::MotionKind::SlideIn { dx, dy } => {
                crate::motion::MotionKind::SlideIn { dx: dx * k, dy: dy * k }
            }
            crate::motion::MotionKind::Chars { stagger, rise } => {
                crate::motion::MotionKind::Chars { stagger, rise: rise * k }
            }
            crate::motion::MotionKind::Words { stagger, rise } => {
                crate::motion::MotionKind::Words { stagger, rise: rise * k }
            }
            other => other,
        };
    }
    match &mut l.content {
        Content::Text { style, .. } => scale_style(style, k),
        Content::Title { style, subtitle_style, .. } => {
            scale_opt(style, k);
            scale_opt(subtitle_style, k);
        }
        Content::LowerThird { style, detail_style, .. } => {
            scale_opt(style, k);
            scale_opt(detail_style, k);
        }
        Content::Counter { style, label_style, .. } => {
            scale_opt(style, k);
            scale_opt(label_style, k);
        }
        Content::Callout { spec, style, detail_style } => {
            spec.ring *= k;
            scale_opt(style, k);
            scale_opt(detail_style, k);
        }
        Content::PullUp { spec, style } => {
            spec.radius *= k;
            spec.border_width *= k;
            scale_opt(style, k);
        }
        Content::Clip { radius, border, shadow, max_width, .. } => {
            *radius *= k;
            if let Some((_, w)) = border {
                *w *= k;
            }
            if let Some(sh) = shadow {
                sh.blur *= k;
                sh.dx *= k;
                sh.dy *= k;
            }
            // Decoding a clip larger than it can ever be shown is the single
            // biggest waste in a preview pass.
            *max_width = ((*max_width as f64 * k).round() as u32).max(16);
        }
        // A camera's framings are in the *source* image's pixels, which do not
        // change when the output frame does.
        Content::Still { .. } | Content::Solid { .. } | Content::Gradient { .. } | Content::Scrim { .. } => {}
    }
}

fn scale_scene(s: &mut Scene, k: f64) {
    for l in &mut s.layers {
        scale_layer(l, k);
    }
}

/// A film rendered at `k` times its declared size.
///
/// Frame dimensions are rounded to even numbers, because h264 in yuv420p
/// requires it and a preview that cannot be encoded is not a preview.
pub fn scale_film(film: &Film, k: f64) -> Film {
    let mut f = film.clone();
    if (k - 1.0).abs() < 1e-9 {
        return f;
    }
    f.width = (((f.width as f64 * k).round() as u32).max(2) / 2) * 2;
    f.height = (((f.height as f64 * k).round() as u32).max(2) / 2) * 2;
    // Use the dimensions actually landed on, so type never drifts from frame.
    let kx = f.width as f64 / film.width as f64;
    f.theme = Some(match &film.theme {
        Some(t) => t.clone().scaled(kx),
        None => Theme::for_height(film.height).scaled(kx),
    });
    scale_scene(&mut f.timeline.opening, kx);
    for link in &mut f.timeline.then {
        scale_scene(&mut link.scene, kx);
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Anchor;
    use crate::time::Time;
    use crate::transition::Transition;

    fn film() -> Film {
        Film::new(1920, 1080, 60.0)
            .open(
                Scene::new(2.0)
                    .layer(Layer::title("Hello").at(Anchor::Centre))
                    .layer(Layer::text("body").styled(TextStyle::default().size(40.0)))
                    .layer(Layer::clip("c.mp4").framed(20.0, Some((crate::color::Color::WHITE, 4.0)))),
            )
            .then(Transition::dissolve(0.5), Scene::new(2.0).layer(Layer::counter(0.0, 10.0, 1.0)))
    }

    #[test]
    fn halving_halves_the_frame_and_the_type() {
        let s = scale_film(&film(), 0.5);
        assert_eq!((s.width, s.height), (960, 540));
        let Content::Text { style, .. } = &s.timeline.opening.layers[1].content else {
            panic!("expected text")
        };
        assert!((style.size - 20.0).abs() < 1e-9, "got {}", style.size);
    }

    #[test]
    fn placement_padding_scales_but_fractions_do_not() {
        let mut f = film();
        f.timeline.opening.layers.push(Layer::text("x").frac(0.1, 0.2, 0.3, 0.4));
        let s = scale_film(&f, 0.5);
        let Placement::Anchored { pad, .. } = s.timeline.opening.layers[0].placement() else {
            panic!("expected an anchored title")
        };
        assert!((pad - 36.0).abs() < 1e-9, "pad {pad}");
        let Placement::Frac { fx, fw, .. } =
            s.timeline.opening.layers.last().unwrap().placement()
        else {
            panic!("expected fractions")
        };
        assert_eq!((fx, fw), (0.1, 0.3), "fractions must not scale");
    }

    #[test]
    fn clip_treatment_and_decode_width_scale() {
        let s = scale_film(&film(), 0.5);
        let Content::Clip { radius, border, max_width, .. } = &s.timeline.opening.layers[2].content
        else {
            panic!("expected a clip")
        };
        assert!((radius - 10.0).abs() < 1e-9);
        assert!((border.unwrap().1 - 2.0).abs() < 1e-9);
        assert_eq!(*max_width, 960);
    }

    #[test]
    fn timing_never_scales() {
        let s = scale_film(&film(), 0.25);
        assert_eq!(s.duration(), film().duration());
        assert_eq!(s.timeline.then[0].transition.duration, Time(0.5));
    }

    #[test]
    fn odd_results_are_rounded_to_even() {
        let f = Film::new(1921 - 1, 1081 - 1, 30.0).open(Scene::new(1.0));
        let s = scale_film(&f, 0.333);
        assert_eq!(s.width % 2, 0);
        assert_eq!(s.height % 2, 0);
        assert!(s.validate().is_empty(), "{:?}", s.validate());
    }

    #[test]
    fn scaling_by_one_is_the_identity() {
        assert_eq!(scale_film(&film(), 1.0), film());
    }
}
