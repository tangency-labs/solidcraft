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
    let mesh = body.tessellate(0.05).unwrap();
    let beyond = mesh.positions.iter().map(|p| (*p - origin).dot(normal)).fold(f64::MIN, f64::max);
    assert!(beyond < 1e-3, "the join reaches {beyond} mm past the end plane");
}

/// A pen holder (#5797): a tube with a 1 mm wall lofted from a Ø`lower` ring on XY to a Ø67 ring 90 mm up, and a 1 mm disc
/// under it, as wide as the tube's foot, joined to it. The disc's top face takes the tube's ring and its bore circle. Copied in,
/// the bore's arcs run round the disc's face both ways; the hole they make was given to the region inside the bore whenever the
/// point asked lay inside it, which depended on the diameter (Ø52 joined, Ø45 did not). The join then went through every
/// fallback of the boolean, a hundred seconds that ended in "Union: no result", and the app was closed while it worked.
#[test]
fn a_disc_joined_flush_under_a_lofted_tube_follows_its_diameter() {
    // A circle and its offset 1 mm in; returns the circle's id.
    let ring = |s: &mut Session, name: &str, plane: &str, dimension: &str| -> String {
        run(s, "sketch.create", json!({"plane": plane, "name": name}));
        let circle = run(s, "sketch.circle.center", json!({"center": [0, 0], "diameter": 10}));
        let c = circle["created"]["curves"][0].as_str().unwrap_or_default().to_string();
        run(s, "sketch.dimension", json!({"entities": [c], "type": "diameter", "value": dimension}));
        run(s, "sketch.offset", json!({"curves": [c], "distance": 1, "side": [0, 0]}));
        run(s, "sketch.finish", json!({}));
        c
    };
    let mut s = Session::default();
    let lower = ring(&mut s, "Lower", "XY", "lowerDia = 52");
    run(&mut s, "solid.extrude", json!({"distance": 1, "direction": "negative", "profiles": "all", "operation": "new", "body_name": "Base"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 90, "name": "Rim"}));
    let upper = ring(&mut s, "Upper", "Rim", "upperDia = 67");
    // Each section the ring inside the circle (the offset its hole); the disc is both of the base sketch's profiles.
    let section = |sketch: &str, c: &str| json!({"sketch": sketch, "profiles": [{"curves": [c]}]});
    run(&mut s, "solid.loft", json!({"sections": [section("Lower", &lower), section("Upper", &upper)], "operation": "new", "body_name": "Tube"}));
    run(&mut s, "solid.combine", json!({"target": "Tube", "tools": ["Base"], "operation": "join"}));
    // The disc, and the tube: the frustum of its outside less the frustum of its bore.
    let expected = |lower: f64| {
        let (r0, r1, h) = (lower / 2.0, 33.5, 90.0);
        let frustum = |a: f64, b: f64| PI * h / 3.0 * (a * a + a * b + b * b);
        PI * r0 * r0 + frustum(r0, r1) - frustum(r0 - 1.0, r1 - 1.0)
    };
    for lower in [52.0, 45.0, 30.0] {
        let r = run(&mut s, "parameters.change", json!({"name": "lowerDia", "expression": format!("{lower}")}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "Ø{lower}: {r}");
        let state = s.model.state();
        assert_eq!(state.bodies.len(), 1, "Ø{lower}");
        let body = &state.bodies[0].body;
        assert!(body.validity().is_empty(), "Ø{lower}: {:?}", body.validity());
        assert_eq!(body.lumps().unwrap().len(), 1, "Ø{lower}");
        let v = volume(&mut s);
        assert!(rel(v, expected(lower)) < 1e-3, "Ø{lower}: {v} against {}", expected(lower));
    }
}
