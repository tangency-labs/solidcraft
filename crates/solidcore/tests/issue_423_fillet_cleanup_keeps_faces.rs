//! tangency-labs/solidcore#423 (WR-459): `DoFilleting`'s `BooleanPostProcess(TRUE)` checks
//! every edge of the target, not only the fillet's, so it also merges the target's own
//! faces that lie on coincident surfaces (the five patches of a sphere imported in six
//! faces became one face no patch's parameterization could carry). The opt-in
//! `Kernel::fixes.fillet_cleanup_keeps_target_faces` removes no edge the target had and
//! none between two of its faces, as the fork does (tangency-labs/smlib@9ddf4a8).
//!
//! The sphere's own failure (loops of the wrong orientation, rails off their surfaces)
//! needs analytic patches in six faces and is not built here. This guards the mechanism:
//! a cylinder united with a disc extruded from two half arcs leaves the disc's two wall
//! halves, coincident, with an edge between them; a round on the disc's top rim removes
//! that edge by default and keeps it with the flag. Faces and volume are the same.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
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
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel};

const R: f64 = 30.0;
const H: f64 = 5.0;

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

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The disc's top rim (its two half arcs at z = H) rounded r 1: the faces, the edges with
/// one face on both sides (seams) and the volume.
fn rounded(keep: bool) -> (usize, usize, f64) {
    let mut k = Kernel::default();
    k.fixes.fillet_cleanup_keeps_target_faces = keep;
    let a = half_arc(&k, 0.0, 180.0);
    let b = half_arc(&k, 180.0, 360.0);
    let flange = create_cylinder(&mut k, Point3::new(0.0, 0.0, -6.0), R, 6.0).unwrap();
    let disc = create_linear_sweep(
        &mut k,
        &[Some(&a), Some(&b)],
        Vec3::new(0.0, 0.0, 1.0),
        H,
        true,
    )
    .unwrap();
    let mut disc = boolean_union(&mut k, flange, disc).unwrap();
    let rim: Vec<_> = p1::edges(&disc, None)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let a = edge::vertex(&disc, e).unwrap().unwrap();
            let b = edge::other_vertex(&disc, e, Some(a)).unwrap().unwrap();
            [a, b]
                .iter()
                .all(|&v| (vertex::point(&disc, v).unwrap().z - H).abs() < 1e-9)
        })
        .collect();
    assert_eq!(rim.len(), 2);
    fillet_edges(&mut k, &mut disc, &rim, 1.0, 1, 1, 0.5).unwrap();
    assert!(is_manifold_solid(&disc).unwrap());
    let seams = p1::edges(&disc, None)
        .unwrap()
        .into_iter()
        .filter(|&e| edge::faces(&disc, e, None).unwrap().len() == 1)
        .count();
    let faces = p1::faces(&disc, None).unwrap().len();
    (faces, seams, volume(&k, &mut disc))
}

#[test]
fn issue_423_default_removes_the_targets_own_edge() {
    let (faces, seams, _) = rounded(false);
    let (kept_faces, kept_seams, _) = rounded(true);
    assert_eq!(kept_faces, faces);
    assert!(
        kept_seams > seams,
        "kept {kept_seams} one-face edges, default {seams}"
    );
}

#[test]
fn issue_423_kept_faces_same_volume() {
    let (_, _, merged) = rounded(false);
    let (_, _, kept) = rounded(true);
    assert!((merged - kept).abs() <= 1e-6 * merged, "{merged} vs {kept}");
}
