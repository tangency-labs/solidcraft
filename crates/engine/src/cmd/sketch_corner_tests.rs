//! Sketch Fillet and Chamfer at corners (#46): line–line, line–arc and arc–arc corners, the
//! corners of a constrained rectangle, several corners at once, and radii too large for the
//! corner (a clear error that leaves the sketch as it was).

use serde_json::{Value, json};

use crate::Session;

fn new_sketch() -> Session {
    let mut s = Session::default();
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
    s
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn curves(r: &Value) -> Vec<String> {
    r["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn sk(s: &Session) -> solidcraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

fn solved(s: &Session) -> bool {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().report.ok()
}

fn area(s: &Session) -> f64 {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().profiles.iter().map(|p| p.area).sum()
}

#[test]
fn fillet_and_chamfer_corners() {
    use std::f64::consts::PI;
    // A constrained 40 × 20 rectangle: a fillet r5 on one corner keeps it solved and closed,
    // with the area of the corner taken off.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let a0 = area(&s);
    run(&mut s, "sketch.fillet", json!({"a": c[0], "b": c[1], "radius": 5}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - (25.0 - PI * 25.0 / 4.0))).abs() < 1e-6, "{}", area(&s));
    // Several corners at once: the rest of the rectangle's corners.
    let pts: Vec<String> = {
        let k = sk(&s);
        let li = |id: &str| k.curve_index(id).unwrap();
        [(1, 2), (2, 3), (3, 0)]
            .iter()
            .map(|(x, y)| {
                let (cx, cy) = (&k.curves[li(&c[*x])].kind, &k.curves[li(&c[*y])].kind);
                let shared = cx.point_ids().into_iter().find(|p| cy.point_ids().contains(p)).unwrap();
                k.points[shared].id.clone()
            })
            .collect()
    };
    run(&mut s, "sketch.fillet", json!({"points": pts, "radius": 5}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - 4.0 * (25.0 - PI * 25.0 / 4.0))).abs() < 1e-6, "{}", area(&s));
    // Too large for a corner: a clear error, nothing changed.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let n = sk(&s).curves.len();
    let e = s.execute("sketch.fillet", &json!({"a": c[0], "b": c[1], "radius": 30})).unwrap_err().to_string();
    assert!(e.contains("too large") || e.contains("does not fit"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
    let e = s.execute("sketch.chamfer.equal_distance", &json!({"a": c[0], "b": c[1], "distance": 25})).unwrap_err().to_string();
    assert!(e.contains("too large") || e.contains("does not fit") || e.contains("longer"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
    // A line meeting an arc at a corner (down onto its top), and two arcs (a slot's end rounded
    // into a line).
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 20], [0, 10]]})))[0].clone();
    let a = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": format!("{l}.end"), "end": [10, 0], "through": [7.0710678, 7.0710678]})))
        [0]
    .clone();
    run(&mut s, "sketch.fillet", json!({"a": l, "b": a, "radius": 2}));
    assert!(solved(&s));
    let mut s = new_sketch();
    let a1 = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [-10, 0], "end": [0, 0], "through": [-5, -5]})))[0].clone();
    let a2 = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": format!("{a1}.end"), "end": [10, 0], "through": [5, 2]})))[0].clone();
    run(&mut s, "sketch.fillet", json!({"a": a1, "b": a2, "radius": 1}));
    assert!(solved(&s));
    // Chamfers keep a closed profile.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let a0 = area(&s);
    run(&mut s, "sketch.chamfer.equal_distance", json!({"a": c[0], "b": c[1], "distance": 4}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - 8.0)).abs() < 1e-6, "{}", area(&s));
}

/// A chamfer where the two lines have no corner is refused, as a fillet there is: two lines
/// lying on each other made a chamfer line of no length between their cut-back ends (found by a
/// local trim fuzzer), and two lines in line with each other have no corner either.
#[test]
fn a_chamfer_where_two_lines_lie_in_line_is_refused() {
    for far in [[0, 10], [0, 30]] {
        let mut s = new_sketch();
        let a = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 20]], "infer": false})))[0].clone();
        let b = curves(&run(&mut s, "sketch.line", json!({"points": [far, [0, 20]], "infer": false, "connect": false})))[0].clone();
        run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{a}.end"), "b": format!("{b}.end")}));
        let n = sk(&s).curves.len();
        for cmd in ["sketch.chamfer.equal_distance", "sketch.fillet"] {
            let p = if cmd == "sketch.fillet" { json!({"a": a, "b": b, "radius": 2}) } else { json!({"a": a, "b": b, "distance": 2}) };
            let e = s.execute(cmd, &p).unwrap_err().to_string();
            assert!(e.contains("parallel"), "{cmd} at {far:?}: {e}");
            assert_eq!(sk(&s).curves.len(), n, "{cmd} at {far:?}");
        }
    }
}

/// A round that would use up a whole arc is refused, as one too large for two lines is: a
/// quarter arc of radius 5 running into a line along it, rounded with radius 5, was left as an
/// arc of no length (found by a local trim fuzzer).
#[test]
fn a_fillet_that_would_use_up_an_arc_is_refused() {
    let mut s = new_sketch();
    // Centre (15, 5): from (10, 5) down to (15, 0), where the line along y = 0 leaves it.
    let through = [15.0 - 2.5 * 2f64.sqrt(), 5.0 - 2.5 * 2f64.sqrt()];
    let a = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [10, 5], "end": [15, 0], "through": through})))[0].clone();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [format!("{a}.end"), [-35, 0]], "infer": false})))[0].clone();
    let n = sk(&s).curves.len();
    let e = s.execute("sketch.fillet", &json!({"a": a, "b": l, "radius": 5})).unwrap_err().to_string();
    assert!(e.contains("does not fit"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
}

/// A round where a line runs on along an arc's end, off only by rounding, is refused: there is no
/// corner, and the round came out as an arc about 1e-7 mm long (found by a local trim fuzzer,
/// after earlier rounds left the line a hair off the arc's tangent).
#[test]
fn a_fillet_where_a_line_runs_on_smoothly_from_an_arc_is_refused() {
    let mut s = new_sketch();
    let h = 0.5 * 2f64.sqrt();
    let a = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [1, 0], "end": [0, 1], "through": [h, h]})))[0].clone();
    // Off the arc's tangent at (0, 1) by 1e-7 rad.
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [format!("{a}.end"), [-10, 1.000001]], "infer": false})))[0].clone();
    let n = sk(&s).curves.len();
    let e = s.execute("sketch.fillet", &json!({"a": a, "b": l, "radius": 5})).unwrap_err().to_string();
    assert!(e.contains("smoothly") || e.contains("does not fit"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
}

/// The ids of the constraints driven by the model parameter `name`.
fn driven_by(s: &Session, name: &str) -> Vec<String> {
    sk(s).constraints.iter().filter(|c| c.param.as_deref() == Some(name)).map(|c| c.id.clone()).collect()
}

#[test]
fn fully_defined_rectangles_keep_their_size_when_all_corners_are_filleted() {
    for center in [false, true] {
        for square in [false, true] {
            let mut s = new_sketch();
            let height = if square { 40.0 } else { 20.0 };
            let r = if center {
                run(&mut s, "sketch.rectangle.center", json!({"center": [0, 0], "corner": [20, height / 2.0]}))
            } else {
                run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, height]}))
            };
            let anchor = if center { "p5" } else { "p1" };
            run(&mut s, "sketch.constraint.coincident", json!({"a": anchor, "b": "origin"}));
            let width = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
            if square {
                run(&mut s, "sketch.constraint.equal", json!({"a": "l1", "b": "l2"}));
            } else {
                run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": height}));
            }
            let info = run(&mut s, "sketch.inspect", json!({}));
            assert_eq!(info["dof"], 0, "{info}");
            let points: Vec<Value> = ["bottom_left", "bottom_right", "top_right", "top_left"].iter().map(|name| r["corners"][name].clone()).collect();
            for (i, point) in points.iter().enumerate() {
                let f = run(&mut s, "sketch.fillet", json!({"point": point, "radius": 3}));
                assert_eq!(f["sketch"]["solved"], true, "center={center}, square={square}: {f}");
                assert_eq!(f["sketch"]["dof"], 0, "center={center}, square={square}: {f}");
                let expected = 40.0 * height - (i + 1) as f64 * (9.0 - std::f64::consts::PI * 9.0 / 4.0);
                assert!((area(&s) - expected).abs() < 1e-5, "corner={i}, center={center}, square={square}: {} != {expected}", area(&s));
            }
            let expected = 40.0 * height - 4.0 * (9.0 - std::f64::consts::PI * 9.0 / 4.0);
            assert!((area(&s) - expected).abs() < 1e-5, "center={center}, square={square}: {} != {expected}", area(&s));
            run(&mut s, "parameters.change", json!({"name": width["param"], "expression": "50 mm"}));
            assert!(solved(&s));
            let expected = 50.0 * if square { 50.0 } else { height } - 4.0 * (9.0 - std::f64::consts::PI * 9.0 / 4.0);
            assert!((area(&s) - expected).abs() < 1e-5, "center={center}, square={square}: {} != {expected}", area(&s));
            let id = s.active_sketch.unwrap();
            let saved = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
            assert_eq!(saved.sketch(id).unwrap(), s.doc.sketch(id).unwrap());
            run(&mut s, "edit.undo", json!({}));
            run(&mut s, "edit.redo", json!({}));
            assert!((area(&s) - expected).abs() < 1e-5);
        }
    }
}

#[test]
fn filleting_a_side_keeps_its_original_midpoint() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-20, 0], "p1": [20, 30]}));
    run(&mut s, "sketch.constraint.midpoint", json!({"point": "origin", "line": "l1"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 30}));
    for point in ["p1", "p2"] {
        let f = run(&mut s, "sketch.fillet", json!({"point": point, "radius": 3}));
        assert_eq!(f["sketch"]["solved"], true, "{f}");
        assert_eq!(f["sketch"]["dof"], 0, "{f}");
        let (lo, hi) = sk(&s).bounds().unwrap();
        assert!((lo.x + 20.0).abs() < 1e-6 && (hi.x - 20.0).abs() < 1e-6, "{lo:?} {hi:?}");
    }
}

/// The spout of a basin: a vertical riser and a horizontal top, the top's length dimensioned
/// to the corner and the corner's height dimensioned from the origin. Filleting the corner used
/// to drop the height and leave the length on the shortened straight part, so the top came out
/// a fillet radius too long. The corner now stays as a virtual sharp corner (a point
/// on both lines' extensions) and the dimensions measure to it.
#[test]
fn a_fillet_keeps_the_dimensions_of_its_corner_on_a_virtual_sharp() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 100], [-80, 100]], "ids": ["riser", "top"]}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "riser", "mode": "vertical"}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "top", "mode": "horizontal"}));
    let height = run(&mut s, "sketch.dimension", json!({"entities": ["origin", "riser.end"], "type": "vertical", "value": 100}));
    let height = height["param"].as_str().unwrap().to_string();
    let len = run(&mut s, "sketch.dimension", json!({"entities": ["top"], "value": 80}));
    let len = len["param"].as_str().unwrap().to_string();
    let (height_ids, len_ids) = (driven_by(&s, &height), driven_by(&s, &len));
    let f = run(&mut s, "sketch.fillet", json!({"point": "riser.end", "radius": 20}));
    assert!(solved(&s), "{f}");
    // Both dimensions are kept (same ids) and reported as moved to the virtual corner.
    assert_eq!(driven_by(&s, &height), height_ids);
    assert_eq!(driven_by(&s, &len), len_ids);
    let corner = f["virtual_corners"][0].as_str().unwrap_or_default().to_string();
    assert!(!corner.is_empty(), "{f}");
    let moved: Vec<String> = f["moved_to_virtual_corner"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    for id in height_ids.iter().chain(&len_ids) {
        assert!(moved.contains(id), "{id}: {f}");
    }
    // The virtual corner sits where the corner was, and the part keeps its size.
    let k = sk(&s);
    let vc = k.point(k.point_index(&corner).unwrap()).unwrap();
    assert!(vc.dist(solidcraft_geom::Vec2::new(0.0, 100.0)) < 1e-6, "{vc:?}");
    let top_end = k.point(k.resolve_point("top.end").unwrap()).unwrap();
    assert!(top_end.dist(solidcraft_geom::Vec2::new(-80.0, 100.0)) < 1e-6, "{top_end:?}");
    let (lo, hi) = k.bounds().unwrap();
    assert!((hi.x - lo.x - 80.0).abs() < 1e-6 && (hi.y - lo.y - 100.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // It is drawn with extension lines from both lines' ends to it.
    let vcs = k.virtual_corners();
    assert_eq!(vcs.len(), 1, "{vcs:?}");
    assert_eq!(vcs[0].1.len(), 2, "{vcs:?}");
    assert!(vcs[0].1.iter().all(|(a, b)| a.dist(vc) < 1e-9 && (b.dist(vc) - 20.0).abs() < 1e-6), "{vcs:?}");
    // The dimensions still drive the part, measured to the virtual corner.
    run(&mut s, "parameters.change", json!({"name": len, "expression": "90 mm"}));
    run(&mut s, "parameters.change", json!({"name": height, "expression": "120 mm"}));
    assert!(solved(&s));
    let (lo, hi) = sk(&s).bounds().unwrap();
    assert!((hi.x - lo.x - 90.0).abs() < 1e-6 && (hi.y - lo.y - 120.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // It is kept in the design file, and undo and redo take it away and bring it back.
    let id = s.active_sketch.unwrap();
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(doc.sketch(id).unwrap(), s.doc.sketch(id).unwrap());
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(sk(&s).point_index(&corner).is_none());
    run(&mut s, "edit.redo", json!({}));
    assert!(sk(&s).point_index(&corner).is_some());
    // A fillet without anything measuring to its corner leaves no virtual corner behind.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [10, 100], [-80, 100]], "ids": ["riser", "top"]}));
    let n = sk(&s).points.len();
    let f = run(&mut s, "sketch.fillet", json!({"point": "riser.end", "radius": 20}));
    assert_eq!(f["virtual_corners"], json!([]), "{f}");
    // Corner gone, two tangent points and the arc's centre added.
    assert_eq!(sk(&s).points.len(), n + 2);
}

/// Lines meeting almost straight or almost folded back: a fillet or chamfer with dimensions on
/// the corner either works or refuses with an error, never a panic.
#[test]
fn a_fillet_of_a_nearly_straight_or_folded_corner_with_dimensions_does_not_panic() {
    for (x, y) in [(100.0, 0.05), (100.0, -0.05), (100.0, 1e-9), (-100.0, 0.05), (-100.0, 1e-9)] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [100, 0], [100.0 + x, y]], "ids": ["a", "b"]}));
        run(&mut s, "sketch.dimension", json!({"entities": ["a"], "value": 100}));
        for r in [0.01, 1.0, 50.0] {
            let _ = s.execute("sketch.fillet", &json!({"point": "a.end", "radius": r}));
            let _ = s.execute("sketch.chamfer.equal_distance", &json!({"point": "a.end", "distance": r}));
        }
        assert!(sk(&s).points.iter().all(|p| p.pos.is_finite()));
    }
}

/// Two rectangles drawn apart, each with a corner point of its own at one place (both held on the
/// origin): the two lines meeting there make one corner, and rounding it joins the two outlines
/// into one instead of refusing that the lines share no corner point.
#[test]
fn a_fillet_joins_two_profiles_that_have_a_corner_point_each_at_one_place() {
    use std::f64::consts::PI;
    let rectangles = || {
        let mut s = new_sketch();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-26.2, -18.8], "p1": [0, 0]}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [24.8, 27.8]}));
        run(&mut s, "sketch.constraint.coincident", json!({"a": "p3", "b": "origin"}));
        run(&mut s, "sketch.constraint.coincident", json!({"a": "p5", "b": "origin"}));
        s
    };
    let mut s = rectangles();
    let k = sk(&s);
    let at_origin = k.points.iter().filter(|p| p.pos.dist(solidcraft_geom::Vec2::ZERO) < 1e-9).count();
    assert_eq!(at_origin, 3, "the origin and a corner point of each rectangle: {:?}", k.points.iter().map(|p| &p.id).collect::<Vec<_>>());
    let union = 26.2 * 18.8 + 24.8 * 27.8;
    let r = 5.0;
    let f = run(&mut s, "sketch.fillet", json!({"a": "l2", "b": "l5", "radius": r}));
    assert!(solved(&s), "{f}");
    assert_eq!(s.model.state().sketch(s.active_sketch.unwrap()).unwrap().profiles.len(), 1, "one outline round both rectangles: {f}");
    // The round fills the neck between the two rectangles, so the outline gains that sliver.
    let expected = union + r * r * (1.0 - PI / 4.0);
    assert!((area(&s) - expected).abs() < 1e-6, "{} != {expected}", area(&s));
    // A chamfer joins them the same way.
    let mut s = rectangles();
    run(&mut s, "sketch.chamfer.equal_distance", json!({"a": "l2", "b": "l5", "distance": 4}));
    assert!(solved(&s));
    assert_eq!(s.model.state().sketch(s.active_sketch.unwrap()).unwrap().profiles.len(), 1);
    assert!((area(&s) - (union + 8.0)).abs() < 1e-6, "{} != {}", area(&s), union + 8.0);
}
