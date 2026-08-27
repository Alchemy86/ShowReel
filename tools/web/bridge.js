// The wasm/JS boundary itself: allocate, write, call, read back. Shared by
// main.js (boot, transport) and editor.js (every edit reloads the film
// through this same boundary) so there is exactly one copy of it.
export class WasmBridge {
  constructor(wasmInstance) {
    this.wasm = wasmInstance;
    this.mem = wasmInstance.memory;
  }

  writeBytes(bytes) {
    const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    const ptr = this.wasm.sr_alloc(arr.length);
    new Uint8Array(this.mem.buffer, ptr, arr.length).set(arr);
    return [ptr, arr.length];
  }

  writeText(s) {
    return this.writeBytes(new TextEncoder().encode(s));
  }

  readStr(ptrFn, lenFn) {
    const len = lenFn.call(this.wasm);
    return len ? new TextDecoder().decode(new Uint8Array(this.mem.buffer, ptrFn.call(this.wasm), len)) : '';
  }

  lastError() {
    return this.readStr(this.wasm.sr_error_ptr, this.wasm.sr_error_len) || '(no detail)';
  }
}

export async function fetchBytes(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`fetch ${url}: ${res.status}`);
  return new Uint8Array(await res.arrayBuffer());
}

// Mirrors `clip_srclip_name` in src/bin/showreel.rs exactly — see that
// function's doc comment. A collision here (same asset, different trim,
// resolving to the same URL) is a silent wrong-footage bug, not a 404, so the
// two must stay in lock step.
export function clipAssetPath(c, fps) {
  const suffix = c.trim ? `${c.trim[0].toFixed(3)}-${c.trim[1].toFixed(3)}` : 'full';
  return `assets/${c.asset}@${c.max_width}x${fps.toFixed(3)}_${suffix}.srclip`;
}

// The same cache key AssetStore::clip uses, as a plain string — how
// `providedClips` (assets added client-side, not fetched) is keyed.
export function clipCacheKey(asset, maxWidth, fps, trim) {
  return `${asset}@${maxWidth}x${fps.toFixed(3)}_${trim ? `${trim[0].toFixed(3)}-${trim[1].toFixed(3)}` : 'full'}`;
}
