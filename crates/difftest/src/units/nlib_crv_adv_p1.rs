//! Runner for unit `nlib.crv_adv_p1`: homogeneous points and derivatives
//! (`N_CrvEvalPt`, `N_CrvDerivsAtKnot`), evenly spaced points
//! (`N_CrvEvalEvenSpacedPts`), the second-derivative bound (`N_CrvGetMax2ndDeriv`),
//! planar offset derivatives (`N_CrvPlanarOffsetGetDeriv`) and the extensions by arc
//! length (`ST_curxcdr`) and by parameter (`ST_curxcrr`).
//!
//! C++: `solidcore_ffi::units::nlib_crv_adv_p1` (upstream
//! `source/SMLib/src/NL_CrvAdv.cpp`; the file-private extension routines are reached
//! through their one-line public wrappers `N_CrvExtendByDist` /
//! `N_CrvExtendByParamDist`, which call them at depth 0). Rust:
//! `solidcore::nurbs::crv_adv::p1` (methods on `solidcore::nurbs::crv_basic::Curve`).
//!
//! Each case runs one routine (`inputs.op`) on one curve. Both sides produce the same
//! flat record of doubles, split into keys by [`to_output`]: `status` (`0` ok, else
//! the `NL_*_ERR` number, `1000` for upstream's bare error flags: the recursion
//! limits of the extensions and of the Bezier arc length, and `ST_curxcrr`'s size
//! check; `-1` on the Rust side for any other error, which never matches), then on
//! success:
//!
//! - `eval_pt`: `cw` (homogeneous `x y z w`);
//! - `derivs_at_knot`: `dw` (`der + 1` homogeneous points);
//! - `even_spaced`: `pts` (`n + 1` points), `pars`;
//! - `max_2nd_deriv`: `muu`;
//! - `planar_offset`: `od1`, `od2`;
//! - `extend_dist` / `extend_param`: `dims` (`n p m`), `cpts`, `knots` of the result.
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read here as
//! `N_CrvReadFromFilePtr` does). Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_adv::p1::{Continuity, CurveEnd, BARE_FLAG_REASON};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_geom::ToleranceKind;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::nlib_crv_adv_p1::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_adv_p1` runner.
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

/// A parameter: a value, `{"knot": i}` for the curve's knot `U[i]`, or
/// `{"knot": i, "offset": f}` for `U[i] + f * (U[m] - U[0])`.
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
                let (a, b) = (
                    c.u.first().copied().unwrap_or(0.0),
                    c.u.last().copied().unwrap_or(1.0),
                );
                Ok(k + offset * (b - a))
            }
        }
    }
}

/// `NL_LEFT` / `NL_RIGHT`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SideIn {
    #[default]
    Left,
    Right,
}

impl SideIn {
    fn side(self) -> Side {
        match self {
            SideIn::Left => Side::Left,
            SideIn::Right => Side::Right,
        }
    }
    fn flag(self) -> f64 {
        f64::from(self.side().flag())
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    EvalPt {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
    },
    DerivsAtKnot {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
        der: usize,
    },
    EvenSpaced {
        crv: CrvIn,
        u0: Param,
        u1: Param,
        n: usize,
        tol: f64,
    },
    Max2ndDeriv {
        crv: CrvIn,
    },
    PlanarOffset {
        crv: CrvIn,
        u: Param,
        d: f64,
        normal: [f64; 3],
        #[serde(default)]
        side: SideIn,
    },
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
}

impl Inputs {
    fn crv(&self) -> &CrvIn {
        match self {
            Inputs::EvalPt { crv, .. }
            | Inputs::DerivsAtKnot { crv, .. }
            | Inputs::EvenSpaced { crv, .. }
            | Inputs::Max2ndDeriv { crv }
            | Inputs::PlanarOffset { crv, .. }
            | Inputs::ExtendDist { crv, .. }
            | Inputs::ExtendParam { crv, .. } => crv,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[path = "fixtures/nlib_crv_adv_p1_smc.rs"]
mod smc;

/// Reads an upstream `.smc` text file (as `N_CrvReadFromFilePtr`, without its
/// validity check and pruning): `n`, `p`, rational flag, dimension, `n+1` points
/// (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let c = smc::decode(&text, path)?;
    Ok(InlineCrv {
        cpts: c.cpts,
        p: c.p,
        u: c.u,
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

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    let a = &mut at;
    if ok {
        match i {
            Inputs::EvalPt { .. } => out = vals(flat, a, "cw", 4, out)?,
            Inputs::DerivsAtKnot { der, .. } => out = vals(flat, a, "dw", 4 * (der + 1), out)?,
            Inputs::EvenSpaced { n, .. } => {
                out = vals(flat, a, "pts", 3 * (n + 1), out)?;
                out = vals(flat, a, "pars", n + 1, out)?;
            }
            Inputs::Max2ndDeriv { .. } => out = vals(flat, a, "muu", 1, out)?,
            Inputs::PlanarOffset { .. } => {
                out = vals(flat, a, "od1", 3, out)?;
                out = vals(flat, a, "od2", 3, out)?;
            }
            Inputs::ExtendDist { .. } | Inputs::ExtendParam { .. } => {
                let dims = take(flat, a, 3)?.to_vec();
                let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
                let m = count(dims.last().copied().unwrap_or(f64::NAN))?;
                out = out.values("dims", dims);
                out = vals(flat, a, "cpts", 4 * (n + 1), out)?;
                out = vals(flat, a, "knots", m + 1, out)?;
            }
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

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve(i.crv())?;
    let mut o = Vec::new();
    encode_crv(&c, &mut o)?;
    let op = match i {
        Inputs::EvalPt { u, side, .. } => {
            o.extend([u.get(&c)?, side.flag()]);
            Op::EvalPt
        }
        Inputs::DerivsAtKnot { u, side, der, .. } => {
            o.extend([u.get(&c)?, side.flag(), *der as f64]);
            Op::DerivsAtKnot
        }
        Inputs::EvenSpaced { u0, u1, n, tol, .. } => {
            o.extend([u0.get(&c)?, u1.get(&c)?, *n as f64, *tol]);
            Op::EvenSpaced
        }
        Inputs::Max2ndDeriv { .. } => Op::Max2ndDeriv,
        Inputs::PlanarOffset {
            u, d, normal, side, ..
        } => {
            o.extend([u.get(&c)?, *d]);
            o.extend_from_slice(normal);
            o.push(side.flag());
            Op::PlanarOffset
        }
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
                f64::from(if *relative {
                    ToleranceKind::Relative
                } else {
                    ToleranceKind::Absolute
                }
                .flag()),
                f64::from(end.end().flag()),
                f64::from(cont.cont().flag()),
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
            o.extend([
                par.get(&c)?,
                f64::from(end.end().flag()),
                f64::from(cont.cont().flag()),
                flag(*inplace),
            ]);
            Op::ExtendParam
        }
    };
    match ffi::eval(op, &o) {
        Ok(flat) => to_output(i, &flat),
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
                || (operation.ends_with("extend_by_dist_at_depth")) =>
        {
            1000.0
        }
        SolidError::InvalidInput { reason, .. } if *reason == BARE_FLAG_REASON => 1000.0,
        _ => -1.0,
    }
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn push4(o: &mut Vec<f64>, c: CPoint) {
    o.extend(c.to_array());
}

fn push_crv(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for &q in c.cpts() {
        push4(o, q);
    }
    o.extend_from_slice(c.knots());
}

fn rust_body(i: &Inputs, c: &InlineCrv, cur: &Curve, o: &mut Vec<f64>) -> Result<(), SolidError> {
    let bad = |_| SolidError::InvalidInput {
        operation: "difftest::nlib_crv_adv_p1",
        reason: "bad knot index",
    };
    match i {
        Inputs::EvalPt { u, side, .. } => {
            push4(o, cur.eval_pt(u.get(c).map_err(bad)?, side.side())?);
        }
        Inputs::DerivsAtKnot { u, side, der, .. } => {
            for d in cur.derivs_at_knot(u.get(c).map_err(bad)?, side.side(), *der)? {
                push4(o, d);
            }
        }
        Inputs::EvenSpaced { u0, u1, n, tol, .. } => {
            let r = cur.eval_even_spaced_pts(u0.get(c).map_err(bad)?, u1.get(c).map_err(bad)?, *n, *tol)?;
            for p in &r.points {
                push3(o, *p);
            }
            o.extend_from_slice(&r.params);
        }
        Inputs::Max2ndDeriv { .. } => o.push(cur.max_2nd_deriv()?),
        Inputs::PlanarOffset {
            u, d, normal, side, ..
        } => {
            let (od1, od2) = cur.planar_offset_derivs(
                u.get(c).map_err(bad)?,
                *d,
                Vec3::new(normal[0], normal[1], normal[2]),
                side.side(),
            )?;
            push3(o, od1);
            push3(o, od2);
        }
        Inputs::ExtendDist {
            dist,
            relative,
            end,
            cont,
            ..
        } => {
            let kind = if *relative {
                ToleranceKind::Relative
            } else {
                ToleranceKind::Absolute
            };
            let q = cur.extend_by_dist_at_depth(*dist, kind, end.end(), cont.cont(), 0)?;
            push_crv(o, &q);
        }
        Inputs::ExtendParam { par, end, cont, .. } => {
            let q = cur.extend_by_param_dist_at_depth(par.get(c).map_err(bad)?, end.end(), cont.cont(), 0)?;
            push_crv(o, &q);
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve(i.crv())?;
    let cur = to_curve(&c)?;
    let mut o = vec![0.0];
    match rust_body(i, &c, &cur, &mut o) {
        Ok(()) => to_output(i, &o),
        Err(e) => match Output::unported(&e) {
            Some(d) => Ok(d),
            None => to_output(i, &[status(&e)]),
        },
    }
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
    let rational = rng.chance(0.3);
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

/// A rational quadratic circular arc (or full circle) in a random plane.
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
    let planar = rng.chance(0.3);
    let f = if planar {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    } else {
        gen_frame(rng)
    };
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
    let mut u = vec![0.0; 3];
    for k in 1..narcs {
        let v = k as f64 / narcs as f64;
        u.extend([v, v]);
    }
    u.extend([1.0; 3]);
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

/// A curve with a run of coincident control points (a degenerate span), often at an
/// end (exercising the end-tangent fallbacks).
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

fn gen_any_crv(rng: &mut Rng) -> InlineCrv {
    match rng.below(10) {
        0 | 1 => gen_circle(rng),
        2 => gen_line(rng),
        3 => gen_degen(rng),
        _ => gen_crv(rng),
    }
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

/// A parameter: mostly inside the knot range (random, an interior knot or an end),
/// sometimes just outside.
fn gen_u(rng: &mut Rng, c: &InlineCrv) -> f64 {
    let (a, b) = bounds(c);
    let knots = interior(c);
    let roll = rng.unit();
    if roll < 0.1 {
        if rng.chance(0.5) {
            a
        } else {
            b
        }
    } else if roll < 0.3 && !knots.is_empty() {
        knots.get(rng.below(knots.len() as u64) as usize).copied().unwrap_or(a)
    } else if roll < 0.33 {
        if rng.chance(0.5) {
            a - 0.01 * (b - a)
        } else {
            b + 0.01 * (b - a)
        }
    } else {
        rng.uniform(a, b)
    }
}

fn gen_side(rng: &mut Rng) -> SideIn {
    if rng.chance(0.5) {
        SideIn::Left
    } else {
        SideIn::Right
    }
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

fn gen_extend_dist(rng: &mut Rng, mut c: InlineCrv) -> Inputs {
    let end = gen_end(rng);
    let cont = gen_cont(rng);
    if cont != ContIn::Cmax && !tangents_ok(&c, end == EndIn::Start) {
        c = gen_crv(rng);
        if !tangents_ok(&c, end == EndIn::Start) {
            c = gen_circle(rng);
        }
    }
    let rel = if rng.chance(0.02) {
        *rng.choose(&[25.0, 30.0]).unwrap_or(&30.0)
    } else {
        *rng
            .choose(&[0.01, 0.05, 0.1, 0.2, 0.3, 0.5, 0.8, 0.95, 1.0, 1.5, 2.5, 4.0])
            .unwrap_or(&0.3)
    };
    // Absolute distances scale with the control polygon (relative ones on a curve
    // without length).
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

fn gen_extend_param(rng: &mut Rng, mut c: InlineCrv) -> Inputs {
    let end = gen_end(rng);
    let cont = gen_cont(rng);
    if cont != ContIn::Cmax && !tangents_ok(&c, end == EndIn::Start) {
        c = gen_crv(rng);
        if !tangents_ok(&c, end == EndIn::Start) {
            c = gen_circle(rng);
        }
    }
    let f = if rng.chance(0.03) {
        *rng.choose(&[6.5, 8.0]).unwrap_or(&8.0)
    } else {
        *rng
            .choose(&[1e-3, 0.01, 0.1, 0.25, 0.5, 0.9, 0.95, 1.5, 3.0])
            .unwrap_or(&0.25)
    };
    let (a, b) = bounds(&c);
    let par = if end == EndIn::Start {
        a - f * (b - a)
    } else {
        b + f * (b - a)
    };
    // In place, a G2 extension that recurses is undefined upstream (see the ffi).
    let inplace = rng.chance(0.3) && !(cont == ContIn::G2 && f > 0.9213);
    Inputs::ExtendParam {
        crv: CrvIn::Inline(c),
        par: Param::Value(par),
        end,
        cont,
        inplace,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let c = gen_any_crv(rng);
    match rng.below(20) {
        0 | 1 => {
            let u = gen_u(rng, &c);
            Inputs::EvalPt {
                crv: CrvIn::Inline(c),
                u: Param::Value(u),
                side: gen_side(rng),
            }
        }
        2 | 3 => {
            let u = gen_u(rng, &c);
            Inputs::DerivsAtKnot {
                crv: CrvIn::Inline(c),
                u: Param::Value(u),
                side: gen_side(rng),
                der: rng.below(7) as usize,
            }
        }
        4..=6 => {
            let (u0, u1) = if rng.chance(0.2) {
                bounds(&c)
            } else {
                (gen_u(rng, &c), gen_u(rng, &c))
            };
            let n = if rng.chance(0.05) {
                rng.below(2) as usize
            } else {
                2 + rng.below(30) as usize
            };
            let tol = *rng.choose(&[0.1, 1e-2, 1e-3, 1e-6, 0.0]).unwrap_or(&1e-3);
            Inputs::EvenSpaced {
                crv: CrvIn::Inline(c),
                u0: Param::Value(u0),
                u1: Param::Value(u1),
                n,
                tol,
            }
        }
        7 | 8 => Inputs::Max2ndDeriv { crv: CrvIn::Inline(c) },
        9 | 10 => {
            let u = gen_u(rng, &c);
            let normal = if rng.chance(0.5) {
                [0.0, 0.0, if rng.chance(0.5) { 1.0 } else { -1.0 }]
            } else {
                rng.point3(-1.0, 1.0)
            };
            Inputs::PlanarOffset {
                crv: CrvIn::Inline(c),
                u: Param::Value(u),
                d: rng.uniform(-3.0, 3.0),
                normal,
                side: gen_side(rng),
            }
        }
        11..=15 => gen_extend_dist(rng, c),
        _ => gen_extend_param(rng, c),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_adv_p1"
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

    /// The generator reaches success and failure of the routines.
    #[test]
    fn generated_cases_cover_statuses() {
        let mut tally: BTreeMap<(String, i64), usize> = BTreeMap::new();
        let mut rng = Rng::new(11);
        for _ in 0..600 {
            let i = gen_case(&mut rng);
            let op = serde_json::to_value(&i).unwrap()["op"].as_str().unwrap().to_string();
            let c = resolve(i.crv()).unwrap();
            let cur = to_curve(&c).unwrap();
            let mut o = vec![0.0];
            let st = match rust_body(&i, &c, &cur, &mut o) {
                Ok(()) => o[0],
                Err(e) => status(&e),
            };
            *tally.entry((op, st as i64)).or_default() += 1;
        }
        eprintln!("{tally:?}");
        for op in [
            "eval_pt",
            "derivs_at_knot",
            "even_spaced",
            "max2nd_deriv",
            "planar_offset",
            "extend_dist",
            "extend_param",
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
        let mut rng = Rng::new(CaseContext::seed_for("nlib.crv_adv_p1", i));
        println!("{}", serde_json::to_string(&gen_case(&mut rng)).unwrap());
    }

    /// Prints the Rust status of every fixture case.
    #[test]
    #[ignore = "debugging aid"]
    fn fixture_statuses() {
        let ctx = CaseContext { root: crate::case::repo_root(), seeds: 0 };
        for case in ctx.fixture_cases("nlib.crv_adv_p1").unwrap() {
            let i: Inputs = case.parse().unwrap();
            let c = resolve(i.crv()).unwrap();
            let cur = to_curve(&c).unwrap();
            let mut o = vec![0.0];
            let st = match rust_body(&i, &c, &cur, &mut o) {
                Ok(()) => 0.0,
                Err(e) => status(&e),
            };
            println!("{} {st}", case.name);
        }
    }
}
