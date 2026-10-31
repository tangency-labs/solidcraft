//! WR-669 (`fixes.fillet_exact_round_classes`, the SolidCraft fork's tangency-labs/smlib@b5f2867
//! and 7ac037e): a constant round is exact by one classification of its spine from its
//! faces' kinds and placement, in place of upstream's try-chain (planes and cones only,
//! cylinder then torus, each gated on the edge's shape).
//!
//! - A sphere is a quadric now: the concave round where a coaxial cylinder leaves a ball
//!   is the ball's arc turned about the axis (a surface of revolution of the exact volume),
//!   where upstream traces and fits it.
//! - The point class (two cylinders as wide as the ball, their axes meeting) is the ball's
//!   piece without `fillet_corner_ball_patch` (WR-420), where upstream fails 1001.
//! - Rounds the try-chain already made exact (a box edge's cylinder, a plate's rim on a
//!   boss's torus) come out as before.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    volume_to(k, b, 1e-12)
}

fn volume_to(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| keep(midpoint(k, b, e)))
        .collect()
}

/// The number of faces on a surface of revolution (or torus).
fn revolved_faces(b: &Brep) -> usize {
    b.faces()
        .ids()
        .filter(|&f| {
            matches!(
                face::surface(b, f)
                    .unwrap()
                    .map(solidcore::surface::surface::Surface::kind),
                Some(SurfaceKind::SurfOfRevolution(_) | SurfaceKind::Torus(_))
            )
        })
        .count()
}

/// The r5 ball about the origin with the r2 cylinder up the z axis from z = 0 to 10
/// standing out of it, the concave edge where they meet (z = sqrt 21) rounded r1.
fn ball_and_peg(exact: bool) -> (Kernel, Brep, f64, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    let ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0).unwrap();
    let peg = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0, 10.0).unwrap();
    let mut b = boolean_union(&mut k, ball, peg).unwrap();
    let before = volume_to(&k, &mut b, 1e-7);
    let rim = edges_where(&k, &b, |m| (m.z - 21f64.sqrt()).abs() < 1e-6);
    assert_eq!(rim.len(), 1, "the rim edge");
    let r = fillet_edges(&mut k, &mut b, &rim, 1.0, 1, 1, 1.0);
    (k, b, before, r)
}

/// The volume the r1 round adds between the r2 cylinder and the r5 ball: the ball of
/// radius 1 centred 3 from the axis at height zc = sqrt 27 touches the cylinder at
/// rho = 2 and the ball at rho = 2.5; the region between the sphere and the ball's arc,
/// turned about the axis: `2 pi int_2^2.5 rho (zb(rho) - zs(rho)) drho`.
fn ball_and_peg_round_volume() -> f64 {
    let zc = 27f64.sqrt();
    // int rho (zc - sqrt(1 - (rho - 3)^2)) drho, u = rho - 3
    let arc = |u: f64| {
        let w = 1.0 - u * u;
        -(w * w.sqrt()) / 3.0 + 3.0 * (u * w.sqrt() + u.asin()) / 2.0
    };
    let ball_part = zc * (2.5f64 * 2.5 - 2.0 * 2.0) / 2.0 - (arc(-0.5) - arc(-1.0));
    // int rho sqrt(25 - rho^2) drho
    let sphere = |rho: f64| -(25.0 - rho * rho).powf(1.5) / 3.0;
    let sphere_part = sphere(2.5) - sphere(2.0);
    2.0 * PI * (ball_part - sphere_part)
}

/// The largest distance, over a 21 x 21 grid of each revolved face's natural domain,
/// between the ball's radius 1 and the distance from the spine (the circle rho = 3,
/// z = sqrt 27 the ball's centre runs on).
fn revolved_off_spine(k: &Kernel, b: &Brep) -> f64 {
    let zc = 27f64.sqrt();
    let mut worst = 0.0f64;
    for f in b.faces().ids() {
        let Some(s) = face::surface(b, f).unwrap() else {
            continue;
        };
        if !matches!(
            s.kind(),
            SurfaceKind::SurfOfRevolution(_) | SurfaceKind::Torus(_)
        ) {
            continue;
        }
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        for i in 0..=20 {
            for j in 0..=20 {
                let u = d.min().x + (d.max().x - d.min().x) * f64::from(i) / 20.0;
                let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 20.0;
                let p = s
                    .evaluate_point(k, solidcore::geom::vector2d::Point2::new(u, v))
                    .unwrap();
                let rho = (p.x * p.x + p.y * p.y).sqrt();
                let off = ((rho - 3.0).powi(2) + (p.z - zc).powi(2)).sqrt() - 1.0;
                worst = worst.max(off.abs());
            }
        }
    }
    worst
}

#[test]
fn a_round_between_a_ball_and_a_coaxial_peg_is_revolved_and_exact() {
    let (k, mut b, before, r) = ball_and_peg(true);
    r.unwrap();
    assert_eq!(
        revolved_faces(&b),
        1,
        "the round is a surface of revolution"
    );
    let off = revolved_off_spine(&k, &b);
    assert!(off < 1e-12, "the round is off the ball by {off:e}");
    let after = volume_to(&k, &mut b, 1e-7);
    let added = ball_and_peg_round_volume();
    assert!(
        (after - (before + added)).abs() < 1e-6 * after,
        "{before} + {added} != {after} (off {:e})",
        after - before - added
    );
}

#[test]
fn upstream_traces_the_round_between_a_ball_and_a_peg() {
    let (_, b, _, r) = ball_and_peg(false);
    r.unwrap();
    assert_eq!(revolved_faces(&b), 0, "upstream fits the traced round");
}

/// A 25 x 25 x 10 tray, bottom edges R2, then the vertical edges R2 (the corner ball where
/// two bottom rounds meet: the point class) as in tangency-labs/solidcore#416.
fn tray(exact: bool) -> (f64, f64, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    let mut tray = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 25.0, 25.0, 10.0).unwrap();
    let bottom = edges_where(&k, &tray, |m| m.z.abs() < 1e-6);
    fillet_edges(&mut k, &mut tray, &bottom, 2.0, 1, 1, 1.0).unwrap();
    let rounded = volume(&k, &mut tray);
    let near_side = |v: f64, side: f64| (v.min(side - v) > 0.1) && (v.min(side - v) < 1.9);
    let vertical = edges_where(&k, &tray, |m| {
        (m.z - 6.0).abs() < 1e-6
            || (m.z > 0.1 && m.z < 1.9 && near_side(m.x, 25.0) && near_side(m.y, 25.0))
    });
    let r = fillet_edges(&mut k, &mut tray, &vertical, 2.0, 1, 1, 1.0);
    (rounded, volume(&k, &mut tray), r)
}

#[test]
fn the_point_class_makes_the_corner_ball_without_the_ball_patch_flag() {
    // each vertical edge loses (1 - pi/4) 4 along 8, each corner the octant of the two
    // bottom rounds' crossing (16/3) less the corner ball's (4 pi / 3)
    let removed = 4.0 * (32.0 * (1.0 - PI / 4.0) + 16.0 / 3.0 - 4.0 * PI / 3.0);
    let (rounded, after, r) = tray(true);
    r.unwrap();
    assert!(
        (after - (rounded - removed)).abs() < 1e-7 * after,
        "{rounded} {after}"
    );
    let (_, _, r) = tray(false);
    assert!(
        matches!(r, Err(solidcore::SolidError::Upstream { code: 1001, .. })),
        "{r:?}"
    );
}

/// A 20 x 20 x 4 plate with an r3 boss 6 high at its centre, the boss's foot (plane square
/// to the cylinder's axis: the circle class) and one plate edge (two planes: the line
/// class) rounded r1.
fn plate_and_boss(exact: bool) -> (f64, usize) {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = exact;
    let plate = create_box(&mut k, Point3::new(-10.0, -10.0, 0.0), 20.0, 20.0, 4.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(0.0, 0.0, 2.0), 3.0, 8.0).unwrap();
    let mut b = boolean_union(&mut k, plate, boss).unwrap();
    let foot = edges_where(&k, &b, |m| {
        (m.z - 4.0).abs() < 1e-6 && ((m.x * m.x + m.y * m.y).sqrt() - 3.0).abs() < 1e-6
    });
    assert!(!foot.is_empty());
    fillet_edges(&mut k, &mut b, &foot, 1.0, 1, 1, 1.0).unwrap();
    let side = edges_where(&k, &b, |m| {
        (m.z - 4.0).abs() < 1e-6 && (m.x - 10.0).abs() < 1e-6
    });
    assert_eq!(side.len(), 1);
    fillet_edges(&mut k, &mut b, &side, 1.0, 1, 1, 1.0).unwrap();
    (volume(&k, &mut b), revolved_faces(&b))
}

#[test]
fn line_and_circle_rounds_the_chain_made_exact_are_unchanged() {
    let (exact, n_exact) = plate_and_boss(true);
    let (chain, n_chain) = plate_and_boss(false);
    // plate 1600 + boss 36 pi 6; the foot's round adds 2 pi int_3^4 rho (1 - sqrt(1 -
    // (rho - 4)^2)) drho = 2 pi (7/2 - (4 pi / 4 - 1/3)); the plate edge's takes 20 (1 - pi/4)
    let foot = 2.0 * PI * (3.5 - (PI - 1.0 / 3.0));
    let expected = 1600.0 + 54.0 * PI + foot - 20.0 * (1.0 - PI / 4.0);
    assert!(
        (exact - expected).abs() < 1e-9 * expected,
        "{exact} != {expected}"
    );
    assert!((exact - chain).abs() < 1e-9 * exact, "{exact} != {chain}");
    assert_eq!((n_exact, n_chain), (1, 1));
}
