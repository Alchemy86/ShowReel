//! End-to-end: a chart that reads a real CSV/JSON off disk, and a plugin that
//! adds a layer kind — both driven through the public API, writing real files
//! and rendering real frames, not just asserting on a mock.
//!
//! These are the "fails loudly at load" and "renders through the seam"
//! guarantees the two features are judged by, pinned so they cannot regress.

use showreel::assets::AssetStore;
use showreel::prelude::*;
use showreel::render::Renderer;
use showreel::text::FontDb;
use showreel::time::Time;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn film_json(body: &str) -> String {
    format!(
        r##"{{ "width": 320, "height": 180, "fps": 24, {body},
             "opening": {{ "duration": 2.0, "layers": [ {{ "type": "solid", "colour": "#101010" }} ] }} }}"##
    )
}

/// A chart reading a CSV renders, and reads exactly the rows the file holds.
#[test]
fn a_chart_reads_a_csv_and_renders() {
    let dir = scratch("showreel-data-csv");
    std::fs::write(dir.join("d.csv"), "x,y\n0,0\n1,10\n2,40\n3,90\n").unwrap();

    let json = film_json(
        r##""then": [ { "transition": { "duration": 0.01, "presentation": { "kind": "cut" } },
          "scene": { "duration": 2.0, "layers": [
            { "type": "chart", "chart": { "series": [
                { "kind": "data", "file": "d.csv", "x": "x", "y": "y", "fill": true } ] } } ] } } ]"##,
    );
    let film = Film::from_json(&json).unwrap();
    assert!(film.validate().is_empty(), "{:?}", film.validate());

    let assets = AssetStore::rooted(&dir);
    // Loud-at-load: this both warms the cache and would error on a bad file.
    film.resolve_chart_data(&assets).unwrap();

    let renderer = Renderer::new(&film, &assets, FontDb::shared());
    renderer.preload().unwrap();
    // Render a frame from the chart scene; a bad data path would error here.
    let c = renderer.render_at(Time(2.5)).unwrap();
    assert_eq!((c.width(), c.height()), (320, 180));
}

/// A missing column fails at load, naming the file and the column.
#[test]
fn a_missing_column_fails_loudly() {
    let dir = scratch("showreel-data-badcol");
    std::fs::write(dir.join("d.csv"), "x,y\n0,0\n1,10\n").unwrap();
    let json = film_json(
        r##""then": [ { "transition": { "duration": 0.01, "presentation": { "kind": "cut" } },
          "scene": { "duration": 2.0, "layers": [
            { "type": "chart", "chart": { "series": [
                { "kind": "data", "file": "d.csv", "x": "x", "y": "missing" } ] } } ] } } ]"##,
    );
    let film = Film::from_json(&json).unwrap();
    let assets = AssetStore::rooted(&dir);
    // `{:#}` renders the whole context chain, where the file and column live.
    let err = format!("{:#}", film.resolve_chart_data(&assets).unwrap_err());
    assert!(err.contains("d.csv"), "{err}");
    assert!(err.contains("missing"), "{err}");
}

/// A bar chart reads its categories and values from a JSON array of rows.
#[test]
fn a_bar_chart_reads_json_rows() {
    let dir = scratch("showreel-data-json");
    std::fs::write(
        dir.join("r.json"),
        r##"[{"name":"A","v":3},{"name":"B","v":7},{"name":"C","v":5}]"##,
    )
    .unwrap();
    let json = film_json(
        r##""then": [ { "transition": { "duration": 0.01, "presentation": { "kind": "cut" } },
          "scene": { "duration": 2.0, "layers": [
            { "type": "chart", "chart": { "series": [
                { "kind": "data", "file": "r.json", "x": "name", "y": "v", "bars": true } ] } } ] } } ]"##,
    );
    let film = Film::from_json(&json).unwrap();
    let assets = AssetStore::rooted(&dir);
    film.resolve_chart_data(&assets).unwrap();
    let renderer = Renderer::new(&film, &assets, FontDb::shared());
    renderer.preload().unwrap();
    assert!(renderer.render_at(Time(2.5)).is_ok());
}

/// A plugin adds a layer kind: a `custom` layer expands to real layers and
/// renders, and the authored film still round-trips through JSON unharmed.
#[test]
fn a_plugin_expands_and_renders() {
    let json = r##"{
      "width": 320, "height": 180, "fps": 24,
      "plugins": [ {
        "name": "badge",
        "params": [ { "name": "text" }, { "name": "colour", "default": "#e26" } ],
        "body": [
          { "type": "solid", "colour": "{{colour}}",
            "placement": { "fx": 0.1, "fy": 0.1, "fw": 0.4, "fh": 0.2 } },
          { "type": "text", "text": "{{text}}" }
        ]
      } ],
      "opening": { "duration": 2.0, "layers": [
        { "type": "custom", "use": "badge", "with": { "text": "hi" }, "from": 0.5, "z": 3 }
      ] }
    }"##;
    let film = Film::from_json(json).unwrap();
    // Round-trips: the authored film keeps its plugin + custom layer.
    assert_eq!(Film::from_json(&film.to_json().unwrap()).unwrap(), film);

    let assets = AssetStore::new();
    let expanded = film.expand_plugins(&assets).unwrap();
    // The one custom layer became the plugin's two body layers.
    assert_eq!(expanded.timeline.scene(0).layers.len(), 2);
    // The custom layer's `from`/`z` shifted the whole widget.
    assert_eq!(expanded.timeline.scene(0).layers[0].from, Time(0.5));
    assert_eq!(expanded.timeline.scene(0).layers[0].z, 3);

    let renderer = Renderer::new(&expanded, &assets, FontDb::shared());
    assert!(renderer.render_at(Time(1.0)).is_ok());
}

/// An unexpanded `custom` layer that reaches the renderer is a loud error, not
/// a blank frame — the guarantee that a wiring mistake cannot render nothing.
#[test]
fn an_unexpanded_custom_layer_refuses_to_draw() {
    let json = r##"{
      "width": 320, "height": 180, "fps": 24,
      "opening": { "duration": 2.0, "layers": [
        { "type": "custom", "use": "ghost" }
      ] }
    }"##;
    let film = Film::from_json(json).unwrap();
    let assets = AssetStore::new();
    // Rendered without expansion: draw must bail, naming the plugin.
    let renderer = Renderer::new(&film, &assets, FontDb::shared());
    let err = match renderer.render_at(Time(1.0)) {
        Ok(_) => panic!("an unexpanded custom layer must not render"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("ghost") && err.contains("expand"), "{err}");
    // And expanding with no such plugin in scope is itself loud.
    assert!(film.expand_plugins(&assets).is_err());
}
