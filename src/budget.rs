//! A machine-aware, cross-process render budget.
//!
//! `src/segments.rs` used to render every segment strictly serially,
//! reasoning about neither the machine's real memory nor the fact a second
//! `showreel render` might be running at the same time. The measured cost of
//! that gap, 2026-08-29: two concurrent `render` invocations over a large
//! source (a 9248x1568 canvas, ~400MB 60fps footage) drove the kernel OOM
//! killer, which killed 45 processes — the whole desktop session, not just
//! the render — and rebooted the machine. The same evening, a *single*
//! camera pass measured at ~60% of one core on a 20-core box: this crate was
//! simultaneously capable of starving the machine of memory and leaving
//! nearly all its CPU idle, because nothing owned a resource budget.
//!
//! This module is that budget, in three pieces:
//!
//! - [`MachineState::probe`] reads the machine's *real* available memory and
//!   core count (`/proc/meminfo` on Linux; a conservative, loudly-flagged
//!   fallback elsewhere) rather than trusting a guess.
//! - [`PeakSampler`]/[`measure`] read system-wide available memory in the
//!   background, not a per-process RSS: the process the incident's kernel log
//!   actually killed was `ffmpeg`, a *child* this crate spawns for every
//!   decode and encode, so a reading scoped to `showreel`'s own process would
//!   miss most of what a render actually costs. [`segments::render_segmented`]
//!   uses `measure` to find out what one real segment worker costs on the
//!   film actually being rendered — not an invented constant — before
//!   deciding how many more to run at once.
//! - [`Ledger`] is a small advisory, file-locked record under the system temp
//!   directory that every concurrent `showreel render` reads and writes, so a
//!   *second* render sees what the first has already claimed and shrinks its
//!   own plan accordingly — the two-invocations-at-once shape that actually
//!   caused the incident. It is deliberately not a daemon: a `std::fs::File`
//!   advisory lock (stable since Rust 1.89) around a small JSON file, with
//!   every entry pruned once its process is no longer alive (checked via
//!   `/proc/<pid>` on Linux) or hasn't been refreshed in `STALE_AFTER` — so a
//!   crash (a `kill -9`, the very reboot this module exists to prevent)
//!   leaves nothing that jams a later render.
//!
//! [`plan_workers`] is the pure arithmetic tying them together — how many
//! concurrent segment workers fit in a byte budget — kept free of any I/O so
//! it can be tested directly rather than only through a real render.
//!
//! **Scope cut, matching `src/segments.rs`'s own**: only the default
//! whole-film segmented render goes through this budget. A `--frames`
//! sub-range or `--png` dump (debugging/inspection paths, not the
//! long-render-that-eats-the-machine case this exists for) still render
//! through the plain, unbounded `Renderer::render_range` — see that module's
//! doc for why those two are exempt from the resumable-segments machinery
//! too.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// However much memory is currently available, only this fraction of it is
/// ever planned against — the margin that keeps the desktop session alive
/// even when a render's own measurement turns out optimistic.
const SAFETY_FRACTION: f64 = 0.7;

/// On top of `SAFETY_FRACTION`, this much of currently-available memory is
/// never touched at all, regardless of how little is available — a floor
/// under the fraction, not instead of it.
const MIN_HEADROOM_BYTES: u64 = 512 * 1024 * 1024;

/// A worker is never planned as costing less than this — a botched
/// measurement (a first segment that finished before the sampler took a
/// single reading) must not be read as "free", which would grant unbounded
/// concurrency.
const MIN_WORKER_BYTES: u64 = 64 * 1024 * 1024;

/// If available memory falls below this floor *during* a render, a worker
/// stops picking up further segments rather than pressing on — the hard
/// ceiling the brief asks for, checked continuously, not just planned for
/// once up front. Roughly 8% of the machine's total memory, or 512 MiB,
/// whichever is bigger.
fn critical_floor(total_bytes: u64) -> u64 {
    (total_bytes / 12).max(MIN_HEADROOM_BYTES)
}

/// Real memory and core counts for whatever machine this is running on.
#[derive(Debug, Clone, Copy)]
pub struct MachineState {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub cores: usize,
}

impl MachineState {
    /// Reads `/proc/meminfo` on Linux — tightened by the *cgroup's* own
    /// memory ceiling when this process is running inside one that's
    /// actually constrained (see [`read_cgroup_memory`]), since a
    /// `systemd-run --scope -p MemoryMax=`/container cap changes nothing
    /// `/proc/meminfo` reports. Elsewhere — no `/proc`, e.g. macOS — there is
    /// currently no probe, so this assumes a small, fixed budget and says so
    /// on stderr; a real probe for another platform is a documented gap, not
    /// a silent guess dressed up as a measurement.
    pub fn probe() -> Self {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let Some((mut total, mut available)) = read_meminfo() else {
            eprintln!(
                "  budget  could not read /proc/meminfo (not Linux, or unreadable) — \
                 assuming 2 GiB available and rendering serially"
            );
            let two_gib = 2 * 1024 * 1024 * 1024;
            return MachineState { total_bytes: two_gib, available_bytes: two_gib, cores };
        };
        if let Some((cgroup_max, cgroup_available)) = read_cgroup_memory() {
            total = total.min(cgroup_max);
            available = available.min(cgroup_available);
        }
        MachineState { total_bytes: total, available_bytes: available, cores }
    }
}

/// This process's own cgroup v2 memory ceiling and headroom — `(max,
/// max - current)` — or `None` when there isn't an actual limit to see
/// (no cgroup, cgroup v1, cgroup v2 present but `memory.max` is the literal
/// `"max"`/unlimited, or the files aren't readable). Read fresh every call:
/// this is cheap (two small file reads) and a limit set by an external
/// `systemd-run`/container wrapper doesn't change for the life of the
/// process, but nothing here assumes that.
fn read_cgroup_memory() -> Option<(u64, u64)> {
    let cgroup_line = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    // cgroup v2's unified hierarchy is always the "0::" line; cgroup v1's
    // per-controller lines (needed for anything with a non-"0::" prefix)
    // aren't handled — a v1-only host reports no cgroup ceiling here, the
    // same as no cgroup at all.
    let suffix = cgroup_line.lines().find_map(|l| l.strip_prefix("0::"))?;
    let base = format!("/sys/fs/cgroup{suffix}");
    let max_raw = std::fs::read_to_string(format!("{base}/memory.max")).ok()?;
    let max_raw = max_raw.trim();
    if max_raw == "max" {
        return None;
    }
    let max: u64 = max_raw.parse().ok()?;
    let current: u64 = std::fs::read_to_string(format!("{base}/memory.current")).ok()?.trim().parse().ok()?;
    Some((max, max.saturating_sub(current)))
}

/// However much of `available_bytes` a render may plan against — see
/// `SAFETY_FRACTION`/`MIN_HEADROOM_BYTES`.
pub fn usable_bytes(available_bytes: u64) -> u64 {
    let after_floor = available_bytes.saturating_sub(MIN_HEADROOM_BYTES);
    (after_floor as f64 * SAFETY_FRACTION) as u64
}

#[cfg(target_os = "linux")]
fn read_meminfo() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = parse_kb(rest);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = parse_kb(rest);
        }
    }
    Some((total?, available?))
}

#[cfg(not(target_os = "linux"))]
fn read_meminfo() -> Option<(u64, u64)> {
    None
}

fn parse_kb(s: &str) -> Option<u64> {
    let s = s.trim().strip_suffix("kB").unwrap_or(s).trim();
    s.parse::<u64>().ok().map(|kb| kb * 1024)
}

/// The machine's currently available memory, right now — tightened by the
/// enclosing cgroup's own headroom when there is one (see
/// [`read_cgroup_memory`]), same as [`MachineState::probe`]. `None` on a
/// platform [`read_meminfo`] has no probe for.
pub fn available_bytes_now() -> Option<u64> {
    let host = read_meminfo().map(|(_, available)| available)?;
    let cgroup = read_cgroup_memory().map(|(_, available)| available);
    Some(match cgroup {
        Some(cgroup_available) => host.min(cgroup_available),
        None => host,
    })
}

/// Is available memory below [`critical_floor`] right now? Checked by a
/// concurrent worker before it takes on another segment — the hard ceiling
/// that degrades *during* a render, not just in the up-front plan. A
/// platform with no probe never reports critical (there is nothing false to
/// hide behind that would otherwise silently stop workers early).
pub fn is_critical(total_bytes: u64) -> bool {
    available_bytes_now().is_some_and(|avail| avail < critical_floor(total_bytes))
}

/// Samples system-wide available memory in the background and tracks the
/// lowest value seen — i.e. the most memory actually consumed relative to
/// where sampling started. Deliberately a *system-wide* reading, not a
/// per-process RSS: the process the 2026-08-29 incident's kernel log
/// actually names as killed is `ffmpeg`, a child this crate spawns per
/// decode/encode, so a reading scoped to `showreel`'s own process would miss
/// most of what a render costs.
pub struct PeakSampler {
    stop: Arc<AtomicBool>,
    min_available: Arc<AtomicU64>,
    baseline: u64,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl PeakSampler {
    pub fn start(interval: Duration) -> Self {
        let baseline = available_bytes_now().unwrap_or(u64::MAX);
        let stop = Arc::new(AtomicBool::new(false));
        let min_available = Arc::new(AtomicU64::new(baseline));
        let (stop_thread, min_thread) = (stop.clone(), min_available.clone());
        let handle = std::thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                if let Some(avail) = available_bytes_now() {
                    min_thread.fetch_min(avail, Ordering::Relaxed);
                }
                std::thread::sleep(interval);
            }
        });
        PeakSampler { stop, min_available, baseline, handle: Some(handle) }
    }

    /// Bytes consumed at the worst point seen since `start` — 0 if nothing
    /// ever dropped below the baseline (memory can rise during a render, e.g.
    /// the OS reclaiming cache elsewhere, without this render having used
    /// less than zero) and 0 on a platform with no probe at all.
    fn peak_used_bytes(&self) -> u64 {
        if self.baseline == u64::MAX {
            return 0;
        }
        self.baseline.saturating_sub(self.min_available.load(Ordering::Relaxed))
    }

    /// Stops sampling and returns the peak — blocks briefly for the
    /// background thread's current sleep to end.
    pub fn stop(mut self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.peak_used_bytes()
    }
}

impl Drop for PeakSampler {
    fn drop(&mut self) {
        // Only reached if `stop` was never called (an early return/`?`
        // above it) — still tell the background thread to end rather than
        // leaking it.
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Runs `work` while sampling system-wide available memory, returning its
/// result alongside the memory it cost at its worst point. The sampler is
/// always stopped (joined) before propagating `work`'s error, so a failing
/// segment never leaks the sampling thread.
pub fn measure<T>(interval: Duration, work: impl FnOnce() -> Result<T>) -> Result<(T, u64)> {
    let sampler = PeakSampler::start(interval);
    let result = work();
    let used = sampler.stop();
    Ok((result?, used))
}

/// How many concurrent segment workers fit `available_for_us` bytes, each
/// costing `per_worker_bytes` — never more than `cores` (no point exceeding
/// them for CPU-bound raster work, and every worker beyond the shared decode
/// bandwidth just adds contention) nor more than `remaining` (no point idle
/// workers). Always at least 1: a render must still complete, serially if it
/// must, never refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerPlan {
    pub workers: usize,
    /// True when memory, not core count or remaining work, is what capped
    /// this plan below what cores/remaining would otherwise have allowed —
    /// what decides whether the "running serial, only headroom" message
    /// belongs on stderr.
    pub memory_bound: bool,
}

pub fn plan_workers(available_for_us: u64, per_worker_bytes: u64, cores: usize, remaining: usize) -> WorkerPlan {
    let per_worker = per_worker_bytes.max(MIN_WORKER_BYTES);
    let by_memory = available_for_us / per_worker;
    let uncapped = by_memory.max(1) as usize;
    let workers = uncapped.min(cores.max(1)).min(remaining.max(1));
    WorkerPlan { workers, memory_bound: by_memory == 0 }
}

/// One render's claim on the machine, as the ledger sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    pid: u32,
    reserved_bytes: u64,
    /// Unix seconds, refreshed by [`Reservation::touch`] on every segment
    /// checkpoint — what lets a crashed holder's entry be pruned without
    /// ever needing to positively confirm it is gone (see `process_alive`,
    /// which is Linux-only; this timestamp is what protects every platform).
    updated_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LedgerData {
    entries: Vec<Entry>,
}

/// An entry is treated as abandoned once it hasn't been refreshed in this
/// long. Comfortably longer than one segment (8s of film time, even at a
/// slow fraction of realtime) so a legitimately slow segment is never
/// mistaken for a stale one — `render_segmented` refreshes well inside this
/// on every segment checkpoint.
const STALE_AFTER: Duration = Duration::from_secs(15 * 60);

/// A small, advisory, file-locked record of every `showreel render`
/// currently claiming machine memory. Not a daemon — a JSON file next to a
/// lock file, read-modified-written under an exclusive `std::fs::File` lock
/// held only for that critical section, never across an actual render. See
/// the module doc.
#[derive(Clone)]
pub struct Ledger {
    lock_path: PathBuf,
    data_path: PathBuf,
}

impl Ledger {
    /// The machine-wide ledger, under the system temp directory — shared by
    /// every `showreel render` on this machine, which is the entire point.
    pub fn open() -> Self {
        Self::at(&std::env::temp_dir())
    }

    /// A ledger rooted at an arbitrary directory — the seam tests use to
    /// avoid ever touching the real, shared, machine-wide path.
    pub fn at(dir: &Path) -> Self {
        Ledger {
            lock_path: dir.join("showreel-render-budget.lock"),
            data_path: dir.join("showreel-render-budget.json"),
        }
    }

    fn with_lock<T>(&self, f: impl FnOnce(&mut LedgerData) -> T) -> Result<T> {
        if let Some(dir) = self.lock_path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let lock_file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.lock_path)
            .with_context(|| format!("opening {}", self.lock_path.display()))?;
        lock_file.lock().with_context(|| format!("locking {}", self.lock_path.display()))?;
        let mut data = self.read().unwrap_or_default();
        prune_stale(&mut data);
        let result = f(&mut data);
        self.write(&data)?;
        let _ = lock_file.unlock();
        Ok(result)
    }

    fn read(&self) -> Option<LedgerData> {
        let bytes = std::fs::read(&self.data_path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write(&self, data: &LedgerData) -> Result<()> {
        let bytes = serde_json::to_vec(data).context("serialising the render budget ledger")?;
        std::fs::write(&self.data_path, bytes).with_context(|| format!("writing {}", self.data_path.display()))
    }

    /// Claims `bytes` for `pid` (replacing any existing entry for it — a
    /// second call refines rather than duplicates), and returns the sum
    /// reserved by every *other* live entry: what the caller must treat as
    /// already spoken for.
    fn reserve(&self, pid: u32, bytes: u64) -> Result<u64> {
        self.with_lock(|data| {
            data.entries.retain(|e| e.pid != pid);
            let other: u64 = data.entries.iter().map(|e| e.reserved_bytes).sum();
            data.entries.push(Entry { pid, reserved_bytes: bytes, updated_at: now_secs() });
            other
        })
    }

    fn touch(&self, pid: u32, bytes: u64) -> Result<()> {
        self.with_lock(|data| {
            match data.entries.iter_mut().find(|e| e.pid == pid) {
                Some(e) => {
                    e.reserved_bytes = bytes;
                    e.updated_at = now_secs();
                }
                // Our own entry was pruned (we overran STALE_AFTER) — put it
                // back rather than silently rendering unreserved.
                None => data.entries.push(Entry { pid, reserved_bytes: bytes, updated_at: now_secs() }),
            }
        })
    }

    fn release(&self, pid: u32) -> Result<()> {
        self.with_lock(|data| data.entries.retain(|e| e.pid != pid))
    }
}

fn prune_stale(data: &mut LedgerData) {
    let now = now_secs();
    data.entries.retain(|e| now.saturating_sub(e.updated_at) < STALE_AFTER.as_secs() && process_alive(e.pid));
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(target_os = "linux"))]
fn process_alive(_pid: u32) -> bool {
    // No portable liveness check without a new dependency; `STALE_AFTER` is
    // what actually protects a non-Linux machine from a stale entry here,
    // just on a longer fuse than Linux's immediate `/proc` check.
    true
}

/// Releases this reservation's ledger entry when dropped — so a render that
/// returns an error, panics, or simply falls off the end of a scope still
/// frees its claim, not just the success path.
pub struct Reservation {
    ledger: Ledger,
    pid: u32,
}

impl Reservation {
    /// Refreshes this reservation's byte count and heartbeat timestamp.
    /// `render_segmented` calls this on every segment checkpoint so a
    /// genuinely long render is never mistaken for an abandoned one — see
    /// `STALE_AFTER`.
    pub fn touch(&self, bytes: u64) {
        if let Err(e) = self.ledger.touch(self.pid, bytes) {
            eprintln!("  budget  could not refresh the render budget ledger: {e:#}");
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let _ = self.ledger.release(self.pid);
    }
}

/// Reserves `bytes` for the current process against `ledger`, returning the
/// guard (release on drop) and how much every other *live* render on this
/// machine has already claimed.
pub fn reserve(ledger: &Ledger, bytes: u64) -> Result<(Reservation, u64)> {
    let pid = std::process::id();
    let other = ledger.reserve(pid, bytes)?;
    Ok((Reservation { ledger: ledger.clone(), pid }, other))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo_kb_lines() {
        assert_eq!(parse_kb("   16384 kB"), Some(16384 * 1024));
        assert_eq!(parse_kb("0 kB"), Some(0));
        assert_eq!(parse_kb("not a number kB"), None);
    }

    #[test]
    fn usable_bytes_leaves_a_floor_and_a_fraction() {
        // 10 GiB available: floor off 512 MiB, then 70% of the rest.
        let ten_gib = 10 * 1024 * 1024 * 1024;
        let usable = usable_bytes(ten_gib);
        assert!(usable < ten_gib, "must leave something on the table");
        let expected = ((ten_gib - MIN_HEADROOM_BYTES) as f64 * SAFETY_FRACTION) as u64;
        assert_eq!(usable, expected);
    }

    #[test]
    fn usable_bytes_never_goes_negative_on_a_tight_machine() {
        assert_eq!(usable_bytes(0), 0);
        assert_eq!(usable_bytes(100), 0);
    }

    #[test]
    fn plenty_of_memory_plans_up_to_cores_or_remaining_work() {
        let plan = plan_workers(64 * 1024 * 1024 * 1024, 256 * 1024 * 1024, 8, 20);
        assert_eq!(plan.workers, 8, "capped by cores, not memory");
        assert!(!plan.memory_bound);

        let plan = plan_workers(64 * 1024 * 1024 * 1024, 256 * 1024 * 1024, 8, 3);
        assert_eq!(plan.workers, 3, "capped by remaining segments, not memory or cores");
        assert!(!plan.memory_bound);
    }

    #[test]
    fn tight_memory_degrades_to_serial_and_says_so() {
        // Only 100 MiB free, but a worker measured at 2 GiB.
        let plan = plan_workers(100 * 1024 * 1024, 2 * 1024 * 1024 * 1024, 20, 20);
        assert_eq!(plan.workers, 1, "must still render — serially — never refuse");
        assert!(plan.memory_bound, "the reason must be memory, not core/segment count");
    }

    #[test]
    fn memory_for_exactly_a_handful_of_workers_plans_exactly_that_many() {
        let per_worker = 512 * 1024 * 1024u64;
        let plan = plan_workers(per_worker * 4, per_worker, 20, 20);
        assert_eq!(plan.workers, 4);
        assert!(!plan.memory_bound);
    }

    #[test]
    fn a_zero_cost_measurement_is_never_read_as_unlimited_concurrency() {
        // A segment that finished before the sampler ever took a reading
        // must not be treated as "costs nothing" -- MIN_WORKER_BYTES floors
        // it, so a huge memory budget still plans a bounded worker count.
        let plan = plan_workers(1024 * 1024 * 1024 * 1024, 0, 4, 1000);
        assert_eq!(plan.workers, 4, "capped by cores despite a zero measurement");
    }

    fn unique_tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("showreel-budget-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A real, briefly-alive child process, standing in for an "other"
    /// render in a test — `process_alive` prunes any entry whose pid isn't
    /// really running, so a fabricated pid number would be pruned on the
    /// very next ledger operation regardless of what the test means to
    /// check. Killed and reaped when dropped.
    struct FakeRender(std::process::Child);
    impl FakeRender {
        fn spawn() -> Self {
            FakeRender(std::process::Command::new("sleep").arg("30").spawn().expect("spawn a stand-in process"))
        }
        fn pid(&self) -> u32 {
            self.0.id()
        }
    }
    impl Drop for FakeRender {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn a_second_reservation_sees_the_first_and_releases_free_it() {
        // Deliberately isolated from the real, shared `/tmp` ledger path —
        // never exercise this against the machine-wide file a real
        // concurrent render might be relying on.
        let dir = unique_tmp_dir("reserve-release");
        let ledger = Ledger::at(&dir);
        let (a, b, c, d) = (FakeRender::spawn(), FakeRender::spawn(), FakeRender::spawn(), FakeRender::spawn());

        let other = ledger.reserve(a.pid(), 1_000_000).unwrap();
        assert_eq!(other, 0, "nothing else claimed yet");

        let other = ledger.reserve(b.pid(), 2_000_000).unwrap();
        assert_eq!(other, 1_000_000, "must see a's claim");

        ledger.release(a.pid()).unwrap();
        let other = ledger.reserve(c.pid(), 500_000).unwrap();
        assert_eq!(other, 2_000_000, "a is gone, b remains");

        ledger.release(b.pid()).unwrap();
        ledger.release(c.pid()).unwrap();
        let other = ledger.reserve(d.pid(), 1).unwrap();
        assert_eq!(other, 0, "everyone released");
        ledger.release(d.pid()).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_pids_reservation_is_pruned_even_without_a_release() {
        let dir = unique_tmp_dir("prune-dead");
        let ledger = Ledger::at(&dir);
        // PID 1 always exists on a real Linux box (init); a PID this large
        // is vanishingly unlikely to be a live process, simulating a crash
        // that never got to release its own entry.
        let dead_pid = 999_999_999u32;
        ledger.reserve(dead_pid, 5_000_000).unwrap();

        let other = ledger.reserve(std::process::id(), 1).unwrap();
        if cfg!(target_os = "linux") {
            assert_eq!(other, 0, "a dead pid's reservation must be pruned on Linux (checked via /proc)");
        }
        ledger.release(std::process::id()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn touch_refreshes_without_disturbing_other_entries() {
        let dir = unique_tmp_dir("touch");
        let ledger = Ledger::at(&dir);
        let (a, b, c) = (FakeRender::spawn(), FakeRender::spawn(), FakeRender::spawn());
        ledger.reserve(a.pid(), 100).unwrap();
        ledger.reserve(b.pid(), 200).unwrap();
        ledger.touch(a.pid(), 150).unwrap();
        let other = ledger.reserve(c.pid(), 1).unwrap();
        assert_eq!(other, 150 + 200, "a's touched value must be reflected, b untouched");
        ledger.release(a.pid()).unwrap();
        ledger.release(b.pid()).unwrap();
        ledger.release(c.pid()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reservation_guard_releases_on_drop() {
        let dir = unique_tmp_dir("guard-drop");
        let ledger = Ledger::at(&dir);
        {
            let (_guard, other) = reserve(&ledger, 42).unwrap();
            assert_eq!(other, 0);
        }
        // The guard is gone; a fresh reservation must see nothing left.
        let other = ledger.reserve(std::process::id(), 1).unwrap();
        assert_eq!(other, 0, "the dropped guard's entry must be released");
        ledger.release(std::process::id()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn measuring_work_returns_its_result_even_if_memory_never_moves() {
        let (value, _peak) = measure(Duration::from_millis(5), || -> Result<i32> {
            std::thread::sleep(Duration::from_millis(20));
            Ok(7)
        })
        .unwrap();
        assert_eq!(value, 7);
    }

    #[test]
    fn measuring_propagates_a_failing_workload_without_leaking_the_sampler() {
        let result = measure(Duration::from_millis(5), || -> Result<()> { anyhow::bail!("boom") });
        assert!(result.is_err());
    }
}
