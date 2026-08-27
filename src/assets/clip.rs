//! Video clips, decoded to frames through ffmpeg.
//!
//! ShowReel shells out to `ffmpeg` rather than linking a codec library. That is
//! a deliberate trade: it costs a subprocess per clip at load, and it buys
//! every container and codec ffmpeg supports, no build-time C dependency, and
//! the same binary we already need for encoding. Decoding happens once, into
//! memory, at the size the clip will actually be drawn — a Game Boy capture
//! scaled to a corner inset does not need to be held at source resolution.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use tiny_skia::{Pixmap, PixmapRef};

/// How a clip's own timeline maps onto the film's.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ClipLoop {
    /// Hold the last frame once the clip runs out.
    #[default]
    Hold,
    /// Start again from the beginning.
    Loop,
    /// Draw nothing once it runs out.
    Stop,
}

/// A decoded clip: RGBA frames at a fixed rate.
pub struct Clip {
    frames: Vec<Pixmap>,
    fps: f64,
    width: u32,
    height: u32,
    source: PathBuf,
}

impl Clip {
    /// Decode `path` at `fps`, scaled so its width is at most `max_width`.
    ///
    /// `trim` is `(start_secs, duration_secs)` in the clip's own time; passing
    /// the range actually used keeps a long source file from being held whole.
    pub fn load(
        path: impl AsRef<Path>,
        fps: f64,
        max_width: u32,
        trim: Option<(f64, f64)>,
    ) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            bail!("clip not found: {}", path.display());
        }
        let (w, h) = probe_size(path)?;
        let scale = if w > max_width && max_width > 0 {
            max_width as f64 / w as f64
        } else {
            1.0
        };
        // Even dimensions: odd ones break yuv420p downstream and cost nothing
        // to avoid here.
        let out_w = (((w as f64 * scale) as u32).max(2) / 2) * 2;
        let out_h = (((h as f64 * scale) as u32).max(2) / 2) * 2;

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-nostdin").arg("-loglevel").arg("error");
        if let Some((start, _)) = trim {
            cmd.arg("-ss").arg(format!("{start}"));
        }
        cmd.arg("-i").arg(path);
        if let Some((_, dur)) = trim {
            cmd.arg("-t").arg(format!("{dur}"));
        }
        cmd.arg("-vf")
            .arg(format!("fps={fps},scale={out_w}:{out_h}:flags=bicubic"))
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgba")
            .arg("-");

        let out = cmd
            .output()
            .with_context(|| format!("running ffmpeg to decode {}", path.display()))?;
        if !out.status.success() {
            bail!(
                "ffmpeg failed decoding {}: {}",
                path.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }

        let stride = (out_w * out_h * 4) as usize;
        if stride == 0 {
            bail!("clip {} decoded to zero-sized frames", path.display());
        }
        let n = out.stdout.len() / stride;
        if n == 0 {
            bail!("clip {} decoded to no frames", path.display());
        }
        let mut frames = Vec::with_capacity(n);
        for i in 0..n {
            let raw = &out.stdout[i * stride..(i + 1) * stride];
            let mut p = Pixmap::new(out_w, out_h).context("clip frame allocation")?;
            let dst = p.pixels_mut();
            for (j, px) in dst.iter_mut().enumerate() {
                let o = j * 4;
                *px = tiny_skia::ColorU8::from_rgba(raw[o], raw[o + 1], raw[o + 2], raw[o + 3])
                    .premultiply();
            }
            frames.push(p);
        }
        Ok(Clip { frames, fps, width: out_w, height: out_h, source: path.to_path_buf() })
    }

    /// Build a clip directly from decoded frames, skipping ffmpeg — for tests
    /// and for programmatic films that generate their own footage.
    pub fn from_frames(frames: Vec<Pixmap>, fps: f64) -> Self {
        let (width, height) = frames.first().map(|p| (p.width(), p.height())).unwrap_or((1, 1));
        Clip { frames, fps, width, height, source: PathBuf::new() }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn duration(&self) -> f64 {
        self.frames.len() as f64 / self.fps
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn memory_bytes(&self) -> usize {
        self.frames.len() * (self.width * self.height * 4) as usize
    }

    /// The frame at `t` seconds into the clip, honouring `mode` past the end.
    pub fn frame_at(&self, t: f64, mode: ClipLoop) -> Option<PixmapRef<'_>> {
        if self.frames.is_empty() {
            return None;
        }
        let i = (t * self.fps).floor() as i64;
        let n = self.frames.len() as i64;
        let i = if i < 0 {
            0
        } else if i < n {
            i
        } else {
            match mode {
                ClipLoop::Hold => n - 1,
                ClipLoop::Loop => i.rem_euclid(n),
                ClipLoop::Stop => return None,
            }
        };
        Some(self.frames[i as usize].as_ref())
    }
}

/// Source dimensions, via ffprobe.
pub fn probe_size(path: &Path) -> Result<(u32, u32)> {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=width,height",
            "-of", "csv=s=x:p=0",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("running ffprobe on {}", path.display()))?;
    if !out.status.success() {
        bail!("ffprobe failed on {}: {}", path.display(), String::from_utf8_lossy(&out.stderr).trim());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim().lines().next().unwrap_or("").trim();
    let (w, h) = s.split_once('x').with_context(|| format!("ffprobe gave {s:?} for {}", path.display()))?;
    Ok((w.trim().parse()?, h.trim().parse()?))
}

/// Duration in seconds, via ffprobe.
pub fn probe_duration(path: &Path) -> Result<f64> {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "csv=p=0",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("running ffprobe on {}", path.display()))?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim().parse::<f64>().with_context(|| format!("ffprobe duration for {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy(n: usize) -> Clip {
        Clip {
            frames: (0..n).map(|_| Pixmap::new(2, 2).unwrap()).collect(),
            fps: 10.0,
            width: 2,
            height: 2,
            source: PathBuf::from("<test>"),
        }
    }

    #[test]
    fn hold_pins_the_last_frame_and_loop_wraps() {
        let c = dummy(5); // 0.5s at 10fps
        assert!(c.frame_at(0.0, ClipLoop::Hold).is_some());
        assert!(c.frame_at(99.0, ClipLoop::Hold).is_some(), "hold never runs out");
        assert!(c.frame_at(99.0, ClipLoop::Loop).is_some(), "loop never runs out");
        assert!(c.frame_at(99.0, ClipLoop::Stop).is_none(), "stop does run out");
        assert!((c.duration() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn negative_time_shows_the_first_frame() {
        let c = dummy(3);
        assert!(c.frame_at(-1.0, ClipLoop::Stop).is_some());
    }
}
