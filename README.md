# ShowReel

A general toolset for making demo, explainer and animation films: a **timeline**,
a **camera**, **typography**, and **transitions**. A film is *described*
declaratively and rendered deterministically — same description in, same frames
out.

It is a library and a CLI. It knows nothing about any particular subject; maps,
screen captures and clips are *inputs*.

```
showreel render  film.json -o reel.mp4    # master + 720p/30fps mobile cut
showreel still   film.json --at 4.2s      # one frame, in milliseconds
showreel sheet   film.json --every 1s     # the whole film as a labelled grid
showreel preview film.json --scale 0.35   # a fast pass over the real timeline
showreel info    film.json                # scenes, timings, assets
```

## What it does

- **Timeline** — `Scene (Transition Scene)*`. A transition overlaps its two
  neighbours, so `4s + 1s + 4s` is a 7-second film.
- **Camera** — zoom, pan and hold over a source much larger than the frame. A
  48-megapixel still renders at **4.3 ms/frame** to 1080p (measured; see
  `src/assets/still.rs`).
- **Transitions** — cut, dissolve, fade-through-colour, wipe, slide, push, iris
  and zoom, each composable with any timing curve or spring.
- **Easing** — the usual curves, cubic Béziers, and real springs.
- **Layers** — stills, clips, solids, gradients and scrims, composited by
  position, scale, opacity and z-order.
- **Typography** — real shaping (kerning, ligatures), tracking in ems, tabular
  figures, shadows, outlines, word wrap, auto-fit, and per-character or
  per-word kinetic entrances.
- **Overlays** — titles, lower-thirds, callouts that point at things, and
  counters that tick in time with the footage.
- **Pull-up** — lift a region of what is on screen, dim the rest, bring it
  forward enlarged and annotated.
- **Output** — a full-quality mp4 and the Telegram-safe 720p/30fps cut, from one
  command.

## Writing a film

In Rust:

```rust
use showreel::prelude::*;

let film = Film::new(1920, 1080, 60.0)
    .title("A short reel")
    .open(
        Scene::new(4.0)
            .named("title")
            .layer(Layer::gradient(vec![(0.0, Color::parse("#16213a").unwrap()),
                                        (1.0, Color::parse("#070a11").unwrap())], 110.0))
            .layer(Layer::title("AI plays Pokémon")
                .subtitle("600 cold boots, one cartridge")
                .entering(Motion::chars(0.5, 0.022))),
    )
    .then(
        Transition::dissolve(0.8),
        Scene::new(6.0)
            .named("the map")
            .layer(Layer::camera("world.png",
                Camera::pull_back(Framing::at(0.15, 0.80, 288.0), 5.0)))
            .layer(Layer::lower_third("Kanto").detail("226 maps").from(1.0)),
    );
```

Or as JSON, which is the same tree:

```json
{
  "width": 1920, "height": 1080, "fps": 60,
  "opening": { "duration": 4.0, "layers": [
    { "type": "title", "text": "AI plays Pokémon",
      "enter": { "kind": "chars", "stagger": 0.022, "duration": 0.5 } }
  ]},
  "then": [
    { "transition": { "duration": 0.8, "presentation": { "kind": "dissolve" } },
      "scene": { "duration": 6.0, "layers": [] } }
  ]
}
```

The JSON shape cannot express a timeline that begins with a transition, or two
transitions in a row — see `src/timeline.rs`.

## Requirements

`ffmpeg` and `ffprobe` on `PATH`. Fonts are found from the system font
directories; `showreel fonts` lists what it can see.

## The worked example

`examples/kanto_reel.rs` builds a 35-second film — a Game Boy title screen, a
pull-back over a 48-megapixel map of all 226 Pokémon Blue maps, real run
footage bursting out at the coordinates where it was recorded, and a pull-up on
one of them — entirely through the public API. Nothing in `src/` knows what any
of it is.

```bash
cargo run --release --example kanto_reel -- -o kanto.film.json
showreel sheet  kanto.film.json -A <assets> --every 1.5s   # look before rendering
showreel render kanto.film.json -A <assets> -o kanto-reel.mp4
```

Measured on a 20-core machine, 1920×1080 at 60fps:

| | frames | wall | per frame | peak RSS |
|---|---|---|---|---|
| the 48.0 MP pull-back alone | 660 | 10.0 s | 15.2 ms | 2.09 GB |
| the whole film | 2082 | 26.2 s | 12.6 ms | 2.16 GB |

Rendering the same description twice gives byte-identical PNGs and byte-identical
mp4s.

## Iterating

Rendering a film to judge its timing is the slow way round. In rough order of
cost:

| | what it answers | cost here |
|---|---|---|
| `showreel still --at 4.2s` | "what does this moment look like?" | ~100 ms |
| `showreel sheet --every 1.5s` | "is the pacing right?" | ~3 s for the whole film |
| `showreel preview --scale 0.35` | "does the motion work?" | ~8× less pixel work |
| `showreel render` | the delivery | full cost |

`sheet` renders from a genuinely scaled-down film — type, padding, corner radii
and decode sizes all come down with the frame — so a thumbnail looks like the
film rather than like the film with 1080p text pasted on it.
