//! `pulley.insert`: sizes against the belt standards' pitch values, the solid against its area
//! worked out by hand, and hostile parameters.

use std::f64::consts::PI;

use serde_json::{Value, json};

use super::pulleys::{BELT_CLEARANCE, Pulley, outline, profile, pulley_from};
use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Every body's volume, in mm³.
fn volumes(s: &mut Session) -> Vec<f64> {
    let m = run(s, "inspect.measure", json!({}));
    m["bodies"].as_array().map(|b| b.iter().filter_map(|b| b["volume_mm3"].as_f64()).collect()).unwrap_or_default()
}

/// The area two circles share: radii r1 and r2, centres d apart.
fn lens(r1: f64, r2: f64, d: f64) -> f64 {
    let a1 = ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1)).acos();
    let a2 = ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2)).acos();
    let k = ((-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2)).sqrt();
    r1 * r1 * a1 + r2 * r2 * a2 - 0.5 * k
}

/// The grooved outline's area: the outside circle less each groove's circle inside it.
fn outline_area(p: &Pulley) -> f64 {
    let ro = p.outside_radius();
    let rg = p.profile.groove_radius;
    let rc = ro - p.profile.depth + rg;
    PI * ro * ro - f64::from(p.teeth) * lens(ro, rg, rc)
}

fn pulley(p: Value) -> Pulley {
    match pulley_from(&p, "test") {
        Ok(v) => v,
        Err(e) => panic!("{p}: {e}"),
    }
}

#[test]
fn diameters_follow_the_pitch_line() {
    // GT2 20 grooves: pitch circle 40/π = 12.732 mm, outside 2 × 0.254 mm inside it = 12.224 mm.
    let p = pulley(json!({"profile": "gt2", "teeth": 20}));
    assert!((p.pitch_diameter() - 40.0 / PI).abs() < 1e-12);
    assert!((2.0 * p.outside_radius() - (40.0 / PI - 0.508)).abs() < 1e-12);
    assert!((2.0 * p.outside_radius() - 12.224).abs() < 1e-3, "{}", 2.0 * p.outside_radius());
    // HTD 5M 30 grooves: 150/π = 47.75 mm pitch circle, outside 46.60 mm.
    let p = pulley(json!({"profile": "htd5m", "teeth": 30}));
    assert!((2.0 * p.outside_radius() - (150.0 / PI - 1.143)).abs() < 1e-12);
    // The profile's own belt width by default, the toothed part 1 mm wider, a flange each side.
    assert_eq!(p.belt_width, 15.0);
    assert!((p.width() - (15.0 + BELT_CLEARANCE + 2.0 * 1.5)).abs() < 1e-12);
}

#[test]
fn the_outline_is_a_closed_loop_of_grooves_and_lands() {
    for (id, teeth) in [("gt2", 10), ("gt2", 20), ("htd3m", 15), ("htd5m", 60), ("gt2", 200)] {
        let p = pulley(json!({"profile": id, "teeth": teeth}));
        let arcs = outline(&p).unwrap();
        assert_eq!(arcs.len(), 2 * teeth as usize, "{id} {teeth}");
        for (k, a) in arcs.iter().enumerate() {
            let next = &arcs[(k + 1) % arcs.len()];
            // Each arc's end is the next one's start (grooves are listed end to start).
            let (end, start) = if k % 2 == 0 { (a.a, next.a) } else { (a.b, next.b) };
            assert!((end[0] - start[0]).hypot(end[1] - start[1]) < 1e-9, "{id} {teeth}: arc {k}");
        }
    }
}

#[test]
fn a_flanged_pulley_with_a_bore_has_the_volume_worked_out_by_hand() {
    let mut s = Session::default();
    let r = run(&mut s, "pulley.insert", json!({"profile": "gt2", "teeth": 20, "bore_diameter": 5}));
    assert_eq!(r["name"], "GT2 Pulley 20T 6mm");
    assert!((r["outside_diameter"].as_f64().unwrap() - 12.224).abs() < 1e-3, "{r}");
    assert!((r["width"].as_f64().unwrap() - 9.0).abs() < 1e-9, "{r}");
    let p = pulley(json!({"profile": "gt2", "teeth": 20, "bore_diameter": 5}));
    let flange = PI * (p.flange_diameter() / 2.0).powi(2) * 1.0;
    let hole = PI * 2.5 * 2.5 * p.width();
    let expected = outline_area(&p) * 7.0 + 2.0 * flange - hole;
    let v = volumes(&mut s);
    assert_eq!(v.len(), 1, "one body: {v:?}");
    assert!((v[0] - expected).abs() / expected < 1e-3, "volume {} expected {expected}", v[0]);
    // Its own component, free to be placed, and the design's active component again after.
    let comp = r["component"].as_u64().unwrap();
    assert!(s.doc.components.iter().any(|c| c.id == comp && c.name == "GT2 Pulley 20T 6mm"));
    assert_eq!(s.active_component, 0);
    // One undo removes the whole pulley.
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.components.iter().all(|c| c.id != comp));
}

#[test]
fn without_flanges_or_bore_it_is_the_grooved_outline_extruded() {
    let mut s = Session::default();
    let r = run(&mut s, "pulley.insert", json!({"profile": "htd3m", "teeth": 24, "belt_width": 6, "flanges": false, "name": "Idler"}));
    assert_eq!(r["name"], "Idler");
    let p = pulley(json!({"profile": "htd3m", "teeth": 24, "belt_width": 6, "flanges": false}));
    let expected = outline_area(&p) * 7.0;
    let v = volumes(&mut s);
    assert!((v[0] - expected).abs() / expected < 1e-3, "volume {} expected {expected}", v[0]);
}

#[test]
fn an_offset_bore_is_cut_off_the_axis() {
    let mut s = Session::default();
    run(&mut s, "pulley.insert", json!({"profile": "htd5m", "teeth": 30, "bore_diameter": 8, "bore_offset": 6}));
    let p = pulley(json!({"profile": "htd5m", "teeth": 30, "bore_diameter": 8, "bore_offset": 6}));
    assert_eq!(p.bore, Some((8.0, 6.0)));
    let flange = PI * (p.flange_diameter() / 2.0).powi(2) * 1.5;
    let expected = outline_area(&p) * 16.0 + 2.0 * flange - PI * 16.0 * p.width();
    let v = volumes(&mut s);
    assert!((v[0] - expected).abs() / expected < 1e-3, "volume {} expected {expected}", v[0]);
}

#[test]
fn hostile_params_never_panic() {
    let mut s = Session::default();
    let bad = [
        json!({}),
        json!({"profile": "gt2"}),
        json!({"profile": "t5", "teeth": 20}),
        json!({"profile": 7, "teeth": 20}),
        json!({"profile": "gt2", "teeth": 9}),
        json!({"profile": "gt2", "teeth": 201}),
        json!({"profile": "gt2", "teeth": -20}),
        json!({"profile": "gt2", "teeth": 20.5}),
        json!({"profile": "gt2", "teeth": u64::MAX}),
        json!({"profile": "gt2", "teeth": 20, "belt_width": 0}),
        json!({"profile": "gt2", "teeth": 20, "belt_width": 1e308}),
        json!({"profile": "gt2", "teeth": 20, "belt_width": "wide"}),
        json!({"profile": "gt2", "teeth": 20, "flanges": "yes"}),
        json!({"profile": "gt2", "teeth": 20, "bore_diameter": 0}),
        json!({"profile": "gt2", "teeth": 20, "bore_diameter": -5}),
        json!({"profile": "gt2", "teeth": 20, "bore_diameter": 12}),
        json!({"profile": "gt2", "teeth": 20, "bore_diameter": 3, "bore_offset": 4}),
        json!({"profile": "gt2", "teeth": 20, "bore_diameter": 3, "bore_offset": "x"}),
        json!({"profile": "gt2", "teeth": 20, "bore_diameter": 3, "bore_offset": -1e300}),
    ];
    for p in bad {
        let before = s.doc.components.len();
        match s.execute("pulley.insert", &p) {
            Err(crate::EngineError::BadParams { .. }) => {}
            other => panic!("{p} should be refused as bad parameters: {other:?}"),
        }
        assert_eq!(s.doc.components.len(), before, "{p} left a component behind");
    }
}

#[test]
fn every_profile_is_known_by_its_id() {
    for id in ["gt2", "htd3m", "htd5m", " GT2 "] {
        assert!(profile(id).is_some(), "{id}");
    }
    assert!(profile("t2.5").is_none());
}
