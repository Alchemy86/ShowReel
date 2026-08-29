//! Video clips, decoded to frames through ffmpeg.
//!
//! ShowReel shells out to `ffmpeg` rather than linking a codec library. That is
//! a deliberate trade: it costs a subprocess per clip at load, and it buys
//! every container and codec ffmpeg supports, no build-time C dependency, and
//! the same binary we already need for encoding.
//!
//! **Decoding is lazy, bounded by a window, not by film length.** A `Clip`
//! picks one of two backings at [`Clip::load`] time, from the clip's own
//! estimated size — not a knob the caller sets:
//!
//! - **Eager** (`estimated decoded bytes <= EAGER_MAX_BYTES`, 256 MiB): the
//!   whole clip is decoded up front into `Arc<Pixmap>`s, exactly as this
//!   module always worked. A short sting reused across many cuts stays cheap
//!   to keep fully resident — no process bookkeeping, no cache misses.
//! - **Streaming** (bigger than that): frames are pulled from a running
//!   `ffmpeg` child process on demand, in [`StreamState`], and kept in a
//!   bounded cache (sized off the render's own parallelism —
//!   [`stream_cache_frames`] — plus a hard byte ceiling, `STREAM_MEMORY_BUDGET`,
//!   512 MiB). Frames outside that window are decoded again rather than held:
//!   a ten-minute source costs the same peak memory as a ten-second one. A
//!   request that lands behind the window (a `ClipLoop::Loop` wrap, a studio
//!   scrub) restarts the decoder with a fresh `-ss` seek — slower than a
//!   cache hit, never wrong.
//!
//! This is why [`Clip::frame_at`] returns `Result<Option<Arc<Pixmap>>>`
//! rather than the old zero-copy `PixmapRef<'_>`: a streaming clip's frame
//! lives behind a `Mutex`-guarded cache, not in an owned `Vec` borrowed for
//! `self`'s lifetime, and a real decode failure (the file vanished mid-render,
//! `ffmpeg` crashed) can now surface lazily, on whichever frame request
//! triggers it, rather than only at `load` time — callers already thread `?`
//! through their own `Result`-returning draw path, so this costs a `.clone()`
//! of an `Arc` (a refcount bump), not a pixel copy.
//!
//! The renderer's own parallel chunking (`render.rs`) intentionally runs
//! frames in a chunk out of order across worker threads, and a clip whose
//! `decode_fps` samples slower than the film's own fps is asked for the same
//! decoded frame index repeatedly. Both are why the streaming cache is sized
//! to the render's concurrency rather than "a couple of frames": a window
//! that only covered one frame would thrash into a reseek on almost every
//! chunk. [`Clip::load`] still proves the file actually decodes before
//! returning (it pulls frame zero synchronously) — the "loud at load"
//! contract just moves from "every frame decoded" to "the first frame
//! decoded," since decoding every frame up front is the exact cost this
//! module exists to avoid.
//!
//! **The browser build never reaches any of this.** `wasm32-unknown-unknown`
//! has no `ffmpeg` and no filesystem; [`Clip::from_frames`] (fed by
//! `webclip::decode`, unpacking a pre-baked `.srclip`) is the only
//! constructor it uses, and it always produces the Eager backing.

use anyhow::{Context, Result, bail};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use tiny_skia::Pixmap;

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

/// Below this estimated decoded size, [`Clip::load`] decodes eagerly rather
/// than streaming — see the module doc. 256 MiB is a little over half a
/// minute of 1080p30 (see the arithmetic in the module doc's sibling: one
/// second at 1080p30 is ~249 MB), comfortably covering a sting or loop reused
/// across many cuts while keeping the eager path off anything that would
/// itself be a meaningful chunk of a machine's memory.
const EAGER_MAX_BYTES: usize = 256 * 1024 * 1024;

/// Hard ceiling on a streaming clip's frame cache, regardless of how much
/// parallelism [`stream_cache_frames`] would otherwise ask for. Bounds worst
/// case memory for one streaming clip to this, independent of resolution.
const STREAM_MEMORY_BUDGET: usize = 512 * 1024 * 1024;

/// A decoded clip: RGBA frames at a fixed rate, backed eagerly or lazily —
/// see the module doc.
pub struct Clip {
    backing: Backing,
    fps: f64,
    width: u32,
    height: u32,
    source: PathBuf,
}

enum Backing {
    /// The whole clip, already decoded.
    Eager(Vec<Arc<Pixmap>>),
    /// Decoded on demand from a running (or not-yet-spawned) `ffmpeg` child.
    /// Boxed so this variant doesn't blow up `Backing`'s size relative to
    /// `Eager`'s bare `Vec` — `StreamState` carries a `PathBuf` and cache
    /// bookkeeping that `Eager` has no equivalent of.
    Streaming(Box<Mutex<StreamState>>),
}

/// The mutable, lazily-advanced state behind a streaming [`Clip`]. Locked for
/// the duration of one `frame_at` call — decode of a single clip is
/// inherently sequential (one `ffmpeg` process, one pipe) so this only
/// serializes concurrent access *to the same clip*; different clips (and
/// cache hits within this one) don't contend with each other's decode work.
struct StreamState {
    path: PathBuf,
    fps: f64,
    out_w: u32,
    out_h: u32,
    stride: usize,
    /// `(start_secs, duration_secs)` in the clip's own time, as given to
    /// `Clip::load` — reused every time the decoder is (re)spawned.
    trim: Option<(f64, f64)>,
    /// A rough, cheap (probe-only, no decode) upper bound used only to avoid
    /// treating "clearly in range" queries as "might be past the end" — see
    /// `discover_total`. Never trusted for pixel correctness.
    estimated_total: usize,
    /// The real count, known only once a decode has actually hit EOF.
    known_total: Option<usize>,
    decoder: Option<(Child, ChildStdout)>,
    /// The absolute frame index the running decoder will produce next.
    next_index: usize,
    cache: HashMap<usize, Arc<Pixmap>>,
    /// Insertion order, for FIFO eviction once `cache` exceeds `cache_cap`.
    order: VecDeque<usize>,
    cache_cap: usize,
}

impl Clip {
    /// Decode `path` at `fps`, scaled so its width is at most `max_width`.
    ///
    /// `trim` is `(start_secs, duration_secs)` in the clip's own time. Unlike
    /// before, it is no longer the thing standing between a long source and
    /// an out-of-memory render — see the module doc — but it still narrows
    /// what a streaming clip probes as its estimated length, and still keeps
    /// a large source's *un-used* footage from ever being touched at all.
    pub fn load(
        path: impl AsRef<Path>,
        fps: f64,
        max_width: u32,
        trim: Option<(f64, f64)>,
    ) -> Result<Self> {
        Self::load_with_eager_threshold(path, fps, max_width, trim, EAGER_MAX_BYTES)
    }

    /// [`Clip::load`], but with the eager/streaming size threshold overridden
    /// — `pub(crate)` purely so tests can force the streaming path onto a
    /// clip that would otherwise be small enough to decode eagerly, to prove
    /// the two backings agree pixel-for-pixel. Not a knob for production
    /// callers: the real threshold in `load` is the only one that ships.
    pub(crate) fn load_with_eager_threshold(
        path: impl AsRef<Path>,
        fps: f64,
        max_width: u32,
        trim: Option<(f64, f64)>,
        eager_max_bytes: usize,
    ) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            bail!("clip not found: {}", path.display());
        }
        let (w, h) = probe_size(path)?;
        let (out_w, out_h) = scaled_dims(w, h, max_width);
        let stride = (out_w * out_h * 4) as usize;
        if stride == 0 {
            bail!("clip {} decoded to zero-sized frames", path.display());
        }

        let est_secs = match trim {
            Some((_, dur)) => dur,
            None => probe_duration(path)?,
        };
        let estimated_total = (est_secs * fps).round().max(0.0) as usize;
        let estimated_bytes = estimated_total.saturating_mul(stride);

        if estimated_bytes <= eager_max_bytes {
            let frames = decode_eager(path, fps, out_w, out_h, trim)?;
            Ok(Clip { backing: Backing::Eager(frames), fps, width: out_w, height: out_h, source: path.to_path_buf() })
        } else {
            let cache_cap = stream_cache_frames(stride);
            let mut state = StreamState {
                path: path.to_path_buf(),
                fps,
                out_w,
                out_h,
                stride,
                trim,
                estimated_total,
                known_total: None,
                decoder: None,
                next_index: 0,
                cache: HashMap::new(),
                order: VecDeque::new(),
                cache_cap,
            };
            // Prove the file actually decodes now — the one piece of the old
            // "loud at load" guarantee that's still cheap to keep — and prime
            // the pipe so the first real `frame_at` is a cache hit, not a
            // fresh spawn.
            state.ensure_decoder_from(0)?;
            state
                .read_one()?
                .with_context(|| format!("clip {} decoded to no frames", path.display()))?;
            Ok(Clip {
                backing: Backing::Streaming(Box::new(Mutex::new(state))),
                fps,
                width: out_w,
                height: out_h,
                source: path.to_path_buf(),
            })
        }
    }

    /// Build a clip directly from decoded frames, skipping ffmpeg — for tests
    /// and for programmatic films that generate their own footage, and the
    /// only constructor the wasm build ever calls (fed by `webclip::decode`
    /// unpacking a pre-baked `.srclip`). Always eager: these frames are
    /// already in memory, so there is nothing to stream.
    pub fn from_frames(frames: Vec<Pixmap>, fps: f64) -> Self {
        let (width, height) = frames.first().map(|p| (p.width(), p.height())).unwrap_or((1, 1));
        let frames = frames.into_iter().map(Arc::new).collect();
        Clip { backing: Backing::Eager(frames), fps, width, height, source: PathBuf::new() }
    }

    /// The number of frames — exact once known (always, for an eager clip;
    /// after the first past-the-end or wraparound query, for a streaming
    /// one), otherwise a probe-based estimate. Metadata only: see the module
    /// doc for why pixel correctness never depends on this being exact.
    pub fn frame_count(&self) -> usize {
        match &self.backing {
            Backing::Eager(f) => f.len(),
            Backing::Streaming(s) => {
                let st = s.lock().unwrap();
                st.known_total.unwrap_or(st.estimated_total)
            }
        }
    }

    pub fn fps(&self) -> f64 {
        self.fps
    }

    pub fn duration(&self) -> f64 {
        self.frame_count() as f64 / self.fps
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Bytes actually resident right now: the whole decode for an eager
    /// clip, or just the current cache window for a streaming one — a live
    /// reading, not the clip's full theoretical size.
    pub fn memory_bytes(&self) -> usize {
        let frame_bytes = (self.width * self.height * 4) as usize;
        match &self.backing {
            Backing::Eager(f) => f.len() * frame_bytes,
            Backing::Streaming(s) => s.lock().unwrap().cache.len() * frame_bytes,
        }
    }

    /// The frame at `t` seconds into the clip, honouring `mode` past the end.
    ///
    /// Returns `Ok(None)` when there is no frame to show (an empty clip, or
    /// `mode: Stop` past the end) and `Err` only for a genuine decode
    /// failure — see the module doc for why a streaming clip can fail here
    /// rather than only at `load`.
    pub fn frame_at(&self, t: f64, mode: ClipLoop) -> Result<Option<Arc<Pixmap>>> {
        match &self.backing {
            Backing::Eager(frames) => {
                let n = frames.len() as i64;
                let i_raw = (t * self.fps).floor() as i64;
                let Some(i) = clamp_index(i_raw, n, mode) else { return Ok(None) };
                Ok(Some(frames[i as usize].clone()))
            }
            Backing::Streaming(state) => {
                let mut st = state.lock().unwrap();
                let i_raw_signed = (t * self.fps).floor() as i64;
                if i_raw_signed < 0 {
                    // Negative time shows the first frame regardless of
                    // `mode` — matches the eager path via `clamp_index`.
                    return st.get(0);
                }
                let i_raw = i_raw_signed as usize;
                // A little slack against an estimate that ran slightly long:
                // still try the fast, no-total-needed path first, and fall
                // back to actually pinning down the end if it turns out we
                // were wrong (real EOF arrives) or we're close enough it's
                // worth checking properly (`ClipLoop::Hold`/`Loop` need the
                // exact last index, not an estimate).
                let safety = 4usize;
                loop {
                    if let Some(n) = st.known_total {
                        let Some(i) = clamp_index(i_raw as i64, n as i64, mode) else {
                            return Ok(None);
                        };
                        return st.get(i as usize);
                    }
                    if i_raw + safety < st.estimated_total {
                        match st.get(i_raw)? {
                            Some(p) => return Ok(Some(p)),
                            // Estimate ran long: real EOF hit while streaming
                            // toward `i_raw`. `known_total` is now set;
                            // loop back around to apply the mode clamp.
                            None => continue,
                        }
                    }
                    st.discover_total()?;
                }
            }
        }
    }
}

/// Shared by both backings: `i_raw` is the frame index before clamping,
/// `n` the frame count, and the result honours `mode` past the end exactly
/// the way `Clip::frame_at` always has.
fn clamp_index(i_raw: i64, n: i64, mode: ClipLoop) -> Option<i64> {
    if n <= 0 {
        return None;
    }
    if i_raw < 0 {
        return Some(0);
    }
    if i_raw < n {
        return Some(i_raw);
    }
    match mode {
        ClipLoop::Hold => Some(n - 1),
        ClipLoop::Loop => Some(i_raw.rem_euclid(n)),
        ClipLoop::Stop => None,
    }
}

/// Even dimensions: odd ones break yuv420p downstream and cost nothing to
/// avoid here.
fn scaled_dims(w: u32, h: u32, max_width: u32) -> (u32, u32) {
    let scale = if w > max_width && max_width > 0 { max_width as f64 / w as f64 } else { 1.0 };
    let out_w = (((w as f64 * scale) as u32).max(2) / 2) * 2;
    let out_h = (((h as f64 * scale) as u32).max(2) / 2) * 2;
    (out_w, out_h)
}

/// How many frames a streaming clip's cache holds: enough to cover one
/// `render.rs` parallel chunk (`(threads*2).max(2)`, duplicated here rather
/// than imported to avoid a `render` <-> `assets` dependency) plus a margin
/// for `decode_fps`-mismatch reuse, clamped to `[8, 128]` frames and then to
/// whatever `STREAM_MEMORY_BUDGET` allows at this clip's resolution —
/// whichever is smaller wins, so a very high core count or a very large
/// frame can't blow the budget.
fn stream_cache_frames(stride: usize) -> usize {
    #[cfg(feature = "parallel")]
    let concurrency = rayon::current_num_threads() * 2;
    #[cfg(not(feature = "parallel"))]
    let concurrency = 2usize;
    let by_concurrency = (concurrency + 8).clamp(8, 128);
    let by_budget = (STREAM_MEMORY_BUDGET / stride.max(1)).max(4);
    by_concurrency.min(by_budget)
}

fn ffmpeg_decode_command(path: &Path, fps: f64, out_w: u32, out_h: u32, trim: Option<(f64, f64)>) -> Command {
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
    cmd
}

/// The original, whole-file-at-once decode — unchanged behaviour, still used
/// for anything under `EAGER_MAX_BYTES`.
fn decode_eager(path: &Path, fps: f64, out_w: u32, out_h: u32, trim: Option<(f64, f64)>) -> Result<Vec<Arc<Pixmap>>> {
    let mut cmd = ffmpeg_decode_command(path, fps, out_w, out_h, trim);
    let out = cmd.output().with_context(|| format!("running ffmpeg to decode {}", path.display()))?;
    if !out.status.success() {
        bail!("ffmpeg failed decoding {}: {}", path.display(), String::from_utf8_lossy(&out.stderr).trim());
    }

    let stride = (out_w * out_h * 4) as usize;
    let n = out.stdout.len() / stride;
    if n == 0 {
        bail!("clip {} decoded to no frames", path.display());
    }
    let mut frames = Vec::with_capacity(n);
    for i in 0..n {
        let raw = &out.stdout[i * stride..(i + 1) * stride];
        frames.push(Arc::new(pixmap_from_raw(raw, out_w, out_h)?));
    }
    Ok(frames)
}

fn pixmap_from_raw(raw: &[u8], out_w: u32, out_h: u32) -> Result<Pixmap> {
    let mut p = Pixmap::new(out_w, out_h).context("clip frame allocation")?;
    let dst = p.pixels_mut();
    for (j, px) in dst.iter_mut().enumerate() {
        let o = j * 4;
        *px = tiny_skia::ColorU8::from_rgba(raw[o], raw[o + 1], raw[o + 2], raw[o + 3]).premultiply();
    }
    Ok(p)
}

impl StreamState {
    /// (Re)spawn the decoder so its next output frame is absolute index
    /// `idx`. Kills whatever decoder was already running first.
    fn ensure_decoder_from(&mut self, idx: usize) -> Result<()> {
        if let Some((mut child, _)) = self.decoder.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let trim_start = self.trim.map(|(s, _)| s).unwrap_or(0.0);
        let seek = trim_start + idx as f64 / self.fps;
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-nostdin").arg("-loglevel").arg("error");
        cmd.arg("-ss").arg(format!("{seek}"));
        cmd.arg("-i").arg(&self.path);
        if let Some((_, dur)) = self.trim {
            let remaining = (dur - idx as f64 / self.fps).max(0.0);
            cmd.arg("-t").arg(format!("{remaining}"));
        }
        cmd.arg("-vf")
            .arg(format!("fps={},scale={}:{}:flags=bicubic", self.fps, self.out_w, self.out_h))
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgba")
            .arg("-");
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child =
            cmd.spawn().with_context(|| format!("spawning ffmpeg to stream {}", self.path.display()))?;
        let out = child.stdout.take().context("ffmpeg stdout")?;
        self.decoder = Some((child, out));
        self.next_index = idx;
        Ok(())
    }

    /// Wait for the current decoder to exit and check its status, surfacing
    /// a loud error on a real ffmpeg failure — the same check `decode_eager`
    /// makes, just discovered lazily instead of up front.
    ///
    /// Reads whatever stderr is already buffered *after* the child has
    /// exited, not concurrently with reading stdout — deliberately not a
    /// background thread (this module must stay compilable, if unused, on
    /// wasm32-unknown-unknown, which has no `std::thread::spawn`). With
    /// `-loglevel error` the child writes at most a few lines even on
    /// failure, well under a pipe's buffer, so this doesn't deadlock in
    /// practice; a truly pathological amount of stderr output could, and
    /// that's a known, accepted gap rather than a silent one.
    fn finish_decoder(&mut self) -> Result<()> {
        if let Some((mut child, _stdout)) = self.decoder.take() {
            // `_stdout` already hit real EOF (that's why we're here) and is
            // dropped at the end of this block; no need to close it early.
            let status = child.wait().context("waiting for ffmpeg")?;
            if !status.success() {
                let mut buf = Vec::new();
                if let Some(mut se) = child.stderr.take() {
                    let _ = se.read_to_end(&mut buf);
                }
                bail!("ffmpeg failed decoding {}: {}", self.path.display(), String::from_utf8_lossy(&buf).trim());
            }
        }
        Ok(())
    }

    /// Read exactly one more frame from the running decoder, cache it, and
    /// evict the oldest cached frame if that pushes the cache over its cap.
    /// `Ok(None)` means real end of stream (and sets `known_total`); an `Err`
    /// is a genuine decode failure.
    fn read_one(&mut self) -> Result<Option<Arc<Pixmap>>> {
        let Some((_, out)) = self.decoder.as_mut() else { return Ok(None) };
        let mut raw = vec![0u8; self.stride];
        match out.read_exact(&mut raw) {
            Ok(()) => {
                let p = pixmap_from_raw(&raw, self.out_w, self.out_h)?;
                let idx = self.next_index;
                let arc = Arc::new(p);
                self.cache.insert(idx, arc.clone());
                self.order.push_back(idx);
                while self.order.len() > self.cache_cap {
                    if let Some(old) = self.order.pop_front() {
                        self.cache.remove(&old);
                    }
                }
                self.next_index += 1;
                Ok(Some(arc))
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.finish_decoder()?;
                self.known_total = Some(self.next_index);
                Ok(None)
            }
            Err(e) => Err(e).context("reading decoded clip frame"),
        }
    }

    /// Pin down the real frame count by seeking near the estimate and
    /// reading forward to the true end — a handful of frames' cost, not a
    /// full re-decode, and never a seek to a wild/unbounded target (which
    /// would otherwise corrupt `known_total` if it landed past the real end
    /// with zero frames produced).
    fn discover_total(&mut self) -> Result<usize> {
        if let Some(n) = self.known_total {
            return Ok(n);
        }
        let start = self.estimated_total.saturating_sub(8);
        self.ensure_decoder_from(start)?;
        while self.read_one()?.is_some() {}
        Ok(self.known_total.unwrap_or(0))
    }

    /// Fetch absolute frame `idx`, decoding/seeking as needed. Does not
    /// itself apply `ClipLoop`/estimate logic — see `Clip::frame_at`.
    fn get(&mut self, idx: usize) -> Result<Option<Arc<Pixmap>>> {
        if let Some(p) = self.cache.get(&idx) {
            return Ok(Some(p.clone()));
        }
        if self.known_total.is_some_and(|n| idx >= n) {
            return Ok(None);
        }
        let need_seek = self.decoder.is_none() || idx < self.next_index;
        if need_seek {
            self.ensure_decoder_from(idx)?;
        }
        let mut result = None;
        while self.next_index <= idx {
            match self.read_one()? {
                Some(arc) => {
                    if self.next_index - 1 == idx {
                        result = Some(arc);
                    }
                }
                None => break,
            }
        }
        Ok(result)
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
            backing: Backing::Eager((0..n).map(|_| Arc::new(Pixmap::new(2, 2).unwrap())).collect()),
            fps: 10.0,
            width: 2,
            height: 2,
            source: PathBuf::from("<test>"),
        }
    }

    #[test]
    fn hold_pins_the_last_frame_and_loop_wraps() {
        let c = dummy(5); // 0.5s at 10fps
        assert!(c.frame_at(0.0, ClipLoop::Hold).unwrap().is_some());
        assert!(c.frame_at(99.0, ClipLoop::Hold).unwrap().is_some(), "hold never runs out");
        assert!(c.frame_at(99.0, ClipLoop::Loop).unwrap().is_some(), "loop never runs out");
        assert!(c.frame_at(99.0, ClipLoop::Stop).unwrap().is_none(), "stop does run out");
        assert!((c.duration() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn negative_time_shows_the_first_frame() {
        let c = dummy(3);
        assert!(c.frame_at(-1.0, ClipLoop::Stop).unwrap().is_some());
    }

    fn ffmpeg_available() -> bool {
        Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    /// A short synthetic source, generated by ffmpeg itself (`lavfi
    /// testsrc`), so these tests need no committed asset — `testsrc` paints a
    /// different, deterministic pattern every frame, which is exactly what
    /// exercises "did we return the right frame" rather than "did we return
    /// *a* frame".
    fn synth_clip(dir: &Path, name: &str, seconds: f64, fps: f64) -> PathBuf {
        let path = dir.join(name);
        let status = Command::new("ffmpeg")
            .args(["-y", "-nostdin", "-loglevel", "error", "-f", "lavfi"])
            .arg("-i")
            .arg(format!("testsrc=duration={seconds}:size=32x18:rate={fps}"))
            .arg(&path)
            .status()
            .expect("spawning ffmpeg to build a synthetic test clip");
        assert!(status.success(), "ffmpeg failed to build the synthetic test clip");
        path
    }

    #[test]
    fn streaming_matches_eager_frame_for_frame() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not on PATH");
            return;
        }
        let dir = std::env::temp_dir().join("showreel-clip-stream-parity");
        std::fs::create_dir_all(&dir).unwrap();
        let path = synth_clip(&dir, "synth.mp4", 2.0, 10.0);

        let eager = Clip::load_with_eager_threshold(&path, 10.0, 1920, None, usize::MAX).unwrap();
        let streaming = Clip::load_with_eager_threshold(&path, 10.0, 1920, None, 0).unwrap();
        assert!(matches!(eager.backing, Backing::Eager(_)));
        assert!(matches!(streaming.backing, Backing::Streaming(_)));
        assert_eq!(eager.frame_count(), streaming.frame_count());

        // Forward, backward, repeated and past-the-end — including a
        // deliberately scrambled order, standing in for `render.rs`'s
        // out-of-order parallel chunk access.
        let times = [0.0, 0.3, 1.9, 0.1, 5.0, 1.0, 5.0, 0.0, 1.5];
        for &t in &times {
            for mode in [ClipLoop::Hold, ClipLoop::Loop, ClipLoop::Stop] {
                let a = eager.frame_at(t, mode).unwrap();
                let b = streaming.frame_at(t, mode).unwrap();
                match (a, b) {
                    (Some(a), Some(b)) => assert_eq!(a.data(), b.data(), "t={t} mode={mode:?}"),
                    (None, None) => {}
                    other => panic!("t={t} mode={mode:?}: eager/streaming disagree on presence: {other:?}"),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn streaming_cache_stays_bounded_across_a_long_forward_sweep() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not on PATH");
            return;
        }
        let dir = std::env::temp_dir().join("showreel-clip-stream-bounded");
        std::fs::create_dir_all(&dir).unwrap();
        let path = synth_clip(&dir, "synth.mp4", 3.0, 20.0); // 60 frames
        let clip = Clip::load_with_eager_threshold(&path, 20.0, 1920, None, 0).unwrap();
        assert!(matches!(clip.backing, Backing::Streaming(_)));

        let frame_bytes = {
            let (w, h) = clip.size();
            (w * h * 4) as usize
        };
        let mut cache_cap = 0usize;
        for i in 0..60 {
            let t = i as f64 / 20.0;
            clip.frame_at(t, ClipLoop::Stop).unwrap();
            if let Backing::Streaming(s) = &clip.backing {
                cache_cap = s.lock().unwrap().cache_cap;
            }
            assert!(
                clip.memory_bytes() <= cache_cap * frame_bytes,
                "memory grew past the cache cap at frame {i}"
            );
        }
        assert!(cache_cap < 60, "test is meaningless unless the cache is smaller than the whole clip");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
