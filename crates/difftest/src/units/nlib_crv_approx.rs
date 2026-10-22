//! Runner for unit `nlib.crv_approx`: the number of control points of a
//! non-rational circle (`N_CalcNumCPtsToApproxArc`), the non-rational fit of a
//! circular arc (`N_ApproxCircArcWithCrvData`) and the non-rational approximation of
//! a NURBS curve (`N_ApproxNurbsWithNonRatCrv`).
//!
//! C++: `solidcore_ffi::units::nlib_crv_approx` (upstream
//! `source/SMLib/src/NL_CrvApprox.cpp`). Rust: `solidcore::nurbs::crv_approx`.
//!
//! Each case runs one routine (`inputs.op`). Both sides produce the same flat record
//! of doubles, split into keys: `status` (`0` ok, else the `NL_*_ERR` number, `1000`
//! for upstream's bare error flags (the recursion limit of the Bezier arc length);
//! `-1` on the Rust side for any other error, which never matches), then on success
//! `nc` (`num_cpts`) or the curve as `dims` (`n p m`), `cpts`, `knots`.
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`). Every run has
//! at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_adv::p1::BARE_FLAG_REASON;
use solidcore::nurbs::crv_approx::{
    approx_circ_arc_with_crv_data, approx_nurbs_with_non_rat_crv, arc_cpts_function,
    calc_num_cpts_to_approx_arc, CircArcData,
};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::crv_fit::p3::ParamSource;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_crv_approx::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `nlib.crv_approx` runner.
pub struct Runner;

/// An inline curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A curve input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CrvIn {
    /// An upstream `.smc` file (absolute path after fixture expansion).
    File { smc: String },
    /// Inline data.
    Inline(InlineCrv),
}

/// The `par` flag of `N_ApproxNurbsWithNonRatCrv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParIn {
    Uniform,
    ChordLength,
    Centripetal,
    Inherited,
}

impl ParIn {
    fn source(self) -> ParamSource {
        match self {
            ParIn::Uniform => ParamSource::Method(ParamMethod::Uniform),
            ParIn::ChordLength => ParamSource::Method(ParamMethod::ChordLength),
            ParIn::Centripetal => ParamSource::Method(ParamMethod::Centripetal),
            ParIn::Inherited => ParamSource::Inherited,
        }
    }
}

/// How `num_cpts` gets its surface function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SfnMode {
    /// `sfn` NULL, `flg = NL_NO`.
    Local,
    /// `sfn` NULL, `flg = NL_YES`.
    Kept,
    /// `sfn` built by an earlier call and passed in.
    Given,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    NumCpts {
        tol: f64,
        ang: f64,
        deg: usize,
        #[serde(default = "default_mode")]
        mode: SfnMode,
    },
    CircArc {
        cen: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        rad: f64,
        #[serde(rename = "as")]
        as_: f64,
        ae: f64,
        /// Start point; on the circle when absent.
        #[serde(default)]
        ps: Option<[f64; 3]>,
        /// End point; on the circle when absent.
        #[serde(default)]
        pe: Option<[f64; 3]>,
        deg: usize,
        nc: usize,
    },
    NonRat {
        crv: CrvIn,
        tol: f64,
        deg: usize,
        par: ParIn,
        #[serde(default)]
        tan: bool,
    },
}

fn default_mode() -> SfnMode {
    SfnMode::Local
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| {
            t.parse::<f64>()
                .map_err(|e| harness(format!("{path}: {e}")))
        })
        .collect()
}

fn int(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || v < 0.0 {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.smc` text file (as `N_CrvReadFromFilePtr`, without its
/// validity check and pruning): `n`, `p`, rational flag, dimension, `n+1` points
/// (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let p = int(path, num()?)?;
    let rat = num()? != 0.0;
    let dim = int(path, num()?)?;
    let mut cpts = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let (x, y) = (num()?, num()?);
        let z = if dim == 3 { num()? } else { NOZ };
        let w = if rat { num()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineCrv { cpts, p, u })
}

fn resolve(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(c) => Ok(c.clone()),
    }
}

fn to_curve(c: &InlineCrv) -> Result<Curve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))
}

/// The point at `ang` degrees on the circle (`cen + rad cos X + rad sin Y` with the
/// axes as given, as the runner's default end points; identical on both sides).
fn circle_pt(cen: [f64; 3], x: [f64; 3], y: [f64; 3], rad: f64, ang: f64) -> [f64; 3] {
    let t = ang.to_radians();
    let (s, c) = t.sin_cos();
    let nx = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
    let ny = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt();
    let (nx, ny) = (if nx > 0.0 { nx } else { 1.0 }, if ny > 0.0 { ny } else { 1.0 });
    [
        cen[0] + rad * c * x[0] / nx + rad * s * y[0] / ny,
        cen[1] + rad * c * x[1] / nx + rad * s * y[1] / ny,
        cen[2] + rad * c * x[2] / nx + rad * s * y[2] / ny,
    ]
}

/// The arc data with default end points filled in.
#[allow(clippy::too_many_arguments)]
fn arc_ends(
    cen: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    rad: f64,
    as_: f64,
    ae: f64,
    ps: Option<[f64; 3]>,
    pe: Option<[f64; 3]>,
) -> ([f64; 3], [f64; 3]) {
    (
        ps.unwrap_or_else(|| circle_pt(cen, x, y, rad, as_)),
        pe.unwrap_or_else(|| circle_pt(cen, x, y, rad, ae)),
    )
}

// ---- records ----------------------------------------------------------------------

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let part = at
        .checked_add(k)
        .and_then(|end| flat.get(*at..end))
        .ok_or_else(|| harness(format!("record too short ({} values)", flat.len())))?;
    *at += k;
    Ok(part)
}

fn count(x: f64) -> Result<usize, HarnessError> {
    if x.fract() != 0.0 || !(0.0..=1e8).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let a = &mut at;
    let status = take(flat, a, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    if ok {
        if let Inputs::NumCpts { .. } = i {
            out = out.values("nc", take(flat, a, 1)?.to_vec());
        } else {
            let dims = take(flat, a, 3)?.to_vec();
            let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
            let m = count(dims.last().copied().unwrap_or(f64::NAN))?;
            out = out.values("dims", dims);
            out = out.values("cpts", take(flat, a, 4 * (n + 1))?.to_vec());
            out = out.values("knots", take(flat, a, m + 1)?.to_vec());
        }
    }
    if at != flat.len() {
        return Err(harness(format!(
            "record has {} values, layout used {at}",
            flat.len()
        )));
    }
    Ok(out)
}

// ---- C++ side ---------------------------------------------------------------------

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty curve"))?;
    let m = c.u.len().checked_sub(1).ok_or_else(|| harness("empty knots"))?;
    o.extend([n as f64, c.p as f64, m as f64]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = match i {
        Inputs::NumCpts {
            tol,
            ang,
            deg,
            mode,
        } => {
            let m = match mode {
                SfnMode::Local => 0.0,
                SfnMode::Kept => 1.0,
                SfnMode::Given => 2.0,
            };
            o.extend([*tol, *ang, *deg as f64, m]);
            Op::NumCpts
        }
        Inputs::CircArc {
            cen,
            x,
            y,
            rad,
            as_,
            ae,
            ps,
            pe,
            deg,
            nc,
        } => {
            let (ps, pe) = arc_ends(*cen, *x, *y, *rad, *as_, *ae, *ps, *pe);
            o.extend(cen);
            o.extend(x);
            o.extend(y);
            o.extend([*rad, *as_, *ae]);
            o.extend(ps);
            o.extend(pe);
            o.extend([*deg as f64, *nc as f64]);
            Op::CircArcData
        }
        Inputs::NonRat {
            crv,
            tol,
            deg,
            par,
            tan,
        } => {
            encode_crv(&resolve(crv)?, &mut o)?;
            o.extend([
                *tol,
                *deg as f64,
                f64::from(par.source().flag()),
                if *tan { 1.0 } else { 0.0 },
            ]);
            Op::NonRat
        }
    };
    Ok((op, o))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, o) = cpp_input(i)?;
    match ffi::eval(op, &o) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::Status(s)) => Err(harness(format!("shim status {s}"))),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// Status of a Rust error: the NLib number, `1000` for upstream's bare error flags
/// (the recursion limit of the Bezier arc length), `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        SolidError::NotConverged { operation, .. } if operation.starts_with("nurbs::bezier") => {
            1000.0
        }
        SolidError::InvalidInput { reason, .. } if *reason == BARE_FLAG_REASON => 1000.0,
        _ => -1.0,
    }
}

fn push_crv(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for &q in c.cpts() {
        o.extend(q.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn rust_record(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let res: Result<Vec<f64>, SolidError> = match i {
        Inputs::NumCpts {
            tol,
            ang,
            deg,
            mode,
        } => match mode {
            SfnMode::Local | SfnMode::Kept => calc_num_cpts_to_approx_arc(*tol, *ang, *deg, None),
            SfnMode::Given => arc_cpts_function(*deg)
                .and_then(|f| calc_num_cpts_to_approx_arc(*tol, *ang, *deg, Some(&f))),
        }
        .map(|nc| vec![0.0, nc as f64]),
        Inputs::CircArc {
            cen,
            x,
            y,
            rad,
            as_,
            ae,
            ps,
            pe,
            deg,
            nc,
        } => {
            let (ps, pe) = arc_ends(*cen, *x, *y, *rad, *as_, *ae, *ps, *pe);
            let arc = CircArcData {
                center: v3(*cen),
                x: v3(*x),
                y: v3(*y),
                radius: *rad,
                start_angle: *as_,
                end_angle: *ae,
                start: v3(ps),
                end: v3(pe),
            };
            approx_circ_arc_with_crv_data(&arc, *deg, *nc).map(|c| {
                let mut o = vec![0.0];
                push_crv(&mut o, &c);
                o
            })
        }
        Inputs::NonRat {
            crv,
            tol,
            deg,
            par,
            tan,
        } => {
            let cur = to_curve(&resolve(crv)?)?;
            approx_nurbs_with_non_rat_crv(&cur, *tol, *deg, par.source(), *tan).map(|c| {
                let mut o = vec![0.0];
                push_crv(&mut o, &c);
                o
            })
        }
    };
    Ok(match res {
        Ok(o) => o,
        Err(e) => vec![status(&e)],
    })
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_record(i)?)
}

// ---- generator --------------------------------------------------------------------

fn log_uniform(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    (rng.uniform(lo.ln(), hi.ln())).exp()
}

/// A random clamped curve: degree `1..=4`, interior knots possibly repeated (up to
/// the degree), optionally rational and/or 2D.
fn gen_curve(rng: &mut Rng) -> InlineCrv {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(7) as usize;
    let rational = rng.chance(0.5);
    let two_d = rng.chance(0.3);
    let scale = log_uniform(rng, 0.1, 100.0);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-scale, scale);
            let z = if two_d { NOZ } else { z };
            let w = if rational { rng.uniform(0.3, 3.0) } else { NOW };
            if rational {
                let zz = if two_d { NOZ } else { z * w };
                [x * w, y * w, zz, w]
            } else {
                [x, y, z, w]
            }
        })
        .collect();
    let a = rng.uniform(-5.0, 5.0);
    let len = log_uniform(rng, 0.1, 20.0);
    // Interior knots: n - p sorted values in (0, 1), some repeated (multiplicity <= p).
    let mut raw: Vec<f64> = (0..n - p).map(|_| rng.uniform(0.02, 0.98)).collect();
    raw.sort_by(f64::total_cmp);
    let mut inner: Vec<f64> = Vec::with_capacity(raw.len());
    let mut run = 1;
    for k in raw {
        let next = match inner.last().copied() {
            Some(prev) if run < p && rng.chance(0.3) => {
                run += 1;
                prev
            }
            Some(prev) => {
                run = 1;
                if k == prev {
                    (k + 1e-3).min(0.99)
                } else {
                    k
                }
            }
            None => k,
        };
        inner.push(next);
    }
    inner.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(inner.iter().map(|&t| a + t * len));
    u.extend(std::iter::repeat_n(a + len, p + 1));
    InlineCrv { cpts, p, u }
}

/// The exact rational quarter / half / full circles (degree 2, weights `cos(45)`).
fn gen_circle(rng: &mut Rng) -> InlineCrv {
    let r = log_uniform(rng, 0.1, 50.0);
    let [cx, cy, cz] = rng.point3(-10.0, 10.0);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarters = 1 + rng.below(4) as usize;
    let dirs = [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (1.0, 0.0)];
    let mut cpts = Vec::new();
    for (k, &(dx, dy)) in dirs.iter().take(2 * quarters + 1).enumerate() {
        let wk = if k % 2 == 1 { w } else { 1.0 };
        let (x, y) = (cx + r * dx, cy + r * dy);
        cpts.push([x * wk, y * wk, cz * wk, wk]);
    }
    let mut u = vec![0.0; 3];
    for q in 1..quarters {
        let t = q as f64 / quarters as f64;
        u.extend([t, t]);
    }
    u.extend([1.0; 3]);
    InlineCrv { cpts, p: 2, u }
}

fn gen_non_rat(rng: &mut Rng) -> Inputs {
    let crv = if rng.chance(0.2) {
        gen_circle(rng)
    } else {
        gen_curve(rng)
    };
    let tol = if rng.chance(0.05) {
        1.0
    } else {
        log_uniform(rng, 1e-5, 0.2)
    };
    let deg = 1 + rng.below(5) as usize;
    let par = *rng
        .choose(&[
            ParIn::Uniform,
            ParIn::ChordLength,
            ParIn::Centripetal,
            ParIn::Inherited,
        ])
        .unwrap_or(&ParIn::Inherited);
    Inputs::NonRat {
        crv: CrvIn::Inline(crv),
        tol,
        deg,
        par,
        tan: rng.chance(0.5),
    }
}

fn gen_num_cpts(rng: &mut Rng) -> Inputs {
    let tol = if rng.chance(0.2) {
        *rng.choose(&[1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 0.01, 0.1, 1.0, 3.0])
            .unwrap_or(&0.1)
    } else {
        log_uniform(rng, 1e-8, 5.0)
    };
    let ang = if rng.chance(0.2) {
        *rng.choose(&[-10.0, 0.0, 1e-9, 45.0, 90.0, 180.0, 359.99, 360.0, 360.5])
            .unwrap_or(&90.0)
    } else {
        rng.uniform(0.0, 360.0)
    };
    let deg = if rng.chance(0.1) {
        *rng.choose(&[0, 1, 5]).unwrap_or(&1)
    } else {
        2 + rng.below(3) as usize
    };
    let mode = *rng
        .choose(&[SfnMode::Local, SfnMode::Kept, SfnMode::Given])
        .unwrap_or(&SfnMode::Local);
    Inputs::NumCpts {
        tol,
        ang,
        deg,
        mode,
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn gen_circ_arc(rng: &mut Rng) -> Inputs {
    let cen = rng.point3(-20.0, 20.0);
    let (x, y) = loop {
        let x = rng.point3(-2.0, 2.0);
        let y = if rng.chance(0.3) {
            // Perpendicular-ish frames, as from real circles.
            let len = (dot(x, x)).sqrt();
            [-x[1] * len, x[0] * len, rng.uniform(-0.1, 0.1)]
        } else {
            rng.point3(-2.0, 2.0)
        };
        let (nx, ny) = (dot(x, x).sqrt(), dot(y, y).sqrt());
        if nx > 0.1 && ny > 0.1 && (dot(x, y) / (nx * ny)).abs() < 0.9 {
            break (x, y);
        }
    };
    let rad = if rng.chance(0.03) {
        *rng.choose(&[0.0, -1.0]).unwrap_or(&0.0)
    } else {
        log_uniform(rng, 0.01, 100.0)
    };
    let as_ = rng.uniform(-360.0, 360.0);
    let sweep = if rng.chance(0.15) {
        *rng.choose(&[360.0, 359.99995, 90.0, 180.0, 270.0])
            .unwrap_or(&90.0)
    } else {
        rng.uniform(1.0, 359.0)
    };
    let ae = if rng.chance(0.2) {
        as_ + sweep - 360.0 // ae < as: upstream adds 360
    } else {
        as_ + sweep
    };
    let deg = if rng.chance(0.05) {
        *rng.choose(&[1, 5]).unwrap_or(&1)
    } else {
        2 + rng.below(3) as usize
    };
    let mut nc = if rng.chance(0.3) {
        deg
    } else if rng.chance(0.05) {
        deg.saturating_sub(1)
    } else {
        deg + rng.below(40) as usize
    };
    // Degree 2 parabolic arcs need clearly non-parallel end tangents.
    let eff = {
        let mut e = ae;
        if e <= as_ {
            e += 360.0;
        }
        e - as_
    };
    if deg == 2 && nc == 2 && ((eff - 180.0).abs() < 5.0 || eff > 355.0) {
        nc = 3;
    }
    let perturb = |rng: &mut Rng, p: [f64; 3]| {
        let d = rng.point3(-1e-3, 1e-3);
        [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
    };
    let (ps0, pe0) = arc_ends(cen, x, y, rad, as_, ae, None, None);
    let ps = if rng.chance(0.2) {
        Some(perturb(rng, ps0))
    } else {
        None
    };
    let pe = if rng.chance(0.2) {
        Some(perturb(rng, pe0))
    } else {
        None
    };
    Inputs::CircArc {
        cen,
        x,
        y,
        rad,
        as_,
        ae,
        ps,
        pe,
        deg,
        nc,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0..=1 => gen_num_cpts(rng),
        2..=5 => gen_circ_arc(rng),
        _ => gen_non_rat(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_approx"
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
        let i = case.parse()?;
        // Run only what the FFI accepts, as the C++ side: its guards refuse inputs
        // upstream (and the faithful port) never finish on (fuzz Rust-side hangs).
        let (code, input) = cpp_input(&i)?;
        ffi::validate(code, &input).map_err(|e| HarnessError::new(e.to_string()))?;
        rust(&i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn oversampled_approximation_fails_softly() {
        // fuzz05147 (`tests/fuzz_corpus/nlib.crv_approx.json`): the FFI refuses its
        // mixed-sign weights, so the runner never hands it to the port; run directly,
        // the approximation sampled ten million points and its knot removal ran for
        // minutes (issue #504). It now fails past `MAX_APPROX_SAMPLES`.
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fuzz_corpus/nlib.crv_approx.json"
        ))
        .unwrap();
        let case = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "fuzz05147_rust_hang")
            .unwrap();
        let input: Inputs = serde_json::from_value(case["inputs"].clone()).unwrap();
        let out = rust(&input).unwrap();
        assert_eq!(out.values["status"], vec![-1.0], "{out:?}");
    }


    /// The generator reaches success of every routine and no Rust-only error.
    #[test]
    fn generated_cases_cover_statuses() {
        let mut tally: BTreeMap<(String, i64), usize> = BTreeMap::new();
        let mut rng = Rng::new(7);
        for _ in 0..600 {
            let i = gen_case(&mut rng);
            let op = serde_json::to_value(&i).unwrap()["op"]
                .as_str()
                .unwrap()
                .to_string();
            let rec = rust_record(&i).unwrap();
            *tally.entry((op, rec[0] as i64)).or_default() += 1;
        }
        eprintln!("{tally:?}");
        for op in ["num_cpts", "circ_arc", "non_rat"] {
            assert!(tally.contains_key(&(op.to_string(), 0)), "{op} never succeeds");
        }
        assert!(!tally.keys().any(|(_, s)| *s == -1), "unexpected Rust-only errors");
    }
}
