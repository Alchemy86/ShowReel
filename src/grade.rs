//! A colour grade — the one capability `docs/anarchist-study.md` found
//! ShowReel genuinely lacked, everything else on that study's technique list
//! already being covered by the camera, motion, callout and audio pipelines.
//!
//! It follows [`crate::layer::Content::Parallax`]'s own lesson rather than
//! inventing a second idiom: a composition convenience, not new render
//! machinery. A grade is a single post-composite pass — lift, contrast,
//! saturation, a warm/cool tilt, then a vignette — applied once to a finished
//! scene canvas after every layer has already drawn onto it
//! ([`crate::render::Renderer::draw_scene`]), the same place `Scene::background`
//! already hooks in. It therefore composes with a still, a clip, a parallax
//! stack or any mix of layers with zero knowledge of what any of them draw.
//!
//! Written as a bare word for the built-in look — `"grade": "documentary"` —
//! and as an object when it needs numbers, the same shorthand
//! [`crate::layer::Placement`] uses for the same reason: the common case is
//! what an author wants to type, not `{"kind": "documentary"}`.

use serde::{Deserialize, Serialize};

/// A colour grade: five knobs, all `0.0` (no change) by default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "GradeRepr", into = "GradeRepr")]
pub struct Grade {
    /// -1..1. Pushes tones away from (positive) or toward (negative) mid-grey.
    pub contrast: f64,
    /// -1..1. `-1` is greyscale, `0` unchanged, positive boosts colour.
    pub saturation: f64,
    /// -1..1. Raises the black point (a faded, "shot on film" shadow) when
    /// positive; crushes shadows toward pure black when negative.
    pub lift: f64,
    /// -1..1. A warm (positive, more red/less blue) or cool (negative) tilt
    /// across the whole frame — the "serious documentary" grade leans cool.
    pub temperature: f64,
    /// 0..1. Corner darkening strength; `0` is none.
    pub vignette: f64,
}

impl Default for Grade {
    fn default() -> Self {
        Grade { contrast: 0.0, saturation: 0.0, lift: 0.0, temperature: 0.0, vignette: 0.0 }
    }
}

impl Grade {
    /// Desaturated, contrast-pushed, faintly cool and vignetted — the
    /// "serious documentary" look `docs/anarchist-study.md` names as the
    /// visual signature of the genre the captain pointed at. Tuned by eye
    /// against `examples/grade_demo.rs`'s before/after stills, not derived
    /// from a formula.
    pub fn documentary() -> Self {
        Grade { contrast: 0.22, saturation: -0.35, lift: 0.04, temperature: -0.07, vignette: 0.22 }
    }

    pub fn contrast(mut self, v: f64) -> Self {
        self.contrast = v;
        self
    }

    pub fn saturation(mut self, v: f64) -> Self {
        self.saturation = v;
        self
    }

    pub fn lift(mut self, v: f64) -> Self {
        self.lift = v;
        self
    }

    pub fn temperature(mut self, v: f64) -> Self {
        self.temperature = v;
        self
    }

    pub fn vignette(mut self, v: f64) -> Self {
        self.vignette = v;
        self
    }

    /// True when every knob is at its neutral value — the fast path
    /// [`crate::canvas::apply_grade`] uses to skip the frame entirely.
    pub fn is_noop(&self) -> bool {
        self.contrast == 0.0
            && self.saturation == 0.0
            && self.lift == 0.0
            && self.temperature == 0.0
            && self.vignette == 0.0
    }
}

/// The wire form of [`Grade`].
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum GradeRepr {
    /// A named look — currently just `"documentary"`.
    Word(String),
    Full {
        #[serde(default)]
        contrast: f64,
        #[serde(default)]
        saturation: f64,
        #[serde(default)]
        lift: f64,
        #[serde(default)]
        temperature: f64,
        #[serde(default)]
        vignette: f64,
    },
}

impl From<GradeRepr> for Grade {
    fn from(r: GradeRepr) -> Grade {
        match r {
            GradeRepr::Word(w) => {
                if w.eq_ignore_ascii_case("documentary") {
                    Grade::documentary()
                } else {
                    // An unrecognised word is a no-op grade rather than a
                    // hard error — the same "a typo should cost you a look,
                    // not a render" reasoning `Placement`'s word form uses.
                    Grade::default()
                }
            }
            GradeRepr::Full { contrast, saturation, lift, temperature, vignette } => {
                Grade { contrast, saturation, lift, temperature, vignette }
            }
        }
    }
}

impl From<Grade> for GradeRepr {
    fn from(g: Grade) -> GradeRepr {
        GradeRepr::Full {
            contrast: g.contrast,
            saturation: g.saturation,
            lift: g.lift,
            temperature: g.temperature,
            vignette: g.vignette,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_shorthand_resolves_the_named_look() {
        let g: Grade = serde_json::from_str("\"documentary\"").unwrap();
        assert_eq!(g, Grade::documentary());
    }

    #[test]
    fn an_unrecognised_word_is_a_no_op_not_an_error() {
        let g: Grade = serde_json::from_str("\"nonsense\"").unwrap();
        assert!(g.is_noop());
    }

    #[test]
    fn the_object_form_round_trips() {
        let g = Grade::default().contrast(0.3).saturation(-0.5).vignette(0.1);
        let s = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<Grade>(&s).unwrap(), g);
    }

    #[test]
    fn default_is_a_no_op() {
        assert!(Grade::default().is_noop());
        assert!(!Grade::documentary().is_noop());
    }
}
