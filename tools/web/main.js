// Boots the wasm renderer, drives the transport/scrubber, and is the only
// place that owns the wasm/JS asset boundary — `editor.js` mutates the film
// tree and calls back into `scheduleReload()`; it never touches wasm itself.
//
// # The single source of truth
//
// `film` (below) is a plain JS object the editor mutates in place. It is
// serialised to JSON and handed to `sr_load_film` on every reload, but
// wasm never hands anything back except render output and validation
// errors — there is no "the parsed film, canonicalised" call. So this file,
// not wasm, is authoritative for what the film *is*; wasm is authoritative
// only for what it *looks like* at a given time.

import { WasmBridge, fetchBytes, clipAssetPath, clipCacheKey } from './bridge.js';
import { createEditor, newFilm, newLayer } from './editor.js';
import { importClip } from './clipimport.js';
import { exportWebM, webCodecsAvailable } from './export.js';

const WASM = './showreel.wasm';
const FONTS = [
  'fonts/Montserrat-Regular.otf', 'fonts/Montserrat-SemiBold.otf', 'fonts/Montserrat-ExtraBold.otf',
  'fonts/OpenSans-Regular.ttf', 'fonts/OpenSans-Semibold.ttf', 'fonts/OpenSans-Bold.ttf',
];

const $ = (id) => document.getElementById(id);
const canvas = $('view'), ctx = canvas.getContext('2d', { alpha: false });
const splash = $('splash'), errBanner = $('error-banner');
const seek = $('seek'), playBtn = $('btn-play'), timecode = $('timecode'), fpsReadout = $('fps-readout');

// `src/preview.rs`'s own answer to "does the motion work?" is a quarter-size
// pass over the real timeline. The browser build had never taken that route
// for playback at all: `sr_load_film` always loaded at scale 1.0, so every
// played frame did the same full-resolution work a paused scrub does — on
// the reference film this task shipped with (1920x1080/60fps,
// `examples/kanto.film.jsonc`), that measured at roughly 0.9-1.1 raw frames
// per second, matching the "YouTube at 3fps" complaint this fix answers.
// `sr_set_draft_scale` (src/wasm.rs) applies a further discount on top of
// the already-loaded preview, only while playing; pausing or scrubbing
// always renders `preview` at full quality again.
//
// Quarter-size (0.25, "roughly 16x less pixel work" per preview.rs) was the
// first thing tried, direct-benchmarked on that same film/machine via
// `wasm.sr_render_at` in a tight loop (bypassing rAF entirely, so this is
// raw achievable throughput, not scheduler-limited): 1.0 -> 0.9fps, 0.5 ->
// 3.7fps, 0.25 -> 12.5fps. Short of the 24-30fps target, so the same
// benchmark was walked further down: 0.125 lands at ~26fps (40-sample
// average) with headroom, and still holds a legible small preview frame
// (240x135 on this film) — soft, but composition and motion read fine,
// which is what playback needs; a paused frame is never affected.
const PLAYBACK_DRAFT_SCALE = 0.125;

let wasm, bridge;
let film = null;
let duration = 0, fps = 30, currentT = 0, playing = false, playAnchorWall = 0, playAnchorT = 0;
let reloadTimer = null;
// Rolling window of recent per-frame render+paint times, the same shape
// `src/studio/page.js`'s `recordFrameTime` uses, so the browser build is
// honest about achieved playback rate the same way the native studio is —
// never a claimed real-time clock, always the frame rate actually measured.
let frameTimes = [];

// name -> Uint8Array (stills) or File (clip sources) added from the browser,
// not fetched from the server — see editor.js's module docs on why a clip
// keeps its *source* here rather than one fixed decode.
const providedStills = new Map();
const providedClips = new Map();
// `${kind}:${name}` -> 'ok' | 'missing', for the Assets panel's status dots.
const assetStatus = new Map();
// clipCacheKey(...) -> {bytes} — avoids re-decoding a browser clip on every
// keystroke; only a changed (asset, fps, max_width, trim) tuple re-extracts.
const clipExtractCache = new Map();

function fail(msg) {
  errBanner.hidden = false;
  errBanner.textContent = msg;
  console.error(msg);
}
function clearFail() {
  errBanner.hidden = true;
}

// ---- comment-tolerant JSON, for opening a hand-written .jsonc film -------
//
// This is a best-effort stripper, not `src/timeline.rs`'s real JSONC reader
// (`jsonc-parser`) — that parser lives in wasm and only ever returns a
// *rendered frame*, never the parsed tree, so there is no way to ask wasm
// for "the film, canonicalised" to edit. Opening a `.jsonc` film this way
// works for ordinary comments and trailing commas; anything stranger should
// be cleaned up by hand first. Saving back out always emits plain JSON (see
// `btn-save`), same as `Film::to_json` does natively.
// `Film::to_json()` (src/timeline.rs) omits several fields entirely rather
// than writing them as empty/null — `#[serde(skip_serializing_if = ...)]`
// on `audio` (empty vec), `title`/`theme` (None) — since a hand-written film
// file usually leaves them out too. The editor always expects the keys it
// reads to exist, so any freshly-loaded film (server-provided or opened from
// disk) is normalised once here rather than every call site re-guessing
// which fields might be missing.
function normalizeFilm(f) {
  if (!Array.isArray(f.audio)) f.audio = [];
  if (!Array.isArray(f.then)) f.then = [];
  return f;
}

function stripJsonc(text) {
  let out = '';
  let inStr = false, strCh = '';
  for (let i = 0; i < text.length; i++) {
    const c = text[i], n = text[i + 1];
    if (inStr) {
      out += c;
      if (c === '\\') { out += n; i++; continue; }
      if (c === strCh) inStr = false;
      continue;
    }
    if (c === '"' || c === "'") { inStr = true; strCh = c; out += c; continue; }
    if (c === '/' && n === '/') { while (i < text.length && text[i] !== '\n') i++; out += '\n'; continue; }
    if (c === '/' && n === '*') { i += 2; while (i < text.length && !(text[i] === '*' && text[i + 1] === '/')) i++; i++; continue; }
    out += c;
  }
  return out.replace(/,(\s*[}\]])/g, '$1');
}

// ---- the wasm/JS boundary ----------------------------------------------

async function loadFonts() {
  for (const url of FONTS) {
    const [ptr, len] = bridge.writeBytes(await fetchBytes(url));
    if (!wasm.sr_add_font(ptr, len)) throw new Error(`font ${url}: ${bridge.lastError()}`);
  }
}

async function probeVideo(file) {
  const video = document.createElement('video');
  video.muted = true;
  video.src = URL.createObjectURL(file);
  try {
    await new Promise((resolve, reject) => {
      video.addEventListener('loadedmetadata', resolve, { once: true });
      video.addEventListener('error', () => reject(new Error('not a playable video')), { once: true });
    });
    return { duration: video.duration, width: video.videoWidth, height: video.videoHeight };
  } finally {
    URL.revokeObjectURL(video.src);
  }
}

// Fetches and registers every asset the loaded film named, consulting
// browser-provided assets first — mirrors the original static page's
// `loadAssets`, but a name may now resolve to something the editor added
// rather than a server file. See `sr_assets_needed_ptr`'s doc comment
// (src/wasm.rs) for the manifest shape.
async function loadAssets(onProgress) {
  const needed = JSON.parse(bridge.readStr(wasm.sr_assets_needed_ptr, wasm.sr_assets_needed_len) || '[]');
  let done = 0;
  for (const u of needed) {
    if ('Still' in u) {
      const name = u.Still;
      let bytes = providedStills.get(name);
      if (!bytes) bytes = await fetchBytes(`assets/${name}`).catch(() => null);
      assetStatus.set(`still:${name}`, bytes ? 'ok' : 'missing');
      if (bytes) {
        const [np, nl] = bridge.writeText(name);
        const [bp, bl] = bridge.writeBytes(bytes);
        if (!wasm.sr_add_still(np, nl, bp, bl)) throw new Error(`still ${name}: ${bridge.lastError()}`);
      }
    } else if ('Clip' in u) {
      const c = u.Clip;
      const fpsUsed = c.decode_fps ?? fps;
      const trim = c.trim ?? null;
      const [trimStart, trimDur] = trim ?? [0, 0];
      const key = clipCacheKey(c.asset, c.max_width, fpsUsed, trim);
      let container = clipExtractCache.get(key);
      if (!container) {
        const sourceFile = providedClips.get(c.asset);
        if (sourceFile) {
          if (!trim) {
            fail(`clip "${c.asset}": browser-added clips need a trim set (no whole-file decode in the browser)`);
          } else {
            const res = await importClip(sourceFile, { trimStart, trimDuration: trimDur, decodeFps: fpsUsed, maxWidth: c.max_width });
            container = res.container;
          }
        } else {
          container = await fetchBytes(clipAssetPath(c, fpsUsed)).catch(() => null);
        }
        if (container) clipExtractCache.set(key, container);
      }
      assetStatus.set(`clip:${c.asset}`, container ? 'ok' : 'missing');
      if (container) {
        const [np, nl] = bridge.writeText(c.asset);
        const [bp, bl] = bridge.writeBytes(container);
        if (!wasm.sr_add_clip(np, nl, fpsUsed, c.max_width, trim ? 1 : 0, trimStart, trimDur, bp, bl)) {
          throw new Error(`clip ${c.asset}: ${bridge.lastError()}`);
        }
      }
    }
    onProgress?.(++done, needed.length);
  }
}

function resolveAssetStatus(kind, name) {
  return assetStatus.get(`${kind}:${name}`) || 'unknown';
}

// ---- painting / structure -------------------------------------------------

function paint() {
  const w = wasm.sr_frame_width(), h = wasm.sr_frame_height();
  if (!w || !h) return;
  if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
  const data = new Uint8ClampedArray(bridge.mem.buffer, wasm.sr_frame_ptr(), wasm.sr_frame_len());
  ctx.putImageData(new ImageData(new Uint8ClampedArray(data), w, h), 0, 0);
}
function fmtT(t) {
  const m = Math.floor(t / 60), s = t - m * 60;
  return `${m}:${s.toFixed(1).padStart(4, '0')}`;
}
function recordFrameTime(ms) {
  frameTimes.push(ms);
  if (frameTimes.length > 20) frameTimes.shift();
  if (playing) {
    const avg = frameTimes.reduce((a, b) => a + b, 0) / frameTimes.length;
    const achieved = Math.min(1000 / avg, fps);
    fpsReadout.textContent = `~${achieved.toFixed(1)} fps (best effort)`;
  } else {
    fpsReadout.textContent = '';
  }
}

function renderAt(t) {
  currentT = Math.max(0, Math.min(duration, t));
  const started = performance.now();
  if (!wasm.sr_render_at(currentT)) { fail(`rendering the frame: ${bridge.lastError()}`); return; }
  paint();
  recordFrameTime(performance.now() - started);
  seek.value = currentT;
  timecode.textContent = `${fmtT(currentT)} / ${fmtT(duration)}`;
}
function buildStructure() {
  const s = JSON.parse(bridge.readStr(wasm.sr_structure_ptr, wasm.sr_structure_len) || '{}');
  duration = s.duration || 0;
  fps = s.fps || film.fps || 30;
  $('film-title').textContent = s.title || '';
  $('film-stats').textContent = `${s.width}x${s.height} · ${s.fps}fps · ${s.duration.toFixed(2)}s`;
  seek.max = duration || 1;
  const scenes = $('scenes');
  scenes.innerHTML = '';
  for (const p of s.placements || []) {
    const frac = duration > 0 ? p.duration / duration : 0;
    const seg = document.createElement('div');
    seg.style.flex = `${Math.max(frac, 0.001)} 0 0`;
    seg.title = `${p.name || `scene ${p.index + 1}`} — ${p.start.toFixed(2)}s..${p.end.toFixed(2)}s`;
    seg.addEventListener('click', () => renderAt(p.start));
    scenes.appendChild(seg);
  }
}

// ---- reload cycle: film object -> wasm -----------------------------------

async function doReload() {
  const [ptr, len] = bridge.writeText(JSON.stringify(film));
  if (!wasm.sr_load_film(ptr, len, 1.0)) {
    fail(`film has a problem: ${bridge.lastError()}`);
    editor.render();
    return;
  }
  clearFail();
  buildStructure();
  try {
    await loadAssets(() => {});
  } catch (e) {
    fail(String(e && e.stack || e));
  }
  renderAt(Math.min(currentT, duration));
  editor.render();
}

function scheduleReload() {
  clearTimeout(reloadTimer);
  reloadTimer = setTimeout(doReload, 250);
}

// ---- editor wiring ----------------------------------------------------

const editor = createEditor({
  timelineEl: $('panel-timeline'),
  inspectorEl: $('panel-inspector'),
  getFilm: () => film,
  onChange: scheduleReload,
  resolveAssetStatus,
});

function uniqueAssetName(map, base) {
  const clean = (base || 'asset').replace(/[^a-zA-Z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'asset';
  let candidate = clean, i = 1;
  while (map.has(candidate)) candidate = `${clean}-${i++}`;
  return candidate;
}

// ---- transport -----------------------------------------------------------

// A scrub always renders full quality — [`sr_set_draft_scale`]'s discount is
// for continuous playback only, never for the single frame a drag lands on.
seek.addEventListener('input', () => {
  playing = false;
  playBtn.textContent = '▶';
  wasm.sr_set_draft_scale(1.0);
  fpsReadout.textContent = '';
  renderAt(parseFloat(seek.value));
});
playBtn.addEventListener('click', () => {
  playing = !playing;
  playBtn.textContent = playing ? '⏸' : '▶';
  playAnchorWall = performance.now();
  playAnchorT = currentT;
  wasm.sr_set_draft_scale(playing ? PLAYBACK_DRAFT_SCALE : 1.0);
  if (playing) {
    frameTimes = [];
  } else {
    fpsReadout.textContent = '';
    renderAt(currentT); // land back on the same frame at full quality
  }
});
document.addEventListener('keydown', (e) => {
  if (e.code === 'Space' && !playBtn.disabled && e.target.tagName !== 'INPUT' && e.target.tagName !== 'TEXTAREA') {
    e.preventDefault(); playBtn.click();
  }
});
function tick(now) {
  requestAnimationFrame(tick);
  if (!playing) return;
  let t = playAnchorT + (now - playAnchorWall) / 1000;
  if (t >= duration) { t = 0; playAnchorT = 0; playAnchorWall = now; }
  renderAt(t);
}
requestAnimationFrame(tick);

// ---- toolbar: open / new still / new clip / save ---------------------

$('btn-open').addEventListener('click', () => $('file-open').click());
$('file-open').addEventListener('change', async (e) => {
  const file = e.target.files[0];
  e.target.value = '';
  if (!file) return;
  try {
    film = normalizeFilm(JSON.parse(stripJsonc(await file.text())));
  } catch (err) {
    fail(`could not parse ${file.name} as a film: ${err.message}`);
    return;
  }
  editor.select(null);
  await doReload();
});

$('btn-new-still').addEventListener('click', () => $('file-still').click());
$('file-still').addEventListener('change', async (e) => {
  const file = e.target.files[0];
  e.target.value = '';
  if (!file) return;
  const name = uniqueAssetName(providedStills, file.name);
  providedStills.set(name, new Uint8Array(await file.arrayBuffer()));
  editor.insertAssetLayer({ ...newLayer('still'), asset: name });
});

$('btn-new-clip').addEventListener('click', () => $('file-clip').click());
$('file-clip').addEventListener('change', async (e) => {
  const file = e.target.files[0];
  e.target.value = '';
  if (!file) return;
  const name = uniqueAssetName(providedClips, file.name);
  providedClips.set(name, file);
  let trim = [0, 3];
  try {
    const info = await probeVideo(file);
    trim = [0, Math.min(3, info.duration)];
  } catch { /* fall back to the default trim; loadAssets will report the real error */ }
  // A server-packed clip's `decode_fps: null` (use the film's own rate) is
  // fine — ffmpeg decoded it once, natively, ahead of time. A browser-added
  // clip is decoded by `clipimport.js`'s seeked-<video> loop instead, which
  // measured at roughly 0.5-1s *per frame* in this environment (see the
  // task writeup) — decoding at a 60fps film's own rate would make a 3s clip
  // a multi-minute import. 12fps keeps a first import tractable; the
  // "Decode fps override" field is right there to raise it.
  editor.insertAssetLayer({ ...newLayer('clip'), asset: name, trim, decode_fps: Math.min(fps || 30, 12) });
});

$('btn-save').addEventListener('click', () => {
  const blob = new Blob([JSON.stringify(film, null, 2)], { type: 'application/json' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `${(film.title || 'film').replace(/[^a-zA-Z0-9_-]+/g, '-')}.film.json`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
});

// ---- export modal -------------------------------------------------------

const exportModal = $('export-modal');
const exportForm = $('export-form');
let exportAbort = null;

function renderExportForm(state) {
  if (state?.running) {
    exportForm.innerHTML = `
      <div class="progress-track"><div class="progress-fill" style="width:${state.pct}%"></div></div>
      <div class="hint">${state.label}</div>
      <div class="modal-actions"><button class="toolbar-btn" data-act="cancel">Cancel</button></div>`;
    return;
  }
  if (!webCodecsAvailable()) {
    exportForm.innerHTML = `
      <div class="hint">This browser has no WebCodecs <code>VideoEncoder</code>, so there is no in-tab
      export route available (see the task writeup for what was evaluated). Use <code>showreel render</code>
      natively, or open this page in a current Chrome/Edge.</div>
      <div class="modal-actions"><button class="toolbar-btn" data-act="close">Close</button></div>`;
    return;
  }
  exportForm.innerHTML = `
    <div class="hint">Exports the film exactly as scrubbed above, video only (no audio yet — see the writeup),
    as WebM/VP8 via WebCodecs. ${film.width}x${film.height} · ${fps}fps · ${duration.toFixed(1)}s
    · ${Math.round(duration * fps)} frames.</div>
    <div class="modal-actions"><button class="toolbar-btn" data-act="cancel">Cancel</button>
      <button class="toolbar-btn primary" data-act="start">Export</button></div>`;
}

$('btn-export').addEventListener('click', () => { exportModal.hidden = false; renderExportForm(); });
exportForm.addEventListener('click', async (e) => {
  const act = e.target.closest('[data-act]')?.dataset.act;
  if (!act) return;
  if (act === 'cancel' || act === 'close') {
    if (exportAbort) exportAbort.abort();
    exportModal.hidden = true;
    return;
  }
  if (act === 'start') {
    const wasPlaying = playing;
    playing = false;
    // Export always reads full quality, regardless of whether playback left
    // the draft scale engaged.
    wasm.sr_set_draft_scale(1.0);
    fpsReadout.textContent = '';
    exportAbort = new AbortController();
    renderExportForm({ running: true, pct: 0, label: 'starting…' });
    try {
      const blob = await exportWebM({
        width: film.width, height: film.height, fps, duration,
        signal: exportAbort.signal,
        renderFrame: (t) => {
          if (!wasm.sr_render_at(t)) throw new Error(bridge.lastError());
          const w = wasm.sr_frame_width(), h = wasm.sr_frame_height();
          return new Uint8Array(bridge.mem.buffer, wasm.sr_frame_ptr(), w * h * 4).slice();
        },
        onProgress: (done, total) => renderExportForm({ running: true, pct: Math.round((done / total) * 100), label: `frame ${done}/${total}` }),
      });
      if (blob) {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = `${(film.title || 'film').replace(/[^a-zA-Z0-9_-]+/g, '-')}.webm`;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 4000);
      }
      exportModal.hidden = true;
    } catch (err) {
      renderExportForm();
      fail(`export failed: ${String(err && err.message || err)}`);
    } finally {
      renderAt(currentT);
      playing = wasPlaying;
      if (wasPlaying) {
        wasm.sr_set_draft_scale(PLAYBACK_DRAFT_SCALE);
        playAnchorWall = performance.now();
        playAnchorT = currentT;
      }
    }
  }
});

// ---- boot ------------------------------------------------------------

async function boot() {
  const res = await fetch(WASM);
  if (!res.ok) throw new Error(`fetch ${WASM}: ${res.status} — run ./build-wasm.sh first`);
  const { instance } = await WebAssembly.instantiate(await res.arrayBuffer(), {});
  wasm = instance.exports;
  bridge = new WasmBridge(wasm);

  splash.textContent = 'loading fonts…';
  await loadFonts();

  splash.textContent = 'loading film…';
  const res2 = await fetch('./film.json').catch(() => null);
  if (res2 && res2.ok) {
    film = normalizeFilm(JSON.parse(stripJsonc(await res2.text())));
  } else {
    film = newFilm();
  }

  await doReload();
  splash.hidden = true;
  seek.disabled = false;
  playBtn.disabled = false;
}

boot().catch((e) => { fail(String(e && e.stack || e)); splash.textContent = 'failed to start — see the error above'; });
