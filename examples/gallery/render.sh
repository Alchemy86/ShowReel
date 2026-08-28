#!/usr/bin/env bash
# Render every gallery film to a README-sized GIF in docs/gallery/.
#
# Regenerate the films/stills first if you've edited examples/gallery.rs:
#   cargo run --release --example gallery
#
# Then:  examples/gallery/render.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

BIN=./target/release/showreel
[ -x "$BIN" ] || { echo "build first: cargo build --release --bin showreel" >&2; exit 1; }

SRC=examples/gallery
OUT=docs/gallery
mkdir -p "$OUT"

# Every film is short and already single-idea, so each GIF is the whole film.
# Defaults (480px wide, 15fps, Bayer dither, full palette) suit most of them;
# the palette focus is left at `full` because several show a whole-frame change
# (the grade, the transitions) that `diff` would under-serve.
#
# The two full-frame-motion shots (camera push, parallax) are the only heavy
# files, because every pixel moves every frame so nothing can be left un-repainted.
# Both were checked by eye at a reduced palette and hold up (Bayer dithering
# hides the smaller palette on their gradients): 128 colours on each, and 12fps
# on the slow parallax push, keep them honest for a README without visible loss.
extra_args() {
  case "$1" in
    camera)   echo "--colors 128" ;;
    parallax) echo "--colors 128 --fps 12" ;;
    *)        echo "" ;;
  esac
}

for film in "$SRC"/*.film.jsonc; do
  name=$(basename "$film" .film.jsonc)
  # shellcheck disable=SC2046
  "$BIN" gif "$film" $(extra_args "$name") -o "$OUT/$name.gif"
done

echo
echo "Wrote GIFs to $OUT/:"
ls -la "$OUT"/*.gif
