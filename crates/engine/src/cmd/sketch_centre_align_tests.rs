//! #617: a circle inside a square — manual Horizontal/Vertical between the circle's centre and
//! a square edge, and a distance dimension from an edge to the centre. Both must apply: the
//! engine already accepts centre refs (`c1.center`) and midpoint refs, so these pin the
//! acceptance criteria the reporter's manual tool path needed.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn point(s: &Session, id: &str) -> Vec2 {
    let sk = s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone();
    sk.resolve_point(id).and_then(|p| sk.point(p)).unwrap()
}

/// A fixed 100x100 square (`l1` bottom, `l2` right edge) with a radius-10 circle `c1` at (40, 60).
fn square_with_circle() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 100]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1", "l2", "l3", "l4"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [40, 60], "radius": 10, "id": "c1"}));
    s
}

#[test]
fn manual_vertical_constraint_aligns_circle_centre_with_a_square_edge() {
    let mut s = square_with_circle();
    // `l2` is the square's right edge (x = 100). Align the centre vertically with it.
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["c1.center", {"midpoint": "l2"}], "mode": "vertical"}));
    assert!((point(&s, "c1.center").x - 100.0).abs() < 1e-6, "centre x: {:?}", point(&s, "c1.center"));
    // The alignment persists through undo/redo and reopening.
    run(&mut s, "edit.undo", json!({}));
    assert!((point(&s, "c1.center").x - 40.0).abs() < 1e-6, "undo restores the centre");
    run(&mut s, "edit.redo", json!({}));
    assert!((point(&s, "c1.center").x - 100.0).abs() < 1e-6, "redo re-applies the alignment");
}

#[test]
fn manual_horizontal_constraint_aligns_circle_centre_with_a_square_edge() {
    let mut s = square_with_circle();
    // `l1` is the square's bottom edge (y = 0): align the centre horizontally with its midpoint.
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["c1.center", {"midpoint": "l1"}], "mode": "horizontal"}));
    assert!((point(&s, "c1.center").y - 0.0).abs() < 1e-6, "centre y: {:?}", point(&s, "c1.center"));
}

#[test]
fn a_dimension_from_an_edge_to_the_centre_measures_the_gap() {
    let mut s = square_with_circle();
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l2", "c1.center"]}));
    // The centre sits 60 from the right edge (x = 100).
    assert!((d["measured"].as_f64().unwrap() - 60.0).abs() < 1e-6, "{d}");
}
