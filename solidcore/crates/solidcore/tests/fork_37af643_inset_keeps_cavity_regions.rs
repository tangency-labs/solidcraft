//! Fork tangency-labs/smlib@37af643 (WR-667), the kernel's default: an inset keeps the
//! regions that are cavity, not every finite region.
//!
//! SolidCraft rig case shell/545's frame, rebuilt from the commit message: a 20 x 20 x 5
//! block with a recess from the top ((5, 5)..(15, 15), z 3..5) and a square ring cut from
//! the bottom ((2, 2)..(18, 18) less (7, 7)..(13, 13), z 0..2.5), so a 0.5 floor lies
//! between the recess and the ring where they overlap. Shelled 0.5 inward (extended convex
//! edges, merged) open at the ring cut's outer wall x = 18: the floor is no thicker than
//! the wall, its two faces' offsets cross, and the merge closes the region between them.
//! Upstream's `MakeManifold` keeps every finite region ("we will need to select regions
//! to keep"), so the result is no manifold solid. The kernel keeps only the cavity.
//!
//! Closed form (sharp offsets, the open face's neighbours' walls ending at its plane):
//! the cavity is z 0.5..2.5 over the core (7.5..12.5)^2 and the outer band
//! (0.5..19.5)^2 less (1.5..18) x (1.5..18.5) (25 + 80.5, height 2: 211), z 2.5..3 over
//! the band (80.5 x 0.5: 40.25), and z 3..4.5 over (0.5..19.5)^2 less the recess walls
//! (4.5..15.5)^2 (240 x 1.5: 360): 611.25 of the body's 1250, a shell of 638.75.
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
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// The frame, the ring cut's top at `ring_top` (2.5: a 0.5 floor under the recess).
fn frame(k: &mut Kernel, ring_top: f64) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 5.0).unwrap();
    let recess = create_box(k, Point3::new(5.0, 5.0, 3.0), 10.0, 10.0, 3.0).unwrap();
    let outer = create_box(k, Point3::new(2.0, 2.0, -1.0), 16.0, 16.0, ring_top + 1.0).unwrap();
    let inner = create_box(k, Point3::new(7.0, 7.0, -2.0), 6.0, 6.0, 5.0).unwrap();
    let ring = boolean_difference(k, outer, inner).unwrap();
    let recessed = boolean_difference(k, block, recess).unwrap();
    boolean_difference(k, recessed, ring).unwrap()
}

fn shell_options() -> ShellOptions {
    ShellOptions {
        offset: OffsetOptions {
            distance: -0.5,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    }
}

/// The frame shelled 0.5 inward open at the ring cut's outer wall (x = 18), under
/// `fixes`: the shell's result.
fn shell_result(fixes: &UpstreamFixes, ring_top: f64) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.clone_from(fixes);
    let mut part = frame(&mut k, ring_top);
    part.refresh_cached_topology();
    let open = planar_faces_at(&k, &part, 0, 18.0);
    assert_eq!(open.len(), 1);
    let mut notifications = Notifications::new(&mut part);
    let mut wall = OffsetExecutive::shell_brep(
        &mut k,
        part,
        shell_options(),
        &open,
        &mut notifications.context(),
    )
    .result;
    if let Ok(wall) = wall.as_mut() {
        wall.refresh_cached_topology();
    }
    (k, wall)
}

/// [`shell_result`]'s body: whether it is a manifold solid, and its volume.
fn shell(fixes: &UpstreamFixes, ring_top: f64) -> (bool, f64) {
    let (k, wall) = shell_result(fixes, ring_top);
    let mut wall = wall.unwrap();
    assert!(solidcore::topology::integrity::check(&wall).is_ok());
    let manifold = brep_is_manifold_solid(&wall).unwrap();
    (manifold, volume(&k, &mut wall).unwrap())
}

/// Only the cavity is made manifold; the shell is the closed form and passes `check_brep`
/// at every level.
#[test]
fn keeps_only_the_cavity() {
    let (mut k, wall) = shell_result(&UpstreamFixes::default(), 2.5);
    let mut wall = wall.unwrap();
    for level in [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2] {
        let checked = check_brep(&mut k, &mut wall, level).unwrap();
        assert!(checked.is_valid(), "{level:?}: {:?}", checked.lines());
    }
    let (manifold, v) = shell(&UpstreamFixes::default(), 2.5);
    assert!(manifold);
    assert!((v - 638.75).abs() <= 1e-9 * 638.75, "{v}");
}

/// Control: a 1.0 floor (ring top at 2) has no crossing offsets and one finite region, so
/// there is nothing to select. Closed form as above: the body is 2000 - 200 - 220 x 2 =
/// 1360; the cavity is z 0.5..2.5 over the core and the band ((25 + 80.5) x 2: 211) and
/// z 2.5..4.5 over (0.5..19.5)^2 less the recess walls (240 x 2: 480), 691; a shell of 669.
#[test]
fn thicker_floor_is_the_closed_form() {
    let (manifold, v) = shell(&UpstreamFixes::default(), 2.0);
    assert!(manifold);
    assert!((v - 669.0).abs() <= 1e-9 * 669.0, "{v}");
}
