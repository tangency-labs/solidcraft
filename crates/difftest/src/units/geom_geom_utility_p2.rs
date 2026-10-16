//! Runner for unit `geom.geom_utility_p2`: the `smgu_*` distance, intersection,
//! best-fit, 4x4 matrix, Frenet and continuity functions of `SmGeomUtility.cpp`
//! owned by the unit.
//!
//! C++: `solidcore_ffi::units::geom_geom_utility_p2`. Rust:
//! `solidcore::geom::geom_utility::p2`.
//!
//! Every case is one function `op` on the flat input array `x` (layouts in
//! [`ffi::Op`]) with integer argument `n` (Frenet order, continuity third-derivative
//! flags). Failures are normalised to `status_<SmStatus>` on both sides
//! (`SM_ERR` = 1001, `SM_ERR_INVALID_INPUT` = 1007). Entries upstream leaves unset
//! are never compared: hit arrays are cut to `count`, and the distance of a
//! degenerate-segment `segment_segment_closest_point` is not compared (upstream
//! leaves it unset). Every run uses at least [`MIN_GENERATED`] generated inputs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::geom_utility::p2 as gu;
use solidcore::geom::vector3d::Vec3;
use solidcore::SolidError;
use solidcore_ffi::units::geom_geom_utility_p2::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3200;

/// Upstream `SM_ERR`.
const SM_ERR: i32 = 1001;

/// `geom.geom_utility_p2` runner.
pub struct Runner;

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 16] = [
    ("triangle_point_distance", Op::TrianglePointDistance),
    ("line_line_at_distance", Op::LineLineAtDistance),
    ("is_point_on_segment", Op::IsPointOnSegment),
    ("segment_segment_projected", Op::SegSegProjected),
    ("segment_segment_perspective", Op::SegSegPerspective),
    ("segment_segment_intersect", Op::SegSegIntersect),
    ("segment_segment_closest", Op::SegSegClosest),
    ("circle_plane", Op::CirclePlane),
    ("line_ellipse", Op::LineEllipse),
    ("sphere_circles", Op::SphereCircles),
    ("two_planes", Op::TwoPlanes),
    ("best_fit_plane", Op::BestFitPlane),
    ("mult_4x4", Op::Mult4x4),
    ("inverse_4x4", Op::Inverse4x4),
    ("frenet", Op::Frenet),
    ("continuity", Op::Continuity),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    x: Vec<f64>,
    #[serde(default)]
    n: i64,
}

fn lookup(name: &str) -> Result<Op, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, op)| *op)
        .ok_or_else(|| HarnessError::new(format!("unknown op {name}")))
}

fn status(code: i32) -> Output {
    Output::err(format!("status_{code}"))
}

fn normalise(e: &SolidError) -> String {
    match e {
        SolidError::InvalidInput { .. } => "status_1007".into(),
        SolidError::Upstream { code, .. } => format!("status_{code}"),
        other => format!("rust_error: {other}"),
    }
}

/// Flat input accessor.
struct In<'a>(&'a [f64]);

impl In<'_> {
    fn f(&self, i: usize) -> f64 {
        self.0.get(i).copied().unwrap_or(f64::NAN)
    }
    fn v(&self, i: usize) -> Vec3 {
        Vec3::new(self.f(i), self.f(i + 1), self.f(i + 2))
    }
    fn m4(&self, off: usize) -> [[f64; 4]; 4] {
        let mut m = [[0.0; 4]; 4];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, x) in row.iter_mut().enumerate() {
                *x = self.f(off + 4 * r + c);
            }
        }
        m
    }
}

fn flat(pts: &[Vec3]) -> Vec<f64> {
    pts.iter().flat_map(|p| p.to_array()).collect()
}

fn take(v: &[f64], from: usize, n: usize) -> Vec<f64> {
    v.iter().skip(from).take(n).copied().collect()
}

/// Hits output: `count`, then the first `count` entries of each array.
fn hits_out(count: usize, p1: &[f64], p2: &[f64], d: Option<&[f64]>) -> Output {
    let o = Output::new()
        .int("count", count as i64)
        .values("params1", take(p1, 0, count))
        .values("params2", take(p2, 0, count));
    match d {
        Some(d) => o.values("distances", take(d, 0, count)),
        None => o,
    }
}

/// `segment_segment_closest_point` takes the degenerate branch (distance unset
/// upstream).
fn closest_degenerate(x: &In<'_>) -> bool {
    (x.v(3) - x.v(0)).length_squared() < solidcore::math::EFF_ZERO_SQ
        || (x.v(9) - x.v(6)).length_squared() < solidcore::math::EFF_ZERO_SQ
}

/// `intersect_two_planes` returns `SM_ERR` for parallel planes (the case with a
/// meaningful coincidence flag) or when the three-plane intersection fails.
fn planes_parallel(x: &In<'_>) -> bool {
    x.v(3).cross(x.v(9)).length_squared() < solidcore::math::EFF_ZERO_SQ
}

fn count_of(v: &[f64]) -> usize {
    v.first().map_or(0, |&c| c as usize)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    let r = ffi::op(op, &i.x, i.n).map_err(|e| HarnessError::new(e.to_string()))?;
    let x = In(&i.x);
    let v = &r.values;
    if op == Op::TwoPlanes && r.status == SM_ERR && planes_parallel(&x) {
        let coincident = v.first().copied().unwrap_or(-1.0);
        return Ok(Output::new()
            .int("parallel", 1)
            .int("coincident", coincident as i64));
    }
    if r.status != 0 {
        return Ok(status(r.status));
    }
    Ok(match op {
        Op::TrianglePointDistance => Output::new()
            .values("dist", take(v, 0, 1))
            .values("closest", take(v, 1, 3))
            .values("uv", take(v, 4, 2)),
        Op::LineLineAtDistance => Output::new()
            .values("params1", take(v, 0, 4))
            .values("params2", take(v, 4, 4)),
        Op::IsPointOnSegment => Output::new()
            .int("on", v.first().copied().unwrap_or(-1.0) as i64)
            .values("dist_param", take(v, 1, 2)),
        Op::SegSegProjected | Op::SegSegPerspective | Op::SegSegClosest => {
            let n = count_of(v);
            let d = take(v, 5, 2);
            let skip_d = op == Op::SegSegClosest && closest_degenerate(&x);
            hits_out(
                n,
                &take(v, 1, 2),
                &take(v, 3, 2),
                (!skip_d).then_some(d.as_slice()),
            )
        }
        Op::SegSegIntersect | Op::CirclePlane | Op::SphereCircles => {
            let n = count_of(v);
            let pts = if n <= 2 { 3 * n } else { 0 };
            Output::new()
                .int("count", n as i64)
                .values("points", take(v, 1, pts))
        }
        Op::LineEllipse => {
            let n = count_of(v);
            Output::new()
                .int("count", n as i64)
                .values("params", take(v, 1, n.min(2)))
        }
        Op::TwoPlanes => Output::new()
            .int("parallel", 0)
            .values("line", take(v, 1, 6)),
        Op::BestFitPlane => Output::new()
            .int("type", v.first().copied().unwrap_or(-1.0) as i64)
            .values("plane", take(v, 1, 8)),
        Op::Mult4x4 | Op::Inverse4x4 => Output::new().values("m", take(v, 0, 16)),
        Op::Frenet => Output::new().values("frame", v.clone()),
        Op::Continuity => Output::new().int("type", v.first().copied().unwrap_or(-1.0) as i64),
    })
}

fn rust_run(op: Op, i: &Inputs) -> solidcore::Result<Output> {
    let x = In(&i.x);
    Ok(match op {
        Op::TrianglePointDistance => {
            let t = gu::triangle_point_distance(x.v(0), x.v(3), x.v(6), x.v(9))?;
            Output::new()
                .values("dist", vec![t.distance])
                .values("closest", t.closest.to_array().to_vec())
                .values("uv", vec![t.u, t.v])
        }
        Op::LineLineAtDistance => {
            let (a, b) = gu::line_line_at_distance(x.v(0), x.v(3), x.v(6), x.v(9), x.f(12))?;
            Output::new()
                .values("params1", a.to_vec())
                .values("params2", b.to_vec())
        }
        Op::IsPointOnSegment => {
            let r = gu::is_point_on_segment(x.v(0), x.v(3), x.v(6), x.f(9))?;
            Output::new()
                .int("on", i64::from(r.on))
                .values("dist_param", vec![r.distance, r.parameter])
        }
        Op::SegSegProjected | Op::SegSegPerspective | Op::SegSegClosest => {
            let h = match op {
                Op::SegSegProjected => gu::segment_segment_projected_intersect(
                    x.v(0),
                    x.v(3),
                    x.v(6),
                    x.v(9),
                    x.v(12),
                    x.f(15),
                )?,
                Op::SegSegPerspective => gu::segment_segment_perspective_intersect(
                    x.v(0),
                    x.v(3),
                    x.v(6),
                    x.v(9),
                    &gu::PerspectivePlane {
                        point: x.v(12),
                        unit_normal: x.v(15),
                        eye: x.v(18),
                    },
                    x.f(21),
                )?,
                _ => gu::segment_segment_closest_point(x.v(0), x.v(3), x.v(6), x.v(9), x.f(12))?,
            };
            let skip_d = op == Op::SegSegClosest && closest_degenerate(&x);
            hits_out(
                h.count,
                &h.params1,
                &h.params2,
                (!skip_d).then_some(h.distances.as_slice()),
            )
        }
        Op::SegSegIntersect => {
            let p = gu::segment_segment_intersect(x.v(0), x.v(3), x.v(6), x.v(9), x.f(12))?;
            Output::new()
                .int("count", p.len() as i64)
                .values("points", flat(&p))
        }
        Op::CirclePlane | Op::SphereCircles => {
            let r = if op == Op::CirclePlane {
                gu::circle_plane_intersect(x.f(0), x.f(1), x.v(2), x.v(5), x.v(8), x.v(11))?
            } else {
                gu::sphere_circle_sphere_circle_intersect(
                    x.v(0),
                    x.f(3),
                    x.v(4),
                    x.v(7),
                    x.v(10),
                    x.v(13),
                    x.f(16),
                )?
            };
            match r {
                gu::CurveIntersection::Points(p) => Output::new()
                    .int("count", p.len() as i64)
                    .values("points", flat(&p)),
                gu::CurveIntersection::Coincident => {
                    Output::new().int("count", 3).values("points", Vec::new())
                }
            }
        }
        Op::LineEllipse => {
            let t = gu::line_coplanar_ellipse_intersect(
                x.v(0),
                x.v(3),
                &gu::CoplanarEllipse {
                    center: x.v(6),
                    x_axis: x.v(9),
                    x_radius: x.f(15),
                    y_axis: x.v(12),
                    y_radius: x.f(16),
                },
                x.f(17),
            )?;
            Output::new()
                .int("count", t.len() as i64)
                .values("params", t)
        }
        Op::TwoPlanes => match gu::intersect_two_planes(x.v(0), x.v(3), x.v(6), x.v(9))? {
            gu::PlanesIntersection::Line { point, direction } => Output::new()
                .int("parallel", 0)
                .values("line", flat(&[point, direction])),
            gu::PlanesIntersection::Parallel { coincident } => Output::new()
                .int("parallel", 1)
                .int("coincident", i64::from(coincident)),
        },
        Op::BestFitPlane => {
            let n = (i.x.len().saturating_sub(1)) / 3;
            let pts: Vec<Vec3> = (0..n).map(|k| x.v(3 * k)).collect();
            let f = gu::best_fit_plane_to_points(&pts, x.f(3 * n))?;
            let mut plane = flat(&[f.point, f.normal]);
            plane.extend([f.max_plane_gap, f.max_centroid_gap]);
            Output::new()
                .int("type", f.set_type as i64)
                .values("plane", plane)
        }
        Op::Mult4x4 => {
            let m = gu::mult_4x4(&x.m4(0), &x.m4(16));
            Output::new().values("m", m.iter().flatten().copied().collect::<Vec<_>>())
        }
        Op::Inverse4x4 => {
            let m = gu::inverse_4x4(&x.m4(0))?;
            Output::new().values("m", m.iter().flatten().copied().collect::<Vec<_>>())
        }
        Op::Frenet => {
            let vals = [x.v(0), x.v(3), x.v(6), x.v(9)];
            let n = usize::try_from(i.n).unwrap_or(usize::MAX);
            let f = gu::frenet_from_derivatives(&vals, n)?;
            Output::new().values("frame", flat(&f))
        }
        Op::Continuity => {
            let e1 = gu::CurveEval {
                position: x.v(0),
                first: x.v(3),
                second: x.v(6),
                third: (i.n & 1 != 0).then(|| x.v(19)),
            };
            let e2 = gu::CurveEval {
                position: x.v(9),
                first: x.v(12),
                second: x.v(15),
                third: (i.n & 2 != 0).then(|| x.v(22)),
            };
            let t = gu::evaluate_curve_continuity(&e1, &e2, x.f(18))?;
            Output::new().int("type", t as i64)
        }
    })
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    match rust_run(op, i) {
        Ok(o) => Ok(o),
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(normalise(&e)))),
    }
}

// ---- generation ---------------------------------------------------------------

/// A random element of `items`.
fn pick<T: Copy + Default>(rng: &mut Rng, items: &[T]) -> T {
    rng.choose(items).copied().unwrap_or_default()
}

fn coord(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 | 2 => rng.range_i64(-3, 3) as f64,
        3 => rng.uniform(-1000.0, 1000.0),
        _ => rng.uniform(-10.0, 10.0),
    }
}

fn point(rng: &mut Rng) -> Vec3 {
    Vec3::new(coord(rng), coord(rng), coord(rng))
}

/// A unit vector: random, axis-aligned, or nearly axis-aligned.
fn unit(rng: &mut Rng) -> Vec3 {
    let v = match rng.below(6) {
        0 => pick(rng, &[Vec3::X, Vec3::Y, Vec3::Z]) * if rng.chance(0.5) { 1.0 } else { -1.0 },
        1 => Vec3::new(1.0, rng.uniform(-1e-7, 1e-7), rng.uniform(-1e-7, 1e-7)),
        _ => Vec3::new(
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        ),
    };
    v.unitize().unwrap_or(Vec3::Z)
}

/// A tolerance: typical, tiny, zero or large.
fn tol(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => 0.0,
        1 => 1e-10,
        2 => 0.5,
        3 => rng.uniform(0.01, 2.0),
        _ => 1e-6,
    }
}

/// Small perturbation of `p` (within or just beyond typical tolerances).
fn near(rng: &mut Rng, p: Vec3) -> Vec3 {
    let s = pick(rng, &[0.0, 1e-13, 1e-9, 1e-7, 1e-3]);
    p + Vec3::new(rng.uniform(-s, s), rng.uniform(-s, s), rng.uniform(-s, s))
}

/// Two segments in one of several relative configurations.
fn segments(rng: &mut Rng) -> [Vec3; 4] {
    let s1 = point(rng);
    let e1 = if rng.chance(0.1) {
        near(rng, s1)
    } else {
        point(rng)
    };
    let d = e1 - s1;
    match rng.below(8) {
        0 => {
            // collinear, overlapping or touching
            let a = rng.uniform(-0.5, 1.5);
            let r = rng.uniform(-0.5, 1.5);
            let b = pick(rng, &[r, 1.0, 0.0]);
            [s1, e1, near(rng, s1 + a * d), near(rng, s1 + b * d)]
        }
        1 => {
            // parallel offset
            let off = point(rng) * pick(rng, &[1e-8, 1e-3, 0.1]);
            [s1, e1, s1 + off, e1 + off]
        }
        2 => {
            // crossing through an interior point
            let c = s1 + rng.uniform(0.0, 1.0) * d;
            let w = point(rng);
            [
                s1,
                e1,
                near(rng, c - 0.5 * w),
                c + rng.uniform(0.1, 2.0) * w,
            ]
        }
        3 => {
            // end touching
            [s1, e1, near(rng, e1), point(rng)]
        }
        4 => {
            // degenerate second segment
            let p = point(rng);
            [s1, e1, p, near(rng, p)]
        }
        _ => [s1, e1, point(rng), point(rng)],
    }
}

fn push(x: &mut Vec<f64>, vs: &[Vec3]) {
    for v in vs {
        x.extend(v.to_array());
    }
}

fn matrix4(rng: &mut Rng) -> Vec<f64> {
    let mut m: Vec<f64> = (0..16).map(|_| coord(rng)).collect();
    match rng.below(6) {
        0 => {
            // affine transform (last row 0 0 0 1)
            for (k, x) in m.iter_mut().enumerate().skip(12) {
                *x = if k == 15 { 1.0 } else { 0.0 };
            }
        }
        1 => {
            // singular: duplicate a row
            let (a, b) = (rng.below(4) as usize, rng.below(4) as usize);
            let row: Vec<f64> = m.iter().skip(4 * a).take(4).copied().collect();
            for (x, r) in m.iter_mut().skip(4 * b).zip(row) {
                *x = r;
            }
        }
        2 => {
            // near-singular determinant around SM_EFF_ZERO
            let s = pick(rng, &[1e-3, 1e-4, 1e-5]);
            for (k, x) in m.iter_mut().enumerate() {
                *x = if k % 5 == 0 { s } else { 0.0 };
            }
        }
        3 => {
            for (k, x) in m.iter_mut().enumerate() {
                *x = if k % 5 == 0 { 1.0 } else { 0.0 };
            }
        }
        _ => {}
    }
    m
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let &(name, op) = rng.choose(&OPS).unwrap_or(&("frenet", Op::Frenet));
    let mut x = Vec::new();
    let mut n = 0;
    match op {
        Op::TrianglePointDistance => {
            let p0 = point(rng);
            let p1 = point(rng);
            let p2 = match rng.below(5) {
                0 => {
                    let q = p0 + rng.uniform(-2.0, 2.0) * (p1 - p0);
                    near(rng, q)
                }
                1 => near(rng, p1),
                _ => point(rng),
            };
            let t = if rng.chance(0.3) {
                p0 + rng.uniform(-0.5, 1.0) * (p1 - p0) + rng.uniform(-0.5, 1.0) * (p2 - p0)
            } else {
                point(rng)
            };
            push(&mut x, &[p0, p1, p2, t]);
        }
        Op::LineLineAtDistance => {
            let [s1, e1, s2, e2] = segments(rng);
            push(&mut x, &[s1, e1 - s1, s2, e2 - s2]);
            x.push(match rng.below(3) {
                0 => rng.uniform(0.0, 1.0),
                _ => rng.uniform(0.0, 50.0),
            });
        }
        Op::IsPointOnSegment => {
            let s = point(rng);
            let e = if rng.chance(0.1) { s } else { point(rng) };
            let t = if rng.chance(0.5) {
                let q = s + rng.uniform(-0.2, 1.2) * (e - s);
                near(rng, q)
            } else {
                point(rng)
            };
            push(&mut x, &[s, e, t]);
            x.push(tol(rng));
        }
        Op::SegSegProjected => {
            push(&mut x, &segments(rng));
            let nrm = if rng.chance(0.8) {
                unit(rng)
            } else {
                point(rng)
            };
            push(&mut x, &[nrm]);
            x.push(tol(rng));
        }
        Op::SegSegPerspective => {
            push(&mut x, &segments(rng));
            let pp = point(rng);
            let nrm = unit(rng);
            let eye = pp + rng.uniform(1.0, 50.0) * nrm + point(rng) * 0.1;
            push(&mut x, &[pp, nrm, eye]);
            x.push(tol(rng));
        }
        Op::SegSegIntersect | Op::SegSegClosest => {
            push(&mut x, &segments(rng));
            x.push(tol(rng));
        }
        Op::CirclePlane => {
            x.push(tol(rng));
            let r = rng.uniform(0.1, 10.0);
            x.push(pick(rng, &[r, 0.0, 1.0]));
            let o = point(rng);
            let cn = unit(rng);
            let pn = match rng.below(4) {
                0 => cn,
                1 => (cn + Vec3::new(1e-7, 0.0, 0.0)).unitize().unwrap_or(cn),
                _ => unit(rng),
            };
            let pp = if rng.chance(0.4) {
                o + rng.uniform(-2.0, 2.0) * pn
            } else {
                point(rng)
            };
            push(&mut x, &[o, cn, pp, pn]);
        }
        Op::LineEllipse => {
            let c = point(rng);
            let xa = unit(rng);
            let ya = xa.normal_in_plane(unit(rng)).unitize().unwrap_or(Vec3::Y);
            let rx = rng.uniform(0.1, 10.0);
            let ry = if rng.chance(0.2) {
                rx
            } else {
                rng.uniform(0.1, 10.0)
            };
            let lp = c + rng.uniform(-12.0, 12.0) * xa + rng.uniform(-12.0, 12.0) * ya;
            let lv = rng.uniform(-3.0, 3.0) * xa + rng.uniform(-3.0, 3.0) * ya;
            push(&mut x, &[lp, lv, c, xa, ya]);
            x.extend([rx, ry, tol(rng)]);
        }
        Op::SphereCircles => {
            let c = point(rng);
            let r = rng.uniform(0.5, 10.0);
            let n1 = unit(rng);
            let n2 = match rng.below(5) {
                0 => n1,
                1 => -n1,
                _ => unit(rng),
            };
            let d1 = rng.uniform(-r, r);
            let d2 = if rng.chance(0.15) {
                d1
            } else {
                rng.uniform(-r, r)
            };
            push(&mut x, &[c]);
            x.push(r);
            push(&mut x, &[c + d1 * n1, n1, c + d2 * n2, n2]);
            x.push(tol(rng));
        }
        Op::TwoPlanes => {
            let p1 = point(rng);
            let v1 = if rng.chance(0.05) {
                point(rng)
            } else {
                unit(rng)
            };
            let v2 = match rng.below(5) {
                0 => v1,
                1 => -v1,
                _ => unit(rng),
            };
            let p2 = match rng.below(3) {
                0 => p1 + point(rng).normal_in_plane(v1),
                _ => point(rng),
            };
            push(&mut x, &[p1, v1, p2, v2]);
        }
        Op::BestFitPlane => {
            let k = rng.below(12) as usize;
            let c = point(rng);
            let a = point(rng);
            let b = point(rng);
            let mode = rng.below(4);
            for _ in 0..k {
                let p = match mode {
                    0 => near(rng, c),
                    1 => {
                        let q = c + rng.uniform(-1.0, 1.0) * a;
                        near(rng, q)
                    }
                    2 => {
                        let q = c + rng.uniform(-1.0, 1.0) * a + rng.uniform(-1.0, 1.0) * b;
                        near(rng, q)
                    }
                    _ => point(rng),
                };
                push(&mut x, &[p]);
            }
            x.push(tol(rng));
        }
        Op::Mult4x4 => {
            x = matrix4(rng);
            x.extend(matrix4(rng));
        }
        Op::Inverse4x4 => x = matrix4(rng),
        Op::Frenet => {
            n = rng.below(4) as i64;
            let d1 = if rng.chance(0.1) {
                Vec3::ZERO
            } else {
                point(rng)
            };
            let d2 = match rng.below(6) {
                0 => Vec3::ZERO,
                1 => d1 * rng.uniform(-2.0, 2.0),
                _ => point(rng),
            };
            push(&mut x, &[point(rng), d1, d2, point(rng)]);
        }
        Op::Continuity => {
            let p = point(rng);
            let v = if rng.chance(0.05) {
                Vec3::ZERO
            } else {
                point(rng)
            };
            let s = point(rng);
            let t = point(rng);
            let p2 = match rng.below(4) {
                0 => p + Vec3::new(1e-3, 0.0, 0.0),
                1 => near(rng, p),
                _ => p,
            };
            let v2 = match rng.below(6) {
                0 => v * rng.uniform(0.5, 2.0),
                1 => near(rng, v),
                2 => point(rng),
                _ => v,
            };
            let s2 = match rng.below(5) {
                0 => -s,
                1 => near(rng, s),
                2 => point(rng),
                _ => s,
            };
            let t2 = match rng.below(4) {
                0 => point(rng),
                1 => near(rng, t),
                _ => t,
            };
            push(&mut x, &[p, v, s, p2, v2, s2]);
            x.push(pick(rng, &[1.0, 0.5, 5.0, 0.0]));
            push(&mut x, &[t, t2]);
            n = rng.below(4) as i64;
        }
    }
    Inputs {
        op: name.to_string(),
        x,
        n,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.geom_utility_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
