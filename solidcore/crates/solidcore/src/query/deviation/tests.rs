//! Deviation: two parallel lines 5 apart, a 0.0056 offset read back, a curve lying on a plane,
//! and the station count refused.

use super::*;
use crate::kernel::KernelOptions;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::primitive::api_primitives::create_plane_surface;

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn line(p: [f64; 3], q: [f64; 3]) -> Curve {
    Curve::from(BSplineCurve::create_line_segment(3, v(p), v(q)).unwrap())
}

/// A cubic Bézier curve on [0, 1] through `points`.
fn cubic(points: &[[f64; 3]; 4]) -> Curve {
    let control: Vec<Vec3> = points.iter().map(|&p| v(p)).collect();
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &control,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[4, 4],
            knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

#[test]
fn two_parallel_lines_five_apart_deviate_by_five() {
    let k = Kernel::new(&KernelOptions::default());
    let a = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let b = line([0.0, 5.0, 0.0], [10.0, 5.0, 0.0]);
    let d = curve_deviation(&k, &a, &b, 11).unwrap();
    assert!((d.max - 5.0).abs() < 1e-9, "{d:?}");
    assert_eq!(d.comb.len(), 11);
    for (p, q) in &d.comb {
        assert!((p.distance(*q) - 5.0).abs() < 1e-9, "{p:?} {q:?}");
    }
    let bound = d.bound.unwrap();
    assert!((5.0 - 1e-9..5.0 + 1e-9).contains(&bound), "{bound}");
}

/// The same cubic moved 0.0056 up: that offset reads back.
#[test]
fn a_small_offset_reads_back() {
    let k = Kernel::new(&KernelOptions::default());
    let pts = [
        [0.0, 0.0, 0.0],
        [3.0, 4.0, 0.0],
        [7.0, -2.0, 0.0],
        [10.0, 1.0, 0.0],
    ];
    let up = pts.map(|p| [p[0], p[1], p[2] + 0.0056]);
    let d = curve_deviation(&k, &cubic(&pts), &cubic(&up), 33).unwrap();
    assert!((d.max - 0.0056).abs() < 1e-9, "{d:?}");
    assert!((d.to.z - d.at.z - 0.0056).abs() < 1e-9, "{d:?}");
}

/// A curve in the plane of a flat surface: no deviation.
#[test]
fn a_curve_lying_on_a_plane_does_not_deviate() {
    let k = Kernel::new(&KernelOptions::default());
    let plane =
        Surface::from(create_plane_surface(Point3::new(0.0, 0.0, 0.0), 20.0, 20.0).unwrap());
    let domain = plane.natural_uv_domain_with_nurb(&k).unwrap();
    let c = cubic(&[
        [1.0, 1.0, 0.0],
        [5.0, 9.0, 0.0],
        [12.0, 3.0, 0.0],
        [18.0, 15.0, 0.0],
    ]);
    let d = curve_surface_deviation(&k, &c, &plane, &domain, 21).unwrap();
    assert!(d.max < 1e-9, "{d:?}");
    assert_eq!(d.bound, None);
}

#[test]
fn a_station_count_out_of_range_is_refused() {
    let k = Kernel::new(&KernelOptions::default());
    let a = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    assert!(curve_deviation(&k, &a, &a, 1).is_err());
    assert!(curve_deviation(&k, &a, &a, MAX_SAMPLES + 1).is_err());
}
