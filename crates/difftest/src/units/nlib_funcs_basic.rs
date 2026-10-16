//! Runner for unit `nlib.funcs_basic`: scalar B-spline curve and surface functions
//! (`NL_CFUN`, `NL_SFUN`): accessors, storage sizing, compaction, curve evaluation
//! and derivatives, surface evaluation.
//!
//! C++: `solidcore_ffi::units::nlib_funcs_basic` (upstream
//! `source/SMLib/src/NL_FuncsBasic.cpp`). Rust: `solidcore::nurbs::funcs_basic`.
//!
//! Each case runs one *op* (`inputs.op`, a group of upstream calls). Both sides
//! produce the same flat record of doubles, split into keys by [`to_output`].
//! Statuses are inlined: `0` ok, else the `NL_*_ERR` number (`-1` on the Rust side
//! for a non-NLib error, which never matches). Outputs upstream leaves unwritten are
//! NaN on both sides. A curve function is dumped as `dims = [n, p, m]`, the values
//! in use and the knots in use; a surface function as `dims = [n, m, p, q, r, s]`,
//! the values in use (row-major), `u` and `v`.
//!
//! A function input holds its storage (`values`, knot arrays), degree(s) and
//! optional indices in use below the storage (`used`). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::funcs_basic::{CurveFunction, SurfaceFunction};
use solidcore::nurbs::knots::Side;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_funcs_basic::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.funcs_basic` runner.
pub struct Runner;

/// `NL_LEFT` / `NL_RIGHT`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Flag {
    Left,
    Right,
}

impl Flag {
    fn side(self) -> Side {
        match self {
            Flag::Left => Side::Left,
            Flag::Right => Side::Right,
        }
    }

    fn code(self) -> f64 {
        f64::from(self.side().flag())
    }
}

/// A curve function: storage plus optional indices in use `[n, m]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrvIn {
    values: Vec<f64>,
    p: usize,
    knots: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used: Option<[usize; 2]>,
}

/// A surface function: storage plus optional indices in use `[n, m, r, s]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SrfIn {
    values: Vec<Vec<f64>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used: Option<[usize; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrvPt {
    u: f64,
    flag: Flag,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SrfPt {
    u: f64,
    v: f64,
    ufl: Flag,
    vfl: Flag,
}

/// Curve modifications; `idx` is `[n, p, m]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CrvKind {
    Compact,
    SetSize { idx: [usize; 3] },
    SetPtrs { values: Vec<f64>, knots: Vec<f64> },
    SizeArrays { idx: [usize; 3] },
    SizeArraysNull { idx: [usize; 3] },
}

/// Surface modifications; `idx` is `[n, m, p, q, r, s]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SrfKind {
    SetSize { idx: [usize; 6] },
    SizeArrays { idx: [usize; 6] },
    SizeArraysNull { idx: [usize; 6] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    CrvEval {
        crv: CrvIn,
        der: usize,
        params: Vec<CrvPt>,
    },
    CrvInspect {
        crv: CrvIn,
        sized: [usize; 2],
    },
    CrvModify {
        crv: CrvIn,
        #[serde(flatten)]
        kind: CrvKind,
    },
    SrfEval {
        srf: SrfIn,
        params: Vec<SrfPt>,
    },
    SrfInspect {
        srf: SrfIn,
        sized: [usize; 4],
    },
    SrfModify {
        srf: SrfIn,
        #[serde(flatten)]
        kind: SrfKind,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- function inputs --------------------------------------------------------------

/// Storage `[N, M]` and indices in use `[n, m]` of a curve function.
fn crv_shape(c: &CrvIn) -> Result<([usize; 2], [usize; 2]), HarnessError> {
    let big_n = c.values.len().checked_sub(1).ok_or_else(|| harness("empty values"))?;
    let big_m = c.knots.len().checked_sub(1).ok_or_else(|| harness("empty knots"))?;
    let big = [big_n, big_m];
    let used = c.used.unwrap_or(big);
    if used.iter().zip(&big).any(|(a, b)| a > b) {
        return Err(harness("indices in use above the storage"));
    }
    Ok((big, used))
}

/// Storage `[N, M, R, S]` and indices in use `[n, m, r, s]` of a surface function.
fn srf_shape(s: &SrfIn) -> Result<([usize; 4], [usize; 4]), HarnessError> {
    let big_n = s.values.len().checked_sub(1).ok_or_else(|| harness("empty values"))?;
    let cols = s.values.first().map_or(0, Vec::len);
    let big_m = cols.checked_sub(1).ok_or_else(|| harness("empty value row"))?;
    if s.values.iter().any(|r| r.len() != cols) {
        return Err(harness("value rows differ in length"));
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

fn encode_crv(c: &CrvIn, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (big, used) = crv_shape(c)?;
    o.extend([big[0], big[1], used[0], used[1], c.p].map(|k| k as f64));
    o.extend_from_slice(&c.values);
    o.extend_from_slice(&c.knots);
    Ok(())
}

fn encode_srf(s: &SrfIn, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (big, used) = srf_shape(s)?;
    o.extend(big.iter().map(|&k| k as f64));
    o.extend([used[0], used[1], s.p, s.q, used[2], used[3]].map(|k| k as f64));
    o.extend(s.values.iter().flatten());
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn build_crv(c: &CrvIn) -> Result<CurveFunction, HarnessError> {
    let (_, used) = crv_shape(c)?;
    CurveFunction::with_indices(c.values.clone(), used[0], c.p, c.knots.clone(), used[1])
        .map_err(|e| harness(format!("rust curve function: {e}")))
}

fn build_srf(s: &SrfIn) -> Result<SurfaceFunction, HarnessError> {
    let (_, used) = srf_shape(s)?;
    SurfaceFunction::with_indices(
        s.values.clone(),
        used[0],
        used[1],
        s.p,
        s.q,
        s.u.clone(),
        used[2],
        s.v.clone(),
        used[3],
    )
    .map_err(|e| harness(format!("rust surface function: {e}")))
}

// ---- output layout ----------------------------------------------------------------

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

fn split_crv_dump(flat: &[f64], at: &mut usize, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?;
    let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
    let m = count(dims.get(2).copied().unwrap_or(f64::NAN))?;
    out = out.values("dims", dims.to_vec());
    out = out.values("values", take(flat, at, n + 1)?.to_vec());
    Ok(out.values("knots", take(flat, at, m + 1)?.to_vec()))
}

fn split_srf_dump(flat: &[f64], at: &mut usize, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 6)?;
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m, r, s) = (dim(0)?, dim(1)?, dim(4)?, dim(5)?);
    out = out.values("dims", dims.to_vec());
    out = out.values("values", take(flat, at, (n + 1) * (m + 1))?.to_vec());
    out = out.values("u", take(flat, at, r + 1)?.to_vec());
    Ok(out.values("v", take(flat, at, s + 1)?.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    match i {
        Inputs::CrvEval { der, params, .. } => {
            let (mut st_f, mut f, mut st_d, mut fd) = (vec![], vec![], vec![], vec![]);
            for _ in params {
                st_f.extend_from_slice(take(flat, &mut at, 1)?);
                f.extend_from_slice(take(flat, &mut at, 1)?);
                st_d.extend_from_slice(take(flat, &mut at, 1)?);
                fd.extend_from_slice(take(flat, &mut at, der + 1)?);
            }
            out = out
                .values("eval_status", st_f)
                .values("f", f)
                .values("derivs_status", st_d)
                .values("fd", fd);
        }
        Inputs::CrvInspect { .. } | Inputs::SrfInspect { .. } => {
            out = out
                .values("is_sized", take(flat, &mut at, 1)?.to_vec())
                .values("null_flags", take(flat, &mut at, 2)?.to_vec())
                .values("accessors", take(flat, &mut at, 10)?.to_vec());
            out = if matches!(i, Inputs::CrvInspect { .. }) {
                split_crv_dump(flat, &mut at, out)?
            } else {
                split_srf_dump(flat, &mut at, out)?
            };
        }
        Inputs::CrvModify { kind, .. } => {
            out = out.values("status", take(flat, &mut at, 1)?.to_vec());
            if let CrvKind::SizeArraysNull { .. } = kind {
                out = out
                    .values("dims", take(flat, &mut at, 3)?.to_vec())
                    .values("null", take(flat, &mut at, 1)?.to_vec());
            } else {
                out = split_crv_dump(flat, &mut at, out)?;
            }
        }
        Inputs::SrfEval { params, .. } => {
            let (mut st, mut f) = (vec![], vec![]);
            for _ in params {
                st.extend_from_slice(take(flat, &mut at, 1)?);
                f.extend_from_slice(take(flat, &mut at, 1)?);
            }
            out = out.values("status", st).values("f", f);
        }
        Inputs::SrfModify { kind, .. } => {
            out = out.values("status", take(flat, &mut at, 1)?.to_vec());
            if let SrfKind::SizeArraysNull { .. } = kind {
                out = out
                    .values("dims", take(flat, &mut at, 6)?.to_vec())
                    .values("null", take(flat, &mut at, 1)?.to_vec());
            } else {
                out = split_srf_dump(flat, &mut at, out)?;
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

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = match i {
        Inputs::CrvEval { crv, der, params } => {
            encode_crv(crv, &mut o)?;
            o.push(*der as f64);
            o.push(params.len() as f64);
            for pt in params {
                o.extend([pt.u, pt.flag.code()]);
            }
            Op::CrvEval
        }
        Inputs::CrvInspect { crv, sized } => {
            encode_crv(crv, &mut o)?;
            o.extend(sized.iter().map(|&k| k as f64));
            Op::CrvInspect
        }
        Inputs::CrvModify { crv, kind } => {
            encode_crv(crv, &mut o)?;
            match kind {
                CrvKind::Compact => o.push(0.0),
                CrvKind::SetSize { idx } => {
                    o.push(1.0);
                    o.extend(idx.iter().map(|&k| k as f64));
                }
                CrvKind::SetPtrs { values, knots } => {
                    o.push(2.0);
                    o.push(values.len() as f64);
                    o.extend_from_slice(values);
                    o.push(knots.len() as f64);
                    o.extend_from_slice(knots);
                }
                CrvKind::SizeArrays { idx } => {
                    o.push(3.0);
                    o.extend(idx.iter().map(|&k| k as f64));
                }
                CrvKind::SizeArraysNull { idx } => {
                    o.push(4.0);
                    o.extend(idx.iter().map(|&k| k as f64));
                }
            }
            Op::CrvModify
        }
        Inputs::SrfEval { srf, params } => {
            encode_srf(srf, &mut o)?;
            o.push(params.len() as f64);
            for pt in params {
                o.extend([pt.u, pt.v, pt.ufl.code(), pt.vfl.code()]);
            }
            Op::SrfEval
        }
        Inputs::SrfInspect { srf, sized } => {
            encode_srf(srf, &mut o)?;
            o.extend(sized.iter().map(|&k| k as f64));
            Op::SrfInspect
        }
        Inputs::SrfModify { srf, kind } => {
            encode_srf(srf, &mut o)?;
            let (code, idx) = match kind {
                SrfKind::SetSize { idx } => (0.0, idx),
                SrfKind::SizeArrays { idx } => (1.0, idx),
                SrfKind::SizeArraysNull { idx } => (2.0, idx),
            };
            o.push(code);
            o.extend(idx.iter().map(|&k| k as f64));
            Op::SrfModify
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

fn dump_crv(o: &mut Vec<f64>, c: &CurveFunction) {
    let (n, m) = c.array_sizes();
    o.extend([n, c.degree(), m].map(|k| k as f64));
    o.extend_from_slice(c.values());
    o.extend_from_slice(c.knots());
}

fn dump_srf(o: &mut Vec<f64>, s: &SurfaceFunction) -> Result<(), HarnessError> {
    let (n, m, _, _) = s.array_sizes();
    let (p, q) = s.degrees();
    let (knu, knv) = s
        .knot_vectors()
        .map_err(|e| harness(format!("knot vectors: {e}")))?;
    o.extend([n, m, p, q, knu.last_index(), knv.last_index()].map(|k| k as f64));
    for row in s.rows() {
        o.extend_from_slice(row);
    }
    o.extend_from_slice(s.u_knots());
    o.extend_from_slice(s.v_knots());
    Ok(())
}

#[allow(clippy::too_many_lines)] // one arm per op, mirroring the shim
fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::CrvEval { crv, der, params } => {
            let c = build_crv(crv)?;
            for pt in params {
                let f = c.eval(pt.u, pt.flag.side());
                o.push(status(&f));
                o.push(f.unwrap_or(f64::NAN));
                let d = c.derivs(pt.u, pt.flag.side(), *der);
                o.push(status(&d));
                match d {
                    Ok(v) => {
                        if v.len() != der + 1 {
                            return Err(harness("derivs: wrong length"));
                        }
                        o.extend(v);
                    }
                    Err(_) => o.extend(std::iter::repeat_n(f64::NAN, der + 1)),
                }
            }
        }
        Inputs::CrvInspect { crv, sized } => {
            let c = build_crv(crv)?;
            o.push(status(&c.is_sized(sized[0], sized[1])));
            let none: Option<CurveFunction> = None;
            o.push(flag(false)); // a built function is never NULL
            o.push(flag(none.is_none()));
            let (n, m) = c.array_sizes();
            let knt = c
                .knot_vector()
                .map_err(|e| harness(format!("knot vector: {e}")))?;
            o.extend([n, m, c.degree(), c.n(), c.degree(), knt.last_index(), c.m(), knt.last_index()].map(|k| k as f64));
            o.push(c.values().first().copied().unwrap_or(f64::NAN));
            o.push(knt.knots().first().copied().unwrap_or(f64::NAN));
            dump_crv(&mut o, &c);
        }
        Inputs::CrvModify { crv, kind } => {
            let mut c = build_crv(crv)?;
            let st = match kind {
                CrvKind::Compact => {
                    c.compact();
                    0.0
                }
                CrvKind::SetSize { idx } => status(&c.set_size_indices(idx[0], idx[1], idx[2])),
                CrvKind::SetPtrs { values, knots } => status(&c.set_arrays(values.clone(), knots.clone())),
                CrvKind::SizeArrays { idx } => {
                    let mut slot = Some(c);
                    let st = status(&CurveFunction::size_arrays(&mut slot, idx[0], idx[1], idx[2]));
                    c = slot.ok_or_else(|| harness("size_arrays left None"))?;
                    st
                }
                CrvKind::SizeArraysNull { idx } => {
                    let mut slot = None;
                    let r = CurveFunction::size_arrays(&mut slot, idx[0], idx[1], idx[2]);
                    o.push(status(&r));
                    let c = slot.as_ref().ok_or_else(|| harness("size_arrays left None"))?;
                    let (n, m) = c.array_sizes();
                    o.extend([n, c.degree(), m].map(|k| k as f64));
                    o.push(flag(slot.is_none()));
                    return Ok(o);
                }
            };
            o.push(st);
            dump_crv(&mut o, &c);
        }
        Inputs::SrfEval { srf, params } => {
            let s = build_srf(srf)?;
            for pt in params {
                let f = s.eval(pt.u, pt.v, pt.ufl.side(), pt.vfl.side());
                o.push(status(&f));
                o.push(f.unwrap_or(f64::NAN));
            }
        }
        Inputs::SrfInspect { srf, sized } => {
            let s = build_srf(srf)?;
            o.push(status(&s.is_sized(sized[0], sized[1], sized[2], sized[3])));
            let none: Option<SurfaceFunction> = None;
            o.push(flag(false)); // a built function is never NULL
            o.push(flag(none.is_none()));
            let (n, m, r, ss) = s.array_sizes();
            let (p, q) = s.degrees();
            let rows = s.rows().count().saturating_sub(1);
            let cols = s.rows().next().map_or(0, <[f64]>::len).saturating_sub(1);
            let (knu, knv) = s
                .knot_vectors()
                .map_err(|e| harness(format!("knot vectors: {e}")))?;
            o.extend([n, m, r, ss, rows, cols, p, q, knu.last_index(), knv.last_index()].map(|k| k as f64));
            dump_srf(&mut o, &s)?;
        }
        Inputs::SrfModify { srf, kind } => {
            let mut s = build_srf(srf)?;
            let st = match kind {
                SrfKind::SetSize { idx } => {
                    status(&s.set_size_indices(idx[0], idx[1], idx[2], idx[3], idx[4], idx[5]))
                }
                SrfKind::SizeArrays { idx } => {
                    let mut slot = Some(s);
                    let st = status(&SurfaceFunction::size_arrays(
                        &mut slot, idx[0], idx[1], idx[2], idx[3], idx[4], idx[5],
                    ));
                    s = slot.ok_or_else(|| harness("size_arrays left None"))?;
                    st
                }
                SrfKind::SizeArraysNull { idx } => {
                    let mut slot = None;
                    let r = SurfaceFunction::size_arrays(&mut slot, idx[0], idx[1], idx[2], idx[3], idx[4], idx[5]);
                    o.push(status(&r));
                    let s = slot.as_ref().ok_or_else(|| harness("size_arrays left None"))?;
                    let (n, m, r, ss) = s.array_sizes();
                    let (p, q) = s.degrees();
                    o.extend([n, m, p, q, r, ss].map(|k| k as f64));
                    o.push(flag(slot.is_none()));
                    return Ok(o);
                }
            };
            o.push(st);
            dump_srf(&mut o, &s)?;
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation -------------------------------------------------------------------

fn value(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => rng.uniform(-1e4, 1e4),
        2 => rng.uniform(-1.0, 1.0) * 1e-6,
        3 => 1.0,
        _ => rng.uniform(-10.0, 10.0),
    }
}

/// `cnt` knots for degree `p`: clamped with sorted interior knots (sometimes with
/// repeated knots, of multiplicity up to `p + 2`), unclamped sorted, or (rarely)
/// unsorted.
fn knots(rng: &mut Rng, cnt: usize, p: usize) -> Vec<f64> {
    let (a, b) = match rng.below(5) {
        0 => (0.0, 1.0),
        1 => (rng.uniform(-5.0, 0.0), rng.uniform(0.5, 5.0)),
        2 => (rng.uniform(-100.0, 100.0), rng.uniform(100.0, 300.0)),
        3 => (0.0, rng.uniform(1e-6, 1e-3)),
        _ => (0.0, rng.uniform(0.1, 10.0)),
    };
    let style = rng.below(20);
    if style == 0 {
        return (0..cnt).map(|_| rng.uniform(a, b)).collect();
    }
    if style <= 2 {
        let mut k: Vec<f64> = (0..cnt).map(|_| rng.uniform(a, b)).collect();
        k.sort_by(f64::total_cmp);
        return k;
    }
    let mut k: Vec<f64> = (0..cnt)
        .map(|i| {
            if i <= p {
                a
            } else if i + p + 1 >= cnt {
                b
            } else if rng.chance(0.3) {
                // A "round" knot (repeats likely).
                a + (b - a) * (rng.below(4) as f64 + 1.0) / 5.0
            } else {
                rng.uniform(a, b)
            }
        })
        .collect();
    let lo = (p + 1).min(cnt);
    let hi = cnt.saturating_sub(p + 1).max(lo);
    if let Some(mid) = k.get_mut(lo..hi) {
        mid.sort_by(f64::total_cmp);
        if rng.chance(0.15) && !mid.is_empty() {
            // A run of equal knots (multiplicity up to p + 2, can exceed the degree).
            let at = rng.below(mid.len() as u64) as usize;
            let run = 1 + rng.below(p as u64 + 2) as usize;
            let x = mid.get(at).copied().unwrap_or(a);
            for y in mid.iter_mut().skip(at).take(run) {
                *y = x;
            }
            mid.sort_by(f64::total_cmp);
        }
    }
    k
}

/// Parameters for knots `k`: random inside, knots themselves, ends, just outside.
fn param(rng: &mut Rng, k: &[f64]) -> f64 {
    let a = k.first().copied().unwrap_or(0.0);
    let b = k.last().copied().unwrap_or(1.0);
    let lo = a.min(b);
    let hi = a.max(b);
    let w = (hi - lo).max(1e-9);
    match rng.below(10) {
        0..=3 => rng.uniform(lo, hi),
        4 | 5 => *rng.choose(k).unwrap_or(&a),
        6 => *rng.choose(&[a, b]).unwrap_or(&a),
        7 => {
            let x = *rng.choose(k).unwrap_or(&a);
            x + rng.uniform(-1.0, 1.0) * w * 1e-12
        }
        8 => *rng.choose(&[lo - w * 0.1, hi + w * 0.1, lo - 1e-14, hi + 1e-14]).unwrap_or(&lo),
        _ => rng.uniform(lo - 0.05 * w, hi + 0.05 * w),
    }
}

fn flag_of(rng: &mut Rng) -> Flag {
    if rng.chance(0.5) {
        Flag::Left
    } else {
        Flag::Right
    }
}

/// A curve function inside the evaluator domain (`m >= 2p+1`, storage covers
/// `m - p - 1`), with optional extra storage.
fn gen_crv(rng: &mut Rng, evaluable: bool) -> CrvIn {
    let p = match rng.below(10) {
        0 => 0,
        1..=6 => 1 + rng.below(3) as usize,
        _ => 4 + rng.below(if evaluable { 9 } else { 30 }) as usize,
    };
    let n = p + rng.below(8) as usize;
    let m = if rng.chance(0.85) {
        n + p + 1
    } else {
        // Knot count off the curve definition, within the evaluator domain.
        let lo = 2 * p + 1;
        let hi = n + p + 1 + 2;
        lo + rng.below((hi - lo) as u64 + 1) as usize
    };
    let mut values: Vec<f64> = (0..=n).map(|_| value(rng)).collect();
    let mut kn = knots(rng, m + 1, p);
    let mut used = None;
    // Value storage must reach m - p - 1 for evaluation.
    let need = (m + 1).saturating_sub(p + 1);
    while values.len() < need {
        values.push(value(rng));
    }
    let n = values.len() - 1;
    if rng.chance(0.25) {
        for _ in 0..rng.below(4) {
            values.push(value(rng));
        }
        for _ in 0..rng.below(4) {
            kn.push(rng.uniform(-9.0, 9.0));
        }
        used = Some([n, m]);
    }
    CrvIn {
        values,
        p,
        knots: kn,
        used,
    }
}

fn gen_srf(rng: &mut Rng, evaluable: bool) -> SrfIn {
    let deg = |rng: &mut Rng| match rng.below(10) {
        0 => 0,
        1..=7 => 1 + rng.below(3) as usize,
        _ => 4 + rng.below(if evaluable { 5 } else { 20 }) as usize,
    };
    let p = deg(rng);
    let q = deg(rng);
    let n = p + rng.below(5) as usize;
    let m = q + rng.below(5) as usize;
    let r = if rng.chance(0.9) { n + p + 1 } else { 2 * p + 1 + rng.below(n as u64 + 2) as usize };
    let s = if rng.chance(0.9) { m + q + 1 } else { 2 * q + 1 + rng.below(m as u64 + 2) as usize };
    let n = n.max(r.saturating_sub(p + 1));
    let m = m.max(s.saturating_sub(q + 1));
    let mut values: Vec<Vec<f64>> = (0..=n).map(|_| (0..=m).map(|_| value(rng)).collect()).collect();
    let mut u = knots(rng, r + 1, p);
    let mut v = knots(rng, s + 1, q);
    let mut used = None;
    if rng.chance(0.2) {
        let (en, em) = (rng.below(3) as usize, rng.below(3) as usize);
        for row in &mut values {
            for _ in 0..em {
                row.push(value(rng));
            }
        }
        for _ in 0..en {
            values.push((0..=m + em).map(|_| value(rng)).collect());
        }
        for _ in 0..rng.below(3) {
            u.push(rng.uniform(-9.0, 9.0));
        }
        for _ in 0..rng.below(3) {
            v.push(rng.uniform(-9.0, 9.0));
        }
        used = Some([n, m, r, s]);
    }
    SrfIn {
        values,
        p,
        q,
        u,
        v,
        used,
    }
}

fn crv_used(c: &CrvIn) -> [usize; 2] {
    c.used
        .unwrap_or([c.values.len().saturating_sub(1), c.knots.len().saturating_sub(1)])
}

fn srf_storage(s: &SrfIn) -> [usize; 4] {
    [
        s.values.len().saturating_sub(1),
        s.values.first().map_or(0, Vec::len).saturating_sub(1),
        s.u.len().saturating_sub(1),
        s.v.len().saturating_sub(1),
    ]
}

fn srf_used(s: &SrfIn) -> [usize; 4] {
    s.used.unwrap_or_else(|| srf_storage(s))
}

/// An index near `k`: equal, below, or a little above.
fn near(rng: &mut Rng, k: usize) -> usize {
    match rng.below(3) {
        0 => k,
        1 => rng.below(k as u64 + 1) as usize,
        _ => rng.below(k as u64 + 4) as usize,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0..=3 => {
            let crv = gen_crv(rng, true);
            let used = crv_used(&crv);
            let k_used = crv.knots.get(..=used[1]).unwrap_or(&crv.knots).to_vec();
            let der = rng.below(crv.p as u64 + 3) as usize;
            let params = (0..=rng.below(12))
                .map(|_| CrvPt {
                    u: param(rng, &k_used),
                    flag: flag_of(rng),
                })
                .collect();
            Inputs::CrvEval { crv, der, params }
        }
        4 => {
            let crv = gen_crv(rng, false);
            let used = crv_used(&crv);
            Inputs::CrvInspect {
                sized: [near(rng, used[0]), near(rng, used[1])],
                crv,
            }
        }
        5 => {
            let crv = gen_crv(rng, false);
            let used = crv_used(&crv);
            let big = [crv.values.len() - 1, crv.knots.len() - 1];
            let kind = match rng.below(5) {
                0 => CrvKind::Compact,
                1 => CrvKind::SetSize {
                    idx: [
                        rng.below(big[0] as u64 + 1) as usize,
                        rng.below(8) as usize,
                        rng.below(big[1] as u64 + 1) as usize,
                    ],
                },
                2 => {
                    let lf = used[0] + 1 + rng.below(3) as usize;
                    let lu = used[1] + 1 + rng.below(3) as usize;
                    CrvKind::SetPtrs {
                        values: (0..lf).map(|_| value(rng)).collect(),
                        knots: (0..lu).map(|_| rng.uniform(-5.0, 5.0)).collect(),
                    }
                }
                3 => CrvKind::SizeArrays {
                    idx: [near(rng, used[0]), rng.below(8) as usize, near(rng, used[1])],
                },
                _ => CrvKind::SizeArraysNull {
                    idx: [rng.below(40) as usize, rng.below(8) as usize, rng.below(40) as usize],
                },
            };
            Inputs::CrvModify { crv, kind }
        }
        6 | 7 => {
            let srf = gen_srf(rng, true);
            let used = srf_used(&srf);
            let u_used = srf.u.get(..=used[2]).unwrap_or(&srf.u).to_vec();
            let v_used = srf.v.get(..=used[3]).unwrap_or(&srf.v).to_vec();
            let params = (0..=rng.below(12))
                .map(|_| SrfPt {
                    u: param(rng, &u_used),
                    v: param(rng, &v_used),
                    ufl: flag_of(rng),
                    vfl: flag_of(rng),
                })
                .collect();
            Inputs::SrfEval { srf, params }
        }
        8 => {
            let srf = gen_srf(rng, false);
            let used = srf_used(&srf);
            Inputs::SrfInspect {
                sized: [
                    near(rng, used[0]),
                    near(rng, used[1]),
                    near(rng, used[2]),
                    near(rng, used[3]),
                ],
                srf,
            }
        }
        _ => {
            let srf = gen_srf(rng, false);
            let used = srf_used(&srf);
            let big = srf_storage(&srf);
            let kind = match rng.below(3) {
                0 => SrfKind::SetSize {
                    idx: [
                        rng.below(big[0] as u64 + 1) as usize,
                        rng.below(big[1] as u64 + 1) as usize,
                        rng.below(6) as usize,
                        rng.below(6) as usize,
                        rng.below(big[2] as u64 + 1) as usize,
                        rng.below(big[3] as u64 + 1) as usize,
                    ],
                },
                1 => SrfKind::SizeArrays {
                    idx: [
                        near(rng, used[0]),
                        near(rng, used[1]),
                        rng.below(6) as usize,
                        rng.below(6) as usize,
                        near(rng, used[2]),
                        near(rng, used[3]),
                    ],
                },
                _ => SrfKind::SizeArraysNull {
                    idx: [
                        rng.below(12) as usize,
                        rng.below(12) as usize,
                        rng.below(6) as usize,
                        rng.below(6) as usize,
                        rng.below(20) as usize,
                        rng.below(20) as usize,
                    ],
                },
            };
            Inputs::SrfModify { srf, kind }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.funcs_basic"
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
