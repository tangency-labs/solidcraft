//! A revolve axis line picked in another sketch (#6548): curve ids restart in every sketch
//! (#3726), so a bare id resolves in the profile sketch and a colliding id picks the wrong line.
//! `{sketch, curve}` names the owning sketch; the axis is resolved there and follows it when that
//! sketch is edited. A bare id keeps resolving in the profile sketch, as saved designs replay.

use std::f64::consts::PI;

use serde_json::{Value, json};

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

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// fkirill's repro: `AxisSketch` on XY with a construction line `l1` on x = `ax` (a parameter,
/// 0 mm); `ProfileSketch` on XY with its own construction line `l1` at x = 15 and a 10 × 10
/// rectangle at x 10…20, y −5…5. Both sketches hold a curve called `l1`.
fn colliding_sketches() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "ax", "expression": "0 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "AxisSketch"}));
    run(&mut s, "sketch.line", json!({"points": [["ax", -20.0], ["ax", 20.0]], "construction": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "ProfileSketch"}));
    run(&mut s, "sketch.line", json!({"points": [[15.0, -10.0], [15.0, 10.0]], "construction": true}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10.0, -5.0], "p1": [20.0, 5.0]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

/// The ring the profile makes about x = 0: π · (20² − 10²) · 10.
const RING: f64 = PI * (400.0 - 100.0) * 10.0;

#[test]
fn revolve_axis_named_by_its_owning_sketch_uses_that_sketch() {
    let mut s = colliding_sketches();
    run(
        &mut s,
        "solid.revolve",
        json!({"sketch": "ProfileSketch", "profiles": [0], "axis": {"sketch": "AxisSketch", "curve": "l1"}, "name": "Turn"}),
    );
    assert!(rel(volume(&mut s), RING) < 1e-4, "turned about AxisSketch's l1: {}", volume(&mut s));
    let Some(solidcraft_doc::Feature { kind: solidcraft_doc::FeatureKind::Revolve { axis, .. }, .. }) = s.doc.features.last() else {
        panic!("no revolve")
    };
    assert!(matches!(axis, solidcraft_doc::AxisRef::SketchLine { curve, sketch: Some(_) } if curve == "l1"), "kept qualified: {axis:?}");
    // Saved and reopened it still turns about the other sketch's line.
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(rel(volume(&mut reopened), RING) < 1e-4, "reopened: {}", volume(&mut reopened));
}

#[test]
fn revolve_axis_follows_the_owning_sketch_when_it_is_edited() {
    let mut s = colliding_sketches();
    run(&mut s, "solid.revolve", json!({"sketch": "ProfileSketch", "profiles": [0], "axis": {"sketch": "AxisSketch", "curve": "l1"}}));
    assert!(rel(volume(&mut s), RING) < 1e-4, "{}", volume(&mut s));
    // Move AxisSketch's line to x = 5: the profile spans x 10…20, so the ring becomes
    // π · (15² − 5²) · 10 about the moved axis.
    run(&mut s, "parameters.change", json!({"name": "ax", "expression": "5 mm"}));
    let expected = PI * (15.0f64.powi(2) - 5.0f64.powi(2)) * 10.0;
    assert!(rel(volume(&mut s), expected) < 1e-4, "followed the moved axis: {} want {expected}", volume(&mut s));
}

#[test]
fn bare_axis_id_still_resolves_in_the_profile_sketch() {
    // Backward compatibility: the bare id `l1` is ProfileSketch's own line at x = 15, which the
    // rectangle crosses — the old behaviour, kept for designs saved before #6548.
    let mut s = colliding_sketches();
    let err = s
        .execute("solid.revolve", &json!({"sketch": "ProfileSketch", "profiles": [0], "axis": "l1"}))
        .expect_err("the profile sketch's own l1 crosses the profile");
    assert!(err.to_string().contains("crosses"), "{err}");
}

#[test]
fn surface_revolve_axis_named_by_its_owning_sketch() {
    let mut s = colliding_sketches();
    // The closed rectangle profile turned a full turn about x = 0: a ring reaching x ±20.
    run(
        &mut s,
        "surface.revolve",
        json!({"sketch": "ProfileSketch", "profiles": [0], "axis": {"sketch": "AxisSketch", "curve": "l1"}, "angle": "360 deg"}),
    );
    let st = s.world_state();
    let b = st.bodies.last().expect("a body").mesh().bounds();
    assert!((b.min.x - -20.0).abs() < 1e-3 && (b.max.x - 20.0).abs() < 1e-3, "turned about x = 0: {b:?}");
}
