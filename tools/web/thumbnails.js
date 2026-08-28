// Cheap, one-time thumbnails for stills and clips — the single
// highest-leverage change docs/youcut-study.md names: recognising footage by
// what's in it instead of reading a filename. Generated once per unique
// asset (main.js's `thumbCache` keeps the result across reloads) and never
// recomputed per frame or per row.

const THUMB_W = 120, THUMB_H = 68; // ~16:9, small enough to be cheap to hold many of

// A still's raw bytes, downscaled to a small JPEG object URL via one
// `createImageBitmap` decode — the browser's own scaled-decode path, not a
// full-resolution decode followed by a manual canvas downscale. That
// difference is the point for something like `kanto.png`'s 48-megapixel
// atlas: this never holds a full-size bitmap in memory.
export async function stillThumbnail(bytes) {
  const bitmap = await createImageBitmap(new Blob([bytes]), {
    resizeWidth: THUMB_W,
    resizeHeight: THUMB_H,
    resizeQuality: 'medium',
  });
  const canvas = new OffscreenCanvas(THUMB_W, THUMB_H);
  const ctx = canvas.getContext('2d');
  ctx.drawImage(bitmap, 0, 0, THUMB_W, THUMB_H);
  bitmap.close();
  const blob = await canvas.convertToBlob({ type: 'image/jpeg', quality: 0.7 });
  return URL.createObjectURL(blob);
}

// A clip's own first decoded frame, sliced straight out of the `.srclip`
// container `showreel web-pack`/`clipimport.js` already produced — see
// `src/webclip.rs`'s doc comment for the exact layout, mirrored here the same
// way `srclip.js`'s `packSrclip` mirrors it for encoding. Already JPEG,
// already small (capped by the film's own clip `max_width`), so this is a
// byte slice, not a decode: cheaper than the still path above, not a
// duplicate of it.
export function clipThumbnail(srclipBytes) {
  if (srclipBytes.length < 32) return null;
  const dv = new DataView(srclipBytes.buffer, srclipBytes.byteOffset, srclipBytes.byteLength);
  const frameCount = dv.getUint32(24, true);
  if (frameCount === 0) return null;
  const frameLen = dv.getUint32(28, true);
  const jpeg = srclipBytes.slice(32, 32 + frameLen);
  return URL.createObjectURL(new Blob([jpeg], { type: 'image/jpeg' }));
}
