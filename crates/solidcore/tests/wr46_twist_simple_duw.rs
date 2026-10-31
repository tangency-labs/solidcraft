//! WR-46: the simple twist's mixed derivative follows its first derivative.
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::surface::twist_volume::TwistVolume;
use solidcore::surface::volume::{EvalOptions, Volume, VolumeKind};

#[test]
fn opt_in_duw_matches_central_difference() {
    let mut kernel = Kernel::default();
    assert!(!kernel.fixes.twist_simple_duw_sign);
    kernel.fixes.twist_simple_duw_sign = true;
    // Simple evaluation is in the identity frame, without orientation or compounding.
    for period in [4.0, -4.0] {
        let twist = Volume::new(VolumeKind::Twist(TwistVolume::from_members(period, 1e-5)));
        for w in [0.0, 0.3] {
            for n in [2, 3] {
                let p = Vec3::new(0.7, -0.4, w);
                let h = 1e-6;
                let du = |w| {
                    twist
                        .map(Vec3::new(p.x, p.y, w), 1, &EvalOptions::default())
                        .unwrap()[4]
                };
                let difference = (du(w + h) - du(w - h)) / (2.0 * h);
                let fixed = twist
                    .map(
                        p,
                        n,
                        &EvalOptions {
                            fixes: kernel.fixes,
                            ..EvalOptions::default()
                        },
                    )
                    .unwrap();
                let index = (n + 1) * (n + 1) + 1;
                assert!(
                    (fixed[index] - difference).length() < 1e-9,
                    "period={period}, w={w}, n={n}: {:?} vs {difference:?}",
                    fixed[index]
                );
            }
        }
    }
}

#[test]
fn default_preserves_cpp_sign_and_all_other_entries() {
    let mut kernel = Kernel::default();
    let twist = Volume::new(VolumeKind::Twist(TwistVolume::from_members(4.0, 1e-5)));
    for n in 0..=3 {
        let p = Vec3::new(0.7, -0.4, 0.0);
        let legacy = twist
            .evaluate_simple(p, n, &EvalOptions::default())
            .unwrap();
        let default = twist
            .map(
                p,
                n,
                &EvalOptions {
                    fixes: kernel.fixes,
                    ..EvalOptions::default()
                },
            )
            .unwrap();
        assert_eq!(default, legacy);
        kernel.fixes.twist_simple_duw_sign = true;
        let fixed = twist
            .map(
                p,
                n,
                &EvalOptions {
                    fixes: kernel.fixes,
                    ..EvalOptions::default()
                },
            )
            .unwrap();
        for (i, (before, after)) in legacy.iter().zip(&fixed).enumerate() {
            if n >= 2 && i == (n + 1) * (n + 1) + 1 {
                assert!(before.y < 0.0);
                assert_eq!(after.y, -before.y);
                assert_eq!(after.x, before.x);
                assert_eq!(after.z, before.z);
            } else {
                assert_eq!(before, after);
            }
        }
        kernel.fixes.twist_simple_duw_sign = false;
    }
    assert!(twist
        .map(
            Vec3::ZERO,
            4,
            &EvalOptions {
                fixes: kernel.fixes,
                ..EvalOptions::default()
            }
        )
        .is_err());
}

#[test]
fn mapped_curve_propagates_kernel_fix_through_compounding() {
    use solidcore::curve::crv_in_volume::CrvInVolume;
    use solidcore::curve::curve::Curve;
    use solidcore::curve::line::Line;
    use solidcore::geom::transform::Transform;

    let mut kernel = Kernel::default();
    let start = Vec3::new(0.7, -0.4, 0.0);
    let end = Vec3::new(1.7, -0.4, 1.0);
    let line = Curve::from(Line::create_line_segment(3, start, end, None).unwrap());
    for in_param_space in [false, true] {
        let twist = Volume::new(VolumeKind::Twist(TwistVolume::from_members(4.0, 1e-5)));
        // A parameter-space curve uses Evaluate, whose next map uses Map;
        // an in-space curve uses Map directly.
        let volume = if in_param_space {
            let mut first = Volume::new(VolumeKind::Transform(Transform::default()));
            first.add_next_map(twist, 0).unwrap();
            first
        } else {
            twist
        };
        let curve = CrvInVolume::new(line.clone(), in_param_space, volume, 0, 0).unwrap();
        let t = 0.3;
        let h = 1e-6;
        kernel.fixes.twist_simple_duw_sign = false;
        let legacy = curve.evaluate(&kernel, t, 2, true, false).unwrap();
        let first = |t| curve.evaluate(&kernel, t, 1, true, false).unwrap()[1];
        let difference = (first(t + h) - first(t - h)) / (2.0 * h);
        assert!((legacy[2] - difference).length() > 0.1);
        kernel.fixes.twist_simple_duw_sign = true;
        let fixed = curve.evaluate(&kernel, t, 2, true, false).unwrap();
        assert!((fixed[2] - difference).length() < 1e-9);
        assert_eq!(legacy[..2], fixed[..2]);
    }
}

#[test]
fn mapped_surface_propagates_kernel_fix() {
    use solidcore::geom::vector2d::Point2;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    use solidcore::surface::plane::Plane;
    use solidcore::surface::srf_in_volume::SrfInVolume;
    use solidcore::surface::surface::Surface;

    let mut kernel = Kernel::default();
    let base = Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0)));
    let twist = TwistVolume::create(
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 0.0),
        4.0,
        1e-5,
    );
    let surface = SrfInVolume::new(&kernel, base, false, twist, 0, 0).unwrap();
    let uv = Point2::new(0.3, 0.7);
    let flags = EvaluateFlags {
        only_upper_half: true,
        ..EvaluateFlags::default()
    };
    let h = 1e-6;
    let du = |v| {
        surface
            .evaluate(&kernel, Point2::new(uv.x, v), 1, 1, flags)
            .unwrap()[1][0]
    };
    let difference = (du(uv.y + h) - du(uv.y - h)) / (2.0 * h);
    let legacy = surface.evaluate(&kernel, uv, 2, 2, flags).unwrap();
    assert!((legacy[1][1] - difference).length() > 0.1);
    kernel.fixes.twist_simple_duw_sign = true;
    let fixed = surface.evaluate(&kernel, uv, 2, 2, flags).unwrap();
    assert!((fixed[1][1] - difference).length() < 1e-9);
    assert_eq!(legacy[0][0], fixed[0][0]);
}
