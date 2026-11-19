//! `sketch.spline.control_knots`: a control point spline with the degree, knot vector and weights
//! it is given, drawn and extruded exactly as given (#19524).

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::MeshGeom;
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{CurveKind, Sketch};

use crate::{EngineError, Session};

const KNOTS_CMD: &str = "sketch.spline.control_knots";

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

/// The sketch being edited, or else the design's first feature's sketch once it is finished.
fn sketch(s: &Session) -> Sketch {
    let id = s.active_sketch.unwrap_or_else(|| s.doc.features.first().unwrap().id);
    s.model.state().sketch(id).unwrap().sketch.clone()
}

/// The 33 control points of the issue's example: a wave along X.
fn issue_points() -> Vec<[f64; 2]> {
    (0..33).map(|i| [i as f64 * 0.7, (i as f64 * 0.9).sin() * 3.0]).collect()
}

/// The clamped knot vector of 33 points of degree 3: 30 knot spans of 1/64 (28 of them), 1/2 and
/// 1/16, so the steps are not uniform and every value is exact in binary.
fn issue_knots() -> Vec<f64> {
    let spacings: Vec<f64> = std::iter::repeat_n(1.0 / 64.0, 28).chain([0.5, 1.0 / 16.0]).collect();
    let mut knots = vec![0.0; 4];
    let mut at = 0.0;
    for s in spacings.iter().take(29) {
        at += s;
        knots.push(at);
    }
    knots.extend([1.0; 4]);
    knots
}

fn points_json(points: &[[f64; 2]]) -> Value {
    json!(points)
}

/// The curve of the sketch's only spline.
fn spline_index(sk: &Sketch) -> usize {
    sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Spline { .. })).unwrap()
}

/// Shoelace area of a closed loop of points.
fn area(loop_pts: &[Vec2]) -> f64 {
    let n = loop_pts.len();
    (0..n).map(|i| loop_pts[i].x * loop_pts[(i + 1) % n].y - loop_pts[(i + 1) % n].x * loop_pts[i].y).sum::<f64>() / 2.0
}

/// The curve over `[0, 1)` by the Cox–de Boor recursion, independent of the sketch's evaluation.
fn cox_de_boor(knots: &[f64], pts: &[[f64; 2]], degree: usize, u: f64) -> Vec2 {
    fn basis(knots: &[f64], i: usize, p: usize, u: f64) -> f64 {
        if p == 0 {
            return if knots[i] <= u && u < knots[i + 1] { 1.0 } else { 0.0 };
        }
        let left = if knots[i + p] > knots[i] { (u - knots[i]) / (knots[i + p] - knots[i]) * basis(knots, i, p - 1, u) } else { 0.0 };
        let right = if knots[i + p + 1] > knots[i + 1] {
            (knots[i + p + 1] - u) / (knots[i + p + 1] - knots[i + 1]) * basis(knots, i + 1, p - 1, u)
        } else {
            0.0
        };
        left + right
    }
    let (mut x, mut y) = (0.0, 0.0);
    for (i, q) in pts.iter().enumerate() {
        let w = basis(knots, i, degree, u);
        x += w * q[0];
        y += w * q[1];
    }
    Vec2::new(x, y)
}

/// The height of the closing path under the curve: below every control point, so the loop the
/// spline and the path make does not cross itself.
const FLOOR_Y: f64 = -10.0;

/// The spline drawn in a new sketch, which is still being edited.
fn drawn(points: &[[f64; 2]], degree: u64, knots: &[f64]) -> Session {
    let mut s = new_sketch();
    run(&mut s, KNOTS_CMD, json!({"points": points_json(points), "degree": degree, "knots": knots}));
    s
}

/// The spline closed by a path down from its end, across below it and up to its start, then
/// the sketch finished: a simple closed profile.
fn closed_profile(points: &[[f64; 2]], degree: u64, knots: &[f64]) -> Session {
    let mut s = drawn(points, degree, knots);
    let (first, last) = (points[0], points[points.len() - 1]);
    run(&mut s, "sketch.line", json!({"points": [last, [last[0], FLOOR_Y], [first[0], FLOOR_Y], first]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn bodies(s: &Session) -> Vec<(String, f64)> {
    s.model
        .state()
        .bodies
        .iter()
        .map(|b| (b.name.clone(), b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-3)).unwrap().measure().volume))
        .collect()
}

#[test]
fn the_issue_spline_is_drawn_with_its_knots_and_extrudes_to_a_closed_body() {
    let points = issue_points();
    let knots = issue_knots();
    let mut s = closed_profile(&points, 3, &knots);
    let sk = sketch(&s);
    let ci = spline_index(&sk);
    let CurveKind::Spline { nurbs: Some(nurbs), degree, control, periodic, .. } = &sk.curves[ci].kind else { panic!("{:?}", sk.curves[ci].kind) };
    assert_eq!((*degree, *control, *periodic), (3, true, false));
    assert_eq!(nurbs.knots, knots);
    assert!(nurbs.weights.is_empty());
    // Thirty knot spans, each a cubic Bézier piece of the spline.
    assert_eq!(sk.exact_pieces(ci).len(), 30);
    // The profile, sampled at its own parameters by the independent recursion, extrudes to a
    // closed body of the same area times the distance.
    let mut profile: Vec<Vec2> = (0..4096).map(|k| cox_de_boor(&knots, &points, 3, k as f64 / 4096.0)).collect();
    let last = points[points.len() - 1];
    let first = points[0];
    profile.push(Vec2::new(last[0], FLOOR_Y));
    profile.push(Vec2::new(first[0], FLOOR_Y));
    let profile_area = area(&profile).abs();
    run(&mut s, "solid.extrude", json!({"distance": 5, "body_name": "Knotted"}));
    let result = s.model.result(s.doc.features.last().unwrap().id).unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    let found = bodies(&s);
    assert_eq!(found.len(), 1, "{found:?}");
    let volume = found[0].1;
    assert!((volume - profile_area * 5.0).abs() < 1e-2 * profile_area * 5.0, "volume {volume} area {profile_area}");
}

#[test]
fn a_quarter_circle_as_a_rational_quadratic_is_the_analytic_circle() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let mut s = new_sketch();
    run(&mut s, KNOTS_CMD, json!({"points": [[2, 0], [2, 2], [0, 2]], "degree": 2, "knots": [0, 0, 0, 1, 1, 1], "weights": [1, w, 1]}));
    let sk = sketch(&s);
    let ci = spline_index(&sk);
    let poly = sk.polyline(ci);
    assert!(poly.len() > 10);
    for q in &poly {
        assert!((q.len() - 2.0).abs() < 1e-12, "{q:?} radius {}", q.len());
    }
    assert!(poly[0].dist(Vec2::new(2.0, 0.0)) < 1e-12 && poly[poly.len() - 1].dist(Vec2::new(0.0, 2.0)) < 1e-12);
    let pieces = sk.exact_pieces(ci);
    assert!(matches!(pieces[..], [solidcore_record::Seg2::Conic { .. }]), "{pieces:?}");
}

#[test]
fn a_spline_with_knots_is_saved_and_read_back_bit_identically() {
    let points = issue_points();
    let knots = issue_knots();
    let s = closed_profile(&points, 3, &knots);
    let json = s.doc.to_json();
    let back = Session::new(solidcraft_doc::Document::from_json(&json).unwrap());
    let sk = sketch(&back);
    let ci = spline_index(&sk);
    let CurveKind::Spline { nurbs: Some(nurbs), .. } = &sk.curves[ci].kind else { panic!("{:?}", sk.curves[ci].kind) };
    assert_eq!(nurbs.knots.len(), knots.len());
    assert!(nurbs.knots.iter().zip(&knots).all(|(a, b)| a.to_bits() == b.to_bits()));
    assert_eq!(back.doc.to_json(), json);
}

#[test]
fn spline_knots_that_do_not_fit_the_points_are_refused_and_the_sketch_is_unchanged() {
    let p3 = [[0.0, 0.0], [1.0, 2.0], [2.0, 2.0], [3.0, 0.0]];
    let clamped4 = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let weights_ok = json!([1, 2, 1, 2]);
    let cases: Vec<(&str, Value)> = vec![
        ("degree 0", json!({"points": p3, "degree": 0, "knots": clamped4})),
        ("degree 10", json!({"points": p3, "degree": 10, "knots": clamped4})),
        ("a missing knot", json!({"points": p3, "degree": 3, "knots": [0, 0, 0, 0, 1, 1, 1]})),
        ("an extra knot", json!({"points": p3, "degree": 3, "knots": [0, 0, 0, 0, 1, 1, 1, 1, 1]})),
        ("not clamped at the start", json!({"points": p3, "degree": 3, "knots": [0, 0, 0, 0.2, 1, 1, 1, 1]})),
        ("not clamped at the end", json!({"points": p3, "degree": 3, "knots": [0, 0, 0, 0, 0.8, 1, 1, 1]})),
        (
            "decreasing knots",
            json!({"points": [[0, 0], [1, 2], [2, 2], [3, 0], [4, 1], [5, 0]], "degree": 3, "knots": [0, 0, 0, 0, 0.7, 0.3, 1, 1, 1, 1]}),
        ),
        (
            "an interior knot repeated more than the degree",
            json!({"points": [[0, 0], [1, 2], [2, 2], [3, 0], [4, 1], [5, 0], [6, 1], [7, 0]], "degree": 3, "knots": [0, 0, 0, 0, 0.5, 0.5, 0.5, 0.5, 1, 1, 1, 1]}),
        ),
        ("a knot that is not a number", json!({"points": p3, "degree": 3, "knots": [0, 0, 0, "x", 1, 1, 1, 1]})),
        ("too few points for the degree", json!({"points": [[0, 0], [1, 1], [2, 0]], "degree": 3, "knots": [0, 0, 0, 0, 1, 1, 1, 1]})),
        (
            "a weight that is not positive",
            json!({"points": [[2, 0], [2, 2], [0, 2]], "degree": 2, "knots": [0, 0, 0, 1, 1, 1], "weights": [1, 0, 1]}),
        ),
        ("one weight too few", json!({"points": [[2, 0], [2, 2], [0, 2]], "degree": 2, "knots": [0, 0, 0, 1, 1, 1], "weights": [1, 2]})),
        ("weights on a cubic", json!({"points": p3, "degree": 3, "knots": clamped4, "weights": weights_ok})),
    ];
    for (what, params) in cases {
        let mut s = new_sketch();
        let before = s.doc.to_json();
        let err = s.execute(KNOTS_CMD, &params).expect_err(what);
        assert!(matches!(err, EngineError::BadParams { .. }), "{what}: {err:?}");
        assert_eq!(s.doc.to_json(), before, "{what}: a refused spline changes nothing");
    }
}

#[test]
fn a_spline_with_its_own_knots_keeps_them_when_a_control_point_is_dragged() {
    let points = issue_points();
    let knots = issue_knots();
    let mut s = drawn(&points, 3, &knots);
    let sk = sketch(&s);
    let ci = spline_index(&sk);
    let CurveKind::Spline { pts, .. } = &sk.curves[ci].kind else { panic!() };
    let moved = sk.points[pts[10]].id.clone();
    let before_knots = sk.curves[ci].kind.clone();
    run(&mut s, "sketch.move_point", json!({"point": moved, "to": [points[10][0] + 1.5, points[10][1] - 2.0]}));
    let after = sketch(&s);
    let CurveKind::Spline { nurbs: before, .. } = &before_knots else { panic!() };
    let CurveKind::Spline { nurbs: Some(now), pts: now_pts, .. } = &after.curves[ci].kind else { panic!() };
    assert_eq!(now, before.as_ref().unwrap());
    assert!(after.point(now_pts[10]).unwrap().dist(Vec2::new(points[10][0] + 1.5, points[10][1] - 2.0)) < 1e-9);
}

#[test]
fn a_control_point_of_a_spline_with_its_own_knots_cannot_be_removed() {
    let points = issue_points();
    let mut s = drawn(&points, 3, &issue_knots());
    let sk = sketch(&s);
    let ci = spline_index(&sk);
    let CurveKind::Spline { pts, .. } = &sk.curves[ci].kind else { panic!() };
    let inner = sk.points[pts[10]].id.clone();
    let before = s.doc.to_json();
    let err = s.execute("sketch.spline.remove_point", &json!({"curve": sk.curves[ci].id, "point": inner})).expect_err("refused");
    assert!(matches!(err, EngineError::BadParams { .. }), "{err:?}");
    assert_eq!(s.doc.to_json(), before);
}
