//! Sketch points placed by parameters: a coordinate typed as an expression fixes the point
//! where it comes to, and the point follows the parameters without a dimension or a solve.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn no_trouble(s: &Session) {
    for r in &s.model.results {
        assert!(r.error.is_none() && r.warning.is_none(), "{}: {:?} {:?}", r.name, r.error, r.warning);
    }
}

/// Where point `id` of sketch `S` is in the evaluated model.
fn solved(s: &Session, id: &str) -> Vec2 {
    let f = s.doc.find_feature("S").unwrap().id;
    let st = s.model.state();
    let sk = &st.sketch(f).unwrap().sketch;
    sk.points.iter().find(|q| q.id == id).unwrap().pos
}

fn reopened(s: &Session) -> Session {
    Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap())
}

#[test]
fn an_outline_through_points_typed_as_expressions_follows_its_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "parameters.add", json!({"name": "H", "expression": "30 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], ["W", 0], ["W", "H / 2 + H / 2"], [0, "H"]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 10}));
    no_trouble(&s);
    assert!((volume(&mut s) - 40.0 * 30.0 * 10.0).abs() < 1e-6);
    // One change of both: the outline follows without a dimension.
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "70 mm"}));
    run(&mut s, "parameters.change", json!({"name": "H", "expression": "2 in"}));
    no_trouble(&s);
    assert!((volume(&mut s) - 70.0 * 50.8 * 10.0).abs() < 1e-6);
    assert!(s.doc.to_json().contains("\"exprs\""), "the design file keeps the expressions");
    let mut r = reopened(&s);
    no_trouble(&r);
    assert!((volume(&mut r) - 70.0 * 50.8 * 10.0).abs() < 1e-6);
    // Editing the sketch later keeps the points where the parameters put them.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.point", json!({"point": [200, 200]}));
    run(&mut s, "sketch.finish", json!({}));
    assert!((volume(&mut s) - 70.0 * 50.8 * 10.0).abs() < 1e-6);
}

#[test]
fn a_placed_point_is_fixed_listed_with_its_parameters_and_renamed_with_them() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "60 mm"}));
    run(&mut s, "parameters.add", json!({"name": "H", "expression": "30 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.point", json!({"point": ["W / 2", "H / 3"], "id": "c"}));
    run(&mut s, "sketch.circle.center", json!({"center": ["W / 4", 5], "radius": 2, "id": "hole"}));
    // A text without a parameter is just a number: a free point.
    run(&mut s, "sketch.point", json!({"point": ["5", "6"], "id": "free"}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    assert_eq!(solved(&s, "c"), Vec2::new(30.0, 10.0));
    let f = s.doc.find_feature("S").unwrap().id;
    let sk = s.doc.sketch(f).unwrap();
    let c = sk.points.iter().find(|q| q.id == "c").unwrap();
    assert!(c.fixed && c.exprs.as_ref().is_some_and(|e| e.x.as_deref() == Some("W / 2")), "{c:?}");
    let free = sk.points.iter().find(|q| q.id == "free").unwrap();
    assert!(!free.fixed && free.exprs.is_none() && free.pos == Vec2::new(5.0, 6.0), "{free:?}");
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "100 mm"}));
    assert_eq!(solved(&s, "c"), Vec2::new(50.0, 10.0));
    let centre = s.doc.sketch(f).unwrap().curves.iter().find(|c| c.id == "hole").map(|c| c.kind.point_ids()).unwrap_or_default();
    let centre_id = s.doc.sketch(f).unwrap().points.get(centre[0]).unwrap().id.clone();
    assert_eq!(solved(&s, &centre_id), Vec2::new(25.0, 5.0));
    // The parameter knows the point uses it; a rename reaches the expression.
    let users = run(&mut s, "parameters.users", json!({"name": "H"}));
    assert!(users.to_string().contains("Point Y"), "{users}");
    run(&mut s, "parameters.rename", json!({"name": "W", "new_name": "width"}));
    let c = s.doc.sketch(f).unwrap().points.iter().find(|q| q.id == "c").unwrap().clone();
    assert_eq!(c.exprs.and_then(|e| e.x), Some("width / 2".to_string()));
    run(&mut s, "parameters.change", json!({"name": "width", "expression": "80 mm"}));
    no_trouble(&s);
    assert_eq!(solved(&s, "c"), Vec2::new(40.0, 10.0));
}

#[test]
fn a_point_expression_that_cannot_be_worked_out_is_refused() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "turn", "expression": "30 deg"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    for bad in [json!(["nope", 0]), json!(["turn", 0]), json!([0, ""]), json!(["1 / 0", 0]), json!(["1e300 * 1e300", 0])] {
        assert!(s.execute("sketch.point", &json!({"point": bad})).is_err(), "{bad}");
        assert!(s.execute("sketch.line", &json!({"points": [[0, 0], bad]})).is_err(), "{bad}");
    }
    assert_eq!(s.doc.sketch(s.doc.find_feature("S").unwrap().id).unwrap().points.len(), 1, "only the origin");
}

#[test]
fn a_point_its_parameters_put_out_of_range_is_an_error_not_a_crash() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.point", json!({"point": ["W", 0], "id": "c"}));
    run(&mut s, "sketch.finish", json!({}));
    let changed = s.execute("parameters.change", &json!({"name": "W", "expression": "1e12 mm"}));
    let failed = s.model.results.iter().any(|r| r.error.as_deref().is_some_and(|e| e.contains("out of range")));
    assert!(changed.is_err() || failed, "{changed:?}");
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "50 mm"}));
    no_trouble(&s);
    assert_eq!(solved(&s, "c"), Vec2::new(50.0, 0.0));
    // A design file whose point expression cannot be worked out opens with that sketch in error.
    let file = s.doc.to_json();
    let hostile = file.replace("\"x\": \"W\"", "\"x\": \"W * 1e300 * 1e300\"").replace("\"x\":\"W\"", "\"x\":\"W * 1e300 * 1e300\"");
    assert_ne!(hostile, file, "the point's expression is in the file");
    let r = Session::new(solidcraft_doc::Document::from_json(&hostile).unwrap());
    assert!(r.model.results.iter().any(|r| r.error.is_some()), "the sketch reports the bad point");
}

/// A line from a point placed by parameters, tangent to an arc at the arc's start (the end they
/// share): the two tangents from the point are mirror images, and the sketch was drawn on one.
/// Moved by its parameters, the line stays on the side it was drawn on (the arc's centre keeps
/// its side of the line; the other tangent solves every constraint too), and where the move can
/// only end on the other side (a straight walk there runs through the arc) it says so.
#[test]
fn a_tangent_from_a_placed_point_keeps_the_side_it_was_drawn_on() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "ex", "expression": "30 mm"}));
    run(&mut s, "parameters.add", json!({"name": "ey", "expression": "-12 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = 7.5_f64;
    let e0 = Vec2::new(30.0, -12.0);
    // The tangent point left of origin -> E, and the arc on from it.
    let a = e0.y.atan2(e0.x) + (r / e0.len()).acos();
    let at = |t: f64| [r * t.cos(), r * t.sin()];
    run(&mut s, "sketch.point", json!({"point": ["ex", "ey"], "id": "e"}));
    run(&mut s, "sketch.point", json!({"point": [0, 0], "id": "o"}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["o"], "fixed": true}));
    run(&mut s, "sketch.point", json!({"point": at(a + 1.2), "id": "q"}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["q"], "fixed": true}));
    run(&mut s, "sketch.arc.three_point", json!({"start": at(a), "end": at(a + 1.2), "through": at(a + 0.6), "connect": false, "id": "round"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "round.center", "b": "o"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "round.end", "b": "q"}));
    run(&mut s, "sketch.line", json!({"points": ["e", "round.start"], "ids": ["flank"]}));
    run(&mut s, "sketch.constraint.tangent", json!({"a": "flank", "b": "round"}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    let f = s.doc.find_feature("S").unwrap().id;
    // +1: the tangent point left of origin -> E, as drawn.
    let side = |s: &Session| {
        let st = s.model.state();
        let sk = &st.sketch(f).unwrap().sketch;
        let Some(solidcraft_sketch::CurveKind::Line { a, b }) = sk.curves.iter().find(|c| c.id == "flank").map(|c| &c.kind) else {
            panic!("no flank")
        };
        let (e, t) = (sk.points[*a].pos, sk.points[*b].pos);
        (e.cross(t).signum(), (t.len() - r).abs() < 1e-6)
    };
    assert_eq!(side(&s), (1.0, true));
    let mut flipped = Vec::new();
    for (x, y) in [(12, -25), (-20, -18), (-28, 5), (-10, 26), (18, 22), (30, -12), (-30, -2), (25, 15), (5, -30), (-25, 20), (30, -12)] {
        run(&mut s, "parameters.change", json!({"name": "ex", "expression": format!("{x} mm")}));
        run(&mut s, "parameters.change", json!({"name": "ey", "expression": format!("{y} mm")}));
        let (sd, on) = side(&s);
        let warned = s.model.results.iter().any(|r| r.warning.is_some() || r.error.is_some());
        assert!(on || warned, "({x}, {y}): the tangent point is off the arc without a word");
        if sd != 1.0 && !warned {
            flipped.push((x, y));
        }
        // Reached in a straight walk that stays clear of the arc: kept, and nothing to say.
        if [(12, -25), (-20, -18), (5, -30), (25, 15)].contains(&(x, y)) {
            assert!(sd == 1.0 && !warned, "({x}, {y}): side {sd}, warned {warned}");
        }
    }
    assert!(flipped.is_empty(), "the tangent went over to the other side, without a word, at {flipped:?}");
}

/// Sketch `S` as the model solved it.
fn solved_sketch(s: &Session) -> solidcraft_sketch::Sketch {
    let f = s.doc.find_feature("S").unwrap().id;
    s.model.state().sketch(f).unwrap().sketch.clone()
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-6
}

/// The four corners of a rectangle as its reply names them, as the model solved them.
fn corners(s: &Session, r: &Value) -> [Vec2; 4] {
    ["bottom_left", "bottom_right", "top_right", "top_left"].map(|k| solved(s, r["corners"][k].as_str().unwrap()))
}

#[test]
fn rectangle_corners_typed_as_expressions_follow_their_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "parameters.add", json!({"name": "H", "expression": "30 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    // p1 below and right of p0: which corner each is does not matter.
    let two = run(&mut s, "sketch.rectangle.two_point", json!({"p0": ["W / 2", "H"], "p1": ["W", 20]}));
    let centre = run(&mut s, "sketch.rectangle.center", json!({"center": ["-W", 0], "corner": ["-W / 2", "H / 3"]}));
    // Plain numbers: free corners, as before.
    let plain = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, -50], "p1": [10, -40]}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    let box_of = |lo: (f64, f64), hi: (f64, f64)| [Vec2::new(lo.0, lo.1), Vec2::new(hi.0, lo.1), Vec2::new(hi.0, hi.1), Vec2::new(lo.0, hi.1)];
    let same = |got: [Vec2; 4], want: [Vec2; 4]| got.iter().zip(want).all(|(g, w)| near(*g, w));
    assert!(same(corners(&s, &two), box_of((20.0, 20.0), (40.0, 30.0))), "{:?}", corners(&s, &two));
    assert!(same(corners(&s, &centre), box_of((-60.0, -10.0), (-20.0, 10.0))), "{:?}", corners(&s, &centre));
    let f = s.doc.find_feature("S").unwrap().id;
    let fixed = |id: &Value| s.doc.sketch(f).unwrap().points.iter().find(|q| q.id == id.as_str().unwrap()).unwrap().fixed;
    assert!(fixed(&two["corners"]["top_left"]) && fixed(&two["corners"]["bottom_right"]) && !fixed(&two["corners"]["bottom_left"]));
    assert!(fixed(&centre["corners"]["top_right"]) && !fixed(&centre["corners"]["bottom_left"]));
    assert!(["bottom_left", "top_right"].iter().all(|k| !fixed(&plain["corners"][*k])), "plain numbers leave the corners free");

    // Both rectangles follow the parameters, and keep doing so in the design file.
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "60 mm"}));
    run(&mut s, "parameters.change", json!({"name": "H", "expression": "48 mm"}));
    let check = |s: &Session| {
        no_trouble(s);
        assert!(same(corners(s, &two), box_of((30.0, 20.0), (60.0, 48.0))), "{:?}", corners(s, &two));
        assert!(same(corners(s, &centre), box_of((-90.0, -16.0), (-30.0, 16.0))), "{:?}", corners(s, &centre));
        assert!(same(corners(s, &plain), box_of((0.0, -50.0), (10.0, -40.0))), "{:?}", corners(s, &plain));
    };
    check(&s);
    check(&reopened(&s));
}

#[test]
fn a_circle_and_an_ellipse_sized_by_expressions_follow_their_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let by_radius = run(&mut s, "sketch.circle.center", json!({"center": ["W", 0], "radius": "W / 4", "id": "r"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "diameter": "W / 2", "id": "d"}));
    let ellipse = run(&mut s, "sketch.ellipse", json!({"center": [0, "W"], "major": ["W / 2", "W"], "minor_radius": "W / 8"}));
    // A plain number is just a size: no dimension.
    let plain = run(&mut s, "sketch.circle.center", json!({"center": [0, -40], "radius": 3, "id": "p"}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    assert!(by_radius["param"].is_string() && ellipse["param"].is_string(), "{by_radius} {ellipse}");
    assert!(plain.get("param").is_none(), "{plain}");
    let e = ellipse["curves"][0].as_str().unwrap().to_string();
    let f = s.doc.find_feature("S").unwrap().id;
    let dims = s.doc.sketch(f).unwrap().constraints.iter().filter(|c| c.param.is_some()).count();
    assert_eq!(dims, 3, "a driving dimension for each size typed as an expression");

    let check = |s: &Session, w: f64| {
        no_trouble(s);
        let sk = solved_sketch(s);
        let curve = |id: &str| sk.curves.iter().position(|c| c.id == id).unwrap();
        let (r, d) = (curve("r"), curve("d"));
        assert!((sk.radius(r).unwrap() - w / 4.0).abs() < 1e-6 && near(sk.center(r).unwrap(), Vec2::new(w, 0.0)), "{w}");
        assert!((sk.radius(d).unwrap() - w / 4.0).abs() < 1e-6 && near(sk.center(d).unwrap(), Vec2::ZERO), "{w}");
        assert!((sk.radius(curve("p")).unwrap() - 3.0).abs() < 1e-9);
        let Some(solidcraft_sketch::CurveKind::Ellipse { c, m, r }) = sk.curves.get(curve(&e)).map(|c| c.kind.clone()) else { panic!("no ellipse") };
        assert!(near(sk.points[c].pos, Vec2::new(0.0, w)) && near(sk.points[m].pos, Vec2::new(w / 2.0, w)), "{w}");
        assert!((r - w / 8.0).abs() < 1e-6, "{w}: minor radius {r}");
    };
    check(&s, 40.0);
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "60 mm"}));
    check(&s, 60.0);
    check(&reopened(&s), 60.0);
    // One undo takes the whole circle and its dimension away.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    let curves = s.doc.sketch(f).unwrap().curves.len();
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": "W / 10"}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.sketch(f).unwrap().curves.len(), curves);
}

#[test]
fn a_size_expression_that_cannot_be_worked_out_is_refused() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    for bad in ["nope", "W - W", "-W", "1 / 0", ""] {
        assert!(s.execute("sketch.circle.center", &json!({"center": [0, 0], "radius": bad})).is_err(), "{bad}");
        assert!(s.execute("sketch.ellipse", &json!({"center": [0, 0], "major": [10, 0], "minor_radius": bad})).is_err(), "{bad}");
    }
    assert!(s.execute("sketch.rectangle.two_point", &json!({"p0": ["nope", 0], "p1": [10, 10]})).is_err());
    assert!(s.execute("sketch.rectangle.center", &json!({"center": [0, 0], "corner": ["W - W", 10]})).is_err(), "no width");
    assert_eq!(s.doc.sketch(s.doc.find_feature("S").unwrap().id).unwrap().curves.len(), 0);
}
