//! Runner for unit `curve.parabola`: parabolas as analytic B-spline curves.
//!
//! C++: `solidcore_ffi::units::curve_parabola` (upstream `SmParabola`,
//! `source/SMLib/inc/SmParabola.h`, `source/SMLib/src/SmParabola.cpp`).
//! Rust: `solidcore::curve::parabola` (and its dispatch in `solidcore::curve::curve`).
//!
//! Every case builds parabola `a` (and `b` for `eq`) from a construction `kind`
//! (`new`: the constructor; `canonical`: `CreateCanonical` of the placement), centre,
//! axes, STEP domain, focal distance and dimension, then runs one operation `op` with
//! the numeric arguments `args` (layout per op in
//! `solidcore_ffi::units::curve_parabola::Op`) and, for `read_db`, the stream `data`
//! (`binary`: tokens `d<double>`, `l<long>`, `s<short>` written native-endian). The
//! result `r` is the op status (`0` = success) and its values; a curve state is
//! `type dim oob borrowed knot_type form O X Y F step_min step_max has_nurb [n p m
//! cpts knots]`. `gps_step` runs `SmCurve::GlobalPointSolve` (`curve.curve_p1`);
//! a case still reaching an unported callee is deferred.
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use std::fmt::Write;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::PARABOLA_TYPE;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::parabola::Parabola;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::vector3d::Vec3;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::curve_parabola::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// Upstream `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

/// `curve.parabola` runner.
pub struct Runner;

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 13] = [
    ("state", Op::State),
    ("eq", Op::Eq),
    ("adjust", Op::Adjust),
    ("invert", Op::Invert),
    ("copy", Op::Copy),
    ("eval_step", Op::EvalStep),
    ("gps_step", Op::GpsStep),
    ("queries", Op::Queries),
    ("make_nurb", Op::MakeNurb),
    ("reverse", Op::Reverse),
    ("transform", Op::Transform),
    ("read_db", Op::ReadDb),
    ("virtuals", Op::Virtuals),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurveIn {
    /// `new` or `canonical`.
    kind: String,
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    #[serde(default)]
    domain: [f64; 2],
    f: f64,
    #[serde(default = "default_dim")]
    dim: u32,
}

fn default_dim() -> u32 {
    3
}

impl Default for CurveIn {
    fn default() -> Self {
        Self {
            kind: "new".into(),
            center: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            domain: [-1.0, 1.0],
            f: 1.0,
            dim: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    a: CurveIn,
    #[serde(default)]
    b: CurveIn,
    #[serde(default)]
    args: Vec<f64>,
    #[serde(default)]
    data: String,
    #[serde(default)]
    binary: bool,
}

fn lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Result<T, HarnessError> {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, x)| *x)
        .ok_or_else(|| HarnessError::new(format!("unknown name {name}")))
}

fn harness(msg: &str) -> HarnessError {
    HarnessError::new(msg.to_string())
}

fn status_code(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// The stream bytes of a case: `data` as text, or (`binary`) the tokens `d<f64>`,
/// `l<i64>`, `s<i16>` written native-endian.
fn db_bytes(i: &Inputs) -> Result<Vec<u8>, HarnessError> {
    if !i.binary {
        return Ok(i.data.clone().into_bytes());
    }
    let mut out = Vec::new();
    for tok in i.data.split_whitespace() {
        let bad = || HarnessError::new(format!("bad binary token {tok}"));
        let (kind, val) = tok.split_at(1);
        match kind {
            "d" => out.extend(val.parse::<f64>().map_err(|_| bad())?.to_ne_bytes()),
            "l" => out.extend(val.parse::<i64>().map_err(|_| bad())?.to_ne_bytes()),
            "s" => out.extend(val.parse::<i16>().map_err(|_| bad())?.to_ne_bytes()),
            _ => return Err(bad()),
        }
    }
    Ok(out)
}

fn ffi_curve(c: &CurveIn) -> Result<ffi::Curve, HarnessError> {
    let kind = match c.kind.as_str() {
        "new" => 0,
        "canonical" => 1,
        k => return Err(HarnessError::new(format!("unknown curve kind {k}"))),
    };
    Ok(ffi::Curve {
        kind,
        center: c.center,
        x_axis: c.x,
        y_axis: c.y,
        domain: c.domain,
        focal_dist: c.f,
        dim: c.dim,
    })
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&OPS, &i.op)?;
    let a = ffi_curve(&i.a)?;
    let b = ffi_curve(&i.b)?;
    match ffi::op(op, &a, &b, &i.args, &db_bytes(i)?) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(FfiError::Status(s)) if s > 0 => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

/// Build a curve as the shim does.
fn build(c: &CurveIn) -> Result<Parabola, HarnessError> {
    let center = Vec3::from(c.center);
    let x = Vec3::from(c.x);
    let y = Vec3::from(c.y);
    match c.kind.as_str() {
        "new" => Ok(Parabola::new(
            center,
            x,
            y,
            Extent1d::from_min_max(c.domain[0], c.domain[1]),
            c.f,
            u64::from(c.dim),
        )),
        "canonical" => {
            let mut a2p = Axis2Placement::default();
            let _ = a2p.set_canonical(center, x, y);
            Ok(Parabola::create_canonical(&a2p, c.f))
        }
        k => Err(HarnessError::new(format!("unknown curve kind {k}"))),
    }
}

fn put_v(out: &mut Vec<f64>, v: Vec3) {
    out.extend(v.to_array());
}

fn put_state(out: &mut Vec<f64>, p: &Parabola) {
    let base = p.base();
    out.push(p.type_code() as f64);
    out.push(p.dim() as f64);
    out.push(flag(base.is_out_of_bounds_enabled()));
    out.push(flag(base.is_nurb_borrowed()));
    out.push(f64::from(base.knot_type() as i32));
    out.push(f64::from(base.form() as i32));
    let (a2p, f) = p.canonical();
    put_v(out, a2p.origin());
    put_v(out, a2p.x_axis());
    put_v(out, a2p.y_axis());
    out.push(f);
    let si = p.step_interval();
    out.extend([si.min(), si.max()]);
    match base.gw_nurb() {
        None => out.push(0.0),
        Some(cur) => {
            out.push(1.0);
            out.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for q in cur.cpts() {
                out.extend([q.x, q.y, q.z, q.w]);
            }
            out.extend_from_slice(cur.knots());
        }
    }
}

fn str_codes(out: &mut Vec<f64>, s: &str) {
    out.extend(s.bytes().map(f64::from));
}

fn arg(args: &[f64], i: usize) -> Result<f64, HarnessError> {
    args.get(i)
        .copied()
        .ok_or_else(|| harness("missing argument"))
}

fn arg3(args: &[f64], i: usize) -> Result<Vec3, HarnessError> {
    Ok(Vec3::new(
        arg(args, i)?,
        arg(args, i + 1)?,
        arg(args, i + 2)?,
    ))
}

fn st<T>(r: &solidcore::Result<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(e) => status_code(e),
    }
}

fn solver_op(code: f64) -> Result<SolverOperation, HarnessError> {
    Ok(match code as i64 {
        0 => SolverOperation::Intersect,
        3 => SolverOperation::Minimize,
        4 => SolverOperation::Maximize,
        5 => SolverOperation::Normalize,
        _ => return Err(harness("solver operation")),
    })
}

/// `Err` = the case is deferred (an `Unported` error).
fn rust_values(i: &Inputs) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let op = lookup(&OPS, &i.op)?;
    let mut a = build(&i.a)?;
    let args = &i.args;
    let mut out = Vec::new();
    match op {
        Op::State => {
            out.push(0.0);
            put_state(&mut out, &a);
        }
        Op::Eq => {
            out.push(0.0);
            if arg(args, 0)? == 0.0 {
                let b = build(&i.b)?;
                out.push(flag(a.approx_eq(&b)));
            } else {
                out.push(flag(a.approx_eq(&a)));
            }
        }
        Op::Adjust => {
            let ivl = Extent1d::from_min_max(arg(args, 0)?, arg(args, 1)?);
            let r = a.adjust_step_interval(&ivl, None);
            if let Err(e @ SolidError::Unported { .. }) = r {
                return Ok(Err(e));
            }
            out.push(st(&r));
            put_state(&mut out, &a);
        }
        Op::Invert => {
            let p = arg3(args, 0)?;
            let (t, loc) = a.step_inversion(p, arg(args, 3)? != 0.0);
            out.extend([0.0, t, loc.map_or(-1.0, |l| f64::from(l as i32))]);
        }
        Op::Copy => {
            let c = a.copy();
            out.push(0.0);
            put_state(&mut out, &c);
            out.push(flag(c.approx_eq(&a)));
        }
        Op::EvalStep => {
            let n = usize::try_from(arg(args, 1)? as i64).map_err(|_| harness("negative n"))?;
            match a.evaluate_step(arg(args, 0)?, n) {
                Ok(v) => {
                    out.push(0.0);
                    for x in v {
                        put_v(&mut out, x);
                    }
                }
                Err(e) => out.push(status_code(&e)),
            }
        }
        Op::GpsStep => {
            let ivl = Extent1d::from_min_max(arg(args, 0)?, arg(args, 1)?);
            let sop = solver_op(arg(args, 2)?)?;
            let p = arg3(args, 3)?;
            let tol = arg(args, 6)?;
            let target = (arg(args, 7)? != 0.0).then_some(arg(args, 8)?);
            let req = if arg(args, 9)? == 0.0 {
                SolutionRequested::Single
            } else {
                SolutionRequested::All
            };
            let k = solidcore::kernel::Kernel::default();
            match a.global_point_solve_step(&k, None, &ivl, sop, p, tol, target, req) {
                Ok(sols) => {
                    out.push(0.0);
                    out.push(sols.len() as f64);
                    for s in &sols {
                        out.push(f64::from(s.solution_type as i32));
                        out.push(s.start.parameters.first().copied().unwrap_or(0.0));
                        out.push(s.start.solution_value);
                    }
                }
                Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
                Err(e) => out.extend([status_code(&e), 0.0]),
            }
            put_state(&mut out, &a);
        }
        Op::Queries => {
            let t = arg(args, 0)? as i64;
            out.push(0.0);
            out.extend([
                flag(a.is_bounded()),
                flag(a.is_analytic()),
                flag(a.is_arc()),
            ]);
            let si = a.step_interval();
            out.extend([si.min(), si.max()]);
            let md = a.max_analytic_domain();
            out.extend([md.min(), md.max()]);
            out.push(flag(a.is_kind_of(t)));
            out.push(a.type_code() as f64);
            str_codes(&mut out, a.type_string());
            out.push(-1.0);
            str_codes(&mut out, a.class_string());
        }
        Op::MakeNurb => {
            let r = a.make_nurb(None);
            out.push(st(&r));
            put_state(&mut out, &a);
        }
        Op::Reverse => {
            let ivl = Extent1d::from_min_max(arg(args, 0)?, arg(args, 1)?);
            match a.reverse_parameterization(&ivl, None) {
                Ok(n) => out.extend([0.0, n.min(), n.max()]),
                Err(e) => return Ok(Err(e)),
            }
            put_state(&mut out, &a);
        }
        Op::Transform => {
            let mut a2p = Axis2Placement::default();
            if let Err(e) = a2p.set_canonical(arg3(args, 0)?, arg3(args, 3)?, arg3(args, 6)?) {
                out.extend([-2.0, status_code(&e)]);
            } else {
                let scale = (arg(args, 9)? != 0.0).then_some(arg3(args, 10)?);
                let r = a.transform(&a2p, scale);
                out.push(st(&r));
                put_state(&mut out, &a);
            }
        }
        Op::ReadDb => {
            let ft = if arg(args, 0)? == 0.0 {
                FileType::Ascii
            } else {
                FileType::Binary
            };
            let dim = arg(args, 1)? as u64;
            let version = arg(args, 2)? as u64;
            let via_curve = arg(args, 3)? != 0.0;
            let mut db = DatabaseReader::new();
            db.open_for_read(db_bytes(i)?, ft);
            let r = if via_curve {
                Curve::read_from_db(
                    &solidcore::kernel::Kernel::default(),
                    PARABOLA_TYPE,
                    &mut db,
                    dim,
                    version,
                )
                .and_then(|c| match c.into_kind() {
                    CurveKind::Parabola(p) => Ok(p),
                    _ => Err(SolidError::sm_err("difftest::curve_parabola::read_db")),
                })
            } else {
                Parabola::read_from_db(&mut db, dim, version)
            };
            match r {
                Ok(p) => {
                    out.push(0.0);
                    put_state(&mut out, &p);
                }
                Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
                Err(e) => out.push(status_code(&e)),
            }
        }
        Op::Virtuals => {
            let p = arg3(args, 0)?;
            let c = Curve::from(a);
            let r = c.step_inversion(&solidcore::kernel::Kernel::default(), p);
            out.push(0.0);
            out.push(st(&r));
            out.push(r.unwrap_or(0.0));
            out.extend([flag(c.is_analytic()), flag(c.is_bounded())]);
            out.push(c.type_code() as f64);
        }
    }
    Ok(Ok(out))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_values(i)? {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => Ok(Output::unported(&e)
            .unwrap_or_else(|| Output::err(format!("status_{}", status_code(&e) as i64)))),
    }
}

// ---- generation ---------------------------------------------------------------------

fn vec_scaled(rng: &mut Rng, scale: f64) -> [f64; 3] {
    rng.point3(-scale, scale)
}

fn gen_axis(rng: &mut Rng) -> [f64; 3] {
    match rng.below(6) {
        0 => *rng
            .choose(&[
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [-1.0, 0.0, 0.0],
            ])
            .unwrap_or(&[1.0, 0.0, 0.0]),
        1 => {
            // unit length
            let v = rng.point3(-1.0, 1.0);
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-3);
            [v[0] / l, v[1] / l, v[2] / l]
        }
        _ => {
            let s = *rng.choose(&[1.0, 5.0, 0.01]).unwrap_or(&1.0);
            vec_scaled(rng, s)
        }
    }
}

fn gen_domain(rng: &mut Rng) -> [f64; 2] {
    match rng.below(12) {
        0 => [-1.0, 2.0],
        1 => [0.0, 2.0],
        2 => [-1e4, 1e4],
        3 => [-2000.0, 5.0],
        4 => {
            let v = rng.uniform(-3.0, 3.0);
            [v, v]
        }
        5 => {
            let v = rng.uniform(-3.0, 3.0);
            [v, v + 1e-9 * rng.uniform(0.5, 2.0)]
        }
        6 => [0.0, rng.uniform(0.1, 5.0)],
        7 => {
            let v = rng.uniform(0.1, 50.0);
            [-v, v]
        }
        8 => [rng.uniform(1.0, 3.0), rng.uniform(-3.0, -1.0)],
        _ => {
            let lo = rng.uniform(-10.0, 10.0);
            [lo, lo + rng.uniform(0.01, 20.0)]
        }
    }
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    let scale = *rng.choose(&[1.0, 10.0, 1000.0, 1e-3]).unwrap_or(&1.0);
    let center = vec_scaled(rng, scale);
    let x = gen_axis(rng);
    let mut y = gen_axis(rng);
    if rng.chance(0.03) {
        // parallel axes (SetCanonical fails)
        y = [2.0 * x[0], 2.0 * x[1], 2.0 * x[2]];
    }
    let f = match rng.below(10) {
        0 => 0.5,
        1 => 1.0,
        2 => 0.0,
        3 => -rng.uniform(0.1, 3.0),
        4 => rng.uniform(1e-4, 1e-2),
        5 => rng.uniform(10.0, 1000.0),
        _ => rng.uniform(0.05, 5.0),
    };
    CurveIn {
        kind: if rng.chance(0.15) {
            "canonical".into()
        } else {
            "new".into()
        },
        center,
        x,
        y,
        domain: gen_domain(rng),
        f,
        dim: if rng.chance(0.2) { 2 } else { 3 },
    }
}

fn param_in(rng: &mut Rng, d: [f64; 2]) -> f64 {
    let (lo, hi) = (d[0].min(d[1]).max(-1111.111), d[0].max(d[1]).min(1111.111));
    match rng.below(6) {
        0 => lo,
        1 => hi,
        2 => lo - rng.uniform(0.0, 2.0),
        3 => hi + rng.uniform(0.0, 2.0),
        _ => lo + (hi - lo) * rng.unit(),
    }
}

/// Evaluate the analytic parabola of `c` (after construction) at `t`.
fn point_on(c: &CurveIn, t: f64) -> [f64; 3] {
    build(c)
        .ok()
        .and_then(|p| p.evaluate_step(t, 0).ok())
        .and_then(|v| v.first().copied())
        .map_or([0.0; 3], Vec3::to_array)
}

/// `n p rat dim [kt form]`, control points and knots of the base of `p`, as ASCII
/// lines or binary tokens.
fn base_stream(p: &Parabola, binary: bool, version: u64, data: &mut Vec<String>) {
    let Some(cur) = p.base().gw_nurb() else {
        return;
    };
    let rational = cur
        .cpts()
        .first()
        .is_some_and(|q| q.w != solidcore::nurbs::math::NOW);
    let two_d = p.dim() == 2;
    let n = cur.n() as i64;
    let deg = cur.degree() as i64;
    let rat = i64::from(rational);
    let dim = if two_d { 2 } else { 3 };
    let kt = p.base().knot_type() as i64;
    let form = p.base().form() as i64;
    if binary {
        data.push(format!("l{n} s{deg} s{rat} s{dim}"));
        if version > 32 {
            data.push(format!("l{kt} l{form}"));
        }
        for q in cur.cpts() {
            let mut line = format!("d{:e} d{:e}", q.x, q.y);
            if !two_d {
                let _ = write!(line, " d{:e}", q.z);
            }
            if rational {
                let _ = write!(line, " d{:e}", q.w);
            }
            data.push(line);
        }
        for k in cur.knots() {
            data.push(format!("d{k:e}"));
        }
    } else {
        data.extend([
            n.to_string(),
            deg.to_string(),
            rat.to_string(),
            dim.to_string(),
        ]);
        if version > 32 {
            data.extend([kt.to_string(), form.to_string()]);
        }
        for q in cur.cpts() {
            let mut line = format!("{:e} {:e}", q.x, q.y);
            if !two_d {
                let _ = write!(line, " {:e}", q.z);
            }
            if rational {
                let _ = write!(line, " {:e}", q.w);
            }
            data.push(line);
        }
        for k in cur.knots() {
            data.push(format!("{k:e}"));
        }
    }
}

fn gen_read(rng: &mut Rng, a: &CurveIn) -> (Vec<f64>, String, bool) {
    let binary = rng.chance(0.5);
    let version = *rng.choose(&[33u64, 33, 30]).unwrap_or(&33);
    let dim = *rng.choose(&[3u64, 3, 2]).unwrap_or(&3);
    let via_curve = rng.chance(0.5);
    // A short or missing base stream leaves upstream reading undefined values: use
    // a curve with a NURB.
    let p = build(a)
        .ok()
        .filter(|p| p.base().gw_nurb().is_some())
        .or_else(|| build(&CurveIn::default()).ok());
    let (o, x, y, f, d) = match &p {
        Some(p) => {
            let (a2p, f) = p.canonical();
            let si = p.step_interval();
            (
                a2p.origin(),
                a2p.x_axis(),
                a2p.y_axis(),
                f,
                [si.min(), si.max()],
            )
        }
        None => (Vec3::ZERO, Vec3::X, Vec3::Y, 1.0, [0.0, 1.0]),
    };
    // the analytic data may disagree with the B-spline data
    let (x, y) = if rng.chance(0.1) {
        (Vec3::from(gen_axis(rng)), Vec3::from(gen_axis(rng)))
    } else {
        (x, y)
    };
    let d = if rng.chance(0.1) { [d[1], d[0]] } else { d };
    let mut parts = Vec::new();
    if binary {
        for v in [o, x, y] {
            parts.push(format!("d{:e} d{:e} d{:e}", v.x, v.y, v.z));
        }
        parts.push(format!("d{:e} d{:e}", d[0], d[1]));
        parts.push(format!("d{f:e}"));
    } else {
        for (v, label) in [(o, "Origin"), (x, "X Axis"), (y, "Y Axis")] {
            parts.push(format!("{:e} {:e} {:e} SmParabola {label}", v.x, v.y, v.z));
        }
        parts.push(format!("{:e} {:e} SmParabola domain", d[0], d[1]));
        parts.push(format!("{f:e} SmParabola Focal Distance"));
    }
    if let Some(p) = &p {
        base_stream(p, binary, version, &mut parts);
    }
    let sep = if binary { " " } else { "\n" };
    let mut data = parts.join(sep);
    if !binary {
        data.push('\n');
    }
    (
        vec![flag(binary), dim as f64, version as f64, flag(via_curve)],
        data,
        binary,
    )
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let idx = rng.below(OPS.len() as u64) as usize;
    let (name, op) = OPS.get(idx).copied().unwrap_or(("state", Op::State));
    let a = gen_curve(rng);
    let mut b = gen_curve(rng);
    let mut args = Vec::new();
    let mut data = String::new();
    let mut binary = false;
    match op {
        Op::Eq => {
            let self_eq = rng.chance(0.1);
            args.push(flag(self_eq));
            if rng.chance(0.6) {
                b = a.clone();
                match rng.below(6) {
                    0 => b.f += 1e-12 * rng.uniform(-1.0, 1.0),
                    1 => b.f += 1e-6,
                    2 => b.domain[1] += 1e-11,
                    3 => b.center[0] += 1e-9 * (1.0 + b.center[0].abs()),
                    4 => b.dim = 5 - b.dim,
                    _ => {}
                }
            }
        }
        Op::Adjust | Op::Reverse => {
            let d = if rng.chance(0.2) {
                a.domain
            } else {
                gen_domain(rng)
            };
            args.extend(d);
        }
        Op::Invert | Op::Virtuals => {
            let t = param_in(rng, a.domain);
            let mut p = point_on(&a, t);
            match rng.below(5) {
                0 => {
                    for c in &mut p {
                        *c += 1e-3 * rng.uniform(-1.0, 1.0);
                    }
                }
                1 => {
                    for c in &mut p {
                        *c += 1e-11 * rng.uniform(-1.0, 1.0);
                    }
                }
                _ => {}
            }
            args.extend(p);
            if op == Op::Invert {
                args.push(flag(rng.chance(0.7)));
            }
        }
        Op::EvalStep => {
            args.push(param_in(rng, a.domain));
            args.push(rng.range_i64(0, 5) as f64);
        }
        Op::GpsStep => {
            let mut d = if rng.chance(0.3) {
                a.domain
            } else {
                gen_domain(rng)
            };
            // Upstream solves on a curve without NURB (and crashes) when the interval
            // is unchanged: give such curves another interval.
            if let Ok(p) = build(&a) {
                if p.base().gw_nurb().is_none() {
                    let si = p.step_interval();
                    d = [si.min() - 1.0, si.min() + 0.5];
                }
            }
            args.extend(d);
            args.push(*rng.choose(&[0.0, 3.0, 4.0, 5.0]).unwrap_or(&3.0));
            let t = param_in(rng, d);
            args.extend(point_on(&a, t));
            args.push(*rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-6));
            args.extend([0.0, 0.0]);
            args.push(flag(rng.chance(0.5)));
        }
        Op::Queries => {
            args.push(
                *rng.choose(&[14023.0, 14033.0, 14001.0, 14024.0, 0.0, 22008.0])
                    .unwrap_or(&14023.0),
            );
        }
        Op::Transform => {
            let o = vec_scaled(rng, 10.0);
            let (x, y) = if rng.chance(0.2) {
                ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            } else {
                let x = gen_axis(rng);
                let mut y = gen_axis(rng);
                if rng.chance(0.05) {
                    y = x;
                }
                (x, y)
            };
            let o = if rng.chance(0.2) { [0.0; 3] } else { o };
            args.extend(o);
            args.extend(x);
            args.extend(y);
            let has_scale = rng.chance(0.6);
            args.push(flag(has_scale));
            let s = rng.uniform(0.1, 5.0);
            let scale = match rng.below(5) {
                0 => [1.0, 1.0, 1.0],
                1 => [s, s * (1.0 + 1e-14), s],
                2 => [s, 2.0 * s, s],
                _ => [s, s, s],
            };
            args.extend(scale);
        }
        Op::ReadDb => {
            let (r_args, r_data, r_binary) = gen_read(rng, &a);
            args = r_args;
            data = r_data;
            binary = r_binary;
        }
        _ => {}
    }
    Inputs {
        op: name.to_string(),
        a,
        b,
        args,
        data,
        binary,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.parabola"
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
