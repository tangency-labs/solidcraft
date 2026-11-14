//! Headless test fixtures. Callers retain frame counts, input and command parameters.
use crate::{Services, SolveApp, scenario::Harness};
use serde_json::{Value, json};
use solidcraft_engine::{Sel, Session};

// The generic frame and event helpers are the kit's; SolidCraft's tests use them under these names.
pub(crate) use crate::ui_kit::test_support::{each_shape, key, pointer, raw_frame as frame, text_shapes};

/// What SolidCraft installs into the shared crates before its first frame: its catalogs, its
/// icons, its tab table, its navigation slots and its built-in themes. A test that reads theme files or draws icons without an app or a
/// test context ([`crate::fonts::test_context`]) calls it first.
/// The UI kit crate's sources (`crates/ui-kit/src`), which SolidCraft's source guards read beside
/// its own.
pub(crate) fn kit_src() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui-kit/src")
}

/// The kit's components and their tests, as paths in [`kit_src`]. The source guards hold them to
/// the rules of SolidCraft's own UI code (they are drawn from the kit's controls), not to the
/// kit's.
pub(crate) const KIT_COMPONENTS: &[&str] = &[
    "app_bar.rs",
    "app_bar_tests.rs",
    "blur.rs",
    "blur.wgsl",
    "blur_native.rs",
    "frame.rs",
    "history_bar.rs",
    "history_bar_tests.rs",
    "nav_bar.rs",
    "nav_bar_layout.rs",
    "nav_bar_tests.rs",
    "navigation_bar.rs",
    "prefs_window.rs",
    "prefs_window_tests.rs",
    "ribbon.rs",
    "ribbon_tests.rs",
    "start_page.rs",
    "theme_files.rs",
    "theme_options.rs",
    "view_compass.rs",
    "window_chrome",
    "window_chrome.rs",
];

/// Whether `path` (under [`kit_src`]) is kit code proper rather than one of its components.
pub(crate) fn in_kit(path: &std::path::Path) -> bool {
    path.strip_prefix(kit_src()).is_ok_and(|relative| !KIT_COMPONENTS.iter().any(|component| relative.starts_with(component)))
}

/// Every file of the kit crate's sources with the name the source guards give it: `ui_kit/<path>`
/// for kit code, the bare path for a component (the names they had in `ui-egui`).
pub(crate) fn kit_files() -> Vec<(std::path::PathBuf, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { walk(&path, out) } else { out.push(path) }
        }
    }
    let root = kit_src();
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let relative = if relative == "lib.rs" { "mod.rs".to_string() } else { relative };
            let name = if in_kit(&path) { format!("ui_kit/{relative}") } else { relative };
            (path, name)
        })
        .collect()
}

/// The file a source guard names (`ui_kit/<path>`, a component, or a SolidCraft file).
pub(crate) fn guard_file(name: &str) -> std::path::PathBuf {
    if let Some(kit) = name.strip_prefix("ui_kit/") {
        return kit_src().join(if kit == "mod.rs" { "lib.rs" } else { kit });
    }
    if KIT_COMPONENTS.iter().any(|component| name.starts_with(component)) {
        return kit_src().join(name);
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name)
}

pub(crate) fn host_data() {
    crate::i18n::install();
    solidcraft_look::install_tables();
    crate::workspace::install_tabs();
    crate::navigation_slots::install();
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

/// Reach a real dialog control with Tab rather than injecting a widget ID into focus memory.
pub(crate) fn dialog_tab_to(h: &mut Harness, handle: &str) {
    for _ in 0..96 {
        let at = crate::scenario::handle_at(handle).unwrap_or_else(|| panic!("missing dialog target {handle}"));
        if h.ctx.memory(|m| m.focused()).and_then(|id| h.ctx.read_response(id)).is_some_and(|r| {
            r.enabled()
                && r.rect.is_positive()
                && r.rect.contains(at)
                && h.app.viewport.dialog_rect.is_some_and(|dialog| dialog.contains(r.rect.center()))
        }) {
            return;
        }
        h.step(&json!({"key":"Tab"})).unwrap();
    }
    panic!("Tab did not reach {handle}");
}

/// Verify a whole native focus cycle and its exact reverse without changing picks or the model.
pub(crate) fn dialog_tab_cycle(h: &mut Harness) {
    let start = h.ctx.memory(|m| m.focused()).expect("start inside the dialog");
    let layer = h.ctx.read_response(start).unwrap().layer_id;
    let document = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let selection = h.app.session.selection.clone();
    let picks: Vec<_> = h.app.dialog.as_ref().unwrap().inputs.iter().map(|input| input.items.clone()).collect();
    let mut forward = vec![start];
    let mut wrapped = false;
    for _ in 0..96 {
        h.step(&json!({"key":"Tab"})).unwrap();
        let id = h.ctx.memory(|m| m.focused()).expect("Tab must always reach a control");
        let response = h.ctx.read_response(id).unwrap();
        assert_eq!(response.layer_id, layer, "Tab must not enter canvas, browser, toolbar or palette");
        assert!(response.enabled() && response.rect.is_positive());
        assert!(h.app.viewport.dialog_rect.unwrap().contains(response.rect.center()));
        assert_ne!(Some(response.rect), crate::scenario::rect_of("dialog:header"), "drag headers are not controls");
        if h.app.preferences.dialog_buttons == crate::ui_kit::DialogButtons::Bottom {
            assert!(!response.rect.contains(crate::scenario::handle_at("dialog:close").unwrap()), "Close is pointer-only");
        }
        if id == start {
            wrapped = true;
            break;
        }
        assert!(!forward.contains(&id), "focus must not get stuck in a smaller sub-cycle");
        forward.push(id);
    }
    assert!(wrapped && forward.len() > 2, "fields and actions must form a bounded cycle");
    for expected in forward.iter().rev() {
        h.step(&json!({"key":"Tab", "shift":true})).unwrap();
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(*expected), "Shift+Tab must exactly reverse the native order");
    }
    assert_eq!(h.app.session.doc.to_json(), document, "navigation must not commit preview geometry");
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.selection, selection);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs.iter().map(|input| input.items.clone()).collect::<Vec<_>>(), picks);
}
