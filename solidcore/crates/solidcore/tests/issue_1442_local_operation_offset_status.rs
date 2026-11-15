//! `SmBrep::LocalOperation` returns the extended offset's own error, not `SM_ERR` as
//! upstream does.
//!
//! A 40 x 40 x 10 plate with an r2 through hole at x = 5 has its side face x = 0 moved to
//! x = 3: the hole wall (x 3..7) is then tangent to the moved side at (3, 20), so the
//! rebuilt top face has loops that touch without crossing. With
//! `offset_resolve_crossing_loops` the extended offset
//! fails with `SM_ERR_OFFSET_WALLS_TOUCH` (1042), and `LocalOperation` passes that on.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, Notifications};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::error::{ErrorKind, SM_ERR_OFFSET_WALLS_TOUCH};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::Kernel;

fn move_side() -> solidcore::Result<()> {
    let mut k = Kernel::default();
    k.fixes.offset_resolve_crossing_loops = true;
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 10.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(5.0, 20.0, -1.0), 2.0, 12.0).unwrap();
    let mut part = boolean_difference(&mut k, plate, hole).unwrap();
    let side = planar_faces_at(&k, &part, 0, 0.0);
    assert_eq!(side.len(), 1);
    let moved = Surface::from(Plane::from_normal(Vec3::new(3.0, 0.0, 0.0), -Vec3::X));
    let mut notifications = Notifications::new(&mut part);
    local_operation(
        &mut k,
        &mut part,
        &side,
        vec![moved],
        &mut notifications.context(),
    )
}

#[test]
fn issue_1442_local_operation_passes_on_the_offset_status() {
    let r = move_side();
    match r {
        Err(e) => assert_eq!(e.sm_status(), Some(SM_ERR_OFFSET_WALLS_TOUCH)),
        other => panic!("{other:?}"),
    }
}

/// The local operation's error is the offset's refusal in solidcore's terms, with no
/// wrapping error around it.
#[test]
fn issue_2615_local_operation_names_the_walls_that_touch() {
    let e = move_side().unwrap_err();
    assert_eq!(e.kind(), ErrorKind::OffsetWallsTouch);
    assert_eq!(e.cause(), None);
    let message = e.to_string();
    assert!(
        message.starts_with(
            "offset::offset_executive::crossing_loops::resolve_crossing_loops: the loops of the \
             offset of face "
        ),
        "{message}"
    );
    assert!(
        message.ends_with(" run along one another, so the offset walls would meet there")
            || message.ends_with(" touch without crossing, so the offset walls would meet there"),
        "{message}"
    );
    assert!(
        !message.contains("1042") && !message.contains("status"),
        "{message}"
    );
}
