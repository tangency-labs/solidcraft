//! tangency-labs/solidcore#1442 (fork tangency-labs/smlib a759622): an offset whose walls
//! would meet is refused as such. SolidCraft rig case shell/4711: a 60 x 40 x 10 block
//! less an r3 through hole at (22.5, 7) and a 10 x 10 notch (x 25..35, through the
//! front), shelled inward open at the bottom face (z = 0).
//!
//! The hole lies 4 = 2 x 2 from the front face, so shelled 2 the hole wall's offset
//! (r 5) is tangent to the front wall's offset (y = 2) at (22.5, 2): the offset face on
//! z = 0 has loops that touch without crossing, the walls meet along a line and the
//! offset bounds no solid there. With `offset_resolve_crossing_loops` and
//! `offset_walls_touch_refused` the shell fails with `SM_ERR_OFFSET_WALLS_TOUCH` (1042);
//! with `offset_resolve_crossing_loops` alone it fails with `SM_ERR_BAD_INTERSECTIONS`
//! (1027), as the fork did before a759622. Thicknesses 1.5 and 2.5 (loops apart or
//! crossing) build either way.
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
use solidcore::error::{SolidError, SM_ERR_OFFSET_WALLS_TOUCH};
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::{Brep, Kernel};

/// `SM_ERR_BAD_INTERSECTIONS`.
const SM_ERR_BAD_INTERSECTIONS: i32 = 1027;

fn part(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let pin = create_cylinder(k, Point3::new(22.5, 7.0, -5.0), 3.0, 20.0).unwrap();
    let notch = create_box(k, Point3::new(25.0, -5.0, -5.0), 10.0, 10.0, 20.0).unwrap();
    let holed = boolean_difference(k, block, pin).unwrap();
    boolean_difference(k, holed, notch).unwrap()
}

fn shell(walls_touch: bool, distance: f64) -> (Kernel, solidcore::Result<Brep>) {
    shell_with(true, walls_touch, distance)
}

fn shell_with(
    resolve_crossing: bool,
    walls_touch: bool,
    distance: f64,
) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.offset_resolve_crossing_loops = resolve_crossing;
    k.fixes.offset_walls_touch_refused = walls_touch;
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
        Err(SolidError::Upstream { code, .. }) => Some(*code),
        _ => None,
    }
}

#[test]
fn issue_1442_walls_touch_refused_with_its_own_status() {
    let (_, result) = shell(true, -2.0);
    assert_eq!(code(&result), Some(SM_ERR_OFFSET_WALLS_TOUCH), "{result:?}");
    if let Err(SolidError::Upstream { operation, .. }) = &result {
        assert!(operation.starts_with("Offset face loops touch without crossing"));
    }
}

#[test]
fn issue_1442_walls_touch_without_the_opt_in_is_bad_intersections() {
    let (_, result) = shell(false, -2.0);
    assert_eq!(code(&result), Some(SM_ERR_BAD_INTERSECTIONS), "{result:?}");
}

/// `offset_walls_touch_refused` only changes `ResolveCrossingLoopsOfFace`, which runs
/// only with `offset_resolve_crossing_loops`: without it the shell is the default's.
#[test]
fn issue_1442_walls_touch_needs_resolve_crossing_loops() {
    let summary = |r: &solidcore::Result<Brep>| match r {
        Ok(b) => format!("Ok faces={}", b.face_list().len()),
        Err(e) => format!("{e:?}"),
    };
    let (_, default) = shell_with(false, false, -2.0);
    let (_, alone) = shell_with(false, true, -2.0);
    assert_eq!(summary(&default), summary(&alone));
}

/// The fork's C++ (a759622's commit message: 4711 at 1.5 and 2.5).
#[test]
fn issue_1442_thinner_and_thicker_walls_still_build() {
    for (distance, want) in [(-1.5, 6301.584956), (-2.5, 9840.098462)] {
        for walls_touch in [false, true] {
            let (k, result) = shell(walls_touch, distance);
            let mut b = result.unwrap_or_else(|e| panic!("{distance} {walls_touch}: {e:?}"));
            assert!(brep_is_manifold_solid(&b).unwrap(), "{distance}");
            let v = volume(&k, &mut b).unwrap();
            eprintln!("issue_1442 shell {distance} walls_touch={walls_touch}: volume {v}");
            assert!((v - want).abs() <= 1e-6 * want, "{distance}: {v} vs {want}");
        }
    }
}

/// #854's plate (40 x 40 x 10, an r2 through hole) shelled 1.5 inward open at the top:
/// with the hole at x = 4.5 its offset (r 3.5) crosses the side's (x = 1.5) and the
/// floor is resolved (a759622's "plates at x 4.5 build as before"); at x = 5 they are
/// tangent at (1.5, 20) and the walls touch.
fn plate_shell(hole_x: f64, walls_touch: bool) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.offset_resolve_crossing_loops = true;
    k.fixes.offset_walls_touch_refused = walls_touch;
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
    let (k, result) = plate_shell(4.5, true);
    let mut b = result.unwrap();
    assert!(brep_is_manifold_solid(&b).unwrap());
    let (k0, result0) = plate_shell(4.5, false);
    let v = volume(&k, &mut b).unwrap();
    let v0 = volume(&k0, &mut result0.unwrap()).unwrap();
    assert_eq!(v.to_bits(), v0.to_bits(), "{v} vs {v0}");
}

#[test]
fn issue_1442_plate_hole_tangent_to_side_walls_touch() {
    let (_, result) = plate_shell(5.0, true);
    assert_eq!(code(&result), Some(SM_ERR_OFFSET_WALLS_TOUCH), "{result:?}");
}
