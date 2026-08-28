//! Charts: data that arrives over time.
//!
//! A [`crate::layer::Content::Chart`] is the general form of the treatment the
//! captain pointed at — a plotted function drawn progressively across the shot,
//! on axes that look deliberate. It is **data**, the same as every other layer:
//! a serde tree with series, axes and a reveal, no closures and no callbacks,
//! so a chart can be written in Rust, loaded from JSON, or emitted by a tool
//! over MCP without any of them being a special case. Nothing here knows what
//! the numbers *are* — a growth curve, a poll, a benchmark — the same rule that
//! shapes the rest of the crate.
//!
//! Three series kinds, one animation idea:
//!
//! - [`Series::Line`] plots an explicit list of `(x, y)` points.
//! - [`Series::Function`] plots an expression (`"sin(x)"`, `"40*log(x+1)"`)
//!   sampled over the x-axis — see [`crate::expr`].
//! - [`Series::Bars`] grows bars to their values, one slot per category, and
//!   several bar series cluster into groups.
//!
//! The one animation is a **[`Reveal`] sweep**: a single eased 0..1 that crosses
//! the plot left to right on the film's own clock, exactly the way
//! [`crate::layer::BarSpec`] and [`crate::layer::CounterSpec`] ease a value over
//! `over` seconds. A line is drawn only up to the swept x (its last segment
//! interpolated, so the tip advances smoothly, not a point at a time); bars pop
//! in as the sweep passes their slot. "Data arriving over time" is therefore
//! one concept the whole chart shares, not a per-series animation each kind
//! reinvents.
//!
//! A chart composes with the rest of the crate without knowing it does: the
//! colour grade is a post-composite pass that lands on the finished pixels
//! (`Renderer::draw_scene`), a [`crate::layer::Content::Callout`] or a title is
//! just another layer with a higher `z`, and a [`crate::layer::Content::PullUp`]
//! lifts and enlarges a region of *any* layer — the crate's own "push in on a
//! chart" — because it reads the canvas beneath it rather than the chart's data.

use crate::color::{Color, Paint};
use crate::ease::Easing;
use crate::expr::Expr;
use crate::geom::Rect;
use crate::layer::RenderCtx;
use crate::text::{TextLayout, TextStyle};
use crate::time::Time;
use serde::{Deserialize, Serialize};
use tiny_skia::PathBuilder;

/// A chart: its series, its axes, and how it draws in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSpec {
    /// Drawn back to front. A chart with any [`Series::Bars`] is a bar chart —
    /// its x-axis is the bars' categories — and non-bar series in it are
    /// ignored (mixing the two coordinate systems is an authoring mistake, not
    /// a supported combo; see [`ChartSpec::is_categorical`]).
    pub series: Vec<Series>,
    #[serde(default, skip_serializing_if = "Axis::is_empty")]
    pub x: Axis,
    #[serde(default, skip_serializing_if = "Axis::is_empty")]
    pub y: Axis,
    /// How the chart draws in. Omitted in JSON means "already drawn" (static);
    /// the Rust [`crate::layer::Layer::chart`] builder animates by default.
    #[serde(default, skip_serializing_if = "Reveal::is_static")]
    pub reveal: Reveal,
    /// Reference lines at a data value — the highlighted marker the reference
    /// film draws at its answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// Faint gridlines behind the data.
    #[serde(default = "yes")]
    pub grid: bool,
    /// The axis, frame and gridline colour. Defaults to a muted tint of the
    /// theme's caption colour, so it reads on either a light or a dark film.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_colour: Option<Color>,
    /// The style for tick and axis labels. Defaults to the theme's caption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_style: Option<TextStyle>,
}

fn yes() -> bool {
    true
}

/// One axis's range and ticks. Every field is optional: an unset range is read
/// from the data (a [`Series::Function`] is the exception — it has no intrinsic
/// domain, so its chart's x range must be given).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Axis {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Roughly how many ticks to aim for; the actual count is rounded to nice
    /// numbers. Default 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks: Option<usize>,
    /// A word beside the axis — "age", "£m", "requests/s".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Axis {
    fn is_empty(&self) -> bool {
        *self == Axis::default()
    }

    /// The range `[a, b]`, given the data bounds to fall back on.
    fn range(&self, data: (f64, f64)) -> (f64, f64) {
        (self.min.unwrap_or(data.0), self.max.unwrap_or(data.1))
    }
}

/// How the chart draws in: an eased sweep across the plot on the film's clock.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reveal {
    /// How long the sweep takes. `0` (the JSON default) is a static chart,
    /// fully drawn from its first frame.
    pub over: Time,
    #[serde(default)]
    pub easing: Easing,
}

impl Default for Reveal {
    fn default() -> Self {
        Reveal { over: Time::ZERO, easing: Easing::default() }
    }
}

impl Reveal {
    /// An animated reveal, the common case the builder reaches for.
    pub fn over(secs: impl Into<Time>) -> Self {
        Reveal { over: secs.into(), easing: Easing::OutCubic }
    }

    fn is_static(&self) -> bool {
        self.over.as_secs() <= 0.0
    }

    /// The eased sweep 0..1 at `local` seconds into the layer. A static reveal
    /// is fully swept from the start.
    pub fn sweep_at(&self, local: f64) -> f64 {
        let d = self.over.as_secs();
        if d <= 0.0 {
            return 1.0;
        }
        self.easing.apply((local / d).clamp(0.0, 1.0))
    }
}

/// A reference line at a fixed data value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Marker {
    /// A vertical line at `x`.
    VLine {
        x: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        colour: Option<Color>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default = "marker_width")]
        width: f64,
    },
    /// A horizontal line at `y`.
    HLine {
        y: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        colour: Option<Color>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default = "marker_width")]
        width: f64,
    },
}

fn marker_width() -> f64 {
    3.0
}

/// One plotted series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Series {
    /// An explicit list of `[x, y]` points, joined in order.
    Line {
        points: Vec<[f64; 2]>,
        #[serde(default, flatten)]
        style: LineStyle,
    },
    /// A function of `x`, sampled across the x-axis range. `expr` is parsed by
    /// [`crate::expr`]; a sample that is not finite breaks the line rather than
    /// spiking it to an edge.
    Function {
        expr: String,
        #[serde(default = "default_samples")]
        samples: usize,
        #[serde(default, flatten)]
        style: LineStyle,
    },
    /// Bars, one per category. `labels`, if given, name the categories on the
    /// x-axis. Several `Bars` series in one chart cluster into groups.
    Bars {
        values: Vec<f64>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        labels: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        paint: Option<Paint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

fn default_samples() -> usize {
    240
}

/// The look of a line or curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineStyle {
    /// A solid colour or a gradient. Defaults to the series' palette colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint: Option<Paint>,
    #[serde(default = "default_line_width")]
    pub width: f64,
    /// Fill the area under the curve, at a low opacity.
    #[serde(default)]
    pub fill: bool,
    /// Draw a dot at each explicit point (for [`Series::Line`]).
    #[serde(default)]
    pub dots: bool,
}

impl Default for LineStyle {
    fn default() -> Self {
        LineStyle { paint: None, width: default_line_width(), fill: false, dots: false }
    }
}

fn default_line_width() -> f64 {
    5.0
}

impl Series {
    fn is_bars(&self) -> bool {
        matches!(self, Series::Bars { .. })
    }
}

impl ChartSpec {
    /// A chart is a bar chart the moment any series is [`Series::Bars`].
    pub fn is_categorical(&self) -> bool {
        self.series.iter().any(Series::is_bars)
    }

    /// Problems that would make the chart wrong or empty, for
    /// [`crate::timeline::Film::validate`]. Kept cheap — this parses each
    /// function once, which is work the draw path would do anyway.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.series.is_empty() {
            out.push("a chart has no series".into());
        }
        let categorical = self.is_categorical();
        for (i, s) in self.series.iter().enumerate() {
            match s {
                Series::Function { expr, samples, .. } => {
                    if let Err(e) = Expr::parse(expr) {
                        out.push(format!("series {i}: cannot parse {expr:?}: {e}"));
                    }
                    if *samples < 2 {
                        out.push(format!("series {i}: needs at least 2 samples"));
                    }
                    if self.x.min.is_none() || self.x.max.is_none() {
                        out.push(format!(
                            "series {i} is a function but the chart's x range is unset — a function has no domain of its own, so give x.min and x.max"
                        ));
                    }
                }
                Series::Line { points, .. } => {
                    if points.len() < 2 {
                        out.push(format!("series {i}: a line needs at least 2 points"));
                    }
                }
                Series::Bars { values, .. } => {
                    if values.is_empty() {
                        out.push(format!("series {i}: a bar series has no values"));
                    }
                }
            }
            if categorical && !s.is_bars() {
                out.push(format!(
                    "series {i} is not bars, but the chart has bars — a chart is either xy or categorical, not both; move it to its own chart"
                ));
            }
        }
        out
    }
}

// -------------------------------------------------------------------------
// Geometry
// -------------------------------------------------------------------------

/// Maps data coordinates to pixels within the plot rect.
struct Plot {
    rect: Rect,
    xr: (f64, f64),
    yr: (f64, f64),
}

impl Plot {
    fn px(&self, x: f64) -> f64 {
        let (a, b) = self.xr;
        let t = if (b - a).abs() < f64::EPSILON { 0.0 } else { (x - a) / (b - a) };
        self.rect.x + t * self.rect.w
    }

    fn py(&self, y: f64) -> f64 {
        let (a, b) = self.yr;
        let t = if (b - a).abs() < f64::EPSILON { 0.0 } else { (y - a) / (b - a) };
        // y grows upward on screen.
        self.rect.bottom() - t * self.rect.h
    }
}

/// "Nice" tick values across `[lo, hi]`, aiming for about `target` of them,
/// snapped to 1/2/5 × 10ⁿ so the labels read as round numbers rather than
/// wherever the data happened to fall.
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    let target = target.max(2);
    if !lo.is_finite() || !hi.is_finite() || (hi - lo).abs() < f64::EPSILON {
        return vec![lo];
    }
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let raw = (hi - lo) / target as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = if norm < 1.5 {
        1.0
    } else if norm < 3.0 {
        2.0
    } else if norm < 7.0 {
        5.0
    } else {
        10.0
    } * mag;
    let first = (lo / step).ceil() * step;
    // Round each tick to the step's own decimal precision, so a step of 0.2
    // yields 0.6 rather than 0.6000000000000001 — the noise would otherwise
    // reach the label and read as a bug.
    let decimals = (-step.log10().floor()).max(0.0) as usize + 1;
    let factor = 10f64.powi(decimals as i32);
    let round = |v: f64| (v * factor).round() / factor;
    let mut ticks = Vec::new();
    let mut v = first;
    // A tiny epsilon so a tick landing exactly on `hi` (floating error and all)
    // is not dropped.
    let eps = step * 1e-6;
    while v <= hi + eps {
        ticks.push(round(v));
        v += step;
    }
    ticks
}

/// Format a tick value: trim trailing zeros, keep it short.
pub fn format_tick(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    // Choose decimals by magnitude, then strip trailing zeros.
    let decimals = if a >= 100.0 {
        0
    } else if a >= 1.0 {
        1
    } else if a >= 0.01 {
        3
    } else {
        4
    };
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

// -------------------------------------------------------------------------
// Drawing
// -------------------------------------------------------------------------

/// A five-colour default palette, used when a series names no paint. The first
/// is the theme accent (passed in); the rest are a cool-to-warm spread that
/// stays legible on both light and dark grounds.
fn palette(accent: Color, i: usize) -> Color {
    const REST: [Color; 4] = [
        Color::rgb(56, 210, 220),  // cyan
        Color::rgb(226, 96, 173),  // magenta
        Color::rgb(120, 200, 120), // green
        Color::rgb(240, 158, 74),  // orange
    ];
    if i == 0 {
        accent
    } else {
        REST[(i - 1) % REST.len()]
    }
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Draw a chart into `box_` at `local` seconds into its layer, at `alpha`.
///
/// Pure with respect to the film description — the same arguments draw the same
/// pixels — so it slots into the deterministic render the same way every other
/// content kind does.
pub fn draw(
    canvas: &mut crate::canvas::Canvas,
    ctx: &RenderCtx<'_>,
    spec: &ChartSpec,
    box_: Rect,
    local: Time,
    alpha: f64,
) {
    if box_.w <= 1.0 || box_.h <= 1.0 || alpha <= 0.0 {
        return;
    }
    let label_style = spec.label_style.clone().unwrap_or_else(|| ctx.theme.caption.clone());
    let ink = spec
        .axis_colour
        .unwrap_or_else(|| label_style.fill.dominant());
    let sweep = spec.reveal.sweep_at(local.as_secs());
    let accent = ctx.theme.accent;

    if spec.is_categorical() {
        draw_categorical(canvas, ctx, spec, box_, &label_style, ink, accent, sweep, alpha);
    } else {
        draw_xy(canvas, ctx, spec, box_, &label_style, ink, accent, sweep, alpha);
    }
}

/// Everything shared by both chart modes: reserve room for the labels, draw the
/// frame, gridlines, ticks and labels, and hand back the inner plot rect.
#[allow(clippy::too_many_arguments)]
fn frame_and_axes(
    canvas: &mut crate::canvas::Canvas,
    ctx: &RenderCtx<'_>,
    box_: Rect,
    label_style: &TextStyle,
    ink: Color,
    xr: (f64, f64),
    yr: (f64, f64),
    x_ticks: &[(f64, String)],
    y_ticks: &[f64],
    x_label: Option<&str>,
    y_label: Option<&str>,
    grid: bool,
    alpha: f64,
) -> Plot {
    let lh = label_style.size * 1.15;
    let gap = label_style.size * 0.45;

    // Left margin: the widest y-tick label plus a gap. Measuring, rather than
    // guessing a fraction, is what keeps labels from colliding with the plot.
    let mut y_label_w: f64 = 0.0;
    for &t in y_ticks {
        if let Some(l) = TextLayout::build(ctx.fonts, &format_tick(t), label_style, None) {
            y_label_w = y_label_w.max(l.width);
        }
    }
    let axis_title = lh; // one line reserved for an axis title, if present
    let ml = y_label_w + gap * 1.5 + if y_label.is_some() { axis_title } else { 0.0 };
    let mb = lh + gap + if x_label.is_some() { axis_title } else { 0.0 };
    let mt = lh * 0.6;
    let mr = lh * 0.8;

    let plot = Rect::new(
        box_.x + ml,
        box_.y + mt,
        (box_.w - ml - mr).max(1.0),
        (box_.h - mt - mb).max(1.0),
    );
    let p = Plot { rect: plot, xr, yr };

    let grid_paint = Paint::Solid(ink.opacity(alpha * 0.14));
    let axis_paint = Paint::Solid(ink.opacity(alpha * 0.55));
    let tick_len = lh * 0.28;

    // Gridlines first, behind everything.
    if grid {
        for &t in y_ticks {
            let y = p.py(t);
            canvas.line(plot.x, y, plot.right(), y, 1.5, &grid_paint);
        }
        for (tx, _) in x_ticks {
            let x = p.px(*tx);
            canvas.line(x, plot.y, x, plot.bottom(), 1.5, &grid_paint);
        }
    }

    // The plot frame: a full, subtle rectangle, with the two data axes drawn
    // stronger on top so the L reads as the axes and the rest as a frame.
    canvas.stroke_round_rect(plot, 0.0, 1.5, &Paint::Solid(ink.opacity(alpha * 0.28)));
    canvas.line(plot.x, plot.y, plot.x, plot.bottom(), 2.0, &axis_paint);
    canvas.line(plot.x, plot.bottom(), plot.right(), plot.bottom(), 2.0, &axis_paint);

    // y tick marks and labels, right-aligned into the left margin.
    for &t in y_ticks {
        let y = p.py(t);
        canvas.line(plot.x - tick_len, y, plot.x, y, 2.0, &axis_paint);
        if let Some(l) = TextLayout::build(ctx.fonts, &format_tick(t), label_style, None) {
            let ox = plot.x - tick_len - gap - l.width;
            let oy = y - l.height / 2.0;
            crate::text::draw(canvas, ctx.fonts, &l, label_style, (ox, oy), alpha, None);
        }
    }

    // x tick marks and labels, centred under each tick and clamped so the end
    // labels never run off the frame.
    for (tx, text) in x_ticks {
        let x = p.px(*tx);
        canvas.line(x, plot.bottom(), x, plot.bottom() + tick_len, 2.0, &axis_paint);
        if let Some(l) = TextLayout::build(ctx.fonts, text, label_style, None) {
            let ox = (x - l.width / 2.0).clamp(box_.x, box_.right() - l.width);
            let oy = plot.bottom() + tick_len + gap * 0.6;
            crate::text::draw(canvas, ctx.fonts, &l, label_style, (ox, oy), alpha, None);
        }
    }

    // Axis titles.
    if let Some(text) = x_label
        && let Some(l) = TextLayout::build(ctx.fonts, text, label_style, None)
    {
        let ox = plot.x + (plot.w - l.width) / 2.0;
        let oy = box_.bottom() - l.height;
        crate::text::draw(canvas, ctx.fonts, &l, label_style, (ox, oy), alpha, None);
    }
    // Drawn horizontally at the top-left of the axis rather than rotated: the
    // crate's text path has no glyph rotation, and a short unit label reads fine
    // sitting above the axis.
    if let Some(text) = y_label
        && let Some(l) = TextLayout::build(ctx.fonts, text, label_style, None)
    {
        crate::text::draw(canvas, ctx.fonts, &l, label_style, (box_.x, box_.y), alpha, None);
    }

    p
}

#[allow(clippy::too_many_arguments)]
fn draw_xy(
    canvas: &mut crate::canvas::Canvas,
    ctx: &RenderCtx<'_>,
    spec: &ChartSpec,
    box_: Rect,
    label_style: &TextStyle,
    ink: Color,
    accent: Color,
    sweep: f64,
    alpha: f64,
) {
    // Sample every series once, so ranges and drawing share one set of points.
    let sampled: Vec<(Vec<[f64; 2]>, &LineStyle)> = spec
        .series
        .iter()
        .filter_map(|s| match s {
            Series::Line { points, style } => Some((points.clone(), style)),
            Series::Function { expr, samples, style } => {
                let e = Expr::parse(expr).ok()?;
                let (a, b) = spec.x.range((0.0, 10.0));
                let n = (*samples).max(2);
                let pts = (0..n)
                    .map(|i| {
                        let x = a + (b - a) * i as f64 / (n - 1) as f64;
                        [x, e.eval(x)]
                    })
                    .collect();
                Some((pts, style))
            }
            Series::Bars { .. } => None,
        })
        .collect();

    // Data bounds, for any axis range left unset.
    let (mut dxlo, mut dxhi) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut dylo, mut dyhi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (pts, _) in &sampled {
        for p in pts {
            if p[0].is_finite() {
                dxlo = dxlo.min(p[0]);
                dxhi = dxhi.max(p[0]);
            }
            if p[1].is_finite() {
                dylo = dylo.min(p[1]);
                dyhi = dyhi.max(p[1]);
            }
        }
    }
    if !dxlo.is_finite() {
        (dxlo, dxhi) = (0.0, 1.0);
    }
    if !dylo.is_finite() {
        (dylo, dyhi) = (0.0, 1.0);
    }
    // A little headroom above and below so the curve does not touch the frame.
    let ypad = ((dyhi - dylo) * 0.08).max(f64::EPSILON);
    let xr = spec.x.range((dxlo, dxhi));
    let yr = spec.y.range((dylo - ypad, dyhi + ypad));

    let x_ticks: Vec<(f64, String)> = nice_ticks(xr.0, xr.1, spec.x.ticks.unwrap_or(6))
        .into_iter()
        .filter(|t| *t >= xr.0 - 1e-9 && *t <= xr.1 + 1e-9)
        .map(|t| (t, format_tick(t)))
        .collect();
    let y_ticks: Vec<f64> = nice_ticks(yr.0, yr.1, spec.y.ticks.unwrap_or(5))
        .into_iter()
        .filter(|t| *t >= yr.0 - 1e-9 && *t <= yr.1 + 1e-9)
        .collect();

    let p = frame_and_axes(
        canvas,
        ctx,
        box_,
        label_style,
        ink,
        xr,
        yr,
        &x_ticks,
        &y_ticks,
        spec.x.label.as_deref(),
        spec.y.label.as_deref(),
        spec.grid,
        alpha,
    );

    let xcut = xr.0 + (xr.1 - xr.0) * sweep;

    for (i, (pts, style)) in sampled.iter().enumerate() {
        let colour = style.paint.clone().unwrap_or_else(|| Paint::Solid(palette(accent, i)));
        draw_line_series(canvas, &p, pts, style, &colour, xcut, alpha);
    }

    draw_markers(canvas, ctx, spec, &p, label_style, ink, accent, alpha);
}

/// Draw one line/curve up to `xcut`, breaking on non-finite y, optionally
/// filling under it and dotting its points.
fn draw_line_series(
    canvas: &mut crate::canvas::Canvas,
    p: &Plot,
    pts: &[[f64; 2]],
    style: &LineStyle,
    paint: &Paint,
    xcut: f64,
    alpha: f64,
) {
    // Split into runs of consecutive finite points that fall at or before the
    // sweep, interpolating one extra point exactly at `xcut` so the drawn tip
    // advances smoothly rather than a whole segment at a time.
    let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let finite = |q: &[f64; 2]| q[0].is_finite() && q[1].is_finite();

    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if finite(&a) && a[0] <= xcut {
            if cur.is_empty() {
                cur.push((p.px(a[0]), p.py(a[1])));
            }
            if finite(&b) && b[0] <= xcut {
                cur.push((p.px(b[0]), p.py(b[1])));
            } else if finite(&b) && a[0] < xcut && b[0] > xcut && (b[0] - a[0]).abs() > f64::EPSILON {
                // Interpolate the crossing point at exactly xcut.
                let t = (xcut - a[0]) / (b[0] - a[0]);
                let y = a[1] + (b[1] - a[1]) * t;
                cur.push((p.px(xcut), p.py(y)));
                runs.push(std::mem::take(&mut cur));
            } else {
                // b is non-finite: end this run here.
                runs.push(std::mem::take(&mut cur));
            }
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }

    for run in &runs {
        if run.len() < 2 {
            continue;
        }
        if style.fill {
            // Close down to the baseline (y range floor, clamped into the plot)
            // and back, filled at a low opacity.
            let base = p.rect.bottom();
            let mut pb = PathBuilder::new();
            pb.move_to(run[0].0 as f32, base as f32);
            for &(x, y) in run {
                pb.line_to(x as f32, y as f32);
            }
            pb.line_to(run[run.len() - 1].0 as f32, base as f32);
            pb.close();
            if let Some(path) = pb.finish() {
                canvas.fill_path_with(&path, &paint.opacity(alpha * 0.16), p.rect);
            }
        }
        let mut pb = PathBuilder::new();
        pb.move_to(run[0].0 as f32, run[0].1 as f32);
        for &(x, y) in &run[1..] {
            pb.line_to(x as f32, y as f32);
        }
        if let Some(path) = pb.finish() {
            canvas.stroke_path_with(&path, &paint.opacity(alpha), style.width, p.rect);
        }
    }

    if style.dots {
        let r = style.width * 1.4;
        for q in pts {
            if finite(q) && q[0] <= xcut {
                let (cx, cy) = (p.px(q[0]), p.py(q[1]));
                canvas.fill_round_rect(
                    Rect::new(cx - r, cy - r, r * 2.0, r * 2.0),
                    r,
                    &paint.opacity(alpha),
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_categorical(
    canvas: &mut crate::canvas::Canvas,
    ctx: &RenderCtx<'_>,
    spec: &ChartSpec,
    box_: Rect,
    label_style: &TextStyle,
    ink: Color,
    accent: Color,
    sweep: f64,
    alpha: f64,
) {
    let bar_series: Vec<(&Vec<f64>, &Vec<String>, Option<Paint>)> = spec
        .series
        .iter()
        .filter_map(|s| match s {
            Series::Bars { values, labels, paint, .. } => Some((values, labels, paint.clone())),
            _ => None,
        })
        .collect();
    if bar_series.is_empty() {
        return;
    }

    let groups = bar_series.iter().map(|(v, _, _)| v.len()).max().unwrap_or(0);
    if groups == 0 {
        return;
    }
    let max_val = bar_series
        .iter()
        .flat_map(|(v, _, _)| v.iter().copied())
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);

    // Bars grow from a zero baseline; give the top a little headroom.
    let yr = spec.y.range((0.0, max_val * 1.12));
    let y_ticks: Vec<f64> = nice_ticks(yr.0, yr.1, spec.y.ticks.unwrap_or(5))
        .into_iter()
        .filter(|t| *t >= yr.0 - 1e-9 && *t <= yr.1 + 1e-9)
        .collect();

    // Category labels come from the first series that names them.
    let labels = bar_series
        .iter()
        .map(|(_, l, _)| *l)
        .find(|l| !l.is_empty());
    let x_ticks: Vec<(f64, String)> = (0..groups)
        .map(|g| {
            let text = labels
                .and_then(|l| l.get(g))
                .cloned()
                .unwrap_or_else(|| (g + 1).to_string());
            (g as f64 + 0.5, text)
        })
        .collect();

    // x runs 0..groups so a tick at g+0.5 sits under the middle of group g.
    let p = frame_and_axes(
        canvas,
        ctx,
        box_,
        label_style,
        ink,
        (0.0, groups as f64),
        yr,
        &x_ticks,
        &y_ticks,
        spec.x.label.as_deref(),
        spec.y.label.as_deref(),
        spec.grid,
        alpha,
    );

    let n_series = bar_series.len() as f64;
    let slot = p.rect.w / groups as f64;
    let group_pad = slot * 0.18; // gap between groups
    let inner = slot - group_pad;
    let bar_w = inner / n_series;
    let base_y = p.rect.bottom();

    for (si, (values, _, paint)) in bar_series.iter().enumerate() {
        let colour = paint.clone().unwrap_or_else(|| Paint::Solid(palette(accent, si)));
        for (g, &val) in values.iter().enumerate() {
            // Each group pops in as the sweep passes its slot, so the chart
            // fills left to right the same way a line does.
            let grow = smoothstep((sweep - g as f64 / groups as f64) * groups as f64);
            if grow <= 0.0 {
                continue;
            }
            let full_h = (p.py(0.0) - p.py(val)).max(0.0);
            let h = full_h * grow;
            let x = p.rect.x + g as f64 * slot + group_pad / 2.0 + si as f64 * bar_w;
            let r = (bar_w * 0.12).min(h / 2.0);
            canvas.fill_round_rect(
                Rect::new(x + bar_w * 0.06, base_y - h, bar_w * 0.88, h),
                r,
                &colour.opacity(alpha),
            );
        }
    }

    draw_markers(canvas, ctx, spec, &p, label_style, ink, accent, alpha);
}

#[allow(clippy::too_many_arguments)]
fn draw_markers(
    canvas: &mut crate::canvas::Canvas,
    ctx: &RenderCtx<'_>,
    spec: &ChartSpec,
    p: &Plot,
    label_style: &TextStyle,
    _ink: Color,
    accent: Color,
    alpha: f64,
) {
    for m in &spec.markers {
        match m {
            Marker::VLine { x, colour, label, width } => {
                let px = p.px(*x);
                if px < p.rect.x - 0.5 || px > p.rect.right() + 0.5 {
                    continue;
                }
                let c = colour.unwrap_or(accent);
                canvas.line(px, p.rect.y, px, p.rect.bottom(), *width, &Paint::Solid(c.opacity(alpha)));
                if let Some(text) = label {
                    let ms = TextStyle { fill: Paint::Solid(c), ..label_style.clone() };
                    if let Some(l) = TextLayout::build(ctx.fonts, text, &ms, None) {
                        let ox = (px + label_style.size * 0.3)
                            .min(p.rect.right() - l.width);
                        crate::text::draw(canvas, ctx.fonts, &l, &ms, (ox, p.rect.y), alpha, None);
                    }
                }
            }
            Marker::HLine { y, colour, label, width } => {
                let py = p.py(*y);
                if py < p.rect.y - 0.5 || py > p.rect.bottom() + 0.5 {
                    continue;
                }
                let c = colour.unwrap_or(accent);
                canvas.line(p.rect.x, py, p.rect.right(), py, *width, &Paint::Solid(c.opacity(alpha)));
                if let Some(text) = label {
                    let ms = TextStyle { fill: Paint::Solid(c), ..label_style.clone() };
                    if let Some(l) = TextLayout::build(ctx.fonts, text, &ms, None) {
                        crate::text::draw(
                            canvas,
                            ctx.fonts,
                            &l,
                            &ms,
                            (p.rect.x + label_style.size * 0.3, py - l.height),
                            alpha,
                            None,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_ticks_are_round_numbers() {
        let t = nice_ticks(0.0, 80.0, 5);
        assert_eq!(t, vec![0.0, 20.0, 40.0, 60.0, 80.0]);
        let t = nice_ticks(0.0, 1.0, 5);
        assert_eq!(t, vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    }

    #[test]
    fn nice_ticks_handle_a_degenerate_range() {
        assert_eq!(nice_ticks(5.0, 5.0, 5), vec![5.0]);
        assert_eq!(nice_ticks(f64::NAN, 1.0, 5).len(), 1);
    }

    #[test]
    fn ticks_format_short() {
        assert_eq!(format_tick(0.0), "0");
        assert_eq!(format_tick(40.0), "40");
        assert_eq!(format_tick(0.5), "0.5");
        assert_eq!(format_tick(1000.0), "1000");
        assert_eq!(format_tick(2.5), "2.5");
    }

    #[test]
    fn a_function_series_needs_an_x_range() {
        let spec = ChartSpec {
            series: vec![Series::Function {
                expr: "sin(x)".into(),
                samples: 100,
                style: LineStyle::default(),
            }],
            x: Axis::default(),
            y: Axis::default(),
            reveal: Reveal::default(),
            markers: vec![],
            grid: true,
            axis_colour: None,
            label_style: None,
        };
        assert!(spec.problems().iter().any(|p| p.contains("domain")));
    }

    #[test]
    fn a_bad_expression_is_a_problem_not_a_panic() {
        let spec = ChartSpec {
            series: vec![Series::Function {
                expr: "sin(".into(),
                samples: 100,
                style: LineStyle::default(),
            }],
            x: Axis { min: Some(0.0), max: Some(1.0), ..Axis::default() },
            y: Axis::default(),
            reveal: Reveal::default(),
            markers: vec![],
            grid: true,
            axis_colour: None,
            label_style: None,
        };
        assert!(spec.problems().iter().any(|p| p.contains("parse")));
    }

    #[test]
    fn mixing_bars_and_lines_is_flagged() {
        let spec = ChartSpec {
            series: vec![
                Series::Bars { values: vec![1.0], labels: vec![], paint: None, name: None },
                Series::Line { points: vec![[0.0, 0.0], [1.0, 1.0]], style: LineStyle::default() },
            ],
            x: Axis::default(),
            y: Axis::default(),
            reveal: Reveal::default(),
            markers: vec![],
            grid: true,
            axis_colour: None,
            label_style: None,
        };
        assert!(spec.is_categorical());
        assert!(spec.problems().iter().any(|p| p.contains("either xy or categorical")));
    }

    #[test]
    fn reveal_sweeps_then_holds() {
        let r = Reveal::over(2.0);
        assert_eq!(r.sweep_at(0.0), 0.0);
        assert_eq!(r.sweep_at(2.0), 1.0);
        assert_eq!(r.sweep_at(5.0), 1.0); // holds past the end
        // A static reveal is fully drawn from the first instant.
        assert_eq!(Reveal::default().sweep_at(0.0), 1.0);
    }

    #[test]
    fn round_trips_through_json() {
        let spec = ChartSpec {
            series: vec![
                Series::Function {
                    expr: "40*log(x+1)".into(),
                    samples: 200,
                    style: LineStyle { fill: true, ..LineStyle::default() },
                },
            ],
            x: Axis { min: Some(0.0), max: Some(80.0), label: Some("age".into()), ..Axis::default() },
            y: Axis::default(),
            reveal: Reveal::over(2.5),
            markers: vec![Marker::VLine { x: 40.0, colour: None, label: Some("mid".into()), width: 3.0 }],
            grid: true,
            axis_colour: None,
            label_style: None,
        };
        let s = serde_json::to_string(&spec).unwrap();
        let back: ChartSpec = serde_json::from_str(&s).unwrap();
        assert_eq!(spec, back);
    }
}
