//! tangency-labs/solidcore#423 (WR-459): `DoFilleting`'s `BooleanPostProcess(TRUE)` checks
//! every edge of the target, not only the fillet's, so it also merges the target's own
//! faces that lie on coincident surfaces (the five patches of a sphere imported in six
//! faces became one face no patch's parameterization could carry). The opt-in
//! `Kernel::fixes.fillet_cleanup_keeps_target_faces` removes no edge the target had and
//! none between two of its faces, as the fork does (tangency-labs/smlib@9ddf4a8).
//!
//! The sphere's own failure (loops of the wrong orientation, rails off their surfaces)
//! needs analytic patches in six faces and is not built here. The body here: a cylinder
//! united with a disc extruded from two half arcs leaves the disc's two wall halves merged
//! into one face with the edge between them inside it, a slit (one face on both sides), as
//! SMLib's union does. The union's own clean-up removes that slit now (WR-304), so the
//! slit body is the union without its clean-up, then SMLib's clean-up of it by hand (each
//! rim removed as `boolean_post_process` removes it; the rims' end vertices, which the
//! clean-up would delete, stay). A round on the disc's top rim removes the slit by
//! default. With the keep it is an edge of the target, but a slit, which the clean-up
//! checks all the same (WR-602; the fork's tangency-labs/smlib@cac11ea), so it goes too.
//! Faces, one-face edges and volume are the same either way.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_with_options;
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

/// The union with SMLib's clean-up (its intersection edges only), which leaves the slit.
fn slit_union(k: &mut Kernel, flange: Brep, disc: Brep) -> Brep {
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

/// The edges with one face on both sides on the wall's far side, x = -R, where the line
/// between the disc's wall halves runs (the wall's seam is at x = R).
fn far_slits(b: &Brep) -> usize {
    p1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            edge::faces(b, e, None).unwrap().len() == 1
                && [s, t]
                    .iter()
                    .all(|v| vertex::point(b, v.unwrap()).unwrap().x < -R + 1e-6)
        })
        .count()
}

/// The disc's top rim (its two half arcs at z = H) rounded r 1 on the union with the slit,
/// with the keep as `keep` says: the target's slits at x = -R, then the result's faces,
/// its edges with one face on both sides (seams and slits), its slits at x = -R and its
/// volume.
fn rounded(keep: bool) -> (usize, usize, usize, usize, f64) {
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
    let mut disc = slit_union(&mut k, flange, disc);
    let target_slits = far_slits(&disc);
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
    let slits = far_slits(&disc);
    let faces = p1::faces(&disc, None).unwrap().len();
    (target_slits, faces, seams, slits, volume(&k, &mut disc))
}

#[test]
fn issue_423_keep_still_removes_the_targets_own_slit() {
    let (target, faces, seams, slits, _) = rounded(false);
    let (kept_target, kept_faces, kept_seams, kept_slits, _) = rounded(true);
    assert_eq!((target, kept_target), (1, 1), "the target has the slit");
    assert_eq!(kept_faces, faces);
    assert_eq!(
        kept_seams, seams,
        "kept {kept_seams} one-face edges, default {seams}"
    );
    assert_eq!(
        (kept_slits, slits),
        (0, 0),
        "the round's clean-up removes the slit"
    );
}

#[test]
fn issue_423_kept_faces_same_volume() {
    let (_, _, _, _, merged) = rounded(false);
    let (_, _, _, _, kept) = rounded(true);
    assert!((merged - kept).abs() <= 1e-6 * merged, "{merged} vs {kept}");
}
