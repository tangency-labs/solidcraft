//! Runner for unit `geom.geom_utility_p1`: the first part of the free geometric
//! utilities of `SmGeomUtility.cpp` (polygon / triangle measures, line, plane, circle
//! and sphere closest points and intersections, best-fit point and line, small
//! matrices, directional derivatives, sorting helpers).
//!
//! C++: `solidcore_ffi::units::geom_geom_utility_p1` (shims
//! `harness/cpp/shim/geom_geom_utility_p1.cpp` and `..._static.cpp`).
//! Rust: `solidcore::geom::geom_utility::p1`.
//!
//! Every case names one operation `op` and gives its arguments as the flat list `x`
//! (layouts in the shim and in [`solidcore_ffi::units::geom_geom_utility_p1::Op`]);
//! ops taking a point list read it from `pts` (appended to `x`, so fixtures can use
//! `{"$pts": ...}`). The result `r` is `[status, values...]` with the upstream
//! `SmStatus` (`SM_SUCCESS` = 1000, `SM_ERR` = 1001, `SM_ERR_INVALID_INPUT` = 1007,
//! `SM_ERR_WARNING` = 1023, `FALSE` = 0) and booleans as `1.0`/`0.0`; on a failing
//! status only the status is compared. `add_point` / `add_closest_point` (the static
//! helpers) report `[status, state...]` whatever the status.
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p1 as gu;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::SolidError;
use solidcore_ffi::units::geom_geom_utility_p1::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 4000;

const SM_SUCCESS: f64 = 1000.0;

/// `geom.geom_utility_p1` runner.
pub struct Runner;

/// Operation names used in `inputs.op` (besides `add_point`, `add_closest_point`).
const OPS: [(&str, Op); 43] = [
    ("polygon_area", Op::PolygonArea),
    ("triangle_area_squared", Op::TriangleAreaSquared),
    ("triangle_decomposition", Op::TriangleDecomposition),
    ("parallelogram", Op::Parallelogram),
    ("line_point_distance", Op::LinePointDistance),
    ("line_line_closest_point", Op::LineLineClosestPoint),
    ("line_plane", Op::LinePlane),
    ("line_plane_uv", Op::LinePlaneUv),
    ("line_closest_point", Op::LineClosestPoint),
    ("segment_point_distance", Op::SegmentPointDistance),
    ("segment_angle", Op::SegmentAngle),
    ("circle_center_3_points", Op::CircleCenter3Points),
    ("circle_3_points", Op::Circle3Points),
    ("silhouette", Op::Silhouette),
    ("cones_disjoint", Op::ConesDisjoint),
    ("line_circle", Op::LineCircle),
    ("circle_circle", Op::CircleCircle),
    ("line_sphere", Op::LineSphere),
    ("three_planes", Op::ThreePlanes),
    ("plane_eval", Op::PlaneEval),
    ("plane_drop", Op::PlaneDrop),
    ("plane_point_distance", Op::PlanePointDistance),
    ("point_project", Op::PointProject),
    ("vector_project", Op::VectorProject),
    ("start_plane", Op::StartPlane),
    ("perspective", Op::Perspective),
    ("vec_lin_comb", Op::VecLinComb),
    ("best_fit_point", Op::BestFitPoint),
    ("best_fit_line", Op::BestFitLine),
    ("triple_product", Op::TripleProduct),
    ("copy_3x3", Op::Copy3x3),
    ("det_3x3", Op::Det3x3),
    ("det_3_vectors", Op::Det3Vectors),
    ("inverse_3x3", Op::Inverse3x3),
    ("copy_4x4", Op::Copy4x4),
    ("det_4x4", Op::Det4x4),
    ("quadratic", Op::Quadratic),
    ("dir_derivs", Op::DirDerivs),
    ("vol_dir_derivs", Op::VolDirDerivs),
    ("dog_leg", Op::DogLeg),
    ("add_sorted", Op::AddSorted),
    ("shell_sort", Op::ShellSort),
    ("double_less", Op::DoubleLess),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    x: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pts: Vec<[f64; 3]>,
}

impl Inputs {
    fn args(&self) -> Vec<f64> {
        let mut a = self.x.clone();
        a.extend(self.pts.iter().flatten());
        a
    }
}

fn lookup(name: &str) -> Result<Op, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, op)| *op)
        .ok_or_else(|| HarnessError::new(format!("unknown op {name}")))
}

/// Upstream status of a Rust error, or `None` for errors with no upstream status.
fn status_code(e: &SolidError) -> Option<f64> {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => Some(1007.0),
        SolidError::Upstream { code, .. } => Some(f64::from(*code)),
        _ => None,
    }
}

/// Sequential reader over the flat argument list.
struct Args<'a> {
    a: &'a [f64],
    i: usize,
}

impl<'a> Args<'a> {
    fn new(a: &'a [f64]) -> Self {
        Self { a, i: 0 }
    }

    fn f(&mut self) -> Result<f64, HarnessError> {
        let v = self
            .a
            .get(self.i)
            .copied()
            .ok_or_else(|| HarnessError::new("too few arguments"))?;
        self.i += 1;
        Ok(v)
    }

    fn v3(&mut self) -> Result<Vec3, HarnessError> {
        Ok(Vec3::new(self.f()?, self.f()?, self.f()?))
    }

    fn v2(&mut self) -> Result<Vec2, HarnessError> {
        Ok(Vec2::new(self.f()?, self.f()?))
    }

    fn arr3(&mut self) -> Result<[f64; 3], HarnessError> {
        Ok([self.f()?, self.f()?, self.f()?])
    }

    fn rest(&self) -> &'a [f64] {
        self.a.get(self.i..).unwrap_or(&[])
    }

    fn points(&self) -> Vec<Vec3> {
        self.rest()
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[x, y, z]| Vec3::new(x, y, z))
            .collect()
    }

    fn count(&mut self) -> Result<usize, HarnessError> {
        let v = self.f()?;
        if v >= 0.0 && v.fract() == 0.0 && v < 1e9 {
            Ok(v as usize)
        } else {
            Err(HarnessError::new(format!("bad count {v}")))
        }
    }
}

fn put3(o: &mut Vec<f64>, v: Vec3) {
    o.extend(v.to_array());
}

/// The Rust result `[status, values...]` (or the error to classify).
fn rust_values(op: Op, args: &[f64]) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let mut a = Args::new(args);
    let mut o = vec![SM_SUCCESS];
    macro_rules! tri {
        ($e:expr) => {
            match $e {
                Ok(v) => v,
                Err(e) => return Ok(Err(e)),
            }
        };
    }
    match op {
        Op::PolygonArea => {
            let n = a.v3()?;
            o.push(tri!(gu::polygon_area(&a.points(), n)));
        }
        Op::TriangleAreaSquared => {
            o.push(gu::triangle_area_squared(a.v3()?, a.v3()?, a.v3()?));
        }
        Op::TriangleDecomposition => {
            let (along, h) = tri!(gu::triangle_decomposition(a.f()?, a.f()?, a.f()?));
            o.extend([along, h]);
        }
        Op::Parallelogram => {
            let (v1, v2) =
                tri!(gu::parallelogram_cross_vectors(a.v3()?, a.v3()?, a.f()?, a.f()?));
            put3(&mut o, v1);
            put3(&mut o, v2);
        }
        Op::LinePointDistance => {
            let (d, t) = gu::line_point_distance(a.v3()?, a.v3()?, a.v3()?);
            o.extend([d, t]);
        }
        Op::LineLineClosestPoint => {
            let (t1, t2) = tri!(gu::line_line_closest_point(a.v3()?, a.v3()?, a.v3()?, a.v3()?));
            o.extend([t1, t2]);
        }
        Op::LinePlane => {
            o.push(tri!(gu::line_plane_intersect(a.v3()?, a.v3()?, a.v3()?, a.v3()?)));
        }
        Op::LinePlaneUv => {
            let (t, u, v) =
                tri!(gu::line_plane_intersect_uv(a.v3()?, a.v3()?, a.v3()?, a.v3()?, a.v3()?));
            o.extend([t, u, v]);
        }
        Op::LineClosestPoint => {
            o.push(gu::line_closest_point(a.v3()?, a.v3()?, a.v3()?));
        }
        Op::SegmentPointDistance => {
            let (d, t) = gu::segment_point_distance(a.v3()?, a.v3()?, a.v3()?);
            o.extend([d, t]);
        }
        Op::SegmentAngle => {
            let (ang, t) = gu::segment_point_angle_deg_to_perp(a.v3()?, a.v3()?, a.v3()?);
            o.extend([ang, t]);
        }
        Op::CircleCenter3Points => {
            let c = tri!(gu::circle_center_from_3_points(a.v2()?, a.v2()?, a.v2()?));
            o.extend([c.x, c.y]);
        }
        Op::Circle3Points => {
            let (sp, mp, ep) = (a.v3()?, a.v3()?, a.v3()?);
            let dim = a.count()? as u64;
            let closed = a.f()? != 0.0;
            let c = tri!(gu::circle_from_3_points(sp, mp, ep, dim, closed));
            put3(&mut o, c.center);
            put3(&mut o, c.x_axis);
            put3(&mut o, c.y_axis);
            o.extend([c.domain.min(), c.domain.max(), c.radius]);
        }
        Op::Silhouette => {
            let (c, n, r) = (a.v3()?, a.v3()?, a.f()?);
            let persp = a.f()? != 0.0;
            match tri!(gu::circle_find_silhouette_points(c, n, r, persp, a.v3()?)) {
                Some((p1, p2)) => {
                    o.push(1.0);
                    put3(&mut o, p1);
                    put3(&mut o, p2);
                }
                None => o.push(0.0),
            }
        }
        Op::ConesDisjoint => {
            o.push(flag(gu::cones_are_disjoint_perpen(a.v3()?, a.f()?, a.v3()?, a.f()?)));
        }
        Op::LineCircle => {
            let t = tri!(gu::line_coplanar_circle_intersect(
                a.v3()?,
                a.v3()?,
                a.v3()?,
                a.v3()?,
                a.f()?,
                a.f()?
            ));
            o.push(t.len() as f64);
            o.extend(t);
        }
        Op::CircleCircle => {
            let r = tri!(gu::circle_coplanar_circle_intersect(
                a.v3()?,
                a.v3()?,
                a.f()?,
                a.v3()?,
                a.f()?,
                a.f()?
            ));
            o.push(f64::from(r.count()));
            match r {
                gu::CircleCircle::Tangent(p) => put3(&mut o, p),
                gu::CircleCircle::Crossing(p, q) => {
                    put3(&mut o, p);
                    put3(&mut o, q);
                }
                gu::CircleCircle::None | gu::CircleCircle::Coincident => {}
            }
        }
        Op::LineSphere => {
            let r = tri!(gu::line_intersect_sphere(a.v3()?, a.v3()?, a.v3()?, a.f()?, a.f()?));
            match r {
                gu::LineSphere::Miss => o.push(0.0),
                gu::LineSphere::Touch(t) => o.extend([1.0, t]),
                gu::LineSphere::Through(t0, t1) => o.extend([2.0, t0, t1]),
                gu::LineSphere::Grazing(t0, t1, t2) => o.extend([3.0, t0, t1, t2]),
            }
        }
        Op::ThreePlanes => {
            let p = tri!(gu::intersect_three_planes(
                a.v3()?,
                a.v3()?,
                a.v3()?,
                a.v3()?,
                a.v3()?,
                a.v3()?
            ));
            put3(&mut o, p);
        }
        Op::PlaneEval => {
            let pl = Axis2Placement::new(a.v3()?, a.v3()?, a.v3()?);
            put3(&mut o, gu::plane_evaluate_point(&pl, a.v2()?));
        }
        Op::PlaneDrop => {
            let pl = Axis2Placement::new(a.v3()?, a.v3()?, a.v3()?);
            let uv = gu::plane_drop_point(&pl, a.v3()?);
            o.extend([uv.x, uv.y]);
        }
        Op::PlanePointDistance => {
            o.push(tri!(gu::plane_point_distance(a.v3()?, a.v3()?, a.v3()?)));
        }
        Op::PointProject => {
            put3(&mut o, gu::point_project_to_plane(a.v3()?, a.v3()?, a.v3()?));
        }
        Op::VectorProject => {
            put3(&mut o, gu::vector_project_to_plane(a.v3()?, a.v3()?));
        }
        Op::StartPlane => {
            let p = a.v3()?;
            let pl = Axis2Placement::new(a.v3()?, a.v3()?, a.v3()?);
            let sweep = Extent1d::from_min_max(a.f()?, a.f()?);
            let tol = a.f()?;
            let snap = a.f()? != 0.0;
            let r = tri!(gu::transform_point_to_start_plane(p, &pl, &sweep, tol, snap));
            put3(&mut o, r.point);
            o.push(r.dist_to_z_axis);
            o.push(r.angles_deg.len() as f64);
            o.extend(r.angles_deg);
            o.push(flag(r.inside));
        }
        Op::Perspective => {
            put3(&mut o, gu::point_perspective_to_plane(a.v3()?, a.v3()?, a.v3()?, a.v3()?));
        }
        Op::VecLinComb => {
            let uv = tri!(gu::vec_lin_comb_two_vectors(a.v3()?, a.v3()?, a.v3()?));
            o.extend([uv.x, uv.y]);
        }
        Op::BestFitPoint => {
            let tol = a.f()?;
            let r = gu::best_fit_point_to_points(&a.points(), tol);
            o.push(f64::from(r.set_type.code()));
            put3(&mut o, r.center);
            o.push(r.max_gap);
        }
        Op::BestFitLine => {
            let tol = a.f()?;
            let r = tri!(gu::best_fit_line_to_points(&a.points(), tol));
            o.push(f64::from(r.set_type.code()));
            put3(&mut o, r.line_pt);
            put3(&mut o, r.line_vec);
            o.extend([r.max_line_gap, r.max_centroid_gap]);
            if let Some(m) = &r.m {
                o.extend_from_slice(m.as_slice());
            }
        }
        Op::TripleProduct => {
            o.push(gu::vec_triple_product(&a.arr3()?, &a.arr3()?, &a.arr3()?));
        }
        Op::Copy3x3 => {
            let m = [a.arr3()?, a.arr3()?, a.arr3()?];
            o.extend(gu::copy_3x3(&m).iter().flatten());
        }
        Op::Det3x3 => {
            o.push(gu::determinate_3x3(
                a.f()?,
                a.f()?,
                a.f()?,
                a.f()?,
                a.f()?,
                a.f()?,
                a.f()?,
                a.f()?,
                a.f()?,
            ));
        }
        Op::Det3Vectors => {
            o.push(gu::determinant_3_vectors(a.v3()?, a.v3()?, a.v3()?));
        }
        Op::Inverse3x3 => {
            let inv = tri!(gu::inverse_3x3(&a.arr3()?, &a.arr3()?, &a.arr3()?));
            o.extend(inv.iter().flatten());
        }
        Op::Copy4x4 | Op::Det4x4 => {
            let mut m = [[0.0; 4]; 4];
            for row in &mut m {
                for c in row.iter_mut() {
                    *c = a.f()?;
                }
            }
            if op == Op::Copy4x4 {
                o.extend(gu::copy_4x4(&m).iter().flatten());
            } else {
                o.push(gu::determinate_4x4(&m));
            }
        }
        Op::Quadratic => {
            let c = a.arr3()?;
            let r = gu::solve_quadratic_eqn(&c, a.f()?);
            o.push(r.len() as f64);
            o.extend_from_slice(&r);
        }
        Op::DirDerivs => {
            let dir = a.v2()?;
            let (hu, hv) = (a.count()?, a.count()?);
            let d = tri!(gu::directional_derivs(dir, hu, hv, &a.points()));
            o.push(d.len() as f64);
            for v in d {
                put3(&mut o, v);
            }
        }
        Op::VolDirDerivs => {
            let dir = a.v3()?;
            let n = a.count()?;
            let d = tri!(gu::vol_directional_derivs(dir, n, &a.points()));
            o.push(d.len() as f64);
            for v in d {
                put3(&mut o, v);
            }
        }
        Op::DogLeg => {
            o.push(flag(gu::is_dog_leg(
                a.v2()?,
                a.v2()?,
                a.v2()?,
                a.v2()?,
                a.f()?,
                a.f()?,
            )));
        }
        Op::AddSorted => {
            let value = a.f()?;
            let idx = a.count()? as u64;
            let incr = a.f()? != 0.0;
            let with_idx = a.f()? != 0.0;
            let n = a.count()?;
            let mut vals = Vec::with_capacity(n + 1);
            for _ in 0..n {
                vals.push(a.f()?);
            }
            let mut idxs = Vec::with_capacity(n + 1);
            for _ in 0..n {
                idxs.push(a.count()? as u64);
            }
            gu::add_sorted(value, idx, incr, &mut vals, with_idx.then_some(&mut idxs));
            o.push(vals.len() as f64);
            o.extend(&vals);
            if with_idx {
                o.extend(idxs.iter().map(|&i| i as f64));
            }
        }
        Op::ShellSort => {
            let mut v = a.rest().to_vec();
            gu::shell_sort_double_array(&mut v);
            o.extend(v);
        }
        Op::DoubleLess => {
            o.push(flag(gu::double_less_than(a.f()?, a.f()?)));
        }
    }
    Ok(Ok(o))
}

/// `[status, values...]` as the integer `status` plus the float list `r` (so that
/// `--perturb` changes a value rather than the status).
fn finish(v: &[f64]) -> Output {
    let (status, r) = match v.split_first() {
        Some((s, r)) => (*s as i64, r.to_vec()),
        None => (-1, Vec::new()),
    };
    Output::new().int("status", status).values("r", r)
}

fn rust_op(op: Op, args: &[f64]) -> Result<Output, HarnessError> {
    match rust_values(op, args)? {
        Ok(v) => Ok(finish(&v)),
        Err(e) => match status_code(&e) {
            Some(s) => Ok(finish(&[s])),
            None => Ok(Output::unported(&e)
                .unwrap_or_else(|| Output::err(format!("rust_error: {e}")))),
        },
    }
}

fn cpp_op(op: Op, args: &[f64]) -> Result<Output, HarnessError> {
    match ffi::call(op, args) {
        Ok(v) => Ok(finish(&v)),
        Err(FfiError::Status(s)) if s > 0 => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

/// `[n, p0, p1]` / `[n, params1, params2, dists]` state of the static helpers.
fn state7(a: &mut Args<'_>) -> Result<[f64; 7], HarnessError> {
    let mut s = [0.0; 7];
    for v in &mut s {
        *v = a.f()?;
    }
    Ok(s)
}

fn static_cpp(name: &str, args: &[f64]) -> Result<Output, HarnessError> {
    let mut a = Args::new(args);
    let state = state7(&mut a)?;
    let r = if name == "add_point" {
        let tol = a.f()?;
        ffi::add_point(state, tol, a.arr3()?)
    } else {
        ffi::add_closest_point(state, a.f()?, a.f()?, a.f()?, a.f()?)
    };
    match r {
        Ok((st, s)) => {
            let mut v = vec![f64::from(st)];
            v.extend(s);
            Ok(finish(&v))
        }
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

fn static_rust(name: &str, args: &[f64]) -> Result<Output, HarnessError> {
    let mut a = Args::new(args);
    let state = state7(&mut a)?;
    let [n, s1, s2, s3, s4, s5, s6] = state;
    let n = if (0.0..=2.0).contains(&n) && n.fract() == 0.0 {
        n as usize
    } else {
        return Err(HarnessError::new("bad count"));
    };
    let mut v = Vec::with_capacity(8);
    if name == "add_point" {
        let tol = a.f()?;
        let pnt = a.v3()?;
        let all = [Vec3::new(s1, s2, s3), Vec3::new(s4, s5, s6)];
        let mut pts: Vec<Vec3> = all.iter().take(n).copied().collect();
        let st = match gu::add_point(&mut pts, tol, pnt) {
            Ok(()) => SM_SUCCESS,
            Err(e) => status_code(&e).ok_or_else(|| HarnessError::new(e.to_string()))?,
        };
        // Slots beyond the count keep their input values, as in the C++ array.
        let mut slots = all;
        for (slot, p) in slots.iter_mut().zip(&pts) {
            *slot = *p;
        }
        v.push(st);
        v.push(pts.len() as f64);
        for p in slots {
            put3(&mut v, p);
        }
    } else {
        let mut cp = gu::ClosestPoints {
            count: n,
            params1: [s1, s2],
            params2: [s3, s4],
            distances: [s5, s6],
        };
        cp.add(a.f()?, a.f()?, a.f()?, a.f()?);
        v.push(SM_SUCCESS);
        v.push(cp.count as f64);
        v.extend(cp.params1);
        v.extend(cp.params2);
        v.extend(cp.distances);
    }
    Ok(finish(&v))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let args = i.args();
    match i.op.as_str() {
        "add_point" | "add_closest_point" => static_cpp(&i.op, &args),
        name => cpp_op(lookup(name)?, &args),
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let args = i.args();
    match i.op.as_str() {
        "add_point" | "add_closest_point" => static_rust(&i.op, &args),
        name => rust_op(lookup(name)?, &args),
    }
}

// ---- generation ------------------------------------------------------------------

fn coord(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => rng.range_i64(-5, 5) as f64,
        2 => rng.uniform(-1e3, 1e3),
        3 => rng.uniform(-1e-6, 1e-6),
        _ => rng.uniform(-10.0, 10.0),
    }
}

fn point(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

fn unit(rng: &mut Rng) -> [f64; 3] {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if l > 0.1 {
            return [p[0] / l, p[1] / l, p[2] / l];
        }
    }
}

/// A direction: unit, axis, scaled, tiny or zero.
fn direction(rng: &mut Rng) -> [f64; 3] {
    match rng.below(12) {
        0 => [0.0; 3],
        1 => scale(unit(rng), 10f64.powi(-(rng.range_i64(11, 14) as i32))),
        2 => {
            let mut v = [0.0; 3];
            if let Some(c) = v.get_mut(rng.below(3) as usize) {
                *c = if rng.chance(0.5) { 1.0 } else { -1.0 };
            }
            v
        }
        3..=5 => scale(unit(rng), rng.uniform(0.1, 20.0)),
        _ => unit(rng),
    }
}

fn scale(v: [f64; 3], s: f64) -> [f64; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l > 0.0 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// A unit vector perpendicular to `n` (unit).
fn perp(rng: &mut Rng, n: [f64; 3]) -> [f64; 3] {
    let p = cross(n, unit(rng));
    norm(p)
}

/// A second vector related to `v`: parallel, anti-parallel, nearly parallel,
/// perpendicular or independent.
fn partner(rng: &mut Rng, v: [f64; 3]) -> [f64; 3] {
    match rng.below(8) {
        0 => scale(v, rng.uniform(0.2, 3.0)),
        1 => scale(v, -rng.uniform(0.2, 3.0)),
        2 => add(v, scale(unit(rng), 10f64.powi(-(rng.range_i64(5, 14) as i32)))),
        3 => perp(rng, norm(v)),
        _ => direction(rng),
    }
}

/// A tolerance-ish small number.
fn small(rng: &mut Rng) -> f64 {
    match rng.below(5) {
        0 => 0.0,
        1 => 1e-6,
        2 => 1e-3,
        _ => 10f64.powi(-(rng.range_i64(2, 12) as i32)),
    }
}

/// A distance near `target`: exactly, within / beyond `tol`, or random.
fn near(rng: &mut Rng, target: f64, tol: f64) -> f64 {
    match rng.below(8) {
        0 => target,
        1 => target + tol * rng.uniform(-1.0, 1.0),
        2 => target + tol * rng.uniform(1.0, 3.0),
        3 => target - tol * rng.uniform(1.0, 3.0),
        4 => target + 1e-12 * rng.uniform(-5.0, 5.0),
        5 => target * rng.uniform(0.0, 1.0),
        _ => rng.uniform(0.0, 2.0 * target + 1.0),
    }
}

fn flat(vs: &[[f64; 3]]) -> Vec<f64> {
    vs.iter().flatten().copied().collect()
}

fn gen_static(rng: &mut Rng, add_point: bool) -> Vec<f64> {
    let n = rng.below(3) as f64;
    if add_point {
        let tol = small(rng).max(1e-9);
        let p0 = point(rng);
        let p1 = if rng.chance(0.3) {
            add(p0, scale(unit(rng), tol * rng.uniform(0.0, 2.0)))
        } else {
            point(rng)
        };
        let pnt = match rng.below(4) {
            0 => add(p0, scale(unit(rng), tol * rng.uniform(0.0, 2.0))),
            1 => add(p1, scale(unit(rng), tol * rng.uniform(0.0, 2.0))),
            _ => point(rng),
        };
        let mut x = vec![n];
        x.extend(p0);
        x.extend(p1);
        x.push(tol);
        x.extend(pnt);
        x
    } else {
        let d0 = rng.uniform(0.0, 5.0);
        let d1 = d0 + rng.uniform(0.0, 0.01);
        let tol = small(rng);
        let dist = match rng.below(5) {
            0 => d0 + tol * rng.uniform(-2.0, 2.0),
            1 => d1 + tol * rng.uniform(-2.0, 2.0),
            2 => d0,
            _ => rng.uniform(0.0, 6.0),
        };
        vec![
            n,
            rng.unit(),
            rng.unit(),
            rng.unit(),
            rng.unit(),
            d0,
            d1,
            dist,
            rng.unit(),
            rng.unit(),
            tol,
        ]
    }
}

fn gen_args(rng: &mut Rng, op: Op) -> Vec<f64> {
    match op {
        Op::PolygonArea => {
            let n = if rng.chance(0.7) { unit(rng) } else { direction(rng) };
            let mut x = n.to_vec();
            let k = 1 + rng.below(12) as usize;
            let planar = rng.chance(0.6);
            let (o, u) = (point(rng), perp(rng, norm(n)));
            let w = cross(norm(n), u);
            for _ in 0..k {
                let p = if planar {
                    add(o, add(scale(u, rng.uniform(-5.0, 5.0)), scale(w, rng.uniform(-5.0, 5.0))))
                } else {
                    point(rng)
                };
                x.extend(p);
            }
            x
        }
        Op::TriangleAreaSquared => {
            let p0 = point(rng);
            let p1 = point(rng);
            let p2 = if rng.chance(0.2) {
                add(p0, scale(sub(p1, p0), rng.uniform(-2.0, 2.0)))
            } else {
                point(rng)
            };
            flat(&[p0, p1, p2])
        }
        Op::TriangleDecomposition => {
            let base = match rng.below(6) {
                0 => 0.0,
                1 => 1e-13,
                _ => rng.uniform(0.01, 10.0),
            };
            let left = rng.uniform(0.0, 10.0);
            let right = match rng.below(4) {
                0 => base - left + 1e-12 * rng.uniform(-3.0, 3.0),
                1 => (left - base).abs() + 1e-12 * rng.uniform(-3.0, 3.0),
                _ => rng.uniform(0.0, 10.0),
            };
            vec![base, left, right]
        }
        Op::Parallelogram => {
            let b = direction(rng);
            let mut x = b.to_vec();
            x.extend(partner(rng, b));
            x.extend([rng.uniform(-5.0, 5.0), rng.uniform(-5.0, 5.0)]);
            x
        }
        Op::LinePointDistance | Op::LineClosestPoint => {
            let (p, v) = (point(rng), direction(rng));
            let t = if rng.chance(0.2) {
                add(p, scale(v, rng.uniform(-3.0, 3.0)))
            } else {
                point(rng)
            };
            flat(&[p, v, t])
        }
        Op::LineLineClosestPoint => {
            let v1 = direction(rng);
            flat(&[point(rng), v1, point(rng), partner(rng, v1)])
        }
        Op::LinePlane => {
            let n = direction(rng);
            let v = if rng.chance(0.2) {
                add(perp(rng, norm(n)), scale(unit(rng), 1e-13))
            } else {
                partner(rng, n)
            };
            flat(&[point(rng), v, point(rng), n])
        }
        Op::LinePlaneUv => {
            let su = direction(rng);
            let sv = partner(rng, su);
            let v = if rng.chance(0.2) {
                add(su, sv)
            } else {
                direction(rng)
            };
            flat(&[point(rng), v, point(rng), su, sv])
        }
        Op::SegmentPointDistance | Op::SegmentAngle => {
            let s = point(rng);
            let e = if rng.chance(0.1) { s } else { point(rng) };
            let t = match rng.below(4) {
                0 => add(s, scale(sub(e, s), rng.uniform(-1.0, 2.0))),
                1 => add(add(s, scale(sub(e, s), rng.uniform(-1.0, 2.0))), point(rng)),
                _ => point(rng),
            };
            flat(&[s, e, t])
        }
        Op::CircleCenter3Points => {
            let c = [coord(rng), coord(rng)];
            let r = rng.uniform(1e-3, 50.0);
            let mut p = [[0.0; 2]; 3];
            for q in &mut p {
                let a = rng.uniform(0.0, std::f64::consts::TAU);
                *q = [c[0] + r * a.cos(), c[1] + r * a.sin()];
            }
            match rng.below(6) {
                // two (nearly) coincident points
                0 => p[1] = [p[0][0] + 1e-8 * rng.uniform(-1.0, 1.0), p[0][1]],
                // collinear
                1 => {
                    let t = rng.uniform(-2.0, 2.0);
                    p[1] = [
                        p[0][0] + t * (p[2][0] - p[0][0]),
                        p[0][1] + t * (p[2][1] - p[0][1]),
                    ];
                }
                _ => {}
            }
            p.iter().flatten().copied().collect()
        }
        Op::Circle3Points => {
            let c = point(rng);
            let n = unit(rng);
            let u = perp(rng, n);
            let w = cross(n, u);
            let r = rng.uniform(1e-3, 50.0);
            let mut pts = [[0.0; 3]; 3];
            for q in &mut pts {
                let a = rng.uniform(0.0, std::f64::consts::TAU);
                *q = add(c, add(scale(u, r * a.cos()), scale(w, r * a.sin())));
            }
            if rng.chance(0.15) {
                let t = rng.uniform(-2.0, 2.0);
                pts[1] = add(pts[0], scale(sub(pts[2], pts[0]), t));
            }
            let mut x = flat(&pts);
            let dim = match rng.below(8) {
                0 => rng.below(5) as f64,
                1..=3 => 2.0,
                _ => 3.0,
            };
            x.extend([dim, flag(rng.chance(0.3))]);
            x
        }
        Op::Silhouette => {
            let c = point(rng);
            let n = if rng.chance(0.8) { unit(rng) } else { direction(rng) };
            let r = rng.uniform(0.0, 5.0);
            let persp = rng.chance(0.5);
            let eye = if persp {
                add(c, scale(unit(rng), rng.uniform(0.0, 3.0) * r + rng.uniform(0.0, 2.0)))
            } else {
                partner(rng, n)
            };
            let mut x = flat(&[c, n]);
            x.extend([r, flag(persp)]);
            x.extend(eye);
            x
        }
        Op::ConesDisjoint => {
            let v1 = direction(rng);
            let mut x = v1.to_vec();
            x.push(rng.uniform(0.0, 1.0));
            x.extend(partner(rng, v1));
            x.push(rng.uniform(0.0, 1.0));
            x
        }
        Op::LineCircle => {
            let c = point(rng);
            let n = unit(rng);
            let r = rng.uniform(0.0, 10.0);
            let tol = small(rng);
            let u = perp(rng, n);
            let w = cross(n, u);
            let d = near(rng, r, tol.max(1e-9));
            let off = rng.uniform(-5.0, 5.0);
            let lp = add(add(c, scale(w, d)), add(scale(u, off), scale(n, rng.uniform(-1.0, 1.0))));
            let lv = if rng.chance(0.85) {
                add(scale(u, rng.uniform(0.1, 3.0)), scale(n, rng.uniform(-1.0, 1.0)))
            } else {
                partner(rng, n)
            };
            let mut x = flat(&[lp, lv, c, n]);
            x.extend([r, tol]);
            x
        }
        Op::CircleCircle => {
            let n = unit(rng);
            let c1 = point(rng);
            let r1 = rng.uniform(0.0, 10.0);
            let r2 = if rng.chance(0.2) { r1 } else { rng.uniform(0.0, 10.0) };
            let tol = small(rng);
            let target = match rng.below(4) {
                0 => r1 + r2,
                1 => (r1 - r2).abs(),
                2 => 0.0,
                _ => rng.uniform(0.0, r1 + r2 + 1.0),
            };
            let d = near(rng, target, tol.max(1e-9)).max(0.0);
            let c2 = add(add(c1, scale(perp(rng, n), d)), scale(n, rng.uniform(-1.0, 1.0)));
            let mut x = flat(&[n, c1]);
            x.push(r1);
            x.extend(c2);
            x.extend([r2, tol]);
            x
        }
        Op::LineSphere => {
            let c = point(rng);
            let r = rng.uniform(0.0, 10.0);
            let tol = small(rng);
            let lv = direction(rng);
            let d = near(rng, r, tol.max(1e-9)).max(0.0);
            let lp = add(add(c, scale(perp(rng, norm(lv)), d)), scale(norm(lv), rng.uniform(-5.0, 5.0)));
            let mut x = flat(&[lp, lv, c]);
            x.extend([r, tol]);
            x
        }
        Op::ThreePlanes => {
            let v1 = if rng.chance(0.9) { unit(rng) } else { direction(rng) };
            let v2 = if rng.chance(0.2) { v1 } else { unit(rng) };
            let v3 = if rng.chance(0.2) {
                norm(add(v1, v2))
            } else {
                unit(rng)
            };
            flat(&[point(rng), v1, point(rng), v2, point(rng), v3])
        }
        Op::PlaneEval | Op::PlaneDrop => {
            let xa = direction(rng);
            let mut x = flat(&[point(rng), xa, partner(rng, xa)]);
            if op == Op::PlaneEval {
                x.extend([coord(rng), coord(rng)]);
            } else {
                x.extend(point(rng));
            }
            x
        }
        Op::PlanePointDistance => {
            let n = match rng.below(4) {
                0 => direction(rng),
                1 => scale(unit(rng), 1.0 + rng.uniform(-1e-11, 1e-11)),
                _ => unit(rng),
            };
            flat(&[point(rng), n, point(rng)])
        }
        Op::PointProject => flat(&[point(rng), point(rng), direction(rng)]),
        Op::VectorProject => {
            let n = if rng.chance(0.8) { unit(rng) } else { direction(rng) };
            flat(&[partner(rng, n), n])
        }
        Op::StartPlane => {
            let o = point(rng);
            let xa = if rng.chance(0.8) { unit(rng) } else { direction(rng) };
            let ya = if rng.chance(0.8) { perp(rng, norm(xa)) } else { partner(rng, xa) };
            let z = norm(cross(norm(xa), norm(ya)));
            let xn = norm(xa);
            let yn = cross(z, xn);
            let r = rng.uniform(0.0, 10.0);
            let ang = match rng.below(5) {
                0 => 0.0,
                1 => 10f64.powi(-(rng.range_i64(3, 14) as i32)) * rng.uniform(-1.0, 1.0),
                2 => std::f64::consts::TAU - 1e-9 * rng.uniform(0.0, 10.0),
                _ => rng.uniform(-7.0, 7.0),
            };
            let p = if rng.chance(0.08) {
                add(o, scale(z, rng.uniform(-3.0, 3.0)))
            } else {
                add(o, add(add(scale(xn, r * ang.cos()), scale(yn, r * ang.sin())), scale(z, rng.uniform(-3.0, 3.0))))
            };
            let (smin, smax) = match rng.below(5) {
                0 | 1 => (0.0, 360.0),
                2 => (-180.0, 180.0),
                _ => {
                    let a = rng.uniform(-360.0, 360.0);
                    (a, a + rng.uniform(0.0, 360.0))
                }
            };
            let mut x = flat(&[p, o, xa, ya]);
            x.extend([smin, smax, small(rng).max(1e-9), flag(rng.chance(0.5))]);
            x
        }
        Op::Perspective => {
            let n = if rng.chance(0.8) { unit(rng) } else { direction(rng) };
            flat(&[point(rng), point(rng), n, point(rng)])
        }
        Op::VecLinComb => {
            let b1 = direction(rng);
            let b2 = partner(rng, b1);
            let v = if rng.chance(0.3) {
                add(scale(b1, coord(rng)), scale(b2, coord(rng)))
            } else {
                point(rng)
            };
            flat(&[b1, b2, v])
        }
        Op::BestFitPoint | Op::BestFitLine => {
            let tol = small(rng);
            let k = rng.below(12) as usize;
            let p0 = point(rng);
            let d = direction(rng);
            let noise = match rng.below(4) {
                0 => 0.0,
                1 => tol * 0.5,
                2 => tol * 2.0,
                _ => rng.uniform(0.0, 3.0),
            };
            let mut x = vec![tol];
            for _ in 0..k {
                let p = if rng.chance(0.2) {
                    add(p0, scale(unit(rng), noise))
                } else {
                    add(add(p0, scale(d, rng.uniform(-5.0, 5.0))), scale(unit(rng), noise))
                };
                x.extend(p);
            }
            x
        }
        Op::TripleProduct | Op::Det3Vectors | Op::Inverse3x3 | Op::Copy3x3 | Op::Det3x3 => {
            let a = point(rng);
            let b = point(rng);
            let c = match rng.below(5) {
                0 => add(scale(a, coord(rng)), scale(b, coord(rng))),
                1 => add(a, scale(unit(rng), 1e-13)),
                _ => point(rng),
            };
            flat(&[a, b, c])
        }
        Op::Copy4x4 | Op::Det4x4 => {
            let mut m = [[0.0; 4]; 4];
            for row in &mut m {
                for c in row.iter_mut() {
                    *c = coord(rng);
                }
            }
            if rng.chance(0.2) {
                // singular: last row = 2 * row 0 - row 1
                let [r0, r1, _, _] = m;
                for (c, (a, b)) in m[3].iter_mut().zip(r0.iter().zip(&r1)) {
                    *c = a * 2.0 - b;
                }
            }
            m.iter().flatten().copied().collect()
        }
        Op::Quadratic => {
            let tol = match rng.below(3) {
                0 => 1e-8,
                1 => 1e-12,
                _ => small(rng),
            };
            let c: Vec<f64> = match rng.below(5) {
                0 => {
                    // double root
                    let r = coord(rng);
                    let a = rng.uniform(-5.0, 5.0);
                    vec![a * r * r, -2.0 * a * r, a]
                }
                1 => vec![coord(rng), coord(rng), 0.0],
                2 => vec![0.0, coord(rng), coord(rng)],
                _ => vec![coord(rng), coord(rng), coord(rng)],
            };
            let mut x = c;
            x.push(tol);
            x
        }
        Op::DirDerivs => {
            let hu = rng.below(5) as usize;
            let hv = rng.below(5) as usize;
            let mut x = if rng.chance(0.1) {
                vec![0.0, 0.0]
            } else {
                vec![coord(rng), coord(rng)]
            };
            x.extend([hu as f64, hv as f64]);
            for _ in 0..(hu + 1) * (hv + 1) {
                x.extend(point(rng));
            }
            x
        }
        Op::VolDirDerivs => {
            let n = rng.below(5) as usize;
            let mut x = if rng.chance(0.1) { vec![0.0; 3] } else { point(rng).to_vec() };
            x.push(n as f64);
            for _ in 0..(n + 1) * (n + 1) * (n + 1) {
                x.extend(point(rng));
            }
            x
        }
        Op::DogLeg => {
            let j = |rng: &mut Rng| rng.uniform(-0.5, 0.5);
            let w = rng.uniform(0.5, 5.0);
            let mut x = vec![
                0.0 + j(rng),
                0.0 + j(rng),
                0.0 + j(rng),
                1.0 + j(rng),
                w + j(rng),
                1.0 + j(rng),
                w + j(rng),
                2.0 + j(rng),
            ];
            if rng.chance(0.3) {
                for v in &mut x {
                    *v = coord(rng);
                }
            }
            x.extend([rng.uniform(5.0, 60.0), rng.uniform(5.0, 80.0)]);
            x
        }
        Op::AddSorted => {
            let n = rng.below(10) as usize;
            let incr = rng.chance(0.5);
            let mut vals: Vec<f64> = (0..n).map(|_| rng.range_i64(-5, 5) as f64).collect();
            vals.sort_by(f64::total_cmp);
            if !incr {
                vals.reverse();
            }
            let value = if rng.chance(0.5) {
                rng.range_i64(-6, 6) as f64
            } else {
                rng.uniform(-6.0, 6.0)
            };
            let mut x = vec![value, rng.below(100) as f64, flag(incr), flag(rng.chance(0.6)), n as f64];
            x.extend(vals);
            x.extend((0..n).map(|_| rng.below(100) as f64));
            x
        }
        Op::ShellSort => {
            let n = rng.below(30) as usize;
            (0..n)
                .map(|_| {
                    if rng.chance(0.3) {
                        rng.range_i64(-3, 3) as f64
                    } else {
                        coord(rng)
                    }
                })
                .collect()
        }
        Op::DoubleLess => {
            let a = coord(rng);
            vec![a, if rng.chance(0.3) { a } else { coord(rng) }]
        }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let k = rng.below(OPS.len() as u64 + 2) as usize;
    let (op, x) = if let Some((name, op)) = OPS.get(k) {
        ((*name).to_string(), gen_args(rng, *op))
    } else {
        let ap = k == OPS.len();
        let name = if ap { "add_point" } else { "add_closest_point" };
        (name.to_string(), gen_static(rng, ap))
    };
    Inputs {
        op,
        x,
        pts: Vec::new(),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.geom_utility_p1"
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::case::CaseContext;

    /// The generator reaches every operation, mostly with `SM_SUCCESS` and also with
    /// failing statuses where upstream has them.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<String, BTreeMap<i64, usize>> = BTreeMap::new();
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("geom.geom_utility_p1", i));
            let inputs = gen_case(&mut rng);
            let args = inputs.args();
            let out = match inputs.op.as_str() {
                "add_point" | "add_closest_point" => static_cpp(&inputs.op, &args),
                name => cpp_op(lookup(name).unwrap(), &args),
            }
            .unwrap();
            let status = out.ints.get("status").and_then(|v| v.first()).copied().unwrap();
            *tally.entry(inputs.op).or_default().entry(status).or_default() += 1;
        }
        for (name, _) in OPS {
            let t = tally.get(name).unwrap_or_else(|| panic!("{name} never generated"));
            assert!(t.get(&1000).copied().unwrap_or(0) >= 20, "{name}: {t:?}");
        }
        for name in [
            "triangle_decomposition",
            "line_line_closest_point",
            "line_plane",
            "line_plane_uv",
            "circle_center_3_points",
            "circle_3_points",
            "three_planes",
            "plane_point_distance",
            "vec_lin_comb",
            "inverse_3x3",
            "add_point",
        ] {
            let t = tally.get(name).unwrap();
            assert!(t.keys().any(|&s| s != 1000), "{name} has no failing case: {t:?}");
        }
    }
}
