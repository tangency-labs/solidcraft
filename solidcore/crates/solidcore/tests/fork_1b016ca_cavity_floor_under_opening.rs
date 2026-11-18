//! Fork tangency-labs/smlib@1b016ca (WR-345 addendum): an offset face's loops winding the
//! other way under an opening are the cavity's floor.
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
//!
//! tangency-labs/solidcore#3329: a region the loops wind round the other way that lies
//! under no opening is not the cavity's floor. A drafted boss (45°) whose foot stands 0.2
//! in from the base's outer wall, shelled 0.5 open at the boss's top: the base top's offset
//! (z = -0.5) has the boss's foot as a hole that shrinks by only 0.5 (sqrt 2 - 1) while the
//! wall's side of the outer loop moves in by 0.5, so the hole runs out past it. Both walls
//! across that region are offset (the drafted side and the outer wall), and they meet each
//! other above it; the region was kept as a reversed face and the body came back closed
//! with a fold-back sliver of floor inside the wall (180° edges).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, volume, Notifications};
use solidcore::boolean::api_brep::{boolean_intersection, boolean_union};
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_pyramid};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// The block with the rib `inset` in from its outer wall x = 0.
fn ribbed(k: &mut Kernel, inset: f64) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 5.0).unwrap();
    let rib = create_box(k, Point3::new(inset, 2.0, 5.0), 3.0, 16.0, 3.0).unwrap();
    boolean_union(k, block, rib).unwrap()
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
        shell_options(),
        &open,
        &mut notifications.context(),
    )
    .result?;
    wall.refresh_cached_topology();
    assert!(solidcore::topology::integrity::check(&wall).is_ok());
    let manifold = brep_is_manifold_solid(&wall)?;
    Ok((manifold, volume(&k, &mut wall)?))
}

/// The floor under the opening is kept: the shell is the closed form (a759622's touch
/// refusal does not reach this case).
#[test]
fn floor_under_the_opening_is_kept() {
    let (manifold, v) = shell(&UpstreamFixes::default(), 0.15).unwrap();
    assert!(manifold);
    assert!((v - 587.5).abs() <= 1e-9 * 587.5, "{v}");
}

/// Control: the rib 0.6 in, past the moved wall, has loops that do not cross; the shell
/// has the same closed form.
#[test]
fn rib_past_the_moved_wall_is_unchanged() {
    let (manifold, v) = shell(&UpstreamFixes::default(), 0.6).unwrap();
    assert!(manifold);
    assert!((v - 587.5).abs() <= 1e-9 * 587.5, "{v}");
}

/// The base 16 x 6.4 x 4 (z = -4..0) with a 45° drafted square boss on its top: foot
/// 6 x 6, top 2 x 2 at z = 2 (a pyramid of base 6 and height 3 cut at z = 2). The foot's
/// sides at y = +-3 stand 0.2 in from the base's walls at y = +-3.2.
fn drafted_boss(k: &mut Kernel) -> Brep {
    let base = create_box(k, Point3::new(-8.0, -3.2, -4.0), 16.0, 6.4, 4.0).unwrap();
    let pyramid = create_pyramid(k, Point3::new(0.0, 0.0, 0.0), 6.0, 3.0).unwrap();
    let slab = create_box(k, Point3::new(-5.0, -5.0, 0.0), 10.0, 10.0, 2.0).unwrap();
    let boss = boolean_intersection(k, pyramid, slab).unwrap();
    boolean_union(k, base, boss).unwrap()
}

/// The drafted boss shelled 0.5 inward open at its top (solidcore#3329's shape) never
/// comes back with the sliver: built, it is the closed form with no face of the cavity
/// inside the wall; refused, the refusal is the offset's own. The two walls across the
/// region (the drafted side's offset and the outer wall's) end on the region's edges and
/// would have to be carried on past them to meet each other above it, which the offset
/// does not do yet: today the region is dropped and the merged walls do not close, which
/// the shell's manifold gate refuses (`OffsetFailure::Unsound`).
///
/// Closed form: the body is the base (409.6) and the boss's frustum (integral of
/// 4 (3 - z)^2 over 0..2: 104/3). The cavity is the base's (-7.5..7.5) x (-2.7..2.7) x
/// (-3.5..-0.5) (243) and, above z = -0.5, the boss's drafted sides moved in by 0.5
/// (half-width w = a - z, a = 3 - 0.5 sqrt 2) up to the opening at z = 2, cut at
/// y = +-2.7 by the base's moved walls where w is wider (z below z0 = a - 2.7):
/// 5.4 ((a + 0.5)^2 - 2.7^2) + 4/3 (2.7^3 - (a - 2)^3).
#[test]
fn region_under_no_opening_is_not_kept() {
    let mut k = Kernel::default();
    let mut part = drafted_boss(&mut k);
    part.refresh_cached_topology();
    let open = planar_faces_at(&k, &part, 2, 2.0);
    assert_eq!(open.len(), 1);
    let mut notifications = Notifications::new(&mut part);
    let result = OffsetExecutive::shell_brep(
        &mut k,
        part,
        shell_options(),
        &open,
        &mut notifications.context(),
    )
    .result;
    let mut wall = match result {
        Ok(wall) => wall,
        Err(e) => {
            assert!(
                matches!(
                    e.failure().map(|f| &f.reason),
                    Some(solidcore::error::Reason::Offset(_))
                ),
                "not an offset refusal: {e:?}"
            );
            return;
        }
    };
    wall.refresh_cached_topology();
    assert!(solidcore::topology::integrity::check(&wall).is_ok());
    assert!(brep_is_manifold_solid(&wall).unwrap());
    let check = check_brep(&mut k, &mut wall, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "{:?}",
        check.failures().collect::<Vec<_>>()
    );
    // no vertex below the base's top lies inside the wall along y = +-3.2 (0.5 thick):
    // the sliver's corners stood on the floor (z = -0.5) at |y| = a + 0.5
    for v in wall.vertices().ids().collect::<Vec<_>>() {
        let p = wall.vertex(v).unwrap().point;
        assert!(
            p.z >= -1e-6 || p.y.abs() <= 2.7 + 1e-6 || p.y.abs() >= 3.2 - 1e-6,
            "a vertex inside the wall: {p:?}"
        );
    }
    let a = 3.0 - 0.5 * 2.0_f64.sqrt();
    let body = 16.0 * 6.4 * 4.0 + 104.0 / 3.0;
    let cavity = 243.0
        + 5.4 * ((a + 0.5).powi(2) - 2.7_f64.powi(2))
        + 4.0 / 3.0 * (2.7_f64.powi(3) - (a - 2.0).powi(3));
    let want = body - cavity;
    let v = volume(&k, &mut wall).unwrap();
    assert!((v - want).abs() <= 1e-9 * want, "{v} against {want}");
}
