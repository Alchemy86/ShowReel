# `burst_demo` assets

The proof film for the burst effect (`Drift` + `Layer::burst`) is
`examples/burst_demo.film.jsonc`. It deliberately uses synthetic, generic
clips rather than real footage — the effect is subject-agnostic (any
clip works: battle footage, screen captures, anything), and the crate rule
that nothing in `src/` may know what a film is about extends to keeping its
own proof film's assets equally generic.

## NOT committed (in `examples/burst_demo/`, `.gitignore`d)

- **`battle-1.mp4` … `battle-6.mp4`** — six 2.5 s, 480×270, 30 fps synthetic
  clips, each `ffmpeg`'s own `testsrc2` test pattern hue-shifted to a distinct
  colour so six overlapping, travelling clips stay visually distinguishable at
  a glance. ~180 KB each. Regenerate with:

```bash
cd examples/burst_demo
i=1
for h in 0 60 120 180 240 300; do
  ffmpeg -nostdin -v error -y \
    -f lavfi -i "testsrc2=size=480x270:rate=30:duration=2.5" \
    -vf "hue=h=${h}:s=2.2" -pix_fmt yuv420p -threads 2 "battle-$i.mp4"
  i=$((i+1))
done
```

(Note the `${h}` braces — zsh's `$h:s=...` parses `:s` as a history-style
parameter modifier without them.)

Any six short clips at this path work just as well; the film references them
as bare names resolved through `-A examples/burst_demo`.

## The rendered cut (committed, under `docs/`)

- **`docs/burst-demo.mp4`** — the proof. Re-render with:

```bash
showreel render examples/burst_demo.film.jsonc -A examples/burst_demo --crf 23 -o docs/burst-demo.mp4
```

If you edit the film's timing or the `BurstSpec` this was generated from, use
`showreel sheet`/`still` to re-check the pacing (see the tuning notes at the
top of the film file) before re-rendering — this effect lives and dies by
timing, and the contact sheet is seconds where a render is not.
