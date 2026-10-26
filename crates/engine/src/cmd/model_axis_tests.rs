//! An axis on an edge or round face of the model (`{edge|face: [x,y,z]}`): Revolve, Surface
//! Revolve, Circular Pattern and Plane at Angle turn about a circular edge's or a cylinder's
//! axis, and follow it when a parameter moves the cylinder.

use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::view::construction_planes;

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

/// A Ø10 × 10 shaft standing on XY at x = `cx` (a parameter, 40 mm).
fn shaft() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "cx", "expression": "40 mm"}));
    run(&mut s, "solid.cylinder", json!({"base": ["cx", 0, 0], "radius": 5, "height": 10, "body_name": "Shaft"}));
    s
}

const SHAFT: f64 = PI * 25.0 * 10.0;

/// A 2 × 6 rectangle on XZ, x 46…48, z 2…8: 6 to 8 mm from the shaft's axis.
fn ring_profile(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XZ", "name": "Profile"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [46, -2], "p1": [48, -8]}));
    run(s, "sketch.finish", json!({}));
}

/// Pappus: a 2 × 6 section whose centre is `r` from the axis, turned a full turn.
fn ring(r: f64) -> f64 {
    2.0 * PI * r * 12.0
}

#[test]
fn revolve_about_a_circular_edge_follows_the_cylinder() {
    let mut s = shaft();
    ring_profile(&mut s);
    // The shaft's top rim.
    run(&mut s, "solid.revolve", json!({"sketch": "Profile", "axis": {"edge": [45, 0, 10]}, "operation": "new", "name": "Turn"}));
    assert!(rel(volume(&mut s), SHAFT + ring(7.0)) < 1e-4, "turned about the shaft's axis: {}", volume(&mut s));
    run(&mut s, "parameters.change", json!({"name": "cx", "expression": "40.5 mm"}));
    assert!(rel(volume(&mut s), SHAFT + ring(6.5)) < 1e-4, "follows the shaft: {}", volume(&mut s));
    // Saved and opened again it still follows.
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(rel(volume(&mut reopened), volume(&mut s)) < 1e-9);
    run(&mut reopened, "parameters.change", json!({"name": "cx", "expression": "40 mm"}));
    assert!(rel(volume(&mut reopened), SHAFT + ring(7.0)) < 1e-4);
}

#[test]
fn revolve_about_a_cylinder_wall() {
    let mut s = shaft();
    ring_profile(&mut s);
    run(&mut s, "solid.revolve", json!({"sketch": "Profile", "axis": {"face": [45, 0, 5]}, "operation": "new"}));
    assert!(rel(volume(&mut s), SHAFT + ring(7.0)) < 1e-4, "{}", volume(&mut s));
    let Some(solidcraft_doc::Feature { kind: solidcraft_doc::FeatureKind::Revolve { axis, .. }, .. }) = s.doc.features.last() else {
        panic!("no revolve")
    };
    assert!(matches!(axis, solidcraft_doc::AxisRef::Model { .. }), "kept as a reference: {axis:?}");
}

#[test]
fn surface_revolve_about_a_circular_edge() {
    let mut s = shaft();
    ring_profile(&mut s);
    run(&mut s, "surface.revolve", json!({"sketch": "Profile", "axis": {"edge": [45, 0, 0]}, "angle": "180 deg"}));
    run(&mut s, "parameters.change", json!({"name": "cx", "expression": "41 mm"}));
    let st = s.world_state().unwrap();
    // A half turn about x = 41: the far side of the profile ends up at x = 41 − 7 ± 1.
    let b = st.bodies.last().map(|b| b.mesh().bounds()).unwrap();
    assert!((b.min.x - (41.0 - 7.0)).abs() < 1e-3, "{b:?}");
    assert!((b.max.x - 48.0).abs() < 1e-3, "{b:?}");
}

#[test]
fn circular_pattern_about_a_boss_wall_follows_the_boss() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "cx", "expression": "100 mm"}));
    run(&mut s, "solid.box", json!({"length": 200, "width": 200, "height": 5, "corner": [0, 0, 0], "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"base": ["cx", 100, 0], "radius": 10, "height": 20, "operation": "join", "targets": ["Plate"]}));
    run(&mut s, "solid.hole", json!({"position": [130, 100, 5], "diameter": 2, "name": "Hole"}));
    run(&mut s, "solid.pattern.circular", json!({"features": ["Hole"], "axis": {"face": [110, 100, 10]}, "count": 4, "name": "Ring"}));
    let holes = |s: &Session, cx: f64, r: f64| {
        let st = s.world_state().unwrap();
        let body = &st.bodies[0].body;
        [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)].iter().all(|(dx, dy)| !body.contains(Vec3::new(cx + dx, 100.0 + dy, 2.5)))
    };
    assert!(holes(&s, 100.0, 30.0), "four holes round the boss");
    run(&mut s, "parameters.change", json!({"name": "cx", "expression": "90 mm"}));
    assert!(holes(&s, 90.0, 40.0), "the copies turn about the boss where it is now");
    let st = s.world_state().unwrap();
    assert!(st.bodies[0].body.contains(Vec3::new(100.0, 130.0, 2.5)), "not where the boss was");
}

#[test]
fn plane_at_angle_about_a_cylinder_axis_follows_it() {
    let mut s = shaft();
    run(&mut s, "construct.plane.at_angle", json!({"base": "XZ", "axis": {"face": [45, 0, 5]}, "angle": "90 deg", "name": "Square"}));
    let plane = |s: &Session| construction_planes(s).into_iter().find(|p| p.1 == "Square").map(|p| p.2).unwrap();
    let pl = plane(&s);
    assert!(pl.normal().cross(Vec3::X).len() < 1e-9, "turned square to XZ about Z: {pl:?}");
    assert!(pl.height(Vec3::new(40.0, 0.0, 0.0)).abs() < 1e-6, "through the shaft's axis: {pl:?}");
    run(&mut s, "parameters.change", json!({"name": "cx", "expression": "25 mm"}));
    assert!(plane(&s).height(Vec3::new(25.0, 0.0, 0.0)).abs() < 1e-6, "follows the shaft");
}

#[test]
fn plane_at_angle_on_a_face_about_an_edge_follows_both() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "20 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": "h", "name": "Base"}));
    // The top face, turned 45° about its front edge (y = 0, along X).
    run(
        &mut s,
        "construct.plane.at_angle",
        json!({"base": {"face": [5, 5, 20]}, "axis": {"edge": [20, 0, 20]}, "angle": "45 deg", "name": "Slanted"}),
    );
    let feature = s.doc.find_feature("Slanted").unwrap();
    let solidcraft_doc::FeatureKind::ConstructionPlane { plane: solidcraft_doc::PlaneRef::AtAngle { base, line, .. } } = &feature.kind else {
        panic!("at_angle plane: {:?}", feature.kind)
    };
    assert!(matches!(base.as_ref(), solidcraft_doc::PlaneRef::Face { anchor: Some(_), .. }), "{base:?}");
    assert!(line.is_some(), "the edge is kept as a model reference");
    let plane = |s: &Session| construction_planes(s).into_iter().find(|p| p.1 == "Slanted").map(|p| p.2).unwrap();
    let half = 0.5_f64.sqrt();
    let pl = plane(&s);
    assert!(pl.height(Vec3::new(20.0, 0.0, 20.0)).abs() < 1e-6, "through the edge: {pl:?}");
    assert!((pl.normal().z.abs() - half).abs() < 1e-9 && pl.normal().x.abs() < 1e-9, "45° to the top: {pl:?}");
    // A taller box: the face and the edge both move up.
    run(&mut s, "parameters.change", json!({"name": "h", "expression": "30 mm"}));
    let pl = plane(&s);
    assert!(pl.height(Vec3::new(20.0, 0.0, 30.0)).abs() < 1e-6, "follows the edge: {pl:?}");
    assert!((pl.normal().z.abs() - half).abs() < 1e-9, "{pl:?}");
}

#[test]
fn the_axis_keeps_its_turn_whichever_way_the_edge_runs() {
    // The top and bottom rims run opposite ways round the shaft; a quarter turn about either
    // lands in the same place.
    let quarter = |edge: [f64; 3]| {
        let mut s = shaft();
        ring_profile(&mut s);
        run(&mut s, "solid.revolve", json!({"sketch": "Profile", "axis": {"edge": edge}, "angle": "90 deg", "operation": "new"}));
        let st = s.world_state().unwrap();
        st.bodies.last().map(|b| b.mesh().bounds()).unwrap()
    };
    let (top, bottom) = (quarter([45.0, 0.0, 10.0]), quarter([45.0, 0.0, 0.0]));
    assert!(top.min.dist(bottom.min) < 1e-6 && top.max.dist(bottom.max) < 1e-6, "{top:?} vs {bottom:?}");
}

#[test]
fn hostile_model_axes_are_refused_without_panicking() {
    let mut s = shaft();
    ring_profile(&mut s);
    let before = s.doc.to_json();
    for axis in [
        json!({"edge": [1e9, 0, 0]}),
        json!({"face": [1e300, -1e300, 0]}),
        json!({"edge": "top"}),
        json!({"face": [45, 0]}),
        // The flat top: no axis.
        json!({"face": [40, 0, 10]}),
        // The shaft's axis doesn't lie in a sketch on XY.
    ] {
        for (cmd, p) in [
            ("solid.revolve", json!({"sketch": "Profile", "axis": axis})),
            ("surface.revolve", json!({"sketch": "Profile", "axis": axis})),
            ("solid.pattern.circular", json!({"bodies": ["Shaft"], "axis": axis, "count": 3})),
            ("construct.plane.at_angle", json!({"base": "XZ", "axis": axis, "angle": "10 deg"})),
        ] {
            assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
            assert_eq!(s.doc.to_json(), before, "{cmd} {p} left the design as it was");
        }
    }
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Flat"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [46, 2], "p1": [48, 8]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.revolve", &json!({"sketch": "Flat", "axis": {"face": [45, 0, 5]}})).unwrap_err().to_string();
    assert!(e.contains("does not lie in the sketch plane"), "{e}");
}
