//! The evaluator's tests: closed-form checks of every kind against its exact surface, the
//! partials against the orders below, curve jets, projection, poles and the NURBS forms.

#![allow(clippy::cast_lossless)] // small integer sample counts

use super::spline::orders;
use super::vec::VecExt;
use super::*;
use crate::math::Vec3;
use std::f64::consts::{FRAC_PI_2, TAU};

fn close(a: Vec3, b: Vec3, tol: f64) -> bool {
    a.dist(b) <= tol * (1.0 + a.len().max(b.len()))
}

fn a_frame() -> Frame {
    Frame {
        origin: [1., 2., 3.],
        x_axis: [1., 0., 0.],
        y_axis: [0., 1., 0.],
    }
}

/// A tilted frame: nothing lines up with the coordinate axes.
fn tilted() -> Frame {
    let c = std::f64::consts::FRAC_1_SQRT_2;
    Frame {
        origin: [1.5, -2.0, 0.75],
        x_axis: [c, c, 0.0],
        y_axis: [-c * 0.6, c * 0.6, 0.8],
    }
}

const IDENTITY: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

#[test]
fn sphere_frame_proves_both_poles_clear_of_all_edges() {
    let support = a_frame();
    let tilted = Frame {
        origin: support.origin,
        x_axis: [0.8, 0.0, -0.6],
        y_axis: [0.0, 1.0, 0.0],
    };
    let rims: Vec<_> = [-4.0, 4.0]
        .into_iter()
        .map(|z| Curve::Circle {
            frame: Frame {
                origin: [1.0, 2.0, 3.0 + z],
                ..support.clone()
            },
            radius: 3.0,
        })
        .collect();
    let edges: Vec<_> = rims.iter().map(|c| (c, [0.0, TAU], 1e-5)).collect();
    assert_eq!(
        form::sphere_frame(&tilted, 5.0, &support, 3.0, &edges, &[]),
        tilted
    );
    let near_angle = 0.6_f64.asin() + 0.001;
    let near = Frame {
        origin: support.origin,
        x_axis: [near_angle.cos(), 0.0, -near_angle.sin()],
        y_axis: [0.0, 1.0, 0.0],
    };
    assert_eq!(
        form::sphere_frame(&near, 5.0, &support, 3.0, &edges, &edges),
        near
    );
    assert_eq!(
        form::sphere_frame(&tilted, 5.0, &support, 3.0, &edges, &edges),
        support
    );
    assert_eq!(
        form::sphere_frame(&support, 5.0, &tilted, 3.0, &edges, &edges),
        support
    );
    assert_eq!(
        form::sphere_frame(&tilted, 5.0, &support, 0.0, &edges, &edges),
        tilted
    );
    assert_eq!(
        form::sphere_frame(&tilted, 5.0, &tilted, 3.0, &edges, &edges),
        tilted
    );
    // A candidate clear of the rims still must not put a pole on another boundary edge.
    let crossing = Curve::Line {
        start: [1.0, 2.0, -3.0],
        end: [1.0, 2.0, 9.0],
    };
    let mut blocked = edges;
    blocked.push((&crossing, [0.0, 1.0], 1e-5));
    assert_eq!(
        form::sphere_frame(&tilted, 5.0, &support, 3.0, &blocked, &blocked),
        tilted
    );
}

fn line(a: Vec3, b: Vec3) -> EvalCurve {
    EvalCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]],
    )
}

/// The exact surface of an analytic kind: the distance of `p` from it, its unit normal there
/// (up to sign) and its principal curvatures' magnitudes, sorted.
fn exact(s: &Surface, p: Vec3) -> (f64, Vec3, [f64; 2]) {
    let ax = |f: &Frame| {
        (
            Vec3::from_array(f.origin),
            Vec3::from_array(f.x_axis).cross(Vec3::from_array(f.y_axis)),
        )
    };
    let sorted = |mut k: [f64; 2]| {
        k.sort_by(f64::total_cmp);
        k
    };
    match s {
        Surface::Sphere { frame, radius } => {
            let d = p - Vec3::from_array(frame.origin);
            (
                (d.len() - radius).abs(),
                d * (1.0 / d.len()),
                [1.0 / radius; 2],
            )
        }
        Surface::Cylinder { frame, radius } => {
            let (o, z) = ax(frame);
            let q = p - o;
            let rad = q - z * q.dot(z);
            (
                (rad.len() - radius).abs(),
                rad * (1.0 / rad.len()),
                [0.0, 1.0 / radius],
            )
        }
        Surface::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => {
            let (o, z) = ax(frame);
            let slope = half_angle.tan();
            let q = p - o;
            let h = q.dot(z);
            let rad = q - z * h;
            let rho = rad.len();
            let k = (1.0 + slope * slope).sqrt();
            (
                (rho - (radius_at_origin + h * slope)).abs() / k,
                (rad * (1.0 / rho) - z * slope) * (1.0 / k),
                [0.0, 1.0 / (rho * k)],
            )
        }
        Surface::Torus {
            frame,
            major_radius,
            minor_radius,
            ..
        } => {
            let (o, z) = ax(frame);
            let q = p - o;
            let rad = q - z * q.dot(z);
            let rho = rad.len();
            let d = p - (o + rad * (major_radius / rho));
            let cosw = (rho - major_radius) / d.len();
            (
                (d.len() - minor_radius).abs(),
                d * (1.0 / d.len()),
                sorted([1.0 / minor_radius, (cosw / rho).abs()]),
            )
        }
        _ => panic!("not analytic: {s:?}"),
    }
}

/// The principal curvatures' magnitudes, sorted, from the first and second partials.
fn principal(su: Vec3, sv: Vec3, suu: Vec3, suv: Vec3, svv: Vec3) -> [f64; 2] {
    let n = su.cross(sv).normalized().unwrap();
    let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let (l, m, nn) = (suu.dot(n), suv.dot(n), svv.dot(n));
    let det = e * g - f * f;
    let (kg, km) = (
        (l * nn - m * m) / det,
        (e * nn - 2.0 * f * m + g * l) / (2.0 * det),
    );
    let disc = (km * km - kg).max(0.0).sqrt();
    let mut k = [(km - disc).abs(), (km + disc).abs()];
    k.sort_by(f64::total_cmp);
    k
}

fn analytic() -> Vec<(Surface, [[f64; 2]; 2], f64)> {
    let f = tilted();
    vec![
        (
            Surface::Sphere {
                frame: f.clone(),
                radius: 7.0,
            },
            [[0.3, 5.9], [-1.3, 1.2]],
            7.0,
        ),
        (
            Surface::Cylinder {
                frame: f.clone(),
                radius: 5.0,
            },
            [[0.2, 6.1], [-4.0, 9.0]],
            5.0,
        ),
        (
            Surface::Cone {
                frame: f.clone(),
                radius_at_origin: 4.0,
                half_angle: 0.3,
            },
            [[0.1, 4.0], [-2.0, 8.0]],
            4.0,
        ),
        (
            Surface::Torus {
                frame: f,
                major_radius: 10.0,
                minor_radius: 3.0,
                tube_start: 0.4,
            },
            [[0.0, 6.2], [0.1, 6.0]],
            10.0,
        ),
    ]
}

fn grid(dom: [[f64; 2]; 2], n: usize) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (a, b) = ((i as f64 + 0.317) / n as f64, (j as f64 + 0.618) / n as f64);
            out.push([
                dom[0][0] + a * (dom[0][1] - dom[0][0]),
                dom[1][0] + b * (dom[1][1] - dom[1][0]),
            ]);
        }
    }
    out
}

/// Each analytic kind, in closed form and as its exact NURBS form on NLib, lies on its exact
/// surface with its exact normal and principal curvatures, to rounding. (Bench, 160 × 160
/// samples: point ≤ 1.1e-15, normal ≤ 2.7e-15 rad, curvature ≤ 1.9e-14 per size for every
/// copy measured; the sphere's umbilic curvature is tested looser for the square root of a
/// vanishing discriminant in `principal`.)
#[test]
fn closed_forms_and_exact_forms_lie_on_their_surfaces() {
    for (s, dom, size) in analytic() {
        let (form, fdom) = form::surface(&s, dom).unwrap();
        let curvature_tol = if matches!(s, Surface::Sphere { .. }) {
            1e-7
        } else {
            1e-13
        };
        for (e, d) in [
            (SurfaceEval::new(&s).unwrap(), dom),
            (SurfaceEval::new(&form).unwrap(), fdom),
        ] {
            for uv in grid(d, 24) {
                let p = e.partials(uv[0], uv[1]).unwrap();
                let (dist, n, k) = exact(&s, p[0][0]);
                assert!(dist <= 4e-15 * size, "{s:?} at {uv:?}: off by {dist}");
                let m = p[1][0].cross(p[0][1]).normalized().unwrap();
                assert!(m.cross(n).len() <= 1e-14, "{s:?} at {uv:?}: normal");
                let kk = principal(p[1][0], p[0][1], p[2][0], p[1][1], p[0][2]);
                for (a, b) in kk.iter().zip(k) {
                    assert!(
                        (a - b).abs() * size <= curvature_tol,
                        "{s:?} at {uv:?}: curvature {kk:?} vs {k:?}"
                    );
                }
                let (q, su, sv) = e.derivs(uv[0], uv[1]).unwrap();
                let (suu, suv, svv) = e.second(uv[0], uv[1]).unwrap();
                for (a, b) in [
                    (q, p[0][0]),
                    (su, p[1][0]),
                    (sv, p[0][1]),
                    (suu, p[2][0]),
                    (suv, p[1][1]),
                    (svv, p[0][2]),
                ] {
                    assert!(close(a, b, 1e-13), "{s:?}: {a:?} vs {b:?}");
                }
            }
        }
    }
}

/// Full-turn recognition must not replace an invalid range with one valid turn.
#[test]
fn analytic_forms_refuse_invalid_angular_domains() {
    let invalid = [
        [0.0, 2.0 * TAU],
        [0.0, TAU * (1.0 + 2e-9)],
        [0.0, f64::INFINITY],
        [f64::NEG_INFINITY, 0.0],
        [-f64::MAX, f64::MAX],
        [f64::NAN, 1.0],
        [0.0, f64::NAN],
        [1.0, 1.0],
        [1.0, 0.0],
    ];
    for (s, domain, _) in analytic() {
        let directions: &[usize] = if matches!(s, Surface::Torus { .. }) {
            &[0, 1]
        } else {
            &[0]
        };
        for &direction in directions {
            for bad in invalid {
                let mut d = domain;
                d[direction] = bad;
                assert!(form::surface(&s, d).is_none(), "{s:?}, {d:?}");
                assert!(form::surface_within(&s, d, 1e-6).is_none(), "{s:?}, {d:?}");
                assert!(
                    form::surface_on_own_arcs(&s, d, 1e-6).is_none(),
                    "{s:?}, {d:?}"
                );
                if direction == 1 {
                    assert!(form::tube_quarters(&s, d, 1e-6).is_none(), "{d:?}");
                }
            }
        }
    }
}

/// A torus face's quarters along its tube: a whole turn that starts off a quarter line needs five
/// (its seam's second use lies past four from the quarter it starts in), whatever quarter it
/// starts in, negative angles included; one that starts on a quarter line, or within the
/// tolerance's angle of one, or is a turn only to rounding, needs four; less than a turn the
/// quarters it crosses.
#[test]
fn a_whole_turn_round_a_tube_off_a_quarter_line_spans_five_quarters() {
    let torus = Surface::Torus {
        frame: Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        },
        major_radius: 2.5,
        minor_radius: 1.8,
        tube_start: 0.0,
    };
    let q = |v: [f64; 2], tol: f64| form::tube_quarters(&torus, [[0.0, FRAC_PI_2], v], tol);
    // The seam of #18778's bend, in the tube's second quarter.
    let seam = 1.910_828_950_980_659_5;
    for k in 0..4 {
        let start = seam + f64::from(k) * FRAC_PI_2;
        assert_eq!(q([start, start + TAU], 1e-7), Some(5), "{start}");
        assert_eq!(q([-start, TAU - start], 1e-7), Some(5), "{start}");
        let on = f64::from(k) * FRAC_PI_2;
        assert_eq!(q([on, on + TAU], 0.0), Some(4), "{on}");
    }
    // A turn to rounding, a start a rounding short of a quarter line within the tolerance's
    // angle (1e-6 / 1.8), and one farther short than that.
    assert_eq!(q([0.0, TAU * (1.0 + 0.5e-9)], 0.0), Some(4));
    let near = FRAC_PI_2 - 1e-9;
    assert_eq!(q([near, near + TAU], 1e-6), Some(4));
    let short = FRAC_PI_2 - 1e-5;
    assert_eq!(q([short, short + TAU], 1e-6), Some(5));
    // Less than a turn: from the second quarter into the fourth, and two whole quarters.
    assert_eq!(q([seam, seam + std::f64::consts::PI], 1e-7), Some(3));
    assert_eq!(q([0.0, std::f64::consts::PI], 0.0), Some(2));
}

/// Keep the existing rounding window around one complete turn and the exact form it chooses.
#[test]
fn analytic_forms_keep_the_existing_whole_turn_rounding_window() {
    for (s, domain, _) in analytic() {
        let mut d = domain;
        d[0] = [0.0, TAU];
        let expected = form::surface(&s, d).unwrap();
        let expected_within = form::surface_within(&s, d, 0.0).unwrap();
        let expected_own = form::surface_on_own_arcs(&s, d, 0.0).unwrap();
        for width in [TAU * (1.0 - 0.5e-9), TAU, TAU * (1.0 + 0.5e-9)] {
            d[0] = [0.0, width];
            assert_eq!(form::surface(&s, d).unwrap(), expected);
            assert_eq!(form::surface_within(&s, d, 0.0).unwrap(), expected_within);
            assert_eq!(form::surface_on_own_arcs(&s, d, 0.0).unwrap(), expected_own);
        }
    }
}

/// A revolution's profile parameter is a length here, not an angle; its range may exceed a turn.
#[test]
fn a_revolution_form_validates_its_angle_without_limiting_its_profile_parameter() {
    for forward in [false, true] {
        let s = Surface::PlacedRevolution {
            profile: Box::new(Curve::Line {
                start: [2.0, 0.0, 0.0],
                end: [2.0, 0.0, 1.0],
            }),
            origin: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            matrix: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            forward,
        };
        let d = if forward {
            [[0.0, 10.0], [0.0, TAU]]
        } else {
            [[0.0, TAU], [0.0, 10.0]]
        };
        let (made, mapped) = form::surface(&s, d).unwrap();
        let at = SurfaceEval::new(&made).unwrap();
        let uv = if forward { [10.0, 0.0] } else { [0.0, 10.0] };
        let p = at.point(uv).unwrap();
        assert!(
            p.dist(Vec3::new(2.0, 0.0, 10.0)) <= 1e-12,
            "{p:?}, {mapped:?}"
        );
        let angle = usize::from(forward);
        for bad in [
            [0.0, 2.0 * TAU],
            [0.0, f64::INFINITY],
            [f64::NEG_INFINITY, 0.0],
        ] {
            let mut d = d;
            d[angle] = bad;
            assert!(form::surface(&s, d).is_none(), "{forward}, {d:?}");
        }
        let profile_direction = 1 - angle;
        for bad in [
            [1.0, 0.0],
            [1.0, 1.0],
            [f64::NAN, 1.0],
            [0.0, f64::INFINITY],
        ] {
            let mut d = d;
            d[profile_direction] = bad;
            assert!(form::surface(&s, d).is_none(), "profile {forward}, {d:?}");
        }
    }
}

/// An exact form's points are its label's at the same angles: the form parameter of angle θ is
/// `form::param(θ)`, and `form::angle` maps it back.
#[test]
fn a_form_is_its_label_at_the_mapped_parameters() {
    for (s, dom, size) in analytic() {
        let (form, _) = form::surface(&s, dom).unwrap();
        let (label, f) = (
            SurfaceEval::new(&s).unwrap(),
            SurfaceEval::new(&form).unwrap(),
        );
        for uv in grid(dom, 9) {
            let map = |a: f64| form::param(a);
            let at = match s {
                Surface::Cylinder { .. } | Surface::Cone { .. } => [map(uv[0]), uv[1]],
                Surface::Sphere { .. } => [map(uv[0]), map(uv[1])],
                _ => continue,
            };
            let (p, q) = (label.point(uv).unwrap(), f.point(at).unwrap());
            assert!(p.dist(q) <= 1e-14 * size, "{s:?} at {uv:?}: {p:?} vs {q:?}");
            assert!((form::angle(form::param(uv[0])) - uv[0]).abs() < 1e-14);
        }
    }
}

/// A circle, closed form and as its rational quadratic form, has curvature 1/R towards its
/// centre everywhere, to rounding.
#[test]
fn a_circles_forms_have_its_curvature() {
    let f = tilted();
    let circle = Curve::Circle {
        frame: f.clone(),
        radius: 7.0,
    };
    let centre = Vec3::from_array(f.origin);
    let (form, fint) = form::curve(&circle, [0.2, 6.2]).unwrap();
    for (c, [a, b]) in [(circle, [0.2, 6.2]), (form, fint)] {
        let e = CurveEval::new(&c).unwrap();
        for k in 0..200 {
            let t = a + (b - a) * (k as f64 + 0.37) / 200.0;
            let j = e.jet(t).unwrap();
            assert!(((j.point - centre).len() - 7.0).abs() < 1e-14 * 7.0, "{t}");
            assert!(
                close(j.curvature, (centre - j.point) * (1.0 / 49.0), 1e-14),
                "{t}: {j:?}"
            );
        }
    }
}

/// An arc that starts inside a quarter and ends past the fourth quarter line on from it needs five
/// quarters of its frame's grid, which whole run a quarter past one turn. Its form is cut to it:
/// it starts and ends where the arc does, its joints between are the grid's quarter lines, and it
/// has the label's point at every angle's form parameter, as a form on whole quarters has. An arc
/// that needs four keeps them whole.
#[test]
fn an_arcs_form_of_five_quarters_is_cut_to_the_arc() {
    let f = tilted();
    let circle = Curve::Circle {
        frame: f.clone(),
        radius: 7.0,
    };
    let ellipse = Curve::Ellipse {
        frame: f,
        radii: [7.0, 3.0],
    };
    // Across the x axis; a whole turn from inside a quarter; ending 0.02 degrees past a quarter
    // line (an edge of a face that ends within its tolerance of one); starting as far before one.
    let past = 0.02_f64.to_radians();
    let arcs = [
        [1.0, 6.5],
        [-2.5, -2.5 + TAU],
        [30.02_f64.to_radians(), TAU + past],
        [FRAC_PI_2 - past, 4.5 * FRAC_PI_2],
    ];
    for c in [&circle, &ellipse] {
        let label = CurveEval::new(c).unwrap();
        for [t0, t1] in arcs {
            let (form, on) = form::curve(c, [t0, t1]).unwrap();
            let Curve::RationalSpline { knots, homogeneous } = &form else {
                panic!("{form:?}")
            };
            assert_eq!(on, [form::param(t0), form::param(t1)]);
            assert!(on[1] - on[0] <= 4.0 + 1e-12, "more than one turn: {on:?}");
            let q = on[0].floor();
            let joints = [1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0].map(|k| q + k);
            let expected: Vec<f64> = [on[0]; 3]
                .into_iter()
                .chain(joints)
                .chain([on[1]; 3])
                .collect();
            assert_eq!(knots, &expected, "[{t0}, {t1}]");
            assert_eq!(homogeneous.len(), 11);
            assert!(homogeneous.iter().all(|h| h[3] > 0.0), "{homogeneous:?}");
            let e = CurveEval::new(&form).unwrap();
            for k in 0..=400 {
                let t = t0 + (t1 - t0) * f64::from(k) / 400.0;
                let s = form::param(t).clamp(on[0], on[1]);
                let (p, want) = (e.jet(s).unwrap().point, label.jet(t).unwrap().point);
                assert!(
                    close(p, want, 1e-14),
                    "[{t0}, {t1}] at {t}: {p:?} vs {want:?}"
                );
            }
        }
        // Four quarters: the grid's own, whole.
        let (form, on) = form::curve(c, [0.2, 6.2]).unwrap();
        let Curve::RationalSpline { knots, .. } = &form else {
            panic!("{form:?}")
        };
        assert_eq!((knots[0], knots[knots.len() - 1]), (0.0, 4.0));
        assert_eq!(on, [form::param(0.2), form::param(6.2)]);
    }
}

/// The analytic surfaces' partials up to the fourth order agree with their first and second
/// derivatives and are each the derivative of the order below (central differences).
#[test]
fn analytic_partials_are_the_derivatives_of_the_order_below() {
    let f = Frame {
        origin: [1., 2., 3.],
        x_axis: [0.6, 0.8, 0.],
        y_axis: [0., 0., 1.],
    };
    let surfaces = [
        Surface::Cylinder {
            frame: f.clone(),
            radius: 3.,
        },
        Surface::Cone {
            frame: f.clone(),
            radius_at_origin: 2.,
            half_angle: 0.3,
        },
        Surface::Sphere {
            frame: f.clone(),
            radius: 4.,
        },
        Surface::Torus {
            frame: f,
            major_radius: 5.,
            minor_radius: 1.5,
            tube_start: 0.0,
        },
    ];
    let (u, v, h) = (0.8, 0.4, 1e-4);
    for s in &surfaces {
        let e = SurfaceEval::new(s).unwrap();
        let d = e.partials(u, v).unwrap();
        let (p, su, sv) = e.derivs(u, v).unwrap();
        let (suu, suv, svv) = e.second(u, v).unwrap();
        for (a, b) in [
            (d[0][0], p),
            (d[1][0], su),
            (d[0][1], sv),
            (d[2][0], suu),
            (d[1][1], suv),
            (d[0][2], svv),
        ] {
            assert!(close(a, b, 1e-12), "{s:?}: {a:?} vs {b:?}");
        }
        let (up, um, vp, vm) = (
            e.partials(u + h, v).unwrap(),
            e.partials(u - h, v).unwrap(),
            e.partials(u, v + h).unwrap(),
            e.partials(u, v - h).unwrap(),
        );
        for i in 0..MAX_PARTIAL_ORDER {
            for j in 0..MAX_PARTIAL_ORDER - i {
                assert!(
                    close((up[i][j] - um[i][j]) * (0.5 / h), d[i + 1][j], 1e-6),
                    "{s:?} d/du ({i},{j})"
                );
                assert!(
                    close((vp[i][j] - vm[i][j]) * (0.5 / h), d[i][j + 1], 1e-6),
                    "{s:?} d/dv ({i},{j})"
                );
            }
        }
    }
}

fn quarter_cylinder() -> EvalSurface {
    // A rational quadratic quarter circle of radius 2 in u, a line 3 long in v: exact.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    EvalSurface::nurbs(
        [2, 1],
        [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
        [3, 2],
        vec![
            [2.0, 0.0, 0.0, 1.0],
            [2.0 * w, 2.0 * w, 0.0, w],
            [0.0, 2.0, 0.0, 1.0],
            [2.0, 0.0, 3.0, 1.0],
            [2.0 * w, 2.0 * w, 3.0 * w, w],
            [0.0, 2.0, 3.0, 1.0],
        ],
    )
}

#[test]
fn every_spline_partial_is_the_derivative_of_the_order_below() {
    let cubic = EvalSurface::nurbs(
        [3, 2],
        [
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        ],
        [4, 3],
        (0..12)
            .map(|k| {
                [
                    k as f64 * 0.7,
                    (k * k) as f64 * 0.1,
                    (k % 5) as f64,
                    1.0 + 0.1 * (k % 3) as f64,
                ]
            })
            .collect(),
    );
    let surfaces = [
        quarter_cylinder(),
        cubic,
        EvalSurface::Revolved {
            profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 1.0)),
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: [
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0, 2.0],
                [0.0, 1.0, 0.0, 3.0],
            ],
            swap: true,
        },
    ];
    let h = 1e-4;
    for s in &surfaces {
        let (u, v) = (0.37, 0.61);
        let d = s.partials(u, v).unwrap();
        let (p, su, sv) = s.derivs(u, v).unwrap();
        assert!(
            close(d[0][0], p, 1e-12) && close(d[1][0], su, 1e-12) && close(d[0][1], sv, 1e-12),
            "{s:?}"
        );
        let (suu, suv, svv) = s.second(u, v).unwrap();
        assert!(
            close(d[2][0], suu, 1e-12) && close(d[1][1], suv, 1e-12) && close(d[0][2], svv, 1e-12),
            "{s:?}"
        );
        let (up, um) = (s.partials(u + h, v).unwrap(), s.partials(u - h, v).unwrap());
        let (vp, vm) = (s.partials(u, v + h).unwrap(), s.partials(u, v - h).unwrap());
        for (i, j) in orders().filter(|(i, j)| i + j < MAX_PARTIAL_ORDER) {
            let du = (up[i][j] - um[i][j]) * (0.5 / h);
            let dv = (vp[i][j] - vm[i][j]) * (0.5 / h);
            assert!(
                close(du, d[i + 1][j], 1e-5),
                "d/du of ({i},{j}): {du:?} vs {:?}",
                d[i + 1][j]
            );
            assert!(
                close(dv, d[i][j + 1], 1e-5),
                "d/dv of ({i},{j}): {dv:?} vs {:?}",
                d[i][j + 1]
            );
        }
    }
}

#[test]
fn a_rational_quarter_circle_has_the_circles_derivatives() {
    let s = quarter_cylinder();
    for u in [0.0, 0.2, 0.5, 0.9, 1.0] {
        let d = s.partials(u, 0.4).unwrap();
        assert!((d[0][0].x.hypot(d[0][0].y) - 2.0).abs() < 1e-12);
        let (r1, r2) = (d[1][0], d[2][0]);
        let k = r1.cross(r2).len() / r1.len().powi(3);
        assert!((k - 0.5).abs() < 1e-12, "{k}");
        assert!(d[0][2].len() < 1e-12 && d[1][1].z.abs() < 1e-12);
    }
}

#[test]
fn hostile_parameters_give_none() {
    let s = quarter_cylinder();
    assert!(s.partials(f64::NAN, 0.0).is_none());
    assert!(s.partials(0.0, f64::INFINITY).is_none());
    let c = EvalCurve::new(2, vec![0.0; 3], vec![[0.0; 4]; 3]);
    assert!(c.derivatives(0.5, 2).is_none());
}

/// A piece from the float below a knot up to the knot is taken from the span it starts in: its
/// middle rounds onto the knot, whose span on the right is another polynomial. Here x is
/// `1 - t` on `[0, 1]` and `0` on `[1, 2]`, so the piece's start is `2^-53`, not `0`.
#[test]
fn a_bezier_piece_ending_at_a_knot_is_its_own_spans() {
    let c = EvalCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 2.0],
        vec![
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
    let below = 1.0 - f64::EPSILON / 2.0;
    assert_eq!(
        0.5 * (below + 1.0),
        1.0,
        "the piece's middle rounds onto the knot"
    );
    let piece = c.bezier(below, 1.0).unwrap();
    let ([x, ..], [e, ..]) = piece[0];
    assert!((x - f64::EPSILON / 2.0).abs() <= e, "x {x}, bound {e}");
    assert_eq!(piece[1].0[0], 0.0);
    // Either side of the knot, and the range's very end, still have their pieces.
    assert!(c.bezier(1.0, 1.5).is_some_and(|p| p[0].0[0] == 0.0));
    assert!(c.bezier(2.0, 2.0).is_some());
    // A piece across the knot is not one span's.
    assert!(c.bezier(0.5, 1.5).is_none());
}

/// A control point whose exact coordinate lies below the least subnormal rounds to zero, and its
/// bound still encloses it; a knot span whose width overflows has no piece.
#[test]
fn a_bezier_bound_counts_underflow_and_an_overflowing_span_has_no_piece() {
    let least = f64::from_bits(1);
    // x is `least · t` on [0, 1]: at 0.5 it is half the least subnormal.
    let c = EvalCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![[0.0, 0.0, 0.0, 1.0], [least, 0.0, 0.0, 1.0]],
    );
    let ([x, ..], [e, ..]) = c.bezier(0.5, 1.0).unwrap()[0];
    // The exact x, half of `least`, is no float: the bound reaches past it from what came back.
    assert!(x.abs() + least <= e, "x {x}, bound {e}");
    // x is `t` on [0, 2]: at the least subnormal it is that, and `least / 2` rounds to zero.
    let c = EvalCurve::new(
        1,
        vec![0.0, 0.0, 2.0, 2.0],
        vec![[0.0, 0.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0]],
    );
    let ([x, ..], [e, ..]) = c.bezier(least, 1.0).unwrap()[0];
    assert!((x - least).abs() <= e, "x {x}, bound {e}");
    // Knots 2e308 apart: the width is not a number a step can divide by.
    let c = EvalCurve::new(
        1,
        vec![-1e308, -1e308, 1e308, 1e308],
        vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]],
    );
    assert!(c.bezier(0.0, 1.0).is_none());
}

#[test]
fn plane_point_and_normal() {
    let s = EvalSurface::Plane {
        origin: Vec3::new(1.0, 2.0, 3.0),
        du: Vec3::X,
        dv: Vec3::Y,
    };
    let (p, n) = s.eval(2.0, 3.0).unwrap();
    assert!(close(p, Vec3::new(3.0, 5.0, 3.0), 1e-12));
    assert!(close(n, Vec3::Z, 1e-12));
}

#[test]
fn revolved_line_is_a_cylinder() {
    let s = EvalSurface::Revolved {
        profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 5.0)),
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        transform: IDENTITY,
        swap: false,
    };
    for &(u, v) in &[(0.0, 0.0), (0.5, 1.0), (1.0, 4.0), (0.25, 6.0)] {
        let (p, n) = s.eval(u, v).unwrap();
        assert!(
            close(p, Vec3::new(2.0 * v.cos(), 2.0 * v.sin(), 5.0 * u), 1e-12),
            "{p:?}"
        );
        // S_u = +Z, S_v = tangential: the normal points inward (−radial) for this profile.
        assert!(close(n, Vec3::new(-v.cos(), -v.sin(), 0.0), 1e-12), "{n:?}");
    }
}

#[test]
fn swapped_parameters_reverse_the_normal() {
    let mk = |swap| EvalSurface::Revolved {
        profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 5.0)),
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        transform: IDENTITY,
        swap,
    };
    let (p0, n0) = mk(false).eval(0.3, 1.1).unwrap();
    let (p1, n1) = mk(true).eval(1.1, 0.3).unwrap();
    assert!(close(p0, p1, 1e-12));
    assert!(close(n0, -n1, 1e-12));
}

#[test]
fn mirror_transform_keeps_the_normal_outward() {
    let mirror = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let s = EvalSurface::Revolved {
        profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 5.0)),
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        transform: mirror,
        swap: false,
    };
    let (p, n) = s.eval(0.5, 0.0).unwrap();
    assert!(close(p, Vec3::new(-2.0, 0.0, 2.5), 1e-12));
    assert!(close(n, Vec3::X, 1e-12), "{n:?}");
}

#[test]
fn rational_quarter_circle_profile_is_exact() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let profile = EvalCurve::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            [3.0, 0.0, 0.0, 1.0],
            [3.0 * w, 0.0, 3.0 * w, w],
            [0.0, 0.0, 3.0, 1.0],
        ],
    );
    let s = EvalSurface::Revolved {
        profile,
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        transform: IDENTITY,
        swap: false,
    };
    for i in 0..=10 {
        for j in 0..=6 {
            let (u, v) = (i as f64 / 10.0, j as f64);
            let (p, n) = s.eval(u, v).unwrap();
            assert!((p.len() - 3.0).abs() < 1e-12, "{p:?}");
            // S_u × S_v points into this sphere; at the pole (u = 1) the limit, exactly.
            assert!(
                n.cross(p).len() < 1e-12 && n.dot(p) < 0.0,
                "normal {n:?} not inward at {p:?}"
            );
        }
    }
}

#[test]
fn bilinear_nurbs_patch_and_derivatives() {
    let ctrl = vec![
        [0.0, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [2.0, 1.0, 1.0, 1.0],
    ];
    let s = EvalSurface::nurbs(
        [1, 1],
        [vec![0.0, 0.0, 2.0, 2.0], vec![0.0, 0.0, 1.0, 1.0]],
        [2, 2],
        ctrl,
    );
    let Some((p, du, dv)) = s.derivs(1.0, 0.5) else {
        panic!("evaluates")
    };
    assert!(close(p, Vec3::new(1.0, 0.5, 0.25), 1e-12), "{p:?}");
    assert!(close(du, Vec3::new(1.0, 0.0, 0.25), 1e-12), "{du:?}");
    assert!(close(dv, Vec3::new(0.0, 1.0, 0.5), 1e-12), "{dv:?}");
}

/// A weighted bicubic-by-quadratic patch against the rational de Boor of its homogeneous net
/// (a test oracle), and its derivatives against central differences.
#[test]
fn a_rational_patch_matches_de_boor() {
    let knots = [
        vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
    ];
    let (nu, nv) = (5, 4);
    let mut ctrl = Vec::new();
    for j in 0..nv {
        for i in 0..nu {
            let w = 1.0 + ((i + 2 * j) % 3) as f64 * 0.25;
            ctrl.push([
                i as f64 * w,
                j as f64 * w,
                ((i * 7 + j * 3) % 5) as f64 * 0.3 * w,
                w,
            ]);
        }
    }
    let de_boor = |k: &[f64], p: usize, pts: &[[f64; 4]], t: f64| -> [f64; 4] {
        let n = pts.len();
        let mut s = p;
        while s + 1 < n && k[s + 1] <= t {
            s += 1;
        }
        let mut d: Vec<[f64; 4]> = (0..=p).map(|j| pts[j + s - p]).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = j + s - p;
                let a = (t - k[i]) / (k[i + 1 + p - r] - k[i]);
                d[j] = [0, 1, 2, 3].map(|c| (1.0 - a) * d[j - 1][c] + a * d[j][c]);
            }
        }
        d[p]
    };
    let s = EvalSurface::nurbs([3, 2], knots.clone(), [nu, nv], ctrl.clone());
    for i in 0..=8 {
        for j in 0..=8 {
            let (u, v) = (i as f64 / 8.0, j as f64 / 4.0);
            let column: Vec<[f64; 4]> = (0..nv)
                .map(|r| de_boor(&knots[0], 3, &ctrl[r * nu..(r + 1) * nu], u))
                .collect();
            let h = de_boor(&knots[1], 2, &column, v);
            let want = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
            let Some((p, du, dv)) = s.derivs(u, v) else {
                panic!("evaluates")
            };
            assert!(close(p, want, 1e-14), "({u}, {v}): {p:?} vs {want:?}");
            let e = 1e-6;
            let fd = |a: (f64, f64), b: (f64, f64)| {
                (s.derivs(b.0, b.1).unwrap().0 - s.derivs(a.0, a.1).unwrap().0)
                    * (1.0 / (b.0 - a.0 + b.1 - a.1))
            };
            let fu = fd((u.max(e) - e, v), ((u + e).min(1.0), v));
            let fv = fd((u, v.max(e) - e), (u, (v + e).min(2.0)));
            assert!(close(du, fu, 1e-4), "du at ({u}, {v}): {du:?} vs {fu:?}");
            assert!(close(dv, fv, 1e-4), "dv at ({u}, {v}): {dv:?} vs {fv:?}");
        }
    }
}

#[test]
fn overflowing_control_counts_are_unevaluable() {
    let knots = [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]];
    let ctrl = vec![
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [1.0, 1.0, 0.0, 1.0],
    ];
    // The first count overflows n + p; the second overflows the following + 1.
    for huge in [usize::MAX, usize::MAX - 1] {
        for direction in 0..2 {
            let mut count = [2, 2];
            count[direction] = huge;
            let inferred = EvalSurface::nurbs([1, 1], knots.clone(), count, ctrl.clone());
            let explicit = EvalSurface::nurbs_closed(
                [1, 1],
                knots.clone(),
                count,
                ctrl.clone(),
                [false, false],
            );
            for surface in [inferred, explicit] {
                assert!(surface.derivs(0.5, 0.5).is_none());
                assert!(surface.eval(0.5, 0.5).is_err());
            }
        }
    }
}

#[test]
fn hostile_surfaces_do_not_panic() {
    let bad = [
        EvalSurface::nurbs([9, 1], [vec![0.0; 3], vec![0.0; 3]], [1, 1], vec![]),
        EvalSurface::nurbs(
            [1, 1],
            [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
            [2, 2],
            vec![[0.0; 4]; 4],
        ),
        EvalSurface::Revolved {
            profile: EvalCurve::new(0, vec![], vec![]),
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: IDENTITY,
            swap: false,
        },
    ];
    for s in &bad {
        for &(u, v) in &[(0.5, 0.5), (f64::NAN, 0.0), (1e300, -1e300)] {
            if let Ok((p, n)) = s.eval(u, v) {
                assert!(p.is_finite() && n.is_finite());
            }
        }
    }
}

/// A point next to a sphere's pole is a pole to within the reach it is given: its circle of
/// longitude moves it by the circle's diameter, twice its distance from the axis (4e-7 here),
/// and no further.
#[test]
fn a_point_beside_a_pole_is_one_within_its_reach() {
    let sphere = SurfaceEval::new(&Surface::Sphere {
        frame: a_frame(),
        radius: 2.,
    })
    .unwrap();
    let near = [0.3, FRAC_PI_2 - 1e-7];
    assert_eq!(sphere.pole(near).unwrap(), [false, false]);
    assert_eq!(sphere.pole_within(near, 1e-6).unwrap(), [true, false]);
    assert_eq!(sphere.pole_within(near, 1e-7).unwrap(), [false, false]);
    assert_eq!(
        sphere.pole_within([0.3, 0.2], 1e-3).unwrap(),
        [false, false]
    );
    assert_eq!(sphere.pole([0.3, FRAC_PI_2]).unwrap(), [true, false]);
}

/// At the threshold, r = reach / 2: a form whose circles are no wider than the reach is thin,
/// not collapsed. A cylinder of radius 0.049, 0.05 or 0.051 at a reach of 0.1 (a pin's round in a
/// file of 0.1 mm uncertainty) is a pole nowhere; a ball of radius 0.05 only at its exact poles, a torus's tube
/// of radius 0.05 nowhere. A ball a little wider still has its poles within the reach.
#[test]
fn a_surface_no_wider_than_the_reach_is_thin_not_a_pole() {
    let reach = 0.1;
    // A cylinder just under, at and just over r = reach / 2: a pole nowhere.
    for k in [0.49, 0.5, 0.51] {
        let cylinder = SurfaceEval::new(&Surface::Cylinder {
            frame: a_frame(),
            radius: k * reach,
        })
        .unwrap();
        assert_eq!(
            cylinder.pole_within([0.3, 0.5], reach).unwrap(),
            [false, false],
            "r = {k} reach"
        );
    }
    let ball = SurfaceEval::new(&Surface::Sphere {
        frame: a_frame(),
        radius: 0.5 * reach,
    })
    .unwrap();
    assert_eq!(ball.pole_within([0.3, 0.2], reach).unwrap(), [false, false]);
    assert_eq!(
        ball.pole_within([0.3, FRAC_PI_2], reach).unwrap(),
        [true, false]
    );
    let wider = SurfaceEval::new(&Surface::Sphere {
        frame: a_frame(),
        radius: 0.501 * reach,
    })
    .unwrap();
    assert_eq!(wider.pole_within([0.3, 0.2], reach).unwrap(), [true, false]);
    let torus = SurfaceEval::new(&Surface::Torus {
        frame: a_frame(),
        major_radius: 4.0,
        minor_radius: 0.5 * reach,
        tube_start: 0.0,
    })
    .unwrap();
    assert_eq!(
        torus.pole_within([0.3, 0.2], reach).unwrap(),
        [false, false]
    );
}

/// A curve's curvature vector, exactly, on every kind: a circle's is 1/R towards its centre (as
/// a conic and as its rational quadratic B-spline); an ellipse's at its vertices is a/b² and
/// b/a²; a line is straight; and the circle where a plane z = 3 cuts a sphere of radius 5 about
/// the origin, traced from a rough leader, has radius 4.
#[test]
fn curve_jets_are_exact_on_every_kind() {
    let near = |a: Vec3, b: Vec3| a.dist(b) <= 1e-9 * (1.0 + b.len());
    let jet = |c: &Curve, t: f64| CurveEval::new(c).unwrap().jet(t).unwrap();
    let flat = Frame {
        origin: [1., 2., 3.],
        x_axis: [0.6, 0.8, 0.],
        y_axis: [-0.8, 0.6, 0.],
    };
    let o = Vec3::new(1., 2., 3.);
    for t in [0.0, 0.7, 2.5, 5.9] {
        let j = jet(
            &Curve::Circle {
                frame: flat.clone(),
                radius: 4.,
            },
            t,
        );
        assert!(near(j.curvature, (o - j.point) * (1.0 / 16.0)), "{j:?}");
        assert!(j.tangent.dot(j.curvature).abs() < 1e-12);
    }
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = Curve::RationalSpline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        homogeneous: vec![[2., 0., 0., 1.], [2. * w, 2. * w, 0., w], [0., 2., 0., 1.]],
    };
    for t in [0.0, 0.3, 0.5, 1.0] {
        let j = jet(&quarter, t);
        assert!(near(j.curvature, j.point * -0.25), "{t}: {j:?}");
    }
    let (a, b) = (10.0, 4.0);
    let ellipse = Curve::Ellipse {
        frame: flat,
        radii: [a, b],
    };
    assert!((jet(&ellipse, 0.0).curvature.len() - a / (b * b)).abs() < 1e-12);
    assert!((jet(&ellipse, FRAC_PI_2).curvature.len() - b / (a * a)).abs() < 1e-12);
    assert_eq!(
        jet(
            &Curve::Line {
                start: [0.; 3],
                end: [1., 2., 3.]
            },
            0.4
        )
        .curvature,
        Vec3::ZERO
    );
    let z = Frame {
        origin: [0.; 3],
        x_axis: [1., 0., 0.],
        y_axis: [0., 1., 0.],
    };
    let leader = Curve::Polyline {
        points: (0..=12)
            .map(|k| (k as f64 * TAU / 12.0).sin_cos())
            .map(|(s, c)| [4.1 * c, 4.1 * s, 3.2])
            .collect(),
    };
    let cut = Curve::SurfaceIntersection {
        surfaces: [
            Box::new(Surface::Plane {
                origin: [0., 0., 3.],
                u_axis: [1., 0., 0.],
                v_axis: [0., 1., 0.],
            }),
            Box::new(Surface::Sphere {
                frame: z,
                radius: 5.,
            }),
        ],
        leader: Box::new(leader),
        tolerance: 1e-6,
    };
    for t in [0.0, 1.5, 4.25, 11.0] {
        let j = jet(&cut, t);
        let centre = Vec3::new(0., 0., 3.);
        assert!(
            (j.point.dist(centre) - 4.0).abs() < 1e-9 && (j.point.z - 3.0).abs() < 1e-12,
            "{j:?}"
        );
        assert!(
            near(j.curvature, (centre - j.point) * (1.0 / 16.0)),
            "{t}: {j:?}"
        );
        assert!(j.tangent.dot(j.point - centre).abs() < 1e-9);
    }
}

/// A nest of curves taken apart one level at a time: dropped whole, it would recurse as deep as
/// it is nested.
fn take_apart(c: Curve) {
    let mut stack = vec![c];
    while let Some(c) = stack.pop() {
        if let Curve::SurfaceIntersection {
            surfaces, leader, ..
        } = c
        {
            stack.push(*leader);
            for s in surfaces {
                if let Surface::PlacedRevolution { profile, .. } = *s {
                    stack.push(*profile);
                }
            }
        }
    }
}

/// An intersection's leader is a polyline or a spline, and a revolution's profile is never an
/// intersection: either nested in another is refused at its own level, before the evaluator
/// descends into it, so a record nested as deep as its producer likes cannot exhaust the stack.
#[test]
fn a_nested_intersection_is_refused_without_descending_into_it() {
    const DEPTH: usize = 100_000;
    let plane = || {
        Box::new(Surface::Plane {
            origin: [0.; 3],
            u_axis: [1., 0., 0.],
            v_axis: [0., 1., 0.],
        })
    };
    let leaf = || Curve::Polyline {
        points: vec![[0.; 3], [1., 0., 0.]],
    };
    let mut through_leader = leaf();
    for _ in 0..DEPTH {
        through_leader = Curve::SurfaceIntersection {
            surfaces: [plane(), plane()],
            leader: Box::new(through_leader),
            tolerance: 1e-6,
        };
    }
    let mut through_profile = leaf();
    for _ in 0..DEPTH {
        through_profile = Curve::SurfaceIntersection {
            surfaces: [
                Box::new(Surface::PlacedRevolution {
                    profile: Box::new(through_profile),
                    origin: [0.; 3],
                    axis: [0., 0., 1.],
                    matrix: [
                        [1., 0., 0., 0.],
                        [0., 1., 0., 0.],
                        [0., 0., 1., 0.],
                        [0., 0., 0., 1.],
                    ],
                    forward: true,
                }),
                plane(),
            ],
            leader: Box::new(leaf()),
            tolerance: 1e-6,
        };
    }
    // A small stack, so that descending even a fraction of the nest would overflow it.
    let (leader, profile, through_leader, through_profile) = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            (
                CurveEval::new(&through_leader).err(),
                CurveEval::new(&through_profile).err(),
                through_leader,
                through_profile,
            )
        })
        .unwrap()
        .join()
        .unwrap();
    take_apart(through_leader);
    take_apart(through_profile);
    assert!(matches!(leader, Some(EvalError::Invalid(_))), "{leader:?}");
    assert!(
        matches!(profile, Some(EvalError::Unsupported(_))),
        "{profile:?}"
    );
}

/// A line extends for ever past its knots, as a B-spline curve continues its end span.
#[test]
fn a_spline_curve_continues_past_its_knots() {
    let c = line(Vec3::new(1.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
    let (p, d) = c.eval(1.5).unwrap();
    assert!(close(p, Vec3::new(4.0, 0.0, 0.0), 1e-15) && close(d, Vec3::new(2.0, 0.0, 0.0), 1e-15));
    let (p, _) = c.eval(-0.5).unwrap();
    assert!(close(p, Vec3::new(0.0, 0.0, 0.0), 1e-15));
}

/// A curve's first derivative from either side of a knot where it has a corner.
#[test]
fn a_corner_has_two_sides() {
    let c = EvalCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 2.0],
        vec![
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, 1.0],
        ],
    );
    let (left, right) = c.sides(1.0).unwrap();
    assert!(
        close(left, Vec3::X, 1e-15) && close(right, Vec3::Y, 1e-15),
        "{left:?} {right:?}"
    );
}

#[test]
fn a_form_on_arcs_of_its_own_is_the_surface_and_does_not_close_around_an_open_face() {
    let torus = Surface::Torus {
        frame: a_frame(),
        major_radius: 5.,
        minor_radius: 1.5,
        tube_start: 0.,
    };
    let label = [[0.3, 1.2], [-2.9, 2.0]];
    let tube = |s: &Surface| match s {
        Surface::RationalSpline { v_knots, .. } => (
            v_knots.first().copied().unwrap(),
            v_knots.last().copied().unwrap(),
        ),
        _ => panic!("not a NURBS: {s:?}"),
    };
    let (grid, _) = form::surface(&torus, label).unwrap();
    assert_eq!(
        tube(&grid),
        (-2.0, 2.0),
        "the grid's form is the whole tube"
    );
    let tol = 1e-3;
    let (own, domain) = form::surface_on_own_arcs(&torus, label, tol).unwrap();
    let (v0, v1) = tube(&own);
    assert_eq!((v0, v1), (0.0, 4.0));
    let (label_at, own_at) = (
        SurfaceEval::new(&torus).unwrap(),
        SurfaceEval::new(&own).unwrap(),
    );
    let u = domain[0][0];
    assert!(
        own_at
            .point([u, v0])
            .unwrap()
            .dist(own_at.point([u, v1]).unwrap())
            > 0.1,
        "the form does not close"
    );
    for i in 0..=8 {
        for j in 0..=12 {
            let uv = [
                domain[0][0] + (domain[0][1] - domain[0][0]) * i as f64 / 8.,
                v0 + (v1 - v0) * j as f64 / 12.,
            ];
            let p = own_at.point(uv).unwrap() - Vec3::new(1., 2., 3.);
            let off = ((p.x.hypot(p.y) - 5.).powi(2) + p.z.powi(2)).sqrt() - 1.5;
            assert!(off.abs() < 1e-12, "{uv:?}: {off}");
        }
    }
    for (a, b) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let (p, q) = (
            label_at.point([label[0][a], label[1][b]]).unwrap(),
            own_at.point([domain[0][a], domain[1][b]]).unwrap(),
        );
        assert!(p.dist(q) < 1e-9, "corner {a}{b}: {p:?} vs {q:?}");
    }
    assert!(domain[1][0] > v0 && domain[1][1] < v1, "{domain:?}");
    let (whole, _) =
        form::surface_on_own_arcs(&torus, [[0.3, 1.2], [-FRAC_PI_2, -FRAC_PI_2 + TAU]], tol)
            .unwrap();
    assert_eq!(tube(&whole), (-1.0, 3.0));
    assert!(form::surface_on_own_arcs(
        &Surface::Plane {
            origin: [0.; 3],
            u_axis: [1., 0., 0.],
            v_axis: [0., 1., 0.]
        },
        label,
        tol
    )
    .is_none());
}

/// A hole beside a cylinder's seam: the face's box is the outer loop's turn with the hole in it.
#[test]
fn a_hole_beside_the_seam_lies_in_its_face_turn() {
    let (r, h) = (20.0f64, 60.0);
    let cylinder = Surface::Cylinder {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let s = -10f64.to_radians();
    let at = |a: f64, z: f64| [r * a.cos(), r * a.sin(), z];
    let ring = |z: f64| Curve::Circle {
        frame: Frame {
            origin: [0., 0., z],
            x_axis: [s.cos(), s.sin(), 0.],
            y_axis: [-s.sin(), s.cos(), 0.],
        },
        radius: r,
    };
    let (bottom, top) = (ring(0.), ring(h));
    let seam = Curve::Line {
        start: at(s, 0.),
        end: at(s, h),
    };
    let (a0, a1) = (15f64.to_radians(), 25f64.to_radians());
    let hole = Curve::Spline {
        knots: vec![0., 0., 1., 2., 3., 4., 4.],
        points: vec![
            at(a0, 25.),
            at(a1, 25.),
            at(a1, 35.),
            at(a0, 35.),
            at(a0, 25.),
        ],
    };
    let outer = vec![
        (&bottom, [0., TAU], true),
        (&seam, [0., 1.], true),
        (&top, [0., TAU], false),
        (&seam, [0., 1.], false),
    ];
    let inner = vec![(&hole, [0., 4.], true)];
    let [u, v] = form::extent(&cylinder, &[outer, inner], 1e-9)
        .unwrap()
        .expect("an extent");
    assert!((u[1] - u[0] - TAU).abs() < 1e-9, "one turn: {u:?}");
    assert!(
        (u[0].rem_euclid(TAU) - s.rem_euclid(TAU)).abs() < 1e-9,
        "from the seam: {u:?}"
    );
    let hole_u = a0 + ((u[0] - a0) / TAU).ceil() * TAU;
    assert!(
        hole_u > u[0] && hole_u + (a1 - a0) < u[1],
        "the hole in the turn: {u:?}"
    );
    assert!(v[0].abs() < 1e-9 && (v[1] - h).abs() < 1e-9, "{v:?}");
}

/// A hemisphere bounded by its rim and its seam: through the pole the loop goes on a turn away.
#[test]
fn a_loop_through_a_pole_closes_its_turn_from_the_seam() {
    let r = 15.0f64;
    let sphere = Surface::Sphere {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let rim = Curve::Circle {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let seam = Curve::Circle {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 0., -1.],
        },
        radius: r,
    };
    let outer = vec![
        (&rim, [0., FRAC_PI_2], false),
        (&seam, [0., FRAC_PI_2], true),
        (&seam, [0., FRAC_PI_2], false),
        (&rim, [FRAC_PI_2, TAU], false),
    ];
    let [u, v] = form::extent(&sphere, &[outer], 1e-9)
        .unwrap()
        .expect("an extent");
    assert!((u[1] - u[0] - TAU).abs() < 1e-9, "one turn: {u:?}");
    let from = u[0].rem_euclid(TAU);
    assert!(from.min(TAU - from) < 1e-9, "from the seam: {u:?}");
    assert!(
        (v[0] + FRAC_PI_2).abs() < 1e-9 && v[1].abs() < 1e-9,
        "{v:?}"
    );
}

/// A 5 mm sphere about z, its seam the meridian through +x (angle −π/2 at the south pole, π/2
/// at the north), and the circle where the plane at `d` from the centre across direction
/// `(cos a, sin a, 0)` meets it (angle 0 at its top, a quarter turn towards +y about it).
fn sphere_seam_and_cut(a: f64, d: f64) -> (Surface, Curve, Curve) {
    let r = 5.0f64;
    let sphere = Surface::Sphere {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let seam = Curve::Circle {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 0., 1.],
        },
        radius: r,
    };
    let (s, c) = a.sin_cos();
    let cut = Curve::Circle {
        frame: Frame {
            origin: [d * c, d * s, 0.],
            x_axis: [0., 0., 1.],
            y_axis: [-s, c, 0.],
        },
        radius: (r * r - d * d).sqrt(),
    };
    (sphere, seam, cut)
}

/// A sphere less a hole beside its seam (#16872: a ball with a bore in it, after a STEP round
/// trip): the seam's loop, up the meridian and back down it, has no width, yet the face covers
/// the whole turn with the hole in it, whichever loop comes first. Taken as wide as the hole, the
/// face's form was a sliver and its seam loop enclosed nothing.
#[test]
fn a_sphere_less_a_hole_beside_its_seam_spans_the_whole_turn() {
    let (sphere, seam, hole) = sphere_seam_and_cut(0.4, 4.8);
    let seam_loop = vec![
        (&seam, [-FRAC_PI_2, FRAC_PI_2], true),
        (&seam, [-FRAC_PI_2, FRAC_PI_2], false),
    ];
    let hole_loop = vec![(&hole, [0., TAU], false)];
    for loops in [
        [seam_loop.clone(), hole_loop.clone()],
        [hole_loop.clone(), seam_loop.clone()],
    ] {
        let [u, v] = form::extent(&sphere, &loops, 1e-9)
            .unwrap()
            .expect("an extent");
        assert!((u[1] - u[0] - TAU).abs() < 1e-9, "one turn: {u:?}");
        let from = u[0].rem_euclid(TAU);
        assert!(from.min(TAU - from) < 1e-9, "from the seam: {u:?}");
        let hole_u = 0.4 + ((u[0] - 0.4) / TAU).ceil() * TAU;
        assert!(
            hole_u > u[0] && hole_u < u[1],
            "the hole in the turn: {u:?}"
        );
        assert!(
            (v[0] + FRAC_PI_2).abs() < 1e-9 && (v[1] - FRAC_PI_2).abs() < 1e-9,
            "{v:?}"
        );
    }
}

/// A sphere less a bore across its seam (#16872): its loop runs up the seam to the bore, round
/// the bore's one side, up the seam to the north pole, down the seam's other use, round the
/// bore's other side and down to the south pole. It passes both poles, so it closes whatever
/// turns it takes there; the face lies on its left, so over the north pole it turns back a whole
/// turn and the face covers it. Closing the loop instead took the turns of no width, a box just
/// as wide as the bore, and the face came out as the bore turned over.
#[test]
fn a_loop_over_both_poles_turns_the_way_its_face_lies() {
    let (sphere, seam, bore) = sphere_seam_and_cut(0.0, 4.0);
    let below = [-FRAC_PI_2, -(0.6f64.asin())];
    let above = [0.6f64.asin(), FRAC_PI_2];
    let outer = [
        (&seam, below, true),
        (&bore, [std::f64::consts::PI, TAU], true),
        (&seam, above, true),
        (&seam, above, false),
        (&bore, [0., std::f64::consts::PI], true),
        (&seam, below, false),
    ];
    // Started anywhere round it, the loop takes the same turns.
    for k in 0..outer.len() {
        let l: Vec<_> = outer
            .iter()
            .cycle()
            .skip(k)
            .take(outer.len())
            .copied()
            .collect();
        let [u, v] = form::extent(&sphere, &[l], 1e-9)
            .unwrap()
            .expect("an extent");
        assert!(
            (u[1] - u[0] - TAU).abs() < 1e-9,
            "start {k}: one turn: {u:?}"
        );
        let from = u[0].rem_euclid(TAU);
        assert!(
            from.min(TAU - from) < 1e-9,
            "start {k}: from the seam: {u:?}"
        );
        assert!(
            (v[0] + FRAC_PI_2).abs() < 1e-9 && (v[1] - FRAC_PI_2).abs() < 1e-9,
            "start {k}: {v:?}"
        );
    }
}

/// Half a dome of revolution, its profile a spline from the axis, bounded by a meridian into
/// the pole, the meridian half a turn round back out of it and half the rim: the loop out of the
/// pole is placed from the whole surface, not from the last point before the pole (across the
/// axis, where the nearest-point search settled on the pole and the meridian and the rim's first
/// point bounded nothing, leaving the box a sample short of half a turn).
#[test]
fn a_loop_out_of_a_revolutions_pole_reaches_the_meridian_it_leaves_on() {
    let identity = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let profile = |side: f64| Curve::Spline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        points: vec![[0., 0., 0.], [0., 2. * side, 0.], [1.5, 3. * side, 0.]],
    };
    let dome = Surface::PlacedRevolution {
        profile: Box::new(profile(1.0)),
        origin: [0.; 3],
        axis: [1., 0., 0.],
        matrix: identity,
        forward: false,
    };
    let (east, west) = (profile(1.0), profile(-1.0));
    let rim = Curve::Circle {
        frame: Frame {
            origin: [1.5, 0., 0.],
            x_axis: [0., 1., 0.],
            y_axis: [0., 0., 1.],
        },
        radius: 3.,
    };
    // The half below z = 0: in on the −y meridian, out on the +y one, back round the rim.
    let lune = vec![
        (&west, [0., 1.], false),
        (&east, [0., 1.], true),
        (&rim, [std::f64::consts::PI, TAU], false),
    ];
    let [u, v] = form::extent(&dome, &[lune], 1e-6)
        .unwrap()
        .expect("an extent");
    assert!(
        (u[1] - u[0] - std::f64::consts::PI).abs() < 1e-9,
        "half a turn: {u:?}"
    );
    let to = u[1].rem_euclid(TAU);
    assert!(to.min(TAU - to) < 1e-9, "to the +y meridian: {u:?}");
    assert!(v[0].abs() < 1e-9 && (v[1] - 1.).abs() < 1e-9, "{v:?}");
}

/// A face's box reaches its boundary's extremes, not its samples' nearest ones.
#[test]
fn a_face_box_reaches_its_boundary_extremes() {
    let r = 20.0f64;
    let cylinder = Surface::Cylinder {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let cut = Curve::Ellipse {
        frame: Frame {
            origin: [0., 0., 30.],
            x_axis: [c, 0., c],
            y_axis: [0., 1., 0.],
        },
        radii: [r * 2f64.sqrt(), r],
    };
    let [_, v] = form::extent(&cylinder, &[vec![(&cut, [0.1, 0.1 + TAU], true)]], 1e-9)
        .unwrap()
        .expect("an extent");
    assert!(
        (v[0] - 10.0).abs() < 1e-9 && (v[1] - 50.0).abs() < 1e-9,
        "{v:?}"
    );
}

/// A loop that goes round a cylinder once and backs up a little (along a notch: a slot wall, the
/// curve where two drill points meet) has a box a little wider than a turn, but it does not wind:
/// its face is one turn wide, like any other band. A loop that goes back more than a turn (a
/// thread's flank, a screw's cylinder down its thread) keeps its box.
#[test]
fn a_loop_that_goes_round_once_and_backs_up_a_little_does_not_wind() {
    let (r, h) = (20.0f64, 60.0);
    let cylinder = Surface::Cylinder {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let ring = |z: f64| Curve::Circle {
        frame: Frame {
            origin: [0., 0., z],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let ruling = |turns: f64| {
        let a = turns * TAU;
        Curve::Line {
            start: [r * a.cos(), r * a.sin(), 0.],
            end: [r * a.cos(), r * a.sin(), h],
        }
    };
    let (bottom, top) = (ring(0.), ring(h));
    // Once round and `back` turns back: forward 1 + back turns along the bottom, up the ruling
    // there, back `back` turns along the top, down the ruling at the start.
    let width = |back: f64| {
        let (up, down) = (ruling(back), ruling(0.));
        let l = vec![
            (&bottom, [0., (1.0 + back) * TAU], true),
            (&up, [0., 1.], true),
            (&top, [0., back * TAU], false),
            (&down, [0., 1.], false),
        ];
        let [u, _] = form::extent(&cylinder, &[l], 1e-9)
            .unwrap()
            .expect("an extent");
        u[1] - u[0]
    };
    for back in [0.01, 0.1, 0.5] {
        let w = width(back);
        assert!((w - TAU).abs() < 1e-9, "backed up {back} turn: {w}");
        assert!(!form::winds([[0.0, w], [0.0, 1.0]]));
    }
    // Back more than a turn: the box is the whole loop's.
    let w = width(1.1);
    assert!((w - 2.1 * TAU).abs() < 1e-9, "{w}");
    assert!(form::winds([[0.0, w], [0.0, 1.0]]));
}

/// One edge that winds 33 times round a cylinder (a plane's section of it, run on for 33 turns)
/// is measured 33 turns wide: sampled at 32 points it would step a turn and a thirty-second at a
/// time and read as one turn.
#[test]
fn an_edge_that_winds_more_than_sixteen_turns_is_measured_whole() {
    let r = 20.0f64;
    let cylinder = Surface::Cylinder {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let cut = Curve::Ellipse {
        frame: Frame {
            origin: [0., 0., 30.],
            x_axis: [c, 0., c],
            y_axis: [0., 1., 0.],
        },
        radii: [r * 2f64.sqrt(), r],
    };
    let turns = 33.0;
    let [u, v] = form::extent(&cylinder, &[vec![(&cut, [0., turns * TAU], true)]], 1e-9)
        .unwrap()
        .expect("an extent");
    assert!((u[1] - u[0] - turns * TAU).abs() < 1e-6, "{u:?}");
    assert!(
        (v[0] - 10.0).abs() < 1e-9 && (v[1] - 50.0).abs() < 1e-9,
        "{v:?}"
    );
}

/// An edge of a million turns is probed over a step of a whole turn at the start: that step
/// projects back onto its start and would read as no winding, so the face's box would be one turn
/// wide. The step is halved until it advances less than a quarter turn, and the edge is then
/// refused as wound beyond what is measured.
#[test]
fn an_edge_of_a_million_turns_is_refused_not_read_as_one_turn() {
    let r = 20.0f64;
    let cylinder = Surface::Cylinder {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let cut = Curve::Ellipse {
        frame: Frame {
            origin: [0., 0., 30.],
            x_axis: [c, 0., c],
            y_axis: [0., 1., 0.],
        },
        radii: [r * 2f64.sqrt(), r],
    };
    let turns = 1.0e6;
    let found = form::extent(&cylinder, &[vec![(&cut, [0., turns * TAU], true)]], 1e-9).unwrap();
    assert!(found.is_none(), "{found:?}");
}

/// The flux of `p / 3` through a whole sphere is its volume.
#[test]
fn a_spheres_flux_is_its_volume() {
    let s = SurfaceEval::new(&Surface::Sphere {
        frame: a_frame(),
        radius: 2.0,
    })
    .unwrap();
    let o = Vec3::new(1., 2., 3.);
    // The face's (u, v) rectangle, counter-clockwise.
    let seg = |a: [f64; 2], b: [f64; 2]| Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![a, b],
    };
    let (u0, u1, v0, v1) = (0.0, TAU, -FRAC_PI_2, FRAC_PI_2);
    let sides = [
        seg([u0, v0], [u1, v0]),
        seg([u1, v0], [u1, v1]),
        seg([u1, v1], [u0, v1]),
        seg([u0, v1], [u0, v0]),
    ];
    let loops = vec![sides.iter().map(|c| (c, true)).collect::<Vec<_>>()];
    let flux = face_flux(&s, v0, &loops, &|p| (p - o) * (1.0 / 3.0)).unwrap();
    let volume = 4.0 / 3.0 * std::f64::consts::PI * 8.0;
    assert!((flux - volume).abs() < 1e-3 * volume, "{flux} vs {volume}");
}

/// At a sphere form's poles the normal is the limit, exactly radial, and points the way the
/// surface does next to the pole; the same after the net is moved.
#[test]
fn a_pole_takes_the_limit_normal() {
    let s = Surface::Sphere {
        frame: tilted(),
        radius: 7.0,
    };
    let (form, fdom) = form::surface(&s, [[0.3, 5.9], [-FRAC_PI_2, FRAC_PI_2]]).unwrap();
    let SurfaceEval::Eval(e, _) = SurfaceEval::new(&form).unwrap() else {
        panic!("a spline")
    };
    let centre = Vec3::from_array(tilted().origin);
    for v in [fdom[1][0], fdom[1][1]] {
        for u in [fdom[0][0], 0.5 * (fdom[0][0] + fdom[0][1]), fdom[0][1]] {
            let (p, n) = e.eval(u, v).unwrap();
            let radial = (p - centre) * (1.0 / 7.0);
            let (_, nn) = e
                .eval(u, v + if v == fdom[1][0] { 1e-3 } else { -1e-3 })
                .unwrap();
            assert!(n.cross(radial).len() < 1e-14, "{n:?} vs {radial:?}");
            assert!(n.dot(nn) > 0.99, "{n:?} vs next to it {nn:?}");
        }
    }
}

/// Higher derivatives are taken on the surface: a parameter past the knot range by rounding is
/// the range's end, one farther out has none.
#[test]
fn higher_derivatives_take_a_rounding_past_the_end_as_the_end() {
    let s = quarter_cylinder();
    let at_end = s.second(0.4, 1.0).unwrap();
    let past = s.second(0.4, 1.0 + 1e-15).unwrap();
    assert_eq!(at_end, past);
    assert!(s.second(0.4, 1.0 + 1e-6).is_none() && s.partials(1.0 + 1e-6, 0.5).is_none());
    assert!(
        s.derivs(1.0 + 1e-6, 0.5).is_some(),
        "first derivatives continue the end span"
    );
}

/// The analytic forms' poles in closed form: a cone's apex (where its radius r0 + v tan(half
/// angle) is zero) is collapsed along u; a horn torus (ring radius equal to the tube's) is
/// collapsed along u where its ring radius major + minor cos(v + tube) reaches zero; neither is
/// collapsed elsewhere.
#[test]
fn analytic_apexes_and_rings_of_radius_zero_are_poles() {
    let cone = SurfaceEval::new(&Surface::Cone {
        frame: a_frame(),
        radius_at_origin: 2.0,
        half_angle: -0.4,
    })
    .unwrap();
    let apex = 2.0 / 0.4f64.tan();
    assert_eq!(cone.collapse([0.7, apex], 0.0).unwrap(), Collapse::U);
    assert_eq!(
        cone.collapse([0.7, 0.5 * apex], 1e-6).unwrap(),
        Collapse::Neither
    );
    let torus = SurfaceEval::new(&Surface::Torus {
        frame: a_frame(),
        major_radius: 3.0,
        minor_radius: 3.0,
        tube_start: 0.25,
    })
    .unwrap();
    let inner = std::f64::consts::PI - 0.25;
    assert_eq!(torus.collapse([1.1, inner], 0.0).unwrap(), Collapse::U);
    assert_eq!(
        torus.collapse([1.1, inner + 0.3], 1e-6).unwrap(),
        Collapse::Neither
    );
}

/// A hemisphere held as a rational spline (u one turn on four quarter arcs, v from the equator
/// to the pole) whose pole row an importer left coinciding only to 1e-10 of its size, and a lune
/// of it between the meridians at a quarter and a half turn, its loop starting at the pole. Given
/// a reach (a tolerance) the row is a pole: the pole bounds nothing and the box is u in
/// [¼, ½], v in [0, 1]. At no reach it is no pole, and the pole's projection (from no hint, its u
/// anything) widens the box.
#[test]
fn an_imported_pole_row_is_a_pole_within_the_extents_reach() {
    let r = 15.0f64;
    let m = std::f64::consts::FRAC_1_SQRT_2;
    let circle = [
        ([1.0, 0.0], 1.0),
        ([1.0, 1.0], m),
        ([0.0, 1.0], 1.0),
        ([-1.0, 1.0], m),
        ([-1.0, 0.0], 1.0),
        ([-1.0, -1.0], m),
        ([0.0, -1.0], 1.0),
        ([1.0, -1.0], m),
        ([1.0, 0.0], 1.0),
    ];
    let quarter = [([1.0, 0.0], 1.0), ([1.0, 1.0], m), ([0.0, 1.0], 1.0)];
    let homogeneous: Vec<Vec<[f64; 4]>> = circle
        .iter()
        .enumerate()
        .map(|(i, ([cx, cy], wu))| {
            quarter
                .iter()
                .enumerate()
                .map(|(j, ([rho, z], wv))| {
                    let w = wu * wv;
                    // The pole row (j = 2) spread by ±1e-10 r in x, the seam's two ends alike.
                    let spread = if j == 2 && i % 8 != 0 {
                        if i % 2 == 0 {
                            1e-10 * r
                        } else {
                            -1e-10 * r
                        }
                    } else {
                        0.0
                    };
                    [
                        w * (cx * rho * r + spread),
                        w * (cy * rho * r),
                        w * (z * r),
                        w,
                    ]
                })
                .collect()
        })
        .collect();
    let label = Surface::RationalSpline {
        u_knots: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        homogeneous,
    };
    let meridian = |x: [f64; 3]| Curve::Circle {
        frame: Frame {
            origin: [0.; 3],
            x_axis: x,
            y_axis: [0., 0., 1.],
        },
        radius: r,
    };
    let (quarter_turn, half_turn) = (meridian([0., 1., 0.]), meridian([-1., 0., 0.]));
    let rim = Curve::Circle {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let lune = vec![
        (&quarter_turn, [0., FRAC_PI_2], false),
        (&rim, [FRAC_PI_2, std::f64::consts::PI], true),
        (&half_turn, [0., FRAC_PI_2], true),
    ];
    let [u, v] = form::extent(&label, std::slice::from_ref(&lune), 1e-6)
        .unwrap()
        .expect("an extent");
    assert!(
        (u[0] - 0.25).abs() < 1e-9 && (u[1] - 0.5).abs() < 1e-9,
        "a quarter turn: {u:?}"
    );
    assert!(v[0].abs() < 1e-9 && (v[1] - 1.0).abs() < 1e-9, "{v:?}");
    let at_rounding = form::extent(&label, &[lune], 0.0).unwrap();
    assert!(
        at_rounding.is_none_or(|[u, _]| (u[0] - 0.25).abs() > 1e-6 || (u[1] - 0.5).abs() > 1e-6),
        "at no reach the row is no pole: {at_rounding:?}"
    );
}

/// A whole turn whose seam lies a rounding short of a quarter line (as a file's seam on its
/// frame's x axis measures, 2π − 2e-12 or −2e-12) has its form from that quarter line: four
/// quarters that end where the seam's second use lies (#12760; from the quarter before, the
/// form ended a quarter short of it and the face's last quarter was off its form).
#[test]
fn a_whole_turn_a_rounding_short_of_a_quarter_line_has_its_form_from_it() {
    let cylinder = Surface::Cylinder {
        frame: a_frame(),
        radius: 21.2,
    };
    let u_knots = |s: &Surface| match s {
        Surface::RationalSpline { u_knots, .. } => (
            u_knots.first().copied().unwrap(),
            u_knots.last().copied().unwrap(),
        ),
        _ => panic!("not a NURBS: {s:?}"),
    };
    for (start, q) in [(TAU - 2e-12, 4.0), (-2.2e-12, 0.0), (TAU + 2e-12, 4.0)] {
        let (form, domain) =
            form::surface_within(&cylinder, [[start, start + TAU], [0., 10.]], 2e-5).unwrap();
        assert_eq!(u_knots(&form), (q, q + 4.0), "seam at {start}");
        assert_eq!(domain[0], [q, q + 4.0], "seam at {start}");
    }
}

/// A narrow lune between two meridians, up one to a pole and down the other to the opposite
/// pole, keeps its width round the axis wherever the loop starts and wherever the lune lies,
/// including across angle 0: the point found afresh past each pole is moved onto the branch of
/// the point before it.
#[test]
fn a_lune_between_two_poles_keeps_its_width_round_the_axis() {
    let r = 10.0f64;
    let sphere = Surface::Sphere {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let meridian = |a: f64| Curve::Circle {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [a.cos(), a.sin(), 0.],
            y_axis: [0., 0., 1.],
        },
        radius: r,
    };
    for &(centre, width) in &[
        (0.0, 0.2),
        (0.0, 0.02),
        (std::f64::consts::PI, 0.2),
        (1.0, 0.5),
        (-0.05, 0.3),
        (6.2, 0.4),
    ] {
        let (a, b) = (
            meridian(centre - width / 2.0),
            meridian(centre + width / 2.0),
        );
        let loops = [
            (
                "up a, down b",
                vec![
                    (&a, [-FRAC_PI_2, FRAC_PI_2], true),
                    (&b, [-FRAC_PI_2, FRAC_PI_2], false),
                ],
            ),
            (
                "up b, down a",
                vec![
                    (&b, [-FRAC_PI_2, FRAC_PI_2], true),
                    (&a, [-FRAC_PI_2, FRAC_PI_2], false),
                ],
            ),
            (
                "from a's middle",
                vec![
                    (&a, [0., FRAC_PI_2], true),
                    (&b, [-FRAC_PI_2, FRAC_PI_2], false),
                    (&a, [-FRAC_PI_2, 0.], true),
                ],
            ),
            (
                "from b's middle",
                vec![
                    (&b, [-FRAC_PI_2, 0.], false),
                    (&a, [-FRAC_PI_2, FRAC_PI_2], true),
                    (&b, [0., FRAC_PI_2], false),
                ],
            ),
        ];
        for (name, lune) in loops {
            let [u, v] = form::extent(&sphere, &[lune], 1e-9)
                .unwrap()
                .expect("an extent");
            assert!(
                (u[1] - u[0] - width).abs() < 1e-9,
                "centre {centre}, width {width}, {name}: {u:?}"
            );
            assert!(
                (v[0] + FRAC_PI_2).abs() < 1e-9 && (v[1] - FRAC_PI_2).abs() < 1e-9,
                "centre {centre}, width {width}, {name}: {v:?}"
            );
        }
    }
}

#[test]
fn attachment_residuals_keep_derivatives_of_the_point_and_the_moving_curve() {
    use super::constraint::{self as k, Dual, Num, V};
    // Radius five in XY. A point at (8,0,3) is three above the plane and
    // three beyond the circle. Translating the circle alone reverses the point gradient.
    let p = V {
        x: Dual::var(8.0, 0),
        y: Dual::var(0.0, 1),
        z: Dual::var(3.0, 2),
    };
    let circle = |x, y| V {
        x: Dual::num(x) + Dual::var(0.0, 3),
        y: Dual::num(y),
        z: Dual::var(0.0, 4),
    };
    let [height, radial] =
        k::point_on_circle(p, circle(5.0, 0.0), circle(0.0, 5.0), circle(-5.0, 0.0));
    assert!((height.v - 3.0).abs() < 1e-12);
    assert!((radial.v - 3.0).abs() < 1e-12);
    for (actual, expected) in height.d.into_iter().take(5).zip([0.0, 0.0, 1.0, 0.0, -1.0]) {
        assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
    }
    for (actual, expected) in radial.d.into_iter().take(5).zip([1.0, 0.0, 0.0, -1.0, 0.0]) {
        assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
    }
    let line = k::point_on_line(p, V::of([0.0, 0.0, 0.0]), V::of([10.0, 0.0, 0.0]));
    assert_eq!(line.map(|v| v.v), [0.0, 3.0, 0.0]);
    assert_eq!(line[1].d[2], 1.0);
    assert_eq!(line[2].d[1], -1.0);
}

#[test]
fn plane_measurements_and_circle_operands_have_closed_form_values() {
    use super::constraint::{self as k, Dual, Num, V};
    let origin = V::of([0.0, 0.0, 5.0]);
    let normal = V::of([0.0, 0.0, 1.0]);
    let point = V {
        x: Dual::num(4.0),
        y: Dual::num(0.0),
        z: Dual::var(2.0, 0),
    };
    let d = k::plane_distance(point, origin, normal);
    assert_eq!((d.v, d.d[0]), (-3.0, 1.0));
    let end = V {
        x: Dual::num(4.0),
        y: Dual::num(0.0),
        z: Dual::var(3.0, 0),
    };
    let (angle, length) = k::plane_angle(V::of([0.0; 3]), end, normal);
    assert!((angle.v - 3.0_f64.atan2(4.0)).abs() < 1e-12);
    assert!((angle.d[0] - 4.0 / 25.0).abs() < 1e-12);
    assert_eq!(length.v, 5.0);
    let frame = a_frame();
    assert_eq!(k::plane_point(&frame, [2.0, 4.0]), [3.0, 6.0, 3.0]);
    assert_eq!(k::plane_normal(&frame), [0.0, 0.0, 1.0]);
    for [x, y, z] in k::arc_circle_points([0.0, 0.0], [5.0, 0.0], &frame) {
        assert!(((x - 1.0).powi(2) + (y - 2.0).powi(2) - 25.0).abs() < 1e-12);
        assert_eq!(z, 3.0);
    }
    let mut other = frame.clone();
    other.origin = [100.0, 200.0, 3.0];
    other.y_axis = [0.0, -1.0, 0.0];
    assert!(k::same_plane(&frame, &other));
    other.origin[2] = 4.0;
    assert!(!k::same_plane(&frame, &other));
}

/// A lune half a turn wide (a half sphere whose rim is a meridian circle) is as wide as the
/// other half, so its width says nothing of which half it is: the face is the half on the loop's
/// left in (u, v), whichever way the loop runs, wherever it starts and wherever the seam lies,
/// across it included.
#[test]
fn a_half_sphere_lune_is_the_half_on_its_loops_left() {
    let r = 1.0f64;
    let sphere = Surface::Sphere {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        },
        radius: r,
    };
    let meridian = |a: f64| Curve::Circle {
        frame: Frame {
            origin: [0.; 3],
            x_axis: [a.cos(), a.sin(), 0.],
            y_axis: [0., 0., 1.],
        },
        radius: r,
    };
    let half = FRAC_PI_2;
    for centre in [0.0, 0.3, std::f64::consts::PI, -1.0, 6.2] {
        let (a, b) = (meridian(centre - half), meridian(centre + half));
        // Up a meridian the face is on the side of lower angle, down one on the side of higher.
        let loops = [
            (
                "up b, down a",
                centre,
                vec![
                    (&b, [-FRAC_PI_2, FRAC_PI_2], true),
                    (&a, [-FRAC_PI_2, FRAC_PI_2], false),
                ],
            ),
            (
                "up a, down b",
                centre + std::f64::consts::PI,
                vec![
                    (&a, [-FRAC_PI_2, FRAC_PI_2], true),
                    (&b, [-FRAC_PI_2, FRAC_PI_2], false),
                ],
            ),
            (
                "from b's middle",
                centre,
                vec![
                    (&b, [0., FRAC_PI_2], true),
                    (&a, [-FRAC_PI_2, FRAC_PI_2], false),
                    (&b, [-FRAC_PI_2, 0.], true),
                ],
            ),
            (
                "from a's middle",
                centre + std::f64::consts::PI,
                vec![
                    (&a, [0., FRAC_PI_2], true),
                    (&b, [-FRAC_PI_2, FRAC_PI_2], false),
                    (&a, [-FRAC_PI_2, 0.], true),
                ],
            ),
        ];
        for (name, middle, lune) in loops {
            let [u, v] = form::extent(&sphere, &[lune], 1e-9)
                .unwrap()
                .expect("an extent");
            assert!(
                (u[1] - u[0] - std::f64::consts::PI).abs() < 1e-9,
                "centre {centre}, {name}: {u:?}"
            );
            let off = (0.5 * (u[0] + u[1]) - middle).rem_euclid(TAU);
            assert!(
                off.min(TAU - off) < 1e-9,
                "centre {centre}, {name}: {u:?}, middle {middle} expected"
            );
            assert!(
                (v[0] + FRAC_PI_2).abs() < 1e-9 && (v[1] - FRAC_PI_2).abs() < 1e-9,
                "centre {centre}, {name}: {v:?}"
            );
        }
    }
}

/// The 3-4-5 pole coincidence is an arithmetic event. Enlarging a modeling tolerance
/// must not turn a nearby rim into an exactly coincident one, including far from the origin.
#[test]
fn sphere_frame_witness_does_not_grow_with_modelling_tolerance() {
    for origin in [[0.0; 3], [1e6, -1e6, 1e6], [1e9, -1e9, 1e9]] {
        let support = Frame {
            origin,
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        };
        let tilted = Frame {
            x_axis: [0.8, 0.0, -0.6],
            ..support.clone()
        };
        let rims: Vec<_> = [-4.0, 4.0]
            .into_iter()
            .map(|z| Curve::Circle {
                frame: Frame {
                    origin: [origin[0], origin[1], origin[2] + z],
                    ..support.clone()
                },
                radius: 3.0,
            })
            .collect();
        for tolerance in [2e-6, 2e-5, 2e-4] {
            let edges: Vec<_> = rims
                .iter()
                .map(|curve| (curve, [0.0, TAU], tolerance))
                .collect();
            assert_eq!(
                form::sphere_frame(&tilted, 5.0, &support, 3.0, &edges, &edges),
                support
            );
            for delta in [-1e-3, 1e-3, -1e-6, 1e-6, -1e-8, 1e-8] {
                let angle = 0.6_f64.asin() + delta;
                let near = Frame {
                    x_axis: [angle.cos(), 0.0, -angle.sin()],
                    ..support.clone()
                };
                assert_eq!(
                    form::sphere_frame(&near, 5.0, &support, 3.0, &edges, &edges),
                    near,
                    "origin {origin:?}, delta {delta}, tolerance {tolerance}"
                );
            }
        }
    }
}
