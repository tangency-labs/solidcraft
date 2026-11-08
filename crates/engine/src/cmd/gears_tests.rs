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
