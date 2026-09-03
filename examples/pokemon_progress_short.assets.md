# Assets for `pokemon_progress_short.film.jsonc`

None of this film's assets are committed — they are all external, undistributed
research material from sibling projects on the machine this was built on, not
ShowReel fixtures. This file is the exact record of where every asset came from and
how the two derived-asset prep steps were done, so the film can be rebuilt (or
rebuilt with a fresher swarm run / fresher stats) elsewhere.

## 1. PixelGB cartridge art (no prep — used directly)

Root: `projects/pixelgb/images/pokemon-blue/` (10,712 extracted, scaled PNGs; see that
project's `docs/graphics-inventory.md`). Referenced in the film by the bare relative
path under that root, e.g. `"asset": "maps/four-shade/8x/012-route-1.png"`.

Used directly, no modification:

- `title/four-shade/8x/pokemon-logo.png` — hook logo
- `maps/four-shade/8x/038-reds-house-2f.png` — bedroom background
- `maps/four-shade/8x/000-pallet-town.png` — Oak/starter/rival background
- `sprites/four-shade/8x/pokemon-front-{001-bulbasaur,004-charmander,007-squirtle}.png`
  — the three starters, shown as a choice, not a specific pick (the run's own pick was
  never independently confirmed)
- `maps/four-shade/8x/012-route-1.png` — Route 1 background
- `maps/four-shade/8x/042-viridian-mart.png` — mart backgrounds (route1-mart-dex AND
  shop-screen scenes)
- `sprites/four-shade/16x/overworld-pokedex.png` — Pokédex icon pop-in
- `maps/four-shade/8x/051-viridian-forest.png` — forest background

Used after a one-time crop (see §3 below), not directly:

- `portraits/four-shade/8x/trainer-26-prof-oak.png`, `trainer-25-rival.png`

Used after a one-time downscale (see §3 below), not directly:

- `atlas/region-tint/2x/kanto.png` (15008×14688, 9.1 MB) — goal-card background

## 2. Real agent-run footage

**Per-frame screenshots** (native 160×144 Game Boy resolution), from
`pokemon-run-txtscr/film/mgba-screen-triggers.mp4/captures/a01/` — agent `a01`, seed
4200127, one of 3 finishers (of 16) in the grid run this film's "today" stat also uses,
and the only one with individually addressable per-frame captures. Frame numbers below
were **picked by hand after checking mean brightness** (`PIL`, `convert('L')` +
average pixel value) to reject transition/fade frames — several nearby "obvious"
frame numbers (e.g. `f00022`, the literal frame the milestone timestamp names for
"leaves the house") turned out to be a pure-black door-transition frame, not usable
footage; the neighbour frame one tick later was checked and used instead. Every frame
below was also **opened and eyeballed**, not just brightness-checked, before use.

| Derived name | Source frame | Note |
|---|---|---|
| `frames/bedroom.png` | `f00003.png` | (not `f00009` — that one is the very start of the room, still scrolling in) |
| `frames/leave-house.png` | `f00023.png` | (not `f00022` — that frame is pure black, a door-transition frame) |
| `frames/follow-oak.png` | `f00069.png` | |
| `frames/take-starter.png` | `f00154.png` | |
| `frames/battle-rival.png` | `f00354.png` | |
| `frames/route-1.png` | `f00598.png` | |
| `frames/collect-parcel.png` | `f00648.png` | |
| `frames/buy-pokeballs.png` | `f01207.png` | shows the actual BUY menu + `MONEY ₽3175` |
| `frames/forest-gate.png` | `f01329.png` | (not `f01322` — that frame is half-black, mid-scroll) |

Regen: `ffmpeg -y -i <capture>/f<NNNNN>.png -vf "scale=960:864:flags=neighbor" <out>.png`
— **`flags=neighbor` (nearest-neighbour) is load-bearing**: a smooth scaler blurs GB
pixel art when upscaling 6×; neighbour keeps every pixel a hard-edged square, matching
the aesthetic the PixelGB assets already use.

**The 16-agent swarm grid**: `pokemon-run-txtscr/film/mgba-screen-triggers.mp4/mgba-grid-16agents-mobile.mp4`
(720×776, 30fps, 260.13s), used directly (no re-encode) via `trim: [120.0, 8.0]` — an
8s window starting 120s in, chosen by eye for a mix of agents mid-run rather than all
freshly spawned or all long finished. `grid-manifest.json` beside it (same
`mgba-screen-triggers.mp4/` directory) is the source of the film's "3 of 16" figure —
see the stats section below for where "11 of 16" comes from (a different, later run,
no video).

## 3. Derived-asset prep (one-time, plain tools, no ShowReel code)

```python
from PIL import Image
im = Image.open(source).convert("RGBA")
im.crop(im.getbbox()).save(dest)   # oak/rival portraits: 448x448 -> 232x448
```
Both cartridge portraits ship on a much wider transparent canvas than the character
drawn on it (52% fill). Feeding the padded original into a `still` layer with
`fit: "contain"` sizes against the *canvas's* 1:1 aspect ratio, not the subject's —
the character renders noticeably smaller than the placement box appears to allow.
Cropping to `getbbox()` first fixes this; see the sharp edge in the project `AGENTS.md`
for the general version of this gotcha.

```
ffmpeg -y -i atlas/region-tint/2x/kanto.png -vf "scale=2400:-1" kanto-atlas-2400.png
```
The source atlas decodes to ~880 MB of raw RGBA at full size (15008×14688) — far more
than a static, never-zoomed-in background needs. Downscaled once to 2400px wide
(5.3 MB PNG) for the goal card.

## 4. The "today" stats — exact source and verification

- **"3 of 16" → "11 of 16", 8 gained, 0 lost**: `grid-manifest.json`
  (`pokemon-run-txtscr/film/mgba-screen-triggers.mp4/`) gives the "before" figure
  (`"finished": 3` of 16, milestone `cross-viridian-forest`) — this is also the run the
  swarm-grid video shows. The "after" figure (11/16) is a **separate, later** paired
  run at `pokemon-run-fbt/paired/{before,after}-{0..15}.json` (32 files, same 16 seeds,
  one variable changed — a name-free screen-read recognizer replacing one that broke on
  a foreign emulator's cold-boot text). Counted directly from the raw JSON
  (`finished: true/false` per file), not taken from a doc's prose: before 3/16, after
  11/16, every seed that finished before still finishes after (8 gained, 0 lost, none
  regressed). No swarm-grid video exists for the "after" run — the film's swarm-grid
  clip is the "before" state; the counter animating 3→11 is a stat overlay on
  representative footage, not a claim that the clip itself shows 11 agents finishing.
- **"0/20" vs "20/20" shop-buying**: `pokemon-run-shop/arms/{w775,w475}-{adapter,money}.json`
  — `*-adapter.json` (network alone, no screen-read rule) is `bought_at_least_one: 0`
  at both wallet levels tested (¥775 and ¥475); `*-money.json` (the shipped rule) is
  `bought_at_least_one: 20` at both, `balls_median: 3.0` / `2.0` respectively. The
  on-screen caption says "a wallet too poor for the full order" rather than "too poor
  to afford anything" deliberately — ¥775/¥475 could afford 2-3 balls, just not the
  network's over-committed order of 5; that distinction is in the raw data, not just
  the doc prose.
- **Current frontier is Viridian Forest, not Pewter**: `data/student-config.json`'s
  shipped `"current"` config's last link is `cross-viridian-forest`; configs that go
  further (toward Pewter) are all marked `"status": "candidate"`, not shipped. The most
  recent attempt past the forest (dated 2026-09-02, `fm/gb-pewter-grid-9`) had 0/9 agents
  reach Pewter, 7/9 stalling on an identical Route 2 tile pair. The film's end card says
  "next: Route 2, then Pewter" for this reason — not "stops around Pewter" as looser
  framing might suggest, and never implies Pewter itself has been reached.

All four of the above were independently verified against raw JSON/log files, not
taken from any doc's summary prose — see the captain-facing delivery note for the
full verification trail.
