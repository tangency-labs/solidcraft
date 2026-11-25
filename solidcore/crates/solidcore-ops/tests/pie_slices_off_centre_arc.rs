//! Two pie slices whose arc centre misses the apex, side by side with a gap between their
//! facing walls: the union is the two slices touching along the apex edge.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test

use std::f64::consts::PI;

use solidcore_ops::boolean::boolean;
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{session, Body, BooleanOp, CheckLevel};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

const RHO: f64 = 85.5;
const H: f64 = 119.0;

/// The corners of a 120° pie slice symmetric about +y, `RHO` from the apex.
#[allow(clippy::disallowed_methods)] // the test's operand geometry, not ops computing it
fn corners() -> ([f64; 2], [f64; 2]) {
    let p0 = [RHO * (PI / 6.0).cos(), RHO * (PI / 6.0).sin()];
    (p0, [-p0[0], p0[1]])
}

/// A pie slice of height `H` whose arc is the circle about (0, `c`) through its corners
/// (SolidCraft draws the arc through its rounded end points, so its centre can miss the
/// apex), turned by `turn` about z.
#[allow(clippy::disallowed_methods)] // the test's operand geometry, not ops computing it
fn sector(c: f64, turn: f64) -> Body {
    let (p0, p1) = corners();
    let (ct, st) = (turn.cos(), turn.sin());
    let at = |p: [f64; 2]| [ct * p[0] - st * p[1], st * p[0] + ct * p[1], 0.0];
    let profile = [
        Curve::Line {
            start: [0.0; 3],
            end: at(p0),
        },
        Curve::Arc {
            center: at([0.0, c]),
            axis: [0.0, 0.0, 1.0],
            reference: at([1.0, 0.0]),
            radius: (p0[0].powi(2) + (p0[1] - c).powi(2)).sqrt(),
            start: (p0[1] - c).atan2(p0[0]),
            end: (p1[1] - c).atan2(p1[0]),
        },
        Curve::Line {
            start: at(p1),
            end: [0.0; 3],
        },
    ];
    extrude(
        &profile,
        [0.0, 0.0, 1.0],
        H,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap()
}

/// The slice's volume in closed form: the triangle apex-corner-corner plus the circular
/// segment between the chord and the arc, times `H`.
#[allow(clippy::disallowed_methods)] // the closed form the test checks against
fn slice_volume(c: f64) -> f64 {
    let (p0, p1) = corners();
    let r = (p0[0].powi(2) + (p0[1] - c).powi(2)).sqrt();
    let theta = 2.0 * (p0[0] / r).asin();
    let triangle = 0.5 * (p0[0] * p1[1] - p0[1] * p1[0]);
    (triangle + r * r / 2.0 * (theta - theta.sin())) * H
}

/// The second slice is turned 120° + `apart` about the apex, so the facing walls part by
/// `apart` · 85.5 at the corners (2.8e-5 and 8.6e-5, past the 2e-5 band). With the arc
/// centre 4.85e-5 off the apex the two arc cylinders cross 1.41e-5 from both corners, and
/// the crossing's ends classify onto A's corner vertex in A and onto B's in B, 2.82e-5
/// apart. A proto vertex relating them merged the corners: `NotSolid`. Now no proto vertex
/// relates them, and the union is the two slices touching along the apex edge: 2 solids,
/// 10 faces, 17 edges, 10 vertices, V = 2 · slice in closed form.
#[test]
fn pie_slices_with_an_off_centre_arc_and_parted_walls_touch_along_the_apex() {
    all_fixes();
    for c in [-4.85e-5, 4.85e-5] {
        for apart in [3.3e-7, 1e-6] {
            let a = sector(c, 0.0);
            let b = sector(c, 2.0 * PI / 3.0 + apart);
            let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
            let report = u.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{c} {apart}: {}", report.text);
            assert_eq!(u.solid_count().unwrap(), 2, "{c} {apart}");
            assert_eq!(u.counts().unwrap(), [10, 17, 10], "{c} {apart}");
            let v = u.volume(1e-8).unwrap();
            let want = 2.0 * slice_volume(c);
            let err = (v - want).abs();
            assert!(
                !(err.is_nan() || err > 1e-7 * want),
                "{c} {apart}: {v}, closed form {want}"
            );
        }
    }
}
