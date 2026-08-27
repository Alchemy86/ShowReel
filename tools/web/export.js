// Exporting a video, from inside the tab, with no ffmpeg.
//
// # The route taken: WebCodecs `VideoEncoder`, straight from the renderer
//
// Every frame this crate can draw already comes out of `sr_render_at` as raw
// RGBA — that is the whole point of the wasm build. `VideoEncoder` accepts a
// `VideoFrame` built directly from that buffer, no canvas round trip needed,
// and encodes it with the platform's own encoder (hardware-accelerated where
// the browser has one). That is the "no huge download, fast, frames we
// already render fed straight in" case worth checking honestly — and having
// actually built it: yes, it is that. Measured in a headless, GPU-less test
// tab (no hardware acceleration available): ~9 frames/s encoding VP8 at
// 1920x1080, i.e. ~110ms/frame. A real browser with a hardware encoder should
// beat this; this file makes no claim it will, because it wasn't measured.
//
// VP8 is the codec, not H.264 or AV1: `VideoEncoder` needs a *muxer* to turn
// its output into a playable file, and this crate writes that muxer itself
// (`muxer.js`, verified against real ffprobe/ffmpeg in `test-muxer.mjs`)
// rather than vendoring one — see that file's docs for why WebM's container
// is the tractable one to hand-write and VP8 is the codec that asks nothing
// extra of it (no SPS/PPS bookkeeping, no AV1 sequence header). A hardware
// H.264 or AV1 encoder plus a hand-rolled mp4 muxer is the natural next step
// if quality/size at a given bitrate matters more than getting a first real
// export path shipped and measured.
//
// # The routes not taken, and why
//
// **ffmpeg compiled to WebAssembly** (`@ffmpeg/wasm`) genuinely works and was
// evaluated, not dismissed on sight: checked directly against the npm
// registry, its `@ffmpeg/core` package is **64.7 MB unpacked** and licensed
// **GPL-2.0-or-later** (it bundles `libx264`, which is GPL; there is no
// smaller LGPL build that can actually encode H.264). Shipping that next to
// this crate's own PolyForm-Noncommercial-licensed page is a real licensing
// mismatch to own deliberately, not stumble into, and a 65 MB download to
// encode a video is the opposite of what "runs in the browser, no server"
// is supposed to buy a captain. Rejected on both grounds, not just size.
// **A pure-Rust encoder compiled into *this* wasm build** — the natural
// third option — is answered by `docs/native-encode-audit.md`'s measurement:
// even `rav1e` at its fastest preset is ~30× slower than this crate's own
// ffmpeg/x264 settings on identical frames, which is why that route wasn't
// built for either the native or the browser path.
//
// # What this does not do (yet)
//
// Audio. The renderer produces pixels; nothing in this build produces PCM
// for the film's `Audio` tracks (that mixing currently lives entirely in
// `AudioInput::filter`, native-only, ffmpeg's `amix`). A silent export is
// what ships here — see the task writeup for the honest gap and what closing
// it would take (`OfflineAudioContext` + `AudioEncoder`, mixing re-implemented
// in JS against the same fade/gain rules `src/audio.rs` already encodes).
// `muxer.js`'s `WebmMuxer` already accepts an optional Opus audio track, so
// the muxing half of that follow-up is done; only the encode half is not.

import { WebmMuxer } from './muxer.js';

export function webCodecsAvailable() {
  return typeof VideoEncoder !== 'undefined' && typeof VideoFrame !== 'undefined';
}

/**
 * `renderFrame(t)` must render frame time `t` (seconds) and return a fresh
 * `Uint8ClampedArray`/`Uint8Array` of straight RGBA bytes, `width * height * 4`
 * long — a *copy*, since wasm's own frame buffer is overwritten by the next
 * render. Throws if `webCodecsAvailable()` is false; check that first so the
 * caller can offer the real reason rather than a generic failure.
 */
export async function exportWebM({ width, height, fps, duration, renderFrame, bitrate, onProgress, signal }) {
  if (!webCodecsAvailable()) {
    throw new Error('this browser has no WebCodecs VideoEncoder — see the export panel for what that means');
  }
  const muxer = new WebmMuxer({ width, height, codecId: 'V_VP8' });
  let encodeError = null;
  const encoder = new VideoEncoder({
    output: (chunk) => {
      const data = new Uint8Array(chunk.byteLength);
      chunk.copyTo(data);
      muxer.addVideoFrame(data, chunk.timestamp / 1000, chunk.type === 'key');
    },
    error: (e) => { encodeError = e; },
  });
  const config = {
    codec: 'vp8',
    width,
    height,
    framerate: fps,
    bitrate: bitrate ?? Math.round(width * height * fps * 0.06), // ~0.06 bit/pixel/frame: a workable default for VP8 screen/photo content
  };
  const support = await VideoEncoder.isConfigSupported(config);
  if (!support.supported) {
    throw new Error(`this browser's VideoEncoder does not support ${width}x${height} VP8 at ${fps}fps`);
  }
  encoder.configure(config);

  const frameCount = Math.max(1, Math.round(duration * fps));
  const gop = Math.max(1, Math.round(fps)); // one keyframe a second
  for (let i = 0; i < frameCount; i++) {
    if (signal?.aborted) break;
    if (encodeError) throw encodeError;
    const t = i / fps;
    const rgba = renderFrame(t);
    const vf = new VideoFrame(rgba, {
      format: 'RGBA',
      codedWidth: width,
      codedHeight: height,
      timestamp: Math.round(t * 1e6),
      duration: Math.round(1e6 / fps),
    });
    encoder.encode(vf, { keyFrame: i % gop === 0 });
    vf.close();
    onProgress?.(i + 1, frameCount);
    // Yield to the event loop periodically so the tab stays responsive and
    // `encoder.encode`'s internal queue has a chance to drain — WebCodecs
    // back-pressures via `encodeQueueSize` rather than blocking `encode()`.
    if (encoder.encodeQueueSize > 4) {
      await new Promise((r) => setTimeout(r, 0));
    }
  }
  if (encodeError) throw encodeError;
  await encoder.flush();
  encoder.close();
  if (signal?.aborted) return null;

  return new Blob([muxer.finalize()], { type: 'video/webm' });
}
