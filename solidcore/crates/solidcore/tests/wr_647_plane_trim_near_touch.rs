//! WR-647: a curve that comes near a plane domain's boundary without crossing it is not cut
//! there (the goldens row `surface.plane_p2` gen0007).
//!
//! `TrimCurveToPlaneDomain` intersects the curve with the four boundary lines of the
//! domain at tolerance 0.5. The quadratic B-spline below runs about 0.1 off the plane and,
//! on its middle span [1/3, 2/3], comes within 0.19 in 3D (0.16 in the plane) of the
//! boundary `v = v_min`. Upstream reports that near approach as a coincidence range,
//! [0.3958, 0.40625], and the trim cuts the curve at both of its ends: two cuts that are
//! no boundary crossing. Decided on the merged range, the approach is a touch, one point,
//! and those two cuts go. The other cuts, at 0.5 and 0.859375, are upstream's too: the
//! ends of the curve's coincidence range with the boundary `u = u_min` (the curve stays
//! within 0.5 of that line there, and crosses it at t = 0.8717), unchanged here.
//!
//! The certificate is closed form. The plane is affine in (u, v), so the curve's (u, v)
//! image is the quadratic B-spline of the control points' images. On [1/3, 2/3] its
//! Bezier control values (`u - u_min`, `u_max - u`, `v_max - v`) are all positive, so by the
//! convex hull the curve stays strictly inside those three bounds; `v - v_min` is a
//! quadratic in the span's Bezier parameter whose discriminant is negative (by far more
//! than rounding) with a positive leading coefficient, so it never vanishes. No boundary
//! line is crossed on [1/3, 2/3], so the cuts at 0.3958 and 0.40625 were at no crossing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::surface::plane::Plane;

const O: [f64; 3] = [-4.0, 9.630064601095626, -2.390561472614028];
const X: [f64; 3] = [0.0, 0.0, -1.0];
const Y: [f64; 3] = [0.0016738039767981102, -0.9999985991891425, 0.0];
const DOM: [f64; 4] = [
    6.4928671509140905,
    -0.31077522373115585,
    9.649072906582429,
    5.927852372039439,
];
const CPTS: [[f64; 3]; 5] = [
    [-4.098405892907581, 7.110273770139149, -10.733233013056338],
    [-4.100414482937467, 8.310287179160257, -10.208454650410058],
    [-4.10447798907518, 10.737991081072835, -9.20301298104388],
    [-4.08909546799864, 1.547847108561392, -9.444401996935865],
    [-4.099737980406884, 7.906117041970887, -8.009038861962402],
];
const KNOTS: [f64; 8] = [0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0];

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

#[test]
fn wr_647_a_curve_near_a_domain_boundary_is_not_cut_there() {
    let k = Kernel::default();
    let dom = Extent2d::from_bounds(DOM[0], DOM[1], DOM[2], DOM[3]);
    let mut plane = Plane::new(v3(O), v3(X), v3(Y), Vec2::new(1.0, 1.0), &dom);
    if plane.base().gw_nurb().is_none() {
        plane.make_nurb().unwrap();
    }
    let pw: Vec<CPoint> = CPTS
        .iter()
        .map(|q| CPoint::new(q[0], q[1], q[2], NOW))
        .collect();
    let curve = Curve::from(BSplineCurve::from_data(3, 4, 7, 2, &pw, &KNOTS).unwrap());

    // the plane is the affine map (u, v) -> O + u X + v Y (X, Y orthogonal; the unclamped
    // evaluation: `evaluate_point` clamps to the domain)
    let (o, x, y) = (v3(O), v3(X), v3(Y));
    assert!(x.dot(y).abs() < 1e-15);
    for uv in [[0.0, 0.0], [7.0, 0.0], [0.0, 3.0], [8.0, 5.0]] {
        let p = plane.evaluate_point_fast(Point2::new(uv[0], uv[1]));
        let q = o + x * uv[0] + y * uv[1];
        assert!(p.distance(q) < 1e-12, "{uv:?}: {p:?} {q:?}");
    }
    let uv_of = |p: Vec3| ((p - o).dot(x) / x.dot(x), (p - o).dot(y) / y.dot(y));

    // the middle span's Bezier control points in (u, v): b1 is the middle control point,
    // b0 and b2 the midpoints of the legs (uniform knots 1/3 apart)
    let q: Vec<(f64, f64)> = CPTS[1..4].iter().map(|&c| uv_of(v3(c))).collect();
    let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    let bez = [mid(q[0], q[1]), q[1], mid(q[1], q[2])];
    for b in bez {
        assert!(b.0 - DOM[0] > 0.3, "u - u_min {b:?}");
        assert!(DOM[2] - b.0 > 2.0, "u_max - u {b:?}");
        assert!(DOM[3] - b.1 > 2.0, "v_max - v {b:?}");
    }
    let [a0, a1, a2] = bez.map(|b| b.1 - DOM[1]);
    let lead = a0 - 2.0 * a1 + a2;
    let lin = 2.0 * (a1 - a0);
    let disc = lin * lin - 4.0 * lead * a0;
    assert!(
        lead > 5.0 && disc < -3.0,
        "v - v_min: lead {lead} disc {disc}"
    );

    let pieces = plane
        .trim_curve_to_plane_domain(&k, &dom, &curve, 0.5)
        .unwrap();
    let ivls: Vec<[f64; 2]> = pieces
        .iter()
        .map(|c| {
            let i = c.natural_interval();
            [i.min(), i.max()]
        })
        .collect();
    // upstream's pieces without the two cuts at the near approach (0.3958 and 0.40625)
    assert_eq!(
        ivls,
        vec![[0.0, 0.5], [0.5, 0.859375], [0.859375, 1.0]],
        "{ivls:?}"
    );
}
