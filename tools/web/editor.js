// Editing a film in the browser: the scene/layer/transition tree, and the
// panels in `tools/web/index.html` that read and write it.
//
// The whole editing model is one rule: **the JS object *is* the wire JSON at
// all times.** There is no internal-only shape that gets converted on save —
// a layer's `placement` field here is exactly `Placement`'s untagged wire
// form (`src/layer.rs`), a transition's `presentation` is exactly
// `Presentation`'s tagged form (`src/transition.rs`), and so on. That means
// nothing needs a translation step before `main.js` hands the tree to
// `sr_load_film`, and the "advanced" JSON textarea every inspector carries
// can show and accept *exactly* what a hand-written film file would — the
// same object, not a projection of it. It also means every constructor below
// mirrors the matching Rust builder's own defaults (`Layer::title()`,
// `Motion::rise()`, ...) so a film built by clicking "Add layer → Title"
// and one built by `Layer::title("...")` start out identical.
//
// This module owns tree *mutation* and the two side panels' markup. It knows
// nothing about wasm, fetch, or files — `main.js` owns the wasm boundary and
// calls back in here only to hand over a freshly loaded film or a
// freshly-imported asset-backed layer.

const ACCENT = '#ffd147';

const SPRING_GENTLE = { mass: 1, stiffness: 120, damping: 20, clamp: false };

const ANCHORS = ['top-left', 'top', 'top-right', 'left', 'centre', 'right', 'bottom-left', 'bottom', 'bottom-right'];
const DIRECTIONS = ['left', 'right', 'up', 'down'];
const EASINGS = [
  'linear', 'in-out-cubic', 'in-quad', 'out-quad', 'in-out-quad', 'in-cubic', 'out-cubic',
  'in-quart', 'out-quart', 'in-out-quart', 'in-expo', 'out-expo', 'in-out-expo',
  'in-circ', 'out-circ', 'in-out-circ', 'in-sine', 'out-sine', 'in-out-sine',
  'in-back', 'out-back', 'in-out-back', 'out-elastic', 'out-bounce', 'step',
];
const PRESENTATIONS = ['cut', 'dissolve', 'fade', 'wipe', 'slide', 'push', 'iris', 'zoom-in'];
const MOTION_KINDS = ['none', 'fade', 'rise', 'drop', 'slide-in', 'scale', 'chars', 'words'];
const CLIP_MODES = ['hold', 'loop', 'stop'];
const FITS = ['cover', 'contain', 'stretch', 'none'];

export const LAYER_KINDS = ['solid', 'gradient', 'scrim', 'still', 'clip', 'text', 'title', 'lower-third', 'counter', 'callout', 'pull-up'];
export const LAYER_LABELS = {
  solid: 'Solid colour', gradient: 'Gradient', scrim: 'Scrim',
  still: 'Still image', clip: 'Video clip', text: 'Text',
  title: 'Title', 'lower-third': 'Lower third', counter: 'Counter',
  callout: 'Callout', 'pull-up': 'Pull-up',
};

// ---- templates: the vocabulary a film is built from, matching src/layer.rs's builders ----

export function newFilm() {
  return {
    width: 1920, height: 1080, fps: 30,
    title: 'Untitled film',
    background: '#080a0e',
    audio: [],
    opening: newScene('Opening'),
    then: [],
  };
}

export function newScene(name) {
  return { name: name || null, duration: 3, background: null, layers: [] };
}

export function newTransition() {
  return { duration: 0.6, presentation: { kind: 'dissolve' }, timing: { kind: 'eased', easing: 'in-out-cubic' } };
}

export function presentationTemplate(kind) {
  switch (kind) {
    case 'cut': return { kind: 'cut' };
    case 'dissolve': return { kind: 'dissolve' };
    case 'fade': return { kind: 'fade', through: '#000000' };
    case 'wipe': return { kind: 'wipe', direction: 'left', softness: 64 };
    case 'slide': return { kind: 'slide', direction: 'left' };
    case 'push': return { kind: 'push', direction: 'left' };
    case 'iris': return { kind: 'iris', cx: 0.5, cy: 0.5, softness: 24 };
    case 'zoom-in': return { kind: 'zoom-in', from: 0.86 };
    default: return { kind: 'dissolve' };
  }
}

function motionTemplate(kind, duration) {
  const timing = { kind: 'eased', easing: 'out-cubic' };
  switch (kind) {
    case 'none': return null;
    case 'fade': return { kind: 'fade', duration, timing };
    case 'rise': return { kind: 'rise', distance: 40, duration, timing: { kind: 'spring', spring: SPRING_GENTLE } };
    case 'drop': return { kind: 'drop', distance: 40, duration, timing: { kind: 'spring', spring: SPRING_GENTLE } };
    case 'slide-in': return { kind: 'slide-in', dx: -80, dy: 0, duration, timing: { kind: 'spring', spring: SPRING_GENTLE } };
    case 'scale': return { kind: 'scale', from: 0.86, duration, timing: { kind: 'spring', spring: SPRING_GENTLE } };
    case 'chars': return { kind: 'chars', stagger: 0.02, rise: 26, duration, timing: { kind: 'spring', spring: SPRING_GENTLE } };
    case 'words': return { kind: 'words', stagger: 0.06, rise: 30, duration, timing: { kind: 'spring', spring: SPRING_GENTLE } };
    default: return null;
  }
}

const layerBase = () => ({ from: 0, duration: null, placement: null, enter: null, exit: null, opacity: 1, z: 0 });

export function newLayer(kind) {
  const base = layerBase();
  switch (kind) {
    case 'solid':
      return { type: 'solid', colour: '#334155', ...base };
    case 'gradient':
      return { type: 'gradient', stops: [[0, '#0d1117'], [1, '#334155']], angle: 0, ...base };
    case 'scrim':
      return { type: 'scrim', height: 0.42, strength: 0.85, colour: '#000000', ...base };
    case 'still':
      return { type: 'still', asset: '', fit: 'cover', camera: null, ...base };
    case 'clip':
      return { type: 'clip', asset: '', fit: 'cover', camera: null, trim: [0, 3], start: 0, decode_fps: null, mode: 'hold', max_width: 1920, radius: 0, border: null, shadow: null, ...base };
    case 'text':
      // Unlike every other content kind, `Content::Text.style` is a bare
      // `TextStyle`, not `Option<TextStyle>` (src/layer.rs) — `null` fails to
      // deserialise ("invalid type: null, expected struct TextStyle"), so
      // the key is left out entirely and Rust's own `#[serde(default)]`
      // supplies `TextStyle::default()`.
      return { type: 'text', text: 'Text', wrap: 1, fit: false, ...base };
    case 'title':
      return { type: 'title', text: 'Title', subtitle: null, style: null, subtitle_style: null, ...base, enter: motionTemplate('rise', 0.7) };
    case 'lower-third':
      return {
        type: 'lower-third', text: 'Lower third', detail: null, accent: ACCENT, style: null, detail_style: null,
        ...base,
        enter: { kind: 'slide-in', dx: -80, dy: 0, duration: 0.55, timing: { kind: 'spring', spring: SPRING_GENTLE } },
        exit: { kind: 'slide-in', dx: -60, dy: 0, duration: 0.35, timing: { kind: 'spring', spring: SPRING_GENTLE } },
      };
    case 'counter':
      return {
        type: 'counter',
        count: { from: 0, to: 100, over: 2, easing: 'out-expo', decimals: 0, group: true, prefix: '', suffix: '' },
        label: null, style: null, label_style: null,
        ...base, enter: motionTemplate('rise', 0.4),
      };
    case 'callout':
      return {
        type: 'callout', target: [0.5, 0.5], label_at: [0.5, 0.3], text: 'Callout', detail: null, accent: ACCENT, ring: 26,
        style: null, detail_style: null,
        ...base, enter: motionTemplate('fade', 0.45),
      };
    case 'pull-up':
      return {
        type: 'pull-up', region: [0.3, 0.3, 0.4, 0.4], to: null, dim: 0.62, radius: 14, border: '#ffffff', border_width: 3, label: null, tether: true,
        style: null,
        ...base, enter: motionTemplate('scale', 0.6),
      };
    default:
      throw new Error(`unknown layer kind ${kind}`);
  }
}

export function newAudio() {
  return { asset: '', at: 0, from: 0, duration: null, fade_in: 0, fade_out: 0, gain: 1 };
}

// ---- timeline accessors: Scene (Transition Scene)* — src/timeline.rs ----

export function sceneCount(film) {
  return 1 + film.then.length;
}
export function getScene(film, i) {
  return i === 0 ? film.opening : film.then[i - 1].scene;
}
export function setScene(film, i, scene) {
  if (i === 0) film.opening = scene; else film.then[i - 1].scene = scene;
}
export function getTransitionInto(film, i) {
  return i > 0 ? film.then[i - 1].transition : null;
}
export function addSceneAfter(film, i) {
  film.then.splice(i, 0, { transition: newTransition(), scene: newScene() });
  return i + 1;
}
export function deleteScene(film, i) {
  if (sceneCount(film) <= 1) return;
  if (i === 0) film.opening = film.then.shift().scene;
  else film.then.splice(i - 1, 1);
}
export function moveScene(film, i, dir) {
  const j = i + dir;
  if (j < 0 || j >= sceneCount(film)) return false;
  const a = getScene(film, i), b = getScene(film, j);
  setScene(film, i, b);
  setScene(film, j, a);
  return true;
}
function placedTimes(film) {
  // Mirrors Timeline::placements — used only to label the timeline panel.
  const out = [];
  let start = 0;
  for (let i = 0; i < sceneCount(film); i++) {
    if (i === 0) { out.push(0); continue; }
    const tr = getTransitionInto(film, i);
    start = out[i - 1] + getScene(film, i - 1).duration - (tr ? tr.duration : 0);
    out.push(start);
  }
  return out;
}

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}
function num(v) {
  return v === null || v === undefined || Number.isNaN(v) ? '' : v;
}

// ---- field builders: every inspector form is assembled from these ----

// Every field gets an id derived from its bind path so `<label for>` really
// associates with its control (screen readers, and label-click-to-focus) —
// paths are unique within whichever single inspector form is on screen at a
// time, which is the only place a given id needs to be unique.
let fieldSeq = 0;
function fieldId(path) {
  return `f-${path.replace(/[^a-zA-Z0-9]/g, '-')}-${fieldSeq++}`;
}
function fRow(...fields) {
  return `<div class="field-row">${fields.join('')}</div>`;
}
function fNumber(label, path, value, opts = {}) {
  const id = fieldId(path);
  const attrs = [opts.step !== undefined ? `step="${opts.step}"` : 'step="any"', opts.min !== undefined ? `min="${opts.min}"` : ''].join(' ');
  return `<div class="field"><label for="${id}">${esc(label)}</label><input id="${id}" type="number" data-bind="${path}" data-kind="number" value="${num(value)}" ${attrs}></div>`;
}
function fNullableNumber(label, path, value, opts = {}) {
  const id = fieldId(path);
  const attrs = [opts.step !== undefined ? `step="${opts.step}"` : 'step="any"'].join(' ');
  return `<div class="field"><label for="${id}">${esc(label)}</label><input id="${id}" type="number" data-bind="${path}" data-kind="nullable-number" value="${value === null || value === undefined ? '' : value}" placeholder="${esc(opts.placeholder || '(default)')}" ${attrs}></div>`;
}
function fText(label, path, value) {
  const id = fieldId(path);
  return `<div class="field"><label for="${id}">${esc(label)}</label><input id="${id}" type="text" data-bind="${path}" data-kind="string" value="${esc(value)}"></div>`;
}
function fColor(label, path, value) {
  const id = fieldId(path);
  const hex = /^#[0-9a-fA-F]{6}/.test(value || '') ? value.slice(0, 7) : '#ffffff';
  return `<div class="field"><label for="${id}">${esc(label)}</label><input id="${id}" type="color" data-bind="${path}" data-kind="string" value="${hex}"></div>`;
}
function fCheck(label, path, checked) {
  const id = fieldId(path);
  return `<div class="field check-field"><input id="${id}" type="checkbox" data-bind="${path}" data-kind="bool" ${checked ? 'checked' : ''}><label for="${id}">${esc(label)}</label></div>`;
}
function fSelect(label, path, value, options, labels) {
  const id = fieldId(path);
  const opts = options.map((o) => `<option value="${esc(o)}" ${o === value ? 'selected' : ''}>${esc(labels?.[o] || o)}</option>`).join('');
  return `<div class="field"><label for="${id}">${esc(label)}</label><select id="${id}" data-bind="${path}" data-kind="string">${opts}</select></div>`;
}
function fTextarea(label, path, value) {
  const id = fieldId(path);
  return `<div class="field"><label for="${id}">${esc(label)}</label><textarea id="${id}" data-bind="${path}" data-kind="string" rows="2">${esc(value)}</textarea></div>`;
}

function advancedJson(getObj, label) {
  const json = JSON.stringify(getObj(), null, 2);
  return `<details class="adv"><summary>${esc(label || 'Advanced (raw JSON)')}</summary>
    <textarea class="adv-json" rows="10">${esc(json)}</textarea>
    <div class="field-row" style="margin-top:6px"><button class="mini-btn" data-act="apply-json">Apply</button>
    <span class="hint">Full field vocabulary lives here — anything without a control above.</span></div>
  </details>`;
}

// ---- placement editor: exactly Placement's untagged wire form (src/layer.rs) ----

function placementKind(p) {
  if (p === null || p === undefined) return 'default';
  if (typeof p === 'string') return p.toLowerCase() === 'full' ? 'full' : 'anchor-word';
  if ('fx' in p) return 'frac';
  if ('anchor' in p) return 'anchor';
  if ('x' in p) return 'rect';
  return 'default';
}
function placementDefault(kind) {
  switch (kind) {
    case 'default': return null;
    case 'full': return 'full';
    case 'anchor': return { anchor: 'centre', pad: 72 };
    case 'frac': return { fx: 0.3, fy: 0.3, fw: 0.4, fh: 0.3 };
    case 'rect': return { x: 100, y: 100, w: 400, h: 240 };
    default: return null;
  }
}
function renderPlacement(placement) {
  const kind = placementKind(placement);
  let extra = '';
  if (kind === 'anchor' || kind === 'anchor-word') {
    // `kind === 'anchor-word'` is the bare-word shorthand (`"placement":
    // "centre"`) — shown with the same fields; touching either one promotes
    // it to the object form via `applyBind`'s normalisation.
    const anchor = kind === 'anchor-word' ? placement : placement.anchor;
    const pad = kind === 'anchor-word' ? 72 : placement.pad;
    extra = fRow(fSelect('Anchor', 'placement.anchor', anchor, ANCHORS), fNumber('Pad', 'placement.pad', pad));
  } else if (kind === 'frac') {
    extra = fRow(fNumber('fx', 'placement.fx', placement.fx, { step: 0.01 }), fNumber('fy', 'placement.fy', placement.fy, { step: 0.01 }))
      + fRow(fNumber('fw', 'placement.fw', placement.fw, { step: 0.01 }), fNumber('fh', 'placement.fh', placement.fh, { step: 0.01 }));
  } else if (kind === 'rect') {
    extra = fRow(fNumber('x (px)', 'placement.x', placement.x), fNumber('y (px)', 'placement.y', placement.y))
      + fRow(fNumber('w (px)', 'placement.w', placement.w), fNumber('h (px)', 'placement.h', placement.h));
  }
  const id = fieldId('placement-kind');
  return `<div class="field"><label for="${id}">Placement</label>
    <select id="${id}" data-act="placement-kind">
      <option value="default" ${kind === 'default' ? 'selected' : ''}>Default for this content</option>
      <option value="full" ${kind === 'full' ? 'selected' : ''}>Full frame</option>
      <option value="anchor" ${kind === 'anchor' || kind === 'anchor-word' ? 'selected' : ''}>Anchored</option>
      <option value="frac" ${kind === 'frac' ? 'selected' : ''}>Fraction of frame</option>
      <option value="rect" ${kind === 'rect' ? 'selected' : ''}>Exact pixels</option>
    </select></div>${extra}`;
}

// ---- motion (enter/exit) editor: Motion's flattened wire form (src/motion.rs) ----

function renderMotion(label, act, motion) {
  const kind = motion ? motion.kind : 'none';
  let extra = '';
  if (motion) {
    extra += fNumber('Duration (s)', `${act}.duration`, motion.duration, { step: 0.05 });
    if ('distance' in motion) extra += fNumber('Distance (px)', `${act}.distance`, motion.distance);
    if ('dx' in motion) extra += fRow(fNumber('dx (px)', `${act}.dx`, motion.dx), fNumber('dy (px)', `${act}.dy`, motion.dy));
    if ('from' in motion) extra += fNumber('Start scale', `${act}.from`, motion.from, { step: 0.02 });
    if ('stagger' in motion) extra += fRow(fNumber('Stagger (s)', `${act}.stagger`, motion.stagger, { step: 0.01 }), fNumber('Rise (px)', `${act}.rise`, motion.rise));
    const timingKind = motion.timing?.kind === 'spring' ? 'spring' : 'eased';
    const timingId = fieldId(`${act}-timing`);
    extra += `<div class="field"><label for="${timingId}">Pacing</label><select id="${timingId}" data-act="${act}-timing">
      <option value="eased" ${timingKind === 'eased' ? 'selected' : ''}>Eased</option>
      <option value="spring" ${timingKind === 'spring' ? 'selected' : ''}>Spring</option>
    </select></div>`;
    if (timingKind === 'eased') extra += fSelect('Easing', `${act}.timing.easing`, motion.timing.easing, EASINGS);
  }
  const kindId = fieldId(`${act}-kind`);
  return `<div class="field"><label for="${kindId}">${esc(label)}</label><select id="${kindId}" data-act="${act}-kind">
    ${MOTION_KINDS.map((k) => `<option value="${k}" ${k === kind ? 'selected' : ''}>${k === 'none' ? 'None' : k}</option>`).join('')}
  </select></div>${extra}`;
}

// ---- per-content-type inspector fields ----

function renderContentFields(layer) {
  switch (layer.type) {
    case 'solid':
      return fColor('Colour', 'colour', layer.colour);
    case 'gradient':
      return `<div class="field"><label>Stops (JSON array of [t, colour])</label>
        <textarea data-bind="stops" data-kind="json" rows="2">${esc(JSON.stringify(layer.stops))}</textarea></div>`
        + fNumber('Angle (deg)', 'angle', layer.angle);
    case 'scrim':
      return fRow(fNumber('Height (frac)', 'height', layer.height, { step: 0.02 }), fNumber('Strength', 'strength', layer.strength, { step: 0.02 }))
        + fColor('Colour', 'colour', layer.colour);
    case 'still':
      return fText('Asset', 'asset', layer.asset) + fSelect('Fit', 'fit', layer.fit, FITS)
        + `<div class="hint">Camera moves aren't editable here yet — use Advanced JSON.</div>`;
    case 'clip': {
      const trim = layer.trim || [0, 0];
      return fText('Asset', 'asset', layer.asset) + fSelect('Fit', 'fit', layer.fit, FITS)
        + fRow(fNumber('Trim start (s)', 'trim.0', trim[0], { step: 0.1, min: 0 }), fNumber('Trim length (s)', 'trim.1', trim[1], { step: 0.1, min: 0.1 }))
        + fNumber('Layer start offset (s)', 'start', layer.start, { step: 0.1 })
        + fSelect('When it runs out', 'mode', layer.mode, CLIP_MODES)
        + fNullableNumber('Decode fps override', 'decode_fps', layer.decode_fps, { placeholder: 'film fps' })
        + fNumber('Max decode width (px)', 'max_width', layer.max_width, { min: 16 })
        + `<div class="hint">Changing the trim on a browser-added clip re-decodes it from the attached source file.</div>`;
    }
    case 'text':
      return fTextarea('Text', 'text', layer.text) + fRow(fNumber('Wrap (frac)', 'wrap', layer.wrap, { step: 0.05 }), fCheck('Shrink to fit', 'fit', layer.fit));
    case 'title':
      return fText('Text', 'text', layer.text) + fText('Subtitle', 'subtitle', layer.subtitle || '');
    case 'lower-third':
      return fText('Text', 'text', layer.text) + fText('Detail', 'detail', layer.detail || '') + fColor('Accent', 'accent', layer.accent);
    case 'counter':
      return fRow(fNumber('From', 'count.from', layer.count.from), fNumber('To', 'count.to', layer.count.to))
        + fRow(fNumber('Over (s)', 'count.over', layer.count.over, { step: 0.1 }), fNumber('Decimals', 'count.decimals', layer.count.decimals, { step: 1, min: 0 }))
        + fRow(fText('Prefix', 'count.prefix', layer.count.prefix), fText('Suffix', 'count.suffix', layer.count.suffix))
        + fCheck('Group thousands', 'count.group', layer.count.group)
        + fText('Label', 'label', layer.label || '');
    case 'callout':
      return fText('Text', 'text', layer.text) + fText('Detail', 'detail', layer.detail || '')
        + fRow(fNumber('Target x (frac)', 'target.0', layer.target[0], { step: 0.01 }), fNumber('Target y (frac)', 'target.1', layer.target[1], { step: 0.01 }))
        + fRow(fNumber('Label x (frac)', 'label_at.0', layer.label_at[0], { step: 0.01 }), fNumber('Label y (frac)', 'label_at.1', layer.label_at[1], { step: 0.01 }))
        + fColor('Accent', 'accent', layer.accent) + fNumber('Ring radius (px)', 'ring', layer.ring);
    case 'pull-up':
      return `<div class="field"><label>Region (fx, fy, fw, fh)</label>` + fRow(
        fNumber('', 'region.0', layer.region[0], { step: 0.01 }), fNumber('', 'region.1', layer.region[1], { step: 0.01 }),
      ) + fRow(fNumber('', 'region.2', layer.region[2], { step: 0.01 }), fNumber('', 'region.3', layer.region[3], { step: 0.01 })) + `</div>`
        + fText('Label', 'label', layer.label || '')
        + fRow(fNumber('Dim (0-1)', 'dim', layer.dim, { step: 0.02 }), fNumber('Corner radius', 'radius', layer.radius))
        + fRow(fColor('Border', 'border', layer.border), fNumber('Border width', 'border_width', layer.border_width))
        + fCheck('Tether line', 'tether', layer.tether);
    default:
      return '';
  }
}

// ---- the editor object ----

export function createEditor({ timelineEl, inspectorEl, getFilm, onChange, resolveAssetStatus }) {
  let sel = null; // {kind:'scene'|'transition'|'layer'|'audio', i, j}

  function film() { return getFilm(); }
  function notify() { onChange(); }

  function currentSceneIndex() {
    if (sel?.kind === 'scene') return sel.i;
    if (sel?.kind === 'layer') return sel.i;
    return 0;
  }

  function select(next) {
    sel = next;
    renderInspector();
    renderTimeline();
  }

  function selectedLayer() {
    if (sel?.kind !== 'layer') return null;
    return getScene(film(), sel.i).layers[sel.j];
  }

  function insertAssetLayer(layer) {
    const i = currentSceneIndex();
    const scene = getScene(film(), i);
    scene.layers.push(layer);
    select({ kind: 'layer', i, j: scene.layers.length - 1 });
    notify();
  }

  // ---- timeline panel ----

  function renderTimeline() {
    const f = film();
    const starts = placedTimes(f);
    const n = sceneCount(f);
    let rows = '';
    for (let i = 0; i < n; i++) {
      if (i > 0) {
        const t = getTransitionInto(f, i);
        rows += `<div class="transition-chip" data-sel="t:${i}"><span class="glyph">⇄</span> ${esc(t.presentation.kind)} · ${t.duration.toFixed(2)}s</div>`;
      }
      const scene = getScene(f, i);
      const selected = sel?.kind === 'scene' && sel.i === i || sel?.kind === 'layer' && sel.i === i;
      rows += `<div class="chip-row ${selected ? 'selected' : ''}" data-sel="s:${i}">
        <span class="label">${esc(scene.name || `Scene ${i + 1}`)}</span>
        <span class="meta">${starts[i].toFixed(1)}s · ${scene.duration.toFixed(1)}s</span>
      </div>`;
    }
    const scene = getScene(f, currentSceneIndex());
    let layerRows = '';
    scene.layers.forEach((l, j) => {
      const selected = sel?.kind === 'layer' && sel.i === currentSceneIndex() && sel.j === j;
      layerRows += `<div class="chip-row ${selected ? 'selected' : ''}" data-sel="l:${j}">
        <span class="label">${esc(LAYER_LABELS[l.type] || l.type)}${l.type === 'text' || l.type === 'title' || l.type === 'lower-third' ? `: ${esc((l.text || '').slice(0, 24))}` : ''}</span>
        <span class="meta">z${l.z}</span>
        <button class="mini-btn" data-act="layer-up" data-j="${j}" title="Move up">↑</button>
        <button class="mini-btn" data-act="layer-down" data-j="${j}" title="Move down">↓</button>
        <button class="mini-btn danger" data-act="layer-del" data-j="${j}" title="Delete">✕</button>
      </div>`;
    });
    const addMenu = `<div class="add-menu">
        <button class="mini-btn" data-act="toggle-add-layer">+ Layer</button>
        <div class="add-menu-list" id="add-layer-list" hidden>
          ${LAYER_KINDS.map((k) => `<button data-act="add-layer" data-kind="${k}">${esc(LAYER_LABELS[k])}</button>`).join('')}
        </div>
      </div>`;

    const assetRows = renderAssetRows(f);
    const audioRows = f.audio.map((a, i) => `<div class="chip-row ${sel?.kind === 'audio' && sel.i === i ? 'selected' : ''}" data-sel="a:${i}">
        <span class="label">${esc(a.asset || '(no asset)')}</span><span class="meta">${a.at}s</span>
        <button class="mini-btn danger" data-act="audio-del" data-i="${i}">✕</button>
      </div>`).join('') || '<div class="empty-hint">No audio tracks.</div>';

    timelineEl.innerHTML = `
      <div class="pane">
        <h2>Film</h2>
        ${fText('Title', '__film.title', f.title || '')}
        ${fRow(fNumber('Width', '__film.width', f.width, { step: 2, min: 2 }), fNumber('Height', '__film.height', f.height, { step: 2, min: 2 }))}
        ${fRow(fNumber('FPS', '__film.fps', f.fps, { step: 1, min: 1 }), fColor('Background', '__film.background', f.background))}
      </div>
      <div class="pane">
        <h2>Timeline <button class="mini-btn" data-act="add-scene">+ Scene</button></h2>
        <div class="row">${rows}</div>
      </div>
      <div class="pane">
        <h2>Layers — ${esc(scene.name || `Scene ${currentSceneIndex() + 1}`)} ${addMenu}</h2>
        <div class="row">${layerRows || '<div class="empty-hint">No layers yet.</div>'}</div>
      </div>
      <div class="pane">
        <h2>Assets</h2>
        <div>${assetRows}</div>
      </div>
      <div class="pane">
        <h2>Audio <button class="mini-btn" data-act="add-audio">+ Track</button></h2>
        <div class="row">${audioRows}</div>
      </div>
    `;
  }

  function renderAssetRows(f) {
    const names = new Set();
    for (let i = 0; i < sceneCount(f); i++) {
      for (const l of getScene(f, i).layers) {
        if (l.type === 'still' || l.type === 'clip') if (l.asset) names.add(`${l.type}:${l.asset}`);
      }
    }
    if (!names.size) return '<div class="empty-hint">No stills or clips referenced yet.</div>';
    return [...names].map((key) => {
      const [kind, name] = key.split(/:(.+)/);
      const status = resolveAssetStatus ? resolveAssetStatus(kind, name) : 'unknown';
      return `<div class="asset-row"><span class="dot ${status}"></span><span class="name">${esc(name)}</span><span class="meta">${kind}</span></div>`;
    }).join('');
  }

  // ---- inspector panel ----

  function renderInspector() {
    if (!sel) { inspectorEl.innerHTML = `<div class="pane"><div class="empty-hint">Select a scene, layer, transition or audio track to edit it.</div></div>`; return; }
    const f = film();
    if (sel.kind === 'scene') {
      const scene = getScene(f, sel.i);
      inspectorEl.innerHTML = `<div class="pane">
        <h2>Scene ${sel.i + 1} <button class="mini-btn" data-act="scene-up">↑</button><button class="mini-btn" data-act="scene-down">↓</button>
          <button class="mini-btn danger" data-act="scene-del">Delete</button></h2>
        ${fText('Name', 'name', scene.name || '')}
        ${fNumber('Duration (s)', 'duration', scene.duration, { step: 0.1, min: 0.1 })}
        ${fColor('Background', 'background', scene.background || '#080a0e')}
        ${advancedJson(() => scene, 'Scene JSON')}
      </div>`;
    } else if (sel.kind === 'transition') {
      const t = getTransitionInto(f, sel.i);
      const p = t.presentation;
      let extra = '';
      if (p.kind === 'fade') extra = fColor('Through colour', 'presentation.through', p.through);
      if (p.kind === 'wipe') extra = fRow(fSelect('Direction', 'presentation.direction', p.direction, DIRECTIONS), fNumber('Softness (px)', 'presentation.softness', p.softness));
      if (p.kind === 'slide' || p.kind === 'push') extra = fSelect('Direction', 'presentation.direction', p.direction, DIRECTIONS);
      if (p.kind === 'iris') extra = fRow(fNumber('cx (frac)', 'presentation.cx', p.cx, { step: 0.01 }), fNumber('cy (frac)', 'presentation.cy', p.cy, { step: 0.01 })) + fNumber('Softness (px)', 'presentation.softness', p.softness);
      if (p.kind === 'zoom-in') extra = fNumber('Starting scale', 'presentation.from', p.from, { step: 0.02 });
      const timingKind = t.timing.kind;
      inspectorEl.innerHTML = `<div class="pane">
        <h2>Transition into scene ${sel.i + 1}</h2>
        ${fNumber('Duration (s)', 'duration', t.duration, { step: 0.05, min: 0 })}
        ${fSelect('Look', 'presentation.kind', p.kind, PRESENTATIONS)}
        ${extra}
        <div class="field"><label for="transition-pacing">Pacing</label><select id="transition-pacing" data-act="transition-timing-kind">
          <option value="eased" ${timingKind === 'eased' ? 'selected' : ''}>Eased</option>
          <option value="spring" ${timingKind === 'spring' ? 'selected' : ''}>Spring</option>
        </select></div>
        ${timingKind === 'eased' ? fSelect('Easing', 'timing.easing', t.timing.easing, EASINGS) : ''}
        ${advancedJson(() => t, 'Transition JSON')}
      </div>`;
    } else if (sel.kind === 'layer') {
      const layer = selectedLayer();
      if (!layer) { select(null); return; }
      inspectorEl.innerHTML = `<div class="pane">
        <h2>${esc(LAYER_LABELS[layer.type] || layer.type)}</h2>
        ${renderContentFields(layer)}
      </div>
      <div class="pane">
        <h2>Timing &amp; placement</h2>
        ${fRow(fNumber('From (s)', 'from', layer.from, { step: 0.05, min: 0 }), fNullableNumber('Duration (s)', 'duration', layer.duration, { placeholder: 'to scene end', step: 0.05 }))}
        ${fRow(fNumber('Opacity', 'opacity', layer.opacity, { step: 0.05, min: 0 }), fNumber('Z (draw order)', 'z', layer.z, { step: 1 }))}
        ${renderPlacement(layer.placement)}
      </div>
      <div class="pane">
        <h2>Motion</h2>
        ${renderMotion('Enter', 'enter', layer.enter)}
        ${renderMotion('Exit', 'exit', layer.exit)}
      </div>
      <div class="pane">
        ${advancedJson(() => layer, 'Layer JSON — every field, including camera moves, text style and shadows')}
      </div>`;
    } else if (sel.kind === 'audio') {
      const a = f.audio[sel.i];
      inspectorEl.innerHTML = `<div class="pane">
        <h2>Audio track ${sel.i + 1}</h2>
        ${fText('Asset', 'asset', a.asset)}
        ${fRow(fNumber('At (film s)', 'at', a.at, { step: 0.1, min: 0 }), fNumber('From (source s)', 'from', a.from, { step: 0.1, min: 0 }))}
        ${fNullableNumber('Duration (s)', 'duration', a.duration, { placeholder: 'to end of film', step: 0.1 })}
        ${fRow(fNumber('Fade in (s)', 'fade_in', a.fade_in, { step: 0.1, min: 0 }), fNumber('Fade out (s)', 'fade_out', a.fade_out, { step: 0.1, min: 0 }))}
        ${fNumber('Gain', 'gain', a.gain, { step: 0.05, min: 0 })}
        <div class="hint">No audio decode/playback in this build yet — see the writeup. Values are still exported to the film JSON.</div>
      </div>`;
    }
  }

  function getSelectedObject() {
    const f = film();
    if (sel.kind === 'scene') return getScene(f, sel.i);
    if (sel.kind === 'transition') return getTransitionInto(f, sel.i);
    if (sel.kind === 'layer') return selectedLayer();
    if (sel.kind === 'audio') return f.audio[sel.i];
    return null;
  }

  function setPath(obj, path, value) {
    const parts = path.split('.');
    let cur = obj;
    for (let i = 0; i < parts.length - 1; i++) cur = cur[parts[i]];
    cur[parts[parts.length - 1]] = value;
  }

  function coerce(input) {
    const kind = input.dataset.kind;
    if (kind === 'number') return parseFloat(input.value) || 0;
    if (kind === 'nullable-number') return input.value === '' ? null : parseFloat(input.value);
    if (kind === 'bool') return input.checked;
    if (kind === 'json') { try { return JSON.parse(input.value); } catch { return undefined; } }
    return input.value;
  }

  // ---- event wiring ----

  timelineEl.addEventListener('click', (e) => {
    const selEl = e.target.closest('[data-sel]');
    const actEl = e.target.closest('[data-act]');
    if (selEl) {
      const [kind, idx] = selEl.dataset.sel.split(':');
      const i = parseInt(idx, 10);
      if (kind === 's') select({ kind: 'scene', i });
      else if (kind === 't') select({ kind: 'transition', i });
      else if (kind === 'l') select({ kind: 'layer', i: currentSceneIndex(), j: i });
      else if (kind === 'a') select({ kind: 'audio', i });
      return;
    }
    if (!actEl) return;
    const act = actEl.dataset.act;
    const f = film();
    if (act === 'add-scene') { const i = addSceneAfter(f, sceneCount(f) - 1); select({ kind: 'scene', i }); notify(); }
    // scene-up/scene-down/scene-del: handled by inspectorEl's own listener —
    // those buttons render inside the scene inspector, not this panel.
    else if (act === 'toggle-add-layer') { document.getElementById('add-layer-list').hidden = !document.getElementById('add-layer-list').hidden; }
    else if (act === 'add-layer') {
      const scene = getScene(f, currentSceneIndex());
      scene.layers.push(newLayer(actEl.dataset.kind));
      select({ kind: 'layer', i: currentSceneIndex(), j: scene.layers.length - 1 });
      notify();
    } else if (act === 'layer-up' || act === 'layer-down') {
      const scene = getScene(f, currentSceneIndex());
      const j = parseInt(actEl.dataset.j, 10);
      const k = act === 'layer-up' ? j - 1 : j + 1;
      if (k >= 0 && k < scene.layers.length) {
        [scene.layers[j], scene.layers[k]] = [scene.layers[k], scene.layers[j]];
        if (sel?.kind === 'layer' && sel.j === j) sel = { ...sel, j: k };
      }
      notify(); renderTimeline();
    } else if (act === 'layer-del') {
      const scene = getScene(f, currentSceneIndex());
      scene.layers.splice(parseInt(actEl.dataset.j, 10), 1);
      select(null); notify();
    } else if (act === 'add-audio') {
      f.audio.push(newAudio());
      select({ kind: 'audio', i: f.audio.length - 1 });
      notify();
    } else if (act === 'audio-del') {
      f.audio.splice(parseInt(actEl.dataset.i, 10), 1);
      select(null); notify();
    }
  });

  timelineEl.addEventListener('input', (e) => {
    const input = e.target.closest('[data-bind]');
    if (!input) return;
    const path = input.dataset.bind;
    const f = film();
    if (path.startsWith('__film.')) {
      setPath(f, path.slice(7), coerce(input));
      notify();
      if (path === '__film.title') { /* header updates on next full reload */ }
    }
  });

  function applyBind(root, input) {
    const obj = getSelectedObject();
    if (!obj) return;
    const path = input.dataset.bind;
    // A `placement` can still be the bare-word string form (`"centre"`,
    // PlacementRepr::Word — src/layer.rs) if it came from an opened film or
    // the advanced JSON editor. Editing its anchor/pad through the ordinary
    // fields only makes sense once it's the object form, so promote it here
    // rather than crashing on `"centre".anchor = ...`.
    if (path.startsWith('placement.') && typeof obj.placement === 'string') {
      obj.placement = { anchor: obj.placement, pad: 72 };
    }
    const value = coerce(input);
    if (value === undefined) return; // bad JSON in a `data-kind="json"` field — leave it for the user to fix
    setPath(obj, path, value);
    notify();
  }

  inspectorEl.addEventListener('input', (e) => {
    const input = e.target.closest('[data-bind]');
    if (input && input.type !== 'color') { applyBind(inspectorEl, input); return; }
  });
  inspectorEl.addEventListener('change', (e) => {
    const input = e.target.closest('[data-bind]');
    if (input && input.dataset.bind === 'presentation.kind') {
      // Presentation's fields differ per kind (Wipe has direction/softness,
      // Fade has through, ...) — a plain applyBind would leave stale fields
      // from whichever kind was picked before, so replace the whole object
      // and re-render rather than just patching `.kind`.
      getTransitionInto(film(), sel.i).presentation = presentationTemplate(input.value);
      renderInspector(); notify();
      return;
    }
    if (input) { applyBind(inspectorEl, input); return; }

    const actEl = e.target.closest('[data-act]');
    if (!actEl) return;
    const act = actEl.dataset.act;
    if (act === 'placement-kind') {
      const layer = selectedLayer();
      layer.placement = placementDefault(actEl.value);
      renderInspector(); notify();
    } else if (act === 'enter-kind' || act === 'exit-kind') {
      const layer = selectedLayer();
      const field = act === 'enter-kind' ? 'enter' : 'exit';
      const dur = layer[field]?.duration ?? 0.5;
      layer[field] = motionTemplate(actEl.value, dur);
      renderInspector(); notify();
    } else if (act === 'enter-timing' || act === 'exit-timing') {
      const layer = selectedLayer();
      const field = act === 'enter-timing' ? 'enter' : 'exit';
      layer[field].timing = actEl.value === 'spring' ? { kind: 'spring', spring: SPRING_GENTLE } : { kind: 'eased', easing: 'out-cubic' };
      renderInspector(); notify();
    } else if (act === 'transition-timing-kind') {
      const t = getTransitionInto(film(), sel.i);
      t.timing = actEl.value === 'spring' ? { kind: 'spring', spring: SPRING_GENTLE } : { kind: 'eased', easing: 'in-out-cubic' };
      renderInspector(); notify();
    }
  });

  inspectorEl.addEventListener('click', (e) => {
    const actEl = e.target.closest('[data-act]');
    if (!actEl) return;
    // The scene ↑/↓/Delete buttons live in this panel (they're part of the
    // scene inspector), not the timeline panel, so they're handled here
    // rather than in `timelineEl`'s listener even though every other
    // scene/layer mutation is.
    const f = film();
    if (actEl.dataset.act === 'scene-up') { moveScene(f, sel.i, -1) && select({ kind: 'scene', i: sel.i - 1 }); notify(); return; }
    if (actEl.dataset.act === 'scene-down') { moveScene(f, sel.i, 1) && select({ kind: 'scene', i: sel.i + 1 }); notify(); return; }
    if (actEl.dataset.act === 'scene-del') { deleteScene(f, sel.i); select(null); notify(); return; }
    if (actEl.dataset.act !== 'apply-json') return;
    const ta = actEl.closest('details').querySelector('.adv-json');
    try {
      const parsed = JSON.parse(ta.value);
      if (sel.kind === 'scene') setScene(film(), sel.i, parsed);
      else if (sel.kind === 'transition') { getTransitionInto(film(), sel.i); film().then[sel.i - 1].transition = parsed; }
      else if (sel.kind === 'layer') { getScene(film(), sel.i).layers[sel.j] = parsed; }
      else if (sel.kind === 'audio') { film().audio[sel.i] = parsed; }
      renderInspector(); renderTimeline(); notify();
    } catch (err) {
      ta.title = `invalid JSON: ${err.message}`;
    }
  });

  return {
    render() { renderTimeline(); renderInspector(); },
    select,
    getSelection: () => sel,
    currentSceneIndex,
    insertAssetLayer,
  };
}
