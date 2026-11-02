//! WR-696 (#2273, step 5 of the generic blend; the fork's tangency-labs/smlib#79, 410dfe1,
//! 20ba5a9, 8c40818, a34b5e8 and a3d6d81, merged at 5e8c9c1): with
//! `fixes.fillet_gn_section_generator` the rolling ball's local regularity is judged along
//! every blend before the corners (`SmFilletSolver::BallIsRegular`), and a blend that is not
//! regular is refused with `NoRoom`, naming the radius that works and noting the largest
//! size that builds ([`FilletSession::fit_size`]).
//!
//! Values: closed forms from the fork's capi tests (`ops_kernel.cpp` at 5e8c9c1), and the
//! fork's own numbers from a local build of 5e8c9c1's `SMLib` and `SM_API` run the same way
//! (`SetFilletParameters`, `CreateFilletCorners`, `DoFilleting`, `GetFitRadius`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn kernel(gn: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_gn_section_generator = gn;
    k.fixes = fixes;
    k
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

struct Refusal {
    comment: String,
    reason: Option<NoRoomReason>,
    fit: Option<(f64, FitKind, bool)>,
}

fn round(k: &mut Kernel, b: &mut Brep, at: Point3, r: f64) -> Result<(), Refusal> {
    let e = pick(k, b, at);
    let mut session = FilletSession::new(k, b).unwrap();
    session
        .prepare_constant_radius_edges(k, &[e], r, 1, 2, 0.0)
        .unwrap();
    session.do_fillet(k).map_err(|_| {
        let info = session.error_info();
        Refusal {
            comment: info.comment.clone(),
            reason: info.no_room_reason(),
            fit: session
                .fit_size()
                .map(|f| (f.size, f.kind, f.edge == Some(e))),
        }
    })
}

/// A round wider than the rim it rounds: a ball of r2 inside a pin of radius R < 2 (the
/// fork's capi test of 4ebf04e/a3d6d81). The side curves toward the ball by 1 / R, so the
/// ball fits while 1 - r / R > 0: refused naming R, and the largest radius that builds is R
/// itself (a closed bound: the hemispherical end). The fork's build gives 1.99, 1.5, 1 in
/// the reason and fits 1.9899999999999984, 1.4999999999999984, 0.99999999999999956.
#[test]
fn a_ball_wider_than_its_pin_is_refused_naming_the_pin() {
    for (pin, fork_fit) in [
        (1.99, 1.9899999999999984),
        (1.5, 1.4999999999999984),
        (1.0, 0.99999999999999956),
    ] {
        let mut k = kernel(true);
        let mut b = create_cylinder(&mut k, Point3::ZERO, pin, 10.0).unwrap();
        let refusal = round(&mut k, &mut b, Point3::new(0.0, pin, 10.0), 2.0).expect_err("refused");
        eprintln!("R {pin}: {} fit {:?}", refusal.comment, refusal.fit);
        assert_eq!(refusal.reason, Some(NoRoomReason::BallDoesNotFit));
        assert_eq!(
            refusal.comment,
            format!("Fillet Error: a face curves tighter than the rolling ball of radius 2, which does not fit it: a radius below {pin} fits")
        );
        let (fit, kind, on_edge) = refusal.fit.expect("a fit");
        assert_eq!(kind, FitKind::Radius);
        assert!(on_edge);
        assert!(
            (fit - pin).abs() <= 1e-9 * pin,
            "{fit} vs closed form {pin}"
        );
        assert!(
            (fit - fork_fit).abs() <= 1e-12 * pin,
            "{fit} vs fork {fork_fit}"
        );
    }
}

/// Without the flag the round is not refused by the ball's regularity: no fit is noted.
#[test]
fn without_the_flag_no_fit_is_noted() {
    let mut k = kernel(false);
    let mut b = create_cylinder(&mut k, Point3::ZERO, 1.5, 10.0).unwrap();
    if let Err(refusal) = round(&mut k, &mut b, Point3::new(0.0, 1.5, 10.0), 2.0) {
        assert!(refusal.fit.is_none());
        assert!(
            !refusal.comment.contains("a radius below"),
            "{}",
            refusal.comment
        );
    }
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// A pin of radius 2 standing on a plate whose top is the plane z = x tan 60 degrees, its
/// foot rounded (the fork's capi test of c11eeb6). The touch arc is progressive while
/// (R + r)^2 - r (R + r) / cos a > 0, zero at r = R cos a / (1 - cos a) = 2: r2.2 is refused
/// naming 2 (the fork's build: "a radius below 2 fits", fit 1.9899999999991815, the open
/// bound less a thousand edge tolerances of 1e-5), and r1.8 builds (the fork's volume
/// 32500.37689795169).
fn pin_on_a_slope(k: &mut Kernel) -> Brep {
    let a = std::f64::consts::PI / 3.0;
    let (ca, sa) = (a.cos(), a.sin());
    let mut slab = create_box(k, Point3::new(-20.0, -20.0, -20.0), 40.0, 40.0, 20.0).unwrap();
    let m = [
        [ca, 0.0, sa, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [-sa, 0.0, ca, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    transform_matrix(k, &mut slab, &m, true).unwrap();
    let pin = create_cylinder(k, Point3::new(0.0, 0.0, -30.0), 2.0, 60.0).unwrap();
    boolean_union(k, slab, pin).unwrap()
}

#[test]
fn a_round_at_a_pin_on_a_slope_folds_past_two() {
    let mut k = kernel(true);
    let mut b = pin_on_a_slope(&mut k);
    let refusal = round(&mut k, &mut b, Point3::new(0.0, 2.0, 0.0), 2.2).expect_err("refused");
    eprintln!("{} fit {:?}", refusal.comment, refusal.fit);
    assert_eq!(refusal.reason, Some(NoRoomReason::BallsFold));
    assert!(refusal.comment.starts_with("Fillet Error: the rolling ball of radius 2.2 folds over itself along the edge (its touch circles move back): a radius below 2 fits"), "{}", refusal.comment);
    let (fit, kind, on_edge) = refusal.fit.expect("a fit");
    assert_eq!(kind, FitKind::Radius);
    assert!(on_edge);
    assert!(
        (fit - (2.0 - 0.01)).abs() < 1e-9,
        "{fit} vs closed form 2 less the margin"
    );
    assert!((fit - 1.9899999999991815).abs() < 1e-9, "{fit} vs fork");

    let mut k = kernel(true);
    let mut b = pin_on_a_slope(&mut k);
    round(&mut k, &mut b, Point3::new(0.0, 2.0, 0.0), 1.8)
        .map_err(|r| r.comment)
        .unwrap();
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let v = volume(&k, &mut b);
    eprintln!("r1.8 volume {v}");
    assert!((v - 32500.37689795169).abs() <= 1e-7 * v, "{v}");
}
