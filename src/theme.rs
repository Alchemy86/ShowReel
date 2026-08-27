//! The default look.
//!
//! A toolset whose defaults are ugly gets used to make ugly things, so the
//! defaults here are a designed set rather than a neutral one: a geometric
//! display face for headings, a humanist face for body copy, tracked-out
//! uppercase for the small labels, tabular figures for anything numeric, and a
//! shadow on everything that might sit over footage.
//!
//! Every field is a plain [`TextStyle`], so overriding one is an assignment
//! rather than a fight with the theme.

use crate::color::{Color, Paint};
use crate::text::{Align, Shadow, TextStyle};
use serde::{Deserialize, Serialize};

/// The families tried, in order, for headings.
pub const DISPLAY: &[&str] = &["Montserrat", "Inter", "Open Sans", "Liberation Sans", "DejaVu Sans"];
/// The families tried, in order, for body copy.
pub const BODY: &[&str] = &["Open Sans", "Inter", "Noto Sans", "Liberation Sans", "DejaVu Sans"];

fn families(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub title: TextStyle,
    pub subtitle: TextStyle,
    pub caption: TextStyle,
    pub lower_third: TextStyle,
    pub lower_third_detail: TextStyle,
    pub counter: TextStyle,
    pub counter_label: TextStyle,
    pub callout: TextStyle,
    pub callout_detail: TextStyle,
    /// The accent used by lower-thirds, callouts and pull-up borders.
    pub accent: Color,
    /// What an empty frame is.
    pub background: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

impl Theme {
    /// Light type on dark footage — the explainer default.
    pub fn dark() -> Self {
        let ink = Color::WHITE;
        let muted = Color::rgb(196, 203, 214);
        Theme {
            title: TextStyle {
                family: families(DISPLAY),
                weight: 800,
                size: 104.0,
                // Large display type needs *negative* tracking or it reads
                // loose; this is the single most visible typographic setting
                // on a title card.
                tracking: -0.018,
                line_height: 1.06,
                fill: Paint::Solid(ink),
                shadow: Some(Shadow { colour: Color::rgba(0, 0, 0, 165), blur: 30.0, dx: 0.0, dy: 8.0 }),
                align: Align::Centre,
                ..TextStyle::default()
            },
            subtitle: TextStyle {
                family: families(BODY),
                weight: 400,
                size: 38.0,
                tracking: 0.004,
                line_height: 1.35,
                fill: Paint::Solid(muted),
                shadow: Some(Shadow::tight()),
                align: Align::Centre,
                ..TextStyle::default()
            },
            caption: TextStyle {
                family: families(BODY),
                weight: 500,
                size: 34.0,
                line_height: 1.4,
                fill: Paint::Solid(ink),
                shadow: Some(Shadow::tight()),
                ..TextStyle::default()
            },
            lower_third: TextStyle {
                family: families(DISPLAY),
                weight: 700,
                size: 46.0,
                tracking: -0.006,
                line_height: 1.1,
                fill: Paint::Solid(ink),
                // On a plate, a shadow is noise — the plate is the contrast.
                shadow: None,
                ..TextStyle::default()
            },
            lower_third_detail: TextStyle {
                family: families(BODY),
                weight: 400,
                size: 27.0,
                tracking: 0.008,
                line_height: 1.3,
                fill: Paint::Solid(muted),
                shadow: None,
                ..TextStyle::default()
            },
            counter: TextStyle {
                family: families(DISPLAY),
                weight: 700,
                size: 82.0,
                tracking: -0.01,
                line_height: 1.0,
                fill: Paint::Solid(ink),
                shadow: Some(Shadow::soft()),
                align: Align::Right,
                tabular: true,
                ..TextStyle::default()
            },
            counter_label: TextStyle {
                family: families(DISPLAY),
                weight: 600,
                size: 22.0,
                // Small uppercase needs generous tracking to stay readable.
                tracking: 0.14,
                fill: Paint::Solid(muted),
                shadow: Some(Shadow::tight()),
                case: crate::text::Case::Upper,
                align: Align::Right,
                ..TextStyle::default()
            },
            callout: TextStyle {
                family: families(DISPLAY),
                weight: 600,
                size: 32.0,
                tracking: -0.002,
                line_height: 1.2,
                fill: Paint::Solid(ink),
                shadow: None,
                ..TextStyle::default()
            },
            callout_detail: TextStyle {
                family: families(BODY),
                weight: 400,
                size: 24.0,
                line_height: 1.35,
                fill: Paint::Solid(muted),
                shadow: None,
                ..TextStyle::default()
            },
            accent: Color::rgb(255, 209, 71),
            background: Color::rgb(8, 10, 14),
        }
    }

    /// Dark type on light backgrounds.
    pub fn light() -> Self {
        let ink = Color::rgb(16, 18, 24);
        let muted = Color::rgb(88, 96, 110);
        let d = Theme::dark();
        Theme {
            title: TextStyle { fill: Paint::Solid(ink), shadow: None, ..d.title },
            subtitle: TextStyle { fill: Paint::Solid(muted), shadow: None, ..d.subtitle },
            caption: TextStyle { fill: Paint::Solid(ink), shadow: None, ..d.caption },
            counter: TextStyle { fill: Paint::Solid(ink), shadow: None, ..d.counter },
            counter_label: TextStyle { fill: Paint::Solid(muted), shadow: None, ..d.counter_label },
            background: Color::rgb(247, 247, 249),
            ..d
        }
    }

    /// Scale every size for a different frame height. A theme tuned at 1080p
    /// looks tiny at 4K and enormous on a phone cut; this keeps one set of
    /// numbers honest across all of them.
    pub fn scaled(mut self, k: f64) -> Self {
        for s in [
            &mut self.title,
            &mut self.subtitle,
            &mut self.caption,
            &mut self.lower_third,
            &mut self.lower_third_detail,
            &mut self.counter,
            &mut self.counter_label,
            &mut self.callout,
            &mut self.callout_detail,
        ] {
            s.size *= k;
            if let Some(sh) = &mut s.shadow {
                sh.blur *= k;
                sh.dx *= k;
                sh.dy *= k;
            }
        }
        self
    }

    /// The theme scaled for a frame `height` pixels tall, relative to 1080.
    pub fn for_height(height: u32) -> Self {
        Theme::dark().scaled(height as f64 / 1080.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_styles_are_tracked_tighter_than_labels() {
        let t = Theme::dark();
        assert!(t.title.tracking < 0.0, "big display type needs negative tracking");
        assert!(t.counter_label.tracking > 0.1, "small caps need generous tracking");
    }

    #[test]
    fn counters_default_to_tabular() {
        assert!(Theme::dark().counter.tabular);
    }

    #[test]
    fn scaling_moves_sizes_and_shadows_together() {
        let base = Theme::dark();
        let big = Theme::dark().scaled(2.0);
        assert!((big.title.size - base.title.size * 2.0).abs() < 1e-9);
        assert!(
            (big.title.shadow.as_ref().unwrap().blur
                - base.title.shadow.as_ref().unwrap().blur * 2.0)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn for_height_is_identity_at_1080() {
        assert_eq!(Theme::for_height(1080).title.size, Theme::dark().title.size);
    }

    #[test]
    fn light_theme_drops_shadows_and_darkens_ink() {
        let l = Theme::light();
        assert!(l.title.shadow.is_none());
        assert!(l.title.fill.dominant().luminance() < 0.1);
        assert!(l.background.luminance() > 0.8);
    }

    #[test]
    fn round_trips_through_json() {
        let t = Theme::dark();
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Theme>(&s).unwrap(), t);
    }
}
