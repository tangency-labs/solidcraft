//! solidcraft#13085: a revolve cut whose tool has a cylindrical face a hair (1e-10) outside the
//! wall it is meant to coincide with, meeting it at a torus/cylinder tangent junction. The
//! boolean builds a body whose loops are out of tolerance (gaps of 5.9e-5 and 2.9e-5 against
//! 2e-5), or fails in `merge::p2::make_faceuse` for offsets of 1e-9 to 1e-6. With the face
//! exactly on the wall, or 1e-4 or more away, the cut is right.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_ops::boolean::{boolean, BooleanOp};
use solidcore_ops::sweep::{extrude, revolve, Curve, DraftCorner};
use solidcore_ops::{Body, CheckLevel};

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

fn arc(
    center: [f64; 3],
    axis: [f64; 3],
    reference: [f64; 3],
    radius: f64,
    start: f64,
    end: f64,
) -> Curve {
    Curve::Arc {
        center,
        axis,
        reference,
        radius,
        start,
        end,
    }
}

/// The cup: a half-disc of radius 24.75 (y <= 0) with a 7 mm lug down to y = -34.75 (its end
/// round r 3.5 about (0, -31.25), its junctions with the ring filleted r 5 about
/// (+-8.5, -28.5099), tangent to the ring and to the lug's sides x = +-3.5) and a 2.5 mm eye
/// hole through the lug, extruded 20; a half-disc of radius 23.75 cut 19 deep from the top
/// (a 1 mm wall and floor).
fn cup() -> Body {
    const R: f64 = 24.75;
    const FY: f64 = -28.509_866_713_120_786;
    let z = [0.0, 0.0, 1.0];
    let zn = [0.0, 0.0, -1.0];
    let x = [1.0, 0.0, 0.0];
    // The ring's tangent point with a fillet: along the line from the origin through the
    // fillet's centre, 24.75 out.
    let t = (-23.718_292_475_621_443_f64).atan2(-7.071_428_571_428_571); // -1.8605 rad
    let outline = [
        line([R, 0.0, 0.0], [-R, 0.0, 0.0]),
        arc([0.0; 3], z, x, R, PI, t + TAU),
        arc([-8.5, FY, 0.0], zn, x, 5.0, -(PI + t), 0.0),
        line([-3.5, FY, 0.0], [-3.5, -31.25, 0.0]),
        arc([0.0, -31.25, 0.0], z, x, 3.5, PI, TAU),
        line([3.5, -31.25, 0.0], [3.5, FY, 0.0]),
        arc([8.5, FY, 0.0], zn, x, 5.0, -PI, t),
        arc([0.0; 3], z, x, R, -(PI + t), 0.0),
        arc([0.0, -31.25, 0.0], zn, x, 2.5, -TAU, 0.0),
    ];
    let body = extrude(&outline, z, 20.0, 0.0, DraftCorner::Curvature, true).unwrap();
    let inner = [
        line([23.75, 0.0, 1.0], [-23.75, 0.0, 1.0]),
        arc([0.0, 0.0, 1.0], z, x, 23.75, PI, TAU),
    ];
    let hollow = extrude(&inner, z, 19.0, 0.0, DraftCorner::Curvature, true).unwrap();
    boolean(body, hollow, BooleanOp::Subtract)
        .unwrap()
        .expect("the cup")
}

/// The tool: the profile Y in [-34.75, wall], Z in [5, 20], its lower corner a quarter arc of
/// radius 10 about (-34.75, 15) ending tangent to the vertical line at (`wall`, 15), revolved
/// `angle` about Z, placed symmetrically about the YZ plane (the profile turned by -angle/2).
fn tool(wall: f64, angle: f64) -> Body {
    let (s, c) = (-angle / 2.0).sin_cos();
    let turn = |p: [f64; 3]| [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]];
    let turn_dir = |d: [f64; 3]| [c * d[0] - s * d[1], s * d[0] + c * d[1], d[2]];
    let profile = [
        line(turn([0.0, -34.75, 20.0]), turn([0.0, -34.75, 5.0])),
        arc(
            turn([0.0, -34.75, 15.0]),
            turn_dir([1.0, 0.0, 0.0]),
            turn_dir([0.0, 1.0, 0.0]),
            10.0,
            -FRAC_PI_2,
            0.0,
        ),
        line(turn([0.0, wall, 15.0]), turn([0.0, wall, 20.0])),
        line(turn([0.0, wall, 20.0]), turn([0.0, -34.75, 20.0])),
    ];
    revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], angle, true).unwrap()
}

/// The cut, or why it failed: the boolean's error, or no body.
fn cut(wall: f64, angle_deg: f64) -> Result<Body, String> {
    match boolean(
        cup(),
        tool(wall, angle_deg.to_radians()),
        BooleanOp::Subtract,
    ) {
        Ok(Some(b)) => Ok(b),
        Ok(None) => Err("no body".to_string()),
        Err(e) => Err(format!("{e}")),
    }
}

/// The tool's vertical face exactly on the cup's outer wall: the cut is right.
#[test]
fn a_tool_face_on_the_wall_cuts() {
    for angle in [28.0, 36.0] {
        let b = cut(-24.75, angle).unwrap_or_else(|e| panic!("{angle} deg: {e}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{angle} deg:\n{}", report.text);
    }
}

/// The tool's vertical face 1e-10 outside the wall (a sketch's rounding): the 36 degree cut
/// fails the check with loops out of tolerance; at 28 degrees the tool's end planes clear the
/// lug's fillets and the cut is right.
#[test]
fn a_tool_face_a_hair_outside_the_wall_cuts_too() {
    for angle in [28.0, 36.0] {
        let b = cut(-24.75 - 1e-10, angle).unwrap_or_else(|e| panic!("{angle} deg: {e}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{angle} deg:\n{}", report.text);
    }
}

/// The same a hair outside the wall at the offsets where the boolean fails outright on main.
#[test]
fn a_tool_face_a_little_outside_the_wall_cuts_too() {
    for off in [1e-9, 1e-7, 1e-5] {
        let b = cut(-24.75 - off, 36.0).unwrap_or_else(|e| panic!("offset {off}: {e}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "offset {off}:\n{}", report.text);
    }
}
