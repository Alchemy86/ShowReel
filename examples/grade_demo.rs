//! Before/after for `Grade::documentary()` — see `src/grade.rs` and the "A
//! colour grade" section of README.md for what it does and what it costs.
//!
//! Self-contained like `showcase.rs` and `parallax_demo.rs`: it draws its own
//! colourful "evidence card" backdrop (a saturated gradient plus a few
//! accent chips, the same kind of flat, colourful source a `Content::Still`
//! or a `Content::Parallax` plane would be) rather than depending on
//! anything on disk, then renders the *same* scene once ungraded and once
//! with `.grade(Grade::documentary())` — nothing else about the film
//! changes, which is the point: the grade is a post-composite pass, not a
//! different render path.
//!
//! ```text
//! cargo run --release --example grade_demo
//! ```
//!
//! Writes `docs/stills/grade-before.png` and `docs/stills/grade-after.png`.

use showreel::assets::AssetStore;
use showreel::prelude::*;
use showreel::text::FontDb;

const CANVAS: (u32, u32) = (1920, 1080);

/// A colourful synthetic "evidence card": a saturated sky-to-ground
/// gradient, a few accent chips in different hues, and an amber highlight —
/// enough colour variance for desaturation, contrast and the vignette to all
/// read clearly in a still.
fn build_backdrop(path: &std::path::Path) {
    let (w, h) = CANVAS;
    let (fw, fh) = (w as f64, h as f64);
    let mut cv = Canvas::new(w, h).unwrap();
    for y in 0..h {
        let t = y as f64 / fh;
        let top = Color::rgb(30, 90, 210);
        let bottom = Color::rgb(210, 70, 60);
        let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        let c = Color::rgb(mix(top.r, bottom.r), mix(top.g, bottom.g), mix(top.b, bottom.b));
        cv.fill_rect(Rect::new(0.0, y as f64, fw, 1.0), &Paint::Solid(c));
    }
    let chips: [(f64, f64, Color); 5] = [
        (0.10, 0.72, Color::rgb(255, 209, 71)),
        (0.30, 0.62, Color::rgb(90, 220, 140)),
        (0.50, 0.78, Color::rgb(230, 60, 200)),
        (0.70, 0.60, Color::rgb(60, 220, 230)),
        (0.88, 0.74, Color::rgb(255, 110, 60)),
    ];
    for (fx, fy, c) in chips {
        let r = Rect::centred(fw * fx, fh * fy, fw * 0.10, fw * 0.10);
        cv.fill_round_rect(r, fw * 0.012, &Paint::Solid(c));
        cv.stroke_round_rect(r, fw * 0.012, 3.0, &Paint::Solid(Color::rgba(255, 255, 255, 90)));
    }
    cv.save_png(path).unwrap();
}

fn build(backdrop: &str, grade: Option<Grade>) -> Film {
    let mut spec = Film::new(CANVAS.0, CANVAS.1, 30.0)
        .title("ShowReel — colour grade")
        .background(Color::BLACK)
        .theme(Theme::dark());
    if let Some(g) = grade {
        spec = spec.grade(g);
    }
    spec.open(
        Scene::new(3.0)
            .layer(Layer::still(backdrop).fit(Fit::Cover))
            .layer(
                Layer::lower_third("Internet Documentary Style")
                    .detail("Grade::documentary() — desaturated, contrast-pushed, vignetted")
                    .accent(Color::rgb(255, 209, 71))
                    .lasting(3.0),
            ),
    )
}

fn main() -> anyhow::Result<()> {
    let out_dir = std::path::PathBuf::from("out");
    std::fs::create_dir_all(&out_dir)?;
    let backdrop = out_dir.join("grade-demo-backdrop.png");
    if !backdrop.exists() {
        build_backdrop(&backdrop);
    }
    let backdrop_str = backdrop.to_str().unwrap();

    let assets = AssetStore::new();
    let fonts = FontDb::shared();

    let before = build(backdrop_str, None);
    let after = build(backdrop_str, Some(Grade::documentary()));
    for f in [&before, &after] {
        let errs = f.validate();
        if !errs.is_empty() {
            for e in &errs {
                eprintln!("error: {e}");
            }
            anyhow::bail!("the film does not validate");
        }
    }

    let t = Time(1.5);
    let stills_dir = std::path::PathBuf::from("docs/stills");
    std::fs::create_dir_all(&stills_dir)?;
    let before_png = stills_dir.join("grade-before.png");
    let after_png = stills_dir.join("grade-after.png");
    showreel::preview::still_at(&before, &assets, fonts, t)?.save_png(&before_png)?;
    showreel::preview::still_at(&after, &assets, fonts, t)?.save_png(&after_png)?;
    println!("wrote {} and {}", before_png.display(), after_png.display());
    Ok(())
}
