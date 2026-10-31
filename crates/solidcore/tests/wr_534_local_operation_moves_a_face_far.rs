//! Regression test for WR-534 (the fork's tangency-labs/smlib@a1ee273, the capi's
//! `smlib_offset_faces`): `SmBrep::LocalOperation` extends the neighbours of a moved
//! face by a distance taken from the original body's bounding sphere, so a face moved
//! further than about the body's size lies beyond them, `MateEdges` finds no
//! intersection and the result is open. A 20 mm cube has its top moved up by 60, 200
//! and 1000 (its own plane offset along its outward normal). With
//! `local_operation_box_holds_new_surfaces` the body's box takes in the new surface first
//! and the result is the closed 20 x 20 x (20 + d) box; by default it is open, of volume 0.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, volume, Notifications};
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::{Brep, Kernel};

/// The cube with its top moved up by `d`, with WR-534's `fixed`.
fn raised(fixed: bool, d: f64) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.local_operation_box_holds_new_surfaces = fixed;
    let mut part = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let top = planar_faces_at(&k, &part, 2, 20.0);
    assert_eq!(top.len(), 1, "the top");
    let face = part.face(top[0]).unwrap();
    let s = face.surface.as_ref().unwrap();
    let n = s
        .evaluate_normal(&k, face.uv_domain.mid(), Side::Left, Side::Left)
        .unwrap();
    let moved = s
        .create_offset_surface(&k, if n.z > 0.0 { d } else { -d }, 1e-6)
        .unwrap()
        .unwrap();
    let mut notifications = Notifications::new(&mut part);
    local_operation(
        &mut k,
        &mut part,
        &top,
        vec![moved],
        &mut notifications.context(),
    )
    .unwrap();
    (k, part)
}

#[test]
fn wr_534_far_moved_top_closes_with_the_fix() {
    for d in [60.0, 200.0, 1000.0] {
        let (k, mut b) = raised(true, d);
        assert!(brep_is_manifold_solid(&b).unwrap(), "d {d}");
        assert_eq!(b.face_list().len(), 6, "d {d}");
        let (v, exact) = (volume(&k, &mut b).unwrap(), 400.0 * (20.0 + d));
        assert!(
            ((v - exact) / exact).abs() < 1e-9,
            "d {d}: {v} against {exact}"
        );
    }
}

/// Upstream's neighbours do not reach the moved top: the result is open.
#[test]
fn wr_534_default_leaves_the_far_top_open() {
    for d in [60.0, 200.0, 1000.0] {
        let (k, mut b) = raised(false, d);
        assert!(!brep_is_manifold_solid(&b).unwrap(), "d {d}");
        assert!(volume(&k, &mut b).unwrap().abs() < 1e-9, "d {d}");
    }
}
