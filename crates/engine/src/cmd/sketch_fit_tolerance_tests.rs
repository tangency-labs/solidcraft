//! `sketch.spline.fit_tolerance`: one spline through as few of a contour's many points as keep
//! every point within a tolerance (#20048).

use serde_json::{Value, json};
use solidcraft_geom::MeshGeom;
use solidcraft_sketch::{CurveKind, Sketch};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

/// The sketch being edited, or else the design's first feature's sketch once it is finished.
fn sketch(s: &Session) -> Sketch {
    let id = s.active_sketch.unwrap_or_else(|| s.doc.features.first().unwrap().id);
    s.model.state().sketch(id).unwrap().sketch.clone()
}

fn bodies(s: &Session) -> Vec<(String, f64)> {
    s.model
        .state()
        .bodies
        .iter()
        .map(|b| (b.name.clone(), b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-3)).unwrap().measure().volume))
        .collect()
}

/// The issue's contour: 800 samples of a sine, past the 500 a fit point spline takes.
fn sine_points() -> Vec<[f64; 2]> {
    (0..800)
        .map(|i| {
            let x = 80.0 * i as f64 / 799.0;
            [x, 10.0 * (x / 10.0).sin()]
        })
        .collect()
}

/// The issue's 800-point contour becomes one spline within the tolerance, solved, as a cubic fit
/// spline through the points it chose.
#[test]
fn the_issue_s_800_point_contour_becomes_one_spline_within_the_tolerance() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.spline.fit_tolerance", json!({"points": sine_points()}));
    assert_eq!(r["curves"].as_array().map(Vec::len), Some(1), "{r}");
    assert_eq!(r["closed"], false);
    let points = r["points"].as_u64().unwrap();
    assert!((10..=150).contains(&points), "chose {points} points");
    assert!(r["deviation"].as_f64().unwrap() <= 0.01, "{r}");
    assert_eq!(r["sketch"]["solved"], true, "{r}");
    let sk = sketch(&s);
    assert!(
        matches!(&sk.curves[..], [c] if matches!(c.kind, CurveKind::Spline { control: false, periodic: false, degree: 3, nurbs: None, .. })),
        "{:?}",
        sk.curves
    );
}

/// A closed 800-point contour extrudes to its body: the spline stays within the tolerance of the
/// circle, so the volume lies in a band around π·r²·h.
#[test]
fn a_closed_contour_extrudes_to_its_body_within_the_tolerance() {
    // 800 points around a circle of radius 10, the last repeating the first.
    let pts: Vec<[f64; 2]> = (0..800)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 799.0;
            [10.0 * a.cos(), 10.0 * a.sin()]
        })
        .collect();
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.spline.fit_tolerance", json!({"points": pts, "periodic": true}));
    assert_eq!(r["closed"], true, "{r}");
    assert!(r["points"].as_u64().unwrap() < 800, "{r}");
    assert!(r["deviation"].as_f64().unwrap() <= 0.01, "{r}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    let found = bodies(&s);
    assert_eq!(found.len(), 1, "{found:?}");
    let want = std::f64::consts::PI * 100.0 * 5.0;
    assert!((found[0].1 - want).abs() < 5.0, "volume {} want {want}", found[0].1);
}

/// Points too rough for a smooth spline refuse, naming that even the most points cannot reach
/// the tolerance (#20048).
#[test]
fn rough_points_refuse_naming_that_even_500_points_cannot_reach_it() {
    let mut s = new_sketch();
    let pts: Vec<[f64; 2]> = (0..1000).map(|i| [0.1 * i as f64, 0.5 * (i % 2) as f64]).collect();
    let err = match s.execute("sketch.spline.fit_tolerance", &json!({"points": pts, "tolerance": 0.0001})) {
        Err(e) => e.to_string(),
        Ok(v) => panic!("refused nothing: {v}"),
    };
    assert!(err.contains("do not reach the tolerance"), "{err}");
    assert!(sketch(&s).curves.is_empty(), "a refused call adds nothing");
}

/// Hostile parameters are refused and leave the sketch unchanged.
#[test]
fn hostile_parameters_are_refused() {
    let mut s = new_sketch();
    let too_many: Vec<[f64; 2]> = (0..10001).map(|i| [i as f64, 0.0]).collect();
    for p in [
        json!({}),
        json!({"points": [[0, 0]]}),
        json!({"points": too_many}),
        json!({"points": [[0, 0], [1, 1]], "tolerance": 11.0}),
        json!({"points": [[0, 0], [1, 1]], "tolerance": 0}),
        json!({"points": [[0, 0], [1, 1]], "periodic": "yes"}),
    ] {
        assert!(s.execute("sketch.spline.fit_tolerance", &p).is_err(), "refused nothing: {p}");
    }
    assert!(sketch(&s).curves.is_empty());
}
