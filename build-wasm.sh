#!/usr/bin/env bash
# Build the OPTIONAL browser target: the renderer, with no ffmpeg and no
# filesystem, as a wasm blob a static page can load. The native `showreel`
# binary is the product and needs none of this; see AGENTS.md.
#
#   ./build-wasm.sh          -> tools/web/showreel.wasm
set -euo pipefail
cd "$(dirname "$0")"

if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  echo "need the wasm target: rustup target add wasm32-unknown-unknown" >&2
  exit 1
fi

# --no-default-features drops the CLI (clap) and threaded batch rendering
# (rayon) — both need a real OS. The `web` profile (Cargo.toml) trades
# compile speed for download size, which is the trade a browser build wants
# and a native release build does not — though not runtime speed: see below.
#
# `+simd128` turns on tiny-skia's own `target_feature = "simd128"` codepaths
# (src/wide/*.rs in the tiny-skia source — its `simd` cargo feature is
# already on by default, this is the other half). Measured on this film's
# raw sr_render_at throughput (bypassing rAF — see AGENTS.md), tight-loop
# average at 1920x1080: 1.76fps with neither flag, 3.58fps with only
# `+simd128` (opt-level "z" still throttles it), 7.62fps with only
# opt-level 3 (no simd128), 20.6fps with both — the two compound rather
# than add, because "z" also skips the inlining simd128's own codegen
# needs to pay off. That is why `[profile.web]` below is opt-level 3, not
# the smaller "z": a renderer's whole job is this hot loop, and "z" was
# costing it more than 10x for a wasm binary that only grew ~25% (1.8MB ->
# 2.25MB, stripped). See the "browser playback" section of AGENTS.md.
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+simd128" \
  cargo build --profile web --target wasm32-unknown-unknown \
  --no-default-features --features wasm --lib

mkdir -p tools/web
cp target/wasm32-unknown-unknown/web/showreel.wasm tools/web/showreel.wasm

# Optional, mechanical, and free when available: a couple more optimization
# passes over what rustc already emitted. Not required — skipped silently
# if the tool isn't on PATH, since neither this repo nor its CI installs it.
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -O3 --enable-simd tools/web/showreel.wasm -o tools/web/showreel.wasm
  echo "wasm-opt -O3 applied"
else
  echo "wasm-opt not found on PATH — skipping the extra binaryen pass (see AGENTS.md)"
fi

echo "wasm -> tools/web/showreel.wasm ($(du -h tools/web/showreel.wasm | cut -f1))"
echo "package a film with:  cargo run --release --features wasm -- web-pack <film> -o dist"
echo "  (needs ffmpeg — it pre-decodes clips; see src/wasm.rs)"
echo "then serve dist/ over any static file host — it is the whole shareable page"
