//! #1060 (WR-615, fork tangency-labs/smlib a96ce9f; SolidCraft rig 4492): mitred rounds
//! on a moved face follow it together. A 40 x 30 x 10 box has its four top edges rounded
//! r 3 (mitred where they meet at the top corners), and the top face is offset by +19
//! (`OffsetFacesExecutive::offset_faces`). The four rounds form one group that comes off
//! at once through `SmFeatureExecutive` (`SM_RB_EXTEND_SURFACE`), because every face
//! round it is a plane. The top then moves to z 29 and the rounds are put back.
//!
//! The fork (2fc375f) gives 10 faces, 20 edges and 12 vertices (its volume, 34539.95627294846,
//! is its marched mitres' and approximated sections', 5.9e-4 over the closed form). Before
//! the group removal was ported, the move was refused as `OffsetFacesRefusal::Unsupported`.
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

/// The rig: the rounded box with its top moved +19.
fn rounded_box_top_up(k: &mut Kernel) -> Brep {
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
    let r = ex.offset_faces(k, &mut b, &[t], 19.0);
    assert!(r.is_ok(), "{r:?}: {:?} {}", ex.refusal(), ex.reason());
    assert_eq!(ex.followed_blends(), 4);
    b
}

/// The rig's body checked (manifold, 10 faces, 20 edges, 12 vertices, the top at z 29);
/// its volume.
fn check(k: &mut Kernel) -> f64 {
    let mut b = rounded_box_top_up(k);
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), 10);
    assert_eq!(b.edges().ids().count(), 20);
    assert_eq!(b.vertices().ids().count(), 12);
    // the moved top: a plane at z 29
    let _ = top(&b, 29.0);
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, &mut b, 1e-9, &attrs).unwrap()
}

/// On the default kernel the rounds' sections are polynomial arcs within 0.05% of the radius
/// (WR-708), so the volume is not the closed form; the topology is.
#[test]
fn mitred_rounds_follow_the_moved_top() {
    check(&mut Kernel::default());
}

/// With every fix but `geometric_g2_continuity` the rounds' sections are the ball's exact
/// circles and the mitres the blend corner's exact plane sections: the body is the box
/// 40 x 30 x 29 less each top edge's spandrel `(1 - pi/4) r^2` along it and plus, at each
/// corner, what the two spandrel prisms share, `(5/3 - pi/2) r^3`, to 1e-9.
#[test]
fn mitred_rounds_follow_the_moved_top_with_every_fix() {
    let mut k = Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    };
    k.fixes.geometric_g2_continuity = false;
    let r = 3.0_f64;
    let pi = core::f64::consts::PI;
    let want = 40.0 * 30.0 * 29.0 - (1.0 - pi / 4.0) * r * r * 140.0
        + 4.0 * (5.0 / 3.0 - pi / 2.0) * r * r * r;
    let v = check(&mut k);
    assert!((v - want).abs() <= 1e-9 * want, "volume {v}, want {want}");
}
