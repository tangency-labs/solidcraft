//! Regression test for tangency-labs/solidcore#1511: a disc whose seam vertex lies on
//! the axis of a rotational sweep. Upstream's `SmTopologySweep::SweepEdgeHigh` read the
//! base edge's curve after `MakeFaceWithCurves` had let it go (the swept vertex is a
//! point and the curve's trim merged away) and crashed; fork tangency-labs/smlib 80d307d
//! checks the curve against the face's trims first. The port holds the face's curves by
//! index (`built.curve_edges`), so a curve the face creator dropped is a missing edge,
//! never a stale read: the sweep returns an error or a sound body, and never panics.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::{Kernel, Point3, Vec3};
use std::f64::consts::PI;

/// The fork's capi case: the circle of radius 5 about (0, 0, 20) in the plane z = 20,
/// from (5, 0, 20), revolved a quarter turn about the line through (5, 0, 20) along Y
/// (the circle's seam vertex on the axis), capped.
fn revolve_touching_disc(k: &mut Kernel) -> solidcore::Result<solidcore::Brep> {
    let frame = Axis2Placement::new(Point3::new(0.0, 0.0, 20.0), Vec3::X, Vec3::Y);
    let c = Curve::from(
        Ellipse::create_canonical_circle(
            k,
            &frame,
            5.0,
            Some(&Extent1d::from_min_max(0.0, 360.0)),
            None,
        )
        .unwrap(),
    );
    create_rotational_sweep(
        k,
        &[Some(&c)],
        Point3::new(5.0, 0.0, 20.0),
        Vec3::Y,
        90.0,
        true,
    )
}

#[test]
fn issue_1511_disc_touching_the_axis_errs_or_builds_a_sound_body() {
    // a quarter of the horn torus: (pi / 2) * (pi 5^2) * 5 (Pappus)
    let want = PI / 2.0 * PI * 25.0 * 5.0;
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.revolve_analytic_profile_faces = fix;
        match revolve_touching_disc(&mut k) {
            Err(e) => eprintln!("fix {fix}: no body: {e}"),
            Ok(mut b) => {
                // a sound body: intact topology and the quarter horn torus's volume (to a
                // coarse integration tolerance, which keeps the test cheap)
                solidcore::topology::integrity::check(&b).unwrap();
                let store = AttributeStore::default();
                let attrs = PropertyAttributes {
                    store: &store,
                    key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
                };
                let got = brep_volume(&k, &mut b, 1e-3, &attrs).unwrap();
                eprintln!("fix {fix}: body, volume {got} (horn torus quarter {want})");
                assert!(
                    (got - want).abs() <= 1e-2 * want,
                    "fix {fix}: unsound body, volume {got}, want {want}"
                );
            }
        }
    }
}
