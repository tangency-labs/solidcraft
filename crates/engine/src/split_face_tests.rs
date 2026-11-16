//! Split Face: the top of a box divided by a plane, by another body's face and by sketch curves.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// (kernel faces, volume) of a body: the merged count joins pieces of one surface back up.
fn faces_volume(s: &mut Session, b: &str) -> (u64, f64) {
    let m = run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0].clone();
    (m["kernel"]["faces"].as_u64().unwrap_or(0), m["volume_mm3"].as_f64().unwrap_or(0.0))
}

fn boxed() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
    s
}

#[test]
fn split_a_face_by_a_plane() {
    let mut s = boxed();
    run(&mut s, "solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    let (f, v) = faces_volume(&mut s, "B");
    assert_eq!(f, 7);
    assert!((v - 24000.0).abs() < 1e-6 * 24000.0, "{v}");
    assert_eq!(s.doc.features.last().unwrap().kind.type_name(), "SplitFaceFeature");
}

#[test]
fn split_a_face_by_another_bodys_face() {
    let mut s = boxed();
    run(&mut s, "solid.box", json!({"corner": [-5, 10, 25], "length": 50, "width": 5, "height": 5, "body_name": "Tool"}));
    // The tool's face at y = 10, extended down through the box's top.
    run(&mut s, "solid.split_face", json!({"faces": [[20, 5, 20]], "body": "B", "tool": {"body": "Tool", "point": [20, 10, 27]}}));
    assert_eq!(faces_volume(&mut s, "B").0, 7);
}

#[test]
fn split_a_face_by_sketch_curves() {
    let mut s = boxed();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cuts"}));
    run(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 35]]}));
    run(&mut s, "sketch.line", json!({"points": [[30, -5], [30, 35]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Cuts"}));
    let (f, v) = faces_volume(&mut s, "B");
    assert_eq!(f, 8, "two lines make three pieces of the top");
    assert!((v - 24000.0).abs() < 1e-6 * 24000.0, "{v}");
}

/// A triangle of three lines on the top: by default each side is extended across the face; with
/// `loops` the sides are joined first and the closed loop cuts out the triangle only. Open curves
/// are extended either way.
#[test]
fn split_a_face_by_a_closed_loop_of_lines() {
    let split = |loops: bool| {
        let mut s = boxed();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Loop"}));
        run(&mut s, "sketch.line", json!({"points": [[10, 5], [30, 5], [20, 25]], "closed": true}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.split_face", json!({"faces": [[20, 12, 20]], "body": "B", "sketch": "Loop", "loops": loops}));
        faces_volume(&mut s, "B")
    };
    let (f, v) = split(true);
    assert_eq!(f, 7, "the triangle and the rest of the top");
    assert!((v - 24000.0).abs() < 1e-6 * 24000.0, "{v}");
    assert!(split(false).0 > 7, "each side extended across the top");

    let mut s = boxed();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cuts"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [10, 20]]}));
    run(&mut s, "sketch.line", json!({"points": [[30, 10], [30, 20]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Cuts", "loops": true}));
    assert_eq!(faces_volume(&mut s, "B").0, 8, "two open lines still cut across the top");
}

#[test]
fn a_tool_that_misses_is_refused() {
    let mut s = boxed();
    let n = s.doc.features.len();
    assert!(s.execute("solid.split_face", &json!({"faces": [[10, 15, 20]], "plane": {"origin": [100, 0, 0], "normal": [1, 0, 0]}})).is_err());
    assert_eq!(s.doc.features.len(), n);
}

#[test]
fn export_keeps_split_faces_through_later_features() {
    let mut s = boxed();
    run(&mut s, "solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    // A later feature rebuilds the body; the split stays a split in the file.
    run(&mut s, "solid.fillet", json!({"edges": [[20, 0, 0]], "radius": 2}));
    let (f, _) = faces_volume(&mut s, "B");
    assert_eq!(f, 8);
    let dir = std::env::temp_dir().join(format!("solidcraft-split-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let step = dir.join("split.step");
    run(&mut s, "file.export", json!({"path": step.to_string_lossy()}));
    let text = std::fs::read_to_string(&step).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    // Six box faces, the top's second piece and the fillet.
    assert_eq!(text.matches("ADVANCED_FACE(").count(), 8, "{text}");
}

#[test]
fn split_pieces_count_and_move_apart() {
    let mut s = boxed();
    run(&mut s, "solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    // Measure counts the two halves of the top as two faces.
    let m = run(&mut s, "inspect.measure", json!({ "bodies": ["B"] }))["bodies"][0].clone();
    assert_eq!(m["faces"].as_u64(), Some(7), "{m}");
    // Offset Face on one half moves that half only: a 20 × 30 step 5 high.
    run(&mut s, "solid.offset_face", json!({"faces": [[10, 15, 20]], "distance": 5}));
    let (_, v) = faces_volume(&mut s, "B");
    assert!((v - 27000.0).abs() < 1e-6 * 27000.0, "{v}");
    // And the other half down 3: a 20 × 30 pocket.
    run(&mut s, "solid.offset_face", json!({"faces": [[30, 15, 20]], "distance": -3}));
    let (_, v) = faces_volume(&mut s, "B");
    assert!((v - 25200.0).abs() < 1e-6 * 25200.0, "{v}");
}
