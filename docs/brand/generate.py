#!/usr/bin/env python3
"""Generate the ShowReel brand SVGs.

Every letterform is hand-drawn here as a stroked skeleton path — **no font is
embedded, subset or traced**, so there is no third-party licence in these
files.  The panel, the alphabet, the tight tracking, the accent full stop and
the grey wide-tracked tagline are the house style shared with the sibling
projects (TerminalGB, AgentGB, AtlasGB, AsciiWorldEngine); the *motif* and the
*accent colour* are this project's own.

Run from anywhere:  python3 docs/brand/generate.py
It rewrites `showreel-logo.svg` and `showreel-icon.svg` deterministically.
Edit this file, never the SVGs by hand.
"""

import math
import os

OUT = os.path.dirname(os.path.abspath(__file__))

# ---------------------------------------------------------------------------
# Palette.  Panel, border, wordmark white and tagline grey are the house
# values, unchanged — that is what makes this a sibling mark rather than a
# different project.  The ACCENT is this project's own: `#ffd147` is
# `Theme::accent` out of `src/theme.rs`, the amber that draws every
# lower-third bar, every callout ring and leader line, and every pull-up
# border in a ShowReel film.  Look at any still from `examples/kanto_reel.rs`
# and this is the only colour in it.  The mark and the product agree.
BG = "#0d1117"      # near-black panel (matches GitHub dark, works on light)
EDGE = "#30363d"    # faint panel border so the card reads on pure black too
FG = "#f0f3f6"      # wordmark white
GREY = "#8b949e"    # tagline grey
AMBER = "#ffd147"   # Theme::accent — the film's own colour

TAGLINE = "DESCRIBE A FILM AND RENDER IT"

# ---------------------------------------------------------------------------
# Stroke-skeleton capital letters.  Cap height 100, stroke 26 (half-stroke 13);
# every endpoint is inset 13 so round caps land on the ink edge.  The value is
# (advance width, list of path data).
#
# These are the shapes the sibling marks already draw, unchanged, so the
# wordmarks are visibly the same alphabet.  F is new here and is drawn to the
# same rules — it is E without the foot, same cap height, same inset, same
# stroke.
S = 13
GLYPHS = {
    'A': (76, ["M11 87 L38 15 L65 87", "M23 62 H53"]),
    'B': (66, ["M13 13 V87", "M13 13 H35 A18 18 0 0 1 35 49 H13",
               "M13 49 H37 A19 19 0 0 1 37 87 H13"]),
    'C': (60, ["M42 18 A19 37 0 1 0 42 82"]),
    'D': (68, ["M13 13 V87", "M13 13 H31 A24 37 0 0 1 31 87 H13"]),
    'E': (56, ["M43 13 H13 V87 H43", "M13 50 H36"]),
    'F': (56, ["M43 13 H13 V87", "M13 50 H36"]),
    'H': (66, ["M13 13 V87", "M53 13 V87", "M13 50 H53"]),
    'I': (26, ["M13 13 V87"]),
    'L': (54, ["M13 13 V87 H41"]),
    'M': (86, ["M13 87 V13 L43 57 L73 13 V87"]),
    'N': (64, ["M13 87 V13 L51 87 V13"]),
    'O': (64, ["M32 13 A19 37 0 1 0 32 87 A19 37 0 1 0 32 13"]),
    'R': (68, ["M13 87 V13 H36 A19 19 0 0 1 36 51 H13", "M37 54 L54 87"]),
    'S': (72, ["M57 13 H36 A23 18.5 0 0 0 36 50 A23 18.5 0 0 1 36 87 H15"]),
    'T': (72, ["M13 13 H59", "M36 13 V87"]),
    'W': (96, ["M13 13 L33 87 L48 34 L63 87 L83 13"]),
    ' ': (30, []),
}

TRACK = 8  # tight letter spacing


def word_width(text, track=TRACK, widths=None):
    w = 0
    for i, ch in enumerate(text):
        w += (widths.get(ch) if widths and ch in widths else GLYPHS[ch][0])
        if i < len(text) - 1:
            w += track
    return w


def draw_word(text, x, y, scale, color, track=TRACK):
    """SVG for `text` with the letter grid's top-left at (x, y)."""
    parts = []
    cx = 0.0
    for ch in text:
        w, paths = GLYPHS[ch]
        for d in paths:
            parts.append(
                f'<path transform="translate({x + cx * scale:.1f} {y:.1f}) '
                f'scale({scale:.4f})" d="{d}" fill="none" stroke="{color}" '
                f'stroke-width="26" stroke-linecap="round" '
                f'stroke-linejoin="round"/>')
        cx += w + track
    return "\n".join(parts)


def svg(width, height, body, comment):
    return (f"<!-- {comment}\n"
            "     Hand-authored for ShowReel.  No font embedded, subset or "
            "traced;\n     letterforms are original stroked paths. "
            "Regenerate with docs/brand/generate.py -->\n"
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} '
            f'{height}" width="{width}" height="{height}" role="img">\n'
            f"{body}\n</svg>\n")


def panel(w, h, rx=24):
    return (f'<rect x="1" y="1" width="{w - 2}" height="{h - 2}" rx="{rx}" '
            f'fill="{BG}" stroke="{EDGE}" stroke-width="2"/>')


def write(name, content):
    path = os.path.join(OUT, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as handle:
        handle.write(content)
    print(f"wrote {name} ({len(content)} bytes)")


# ---------------------------------------------------------------------------
# THE MARK.
#
# The house composition: one heavy geometric wordmark, tight-tracked, white on
# a near-black panel; the motif REPLACING one letter; an accent-colour full
# stop closing the word; the tagline underneath, lighter, grey, wide-tracked.
#
# The motif is a **lens iris** standing where the O of SHOW would be.  The
# letter is a ring already, so the substitution costs the word nothing — and
# what stands in its place is the one capability the whole toolset was built
# around.  ShowReel's headline feature is a camera that zooms, pans and holds
# over a source far larger than the frame (`src/camera.rs`), and `Iris` is a
# transition it actually ships (`src/transition.rs`): the aperture is not a
# generic camera pictogram borrowed to mean "video", it is a thing in the code.
#
# The blades are drawn straight and hard-edged against an alphabet of round
# terminals, so the iris reads as a mechanism sitting in a line of letters
# rather than as another glyph — the same trick the sibling marks use.
#
# Under the wordmark, where AsciiWorldEngine puts a skyline, ShowReel puts its
# **timeline**: scenes as blocks, and between them the short overlap where a
# transition runs and both scenes are on screen at once.  That overlap is the
# arithmetic in `Timeline::placements` — a 4s scene, a 1s transition and a 4s
# scene make a 7 second film, not 9 — drawn at a glance.
#
# Nothing here is borrowed: the whole vocabulary is an aperture, a timeline
# and a full stop.

IRIS_W = 82         # the plot the iris stands on, wider than the O it replaces
BLADES = 6


def _aperture(parts, cx, cy, inner, ap, seam_w, ground):
    """A diaphragm: a filled disc, a polygon hole, and the blade seams.

    Drawn this way round — fill, then knock the opening out in the panel
    colour, then cut the seams — because that is how the mechanism actually
    reads.  Building it out of triangles instead produces a pinwheel of spikes
    that looks like a star, which was the first attempt and was wrong.
    """
    parts.append(f'<circle cx="{cx:.2f}" cy="{cy:.2f}" r="{inner:.2f}" '
                 f'fill="{AMBER}"/>')
    # The opening: a regular polygon, flat edge uppermost.
    rot = -math.pi / 2 - math.pi / BLADES
    verts = [(cx + ap * math.cos(rot + 2 * math.pi * i / BLADES),
              cy + ap * math.sin(rot + 2 * math.pi * i / BLADES))
             for i in range(BLADES)]
    pts = " ".join(f"{x:.2f},{y:.2f}" for x, y in verts)
    parts.append(f'<polygon points="{pts}" fill="{ground}"/>')
    # The seams: each blade's leading edge, running from an opening corner
    # outwards along that edge's own direction until it meets the rim.
    for i, (vx, vy) in enumerate(verts):
        px, py = verts[(i - 1) % BLADES]
        dx, dy = vx - px, vy - py
        n = math.hypot(dx, dy) or 1.0
        dx, dy = dx / n, dy / n
        parts.append(
            f'<line x1="{vx:.2f}" y1="{vy:.2f}" '
            f'x2="{vx + dx * inner:.2f}" y2="{vy + dy * inner:.2f}" '
            f'stroke="{ground}" stroke-width="{seam_w}" stroke-linecap="butt"/>')


def draw_iris(x, y, scale):
    """The lens iris, on the same 0..100 cap-height grid as the letters."""
    cx, cy = IRIS_W / 2.0, 50.0
    ring_r = 37.0               # stroke 26 centred here spans the full cap height
    inner = ring_r - S          # the clear opening inside the ring's ink
    parts = [f'<g transform="translate({x:.1f} {y:.1f}) scale({scale:.4f})">']
    _aperture(parts, cx, cy, inner, ap=11.0, seam_w=4.5, ground=BG)
    # The ring is the letter: same stroke weight as every other glyph, so the
    # iris sits on the line at exactly the letters' colour and weight. Drawn
    # last so its ink covers where the blades meet the rim.
    parts.append(
        f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{ring_r:.1f}" fill="none" '
        f'stroke="{FG}" stroke-width="26"/>')
    parts.append("</g>")
    return "\n".join(parts)


# Scene widths and the transition overlap between each pair, in strip units.
# Deliberately uneven: a timeline of equal blocks reads as a progress bar.
SCENES = (34, 21, 46, 17, 29)
OVERLAP = 7


def timeline(x, baseline, width, height=12):
    """Scenes as blocks, with the transition overlap marked between them."""
    total = sum(SCENES) - OVERLAP * (len(SCENES) - 1)
    unit = width / total
    parts = []
    cursor = 0.0
    for i, w in enumerate(SCENES):
        # Alternating weight, so abutting scenes read as separate blocks
        # rather than merging into one continuous bar.
        parts.append(
            f'<rect x="{x + cursor * unit:.1f}" y="{baseline:.1f}" '
            f'width="{w * unit:.1f}" height="{height}" rx="{height / 2:.1f}" '
            f'fill="{AMBER}" fill-opacity="{0.30 + 0.18 * (i % 2):.2f}"/>')
        if i < len(SCENES) - 1:
            # The overlap: where both scenes are live and the transition runs.
            ov_x = cursor + w - OVERLAP
            parts.append(
                f'<rect x="{x + ov_x * unit:.1f}" y="{baseline:.1f}" '
                f'width="{OVERLAP * unit:.1f}" height="{height}" '
                f'rx="{height / 2:.1f}" fill="{AMBER}"/>')
        cursor += w - OVERLAP
    return "\n".join(parts)


def logo():
    W, H = 1400, 420
    text = "SHOWREEL"
    IRIS_AT = 2            # the O of SHOW
    DOT_R = 16             # the full stop, bottom-aligned with the letter ink
    scale = 1.10
    # advance table with the iris's plot substituted for that one letter
    adv = [GLYPHS[c][0] for c in text]
    adv[IRIS_AT] = IRIS_W
    total = (sum(adv) + TRACK * len(text) + 2 * DOT_R) * scale
    x0 = (W - total) / 2
    y0 = 84
    parts = [panel(W, H)]
    cx = 0.0
    for i, ch in enumerate(text):
        if i == IRIS_AT:
            parts.append(draw_iris(x0 + cx * scale, y0, scale))
        else:
            parts.append(draw_word(ch, x0 + cx * scale, y0, scale, FG))
        cx += adv[i] + TRACK
    parts.append(
        f'<circle cx="{x0 + (cx + DOT_R) * scale:.1f}" '
        f'cy="{y0 + (100 - DOT_R) * scale:.1f}" r="{DOT_R * scale:.1f}" '
        f'fill="{AMBER}"/>')
    parts.append(timeline(x0, 284, total))
    tw = word_width(TAGLINE, track=14) * 0.30
    parts.append(draw_word(TAGLINE, (W - tw) / 2, 322, 0.30, GREY, track=14))
    write("showreel-logo.svg",
          svg(W, H, "\n".join(parts),
              "ShowReel logo — the wordmark, the iris, the timeline, the full stop"))


def icon():
    """The iris alone, square, at avatar and favicon size.

    The wordmark's motif distilled.  Sized so that at a 16 px favicon the ring
    is still a ring and the opening is still a hole: the ring stroke is 14 of
    128 (under 2 px at 16) and the opening is 38 across (nearly 5 px).  The
    wordmark's finer iris does not survive that reduction, so this one is drawn
    bolder on purpose.  `preview/icon-16.png` is the real test, not a resized
    illustration.
    """
    IW = 128
    c = IW / 2.0
    ring_r = 42.0
    inner = ring_r - 7.0
    body = [panel(IW, IW, rx=28)]
    # Deliberately bolder than the wordmark's iris: a bigger opening and
    # heavier seams, because at 16 px the fine version collapses to a blob.
    _aperture(body, c, c, inner, ap=19.0, seam_w=7.0, ground=BG)
    body.append(f'<circle cx="{c}" cy="{c}" r="{ring_r}" fill="none" '
                f'stroke="{FG}" stroke-width="14"/>')
    write("showreel-icon.svg",
          svg(IW, IW, "\n".join(body), "ShowReel icon — the lens iris"))


if __name__ == "__main__":
    logo()
    icon()
