# Examples

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

### Running it

```bash
cargo run --release --example kanto_reel -- -o kanto.film.json
showreel sheet  kanto.film.json -A <assets> --every 1.5s -o sheet.png
showreel render kanto.film.json -A <assets> -o kanto-reel.mp4
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
