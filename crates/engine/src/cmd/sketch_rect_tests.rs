//! Regression tests: freshly drawn rectangles take every ordinary edit without being reported
//! over-constrained (dragging, dimensioning, pinning the centre, making them square).

use serde_json::{Value, json};
use solidcraft_geom::prelude::*;

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

// ---- Edge Center Rectangle -------------------------------------------------------------

fn active(s: &Session) -> solidcraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

/// The area of the sketch's one profile, and whether the sketch solved.
fn profile(s: &Session) -> (usize, f64, bool) {
    let st = s.model.state();
    let sk = st.sketch(s.active_sketch.unwrap()).unwrap();
    (sk.profiles.len(), sk.profiles.iter().map(|p| p.area).sum(), sk.report.ok())
}

/// The four corners of the rectangle drawn by `r` (the points of its sides), as (x, y).
fn corners(s: &Session, r: &Value) -> Vec<(f64, f64)> {
    let sk = active(s);
    let mut out = Vec::new();
    for key in ["anchor_start", "anchor_end", "far_end", "far_start"] {
        let id = r["corners"][key].as_str().unwrap();
        let p = sk.point(sk.resolve_point(id).unwrap()).unwrap();
        out.push((p.x, p.y));
    }
    out
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn edge_rect(anchor: Value, corner: Value, direction: Value) -> (Session, Value) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.rectangle.edge_center", json!({"anchor": anchor, "corner": corner, "direction": direction}));
    (s, r)
}

#[test]
fn edge_center_rectangle_axis_example_and_mirror() {
    // Anchor (0,0), corner (4,3), vertical anchored edge: x = 0..4, y = -3..3.
    let (s, r) = edge_rect(json!([0, 0]), json!([4, 3]), json!([0, 1]));
    let c = corners(&s, &r);
    let (xs, ys): (Vec<f64>, Vec<f64>) = c.iter().copied().unzip();
    assert!(near(xs.iter().cloned().fold(f64::MAX, f64::min), 0.0) && near(xs.iter().cloned().fold(f64::MIN, f64::max), 4.0), "{c:?}");
    assert!(near(ys.iter().cloned().fold(f64::MAX, f64::min), -3.0) && near(ys.iter().cloned().fold(f64::MIN, f64::max), 3.0), "{c:?}");
    assert_eq!(ids(&r["curves"]).len(), 4);
    let (n, area, ok) = profile(&s);
    assert!(n == 1 && ok && near(area, 24.0), "{n} {area} {ok}");

    // The other way: the corner on the other side extends the other way.
    let (s, r) = edge_rect(json!([0, 0]), json!([-4, 3]), json!([0, 1]));
    let c = corners(&s, &r);
    assert!(c.iter().all(|p| p.0 <= 1e-9 && p.0 >= -4.0 - 1e-9 && p.1.abs() <= 3.0 + 1e-9), "{c:?}");
    assert!(c.iter().any(|p| near(p.0, -4.0)));
    assert!(near(profile(&s).1, 24.0));
}

#[test]
fn edge_center_rectangle_horizontal_edge_and_positions() {
    // Horizontal anchored edge: spans x = ax±h and extends in y toward the corner, up or down.
    for (anchor, corner) in [([10.0_f64, 5.0], [13.0_f64, 9.0]), ([-7.5, 2.0], [-9.0, -6.0]), ([1e3, -2e3], [1e3 + 0.5, -2e3 + 0.25])] {
        let (s, r) = edge_rect(json!(anchor), json!(corner), json!([1, 0]));
        let (w, h) = ((corner[1] - anchor[1]).abs(), (corner[0] - anchor[0]).abs());
        let c = corners(&s, &r);
        let side = (corner[1] - anchor[1]).signum();
        assert!(near(c[0].0, anchor[0] - h) && near(c[1].0, anchor[0] + h) && near(c[0].1, anchor[1]) && near(c[1].1, anchor[1]), "{c:?}");
        assert!(near(c[2].1, anchor[1] + side * w) && near(c[3].1, anchor[1] + side * w), "{c:?}");
        let (n, area, ok) = profile(&s);
        assert!(n == 1 && ok && (area - 2.0 * h * w).abs() < 1e-6 * (1.0 + area), "{anchor:?} {area}");
    }
}

#[test]
fn edge_center_rectangle_rotated_edges() {
    for deg in [30.0_f64, 135.0, -70.0, 210.0] {
        let (sin, cos) = deg.to_radians().sin_cos();
        let d = [cos, sin];
        let n = [-sin, cos];
        let (anchor, along, across) = ([4.0, -3.0], 2.5, 6.0);
        for side in [1.0, -1.0] {
            let corner = [anchor[0] + d[0] * along + n[0] * across * side, anchor[1] + d[1] * along + n[1] * across * side];
            let (mut s, r) = edge_rect(json!(anchor), json!(corner), json!(d));
            let c = corners(&s, &r);
            let want = [
                (anchor[0] - d[0] * along, anchor[1] - d[1] * along),
                (anchor[0] + d[0] * along, anchor[1] + d[1] * along),
                (anchor[0] + d[0] * along + n[0] * across * side, anchor[1] + d[1] * along + n[1] * across * side),
                (anchor[0] - d[0] * along + n[0] * across * side, anchor[1] - d[1] * along + n[1] * across * side),
            ];
            for (g, w) in c.iter().zip(want) {
                assert!(near(g.0, w.0) && near(g.1, w.1), "{deg} {side}: {c:?} vs {want:?}");
            }
            let (n, area, ok) = profile(&s);
            assert!(n == 1 && ok && near(area, 2.0 * along * across), "{deg}: {area}");
            // Still a rectangle after the solver has had its say, with a sane degree of freedom.
            assert_eq!(healthy(&mut s, "edge center"), 5 - if deg.rem_euclid(90.0) == 0.0 { 1 } else { 0 });
        }
    }
}

#[test]
fn edge_center_rectangle_snaps_to_a_line_midpoint() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let l = run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 10]], "connect": false}));
    let line = ids(&l["curves"])[0].clone();
    // No direction given: it comes from the line the anchor snaps to.
    let r = run(&mut s, "sketch.rectangle.edge_center", json!({"anchor": format!("mid:{line}"), "corner": [0, 12]}));
    let c = corners(&s, &r);
    let mid = (5.0, 5.0);
    assert!(near((c[0].0 + c[1].0) / 2.0, mid.0) && near((c[0].1 + c[1].1) / 2.0, mid.1), "{c:?}");
    // The anchored edge is parallel to the line: its direction is (1,1)/sqrt2.
    assert!(near(c[1].0 - c[0].0, c[1].1 - c[0].1), "{c:?}");
    assert_eq!(profile(&s).0, 1, "the rectangle is one closed profile");
    let sk = active(&s);
    assert_eq!(sk.curves.iter().filter(|c| matches!(c.kind, solidcraft_sketch::CurveKind::Line { .. })).count(), 5);
    assert_ne!(run(&mut s, "sketch.inspect", json!({}))["status"], "conflict");
}

#[test]
fn edge_center_rectangle_orientation_is_never_assumed() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let before = active(&s).curves.len();
    for p in [
        json!({"anchor": [0, 0], "corner": [4, 3]}),
        json!({"anchor": [0, 0], "corner": [4, 3], "direction": [0, 0]}),
        json!({"anchor": [0, 0], "corner": [4, 0], "direction": [0, 1]}),
        json!({"anchor": [0, 0], "corner": [0, 3], "direction": [0, 1]}),
        json!({"anchor": [0, 0], "corner": [4, 3], "direction": "x"}),
        json!({"anchor": [0, 0], "corner": [4, 3], "line": "nope"}),
    ] {
        assert!(s.execute("sketch.rectangle.edge_center", &p).is_err(), "{p}");
    }
    assert_eq!(active(&s).curves.len(), before, "a rejected rectangle leaves nothing behind");
}
