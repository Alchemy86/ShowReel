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

// ---------------------------------------------------------------------------
// Audio, end to end.
//
// These are the only tests here that shell out, so they skip rather than fail
// where ffmpeg is absent. The source is a tone ffmpeg synthesises on the spot:
// the file stays asset-free, and a tone is enough to prove a stream exists,
// survives the mix and reaches both outputs.
// ---------------------------------------------------------------------------

use showreel::audio::Audio;
use showreel::encode::{
    EncodeOptions, FfmpegSink, MobileOptions, ffmpeg_available, mobile_cut, mobile_path,
};
use showreel::render::Renderer;
use std::path::Path;
use std::process::Command;

/// Codec name of the first audio stream, or None if the file has none.
fn audio_stream(path: &Path) -> Option<String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0"])
        .args(["-show_entries", "stream=codec_name", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Mean volume in dBFS, via ffmpeg's volumedetect.
fn mean_volume_db(path: &Path) -> Option<f64> {
    let out = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "info", "-i"])
        .arg(path)
        .args(["-af", "volumedetect", "-f", "null", "-"])
        .output()
        .ok()?;
    let err = String::from_utf8_lossy(&out.stderr);
    err.lines()
        .find_map(|l| l.split("mean_volume:").nth(1))
        .and_then(|v| v.trim().split_whitespace().next())
        .and_then(|v| v.parse().ok())
}

fn tone(dir: &Path, name: &str, seconds: f64) -> std::path::PathBuf {
    let p = dir.join(name);
    let ok = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .args(["-ac", "2"])
        .arg(&p)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "could not synthesise a test tone");
    p
}

fn encode_with_audio(dir: &Path, film: &Film, tracks: Vec<showreel::audio::AudioInput>) -> std::path::PathBuf {
    let out = dir.join("with-audio.mp4");
    let store = AssetStore::new();
    let fonts = FontDb::shared();
    let opts = EncodeOptions::preview().with_audio(tracks);
    let mut sink =
        FfmpegSink::new(&out, film.width, film.height, film.fps, &opts, film.background).unwrap();
    Renderer::new(film, &store, fonts).render_all(&mut sink).unwrap();
    out
}

#[test]
fn a_film_with_a_track_reaches_both_the_master_and_the_mobile_cut_with_sound() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg is not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join("showreel-audio-e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let src = tone(&dir, "tone.wav", 3.0);

    let film = Film::new(64, 36, 10.0)
        .open(Scene::new(2.0).layer(Layer::solid(Color::rgb(20, 20, 20))));
    let track = Audio::track("tone.wav").fades(0.2, 0.5).resolve(&src, film.duration());
    let master = encode_with_audio(&dir, &film, vec![track]);

    assert_eq!(audio_stream(&master).as_deref(), Some("aac"), "the master must carry audio");

    // The regression this whole test exists for: the mobile cut used to be
    // built with `-an`, so a film with sound arrived on the phone silent.
    let mob = mobile_path(&master);
    mobile_cut(&master, &mob, &MobileOptions::default()).unwrap();
    assert_eq!(audio_stream(&mob).as_deref(), Some("aac"), "the mobile cut must carry audio too");

    // Present is not the same as audible: a muted stream is still a stream.
    let db = mean_volume_db(&mob).expect("volumedetect should report a level");
    assert!(db > -50.0, "the mobile cut is effectively silent at {db} dBFS");
    assert!(db < 0.0, "the mobile cut is clipping at {db} dBFS");

    // The track is shorter than nothing, but the film's length still rules.
    let dur = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(&master)
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
        .unwrap();
    assert!((dur - 2.0).abs() < 0.35, "the film should still be ~2s, got {dur}s");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_film_with_no_track_still_encodes_silent() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg is not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join("showreel-audio-e2e-silent");
    std::fs::create_dir_all(&dir).unwrap();
    let film = Film::new(64, 36, 10.0)
        .open(Scene::new(1.0).layer(Layer::solid(Color::rgb(20, 20, 20))));
    let master = encode_with_audio(&dir, &film, Vec::new());
    assert!(audio_stream(&master).is_none(), "a film with no tracks must have no audio stream");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_track_survives_the_json_round_trip_with_its_placement() {
    let film = Film::new(64, 36, 10.0)
        .open(Scene::new(2.0).layer(Layer::solid(Color::WHITE)))
        .sound(Audio::track("theme.wav").at(0.5).from(1.0).lasting(1.0).fades(0.1, 0.4).gain(0.8));
    let json = film.to_json().unwrap();
    let back = Film::from_json(&json).unwrap();
    assert_eq!(back, film);
    assert_eq!(back.audio.len(), 1);
    assert_eq!(back.audio[0].gain, 0.8);
}

// ---------------------------------------------------------------------------
// The committed film description.
//
// `examples/kanto.film.json` exists so that a film can be read and rendered
// without compiling anything. It is generated from `examples/kanto_reel.rs`,
// which is canonical; `kanto_reel --check` is the guard against the two
// drifting. What is checked *here* is the thing that would rot silently: that
// the committed file still parses against today's types and still validates.
// ---------------------------------------------------------------------------

#[test]
fn the_committed_film_description_still_loads_and_validates() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/kanto.film.json");
    let film = Film::load(&path).expect("the committed description must parse");
    let errs = film.validate();
    assert!(errs.is_empty(), "{errs:?}");

    // It is the whole film, not a fragment.
    assert_eq!(film.timeline.scene_count(), 5);
    assert!((film.duration().as_secs() - 34.7).abs() < 1e-6);

    // Every asset is a bare name, resolved by `-A/--assets`. An absolute path
    // here would make the file useless on anyone else's machine.
    let refs: Vec<String> = film
        .assets_used()
        .iter()
        .map(|u| match u {
            showreel::timeline::AssetUse::Still(a) => a.clone(),
            showreel::timeline::AssetUse::Clip { asset, .. } => asset.clone(),
        })
        .chain(film.audio_assets().iter().map(|s| s.to_string()))
        .collect();
    assert!(!refs.is_empty());
    for r in &refs {
        assert!(!r.starts_with('/'), "{r} is an absolute path");
        assert!(!r.contains(".."), "{r} escapes the assets root");
    }

    // And it carries its sound.
    assert_eq!(film.audio.len(), 2, "the reel is scored");
}
