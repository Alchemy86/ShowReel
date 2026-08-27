//! End-to-end: build a film that uses every kind of layer, render it, and
//! check the pipeline holds together.
//!
//! These deliberately touch no external asset, so they run anywhere. The
//! worked example in `examples/kanto_reel.rs` is the one that exercises real
//! media.

use showreel::prelude::*;

fn kitchen_sink() -> Film {
    let scene_a = Scene::new(2.0)
        .named("a")
        .layer(Layer::gradient(
            vec![(0.0, Color::rgb(30, 40, 70)), (1.0, Color::rgb(8, 10, 16))],
            110.0,
        ))
        .layer(
            Layer::title("A Title")
                .subtitle("and its standfirst")
                .entering(Motion::chars(0.4, 0.02)),
        )
        .layer(Layer::scrim().from(0.5));

    let scene_b = Scene::new(2.5)
        .named("b")
        .layer(Layer::solid(Color::rgb(24, 26, 34)))
        .layer(Layer::lower_third("Headline").detail("strapline").from(0.2))
        .layer(Layer::counter(0.0, 600.0, 1.0).label("runs").from(0.3))
        .layer(
            Layer::callout("There", (0.3, 0.4), (0.6, 0.4))
                .detail("a thing worth naming")
                .from(0.5),
        )
        .layer(Layer::text("Plain text, wrapped and fitted.").frac(0.1, 0.75, 0.5, 0.15).from(0.6));

    let scene_c = Scene::new(2.0)
        .named("c")
        .layer(Layer::gradient(vec![(0.0, Color::WHITE), (1.0, Color::rgb(120, 130, 150))], 0.0))
        .layer(Layer::pull_up((0.3, 0.3, 0.3, 0.3)).label("lifted").from(0.2));

    Film::new(320, 180, 30.0)
        .title("kitchen sink")
        .open(scene_a)
        .then(Transition::dissolve(0.4), scene_b)
        .then(Transition::wipe(0.4, Direction::Left), scene_c)
}

#[test]
fn a_film_using_every_layer_kind_renders() {
    let film = kitchen_sink();
    assert!(film.validate().is_empty(), "{:?}", film.validate());
    let store = AssetStore::new();
    let r = Renderer::new(&film, &store, FontDb::shared());
    let mut sink = Collect::default();
    let stats = r.render_all(&mut sink).unwrap();
    assert_eq!(stats.frames, film.frame_count());
    assert_eq!(sink.0.len() as u32, film.frame_count());
    // Every frame must have something on it — an all-transparent frame means a
    // layer silently failed.
    for (i, c) in sink.0.iter().enumerate() {
        let lit = c.as_ref().pixels().iter().filter(|p| p.alpha() > 0).count();
        assert!(lit > 0, "frame {i} is empty");
    }
}

#[test]
fn the_whole_pipeline_is_deterministic() {
    let film = kitchen_sink();
    let render = || {
        let store = AssetStore::new();
        let r = Renderer::new(&film, &store, FontDb::shared());
        let mut sink = Collect::default();
        r.render_all(&mut sink).unwrap();
        sink.0.iter().map(|c| c.data().to_vec()).collect::<Vec<_>>()
    };
    assert_eq!(render(), render(), "two renders of one description must agree");
}

#[test]
fn a_json_round_trip_renders_identically() {
    // The Rust builders and the JSON must be the same film, not merely similar.
    let film = kitchen_sink();
    let json = film.to_json().unwrap();
    let parsed = Film::from_json(&json).unwrap();
    assert_eq!(parsed, film);

    let frame_of = |f: &Film| {
        let store = AssetStore::new();
        Renderer::new(f, &store, FontDb::shared()).render_frame(40).unwrap().data().to_vec()
    };
    assert_eq!(frame_of(&film), frame_of(&parsed));
}

#[test]
fn a_scaled_film_still_renders_and_stays_encodable() {
    let film = showreel::scale::scale_film(&kitchen_sink(), 0.4);
    assert!(film.validate().is_empty(), "{:?}", film.validate());
    assert_eq!(film.width % 2, 0);
    assert_eq!(film.height % 2, 0);
    // Timing must survive scaling exactly, or a preview is not a preview.
    assert_eq!(film.frame_count(), kitchen_sink().frame_count());
    let store = AssetStore::new();
    let mut sink = Collect::default();
    Renderer::new(&film, &store, FontDb::shared()).render_all(&mut sink).unwrap();
    assert_eq!(sink.0[0].width(), film.width);
}

#[test]
fn a_contact_sheet_of_the_whole_film_is_produced() {
    let film = kitchen_sink();
    let store = AssetStore::new();
    let sheet = showreel::preview::contact_sheet(
        &film,
        &store,
        FontDb::shared(),
        Time::secs(0.5),
        4,
        160,
    )
    .unwrap();
    assert!(sheet.width() > 600 && sheet.height() > 200, "{}x{}", sheet.width(), sheet.height());
}

#[test]
fn an_invalid_film_is_reported_rather_than_rendered() {
    let bad = Film::new(320, 180, 30.0)
        .open(Scene::new(0.5).named("too short"))
        .then(Transition::dissolve(2.0), Scene::new(2.0));
    let errs = bad.validate();
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.contains("too short")), "{errs:?}");
}
