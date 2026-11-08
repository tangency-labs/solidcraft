//! WR-260's failed-reversal guard is observable on a public multi-face body.
//!
//! Upstream `SmEdge::ReverseOrientation` (`source/SMLib/src/SmEdge.cpp`) installs the
//! reversed interval, flips every edge use's orientation and deletes its UV trim even
//! when the curve's `ReverseParameterization` fails, and it ignores that status. WR-260
//! records the result: the interval becomes the unset `[1e20, -1e20]` and the topology is
//! mutated despite the failed curve edit. The port's `reverse_orientation` reproduces
//! that by default (WR-260's `failed` path), and with
//! `Kernel::fixes.edge_reversal_keeps_failed_edge` it returns the curve error before any
//! interval, edge use or UV trim change, as the issue's opt-in fix.
//!
//! The isolation uses a public six-face box (`create_box`) whose edge carries the
//! failure condition of the routine-level `issue_521` test: a `HermiteCurve`, whose
//! `reverse_parameterization` is `SmCurve`'s base `SM_ERR` (1001). No modelling
//! operation produces such an edge today (Hermite curves stay inside intersection, and
//! Iso/Conic/Composite curves have no public edge producer), so this pins the guard on
//! the public record API rather than on an ordinary construction; see the WR-260
//! addendum of 2026-11-02.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::OrientType;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p3 as eu3;
use solidcore::topology::{Brep, EdgeId, EdgeuseId};
use solidcore::{Kernel, SolidError};

/// The edge state WR-260 changes: its interval, its uses' orientations and their trims.
#[derive(Debug, Clone, PartialEq)]
struct Observed {
    interval: Extent1d,
    orientations: Vec<OrientType>,
    trims: Vec<bool>,
}

fn observe(b: &Brep, e: EdgeId, eus: &[EdgeuseId]) -> Observed {
    Observed {
        interval: b.edge(e).unwrap().interval,
        orientations: eus
            .iter()
            .map(|&eu| b.edgeuse(eu).unwrap().orientation)
            .collect(),
        trims: eus
            .iter()
            .map(|&eu| b.edgeuse(eu).unwrap().uv_trim_curve.is_some())
            .collect(),
    }
}

fn flipped(o: OrientType) -> OrientType {
    match o {
        OrientType::Same => OrientType::Opposite,
        OrientType::Opposite => OrientType::Same,
        OrientType::Unknown => OrientType::Unknown,
    }
}

/// A solid box `10 x 20 x 30` with UV trims on its first edge's uses, whose curve is
/// replaced by a Hermite curve: `SmCurve::ReverseParameterization` (`SM_ERR`, 1001),
/// the failure condition of the routine-level `issue_521` reversal test.
fn box_with_failing_reversal_edge(k: &mut Kernel) -> (Brep, EdgeId, Vec<EdgeuseId>) {
    let mut b = create_box(k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    assert_eq!(b.face_list().len(), 6);
    assert_eq!(b.edge_list().len(), 12);
    let e = b.edge_list()[0];
    // The body is multi-face: the edge is shared by two of the box's six faces.
    assert_eq!(edge::faces(&b, e, None).unwrap().len(), 2);
    let eus = edge::edgeuses(&b, e, None, None).unwrap();
    assert_eq!(eus.len(), 4);
    for &eu in &eus {
        eu3::get_or_create_uv_trim_curve(k, &mut b, eu, false, false, true)
            .unwrap()
            .status
            .unwrap();
    }
    let hermite = HermiteCurve::new(Vec3::ZERO, Vec3::X, Vec3::X, Vec3::Y, 3);
    b.edge_mut(e).unwrap().curve = Some(Curve::from(hermite));
    (b, e, eus)
}

#[test]
fn failed_reversal_is_observable_on_a_public_box_edge() {
    // Default: upstream ignores the failed curve reversal, installs the unset interval,
    // flips the uses and deletes the trims (WR-260).
    let mut k = Kernel::default();
    assert!(!k.fixes.edge_reversal_keeps_failed_edge);
    let (mut b, e, eus) = box_with_failing_reversal_edge(&mut k);
    let before = observe(&b, e, &eus);
    assert_eq!(before.interval, Extent1d::from_min_max(0.0, 20.0));
    assert_eq!(
        before.orientations,
        [
            OrientType::Same,
            OrientType::Opposite,
            OrientType::Same,
            OrientType::Opposite,
        ]
    );
    assert_eq!(before.trims, [true, false, true, false]);

    assert_eq!(edge::reverse_orientation(&k, &mut b, e), Ok(()));
    let after = observe(&b, e, &eus);
    assert_eq!(after.interval, Extent1d::new());
    assert_eq!(
        (after.interval.min(), after.interval.max()),
        (1.0e20, -1.0e20)
    );
    assert_eq!(
        after.orientations,
        [
            OrientType::Opposite,
            OrientType::Same,
            OrientType::Opposite,
            OrientType::Same,
        ]
    );
    assert_eq!(after.trims, [false, false, false, false]);
    assert!(after
        .orientations
        .iter()
        .zip(&before.orientations)
        .all(|(&a, &b)| a == flipped(b)));
    assert_eq!(b.face_list().len(), 6);
    assert_eq!(b.edge_list().len(), 12);

    // Opt-in: the failed reversal returns before any edge, edge use or trim change.
    let mut k = Kernel::default();
    k.fixes.edge_reversal_keeps_failed_edge = true;
    let (mut b, e, eus) = box_with_failing_reversal_edge(&mut k);
    let before = observe(&b, e, &eus);
    assert_eq!(before.interval, Extent1d::from_min_max(0.0, 20.0));
    assert_eq!(
        edge::reverse_orientation(&k, &mut b, e),
        Err(SolidError::Upstream {
            operation: "curve::curve::p2::Curve",
            code: 1001,
            entity: None,
        })
    );
    assert_eq!(observe(&b, e, &eus), before);
    assert_eq!(b.face_list().len(), 6);
    assert_eq!(b.edge_list().len(), 12);
}
