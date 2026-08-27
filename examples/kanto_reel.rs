//! The captain's shot, built entirely through ShowReel's public API.
//!
//! > Open on Pokémon Blue's title screen → pull back to reveal the whole world
//! > map → battle moments from our swarm runs burst out from where they
//! > happened.
//!
//! Nothing here reaches into ShowReel's internals and nothing in ShowReel
//! knows what a Game Boy is. Everything specific to this subject — which map
//! sits where in the atlas, which second of which film shows which milestone —
//! lives in this file, as *input*.
//!
//! Run it to write the film description, then render it:
//!
//! ```text
//! cargo run --release --example kanto_reel -- --assets <dir> --out kanto.film.json
//! showreel render kanto.film.json -o kanto-reel.mp4
//! ```
//!
//! ## Where the assets come from, and what is real
//!
//! - `kanto.png` — PixelGB's atlas of all 226 Pokémon Blue maps, 6832 × 7024,
//!   48.0 megapixels. Produced by `pixelgb atlas --rom <blue.gb>`.
//! - `title-screen.mp4` — a real capture of the cartridge booting, taken
//!   headlessly from the `terminalgb` emulator's `video_out` example. It is the
//!   game's own title screen, not a picture of one.
//! - `pixel-chain-run.mp4` — `agentgb`'s screen-only policy playing a real cold
//!   boot through the opening chain, captions burned in by the run itself.
//! - `pixel-chain-grid.mp4` — 26 of those runs at once.
//!
//! The map rectangles below are read off PixelGB's own `atlas.json`. The clip
//! timestamps were checked by extracting the frame at each one and reading the
//! milestone caption the run burned into it, so "this moment happened here" is
//! a claim with evidence behind it rather than a guess.

use showreel::prelude::*;

/// The atlas PixelGB writes, in pixels.
const ATLAS_W: f64 = 6832.0;
const ATLAS_H: f64 = 7024.0;

/// A map's rectangle in that atlas, straight from `atlas.json`.
struct MapRect {
    name: &'static str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Pallet Town's centre, which the pull-back is framed on at both ends.
const PALLET_CX: f64 = 1360.0 + 320.0 / 2.0;
const PALLET_CY: f64 = 4416.0 + 288.0 / 2.0;

const PALLET_TOWN: MapRect = MapRect { name: "Pallet Town", x: 1360.0, y: 4416.0, w: 320.0, h: 288.0 };
const REDS_HOUSE_2F: MapRect = MapRect { name: "Red's house, upstairs", x: 1216.0, y: 4352.0, w: 128.0, h: 128.0 };
const OAKS_LAB: MapRect = MapRect { name: "Oak's Lab", x: 1696.0, y: 4576.0, w: 160.0, h: 192.0 };
const ROUTE_1: MapRect = MapRect { name: "Route 1", x: 1360.0, y: 3840.0, w: 320.0, h: 576.0 };
const VIRIDIAN_CITY: MapRect = MapRect { name: "Viridian City", x: 1200.0, y: 3264.0, w: 640.0, h: 576.0 };

/// Where a point of the atlas lands in the frame when the camera is showing
/// the whole thing.
///
/// `Framing::Whole` widens the atlas to the frame's aspect ratio and centres
/// it, so the visible span is `ATLAS_H * aspect` wide starting left of zero.
/// This is the only arithmetic the example needs, and it belongs here rather
/// than in the library: it is a fact about *this* source image.
fn atlas_to_frame(x: f64, y: f64, aspect: f64) -> (f64, f64) {
    let view_w = ATLAS_H * aspect;
    let left = (ATLAS_W - view_w) / 2.0;
    (((x - left) / view_w).clamp(0.0, 1.0), (y / ATLAS_H).clamp(0.0, 1.0))
}

fn centre_of(m: &MapRect, aspect: f64) -> (f64, f64) {
    atlas_to_frame(m.x + m.w / 2.0, m.y + m.h / 2.0, aspect)
}

/// One moment of real footage, and the place it happened.
///
/// The opening chain all happens within about five hundred atlas pixels of
/// Pallet Town, which in a frame showing all 226 maps is a few dozen pixels
/// across. Drawing each clip *at* its map would pile them on top of each
/// other, so the inset is hand-placed around the edge of the frame and a
/// callout draws the line back to where it actually happened. That is the
/// explainer idiom, and it is also the truthful one: the line, not the
/// position, is the claim.
struct Burst {
    /// Which film, and the seconds into it that were checked.
    asset: &'static str,
    from_source: f64,
    duration: f64,
    /// The milestone caption burned into the footage at that timestamp.
    milestone: &'static str,
    place: &'static MapRect,
    /// When it appears in the reel, and for how long.
    at: f64,
    lasting: f64,
    /// Top-left of the inset, in frame fractions, and its width.
    inset: (f64, f64),
    size: f64,
}

const BURSTS: &[Burst] = &[
    Burst {
        asset: "pixel-chain-run.mp4",
        from_source: 8.0,
        duration: 3.4,
        milestone: "leave-the-bedroom",
        place: &REDS_HOUSE_2F,
        at: 0.5,
        lasting: 3.4,
        inset: (0.045, 0.10),
        size: 0.165,
    },
    Burst {
        asset: "pixel-chain-run.mp4",
        from_source: 176.0,
        duration: 4.2,
        milestone: "the rival battle",
        place: &OAKS_LAB,
        at: 2.0,
        lasting: 4.2,
        inset: (0.605, 0.085),
        size: 0.195,
    },
    Burst {
        asset: "pixel-chain-run.mp4",
        from_source: 277.0,
        duration: 3.4,
        milestone: "out-of-the-lab",
        place: &PALLET_TOWN,
        at: 3.7,
        lasting: 3.4,
        inset: (0.045, 0.60),
        size: 0.165,
    },
    Burst {
        asset: "pixel-chain-run.mp4",
        from_source: 330.0,
        duration: 3.4,
        milestone: "north-out-of-pallet",
        place: &ROUTE_1,
        at: 5.3,
        lasting: 3.4,
        inset: (0.655, 0.585),
        size: 0.165,
    },
    Burst {
        asset: "pixel-chain-grid.mp4",
        from_source: 22.0,
        duration: 4.4,
        milestone: "26 cold boots at once",
        place: &VIRIDIAN_CITY,
        at: 6.8,
        lasting: 4.4,
        inset: (0.045, 0.115),
        size: 0.215,
    },
];

fn ink() -> Color {
    Color::WHITE
}

fn accent() -> Color {
    Color::rgb(255, 209, 71)
}

fn deep() -> Color {
    Color::rgb(7, 9, 14)
}

fn build(aspect: f64) -> Film {
    // ---- 1. the title screen ------------------------------------------
    let title = Scene::new(5.2)
        .named("title screen")
        .background(deep())
        .layer(Layer::gradient(
            vec![(0.0, Color::rgb(20, 27, 48)), (1.0, Color::rgb(6, 8, 13))],
            108.0,
        ))
        .layer(
            // The cartridge's own title screen, nearest-neighbour magnified so
            // the Game Boy's pixels stay square.
            Layer::clip("title-screen.mp4")
                .fit(Fit::Contain)
                .trim(0.0, 5.0)
                .decode_fps(30.0)
                .decode_width(640)
                .frac(0.5 - 0.30, 0.06, 0.60, 0.64)
                .framed(6.0, Some((Color::rgba(255, 255, 255, 46), 2.0)))
                .entering(Motion::scale_up(0.9)),
        )
        .layer(Layer::scrim().from(1.0))
        .layer(
            Layer::title("AI plays Pokémon")
                .subtitle("600 cold boots · one retail cartridge · no save states")
                .frac(0.1, 0.74, 0.8, 0.2)
                .from(1.1)
                .entering(Motion::chars(0.52, 0.020)),
        );

    // ---- 2. the pull-back ----------------------------------------------
    // The move the whole toolset was built for: open at 1:1 on the Game Boy's
    // own 160x144 window over Pallet Town, hold, then pull back until all 226
    // maps are in frame. 48.0 megapixels, in one continuous move.
    let pull_back = Scene::new(11.0)
        .named("the pull-back")
        .background(deep())
        .layer(Layer::camera(
            "kanto.png",
            // Both ends are framed on the *same point* — Pallet Town — and only
            // the height changes. The camera therefore stays on its subject for
            // as long as the subject can stay centred, and `clamp_to_source`
            // slides the frame to the middle only once the viewport is wider
            // than the atlas. Naming the far end `Framing::Whole` instead makes
            // the centre travel from the start, which drifts off Pallet Town
            // onto empty ground while still too zoomed in to show context.
            Camera::new()
                .to(0.0, Framing::point(PALLET_CX, PALLET_CY, 144.0))
                .hold_until(1.1)
                .shot(
                    Shot::new(9.6, Framing::point(PALLET_CX, PALLET_CY, ATLAS_H))
                        .eased(Easing::InOutCubic),
                ),
        ))
        .layer(
            Layer::lower_third("Kanto, entire")
                .detail("226 maps · 6 832 × 7 024 px · one picture")
                .accent(accent())
                .from(1.6)
                .lasting(6.0)
                .exiting(Motion::slide_in(0.45, -70.0, 0.0)),
        )
        .layer(
            Layer::counter(0.0, 226.0, 3.2)
                .label("maps in this picture")
                .from(3.0)
                .exiting(Motion::fade(0.5)),
        );

    // ---- 3. the bursts --------------------------------------------------
    // The whole map held still while real footage appears at the coordinates
    // where it was recorded.
    let mut bursts = Scene::new(11.4)
        .named("where they happened")
        .background(deep())
        .layer(Layer::camera("kanto.png", Camera::hold(Framing::Whole)))
        // Knock the map back a little so the footage reads as being on top of
        // it rather than in it.
        .layer(Layer::solid(Color::rgba(5, 7, 12, 168)).from(0.3).entering(Motion::fade(0.7)));

    for b in BURSTS {
        let (tx, ty) = centre_of(b.place, aspect);
        let (fx, fy) = b.inset;
        let fh = b.size * aspect;
        // The label sits below its inset, so the plate never covers the
        // footage it is naming.
        let label_at = (fx + b.size * 0.5, fy + fh + 0.045);
        bursts = bursts
            .layer(
                Layer::clip(b.asset)
                    .trim(b.from_source, b.duration)
                    .decode_fps(30.0)
                    .decode_width(480)
                    .fit(Fit::Contain)
                    .frac(fx, fy, b.size, fh)
                    .framed(8.0, Some((Color::rgba(255, 255, 255, 200), 2.5)))
                    .from(b.at)
                    .lasting(b.lasting)
                    .entering(Motion::scale_up(0.55))
                    .exiting(Motion::new(MotionKind::Scale { from: 0.8 }, 0.4))
                    .z(10),
            )
            .layer(
                Layer::callout(b.place.name, (tx, ty), label_at)
                    .detail(b.milestone)
                    .accent(accent())
                    .from(b.at + 0.3)
                    .lasting(b.lasting - 0.3)
                    .entering(Motion::fade(0.55))
                    .exiting(Motion::fade(0.35))
                    .z(20),
            );
    }

    bursts = bursts.layer(
        Layer::counter(0.0, 584.0, 2.4)
            .label("runs finished")
            .from(1.0)
            .z(30),
    );

    // ---- 4. pull one up -------------------------------------------------
    // The explainer move: take what is already on screen, dim the rest, bring
    // it forward, name it.
    let (bx, by) = (0.605, 0.085);
    let bw = 0.195;
    let pull_up = Scene::new(5.6)
        .named("pull it up")
        .background(deep())
        .layer(Layer::camera("kanto.png", Camera::hold(Framing::Whole)))
        .layer(Layer::solid(Color::rgba(5, 7, 12, 168)))
        .layer(
            Layer::clip("pixel-chain-run.mp4")
                .trim(176.0, 5.4)
                .decode_fps(30.0)
                .decode_width(480)
                .fit(Fit::Contain)
                .frac(bx, by, bw, bw * aspect)
                .framed(8.0, Some((Color::rgba(255, 255, 255, 190), 2.5)))
                .z(10),
        )
        .layer(
            Layer::pull_up((bx - 0.005, by - 0.005, bw + 0.010, bw * aspect + 0.010))
                .label("Oak's Lab — the rival battle, decision 226")
                .accent(accent())
                .from(0.5)
                .entering(Motion::new(MotionKind::Scale { from: 0.0 }, 1.0).timed(
                    Timing::eased(Easing::InOutCubic),
                ))
                .z(40),
        );

    // ---- 5. the end card -------------------------------------------------
    let end = Scene::new(4.6)
        .named("end card")
        .background(deep())
        .layer(Layer::gradient(
            vec![(0.0, Color::rgb(18, 24, 42)), (1.0, Color::rgb(6, 8, 13))],
            108.0,
        ))
        .layer(
            Layer::title("ShowReel")
                .subtitle("timeline · camera · typography · transitions")
                .frac(0.1, 0.30, 0.8, 0.28)
                .entering(Motion::words(0.6, 0.07)),
        )
        .layer(
            Layer::text("Every frame in this film is a pure function of one JSON description.")
                .styled(
                    TextStyle::default()
                        .family("Open Sans")
                        .size(30.0)
                        .weight(400)
                        .colour(Color::rgb(150, 160, 178))
                        .centred(),
                )
                .frac(0.15, 0.62, 0.7, 0.1)
                .from(1.3)
                .entering(Motion::rise(0.6)),
        );

    Film::new(1920, 1080, 60.0)
        .title("Kanto, entire — a ShowReel demonstration")
        .background(deep())
        .theme(Theme::dark())
        .open(title)
        .then(Transition::fade_black(0.75), pull_back)
        .then(Transition::dissolve(0.85), bursts)
        .then(Transition::dissolve(0.7), pull_up)
        .then(Transition::fade_black(0.8), end)
}

fn main() -> anyhow::Result<()> {
    let mut out = std::path::PathBuf::from("kanto.film.json");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" | "-o" => out = args.next().unwrap_or_default().into(),
            other => eprintln!("ignoring unknown argument {other:?}"),
        }
    }

    let film = build(1920.0 / 1080.0);
    let errs = film.validate();
    if !errs.is_empty() {
        for e in &errs {
            eprintln!("error: {e}");
        }
        anyhow::bail!("the film does not validate");
    }
    std::fs::write(&out, film.to_json()?)?;
    println!(
        "{} — {} scenes, {:.2}s, {} frames at {}x{}",
        out.display(),
        film.timeline.scene_count(),
        film.duration().as_secs(),
        film.frame_count(),
        film.width,
        film.height
    );
    for u in film.assets_used() {
        println!("  needs {u:?}");
    }
    let _ = ink();
    Ok(())
}
