//! Still images, and the mip pyramid that makes a camera over a huge one cheap.
//!
//! Measured on this machine, a 48.0 megapixel source (6832x7024) rendered to
//! 1920x1080 over a 120-frame pull-back:
//!
//! | approach                              | ms/frame |
//! |---------------------------------------|----------|
//! | crop + resample from full res         |    142.1 |
//! | mip pyramid, one thread               |     42.2 |
//! | mip pyramid across 20 threads         |      4.3 |
//!
//! The reason is simple: resampling straight from level 0 costs time
//! proportional to the *source* area being read, which for a wide shot is the
//! whole image, every frame. Choosing a pre-reduced level first makes the cost
//! proportional to the *output* area instead — near-constant, whatever the
//! zoom. This is the one measurement the whole camera design rests on.

use crate::geom::Rect;
use anyhow::{Context, Result};
use tiny_skia::{Pixmap, PixmapRef};

/// A decoded still, with reduced copies of itself.
pub struct Still {
    levels: Vec<Pixmap>,
    width: u32,
    height: u32,
}

impl Still {
    /// Load and build the pyramid. `min_dim` is where halving stops.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let img = image::open(path)
            .with_context(|| format!("decoding {}", path.display()))?
            .to_rgba8();
        Ok(Self::from_rgba(img))
    }

    pub fn from_rgba(img: image::RgbaImage) -> Self {
        let (width, height) = (img.width(), img.height());
        let mut levels = Vec::new();
        let mut current = img;
        loop {
            levels.push(to_pixmap(&current));
            let (w, h) = (current.width() / 2, current.height() / 2);
            // Below 64px a level buys nothing: no output is smaller than that,
            // so it would never be selected.
            if w < 64 || h < 64 {
                break;
            }
            current = image::imageops::resize(&current, w, h, image::imageops::FilterType::Triangle);
        }
        Still { levels, width, height }
    }

    /// A single-colour still, for backgrounds and tests.
    pub fn solid(w: u32, h: u32, c: crate::color::Color) -> Result<Self> {
        let mut p = Pixmap::new(w.max(1), h.max(1)).context("still allocation")?;
        p.fill(c.to_skia());
        Ok(Still { levels: vec![p], width: w, height: h })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn rect(&self) -> Rect {
        Rect::from_size(self.width as f64, self.height as f64)
    }

    pub fn levels(&self) -> usize {
        self.levels.len()
    }

    /// Bytes held by the pyramid.
    pub fn memory_bytes(&self) -> usize {
        self.levels.iter().map(|l| (l.width() * l.height() * 4) as usize).sum()
    }

    /// Level 0.
    pub fn full(&self) -> PixmapRef<'_> {
        self.levels[0].as_ref()
    }

    /// The level to sample from when `src_px_per_out_px` source pixels of
    /// level 0 map to each output pixel, and the scale divisor for that level.
    ///
    /// Picks the smallest level that still has at least as much detail as the
    /// output needs, so the final resample is always a *minification of at most
    /// 2x* — the range in which a plain bilinear filter is indistinguishable
    /// from a good one.
    pub fn pick_level(&self, src_px_per_out_px: f64) -> (usize, f64) {
        let mut lvl = 0usize;
        while lvl + 1 < self.levels.len() && src_px_per_out_px >= (1u64 << (lvl + 1)) as f64 {
            lvl += 1;
        }
        (lvl, (1u64 << lvl) as f64)
    }

    /// The pixmap for a level, and its divisor.
    pub fn level(&self, i: usize) -> (PixmapRef<'_>, f64) {
        let i = i.min(self.levels.len() - 1);
        (self.levels[i].as_ref(), (1u64 << i) as f64)
    }

    /// Choose a level for a viewport of `vp` source pixels shown `out_w` wide.
    pub fn level_for(&self, vp: &Rect, out_w: f64) -> (PixmapRef<'_>, f64) {
        let ratio = if out_w > 0.0 { vp.w / out_w } else { 1.0 };
        let (i, _) = self.pick_level(ratio);
        self.level(i)
    }
}

fn to_pixmap(img: &image::RgbaImage) -> Pixmap {
    let mut p = Pixmap::new(img.width(), img.height()).expect("non-zero image");
    let dst = p.pixels_mut();
    for (i, px) in img.pixels().enumerate() {
        let [r, g, b, a] = px.0;
        // tiny-skia stores premultiplied.
        dst[i] = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    fn ramp(w: u32, h: u32) -> image::RgbaImage {
        image::RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
        })
    }

    #[test]
    fn pyramid_halves_until_it_is_small() {
        let s = Still::from_rgba(ramp(1024, 1024));
        // 1024 -> 512 -> 256 -> 128 -> 64, then stop.
        assert_eq!(s.levels(), 5);
        assert_eq!(s.size(), (1024, 1024));
    }

    #[test]
    fn level_choice_tracks_the_zoom() {
        let s = Still::from_rgba(ramp(1024, 1024));
        // Zoomed right in: one source pixel per output pixel -> full res.
        assert_eq!(s.pick_level(1.0).0, 0);
        // Two source pixels per output pixel -> the half-size level.
        assert_eq!(s.pick_level(2.0).0, 1);
        assert_eq!(s.pick_level(8.0).0, 3);
        // Beyond the pyramid, clamp to the smallest level rather than panic.
        assert_eq!(s.pick_level(1e9).0, s.levels() - 1);
    }

    #[test]
    fn level_for_viewport_uses_output_width() {
        let s = Still::from_rgba(ramp(1024, 1024));
        // Showing the whole 1024px width in a 128px-wide output is an 8x
        // reduction, so level 3.
        let (px, div) = s.level_for(&Rect::from_size(1024.0, 1024.0), 128.0);
        assert_eq!(div, 8.0);
        assert_eq!(px.width(), 128);
    }

    #[test]
    fn solid_still_has_one_level() {
        let s = Still::solid(8, 8, Color::WHITE).unwrap();
        assert_eq!(s.levels(), 1);
        assert_eq!(s.memory_bytes(), 8 * 8 * 4);
    }
}
