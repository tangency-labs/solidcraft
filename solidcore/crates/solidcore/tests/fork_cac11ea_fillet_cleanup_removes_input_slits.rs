//! WR-602 (the SolidCraft fork's tangency-labs/smlib@cac11ea): with the fork's keep
//! (`Kernel::fixes.fillet_cleanup_keeps_target_faces`), `DoFilleting`'s
//! `BooleanPostProcess` leaves every edge of the fillet's input, and every edge between two
//! of its faces, alone. Where a round takes away the face on one side of a target edge,
//! that edge is left with one face on both sides: a slit, kept as an input edge.
//! `Kernel::fixes.fillet_cleanup_removes_input_slits` checks such an edge all the same
//! (removing it merges no faces), and the kept-faces test needs two faces.
//!
//! The slit here is the one of #1120 (WR-304): a disc extruded from two half arcs joined
//! to a cylinder of the same radius leaves the line between the halves' walls inside the
//! merged wall. The clean-up then runs as after a fillet whose target is that body: every
//! edge and face is kept.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::boolean::merge::p1::{boolean_post_process, CleanupRelations};
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
use solidcore::topology::{edge, EdgeId};
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

/// A cylinder r 30, z -6..0, joined to a disc r 30, z 0..5 extruded from two half arcs,
/// with the slit between the halves left (`boolean_cleanup_removes_slits` off).
fn joined(k: &mut Kernel) -> Brep {
    let flange = create_cylinder(k, Point3::new(0.0, 0.0, -6.0), R, 6.0).unwrap();
    let a = half_arc(k, 0.0, 180.0);
    let b = half_arc(k, 180.0, 360.0);
    let disc = create_linear_sweep(
        k,
        &[Some(&a), Some(&b)],
        Vec3::new(0.0, 0.0, 1.0),
        5.0,
        true,
    )
    .unwrap();
    boolean_union(k, flange, disc).unwrap()
}

/// The edges with the same face on both sides (seams and slits).
fn one_face_edges(b: &Brep) -> Vec<EdgeId> {
    p1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| edge::faces(b, e, None).unwrap().len() == 1)
        .collect()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The joined body before and after a clean-up that keeps all of it, as `(one-face edges,
/// edges)` each, and the volume after.
fn cleaned(remove_input_slits: bool) -> ((usize, usize), (usize, usize), f64) {
    let mut k = Kernel::default();
    let mut b = joined(&mut k);
    let before = (one_face_edges(&b).len(), p1::edges(&b, None).unwrap().len());
    k.fixes.fillet_cleanup_keeps_target_faces = true;
    k.fixes.fillet_cleanup_removes_input_slits = remove_input_slits;
    let relations = CleanupRelations {
        kept_edges: p1::edges(&b, None).unwrap().into_iter().collect(),
        kept_faces: p1::faces(&b, None).unwrap().into_iter().collect(),
        ..CleanupRelations::default()
    };
    // `DoFilleting` edits the body it cleans up.
    b.set_editing_enabled(true);
    boolean_post_process(&mut k, &mut b, &relations, true).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    let after = (one_face_edges(&b).len(), p1::edges(&b, None).unwrap().len());
    let v = volume(&k, &mut b);
    (before, after, v)
}

#[test]
fn wr602_default_keeps_the_input_slit() {
    let (before, after, _) = cleaned(false);
    // The merged wall has its seam and the slit.
    assert!(before.0 >= 2, "{before:?}");
    assert_eq!(after, before, "upstream keeps every input edge");
}

#[test]
fn wr602_input_slit_removed_seam_kept() {
    let (before, after, v) = cleaned(true);
    assert_eq!(after.0, before.0 - 1, "the slit goes, the seam stays");
    assert_eq!(after.1, before.1 - 1, "no other edge goes");
    let want = core::f64::consts::PI * R * R * 11.0;
    assert!((v - want).abs() <= 1e-6 * want, "volume {v}, want {want}");
}
