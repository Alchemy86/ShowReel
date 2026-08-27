//! Handing frames to ffmpeg.
//!
//! # Why this is ShowReel's own code
//!
//! `agentgb`'s `video.py` already owns an `encode`/`mobile_cut` pair, and the
//! brief was explicit that ShowReel must not silently fork it. The decision
//! here is to **reimplement, and say so**: `agentgb` is a Python package and
//! ShowReel is a Rust binary, so "depend on it" would mean shelling out to a
//! Python interpreter to build an ffmpeg command line — more moving parts than
//! the command line itself, and a dependency on somebody else's tree for a
//! general-purpose tool that must not know what a Game Boy is. What is
//! deliberately kept is the *recipe*, because it encodes a fact about the
//! world rather than a fact about `agentgb`:
//!
//! > Telegram's `sendVideo` refuses 60fps outright, whatever the file size.
//!
//! So the mobile cut is `scale=720:-2, fps=30, yuv420p, +faststart`, and it is
//! produced by default rather than on request.
//!
//! Frames are piped to ffmpeg as raw `rgb24` on stdin rather than written out
//! as numbered PNGs. It is materially faster — no PNG compression, no
//! filesystem round trip — and just as deterministic. PNG sequences are still
//! available via [`crate::render::PngSequence`] when you want the frames
//! themselves.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::render::FrameSink;
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Encoder settings for the full-quality master.
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    /// Constant Rate Factor: lower is better quality and a bigger file.
    pub crf: u8,
    pub preset: String,
    pub pixel_format: String,
    /// Put the moov atom first, so the file starts playing before it has
    /// finished downloading.
    pub faststart: bool,
    /// `libx264` unless you have a reason.
    pub codec: String,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        EncodeOptions {
            crf: 17,
            preset: "slow".into(),
            // yuv420p is the only chroma layout every player and every phone
            // agrees on; anything else risks a file that plays here and not
            // on the device it was made for.
            pixel_format: "yuv420p".into(),
            faststart: true,
            codec: "libx264".into(),
        }
    }
}

impl EncodeOptions {
    /// Faster and rougher, for preview passes.
    pub fn preview() -> Self {
        EncodeOptions { crf: 26, preset: "veryfast".into(), ..Default::default() }
    }
}

/// The mobile delivery cut.
#[derive(Debug, Clone)]
pub struct MobileOptions {
    pub width: u32,
    pub fps: u32,
    pub crf: u8,
    pub preset: String,
}

impl Default for MobileOptions {
    fn default() -> Self {
        // 720 wide and 30fps: Telegram's sendVideo rejects 60fps outright, so
        // this is a delivery requirement rather than a size optimisation.
        MobileOptions { width: 720, fps: 30, crf: 23, preset: "medium".into() }
    }
}

/// An ffmpeg process being fed raw frames.
pub struct FfmpegSink {
    child: Option<Child>,
    output: PathBuf,
    background: Color,
    frames: u64,
}

impl FfmpegSink {
    pub fn new(
        output: impl AsRef<Path>,
        width: u32,
        height: u32,
        fps: f64,
        opts: &EncodeOptions,
        background: Color,
    ) -> Result<Self> {
        let output = output.as_ref().to_path_buf();
        if let Some(dir) = output.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).ok();
        }
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-nostdin")
            .args(["-loglevel", "error", "-y"])
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24"])
            .args(["-s", &format!("{width}x{height}")])
            .args(["-r", &format!("{fps}")])
            .args(["-i", "-"])
            .args(["-an"])
            .args(["-c:v", &opts.codec])
            .args(["-crf", &opts.crf.to_string()])
            .args(["-preset", &opts.preset])
            .args(["-pix_fmt", &opts.pixel_format]);
        if opts.faststart {
            cmd.args(["-movflags", "+faststart"]);
        }
        cmd.arg(&output).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());

        let child = cmd
            .spawn()
            .with_context(|| format!("starting ffmpeg to write {}", output.display()))?;
        Ok(FfmpegSink { child: Some(child), output, background, frames: 0 })
    }

    pub fn output(&self) -> &Path {
        &self.output
    }

    pub fn frames_written(&self) -> u64 {
        self.frames
    }
}

impl FrameSink for FfmpegSink {
    fn accept(&mut self, _index: u32, canvas: &Canvas) -> Result<()> {
        let Some(child) = self.child.as_mut() else { bail!("encoder already finished") };
        let stdin = child.stdin.as_mut().context("ffmpeg stdin closed")?;
        stdin
            .write_all(&canvas.to_rgb24(self.background))
            .context("writing a frame to ffmpeg (it may have exited early)")?;
        self.frames += 1;
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        let Some(mut child) = self.child.take() else { return Ok(()) };
        // Dropping stdin is what tells ffmpeg the stream is over.
        drop(child.stdin.take());
        let out = child.wait_with_output().context("waiting for ffmpeg")?;
        if !out.status.success() {
            bail!(
                "ffmpeg failed writing {}: {}",
                self.output.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }
}

impl Drop for FfmpegSink {
    fn drop(&mut self) {
        // If the render failed part way, do not leave an ffmpeg behind waiting
        // on a pipe nobody will write to again.
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            let _ = child.wait();
        }
    }
}

/// Produce the mobile-safe cut from a finished master.
///
/// `scale=720:-2` sets the width and lets the height follow, rounded to an even
/// number — the `-2` rather than `-1` is what keeps h264 happy on odd source
/// aspect ratios.
pub fn mobile_cut(input: impl AsRef<Path>, output: impl AsRef<Path>, opts: &MobileOptions) -> Result<()> {
    let (input, output) = (input.as_ref(), output.as_ref());
    if let Some(dir) = output.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).ok();
    }
    let out = Command::new("ffmpeg")
        .arg("-nostdin")
        .args(["-loglevel", "error", "-y"])
        .arg("-i")
        .arg(input)
        .args(["-vf", &format!("scale={}:-2,fps={}", opts.width, opts.fps)])
        .args(["-c:v", "libx264"])
        .args(["-crf", &opts.crf.to_string()])
        .args(["-preset", &opts.preset])
        .args(["-pix_fmt", "yuv420p"])
        .args(["-movflags", "+faststart"])
        .args(["-an"])
        .arg(output)
        .output()
        .with_context(|| format!("running ffmpeg for the mobile cut of {}", input.display()))?;
    if !out.status.success() {
        bail!(
            "ffmpeg failed writing {}: {}",
            output.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// The mobile cut's path for a given master: `film.mp4` -> `film.mobile.mp4`.
pub fn mobile_path(master: &Path) -> PathBuf {
    let stem = master.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "film".into());
    let ext = master.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "mp4".into());
    master.with_file_name(format!("{stem}.mobile.{ext}"))
}

/// Is ffmpeg on the PATH?
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_path_sits_beside_the_master() {
        assert_eq!(
            mobile_path(Path::new("out/reel.mp4")),
            PathBuf::from("out/reel.mobile.mp4")
        );
        assert_eq!(mobile_path(Path::new("reel.mp4")), PathBuf::from("reel.mobile.mp4"));
    }

    #[test]
    fn defaults_encode_a_universally_playable_file() {
        let o = EncodeOptions::default();
        assert_eq!(o.pixel_format, "yuv420p");
        assert!(o.faststart);
        // The mobile cut must be 30fps: Telegram refuses 60.
        assert_eq!(MobileOptions::default().fps, 30);
        assert_eq!(MobileOptions::default().width, 720);
    }

    #[test]
    fn preview_options_trade_quality_for_speed() {
        assert!(EncodeOptions::preview().crf > EncodeOptions::default().crf);
    }
}
