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
            assert_eq!(h.app.preferences.browser_collapsed, !open);
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

#[test]
fn stand_up_memory_controls_apply_and_forget_with_visible_evidence() {
    use solidcraft_ui_egui::stand_up::Answer;
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/stand-up-memory-review");
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1400,"height":1200}));
    h.call("ui.prefs", json!({"open":"Design"}));
    h.frames(3);
    // Main's settings layout can put either row below the viewport. Reveal controls before
    // clicking, just as the current preference tests do, instead of relying on page height.
    let reveal = |h: &mut Harness, handles: &[&str]| {
        for _ in 0..32 {
            let page = scenario::rect_of("dialog:page_body").unwrap();
            if handles.iter().all(|name| scenario::handle_at(name).is_some_and(|at| page.shrink(theme::Tokens::get().space_16).contains(at))) {
                return;
            }
            h.call("ui.scroll", json!({"x":page.center().x,"y":page.center().y,"delta":-100}));
            h.frames(20);
        }
        panic!("preference controls were not revealed: {handles:?}");
    };
    reveal(&mut h, &["prefs:remember_stand_up"]);
    h.step(&json!({"click":{"handle":"prefs:remember_stand_up"}})).unwrap();
    assert!(!h.app.preferences.remember_stand_up);
    h.step(&json!({"click":{"handle":"prefs:remember_stand_up"}})).unwrap();
    assert!(h.app.preferences.remember_stand_up);
    reveal(&mut h, &["prefs:reuse_imports", "prefs:clear_imports"]);
    assert!(h.app.preferences.reuse_imports);
    h.step(&json!({"click":{"handle":"prefs:reuse_imports"}})).unwrap();
    assert!(!h.app.preferences.reuse_imports);
    h.step(&json!({"click":{"handle":"prefs:reuse_imports"}})).unwrap();
    assert!(h.app.preferences.reuse_imports);
    h.step(&json!({"click":{"handle":"prefs:clear_imports"}})).unwrap();
    let digest = "a".repeat(64);
    h.app.preferences.stand_up_answers.remember(digest.clone(), Answer::Leave);
    h.step(&json!({"widget":"Forget remembered answers"})).unwrap();
    assert_eq!(h.app.preferences.stand_up_answers.get(&digest), None);
    let capture = |h: &mut Harness, name: &str| {
        h.ctx = egui::Context::default();
        solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
        theme::apply(&h.ctx);
        let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
        for _ in 0..8 {
            raster::paint(h, &mut textures, vec![]);
        }
        for _ in 0..16 {
            let (Some(page), Some(at)) = (scenario::rect_of("dialog:page_body"), scenario::handle_at("prefs:clear_imports")) else { break };
            if page.contains(at) {
                break;
            }
            raster::paint(
                h,
                &mut textures,
                vec![
                    egui::Event::PointerMoved(page.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -100.0),
                        modifiers: egui::Modifiers::NONE,
                        phase: egui::TouchPhase::Move,
                    },
                ],
            );
            for _ in 0..20 {
                raster::paint(h, &mut textures, vec![]);
            }
        }
        let output = raster::paint(h, &mut textures, vec![]);
        raster::raster(h, &textures, output).save(dir.join(name)).unwrap();
    };
    capture(&mut h, "preferences.png");
    h.call("ui.prefs", json!({"button":"ok", "stand_up_unknown":"ask"}));
    let path = dir.join("plate.step").to_string_lossy().to_string();
    h.app.run("solid.box", json!({"length":10,"width":40,"height":5})).unwrap();
    h.app.run("file.export", json!({"path":path})).unwrap();
    let text = std::fs::read_to_string(&path).unwrap().replace("'SolidCraft','SolidCraft'", "'SolidCraft','Example CAD'");
    std::fs::write(&path, text).unwrap();
    h.app.open_path(&path);
    assert!(h.app.stand_up.is_some());
    capture(&mut h, "first-offer.png");
    h.step(&json!({"click":{"handle":"stand_up:stand"}})).unwrap();
    let stood = solidcraft_engine::view::bounds(&h.app.session);
    h.app.open_path(&path);
    assert!(h.app.stand_up.is_none());
    assert_eq!(solidcraft_engine::view::bounds(&h.app.session), stood);
    capture(&mut h, "reopened.png");
}
