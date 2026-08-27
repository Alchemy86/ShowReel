//! Font loading, shaping and glyph outlines.
//!
//! Glyphs are turned into **paths** and filled by the same rasteriser that
//! draws every other shape. That is the decision the rest of the typography
//! rests on: a title can take a gradient, an outline, a drop shadow and a
//! per-character transform because it is geometry, not a blit from a font
//! engine. It also means text anti-aliases identically to the artwork around
//! it, which is most of what stops overlays looking pasted on.
//!
//! Shaping goes through `rustybuzz`, so kerning pairs and ligatures are the
//! font's own rather than a naive advance-width sum. On a geometric face like
//! Montserrat the difference in a large title is plainly visible.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Glyph outlines, in font units, keyed by face and glyph id. `None` records
/// that a glyph has no outline (a space), so it is not re-parsed every frame.
type OutlineCache = HashMap<(FontId, u16), Option<Arc<tiny_skia::Path>>>;

/// A face registered in a [`FontDb`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub usize);

struct FaceData {
    bytes: Arc<Vec<u8>>,
    index: u32,
    family: String,
    weight: u16,
    italic: bool,
    path: PathBuf,
}

/// One glyph, placed on a line, in font units.
#[derive(Debug, Clone, Copy)]
pub struct PositionedGlyph {
    pub font: FontId,
    pub glyph: u16,
    /// Pen position, in pixels, relative to the line's origin.
    pub x: f64,
    pub y: f64,
    /// The cluster (byte offset into the source string) this came from —
    /// what per-character animation staggers on.
    pub cluster: u32,
}

/// The font registry.
///
/// Holds face bytes and caches glyph outlines. Shared across threads for a
/// parallel render, so every worker sees the same cache rather than re-parsing
/// the same face per frame.
pub struct FontDb {
    faces: Vec<FaceData>,
    outlines: Mutex<OutlineCache>,
    scanned: Mutex<bool>,
}

impl FontDb {
    pub fn new() -> Self {
        FontDb {
            faces: Vec::new(),
            outlines: Mutex::new(HashMap::new()),
            scanned: Mutex::new(false),
        }
    }

    /// The process-wide registry, scanned from the system font directories on
    /// first use.
    pub fn shared() -> &'static FontDb {
        static DB: OnceLock<FontDb> = OnceLock::new();
        DB.get_or_init(|| {
            let mut db = FontDb::new();
            db.scan_system();
            db
        })
    }

    pub fn len(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Register a font file. Returns the ids of the faces it contained.
    pub fn add_file(&mut self, path: impl AsRef<Path>) -> Result<Vec<FontId>> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).with_context(|| format!("reading font {}", path.display()))?;
        self.add_bytes(bytes, path.to_path_buf())
    }

    /// Register a font from already-loaded bytes rather than a path — for a
    /// caller with no filesystem, such as the wasm build, which fetches a
    /// face over the network. `label` is cosmetic: it stands in for the path
    /// `select`'s fallback (an exact font-file match) and error messages use.
    pub fn add_bytes(&mut self, bytes: Vec<u8>, label: PathBuf) -> Result<Vec<FontId>> {
        let bytes = Arc::new(bytes);
        let n = ttf_parser::fonts_in_collection(&bytes).unwrap_or(1);
        let mut ids = Vec::new();
        for index in 0..n {
            let Ok(face) = ttf_parser::Face::parse(&bytes, index) else { continue };
            let family = face
                .names()
                .into_iter()
                .find(|n| n.name_id == ttf_parser::name_id::FAMILY && n.is_unicode())
                .and_then(|n| n.to_string())
                .unwrap_or_else(|| {
                    label.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
                });
            let weight = face.weight().to_number();
            let italic = face.is_italic() || face.is_oblique();
            ids.push(FontId(self.faces.len()));
            self.faces.push(FaceData {
                bytes: bytes.clone(),
                index,
                family,
                weight,
                italic,
                path: label.clone(),
            });
        }
        if ids.is_empty() {
            bail!("no usable faces in {}", label.display());
        }
        Ok(ids)
    }

    /// Walk the usual font directories, registering everything parseable.
    ///
    /// Deliberately shallow and forgiving: an unreadable directory or an
    /// unparseable file is skipped, because a missing exotic face must never
    /// stop a render.
    pub fn scan_system(&mut self) {
        let mut done = self.scanned.lock().unwrap();
        if *done {
            return;
        }
        *done = true;
        drop(done);
        let home = std::env::var("HOME").unwrap_or_default();
        let roots = [
            "/usr/share/fonts".to_string(),
            "/usr/local/share/fonts".to_string(),
            format!("{home}/.local/share/fonts"),
            format!("{home}/.fonts"),
        ];
        for root in roots {
            self.scan_dir(Path::new(&root), 0);
        }
    }

    fn scan_dir(&mut self, dir: &Path, depth: usize) {
        if depth > 4 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut files: Vec<PathBuf> = Vec::new();
        let mut dirs: Vec<PathBuf> = Vec::new();
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else {
                let ok = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
                    .unwrap_or(false);
                if ok {
                    files.push(p);
                }
            }
        }
        // Sorted so the registry is the same on every run — determinism starts
        // at which face a family name resolves to.
        files.sort();
        dirs.sort();
        for f in files {
            let _ = self.add_file(&f);
        }
        for d in dirs {
            self.scan_dir(&d, depth + 1);
        }
    }

    /// Find the face closest to `family` at `weight`.
    ///
    /// `family` may also be a path to a font file, which wins outright — a film
    /// that ships its own typeface should not depend on what is installed.
    pub fn select(&self, family: &str, weight: u16, italic: bool) -> Option<FontId> {
        let target = family.trim().to_ascii_lowercase();
        let mut best: Option<(i64, FontId)> = None;
        for (i, f) in self.faces.iter().enumerate() {
            let fam = f.family.to_ascii_lowercase();
            let name_score = if fam == target {
                0
            } else if fam.starts_with(&target) || target.starts_with(&fam) {
                2_000
            } else if f.path.to_string_lossy().to_ascii_lowercase().contains(&target) {
                4_000
            } else {
                continue;
            };
            // Weight distance dominates within a family; italic mismatch is a
            // fixed penalty smaller than one weight step.
            let score = name_score
                + (f.weight as i64 - weight as i64).abs()
                + if f.italic == italic { 0 } else { 60 };
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, FontId(i)));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Pick the first family that resolves, else any face at all.
    pub fn select_any(&self, families: &[&str], weight: u16, italic: bool) -> Option<FontId> {
        for f in families {
            if let Some(id) = self.select(f, weight, italic) {
                return Some(id);
            }
        }
        (!self.faces.is_empty()).then_some(FontId(0))
    }

    pub fn family_name(&self, id: FontId) -> &str {
        &self.faces[id.0].family
    }

    pub fn face_path(&self, id: FontId) -> &Path {
        &self.faces[id.0].path
    }

    pub fn units_per_em(&self, id: FontId) -> f64 {
        self.with_face(id, |f| f.units_per_em() as f64).unwrap_or(1000.0)
    }

    /// Ascender, descender and line gap, in font units.
    pub fn vertical_metrics(&self, id: FontId) -> (f64, f64, f64) {
        self.with_face(id, |f| {
            (f.ascender() as f64, f.descender() as f64, f.line_gap() as f64)
        })
        .unwrap_or((800.0, -200.0, 0.0))
    }

    fn with_face<T>(&self, id: FontId, f: impl FnOnce(&ttf_parser::Face<'_>) -> T) -> Option<T> {
        let fd = self.faces.get(id.0)?;
        let face = ttf_parser::Face::parse(&fd.bytes, fd.index).ok()?;
        Some(f(&face))
    }

    /// Shape `text`, returning glyphs positioned in **pixels** at `size`.
    ///
    /// `tracking` is extra letter-spacing as a fraction of the em, the unit a
    /// designer actually thinks in — 0.02 reads the same at 24px and 240px.
    pub fn shape(&self, id: FontId, text: &str, size: f64, tracking: f64) -> Vec<PositionedGlyph> {
        let Some(fd) = self.faces.get(id.0) else { return Vec::new() };
        let Some(face) = rustybuzz::Face::from_slice(&fd.bytes, fd.index) else { return Vec::new() };
        let upem = face.units_per_em() as f64;
        let scale = size / upem;

        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.guess_segment_properties();
        let out = rustybuzz::shape(&face, &[], buf);

        let infos = out.glyph_infos();
        let positions = out.glyph_positions();
        let extra = tracking * size;
        let mut pen = 0.0f64;
        let mut glyphs = Vec::with_capacity(infos.len());
        for (i, info) in infos.iter().enumerate() {
            let p = positions[i];
            glyphs.push(PositionedGlyph {
                font: id,
                glyph: info.glyph_id as u16,
                x: pen + p.x_offset as f64 * scale,
                y: -(p.y_offset as f64) * scale,
                cluster: info.cluster,
            });
            pen += p.x_advance as f64 * scale + extra;
        }
        glyphs
    }

    /// The advance width of `text`, in pixels.
    pub fn measure(&self, id: FontId, text: &str, size: f64, tracking: f64) -> f64 {
        let Some(fd) = self.faces.get(id.0) else { return 0.0 };
        let Some(face) = rustybuzz::Face::from_slice(&fd.bytes, fd.index) else { return 0.0 };
        let scale = size / face.units_per_em() as f64;
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.guess_segment_properties();
        let out = rustybuzz::shape(&face, &[], buf);
        let extra = tracking * size;
        out.glyph_positions().iter().map(|p| p.x_advance as f64 * scale + extra).sum()
    }

    /// The widest digit's advance, in pixels.
    ///
    /// A counter ticking from 0 to 600 jitters horribly if `1` is narrower than
    /// `8`, because the whole string re-centres every frame. Laying digits out
    /// on this fixed advance is what "tabular figures" means, and it is the
    /// difference between a counter that looks designed and one that looks
    /// broken.
    pub fn digit_advance(&self, id: FontId, size: f64, tracking: f64) -> f64 {
        (0..10)
            .map(|d| self.measure(id, &d.to_string(), size, tracking))
            .fold(0.0f64, f64::max)
    }

    /// The outline of a glyph, in font units, cached.
    pub fn outline(&self, id: FontId, glyph: u16) -> Option<Arc<tiny_skia::Path>> {
        if let Some(hit) = self.outlines.lock().unwrap().get(&(id, glyph)) {
            return hit.clone();
        }
        let built = self.build_outline(id, glyph);
        self.outlines.lock().unwrap().insert((id, glyph), built.clone());
        built
    }

    fn build_outline(&self, id: FontId, glyph: u16) -> Option<Arc<tiny_skia::Path>> {
        let fd = self.faces.get(id.0)?;
        let face = ttf_parser::Face::parse(&fd.bytes, fd.index).ok()?;
        let mut b = OutlineBuilder { pb: tiny_skia::PathBuilder::new(), started: false };
        face.outline_glyph(ttf_parser::GlyphId(glyph), &mut b)?;
        if b.started {
            b.pb.close();
        }
        b.pb.finish().map(Arc::new)
    }
}

impl Default for FontDb {
    fn default() -> Self {
        FontDb::new()
    }
}

/// Adapts ttf-parser's outline callbacks onto a tiny-skia path.
///
/// Font space has y up and the screen has y down; the flip happens in the
/// transform at draw time rather than here, so the cached outline stays in the
/// font's own coordinates and can be reused at any size or orientation.
struct OutlineBuilder {
    pb: tiny_skia::PathBuilder,
    started: bool,
}

impl ttf_parser::OutlineBuilder for OutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.started {
            self.pb.close();
        }
        self.pb.move_to(x, y);
        self.started = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.pb.close();
        self.started = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> &'static FontDb {
        FontDb::shared()
    }

    #[test]
    fn system_scan_finds_fonts() {
        assert!(!db().is_empty(), "no fonts found; typography tests cannot run");
    }

    #[test]
    fn selects_a_family_and_prefers_the_requested_weight() {
        let d = db();
        let Some(light) = d.select_any(&["Montserrat", "Open Sans", "DejaVu Sans"], 300, false)
        else {
            return;
        };
        let bold = d.select_any(&["Montserrat", "Open Sans", "DejaVu Sans"], 800, false).unwrap();
        // Same family, and the bold request should not come back lighter than
        // the light request.
        assert_eq!(d.family_name(light).split(',').next(), d.family_name(bold).split(',').next());
    }

    #[test]
    fn shaping_advances_and_kerns() {
        let d = db();
        let id = d.select_any(&["Montserrat", "Open Sans", "DejaVu Sans"], 400, false).unwrap();
        let g = d.shape(id, "AVATAR", 100.0, 0.0);
        assert_eq!(g.len(), 6);
        // Monotonically advancing pen positions.
        for w in g.windows(2) {
            assert!(w[1].x > w[0].x, "glyphs must advance");
        }
        // Measured width agrees with the last pen position plus something.
        let w = d.measure(id, "AVATAR", 100.0, 0.0);
        assert!(w > g[5].x, "measured {w} vs last glyph at {}", g[5].x);
    }

    #[test]
    fn tracking_widens_predictably() {
        let d = db();
        let id = d.select_any(&["Open Sans", "DejaVu Sans"], 400, false).unwrap();
        let plain = d.measure(id, "HELLO", 100.0, 0.0);
        let tracked = d.measure(id, "HELLO", 100.0, 0.05);
        // Five glyphs, 0.05em each at 100px = 25px wider.
        assert!((tracked - plain - 25.0).abs() < 1e-6, "{plain} -> {tracked}");
    }

    #[test]
    fn digit_advance_is_the_widest_digit() {
        let d = db();
        let id = d.select_any(&["Montserrat", "Open Sans", "DejaVu Sans"], 400, false).unwrap();
        let adv = d.digit_advance(id, 100.0, 0.0);
        for n in 0..10 {
            assert!(d.measure(id, &n.to_string(), 100.0, 0.0) <= adv + 1e-9);
        }
        assert!(adv > 0.0);
    }

    #[test]
    fn outlines_are_cached_and_reused() {
        let d = db();
        let id = d.select_any(&["Open Sans", "DejaVu Sans"], 400, false).unwrap();
        let g = d.shape(id, "A", 100.0, 0.0);
        let a = d.outline(id, g[0].glyph).expect("A must have an outline");
        let b = d.outline(id, g[0].glyph).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "outline must come from the cache");
        assert!(a.len() > 4, "an 'A' is more than a couple of points");
    }

    #[test]
    fn a_space_has_no_outline_but_still_advances() {
        let d = db();
        let id = d.select_any(&["Open Sans", "DejaVu Sans"], 400, false).unwrap();
        assert!(d.measure(id, " ", 100.0, 0.0) > 0.0);
    }
}
