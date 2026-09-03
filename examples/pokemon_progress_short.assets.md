# Assets for `pokemon_progress_short.film.jsonc`

None of this film's assets are committed — they are all external, undistributed
research material from sibling projects on the machine this was built on, not
ShowReel fixtures. This file is the exact record of where every asset came from and
how the derived-asset prep steps were done, so the film can be rebuilt (or rebuilt
with a fresher swarm run / fresher stats) elsewhere.

**v2** (captain's direction after watching v1: open on the map-zoom payoff instead of
closing on it, zoom to Pallet Town specifically, bring back the starters and the game
character, and end the opener with a swarm bursting out of the house) replaced
`kanto-atlas-2400.png` with a higher-resolution, non-downscaled `kanto-atlas-full.png`
(§3 below — the opener now zooms in tight enough that the old 2400px-wide downscale
would have shown visible blur, not crisp GB tile art) and added the player's own
overworld sprite (§1) alongside the three starters already in use. Everything else in
this file (§1's four-shade art, §2's captured frames and the swarm clip, §4's stats) is
unchanged from v1 — re-verified, not re-derived.

**v3** (the captain rejected v2's burst on sight — seven still sprites sliding, not
walking) replaces that burst with real walk-cycle *clips* baked from the same sprite
sheet, and adds a new full-bleed real-footage insert right after it. Everything from §1
and §2 above is still used unchanged; §5 below is new.

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
- `sprites/four-shade/8x/overworld-red-{down,side,up}-walk.png` — **v2**: the player's
  own overworld sprite (the in-game protagonist is called "Red" regardless of
  cartridge version — "Blue" is the rival's own overworld sprite, not the player's).
  Used as a small "cast" cameo alongside the three starters. **v3**: also the source
  frames for the walk-cycle burst clips — see §5

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
(720×776, 30fps, 260.13s), used directly (no re-encode), twice, with two different
`trim` windows so the same pixels never repeat: `trim: [120.0, 8.0]` in TODAY (an 8s
window starting 120s in, chosen by eye for a mix of agents mid-run rather than all
freshly spawned or all long finished), and **v3**'s `trim: [40.0, 3.6]` in the new
opener insert (§5 — an earlier, more chaotic window: rival battles and menus rather
than TODAY's later shop/wild-battle mix). `grid-manifest.json` beside it (same
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

**v1** downscaled the committed 2x atlas for its (never zoomed in past a wide
establishing shot) goal-card background:
```
ffmpeg -y -i atlas/region-tint/2x/kanto.png -vf "scale=2400:-1" kanto-atlas-2400.png
```
**v2**'s opener needs a real push all the way to a single house's front door, which
that 2400px-wide downscale doesn't have the detail left to survive — so it regenerates
the atlas at native 1x resolution instead (`pixelgb atlas`'s own render, `tile_px=16`,
no interpolation to soften it) and uses the PNG directly, no further scaling:
```
pixelgb atlas --rom <pokemon-blue.gb> --out <dir> --scales 1
cp <dir>/pokemon-blue/atlas/region-tint/1x/kanto.png kanto-atlas-full.png
```
7504×7344, ~2.9 MB PNG (~200 MB decoded — well within a mip-backed still's budget; see
`src/camera.rs`'s "mip-backed" design). `pixelgb atlas` also writes `<dir>/atlas.json`
alongside the PNGs — every map's exact `rect` (atlas pixels at scale 1) and, for an
interior, its own door's tile coordinate — which is how the opener's camera targets a
*real* location rather than an eyeballed guess:

| Target | Source | Atlas pixel (1x) | `fx`, `fy` (pixel ÷ 7504, 7344) |
|---|---|---|---|
| Pallet Town (whole town) | `atlas.json` map id `0`'s `rect` centre: `x=1360+160, y=4400+144` | `(1520, 4544)` | `0.20256, 0.61873` |
| Red's own front door | same map's `rect` origin `+ warp[0]`'s tile `(5, 5) × tile_px 16`, centred on the door tile | `(1448, 4488)` | `0.19296, 0.61111` |

Both were also visually confirmed, not trusted to arithmetic alone — cropped straight
out of the freshly rendered atlas and eyeballed against the four-shade Pallet Town map
before committing to the film (matching warp index 0 to the *west* house, the one shown
in the crop, not the rival's house or the lab).

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

## 5. v3: walking burst clips + the real swarm-progress insert

**The walk-cycle clips** (`walk/walk-{down,left,right}.mov`, in the derived assets
dir). Each direction's real Gen-1 two-frame walk pose — the game's own sprite sheet
already draws both leg positions per direction, e.g. `overworld-red-down.png` and
`overworld-red-down-walk.png` are the *same* pose's two alternating frames, not an
idle vs. a walk sprite (confirmed by diffing them — `compare -metric AE` reports large
per-pixel differences, and eyeballing the two shows the classic alternating-leg
silhouette). "right" is a horizontal flip of "side" (the sheet only ever draws a
left-facing pose; the game mirrors it for right, done here with `ffmpeg -vf hflip`).
Each direction's 6-frame (A,B,A,B,A,B — three full strides), 12fps loop is built from
an explicit numbered image sequence (not `-loop 1`/concat, which under-counted frames
in an earlier attempt) and encoded **`qtrle` in a `.mov`**, not `libvpx-vp9`/webm —
see the "transparent `Content::Clip`" sharp edge in the project `AGENTS.md` for why.
Regen (per direction, `frameA`/`frameB` the direction's still + `-walk` pair):
```
mkdir seq
cp frameA.png seq/f01.png; cp frameB.png seq/f02.png
cp frameA.png seq/f03.png; cp frameB.png seq/f04.png
cp frameA.png seq/f05.png; cp frameB.png seq/f06.png
ffmpeg -framerate 12 -i seq/f%02d.png -c:v qtrle -pix_fmt argb walk-<direction>.mov
```
Verified end to end before trusting it in the film: `showreel still` at 0.1s steps
over a tiny test film using these clips, confirmed both the transparency (a known
background pixel decodes `alpha=0` after the full render pipeline, not just at the
ffmpeg-probe stage) and the leg alternation actually playing.

**Twelve walkers' headings**, fanned 12°-168° clockwise from +x (mostly downward and
outward, matching v2's own "never back into the building" constraint) with the crate's
own `burst_jitter` splitmix64 hash (`src/layer.rs`) ported to Python, seed `7`,
`jitter_deg=8`, so the wobble is reproducible the same way `Layer::burst` itself is —
not hand-picked numbers. Distance 500px, `scale_to: 1.6`, 0.95s on screen (about two
walk-cycle loops — enough to read as running, not one step), 0.09s stagger between
launches. Direction picked per walker from its own heading (`<65°` right, `>115°`
left, otherwise down) rather than cycled for texture, as v2's own note admitted. The
generator script (not committed — a throwaway, the film's own JSON is what's
canonical) is a ~30-line Python file computing `dx`/`dy` from each heading and
printing the layer JSON directly; `burst_jitter`'s own splitmix64 steps (`src/layer.rs`)
are the exact formula it ports — read that function if reproducing by hand.

**The real swarm-map insert** (`swarm-map-progress.mp4`, in the derived assets dir):
a crop + trim of `teacher-swarm-cerulean-mobile.mp4` (878×494, 30fps, 316.5s — a
32-agent swarm timelapse crossing the actual Kanto map, captured the night this brief
landed). Cropped to `705×452` from the top-left to drop the debug sidebar (agent
count/stats text) and footer (a status bar), keeping just the map + agent-dot
telemetry; trimmed to a 3.6s window starting at 54s, picked by eye for a visible
cluster of agent dots with the "emulated minutes" readout visibly ticking up across
the window (real motion, not a frozen frame with fake motion blur). Regen:
```
ffmpeg -ss 54 -t 3.6 -i teacher-swarm-cerulean-mobile.mp4 \
  -vf "crop=705:452:0:0" -c:v libx264 -crf 18 -pix_fmt yuv420p -an \
  swarm-map-progress.mp4
```
Placed as the top half of a split-screen (the bottom half is the grid clip's own
`trim`, done directly in the film JSON with no pre-processing — see §2 above), a
divider line, and one caption. The caption is deliberately short and generic ("the
swarm, at scale." / "representative footage, not proof") — this footage is a
different, later swarm run than the one behind any on-screen stat in this film, and
must never be captioned as if it proves one.
