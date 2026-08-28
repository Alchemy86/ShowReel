//! Baking narration to WAV — the synthesiser seam, and the one place ShowReel
//! shells out to Python.
//!
//! This is narration's `web-pack`: an explicit, ahead-of-time step that turns a
//! [`Narration`] script into a plain WAV asset beside the film, so that
//! `render` needs only ffmpeg and never Python (see [`crate::narration`] for
//! the whole rationale). It verifies every synthesised line and the finished
//! assembly against the native corruption check, and writes the WAV plus a
//! word-timing manifest.
//!
//! # The engine is a seam, not the design
//!
//! *Which* synthesiser runs is chosen by the film's
//! [`Narration::engine`](crate::narration::Narration::engine) and resolved by
//! [`engine_for`] to a [`SynthEngine`]. [`KokoroEngine`] is the one
//! implementation today — the captain's chosen voice model — but it is one
//! entry behind the seam: a different model (a voice-cloning engine, say) is a
//! new `impl SynthEngine`, one row in [`engine_for`], and one in
//! [`crate::narration::KNOWN_ENGINES`]. Everything downstream of the engine —
//! pause insertion, concatenation, verification, the manifest — is
//! engine-agnostic ([`Narration::assemble`],
//! [`crate::narration::looks_like_speech`]), so it is testable with no model in
//! the loop and shared by every engine.
//!
//! # The Python surface is deliberately tiny
//!
//! For the Kokoro engine, the only thing Python does is turn text into speech
//! and report where each word lands: [`DRIVER`]
//! (`tools/narrate/kokoro_narrate.py`) is embedded in the binary and written to
//! a temp file at bake time, so `showreel narrate` is self-contained — a user
//! needs the Kokoro venv, not this repo's layout.
//!
//! # Verify before writing
//!
//! A neural vocoder can emit plausible-looking noise that passes duration/codec
//! checks. Every line, and the whole assembly, is checked with
//! [`zero_crossing_rate`](crate::narration::zero_crossing_rate) before anything
//! is written as final — the file the render path will trust is never written
//! until it has been measured to be speech.

use crate::narration::{self, Narration, SynthLine, WordTiming};
use crate::timeline::Film;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// The Kokoro driver, embedded so `showreel narrate` carries its own Python and
/// does not depend on the repo being checked out. Written to a temp file at
/// bake time. Kept in sync with `tools/narrate/kokoro_narrate.py`.
pub const DRIVER: &str = include_str!("../tools/narrate/kokoro_narrate.py");

/// A narration synthesiser: turns a script into per-line audio and word
/// timings. The seam another voice model slots into — see the module docs.
/// Everything after this (pauses, verification, manifest) is engine-agnostic.
pub trait SynthEngine {
    /// The engine's id, matching a film's `narration.engine`.
    fn id(&self) -> &str;
    /// Synthesise every line, in script order: each result is the line's mono
    /// [`crate::narration::SAMPLE_RATE`] samples and that line's word timings
    /// *relative to the line's own start*. Pace is applied here (it changes the
    /// synthesis); pauses are inserted later by [`Narration::assemble`].
    fn synthesise(&self, narr: &Narration, opts: &NarrateOptions) -> Result<Vec<SynthLine>>;
}

/// Resolve a film's engine name to its implementation. Keep in lock step with
/// [`crate::narration::KNOWN_ENGINES`] — adding an engine adds a row to both.
pub fn engine_for(name: &str) -> Result<Box<dyn SynthEngine>> {
    match name.trim() {
        "kokoro" => Ok(Box::new(KokoroEngine)),
        other => bail!(
            "unknown narration engine {other:?} (known: {})",
            narration::KNOWN_ENGINES.join(", ")
        ),
    }
}

/// Where to find the Python interpreter that has Kokoro installed. The venv the
/// captain proved is at `~/.local/share/kokoro-venv`; override with the
/// `SHOWREEL_KOKORO_PYTHON` environment variable or the `--python` flag.
fn default_python() -> PathBuf {
    if let Ok(p) = std::env::var("SHOWREEL_KOKORO_PYTHON") {
        return PathBuf::from(p);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/kokoro-venv/bin/python");
    }
    PathBuf::from("python3")
}

/// Options for a bake.
#[derive(Debug, Clone)]
pub struct NarrateOptions {
    /// The Python interpreter with Kokoro installed. Kokoro-specific — a future
    /// engine that needs no Python simply ignores it.
    pub python: PathBuf,
    /// Where the baked WAV and manifest are written. Point `render`'s
    /// `-A/--assets` at this directory so it finds them.
    pub out_dir: PathBuf,
    /// Re-synthesise even if a matching bake is already present.
    pub force: bool,
}

impl Default for NarrateOptions {
    fn default() -> Self {
        NarrateOptions { python: default_python(), out_dir: PathBuf::from("."), force: false }
    }
}

/// What a bake produced for one narration track.
#[derive(Debug, Clone)]
pub struct TrackReport {
    pub track_index: usize,
    pub voice: String,
    pub lines: usize,
    pub duration: f64,
    /// Zero-crossing rate of the loudest window of the finished track — the
    /// corruption measure. Well under 0.30 for real speech.
    pub zcr: f64,
    pub wav: PathBuf,
    pub manifest: PathBuf,
    /// True when a matching bake was already present and synthesis was skipped.
    pub cached: bool,
}

/// Bake every narration track in `film`, writing WAVs and manifests into
/// `opts.out_dir`. A film with no narration does nothing. Returns one report per
/// narration track, in track order.
pub fn bake_film(film: &Film, opts: &NarrateOptions) -> Result<Vec<TrackReport>> {
    std::fs::create_dir_all(&opts.out_dir)
        .with_context(|| format!("creating narration output dir {}", opts.out_dir.display()))?;
    let mut reports = Vec::new();
    for (i, track) in film.audio.iter().enumerate() {
        if let Some(narr) = &track.narration {
            let r = bake_narration(narr, i, opts)
                .with_context(|| format!("baking narration for audio track {}", i + 1))?;
            reports.push(r);
        }
    }
    Ok(reports)
}

/// Bake one narration script to `out_dir/<baked_name>.wav` and its manifest.
pub fn bake_narration(narr: &Narration, track_index: usize, opts: &NarrateOptions) -> Result<TrackReport> {
    let errs = narr.validate(&format!("audio track {}", track_index + 1));
    if !errs.is_empty() {
        bail!("narration is not well-formed:\n  {}", errs.join("\n  "));
    }

    let wav_path = opts.out_dir.join(narr.baked_name());
    let manifest_path = opts.out_dir.join(narr.manifest_name());

    // Content-addressed: an existing WAV was verified when it was written and
    // matches this exact script, so reuse it unless forced.
    if !opts.force && is_complete_wav(&wav_path) {
        let bytes = std::fs::read(&wav_path)?;
        let w = narration::read_wav(&bytes)?;
        return Ok(TrackReport {
            track_index,
            voice: narr.voice.clone(),
            lines: narr.lines.len(),
            duration: w.samples.len() as f64 / w.sample_rate.max(1) as f64,
            zcr: narration::zero_crossing_rate(&w.samples, w.sample_rate),
            wav: wav_path,
            manifest: manifest_path,
            cached: true,
        });
    }

    let engine = engine_for(&narr.engine)?;
    let synth = engine
        .synthesise(narr, opts)
        .with_context(|| format!("synthesising with the {:?} engine", engine.id()))?;

    // Verify every line before assembling: a single corrupt line is a broken
    // bake, and catching it here names which line rather than shipping noise.
    for (i, line) in synth.iter().enumerate() {
        let zcr = narration::zero_crossing_rate(&line.samples, narration::SAMPLE_RATE);
        if !narration::looks_like_speech(&line.samples, narration::SAMPLE_RATE) {
            bail!(
                "line {i} synthesised as noise, not speech (zero-crossing rate {zcr:.3}, \
                 speech is < 0.30). Refusing to write a corrupt bake."
            );
        }
    }

    let assembled = narr.assemble(&synth);

    let zcr = narration::zero_crossing_rate(&assembled.samples, narration::SAMPLE_RATE);
    if !narration::looks_like_speech(&assembled.samples, narration::SAMPLE_RATE) {
        bail!(
            "the assembled narration measures as noise (zero-crossing rate {zcr:.3}). \
             Refusing to write a corrupt bake."
        );
    }

    // Write both atomically (temp then rename) so a concurrent render never
    // reads a half-written WAV, mirroring `Music::render_to_temp`.
    let wav = narration::wav_bytes(&assembled.samples, narration::SAMPLE_RATE);
    write_atomic(&wav_path, &wav)?;
    let manifest = serde_json::to_vec_pretty(&assembled.manifest)?;
    write_atomic(&manifest_path, &manifest)?;

    Ok(TrackReport {
        track_index,
        voice: narr.voice.clone(),
        lines: narr.lines.len(),
        duration: assembled.manifest.duration,
        zcr,
        wav: wav_path,
        manifest: manifest_path,
        cached: false,
    })
}

/// The Kokoro engine: shells out to the embedded Python driver.
pub struct KokoroEngine;

impl KokoroEngine {
    /// Kokoro's language code for a voice: `b` for a British voice (the
    /// `b`-prefixed ids), `a` for American. A Kokoro detail, so it lives here
    /// rather than in the engine-agnostic [`Narration`].
    fn lang(voice: &str) -> &'static str {
        if voice.starts_with('b') { "b" } else { "a" }
    }
}

impl SynthEngine for KokoroEngine {
    fn id(&self) -> &str {
        "kokoro"
    }

    fn synthesise(&self, narr: &Narration, opts: &NarrateOptions) -> Result<Vec<SynthLine>> {
        let tmp = tempdir()?;
        let driver_path = tmp.join("kokoro_narrate.py");
        std::fs::write(&driver_path, DRIVER).context("writing the embedded Kokoro driver")?;

        let result_path = tmp.join("timings.json");
        let mut job_lines = Vec::new();
        let mut line_wavs = Vec::new();
        for (i, line) in narr.lines.iter().enumerate() {
            let wav = tmp.join(format!("line-{i}.wav"));
            job_lines.push(serde_json::json!({
                "id": i,
                "text": line.text,
                "speed": line.pace,
                "wav": wav.to_string_lossy(),
            }));
            line_wavs.push(wav);
        }
        let job = serde_json::json!({
            "voice": narr.voice,
            "lang": KokoroEngine::lang(&narr.voice),
            "sample_rate": narration::SAMPLE_RATE,
            "result": result_path.to_string_lossy(),
            "lines": job_lines,
        });
        let job_path = tmp.join("job.json");
        std::fs::write(&job_path, serde_json::to_vec_pretty(&job)?)?;

        run_driver(&opts.python, &driver_path, &job_path)?;

        let result_bytes = std::fs::read(&result_path).with_context(|| {
            format!(
                "the Kokoro driver produced no result file. Is Kokoro installed in {}? \
                 See tools/narrate/README.md.",
                opts.python.display()
            )
        })?;
        let result: DriverResult = serde_json::from_slice(&result_bytes)
            .context("parsing the Kokoro driver's result JSON")?;

        if result.lines.len() != narr.lines.len() {
            bail!(
                "the driver returned {} lines for a {}-line script",
                result.lines.len(),
                narr.lines.len()
            );
        }

        let mut out = Vec::with_capacity(narr.lines.len());
        for (i, line_wav) in line_wavs.iter().enumerate() {
            let bytes = std::fs::read(line_wav).with_context(|| {
                format!("reading synthesised line {i} at {}", line_wav.display())
            })?;
            let wav = narration::read_wav(&bytes)
                .with_context(|| format!("parsing synthesised line {i}"))?;
            let words = result.lines[i]
                .words
                .iter()
                .map(|w| WordTiming { text: w.text.clone(), start: w.start, end: w.end })
                .collect();
            out.push(SynthLine { samples: wav.samples, words });
        }
        Ok(out)
    }
}

/// Invoke the driver, unsetting `VIRTUAL_ENV` (Kokoro's first-run spaCy install
/// throws a confusing error otherwise — a documented gotcha). Surfaces the
/// driver's own stderr on failure, since that carries the real cause (a missing
/// model download, an import error).
fn run_driver(python: &Path, driver: &Path, job: &Path) -> Result<()> {
    let output = std::process::Command::new(python)
        .arg(driver)
        .arg(job)
        .env_remove("VIRTUAL_ENV")
        .output()
        .with_context(|| {
            format!(
                "could not run the Kokoro interpreter at {}. Set SHOWREEL_KOKORO_PYTHON or \
                 pass --python; see tools/narrate/README.md.",
                python.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Keep the tail — the traceback's final lines carry the cause.
        let tail: String = stderr.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        bail!("the Kokoro driver failed:\n{tail}");
    }
    Ok(())
}

fn is_complete_wav(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.len() > 44).unwrap_or(false)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// A unique temp directory for one bake's scratch files, created under the OS
/// temp dir. Not cleaned up automatically — the files are small and named by
/// pid, and leaving them mirrors `showreel-music`'s temp cache.
fn tempdir() -> Result<PathBuf> {
    let dir = std::env::temp_dir()
        .join("showreel-narrate")
        .join(format!("{}-{}", std::process::id(), nanos()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[derive(serde::Deserialize)]
struct DriverResult {
    lines: Vec<DriverLine>,
}

#[derive(serde::Deserialize)]
struct DriverLine {
    #[allow(dead_code)]
    id: usize,
    #[allow(dead_code)]
    duration: f64,
    words: Vec<DriverWord>,
}

#[derive(serde::Deserialize)]
struct DriverWord {
    text: String,
    start: f64,
    end: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_driver_is_the_repo_driver() {
        // include_str! guarantees they are byte-identical at build time; assert
        // it is non-empty and is the real script, so a broken path fails a test
        // rather than a bake.
        assert!(DRIVER.contains("KPipeline"), "the embedded driver is not the Kokoro driver");
        assert!(DRIVER.contains("join_timestamps") || DRIVER.contains("start_ts"));
    }

    #[test]
    fn the_engine_registry_knows_kokoro_and_rejects_the_unknown() {
        assert_eq!(engine_for("kokoro").unwrap().id(), "kokoro");
        let err = engine_for("mystery").err().expect("unknown engine must error").to_string();
        assert!(err.contains("unknown narration engine"), "{err}");
        // The registry and the pure module's list must agree.
        assert!(narration::KNOWN_ENGINES.iter().all(|e| engine_for(e).is_ok()));
    }

    #[test]
    fn kokoro_lang_follows_the_voice_prefix() {
        assert_eq!(KokoroEngine::lang("bm_george"), "b");
        assert_eq!(KokoroEngine::lang("am_adam"), "a");
    }

    #[test]
    fn default_python_prefers_the_env_override() {
        // Not run in parallel with other env tests; SHOWREEL_KOKORO_PYTHON is
        // unique to this feature.
        unsafe { std::env::set_var("SHOWREEL_KOKORO_PYTHON", "/opt/kok/bin/python") };
        assert_eq!(default_python(), PathBuf::from("/opt/kok/bin/python"));
        unsafe { std::env::remove_var("SHOWREEL_KOKORO_PYTHON") };
    }
}
