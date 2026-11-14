//! WR-46: the simple twist's mixed derivative follows its first derivative.
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::surface::twist_volume::TwistVolume;
use solidcore::surface::volume::{EvalOptions, Volume, VolumeKind};

#[test]
fn duw_matches_central_difference() {
    let kernel = Kernel::default();
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
fn simple_evaluation_matches_the_twist_closed_form() {
    // P = (u cos a - v sin a, u sin a + v cos a, w), a = per w; every partial up to
    // order n, including Duw = (-per sin a, per cos a, 0).
    for period in [4.0, -4.0] {
        let twist = Volume::new(VolumeKind::Twist(TwistVolume::from_members(period, 1e-5)));
        let per = 2.0 * std::f64::consts::PI / period;
        for w in [0.0, 0.3] {
            let p = Vec3::new(0.7, -0.4, w);
            let (c, s) = ((per * w).cos(), (per * w).sin());
            for n in 0..=3 {
                let got = twist
                    .evaluate_simple(p, n, &EvalOptions::default())
                    .unwrap();
                let n1 = n + 1;
                for i in 0..n1 {
                    for j in 0..n1 {
                        for k in 0..n1 {
                            let dc = [c, -s, -c, s][k];
                            let ds = [s, c, -s, -c][k];
                            let sc = per.powi(k as i32);
                            let want = if i + j + k > n {
                                Vec3::ZERO
                            } else {
                                match (i, j) {
                                    (0, 0) => Vec3::new(
                                        sc * (p.x * dc - p.y * ds),
                                        sc * (p.x * ds + p.y * dc),
                                        [w, 1.0, 0.0, 0.0][k],
                                    ),
                                    (1, 0) => Vec3::new(sc * dc, sc * ds, 0.0),
                                    (0, 1) => Vec3::new(-sc * ds, sc * dc, 0.0),
                                    _ => Vec3::ZERO,
                                }
                            };
                            let at = got[(i * n1 + j) * n1 + k];
                            assert!(
                                (at - want).length() < 1e-12,
                                "period={period}, w={w}, n={n}, D[{i}][{j}][{k}]: {at:?} vs {want:?}"
                            );
                        }
                    }
                }
                if n >= 2 {
                    let duw = got[n1 * n1 + 1];
                    assert_eq!(duw.y, per * c, "Duw.y is +per cos a");
                }
            }
        }
        assert!(twist
            .evaluate_simple(Vec3::ZERO, 4, &EvalOptions::default())
            .is_err());
    }
}

#[test]
fn mapped_curve_second_derivative_through_compounding() {
    use solidcore::curve::crv_in_volume::CrvInVolume;
    use solidcore::curve::curve::Curve;
    use solidcore::curve::line::Line;
    use solidcore::geom::transform::Transform;

    let kernel = Kernel::default();
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
        let first = |t| curve.evaluate(&kernel, t, 1, true, false).unwrap()[1];
        let difference = (first(t + h) - first(t - h)) / (2.0 * h);
        let second = curve.evaluate(&kernel, t, 2, true, false).unwrap();
        assert!((second[2] - difference).length() < 1e-9);
    }
}

#[test]
fn mapped_surface_mixed_derivative() {
    use solidcore::geom::vector2d::Point2;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    use solidcore::surface::plane::Plane;
    use solidcore::surface::srf_in_volume::SrfInVolume;
    use solidcore::surface::surface::Surface;

    let kernel = Kernel::default();
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
    let second = surface.evaluate(&kernel, uv, 2, 2, flags).unwrap();
    assert!((second[1][1] - difference).length() < 1e-9);
}
