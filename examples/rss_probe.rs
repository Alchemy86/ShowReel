//! Standalone memory/time probe for `Clip::load` / `Clip::frame_at`, used to
//! measure real RSS before/after the streaming-decode change — see
//! `docs/clip-streaming.md`. Not part of the crate's normal example set.
//!
//! Usage: rss_probe <path> <fps> <max_width> [trim_start trim_dur]

use showreel::assets::{Clip, ClipLoop};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let fps: f64 = args[2].parse().unwrap();
    let max_width: u32 = args[3].parse().unwrap();
    let trim = if args.len() > 5 {
        Some((args[4].parse().unwrap(), args[5].parse().unwrap()))
    } else {
        None
    };

    let start = Instant::now();
    let clip = Clip::load(path, fps, max_width, trim).expect("clip load failed");
    let load_time = start.elapsed();

    // Walk every frame sequentially, forward, once — the one access pattern
    // every backing (old eager-only, or new eager/streaming) must support
    // cheaply, so the walk cost is comparable across builds.
    let n = clip.frame_count();
    let walk_start = Instant::now();
    let mut touched = 0u64;
    for i in 0..n {
        let t = i as f64 / fps;
        if let Some(f) = clip.frame_at(t, ClipLoop::Hold).expect("frame_at failed") {
            touched += f.data().len() as u64;
        }
    }
    let walk_time = walk_start.elapsed();

    eprintln!(
        "frames={} load_ms={} walk_ms={} bytes_touched={} reported_memory_bytes={}",
        n,
        load_time.as_millis(),
        walk_time.as_millis(),
        touched,
        clip.memory_bytes()
    );
}
