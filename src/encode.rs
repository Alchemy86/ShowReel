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

use crate::audio::{AudioInput, mix_filter};
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
    /// Tracks to mix under the video. Empty means a silent file, encoded with
    /// exactly the command line this took before audio existed.
    pub audio: Vec<AudioInput>,
    /// `aac` is the one audio codec an mp4 can carry that every phone,
    /// browser and messenger decodes.
    pub audio_codec: String,
    pub audio_bitrate: String,
    /// Caps the encoder's own thread count (`-threads N`). `None` leaves
    /// ffmpeg's own auto-detection alone — every existing caller's
    /// behaviour, unchanged. `src/segments.rs`'s concurrent segment workers
    /// set this to roughly `cores / worker_count`: ffmpeg defaults to using
    /// every core it can see for both decode and encode, so N workers each
    /// left at the default would oversubscribe the machine N-fold and cost
    /// wall clock rather than saving it — measured directly (see
    /// `docs/render-budget.md`): four unthrottled concurrent workers on a
    /// 20-core box were *slower* than one serial pass, confirmed by a 15×
    /// jump in involuntary context switches.
    pub threads: Option<u32>,
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
            audio: Vec::new(),
            audio_codec: "aac".into(),
            audio_bitrate: "192k".into(),
            threads: None,
        }
    }
}

impl EncodeOptions {
    /// Faster and rougher, for preview passes.
    pub fn preview() -> Self {
        EncodeOptions { crf: 26, preset: "veryfast".into(), ..Default::default() }
    }

    pub fn with_audio(mut self, tracks: Vec<AudioInput>) -> Self {
        self.audio = tracks;
        self
    }
}

/// How a GIF's 256-colour palette is chosen, and how the reduction to it is
/// hidden. GIF has no truecolour, so this is the whole quality story — a naive
/// conversion banks gradients and shimmers in a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifDither {
    /// No dithering: flat, hard colour banding. Only right for footage that is
    /// already near-flat (a few solid fields), where a dither pattern would be
    /// noise over nothing.
    None,
    /// Ordered (Bayer) dithering. The default, because it is **stable frame to
    /// frame** — the pattern is a fixed function of pixel position, so a static
    /// background does not crawl the way error-diffusion makes it crawl in a
    /// loop. `scale` is the pattern size, 1..=5; larger trades a coarser but
    /// less noticeable texture.
    Bayer { scale: u8 },
    /// Error-diffusion (`sierra2_4a`). Smoothest gradients in a *single* frame,
    /// but the diffused error moves as the image moves, so a looping GIF
    /// shimmers. Offered for one-frame-ish clips, not the default.
    Sierra2,
}

/// Which pixels the palette is optimised for. `Diff` biases the 256 colours
/// toward regions that change between frames — right for an animation over a
/// mostly-static background. `Full` weights every pixel equally — right when
/// the *whole* frame changes (a colour grade, a full-frame transition), where
/// biasing toward "what moved" would starve the parts that matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifStats {
    Full,
    Diff,
}

/// A GIF is a section of a film, palette-optimised, sized for a README or a
/// web page. Never the whole of a long film — a sixty-second GIF is both
/// enormous and useless — which is why the CLI takes a `--from`/`--to` window.
#[derive(Debug, Clone)]
pub struct GifOptions {
    /// Output width in pixels; the height follows the source aspect ratio.
    /// A README column is ~480px wide at most, so that is the default.
    pub width: u32,
    /// GIF frame rate. Deliberately low (a GIF is not video): 15fps reads as
    /// smooth for UI motion and keeps the file honest.
    pub fps: f64,
    pub dither: GifDither,
    pub stats: GifStats,
    /// libavformat's gif `loop`: 0 loops forever, -1 plays once. A README GIF
    /// wants to loop.
    pub loops: i32,
    /// Cap on palette size, 2..=256. Fewer colours, smaller file.
    pub max_colors: u16,
}

impl Default for GifOptions {
    fn default() -> Self {
        GifOptions {
            width: 480,
            fps: 15.0,
            // Ordered dithering, because the output loops. See `GifDither`.
            dither: GifDither::Bayer { scale: 3 },
            // Weight the whole frame by default: it is the safe choice for the
            // grade/transition demos this feature exists to show, and costs
            // an animation over a static background very little.
            stats: GifStats::Full,
            loops: 0,
            max_colors: 256,
        }
    }
}

impl GifOptions {
    /// The `-vf` filtergraph that turns the piped frames into a well-dithered,
    /// palette-optimised GIF in a single pass: decimate to the target rate,
    /// Lanczos-downscale to the target width, then split the stream so one copy
    /// generates the palette and the other is quantised against it.
    ///
    /// `diff_mode=rectangle` restricts each frame's repaint to the rectangle
    /// that actually changed — smaller files, and no dither churn in the parts
    /// that held still.
    fn filtergraph(&self) -> String {
        let stats = match self.stats {
            GifStats::Full => "full",
            GifStats::Diff => "diff",
        };
        let use_ = match self.dither {
            GifDither::None => "dither=none".to_string(),
            GifDither::Bayer { scale } => {
                format!("dither=bayer:bayer_scale={}", scale.clamp(1, 5))
            }
            GifDither::Sierra2 => "dither=sierra2_4a".to_string(),
        };
        format!(
            "fps={fps},scale={w}:-1:flags=lanczos,split[a][b];\
             [a]palettegen=max_colors={n}:stats_mode={stats}[p];\
             [b][p]paletteuse={use_}:diff_mode=rectangle",
            fps = self.fps,
            w = self.width,
            n = self.max_colors.clamp(2, 256),
        )
    }
}

/// An ffmpeg process being fed raw frames that writes an animated GIF.
///
/// Mirrors [`FfmpegSink`] exactly — same raw `rgb24`-on-stdin contract, same
/// `FrameSink` impl — differing only in the output filtergraph. The palette is
/// generated from *these* frames, not a fixed web palette, which is the whole
/// reason the result is watchable.
pub struct GifSink {
    child: Option<Child>,
    output: PathBuf,
    background: Color,
    frames: u64,
}

impl GifSink {
    pub fn new(
        output: impl AsRef<Path>,
        width: u32,
        height: u32,
        source_fps: f64,
        opts: &GifOptions,
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
            // The rate the frames arrive at; the filtergraph's `fps=` decimates
            // from here to the GIF's own rate.
            .args(["-r", &format!("{source_fps}")])
            .args(["-i", "-"])
            .args(["-vf", &opts.filtergraph()])
            .args(["-loop", &opts.loops.to_string()])
            .arg(&output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = cmd
            .spawn()
            .with_context(|| format!("starting ffmpeg to write {}", output.display()))?;
        Ok(GifSink { child: Some(child), output, background, frames: 0 })
    }

    pub fn output(&self) -> &Path {
        &self.output
    }

    pub fn frames_written(&self) -> u64 {
        self.frames
    }
}

impl FrameSink for GifSink {
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

impl Drop for GifSink {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            let _ = child.wait();
        }
    }
}

/// The mobile delivery cut.
#[derive(Debug, Clone)]
pub struct MobileOptions {
    pub width: u32,
    pub fps: u32,
    pub crf: u8,
    pub preset: String,
    /// Lower than the master's: this cut exists to be small enough to send.
    pub audio_bitrate: String,
}

impl Default for MobileOptions {
    fn default() -> Self {
        // 720 wide and 30fps: Telegram's sendVideo rejects 60fps outright, so
        // this is a delivery requirement rather than a size optimisation.
        MobileOptions {
            width: 720,
            fps: 30,
            crf: 23,
            preset: "medium".into(),
            audio_bitrate: "128k".into(),
        }
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
            .args(["-i", "-"]);
        // Every track is its own ffmpeg input, declared after the piped video,
        // so the video is input 0 and the tracks are 1..=n.
        for t in &opts.audio {
            cmd.arg("-i").arg(&t.path);
        }
        match mix_filter(&opts.audio, 1) {
            Some((filter, label)) => {
                cmd.args(["-filter_complex", &filter])
                    .args(["-map", "0:v"])
                    .args(["-map", &label])
                    .args(["-c:a", &opts.audio_codec])
                    .args(["-b:a", &opts.audio_bitrate])
                    // The mix is padded with silence, so the video is the
                    // finite side and `-shortest` means "as long as the film".
                    .arg("-shortest");
            }
            None => {
                cmd.args(["-an"]);
            }
        }
        cmd.args(["-c:v", &opts.codec])
            .args(["-crf", &opts.crf.to_string()])
            .args(["-preset", &opts.preset])
            .args(["-pix_fmt", &opts.pixel_format]);
        if let Some(threads) = opts.threads {
            cmd.args(["-threads", &threads.to_string()]);
        }
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

/// Finish a segmented, resumable render (`src/segments.rs`): remux a
/// sequence of same-codec-settings `.ts` segments into one file, mixing in
/// `opts.audio` in the same pass. `-c:v copy` — the segments already carry
/// the exact codec settings the caller wants, so this never re-encodes the
/// picture, only rewraps it (and, if there is audio, decodes/re-encodes
/// *that*, exactly as [`FfmpegSink`] would for a non-segmented render).
///
/// Segments are concatenated via ffmpeg's `concat:` protocol rather than the
/// concat *demuxer*: a straightforward stream-level splice for same-codec
/// `.ts` files, with none of standalone `.mp4` files' per-file
/// `moov`/edit-list metadata to cause timestamp discontinuities at the seams.
pub fn finish_segmented_render(segments: &[PathBuf], output: &Path, opts: &EncodeOptions) -> Result<()> {
    if segments.is_empty() {
        bail!("no segments to concatenate");
    }
    if let Some(dir) = output.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).ok();
    }
    let concat_input =
        format!("concat:{}", segments.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("|"));

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-nostdin").args(["-loglevel", "error", "-y"]);
    cmd.args(["-i", &concat_input]);
    // Every audio track is its own ffmpeg input, declared after the
    // concatenated video, so the video is input 0 and the tracks are 1..=n
    // — the same convention `FfmpegSink::new` uses.
    for t in &opts.audio {
        cmd.arg("-i").arg(&t.path);
    }
    match mix_filter(&opts.audio, 1) {
        Some((filter, label)) => {
            cmd.args(["-filter_complex", &filter])
                .args(["-map", "0:v"])
                .args(["-map", &label])
                .args(["-c:a", &opts.audio_codec])
                .args(["-b:a", &opts.audio_bitrate])
                .arg("-shortest");
        }
        None => {
            cmd.args(["-an"]);
        }
    }
    cmd.args(["-c:v", "copy"]);
    if opts.faststart {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.arg(output);

    let out = cmd.output().context("running ffmpeg to finish a segmented render")?;
    if !out.status.success() {
        bail!(
            "ffmpeg failed finishing segmented render {}: {}",
            output.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
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
        // Re-encoded rather than `-an` (which silenced this cut) and rather
        // than `-c:a copy`: the master's 192k is more than a phone speaker
        // needs, and a copy would carry it anyway. A silent master simply
        // produces a silent cut — ffmpeg maps no stream that is not there.
        .args(["-c:a", "aac"])
        .args(["-b:a", &opts.audio_bitrate])
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
    fn a_silent_film_takes_the_command_line_it_always_took() {
        // No tracks must mean `-an`, not an empty filter graph.
        assert!(mix_filter(&EncodeOptions::default().audio, 1).is_none());
    }

    #[test]
    fn the_mobile_cut_is_not_silent_by_default() {
        // Regression: this cut passed `-an`, so a film with sound reached the
        // phone silently while the master played fine.
        assert!(!MobileOptions::default().audio_bitrate.is_empty());
    }

    #[test]
    fn preview_options_trade_quality_for_speed() {
        assert!(EncodeOptions::preview().crf > EncodeOptions::default().crf);
    }

    #[test]
    fn a_gif_defaults_to_a_readme_sized_looping_clip() {
        let g = GifOptions::default();
        assert_eq!(g.width, 480);
        assert_eq!(g.loops, 0, "a README GIF must loop forever");
        // Ordered dithering, because the output loops and error-diffusion
        // would crawl. See `GifDither`.
        assert!(matches!(g.dither, GifDither::Bayer { .. }));
    }

    #[test]
    fn the_gif_filtergraph_generates_its_palette_from_the_footage() {
        let f = GifOptions::default().filtergraph();
        // Two-stage: build a palette from these frames, then quantise against
        // it — never a fixed web palette.
        assert!(f.contains("palettegen"), "must generate a palette: {f}");
        assert!(f.contains("paletteuse"), "must apply it: {f}");
        // Lanczos downscale and an fps decimation are both in the one pass.
        assert!(f.contains("scale=480:-1:flags=lanczos"), "{f}");
        assert!(f.contains("fps=15"), "{f}");
    }

    #[test]
    fn dither_and_palette_focus_reach_the_filtergraph() {
        let none = GifOptions { dither: GifDither::None, ..Default::default() };
        assert!(none.filtergraph().contains("dither=none"));
        let bayer = GifOptions { dither: GifDither::Bayer { scale: 4 }, ..Default::default() };
        assert!(bayer.filtergraph().contains("dither=bayer:bayer_scale=4"));
        let diff = GifOptions { stats: GifStats::Diff, ..Default::default() };
        assert!(diff.filtergraph().contains("stats_mode=diff"));
        let full = GifOptions { stats: GifStats::Full, ..Default::default() };
        assert!(full.filtergraph().contains("stats_mode=full"));
    }

    #[test]
    fn a_gif_palette_is_clamped_to_a_legal_range() {
        // 256 is the ceiling GIF allows; a bayer scale above 5 is meaningless.
        let over = GifOptions { max_colors: 999, dither: GifDither::Bayer { scale: 9 }, ..Default::default() };
        let f = over.filtergraph();
        assert!(f.contains("max_colors=256"), "{f}");
        assert!(f.contains("bayer_scale=5"), "{f}");
    }
}
