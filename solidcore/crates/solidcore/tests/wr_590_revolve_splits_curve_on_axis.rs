//! Regression test for WR-590 (the fork's tangency-labs/smlib@e4f1377, capi test
//! `sweeps`, "revolve horn disc"): the whole disc of radius 5 centred 5 from the Z axis,
//! in the XZ plane, touching the axis at the origin, turned a quarter, half and three
//! quarters of a turn and capped (a horn torus part). Its circle starts on the far side
//! (10, 0, 0), up at (5, 0, 5), or at the origin. Upstream's
//! `SmRotationalSweepGeometry::CheckCurveSweep` refuses a curve that meets the axis
//! anywhere but at its ends (`SM_SC_ROT_ON_CURVE`), so the first two starts fail with
//! `SM_ERR`. The sweep splits the circle at the origin first (and joins its last piece back onto its first), and every start gives
//! one turned face and two end discs of volume turn * 2 pi 5 * pi 25 (Pappus).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::{Kernel, Point3, Vec3};
use std::f64::consts::PI;

/// The circle's reference directions: it starts at (10, 0, 0), (5, 0, 5) and the origin.
const REFS: [Vec3; 3] = [
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
    Vec3::new(-1.0, 0.0, 0.0),
];
const TURNS: [f64; 3] = [0.25, 0.5, 0.75];

/// The disc's circle: centre (5, 0, 0), normal -Y, starting along `reference`.
fn disc(k: &Kernel, reference: Vec3) -> Curve {
    let normal = Vec3::new(0.0, -1.0, 0.0);
    let frame = Axis2Placement::new(
        Point3::new(5.0, 0.0, 0.0),
        reference,
        normal.cross(reference),
    );
    Curve::from(Ellipse::create_canonical_circle(k, &frame, 5.0, None, None).unwrap())
}

/// The capped turn of the disc about Z by `turn` of a whole turn.
fn revolve(reference: Vec3, turn: f64) -> (Kernel, solidcore::Result<solidcore::Brep>) {
    let mut k = Kernel::default();
    let c = disc(&k, reference);
    let r = create_rotational_sweep(
        &mut k,
        &[Some(&c)],
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        360.0 * turn,
        true,
    );
    (k, r)
}

fn volume(k: &Kernel, b: &mut solidcore::Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-3, &attrs).unwrap()
}

/// At the half turn (the integration is slow on the horn torus's apex): the Pappus
/// volume, to the integration tolerance.
fn check_volume(k: &Kernel, b: &mut solidcore::Brep, reference: Vec3, turn: f64) {
    if (turn - 0.5).abs() > 1e-12 {
        return;
    }
    let want = turn * 2.0 * PI * 5.0 * PI * 25.0;
    let got = volume(k, b);
    assert!(
        (got - want).abs() <= 1e-3 * want,
        "ref {reference:?} turn {turn}: volume {got}, want {want}"
    );
}

#[test]
fn wr_590_on_every_start_turns_into_three_faces_of_the_pappus_volume() {
    for reference in REFS {
        for turn in TURNS {
            let (k, r) = revolve(reference, turn);
            let mut b = r.unwrap_or_else(|e| panic!("ref {reference:?} turn {turn}: {e}"));
            solidcore::topology::integrity::check(&b).unwrap();
            assert_eq!(b.face_list().len(), 3, "ref {reference:?} turn {turn}");
            assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
            check_volume(&k, &mut b, reference, turn);
        }
    }
}
