//! A self-contained tour of ShowReel, using no assets but its own.
//!
//! The worked example (`kanto_reel.rs`) is the better film, but it needs
//! pictures derived from a Game Boy cartridge, which belong to whoever owns
//! that cartridge and are not in this repository. This one generates
//! everything it needs — including the 25-megapixel still the camera moves
//! over — with ShowReel's own drawing API, so `./reel` works on a fresh clone
//! with nothing else on disk.
//!
//! ```text
//! cargo run --release --example showcase -- --out showcase.film.json
//! showreel render showcase.film.json -o showcase.mp4
//! ```

use showreel::prelude::*;

const AMBER: Color = Color::rgb(255, 209, 71);
const DEEP: Color = Color::rgb(7, 9, 14);

/// Draw a large poster for the camera to move over.
///
/// Deliberately detailed at every scale: fine hatching that only resolves when
/// the camera is close, blocks that only read as a pattern when it is far out.
/// A pull-back over a flat image proves nothing.
fn build_poster(path: &std::path::Path, w: u32, h: u32) -> anyhow::Result<()> {
    let mut cv = Canvas::filled(w, h, Color::rgb(11, 14, 20))?;
    let (fw, fh) = (w as f64, h as f64);

    // A coarse grid, visible from the widest shot.
    let cells = 24.0;
    let step = fw / cells;
    for i in 0..=cells as u32 {
        let x = i as f64 * step;
        cv.line(x, 0.0, x, fh, 3.0, &Paint::Solid(Color::rgba(120, 140, 180, 26)));
        cv.line(0.0, x * fh / fw, fw, x * fh / fw, 3.0, &Paint::Solid(Color::rgba(120, 140, 180, 26)));
    }

    // Blocks on the grid, each with fine internal hatching. The hatching is
    // the point: it is invisible in a wide shot and crisp at 1:1.
    let mut seed = 0x5eed_u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / (u32::MAX as f64 / 2.0)
    };
    for gy in 0..cells as u32 {
        for gx in 0..cells as u32 {
            if rnd() > 0.62 {
                continue;
            }
            let pad = step * (0.10 + rnd() * 0.16);
            let r = Rect::new(
                gx as f64 * step + pad,
                gy as f64 * (fh / cells) + pad,
                step - pad * 2.0,
                fh / cells - pad * 2.0,
            );
            let lit = rnd();
            let base = if lit > 0.86 { AMBER.opacity(0.5) } else { Color::rgba(150, 170, 205, 40) };
            cv.fill_round_rect(r, step * 0.05, &Paint::Solid(base));
            cv.stroke_round_rect(r, step * 0.05, 2.0, &Paint::Solid(Color::rgba(190, 210, 240, 60)));
            let rows = (6.0 + rnd() * 10.0) as u32;
            for k in 1..rows {
                let y = r.y + r.h * k as f64 / rows as f64;
                cv.line(r.x + 6.0, y, r.right() - 6.0, y, 1.5,
                        &Paint::Solid(Color::rgba(210, 225, 245, 34)));
            }
        }
    }

    // One amber marker, so there is something to push in on and pull up.
    let mark = Rect::centred(fw * 0.28, fh * 0.66, step * 0.62, step * 0.62);
    cv.fill_round_rect(mark, step * 0.1, &Paint::Solid(AMBER));
    cv.stroke_round_rect(mark.inset(-step * 0.22), step * 0.14, 5.0,
                         &Paint::Solid(AMBER.opacity(0.55)));
    cv.save_png(path)?;
    Ok(())
}

fn build(poster: &str, poster_w: f64, poster_h: f64) -> Film {
    let heading = |t: &str| {
        Layer::text(t).styled(
            TextStyle::default().family("Montserrat").size(30.0).weight(700)
                .tracking(0.16).colour(AMBER).centred(),
        )
    };

    // 1 — the title card.
    let title = Scene::new(5.0)
        .named("title")
        .layer(Layer::gradient(vec![(0.0, Color::rgb(20, 27, 48)), (1.0, DEEP)], 108.0))
        .layer(
            Layer::title("ShowReel")
                .subtitle("describe a film and render it")
                .entering(Motion::chars(0.55, 0.03)),
        );

    // 2 — the camera, over a still far larger than the frame.
    let camera = Scene::new(10.0)
        .named("the camera")
        .layer(Layer::camera(
            poster,
            Camera::new()
                .to(0.0, Framing::point(poster_w * 0.28, poster_h * 0.66, 200.0))
                .hold_until(1.0)
                .shot(Shot::new(8.6, Framing::point(poster_w * 0.28, poster_h * 0.66, poster_h))
                    .eased(Easing::InOutCubic)),
        ))
        .layer(heading("THE CAMERA").frac(0.1, 0.07, 0.8, 0.06).from(0.4)
            .entering(Motion::rise(0.5)).exiting(Motion::fade(0.4)))
        .layer(
            Layer::lower_third("Zoom, pan and hold")
                .detail("over a source far larger than the frame")
                .accent(AMBER)
                .from(1.4)
                .lasting(5.0),
        )
        .layer(
            Layer::counter(0.0, poster_w * poster_h / 1.0e6, 2.6)
                .decimals(1)
                .suffix(" MP")
                .label("source, this shot")
                .from(2.6)
                .exiting(Motion::fade(0.5)),
        );

    // 3 — typography and overlays, all at once.
    let words = Scene::new(8.0)
        .named("the words")
        .layer(Layer::still(poster).fit(Fit::Cover))
        .layer(Layer::solid(Color::rgba(5, 7, 12, 186)))
        .layer(heading("THE WORDS").frac(0.1, 0.07, 0.8, 0.06)
            .entering(Motion::rise(0.5)))
        .layer(
            Layer::text("Real shaping. Kerning, tracking in ems,\nand tabular figures that do not jitter.")
                .styled(TextStyle::default().size(46.0).weight(600).centred().line_height(1.35))
                .frac(0.12, 0.24, 0.76, 0.2)
                .from(0.5)
                .entering(Motion::words(0.5, 0.06)),
        )
        .layer(
            Layer::counter(0.0, 1_048_576.0, 3.0)
                .label("frames, and counting")
                .from(1.2),
        )
        .layer(
            Layer::lower_third("Lower thirds")
                .detail("a plate, an accent bar, and a strapline")
                .accent(AMBER)
                .from(2.0),
        )
        .layer(
            Layer::callout("Callouts", (0.30, 0.66), (0.44, 0.66))
                .detail("point at a thing and name it")
                .accent(AMBER)
                .from(3.4),
        );

    // 4 — the pull-up.
    let lift = Scene::new(6.0)
        .named("the pull-up")
        .layer(Layer::camera(poster, Camera::hold(Framing::At {
            fx: 0.28, fy: 0.66, height: poster_h * 0.22,
        })))
        .layer(heading("THE PULL-UP").frac(0.1, 0.07, 0.8, 0.06)
            .entering(Motion::rise(0.5)).z(30))
        .layer(
            Layer::pull_up((0.40, 0.36, 0.20, 0.30))
                .label("lift a piece of the frame, dim the rest, name it")
                .accent(AMBER)
                .from(0.9)
                .entering(Motion::new(MotionKind::Scale { from: 0.0 }, 1.1)
                    .timed(Timing::eased(Easing::InOutCubic)))
                .z(40),
        );

    // 5 — the end card.
    let end = Scene::new(4.6)
        .named("end card")
        .layer(Layer::gradient(vec![(0.0, Color::rgb(18, 24, 42)), (1.0, DEEP)], 108.0))
        .layer(
            Layer::title("ShowReel")
                .subtitle("timeline · camera · typography · transitions")
                .frac(0.1, 0.30, 0.8, 0.28)
                .entering(Motion::words(0.6, 0.07)),
        )
        .layer(
            Layer::text("Every frame is a pure function of one description.")
                .styled(TextStyle::default().family("Open Sans").size(28.0).weight(400)
                    .colour(Color::rgb(150, 160, 178)).centred())
                .frac(0.15, 0.62, 0.7, 0.1)
                .from(1.3)
                .entering(Motion::rise(0.6)),
        );

    Film::new(1920, 1080, 60.0)
        .title("ShowReel — a tour")
        .background(DEEP)
        .theme(Theme::dark())
        .open(title)
        .then(Transition::fade_black(0.7), camera)
        .then(Transition::wipe(0.8, Direction::Left), words)
        .then(Transition::iris(0.8), lift)
        .then(Transition::fade_black(0.8), end)
}

fn main() -> anyhow::Result<()> {
    let mut out = std::path::PathBuf::from("showcase.film.json");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" | "-o" => out = args.next().unwrap_or_default().into(),
            other => eprintln!("ignoring unknown argument {other:?}"),
        }
    }
    let dir = out.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    std::fs::create_dir_all(&dir)?;

    let (pw, ph) = (5000u32, 5000u32);
    let poster = dir.join("showcase-poster.png");
    if !poster.exists() {
        eprintln!("drawing a {pw}x{ph} poster for the camera to move over…");
        build_poster(&poster, pw, ph)?;
    }

    let film = build("showcase-poster.png", pw as f64, ph as f64);
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
    Ok(())
}
