//! Canonical generator for `examples/burst_demo.film.jsonc` — the same
//! `kanto_reel.rs` pattern: seven scenes' worth of computed headings and
//! timings is too much to hand-maintain without a drift guard, so this is
//! the source and the committed file is checked against it (`--check`),
//! the same as `kanto.film.jsonc`. Regenerate with:
//!   cargo run --release --example burst_demo -- -o examples/burst_demo.film.jsonc
//! then hand-restore the header comment (see the note on `--check` in
//! `main` — this generator has no way to reconstruct hand-written prose).
use showreel::color::Color;
use showreel::ease::Easing;
use showreel::layer::{BurstSpec, Layer};
use showreel::time::Time;
use showreel::timeline::{Film, Scene};
use showreel::transition::Transition;

const ASSETS: [&str; 6] =
    ["battle-1.mp4", "battle-2.mp4", "battle-3.mp4", "battle-4.mp4", "battle-5.mp4", "battle-6.mp4"];

fn label(text: &str) -> Layer {
    Layer::lower_third(text)
}

/// `visual_factor` is how much of each clip's own `over` actually reads as
/// "still on screen and moving" before the accelerating travel clears it —
/// empirically ~0.45 for `in-quad`, tuned by watching (see the demo film's
/// header comment); `linear` clears more slowly so it wants a higher factor.
/// `dur_override` bypasses the estimate entirely for a scene that needs
/// hand-tuning after a look.
fn scene(
    name: &str,
    visual_factor: f64,
    hold: f64,
    dur_override: Option<f64>,
    caption: &str,
    spec: BurstSpec,
    assets: &[&str],
) -> Scene {
    let burst_end =
        spec.stagger.as_secs() * (assets.len().max(1) - 1) as f64 + spec.over.as_secs() * visual_factor;
    let dur = dur_override.unwrap_or(burst_end + hold);
    let mut layers = Layer::burst(assets, &spec);
    layers.push(label(caption));
    Scene::new(dur).named(name).layers(layers)
}

fn main() -> anyhow::Result<()> {
    let base = BurstSpec::default();

    let linear = BurstSpec { easing: Easing::Linear, ..base };
    let no_jitter = BurstSpec { jitter_deg: 0.0, ..base };
    let heavy_jitter = BurstSpec { jitter_deg: 45.0, seed: 2, ..base };
    let slow_big = BurstSpec {
        distance: 950.0,
        over: Time::secs(2.2),
        stagger: Time::secs(0.18),
        scale_to: 2.6,
        ..base
    };
    let fast_tight = BurstSpec {
        distance: 480.0,
        over: Time::secs(0.6),
        stagger: Time::secs(0.05),
        scale_to: 1.5,
        ..base
    };
    let dense = BurstSpec { seed: 3, ..base };
    let dense_assets: Vec<&str> = ASSETS.iter().chain(ASSETS.iter()).copied().collect();

    let film = Film::new(1280, 720, 30.0)
        .title("ShowReel — a burst")
        .background(Color::rgb(6, 8, 16))
        .open(scene(
            "baseline",
            0.45,
            0.35,
            None,
            "BASELINE — in-quad easing, 14 deg jitter",
            base,
            &ASSETS,
        ))
        .then(
            Transition::cut(),
            scene(
                "linear",
                0.85,
                0.35,
                None,
                "LINEAR EASING — constant speed reads cheap",
                linear,
                &ASSETS,
            ),
        )
        .then(
            Transition::cut(),
            scene(
                "no-jitter",
                0.45,
                0.35,
                None,
                "NO JITTER — even fan reads mechanical",
                no_jitter,
                &ASSETS,
            ),
        )
        .then(
            Transition::cut(),
            scene(
                "heavy-jitter",
                0.55,
                0.35,
                None,
                "HEAVY JITTER (45 deg) — reads chaotic",
                heavy_jitter,
                &ASSETS,
            ),
        )
        .then(
            Transition::cut(),
            scene(
                "slow-big",
                0.5,
                0.4,
                None,
                "SLOW + BIG — distance 950, scale 2.6x, over 2.2s",
                slow_big,
                &ASSETS,
            ),
        )
        .then(
            Transition::cut(),
            scene(
                "fast-tight",
                0.5,
                0.3,
                None,
                "FAST + TIGHT — machine-gun stagger, over 0.6s",
                fast_tight,
                &ASSETS,
            ),
        )
        .then(
            Transition::cut(),
            scene(
                "dense",
                0.45,
                0.35,
                None,
                "DENSE — 12 clips, same tuning as baseline",
                dense,
                &dense_assets,
            ),
        );

    let errs = film.validate();
    if !errs.is_empty() {
        for e in &errs {
            eprintln!("error: {e}");
        }
        anyhow::bail!("the film does not validate");
    }

    let mut out = std::path::PathBuf::from("burst_demo.film.jsonc");
    let mut check = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" | "-o" => out = args.next().unwrap_or_default().into(),
            "--check" => check = true,
            other => eprintln!("ignoring unknown argument {other:?}"),
        }
    }

    if check {
        // Compares *parsed* films, not raw text — the hand-written header
        // comment this file can't reconstruct never trips this guard, and
        // it can't catch that comment going stale either. See kanto_reel.rs.
        let want = Film::from_json(&film.to_json()?)?;
        let got_text = std::fs::read_to_string(&out)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", out.display()))?;
        let got = Film::from_json(&got_text)
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", out.display()))?;
        if got != want {
            anyhow::bail!(
                "{} is out of step with burst_demo.rs — regenerate it:\n                     cargo run --release --example burst_demo -- -o {}\n                 (this overwrites the file and discards the hand-written header comment)",
                out.display(),
                out.display()
            );
        }
        println!("{} is in step with burst_demo.rs", out.display());
        return Ok(());
    }

    std::fs::write(&out, film.to_json()?)?;
    println!("{} written — regenerated from BurstSpec, header comment not included", out.display());
    Ok(())
}
