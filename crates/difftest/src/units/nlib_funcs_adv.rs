//! Runner for unit `nlib.funcs_adv`: knot refinement and derivative of a scalar
//! B-spline curve function, and the symbolic products function * function and
//! function * curve (xyz and 4D).
//!
//! C++: `solidcore_ffi::units::nlib_funcs_adv` (upstream
//! `source/SMLib/src/NL_FuncsAdv.cpp`). Rust: `solidcore::nurbs::funcs_adv` (methods on
//! `solidcore::nurbs::funcs_basic::CurveFunction`).
//!
//! Each case runs one routine (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), then on
//! success the outputs. A function is `<pfx>dims = [n, p, m]`, its values `<pfx>v` and
//! knots `<pfx>u`; a curve is `<pfx>dims`, `<pfx>cpts` (x y z w) and `<pfx>u`. The
//! products also return both inputs (`f_`, `g_`), whose knot vectors upstream rescales
//! in place. `refine` may run in place on the C++ side (`inplace`); `mul_func` with no
//! `g` is the upstream squaring call (`F` passed twice; the Rust side multiplies by a
//! clone).
//!
//! A function input is inline (`v`, degree `p`, knots `u`) or one coordinate (`x`, `y`,
//! `z` or `w`) of the control points of an upstream `.smc` curve over its knots
//! (`{"smc": {"$upstream_file": "curve0.smc"}, "coord": "w"}`, the denominator
//! function as upstream's symbolic curve routines build it). A curve input is inline
//! (`cpts`, `p`, `u`) or an `.smc` file. Every run has at least [`MIN_GENERATED`]
//! generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::funcs_basic::CurveFunction;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_funcs_adv::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.funcs_adv` runner.
pub struct Runner;

/// An inline curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// An inline function.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineFun {
    v: Vec<f64>,
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

/// A function input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum FunIn {
    /// One coordinate of the control points of an upstream `.smc` curve.
    File { smc: String, coord: String },
    /// Inline data.
    Inline(InlineFun),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Refine {
        f: FunIn,
        x: Vec<f64>,
        #[serde(default)]
        inplace: bool,
    },
    Deriv {
        f: FunIn,
        d: usize,
    },
    MulFunc {
        f: FunIn,
        #[serde(default)]
        g: Option<FunIn>,
    },
    MulCrv {
        f: FunIn,
        g: CrvIn,
    },
    MulCrv4d {
        f: FunIn,
        g: CrvIn,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// Whitespace-separated numbers of an upstream text file.
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

fn resolve_crv(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(c) => Ok(c.clone()),
    }
}

fn resolve_fun(f: &FunIn) -> Result<InlineFun, HarnessError> {
    match f {
        FunIn::File { smc, coord } => {
            let c = read_smc(smc)?;
            let k = match coord.as_str() {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                "w" => 3,
                _ => return Err(harness(format!("bad coord {coord}"))),
            };
            let v = c
                .cpts
                .iter()
                .map(|pt| pt.get(k).copied().unwrap_or(f64::NAN))
                .collect();
            Ok(InlineFun { v, p: c.p, u: c.u })
        }
        FunIn::Inline(f) => Ok(f.clone()),
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

fn to_fun(f: &InlineFun) -> Result<CurveFunction, HarnessError> {
    CurveFunction::new(f.v.clone(), f.p, f.u.clone()).map_err(|e| harness(format!("function: {e}")))
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
    if x.fract() != 0.0 || !(0.0..=1e7).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

/// Splits a dump with `width` doubles per coefficient starting at `at` into
/// `<pfx>dims`, `<pfx><coef_key>`, `<pfx>u`.
fn split_dump(
    flat: &[f64],
    at: &mut usize,
    pfx: &str,
    width: usize,
    mut out: Output,
) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?;
    let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
    let m = count(dims.get(2).copied().unwrap_or(f64::NAN))?;
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    let c = take(flat, at, width * (n + 1))?;
    let key = if width == 1 { "v" } else { "cpts" };
    out = out.values(&format!("{pfx}{key}"), c.to_vec());
    let u = take(flat, at, m + 1)?;
    Ok(out.values(&format!("{pfx}u"), u.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    if ok {
        match i {
            Inputs::Refine { .. } | Inputs::Deriv { .. } => {
                out = split_dump(flat, &mut at, "", 1, out)?;
            }
            Inputs::MulFunc { .. } => {
                out = split_dump(flat, &mut at, "h_", 1, out)?;
                out = split_dump(flat, &mut at, "f_", 1, out)?;
                out = split_dump(flat, &mut at, "g_", 1, out)?;
            }
            Inputs::MulCrv { .. } | Inputs::MulCrv4d { .. } => {
                out = split_dump(flat, &mut at, "h_", 4, out)?;
                out = split_dump(flat, &mut at, "f_", 1, out)?;
                out = split_dump(flat, &mut at, "g_", 4, out)?;
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

fn encode_fun(f: &InlineFun, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let n = f.v.len().checked_sub(1).ok_or_else(|| harness("empty function"))?;
    let m = f.u.len().checked_sub(1).ok_or_else(|| harness("empty knots"))?;
    o.extend([n as f64, f.p as f64, m as f64]);
    o.extend_from_slice(&f.v);
    o.extend_from_slice(&f.u);
    Ok(())
}

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
        Inputs::Refine { f, x, inplace } => {
            encode_fun(&resolve_fun(f)?, &mut o)?;
            o.push(x.len() as f64);
            o.extend_from_slice(x);
            o.push(flag(*inplace));
            Op::Refine
        }
        Inputs::Deriv { f, d } => {
            encode_fun(&resolve_fun(f)?, &mut o)?;
            o.push(*d as f64);
            Op::Deriv
        }
        Inputs::MulFunc { f, g } => {
            encode_fun(&resolve_fun(f)?, &mut o)?;
            o.push(flag(g.is_none()));
            if let Some(g) = g {
                encode_fun(&resolve_fun(g)?, &mut o)?;
            }
            Op::MulFunc
        }
        Inputs::MulCrv { f, g } | Inputs::MulCrv4d { f, g } => {
            encode_fun(&resolve_fun(f)?, &mut o)?;
            encode_crv(&resolve_crv(g)?, &mut o)?;
            if matches!(i, Inputs::MulCrv { .. }) {
                Op::MulCrv
            } else {
                Op::MulCrv4d
            }
        }
    };
    Ok((op, o))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::Status(s)) => Err(harness(format!("shim status {s}"))),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn dump_fun(o: &mut Vec<f64>, f: &CurveFunction) {
    o.extend([f.n() as f64, f.degree() as f64, f.m() as f64]);
    o.extend_from_slice(f.values());
    o.extend_from_slice(f.knots());
}

fn dump_crv(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for pw in c.cpts() {
        o.extend(pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

/// Status of a Rust error: the NLib number, `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        _ => -1.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o = vec![0.0];
    let res: Result<(), SolidError> = match i {
        Inputs::Refine { f, x, .. } => {
            let f = to_fun(&resolve_fun(f)?)?;
            f.refine(x).map(|r| dump_fun(&mut o, &r))
        }
        Inputs::Deriv { f, d } => {
            let f = to_fun(&resolve_fun(f)?)?;
            f.deriv(*d).map(|r| dump_fun(&mut o, &r))
        }
        Inputs::MulFunc { f, g } => {
            let mut f = to_fun(&resolve_fun(f)?)?;
            let mut g = match g {
                Some(g) => to_fun(&resolve_fun(g)?)?,
                None => f.clone(),
            };
            f.multiply_func(&mut g).map(|h| {
                dump_fun(&mut o, &h);
                dump_fun(&mut o, &f);
                dump_fun(&mut o, &g);
            })
        }
        Inputs::MulCrv { f, g } | Inputs::MulCrv4d { f, g } => {
            let mut f = to_fun(&resolve_fun(f)?)?;
            let mut g = to_curve(&resolve_crv(g)?)?;
            let h = if matches!(i, Inputs::MulCrv { .. }) {
                f.multiply_crv(&mut g)
            } else {
                f.multiply_crv_4d(&mut g)
            };
            h.map(|h| {
                dump_crv(&mut o, &h);
                dump_fun(&mut o, &f);
                dump_crv(&mut o, &g);
            })
        }
    };
    match res {
        Ok(()) => to_output(i, &o),
        Err(e) => match Output::unported(&e) {
            Some(d) => Ok(d),
            None => to_output(i, &[status(&e)]),
        },
    }
}

// ---- generated cases --------------------------------------------------------------

/// A random degree: 1..=4 mostly, sometimes 5..=9 (products may exceed the limit),
/// rarely 0.
fn gen_degree(rng: &mut Rng) -> usize {
    let roll = rng.unit();
    if roll < 0.05 {
        0
    } else if roll < 0.15 {
        5 + rng.below(5) as usize
    } else {
        1 + rng.below(4) as usize
    }
}

/// A random parameter range `(a, b)`.
fn gen_range(rng: &mut Rng) -> (f64, f64) {
    match rng.below(4) {
        0 => (0.0, 1.0),
        1 => {
            let a = rng.below(5) as f64 - 2.0;
            (a, a + 1.0 + rng.below(6) as f64)
        }
        _ => {
            let a = rng.uniform(-5.0, 5.0);
            (a, a + rng.uniform(0.25, 10.0))
        }
    }
}

/// Distinct interior knot values in `(a, b)`, sorted: up to `k` of them.
fn gen_values(rng: &mut Rng, a: f64, b: f64, k: usize) -> Vec<f64> {
    let len = b - a;
    let integral = rng.chance(0.4);
    let mut used: Vec<f64> = Vec::new();
    for _ in 0..k {
        for attempt in 0..32 {
            let v = if integral && attempt < 16 {
                a + len * (1.0 + rng.below(15) as f64) / 16.0
            } else {
                rng.uniform(a, b)
            };
            if v > a && v < b && !used.contains(&v) {
                used.push(v);
                break;
            }
        }
    }
    used.sort_by(f64::total_cmp);
    used
}

/// A clamped knot vector of degree `p` over `[a, b]` with the given distinct interior
/// values, each with a random multiplicity `1..=p`.
fn knots_from(rng: &mut Rng, p: usize, a: f64, b: f64, vals: &[f64]) -> Vec<f64> {
    let mut u = vec![a; p + 1];
    for &v in vals {
        let mult = if p > 1 && rng.chance(0.25) {
            1 + rng.below(p as u64) as usize
        } else {
            1
        };
        u.extend(std::iter::repeat_n(v, mult.max(1)));
    }
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

/// Random values for a knot vector of degree `p`.
fn fun_over(rng: &mut Rng, p: usize, u: Vec<f64>) -> InlineFun {
    let n = u.len() - p - 2;
    let v = (0..=n).map(|_| rng.uniform(-10.0, 10.0)).collect();
    InlineFun { v, p, u }
}

/// A random clamped function of degree `p` over `[a, b]` with interior values `vals`
/// (degree 0: a single segment).
fn gen_fun_with(rng: &mut Rng, p: usize, a: f64, b: f64, vals: &[f64]) -> InlineFun {
    let u = if p == 0 {
        vec![a, b]
    } else {
        knots_from(rng, p, a, b, vals)
    };
    fun_over(rng, p, u)
}

fn gen_fun(rng: &mut Rng) -> InlineFun {
    let p = gen_degree(rng);
    let (a, b) = gen_range(rng);
    let k = rng.below(7) as usize;
    let vals = gen_values(rng, a, b, k);
    gen_fun_with(rng, p, a, b, &vals)
}

/// A random curve over a knot vector of degree `p`.
fn crv_over(rng: &mut Rng, p: usize, u: Vec<f64>) -> InlineCrv {
    let n = u.len() - p - 2;
    let three_d = rng.chance(0.8);
    let rational = rng.chance(0.4);
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

/// Interior values for the second operand of a product, given the first's `vals` over
/// `[a, b]`, and the second's range.
fn partner(rng: &mut Rng, a: f64, b: f64, vals: &[f64]) -> (f64, f64, Vec<f64>) {
    match rng.below(7) {
        // Same knot values.
        0 => (a, b, vals.to_vec()),
        // A subset of them.
        1 => (a, b, vals.iter().copied().filter(|_| rng.chance(0.5)).collect()),
        // Independent values over the same range.
        2 => {
            let k = rng.below(6) as usize;
            (a, b, gen_values(rng, a, b, k))
        }
        // Shared plus independent values.
        3 => {
            let k = rng.below(4) as usize;
            let mut v = gen_values(rng, a, b, k);
            v.extend(vals.iter().copied().filter(|_| rng.chance(0.6)));
            v.sort_by(f64::total_cmp);
            v.dedup();
            (a, b, v)
        }
        // A different range (one vector is rescaled).
        4 => {
            let (c, d) = gen_range(rng);
            let k = rng.below(6) as usize;
            (c, d, gen_values(rng, c, d, k))
        }
        // The same values on a scaled range (rescaling maps them nearly onto vals).
        5 => {
            let s = rng.choose(&[0.5, 2.0, 3.0, 0.1]).copied().unwrap_or(2.0);
            let off = rng.choose(&[0.0, 1.0, -3.0]).copied().unwrap_or(0.0);
            let v = vals.iter().map(|x| off + s * (x - a)).collect();
            (off, off + s * (b - a), v)
        }
        // Values within (or just outside) PTOL of the first's.
        _ => {
            let d = rng.choose(&[1e-13, 5e-13, 2e-12, 1e-9]).copied().unwrap_or(1e-13);
            let v = vals
                .iter()
                .map(|x| if rng.chance(0.5) { x + d } else { *x })
                .collect();
            (a, b, v)
        }
    }
}

/// A parameter vector for refinement inside `f`'s range (non-decreasing; may repeat
/// existing knots or hit the ends, rarely be empty).
fn gen_refine(rng: &mut Rng, f: &InlineFun) -> Vec<f64> {
    let a = f.u.first().copied().unwrap_or(0.0);
    let b = f.u.last().copied().unwrap_or(1.0);
    if rng.chance(0.02) {
        return Vec::new();
    }
    let k = 1 + rng.below(6) as usize;
    let mut inner: Vec<f64> = f.u.iter().copied().filter(|&u| u > a && u < b).collect();
    inner.dedup();
    let mut x: Vec<f64> = (0..k)
        .map(|_| {
            if !inner.is_empty() && rng.chance(0.3) {
                inner
                    .get(rng.below(inner.len() as u64) as usize)
                    .copied()
                    .unwrap_or(a)
            } else {
                rng.uniform(a, b)
            }
        })
        .filter(|&v| v > a && v < b)
        .collect();
    if rng.chance(0.03) {
        x.push(if rng.chance(0.5) { a } else { b });
    }
    if x.is_empty() {
        x.push(0.5 * (a + b));
    }
    x.sort_by(f64::total_cmp);
    x
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0 | 1 => {
            let mut f = gen_fun(rng);
            if f.p == 0 {
                f = gen_fun_with(rng, 2, 0.0, 1.0, &[0.5]);
            }
            let x = gen_refine(rng, &f);
            Inputs::Refine {
                f: FunIn::Inline(f),
                x,
                inplace: rng.chance(0.4),
            }
        }
        2 | 3 => {
            let f = gen_fun(rng);
            let d = if rng.chance(0.05) {
                f.p + 1
            } else {
                rng.below(f.p as u64 + 1) as usize
            };
            Inputs::Deriv {
                f: FunIn::Inline(f),
                d,
            }
        }
        r => {
            let p = gen_degree(rng);
            let (a, b) = gen_range(rng);
            let k = rng.below(6) as usize;
            let vals = gen_values(rng, a, b, k);
            let mut f = gen_fun_with(rng, p, a, b, &vals);
            if p == 0 && rng.chance(0.2) {
                // More than one segment: upstream rejects it.
                f = InlineFun {
                    v: vec![1.0, 2.0],
                    p: 0,
                    u: vec![a, 0.5 * (a + b), b],
                };
            }
            let q = gen_degree(rng);
            let (c, d, gvals) = partner(rng, a, b, &vals);
            let gu = if q == 0 {
                vec![c, d]
            } else {
                knots_from(rng, q, c, d, &gvals)
            };
            match r {
                4..=6 => {
                    if rng.chance(0.08) {
                        Inputs::MulFunc {
                            f: FunIn::Inline(f),
                            g: None,
                        }
                    } else {
                        let g = fun_over(rng, q, gu);
                        Inputs::MulFunc {
                            f: FunIn::Inline(f),
                            g: Some(FunIn::Inline(g)),
                        }
                    }
                }
                7 | 8 => Inputs::MulCrv {
                    f: FunIn::Inline(f),
                    g: CrvIn::Inline(crv_over(rng, q, gu)),
                },
                _ => Inputs::MulCrv4d {
                    f: FunIn::Inline(f),
                    g: CrvIn::Inline(crv_over(rng, q, gu)),
                },
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.funcs_adv"
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
