use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn folder(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-relink-{case}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn part(path: &Path, height: u32) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": height, "corner": [0,0,0]}));
    run(&mut s, "file.save", json!({"path": path}));
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

#[test]
fn reused_design_insertion_is_free_even_before_a_link_exists() {
    let dir = folder("reuse-grounding");
    let file = dir.join("part.solidcraft");
    part(&file, 30);
    let mut s = Session::default();
    for count in 1..=2 {
        let inserted = run(&mut s, "component.insert_design", json!({"path": file, "reuse": true}));
        let occurrence = inserted["occurrence"].as_u64().unwrap();
        assert!(!s.doc.occurrences.iter().find(|o| o.id == occurrence).unwrap().grounded);
        assert_eq!(s.doc.links.len(), 1);
        assert_eq!(s.undo.len(), count, "each paste is one undo step");
        run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [10, 0, 0]}));
        run(&mut s, "edit.undo", json!({}));
    }
    let mut ordinary = Session::default();
    let inserted = run(&mut ordinary, "component.insert_design", json!({"path": file}));
    let occurrence = inserted["occurrence"].as_u64().unwrap();
    assert!(ordinary.doc.occurrences.iter().find(|o| o.id == occurrence).unwrap().grounded);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn relink_repairs_a_moved_source_for_every_instance_in_one_undo_step() {
    let dir = folder("moved");
    let old = dir.join("old.solidcraft");
    let moved = dir.join("moved.solidcraft");
    let assembly = dir.join("assembly.solidcraft");
    part(&old, 30);
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": assembly}));
    let inserted = run(&mut s, "component.insert_design", json!({"path": old}));
    let component = inserted["component"].as_u64().unwrap();
    let first = inserted["occurrence"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [70,30,10]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": second, "translate": [0,12,0], "capture": false}));
    let occurrences = s.doc.occurrences.clone();
    let pending = s.pending_moves.clone();
    let before = serde_json::to_value(&*s.doc).unwrap();
    let undo = s.undo.len();
    std::fs::rename(&old, &moved).unwrap();
    part(&moved, 45);
    let result = run(&mut s, "component.relink", json!({"component": component, "path": moved}));
    assert_eq!(result["previous_path"], "old.solidcraft");
    assert_eq!(result["path"], "moved.solidcraft");
    assert_eq!(result["geometry_changed"], true);
    assert_eq!(result["warnings"], json!([]));
    assert_eq!(s.doc.occurrences, occurrences);
    assert_eq!(s.pending_moves, pending);
    assert_eq!(s.undo.len(), undo + 1);
    assert!((volume(&mut s) - 18000.0).abs() < 1e-5);
    let after = serde_json::to_value(&*s.doc).unwrap();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
    assert_eq!(s.pending_moves, pending);
    assert!((volume(&mut s) - 12000.0).abs() < 1e-5);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(serde_json::to_value(&*s.doc).unwrap(), after);
    run(&mut s, "file.save", json!({}));
    let mut reopened = Session::default();
    run(&mut reopened, "doc.open", json!({"path": assembly}));
    assert_eq!(run(&mut reopened, "component.links", json!({}))["links"][0]["path"], "moved.solidcraft");
    assert_eq!(run(&mut reopened, "component.links", json!({}))["links"][0]["found"], true);
    assert!((volume(&mut reopened) - 18000.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn relink_rejects_bad_sources_and_targets_without_touching_the_design_or_undo() {
    let dir = folder("failures");
    let source = dir.join("source.solidcraft");
    let assembly = dir.join("assembly.solidcraft");
    let empty = dir.join("empty.solidcraft");
    let corrupt = dir.join("corrupt.solidcraft");
    part(&source, 30);
    let mut s = Session::default();
    let component = run(&mut s, "component.insert_design", json!({"path": source}))["component"].as_u64().unwrap();
    run(&mut s, "file.save", json!({"path": assembly}));
    run(&mut Session::default(), "file.save", json!({"path": empty}));
    std::fs::write(&corrupt, b"not a design").unwrap();
    let before = serde_json::to_value(&*s.doc).unwrap();
    let undo = s.undo.len();
    for params in [
        json!({}),
        json!({"component": component}),
        json!({"component": false, "path": source}),
        json!({"component": 0, "path": source}),
        json!({"component": "unknown", "path": source}),
        json!({"component": component, "path": "   "}),
        json!({"component": component, "path": false}),
        json!({"component": component, "path": dir.join("missing.solidcraft")}),
        json!({"component": component, "path": dir.join("source.step")}),
        json!({"component": component, "path": empty}),
        json!({"component": component, "path": corrupt}),
        json!({"component": component, "path": assembly}),
    ] {
        assert!(s.execute("component.relink", &params).is_err(), "{params}");
        assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before, "{params}");
        assert_eq!(s.undo.len(), undo);
        assert!((volume(&mut s) - 6000.0).abs() < 1e-5);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn relink_keeps_joint_definitions_and_reports_solver_diagnostics() {
    let dir = folder("joints");
    let original = dir.join("original.solidcraft");
    let replacement = dir.join("replacement.solidcraft");
    part(&original, 30);
    part(&replacement, 45);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": original}));
    let component = inserted["component"].as_u64().unwrap();
    let occurrence = inserted["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.ground", json!({"occurrence": occurrence, "grounded": false}));
    // Explicit points can keep solving after the source changes, even if the intended
    // connection no longer exists. The source-change warning must still be present.
    run(
        &mut s,
        "joint.create",
        json!({"name": "Seat", "type": "rigid", "a": {"occurrence": 0, "point": [0,0,0]}, "b": {"occurrence": occurrence, "point": [0,0,0]}}),
    );
    let joints = s.doc.assembly.joints.clone();
    let identical = dir.join("identical.solidcraft");
    std::fs::copy(&original, &identical).unwrap();
    let result = run(&mut s, "component.relink", json!({"component": component, "path": identical}));
    assert_eq!(result["geometry_changed"], false, "renaming a source is not a geometry change");
    assert_eq!(result["warnings"], json!([]));
    assert_eq!(s.doc.assembly.joints, joints);
    let result = run(&mut s, "component.relink", json!({"component": component, "path": replacement}));
    assert_eq!(s.doc.assembly.joints, joints);
    assert!(result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("check this component's joints"))));
    // A deliberately lost named face exercises the ordinary solver diagnostics, without
    // requiring relink to guess which face of a different design was intended.
    let joint = s.doc_mut().assembly.joints.first_mut().unwrap();
    joint.b.snap =
        solidcraft_doc::joints::Snap::FaceCenter { pick: solidcraft_geom::Vec3::ZERO, face: Some("no-such-linked-face".to_string()), x: None };
    s.refresh();
    let result = run(&mut s, "component.relink", json!({"component": component, "path": replacement}));
    assert!(result["joint_errors"].as_array().unwrap().iter().any(|e| e["name"] == "Seat"));
    assert!(result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("joint Seat:"))));
    let _ = std::fs::remove_dir_all(dir);
}
