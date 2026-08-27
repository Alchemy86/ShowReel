// Packs already-JPEG-encoded frames into the `.srclip` container `src/webclip.rs`
// decodes (see that module's doc comment for the exact byte layout — this is
// its JS-side encoder, kept byte-for-byte in step with it by hand since nothing
// generates both sides from one definition).
//
// This is how a clip dropped into the *browser* editor becomes an asset
// `sr_add_clip` (src/wasm.rs) will accept: the browser has no ffmpeg to do
// what `showreel web-pack` does natively, but it can decode its own video
// element frame-by-frame (see clipimport.js) and JPEG-encode each one with
// the canvas it already has — producing the exact container the wasm build
// already knows how to unpack, with no change to the Rust side at all.
//
// Pure byte-array code, no browser API — see test-muxer.mjs's sibling test
// for how this gets checked without a page.

const MAGIC = new Uint8Array([0x53, 0x52, 0x43, 0x4c, 0x49, 0x50, 0x31, 0x00]); // b"SRCLIP1\0"

function u32le(n) {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, n, true);
  return b;
}

function f64le(n) {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setFloat64(0, n, true);
  return b;
}

/**
 * @param {{width:number, height:number, fps:number, frames: Uint8Array[]}} clip
 *   `frames` are JPEG-encoded bytes, one per decoded frame, in playback order.
 * @returns {Uint8Array} a `.srclip` container, ready for `sr_add_clip`.
 */
export function packSrclip({ width, height, fps, frames }) {
  const parts = [MAGIC, u32le(width), u32le(height), f64le(fps), u32le(frames.length)];
  for (const jpeg of frames) {
    parts.push(u32le(jpeg.length), jpeg);
  }
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}
