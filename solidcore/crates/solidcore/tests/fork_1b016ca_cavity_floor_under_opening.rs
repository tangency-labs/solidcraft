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
//! with a fold-back sliver of floor inside the wall (180° edges). The region is dropped
//! and the two walls are carried on past it until they meet. The same with curved walls,
//! as reported: a round boss off the axis of a cylinder, its foot rounded r1.2 to 0.1 in
//! from the cylinder's side.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::math::det;

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, volume, Notifications};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_intersection, boolean_union};
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_pyramid};
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

/// The drafted boss shelled 0.5 inward open at its top (solidcore#3329's shape) is the
/// closed form, with no face of the cavity inside the wall. The two walls across the
/// region (the drafted side's offset and the outer wall's) end on the region's edges; they
/// are carried on past them, meet above the region, and the merge cuts them there.
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
    let mut wall = OffsetExecutive::shell_brep(
        &mut k,
        part,
        shell_options(),
        &open,
        &mut notifications.context(),
    )
    .result
    .unwrap();
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
        + 5.4 * (det::powi(a + 0.5, 2) - det::powi(2.7_f64, 2))
        + 4.0 / 3.0 * (det::powi(2.7_f64, 3) - det::powi(a - 2.0, 3));
    let want = body - cavity;
    let v = volume(&k, &mut wall).unwrap();
    assert!((v - want).abs() <= 1e-9 * want, "{v} against {want}");
}

/// Simpson's rule for `f` over `[a, b]` in `n` (even) steps.
fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64, n: u32) -> f64 {
    let h = (b - a) / f64::from(n);
    let mut sum = f(a) + f(b);
    for i in 1..n {
        let weight = if i % 2 == 1 { 4.0 } else { 2.0 };
        sum += weight * f(a + f64::from(i) * h);
    }
    sum * h / 3.0
}

/// solidcore#3329 as reported, with curved walls: a cylinder r5 (z = -10..0) with a round
/// boss r1.2 at (2.5, 0) on its top (z = 0..10), the boss's foot rounded r1.2, shelled 0.5
/// inward open at the boss's top. The round's offset (tube r1.7) meets the floor's offset
/// (z = -0.5) on a circle of r2.4 about the boss's axis, out to 4.9 from the cylinder's
/// axis, past its moved side (r4.5). Before, the shell came back closed with the sliver;
/// now it is the body less the cavity, with no vertex inside the side's wall.
///
/// The volumes, in rho about the boss's axis: the round's ring
/// (integral over rho 1.2..2.4 of 2 pi rho (1.2 - sqrt(1.2^2 - (rho - 2.4)^2)));
/// the cavity: the cylinder's r4.5 over z = -9.5..-0.5, the boss's r0.7 over z =
/// -0.5..10 (open at the top), and the round's moved ring over rho 0.7..2.4, from z = -0.5
/// up to the tube r1.7, taken where it lies inside r4.5 about the cylinder's axis: at
/// rho, all of the circle but the arc where cos(theta) > (4.5^2 - 2.5^2 - rho^2) /
/// (2 2.5 rho).
#[test]
fn round_past_a_curved_wall_meets_it() {
    let mut k = Kernel::default();
    let base = create_cylinder(&mut k, Point3::new(0.0, 0.0, -10.0), 5.0, 10.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(2.5, 0.0, 0.0), 1.2, 10.0).unwrap();
    let mut part = boolean_union(&mut k, base, boss).unwrap();
    // the boss's foot: the circle at z = 0 through (2.5, 1.2, 0)
    let foot: Vec<_> = part
        .edges()
        .ids()
        .collect::<Vec<_>>()
        .into_iter()
        .filter(|&e| {
            solidcore::query::edge::closest_point(&k, &part, e, Point3::new(2.5, 1.2, 0.0))
                .unwrap()
                .is_some_and(|cp| cp.distance < 1e-6)
        })
        .collect();
    assert_eq!(foot.len(), 1);
    fillet_edges(&mut k, &mut part, &foot, 1.2, 1, 1, 1.0).unwrap();
    part.refresh_cached_topology();
    let open = planar_faces_at(&k, &part, 2, 10.0);
    assert_eq!(open.len(), 1);
    let mut notifications = Notifications::new(&mut part);
    let mut wall = OffsetExecutive::shell_brep(
        &mut k,
        part,
        shell_options(),
        &open,
        &mut notifications.context(),
    )
    .result
    .unwrap();
    wall.refresh_cached_topology();
    assert!(solidcore::topology::integrity::check(&wall).is_ok());
    assert!(brep_is_manifold_solid(&wall).unwrap());
    let check = check_brep(&mut k, &mut wall, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "{:?}",
        check.failures().collect::<Vec<_>>()
    );
    for v in wall.vertices().ids().collect::<Vec<_>>() {
        let p = wall.vertex(v).unwrap().point;
        let r = det::hypot(p.x, p.y);
        assert!(
            p.z >= -1e-6 || r <= 4.5 + 1e-6 || r >= 5.0 - 1e-6,
            "a vertex inside the wall: {p:?}"
        );
    }
    let pi = core::f64::consts::PI;
    let n = 200_000;
    let ring = simpson(
        |rho| 2.0 * pi * rho * (1.2 - (1.44 - det::powi(rho - 2.4, 2)).max(0.0).sqrt()),
        1.2,
        2.4,
        n,
    );
    let body = pi * 25.0 * 10.0 + pi * 1.44 * 10.0 + ring;
    let around = |rho: f64| {
        let c = (det::powi(4.5_f64, 2) - det::powi(2.5_f64, 2) - rho * rho) / (2.0 * 2.5 * rho);
        if c >= 1.0 {
            2.0 * pi
        } else {
            2.0 * pi - 2.0 * det::acos(c)
        }
    };
    let moved_ring = simpson(
        |rho| rho * around(rho) * (1.7 - (2.89 - det::powi(rho - 2.4, 2)).max(0.0).sqrt()),
        0.7,
        2.4,
        n,
    );
    let cavity = pi * 4.5 * 4.5 * 9.0 + pi * 0.49 * 10.5 + moved_ring;
    let want = body - cavity;
    let v = volume(&k, &mut wall).unwrap();
    assert!((v - want).abs() <= 1e-8 * want, "{v} against {want}");
}
