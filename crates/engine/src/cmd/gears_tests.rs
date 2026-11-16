//! `gear.insert`: the gear's size, volume and build time.

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
    let m = run(s, "inspect.measure", json!({}));
    m["bodies"].as_array().map(|b| b.iter().filter_map(|x| x["volume_mm3"].as_f64()).sum()).unwrap_or(0.0)
}

#[test]
fn a_gear_is_solid_between_its_root_and_tip_circles() {
    let mut s = Session::default();
    let r = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10}));
    assert!((r["pitch_diameter"].as_f64().unwrap_or(0.0) - 40.0).abs() < 1e-9);
    let v = volume(&mut s);
    let (root, tip) = (17.5f64, 22.0f64);
    assert!(v > PI * root * root * 10.0 && v < PI * tip * tip * 10.0, "{v}");
    // The pitch circle cuts a tooth and a gap equally: the area is about that of the pitch circle.
    assert!((v / 10.0 - PI * 20.0 * 20.0).abs() < 0.06 * PI * 400.0, "{v}");
}

#[test]
fn hostile_gear_parameters_are_refused_and_leave_the_design_alone() {
    let mut s = Session::default();
    for p in [
        json!({"module": 0, "teeth": 20}),
        json!({"module": -2, "teeth": 20}),
        json!({"module": 2, "teeth": 3}),
        json!({"module": 2, "teeth": 100000}),
        json!({"module": 2, "teeth": 20, "pressure_angle": 90}),
        json!({"module": 2, "teeth": 20, "face_width": 0}),
        json!({"module": 2, "teeth": 20, "flank_points": 100000}),
        json!({"module": 2, "teeth": 20, "profile_shift": 1e9}),
        json!({"module": 1e300, "teeth": 20}),
    ] {
        assert!(s.execute("gear.insert", &p).is_err(), "{p}");
    }
    assert_eq!(volume(&mut s), 0.0);
}

#[test]
fn a_gear_with_many_teeth_builds_with_the_right_size() {
    // Four edges a tooth keep the extrude cheap (#6796): a 120-tooth gear is still solid at its pitch circle.
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 1, "teeth": 120, "face_width": 5}));
    let v = volume(&mut s);
    assert!((v / 5.0 - PI * 60.0 * 60.0).abs() < 0.03 * PI * 3600.0, "{v}");
}

fn tip_diameter(r: &Value) -> f64 {
    r["tip_diameter"].as_f64().unwrap_or(0.0)
}

#[test]
fn editing_a_gear_rebuilds_its_teeth_in_place() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10}));
    let comp = g["component"].clone();
    let before = volume(&mut s);
    let e = run(&mut s, "gear.edit", json!({"component": comp, "teeth": 30}));
    // The same component and occurrence, a bigger gear: 30 teeth of module 2 is Ø60 pitch.
    assert_eq!((&e["component"], &e["occurrence"]), (&g["component"], &g["occurrence"]));
    assert!((e["pitch_diameter"].as_f64().unwrap_or(0.0) - 60.0).abs() < 1e-9);
    assert!(tip_diameter(&e) > tip_diameter(&g));
    let after = volume(&mut s);
    assert!((after / 10.0 - PI * 30.0 * 30.0).abs() < 0.06 * PI * 900.0, "{before} {after}");
    // The default name follows the values.
    assert_eq!(e["name"], "Spur Gear M2 Z30");
    // One undo takes the edit back.
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - before).abs() < 1e-6 * before);
    assert_eq!(s.doc.gears[&comp.as_u64().unwrap_or(0)].teeth, 20);
}

#[test]
fn a_gear_keeps_its_values_through_save_and_reopen() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 1.5, "teeth": 24, "pressure_angle": 25, "face_width": 8, "backlash": 0.05}));
    let comp = g["component"].as_u64().unwrap_or(0);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json().to_string()).unwrap());
    assert_eq!(reopened.doc.gears.get(&comp), s.doc.gears.get(&comp));
    // The reopened design edits the same gear.
    let e = run(&mut reopened, "gear.edit", json!({"component": comp, "face_width": 12}));
    assert_eq!(e["component"].as_u64(), Some(comp));
    assert_eq!(reopened.doc.gears[&comp].teeth, 24);
    assert_eq!(reopened.doc.gears[&comp].backlash, 0.05);
}

#[test]
fn a_bad_gear_edit_is_refused_and_leaves_the_gear_alone() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20}));
    let comp = g["component"].clone();
    let before = volume(&mut s);
    for p in [json!({"teeth": 2}), json!({"module": -1}), json!({"pressure_angle": 90}), json!({"face_width": 0}), json!({"profile_shift": 1e9})] {
        let mut p = p;
        p["component"] = comp.clone();
        assert!(s.execute("gear.edit", &p).is_err(), "{p}");
    }
    assert!(s.execute("gear.edit", &json!({"component": 999, "teeth": 30})).is_err());
    assert!(s.execute("gear.edit", &json!({"component": "nothing"})).is_err());
    assert_eq!(volume(&mut s), before);
    assert_eq!(s.doc.gears[&comp.as_u64().unwrap_or(0)].teeth, 20);
}

#[test]
fn deleting_a_gear_forgets_its_values() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20}));
    run(&mut s, "component.delete", json!({"component": g["component"]}));
    assert!(s.doc.gears.is_empty());
}

#[test]
fn an_internal_gear_is_a_ring_with_its_teeth_inside() {
    let mut s = Session::default();
    let r = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 40, "face_width": 8, "internal": true}));
    assert_eq!(r["name"], "Internal Gear M2 Z40");
    // Tips of the ring's teeth are inside the pitch circle, the roots outside it.
    assert!((r["tip_diameter"].as_f64().unwrap_or(0.0) - 76.0).abs() < 1e-9);
    assert!((r["root_diameter"].as_f64().unwrap_or(0.0) - 85.0).abs() < 1e-9);
    // The default ring is two modules thick outside the roots: outer radius 42.5 + 4. The pitch circle cuts a tooth and a gap equally.
    let v = volume(&mut s);
    assert!((v / 8.0 - PI * (46.5 * 46.5 - 40.0 * 40.0)).abs() < 0.06 * PI * 46.5 * 46.5, "{v}");
}

#[test]
fn an_external_gear_takes_an_axle_hole_and_edit_can_turn_it_into_a_ring() {
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10, "bore_diameter": 10}));
    let v = volume(&mut s);
    assert!((v / 10.0 - PI * (20.0 * 20.0 - 5.0 * 5.0)).abs() < 0.06 * PI * 400.0, "{v}");
    run(&mut s, "gear.edit", json!({"component": "Spur Gear M2 Z20", "bore_diameter": 0}));
    assert!((volume(&mut s) / 10.0 - PI * 400.0).abs() < 0.06 * PI * 400.0);
    // Inside out: the saved axle hole is dropped and the name follows.
    let r = run(&mut s, "gear.edit", json!({"component": "Spur Gear M2 Z20", "internal": true, "teeth": 40}));
    assert_eq!(r["name"], "Internal Gear M2 Z40");
    assert!(volume(&mut s) < 8.0 * 10.0 * PI * 46.5 * 46.5);
}

#[test]
fn an_internal_or_bored_gear_keeps_its_values_through_save_and_reopen() {
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 30, "internal": true, "rim_thickness": 5}));
    run(&mut s, "gear.insert", json!({"module": 1, "teeth": 24, "bore_diameter": 6}));
    let saved = s.doc.gears.clone();
    let text = serde_json::to_string(&saved).unwrap_or_default();
    let back: std::collections::BTreeMap<u64, solidcraft_doc::SpurGear> = serde_json::from_str(&text).unwrap_or_default();
    assert_eq!(back, saved);
    assert!(saved.values().any(|g| g.internal && g.rim_thickness == 5.0));
    assert!(saved.values().any(|g| !g.internal && g.bore_diameter == 6.0));
}

#[test]
fn hostile_internal_and_bore_values_are_refused_and_leave_the_design_alone() {
    let mut s = Session::default();
    for p in [
        json!({"module": 2, "teeth": 40, "internal": true, "profile_shift": 0.5}),
        json!({"module": 2, "teeth": 40, "internal": true, "bore_diameter": 5}),
        json!({"module": 2, "teeth": 40, "internal": true, "rim_thickness": -1}),
        json!({"module": 2, "teeth": 40, "rim_thickness": 3}),
        json!({"module": 2, "teeth": 40, "bore_diameter": -1}),
        json!({"module": 2, "teeth": 40, "bore_diameter": 1e9}),
        json!({"module": 2, "teeth": 40, "bore_diameter": 80}),
        json!({"module": 2, "teeth": 40, "internal": "yes"}),
        json!({"module": 2, "teeth": 40, "backlash": -1}),
    ] {
        assert!(s.execute("gear.insert", &p).is_err(), "{p}");
    }
    assert_eq!(volume(&mut s), 0.0);
}
