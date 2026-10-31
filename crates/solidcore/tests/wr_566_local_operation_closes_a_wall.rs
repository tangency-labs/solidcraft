//! Regression test for WR-566 (the fork's tangency-labs/smlib ccbf77a): a stepped block
//! (40 x 30 x 20 with the part x > 20, z > 10 cut away) has its low top moved up 10 onto
//! the high top's plane, or its high top moved down 10 onto the low top's plane
//! (SolidCraft's replace face, rig case shell/cov-replace-face-step-onto-the-higher-face).
//! The move closes the step's wall to nothing: its edges along the sides close to points.
//! With `local_operation_step_walls` alone `CloseLoopGap` refuses those edges
//! (`SM_ERR_OFFSET_EDGE_VANISHES`); with `local_operation_closes_walls_to_nothing` they
//! are squeezed out, the wall is deleted, and the result is the plain 40 x 30 x 20 box
//! (24000) or 40 x 30 x 10 box (12000).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, report, Notifications};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::error::{SolidError, SM_ERR_OFFSET_EDGE_VANISHES};
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::FaceId;
use solidcore::{Brep, Kernel};

fn kernel(fixed: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.local_operation_step_walls = true;
    k.fixes.local_operation_closes_walls_to_nothing = fixed;
    // the executive's own status, not SM_ERR
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

/// The horizontal plane faces at height `z` (`planar_faces_at` also finds the side
/// faces whose middle lies at that height).
fn tops_at(k: &Kernel, b: &Brep, z: f64) -> Vec<FaceId> {
    planar_faces_at(k, b, 2, z)
        .into_iter()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let n = s
                .evaluate_normal(k, face.uv_domain.mid(), Side::Left, Side::Left)
                .unwrap();
            n.z.abs() > 0.99
        })
        .collect()
}

/// The top at height `z` moved to height `to` (`SmBrep::LocalOperation`): its own plane
/// offset (`CreateOffsetSurface`, as the fork's `smlib_offset_faces` moves a face), which
/// keeps the face's parameterization; the local operation lifts the face's edges onto the
/// new surface by their UV.
fn move_top(fixed: bool, z: f64, to: f64) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = kernel(fixed);
    let mut b = stepped(&mut k);
    let tops = tops_at(&k, &b, z);
    assert_eq!(tops.len(), 1, "the top at z = {z}");
    let surface = {
        let face = b.face(tops[0]).unwrap();
        let s = face.surface.as_ref().unwrap();
        let n = s
            .evaluate_normal(&k, face.uv_domain.mid(), Side::Left, Side::Left)
            .unwrap();
        // along the plane's own normal, whichever way it faces
        let d = if n.z > 0.0 { to - z } else { z - to };
        s.create_offset_surface(&k, d, 1e-6).unwrap().unwrap()
    };
    let mut notifications = Notifications::new(&mut b);
    let r = local_operation(
        &mut k,
        &mut b,
        &tops,
        vec![surface],
        &mut notifications.context(),
    );
    (
        k,
        r.map(|()| {
            b.refresh_cached_topology();
            b
        }),
    )
}

#[test]
fn wr_566_low_top_up_onto_the_high_top_closes_the_wall() {
    let (k, result) = move_top(true, 10.0, 20.0);
    assert!(report(
        &k,
        "wr_566 low top up 10",
        result,
        Some(24000.0),
        1e-9
    ));
}

#[test]
fn wr_566_high_top_down_onto_the_low_top_closes_the_wall() {
    let (k, result) = move_top(true, 20.0, 10.0);
    assert!(report(
        &k,
        "wr_566 high top down 10",
        result,
        Some(12000.0),
        1e-9
    ));
}

/// Without the opt-in the closed edges are `SM_ERR_OFFSET_EDGE_VANISHES`, as before.
#[test]
fn wr_566_without_the_opt_in_the_closed_wall_is_refused() {
    for (z, to) in [(10.0, 20.0), (20.0, 10.0)] {
        match move_top(false, z, to).1 {
            Err(e @ SolidError::Upstream { code, .. }) => {
                assert_eq!(code, SM_ERR_OFFSET_EDGE_VANISHES, "z = {z}");
                assert!(e.is_offset_edge_vanishes(), "z = {z}");
            }
            other => panic!("z = {z}: {other:?}"),
        }
    }
}
