//! Looking at a film without rendering all of it.
//!
//! Rendering a whole film to judge timing is the miserable loop the brief
//! called out, and Remotion's answer is a browser Studio with a scrubber. We
//! cannot have that without a browser, so this module closes the gap from the
//! other end, with three cheap operations that answer the questions a scrubber
//! is actually used for:
//!
//! - **"what does it look like at 4.2 seconds?"** — [`still_at`], one frame,
//!   typically milliseconds;
//! - **"is the pacing right?"** — [`contact_sheet`], the whole film as a
//!   labelled grid of thumbnails, in one image;
//! - **"does the motion work?"** — a quarter-size pass over the real timeline
//!   via [`crate::scale::scale_film`], which is roughly 16x less pixel work.

use crate::assets::AssetStore;
use crate::canvas::Canvas;
use crate::color::{Color, Paint};
use crate::geom::Rect;
use crate::render::Renderer;
use crate::text::{FontDb, TextLayout, TextStyle};
use crate::theme::Theme;
use crate::time::Time;
use crate::timeline::Film;
use anyhow::Result;

/// One frame, at a wall-clock time in the film.
pub fn still_at(film: &Film, assets: &AssetStore, fonts: &FontDb, t: Time) -> Result<Canvas> {
    Renderer::new(film, assets, fonts).render_at(t)
}

/// A labelled grid of thumbnails covering the whole film.
///
/// Each cell carries its timecode and the name of the scene it came from, so a
/// mistimed beat is visible at a glance rather than by scrubbing.
pub fn contact_sheet(
    film: &Film,
    assets: &AssetStore,
    fonts: &FontDb,
    every: Time,
    columns: u32,
    thumb_width: u32,
) -> Result<Canvas> {
    let step = every.as_secs().max(1.0 / film.fps);
    let total = film.duration().as_secs();
    let times: Vec<f64> = {
        let n = ((total / step).floor() as usize).max(0) + 1;
        (0..n).map(|i| (i as f64 * step).min(total.max(0.0))).collect()
    };

    let k = thumb_width as f64 / film.width as f64;
    let thumb_h = ((film.height as f64 * k).round() as u32).max(1);
    let columns = columns.max(1);
    let rows = times.len().div_ceil(columns as usize) as u32;

    let gutter = (thumb_width as f64 * 0.035).round().max(6.0);
    let label_h = (thumb_width as f64 * 0.11).round().max(16.0);
    let sheet_w = columns * thumb_width + (columns + 1) * gutter as u32;
    let sheet_h = rows * (thumb_h + label_h as u32) + (rows + 1) * gutter as u32;

    let mut sheet = Canvas::filled(sheet_w, sheet_h, Color::rgb(16, 18, 22))?;
    // Preview thumbnails are rendered from a scaled *film*, not by shrinking
    // full-size frames: the point is that it is cheap.
    let small = crate::scale::scale_film(film, k);
    let renderer = Renderer::new(&small, assets, fonts);
    renderer.preload()?;

    let theme = Theme::dark();
    let label_style = TextStyle {
        size: (thumb_width as f64 * 0.055).max(10.0),
        weight: 600,
        tracking: 0.05,
        shadow: None,
        fill: Paint::Solid(Color::rgb(150, 158, 172)),
        ..theme.counter_label.clone()
    };
    let time_style = TextStyle {
        align: crate::text::Align::Left,
        fill: Paint::Solid(Color::rgb(238, 240, 245)),
        ..label_style.clone()
    };

    let placements = film.timeline.placements();
    for (i, t) in times.iter().enumerate() {
        let col = i as u32 % columns;
        let row = i as u32 / columns;
        let x = gutter + col as f64 * (thumb_width as f64 + gutter);
        let y = gutter + row as f64 * (thumb_h as f64 + label_h + gutter);

        let frame = renderer.render_at(Time(*t))?;
        sheet.draw_canvas(&frame, Rect::new(x, y, small.width as f64, small.height as f64), 1.0);
        sheet.stroke_round_rect(
            Rect::new(x, y, small.width as f64, small.height as f64),
            2.0,
            1.0,
            &Paint::Solid(Color::rgba(255, 255, 255, 40)),
        );

        let ly = y + thumb_h as f64 + label_h * 0.18;
        if let Some(l) = TextLayout::build(fonts, &timecode(*t, film.fps), &time_style, None) {
            crate::text::draw(&mut sheet, fonts, &l, &time_style, (x, ly), 1.0, None);
        }
        // Name the scene this thumbnail belongs to.
        let scene_name = placements
            .iter()
            .rev()
            .find(|p| *t >= p.start.as_secs())
            .map(|p| {
                film.timeline
                    .scene(p.index)
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("scene {}", p.index + 1))
            })
            .unwrap_or_default();
        if let Some(l) = TextLayout::build(fonts, &scene_name, &label_style, None) {
            let lx = x + thumb_width as f64 - l.width;
            crate::text::draw(&mut sheet, fonts, &l, &label_style, (lx, ly), 1.0, None);
        }
    }
    Ok(sheet)
}

/// `m:ss.cc`, which is short enough for a thumbnail label and precise enough
/// to type back into `showreel still --at`.
pub fn timecode(t: f64, _fps: f64) -> String {
    let m = (t / 60.0).floor() as u64;
    let s = t - m as f64 * 60.0;
    format!("{m}:{s:05.2}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Layer;
    use crate::timeline::Scene;
    use crate::transition::Transition;

    fn film() -> Film {
        Film::new(320, 180, 30.0)
            .open(Scene::new(2.0).named("open").layer(Layer::solid(Color::rgb(200, 40, 40))))
            .then(
                Transition::dissolve(0.5),
                Scene::new(2.0).named("close").layer(Layer::solid(Color::rgb(40, 40, 200))),
            )
    }

    #[test]
    fn timecodes_read_correctly() {
        assert_eq!(timecode(0.0, 30.0), "0:00.00");
        assert_eq!(timecode(75.5, 30.0), "1:15.50");
    }

    #[test]
    fn a_still_is_the_frame_at_that_time() {
        let f = film();
        let store = AssetStore::new();
        let c = still_at(&f, &store, FontDb::shared(), Time(0.5)).unwrap();
        assert_eq!((c.width(), c.height()), (320, 180));
        let p = c.as_ref().pixels()[100];
        assert!(p.red() > 150, "should be the opening scene");
    }

    #[test]
    fn a_contact_sheet_covers_the_whole_film() {
        let f = film();
        let store = AssetStore::new();
        // 3.5s of film sampled every 0.5s = 8 thumbnails, 4 columns = 2 rows.
        let sheet = contact_sheet(&f, &store, FontDb::shared(), Time(0.5), 4, 160).unwrap();
        assert!(sheet.width() > 160 * 4, "four columns plus gutters");
        assert!(sheet.height() > 90 * 2, "two rows plus labels");
        // It has actually drawn something from both scenes.
        let reds = sheet.as_ref().pixels().iter().filter(|p| p.red() > 150 && p.blue() < 80).count();
        let blues = sheet.as_ref().pixels().iter().filter(|p| p.blue() > 150 && p.red() < 80).count();
        assert!(reds > 1000 && blues > 1000, "reds {reds} blues {blues}");
    }

    #[test]
    fn a_sheet_of_a_one_frame_film_still_works() {
        let f = Film::new(64, 36, 30.0).open(Scene::new(0.05).layer(Layer::solid(Color::WHITE)));
        let store = AssetStore::new();
        let sheet = contact_sheet(&f, &store, FontDb::shared(), Time(1.0), 4, 64).unwrap();
        assert!(sheet.width() > 0 && sheet.height() > 0);
    }
}
