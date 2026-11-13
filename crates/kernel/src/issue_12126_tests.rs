//! Issue #12126: a sweep whose profile reaches past the centre of a bend in its path. SMLib
//! refuses it (the arc's piece is a turn about an axis the profile crosses), and the message
//! said only "SMLib failed (status 1025)".

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::FRAC_PI_2;

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::{PathSeg, sweep};

/// A square of side `side` on the plane across the path's start (the YZ plane at the origin).
fn square(side: f64) -> Region2 {
    let h = side / 2.0;
    Region2 { outer: Loop2::polygon(&[Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)]), holes: vec![] }
}

fn across_start() -> Plane {
    Plane { origin: Vec3::ZERO, x: Vec3::Y, y: Vec3::Z }
}

/// A straight run along X, then a quarter turn of `radius` towards +Y.
fn line_then_bend(radius: f64) -> [PathSeg; 2] {
    [
        PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(20.0, 0.0, 0.0) },
        PathSeg::Arc { a: Vec3::new(20.0, 0.0, 0.0), center: Vec3::new(20.0, radius, 0.0), axis: Vec3::Z, angle: FRAC_PI_2 },
    ]
}

#[test]
fn a_profile_past_the_centre_of_a_bend_names_the_bend() {
    // The square reaches 2 mm towards the bend's centre, which is 1.5 mm away.
    let e = sweep(&across_start(), &square(4.0), &line_then_bend(1.5)).expect_err("the profile crosses the bend's axis");
    let msg = e.to_string();
    assert!(!msg.contains("status 1025"), "{msg}");
    assert!(msg.contains("bend") && msg.contains("1.5"), "{msg}");
}

#[test]
fn a_profile_inside_the_bend_sweeps() {
    // The same square round a 3 mm bend stays clear of its centre.
    sweep(&across_start(), &square(4.0), &line_then_bend(3.0)).unwrap();
}
