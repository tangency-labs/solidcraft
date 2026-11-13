//! Two pie slices whose arc centre misses the apex, the second turned so that the facing walls
//! overlap by a thin wedge: the two arc cylinders, whose axes lie 8.4e-5 apart, coincide only
//! over the sliver between the two corners.
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

/// The overlap wedge of walls turned `overlap` into each other: the triangle apex-corner-corner
/// times `H`. Between the two corners (8.55e-5 apart) the arc bulges past the chord by
/// chord² / (8 R) = 1.1e-11, so the segment adds 6e-16 per unit height, below the volume's
/// own rounding.
#[allow(clippy::disallowed_methods)] // the closed form the test checks against
fn wedge_volume(overlap: f64) -> f64 {
    0.5 * RHO * RHO * overlap.sin() * H
}

/// The second slice is turned 120° − 1e-6 rad, so its first wall lies 8.55e-5 inside the
/// first slice at the corners. The arc centres miss the apex by 4.85e-5 along each slice's
/// bisector, so the two arc cylinders' axes lie 8.4e-5 apart, yet over the sliver between the
/// corners they stay within 1e-10 of each other. Deciding their coincidence over the whole
/// circles called them apart: the intersection and the difference failed (1001 or
/// `NotSolid`). Now both are the closed-form wedge and the slice minus it.
#[test]
fn pie_slices_with_off_centre_arcs_and_overlapping_walls_meet_in_the_wedge() {
    all_fixes();
    let overlap = 1e-6;
    let wedge = wedge_volume(overlap);
    for c in [-4.85e-5, 4.85e-5] {
        for (op, counts, want) in [
            (BooleanOp::Intersect, [5, 11, 8], wedge),
            (BooleanOp::Subtract, [5, 9, 6], slice_volume(c) - wedge),
        ] {
            let a = sector(c, 0.0);
            let b = sector(c, 2.0 * PI / 3.0 - overlap);
            let r = boolean(a, b, op).unwrap().unwrap();
            let report = r.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{c} {op:?}: {}", report.text);
            assert_eq!(r.solid_count().unwrap(), 1, "{c} {op:?}");
            assert_eq!(r.counts().unwrap(), counts, "{c} {op:?}");
            let v = r.volume(1e-10).unwrap();
            // the wedge is 0.435; its volume is measured to 1e-10 (1e-9 of the slice)
            let err = (v - want).abs();
            assert!(
                !(err.is_nan() || err > 1e-6),
                "{c} {op:?}: {v}, closed form {want}"
            );
        }
    }
}
