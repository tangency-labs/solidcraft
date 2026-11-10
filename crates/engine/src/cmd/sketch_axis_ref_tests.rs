//! Constraints that take a line accept `x` / `y` for the sketch's own axes (issue #394): the
//! axis becomes one linked construction line on it, shared by every constraint on that axis,
//! reported in `created`, and left out of profiles. A real curve with that id wins.

use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{CurveKind, LinkSource, Sketch};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch(s: &Session) -> Sketch {
    s.doc.sketch(s.active_sketch.unwrap()).unwrap().clone()
}

fn on(plane: Value) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({ "plane": plane }));
    s
}

fn pt(sk: &Sketch, r: &str) -> Vec2 {
    sk.point(sk.resolve_point(r).unwrap()).unwrap()
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The sketch's axis lines: (axis name, curve id).
fn axis_lines(sk: &Sketch) -> Vec<(String, String)> {
    sk.links
        .iter()
        .filter_map(|l| match &l.source {
            LinkSource::SketchAxis { name } => Some((name.clone(), l.id.clone())),
            _ => None,
        })
        .flat_map(|(name, link)| sk.link_curves(&link).into_iter().map(move |c| (name.clone(), sk.curves[c].id.clone())))
        .collect()
}

fn assert_axis_line(sk: &Sketch, id: &str, dir: Vec2) {
    let c = sk.curve_index(id).unwrap();
    let curve = &sk.curves[c];
    assert!(curve.construction, "axis line {id} is construction");
    assert!(sk.is_linked_curve(c), "axis line {id} is linked");
    let CurveKind::Line { a, b } = curve.kind else { panic!("axis line {id} is a line") };
    for p in [sk.point(a).unwrap(), sk.point(b).unwrap()] {
        assert!(p.cross(dir).abs() < 1e-9, "{id} end {p:?} lies on the axis {dir:?}");
    }
}

#[test]
fn issue_394_symmetry_about_the_sketch_y_axis() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.line", json!({"points": [[-4, 0], [10, 0]], "ids": ["a"]}));
    let r = run(&mut s, "sketch.constraint.symmetry", json!({"a": "a.start", "b": "a.end", "line": "y"}));
    assert!(r["sketch"]["solved"].as_bool().unwrap(), "{r}");
    let sk = sketch(&s);
    let (start, end) = (pt(&sk, "a.start"), pt(&sk, "a.end"));
    assert!((start.x + end.x).abs() < 1e-7 && (start.y - end.y).abs() < 1e-7, "mirrored about y: {start:?} {end:?}");
    assert!((start.x - -4.0).abs() < 1e-7, "the first entity stays: {start:?}");
    let axes = axis_lines(&sk);
    assert_eq!(axes.len(), 1, "{axes:?}");
    let (name, line) = &axes[0];
    assert_eq!(name, "y");
    assert_axis_line(&sk, line, Vec2::Y);
    assert_eq!(strs(&r["created"]["curves"]), vec![line.clone()], "the axis line is reported in created: {r}");
    assert_eq!(strs(&r["created"]["constraints"]).len(), 1, "{r}");

    // A second constraint on the same axis reuses its line.
    run(&mut s, "sketch.line", json!({"points": [[-3, 5], [8, 5]], "ids": ["b"]}));
    let r = run(&mut s, "sketch.constraint.symmetry", json!({"a": "b.start", "b": "b.end", "line": "y"}));
    assert!(strs(&r["created"]["curves"]).is_empty(), "{r}");
    assert_eq!(axis_lines(&sketch(&s)).len(), 1);
}

#[test]
fn a_real_curve_named_like_an_axis_wins() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.line", json!({"points": [[5, -20], [5, 20]], "ids": ["y"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "y"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [12, 0]], "ids": ["a"]}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": "a.start", "b": "a.end", "line": "y"}));
    let sk = sketch(&s);
    assert!(axis_lines(&sk).is_empty(), "no axis line when a curve is called y");
    let (start, end) = (pt(&sk, "a.start"), pt(&sk, "a.end"));
    assert!((start.x + end.x - 10.0).abs() < 1e-7, "mirrored about x = 5: {start:?} {end:?}");
}

/// Tangent also takes a curve's end point (#12092), but `x` and `y` still name the axes before
/// their lines exist: a point called `x` does not take the axis's place.
#[test]
fn a_point_named_like_an_axis_leaves_tangent_on_the_axis() {
    for (a, b) in [("c", "x"), ("x", "c")] {
        let mut s = on(json!("XY"));
        run(&mut s, "sketch.point", json!({"point": [3, 7], "id": "x", "connect": false}));
        run(&mut s, "sketch.circle.center", json!({"center": [10, 4], "radius": 2, "id": "c"}));
        run(&mut s, "sketch.constraint.tangent", json!({"a": a, "b": b}));
        let sk = sketch(&s);
        assert_eq!(axis_lines(&sk).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["x"], "{a}, {b}: the x axis's line");
        let CurveKind::Circle { c: centre, r } = sk.curves[sk.curve_index("c").unwrap()].kind else { panic!() };
        assert!((sk.point(centre).unwrap().y.abs() - r).abs() < 1e-7, "{a}, {b}: tangent to x");
        assert!(pt(&sk, "x").dist(Vec2::new(3.0, 7.0)) < 1e-9, "{a}, {b}: the point called x moved");
    }
}

#[test]
fn a_rectangle_centred_on_the_origin_keeps_one_profile() {
    let mut s = on(json!("XY"));
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 1], "p1": [12, 7]}));
    let sk = sketch(&s);
    let lines = strs(&r["curves"]);
    let dir = |id: &String| {
        let CurveKind::Line { a, b } = sk.curves[sk.curve_index(id).unwrap()].kind else { panic!() };
        sk.point(b).unwrap() - sk.point(a).unwrap()
    };
    let horizontal = lines.iter().find(|l| dir(l).y.abs() < 1e-9).unwrap().clone();
    let vertical = lines.iter().find(|l| dir(l).x.abs() < 1e-9).unwrap().clone();
    run(&mut s, "sketch.constraint.symmetry", json!({"a": format!("{horizontal}.start"), "b": format!("{horizontal}.end"), "line": "y"}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": format!("{vertical}.start"), "b": format!("{vertical}.end"), "line": "x"}));
    let sk = sketch(&s);
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for l in &lines {
        for end in ["start", "end"] {
            let p = pt(&sk, &format!("{l}.{end}"));
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    assert!((lo.x + hi.x).abs() < 1e-7 && (lo.y + hi.y).abs() < 1e-7, "centred on the origin: {lo:?} {hi:?}");
    assert_eq!(axis_lines(&sk).len(), 2);
    // The axis lines cross the rectangle but are construction: still one profile.
    assert_eq!(run(&mut s, "sketch.solve", json!({}))["profiles"], json!(1));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
}

#[test]
fn sketch_axes_on_a_plane_off_the_world_axes() {
    // A plane whose own axes are none of the world axes: x along world Y, y along world Z,
    // origin away from the world origin.
    let origin = Vec3::new(5.0, 7.0, 9.0);
    let mut s = on(json!({"origin": [5, 7, 9], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}));
    run(&mut s, "sketch.line", json!({"points": [[3, -6], [3, 2]], "ids": ["a"]}));
    let r = run(&mut s, "sketch.constraint.symmetry", json!({"a": "a.start", "b": "a.end", "line": "x"}));
    assert!(r["sketch"]["solved"].as_bool().unwrap(), "{r}");
    let sk = sketch(&s);
    let (start, end) = (pt(&sk, "a.start"), pt(&sk, "a.end"));
    assert!((start.y + end.y).abs() < 1e-7 && (start.x - end.x).abs() < 1e-7, "mirrored about the sketch x axis: {start:?} {end:?}");
    let axes = axis_lines(&sk);
    assert_eq!(axes.len(), 1);
    assert_axis_line(&sk, &axes[0].1, Vec2::X);

    // After a recompute the axis line is still the sketch's x axis, in world space too.
    run(&mut s, "sketch.finish", json!({}));
    let st = s.model.state();
    let id = s.doc.find_feature("Sketch1").map(|f| f.id).unwrap();
    let ss = st.sketch(id).unwrap();
    let c = ss.sketch.curve_index(&axes[0].1).unwrap();
    let CurveKind::Line { a, b } = ss.sketch.curves[c].kind else { panic!() };
    for p in [a, b] {
        let w = ss.plane.to_world(ss.sketch.point(p).unwrap()) - origin;
        assert!(w.x.abs() < 1e-9 && w.z.abs() < 1e-9, "on the plane's x axis (world Y through its origin): {w:?}");
    }
}

#[test]
fn other_line_constraints_take_sketch_axes() {
    let mut s = on(json!("XZ"));
    run(&mut s, "sketch.line", json!({"points": [[1, 1], [6, 3]], "ids": ["p"]}));
    run(&mut s, "sketch.constraint.parallel", json!({"a": "p", "b": "x"}));
    let sk = sketch(&s);
    assert!((pt(&sk, "p.start").y - pt(&sk, "p.end").y).abs() < 1e-7, "parallel to x");

    run(&mut s, "sketch.line", json!({"points": [[-2, 1], [-1, 6]], "ids": ["q"]}));
    run(&mut s, "sketch.constraint.perpendicular", json!({"a": "x", "b": "q"}));
    let sk = sketch(&s);
    assert!((pt(&sk, "q.start").x - pt(&sk, "q.end").x).abs() < 1e-7, "perpendicular to x");

    run(&mut s, "sketch.line", json!({"points": [[2, -5], [4, -8]], "ids": ["r"]}));
    run(&mut s, "sketch.constraint.collinear", json!({"a": "r", "b": "y"}));
    let sk = sketch(&s);
    assert!(pt(&sk, "r.start").x.abs() < 1e-7 && pt(&sk, "r.end").x.abs() < 1e-7, "on the y axis");

    run(&mut s, "sketch.circle.center", json!({"center": [10, 4], "radius": 2, "id": "c"}));
    run(&mut s, "sketch.constraint.tangent", json!({"a": "c", "b": "x"}));
    let sk = sketch(&s);
    let c = sk.curve_index("c").unwrap();
    let CurveKind::Circle { c: centre, r } = sk.curves[c].kind else { panic!() };
    assert!((sk.point(centre).unwrap().y.abs() - r).abs() < 1e-7, "tangent to x");

    // A point onto an axis, either pick order.
    run(&mut s, "sketch.point", json!({"point": [7, 9], "id": "u"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "u", "b": "x"}));
    assert!(pt(&sketch(&s), "u").y.abs() < 1e-7);
    run(&mut s, "sketch.point", json!({"point": [7, 9], "id": "v"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "y", "b": "v"}));
    assert!(pt(&sketch(&s), "v").x.abs() < 1e-7);

    // Still one line per axis.
    let mut names: Vec<String> = axis_lines(&sketch(&s)).into_iter().map(|(n, _)| n).collect();
    names.sort();
    assert_eq!(names, ["x", "y"]);
}

/// A point picked with an origin axis (in the view or the browser) goes onto the axis's
/// projection, either pick order, and stays on it when the point is dragged.
#[test]
fn coincident_takes_an_origin_axis_reference() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.point", json!({"point": [7, 9], "id": "u"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "u", "b": {"axis": "X"}}));
    assert!(pt(&sketch(&s), "u").y.abs() < 1e-7, "on X");
    run(&mut s, "sketch.point", json!({"point": [3, 4], "id": "v"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": {"axis": "Y"}, "b": "v"}));
    assert!(pt(&sketch(&s), "v").x.abs() < 1e-7, "on Y");
    // Z is seen end on: the point goes to where it pierces the sketch.
    run(&mut s, "sketch.point", json!({"point": [5, -2], "id": "w"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "w", "b": {"axis": "Z"}}));
    assert!(pt(&sketch(&s), "w").len() < 1e-7, "on Z");
    let from = pt(&sketch(&s), "u");
    run(&mut s, "sketch.drag", json!({"entity": "u", "from": [from.x, from.y], "to": [12, 6]}));
    assert!(pt(&sketch(&s), "u").y.abs() < 1e-7, "still on X after a drag");
}

/// A line dimensioned to an origin axis is their distance, not the line's length, and the
/// line stays parallel to the axis.
#[test]
fn dimension_measures_a_line_to_an_origin_axis() {
    // Either pick order: the axis is the reference and the line is kept parallel to it.
    for entities in [json!(["l", {"axis": "X"}]), json!([{"axis": "X"}, "l"])] {
        let mut s = on(json!("XY"));
        run(&mut s, "sketch.line", json!({"points": [[-4, 7], [10, 7]], "ids": ["l"]}));
        let out = run(&mut s, "sketch.dimension", json!({"entities": entities, "value": "12"}));
        let sk = sketch(&s);
        assert!((pt(&sk, "l.start").y.abs() - 12.0).abs() < 1e-7, "12 from X ({entities})");
        assert!((pt(&sk, "l.end").y - pt(&sk, "l.start").y).abs() < 1e-7, "kept parallel to X, so all of it is 12 from X ({entities})");
        assert!((pt(&sk, "l.end").dist(pt(&sk, "l.start")) - 12.0).abs() > 1.0, "not the line's length");
        // What it measures once solved, not the 7 before (the Parallel after it is not it).
        assert!((out["measured"].as_f64().unwrap_or(f64::NAN) - 12.0).abs() < 1e-6, "{out}");
    }
}

/// A line dimensioned to a projected model edge keeps its old dimension: a distance from one
/// end, with no Parallel to the edge, so a later edit can still turn the line.
#[test]
fn a_line_dimensioned_to_a_projected_edge_gets_no_parallel() {
    for entities in [json!(["l", {"edge": [5, 0, 0]}]), json!([{"edge": [5, 0, 0]}, "l"])] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.line", json!({"points": [[-4, 7], [10, 7]], "ids": ["l"]}));
        run(&mut s, "sketch.dimension", json!({"entities": entities, "value": "12"}));
        let sk = sketch(&s);
        assert!(
            !sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::Parallel { .. })),
            "{entities}: {:?}",
            sk.constraints
        );
    }
}

/// A driven dimension to an origin axis reports what it measures on the solved sketch.
#[test]
fn a_driven_dimension_to_an_origin_axis_reports_the_solved_value() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.line", json!({"points": [[-4, 7], [10, 7]], "ids": ["l"]}));
    let out = run(&mut s, "sketch.dimension", json!({"entities": ["l", {"axis": "X"}], "driven": true}));
    assert_eq!(out["driven"], json!(true), "{out}");
    assert!((out["measured"].as_f64().unwrap_or(f64::NAN) - 7.0).abs() < 1e-6, "{out}");
}

/// A line parallel only to another free line still gets its Parallel to the axis: the two free
/// lines could otherwise turn together and leave all but one end of the line off the distance.
/// Once that other line's direction is fixed, the line's is too, and no Parallel is added.
#[test]
fn a_line_parallel_to_a_free_line_still_keeps_parallel_to_the_axis() {
    let axis_parallels = |sk: &Sketch| {
        let axis: Vec<usize> = (0..sk.curves.len()).filter(|c| sk.curves[*c].link.is_some()).collect();
        sk.constraints
            .iter()
            .filter(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::Parallel { a, b } if axis.contains(&a) || axis.contains(&b)))
            .count()
    };
    for fixed in [false, true] {
        let mut s = on(json!("XY"));
        run(&mut s, "sketch.line", json!({"points": [[-4, 7], [10, 7]], "ids": ["l"]}));
        run(&mut s, "sketch.line", json!({"points": [[-4, 20], [10, 20]], "ids": ["m"]}));
        run(&mut s, "sketch.constraint.parallel", json!({"a": "l", "b": "m"}));
        if fixed {
            run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "m", "mode": "horizontal"}));
        }
        run(&mut s, "sketch.dimension", json!({"entities": ["l", {"axis": "X"}], "value": "12"}));
        let sk = sketch(&s);
        assert_eq!(axis_parallels(&sk), usize::from(!fixed), "m fixed: {fixed}: {:?}", sk.constraints);
        assert!((pt(&sk, "l.start").y.abs() - 12.0).abs() < 1e-7 && (pt(&sk, "l.end").y.abs() - 12.0).abs() < 1e-7, "all of l is 12 from X");
    }
}

/// An origin axis is `{"axis": "X"|"Y"|"Z"}`; lowercase `x`/`y` name the sketch's own axes, so
/// `{"axis": "x"}` is refused and changes nothing.
#[test]
fn a_lowercase_origin_axis_object_is_refused() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.line", json!({"points": [[-4, 7], [10, 7]], "ids": ["l"]}));
    let before = s.doc.to_json();
    assert!(s.execute("sketch.dimension", &json!({"entities": ["l", {"axis": "x"}], "value": "12"})).is_err());
    assert_eq!(s.doc.to_json(), before);
}

/// A line whose angle is already set keeps it: no Parallel is added to the axis.
#[test]
fn a_line_with_an_angle_gets_no_parallel_to_the_axis() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.line", json!({"points": [[-4, 7], [10, 7]], "ids": ["l"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l"], "type": "angle", "value": "0 deg"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l", {"axis": "X"}], "value": "12"}));
    let sk = sketch(&s);
    assert!(!sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::Parallel { .. })), "{:?}", sk.constraints);
}

#[test]
fn the_axis_survives_save_and_load() {
    let mut s = on(json!("YZ"));
    run(&mut s, "sketch.line", json!({"points": [[-4, 0], [10, 0]], "ids": ["a"]}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": "a.start", "b": "a.end", "line": "y"}));
    run(&mut s, "sketch.finish", json!({}));
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let id = doc.find_feature("Sketch1").map(|f| f.id).unwrap();
    let sk = doc.sketch(id).unwrap();
    assert_eq!(axis_lines(sk).len(), 1);
    assert_eq!(sk.links.iter().map(|l| &l.source).collect::<Vec<_>>(), [&LinkSource::SketchAxis { name: "y".into() }]);
}

#[test]
fn bad_line_references_are_errors_and_change_nothing() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.line", json!({"points": [[-4, 0], [10, 0]], "ids": ["a"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 3], [9, 4]], "ids": ["b"]}));
    let before = s.doc.to_json();
    let hostile =
        [json!("Y"), json!("X"), json!("z"), json!(""), json!("x.start"), json!(" y"), json!(5), json!(null), json!(["y"]), json!({"axis": "y"})];
    for line in &hostile {
        assert!(s.execute("sketch.constraint.symmetry", &json!({"a": "a.start", "b": "a.end", "line": line})).is_err(), "symmetry line {line}");
        for cmd in ["sketch.constraint.parallel", "sketch.constraint.perpendicular", "sketch.constraint.collinear", "sketch.constraint.tangent"] {
            assert!(s.execute(cmd, &json!({"a": "b", "b": line})).is_err(), "{cmd} b {line}");
        }
        assert!(s.execute("sketch.constraint.coincident", &json!({"a": "b.start", "b": line})).is_err(), "coincident b {line}");
    }
    // Constraints that do not take an axis still refuse x / y, and leave no axis line behind.
    for cmd in ["sketch.constraint.equal", "sketch.constraint.concentric"] {
        assert!(s.execute(cmd, &json!({"a": "b", "b": "x"})).is_err(), "{cmd}");
    }
    assert!(s.execute("sketch.constraint.midpoint", &json!({"point": "b.start", "line": "y"})).is_err());
    assert!(s.execute("sketch.constraint.horizontal_vertical", &json!({"line": "x"})).is_err());
    assert_eq!(s.doc.to_json(), before, "failed commands change nothing");
    // A constraint the sketch refuses rolls the axis line back with it.
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "b", "mode": "horizontal"}));
    let before_redundant = s.doc.to_json();
    // (A redundant one is kept unless refused: refused here.)
    assert!(s.execute("sketch.constraint.parallel", &json!({"a": "b", "b": "x", "if_redundant": "refuse"})).is_err(), "already horizontal");
    assert_eq!(s.doc.to_json(), before_redundant);
    assert!(axis_lines(&sketch(&s)).is_empty());
}

/// #2364: a dimension takes `x` / `y` too. A line parallel to the axis gets its distance to it,
/// a point its distance to it, and both share the one axis line.
#[test]
fn issue_2364_dimension_to_a_sketch_axis() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.line", json!({"points": [[5, 0], [5, 10]], "ids": ["a"]}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "a"}));
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["a", "y"], "value": 12}));
    assert!(r["sketch"]["solved"].as_bool().unwrap(), "{r}");
    let sk = sketch(&s);
    for end in ["a.start", "a.end"] {
        let p = pt(&sk, end);
        assert!((p.x.abs() - 12.0).abs() < 1e-7, "{end} is 12 from the y axis: {p:?}");
    }
    let axes = axis_lines(&sk);
    assert_eq!(axes.len(), 1, "{axes:?}");
    assert_eq!(axes[0].0, "y");
    assert_axis_line(&sk, &axes[0].1, Vec2::Y);

    // A point to the other axis; the y axis line is not made again.
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["a.end", "x"], "value": 7}));
    assert!(r["sketch"]["solved"].as_bool().unwrap(), "{r}");
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["y", "a.start"], "value": 12, "driven": true}));
    assert!(r["sketch"]["solved"].as_bool().unwrap(), "{r}");
    let sk = sketch(&s);
    assert!((pt(&sk, "a.end").y.abs() - 7.0).abs() < 1e-7, "{:?}", pt(&sk, "a.end"));
    let mut names: Vec<String> = axis_lines(&sk).into_iter().map(|(n, _)| n).collect();
    names.sort();
    assert_eq!(names, ["x", "y"], "one line per axis");
}

/// #2364: a real curve named like an axis is still the curve, not the axis.
#[test]
fn issue_2364_a_curve_named_like_an_axis_wins_in_a_dimension() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [8, 0]], "ids": ["y"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["y"], "value": 20}));
    let sk = sketch(&s);
    let (a, b) = (pt(&sk, "y.start"), pt(&sk, "y.end"));
    assert!(((b - a).len() - 20.0).abs() < 1e-7, "the line `y` is 20 long: {a:?} {b:?}");
    assert!(axis_lines(&sk).is_empty(), "no axis line was made");
}

/// #3137: a dimension takes `x` / `y` for the sketch's axes too, from a circle's centre point,
/// and both dimensions share the axis line.
#[test]
fn issue_3137_dimensions_from_a_circle_centre_to_the_sketch_axes() {
    let mut s = on(json!("XY"));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 20], "radius": 5, "id": "c"}));
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c.center", "y"], "value": 40}));
    let line = strs(&r["created"]["curves"]);
    assert_eq!(line.len(), 1, "the y axis line is created: {r}");
    run(&mut s, "sketch.dimension", json!({"entities": ["c.center", "x"], "value": 15}));
    let sk = sketch(&s);
    let o = pt(&sk, "c.center");
    assert!((o.x - 40.0).abs() < 1e-7 && (o.y - 15.0).abs() < 1e-7, "the centre is 40 from y and 15 from x: {o:?}");
    let axes = axis_lines(&sk);
    assert_eq!(axes.len(), 2, "{axes:?}");
    assert_axis_line(&sk, &line[0], Vec2::Y);
    assert!(sk.radius(sk.curve_index("c").unwrap()).is_some_and(|r| (r - 5.0).abs() < 1e-7), "the circle keeps its radius");
}
