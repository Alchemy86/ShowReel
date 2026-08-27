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
use crate::layer::Content;
use crate::render::Renderer;
use crate::scale::scale_film;
use crate::text::FontDb;
use crate::time::Time;
use crate::timeline::{Film, Scene};
use serde::Serialize;
use std::path::PathBuf;

struct State {
    /// The film exactly as written — durations, titles, scene names.
    film: Option<Film>,
    /// The same film through [`scale_film`] — the full-quality frame a scrub
    /// or a paused moment renders from. Also what every registered clip's
    /// `max_width` was decoded at, so this is the shape any further-scaled
    /// [`draft`](Self::draft) must not disturb.
    preview: Option<Film>,
    /// [`preview`](Self::preview), scaled further down for continuous
    /// playback — see [`sr_set_draft_scale`]. `None` means "render `preview`
    /// directly," which is also what a paused frame always does.
    draft: Option<Film>,
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
            draft: None,
            assets: AssetStore::new(),
            fonts: FontDb::new(),
            last_frame: None,
            error: String::new(),
            assets_needed_json: Vec::new(),
            structure_json: Vec::new(),
        }
    }
}

/// `scale_film(base, k)`, with every clip layer's `max_width` put back to
/// `base`'s own value.
///
/// `scale_film` shrinks a clip's `max_width` right along with everything
/// else — the right call for [`crate::preview::contact_sheet`], where a
/// fresh, smaller decode is cheaper than a big one. But in the browser there
/// is no ffmpeg to do that fresh decode: [`AssetStore::clip`] was populated
/// by [`sr_add_clip`] under whatever `max_width` [`sr_assets_needed_ptr`]
/// reported at load time, and asking for any other `max_width` afterward is a
/// guaranteed cache miss — which native code answers by shelling out to
/// `Clip::load`, which wasm cannot do at all. Every other field (frame size,
/// placement, type) scales normally; only a clip's decode width stays pinned
/// to whatever `base` already declared it as.
///
/// [`sr_load_film`] uses this (with `base` the film exactly as given) so a
/// browser-chosen preview size never touches clip decode width at all — the
/// requested `max_width` is always whatever the page (or `showreel web-pack`)
/// already packed. [`sr_set_draft_scale`] then reuses it a second time (with
/// `base` the registered preview) to narrow further for continuous playback
/// without disturbing that same pinned width again.
fn scale_keep_clip_decode(base: &Film, k: f64) -> Film {
    fn restore(scene: &mut Scene, original: &Scene) {
        for (l, ol) in scene.layers.iter_mut().zip(original.layers.iter()) {
            if let (Content::Clip { max_width, .. }, Content::Clip { max_width: orig, .. }) =
                (&mut l.content, &ol.content)
            {
                *max_width = *orig;
            }
        }
    }
    let mut scaled = scale_film(base, k);
    restore(&mut scaled.timeline.opening, &base.timeline.opening);
    for (link, orig_link) in scaled.timeline.then.iter_mut().zip(base.timeline.then.iter()) {
        restore(&mut link.scene, &orig_link.scene);
    }
    scaled
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
/// scaled by `scale` for rendering. `scale` shrinks the frame, type and every
/// other visible quantity but never a clip's decode `max_width` (see
/// [`scale_keep_clip_decode`]) — so the caller is free to pick whatever
/// `scale` fits the on-screen preview box without needing differently-packed
/// clip assets to match. Returns 1 on success, 0 on failure (see
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
            let preview = scale_keep_clip_decode(&film, scale.clamp(0.05, 4.0));
            let st = state();
            st.assets_needed_json = serde_json::to_vec(&preview.assets_used()).unwrap_or_default();
            st.structure_json = structure(&film).into_bytes();
            st.assets = AssetStore::new();
            st.last_frame = None;
            st.draft = None;
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

/// Set (or clear) the extra scale-down applied on top of the loaded preview
/// for continuous playback. `k >= 1.0` clears it — the next [`sr_render_at`]
/// renders `preview` directly, full quality. `0.05..1.0` rebuilds a smaller
/// [`draft_film`] once here, so playback's per-frame cost is exactly a
/// smaller [`Renderer::render_at`] and not a `scale_film` call every tick.
/// 1 on success, 0 if no film is loaded yet.
#[unsafe(no_mangle)]
pub extern "C" fn sr_set_draft_scale(k: f64) -> i32 {
    let st = state();
    let Some(preview) = &st.preview else {
        set_error("no film loaded");
        return 0;
    };
    st.draft = (k < 1.0 - 1e-9).then(|| scale_keep_clip_decode(preview, k.clamp(0.05, 1.0)));
    1
}

/// Render the frame at `t` seconds. 1 on success, 0 on failure (a missing
/// asset the film needs, most likely — see [`sr_error_ptr`]).
#[unsafe(no_mangle)]
pub extern "C" fn sr_render_at(t: f64) -> i32 {
    let st = state();
    let Some(preview) = st.draft.as_ref().or(st.preview.as_ref()) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Layer;
    use crate::text::TextStyle;
    use crate::timeline::Scene;

    fn film_with_clip_and_text() -> Film {
        Film::new(1920, 1080, 30.0).open(
            Scene::new(2.0)
                .layer(Layer::clip("c.mp4"))
                .layer(Layer::text("body").styled(TextStyle::default().size(40.0))),
        )
    }

    // The bug a naive `scale_film(base, k)` would reintroduce: a clip
    // layer's `max_width` would shrink along with everything else, so
    // `sr_render_at` would ask `AssetStore` for a clip at a `max_width` that
    // was never registered by `sr_add_clip` — a guaranteed miss that native
    // code answers by calling `Clip::load` (ffmpeg), which does not exist in
    // wasm. `scale_keep_clip_decode` must leave clip `max_width` exactly as
    // `base` has it while still shrinking everything else (frame size, type).
    #[test]
    fn scale_keep_clip_decode_pins_clip_width_but_shrinks_everything_else() {
        let base = film_with_clip_and_text();
        let scaled = scale_keep_clip_decode(&base, 0.25);

        assert_eq!((scaled.width, scaled.height), (480, 270));

        let Content::Clip { max_width, .. } = &scaled.timeline.opening.layers[0].content else {
            panic!("expected a clip")
        };
        let Content::Clip { max_width: registered, .. } = &base.timeline.opening.layers[0].content else {
            panic!("expected a clip")
        };
        assert_eq!(max_width, registered, "clip decode width must match what sr_add_clip registered");

        let Content::Text { style, .. } = &scaled.timeline.opening.layers[1].content else {
            panic!("expected text")
        };
        assert!((style.size - 10.0).abs() < 1e-9, "text should still scale down: got {}", style.size);
    }

    // sr_load_film's own scale must get the same treatment as
    // sr_set_draft_scale's — a browser-chosen preview size must never move a
    // clip's decode width away from what the page actually registered.
    #[test]
    fn sr_load_film_scale_also_pins_clip_decode_width() {
        let base = film_with_clip_and_text();
        let bytes = serde_json::to_vec(&base).unwrap();
        let ptr = sr_alloc(bytes.len() as u32);
        unsafe { std::slice::from_raw_parts_mut(ptr, bytes.len()) }.copy_from_slice(&bytes);

        assert_eq!(unsafe { sr_load_film(ptr, bytes.len() as u32, 0.5) }, 1, "{}", state().error);

        let preview = state().preview.as_ref().unwrap();
        assert_eq!(preview.width, 960);
        let Content::Clip { max_width, .. } = &preview.timeline.opening.layers[0].content else {
            panic!("expected a clip")
        };
        let Content::Clip { max_width: registered, .. } = &base.timeline.opening.layers[0].content else {
            panic!("expected a clip")
        };
        assert_eq!(max_width, registered, "sr_load_film's scale must not move clip decode width");
    }
}

