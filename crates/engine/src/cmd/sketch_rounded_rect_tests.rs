//! Rounded Rectangle (#891): a 2-point rectangle whose corners are filleted alike, in one step.

use serde_json::{Value, json};
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
