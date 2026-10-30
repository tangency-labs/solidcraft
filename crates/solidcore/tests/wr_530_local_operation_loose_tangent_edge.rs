//! Regression test for WR-530 (tangency-labs/solidcore#402; the fork's
//! tangency-labs/smlib@23c5b0a): a 60 x 40 x 10 plate with an r10 boss on it and an r2
//! round at the boss's base has its boss wall moved out by 1 (`SmBrep::LocalOperation`,
//! the capi's `smlib_offset_faces`). Only the wall gets a new surface, so the tangent
//! edge between it and the round is dropped across the 1 mm gap between them. Upstream
//! ignores that ("will be fixed later") and returns a body with an edge as loose as the
//! gap; with `local_operation_refuses_loose_tangent_edges` the local operation is
//! refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::error::{SolidError, SM_ERR};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_cylinder_surface};
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::FaceId;
use solidcore::{Brep, Kernel};

const AXIS: (f64, f64) = (30.0, 20.0);

fn axis_distance(p: Point3) -> f64 {
    (p.x - AXIS.0).hypot(p.y - AXIS.1)
}

/// The plate with its boss and the r2 round at the boss's base.
fn part(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let boss = create_cylinder(k, Point3::new(AXIS.0, AXIS.1, 5.0), 10.0, 15.0).unwrap();
    let mut b = boolean_union(k, plate, boss).unwrap();
    let base: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(k, e.interval.mid()).unwrap();
            (p.z - 10.0).abs() < 1e-9 && (axis_distance(p) - 10.0).abs() < 1e-9
        })
        .map(|(id, _)| id)
        .collect();
    assert!(!base.is_empty(), "the boss's base edge");
    fillet_edges(k, &mut b, &base, 2.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    b
}

/// The faces of the boss's wall: on the r10 circle about the boss's axis, above the round.
fn wall(k: &Kernel, b: &Brep) -> Vec<FaceId> {
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let p = s.evaluate_point(k, face.uv_domain.mid()).unwrap();
            p.z > 12.5 && (axis_distance(p) - 10.0).abs() < 1e-6
        })
        .collect()
}

fn move_wall(fixed: bool) -> solidcore::Result<()> {
    let mut k = Kernel::default();
    k.fixes.local_operation_refuses_loose_tangent_edges = fixed;
    let mut b = part(&mut k);
    let faces = wall(&k, &b);
    assert!(!faces.is_empty(), "the boss's wall");
    let surfaces = faces
        .iter()
        .map(|_| {
            Surface::from(
                create_cylinder_surface(&k, Point3::new(AXIS.0, AXIS.1, 5.0), 11.0, 15.0).unwrap(),
            )
        })
        .collect();
    let mut notifications = Notifications::new(&mut b);
    local_operation(
        &mut k,
        &mut b,
        &faces,
        surfaces,
        &mut notifications.context(),
    )
}

#[test]
fn wr_530_moved_boss_wall_beside_its_round_is_refused() {
    match move_wall(true) {
        Err(SolidError::Upstream { code, .. }) => assert_eq!(code, SM_ERR),
        other => panic!("{other:?}"),
    }
}

/// Without the opt-in the local operation returns a body, as upstream does.
#[test]
fn wr_530_without_the_opt_in_the_local_operation_succeeds() {
    move_wall(false).unwrap();
}
