//! The local HTTP API, end to end over a real socket — not just the
//! in-process unit tests in `src/studio.rs`, which exercise the JSON-building
//! functions directly but never prove the routing, the HTTP framing or a real
//! `tiny_http` response actually reach a client. `#![cfg(feature = "studio")]`
//! because the API lives on the studio server; this file compiles to nothing
//! (and `cargo test` with no flags reports zero tests here) without it.

#![cfg(feature = "studio")]

use showreel::prelude::*;
use showreel::studio::{StudioOptions, serve};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

/// A film simple enough to need no assets, but real enough to render.
fn film() -> Film {
    Film::new(64, 36, 10.0).title("api test film").open(
        Scene::new(2.0)
            .named("only")
            .layer(Layer::solid(Color::rgb(20, 40, 200)))
            .layer(Layer::title("Hello").subtitle("from the API")),
    )
}

/// Starts a real studio server on an OS-assigned loopback port and returns it
/// once a connection succeeds, so callers never race the server's own
/// startup.
fn start_server() -> (u16, PathBuf, PathBuf) {
    let dir = std::env::temp_dir()
        .join(format!("showreel-api-server-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let film_path = dir.join("film.json");
    std::fs::write(&film_path, film().to_json().unwrap()).unwrap();

    // Claim a free port, then release it immediately for the real server to
    // bind — a small, standard race that is fine for a local test.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();

    let serve_path = film_path.clone();
    std::thread::spawn(move || {
        let _ = serve(
            serve_path,
            Vec::new(),
            StudioOptions { port, host: "127.0.0.1".into(), scale: 1.0 },
        );
    });

    for _ in 0..40 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return (port, dir, film_path);
        }
        std::thread::sleep(Duration::from_millis(75));
    }
    panic!("studio server never started listening on 127.0.0.1:{port}");
}

/// A minimal HTTP/1.1 client: request `Connection: close` and read to EOF,
/// which sidesteps needing to understand chunked encoding or track
/// Content-Length by hand.
fn request(port: u16, method: &str, path: &str) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();

    let split_at = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("no header/body split");
    let head = String::from_utf8_lossy(&raw[..split_at]);
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .expect("no status line");
    (status, raw[split_at + 4..].to_vec())
}

#[test]
fn info_check_and_still_all_answer_over_a_real_socket() {
    let (port, dir, _film_path) = start_server();

    let (status, body) = request(port, "GET", "/api/info");
    assert_eq!(status, 200);
    let info: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(info["title"], "api test film");
    assert_eq!(info["frameCount"], 20);
    assert_eq!(info["scenes"][0]["name"], "only");

    let (status, body) = request(port, "GET", "/api/check");
    assert_eq!(status, 200);
    let check: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(check["ok"], true, "{check}");

    let (status, body) = request(port, "GET", "/api/still?at=1.0");
    assert_eq!(status, 200);
    assert_eq!(&body[1..4], b"PNG", "not a PNG: {} bytes", body.len());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_streams_back_a_real_mp4() {
    if !showreel::encode::ffmpeg_available() {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    }
    let (port, dir, _film_path) = start_server();

    let (status, body) = request(port, "POST", "/api/render?mobile=1");
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert!(body.len() > 1000, "an mp4 this short is suspicious: {} bytes", body.len());
    // The ftyp box near the start of any mp4/mov container.
    assert!(body.windows(4).any(|w| w == b"ftyp"), "does not look like an mp4");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_paths_are_a_clean_404_not_a_server_error() {
    let (port, dir, _film_path) = start_server();
    let (status, _) = request(port, "GET", "/api/nope");
    assert_eq!(status, 404);
    let _ = std::fs::remove_dir_all(&dir);
}
