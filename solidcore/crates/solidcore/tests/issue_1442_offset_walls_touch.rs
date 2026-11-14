//! tangency-labs/solidcore#1442 (fork tangency-labs/smlib a759622): an offset whose walls
//! would meet is refused as such. SolidCraft rig case shell/4711: a 60 x 40 x 10 block
//! less an r3 through hole at (22.5, 7) and a 10 x 10 notch (x 25..35, through the
//! front), shelled inward open at the bottom face (z = 0).
//!
//! The hole lies 4 = 2 x 2 from the front face, so shelled 2 the hole wall's offset
//! (r 5) is tangent to the front wall's offset (y = 2) at (22.5, 2): the offset face on
//! z = 0 has loops that touch without crossing, the walls meet along a line and the
//! offset bounds no solid there. With `offset_resolve_crossing_loops` the shell fails with
//! `SM_ERR_OFFSET_WALLS_TOUCH` (1042), not `SM_ERR_BAD_INTERSECTIONS` (1027) as the fork
//! did before a759622. Thicknesses 1.5 and 2.5 (loops apart or crossing) build.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, volume, Notifications};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::error::offset::{OffsetFailure, WallContact};
use solidcore::error::{EntityRef, ErrorKind, Reason, SM_ERR_OFFSET_WALLS_TOUCH};
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::{Brep, Kernel};

fn part(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let pin = create_cylinder(k, Point3::new(22.5, 7.0, -5.0), 3.0, 20.0).unwrap();
    let notch = create_box(k, Point3::new(25.0, -5.0, -5.0), 10.0, 10.0, 20.0).unwrap();
    let holed = boolean_difference(k, block, pin).unwrap();
    boolean_difference(k, holed, notch).unwrap()
}

fn shell(distance: f64) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.offset_resolve_crossing_loops = true;
    let mut part = part(&mut k);
    let bottom = planar_faces_at(&k, &part, 2, 0.0);
    assert_eq!(bottom.len(), 1);
    let mut notifications = Notifications::new(&mut part);
    let result = OffsetExecutive::shell_brep(
        &mut k,
        part,
        ShellOptions {
            offset: OffsetOptions {
                distance,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: true,
        },
        &bottom,
        &mut notifications.context(),
    )
    .result;
    (k, result)
}

fn code(result: &solidcore::Result<Brep>) -> Option<i32> {
    match result {
        Err(e) => e.sm_status(),
        _ => None,
    }
}

#[test]
fn issue_1442_walls_touch_refused_with_its_own_status() {
    let (_, result) = shell(-2.0);
    assert_eq!(code(&result), Some(SM_ERR_OFFSET_WALLS_TOUCH), "{result:?}");
    // #2615: the refusal in solidcore's terms, naming the face of the input it is about
    // (before, the configuration's text stood in `operation` and the face was not named)
    let err = result.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OffsetWallsTouch);
    let Some(Reason::Offset(OffsetFailure::WallsTouch {
        face: Some(face),
        contact: WallContact::Touch,
    })) = err.failure().map(|f| f.reason.clone())
    else {
        panic!("{err:?}");
    };
    assert_eq!(err.entity(), Some(EntityRef::Face(face)));
    assert_eq!(
        err.operation(),
        "offset::offset_executive::crossing_loops::resolve_crossing_loops"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "offset::offset_executive::crossing_loops::resolve_crossing_loops: the loops of the \
             offset of face {} touch without crossing, so the offset walls would meet there",
            face.index()
        )
    );
}

/// The fork's C++ (a759622's commit message: 4711 at 1.5 and 2.5).
#[test]
fn issue_1442_thinner_and_thicker_walls_still_build() {
    for (distance, want) in [(-1.5, 6301.584956), (-2.5, 9840.098462)] {
        let (k, result) = shell(distance);
        let mut b = result.unwrap_or_else(|e| panic!("{distance}: {e:?}"));
        assert!(brep_is_manifold_solid(&b).unwrap(), "{distance}");
        let v = volume(&k, &mut b).unwrap();
        eprintln!("issue_1442 shell {distance}: volume {v}");
        assert!((v - want).abs() <= 1e-6 * want, "{distance}: {v} vs {want}");
    }
}

/// #854's plate (40 x 40 x 10, an r2 through hole) shelled 1.5 inward open at the top:
/// with the hole at x = 4.5 its offset (r 3.5) crosses the side's (x = 1.5) and the
/// floor is resolved (a759622's "plates at x 4.5 build as before"); at x = 5 they are
/// tangent at (1.5, 20) and the walls touch.
fn plate_shell(hole_x: f64) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.offset_resolve_crossing_loops = true;
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 10.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(hole_x, 20.0, -1.0), 2.0, 12.0).unwrap();
    let part = boolean_difference(&mut k, plate, hole).unwrap();
    let top = planar_faces_at(&k, &part, 2, 10.0);
    assert_eq!(top.len(), 1);
    let result = offset_fixture::shell(&mut k, part, -1.5, &top);
    (k, result)
}

#[test]
fn issue_1442_plate_hole_crossing_side_still_resolved() {
    let (k, result) = plate_shell(4.5);
    let mut b = result.unwrap();
    assert!(brep_is_manifold_solid(&b).unwrap());
    // the part less its cavity: the plate's inside 37 x 37 from z = 1.5 to 10, less the
    // hole's offset disc (r 3.5) where it lies inside x = 1.5 (3 from its centre)
    let pi = core::f64::consts::PI;
    let r: f64 = 3.5;
    let d: f64 = 3.0;
    let segment = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let cavity = (37.0 * 37.0 - (pi * r * r - segment)) * 8.5;
    let want = 40.0 * 40.0 * 10.0 - pi * 2.0 * 2.0 * 10.0 - cavity;
    let v = volume(&k, &mut b).unwrap();
    // to the integrator's 1e-6, and main's value (bit-identical with the flag on and off
    // there) to 1e-12
    assert!((v - want).abs() <= 1e-6 * want, "{v} vs {want}");
    let pinned = 4_554.583_387_224_843_f64;
    assert!((v - pinned).abs() <= 1e-12 * pinned, "{v} vs {pinned}");
}

#[test]
fn issue_1442_plate_hole_tangent_to_side_walls_touch() {
    let (_, result) = plate_shell(5.0);
    assert_eq!(code(&result), Some(SM_ERR_OFFSET_WALLS_TOUCH), "{result:?}");
}
