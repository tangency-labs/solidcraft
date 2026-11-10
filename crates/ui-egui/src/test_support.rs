//! Headless test fixtures. Callers retain frame counts, input and command parameters.
use crate::{Services, SolveApp, scenario::Harness};
use serde_json::{Value, json};
use solidcraft_engine::{Sel, Session};

// The generic frame and event helpers are the kit's; SolidCraft's tests use them under these names.
pub(crate) use crate::ui_kit::test_support::{each_shape, key, pointer, raw_frame as frame, text_shapes};

/// What SolidCraft installs into the shared crates before its first frame: its catalogs, its
/// icons, its tab table, its navigation slots and its built-in themes. A test that reads theme files or draws icons without an app or a
/// test context ([`crate::fonts::test_context`]) calls it first.
pub(crate) fn host_data() {
    crate::i18n::install();
    crate::icon_svg::install();
    crate::workspace::install_tabs();
    crate::navigation_slots::install();
    crate::theme::install_builtin_themes();
}

pub(crate) fn app_with_script(script: Value) -> SolveApp {
    let mut session = Session::default();
    session.run_script(&script).unwrap();
    SolveApp::new(session, Services::default())
}

pub(crate) fn sample_app() -> SolveApp {
    app_with_script(solidcraft_engine::sample::script())
}

pub(crate) fn run_all(app: &mut SolveApp, commands: Vec<(String, Value)>) {
    for (id, params) in commands {
        app.run(&id, params).unwrap();
    }
}

pub(crate) fn play(h: &mut Harness) {
    while !h.app.synthetic.is_empty() {
        h.frame();
    }
    h.frames(2);
}

pub(crate) fn click(h: &mut Harness, handle: &str) {
    assert!(crate::scenario::handle_at(handle).is_some(), "{handle} is not drawn: {:?}", crate::scenario::handles(""));
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.frames(3);
}

/// One component named Block, with its original occurrence free and the root active.
pub(crate) fn component_box() -> (Harness, u64, u64) {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
    let component = part["component"].as_u64().unwrap();
    let occurrence = part["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    (h, component, occurrence)
}

/// Named 20 × 20 × 10 boxes with A grounded; preserve the caller's command entry point.
pub(crate) fn grounded_boxes(parts: &[(&str, i32)], mut run: impl FnMut(&str, Value) -> Value) {
    for (name, x) in parts {
        run("component.activate", json!({"component":"root"}));
        run("component.create", json!({"name":name}));
        run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[x,0,0]}));
    }
    run("component.activate", json!({"component":"root"}));
    run("occurrence.ground", json!({"occurrence":"A:1","grounded":true}));
}

/// Text with its first outline selected; callers decide when to start tools and draw.
pub(crate) fn selected_text(text: &str, at: [i32; 2]) -> (Harness, String, String) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let result = h.app.run("sketch.text", json!({"text":text,"at":at,"height":10})).unwrap();
    let link = result["link"].as_str().unwrap().to_owned();
    let curve = result["curves"][0].as_str().unwrap().to_owned();
    h.app.session.selection = vec![Sel::SketchCurve { id: curve.clone(), sketch: None }];
    (h, link, curve)
}

/// SolidCraft's two-colour built-in themes (1-bit and 1-bit Dark): pure black and white, so a few
/// looks other themes tell apart by colour are told apart by text or shape instead.
pub(crate) fn is_two_colour(id: &str) -> bool {
    matches!(id, "1-bit" | "1-bit-dark")
}

/// A theme file with per-tab tables for SolidCraft's tabs (`tests/fixtures/theme-tabs.json`).
pub(crate) fn tab_fixture(base: &str) -> crate::theme_files::ThemeFile {
    let catalog: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/theme-tabs.json")).unwrap();
    crate::theme_files::from_value(&catalog[base]).unwrap()
}
