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
fn declared_plugin_panel_uses_the_shared_host_and_renders_in_both_themes() {
    use solidcraft_engine::plugin::ui::*;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        for (width, height) in [(1280.0, 720.0), (800.0, 600.0)] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.call("ui.set", json!({"dark":dark}));
            h.frames(3);
            let declaration = PanelSpec {
                id: "jobs".into(),
                title: "Machining jobs".into(),
                zones: vec![DockZone::Bottom],
                layout: Layout {
                    root: 0,
                    nodes: vec![
                        Node { widget: Widget::Group(vec![1, 2, 3]) },
                        Node { widget: Widget::Text("Plate / Setup 1".into()) },
                        Node { widget: Widget::Toolbar(vec![ButtonSpec { label: "Post setup".into(), icon: None, entry: "post".into() }]) },
                        Node {
                            widget: Widget::Table(TableSpec {
                                bind: "/jobs".into(),
                                row_id: "id".into(),
                                columns: vec![
                                    Column { key: "name".into(), label: "Operation".into(), unit: None },
                                    Column { key: "tool".into(), label: "Tool".into(), unit: None },
                                    Column { key: "state".into(), label: "State".into(), unit: None },
                                ],
                                select: SelectionMode::Single,
                                on_select: Some("select".into()),
                                on_edit: Some("edit".into()),
                            }),
                        },
                    ],
                },
            };
            let mut data = PanelData::default();
            let updates: Vec<_> = [
                ("0", "Face stock", "Face mill Ø 20", "Complete"),
                ("1", "Pocket", "End mill Ø 6", "Ready"),
                ("2", "Drill holes", "Drill Ø 3", "Skipped"),
            ]
            .into_iter()
            .flat_map(|(id, name, tool, state)| {
                [
                    Datum { path: format!("/jobs/{id}/id"), value: UiValue::Text(id.into()) },
                    Datum { path: format!("/jobs/{id}/name"), value: UiValue::Text(name.into()) },
                    Datum { path: format!("/jobs/{id}/tool"), value: UiValue::Text(tool.into()) },
                    Datum { path: format!("/jobs/{id}/state"), value: UiValue::Text(state.into()) },
                ]
            })
            .collect();
            data.set_data(&updates).unwrap();
            let folder = h.dir.join("machining");
            std::fs::create_dir_all(&folder).unwrap();
            let declarations = Declarations { ui_version: UI_VERSION, panels: vec![declaration], workspaces: Vec::new() };
            let entries: Vec<_> =
                ["post", "select", "edit"].into_iter().map(|name| json!({"name":name,"effect":"read","input_schema":{"type":"object"}})).collect();
            let manifest = json!({"id":"machining","name":"Machining","version":"1","capabilities":["commands@0.2","ui@0.1"],"runtime":{
                "module":"plugin.wasm","commands":["inspect.measure"],"entries":entries,
                "limits":{"fuel":1000000,"memory_mib":2,"deadline_ms":5000},"ui":declarations
            }});
            let (package, version) = solidcraft_engine::plugin::API.split_once('@').unwrap();
            // Render inert manifest declarations with seeded data; the worker regression runs the guest.
            let source = include_str!("../../craft-plugin-wasm/src/ui_fixture.wat")
                .replace("__PACKAGE__", package)
                .replace("__VERSION__", version)
                .replace("__COMMAND__", "inspect.measure")
                .replace("__COMMAND_LEN__", "15")
                .replace("__ARGS__", "{}")
                .replace("__ARGS_LEN__", "2");
            std::fs::write(folder.join("plugin.json"), manifest.to_string()).unwrap();
            std::fs::write(folder.join("plugin.wasm"), wat::parse_str(source).unwrap()).unwrap();
            h.app.session.plugin_state.plugins_dir = Some(h.dir.clone());
            solidcraft_engine::plugin::sync_panels(&mut h.app.session);
            h.app.session.plugin_state.panels.get_mut("machining").unwrap().data.insert("jobs".into(), data);
            h.ctx = egui::Context::default();
            solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
            solidcraft_theme::apply(&h.ctx);
            let mut textures = Textures::new();
            for _ in 0..8 {
                raster::paint_at(&mut h, &mut textures, Vec::new(), egui::vec2(width, height));
            }
            let frame = h.app.universal_panel_frame().unwrap();
            let panel = frame
                .layout
                .sections
                .iter()
                .find(|section| section.instance.0.starts_with("plugin."))
                .expect("plugin panel registered in the shared host");
            assert!(panel.outer.origin.y >= 0.0);
            let row = scenario::rect_of("plugin-panel:machining:jobs:row:0").expect("table rows are host widgets");
            assert!(h.ctx.viewport_rect().contains_rect(row));
            let output = raster::paint_at(&mut h, &mut textures, Vec::new(), egui::vec2(width, height));
            let image = raster::raster(&mut h, &textures, output);
            let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-docked-panels");
            std::fs::create_dir_all(&dir).unwrap();
            image.save(dir.join(format!("plugin-panel-{}-{width:.0}x{height:.0}.png", if dark { "dark" } else { "light" }))).unwrap();
            h.app.session.disabled_plugins.insert("machining".into());
            h.app.preferences.disabled_plugins.insert("machining".into());
            raster::paint_at(&mut h, &mut textures, Vec::new(), egui::vec2(width, height));
            assert!(
                !h.app.universal_panel_frame().unwrap().layout.sections.iter().any(|section| section.instance.0.starts_with("plugin.")),
                "disabled plugins stop presenting immediately"
            );
        }
    }
    solidcraft_theme::set_dark(previous);
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
    // The Plugins page now keeps global resource ceilings under Advanced (#18764).
    // Open that section before exercising the same visible, exact-integer controls.
    assert!(scenario::rect_of("prefs:plugins:advanced_limits").is_some());
    assert!(scenario::rect_of("dialog:control:Max plugin fuel").is_none());
    click(&mut h, &mut textures, "prefs:plugins:advanced_limits");
    raster::paint(&mut h, &mut textures, vec![]);
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
