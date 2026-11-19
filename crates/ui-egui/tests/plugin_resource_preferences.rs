//! Exercise the visible plugin ceiling fields and rasterise their actual egui meshes.
#[path = "support/raster.rs"]
mod raster;

use egui::{ColorImage, Event, Key, Modifiers, PointerButton, TextureId};
use serde_json::json;
use solidcraft_ui_egui::scenario::{self, Harness};
use std::collections::HashMap;

type Textures = HashMap<TextureId, ColorImage>;
fn click(h: &mut Harness, textures: &mut Textures, target: &str) {
    let at = scenario::rect_of(target).unwrap_or_else(|| panic!("missing visible target {target}")).center();
    for pressed in [true, false] {
        raster::paint(
            h,
            textures,
            vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }],
        );
    }
}
fn replace(h: &mut Harness, textures: &mut Textures, target: &str, text: &str) {
    click(h, textures, target);
    let modifiers = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
    raster::paint(h, textures, vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers }]);
    raster::paint(h, textures, vec![Event::Text(text.into())]);
}

#[test]
fn visible_resource_fields_confirm_exact_integers_and_refuse_invalid_ok() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1100,"height":1500}));
    h.call("ui.prefs", json!({"language":"en","open":"Plugins"}));
    h.app.home.open = false;
    // A fresh context lets this painter receive the entire font atlas and icon uploads.
    h.ctx = egui::Context::default();
    solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
    solidcraft_theme::apply(&h.ctx);
    let mut textures = Textures::new();
    for _ in 0..8 {
        raster::paint(&mut h, &mut textures, vec![]);
    }
    let original = h.app.session.plugin_limits;
    let fuel = "dialog:control:Max plugin fuel";
    let memory = "dialog:control:Max plugin memory (MiB)";
    let calls = "dialog:control:Max plugin host calls";
    let deadline = "dialog:control:Max plugin guest time (ms)";
    for target in [fuel, memory, calls, deadline] {
        let rect = scenario::rect_of(target).unwrap();
        assert!(scenario::rect_of("prefs:page_body").unwrap().contains_rect(rect), "{target} must be fully visible");
    }
    replace(&mut h, &mut textures, fuel, &u64::MAX.to_string());
    replace(&mut h, &mut textures, memory, "512");
    replace(&mut h, &mut textures, calls, "2048");
    replace(&mut h, &mut textures, deadline, "120000");
    assert_eq!(h.app.session.plugin_limits, original, "typing alone does not raise a ceiling");
    raster::paint(&mut h, &mut textures, vec![]);
    click(&mut h, &mut textures, "prefs:apply");
    assert_eq!(h.app.session.plugin_limits.max_fuel, u64::MAX);
    assert_eq!(h.app.session.plugin_limits.max_memory_mib, 512);
    assert_eq!(h.app.session.plugin_limits.max_host_calls, 2048);
    assert_eq!(h.app.session.plugin_limits.max_deadline_ms, 120000);
    let out = raster::paint(&mut h, &mut textures, vec![]);
    let image = raster::raster(&mut h, &textures, out);
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-resource-preferences");
    std::fs::create_dir_all(&dir).unwrap();
    // Share only the settings and their explanation, never the user's plugin folder or list.
    let rect = scenario::rect_of("dialog:row:Max plugin fuel")
        .unwrap()
        .union(scenario::rect_of("dialog:row:Max plugin memory (MiB)").unwrap())
        .union(scenario::rect_of("dialog:row:Max plugin host calls").unwrap())
        .union(scenario::rect_of("dialog:row:Max plugin guest time (ms)").unwrap())
        .union(scenario::rect_of("dialog:hint:plugin_limits").unwrap())
        .expand(4.0)
        .intersect(h.ctx.viewport_rect());
    let ppp = h.ctx.pixels_per_point();
    image::imageops::crop_imm(
        &image,
        (rect.left() * ppp) as u32,
        (rect.top() * ppp) as u32,
        (rect.width() * ppp) as u32,
        (rect.height() * ppp) as u32,
    )
    .to_image()
    .save(dir.join("resource-ceilings-en.png"))
    .unwrap();
    for size in [egui::vec2(1280.0, 720.0), egui::vec2(900.0, 640.0)] {
        for _ in 0..8 {
            raster::paint_at(&mut h, &mut textures, vec![], size);
        }
        let page = scenario::rect_of("prefs:page_body").unwrap();
        for target in [fuel, memory, calls, deadline] {
            assert!(page.contains_rect(scenario::rect_of(target).unwrap()), "{target} must be visible at {size:?}");
        }
        let out = raster::paint_at(&mut h, &mut textures, vec![], size);
        raster::raster(&mut h, &textures, out).save(dir.join(format!("resource-ceilings-{}x{}.png", size.x, size.y))).unwrap();
    }
    // Restore the original form size before continuing the input interactions.
    for _ in 0..8 {
        raster::paint(&mut h, &mut textures, vec![]);
    }
    let applied = h.app.session.plugin_limits;
    for target in [fuel, calls, deadline] {
        replace(&mut h, &mut textures, target, "0");
        click(&mut h, &mut textures, "prefs:ok");
        assert_eq!(h.app.session.plugin_limits, applied);
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["dialog"], "Plugins");
        replace(&mut h, &mut textures, target, "256");
    }
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.app.session.plugin_limits, applied);
}
