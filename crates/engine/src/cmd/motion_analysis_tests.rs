//! `motion.plot` against the analytic motion of a four-bar and a slider-crank, and
//! `motion.interference` on two blocks pushed through each other at a known step.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::{four_bar, occ, part, pin, rocker_pin, run, slider_crank};
use crate::Session;

/// A joint's series in a plot, by joint name.
fn series<'a>(r: &'a Value, name: &str) -> &'a Value {
    r["joints"].as_array().and_then(|list| list.iter().find(|j| j["name"] == name)).unwrap_or(&Value::Null)
}

fn nums(v: &Value) -> Vec<f64> {
    v.as_array().into_iter().flatten().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()
}

/// The crank turns at a constant 50°/s (a full turn in 7.2 s). The rocker turns at
/// ω4 = ω2·a·sin(θ2 − θ3) / (c·sin(θ4 − θ3)) (crank a = 20, rocker c = 40; θ2, θ3 and θ4 the
/// crank, coupler and rocker angles), from differentiating the loop a·e^iθ2 + b·e^iθ3 = d + c·e^iθ4.
#[test]
fn four_bar_rocker_velocity_matches_its_analytic_derivative() {
    let (mut s, _, q0) = four_bar();
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 360, "keys": [{"joint": "Crank", "points": [[0, 0], [360, 360]]}]}));
    let before = s.doc.clone();
    let r = run(&mut s, "motion.plot", json!({"study": "Turn", "joints": ["Crank", "Rocker"], "samples": 181, "duration": 7.2}));
    assert_eq!(*s.doc, *before, "a plot leaves the design as it was");
    assert!(r["stopped_at"].is_null() && r["samples"] == 181, "{}", r["stopped_by_loop"]);
    assert_eq!(r["joints"].as_array().map(Vec::len), Some(2));
    let (crank, rocker) = (series(&r, "Crank"), series(&r, "Rocker"));
    assert_eq!((rocker["unit"].as_str(), rocker["velocity_unit"].as_str()), (Some("deg"), Some("deg/s")));
    let time = nums(&r["time"]);
    assert!((time.last().copied().unwrap_or(0.0) - 7.2).abs() < 1e-9 && (time[1] - 0.04).abs() < 1e-12, "{time:?}");
    // The crank: exactly as keyed, at a constant speed.
    for ((v, w), a) in nums(&crank["value"]).iter().zip(nums(&crank["velocity"])).zip(nums(&crank["acceleration"])) {
        assert!((w - 50.0).abs() < 1e-6 && a.abs() < 1e-6, "{v}°: {w} deg/s, {a} deg/s²");
    }
    let (value, velocity) = (nums(&rocker["value"]), nums(&rocker["velocity"]));
    let start = q0.y.atan2(q0.x - 45.0);
    let (mut q, mut peak, mut worst) = (q0, 0.0f64, 0.0f64);
    for k in 0..181 {
        let v = 2.0 * k as f64;
        if k > 0 {
            q = rocker_pin(v - 2.0, v, q);
        }
        let th2 = (90.0 + v).to_radians();
        let p = Vec3::new(20.0 * th2.cos(), 20.0 * th2.sin(), 0.0);
        let (th3, th4) = ((q - p).y.atan2((q - p).x), q.y.atan2(q.x - 45.0));
        // The rocker's value is its turn from where it was assembled, made continuous.
        let turned = (th4 - start).to_degrees();
        assert!(((value[k] - turned + 540.0).rem_euclid(360.0) - 180.0).abs() < 1e-6, "{v}°: {} wants {turned}", value[k]);
        let w4 = 50.0 * 20.0 * (th2 - th3).sin() / (40.0 * (th4 - th3).sin());
        peak = peak.max(w4.abs());
        worst = worst.max((velocity[k] - w4).abs());
    }
    // Central differences 0.04 s apart: within 0.2 % of the fastest swing.
    assert!(peak > 40.0 && worst < 2e-3 * peak, "worst {worst} deg/s of {peak} deg/s");
}

/// The piston of a slider-crank (crank a = 15, rod b = 40) turning at 100°/s sits at
/// x = a·cos θ + S with S = √(b² − a²·sin²θ), so it accelerates at
/// ω²·(−a·cos θ − a²·cos 2θ / S − a⁴·sin²θ·cos²θ / S³).
#[test]
fn slider_crank_piston_acceleration_matches_its_analytic_form() {
    let (mut s, _, s0) = slider_crank();
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 360, "keys": [{"joint": "Crank", "points": [[0, 0], [360, 360]]}]}));
    let p = json!({"study": "Turn", "joints": ["Bore"], "occurrence": "Piston", "point": [s0.x, s0.y, s0.z], "samples": 181, "duration": 3.6});
    let r = run(&mut s, "motion.plot", p);
    assert!(r["stopped_at"].is_null() && r["samples"] == 181, "{}", r["stopped_by_loop"]);
    let point = &r["point"];
    assert_eq!((point["occurrence"].as_str(), point["acceleration_unit"].as_str()), (Some("Piston:1"), Some("mm/s²")));
    let at = |key: &str, k: usize, c: usize| point[key][k][c].as_f64().unwrap_or(f64::NAN);
    let (a, b, w) = (15.0f64, 40.0f64, 100.0f64.to_radians());
    let (mut peak, mut worst) = (0.0f64, 0.0f64);
    for k in 0..181 {
        let th = (90.0 + 2.0 * k as f64).to_radians();
        let root = (b * b - (a * th.sin()).powi(2)).sqrt();
        assert!((at("position", k, 0) - (a * th.cos() + root)).abs() < 1e-6, "{k}");
        let want = w * w * (-a * th.cos() - a * a * (2.0 * th).cos() / root - a.powi(4) * (th.sin() * th.cos()).powi(2) / root.powi(3));
        peak = peak.max(want.abs());
        // The ends take one-sided differences; inside, central ones.
        let tolerance = if k == 0 || k == 180 { 2e-2 } else { 2e-3 };
        worst = worst.max((at("acceleration", k, 0) - want).abs() / tolerance);
        assert!(at("acceleration", k, 1).abs() < 1e-6 && at("acceleration", k, 2).abs() < 1e-6, "it stays on its axis");
    }
    assert!(peak > 40.0 && worst < peak, "worst {worst} of {peak} mm/s² (scaled by tolerance)");
    // The bore's value is the piston's travel, so its acceleration is the piston's along x.
    let bore = series(&r, "Bore");
    assert_eq!(bore["acceleration_unit"], "mm/s²");
    let along = |k: usize| nums(&bore["value"])[k] - nums(&bore["value"])[0];
    let sign = if along(90) * (at("position", 90, 0) - at("position", 0, 0)) < 0.0 { -1.0 } else { 1.0 };
    for (k, acc) in nums(&bore["acceleration"]).iter().enumerate() {
        assert!((acc - sign * at("acceleration", k, 0)).abs() < 1e-6, "{k}: {acc}");
    }
    // Per step without a duration: 2 steps between samples, 1° a step.
    let r = run(&mut s, "motion.plot", json!({"study": "Turn", "joints": ["Crank"], "samples": 181}));
    let crank = series(&r, "Crank");
    assert!(r["time"].is_null() && crank["velocity_unit"] == "deg/step", "{crank}");
    assert!(nums(&crank["velocity"]).iter().all(|v| (v - 1.0).abs() < 1e-9));
}

/// A dial linked 2:1 to the crank turns on past a turn: its values are not taken a turn round,
/// even half a turn a sample.
#[test]
fn a_linked_value_past_half_a_turn_runs_on() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Dial", [80.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Dial", "Ground", "Dial", Vec3::new(80.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Crank", "b": "Dial", "ratio": 2}));
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 2, "keys": [{"joint": "Crank", "points": [[0, 0], [2, 360]]}]}));
    let r = run(&mut s, "motion.plot", json!({"study": "Turn", "joints": ["Dial"], "samples": 3}));
    let v: Vec<f64> = nums(&series(&r, "Dial")["value"]).iter().map(|x| x.abs()).collect();
    assert!((v[1] - 360.0).abs() < 1e-6 && (v[2] - 720.0).abs() < 1e-6, "{r}");
}

/// Cubes A (grounded, x 0…10) and B (x 20…30) on a slider along x, B 10 mm away at value 10.
fn rail() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    s
}

/// B is pushed 1 mm a step from 10 mm away to 5 mm into A: the faces touch at step 10 and the
/// blocks overlap from step 11 to the end, 100 mm³ more each step.
#[test]
fn interference_names_the_step_two_parts_start_to_collide() {
    let mut s = rail();
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [15, -5]]}]}));
    // A contact set would stop the motion where the faces touch; the check plays through.
    run(&mut s, "contact.create", json!({"occurrences": ["A", "B"]}));
    let before = s.doc.clone();
    for p in [json!({"study": "Push"}), json!({"study": "Push", "occurrences": ["A", "B"]}), json!({"study": "Push", "contact_sets": true})] {
        let r = run(&mut s, "motion.interference", p.clone());
        assert_eq!(*s.doc, *before, "the design is left as it was");
        assert!(r["clear"] == false && r["samples"] == 16 && r["pairs"] == 1 && r["truncated"] == false, "{p}: {r}");
        let c = &r["collisions"][0];
        assert_eq!((c["a"].as_str(), c["b"].as_str()), (Some("A:1"), Some("B:1")), "{r}");
        assert_eq!((c["first_step"].as_f64(), c["last_step"].as_f64()), (Some(11.0), Some(15.0)), "{r}");
        assert_eq!(c["steps"], json!([[11.0, 15.0]]), "{r}");
        // Deepest where they share the most: 5 mm of the 10 x 10 face at the end.
        assert!(c["depth_mm"].as_f64().unwrap_or(0.0) > 1.0 && c["deepest_step"] == 15.0, "{r}");
        assert!((c["volume_mm3"].as_f64().unwrap_or(f64::NAN) - 500.0).abs() < 1e-6, "{r}");
    }
    // B against a far block C only: B's pass through A is not asked about.
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 50, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let r = run(&mut s, "motion.interference", json!({"study": "Push", "occurrences": ["B"], "against": ["C"]}));
    assert!(r["clear"] == true && r["pairs"] == 1, "{r}");
    let r = run(&mut s, "motion.interference", json!({"study": "Push", "occurrences": ["B"], "against": ["A", "C"]}));
    assert!(r["pairs"] == 2 && r["collisions"].as_array().map(Vec::len) == Some(1) && r["collisions"][0]["first_step"] == 11.0, "{r}");
    assert!(s.execute("motion.interference", &json!({"study": "Push", "against": ["A"]})).is_err());
    // Pushed only to touching, they never pass through each other.
    run(&mut s, "motion.study", json!({"study": "Push", "keys": [{"joint": "Rail", "points": [[0, 10], [15, 0]]}]}));
    let r = run(&mut s, "motion.interference", json!({"study": "Push"}));
    assert!(r["clear"] == true && r["collisions"] == json!([]), "{r}");
}

/// An arm (40 x 10 x 10 from (0, -5, 0)) turns on a hinge at the origin from 0 to 90 degrees in
/// 9 steps into a grounded block (50 x 10 x 10 from (-10, 30, 0)). Its corner (40, 5) reaches
/// y = 40 sin θ + 5 cos θ, past 30 from 40.97 degrees, so it overlaps from step 5. Every point of
/// the slab is within 5 mm of z = 0 or z = 10, so from 60 degrees on (where the arm's side
/// crosses y = 35) the depth is 5 mm at every step: only the shared volume, which grows to the
/// arm's 10 x 10 x 10 end inside the block at 90 degrees, tells the deepest step. Sampled twice
/// as densely (5 degrees a sample), the overlap is seen from step 4.5 (45 degrees) and is still
/// deepest at the end.
#[test]
fn interference_is_deepest_where_the_parts_share_the_most_volume() {
    let mut s = Session::default();
    for (name, size, corner) in [("Base", [20, 20, 10], [-10, -10, -10]), ("Arm", [40, 10, 10], [0, -5, 0]), ("Stop", [50, 10, 10], [-10, 30, 0])] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    for name in ["Base", "Stop"] {
        let o = occ(&s, name);
        run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    }
    let end = |o: &str| json!({"occurrence": o, "point": [0, 0, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "revolute", "name": "Hinge", "flip": true, "a": end("Base"), "b": end("Arm"), "quiet": true}));
    run(&mut s, "motion.study", json!({"name": "Open", "steps": 9, "keys": [{"joint": "Hinge", "points": [[0, 0], [9, 90]]}]}));
    for (samples, first) in [(10, 5.0), (19, 4.5)] {
        let r = run(&mut s, "motion.interference", json!({"study": "Open", "samples": samples, "occurrences": ["Arm"], "against": ["Stop"]}));
        let c = &r["collisions"][0];
        assert!(r["clear"] == false && c["first_step"] == first && c["last_step"] == 9.0 && c["deepest_step"] == 9.0, "{samples}: {r}");
        assert!((c["volume_mm3"].as_f64().unwrap_or(f64::NAN) - 1000.0).abs() < 1e-6, "{samples}: {r}");
        assert!(c["depth_mm"].as_f64().is_some_and(|d| d > 0.0 && d <= 5.0 + 1e-9), "{samples}: {r}");
    }
}

/// A number names a study by its id first, text by its name first: a study may be named like
/// another's id.
#[test]
fn studies_by_id_or_name() {
    let mut s = rail();
    let keys = json!([{"joint": "Rail", "points": [[0, 10], [15, -5]]}]);
    let first = run(&mut s, "motion.study", json!({"name": "Late", "steps": 15, "keys": keys}))["study"].as_u64().unwrap_or(0);
    run(&mut s, "motion.study", json!({"name": first.to_string(), "steps": 15, "keys": keys}));
    assert_eq!(run(&mut s, "motion.plot", json!({"study": first, "samples": 3}))["study"], "Late");
    assert_eq!(run(&mut s, "motion.plot", json!({"study": first.to_string(), "samples": 3}))["study"], first.to_string());
}

/// Hostile parameters are refused, never a panic.
#[test]
fn motion_analysis_refuses_what_it_cannot_do() {
    let mut s = rail();
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [15, -5]]}]}));
    let refused = [
        ("motion.plot", json!({"study": "Nope"})),
        ("motion.plot", json!({"study": "Push", "samples": 2})),
        ("motion.plot", json!({"study": "Push", "samples": 1e9})),
        ("motion.plot", json!({"study": "Push", "samples": "many"})),
        ("motion.plot", json!({"study": "Push", "duration": 0})),
        ("motion.plot", json!({"study": "Push", "duration": -3})),
        ("motion.plot", json!({"study": "Push", "duration": 1e-200})),
        ("motion.plot", json!({"study": "Push", "joints": ["Nope"]})),
        ("motion.plot", json!({"study": "Push", "joints": vec![json!("Rail"); 300]})),
        ("motion.plot", json!({"study": "Push", "point": [1, 2]})),
        ("motion.plot", json!({"study": "Push", "occurrence": "B", "point": [1e300, 0, 0]})),
        ("motion.interference", json!({"study": "Push", "samples": 1})),
        ("motion.interference", json!({"study": "Push", "samples": f64::MAX})),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A"]})),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A", "B"], "contact_sets": true})),
        ("motion.interference", json!({"study": "Push", "contact_sets": true})),
    ];
    for (cmd, p) in refused {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
    }
    // A point of a part that does not move has no velocity.
    let r = run(&mut s, "motion.plot", json!({"study": "Push", "occurrence": "A", "point": [5, 5, 5]}));
    assert!(r["point"]["speed"].as_array().is_some_and(|v| v.len() == 16 && v.iter().all(|x| x.as_f64() == Some(0.0))), "{r}");
}
