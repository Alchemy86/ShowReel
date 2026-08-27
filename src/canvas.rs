//! The frame buffer, and everything drawn onto it.
//!
//! A [`Canvas`] is premultiplied RGBA (tiny-skia's own layout) so that layers
//! can be composited without a conversion per operation. Every drawing
//! primitive the rest of the crate needs lands here, which keeps the render
//! path in one place and makes "same description in, same frames out" a
//! property of one file rather than a hope.

use crate::color::{Color, Paint};
use crate::geom::Rect;
use anyhow::{Context, Result};
use tiny_skia::{
    BlendMode, FillRule, FilterQuality, Mask, Paint as SkPaint, PathBuilder, Pixmap, PixmapPaint,
    PixmapRef, Rect as SkRect, Shader, Stroke, Transform,
};

#[derive(Clone)]
pub struct Canvas {
    pub(crate) pixmap: Pixmap,
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Result<Self> {
        let pixmap = Pixmap::new(w.max(1), h.max(1)).context("canvas allocation failed")?;
        Ok(Canvas { pixmap })
    }

    /// A canvas filled with an opaque colour.
    pub fn filled(w: u32, h: u32, c: Color) -> Result<Self> {
        let mut cv = Canvas::new(w, h)?;
        cv.pixmap.fill(c.to_skia());
        Ok(cv)
    }

    pub fn from_pixmap(pixmap: Pixmap) -> Self {
        Canvas { pixmap }
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    pub fn rect(&self) -> Rect {
        Rect::from_size(self.width() as f64, self.height() as f64)
    }

    pub fn as_ref(&self) -> PixmapRef<'_> {
        self.pixmap.as_ref()
    }

    pub fn clear(&mut self, c: Color) {
        self.pixmap.fill(c.to_skia());
    }

    /// Premultiplied RGBA bytes.
    pub fn data(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Un-premultiplied RGB, the layout ffmpeg's `rgb24` wants.
    ///
    /// The frame is composited over `bg` first: an mp4 has no alpha channel, so
    /// anything still transparent at this point has to become *something*, and
    /// silently becoming black is a bug that only shows up in the delivered
    /// file.
    pub fn to_rgb24(&self, bg: Color) -> Vec<u8> {
        let px = self.pixmap.pixels();
        let mut out = Vec::with_capacity(px.len() * 3);
        for p in px {
            let a = p.alpha() as u32;
            // tiny-skia stores premultiplied; demultiply then composite over bg.
            let (r, g, b) = if a == 0 {
                (bg.r as u32, bg.g as u32, bg.b as u32)
            } else if a == 255 {
                (p.red() as u32, p.green() as u32, p.blue() as u32)
            } else {
                let un = |c: u8| (c as u32 * 255 / a).min(255);
                let (r, g, b) = (un(p.red()), un(p.green()), un(p.blue()));
                let over = |c: u32, d: u32| (c * a + d * (255 - a)) / 255;
                (over(r, bg.r as u32), over(g, bg.g as u32), over(b, bg.b as u32))
            };
            out.push(r as u8);
            out.push(g as u8);
            out.push(b as u8);
        }
        out
    }

    /// PNG-encoded bytes, for handing a frame to something that isn't a file
    /// — the studio server's `/api/frame`, in particular.
    pub fn encode_png(&self) -> Result<Vec<u8>> {
        self.pixmap.encode_png().context("encoding PNG")
    }

    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        self.pixmap.save_png(path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    // ---- compositing -----------------------------------------------------

    /// Draw another canvas over this one at `dst`, scaled to fit it exactly.
    pub fn draw_canvas(&mut self, src: &Canvas, dst: Rect, opacity: f64) {
        self.draw_pixmap_rect(src.as_ref(), dst, opacity, BlendMode::SourceOver);
    }

    /// Draw `src` into `dst`, with `opacity` and an explicit blend mode.
    pub fn draw_pixmap_rect(
        &mut self,
        src: PixmapRef<'_>,
        dst: Rect,
        opacity: f64,
        blend: BlendMode,
    ) {
        if dst.w <= 0.0 || dst.h <= 0.0 || opacity <= 0.0 {
            return;
        }
        let sx = dst.w / src.width() as f64;
        let sy = dst.h / src.height() as f64;
        let tf = Transform::from_row(sx as f32, 0.0, 0.0, sy as f32, dst.x as f32, dst.y as f32);
        // Nearest is right when a pixel-art source is being magnified — it
        // keeps the pixels crisp instead of turning a Game Boy screen to mush.
        let quality = if sx >= 1.5 && sy >= 1.5 { FilterQuality::Nearest } else { FilterQuality::Bilinear };
        let paint = PixmapPaint { opacity: opacity as f32, blend_mode: blend, quality };
        self.pixmap.draw_pixmap(0, 0, src, &paint, tf, None);
    }

    /// Draw the `vp` region of `src` — in `src`'s own pixel coordinates — into
    /// `dst`, scaled to fill it. The shared basis for a camera move, whether
    /// the source is a still's mip level or a decoded video frame.
    pub fn draw_pixmap_cropped(
        &mut self,
        src: PixmapRef<'_>,
        vp: Rect,
        dst: Rect,
        opacity: f64,
        mask: Option<&Mask>,
    ) {
        if dst.w <= 0.0 || dst.h <= 0.0 || vp.w <= 0.0 || vp.h <= 0.0 || opacity <= 0.0 {
            return;
        }
        let sx = (dst.w / vp.w) as f32;
        let sy = (dst.h / vp.h) as f32;
        let tf = Transform::from_row(
            sx,
            0.0,
            0.0,
            sy,
            (dst.x - vp.x * sx as f64) as f32,
            (dst.y - vp.y * sy as f64) as f32,
        );
        let quality = if sx >= 1.5 && sy >= 1.5 { FilterQuality::Nearest } else { FilterQuality::Bilinear };
        let paint = PixmapPaint { opacity: opacity as f32, blend_mode: BlendMode::SourceOver, quality };
        self.pixmap.draw_pixmap(0, 0, src, &paint, tf, mask);
    }

    /// Draw `src` into `dst` through `mask` — the basis of wipes and irises.
    pub fn draw_pixmap_masked(&mut self, src: PixmapRef<'_>, dst: Rect, opacity: f64, mask: &Mask) {
        if dst.w <= 0.0 || dst.h <= 0.0 || opacity <= 0.0 {
            return;
        }
        let sx = (dst.w / src.width() as f64) as f32;
        let sy = (dst.h / src.height() as f64) as f32;
        let tf = Transform::from_row(sx, 0.0, 0.0, sy, dst.x as f32, dst.y as f32);
        let paint = PixmapPaint {
            opacity: opacity as f32,
            blend_mode: BlendMode::SourceOver,
            quality: FilterQuality::Bilinear,
        };
        self.pixmap.draw_pixmap(0, 0, src, &paint, tf, Some(mask));
    }

    // ---- shapes ----------------------------------------------------------

    pub fn fill_rect(&mut self, r: Rect, paint: &Paint) {
        self.fill_round_rect(r, 0.0, paint);
    }

    /// A rounded rectangle. The backing plate under a lower-third is this.
    pub fn fill_round_rect(&mut self, r: Rect, radius: f64, paint: &Paint) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let path = match round_rect_path(r, radius) {
            Some(p) => p,
            None => return,
        };
        self.fill_path_with(&path, paint, r);
    }

    pub fn stroke_round_rect(&mut self, r: Rect, radius: f64, width: f64, paint: &Paint) {
        if r.w <= 0.0 || r.h <= 0.0 || width <= 0.0 {
            return;
        }
        if let Some(path) = round_rect_path(r, radius) {
            let mut sk = SkPaint { anti_alias: true, ..Default::default() };
            sk.shader = paint.shader(r);
            let stroke = Stroke { width: width as f32, ..Default::default() };
            self.pixmap.stroke_path(&path, &sk, &stroke, Transform::identity(), None);
        }
    }

    /// Fill an arbitrary path, `bbox` being the box a gradient spans.
    pub fn fill_path_with(&mut self, path: &tiny_skia::Path, paint: &Paint, bbox: Rect) {
        let mut sk = SkPaint { anti_alias: true, ..Default::default() };
        sk.shader = paint.shader(bbox);
        self.pixmap.fill_path(path, &sk, FillRule::Winding, Transform::identity(), None);
    }

    pub fn fill_path_shader(&mut self, path: &tiny_skia::Path, shader: Shader<'_>) {
        let mut sk = SkPaint { anti_alias: true, ..Default::default() };
        sk.shader = shader;
        self.pixmap.fill_path(path, &sk, FillRule::Winding, Transform::identity(), None);
    }

    pub fn stroke_path_with(&mut self, path: &tiny_skia::Path, paint: &Paint, width: f64, bbox: Rect) {
        let mut sk = SkPaint { anti_alias: true, ..Default::default() };
        sk.shader = paint.shader(bbox);
        let stroke = Stroke {
            width: width as f32,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };
        self.pixmap.stroke_path(path, &sk, &stroke, Transform::identity(), None);
    }

    pub fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, width: f64, paint: &Paint) {
        let mut pb = PathBuilder::new();
        pb.move_to(x0 as f32, y0 as f32);
        pb.line_to(x1 as f32, y1 as f32);
        if let Some(p) = pb.finish() {
            let bbox = Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs().max(1.0), (y1 - y0).abs().max(1.0));
            self.stroke_path_with(&p, paint, width, bbox);
        }
    }

    /// Darken the whole canvas — the "everything except the thing" half of a
    /// spotlight.
    pub fn dim(&mut self, amount: f64, colour: Color) {
        if amount <= 0.0 {
            return;
        }
        let r = self.rect();
        self.fill_rect(r, &Paint::Solid(colour.opacity(amount)));
    }

    /// A soft vertical gradient from the bottom edge — the scrim that makes
    /// white text legible over arbitrary footage.
    pub fn scrim_bottom(&mut self, height_frac: f64, strength: f64, colour: Color) {
        let h = self.height() as f64 * height_frac.clamp(0.0, 1.0);
        if h <= 0.0 || strength <= 0.0 {
            return;
        }
        let r = Rect::new(0.0, self.height() as f64 - h, self.width() as f64, h);
        self.fill_rect(
            r,
            &Paint::Linear {
                stops: vec![
                    (0.0, colour.opacity(0.0)),
                    (0.55, colour.opacity(strength * 0.55)),
                    (1.0, colour.opacity(strength)),
                ],
                angle: 90.0,
            },
        );
    }
}

/// A rounded-rect path, clamping the radius so it cannot self-intersect.
pub fn round_rect_path(r: Rect, radius: f64) -> Option<tiny_skia::Path> {
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    if radius <= 0.01 {
        let sk = SkRect::from_xywh(r.x as f32, r.y as f32, r.w as f32, r.h as f32)?;
        return PathBuilder::from_rect(sk).into();
    }
    let (x, y, w, h, rr) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32, radius as f32);
    // 0.5523 is the circle-to-cubic constant; the arcs are indistinguishable
    // from true quarter-circles at any size a frame will show.
    let k = rr * 0.552_285;
    let mut pb = PathBuilder::new();
    pb.move_to(x + rr, y);
    pb.line_to(x + w - rr, y);
    pb.cubic_to(x + w - rr + k, y, x + w, y + rr - k, x + w, y + rr);
    pb.line_to(x + w, y + h - rr);
    pb.cubic_to(x + w, y + h - rr + k, x + w - rr + k, y + h, x + w - rr, y + h);
    pb.line_to(x + rr, y + h);
    pb.cubic_to(x + rr - k, y + h, x, y + h - rr + k, x, y + h - rr);
    pb.line_to(x, y + rr);
    pb.cubic_to(x, y + rr - k, x + rr - k, y, x + rr, y);
    pb.close();
    pb.finish()
}

/// A separable box blur run three times, which approximates a Gaussian closely
/// enough for a drop shadow and is O(n) rather than O(n·r).
///
/// Shadows are what stop overlay text looking pasted on, so this is load
/// bearing for the typography rather than decoration.
pub fn blur_alpha(pixmap: &mut Pixmap, radius: f64) {
    let r = radius.round() as i32;
    if r < 1 {
        return;
    }
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    // Work on the alpha channel only: a shadow is a silhouette.
    let mut a: Vec<u16> = pixmap.pixels().iter().map(|p| p.alpha() as u16).collect();
    let mut tmp = vec![0u16; a.len()];
    for _ in 0..3 {
        box_blur_pass(&a, &mut tmp, w, h, r, true);
        box_blur_pass(&tmp, &mut a, w, h, r, false);
    }
    // Rebuild as a premultiplied black silhouette; the caller tints it.
    let data = pixmap.pixels_mut();
    for (px, av) in data.iter_mut().zip(a.iter()) {
        let av = (*av).min(255) as u8;
        *px = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, av)
            .unwrap_or(tiny_skia::PremultipliedColorU8::TRANSPARENT);
    }
}

fn box_blur_pass(src: &[u16], dst: &mut [u16], w: i32, h: i32, r: i32, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let win = (2 * r + 1) as u32;
    for o in 0..outer {
        let idx = |i: i32| -> usize {
            if horizontal { (o * w + i) as usize } else { (i * w + o) as usize }
        };
        let mut sum: u32 = 0;
        for i in -r..=r {
            sum += src[idx(i.clamp(0, inner - 1))] as u32;
        }
        for i in 0..inner {
            dst[idx(i)] = (sum / win) as u16;
            let out = idx((i - r).clamp(0, inner - 1));
            let inn = idx((i + r + 1).clamp(0, inner - 1));
            sum = sum + src[inn] as u32 - src[out] as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb24_composites_transparency_over_the_background() {
        // A fully transparent canvas must come out as the background colour,
        // not black, or every delivered mp4 gets a black hole in it.
        let cv = Canvas::new(2, 2).unwrap();
        let rgb = cv.to_rgb24(Color::rgb(10, 20, 30));
        assert_eq!(&rgb[0..3], &[10, 20, 30]);
        assert_eq!(rgb.len(), 2 * 2 * 3);
    }

    #[test]
    fn opaque_pixels_survive_the_round_trip() {
        let cv = Canvas::filled(2, 2, Color::rgb(200, 100, 50)).unwrap();
        let rgb = cv.to_rgb24(Color::BLACK);
        assert_eq!(&rgb[0..3], &[200, 100, 50]);
    }

    #[test]
    fn round_rect_radius_cannot_self_intersect() {
        // A radius larger than the box is clamped rather than producing a
        // degenerate path.
        assert!(round_rect_path(Rect::new(0.0, 0.0, 10.0, 10.0), 500.0).is_some());
    }

    #[test]
    fn blur_spreads_alpha_outward() {
        let mut p = Pixmap::new(21, 21).unwrap();
        p.pixels_mut()[10 * 21 + 10] =
            tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 255).unwrap();
        blur_alpha(&mut p, 3.0);
        assert!(p.pixels()[10 * 21 + 12].alpha() > 0, "blur must reach neighbours");
    }
}
