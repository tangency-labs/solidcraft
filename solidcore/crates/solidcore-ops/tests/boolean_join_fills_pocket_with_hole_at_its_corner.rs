//! solidcraft#12785: a block with a hexagonal pocket through it and a round hole drilled from
//! an end face along the pocket's axis plane, ending exactly at a corner of the pocket, so
//! the hole's bottom meets the two pocket walls along a segment of the corner edge (a solid
//! touching itself along a line). The cut leaves that segment as one edge of four faces; a
//! record round trip (`to_record`, `from_record`: SolidCraft's body between features) writes
//! it as two edges of two faces each, and filling the pocket with the same hexagon as a join
//! then fails (`topology_intersector::p1::relate`, status 1001, or a result that is not a
//! closed solid). Without the hole, or without the round trip, the same join works.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::TAU;

use solidcore_ops::boolean::{boolean, BooleanOp};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{Body, CheckLevel};

/// The hexagon's circumradius.
const R: f64 = 6.3685;
const Z: [f64; 3] = [0.0, 0.0, 1.0];

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

/// The regular hexagon of circumradius `R` about the origin with a corner on +X, at z = 0
/// (the reporter's sketch: the corner at (R, 0), the next at (R/2, R sqrt(3)/2)).
fn hexagon() -> Vec<Curve> {
    const H: f64 = 5.515_282_784_001_197;
    let corners = [
        [R, 0.0, 0.0],
        [R / 2.0, H, 0.0],
        [-R / 2.0, H, 0.0],
        [-R, 0.0, 0.0],
        [-R / 2.0, -H, 0.0],
        [R / 2.0, -H, 0.0],
    ];
    (0..6)
        .map(|i| line(corners[i], corners[(i + 1) % 6]))
        .collect()
}

/// The block 100.29 x 32 x 10.
fn block() -> Body {
    extrude(
        &[
            line([-50.145, -16.0, 0.0], [50.145, -16.0, 0.0]),
            line([50.145, -16.0, 0.0], [50.145, 16.0, 0.0]),
            line([50.145, 16.0, 0.0], [-50.145, 16.0, 0.0]),
            line([-50.145, 16.0, 0.0], [-50.145, -16.0, 0.0]),
        ],
        Z,
        10.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap()
}

/// The hexagon extruded through the block's height: the pocket's cutter and the join's tool.
fn tool() -> Body {
    extrude(&hexagon(), Z, 10.0, 0.0, DraftCorner::Curvature, true).unwrap()
}

/// The block with the pocket through it and the hole: 2.5 mm across, along -X from the end
/// face x = 50.145, 43.7765 deep, so it ends at the pocket's corner (R, 0).
fn target() -> Body {
    let pocketed = boolean(block(), tool(), BooleanOp::Subtract)
        .unwrap()
        .expect("the pocketed block");
    let hole = extrude(
        &[Curve::Arc {
            center: [50.145, 0.0, 5.0],
            axis: [-1.0, 0.0, 0.0],
            reference: [0.0, 1.0, 0.0],
            radius: 1.25,
            start: 0.0,
            end: TAU,
        }],
        [-1.0, 0.0, 0.0],
        50.145 - R,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    boolean(pocketed, hole, BooleanOp::Subtract)
        .unwrap()
        .expect("the drilled block")
}

/// `body` as SolidCraft hands it to the next feature: through its record.
fn round_tripped(body: &Body) -> Body {
    let (record, _) = body.to_record().unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// Joins `target` and the hexagon, checks the result and compares its volume with the sum.
fn join_fills(target: Body, what: &str) {
    let want = target.volume(1e-9).unwrap() + tool().volume(1e-9).unwrap();
    let b = match boolean(target, tool(), BooleanOp::Union) {
        Ok(Some(b)) => b,
        Ok(None) => panic!("{what}: no body"),
        Err(e) => panic!("{what}: {e}"),
    };
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{what}:\n{}", report.text);
    let got = b.volume(1e-9).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "{what}: volume {got} vs {want}"
    );
}

/// The cut's own body: the corner segment is one edge of four faces, and the join works.
#[test]
fn filling_a_pocket_whose_corner_a_hole_reaches_joins() {
    let target = target();
    assert_eq!(target.counts().unwrap(), [15, 37, 24]);
    join_fills(target, "the cut's body");
}

/// The same body after its record round trip: the corner segment is two coincident edges of
/// two faces each, and the join must work as well.
#[test]
fn filling_a_pocket_whose_corner_a_hole_reaches_joins_after_a_record_round_trip() {
    let target = round_tripped(&target());
    assert_eq!(target.counts().unwrap(), [15, 38, 24]);
    assert!(target.check(CheckLevel::All).unwrap().is_valid());
    join_fills(target, "the round-tripped body");
}

/// The control: without the hole the same join works, round trip or not.
#[test]
fn filling_a_pocket_without_the_hole_joins() {
    let pocketed = boolean(block(), tool(), BooleanOp::Subtract)
        .unwrap()
        .expect("the pocketed block");
    join_fills(round_tripped(&pocketed), "the pocketed block");
}
