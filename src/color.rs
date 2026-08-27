//! Colour, and the paints text and shapes are filled with.

use serde::{Deserialize, Serialize};
use tiny_skia::{GradientStop, LinearGradient, Point, Shader, SpreadMode, Transform};

/// Straight (non-premultiplied) sRGB with alpha.
///
/// Deserialises from `"#rrggbb"`, `"#rrggbbaa"`, `"#rgb"`, or `[r,g,b,a]` in
/// 0..255, so a film file can be written by hand without ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "ColorRepr", into = "ColorRepr")]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };
    pub const BLACK: Color = Color { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    /// The same colour at a different opacity, `k` in 0..=1.
    pub fn opacity(self, k: f64) -> Self {
        Color { a: ((self.a as f64) * k.clamp(0.0, 1.0)).round() as u8, ..self }
    }

    /// Blend towards `other`.
    pub fn mix(self, other: Color, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Color { r: f(self.r, other.r), g: f(self.g, other.g), b: f(self.b, other.b), a: f(self.a, other.a) }
    }

    /// Relative luminance, for deciding whether text over this should be light
    /// or dark.
    pub fn luminance(self) -> f64 {
        let c = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * c(self.r) + 0.7152 * c(self.g) + 0.0722 * c(self.b)
    }

    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }

    pub fn parse(s: &str) -> Option<Color> {
        let s = s.trim().trim_start_matches('#');
        let hex = |i: usize, n: usize| u8::from_str_radix(&s[i..i + n], 16).ok();
        match s.len() {
            3 => {
                let (r, g, b) = (hex(0, 1)?, hex(1, 1)?, hex(2, 1)?);
                Some(Color::rgb(r * 17, g * 17, b * 17))
            }
            6 => Some(Color::rgb(hex(0, 2)?, hex(2, 2)?, hex(4, 2)?)),
            8 => Some(Color::rgba(hex(0, 2)?, hex(2, 2)?, hex(4, 2)?, hex(6, 2)?)),
            _ => None,
        }
    }
}

impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.a == 255 {
            write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            write!(f, "#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum ColorRepr {
    Hex(String),
    Rgba([u8; 4]),
}

impl From<ColorRepr> for Color {
    fn from(r: ColorRepr) -> Color {
        match r {
            ColorRepr::Hex(s) => Color::parse(&s).unwrap_or(Color::BLACK),
            ColorRepr::Rgba([r, g, b, a]) => Color::rgba(r, g, b, a),
        }
    }
}

impl From<Color> for ColorRepr {
    fn from(c: Color) -> ColorRepr {
        ColorRepr::Hex(c.to_string())
    }
}

/// What fills a shape or a glyph.
///
/// Text being a *path* in ShowReel rather than a font-engine blit is what makes
/// a gradient-filled title possible at all; this enum is where that pays off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "PaintRepr", into = "PaintRepr")]
pub enum Paint {
    Solid(Color),
    /// A linear gradient across the shape's own bounding box. `angle` is in
    /// degrees clockwise from "left to right".
    Linear { stops: Vec<(f64, Color)>, angle: f64 },
}

/// The wire form of [`Paint`].
///
/// Untagged rather than internally tagged, because a solid paint should be
/// written as the colour itself — `"fill": "#ffffff"` — not wrapped in a
/// discriminator. The two arms are unambiguous: a colour is a string or a
/// four-element array, a gradient is an object with `stops`.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum PaintRepr {
    Solid(Color),
    Linear { stops: Vec<(f64, Color)>, angle: f64 },
}

impl From<PaintRepr> for Paint {
    fn from(r: PaintRepr) -> Paint {
        match r {
            PaintRepr::Solid(c) => Paint::Solid(c),
            PaintRepr::Linear { stops, angle } => Paint::Linear { stops, angle },
        }
    }
}

impl From<Paint> for PaintRepr {
    fn from(p: Paint) -> PaintRepr {
        match p {
            Paint::Solid(c) => PaintRepr::Solid(c),
            Paint::Linear { stops, angle } => PaintRepr::Linear { stops, angle },
        }
    }
}

impl Paint {
    pub fn solid(c: Color) -> Self {
        Paint::Solid(c)
    }

    /// Multiply the whole paint's opacity — used by fades.
    pub fn opacity(&self, k: f64) -> Paint {
        match self {
            Paint::Solid(c) => Paint::Solid(c.opacity(k)),
            Paint::Linear { stops, angle } => Paint::Linear {
                stops: stops.iter().map(|(p, c)| (*p, c.opacity(k))).collect(),
                angle: *angle,
            },
        }
    }

    /// A representative colour, for callers that cannot draw a gradient.
    pub fn dominant(&self) -> Color {
        match self {
            Paint::Solid(c) => *c,
            Paint::Linear { stops, .. } => stops.first().map(|s| s.1).unwrap_or(Color::WHITE),
        }
    }

    /// Build a tiny-skia shader covering `bbox`.
    pub fn shader<'a>(&self, bbox: crate::geom::Rect) -> Shader<'a> {
        match self {
            Paint::Solid(c) => Shader::SolidColor(c.to_skia()),
            Paint::Linear { stops, angle } => {
                let rad = angle.to_radians();
                let (cx, cy) = bbox.centre();
                // Half-diagonal so the gradient always spans the whole box.
                let r = (bbox.w.hypot(bbox.h)) / 2.0;
                let (dx, dy) = (rad.cos() * r, rad.sin() * r);
                let start = Point::from_xy((cx - dx) as f32, (cy - dy) as f32);
                let end = Point::from_xy((cx + dx) as f32, (cy + dy) as f32);
                let gs: Vec<GradientStop> = stops
                    .iter()
                    .map(|(p, c)| GradientStop::new(*p as f32, c.to_skia()))
                    .collect();
                LinearGradient::new(start, end, gs, SpreadMode::Pad, Transform::identity())
                    .unwrap_or_else(|| Shader::SolidColor(self.dominant().to_skia()))
            }
        }
    }
}

impl From<Color> for Paint {
    fn from(c: Color) -> Paint {
        Paint::Solid(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(Color::parse("#fff"), Some(Color::WHITE));
        assert_eq!(Color::parse("#ff0000"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(Color::parse("#00ff0080"), Some(Color::rgba(0, 255, 0, 128)));
        assert_eq!(Color::parse("nonsense"), None);
    }

    #[test]
    fn round_trips_through_json() {
        let c = Color::rgba(18, 52, 86, 200);
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(s, "\"#12345 6c8\"".replace(' ', ""));
        assert_eq!(serde_json::from_str::<Color>(&s).unwrap(), c);
    }

    #[test]
    fn solid_paint_serialises_as_a_bare_colour() {
        let p = Paint::Solid(Color::WHITE);
        assert_eq!(serde_json::to_string(&p).unwrap(), "\"#ffffff\"");
        assert_eq!(serde_json::from_str::<Paint>("\"#ffffff\"").unwrap(), p);
    }

    #[test]
    fn gradient_paint_round_trips() {
        let g = Paint::Linear { stops: vec![(0.0, Color::BLACK), (1.0, Color::WHITE)], angle: 90.0 };
        let s = serde_json::to_string(&g).unwrap();
        assert!(s.contains("stops"), "{s}");
        assert_eq!(serde_json::from_str::<Paint>(&s).unwrap(), g);
    }

    #[test]
    fn luminance_orders_light_and_dark() {
        assert!(Color::WHITE.luminance() > Color::BLACK.luminance());
        assert!(Color::WHITE.luminance() > 0.9 && Color::BLACK.luminance() < 0.1);
    }
}
