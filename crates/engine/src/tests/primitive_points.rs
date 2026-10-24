//! Primitive placement points (`base`, `corner`, `center`) take expressions like their sizes do,
//! follow parameter changes, and are refused when they cannot be read (user report: a cylinder
//! with `base: ["basin_width/2", …]` silently landed at the origin).

use serde_json::{Value, json};

use super::run;
use crate::*;

fn bbox(s: &Session, body: &str) -> ([f64; 3], [f64; 3]) {
    let b = s.model.state().body(body).map(|b| b.body.clone()).unwrap();
    let m = solidcraft_kernel::measure(&b).unwrap();
    ([m.bbox.min.x, m.bbox.min.y, m.bbox.min.z], [m.bbox.max.x, m.bbox.max.y, m.bbox.max.z])
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

#[test]
fn cylinder_base_expressions_follow_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "basin_width", "expression": "100 mm"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": ["basin_width/2", "10 mm", "-2"], "body_name": "Pin"}));
    let (lo, hi) = bbox(&s, "Pin");
    assert!(near(lo, [45.0, 5.0, -2.0]) && near(hi, [55.0, 15.0, 8.0]), "{lo:?} {hi:?}");
    // A parameter change moves it on recompute.
    run(&mut s, "parameters.change", json!({"name": "basin_width", "expression": "60 mm"}));
    let (lo, _) = bbox(&s, "Pin");
    assert!(near(lo, [25.0, 5.0, -2.0]), "{lo:?}");
    // And it survives a save and reopen.
    let text = s.doc.to_json();
    assert!(text.contains("basin_width/2"), "{text}");
    let s2 = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
    let (lo, _) = bbox(&s2, "Pin");
    assert!(near(lo, [25.0, 5.0, -2.0]), "{lo:?}");
}

#[test]
fn primitive_points_take_expressions() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "40 mm"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": ["w", 0, 0], "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": "w", "width": 10, "height": 10, "center": [0, "w", 0], "body_name": "B"}));
    run(&mut s, "solid.sphere", json!({"radius": 2, "center": ["w * 2", 0, 0], "body_name": "C"}));
    run(&mut s, "solid.torus", json!({"major": 5, "minor": 1, "center": [0, 0, "w"], "body_name": "D"}));
    run(&mut s, "solid.coil", json!({"diameter": 10, "revolutions": 2, "pitch": 5, "section_size": 1, "base": ["-w", 0, 0], "body_name": "E"}));
    assert!(near(bbox(&s, "A").0, [40.0, 0.0, 0.0]));
    assert!(near(bbox(&s, "B").0, [-20.0, 35.0, -5.0]), "{:?}", bbox(&s, "B"));
    assert!(near(bbox(&s, "C").0, [78.0, -2.0, -2.0]));
    assert!(near(bbox(&s, "D").0, [-6.0, -6.0, 39.0]));
    assert!((bbox(&s, "E").0[0] + 45.5).abs() < 0.2, "{:?}", bbox(&s, "E"));
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "20 mm"}));
    assert!(near(bbox(&s, "A").0, [20.0, 0.0, 0.0]));
    assert!(near(bbox(&s, "B").0, [-10.0, 15.0, -5.0]), "{:?}", bbox(&s, "B"));
    assert!(near(bbox(&s, "C").0, [38.0, -2.0, -2.0]));
    assert!(near(bbox(&s, "D").0, [-6.0, -6.0, 19.0]));
    assert!((bbox(&s, "E").0[0] + 25.5).abs() < 0.2, "{:?}", bbox(&s, "E"));
}

/// Points and axes that cannot be read are refused, never silently put at the origin.
#[test]
fn unreadable_primitive_points_are_refused() {
    let mut s = Session::default();
    let cases: Vec<(&str, Value)> = vec![
        ("solid.cylinder", json!({"radius": 5, "height": 10, "base": ["nope/2", 0, 0]})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "base": [1, 2]})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "base": "XY"})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "base": [1, 2, "5 deg"]})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "axis": ["x", 0, 1]})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "axis": [0, 0, 0]})),
        ("solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [0, null, 0]})),
        ("solid.box", json!({"length": 1, "width": 1, "height": 1, "center": {"x": 1}})),
        ("solid.sphere", json!({"radius": 1, "center": ["a +", 0, 0]})),
        ("solid.torus", json!({"major": 5, "minor": 1, "center": [1e300, 0, 0]})),
        ("solid.coil", json!({"diameter": 10, "revolutions": 2, "pitch": 5, "section_size": 1, "base": [true, 0, 0]})),
    ];
    for (cmd, p) in cases {
        let before = s.doc.features.len();
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p} should be refused");
        assert_eq!(s.doc.features.len(), before, "{cmd} {p} added a feature");
    }
}

/// Renaming a parameter follows into placement points; deleting it is refused while a point
/// uses it. Plain-number points are stored as numbers, as before.
#[test]
fn point_parameters_rename_and_store() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "off", "expression": "30 mm"}));
    run(&mut s, "solid.sphere", json!({"radius": 2, "center": ["off", 0, 0], "body_name": "S"}));
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [1.5, -2, 0], "body_name": "B"}));
    assert!(s.execute("parameters.delete", &json!({"name": "off"})).is_err());
    run(&mut s, "parameters.rename", json!({"name": "off", "new_name": "shift"}));
    run(&mut s, "parameters.change", json!({"name": "shift", "expression": "10 mm"}));
    assert!(near(bbox(&s, "S").0, [8.0, -2.0, -2.0]), "{:?}", bbox(&s, "S"));
    let v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let feats = v["features"].as_array().unwrap();
    assert_eq!(feats[0]["center"], json!(["shift", 0.0, 0.0]), "{v}");
    assert_eq!(feats[1]["corner"], json!([1.5, -2.0, 0.0]), "{v}");
}

/// A primitive placed by an expression in a moved component lands in that component's frame
/// and keeps following its parameter.
#[test]
fn point_expressions_in_a_moved_component() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "x0", "expression": "10 mm"}));
    let c = run(&mut s, "component.create", json!({"name": "Arm", "grounded": false}));
    let (cid, occ) = (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap());
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 50, 0]}));
    run(&mut s, "component.activate", json!({"component": cid}));
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 2, "base": ["x0", 60, 0], "body_name": "Pin"}));
    let wlo = |s: &Session| s.world_state().body("Pin").unwrap().mesh().bounds().min;
    let lo = wlo(&s);
    assert!((lo.x - 9.0).abs() < 1e-3 && (lo.y - 59.0).abs() < 1e-3, "{lo:?}");
    run(&mut s, "parameters.change", json!({"name": "x0", "expression": "20 mm"}));
    let lo = wlo(&s);
    assert!((lo.x - 19.0).abs() < 1e-3 && (lo.y - 59.0).abs() < 1e-3, "{lo:?}");
}
