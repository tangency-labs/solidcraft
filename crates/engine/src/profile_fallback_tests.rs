//! A profile picked by the curves around it, and a sketch on the cap such a profile makes, keep
//! working when the sketch's outline is edited so that no profile is bounded by exactly those
//! curves any more: a line that splits the region, a side deleted and drawn again, a notch
//! trimmed into the outline. The feature falls back to the best match and warns; it no longer
//! fails and takes its body (and everything after it) with it.

use serde_json::{Value, json};

use crate::Session;

const HOLE: f64 = std::f64::consts::PI * 16.0 * 10.0;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &Session) -> f64 {
    s.model.state().bodies.iter().map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-3)).unwrap().measure().volume).sum()
}

fn result(s: &Session, name: &str) -> (Option<String>, Option<String>) {
    let id = s.doc.find_feature(name).unwrap().id;
    let r = s.model.result(id).unwrap();
    (r.error.clone(), r.warning.clone())
}

/// A 40 × 20 plate with a hole: the outline `l1`–`l4` (its width is the parameter `W`) and a
/// circle, the region between them picked by a point and extruded 10 mm.
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 10], "radius": 4}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "W"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "10 mm", "profiles": [{"point": [10, 10]}], "name": "E1"}));
    assert_eq!(result(&s, "E1"), (None, None));
    assert!((volume(&s) - (8000.0 - HOLE)).abs() < 0.5, "{}", volume(&s));
    s
}

fn edit(s: &mut Session, steps: &[(&str, Value)]) {
    run(s, "sketch.edit", json!({"sketch": "S1"}));
    for (id, p) in steps {
        run(s, id, p.clone());
    }
    run(s, "sketch.finish", json!({}));
}

fn assert_survives(s: &Session, feature: &str, expected: f64, says: &str) {
    let (error, warning) = result(s, feature);
    assert_eq!(error, None, "{feature} keeps its body");
    let warning = warning.unwrap_or_default();
    assert!(warning.contains(says), "{feature}: {warning}");
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!((volume(s) - expected).abs() < 0.5, "{feature}: volume {} (expected {expected})", volume(s));
}

/// A line across the picked region cuts it in two: both pieces together are the region that was
/// picked, so the plate keeps its whole volume (with its hole, which stays a hole).
#[test]
fn an_extrude_survives_a_line_splitting_its_profile() {
    for x in [20, 15] {
        let mut s = plate();
        edit(&mut s, &[("sketch.line", json!({"points": [[x, 0], [x, 20]]}))]);
        assert_survives(&s, "E1", 8000.0 - HOLE, "is cut by another curve now; using the 2 pieces inside it");
    }
}

/// A side deleted and drawn again is a new curve: the profile that shares the other three sides
/// is the plate, and the warning names the side that went and the one that came.
#[test]
fn an_extrude_survives_a_side_deleted_and_drawn_again() {
    for (side, line) in [("l2", [[40, 0], [40, 20]]), ("l3", [[40, 20], [0, 20]])] {
        let mut s = plate();
        edit(&mut s, &[("sketch.delete", json!({"entities": [side]})), ("sketch.line", json!({"points": line}))]);
        assert_survives(&s, "E1", 8000.0 - HOLE, &format!("`{side}` no longer on it"));
        assert!(result(&s, "E1").1.unwrap().contains("shares 3 of its 4 curves"));
    }
}

/// A notch trimmed out of the top side adds curves to the outline: the profile still has all four
/// sides and takes the notch with it.
#[test]
fn an_extrude_survives_a_notch_trimmed_into_its_outline() {
    let mut s = plate();
    edit(
        &mut s,
        &[("sketch.line", json!({"points": [[10, 20], [10, 25], [20, 25], [20, 20]]})), ("sketch.trim", json!({"curve": "l3", "at": [15, 20]}))],
    );
    assert_survives(&s, "E1", 8000.0 + 500.0 - HOLE, "using the profile that shares 4 of its 4 curves");
}

/// Dimension edits still match exactly, with no warning; an outline replaced altogether matches
/// nothing, and that is still an error rather than a guess.
#[test]
fn exact_matches_stay_quiet_and_nothing_plausible_is_an_error() {
    let mut s = plate();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "60 mm"}));
    assert_eq!(result(&s, "E1"), (None, None));
    assert!((volume(&s) - (12000.0 - HOLE)).abs() < 0.5);
    edit(
        &mut s,
        &[("sketch.delete", json!({"entities": ["l1", "l2", "l3", "l4"]})), ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [50, 20]}))],
    );
    let (error, _) = result(&s, "E1");
    assert!(error.is_some_and(|e| e.contains("no profile of S1 is bounded by exactly")));
    assert!(s.model.state().bodies.is_empty());
}

/// A block `E1` with a sketch `S2` on its top cap and a hole cut from it, `E2`.
fn block_with_hole() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "W"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "10 mm", "name": "E1"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}, "name": "S2"}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "3 mm", "direction": "negative", "operation": "cut", "name": "E2"}));
    for f in ["E1", "S2", "E2"] {
        assert_eq!(result(&s, f).0, None, "{f}");
    }
    s
}

fn hole_volume() -> f64 {
    std::f64::consts::PI * 9.0 * 3.0
}

fn assert_hole_follows(s: &Session, block: f64, sketch_warns: bool) {
    let (error, warning) = result(s, "S2");
    assert_eq!(error, None, "S2 keeps its face");
    if sketch_warns {
        let warning = warning.unwrap_or_default();
        assert!(warning.contains("the face this sketch sits on, the top cap of E1"), "{warning}");
        assert!(warning.contains("using the top cap of E1 as it is now"), "{warning}");
    }
    assert_eq!(result(s, "E2").0, None, "the hole after it is still cut");
    let id = s.doc.find_feature("S2").unwrap().id;
    let plane = s.model.state().sketch(id).unwrap().plane;
    assert!((plane.origin.z - 10.0).abs() < 1e-9, "{plane:?}");
    assert!((volume(s) - (block - hole_volume())).abs() < 0.5, "volume {} (expected {})", volume(s), block - hole_volume());
}

/// The base sketch cut by a line still makes one block with one top cap.
#[test]
fn a_sketch_on_a_cap_survives_a_line_splitting_the_base_profile() {
    let mut s = block_with_hole();
    edit(&mut s, &[("sketch.line", json!({"points": [[20, 0], [20, 20]]}))]);
    assert_hole_follows(&s, 8000.0, false);
}

/// A side of the base sketch deleted and drawn again: the cap is bounded by a new curve.
#[test]
fn a_sketch_on_a_cap_survives_a_base_side_deleted_and_drawn_again() {
    let mut s = block_with_hole();
    edit(&mut s, &[("sketch.delete", json!({"entities": ["l2"]})), ("sketch.line", json!({"points": [[40, 0], [40, 20]]}))]);
    assert_hole_follows(&s, 8000.0, true);
    assert!(result(&s, "S2").1.unwrap().contains("`l2` no longer around it"));
}

/// A corner notch trimmed into the base sketch.
#[test]
fn a_sketch_on_a_cap_survives_a_notch_trimmed_into_the_base_outline() {
    let mut s = block_with_hole();
    edit(
        &mut s,
        &[
            ("sketch.line", json!({"points": [[0, 15], [5, 15], [5, 20]]})),
            ("sketch.trim", json!({"curve": "l4", "at": [0, 18]})),
            ("sketch.trim", json!({"curve": "l3", "at": [2, 20]})),
        ],
    );
    assert_hole_follows(&s, 8000.0 - 25.0 * 10.0, true);
}

/// The base extrude picked by a point (stored by its curves) and the sketch on its cap both
/// survive the outline being edited.
#[test]
fn a_picked_profile_and_the_sketch_on_its_cap_survive_together() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "10 mm", "profiles": [{"point": [10, 10]}], "name": "E1"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}, "name": "S2"}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "3 mm", "direction": "negative", "operation": "cut", "name": "E2"}));
    edit(&mut s, &[("sketch.delete", json!({"entities": ["l2"]})), ("sketch.line", json!({"points": [[40, 0], [40, 20]]}))]);
    assert!(result(&s, "E1").1.is_some_and(|w| w.contains("`l2` no longer on it")));
    assert_hole_follows(&s, 8000.0, true);
    // Saved and opened again, it evaluates the same way.
    let reopened = Session::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap());
    assert_hole_follows(&reopened, 8000.0, true);
}
