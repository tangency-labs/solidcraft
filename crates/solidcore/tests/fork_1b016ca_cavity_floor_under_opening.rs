//! Fork tangency-labs/smlib@1b016ca under `Kernel::fixes.offset_resolve_crossing_loops`
//! (WR-345 addendum): an offset face's loops winding the other way under an opening are
//! the cavity's floor.
//!
//! SolidCraft rig case shell/616 (a lid shelled 0.5 open at a rib's outer side, which
//! stands 0.15 in from the skirt's outer wall), rebuilt from the commit message: a
//! 20 x 20 x 5 block with a rib on its top, x 0.15..3.15, y 2..18, z 5..8, shelled 0.5
//! inward (extended convex edges, merged) open at the rib's outer side x = 0.15. The top's
//! offset (z = 4.5) has the rib's foot as a hole whose side along the open face stays at
//! x = 0.15 while the outer loop's side along the block's wall moves to x = 0.5. Between
//! them, under the opening, the loops wind round the other way: the cavity's floor.
//! `ResolveCrossingLoopsOfFace` kept only what the loops wind round once and the cavity
//! was left open there (`SM_ERR`, 1001); it is now kept as a reversed face.
//!
//! Closed form: the cavity is the block's (0.5..19.5)^2 x (0.5..4.5) (1444) and the
//! rib's (0.15..2.65) x (2.5..17.5) x (4.5..7.5) (112.5), of a body of 2144: a shell of
//! 587.5. The same rib standing 0.6 in (past the moved wall) has no such region and
//! shells to the same volume.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, volume, Notifications};
use solidcore::boolean::api_brep::boolean_union;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::{Brep, Kernel};

/// The block with the rib `inset` in from its outer wall x = 0.
fn ribbed(k: &mut Kernel, inset: f64) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 5.0).unwrap();
    let rib = create_box(k, Point3::new(inset, 2.0, 5.0), 3.0, 16.0, 3.0).unwrap();
    boolean_union(k, block, rib).unwrap()
}

/// The part shelled 0.5 inward open at the rib's outer side, under `fixes`.
fn shell(fixes: &UpstreamFixes, inset: f64) -> solidcore::Result<(bool, f64)> {
    let mut k = Kernel::default();
    k.fixes.clone_from(fixes);
    let mut part = ribbed(&mut k, inset);
    part.refresh_cached_topology();
    let open = planar_faces_at(&k, &part, 0, inset);
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
    .result?;
    wall.refresh_cached_topology();
    assert!(solidcore::topology::integrity::check(&wall).is_ok());
    let manifold = brep_is_manifold_solid(&wall)?;
    Ok((manifold, volume(&k, &mut wall)?))
}

fn crossing(walls_touch: bool) -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.offset_resolve_crossing_loops = true;
    f.offset_walls_touch_refused = walls_touch;
    f
}

/// Upstream (crossing loops not resolved): the merge returns no manifold solid.
#[test]
fn default_fixes_return_no_solid() {
    let (manifold, _) = shell(&UpstreamFixes::default(), 0.15).unwrap();
    assert!(!manifold);
}

/// The floor under the opening is kept: the shell is the closed form (with and without
/// a759622's touch refusal, which this case does not reach).
#[test]
fn floor_under_the_opening_is_kept() {
    for walls_touch in [false, true] {
        let (manifold, v) = shell(&crossing(walls_touch), 0.15).unwrap();
        assert!(manifold, "{walls_touch}");
        assert!((v - 587.5).abs() <= 1e-9 * 587.5, "{walls_touch}: {v}");
    }
}

/// Control: the rib 0.6 in, past the moved wall, has loops that do not cross; the shell
/// has the same closed form.
#[test]
fn rib_past_the_moved_wall_is_unchanged() {
    let (manifold, v) = shell(&crossing(false), 0.6).unwrap();
    assert!(manifold);
    assert!((v - 587.5).abs() <= 1e-9 * 587.5, "{v}");
}
