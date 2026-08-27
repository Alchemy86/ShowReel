// Turns a video file the user drops into the browser into a `.srclip` asset
// `sr_add_clip` (src/wasm.rs) already knows how to unpack — the browser-side
// half of "add a clip" that `showreel web-pack` does natively with ffmpeg.
//
// WebCodecs' `VideoDecoder` is the right *decoder*, but it only decodes
// already-demuxed chunks — it has no mp4/webm box parser of its own, and
// browsers ship no demuxer API. Hand-rolling an mp4 demuxer to feed it was
// judged out of scope here (see the write-up); `HTMLVideoElement` already
// demuxes and decodes using the same underlying platform decoder WebCodecs
// would reach for, so this uses that instead, driven by seeking rather than
// playback: seek to each frame time, wait for `seeked`, draw the frame to a
// canvas, JPEG-encode it. It is not frame-exact the way ffmpeg's decode is —
// a browser's `currentTime` seek lands on "close enough for this timestamp",
// not a guaranteed exact frame — which is the honest trade for needing no
// vendored demuxer at all.

import { packSrclip } from './srclip.js';

/**
 * @param {File|Blob} file
 * @param {{trimStart:number, trimDuration:number, decodeFps:number, maxWidth:number, jpegQuality?:number}} opts
 * @param {(done:number, total:number) => void} [onProgress]
 * @returns {Promise<{container: Uint8Array, width:number, height:number, fps:number, frameCount:number}>}
 */
export async function importClip(file, opts, onProgress) {
  const { trimStart, trimDuration, decodeFps, maxWidth, jpegQuality = 85 } = opts;
  const url = URL.createObjectURL(file);
  const video = document.createElement('video');
  video.muted = true;
  video.playsInline = true;
  video.preload = 'auto';
  video.src = url;

  try {
    await waitFor(video, 'loadedmetadata');
    const srcW = video.videoWidth, srcH = video.videoHeight;
    if (!srcW || !srcH) throw new Error('the browser could not read this file as video');

    const scale = Math.min(1, maxWidth / srcW);
    const width = Math.max(2, Math.round(srcW * scale) & ~1);
    const height = Math.max(2, Math.round(srcH * scale) & ~1);

    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext('2d', { alpha: false });

    const duration = trimDuration > 0 ? trimDuration : Math.max(0, video.duration - trimStart);
    const frameCount = Math.max(1, Math.round(duration * decodeFps));
    const frames = [];
    for (let i = 0; i < frameCount; i++) {
      const t = trimStart + i / decodeFps;
      video.currentTime = Math.min(t, Math.max(0, video.duration - 0.001));
      await waitFor(video, 'seeked');
      ctx.drawImage(video, 0, 0, width, height);
      const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/jpeg', jpegQuality / 100));
      frames.push(new Uint8Array(await blob.arrayBuffer()));
      onProgress?.(i + 1, frameCount);
    }

    const container = packSrclip({ width, height, fps: decodeFps, frames });
    return { container, width, height, fps: decodeFps, frameCount };
  } finally {
    URL.revokeObjectURL(url);
  }
}

function waitFor(el, event) {
  return new Promise((resolve, reject) => {
    const onErr = () => {
      cleanup();
      reject(new Error(el.error?.message || `video ${event === 'seeked' ? 'seek' : 'load'} failed`));
    };
    const onOk = () => {
      cleanup();
      resolve();
    };
    function cleanup() {
      el.removeEventListener(event, onOk);
      el.removeEventListener('error', onErr);
    }
    el.addEventListener(event, onOk, { once: true });
    el.addEventListener('error', onErr, { once: true });
  });
}
