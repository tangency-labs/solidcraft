//! A flange joined flush with the end of a bent, tapered lofted half tube (#1391).
//!
//! The tube is a half annulus with a 6 mm wall, lofted through three sections along a bend of radius 300 mm and 20 degrees, its radius
//! growing from 60 to 75 mm. The flange is a thicker half annulus (3 mm into the wall, 25 mm beyond it), 8 mm thick, extruded back from
//! the tube's end plane and joined. Its front face lies in that plane, coplanar with the tube's end face, and the neighbours of that
//! face are faces of the loft, which curve and lean: the faces cannot be pushed apart, and the join failed with "Union: no result".
//! (With a 40 degree bend the retry with the tool displaced joins it; with this bend that retry fails too.)

use std::f64::consts::PI;

use super::*;

/// Radius of the bend of the tube's axis, and the turn of it (radians).
const BEND: f64 = 300.0;
const TURN: f64 = 20.0 * PI / 180.0;
/// Inner radius of the tube at its start and at its end, and the wall.
const R0: f64 = 60.0;
const R1: f64 = 75.0;
const WALL: f64 = 6.0;
/// How far the flange reaches into the tube's wall, how far beyond its outer face, and how thick it is.
const LAND_IN: f64 = 3.0;
const LAND_OUT: f64 = 25.0;
const THICK: f64 = 8.0;

/// Sketch coordinates are given to nine decimals, as a design's own would be.
fn r9(x: f64) -> f64 {
    (x * 1e9).round() / 1e9
}

/// The plane of the section at the bend angle `a`, moved `over` along the axis (the sketch's x is up, its y across the tube).
fn plane(a: f64, over: f64) -> Value {
    let (tx, ty) = (a.cos(), a.sin());
    json!({
        "origin": [r9(BEND * a.sin() + over * tx), r9(BEND * (1.0 - a.cos()) + over * ty), 0],
        "x_dir": [0, 0, 1],
        "y_dir": [r9(ty), r9(-tx), 0],
    })
}

/// A half annulus (the half above z = 0) on `plane`, between the radii `r_in` and `r_out`.
fn half_annulus(s: &mut Session, name: &str, plane: Value, r_in: f64, r_out: f64) {
    let (r_in, r_out) = (r9(r_in), r9(r_out));
    run(s, "sketch.create", json!({"plane": plane, "name": name}));
    run(s, "sketch.arc.three_point", json!({"start": [0, -r_out], "end": [0, r_out], "through": [r_out, 0]}));
    run(s, "sketch.line", json!({"points": [[0, r_out], [0, r_in]]}));
    run(s, "sketch.arc.three_point", json!({"start": [0, r_in], "end": [0, -r_in], "through": [r_in, 0]}));
    run(s, "sketch.line", json!({"points": [[0, -r_in], [0, -r_out]]}));
    run(s, "sketch.finish", json!({}));
}

/// The tube: a loft through three sections.
fn tube(s: &mut Session) {
    for i in 0..3 {
        let t = i as f64 / 2.0;
        let r_in = R0 + (R1 - R0) * t;
        half_annulus(s, &format!("S{i}"), plane(TURN * t, 0.0), r_in, r_in + WALL);
    }
    let sections: Vec<Value> = (0..3).map(|i| json!({"sketch": format!("S{i}")})).collect();
    run(s, "solid.loft", json!({"sections": sections, "operation": "new", "name": "TubeLoft", "body_name": "Tube"}));
}

/// The flange, joined to the tube. Its front face is `over` mm past the tube's end plane (0: in it).
fn flange(s: &mut Session, over: f64) {
    half_annulus(s, "Flange", plane(TURN, over), R1 + LAND_IN, R1 + WALL + LAND_OUT);
    let join = json!({"sketch": "Flange", "distance": THICK + over, "direction": "negative", "operation": "join", "targets": ["Tube"], "name": "JoinFlange"});
    run(s, "solid.extrude", join);
}

#[test]
fn a_flange_flush_with_the_end_of_a_bent_tapered_tube_joins() {
    // The flange's face, the half annulus, has this area.
    let (r_in, r_out) = (R1 + LAND_IN, R1 + WALL + LAND_OUT);
    let footprint = PI / 2.0 * (r_out * r_out - r_in * r_in);

    // The reference: with the flange 2 mm past the end plane nothing is coplanar with the tube's end face (that join is made by another
    // route), and what it adds beyond that plane is the footprint, 2 mm deep.
    let mut s = Session::default();
    tube(&mut s);
    flange(&mut s, 2.0);
    let reference = volume(&mut s) - 2.0 * footprint;

    let mut s = Session::default();
    tube(&mut s);
    flange(&mut s, 0.0);
    let v = volume(&mut s);
    assert!(rel(v, reference) < 3e-4, "{v} against {reference}");

    // One valid body in one piece, and nothing of it beyond the end plane (to a micrometre: the curved faces end on it within the
    // tolerance of their edges): the face the flange and the tube share is one flat face.
    let state = s.model.state();
    assert_eq!(state.bodies.len(), 1);
    let body = &state.bodies[0].body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    assert_eq!(body.lumps().unwrap().len(), 1);
    let (origin, normal) = (Vec3::new(BEND * TURN.sin(), BEND * (1.0 - TURN.cos()), 0.0), Vec3::new(TURN.cos(), TURN.sin(), 0.0));
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.05)).unwrap();
    let beyond = mesh.positions.iter().map(|p| (*p - origin).dot(normal)).fold(f64::MIN, f64::max);
    assert!(beyond < 1e-3, "the join reaches {beyond} mm past the end plane");
}
