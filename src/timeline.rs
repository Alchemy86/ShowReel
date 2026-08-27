//! Films, scenes and the timeline that joins them.
//!
//! # Why this shape
//!
//! Remotion's `<TransitionSeries>` is a flat list of children, so
//! `<Transition>` first, two transitions in a row, or a transition longer than
//! its neighbours are all *writable* — and are caught by runtime errors after
//! you have already started a render. The timeline here is
//!
//! ```text
//! Timeline := Scene (Transition Scene)*
//! ```
//!
//! which makes the first three mistakes unrepresentable, in Rust and in the
//! JSON alike, and leaves only the duration rule to check. That is the honest
//! Rust answer to a React API: not a transliteration, but the same idea with
//! the invariant moved into the type.
//!
//! # Comments in film files
//!
//! [`Film::from_json`] reads plain JSON exactly as before, and also a narrow
//! JSONC subset: `//` and `/* */` comments, plus a trailing comma after the
//! last element of an array or object. That is the whole allowance — nothing
//! else JSONC-flavoured parsers tend to also permit (unquoted property names,
//! single-quoted strings, hex numbers, a leading `+`) is accepted, so a film
//! file stays recognisably JSON with two deliberate, named exceptions rather
//! than drifting toward JSON5. Comments matter because the film file is the
//! interface: it is meant to be read by a person, and "why does this scene
//! hold for four seconds" cannot live in a format with no comment syntax.
//! Trailing commas are a separate, smaller convenience worth taking at the
//! same time — reordering or deleting the last line of a hand-edited list is
//! common and a bare comma error there is pure friction.
//!
//! [`jsonc-parser`](https://docs.rs/jsonc-parser) does the parsing, chosen
//! over the alternatives:
//! - `json5` implements the full JSON5 grammar — unquoted keys, single quotes,
//!   hex and leading-`+` numbers included — which is a wider dialect than
//!   "JSON plus comments", and it deserialises through its *own* independent
//!   `serde::Deserializer` rather than `serde_json`'s. This crate leans on
//!   untagged enums and `#[serde(flatten)]` in several places (see the crate
//!   root docs), and a second, less battle-tested `Deserializer`
//!   implementation is exactly where those would misbehave first.
//! - `serde_jsonrc` is a `serde_json` fork with comment support, but its last
//!   release was in 2019; treated as unmaintained and ruled out on that
//!   basis alone.
//! - `jsonc-parser` is maintained (dprint/deno tooling), has zero required
//!   dependencies (`serde` is opt-in, and already a dependency here), and its
//!   `ParseOptions` exposes comments and trailing commas as two independent
//!   flags — which is what let this be a considered, narrow decision instead
//!   of an all-or-nothing dialect switch. It is used here to parse into a
//!   `serde_json::Value` first; the actual type-level deserialisation into
//!   [`Film`] then still goes through `serde_json`'s own `Deserializer`,
//!   unchanged from before this was added.
//!
//! `Film::to_json` still emits plain JSON: there is no general way to
//! reconstruct prose comments from a Rust builder, so nothing here tries.
//! That means regenerating `examples/kanto.film.jsonc` from
//! `examples/kanto_reel.rs` would discard any comments hand-added to the
//! committed copy — so `kanto_reel --check` compares the two *structurally*
//! (parse both, compare the resulting `Film`s) rather than as text, and
//! comments are free to live in the committed file without the drift guard
//! flagging them as a mismatch. See `examples/README.md` for the full
//! reasoning and its trade-off.

use crate::audio::Audio;
use crate::color::Color;
use crate::layer::Layer;
use crate::theme::Theme;
use crate::time::Time;
use crate::transition::Transition;
use serde::{Deserialize, Serialize};

/// One continuous shot: a background and a stack of layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub duration: Time,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
    #[serde(default)]
    pub layers: Vec<Layer>,
}

impl Scene {
    pub fn new(duration: impl Into<Time>) -> Self {
        Scene { name: None, duration: duration.into(), background: None, layers: Vec::new() }
    }

    pub fn named(mut self, n: impl Into<String>) -> Self {
        self.name = Some(n.into());
        self
    }

    pub fn background(mut self, c: Color) -> Self {
        self.background = Some(c);
        self
    }

    pub fn layer(mut self, l: Layer) -> Self {
        self.layers.push(l);
        self
    }

    pub fn layers(mut self, ls: impl IntoIterator<Item = Layer>) -> Self {
        self.layers.extend(ls);
        self
    }

    /// Layers in draw order: by `z`, then by declaration order.
    ///
    /// `sort_by_key` is stable, so equal `z` keeps the order they were written
    /// in — which is what an author expects and what makes `z` optional.
    pub fn draw_order(&self) -> Vec<&Layer> {
        let mut v: Vec<&Layer> = self.layers.iter().collect();
        v.sort_by_key(|l| l.z);
        v
    }
}

/// A scene, and the transition that leads into it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub transition: Transition,
    pub scene: Scene,
}

/// `Scene (Transition Scene)*`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub opening: Scene,
    #[serde(default)]
    pub then: Vec<Link>,
}

/// Where a scene sits on the film's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub index: usize,
    pub start: Time,
    pub duration: Time,
}

impl Placed {
    pub fn end(&self) -> Time {
        self.start + self.duration
    }
}

/// What is on screen at a given instant.
#[derive(Debug)]
pub enum Cut<'a> {
    /// One scene, at `local` seconds into itself.
    Single { scene: &'a Scene, local: Time },
    /// Two scenes overlapping through a transition.
    Blend {
        outgoing: &'a Scene,
        outgoing_local: Time,
        incoming: &'a Scene,
        incoming_local: Time,
        transition: &'a Transition,
        /// Seconds into the transition.
        elapsed: f64,
    },
}

impl Timeline {
    pub fn new(opening: Scene) -> Self {
        Timeline { opening, then: Vec::new() }
    }

    pub fn scene_count(&self) -> usize {
        1 + self.then.len()
    }

    pub fn scene(&self, i: usize) -> &Scene {
        if i == 0 { &self.opening } else { &self.then[i - 1].scene }
    }

    /// The transition *into* scene `i`, if any.
    pub fn transition_into(&self, i: usize) -> Option<&Transition> {
        (i > 0).then(|| &self.then[i - 1].transition)
    }

    /// Every scene's start time.
    ///
    /// A transition overlaps its two neighbours, so each scene starts one
    /// transition-duration before the previous one ends. This is the single
    /// place that arithmetic lives.
    pub fn placements(&self) -> Vec<Placed> {
        let mut out = Vec::with_capacity(self.scene_count());
        let mut cursor = Time::ZERO;
        out.push(Placed { index: 0, start: cursor, duration: self.opening.duration });
        for (i, link) in self.then.iter().enumerate() {
            let prev = out[i];
            let start = prev.end() - link.transition.duration;
            out.push(Placed { index: i + 1, start, duration: link.scene.duration });
            cursor = start + link.scene.duration;
        }
        let _ = cursor;
        out
    }

    /// Total running time: the sum of the scenes, less every transition.
    pub fn duration(&self) -> Time {
        let scenes: f64 = (0..self.scene_count()).map(|i| self.scene(i).duration.as_secs()).sum();
        let overlaps: f64 = self.then.iter().map(|l| l.transition.duration.as_secs()).sum();
        Time((scenes - overlaps).max(0.0))
    }

    /// What to draw at film time `t`.
    pub fn at(&self, t: Time) -> Cut<'_> {
        let placed = self.placements();
        // Walk backwards: the later scene wins where two are live, and the
        // transition it arrived through is the one to apply.
        for (i, p) in placed.iter().enumerate().rev() {
            if t < p.start {
                continue;
            }
            if let Some(tr) = self.transition_into(i) {
                let elapsed = (t - p.start).as_secs();
                if elapsed < tr.duration.as_secs() {
                    let prev = placed[i - 1];
                    return Cut::Blend {
                        outgoing: self.scene(i - 1),
                        outgoing_local: t - prev.start,
                        incoming: self.scene(i),
                        incoming_local: t - p.start,
                        transition: tr,
                        elapsed,
                    };
                }
            }
            return Cut::Single { scene: self.scene(i), local: t - p.start };
        }
        Cut::Single { scene: &self.opening, local: t }
    }

    /// The rules a type cannot carry.
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        for i in 0..self.scene_count() {
            let s = self.scene(i);
            let label = s.name.clone().unwrap_or_else(|| format!("scene {i}"));
            if s.duration.as_secs() <= 0.0 {
                errs.push(format!("{label}: duration must be positive"));
            }
            let into = self.transition_into(i).map(|t| t.duration.as_secs()).unwrap_or(0.0);
            let out = self
                .then
                .get(i)
                .map(|l| l.transition.duration.as_secs())
                .unwrap_or(0.0);
            if into > s.duration.as_secs() + 1e-9 {
                errs.push(format!(
                    "{label}: the transition into it ({into}s) is longer than the scene ({}s)",
                    s.duration.as_secs()
                ));
            }
            if out > s.duration.as_secs() + 1e-9 {
                errs.push(format!(
                    "{label}: the transition out of it ({out}s) is longer than the scene ({}s)",
                    s.duration.as_secs()
                ));
            }
            // Two transitions cannot both be running inside one scene, or the
            // scene is never seen on its own.
            if into + out > s.duration.as_secs() + 1e-9 {
                errs.push(format!(
                    "{label}: its transitions overlap ({into}s + {out}s > {}s)",
                    s.duration.as_secs()
                ));
            }
        }
        errs
    }
}

/// A complete film.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Film {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What shows through where nothing is drawn.
    #[serde(default = "default_bg")]
    pub background: Color,
    /// Overrides the built-in defaults for unstyled text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// Sound under the film, placed on the film's own clock. See
    /// [`crate::audio`] for why this sits here and not on a scene.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio: Vec<Audio>,
    #[serde(flatten)]
    pub timeline: Timeline,
}

fn default_bg() -> Color {
    Color::rgb(8, 10, 14)
}

/// The exact JSONC allowance: comments and trailing commas, nothing else.
/// `jsonc_parser::ParseOptions::default()` turns on every laxity the crate
/// knows — loose (unquoted) property names, single-quoted strings, hex
/// numbers, a leading `+` on numbers, missing commas — which is closer to
/// JSON5 than to "JSON with comments". Naming every field here keeps that a
/// deliberate, visible choice rather than an accident of a library default.
const JSONC_OPTIONS: jsonc_parser::ParseOptions = jsonc_parser::ParseOptions {
    allow_comments: true,
    allow_trailing_commas: true,
    allow_loose_object_property_names: false,
    allow_missing_commas: false,
    allow_single_quoted_strings: false,
    allow_hexadecimal_numbers: false,
    allow_unary_plus_numbers: false,
};

/// Everything about a film except its scenes.
///
/// Exists so that [`FilmSpec::open`] is the only way to make a [`Film`]: a film
/// with no scenes is not a thing, and the builder should not be able to
/// produce one.
#[derive(Debug, Clone)]
pub struct FilmSpec {
    width: u32,
    height: u32,
    fps: f64,
    title: Option<String>,
    background: Color,
    theme: Option<Theme>,
}

impl Film {
    /// Begin describing a film. Finish with [`FilmSpec::open`].
    ///
    /// Returns a [`FilmSpec`] rather than a `Film` on purpose: a film without
    /// an opening scene is not a film, and the builder should not be able to
    /// produce one.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(width: u32, height: u32, fps: f64) -> FilmSpec {
        FilmSpec {
            width,
            height,
            fps,
            title: None,
            background: default_bg(),
            theme: None,
        }
    }

    /// Add a scene, joined by a transition.
    pub fn then(mut self, transition: Transition, scene: Scene) -> Self {
        self.timeline.then.push(Link { transition, scene });
        self
    }

    /// Add a scene with a hard cut.
    pub fn cut_to(self, scene: Scene) -> Self {
        self.then(Transition::cut(), scene)
    }

    /// Lay a track under the film. Repeatable — tracks are mixed.
    ///
    /// It is deliberately not part of [`FilmSpec`]: a track's default length
    /// is "to the end of the film", which is not known until the scenes are.
    pub fn sound(mut self, track: Audio) -> Self {
        self.audio.push(track);
        self
    }

    /// Every audio asset the film refers to, in declaration order.
    ///
    /// Unlike [`Film::assets_used`] these are not deduplicated or decoded:
    /// ffmpeg reads each one itself, and the same file placed twice is two
    /// legitimate inputs.
    pub fn audio_assets(&self) -> Vec<&str> {
        self.audio.iter().map(|a| a.asset.as_str()).collect()
    }

    pub fn duration(&self) -> Time {
        self.timeline.duration()
    }

    pub fn frame_count(&self) -> u32 {
        self.duration().frame_count(self.fps).max(1)
    }

    pub fn frame_rect(&self) -> crate::geom::Rect {
        crate::geom::Rect::from_size(self.width as f64, self.height as f64)
    }

    pub fn aspect(&self) -> f64 {
        self.width as f64 / self.height as f64
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone().unwrap_or_else(|| Theme::for_height(self.height))
    }

    /// Every asset the film refers to, deduplicated, in declaration order.
    ///
    /// Used to decode everything once up front rather than having twenty
    /// worker threads discover the same file at the same moment.
    pub fn assets_used(&self) -> Vec<AssetUse> {
        let mut seen = Vec::new();
        for i in 0..self.timeline.scene_count() {
            for l in &self.timeline.scene(i).layers {
                let u = match &l.content {
                    crate::layer::Content::Still { asset, .. } => {
                        Some(AssetUse::Still(asset.clone()))
                    }
                    crate::layer::Content::Clip { asset, max_width, trim, decode_fps, .. } => {
                        Some(AssetUse::Clip {
                            asset: asset.clone(),
                            max_width: *max_width,
                            trim: trim.map(|(a, b)| (a.as_secs(), b.as_secs())),
                            decode_fps: *decode_fps,
                        })
                    }
                    _ => None,
                };
                if let Some(u) = u
                    && !seen.contains(&u)
                {
                    seen.push(u);
                }
            }
        }
        seen
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errs = self.timeline.validate();
        if self.width == 0 || self.height == 0 {
            errs.push("frame size must be non-zero".into());
        }
        if self.fps <= 0.0 {
            errs.push("fps must be positive".into());
        }
        // An mp4 in yuv420p needs even dimensions; catching it here beats an
        // ffmpeg error after a long render.
        if self.width % 2 == 1 || self.height % 2 == 1 {
            errs.push(format!(
                "frame size {}x{} has an odd dimension; h264 needs both even",
                self.width, self.height
            ));
        }
        let total = self.duration();
        for (i, a) in self.audio.iter().enumerate() {
            errs.extend(a.validate(&format!("audio {i} ({})", a.asset), total));
        }
        errs
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parses a film description. Accepts plain JSON, and also the narrow
    /// JSONC subset described in the module docs above: `//` and `/* */`
    /// comments, and a trailing comma on the last element of an array or
    /// object. Every plain-JSON file that loaded before still loads
    /// unchanged — this is purely additive.
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        let value: serde_json::Value = jsonc_parser::parse_to_serde_value(s, &JSONC_OPTIONS)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let s = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        Film::from_json(&s).map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))
    }
}

/// An asset reference, with how it will be decoded.
#[derive(Debug, Clone, PartialEq)]
pub enum AssetUse {
    Still(String),
    Clip { asset: String, max_width: u32, trim: Option<(f64, f64)>, decode_fps: Option<f64> },
}

impl FilmSpec {
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }

    pub fn background(mut self, c: Color) -> Self {
        self.background = c;
        self
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = Some(t);
        self
    }

    /// Give the film its opening scene, producing a [`Film`].
    pub fn open(self, opening: Scene) -> Film {
        Film {
            width: self.width,
            height: self.height,
            fps: self.fps,
            title: self.title,
            background: self.background,
            theme: self.theme,
            audio: Vec::new(),
            timeline: Timeline::new(opening),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Direction;

    fn film() -> Film {
        Film::new(1920, 1080, 60.0)
            .open(Scene::new(4.0).named("a"))
            .then(Transition::dissolve(1.0), Scene::new(4.0).named("b"))
    }

    #[test]
    fn a_transition_shortens_the_film() {
        // 4 + 4 - 1 = 7, not 9.
        assert!((film().duration().as_secs() - 7.0).abs() < 1e-9);
        assert_eq!(film().frame_count(), 420);
    }

    #[test]
    fn scenes_are_placed_with_the_overlap() {
        let p = film().timeline.placements();
        assert_eq!(p[0].start, Time(0.0));
        // b starts a transition-length before a ends.
        assert_eq!(p[1].start, Time(3.0));
    }

    #[test]
    fn the_cut_is_single_outside_the_transition_and_blended_inside() {
        let f = film();
        match f.timeline.at(Time(1.0)) {
            Cut::Single { scene, local } => {
                assert_eq!(scene.name.as_deref(), Some("a"));
                assert_eq!(local, Time(1.0));
            }
            other => panic!("expected a single scene, got {other:?}"),
        }
        match f.timeline.at(Time(3.5)) {
            Cut::Blend { outgoing, incoming, elapsed, outgoing_local, incoming_local, .. } => {
                assert_eq!(outgoing.name.as_deref(), Some("a"));
                assert_eq!(incoming.name.as_deref(), Some("b"));
                assert!((elapsed - 0.5).abs() < 1e-9);
                assert!((outgoing_local.as_secs() - 3.5).abs() < 1e-9);
                assert!((incoming_local.as_secs() - 0.5).abs() < 1e-9);
            }
            other => panic!("expected a blend, got {other:?}"),
        }
        match f.timeline.at(Time(6.9)) {
            Cut::Single { scene, .. } => assert_eq!(scene.name.as_deref(), Some("b")),
            other => panic!("expected scene b, got {other:?}"),
        }
    }

    #[test]
    fn a_three_scene_film_places_every_scene() {
        let f = film().then(Transition::wipe(0.5, Direction::Left), Scene::new(3.0).named("c"));
        // 4 + 4 + 3 - 1 - 0.5 = 9.5
        assert!((f.duration().as_secs() - 9.5).abs() < 1e-9);
        let p = f.timeline.placements();
        assert_eq!(p.len(), 3);
        assert_eq!(p[2].start, Time(6.5));
        match f.timeline.at(Time(8.0)) {
            Cut::Single { scene, .. } => assert_eq!(scene.name.as_deref(), Some("c")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn validation_catches_an_over_long_transition() {
        let f = Film::new(1920, 1080, 60.0)
            .open(Scene::new(1.0).named("short"))
            .then(Transition::dissolve(2.0), Scene::new(4.0).named("b"));
        let errs = f.validate();
        assert!(!errs.is_empty(), "should have complained");
        assert!(errs.iter().any(|e| e.contains("longer than the scene")), "{errs:?}");
    }

    #[test]
    fn validation_catches_overlapping_transitions() {
        let f = Film::new(1920, 1080, 60.0)
            .open(Scene::new(4.0))
            .then(Transition::dissolve(1.5), Scene::new(2.0).named("squeezed"))
            .then(Transition::dissolve(1.5), Scene::new(4.0));
        assert!(f.validate().iter().any(|e| e.contains("overlap")), "{:?}", f.validate());
    }

    #[test]
    fn validation_catches_odd_frame_dimensions() {
        let f = Film::new(1921, 1080, 60.0).open(Scene::new(1.0));
        assert!(f.validate().iter().any(|e| e.contains("even")), "{:?}", f.validate());
    }

    #[test]
    fn a_valid_film_validates_clean() {
        assert!(film().validate().is_empty());
    }

    #[test]
    fn draw_order_is_z_then_declaration() {
        let s = Scene::new(1.0)
            .layer(Layer::text("first").z(0))
            .layer(Layer::text("second").z(0))
            .layer(Layer::text("under").z(-5));
        let order: Vec<_> = s
            .draw_order()
            .iter()
            .map(|l| match &l.content {
                crate::layer::Content::Text { text, .. } => text.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(order, vec!["under", "first", "second"]);
    }

    #[test]
    fn films_round_trip_through_json() {
        let f = film();
        let s = f.to_json().unwrap();
        assert_eq!(Film::from_json(&s).unwrap(), f);
        // The JSON shape itself cannot express a leading transition.
        assert!(s.contains("\"opening\""), "{s}");
    }

    #[test]
    fn plain_json_still_loads_unchanged() {
        // The addition is purely additive: every file that parsed as strict
        // JSON before must still parse, byte for byte, the same way.
        let f = film();
        assert_eq!(Film::from_json(&f.to_json().unwrap()).unwrap(), f);
    }

    #[test]
    fn comments_and_trailing_commas_are_accepted() {
        let src = r##"{
            // a line comment on its own line
            "width": 64, "height": 36, /* a block comment mid-line */ "fps": 30.0,
            "opening": {
                "duration": 2.0,
                "layers": [
                    // trailing comma after the last (only) element
                    { "type": "solid", "colour": "#ffffff" },
                ],
            }, // trailing comma after the last object field
        }"##;
        let f = Film::from_json(src).unwrap();
        assert_eq!(f.width, 64);
        assert_eq!(f.height, 36);
        assert_eq!(f.timeline.scene_count(), 1);
    }

    #[test]
    fn jsonc_laxity_stops_at_comments_and_trailing_commas() {
        // Unquoted keys are JSON5-shaped, not JSONC, and this crate does not
        // accept them: the allowance is exactly comments and trailing
        // commas, not a slide toward JSON5.
        let src = r#"{ width: 64, "height": 36, "fps": 30.0, "opening": { "duration": 1.0 } }"#;
        assert!(Film::from_json(src).is_err());
    }

    #[test]
    fn clip_asset_use_carries_the_decode_parameters() {
        // The same file trimmed two ways is two decodes, not one.
        let f = Film::new(64, 36, 30.0).open(
            Scene::new(1.0)
                .layer(Layer::clip("a.mp4").trim(0.0, 2.0))
                .layer(Layer::clip("a.mp4").trim(10.0, 2.0)),
        );
        assert_eq!(f.assets_used().len(), 2);
    }

    #[test]
    fn assets_used_deduplicates() {
        let f = Film::new(64, 36, 30.0).open(
            Scene::new(1.0)
                .layer(Layer::still("map.png"))
                .layer(Layer::still("map.png"))
                .layer(Layer::still("other.png")),
        );
        assert_eq!(f.assets_used().len(), 2);
    }
}
