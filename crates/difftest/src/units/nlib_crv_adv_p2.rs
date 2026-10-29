//! Runner for unit `nlib.crv_adv_p2`: the public extension entry points
//! (`N_CrvExtendByDist`, `N_CrvExtendByParamDist`), the planar offset second-derivative
//! bound (`N_CrvOffsetGetMax2ndDeriv`), unbounded evaluation
//! (`N_CrvEvalUnboundedPtsAndDerivs`), constrained knot removal
//! (`N_CrvRemoveKnotsDerivConstraints`), refinement to a knot vector
//! (`N_CrvRefineToKnotVector`), the symbolic operators (`N_CrvSumDiffCrv`,
//! `N_CrvNonRatGetFirstDeriv`, `N_CrvRatGetFirstDeriv`, `N_ConstantMultiplyCrv`,
//! `N_LinearMultiplyCrv`, `N_ConstantMultiplyCrv4d`) and the reparametrisations
//! (`N_CrvReparamRat`, `N_CrvReparamWeights`, `N_SrfReparamMultKnots`,
//! `N_SrfReparamWeights`).
//!
//! C++: `solidcore_ffi::units::nlib_crv_adv_p2` (upstream
//! `source/SMLib/src/NL_CrvAdv.cpp`). Rust: `solidcore::nurbs::crv_adv::p2` (methods on
//! `solidcore::nurbs::crv_basic::Curve`).
//!
//! Each case runs one routine (`inputs.op`). Both sides produce the same flat record
//! of doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number, `1000` for upstream's bare error flags: the recursion limits of the
//! extensions and of the Bezier arc length, and `ST_curxcrr`'s size check; `-1` on the
//! Rust side for any other error, which never matches), then:
//!
//! - curve results (extensions, knot removal, sum/difference, derivative curves,
//!   reparametrisations): `dims` (`n p m`), `cpts`, `knots` on success; the in-place
//!   routines (multiplies, `reparam_weights`, `reparam_end_weights`) always, also
//!   after an error;
//! - `offset_max2nd`: `muu`;
//! - `refine_to_knots`: `qw` (`n + r + 2` homogeneous points);
//! - `eval_unbounded`: one `status` per call (stopping after the first error), `d<i>`
//!   (`ndr + 1` points) per successful call, and after all calls succeeded `ext`
//!   (`1` when the extension curve exists) with `gflg`, `dims`, `cpts`, `knots` of it.
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read here as
//! `N_CrvReadFromFilePtr` does). Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_adv::p1::{Continuity, CurveEnd, BARE_FLAG_REASON};
use solidcore::nurbs::crv_adv::p2::{ConstrainedEnds, SumDiff, UnboundedEval};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_geom::ToleranceKind;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::nlib_crv_adv_p2::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

#[path = "fixtures/nlib_crv_adv_p2_smc.rs"]
mod smc;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_adv_p2` runner.
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

/// A parameter: a value, or `{"knot": i, "offset": f}` for `U[i] + f * (U[m] - U[0])`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
enum Param {
    /// Knot `U[knot]` of the case's curve, shifted by `offset` parameter ranges.
    Knot {
        knot: usize,
        #[serde(default)]
        offset: f64,
    },
    /// The parameter value.
    Value(f64),
}

impl Param {
    fn get(self, c: &InlineCrv) -> Result<f64, HarnessError> {
        match self {
            Param::Value(v) => Ok(v),
            Param::Knot { knot, offset } => {
                let k = c
                    .u
                    .get(knot)
                    .copied()
                    .ok_or_else(|| harness(format!("no knot {knot}")))?;
                if offset == 0.0 {
                    return Ok(k);
                }
                let (a, b) = bounds(c);
                Ok(k + offset * (b - a))
            }
        }
    }
}

/// `NL_START` / `NL_END`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EndIn {
    Start,
    End,
}

impl EndIn {
    fn end(self) -> CurveEnd {
        match self {
            EndIn::Start => CurveEnd::Start,
            EndIn::End => CurveEnd::End,
        }
    }
}

/// `NL_G1` / `NL_G2` / `NL_CMAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContIn {
    G1,
    G2,
    Cmax,
}

impl ContIn {
    fn cont(self) -> Continuity {
        match self {
            ContIn::G1 => Continuity::G1,
            ContIn::G2 => Continuity::G2,
            ContIn::Cmax => Continuity::Cmax,
        }
    }
}

/// One call of `N_CrvEvalUnboundedPtsAndDerivs`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct EvalIn {
    u: Param,
    ndr: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    ExtendDist {
        crv: CrvIn,
        dist: f64,
        #[serde(default)]
        relative: bool,
        end: EndIn,
        cont: ContIn,
        #[serde(default)]
        inplace: bool,
    },
    ExtendParam {
        crv: CrvIn,
        par: Param,
        end: EndIn,
        cont: ContIn,
        #[serde(default)]
        inplace: bool,
    },
    OffsetMax2nd {
        crv: CrvIn,
        d: f64,
        normal: [f64; 3],
    },
    EvalUnbounded {
        crv: CrvIn,
        cont: ContIn,
        evals: Vec<EvalIn>,
    },
    RemoveKnotsDeriv {
        crv: CrvIn,
        tol: f64,
        /// Upstream flag `NL_NO..NL_BOTH` (`0..=3`), `4` for an unknown flag.
        whr: i32,
        der: usize,
        #[serde(default)]
        inplace: bool,
    },
    RefineToKnots {
        crv: CrvIn,
        x: Vec<Param>,
    },
    SumDiff {
        crv: CrvIn,
        other: CrvIn,
        #[serde(default)]
        minus: bool,
    },
    NonRatDeriv {
        crv: CrvIn,
        d: usize,
    },
    RatDeriv {
        crv: CrvIn,
    },
    ConstMul {
        crv: CrvIn,
        alpha: f64,
    },
    LinearMul {
        crv: CrvIn,
        s0: f64,
        s1: f64,
    },
    ConstMul4d {
        crv: CrvIn,
        alpha: f64,
    },
    ReparamRat {
        crv: CrvIn,
        alf: f64,
        bet: f64,
        gam: f64,
        del: f64,
        #[serde(default)]
        inplace: bool,
    },
    ReparamWeights {
        crv: CrvIn,
        w: f64,
        c: f64,
        d: f64,
    },
    ReparamMultKnots {
        crv: CrvIn,
        tol: f64,
        #[serde(default)]
        inplace: bool,
    },
    ReparamEndWeights {
        crv: CrvIn,
        wt0: f64,
        wtn: f64,
    },
}

impl Inputs {
    fn crv(&self) -> &CrvIn {
        match self {
            Inputs::ExtendDist { crv, .. }
            | Inputs::ExtendParam { crv, .. }
            | Inputs::OffsetMax2nd { crv, .. }
            | Inputs::EvalUnbounded { crv, .. }
            | Inputs::RemoveKnotsDeriv { crv, .. }
            | Inputs::RefineToKnots { crv, .. }
            | Inputs::SumDiff { crv, .. }
            | Inputs::NonRatDeriv { crv, .. }
            | Inputs::RatDeriv { crv }
            | Inputs::ConstMul { crv, .. }
            | Inputs::LinearMul { crv, .. }
            | Inputs::ConstMul4d { crv, .. }
            | Inputs::ReparamRat { crv, .. }
            | Inputs::ReparamWeights { crv, .. }
            | Inputs::ReparamMultKnots { crv, .. }
            | Inputs::ReparamEndWeights { crv, .. } => crv,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn sum_diff(minus: bool) -> SumDiff {
    if minus {
        SumDiff::Minus
    } else {
        SumDiff::Plus
    }
}

fn kind(relative: bool) -> ToleranceKind {
    if relative {
        ToleranceKind::Relative
    } else {
        ToleranceKind::Absolute
    }
}

// ---- inputs -----------------------------------------------------------------------

/// Reads an upstream `.smc` text file (as `N_CrvReadFromFilePtr`, without its
/// validity check and pruning): `n`, `p`, rational flag, dimension, `n+1` points
/// (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let curve = smc::decode(&text, path)?;
    Ok(InlineCrv {
        cpts: curve.cpts,
        p: curve.p,
        u: curve.u,
    })
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

/// Splits `k` values at `at` into `key`.
fn vals(flat: &[f64], at: &mut usize, key: &str, k: usize, out: Output) -> Result<Output, HarnessError> {
    Ok(out.values(key, take(flat, at, k)?.to_vec()))
}

/// Splits a curve (`n p m`, control points, knots) at `at`.
fn crv_vals(flat: &[f64], at: &mut usize, out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?.to_vec();
    let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
    let m = count(dims.last().copied().unwrap_or(f64::NAN))?;
    let out = out.values("dims", dims);
    let out = vals(flat, at, "cpts", 4 * (n + 1), out)?;
    vals(flat, at, "knots", m + 1, out)
}

fn to_output(i: &Inputs, c: &InlineCrv, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let a = &mut at;
    let mut out = Output::new();
    if let Inputs::EvalUnbounded { evals, .. } = i {
        let mut statuses = Vec::new();
        let mut ok = true;
        for (k, e) in evals.iter().enumerate() {
            let s = take(flat, a, 1)?.to_vec();
            ok = s == [0.0];
            statuses.extend(s);
            if !ok {
                break;
            }
            out = vals(flat, a, &format!("d{k}"), 3 * (e.ndr + 1), out)?;
        }
        out = out.values("status", statuses);
        if ok {
            let ext = take(flat, a, 1)?.to_vec();
            let used = ext == [1.0];
            out = out.values("ext", ext);
            if used {
                out = vals(flat, a, "gflg", 1, out)?;
                out = crv_vals(flat, a, out)?;
            }
        }
    } else {
        let status = take(flat, a, 1)?.to_vec();
        let ok = status == [0.0];
        out = out.values("status", status);
        match i {
            Inputs::ConstMul { .. }
            | Inputs::LinearMul { .. }
            | Inputs::ConstMul4d { .. }
            | Inputs::ReparamWeights { .. }
            | Inputs::ReparamEndWeights { .. } => out = crv_vals(flat, a, out)?,
            _ if !ok => {}
            Inputs::OffsetMax2nd { .. } => out = vals(flat, a, "muu", 1, out)?,
            Inputs::RefineToKnots { x, .. } => {
                let k = 4 * (c.cpts.len() + x.len());
                out = vals(flat, a, "qw", k, out)?;
            }
            _ => out = crv_vals(flat, a, out)?,
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

fn end_flag(e: EndIn) -> f64 {
    f64::from(e.end().flag())
}

fn cont_flag(c: ContIn) -> f64 {
    f64::from(c.cont().flag())
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve(i.crv())?;
    let mut o = Vec::new();
    encode_crv(&c, &mut o)?;
    let op = match i {
        Inputs::ExtendDist {
            dist,
            relative,
            end,
            cont,
            inplace,
            ..
        } => {
            o.extend([
                *dist,
                f64::from(kind(*relative).flag()),
                end_flag(*end),
                cont_flag(*cont),
                flag(*inplace),
            ]);
            Op::ExtendDist
        }
        Inputs::ExtendParam {
            par,
            end,
            cont,
            inplace,
            ..
        } => {
            o.extend([par.get(&c)?, end_flag(*end), cont_flag(*cont), flag(*inplace)]);
            Op::ExtendParam
        }
        Inputs::OffsetMax2nd { d, normal, .. } => {
            o.push(*d);
            o.extend_from_slice(normal);
            Op::OffsetMax2nd
        }
        Inputs::EvalUnbounded { cont, evals, .. } => {
            o.extend([cont_flag(*cont), evals.len() as f64]);
            for e in evals {
                o.extend([e.u.get(&c)?, e.ndr as f64]);
            }
            Op::EvalUnbounded
        }
        Inputs::RemoveKnotsDeriv {
            tol,
            whr,
            der,
            inplace,
            ..
        } => {
            o.extend([*tol, f64::from(*whr), *der as f64, flag(*inplace)]);
            Op::RemoveKnotsDeriv
        }
        Inputs::RefineToKnots { x, .. } => {
            o.push(x.len() as f64);
            for k in x {
                o.push(k.get(&c)?);
            }
            Op::RefineToKnots
        }
        Inputs::SumDiff { other, minus, .. } => {
            encode_crv(&resolve(other)?, &mut o)?;
            o.push(f64::from(sum_diff(*minus).flag()));
            Op::SumDiff
        }
        Inputs::NonRatDeriv { d, .. } => {
            o.push(*d as f64);
            Op::NonRatDeriv
        }
        Inputs::RatDeriv { .. } => Op::RatDeriv,
        Inputs::ConstMul { alpha, .. } => {
            o.push(*alpha);
            Op::ConstMul
        }
        Inputs::LinearMul { s0, s1, .. } => {
            o.extend([*s0, *s1]);
            Op::LinearMul
        }
        Inputs::ConstMul4d { alpha, .. } => {
            o.push(*alpha);
            Op::ConstMul4d
        }
        Inputs::ReparamRat {
            alf,
            bet,
            gam,
            del,
            inplace,
            ..
        } => {
            o.extend([*alf, *bet, *gam, *del, flag(*inplace)]);
            Op::ReparamRat
        }
        Inputs::ReparamWeights { w, c: cc, d, .. } => {
            o.extend([*w, *cc, *d]);
            Op::ReparamWeights
        }
        Inputs::ReparamMultKnots { tol, inplace, .. } => {
            o.extend([*tol, flag(*inplace)]);
            Op::ReparamMultKnots
        }
        Inputs::ReparamEndWeights { wt0, wtn, .. } => {
            o.extend([*wt0, *wtn]);
            Op::ReparamEndWeights
        }
    };
    match ffi::eval(op, &o) {
        Ok(flat) => to_output(i, &c, &flat),
        Err(FfiError::Status(s)) => Err(harness(format!("shim status {s}"))),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// Status of a Rust error: the NLib number, `1000` for upstream's bare error flags
/// (recursion limits of the extensions and the Bezier arc length, the size check of
/// `ST_curxcrr`), `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        SolidError::NotConverged { operation, .. }
            if operation.starts_with("nurbs::bezier")
                || operation.ends_with("extend_by_param_dist_at_depth")
                || operation.ends_with("extend_by_dist_at_depth") =>
        {
            1000.0
        }
        SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. } if *reason == BARE_FLAG_REASON => 1000.0,
        _ => -1.0,
    }
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn push_crv(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for &q in c.cpts() {
        o.extend(q.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn bad_knot(_: HarnessError) -> SolidError {
    SolidError::InvalidInput {
        operation: "difftest::nlib_crv_adv_p2",
        reason: "bad knot index",
    }
}

/// Runs `i` on the Rust side; returns the record (status first).
fn rust_record(i: &Inputs, c: &InlineCrv, cur: &Curve) -> Vec<f64> {
    let mut o = vec![0.0];
    // In-place routines: status, then the curve whatever the status.
    let inplace = |f: &dyn Fn(&mut Curve) -> Result<(), SolidError>| {
        let mut q = cur.clone();
        let st = match f(&mut q) {
            Ok(()) => 0.0,
            Err(e) => status(&e),
        };
        let mut r = vec![st];
        push_crv(&mut r, &q);
        r
    };
    match i {
        Inputs::ConstMul { alpha, .. } => {
            return inplace(&|q| {
                q.constant_multiply(*alpha);
                Ok(())
            })
        }
        Inputs::LinearMul { s0, s1, .. } => return inplace(&|q| q.linear_multiply(*s0, *s1)),
        Inputs::ConstMul4d { alpha, .. } => {
            return inplace(&|q| {
                q.constant_multiply_4d(*alpha);
                Ok(())
            })
        }
        Inputs::ReparamWeights { w, c: cc, d, .. } => {
            return inplace(&|q| q.reparam_weights(*w, *cc, *d))
        }
        Inputs::ReparamEndWeights { wt0, wtn, .. } => {
            return inplace(&|q| q.reparam_end_weights(*wt0, *wtn))
        }
        Inputs::EvalUnbounded { cont, evals, .. } => {
            let mut r = Vec::new();
            let mut st = UnboundedEval::default();
            for e in evals {
                let res = e
                    .u
                    .get(c)
                    .map_err(bad_knot)
                    .and_then(|u| cur.eval_unbounded_pts_and_derivs(u, e.ndr, cont.cont(), &mut st));
                match res {
                    Ok(d) => {
                        r.push(0.0);
                        for p in d {
                            push3(&mut r, p);
                        }
                    }
                    Err(err) => {
                        r.push(status(&err));
                        return r;
                    }
                }
            }
            match &st.ext {
                None => r.push(0.0),
                Some(ext) => {
                    r.extend([1.0, flag(st.g1_closed)]);
                    push_crv(&mut r, ext);
                }
            }
            return r;
        }
        _ => {}
    }
    match rust_body(i, c, cur, &mut o) {
        Ok(()) => o,
        Err(e) => vec![status(&e)],
    }
}

fn rust_body(i: &Inputs, c: &InlineCrv, cur: &Curve, o: &mut Vec<f64>) -> Result<(), SolidError> {
    match i {
        Inputs::ExtendDist {
            dist,
            relative,
            end,
            cont,
            ..
        } => push_crv(o, &cur.extend_by_dist(*dist, kind(*relative), end.end(), cont.cont())?),
        Inputs::ExtendParam { par, end, cont, .. } => {
            let q = cur.extend_by_param_dist(par.get(c).map_err(bad_knot)?, end.end(), cont.cont())?;
            push_crv(o, &q);
        }
        Inputs::OffsetMax2nd { d, normal, .. } => {
            o.push(cur.offset_max_2nd_deriv(*d, Vec3::new(normal[0], normal[1], normal[2]))?);
        }
        Inputs::RemoveKnotsDeriv { tol, whr, der, .. } => {
            let q = cur.remove_knots_deriv_constraints(*tol, ConstrainedEnds::from_flag(*whr), *der)?;
            push_crv(o, &q);
        }
        Inputs::RefineToKnots { x, .. } => {
            let x = x
                .iter()
                .map(|k| k.get(c))
                .collect::<Result<Vec<_>, _>>()
                .map_err(bad_knot)?;
            for q in cur.refine_to_knot_vector(&x)? {
                o.extend(q.to_array());
            }
        }
        Inputs::SumDiff { other, minus, .. } => {
            let oc = resolve(other).map_err(bad_knot)?;
            let other = to_curve(&oc).map_err(bad_knot)?;
            push_crv(o, &cur.sum_diff(&other, sum_diff(*minus))?);
        }
        Inputs::NonRatDeriv { d, .. } => push_crv(o, &cur.non_rat_first_deriv(*d)?),
        Inputs::RatDeriv { .. } => push_crv(o, &cur.rat_first_deriv()?),
        Inputs::ReparamRat {
            alf, bet, gam, del, ..
        } => push_crv(o, &cur.reparam_rat(*alf, *bet, *gam, *del)?),
        Inputs::ReparamMultKnots { tol, .. } => push_crv(o, &cur.reparam_mult_knots(*tol)?),
        Inputs::ConstMul { .. }
        | Inputs::LinearMul { .. }
        | Inputs::ConstMul4d { .. }
        | Inputs::ReparamWeights { .. }
        | Inputs::ReparamEndWeights { .. }
        | Inputs::EvalUnbounded { .. } => {}
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve(i.crv())?;
    let cur = to_curve(&c)?;
    let rec = rust_record(i, &c, &cur);
    to_output(i, &c, &rec)
}

// ---- generated cases --------------------------------------------------------------

/// A random clamped curve: degree 1..=5 (rarely up to 8), `n - p` interior knots
/// with multiplicities up to `p`, 2D or 3D, rational or not.
fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let p = if rng.chance(0.1) {
        6 + rng.below(3) as usize
    } else {
        1 + rng.below(5) as usize
    };
    let rational = rng.chance(0.3);
    gen_crv_deg(rng, p, rational)
}

/// A random clamped curve of degree 1..=5.
fn gen_crv_low(rng: &mut Rng) -> InlineCrv {
    let p = 1 + rng.below(5) as usize;
    let rational = rng.chance(0.3);
    gen_crv_deg(rng, p, rational)
}

/// A random clamped curve of degree `p`.
fn gen_crv_deg(rng: &mut Rng, p: usize, rational: bool) -> InlineCrv {
    let n = p + rng.below(9) as usize;
    let (a, len) = if rng.chance(0.3) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    };
    let b = a + len;
    let integral = rng.chance(0.3);
    // Distinct values strictly inside (a, b), each with multiplicity 1..=p.
    let mut inner: Vec<f64> = Vec::new();
    let mut used: Vec<f64> = Vec::new();
    let mut remaining = n - p;
    while remaining > 0 {
        let mut v = a + 0.5 * len;
        for attempt in 0..32 {
            v = if integral && attempt < 16 {
                a + len * (1.0 + rng.below(15) as f64) / 16.0
            } else {
                rng.uniform(a, b)
            };
            if v > a && v < b && !used.contains(&v) {
                break;
            }
        }
        if v <= a || v >= b || used.contains(&v) {
            break;
        }
        used.push(v);
        let mult = if rng.chance(0.25) {
            1 + rng.below(p.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining -= mult;
    }
    inner.sort_by(f64::total_cmp);
    let n = p + inner.len();
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(b, p + 1));
    let three_d = rng.chance(0.8);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-10.0, 10.0);
            let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
            let z = if three_d { z * w } else { NOZ };
            [x * w, y * w, z, if rational { w } else { NOW }]
        })
        .collect();
    InlineCrv { cpts, p, u }
}

/// A random rotation (orthonormal columns) from three uniform angles.
fn gen_frame(rng: &mut Rng) -> [[f64; 3]; 3] {
    let (a, b, c) = (
        rng.uniform(0.0, std::f64::consts::TAU),
        rng.uniform(0.0, std::f64::consts::TAU),
        rng.uniform(0.0, std::f64::consts::TAU),
    );
    let (sa, ca) = a.sin_cos();
    let (sb, cb) = b.sin_cos();
    let (sc, cc) = c.sin_cos();
    [
        [ca * cb, ca * sb * sc - sa * cc, ca * sb * cc + sa * sc],
        [sa * cb, sa * sb * sc + ca * cc, sa * sb * cc - ca * sc],
        [-sb, cb * sc, cb * cc],
    ]
}

fn apply(f: &[[f64; 3]; 3], o: [f64; 3], x: f64, y: f64) -> [f64; 3] {
    [
        o[0] + f[0][0] * x + f[0][1] * y,
        o[1] + f[1][0] * x + f[1][1] * y,
        o[2] + f[2][0] * x + f[2][1] * y,
    ]
}

/// A rational quadratic circular arc (or full circle), in the `z = 0` plane (2D or
/// 3D) or in a random plane.
fn gen_circle(rng: &mut Rng) -> InlineCrv {
    let full = rng.chance(0.3);
    let sweep = if full {
        2.0 * std::f64::consts::PI
    } else {
        rng.uniform(0.2, 2.0 * std::f64::consts::PI - 0.2)
    };
    let narcs = if full { 4 } else { 1 + (sweep / (0.5 * std::f64::consts::PI)) as usize };
    let narcs = narcs.min(4);
    let r = rng.uniform(0.1, 20.0);
    let o = rng.point3(-10.0, 10.0);
    let planar = rng.chance(0.4);
    let f = if planar {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    } else {
        gen_frame(rng)
    };
    let o = if planar { [o[0], o[1], 0.0] } else { o };
    let three_d = !planar || rng.chance(0.5);
    let dth = sweep / narcs as f64;
    let w1 = (0.5 * dth).cos();
    let start = rng.uniform(0.0, 6.0);
    let mut cpts = Vec::new();
    let mut push = |pt: [f64; 3], w: f64| {
        let z = if three_d { pt[2] * w } else { NOZ };
        cpts.push([pt[0] * w, pt[1] * w, z, w]);
    };
    let at = |t: f64, s: f64| apply(&f, o, s * t.cos(), s * t.sin());
    push(at(start, r), 1.0);
    for k in 0..narcs {
        let t0 = start + k as f64 * dth;
        let t1 = t0 + dth;
        push(at(0.5 * (t0 + t1), r / w1), w1);
        push(at(t1, r), 1.0);
    }
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-3.0, 3.0);
        (a, a + rng.uniform(0.5, 5.0))
    };
    let mut u = vec![a; 3];
    for k in 1..narcs {
        let v = a + (b - a) * k as f64 / narcs as f64;
        u.extend([v, v]);
    }
    u.extend([b; 3]);
    InlineCrv { cpts, p: 2, u }
}

/// A curve whose control points lie on a line, monotone along it.
fn gen_line(rng: &mut Rng) -> InlineCrv {
    let mut c = gen_crv(rng);
    let a = rng.point3(-10.0, 10.0);
    let d = rng.point3(-5.0, 5.0);
    let n = c.cpts.len();
    let eps = *rng.choose(&[0.0, 0.0, 1e-12, 1e-6, 1e-3]).unwrap_or(&0.0);
    let mut ts: Vec<f64> = (0..n).map(|_| rng.unit()).collect();
    ts.sort_by(f64::total_cmp);
    if let Some(t) = ts.first_mut() {
        *t = 0.0;
    }
    if let Some(t) = ts.last_mut() {
        *t = 1.0;
    }
    for (cp, t) in c.cpts.iter_mut().zip(ts) {
        let w = if cp[3] == NOW { 1.0 } else { cp[3] };
        let p = [
            a[0] + t * d[0] + eps * rng.uniform(-1.0, 1.0),
            a[1] + t * d[1] + eps * rng.uniform(-1.0, 1.0),
            a[2] + t * d[2] + eps * rng.uniform(-1.0, 1.0),
        ];
        cp[0] = p[0] * w;
        cp[1] = p[1] * w;
        if cp[2] != NOZ {
            cp[2] = p[2] * w;
        }
    }
    c
}

/// A curve with a run of coincident control points (a degenerate span).
fn gen_degen(rng: &mut Rng) -> InlineCrv {
    let mut c = gen_crv(rng);
    let n = c.cpts.len();
    let start = match rng.below(3) {
        0 => 0,
        1 => n.saturating_sub(2),
        _ => rng.below(n as u64) as usize,
    };
    let len = 1 + rng.below((c.p + 2) as u64) as usize;
    let base = c.cpts.get(start).copied().unwrap_or([0.0; 4]);
    for k in start..(start + len).min(n) {
        if let Some(cp) = c.cpts.get_mut(k) {
            let w = if cp[3] == NOW { 1.0 } else { cp[3] };
            let bw = if base[3] == NOW { 1.0 } else { base[3] };
            cp[0] = base[0] / bw * w;
            cp[1] = base[1] / bw * w;
            if cp[2] != NOZ {
                cp[2] = base[2] / bw * w;
            }
        }
    }
    c
}

/// A planar curve in the `z = 0` plane (2D or 3D), for the planar offset bound.
fn gen_planar(rng: &mut Rng) -> InlineCrv {
    let mut c = if rng.chance(0.3) { gen_circle(rng) } else { gen_crv(rng) };
    let three_d = rng.chance(0.5);
    for cp in &mut c.cpts {
        cp[2] = if three_d { 0.0 } else { NOZ };
    }
    c
}

/// A piecewise Bezier curve: every interior knot has multiplicity `p` (sometimes
/// one less), as `N_SrfReparamMultKnots` expects.
fn gen_piecewise(rng: &mut Rng) -> InlineCrv {
    let p = 1 + rng.below(4) as usize;
    let segs = 1 + rng.below(5) as usize;
    let a = rng.uniform(-3.0, 3.0);
    let mut u = vec![a; p + 1];
    let mut t = a;
    for _ in 1..segs {
        t += rng.uniform(0.2, 2.0);
        let mult = if p > 1 && rng.chance(0.2) { p - 1 } else { p };
        u.extend(std::iter::repeat_n(t, mult));
        if rng.chance(0.15) {
            t += rng.uniform(0.1, 1.0);
            u.push(t);
        }
    }
    t += rng.uniform(0.2, 2.0);
    u.extend(std::iter::repeat_n(t, p + 1));
    let n = u.len() - p - 2;
    let rational = rng.chance(0.3);
    let three_d = rng.chance(0.7);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-10.0, 10.0);
            let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
            let z = if three_d { z * w } else { NOZ };
            [x * w, y * w, z, if rational { w } else { NOW }]
        })
        .collect();
    InlineCrv { cpts, p, u }
}

fn gen_any_crv(rng: &mut Rng) -> InlineCrv {
    match rng.below(10) {
        0 | 1 => gen_circle(rng),
        2 => gen_line(rng),
        3 => gen_degen(rng),
        _ => gen_crv(rng),
    }
}

/// A non-rational version of `c` (Euclidean control points).
fn non_rational(mut c: InlineCrv) -> InlineCrv {
    for cp in &mut c.cpts {
        if cp[3] != NOW {
            let w = cp[3];
            cp[0] /= w;
            cp[1] /= w;
            if cp[2] != NOZ {
                cp[2] /= w;
            }
            cp[3] = NOW;
        }
    }
    c
}

fn is_rational(c: &InlineCrv) -> bool {
    c.cpts.first().is_some_and(|cp| cp[3] != NOW)
}

/// `o` rescaled to the parameter range of `c`, with interior knots within
/// `NEAR_KNOT_REL` ranges of a knot of `c` snapped onto it (the rational branch of
/// `N_CrvSumDiffCrv` corrupts the heap otherwise; see the ffi).
fn same_range(mut o: InlineCrv, c: &InlineCrv) -> InlineCrv {
    let (a, b) = bounds(c);
    let (oa, ob) = bounds(&o);
    let p = o.p;
    let m = o.u.len() - 1;
    for (k, u) in o.u.iter_mut().enumerate() {
        *u = if k <= p {
            a
        } else if k >= m - p {
            b
        } else {
            a + (*u - oa) / (ob - oa) * (b - a)
        };
    }
    let sep = solidcore_ffi::units::nlib_crv_adv_p2::NEAR_KNOT_REL * (b - a);
    let snapped: Vec<f64> = o
        .u
        .iter()
        .map(|&u| {
            c.u.iter()
                .copied()
                .find(|&x| x != u && (x - u).abs() <= sep)
                .unwrap_or(u)
        })
        .collect();
    // Snap only when that keeps the knots' multiplicities within the degree.
    let ok = snapped
        .iter()
        .all(|x| snapped.iter().filter(|&&y| y == *x).count() <= p || *x == a || *x == b);
    if ok && snapped.windows(2).all(|w| w.first() <= w.last()) {
        o.u = snapped;
    }
    o
}

/// Distinct interior knots of the curve.
fn interior(c: &InlineCrv) -> Vec<f64> {
    let p = c.p;
    let mut v: Vec<f64> = c
        .u
        .get(p + 1..c.u.len() - p - 1)
        .unwrap_or(&[])
        .to_vec();
    v.dedup();
    v
}

fn bounds(c: &InlineCrv) -> (f64, f64) {
    (
        c.u.first().copied().unwrap_or(0.0),
        c.u.last().copied().unwrap_or(1.0),
    )
}

fn euclid(c: &InlineCrv, k: usize) -> [f64; 3] {
    let [x, y, z, w] = c.cpts.get(k).copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn polygon_length(c: &InlineCrv) -> f64 {
    (1..c.cpts.len()).map(|k| dist3(euclid(c, k - 1), euclid(c, k))).sum()
}

/// Whether the extension end tangents are computable without the third point the
/// ffi demands for curves with fewer than three control points.
fn tangents_ok(c: &InlineCrv, start: bool) -> bool {
    let n = c.cpts.len() - 1;
    if n >= 2 {
        return true;
    }
    let d = if start {
        dist3(euclid(c, 0), euclid(c, 1))
    } else {
        dist3(euclid(c, n), euclid(c, n - 1))
    };
    d > 1e-150 && d < 1e150
}

fn gen_end(rng: &mut Rng) -> EndIn {
    if rng.chance(0.5) {
        EndIn::Start
    } else {
        EndIn::End
    }
}

fn gen_cont(rng: &mut Rng) -> ContIn {
    match rng.below(3) {
        0 => ContIn::G1,
        1 => ContIn::G2,
        _ => ContIn::Cmax,
    }
}

/// A curve whose extension tangents are computable for `cont` at both ends.
fn ext_crv(rng: &mut Rng, c: InlineCrv, cont: ContIn) -> InlineCrv {
    if cont == ContIn::Cmax || (tangents_ok(&c, true) && tangents_ok(&c, false)) {
        return c;
    }
    let c = gen_crv(rng);
    if tangents_ok(&c, true) && tangents_ok(&c, false) {
        c
    } else {
        gen_circle(rng)
    }
}

fn gen_extend_dist(rng: &mut Rng, c: InlineCrv) -> Inputs {
    let end = gen_end(rng);
    let cont = gen_cont(rng);
    let c = ext_crv(rng, c, cont);
    let rel = *rng
        .choose(&[0.01, 0.05, 0.1, 0.3, 0.5, 0.95, 1.5, 2.5])
        .unwrap_or(&0.3);
    let len = polygon_length(&c);
    let relative = rng.chance(0.5) || len <= 0.0;
    let dist = if relative { rel } else { rel * len };
    Inputs::ExtendDist {
        crv: CrvIn::Inline(c),
        dist,
        relative,
        end,
        cont,
        inplace: rng.chance(0.3),
    }
}

fn gen_extend_param(rng: &mut Rng, c: InlineCrv) -> Inputs {
    let end = gen_end(rng);
    let cont = gen_cont(rng);
    let c = ext_crv(rng, c, cont);
    let f = *rng
        .choose(&[1e-3, 0.01, 0.1, 0.25, 0.5, 0.9, 0.95, 1.5, 3.0])
        .unwrap_or(&0.25);
    let (a, b) = bounds(&c);
    let par = if end == EndIn::Start {
        a - f * (b - a)
    } else {
        b + f * (b - a)
    };
    let inplace = rng.chance(0.3) && !(cont == ContIn::G2 && f > 0.9213);
    Inputs::ExtendParam {
        crv: CrvIn::Inline(c),
        par: Param::Value(par),
        end,
        cont,
        inplace,
    }
}

/// A parameter for unbounded evaluation: inside, at an end, or up to a few
/// parameter ranges outside.
fn gen_unbounded_u(rng: &mut Rng, c: &InlineCrv) -> f64 {
    let (a, b) = bounds(c);
    let len = b - a;
    match rng.below(6) {
        0 => rng.uniform(a, b),
        1 => {
            if rng.chance(0.5) {
                a
            } else {
                b
            }
        }
        2 => a - len * *rng.choose(&[1e-6, 0.01, 0.3, 0.9]).unwrap_or(&0.3),
        3 => b + len * *rng.choose(&[1e-6, 0.01, 0.3, 0.9]).unwrap_or(&0.3),
        4 => a - len * rng.uniform(0.0, 4.0),
        _ => b + len * rng.uniform(0.0, 4.0),
    }
}

fn gen_unbounded(rng: &mut Rng, c: InlineCrv) -> Inputs {
    let cont = gen_cont(rng);
    let c = ext_crv(rng, c, cont);
    let k = 1 + rng.below(6) as usize;
    let evals = (0..k)
        .map(|_| EvalIn {
            u: Param::Value(gen_unbounded_u(rng, &c)),
            ndr: rng.below(4) as usize,
        })
        .collect();
    Inputs::EvalUnbounded {
        crv: CrvIn::Inline(c),
        cont,
        evals,
    }
}

/// A curve with removable knots: `c` refined with a few interior knots (or `c`
/// itself), occasionally perturbed so the removals stop at the tolerance.
fn removable(rng: &mut Rng, c: InlineCrv) -> InlineCrv {
    let Ok(cur) = to_curve(&c) else {
        return c;
    };
    let (a, b) = bounds(&c);
    let k = rng.below(5) as usize;
    let mut x: Vec<f64> = (0..k).map(|_| rng.uniform(a, b)).collect();
    x.sort_by(f64::total_cmp);
    x.retain(|v| *v > a && *v < b && !c.u.contains(v));
    x.dedup();
    let refined = if x.is_empty() {
        cur
    } else {
        match cur.refine(&x) {
            Ok(r) => r,
            Err(_) => cur,
        }
    };
    let eps = *rng.choose(&[0.0, 0.0, 1e-9, 1e-5, 1e-2]).unwrap_or(&0.0);
    let cpts = refined
        .cpts()
        .iter()
        .map(|q| {
            let mut v = q.to_array();
            v[0] += eps * rng.uniform(-1.0, 1.0);
            v[1] += eps * rng.uniform(-1.0, 1.0);
            v
        })
        .collect();
    InlineCrv {
        cpts,
        p: refined.degree(),
        u: refined.knots().to_vec(),
    }
}

fn gen_refine_knots(rng: &mut Rng, c: &InlineCrv) -> Vec<Param> {
    let (a, b) = bounds(c);
    let k = rng.below(6) as usize;
    let mut x: Vec<f64> = (0..k)
        .map(|_| {
            let knots = interior(c);
            if !knots.is_empty() && rng.chance(0.25) {
                knots
                    .get(rng.below(knots.len() as u64) as usize)
                    .copied()
                    .unwrap_or(a)
            } else {
                rng.uniform(a, b)
            }
        })
        .collect();
    if rng.chance(0.05) {
        x.push(if rng.chance(0.5) { a } else { b });
    }
    x.sort_by(f64::total_cmp);
    // Keep the multiplicities within the degree.
    let mut out: Vec<f64> = Vec::new();
    for v in x {
        let mult = c.u.iter().filter(|&&u| u == v).count() + out.iter().filter(|&&u| u == v).count();
        if (v <= a || v >= b) || mult < c.p {
            out.push(v);
        }
    }
    out.into_iter().map(Param::Value).collect()
}

/// A second curve for the sum/difference: same or different degree and knots,
/// sometimes another parameter range, rational or not.
fn gen_other(rng: &mut Rng, c: &InlineCrv) -> InlineCrv {
    match rng.below(4) {
        0 => {
            // Same knots and degree, other control points.
            let mut o = c.clone();
            for cp in &mut o.cpts {
                let [x, y, z] = rng.point3(-10.0, 10.0);
                let w = if cp[3] == NOW { 1.0 } else { cp[3] };
                cp[0] = x * w;
                cp[1] = y * w;
                if cp[2] != NOZ {
                    cp[2] = z * w;
                }
            }
            o
        }
        // Another curve over the same range.
        1 => same_range(gen_crv_low(rng), c),
        _ => gen_crv_low(rng),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let c = gen_any_crv(rng);
    match rng.below(32) {
        0 => gen_extend_dist(rng, c),
        1 => gen_extend_param(rng, c),
        2 | 3 => {
            let c = gen_planar(rng);
            let normal = if rng.chance(0.7) {
                [0.0, 0.0, if rng.chance(0.5) { 1.0 } else { -1.0 }]
            } else {
                rng.point3(-1.0, 1.0)
            };
            Inputs::OffsetMax2nd {
                crv: CrvIn::Inline(c),
                d: rng.uniform(-3.0, 3.0),
                normal,
            }
        }
        4..=7 => gen_unbounded(rng, c),
        8..=10 => {
            let c = removable(rng, c);
            let tol = *rng.choose(&[1e-12, 1e-8, 1e-4, 1e-2, 0.5]).unwrap_or(&1e-4);
            Inputs::RemoveKnotsDeriv {
                crv: CrvIn::Inline(c),
                tol,
                whr: rng.below(5) as i32,
                der: rng.below(4) as usize,
                inplace: rng.chance(0.3),
            }
        }
        11 | 12 => {
            let x = gen_refine_knots(rng, &c);
            Inputs::RefineToKnots {
                crv: CrvIn::Inline(c),
                x,
            }
        }
        13..=16 => {
            let c = if rng.chance(0.3) { non_rational(c) } else { c };
            let mut other = gen_other(rng, &c);
            if rng.chance(0.3) {
                other = non_rational(other);
            }
            if is_rational(&c) || is_rational(&other) {
                other = same_range(other, &c);
            }
            Inputs::SumDiff {
                crv: CrvIn::Inline(c),
                other: CrvIn::Inline(other),
                minus: rng.chance(0.5),
            }
        }
        17 | 18 => {
            let c = if rng.chance(0.8) { non_rational(c) } else { c };
            let d = rng.below((c.p + 2) as u64) as usize;
            Inputs::NonRatDeriv {
                crv: CrvIn::Inline(c),
                d,
            }
        }
        19..=21 => Inputs::RatDeriv {
            crv: CrvIn::Inline(c),
        },
        22 => Inputs::ConstMul {
            crv: CrvIn::Inline(c),
            alpha: rng.uniform(-3.0, 3.0),
        },
        23 => {
            let s0 = rng.uniform(-3.0, 3.0);
            let s1 = if rng.chance(0.2) { s0 } else { rng.uniform(-3.0, 3.0) };
            Inputs::LinearMul {
                crv: CrvIn::Inline(c),
                s0,
                s1,
            }
        }
        24 => Inputs::ConstMul4d {
            crv: CrvIn::Inline(c),
            alpha: rng.uniform(-3.0, 3.0),
        },
        25 | 26 => {
            // s = (alf u + bet) / (gam u + del) with a denominator that stays
            // positive on the knot range (mostly).
            let (a, b) = bounds(&c);
            let gam = rng.uniform(-0.5, 0.5);
            let del = 1.0 - gam * a.min(b).min(0.0) + gam.abs() * (b - a) + rng.uniform(0.0, 2.0);
            let alf = rng.uniform(0.2, 3.0);
            let bet = if rng.chance(0.1) {
                alf * del / gam.max(1e-3) // degenerate: alf del - bet gam <= 0
            } else {
                rng.uniform(-2.0, 2.0)
            };
            Inputs::ReparamRat {
                crv: CrvIn::Inline(c),
                alf,
                bet,
                gam,
                del,
                inplace: rng.chance(0.3),
            }
        }
        27 | 28 => {
            let (a, b) = bounds(&c);
            let (cc, dd) = if rng.chance(0.5) {
                (a, b)
            } else {
                let cc = rng.uniform(-3.0, 3.0);
                (cc, cc + rng.uniform(0.5, 4.0))
            };
            Inputs::ReparamWeights {
                crv: CrvIn::Inline(c),
                w: rng.uniform(0.3, 3.0),
                c: cc,
                d: dd,
            }
        }
        29 | 30 => {
            let c = if rng.chance(0.7) { gen_piecewise(rng) } else { c };
            Inputs::ReparamMultKnots {
                crv: CrvIn::Inline(c),
                tol: *rng.choose(&[1e-3, 1e-5, 1e-7]).unwrap_or(&1e-5),
                inplace: rng.chance(0.3),
            }
        }
        _ => Inputs::ReparamEndWeights {
            crv: CrvIn::Inline(c),
            wt0: rng.uniform(0.3, 3.0),
            wtn: rng.uniform(0.3, 3.0),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_adv_p2"
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
    use super::*;
    use std::collections::BTreeMap;

    /// The generator reaches success of every routine and no Rust-only error.
    #[test]
    fn generated_cases_cover_statuses() {
        let mut tally: BTreeMap<(String, i64), usize> = BTreeMap::new();
        let mut rng = Rng::new(11);
        for _ in 0..1500 {
            let i = gen_case(&mut rng);
            let op = serde_json::to_value(&i).unwrap()["op"].as_str().unwrap().to_string();
            let c = resolve(i.crv()).unwrap();
            let cur = to_curve(&c).unwrap();
            let rec = rust_record(&i, &c, &cur);
            let st = rec[0];
            *tally.entry((op, st as i64)).or_default() += 1;
        }
        eprintln!("{tally:?}");
        for op in [
            "extend_dist",
            "extend_param",
            "offset_max2nd",
            "eval_unbounded",
            "remove_knots_deriv",
            "refine_to_knots",
            "sum_diff",
            "non_rat_deriv",
            "rat_deriv",
            "const_mul",
            "linear_mul",
            "const_mul4d",
            "reparam_rat",
            "reparam_weights",
            "reparam_mult_knots",
            "reparam_end_weights",
        ] {
            assert!(tally.contains_key(&(op.to_string(), 0)), "{op} never succeeds");
        }
        assert!(!tally.keys().any(|(_, s)| *s == -1), "unexpected Rust-only errors");
    }
}

#[cfg(test)]
mod dump {
    use super::*;
    use crate::case::CaseContext;

    /// `CASE=<i> cargo test ... dump_case -- --ignored --nocapture` prints generated case `i`.
    #[test]
    #[ignore = "debugging aid"]
    fn dump_case() {
        let i: usize = std::env::var("CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("nlib.crv_adv_p2", i));
        println!("{}", serde_json::to_string(&gen_case(&mut rng)).unwrap());
    }
}
