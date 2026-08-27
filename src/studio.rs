//! A local browser studio: a scrubber and timeline over a real film, live.
//!
//! [`crate::preview`] used to be the whole answer to "look before you render"
//! because a browser was, in that module's own words, a gap that "we cannot
//! have". This module is the browser: `showreel studio <film>` starts a
//! server on localhost and serves a single self-contained page — no CDN, no
//! build step, nothing fetched from the network — that scrubs the film with
//! [`crate::preview::still_at`], shows its scene structure, and reloads
//! itself when the film file changes on disk.
//!
//! # Why polling, not a filesystem watcher
//!
//! A watcher crate (`notify` and friends) pulls in a platform-specific
//! backend for a problem a 350ms `stat` loop solves with zero new
//! dependencies and no missed events across editors that save by
//! write-then-rename. The film file is small; stimulating it eight times a
//! second costs nothing next to the frame renders the same server is doing.
//!
//! # Why polling, not a live connection, for the browser itself
//!
//! The obvious alternative to the browser polling `/api/state` is a
//! WebSocket or an SSE stream the server pushes on. Both hold a connection
//! open per worker thread for [`tiny_http`]'s blocking, one-thread-per-request
//! model, which trades a handful of idle threads for not doing what a
//! 700ms `fetch` already does at a cost nobody can measure on a single local
//! tab.
//!
//! # What is scaled, and why
//!
//! Frames come from the film scaled by `--scale` (via
//! [`crate::scale::scale_film`], the same path `showreel preview` uses) so
//! scrubbing stays responsive on a heavy film; the *structure* — scenes,
//! layers, timings shown in the page — is read from the film exactly as
//! written, because scaling never touches timing and the captain should see
//! his own numbers, not derived ones.
//!
//! # Nothing here knows what a film is about
//!
//! The page renders whatever [`crate::layer::Content`]'s own tag names are
//! (`still`, `clip`, `counter`, ...) — the crate's general vocabulary, not a
//! subject. See `AGENTS.md`.

use crate::assets::AssetStore;
use crate::preview;
use crate::scale::scale_film;
use crate::text::FontDb;
use crate::time::Time;
use crate::timeline::Film;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

const PAGE_TEMPLATE: &str = include_str!("studio/page.html");
const PAGE_CSS: &str = include_str!("studio/page.css");
const PAGE_JS: &str = include_str!("studio/page.js");

/// How the studio server is started.
#[derive(Debug, Clone)]
pub struct StudioOptions {
    pub port: u16,
    pub host: String,
    /// Frames are rendered from the film at this fraction of its declared
    /// size — the same knob `showreel preview` uses, for the same reason.
    pub scale: f64,
}

impl Default for StudioOptions {
    fn default() -> Self {
        StudioOptions { port: 7878, host: "127.0.0.1".into(), scale: 1.0 }
    }
}

/// The film, reloaded. Held behind an `Arc` so an in-flight frame render
/// keeps using the snapshot it started with even if the file changes under
/// it — a request never blocks a reload, and a reload never tears a request.
struct Snapshot {
    version: u64,
    /// The film exactly as written — what the page's structure view reads.
    film: Option<Film>,
    /// The same film through [`scale_film`] — what frames are rendered from.
    preview_film: Option<Film>,
    assets: Arc<AssetStore>,
    parse_error: Option<String>,
    validation_errors: Vec<String>,
}

struct Shared {
    film_path: PathBuf,
    asset_roots: Vec<PathBuf>,
    scale: f64,
    fonts: &'static FontDb,
    snapshot: Mutex<Arc<Snapshot>>,
}

impl Shared {
    fn current(&self) -> Arc<Snapshot> {
        self.snapshot.lock().unwrap().clone()
    }
}

/// A store rooted at the film's own directory, plus whatever else was named —
/// the same rule `showreel render` uses, so a film that renders also previews.
fn asset_store(film_path: &Path, roots: &[PathBuf]) -> AssetStore {
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

fn load(film_path: &Path, roots: &[PathBuf], scale: f64, version: u64) -> Snapshot {
    let assets = Arc::new(asset_store(film_path, roots));
    let src = match std::fs::read_to_string(film_path) {
        Ok(s) => s,
        Err(e) => {
            return Snapshot {
                version,
                film: None,
                preview_film: None,
                assets,
                parse_error: Some(format!("reading {}: {e}", film_path.display())),
                validation_errors: Vec::new(),
            };
        }
    };
    match Film::from_json(&src) {
        Ok(film) => {
            let validation_errors = film.validate();
            let preview_film = Some(scale_film(&film, scale));
            Snapshot { version, film: Some(film), preview_film, assets, parse_error: None, validation_errors }
        }
        Err(e) => Snapshot {
            version,
            film: None,
            preview_film: None,
            assets,
            parse_error: Some(format!("{e}")),
            validation_errors: Vec::new(),
        },
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Polls the film file for changes and swaps in a freshly loaded snapshot —
/// the whole of live reload.
fn watch(shared: Arc<Shared>) {
    let mut last = mtime(&shared.film_path);
    loop {
        thread::sleep(Duration::from_millis(350));
        let now = mtime(&shared.film_path);
        if now == last {
            continue;
        }
        last = now;
        let version = shared.current().version + 1;
        let snap = load(&shared.film_path, &shared.asset_roots, shared.scale, version);
        *shared.snapshot.lock().unwrap() = Arc::new(snap);
    }
}

/// Start the studio server and block forever, serving requests. Returns only
/// on a genuine startup failure (the port is taken, say).
pub fn serve(film_path: PathBuf, asset_roots: Vec<PathBuf>, opts: StudioOptions) -> Result<()> {
    let initial = load(&film_path, &asset_roots, opts.scale, 1);
    if let Some(e) = &initial.parse_error {
        eprintln!("warning: {e}");
        eprintln!("  the studio will still open — fix the file and save to see the film.");
    }
    let shared = Arc::new(Shared {
        film_path: film_path.clone(),
        asset_roots,
        scale: opts.scale,
        fonts: FontDb::shared(),
        snapshot: Mutex::new(Arc::new(initial)),
    });

    {
        let shared = shared.clone();
        thread::spawn(move || watch(shared));
    }

    let addr = format!("{}:{}", opts.host, opts.port);
    let server = tiny_http::Server::http(&addr)
        .map_err(|e| anyhow::anyhow!("could not start the studio server on {addr}: {e}"))?;
    let server = Arc::new(server);

    let display_host = if opts.host == "0.0.0.0" { "localhost" } else { opts.host.as_str() };
    println!("ShowReel Studio — {}", film_path.display());
    println!("  http://{display_host}:{}", opts.port);
    println!("  editing the film reloads the browser automatically. Ctrl-C to stop.");

    // A handful of worker threads recv() concurrently — tiny_http's own
    // pattern for a blocking server without an async runtime. One browser tab
    // scrubbing needs at most a couple of these at once; four leaves room for
    // the page load, the poll and a frame request to overlap.
    const WORKERS: usize = 4;
    let workers: Vec<_> = (1..WORKERS)
        .map(|_| {
            let server = server.clone();
            let shared = shared.clone();
            thread::spawn(move || worker_loop(&server, &shared))
        })
        .collect();
    worker_loop(&server, &shared);
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

fn worker_loop(server: &tiny_http::Server, shared: &Shared) {
    loop {
        match server.recv() {
            Ok(request) => handle(request, shared),
            Err(e) => eprintln!("studio: connection error: {e}"),
        }
    }
}

fn handle(request: tiny_http::Request, shared: &Shared) {
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/");
    let method = request.method().clone();

    let (status, content_type, body): (u16, &str, Vec<u8>) = match (method, path) {
        (tiny_http::Method::Get, "/") => (200, "text/html; charset=utf-8", page_html(shared).into_bytes()),
        (tiny_http::Method::Get, "/api/state") => (200, "application/json", state_json(shared)),
        (tiny_http::Method::Get, "/api/frame") => frame_response(shared, &url),
        _ => (404, "text/plain; charset=utf-8", b"not found".to_vec()),
    };
    respond(request, status, content_type, body);
}

fn respond(request: tiny_http::Request, status: u16, content_type: &str, body: Vec<u8>) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("a static ascii content-type is always a valid header");
    let response = tiny_http::Response::from_data(body).with_status_code(status).with_header(header);
    let _ = request.respond(response);
}

fn page_html(shared: &Shared) -> String {
    let snap = shared.current();
    let title = snap
        .film
        .as_ref()
        .and_then(|f| f.title.clone())
        .unwrap_or_else(|| {
            shared.film_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        });
    PAGE_TEMPLATE
        .replace("__CSS__", PAGE_CSS)
        .replace("__JS__", PAGE_JS)
        .replace("__TITLE__", &html_escape(&title))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[derive(Serialize)]
struct PlacementInfo {
    index: usize,
    start: f64,
    duration: f64,
    end: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StateResponse<'a> {
    version: u64,
    path: String,
    parse_error: Option<&'a str>,
    validation_errors: &'a [String],
    film: Option<&'a Film>,
    duration: f64,
    frame_count: u32,
    placements: Vec<PlacementInfo>,
}

fn state_json(shared: &Shared) -> Vec<u8> {
    let snap = shared.current();
    let placements = snap
        .film
        .as_ref()
        .map(|f| {
            f.timeline
                .placements()
                .iter()
                .map(|p| PlacementInfo {
                    index: p.index,
                    start: p.start.as_secs(),
                    duration: p.duration.as_secs(),
                    end: p.end().as_secs(),
                })
                .collect()
        })
        .unwrap_or_default();
    let body = StateResponse {
        version: snap.version,
        path: shared.film_path.display().to_string(),
        parse_error: snap.parse_error.as_deref(),
        validation_errors: &snap.validation_errors,
        duration: snap.film.as_ref().map(|f| f.duration().as_secs()).unwrap_or(0.0),
        frame_count: snap.film.as_ref().map(|f| f.frame_count()).unwrap_or(0),
        film: snap.film.as_ref(),
        placements,
    };
    serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec())
}

fn frame_response(shared: &Shared, url: &str) -> (u16, &'static str, Vec<u8>) {
    let snap = shared.current();
    let Some(preview_film) = &snap.preview_film else {
        return (409, "text/plain; charset=utf-8", b"the film file does not currently parse".to_vec());
    };
    let t = query_param(url, "t").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let t = t.clamp(0.0, preview_film.duration().as_secs().max(0.0));
    let render = preview::still_at(preview_film, &snap.assets, shared.fonts, Time(t))
        .context("rendering the frame")
        .and_then(|canvas| canvas.encode_png().context("encoding the frame as PNG"));
    match render {
        Ok(bytes) => (200, "image/png", bytes),
        Err(e) => (500, "text/plain; charset=utf-8", format!("{e:#}").into_bytes()),
    }
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == key).then(|| percent_decode(v))
    })
}

/// Just enough percent-decoding for numeric query values: `+` and `%XX`.
/// Operates on bytes throughout, so malformed input cannot land a slice on
/// something other than a UTF-8 boundary.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 3 <= bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding_handles_plus_and_hex_escapes() {
        assert_eq!(percent_decode("4.2"), "4.2");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("100%25"), "100%");
        // A truncated escape at the end is left alone rather than panicking.
        assert_eq!(percent_decode("abc%2"), "abc%2");
    }

    #[test]
    fn query_param_reads_the_named_value() {
        assert_eq!(query_param("/api/frame?t=4.2&v=3", "t").as_deref(), Some("4.2"));
        assert_eq!(query_param("/api/frame?t=4.2&v=3", "v").as_deref(), Some("3"));
        assert_eq!(query_param("/api/frame", "t"), None);
    }

    #[test]
    fn a_film_that_fails_to_parse_yields_no_preview_but_a_readable_error() {
        let dir = std::env::temp_dir().join("showreel-studio-test-bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.film.json");
        std::fs::write(&path, "{ not json").unwrap();
        let snap = load(&path, &[], 1.0, 1);
        assert!(snap.film.is_none());
        assert!(snap.preview_film.is_none());
        assert!(snap.parse_error.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_valid_film_loads_with_both_the_full_and_the_preview_copy() {
        let dir = std::env::temp_dir().join("showreel-studio-test-good");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("good.film.json");
        let film = crate::timeline::Film::new(64, 36, 10.0)
            .open(crate::timeline::Scene::new(1.0).named("only"));
        std::fs::write(&path, film.to_json().unwrap()).unwrap();
        let snap = load(&path, &[], 0.5, 1);
        assert!(snap.parse_error.is_none());
        assert_eq!(snap.film.as_ref().unwrap().width, 64);
        // The preview copy is the scaled one.
        assert_eq!(snap.preview_film.as_ref().unwrap().width, 32);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
