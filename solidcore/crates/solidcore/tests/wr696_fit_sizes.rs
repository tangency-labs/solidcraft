//! WR-696 (#2273, step 5 of the generic blend; the fork's tangency-labs/smlib#79, a3d6d81,
//! 49f393d and 5e8c9c1): a blend refused as larger than its faces notes the largest size that builds: where its rails stay on their faces
//! (`LargestSizeOnItsFaces`, a ball or a chord through a boundary point, `BallThrough`),
//! where a band reaches a circular top's centre, and where two bands on a face meet
//! (`BandsMeetAt`). The public API carries it on the refusal
//! ([`SolidError::FilletRefused`]).
//!
//! Values: closed forms from the fork's capi tests (`ops_kernel.cpp` at 5e8c9c1), and the
//! fork's own numbers from a local build of 5e8c9c1's `SMLib` and `SM_API` run the same way
//! (`SetFilletParameters`, `CreateFilletCorners`, `DoFilleting`, `GetFitRadius`,
//! `GetFitKind`). The crease trim's refusal across a tangent edge (`NoteFitOnFace`, step 6's
//! `TrimAtCreases`): the fork's capi `scenario_round_onto_a_tangent_round_refused` at 2cf8964,
//! graded as there (no build of the fork compared).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel, SolidError};

fn kernel() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    }
}

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
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

#[derive(Debug)]
struct Refusal {
    comment: String,
    reason: Option<NoRoomReason>,
    fit: Option<(f64, FitKind, bool)>,
}

/// `xsect` 1 a round, 0 a chamfer; `distance` a constant-distance (chord) blend.
fn blend(
    k: &mut Kernel,
    b: &mut Brep,
    at: &[Point3],
    r: f64,
    xsect: u32,
    distance: bool,
) -> Result<(), Refusal> {
    let edges: Vec<EdgeId> = at.iter().map(|&q| pick(k, b, q)).collect();
    let mut session = FilletSession::new(k, b).unwrap();
    if distance {
        session
            .prepare_per_edge_constant_distance(
                k,
                &edges,
                &[r],
                &[FilletSurfaceGeneratorType::Circular],
                2,
                0.0,
            )
            .unwrap();
    } else {
        session
            .prepare_constant_radius_edges(k, &edges, r, xsect, 2, 0.0)
            .unwrap();
    }
    session.do_fillet(k).map_err(|_| {
        let info = session.error_info();
        Refusal {
            comment: info.comment.clone(),
            reason: info.no_room_reason(),
            fit: session
                .fit_size()
                .map(|f| (f.size, f.kind, f.edge.is_some_and(|e| edges.contains(&e)))),
        }
    })
}

fn check(refusal: &Refusal, reason: NoRoomReason, kind: FitKind, closed: f64, fork: f64) {
    eprintln!("{refusal:?}");
    assert_eq!(refusal.reason, Some(reason), "{}", refusal.comment);
    let (fit, got_kind, on_edge) = refusal.fit.expect("a fit");
    assert_eq!(got_kind, kind);
    assert!(on_edge);
    assert!(
        (fit - closed).abs() <= 1e-9 * closed,
        "{fit} vs closed form {closed}"
    );
    assert!((fit - fork).abs() <= 1e-12 * fork, "{fit} vs fork {fork}");
}

/// The concave L: a floor 20 x 20 x 2 and a wall 2 thick, 10 high, at x = 0.
fn l_wall(k: &mut Kernel) -> Brep {
    let floor = create_box(k, Point3::ZERO, 20.0, 20.0, 2.0).unwrap();
    let wall = create_box(k, Point3::ZERO, 2.0, 20.0, 10.0).unwrap();
    boolean_union(k, floor, wall).unwrap()
}

/// The L's inner edge rounded 9: its rail on the wall at z = 2 + r leaves the wall's top
/// past r = 8 (open): fit 8 less a thousand edge tolerances of 1e-5. The fork's build:
/// 7.9900000000000002.
#[test]
fn a_concave_round_past_a_wall_fits_where_its_rail_reaches_the_top() {
    let mut k = kernel();
    let mut b = l_wall(&mut k);
    let refusal = blend(
        &mut k,
        &mut b,
        &[Point3::new(2.0, 10.0, 2.0)],
        9.0,
        1,
        false,
    )
    .expect_err("refused");
    check(
        &refusal,
        NoRoomReason::ConcaveOverflow,
        FitKind::Radius,
        8.0 - 0.01,
        7.9900000000000002,
    );
}

/// The L's inner edge blended at a chord width 13: the chord of a ball of radius r in the
/// square corner is r sqrt 2, its rail reaches the wall's top at r = 8: a width of 8 sqrt 2
/// (open). The fork's build: 11.303708498984761, kind `SM_FIT_CHORD`.
#[test]
fn a_chord_past_a_wall_fits_at_the_chord_of_the_ball_reaching_the_top() {
    let mut k = kernel();
    let mut b = l_wall(&mut k);
    let refusal = blend(
        &mut k,
        &mut b,
        &[Point3::new(2.0, 10.0, 2.0)],
        13.0,
        1,
        true,
    )
    .expect_err("refused");
    check(
        &refusal,
        NoRoomReason::ConcaveOverflow,
        FitKind::Chord,
        8.0 * 2.0_f64.sqrt() - 0.01,
        11.303708498984761,
    );
}

/// A can (r10, h20) whose top rim is chamfered 10: the cone takes the whole top. The band
/// reaches the top's centre at the ball touching it there and the side, 10 (open). The
/// fork's build: 9.9899999999999967.
#[test]
fn a_chamfer_taking_a_can_top_fits_where_its_band_reaches_the_centre() {
    let mut k = kernel();
    let mut b = create_cylinder(&mut k, Point3::ZERO, 10.0, 20.0).unwrap();
    let refusal = blend(
        &mut k,
        &mut b,
        &[Point3::new(
            10.0 * 100.0_f64.to_radians().cos(),
            10.0 * 100.0_f64.to_radians().sin(),
            20.0,
        )],
        10.0,
        0,
        false,
    )
    .expect_err("refused");
    check(
        &refusal,
        NoRoomReason::FaceConsumed,
        FitKind::Radius,
        10.0 - 0.01,
        9.9899999999999967,
    );
}

/// The bossed can (r7 h48.2, a boss r2.7 on its top) with its top rim rounded 6.9: the
/// rail on the top, a circle of radius 7 - r, reaches the boss's footprint at r = 4.3
/// (open). The fork's build: 4.2899999999999938.
#[test]
fn a_round_past_a_boss_footprint_fits_where_its_rail_reaches_it() {
    let mut k = kernel();
    let can = create_cylinder(&mut k, Point3::ZERO, 7.0, 48.2).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(0.0, 0.0, 48.2), 2.7, 2.0).unwrap();
    let mut b = boolean_union(&mut k, can, boss).unwrap();
    let at = 100.0_f64.to_radians();
    let rim = Point3::new(7.0 * at.cos(), 7.0 * at.sin(), 48.2);
    let refusal = blend(&mut k, &mut b, &[rim], 6.9, 1, false).expect_err("refused");
    check(
        &refusal,
        NoRoomReason::RoundIntoMaterial,
        FitKind::Radius,
        4.3 - 0.01,
        4.2899999999999938,
    );
}

/// Both vertical edges of the 15 wide end of an 80 x 15 x 20 box rounded 7.6: past 7.5
/// each rail lies inside the other's band. The bands meet at 7.5 (closed: a full round).
/// The fork's build: 7.4999999999997264.
#[test]
fn two_rounds_on_a_narrow_end_fit_where_their_bands_meet() {
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::ZERO, 80.0, 15.0, 20.0).unwrap();
    let refusal = blend(
        &mut k,
        &mut b,
        &[Point3::new(80.0, 0.0, 10.0), Point3::new(80.0, 15.0, 10.0)],
        7.6,
        1,
        false,
    )
    .expect_err("refused");
    eprintln!("{refusal:?}");
    assert_eq!(refusal.reason, Some(NoRoomReason::BandsOverlap));
    let (fit, kind, on_edge) = refusal.fit.expect("a fit");
    assert_eq!(kind, FitKind::Radius);
    assert!(on_edge);
    assert!((fit - 7.5).abs() <= 1e-9 * 7.5, "{fit} vs closed form 7.5");
    assert!(
        (fit - 7.4999999999997264).abs() <= 1e-12 * 7.5,
        "{fit} vs fork"
    );
}

/// `fillet_edges` carries the fit on its refusal: the bridged block's round r9 (part 2's
/// `BandCutsThePart`), fit 7.8184271464117412 as the fork's build gives it. The bound behind
/// that number: the band takes all of the bridge where `d(r) = r - sqrt(2 r - 1) = 4`, that
/// is `r = 5 + 2 sqrt 2`, less a thousand edge tolerances of 1e-5. That is the bound
/// `wr696_band_cuts.rs` derives for the same body, edge and size, graded to the same 1e-8.
#[test]
fn fillet_edges_returns_the_fit_on_the_refusal() {
    let mut k = kernel();
    // the round on the continued top; with `fillet_round_ends_on_outline` it ends on the
    // body's outline and keeps the bridge (`wr730_round_ends_on_outline.rs`)
    k.fixes.fillet_round_ends_on_outline = false;
    let block = create_box(&mut k, Point3::ZERO, 20.0, 4.0, 10.0).unwrap();
    let slot = create_box(&mut k, Point3::new(8.0, -1.0, -1.0), 4.0, 6.0, 10.0).unwrap();
    let mut b = boolean_difference(&mut k, block, slot).unwrap();
    let e = pick(&k, &b, Point3::new(2.0, 0.0, 10.0));
    let err = fillet_edges(&mut k, &mut b, &[e], 9.0, 1, 2, 0.0).expect_err("refused");
    // the refusal's status is unchanged: SM_ERR
    assert_eq!(err.sm_status(), Some(1001));
    let SolidError::FilletRefused { reason, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(reason, Some(NoRoomReason::CutsThePart));
    assert_eq!(fit.kind, FitKind::Radius);
    assert_eq!(fit.edge, Some(e));
    let closed = 5.0 + 2.0 * 2.0_f64.sqrt() - 0.01;
    assert!(
        (fit.size - closed).abs() <= 1e-8 * closed,
        "{} vs closed form {closed}",
        fit.size
    );
    assert!(
        (fit.size - 7.8184271464117412).abs() <= 1e-12 * fit.size,
        "{}",
        fit.size
    );
}

/// The three edges at a 20 cube's corner rounded 20 (the fork's capi test
/// `scenario_cube_corner_rounds_as_wide_as_faces_refused`): the corner takes each edge's
/// whole length; the corner takes `r <u, e> = r` of it, so the largest radius that keeps a
/// length is 20 / 1 (open), less the margin. The fork's build: 19.989999999999998.
#[test]
fn rounds_whose_corner_takes_their_edges_fit_at_the_edge_length() {
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let refusal = blend(
        &mut k,
        &mut b,
        &[
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
            Point3::new(0.0, 0.0, 10.0),
        ],
        20.0,
        1,
        false,
    )
    .expect_err("refused");
    check(
        &refusal,
        NoRoomReason::CornersTakeWholeEdge,
        FitKind::Radius,
        20.0 - 0.01,
        19.989999999999998,
    );
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

fn lumps(b: &Brep) -> usize {
    let infinite = b.infinite_region();
    regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count()
}

const BACK: Point3 = Point3::new(10.0, 10.0, 10.0);
const FRONT: Point3 = Point3::new(10.0, 0.0, 10.0);

/// A 20 x 10 x 10 block, its back top edge rounded R3 (`fillet_edges`, as the capi's
/// `smlib_fillet`): its top is flat for 0 <= y <= 7, then runs on tangentially into the round.
fn backed_block(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::ZERO, 20.0, 10.0, 10.0).unwrap();
    let back = pick(k, &b, BACK);
    fillet_edges(k, &mut b, &[back], 3.0, 1, 2, 0.0).unwrap();
    b
}

/// The fork's `scenario_round_onto_a_tangent_round_refused` (capi `ops_kernel.cpp`, 2cf8964):
/// the backed block's front top edge rounded R9. Its rail on the top (y = r) runs off the
/// top's flat across the tangent edge (y = 7) onto the back round, which it would cut: the
/// crease trim refuses it (`CreaseAcrossTangentEdge`) and notes where its rails stay on the
/// top (`NoteFitOnFace`), 7 (open), less the fit margin. The fork grades it just under 7, by
/// at most two margins; here the margin is the front edge's own. Built there, the two rounds
/// take 20 (1 - pi/4) (9 + r^2) off the block (to 1e-6, as the fork), one solid.
#[test]
fn a_round_onto_a_tangent_round_fits_where_its_rail_stays_on_the_top() {
    let mut k = kernel();
    let mut b = backed_block(&mut k);
    let front = pick(&k, &b, FRONT);
    let margin = 1000.0 * edge::tolerance(&b, front).unwrap();
    let err = fillet_edges(&mut k, &mut b, &[front], 9.0, 1, 2, 0.0).expect_err("refused");
    let SolidError::FilletRefused { reason, fit, .. } = err else {
        panic!("{err:?}");
    };
    eprintln!("reason {reason:?}, fit {fit:?}, margin {margin}");
    assert_eq!(reason, Some(NoRoomReason::CreaseAcrossTangentEdge));
    assert_eq!(fit.kind, FitKind::Radius);
    assert_eq!(fit.edge, Some(front));
    // the fork's `just_under(fit, 7.0)`
    assert!(fit.size < 7.0 && fit.size > 7.0 - 0.02, "{}", fit.size);
    let closed = 7.0 - margin;
    assert!(
        (fit.size - closed).abs() <= 1e-9 * closed,
        "{} vs closed form {closed}",
        fit.size
    );

    let mut k = kernel();
    let mut b = backed_block(&mut k);
    let front = pick(&k, &b, FRONT);
    fillet_edges(&mut k, &mut b, &[front], fit.size, 1, 2, 0.0)
        .unwrap_or_else(|e| panic!("at the fit {}: {e:?}", fit.size));
    assert_eq!(lumps(&b), 1, "one solid");
    let v = volume_of(&k, &mut b);
    let want = 2000.0 - 20.0 * (1.0 - PI / 4.0) * (9.0 + fit.size * fit.size);
    eprintln!("volume {v:.12}, want {want:.12}");
    assert!(
        (v - want).abs() <= 1e-6 * want,
        "volume {v:.12}, want {want:.12}"
    );
}
