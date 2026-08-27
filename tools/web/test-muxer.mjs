#!/usr/bin/env node
// Verifies muxer.js against a real VP8 bitstream: ffmpeg encodes a synthetic
// clip to a raw IVF, this script re-muxes those exact frames into WebM with
// WebmMuxer, and ffprobe/ffmpeg (the same binaries the rest of the project
// already depends on) are asked to actually read the result back. Nothing
// here is a browser API, so it runs under plain Node — no page needed to
// catch a container bug.
//
//   node tools/web/test-muxer.mjs

import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { WebmMuxer } from './muxer.js';

function must(cond, msg) {
  if (!cond) {
    console.error(`FAIL: ${msg}`);
    process.exit(1);
  }
  console.log(`ok: ${msg}`);
}

const dir = mkdtempSync(join(tmpdir(), 'showreel-muxer-test-'));
const ivfPath = join(dir, 'src.ivf');
const webmPath = join(dir, 'out.webm');

const WIDTH = 320, HEIGHT = 240, FPS = 10, DURATION_S = 3;

console.log(`encoding a synthetic ${WIDTH}x${HEIGHT}@${FPS}fps VP8 test clip with ffmpeg...`);
execFileSync('ffmpeg', [
  '-y', '-hide_banner', '-loglevel', 'error',
  '-f', 'lavfi', '-i', `testsrc=size=${WIDTH}x${HEIGHT}:rate=${FPS}:duration=${DURATION_S}`,
  '-c:v', 'libvpx', '-qmin', '0', '-qmax', '50', '-b:v', '500k',
  '-g', String(FPS), // one keyframe a second, exercising multi-cluster output
  '-f', 'ivf', ivfPath,
]);

// ---- parse the IVF frames straight out ------------------------------------
function parseIvf(bytes) {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  must(String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3]) === 'DKIF', 'ffmpeg wrote a real IVF header');
  const headerLen = dv.getUint16(6, true);
  const frames = [];
  let o = headerLen;
  while (o + 12 <= bytes.length) {
    const size = dv.getUint32(o, true);
    o += 12; // 4-byte size + 8-byte timestamp, timestamp unused: frame index paces us instead
    frames.push(bytes.subarray(o, o + size));
    o += size;
  }
  return frames;
}

const frames = parseIvf(readFileSync(ivfPath));
must(frames.length > 0, `parsed ${frames.length} VP8 frames from the IVF`);

// VP8 frame tag, byte 0 bit 0: 0 = key frame, 1 = inter frame.
const isKeyframe = (frame) => (frame[0] & 0x01) === 0;
must(isKeyframe(frames[0]), 'the first frame is a keyframe (as any stream must open with)');

// ---- re-mux with the muxer under test --------------------------------------
const muxer = new WebmMuxer({ width: WIDTH, height: HEIGHT, codecId: 'V_VP8' });
frames.forEach((frame, i) => {
  const tsMs = (i * 1000) / FPS;
  muxer.addVideoFrame(frame, tsMs, isKeyframe(frame));
});
const out = muxer.finalize();
writeFileSync(webmPath, out);
console.log(`wrote ${out.length} bytes to ${webmPath}`);

const clusterCount = muxer.clusters.filter((c) => c.blocks.length > 0).length;
must(clusterCount >= 2, `emitted ${clusterCount} clusters (one per keyframe, as intended)`);

// ---- ask ffprobe/ffmpeg — not this script — whether the container is real -
const probe = JSON.parse(execFileSync('ffprobe', [
  '-v', 'error', '-print_format', 'json',
  '-show_entries', 'stream=codec_name,width,height,r_frame_rate:format=duration',
  webmPath,
]).toString());

const stream = probe.streams[0];
must(stream.codec_name === 'vp8', `ffprobe reports codec_name=vp8 (got ${stream.codec_name})`);
must(stream.width === WIDTH && stream.height === HEIGHT, `ffprobe reports ${stream.width}x${stream.height}`);
const durationS = parseFloat(probe.format.duration);
must(Math.abs(durationS - DURATION_S) < 0.5, `ffprobe reports duration ~${durationS.toFixed(2)}s (expected ~${DURATION_S}s)`);

// Full decode, not just header parse: -f null discards output but still
// forces every frame through the VP8 decoder.
execFileSync('ffmpeg', ['-v', 'error', '-i', webmPath, '-f', 'null', '-']);
console.log('ok: ffmpeg decoded every frame without error');

rmSync(dir, { recursive: true, force: true });
console.log('\nmuxer.js: all checks passed');
