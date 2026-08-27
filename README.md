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
