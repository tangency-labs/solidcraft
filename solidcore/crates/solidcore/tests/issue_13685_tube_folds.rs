//! Regression tests for SolidCraft #13685: a rounded offset caps a convex edge with the tube
//! of radius `d` about it, `c + d u` for `u` on the arc between the two faces' normals turned
//! to the offset's side, and that tube folds over itself where `d (u . k) >= 1`, `k` the
//! edge's curvature vector. Built anyway, the fold leaves an unsound body that the offset
//! returned as a success. It is now refused at construction, naming the edge and the largest
//! distance at which its tube is proved not to fold (`OffsetFailure::TubeFolds`).
//!
//! The closed form: a circle of radius `rho` has `k` of length `1 / rho` pointing at its
//! centre, so the tube folds iff `d max over the arc of cos(theta) >= rho`, `theta` the angle
//! between `u` and the direction to the centre: `d_max = rho / max cos(theta)`, which is `rho`
//! where the arc holds that direction and infinite where every `u` faces away from it.
//!
//! The part: a washer revolved about Z from the profile (2, -1), (4, -1), (4, 1), (2, 1),
//! (1, 0) in XZ: its hole is two cones meeting at an inner ridge, a circle of radius
//! `RHO = 1` at z = 0. The ridge is convex, and the arc between its faces' normals
//! (-1, +-1) / sqrt 2 holds -X, the direction to the axis, so its outset tube folds past
//! `d = RHO`. The cones themselves fold only past `RHO / cos 45 = sqrt 2` (their offset
//! parallel at the ridge has radius `RHO - d cos 45`), the mouths' tubes past `2 sqrt 2`, and
//! the outer rims' tubes face away from the axis: at `RHO < d < sqrt 2` the ridge's tube is
//! the only fold.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::curve::curve::Curve;
use solidcore::error::offset::OffsetFailure;
use solidcore::error::{ErrorKind, Reason};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::query::api_queries::edge_bounding_box;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation;
use solidcore::{Brep, Kernel, SolidError, Vec3};

/// The ridge's radius.
const RHO: f64 = 1.0;

/// The body revolved about Z from the closed profile `pts` in XZ.
fn revolve(k: &mut Kernel, pts: &[(f64, f64)]) -> Brep {
    let p = |(x, z): (f64, f64)| Vec3::new(x, 0.0, z);
    let curves: Vec<Curve> = (0..pts.len())
        .map(|i| {
            Curve::from(
                BSplineCurve::create_line_segment(3, p(pts[i]), p(pts[(i + 1) % pts.len()]))
                    .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_rotational_sweep(k, &refs, Vec3::ZERO, Vec3::Z, 360.0, false).unwrap()
}

fn washer(k: &mut Kernel) -> Brep {
    revolve(
        k,
        &[(2.0, -1.0), (4.0, -1.0), (4.0, 1.0), (2.0, 1.0), (RHO, 0.0)],
    )
}

/// The rounded, merged solid outset of `part` by `distance` (`DoSolidOffset`).
fn outset(k: &mut Kernel, part: &mut Brep, distance: f64) -> Result<Brep, SolidError> {
    let mut exec = OffsetExecutive::new(OffsetOperation::SolidOffset, distance, 1e-5);
    exec.set_inset(false);
    exec.set_extend_convex_edges(false);
    exec.set_merge_results(true);
    let mut notifications = Notifications::new(part);
    exec.do_solid_offset(
        k,
        Some(part),
        SolidOffsetOptions::default(),
        &mut notifications.context(),
    )
}

/// The tube refusal behind `e`, if any.
fn tube_refusal(e: &SolidError) -> Option<OffsetFailure> {
    std::iter::successors(Some(e), |e| e.cause()).find_map(|e| {
        match e.failure().map(|f| &f.reason) {
            Some(&Reason::Offset(
                o @ (OffsetFailure::TubeFolds { .. } | OffsetFailure::TubeNotCertified { .. }),
            )) => Some(o),
            _ => None,
        }
    })
}

/// Whether `b` is sound as a solid's result: clean, a manifold solid, positive volume.
fn sound(b: &mut Brep) -> bool {
    let mut k = Kernel::default();
    validation::soundness(&mut k, b).unwrap().sound(true)
}

#[test]
fn the_ridges_tube_past_its_radius_is_refused_naming_it() {
    let mut k = Kernel::default();
    let mut part = washer(&mut k);
    for distance in [1.1 * RHO, 1.25 * RHO, 1.3 * RHO] {
        let e = outset(&mut k, &mut part, distance).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OffsetFolds, "{e}");
        let Some(OffsetFailure::TubeFolds {
            edge,
            distance: d,
            largest,
        }) = tube_refusal(&e)
        else {
            panic!("{e}");
        };
        assert_eq!(d, distance);
        // the ridge: the circle of radius RHO at z = 0
        let bx = edge_bounding_box(&k, &part, edge, true).unwrap();
        assert!(
            (bx.max().x - RHO).abs() < 1e-6 && bx.max().z.abs() < 1e-6 && bx.min().z.abs() < 1e-6,
            "{bx:?}"
        );
        // the radius named is proved regular, so at most d_max = RHO; and it is not far off
        let largest = largest.expect("a distance proved not to fold");
        assert!(largest <= RHO && largest > 0.5 * RHO, "largest {largest}");
        // and the outset by it builds its tubes
        if let Err(e) = outset(&mut k, &mut part, largest) {
            assert!(tube_refusal(&e).is_none(), "{largest}: {e}");
        }
    }
}

#[test]
fn the_ridges_tube_short_of_its_radius_builds() {
    let mut k = Kernel::default();
    let mut part = washer(&mut k);
    let mut b = outset(&mut k, &mut part, 0.75 * RHO).unwrap();
    assert!(sound(&mut b));
}

#[test]
fn a_tube_facing_away_from_its_edges_centre_never_folds() {
    // a solid cylinder of radius RHO: its rims' arcs run from the caps' normals to the
    // outward radial, every u facing away from the axis
    let mut k = Kernel::default();
    let mut part = revolve(&mut k, &[(0.0, 0.0), (RHO, 0.0), (RHO, 1.0), (0.0, 1.0)]);
    let mut b = outset(&mut k, &mut part, 3.0 * RHO).unwrap();
    assert!(sound(&mut b));
}

#[test]
fn at_the_limit_the_tube_is_refused_or_builds_sound() {
    let mut k = Kernel::default();
    let mut part = washer(&mut k);
    for distance in [RHO * (1.0 - 1e-12), RHO * (1.0 + 1e-12)] {
        match outset(&mut k, &mut part, distance) {
            Ok(mut b) => assert!(sound(&mut b), "{distance}"),
            Err(e) => assert!(tube_refusal(&e).is_some(), "{distance}: {e}"),
        }
    }
}

/// The cost of the decision on the ridge (gate data, `--ignored --nocapture`): its wall time
/// at a radius that folds (with the named largest's runs) and at one that is regular.
#[test]
#[ignore = "timing data for the PR, not a check"]
#[allow(clippy::print_stderr)]
fn the_ridges_decision_cost() {
    use solidcore::query::tangent_edge::tube_regularity;
    use std::time::Instant;
    let mut k = Kernel::default();
    let mut part = washer(&mut k);
    let ridge = part
        .edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let bx = edge_bounding_box(&k, &part, e, true).unwrap();
            (bx.max().x - RHO).abs() < 1e-6 && bx.max().z.abs() < 1e-6
        })
        .expect("the ridge");
    for d in [1.25 * RHO, 0.75 * RHO] {
        let start = Instant::now();
        let r = tube_regularity(&k, &mut part, ridge, d, true).unwrap();
        eprintln!("TUBECOST ridge d {d} {r:?} {:?}", start.elapsed());
    }
}
