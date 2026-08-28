//! The MCP server, end to end over real stdio pipes — spawns the actual
//! `showreel mcp` binary (not an in-process handle) and speaks the real
//! newline-delimited JSON-RPC wire format, the same way any MCP client
//! would. `#![cfg(feature = "mcp")]` because the subcommand only exists
//! with that feature; `cargo test` with no flags reports zero tests here.

#![cfg(feature = "mcp")]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Duration;

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: i64,
}

impl Server {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_showreel"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn `showreel mcp`");
        let stdin = Some(child.stdin.take().unwrap());
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut server = Server { child, stdin, stdout, next_id: 1 };
        server.initialize();
        server
    }

    fn send(&mut self, value: serde_json::Value) {
        let stdin = self.stdin.as_mut().expect("stdin already closed");
        let line = serde_json::to_string(&value).unwrap();
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    /// Closes the pipe to the server — the normal way an MCP client
    /// disconnects, and what should make `showreel mcp` exit on its own.
    fn close_stdin(&mut self) {
        self.stdin = None;
    }

    fn recv(&mut self) -> serde_json::Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read a response line");
        assert!(!line.trim().is_empty(), "server closed its stdout unexpectedly");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not valid JSON: {e}: {line:?}"))
    }

    fn initialize(&mut self) {
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "mcp-server-test", "version": "0.0.0" }
            }
        }));
        let resp = self.recv();
        assert!(resp.get("result").is_some(), "initialize failed: {resp}");
        self.send(serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    }

    /// Calls a tool and returns its `result.content` array, or panics with
    /// the JSON-RPC error if the call itself failed.
    fn call(&mut self, name: &str, arguments: serde_json::Value) -> Vec<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }));
        let resp = self.recv();
        assert_eq!(resp["id"], id, "response id mismatch: {resp}");
        resp["result"]["content"]
            .as_array()
            .unwrap_or_else(|| panic!("tool call {name} did not succeed: {resp}"))
            .clone()
    }

    /// Same as `call`, but for a call expected to fail — returns the
    /// JSON-RPC error object.
    fn call_expecting_error(&mut self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }));
        let resp = self.recv();
        assert_eq!(resp["id"], id, "response id mismatch: {resp}");
        resp.get("error").cloned().unwrap_or_else(|| panic!("expected an error, got: {resp}"))
    }

    fn text_block(blocks: &[serde_json::Value]) -> &str {
        blocks
            .iter()
            .find(|b| b["type"] == "text")
            .and_then(|b| b["text"].as_str())
            .expect("no text content block")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn tools_list_advertises_all_five_tools_with_schemas() {
    let mut s = Server::start();
    s.send(serde_json::json!({ "jsonrpc": "2.0", "id": 99, "method": "tools/list", "params": {} }));
    let resp = s.recv();
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    let names: std::collections::BTreeSet<&str> =
        tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let expected: std::collections::BTreeSet<&str> =
        ["new_film", "check_film", "film_info", "render_still", "render_film"].into_iter().collect();
    assert_eq!(names, expected, "{tools:#?}");
    for t in tools {
        assert!(t["inputSchema"]["type"] == "object", "{t:#?}");
    }
}

#[test]
fn the_whole_loop_builds_checks_inspects_and_stills_a_film() {
    let dir = std::env::temp_dir()
        .join(format!("showreel-mcp-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let film_path = dir.join("f.film.json");

    let mut s = Server::start();

    let blocks = s.call(
        "new_film",
        serde_json::json!({ "out": film_path.to_str().unwrap(), "title": "MCP Test", "width": 320, "height": 180, "fps": 24.0 }),
    );
    assert!(Server::text_block(&blocks).contains("wrote"), "{blocks:?}");
    assert!(film_path.exists());

    let blocks = s.call("check_film", serde_json::json!({ "film": film_path.to_str().unwrap() }));
    let check: serde_json::Value = serde_json::from_str(Server::text_block(&blocks)).unwrap();
    assert_eq!(check["ok"], true, "{check}");

    let blocks = s.call("film_info", serde_json::json!({ "film": film_path.to_str().unwrap() }));
    let info: serde_json::Value = serde_json::from_str(Server::text_block(&blocks)).unwrap();
    assert_eq!(info["title"], "MCP Test");
    assert_eq!(info["width"], 320);
    assert_eq!(info["scenes"][0]["name"], "title");

    let blocks = s.call("render_still", serde_json::json!({ "film": film_path.to_str().unwrap(), "at": 1.0 }));
    let image = blocks.iter().find(|b| b["type"] == "image").expect("an image content block");
    assert_eq!(image["mimeType"], "image/png");
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD.decode(image["data"].as_str().unwrap()).unwrap();
    assert_eq!(&png[1..4], b"PNG", "not a real PNG: {} bytes", png.len());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_film_produces_a_real_video_and_the_optional_mobile_cut() {
    if !showreel::encode::ffmpeg_available() {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "showreel-mcp-render-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let film_path = dir.join("f.film.json");
    let out_path = dir.join("out.mp4");

    let mut s = Server::start();
    s.call(
        "new_film",
        serde_json::json!({ "out": film_path.to_str().unwrap(), "width": 320, "height": 180, "fps": 24.0 }),
    );
    let blocks = s.call(
        "render_film",
        serde_json::json!({ "film": film_path.to_str().unwrap(), "out": out_path.to_str().unwrap(), "mobile": true }),
    );
    let text = Server::text_block(&blocks);
    assert!(text.contains("wrote"), "{text}");
    assert!(out_path.exists(), "master mp4 was not written");
    let mobile = out_path.with_file_name("out.mobile.mp4");
    assert!(mobile.exists(), "mobile cut was not written");
    assert!(std::fs::metadata(&out_path).unwrap().len() > 500, "suspiciously small mp4");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_film_is_a_clean_jsonrpc_error_not_a_crash() {
    let mut s = Server::start();
    let err = s.call_expecting_error("film_info", serde_json::json!({ "film": "/definitely/not/a/real/path.json" }));
    assert!(err["message"].as_str().unwrap().contains("path.json"), "{err}");
    // The server must still be alive and answer a normal call afterward.
    s.send(serde_json::json!({ "jsonrpc": "2.0", "id": 500, "method": "tools/list", "params": {} }));
    let resp = s.recv();
    assert!(resp["result"]["tools"].is_array(), "{resp}");
}

/// The bug the manual smoke test during development actually caught: the
/// server's async runtime needs `enable_time`, or the process panics on
/// shutdown draining the moment the *second* server instance in one test
/// binary exercises that path. A wait-then-drop after real tool calls is
/// what triggers it, so this is deliberately not merged into the tests
/// above — it wants a clean process by itself.
#[test]
fn the_server_shuts_down_cleanly_after_real_work() {
    let dir = std::env::temp_dir().join(format!(
        "showreel-mcp-shutdown-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let film_path = dir.join("f.film.json");

    let mut s = Server::start();
    s.call(
        "new_film",
        serde_json::json!({ "out": film_path.to_str().unwrap(), "width": 64, "height": 36, "fps": 10.0 }),
    );
    s.close_stdin();
    let status = s
        .child
        .wait_timeout_checked()
        .expect("the server did not exit after stdin closed");
    assert!(status.success(), "server exited with {status:?}, expected a clean shutdown");

    let _ = std::fs::remove_dir_all(&dir);
}

trait WaitTimeout {
    fn wait_timeout_checked(&mut self) -> Option<std::process::ExitStatus>;
}

impl WaitTimeout for Child {
    fn wait_timeout_checked(&mut self) -> Option<std::process::ExitStatus> {
        let start = std::time::Instant::now();
        loop {
            if let Ok(Some(status)) = self.try_wait() {
                return Some(status);
            }
            if start.elapsed() > Duration::from_secs(5) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
