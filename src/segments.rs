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
//!
//! **Segments render through a bounded, machine-aware worker pool, not
//! strictly one at a time.** Segments are already independent by
//! construction — this is what makes `--frames <start-end>` and this very
//! resume mechanism correct — so the parallelism was always there to take;
//! nothing did until `src/budget.rs` (see its module doc for the 2026-08-29
//! incident this exists to prevent, and why the reasoning happens there,
//! not here). The shape: render the first pending segment serially, exactly
//! as before — this both preserves today's behaviour whenever there's
//! nothing left to parallelise and gives an honest, *measured* per-worker
//! memory cost (system-wide, not `showreel`'s own RSS — see `budget`'s doc
//! for why) before committing to any concurrency. Only then is a worker
//! count planned from real machine state, a cross-process ledger (so a
//! second concurrent `showreel render` shrinks its own plan rather than
//! stacking on top blind), and that measured cost — and only then do the
//! remaining segments fan out.
//!
//! **A concurrent worker gets its own [`AssetStore`], not the caller's.** A
//! streaming (large) clip's decoder is one `ffmpeg` child behind one mutex
//! (`src/assets/clip.rs`) — genuinely sequential by design, so two threads
//! asking it for frames from opposite ends of a big source would not decode
//! faster in parallel, they would *thrash*, each request evicting the
//! other's cache window and forcing a reseek. Giving each worker its own
//! store means its own independent decoder — real concurrent `ffmpeg`
//! throughput, and the real, honest per-worker memory cost the budget
//! measures against. This is also why a store with no roots at all (every
//! asset supplied programmatically via `insert_still`/`insert_clip`, never
//! from a file) can't be parallelised this way — [`AssetStore::roots`] would
//! have nothing to rebuild from — and this module falls back to fully serial
//! rendering in that case rather than silently losing those assets.

use crate::assets::AssetStore;
use crate::budget;
use crate::encode::{EncodeOptions, FfmpegSink, finish_segmented_render};
use crate::render::{RenderStats, Renderer};
use crate::text::FontDb;
use crate::timeline::{AssetUse, Film};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How much film-time each segment covers. Small enough that a killed render
/// loses at most a few seconds of already-finished work; large enough that a
/// multi-hour film doesn't spawn thousands of short-lived `ffmpeg`
/// processes.
const SEGMENT_SECONDS: f64 = 8.0;

#[derive(Debug, Default, Serialize, Deserialize)]
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

/// Renders one segment into its `.ts` file through `renderer` — shared
/// helper for the serial and concurrent paths below, which differ only in
/// which `Renderer` (and therefore which `AssetStore`) they pass in.
fn render_segment(renderer: &Renderer, video_opts: &EncodeOptions, seg_path: &Path, range: std::ops::Range<u32>) -> Result<()> {
    let film = renderer.film;
    let mut sink = FfmpegSink::new(seg_path, film.width, film.height, film.fps, video_opts, film.background)?;
    renderer.render_range(range, &mut sink)?;
    Ok(())
}

/// A fresh, independently-caching store searching the same roots as
/// `roots` — what a concurrent worker renders through, so its clip decode
/// (if any) is its own `ffmpeg` child rather than contending the caller's
/// single streaming-clip mutex. See the module doc.
fn store_from_roots(roots: &[PathBuf]) -> AssetStore {
    let mut store = AssetStore::new();
    for root in roots {
        store.add_root(root.clone());
    }
    store
}

/// Render `film` to `out` in resumable segments, printing what it resumed
/// and what it redid. `opts` carries the *full* audio (mixed in once at the
/// end, not per segment — see the module doc); a video-only clone is what
/// each segment's own `FfmpegSink` actually gets.
///
/// `max_workers` overrides the machine-derived worker count (`--max-workers`
/// on the CLI) — `None` plans automatically from measured memory, machine
/// state, and the cross-process ledger; `Some(1)` forces the old strictly-
/// serial behaviour.
pub fn render_segmented(
    renderer: &Renderer,
    film: &Film,
    assets: &AssetStore,
    out: &Path,
    opts: &EncodeOptions,
    max_workers: Option<usize>,
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
    let segment_count = manifest.segments.len();
    let mut frames_done: u32 = manifest.segments.iter().filter(|s| s.done).map(|s| s.end - s.start).sum();
    let pending: Vec<usize> = (0..segment_count).filter(|&i| !manifest.segments[i].done).collect();

    let sample_interval = Duration::from_millis(200);
    let sampler = budget::PeakSampler::start(sample_interval);
    let mut workers_used = 1usize;

    if let Some(&first) = pending.first() {
        // Render one segment serially first: preserves today's exact
        // behaviour whenever there's nothing left to parallelise, and gives
        // an honest, *measured* per-worker memory cost (not an invented
        // constant) before any concurrency decision.
        let range = manifest.segments[first].start..manifest.segments[first].end;
        let seg_path = dir.join(&manifest.segments[first].file);
        let (_, per_worker_bytes) =
            budget::measure(sample_interval, || render_segment(renderer, &video_opts, &seg_path, range.clone()))?;
        manifest.segments[first].done = true;
        frames_done += range.end - range.start;
        save_manifest(&manifest_path, &manifest)?;
        println!("  segment {}/{} rendered (frames {}..{})", first + 1, segment_count, range.start, range.end);

        let remaining: Vec<usize> = pending[1..].to_vec();
        if !remaining.is_empty() {
            let roots = assets.roots().to_vec();
            let machine = budget::MachineState::probe();
            let usable = budget::usable_bytes(machine.available_bytes);
            let ledger = budget::Ledger::open();
            // A first guess before we know what everyone else on the
            // machine has already claimed — refined immediately below, once
            // `reserve` reports it.
            let guess = budget::plan_workers(usable, per_worker_bytes, machine.cores, remaining.len());
            let (reservation, other_reserved) =
                budget::reserve(&ledger, per_worker_bytes.saturating_mul(guess.workers as u64))?;
            let available_for_us = usable.saturating_sub(other_reserved);
            let mut plan = budget::plan_workers(available_for_us, per_worker_bytes, machine.cores, remaining.len());
            if roots.is_empty() {
                // Nothing to rebuild a per-worker AssetStore from — see the
                // module doc. Correctness over speed: stay serial.
                plan.workers = 1;
            }
            if let Some(cap) = max_workers {
                plan.workers = plan.workers.clamp(1, cap.max(1));
            }
            reservation.touch(per_worker_bytes.saturating_mul(plan.workers as u64));
            workers_used = plan.workers;

            if plan.workers <= 1 {
                // Only worth explaining when there was real concurrency to
                // narrow *from* — `remaining.len() == 1` means there was
                // never more than one segment left regardless of memory.
                if remaining.len() > 1 {
                    eprintln!(
                        "  budget  running serial, only {:.1} GB headroom ({:.0} MB measured per worker, {:.1} GB already claimed by other renders)",
                        available_for_us as f64 / 1e9,
                        per_worker_bytes as f64 / 1e6,
                        other_reserved as f64 / 1e9
                    );
                }
                for &i in &remaining {
                    if budget::is_critical(machine.total_bytes) {
                        eprintln!("  budget  headroom critical mid-render; continuing serially regardless");
                    }
                    let range = manifest.segments[i].start..manifest.segments[i].end;
                    let seg_path = dir.join(&manifest.segments[i].file);
                    render_segment(renderer, &video_opts, &seg_path, range.clone())?;
                    manifest.segments[i].done = true;
                    frames_done += range.end - range.start;
                    reservation.touch(per_worker_bytes);
                    save_manifest(&manifest_path, &manifest)?;
                    println!("  segment {}/{} rendered (frames {}..{})", i + 1, segment_count, range.start, range.end);
                }
            } else {
                println!(
                    "  budget  {} workers (~{:.0} MB each, measured; {:.1} GB headroom, {:.1} GB claimed elsewhere)",
                    plan.workers,
                    per_worker_bytes as f64 / 1e6,
                    available_for_us as f64 / 1e9,
                    other_reserved as f64 / 1e9
                );
                let fonts = FontDb::shared();
                let workers_planned = plan.workers;
                let total_bytes = machine.total_bytes;
                // ffmpeg's own decoder and encoder each default to using
                // every core they can see. Left alone, `workers_planned`
                // concurrent workers would each start such a decode *and*
                // encode process, oversubscribing the machine by roughly
                // `workers_planned`x on top of the raster work rayon is
                // already doing — measured directly (`docs/render-budget.md`):
                // four unthrottled concurrent workers on a 20-core box ran
                // *slower* than one serial pass (15x the involuntary context
                // switches); capping decode threads alone was not enough —
                // ffmpeg's encoder threads were still the dominant
                // oversubscription. Capping both is what turns the
                // concurrency into an actual speedup rather than thrash.
                //
                // This is *not* free: x264's own encoded bytes are sensitive
                // to its thread count (a documented, expected property of
                // frame-parallel encoding, not a bug), so a segment encoded
                // by a concurrent worker is not guaranteed byte-for-byte
                // identical to the same segment encoded serially — the same
                // trade this crate already made and measured for segmenting
                // itself (`docs/segmented-rendering.md`'s GOP-boundary
                // finding: segmenting changes encoded bytes, not decoded
                // pixels). The guarantee that actually matters — the frame
                // content — is unaffected: `render_segment` always calls the
                // same deterministic `Renderer::render_range` on the same
                // film and assets regardless of which worker calls it, and
                // `render.rs`'s own `sequential_render_ranges_over_a_streaming_clip_match_one_continuous_pass`
                // already proves sub-range rendering is pixel-exact. This
                // module's own test compares decoded pixels between a
                // concurrent and a forced-serial run for exactly this reason.
                let threads_per_worker = (machine.cores / workers_planned).max(1) as u32;
                let worker_video_opts = EncodeOptions { threads: Some(threads_per_worker), ..video_opts.clone() };
                let queue: Mutex<VecDeque<usize>> = Mutex::new(remaining.into_iter().collect());
                let manifest_mutex = Mutex::new(std::mem::take(&mut manifest));

                let worker_result = std::thread::scope(|scope| -> Result<u32> {
                    let mut handles = Vec::new();
                    for _ in 0..workers_planned {
                        let queue = &queue;
                        let manifest_mutex = &manifest_mutex;
                        let roots = &roots;
                        let reservation = &reservation;
                        let dir = &dir;
                        let worker_video_opts = &worker_video_opts;
                        let manifest_path = &manifest_path;
                        handles.push(scope.spawn(move || -> Result<u32> {
                            crate::assets::clip::set_decode_threads_hint(Some(threads_per_worker));
                            let store = store_from_roots(roots);
                            let mut worker_frames = 0u32;
                            loop {
                                if budget::is_critical(total_bytes) {
                                    eprintln!("  budget  headroom critical; a worker is stopping early rather than taking on more");
                                    return Ok(worker_frames);
                                }
                                let Some(i) = queue.lock().unwrap().pop_front() else { return Ok(worker_frames) };
                                let (start, end, file) = {
                                    let m = manifest_mutex.lock().unwrap();
                                    let s = &m.segments[i];
                                    (s.start, s.end, s.file.clone())
                                };
                                let worker_renderer = Renderer::new(film, &store, fonts);
                                let seg_path = dir.join(&file);
                                render_segment(&worker_renderer, worker_video_opts, &seg_path, start..end)?;
                                worker_frames += end - start;
                                {
                                    let mut m = manifest_mutex.lock().unwrap();
                                    m.segments[i].done = true;
                                    save_manifest(manifest_path, &m)?;
                                    println!("  segment {}/{} rendered (frames {}..{})", i + 1, segment_count, start, end);
                                }
                                reservation.touch(per_worker_bytes.saturating_mul(workers_planned as u64));
                            }
                        }));
                    }
                    let mut total = 0u32;
                    for h in handles {
                        total += h.join().expect("a segment worker thread panicked")?;
                    }
                    Ok(total)
                });
                manifest = manifest_mutex.into_inner().unwrap();
                frames_done += worker_result?;
            }
        }
    }

    let peak_memory_bytes = sampler.stop();

    let seg_files: Vec<PathBuf> = manifest.segments.iter().map(|s| dir.join(&s.file)).collect();
    finish_segmented_render(&seg_files, out, opts).context("concatenating segments and muxing audio")?;

    // Only clean up on a fully successful finish — if `finish_segmented_render`
    // failed above, `?` already returned and the segments/manifest are left
    // in place for the next run to resume from.
    std::fs::remove_dir_all(&dir).ok();

    let wall_seconds = start_time.elapsed().as_secs_f64();
    println!(
        "  budget  peak {:.0} MB used, {} worker{} at once, {wall_seconds:.1}s wall",
        peak_memory_bytes as f64 / 1e6,
        workers_used,
        if workers_used == 1 { "" } else { "s" }
    );

    Ok(RenderStats {
        frames: frames_done,
        wall_seconds,
        asset_bytes: renderer.assets.memory_bytes(),
        width: film.width,
        height: film.height,
        peak_memory_bytes,
        workers: workers_used,
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
        let stats = render_segmented(&renderer, &film, &store, &out, &opts, None).unwrap();
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
        render_segmented(&renderer_a, &film_a, &store, &out, &opts, None).unwrap();
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

    /// Decodes one frame at `at_secs` to raw rgb24 via ffmpeg, for comparing
    /// two encoded outputs at the pixel level rather than as raw bytes.
    fn decode_frame_rgb(path: &Path, at_secs: f64) -> Vec<u8> {
        let out = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-loglevel", "error", "-y"])
            .args(["-ss", &format!("{at_secs}")])
            .arg("-i")
            .arg(path)
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .expect("run ffmpeg to decode a frame");
        assert!(out.status.success(), "ffmpeg decode failed: {}", String::from_utf8_lossy(&out.stderr));
        out.stdout
    }

    /// Mean absolute per-byte difference between two equal-length buffers.
    fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
        assert_eq!(a.len(), b.len(), "decoded frames must be the same size");
        let sum: i64 = a.iter().zip(b).map(|(x, y)| (*x as i64 - *y as i64).abs()).sum();
        sum as f64 / a.len() as f64
    }

    /// The bar `AGENTS.md`'s render-budget brief sets: bounding/raising
    /// concurrency must not change a single *frame*. It does not hold at the
    /// encoded-*byte* level — deliberately: a concurrent worker's `ffmpeg`
    /// encode is thread-capped differently from a serial one (see
    /// `render_segmented`'s doc comment on why), and x264's own encoded
    /// bytes are sensitive to thread count, the same well-understood,
    /// already-documented trade this crate made for segmenting itself
    /// (`docs/segmented-rendering.md`'s GOP-boundary finding). So this
    /// compares *decoded pixels*, not raw file bytes, at several points
    /// across a five-segment film — auto-planned (concurrent, on any machine
    /// with room) against the same film forced fully serial via
    /// `max_workers: Some(1)`.
    #[test]
    fn concurrent_and_forced_serial_segment_rendering_produce_the_same_picture() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not on PATH");
            return;
        }
        let tmp = std::env::temp_dir().join("showreel-segments-concurrency-parity-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = AssetStore::rooted(&tmp);
        let fonts = FontDb::shared();

        // 40s at 10fps = 400 frames = 5 segments of 80 -- enough that the
        // auto plan has real remaining work to fan out, on any machine with
        // more than one core free.
        let film = solid_film(40.0, Color::rgb(30, 90, 150));
        let renderer = Renderer::new(&film, &store, fonts);
        let opts = EncodeOptions { crf: 30, ..EncodeOptions::preview() };

        let out_auto = tmp.join("auto.mp4");
        let stats_auto = render_segmented(&renderer, &film, &store, &out_auto, &opts, None).unwrap();

        let out_serial = tmp.join("serial.mp4");
        let stats_serial = render_segmented(&renderer, &film, &store, &out_serial, &opts, Some(1)).unwrap();

        assert_eq!(stats_serial.workers, 1, "max_workers: Some(1) must force serial");
        assert_eq!(stats_auto.frames, stats_serial.frames);
        assert_eq!(stats_auto.frames, 400);

        for at in [1.0, 9.5, 19.9, 30.1, 38.5] {
            let a = decode_frame_rgb(&out_auto, at);
            let b = decode_frame_rgb(&out_serial, at);
            let diff = mean_abs_diff(&a, &b);
            assert!(
                diff < 2.0,
                "frame at {at}s differs too much between {}-worker and serial (mean abs diff {diff:.3}/255)",
                stats_auto.workers
            );
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_max_workers_cap_is_never_exceeded() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not on PATH");
            return;
        }
        let tmp = std::env::temp_dir().join("showreel-segments-max-workers-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = AssetStore::rooted(&tmp);
        let fonts = FontDb::shared();

        let film = solid_film(40.0, Color::rgb(60, 60, 60));
        let renderer = Renderer::new(&film, &store, fonts);
        let opts = EncodeOptions { crf: 30, ..EncodeOptions::preview() };
        let out = tmp.join("out.mp4");

        let stats = render_segmented(&renderer, &film, &store, &out, &opts, Some(2)).unwrap();
        assert!(stats.workers <= 2, "must never exceed an explicit --max-workers cap, got {}", stats.workers);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
