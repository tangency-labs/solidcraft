//! `surface::recognise`: a NURBS form of a plane, cylinder, cone, sphere or torus is
//! recognised as that surface with its numbers, wherever it lies and however its frame is
//! tilted; a form moved off its shape is recognised only within a tolerance that covers the
//! move; a free-form surface is not recognised.
//!
//! The forms are quarter surfaces of revolution about an axis tilted off every world axis: a
//! meridian (a line or a rational quadratic quarter circle) swept a quarter turn by the
//! rational quadratic quarter circle, so each is exactly its shape.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::implicit::Implicit;
use solidcore::surface::recognise::analytic_of_form;

/// A frame tilted off every world axis: its axis along (1, 2, 3), its x square to it.
fn tilted() -> (Vec3, Vec3, Vec3) {
    let z = Vec3::new(1.0, 2.0, 3.0);
    let z = z * (1.0 / z.length());
    let x = Vec3::new(3.0, -1.0, 0.0);
    let x = x - z * x.dot(z);
    let x = x * (1.0 / x.length());
    (x, z.cross(x), z)
}

const W: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The meridian `(rho, height, weight)` control points, of degree `degree` with clamped knots
/// on one span, swept a quarter turn about the tilted axis through `origin`: rows along the
/// meridian, columns the quarter circle.
fn revolve(origin: Vec3, meridian: &[(f64, f64, f64)], degree: usize) -> BSplineSurface {
    let (x, y, z) = tilted();
    let rows: Vec<Vec<CPoint>> = meridian
        .iter()
        .map(|&(rho, h, wm)| {
            [(1.0, 0.0, 1.0), (1.0, 1.0, W), (0.0, 1.0, 1.0)]
                .iter()
                .map(|&(a, b, wa)| {
                    let p = origin + x * (rho * a) + y * (rho * b) + z * h;
                    let wt = wm * wa;
                    CPoint::new(p.x * wt, p.y * wt, p.z * wt, wt)
                })
                .collect()
        })
        .collect();
    let mut u_knots = vec![0.0; degree + 1];
    u_knots.extend(vec![1.0; degree + 1]);
    let nurb = solidcore::nurbs::srf_basic::Surface::new(
        rows,
        degree,
        2,
        u_knots,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    )
    .unwrap();
    BSplineSurface::from_nurb(nurb, true)
}

/// The meridian quarter circle about `(rho0, h0)` of radius `r`, from its outer point up.
fn arc(rho0: f64, h0: f64, r: f64) -> Vec<(f64, f64, f64)> {
    vec![
        (rho0 + r, h0, 1.0),
        (rho0 + r, h0 + r, W),
        (rho0, h0 + r, 1.0),
    ]
}

fn close(a: f64, b: f64, size: f64) -> bool {
    (a - b).abs() <= 1e-9 * size
}

fn axis_along(a: Vec3) -> bool {
    let (_, _, z) = tilted();
    (a.dot(z).abs() - 1.0).abs() <= 1e-9
}

/// The distance from `p` to the tilted axis through `origin`.
fn off_axis(p: Vec3, origin: Vec3) -> f64 {
    let (_, _, z) = tilted();
    let d = p - origin;
    (d - z * d.dot(z)).length()
}

#[test]
fn a_cylinder_form_is_its_cylinder_far_from_the_origin() {
    let k = Kernel::default();
    for origin in [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0e3, -2.0e3, 3.0e3)] {
        let form = revolve(origin, &[(5.0, 0.0, 1.0), (5.0, 8.0, 1.0)], 1);
        let r = analytic_of_form(&k, &form, 1e-6).unwrap().unwrap();
        assert!(r.exact);
        let Implicit::Cone {
            origin: o,
            axis,
            radius,
            tan_angle,
        } = r.implicit
        else {
            panic!("not a cylinder: {r:?}");
        };
        assert!(tan_angle == 0.0 && close(radius, 5.0, 10.0) && axis_along(axis));
        assert!(off_axis(o, origin) <= 1e-9 * (1.0 + origin.length()));
    }
}

#[test]
fn a_cone_form_is_its_cone() {
    let k = Kernel::default();
    let origin = Vec3::new(-40.0, 7.0, 12.0);
    let form = revolve(origin, &[(5.0, 0.0, 1.0), (3.0, 8.0, 1.0)], 1);
    let r = analytic_of_form(&k, &form, 1e-6).unwrap().unwrap();
    assert!(r.exact);
    let Implicit::Cone {
        origin: o,
        axis,
        radius,
        tan_angle,
    } = r.implicit
    else {
        panic!("not a cone: {r:?}");
    };
    assert!(axis_along(axis));
    // the radius shrinks by 2 over 8 along the axis, whichever way the axis points
    let (_, _, z) = tilted();
    let t = tan_angle * axis.dot(z).signum();
    assert!(close(t, -0.25, 1.0), "{t}");
    // the radius at the origin is the cone's there
    let h = (o - origin).dot(z);
    assert!(close(radius, 5.0 - 0.25 * h, 10.0), "{radius} at {h}");
}

#[test]
fn a_sphere_form_through_its_pole_is_its_sphere() {
    let k = Kernel::default();
    let centre = Vec3::new(20.0, 30.0, -10.0);
    let form = revolve(centre, &arc(0.0, 0.0, 4.0), 2);
    let r = analytic_of_form(&k, &form, 1e-6).unwrap().unwrap();
    let Implicit::Sphere { centre: c, radius } = r.implicit else {
        panic!("not a sphere: {r:?}");
    };
    assert!(
        (c - centre).length() <= 1e-8 && close(radius, 4.0, 10.0),
        "{r:?}"
    );
}

#[test]
fn a_torus_form_is_its_torus() {
    let k = Kernel::default();
    let centre = Vec3::new(5.0, -5.0, 100.0);
    let form = revolve(centre, &arc(10.0, 0.0, 2.0), 2);
    let r = analytic_of_form(&k, &form, 1e-6).unwrap().unwrap();
    let Implicit::Torus {
        centre: c,
        axis,
        major,
        minor,
    } = r.implicit
    else {
        panic!("not a torus: {r:?}");
    };
    assert!(axis_along(axis) && (c - centre).length() <= 1e-8, "{r:?}");
    assert!(close(major, 10.0, 10.0) && close(minor, 2.0, 10.0), "{r:?}");
}

#[test]
fn a_flat_form_is_its_plane() {
    let k = Kernel::default();
    // a cone of half-angle 90 degrees: an annulus in the plane square to the axis
    let origin = Vec3::new(3.0, 4.0, 5.0);
    let form = revolve(origin, &[(2.0, 0.0, 1.0), (6.0, 0.0, 1.0)], 1);
    let r = analytic_of_form(&k, &form, 1e-6).unwrap().unwrap();
    let Implicit::Plane { origin: o, normal } = r.implicit else {
        panic!("not a plane: {r:?}");
    };
    let (_, _, z) = tilted();
    assert!(
        r.exact && axis_along(normal) && (o - origin).dot(z).abs() <= 1e-9,
        "{r:?}"
    );
}

#[test]
fn a_form_off_its_cylinder_is_one_only_within_a_tolerance_covering_the_move() {
    let k = Kernel::default();
    // the cylinder's middle meridian row moved 1e-5 outwards: a degree 2 meridian whose middle
    // control point bulges, so the surface leaves the cylinder by about half that
    let bulge = 1e-5;
    let form = revolve(
        Vec3::new(0.0, 0.0, 0.0),
        &[(5.0, 0.0, 1.0), (5.0 + bulge, 4.0, 1.0), (5.0, 8.0, 1.0)],
        2,
    );
    let loose = analytic_of_form(&k, &form, 1e-4).unwrap().unwrap();
    assert!(
        !loose.exact && matches!(loose.implicit, Implicit::Cone { .. }),
        "{loose:?}"
    );
    // tighter than the bulge, it is no cylinder (a gently curved meridian may still be a
    // circle's arc to within it, a torus)
    let tight = analytic_of_form(&k, &form, 1e-7).unwrap();
    assert!(
        !matches!(
            tight.as_ref().map(|r| r.implicit),
            Some(Implicit::Cone { .. })
        ),
        "{tight:?}"
    );
}

#[test]
fn a_free_form_is_not_recognised() {
    let k = Kernel::default();
    // a meridian that is no line, circle or cone: a cubic wiggle
    let form = revolve(
        Vec3::new(0.0, 0.0, 0.0),
        &[
            (5.0, 0.0, 1.0),
            (7.0, 2.0, 1.0),
            (3.0, 5.0, 1.0),
            (6.0, 8.0, 1.0),
        ],
        3,
    );
    assert!(analytic_of_form(&k, &form, 1e-3).unwrap().is_none());
}
