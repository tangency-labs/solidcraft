//! An edge curve's caches at its edge's tolerance (`SmTol::GetZoneTol3d(const SmObject*)`
//! on a curve: `GetEdge()->GetTolerance()`). Upstream's `pEdge->GetCurve()->Length(..)`
//! and `FindParameterAtArcLength(..)` build their curve caches at half the edge's
//! tolerance. A port curve cannot reach its edge: [`edge::evaluation_curve`] gives the
//! edge's curve carrying that tolerance, so the same calls work on boolean-made edge
//! curves as upstream's do.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Point3;
use solidcore::mass::integrator::properties::edge_length;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel, SolidError};

const ACCURACY: f64 = 1e-6;

/// A 10 x 10 x 10 box less a cylinder r 3 through it along z, and the union of two
/// overlapping boxes: boolean-made edges, curved (the hole's circles) and straight.
fn results(k: &mut Kernel) -> Vec<Brep> {
    let body = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let drill = create_cylinder(k, Point3::new(5.0, 5.0, -1.0), 3.0, 12.0).unwrap();
    let drilled = boolean::boolean_difference(k, body, drill).unwrap();
    let a = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let b = create_box(k, Point3::new(5.0, 5.0, 5.0), 10.0, 10.0, 10.0).unwrap();
    let joined = boolean::boolean_union(k, a, b).unwrap();
    vec![drilled, joined]
}

/// The edges whose stored curve is owned (by its edge) and records no tolerance: the
/// curves the borrowed-curve gap concerns.
fn owned_unrecorded(b: &Brep) -> Vec<EdgeId> {
    b.edge_list()
        .iter()
        .copied()
        .filter(|&e| {
            edge::curve(b, e)
                .unwrap()
                .is_some_and(|c| c.owner().is_some() && c.owner_zone_tol_3d().is_none())
        })
        .collect()
}

/// Through [`edge::evaluation_curve`] every boolean-made edge curve gives its length
/// (equal, bit for bit, to `SmApiEdgeComputeLength`'s, which reads the edge's tolerance
/// the same way) and the parameter half that length along it.
#[test]
fn evaluation_curves_of_boolean_edges_measure_at_the_edge_tolerance() {
    let mut k = Kernel::default();
    for b in results(&mut k) {
        assert!(!owned_unrecorded(&b).is_empty(), "owned edge curves");
        for &e in b.edge_list() {
            let c = edge::evaluation_curve(&b, e).unwrap().expect("edge curve");
            if c.owner().is_some() {
                assert_eq!(c.owner_zone_tol_3d(), Some(edge::tolerance(&b, e).unwrap()));
            }
            let interval = edge::interval(&b, e).unwrap();
            let length = c.length(&k, &interval, ACCURACY).unwrap();
            assert_eq!(
                length,
                edge_length(&k, &b, e, ACCURACY).unwrap(),
                "edge {e:?}"
            );
            assert!(c.length_base(&k, &interval, ACCURACY).is_ok(), "edge {e:?}");
            let half = c
                .find_parameter_at_arc_length(&k, interval.min(), 0.5 * length)
                .unwrap();
            assert!(half.converged && !half.past_end, "edge {e:?}: {half:?}");
            assert!(interval.min() < half.end_param && half.end_param < interval.max());
            let first = Extent1d::from_min_max(interval.min(), half.end_param);
            let to_half = c.length(&k, &first, ACCURACY).unwrap();
            assert!(
                (to_half - 0.5 * length).abs() <= 1e-6 * length,
                "edge {e:?}: {to_half} of {length}"
            );
        }
    }
}

/// The edge's curve borrowed from its record ([`edge::curve`]) stays
/// [`SolidError::Unported`] for `SmTol::GetZoneTol3d`: it is owned by its edge but holds
/// no tolerance, and a bare curve cannot reach the edge record that has it (the port's
/// curves name their owner, they do not point to it). Upstream reads it through the
/// pointer; the port's route is [`edge::evaluation_curve`] (the test above). Guessing a
/// tolerance here (the system one) would build a different cache than upstream's.
#[test]
fn a_borrowed_owned_edge_curve_stays_unported() {
    let mut k = Kernel::default();
    let is_zone_tol_gap = |r: &solidcore::Result<f64>| {
        matches!(
            r,
            Err(SolidError::Unported {
                upstream: "SmTol::GetZoneTol3d(const SmObject*)",
                ..
            })
        )
    };
    let mut curved = 0;
    for b in results(&mut k) {
        for e in owned_unrecorded(&b) {
            let c: &Curve = edge::curve(&b, e).unwrap().unwrap();
            let interval = edge::interval(&b, e).unwrap();
            let base = c.length_base(&k, &interval, ACCURACY);
            assert!(is_zone_tol_gap(&base), "edge {e:?}: {base:?}");
            if !matches!(c.kind(), CurveKind::Line(_)) {
                // (a line's length is closed form: the arc-length search builds no cache)
                curved += 1;
                let half = c
                    .find_parameter_at_arc_length(&k, interval.min(), 1.0)
                    .map(|p| p.end_param);
                assert!(is_zone_tol_gap(&half), "edge {e:?}: {half:?}");
            }
        }
    }
    assert!(curved > 0, "a curved boolean edge");
}

/// A copy without owner ([`Clone`], as upstream's copy constructor) is measured at the
/// system tolerance, as before.
#[test]
fn an_unowned_copy_is_measured_at_the_system_tolerance() {
    let mut k = Kernel::default();
    let b = results(&mut k).remove(0);
    let e = owned_unrecorded(&b)[0];
    let copy: Curve = edge::curve(&b, e).unwrap().unwrap().clone();
    assert!(copy.owner().is_none());
    let interval = edge::interval(&b, e).unwrap();
    assert!(copy.length_base(&k, &interval, ACCURACY).is_ok());
}
