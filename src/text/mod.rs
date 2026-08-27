//! Typography: styles, layout, and drawing text that looks designed.
//!
//! "Really nice on screen wording" was named as the headline capability, so
//! this module is deliberately opinionated rather than a thin wrapper over a
//! glyph rasteriser. It owns the things that actually separate a professional
//! explainer from a homemade one:
//!
//! - **real shaping** (kerning and ligatures from the font, via `rustybuzz`);
//! - **tracking in ems**, so a style reads the same at any size;
//! - **tabular figures**, so a counter ticking up does not jitter;
//! - **shadows and outlines**, so white text survives being laid over footage;
//! - **word wrap and auto-fit**, so a long caption shrinks instead of running
//!   off the frame;
//! - **per-character and per-word staggering**, so text can arrive rather than
//!   appear.

pub mod font;

pub use font::{FontDb, FontId, PositionedGlyph};

use crate::canvas::Canvas;
use crate::color::{Color, Paint};
use crate::geom::{Anchor, Rect};
use serde::{Deserialize, Serialize};
use tiny_skia::{PathBuilder, Transform};

/// A soft shadow behind text or a shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub colour: Color,
    /// Blur radius in pixels.
    pub blur: f64,
    pub dx: f64,
    pub dy: f64,
}

impl Shadow {
    /// The default that makes light text readable on anything.
    pub fn soft() -> Self {
        Shadow { colour: Color::rgba(0, 0, 0, 170), blur: 14.0, dx: 0.0, dy: 4.0 }
    }

    /// A tight, dark shadow for small text over busy footage.
    pub fn tight() -> Self {
        Shadow { colour: Color::rgba(0, 0, 0, 200), blur: 5.0, dx: 0.0, dy: 2.0 }
    }

    /// A glow rather than a shadow — for text over dark, low-contrast footage.
    pub fn glow(colour: Color) -> Self {
        Shadow { colour, blur: 24.0, dx: 0.0, dy: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Case {
    #[default]
    AsWritten,
    Upper,
    Lower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    #[default]
    Left,
    Centre,
    Right,
}

/// Everything about how a run of text looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    /// Families tried in order. A path to a font file also works.
    pub family: Vec<String>,
    pub weight: u16,
    pub italic: bool,
    pub size: f64,
    /// Letter-spacing as a fraction of the em.
    pub tracking: f64,
    /// Line spacing as a multiple of the size.
    pub line_height: f64,
    pub fill: Paint,
    /// An outline around the glyphs: paint and width in pixels.
    pub stroke: Option<(Paint, f64)>,
    pub shadow: Option<Shadow>,
    pub case: Case,
    pub align: Align,
    /// Lay every digit on the widest digit's advance.
    pub tabular: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            family: vec!["Montserrat".into(), "Open Sans".into(), "DejaVu Sans".into()],
            weight: 600,
            italic: false,
            size: 56.0,
            tracking: 0.0,
            line_height: 1.15,
            fill: Paint::Solid(Color::WHITE),
            stroke: None,
            shadow: Some(Shadow::soft()),
            case: Case::AsWritten,
            align: Align::Left,
            tabular: false,
        }
    }
}

impl TextStyle {
    pub fn size(mut self, s: f64) -> Self {
        self.size = s;
        self
    }
    pub fn weight(mut self, w: u16) -> Self {
        self.weight = w;
        self
    }
    pub fn family(mut self, f: impl Into<String>) -> Self {
        self.family.insert(0, f.into());
        self
    }
    pub fn tracking(mut self, t: f64) -> Self {
        self.tracking = t;
        self
    }
    pub fn line_height(mut self, l: f64) -> Self {
        self.line_height = l;
        self
    }
    pub fn fill(mut self, p: impl Into<Paint>) -> Self {
        self.fill = p.into();
        self
    }
    pub fn colour(mut self, c: Color) -> Self {
        self.fill = Paint::Solid(c);
        self
    }
    pub fn stroke(mut self, p: impl Into<Paint>, w: f64) -> Self {
        self.stroke = Some((p.into(), w));
        self
    }
    pub fn shadow(mut self, s: Shadow) -> Self {
        self.shadow = Some(s);
        self
    }
    pub fn no_shadow(mut self) -> Self {
        self.shadow = None;
        self
    }
    pub fn upper(mut self) -> Self {
        self.case = Case::Upper;
        self
    }
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
    pub fn centred(mut self) -> Self {
        self.align = Align::Centre;
        self
    }
    pub fn tabular(mut self) -> Self {
        self.tabular = true;
        self
    }

    pub fn apply_case(&self, s: &str) -> String {
        match self.case {
            Case::AsWritten => s.to_string(),
            Case::Upper => s.to_uppercase(),
            Case::Lower => s.to_lowercase(),
        }
    }

    pub fn resolve_font(&self, db: &FontDb) -> Option<FontId> {
        let refs: Vec<&str> = self.family.iter().map(|s| s.as_str()).collect();
        db.select_any(&refs, self.weight, self.italic)
    }
}

/// A laid-out line.
#[derive(Debug, Clone)]
pub struct Line {
    pub glyphs: Vec<PositionedGlyph>,
    pub width: f64,
    pub text: String,
}

/// Text, laid out and ready to draw.
#[derive(Debug, Clone)]
pub struct TextLayout {
    pub lines: Vec<Line>,
    pub font: FontId,
    pub size: f64,
    /// Distance between baselines.
    pub line_step: f64,
    /// Baseline offset of the first line from the block's top edge.
    pub first_baseline: f64,
    pub width: f64,
    pub height: f64,
    /// Total glyph count, for staggering.
    pub glyph_count: usize,
}

impl TextLayout {
    /// Lay `text` out, wrapping to `max_width` if given.
    pub fn build(db: &FontDb, text: &str, style: &TextStyle, max_width: Option<f64>) -> Option<Self> {
        let font = style.resolve_font(db)?;
        let text = style.apply_case(text);
        let (asc, desc, gap) = db.vertical_metrics(font);
        let upem = db.units_per_em(font);
        let scale = style.size / upem;
        // Use the font's own metrics for the natural step, then let
        // `line_height` scale it — so 1.0 means "as the designer set it".
        let natural = (asc - desc + gap) * scale;
        let line_step = natural * style.line_height;
        let first_baseline = asc * scale;

        let mut raw_lines: Vec<String> = Vec::new();
        for para in text.split('\n') {
            match max_width {
                None => raw_lines.push(para.to_string()),
                Some(mw) => raw_lines.extend(wrap(db, font, para, style, mw)),
            }
        }
        if raw_lines.is_empty() {
            raw_lines.push(String::new());
        }

        let mut lines = Vec::with_capacity(raw_lines.len());
        let mut width = 0.0f64;
        let mut glyph_count = 0usize;
        for l in raw_lines {
            let glyphs = if style.tabular {
                shape_tabular(db, font, &l, style)
            } else {
                db.shape(font, &l, style.size, style.tracking)
            };
            let w = if style.tabular {
                tabular_width(db, font, &l, style)
            } else {
                db.measure(font, &l, style.size, style.tracking)
            };
            width = width.max(w);
            glyph_count += glyphs.len();
            lines.push(Line { glyphs, width: w, text: l });
        }
        let height = if lines.len() <= 1 {
            (asc - desc) * scale
        } else {
            line_step * (lines.len() - 1) as f64 + (asc - desc) * scale
        };
        Some(TextLayout { lines, font, size: style.size, line_step, first_baseline, width, height, glyph_count })
    }

    /// Lay out at the largest size that fits `box_`, never above `style.size`.
    ///
    /// This is the "a caption must not run off the frame" guarantee. Remotion
    /// ships this as `fitText` in a separate package; here it is the same call
    /// with one more argument, because needing it is the common case.
    pub fn fit(db: &FontDb, text: &str, style: &TextStyle, box_: Rect) -> Option<Self> {
        let mut lo = 4.0f64;
        let mut hi = style.size;
        let fits = |s: f64| -> Option<TextLayout> {
            let st = TextStyle { size: s, ..style.clone() };
            let l = TextLayout::build(db, text, &st, Some(box_.w))?;
            (l.width <= box_.w + 0.5 && l.height <= box_.h + 0.5).then_some(l)
        };
        if let Some(l) = fits(hi) {
            return Some(l);
        }
        let mut best = None;
        // 12 halvings resolves the size to well under a pixel.
        for _ in 0..12 {
            let mid = (lo + hi) / 2.0;
            match fits(mid) {
                Some(l) => {
                    best = Some(l);
                    lo = mid;
                }
                None => hi = mid,
            }
        }
        best.or_else(|| {
            let st = TextStyle { size: lo, ..style.clone() };
            TextLayout::build(db, text, &st, Some(box_.w))
        })
    }

    /// Where this block sits when anchored in `container`.
    pub fn place(&self, container: &Rect, anchor: Anchor, pad: f64) -> Rect {
        anchor.place(container, self.width, self.height, pad)
    }

    /// The x offset of line `i` within a block of `self.width`, per alignment.
    fn line_offset(&self, i: usize, align: Align) -> f64 {
        let w = self.lines[i].width;
        match align {
            Align::Left => 0.0,
            Align::Centre => (self.width - w) / 2.0,
            Align::Right => self.width - w,
        }
    }
}

fn wrap(db: &FontDb, font: FontId, text: &str, style: &TextStyle, max_width: f64) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for w in words {
        let candidate = if current.is_empty() { w.to_string() } else { format!("{current} {w}") };
        if db.measure(font, &candidate, style.size, style.tracking) <= max_width || current.is_empty() {
            current = candidate;
        } else {
            lines.push(std::mem::take(&mut current));
            current = w.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Shape with every digit on the same advance.
fn shape_tabular(db: &FontDb, font: FontId, text: &str, style: &TextStyle) -> Vec<PositionedGlyph> {
    let adv = db.digit_advance(font, style.size, style.tracking);
    let mut out = Vec::new();
    let mut pen = 0.0f64;
    for (i, ch) in text.char_indices() {
        let s = ch.to_string();
        let glyphs = db.shape(font, &s, style.size, style.tracking);
        if ch.is_ascii_digit() {
            let w = db.measure(font, &s, style.size, style.tracking);
            // Centre the digit in its cell, which is what a real tabular
            // figure does rather than left-aligning it.
            let pad = (adv - w) / 2.0;
            for g in &glyphs {
                out.push(PositionedGlyph { x: pen + pad + g.x, cluster: i as u32, ..*g });
            }
            pen += adv;
        } else {
            for g in &glyphs {
                out.push(PositionedGlyph { x: pen + g.x, cluster: i as u32, ..*g });
            }
            pen += db.measure(font, &s, style.size, style.tracking);
        }
    }
    out
}

fn tabular_width(db: &FontDb, font: FontId, text: &str, style: &TextStyle) -> f64 {
    let adv = db.digit_advance(font, style.size, style.tracking);
    text.chars()
        .map(|ch| {
            if ch.is_ascii_digit() {
                adv
            } else {
                db.measure(font, &ch.to_string(), style.size, style.tracking)
            }
        })
        .sum()
}

/// A per-glyph transform, for kinetic text.
///
/// The closure gets the glyph's index and its cluster, and returns an offset,
/// a scale about the glyph's own centre, and an opacity. That is enough to
/// express every staggered entrance in the crate without the drawing code
/// knowing anything about animation.
pub struct GlyphTransform<'a> {
    pub f: &'a dyn Fn(usize, u32) -> (f64, f64, f64, f64),
}

/// Draw `layout` with its top-left at `origin`.
pub fn draw(
    canvas: &mut Canvas,
    db: &FontDb,
    layout: &TextLayout,
    style: &TextStyle,
    origin: (f64, f64),
    opacity: f64,
    per_glyph: Option<&GlyphTransform<'_>>,
) {
    if opacity <= 0.0 {
        return;
    }
    let block = Rect::new(origin.0, origin.1, layout.width.max(1.0), layout.height.max(1.0));

    if let Some(sh) = &style.shadow {
        draw_shadow(canvas, db, layout, style, origin, opacity, per_glyph, sh);
    }

    let mut index = 0usize;
    for (li, line) in layout.lines.iter().enumerate() {
        let ox = origin.0 + layout.line_offset(li, style.align);
        let oy = origin.1 + layout.first_baseline + li as f64 * layout.line_step;
        for g in &line.glyphs {
            let (dx, dy, gscale, galpha) = match per_glyph {
                Some(t) => (t.f)(index, g.cluster),
                None => (0.0, 0.0, 1.0, 1.0),
            };
            index += 1;
            let a = opacity * galpha;
            if a <= 0.001 || gscale <= 0.0 {
                continue;
            }
            let Some(path) = db.outline(g.font, g.glyph) else { continue };
            let tf = glyph_transform(db, g, layout.size, ox + dx, oy + dy, gscale);
            let Some(p) = (*path).clone().transform(tf) else { continue };
            let paint = if a >= 0.999 { style.fill.clone() } else { style.fill.opacity(a) };
            canvas.fill_path_with(&p, &paint, block);
            if let Some((sp, sw)) = &style.stroke {
                let sp = if a >= 0.999 { sp.clone() } else { sp.opacity(a) };
                canvas.stroke_path_with(&p, &sp, *sw, block);
            }
        }
    }
}

/// Font units -> screen pixels, with the y-flip and an optional scale about
/// the glyph's own origin.
fn glyph_transform(
    db: &FontDb,
    g: &PositionedGlyph,
    size: f64,
    ox: f64,
    oy: f64,
    gscale: f64,
) -> Transform {
    let upem = db.units_per_em(g.font);
    let s = (size / upem * gscale) as f32;
    // Scaling about the glyph's pen position keeps a staggered zoom from
    // sliding sideways as it grows.
    Transform::from_row(s, 0.0, 0.0, -s, (ox + g.x) as f32, (oy + g.y) as f32)
}

/// Render the text once into a scratch pixmap, blur its alpha, tint it, and
/// composite it under the real text.
fn draw_shadow(
    canvas: &mut Canvas,
    db: &FontDb,
    layout: &TextLayout,
    style: &TextStyle,
    origin: (f64, f64),
    opacity: f64,
    per_glyph: Option<&GlyphTransform<'_>>,
    sh: &Shadow,
) {
    let pad = (sh.blur * 3.0).ceil() + sh.dx.abs() + sh.dy.abs() + style.size * 0.5;
    let w = (layout.width + pad * 2.0).ceil() as u32;
    let h = (layout.height + pad * 2.0).ceil() as u32;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return;
    }
    let Some(mut scratch) = tiny_skia::Pixmap::new(w, h) else { return };
    {
        let mut tmp = Canvas::from_pixmap(std::mem::replace(
            &mut scratch,
            tiny_skia::Pixmap::new(1, 1).unwrap(),
        ));
        let silhouette = TextStyle {
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            shadow: None,
            ..style.clone()
        };
        draw(&mut tmp, db, layout, &silhouette, (pad, pad), 1.0, per_glyph);
        scratch = tmp.pixmap;
    }
    crate::canvas::blur_alpha(&mut scratch, sh.blur);

    let dst = Rect::new(origin.0 - pad + sh.dx, origin.1 - pad + sh.dy, w as f64, h as f64);
    // The blur leaves a black silhouette; tint it by drawing it as a mask.
    let mut mask = tiny_skia::Mask::new(w, h).unwrap();
    {
        let m = mask.data_mut();
        for (i, px) in scratch.pixels().iter().enumerate() {
            m[i] = px.alpha();
        }
    }
    let mut tint = tiny_skia::Pixmap::new(w, h).unwrap();
    tint.fill(sh.colour.to_skia());
    tint.apply_mask(&mask);
    canvas.draw_pixmap_rect(tint.as_ref(), dst, opacity, tiny_skia::BlendMode::SourceOver);
}

/// A rounded plate behind text — the lower-third's backing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plate {
    pub fill: Paint,
    pub radius: f64,
    /// Padding around the text: (horizontal, vertical).
    pub pad: (f64, f64),
    pub shadow: Option<Shadow>,
    /// A coloured bar down the leading edge.
    pub accent: Option<(Color, f64)>,
}

impl Plate {
    pub fn dark() -> Self {
        Plate {
            fill: Paint::Solid(Color::rgba(12, 14, 18, 214)),
            radius: 10.0,
            pad: (28.0, 18.0),
            shadow: Some(Shadow { colour: Color::rgba(0, 0, 0, 140), blur: 26.0, dx: 0.0, dy: 8.0 }),
            accent: None,
        }
    }

    pub fn accent(mut self, c: Color, w: f64) -> Self {
        self.accent = Some((c, w));
        self
    }

    pub fn draw(&self, canvas: &mut Canvas, r: Rect, opacity: f64) {
        if opacity <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        if let Some(sh) = &self.shadow {
            let sr = Rect::new(r.x + sh.dx, r.y + sh.dy, r.w, r.h);
            // Approximate the plate's shadow with a blurred rounded rect.
            let pad = (sh.blur * 3.0).ceil();
            let (w, h) = ((sr.w + pad * 2.0) as u32, (sr.h + pad * 2.0) as u32);
            if w < 8192 && h < 8192 && w > 0 && h > 0
                && let Some(mut sp) = tiny_skia::Pixmap::new(w, h)
            {
                if let Some(p) = crate::canvas::round_rect_path(
                    Rect::new(pad, pad, sr.w, sr.h),
                    self.radius,
                ) {
                    let mut sk = tiny_skia::Paint { anti_alias: true, ..Default::default() };
                    sk.shader = tiny_skia::Shader::SolidColor(sh.colour.to_skia());
                    sp.fill_path(&p, &sk, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }
                crate::canvas::blur_alpha(&mut sp, sh.blur);
                let mut mask = tiny_skia::Mask::new(w, h).unwrap();
                {
                    let m = mask.data_mut();
                    for (i, px) in sp.pixels().iter().enumerate() {
                        m[i] = px.alpha();
                    }
                }
                let mut tint = tiny_skia::Pixmap::new(w, h).unwrap();
                tint.fill(sh.colour.to_skia());
                tint.apply_mask(&mask);
                canvas.draw_pixmap_rect(
                    tint.as_ref(),
                    Rect::new(sr.x - pad, sr.y - pad, w as f64, h as f64),
                    opacity,
                    tiny_skia::BlendMode::SourceOver,
                );
            }
        }
        canvas.fill_round_rect(r, self.radius, &self.fill.opacity(opacity));
        if let Some((c, w)) = self.accent {
            let bar = Rect::new(r.x, r.y, w, r.h);
            // Only the leading edge is rounded, so the bar reads as part of
            // the plate rather than a floating pill.
            let mut pb = PathBuilder::new();
            let rr = self.radius.min(w);
            pb.move_to((bar.x + rr) as f32, bar.y as f32);
            pb.line_to(bar.right() as f32, bar.y as f32);
            pb.line_to(bar.right() as f32, bar.bottom() as f32);
            pb.line_to((bar.x + rr) as f32, bar.bottom() as f32);
            pb.cubic_to(
                (bar.x + rr * 0.45) as f32, bar.bottom() as f32,
                bar.x as f32, (bar.bottom() - rr * 0.45) as f32,
                bar.x as f32, (bar.bottom() - rr) as f32,
            );
            pb.line_to(bar.x as f32, (bar.y + rr) as f32);
            pb.cubic_to(
                bar.x as f32, (bar.y + rr * 0.45) as f32,
                (bar.x + rr * 0.45) as f32, bar.y as f32,
                (bar.x + rr) as f32, bar.y as f32,
            );
            pb.close();
            if let Some(p) = pb.finish() {
                canvas.fill_path_with(&p, &Paint::Solid(c.opacity(opacity)), bar);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> &'static FontDb {
        FontDb::shared()
    }

    fn style() -> TextStyle {
        TextStyle::default().size(60.0)
    }

    #[test]
    fn lays_out_a_single_line() {
        let l = TextLayout::build(db(), "Kanto", &style(), None).unwrap();
        assert_eq!(l.lines.len(), 1);
        assert!(l.width > 60.0, "width {}", l.width);
        assert!(l.height > 40.0 && l.height < 120.0, "height {}", l.height);
    }

    #[test]
    fn wraps_to_the_given_width() {
        let text = "Two hundred and twenty six maps stitched into one picture";
        let wide = TextLayout::build(db(), text, &style(), Some(4000.0)).unwrap();
        let narrow = TextLayout::build(db(), text, &style(), Some(500.0)).unwrap();
        assert_eq!(wide.lines.len(), 1);
        assert!(narrow.lines.len() > 2, "got {} lines", narrow.lines.len());
        assert!(narrow.width <= 500.0 + 1.0, "width {}", narrow.width);
    }

    #[test]
    fn explicit_newlines_are_honoured() {
        let l = TextLayout::build(db(), "one\ntwo\nthree", &style(), None).unwrap();
        assert_eq!(l.lines.len(), 3);
    }

    #[test]
    fn fit_shrinks_until_it_fits() {
        let box_ = Rect::new(0.0, 0.0, 400.0, 120.0);
        let big = TextStyle::default().size(200.0);
        let l = TextLayout::fit(db(), "A rather long caption indeed", &big, box_).unwrap();
        assert!(l.width <= box_.w + 1.0, "width {} > {}", l.width, box_.w);
        assert!(l.height <= box_.h + 1.0, "height {} > {}", l.height, box_.h);
        assert!(l.size < 200.0, "must have shrunk, got {}", l.size);
    }

    #[test]
    fn fit_leaves_text_that_already_fits_alone() {
        let box_ = Rect::new(0.0, 0.0, 4000.0, 4000.0);
        let s = TextStyle::default().size(40.0);
        let l = TextLayout::fit(db(), "ok", &s, box_).unwrap();
        assert_eq!(l.size, 40.0);
    }

    #[test]
    fn tabular_digits_all_take_the_same_width() {
        let s = style().tabular();
        // Every three-digit number must lay out to exactly the same width, or
        // a counter jitters as it ticks.
        let w111 = TextLayout::build(db(), "111", &s, None).unwrap().width;
        let w888 = TextLayout::build(db(), "888", &s, None).unwrap().width;
        assert!((w111 - w888).abs() < 1e-6, "{w111} vs {w888}");
        // And a proportional style genuinely differs, so the test is not
        // passing by accident.
        let p = style();
        let p111 = TextLayout::build(db(), "111", &p, None).unwrap().width;
        let p888 = TextLayout::build(db(), "888", &p, None).unwrap().width;
        assert!((p111 - p888).abs() > 1e-6, "proportional should differ");
    }

    #[test]
    fn upper_case_is_applied_before_shaping() {
        let s = style().upper();
        let l = TextLayout::build(db(), "kanto", &s, None).unwrap();
        assert_eq!(l.lines[0].text, "KANTO");
    }

    #[test]
    fn drawing_puts_ink_on_the_canvas() {
        let mut cv = Canvas::filled(400, 200, Color::BLACK).unwrap();
        let s = style().size(80.0).colour(Color::WHITE).no_shadow();
        let l = TextLayout::build(db(), "HI", &s, None).unwrap();
        draw(&mut cv, db(), &l, &s, (20.0, 40.0), 1.0, None);
        let lit = cv.as_ref().pixels().iter().filter(|p| p.red() > 128).count();
        assert!(lit > 200, "expected white glyph pixels, got {lit}");
    }

    #[test]
    fn shadow_spreads_beyond_the_glyphs() {
        let plain = {
            let mut cv = Canvas::new(400, 200).unwrap();
            let s = style().size(80.0).no_shadow();
            let l = TextLayout::build(db(), "HI", &s, None).unwrap();
            draw(&mut cv, db(), &l, &s, (100.0, 40.0), 1.0, None);
            cv.as_ref().pixels().iter().filter(|p| p.alpha() > 0).count()
        };
        let shadowed = {
            let mut cv = Canvas::new(400, 200).unwrap();
            let s = style().size(80.0).shadow(Shadow::soft());
            let l = TextLayout::build(db(), "HI", &s, None).unwrap();
            draw(&mut cv, db(), &l, &s, (100.0, 40.0), 1.0, None);
            cv.as_ref().pixels().iter().filter(|p| p.alpha() > 0).count()
        };
        assert!(shadowed > plain * 2, "shadow {shadowed} vs plain {plain}");
    }

    #[test]
    fn per_glyph_opacity_can_hide_everything() {
        let mut cv = Canvas::new(400, 200).unwrap();
        let s = style().size(80.0).no_shadow();
        let l = TextLayout::build(db(), "HIDDEN", &s, None).unwrap();
        let f = |_i: usize, _c: u32| (0.0, 0.0, 1.0, 0.0);
        draw(&mut cv, db(), &l, &s, (20.0, 40.0), 1.0, Some(&GlyphTransform { f: &f }));
        assert_eq!(cv.as_ref().pixels().iter().filter(|p| p.alpha() > 0).count(), 0);
    }
}
