use super::*;
use crate::brep::Frame;
use crate::brep::eval::{CurveEval, SurfaceEval};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn frame(origin: [f64; 3], x_axis: [f64; 3], y_axis: [f64; 3]) -> Frame {
    Frame { origin, x_axis, y_axis }
}
/// A frame about the axis through `origin` along +z, or −z when `down` (x stays +x).
fn about_z(origin: [f64; 3], down: bool) -> Frame {
    frame(origin, [1., 0., 0.], if down { [0., -1., 0.] } else { [0., 1., 0.] })
}
fn one(curve: &Curve, interval: [f64; 2]) -> Vec<Vec<EdgeUse<'_>>> {
    vec![vec![EdgeUse { edge: 0, curve, interval, forward: true, tolerance: 0.0 }]]
}
/// The largest distance between the lifted pcurve and the curve, at 65 points and each span's
/// middle.
fn lift_error(surface: &Surface, curve: &Curve, interval: [f64; 2], pc: &Curve2) -> f64 {
    let (s, c) = (SurfaceEval::new(surface).unwrap(), CurveEval::new(curve).unwrap());
    let [t0, t1] = interval;
    assert_eq!(pc.interval(), Some(interval), "on the edge's interval");
    let knots = match pc {
        Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. } => knots,
    };
    let mut ts: Vec<f64> = (0..=64).map(|k| t0 + (t1 - t0) * k as f64 / 64.).collect();
    ts.extend(knots.windows(2).filter(|k| k[0] < k[1]).map(|k| 0.5 * (k[0] + k[1])));
    ts.iter().map(|&t| s.point(pc.eval(t).unwrap()).unwrap().dist(c.point(t).unwrap())).fold(0., f64::max)
}
fn segment_points(pc: &Curve2) -> [[f64; 2]; 2] {
    match pc {
        Curve2::Spline { knots, points } if knots.len() == 4 && points.len() == 2 => [points[0], points[1]],
        other => panic!("not a degree-1 segment: {other:?}"),
    }
}

#[test]
fn lines_polylines_and_splines_on_planes_are_their_points_mapped() {
    let plane = Surface::PlaneThrough { origin: [1., 2., 3.], u_point: [3., 2., 3.], v_point: [1., 2., 7.] };
    let line = Curve::Line { start: [2., 2., 4.], end: [5., 2., 11.] };
    let pc = &pcurves(&plane, &one(&line, [0., 1.]), 1e-9).unwrap()[0][0];
    // Bit for bit: the ends lift to the line's own ends.
    let [a, b] = segment_points(pc);
    assert_eq!((a, b), ([0.5, 0.25], [2., 2.]));
    // On part of the line, extended past it.
    let pc = &pcurves(&plane, &one(&line, [-1., 3.]), 1e-9).unwrap()[0][0];
    assert!(lift_error(&plane, &line, [-1., 3.], pc) <= 1e-12);
    let poly = Curve::Polyline { points: vec![[1., 2., 3.], [3., 2., 3.], [3., 2., 7.], [5., 2., 9.]] };
    let pc = &pcurves(&plane, &one(&poly, [0.5, 3.]), 1e-9).unwrap()[0][0];
    let Curve2::Spline { knots, points } = pc else { panic!("a spline") };
    assert_eq!(knots, &[0.5, 0.5, 1., 2., 3., 3.]);
    assert_eq!(points, &[[0.5, 0.], [1., 0.], [1., 1.], [2., 1.5]]);
    assert!(lift_error(&plane, &poly, [0.5, 3.], pc) <= 1e-12);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let arc = Curve::RationalSpline {
        knots: vec![0., 0., 0., 1., 1., 1.],
        homogeneous: vec![[3., 2., 3., 1.], [3. * w, 2. * w, 5. * w, w], [1., 2., 5., 1.]],
    };
    let pc = &pcurves(&plane, &one(&arc, [0., 1.]), 1e-9).unwrap()[0][0];
    assert!(matches!(pc, Curve2::RationalSpline { .. }));
    assert!(lift_error(&plane, &arc, [0., 1.], pc) <= 1e-12);
    let cubic = Curve::Spline { knots: vec![0., 0., 0., 0., 2., 2., 2., 2.], points: vec![[1., 2., 3.], [4., 2., 3.], [2., 2., 6.], [5., 2., 8.]] };
    let pc = &pcurves(&plane, &one(&cubic, [0., 2.]), 1e-9).unwrap()[0][0];
    assert_eq!(pc, &Curve2::Spline { knots: vec![0., 0., 0., 0., 2., 2., 2., 2.], points: vec![[0., 0.], [1.5, 0.], [0.5, 0.75], [2., 1.25]] });
    // A piece of the spline is fitted instead (its knots would need cutting).
    let pc = &pcurves(&plane, &one(&cubic, [0.5, 1.5]), 1e-7).unwrap()[0][0];
    assert!(lift_error(&plane, &cubic, [0.5, 1.5], pc) <= 1e-7);
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
        frame(origin, [c, s, 0.], if down { [s, -c, 0.] } else { [-s, c, 0.] })
    };
    let half = 0.4f64;
    let cases = [
        (Surface::Cylinder { frame: about_z(o, false), radius: 5. }, 7., 5., 7.),
        (Surface::Cone { frame: about_z(o, false), radius_at_origin: 2., half_angle: half }, 4., 2. + 4. * half.tan(), 4.),
        (Surface::Sphere { frame: about_z(o, false), radius: 5. }, 3., 4., 3f64.atan2(4.)),
        (Surface::Sphere { frame: about_z(o, false), radius: 5. }, -3., 4., (-3f64).atan2(4.)),
        (Surface::Torus { frame: about_z(o, false), major_radius: 10., minor_radius: 2. }, 2f64.sqrt(), 10. - 2f64.sqrt(), 0.75 * PI),
    ];
    for (surface, h, r, v) in cases {
        for down in [false, true] {
            let circle = Curve::Circle { frame: rotated(at(h), down), radius: r };
            for interval in [[0., TAU], [-1., 2.5], [4., 5.]] {
                let pc = &pcurves(&surface, &one(&circle, interval), 1e-9).unwrap()[0][0];
                let [a, b] = segment_points(pc);
                let sign = if down { -1. } else { 1. };
                // The u-line itself: the only freedom is the period the loop is moved by.
                let shift = a[0] - (tilt + sign * interval[0]);
                assert!((shift / TAU - (shift / TAU).round()).abs() < 1e-14, "{surface:?} {down}: {a:?}");
                assert!((b[0] - a[0] - sign * (interval[1] - interval[0])).abs() < 1e-13);
                assert!(a[1] == b[1] && (a[1] - v).abs() < 1e-14, "{surface:?}: v {a:?} {v}");
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
    let cylinder = Surface::Cylinder { frame: about_z([0.; 3], false), radius: 4. };
    let cone = Surface::Cone { frame: about_z([0., 0., 1.], false), radius_at_origin: 3., half_angle: half };
    let (s, c) = 2f64.sin_cos();
    let up = Curve::Line { start: [4. * c, 4. * s, -1.], end: [4. * c, 4. * s, 6.] };
    let down = Curve::Line { start: [4. * c, 4. * s, 6.], end: [4. * c, 4. * s, -1.] };
    let r = |z: f64| 3. + (z - 1.) * half.tan();
    let slant = Curve::Line { start: [r(2.) * c, r(2.) * s, 2.], end: [r(5.) * c, r(5.) * s, 5.] };
    for (surface, line, heights) in [(&cylinder, &up, [-1., 6.]), (&cylinder, &down, [6., -1.]), (&cone, &slant, [1., 4.])] {
        for interval in [[0., 1.], [0.25, 2.]] {
            let pc = &pcurves(surface, &one(line, interval), 1e-9).unwrap()[0][0];
            let [a, b] = segment_points(pc);
            assert_eq!(a[0], b[0]);
            assert!((a[0] - 2.).abs() < 1e-14);
            let height = |t: f64| heights[0] + (heights[1] - heights[0]) * t;
            assert!((a[1] - height(interval[0])).abs() < 1e-13 && (b[1] - height(interval[1])).abs() < 1e-13, "{a:?} {b:?}");
            assert!(lift_error(surface, line, interval, pc) <= 1e-12);
        }
    }
}

/// Curves with no closed form are fitted cubic B-splines within the tolerance: a tilted circle
/// on a sphere, a plane's ellipse on a cylinder, a circle off the axis on a plane.
#[test]
fn other_curves_are_fitted_within_tolerance() {
    let (s, c) = 0.5f64.sin_cos();
    let sphere = Surface::Sphere { frame: about_z([0.; 3], false), radius: 6. };
    let great = Curve::Circle { frame: frame([0.; 3], [1., 0., 0.], [0., c, s]), radius: 6. };
    let cylinder = Surface::Cylinder { frame: about_z([0.; 3], false), radius: 2. };
    let ellipse = Curve::Ellipse {
        frame: frame([0., 0., 1.], [1. / (1f64 + 0.25).sqrt(), 0., 0.5 / (1f64 + 0.25).sqrt()], [0., 1., 0.]),
        radii: [2. * (1.25f64).sqrt(), 2.],
    };
    let plane = Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] };
    let circle = Curve::Circle { frame: about_z([3., 1., 0.], false), radius: 2. };
    for (surface, curve, interval) in [(&sphere, &great, [0., TAU]), (&cylinder, &ellipse, [0.2, 6.]), (&plane, &circle, [0., 3.])] {
        for tol in [1e-3, 1e-6] {
            let pc = &pcurves(surface, &one(curve, interval), tol).unwrap()[0][0];
            let Curve2::Spline { knots, .. } = pc else { panic!("a B-spline") };
            assert_eq!(knots.len() - pc_count(pc) - 1, 3, "cubic");
            let e = lift_error(surface, curve, interval, pc);
            assert!(e <= tol, "{curve:?} at {tol}: {e}");
        }
    }
}
fn pc_count(pc: &Curve2) -> usize {
    match pc {
        Curve2::Spline { points, .. } => points.len(),
        Curve2::RationalSpline { homogeneous, .. } => homogeneous.len(),
    }
}

/// The ends of each use in loop order and the loop's signed area in (u, v).
fn walk(l: &[EdgeUse<'_>], pcs: &[Curve2]) -> (Vec<([f64; 2], [f64; 2])>, f64) {
    let mut ring = Vec::new();
    let mut ends = Vec::new();
    for (u, pc) in l.iter().zip(pcs) {
        let [a, b] = u.interval;
        let ts: Vec<f64> = (0..=16).map(|k| if u.forward { a + (b - a) * k as f64 / 16. } else { b - (b - a) * k as f64 / 16. }).collect();
        let pts: Vec<[f64; 2]> = ts.iter().map(|t| pc.eval(*t).unwrap()).collect();
        ends.push((pts[0], pts[16]));
        ring.extend_from_slice(&pts[..16]);
    }
    let n = ring.len();
    let area = (0..n).map(|i| ring[i][0] * ring[(i + 1) % n][1] - ring[(i + 1) % n][0] * ring[i][1]).sum::<f64>() * 0.5;
    (ends, area)
}

/// A cylinder's wall between two full circles, with its seam line used twice: the seam's uses
/// are one period apart, the loop closes and turns counter-clockwise, all in the base period.
#[test]
fn a_seam_used_twice_gets_pcurves_a_period_apart() {
    let cylinder = Surface::Cylinder { frame: about_z([0.; 3], false), radius: 3. };
    let seam = Curve::Line { start: [3., 0., 0.], end: [3., 0., 4.] };
    let low = Curve::Circle { frame: about_z([0.; 3], false), radius: 3. };
    let high = Curve::Circle { frame: about_z([0., 0., 4.], false), radius: 3. };
    let full = [0., TAU];
    let l = vec![
        EdgeUse { edge: 0, curve: &low, interval: full, forward: true, tolerance: 0.0 },
        EdgeUse { edge: 1, curve: &seam, interval: [0., 1.], forward: true, tolerance: 0.0 },
        EdgeUse { edge: 2, curve: &high, interval: full, forward: false, tolerance: 0.0 },
        EdgeUse { edge: 1, curve: &seam, interval: [0., 1.], forward: false, tolerance: 0.0 },
    ];
    let pcs = &pcurves(&cylinder, std::slice::from_ref(&l), 1e-9).unwrap()[0];
    let (ends, area) = walk(&l, pcs);
    for k in 0..4 {
        let (end, start) = (ends[k].1, ends[(k + 1) % 4].0);
        assert!((end[0] - start[0]).abs() < 1e-12 && (end[1] - start[1]).abs() < 1e-12, "{k}: {end:?} {start:?}");
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
    let sphere = Surface::Sphere { frame: about_z([0.; 3], false), radius: 6. };
    let meridian = Curve::Circle { frame: frame([0.; 3], [1., 0., 0.], [0., 0., 1.]), radius: 6. };
    let half = [-FRAC_PI_2, FRAC_PI_2];
    let l = vec![
        EdgeUse { edge: 0, curve: &meridian, interval: half, forward: false, tolerance: 0.0 },
        EdgeUse { edge: 0, curve: &meridian, interval: half, forward: true, tolerance: 0.0 },
    ];
    for tol in [1e-3, 1e-6] {
        let pcs = &pcurves(&sphere, std::slice::from_ref(&l), tol).unwrap()[0];
        for pc in pcs {
            assert!(lift_error(&sphere, &meridian, half, pc) <= tol);
            for k in 0..=16 {
                let q = pc.eval(-FRAC_PI_2 + PI * k as f64 / 16.).unwrap();
                let u = pcs[0].eval(0.).unwrap()[0];
                assert!((q[0] - u).abs() < 1e-9 || ((q[0] - u).abs() - TAU).abs() < 1e-9, "straight: {q:?}");
            }
        }
        let (a, b) = (pcs[0].eval(0.).unwrap(), pcs[1].eval(0.).unwrap());
        assert!(((a[0] - b[0]).abs() - TAU).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-12, "{a:?} {b:?}");
        let (_, area) = walk(&l, pcs);
        assert!(area > 0., "counter-clockwise: {area}");
    }
}

/// Two meridians a three-quarter turn apart bound a quarter of a sphere or three quarters of
/// it; the loop alone can't tell which. Given the face's box, the loop spans it: three quarters
/// of a turn in u (a ball revolved 270°), or a quarter where the box says so.
#[test]
fn meridians_at_a_pole_span_the_face_box() {
    let sphere = Surface::Sphere { frame: about_z([0.; 3], false), radius: 6. };
    let at = |a: f64| Curve::Circle { frame: frame([0.; 3], [a.cos(), a.sin(), 0.], [0., 0., 1.]), radius: 6. };
    let (first, last) = (at(0.), at(1.5 * PI));
    let half = [-FRAC_PI_2, FRAC_PI_2];
    for (box_u, span) in [([0., 1.5 * PI], 1.5 * PI), ([-FRAC_PI_2, 0.], FRAC_PI_2)] {
        for flip in [false, true] {
            let l = vec![
                EdgeUse { edge: 0, curve: &first, interval: half, forward: !flip, tolerance: 0.0 },
                EdgeUse { edge: 1, curve: &last, interval: half, forward: flip, tolerance: 0.0 },
            ];
            let pcs = &pcurves_within(&sphere, std::slice::from_ref(&l), 1e-7, Some([box_u, [-FRAC_PI_2, FRAC_PI_2]])).unwrap()[0];
            let (a, b) = (pcs[0].eval(0.).unwrap()[0], pcs[1].eval(0.).unwrap()[0]);
            assert!(((a - b).abs() - span).abs() < 1e-9, "box {box_u:?}, flip {flip}: u {a} and {b}");
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
        let sphere = Surface::Sphere { frame: sf.clone(), radius: 6. };
        let ax = crate::brep::eval::Axes::new(&sf);
        for turn in [0f64, 2.1, -0.7] {
            let x = ax.x * turn.cos() + ax.y * turn.sin();
            let p = |v: Vec3| [v.x, v.y, v.z];
            let (o, r, z) = (ax.o, x * 6., ax.z * 6.);
            let circle = Curve::Circle { frame: Frame { origin: p(o), x_axis: p(x), y_axis: p(ax.z) }, radius: 6. };
            for off in [0., 0.1 * tol] {
                // South pole to north pole through the equator, as two rational quarter arcs,
                // the ends `off` to the side of the poles.
                let h = |v: Vec3, wt: f64| [v.x * wt, v.y * wt, v.z * wt, wt];
                let side = ax.y * off;
                let nurbs = Curve::RationalSpline {
                    knots: vec![0., 0., 0., 0.5, 0.5, 1., 1., 1.],
                    homogeneous: vec![h(o - z + side, 1.), h(o - z + r, w), h(o + r, 1.), h(o + z + r, w), h(o + z - side, 1.)],
                };
                for (seam, iv) in [(&circle, [-FRAC_PI_2, FRAC_PI_2]), (&nurbs, [0., 1.])] {
                    for order in [[true, false], [false, true]] {
                        let l: Vec<EdgeUse> =
                            order.iter().map(|&forward| EdgeUse { edge: 0, curve: seam, interval: iv, forward, tolerance: 0.0 }).collect();
                        let pcs = &pcurves(&sphere, std::slice::from_ref(&l), tol).unwrap()[0];
                        let what = format!("{sf:?} {turn} {off} {seam:?} {order:?}");
                        for pc in pcs {
                            assert!(lift_error(&sphere, seam, iv, pc) <= tol, "{what}");
                        }
                        let mid = 0.5 * (iv[0] + iv[1]);
                        let (a, b) = (pcs[0].eval(mid).unwrap(), pcs[1].eval(mid).unwrap());
                        assert!(((a[0] - b[0]).abs() - TAU).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-12, "a period apart: {a:?} {b:?} {what}");
                        let (ends, area) = walk(&l, pcs);
                        for k in 0..2 {
                            assert!((ends[k].1[1] - ends[(k + 1) % 2].0[1]).abs() < 1e-6, "closed in v: {ends:?} {what}");
                        }
                        // The box less the corners `walk` cuts (it joins each use's 16th
                        // sample to the next use's start): about 15/16 of it.
                        assert!((area / (TAU * PI * 15. / 16.) - 1.).abs() < 1e-2, "counter-clockwise around the box: {area} {what}");
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
    let frames = [about_z([0.; 3], false), frame([1., 2., 3.], [0., 0.6, 0.8], [0., -0.8, 0.6])];
    for sf in frames {
        let torus = Surface::Torus { frame: sf.clone(), major_radius: 10., minor_radius: 2. };
        let ax = crate::brep::eval::Axes::new(&sf);
        let p = |v: Vec3| [v.x, v.y, v.z];
        // The outer equator (v = 0, u from 0) and the tube circle at u = 0 (v from 0, upwards).
        let around = Curve::Circle { frame: sf.clone(), radius: 12. };
        let tube = Curve::Circle { frame: Frame { origin: p(ax.o + ax.x * 10.), x_axis: p(ax.x), y_axis: p(ax.z) }, radius: 2. };
        let full = [0., TAU];
        let l = vec![
            EdgeUse { edge: 0, curve: &around, interval: full, forward: true, tolerance: 0.0 },
            EdgeUse { edge: 1, curve: &tube, interval: full, forward: true, tolerance: 0.0 },
            EdgeUse { edge: 0, curve: &around, interval: full, forward: false, tolerance: 0.0 },
            EdgeUse { edge: 1, curve: &tube, interval: full, forward: false, tolerance: 0.0 },
        ];
        let pcs = &pcurves(&torus, std::slice::from_ref(&l), 1e-9).unwrap()[0];
        let (ends, area) = walk(&l, pcs);
        for k in 0..4 {
            let (end, start) = (ends[k].1, ends[(k + 1) % 4].0);
            assert!((end[0] - start[0]).abs() < 1e-12 && (end[1] - start[1]).abs() < 1e-12, "{k}: {end:?} {start:?}");
        }
        assert!((area / (TAU * TAU) - 1.).abs() < 1e-12, "counter-clockwise around the box: {area}");
    }
}

/// A surface of revolution about +z as a rational spline closed in u, as SMLib writes a fillet
/// ring round a hole's rim and STEP a NURBS cylinder: u the nine-point rational quadratic
/// circle on [0, 1] (first and last rows the same), v the profile's `[radius, height, weight]`
/// points on `v_knots`.
pub(crate) fn closed_ring(profile: &[[f64; 3]], v_knots: &[f64]) -> Surface {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let around = [[1., 0., 1.], [1., 1., w], [0., 1., 1.], [-1., 1., w], [-1., 0., 1.], [-1., -1., w], [0., -1., 1.], [1., -1., w], [1., 0., 1.]];
    let homogeneous = around
        .iter()
        .map(|&[cx, cy, wu]| {
            profile
                .iter()
                .map(|&[r, z, wv]| {
                    let wt = wu * wv;
                    [r * cx * wt, r * cy * wt, z * wt, wt]
                })
                .collect()
        })
        .collect();
    Surface::RationalSpline { u_knots: vec![0., 0., 0., 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1., 1., 1.], v_knots: v_knots.to_vec(), homogeneous }
}

/// The pcurves of a loop on a spline closed in u (period 1): each lifts within `tol`, the
/// seam's two uses (`seam`: their positions) lie one period apart, the loop closes (in v alone
/// where it meets a pole) and turns counter-clockwise once around the whole (u, v) box, all in
/// the base period.
fn closes_around_the_box(surface: &Surface, l: &[EdgeUse<'_>], seam: [usize; 2], pole: Option<usize>, tol: f64, what: &str) {
    let pcs = &pcurves(surface, &[l.to_vec()], tol).unwrap_or_else(|e| panic!("{what}: {e}"))[0];
    for (u, pc) in l.iter().zip(pcs) {
        assert!(lift_error(surface, u.curve, u.interval, pc) <= tol, "{what}: lifts");
    }
    let iv = l[seam[0]].interval;
    let mid = 0.5 * (iv[0] + iv[1]);
    let (a, b) = (pcs[seam[0]].eval(mid).unwrap(), pcs[seam[1]].eval(mid).unwrap());
    assert!(((a[0] - b[0]).abs() - 1.).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9, "{what}: seam uses a period apart: {a:?} {b:?}");
    let (ends, area) = walk(l, pcs);
    let n = l.len();
    for k in 0..n {
        let (end, start) = (ends[k].1, ends[(k + 1) % n].0);
        let free_u = pole == Some((k + 1) % n);
        assert!((free_u || (end[0] - start[0]).abs() < 1e-9) && (end[1] - start[1]).abs() < 1e-9, "{what}: closes at {k}: {end:?} {start:?}");
    }
    // At a pole `walk` cuts the corner (it joins each use's 16th sample to the next use's
    // start, across the pole's line): a sliver of the box less.
    let least = if pole.is_some() { 0.9 } else { 1. - 1e-6 };
    assert!(area >= least && area < 1. + 1e-6, "{what}: counter-clockwise around the box: {area}");
    for pc in pcs {
        let (Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. }) = pc;
        for k in 0..=32 {
            let t = knots[0] + (knots[knots.len() - 1] - knots[0]) * k as f64 / 32.;
            let q = pc.eval(t).unwrap();
            assert!((-1e-9..=1. + 1e-9).contains(&q[0]), "{what}: base period: {q:?}");
        }
    }
}

/// Splines closed on themselves in u (their first and last control rows the same) are periodic
/// there, one knot interval per period: a ring of a torus bounded by two circles and a seam used
/// twice (SMLib's fillet round a hole's rim), a NURBS cylinder, and a NURBS dome whose seam runs
/// into its pole. Each seam's uses come out a period apart, never both on u = 0.
#[test]
fn a_seam_on_a_spline_closed_in_u_gets_pcurves_a_period_apart() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let full = [0., TAU];
    let (big, small) = (5., 1.);
    // The torus band: the tube's quarter from its outer equator (v = 0) to its top (v = 1).
    let band = closed_ring(&[[big + small, 0., 1.], [big + small, small, w], [big, small, 1.]], &[0., 0., 0., 1., 1., 1.]);
    let outer = Curve::Circle { frame: about_z([0.; 3], false), radius: big + small };
    let top = Curve::Circle { frame: about_z([0., 0., small], false), radius: big };
    let quarter = Curve::Circle { frame: frame([big, 0., 0.], [1., 0., 0.], [0., 0., 1.]), radius: small };
    let l = vec![
        EdgeUse { edge: 0, curve: &outer, interval: full, forward: true, tolerance: 0.0 },
        EdgeUse { edge: 1, curve: &quarter, interval: [0., FRAC_PI_2], forward: true, tolerance: 0.0 },
        EdgeUse { edge: 2, curve: &top, interval: full, forward: false, tolerance: 0.0 },
        EdgeUse { edge: 1, curve: &quarter, interval: [0., FRAC_PI_2], forward: false, tolerance: 0.0 },
    ];
    for tol in [1e-4, 1e-7] {
        closes_around_the_box(&band, &l, [1, 3], None, tol, &format!("torus band, tol {tol}"));
    }
    // The cylinder: radius 3 from z = 0 (v = 0) to z = 4 (v = 1), its seam a line.
    let cylinder = closed_ring(&[[3., 0., 1.], [3., 4., 1.]], &[0., 0., 1., 1.]);
    let (low, high) =
        (Curve::Circle { frame: about_z([0.; 3], false), radius: 3. }, Curve::Circle { frame: about_z([0., 0., 4.], false), radius: 3. });
    let line = Curve::Line { start: [3., 0., 0.], end: [3., 0., 4.] };
    let l = vec![
        EdgeUse { edge: 0, curve: &low, interval: full, forward: true, tolerance: 0.0 },
        EdgeUse { edge: 1, curve: &line, interval: [0., 1.], forward: true, tolerance: 0.0 },
        EdgeUse { edge: 2, curve: &high, interval: full, forward: false, tolerance: 0.0 },
        EdgeUse { edge: 1, curve: &line, interval: [0., 1.], forward: false, tolerance: 0.0 },
    ];
    closes_around_the_box(&cylinder, &l, [1, 3], None, 1e-7, "cylinder");
    // The dome: radius 6 from its equator (v = 0) to its pole (v = 1), the seam a meridian.
    let dome = closed_ring(&[[6., 0., 1.], [6., 6., w], [0., 6., 1.]], &[0., 0., 0., 1., 1., 1.]);
    let equator = Curve::Circle { frame: about_z([0.; 3], false), radius: 6. };
    let meridian = Curve::Circle { frame: frame([0.; 3], [1., 0., 0.], [0., 0., 1.]), radius: 6. };
    let l = vec![
        EdgeUse { edge: 0, curve: &equator, interval: full, forward: true, tolerance: 0.0 },
        EdgeUse { edge: 1, curve: &meridian, interval: [0., FRAC_PI_2], forward: true, tolerance: 0.0 },
        EdgeUse { edge: 1, curve: &meridian, interval: [0., FRAC_PI_2], forward: false, tolerance: 0.0 },
    ];
    closes_around_the_box(&dome, &l, [1, 2], Some(2), 1e-7, "dome");
    // A spline whose ends merely come near each other stays open: no period, so the seam's
    // second use cannot be moved and the loop does not close around the box.
    let Surface::RationalSpline { u_knots, v_knots, homogeneous } = &cylinder else { panic!("a rational spline") };
    let mut open = homogeneous.clone();
    open[8][0][1] += 1e-3;
    let open = Surface::RationalSpline { u_knots: u_knots.clone(), v_knots: v_knots.clone(), homogeneous: open };
    assert_eq!(SurfaceEval::new(&open).unwrap().domain().period, [None, None]);
    assert_eq!(SurfaceEval::new(&cylinder).unwrap().domain().period, [Some(1.), None]);
}

/// An intersection record evaluates to the point of both surfaces in the leader's normal
/// plane, and its pcurves on either surface fit.
#[test]
fn intersection_curves_lie_on_both_surfaces() {
    // The cylinder x² + y² = 25 cut by the plane z = 20 + x tan 0.4: an ellipse.
    let cylinder = Surface::Cylinder { frame: about_z([0.; 3], false), radius: 5. };
    let k = 0.4f64.tan();
    let plane = Surface::Plane { origin: [0., 0., 20.], u_axis: [1., 0., k], v_axis: [0., 1., 0.] };
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
        assert!((p.x.hypot(p.y) - 5.).abs() < 1e-11 && (p.z - 20. - p.x * k).abs() < 1e-11, "{i}: {p:?}");
    }
    for surface in [&cylinder, &plane] {
        let pc = &pcurves(surface, &one(&curve, [0., 12.]), 1e-6).unwrap()[0][0];
        assert!(lift_error(surface, &curve, [0., 12.], pc) <= 1e-6);
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
        points: inner.iter().zip(&outer).map(|(a, b)| vec![*a, *b]).collect(),
    };
    let middle =
        Curve::Spline { knots: u_knots, points: inner.iter().zip(&outer).map(|(a, b)| [0, 1, 2].map(|k| (1. - at) * a[k] + at * b[k])).collect() };
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

/// A curve that is smooth through a knot of full multiplicity (one smooth curve with the knot
/// inserted three times, as SMLib's intersection curves come) gets a pcurve with one tangent
/// there. Two pieces fitted apart meet at an angle in (u, v) (a tenth of a degree here, over the
/// degree SMLib's check allows on a real thread), sharp where u runs hundreds of times as fast as
/// v.
#[test]
fn a_smooth_curve_through_a_full_knot_gets_a_pcurve_with_a_tangent_there() {
    let (surface, _) = thread_flank();
    let s = SurfaceEval::new(&surface).unwrap();
    // Along the flank as it winds, a third of a turn (u = t), weaving a little across it.
    let [a, b] = [0.40625, 0.4234375];
    let ts: Vec<f64> = (0..=12).map(|i| a + (b - a) * i as f64 / 12.).collect();
    let v = |t: f64| 0.5 + 0.02 * (3. * TAU * (t - a) / (b - a)).sin();
    let on: Vec<[f64; 3]> = ts.iter().map(|&t| s.point([t, v(t)]).unwrap()).map(|p| [p.x, p.y, p.z]).collect();
    let c = crate::bspline::CurveN::interpolate(&ts, &on, 3).unwrap();
    let k = 0.4167;
    let (mut knots, mut ctrl) = (c.knots, c.ctrl.iter().map(|p| crate::bspline::HPoint::weighted(Vec3::from(*p), 1.)).collect::<Vec<_>>());
    for _ in 0..3 {
        (knots, ctrl) = crate::bspline::insert_knot(3, &knots, &ctrl, k).unwrap();
    }
    let weave = Curve::Spline { knots, points: ctrl.iter().map(|h| h.project().unwrap()).map(|p| [p.x, p.y, p.z]).collect() };
    let pc = &pcurves(&surface, &one(&weave, [a, b]), 1e-3).unwrap()[0][0];
    assert!(lift_error(&surface, &weave, [a, b], pc) <= 1e-3);
    let h = 1e-7;
    let (l, m, r) = (pc.eval(k - h).unwrap(), pc.eval(k).unwrap(), pc.eval(k + h).unwrap());
    let (dl, dr) = ([m[0] - l[0], m[1] - l[1]], [r[0] - m[0], r[1] - m[1]]);
    let cos = (dl[0] * dr[0] + dl[1] * dr[1]) / (dl[0].hypot(dl[1]) * dr[0].hypot(dr[1]));
    assert!(cos > 1. - 1e-6, "the pcurve turns by {} degrees at the knot", cos.min(1.).acos().to_degrees());
}

/// Hostile records give errors, never a panic.
#[test]
fn hostile_records_give_errors() {
    let plane = Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] };
    let line = Curve::Line { start: [0.; 3], end: [1., 0., 0.] };
    for tol in [0., -1., f64::NAN, f64::INFINITY, 1e9] {
        assert!(matches!(pcurves(&plane, &one(&line, [0., 1.]), tol), Err(Error::Invalid(_))), "{tol}");
    }
    for interval in [[1., 1.], [2., 1.], [f64::NAN, 1.], [0., f64::INFINITY], [0., 1e13]] {
        assert!(matches!(pcurves(&plane, &one(&line, interval), 1e-6), Err(Error::Invalid(_))), "{interval:?}");
    }
    let nan = Curve::Line { start: [f64::NAN, 0., 0.], end: [1., 0., 0.] };
    assert!(matches!(pcurves(&plane, &one(&nan, [0., 1.]), 1e-6), Err(Error::Invalid(_))));
    let skew = Surface::Cylinder { frame: frame([0.; 3], [1., 0., 0.], [0.5, 1., 0.]), radius: 1. };
    assert!(matches!(pcurves(&skew, &one(&line, [0., 1.]), 1e-6), Err(Error::Invalid(_))));
    let flat = Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [2., 0., 0.] };
    assert!(matches!(pcurves(&flat, &one(&line, [0., 1.]), 1e-6), Err(Error::Invalid(_))));
    let collapsed = Curve::Circle { frame: about_z([0.; 3], false), radius: 0. };
    assert!(matches!(pcurves(&plane, &one(&collapsed, [0., 1.]), 1e-6), Err(Error::Invalid(_))));
    // Too many uses, refused before any work.
    let many = vec![vec![EdgeUse { edge: 0, curve: &line, interval: [0., 1.], forward: true, tolerance: 0.0 }; MAX_USES + 1]];
    assert!(matches!(pcurves(&plane, &many, 1e-6), Err(Error::TooLarge(_))));
    // A polyline of many points on a curved surface: too many spans for one pcurve.
    let cylinder = Surface::Cylinder { frame: about_z([0.; 3], false), radius: 1. };
    let n = 5000;
    let zigzag = Curve::Polyline { points: (0..n).map(|i| [(i as f64 * 0.01).cos(), (i as f64 * 0.01).sin(), (i % 2) as f64]).collect() };
    assert!(matches!(pcurves(&cylinder, &one(&zigzag, [0., (n - 1) as f64]), 1e-6), Err(Error::TooLarge(_))));
    // A degree the evaluators do not take.
    let high = Curve::Spline { knots: [vec![0.; 10], vec![1.; 10]].concat(), points: (0..10).map(|i| [i as f64, 0., 0.]).collect() };
    assert!(matches!(pcurves(&cylinder, &one(&high, [0., 1.]), 1e-6), Err(Error::Failed(_))));
    // Off the surface, and a curve on two tangent surfaces (no single point).
    assert!(matches!(pcurves(&cylinder, &one(&line, [0., 1.]), 1e-6), Err(Error::Failed(_))));
    let touching = Curve::SurfaceIntersection {
        surfaces: [Box::new(cylinder.clone()), Box::new(Surface::Plane { origin: [1., 0., 0.], u_axis: [0., 1., 0.], v_axis: [0., 0., 1.] })],
        leader: Box::new(Curve::Polyline { points: vec![[1., 0., 0.], [1., 0., 1.]] }),
        tolerance: 1e-3,
    };
    assert!(pcurves(&cylinder, &one(&touching, [0., 1.]), 1e-6).is_err());
    // Extreme but valid: a torus as wide as the schema allows and thin as it allows.
    let torus = Surface::Torus { frame: about_z([0.; 3], false), major_radius: 1e12, minor_radius: 1e-12 };
    let far = Curve::Circle { frame: about_z([0.; 3], false), radius: 1e12 };
    let _ = pcurves(&torus, &one(&far, [0., 1.]), 1e-6);
}

/// An edge that lies up to 7.5e-5 mm off its plane (another kernel's approximation) gets no
/// pcurve at the face's 1e-5 mm, and one within its own record tolerance of 2e-4 mm when the
/// use carries it.
#[test]
fn an_edge_is_fitted_to_its_own_tolerance() {
    let plane = Surface::PlaneThrough { origin: [1., 2., 3.], u_point: [3., 2., 3.], v_point: [1., 2., 7.] };
    let off =
        Curve::Spline { knots: vec![0., 0., 0., 0., 1., 1., 1., 1.], points: vec![[1., 2., 3.], [2., 2.0001, 4.], [3., 2.0001, 5.], [4., 2., 6.]] };
    assert!(pcurves(&plane, &one(&off, [0., 1.]), 1e-5).is_err());
    let own = vec![vec![EdgeUse { edge: 0, curve: &off, interval: [0., 1.], forward: true, tolerance: 2e-4 }]];
    let pc = &pcurves(&plane, &own, 1e-5).unwrap()[0][0];
    assert!(lift_error(&plane, &off, [0., 1.], pc) <= 2e-4);
}
