//! Regression test for WR-542 (the fork's tangency-labs/smlib@c0d74ee): both vertical
//! edges at one end of a 40 x 10 x 20 box rounded past half the box's width. Each
//! round's rail on the end face lies inside the band the other takes off it, and no
//! corner trims them (the edges share no vertex); upstream builds a body that is not
//! closed. With `fillet_refuse_overlapping_bands` the fillet is refused (no room), and
//! a radius that fits still builds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::Kernel;

fn round_the_end(radius: f64) -> solidcore::Result<()> {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_overlapping_bands = true;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 10.0, 20.0).unwrap();
    // the two vertical edges at x = 40
    let ends: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(&k, e.interval.min()).unwrap();
            let q = c.evaluate_point(&k, e.interval.max()).unwrap();
            (p.x - 40.0).abs() < 1e-9 && (q.x - 40.0).abs() < 1e-9 && (p.z - q.z).abs() > 1.0
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ends.len(), 2, "the two vertical edges at x = 40");
    fillet_edges(&mut k, &mut b, &ends, radius, 1, 1, 1.0)
}

#[test]
fn wr_542_rounds_wider_than_half_the_end_are_refused() {
    assert!(round_the_end(6.0).is_err());
    round_the_end(4.0).unwrap();
}
