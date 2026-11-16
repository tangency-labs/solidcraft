//! WR-3: a bounded volume's domain checks contain a segment only when both ends
//! are in the parameter box (a box is convex, so that is the whole segment).

use solidcore::{
    geom::extent3d::Extent3d,
    math::Vec3,
    nurbs::{math::CPoint, spiral::allocate_nurb_volume},
    surface::{
        b_spline_volume::BSplineVolume,
        volume::p1::{Volume, VolumeKind},
    },
    Kernel,
};

#[test]
fn one_endpoint_inside_is_not_contained() {
    let mut nurb = allocate_nurb_volume([1; 3], [1; 3], [3; 3]).unwrap();
    for d in 0..3 {
        nurb.knots_mut(d).copy_from_slice(&[0.0, 0.0, 1.0, 1.0]);
    }
    for (i, point) in nurb.cpts_mut().iter_mut().enumerate() {
        *point = CPoint::new((i / 4) as f64, ((i / 2) % 2) as f64, (i % 2) as f64, 1.0);
    }
    let volume = Volume::new(VolumeKind::BSpline(BSplineVolume::new(nurb)));
    let inside = Vec3::new(0.5, 0.5, 0.5);
    let outside = Vec3::new(2.0, 0.5, 0.5);
    let kernel = Kernel::default();
    for (start, end, expected) in [
        (inside, outside, false),
        (outside, inside, false),
        (Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), true),
        (outside, outside, false),
        (Vec3::new(-1.0, 0.5, 0.5), outside, false),
    ] {
        assert_eq!(
            volume.is_line_in_param_domain_simple(start, end).unwrap(),
            expected,
        );
        assert_eq!(
            volume
                .is_line_in_param_domain(&kernel, start, end, false)
                .unwrap(),
            expected,
        );
        // Also exercise the in-space entry point and compounding option.
        assert_eq!(
            volume
                .is_line_in_in_space_domain(&kernel, start, end, true)
                .unwrap(),
            (expected, start, end),
        );
    }
    // Box checks also traverse this path; the box reaches outside the domain.
    let partial_box = Extent3d::from_points(inside, outside);
    assert!(!volume
        .is_box_in_param_domain(&kernel, &partial_box, false)
        .unwrap());
}
