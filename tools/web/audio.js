// Browser playback of a film's sound.
//
// The wasm renderer only ever produces pixels (`src/wasm.rs`'s module doc) —
// nothing decodes or mixes audio there, and never will, since the real mixer
// is `AudioInput::filter`/`mix_filter` (`src/audio.rs`), built on ffmpeg
// filters that have no wasm/browser equivalent. So this module is a second,
// independent playback path — Web Audio, driven by the same film-time clock
// `main.js`'s `tick()`/`renderAt()` already own — not a port of the ffmpeg
// pipeline.
//
// Two kinds of sound, two different honesty levels:
//
// - A film-level `Audio` track (`film.audio`) ships as its own raw source
//   file (`showreel web-pack` now copies it, like a still) and is decoded
//   here with `AudioContext.decodeAudioData`. Its `at`/`from`/`duration`/
//   `gain`/`fade_in`/`fade_out` are read live off the film object on every
//   rebuild — exactly the fields `Audio::resolve` (src/audio.rs) turns into
//   an `AudioInput` natively — so editing a track in the browser is heard on
//   the next reload, same as everything else in the editor.
// - A clip's own soundtrack is baked into its source video, which the
//   browser never has — only the pre-decoded `.srclip` frames. `showreel
//   web-pack` now also shells to ffmpeg once, natively, to extract exactly
//   the window each clip layer draws for (gain/fades already applied, via
//   `AudioInput::export_filter`) into its own small file, listed in
//   `clip-audio.json`. This is a snapshot, not a live view: changing a
//   clip's `audio` settings or trim in the browser needs a repackage to be
//   heard, the same staleness a `.srclip`'s own decode already has. A
//   browser-dropped clip (no server package behind it at all) has no audio
//   preview yet for the same reason `.srclip` doesn't apply to it either.
//
// Both kinds become a "cue" — `{buffer, from, at, duration, fadeIn, fadeOut,
// gain}` — scheduled against one `AudioContext` clock whenever playback
// starts from a given film time, and stopped outright on pause or scrub, so
// sound only ever plays while the picture does. There is no attempt to make
// a mid-drag scrub scratch audio; landing on a frame is silent, matching
// scrubbing today, and the *moment* playback resumes it is back in sync.

function envelopeAt(cue, tIntoCue) {
  let v = cue.gain;
  if (cue.fadeIn > 0 && tIntoCue < cue.fadeIn) v *= Math.max(0, tIntoCue / cue.fadeIn);
  const fadeOutStart = cue.duration - cue.fadeOut;
  if (cue.fadeOut > 0 && tIntoCue > fadeOutStart) {
    v *= Math.max(0, (cue.duration - tIntoCue) / cue.fadeOut);
  }
  return Math.max(0, v);
}

// Schedules `param` (a GainNode's `.gain`) so a cue resumed partway through
// its own fade-in/out still starts at the correct level, not at full gain —
// `envelopeAt` sets the instantaneous value at `when`, then a ramp carries
// whichever fade edge is still ahead.
function scheduleGain(param, cue, elapsedIntoCue, when) {
  param.cancelScheduledValues(when);
  param.setValueAtTime(envelopeAt(cue, elapsedIntoCue), when);
  if (cue.fadeIn > 0 && elapsedIntoCue < cue.fadeIn) {
    param.linearRampToValueAtTime(cue.gain, when + (cue.fadeIn - elapsedIntoCue));
  }
  if (cue.fadeOut > 0) {
    const fadeOutStart = cue.duration - cue.fadeOut;
    const toFadeOutStart = fadeOutStart - elapsedIntoCue;
    if (toFadeOutStart > 0) param.setValueAtTime(cue.gain, when + toFadeOutStart);
    const toEnd = cue.duration - elapsedIntoCue;
    if (toEnd > 0) param.linearRampToValueAtTime(0, when + toEnd);
  }
}

export class AudioEngine {
  constructor() {
    this.ctx = null;
    this.cues = [];
    this.sources = [];
    this.bufferCache = new Map(); // url -> Promise<AudioBuffer>
  }

  ensureContext() {
    if (!this.ctx) this.ctx = new (window.AudioContext || window.webkitAudioContext)();
    if (this.ctx.state === 'suspended') this.ctx.resume();
    return this.ctx;
  }

  async decode(url) {
    if (!this.bufferCache.has(url)) {
      this.bufferCache.set(
        url,
        fetch(url)
          .then((r) => (r.ok ? r.arrayBuffer() : Promise.reject(new Error(`${url}: ${r.status}`))))
          .then((bytes) => this.ensureContext().decodeAudioData(bytes))
      );
    }
    return this.bufferCache.get(url);
  }

  // Rebuilds the cue list from the live film object plus the packaged
  // clip-audio manifest (`[]` if this page was never `web-pack`ed — a plain
  // `tools/web/` dev serve has no ffmpeg-extracted clip audio to offer).
  // Failures decoding one track (a missing file, an unsupported codec) are
  // swallowed per-cue so one bad track does not silence the rest.
  async rebuild(film, filmDuration, clipAudioManifest) {
    this.stop();
    const cues = [];
    for (const track of film.audio || []) {
      if (!track.asset) continue;
      const at = track.at || 0;
      const from = track.from || 0;
      const duration = track.duration != null ? track.duration : Math.max(0, filmDuration - at);
      if (duration <= 0) continue;
      try {
        const buffer = await this.decode(`assets/${track.asset}`);
        cues.push({
          buffer,
          from,
          at,
          duration,
          fadeIn: Math.min(track.fade_in || 0, duration),
          fadeOut: Math.min(track.fade_out || 0, duration),
          gain: track.gain != null ? track.gain : 1.0,
        });
      } catch {
        // Missing/unreadable — same as a missing still: silently absent,
        // the Assets panel's status dot is what tells the story.
      }
    }
    for (const c of clipAudioManifest || []) {
      if (c.duration <= 0) continue;
      try {
        const buffer = await this.decode(c.file);
        cues.push({ buffer, from: 0, at: c.at, duration: c.duration, fadeIn: 0, fadeOut: 0, gain: 1.0 });
      } catch {
        // A film opened without web-pack's clip-audio.json, or one entry
        // gone stale against an edited film — see this module's doc comment.
      }
    }
    this.cues = cues;
  }

  // Starts every cue active at or after film time `t`, positioned so it
  // sounds exactly as it would if playback had been running continuously
  // since before `t`. Call on every play/resume and on every playback-loop
  // restart; never for a paused scrub.
  start(t) {
    this.stop();
    if (!this.cues.length) return;
    const ctx = this.ensureContext();
    const now = ctx.currentTime;
    for (const cue of this.cues) {
      const cueEnd = cue.at + cue.duration;
      if (t >= cueEnd) continue;
      const elapsedIntoCue = Math.max(0, t - cue.at);
      const when = now + Math.max(0, cue.at - t);
      const offset = cue.from + elapsedIntoCue;
      const remaining = cue.duration - elapsedIntoCue;
      if (offset >= cue.buffer.duration || remaining <= 0) continue;
      const src = ctx.createBufferSource();
      src.buffer = cue.buffer;
      const gainNode = ctx.createGain();
      scheduleGain(gainNode.gain, cue, elapsedIntoCue, when);
      src.connect(gainNode).connect(ctx.destination);
      src.start(when, offset, Math.min(remaining, cue.buffer.duration - offset));
      this.sources.push(src);
    }
  }

  // Stops everything currently scheduled or playing. Call on pause, on
  // every scrub, and before `start()` re-schedules from a new position.
  stop() {
    for (const s of this.sources) {
      try {
        s.stop();
      } catch {
        // Already ended on its own — fine.
      }
    }
    this.sources = [];
  }
}
