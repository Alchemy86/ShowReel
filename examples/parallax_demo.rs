//! The parallax shot: one flat "screenshot" split into depth planes, pushed
//! into with a single camera move — see `docs/anarchist-study.md` for the
//! technique this proves out and `Content::Parallax` (`src/layer.rs`) for the
//! primitive itself.
//!
//! Self-contained, like `showcase.rs`: it draws its own three depth planes
//! (a sky, a row of distant buildings, a row of nearer ones with a UI-style
//! chrome bar) rather than depending on a real screenshot on disk, so
//! `cargo run --example parallax_demo` works on a fresh clone.
//!
//! ```text
//! cargo run --release --example parallax_demo -- --out out/parallax.film.json
//! showreel render out/parallax.film.json -o out/parallax.mp4
//! ```

use showreel::prelude::*;

const CANVAS: (u32, u32) = (3200, 1800);

/// Plane 1 — background: sky gradient, a low sun, and a haze of far hills.
/// Fully opaque; this is what shows through everywhere the nearer planes
/// don't cover.
fn build_sky(path: &std::path::Path) {
    let (w, h) = CANVAS;
    let (fw, fh) = (w as f64, h as f64);
    let mut cv = Canvas::new(w, h).unwrap();
    for y in 0..h {
        let t = y as f64 / fh;
        let top = Color::rgb(18, 22, 46);
        let horizon = Color::rgb(214, 140, 96);
        let c = Color::rgb(
            (top.r as f64 + (horizon.r as f64 - top.r as f64) * t) as u8,
            (top.g as f64 + (horizon.g as f64 - top.g as f64) * t) as u8,
            (top.b as f64 + (horizon.b as f64 - top.b as f64) * t) as u8,
        );
        cv.fill_rect(Rect::new(0.0, y as f64, fw, 1.0), &Paint::Solid(c));
    }
    let sun = Rect::centred(fw * 0.72, fh * 0.62, fw * 0.16, fw * 0.16);
    cv.fill_round_rect(sun, fw * 0.08, &Paint::Solid(Color::rgba(255, 226, 180, 235)));
    // A haze of far, low-contrast hills along the horizon.
    let mut seed = 0x51de_u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / (u32::MAX as f64 / 2.0)
    };
    let base_y = fh * 0.7;
    let mut x = 0.0;
    while x < fw {
        let hw = fw * (0.06 + rnd() * 0.06);
        let hh = fh * (0.03 + rnd() * 0.05);
        let hill = Rect::new(x, base_y - hh, hw * 1.4, hh * 2.0);
        cv.fill_round_rect(hill, hh, &Paint::Solid(Color::rgba(90, 100, 130, 110)));
        x += hw;
    }
    cv.save_png(path).unwrap();
}

/// Plane 2 — a row of distant building silhouettes. Transparent above the
/// skyline, opaque below it, so the sky shows through everywhere but the
/// buildings themselves.
fn build_midground(path: &std::path::Path) {
    let (w, h) = CANVAS;
    let (fw, fh) = (w as f64, h as f64);
    let mut cv = Canvas::new(w, h).unwrap();
    let mut seed = 0xb00b_u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / (u32::MAX as f64 / 2.0)
    };
    let base = fh * 0.82;
    let mut x = 0.0;
    let colour = Color::rgba(46, 52, 74, 255);
    while x < fw {
        let bw = fw * (0.03 + rnd() * 0.035);
        let bh = fh * (0.10 + rnd() * 0.22);
        let r = Rect::new(x, base - bh, bw, bh + fh * 0.2); // run well past the bottom edge
        cv.fill_rect(r, &Paint::Solid(colour));
        // A few lit windows, so the silhouette isn't a flat block.
        if rnd() > 0.3 {
            let rows = (bh / (fh * 0.03)) as u32;
            for row in 0..rows {
                if rnd() > 0.55 {
                    let wy = base - bh + row as f64 * fh * 0.03 + fh * 0.008;
                    let wr = Rect::new(x + bw * 0.25, wy, bw * 0.5, fh * 0.012);
                    cv.fill_rect(wr, &Paint::Solid(Color::rgba(255, 205, 140, 200)));
                }
            }
        }
        x += bw + fw * 0.006;
    }
    cv.save_png(path).unwrap();
}

/// Plane 3 — nearer buildings, larger and closer to camera, plus a UI-style
/// chrome bar along the bottom edge — the "foreground evidence" a real
/// screenshot's own window frame would contribute.
fn build_foreground(path: &std::path::Path) {
    let (w, h) = CANVAS;
    let (fw, fh) = (w as f64, h as f64);
    let mut cv = Canvas::new(w, h).unwrap();
    let mut seed = 0xf0f0_u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / (u32::MAX as f64 / 2.0)
    };
    let base = fh * 0.95;
    let mut x = -fw * 0.05;
    let colour = Color::rgba(14, 16, 26, 255);
    while x < fw * 1.05 {
        let bw = fw * (0.05 + rnd() * 0.07);
        let bh = fh * (0.16 + rnd() * 0.30);
        let r = Rect::new(x, base - bh, bw, bh + fh * 0.2);
        cv.fill_rect(r, &Paint::Solid(colour));
        x += bw + fw * 0.012;
    }
    // A thin browser-chrome bar along the very bottom, the kind of foreground
    // UI element that sells "this was a screenshot."
    let bar = Rect::new(0.0, fh * 0.965, fw, fh * 0.035);
    cv.fill_rect(bar, &Paint::Solid(Color::rgba(8, 9, 14, 255)));
    cv.fill_round_rect(
        Rect::new(fw * 0.02, fh * 0.972, fw * 0.22, fh * 0.02),
        fh * 0.01,
        &Paint::Solid(Color::rgba(40, 44, 60, 255)),
    );
    cv.save_png(path).unwrap();
}

fn build(sky: &str, mid: &str, fg: &str) -> Film {
    let ch = CANVAS.1 as f64;

    // One camera move, authored exactly like a Still's — a slow push and pan
    // toward the sun, low over the skyline. `Content::Parallax` derives every
    // plane's own path from this one move and each plane's depth: the sky
    // barely drifts (0.2), the midground buildings follow it closely (0.55),
    // and the foreground buildings overshoot it (1.2) — exactly the "nearer
    // things move more" cue that reads as depth.
    let move_ = Camera::new()
        .to(0.0, Framing::Whole)
        .shot(Shot::new(7.0, Framing::at(0.62, 0.55, ch * 0.55)).eased(Easing::InOutCubic));

    let shot = Scene::new(7.5)
        .named("parallax push")
        .layer(Layer::parallax(
            vec![
                ParallaxPlane::new(sky, 0.2),
                ParallaxPlane::new(mid, 0.55),
                ParallaxPlane::new(fg, 1.2),
            ],
            move_,
        ))
        .layer(
            Layer::gradient(vec![(0.0, Color::rgba(0, 0, 0, 0)), (1.0, Color::rgba(0, 0, 0, 150))], 90.0)
                .frac(0.0, 0.55, 1.0, 0.45),
        )
        .layer(
            Layer::lower_third("One flat image, three depth planes")
                .detail("a single authored camera move, split by depth")
                .accent(Color::rgb(255, 209, 71))
                .from(0.6)
                .lasting(6.0),
        );

    Film::new(1920, 1080, 30.0)
        .title("ShowReel — the parallax shot")
        .background(Color::rgb(10, 12, 20))
        .theme(Theme::dark())
        .open(shot)
}

fn main() -> anyhow::Result<()> {
    let mut out = std::path::PathBuf::from("parallax.film.json");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" | "-o" => out = args.next().unwrap_or_default().into(),
            other => eprintln!("ignoring unknown argument {other:?}"),
        }
    }
    let dir = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    std::fs::create_dir_all(&dir)?;

    let (sky, mid, fg) =
        (dir.join("parallax-sky.png"), dir.join("parallax-mid.png"), dir.join("parallax-fg.png"));
    if !sky.exists() {
        eprintln!("drawing three {}x{} depth planes…", CANVAS.0, CANVAS.1);
        build_sky(&sky);
        build_midground(&mid);
        build_foreground(&fg);
    }

    let film = build("parallax-sky.png", "parallax-mid.png", "parallax-fg.png");
    let errs = film.validate();
    if !errs.is_empty() {
        for e in &errs {
            eprintln!("error: {e}");
        }
        anyhow::bail!("the film does not validate");
    }
    std::fs::write(&out, film.to_json()?)?;
    println!(
        "{} — {:.2}s, {} frames at {}x{}",
        out.display(),
        film.duration().as_secs(),
        film.frame_count(),
        film.width,
        film.height
    );
    Ok(())
}
