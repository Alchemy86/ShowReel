//! The ShowReel command line.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use showreel::assets::AssetStore;
use showreel::encode::{EncodeOptions, FfmpegSink, MobileOptions, ffmpeg_available, mobile_cut, mobile_path};
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
    /// Describe a film without rendering it.
    Info { film: PathBuf },
    /// Check a film for the mistakes a type cannot catch.
    Check { film: PathBuf },
    /// List the font families ShowReel can see.
    Fonts {
        /// Only families containing this text.
        filter: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Render { film, out, asset_roots, scale, frames, no_mobile, png, crf } => {
            cmd_render(film, out, asset_roots, scale, frames, no_mobile, png, crf)
        }
        Command::Still { film, at, out, asset_roots, scale } => {
            cmd_still(film, at, out, asset_roots, scale)
        }
        Command::Sheet { film, every, out, asset_roots, columns, thumb } => {
            cmd_sheet(film, every, out, asset_roots, columns, thumb)
        }
        Command::Preview { film, out, asset_roots, scale } => {
            cmd_render(film, Some(out), asset_roots, scale, None, true, None, 26)
        }
        Command::Info { film } => cmd_info(film),
        Command::Check { film } => cmd_check(film),
        Command::Fonts { filter } => cmd_fonts(filter),
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
        bail!("{} problem(s) in {}", errs.len(), path.display());
    }
    Ok(film)
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
    let declared = load(&film_path)?;
    let film = scale_film(&declared, scale);
    let out = out.unwrap_or_else(|| film_path.with_extension("mp4"));
    let assets = store(&film_path, &roots);
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

    let mut encoder = FfmpegSink::new(&out, film.width, film.height, film.fps, &EncodeOptions { crf, ..EncodeOptions::default() }, film.background)?;
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
    let film = scale_film(&load(&film_path)?, scale);
    let assets = store(&film_path, &roots);
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
    let film = load(&film_path)?;
    let assets = store(&film_path, &roots);
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

fn cmd_info(film_path: PathBuf) -> Result<()> {
    let film = Film::load(&film_path)?;
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
    }
    .to_string()
}

fn cmd_check(film_path: PathBuf) -> Result<()> {
    let film = Film::load(&film_path)?;
    let errs = film.validate();
    if errs.is_empty() {
        println!("{}: ok", film_path.display());
        return Ok(());
    }
    for e in &errs {
        eprintln!("error: {e}");
    }
    bail!("{} problem(s)", errs.len());
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

