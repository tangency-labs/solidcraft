//! `macro.*`: recording writes Rhai, pause skips, replay rolls back a failure, typed params
//! are checked, and the example macros build a fixture plate, chamfer holes, or generate a gear.

use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Session;

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn error(s: &mut Session, id: &str, params: Value) -> String {
    s.execute(id, &params).map(|_| ()).err().map(|e| e.to_string()).expect("an error")
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("solidcraft-macro-{}-{}-{tag}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn play_example(s: &mut Session, name: &str, params: Value) -> Value {
    s.macros.loaded = Some(example(name));
    run(s, "macro.play", params)
}

#[test]
fn recording_captures_design_commands_and_pause_skips() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "macro.record", json!({"name": "Plate"}));
    run(&mut s, "select.set", json!({"items": []}));
    run(&mut s, "solid.sphere", json!({"radius": 5}));
    run(&mut s, "macro.pause", json!({}));
    run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 4}));
    run(&mut s, "macro.list", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "macro.resume", json!({}));
    run(&mut s, "parameters.add", json!({"name": "height", "expression": "3"}));
    let stop = run(&mut s, "macro.stop", json!({}));
    assert_eq!(stop["steps"], 3, "{stop}");
    let src = s.macros.loaded.as_ref().map(|m| m.source.as_str()).unwrap_or("");
    assert!(src.contains("run(\"select.set\""), "{src}");
    assert!(src.contains("run(\"solid.sphere\""), "{src}");
    assert!(src.contains("run(\"parameters.add\""), "{src}");
    assert!(!src.contains("\"command\""), "recording is Rhai, not JSON: {src}");
    assert!(error(&mut s, "macro.stop", json!({})).contains("nothing is being recorded"));
    assert!(error(&mut s, "macro.pause", json!({})).contains("nothing is being recorded"));
}

#[test]
fn recording_starts_afresh_and_refuses_two_at_once() {
    let mut s = Session::default();
    run(&mut s, "macro.record", json!({}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "macro.stop", json!({}));
    run(&mut s, "macro.record", json!({}));
    assert!(error(&mut s, "macro.record", json!({})).contains("already recording"));
    assert!(error(&mut s, "macro.play", json!({})).contains("stop the recording"));
    assert_eq!(run(&mut s, "macro.stop", json!({}))["steps"], 0);
}

#[test]
fn a_playback_is_one_undo_and_a_failure_rolls_back() {
    let mut s = Session::default();
    let r = run(
        &mut s,
        "macro.play",
        json!({"source": "run(\"solid.box\", #{ length: 10, width: 10, height: 10 });\nrun(\"solid.sphere\", #{ radius: 3 });\n"}),
    );
    assert_eq!(r["ran"], true);
    assert_eq!(s.doc.features.len(), 2);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.features.len(), 0);

    run(&mut s, "solid.sphere", json!({"radius": 1}));
    let before = s.doc.features.len();
    let err = error(
        &mut s,
        "macro.play",
        json!({"source": "run(\"solid.box\", #{ length: 10, width: 10, height: 10 });\nrun(\"sketch.create\", #{ plane: \"NOPE\" });\n"}),
    );
    assert!(err.contains("macro"), "{err}");
    assert_eq!(s.doc.features.len(), before, "the box the first step made is gone");
}

#[test]
fn playback_refuses_bad_steps_before_running_any() {
    let mut s = Session::default();
    let e = error(&mut s, "macro.play", json!({"source": "run(\"no.such.command\", #{});\n"}));
    assert!(e.contains("no.such.command") || e.contains("Unknown") || e.contains("macro"), "{e}");
    assert_eq!(s.doc.features.len(), 0);
    assert!(error(&mut s, "macro.play", json!({"source": "run(\"macro.record\", #{});\n"})).contains("macro"));
    assert!(error(&mut s, "macro.play", json!({})).contains("nothing has been recorded"));
}

#[test]
fn a_macro_asks_for_its_parameters_and_reads_an_earlier_step() {
    let mut s = Session::default();
    let src = "// @param length Length | 40 mm\n// @param width Width\n// @param thick Thickness | 5\nrun(\"solid.box\", #{ length: length, width: width, height: thick });\n";
    let dir = temp_dir("params");
    let path = dir.join("plate.rhai");
    std::fs::write(&path, src).unwrap();
    run(&mut s, "macro.open", json!({"path": path.display().to_string()}));
    let err = error(&mut s, "macro.play", json!({}));
    assert!(err.contains("width"), "{err}");
    let r = run(&mut s, "macro.play", json!({"params": {"width": "20 mm"}}));
    assert_eq!(r["ran"], true);
    assert!((volume(&mut s) - 40.0 * 20.0 * 5.0).abs() < 1e-3, "{}", volume(&mut s));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn find_holes_filters_by_size_feature_name_and_type() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    run(&mut s, "solid.hole", json!({"position": [10, 10, 10], "diameter": "6 mm", "name": "Bore"}));
    run(&mut s, "solid.hole", json!({"position": [30, 20, 10], "diameter": "12 mm", "name": "Big"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 8, "base": [20, 15, 10], "operation": "new", "name": "Boss"}));

    let small = run(&mut s, "inspect.find_holes", json!({"max_diameter": "8 mm"}));
    assert_eq!(small["count"], 1, "only the 6 mm hole: {small}");
    assert_eq!(small["holes"][0]["feature"]["name"], "Bore");
    let edges = small["holes"][0]["edges"].as_array().unwrap();
    assert!(edges.len() >= 2, "a closing edge on each side: {small}");
    assert_eq!(small["edges"].as_array().unwrap().len(), edges.len());

    let named = run(&mut s, "inspect.find_holes", json!({"feature": "Bore"}));
    assert_eq!(named["count"], 1, "{named}");
    assert_eq!(named["holes"][0]["feature"]["type"], "Hole");

    let typed = run(&mut s, "inspect.find_holes", json!({"type": "Hole", "max_diameter": "20 mm"}));
    assert_eq!(typed["count"], 2, "both hole features, not the boss: {typed}");
    let large = run(&mut s, "inspect.find_holes", json!({"min_diameter": "5 mm"}));
    assert_eq!(large["count"], 2, "both holes are wider than 5 mm: {large}");
    let only_big = run(&mut s, "inspect.find_holes", json!({"min_diameter": "8 mm"}));
    assert_eq!(only_big["count"], 1, "only the 12 mm hole: {only_big}");
    assert_eq!(only_big["holes"][0]["feature"]["name"], "Big");
    assert!(error(&mut s, "inspect.find_holes", json!({"feature": "NoSuch"})).contains("NoSuch"));
    assert!(error(&mut s, "inspect.find_holes", json!({})).contains("max_diameter"));

    s.macros.loaded = Some(super::macros::parse_rhai_macro("macro.open", r#"let holes = run("inspect.find_holes", #{ max_diameter: "8 mm", feature: "Bore" }); run("solid.chamfer", #{ edges: holes.edges, distance: "1 mm" });"#).unwrap());
    let before = volume(&mut s);
    let r = run(&mut s, "macro.play", json!({}));
    assert_eq!(r["ran"], true, "{r}");
    assert!(volume(&mut s) < before, "the chamfer cut material on both ends");
}

#[test]
fn saved_macros_write_and_hostile_names_are_refused() {
    let dir = temp_dir("save");
    let path = dir.join("box.rhai");
    let mut s = Session::default();
    run(&mut s, "macro.record", json!({}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "macro.stop", json!({}));
    let saved = run(&mut s, "macro.save", json!({"path": path.display().to_string()}));
    assert!(saved["steps"].as_u64().unwrap() >= 1, "{saved}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("run(\"solid.box\""), "{text}");
    for name in ["../evil.rhai", "a/b.rhai", "", "no extension"] {
        assert!(error(&mut s, "macro.save", json!({"name": name})).contains("name"), "{name}");
    }
    std::fs::write(&path, "").unwrap();
    assert!(
        error(&mut s, "macro.open", json!({"path": path.display().to_string()})).contains("source")
            || error(&mut s, "macro.open", json!({"path": path.display().to_string()})).contains("rhai")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn chamfer_of_no_edges_is_a_no_op() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let r = run(&mut s, "solid.chamfer", json!({"edges": [], "distance": "1 mm"}));
    assert_eq!(r["skipped"], true, "{r}");
    assert_eq!(s.doc.features.len(), 1);
}

#[test]
fn the_example_macros_run() {
    let mut s = Session::default();
    let r = play_example(&mut s, "setup_fixtureplate.rhai", json!({}));
    assert_eq!(r["ran"], true, "{r}");
    let info = run(&mut s, "document.inspect", json!({}));
    let sketches = info["sketches"].as_array().unwrap();
    assert!(sketches.iter().any(|sk| sk["name"] == "clearance_hole"), "{info}");

    let r = play_example(&mut s, "example_fixtureplate.rhai", json!({"params": {"nx": 1, "ny": 1}}));
    assert_eq!(r["ran"], true, "{r}");
    let found = run(&mut s, "inspect.find_holes", json!({"type": "Hole"}));
    assert_eq!(found["count"], 6, "six Ã˜8 holes: {found}");
    let v = volume(&mut s);
    let solid = 500.0 * 350.0 * 8.0;
    assert!(v < solid, "holes and the C cut remove material: {v} vs {solid}");
    assert!(v > solid * 0.7, "the tray is still mostly there: {v}");

    let mut holes = Session::default();
    run(&mut holes, "solid.box", json!({"length": 80, "width": 50, "height": 10, "name": "Plate"}));
    run(&mut holes, "solid.hole", json!({"position": [15, 15, 10], "diameter": "4 mm", "name": "Small"}));
    run(&mut holes, "solid.hole", json!({"position": [65, 15, 10], "diameter": "8 mm", "name": "Wide"}));
    run(&mut holes, "solid.hole", json!({"position": [40, 35, 10], "diameter": "12 mm", "name": "Big"}));
    let before = volume(&mut holes);
    let r = play_example(&mut holes, "chamfer_holes.rhai", json!({}));
    assert_eq!(r["ran"], true, "{r}");
    assert!(volume(&mut holes) < before, "the holes between 5 mm and 20 mm were chamfered");
    let tiny = run(&mut holes, "inspect.find_holes", json!({"feature": "Small"}));
    assert_eq!(tiny["count"], 1, "the 4 mm hole is still a hole: {tiny}");

    let mut gear = Session::default();
    let r = play_example(&mut gear, "gear_generator.rhai", json!({"params": {"teeth": 12, "gear_module": 2, "bore": 8, "thickness": 8, "pa": 20}}));
    assert_eq!(r["ran"], true, "{r}");
    let m = run(&mut gear, "inspect.measure", json!({}));
    assert_eq!(m["body_count"], 1, "{m}");
    let v = volume(&mut gear);
    let (ra, rf, bore_r, thick) = (14.0, 9.5, 4.0, 8.0);
    let max = std::f64::consts::PI * (ra * ra - bore_r * bore_r) * thick;
    let min = std::f64::consts::PI * (rf * rf - bore_r * bore_r) * thick;
    assert!(v > min && v < max, "involute blank {v} should sit between root {min} and tip {max}");

    let dir = temp_dir("stl");
    let step = dir.join("cube.step");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut cube = Session::default();
    run(&mut cube, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut cube, "file.export", json!({"path": step.display().to_string()}));
    let mut batch = Session::default();
    let r = play_example(
        &mut batch,
        "batch_export_stl.rhai",
        json!({"params": {"dir": dir.display().to_string(), "out_dir": out.display().to_string()}}),
    );
    assert_eq!(r["ran"], true, "{r}");
    assert!(out.join("cube.stl").is_file(), "missing STL in {}", out.display());
    std::fs::remove_dir_all(&dir).ok();
}

fn examples_dir() -> String {
    format!("{}/../../examples/macros", env!("CARGO_MANIFEST_DIR"))
}

fn example(name: &str) -> super::macros::Macro {
    let file = format!("{}/{}", examples_dir(), name);
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e}"));
    super::macros::parse_rhai_macro("macro.open", &text).unwrap()
}

#[test]
fn typed_params_are_checked_and_reach_rhai_as_their_kind() {
    let mut s = Session::default();
    let src = "// Box\n// @param n Count int | 2\n// @param side Side length length | 10 mm\n\
               run(\"solid.box\", #{ length: side, width: side, height: side });\n";
    // The length default reaches the command as an expression: a 10 mm box.
    let r = run(&mut s, "macro.play", json!({"source": src}));
    assert_eq!(r["ran"], true);
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
    // A bad int is refused before anything runs.
    let err = error(&mut s, "macro.play", json!({"source": src, "params": { "n": "two" }}));
    assert!(err.contains("`n`"), "{err}");
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6, "the refused macro changed nothing");
    // A plain number for a length is the design's own unit.
    run(&mut s, "edit.undo", json!({}));
    let r = run(&mut s, "macro.play", json!({"source": src, "params": { "side": 2 }}));
    assert_eq!(r["ran"], true);
    assert!((volume(&mut s) - 8.0).abs() < 1e-6);
}

#[test]
fn choice_params_offer_their_options() {
    let mut s = Session::default();
    let src = "// Shape\n// @param shape Shape choice [box, sphere] | box\n\
               if shape == \"sphere\" { run(\"solid.sphere\", #{ radius: 5 }); } else { run(\"solid.box\", #{ length: 10, width: 10, height: 10 }); }\n";
    run(&mut s, "macro.play", json!({"source": src}));
    assert_eq!(s.doc.features.len(), 1);
    // An option outside the list is refused, and the macro runs nothing.
    let err = error(&mut s, "macro.play", json!({"source": src, "params": { "shape": "cone" }}));
    assert!(err.contains("not one of"), "{err}");
    assert_eq!(s.doc.features.len(), 1);
    // A listed option reaches Rhai as text.
    run(&mut s, "macro.play", json!({"source": src, "params": { "shape": "sphere" }}));
    assert_eq!(s.doc.features.len(), 2);
}

#[test]
fn param_declarations_read_kind_and_options() {
    let m = super::macros::parse_rhai_macro(
        "macro.open",
        "// T\n// @param nx Count in X int | 2\n// @param mat Material choice [steel, brass] | brass\n// @param pa Pressure angle (deg) number | 20\nlet x = nx;\n",
    )
    .unwrap();
    assert_eq!(m.params.len(), 3);
    assert_eq!(m.params[0].label.as_deref(), Some("Count in X"));
    assert!(matches!(m.params[0].kind, super::macros::ParamKind::Int));
    assert_eq!(m.params[0].default, Some(json!(2)));
    assert_eq!(m.params[1].options, vec!["steel".to_string(), "brass".to_string()]);
    assert_eq!(m.params[1].default, Some(json!("brass")));
    assert_eq!(m.params[2].label.as_deref(), Some("Pressure angle (deg)"));
    // A choice without options is a bad macro, and so is a default outside its options.
    assert!(super::macros::parse_rhai_macro("macro.open", "// T\n// @param m Material choice | steel\n").is_err());
    assert!(super::macros::parse_rhai_macro("macro.open", "// T\n// @param m Material choice [steel] | wood\n").is_err());
    // An int default that is not a number is refused when the macro opens.
    assert!(super::macros::parse_rhai_macro("macro.open", "// T\n// @param n Count int | many\n").is_err());
}

#[test]
fn a_macro_that_changes_nothing_adds_no_undo_step() {
    let mut s = Session::default();
    let r = run(&mut s, "macro.play", json!({"source": "run(\"engine.commands\", #{});\n"}));
    assert_eq!(r["ran"], true);
    assert!(error(&mut s, "edit.undo", json!({})).contains("nothing to undo"));
}
