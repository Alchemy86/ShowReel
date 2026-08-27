# Examples

## Making a film without writing Rust

**`kanto.film.json` in this directory is a complete, ready-to-render film**, and
it is the honest answer to "what *is* a film in ShowReel?" — a JSON document.
The Rust builders in `kanto_reel.rs` are sugar that produces exactly this and
nothing more. You do not need them:

```bash
showreel render examples/kanto.film.json -A <assets> -o kanto-reel.mp4
```

Open it, read it top to bottom, change a duration or a caption, render it again.
No compiler, no Rust, no build step. `showreel check <file>` reports the
mistakes a type cannot catch before you spend a render on them, and
`showreel sheet <file>` puts the whole film on one page in a couple of seconds.

### On reading it

It is pretty-printed, and every scene carries a `name`, so the timeline is
followable by scrolling. Two honest caveats:

- **JSON has no comment syntax**, so the *reasoning* behind the film — why an
  inset sits where it does, where a clip timestamp came from — cannot live in
  the file. It lives in `kanto_reel.rs`, which is the better document for it.
- It is long (about 1300 lines) because it is fully explicit: every theme
  value, every placement. That is the trade for being readable by a tool as
  well as a person.

### Assets are named, never pathed

Every asset in the file is a bare name — `kanto.png`, `pokemon-blue-title.wav` —
and never a path. `-A/--assets` is what says where the media actually lives, and
the film's own directory is always searched first. That is what keeps the
description portable: nothing in it is specific to the machine that wrote it.
An integration test asserts it, so an absolute path cannot creep back in.

The media itself is **not** in this repository. See the asset table below for
what each reference is and where it came from.

### Which one is canonical

**`kanto_reel.rs` is canonical. `kanto.film.json` is a checked-in artifact
generated from it.** The Rust file holds the map arithmetic and the reasoning;
the JSON is what that arithmetic produces. After changing the Rust, regenerate:

```bash
cargo run --release --example kanto_reel -- -o examples/kanto.film.json
```

and this fails, loudly, if the two have drifted apart:

```bash
cargo run --release --example kanto_reel -- --check -o examples/kanto.film.json
```

If you only want to *edit a film*, edit the JSON — copy it somewhere and render
it. The `--check` guard exists to keep the committed copy honest, not to stop
you using the format it is demonstrating.

## `kanto_reel.rs`

The shot ShowReel was commissioned to prove: **open on Pokémon Blue's title
screen → pull back to reveal the whole world map → real run footage bursts out
from where it happened → pull one of them up and name it.**

Everything subject-specific — which map sits where in the atlas, which second of
which film shows which milestone — lives in that one file, as input. Nothing in
`src/` knows what a Game Boy is.

### Assets it needs

| reference | what it is | where it came from |
|---|---|---|
| `kanto.png` | all 226 Blue maps in one 6832 × 7024 picture (48.0 MP) | `pixelgb atlas --rom <blue.gb> --scales 1` |
| `title-screen.mp4` | the cartridge's own title screen | `terminalgb`'s `video_out` example, frames 1440–1740 |
| `pixel-chain-run.mp4` | one policy playing a real cold boot through the opening chain | `agentgb/docs/media/` |
| `pixel-chain-grid.mp4` | 26 of those runs at once | `agentgb/docs/media/` |
| `pokemon-blue-title.wav` | the cartridge's own title theme | recorded off the emulated sound chip — see below |

### Where the soundtrack comes from

The music is not a file from anywhere — it is the cartridge played by our own
emulator and recorded off its APU. `terminalgb`'s music mode boots a ROM
headlessly and keeps every sample the sound chip produces:

```bash
cargo run --release --example music_probe --features music,image -- \
    <dir-containing-the-rom> --wav <destination> --seconds 40
```

Then point ShowReel's `-A` at a directory holding the result under the name
`pokemon-blue-title.wav`. The ROM and the capture are the user's own; neither is
in this repository, and neither should be.

### Running it

```bash
# The JSON is committed — this is all you need.
showreel sheet  examples/kanto.film.json -A <assets> --every 1.5s -o sheet.png
showreel render examples/kanto.film.json -A <assets> -o kanto-reel.mp4

# Only if you changed kanto_reel.rs:
cargo run --release --example kanto_reel -- -o examples/kanto.film.json
```

### What the footage placements claim

The run film burns its own milestone caption into every frame. Each timestamp
used here was checked by extracting that frame and reading the caption, so
"this moment happened at this map" has evidence behind it:

| source seconds | caption | placed at |
|---|---|---|
| 8 | `reached: leave-the-bedroom` | Red's house, upstairs |
| 176 | `reached: take-a-starter` — the rival battle | Oak's Lab |
| 277 | `reached: out-of-the-lab` | Pallet Town |
| 330 | `reached: north-out-of-pallet` | Route 1 |

The opening chain all happens within a few hundred atlas pixels of Pallet Town,
which is a handful of screen pixels once all 226 maps are in frame. So the
insets are hand-placed around the edge and a callout draws the line back to the
real coordinate. **The line is the claim, not the inset's position.**
