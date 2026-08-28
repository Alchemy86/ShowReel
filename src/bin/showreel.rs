//! The ShowReel command line.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use showreel::assets::AssetStore;
use showreel::audio::AudioInput;
use showreel::encode::{
    EncodeOptions, FfmpegSink, GifDither, GifOptions, GifSink, GifStats, MobileOptions,
    ffmpeg_available, mobile_cut, mobile_path,
};
use showreel::preview;
use showreel::render::{FrameSink, PngSequence, Renderer};
use showreel::scale::scale_film;
use showreel::text::FontDb;
use showreel::time::parse_time;
use showreel::timeline::Film;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "showreel",
    about = "Render declarative films: timeline, camera, typography, transitions.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a film to mp4, plus the mobile-safe cut.
    Render {
        film: PathBuf,
        /// Output mp4. Defaults to the film's name with an .mp4 extension.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Where to look for assets. Repeatable.
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
        /// Render at a fraction of the declared size.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        /// Only these frames, as `start-end` (inclusive) or `start-`.
        #[arg(long)]
        frames: Option<String>,
        /// Skip the 720p/30fps mobile cut.
        #[arg(long)]
        no_mobile: bool,
        /// Also write the frames as numbered PNGs into this directory.
        #[arg(long)]
        png: Option<PathBuf>,
        /// Quality/speed trade-off for the master (lower is better).
        #[arg(long, default_value_t = 17)]
        crf: u8,
    },
    /// Render a section of a film to a palette-optimised animated GIF, sized
    /// for a README or a web page. A GIF is always a *window* of a film, never
    /// the whole of a long one — pick the moment with `--from`/`--to`.
    Gif {
        film: PathBuf,
        /// Output GIF. Defaults to the film's name with a .gif extension.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Where to look for assets. Repeatable.
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
        /// Start of the window, e.g. `2s`, `500ms`. Defaults to the start.
        #[arg(long)]
        from: Option<String>,
        /// End of the window, e.g. `5s`. Defaults to the end of the film.
        #[arg(long)]
        to: Option<String>,
        /// Output width in pixels; the height follows the aspect ratio.
        #[arg(long, default_value_t = 480)]
        width: u32,
        /// GIF frame rate. A GIF is not video: keep it low.
        #[arg(long, default_value_t = 15.0)]
        fps: f64,
        /// Dithering: `bayer` (default, stable in a loop), `sierra2` (smoother
        /// gradients but shimmers when it loops), or `none` (hard banding).
        #[arg(long, default_value = "bayer")]
        dither: String,
        /// Bayer pattern size, 1-5 (only with `--dither bayer`).
        #[arg(long, default_value_t = 3)]
        bayer_scale: u8,
        /// Palette focus: `full` (whole frame — right for a grade or a
        /// full-frame transition) or `diff` (bias toward what moves — right
        /// for animation over a static background).
        #[arg(long, default_value = "full")]
        palette: String,
        /// Cap the palette (2-256). Fewer colours, smaller file.
        #[arg(long, default_value_t = 256)]
        colors: u16,
        /// Render the frames at this fraction of the film's declared size
        /// before the GIF downscale — the same knob `render` uses. Lower it
        /// for a large film so the source render stays cheap.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
    },
    /// Render a single frame to a PNG.
    Still {
        film: PathBuf,
        /// When, e.g. `4.2`, `4.2s`, `250ms`.
        #[arg(long, default_value = "0")]
        at: String,
        #[arg(short, long, default_value = "still.png")]
        out: PathBuf,
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
    },
    /// Render a labelled grid of thumbnails covering the whole film.
    Sheet {
        film: PathBuf,
        /// Sampling interval.
        #[arg(long, default_value = "1s")]
        every: String,
        #[arg(short, long, default_value = "sheet.png")]
        out: PathBuf,
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
        #[arg(long, default_value_t = 5)]
        columns: u32,
        #[arg(long, default_value_t = 384)]
        thumb: u32,
    },
    /// A fast, low-resolution pass over the whole timeline.
    Preview {
        film: PathBuf,
        #[arg(short, long, default_value = "preview.mp4")]
        out: PathBuf,
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
        #[arg(long, default_value_t = 0.35)]
        scale: f64,
    },
    /// Write a starter film: a two-scene example that needs no assets, ready
    /// to edit and render. The fastest way to have something working rather
    /// than a blank page.
    New {
        /// Where to write it. Defaults to `film.jsonc`.
        #[arg(default_value = "film.jsonc")]
        out: PathBuf,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value_t = 1920)]
        width: u32,
        #[arg(long, default_value_t = 1080)]
        height: u32,
        #[arg(long, default_value_t = 30.0)]
        fps: f64,
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Describe a film without rendering it.
    Info {
        film: PathBuf,
        /// Where to look for assets (charts' data files, plugin files).
        /// Repeatable.
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
    },
    /// Check a film for the mistakes a type cannot catch.
    Check {
        film: PathBuf,
        /// Where to look for assets (charts' data files, plugin files).
        /// Repeatable.
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
    },
    /// List the font families ShowReel can see.
    Fonts {
        /// Only families containing this text.
        filter: Option<String>,
    },
    /// Open a local browser studio: scrubber, timeline, live reload. Built
    /// with `--features studio`.
    #[cfg(feature = "studio")]
    Studio {
        film: PathBuf,
        /// Where to look for assets. Repeatable.
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
        #[arg(long, default_value_t = 7878)]
        port: u16,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Render preview frames at this fraction of the film's declared
        /// size — the same knob `showreel preview` uses.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
    },
    /// Run an MCP server over stdio: build, render, still and inspect a
    /// film through typed tool calls instead of shelling out to this CLI
    /// and parsing its output. Built with `--features mcp`.
    #[cfg(feature = "mcp")]
    Mcp,
    /// Package a film to run in a browser with no server: the wasm build
    /// (`build-wasm.sh`) plus this film's assets, clips pre-decoded (needs
    /// ffmpeg — see `src/wasm.rs`). Built with `--features wasm`.
    #[cfg(feature = "wasm")]
    WebPack {
        film: PathBuf,
        /// Where to look for assets. Repeatable.
        #[arg(short = 'A', long = "assets")]
        asset_roots: Vec<PathBuf>,
        #[arg(short, long, default_value = "dist")]
        out: PathBuf,
        /// Package at a fraction of the film's declared size — smaller
        /// frames and, since a clip's decoded size follows the frame,
        /// smaller downloads.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        /// JPEG quality (1-100) for pre-decoded clip frames.
        #[arg(long, default_value_t = 82)]
        clip_quality: u8,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Render { film, out, asset_roots, scale, frames, no_mobile, png, crf } => {
            cmd_render(film, out, asset_roots, scale, frames, no_mobile, png, crf)
        }
        Command::Gif {
            film,
            out,
            asset_roots,
            from,
            to,
            width,
            fps,
            dither,
            bayer_scale,
            palette,
            colors,
            scale,
        } => cmd_gif(
            film, out, asset_roots, from, to, width, fps, dither, bayer_scale, palette, colors,
            scale,
        ),
        Command::Still { film, at, out, asset_roots, scale } => {
            cmd_still(film, at, out, asset_roots, scale)
        }
        Command::Sheet { film, every, out, asset_roots, columns, thumb } => {
            cmd_sheet(film, every, out, asset_roots, columns, thumb)
        }
        Command::Preview { film, out, asset_roots, scale } => {
            cmd_render(film, Some(out), asset_roots, scale, None, true, None, 26)
        }
        Command::New { out, title, width, height, fps, force } => {
            cmd_new(out, title, width, height, fps, force)
        }
        Command::Info { film, asset_roots } => cmd_info(film, asset_roots),
        Command::Check { film, asset_roots } => cmd_check(film, asset_roots),
        Command::Fonts { filter } => cmd_fonts(filter),
        #[cfg(feature = "studio")]
        Command::Studio { film, asset_roots, port, host, scale } => {
            cmd_studio(film, asset_roots, port, host, scale)
        }
        #[cfg(feature = "mcp")]
        Command::Mcp => cmd_mcp(),
        #[cfg(feature = "wasm")]
        Command::WebPack { film, asset_roots, out, scale, clip_quality } => {
            cmd_web_pack(film, asset_roots, out, scale, clip_quality)
        }
    }
}

/// A store that searches the film's own directory first, then anything the
/// caller named. Putting the film's directory in by default is what makes a
/// film file portable: it can refer to `map.png` beside itself.
fn store(film_path: &Path, roots: &[PathBuf]) -> AssetStore {
    let mut s = AssetStore::new();
    if let Some(dir) = film_path.parent()
        && !dir.as_os_str().is_empty()
    {
        s.add_root(dir);
    }
    for r in roots {
        s.add_root(r);
    }
    s
}

fn load(path: &Path) -> Result<Film> {
    let film = Film::load(path)?;
    let errs = film.validate();
    if !errs.is_empty() {
        for e in &errs {
            eprintln!("error: {e}");
        }
        bail!("{} in {}", showreel::timeline::describe_problem_count(errs.len()), path.display());
    }
    Ok(film)
}

/// Locate every audio track and pin its timings against the film's length —
/// resolving a file track to a path and synthesising a music track to a WAV,
/// both through the one seam [`Film::resolve_audio_tracks`].
///
/// Done up front, before a single frame is rendered: a mistyped track name
/// should fail in the first second, not after a ten-minute render followed by
/// an ffmpeg error nobody can read.
fn resolve_audio(film: &Film, assets: &AssetStore) -> Result<Vec<AudioInput>> {
    film.resolve_audio_tracks(assets)
}

fn parse_frames(spec: &str, total: u32) -> Result<std::ops::Range<u32>> {
    let (a, b) = spec.split_once('-').unwrap_or((spec, spec));
    let start: u32 = a.trim().parse().unwrap_or(0);
    let end: u32 = if b.trim().is_empty() { total } else { b.trim().parse::<u32>()? + 1 };
    if start >= end {
        bail!("--frames {spec}: empty range");
    }
    Ok(start..end.min(total))
}

#[allow(clippy::too_many_arguments)]
fn cmd_render(
    film_path: PathBuf,
    out: Option<PathBuf>,
    roots: Vec<PathBuf>,
    scale: f64,
    frames: Option<String>,
    no_mobile: bool,
    png: Option<PathBuf>,
    crf: u8,
) -> Result<()> {
    if !ffmpeg_available() {
        bail!("ffmpeg is not on PATH; ShowReel needs it to encode");
    }
    let assets = store(&film_path, &roots);
    let declared = load(&film_path)?.expand_plugins(&assets)?;
    let film = scale_film(&declared, scale);
    let out = out.unwrap_or_else(|| film_path.with_extension("mp4"));
    let fonts = FontDb::shared();
    let renderer = Renderer::new(&film, &assets, fonts);

    let total = renderer.frame_count();
    let range = match &frames {
        Some(s) => parse_frames(s, total)?,
        None => 0..total,
    };

    println!(
        "{}: {} scenes, {:.2}s, {}x{} at {}fps — {} frames",
        film_path.display(),
        film.timeline.scene_count(),
        film.duration().as_secs(),
        film.width,
        film.height,
        film.fps,
        range.end - range.start
    );

    let mut tracks = resolve_audio(&film, &assets)?;
    let clip_tracks = film.clip_audio(&assets).context("resolving a clip's own audio")?;
    tracks.extend(clip_tracks);
    for t in &tracks {
        println!(
            "  sound   {} — {:.2}s..{:.2}s of the film, from {:.2}s in, fade {}s/{}s",
            t.path.display(),
            t.at,
            t.at + t.duration,
            t.from,
            t.fade_in,
            t.fade_out
        );
    }

    let opts = EncodeOptions { crf, ..EncodeOptions::default() }.with_audio(tracks);
    let mut encoder =
        FfmpegSink::new(&out, film.width, film.height, film.fps, &opts, film.background)?;
    let stats = match png {
        None => renderer.render_range(range, &mut encoder)?,
        Some(dir) => {
            let mut pngs = PngSequence::new(&dir, "frame")?;
            let mut tee = Tee { a: &mut encoder, b: &mut pngs };
            let s = renderer.render_range(range, &mut tee)?;
            println!("  frames  {}", dir.display());
            s
        }
    };

    println!("  {stats}");
    println!("  master  {}", out.display());
    if !no_mobile {
        let mob = mobile_path(&out);
        mobile_cut(&out, &mob, &MobileOptions::default())
            .context("producing the mobile cut")?;
        println!("  mobile  {} (720w, 30fps, +faststart)", mob.display());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_gif(
    film_path: PathBuf,
    out: Option<PathBuf>,
    roots: Vec<PathBuf>,
    from: Option<String>,
    to: Option<String>,
    width: u32,
    fps: f64,
    dither: String,
    bayer_scale: u8,
    palette: String,
    colors: u16,
    scale: f64,
) -> Result<()> {
    if !ffmpeg_available() {
        bail!("ffmpeg is not on PATH; ShowReel needs it to encode a GIF");
    }
    let dither = match dither.as_str() {
        "bayer" => GifDither::Bayer { scale: bayer_scale },
        "sierra2" => GifDither::Sierra2,
        "none" => GifDither::None,
        other => bail!("--dither {other:?}: expected one of bayer, sierra2, none"),
    };
    let stats = match palette.as_str() {
        "full" => GifStats::Full,
        "diff" => GifStats::Diff,
        other => bail!("--palette {other:?}: expected full or diff"),
    };

    let assets = store(&film_path, &roots);
    let declared = load(&film_path)?.expand_plugins(&assets)?;
    let film = scale_film(&declared, scale);
    let out = out.unwrap_or_else(|| film_path.with_extension("gif"));

    let fonts = FontDb::shared();
    let renderer = Renderer::new(&film, &assets, fonts);
    let total = renderer.frame_count();

    // A GIF is a window of the film. `--from`/`--to` are in the film's own
    // authoring unit — seconds — resolved to frames once, the same way the
    // timeline resolves every other second-valued knob.
    let start_frame = match &from {
        Some(s) => {
            let t = parse_time(s).with_context(|| format!("cannot read --from {s:?}"))?;
            (t.as_secs() * film.fps).round() as u32
        }
        None => 0,
    };
    let end_frame = match &to {
        Some(s) => {
            let t = parse_time(s).with_context(|| format!("cannot read --to {s:?}"))?;
            (t.as_secs() * film.fps).round() as u32
        }
        None => total,
    };
    let range = start_frame.min(total)..end_frame.min(total);
    if range.is_empty() {
        bail!("empty window: --from is not before --to (0 frames)");
    }

    let opts = GifOptions {
        width,
        fps,
        dither,
        stats,
        loops: 0,
        max_colors: colors,
    };
    println!(
        "{}: {:.2}s..{:.2}s of {:.2}s — {} source frames -> {}px-wide GIF at {}fps",
        film_path.display(),
        range.start as f64 / film.fps,
        range.end as f64 / film.fps,
        film.duration().as_secs(),
        range.end - range.start,
        width,
        fps,
    );

    let mut sink = GifSink::new(&out, film.width, film.height, film.fps, &opts, film.background)?;
    let stats = renderer.render_range(range, &mut sink)?;

    let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    println!("  {stats}");
    println!("  gif     {} ({})", out.display(), human_bytes(bytes));
    Ok(())
}

/// A file size a person can read at a glance.
fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// Feeds every frame to two sinks — the encoder and a PNG sequence.
struct Tee<'a> {
    a: &'a mut dyn FrameSink,
    b: &'a mut dyn FrameSink,
}

impl FrameSink for Tee<'_> {
    fn accept(&mut self, index: u32, canvas: &showreel::canvas::Canvas) -> Result<()> {
        self.a.accept(index, canvas)?;
        self.b.accept(index, canvas)
    }
    fn finish(&mut self) -> Result<()> {
        self.a.finish()?;
        self.b.finish()
    }
}

fn cmd_still(film_path: PathBuf, at: String, out: PathBuf, roots: Vec<PathBuf>, scale: f64) -> Result<()> {
    let t = parse_time(&at).with_context(|| format!("cannot read --at {at:?}"))?;
    let assets = store(&film_path, &roots);
    let film = scale_film(&load(&film_path)?.expand_plugins(&assets)?, scale);
    let started = std::time::Instant::now();
    let c = preview::still_at(&film, &assets, FontDb::shared(), t)?;
    c.save_png(&out)?;
    println!(
        "{} at {} — {}x{} in {:.0}ms",
        out.display(),
        preview::timecode(t.as_secs(), film.fps),
        c.width(),
        c.height(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

fn cmd_sheet(
    film_path: PathBuf,
    every: String,
    out: PathBuf,
    roots: Vec<PathBuf>,
    columns: u32,
    thumb: u32,
) -> Result<()> {
    let step = parse_time(&every).with_context(|| format!("cannot read --every {every:?}"))?;
    let assets = store(&film_path, &roots);
    let film = load(&film_path)?.expand_plugins(&assets)?;
    let started = std::time::Instant::now();
    let sheet = preview::contact_sheet(&film, &assets, FontDb::shared(), step, columns, thumb)?;
    sheet.save_png(&out)?;
    println!(
        "{} — {}x{}, every {}s of {:.2}s, in {:.2}s",
        out.display(),
        sheet.width(),
        sheet.height(),
        step.as_secs(),
        film.duration().as_secs(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cmd_info(film_path: PathBuf, roots: Vec<PathBuf>) -> Result<()> {
    let loaded = Film::load(&film_path)?;
    // Describe the film as it will render — plugins expanded to real layers.
    let assets = store(&film_path, &roots);
    let film = loaded.expand_plugins(&assets)?;
    println!("{}", film.title.clone().unwrap_or_else(|| film_path.display().to_string()));
    println!(
        "  {}x{} at {}fps, {:.2}s, {} frames",
        film.width,
        film.height,
        film.fps,
        film.duration().as_secs(),
        film.frame_count()
    );
    println!("  scenes:");
    for p in film.timeline.placements() {
        let s = film.timeline.scene(p.index);
        let name = s.name.clone().unwrap_or_else(|| format!("scene {}", p.index + 1));
        let via = film
            .timeline
            .transition_into(p.index)
            .map(|t| format!(" (via {:?} over {}s)", presentation_name(t), t.duration.as_secs()))
            .unwrap_or_default();
        println!(
            "    {:>7.2}s  {:<28} {:>5.2}s  {} layer(s){}",
            p.start.as_secs(),
            name,
            s.duration.as_secs(),
            s.layers.len(),
            via
        );
    }
    if !film.audio.is_empty() {
        println!("  sound:");
        let total = film.duration();
        for a in &film.audio {
            let d = a.resolve_duration(total).as_secs();
            let label = if a.is_music() {
                format!("{} ({} bpm{})", a.source_label(), a.music.as_ref().unwrap().effective_bpm(d).round(),
                    if a.music.as_ref().unwrap().fit == showreel::music::MusicFit::Film { ", fit" } else { "" })
            } else {
                a.source_label()
            };
            println!(
                "    {:>7.2}s  {:<28} {:>5.2}s  from {:.2}s, fade {}s/{}s{}",
                a.at.as_secs(),
                label,
                d,
                a.from.as_secs(),
                a.fade_in.as_secs(),
                a.fade_out.as_secs(),
                if (a.gain - 1.0).abs() < 1e-9 { String::new() } else { format!(", gain {}", a.gain) }
            );
        }
    }
    let mut clip_audio_lines = Vec::new();
    for p in film.timeline.placements() {
        let scene = film.timeline.scene(p.index);
        for l in &scene.layers {
            let showreel::layer::Content::Clip { asset, audio, .. } = &l.content else { continue };
            let span = l.span(scene.duration);
            let at = (p.start + span.start).as_secs();
            let note = if audio.muted {
                "muted".to_string()
            } else {
                let gain = if (audio.gain - 1.0).abs() < 1e-9 {
                    String::new()
                } else {
                    format!(", gain {}", audio.gain)
                };
                format!(
                    "{:.2}s, fade {}s/{}s{gain}",
                    span.duration.as_secs(),
                    audio.fade_in.as_secs(),
                    audio.fade_out.as_secs()
                )
            };
            clip_audio_lines.push(format!("    {at:>7.2}s  {asset:<28} {note}"));
        }
    }
    if !clip_audio_lines.is_empty() {
        println!("  clip audio:");
        for l in clip_audio_lines {
            println!("{l}");
        }
    }
    let used = film.assets_used();
    if !used.is_empty() {
        println!("  assets:");
        for u in used {
            match u {
                showreel::timeline::AssetUse::Still(a) => println!("    still  {a}"),
                showreel::timeline::AssetUse::Clip { asset, max_width, trim, decode_fps } => {
                    let t = trim
                        .map(|(a, d)| format!(", {a}s..{:.2}s", a + d))
                        .unwrap_or_else(|| ", whole file".into());
                    let f = decode_fps.map(|f| format!(", {f}fps")).unwrap_or_default();
                    println!("    clip   {asset} ({max_width}px wide{t}{f})");
                }
                showreel::timeline::AssetUse::Data(file) => println!("    data   {file}"),
            }
        }
    }
    let errs = film.validate();
    if errs.is_empty() {
        println!("  valid");
    } else {
        for e in errs {
            println!("  PROBLEM: {e}");
        }
    }
    Ok(())
}

fn presentation_name(t: &showreel::transition::Transition) -> String {
    use showreel::transition::Presentation::*;
    match &t.presentation {
        Cut => "cut",
        Dissolve => "dissolve",
        Fade { .. } => "fade",
        Wipe { .. } => "wipe",
        Slide { .. } => "slide",
        Push { .. } => "push",
        Iris { .. } => "iris",
        ZoomIn { .. } => "zoom-in",
        CrossBlur { .. } => "cross-blur",
    }
    .to_string()
}

/// A two-scene, asset-free starter film, as JSONC — commented so a first-time
/// reader can see the shape of the format (`opening`/`then`, a layer's `type`
/// tag) without cross-referencing the docs. Every value here is something
/// `showreel render` can turn into a video with no `-A` at all: solid and
/// gradient backgrounds, a title, a lower-third, a counter and plain text.
/// `examples/kanto.film.jsonc` is the fuller worked example, once this one
/// stops being a blank page.
fn starter_jsonc(title: &str, width: u32, height: u32, fps: f64) -> String {
    // JSON-escaped (and already quoted) so a title with a `"` or a backslash
    // in it — plausible for anything typed on a command line — still lands in
    // valid JSON rather than corrupting the file it is interpolated into.
    let title = serde_json::to_string(title).unwrap();
    format!(
        r##"// A ShowReel film. This is JSONC: `//` comments and a trailing comma on the
// last item of a list are both fine — see `AGENTS.md` if you want to know why
// only those two, and not the rest of JSON5.
//
// The shape: one required opening scene, then any number of (transition,
// scene) pairs. Every layer needs a "type" — see `examples/kanto.film.jsonc`
// for the fuller vocabulary (a camera move over a still, video clips, sound).
//
// Try it now:
//   showreel check film.jsonc
//   showreel still film.jsonc --at 2s -o still.png
//   showreel sheet film.jsonc -o sheet.png
//   showreel render film.jsonc -o out.mp4
{{
  "width": {width},
  "height": {height},
  "fps": {fps},
  "title": {title},

  "opening": {{
    "duration": 4.0,
    "layers": [
      {{ "type": "gradient", "stops": [[0.0, "#1c2438"], [1.0, "#07090e"]], "angle": 110.0 }},
      {{
        "type": "title",
        "text": {title},
        "subtitle": "describe a film and render it",
        // Every character arrives in turn — see `src/motion.rs` for the rest
        // of the entrance vocabulary (fade, rise, slide, scale, words...).
        "enter": {{ "kind": "chars", "stagger": 0.03, "rise": 26.0, "duration": 0.6 }}
      }}
    ]
  }},

  "then": [
    {{
      "transition": {{ "duration": 0.8, "presentation": {{ "kind": "dissolve" }} }},
      "scene": {{
        "duration": 5.0,
        "layers": [
          {{ "type": "solid", "colour": "#0d1016" }},
          {{
            "type": "lower-third",
            "text": "Made with ShowReel",
            "detail": "edit this file to make it yours",
            "from": 0.3
          }},
          {{
            "type": "counter",
            "count": {{ "from": 0.0, "to": 100.0, "over": 2.0 }},
            "label": "% of the way to a real film",
            "from": 0.6
          }},
          {{
            "type": "text",
            "text": "A film is a value: a Rust builder or this JSON,\nthe same tree either way.",
            // A box in fractions of the frame — the placement to reach for,
            // since it survives a resolution change. See `src/layer.rs`'s
            // `Placement` for the other kinds (an anchor, an exact rect).
            // Kept clear of the counter (top right) and the lower-third
            // (bottom left, below).
            "placement": {{ "fx": 0.1, "fy": 0.32, "fw": 0.6, "fh": 0.2 }},
            "from": 1.4
          }}
        ]
      }}
    }}
  ]
}}
"##
    )
}

fn cmd_new(
    out: PathBuf,
    title: Option<String>,
    width: u32,
    height: u32,
    fps: f64,
    force: bool,
) -> Result<()> {
    if out.exists() && !force {
        bail!("{} already exists; pass --force to overwrite", out.display());
    }
    let title = title.unwrap_or_else(|| "A New Film".to_string());
    let jsonc = starter_jsonc(&title, width, height, fps);
    // Fail loudly here rather than write something `showreel check` would
    // then also reject — a scaffold that does not itself validate is worse
    // than no scaffold.
    let film = Film::from_json(&jsonc).context("the starter template failed to parse")?;
    let errs = film.validate();
    if !errs.is_empty() {
        for e in &errs {
            eprintln!("error: {e}");
        }
        bail!("the starter template does not validate — this is a bug in `showreel new` itself");
    }
    if let Some(dir) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, jsonc)?;
    println!("{} — {:.1}s, {width}x{height} at {fps}fps, no assets needed", out.display(), film.duration().as_secs());
    println!("  showreel still {} --at 2s -o still.png", out.display());
    println!("  showreel render {} -o out.mp4", out.display());
    Ok(())
}

fn cmd_check(film_path: PathBuf, roots: Vec<PathBuf>) -> Result<()> {
    let film = Film::load(&film_path)?;
    let assets = store(&film_path, &roots);
    // Plugins first: a `custom` layer becomes real layers, and its own errors
    // (unknown plugin, missing parameter) surface before validation runs over
    // the expanded film.
    let film = film.expand_plugins(&assets)?;
    let errs = film.validate();
    if !errs.is_empty() {
        for e in &errs {
            eprintln!("error: {e}");
        }
        bail!("{}", showreel::timeline::describe_problem_count(errs.len()));
    }
    // Then the check a type cannot make: that every chart's external data file
    // resolves, has the named columns, and parses.
    film.resolve_chart_data(&assets)?;
    println!("{}: ok", film_path.display());
    Ok(())
}

#[cfg(feature = "studio")]
fn cmd_studio(
    film_path: PathBuf,
    roots: Vec<PathBuf>,
    port: u16,
    host: String,
    scale: f64,
) -> Result<()> {
    if !film_path.exists() {
        bail!("{} does not exist", film_path.display());
    }
    showreel::studio::serve(film_path, roots, showreel::studio::StudioOptions { port, host, scale })
}

#[cfg(feature = "mcp")]
fn cmd_mcp() -> Result<()> {
    // `enable_time` is not optional: rmcp uses a timer internally (request
    // timeouts, shutdown draining), and omitting it panics on the first path
    // that needs one rather than failing at startup.
    tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .context("starting the MCP server's async runtime")?
        .block_on(showreel::mcp::serve())
}

/// Where a clip's `.srclip` container lands under `assets/`, given the exact
/// parameters it was decoded with. A bare `{asset}.srclip` would collide the
/// moment the same source file is used twice at two different trims — kanto's
/// own film does this six times over `pixel-chain-run.mp4` — so the decode
/// parameters that make a clip's frames what they are go in the name too.
/// `tools/web/index.html`'s `clipAssetPath` builds this exact same string
/// from the same fields (all present in `sr_assets_needed_ptr`'s manifest),
/// so the two must be kept in lock step.
#[cfg(feature = "wasm")]
fn clip_srclip_name(asset: &str, max_width: u32, fps: f64, trim: Option<(f64, f64)>) -> String {
    match trim {
        Some((start, dur)) => format!("{asset}@{max_width}x{fps:.3}_{start:.3}-{dur:.3}.srclip"),
        None => format!("{asset}@{max_width}x{fps:.3}_full.srclip"),
    }
}

/// Render one [`AudioInput`]'s trimmed/gained/faded window to its own small
/// file — the audio counterpart of a `.srclip`: ffmpeg (unavailable in the
/// browser) runs once, natively, here, and the browser only ever plays back
/// what this wrote. Positioning (`AudioInput::at`) is deliberately not baked
/// in — see [`AudioInput::export_filter`] — the browser scheduler places it,
/// the same way `.srclip`'s frames carry no notion of where the clip sits on
/// the film's clock.
#[cfg(feature = "wasm")]
fn extract_audio_window(input: &AudioInput, dest: &Path) -> Result<()> {
    use std::process::Command;
    let out = Command::new("ffmpeg")
        .arg("-nostdin")
        .args(["-loglevel", "error", "-y"])
        .arg("-i")
        .arg(&input.path)
        .args(["-filter_complex", &input.export_filter(0)])
        .args(["-map", "[a]"])
        .args(["-c:a", "libmp3lame", "-q:a", "4"])
        .arg(dest)
        .output()
        .with_context(|| format!("running ffmpeg to extract audio from {}", input.path.display()))?;
    if !out.status.success() {
        bail!(
            "ffmpeg failed extracting audio from {}: {}",
            input.path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Prepare a self-contained directory a static file host can serve as the
/// whole shareable page: the wasm build (already produced by
/// `build-wasm.sh`), the bundled fonts, and this film's own assets — stills
/// copied as-is, clips pre-decoded through the same ffmpeg-backed path
/// `showreel render` uses and packed as `.srclip` (`showreel::webclip`),
/// since ffmpeg itself cannot go to the browser (see `src/wasm.rs`).
#[cfg(feature = "wasm")]
fn cmd_web_pack(
    film_path: PathBuf,
    roots: Vec<PathBuf>,
    out: PathBuf,
    scale: f64,
    clip_quality: u8,
) -> Result<()> {
    use showreel::timeline::AssetUse;

    let declared = load(&film_path)?;
    // Scaled first: a clip's `max_width` scales with the frame (see
    // src/scale.rs), and that is the exact key the wasm build's
    // `AssetStore::clip` will look the decode up by, so packing must use the
    // same scaled values or the browser asks for a clip nobody packed.
    let film = scale_film(&declared, scale);
    let assets = store(&film_path, &roots);

    let web_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/web");
    let wasm_src = web_root.join("showreel.wasm");
    if !wasm_src.exists() {
        bail!("{} does not exist — run ./build-wasm.sh first", wasm_src.display());
    }

    std::fs::create_dir_all(out.join("assets"))?;
    std::fs::create_dir_all(out.join("fonts"))?;
    std::fs::write(out.join("film.json"), film.to_json()?)?;
    std::fs::copy(&wasm_src, out.join("showreel.wasm"))?;
    std::fs::copy(web_root.join("index.html"), out.join("index.html"))?;
    // The editor's JS modules — everything `index.html`'s `<script type="module">`
    // imports (see that file and each module's own doc comment). Named
    // explicitly, the same way the font loop below skips `README.md`, so a
    // stray dev-only file (`test-muxer.mjs`, Node-only) never ships.
    for module in [
        "main.js",
        "editor.js",
        "bridge.js",
        "muxer.js",
        "srclip.js",
        "clipimport.js",
        "export.js",
        "audio.js",
        "geometry.js",
        "thumbnails.js",
    ] {
        std::fs::copy(web_root.join(module), out.join(module))
            .with_context(|| format!("copying {module}"))?;
    }
    for entry in std::fs::read_dir(web_root.join("fonts"))? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            std::fs::copy(&path, out.join("fonts").join(path.file_name().unwrap()))?;
        }
    }

    println!("{}: packaging for the browser at {out:?}, scale {scale}", film_path.display());
    let mut asset_bytes: u64 = 0;
    for u in film.assets_used() {
        match u {
            AssetUse::Still(name) => {
                let bytes = std::fs::read(assets.resolve(&name)?)?;
                let dest = out.join("assets").join(&name);
                if let Some(dir) = dest.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&dest, &bytes)?;
                asset_bytes += bytes.len() as u64;
                println!("  still  {name}  {:.0} KB", bytes.len() as f64 / 1024.0);
            }
            AssetUse::Clip { asset, max_width, trim, decode_fps } => {
                let fps = decode_fps.unwrap_or(film.fps);
                let clip = assets.clip(&asset, fps, max_width, trim)?;
                let packed = showreel::webclip::encode(clip.as_ref(), clip_quality)
                    .with_context(|| format!("packing {asset}"))?;
                let dest = out.join("assets").join(clip_srclip_name(&asset, max_width, fps, trim));
                if let Some(dir) = dest.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&dest, &packed)?;
                asset_bytes += packed.len() as u64;
                println!(
                    "  clip   {asset}  {} frames at {max_width}px, {:.0} KB",
                    clip.frame_count(),
                    packed.len() as f64 / 1024.0
                );
            }
            AssetUse::Data(name) => {
                // Ship the raw CSV/JSON alongside the film, like a still. The
                // browser build does not yet register a chart's data (there is
                // no `insert_data` call wired through bridge.js), so a packaged
                // chart that reads external data is a known gap — see AGENTS.md;
                // copying the file keeps the package complete for when it is.
                let bytes = std::fs::read(assets.resolve(&name)?)?;
                let dest = out.join("assets").join(&name);
                if let Some(dir) = dest.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&dest, &bytes)?;
                asset_bytes += bytes.len() as u64;
                println!("  data   {name}  {:.0} KB", bytes.len() as f64 / 1024.0);
            }
        }
    }

    // Film-level audio tracks (`Film::audio`, e.g. music) ship as the raw
    // source file, copied as-is like a still — the browser decodes it itself
    // (`AudioContext.decodeAudioData`) and applies `at`/`from`/gain/fades
    // live from the film JSON it already has, so editing a track's timing in
    // the browser is heard immediately, no repackage needed.
    let mut audio_seen = std::collections::HashSet::new();
    for name in film.audio_assets() {
        if !audio_seen.insert(name.to_string()) {
            continue;
        }
        let bytes = std::fs::read(assets.resolve(name)?)?;
        let dest = out.join("assets").join(name);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&dest, &bytes)?;
        asset_bytes += bytes.len() as u64;
        println!("  audio  {name}  {:.0} KB", bytes.len() as f64 / 1024.0);
    }

    // Generated-music tracks have no source file to copy: synthesise the WAV
    // natively here (the DSP is pure Rust — no ffmpeg, so `web-pack` can do it
    // ahead of a browser that has neither ffmpeg nor a filesystem) and ship it
    // like any other film-level track, listed in `music.json`. This is a
    // snapshot, the same as a `.srclip`: editing a music track's mood/key/bpm
    // in the browser needs a repackage to be heard, though its
    // at/gain/fades are read live from `music.json` the same way a file track's
    // are from the film JSON. See `src/music.rs` for the wasm story.
    let music_total = film.duration();
    let mut music_manifest = Vec::new();
    for (i, a) in film.audio.iter().enumerate() {
        let Some(music) = &a.music else { continue };
        let dur = a.resolve_duration(music_total).as_secs();
        let bytes = music.wav_bytes(dur);
        let name = format!("music-{i}.wav");
        let dest = out.join("assets").join(&name);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&dest, &bytes)?;
        asset_bytes += bytes.len() as u64;
        println!(
            "  music  {name}  {} ({} bpm), {:.0} KB",
            music.mood.name(),
            music.effective_bpm(dur).round(),
            bytes.len() as f64 / 1024.0
        );
        music_manifest.push(serde_json::json!({
            "file": format!("assets/{name}"),
            "at": a.at.as_secs(),
            "from": 0.0,
            "duration": dur,
            "fade_in": a.fade_in.as_secs(),
            "fade_out": a.fade_out.as_secs(),
            "gain": a.gain,
        }));
    }
    std::fs::write(out.join("music.json"), serde_json::to_string(&music_manifest)?)?;

    // A clip's own soundtrack is baked into its source video, which the
    // browser never has (only the pre-decoded `.srclip` frames — see
    // `src/wasm.rs`'s module doc). Extract exactly the window each clip layer
    // actually draws for, gain/fades already applied (`Film::clip_audio`
    // already resolved those from each layer's `ClipAudio`), as its own
    // small file; `at`/`duration` are recorded so the browser can position
    // it without recomputing scene/layer timing in JS. Unlike the live
    // film-level tracks above, this is a snapshot: editing a clip's `audio`
    // settings or trim in the browser needs a repackage to be heard, the
    // same staleness a `.srclip`'s own trim already carries.
    let mut clip_audio_manifest = Vec::new();
    for (i, input) in film.clip_audio(&assets)?.iter().enumerate() {
        let name = format!("clip-audio-{i}.mp3");
        let dest = out.join("assets").join(&name);
        extract_audio_window(input, &dest)?;
        let bytes = std::fs::metadata(&dest)?.len();
        asset_bytes += bytes;
        println!(
            "  clip audio  {} — {:.2}s..{:.2}s, {:.0} KB",
            input.path.display(),
            input.at,
            input.at + input.duration,
            bytes as f64 / 1024.0
        );
        clip_audio_manifest.push(serde_json::json!({
            "file": format!("assets/{name}"),
            "at": input.at,
            "duration": input.duration,
        }));
    }
    std::fs::write(out.join("clip-audio.json"), serde_json::to_string(&clip_audio_manifest)?)?;

    let wasm_bytes = std::fs::metadata(&wasm_src)?.len();
    let font_bytes: u64 = std::fs::read_dir(out.join("fonts"))?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    let total = wasm_bytes + font_bytes + asset_bytes;
    println!(
        "  {}  —  {:.1} MB total ({:.1} MB wasm, {:.1} MB fonts, {:.1} MB assets)",
        out.display(),
        total as f64 / 1e6,
        wasm_bytes as f64 / 1e6,
        font_bytes as f64 / 1e6,
        asset_bytes as f64 / 1e6,
    );
    Ok(())
}

fn cmd_fonts(filter: Option<String>) -> Result<()> {
    let db = FontDb::shared();
    let mut names: Vec<String> = (0..db.len())
        .map(|i| db.family_name(showreel::text::FontId(i)).to_string())
        .collect();
    names.sort();
    names.dedup();
    let f = filter.map(|s| s.to_lowercase());
    let mut shown = 0;
    for n in &names {
        if f.as_ref().is_none_or(|f| n.to_lowercase().contains(f)) {
            println!("{n}");
            shown += 1;
        }
    }
    eprintln!("{shown} of {} families", names.len());
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Every value `showreel new` might plausibly be asked for — including a
    /// title with the exact characters that would break naive JSON string
    /// interpolation — must still produce a film that parses and validates.
    #[test]
    fn the_starter_template_always_parses_and_validates() {
        for (title, w, h, fps) in [
            ("A New Film", 1920, 1080, 30.0),
            ("Quotes \"and\" \\backslashes\\", 640, 360, 24.0),
            ("", 320, 180, 60.0),
        ] {
            let jsonc = starter_jsonc(title, w, h, fps);
            let film = Film::from_json(&jsonc)
                .unwrap_or_else(|e| panic!("starter for {title:?} failed to parse: {e}\n{jsonc}"));
            let errs = film.validate();
            assert!(errs.is_empty(), "starter for {title:?}: {errs:?}");
            assert_eq!((film.width, film.height, film.fps), (w, h, fps));
        }
    }
}
