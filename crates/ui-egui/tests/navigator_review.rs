//! Local review pictures of the axes navigator, drawn in software with no window (not for the
//! PR). Run: `cargo test -p solidcraft-ui-egui --test navigator_review -- --ignored`.
#[path = "support/raster.rs"]
mod raster;

use egui::{ColorImage, Event, Pos2, Rect, TextureId, vec2};
use serde_json::json;
use solidcraft_ui_egui::scenario::{self, Harness};
use std::collections::HashMap;

const SIZE: egui::Vec2 = vec2(1280.0, 720.0);

fn frames(h: &mut Harness, textures: &mut HashMap<TextureId, ColorImage>, at: Option<Pos2>, n: usize) -> egui::FullOutput {
    let mut out = raster::paint_at(h, textures, at.map(Event::PointerMoved).into_iter().collect(), SIZE);
    for _ in 1..n {
        out = raster::paint_at(h, textures, vec![], SIZE);
    }
    out
}

#[test]
#[ignore = "writes review pictures; run by hand"]
fn navigator_review_pictures() {
    let dir = std::env::var("NAV_REVIEW_DIR").unwrap_or_else(|_| "/Users/Shared/axes-shots/headless".into());
    std::fs::create_dir_all(&dir).unwrap();
    let langs = ["en", "ar", "de", "he", "ja", "ko", "vi", "zh"];
    for lang in langs {
        // (name, view, what the pointer rests on)
        for (name, view, target) in [
            ("rest", "iso", "centre"),
            ("hover-x", "iso", "axis_view:x"),
            ("hover-y", "iso", "axis_view:y"),
            ("front-aligned", "front", "axis_view:-y"),
            ("top", "top", "centre"),
            ("roll-arrow", "iso", "navigator:control:clockwise"),
        ] {
            if lang != "en" && matches!(name, "rest" | "top" | "roll-arrow") {
                continue;
            }
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.session.run_script(&solidcraft_engine::sample::script()).unwrap();
            h.call("ui.prefs", json!({"navigator_style": "axes", "language": lang, "look_at_anim": false}));
            h.call("ui.resize", json!({"width": SIZE.x, "height": SIZE.y}));
            h.call("ui.view", json!({"view": view}));
            h.ctx = egui::Context::default();
            solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
            solidcraft_theme::apply(&h.ctx);
            let mut textures = HashMap::new();
            frames(&mut h, &mut textures, Some(egui::pos2(500.0, 450.0)), 8);
            let outer = scenario::rect_of("axis_view:outer").expect("the navigator is drawn");
            let at = if target == "centre" {
                outer.center() + vec2(6.0, -30.0)
            } else {
                scenario::handle_at(target).unwrap_or_else(|| panic!("{lang} {name}: no {target}"))
            };
            let out = frames(&mut h, &mut textures, Some(at), 6);
            let shot = raster::raster(&mut h, &textures, out);
            let ppp = shot.width() as f32 / SIZE.x;
            // The navigator with room for a label on either side.
            let crop = Rect::from_min_max(outer.min - vec2(200.0, 10.0), outer.max + vec2(200.0, 30.0))
                .intersect(Rect::from_min_size(Pos2::ZERO, SIZE));
            image::imageops::crop_imm(
                &shot,
                (crop.left() * ppp) as u32,
                (crop.top() * ppp) as u32,
                (crop.width() * ppp) as u32,
                (crop.height() * ppp) as u32,
            )
            .to_image()
            .save(format!("{dir}/{lang}-{name}.png"))
            .unwrap();
        }
    }
}

#[test]
#[ignore = "writes review pictures; run by hand"]
fn navigator_small_window_pictures() {
    let dir = std::env::var("NAV_REVIEW_DIR").unwrap_or_else(|_| "/Users/Shared/axes-shots/headless".into());
    std::fs::create_dir_all(&dir).unwrap();
    let size = vec2(640.0, 480.0);
    for (name, target) in [("small-hover-x", "axis_view:x"), ("small-roll-arrow", "navigator:control:clockwise")] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.session.run_script(&solidcraft_engine::sample::script()).unwrap();
        h.call("ui.prefs", json!({"navigator_style": "axes", "language": "en", "look_at_anim": false}));
        h.call("ui.resize", json!({"width": size.x, "height": size.y}));
        h.call("ui.view", json!({"view": "iso"}));
        h.ctx = egui::Context::default();
        solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
        solidcraft_theme::apply(&h.ctx);
        let mut textures = HashMap::new();
        let mut paint = |h: &mut Harness, at: Option<Pos2>, n: usize| {
            let mut out = raster::paint_at(h, &mut textures, at.map(Event::PointerMoved).into_iter().collect(), size);
            for _ in 1..n {
                out = raster::paint_at(h, &mut textures, vec![], size);
            }
            out
        };
        paint(&mut h, Some(egui::pos2(300.0, 300.0)), 8);
        let at = scenario::handle_at(target).unwrap_or_else(|| panic!("small window: no {target}"));
        let out = paint(&mut h, Some(at), 6);
        drop(paint);
        raster::raster(&mut h, &textures, out).save(format!("{dir}/{name}.png")).unwrap();
    }
}
