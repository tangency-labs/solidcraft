//! `assembly.mass_properties` against closed-form values: a box, a cylinder, two boxes apart
//! (parallel axis), a turned occurrence, a rigid group, and inputs that must not panic.

use std::f64::consts::PI;

use serde_json::{Value, json};

use super::MassBody;
use crate::Session;
use solidcraft_geom::Vec3;
use solidcraft_geom::inertia::Tensor3 as Tensor;

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn near(actual: f64, expected: f64, rel: f64) {
    assert!((actual - expected).abs() <= rel * expected.abs().max(1e-9), "{actual} != {expected} (rel {rel})");
}

fn tensor(v: &Value) -> Tensor {
    let mut t = [[0.0; 3]; 3];
    for (i, row) in t.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = num(&v[i][j]);
        }
    }
    t
}

fn diag(t: &Tensor, expected: [f64; 3], rel: f64) {
    let scale = expected.iter().fold(0.0f64, |a, b| a.max(b.abs()));
    for i in 0..3 {
        near(t[i][i], expected[i], rel);
        for j in 0..3 {
            if i != j {
                assert!(t[i][j].abs() <= rel * scale, "off-diagonal [{i}][{j}] = {} in {t:?}", t[i][j]);
            }
        }
    }
}

/// A new component (not grounded) holding a box; its occurrence id and the body's name.
fn part(s: &mut Session, name: &str, size: [f64; 3], material: &str) -> (u64, u64, String) {
    run(s, "component.activate", json!({"component": "root"}));
    let c = run(s, "component.create", json!({"name": name, "grounded": false}));
    run(s, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2]}));
    let body = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap_or_default();
    run(s, "material.assign", json!({"bodies": [body], "material": material}));
    (c["component"].as_u64().unwrap_or(0), c["occurrence"].as_u64().unwrap_or(0), body)
}

fn occurrence(r: &Value, id: u64) -> &Value {
    r["occurrences"].as_array().and_then(|a| a.iter().find(|o| o["occurrence"] == id)).unwrap_or_else(|| panic!("no occurrence {id} in {r}"))
}

/// Box `a × b × c` of mass `m` about its centre: `m/12 (b² + c²)` and so on.
fn box_inertia(m: f64, [a, b, c]: [f64; 3]) -> [f64; 3] {
    [m / 12.0 * (b * b + c * c), m / 12.0 * (a * a + c * c), m / 12.0 * (a * a + b * b)]
}

#[test]
fn a_box_has_its_closed_form_mass_centre_and_inertia() {
    let mut s = Session::default();
    let (_, occ, body) = part(&mut s, "Block", [10.0, 20.0, 30.0], "Steel");
    let rho = s.doc.density(&body);
    let m = 6000.0 * rho * 1e-6;
    let r = run(&mut s, "assembly.mass_properties", json!({}));
    let o = occurrence(&r, occ);
    assert_eq!(o["bodies"][0]["material"], "Steel");
    assert_eq!(o["bodies"][0]["closed"], true);
    near(num(&o["bodies"][0]["volume_mm3"]), 6000.0, 1e-9);
    let mass = &o["mass"];
    near(num(&mass["mass_kg"]), m, 1e-9);
    for (k, c) in [5.0, 10.0, 15.0].iter().enumerate() {
        near(num(&mass["center_of_mass_mm"][k]), *c, 1e-9);
        near(num(&mass["center_of_mass_local_mm"][k]), *c, 1e-9);
    }
    diag(&tensor(&mass["inertia_world_kg_mm2"]), box_inertia(m, [10.0, 20.0, 30.0]), 1e-9);
    diag(&tensor(&mass["inertia_local_kg_mm2"]), box_inertia(m, [10.0, 20.0, 30.0]), 1e-9);
    // The design itself is a fixed group of its own, without mass.
    assert_eq!(r["groups"][0]["occurrences"], json!([0]));
    assert_eq!(r["groups"][0]["fixed"], true);
    assert!(r["groups"][0]["mass"].is_null());
    let g = &r["groups"][o["group"].as_u64().unwrap_or(0) as usize];
    assert_eq!(g["occurrences"], json!([occ]));
    assert_eq!(g["fixed"], false);
    near(num(&g["mass"]["mass_kg"]), m, 1e-9);
}

#[test]
fn a_cylinder_matches_its_closed_form_within_the_tessellation() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Shaft", "grounded": false}));
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 40}));
    let body = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap_or_default();
    let rho = s.doc.density(&body);
    let (r_, h) = (10.0f64, 40.0f64);
    let m = PI * r_ * r_ * h * rho * 1e-6;
    let r = run(&mut s, "assembly.mass_properties", json!({}));
    let o = &r["occurrences"][1];
    assert_eq!(o["bodies"][0]["material"], s.doc.body_material(&body));
    let mass = &o["mass"];
    near(num(&mass["mass_kg"]), m, 1e-3);
    near(num(&mass["center_of_mass_mm"][2]), 20.0, 1e-3);
    assert!(num(&mass["center_of_mass_mm"][0]).abs() < 1e-3);
    let side = m * (3.0 * r_ * r_ + h * h) / 12.0;
    diag(&tensor(&mass["inertia_world_kg_mm2"]), [side, side, m * r_ * r_ / 2.0], 2e-3);
}

#[test]
fn two_boxes_apart_combine_by_the_parallel_axis_theorem() {
    let mut s = Session::default();
    let (_, occ, body) = part(&mut s, "Pair", [10.0, 10.0, 10.0], "Aluminum");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0]}));
    let second = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap_or_default();
    assert_ne!(second, body);
    let (r1, r2) = (s.doc.density(&body), s.doc.density(&second));
    let (m1, m2) = (1000.0 * r1 * 1e-6, 1000.0 * r2 * 1e-6);
    let m = m1 + m2;
    // Centres at x = 5 and x = 35.
    let cx = (5.0 * m1 + 35.0 * m2) / m;
    let r = run(&mut s, "assembly.mass_properties", json!({}));
    let mass = &occurrence(&r, occ)["mass"];
    near(num(&mass["mass_kg"]), m, 1e-9);
    near(num(&mass["center_of_mass_mm"][0]), cx, 1e-9);
    let own = |mi: f64| mi / 12.0 * 200.0;
    let shift = m1 * (cx - 5.0).powi(2) + m2 * (35.0 - cx).powi(2);
    diag(&tensor(&mass["inertia_world_kg_mm2"]), [own(m1) + own(m2), own(m1) + own(m2) + shift, own(m1) + own(m2) + shift], 1e-9);
}

#[test]
fn a_turned_occurrence_turns_its_world_tensor_but_not_its_own() {
    let mut s = Session::default();
    let (_, occ, body) = part(&mut s, "Block", [10.0, 20.0, 30.0], "Steel");
    let m = 6000.0 * s.doc.density(&body) * 1e-6;
    // A quarter turn about z, kept pending (pending moves count, as Properties shows them).
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "axis": [0, 0, 1], "angle": "90 deg", "translate": [100, 0, 0], "capture": false}));
    let r = run(&mut s, "assembly.mass_properties", json!({}));
    let o = occurrence(&r, occ);
    let frame = &o["frame"];
    near(num(&frame["x"][1]), 1.0, 1e-9);
    near(num(&frame["y"][0]), -1.0, 1e-9);
    let mass = &o["mass"];
    let local = box_inertia(m, [10.0, 20.0, 30.0]);
    diag(&tensor(&mass["inertia_world_kg_mm2"]), [local[1], local[0], local[2]], 1e-9);
    diag(&tensor(&mass["inertia_local_kg_mm2"]), local, 1e-9);
    for (k, c) in [5.0, 10.0, 15.0].iter().enumerate() {
        near(num(&mass["center_of_mass_local_mm"][k]), *c, 1e-9);
    }
    // World centre: (5, 10, 15) turned about z then moved.
    let world: Vec<f64> = (0..3).map(|k| num(&mass["center_of_mass_mm"][k])).collect();
    let expected = [num(&frame["origin"][0]) - 10.0, num(&frame["origin"][1]) + 5.0, num(&frame["origin"][2]) + 15.0];
    for (a, e) in world.iter().zip(expected) {
        near(*a, e, 1e-9);
    }
}

#[test]
fn a_rigid_group_is_one_body_and_other_joints_keep_bodies_apart() {
    let mut s = Session::default();
    let (_, a, body_a) = part(&mut s, "Base", [10.0, 10.0, 10.0], "Steel");
    let (_, b, body_b) = part(&mut s, "Cap", [10.0, 10.0, 10.0], "Steel");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, 40]}));
    let (_, c, _) = part(&mut s, "Loose", [10.0, 10.0, 10.0], "Steel");
    run(&mut s, "occurrence.move", json!({"occurrence": c, "translate": [100, 0, 0]}));
    run(&mut s, "joint.rigid_group", json!({"occurrences": [a, b]}));
    let r = run(&mut s, "assembly.mass_properties", json!({}));
    let (ga, gb, gc) = (occurrence(&r, a)["group"].clone(), occurrence(&r, b)["group"].clone(), occurrence(&r, c)["group"].clone());
    assert_eq!(ga, gb);
    assert_ne!(ga, gc);
    let g = &r["groups"][ga.as_u64().unwrap_or(0) as usize];
    let mut held: Vec<u64> = vec![a, b];
    held.sort_unstable();
    assert_eq!(g["occurrences"], json!(held));
    assert_eq!(g["frame_occurrence"], json!(held[0]));
    let m1 = 1000.0 * s.doc.density(&body_a) * 1e-6;
    let m2 = 1000.0 * s.doc.density(&body_b) * 1e-6;
    let m = m1 + m2;
    let mass = &g["mass"];
    near(num(&mass["mass_kg"]), m, 1e-9);
    // Centres at z = 5 and z = 45: the common one halfway, each 20 mm off it.
    near(num(&mass["center_of_mass_mm"][2]), 25.0, 1e-9);
    let own = m1 / 12.0 * 200.0 + m2 / 12.0 * 200.0;
    let shift = m1 * 400.0 + m2 * 400.0;
    diag(&tensor(&mass["inertia_world_kg_mm2"]), [own + shift, own + shift, own], 1e-9);
}

#[test]
fn a_nested_occurrence_no_joint_places_moves_with_its_parent() {
    let mut s = Session::default();
    let outer = run(&mut s, "component.create", json!({"name": "Assembly", "grounded": false}));
    let inner = run(&mut s, "component.create", json!({"name": "Inner", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let r = run(&mut s, "assembly.mass_properties", json!({}));
    let (o, i) = (outer["occurrence"].as_u64().unwrap_or(0), inner["occurrence"].as_u64().unwrap_or(0));
    assert_eq!(occurrence(&r, o)["group"], occurrence(&r, i)["group"]);
    assert!(occurrence(&r, o)["mass"].is_null(), "the outer component has no bodies of its own");
    assert_eq!(occurrence(&r, i)["path"], json!([o, i]));
}

#[test]
fn hostile_and_degenerate_inputs_never_panic() {
    // An empty design: the design itself, without mass.
    let mut s = Session::default();
    let r = run(&mut s, "assembly.mass_properties", json!({}));
    assert_eq!(r["occurrences"].as_array().map(Vec::len), Some(1));
    assert!(r["occurrences"][0]["mass"].is_null());
    // A pending placement that is not finite is refused, not propagated.
    let (_, occ, _) = part(&mut s, "Block", [10.0, 10.0, 10.0], "Steel");
    let mut bad = solidcraft_doc::IDENTITY;
    bad[3][0] = f64::NAN;
    s.pending_moves.insert(occ, bad);
    assert!(s.execute("assembly.mass_properties", &json!({})).is_err());
    let mut huge = solidcraft_doc::IDENTITY;
    huge[0][0] = 1e200;
    huge[1][1] = 1e200;
    huge[2][2] = 1e200;
    s.pending_moves.insert(occ, huge);
    assert!(s.execute("assembly.mass_properties", &json!({})).is_err());
    // A flattening placement: no volume, so no mass, and no panic.
    let mut flat = solidcraft_doc::IDENTITY;
    flat[2][2] = 0.0;
    s.pending_moves.insert(occ, flat);
    let r = run(&mut s, "assembly.mass_properties", json!({}));
    assert!(occurrence(&r, occ)["mass"].is_null(), "{r}");
    // A rigid joint to an occurrence that is gone is ignored.
    s.pending_moves.clear();
    let mut doc = (*s.doc).clone();
    if let Some(j) = doc.assembly.joints.first_mut() {
        j.b.occurrence = 999_999;
    }
    s.doc = std::sync::Arc::new(doc);
    run(&mut s, "assembly.mass_properties", json!({}));

    // The combination itself.
    assert_eq!(MassBody::combine(&[]), None);
    let neg = MassBody { mass: -1.0, ..MassBody::default() };
    assert_eq!(MassBody::combine(&[neg]), None);
    let nan = MassBody { mass: 1.0, center: Vec3::new(f64::NAN, 0.0, 0.0), ..MassBody::default() };
    assert_eq!(MassBody::combine(&[nan]), None);
    let inf = MassBody { mass: f64::INFINITY, ..MassBody::default() };
    assert_eq!(MassBody::combine(&[inf]), None);
    // A part whose tensor is unknown leaves the total's unknown, not wrong.
    let known = MassBody { mass: 1.0, center: Vec3::ZERO, inertia: Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]) };
    let unknown = MassBody { mass: 1.0, center: Vec3::new(2.0, 0.0, 0.0), inertia: None };
    let both = MassBody::combine(&[known, unknown]).unwrap_or_else(|| panic!("positive mass"));
    assert_eq!(both.inertia, None);
    near(both.center.x, 1.0, 1e-12);
}

#[test]
fn combining_two_point_like_bodies_adds_the_parallel_axis_terms() {
    let unit: Tensor = [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]];
    let a = MassBody { mass: 1.0, center: Vec3::new(0.0, 0.0, 0.0), inertia: Some(unit) };
    let b = MassBody { mass: 3.0, center: Vec3::new(4.0, 4.0, 0.0), inertia: Some(unit) };
    let t = MassBody::combine(&[a, b]).and_then(|t| t.inertia).unwrap_or_else(|| panic!("combines"));
    // Centre (3, 3, 0); a is (−3, −3) from it, b (1, 1): Σ m d² terms and −Σ m dx dy.
    let (sxx, sxy) = (1.0 * 9.0 + 3.0 * 1.0, 1.0 * 9.0 + 3.0 * 1.0);
    near(t[0][0], 2.0 + sxx, 1e-12);
    near(t[1][1], 4.0 + sxx, 1e-12);
    near(t[2][2], 6.0 + 2.0 * sxx, 1e-12);
    near(t[0][1], -sxy, 1e-12);
    near(t[1][0], -sxy, 1e-12);
}
