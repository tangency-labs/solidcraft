//! WR-3: the volume domain checks honor opt-in segment containment.

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
fn one_endpoint_inside_is_contained_only_in_legacy_mode() {
    let volume = unit_volume();
    let inside = Vec3::new(0.5, 0.5, 0.5);
    let outside = Vec3::new(2.0, 0.5, 0.5);
    let mut kernel = Kernel::default();
    assert!(!kernel.fixes.extent3d_contains_both_segment_endpoints);
    for fixed in [false, true] {
        kernel.fixes.extent3d_contains_both_segment_endpoints = fixed;
        for (start, end, expected) in [
            (inside, outside, !fixed),
            (outside, inside, !fixed),
            (Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), true),
            (outside, outside, false),
            (Vec3::new(-1.0, 0.5, 0.5), outside, false),
        ] {
            assert_eq!(
                volume
                    .is_line_in_param_domain_simple(&kernel, start, end)
                    .unwrap(),
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
        // Box checks also traverse this path, but their outside face has edges
        // with both ends outside, so even legacy mode rejects this box.
        let partial_box = Extent3d::from_points(inside, outside);
        assert!(!volume
            .is_box_in_param_domain(&kernel, &partial_box, false)
            .unwrap());
    }
}

/// #16459: a non-finite point is a typed invalid-input error at the volume
/// domain entry points, never an answer of "inside" (§7.7: "NaN refuses").
/// Before the fix the point and line calls below return `Ok(true)`.
#[test]
fn non_finite_inputs_are_refused() {
    let volume = unit_volume();
    let mut kernel = Kernel::default();
    let nan = Vec3::new(f64::NAN, f64::NAN, f64::NAN);
    let inside = Vec3::new(0.5, 0.5, 0.5);

    // The issue's repro: both calls return `Ok(true)` today.
    assert!(matches!(
        volume.is_point_in_param_domain_simple(nan),
        Err(SolidError::InvalidInput { .. })
    ));
    assert!(volume.is_point_in_param_domain(nan, false).is_err());
    assert!(volume.is_point_in_param_domain(nan, true).is_err());

    // A NaN end refuses in both segment-endpoint modes.
    for fixed in [false, true] {
        kernel.fixes.extent3d_contains_both_segment_endpoints = fixed;
        assert!(volume
            .is_line_in_param_domain_simple(&kernel, nan, inside)
            .is_err());
        assert!(volume
            .is_line_in_param_domain_simple(&kernel, inside, nan)
            .is_err());
        assert!(volume
            .is_line_in_param_domain(&kernel, nan, inside, false)
            .is_err());
    }

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
