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

#[test]
fn a_rectangle_corner_typed_as_an_expression_follows_its_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let c = run(&mut s, "sketch.rectangle.center", json!({"center": [0, 0], "corner": ["W / 2", "W / 4"]}));
    // A second one, from two corners, one of them mixing a number and an expression.
    let t = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [100, 100], "p1": ["100 + W", 100 + 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 10}));
    no_trouble(&s);
    let corner = |r: &Value, k: &str| r["corners"][k].as_str().unwrap_or_default().to_string();
    assert_eq!(solved(&s, &corner(&c, "top_right")), Vec2::new(20.0, 10.0));
    assert_eq!(solved(&s, &corner(&t, "top_right")), Vec2::new(140.0, 110.0));
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "80 mm"}));
    no_trouble(&s);
    assert_eq!(solved(&s, &corner(&c, "bottom_left")), Vec2::new(-40.0, -20.0));
    assert_eq!(solved(&s, &corner(&c, "top_right")), Vec2::new(40.0, 20.0));
    // The other corners are solved, not placed: equal within the solver's tolerance.
    let br = solved(&s, &corner(&t, "bottom_right"));
    assert!((br.x - 180.0).abs() < 1e-6 && (br.y - 100.0).abs() < 1e-6, "{br:?}");
    assert!((volume(&mut s) - (80.0 * 40.0 + 80.0 * 10.0) * 10.0).abs() < 1e-6);
    // The design file keeps them.
    let mut r = reopened(&s);
    no_trouble(&r);
    assert!((volume(&mut r) - (80.0 * 40.0 + 80.0 * 10.0) * 10.0).abs() < 1e-6);
    // An expression that cannot be worked out is refused, and draws nothing.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    let points = s.doc.sketch(s.doc.find_feature("S").unwrap().id).unwrap().points.len();
    assert!(s.execute("sketch.rectangle.center", &json!({"center": [0, 0], "corner": ["nope", 5]})).is_err());
    assert!(s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [5, "nope"]})).is_err());
    assert_eq!(s.doc.sketch(s.doc.find_feature("S").unwrap().id).unwrap().points.len(), points);
}

#[test]
fn a_circle_size_typed_as_an_expression_is_a_dimension_that_follows_its_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "X", "expression": "10 mm"}));
    run(&mut s, "parameters.add", json!({"name": "R", "expression": "4 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.circle.center", json!({"center": ["X", 0], "radius": "R", "id": "a"}));
    run(&mut s, "sketch.circle.center", json!({"center": [50, 0], "diameter": "R * 4", "id": "b"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 10}));
    no_trouble(&s);
    let radius = |s: &Session, id: &str| {
        let f = s.doc.find_feature("S").unwrap().id;
        let st = s.model.state();
        let sk = &st.sketch(f).unwrap().sketch;
        let c = sk.curves.iter().find(|c| c.id == id).unwrap();
        match c.kind {
            solidcraft_sketch::CurveKind::Circle { r, .. } => r,
            _ => f64::NAN,
        }
    };
    assert!((radius(&s, "a") - 4.0).abs() < 1e-9 && (radius(&s, "b") - 8.0).abs() < 1e-9);
    let before = volume(&mut s);
    run(&mut s, "parameters.change", json!({"name": "R", "expression": "9 mm"}));
    no_trouble(&s);
    assert!((radius(&s, "a") - 9.0).abs() < 1e-9 && (radius(&s, "b") - 18.0).abs() < 1e-9);
    assert!(volume(&mut s) > before);
    // A value that is no size is refused with its own message, not a missing one.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    for bad in [json!({"center": [0, 0], "radius": "nope"}), json!({"center": [0, 0], "radius": "0 mm"}), json!({"center": [0, 0], "radius": true})] {
        let e = s.execute("sketch.circle.center", &bad).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(!e.is_empty() && !e.contains("needs `radius` or `diameter`"), "{bad}: {e}");
    }
    // `radius` wins over `diameter` whatever its form: a bad one is refused, not passed over.
    for bad in [json!({"center": [0, 0], "radius": true, "diameter": 10}), json!({"center": [0, 0], "radius": [1], "diameter": "R"})] {
        let e = s.execute("sketch.circle.center", &bad).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("`radius` must be a number"), "{bad}: {e}");
    }
    let e = s.execute("sketch.circle.center", &json!({"center": [0, 0]})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("needs `radius` or `diameter`"), "{e}");
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": "R", "diameter": 30, "id": "c"}));
    run(&mut s, "sketch.circle.center", json!({"center": [150, 0], "radius": 5, "diameter": "R * 2", "id": "d"}));
    run(&mut s, "sketch.finish", json!({}));
    assert!((circle_radius(&s, "c") - 9.0).abs() < 1e-9 && (circle_radius(&s, "d") - 5.0).abs() < 1e-9);
}

fn circle_radius(s: &Session, id: &str) -> f64 {
    let f = s.doc.find_feature("S").unwrap().id;
    let st = s.model.state();
    let sk = &st.sketch(f).unwrap().sketch;
    match sk.curves.iter().find(|c| c.id == id).map(|c| &c.kind) {
        Some(solidcraft_sketch::CurveKind::Circle { r, .. }) => *r,
        _ => f64::NAN,
    }
}

fn corner_at(s: &Session, r: &Value, k: &str) -> Vec2 {
    solved(s, r["corners"][k].as_str().unwrap_or_default())
}

#[test]
fn a_centre_rectangle_follows_its_parameters_from_any_quadrant_and_an_expression_centre() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let rects: Vec<Value> = [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)]
        .iter()
        .enumerate()
        .map(|(i, (sx, sy))| {
            let at = 200.0 * i as f64;
            let corner = json!([format!("{at} + {sx} * W / 2"), format!("{sy} * W / 4")]);
            run(
                &mut s,
                "sketch.rectangle.center", // The centre is held by an expression (a plain-number centre stays free: it moves with the solver).
                json!({"center": [format!("{at} + 0 * W"), "0 * W"], "corner": corner}),
            )
        })
        .collect();
    // An expression centre too, with a corner expressed from it.
    let c = run(&mut s, "sketch.rectangle.center", json!({"center": [1000, "W"], "corner": [1010, "W + 5"]}));
    let e = run(&mut s, "sketch.rectangle.center", json!({"center": ["W * 20", "-W"], "corner": ["W * 20 + 10", "-W - 5"]}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "80 mm"}));
    no_trouble(&s);
    for (i, r) in rects.iter().enumerate() {
        let at = 200.0 * i as f64;
        let (bl, tr) = (corner_at(&s, r, "bottom_left"), corner_at(&s, r, "top_right"));
        assert!((bl.x - (at - 40.0)).abs() < 1e-6 && (bl.y + 20.0).abs() < 1e-6, "{i}: {bl:?}");
        assert!((tr.x - (at + 40.0)).abs() < 1e-6 && (tr.y - 20.0).abs() < 1e-6, "{i}: {tr:?}");
    }
    let (bl, tr) = (corner_at(&s, &c, "bottom_left"), corner_at(&s, &c, "top_right"));
    assert!((bl.x - 990.0).abs() < 1e-6 && (bl.y - 75.0).abs() < 1e-6 && (tr.x - 1010.0).abs() < 1e-6 && (tr.y - 85.0).abs() < 1e-6, "{bl:?} {tr:?}");
    let (bl, tr) = (corner_at(&s, &e, "bottom_left"), corner_at(&s, &e, "top_right"));
    assert!(
        (bl.x - 1590.0).abs() < 1e-6 && (bl.y + 85.0).abs() < 1e-6 && (tr.x - 1610.0).abs() < 1e-6 && (tr.y + 75.0).abs() < 1e-6,
        "{bl:?} {tr:?}"
    );
}

#[test]
fn a_two_point_rectangle_drawn_from_swapped_corners_follows_its_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    // p0 is the top right, p1 the bottom left: each coordinate is kept with its corner.
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": ["W", "W / 2"], "p1": [0, 0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "60 mm"}));
    no_trouble(&s);
    assert_eq!(corner_at(&s, &r, "top_right"), Vec2::new(60.0, 30.0));
    let bl = corner_at(&s, &r, "bottom_left");
    assert!(bl.x.abs() < 1e-9 && bl.y.abs() < 1e-9, "{bl:?}");
}

#[test]
fn a_size_expression_reads_in_the_design_unit_and_one_naming_no_parameter_is_a_number() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let params = s.doc.params.len();
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": "1", "id": "a"}));
    run(&mut s, "sketch.circle.center", json!({"center": [50, 0], "radius": "2 in", "id": "b"}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "diameter": "1", "id": "c"}));
    // An expression naming no parameter is the number it comes to: no dimension behind it.
    assert_eq!(s.doc.params.len(), params);
    assert!(
        (circle_radius(&s, "a") - 25.4).abs() < 1e-9 && (circle_radius(&s, "b") - 50.8).abs() < 1e-9 && (circle_radius(&s, "c") - 12.7).abs() < 1e-9
    );
    run(&mut s, "parameters.add", json!({"name": "R", "expression": "1 in"}));
    run(&mut s, "sketch.circle.center", json!({"center": [150, 0], "radius": "R / 2", "id": "d"}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    assert!((circle_radius(&s, "d") - 12.7).abs() < 1e-9);
    run(&mut s, "parameters.change", json!({"name": "R", "expression": "2 in"}));
    no_trouble(&s);
    assert!((circle_radius(&s, "d") - 25.4).abs() < 1e-9);
    // The 2-tangent circle reads a bare number typed as text the same way.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    let x = run(&mut s, "sketch.line", json!({"points": [[200, 0], [300, 0]]}));
    let y = run(&mut s, "sketch.line", json!({"points": [[200, 0], [200, 100]], "connect": false}));
    let t = run(&mut s, "sketch.circle.two_tangent", json!({"curves": [x["curves"][0], y["curves"][0]], "radius": "1", "near": [225, 25]}));
    let id = t["curves"][0].as_str().unwrap_or_default().to_string();
    assert!((circle_radius_live(&s, &id) - 25.4).abs() < 1e-6);
}

/// The radius of circle `id` in the sketch being edited.
fn circle_radius_live(s: &Session, id: &str) -> f64 {
    let f = s.doc.find_feature("S").unwrap().id;
    match s.doc.sketch(f).ok().and_then(|sk| sk.curves.iter().find(|c| c.id == id).map(|c| c.kind.clone())) {
        Some(solidcraft_sketch::CurveKind::Circle { r, .. }) => r,
        _ => f64::NAN,
    }
}

#[test]
fn a_rejected_size_or_corner_expression_leaves_nothing_behind() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let f = s.doc.find_feature("S").unwrap().id;
    let count = |s: &Session| {
        let sk = s.doc.sketch(f).unwrap();
        (sk.points.len(), sk.curves.len(), sk.constraints.len(), s.doc.params.len())
    };
    let before = count(&s);
    let tries = [
        ("sketch.circle.center", json!({"center": ["W", 0], "radius": "W - 40"})),
        ("sketch.circle.center", json!({"center": ["W", 0], "radius": "nope"})),
        ("sketch.circle.center", json!({"center": ["W", 0], "radius": "W", "id": "bad|id"})),
        ("sketch.rectangle.center", json!({"center": ["W", 0], "corner": ["W", 5]})),
        ("sketch.rectangle.two_point", json!({"p0": ["W", 0], "p1": ["W", 5]})),
    ];
    for (cmd, p) in tries {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
        assert_eq!(count(&s), before, "{cmd} {p}");
    }
}
