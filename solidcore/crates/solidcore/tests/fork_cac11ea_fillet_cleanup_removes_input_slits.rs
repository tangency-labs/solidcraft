//! WR-602 (the SolidCraft fork's tangency-labs/smlib@cac11ea): with the fork's keep
//! (WR-459, the default since its flag was retired, #17169), `DoFilleting`'s
//! `BooleanPostProcess` leaves every edge of the fillet's input, and every edge between two
//! of its faces, alone. Where a round takes away the face on one side of a target edge,
//! that edge is left with one face on both sides: a slit, kept as an input edge. The
//! clean-up checks such an edge all the same (removing it merges no faces), and the
//! kept-faces test needs two faces; this was opt-in as
//! `Kernel::fixes.fillet_cleanup_removes_input_slits` until that flag was retired.
//!
//! The slit here is the one of #1120 (WR-304): a disc extruded from two half arcs joined
//! to a cylinder of the same radius leaves the line between the halves' walls inside the
//! merged wall, as SMLib's union does. The union's own clean-up removes such a slit now
//! (WR-304), so the body is the union without its clean-up, then SMLib's clean-up of it
//! (the intersection edges only), which leaves the slit. The clean-up then runs as
//! after a fillet whose target is that body: every edge and face is kept.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::p1::{boolean_post_process, CleanupRelations};
use solidcore::boolean::merge::MergeOptions;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p4::{delete_edge, DeleteEnv};
use solidcore::topology::face::p2::set_uv_domain;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, EntityRef, Kernel};

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
/// with the slit between the halves left, as SMLib's union leaves it.
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
    let options = MergeOptions::new(false, false, false, false, false);
    let mut b = boolean_with_options(k, flange, disc, BooleanOperation::Union, options)
        .unwrap()
        .result;
    // SMLib's clean-up of the union removes only the intersection edges (the rims at z = 0
    // between the disc's two wall halves and the flange's wall); the line between the
    // halves is left with the merged wall on both sides. (Their end vertices stay.)
    let at_zero = |b: &Brep, e: EdgeId| {
        let (s, t) = edge::start_end_vertices(b, e).unwrap();
        [s, t]
            .iter()
            .all(|v| vertex::point(b, v.unwrap()).unwrap().z.abs() < 1e-9)
    };
    let rims: Vec<EdgeId> = p1::edges(&b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| at_zero(&b, e))
        .collect();
    assert_eq!(rims.len(), 2, "the two rim arcs at z = 0");
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = DeleteEnv {
        records: &mut records,
        attributes: &mut attributes,
        key_of: &key,
        context: None,
    };
    // each rim as `boolean_post_process` removes it (`IsTopological`, then `DeleteEdge`
    // keeping that surface, then the merged face's domain)
    b.set_editing_enabled(true);
    for e in rims {
        let topological = edge::is_topological_without_growth(k, &mut b, e).unwrap();
        assert!(topological.is_topological, "the rim is topological");
        let keep = topological.surf_to_keep.unwrap();
        let faces = edge::faces(&b, e, None).unwrap();
        let domain = b
            .face(keep)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .natural_uv_domain_with_nurb(k)
            .unwrap();
        set_uv_domain(&mut b, keep, &domain).unwrap();
        delete_edge(k, &mut b, &mut env, e, Some(keep)).unwrap();
        let keep = if b.face(keep).is_ok() {
            keep
        } else {
            *faces.iter().find(|&&f| f != keep).unwrap()
        };
        set_uv_domain(
            &mut b,
            keep,
            &topological.merged_face_domain.unwrap_or_default(),
        )
        .unwrap();
    }
    b.set_editing_enabled(false);
    b
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
fn cleaned() -> ((usize, usize), (usize, usize), f64) {
    let mut k = Kernel::default();
    let mut b = joined(&mut k);
    let before = (one_face_edges(&b).len(), p1::edges(&b, None).unwrap().len());
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
fn wr602_input_slit_removed_seam_kept() {
    let (before, after, v) = cleaned();
    // The merged wall has its seam and the slit.
    assert!(before.0 >= 2, "{before:?}");
    assert_eq!(after.0, before.0 - 1, "the slit goes, the seam stays");
    assert_eq!(after.1, before.1 - 1, "no other edge goes");
    let want = core::f64::consts::PI * R * R * 11.0;
    assert!((v - want).abs() <= 1e-6 * want, "volume {v}, want {want}");
}
