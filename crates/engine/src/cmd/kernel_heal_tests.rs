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

/// #6333: after a mid-split box, two unnamed outside-shells on the same cut pick must hollow
/// both halves. AABB body picks kept hitting the first (already opened) half and reported
/// `no face at` for the bottom.
#[test]
fn shell_outside_the_lower_half_after_a_split() {
    let pick = [-6.775204233387228, 36.56830266326779, 0.0];
    let stock = json!({"corner": [-50.0, 0.0, -25.0], "length": 100, "width": 80, "height": 50, "body_name": "Stock"});
    let half_vol = 100.0 * 80.0 * 25.0;
    let mut s = Session::default();
    run(&mut s, "solid.box", stock);
    run(&mut s, "solid.split_body", json!({"body": "Stock", "plane": "XY"}));
    for pass in 1..=2 {
        match s.execute(
            "solid.shell",
            &json!({"direction": "outside", "faces": [pick], "thickness": 3.5, "tangent_chain": true}),
        ) {
            Ok(_) => {}
            Err(e) => panic!("unnamed outside shell pass {pass} at cut pick: {e}"),
        }
    }
    let after = run(&mut s, "inspect.measure", json!({}));
    let vols: Vec<(String, f64)> = after["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| Some((b["name"].as_str()?.to_string(), b["volume_mm3"].as_f64()?)))
        .collect();
    assert_eq!(vols.len(), 2, "{vols:?}");
    assert!(vols.iter().all(|(_, v)| *v > 0.0 && *v < half_vol * 0.95), "both halves hollow: {vols:?}");
}

/// #6564: two boxes in the same place — a face pick for the second must hollow B, not the
/// already-shelled twin (equal-distance body picks used to prefer the older body).
#[test]
fn shell_second_box_at_the_same_place() {
    let before_b = 40.0 * 30.0 * 20.0;
    // First shelled by its top (gone): the pick must still find B.
    {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "A"}));
        run(&mut s, "solid.shell", json!({"body": "A", "faces": [[20, 15, 20]], "thickness": 2}));
        let v_a = run(&mut s, "inspect.measure", json!({}))["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "A").unwrap()["volume_mm3"]
            .as_f64()
            .unwrap();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
        run(&mut s, "solid.shell", json!({"faces": [[20, 15, 20]], "thickness": 2, "tangent_chain": true}));
        let after = run(&mut s, "inspect.measure", json!({}));
        let a = after["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "A").unwrap()["volume_mm3"].as_f64().unwrap();
        let b = after["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "B").unwrap()["volume_mm3"].as_f64().unwrap();
        assert!((a - v_a).abs() < 1e-3, "A should stay as shelled: {a} vs {v_a}");
        assert!(b < before_b * 0.75 && b > 0.0, "B should be hollow: {b}");
    }
    // First shelled by a side (top remains on both): the shared top must pick B, not re-shell A.
    {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "A"}));
        run(&mut s, "solid.shell", json!({"body": "A", "faces": [[40, 15, 10]], "thickness": 2}));
        let v_a = run(&mut s, "inspect.measure", json!({}))["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "A").unwrap()["volume_mm3"]
            .as_f64()
            .unwrap();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
        run(&mut s, "solid.shell", json!({"faces": [[20, 15, 20]], "thickness": 2}));
        let after = run(&mut s, "inspect.measure", json!({}));
        let a = after["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "A").unwrap()["volume_mm3"].as_f64().unwrap();
        let b = after["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "B").unwrap()["volume_mm3"].as_f64().unwrap();
        assert!((a - v_a).abs() < 1e-3, "A should stay as shelled: {a} vs {v_a}");
        assert!(b < before_b * 0.75 && b > 0.0, "B should be hollow: {b}");
    }
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
