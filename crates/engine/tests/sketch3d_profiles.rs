//! Planar 3D sketch loops as profiles (#741): the issue's pyramid repro through the commands —
//! `sketch.inspect` lists the tilted triangles, and `surface.patch` builds them in their planes.

use serde_json::{Value, json};
use solidcraft_engine::Session;

/// Triangle area: base 20, the apex 15 above the centre: ½·20·√325.
const TRI_AREA: f64 = 180.277_563_773_199_46;

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

/// The issue's repro sketch: a 20×20 square on XY with 3D lines from its corners to (0,0,15).
fn pyramid(legs: &[[[f64; 3]; 2]]) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({ "plane": "XY", "name": "S" }));
    run(&mut s, "sketch.rectangle.two_point", json!({ "p0": [-10.0, -10.0], "p1": [10.0, 10.0] }));
    run(&mut s, "sketch.options", json!({ "sketch": "S", "sketch_3d": true }));
    for leg in legs {
        run(&mut s, "sketch.line3d", json!({ "points": [leg[0], leg[1]] }));
    }
    run(&mut s, "sketch.finish", json!({}));
    s
}

const CORNERS: [[f64; 2]; 4] = [[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]];

fn legs_to_apex() -> Vec<[[f64; 3]; 2]> {
    CORNERS.iter().map(|c| [[c[0], c[1], 0.0], [0.0, 0.0, 15.0]]).collect()
}

#[test]
fn the_issue_repro_lists_the_tilted_triangle() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({ "plane": "XY", "name": "S" }));
    run(&mut s, "sketch.rectangle.two_point", json!({ "p0": [-10.0, -10.0], "p1": [10.0, 10.0] }));
    run(&mut s, "sketch.options", json!({ "sketch": "S", "sketch_3d": true }));
    run(&mut s, "sketch.line3d", json!({ "points": [[-10.0, -10.0, 0.0], [0.0, 0.0, 15.0]] }));
    run(&mut s, "sketch.line3d", json!({ "points": [[10.0, -10.0, 0.0], [0.0, 0.0, 15.0]] }));
    run(&mut s, "sketch.finish", json!({}));
    let si = run(&mut s, "sketch.inspect", json!({ "sketch": "S" }));
    assert_eq!(si["profiles"].as_array().unwrap().len(), 1, "the square");
    let p3 = si["profiles_3d"].as_array().unwrap();
    assert_eq!(p3.len(), 1, "the triangle is listed: {si}");
    assert!((p3[0]["area_mm2"].as_f64().unwrap() - TRI_AREA).abs() < 1e-6);
    // The plane holds the apex and one outline edge: its normal is horizontal-ish and tilted.
    let normal = p3[0]["plane"]["normal"].as_array().unwrap().clone();
    let n: Vec<f64> = normal.iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((n.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-9, "unit normal: {n:?}");
}

#[test]
fn patching_the_pyramid_sides_builds_four_faces_in_their_planes() {
    let legs = legs_to_apex();
    let mut s = pyramid(&legs);
    let si = run(&mut s, "sketch.inspect", json!({ "sketch": "S" }));
    assert_eq!(si["profiles_3d"].as_array().unwrap().len(), 4);
    let out = run(&mut s, "surface.patch", json!({ "sketch": "S", "profiles_3d": [0, 1, 2, 3] }));
    assert_eq!(out["bodies"].as_array().unwrap().len(), 4, "{out}");
    let m = run(&mut s, "inspect.measure", json!({}));
    let areas: Vec<f64> = m["bodies"].as_array().unwrap().iter().map(|b| b["area_mm2"].as_f64().unwrap()).collect();
    assert_eq!(areas.len(), 4, "{m}");
    for a in &areas {
        assert!((a - TRI_AREA).abs() < 1e-4, "each side is the triangle: {areas:?}");
    }
}

#[test]
fn a_3d_loop_patch_leaves_the_sketch_plane_profiles_alone() {
    // Only one of the four sides is patched: the square (a 2D profile) must not be patched too.
    let legs = legs_to_apex();
    let mut s = pyramid(&legs);
    let out = run(&mut s, "surface.patch", json!({ "sketch": "S", "profiles_3d": [0] }));
    assert_eq!(out["bodies"].as_array().unwrap().len(), 1, "{out}");
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["body_count"].as_u64().unwrap(), 1);
}

#[test]
fn a_sketch_with_only_3d_loops_patches() {
    // A tilted square drawn entirely with 3D lines (no in-plane curves at all): the sketch has
    // no 2D profiles, and its one 3D loop patches.
    let corners = [[-6.0, -6.0, 0.0], [6.0, -6.0, 4.0], [6.0, 6.0, 4.0], [-6.0, 6.0, 0.0]];
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({ "plane": "XY", "name": "T" }));
    run(&mut s, "sketch.options", json!({ "sketch": "T", "sketch_3d": true }));
    for k in 0..4 {
        run(&mut s, "sketch.line3d", json!({ "points": [corners[k], corners[(k + 1) % 4]] }));
    }
    run(&mut s, "sketch.finish", json!({}));
    let si = run(&mut s, "sketch.inspect", json!({ "sketch": "T" }));
    assert_eq!(si["profiles"].as_array().unwrap().len(), 0);
    assert_eq!(si["profiles_3d"].as_array().unwrap().len(), 1);
    let out = run(&mut s, "surface.patch", json!({ "sketch": "T", "profiles_3d": [0] }));
    assert_eq!(out["bodies"].as_array().unwrap().len(), 1, "{out}");
    // The tilted square: sides 12 and √(12²+4²) → area 12·√160.
    let m = run(&mut s, "inspect.measure", json!({}));
    let area = m["total"]["area_mm2"].as_f64().unwrap();
    assert!((area - 12.0 * 160.0f64.sqrt()).abs() < 1e-4, "{area}");
}

#[test]
fn a_missing_3d_profile_index_is_refused() {
    let legs = legs_to_apex();
    let mut s = pyramid(&legs);
    let err = s.execute("surface.patch", &json!({ "sketch": "S", "profiles_3d": [9] })).unwrap_err();
    assert!(err.to_string().contains("profile"), "{err}");
}
