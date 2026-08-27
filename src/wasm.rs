//! The renderer compiled to run in a browser tab, with no server.
//!
//! `showreel studio` (`src/studio.rs`) proved the interaction — scrub, see the
//! frame — but every one of its pixels is drawn by the native binary and
//! shipped over localhost. This module draws them in the browser instead: a
//! film opens in someone else's tab, over a plain static file host, and they
//! scrub it themselves.
//!
//! No wasm-bindgen: plain `extern "C"` exports and a view over linear memory,
//! the same shape `projects/asciicity`'s `src/wasm.rs` uses for its browser
//! build. The whole boundary is four kinds of call — load the film's JSON,
//! feed in the bytes of the fonts and assets it names, ask for the frame at a
//! time, and read the RGBA buffer that comes back — which is exactly
//! [`crate::render::Renderer::render_at`] with its inputs pushed across the
//! wasm/JS line instead of read off disk.
//!
//! # What does not survive the trip
//!
//! [`crate::assets::clip::Clip::load`] shells out to `ffmpeg`, and there is no
//! `ffmpeg` in a browser sandbox — nor a filesystem to find one on. So a clip
//! layer's video is never decoded here; it has to already be decoded before
//! this module sees it. `showreel web-pack` (native, `src/bin/showreel.rs`,
//! behind this same `wasm` feature) is that step: it runs the *existing*
//! ffmpeg-backed decode once, ahead of time, and writes each clip's frames out
//! as a [`crate::webclip`] container, which [`sr_add_clip`] reads back. The
//! browser never runs ffmpeg; it only ever unpacks what already ran.
//! ShowReel's `encode` module (writing an mp4 back out) has no wasm story
//! either, for the same reason, and does not try to have one.
//!
//! # State
//!
//! One film at a time, in a single global, because wasm here is
//! single-threaded and the page holds exactly one player. Mirrors
//! `asciicity`'s `static mut ENGINE` for the same reason: nothing else in this
//! module runs concurrently with it.

use crate::assets::clip::Clip;
use crate::assets::still::Still;
use crate::assets::AssetStore;
use crate::canvas::Canvas;
use crate::render::Renderer;
use crate::scale::scale_film;
use crate::text::FontDb;
use crate::time::Time;
use crate::timeline::Film;
use serde::Serialize;
use std::path::PathBuf;

struct State {
    /// The film exactly as written — durations, titles, scene names.
    film: Option<Film>,
    /// The same film through [`scale_film`] — what is actually rendered.
    preview: Option<Film>,
    assets: AssetStore,
    fonts: FontDb,
    last_frame: Option<Canvas>,
    error: String,
    /// Scratch buffers returned to JS by a `_ptr`/`_len` pair; kept alive
    /// here so the pointer stays valid until the next call replaces it.
    assets_needed_json: Vec<u8>,
    structure_json: Vec<u8>,
}

impl State {
    fn new() -> Self {
        State {
            film: None,
            preview: None,
            assets: AssetStore::new(),
            fonts: FontDb::new(),
            last_frame: None,
            error: String::new(),
            assets_needed_json: Vec::new(),
            structure_json: Vec::new(),
        }
    }
}

static mut STATE: Option<State> = None;

#[inline]
fn state() -> &'static mut State {
    // Single-threaded wasm; the state lives for the page's lifetime, exactly
    // as `asciicity::wasm::eng()` treats its engine.
    unsafe { (*core::ptr::addr_of_mut!(STATE)).get_or_insert_with(State::new) }
}

fn set_error(msg: impl Into<String>) {
    state().error = msg.into();
}

/// Reclaim a buffer JS wrote into via [`sr_alloc`]. Takes ownership, so it is
/// freed when this returns — every alloc is consumed by exactly one call.
///
/// # Safety
/// `ptr`/`len` must be exactly what [`sr_alloc`] returned and nothing else
/// must have written past `len` bytes from `ptr`.
unsafe fn take(ptr: *mut u8, len: u32) -> Vec<u8> {
    unsafe { Vec::from_raw_parts(ptr, len as usize, len as usize) }
}

// ---- memory ----------------------------------------------------------

/// Allocate `len` bytes in wasm memory for JS to write into before a call
/// that takes a `(ptr, len)` pair. Every such call consumes (frees) the
/// buffer it is given, so there is no matching `sr_dealloc`.
#[unsafe(no_mangle)]
pub extern "C" fn sr_alloc(len: u32) -> *mut u8 {
    let mut buf = vec![0u8; len as usize];
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

// ---- film -------------------------------------------------------------

/// Parse and validate a film's JSON (or JSONC — see [`Film::from_json`]),
/// scaled by `scale` for rendering. Returns 1 on success, 0 on failure (see
/// [`sr_error_ptr`]/[`sr_error_len`]). A prior film's registered fonts are
/// kept; its assets are dropped, since a new film names its own.
///
/// # Safety
/// `(ptr, len)` must be exactly a pair [`sr_alloc`] handed back with the
/// film's JSON written into it, and nothing else may touch that memory
/// before this call takes ownership of it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sr_load_film(ptr: *mut u8, len: u32, scale: f64) -> i32 {
    let bytes = unsafe { take(ptr, len) };
    let src = String::from_utf8_lossy(&bytes).into_owned();
    match Film::from_json(&src) {
        Ok(film) => {
            let errs = film.validate();
            if !errs.is_empty() {
                set_error(format!("{} problem(s): {}", errs.len(), errs.join("; ")));
                return 0;
            }
            let preview = scale_film(&film, scale.clamp(0.05, 4.0));
            let st = state();
            st.assets_needed_json = serde_json::to_vec(&preview.assets_used()).unwrap_or_default();
            st.structure_json = structure(&film).into_bytes();
            st.assets = AssetStore::new();
            st.last_frame = None;
            st.preview = Some(preview);
            st.film = Some(film);
            1
        }
        Err(e) => {
            set_error(format!("{e:#}"));
            0
        }
    }
}

#[derive(Serialize)]
struct PlacementInfo {
    index: usize,
    start: f64,
    duration: f64,
    end: f64,
    name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Structure {
    title: Option<String>,
    width: u32,
    height: u32,
    fps: f64,
    duration: f64,
    frame_count: u32,
    placements: Vec<PlacementInfo>,
}

fn structure(film: &Film) -> String {
    let placements = film
        .timeline
        .placements()
        .iter()
        .map(|p| PlacementInfo {
            index: p.index,
            start: p.start.as_secs(),
            duration: p.duration.as_secs(),
            end: p.end().as_secs(),
            name: film.timeline.scene(p.index).name.clone(),
        })
        .collect();
    let s = Structure {
        title: film.title.clone(),
        width: film.width,
        height: film.height,
        fps: film.fps,
        duration: film.duration().as_secs(),
        frame_count: film.frame_count(),
        placements,
    };
    serde_json::to_string(&s).unwrap_or_else(|_| "{}".to_string())
}

/// JSON array of the assets the loaded film names — what [`sr_add_still`] and
/// [`sr_add_clip`] are waiting for. See [`crate::timeline::AssetUse`]: a
/// `Still` variant carries its bare name (fetch it at `assets/<name>` and
/// pass the bytes straight to [`sr_add_still`]); a `Clip` variant carries the
/// parameters `showreel web-pack` decoded it with (fetch its sidecar at
/// `assets/<name>.srclip` and pass those, unchanged, to [`sr_add_clip`]).
#[unsafe(no_mangle)]
pub extern "C" fn sr_assets_needed_ptr() -> *const u8 {
    state().assets_needed_json.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn sr_assets_needed_len() -> u32 {
    state().assets_needed_json.len() as u32
}

/// JSON describing the film's scenes and timings, for a timeline/structure
/// view — the same information `showreel studio`'s `/api/state` returns.
#[unsafe(no_mangle)]
pub extern "C" fn sr_structure_ptr() -> *const u8 {
    state().structure_json.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn sr_structure_len() -> u32 {
    state().structure_json.len() as u32
}

// ---- assets -------------------------------------------------------------

fn take_name(ptr: *mut u8, len: u32) -> String {
    String::from_utf8_lossy(&unsafe { take(ptr, len) }).into_owned()
}

/// Register a still image's bytes (whatever `image::open` would decode — PNG,
/// JPEG) under `name`, the same string a `still` layer names. 1 on success.
///
/// # Safety
/// Both `(ptr, len)` pairs must be buffers [`sr_alloc`] returned, written
/// into and not otherwise touched before this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sr_add_still(
    name_ptr: *mut u8,
    name_len: u32,
    bytes_ptr: *mut u8,
    bytes_len: u32,
) -> i32 {
    let name = take_name(name_ptr, name_len);
    let bytes = unsafe { take(bytes_ptr, bytes_len) };
    match Still::from_bytes(&bytes) {
        Ok(still) => {
            state().assets.insert_still(&name, still);
            1
        }
        Err(e) => {
            set_error(format!("still {name:?}: {e:#}"));
            0
        }
    }
}

/// Register a clip already decoded to frames by `showreel web-pack` — a
/// `.srclip` container (see [`crate::webclip`]). `fps`/`max_width`/`trim`
/// must be exactly the values [`sr_assets_needed_ptr`] reported for this
/// clip — they form the cache key [`crate::assets::AssetStore::clip`] looks
/// the decode up by, so a mismatch here means the renderer asks for a clip
/// that was never registered under the key it is looking for.
///
/// # Safety
/// Both `(ptr, len)` pairs must be buffers [`sr_alloc`] returned, written
/// into and not otherwise touched before this call.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sr_add_clip(
    name_ptr: *mut u8,
    name_len: u32,
    fps: f64,
    max_width: u32,
    has_trim: i32,
    trim_start: f64,
    trim_dur: f64,
    container_ptr: *mut u8,
    container_len: u32,
) -> i32 {
    let name = take_name(name_ptr, name_len);
    let container = unsafe { take(container_ptr, container_len) };
    let trim = (has_trim != 0).then_some((trim_start, trim_dur));
    match crate::webclip::decode(&container) {
        Ok((frames, clip_fps)) => {
            state().assets.insert_clip(&name, fps, max_width, trim, Clip::from_frames(frames, clip_fps));
            1
        }
        Err(e) => {
            set_error(format!("clip {name:?}: {e:#}"));
            0
        }
    }
}

/// Register a font face's bytes (TTF/OTF/TTC), the browser-side equivalent of
/// [`FontDb::add_file`] for a caller with no filesystem to scan.
///
/// # Safety
/// `(ptr, len)` must be a buffer [`sr_alloc`] returned, written into and not
/// otherwise touched before this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sr_add_font(ptr: *mut u8, len: u32) -> i32 {
    let bytes = unsafe { take(ptr, len) };
    match state().fonts.add_bytes(bytes, PathBuf::from("<web>")) {
        Ok(ids) => ids.len() as i32,
        Err(e) => {
            set_error(format!("font: {e:#}"));
            0
        }
    }
}

// ---- rendering ----------------------------------------------------------

/// Render the frame at `t` seconds. 1 on success, 0 on failure (a missing
/// asset the film needs, most likely — see [`sr_error_ptr`]).
#[unsafe(no_mangle)]
pub extern "C" fn sr_render_at(t: f64) -> i32 {
    let st = state();
    let Some(preview) = &st.preview else {
        set_error("no film loaded");
        return 0;
    };
    let t = t.clamp(0.0, preview.duration().as_secs().max(0.0));
    match Renderer::new(preview, &st.assets, &st.fonts).render_at(Time(t)) {
        Ok(canvas) => {
            st.last_frame = Some(canvas);
            1
        }
        Err(e) => {
            set_error(format!("{e:#}"));
            0
        }
    }
}

/// Premultiplied RGBA bytes of the last frame [`sr_render_at`] drew — a
/// frame's background is always cleared to an opaque colour first (see
/// `Renderer::draw_scene`), so alpha is 255 throughout and premultiplied is
/// the same as straight; `putImageData` can use this pointer directly.
#[unsafe(no_mangle)]
pub extern "C" fn sr_frame_ptr() -> *const u8 {
    state().last_frame.as_ref().map(|c| c.data().as_ptr()).unwrap_or(std::ptr::null())
}

#[unsafe(no_mangle)]
pub extern "C" fn sr_frame_len() -> u32 {
    state().last_frame.as_ref().map(|c| c.data().len() as u32).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn sr_frame_width() -> u32 {
    state().last_frame.as_ref().map(|c| c.width()).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn sr_frame_height() -> u32 {
    state().last_frame.as_ref().map(|c| c.height()).unwrap_or(0)
}

// ---- errors ---------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn sr_error_ptr() -> *const u8 {
    state().error.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn sr_error_len() -> u32 {
    state().error.len() as u32
}

