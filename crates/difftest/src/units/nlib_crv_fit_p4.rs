//! Runner for unit `nlib.crv_fit_p4`: `N_FitCrvApproxDerivs` (with
//! `N_FitCalcKnotsEndDerivs`), `SmBSplineCurve::CreateApproximatingCurve` and
//! `SmApiCreateCurveApproxPoints`.
//!
//! C++: `solidcore_ffi::units::nlib_crv_fit_p4` (shim
//! `harness/cpp/shim/nlib_crv_fit_p4.cpp`, which documents the layouts).
//! Rust: [`solidcore::nurbs::crv_fit::p4::approx_end_derivs`],
//! [`solidcore::nurbs::b_spline_curve::BSplineCurve::create_approximating_curve`],
//! [`solidcore::primitive::api_curves::create_curve_approx_points`].
//!
//! A case is an operation and its block. The output is the status and the fitted
//! NURB (control points and knots; the dimension for the `SmBSplineCurve` paths).
//! Generated cases (their own seed stream per operation mix) sample arcs, helices,
//! sine waves, noisy lines, planar and spatial zigzags and nearly collinear data at
//! tiny, unit and huge scales, with 2 to 60 points, every parametrization, end
//! derivative orders 0 to 3 at either end, control point counts from below `k + l + 1`
//! up to above the point count, degrees 0 to 6, repeated points, and for the curve
//! paths every vector count (0 to 3, more vectors 0 to 4) and dimensions 2 and 3.

// The generator indexes the arrays it has just built (as other runners do).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::crv_fit::p4::approx_end_derivs;
use solidcore::primitive::api_curves::create_curve_approx_points;
use solidcore::SolidError;
use solidcore_ffi::units::nlib_crv_fit_p4 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 900;

/// `nlib.crv_fit_p4` runner.
pub struct Runner;

/// One case: the shim's operation and block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: i32,
    block: Vec<f64>,
}

fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn output(flat: Vec<f64>) -> Output {
    Output::new().values("out", flat)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::eval(i.op, &i.block) {
        Ok(v) => Ok(output(v)),
        Err(solidcore_ffi::FfiError::InvalidArgument(r)) => Ok(Output::rejected(r)),
        Err(e) => Err(herr(e)),
    }
}

// ---- Rust side ----------------------------------------------------------------------

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, HarnessError> {
        let x = *self.v.get(self.at).ok_or_else(|| herr("short block"))?;
        self.at += 1;
        Ok(x)
    }
    fn int(&mut self) -> Result<usize, HarnessError> {
        Ok(self.num()? as usize)
    }
    fn vecs(&mut self, count: usize) -> Result<Vec<Vec3>, HarnessError> {
        (0..count)
            .map(|_| Ok(Vec3::new(self.num()?, self.num()?, self.num()?)))
            .collect()
    }
}

/// `sc::dump_crv`: `n p m`, the control points and the knots.
fn dump(o: &mut Vec<f64>, c: &Curve) {
    let pw = c.cpts();
    let u = c.knots();
    o.extend([
        pw.len() as f64 - 1.0,
        c.degree() as f64,
        u.len() as f64 - 1.0,
    ]);
    for q in pw {
        o.extend([q.x, q.y, q.z, q.w]);
    }
    o.extend(u.iter().copied());
}

/// The NLib status of a Rust error (`NL_ERROR.eno`).
fn nl_status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        _ => -1.0,
    }
}

/// The `SmStatus` of a Rust error.
fn sm_status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn par_nl(flag: usize) -> Result<ParamMethod, HarnessError> {
    Ok(match flag {
        1 => ParamMethod::Uniform,
        2 => ParamMethod::ChordLength,
        3 => ParamMethod::Centripetal,
        _ => return Err(herr("bad parametrization flag")),
    })
}

/// `lParameterization`: 1 chord length, 2 centripetal, anything else uniform.
fn par_sm(flag: usize) -> ParamMethod {
    match flag {
        1 => ParamMethod::ChordLength,
        2 => ParamMethod::Centripetal,
        _ => ParamMethod::Uniform,
    }
}

fn bspline_result(o: &mut Vec<f64>, r: solidcore::Result<BSplineCurve>) -> Option<Output> {
    match r {
        Ok(mut c) => {
            o.push(0.0);
            o.push(c.dim() as f64);
            match c.get_or_create_gw_nurb() {
                Ok(nurb) => dump(o, nurb),
                Err(e) => return Some(Output::err(e.to_string())),
            }
        }
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Some(d);
            }
            o.push(sm_status(&e));
        }
    }
    None
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut rd = Rd { v: &i.block, at: 0 };
    let mut o = Vec::new();
    match i.op {
        0 => {
            let n = rd.int()?;
            let p = rd.int()?;
            let par = par_nl(rd.int()?)?;
            let k = rd.int()?;
            let l = rd.int()?;
            let c = rd.int()?;
            let pts = rd.vecs(c)?;
            let ds = rd.vecs(k)?;
            let de = rd.vecs(l)?;
            match approx_end_derivs(&pts, &ds, &de, n, p, par) {
                Ok(cur) => {
                    o.push(0.0);
                    dump(&mut o, &cur);
                }
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    o.push(nl_status(&e));
                }
            }
        }
        1 => {
            let num = rd.int()?;
            let par = par_sm(rd.int()?);
            let dim = rd.int()? as u64;
            let degree = rd.int()?;
            let c = rd.int()?;
            let pts: Vec<Point3> = rd.vecs(c)?;
            let nvec = rd.int()?;
            let vecs = rd.vecs(nvec)?;
            let has_more = rd.int()? != 0;
            let nmore = rd.int()?;
            let more = rd.vecs(nmore)?;
            let r = BSplineCurve::create_approximating_curve(
                num,
                par,
                dim,
                degree,
                &pts,
                &vecs,
                has_more.then_some(more.as_slice()),
            );
            if let Some(d) = bspline_result(&mut o, r) {
                return Ok(d);
            }
        }
        _ => {
            let c = rd.int()?;
            let pts: Vec<Point3> = rd.vecs(c)?;
            if let Some(d) = bspline_result(&mut o, create_curve_approx_points(&pts)) {
                return Ok(d);
            }
        }
    }
    Ok(output(o))
}

// ---- generation ---------------------------------------------------------------------

/// Sample points of a test shape at scale `s`.
fn shape(rng: &mut Rng, count: usize, s: f64) -> Vec<[f64; 3]> {
    let kind = rng.below(7);
    let mut pts = Vec::new();
    for i in 0..count {
        let t = if count > 1 { i as f64 / (count - 1) as f64 } else { 0.0 };
        let p = match kind {
            0 => {
                let a = 2.5 * t;
                [a.cos(), a.sin(), 0.0]
            }
            1 => {
                let a = 6.0 * t;
                [a.cos(), a.sin(), 0.3 * a]
            }
            2 => [4.0 * t, (6.0 * t).sin(), 0.0],
            3 => [3.0 * t, 1e-3 * rng.uniform(-1.0, 1.0), 1e-3 * rng.uniform(-1.0, 1.0)],
            4 => [t, if i % 2 == 0 { 0.0 } else { 0.4 }, 0.0],
            5 => [t, if i % 2 == 0 { 0.0 } else { 0.4 }, 0.2 * (i % 3) as f64],
            _ => [2.0 * t, 1e-12 * t, 0.0], // nearly collinear
        };
        pts.push([p[0] * s, p[1] * s, p[2] * s]);
    }
    // repeated points
    if count > 3 && rng.chance(0.08) {
        let j = 1 + rng.below((count - 2) as u64) as usize;
        pts[j] = pts[j - 1];
    }
    pts
}

fn scale(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => 10f64.powi(rng.range_i64(-6, -2) as i32),
        1 => 10f64.powi(rng.range_i64(2, 6) as i32),
        _ => rng.uniform(0.5, 4.0),
    }
}

fn vec3(rng: &mut Rng, s: f64) -> [f64; 3] {
    if rng.chance(0.05) {
        return [0.0, 0.0, 0.0];
    }
    [rng.uniform(-2.0, 2.0) * s, rng.uniform(-2.0, 2.0) * s, rng.uniform(-1.0, 1.0) * s]
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let s = scale(rng);
    let count = match rng.below(6) {
        0 => rng.below(4) as usize,
        1 => 9 + rng.below(8) as usize,
        2 => 17 + rng.below(44) as usize,
        _ => 4 + rng.below(12) as usize,
    };
    let pts = shape(rng, count, s);
    let op = match rng.below(10) {
        0..=4 => 0,
        5..=7 => 1,
        _ => 2,
    };
    let mut b: Vec<f64> = Vec::new();
    match op {
        0 => {
            let p = match rng.below(8) {
                0 => rng.below(2) as usize,
                1 => 4 + rng.below(3) as usize,
                _ => 2 + rng.below(2) as usize,
            };
            let k = (rng.below(4) as usize).min(p.saturating_sub(1)).min(if rng.chance(0.1) { 4 } else { 3 });
            let l = (rng.below(4) as usize).min(p.saturating_sub(1));
            let n = match rng.below(8) {
                0 => k + l + 1,
                1 => (k + l).saturating_sub(rng.below(2) as usize),
                2 => count + rng.below(3) as usize,
                _ => (k + l + 2 + rng.below(count.max(1) as u64) as usize).min(count.saturating_sub(1).max(k + l + 1)),
            };
            let par = 1 + rng.below(3);
            b.extend([n as f64, p as f64, par as f64, k as f64, l as f64, count as f64]);
            for q in &pts {
                b.extend(q);
            }
            for _ in 0..k + l {
                b.extend(vec3(rng, s));
            }
        }
        1 => {
            let nvec = match rng.below(10) {
                0 => 3,
                1..=3 => 1,
                4..=6 => 2,
                _ => 0,
            };
            let degree = match rng.below(8) {
                0 => rng.below(2) as usize,
                1 => 4 + rng.below(3) as usize,
                _ => 2 + rng.below(2) as usize,
            };
            let num = match rng.below(6) {
                0 => rng.below(3) as usize,
                1 => count + rng.below(3) as usize,
                _ => degree + 1 + rng.below(count.max(1) as u64) as usize,
            };
            let dim = if rng.chance(0.2) { 2 } else { 3 };
            b.extend([
                num as f64,
                rng.below(4) as f64,
                f64::from(dim),
                degree as f64,
                count as f64,
            ]);
            for q in &pts {
                let z = if dim == 2 && rng.chance(0.5) { 0.0 } else { q[2] };
                b.extend([q[0], q[1], z]);
            }
            b.push(f64::from(nvec));
            for _ in 0..nvec {
                b.extend(vec3(rng, s));
            }
            let has_more = rng.chance(0.4);
            let nmore = if has_more || rng.chance(0.1) { rng.below(5) as usize } else { 0 };
            b.extend([f64::from(u8::from(has_more)), nmore as f64]);
            for _ in 0..nmore {
                b.extend(vec3(rng, s));
            }
        }
        _ => {
            b.push(count as f64);
            for q in &pts {
                b.extend(q);
            }
        }
    }
    Inputs { op, block: b }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_fit_p4"
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
