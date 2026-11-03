//! WR-707 addendum: with `fixes.fillet_refuse_rounds_breaking_through`, a blend whose
//! section runs off its face (past a cliff, into a hole or an opening) is not refused for
//! breaking through. Only a section that leaves the part through a face and comes back
//! into it (a hollow behind a wall it opens, #2299) breaks through.
//!
//! The cases are SolidCraft rig cases (SolidCraft `tests/kernel-cases/fillets`, built by
//! tangency-labs/smlib 5e8c9c1) that the first version of the flag refused. Each value is
//! a closed form computed here, or (for the notched block, whose corner blends have none)
//! the rig's `expected.json`. Every flag is on, as SolidCraft's backend runs them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::NoRoomReason;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel, SolidError};
use std::f64::consts::PI;

fn kernel(fix: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_refuse_rounds_breaking_through = fix;
    k.fixes = fixes;
    k
}

fn pick(k: &Kernel, b: &Brep, q: [f64; 3]) -> EdgeId {
    let q = Point3::new(q[0], q[1], q[2]);
    b.edges()
        .ids()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 0.1)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("edge")
        .0
}

fn cuboid(k: &mut Kernel, o: [f64; 3], s: [f64; 3]) -> Brep {
    create_box(k, Point3::new(o[0], o[1], o[2]), s[0], s[1], s[2]).unwrap()
}

/// A cylinder along z.
fn post(k: &mut Kernel, o: [f64; 3], r: f64, h: f64) -> Brep {
    create_cylinder(k, Point3::new(o[0], o[1], o[2]), r, h).unwrap()
}

/// `SmApiFilletEdges` with SolidCraft's arguments (G1, thumbweight 1): a round
/// (`xsect` 1) or a chamfer (`xsect` 0, the ball radius being the leg on square edges).
fn blend(
    k: &mut Kernel,
    b: &mut Brep,
    at: &[[f64; 3]],
    r: f64,
    xsect: u32,
) -> solidcore::Result<()> {
    let edges: Vec<EdgeId> = at.iter().map(|&q| pick(k, b, q)).collect();
    fillet_edges(k, b, &edges, r, xsect, 1, 1.0)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// Builds with the flag, one lump, `check_brep` clean, to `want` within `rel`.
fn builds(case: fn(&mut Kernel) -> (Brep, solidcore::Result<()>), want: f64, rel: f64) {
    let mut k = kernel(true);
    let (mut b, built) = case(&mut k);
    built.unwrap();
    let infinite = b.infinite_region();
    let lumps = regions(&b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count();
    assert_eq!(lumps, 1, "one lump");
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.lines().is_empty(), "check_brep: {:?}", check.lines());
    let v = volume(&k, &mut b);
    assert!((v - want).abs() <= rel * want, "{v} vs {want}");
}

/// A 20 cube with three square openings 10 x 10 through it (x, y and z, 5 in from each
/// face); the top front edge (y = 0, z = 20) chamfered 9 runs past the 5 mm between the edge
/// and the openings. The cube less its openings is 8000 - (3 2000 - 3 1000 + 1000) = 4000;
/// the chamfer's 40.5 section takes all 20 of its length where x < 5 or x > 15, and between
/// them only the material left above the y opening and before the z one (y < 5, z > 15,
/// z > 11 + y: 24.5): 4000 - 405 - 245 = 3350.
fn openings(k: &mut Kernel) -> (Brep, solidcore::Result<()>) {
    let cube = cuboid(k, [0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
    let x = cuboid(k, [-1.0, 5.0, 5.0], [22.0, 10.0, 10.0]);
    let b = boolean_difference(k, cube, x).unwrap();
    let y = cuboid(k, [5.0, -1.0, 5.0], [10.0, 22.0, 10.0]);
    let b = boolean_difference(k, b, y).unwrap();
    let z = cuboid(k, [5.0, 5.0, -1.0], [10.0, 10.0, 22.0]);
    let mut b = boolean_difference(k, b, z).unwrap();
    let built = blend(k, &mut b, &[[10.0, 0.0, 20.0]], 9.0, 0);
    (b, built)
}

#[test]
fn a_chamfer_running_into_openings_builds() {
    builds(openings, 3350.0, 1e-12);
}

/// A 40 x 30 x 10 block with a 5 x 5 x 3 notch at a bottom corner, its top outline rounded
/// R8: past the notch's 7 mm wall the round runs into the notch and is cut by it. The rig's
/// expected volume (SolidCraft `1080-fillet-notched-block-top-outline-r8`).
fn notched(k: &mut Kernel) -> (Brep, solidcore::Result<()>) {
    let block = cuboid(k, [0.0, 0.0, 0.0], [40.0, 30.0, 10.0]);
    let notch = cuboid(k, [35.0, -1.0, -1.0], [6.0, 6.0, 4.0]);
    let mut b = boolean_difference(k, block, notch).unwrap();
    let top = [
        [20.0, 0.0, 10.0],
        [40.0, 15.0, 10.0],
        [20.0, 30.0, 10.0],
        [0.0, 15.0, 10.0],
    ];
    let built = blend(k, &mut b, &top, 8.0, 1);
    (b, built)
}

#[test]
fn a_round_running_into_a_notch_builds() {
    builds(notched, 10198.718038815705, 1e-9);
}

/// A 40 x 20 x 10 block with a hole of radius 3 at (20, 14.5) cut by a cylinder longer
/// than the block; the top back edge (y = 20, z = 10) rounded R4, whose top rail at y = 16
/// runs over the hole. Closed form: 8000 - 90 pi - (16 - 4 pi) 40, plus the part of the
/// round's removed section (y > 16, z > 6 + sqrt(16 - (y - 16)^2)) inside the hole,
/// `int_16^17.5 2 sqrt(9 - (y - 14.5)^2) (4 - sqrt(16 - (y - 16)^2)) dy` (integrated to
/// 1e-12 below).
fn over_hole(k: &mut Kernel) -> (Brep, solidcore::Result<()>) {
    let block = cuboid(k, [0.0, 0.0, 0.0], [40.0, 20.0, 10.0]);
    let hole = post(k, [20.0, 14.5, -1.0], 3.0, 12.0);
    let mut b = boolean_difference(k, block, hole).unwrap();
    let built = blend(k, &mut b, &[[10.0, 20.0, 10.0]], 4.0, 1);
    (b, built)
}

/// Gauss-Legendre (5 points) on `n` equal panels.
fn integrate(f: impl Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let x = [
        0.0,
        0.538_469_310_105_683_1,
        -0.538_469_310_105_683_1,
        0.906_179_845_938_664,
        -0.906_179_845_938_664,
    ];
    let w = [
        0.568_888_888_888_888_9,
        0.478_628_670_499_366_5,
        0.478_628_670_499_366_5,
        0.236_926_885_056_189_1,
        0.236_926_885_056_189_1,
    ];
    let h = (b - a) / n as f64;
    (0..n)
        .map(|i| {
            let m = a + (i as f64 + 0.5) * h;
            x.iter()
                .zip(w)
                .map(|(&x, w)| w * f(m + 0.5 * h * x))
                .sum::<f64>()
                * 0.5
                * h
        })
        .sum()
}

#[test]
fn a_round_running_over_a_hole_builds() {
    // y = 17.5 - s^2 takes the square root at the hole's far side out of the integrand
    let overlap = integrate(
        |s| {
            let y = 17.5 - s * s;
            let chord = 2.0 * (9.0 - (y - 14.5) * (y - 14.5)).max(0.0).sqrt();
            let depth = 4.0 - (16.0 - (y - 16.0) * (y - 16.0)).max(0.0).sqrt();
            chord * depth * 2.0 * s
        },
        0.0,
        1.5_f64.sqrt(),
        200,
    );
    let want = 8000.0 - 90.0 * PI - (16.0 - 4.0 * PI) * 40.0 + overlap;
    // the rig's expected value, 7580.287315360674, is 3.5e-12 rel below it
    assert!(
        (want - 7580.287315360674).abs() <= 1e-10 * want,
        "closed form {want}"
    );
    // the round's B-spline intersection with the hole's wall leaves 3.4e-9 rel (the same
    // without the flag)
    builds(over_hole, want, 1e-8);
}

/// A 40 x 40 x 10 plate with a hole of radius 5 through it, its top rim rounded R12: the
/// round runs past the hole's wall below the plate and trims the bottom face round the
/// hole. In (rho, z) the section's centre is (17, -2): the round takes
/// `pi int_2^12 ((17 - sqrt(144 - w^2))^2 - 25) dw` from 16000 - 250 pi.
#[test]
fn a_round_of_a_hole_rim_past_the_plate_bottom_builds() {
    let s = 36.0 * PI - (140.0_f64.sqrt() + 72.0 * (1.0_f64 / 6.0).asin());
    let removed = PI * (2640.0 - 34.0 * s + 1440.0 - (1728.0 - 8.0) / 3.0);
    let want = 16000.0 - 250.0 * PI - removed;
    assert!(
        (want - 13726.857966844447).abs() < 1e-9,
        "closed form {want}"
    );
    builds(
        |k| {
            // the round on the continued wall; with `fillet_round_ends_on_outline` it ends on
            // the body's outline (`wr730_round_ends_on_outline.rs`)
            k.fixes.fillet_round_ends_on_outline = false;
            let plate = cuboid(k, [0.0, 0.0, 0.0], [40.0, 40.0, 10.0]);
            let hole = post(k, [20.0, 20.0, -1.0], 5.0, 12.0);
            let mut b = boolean_difference(k, plate, hole).unwrap();
            let built = blend(k, &mut b, &[[25.0, 20.0, 10.0]], 12.0, 1);
            (b, built)
        },
        want,
        1e-9,
    );
}

/// A disc of radius 13, 2 high, its top rim rounded R2.5: the round runs past the bottom of
/// its 2 mm wall and is cut by the bottom face. In (rho, z) the section's centre is
/// (10.5, -0.5): the round takes `pi int_0.5^2.5 (169 - (10.5 + sqrt(6.25 - w^2))^2) dw` from
/// 338 pi.
#[test]
fn a_round_of_a_disc_rim_past_its_wall_builds() {
    let s = 3.125 * PI / 2.0 - (0.5 * 6.0_f64.sqrt() / 2.0 + 3.125 * 0.2_f64.asin());
    let removed = PI * (58.75 * 2.0 - 21.0 * s - (12.5 - (15.625 - 0.125) / 3.0));
    let want = 338.0 * PI - removed;
    assert!(
        (want - 957.6922389452784).abs() < 1e-10,
        "closed form {want}"
    );
    builds(
        |k| {
            // the round on the continued wall; with `fillet_round_ends_on_outline` it ends on
            // the body's outline (`wr730_round_ends_on_outline.rs`)
            k.fixes.fillet_round_ends_on_outline = false;
            let mut b = post(k, [0.0, 0.0, 0.0], 13.0, 2.0);
            // a point of the rim away from the seam, which also passes through (13, 0, 2)
            let built = blend(k, &mut b, &[[0.0, 13.0, 2.0]], 2.5, 1);
            (b, built)
        },
        want,
        1e-9,
    );
}

/// #2299 stays refused: the round r7 of the outer top edge of a box shelled by 2 breaks
/// through the wall into the hollow behind, past the closed-form bound
/// 2 sqrt 2 / (sqrt 2 - 1) (`wr707_round_breaking_through.rs`), with the same fit.
#[test]
fn a_round_breaking_into_a_hollow_is_still_refused() {
    let mut k = kernel(true);
    let outer = cuboid(&mut k, [-52.0, -32.0, 0.0], [104.0, 64.0, 42.0]);
    let inner = cuboid(&mut k, [-50.0, -30.0, -1.0], [100.0, 60.0, 41.0]);
    let mut b = boolean_difference(&mut k, outer, inner).unwrap();
    let err = blend(&mut k, &mut b, &[[0.0, 32.0, 42.0]], 7.0, 1).expect_err("refused");
    let SolidError::FilletRefused { reason, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(reason, Some(NoRoomReason::BreaksThrough));
    let limit = 2.0 * 2.0_f64.sqrt() / (2.0_f64.sqrt() - 1.0);
    assert!((fit.size - (limit - 0.01)).abs() <= 1e-4, "{}", fit.size);
    // the fit the first version of the flag noted: the bound is unchanged
    assert!(
        (fit.size - 6.818444195803968).abs() <= 1e-12,
        "{}",
        fit.size
    );
}

/// A 40 x 20 x 10 block with a hole of radius 1 at (20, 18.2), its top back edge (y = 20,
/// z = 10) rounded R4. The hole lies between the round's rails (y 17.2 to 19.2 against
/// the top rail at y = 16), so the section passes through it and comes back into the part.
fn block_with_hole(k: &mut Kernel, z0: f64, height: f64) -> (Brep, solidcore::Result<()>) {
    let block = cuboid(k, [0.0, 0.0, 0.0], [40.0, 20.0, 10.0]);
    let hole = post(k, [20.0, 18.2, z0], 1.0, height);
    let mut b = boolean_difference(k, block, hole).unwrap();
    let built = blend(k, &mut b, &[[5.0, 20.0, 10.0]], 4.0, 1);
    (b, built)
}

/// The part of the round's removed section (z above `6 + sqrt(16 - (y - 16)^2)`) inside
/// the hole below `top`, from `y = 18.2 + sin theta` on (theta0, pi/2).
fn hole_overlap(top: f64, theta0: f64) -> f64 {
    integrate(
        |t: f64| {
            let y = 18.2 + t.sin();
            let low = 6.0 + (16.0 - (y - 16.0) * (y - 16.0)).sqrt();
            2.0 * t.cos() * t.cos() * (top - low)
        },
        theta0,
        PI / 2.0,
        400,
    )
}

/// Through a hole in the blended top face the section comes back into the part, but the
/// hole's wall bounds the top face: the round is trimmed to it and builds.
/// Closed form: 8000 - 10 pi - (16 - 4 pi) 40 plus the removed section inside the hole.
#[test]
fn a_round_over_a_through_hole_between_its_rails_builds() {
    let want = 8000.0 - 10.0 * PI - (16.0 - 4.0 * PI) * 40.0 + hole_overlap(10.0, -PI / 2.0);
    // the round's B-spline intersection with the hole's wall leaves 1.5e-9 rel
    builds(|k| block_with_hole(k, -1.0, 12.0), want, 1e-8);
}

/// A blind pocket from below, its ceiling at z = 9.5 under the blended top face: the
/// section breaks through the pocket's ceiling and wall, faces that share no edge with
/// the blended faces. Refused; without the flag the round builds without trimming them,
/// 0.83 short of the closed form 8000 - 9.5 pi - (16 - 4 pi) 40 plus the removed section
/// inside the pocket (above y = 16 + sqrt(16 - 3.5^2), where it reaches below 9.5).
#[test]
fn a_round_opening_a_blind_pocket_is_refused() {
    let y = 16.0 + (16.0_f64 - 3.5 * 3.5).sqrt();
    let want = 8000.0 - 9.5 * PI - (16.0 - 4.0 * PI) * 40.0 + hole_overlap(9.5, (y - 18.2).asin());
    let mut k = kernel(true);
    let (_, built) = block_with_hole(&mut k, -1.0, 10.5);
    let err = built.expect_err("refused");
    let SolidError::FilletRefused { reason, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(reason, Some(NoRoomReason::BreaksThrough));
    let mut k = kernel(false);
    let (mut b, built) = block_with_hole(&mut k, -1.0, 10.5);
    built.unwrap();
    let v = volume(&k, &mut b);
    assert!((v - want).abs() > 0.5, "{v} vs {want}");
}

/// An 80 x 5 x 5 bar, its long top edge (y = 0, z = 5) rounded R10, wider than both faces
/// it rounds (SolidCraft `4782-fillet-bar-edge-wider-than-both-faces`; fork 5e8c9c1 does not
/// build it yet). Both rails run off their faces and the section crosses the bar's far
/// faces only in pieces that reach a rail. In (y, z) the section keeps the bar's square
/// within the circle of centre (10, -5), radius 10: from y = 10 - sqrt 75, where the circle
/// leaves z = 0, the area under z = sqrt(100 - (y - 10)^2) - 5; V = 80 area.
#[test]
fn a_round_wider_than_both_its_faces_builds() {
    let area = integrate(
        |y| (100.0 - (y - 10.0) * (y - 10.0)).sqrt() - 5.0,
        10.0 - 75.0_f64.sqrt(),
        5.0,
        400,
    );
    let want = 80.0 * area;
    // the rig's 630.293125 is a grid quadrature
    assert!((want - 630.293125).abs() < 1e-3, "closed form {want}");
    builds(
        |k| {
            // the round on the continued wall; with `fillet_round_ends_on_outline` it ends on
            // the body's outline (`wr730_round_ends_on_outline.rs`)
            k.fixes.fillet_round_ends_on_outline = false;
            let mut b = cuboid(k, [0.0, 0.0, 0.0], [80.0, 5.0, 5.0]);
            let built = blend(k, &mut b, &[[40.0, 0.0, 5.0]], 10.0, 1);
            (b, built)
        },
        want,
        1e-9,
    );
}
