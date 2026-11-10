//! tangency-labs/solidcore#417 (WR-422): a round as wide as the corner round it runs round
//! is centred on that round's axis, where the offset faces collapse, so the rail points
//! carry no angle and the torus could turn the long way round.
//! `fixes.fillet_torus_center_on_axis` turns them into the half-planes through the edge's
//! ends. The case is the fork's capi test: a 297 x 100 x 60 block with its vertical edges
//! rounded R5, then its top rim R5.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The midpoint of edge `e`.
fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

/// The edges whose midpoint satisfies `keep`.
fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| keep(midpoint(k, b, e)))
        .collect()
}

/// The block's volume after its corners are rounded, after its top rim is rounded, and the
/// rim round's result.
fn rounded_block(fix: bool) -> (f64, f64, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_torus_center_on_axis = fix;
    let mut block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 297.0, 100.0, 60.0).unwrap();
    let corners = edges_where(&k, &block, |m| (m.z - 30.0).abs() < 1e-6);
    fillet_edges(&mut k, &mut block, &corners, 5.0, 1, 1, 1.0).unwrap();
    let rounded = volume(&k, &mut block);
    let rim = edges_where(&k, &block, |m| (m.z - 60.0).abs() < 1e-6);
    let r = fillet_edges(&mut k, &mut block, &rim, 5.0, 1, 1, 1.0);
    (rounded, volume(&k, &mut block), r)
}

#[test]
fn the_top_rim_round_turns_the_right_way_round_the_corners_with_the_fix() {
    // each straight edge loses (1 - pi/4) 25 along 287 or 90, each corner the square
    // 5 x 5 less the quarter disc, turned a quarter about the axis: (pi/2) (62.5 - 125/3)
    let removed = 754.0 * 25.0 * (1.0 - PI / 4.0) + 4.0 * (PI / 2.0) * (62.5 - 125.0 / 3.0);

    let (rounded, after, r) = rounded_block(true);
    r.unwrap();
    assert!(
        (after - (rounded - removed)).abs() < 1e-7 * rounded,
        "{rounded} {after}"
    );

    // upstream: the torus turns the long way round and the rollover test fails
    let (_, _, r) = rounded_block(false);
    assert!(r.is_err(), "upstream should fail: {r:?}");
}
