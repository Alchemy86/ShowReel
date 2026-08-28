# Examples

## The gallery — the shortest films here

`gallery.rs` is the source for the README's visual gallery: eight two-to-three
second films, each showing exactly one capability (camera push, parallax,
colour grade, animated chart, counter, kinetic captions, transitions,
callouts). It draws its own procedural stills, so it needs nothing on disk:

```bash
cargo run --release --example gallery     # writes examples/gallery/*.film.jsonc + assets/
examples/gallery/render.sh                # renders docs/gallery/*.gif via `showreel gif`
```

The `.film.jsonc` files are committed and runnable on their own —
`showreel gif examples/gallery/camera.film.jsonc -o camera.gif` — and they are
the documentation for *how* each effect is authored, cross-linked from the four
studies in `docs/`. Unlike `kanto.film.jsonc` they carry no `--check` drift
guard; the `.rs` is simply the canonical source, and the GIFs are meant to be
watched back by eye after any change.

## Making a film without writing Rust

**`kanto.film.jsonc` in this directory is a complete, ready-to-render film**,
and it is the honest answer to "what *is* a film in ShowReel?" — a JSON
document. The Rust builders in `kanto_reel.rs` are sugar that produces exactly
this and nothing more. You do not need them:

```bash
showreel render examples/kanto.film.jsonc -A <assets> -o kanto-reel.mp4
```

Open it, read it top to bottom, change a duration or a caption, render it again.
No compiler, no Rust, no build step. `showreel check <file>` reports the
mistakes a type cannot catch before you spend a render on them, and
`showreel sheet <file>` puts the whole film on one page in a couple of seconds.

### On reading it

It is pretty-printed, and every scene carries a `name`, so the timeline is
followable by scrolling. It is long (about 1300 lines) because it is fully
explicit: every theme value, every placement. That is the trade for being
readable by a tool as well as a person.

**It has comments, which is why it's `.jsonc` and not `.json`.** Why a scene
holds for four seconds, why an inset sits where it does, where a clip's
timestamp came from — that reasoning used to live only in `kanto_reel.rs`,
because JSON has no comment syntax. It now lives here too, next to the values
it explains. `Film::from_json` accepts a narrow JSONC subset — `//` and
`/* */` comments, plus a trailing comma on the last element of an array or
object — via the [`jsonc-parser`](https://crates.io/crates/jsonc-parser)
crate; see the "Comments in film files" section atop `src/timeline.rs` for
why that crate and not one of the alternatives. A plain `.json` film with no
comments still loads exactly as before.

The map arithmetic and asset bookkeeping still stay in `kanto_reel.rs`. What
moved into the JSON is the storytelling — why the film is shaped the way it
is — not how a map rectangle's centre was computed.

### Assets are named, never pathed

Every asset in the file is a bare name — `kanto.png`, `pokemon-blue-title.wav` —
and never a path. `-A/--assets` is what says where the media actually lives, and
the film's own directory is always searched first. That is what keeps the
description portable: nothing in it is specific to the machine that wrote it.
An integration test asserts it, so an absolute path cannot creep back in.

The media itself is **not** in this repository. See the asset table below for
what each reference is and where it came from.

### Which one is canonical

**`kanto_reel.rs` is canonical. `kanto.film.jsonc` is a checked-in artifact
generated from it.** The Rust file holds the map arithmetic; the JSON is what
that arithmetic produces, annotated by hand with the reasoning behind the
film itself. After changing the Rust, regenerate:

```bash
cargo run --release --example kanto_reel -- -o examples/kanto.film.jsonc
```

and this fails, loudly, if the two have drifted apart:

```bash
cargo run --release --example kanto_reel -- --check -o examples/kanto.film.jsonc
```

**Regenerating overwrites the file and discards its comments.** `to_json`
only ever emits plain JSON — there is no way to reconstruct hand-written
prose from a Rust builder. Comparing raw text would then fail `--check` on
every comment in the committed file, which defeats the point of having them.
So `--check` compares *parsed* films instead: it loads both the freshly
generated JSON and the committed file and checks the resulting `Film` values
are equal. A comment can go stale without tripping the guard — chosen over
the other two options on the table: teaching the generator to emit comments
(machinery for prose that belongs to a human editor, not a map-rectangle
computation), or making the JSON canonical instead of the Rust (hand-
maintaining the atlas arithmetic in JSON, the opposite of `AGENTS.md`'s
"nothing in the crate may know what its films are about"). If you regenerate
after changing `kanto_reel.rs`, move any comments that are still true back
into the new file by hand; the CLI warns if the file it's about to overwrite
looks like it has comments in it.

If you only want to *edit a film*, edit the JSONC — copy it somewhere and
render it. The `--check` guard exists to keep the committed copy honest, not
to stop you using the format it is demonstrating.

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
# The JSONC is committed — this is all you need.
showreel sheet  examples/kanto.film.jsonc -A <assets> --every 1.5s -o sheet.png
showreel render examples/kanto.film.jsonc -A <assets> -o kanto-reel.mp4

# Only if you changed kanto_reel.rs:
cargo run --release --example kanto_reel -- -o examples/kanto.film.jsonc
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
