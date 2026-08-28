//! An MCP server: build, render, still and inspect a film through typed tool
//! calls, so an agent drives ShowReel directly instead of shelling out to the
//! CLI and parsing its stdout.
//!
//! # Why five tools, and why not more
//!
//! They mirror the CLI's own vocabulary rather than inventing a bespoke set:
//! `new_film` (`showreel new`), `check_film` (`showreel check`), `film_info`
//! (`showreel info`), `render_still` (`showreel still`) and `render_film`
//! (`showreel render`) — the same four+one the local HTTP API exposes (see
//! `src/studio.rs`'s "API access" section), over stdio instead of a socket.
//!
//! **"Build a film" beyond scaffolding a starter is deliberately not a tool
//! per `Content` variant.** A film is JSON — the browser editor's own film
//! object *is* the wire JSON (see `AGENTS.md`) — and an agent with ordinary
//! file tools already reads and writes that JSON directly, faster than a
//! round trip through a tool call for every layer. What these five close is
//! the loop that *does* need ShowReel itself: validating the JSON an agent
//! wrote, seeing what a moment of it actually looks like, and turning it
//! into real video — the same reason `showreel still`/`check` exist for a
//! person instead of asking them to read `render.rs`.
//!
//! # Why the official SDK, not a hand-rolled JSON-RPC framing
//!
//! Every other protocol this crate speaks natively is either its own file
//! format (the film JSON) or something it has to shell out to anyway
//! (ffmpeg). MCP's initialize handshake, tool schema shape and message
//! framing are a moving target maintained upstream; `rmcp` (the official
//! Rust SDK) tracks that so this module doesn't have to. It is optional
//! (`--features mcp`) for the same reason `tiny_http` is behind `studio`:
//! it pulls in an async runtime (`tokio`) the plain render path has never
//! needed, so the default build stays exactly as dependency-light as before.
//!
//! # Matching the fleet's own conventions
//!
//! The `axi` skill owns the standard for agent-facing tool ergonomics in
//! this fleet — minimal default schemas, structured errors over raw
//! dependency output, contextual "what next" hints — and its principles are
//! applied here even though the transport is MCP, not a CLI: `film_info`'s
//! scene list stays terse, every error is a short actionable sentence
//! rather than an `anyhow` chain, and `new_film`'s response says what to run
//! next. `openhuman-core` is already wired in as an MCP server in this
//! fleet; this is a second one, not a fourth convention invented alongside
//! the `-axi` CLI house style.

use crate::assets::AssetStore;
use crate::encode::{
    EncodeOptions, FfmpegSink, MobileOptions, ffmpeg_available, mobile_cut, mobile_path,
};
use crate::layer::Layer;
use crate::preview;
use crate::render::Renderer;
use crate::scale::scale_film;
use crate::text::FontDb;
use crate::time::Time;
use crate::timeline::{Film, Scene};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::Deserialize;

/// A store rooted at the film's own directory, plus whatever else was named —
/// the same rule the CLI and the studio use, so a film that renders from the
/// terminal renders the same way from an agent.
fn store(film_path: &std::path::Path, roots: &[String]) -> AssetStore {
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

/// Translate an internal failure into a short, actionable MCP error — never
/// an `anyhow` chain leaking file offsets or a dependency's own error type,
/// per the `axi` skill's "structured errors" principle.
fn mcp_err(e: anyhow::Error) -> McpError {
    McpError::internal_error(format!("{e:#}"), None)
}

/// Load and validate — the same two-step `load()` the CLI binary uses before
/// `render`/`still`, so a film an agent couldn't render is refused with the
/// same errors a person would see, not a confusing downstream failure.
fn load_valid(film_path: &std::path::Path) -> Result<Film, McpError> {
    let film = Film::load(film_path).map_err(mcp_err)?;
    let errs = film.validate();
    if !errs.is_empty() {
        return Err(McpError::invalid_params(
            format!("{} does not validate:\n{}", film_path.display(), errs.join("\n")),
            None,
        ));
    }
    Ok(film)
}

fn default_width() -> u32 {
    1920
}
fn default_height() -> u32 {
    1080
}
fn default_fps() -> f64 {
    30.0
}
fn default_scale() -> f64 {
    1.0
}

#[derive(Deserialize, schemars::JsonSchema)]
struct NewFilmParams {
    /// Where to write the new film's JSON.
    out: String,
    /// Shown on the title card. Defaults to "Untitled".
    #[serde(default)]
    title: Option<String>,
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default = "default_fps")]
    fps: f64,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct FilmPathParams {
    /// Path to a film's JSON (or JSONC) file. Neither `check_film` nor
    /// `film_info` touches its assets — both read only the film's own JSON —
    /// so, unlike `render_still`/`render_film`, there is no `assets` field.
    film: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct StillParams {
    film: String,
    #[serde(default)]
    assets: Vec<String>,
    /// Seconds into the film.
    #[serde(default)]
    at: f64,
    /// Render at this fraction of the film's declared size.
    #[serde(default = "default_scale")]
    scale: f64,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct RenderParams {
    film: String,
    #[serde(default)]
    assets: Vec<String>,
    /// Where to write the rendered video.
    out: String,
    #[serde(default = "default_scale")]
    scale: f64,
    /// Also produce the 720p mobile delivery cut alongside the master.
    #[serde(default)]
    mobile: bool,
}

/// No fields: `#[tool_router]` generates `Self::tool_router()` as an
/// associated function, not something this needs to store per instance —
/// there is no other server state either, since every tool is a pure
/// function of the film path it's given.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShowReelMcp;

#[tool_router]
impl ShowReelMcp {
    pub fn new() -> Self {
        ShowReelMcp
    }

    #[tool(
        description = "Scaffold a new film: writes a starter JSON film — one title-card scene — to `out`, ready to open and edit further with ordinary file tools. Mirrors `showreel new`."
    )]
    async fn new_film(
        &self,
        Parameters(p): Parameters<NewFilmParams>,
    ) -> Result<CallToolResult, McpError> {
        let title = p.title.unwrap_or_else(|| "Untitled".to_string());
        let film = Film::new(p.width, p.height, p.fps).title(title.clone()).open(
            Scene::new(4.0).named("title").layer(
                Layer::title(&title).entering(crate::motion::Motion::chars(0.5, 0.02)),
            ),
        );
        let json = film.to_json().map_err(mcp_err)?;
        std::fs::write(&p.out, &json)
            .map_err(|e| mcp_err(anyhow::anyhow!("writing {}: {e}", p.out)))?;
        let text = format!(
            "wrote {} — {}x{} at {}fps, one scene (\"title\", 4s)\nnext: edit the JSON directly (see README.md's \"Writing a film\"), then check_film or render_still to see it",
            p.out, p.width, p.height, p.fps
        );
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    #[tool(
        description = "Validate a film file: the mistakes a JSON schema alone cannot catch (an odd frame dimension, a scene shorter than its own transition, a track past the end of the film, ...). Mirrors `showreel check`."
    )]
    async fn check_film(
        &self,
        Parameters(p): Parameters<FilmPathParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = std::path::PathBuf::from(&p.film);
        let film = Film::load(&path).map_err(mcp_err)?;
        let errors = film.validate();
        let value = serde_json::json!({ "ok": errors.is_empty(), "errors": errors });
        Ok(CallToolResult::success(vec![ContentBlock::json(value)?]))
    }

    #[tool(
        description = "Describe a film's structure: title, size/fps, duration, every scene (name, start, duration, transition-in, layer count) and every audio track. Mirrors `showreel info`."
    )]
    async fn film_info(
        &self,
        Parameters(p): Parameters<FilmPathParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = std::path::PathBuf::from(&p.film);
        let film = Film::load(&path).map_err(mcp_err)?;
        let value = serde_json::to_value(film.summary())
            .map_err(|e| mcp_err(anyhow::anyhow!(e)))?;
        Ok(CallToolResult::success(vec![ContentBlock::json(value)?]))
    }

    #[tool(
        description = "Render one frame of a film to a PNG image, returned inline. Mirrors `showreel still`."
    )]
    async fn render_still(
        &self,
        Parameters(p): Parameters<StillParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = std::path::PathBuf::from(&p.film);
        let film = load_valid(&path)?;
        let film = scale_film(&film, p.scale);
        let assets = store(&path, &p.assets);
        let canvas = preview::still_at(&film, &assets, FontDb::shared(), Time(p.at))
            .map_err(mcp_err)?;
        let (w, h) = (canvas.width(), canvas.height());
        let png = canvas.encode_png().map_err(mcp_err)?;
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png);
        Ok(CallToolResult::success(vec![
            ContentBlock::text(format!("{w}x{h} at {:.2}s", p.at)),
            ContentBlock::image(b64, "image/png"),
        ]))
    }

    #[tool(
        description = "Render a film to a real video file via ffmpeg — the master, and optionally the 720p mobile delivery cut. Mirrors `showreel render`."
    )]
    async fn render_film(
        &self,
        Parameters(p): Parameters<RenderParams>,
    ) -> Result<CallToolResult, McpError> {
        if !ffmpeg_available() {
            return Err(McpError::internal_error(
                "ffmpeg is not on PATH; ShowReel needs it to encode",
                None,
            ));
        }
        let path = std::path::PathBuf::from(&p.film);
        let film = load_valid(&path)?;
        let assets = store(&path, &p.assets);
        let render_film = scale_film(&film, p.scale);

        let mut tracks = render_film.resolve_audio_tracks(&assets).map_err(mcp_err)?;
        tracks.extend(render_film.clip_audio(&assets).map_err(mcp_err)?);
        let opts = EncodeOptions::default().with_audio(tracks);
        let mut encoder = FfmpegSink::new(
            &p.out,
            render_film.width,
            render_film.height,
            render_film.fps,
            &opts,
            render_film.background,
        )
        .map_err(mcp_err)?;
        let stats = Renderer::new(&render_film, &assets, FontDb::shared())
            .render_all(&mut encoder)
            .map_err(mcp_err)?;

        let mut lines = vec![format!("wrote {} — {stats}", p.out)];
        if p.mobile {
            let mob = mobile_path(std::path::Path::new(&p.out));
            mobile_cut(&p.out, &mob, &MobileOptions::default()).map_err(mcp_err)?;
            lines.push(format!("wrote {} (720p mobile cut)", mob.display()));
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(lines.join("\n"))]))
    }
}

#[tool_handler]
impl ServerHandler for ShowReelMcp {
    fn get_info(&self) -> ServerInfo {
        // The default `Implementation::from_build_env()` reports `rmcp`
        // itself (it reads the *defining* crate's build env, not the
        // caller's), so this is set explicitly rather than left implicit.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new("showreel", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "ShowReel builds and renders declarative films. A film is one JSON file — \
                 write and edit it with ordinary file tools (see README.md's \"Writing a \
                 film\" and examples/kanto.film.jsonc for the shape), then use check_film, \
                 film_info and render_still to verify it before render_film. new_film \
                 scaffolds a starting point.",
            )
    }
}

/// Run the server on stdio until the client disconnects.
pub async fn serve() -> anyhow::Result<()> {
    let server = ShowReelMcp::new().serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;
    Ok(())
}
