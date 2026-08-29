//! Audio source separation — pulling speech apart from music/effects in a
//! video or audio file, and being able to put the chosen stem back against
//! the original picture.
//!
//! This is `narrate.rs`'s shape applied to a different Python model: a
//! subprocess boundary to a pinned venv, ahead of time rather than baked into
//! `render` (the same reasoning — a real separation model is a GPU-friendly
//! PyTorch/TensorFlow stack with no wasm story and no business living in the
//! render path). Unlike narration there is no per-line script to assemble;
//! the whole job is "take this audio, hand back two stems," so this module
//! shells out to each model's own CLI directly rather than embedding a custom
//! driver script — there is no bespoke logic (word timings, pause insertion)
//! to isolate into one, and the CLI already is the file-in/file-out job we
//! want. Every other file operation — pulling audio out of a video, and
//! muxing a stem back against the original picture — is ordinary ffmpeg,
//! done here, so the model itself never sees a video container.
//!
//! # The licence gate — read this before adding a model or changing the default
//!
//! Source separation is a solved problem with several mature pretrained
//! models; the choice here is not about which sounds best; it is about which
//! one we are actually allowed to use. **Code and model-weight licences are
//! independent facts and were checked separately** — this project has been
//! caught by exactly that split before (XTTS-v2: MIT code, non-commercial
//! weights).
//!
//! | Model | Code licence | Pretrained-weights licence | Verdict |
//! |---|---|---|---|
//! | **Spleeter** (Deezer) | MIT | **MIT** — Deezer trained on its own private catalogue (the paper: "we cannot release the training data for copyright reasons... sharing pre-trained models were the only way") and released the resulting weights under the same MIT licence as the code, its own call to make since it owns that data | **Usable anywhere**, commercial included |
//! | **Demucs** (Meta, `htdemucs` family) | MIT | **Not covered by the MIT licence.** The maintainer, on the record: "The model weights are not covered by the MIT license, and are provided only for scientific purposes." ([facebookresearch/demucs#327](https://github.com/facebookresearch/demucs/issues/327)) Every released weight set is trained wholly or partly on MUSDB18(-HQ), whose own terms restrict it to academic/research use — the encumbrance traces to the training data, not a Meta-specific restriction | Research/non-commercial use only, best measured quality (see `docs/audio-isolation.md`) |
//! | **Open-Unmix** | MIT | `umxl`: explicit **CC BY-NC-SA 4.0** (non-commercial). `umx`/`umxhq`: no explicit commercial grant, and both are trained on the same MUSDB18(-HQ) that gates Demucs — no cleaner than Demucs, not investigated further | Not used |
//!
//! **[`Model::Spleeter`] is the default for exactly this reason**: it is the
//! only option whose weights carry the same permissive terms as its code.
//! [`Model::Demucs`] is offered — it is real, measurably better on this kind
//! of source (see `docs/audio-isolation.md`, and a prior task's
//! `voice-clone-qwert` report measuring it cleaning a narrated reference clip)
//! — but every call prints a loud, unmissable warning naming the research-only
//! restriction, the same way a corrupt narration bake refuses silently to
//! become a working one. Nothing here launders that restriction away.
//!
//! # What Rust does vs. what Python does
//!
//! Rust: probes the input (does it have audio? video?), extracts audio to a
//! plain WAV via ffmpeg, invokes the model's own CLI in a pinned venv,
//! locates its output files (each model's CLI has a fixed, documented output
//! layout — see [`run_separation`]), copies the two stems to the names this
//! module promises (`speech.wav`/`music.wav`, regardless of what the
//! underlying model calls them), verifies the speech stem actually measures
//! as speech using the same corruption gate narration bakes use
//! ([`crate::narration::zero_crossing_rate`] — a real, model-agnostic sanity
//! check, not a quality score), and can mux a chosen stem back against the
//! original picture. Python: everything else is the model's own CLI,
//! unmodified — no custom driver script, unlike `narrate.rs`'s Kokoro driver,
//! because there is no bespoke logic (word timings, pause direction) to keep
//! testable independent of a model in the loop.

use crate::narration;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which separation model to run. See the module docs for the licence
/// position of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// Deezer's Spleeter — MIT code, MIT weights. The default.
    Spleeter,
    /// Meta's Demucs (`htdemucs`) — MIT code, research-only weights. Prints a
    /// warning on every use; never silently substituted for the default.
    Demucs,
}

/// Every model name this module accepts, for error messages and `--help`.
pub const KNOWN_MODELS: &[&str] = &["spleeter", "demucs"];

impl Model {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "spleeter" => Ok(Model::Spleeter),
            "demucs" | "htdemucs" => Ok(Model::Demucs),
            other => bail!("unknown separation model {other:?} (known: {})", KNOWN_MODELS.join(", ")),
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Model::Spleeter => "spleeter",
            Model::Demucs => "demucs",
        }
    }

    /// The venv this model looks for by default, mirroring
    /// `narrate::default_python`'s `~/.local/share/<name>-venv` convention —
    /// deliberately a *different* venv per model rather than one shared
    /// environment, since Spleeter pins TensorFlow 2.12.1 on Python <3.12 and
    /// Demucs wants a current PyTorch; forcing them into one environment
    /// would pin one against the other for no reason.
    fn default_python(&self) -> PathBuf {
        let env_var = match self {
            Model::Spleeter => "SHOWREEL_SPLEETER_PYTHON",
            Model::Demucs => "SHOWREEL_DEMUCS_PYTHON",
        };
        if let Ok(p) = std::env::var(env_var) {
            return PathBuf::from(p);
        }
        let dir = match self {
            Model::Spleeter => "spleeter-venv",
            Model::Demucs => "demucs-venv",
        };
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share").join(dir).join("bin/python");
        }
        PathBuf::from("python3")
    }
}

/// Options for one isolate run.
#[derive(Debug, Clone)]
pub struct IsolateOptions {
    pub model: Model,
    /// The Python interpreter with the chosen model installed. Defaults to
    /// the model's own venv convention — see [`Model::default_python`].
    pub python: PathBuf,
    /// Where the stems (and any muxed video) are written.
    pub out_dir: PathBuf,
    /// Also write a video with this stem's audio remuxed against the
    /// original picture: `"speech"` or `"music"`. Only meaningful when the
    /// input has a video stream.
    pub mux: Option<String>,
    /// Re-run separation even if matching stems are already present.
    pub force: bool,
}

impl IsolateOptions {
    /// Defaults to [`Model::Spleeter`] and its venv. Use [`IsolateOptions::for_model`]
    /// when the model is a run-time choice (e.g. from `--model`) — `python`
    /// must be derived from whichever model actually ends up running, since
    /// each model looks for a different venv (see [`Model::default_python`]).
    pub fn new(out_dir: impl Into<PathBuf>) -> Self {
        Self::for_model(Model::Spleeter, out_dir)
    }

    pub fn for_model(model: Model, out_dir: impl Into<PathBuf>) -> Self {
        IsolateOptions { python: model.default_python(), model, out_dir: out_dir.into(), mux: None, force: false }
    }
}

/// What one isolate run produced.
#[derive(Debug, Clone)]
pub struct IsolateReport {
    pub model: Model,
    pub input_duration: f64,
    pub speech: PathBuf,
    pub music: PathBuf,
    /// Zero-crossing rate of the speech stem's loudest window — the same
    /// corruption measure narration bakes use. Not a quality score: it
    /// answers "does this look like speech at all," not "how clean is it."
    pub speech_zcr: f64,
    pub muxed: Option<PathBuf>,
    /// True when matching output was already present and separation was
    /// skipped.
    pub cached: bool,
}

/// Separate `input` (a video or audio file) into a speech stem and a
/// music/effects stem, writing both into `opts.out_dir`, and optionally mux
/// one stem back against `input`'s own picture.
pub fn isolate(input: &Path, opts: &IsolateOptions) -> Result<IsolateReport> {
    if !input.exists() {
        bail!("input file not found: {}", input.display());
    }
    std::fs::create_dir_all(&opts.out_dir)
        .with_context(|| format!("creating output dir {}", opts.out_dir.display()))?;

    let streams = probe_streams(input)?;
    if !streams.has_audio {
        bail!(
            "{} has no audio stream — nothing to separate. (ffprobe found {})",
            input.display(),
            if streams.has_video { "a video stream but no audio" } else { "neither audio nor video" }
        );
    }
    if opts.mux.is_some() && !streams.has_video {
        bail!(
            "--mux was given but {} has no video stream to mux against",
            input.display()
        );
    }

    let speech_path = opts.out_dir.join("speech.wav");
    let music_path = opts.out_dir.join("music.wav");
    let manifest_path = opts.out_dir.join(".isolate-manifest.json");

    let input_meta = std::fs::metadata(input)?;
    let fingerprint = format!(
        "{}:{}:{}",
        opts.model.id(),
        input_meta.len(),
        input_meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
    );

    if !opts.force
        && speech_path.exists()
        && music_path.exists()
        && let Ok(existing) = std::fs::read_to_string(&manifest_path)
        && existing.trim() == fingerprint
    {
        let duration = probe_duration(input).unwrap_or(0.0);
        let speech_zcr = zcr_of_wav(&speech_path).unwrap_or(0.0);
        // The stems are cached, but a requested mux might not be: mux is
        // keyed off the stem files, not the fingerprint, so a first run
        // without `--mux` followed by a second run with it must still
        // produce the muxed file rather than silently reporting `None`.
        let muxed = ensure_mux(input, &opts.mux, &speech_path, &music_path, &opts.out_dir, opts.force)?;
        return Ok(IsolateReport {
            model: opts.model,
            input_duration: duration,
            speech: speech_path,
            music: music_path,
            speech_zcr,
            muxed,
            cached: true,
        });
    }

    if opts.model == Model::Demucs {
        eprintln!(
            "warning: the `demucs` (htdemucs) pretrained weights are not covered by its MIT \
             code licence — the maintainer states they are \"provided only for scientific \
             purposes\" (trained on MUSDB18, an academic-only dataset). Using them for anything \
             beyond research/internal use needs a licence from Meta. See docs/audio-isolation.md."
        );
    }

    let work = tempdir()?;
    let track_wav = work.join("track.wav");
    extract_audio(input, &track_wav)?;
    let input_duration = probe_duration(&track_wav)?;

    let (raw_speech, raw_music) = run_separation(&opts.python, opts.model, &track_wav, &work)?;

    // Re-encode to a canonical 16-bit PCM WAV on the way out, rather than
    // trusting whatever bit depth/float format the model's own CLI chose —
    // it also means `zero_crossing_rate` (which reads canonical PCM16) can
    // check the result without a second WAV parser.
    to_pcm16(&raw_speech, &speech_path)?;
    to_pcm16(&raw_music, &music_path)?;
    std::fs::write(&manifest_path, &fingerprint)?;

    let speech_zcr = zcr_of_wav(&speech_path)?;
    if !narration::looks_like_speech(&read_pcm16(&speech_path)?, 44_100) {
        eprintln!(
            "warning: the speech stem measures as noise, not speech (zero-crossing rate \
             {speech_zcr:.3}, speech is < 0.30) — separation likely failed or the source has \
             no dialogue in it. Files were still written; listen before trusting them."
        );
    }

    let muxed = ensure_mux(input, &opts.mux, &speech_path, &music_path, &opts.out_dir, true)?;

    // Scratch files are small and pid-named (mirrors `narrate`'s tempdir);
    // not worth failing the whole run over a cleanup error.
    let _ = std::fs::remove_dir_all(&work);

    Ok(IsolateReport {
        model: opts.model,
        input_duration,
        speech: speech_path,
        music: music_path,
        speech_zcr,
        muxed,
        cached: false,
    })
}

/// Mux the requested stem against `input`'s picture if asked, writing (or
/// reusing) `<out_dir>/<stem>-muxed.mp4`. Separate from the stem cache check
/// above: a mux request can arrive on a run whose stems were already cached
/// from an earlier, mux-less run, and must still produce the file rather than
/// silently reporting `None`.
fn ensure_mux(
    input: &Path,
    mux: &Option<String>,
    speech_path: &Path,
    music_path: &Path,
    out_dir: &Path,
    force: bool,
) -> Result<Option<PathBuf>> {
    let Some(stem) = mux else { return Ok(None) };
    let src = match stem.as_str() {
        "speech" => speech_path,
        "music" => music_path,
        other => bail!("unknown --mux stem {other:?} (expected \"speech\" or \"music\")"),
    };
    let dest = out_dir.join(format!("{stem}-muxed.mp4"));
    if force || !dest.exists() {
        mux_stem(input, src, &dest)?;
    }
    Ok(Some(dest))
}

struct Streams {
    has_audio: bool,
    has_video: bool,
}

/// Whether `path` has an audio and/or video stream, via ffprobe. Checked up
/// front so a video with no audio (a real, common shape — see the crate's own
/// `clip_track`/audio sharp edge) fails with one clear sentence instead of an
/// opaque ffmpeg error three steps later.
fn probe_streams(path: &Path) -> Result<Streams> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .with_context(|| format!("running ffprobe on {}", path.display()))?;
    if !out.status.success() {
        bail!("ffprobe failed on {}: {}", path.display(), String::from_utf8_lossy(&out.stderr).trim());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(Streams { has_audio: s.lines().any(|l| l.trim() == "audio"), has_video: s.lines().any(|l| l.trim() == "video") })
}

fn probe_duration(path: &Path) -> Result<f64> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .with_context(|| format!("running ffprobe on {}", path.display()))?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .with_context(|| format!("ffprobe duration for {}", path.display()))
}

/// Pull the audio out of `input` (video or audio, doesn't matter — ffmpeg
/// demuxes either) to a plain stereo 44.1kHz WAV, the format every separation
/// model here expects.
fn extract_audio(input: &Path, dest: &Path) -> Result<()> {
    let out = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y"])
        .arg("-i")
        .arg(input)
        .args(["-vn", "-ac", "2", "-ar", "44100", "-c:a", "pcm_s16le"])
        .arg(dest)
        .output()
        .with_context(|| format!("running ffmpeg to extract audio from {}", input.display()))?;
    if !out.status.success() {
        bail!("ffmpeg failed extracting audio from {}: {}", input.display(), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Run the chosen model's own CLI over `track_wav` inside `work`, and return
/// the paths it wrote the two stems to. Each model's CLI has a fixed output
/// layout given a known input basename — `track_wav` is always named
/// `track.wav` by [`isolate`] specifically so these paths are predictable
/// rather than scraped from stdout.
fn run_separation(python: &Path, model: Model, track_wav: &Path, work: &Path) -> Result<(PathBuf, PathBuf)> {
    let sep_out = work.join("separated");
    std::fs::create_dir_all(&sep_out)?;

    match model {
        Model::Spleeter => {
            // `spleeter separate -p spleeter:2stems -o <out> track.wav` writes
            // <out>/track/vocals.wav and <out>/track/accompaniment.wav.
            // `MODEL_PATH` pins the pretrained-weights cache to the venv's own
            // directory: left unset, Spleeter defaults to a relative
            // `pretrained_models/` in whatever directory the caller happened
            // to invoke `showreel` from — the repo root, once, until this was
            // caught — rather than a persistent, out-of-tree cache.
            let model_cache = python.parent().and_then(Path::parent).map(|venv| venv.join("pretrained_models"));
            run_model_cli(
                python,
                &["-m", "spleeter", "separate", "-p", "spleeter:2stems", "-c", "wav", "-o"],
                &sep_out,
                track_wav,
                "Spleeter",
                model_cache.map(|p| ("MODEL_PATH", p)),
            )?;
            let base = sep_out.join("track");
            Ok((base.join("vocals.wav"), base.join("accompaniment.wav")))
        }
        Model::Demucs => {
            // `python -m demucs --two-stems vocals -n htdemucs -o <out>
            // track.wav` writes <out>/htdemucs/track/vocals.wav and no_vocals.wav.
            // Its default output (neither --int24 nor --float32 given) is
            // 16-bit PCM, which `to_pcm16` below re-confirms rather than
            // trusts. Demucs caches its weights under `torch.hub`'s own
            // (already out-of-tree) default, so it needs no
            // `MODEL_PATH`-style override.
            run_model_cli(
                python,
                &["-m", "demucs", "--two-stems", "vocals", "-n", "htdemucs", "-o"],
                &sep_out,
                track_wav,
                "Demucs",
                None,
            )?;
            let base = sep_out.join("htdemucs").join("track");
            Ok((base.join("vocals.wav"), base.join("no_vocals.wav")))
        }
    }
}

fn run_model_cli(
    python: &Path,
    leading_args: &[&str],
    out_dir: &Path,
    track_wav: &Path,
    name: &str,
    extra_env: Option<(&str, PathBuf)>,
) -> Result<()> {
    let mut cmd = Command::new(python);
    cmd.args(leading_args).arg(out_dir).arg(track_wav).env_remove("VIRTUAL_ENV");
    if let Some((key, val)) = extra_env {
        cmd.env(key, val);
    }
    let output = cmd
        .output()
        .with_context(|| {
            format!(
                "could not run {name} via {}. Set the model's *_PYTHON env var or pass --python; \
                 see docs/audio-isolation.md.",
                python.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        bail!("{name} failed:\n{tail}");
    }
    Ok(())
}

/// Re-encode any WAV (whatever bit depth/float format a model's CLI chose)
/// to canonical stereo 16-bit PCM at 44.1kHz.
fn to_pcm16(src: &Path, dest: &Path) -> Result<()> {
    if !src.exists() {
        bail!(
            "expected separated output at {} but it does not exist — the model's CLI output \
             layout may have changed; see run_separation's doc comment",
            src.display()
        );
    }
    let out = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y"])
        .arg("-i")
        .arg(src)
        .args(["-ac", "2", "-ar", "44100", "-c:a", "pcm_s16le"])
        .arg(dest)
        .output()
        .with_context(|| format!("running ffmpeg to normalise {}", src.display()))?;
    if !out.status.success() {
        bail!("ffmpeg failed normalising {}: {}", src.display(), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Mux `stem_wav`'s audio against `original`'s own video stream, re-encoding
/// audio to AAC (mp4-compatible) and copying video untouched.
fn mux_stem(original: &Path, stem_wav: &Path, dest: &Path) -> Result<()> {
    let out = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y"])
        .arg("-i")
        .arg(original)
        .arg("-i")
        .arg(stem_wav)
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest"])
        .arg(dest)
        .output()
        .with_context(|| format!("running ffmpeg to mux {} against {}", stem_wav.display(), original.display()))?;
    if !out.status.success() {
        bail!("ffmpeg failed muxing {}: {}", dest.display(), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn read_pcm16(path: &Path) -> Result<Vec<i16>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(narration::read_wav(&bytes)?.samples)
}

fn zcr_of_wav(path: &Path) -> Result<f64> {
    let samples = read_pcm16(path)?;
    Ok(narration::zero_crossing_rate(&samples, 44_100))
}

/// A unique temp directory for one run's scratch files — mirrors
/// `narrate::tempdir`.
fn tempdir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("showreel-isolate").join(format!("{}-{}", std::process::id(), nanos()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_parse_accepts_known_names_and_rejects_others() {
        assert_eq!(Model::parse("spleeter").unwrap(), Model::Spleeter);
        assert_eq!(Model::parse("Spleeter").unwrap(), Model::Spleeter);
        assert_eq!(Model::parse("demucs").unwrap(), Model::Demucs);
        assert_eq!(Model::parse("htdemucs").unwrap(), Model::Demucs);
        let err = Model::parse("mystery").unwrap_err().to_string();
        assert!(err.contains("unknown separation model"), "{err}");
    }

    #[test]
    fn default_model_is_spleeter_the_only_licence_clean_option() {
        assert_eq!(IsolateOptions::new(".").model, Model::Spleeter);
    }

    #[test]
    fn default_python_prefers_the_env_override_and_is_per_model() {
        unsafe { std::env::set_var("SHOWREEL_SPLEETER_PYTHON", "/opt/sep/bin/python") };
        assert_eq!(Model::Spleeter.default_python(), PathBuf::from("/opt/sep/bin/python"));
        unsafe { std::env::remove_var("SHOWREEL_SPLEETER_PYTHON") };

        // Different models must not share a venv path even with no override.
        assert_ne!(Model::Spleeter.default_python(), Model::Demucs.default_python());
    }

    #[test]
    fn isolate_on_a_missing_file_fails_loudly_rather_than_shelling_out() {
        let opts = IsolateOptions::new(std::env::temp_dir().join("showreel-isolate-test-missing-out"));
        let err = isolate(Path::new("/no/such/file.mp4"), &opts).unwrap_err().to_string();
        assert!(err.contains("not found"), "{err}");
    }

    #[test]
    fn for_model_picks_that_models_own_python_not_the_default() {
        // Regression: `--model demucs` once silently ran through Spleeter's
        // venv because only `IsolateOptions::new` (hard-wired to Spleeter)
        // existed and the CLI forgot to re-derive `python` for the chosen
        // model.
        assert_eq!(IsolateOptions::for_model(Model::Spleeter, ".").python, Model::Spleeter.default_python());
        assert_eq!(IsolateOptions::for_model(Model::Demucs, ".").python, Model::Demucs.default_python());
        assert_ne!(
            IsolateOptions::for_model(Model::Demucs, ".").python,
            IsolateOptions::for_model(Model::Spleeter, ".").python
        );
    }

    fn ffmpeg_available() -> bool {
        Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn synth_video(path: &Path) {
        let status = Command::new("ffmpeg")
            .args(["-y", "-nostdin", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=duration=1:size=16x16:rate=5"])
            .args(["-f", "lavfi", "-i", "anullsrc=r=44100:cl=stereo:d=1"])
            .args(["-shortest", "-c:v", "libx264", "-c:a", "aac"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn synth_silence_wav(path: &Path) {
        let status = Command::new("ffmpeg")
            .args(["-y", "-nostdin", "-loglevel", "error", "-f", "lavfi", "-i", "anullsrc=r=44100:cl=stereo:d=1"])
            .args(["-c:a", "pcm_s16le"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn ensure_mux_runs_on_a_cache_hit_even_when_an_earlier_run_never_asked_for_it() {
        // Regression: `isolate`'s cache-hit branch used to check only whether
        // a previously-muxed file already existed, never producing one for a
        // *first* `--mux` request against already-cached stems.
        if !ffmpeg_available() {
            return;
        }
        let dir = tempdir().unwrap();
        let video = dir.join("in.mp4");
        let speech = dir.join("speech.wav");
        let music = dir.join("music.wav");
        synth_video(&video);
        synth_silence_wav(&speech);
        synth_silence_wav(&music);

        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let muxed = ensure_mux(&video, &Some("speech".to_string()), &speech, &music, &out, false).unwrap();
        let muxed = muxed.expect("a mux was requested, so a path must come back");
        assert!(muxed.exists(), "ensure_mux must actually write the file on first request");

        // A second call with the file already present and force=false must
        // not error and must still report the same path.
        let again = ensure_mux(&video, &Some("speech".to_string()), &speech, &music, &out, false).unwrap();
        assert_eq!(again, Some(muxed));
    }
}
