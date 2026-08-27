//! The renderer: a film and a frame number in, an image out.
//!
//! Frame `n` is a pure function of the description. Nothing here reads the
//! clock, draws a random number or mutates shared state, so the same film
//! renders to the same bytes on every run and on every machine with the same
//! fonts — which is what makes "deterministic render" a property rather than an
//! aspiration, and what makes rendering frames out of order across twenty
//! threads safe rather than merely fast.

use crate::assets::AssetStore;
use crate::canvas::Canvas;
use crate::layer::RenderCtx;
use crate::text::FontDb;
use crate::timeline::{Cut, Film, Scene};
use crate::time::Time;
use anyhow::Result;
use rayon::prelude::*;
use std::time::Instant;

/// Somewhere rendered frames go.
pub trait FrameSink: Send {
    /// Frames arrive in order, starting at the range's start.
    fn accept(&mut self, index: u32, canvas: &Canvas) -> Result<()>;
    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Writes `prefix000001.png` into a directory.
pub struct PngSequence {
    pub dir: std::path::PathBuf,
    pub prefix: String,
    written: usize,
}

impl PngSequence {
    pub fn new(dir: impl Into<std::path::PathBuf>, prefix: impl Into<String>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(PngSequence { dir, prefix: prefix.into(), written: 0 })
    }

    pub fn written(&self) -> usize {
        self.written
    }
}

impl FrameSink for PngSequence {
    fn accept(&mut self, index: u32, canvas: &Canvas) -> Result<()> {
        let path = self.dir.join(format!("{}{:06}.png", self.prefix, index));
        canvas.save_png(&path)?;
        self.written += 1;
        Ok(())
    }
}

/// Keeps frames in memory. For tests and contact sheets.
#[derive(Default)]
pub struct Collect(pub Vec<Canvas>);

impl FrameSink for Collect {
    fn accept(&mut self, _index: u32, canvas: &Canvas) -> Result<()> {
        self.0.push(canvas.clone());
        Ok(())
    }
}

/// What a render cost.
#[derive(Debug, Clone, Copy)]
pub struct RenderStats {
    pub frames: u32,
    pub wall_seconds: f64,
    pub asset_bytes: usize,
    pub width: u32,
    pub height: u32,
}

impl RenderStats {
    pub fn fps(&self) -> f64 {
        if self.wall_seconds <= 0.0 { 0.0 } else { self.frames as f64 / self.wall_seconds }
    }

    pub fn ms_per_frame(&self) -> f64 {
        if self.frames == 0 { 0.0 } else { self.wall_seconds * 1000.0 / self.frames as f64 }
    }
}

impl std::fmt::Display for RenderStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} frames at {}x{} in {:.2}s — {:.1} ms/frame, {:.1} fps, {:.0} MB of assets",
            self.frames,
            self.width,
            self.height,
            self.wall_seconds,
            self.ms_per_frame(),
            self.fps(),
            self.asset_bytes as f64 / 1e6
        )
    }
}

pub struct Renderer<'a> {
    pub film: &'a Film,
    pub assets: &'a AssetStore,
    pub fonts: &'a FontDb,
}

impl<'a> Renderer<'a> {
    pub fn new(film: &'a Film, assets: &'a AssetStore, fonts: &'a FontDb) -> Self {
        Renderer { film, assets, fonts }
    }

    pub fn frame_count(&self) -> u32 {
        self.film.frame_count()
    }

    /// Decode every asset the film names, once, before any parallel work.
    ///
    /// Without this, twenty threads reaching a scene at the same moment each
    /// try to decode the same 48-megapixel still; the cache would serialise
    /// them but only after the work was already duplicated.
    pub fn preload(&self) -> Result<()> {
        for u in self.film.assets_used() {
            match u {
                crate::timeline::AssetUse::Still(a) => {
                    self.assets.still(&a)?;
                }
                crate::timeline::AssetUse::Clip(a, max_w) => {
                    self.assets.clip(&a, self.film.fps, max_w, None)?;
                }
            }
        }
        Ok(())
    }

    fn ctx(&self) -> RenderCtx<'a> {
        RenderCtx {
            assets: self.assets,
            fonts: self.fonts,
            frame: self.film.frame_rect(),
            fps: self.film.fps,
        }
    }

    /// Render one scene at `local` seconds into it.
    fn draw_scene(&self, scene: &Scene, local: Time, into: &mut Canvas) -> Result<()> {
        into.clear(scene.background.unwrap_or(self.film.background));
        let ctx = self.ctx();
        for layer in scene.draw_order() {
            layer.draw(into, &ctx, local, scene.duration)?;
        }
        Ok(())
    }

    /// The image at film time `t`.
    pub fn render_at(&self, t: Time) -> Result<Canvas> {
        let (w, h) = (self.film.width, self.film.height);
        match self.film.timeline.at(t) {
            Cut::Single { scene, local } => {
                let mut c = Canvas::new(w, h)?;
                self.draw_scene(scene, local, &mut c)?;
                Ok(c)
            }
            Cut::Blend { outgoing, outgoing_local, incoming, incoming_local, transition, elapsed } => {
                // Two full frames plus the composite. The cost is why a
                // transition is a distinct concept rather than a layer trick:
                // it is the only thing in the crate that renders twice.
                let mut a = Canvas::new(w, h)?;
                self.draw_scene(outgoing, outgoing_local, &mut a)?;
                let mut b = Canvas::new(w, h)?;
                self.draw_scene(incoming, incoming_local, &mut b)?;
                let mut dst = Canvas::new(w, h)?;
                dst.clear(self.film.background);
                transition.compose(&a, &b, elapsed, &mut dst);
                Ok(dst)
            }
        }
    }

    /// The image for frame `index`.
    ///
    /// The frame's time is its *centre*, not its start: sampling at the start
    /// makes the first frame of every scene the un-animated zeroth state, and
    /// a one-frame entrance never appears at all.
    pub fn render_frame(&self, index: u32) -> Result<Canvas> {
        let t = Time((index as f64 + 0.5) / self.film.fps);
        self.render_at(t)
    }

    /// Render `range` into `sink`, in order, across every available core.
    ///
    /// Frames are rendered in chunks so that memory stays bounded: a 4K frame
    /// is 33 MB, and rendering ten thousand of them into a queue before the
    /// encoder drains it is how a renderer eats a machine.
    pub fn render_range(
        &self,
        range: std::ops::Range<u32>,
        sink: &mut dyn FrameSink,
    ) -> Result<RenderStats> {
        let start = Instant::now();
        self.preload()?;
        let threads = rayon::current_num_threads().max(1);
        let chunk = (threads * 2).max(2);
        let mut done = 0u32;

        for base in range.clone().step_by(chunk) {
            let end = (base + chunk as u32).min(range.end);
            let rendered: Vec<Result<Canvas>> =
                (base..end).into_par_iter().map(|i| self.render_frame(i)).collect();
            for (offset, frame) in rendered.into_iter().enumerate() {
                sink.accept(base + offset as u32, &frame?)?;
                done += 1;
            }
        }
        sink.finish()?;
        Ok(RenderStats {
            frames: done,
            wall_seconds: start.elapsed().as_secs_f64(),
            asset_bytes: self.assets.memory_bytes(),
            width: self.film.width,
            height: self.film.height,
        })
    }

    /// Render the whole film.
    pub fn render_all(&self, sink: &mut dyn FrameSink) -> Result<RenderStats> {
        self.render_range(0..self.frame_count(), sink)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::layer::Layer;
    use crate::timeline::Scene;
    use crate::transition::Transition;

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    fn film() -> Film {
        Film::new(64, 36, 10.0)
            .background(Color::BLACK)
            .open(Scene::new(1.0).layer(Layer::solid(RED)))
            .then(Transition::dissolve(0.4), Scene::new(1.0).layer(Layer::solid(BLUE)))
    }

    fn centre(c: &Canvas) -> (u8, u8, u8) {
        let p = c.as_ref().pixels()[((c.height() / 2) * c.width() + c.width() / 2) as usize];
        (p.red(), p.green(), p.blue())
    }

    #[test]
    fn renders_the_right_scene_at_the_right_time() {
        let f = film();
        let store = AssetStore::new();
        let r = Renderer::new(&f, &store, FontDb::shared());
        assert_eq!(centre(&r.render_at(Time(0.2)).unwrap()).0, 255);
        assert_eq!(centre(&r.render_at(Time(1.5)).unwrap()).2, 255);
    }

    #[test]
    fn a_transition_blends_the_two_scenes() {
        let f = film();
        let store = AssetStore::new();
        let r = Renderer::new(&f, &store, FontDb::shared());
        // The transition runs from 0.6s to 1.0s; sample its middle.
        let (red, _, blue) = centre(&r.render_at(Time(0.8)).unwrap());
        assert!(red > 60 && red < 200, "red {red}");
        assert!(blue > 60 && blue < 200, "blue {blue}");
    }

    #[test]
    fn frame_count_matches_the_duration() {
        // 1 + 1 - 0.4 = 1.6s at 10fps = 16 frames.
        assert_eq!(film().frame_count(), 16);
    }

    #[test]
    fn rendering_is_deterministic() {
        let f = film();
        let store = AssetStore::new();
        let r = Renderer::new(&f, &store, FontDb::shared());
        let a = r.render_frame(7).unwrap();
        let b = r.render_frame(7).unwrap();
        assert_eq!(a.data(), b.data(), "the same frame must render identically");
    }

    #[test]
    fn parallel_render_produces_frames_in_order() {
        let f = film();
        let store = AssetStore::new();
        let r = Renderer::new(&f, &store, FontDb::shared());
        let mut sink = Collect::default();
        let stats = r.render_all(&mut sink).unwrap();
        assert_eq!(stats.frames, 16);
        assert_eq!(sink.0.len(), 16);
        // First frame is the opening scene, last is the closing one.
        assert_eq!(centre(&sink.0[0]).0, 255);
        assert_eq!(centre(&sink.0[15]).2, 255);
    }

    #[test]
    fn parallel_and_serial_renders_agree() {
        let f = film();
        let store = AssetStore::new();
        let r = Renderer::new(&f, &store, FontDb::shared());
        let mut sink = Collect::default();
        r.render_all(&mut sink).unwrap();
        for (i, c) in sink.0.iter().enumerate() {
            assert_eq!(
                c.data(),
                r.render_frame(i as u32).unwrap().data(),
                "frame {i} differs between the parallel and serial paths"
            );
        }
    }

    #[test]
    fn a_range_renders_only_what_was_asked_for() {
        let f = film();
        let store = AssetStore::new();
        let r = Renderer::new(&f, &store, FontDb::shared());
        let mut sink = Collect::default();
        let stats = r.render_range(4..9, &mut sink).unwrap();
        assert_eq!(stats.frames, 5);
        assert_eq!(sink.0.len(), 5);
    }

    #[test]
    fn frames_are_sampled_at_their_centre() {
        // At 10fps, frame 0 samples 0.05s, not 0.0s, so a layer that only
        // exists for the first frame is actually seen.
        let f = Film::new(16, 16, 10.0)
            .background(Color::BLACK)
            .open(Scene::new(1.0).layer(Layer::solid(RED).lasting(0.09)));
        let store = AssetStore::new();
        let r = Renderer::new(&f, &store, FontDb::shared());
        assert_eq!(centre(&r.render_frame(0).unwrap()).0, 255);
        assert_eq!(centre(&r.render_frame(3).unwrap()).0, 0);
    }
}
