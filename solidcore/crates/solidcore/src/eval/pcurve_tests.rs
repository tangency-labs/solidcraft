#![allow(
    clippy::cast_lossless,
    clippy::type_complexity,
    clippy::needless_range_loop
)] // test arithmetic and its tables
use super::super::{form, CurveEval, Frame, SurfaceEval};
use super::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn frame(origin: [f64; 3], x_axis: [f64; 3], y_axis: [f64; 3]) -> Frame {
    Frame {
        origin,
        x_axis,
        y_axis,
    }
}
/// A frame about the axis through `origin` along +z, or −z when `down` (x stays +x).
fn about_z(origin: [f64; 3], down: bool) -> Frame {
    frame(
        origin,
        [1., 0., 0.],
        if down { [0., -1., 0.] } else { [0., 1., 0.] },
    )
}

/// A face's inward ray is normal × tangent, reversed with the loop. Every
/// permutation must give the same cyclic order, including an oblique edge.
#[test]
fn radial_order_uses_exact_face_directions_in_every_input_order() {
    for (axis, x) in [
        (Vec3::new(0., 0., 1.), Vec3::new(1., 0., 0.)),
        (Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)),
        (Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)),
        (Vec3::new(1., 2., 3.), Vec3::new(2., -1., 0.)),
    ] {
        let point = |v: Vec3| [v.x, v.y, v.z];
        let y = axis.cross(x);
        let surfaces: Vec<_> = [x, y, -x, -y]
            .into_iter()
            .enumerate()
            .map(|(i, ray)| Surface::Plane {
                origin: [0.; 3],
                u_axis: point(axis),
                v_axis: point(if i & 1 == 0 { ray } else { -ray }),
            })
            .collect();
        let curve = Curve::Line {
            start: [0.; 3],
            end: point(axis),
        };
        let pc = Curve2::Spline {
            knots: vec![0., 0., 1., 1.],
            points: vec![[0., 0.], [1., 0.]],
        };
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let input = [a, b, c, d];
                        let mut distinct = input;
                        distinct.sort_unstable();
                        if distinct != [0, 1, 2, 3] {
                            continue;
                        }
                        let faces: Vec<_> = input
                            .iter()
                            .map(|&i| RadialFace {
                                surface: &surfaces[i],
                                pcurve: &pc,
                                forward: i & 1 == 0,
                            })
                            .collect();
                        let order = radial_order(&curve, [0., 1.], &faces).unwrap();
                        let rays: Vec<_> = order.into_iter().map(|i| input[i]).collect();
                        let expected: Vec<_> = (0..4).map(|i| (a + i) % 4).collect();
                        assert_eq!(rays, expected, "{axis:?}, {input:?}");
                    }
                }
            }
        }
    }
}

/// #21183: a Ø0.12 hole 0.44 off a Ø1 bar's axis touches its wall along the bar's seam line, so
/// their rays tie in pairs (up to rounding once turned off the axes). The hole curves away faster:
/// about +z, bar back, hole back, hole forward, bar forward, so wall and space alternate.
#[test]
fn faces_tangent_along_an_edge_are_ordered_by_how_fast_they_curve_away() {
    let seam = Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![[0., 0.], [0., 1.]],
    };
    for turn in [0.0f64, 0.3, 1.1, 2.5, 4.0] {
        let (s, c) = turn.sin_cos();
        let at = |r: f64, z: f64| [r * c, r * s, z];
        let turned = |origin: [f64; 3]| frame(origin, [c, s, 0.], [-s, c, 0.]);
        let bar = Surface::Cylinder {
            frame: turned([0.; 3]),
            radius: 0.5,
        };
        let hole = Surface::Cylinder {
            frame: turned(at(0.44, 0.)),
            radius: 0.06,
        };
        let curve = Curve::Line {
            start: at(0.5, 0.),
            end: at(0.5, 1.),
        };
        let uses = [(&bar, false), (&hole, false), (&hole, true), (&bar, true)];
        for perm in 0..24usize {
            let mut pool: Vec<usize> = (0..4).collect();
            let mut k = perm;
            let mut input = Vec::new();
            for n in (1..=4).rev() {
                input.push(pool.remove(k % n));
                k /= n;
            }
            let faces: Vec<_> = input
                .iter()
                .map(|&i| RadialFace {
                    surface: uses[i].0,
                    pcurve: &seam,
                    forward: uses[i].1,
                })
                .collect();
            let order = radial_order(&curve, [0., 1.], &faces).unwrap();
            let got: Vec<_> = order.into_iter().map(|i| input[i]).collect();
            let expected: Vec<_> = (0..4).map(|n| (input[0] + n) % 4).collect();
            assert_eq!(got, expected, "turn {turn}, {input:?}");
        }
    }
    let bar = |turn: f64| {
        let (s, c) = turn.sin_cos();
        Surface::Cylinder {
            frame: frame([0.; 3], [c, s, 0.], [-s, c, 0.]),
            radius: 0.5,
        }
    };
    let (s, c) = 0.3f64.sin_cos();
    let curve = Curve::Line {
        start: [0.5 * c, 0.5 * s, 0.],
        end: [0.5 * c, 0.5 * s, 1.],
    };
    // These turns give bends a rounding apart (-2 ± 2 ulps).
    for turn in [0.0519, 0.2249] {
        let (low, high) = (bar(0.3), bar(0.3 + turn));
        let below = Curve2::Spline {
            knots: vec![0., 0., 1., 1.],
            points: vec![[-turn, 0.], [-turn, 1.]],
        };
        let faces = [
            RadialFace {
                surface: &low,
                pcurve: &seam,
                forward: true,
            },
            RadialFace {
                surface: &high,
                pcurve: &below,
                forward: true,
            },
        ];
        assert!(
            matches!(radial_order(&curve, [0., 1.], &faces), Err(Error::Failed(m)) if m.contains("tangent faces")),
            "turn {turn}"
        );
    }
}

#[test]
fn radial_order_refuses_undefined_and_indistinguishable_directions() {
    let curve = Curve::Line {
        start: [0.; 3],
        end: [0., 0., 1.],
    };
    let surface = Surface::Plane {
        origin: [0.; 3],
        u_axis: [0., 0., 1.],
        v_axis: [1., 0., 0.],
    };
    let pc = Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![[0., 0.], [1., 0.]],
    };
    let face = RadialFace {
        surface: &surface,
        pcurve: &pc,
        forward: true,
    };
    assert!(
        matches!(radial_order(&curve, [0., 1.], &[face, face]), Err(Error::Failed(m)) if m.contains("tangent faces"))
    );
    for interval in [[0., 0.], [1., 0.], [f64::NAN, 1.], [0., f64::INFINITY]] {
        assert!(matches!(
            radial_order(&curve, interval, &[face]),
            Err(Error::Invalid(_))
        ));
    }
    let collapsed = Curve::Line {
        start: [0.; 3],
        end: [0.; 3],
    };
    assert!(radial_order(&collapsed, [0., 1.], &[face]).is_err());
    let singular = Surface::Plane {
        origin: [0.; 3],
        u_axis: [0., 0., 1.],
        v_axis: [0., 0., 1.],
    };
    assert!(radial_order(
        &curve,
        [0., 1.],
        &[RadialFace {
            surface: &singular,
            ..face
        }]
    )
    .is_err());
    let bad_pc = Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![[f64::NAN, 0.], [1., 0.]],
    };
    assert!(radial_order(
        &curve,
        [0., 1.],
        &[RadialFace {
            pcurve: &bad_pc,
            ..face
        }]
    )
    .is_err());
}

fn one(curve: &Curve, interval: [f64; 2]) -> Vec<Vec<EdgeUse<'_>>> {
    vec![vec![EdgeUse {
        edge: 0,
        curve,
        interval,
        forward: true,
        tolerance: 0.0,
        given: None,
    }]]
}
/// The largest distance between the lifted pcurve and the curve, at 65 points and each span's
/// middle.
fn lift_error(surface: &Surface, curve: &Curve, interval: [f64; 2], pc: &Curve2) -> f64 {
    let (s, c) = (
        SurfaceEval::new(surface).unwrap(),
        CurveEval::new(curve).unwrap(),
    );
    let [t0, t1] = interval;
    assert_eq!(pc.interval(), Some(interval), "on the edge's interval");
    let knots = match pc {
        Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. } => knots,
    };
    let mut ts: Vec<f64> = (0..=64)
        .map(|k| {
            if k == 64 {
                t1
            } else {
                t0 + (t1 - t0) * k as f64 / 64.
            }
        })
        .collect();
    ts.extend(
        knots
            .windows(2)
            .filter(|k| k[0] < k[1])
            .map(|k| 0.5 * (k[0] + k[1])),
    );
    ts.iter()
        .map(|&t| {
            s.point(eval(pc, t).unwrap())
                .unwrap()
                .dist(c.point(t).unwrap())
        })
        .fold(0., f64::max)
}
fn segment_points(pc: &Curve2) -> [[f64; 2]; 2] {
    match pc {
        Curve2::Spline { knots, points } if knots.len() == 4 && points.len() == 2 => {
            [points[0], points[1]]
        }
        other => panic!("not a degree-1 segment: {other:?}"),
    }
}

#[test]
fn a_spline_on_a_plane_over_an_interval_whose_even_steps_round_past_its_end_is_mapped() {
    // Poppy Humanoid's edge: t0 + (t1 − t0)·1 is 1.2985843833752289, one rounding past t1, where
    // a pcurve on [t0, t1] has no point; the closed form was refused for it.
    let (t0, t1) = (0.013209624271103393f64, 1.2985843833752286f64);
    assert!(
        t0 + (t1 - t0) * 1.0 > t1,
        "the interval's even step rounds past its end"
    );
    let plane = Surface::Plane {
        origin: [0.; 3],
        u_axis: [1., 0., 0.],
        v_axis: [0., 1., 0.],
    };
    let curve = Curve::Spline {
        knots: vec![t0, t0, t0, t1, t1, t1],
        points: vec![[0., 0., 0.], [1., 2., 0.], [3., 1., 0.]],
    };
    let pc = &pcurves_within(&plane, &one(&curve, [t0, t1]), 1e-9, None).unwrap()[0][0];
    assert!(lift_error(&plane, &curve, [t0, t1], pc) < 1e-12);
}

#[test]
fn lines_polylines_and_splines_on_planes_are_their_points_mapped() {
    let plane = Surface::PlaneThrough {
        origin: [1., 2., 3.],
        u_point: [3., 2., 3.],
        v_point: [1., 2., 7.],
    };
    let line = Curve::Line {
        start: [2., 2., 4.],
        end: [5., 2., 11.],
    };
    let pc = &pcurves(&plane, &one(&line, [0., 1.]), 1e-9).unwrap()[0][0];
    // Bit for bit: the ends lift to the line's own ends.
    let [a, b] = segment_points(pc);
    assert_eq!((a, b), ([0.5, 0.25], [2., 2.]));
    // On part of the line, extended past it.
    let pc = &pcurves(&plane, &one(&line, [-1., 3.]), 1e-9).unwrap()[0][0];
    assert!(lift_error(&plane, &line, [-1., 3.], pc) <= 1e-12);
    let poly = Curve::Polyline {
        points: vec![[1., 2., 3.], [3., 2., 3.], [3., 2., 7.], [5., 2., 9.]],
    };
    let pc = &pcurves(&plane, &one(&poly, [0.5, 3.]), 1e-9).unwrap()[0][0];
    let Curve2::Spline { knots, points } = pc else {
        panic!("a spline")
    };
    assert_eq!(knots, &[0.5, 0.5, 1., 2., 3., 3.]);
    assert_eq!(points, &[[0.5, 0.], [1., 0.], [1., 1.], [2., 1.5]]);
    assert!(lift_error(&plane, &poly, [0.5, 3.], pc) <= 1e-12);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let arc = Curve::RationalSpline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        homogeneous: vec![
            [3., 2., 3., 1.],
            [3. * w, 2. * w, 5. * w, w],
            [1., 2., 5., 1.],
        ],
    };
    let pc = &pcurves(&plane, &one(&arc, [0., 1.]), 1e-9).unwrap()[0][0];
    assert!(matches!(pc, Curve2::RationalSpline { .. }));
    assert!(lift_error(&plane, &arc, [0., 1.], pc) <= 1e-12);
    let cubic = Curve::Spline {
        knots: vec![0., 0., 0., 0., 2., 2., 2., 2.],
        points: vec![[1., 2., 3.], [4., 2., 3.], [2., 2., 6.], [5., 2., 8.]],
    };
    let pc = &pcurves(&plane, &one(&cubic, [0., 2.]), 1e-9).unwrap()[0][0];
    assert_eq!(
        pc,
        &Curve2::Spline {
            knots: vec![0., 0., 0., 0., 2., 2., 2., 2.],
            points: vec![[0., 0.], [1.5, 0.], [0.5, 0.75], [2., 1.25]]
        }
    );
    // A piece of the spline is fitted instead (its knots would need cutting).
    let pc = &pcurves(&plane, &one(&cubic, [0.5, 1.5]), 1e-7).unwrap()[0][0];
    assert!(lift_error(&plane, &cubic, [0.5, 1.5], pc) <= 1e-7);
}

/// A G4 blend curve is one Bézier of degree 9 (five conditions at each end): on its sketch's
/// plane its pcurve is its points mapped, and on the face extruded from it, of degree (9, 1),
/// the kernel's own iso-line trims close the loop and lift onto the edges. Degree 9 was past the
/// evaluator's old cap of 7, so both faces were refused ("a spline surface of degree (9, 1)").
#[test]
fn a_g4_blend_of_degree_9_and_the_face_extruded_from_it_get_exact_pcurves() {
    let bezier = |z: f64| -> Vec<[f64; 3]> {
        let along_x = (0..5).map(|k| [10. + k as f64, 0., z]);
        let along_y = (5..10).map(|k| [20., k as f64 + 1., z]);
        along_x.chain(along_y).collect()
    };
    let knots: Vec<f64> = [[0.; 10], [1.; 10]].concat();
    let low = Curve::Spline {
        knots: knots.clone(),
        points: bezier(0.),
    };
    let plane = Surface::Plane {
        origin: [0.; 3],
        u_axis: [1., 0., 0.],
        v_axis: [0., 1., 0.],
    };
    let pc = &pcurves(&plane, &one(&low, [0., 1.]), 1e-9).unwrap()[0][0];
    let Curve2::Spline { knots: k, points } = pc else {
        panic!("a spline")
    };
    assert_eq!(
        (k, points.len()),
        (&knots, 10),
        "the curve's own knots and points"
    );
    assert!(lift_error(&plane, &low, [0., 1.], pc) <= 1e-12);

    let (low_pts, high_pts) = (bezier(0.), bezier(10.));
    let rows = low_pts
        .iter()
        .zip(&high_pts)
        .map(|(a, b)| vec![*a, *b])
        .collect();
    let wall = Surface::Spline {
        u_knots: knots.clone(),
        v_knots: vec![0., 0., 1., 1.],
        points: rows,
    };
    let high = Curve::Spline {
        knots,
        points: high_pts,
    };
    let right = Curve::Line {
        start: [20., 10., 0.],
        end: [20., 10., 10.],
    };
    let left = Curve::Line {
        start: [10., 0., 10.],
        end: [10., 0., 0.],
    };
    let seg = |a: [f64; 2], b: [f64; 2]| Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![a, b],
    };
    let given = [
        seg([0., 0.], [1., 0.]),
        seg([1., 0.], [1., 1.]),
        seg([0., 1.], [1., 1.]),
        seg([0., 1.], [0., 0.]),
    ];
    let edges = [(&low, true), (&right, true), (&high, false), (&left, true)];
    let l: Vec<EdgeUse<'_>> = edges
        .iter()
        .zip(&given)
        .enumerate()
        .map(|(e, ((curve, forward), g))| EdgeUse {
            edge: e as u32,
            curve,
            interval: [0., 1.],
            forward: *forward,
            tolerance: 0.0,
            given: Some(g),
        })
        .collect();
    let pcs = pcurves_within(&wall, &[l], 1e-7, Some([[0., 1.], [0., 1.]])).unwrap();
    for ((curve, _), pc) in edges.iter().zip(&pcs[0]) {
        assert!(lift_error(&wall, curve, [0., 1.], pc) <= 1e-12, "{curve:?}");
    }
}

/// A circle about the axis is a u-line: v fixed, u its angle plus a constant (minus where it
/// turns the other way), on cylinders, cones, spheres and tori, in either direction.
#[test]
fn circles_about_the_axis_are_u_lines() {
    let o = [1., -2., 3.];
    let at = |h: f64| [1., -2., 3. + h];
    let tilt = 0.3f64;
    let rotated = |origin: [f64; 3], down: bool| {
        let (s, c) = tilt.sin_cos();
        frame(
            origin,
            [c, s, 0.],
            if down { [s, -c, 0.] } else { [-s, c, 0.] },
        )
    };
    let half = 0.4f64;
    let cases = [
        (
            Surface::Cylinder {
                frame: about_z(o, false),
                radius: 5.,
            },
            7.,
            5.,
            7.,
        ),
        (
            Surface::Cone {
                frame: about_z(o, false),
                radius_at_origin: 2.,
                half_angle: half,
            },
            4.,
            2. + 4. * half.tan(),
            4.,
        ),
        (
            Surface::Sphere {
                frame: about_z(o, false),
                radius: 5.,
            },
            3.,
            4.,
            3f64.atan2(4.),
        ),
        (
            Surface::Sphere {
                frame: about_z(o, false),
                radius: 5.,
            },
            -3.,
            4.,
            (-3f64).atan2(4.),
        ),
        (
            Surface::Torus {
                frame: about_z(o, false),
                major_radius: 10.,
                minor_radius: 2.,
                tube_start: 0.0,
            },
            2f64.sqrt(),
            10. - 2f64.sqrt(),
            0.75 * PI,
        ),
    ];
    for (surface, h, r, v) in cases {
        for down in [false, true] {
            let circle = Curve::Circle {
                frame: rotated(at(h), down),
                radius: r,
            };
            for interval in [[0., TAU], [-1., 2.5], [4., 5.]] {
                let pc = &pcurves(&surface, &one(&circle, interval), 1e-9).unwrap()[0][0];
                let [a, b] = segment_points(pc);
                let sign = if down { -1. } else { 1. };
                // The u-line itself: the only freedom is the period the loop is moved by.
                let shift = a[0] - (tilt + sign * interval[0]);
                assert!(
                    (shift / TAU - (shift / TAU).round()).abs() < 1e-14,
                    "{surface:?} {down}: {a:?}"
                );
                assert!((b[0] - a[0] - sign * (interval[1] - interval[0])).abs() < 1e-13);
                assert!(
                    a[1] == b[1] && (a[1] - v).abs() < 1e-14,
                    "{surface:?}: v {a:?} {v}"
                );
                let e = lift_error(&surface, &circle, interval, pc);
                assert!(e <= 1e-12, "{surface:?} {down} {interval:?}: {e}");
            }
        }
    }
}

/// A line along a ruling of a cylinder or a cone is a v-line: u fixed, v its height.
#[test]
fn rulings_are_v_lines() {
    let half = 0.3f64;
    let cylinder = Surface::Cylinder {
        frame: about_z([0.; 3], false),
        radius: 4.,
    };
    let cone = Surface::Cone {
        frame: about_z([0., 0., 1.], false),
        radius_at_origin: 3.,
        half_angle: half,
    };
    let (s, c) = 2f64.sin_cos();
    let up = Curve::Line {
        start: [4. * c, 4. * s, -1.],
        end: [4. * c, 4. * s, 6.],
    };
    let down = Curve::Line {
        start: [4. * c, 4. * s, 6.],
        end: [4. * c, 4. * s, -1.],
    };
    let r = |z: f64| 3. + (z - 1.) * half.tan();
    let slant = Curve::Line {
        start: [r(2.) * c, r(2.) * s, 2.],
        end: [r(5.) * c, r(5.) * s, 5.],
    };
    for (surface, line, heights) in [
        (&cylinder, &up, [-1., 6.]),
        (&cylinder, &down, [6., -1.]),
        (&cone, &slant, [1., 4.]),
    ] {
        for interval in [[0., 1.], [0.25, 2.]] {
            let pc = &pcurves(surface, &one(line, interval), 1e-9).unwrap()[0][0];
            let [a, b] = segment_points(pc);
            assert_eq!(a[0], b[0]);
            assert!((a[0] - 2.).abs() < 1e-14);
            let height = |t: f64| heights[0] + (heights[1] - heights[0]) * t;
            assert!(
                (a[1] - height(interval[0])).abs() < 1e-13
                    && (b[1] - height(interval[1])).abs() < 1e-13,
                "{a:?} {b:?}"
            );
            assert!(lift_error(surface, line, interval, pc) <= 1e-12);
        }
    }
}

/// Curves with no closed form on their face are refused, never fitted: a tilted circle on a
/// sphere, a plane's ellipse on a cylinder, a circle off the axis on a plane.
#[test]
fn curves_without_a_closed_form_are_refused() {
    let (s, c) = 0.5f64.sin_cos();
    let sphere = Surface::Sphere {
        frame: about_z([0.; 3], false),
        radius: 6.,
    };
    let great = Curve::Circle {
        frame: frame([0.; 3], [1., 0., 0.], [0., c, s]),
        radius: 6.,
    };
    let cylinder = Surface::Cylinder {
        frame: about_z([0.; 3], false),
        radius: 2.,
    };
    let ellipse = Curve::Ellipse {
        frame: frame(
            [0., 0., 1.],
            [1. / (1f64 + 0.25).sqrt(), 0., 0.5 / (1f64 + 0.25).sqrt()],
            [0., 1., 0.],
        ),
        radii: [2. * (1.25f64).sqrt(), 2.],
    };
    let plane = Surface::Plane {
        origin: [0.; 3],
        u_axis: [1., 0., 0.],
        v_axis: [0., 1., 0.],
    };
    let circle = Curve::Circle {
        frame: about_z([3., 1., 0.], false),
        radius: 2.,
    };
    for (surface, curve, interval) in [
        (&sphere, &great, [0., TAU]),
        (&cylinder, &ellipse, [0.2, 6.]),
        (&plane, &circle, [0., 3.]),
    ] {
        let r = pcurves(surface, &one(curve, interval), 1e-6);
        assert!(
            matches!(&r, Err(Error::Failed(m)) if m.contains("not supported yet: no exact pcurve")),
            "{curve:?}: {r:?}"
        );
    }
}
/// The ends of each use in loop order and the loop's signed area in (u, v).
fn walk(l: &[EdgeUse<'_>], pcs: &[Curve2]) -> (Vec<([f64; 2], [f64; 2])>, f64) {
    let mut ring = Vec::new();
    let mut ends = Vec::new();
    for (u, pc) in l.iter().zip(pcs) {
        let [a, b] = u.interval;
        let ts: Vec<f64> = (0..=16)
            .map(|k| {
                if u.forward {
                    a + (b - a) * k as f64 / 16.
                } else {
                    b - (b - a) * k as f64 / 16.
                }
            })
            .collect();
        let pts: Vec<[f64; 2]> = ts.iter().map(|t| eval(pc, *t).unwrap()).collect();
        ends.push((pts[0], pts[16]));
        ring.extend_from_slice(&pts[..16]);
    }
    let n = ring.len();
    let area = (0..n)
        .map(|i| ring[i][0] * ring[(i + 1) % n][1] - ring[(i + 1) % n][0] * ring[i][1])
        .sum::<f64>()
        * 0.5;
    (ends, area)
}

/// A cylinder's wall between two full circles, with its seam line used twice: the seam's uses
/// are one period apart, the loop closes and turns counter-clockwise, all in the base period.
#[test]
fn a_seam_used_twice_gets_pcurves_a_period_apart() {
    let cylinder = Surface::Cylinder {
        frame: about_z([0.; 3], false),
        radius: 3.,
    };
    let seam = Curve::Line {
        start: [3., 0., 0.],
        end: [3., 0., 4.],
    };
    let low = Curve::Circle {
        frame: about_z([0.; 3], false),
        radius: 3.,
    };
    let high = Curve::Circle {
        frame: about_z([0., 0., 4.], false),
        radius: 3.,
    };
    let full = [0., TAU];
    let l = vec![
        EdgeUse {
            edge: 0,
            curve: &low,
            interval: full,
            forward: true,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 1,
            curve: &seam,
            interval: [0., 1.],
            forward: true,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 2,
            curve: &high,
            interval: full,
            forward: false,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 1,
            curve: &seam,
            interval: [0., 1.],
            forward: false,
            tolerance: 0.0,
            given: None,
        },
    ];
    let pcs = &pcurves(&cylinder, std::slice::from_ref(&l), 1e-9).unwrap()[0];
    let (ends, area) = walk(&l, pcs);
    for k in 0..4 {
        let (end, start) = (ends[k].1, ends[(k + 1) % 4].0);
        assert!(
            (end[0] - start[0]).abs() < 1e-12 && (end[1] - start[1]).abs() < 1e-12,
            "{k}: {end:?} {start:?}"
        );
    }
    assert!(area > 0., "counter-clockwise: {area}");
    let (a, b) = (segment_points(&pcs[1]), segment_points(&pcs[3]));
    assert!(((a[0][0] - b[0][0]).abs() - TAU).abs() < 1e-12 && a[0][1] == b[0][1]);
    for pc in pcs {
        for p in segment_points(pc) {
            assert!((-1e-12..=TAU + 1e-12).contains(&p[0]), "base period: {p:?}");
        }
    }
}

/// A sphere bounded by one meridian used twice from pole to pole: each use runs straight into
/// the poles, the two a period apart, and the loop runs along the poles' lines.
#[test]
fn meridians_run_straight_into_the_poles() {
    let sphere = Surface::Sphere {
        frame: about_z([0.; 3], false),
        radius: 6.,
    };
    let meridian = Curve::Circle {
        frame: frame([0.; 3], [1., 0., 0.], [0., 0., 1.]),
        radius: 6.,
    };
    let half = [-FRAC_PI_2, FRAC_PI_2];
    let l = vec![
        EdgeUse {
            edge: 0,
            curve: &meridian,
            interval: half,
            forward: false,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 0,
            curve: &meridian,
            interval: half,
            forward: true,
            tolerance: 0.0,
            given: None,
        },
    ];
    for tol in [1e-3, 1e-6] {
        let pcs = &pcurves(&sphere, std::slice::from_ref(&l), tol).unwrap()[0];
        for pc in pcs {
            assert!(lift_error(&sphere, &meridian, half, pc) <= tol);
            for k in 0..=16 {
                let q = eval(pc, -FRAC_PI_2 + PI * k as f64 / 16.).unwrap();
                let u = eval(&pcs[0], 0.).unwrap()[0];
                assert!(
                    (q[0] - u).abs() < 1e-9 || ((q[0] - u).abs() - TAU).abs() < 1e-9,
                    "straight: {q:?}"
                );
            }
        }
        let (a, b) = (eval(&pcs[0], 0.).unwrap(), eval(&pcs[1], 0.).unwrap());
        assert!(
            ((a[0] - b[0]).abs() - TAU).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-12,
            "{a:?} {b:?}"
        );
        let (_, area) = walk(&l, pcs);
        assert!(area > 0., "counter-clockwise: {area}");
    }
}

/// Two meridians a three-quarter turn apart bound a quarter of a sphere or three quarters of
/// it; the loop alone can't tell which. Given the face's box, the loop spans it: three quarters
/// of a turn in u (a ball revolved 270°), or a quarter where the box says so.
#[test]
fn meridians_at_a_pole_span_the_face_box() {
    let sphere = Surface::Sphere {
        frame: about_z([0.; 3], false),
        radius: 6.,
    };
    let at = |a: f64| Curve::Circle {
        frame: frame([0.; 3], [a.cos(), a.sin(), 0.], [0., 0., 1.]),
        radius: 6.,
    };
    let (first, last) = (at(0.), at(1.5 * PI));
    let half = [-FRAC_PI_2, FRAC_PI_2];
    for (box_u, span) in [([0., 1.5 * PI], 1.5 * PI), ([-FRAC_PI_2, 0.], FRAC_PI_2)] {
        for flip in [false, true] {
            let l = vec![
                EdgeUse {
                    edge: 0,
                    curve: &first,
                    interval: half,
                    forward: !flip,
                    tolerance: 0.0,
                    given: None,
                },
                EdgeUse {
                    edge: 1,
                    curve: &last,
                    interval: half,
                    forward: flip,
                    tolerance: 0.0,
                    given: None,
                },
            ];
            let pcs = &pcurves_within(
                &sphere,
                std::slice::from_ref(&l),
                1e-7,
                Some([box_u, [-FRAC_PI_2, FRAC_PI_2]]),
            )
            .unwrap()[0];
            let (a, b) = (eval(&pcs[0], 0.).unwrap()[0], eval(&pcs[1], 0.).unwrap()[0]);
            assert!(
                ((a - b).abs() - span).abs() < 1e-9,
                "box {box_u:?}, flip {flip}: u {a} and {b}"
            );
        }
    }
}

/// A whole sphere bounded only by its seam (one meridian from pole to pole, used twice), in
/// frames where rounding leaves the loop with the uses on one line a tiny positive area: the
/// two uses come out a period apart and the loop turns counter-clockwise around the whole
/// (u, v) box, in either loop order, for a circle or a NURBS meridian, with its ends at the
/// poles or within a tenth of the tolerance of them.
#[test]
fn a_seam_from_pole_to_pole_encloses_the_whole_sphere() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let tol = 1e-7;
    let frames = [
        about_z([0.; 3], false),
        about_z([1., -2., 3.], true),
        frame([1., 2., 3.], [0., 0.6, 0.8], [0., -0.8, 0.6]),
        frame([-4., 0.5, 2.], [0.36, 0.48, 0.8], [-0.8, 0.6, 0.]),
    ];
    for sf in frames {
        let sphere = Surface::Sphere {
            frame: sf.clone(),
            radius: 6.,
        };
        let ax = Axes::new(&sf);
        for turn in [0f64, 2.1, -0.7] {
            let x = ax.x * turn.cos() + ax.y * turn.sin();
            let p = |v: Vec3| [v.x, v.y, v.z];
            let (o, r, z) = (ax.o, x * 6., ax.z * 6.);
            let circle = Curve::Circle {
                frame: Frame {
                    origin: p(o),
                    x_axis: p(x),
                    y_axis: p(ax.z),
                },
                radius: 6.,
            };
            for off in [0., 0.1 * tol] {
                // South pole to north pole through the equator, as two rational quarter arcs,
                // the ends `off` to the side of the poles.
                let h = |v: Vec3, wt: f64| [v.x * wt, v.y * wt, v.z * wt, wt];
                let side = ax.y * off;
                let nurbs = Curve::RationalSpline {
                    knots: vec![0., 0., 0., 0.5, 0.5, 1., 1., 1.],
                    homogeneous: vec![
                        h(o - z + side, 1.),
                        h(o - z + r, w),
                        h(o + r, 1.),
                        h(o + z + r, w),
                        h(o + z - side, 1.),
                    ],
                };
                // The circle on the sphere as it is; the NURBS meridian, on a parameter of its own,
                // on the sphere's NURBS form (one turn is four quarters there, the latitude two).
                let (form, _) =
                    form::surface(&sphere, [[0., TAU], [-FRAC_PI_2, FRAC_PI_2]]).unwrap();
                for (seam, iv, on, period, height) in [
                    (&circle, [-FRAC_PI_2, FRAC_PI_2], &sphere, TAU, PI),
                    (&nurbs, [0., 1.], &form, 4., 2.),
                ] {
                    for order in [[true, false], [false, true]] {
                        let l: Vec<EdgeUse> = order
                            .iter()
                            .map(|&forward| EdgeUse {
                                edge: 0,
                                curve: seam,
                                interval: iv,
                                forward,
                                tolerance: 0.0,
                                given: None,
                            })
                            .collect();
                        let pcs = &pcurves(on, std::slice::from_ref(&l), tol).unwrap()[0];
                        let what = format!("{sf:?} {turn} {off} {seam:?} {order:?}");
                        for pc in pcs {
                            assert!(lift_error(on, seam, iv, pc) <= tol, "{what}");
                        }
                        let mid = 0.5 * (iv[0] + iv[1]);
                        let (a, b) = (eval(&pcs[0], mid).unwrap(), eval(&pcs[1], mid).unwrap());
                        assert!(
                            ((a[0] - b[0]).abs() - period).abs() < 1e-6
                                && (a[1] - b[1]).abs() < 1e-12,
                            "a period apart: {a:?} {b:?} {what}"
                        );
                        let (ends, area) = walk(&l, pcs);
                        for k in 0..2 {
                            assert!(
                                (ends[k].1[1] - ends[(k + 1) % 2].0[1]).abs() < 1e-6,
                                "closed in v: {ends:?} {what}"
                            );
                        }
                        // The box less the corners `walk` cuts (it joins each use's 16th
                        // sample to the next use's start): about 15/16 of it.
                        assert!(
                            (area / (period * height * 15. / 16.) - 1.).abs() < 1e-2,
                            "counter-clockwise around the box: {area} {what}"
                        );
                    }
                }
            }
        }
    }
}

/// A whole torus bounded only by its two seams (a circle about the axis and a circle around
/// the tube, each used twice): no pole, continuity alone puts each seam's uses a period apart
/// and the loop around the whole (u, v) box.
#[test]
fn a_torus_bounded_by_its_seams_spans_both_periods() {
    let frames = [
        about_z([0.; 3], false),
        frame([1., 2., 3.], [0., 0.6, 0.8], [0., -0.8, 0.6]),
    ];
    for sf in frames {
        let torus = Surface::Torus {
            frame: sf.clone(),
            major_radius: 10.,
            minor_radius: 2.,
            tube_start: 0.0,
        };
        let ax = Axes::new(&sf);
        let p = |v: Vec3| [v.x, v.y, v.z];
        // The outer equator (v = 0, u from 0) and the tube circle at u = 0 (v from 0, upwards).
        let around = Curve::Circle {
            frame: sf.clone(),
            radius: 12.,
        };
        let tube = Curve::Circle {
            frame: Frame {
                origin: p(ax.o + ax.x * 10.),
                x_axis: p(ax.x),
                y_axis: p(ax.z),
            },
            radius: 2.,
        };
        let full = [0., TAU];
        let l = vec![
            EdgeUse {
                edge: 0,
                curve: &around,
                interval: full,
                forward: true,
                tolerance: 0.0,
                given: None,
            },
            EdgeUse {
                edge: 1,
                curve: &tube,
                interval: full,
                forward: true,
                tolerance: 0.0,
                given: None,
            },
            EdgeUse {
                edge: 0,
                curve: &around,
                interval: full,
                forward: false,
                tolerance: 0.0,
                given: None,
            },
            EdgeUse {
                edge: 1,
                curve: &tube,
                interval: full,
                forward: false,
                tolerance: 0.0,
                given: None,
            },
        ];
        let pcs = &pcurves(&torus, std::slice::from_ref(&l), 1e-9).unwrap()[0];
        let (ends, area) = walk(&l, pcs);
        for k in 0..4 {
            let (end, start) = (ends[k].1, ends[(k + 1) % 4].0);
            assert!(
                (end[0] - start[0]).abs() < 1e-12 && (end[1] - start[1]).abs() < 1e-12,
                "{k}: {end:?} {start:?}"
            );
        }
        assert!(
            (area / (TAU * TAU) - 1.).abs() < 1e-12,
            "counter-clockwise around the box: {area}"
        );
    }
}

/// The pcurves of a loop on a spline closed in u (period 1): each lifts within `tol`, the
/// seam's two uses (`seam`: their positions) lie one period apart, the loop closes (in v alone
/// where it meets a pole) and turns counter-clockwise once around the whole (u, v) box, all in
/// the base period.
fn closes_around_the_box(
    surface: &Surface,
    l: &[EdgeUse<'_>],
    seam: [usize; 2],
    pole: Option<usize>,
    tol: f64,
    what: &str,
) {
    let pcs = &pcurves(surface, &[l.to_vec()], tol).unwrap_or_else(|e| panic!("{what}: {e}"))[0];
    for (u, pc) in l.iter().zip(pcs) {
        assert!(
            lift_error(surface, u.curve, u.interval, pc) <= tol,
            "{what}: lifts"
        );
    }
    let iv = l[seam[0]].interval;
    let mid = 0.5 * (iv[0] + iv[1]);
    let (a, b) = (
        eval(&pcs[seam[0]], mid).unwrap(),
        eval(&pcs[seam[1]], mid).unwrap(),
    );
    let dom = SurfaceEval::new(surface).unwrap().domain();
    let period = dom.period[0].unwrap();
    assert!(
        ((a[0] - b[0]).abs() - period).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9,
        "{what}: seam uses a period apart: {a:?} {b:?}"
    );
    let (ends, area) = walk(l, pcs);
    let n = l.len();
    for k in 0..n {
        let (end, start) = (ends[k].1, ends[(k + 1) % n].0);
        let free_u = pole == Some((k + 1) % n);
        assert!(
            (free_u || (end[0] - start[0]).abs() < 1e-9) && (end[1] - start[1]).abs() < 1e-9,
            "{what}: closes at {k}: {end:?} {start:?}"
        );
    }
    // At a pole `walk` cuts the corner (it joins each use's 16th sample to the next use's
    // start, across the pole's line): a sliver of the box less.
    let [u, v] = dom.range.map(|r| r.unwrap());
    let area = area / ((u.1 - u.0) * (v.1 - v.0));
    let least = if pole.is_some() { 0.9 } else { 1. - 1e-6 };
    assert!(
        area >= least && area < 1. + 1e-6,
        "{what}: counter-clockwise around the box: {area}"
    );
    for pc in pcs {
        let (Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. }) = pc;
        for k in 0..=32 {
            let t = knots[0] + (knots[knots.len() - 1] - knots[0]) * k as f64 / 32.;
            let q = eval(pc, t).unwrap();
            assert!(
                (u.0 - 1e-9..=u.1 + 1e-9).contains(&q[0]),
                "{what}: base period: {q:?}"
            );
        }
    }
}

/// A loop that starts on a closed spline's seam, its lowest point a hair below the range's start
/// (an edge a few µm off the seam line, a writer's rounding: 1e-6 of the period, about 2e-5 mm on
/// this cylinder of radius 3), stays on the range at a tolerance that covers that: moved a whole
/// period on, past the range's end, the face's pcurves lay off its NURBS form, which measured it
/// short. At a tolerance below it the loop stays where its lowest point puts it.
#[test]
fn a_loop_a_hair_below_a_closed_splines_range_stays_on_it() {
    let (cylinder, _) = form::surface(
        &Surface::Cylinder {
            frame: about_z([0.; 3], false),
            radius: 3.,
        },
        [[0., TAU], [0., 4.]],
    )
    .unwrap();
    let dom = SurfaceEval::new(&cylinder).unwrap().domain();
    let (a, b) = dom.range[0].unwrap();
    let period = dom.period[0].unwrap();
    let (lo, hi) = (a - 1e-6 * period, a + 0.75 * period);
    let line = |p: [f64; 2], q: [f64; 2]| Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![p, q],
    };
    // Three quarters of a turn, counter-clockwise in (u, v).
    let given = [
        line([lo, 0.], [hi, 0.]),
        line([hi, 0.], [hi, 4.]),
        line([hi, 4.], [lo, 4.]),
        line([lo, 4.], [lo, 0.]),
    ];
    let edge = Curve::Line {
        start: [0.; 3],
        end: [1., 0., 0.],
    };
    let l: Vec<EdgeUse<'_>> = given
        .iter()
        .enumerate()
        .map(|(e, g)| EdgeUse {
            edge: e as u32,
            curve: &edge,
            interval: [0., 1.],
            forward: true,
            tolerance: 0.0,
            given: Some(g),
        })
        .collect();
    let pcs = pcurves(&cylinder, std::slice::from_ref(&l), 1e-4).unwrap();
    for pc in &pcs[0] {
        for q in points(pc) {
            assert!(
                (lo - 1e-12..=hi + 1e-12).contains(&q[0]) && q[0] < b,
                "on the range [{a}, {b}]: {q:?}"
            );
        }
    }
    let pcs = pcurves(&cylinder, &[l], 1e-7).unwrap();
    for (got, given) in pcs[0].iter().zip(&given) {
        let moved: Vec<[f64; 2]> = points(given)
            .iter()
            .map(|q| [q[0] + period, q[1]])
            .collect();
        assert_eq!(points(got), moved, "below the tolerance: a period on");
    }
}

#[test]
fn stored_trim_hulls_leaving_a_base_period_request_a_topology_edit() {
    let surface = Surface::Spline {
        u_knots: vec![0., 0., 1., 2., 3., 4., 4.],
        v_knots: vec![0., 0., 3., 3.],
        points: [[-2., -2.], [2., -2.], [2., 2.], [-2., 2.], [-2., -2.]]
            .map(|[x, y]| vec![[x, y, 0.], [x, y, 3.]])
            .to_vec(),
    };
    let domain = SurfaceEval::new(&surface).unwrap().domain();
    let line = |a, b| Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![a, b],
    };
    for u in [0.0, 4.0] {
        assert!(!leaves_base_period(domain, &line([u, 0.], [u, 3.])));
    }
    assert!(leaves_base_period(domain, &line([3.9, 0.], [4.1, 3.])));
    assert!(leaves_base_period(domain, &line([-0.1, 0.], [0.1, 3.])));
    // The analogous cut in v is handled the same way; an open direction is not a seam.
    let Surface::Spline {
        u_knots,
        v_knots,
        points,
    } = &surface
    else {
        unreachable!()
    };
    let transposed = Surface::Spline {
        u_knots: v_knots.clone(),
        v_knots: u_knots.clone(),
        points: (0..2)
            .map(|j| points.iter().map(|row| row[j]).collect())
            .collect(),
    };
    assert!(leaves_base_period(
        SurfaceEval::new(&transposed).unwrap().domain(),
        &line([0., 3.9], [3., 4.1])
    ));
    assert!(!leaves_base_period(domain, &line([0., -1.], [4., 4.])));
}

/// A full-period loop with a small overrun at its cut belongs around the base
/// period. Shifting its lowest control point inside moved most of it one turn out.
#[test]
fn a_full_period_loop_is_centered_on_the_base_period() {
    let surface = Surface::Spline {
        u_knots: vec![0., 0., 1., 2., 3., 4., 4.],
        v_knots: vec![0., 0., 3., 3.],
        points: [[-2., -2.], [2., -2.], [2., 2.], [-2., 2.], [-2., -2.]]
            .map(|[x, y]| vec![[x, y, 0.], [x, y, 3.]])
            .to_vec(),
    };
    let on = On::new(&surface).unwrap();
    let epsilon = 1e-5;
    let line = |a, b| Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![a, b],
    };
    for offset in [-8., 0., 12.] {
        let mut uses = vec![
            (
                line([offset - epsilon, 0.], [offset + 4. + epsilon, 0.]),
                true,
            ),
            (
                line([offset + 4. + epsilon, 0.], [offset + 4. - epsilon, 3.]),
                true,
            ),
            (
                line([offset + 4. - epsilon, 3.], [offset + epsilon, 3.]),
                true,
            ),
            (line([offset + epsilon, 3.], [offset - epsilon, 0.]), true),
        ];
        on.close(&mut uses, 1e-3).unwrap();
        for (pc, _) in uses {
            for p in points(&pc) {
                assert!(p[0] >= -2. * epsilon && p[0] <= 4. + 2. * epsilon, "{p:?}");
            }
        }
    }
}

/// Splines closed on themselves in u (their first and last control rows the same) are periodic
/// there, one knot interval per period: a ring of a torus bounded by two circles and a seam used
/// twice (SMLib's fillet round a hole's rim), a NURBS cylinder, and a NURBS dome whose seam runs
/// into its pole. Each seam's uses come out a period apart, never both on u = 0.
#[test]
fn a_seam_on_a_spline_closed_in_u_gets_pcurves_a_period_apart() {
    let full = [0., TAU];
    let (big, small) = (5., 1.);
    // Each surface and circle on its NURBS form (`form`), as SolidCraft's builders give them.
    let on = |c: &Curve, iv: [f64; 2]| form::curve(c, iv).unwrap();
    // The torus band: the tube's quarter from its outer equator to its top.
    let torus = Surface::Torus {
        frame: about_z([0.; 3], false),
        major_radius: big,
        minor_radius: small,
        tube_start: 0.0,
    };
    let (band, _) = form::surface(&torus, [full, [0., FRAC_PI_2]]).unwrap();
    let outer = on(
        &Curve::Circle {
            frame: about_z([0.; 3], false),
            radius: big + small,
        },
        full,
    );
    let top = on(
        &Curve::Circle {
            frame: about_z([0., 0., small], false),
            radius: big,
        },
        full,
    );
    let quarter = on(
        &Curve::Circle {
            frame: frame([big, 0., 0.], [1., 0., 0.], [0., 0., 1.]),
            radius: small,
        },
        [0., FRAC_PI_2],
    );
    let l = vec![
        EdgeUse {
            edge: 0,
            curve: &outer.0,
            interval: outer.1,
            forward: true,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 1,
            curve: &quarter.0,
            interval: quarter.1,
            forward: true,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 2,
            curve: &top.0,
            interval: top.1,
            forward: false,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 1,
            curve: &quarter.0,
            interval: quarter.1,
            forward: false,
            tolerance: 0.0,
            given: None,
        },
    ];
    for tol in [1e-4, 1e-7] {
        closes_around_the_box(
            &band,
            &l,
            [1, 3],
            None,
            tol,
            &format!("torus band, tol {tol}"),
        );
    }
    // The cylinder: radius 3 from z = 0 to z = 4, its seam a line.
    let (cylinder, _) = form::surface(
        &Surface::Cylinder {
            frame: about_z([0.; 3], false),
            radius: 3.,
        },
        [full, [0., 4.]],
    )
    .unwrap();
    let low = on(
        &Curve::Circle {
            frame: about_z([0.; 3], false),
            radius: 3.,
        },
        full,
    );
    let high = on(
        &Curve::Circle {
            frame: about_z([0., 0., 4.], false),
            radius: 3.,
        },
        full,
    );
    let line = Curve::Line {
        start: [3., 0., 0.],
        end: [3., 0., 4.],
    };
    let l = vec![
        EdgeUse {
            edge: 0,
            curve: &low.0,
            interval: low.1,
            forward: true,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 1,
            curve: &line,
            interval: [0., 1.],
            forward: true,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 2,
            curve: &high.0,
            interval: high.1,
            forward: false,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 1,
            curve: &line,
            interval: [0., 1.],
            forward: false,
            tolerance: 0.0,
            given: None,
        },
    ];
    closes_around_the_box(&cylinder, &l, [1, 3], None, 1e-7, "cylinder");
    // The dome: radius 6 from its equator to its pole, the seam a meridian.
    let (dome, _) = form::surface(
        &Surface::Sphere {
            frame: about_z([0.; 3], false),
            radius: 6.,
        },
        [full, [0., FRAC_PI_2]],
    )
    .unwrap();
    let equator = on(
        &Curve::Circle {
            frame: about_z([0.; 3], false),
            radius: 6.,
        },
        full,
    );
    let meridian = on(
        &Curve::Circle {
            frame: frame([0.; 3], [1., 0., 0.], [0., 0., 1.]),
            radius: 6.,
        },
        [0., FRAC_PI_2],
    );
    let l = vec![
        EdgeUse {
            edge: 0,
            curve: &equator.0,
            interval: equator.1,
            forward: true,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 1,
            curve: &meridian.0,
            interval: meridian.1,
            forward: true,
            tolerance: 0.0,
            given: None,
        },
        EdgeUse {
            edge: 1,
            curve: &meridian.0,
            interval: meridian.1,
            forward: false,
            tolerance: 0.0,
            given: None,
        },
    ];
    closes_around_the_box(&dome, &l, [1, 2], Some(2), 1e-7, "dome");
    // A spline whose ends merely come near each other stays open: no period, so the seam's
    // second use cannot be moved and the loop does not close around the box.
    let Surface::RationalSpline {
        u_knots,
        v_knots,
        homogeneous,
    } = &cylinder
    else {
        panic!("a rational spline")
    };
    let mut open = homogeneous.clone();
    open[8][0][1] += 1e-3;
    let open = Surface::RationalSpline {
        u_knots: u_knots.clone(),
        v_knots: v_knots.clone(),
        homogeneous: open,
    };
    assert_eq!(
        SurfaceEval::new(&open).unwrap().domain().period,
        [None, None]
    );
    assert_eq!(
        SurfaceEval::new(&cylinder).unwrap().domain().period,
        [Some(4.), None]
    );
}

/// A loop once round a spline closed in u, given by its producer a period on, with one use
/// starting 1.56e-9 of a period short of the vertex where the use before it ends, at the period's
/// start: as SMLib's trims gave face #714 of the file in #19315 (edge #449 ends at u = 1, edge
/// #450 starts at 1 - 1.56e-9). The loop comes back by the whole period, into the surface's range
/// but for that rounding; left a period past it, the face's domain (the loops' box within the
/// range) would hold only that rounding of it.
#[test]
fn a_loop_round_a_closed_spline_given_a_period_on_and_a_rounding_short_lands_in_its_range() {
    fn use_of(edge: u32, curve: &Curve, interval: [f64; 2], forward: bool) -> EdgeUse<'_> {
        EdgeUse {
            edge,
            curve,
            interval,
            forward,
            tolerance: 0.0,
            given: None,
        }
    }
    let full = [0., TAU];
    let on = |c: &Curve, iv: [f64; 2]| form::curve(c, iv).unwrap();
    let (cylinder, _) = form::surface(
        &Surface::Cylinder {
            frame: about_z([0.; 3], false),
            radius: 3.,
        },
        [full, [0., 4.]],
    )
    .unwrap();
    let low = on(
        &Curve::Circle {
            frame: about_z([0.; 3], false),
            radius: 3.,
        },
        full,
    );
    let high = on(
        &Curve::Circle {
            frame: about_z([0., 0., 4.], false),
            radius: 3.,
        },
        full,
    );
    let line = Curve::Line {
        start: [3., 0., 0.],
        end: [3., 0., 4.],
    };
    let l = vec![
        use_of(0, &low.0, low.1, true),
        use_of(1, &line, [0., 1.], true),
        use_of(2, &high.0, high.1, false),
        use_of(1, &line, [0., 1.], false),
    ];
    let base = pcurves(&cylinder, std::slice::from_ref(&l), 1e-7)
        .unwrap()
        .remove(0);
    let [u0, u1] = {
        let r = SurfaceEval::new(&cylinder).unwrap().domain().range[0].unwrap();
        [r.0, r.1]
    };
    let period = SurfaceEval::new(&cylinder).unwrap().domain().period[0].unwrap();
    assert_eq!((u0, u1, period), (0., 4., 4.));
    // The loop as made lies in the range, from the seam at its start: the lower circle starts
    // there, where the seam's second use ends (run backwards: its pcurve, in the line's
    // direction, starts there).
    let lowest = base
        .iter()
        .flat_map(points)
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min);
    assert_eq!(lowest, u0);
    assert_eq!(points(&base[0])[0], [u0, 0.]);
    assert_eq!(points(&base[3])[0], [u0, 0.]);
    // Given a period on, the circle starting short of where the seam ends: the vertex is
    // where the seam puts it, at the period's start. By the rounding of the file, and by a
    // thousandth of a period, which no slack for rounding on the lowest point covers.
    for short in [1.56e-9 * period, 1e-3 * period] {
        let mut given: Vec<Curve2> = base
            .iter()
            .map(|c| {
                let mut c = c.clone();
                move_by(&mut c, 0, period);
                c
            })
            .collect();
        match &mut given[0] {
            Curve2::Spline { points, .. } => points[0][0] -= short,
            Curve2::RationalSpline { homogeneous, .. } => {
                homogeneous[0][0] -= short * homogeneous[0][2];
            }
        }
        let l_given: Vec<EdgeUse<'_>> = l
            .iter()
            .zip(&given)
            .map(|(u, g)| EdgeUse {
                given: Some(g),
                ..*u
            })
            .collect();
        let out = &pcurves(&cylinder, &[l_given], 1e-7).unwrap()[0];
        for (k, (got, g)) in out.iter().zip(&given).enumerate() {
            // Moved back by the whole period: the made pcurve, the circle's start short.
            let mut want = g.clone();
            move_by(&mut want, 0, -period);
            let (p, q) = (points(got), points(&want));
            assert_eq!(p.len(), q.len(), "use {k}");
            for (p, q) in p.iter().zip(&q) {
                assert!(
                    (p[0] - q[0]).abs() <= 1e-12 * period && (p[1] - q[1]).abs() <= 1e-12,
                    "short {short}, use {k}: {p:?}, not {q:?} (u range [{u0}, {u1}])"
                );
            }
        }
    }
}

/// A loop inside one period of a spline closed in u whose control polygon reaches past the
/// period's end while its curve does not: a cubic from (3.5, 0.2) to (3.5, 0.8) with control u
/// 3.5, 4.5625, 3.5, 3.5 (its u peaks at 3.5 + 3 · 1.0625 · 4/27 = 3.97 at t = 1/3) and the line
/// back. Its lowest point, u = 3.5, lies in the base period [0, 4], so it stays where it is: a
/// period lower, the whole loop would lie outside the range.
#[test]
fn a_loop_whose_control_polygon_reaches_past_its_period_stays_in_it() {
    let full = [0., TAU];
    let (cylinder, _) = form::surface(
        &Surface::Cylinder {
            frame: about_z([0.; 3], false),
            radius: 3.,
        },
        [full, [0., 4.]],
    )
    .unwrap();
    assert_eq!(
        SurfaceEval::new(&cylinder).unwrap().domain().period,
        [Some(4.), None]
    );
    let arc = Curve2::Spline {
        knots: vec![0., 0., 0., 0., 1., 1., 1., 1.],
        points: vec![[3.5, 0.2], [4.5625, 0.4], [3.5, 0.6], [3.5, 0.8]],
    };
    let back = Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![[3.5, 0.8], [3.5, 0.2]],
    };
    // The edges' curves are not read: a given pcurve is taken as it is.
    let line = Curve::Line {
        start: [3., 0., 0.],
        end: [3., 0., 4.],
    };
    let l = [(0, &arc), (1, &back)].map(|(edge, pc)| EdgeUse {
        edge,
        curve: &line,
        interval: [0., 1.],
        forward: true,
        tolerance: 0.0,
        given: Some(pc),
    });
    let out = &pcurves(&cylinder, &[l.to_vec()], 1e-7).unwrap()[0];
    for (k, (got, given)) in out.iter().zip([&arc, &back]).enumerate() {
        assert_eq!(points(got), points(given), "use {k}: left where it is");
    }
}

/// Loops on a spline closed in u (period and range [0, 4]) that lie outside the range both where
/// their lowest point puts them and a period lower stay where their lowest point puts them: a
/// rectangle across the seam, u 3 to 5, and one a period wide from a ten-thousandth of a period
/// short of the range's end, u 3.9996 to 7.9996, whose uses meet exactly. Both are placed as on
/// main: only a loop that lies in the range a period lower, to within how far its uses miss each
/// other, is moved there.
#[test]
fn a_loop_outside_the_range_a_period_lower_too_stays_where_its_lowest_point_puts_it() {
    let full = [0., TAU];
    let (cylinder, _) = form::surface(
        &Surface::Cylinder {
            frame: about_z([0.; 3], false),
            radius: 3.,
        },
        [full, [0., 4.]],
    )
    .unwrap();
    let domain = SurfaceEval::new(&cylinder).unwrap().domain();
    assert_eq!(
        (domain.range[0], domain.period[0]),
        (Some((0., 4.)), Some(4.))
    );
    let line = Curve::Line {
        start: [3., 0., 0.],
        end: [3., 0., 4.],
    };
    for [a, b] in [[3., 5.], [3.9996, 7.9996]] {
        let corners = [[a, 0.2], [b, 0.2], [b, 0.8], [a, 0.8]];
        let sides: Vec<Curve2> = (0..4)
            .map(|k| Curve2::Spline {
                knots: vec![0., 0., 1., 1.],
                points: vec![corners[k], corners[(k + 1) % 4]],
            })
            .collect();
        let l: Vec<EdgeUse<'_>> = sides
            .iter()
            .zip(0..)
            .map(|(pc, edge)| EdgeUse {
                edge,
                curve: &line,
                interval: [0., 1.],
                forward: true,
                tolerance: 0.0,
                given: Some(pc),
            })
            .collect();
        let out = &pcurves(&cylinder, &[l], 1e-7).unwrap()[0];
        assert_eq!(out.len(), 4);
        for (k, (got, given)) in out.iter().zip(&sides).enumerate() {
            assert_eq!(
                points(got),
                points(given),
                "u {a} to {b}, use {k}: left where it is"
            );
        }
    }
}

/// A loop on a sphere from its south pole up the meridian at u = 2π - 0.5, along the equator to
/// u = 2π + 2.5 and back down to the pole along the meridian at u = 2π + 0.5: its two uses meet at
/// the pole a radian apart, where u is free, and exactly elsewhere. It lies outside the range
/// [0, 2π] where its lowest point puts it and would lie inside it a period lower to within that
/// radian; the pole's free parameter is no rounding, so it stays as on main.
#[test]
fn a_loop_whose_uses_meet_at_a_pole_a_radian_apart_stays_where_its_lowest_point_puts_it() {
    let sphere = Surface::Sphere {
        frame: about_z([0.; 3], false),
        radius: 6.,
    };
    let domain = SurfaceEval::new(&sphere).unwrap().domain();
    assert_eq!(domain.period[0], Some(TAU));
    let (a, b, c) = (TAU - 0.5, TAU + 2.5, TAU + 0.5);
    let corners = [[a, -FRAC_PI_2], [a, 0.], [b, 0.], [c, 0.], [c, -FRAC_PI_2]];
    let sides: Vec<Curve2> = corners
        .windows(2)
        .map(|w| Curve2::Spline {
            knots: vec![0., 0., 1., 1.],
            points: w.to_vec(),
        })
        .collect();
    // The edges' curves are not read: a given pcurve is taken as it is.
    let line = Curve::Line {
        start: [6., 0., 0.],
        end: [0., 0., -6.],
    };
    let l: Vec<EdgeUse<'_>> = sides
        .iter()
        .zip(0..)
        .map(|(pc, edge)| EdgeUse {
            edge,
            curve: &line,
            interval: [0., 1.],
            forward: true,
            tolerance: 0.0,
            given: Some(pc),
        })
        .collect();
    let out = &pcurves(&sphere, &[l], 1e-7).unwrap()[0];
    assert_eq!(out.len(), 4);
    for (k, (got, given)) in out.iter().zip(&sides).enumerate() {
        assert_eq!(points(got), points(given), "use {k}: left where it is");
    }
}

/// An intersection record evaluates to the point of both surfaces in the leader's normal
/// plane; it has no exact pcurve on either.
#[test]
fn intersection_curves_lie_on_both_surfaces() {
    // The cylinder x² + y² = 25 cut by the plane z = 20 + x tan 0.4: an ellipse.
    let cylinder = Surface::Cylinder {
        frame: about_z([0.; 3], false),
        radius: 5.,
    };
    let k = 0.4f64.tan();
    let plane = Surface::Plane {
        origin: [0., 0., 20.],
        u_axis: [1., 0., k],
        v_axis: [0., 1., 0.],
    };
    // A coarse leader through points of the curve, its chords up to 0.03 mm off it.
    let leader: Vec<[f64; 3]> = (0..=12)
        .map(|i| {
            let a = 0.2 + 2.5 * i as f64 / 12.;
            [5. * a.cos(), 5. * a.sin(), 20. + 5. * a.cos() * k]
        })
        .collect();
    let curve = Curve::SurfaceIntersection {
        surfaces: [Box::new(cylinder.clone()), Box::new(plane.clone())],
        leader: Box::new(Curve::Polyline { points: leader }),
        tolerance: 0.2,
    };
    let eval = CurveEval::new(&curve).unwrap();
    for i in 0..=48 {
        let p = eval.point(12. * i as f64 / 48.).unwrap();
        assert!(
            (p.x.hypot(p.y) - 5.).abs() < 1e-11 && (p.z - 20. - p.x * k).abs() < 1e-11,
            "{i}: {p:?}"
        );
    }
    // It has no closed-form pcurve on either surface (an intersection record is a construction,
    // not a NURBS): refused, never fitted.
    for surface in [&cylinder, &plane] {
        assert!(
            matches!(pcurves(surface, &one(&curve, [0., 12.]), 1e-6), Err(Error::Failed(m)) if m.contains("no exact pcurve"))
        );
    }
}

/// A thread flank as a screw sweep leaves it: a spline strip winding twenty-two turns of pitch 1 about
/// z, rising from radius 3 to radius 4 by half the pitch, so one turn lies 0.89 mm from the next,
/// and its v = ½ line (its rows' midpoints on its u knots, so it lies on the strip).
fn thread_flank() -> (Surface, Curve) {
    helical_strip([3., 4.], 0.5, 0.5)
}

/// A spline strip winding twenty-two turns of pitch 1 about z from radius `r[0]` (v = 0) to `r[1]`
/// (v = 1), rising by `rise` across, and its line at v = `at` (its rows' points there on its u
/// knots, so it lies on the strip).
fn helical_strip(r: [f64; 2], rise: f64, at: f64) -> (Surface, Curve) {
    // Control points 7.3 to a turn (161 over 22 turns), so the knots fall at other angles on
    // each turn.
    const PER_TURN: f64 = 7.3;
    let n = 161;
    let helix = |r: f64, rise: f64| -> Vec<[f64; 3]> {
        (0..n)
            .map(|i| {
                let a = TAU * i as f64 / PER_TURN;
                [r * a.cos(), r * a.sin(), a / TAU + rise]
            })
            .collect()
    };
    let (inner, outer) = (helix(r[0], 0.), helix(r[1], rise));
    let mut u_knots = vec![0.; 4];
    u_knots.extend((1..n - 3).map(|k| k as f64 / (n - 3) as f64));
    u_knots.extend([1.; 4]);
    let surface = Surface::Spline {
        u_knots: u_knots.clone(),
        v_knots: vec![0., 0., 1., 1.],
        points: inner
            .iter()
            .zip(&outer)
            .map(|(a, b)| vec![*a, *b])
            .collect(),
    };
    let middle = Curve::Spline {
        knots: u_knots,
        points: inner
            .iter()
            .zip(&outer)
            .map(|(a, b)| [0, 1, 2].map(|k| (1. - at) * a[k] + at * b[k]))
            .collect(),
    };
    (surface, middle)
}

/// Edges along a surface winding several turns (the flank's middle line, from mid-turn to
/// mid-turn, and a short run of it) get pcurves on their own turn, never the next one.
#[test]
fn edges_on_a_spline_winding_several_turns_stay_on_their_turn() {
    let (surface, middle) = thread_flank();
    for interval in [[0., 1.], [0.0625, 0.9375], [0.40625, 0.4375]] {
        for tol in [1e-3, 1e-6] {
            let pc = &pcurves(&surface, &one(&middle, interval), tol).unwrap()[0][0];
            let e = lift_error(&surface, &middle, interval, pc);
            assert!(e <= tol, "{interval:?} at {tol}: {e}");
        }
    }
}

/// A thread's root band, 0.93 of the pitch tall, its turns 0.07 mm apart: an edge along its lower
/// rim (as where the root meets a shoulder) gets a pcurve on its own turn, though the rim of the
/// turn below passes nearer some of the search's starts than its own turn does.
#[test]
fn an_edge_along_the_rim_of_a_band_winding_close_to_itself_stays_on_its_turn() {
    let (surface, rim) = helical_strip([5., 5.], 0.93, 0.);
    // Short edges starting every 1/64 of a turn round one turn.
    for k in 0..64 {
        let t0 = 0.5 + f64::from(k) / (64. * 22.);
        let interval = [t0, t0 + 1. / 64.];
        let pc = &pcurves(&surface, &one(&rim, interval), 2e-4).unwrap()[0][0];
        let e = lift_error(&surface, &rim, interval, pc);
        assert!(e <= 2e-4, "{interval:?}: {e}");
    }
}

/// A curve weaving across a thread flank, smooth through a knot of full multiplicity (one smooth
/// curve with the knot inserted three times, as SMLib's intersection curves come), has no
/// closed-form pcurve on the flank: it is refused, never fitted (SMLib gives its own trims).
#[test]
fn a_curve_weaving_across_a_thread_flank_is_refused() {
    let (surface, _) = thread_flank();
    let s = SurfaceEval::new(&surface).unwrap();
    // Along the flank as it winds, a third of a turn (u = t), weaving a little across it.
    let [a, b] = [0.40625, 0.4234375];
    let ts: Vec<f64> = (0..=12).map(|i| a + (b - a) * i as f64 / 12.).collect();
    let v = |t: f64| 0.5 + 0.02 * (3. * TAU * (t - a) / (b - a)).sin();
    let on: Vec<[f64; 3]> = ts
        .iter()
        .map(|&t| s.point([t, v(t)]).unwrap())
        .map(|p| [p.x, p.y, p.z])
        .collect();
    let (knots, ctrl) = interpolate(&ts, &on, 3);
    let k = 0.4167;
    let (mut knots, mut ctrl) = (
        knots,
        ctrl.iter()
            .map(|p| [p[0], p[1], p[2], 1.])
            .collect::<Vec<_>>(),
    );
    for _ in 0..3 {
        (knots, ctrl) = insert_knot(3, &knots, &ctrl, k).unwrap();
    }
    let weave = Curve::Spline {
        knots,
        points: ctrl
            .iter()
            .map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]])
            .collect(),
    };
    // A curve weaving across the flank has no closed-form pcurve there: refused, never fitted.
    assert!(
        matches!(pcurves(&surface, &one(&weave, [a, b]), 1e-3), Err(Error::Failed(m)) if m.contains("no exact pcurve"))
    );
}

/// A short cubic edge on a cylinder's NURBS form, its angle not moving at a steady pace (θ
/// quadratic in its parameter), ending on the form's seam. Its end there projects to either side
/// of the seam; on the far side the segment between its ends ran the wrong way round, and the one
/// through its middle at its middle's pace was taken, within half a tolerance of 0.02 of the edge
/// but with its ends 5 um round the axis from the edge's: the loop's pcurves did not meet at the
/// vertices once healing set the edge tolerances to the edges' own gaps (`SmTrimmingTools` test
/// 0). The ends are taken on the middle's sheet, so the segment between them is the one taken,
/// ending where the edge ends, as off the seam.
#[test]
fn a_spline_edge_ending_on_a_closed_forms_seam_gets_the_segment_between_its_ends() {
    let r = 20.;
    let (surface, _) = form::surface(
        &Surface::Cylinder {
            frame: about_z([0.; 3], false),
            radius: r,
        },
        [[0., TAU], [0., 1.]],
    )
    .unwrap();
    let s = SurfaceEval::new(&surface).unwrap();
    for end in [TAU, 3.0] {
        // The cubic Bézier with the ends and end tangents of the curve at angle θ(t) and height
        // 0.35·t, 0.05 rad round the axis (5e-4 rad of it from the quadratic term). The segment
        // between its ends and the one through its middle each come within 5.0e-3 of it.
        let start = end - 0.05;
        let theta = |t: f64| start + 0.0495 * t + 0.0005 * t * t;
        let dtheta = |t: f64| 0.0495 + 0.001 * t;
        let at = |t: f64| [r * theta(t).cos(), r * theta(t).sin(), 0.35 * t];
        let tangent = |t: f64| {
            let (a, w) = (theta(t), dtheta(t));
            [-r * w * a.sin(), r * w * a.cos(), 0.35]
        };
        let (p0, p3, d0, d3) = (at(0.), at(1.), tangent(0.), tangent(1.));
        let edge = Curve::Spline {
            knots: vec![0., 0., 0., 0., 1., 1., 1., 1.],
            points: vec![
                p0,
                [0, 1, 2].map(|k| p0[k] + d0[k] / 3.),
                [0, 1, 2].map(|k| p3[k] - d3[k] / 3.),
                p3,
            ],
        };
        let pc = &pcurves(&surface, &one(&edge, [0., 1.]), 0.02).unwrap()[0][0];
        for (t, p) in [(0., p0), (1., p3)] {
            let lifted = s.point(eval(pc, t).unwrap()).unwrap();
            assert!(
                lifted.dist(v3(p)) < 1e-9,
                "ending at {end}: the pcurve at {t} lifts to {lifted:?}, the edge is at {p:?}"
            );
        }
        assert!(lift_error(&surface, &edge, [0., 1.], pc) <= 0.01);
    }
}

/// The B-spline of degree `p` through `points` at `params` (knots by averaging, NURBS Book §9.2.1).
fn interpolate(params: &[f64], points: &[[f64; 3]], p: usize) -> (Vec<f64>, Vec<[f64; 3]>) {
    let m = points.len();
    let mut knots = vec![params[0]; p + 1];
    for j in 1..m - p {
        knots.push(params[j..j + p].iter().sum::<f64>() / p as f64);
    }
    knots.extend(std::iter::repeat_n(params[m - 1], p + 1));
    let mut a = vec![vec![0.0f64; m]; m];
    for (row, &t) in a.iter_mut().zip(params) {
        let span = find_span(&knots, p, m, t).unwrap();
        for (j, n) in basis(&knots, p, span, t).unwrap().into_iter().enumerate() {
            row[span - p + j] = n;
        }
    }
    let mut q = points.to_vec();
    for col in 0..m {
        let piv = (col..m)
            .max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))
            .unwrap();
        a.swap(col, piv);
        q.swap(col, piv);
        for r in 0..m {
            if r != col {
                let f = a[r][col] / a[col][col];
                for c in 0..m {
                    a[r][c] -= f * a[col][c];
                }
                for d in 0..3 {
                    q[r][d] -= f * q[col][d];
                }
            }
        }
    }
    (knots, (0..m).map(|i| q[i].map(|x| x / a[i][i])).collect())
}

/// Hostile records give errors, never a panic.
#[test]
fn hostile_records_give_errors() {
    let plane = Surface::Plane {
        origin: [0.; 3],
        u_axis: [1., 0., 0.],
        v_axis: [0., 1., 0.],
    };
    let line = Curve::Line {
        start: [0.; 3],
        end: [1., 0., 0.],
    };
    for tol in [0., -1., f64::NAN, f64::INFINITY, 1e9] {
        assert!(
            matches!(
                pcurves(&plane, &one(&line, [0., 1.]), tol),
                Err(Error::Invalid(_))
            ),
            "{tol}"
        );
    }
    // An empty, reversed or non-finite interval is refused, given pcurve or not; one past the
    // curve's natural range is the record's to refuse (solidcore-ops' pcurve), an error here.
    for interval in [[1., 1.], [2., 1.], [f64::NAN, 1.], [0., f64::INFINITY]] {
        assert!(
            matches!(
                pcurves(&plane, &one(&line, interval), 1e-6),
                Err(Error::Invalid(_))
            ),
            "{interval:?}"
        );
        let given = Curve2::Spline {
            knots: vec![0., 0., 1., 1.],
            points: vec![[0., 0.], [1., 0.]],
        };
        let uses = vec![vec![EdgeUse {
            edge: 0,
            curve: &line,
            interval,
            forward: true,
            tolerance: 0.0,
            given: Some(&given),
        }]];
        assert!(
            matches!(
                exact_pcurves(&plane, &uses, 1e-6, None),
                Err(Error::Invalid(_))
            ),
            "given, {interval:?}"
        );
    }
    let _ = pcurves(&plane, &one(&line, [0., 1e13]), 1e-6);
    let nan = Curve::Line {
        start: [f64::NAN, 0., 0.],
        end: [1., 0., 0.],
    };
    let _ = pcurves(&plane, &one(&nan, [0., 1.]), 1e-6);
    let skew = Surface::Cylinder {
        frame: frame([0.; 3], [1., 0., 0.], [0.5, 1., 0.]),
        radius: 1.,
    };
    assert!(matches!(
        pcurves(&skew, &one(&line, [0., 1.]), 1e-6),
        Err(Error::Invalid(_))
    ));
    let flat = Surface::Plane {
        origin: [0.; 3],
        u_axis: [1., 0., 0.],
        v_axis: [2., 0., 0.],
    };
    assert!(matches!(
        pcurves(&flat, &one(&line, [0., 1.]), 1e-6),
        Err(Error::Invalid(_))
    ));
    let collapsed = Curve::Circle {
        frame: about_z([0.; 3], false),
        radius: 0.,
    };
    let _ = pcurves(&plane, &one(&collapsed, [0., 1.]), 1e-6);
    // Too many uses, refused before any work.
    let many = vec![vec![
        EdgeUse {
            edge: 0,
            curve: &line,
            interval: [0., 1.],
            forward: true,
            tolerance: 0.0,
            given: None
        };
        MAX_USES + 1
    ]];
    assert!(matches!(
        pcurves(&plane, &many, 1e-6),
        Err(Error::TooLarge(_))
    ));
    // A polyline of many points across a curved surface: it has no exact pcurve there, and is
    // refused as such (nothing is fitted).
    let cylinder = Surface::Cylinder {
        frame: about_z([0.; 3], false),
        radius: 1.,
    };
    let n = 5000;
    let zigzag = Curve::Polyline {
        points: (0..n)
            .map(|i| {
                [
                    (i as f64 * 0.01).cos(),
                    (i as f64 * 0.01).sin(),
                    (i % 2) as f64,
                ]
            })
            .collect(),
    };
    assert!(
        matches!(pcurves(&cylinder, &one(&zigzag, [0., (n - 1) as f64]), 1e-6), Err(Error::Failed(m)) if m.contains("no exact pcurve"))
    );
    // A degree the evaluators do not take.
    let high = Curve::Spline {
        knots: [vec![0.; 10], vec![1.; 10]].concat(),
        points: (0..10).map(|i| [i as f64, 0., 0.]).collect(),
    };
    assert!(matches!(
        pcurves(&cylinder, &one(&high, [0., 1.]), 1e-6),
        Err(Error::Failed(_))
    ));
    // Off the surface, and a curve on two tangent surfaces (no single point).
    assert!(matches!(
        pcurves(&cylinder, &one(&line, [0., 1.]), 1e-6),
        Err(Error::Failed(_))
    ));
    let touching = Curve::SurfaceIntersection {
        surfaces: [
            Box::new(cylinder.clone()),
            Box::new(Surface::Plane {
                origin: [1., 0., 0.],
                u_axis: [0., 1., 0.],
                v_axis: [0., 0., 1.],
            }),
        ],
        leader: Box::new(Curve::Polyline {
            points: vec![[1., 0., 0.], [1., 0., 1.]],
        }),
        tolerance: 1e-3,
    };
    assert!(pcurves(&cylinder, &one(&touching, [0., 1.]), 1e-6).is_err());
    // Extreme but valid: a torus as wide as the schema allows and thin as it allows.
    let torus = Surface::Torus {
        frame: about_z([0.; 3], false),
        major_radius: 1e12,
        minor_radius: 1e-12,
        tube_start: 0.0,
    };
    let far = Curve::Circle {
        frame: about_z([0.; 3], false),
        radius: 1e12,
    };
    let _ = pcurves(&torus, &one(&far, [0., 1.]), 1e-6);
}

/// An edge that lies up to 7.5e-5 mm off its plane (another kernel's approximation) gets no
/// pcurve at the face's 1e-5 mm, and one within its own record tolerance of 2e-4 mm when the
/// use carries it.
#[test]
fn an_edge_is_fitted_to_its_own_tolerance() {
    let plane = Surface::PlaneThrough {
        origin: [1., 2., 3.],
        u_point: [3., 2., 3.],
        v_point: [1., 2., 7.],
    };
    let off = Curve::Spline {
        knots: vec![0., 0., 0., 0., 1., 1., 1., 1.],
        points: vec![
            [1., 2., 3.],
            [2., 2.0001, 4.],
            [3., 2.0001, 5.],
            [4., 2., 6.],
        ],
    };
    assert!(pcurves(&plane, &one(&off, [0., 1.]), 1e-5).is_err());
    let own = vec![vec![EdgeUse {
        edge: 0,
        curve: &off,
        interval: [0., 1.],
        forward: true,
        tolerance: 2e-4,
        given: None,
    }]];
    let pc = &pcurves(&plane, &own, 1e-5).unwrap()[0][0];
    assert!(lift_error(&plane, &off, [0., 1.], pc) <= 2e-4);
}

#[test]
fn restarting_a_closed_curve_preserves_both_pieces() {
    // Non-unit domain, unequal seam weights: each side is a rational quadratic
    // Bezier. The cut inside a span requires knot insertion; 5 is a repeated knot.
    let rational = Curve::RationalSpline {
        knots: vec![2., 2., 2., 5., 5., 8., 8., 8.],
        homogeneous: vec![
            [2., 0., 0., 2.],
            [2., 4., 0., 2.],
            [4., 0., 0., 2.],
            [1., -2., 0., 1.],
            [1., 0., 0., 1.],
        ],
    };
    let spline = Curve::Spline {
        knots: vec![2., 2., 2., 5., 5., 8., 8., 8.],
        points: vec![
            [0., 0., 0.],
            [1., 2., 0.],
            [2., 0., 0.],
            [1., -2., 0.],
            [0., 0., 0.],
        ],
    };
    let polyline = Curve::Polyline {
        points: vec![
            [0., 0., 0.],
            [1., 0., 0.],
            [1., 1., 0.],
            [0., 1., 0.],
            [0., 0., 0.],
        ],
    };
    for (curve, a, b, cuts) in [
        (rational, 2., 8., [4.25, 5.]),
        (spline, 2., 8., [4.25, 5.]),
        (polyline, 0., 4., [1.25, 2.]),
    ] {
        let original = CurveEval::new(&curve).unwrap();
        for start in cuts {
            let rotated = restart_curve(&curve, start).unwrap();
            let eval = CurveEval::new(&rotated).unwrap();
            for i in 0..=128 {
                let t = start + (b - a) * f64::from(i) / 128.;
                let old_t = if t >= b { t - (b - a) } else { t };
                assert!(
                    eval.point(t).unwrap().dist(original.point(old_t).unwrap()) < 1e-12,
                    "{start} {t}"
                );
            }
            assert!(restart_curve(&curve, a).is_err());
            assert!(restart_curve(&curve, b).is_err());
            assert!(restart_curve(&curve, f64::NAN).is_err());
        }
    }
    // Approximate closure must not move either endpoint or its neighbouring span.
    let near_closed = Curve::Polyline {
        points: vec![[0., 0., 0.], [1., 1., 0.], [0., 1e-8, 0.]],
    };
    assert!(restart_curve(&near_closed, 1.).is_err());
}

/// A reader-style rational spline: the two seam coefficients are formed as p*w.
fn weighted_decimal_seam(endpoint: [f64; 3], weight: f64) -> Curve {
    let [x, y, z] = endpoint;
    Curve::RationalSpline {
        knots: vec![2., 2., 2., 5., 5., 8., 8., 8.],
        homogeneous: vec![
            [x * weight, y * weight, z * weight, weight],
            [3., 6., 0., 3.],
            [4., 0., 0., 2.],
            [1., -2., 0., 1.],
            [x, y, z, 1.],
        ],
    }
}

#[test]
fn restarting_a_decimal_seam_accepts_only_endpoint_rounding() {
    assert_ne!(0.1_f64 * 3. / 3., 0.1);
    for endpoint in [
        [0.1, -0.2, 0.3],
        [0.1, -0.2, 0.0],
        [1e8, 0.1, 1e-8],
        [1e-100, -2e-100, 3e-100],
    ] {
        let curve = weighted_decimal_seam(endpoint, 3.);
        let original = CurveEval::new(&curve).unwrap();
        for start in [4.25, 5.] {
            let rotated = restart_curve(&curve, start).unwrap();
            let eval = CurveEval::new(&rotated).unwrap();
            for i in 0..=128 {
                let t = start + 6. * f64::from(i) / 128.;
                let old_t = if t >= 8. { t - 6. } else { t };
                let expected = original.point(old_t).unwrap();
                assert!(eval.point(t).unwrap().dist(expected) <= 1e-12 * (1. + expected.len()));
            }
        }
        let Curve::RationalSpline {
            mut homogeneous,
            knots,
        } = curve
        else {
            panic!()
        };
        // A small y gap must not be hidden by a large x coordinate or an absolute floor.
        homogeneous.last_mut().unwrap()[1] += endpoint[1].abs() * f64::EPSILON * 64.;
        assert!(restart_curve(&Curve::RationalSpline { knots, homogeneous }, 4.25).is_err());
    }
    for bad in [f64::INFINITY, f64::NAN, f64::MAX] {
        let curve = Curve::RationalSpline {
            knots: vec![0., 0., 1., 2., 2.],
            homogeneous: vec![[bad, 0., 0., 0.5], [1., 1., 0., 1.], [bad, 0., 0., 0.5]],
        };
        assert!(restart_curve(&curve, 0.5).is_err());
    }
}

#[test]
fn restarting_weighted_seams_encloses_random_coordinate_rounding() {
    // Deterministic random mantissas of both signs, all bounded away from zero.
    let mut state = 0x6a09_e667_f3bc_c909_u64;
    let mut coordinate = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let magnitude = f64::from_bits(0x3ff0_0000_0000_0000 | (state >> 12)) - 0.75;
        if state & 1 == 0 {
            magnitude
        } else {
            -magnitude
        }
    };
    let mut unequal_quotients = [0; 2];
    for scale in [1e-100, 1., 1e100] {
        for _ in 0..64 {
            let endpoint = [
                coordinate() * scale,
                coordinate() * scale,
                coordinate() * scale,
            ];
            // The exact STEP quarter-circle seam weight is 0.7071067811865476.
            for (which, weight) in [std::f64::consts::FRAC_1_SQRT_2, 3.]
                .into_iter()
                .enumerate()
            {
                unequal_quotients[which] +=
                    usize::from(endpoint.iter().any(|x| x * weight / weight != *x));
                let curve = weighted_decimal_seam(endpoint, weight);
                let original = CurveEval::new(&curve).unwrap();
                for start in [4.25, 5.] {
                    let restarted = restart_curve(&curve, start).unwrap();
                    let eval = CurveEval::new(&restarted).unwrap();
                    for i in 0..=16 {
                        let t = start + 6. * f64::from(i) / 16.;
                        let old_t = if t >= 8. { t - 6. } else { t };
                        let expected = original.point(old_t).unwrap();
                        assert!(
                            eval.point(t).unwrap().dist(expected)
                                <= 1e-12 * expected.len().max(scale),
                            "{endpoint:?}, weight {weight}, t {t}"
                        );
                    }
                }
                // Each axis is checked on its own, only a few ulps from closure.
                for axis in 0..3 {
                    let Curve::RationalSpline {
                        knots,
                        mut homogeneous,
                    } = curve.clone()
                    else {
                        panic!()
                    };
                    homogeneous.last_mut().unwrap()[axis] +=
                        endpoint[axis].abs() * f64::EPSILON * 64.;
                    assert!(
                        restart_curve(&Curve::RationalSpline { knots, homogeneous }, 4.25).is_err(),
                        "{endpoint:?}, weight {weight}, axis {axis}"
                    );
                }
            }
        }
    }
    assert!(
        unequal_quotients.iter().all(|count| *count > 0),
        "exercise unequal divisions of matching source endpoints"
    );
}

#[test]
fn restarting_a_curve_returns_typed_errors_with_its_operation() {
    use crate::SolidError;
    let operation = "eval::pcurve::restart_curve";
    let unsupported = Curve::Line {
        start: [0.; 3],
        end: [1.; 3],
    };
    let error = restart_curve(&unsupported, 0.5).unwrap_err();
    assert_eq!(error.operation(), operation);
    assert!(matches!(
        error,
        SolidError::Unsupported {
            reason: "only splines and polylines can be restarted",
            ..
        }
    ));

    let closed = Curve::Polyline {
        points: vec![[0.; 3], [1.; 3], [0.; 3]],
    };
    let error = restart_curve(&closed, 0.).unwrap_err();
    assert_eq!(error.operation(), operation);
    assert!(matches!(
        error,
        SolidError::InvalidInput {
            reason: "spline restart outside the open domain",
            ..
        }
    ));

    let malformed = Curve::Spline {
        knots: vec![0., 1.],
        points: vec![[0.; 3], [1.; 3]],
    };
    let error = restart_curve(&malformed, 0.5).unwrap_err();
    assert_eq!(error.operation(), operation);
    assert!(matches!(
        error,
        SolidError::InvalidInput {
            reason: "spline degree",
            ..
        }
    ));

    let open = Curve::Polyline {
        points: vec![[0.; 3], [1.; 3], [2.; 3]],
    };
    let error = restart_curve(&open, 0.5).unwrap_err();
    assert_eq!(error.operation(), operation);
    assert!(matches!(
        error,
        SolidError::InvalidInput {
            reason: "spline seam endpoints do not coincide",
            ..
        }
    ));
}

/// The pieces a face's flux is integrated over along a pcurve: the knot spans inside its
/// interval, each in one piece for a straight pcurve and in two for a curved one, and 64 even
/// pieces over the interval where that would be more.
#[test]
fn flux_pieces_are_one_to_a_straight_span_two_to_a_curved_one_and_at_most_64() {
    // A pcurve of `degree` on clamped knots over [0, spans], a span to each unit, with
    // `degree + spans` points zigzagging (so each span of a degree-1 one is straight).
    let pcurve = |spans: usize, degree: usize| {
        let mut knots = vec![0.; degree];
        knots.extend((0..=spans).map(|i| i as f64));
        knots.extend(std::iter::repeat_n(spans as f64, degree));
        Curve2::Spline {
            knots,
            points: (0..degree + spans)
                .map(|i| [i as f64, (i % 2) as f64])
                .collect(),
        }
    };
    // Each unit span [k, k + 1] of [0, n] in halves, [k, k + ½] and [k + ½, k + 1].
    let halves = |n: usize| -> Vec<[f64; 2]> {
        (0..2 * n)
            .map(|k| [k as f64 / 2., (k + 1) as f64 / 2.])
            .collect()
    };
    // Degree 1 on [0, 0, 1, 2, 2] (3 points: 5 − 3 − 1 = 1): spans [0, 1] and [1, 2], one piece
    // each.
    assert_eq!(flux_pieces(&pcurve(2, 1)), Some(vec![[0., 1.], [1., 2.]]));
    // The same polygon (0, 0), (1, 1), (2, 0) weighted 1, 2, 1 is rational, so curved: two
    // pieces to each span, 4 in all.
    let rational = Curve2::RationalSpline {
        knots: vec![0., 0., 1., 2., 2.],
        homogeneous: vec![[0., 0., 1.], [2., 2., 2.], [2., 0., 1.]],
    };
    assert_eq!(flux_pieces(&rational), Some(halves(2)));
    // A cubic on [0, 0, 0, 0, 1, 2, 3, 3, 3, 3] (6 points): 3 spans, 2 pieces each, 6 in all.
    assert_eq!(flux_pieces(&pcurve(3, 3)), Some(halves(3)));
    // Only the spans inside the interval: a periodic quadratic on [−2, −1, 0, 1, 2, 3, 4] (4
    // points) runs over [knots[2], knots[4]] = [0, 2], 2 of its 6 spans, so 4 pieces.
    let periodic = Curve2::Spline {
        knots: vec![-2., -1., 0., 1., 2., 3., 4.],
        points: vec![[0., 0.], [1., 1.], [2., 0.], [3., 1.]],
    };
    assert_eq!(flux_pieces(&periodic), Some(halves(2)));
    // 32 cubic spans make 32 · 2 = 64 pieces, the most: still each span in halves.
    assert_eq!(flux_pieces(&pcurve(32, 3)), Some(halves(32)));
    // 40 make 80, past 64: 64 even pieces over [0, 40] instead, each 40 / 64 = 0.625 long (the
    // k-th starts at 40 k / 64 = 0.625 k, exactly).
    let even: Vec<[f64; 2]> = (0..64)
        .map(|k| [0.625 * k as f64, 0.625 * (k + 1) as f64])
        .collect();
    assert_eq!(flux_pieces(&pcurve(40, 3)), Some(even));
    // Two knots, fewer than two points of any degree take (2 + 0 + 1 = 3): no pieces.
    let short = Curve2::Spline {
        knots: vec![0., 1.],
        points: vec![[0., 0.], [1., 0.]],
    };
    assert_eq!(flux_pieces(&short), None);
}

/// `eval_d1` answers where `eval` does, and only with numbers. A quarter of the unit circle as a
/// rational quadratic over [0, 1] (weights 1, √½, 1): at 0 the point (1, 0) and the derivative
/// (0, √2), as A′(0) = 2 (P₁ − P₀) = (2(√½ − 1), 2√½, 2(√½ − 1)) and C′ = (A′ − w′ C) / w =
/// (0, 2√½); at 1 the point (0, 1) and the derivative (−√2, 0); at ½ the point `eval` gives.
/// Past [0, 1], or at NaN, neither answers. A line on the knots 0, 1, 2, 3 (degree 1, its
/// interval [1, 2]) has the point (½, 0) and the derivative (1, 0) at 1.5, and none at 2.5,
/// where its last basis function alone still reads ½ (the point (1, 0), at weight ½, came back).
/// A control point at infinity leaves no point between the knots it reaches: (∞, 0) came back.
#[test]
fn a_pcurves_derivative_is_answered_on_its_interval_and_only_in_numbers() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let s = std::f64::consts::SQRT_2;
    let quarter = Curve2::RationalSpline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        homogeneous: vec![[1., 0., 1.], [w, w, w], [0., 1., 1.]],
    };
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-12;
    for (t, point, way) in [(0., [1., 0.], [0., s]), (1., [0., 1.], [-s, 0.])] {
        let (p, d) = eval_d1(&quarter, t).unwrap();
        assert!(near(p, point) && near(d, way), "{t}: {p:?} {d:?}");
    }
    let (p, _) = eval_d1(&quarter, 0.5).unwrap();
    assert!(near(p, eval(&quarter, 0.5).unwrap()), "{p:?}");
    for t in [-0.1, 1.1, f64::NAN] {
        assert_eq!(eval(&quarter, t), None, "{t}");
        assert_eq!(eval_d1(&quarter, t), None, "{t}");
    }
    let line = Curve2::Spline {
        knots: vec![0., 1., 2., 3.],
        points: vec![[0., 0.], [1., 0.]],
    };
    let (p, d) = eval_d1(&line, 1.5).unwrap();
    assert!(near(p, [0.5, 0.]) && near(d, [1., 0.]), "{p:?} {d:?}");
    assert_eq!(eval_d1(&line, 2.5), None);
    let far = Curve2::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![[0., 0.], [f64::INFINITY, 0.]],
    };
    assert_eq!(eval_d1(&far, 0.5), None);
}

/// A degree-1 patch over [0, 1]² on the corners (0, 0, 0), (0, 10, 0), (10, 0, 0) and (10, 10,
/// `top`): not a parallelogram for a `top` other than 0, so not a plane ([`flat_patch`]).
fn twisted(top: f64) -> Surface {
    Surface::Spline {
        u_knots: vec![0., 0., 1., 1.],
        v_knots: vec![0., 0., 1., 1.],
        points: vec![
            vec![[0., 0., 0.], [0., 10., 0.]],
            vec![[10., 0., 0.], [10., 10., top]],
        ],
    }
}

/// The four boundary segments of [`twisted`], from the (u, v) corner `from` to `to`, each as the
/// edge that runs along it (its two control points `off` mm above the boundary).
fn boundary_edge(top: f64, from: [f64; 2], to: [f64; 2], off: f64) -> Curve {
    let at = |[u, v]: [f64; 2]| {
        let z = if u == 1. && v == 1. { top } else { 0. };
        [10. * u, 10. * v, z + off]
    };
    Curve::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![at(from), at(to)],
    }
}

const SIDES: [([f64; 2], [f64; 2]); 4] = [
    ([0., 0.], [1., 0.]),
    ([0., 1.], [1., 1.]),
    ([0., 0.], [0., 1.]),
    ([1., 0.], [1., 1.]),
];

/// A whole boundary curve of a patch gets the straight (u, v) line along it by a proved bound,
/// whatever its offset within the cap, running either way; and the bound is the gap it has.
#[test]
fn an_edge_along_a_patch_boundary_gets_its_straight_line_with_the_proved_gap() {
    let surface = twisted(2.);
    for off in [0.0, 2e-5, 7.5e-5] {
        for (from, to) in SIDES {
            for reversed in [false, true] {
                let (a, b) = if reversed { (to, from) } else { (from, to) };
                let edge = boundary_edge(2., a, b, off);
                let found = boundary_iso(&surface, &edge, [0., 1.], 1e-3)
                    .unwrap()
                    .unwrap_or_else(|| panic!("{off} {from:?} {to:?} {reversed}"));
                assert!(
                    found.gap >= off && found.gap <= off + 1e-9,
                    "{off}: proved {}",
                    found.gap
                );
                assert_eq!(segment_points(&found.pcurve), [a, b]);
                assert_eq!(found.pcurve.interval(), Some([0., 1.]));
                // The lift is the boundary: as far from the edge as the proof says.
                let lifted = lift_error(&surface, &edge, [0., 1.], &found.pcurve);
                assert!(lifted <= found.gap, "{off}: {lifted} against {}", found.gap);
            }
        }
    }
}

/// A line record and an interval of its own parameters: the same proof.
#[test]
fn a_line_edge_along_a_patch_boundary_is_proved_too() {
    let surface = twisted(2.);
    let edge = Curve::Line {
        start: [0., 10., 1e-5],
        end: [10., 10., 2. + 1e-5],
    };
    let found = boundary_iso(&surface, &edge, [0., 1.], 1e-3)
        .unwrap()
        .unwrap();
    assert_eq!(segment_points(&found.pcurve), [[0., 1.], [1., 1.]]);
    assert!((found.gap - 1e-5).abs() < 1e-9, "{}", found.gap);
    // Not the line's whole extent: not the whole boundary.
    assert_eq!(
        boundary_iso(&surface, &edge, [0., 0.5], 1e-3).unwrap(),
        None
    );
}

/// The importer raises the edge's tolerance to cover the proved gap and then asks for pcurves:
/// the closed form is taken at the raised tolerance, and not at the file's, where the edge is
/// farther from its boundary than half of it.
#[test]
fn a_patch_boundary_pcurve_is_exact_where_the_tolerance_covers_its_proved_gap() {
    let surface = twisted(2.);
    for off in [0.0, 2e-5, 7.5e-5] {
        let edge = boundary_edge(2., [0., 1.], [1., 1.], off);
        let gap = boundary_iso(&surface, &edge, [0., 1.], 1e-3)
            .unwrap()
            .unwrap()
            .gap;
        let raised = SURFACE_GAP_MARGIN * gap.max(1e-6);
        let made = exact_pcurves(&surface, &one(&edge, [0., 1.]), raised, None).unwrap();
        let pc = made[0][0].as_ref().expect("a closed form");
        assert_eq!(segment_points(pc), [[0., 1.], [1., 1.]]);
        assert!(lift_error(&surface, &edge, [0., 1.], pc) <= 0.5 * raised);
        let all = pcurves(&surface, &one(&edge, [0., 1.]), raised).unwrap();
        assert_eq!(all[0][0], *pc);
    }
    // At a tolerance the gap is past, the line is not taken (nothing is fitted).
    let edge = boundary_edge(2., [0., 1.], [1., 1.], 7.5e-5);
    let made = exact_pcurves(&surface, &one(&edge, [0., 1.]), 1e-6, None).unwrap();
    assert_eq!(made[0][0], None);
}

/// Anything the proof does not cover is `None`, so the caller carries on as it did.
#[test]
fn what_is_not_a_whole_boundary_curve_of_a_polynomial_patch_has_no_proof() {
    let surface = twisted(2.);
    let (cap, whole) = (1e-3, [0., 1.]);
    let none = |s: &Surface, c: &Curve, why: &str| {
        assert_eq!(boundary_iso(s, c, whole, cap).unwrap(), None, "{why}");
    };
    // On the interior: the iso-curve at u = 0.5 is not a boundary.
    let middle = Curve::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![[5., 0., 0.], [5., 10., 1.]],
    };
    none(&surface, &middle, "the interior");
    // Off the boundary by more than the cap.
    none(
        &surface,
        &boundary_edge(2., [0., 0.], [1., 0.], 0.5),
        "past the cap",
    );
    assert!(boundary_iso(
        &surface,
        &boundary_edge(2., [0., 0.], [1., 0.], 0.5),
        whole,
        1.
    )
    .unwrap()
    .is_some());
    // The right ends at the wrong pace: the quadratic whose middle control point is a quarter
    // of the way is the boundary's line, a eighth of its length (1.25 mm) off at its middle.
    let slow = Curve::Spline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        points: vec![[0., 0., 0.], [2.5, 0., 0.], [10., 0., 0.]],
    };
    none(&surface, &slow, "a parameter that is not linear");
    // The same line at a parameter that is: a line's own elevation is the boundary.
    let elevated = Curve::Spline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        points: vec![[0., 0., 0.], [5., 0., 0.], [10., 0., 0.]],
    };
    assert!(boundary_iso(&surface, &elevated, whole, cap)
        .unwrap()
        .is_some());
    // A rational patch, with its weights not all one.
    let rational = Surface::RationalSpline {
        u_knots: vec![0., 0., 1., 1.],
        v_knots: vec![0., 0., 1., 1.],
        homogeneous: vec![
            vec![[0., 0., 0., 1.], [0., 10., 0., 1.]],
            vec![[10., 0., 0., 1.], [20., 20., 4., 2.]],
        ],
    };
    // Its side with weights (1, 2) is a rational curve, no polynomial edge's; its side with
    // weights (1, 1) is a polynomial one, and is proved.
    none(
        &rational,
        &boundary_edge(2., [1., 0.], [1., 1.], 0.),
        "a rational boundary against a polynomial edge",
    );
    assert!(boundary_iso(
        &rational,
        &boundary_edge(2., [0., 0.], [1., 0.], 0.),
        whole,
        cap
    )
    .unwrap()
    .is_some());
    // A rational edge on a polynomial patch, a polyline: no boundary of it.
    for edge in [
        Curve::RationalSpline {
            knots: vec![0., 0., 1., 1.],
            homogeneous: vec![[0., 0., 0., 1.], [10., 0., 0., 1.]],
        },
        Curve::Polyline {
            points: vec![[0., 0., 0.], [10., 0., 0.]],
        },
    ] {
        none(&surface, &edge, "not a polynomial B-spline");
    }
    // Not a spline patch.
    let plane = Surface::Plane {
        origin: [0.; 3],
        u_axis: [10., 0., 0.],
        v_axis: [0., 10., 0.],
    };
    none(
        &plane,
        &boundary_edge(2., [0., 0.], [1., 0.], 0.),
        "a plane",
    );
    // A patch closed in u: its ends are one curve, and the loop's pcurves run round it.
    let square = [[0., 0.], [10., 0.], [10., 10.], [0., 10.], [0., 0.]];
    let closed = Surface::Spline {
        u_knots: vec![0., 0., 1., 2., 3., 4., 4.],
        v_knots: vec![0., 0., 1., 1.],
        points: square
            .iter()
            .map(|[x, y]| vec![[*x, *y, 0.], [*x, *y, 5.]])
            .collect(),
    };
    let rim = Curve::Spline {
        knots: vec![0., 0., 1., 2., 3., 4., 4.],
        points: square.iter().map(|[x, y]| [*x, *y, 0.]).collect(),
    };
    assert_eq!(
        boundary_iso(&closed, &rim, [0., 4.], cap).unwrap(),
        None,
        "closed in u"
    );
    // The same polygon left open is a boundary.
    let open = Surface::Spline {
        u_knots: vec![0., 0., 1., 2., 3., 3.],
        v_knots: vec![0., 0., 1., 1.],
        points: square[..4]
            .iter()
            .map(|[x, y]| vec![[*x, *y, 0.], [*x, *y, 5.]])
            .collect(),
    };
    let rim = Curve::Spline {
        knots: vec![0., 0., 1., 2., 3., 3.],
        points: square[..4].iter().map(|[x, y]| [*x, *y, 0.]).collect(),
    };
    let found = boundary_iso(&open, &rim, [0., 3.], cap).unwrap().unwrap();
    assert_eq!(found.pcurve.interval(), Some([0., 3.]));
}

/// Hostile numbers are errors or no proof, never a panic.
#[test]
fn boundary_proofs_of_hostile_records_give_errors_or_none() {
    let surface = twisted(2.);
    let edge = boundary_edge(2., [0., 0.], [1., 0.], 0.);
    for bad in [
        [f64::NAN, 1.],
        [0., f64::NAN],
        [0., f64::INFINITY],
        [1., 0.],
        [0.5, 0.5],
    ] {
        assert!(
            matches!(
                boundary_iso(&surface, &edge, bad, 1e-3),
                Err(Error::Invalid(_))
            ),
            "{bad:?}"
        );
    }
    for cap in [f64::NAN, f64::INFINITY, 0., -1.] {
        assert!(
            matches!(
                boundary_iso(&surface, &edge, [0., 1.], cap),
                Err(Error::Invalid(_))
            ),
            "{cap}"
        );
    }
    // Numbers not finite in the records.
    let nan_edge = Curve::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![[0., 0., 0.], [f64::NAN, 0., 0.]],
    };
    assert!(boundary_iso(&surface, &nan_edge, [0., 1.], 1e-3).is_err());
    let nan_patch = Surface::Spline {
        u_knots: vec![0., 0., 1., 1.],
        v_knots: vec![0., 0., 1., 1.],
        points: vec![
            vec![[0., 0., 0.], [0., 10., f64::INFINITY]],
            vec![[10., 0., 0.], [10., 10., 0.]],
        ],
    };
    assert!(boundary_iso(&nan_patch, &edge, [0., 1.], 1e-3).is_err());
    // Nets of nothing, of one point, of knots that do not fit, and of every size of number.
    for points in [vec![], vec![vec![]], vec![vec![[0.; 3]]]] {
        let patch = Surface::Spline {
            u_knots: vec![0., 0., 1., 1.],
            v_knots: vec![0., 0., 1., 1.],
            points,
        };
        let r = boundary_iso(&patch, &edge, [0., 1.], 1e-3);
        assert!(matches!(r, Err(_) | Ok(None)), "{r:?}");
    }
    for scale in [1e-300, 1e-150, 1e150, 1e300] {
        let big = |p: [f64; 3]| p.map(|x| x * scale);
        let patch = Surface::Spline {
            u_knots: vec![0., 0., 1., 1.],
            v_knots: vec![0., 0., 1., 1.],
            points: vec![
                vec![big([0., 0., 0.]), big([0., 10., 0.])],
                vec![big([10., 0., 0.]), big([10., 10., 2.])],
            ],
        };
        let curve = Curve::Spline {
            knots: vec![0., 0., 1., 1.],
            points: vec![big([0., 0., 0.]), big([10., 0., 0.])],
        };
        let r = boundary_iso(&patch, &curve, [0., 1.], 1e-3);
        assert!(matches!(r, Err(_) | Ok(None | Some(_))), "{scale}: {r:?}");
        // Where a proof is returned, its bound is a number and positive, at every scale: a leg of
        // coordinates whose squares underflow is not read as no length at all.
        if let Ok(Some(found)) = &r {
            assert!(
                found.gap.is_finite() && found.gap > 0.0,
                "{scale}: {}",
                found.gap
            );
        }
    }
    // An edge with more control points than the proof refines.
    let n = MAX_BOUNDARY_POINTS + 1;
    let long = Curve::Spline {
        knots: std::iter::once(0.)
            .chain((0..n).map(|k| k as f64 / n as f64))
            .chain([1., 1.])
            .collect(),
        points: (0..n)
            .map(|k| [10. * k as f64 / n as f64, 0., 0.])
            .collect(),
    };
    assert_eq!(boundary_iso(&surface, &long, [0., 1.], 1e-3).unwrap(), None);
}

/// A tiny deterministic generator (xorshift) for the tests' perturbations, in [0, 1).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A point of the degree-`p` B-spline of `points` on `knots` by de Boor's algorithm, written here
/// on its own (not the library's evaluator) so a bound is not checked with its own code.
fn de_boor(points: &[[f64; 3]], knots: &[f64], p: usize, t: f64) -> [f64; 3] {
    let n = points.len();
    let mut l = p;
    while l + 1 < n && knots[l + 1] <= t {
        l += 1;
    }
    let mut d: Vec<[f64; 3]> = (0..=p).map(|j| points[l - p + j]).collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = l - p + j;
            let den = knots[i + p + 1 - r] - knots[i];
            let a = if den > 0.0 { (t - knots[i]) / den } else { 0.0 };
            d[j] = std::array::from_fn(|k| (1.0 - a) * d[j - 1][k] + a * d[j][k]);
        }
    }
    d[p]
}

/// The knots `k` with every interior one moved by up to `delta` (the ends kept, the order kept),
/// each by `delta` exactly where `extreme` (a random sign), else by a random amount.
fn shifted(k: &[f64], p: usize, delta: f64, extreme: bool, rng: &mut Lcg) -> Vec<f64> {
    let (first, last) = (k[0], k[k.len() - 1]);
    let mut out = k.to_vec();
    for j in p + 1..k.len() - p - 1 {
        let s = if extreme {
            if rng.next() < 0.5 {
                -1.0
            } else {
                1.0
            }
        } else {
            2.0 * rng.next() - 1.0
        };
        // A shade under `delta`, so the sum's own rounding stays inside it.
        out[j] = (k[j] + s * delta * (1.0 - 1e-3))
            .max(out[j - 1])
            .min(last)
            .max(first);
    }
    // The running maximum cannot have moved a knot by more than `delta`.
    assert!(out.iter().zip(k).all(|(a, b)| (a - b).abs() <= delta));
    assert!(out.windows(2).all(|w| w[0] <= w[1]));
    out
}

/// The largest distance between the curve of `points` on `a` and on `b` over `a`'s and `b`'s
/// knots, their middles, and three random points of each span.
fn curves_apart(points: &[[f64; 3]], a: &[f64], b: &[f64], p: usize, rng: &mut Lcg) -> f64 {
    let (first, last) = (a[0], a[a.len() - 1]);
    let mut ts = vec![first, last];
    for k in [a, b] {
        for w in k.windows(2) {
            ts.push(w[0]);
            ts.push(0.5 * (w[0] + w[1]));
            ts.extend((0..3).map(|_| w[0] + rng.next() * (w[1] - w[0])));
        }
    }
    ts.iter()
        .filter(|t| **t >= first && **t <= last)
        .map(|&t| {
            let (x, y) = (de_boor(points, a, p, t), de_boor(points, b, p, t));
            ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt()
        })
        .fold(0.0, f64::max)
}

/// The estimate the first proof used, `2 p L δ`, `L` the steepest derivative control point.
fn first_order_estimate(points: &[[f64; 3]], knots: &[f64], p: usize, delta: f64) -> f64 {
    let mut l = 0.0_f64;
    for (i, w) in points.windows(2).enumerate() {
        let gap = knots[i + p + 1] - knots[i + 1];
        let step = ((w[1][0] - w[0][0]).powi(2)
            + (w[1][1] - w[0][1]).powi(2)
            + (w[1][2] - w[0][2]).powi(2))
        .sqrt();
        if gap > 0.0 {
            l = l.max(p as f64 * step / gap);
        } else if step > 0.0 {
            return f64::INFINITY;
        }
    }
    2.0 * p as f64 * l * delta
}

/// Clamped knots of degree `p` over `[0, last]`, the interior ones as `interior` gives them.
fn clamped(p: usize, interior: &[f64], last: f64) -> Vec<f64> {
    let mut k = vec![0.0; p + 1];
    k.extend(interior);
    k.extend(vec![last; p + 1]);
    k
}

/// Control points of a wandering polygon of `n` points about a few tens of millimetres.
fn polygon(n: usize, rng: &mut Lcg) -> Vec<[f64; 3]> {
    (0..n)
        .map(|i| {
            let x = i as f64;
            [
                0.6 * x + 3.0 * rng.next(),
                20.0 * (x / 9.0).sin() + 3.0 * rng.next(),
                5.0 * (x / 3.0).cos() + 6.0 * rng.next(),
            ]
        })
        .collect()
}

/// What moving the knots does is bounded by `knot_shift_bound`, which is a proved bound where the
/// estimate `2 p L δ` it replaces was not: for a cubic of 168 poles on uneven knots, and for one
/// whose knots come in clusters (triple, pairs, and four within a few parts in 10⁷), every knot
/// moved by up to δ, in random amounts and by δ in a random direction, δ from far below the knot
/// gaps to past them; each time the curve's actual deviation is under the bound, with the knots
/// as given as the computed ones and the other way round.
#[test]
fn the_knot_shift_bound_covers_the_curves_a_perturbation_moves() {
    let p = 3;
    let mut rng = Lcg(0x9e37_79b9_7f4a_7c15);
    let poles = 168;
    let points = polygon(poles, &mut rng);
    let cp = |pts: &[[f64; 3]]| -> Vec<CPoint> {
        pts.iter()
            .map(|q| CPoint::new(q[0], q[1], q[2], NOW))
            .collect()
    };
    let cps = cp(&points);
    // Uneven: gaps between 0.2 and 1.8.
    let uneven: Vec<f64> = (0..poles - p - 1)
        .scan(0.0, |x, _| {
            *x += 0.2 + 1.6 * rng.next();
            Some(*x)
        })
        .collect();
    let last = uneven[uneven.len() - 1] + 1.0;
    let even = clamped(p, &uneven, last);
    // Clusters: 41 centres, four knots each, the pattern changing with the centre.
    let mut clustered = Vec::new();
    for c in 0..(poles - p - 1) / 4 {
        let at = 1.0 + c as f64;
        let s = 1e-7 * (1 + c % 3) as f64;
        match c % 4 {
            // A knot of multiplicity 3, one more beside it.
            0 => clustered.extend([at, at, at, at + s]),
            // Four distinct knots within 3 s.
            1 => clustered.extend([at, at + s, at + 2.0 * s, at + 3.0 * s]),
            // Two pairs.
            2 => clustered.extend([at, at, at + s, at + s]),
            _ => clustered.extend([at, at + 0.5 * s, at + 0.5 * s, at + 2.0 * s]),
        }
    }
    assert_eq!(clustered.len(), poles - p - 1);
    let cluster_end = 1.0 + (clustered.len() / 4) as f64;
    let clustered = clamped(p, &clustered, cluster_end);
    assert_eq!(even.len(), poles + p + 1);
    assert_eq!(clustered.len(), poles + p + 1);
    let mut report = Vec::new();
    for (name, knots, deltas) in [
        ("uneven", &even, vec![1e-9, 1e-4, 0.05, 0.2, 0.5]),
        (
            "clustered",
            &clustered,
            vec![1e-9, 1e-8, 1e-7, 3e-7, 1e-6, 1e-4, 0.01],
        ),
    ] {
        for delta in deltas {
            let mut worst = 0.0_f64;
            let mut best_ratio = f64::INFINITY;
            for trial in 0..12 {
                let moved = shifted(knots, p, delta, trial % 2 == 0, &mut rng);
                let actual = curves_apart(&points, knots, &moved, p, &mut rng);
                for (given, computed) in [(knots, &moved), (&moved, knots)] {
                    let bound = knot_shift_bound(&cps, computed, p, delta).unwrap();
                    assert!(
                        actual <= bound + 1e-11,
                        "{name} δ = {delta} trial {trial}: moved {actual}, bound {bound} (given {} knots)",
                        given.len()
                    );
                    best_ratio = best_ratio.min(bound / actual.max(1e-300));
                }
                worst = worst.max(actual);
            }
            let old = first_order_estimate(&points, knots, p, delta);
            let bound = knot_shift_bound(&cps, knots, p, delta).unwrap();
            report.push(format!("{name} δ = {delta:e}: worst moved {worst:.3e}, bound {bound:.3e}, 2pLδ {old:.3e}, tightest bound/moved {best_ratio:.2}"));
        }
    }
    eprintln!("{}", report.join("\n"));
    // Hostile arguments: no bound, never a panic.
    assert_eq!(knot_shift_bound(&cps, &even, 0, 1e-3), None);
    assert_eq!(knot_shift_bound(&cps, &even[1..], p, 1e-3), None);
    assert_eq!(knot_shift_bound(&cps, &even, p, f64::NAN), None);
    assert_eq!(knot_shift_bound(&cps, &even, p, -1.0), None);
    let mut unordered = even.clone();
    unordered.swap(10, 11);
    assert_eq!(knot_shift_bound(&cps, &unordered, p, 1e-3), None);
    assert_eq!(knot_shift_bound(&[], &[0.0; 4], 3, 0.0), None);
}

/// Where knots cluster closer than δ (five of a cubic's interior knots within 4 mm in a span of
/// five, δ from a quarter of their spacing to past it) the curve moves by up to what the proved
/// bound allows and no more; the first-order estimate `2 p L δ` it replaced is only a comparison
/// (it was never proved: no case here breaks it, and the proved bound is the smaller).
#[test]
fn the_knot_shift_bound_holds_where_the_knots_of_a_cubic_cluster() {
    let p = 3;
    let points: Vec<[f64; 3]> = (0..12)
        .map(|i| [i as f64, if i % 2 == 0 { 0.0 } else { 3.0 }, 0.0])
        .collect();
    let cps: Vec<CPoint> = points
        .iter()
        .map(|q| CPoint::new(q[0], q[1], q[2], NOW))
        .collect();
    // Eight interior knots: three even, then five a thousandth apart.
    let interior = [1.0, 2.0, 3.0, 4.0, 4.001, 4.002, 4.003, 4.004];
    let knots = clamped(p, &interior, 5.0);
    let mut rng = Lcg(7);
    for delta in [2.5e-4, 1e-3, 2e-3, 4e-3, 1e-2] {
        let old = first_order_estimate(&points, &knots, p, delta);
        let bound = knot_shift_bound(&cps, &knots, p, delta).unwrap();
        let mut worst = 0.0_f64;
        for k in 0..40 {
            let moved = shifted(&knots, p, delta, k % 2 == 0, &mut rng);
            let actual = curves_apart(&points, &knots, &moved, p, &mut rng);
            assert!(
                actual <= bound + 1e-11,
                "δ = {delta}: {actual} against {bound}"
            );
            worst = worst.max(actual);
        }
        assert!(
            bound <= old,
            "δ = {delta}: the proved bound {bound} is no larger than the estimate {old}"
        );
        eprintln!(
            "cluster δ = {delta}: 2pLδ {old:.4e}, proved {bound:.4e}, worst moved {worst:.4e}"
        );
    }
}

/// A leg of tiny coordinates counts its length: the squares of `5e-170` underflow to 0, and a
/// bound that read the leg as no length at all would sit below the displacement (a degree-1 curve
/// whose middle knot moves by δ is displaced at that knot by `|ΔP₀| δ / (0.5 + δ)`, in closed form:
/// the point is `P₁` on one knot vector and a convex combination on the other).
#[test]
fn the_knot_shift_bound_counts_a_leg_of_tiny_coordinates() {
    let cp = |x: f64, y: f64| CPoint::new(x, y, 0.0, NOW);
    for scale in [1.0, 1e-100, 1e-170] {
        let points = [
            cp(0.0, 0.0),
            cp(3.0 * scale, 4.0 * scale),
            cp(6.0 * scale, 8.0 * scale),
        ];
        let knots = [0.0, 0.0, 0.5, 1.0, 1.0];
        let leg = (3.0 * scale).hypot(4.0 * scale);
        for delta in [1e-12, 1e-6, 1e-3] {
            let displacement = leg * delta / (0.5 + delta);
            let bound = knot_shift_bound(&points, &knots, 1, delta).unwrap();
            assert!(bound.is_finite() && bound > 0.0, "{scale} {delta}: {bound}");
            assert!(
                bound >= displacement,
                "{scale} {delta}: the bound {bound} is under the displacement {displacement}"
            );
        }
    }
}

/// `extract_iso_crv` at the end of a clamped range returns the net's end row and the knots
/// themselves, bit for bit, on all four sides (the u-curves at the least and greatest v, the
/// v-curves at the least and greatest u): the extraction term of a boundary proof is exactly 0.
#[test]
fn the_extracted_boundary_is_a_copy_of_the_net_s_row() {
    let u_knots = vec![0.0, 0.0, 0.0, 0.0, 0.37, 0.81, 1.0, 1.0, 1.0, 1.0];
    let v_knots = vec![0.25, 0.25, 0.25, 0.6, 1.5, 1.5, 1.5];
    let mut rng = Lcg(11);
    let points: Vec<Vec<[f64; 3]>> = (0..6)
        .map(|i| {
            (0..4)
                .map(|j| {
                    [
                        i as f64 + rng.next(),
                        j as f64 * 3.0 + rng.next(),
                        10.0 * rng.next() - 5.0,
                    ]
                })
                .collect()
        })
        .collect();
    let surface = Surface::Spline {
        u_knots: u_knots.clone(),
        v_knots: v_knots.clone(),
        points: points.clone(),
    };
    let SurfaceEval::Eval(EvalSurface::Nurbs(n), _) = SurfaceEval::new(&surface).unwrap() else {
        panic!("a spline")
    };
    let nl = n.nl_surface().expect("an NLib surface");
    let row = |c: &NlCurve| -> Vec<[f64; 3]> { c.cpts().iter().map(|q| [q.x, q.y, q.z]).collect() };
    let u_end = extract_iso_crv(nl, 0.25, Direction::U).unwrap();
    assert_eq!(row(&u_end), points.iter().map(|r| r[0]).collect::<Vec<_>>());
    assert_eq!(u_end.knots(), &u_knots[..]);
    let v_end = extract_iso_crv(nl, 1.5, Direction::U).unwrap();
    assert_eq!(row(&v_end), points.iter().map(|r| r[3]).collect::<Vec<_>>());
    assert_eq!(v_end.knots(), &u_knots[..]);
    let first = extract_iso_crv(nl, 0.0, Direction::V).unwrap();
    assert_eq!(row(&first), points[0]);
    assert_eq!(first.knots(), &v_knots[..]);
    let last = extract_iso_crv(nl, 1.0, Direction::V).unwrap();
    assert_eq!(row(&last), points[5]);
    assert_eq!(last.knots(), &v_knots[..]);
}

/// A cubic patch of 168 poles along u (uneven knots) over a bilinear v, whose four edges are its
/// boundary rows 20 micrometres off, on parameter intervals that are no affine image of the
/// patch's but one whose knots round (so the proof's knot-map term is exercised on a long
/// polygon): each gets its line, with a proved gap that is the offset plus rounding, and the line
/// lifts onto the edge within it.
#[test]
fn a_long_cubic_patch_boundary_is_proved_on_an_interval_of_its_own() {
    let p = 3;
    let mut rng = Lcg(0xdead_beef);
    let poles = 168;
    let row = polygon(poles, &mut rng);
    let uneven: Vec<f64> = (0..poles - p - 1)
        .scan(0.0, |x, _| {
            *x += 0.2 + 1.6 * rng.next();
            Some(*x)
        })
        .collect();
    let end = uneven[uneven.len() - 1] + 1.0;
    let u_knots = clamped(p, &uneven, end);
    let lift = [0.0, 0.0, 10.0];
    let points: Vec<Vec<[f64; 3]>> = row
        .iter()
        .map(|q| vec![*q, [q[0] + lift[0], q[1] + lift[1], q[2] + lift[2]]])
        .collect();
    let surface = Surface::Spline {
        u_knots: u_knots.clone(),
        v_knots: vec![0., 0., 1., 1.],
        points,
    };
    let off = 2e-5;
    for (t0, t1) in [(0.0, 1.0), (1000.0, 1007.5), (-3.0, 4.25)] {
        for reversed in [false, true] {
            // Boundary v = 0, on [t0, t1]: knots a linear map of the patch's, rounded.
            let map = |x: f64| t0 + x / end * (t1 - t0);
            let mut knots: Vec<f64> = u_knots.iter().map(|x| map(*x)).collect();
            knots[0] = t0;
            let n = knots.len();
            knots[n - 1] = t1;
            for k in knots.iter_mut().take(p + 1) {
                *k = t0;
            }
            for k in knots.iter_mut().skip(n - p - 1) {
                *k = t1;
            }
            let mut pts: Vec<[f64; 3]> = row.iter().map(|q| [q[0], q[1], q[2] + off]).collect();
            if reversed {
                pts.reverse();
                knots = knots.iter().rev().map(|k| t0 + t1 - k).collect();
                let n = knots.len();
                for k in knots.iter_mut().take(p + 1) {
                    *k = t0;
                }
                for k in knots.iter_mut().skip(n - p - 1) {
                    *k = t1;
                }
            }
            let edge = Curve::Spline { knots, points: pts };
            let found = boundary_iso(&surface, &edge, [t0, t1], 1e-3)
                .unwrap()
                .unwrap_or_else(|| panic!("[{t0}, {t1}] {reversed}"));
            assert!(
                found.gap >= off * (1.0 - 1e-9) && found.gap <= off + 1e-6,
                "[{t0}, {t1}] {reversed}: proved {}",
                found.gap
            );
            let [from, to] = segment_points(&found.pcurve);
            assert_eq!([from[1], to[1]], [0.0, 0.0], "on v = 0");
            assert_eq!(from[0] == 0.0, !reversed);
            let lifted = lift_error(&surface, &edge, [t0, t1], &found.pcurve);
            assert!(
                lifted <= found.gap,
                "[{t0}, {t1}] {reversed}: {lifted} against {}",
                found.gap
            );
        }
    }
}

/// `boundary_covered` is the importer's one question: an edge is covered for the line only where
/// `exact_pcurves` then takes it at the covered tolerance. For every side, either way, and the gaps
/// of the other tests: the answer is the proved gap exactly where the line is taken at the larger of
/// the tolerance and the margin times the gap; at a margin under 2 (which asks the proof within
/// half of less than its own gap) the bound is proved and the line is not taken, and no gap is
/// given.
#[test]
fn a_boundary_gap_is_covered_only_where_the_closed_form_takes_the_line() {
    let surface = twisted(2.);
    let (face, margin, most) = (1e-6, SURFACE_GAP_MARGIN, 1.0);
    for off in [0.0, 2e-5, 7.5e-5, 1e-3, 0.016, 0.3] {
        for (from, to) in SIDES {
            for reversed in [false, true] {
                let (a, b) = if reversed { (to, from) } else { (from, to) };
                let edge = boundary_edge(2., a, b, off);
                let proved = boundary_iso(&surface, &edge, [0., 1.], BOUNDARY_GAP_CAP).unwrap();
                let covered =
                    boundary_covered(&surface, &edge, [0., 1.], face, margin, most).unwrap();
                assert_eq!(covered, proved.as_ref().map(|p| p.gap), "{off} {a:?} {b:?}");
                if let Some(gap) = covered {
                    let raised = face.max(margin * gap);
                    let made =
                        exact_pcurves(&surface, &one(&edge, [0., 1.]), raised, None).unwrap();
                    assert!(made[0][0].is_some(), "{off}: covered but not taken");
                    // The edge's own tolerance, where it is the larger, is the one asked at.
                    let wide =
                        boundary_covered(&surface, &edge, [0., 1.], 0.5, margin, most).unwrap();
                    assert_eq!(wide, Some(gap));
                }
            }
        }
        let edge = boundary_edge(2., [0., 0.], [1., 0.], off);
        let proved = boundary_iso(&surface, &edge, [0., 1.], most).unwrap();
        // A margin of 1.5 asks the closed form within 0.75 of the gap, which the proof is not.
        let low = boundary_covered(&surface, &edge, [0., 1.], face, 1.5, most).unwrap();
        if let Some(p) = proved {
            if p.gap > 0.5 * face / 0.75 {
                assert_eq!(
                    low, None,
                    "{off}: proved {} but not taken at a margin of 1.5",
                    p.gap
                );
            }
        }
    }
    // Past what a tolerance holds, nothing; and nothing for a tolerance or margin that is not a number.
    let far = boundary_edge(2., [0., 0.], [1., 0.], 0.5);
    assert_eq!(
        boundary_covered(&surface, &far, [0., 1.], face, margin, most).unwrap(),
        None
    );
    for (tolerance, margin) in [
        (f64::NAN, 3.0),
        (1e-6, f64::NAN),
        (0.0, 3.0),
        (1e-6, f64::INFINITY),
        (1e-6, -3.0),
    ] {
        assert!(boundary_covered(&surface, &far, [0., 1.], tolerance, margin, 1.0).is_err());
    }
}

/// A patch closed in one direction has a seam: its edge along the seam (or round the closed
/// direction) is no boundary, the proof gives none, no tolerance is covered for it, and
/// `exact_pcurves` makes of it only what the straight-line candidate always made (taken at
/// the tolerance, nothing proved). A polynomial net that comes back to its first row in u.
#[test]
fn a_patch_closed_in_one_direction_has_no_boundary_proof_on_its_seam() {
    let square = [[0., 0.], [10., 0.], [10., 10.], [0., 10.], [0., 0.]];
    let closed = Surface::Spline {
        u_knots: vec![0., 0., 1., 2., 3., 4., 4.],
        v_knots: vec![0., 0., 1., 1.],
        points: square
            .iter()
            .map(|[x, y]| vec![[*x, *y, 0.], [*x, *y, 5.]])
            .collect(),
    };
    // The seam: the v-curve at u = 0 (and the same at u = 4), either way; the u-curves at v = 0 and v = 1.
    let seams = [([0., 0., 0.], [0., 0., 5.]), ([0., 0., 5.], [0., 0., 0.])];
    let rim = |z: f64| Curve::Spline {
        knots: vec![0., 0., 1., 2., 3., 4., 4.],
        points: square.iter().map(|[x, y]| [*x, *y, z]).collect(),
    };
    let mut edges: Vec<(Curve, [f64; 2])> = seams
        .iter()
        .map(|(a, b)| {
            (
                Curve::Spline {
                    knots: vec![0., 0., 1., 1.],
                    points: vec![*a, *b],
                },
                [0., 1.],
            )
        })
        .collect();
    edges.push((rim(0.), [0., 4.]));
    edges.push((rim(5.), [0., 4.]));
    for (edge, interval) in &edges {
        assert_eq!(boundary_iso(&closed, edge, *interval, 1e-3).unwrap(), None);
        assert_eq!(
            boundary_covered(&closed, edge, *interval, 1e-6, SURFACE_GAP_MARGIN, 1.0).unwrap(),
            None
        );
        // What `exact_pcurves` makes is what the straight-line candidate makes, and nothing else.
        let on = On::new(&closed).unwrap();
        let eval = CurveEval::new(edge).unwrap();
        let tol = 1e-3;
        let direct = on.pcurve(edge, &eval, *interval, tol).ok();
        let made = exact_pcurves(&closed, &one(edge, *interval), tol, None)
            .unwrap()
            .remove(0)
            .remove(0);
        assert_eq!(made, direct);
        assert_eq!(
            made,
            on.along(&eval, *interval, tol)
                .filter(|pc| on.lifts(pc, &eval, *interval, 0.5 * tol))
        );
    }
}

/// A patch of 10 m (the twisted one, in millimetres), and its side v = 0 as an edge `off` mm above
/// it.
fn metres_edge(off: f64) -> (Surface, Curve) {
    let k = 1000.0;
    let at = |x: f64, y: f64, z: f64| [x * k, y * k, z * k];
    let surface = Surface::Spline {
        u_knots: vec![0., 0., 1., 1.],
        v_knots: vec![0., 0., 1., 1.],
        points: vec![
            vec![at(0., 0., 0.), at(0., 10., 0.)],
            vec![at(10., 0., 0.), at(10., 10., 2.)],
        ],
    };
    let edge = Curve::Spline {
        knots: vec![0., 0., 1., 1.],
        points: vec![[0., 0., off], [10. * k, 0., off]],
    };
    (surface, edge)
}

/// No edge tolerance past 10³ mm is honoured, the face's or an edge record's (the largest
/// `pcurves` takes): a cover that would ask for one is none, whatever `max_tolerance` allows.
#[test]
fn a_boundary_is_not_covered_at_a_tolerance_past_what_an_edge_is_honoured_at() {
    let (most, margin) = (2e3, 1.0);
    // The metre-scale gaps here are far past the exporter-rounding cap, so the ceiling is asked
    // with that cap lifted to what the tolerance alone allows.
    let wide = |surface: &Surface, edge: &Curve, tolerance: f64, margin: f64, most: f64| {
        covered_on(
            &On::new(surface).unwrap(),
            edge,
            [0., 1.],
            tolerance,
            margin,
            most,
            f64::INFINITY,
        )
        .unwrap()
        .map(|(gap, _)| gap)
    };
    // An edge 400 mm off a patch 10 m across, covered at 900 mm: within the ceiling, and taken.
    let (surface, near) = metres_edge(400.0);
    let found = wide(&surface, &near, 900.0, margin, most);
    assert!(
        found.is_some_and(|g| (400.0..401.0).contains(&g)),
        "{found:?}"
    );
    // An edge 1200 mm off, covered at 3000 mm: the proof holds and 3000 is within `max_tolerance`,
    // but is past the ceiling, so no cover.
    let (surface, far) = metres_edge(1200.0);
    assert!(boundary_iso(&surface, &far, [0., 1.], most)
        .unwrap()
        .is_some());
    assert_eq!(wide(&surface, &far, 3000.0, margin, 4e3), None);
    // The same edge at the ceiling itself is covered.
    assert!(
        wide(&surface, &far, 1000.0, margin, 4e3).is_none(),
        "half of 1000 mm is under the 1200 mm gap"
    );
    let (surface, mid) = metres_edge(450.0);
    assert!(wide(&surface, &mid, 1000.0, margin, 4e3).is_some());
}

/// An edge 16 µm off a patch's boundary is another curve, not exporter rounding: no proof, no
/// cover, however large the tolerance the file allows.
#[test]
fn an_edge_far_off_the_boundary_for_rounding_is_no_boundary() {
    let surface = twisted(2.);
    let edge = boundary_edge(2., [0., 0.], [1., 0.], 0.016);
    assert_eq!(
        boundary_iso(&surface, &edge, [0., 1.], BOUNDARY_GAP_CAP).unwrap(),
        None
    );
    assert_eq!(
        boundary_covered(&surface, &edge, [0., 1.], 0.01, SURFACE_GAP_MARGIN, 1.0).unwrap(),
        None
    );
}

/// A rounded cube's edge strip and corner patch, as a STEP exporter writes them: rational
/// quadratic conics of middle weight `W`, the edge curves their own records `off` mm off.
const CONIC_W: f64 = 2.0 / 3.0;

fn homogeneous(p: [f64; 3], w: f64) -> [f64; 4] {
    [p[0] * w, p[1] * w, p[2] * w, w]
}

/// A conic of weights (1, `CONIC_W`, 1) through `a` and `b` with middle control point `m`.
fn conic(a: [f64; 3], m: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::RationalSpline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        homogeneous: vec![
            homogeneous(a, 1.0),
            homogeneous(m, CONIC_W),
            homogeneous(b, 1.0),
        ],
    }
}

/// The strip along z of a rounded edge (radius 3, z from 3 to 17): u across the conic, v along z.
fn conic_strip() -> Surface {
    let rows = [([3., 0.], 1.0), ([0., 0.], CONIC_W), ([0., 3.], 1.0)];
    Surface::RationalSpline {
        u_knots: vec![0., 0., 0., 1., 1., 1.],
        v_knots: vec![0., 0., 1., 1.],
        homogeneous: rows
            .iter()
            .map(|([x, y], w)| {
                vec![
                    homogeneous([*x, *y, 3.], *w),
                    homogeneous([*x, *y, 17.], *w),
                ]
            })
            .collect(),
    }
}

fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::Line { start: a, end: b }
}

/// The corner patch at the origin: a biquadratic net with its u = 0 side collapsed to a point.
fn conic_corner() -> Surface {
    let net = [
        [[3., 0., 3.], [3., 0., 3.], [3., 0., 3.]],
        [[0., 0., 3.], [0.75, 0.75, 0.75], [3., 0., 0.]],
        [[0., 3., 3.], [0., 3., 0.], [3., 3., 0.]],
    ];
    let w = [1.0, CONIC_W, 1.0];
    Surface::RationalSpline {
        u_knots: vec![0., 0., 0., 1., 1., 1.],
        v_knots: vec![0., 0., 0., 1., 1., 1.],
        homogeneous: net
            .iter()
            .zip(w)
            .map(|(row, wi)| {
                row.iter()
                    .zip(w)
                    .map(|(p, wj)| homogeneous(*p, wi * wj))
                    .collect()
            })
            .collect(),
    }
}

fn uses_with(edges: &[(Curve, [f64; 2])]) -> Vec<EdgeUse<'_>> {
    uses_of(edges, &[20, 21, 22, 23][..edges.len()])
}

/// A rational patch's boundary is proved where the edge has the patch's own knots and weights
/// (a conic edge a few micrometres off, and the straight sides along its v direction, which is
/// not rational), and a face with one side collapsed to a point is the whole patch on its other
/// three. A different weight, a different knot, or an edge on the collapsed side is not proved.
#[test]
fn a_rational_patchs_boundary_is_proved_where_the_edge_has_its_knots_and_weights() {
    let off = 3e-5;
    let strip = conic_strip();
    let edges = vec![
        (conic([3., 0., 3.], [0., off, 3.], [0., 3., 3.]), [0., 1.]),
        (
            conic([3., 0., 17.], [0., off, 17.], [0., 3., 17.]),
            [0., 1.],
        ),
        (line([3., 0., 3.], [3., 0., 17.]), [0., 1.]),
        (line([0., 3., 3.], [0., 3., 17.]), [0., 1.]),
    ];
    let gaps = boundary_covers(&strip, &uses_with(&edges), None, SURFACE_GAP_MARGIN, 1.0)
        .unwrap()
        .expect("the strip's four sides");
    assert_eq!(gaps.len(), 4);
    assert!((off..off + 1e-9).contains(&gaps[0]), "{gaps:?}");
    assert!(gaps[2] < 1e-9, "{gaps:?}");
    // The same edge with another middle weight is a different curve: no proof.
    let mut other = edges.clone();
    other[0].0 = Curve::RationalSpline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        homogeneous: vec![
            homogeneous([3., 0., 3.], 1.0),
            homogeneous([0., off, 3.], 0.7),
            homogeneous([0., 3., 3.], 1.0),
        ],
    };
    assert_eq!(
        boundary_covers(&strip, &uses_with(&other), None, SURFACE_GAP_MARGIN, 1.0).unwrap(),
        None
    );
    // The corner patch: three edges and a collapsed side.
    let corner = conic_corner();
    let edges = vec![
        (conic([3., 0., 3.], [0., off, 3.], [0., 3., 3.]), [0., 1.]),
        (conic([0., 3., 3.], [0., 3., 0.], [3., 3., 0.]), [0., 1.]),
        (conic([3., 3., 0.], [3., 0., 0.], [3., 0., 3.]), [0., 1.]),
    ];
    let gaps = boundary_covers(&corner, &uses_with(&edges), None, SURFACE_GAP_MARGIN, 1.0)
        .unwrap()
        .expect("the corner's three sides");
    assert_eq!(gaps.len(), 3);
    assert!(gaps.iter().all(|g| *g < 1e-4), "{gaps:?}");
    // Four uses of a patch with a collapsed side, or two, are not the whole patch.
    assert_eq!(
        boundary_covers(
            &corner,
            &uses_with(&edges[..2]),
            None,
            SURFACE_GAP_MARGIN,
            1.0
        )
        .unwrap(),
        None
    );
}

/// A face's parameter box drops the sides it does not hold before they are ranked: the line is the
/// face's boundary only where the box holds it.
#[test]
fn a_side_outside_the_faces_box_is_no_boundary_of_it() {
    let surface = twisted(2.);
    let tol = 1e-3;
    for (side, (from, to)) in SIDES.iter().enumerate() {
        let edge = boundary_edge(2., *from, *to, 2e-5);
        let eval = CurveEval::new(&edge).unwrap();
        let line = |face: Option<[[f64; 2]; 2]>| {
            On::new(&surface)
                .unwrap()
                .within(face)
                .boundary_line(&edge, &eval, [0., 1.], tol)
        };
        // The whole box, a padded one, and none: the side is found, and said to be the one it is.
        for face in [
            None,
            Some([[0., 1.], [0., 1.]]),
            Some([[-1e-12, 1.], [0., 1. + 1e-12]]),
            Some([[-1., 2.], [-1., 2.]]),
        ] {
            assert_eq!(line(face).map(|b| b.side), Some(side), "{side} in {face:?}");
        }
        // A box that does not reach the side: not chosen. Each of the four halves of the patch
        // holds two sides whole and the ends of the others.
        for (name, half) in [
            ("v from 0.5", [[0., 1.], [0.5, 1.]]),
            ("v to 0.5", [[0., 1.], [0., 0.5]]),
            ("u from 0.5", [[0.5, 1.], [0., 1.]]),
            ("u to 0.5", [[0., 0.5], [0., 1.]]),
        ] {
            let holds = [from, to]
                .iter()
                .all(|q| q.iter().zip(half).all(|(x, r)| *x >= r[0] && *x <= r[1]));
            assert_eq!(line(Some(half)).is_some(), holds, "side {side} in {name}");
        }
    }
    // The ranking is among the sides the box holds: a box over the v = 1 half, where v = 0's
    // edge is no candidate, gives the edge of the side v = 1 its own side, and v = 0's nothing.
    let top = Some([[0., 1.], [0.5, 1.]]);
    let low = boundary_edge(2., [0., 0.], [1., 0.], 0.);
    let (on, eval) = (
        On::new(&surface).unwrap().within(top),
        CurveEval::new(&low).unwrap(),
    );
    assert_eq!(on.boundary_line(&low, &eval, [0., 1.], tol), None);
    let found = |face| {
        covered_on(
            &On::new(&surface).unwrap().within(face),
            &low,
            [0., 1.],
            1e-6,
            SURFACE_GAP_MARGIN,
            1.0,
            BOUNDARY_GAP_CAP,
        )
        .unwrap()
    };
    assert!(found(None).is_some_and(|(_, side)| side == 0));
    assert_eq!(found(top), None);
}

/// The four sides of [`twisted`] as the face's one loop: each `off` mm above its side, edge ids
/// 0 to 3 in the order of `order` (indices into [`SIDES`]), each running `reversed` where it says.
fn whole_patch(off: f64, order: [usize; 4], reversed: [bool; 4]) -> Vec<(Curve, [f64; 2])> {
    order
        .iter()
        .zip(reversed)
        .map(|(k, r)| {
            let (from, to) = SIDES[*k];
            let (a, b) = if r { (to, from) } else { (from, to) };
            (boundary_edge(2., a, b, off), [0., 1.])
        })
        .collect()
}

fn uses_of<'a>(edges: &'a [(Curve, [f64; 2])], ids: &[u32]) -> Vec<EdgeUse<'a>> {
    edges
        .iter()
        .zip(ids)
        .map(|((curve, interval), edge)| EdgeUse {
            edge: *edge,
            curve,
            interval: *interval,
            forward: true,
            tolerance: 1e-6,
            given: None,
        })
        .collect()
}

/// A face is read as the whole patch only where its uses are exactly the four natural sides, one
/// each, each covered: in any order or direction it gives the four gaps (each what the per-edge
/// question gives); three uses, a side used twice, an edge used twice, a side with no edge, an
/// edge on no side, a box that is a part of the patch, a rational patch or a plane give none.
#[test]
fn a_face_is_the_whole_patch_only_as_its_four_natural_sides() {
    let surface = twisted(2.);
    let margin = SURFACE_GAP_MARGIN;
    let covers = |s: &Surface, uses: &[EdgeUse<'_>], domain| {
        boundary_covers(s, uses, domain, margin, 1.0).unwrap()
    };
    let ids = [10, 11, 12, 13];
    for (order, reversed) in [
        ([0, 1, 2, 3], [false; 4]),
        ([3, 2, 1, 0], [true; 4]),
        ([2, 0, 3, 1], [true, false, false, true]),
    ] {
        let edges = whole_patch(2e-5, order, reversed);
        let uses = uses_of(&edges, &ids);
        let gaps =
            covers(&surface, &uses, None).unwrap_or_else(|| panic!("{order:?} {reversed:?}"));
        assert_eq!(gaps.len(), 4);
        for (g, (edge, interval)) in gaps.iter().zip(&edges) {
            let one = boundary_covered(&surface, edge, *interval, 1e-6, margin, 1.0).unwrap();
            assert_eq!(Some(*g), one, "{order:?}");
            assert!(*g >= 2e-5 && *g <= 2e-5 + 1e-9, "{g}");
        }
        // The natural box, as it is or padded, is the whole patch; a part of it is a trimmed face.
        let natural = Some([[0., 1.], [0., 1.]]);
        assert_eq!(covers(&surface, &uses, natural), Some(gaps.clone()));
        assert_eq!(
            covers(&surface, &uses, Some([[0., 1. + 1e-12], [0., 1.]])),
            Some(gaps.clone())
        );
        for part in [
            [[0., 1.], [0., 0.5]],
            [[0.1, 1.], [0., 1.]],
            [[0., 1.], [0., 1.1]],
            [[0., 1.], [f64::NAN, 1.]],
        ] {
            assert_eq!(covers(&surface, &uses, Some(part)), None, "{part:?}");
        }
        // Three uses, none, or five.
        assert_eq!(covers(&surface, &uses[..3], None), None);
        assert_eq!(covers(&surface, &[], None), None);
        let mut five = edges.clone();
        five.push(five[0].clone());
        assert_eq!(
            covers(&surface, &uses_of(&five, &[10, 11, 12, 13, 14]), None),
            None
        );
        // An edge used twice (the same id, so four uses of three edges), or a side twice (two
        // edges on one side, and none on another).
        assert_eq!(
            covers(&surface, &uses_of(&edges, &[10, 11, 12, 12]), None),
            None
        );
        let mut twice = edges.clone();
        twice[3] = twice[0].clone();
        assert_eq!(covers(&surface, &uses_of(&twice, &ids), None), None);
        // An edge that is no boundary, or off by more than the cap.
        let mut trimmed = edges.clone();
        trimmed[2] = (
            Curve::Spline {
                knots: vec![0., 0., 1., 1.],
                points: vec![[5., 0., 0.], [5., 10., 1.]],
            },
            [0., 1.],
        );
        assert_eq!(covers(&surface, &uses_of(&trimmed, &ids), None), None);
        let mut far = edges.clone();
        far[1] = whole_patch(0.5, order, reversed).swap_remove(1);
        assert_eq!(covers(&surface, &uses_of(&far, &ids), None), None);
    }
    // Not a polynomial patch: a plane, a rational net.
    let edges = whole_patch(0., [0, 1, 2, 3], [false; 4]);
    let uses = uses_of(&edges, &ids);
    let plane = Surface::Plane {
        origin: [0.; 3],
        u_axis: [10., 0., 0.],
        v_axis: [0., 10., 0.],
    };
    let rational = Surface::RationalSpline {
        u_knots: vec![0., 0., 1., 1.],
        v_knots: vec![0., 0., 1., 1.],
        homogeneous: vec![
            vec![[0., 0., 0., 1.], [0., 10., 0., 1.]],
            vec![[10., 0., 0., 1.], [20., 20., 4., 2.]],
        ],
    };
    assert_eq!(covers(&plane, &uses, None), None);
    assert_eq!(covers(&rational, &uses, None), None);
    // Numbers that are not numbers are errors, never a panic.
    for (margin, most) in [(f64::NAN, 1.0), (3.0, 0.0), (-3.0, 1.0)] {
        assert!(boundary_covers(&surface, &uses, None, margin, most).is_err());
    }
    let mut bad = uses.clone();
    bad[0].tolerance = f64::NAN;
    assert!(boundary_covers(&surface, &bad, None, margin, 1.0).is_err());
}
