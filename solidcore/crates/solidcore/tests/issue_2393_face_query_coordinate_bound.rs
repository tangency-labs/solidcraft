//! Regression for HANG-29 reached from the face queries (#2393, nightly fuzz
//! `query.face_p1` `fuzz00201`): a B-spline face with a control point of `1e300`. Upstream's
//! tight face box does not return on it (from `1e155`), nor do its point and edge-curve
//! classifications (from `1e153`; measured by Sloptimum on v1.0.1, #1364), and the Rust
//! tight box took 18-31 s. The kernel now refuses a face with a coordinate above `1e150`
//! at the entry of those queries with [`SolidError::InvalidInput`], unconditionally.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::time::{Duration, Instant};

use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::intersect::point_class::PointClassification;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::primitive::api_trim_ops::trim_surface_with_3d_curves;
use solidcore::query::face::{p1, p3};
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::topology_traverser::face_edges;
use solidcore::{Brep, FaceId, Kernel, SolidError};

/// A biquadratic patch over the unit square in `z = 0`, its centre control point lifted
/// to `z = centre` (the boundary curves keep `z = 0` whatever `centre` is).
fn patch(centre: f64) -> Surface {
    let net: Vec<Vec<CPoint>> = (0..3)
        .map(|i| {
            (0..3)
                .map(|j| {
                    let z = if i == 1 && j == 1 { centre } else { 0.0 };
                    CPoint::new(0.5 * f64::from(i), 0.5 * f64::from(j), z, 1.0)
                })
                .collect()
        })
        .collect();
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let nurb = NurbSurface::new(net, 2, 2, knots.clone(), knots).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

fn line(from: Point3, to: Point3) -> Curve {
    let interval = Extent1d::from_min_max(0.0, 1.0);
    Curve::from(Line::create_line_segment(3, from, to, Some(&interval)).unwrap())
}

/// The flat patch trimmed by its four boundary lines, then given the surface lifted to
/// `centre` (the fuzz face's shape: its trim built before the huge value arrives).
fn face(k: &mut Kernel, centre: f64) -> (Brep, FaceId) {
    let p = |x, y| Point3::new(x, y, 0.0);
    let curves = vec![
        line(p(0.0, 0.0), p(1.0, 0.0)),
        line(p(1.0, 0.0), p(1.0, 1.0)),
        line(p(1.0, 1.0), p(0.0, 1.0)),
        line(p(0.0, 1.0), p(0.0, 0.0)),
    ];
    let mut b = trim_surface_with_3d_curves(k, patch(0.0), &[4], curves).unwrap();
    let fs = faces(&b, None).unwrap();
    assert_eq!(fs.len(), 1);
    b.face_mut(fs[0]).unwrap().surface = Some(patch(centre));
    (b, fs[0])
}

fn is_refusal(r: &Result<(), SolidError>) -> bool {
    matches!(
        r,
        Err(SolidError::InvalidInput { reason, .. }) if reason.contains("HANG-29")
    )
}

/// Runs the three guarded queries and the tight box through `p1`; whether each refused.
fn queries(k: &Kernel, b: &mut Brep, f: FaceId) -> [bool; 4] {
    let tight = is_refusal(&p3::calculate_tight_bounding_box(k, &*b, f).map(|_| ()));
    let tight_p1 = is_refusal(&p1::calculate_bounding_box(k, &*b, f, true).map(|_| ()));
    let mut pc = PointClassification::new(1e-6);
    let point = is_refusal(&p1::point_3d_classify(
        k,
        b,
        f,
        Point3::new(0.5, 0.5, 0.0),
        1e-6,
        true,
        &mut pc,
        p1::Point3dClassifyOptions::default(),
    ));
    let edge = face_edges(b, f).unwrap()[0];
    let crossing = line(Point3::new(0.5, -0.5, 0.0), Point3::new(0.5, 0.5, 0.0));
    let opts = p1::EdgeCurveOnOptions::new(
        0.5,
        Extent1d::from_min_max(0.0, 1.0),
        OrientType::Same,
        1e-6,
    );
    let edge_on =
        is_refusal(&p1::edge_curve_on_classify(k, b, f, Some(edge), &crossing, &opts).map(|_| ()));
    [tight, tight_p1, point, edge_on]
}

#[test]
fn face_with_a_1e300_control_point_is_refused_quickly() {
    let mut k = Kernel::default();
    let (mut b, f) = face(&mut k, 1e300);
    let start = Instant::now();
    assert_eq!(queries(&k, &mut b, f), [true; 4]);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn bound_is_1e150() {
    let mut k = Kernel::default();
    // a centre control point of 4e150 puts the loose surface box above the bound
    let (mut b, f) = face(&mut k, 4e150);
    assert_eq!(queries(&k, &mut b, f), [true; 4]);
    // an ordinary bulge is not refused
    let (mut b, f) = face(&mut k, 0.25);
    assert_eq!(queries(&k, &mut b, f), [false; 4]);
}
