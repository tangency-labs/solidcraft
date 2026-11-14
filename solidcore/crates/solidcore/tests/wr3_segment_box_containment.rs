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
    Kernel, SolidError,
};

/// The issue's unit B-spline volume: degree 1 in each direction, knots
/// `[0, 0, 1, 1]`, natural domain the unit box.
fn unit_volume() -> Volume {
    let mut nurb = allocate_nurb_volume([1; 3], [1; 3], [3; 3]).unwrap();
    for d in 0..3 {
        nurb.knots_mut(d).copy_from_slice(&[0.0, 0.0, 1.0, 1.0]);
    }
    for (i, point) in nurb.cpts_mut().iter_mut().enumerate() {
        *point = CPoint::new((i / 4) as f64, ((i / 2) % 2) as f64, (i % 2) as f64, 1.0);
    }
    Volume::new(VolumeKind::BSpline(BSplineVolume::new(nurb)))
}

#[test]
fn one_endpoint_inside_is_not_contained() {
    let volume = unit_volume();
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

/// #16459: a non-finite point is a typed invalid-input error at the volume
/// domain entry points, never an answer of "inside" (§7.7: "NaN refuses").
/// Before the fix the point and line calls below return `Ok(true)`.
#[test]
fn non_finite_inputs_are_refused() {
    let volume = unit_volume();
    let kernel = Kernel::default();
    let nan = Vec3::new(f64::NAN, f64::NAN, f64::NAN);
    let inside = Vec3::new(0.5, 0.5, 0.5);

    // The issue's repro: both calls return `Ok(true)` today.
    assert!(matches!(
        volume.is_point_in_param_domain_simple(nan),
        Err(SolidError::InvalidInput { .. })
    ));
    assert!(volume.is_point_in_param_domain(nan, false).is_err());
    assert!(volume.is_point_in_param_domain(nan, true).is_err());

    // A NaN end refuses: both ends are now the only segment test (#16442
    // retired the flag), so the line checks run once at the default.
    assert!(volume
        .is_line_in_param_domain_simple(nan, inside)
        .is_err());
    assert!(volume
        .is_line_in_param_domain_simple(inside, nan)
        .is_err());
    assert!(volume
        .is_line_in_param_domain(&kernel, nan, inside, false)
        .is_err());

    // A box with a non-finite corner, and the in-space forms, refuse too.
    let nan_box = Extent3d::from_points(nan, inside);
    assert!(volume
        .is_box_in_param_domain(&kernel, &nan_box, false)
        .is_err());
    assert!(volume.is_point_in_in_space_domain(nan, true).is_err());
    assert!(volume
        .is_line_in_in_space_domain(&kernel, nan, inside, false)
        .is_err());

    // Infinite coordinates refuse the same way.
    let inf = Vec3::new(f64::INFINITY, 0.5, 0.5);
    assert!(volume.is_point_in_param_domain_simple(inf).is_err());

    // Control cases: finite points still get an answer.
    assert!(volume.is_point_in_param_domain_simple(inside).unwrap());
    assert!(!volume
        .is_point_in_param_domain_simple(Vec3::new(2.0, 0.5, 0.5))
        .unwrap());
}
