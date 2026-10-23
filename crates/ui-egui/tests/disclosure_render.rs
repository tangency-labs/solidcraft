//! Actual app evidence for disclosure states, rendered from egui meshes without a GPU.
#[path = "support/raster.rs"]
mod raster;

use egui::{ColorImage, Rect, TextureId};
use serde_json::json;
use solidcraft_ui_egui::{
    scenario::{self, Harness},
    shell::LayoutMode,
    theme,
};
use std::collections::HashMap;

#[test]
fn appearance_and_browser_folds_keep_handles_and_render_both_states() {
    let previous = theme::is_dark();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/disclosure-review");
    std::fs::create_dir_all(&dir).unwrap();
    for dark in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1100,"height":1500}));
        h.call("ui.set", json!({"dark":dark}));
        h.app.ui.layout = LayoutMode::Floating;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Body1".into() }];
        h.app.start("appearance.assign");
        h.frames(3);
        for open in [true, false] {
            if !open {
                h.step(&json!({"click":{"handle":"dialog:collapse"}})).unwrap();
                h.step(&json!({"click":{"handle":"browser:header"}})).unwrap();
            }
            assert_eq!(h.app.dialog.as_ref().unwrap().collapsed, !open);
            assert_eq!(h.app.tree.panel_collapsed, !open);
            assert_eq!(scenario::handle_at("fold:Bodies").is_some(), open);
            assert_eq!(scenario::handle_at("appearance:search").is_some(), open);
            // Fresh context transfers all textures to the software renderer, including fonts.
            h.ctx = egui::Context::default();
            solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
            theme::apply(&h.ctx);
            let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
            for _ in 0..8 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            let output = raster::paint(&mut h, &mut textures, vec![]);
            let screenshot = raster::raster(&mut h, &textures, output);
            let mode = if dark { "dark" } else { "light" };
            let state = if open { "open" } else { "closed" };
            screenshot.save(dir.join(format!("{mode}-{state}.png"))).unwrap();
            for (name, rect) in
                [("appearance", scenario::rect_of("dialog:header").unwrap()), ("browser", scenario::rect_of("browser:header").unwrap())]
            {
                let bounds = rect.intersect(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(screenshot.width() as f32, screenshot.height() as f32)));
                image::imageops::crop_imm(&screenshot, bounds.left() as u32, bounds.top() as u32, bounds.width() as u32, bounds.height() as u32)
                    .to_image()
                    .save(dir.join(format!("{mode}-{state}-{name}.png")))
                    .unwrap();
            }
        }
    }
    theme::set_dark(previous);
}

#[test]
fn requirements_window_renders_a_status_list_without_external_parameters() {
    let previous = theme::is_dark();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/requirements-review");
    std::fs::create_dir_all(&dir).unwrap();
    for (dark, width, height) in [(false, 1400, 1000), (true, 1100, 700)] {
        let mut harness = Harness::new();
        harness.app.home.open = false;
        harness.call("ui.resize", json!({"width":width, "height":height}));
        harness.call("ui.set", json!({"dark":dark, "tab":"INSPECT"}));
        harness.app.services.issue_system = Some(std::sync::Arc::new(|_, _| panic!("Local requirements must not connect")));
        harness
            .app
            .run(
                "requirements.import",
                json!({"spec":{
                    "schema":"solidcraft-requirements/1",
                    "requirements":[{"id":"fit", "title":"Lid fits the enclosure", "description":"Check the assembly clearance."},
                        {"id":"edges", "title":"Exposed edges are smooth"}]
                }}),
            )
            .unwrap();
        harness.app.run("requirements.review", json!({"id":"fit", "status":"in_progress", "note":"Checking assembly clearance"})).unwrap();
        harness.frames(3);
        if width >= 1400 {
            assert!(scenario::handle_at("button:requirements.list").is_some());
            harness.step(&json!({"click":{"handle":"toolbar:requirements.list"}})).unwrap();
        } else {
            harness.app.start("requirements.list");
        }
        harness.frames(3);
        assert!(harness.widget("In progress  |  Lid fits the enclosure").is_some());
        assert!(harness.widget("Open  |  Exposed edges are smooth").is_some());
        harness.ctx = egui::Context::default();
        solidcraft_ui_egui::fonts::setup_fonts(&harness.ctx);
        theme::apply(&harness.ctx);
        let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
        for _ in 0..8 {
            raster::paint_sized(&mut harness, &mut textures, vec![], egui::vec2(width as f32, height as f32));
        }
        assert!(harness.app.session.doc.params.is_empty());
        let output = raster::paint_sized(&mut harness, &mut textures, vec![], egui::vec2(width as f32, height as f32));
        let screenshot = raster::raster(&mut harness, &textures, output);
        assert_eq!((screenshot.width(), screenshot.height()), (width, height));
        assert!(screenshot.pixels().filter(|pixel| pixel.0 != [255, 255, 255, 255]).count() > 10_000);
        let mode = if dark { "dark-compact" } else { "light" };
        screenshot.save(dir.join(format!("{mode}.png"))).unwrap();
    }
    theme::set_dark(previous);
}
