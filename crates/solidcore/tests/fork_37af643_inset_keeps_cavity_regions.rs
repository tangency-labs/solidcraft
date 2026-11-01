//! Fork tangency-labs/smlib@37af643 under `Kernel::fixes.offset_keep_cavity_regions`
//! (WR-667): an inset keeps the regions that are cavity, not every finite region.
//!
//! SolidCraft rig case shell/545's frame, rebuilt from the commit message: a 20 x 20 x 5
//! block with a recess from the top ((5, 5)..(15, 15), z 3..5) and a square ring cut from
//! the bottom ((2, 2)..(18, 18) less (7, 7)..(13, 13), z 0..2.5), so a 0.5 floor lies
//! between the recess and the ring where they overlap. Shelled 0.5 inward (extended convex
//! edges, merged) open at the ring cut's outer wall x = 18: the floor is no thicker than
//! the wall, its two faces' offsets cross, and the merge closes the region between them.
//! Upstream's `MakeManifold` keeps every finite region ("we will need to select regions
//! to keep"), so the result is no manifold solid. With the flag only the cavity is kept.
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

/// The frame shelled 0.5 inward open at the ring cut's outer wall (x = 18), under
/// `fixes`: whether the result is a manifold solid, and its volume.
fn shell(fixes: &UpstreamFixes, ring_top: f64) -> (bool, f64) {
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
        ShellOptions {
            offset: OffsetOptions {
                distance: -0.5,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: true,
        },
        &open,
        &mut notifications.context(),
    )
    .result
    .unwrap();
    wall.refresh_cached_topology();
    assert!(solidcore::topology::integrity::check(&wall).is_ok());
    let manifold = brep_is_manifold_solid(&wall).unwrap();
    (manifold, volume(&k, &mut wall).unwrap())
}

fn keep_cavity() -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.offset_keep_cavity_regions = true;
    f
}

/// Upstream (flag off): the sliver between the floor's crossing offsets is kept too and
/// the result is no manifold solid.
#[test]
fn flag_off_keeps_the_sliver_region() {
    let (manifold, _) = shell(&UpstreamFixes::default(), 2.5);
    assert!(!manifold);
}

/// The flag: only the cavity is made manifold; the shell is the closed form.
#[test]
fn flag_on_keeps_only_the_cavity() {
    let (manifold, v) = shell(&keep_cavity(), 2.5);
    assert!(manifold);
    assert!((v - 638.75).abs() <= 1e-9 * 638.75, "{v}");
}

/// With the SolidCraft set's offset fixes that touch this path as well (WR-345's crossing
/// loops, WR-455's one-pass stitch), the flag gives the same shell.
#[test]
fn flag_on_with_offset_fixes() {
    let mut fixes = keep_cavity();
    fixes.offset_resolve_crossing_loops = true;
    fixes.offset_stitch_at_body_tolerance = true;
    fixes.offset_extended_edges_mated = true;
    let (manifold, v) = shell(&fixes, 2.5);
    assert!(manifold);
    assert!((v - 638.75).abs() <= 1e-9 * 638.75, "{v}");
}

/// Control: a 1.0 floor (ring top at 2) has no crossing offsets, one finite region, and
/// shells the same with the flag off and on.
#[test]
fn thicker_floor_is_unchanged() {
    let off = shell(&UpstreamFixes::default(), 2.0);
    let on = shell(&keep_cavity(), 2.0);
    assert!(off.0 && on.0);
    assert_eq!(off.1.to_bits(), on.1.to_bits());
}
