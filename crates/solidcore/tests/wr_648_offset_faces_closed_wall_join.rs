//! Regression test for WR-648 (the fork's tangency-labs/smlib 9504880, following up
//! WR-566's ccbf77a): a stepped block (40 x 30 x 20 with the part x > 20, z > 10 cut
//! away) has its low top moved up 10 onto the high top's plane, or its high top moved
//! down 10 onto the low top's plane, through the face move (`SmOffsetFacesExecutive`).
//! The move closes the step's wall to nothing and the wall goes (WR-566); the faces
//! either side of it, a moved face and one that stayed, then meet on one plane. With
//! `local_operation_closes_walls_to_nothing` the edge between them goes as well: the
//! result is the plain 40 x 30 x 20 box (24000) or 40 x 30 x 10 box (12000), 6 faces
//! (7 before 9504880).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::offset::offset_faces::OffsetFacesExecutive;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::{Brep, FaceId};
use solidcore::Kernel;

fn kernel(fixed: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.local_operation_step_walls = true;
    k.fixes.local_operation_closes_walls_to_nothing = fixed;
    k.fixes.local_operation_offset_status = true;
    k
}

fn stepped(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let cut = create_box(k, Point3::new(20.0, -1.0, 10.0), 21.0, 32.0, 11.0).unwrap();
    let mut b = boolean_difference(k, block, cut).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The upward-facing plane face at height `z`.
fn top_at(k: &Kernel, b: &Brep, z: f64) -> FaceId {
    let tops: Vec<FaceId> = b
        .faces()
        .ids()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let mid = face.uv_domain.mid();
            let p = s.evaluate_point(k, mid).unwrap();
            let n = s.evaluate_normal(k, mid, Side::Left, Side::Left).unwrap();
            n.z.abs() > 0.99 && (p.z - z).abs() < 1e-9
        })
        .collect();
    assert_eq!(tops.len(), 1, "the top at z = {z}");
    tops[0]
}

/// The top at `z` moved by `d` along its outward normal; the face count and volume.
fn move_top(fixed: bool, z: f64, d: f64) -> (usize, f64) {
    let mut k = kernel(fixed);
    let mut b = stepped(&mut k);
    let top = top_at(&k, &b, z);
    let mut ex = OffsetFacesExecutive::new();
    ex.offset_faces(&mut k, &mut b, &[top], d)
        .unwrap_or_else(|e| panic!("z = {z}: {e:?} ({})", ex.reason()));
    assert!(brep_is_manifold_solid(&b).unwrap(), "z = {z}: a solid");
    (b.faces().ids().count(), volume(&k, &mut b))
}

fn near(got: f64, want: f64) -> bool {
    (got - want).abs() <= 1e-9 * want.abs()
}

#[test]
fn wr_648_low_top_up_onto_the_high_top_is_one_face() {
    let (faces, v) = move_top(true, 10.0, 10.0);
    assert!(near(v, 24000.0), "volume {v}");
    assert_eq!(faces, 6);
}

#[test]
fn wr_648_high_top_down_onto_the_low_top_is_one_face() {
    let (faces, v) = move_top(true, 20.0, -10.0);
    assert!(near(v, 12000.0), "volume {v}");
    assert_eq!(faces, 6);
}

/// Without the opt-in the closed wall is refused (`SM_ERR_OFFSET_EDGE_VANISHES`, WR-566),
/// as before.
#[test]
fn wr_648_without_the_opt_in_the_closed_wall_is_refused() {
    for (z, d) in [(10.0, 10.0), (20.0, -10.0)] {
        let mut k = kernel(false);
        let mut b = stepped(&mut k);
        let top = top_at(&k, &b, z);
        match OffsetFacesExecutive::new().offset_faces(&mut k, &mut b, &[top], d) {
            Err(SolidError::Upstream { code, .. }) => assert_eq!(code, 1043, "z = {z}"),
            other => panic!("z = {z}: {other:?}"),
        }
    }
}
