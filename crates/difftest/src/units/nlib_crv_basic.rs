//! Runner for unit `nlib.crv_basic`: the NLib curve object (`NL_CURVE`): accessors,
//! storage sizing and copying, validity checks, rationality and dimension
//! conversions, closedness, degeneracy, straightness and planarity, equality and
//! compatibility of curve sets, numerator / denominator split, conic segment counts
//! and cross-boundary derivative curves.
//!
//! C++: `solidcore_ffi::units::nlib_crv_basic` (upstream
//! `source/SMLib/src/NL_CrvBasic.cpp`, `N_CrossBoundDerivCrvNonRatSrf` of
//! `NL_SrfSymbol.cpp`). Rust: `solidcore::nurbs::crv_basic`.
//!
//! Each case runs one *op* (`inputs.op`, a group of upstream calls). Both sides
//! produce the same flat record of doubles, split into keys by [`to_output`].
//! Statuses are inlined: `0` ok, else the `NL_*_ERR` number (`-1` on the Rust side
//! for a non-NLib error, which never matches). Outputs upstream leaves unwritten are
//! NaN on both sides. A curve is dumped as `dims = [n, p, m]`, the control points in
//! use (`cpts`, x y z w) and the knots in use (`u`); a NULL curve as `dims = [-1]`.
//! Curve functions are dumped the same way (`fu` instead of `cpts`).
//!
//! A curve input is either inline (control point storage `[x, y, z, w]`, degree,
//! knot storage, optional indices in use `[n, m]` below the storage) or an upstream
//! `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read here by
//! [`read_smc`]); surfaces likewise (`.sms`). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::{
    conic_data, cross_bound_deriv_crv_non_rat_srf, curve_from_num_and_denom,
    curves_are_coincident, curves_are_compatible, make_rat_compatible, size_arrays, Boundary,
    ConicType, Curve,
};
use solidcore::nurbs::funcs_basic::CurveFunction;
use solidcore::nurbs::geom::Interval;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::{Surface, SurfaceIndices};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_crv_basic::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.crv_basic` runner.
pub struct Runner;

/// An inline curve: storage plus optional indices in use (default: all).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used: Option<[usize; 2]>,
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

/// An inline curve function.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCfn {
    fu: Vec<f64>,
    p: usize,
    u: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used: Option<[usize; 2]>,
}

/// An inline surface: storage plus optional indices in use (default: all).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used: Option<[usize; 4]>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Kind {
    Compress,
    To2d,
    To3d,
    RatToNonRat,
    NonRatToRat,
    MakeNonRat,
    PruneRat { flg: bool },
    Reparam { interval: [f64; 2] },
    SetSize { idx: [usize; 3] },
    CopyNull,
    CopyInto { dst: CrvIn },
    SizeArrays { idx: [usize; 3] },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    /// `N_CrvGetNumAndDenom(crv, dst, den)`.
    NumDenom,
    /// `N_CrvGetDenomCrvFunc(crv, den)`.
    DenomFunc,
    /// `N_CreateCrvFromNumAndDenom(crv, den, dst)`.
    FromNumDenom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Inspect {
        crv: CrvIn,
        tol: f64,
        sized: [usize; 2],
    },
    Epolygon {
        crv: CrvIn,
        k: usize,
        l: usize,
    },
    Modify {
        crv: CrvIn,
        #[serde(flatten)]
        kind: Kind,
    },
    Equal {
        a: CrvIn,
        b: CrvIn,
        tol3d: f64,
        tol1d: f64,
    },
    Compatible {
        curves: Vec<CrvIn>,
    },
    RatCompatible {
        curves: Vec<CrvIn>,
    },
    Coincident {
        a: CrvIn,
        b: CrvIn,
        tol: f64,
    },
    Conic {
        ctp: i32,
        start: f64,
        end: f64,
    },
    CrossBoundary {
        sur: SrfIn,
        side: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dst: Option<CrvIn>,
    },
    NumDenom {
        crv: CrvIn,
        mode: Mode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dst: Option<CrvIn>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        den: Option<InlineCfn>,
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
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let smc::SmcCurve { cpts, p, u } = smc::decode(&text, path)?;
    Ok(InlineCrv {
        cpts,
        p,
        u,
        used: None,
    })
}

#[path = "fixtures/nlib_crv_basic_smc.rs"]
mod smc;

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let m = int(path, num()?)?;
    let p = int(path, num()?)?;
    let q = int(path, num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let mut row = Vec::with_capacity(m + 1);
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf {
        net,
        p,
        q,
        u,
        v,
        used: None,
    })
}

fn resolve(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(i) => Ok(i.clone()),
    }
}

fn resolve_srf(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

/// Storage `(N, K)` and indices in use `(n, m)` of a curve or function with `len`
/// entries and `ulen` knots.
fn shape_of(len: usize, ulen: usize, used: Option<[usize; 2]>) -> Result<([usize; 2], [usize; 2]), HarnessError> {
    let big_n = len.checked_sub(1).ok_or_else(|| harness("empty control polygon"))?;
    let big_k = ulen.checked_sub(1).ok_or_else(|| harness("empty knots"))?;
    let big = [big_n, big_k];
    let used = used.unwrap_or(big);
    if used[0] > big_n || used[1] > big_k {
        return Err(harness("indices in use above the storage"));
    }
    Ok((big, used))
}

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (big, used) = shape_of(c.cpts.len(), c.u.len(), c.used)?;
    o.extend([
        big[0] as f64,
        big[1] as f64,
        used[0] as f64,
        c.p as f64,
        used[1] as f64,
    ]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    Ok(())
}

fn encode_cfn(f: &InlineCfn, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (big, used) = shape_of(f.fu.len(), f.u.len(), f.used)?;
    o.extend([
        big[0] as f64,
        big[1] as f64,
        used[0] as f64,
        f.p as f64,
        used[1] as f64,
    ]);
    o.extend_from_slice(&f.fu);
    o.extend_from_slice(&f.u);
    Ok(())
}

/// Storage `(N, M, R, S)` and indices in use `(n, m, r, s)` of a surface.
fn srf_shape(s: &InlineSrf) -> Result<([usize; 4], [usize; 4]), HarnessError> {
    let big_n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let big_m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    let big_r = s.u.len().checked_sub(1).ok_or_else(|| harness("empty u"))?;
    let big_s = s.v.len().checked_sub(1).ok_or_else(|| harness("empty v"))?;
    let big = [big_n, big_m, big_r, big_s];
    let used = s.used.unwrap_or(big);
    if used.iter().zip(&big).any(|(a, b)| a > b) {
        return Err(harness("indices in use above the storage"));
    }
    Ok((big, used))
}

fn encode_srf(s: &InlineSrf, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (big, used) = srf_shape(s)?;
    o.extend(big.iter().map(|&k| k as f64));
    o.extend([
        used[0] as f64,
        used[1] as f64,
        s.p as f64,
        s.q as f64,
        used[2] as f64,
        used[3] as f64,
    ]);
    o.extend(s.net.iter().flatten().flatten());
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn build_crv(c: &InlineCrv) -> Result<Curve, HarnessError> {
    let (_, used) = shape_of(c.cpts.len(), c.u.len(), c.used)?;
    let pw = c
        .cpts
        .iter()
        .map(|a| CPoint::new(a[0], a[1], a[2], a[3]))
        .collect();
    Curve::with_indices(pw, used[0], c.p, c.u.clone(), used[1])
        .map_err(|e| harness(format!("rust curve: {e}")))
}

fn build_cfn(f: &InlineCfn) -> Result<CurveFunction, HarnessError> {
    let (_, used) = shape_of(f.fu.len(), f.u.len(), f.used)?;
    CurveFunction::with_indices(f.fu.clone(), used[0], f.p, f.u.clone(), used[1])
        .map_err(|e| harness(format!("rust curve function: {e}")))
}

fn build_srf(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let (_, used) = srf_shape(s)?;
    let net = s
        .net
        .iter()
        .map(|row| row.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
        .collect();
    let idx = SurfaceIndices {
        n: used[0],
        m: used[1],
        p: s.p,
        q: s.q,
        r: used[2],
        s: used[3],
    };
    Surface::with_indices(net, s.u.clone(), s.v.clone(), idx)
        .map_err(|e| harness(format!("rust surface: {e}")))
}

// ---- output layout ----------------------------------------------------------------

const INSPECT_LAYOUT: &[(&str, usize)] = &[
    ("is_3d", 1),
    ("weights_are_valid", 1),
    ("is_sized", 1),
    ("is_rat", 1),
    ("is_closed", 1),
    ("is_degen", 1),
    ("is_line", 1),
    ("are_weights_equal", 1),
    ("is_4d_degen", 1),
    ("is_in_z0_plane", 1),
    ("param_bounds", 2),
    ("sizes_n_p_m", 3),
    ("is_planar_status", 1),
    ("is_planar_flt_pp_nn", 7),
];

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let part = flat
        .get(*at..*at + k)
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

/// Splits a curve (`width = 4`) or function (`width = 1`) dump starting at `at` into
/// `<pfx>dims`, `<pfx><vals>`, `<pfx>u`.
fn split_dump(
    flat: &[f64],
    at: &mut usize,
    pfx: &str,
    vals: &str,
    width: usize,
    mut out: Output,
) -> Result<Output, HarnessError> {
    let first = take(flat, at, 1)?;
    if first == [-1.0] {
        return Ok(out.values(&format!("{pfx}dims"), first.to_vec()));
    }
    let rest = take(flat, at, 2)?;
    let n = count(first.first().copied().unwrap_or(f64::NAN))?;
    let m = count(rest.get(1).copied().unwrap_or(f64::NAN))?;
    let mut dims = first.to_vec();
    dims.extend_from_slice(rest);
    out = out.values(&format!("{pfx}dims"), dims);
    let c = take(flat, at, width * (n + 1))?;
    out = out.values(&format!("{pfx}{vals}"), c.to_vec());
    let u = take(flat, at, m + 1)?;
    Ok(out.values(&format!("{pfx}u"), u.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    match i {
        Inputs::Inspect { .. } => {
            for (key, k) in INSPECT_LAYOUT {
                out = out.values(key, take(flat, &mut at, *k)?.to_vec());
            }
        }
        Inputs::Epolygon { .. } => {
            out = out.values("status", take(flat, &mut at, 1)?.to_vec());
            let rest = flat.len() - at;
            out = out.values("pts", take(flat, &mut at, rest)?.to_vec());
        }
        Inputs::Modify { kind, .. } => {
            out = out.values("status", take(flat, &mut at, 1)?.to_vec());
            if let Kind::SizeArrays { .. } = kind {
                out = out.values("dims", take(flat, &mut at, 3)?.to_vec());
            } else {
                out = split_dump(flat, &mut at, "", "cpts", 4, out)?;
            }
        }
        Inputs::Equal { .. } => {
            out = out.values("equal", take(flat, &mut at, 1)?.to_vec());
        }
        Inputs::Compatible { .. } => {
            out = out.values("compatible", take(flat, &mut at, 1)?.to_vec());
        }
        Inputs::RatCompatible { curves } => {
            for k in 0..curves.len() {
                out = split_dump(flat, &mut at, &format!("c{k}_"), "cpts", 4, out)?;
            }
        }
        Inputs::Coincident { .. } => {
            out = out.values("coincident", take(flat, &mut at, 1)?.to_vec());
            out = split_dump(flat, &mut at, "a_", "cpts", 4, out)?;
            out = split_dump(flat, &mut at, "b_", "cpts", 4, out)?;
        }
        Inputs::Conic { .. } => {
            out = out.values("ae_n_p_m", take(flat, &mut at, 4)?.to_vec());
        }
        Inputs::CrossBoundary { .. } => {
            out = out.values("status", take(flat, &mut at, 1)?.to_vec());
            out = split_dump(flat, &mut at, "", "cpts", 4, out)?;
        }
        Inputs::NumDenom { .. } => {
            out = out.values("status", take(flat, &mut at, 1)?.to_vec());
            out = split_dump(flat, &mut at, "crv_", "cpts", 4, out)?;
            out = split_dump(flat, &mut at, "fn_", "fu", 1, out)?;
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

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = match i {
        Inputs::Inspect { crv, tol, sized } => {
            encode_crv(&resolve(crv)?, &mut o)?;
            o.push(*tol);
            o.extend(sized.iter().map(|&k| k as f64));
            Op::Inspect
        }
        Inputs::Epolygon { crv, k, l } => {
            encode_crv(&resolve(crv)?, &mut o)?;
            o.extend([*k as f64, *l as f64]);
            Op::Epolygon
        }
        Inputs::Modify { crv, kind } => {
            encode_crv(&resolve(crv)?, &mut o)?;
            match kind {
                Kind::Compress => o.push(0.0),
                Kind::To2d => o.push(1.0),
                Kind::To3d => o.push(2.0),
                Kind::RatToNonRat => o.push(3.0),
                Kind::NonRatToRat => o.push(4.0),
                Kind::MakeNonRat => o.push(5.0),
                Kind::PruneRat { flg } => o.extend([6.0, if *flg { 1.0 } else { 0.0 }]),
                Kind::Reparam { interval } => {
                    o.push(7.0);
                    o.extend_from_slice(interval);
                }
                Kind::SetSize { idx } => {
                    o.push(8.0);
                    o.extend(idx.iter().map(|&k| k as f64));
                }
                Kind::CopyNull => o.push(9.0),
                Kind::CopyInto { dst } => {
                    o.push(10.0);
                    encode_crv(&resolve(dst)?, &mut o)?;
                }
                Kind::SizeArrays { idx } => {
                    o.push(11.0);
                    o.extend(idx.iter().map(|&k| k as f64));
                }
            }
            Op::Modify
        }
        Inputs::Equal { a, b, tol3d, tol1d } => {
            encode_crv(&resolve(a)?, &mut o)?;
            encode_crv(&resolve(b)?, &mut o)?;
            o.extend([*tol3d, *tol1d]);
            Op::Equal
        }
        Inputs::Compatible { curves } | Inputs::RatCompatible { curves } => {
            o.push(curves.len() as f64);
            for c in curves {
                encode_crv(&resolve(c)?, &mut o)?;
            }
            if matches!(i, Inputs::Compatible { .. }) {
                Op::Compatible
            } else {
                Op::RatCompatible
            }
        }
        Inputs::Coincident { a, b, tol } => {
            encode_crv(&resolve(a)?, &mut o)?;
            encode_crv(&resolve(b)?, &mut o)?;
            o.push(*tol);
            Op::Coincident
        }
        Inputs::Conic { ctp, start, end } => {
            o.extend([f64::from(*ctp), *start, *end]);
            Op::Conic
        }
        Inputs::CrossBoundary { sur, side, dst } => {
            encode_srf(&resolve_srf(sur)?, &mut o)?;
            o.push(f64::from(*side));
            match dst {
                Some(d) => {
                    o.push(1.0);
                    encode_crv(&resolve(d)?, &mut o)?;
                }
                None => o.push(0.0),
            }
            Op::CrossBoundary
        }
        Inputs::NumDenom { crv, mode, dst, den } => {
            encode_crv(&resolve(crv)?, &mut o)?;
            o.push(match mode {
                Mode::NumDenom => 0.0,
                Mode::DenomFunc => 1.0,
                Mode::FromNumDenom => 2.0,
            });
            match dst {
                Some(d) => {
                    o.push(1.0);
                    encode_crv(&resolve(d)?, &mut o)?;
                }
                None => o.push(0.0),
            }
            match den {
                Some(f) => {
                    o.push(1.0);
                    encode_cfn(f, &mut o)?;
                }
                None => o.push(0.0),
            }
            Op::NumDenom
        }
    };
    Ok((op, o))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::InvalidArgument(m)) => Err(harness(format!("ffi rejected: {m}"))),
        Err(e) => Err(harness(format!("shim failure: {e}"))),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// `0` for `Ok`, the `NL_*_ERR` number for an NLib error, `-1` otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(_) => -1.0,
    }
}

fn dump_crv(o: &mut Vec<f64>, c: Option<&Curve>) {
    let Some(c) = c else {
        o.push(-1.0);
        return;
    };
    let (n, m) = c.array_sizes();
    o.extend([n as f64, c.degree() as f64, m as f64]);
    for pw in c.cpts() {
        o.extend_from_slice(&pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn dump_cfn(o: &mut Vec<f64>, f: Option<&CurveFunction>) {
    let Some(f) = f else {
        o.push(-1.0);
        return;
    };
    let (n, m) = f.array_sizes();
    o.extend([n as f64, f.degree() as f64, m as f64]);
    o.extend_from_slice(f.values());
    o.extend_from_slice(f.knots());
}

/// The Rust record, or `Err(Ok(output))` for a deferred case.
fn rust_flat(i: &Inputs) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::Inspect { crv, tol, sized } => {
            let c = build_crv(&resolve(crv)?)?;
            o.push(flag(c.is_3d()));
            o.push(status(&c.weights_are_valid()));
            o.push(status(&c.is_sized(sized[0], sized[1])));
            o.push(flag(c.is_rational()));
            o.push(flag(c.is_closed()));
            o.push(flag(c.is_degen()));
            o.push(flag(c.is_line(*tol)));
            o.push(flag(c.are_weights_equal()));
            o.push(flag(c.is_4d_degen()));
            o.push(flag(c.is_in_z0_plane()));
            let (ul, ur) = c.param_bounds();
            o.extend([ul, ur]);
            let (n, m) = c.array_sizes();
            o.extend([n as f64, c.degree() as f64, m as f64]);
            let pl = c.is_planar(*tol);
            o.push(status(&pl));
            match pl {
                Ok(pl) => {
                    o.push(flag(pl.flat));
                    match pl.plane {
                        Some((pp, nn)) => {
                            o.extend_from_slice(&pp.to_array());
                            o.extend_from_slice(&nn.to_array());
                        }
                        None => o.extend([f64::NAN; 6]),
                    }
                }
                Err(_) => o.extend([f64::NAN; 7]),
            }
        }
        Inputs::Epolygon { crv, k, l } => {
            let c = build_crv(&resolve(crv)?)?;
            let r = c.epolygon(*k, *l);
            o.push(status(&r));
            if let Ok(pts) = r {
                for p in pts {
                    o.extend_from_slice(&p.to_array());
                }
            }
        }
        Inputs::Modify { crv, kind } => {
            let mut c = build_crv(&resolve(crv)?)?;
            let mut st = 0.0;
            match kind {
                Kind::Compress => c.compress(),
                Kind::To2d => c.to_2d(),
                Kind::To3d => c.to_3d(),
                Kind::RatToNonRat => c.rat_to_non_rat(),
                Kind::NonRatToRat => c.non_rat_to_rat(),
                Kind::MakeNonRat => c.make_non_rat(),
                Kind::PruneRat { flg } => c.prune_rat(*flg),
                Kind::Reparam { interval } => {
                    st = status(&c.reparam_to_interval(Interval::new(interval[0], interval[1])));
                }
                Kind::SetSize { idx } => {
                    st = status(&c.set_size_indices(idx[0], idx[1], idx[2]));
                }
                Kind::CopyNull => {
                    let mut slot = None;
                    st = status(&c.copy_into(&mut slot));
                    c = slot.ok_or_else(|| harness("copy_into left None"))?;
                }
                Kind::CopyInto { dst } => {
                    let mut slot = Some(build_crv(&resolve(dst)?)?);
                    st = status(&c.copy_into(&mut slot));
                    c = slot.ok_or_else(|| harness("copy_into left None"))?;
                }
                Kind::SizeArrays { idx } => {
                    let mut slot = Some(c);
                    let r = size_arrays(&mut slot, idx[0], idx[1], idx[2]).map(|_| ());
                    o.push(status(&r));
                    let c = slot.ok_or_else(|| harness("size_arrays left None"))?;
                    let (n, m) = c.array_sizes();
                    o.extend([n as f64, c.degree() as f64, m as f64]);
                    return Ok(Ok(o));
                }
            }
            o.push(st);
            dump_crv(&mut o, Some(&c));
        }
        Inputs::Equal { a, b, tol3d, tol1d } => {
            let a = build_crv(&resolve(a)?)?;
            let b = build_crv(&resolve(b)?)?;
            o.push(flag(a.equals(&b, *tol3d, *tol1d)));
        }
        Inputs::Compatible { curves } | Inputs::RatCompatible { curves } => {
            let mut crvs = curves
                .iter()
                .map(|c| build_crv(&resolve(c)?))
                .collect::<Result<Vec<_>, _>>()?;
            if matches!(i, Inputs::Compatible { .. }) {
                o.push(flag(curves_are_compatible(&crvs)));
            } else {
                make_rat_compatible(&mut crvs);
                for c in &crvs {
                    dump_crv(&mut o, Some(c));
                }
            }
        }
        Inputs::Coincident { a, b, tol } => {
            let mut a = build_crv(&resolve(a)?)?;
            let mut b = build_crv(&resolve(b)?)?;
            match curves_are_coincident(&mut a, &mut b, *tol) {
                Ok(f) => {
                    o.push(flag(f));
                    dump_crv(&mut o, Some(&a));
                    dump_crv(&mut o, Some(&b));
                }
                Err(e) => {
                    return match Output::unported(&e) {
                        Some(d) => Ok(Err(d)),
                        None => Err(harness(format!("coincident: {e}"))),
                    }
                }
            }
        }
        Inputs::Conic { ctp, start, end } => {
            let ctp = ConicType::from_flag(*ctp).ok_or_else(|| harness("conic: bad type"))?;
            let d = conic_data(ctp, *start, *end).map_err(|e| harness(format!("conic: {e}")))?;
            o.extend([d.ae, d.n as f64, d.p as f64, d.m as f64]);
        }
        Inputs::CrossBoundary { sur, side, dst } => {
            let s = build_srf(&resolve_srf(sur)?)?;
            let side = Boundary::from_flag(*side).ok_or_else(|| harness("cross: bad side"))?;
            let mut slot = match dst {
                Some(d) => Some(build_crv(&resolve(d)?)?),
                None => None,
            };
            let r = cross_bound_deriv_crv_non_rat_srf(&s, side, &mut slot);
            o.push(status(&r));
            dump_crv(&mut o, slot.as_ref());
        }
        Inputs::NumDenom { crv, mode, dst, den } => {
            let c = build_crv(&resolve(crv)?)?;
            let mut dc = match dst {
                Some(d) => Some(build_crv(&resolve(d)?)?),
                None => None,
            };
            let mut df = match den {
                Some(f) => Some(build_cfn(f)?),
                None => None,
            };
            let r = match mode {
                Mode::NumDenom => c.num_and_denom(&mut dc, &mut df),
                Mode::DenomFunc => c.denom_func(&mut df),
                Mode::FromNumDenom => curve_from_num_and_denom(&c, df.as_ref(), &mut dc),
            };
            o.push(status(&r));
            dump_crv(&mut o, dc.as_ref());
            dump_cfn(&mut o, df.as_ref());
        }
    }
    Ok(Ok(o))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_flat(i)? {
        Ok(flat) => to_output(i, &flat),
        Err(deferred) => Ok(deferred),
    }
}

// ---- generation -------------------------------------------------------------------

fn coord(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => rng.uniform(-1e4, 1e4),
        2 => rng.uniform(-1.0, 1.0) * 1e-6,
        _ => rng.uniform(-10.0, 10.0),
    }
}

fn weight(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        0 => *rng.choose(&[1e-4, 5e-4, 150.0, 1e-3, 100.0, 0.0, -1.0]).unwrap_or(&1.0),
        1..=4 => 1.0,
        _ => rng.uniform(0.5, 2.0),
    }
}

/// Clamped knot vector of `cnt` knots for degree `p` (now and then broken), or random
/// knots.
fn knots(rng: &mut Rng, cnt: usize, p: usize) -> Vec<f64> {
    let (a, b) = match rng.below(4) {
        0 => (0.0, 1.0),
        1 => (rng.uniform(-5.0, 0.0), rng.uniform(0.5, 5.0)),
        2 => (rng.uniform(-100.0, 100.0), rng.uniform(100.0, 300.0)),
        _ => (0.0, rng.uniform(0.1, 10.0)),
    };
    if rng.chance(0.08) {
        return (0..cnt).map(|_| rng.uniform(a, b)).collect();
    }
    let mut k: Vec<f64> = (0..cnt)
        .map(|i| {
            if i <= p {
                a
            } else if i + p + 1 >= cnt {
                b
            } else {
                rng.uniform(a, b)
            }
        })
        .collect();
    let lo = (p + 1).min(cnt);
    let hi = cnt.saturating_sub(p + 1).max(lo);
    if let Some(mid) = k.get_mut(lo..hi) {
        mid.sort_by(f64::total_cmp);
        if rng.chance(0.2) {
            if let [x, y, ..] = mid {
                *y = *x;
            }
        }
    }
    k
}

/// Shape families of generated control polygons.
#[derive(Clone, Copy)]
enum Family {
    Random,
    Line,
    Planar,
    Z0,
    Point,
    Closed,
    Hook,
}

/// Euclidean control points of a family.
fn polygon(rng: &mut Rng, n: usize, fam: Family) -> Vec<[f64; 3]> {
    let o = [coord(rng), coord(rng), coord(rng)];
    let d = rng.point3(-5.0, 5.0);
    let e = rng.point3(-5.0, 5.0);
    let noise = *rng.choose(&[0.0, 1e-9, 1e-7, 1e-5, 1e-3]).unwrap_or(&0.0);
    let jit = |rng: &mut Rng| rng.uniform(-1.0, 1.0) * noise;
    let mut pts: Vec<[f64; 3]> = (0..=n)
        .map(|i| {
            let t = if n == 0 { 0.0 } else { i as f64 / n as f64 };
            match fam {
                Family::Random | Family::Closed => [coord(rng), coord(rng), coord(rng)],
                Family::Line | Family::Hook => [
                    o[0] + t * d[0] + jit(rng),
                    o[1] + t * d[1] + jit(rng),
                    o[2] + t * d[2] + jit(rng),
                ],
                Family::Planar => {
                    let (s, r) = (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0));
                    [
                        o[0] + s * d[0] + r * e[0] + jit(rng),
                        o[1] + s * d[1] + r * e[1] + jit(rng),
                        o[2] + s * d[2] + r * e[2] + jit(rng),
                    ]
                }
                Family::Z0 => [coord(rng), coord(rng), 0.0],
                Family::Point => [o[0] + jit(rng), o[1] + jit(rng), o[2] + jit(rng)],
            }
        })
        .collect();
    match fam {
        Family::Closed => {
            if let Some(&first) = pts.first() {
                let eps = *rng.choose(&[0.0, 5e-8, 1e-6]).unwrap_or(&0.0);
                if let Some(last) = pts.last_mut() {
                    *last = [first[0] + eps, first[1], first[2]];
                }
            }
        }
        // A point past an end of the chord, or a backtrack.
        Family::Hook if n >= 2 => {
            let i = 1 + rng.below((n - 1) as u64) as usize;
            let t = *rng.choose(&[-0.01, 1.01, -1e-7, 0.5]).unwrap_or(&0.5);
            if let Some(p) = pts.get_mut(i) {
                *p = [o[0] + t * d[0], o[1] + t * d[1], o[2] + t * d[2]];
            }
        }
        _ => {}
    }
    pts
}

/// A random curve, inline, with optional extra storage.
fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let n = rng.below(8) as usize;
    let p = if rng.chance(0.9) {
        rng.below(n.min(5) as u64 + 1) as usize
    } else {
        rng.below(12) as usize
    };
    let m = if rng.chance(0.9) { n + p + 1 } else { rng.below(14) as usize };
    let fam = match rng.below(9) {
        0 | 1 => Family::Random,
        2 => Family::Line,
        4 => Family::Z0,
        5 => Family::Point,
        6 => Family::Closed,
        7 => Family::Hook,
        _ => Family::Planar, // 3 and 8
    };
    let rational = rng.chance(0.4);
    let two_d = rng.chance(0.15);
    let all_one = rational && rng.chance(0.2);
    let pts = polygon(rng, n, fam);
    let mut cpts: Vec<[f64; 4]> = pts
        .iter()
        .map(|q| {
            let z = if two_d { NOZ } else { q[2] };
            if rational {
                let w = if all_one { 1.0 } else { weight(rng) };
                let zw = if z == NOZ { NOZ } else { z * w };
                [q[0] * w, q[1] * w, zw, w]
            } else {
                [q[0], q[1], z, NOW]
            }
        })
        .collect();
    // Mixed rationality / dimension.
    if rng.chance(0.05) {
        let i = rng.below(n as u64 + 1) as usize;
        if let Some(c) = cpts.get_mut(i) {
            if rng.chance(0.5) {
                c[3] = if rational { NOW } else { 1.0 };
            } else {
                c[2] = if two_d { 0.0 } else { NOZ };
            }
        }
    }
    let mut u = knots(rng, m + 1, p);
    let mut used = None;
    if rng.chance(0.2) {
        for _ in 0..rng.below(3) {
            cpts.push([coord(rng), coord(rng), coord(rng), weight(rng)]);
        }
        for _ in 0..rng.below(3) {
            u.push(rng.uniform(-9.0, 9.0));
        }
        used = Some([n, m]);
    }
    InlineCrv { cpts, p, u, used }
}

/// A well-formed clamped B-spline (for `N_CrvsMakeCompatible`).
fn gen_bspline(rng: &mut Rng) -> InlineCrv {
    let n = 1 + rng.below(6) as usize;
    let p = 1 + rng.below(n.min(3) as u64) as usize;
    let rational = rng.chance(0.3);
    let two_d = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|_| {
            let (x, y) = (rng.uniform(-10.0, 10.0), rng.uniform(-10.0, 10.0));
            let z = if two_d { NOZ } else { rng.uniform(-10.0, 10.0) };
            if rational {
                let w = rng.uniform(0.5, 2.0);
                [x * w, y * w, if z == NOZ { NOZ } else { z * w }, w]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect();
    let (a, b) = if rng.chance(0.5) { (0.0, 1.0) } else { (rng.uniform(-3.0, 0.0), rng.uniform(1.0, 4.0)) };
    let inner = n - p;
    let mut mid: Vec<f64> = (0..inner).map(|k| a + (b - a) * (k as f64 + 1.0 + rng.uniform(-0.3, 0.3)) / (inner as f64 + 1.0)).collect();
    mid.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(mid);
    u.extend(vec![b; p + 1]);
    InlineCrv {
        cpts,
        p,
        u,
        used: None,
    }
}

fn storage(c: &InlineCrv) -> [usize; 2] {
    [c.cpts.len().saturating_sub(1), c.u.len().saturating_sub(1)]
}

fn in_use(c: &InlineCrv) -> [usize; 2] {
    c.used.unwrap_or_else(|| storage(c))
}

/// A perturbed copy of `c` (for equality tests).
fn near_copy(rng: &mut Rng, c: &InlineCrv) -> InlineCrv {
    let mut t = c.clone();
    match rng.below(6) {
        0 => {}
        1 => {
            let i = rng.below(t.cpts.len() as u64) as usize;
            let k = rng.below(4) as usize;
            let d = rng.uniform(-1.0, 1.0) * 1e-5;
            if let Some(x) = t.cpts.get_mut(i).and_then(|c| c.get_mut(k)) {
                if *x != NOW && *x != NOZ {
                    *x += d;
                }
            }
        }
        2 => {
            let i = rng.below(t.u.len() as u64) as usize;
            let d = rng.uniform(-1.0, 1.0) * 1e-5;
            if let Some(x) = t.u.get_mut(i) {
                *x += d;
            }
        }
        3 => t.p += 1,
        4 => {
            if let Some(c) = t.cpts.first_mut() {
                c[2] = if c[2] == NOZ { 0.0 } else { NOZ };
            }
        }
        _ => return gen_crv(rng),
    }
    t
}

/// A random surface whose shape allows `side`.
fn gen_srf(rng: &mut Rng, side: i32) -> InlineSrf {
    let along_u = side <= 2;
    let n = if along_u { 1 + rng.below(5) as usize } else { rng.below(5) as usize };
    let m = if along_u { rng.below(5) as usize } else { 1 + rng.below(5) as usize };
    let p = rng.below(n.min(3) as u64 + 1) as usize;
    let q = rng.below(m.min(3) as u64 + 1) as usize;
    let (mut r, mut s) = (n + p + 1, m + q + 1);
    if rng.chance(0.1) {
        // Fewer knots than a valid surface, still enough for the side.
        match side {
            1 => r = r.min(p + 1 + rng.below(3) as usize),
            2 => r = r.min(n + p + rng.below(2) as usize),
            3 => s = s.min(q + 1 + rng.below(3) as usize),
            _ => s = s.min(m + q + rng.below(2) as usize),
        }
    }
    let rational = rng.chance(0.3);
    let two_d = rng.chance(0.1);
    let net = (0..=n)
        .map(|_| {
            (0..=m)
                .map(|_| {
                    let (x, y) = (coord(rng), coord(rng));
                    let z = if two_d { NOZ } else { coord(rng) };
                    if rational {
                        let w = weight(rng);
                        [x * w, y * w, if z == NOZ { NOZ } else { z * w }, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    let mut u = knots(rng, r + 1, p);
    let mut v = knots(rng, s + 1, q);
    let mut net: Vec<Vec<[f64; 4]>> = net;
    let mut used = None;
    if rng.chance(0.2) {
        let em = rng.below(2) as usize;
        for row in &mut net {
            for _ in 0..em {
                row.push([coord(rng), coord(rng), coord(rng), NOW]);
            }
        }
        if rng.chance(0.5) {
            net.push((0..=m + em).map(|_| [coord(rng), coord(rng), coord(rng), NOW]).collect());
        }
        u.push(rng.uniform(-9.0, 9.0));
        v.push(rng.uniform(-9.0, 9.0));
        used = Some([n, m, r, s]);
    }
    InlineSrf {
        net,
        p,
        q,
        u,
        v,
        used,
    }
}

fn gen_cfn(rng: &mut Rng, n: usize, p: usize, m: usize) -> InlineCfn {
    let mut fu: Vec<f64> = (0..=n).map(|_| weight(rng)).collect();
    let mut u = knots(rng, m + 1, p);
    let mut used = None;
    if rng.chance(0.3) {
        fu.push(rng.uniform(-3.0, 3.0));
        u.push(rng.uniform(-3.0, 3.0));
        used = Some([n, m]);
    }
    InlineCfn { fu, p, u, used }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let crv = gen_crv(rng);
    let [n, m] = in_use(&crv);
    match rng.below(11) {
        0 | 1 => {
            let tol = *rng
                .choose(&[1e-6, 1e-3, 0.0, 1e-9, 0.1, 1e-7])
                .unwrap_or(&1e-6);
            let pick = |k: usize, rng: &mut Rng| -> usize {
                if rng.chance(0.5) {
                    k
                } else {
                    rng.below(k as u64 + 3) as usize
                }
            };
            let sized = [pick(n, rng), pick(m, rng)];
            Inputs::Inspect {
                crv: CrvIn::Inline(crv),
                tol,
                sized,
            }
        }
        2 => {
            let k = rng.below(n as u64 + 2) as usize;
            let l = if rng.chance(0.8) {
                k + rng.below((n + 1).saturating_sub(k) as u64 + 1) as usize
            } else {
                rng.below(n as u64 + 3) as usize
            };
            Inputs::Epolygon {
                crv: CrvIn::Inline(crv),
                k,
                l,
            }
        }
        3 | 4 => {
            let big = storage(&crv);
            let p = crv.p;
            let kind = match rng.below(12) {
                0 => Kind::Compress,
                1 => Kind::To2d,
                2 => Kind::To3d,
                3 => Kind::RatToNonRat,
                4 => Kind::NonRatToRat,
                5 => Kind::MakeNonRat,
                6 => Kind::PruneRat {
                    flg: rng.chance(0.5),
                },
                7 => {
                    if m < p {
                        Kind::Compress
                    } else {
                        let interval = match rng.below(4) {
                            0 => [
                                crv.u.first().copied().unwrap_or(0.0),
                                crv.u.get(m).copied().unwrap_or(1.0),
                            ],
                            1 => [0.0, 1.0],
                            _ => {
                                let a = rng.uniform(-10.0, 10.0);
                                [a, a + rng.uniform(0.1, 20.0)]
                            }
                        };
                        Kind::Reparam { interval }
                    }
                }
                8 => Kind::SetSize {
                    idx: [
                        rng.below(big[0] as u64 + 1) as usize,
                        rng.below(6) as usize,
                        rng.below(big[1] as u64 + 1) as usize,
                    ],
                },
                9 => Kind::CopyNull,
                10 => Kind::CopyInto {
                    dst: CrvIn::Inline(gen_crv(rng)),
                },
                _ => {
                    let pick = |k: usize, rng: &mut Rng| -> usize {
                        if rng.chance(0.5) {
                            rng.below(k as u64 + 1) as usize
                        } else {
                            rng.below(k as u64 + 4) as usize
                        }
                    };
                    Kind::SizeArrays {
                        idx: [pick(n, rng), rng.below(6) as usize, pick(m, rng)],
                    }
                }
            };
            Inputs::Modify {
                crv: CrvIn::Inline(crv),
                kind,
            }
        }
        5 => {
            let b = near_copy(rng, &crv);
            let tol3d = *rng.choose(&[0.0, 1e-6, 1e-4, 1e-3]).unwrap_or(&0.0);
            let tol1d = *rng.choose(&[0.0, 1e-6, 1e-4, 1e-3]).unwrap_or(&0.0);
            Inputs::Equal {
                a: CrvIn::Inline(crv),
                b: CrvIn::Inline(b),
                tol3d,
                tol1d,
            }
        }
        6 => {
            let k = rng.below(4) as usize;
            let mut curves = vec![CrvIn::Inline(crv.clone())];
            for _ in 0..k {
                let c = if rng.chance(0.6) {
                    near_copy(rng, &crv)
                } else {
                    gen_crv(rng)
                };
                curves.push(CrvIn::Inline(c));
            }
            if rng.chance(0.5) {
                Inputs::Compatible { curves }
            } else {
                Inputs::RatCompatible { curves }
            }
        }
        7 => {
            let a = gen_bspline(rng);
            let b = if rng.chance(0.5) {
                let mut b = a.clone();
                for c in &mut b.cpts {
                    c[0] += rng.uniform(-1.0, 1.0) * 1e-7;
                }
                b
            } else {
                gen_bspline(rng)
            };
            Inputs::Coincident {
                a: CrvIn::Inline(a),
                b: CrvIn::Inline(b),
                tol: *rng.choose(&[1e-6, 1e-3, 0.0]).unwrap_or(&1e-6),
            }
        }
        8 => {
            let ctp = *rng.choose(&[2, 4, 5]).unwrap_or(&2);
            let start = match rng.below(4) {
                0 => 0.0,
                1 => rng.uniform(-720.0, 720.0),
                2 => *rng.choose(&[90.0, 180.0, 270.0, 360.0, -90.0]).unwrap_or(&0.0),
                _ => rng.uniform(-1e5, 1e5),
            };
            let end = match rng.below(6) {
                0 => start,
                1 => start + *rng.choose(&[90.0, 91.0, 120.0, 121.0, 181.0, 240.0, 241.0, 270.0, 271.0, 360.0]).unwrap_or(&90.0),
                2 => start + rng.uniform(-1.0, 1.0) * 1e-6,
                3 => start + 360.0 + rng.uniform(-1.0, 1.0) * 1e-6,
                4 => rng.uniform(-1e5, 1e5),
                _ => start + rng.uniform(-800.0, 800.0),
            };
            Inputs::Conic { ctp, start, end }
        }
        9 => {
            let side = 1 + rng.below(4) as i32;
            let sur = gen_srf(rng, side);
            let dst = if rng.chance(0.3) {
                Some(CrvIn::Inline(gen_crv(rng)))
            } else {
                None
            };
            Inputs::CrossBoundary {
                sur: SrfIn::Inline(sur),
                side,
                dst,
            }
        }
        _ => {
            let mode = match rng.below(3) {
                0 => Mode::NumDenom,
                1 => Mode::DenomFunc,
                _ => Mode::FromNumDenom,
            };
            let dst = if rng.chance(0.3) {
                Some(CrvIn::Inline(gen_crv(rng)))
            } else {
                None
            };
            let den = match mode {
                Mode::FromNumDenom => {
                    if rng.chance(0.3) {
                        None
                    } else if rng.chance(0.85) {
                        Some(gen_cfn(rng, n, crv.p, m))
                    } else {
                        let (a, b, c) = (rng.below(8) as usize, rng.below(4) as usize, rng.below(12) as usize);
                        Some(gen_cfn(rng, a, b, c))
                    }
                }
                _ => {
                    if rng.chance(0.3) {
                        let (a, b) = (rng.below(n as u64 + 3) as usize, rng.below(m as u64 + 3) as usize);
                        let q = rng.below(4) as usize;
                        Some(gen_cfn(rng, a, q, b))
                    } else {
                        None
                    }
                }
            };
            // When the denominator is too small, upstream returns after allocating a
            // NULL numerator with `malloc` and before writing it: its contents are
            // uninitialised, so such cases always pass an existing numerator.
            let dst = match (mode, &den, dst) {
                (Mode::NumDenom, Some(_), None) => Some(CrvIn::Inline(gen_crv(rng))),
                (_, _, dst) => dst,
            };
            Inputs::NumDenom {
                crv: CrvIn::Inline(crv),
                mode,
                dst,
                den,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_basic"
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

