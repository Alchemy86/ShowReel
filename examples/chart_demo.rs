//! Charts that draw themselves in — the animated-chart feature, shown three
//! ways and proven to compose with the rest of the crate.
//!
//! Self-contained like `grade_demo.rs`: it needs nothing on disk, builds a
//! short film in Rust, validates it, writes it out as a readable
//! `examples/chart.film.jsonc`, and drops a contact sheet to
//! `docs/stills/chart-demo-sheet.png` so the whole thing can be eyeballed at a
//! glance.
//!
//! ```text
//! cargo run --release --example chart_demo
//! # then render the video from the emitted film:
//! showreel render examples/chart.film.jsonc -o out/chart_demo.mp4
//! ```
//!
//! Three scenes:
//! 1. A **function** (`40·ln(x+1)`) drawing in left to right over its axis, a
//!    cyan→magenta gradient stroke filled underneath, and a reference marker at
//!    the midpoint — the treatment the captain's reference short uses.
//! 2. **Grouped bars**, two series across four quarters, growing as the reveal
//!    sweeps across them.
//! 3. The same curve, **composed** with everything else: a scene colour grade
//!    over it, a callout pointing into it, and a pull-up enlarging a corner of
//!    it — none of which know it is a chart.

use showreel::assets::AssetStore;
use showreel::prelude::*;
use showreel::text::FontDb;

const CANVAS: (u32, u32) = (1920, 1080);

fn cyan() -> Color {
    Color::rgb(56, 210, 220)
}
fn magenta() -> Color {
    Color::rgb(226, 96, 173)
}
fn green() -> Color {
    Color::rgb(120, 220, 140)
}

/// The cyan→magenta stroke the reference film uses along its curve.
fn spectrum() -> Paint {
    Paint::Linear { stops: vec![(0.0, cyan()), (1.0, magenta())], angle: 0.0 }
}

fn build() -> Film {
    // ---- Scene 1: a function drawing in --------------------------------
    let curve = Layer::chart(vec![Series::Function {
        expr: "40*log(x+1)".into(),
        samples: 240,
        style: LineStyle { paint: Some(spectrum()), width: 6.0, fill: true, dots: false },
    }])
    .chart_x(0.0, 80.0)
    .chart_axes("age (years)", "how fast a year feels")
    .chart_marker(Marker::VLine {
        x: 40.0,
        colour: Some(green()),
        label: Some("midpoint".into()),
        width: 3.0,
    })
    .drawing_in(3.0)
    .frac(0.10, 0.26, 0.82, 0.60);

    let scene1 = Scene::new(4.5)
        .layer(Layer::gradient(
            vec![(0.0, Color::rgb(14, 16, 26)), (1.0, Color::rgb(26, 20, 40))],
            90.0,
        ))
        .layer(
            Layer::title("Life, plotted")
                .subtitle("a function drawn on the film's own clock")
                .at(Anchor::Top)
                .lasting(4.5),
        )
        .layer(curve.lasting(4.5));

    // ---- Scene 2: grouped bars growing ---------------------------------
    let bars = Layer::chart(vec![
        Series::Bars {
            values: vec![32.0, 48.0, 61.0, 74.0],
            labels: vec!["Q1".into(), "Q2".into(), "Q3".into(), "Q4".into()],
            paint: Some(Paint::Solid(cyan())),
            name: Some("2023".into()),
        },
        Series::Bars {
            values: vec![40.0, 55.0, 70.0, 92.0],
            labels: vec![],
            paint: Some(Paint::Solid(magenta())),
            name: Some("2024".into()),
        },
    ])
    .chart_axes("quarter", "revenue (£k)")
    .drawing_in(2.8)
    .frac(0.10, 0.26, 0.82, 0.60);

    let scene2 = Scene::new(4.0)
        .layer(Layer::gradient(
            vec![(0.0, Color::rgb(14, 16, 26)), (1.0, Color::rgb(20, 28, 36))],
            90.0,
        ))
        .layer(
            Layer::title("Bars that grow")
                .subtitle("two series, revealed as the sweep crosses them")
                .at(Anchor::Top)
                .lasting(4.0),
        )
        .layer(bars.lasting(4.0));

    // ---- Scene 3: it composes ------------------------------------------
    // The same curve, static this time, with a grade over it, a callout into
    // it, and a pull-up enlarging a piece of it — proof the chart is just a
    // layer like any other.
    let curve_static = Layer::chart_static(vec![Series::Function {
        expr: "40*log(x+1)".into(),
        samples: 240,
        style: LineStyle { paint: Some(spectrum()), width: 6.0, fill: true, dots: false },
    }])
    .chart_x(0.0, 80.0)
    .chart_axes("age (years)", "how fast a year feels")
    .frac(0.10, 0.26, 0.82, 0.60);

    let scene3 = Scene::new(4.0)
        .grade(Grade::documentary())
        .layer(Layer::gradient(
            vec![(0.0, Color::rgb(14, 16, 26)), (1.0, Color::rgb(26, 20, 40))],
            90.0,
        ))
        .layer(
            Layer::title("And it composes")
                .subtitle("grade + callout + pull-up, none of them chart-aware")
                .at(Anchor::Top)
                .lasting(4.0),
        )
        .layer(curve_static.lasting(4.0))
        .layer(
            Layer::callout("steep early, then it flattens", (0.28, 0.46), (0.28, 0.80))
                .accent(green())
                .from(0.6)
                .lasting(3.4),
        )
        .layer(
            Layer::pull_up((0.13, 0.36, 0.16, 0.26))
                .label("the first years")
                .landing(0.60, 0.42, 0.32, 0.42)
                .from(1.4)
                .lasting(2.6),
        );

    Film::new(CANVAS.0, CANVAS.1, 30.0)
        .title("ShowReel — animated charts")
        .background(Color::rgb(10, 12, 20))
        .theme(Theme::dark())
        .open(scene1)
        .then(Transition::dissolve(0.5), scene2)
        .then(Transition::dissolve(0.5), scene3)
}

fn main() -> anyhow::Result<()> {
    let film = build();
    let errs = film.validate();
    if !errs.is_empty() {
        for e in &errs {
            eprintln!("error: {e}");
        }
        anyhow::bail!("the film does not validate");
    }

    let film_path = std::path::PathBuf::from("examples/chart.film.jsonc");
    std::fs::write(&film_path, film.to_json()?)?;
    println!("wrote {}", film_path.display());

    let assets = AssetStore::new();
    let fonts = FontDb::shared();

    let stills_dir = std::path::PathBuf::from("docs/stills");
    std::fs::create_dir_all(&stills_dir)?;
    let sheet_path = stills_dir.join("chart-demo-sheet.png");
    let sheet = showreel::preview::contact_sheet(&film, &assets, fonts, Time(0.75), 5, 480)?;
    sheet.save_png(&sheet_path)?;
    println!("wrote {}", sheet_path.display());

    Ok(())
}
