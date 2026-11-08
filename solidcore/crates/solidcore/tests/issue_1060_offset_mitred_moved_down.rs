//! #1060 (WR-615, fork tangency-labs/smlib a96ce9f; SolidCraft rig 4492), the rig of
//! `issue_1060_offset_mitred_rounded_box_top` moved the other way: the 40 x 30 x 10 box
//! with its four top edges rounded r 3 (mitred at the top corners) has its top face offset
//! by -6, into the body, to z 4 (the rounds still fit). The four rounds come off together
//! through `SmFeatureExecutive` (`SM_RB_EXTEND_SURFACE`) and go back on the lowered top.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::offset::offset_faces::OffsetFacesExecutive;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId};
use solidcore::Kernel;

/// The edge whose midpoint is `p`.
fn edge_through(b: &Brep, p: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let a = vertex::point(b, s.unwrap()).unwrap();
            let c = vertex::point(b, t.unwrap()).unwrap();
            (0.5 * (a.x + c.x) - p.x).abs() < 1e-6
                && (0.5 * (a.y + c.y) - p.y).abs() < 1e-6
                && (0.5 * (a.z + c.z) - p.z).abs() < 1e-6
        })
        .unwrap()
}

/// The plane face whose vertices are all at height `z`.
fn top(b: &Brep, z: f64) -> FaceId {
    *b.face_list()
        .iter()
        .find(|&&f| {
            face2::surface(b, f)
                .unwrap()
                .unwrap()
                .is_kind_of(Plane::TYPE)
                .unwrap()
                && solidcore::topology::face::p4::edges(b, f, None, false, None)
                    .unwrap()
                    .into_iter()
                    .all(|e| {
                        let (s, _) = edge::start_end_vertices(b, e).unwrap();
                        (vertex::point(b, s.unwrap()).unwrap().z - z).abs() < 1e-6
                    })
        })
        .unwrap()
}

/// The rounded box with its top moved by `distance`.
fn rounded_box_top_moved(k: &mut Kernel, distance: f64) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let edges: Vec<EdgeId> = [
        Point3::new(20.0, 0.0, 10.0),
        Point3::new(20.0, 30.0, 10.0),
        Point3::new(0.0, 15.0, 10.0),
        Point3::new(40.0, 15.0, 10.0),
    ]
    .into_iter()
    .map(|p| edge_through(&b, p))
    .collect();
    fillet_edges(k, &mut b, &edges, 3.0, 1, 1, 1.0).unwrap();
    let t = top(&b, 10.0);
    let mut ex = OffsetFacesExecutive::new();
    let r = ex.offset_faces(k, &mut b, &[t], distance);
    assert!(r.is_ok(), "{r:?}: {:?} {}", ex.refusal(), ex.reason());
    assert_eq!(ex.followed_blends(), 4);
    b
}

/// The volume is the box's, 1200 (10 - 6), less each straight round's corner section
/// A3 = 9 (1 - pi/4) along its 140 mm of edges, plus 4 (2 A3 3 - 9) back at the mitred
/// corners, to 1e-7: the mitres are not exact in this closed form (at +19 it is 1.7e-8
/// off the fork's volume), so `issue_1060_offset_mitred_rounded_box_top` pins the fork's
/// digits at +19.
#[test]
fn mitred_rounds_follow_the_top_moved_down() {
    let mut k = Kernel::default();
    let mut b = rounded_box_top_moved(&mut k, -6.0);
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), 10);
    assert_eq!(b.edges().ids().count(), 20);
    assert_eq!(b.vertices().ids().count(), 12);
    // the moved top: a plane at z 4
    let _ = top(&b, 4.0);
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-9, &attrs).unwrap();
    let a3 = 9.0 * (1.0 - std::f64::consts::PI / 4.0);
    let want = 1200.0 * 4.0 - 140.0 * a3 + 4.0 * (2.0 * a3 * 3.0 - 9.0);
    assert!(
        (v - want).abs() <= 1e-7 * want,
        "volume {v}, closed form {want}"
    );
}
