//! WR-730 (#2541): with `fixes.fillet_round_ends_on_outline`, a round bigger than the wall it
//! rolls on ends on the body's outline: the ball stays tangent to the face it keeps and its
//! section passes through the wall's rim, where the cliff overflow builds it on the wall
//! continued past the rim and cuts it from the body, taking the plate under the rim.
//!
//! The plate is 100 x 100 x 10, its top edge at x = 0 rounded r (L = 100, t = 10). Every
//! expected value is closed form, from the section:
//! - up to r = (L² + t²) / 2t = 505 the ball touches the top face at
//!   xc = sqrt(r² - (r - t)²) and passes through the bottom edge (0, 0): the material above
//!   that arc goes;
//! - past it the ball touches the far top edge (L, t) and the bottom edge: the material
//!   above the arc through them goes;
//! - without the flag, the ball touches the top face at x = r and the wall continued below
//!   the plate at z = t - r, and the material above that arc goes.
//!
//! The 2 mm disc wall (R13 x 2, r 2.5) is the plate's section turned about the disc's axis,
//! by Pappus. The cases the construction leaves alone (rig `fillets/5158`, `5236`, a round
//! breaking into a hollow) keep their results with the flag.

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
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::p5::DropPointOptions;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{face, EdgeId};
use solidcore::{Brep, Kernel, SolidError};
use std::f64::consts::PI;

/// The plate's length across the round, its thickness, and its length along the edge.
const L: f64 = 100.0;
const T: f64 = 10.0;
const W: f64 = 100.0;

/// Every flag on, as SolidCraft's backend runs blends (`fillet_refuse_rounds_breaking_through`
/// off, its preset), with `outline` as this construction's flag.
fn kernel(outline: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_refuse_rounds_breaking_through = false;
    fixes.fillet_round_ends_on_outline = outline;
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

/// A round (`fillet_edges` with SolidCraft's arguments: G1, thumbweight 1) of the edge
/// through `at`.
fn round(k: &mut Kernel, b: &mut Brep, at: [f64; 3], r: f64) -> solidcore::Result<()> {
    let e = pick(k, b, at);
    fillet_edges(k, b, &[e], r, 1, 1, 1.0)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// `∫ sqrt(r² - u²) du` from 0 to `u`.
fn quarter(u: f64, r: f64) -> f64 {
    (u * (r * r - u * u).sqrt() + r * r * (u / r).asin()) / 2.0
}

/// The regimes' bound, r = (L² + t²) / 2t: the ball touching the top face and the bottom
/// edge touches the top face at its far edge.
fn bound() -> f64 {
    (L * L + T * T) / (2.0 * T)
}

/// The plate up to the bound: the ball of centre (xc, t - r), xc = sqrt(r² - (r - t)²),
/// touches the top face at xc and passes through (0, 0); the section loses the region above
/// its arc, xc t - (xc (t - r) + ∫ from -xc to 0 of sqrt(r² - u²) du).
fn tangent_to_top(r: f64) -> f64 {
    let xc = (r * r - (r - T) * (r - T)).sqrt();
    let lost = xc * T - (xc * (T - r) + quarter(xc, r));
    L * T * W - W * lost
}

/// The plate past the bound: the ball through (0, 0) and (L, t), its centre away from the
/// edge; the section loses the triangle over their chord less the circle's segment on it.
fn through_both_edges(r: f64) -> f64 {
    let chord = (L * L + T * T).sqrt();
    let angle = 2.0 * (chord / (2.0 * r)).asin();
    let segment = r * r / 2.0 * (angle - angle.sin());
    L * T * W - W * (L * T / 2.0 - segment)
}

/// The plate without the flag: the ball of centre (r, t - r) on the wall continued below
/// the plate, crossing the bottom at xm = r - sqrt(r² - (r - t)²): the section loses the
/// whole thickness up to xm and the region above the arc from there to x = r.
fn on_continued_wall(r: f64) -> f64 {
    let xm = r - (r * r - (r - T) * (r - T)).sqrt();
    let lost = T * xm + r * (r - xm) - quarter(r - xm, r);
    L * T * W - W * lost
}

fn plate(k: &mut Kernel) -> Brep {
    create_box(k, Point3::new(0.0, 0.0, 0.0), L, W, T).unwrap()
}

/// One lump, a manifold solid, `check_brep` clean at Level2, `faces` faces and the volume
/// `want` to `rel`.
fn holds(k: &mut Kernel, b: &mut Brep, faces: usize, want: f64, rel: f64) {
    let infinite = b.infinite_region();
    let lumps = regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count();
    assert_eq!(lumps, 1, "one lump");
    assert!(is_manifold_solid(b).unwrap(), "a manifold solid");
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(check.lines().is_empty(), "check_brep: {:?}", check.lines());
    assert_eq!(b.faces().ids().count(), faces, "faces");
    let v = volume(k, b);
    assert!((v - want).abs() <= rel * want, "{v} vs {want}");
}

/// The round's face: the one face not on a plane of the plate's.
fn round_face(k: &Kernel, b: &Brep) -> solidcore::topology::FaceId {
    let mut found = None;
    for f in b.faces().ids() {
        let s = face::p2::surface(b, f).unwrap().unwrap();
        if !s.is_planar(k, 1e-9).unwrap().is_planar {
            assert!(found.is_none(), "one round face");
            found = Some(f);
        }
    }
    found.expect("a round face")
}

/// The round's surface at its point nearest `p`: the point's distance and the unit normal.
fn on_round(k: &Kernel, b: &Brep, p: Point3) -> (f64, Vec3) {
    let f = round_face(k, b);
    let s = face::p2::surface(b, f).unwrap().unwrap();
    let domain = s.natural_uv_domain_with_nurb(k).unwrap();
    let dropped = s
        .drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())
        .unwrap();
    assert!(dropped.success);
    let n = s
        .evaluate_normal(
            k,
            dropped.result_uv,
            solidcore::nurbs::knots::Side::Left,
            solidcore::nurbs::knots::Side::Left,
        )
        .unwrap();
    (dropped.gap, n.unitize().unwrap())
}

/// The plate up to the bound: the closed form to 1e-9, six faces (the wall is gone), the
/// round tangent to the top face along x = xc (normals within 1e-9 rad) and through the
/// bottom edge.
#[test]
fn a_round_up_to_the_bound_is_tangent_to_the_top_and_ends_on_the_bottom_edge() {
    assert!((bound() - 505.0).abs() < 1e-12);
    for r in [20.0, 50.0, 90.0, 100.0, 154.7, 200.0, 400.0, 500.0] {
        let mut k = kernel(true);
        let mut b = plate(&mut k);
        round(&mut k, &mut b, [0.0, 50.0, T], r).unwrap();
        holds(&mut k, &mut b, 6, tangent_to_top(r), 1e-9);
        let xc = (r * r - (r - T) * (r - T)).sqrt();
        for y in [10.0, 50.0, 90.0] {
            // tangent to the top face where it meets it
            let (gap, n) = on_round(&k, &b, Point3::new(xc, y, T));
            assert!(
                gap <= 1e-9 * r,
                "r {r}: tangent line off the round by {gap}"
            );
            let angle = n.cross(Vec3::new(0.0, 0.0, 1.0)).length().asin();
            assert!(angle <= 1e-9, "r {r}: normals {angle} rad apart");
            // through the bottom edge
            let (gap, _) = on_round(&k, &b, Point3::new(0.0, y, 0.0));
            assert!(gap <= 1e-9 * r, "r {r}: bottom edge off the round by {gap}");
        }
    }
}

/// Past the bound the ball touches both edges: the closed form to 1e-9, five faces (the top
/// face and the wall are gone), the round through the bottom edge and the far top edge.
#[test]
fn a_round_past_the_bound_runs_from_the_bottom_edge_to_the_far_top_edge() {
    // the two regimes meet at the bound
    let b505 = bound();
    assert!((tangent_to_top(b505) - through_both_edges(b505)).abs() <= 1e-9 * L * T * W);
    for r in [570.0, 600.0] {
        let mut k = kernel(true);
        let mut b = plate(&mut k);
        round(&mut k, &mut b, [0.0, 50.0, T], r).unwrap();
        holds(&mut k, &mut b, 5, through_both_edges(r), 1e-9);
        for y in [10.0, 50.0, 90.0] {
            for p in [Point3::new(0.0, y, 0.0), Point3::new(L, y, T)] {
                let (gap, _) = on_round(&k, &b, p);
                assert!(gap <= 1e-9 * r, "r {r}: edge off the round by {gap}");
            }
        }
    }
}

/// Without the flag the round is built on the wall continued below the plate, as before.
#[test]
fn without_the_flag_the_round_is_on_the_continued_wall() {
    // (from r = L the rail on the top reaches its far edge and the top face goes)
    for r in [20.0, 50.0, 90.0] {
        let mut k = kernel(false);
        let mut b = plate(&mut k);
        round(&mut k, &mut b, [0.0, 50.0, T], r).unwrap();
        holds(&mut k, &mut b, 6, on_continued_wall(r), 1e-9);
    }
}

/// The disc R13 x 2, its top rim rounded r 2.5 (rig `190-fillet-disc-rim-past-its-2mm-wall`):
/// the plate's section turned about the axis. With the flag the ball touches the top at
/// sqrt(2rt - t²) = sqrt 6 inside the rim and passes through the bottom rim; the solid loses
/// 2π ∫ ρ dA over the region above that arc (`w` from the tangent point):
/// (R - xc)(r xc - ∫₀^xc sqrt(r² - w²) dw) + r xc²/2 + ((r - t)³ - r³)/3.
/// Without it, 957.692239 (`wr707_rounds_running_off_their_faces.rs`).
#[test]
fn the_disc_wall_ends_on_its_bottom_rim() {
    let (big, t, r) = (13.0_f64, 2.0_f64, 2.5_f64);
    let xc = (r * r - (r - t) * (r - t)).sqrt();
    let lost = (big - xc) * (r * xc - quarter(xc, r))
        + r * xc * xc / 2.0
        + ((r - t).powi(3) - r.powi(3)) / 3.0;
    let want = PI * big * big * t - 2.0 * PI * lost;
    assert!((want - 965.536_758_055_394).abs() < 1e-9, "{want}");
    let mut k = kernel(true);
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), big, t).unwrap();
    round(&mut k, &mut b, [0.0, big, t], r).unwrap();
    holds(&mut k, &mut b, 3, want, 1e-9);

    let s = 3.125 * PI / 2.0 - (0.5 * 6.0_f64.sqrt() / 2.0 + 3.125 * 0.2_f64.asin());
    let removed = PI * (58.75 * 2.0 - 21.0 * s - (12.5 - (15.625 - 0.125) / 3.0));
    let continued = 338.0 * PI - removed;
    let mut k = kernel(false);
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), big, t).unwrap();
    round(&mut k, &mut b, [0.0, big, t], r).unwrap();
    holds(&mut k, &mut b, 3, continued, 1e-9);
}

/// The volume and face count of a build, or its error.
fn outcome(k: &mut Kernel, mut b: Brep, at: [f64; 3], r: f64) -> Result<(f64, usize), String> {
    match round(k, &mut b, at, r) {
        Ok(()) => Ok((volume(k, &mut b), b.faces().ids().count())),
        Err(e) => Err(format!("{e:?}")),
    }
}

/// Rig `fillets/5158`: #1467's 40 x 20 x 10 box, its top edge on the 10 mm face rounded
/// r 10.01. The round on the continued wall passes 5e-6 inside the bottom edge, within the
/// body's tolerance: it does not take the material under the rim, and the flag leaves it as
/// it is, bit for bit.
#[test]
fn a_round_a_hair_past_its_wall_is_left_as_it_is() {
    let r = 10.01_f64;
    // the continued wall's ball passes inside the rim by less than the tolerance
    let inside = (r * r + (r - 10.0) * (r - 10.0)).sqrt() - r;
    assert!(inside < 1e-5, "{inside}");
    let [a, b] = [true, false].map(|flag| {
        let mut k = kernel(flag);
        let body = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 20.0, 10.0).unwrap();
        outcome(&mut k, body, [20.0, 0.0, 10.0], r)
    });
    assert_eq!(a, b);
}

/// Rig `fillets/5236`: the end rim of a cylinder r 10 rounded r 10, a hemispherical end. The
/// end face has no rim (its only edge is the filleted one): the flag leaves the result as it
/// is, bit for bit.
#[test]
fn a_round_taking_a_whole_end_face_is_left_as_it_is() {
    let [a, b] = [true, false].map(|flag| {
        let mut k = kernel(flag);
        let body = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 30.0).unwrap();
        outcome(&mut k, body, [0.0, 10.0, 30.0], 10.0)
    });
    assert_eq!(a, b);
    // the hemispherical end: the cylinder to 20 and the half ball
    let (v, faces) = a.expect("built");
    let want = PI * 100.0 * 20.0 + 2.0 / 3.0 * PI * 1000.0;
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    assert_eq!(faces, 3);
}

/// #2299: R7 on the outer top edge of a box shelled by 2 breaks through the roof into the
/// hollow behind: the wall's rim is 42 below, past the rail, so the round does not end on
/// the outline, and it is still refused with the fit of `wr707_round_breaking_through.rs`.
#[test]
fn a_round_breaking_into_a_hollow_is_still_refused() {
    let mut k = kernel(true);
    k.fixes.fillet_refuse_rounds_breaking_through = true;
    let outer = create_box(&mut k, Point3::new(-52.0, -32.0, 0.0), 104.0, 64.0, 42.0).unwrap();
    let inner = create_box(&mut k, Point3::new(-50.0, -30.0, -1.0), 100.0, 60.0, 41.0).unwrap();
    let mut b = boolean_difference(&mut k, outer, inner).unwrap();
    let err = round(&mut k, &mut b, [0.0, 32.0, 42.0], 7.0).expect_err("refused");
    let SolidError::FilletRefused { reason, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(reason, Some(NoRoomReason::BreaksThrough));
    let limit = 2.0 * 2.0_f64.sqrt() / (2.0_f64.sqrt() - 1.0);
    assert!((fit.size - (limit - 0.01)).abs() <= 1e-4, "{}", fit.size);
}

/// A 40 x 40 x 10 plate with a hole of radius 5 through it, its top rim rounded R12 (rig
/// `every-hole-top-rim-r12-past-plate-bottom`): the disc's section turned the other way, the
/// material outside the hole. The ball touches the top at ρc = 5 + xc,
/// xc = sqrt(2 r t - t²) = sqrt 140, and passes through the bottom rim; the solid loses
/// 2π (ρc (r xc - ∫₀^xc sqrt(r² - w²) dw) - r xc²/2 - ((r - t)³ - r³)/3). Without the
/// flag, 13726.857967 (`wr707_rounds_running_off_their_faces.rs`).
#[test]
fn a_hole_rim_ends_on_the_hole_s_bottom_rim() {
    let (r, t) = (12.0_f64, 10.0_f64);
    let xc = (r * r - (r - t) * (r - t)).sqrt();
    let rc = 5.0 + xc;
    let lost =
        rc * (r * xc - quarter(xc, r)) - r * xc * xc / 2.0 - ((r - t).powi(3) - r.powi(3)) / 3.0;
    let want = 16000.0 - 250.0 * PI - 2.0 * PI * lost;
    assert!((want - 13_811.173_028_388_774).abs() < 1e-8, "{want}");
    let mut k = kernel(true);
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 10.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(20.0, 20.0, -1.0), 5.0, 12.0).unwrap();
    let mut b = boolean_difference(&mut k, plate, hole).unwrap();
    round(&mut k, &mut b, [25.0, 20.0, 10.0], r).unwrap();
    // the plate's four sides, top and bottom, and the round (the hole's wall is gone)
    holds(&mut k, &mut b, 7, want, 1e-9);
}

/// An 80 x 5 x 5 bar, its long top edge rounded R10 (rig
/// `4782-fillet-bar-edge-wider-than-both-faces`): wider than both faces, the ball touching
/// one rim would land past the other face's rim, so it touches both, the far top edge
/// (y, z) = (5, 5) and the bottom edge (0, 0). The section loses the triangle over their
/// chord less the circle's segment on it: V = 80 (25 - 12.5 + segment). Without the flag,
/// the round on both faces continued (`wr707_rounds_running_off_their_faces.rs`).
#[test]
fn a_round_wider_than_both_its_faces_ends_on_both_rims() {
    let r = 10.0_f64;
    let chord = 50.0_f64.sqrt();
    let angle = 2.0 * (chord / (2.0 * r)).asin();
    let segment = r * r / 2.0 * (angle - angle.sin());
    let want = 80.0 * (25.0 - 12.5 + segment);
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 80.0, 5.0, 5.0).unwrap();
    round(&mut k, &mut b, [40.0, 0.0, 5.0], r).unwrap();
    holds(&mut k, &mut b, 5, want, 1e-9);
}

/// `∫₀^t (r - sqrt(r² - u²)) du`: the region between a face, the round's arc tangent to it
/// and the line square to the face through the arc's point `t` along.
fn corner_area(t: f64, r: f64) -> f64 {
    r * t - quarter(t, r)
}

/// The bridged block of `wr696_band_cuts.rs`: 20 x 4 x 10, a slot 4 wide (x 8 to 12) cut
/// from below to 1 under its top, the top front edge (y = 0, z = 10) rounded r past the 4 mm
/// top. The ball stays tangent to the front face, at zt = 10 - sqrt(r² - (r - 4)²), and
/// passes through the top's back edge (4, 10). The full section loses
/// `corner_area(10 - zt)`, the bridge (z 9 to 10) that less `corner_area(9 - zt)`: over 16
/// and 4 of the length. The bridge keeps a strip, so the part stays whole and nothing is
/// refused, at r 6 and at r 9 (which the round on the continued top cuts through).
#[test]
fn a_round_past_the_top_of_a_bridge_ends_on_its_back_edge() {
    for r in [6.0_f64, 9.0] {
        let q = (r * r - (r - 4.0) * (r - 4.0)).sqrt();
        let full = corner_area(q, r);
        let bridge = full - corner_area(q - 1.0, r);
        assert!(bridge < 4.0, "r {r}: the bridge keeps {}", 4.0 - bridge);
        let want = 656.0 - (16.0 * full + 4.0 * bridge);
        let mut k = kernel(true);
        let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 4.0, 10.0).unwrap();
        let slot = create_box(&mut k, Point3::new(8.0, -1.0, -1.0), 4.0, 6.0, 10.0).unwrap();
        let mut b = boolean_difference(&mut k, block, slot).unwrap();
        round(&mut k, &mut b, [2.0, 0.0, 10.0], r).unwrap();
        // the outside and one solid
        assert_eq!(regions(&b, None).unwrap().len(), 2, "r {r}: one piece");
        assert!(is_manifold_solid(&b).unwrap(), "r {r}");
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.lines().is_empty(), "r {r}: {:?}", check.lines());
        let v = volume(&k, &mut b);
        assert!((v - want).abs() <= 1e-9 * want, "r {r}: {v} vs {want}");
    }
}

/// The arc-ended box of `issue_2401_refusal_sizes.rs` (10 x 20 x 30, its end y = 20 cut by a
/// vertical cylinder of radius 12 about (5, 30)), its top edge x = 10, z = 30 rounded past
/// the 10 mm top. The round would end on the cylindrical end, which no plane cuts it by: it
/// is left to the cliff overflow, which refuses it as before (`CliffEndFaceNotPlanar`).
#[test]
fn a_round_ending_on_a_curved_face_is_left_as_it_is() {
    for r in [12.0, 10.01] {
        let [a, b] = [true, false].map(|flag| {
            let mut k = kernel(flag);
            let body = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
            let c = create_cylinder(&mut k, Point3::new(5.0, 30.0, -1.0), 12.0, 32.0).unwrap();
            let body = boolean_difference(&mut k, body, c).unwrap();
            outcome(&mut k, body, [10.0, 9.0, 30.0], r)
        });
        assert!(a.is_err(), "r {r} refused: {a:?}");
        assert_eq!(a, b, "r {r}");
    }
}
