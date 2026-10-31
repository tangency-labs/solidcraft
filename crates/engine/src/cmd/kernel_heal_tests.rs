//! Delete Face and shells/offsets on cones and spheres, through the engine.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// A box with a drilled hole (cone tip) shells: the cavity is the box shrunk by the wall and the
/// hole grown by it (its cone slid down its axis), measured against that analytic volume.
#[test]
fn shell_a_box_with_a_drilled_hole() {
    use std::f64::consts::PI;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "solid.hole", json!({"position": [20, 15, 20], "diameter": 8, "depth": 10, "type": "drilled", "tip_angle": 118}));
    let v_body = volume(&mut s);
    let (r, t) = (4.0, 2.0);
    let a = (59.0f64).to_radians();
    // Cylinder depth from the measured body: 24000 − π r² h − π r² (r / tan α) / 3.
    let h = (24000.0 - v_body - PI * r * r * (r / a.tan()) / 3.0) / (PI * r * r);
    assert!(h > 9.0 && h < 11.0, "{h}");
    run(&mut s, "solid.shell", json!({"faces": [[5, 5, 20]], "thickness": t}));
    let v_shell = volume(&mut s);
    let apex = 20.0 - h - r / a.tan() - t / a.sin();
    let big_r = r + t;
    let joint = apex + big_r / a.tan();
    let grown = PI * big_r * big_r * (20.0 - joint) + PI * big_r * big_r * (joint - apex) / 3.0;
    let cavity = 36.0 * 26.0 * 18.0 - grown;
    let want = v_body - cavity;
    assert!((v_shell - want).abs() < 2e-4 * want, "{v_shell} vs {want}");
}

/// #4009: shell with no open face leaves a closed hollow (analytic box volume).
#[test]
fn shell_closed_hollow_with_no_open_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 12.5}));
    run(&mut s, "solid.shell", json!({"faces": [], "thickness": 2.5}));
    let vol = volume(&mut s);
    let want = 40.0 * 30.0 * 12.5 - 35.0 * 25.0 * 7.5;
    assert!((vol - want).abs() < 1e-6 * want, "{vol} vs {want}");
    let m = run(&mut s, "inspect.measure", json!({}));
    let faces = m["bodies"].as_array().unwrap()[0]["faces"].as_u64().unwrap();
    assert_eq!(faces, 12, "outer and inner shells: {m}");
}

/// #4009: with several bodies, a closed hollow needs an explicit body name.
#[test]
fn shell_closed_hollow_needs_a_body_when_several_exist() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 12.5, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"corner": [50, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "B"}));
    match s.execute("solid.shell", &json!({"faces": [], "thickness": 2.5})) {
        Ok(_) => panic!("expected a body name when two bodies exist"),
        Err(e) => assert!(e.to_string().contains("body") || e.to_string().contains("select"), "{e}"),
    }
    run(&mut s, "solid.shell", json!({"body": "A", "faces": [], "thickness": 2.5}));
    let a = run(&mut s, "inspect.measure", json!({}))["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "A")
        .and_then(|b| b["volume_mm3"].as_f64())
        .unwrap();
    let want = 40.0 * 30.0 * 12.5 - 35.0 * 25.0 * 7.5;
    assert!((a - want).abs() < 1e-6 * want, "{a} vs {want}");
}

/// Offset Face on a cone and a sphere move them exactly.
#[test]
fn offset_cone_and_sphere_faces() {
    use std::f64::consts::PI;
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 0], "radius": 10}));
    let r = run(&mut s, "solid.offset_face", json!({"faces": [[10, 0, 0]], "distance": 2}));
    let v = volume(&mut s);
    assert!((v - 4.0 / 3.0 * PI * 1728.0).abs() < 1e-3 * v, "{v} {r}");
}

/// Offset Face on a torus changes its tube radius and keeps the major radius.
#[test]
fn offset_torus_face() {
    use std::f64::consts::PI;
    let mut s = Session::default();
    run(&mut s, "solid.torus", json!({"major": 20, "minor": 5}));
    let v0 = volume(&mut s);
    assert!((v0 - 2.0 * PI * PI * 20.0 * 25.0).abs() < 1e-3 * v0, "{v0}");
    run(&mut s, "solid.offset_face", json!({"faces": [[25, 0, 0]], "distance": 1}));
    let v = volume(&mut s);
    assert!((v - 2.0 * PI * PI * 20.0 * 36.0).abs() < 1e-3 * v, "{v}");
}
