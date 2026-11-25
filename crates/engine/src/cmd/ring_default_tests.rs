//! Sweep, revolve and loft with `profiles` left out keep a ring a ring, as extrude does: a loop
//! inside an outline is its hole, not material filled in (#14395).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn volume(s: &Session, body: &str) -> f64 {
    solidcraft_kernel::measure(&s.world_state().unwrap().body(body).unwrap().body).unwrap().volume
}

/// Two concentric circles, diameter 20 and 10, in a sketch `name` on `plane` at `offset`.
fn ring(s: &mut Session, name: &str, plane: &str, offset: f64, centre: [f64; 2]) {
    run(s, "sketch.create", json!({"plane": plane, "offset": offset, "name": name}));
    for r in [10.0, 5.0] {
        run(s, "sketch.circle.center", json!({"center": centre, "radius": r}));
    }
    run(s, "sketch.finish", json!({}));
}

fn assert_close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() < 5e-3 * want, "{what}: volume {got} vs {want}");
}

#[test]
fn sweeping_a_ring_without_profiles_keeps_the_hole() {
    let mut s = Session::default();
    ring(&mut s, "R", "XY", 0.0, [0.0, 0.0]);
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "P"}));
    // The XZ sketch's y runs down world Z.
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, -40]], "ids": ["up"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.sweep", json!({"sketch": "R", "path_sketch": "P", "path": ["up"], "operation": "new", "body_name": "Tube"}));
    assert_close(volume(&s, "Tube"), PI * 75.0 * 40.0, "sweep");
}

#[test]
fn revolving_a_ring_without_profiles_keeps_the_hole() {
    let mut s = Session::default();
    ring(&mut s, "R", "XZ", 0.0, [30.0, 0.0]);
    run(&mut s, "solid.revolve", json!({"sketch": "R", "axis": "y", "operation": "new", "body_name": "Torus"}));
    assert_close(volume(&s, "Torus"), PI * 75.0 * 2.0 * PI * 30.0, "revolve");
}

#[test]
fn lofting_rings_without_profiles_pairs_the_holes() {
    let mut s = Session::default();
    ring(&mut s, "A", "XY", 0.0, [0.0, 0.0]);
    ring(&mut s, "B", "XY", 20.0, [0.0, 0.0]);
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "A"}, {"sketch": "B"}], "operation": "new", "body_name": "Pipe"}));
    assert_close(volume(&s, "Pipe"), PI * 75.0 * 20.0, "loft");
}

#[test]
fn an_explicit_all_still_fills_the_disc() {
    let mut s = Session::default();
    ring(&mut s, "R", "XZ", 0.0, [30.0, 0.0]);
    run(&mut s, "solid.revolve", json!({"sketch": "R", "axis": "y", "profiles": "all", "operation": "new", "body_name": "Full"}));
    assert_close(volume(&s, "Full"), PI * 100.0 * 2.0 * PI * 30.0, "profiles all");
}
