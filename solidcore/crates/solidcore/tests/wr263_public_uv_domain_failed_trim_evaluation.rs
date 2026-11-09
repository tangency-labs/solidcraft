//! Public-operation regression for tangency-labs/solidcore#521 / WR-263:
//! `SmEdgeuse::GetStartPointUVLoopGap` reads endpoints a failed trim evaluation never
//! set. The opt-in `Kernel::fixes.topology_failed_evaluations_propagate` reports a gap
//! it cannot measure as `SM_BIG_DOUBLE` instead.
//!
//! The measurement is through public APIs only: a public box (a trimmed face), a
//! public record edit of one edge use's UV trim curve (its evaluation fails at an
//! interior parameter, while its control-point bounding box is defined), the loop's
//! edge intervals moved so the gap samples that interior parameter, and the public
//! operation `calculate_uv_domain_from_uv_trim_curves` with `want_max_gap`. The flag
//! off and on give two different exact `max_uv_loop_gap` values on the same face; the
//! flag-on value is pinned here, and forcing the flag off makes the test fail.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, BIG_DOUBLE};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::edgeuse::p1::{set_uv_trim_curve, UvTrimCurveArg};
use solidcore::topology::face::p1::calculate_uv_domain_from_uv_trim_curves;
use solidcore::topology::{Brep, EdgeuseId, EdgeuseOwner, FaceId};

/// The first face with a loop of at least two edge uses (a trimmed solid face).
fn trimmed_face(b: &Brep) -> (FaceId, Vec<EdgeuseId>) {
    for f in b.faces().ids() {
        for &l in &b.face(f).unwrap().loops {
            let Some(lu) = b.loop_(l).unwrap().loopuse else {
                continue;
            };
            let eus: Vec<EdgeuseId> = b
                .edgeuses()
                .iter()
                .filter(|(_, e)| e.owner == EdgeuseOwner::Loopuse(Some(lu)))
                .map(|(id, _)| id)
                .collect();
            if eus.len() >= 2 {
                return (f, eus);
            }
        }
    }
    panic!("the box has no trimmed face");
}

/// A degree-1 curve with two control points and two knots: the public NURB
/// constructor accepts it and its control-point bounding box is defined, but
/// `evaluate_point` fails at interior parameters. At the ends (`0`, `1`) it still
/// returns the control points, so the gap must sample an interior parameter to see
/// the failure.
fn malformed_trim() -> Curve {
    let cp = |x: f64| CPoint::new(x, 0.0, 0.0, NOW);
    Curve::from(BSplineCurve::from_nurb(
        NurbCurve::new(vec![cp(0.0), cp(1.0)], 1, vec![0.0, 1.0]).unwrap(),
        2,
        false,
    ))
}

/// Runs the public operation on a fresh box with the record edits applied.
fn max_uv_loop_gap(propagate: bool) -> (Extent2d, f64) {
    let mut k = Kernel::default();
    k.fixes.topology_failed_evaluations_propagate = propagate;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    b.refresh_cached_topology();
    let (f, eus) = trimmed_face(&b);

    // Move the loop's edge intervals off the trim's end parameters, so the gap
    // samples the interior parameter where the edited trim's evaluation fails.
    for &eu in &eus {
        let e = b.edgeuse(eu).unwrap().edge.unwrap();
        b.edge_mut(e).unwrap().interval = Extent1d::from_min_max(0.25, 0.75);
    }
    b.refresh_cached_topology();

    // Public record edit: the UV trim record of one edge use.
    set_uv_trim_curve(
        &mut b,
        eus[0],
        UvTrimCurveArg::New(Box::new(malformed_trim())),
        0.0,
        false,
        None,
    )
    .unwrap();

    // Public operation: CalculateUVDomainFromUVTrimCurves with the max-gap request.
    let out = calculate_uv_domain_from_uv_trim_curves(&k, &mut b, f, true, true, true).unwrap();
    (out.domain, out.max_uv_loop_gap)
}

#[test]
fn wr263_public_uv_domain_gap_propagates_a_failed_trim_endpoint() {
    let (off_domain, off_gap) = max_uv_loop_gap(false);
    let (on_domain, on_gap) = max_uv_loop_gap(true);

    // Same face, same domain: only the unmeasurable gap differs.
    assert_eq!(off_domain, Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0));
    assert_eq!(on_domain, off_domain);

    // Upstream / default: the failed endpoint evaluation's unset value is measured.
    assert_eq!(off_gap, 1.030_776_406_404_415_1);
    // WR-263 with the opt-in fix: a gap that cannot be measured.
    assert_eq!(on_gap, BIG_DOUBLE);
}
