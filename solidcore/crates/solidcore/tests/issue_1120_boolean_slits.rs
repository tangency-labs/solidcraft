//! tangency-labs/solidcore#1120 (WR-304): `SmMerge::BooleanPostProcess` checks only the
//! intersection edges. Removing one merges its two faces; an operand edge between two
//! faces that this way become one then has the merged face on both sides: a slit. Here a
//! disc extruded from two half arcs stands on a cylinder of the same radius: the union
//! merges both wall halves into the cylinder's wall, and the line between the halves at
//! 180 degrees stays as a slit. The opt-in `Kernel::fixes.boolean_cleanup_removes_slits`
//! also checks the merged face's edges with one face, by the same `IsTopological` test
//! (which keeps seams).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::{Brep, Kernel};

const R: f64 = 30.0;

/// The r 30 circle about the Z axis in z = 0 over `[start, end]` degrees.
fn half_arc(k: &Kernel, start: f64, end: f64) -> Curve {
    let mut frame = Axis2Placement::default();
    frame
        .set_canonical(
            Point3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
    let interval = Extent1d::from_min_max(start, end);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, R, Some(&interval), None).unwrap())
}

/// A cylinder r 30, z -6..0, united with a disc r 30, z 0..5, extruded from two half arcs.
fn joined(remove_slits: bool) -> Brep {
    let mut k = Kernel::default();
    k.fixes.boolean_cleanup_removes_slits = remove_slits;
    let flange = create_cylinder(&mut k, Point3::new(0.0, 0.0, -6.0), R, 6.0).unwrap();
    let a = half_arc(&k, 0.0, 180.0);
    let b = half_arc(&k, 180.0, 360.0);
    let disc = create_linear_sweep(
        &mut k,
        &[Some(&a), Some(&b)],
        Vec3::new(0.0, 0.0, 1.0),
        5.0,
        true,
    )
    .unwrap();
    let mut joined = boolean_union(&mut k, flange, disc).unwrap();
    assert!(is_manifold_solid(&joined).unwrap());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(&k, &mut joined, 1e-6, &attrs).unwrap();
    let want = core::f64::consts::PI * R * R * 11.0;
    assert!(
        (volume - want).abs() <= 1e-6 * want,
        "volume {volume}, want {want}"
    );
    joined
}

/// The edges with the same face on both sides (seams and slits).
fn one_face_edges(b: &Brep) -> usize {
    p1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| edge::faces(b, e, None).unwrap().len() == 1)
        .count()
}

#[test]
fn issue_1120_slit_left_by_default() {
    let upstream = one_face_edges(&joined(false));
    let fixed = one_face_edges(&joined(true));
    assert!(
        fixed < upstream,
        "upstream keeps the slit: {upstream} one-face edges, fixed {fixed}"
    );
}

#[test]
fn issue_1120_slit_removed_seams_kept() {
    // The merged wall keeps one seam, so at least one one-face edge is left.
    let fixed = one_face_edges(&joined(true));
    assert!(fixed >= 1, "{fixed} one-face edges");
}
