//! Where two bodies' surfaces cross: the cases of Form Studio's
//! test/kernel/intersect.test.mjs that stand on exact surfaces alone.

// Test code, as Solidcore's test files: indexing and unwrap fail the test.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use ssi::piece::{norm, sub, V3};
use ssi::{intersect_bodies, shapes, Intersection, Options, Patch};

const S: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// An exact sphere: one rational biquadratic B-spline, u round z from +x, v from the south pole; normals out.
fn nurbs_sphere(center: V3, r: f64) -> Patch {
    let ring = [
        [1.0, 0.0, 1.0],
        [1.0, 1.0, S],
        [0.0, 1.0, 1.0],
        [-1.0, 1.0, S],
        [-1.0, 0.0, 1.0],
        [-1.0, -1.0, S],
        [0.0, -1.0, 1.0],
        [1.0, -1.0, S],
        [1.0, 0.0, 1.0],
    ];
    let profile = [
        [0.0, -1.0, 1.0],
        [1.0, -1.0, S],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, S],
        [0.0, 1.0, 1.0],
    ];
    let mut poles = Vec::new();
    let mut weights = Vec::new();
    for [cx, cy, wu] in ring {
        for [px, pz, wv] in profile {
            poles.push([
                center[0] + r * cx * px,
                center[1] + r * cy * px,
                center[2] + r * pz,
            ]);
            weights.push(wu * wv);
        }
    }
    Patch {
        u_degree: 2,
        v_degree: 2,
        u_count: 9,
        v_count: 5,
        poles,
        weights: Some(weights),
        u_knots: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        v_knots: vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        analytic: None,
    }
}

/// A bilinear square through `origin` spanned by `u` and `v`; its normal is u × v.
fn square(origin: V3, u: V3, v: V3) -> Patch {
    let at = |a: f64, b: f64| [0, 1, 2].map(|k| origin[k] + a * u[k] + b * v[k]);
    Patch {
        u_degree: 1,
        v_degree: 1,
        u_count: 2,
        v_count: 2,
        poles: vec![at(0.0, 0.0), at(0.0, 1.0), at(1.0, 0.0), at(1.0, 1.0)],
        weights: None,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        analytic: None,
    }
}

/// The largest distance from a reported point to either body's surface at its reported parameters.
fn worst_residual(result: &Intersection, a: &[Patch], b: &[Patch]) -> f64 {
    let mut worst: f64 = 0.0;
    for curve in &result.curves {
        for q in &curve.points {
            worst = worst.max(norm(sub(a[q.a.patch].evaluate(q.a.u, q.a.v).unwrap(), q.p)));
            worst = worst.max(norm(sub(b[q.b.patch].evaluate(q.b.u, q.b.v).unwrap(), q.p)));
        }
    }
    worst
}

#[test]
fn bezier_pieces_reproduce_the_bspline_they_came_from_rational_and_clamped_or_not() {
    let sphere = nurbs_sphere([0.2, -0.1, 0.3], 1.5);
    // An unclamped cubic on both sides: the domain starts and ends inside the knot vector.
    let mut poles = Vec::new();
    for i in 0..6 {
        for j in 0..5 {
            let (x, y) = (f64::from(i), f64::from(j));
            poles.push([x, y, (x + 2.0 * y).sin()]);
        }
    }
    let unclamped = Patch {
        u_degree: 3,
        v_degree: 3,
        u_count: 6,
        v_count: 5,
        poles,
        weights: None,
        u_knots: vec![-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
        v_knots: vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.5, 1.5, 1.5],
        analytic: None,
    };
    for patch in [&sphere, &unclamped] {
        for piece in patch.bezier_pieces(0).unwrap() {
            for (s, t) in [(0.0, 0.0), (0.3, 0.7), (1.0, 0.5), (0.9, 1.0)] {
                let u = piece.u0 + s * (piece.u1 - piece.u0);
                let v = piece.v0 + t * (piece.v1 - piece.v0);
                assert!(
                    norm(sub(
                        piece.evaluate(s, t).point,
                        patch.evaluate(u, v).unwrap()
                    )) < 1e-12
                );
            }
        }
    }
    assert_eq!(sphere.bezier_pieces(0).unwrap().len(), 8);
    // u spans [0, 3] in three, v spans [0, 1] in two (0.4 is its one interior knot).
    assert_eq!(unclamped.bezier_pieces(0).unwrap().len(), 3 * 2);
}

#[test]
fn a_plane_through_an_exact_sphere_crosses_it_in_one_circle_every_point_on_both() {
    let z = 0.3;
    let sphere = [nurbs_sphere([0.0; 3], 1.0)];
    let plane = [square([-2.0, -2.0, z], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0])];
    let result = intersect_bodies(&sphere, &plane, &Options::default()).unwrap();
    assert_eq!(result.curves.len(), 1);
    let curve = &result.curves[0];
    assert!(curve.closed);
    for q in &curve.points {
        assert!((norm(q.p) - 1.0).abs() < 1e-10);
        assert!((q.p[2] - z).abs() < 1e-10);
    }
    assert!(worst_residual(&result, &sphere, &plane) < 1e-10);
    // The polyline's length is the circle's, less the chords' sag.
    let radius = (1.0 - z * z).sqrt();
    let n = curve.points.len();
    let length: f64 = (0..n)
        .map(|i| norm(sub(curve.points[(i + 1) % n].p, curve.points[i].p)))
        .sum();
    assert!(
        (length / (2.0 * std::f64::consts::PI * radius) - 1.0).abs() < 1e-3,
        "length {length}"
    );
    // It runs along nA × nB: with the sphere's normal out and the plane's up, clockwise seen from above.
    let east = (0..n)
        .max_by(|&i, &j| curve.points[i].p[0].total_cmp(&curve.points[j].p[0]))
        .unwrap();
    assert!(curve.points[(east + 1) % n].p[1] < curve.points[east].p[1]);
}

#[test]
fn two_exact_spheres_meet_in_the_circle_x_one_half_across_both_seams_and_every_span() {
    let a = [nurbs_sphere([0.0; 3], 1.0)];
    let b = [nurbs_sphere([1.0, 0.0, 0.0], 1.0)];
    let result = intersect_bodies(&a, &b, &Options::default()).unwrap();
    assert_eq!(result.curves.len(), 1);
    let curve = &result.curves[0];
    assert!(curve.closed);
    for q in &curve.points {
        assert!((q.p[0] - 0.5).abs() < 1e-10);
        assert!((q.p[1].hypot(q.p[2]) - 0.75_f64.sqrt()).abs() < 1e-10);
    }
    assert!(worst_residual(&result, &a, &b) < 1e-10);
    // Seen from outside A, B's inside lies to the left: T × nA points out of B.
    let (first, second) = (curve.points[0].p, curve.points[1].p);
    let t = sub(second, first);
    let right = ssi::piece::cross(t, first);
    assert!(right[0] < 0.0, "moving right on A leaves B, toward -x");
}

#[test]
fn bodies_that_do_not_meet_give_no_curve_and_one_inside_the_other_none_either() {
    let options = Options::default();
    let apart = intersect_bodies(
        &[nurbs_sphere([0.0; 3], 1.0)],
        &[nurbs_sphere([3.0, 0.0, 0.0], 1.0)],
        &options,
    )
    .unwrap();
    assert!(apart.curves.is_empty());
    let inside = intersect_bodies(
        &[nurbs_sphere([0.0; 3], 1.0)],
        &[nurbs_sphere([0.1, 0.0, 0.0], 0.5)],
        &options,
    )
    .unwrap();
    assert!(inside.curves.is_empty());
}

#[test]
fn a_nurbs_sphere_and_square_that_touch_meet_in_one_point_through_their_implicit_forms() {
    let sphere = [nurbs_sphere([0.0; 3], 1.0)];
    let tangent = [square([1.0, -2.0, -2.0], [0.0, 0.0, 4.0], [0.0, 4.0, 0.0])];
    let result = intersect_bodies(&sphere, &tangent, &Options::default()).unwrap();
    assert!(result.curves.is_empty(), "{:?}", result.curves.len());
    assert_eq!(result.points.len(), 1);
    assert!(
        norm(sub(result.points[0].p, [1.0, 0.0, 0.0])) < 1e-9,
        "{:?}",
        result.points[0].p
    );
}

/// A bicubic dome: z = k B(s) B(t) with B(s) = 3 s (1 - s) (1 - s / 2),
/// highest where s = t = 1 - 1 / sqrt(3) (z = k / 3), over an irregular net
/// in x and y, so of the generic implicit degree 18, past any implicit form
/// fitted; mirrored through z = `mirror` when given.
fn dome(k: f64, mirror: Option<f64>) -> Patch {
    let b = [0.0, 1.0, 0.5, 0.0];
    let mut poles = Vec::new();
    for i in 0..4 {
        for j in 0..4 {
            let z = k * b[i] * b[j];
            let (x, y) = (
                i as f64 / 3.0 + 0.04 * ((i * 7 + j * 3) % 5) as f64,
                j as f64 / 3.0 + 0.03 * ((i * 5 + j * 11) % 4) as f64,
            );
            poles.push([x, y, mirror.map_or(z, |m| 2.0 * m - z)]);
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    Patch {
        u_degree: 3,
        v_degree: 3,
        u_count: 4,
        v_count: 4,
        poles,
        weights: None,
        u_knots: knots.clone(),
        v_knots: knots,
        analytic: None,
    }
}

/// Two domes of no implicit form within reach, touching at their peaks: the
/// touch point, found on a local field of the separation (`trace2d::trace_local`).
#[test]
fn free_form_surfaces_that_touch_meet_at_their_one_point() {
    let (top, peak) = (1.0 / 3.0, 1.0 - 1.0 / 3.0_f64.sqrt());
    let (a, b) = ([dome(1.0, None)], [dome(1.0, Some(top))]);
    let touch = a[0].evaluate(peak, peak).unwrap();
    let result = intersect_bodies(&a, &b, &Options::default()).unwrap();
    assert!(result.curves.is_empty(), "{:?}", result.curves);
    assert_eq!(result.points.len(), 1);
    let q = result.points[0];
    assert!(norm(sub(q.p, touch)) < 1e-9, "{:?} against {touch:?}", q.p);
    for param in [q.a, q.b] {
        assert!((param.u - peak).abs() < 1e-8 && (param.v - peak).abs() < 1e-8);
    }
}

/// A square on the plane z = height, with the plane as its analytic surface.
fn square_on_plane(corner: [f64; 2], side: f64, height: f64) -> Patch {
    use ssi::geom3::{Frame, V3 as Exact3};
    let mut patch = square(
        [corner[0], corner[1], height],
        [side, 0.0, 0.0],
        [0.0, side, 0.0],
    );
    let f = Frame {
        o: Exact3::new(0.0, 0.0, height),
        x: Exact3::new(1.0, 0.0, 0.0),
        z: Exact3::new(0.0, 0.0, 1.0),
    };
    patch.analytic = Some(ssi::surface3::Surf::Plane { f });
    patch
}

#[test]
fn patches_on_one_plane_meet_in_the_boundary_of_the_region_they_share() {
    let a = [square_on_plane([0.0, 0.0], 2.0, 0.0)];
    let b = [square_on_plane([1.0, 1.0], 2.0, 0.0)];
    let result = intersect_bodies(&a, &b, &Options::default()).unwrap();
    assert_eq!(result.curves.len(), 1);
    let curve = &result.curves[0];
    assert!(curve.closed);
    // Every point on the boundary of [1, 2]^2, and every corner of it reached.
    for q in &curve.points {
        assert!(
            ((q.p[0] - 1.5).abs().max((q.p[1] - 1.5).abs()) - 0.5).abs() < 1e-12,
            "{:?}",
            q.p
        );
        assert!(q.p[2].abs() < 1e-15);
    }
    for corner in [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]] {
        assert!(
            curve
                .points
                .iter()
                .any(|q| (q.p[0] - corner[0]).abs() + (q.p[1] - corner[1]).abs() < 1e-12),
            "corner {corner:?}"
        );
    }
    assert!(worst_residual(&result, &a, &b) < 1e-12);
}

#[test]
fn parallel_planes_1e_7_apart_are_proved_apart_not_refused() {
    let a = [square_on_plane([0.0, 0.0], 2.0, 0.0)];
    let b = [square_on_plane([0.5, 0.5], 2.0, 1e-7)];
    assert!(intersect_bodies(&a, &b, &Options::default())
        .unwrap()
        .curves
        .is_empty());
}

/// The one closed curve of `result`, every point at height `z` and at `radius`
/// from the z axis, to `within` of it.
fn one_loop_about_z(result: &Intersection, z: f64, radius: f64, within: f64) {
    assert_eq!(result.curves.len(), 1);
    assert!(result.curves[0].closed);
    assert!(result.points.is_empty());
    for q in &result.curves[0].points {
        assert!(
            (q.p[0].hypot(q.p[1]) / radius - 1.0).abs() < within,
            "at {:?}",
            q.p
        );
        assert!((q.p[2] - z).abs() < 1e-20);
    }
}

#[test]
fn a_plane_5e_15_under_a_spheres_pole_cuts_its_loop_of_radius_1e_7() {
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let d = 0.999_999_999_999_995;
    let plane = [shapes::plane([0.0; 3], x, y, [-1.0, 1.0], [-1.0, 1.0])];
    let sphere = [shapes::sphere(
        [0.0, 0.0, d],
        x,
        y,
        1.0,
        [0.0, 360.0],
        [-90.0, 90.0],
    )];
    let result = intersect_bodies(&plane, &sphere, &Options::default()).unwrap();
    one_loop_about_z(&result, 0.0, ((1.0 - d) * (1.0 + d)).sqrt(), 1e-9);
    assert!(worst_residual(&result, &plane, &sphere) < 1e-15);
}

#[test]
fn a_plane_1e_14_over_a_paraboloids_apex_cuts_its_loop_of_radius_1e_7() {
    // z = x^2 + y^2 over [-1, 1]^2, one biquadratic Bézier patch.
    let poles = vec![
        [-1.0, -1.0, 2.0],
        [-1.0, 0.0, 0.0],
        [-1.0, 1.0, 2.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, -2.0],
        [0.0, 1.0, 0.0],
        [1.0, -1.0, 2.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 2.0],
    ];
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let paraboloid = [Patch {
        u_degree: 2,
        v_degree: 2,
        u_count: 3,
        v_count: 3,
        poles,
        weights: None,
        u_knots: knots.clone(),
        v_knots: knots,
        analytic: None,
    }];
    let h = 1e-14;
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let plane = [shapes::plane([0.0, 0.0, h], x, y, [-1.0, 1.0], [-1.0, 1.0])];
    let result = intersect_bodies(&paraboloid, &plane, &Options::default()).unwrap();
    // The form's value at the apex is known to about 1% (rounding in terms
    // of 0.1, for a value of 1e-15), the radius to half that.
    one_loop_about_z(&result, h, h.sqrt(), 1e-2);
    assert!(worst_residual(&result, &paraboloid, &plane) < 1e-15);
}
