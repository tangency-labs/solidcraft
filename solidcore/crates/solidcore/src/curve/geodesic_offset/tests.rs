//! Geodesic offsets against closed forms: a line on a plane (a parallel line), a circle round a
//! cylinder (the circles above and below it), the equator of a sphere (the latitudes `d / R`
//! from it); the parameter box left; the inputs refused.

use super::*;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::primitive::api_primitives::{create_cylinder, create_plane, create_sphere};
use crate::topology::Brep;

/// The surface and parameter box of `b`'s face whose surface `on` accepts a point of.
fn face_of(b: &Brep, pick: impl Fn(&Surface, &Extent2d) -> bool) -> (Surface, Extent2d) {
    for &f in b.face_list() {
        let face = b.face(f).unwrap();
        if let Some(s) = face.surface.as_ref() {
            if pick(s, &face.uv_domain) {
                return (s.clone(), face.uv_domain);
            }
        }
    }
    panic!("no such face");
}

/// The cubic interpolating `n` points `p(i / (n - 1))`, uniform parameters on [0, 1].
fn through(n: usize, p: impl Fn(f64) -> Vec3) -> Curve {
    let params: Vec<f64> = (0..n).map(|i| i as f64 / (n - 1) as f64).collect();
    let points: Vec<Vec3> = params.iter().map(|&t| p(t)).collect();
    Curve::from(
        BSplineCurve::create_interpolating_curve_with_params(
            ParamMethod::ChordLength,
            3,
            3,
            &points,
            &[],
            None,
            false,
            Some(&params),
        )
        .unwrap(),
    )
}

/// Points along `c` at 201 even parameter steps.
fn along(k: &Kernel, c: &Curve) -> Vec<Vec3> {
    let ivl = c.natural_interval();
    (0..=200)
        .map(|i| {
            c.evaluate_point(k, ivl.evaluate(f64::from(i) / 200.0))
                .unwrap()
        })
        .collect()
}

#[test]
fn a_line_on_a_plane_offsets_to_the_parallel_line() {
    let mut k = Kernel::default();
    let b = create_plane(&mut k, Point3::ZERO, 20.0, 10.0).unwrap();
    let (s, dom) = face_of(&b, |_, _| true);
    let line = through(5, |t| Vec3::new(2.0 + 16.0 * t, 5.0, 0.0));
    let mut ys = Vec::new();
    for side in [Side::Left, Side::Right] {
        let o = geodesic_offset(
            &k,
            &s,
            &dom,
            &line,
            &line.natural_interval(),
            3.0,
            side,
            1e-6,
        )
        .unwrap();
        assert!(o.worst <= 1e-6, "{o:?}");
        let pts = along(&k, &o.curve);
        for p in &pts {
            assert!(
                ((p.y - 5.0).abs() - 3.0).abs() <= 1e-6 && p.z.abs() <= 1e-9,
                "{p:?}"
            );
        }
        assert!((pts[0].x - 2.0).abs() <= 1e-6 && (pts[200].x - 18.0).abs() <= 1e-6);
        ys.push(pts[100].y);
    }
    // the two sides lie either side of the line
    assert!((ys[0] - 5.0) * (ys[1] - 5.0) < 0.0, "{ys:?}");
}

#[test]
fn a_circle_round_a_cylinder_offsets_to_the_circles_above_and_below() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
    // the wall: the face whose surface holds a point 5 out at mid height
    let (s, dom) = face_of(&b, |s, d| {
        s.drop_point::<NoObject>(
            &k,
            Vec3::new(5.0, 0.0, 5.0),
            d,
            &DropPointOptions::default(),
        )
        .is_ok_and(|f| f.success && f.gap <= 1e-9)
    });
    let tau = std::f64::consts::TAU;
    let round = through(65, |t| {
        Vec3::new(5.0 * (tau * t).cos(), 5.0 * (tau * t).sin(), 5.0)
    });
    let mut zs = Vec::new();
    for side in [Side::Left, Side::Right] {
        let o = geodesic_offset(
            &k,
            &s,
            &dom,
            &round,
            &round.natural_interval(),
            2.0,
            side,
            1e-5,
        )
        .unwrap();
        for p in along(&k, &o.curve) {
            assert!((p.x.hypot(p.y) - 5.0).abs() <= 1e-5, "{p:?}");
            assert!(((p.z - 5.0).abs() - 2.0).abs() <= 1e-5, "{p:?}");
        }
        zs.push(o.curve.evaluate_point(&k, 0.5).unwrap().z);
    }
    zs.sort_by(f64::total_cmp);
    assert!(
        (zs[0] - 3.0).abs() <= 1e-5 && (zs[1] - 7.0).abs() <= 1e-5,
        "{zs:?}"
    );
}

/// On a sphere of radius 10 the equator offset 3 along the surface is the latitude 0.3 rad
/// north or south: the geodesics are meridians, so the offset bends with the sphere.
#[test]
fn the_equator_of_a_sphere_offsets_to_the_latitudes_d_over_r_from_it() {
    let mut k = Kernel::default();
    let b = create_sphere(&mut k, Point3::ZERO, 10.0).unwrap();
    let (s, dom) = face_of(&b, |_, _| true);
    let tau = std::f64::consts::TAU;
    let equator = through(65, |t| {
        Vec3::new(10.0 * (tau * t).cos(), 10.0 * (tau * t).sin(), 0.0)
    });
    let (z, r) = (10.0 * 0.3_f64.sin(), 10.0 * 0.3_f64.cos());
    for side in [Side::Left, Side::Right] {
        let o = geodesic_offset(
            &k,
            &s,
            &dom,
            &equator,
            &equator.natural_interval(),
            3.0,
            side,
            1e-5,
        )
        .unwrap();
        for p in along(&k, &o.curve) {
            assert!((p.z.abs() - z).abs() <= 1e-5, "{p:?}: z {z}");
            assert!((p.x.hypot(p.y) - r).abs() <= 1e-5, "{p:?}: r {r}");
        }
    }
}

#[test]
fn an_offset_past_the_curves_radius_folds_and_is_refused() {
    let mut k = Kernel::default();
    let b = create_plane(&mut k, Point3::ZERO, 20.0, 10.0).unwrap();
    let (s, dom) = face_of(&b, |_, _| true);
    // A circle of radius 4 about (10, 5): inward by 3 it is the circle of radius 1; by 6 the
    // geodesics cross the centre and the ends run back (the circle of radius 2, the other way).
    let tau = std::f64::consts::TAU;
    let circle = through(65, |t| {
        Vec3::new(
            10.0 + 4.0 * (tau * t).cos(),
            5.0 + 4.0 * (tau * t).sin(),
            0.0,
        )
    });
    let ivl = circle.natural_interval();
    let inward = [Side::Left, Side::Right]
        .into_iter()
        .find(|side| {
            geodesic_offset(&k, &s, &dom, &circle, &ivl, 3.0, *side, 1e-5).is_ok_and(|o| {
                along(&k, &o.curve)
                    .iter()
                    .all(|p| ((p.x - 10.0).hypot(p.y - 5.0) - 1.0).abs() < 1e-4)
            })
        })
        .expect("one side runs inward");
    let e = geodesic_offset(&k, &s, &dom, &circle, &ivl, 6.0, inward, 1e-5).unwrap_err();
    assert!(e.to_string().contains("folds over itself"), "{e}");
}

/// On a cylinder of radius 1 the helix `(cos θ, sin θ, θ/2 − 1)`, θ in [π + 0.2, 3π/2 + 0.2],
/// offset `π√5` along the wall: unrolled, the helix runs along (1, 1/2) in (θ, z) and the offset
/// moves it by (−π, 2π) square to that, so the offset is the helix
/// `(−cos θ, −sin θ, θ/2 − 1 + 2π)` on the far side of the cylinder. It does not fold, though its direction in space there runs
/// against the source's (dot product −3/4 of their unscaled tangents).
#[test]
fn a_helix_round_a_cylinder_offsets_to_the_far_side_without_folding() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::ZERO, 1.0, 10.0).unwrap();
    let (s, dom) = face_of(&b, |s, d| {
        s.drop_point::<NoObject>(
            &k,
            Vec3::new(1.0, 0.0, 5.0),
            d,
            &DropPointOptions::default(),
        )
        .is_ok_and(|f| f.success && f.gap <= 1e-9)
    });
    let (pi, quarter) = (std::f64::consts::PI, std::f64::consts::FRAC_PI_2);
    let lo = pi + 0.2;
    let helix = through(65, |t| {
        let th = lo + quarter * t;
        Vec3::new(th.cos(), th.sin(), th / 2.0 - 1.0)
    });
    let ivl = helix.natural_interval();
    let d = pi * 5f64.sqrt();
    // The side going down leaves the wall below z = 0; the side going up is the closed form.
    let both: Vec<_> = [Side::Left, Side::Right]
        .into_iter()
        .map(|side| geodesic_offset(&k, &s, &dom, &helix, &ivl, d, side, 1e-5))
        .collect();
    let up: Vec<&GeodesicOffset> = both.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(up.len(), 1, "{both:?}");
    let tau = std::f64::consts::TAU;
    for p in along(&k, &up[0].curve) {
        assert!((p.x.hypot(p.y) - 1.0).abs() <= 1e-5, "{p:?}");
        // the offset point's own angle θ' = θ − π, so z = (θ' + π)/2 − 1 + 2π
        let th = p.y.atan2(p.x);
        assert!((0.2 - 1e-5..=quarter + 0.2 + 1e-5).contains(&th), "{p:?}");
        assert!((p.z - ((th + pi) / 2.0 - 1.0 + tau)).abs() <= 1e-5, "{p:?}");
    }
}

/// A geodesic that starts on a periodic surface's seam and runs out of the parameter box there
/// reads the spans across the seam: the helix `(cos θ, sin θ, θ/2 − 2)`, θ in [2π − 0.5, 2π],
/// offset `0.2√5` toward smaller θ is the same helix moved by (−0.2, 0.4) in (θ, z), so
/// `z = θ'/2 − 1.5` at its own angle θ' in [2π − 0.7, 2π − 0.2]. Its last station lies on the
/// seam (parameter 0) and its geodesic runs back across it. The other side's geodesics cross
/// the seam on their way.
#[test]
fn a_geodesic_from_the_seam_reads_the_spans_across_it() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::ZERO, 1.0, 10.0).unwrap();
    let (s, dom) = face_of(&b, |s, d| {
        s.drop_point::<NoObject>(
            &k,
            Vec3::new(1.0, 0.0, 5.0),
            d,
            &DropPointOptions::default(),
        )
        .is_ok_and(|f| f.success && f.gap <= 1e-9)
    });
    let tau = std::f64::consts::TAU;
    let helix = through(65, |t| {
        let th = tau - 0.5 + 0.5 * t;
        Vec3::new(th.cos(), th.sin(), th / 2.0 - 2.0)
    });
    let ivl = helix.natural_interval();
    // back: θ' = θ − 0.2 in [2π − 0.7, 2π − 0.2], z = θ'/2 − 1.5; on: θ' = θ + 0.2 in
    // [2π − 0.3, 2π + 0.2] (across the seam), z = θ'/2 − 2.5
    let fits = |o: &GeodesicOffset, lo: f64, rise: f64| {
        along(&k, &o.curve).iter().all(|p| {
            // the angle taken within half a turn of the range's middle
            let mid = lo + 0.25;
            let th = mid + (p.y.atan2(p.x) - mid + 0.5 * tau).rem_euclid(tau) - 0.5 * tau;
            (p.x.hypot(p.y) - 1.0).abs() <= 1e-5
                && (lo - 1e-5..=lo + 0.5 + 1e-5).contains(&th)
                && (p.z - (th / 2.0 + rise)).abs() <= 1e-5
        })
    };
    let offsets: Vec<GeodesicOffset> = [Side::Left, Side::Right]
        .into_iter()
        .map(|side| geodesic_offset(&k, &s, &dom, &helix, &ivl, 0.2 * 5f64.sqrt(), side, 1e-5))
        .collect::<Result<_>>()
        .unwrap();
    let back = offsets.iter().filter(|o| fits(o, tau - 0.7, -1.5)).count();
    let on = offsets.iter().filter(|o| fits(o, tau - 0.3, -2.5)).count();
    assert_eq!((back, on), (1, 1));
    // a helix on a cylinder is smooth on both sides of the seam: the first stations do, as on
    // a span (no step reads across the seam)
    for o in &offsets {
        assert!(o.stations == 17 && o.worst <= 1e-5, "{o:?}");
    }
}

#[test]
fn an_offset_off_the_surface_and_bad_inputs_are_refused() {
    let mut k = Kernel::default();
    let b = create_plane(&mut k, Point3::ZERO, 20.0, 10.0).unwrap();
    let (s, dom) = face_of(&b, |_, _| true);
    // 1 from the sheet's edge at y = 10: one side leaves it at 3
    let near_edge = through(5, |t| Vec3::new(2.0 + 16.0 * t, 9.0, 0.0));
    let left = geodesic_offset(
        &k,
        &s,
        &dom,
        &near_edge,
        &near_edge.natural_interval(),
        3.0,
        Side::Left,
        1e-6,
    );
    let right = geodesic_offset(
        &k,
        &s,
        &dom,
        &near_edge,
        &near_edge.natural_interval(),
        3.0,
        Side::Right,
        1e-6,
    );
    assert!(
        matches!(
            (&left, &right),
            (Err(SolidError::InvalidInput { .. }), Ok(_))
                | (Ok(_), Err(SolidError::InvalidInput { .. }))
        ),
        "{left:?} {right:?}"
    );
    let line = through(5, |t| Vec3::new(2.0 + 16.0 * t, 5.0, 0.0));
    for (d, tol) in [
        (0.0, 1e-6),
        (-1.0, 1e-6),
        (f64::NAN, 1e-6),
        (1.0, 0.0),
        (1.0, f64::INFINITY),
    ] {
        assert!(matches!(
            geodesic_offset(
                &k,
                &s,
                &dom,
                &line,
                &line.natural_interval(),
                d,
                Side::Left,
                tol
            ),
            Err(SolidError::InvalidInput { .. })
        ));
    }
    let point = Extent1d::from_value(0.5);
    assert!(matches!(
        geodesic_offset(&k, &s, &dom, &line, &point, 1.0, Side::Left, 1e-6),
        Err(SolidError::InvalidInput { .. })
    ));
}
