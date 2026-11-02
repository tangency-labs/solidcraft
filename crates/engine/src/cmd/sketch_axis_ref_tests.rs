//! Constraints that take a line accept `x` / `y` for the sketch's own axes (issue #394): the
//! axis becomes one linked construction line on it, shared by every constraint on that axis,
//! reported in `created`, and left out of profiles. A real curve with that id wins.

use serde_json::{Value, json};
use solidcraft_geom::{Vec2, Vec3};
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

/// A point placed on a sketch axis while drawing (`on:x:…`, `on:y:…`, and the crossing of
/// both, `x:x+y:…`) is held on that axis's line: it stays there when the line it starts is
/// pulled about, as on any curve (#9330).
#[test]
fn a_point_drawn_on_a_sketch_axis_is_held_on_it() {
    use solidcraft_sketch::ConstraintKind;
    let mut s = on(json!("XY"));
    let r = run(&mut s, "sketch.line", json!({"points": ["on:x:5,0.3", [20, 10]], "ids": ["a"]}));
    assert!(r["sketch"]["solved"].as_bool().unwrap(), "{r}");
    run(&mut s, "sketch.line", json!({"points": ["on:y:-0.2,7", [-15, 12]], "ids": ["b"]}));
    run(&mut s, "sketch.line", json!({"points": ["x:x+y:0.1,-0.1", [6, -9]], "ids": ["c"]}));
    let sk = sketch(&s);
    let axes = axis_lines(&sk);
    assert_eq!(axes.len(), 2, "one line per axis, shared: {axes:?}");
    let line = |name: &str| axes.iter().find(|(n, _)| n == name).map(|(_, id)| sk.curve_index(id).unwrap()).unwrap();
    let held = |p: &str, c: usize| {
        let p = sk.resolve_point(p).unwrap();
        sk.constraints.iter().any(|k| k.kind == ConstraintKind::PointOnCurve { p, c })
    };
    assert!(held("a.start", line("x")) && held("b.start", line("y")), "{:?}", sk.constraints);
    assert!(held("c.start", line("x")) && held("c.start", line("y")));
    assert!(pt(&sk, "a.start").y.abs() < 1e-9 && pt(&sk, "b.start").x.abs() < 1e-9, "on the axes");
    assert!(pt(&sk, "c.start").len() < 1e-9, "at the origin");
    // Dragged along, the points stay on their axes.
    run(&mut s, "sketch.drag", json!({"entity": "a.start", "to": [9, 4]}));
    run(&mut s, "sketch.drag", json!({"entity": "b.start", "to": [3, -6]}));
    let sk = sketch(&s);
    assert!(pt(&sk, "a.start").y.abs() < 1e-7 && pt(&sk, "b.start").x.abs() < 1e-7, "{:?} {:?}", pt(&sk, "a.start"), pt(&sk, "b.start"));
}
