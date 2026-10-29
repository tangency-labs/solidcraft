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
fn render_browser_uses_shared_rows_in_both_lenses_and_themes() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/render-browser-review");
    std::fs::create_dir_all(&dir).unwrap();
    for dark in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.session.run_script(&solidcraft_engine::sample::script()).unwrap();
        h.app.run("solid.box", json!({"corner":[60,0,0],"length":10,"width":10,"height":10,"body_name":"Detail block"})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Plate"],"appearance":"ABS - Red"})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Detail block"],"appearance":"ABS - Blue"})).unwrap();
        h.call("ui.resize", json!({"width":1440,"height":1100}));
        h.call("ui.set", json!({"dark":dark}));
        h.app
            .run(
                "render.settings.set",
                json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{
                    "lighting":{"environment":[0.2,0.2,0.2],"sun":{"direction":[1,-1,-2],"angular_radius_rad":0.1,"radiance":[2,2,2]},
                        "area_lights":[{"center":[10,20,30],"u":[4,0,0],"v":[0,-2,0],"radiance":[1,1,1]}]}
                }}}}),
            )
            .unwrap();
        h.app.run("render.settings.set", json!({"name":"Detail","set":{"job":{"method":"raster"}}})).unwrap();
        h.call("ui.workspace", json!({"workspace":"render"}));
        h.ctx = egui::Context::default();
        solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
        theme::apply(&h.ctx);
        let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
        for _ in 0..8 {
            raster::paint(&mut h, &mut textures, vec![]);
        }
        for lens in ["object", "look"] {
            if lens == "look" {
                let at = scenario::rect_of("render:tree:by-look").unwrap().center();
                // Capture texture uploads from the input frames too: the look rows may
                // introduce glyphs that were absent from the object lens.
                for pressed in [true, false] {
                    raster::paint(
                        &mut h,
                        &mut textures,
                        vec![
                            egui::Event::PointerMoved(at),
                            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                        ],
                    );
                }
            }
            for _ in 0..4 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            let output = raster::paint(&mut h, &mut textures, vec![]);
            for section in ["environment", "lights", "shots", "objects", "output"] {
                assert!(scenario::rect_of(&format!("render:tree:{section}")).is_some());
            }
            for action in ["render:tree:lights:add", "render:tree:light:0:solo", "render:tree:light:0:mute", "render:tree:shots:add"] {
                assert!(scenario::rect_of(action).is_some(), "{action}");
            }
            assert_eq!(theme::is_dark(), dark);
            assert_eq!(scenario::rect_of("render:tree:look:0").is_some(), lens == "look");
            let browser = scenario::rect_of("browser:card").unwrap();
            let screenshot = raster::raster(&mut h, &textures, output);
            let ppp = h.ctx.pixels_per_point();
            image::imageops::crop_imm(
                &screenshot,
                (browser.left() * ppp) as u32,
                (browser.top() * ppp) as u32,
                (browser.width() * ppp) as u32,
                (browser.height() * ppp) as u32,
            )
            .to_image()
            .save(dir.join(format!("{}-{lens}.png", if dark { "dark" } else { "light" })))
            .unwrap();
        }
    }
}
