//! WR-468 (`fillet_through_extended_rail_end`, the fork's tangency-labs/smlib@d33890f) on
//! overhanging bosses, with the fork's other Nx1 corner fixes on: WR-499
//! (`fillet_nx1_opposite_continuation_side_edge`, 420e47d) and WR-457
//! (`fillet_nx1_tangent_side_edge`), without which a boss's base arc that runs on past a
//! wall as the overhang's rim fails at its corner (#1021).
//!
//! - The fork's own case (its capi test for SolidCraft rig fillets/4369): a 60 x 40 x 10
//!   block rounded r 10 in plan, a boss r 10 x 10 at (12, 32) overhanging its corner, the
//!   boss's base arc over the block rounded R 2. The round adds the material within 2 of
//!   the boss's foot under the quarter disc over the block's outline, to 1e-7 as the
//!   fork's test holds it. The port gives that with and without WR-468, and so does the
//!   fork's main (83748ba) C++ built with and without d33890f's hunk.
//! - A 30 x 60 x 10 block with a boss r 5 x 10 at (4, 33) overhanging its wall x = 0
//!   (WR-641): without WR-468 the round succeeds, 18807.824471783 here and
//!   18807.824471782 in the fork's main without the hunk. With WR-468 the trace point
//!   it adds at the edge's end cannot be solved (`sm_FindFilletTsectPnt` from a guess
//!   point past the wall) and the round fails with 1001 "Fillet Surface Computation
//!   Failure", as it does in the fork's main C++. This test pins that parity; it is the
//!   fork's defect (`plan/upstream-issues/WR-641--*.md`), not the port's.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel, SolidError};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The fork's Nx1 corner fixes on, WR-468 as `wr468` says.
fn kernel(wr468: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_nx1_opposite_continuation_side_edge = true;
    k.fixes.fillet_nx1_tangent_side_edge = true;
    k.fixes.fillet_through_extended_rail_end = wr468;
    k
}

/// The distance from `p` to edge `e`, over 4001 points along it.
fn distance_to_edge(k: &Kernel, b: &Brep, e: EdgeId, p: Point3) -> f64 {
    let c = edge::curve(b, e).unwrap().unwrap();
    let iv = edge::interval(b, e).unwrap();
    (0..=4000)
        .map(|i| {
            c.evaluate_point(k, iv.evaluate(f64::from(i) / 4000.0))
                .unwrap()
                .distance(p)
        })
        .fold(f64::INFINITY, f64::min)
}

/// `SmApiFilletEdges` (circular, G1) with radius `r` on the edges nearest `picks`.
fn round(k: &mut Kernel, b: &mut Brep, picks: &[Point3], r: f64) -> solidcore::Result<()> {
    let all: Vec<EdgeId> = b.edges().ids().collect();
    let edges: Vec<EdgeId> = picks
        .iter()
        .map(|&p| {
            *all.iter()
                .min_by(|&&x, &&y| {
                    distance_to_edge(k, b, x, p).total_cmp(&distance_to_edge(k, b, y, p))
                })
                .unwrap()
        })
        .collect();
    let mut session = FilletSession::new(k, b).unwrap();
    session.prepare_constant_radius_edges(k, &edges, r, 1, 1, 1.0)?;
    session.do_fillet(k)
}

/// The fork's 4369 block: a 60 x 40 rectangle with corners rounded r 10, extruded 10.
fn rounded_block(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let line = |a, b| Curve::from(create_line_segment(a, b).unwrap());
    let arc = |c, s: f64, e: f64| Curve::from(create_arc(c, 10.0, s, e).unwrap());
    let profile = [
        line(p(10.0, 0.0), p(50.0, 0.0)),
        arc(p(50.0, 10.0), -90.0, 0.0),
        line(p(60.0, 10.0), p(60.0, 30.0)),
        arc(p(50.0, 30.0), 0.0, 90.0),
        line(p(50.0, 40.0), p(10.0, 40.0)),
        arc(p(10.0, 30.0), 90.0, 180.0),
        line(p(0.0, 30.0), p(0.0, 10.0)),
        arc(p(10.0, 10.0), 180.0, 270.0),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

/// The fork test's closed form: the integral of `2 - sqrt(4 - (12 - rho)^2)` over the plan
/// annulus `10 < rho < 12` about the boss's axis inside the block's outline (its 1000 x
/// 8000 midpoint rule).
fn added_by_the_round() -> f64 {
    let in_outline = |x: f64, y: f64| {
        let qx = (10.0 - x).max(0.0).max(x - 50.0);
        let qy = (10.0 - y).max(0.0).max(y - 30.0);
        (0.0..=60.0).contains(&x) && (0.0..=40.0).contains(&y) && qx * qx + qy * qy <= 100.0
    };
    let (n_rho, n_turn) = (1000, 8000);
    let mut added = 0.0;
    for i in 0..n_rho {
        let rho = 10.0 + (f64::from(i) + 0.5) * 2.0 / f64::from(n_rho);
        let height = 2.0 - (4.0 - (12.0 - rho) * (12.0 - rho)).sqrt();
        for j in 0..n_turn {
            let t = (f64::from(j) + 0.5) * 2.0 * PI / f64::from(n_turn);
            if in_outline(12.0 + rho * t.cos(), 32.0 + rho * t.sin()) {
                added += height * rho * (2.0 / f64::from(n_rho)) * (2.0 * PI / f64::from(n_turn));
            }
        }
    }
    added
}

#[test]
fn fork_4369_boss_rounds_to_its_closed_form_with_and_without_wr468() {
    let added = added_by_the_round();
    let foot = [
        Point3::new(12.0, 22.0, 10.0),
        Point3::new(
            12.0 + 10.0 * (PI / 6.0).cos(),
            32.0 + 10.0 * (PI / 6.0).sin(),
            10.0,
        ),
    ];
    for wr468 in [false, true] {
        let mut k = kernel(wr468);
        let block = rounded_block(&mut k);
        let boss = create_cylinder(&mut k, Point3::new(12.0, 32.0, 10.0), 10.0, 10.0).unwrap();
        let mut b = boolean_union(&mut k, block, boss).unwrap();
        let joined = volume(&k, &mut b);
        round(&mut k, &mut b, &foot, 2.0).unwrap();
        assert!(is_manifold_solid(&b).unwrap(), "WR-468 {wr468}");
        let (v, want) = (volume(&k, &mut b), joined + added);
        assert!(
            ((v - want) / want).abs() < 1e-7,
            "WR-468 {wr468}: {v} against {want}"
        );
    }
}

/// The 30 x 60 x 10 block with the boss r 5 x 10 at (4, 33) over its wall x = 0, its base
/// arc over the block (two edges, split at the cylinder's seam) rounded R 2.
fn wall_boss(wr468: bool) -> (Kernel, Brep, solidcore::Result<()>) {
    let mut k = kernel(wr468);
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 60.0, 10.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(4.0, 33.0, 10.0), 5.0, 10.0).unwrap();
    let mut b = boolean_union(&mut k, block, boss).unwrap();
    let a = PI / 3.0;
    let picks = [
        Point3::new(4.0 + 5.0 * a.cos(), 33.0 + 5.0 * a.sin(), 10.0),
        Point3::new(4.0 + 5.0 * a.cos(), 33.0 - 5.0 * a.sin(), 10.0),
    ];
    let r = round(&mut k, &mut b, &picks, 2.0);
    (k, b, r)
}

#[test]
fn wr_641_wall_boss_rounds_without_wr468() {
    let (k, mut b, r) = wall_boss(false);
    r.unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    let v = volume(&k, &mut b);
    // the fork's main without d33890f's hunk: 18807.824471782
    assert!((v - 18807.824471782).abs() < 1e-9 * v, "{v}");
}

/// WR-641: the fork's main C++ (with d33890f) fails on this boss with 1001, and so does the
/// port with WR-468 on. Pinned for parity; the defect is the fork's.
#[test]
fn wr_641_wr468_fails_the_wall_boss_as_the_fork_does() {
    let (_, b, r) = wall_boss(true);
    assert!(
        matches!(r, Err(SolidError::Upstream { code: 1001, .. })),
        "{r:?}"
    );
    assert!(is_manifold_solid(&b).unwrap(), "the body is left as it was");
}
