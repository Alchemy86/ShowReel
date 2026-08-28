# Assets for `showreel_demo.film.jsonc` — where each one comes from

The demo reel (`examples/showreel_demo.film.jsonc`) references every asset by a
**bare name**, exactly like `kanto.film.jsonc` does — the film knows no paths,
and `-A/--assets` says where the media actually lives. The rendered reel is
committed (`docs/showreel-reel.mp4` and its mobile cut), so you can watch it
without any of this. This file is here so you can **re-render** it: it lists
every asset and the exact command that produced it.

The media itself is not committed (a 3 MB atlas and a 55 MB source clip do not
belong in git, the same call `examples/README.md` makes). Two assets come from
`~/pokemon-run` — our own Pokémon Blue run output — and the rest are generated
from those by the one-liners below. Point `-A` at a directory holding the
results:

```bash
showreel render examples/showreel_demo.film.jsonc -A <assets-dir> -o showreel-reel.mp4
```

## The two source assets (from `~/pokemon-run`)

| bare name | what it is | origin |
|---|---|---|
| `kanto.png` | all 226 Pokémon Blue maps in one 7184 × 7936 picture (57.0 MP), region-tinted | `pixelgb atlas` — copied from `atlas/pokemon-blue_atlas_region-tint_1x_kanto.png` |
| `route1.mp4` | 600 agents playing Route 1 at once, a real swarm run (1280 × 720, colour heatmap) | `training/route1-showcase-600agents.mp4` |

```bash
cp  ~/pokemon-run/atlas/pokemon-blue_atlas_region-tint_1x_kanto.png  <assets-dir>/kanto.png
ln -s ~/pokemon-run/training/route1-showcase-600agents.mp4           <assets-dir>/route1.mp4
```

## The generated visual assets (ImageMagick, from `kanto.png` + our sprites)

The parallax planes and the grade card are cut from the same atlas plus our own
Game Boy player sprite (`~/pokemon-run/sprites/player-*.png`, 16 × 16, already
transparent). Splitting a flat image into depth planes is pre-production — the
crate's rule is that it only *composes* planes that already exist — so it
happens here, not in `src/`.

```bash
cd <assets-dir>
S=~/pokemon-run/sprites

# Parallax — three 2400×1350 planes: the map behind (opaque, darkened),
# a scatter of mid-distance trainers, and one large foreground player.
magick kanto.png -crop 2880x1620+2152+3158 +repage -filter Lanczos \
  -resize 2400x1350^ -gravity center -extent 2400x1350 -modulate 78,105,100 px-bg.png
magick -size 2400x1350 xc:none \
  \( "$S/player-up-w0.png"    -filter point -resize 150x150 \) -gravity northwest -geometry +520+300  -composite \
  \( "$S/player-left-w0.png"  -filter point -resize 132x132 \) -gravity northwest -geometry +1180+250 -composite \
  \( "$S/player-right-w0.png" -filter point -resize 144x144 \) -gravity northwest -geometry +1650+470 -composite \
  \( "$S/player-down-w1.png"  -filter point -resize 138x138 \) -gravity northwest -geometry +900+560  -composite \
  px-mid.png
magick -size 2400x1350 xc:none \
  \( "$S/player-down-f0.png" -filter point -resize 460x460 \) -gravity south -geometry +40+30 -composite \
  px-fg.png

# Grade card — a dense, deliberately vivid 1920×1080 crop, so the documentary
# grade's desaturate/contrast/vignette all read hard in the before/after.
# (grade_demo.rs does the same: it draws a *saturated* card on purpose.)
magick kanto.png -crop 3413x1920+3400+1000 +repage -filter Lanczos \
  -resize 1920x1080^ -gravity center -extent 1920x1080 \
  -modulate 100,178,100 -sigmoidal-contrast 3x50% grade-card.png
```

## The soundtrack (synthesised — nothing came from the internet)

No source music ships with ShowReel, and none of our Pokémon clips carry an
audio stream, so the bed and the shimmer are **synthesised from ffmpeg
oscillators** — a warm Cmaj-ish pad and a higher shimmer pad. They exist to
demonstrate the audio pipeline honestly (place, trim, fade, gain, and two
tracks mixing without rescaling each other), not to be a soundtrack.

```bash
cd <assets-dir>

# Bed — a warm ambient pad (C3 G3 C4 E4 G4), slow tremolo + a little space,
# normalised to a healthy level so it survives gain + mixing in the master.
ffmpeg -y \
  -f lavfi -i "sine=frequency=130.81:duration=64" \
  -f lavfi -i "sine=frequency=196.00:duration=64" \
  -f lavfi -i "sine=frequency=261.63:duration=64" \
  -f lavfi -i "sine=frequency=329.63:duration=64" \
  -f lavfi -i "sine=frequency=392.00:duration=64" \
  -filter_complex "[0:a][1:a][2:a][3:a][4:a]amix=inputs=5:normalize=0[m]; \
    [m]tremolo=f=0.18:d=0.35,aphaser=in_gain=0.5:out_gain=0.7:delay=3:decay=0.3:speed=0.3,\
lowpass=f=2100,aecho=0.8:0.7:60|140:0.25|0.18,loudnorm=I=-17:TP=-2:LRA=7,afade=t=in:st=0:d=0.02[out]" \
  -map "[out]" -ar 48000 -ac 2 -c:a pcm_s16le showreel-bed.wav

# Shimmer — a higher pad (C5 E5 G5), quieter, mixed in over the middle.
ffmpeg -y \
  -f lavfi -i "sine=frequency=523.25:duration=26" \
  -f lavfi -i "sine=frequency=659.25:duration=26" \
  -f lavfi -i "sine=frequency=783.99:duration=26" \
  -filter_complex "[0:a][1:a][2:a]amix=inputs=3:normalize=0[m]; \
    [m]tremolo=f=4.5:d=0.5,vibrato=f=5:d=0.3,lowpass=f=3200,\
aecho=0.8:0.6:90|200:0.3|0.2,loudnorm=I=-22:TP=-3:LRA=7[out]" \
  -map "[out]" -ar 48000 -ac 2 -c:a pcm_s16le showreel-shimmer.wav
```

## How the committed cuts were made

```bash
showreel render examples/showreel_demo.film.jsonc -A <assets-dir> \
    --crf 27 -o docs/showreel-reel.mp4
```

`--crf 27` (rather than the default 17) keeps the master well under the size
budget: the dithered Game Boy pixel-art is expensive to encode losslessly, so
17 produced ~105 MB. 27 lands it at ~28 MB with no visible loss on this
content, and the mobile cut (`docs/showreel-reel.mobile.mp4`, 720p/30fps, made
in the same command) at ~7 MB. Both carry the audio.
