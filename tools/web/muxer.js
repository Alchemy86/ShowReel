// A minimal WebM (Matroska/EBML) muxer for a single VP8 video track, plus an
// optional Opus audio track. Written by hand rather than vendored, for two
// reasons: it keeps the browser build dependency-free (no CDN, no bundler —
// the house style set by tools/web/index.html and src/wasm.rs), and WebM's
// container is small enough to get right in the open rather than trust.
//
// This file has no browser-only calls in it (no `document`, no WebCodecs) so
// it can be exercised from plain Node — see tools/web/test-muxer.mjs, which
// feeds it real VP8 frames from ffmpeg and checks the result with ffprobe.
// The browser-only half (capturing frames with WebCodecs, or falling back to
// MediaRecorder) lives in export.js.
//
// Reference: the Matroska/WebM element IDs below are the subset this needs —
// https://www.matroska.org/technical/elements.html, restricted to the WebM
// profile (https://www.webmproject.org/docs/container/).

const ID = {
  EBML: [0x1a, 0x45, 0xdf, 0xa3],
  EBMLVersion: [0x42, 0x86],
  EBMLReadVersion: [0x42, 0xf7],
  EBMLMaxIDLength: [0x42, 0xf2],
  EBMLMaxSizeLength: [0x42, 0xf3],
  DocType: [0x42, 0x82],
  DocTypeVersion: [0x42, 0x87],
  DocTypeReadVersion: [0x42, 0x85],
  Segment: [0x18, 0x53, 0x80, 0x67],
  Info: [0x15, 0x49, 0xa9, 0x66],
  TimecodeScale: [0x2a, 0xd7, 0xb1],
  Duration: [0x44, 0x89],
  MuxingApp: [0x4d, 0x80],
  WritingApp: [0x57, 0x41],
  Tracks: [0x16, 0x54, 0xae, 0x6b],
  TrackEntry: [0xae],
  TrackNumber: [0xd7],
  TrackUID: [0x73, 0xc5],
  TrackType: [0x83],
  CodecID: [0x86],
  Video: [0xe0],
  PixelWidth: [0xb0],
  PixelHeight: [0xba],
  Audio: [0xe1],
  SamplingFrequency: [0xb5],
  Channels: [0x9f],
  Cluster: [0x1f, 0x43, 0xb6, 0x75],
  Timecode: [0xe7],
  SimpleBlock: [0xa3],
};

function u8(bytes) {
  return bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
}

function concat(parts) {
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

// Smallest EBML vint that can hold `value` without colliding with the
// all-ones "unknown size" pattern that length carries.
function vintLength(value) {
  let n = 1;
  while (n < 8 && value > 2 ** (7 * n) - 2) n++;
  return n;
}

function writeVint(value, forceLen) {
  const n = forceLen || vintLength(value);
  const out = new Uint8Array(n);
  let v = BigInt(value);
  for (let i = n - 1; i >= 0; i--) {
    out[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  out[0] |= 1 << (8 - n);
  return out;
}

// Big-endian, minimal-width, no marker bit — an EBML "uint" element body.
function uintBytes(value) {
  if (value === 0) return new Uint8Array([0]);
  let v = BigInt(value);
  const bytes = [];
  while (v > 0n) {
    bytes.unshift(Number(v & 0xffn));
    v >>= 8n;
  }
  return new Uint8Array(bytes);
}

function floatBytes(value) {
  const buf = new ArrayBuffer(8);
  new DataView(buf).setFloat64(0, value, false);
  return new Uint8Array(buf);
}

function strBytes(s) {
  return new TextEncoder().encode(s);
}

function el(id, payload) {
  const p = u8(payload);
  return concat([u8(id), writeVint(p.length), p]);
}

function uintEl(id, value) {
  return el(id, uintBytes(value));
}

function ebmlHeader(docType) {
  return el(ID.EBML, concat([
    uintEl(ID.EBMLVersion, 1),
    uintEl(ID.EBMLReadVersion, 1),
    uintEl(ID.EBMLMaxIDLength, 4),
    uintEl(ID.EBMLMaxSizeLength, 8),
    el(ID.DocType, strBytes(docType)),
    uintEl(ID.DocTypeVersion, 4),
    uintEl(ID.DocTypeReadVersion, 2),
  ]));
}

// TimecodeScale fixed at 1ms (1_000_000ns) so every duration/timecode in
// this file is plain milliseconds — one fewer unit conversion to get wrong.
const TIMECODE_SCALE_NS = 1_000_000;
const AUDIO_TRACK_NUMBER = 2;
const VIDEO_TRACK_NUMBER = 1;
// A cluster's SimpleBlocks carry a *signed 16-bit* timecode relative to the
// cluster's own base, so a cluster must not span more than this before the
// next keyframe starts a fresh one.
const MAX_CLUSTER_SPAN_MS = 30000;

export class WebmMuxer {
  /**
   * @param {{width:number, height:number, codecId?:string,
   *   audio?: {codecId:string, sampleRate:number, channels:number, codecPrivate?:Uint8Array}}} opts
   */
  constructor(opts) {
    this.width = opts.width;
    this.height = opts.height;
    this.codecId = opts.codecId || 'V_VP8';
    this.audio = opts.audio || null;
    this.clusters = []; // each: {baseMs, blocks: Uint8Array[]}
    this.cur = null;
    this.durationMs = 0;
  }

  _startCluster(baseMs) {
    this.cur = { baseMs, blocks: [] };
    this.clusters.push(this.cur);
  }

  _pushBlock(trackNumber, timestampMs, keyframe, bytes) {
    if (
      !this.cur ||
      timestampMs - this.cur.baseMs > MAX_CLUSTER_SPAN_MS ||
      timestampMs < this.cur.baseMs
    ) {
      this._startCluster(timestampMs);
    }
    const rel = Math.round(timestampMs - this.cur.baseMs);
    const flags = keyframe ? 0x80 : 0x00;
    const block = concat([
      writeVint(trackNumber),
      new Uint8Array([(rel >> 8) & 0xff, rel & 0xff]),
      new Uint8Array([flags]),
      bytes,
    ]);
    this.cur.blocks.push(el(ID.SimpleBlock, block));
    this.durationMs = Math.max(this.durationMs, timestampMs);
  }

  /** A keyframe starts a fresh cluster; every other frame joins the current one. */
  addVideoFrame(bytes, timestampMs, keyframe) {
    if (keyframe || !this.cur) this._startCluster(timestampMs);
    this._pushBlock(VIDEO_TRACK_NUMBER, timestampMs, keyframe, u8(bytes));
  }

  addAudioFrame(bytes, timestampMs) {
    if (!this.audio) throw new Error('no audio track configured');
    this._pushBlock(AUDIO_TRACK_NUMBER, timestampMs, true, u8(bytes));
  }

  _tracksElement() {
    const video = el(ID.TrackEntry, concat([
      uintEl(ID.TrackNumber, VIDEO_TRACK_NUMBER),
      uintEl(ID.TrackUID, VIDEO_TRACK_NUMBER),
      uintEl(ID.TrackType, 1),
      el(ID.CodecID, strBytes(this.codecId)),
      el(ID.Video, concat([
        uintEl(ID.PixelWidth, this.width),
        uintEl(ID.PixelHeight, this.height),
      ])),
    ]));
    const entries = [video];
    if (this.audio) {
      entries.push(el(ID.TrackEntry, concat([
        uintEl(ID.TrackNumber, AUDIO_TRACK_NUMBER),
        uintEl(ID.TrackUID, AUDIO_TRACK_NUMBER),
        uintEl(ID.TrackType, 2),
        el(ID.CodecID, strBytes(this.audio.codecId)),
        el(ID.Audio, concat([
          el(ID.SamplingFrequency, floatBytes(this.audio.sampleRate)),
          uintEl(ID.Channels, this.audio.channels),
        ])),
      ])));
    }
    return el(ID.Tracks, concat(entries));
  }

  /** Assemble the whole file. Call once, after every frame is added. */
  finalize() {
    const info = el(ID.Info, concat([
      uintEl(ID.TimecodeScale, TIMECODE_SCALE_NS),
      el(ID.Duration, floatBytes(this.durationMs)),
      el(ID.MuxingApp, strBytes('showreel-web')),
      el(ID.WritingApp, strBytes('showreel-web')),
    ]));
    const tracks = this._tracksElement();
    const clusters = this.clusters
      .filter((c) => c.blocks.length > 0)
      .map((c) => el(ID.Cluster, concat([uintEl(ID.Timecode, c.baseMs), ...c.blocks])));
    const segment = el(ID.Segment, concat([info, tracks, ...clusters]));
    return concat([ebmlHeader('webm'), segment]);
  }
}
