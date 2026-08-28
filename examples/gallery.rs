//! The README gallery: one short, single-idea film per visual capability the
//! four studies in `docs/` say ShowReel can do. Each is authored to read
//! clearly at a README's width (~480px) and to last two or three seconds —
//! long enough to show one thing land, short enough to loop as a GIF.
//!
//! Self-contained like `showcase.rs`/`parallax_demo.rs`: the three capabilities
//! that need a picture (camera push, parallax, colour grade) get their stills
//! *drawn here*, procedurally, into `examples/gallery/assets/`, so a clean
//! clone can render every film with nothing else on disk. The other five need
//! no assets at all.
//!
//! ```text
//! cargo run --release --example gallery          # (re)writes films + stills
//! examples/gallery/render.sh                     # renders every GIF
//! ```
//!
//! The films are committed as `examples/gallery/*.film.jsonc`; the rendered
//! GIFs live in `docs/gallery/` and are shown in `README.md`. This `.rs` file
//! is the canonical source — the fluent builder calls here are the "HOW" the
//! JSON is the "what".

use showreel::prelude::*;
use std::path::{Path, PathBuf};

const AMBER: Color = Color::rgb(255, 209, 71);
const DEEP: Color = Color::rgb(9, 11, 17);
const CYAN: Color = Color::rgb(90, 210, 235);

/// Films are authored at 720p and downscaled to the GIF's width by ffmpeg's
/// Lanczos filter — a native render then a good downscale is crisper than
/// rendering small, and it lets one film serve any GIF width. Type is sized
/// generously so it survives the trip down to ~480px.
const W: u32 = 1280;
const H: u32 = 720;

// A cheap deterministic PRNG so the drawn stills never change between runs.
fn rng(seed: u64) -> impl FnMut() -> f64 {
    let mut s = seed;
    move || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((s >> 33) as f64) / (u32::MAX as f64 / 2.0)
    }
}

fn heading(t: &str) -> Layer {
    Layer::text(t)
        .styled(
            TextStyle::default()
                .family("Montserrat")
                .size(40.0)
                .weight(800)
                .tracking(0.18)
                .colour(AMBER)
                .centred(),
        )
        .frac(0.1, 0.06, 0.8, 0.12)
        .entering(Motion::rise(0.4))
        .z(50)
}

// ---- procedural stills ---------------------------------------------------

/// A detailed poster, larger than the frame, for the camera to move over —
/// fine hatching that only resolves close in, blocks that read as a pattern
/// far out. A push over a flat image proves nothing.
fn draw_poster(path: &Path) {
    let (w, h) = (1200u32, 1200u32);
    let (fw, fh) = (w as f64, h as f64);
    let mut cv = Canvas::filled(w, h, Color::rgb(12, 15, 22)).unwrap();
    let cells = 16.0;
    let step = fw / cells;
    for i in 0..=cells as u32 {
        let x = i as f64 * step;
        cv.line(
            x,
            0.0,
            x,
            fh,
            3.0,
            &Paint::Solid(Color::rgba(120, 140, 180, 26)),
        );
        cv.line(
            0.0,
            x,
            fw,
            x,
            3.0,
            &Paint::Solid(Color::rgba(120, 140, 180, 26)),
        );
    }
    // Detail kept deliberately blocky rather than finely-hatched: fine AA lines
    // compress terribly as PNG, and a committed asset should stay small. The
    // grid plus a couple of inner bars per block is enough to read as a real
    // zoom rather than an upscale.
    let mut rnd = rng(0x5eed);
    for gy in 0..cells as u32 {
        for gx in 0..cells as u32 {
            if rnd() > 0.55 {
                continue;
            }
            let pad = step * (0.12 + rnd() * 0.14);
            let r = Rect::new(
                gx as f64 * step + pad,
                gy as f64 * step + pad,
                step - pad * 2.0,
                step - pad * 2.0,
            );
            let lit = rnd();
            let base = if lit > 0.82 {
                AMBER.opacity(0.5)
            } else {
                Color::rgba(150, 170, 205, 40)
            };
            cv.fill_round_rect(r, step * 0.06, &Paint::Solid(base));
            cv.stroke_round_rect(
                r,
                step * 0.06,
                2.0,
                &Paint::Solid(Color::rgba(190, 210, 240, 60)),
            );
            let rows = (2.0 + rnd() * 3.0) as u32;
            for k in 1..rows {
                let y = r.y + r.h * k as f64 / rows as f64;
                cv.line(
                    r.x + 6.0,
                    y,
                    r.right() - 6.0,
                    y,
                    2.5,
                    &Paint::Solid(Color::rgba(210, 225, 245, 40)),
                );
            }
        }
    }
    // One amber marker to push in on.
    let mark = Rect::centred(fw * 0.3, fh * 0.62, step * 0.7, step * 0.7);
    cv.fill_round_rect(mark, step * 0.12, &Paint::Solid(AMBER));
    cv.stroke_round_rect(
        mark.inset(-step * 0.22),
        step * 0.16,
        5.0,
        &Paint::Solid(AMBER.opacity(0.55)),
    );
    cv.save_png(path).unwrap();
}

/// A colourful "evidence card" — a saturated gradient plus accent chips —
/// with enough colour variance that a desaturating, contrast-pushing grade
/// reads clearly against it.
fn draw_grade_card(path: &Path) {
    let (fw, fh) = (W as f64, H as f64);
    let mut cv = Canvas::new(W, H).unwrap();
    for y in 0..H {
        let t = y as f64 / fh;
        let top = Color::rgb(30, 90, 210);
        let bottom = Color::rgb(210, 70, 60);
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        cv.fill_rect(
            Rect::new(0.0, y as f64, fw, 1.0),
            &Paint::Solid(Color::rgb(
                m(top.r, bottom.r),
                m(top.g, bottom.g),
                m(top.b, bottom.b),
            )),
        );
    }
    let chips: [(f64, f64, Color); 5] = [
        (0.14, 0.66, Color::rgb(255, 209, 71)),
        (0.34, 0.56, Color::rgb(90, 220, 140)),
        (0.54, 0.7, Color::rgb(230, 60, 200)),
        (0.72, 0.54, Color::rgb(60, 220, 230)),
        (0.87, 0.66, Color::rgb(255, 110, 60)),
    ];
    for (fx, fy, c) in chips {
        let r = Rect::centred(fw * fx, fh * fy, fw * 0.1, fw * 0.1);
        cv.fill_round_rect(r, fw * 0.012, &Paint::Solid(c));
        cv.stroke_round_rect(
            r,
            fw * 0.012,
            3.0,
            &Paint::Solid(Color::rgba(255, 255, 255, 90)),
        );
    }
    cv.save_png(path).unwrap();
}

/// Three depth planes of a city at dusk: a sky (opaque), distant buildings and
/// nearer buildings (transparent above their skylines). `Content::Parallax`
/// derives each plane's own drift from one camera move plus the plane's depth.
fn draw_parallax(dir: &Path) {
    let (w, h) = (2400u32, 1350u32);
    let (fw, fh) = (w as f64, h as f64);

    // Sky.
    let mut sky = Canvas::new(w, h).unwrap();
    for y in 0..h {
        let t = y as f64 / fh;
        let top = Color::rgb(18, 22, 46);
        let hz = Color::rgb(214, 140, 96);
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t) as u8;
        sky.fill_rect(
            Rect::new(0.0, y as f64, fw, 1.0),
            &Paint::Solid(Color::rgb(m(top.r, hz.r), m(top.g, hz.g), m(top.b, hz.b))),
        );
    }
    let sun = Rect::centred(fw * 0.72, fh * 0.6, fw * 0.14, fw * 0.14);
    sky.fill_round_rect(
        sun,
        fw * 0.07,
        &Paint::Solid(Color::rgba(255, 226, 180, 235)),
    );
    sky.save_png(&dir.join("px-sky.png")).unwrap();

    // Distant buildings.
    let mut mid = Canvas::new(w, h).unwrap();
    let mut rnd = rng(0xb00b);
    let base = fh * 0.8;
    let mut x = 0.0;
    while x < fw {
        let bw = fw * (0.03 + rnd() * 0.035);
        let bh = fh * (0.1 + rnd() * 0.22);
        mid.fill_rect(
            Rect::new(x, base - bh, bw, bh + fh * 0.25),
            &Paint::Solid(Color::rgba(46, 52, 74, 255)),
        );
        if rnd() > 0.3 {
            let rows = (bh / (fh * 0.03)) as u32;
            for row in 0..rows {
                if rnd() > 0.55 {
                    let wy = base - bh + row as f64 * fh * 0.03 + fh * 0.008;
                    mid.fill_rect(
                        Rect::new(x + bw * 0.25, wy, bw * 0.5, fh * 0.012),
                        &Paint::Solid(Color::rgba(255, 205, 140, 200)),
                    );
                }
            }
        }
        x += bw + fw * 0.006;
    }
    mid.save_png(&dir.join("px-mid.png")).unwrap();

    // Nearer buildings + a chrome bar.
    let mut fg = Canvas::new(w, h).unwrap();
    let mut rnd = rng(0xf0f0);
    let base = fh * 0.95;
    let mut x = -fw * 0.05;
    while x < fw * 1.05 {
        let bw = fw * (0.05 + rnd() * 0.07);
        let bh = fh * (0.16 + rnd() * 0.3);
        fg.fill_rect(
            Rect::new(x, base - bh, bw, bh + fh * 0.2),
            &Paint::Solid(Color::rgba(14, 16, 26, 255)),
        );
        x += bw + fw * 0.012;
    }
    fg.fill_rect(
        Rect::new(0.0, fh * 0.965, fw, fh * 0.035),
        &Paint::Solid(Color::rgba(8, 9, 14, 255)),
    );
    fg.save_png(&dir.join("px-fg.png")).unwrap();
}

// ---- the films -----------------------------------------------------------

/// 1. Camera push over a still far larger than the frame (anarchist study #2).
fn camera() -> Film {
    let pw = 1200.0;
    let move_ = Camera::new()
        .to(0.0, Framing::point(pw * 0.3, pw * 0.62, pw * 0.55))
        .hold_until(0.4)
        .shot(
            Shot::new(2.6, Framing::point(pw * 0.3, pw * 0.62, pw * 0.13))
                .eased(Easing::InOutCubic),
        );
    Film::new(W, H, 30.0)
        .title("Camera push")
        .background(DEEP)
        .theme(Theme::dark())
        .open(
            Scene::new(3.0)
                .layer(Layer::camera("assets/poster.png", move_))
                .layer(heading("CAMERA PUSH").exiting(Motion::fade(0.3))),
        )
}

/// 2. Parallax: one flat image, three depth planes, one camera move
/// (anarchist study #1 — which the study only dared call "could *nearly* do").
fn parallax() -> Film {
    let move_ = Camera::new()
        .to(0.0, Framing::Whole)
        .shot(Shot::new(3.0, Framing::at(0.6, 0.55, 1350.0 * 0.62)).eased(Easing::InOutCubic));
    Film::new(W, H, 30.0)
        .title("Parallax")
        .background(Color::rgb(10, 12, 20))
        .theme(Theme::dark())
        .open(
            Scene::new(3.2)
                .layer(Layer::parallax(
                    vec![
                        ParallaxPlane::new("assets/px-sky.png", 0.2),
                        ParallaxPlane::new("assets/px-mid.png", 0.55),
                        ParallaxPlane::new("assets/px-fg.png", 1.2),
                    ],
                    move_,
                ))
                .layer(heading("PARALLAX")),
        )
}

/// 3. Colour grade, before and after, in one film — the same scene, cut from
/// ungraded to `Grade::documentary()` (anarchist study #7 said "cannot do
/// this"; it can now).
fn grade() -> Film {
    let scene = |label: &str, g: Option<Grade>| {
        let mut s = Scene::new(1.8)
            .layer(Layer::still("assets/grade-card.png").fit(Fit::Cover))
            .layer(
                Layer::lower_third(label)
                    .detail(if g.is_some() {
                        "Grade::documentary() — desaturated, contrast-pushed, vignetted"
                    } else {
                        "the raw composite, no grade"
                    })
                    .accent(AMBER)
                    .lasting(1.8),
            );
        if let Some(g) = g {
            s = s.grade(g);
        }
        s
    };
    Film::new(W, H, 30.0)
        .title("Colour grade")
        .background(Color::BLACK)
        .theme(Theme::dark())
        .open(scene("BEFORE", None))
        .then(
            Transition::wipe(0.5, Direction::Left),
            scene("AFTER", Some(Grade::documentary())),
        )
}

/// 4. A chart drawing itself in — a plotted function revealed left to right
/// (tvjunkie study, "cheapest wins near the counter and chart work").
fn chart() -> Film {
    let curve = Layer::chart_function("40*log(x+1)", 0.0, 80.0)
        .chart_axes("age (years)", "how fast a year feels")
        .chart_marker(Marker::VLine {
            x: 40.0,
            colour: Some(AMBER),
            label: Some("midpoint".into()),
            width: 3.0,
        })
        .frac(0.08, 0.2, 0.84, 0.72)
        .from(0.2);
    Film::new(W, H, 30.0)
        .title("Animated chart")
        .background(DEEP)
        .theme(Theme::dark())
        .open(
            Scene::new(3.2)
                .layer(Layer::title("It draws itself in").frac(0.06, 0.03, 0.88, 0.14))
                .layer(curve),
        )
}

/// 5. A bankroll counter ticking up — tabular figures, grouped thousands
/// (tvjunkie study #1).
fn counter() -> Film {
    Film::new(W, H, 30.0)
        .title("Counter")
        .background(Color::rgb(6, 20, 12))
        .theme(Theme::dark())
        .open(
            Scene::new(3.0)
                .layer(Layer::gradient(
                    vec![(0.0, Color::rgb(10, 34, 22)), (1.0, Color::rgb(4, 12, 9))],
                    108.0,
                ))
                .layer(
                    Layer::counter(0.0, 128_540.0, 2.4)
                        .prefix("$")
                        .frac(0.05, 0.3, 0.9, 0.4)
                        .from(0.3),
                )
                .layer(
                    Layer::text("BANKROLL")
                        .styled(
                            TextStyle::default()
                                .family("Montserrat")
                                .size(34.0)
                                .weight(700)
                                .tracking(0.32)
                                .colour(Color::rgb(120, 230, 170))
                                .centred(),
                        )
                        .frac(0.1, 0.2, 0.8, 0.1),
                ),
        )
}

/// 6. Kinetic captions — words landing one at a time with a spring overshoot
/// (anarchist study #3, tvjunkie study #2, youcut effect presets).
fn captions() -> Film {
    let line = |t: &str, y: f64, size: f64, colour: Color, from: f64| {
        Layer::text(t)
            .styled(
                TextStyle::default()
                    .family("Montserrat")
                    .size(size)
                    .weight(800)
                    .tracking(0.02)
                    .colour(colour)
                    .centred(),
            )
            .frac(0.05, y, 0.9, 0.24)
            .from(from)
            .entering(Motion::words(0.5, 0.09).timed(Timing::eased(Easing::OutBack)))
    };
    Film::new(W, H, 30.0)
        .title("Kinetic captions")
        .background(DEEP)
        .theme(Theme::dark())
        .open(
            Scene::new(3.0)
                .layer(line("Every word", 0.2, 78.0, Color::WHITE, 0.15))
                .layer(line("lands with", 0.42, 78.0, Color::WHITE, 0.6))
                .layer(line("a snap.", 0.64, 90.0, AMBER, 1.05)),
        )
}

/// 7. A transition — the cross-blur dissolve (youcut study flagged it as
/// "added but untested"), between two cards, then a wipe, so the motion of the
/// cut is the subject.
fn transitions() -> Film {
    let card = |word: &str, bg: Color, fg: Color| {
        Scene::new(1.5).layer(Layer::solid(bg)).layer(
            Layer::text(word)
                .styled(
                    TextStyle::default()
                        .family("Montserrat")
                        .size(96.0)
                        .weight(800)
                        .tracking(0.06)
                        .colour(fg)
                        .centred(),
                )
                .frac(0.05, 0.36, 0.9, 0.3),
        )
    };
    Film::new(W, H, 30.0)
        .title("Transitions")
        .background(DEEP)
        .theme(Theme::dark())
        .open(card("CROSS", Color::rgb(18, 24, 46), CYAN))
        .then(
            Transition::cross_blur(0.7),
            card("BLUR", Color::rgb(40, 16, 40), AMBER),
        )
        .then(
            Transition::wipe(0.6, Direction::Left),
            card("WIPE", Color::rgb(12, 30, 24), Color::rgb(120, 230, 170)),
        )
}

/// 8. A callout — a ring drawn at a target, a connected label naming it
/// (anarchist study #4).
fn callouts() -> Film {
    Film::new(W, H, 30.0)
        .title("Callouts")
        .background(DEEP)
        .theme(Theme::dark())
        .open(
            Scene::new(3.0)
                .layer(Layer::camera(
                    "assets/poster.png",
                    Camera::hold(Framing::at(0.3, 0.62, 1200.0 * 0.45)),
                ))
                .layer(Layer::solid(Color::rgba(6, 8, 14, 90)))
                .layer(heading("CALLOUTS"))
                .layer(
                    Layer::callout("point at a thing", (0.3, 0.55), (0.55, 0.34))
                        .detail("and name it")
                        .accent(AMBER)
                        .from(0.5),
                ),
        )
}

fn films() -> Vec<(&'static str, Film)> {
    vec![
        ("camera", camera()),
        ("parallax", parallax()),
        ("grade", grade()),
        ("chart", chart()),
        ("counter", counter()),
        ("captions", captions()),
        ("transitions", transitions()),
        ("callouts", callouts()),
    ]
}

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from("examples/gallery");
    let assets = root.join("assets");
    std::fs::create_dir_all(&assets)?;

    eprintln!("drawing procedural stills into {}…", assets.display());
    draw_poster(&assets.join("poster.png"));
    draw_grade_card(&assets.join("grade-card.png"));
    draw_parallax(&assets);

    for (name, film) in films() {
        let errs = film.validate();
        if !errs.is_empty() {
            for e in &errs {
                eprintln!("error in {name}: {e}");
            }
            anyhow::bail!("{name} does not validate");
        }
        let path = root.join(format!("{name}.film.jsonc"));
        std::fs::write(&path, film.to_json()?)?;
        println!(
            "{}  — {:.2}s, {} frames at {}x{}",
            path.display(),
            film.duration().as_secs(),
            film.frame_count(),
            film.width,
            film.height
        );
    }
    Ok(())
}
