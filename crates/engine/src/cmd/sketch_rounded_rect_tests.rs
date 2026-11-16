//! Rounded Rectangle (#891): a 2-point rectangle whose corners are filleted alike, in one step.

use serde_json::{Value, json};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::ConstraintKind;

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

fn sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

/// The evaluated (solved) sketch.
fn solved(s: &Session) -> solidcraft_sketch::Sketch {
    let id = s.active_sketch.expect("an open sketch");
    s.model.state().sketch(id).map(|ss| ss.sketch.clone()).expect("an evaluated sketch")
}

/// The corner arcs' radii, from the evaluated sketch.
fn radii(s: &Session, arcs: &[String]) -> Vec<f64> {
    let sk = solved(s);
    arcs.iter()
        .map(|id| {
            let i = sk.curves.iter().position(|c| &c.id == id).expect("arc in sketch");
            match &sk.curves[i].kind {
                solidcraft_sketch::CurveKind::Arc { c, a, .. } => sk.point(*c).unwrap().dist(sk.point(*a).unwrap()),
                k => panic!("{id} is not an arc: {k:?}"),
            }
        })
        .collect()
}

#[test]
fn a_rounded_rectangle_is_four_lines_and_four_equal_arcs_in_one_closed_profile() {
    let mut s = sketch();
    let r = run(&mut s, "sketch.rectangle.rounded", json!({"p0": [2, 3], "p1": [42, 23], "radius": 5}));
    let (curves, arcs) = (ids(&r["curves"]), ids(&r["arcs"]));
    assert_eq!(curves.len(), 8, "{r}");
    assert_eq!(arcs.len(), 4, "{r}");
    assert!(r["sides"]["bottom"].is_string() && r["sides"]["left"].is_string(), "{r}");
    assert!(r.get("corners").is_none(), "the sharp corners are gone: {r}");
    assert!(r["param"].is_string(), "one radius parameter: {r}");
    for radius in radii(&s, &arcs) {
        assert!((radius - 5.0).abs() < 1e-6, "{radius}");
    }
    let si = run(&mut s, "sketch.inspect", json!({}));
    let profiles = si["profiles"].as_array().expect("profiles");
    assert_eq!(profiles.len(), 1, "{si}");
    let area = profiles[0]["area_mm2"].as_f64().unwrap();
    let want = 40.0 * 20.0 - (4.0 - std::f64::consts::PI) * 25.0;
    assert!((area - want).abs() < 1e-6, "area {area}, want {want}");
    assert_ne!(si["status"], "conflict", "{si}");
    assert!(si["status"].as_str().is_none_or(|t| !t.contains("over")), "{si}");
    // Position and size remain free; the radius is dimensioned and the arcs are equal.
    assert_eq!(si["dof"], 4, "{si}");
}

#[test]
fn a_rounded_rectangle_is_one_undo_step() {
    let mut s = sketch();
    run(&mut s, "sketch.rectangle.rounded", json!({"p0": [0, 0], "p1": [30, 30], "radius": 4}));
    run(&mut s, "edit.undo", json!({}));
    let sk = solved(&s);
    assert!(sk.curves.is_empty(), "undo removes the whole shape: {:?}", sk.curves.iter().map(|c| &c.id).collect::<Vec<_>>());
}

#[test]
fn the_radius_parameter_drives_all_four_corners() {
    let mut s = sketch();
    let r = run(&mut s, "sketch.rectangle.rounded", json!({"p0": [-10, -5], "p1": [30, 25], "radius": "3 mm"}));
    let arcs = ids(&r["arcs"]);
    let param = r["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": param, "expression": "6 mm"}));
    for radius in radii(&s, &arcs) {
        assert!((radius - 6.0).abs() < 1e-6, "{radius}");
    }
}

#[test]
fn a_radius_too_large_or_not_positive_is_refused_and_draws_nothing() {
    for radius in [json!(10), json!(12), json!(0), json!(-2), json!("nope")] {
        let mut s = sketch();
        let err = s.execute("sketch.rectangle.rounded", &json!({"p0": [0, 0], "p1": [40, 20], "radius": radius}));
        assert!(err.is_err(), "radius {radius} accepted");
        let sk = solved(&s);
        assert!(sk.curves.is_empty(), "radius {radius} left curves behind");
    }
    let mut s = sketch();
    assert!(s.execute("sketch.rectangle.rounded", &json!({"p0": [0, 0], "p1": [40, 20]})).is_err(), "radius is required");
}

/// #7878: a rounded rectangle whose corner lands on an existing point (the far end of a line)
/// completes, as the plain rectangle does, and the vertex stays tied to the corner.
#[test]
fn a_rounded_rectangle_completes_from_an_existing_vertex() {
    let mut s = sketch();
    let line = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}))["curves"])[0].clone();
    let r = match s.execute("sketch.rectangle.rounded", &json!({"p0": [40, 0], "p1": [60, 20], "radius": 2})) {
        Ok(v) => v,
        Err(e) => panic!("the rounded rectangle refused the vertex: {e}"),
    };
    assert_eq!(ids(&r["arcs"]).len(), 4, "{r}");
    assert_eq!(ids(&r["curves"]).len(), 8, "{r}");
    let sk = solved(&s);
    // The vertex stays where it was, and the line's end stays tied to the corner.
    let at_vertex = |i: usize| sk.point(i).is_some_and(|p| (p.x - 40.0).abs() < 1e-6 && p.y.abs() < 1e-6);
    let end = sk.resolve_point(&format!("{line}.end")).expect("the line's end");
    assert!(at_vertex(end), "the line's end stays at the vertex");
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Coincident { p, q } if at_vertex(p) && at_vertex(q))),
        "the vertex stays tied to the corner"
    );
    // The shape is whole: a 20 × 20 rectangle with 2 mm corners.
    let si = run(&mut s, "sketch.inspect", json!({}));
    let area = si["profiles"][0]["area_mm2"].as_f64().unwrap();
    let want = 20.0 * 20.0 - (4.0 - std::f64::consts::PI) * 4.0;
    assert!((area - want).abs() < 1e-6, "area {area}, want {want}: {si}");
}

/// Typed width and height (the drawing tool's boxes) dimension the overall size, between
/// opposite sides, not the straight part of a side.
#[test]
fn typed_width_and_height_measure_the_whole_rounded_rectangle() {
    let mut s = sketch();
    let r = run(
        &mut s,
        "sketch.draw.dimensioned",
        json!({"command": "sketch.rectangle.rounded", "params": {"p0": [1, 1], "p1": [31, 21], "radius": 4}, "dimensions": {"width": "50 mm", "height": 25}}),
    );
    assert!(r["dimensions"]["width"].is_object() && r["dimensions"]["height"].is_object(), "{r}");
    let si = run(&mut s, "sketch.inspect", json!({}));
    let area = si["profiles"][0]["area_mm2"].as_f64().unwrap();
    let want = 50.0 * 25.0 - (4.0 - std::f64::consts::PI) * 16.0;
    assert!((area - want).abs() < 1e-6, "area {area}, want {want}: {si}");
    assert_eq!(si["dof"], 2, "only the position is free: {si}");
}

/// The corners take expressions over the design's parameters, as a 2-point rectangle's do: the
/// rounded rectangle follows them, and the radius is checked against the sides they work out to.
#[test]
fn corners_typed_as_expressions_follow_their_parameters() {
    let mut s = sketch();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "parameters.add", json!({"name": "H", "expression": "20 mm"}));
    let r = run(&mut s, "sketch.rectangle.rounded", json!({"p0": ["-W / 2", "-H / 2"], "p1": ["W / 2", "H / 2"], "radius": 3}));
    assert_eq!(ids(&r["arcs"]).len(), 4, "{r}");
    let area = |s: &mut Session| {
        let si = run(s, "sketch.inspect", json!({}));
        si["profiles"][0]["area_mm2"].as_f64().unwrap_or(f64::NAN)
    };
    let corners = (4.0 - std::f64::consts::PI) * 9.0;
    let a = area(&mut s);
    assert!((a - (40.0 * 20.0 - corners)).abs() < 1e-6, "area {a}");
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "70 mm"}));
    let a = area(&mut s);
    assert!((a - (70.0 * 20.0 - corners)).abs() < 1e-6, "area after W = 70 mm: {a}");
    // A radius the worked-out sides cannot take is refused, as for numbers.
    let too_big = s.execute("sketch.rectangle.rounded", &json!({"p0": [0, 0], "p1": ["W", "H / 4"], "radius": 3}));
    assert!(too_big.is_err(), "radius 3 mm on a 5 mm side: {too_big:?}");
}

/// #7908: a rounded rectangle has a centre point, held at the midpoint of a construction
/// diagonal between opposite corner arc centres. It stays in the middle when the size is
/// dimensioned or a parameter moves the corners, and it adds no freedom and nothing redundant.
#[test]
fn a_rounded_rectangle_keeps_a_centre_point_in_the_middle() {
    let centre_of = |s: &Session, r: &Value| {
        let sk = solved(s);
        let at = |id: &str| sk.resolve_point(id).and_then(|i| sk.point(i)).unwrap_or_else(|| panic!("no point {id}: {r}"));
        let side = |k: &str| at(&format!("{}.start", r["sides"][k].as_str().unwrap_or_else(|| panic!("no {k} side: {r}"))));
        let centre = at(r["centre"].as_str().expect("a centre"));
        let diagonal = r["diagonal"].as_str().expect("a diagonal");
        let d = sk.curves.iter().find(|c| c.id == diagonal).expect("the diagonal in the sketch");
        assert!(d.construction, "the diagonal is construction: {r}");
        (centre, solidcore_record::Vec2::new(0.5 * (side("left").x + side("right").x), 0.5 * (side("bottom").y + side("top").y)))
    };
    // Plain: drawn in the middle, the outline unchanged, still 4 DOF (position and size).
    let mut s = sketch();
    let r = run(&mut s, "sketch.rectangle.rounded", json!({"p0": [2, 3], "p1": [42, 23], "radius": 5}));
    assert_eq!(ids(&r["curves"]).len(), 8, "the centre's diagonal is not part of the outline: {r}");
    let (centre, want) = centre_of(&s, &r);
    assert!(centre.dist(solidcore_record::Vec2::new(22.0, 13.0)) < 1e-9 && centre.dist(want) < 1e-9, "centre {centre:?}, want {want:?}");
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 4, "the centre adds no freedom: {si}");
    assert_eq!(si["profiles"].as_array().map(Vec::len), Some(1), "{si}");
    assert!(si["status"].as_str().is_none_or(|t| !t.contains("over") && !t.contains("redundant")), "{si}");
    // Dimensioned: the size changes, the centre follows.
    let mut s = sketch();
    let d = run(
        &mut s,
        "sketch.draw.dimensioned",
        json!({"command": "sketch.rectangle.rounded", "params": {"p0": [1, 1], "p1": [31, 21], "radius": 4}, "dimensions": {"width": "50 mm", "height": 25}}),
    );
    let (centre, want) = centre_of(&s, &d);
    assert!(centre.dist(want) < 1e-6, "centre {centre:?}, want {want:?}: {d}");
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 2, "only the position is free: {si}");
    // Parameter-driven corners: the centre stays at the origin as they move.
    let mut s = sketch();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
    let r = run(&mut s, "sketch.rectangle.rounded", json!({"p0": ["-W / 2", -10], "p1": ["W / 2", 10], "radius": 3}));
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "70 mm"}));
    let (centre, want) = centre_of(&s, &r);
    assert!(centre.dist(solidcore_record::Vec2::ZERO) < 1e-6 && centre.dist(want) < 1e-6, "centre {centre:?}, want {want:?}");
}
