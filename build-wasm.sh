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
# and a native release build does not.
cargo build --profile web --target wasm32-unknown-unknown \
  --no-default-features --features wasm --lib

mkdir -p tools/web
cp target/wasm32-unknown-unknown/web/showreel.wasm tools/web/showreel.wasm
echo "wasm -> tools/web/showreel.wasm ($(du -h tools/web/showreel.wasm | cut -f1))"
echo "package a film with:  cargo run --release --features wasm -- web-pack <film> -o dist"
echo "  (needs ffmpeg — it pre-decodes clips; see src/wasm.rs)"
echo "then serve dist/ over any static file host — it is the whole shareable page"
