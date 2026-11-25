//! A plugin dialog with section headings, a folding section, units that follow a choice and a help
//! link, rasterised from the actual egui meshes without a GPU (images in target/plugin-dialog-layout).
// The shared helper also has a fixed-size `paint` this test does not use.
#[allow(dead_code)]
#[path = "support/raster.rs"]
mod raster;

use egui::{ColorImage, TextureId};
use serde_json::{Value, json};
use solidcraft_ui_egui::scenario::{self, Harness};
use std::collections::HashMap;

/// A cabinet entry's inputs, written as JSON text so the properties keep their declared order;
/// `layout` adds the dialog layout annotations.
fn schema(layout: bool) -> String {
    let mut properties = vec![
        (
            "units",
            json!({"type":"string","title":"Units","default":"mm","oneOf":[{"const":"mm","title":"Millimetres"},{"const":"in","title":"Inches"}]}),
        ),
        ("width", json!({"type":"number","title":"Width","default":600,"exclusiveMinimum":0,"description":"Outside width (mm or in)"})),
        ("height", json!({"type":"number","title":"Height","default":720,"exclusiveMinimum":0,"description":"Outside height (mm or in)"})),
        (
            "joints",
            json!({"type":"string","title":"Joints","default":"dowels","oneOf":[{"const":"dowels","title":"Dowels"},{"const":"biscuits","title":"Biscuits"}]}),
        ),
        ("dowel_d", json!({"type":"number","title":"Dowel diameter","default":8,"exclusiveMinimum":0,"description":"(mm or in)"})),
        ("drawers", json!({"type":"integer","title":"Drawers","default":2,"minimum":0,"maximum":8})),
        ("drawer_height", json!({"type":"number","title":"Drawer height","default":140,"exclusiveMinimum":0,"description":"(mm or in)"})),
        ("labels", json!({"type":"boolean","title":"Part labels","default":true})),
    ];
    if layout {
        for (key, property) in &mut properties {
            let section = match *key {
                "units" => Some("Carcass"),
                "joints" => Some("Joinery"),
                "drawers" => Some("Drawers"),
                "labels" => Some(""),
                _ => None,
            };
            if let Some(section) = section {
                property["x-solidcraft-section"] = json!(section);
            }
            if property["type"] == "number" {
                property["x-solidcraft-unit-from"] = json!("units");
                property.as_object_mut().unwrap().remove("description");
            }
        }
        properties[1].1["x-solidcraft-help"] = json!("https://example.com/woodwork/width");
    }
    let properties: Vec<String> = properties.iter().map(|(key, property)| format!("{}:{property}", json!(key))).collect();
    let sections = if layout { r#","x-solidcraft-sections":"collapsible""# } else { "" };
    format!(r#"{{"type":"object","x-solidcraft-field-order":"declared"{sections},"properties":{{{}}}}}"#, properties.join(","))
}

fn shot(plugins: &std::path::Path, name: &str, layout: bool, values: Value, fold: Option<&str>) {
    let folder = plugins.join("layoutdemo");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let manifest = format!(
        r#"{{"id":"layoutdemo","name":"Layout demo","version":"1.0.0","capabilities":["commands@0.1"],"runtime":{{"module":"plugin.wasm","commands":["solid.box"],"entries":[{{"name":"cabinet","effect":"edit","input_schema":{}}}]}}}}"#,
        schema(layout)
    );
    let (package, version) = solidcraft_engine::plugin::API.rsplit_once('@').unwrap();
    let wat = include_str!("../src/plugin_menu_fixture.wat").replace("PLUGIN_PACKAGE", package).replace("PLUGIN_VERSION", version);
    std::fs::write(folder.join("plugin.json"), manifest).unwrap();
    std::fs::write(folder.join("plugin.wasm"), wat::parse_str(&wat).unwrap()).unwrap();
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.set", json!({"dark":false}));
    let opened = h.call("ui.start", json!({"command":"plugin.edit","params":{"plugin":"layoutdemo","entry":"cabinet","params":values}}));
    assert_eq!(opened["ok"], true, "{opened}");
    h.frames(4);
    assert!(scenario::handle_at("plugin_dialog:field:width").is_some(), "the dialog is open");
    // A fresh context hands every texture, fonts included, to the software rasteriser.
    h.ctx = egui::Context::default();
    solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
    solidcraft_theme::apply(&h.ctx);
    let size = egui::vec2(1280.0, 720.0);
    let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
    for _ in 0..8 {
        raster::paint_at(&mut h, &mut textures, Vec::new(), size);
    }
    if let Some(section) = fold {
        // Folding is per context, so it is clicked in the context that is rasterised.
        let pos = scenario::handle_at(&format!("dialog:section:{section}")).expect("collapsible heading");
        let click = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        raster::paint_at(&mut h, &mut textures, vec![egui::Event::PointerMoved(pos), click(true)], size);
        raster::paint_at(&mut h, &mut textures, vec![click(false)], size);
        for _ in 0..3 {
            raster::paint_at(&mut h, &mut textures, Vec::new(), size);
        }
        assert!(scenario::handle_at("plugin_dialog:field:joints").is_none(), "the folded section's field is hidden");
    }
    let output = raster::paint_at(&mut h, &mut textures, Vec::new(), size);
    let drawn = !layout || (scenario::rect_of("dialog:section:Carcass").is_some() && scenario::handle_at("plugin_dialog:help:width").is_some());
    let image = raster::raster(&mut h, &textures, output);
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-dialog-layout");
    std::fs::create_dir_all(&dir).unwrap();
    image.save(dir.join(format!("{name}.png"))).unwrap();
    assert!(drawn, "{name}: the heading and the help link are drawn");
}

#[test]
fn plugin_dialog_layout_renders_headings_folds_units_and_help() {
    // Only this test's plugin: never the plugins installed for the person running the tests.
    let plugins = std::env::temp_dir().join(format!("solidcraft-plugin-dialog-layout-{}", std::process::id()));
    std::fs::create_dir_all(&plugins).unwrap();
    solidcraft_engine::plugin::configure_process_plugins(Some(plugins.clone()), false).unwrap();
    let previous = solidcraft_theme::is_dark();
    solidcraft_theme::set_dark(false);
    shot(&plugins, "before", false, json!({}), None);
    shot(&plugins, "after-mm", true, json!({}), None);
    shot(&plugins, "after-in-joinery-folded", true, json!({"units":"in"}), Some("Joinery"));
    let _ = std::fs::remove_dir_all(&plugins);
    solidcraft_theme::set_dark(previous);
}
