//! `motion.simulate` against closed forms: a bar swinging on a hinge (compound pendulum
//! period), a free block falling, two bars geared together, and inputs it must refuse.

use std::f64::consts::TAU;

use serde_json::{Value, json};

use crate::Session;

const G: f64 = 9806.65;

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

/// A component with one box, grounded or not.
fn part(s: &mut Session, name: &str, grounded: bool, size: [f64; 3], corner: [f64; 3]) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": name, "grounded": grounded}));
    run(s, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner}));
}

/// A bar 10 × 10 × 100 mm hanging from a hinge about Y at `(x, 0, 0)`, its top face's centre.
fn bar(s: &mut Session, name: &str, x: f64) {
    part(s, name, false, [10.0, 10.0, 100.0], [x - 5.0, -5.0, -100.0]);
    run(
        s,
        "joint.create",
        json!({"type": "revolute", "name": format!("{name} hinge"), "flip": true,
               "a": {"occurrence": "Frame", "point": [x, 0, 0], "z": [0, 1, 0]},
               "b": {"occurrence": name, "point": [x, 0, 0], "z": [0, 1, 0]}}),
    );
}

fn frame(s: &mut Session) {
    part(s, "Frame", true, [200.0, 20.0, 20.0], [-50.0, 10.0, 0.0]);
}

/// The compound pendulum's period at small amplitude `a` (rad): 2π√(I_pivot / (m g d)), with
/// I_pivot = m(10² + 100²)/12 + m·50², d = 50, lengthened by a²/16.
fn bar_period(a: f64) -> f64 {
    let i = (100.0 + 10_000.0) / 12.0 + 2500.0;
    TAU * (i / (G * 50.0)).sqrt() * (1.0 + a * a / 16.0)
}

fn joint_value(sample: &Value, k: usize) -> f64 {
    num(&sample["joints"][k]["values"][0])
}

fn mean_period(r: &Value, k: usize) -> f64 {
    let samples = r["samples"].as_array().unwrap();
    let mut crossings = Vec::new();
    for w in samples.windows(2) {
        let (a, b) = (joint_value(&w[0], k), joint_value(&w[1], k));
        if a < 0.0 && b >= 0.0 {
            let (ta, tb) = (num(&w[0]["time"]), num(&w[1]["time"]));
            crossings.push(ta + (tb - ta) * a / (a - b));
        }
    }
    assert!(crossings.len() >= 3, "too few crossings: {crossings:?}");
    (crossings.last().unwrap() - crossings.first().unwrap()) / (crossings.len() - 1) as f64
}

#[test]
fn a_bar_on_a_hinge_swings_at_the_compound_pendulum_period() {
    let mut s = Session::default();
    frame(&mut s);
    bar(&mut s, "Bar", 0.0);
    run(&mut s, "joint.drive", json!({"joint": "Bar hinge", "value": 2}));
    let before = s.doc.occurrences.clone();
    let r = run(&mut s, "motion.simulate", json!({"duration": 3.0, "step": 0.001, "output_interval": 0.001}));
    assert_eq!(r["groups"].as_array().map(Vec::len), Some(1), "{}", r["groups"]);
    assert_eq!(r["joints"].as_array().map(Vec::len), Some(1));
    // The run starts where the joint is, in the units joint.list gives.
    let first = &r["samples"][0];
    assert!((joint_value(first, 0) - 2.0).abs() < 1e-6, "{}", first["joints"]);
    let measured = mean_period(&r, 0);
    let expected = bar_period(2f64.to_radians());
    assert!((measured - expected).abs() < 2e-4 * expected, "period {measured} vs {expected}");
    // The amplitude holds (no energy lost) and the hinge holds.
    let peak = r["samples"].as_array().unwrap().iter().map(|x| joint_value(x, 0).abs()).fold(0.0, f64::max);
    assert!((peak - 2.0).abs() < 1e-3, "{peak}");
    assert!(num(&r["max_constraint_error_mm"]) < 1e-8);
    // The bar's frame (its component's origin, at the world origin) lies on the hinge axis, so
    // it turns there without moving.
    let pose = &r["samples"][1000]["groups"][0];
    let o = [num(&pose["origin"][0]), num(&pose["origin"][1]), num(&pose["origin"][2])];
    assert!(o.iter().all(|c| c.abs() < 1e-6), "the bar's frame origin is on the hinge axis: {o:?}");
    // Read-only: nothing in the design moved.
    assert_eq!(s.doc.occurrences, before);
}

#[test]
fn a_free_block_falls_freely() {
    let mut s = Session::default();
    part(&mut s, "Block", false, [10.0, 20.0, 30.0], [0.0, 0.0, 0.0]);
    let r = run(&mut s, "motion.simulate", json!({"duration": 0.5, "output_interval": 0.05}));
    let samples = r["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 11);
    for smp in samples {
        let t = num(&smp["time"]);
        let pose = &smp["groups"][0];
        assert!((num(&pose["origin"][2]) + 0.5 * G * t * t).abs() < 1e-6, "t={t}: {pose}");
        assert!(num(&pose["origin"][0]).abs() < 1e-9 && num(&pose["z"][2]) > 1.0 - 1e-12);
        // Kinetic energy gained = potential lost (J).
        let e = &smp["energy"];
        let e0 = &samples[0]["energy"];
        let total = |e: &Value| num(&e["kinetic"]) + num(&e["potential"]);
        assert!((total(e) - total(e0)).abs() < 1e-9, "{e} vs {e0}");
    }
    // Gravity can be given, here along +X at 1000 mm/s².
    let r = run(&mut s, "motion.simulate", json!({"duration": 1.0, "output_interval": 0.5, "gravity": [1000, 0, 0]}));
    assert!((num(&r["samples"][2]["groups"][0]["origin"][0]) - 500.0).abs() < 1e-6, "{}", r["samples"][2]);
}

#[test]
fn geared_bars_keep_their_ratio() {
    // Two equal bars hinged 60 mm apart, geared 10 : 20: the second turns −0.5 of the first.
    // Both swing as one pendulum whose inertia and gravity stiffness are each 1.25 of a bar's,
    // so the period is a single bar's.
    let mut s = Session::default();
    frame(&mut s);
    bar(&mut s, "Left", 0.0);
    bar(&mut s, "Right", 60.0);
    run(&mut s, "joint.gear", json!({"a": "Left hinge", "b": "Right hinge", "ra": 10, "rb": 20}));
    run(&mut s, "joint.drive", json!({"joint": "Left hinge", "value": 2}));
    let r = run(&mut s, "motion.simulate", json!({"duration": 2.0, "output_interval": 0.002}));
    assert_eq!(r["links"], json!([0]));
    for smp in r["samples"].as_array().unwrap() {
        let (a, b) = (joint_value(smp, 0), joint_value(smp, 1));
        assert!((b + 0.5 * a).abs() < 1e-6, "t={}: {a} and {b}", smp["time"]);
    }
    let first = &r["samples"][0];
    assert!((joint_value(first, 0) - 2.0).abs() < 1e-6 && (joint_value(first, 1) + 1.0).abs() < 1e-6, "{}", first["joints"]);
    let measured = mean_period(&r, 0);
    let expected = bar_period(2f64.to_radians());
    assert!((measured - expected).abs() < 1e-3 * expected, "period {measured} vs {expected}");
}

#[test]
fn unsupported_joints_are_named() {
    let mut s = Session::default();
    frame(&mut s);
    part(&mut s, "Pin", false, [10.0, 10.0, 10.0], [-5.0, -5.0, -10.0]);
    run(
        &mut s,
        "joint.create",
        json!({"type": "pin_slot", "name": "Slot", "flip": true,
               "a": {"occurrence": "Frame", "point": [0, 0, 0], "z": [0, 1, 0]},
               "b": {"occurrence": "Pin", "point": [0, 0, 0], "z": [0, 1, 0]}}),
    );
    let e = s.execute("motion.simulate", &json!({"duration": 0.1})).unwrap_err().to_string();
    assert!(e.contains("not supported yet") && e.contains("Slot"), "{e}");
}

#[test]
fn hostile_parameters_are_refused() {
    let mut s = Session::default();
    // Nothing to simulate: the command is disabled.
    assert!(s.execute("motion.simulate", &json!({"duration": 1.0})).is_err());
    frame(&mut s);
    // Everything grounded: still disabled.
    assert!(s.execute("motion.simulate", &json!({"duration": 1.0})).is_err());
    bar(&mut s, "Bar", 0.0);
    for p in [
        json!({}),
        json!({"duration": 0}),
        json!({"duration": -1}),
        json!({"duration": "long"}),
        json!({"duration": 1e9}),
        json!({"duration": 1, "step": 0}),
        json!({"duration": 1, "step": -0.1}),
        json!({"duration": 1, "output_interval": -1}),
        json!({"duration": 1, "gravity": [1, 2]}),
        json!({"duration": 1, "gravity": [1e300, 0, 0]}),
        json!({"duration": 3600, "step": 1e-4}),
        json!({"duration": 1000, "step": 1e-3, "output_interval": 1e-3}),
    ] {
        assert!(s.execute("motion.simulate", &p).is_err(), "{p} should be refused");
    }
    // An output interval shorter than the step is taken as the step.
    let r = run(&mut s, "motion.simulate", json!({"duration": 0.1, "step": 0.05, "output_interval": 0.01}));
    assert_eq!(r["samples"].as_array().map(Vec::len), Some(3));
}
