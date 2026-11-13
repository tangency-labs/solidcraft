//! User scenarios in the real UI, headless (see `solidcraft_ui_egui::scenario`). Each JSON file
//! under `tests/scenarios/` is one workflow done with clicks, keys and drags, and one test here:
//! `build.rs` makes a test per file, named after it. A scenario waiting for a fix starts with a
//! `{"pending": "why"}` step (its test is ignored with that reason). Adding a scenario is adding
//! its file.

use solidcraft_engine::geom::prelude::*;
use solidcraft_ui_egui::scenario::Harness;

fn run(name: &str) {
    let path = format!("{}/tests/scenarios/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let steps: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut h = Harness::new();
    if let Err(e) = h.run(&steps) {
        panic!("{name}: {e}");
    }
}

include!(concat!(env!("OUT_DIR"), "/scenario_tests.rs"));

/// A scenario file named by `SOLIDCRAFT_SCENARIO_FILE` (for trying one out):
/// `SOLIDCRAFT_SCENARIO_FILE=x.json cargo test --test scenarios adhoc -- --ignored --nocapture`.
#[test]
#[ignore]
fn adhoc() {
    let Ok(path) = solidcraft_engine::geom::env::var("SCENARIO_FILE").ok_or(()) else { return };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let steps: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut h = Harness::new();
    if let Err(e) = h.run(&steps) {
        panic!("{path}: {e}");
    }
}

#[test]
fn angle_dimension_both_interactive_workflows() {
    use serde_json::json;
    for preselect in [false, true] {
        let mut h = Harness::new();
        h.run(&json!([
            {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
            {"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,0],[40,0]]}}},
            {"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,0],[30,30]]}}},
            {"call":"ui.view","params":{"view":"top"}}
        ]))
        .unwrap();
        // Pick the lines away from MID snaps so this remains an angular workflow.
        let picks = json!([
            {"click":{"sketch":[10,0]}},
            {"click":{"sketch":[10,10]},"shift":true}
        ]);
        if preselect {
            h.run(&picks).unwrap();
            h.run(&json!([{ "key":"D" }])).unwrap();
        } else {
            h.run(&json!([{ "key":"D" }])).unwrap();
            h.run(&picks).unwrap();
        }
        h.run(&json!([
            {"click":{"sketch":[20,8]}},
            {"text":"45"}, {"key":"Enter"},
            {"expect":{"errors":0,"param":{"d1":std::f64::consts::FRAC_PI_4}}}
        ]))
        .unwrap();
        let sk = &h.app.session.model.state().sketches[0].sketch;
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Angle { .. })));
    }
}

/// The conic tool still places a curve in three clicks (start, end, apex), the way it always
/// has, defaulting to a parabola (`rho` 0.5). A further drag on the curve itself (not on one of
/// its three points) reads `rho` off where the drag lands between the chord's midpoint and the
/// apex, so an ellipse or a hyperbola is a drag away rather than a fourth click away. The apex
/// stays an ordinary, separately draggable point: dragging it slides the curve's tangent point
/// and leaves `rho` alone. Both drags are tried after leaving the tool with Escape, the way a
/// person reshapes anything else they have just drawn (the tool is sticky, placing more conics
/// on further clicks, the way the ellipse and slot tools already are too).
#[test]
fn dragging_a_conic_after_the_tool_sets_rho_and_its_apex_stays_separately_draggable() {
    use serde_json::json;
    let mut h = Harness::new();
    h.run(&json!([
        {"call": "engine.execute", "params": {"command": "sketch.create", "params": {"plane": "XY"}}},
        {"call": "ui.view", "params": {"view": "top"}},
        {"start": "sketch.conic"},
        {"click": {"sketch": [0, 0]}},
        {"click": {"sketch": [40, 0]}},
        {"click": {"sketch": [20, 20]}},
        {"key": "Escape"},
        {"expect": {"errors": 0}}
    ]))
    .unwrap();

    let sk = h.app.session.model.state().sketches[0].sketch.clone();
    let (ci, apex_idx, rho) = sk
        .curves
        .iter()
        .enumerate()
        .find_map(|(i, c)| match c.kind {
            solidcraft_engine::sketch::CurveKind::Conic { apex, rho, .. } => Some((i, apex, rho)),
            _ => None,
        })
        .expect("the three clicks created a conic");
    assert!((rho - 0.5).abs() < 1e-6, "default rho {rho}");
    let apex_before = sk.point(apex_idx).expect("apex point resolves");
    assert!(apex_before.dist(solidcore_record::Vec2::new(20.0, 20.0)) < 1e-6);

    // Grabbed on the curve itself, at its default-rho midpoint (20, 10): the chord's midpoint
    // is (20, 0), the apex (20, 20), so (20, 14) is 0.7 of the way from one to the other.
    h.run(&json!([
        {"drag": [{"sketch": [20, 10]}, {"sketch": [20, 14]}], "steps": 4},
        {"expect": {"errors": 0}}
    ]))
    .unwrap();
    let sk_after_rho_drag = h.app.session.model.state().sketches[0].sketch.clone();
    let rho_after = match sk_after_rho_drag.curves[ci].kind {
        solidcraft_engine::sketch::CurveKind::Conic { rho, .. } => rho,
        ref k => panic!("not a conic: {k:?}"),
    };
    assert!((rho_after - 0.7).abs() < 0.02, "rho after drag {rho_after}");
    let apex_after_rho_drag = sk_after_rho_drag.point(apex_idx).expect("apex point resolves");
    assert!(apex_before.dist(apex_after_rho_drag) < 1e-6, "the apex moved on a curve-body drag");

    // Grabbed on the apex point itself: it slides, and rho is left as the previous drag set it.
    h.run(&json!([
        {"drag": [{"sketch": [20, 20]}, {"sketch": [10, 30]}], "steps": 4},
        {"expect": {"errors": 0}}
    ]))
    .unwrap();
    let sk_after_apex_drag = h.app.session.model.state().sketches[0].sketch.clone();
    let apex_after = sk_after_apex_drag.point(apex_idx).expect("apex point still resolves");
    assert!(apex_before.dist(apex_after) > 1.0, "apex did not move: still at {apex_after:?}");
    let rho_final = match sk_after_apex_drag.curves[ci].kind {
        solidcraft_engine::sketch::CurveKind::Conic { rho, .. } => rho,
        ref k => panic!("not a conic: {k:?}"),
    };
    assert!((rho_final - 0.7).abs() < 0.02, "rho changed on an apex drag: {rho_final}");
    // The same curve, reshaped in place (not replaced): the id survives both drags.
    assert_eq!(sk_after_apex_drag.curves.get(ci).map(|c| &c.id), sk.curves.get(ci).map(|c| &c.id));
}

#[test]
fn home_tab_and_file_recent_open_designs() {
    use serde_json::json;
    let mut h = Harness::new();
    let path = h.dir.join("recent.solidcraft").to_string_lossy().into_owned();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"solid.box","params":{"length":10,"width":10,"height":10}}},
        {"call":"engine.execute","params":{"command":"file.save_as","params":{"path":path}}},
        {"click":{"handle":"tab:home"}},
        {"expect":{"home":true,"documents":1}},
        {"click":{"handle":"tab:design:0"}},
        {"expect":{"home":false,"bodies":1}},
        {"click":{"handle":"tab:home"}},
        {"click":{"handle":"home:recent:0"}},
        {"until":{"file_busy":false}},
        {"expect":{"home":false,"bodies":1,"documents":1}},
        {"click":{"handle":"appbar:folder"}},
        {"widget":"Open Recent"}
    ]))
    .unwrap();
    h.run(&json!([
        {"widget":path},
        {"until":{"file_busy":false}},
        {"expect":{"home":false,"bodies":1,"documents":1}}
    ]))
    .unwrap();
}

#[test]
fn sketch_toolbar_imports_use_file_picker() {
    use serde_json::json;
    let mut h = Harness::new();
    let path = h.dir.join("drawing.svg");
    std::fs::write(
        &path,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="20mm" viewBox="0 0 40 20"><path d="M 0 0 L 40 0 L 40 20 L 0 20 Z"/></svg>"#,
    )
    .unwrap();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
        {"choose_file":path},
        {"start":"sketch.insert_svg"},
        // The picked drawing is placed first (#2605): OK adds it.
        {"expect":{"dialog":"InsertDrawing","sketch_curves":{"own":0}}},
        {"key":"Enter"},
        {"expect":{"errors":0,"sketch_curves":{"own":4}}}
    ]))
    .unwrap();
}

#[test]
fn narrow_toolbar_reaches_collapsed_commands() {
    use serde_json::json;
    let mut h = Harness::new();
    h.run(&json!([
        // At 640 wide the ribbon still needs no » menu: Construct's three buttons (Plane, Axis
        // and Point) fold into the panel's own button.
        {"call":"ui.resize","params":{"width":640,"height":800}},
        {"expect":{"shown_handles":["collapsed:MODEL:CONSTRUCT"],"hidden_handles":["toolbar:overflow","button:construct.plane"]}},
        // Too narrow for every button: the panels go into the » menu.
        {"call":"ui.resize","params":{"width":440,"height":800}},
        {"expect":{"shown_handles":["toolbar:overflow"],"hidden_handles":["button:construct.plane","collapsed:MODEL:CONSTRUCT"]}},
        {"click":{"handle":"toolbar:overflow"}},
        {"widget":"CONSTRUCT"},
        {"expect":{"shown_handles":["toolbar:construct.plane"]}},
        {"click":{"handle":"toolbar:construct.plane"}},
        {"expect":{"dialog":"ConstructAuto"}}
    ]))
    .unwrap();
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn sketch_finish_plugin(h: &Harness, fail_after: bool) -> String {
    use serde_json::json;
    let command = "sketch.finish";
    let (package, version) = solidcraft_engine::plugin::API.rsplit_once('@').unwrap();
    let template =
        include_str!("../../engine/src/plugin/fixture.wat").replace("\r\n", "\n").replace("__PACKAGE__", package).replace("__VERSION__", version);
    let calls = "      call $box\n      call $box\n";
    assert!(template.contains(calls));
    let command_length = "      i32.const 9\n";
    let params_length = "      i32.const 33\n";
    let original_data = r#"(data (;1;) (i32.const 1024) "{\22length\22:2,\22width\22:3,\22height\22:4}")"#;
    assert!(template.contains(command_length) && template.contains(params_length) && template.contains(original_data));
    let body = if fail_after { "      call $box\n      unreachable\n" } else { "      call $box\n" };
    let wat = template
        .replacen(calls, body, 1)
        .replacen(command_length, &format!("      i32.const {}\n", command.len()), 1)
        .replacen(params_length, "      i32.const 2\n", 1)
        .replace(original_data, r#"(data (;1;) (i32.const 1024) "{}")"#)
        .replace(r#""solid.box""#, &format!("\"{command}\""));
    let dir = h.dir.join("sample");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("plugin.wasm"), wat::parse_str(wat).unwrap()).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        json!({"id":"sample","name":"Sketch exit","version":"1","capabilities":["commands@0.1"],"runtime":{
            "module":"plugin.wasm",
            "entries":[{"name":"make","effect":"edit","input_schema":{"type":"object"}}],
            "commands":[command]
        }})
        .to_string(),
    )
    .unwrap();
    dir.to_string_lossy().into_owned()
}

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn approved_plugin_sketch_finish_restores_saved_camera_only_after_success() {
    use serde_json::json;
    for fail_after in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.sketch_exit_view = solidcraft_ui_egui::prefs::SketchExitView::Previous;
        h.call("ui.view", json!({"view":"iso"}));
        h.settle();
        let before_sketch = h.app.cam;
        h.step(&json!({"start":"sketch.create"})).unwrap();
        h.step(&json!({"click":{"plane":"XY"}})).unwrap();
        h.settle();
        h.app.run("sketch.line", json!({"points":[[0,0],[30,10]]})).unwrap();
        h.app.start("sketch.line");
        assert!(h.app.tool.is_some());
        assert!(h.app.pre_sketch_cam.is_some());
        let open = h.app.session.active_sketch;
        let saved_view = h.app.pre_sketch_cam;
        let saved_component = h.app.pre_sketch_component.clone();
        let tool = h.app.tool.as_ref().unwrap().cmd;
        let camera = h.app.cam;
        let before = h.app.session.doc.to_json();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        let path = sketch_finish_plugin(&h, fail_after);
        let inspected = h.app.run("plugin.inspect", json!({"path":path})).unwrap();
        // call_queued enters offload_control; call() executes the direct
        // handler and does not exercise the accepted plugin worker result.
        let (reply, _) =
            h.call_queued("engine.execute", json!({"command":"plugin.run","params":{"path":path,"approval":inspected["approval"],"entry":"make"}}));
        assert_eq!(reply["ok"], !fail_after, "trap {fail_after}: {reply}");
        if fail_after {
            assert_eq!(h.app.session.active_sketch, open);
            assert_eq!(h.app.pre_sketch_cam, saved_view);
            assert_eq!(h.app.pre_sketch_component, saved_component);
            assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some(tool));
            assert_eq!(h.app.cam, camera);
            assert_eq!(h.app.session.doc.to_json(), before);
            assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
        } else {
            assert_eq!(h.app.session.active_sketch, None);
            assert!(h.app.tool.is_none());
            assert!(h.app.pre_sketch_cam.is_none());
            assert!(h.app.pre_sketch_component.is_none());
            assert_eq!(h.app.cam, before_sketch, "restore the view saved on interactive entry");
        }
    }
}
