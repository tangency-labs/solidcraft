//! Regression tests: freshly drawn rectangles take every ordinary edit without being reported
//! over-constrained (dragging, dimensioning, pinning the centre, making them square).

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// A new sketch with one rectangle of the given kind; returns its four side ids.
fn rect(kind: &str) -> (Session, Vec<String>) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = match kind {
        "center" => run(&mut s, "sketch.rectangle.center", json!({"center": [3, 2], "corner": [13, 9]})),
        "two" => run(&mut s, "sketch.rectangle.two_point", json!({"p0": [1, 1], "p1": [21, 15]})),
        _ => run(&mut s, "sketch.rectangle.three_point", json!({"p0": [0, 0], "p1": [20, 5], "p2": [18, 15], "connect": false})),
    };
    let sides = ids(&r["curves"]);
    assert_eq!(sides.len(), 4, "{r}");
    (s, sides)
}

fn healthy(s: &mut Session, what: &str) -> i64 {
    let si = run(s, "sketch.inspect", json!({}));
    assert_ne!(si["status"], "conflict", "{what}: {si}");
    assert!(si["status"].as_str().is_none_or(|t| !t.contains("over")), "{what}: {si}");
    si["dof"].as_i64().unwrap_or(-1)
}

const KINDS: [&str; 3] = ["center", "two", "three"];

#[test]
fn fresh_rectangles_have_the_expected_freedom() {
    for k in KINDS {
        let (mut s, _) = rect(k);
        let dof = healthy(&mut s, k);
        // Centre/two-point: x, y, width, height. Three-point adds the rotation.
        let want = if k == "three" { 5 } else { 4 };
        assert_eq!(dof, want, "{k}");
    }
}

#[test]
fn rectangles_take_dimensions_without_conflict() {
    for k in KINDS {
        let (mut s, l) = rect(k);
        run(&mut s, "sketch.dimension", json!({"entities": [l[0]], "value": 30}));
        healthy(&mut s, &format!("{k} width"));
        run(&mut s, "sketch.dimension", json!({"entities": [l[1]], "value": 12}));
        let dof = healthy(&mut s, &format!("{k} width+height"));
        assert_eq!(dof, if k == "three" { 3 } else { 2 }, "{k}");
    }
}

#[test]
fn rectangles_can_be_dragged_pinned_and_made_square() {
    for k in KINDS {
        // Drag a corner, then the middle of an edge.
        let (mut s, l) = rect(k);
        run(&mut s, "sketch.move_point", json!({"point": format!("{}.start", l[0]), "to": [-4, -3]}));
        healthy(&mut s, &format!("{k} drag corner"));
        run(&mut s, "sketch.move_point", json!({"point": format!("{}.end", l[2]), "to": [25, 22]}));
        healthy(&mut s, &format!("{k} drag other corner"));
        // Pin a corner to the origin.
        run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", l[0]), "b": "origin"}));
        healthy(&mut s, &format!("{k} corner on origin"));
        // Square: equal sides.
        run(&mut s, "sketch.constraint.equal", json!({"a": l[0], "b": l[1]}));
        healthy(&mut s, &format!("{k} square"));
        run(&mut s, "sketch.dimension", json!({"entities": [l[0]], "value": 10}));
        let dof = healthy(&mut s, &format!("{k} square sized"));
        assert_eq!(dof, if k == "three" { 1 } else { 0 }, "{k}");
    }
}

#[test]
fn dragging_corners_never_collapses_a_side() {
    for k in KINDS {
        let (mut s, l) = rect(k);
        for (i, to) in [[-4.0, -3.0], [25.0, 22.0], [30.0, -5.0], [-8.0, 18.0]].iter().enumerate() {
            let pt = format!("{}.start", l[i % 4]);
            run(&mut s, "sketch.move_point", json!({"point": pt, "to": to}));
            let sk = s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone();
            for c in &l {
                let ci = sk.curve_index(c).unwrap();
                let pi = sk.curves[ci].kind.point_ids();
                let len = sk.point(pi[0]).unwrap().dist(sk.point(pi[1]).unwrap());
                assert!(len > 0.5, "{k}: side {c} collapsed to {len} after dragging {pt}");
            }
            healthy(&mut s, &format!("{k} drag {i}"));
        }
    }
}

#[test]
fn center_rectangle_draws_both_diagonals_like_fusion() {
    let (mut s, _) = rect("center");
    let si = run(&mut s, "sketch.inspect", json!({}));
    let curves = si["curves"].as_array().cloned().unwrap_or_default();
    // Four sides plus two construction diagonals; the other rectangles have no diagonals.
    assert_eq!(curves.len(), 6, "{si}");
    assert_eq!(curves.iter().filter(|c| c["construction"] == true).count(), 2, "{si}");
    assert_eq!(healthy(&mut s, "center diagonals"), 4);
    for k in ["two", "three"] {
        let (mut s, _) = rect(k);
        let si = run(&mut s, "sketch.inspect", json!({}));
        assert_eq!(si["curves"].as_array().map(Vec::len), Some(4), "{k}: {si}");
    }
}

#[test]
fn equal_parameter_makes_rectangles_square_with_one_dimension() {
    for k in KINDS {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        let (cmd, params) = match k {
            "center" => ("sketch.rectangle.center", json!({"center": [3, 2], "corner": [13, 9], "equal": true})),
            "two" => ("sketch.rectangle.two_point", json!({"p0": [1, 1], "p1": [21, 15], "equal": true})),
            _ => ("sketch.rectangle.three_point", json!({"p0": [1, 1], "p1": [21, 1], "p2": [21, 13], "equal": true, "connect": false})),
        };
        let r = run(&mut s, "sketch.draw.dimensioned", json!({"command": cmd, "params": params, "dimensions": {"width": 16}}));
        assert_eq!(ids(&r["curves"]).len(), 4, "{k}: {r}");
        let dof = healthy(&mut s, &format!("{k} equal square"));
        assert_eq!(dof, if k == "three" { 3 } else { 2 }, "{k}");
        let si = run(&mut s, "sketch.inspect", json!({}));
        let area = si["profiles"][0]["area_mm2"].as_f64().unwrap();
        assert!((area - 256.0).abs() < 1e-6, "{k}: area {area}");
    }
}
