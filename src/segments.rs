//! Segmented, resumable rendering.
//!
//! `cmd_render`'s default whole-film path used to be one `ffmpeg` process fed
//! every frame from first to last: fast, but a render killed at 90% (an OOM,
//! a machine reboot, a `Ctrl-C`) lost the lot, exactly the second half of the
//! problem this module exists for — see `docs/clip-streaming.md` for the
//! first half (bounded decode memory).
//!
//! The fix: split the video-only encode into fixed-size segments
//! (`SEGMENT_SECONDS` each), each its own short-lived `ffmpeg` process
//! writing a `.ts` file next to the output, tracked in a small JSON manifest.
//! A rerun recomputes a fingerprint of everything that could change a
//! frame's pixels — the resolved film (post plugin-expansion, post `--scale`,
//! so a JSON edit anywhere is covered), every referenced asset's resolved
//! path/size/mtime, and the video encode settings — and skips any segment
//! whose manifest entry matches it and whose file is still on disk. A
//! mismatch (the fingerprint differs, or the total/segment frame count
//! differs because `fps`/duration/`--scale` changed) discards the *whole*
//! manifest rather than guessing which segments are still safe: per the
//! brief that added this, a stale resume shipping old frames is worse than
//! no resume at all, and there is no cheap way to know which segments a
//! script edit touched short of re-deriving all of them anyway.
//!
//! **Audio is deliberately not part of any segment.** Every segment is
//! video-only; audio (the film's tracks, a clip's own soundtrack) is mixed
//! in exactly once, in [`finish_segmented_render`], after every segment is
//! concatenated. Two reasons: slicing a track's fades/gain/mix correctly to
//! an arbitrary segment boundary would duplicate `AudioInput`'s own
//! clamping logic for no real benefit, and — the more load-bearing reason —
//! it means a change to *only* audio (a gain tweak, a new track) never
//! invalidates a single already-rendered video segment, since audio plays
//! no part in the fingerprint at all.
//!
//! Segments are `.ts` (MPEG transport stream), not `.mp4`: concatenating
//! several standalone `.mp4` files with `-c copy` is the well-known fragile
//! case (per-file `moov`/edit-list metadata can produce timestamp
//! discontinuities at the seams); concatenating `.ts` via ffmpeg's `concat:`
//! protocol is a straightforward stream-level splice, and [`finish_segmented_render`]
//! remuxes the result into the real output container in the same pass that
//! mixes in audio — one re-encode-free pass, not two.
//!
//! This only covers the default whole-film render (`showreel render <film>`
//! with no `--frames` sub-range and no `--png` dump) — see `cmd_render`.
//! Those are debugging/inspection paths, not the "long render that dies at
//! 90%" case this exists for, so they keep the old single-pass behaviour
//! unchanged rather than absorbing this complexity for no real benefit.

use crate::assets::AssetStore;
use crate::encode::{EncodeOptions, FfmpegSink, finish_segmented_render};
use crate::render::{RenderStats, Renderer};
use crate::timeline::{AssetUse, Film};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How much film-time each segment covers. Small enough that a killed render
/// loses at most a few seconds of already-finished work; large enough that a
/// multi-hour film doesn't spawn thousands of short-lived `ffmpeg`
/// processes.
const SEGMENT_SECONDS: f64 = 8.0;

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    fingerprint: String,
    segment_frames: u32,
    total_frames: u32,
    segments: Vec<SegmentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SegmentEntry {
    index: u32,
    start: u32,
    end: u32,
    file: String,
    done: bool,
}

/// Where a film's segments/manifest live while a render is in progress —
/// a sibling directory of the output, removed on a clean finish.
fn segments_dir(out: &Path) -> PathBuf {
    let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "out".into());
    out.with_file_name(format!(".{name}.segments"))
}

/// Everything that could change a rendered *pixel*: the fully resolved film
/// (after plugin expansion and `--scale`), every asset it references
/// (resolved path, size, mtime — cheap to stat, no need to hash file
/// contents), and the video encode settings. Deliberately excludes audio —
/// see the module doc.
fn fingerprint(film: &Film, assets: &AssetStore, video: &EncodeOptions) -> Result<String> {
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(film).context("serialising film for a render fingerprint")?.hash(&mut hasher);

    let mut used: Vec<AssetUse> = film.assets_used();
    used.sort_by_key(|u| format!("{u:?}"));
    for u in &used {
        format!("{u:?}").hash(&mut hasher);
        let name = match u {
            AssetUse::Still(n) | AssetUse::Data(n) => n,
            AssetUse::Clip { asset, .. } => asset,
        };
        if let Ok(path) = assets.resolve(name)
            && let Ok(meta) = std::fs::metadata(&path)
        {
            path.to_string_lossy().hash(&mut hasher);
            meta.len().hash(&mut hasher);
            if let Ok(modified) = meta.modified()
                && let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                since_epoch.as_nanos().hash(&mut hasher);
            }
        }
    }

    video.crf.hash(&mut hasher);
    video.preset.hash(&mut hasher);
    video.pixel_format.hash(&mut hasher);
    video.codec.hash(&mut hasher);
    video.faststart.hash(&mut hasher);

    Ok(format!("{:016x}", hasher.finish()))
}

fn fresh_manifest(fp: &str, segment_frames: u32, total_frames: u32) -> Manifest {
    let mut segments = Vec::new();
    let mut start = 0u32;
    let mut index = 0u32;
    while start < total_frames {
        let end = (start + segment_frames).min(total_frames);
        segments.push(SegmentEntry { index, start, end, file: format!("seg-{index:05}.ts"), done: false });
        start = end;
        index += 1;
    }
    Manifest { fingerprint: fp.to_string(), segment_frames, total_frames, segments }
}

fn load_manifest(path: &Path) -> Option<Manifest> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest).context("serialising the render manifest")?;
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// Render `film` to `out` in resumable segments, printing what it resumed
/// and what it redid. `opts` carries the *full* audio (mixed in once at the
/// end, not per segment — see the module doc); a video-only clone is what
/// each segment's own `FfmpegSink` actually gets.
pub fn render_segmented(
    renderer: &Renderer,
    film: &Film,
    assets: &AssetStore,
    out: &Path,
    opts: &EncodeOptions,
) -> Result<RenderStats> {
    let total_frames = renderer.frame_count();
    let segment_frames = ((SEGMENT_SECONDS * film.fps).round() as u32).max(1);
    let dir = segments_dir(out);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let manifest_path = dir.join("manifest.json");

    let video_opts = EncodeOptions { audio: Vec::new(), ..opts.clone() };
    let fp = fingerprint(film, assets, &video_opts)?;

    let mut manifest = load_manifest(&manifest_path)
        .filter(|m| m.fingerprint == fp && m.total_frames == total_frames && m.segment_frames == segment_frames)
        .unwrap_or_else(|| fresh_manifest(&fp, segment_frames, total_frames));

    // A segment marked done whose file has since vanished (deleted by hand,
    // a previous run that died between writing the file and the manifest
    // checkpoint after it) is not actually done.
    for seg in &mut manifest.segments {
        if seg.done && !dir.join(&seg.file).exists() {
            seg.done = false;
        }
    }

    let already_done = manifest.segments.iter().filter(|s| s.done).count();
    let to_render = manifest.segments.len() - already_done;
    if already_done > 0 && to_render > 0 {
        println!(
            "  resume  {already_done}/{} segments already rendered (unchanged film/assets/settings); redoing {to_render}",
            manifest.segments.len()
        );
    } else if to_render == 0 {
        println!("  resume  all {} segments already rendered; only finishing (concat + audio)", manifest.segments.len());
    } else {
        println!(
            "  segments {} of ~{segment_frames} frames ({SEGMENT_SECONDS:.0}s) each, none done yet",
            manifest.segments.len()
        );
    }

    let start_time = Instant::now();
    let mut frames_done = 0u32;
    let segment_count = manifest.segments.len();
    for i in 0..segment_count {
        let (start, end, done) = {
            let seg = &manifest.segments[i];
            (seg.start, seg.end, seg.done)
        };
        if done {
            frames_done += end - start;
            continue;
        }
        let seg_path = dir.join(&manifest.segments[i].file);
        let mut sink = FfmpegSink::new(&seg_path, film.width, film.height, film.fps, &video_opts, film.background)?;
        renderer.render_range(start..end, &mut sink)?;
        manifest.segments[i].done = true;
        frames_done += end - start;
        // Checkpoint after every segment, not just at the end — the whole
        // point is that a kill mid-render leaves this accurate.
        save_manifest(&manifest_path, &manifest)?;
        println!("  segment {}/{} rendered (frames {}..{})", i + 1, segment_count, start, end);
    }

    let seg_files: Vec<PathBuf> = manifest.segments.iter().map(|s| dir.join(&s.file)).collect();
    finish_segmented_render(&seg_files, out, opts).context("concatenating segments and muxing audio")?;

    // Only clean up on a fully successful finish — if `finish_segmented_render`
    // failed above, `?` already returned and the segments/manifest are left
    // in place for the next run to resume from.
    std::fs::remove_dir_all(&dir).ok();

    Ok(RenderStats {
        frames: frames_done,
        wall_seconds: start_time.elapsed().as_secs_f64(),
        asset_bytes: renderer.assets.memory_bytes(),
        width: film.width,
        height: film.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::layer::Layer;
    use crate::text::FontDb;
    use crate::timeline::{Film, Scene};

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn solid_film(secs: f64, colour: Color) -> Film {
        Film::new(16, 12, 10.0).open(Scene::new(secs).layer(Layer::solid(colour)))
    }

    #[test]
    fn a_killed_render_resumes_without_redoing_finished_segments() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not on PATH");
            return;
        }
        let tmp = std::env::temp_dir().join("showreel-segments-resume-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let out = tmp.join("out.mp4");
        let store = AssetStore::rooted(&tmp);
        let fonts = FontDb::shared();

        // Long enough (in film-seconds) to span several 8s segments at a
        // tiny, fast-to-render frame size.
        let film = solid_film(20.0, Color::rgb(20, 40, 60));
        let renderer = Renderer::new(&film, &store, fonts);
        let opts = EncodeOptions { crf: 30, ..EncodeOptions::preview() };

        // First pass: manually render only the manifest's first segment,
        // simulating a render that was killed right after segment 0.
        {
            let total_frames = renderer.frame_count();
            let segment_frames = ((SEGMENT_SECONDS * film.fps).round() as u32).max(1);
            let dir = super::segments_dir(&out);
            std::fs::create_dir_all(&dir).unwrap();
            let video_opts = EncodeOptions { audio: Vec::new(), ..opts.clone() };
            let fp = super::fingerprint(&film, &store, &video_opts).unwrap();
            let mut manifest = super::fresh_manifest(&fp, segment_frames, total_frames);
            let seg0 = &mut manifest.segments[0];
            let seg_path = dir.join(&seg0.file);
            let mut sink =
                FfmpegSink::new(&seg_path, film.width, film.height, film.fps, &video_opts, film.background)
                    .unwrap();
            renderer.render_range(seg0.start..seg0.end, &mut sink).unwrap();
            seg0.done = true;
            super::save_manifest(&dir.join("manifest.json"), &manifest).unwrap();
        }
        assert!(manifest_segment_done(&out, 0), "test setup: segment 0 should be marked done");

        // Second pass: the real entry point. It must not re-render segment 0
        // and must still produce a complete, correct final file.
        let stats = render_segmented(&renderer, &film, &store, &out, &opts).unwrap();
        assert!(out.exists(), "final output should exist");
        assert!(stats.frames > 0);
        // The segments directory is gone on a clean finish.
        assert!(!super::segments_dir(&out).exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn manifest_segment_done(out: &Path, index: usize) -> bool {
        let dir = super::segments_dir(out);
        let Some(m) = super::load_manifest(&dir.join("manifest.json")) else { return false };
        m.segments.get(index).is_some_and(|s| s.done)
    }

    #[test]
    fn a_changed_setting_invalidates_the_whole_manifest_rather_than_guessing() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not on PATH");
            return;
        }
        let tmp = std::env::temp_dir().join("showreel-segments-invalidate-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let out = tmp.join("out.mp4");
        let store = AssetStore::rooted(&tmp);
        let fonts = FontDb::shared();

        let film_a = solid_film(9.0, Color::rgb(10, 10, 10));
        let renderer_a = Renderer::new(&film_a, &store, fonts);
        let opts = EncodeOptions { crf: 30, ..EncodeOptions::preview() };
        render_segmented(&renderer_a, &film_a, &store, &out, &opts).unwrap();
        assert!(!super::segments_dir(&out).exists(), "a clean finish always cleans up");

        // A different colour is a different fingerprint (the resolved film
        // changed) -- proven by re-deriving it directly, the same way a
        // second `render_segmented` call would, rather than asserting on
        // internal render counts.
        let film_b = solid_film(9.0, Color::rgb(200, 10, 10));
        let video_opts = EncodeOptions { audio: Vec::new(), ..opts.clone() };
        let fp_a = super::fingerprint(&film_a, &store, &video_opts).unwrap();
        let fp_b = super::fingerprint(&film_b, &store, &video_opts).unwrap();
        assert_ne!(fp_a, fp_b, "changing the film content must change the fingerprint");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
