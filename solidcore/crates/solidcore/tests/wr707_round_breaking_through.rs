//! WR-707 (#2299): with `fixes.fillet_refuse_rounds_breaking_through` (and
//! `fixes.fillet_gn_section_generator`), a convex round whose section between its rails
//! would cross a face of the part inside that face is refused, naming the largest radius
//! that stays inside, with that radius less the fit margin on the refusal
//! ([`SolidError::FilletRefused`]).
//!
//! The case (SolidCraft #1668, filed as #2299): a 104 x 64 x 42 box shelled outward by 2,
//! open at the bottom, its outer top edge along y = 32, z = 42 rounded. The ball of radius
//! r has its centre at (32 - r, 42 - r) in (y, z); the wall's inner corner (30, 40) lies
//! sqrt 2 (r - 2) from it, inside the ball past r = 2 sqrt 2 / (sqrt 2 - 1) = 6.828427: the
//! round r7 crosses the inner faces y = 30 and z = 40 and would leave a body that
//! intersects itself. Closed form; fork 5e8c9c1 (built locally) segfaults in `DoFilleting`
//! on this body, so it gives no value to compare against.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel, SolidError};

fn kernel(fix: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_refuse_rounds_breaking_through = fix;
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

fn shelled(k: &mut Kernel) -> Brep {
    let outer = create_box(k, Point3::new(-52.0, -32.0, 0.0), 104.0, 64.0, 42.0).unwrap();
    let inner = create_box(k, Point3::new(-50.0, -30.0, -1.0), 100.0, 60.0, 41.0).unwrap();
    boolean_difference(k, outer, inner).unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn round(k: &mut Kernel, b: &mut Brep, r: f64) -> solidcore::Result<()> {
    let e = pick(k, b, Point3::new(0.0, 32.0, 42.0));
    fillet_edges(k, b, &[e], r, 1, 2, 0.0)
}

#[test]
fn a_round_breaking_through_a_thin_wall_is_refused_naming_the_radius_that_stays_inside() {
    let mut k = kernel(true);
    let mut b = shelled(&mut k);
    let err = round(&mut k, &mut b, 7.0).expect_err("refused");
    eprintln!("{err:?}");
    assert_eq!(err.sm_status(), Some(1001));
    let SolidError::FilletRefused { reason, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(reason, Some(NoRoomReason::BreaksThrough));
    assert_eq!(fit.kind, FitKind::Radius);
    let limit = 2.0 * 2.0_f64.sqrt() / (2.0_f64.sqrt() - 1.0);
    // open bound, less a thousand edge tolerances of 1e-5. A crossing within the zone
    // tolerance (1e-5) of the inner edge is the edge's, not a face's, so the bound is found
    // to within that: 1.7e-5 over the closed form here, the fit still 0.01 inside it.
    let closed = limit - 0.01;
    assert!(
        (fit.size - closed).abs() <= 1e-4,
        "{} vs closed form {closed}",
        fit.size
    );
    assert!(
        fit.size < limit - 0.009,
        "{} vs the bound {limit}",
        fit.size
    );
    // the fit builds: the round inside the wall
    let mut k = kernel(true);
    let mut b = shelled(&mut k);
    round(&mut k, &mut b, fit.size).unwrap();
    let want = 39552.0 - (1.0 - std::f64::consts::FRAC_PI_4) * fit.size * fit.size * 104.0;
    let v = volume(&k, &mut b);
    assert!((v - want).abs() <= 1e-7 * want, "{v} vs {want}");
}

/// A round of 5 stays inside the wall and builds: it takes (1 - pi/4) 25 along 104 off the
/// shelled box's 104 64 42 - 100 60 40 = 39552.
#[test]
fn a_round_inside_the_wall_builds() {
    let mut k = kernel(true);
    let mut b = shelled(&mut k);
    round(&mut k, &mut b, 5.0).unwrap();
    let want = 39552.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 25.0 * 104.0;
    let v = volume(&k, &mut b);
    assert!((v - want).abs() <= 1e-7 * want, "{v} vs {want}");
}

/// Without the flag the round r7 is not refused for breaking through (#2299's result).
#[test]
fn without_the_flag_the_break_through_is_not_judged() {
    let mut k = kernel(false);
    let mut b = shelled(&mut k);
    if let Err(err) = round(&mut k, &mut b, 7.0) {
        assert!(
            !matches!(
                err,
                SolidError::FilletRefused {
                    reason: Some(NoRoomReason::BreaksThrough),
                    ..
                }
            ),
            "{err:?}"
        );
    }
}
