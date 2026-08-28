// Frame-space geometry for the browser editor's "click the picture to
// select it, drag it to move it" interactions — a from-scratch JS mirror of
// a narrow slice of `src/geom.rs` and `src/layer.rs`'s placement resolution,
// used only to draw a selection overlay and compute a drag target. It never
// touches wasm and never affects a rendered pixel; the wasm renderer stays
// the single source of truth for what a film actually looks like.
//
// `Placement::Full`, `::Rect` and `::Frac` resolve to an exact box from the
// frame size alone (see `Placement::resolve`, src/layer.rs) — no font
// metrics involved — so those three are reproduced here exactly. A bare
// `Content::Text` layer's "natural size" fed to `Placement::Anchored` is
// also a fixed `(frame.w*0.8, frame.h*0.3)` regardless of what the text
// says (src/layer.rs's `draw_content`), so that one is exact too.
//
// `Title`/`LowerThird`/`Counter` anchor against their *measured* text plate
// (real font metrics, computed only on the Rust side) — this module has no
// font engine, so their default (unset) placement box is an approximation:
// a fixed size roughly matching what that content typically renders at,
// centred/anchored the same way Rust would. Good enough to click and to
// start a drag; the moment a drag commits, the layer's placement becomes an
// explicit `Frac` box, which is exact from then on — the approximation only
// ever applies to a layer nobody has touched yet.
const ANCHOR_FRACTIONS = {
  'top-left': [0, 0], top: [0.5, 0], 'top-right': [1, 0],
  left: [0, 0.5], centre: [0.5, 0.5], right: [1, 0.5],
  'bottom-left': [0, 1], bottom: [0.5, 1], 'bottom-right': [1, 1],
};

function anchorPlace(anchor, frame, w, h, pad) {
  const [fx, fy] = ANCHOR_FRACTIONS[anchor] || ANCHOR_FRACTIONS.centre;
  const inner = { x: frame.x + pad, y: frame.y + pad, w: frame.w - 2 * pad, h: frame.h - 2 * pad };
  return { x: inner.x + (inner.w - w) * fx, y: inner.y + (inner.h - h) * fy, w, h };
}

export function resolvePlacement(placement, frame, natural) {
  if (placement === null || placement === undefined) return null;
  if (typeof placement === 'string') {
    if (placement.toLowerCase() === 'full') return { x: frame.x, y: frame.y, w: frame.w, h: frame.h };
    return anchorPlace(placement.toLowerCase(), frame, natural.w, natural.h, 72);
  }
  if ('fx' in placement) {
    return { x: frame.w * placement.fx, y: frame.h * placement.fy, w: frame.w * placement.fw, h: frame.h * placement.fh };
  }
  if ('x' in placement) {
    return { x: placement.x, y: placement.y, w: placement.w, h: placement.h };
  }
  if ('anchor' in placement) {
    return anchorPlace(placement.anchor, frame, natural.w, natural.h, placement.pad ?? 72);
  }
  return null;
}

// Approximate default sizes for anchor-placed text content, in frame
// pixels. Matches `Content::default_placement` (src/layer.rs) for anchor +
// pad; the box size itself is a stand-in for a measured text plate.
const APPROX_NATURAL = {
  text: (frame) => ({ w: frame.w * 0.8, h: frame.h * 0.3 }), // exact — see module doc
  title: (frame) => ({ w: frame.w * 0.62, h: frame.h * 0.24 }),
  'lower-third': (frame) => ({ w: frame.w * 0.38, h: frame.h * 0.14 }),
  counter: (frame) => ({ w: frame.w * 0.22, h: frame.h * 0.14 }),
};

const DEFAULT_ANCHOR = { text: 'centre', title: 'centre', 'lower-third': 'bottom-left', counter: 'top-right' };
const DEFAULT_PAD = { text: 72, title: 72, 'lower-third': 96, counter: 72 };

export const DRAGGABLE_KINDS = new Set(Object.keys(APPROX_NATURAL));

// The on-screen box a layer would draw into, in frame pixels — or `null` for
// a content kind this module doesn't track (stills/clips/solid fills etc.,
// which aren't meant to be dragged around the canvas today).
export function layerFrameRect(layer, film) {
  if (!DRAGGABLE_KINDS.has(layer.type)) return null;
  const frame = { x: 0, y: 0, w: film.width, h: film.height };
  if (layer.placement) return resolvePlacement(layer.placement, frame, { w: 0, h: 0 });
  const natural = APPROX_NATURAL[layer.type](frame);
  return anchorPlace(DEFAULT_ANCHOR[layer.type], frame, natural.w, natural.h, DEFAULT_PAD[layer.type]);
}

export function pointInRect(x, y, r) {
  return r && x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
}

// A callout doesn't go through `Placement` at all — `target` and `label_at`
// (src/layer.rs's `CalloutSpec`) are already plain `[fx, fy]` fractions, so
// unlike everything above, both pins are exact: no natural-size guess, no
// font metrics, nothing approximated. That makes it the cleanest case for
// "point at where it should go" — there's no rendering detail this module
// doesn't already know.
export function calloutPins(layer, film) {
  return {
    target: { x: layer.target[0] * film.width, y: layer.target[1] * film.height },
    label: { x: layer.label_at[0] * film.width, y: layer.label_at[1] * film.height },
  };
}
export function fracFromFrame(x, y, film) {
  return [x / film.width, y / film.height];
}
export function calloutHit(layer, film, fx, fy, radius = 44) {
  const p = calloutPins(layer, film);
  const near = (a) => Math.hypot(fx - a.x, fy - a.y) <= radius;
  return near(p.target) || near(p.label);
}

// Turns a frame-pixel rect into the `Placement::Frac` object that reproduces
// it exactly (`Placement::Frac`'s `resolve` is pure arithmetic on the frame
// size, so this round-trips with no approximation, unlike the anchor guess
// above).
export function rectToFrac(rect, film) {
  return { fx: rect.x / film.width, fy: rect.y / film.height, fw: rect.w / film.width, fh: rect.h / film.height };
}
