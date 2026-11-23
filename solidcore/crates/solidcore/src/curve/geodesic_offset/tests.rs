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
