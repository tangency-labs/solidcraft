//! Regression test for WR-532 (the fork's tangency-labs/smlib@b283ea2, capi test
//! `shelled_rounded_box_rim`): a 60 x 40 x 20 box with its four vertical edges rounded
//! r6, shelled 2 mm inward open at the top. The cavity's rounds are the r4 offsets of
//! the outer ones. With `offset_extends_cones_as_themselves` the extended offset
//! extends the rounds' cylinders as themselves, so the rim faces that reach a hair
//! past the rounds' ends stay on the circle, and every cylinder of the result has the
//! radius of its round to 1e-12. A single r5 round comes back a cylinder of radius 3
//! (tangency-labs/smlib@3626d4b, capi `shelled_single_round_stays_a_cylinder`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::SurfaceKind;
use solidcore::Kernel;

/// A 60 x 40 x 20 box with `count` of its vertical edges rounded `radius`, shelled 2 mm
/// inward open at the top with the opt-in: the radii of the result's cylinder faces.
fn shelled_round_radii(count: usize, radius: f64) -> Vec<f64> {
    let mut k = Kernel::default();
    k.fixes.offset_extends_cones_as_themselves = true;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let vertical: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(&k, e.interval.min()).unwrap();
            let q = c.evaluate_point(&k, e.interval.max()).unwrap();
            (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9
        })
        .map(|(id, _)| id)
        .take(count)
        .collect();
    assert_eq!(vertical.len(), count, "the vertical edges");
    fillet_edges(&mut k, &mut b, &vertical, radius, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    let top = planar_faces_at(&k, &b, 2, 20.0);
    assert_eq!(top.len(), 1, "the top face");
    let result = shell(&mut k, b, -2.0, &top).unwrap();
    let mut radii = Vec::new();
    for &f in result.face_list() {
        if let Some(SurfaceKind::Cone(cone)) = result
            .face(f)
            .unwrap()
            .surface
            .as_ref()
            .map(solidcore::surface::surface::Surface::kind)
        {
            radii.push(cone.bot_radius());
        }
    }
    radii
}

/// All four vertical edges rounded r6: the cavity's four r4 rounds, and the r6 ones
/// where the merge made them cylinders too, on their radii to 1e-12.
#[test]
fn wr_532_shelled_rounds_keep_their_radii() {
    let radii = shelled_round_radii(4, 6.0);
    let inner: Vec<f64> = radii
        .iter()
        .copied()
        .filter(|r| (r - 4.0).abs() < 1e-3)
        .collect();
    assert_eq!(inner.len(), 4, "the cavity's four rounds: {radii:?}");
    for r in radii
        .into_iter()
        .filter(|r| (r - 4.0).abs() < 1e-3 || (r - 6.0).abs() < 1e-3)
    {
        let want = if r > 5.0 { 6.0 } else { 4.0 };
        assert!(
            (r - want).abs() <= 1e-12,
            "a round of radius {r}, want {want}"
        );
    }
}

/// tangency-labs/smlib@3626d4b (capi `shelled_single_round_stays_a_cylinder`): one
/// vertical edge rounded r5, a round in one rational span. Extended round its axis one
/// side at a time with SMLib's circle knots kept, the cavity's round is found to be a
/// cylinder again, of radius 3.
#[test]
fn wr_532_single_round_stays_a_cylinder() {
    let radii = shelled_round_radii(1, 5.0);
    let inner: Vec<f64> = radii
        .iter()
        .copied()
        .filter(|r| (r - 3.0).abs() < 1e-3)
        .collect();
    assert_eq!(inner.len(), 1, "the cavity's round: {radii:?}");
    assert!((inner[0] - 3.0).abs() <= 1e-12, "radius {}", inner[0]);
}
