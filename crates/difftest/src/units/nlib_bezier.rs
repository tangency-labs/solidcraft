//! Runner for unit `nlib.bezier`: NLib Bezier routines (degree elevation and
//! product matrices, elevation of curves and surface rows, function/curve products,
//! one Bernstein polynomial, degree reduction, splitting, arc length, extension of
//! curves and surface strips).
//!
//! C++: `solidcore_ffi::units::nlib_bezier` (upstream `source/SMLib/src/NL_Bezier.cpp`).
//! Rust: `solidcore::nurbs::bezier`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` first (`0` ok, else the
//! `NL_*_ERR` number; `1000` for an error flag without a number, which upstream
//! returns when `ST_BezCalcArcLength` passes `NL_RECURSELIM`; `-1` on the Rust side
//! for any other error, which never matches), then the op's outputs on success.
//! Control points are `[x, y, z, w]` (`w = NOW` non-rational, `z = NOZ` 2D); a
//! fixture may give `[x, y, z]` triples (non-rational), e.g. from a `$pts` file.
//! Surfaces are inline or an upstream `.sms` file (`{"sms": {"$upstream_file": ..}}`).
//! Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::bezier::{
    calc_arc_length, calc_length, crv_extend, degree_reduce_coefs, elevate_degree,
    elevation_matrix, eval_one_basis, func_multiply_crv, func_multiply_crv_4d,
    func_multiply_func, product_matrix, reduce_degree, split, srf_elevate_degree,
    srf_extend_4d, ExtendAt,
};
use solidcore::nurbs::math::{CPoint, RealMatrix, NOW, NOZ};
use solidcore::nurbs::srf_basic::{Direction, Surface, SurfaceIndices};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_bezier::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.bezier` runner.
pub struct Runner;

/// A control point list: `[x, y, z, w]` quadruples or `[x, y, z]` triples.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Cpts {
    Four(Vec<[f64; 4]>),
    Three(Vec<[f64; 3]>),
}

impl Cpts {
    /// The first `k` points as quadruples.
    fn first(&self, k: usize) -> Result<Vec<[f64; 4]>, HarnessError> {
        let all: Vec<[f64; 4]> = match self {
            Cpts::Four(v) => v.clone(),
            Cpts::Three(v) => v.iter().map(|p| [p[0], p[1], p[2], NOW]).collect(),
        };
        all.get(..k)
            .map(<[[f64; 4]]>::to_vec)
            .ok_or_else(|| harness(format!("need {k} control points, have {}", all.len())))
    }
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

/// Serde for scalars that may be non-finite: JSON numbers, or `"NaN"` / `"inf"` /
/// `"-inf"` (JSON has no non-finite numbers).
mod nf {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;

    #[allow(clippy::trivially_copy_pass_by_ref)] // serde's `with` signature
    pub fn serialize<S: Serializer>(x: &f64, s: S) -> Result<S::Ok, S::Error> {
        if x.is_nan() {
            s.serialize_str("NaN")
        } else if x.is_infinite() {
            s.serialize_str(if *x > 0.0 { "inf" } else { "-inf" })
        } else {
            s.serialize_f64(*x)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => n.as_f64().ok_or_else(|| D::Error::custom("bad number")),
            Value::String(s) if s == "NaN" => Ok(f64::NAN),
            Value::String(s) if s == "inf" => Ok(f64::INFINITY),
            Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
            other => Err(D::Error::custom(format!("bad float {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    ElevMatrix {
        p: usize,
        t: usize,
    },
    ProdMatrix {
        p: usize,
        q: usize,
    },
    Elevate {
        p: usize,
        t: usize,
        f: usize,
        l: usize,
        pre: bool,
        pw: Cpts,
    },
    FuncFunc {
        p: usize,
        q: usize,
        s: usize,
        e: usize,
        pre: bool,
        f: Vec<f64>,
        g: Vec<f64>,
    },
    FuncCrv {
        p: usize,
        q: usize,
        s: usize,
        e: usize,
        pre: bool,
        four_d: bool,
        f: Vec<f64>,
        pw: Cpts,
    },
    SrfElevate {
        r: usize,
        t: usize,
        dir: i32,
        f: usize,
        l: usize,
        roc: usize,
        pre: bool,
        net: Vec<Vec<[f64; 4]>>,
    },
    OneBasis {
        p: usize,
        i: usize,
        #[serde(with = "nf")]
        u: f64,
    },
    ReduceCoefs {
        p: usize,
    },
    Reduce {
        p: usize,
        pw: Cpts,
    },
    Split {
        p: usize,
        #[serde(with = "nf")]
        u: f64,
        pw: Cpts,
    },
    ArcLength {
        p: usize,
        #[serde(with = "nf")]
        eps: f64,
        #[serde(with = "nf")]
        len: f64,
        pw: Cpts,
    },
    Length {
        p: usize,
        #[serde(with = "nf")]
        tol: f64,
        pw: Cpts,
    },
    CrvExtend {
        p: usize,
        whr: i32,
        rev: bool,
        pw: Cpts,
    },
    SrfExtend {
        dir: i32,
        whr: i32,
        rev: bool,
        in_place: bool,
        sur: SrfIn,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- surface inputs ---------------------------------------------------------------

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let mut it = text.split_whitespace();
    let mut num = || -> Result<f64, HarnessError> {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))?
            .parse::<f64>()
            .map_err(|e| harness(format!("{path}: {e}")))
    };
    let int = |v: f64| -> Result<usize, HarnessError> {
        if v.fract() != 0.0 || v < 0.0 {
            return Err(harness(format!("{path}: bad count {v}")));
        }
        Ok(v as usize)
    };
    let n = int(num()?)?;
    let m = int(num()?)?;
    let p = int(num()?)?;
    let q = int(num()?)?;
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

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

/// Storage sizes `(N, M, R, S)` and indices in use `(n, m, r, s)`.
fn shape(s: &InlineSrf) -> Result<([usize; 4], [usize; 4]), HarnessError> {
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
    let (big, used) = shape(s)?;
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

fn build_srf(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let (_, used) = shape(s)?;
    let idx = SurfaceIndices {
        n: used[0],
        m: used[1],
        p: s.p,
        q: s.q,
        r: used[2],
        s: used[3],
    };
    Surface::with_indices(to_net(&s.net), s.u.clone(), s.v.clone(), idx)
    .map_err(|e| harness(format!("rust surface: {e}")))
}

fn to_cpts(v: &[[f64; 4]]) -> Vec<CPoint> {
    v.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect()
}

fn to_net(net: &[Vec<[f64; 4]>]) -> Vec<Vec<CPoint>> {
    net.iter().map(|row| to_cpts(row)).collect()
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

/// `r = (p - 1) / 2` with C integer division.
fn mid(p: usize) -> usize {
    p.saturating_sub(1) / 2
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    let mut out = Output::new().values("status", status);
    let rest = |at: &mut usize| -> Result<Vec<f64>, HarnessError> {
        let k = flat.len() - *at;
        Ok(take(flat, at, k)?.to_vec())
    };
    if ok {
        match i {
            Inputs::ElevMatrix { .. } | Inputs::ProdMatrix { .. } => {
                out = out.values("band", rest(&mut at)?);
            }
            Inputs::Elevate { .. }
            | Inputs::FuncCrv { .. }
            | Inputs::SrfElevate { .. }
            | Inputs::CrvExtend { .. } => {
                out = out.values("qw", rest(&mut at)?);
            }
            Inputs::FuncFunc { .. } => out = out.values("fg", rest(&mut at)?),
            Inputs::OneBasis { .. } => out = out.values("b", take(flat, &mut at, 1)?.to_vec()),
            Inputs::ReduceCoefs { p } => {
                let r = mid(*p);
                let nb = if *p >= 2 { p - 1 - r } else { 0 };
                for (key, k) in [("alf", r), ("oma", r), ("bet", nb), ("omb", nb)] {
                    out = out.values(key, take(flat, &mut at, k)?.to_vec());
                }
            }
            Inputs::Reduce { p, .. } => {
                out = out.values("qw", take(flat, &mut at, 4 * p)?.to_vec());
                out = out.values("err", take(flat, &mut at, 1)?.to_vec());
            }
            Inputs::Split { p, .. } => {
                out = out.values("qw", take(flat, &mut at, 4 * (p + 1))?.to_vec());
                out = out.values("rw", take(flat, &mut at, 4 * (p + 1))?.to_vec());
            }
            Inputs::ArcLength { .. } => {
                out = out.values("eps", take(flat, &mut at, 1)?.to_vec());
                out = out.values("len", take(flat, &mut at, 1)?.to_vec());
            }
            Inputs::Length { .. } => out = out.values("len", take(flat, &mut at, 1)?.to_vec()),
            Inputs::SrfExtend { .. } => {
                let dims = take(flat, &mut at, 6)?;
                let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
                let (n, m, r, s) = (dim(0)?, dim(1)?, dim(4)?, dim(5)?);
                out = out.values("dims", dims.to_vec());
                out = out.values("net", take(flat, &mut at, 4 * (n + 1) * (m + 1))?.to_vec());
                out = out.values("u", take(flat, &mut at, r + 1)?.to_vec());
                out = out.values("v", take(flat, &mut at, s + 1)?.to_vec());
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

fn push_cpts(o: &mut Vec<f64>, pts: &[[f64; 4]]) {
    o.extend(pts.iter().flatten());
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = match i {
        Inputs::ElevMatrix { p, t } => {
            o.extend([*p as f64, *t as f64]);
            Op::ElevMatrix
        }
        Inputs::ProdMatrix { p, q } => {
            o.extend([*p as f64, *q as f64]);
            Op::ProdMatrix
        }
        Inputs::Elevate { p, t, f, l, pre, pw } => {
            o.extend([*p as f64, *t as f64, *f as f64, *l as f64, flag(*pre)]);
            push_cpts(&mut o, &pw.first(p + 1)?);
            Op::Elevate
        }
        Inputs::FuncFunc {
            p,
            q,
            s,
            e,
            pre,
            f,
            g,
        } => {
            o.extend([*p as f64, *q as f64, *s as f64, *e as f64, flag(*pre)]);
            o.extend_from_slice(f.get(..=*p).ok_or_else(|| harness("f too short"))?);
            o.extend_from_slice(g.get(..=*q).ok_or_else(|| harness("g too short"))?);
            Op::FuncFunc
        }
        Inputs::FuncCrv {
            p,
            q,
            s,
            e,
            pre,
            four_d,
            f,
            pw,
        } => {
            o.extend([*p as f64, *q as f64, *s as f64, *e as f64, flag(*pre)]);
            o.extend_from_slice(f.get(..=*p).ok_or_else(|| harness("f too short"))?);
            push_cpts(&mut o, &pw.first(q + 1)?);
            if *four_d {
                Op::FuncCrv4d
            } else {
                Op::FuncCrv
            }
        }
        Inputs::SrfElevate {
            r,
            t,
            dir,
            f,
            l,
            roc,
            pre,
            net,
        } => {
            let cols = net.first().map_or(0, Vec::len);
            if net.iter().any(|row| row.len() != cols) {
                return Err(harness("net rows differ in length"));
            }
            o.extend([
                *r as f64,
                *t as f64,
                f64::from(*dir),
                *f as f64,
                *l as f64,
                *roc as f64,
                flag(*pre),
                net.len() as f64,
                cols as f64,
            ]);
            o.extend(net.iter().flatten().flatten());
            Op::SrfElevate
        }
        Inputs::OneBasis { p, i, u } => {
            o.extend([*p as f64, *i as f64, *u]);
            Op::OneBasis
        }
        Inputs::ReduceCoefs { p } => {
            o.push(*p as f64);
            Op::ReduceCoefs
        }
        Inputs::Reduce { p, pw } => {
            o.push(*p as f64);
            push_cpts(&mut o, &pw.first(p + 1)?);
            Op::Reduce
        }
        Inputs::Split { p, u, pw } => {
            o.extend([*p as f64, *u]);
            push_cpts(&mut o, &pw.first(p + 1)?);
            Op::Split
        }
        Inputs::ArcLength { p, eps, len, pw } => {
            o.extend([*p as f64, *eps, *len]);
            push_cpts(&mut o, &pw.first(p + 1)?);
            Op::ArcLength
        }
        Inputs::Length { p, tol, pw } => {
            o.extend([*p as f64, *tol]);
            push_cpts(&mut o, &pw.first(p + 1)?);
            Op::Length
        }
        Inputs::CrvExtend { p, whr, rev, pw } => {
            o.extend([*p as f64, f64::from(*whr), flag(*rev)]);
            push_cpts(&mut o, &pw.first(p + 1)?);
            Op::CrvExtend
        }
        Inputs::SrfExtend {
            dir,
            whr,
            rev,
            in_place,
            sur,
        } => {
            o.extend([f64::from(*dir), f64::from(*whr), flag(*rev), flag(*in_place)]);
            encode_srf(&resolve(sur)?, &mut o)?;
            Op::SrfExtend
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

/// `0` ok, the `NL_*_ERR` number, `1000` for the arc-length recursion limit, `-1`
/// otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(SolidError::NotConverged { .. }) => 1000.0,
        Err(_) => -1.0,
    }
}

fn push_cpt(o: &mut Vec<f64>, c: CPoint) {
    o.extend_from_slice(&c.to_array());
}

fn push_band(o: &mut Vec<f64>, a: &RealMatrix, rows: usize, shift: usize, hi: usize) {
    for i in 0..=rows {
        for j in i.saturating_sub(shift)..=i.min(hi) {
            o.push(a.get(i, j).unwrap_or(f64::NAN));
        }
    }
}

/// Runs `r`, pushing its status and, on success, what `put` writes.
fn emit<T>(o: &mut Vec<f64>, r: SolidResult<T>, put: impl FnOnce(&mut Vec<f64>, T)) {
    o.push(status(&r));
    if let Ok(v) = r {
        put(o, v);
    }
}

fn pre_matrix(pre: bool, build: impl FnOnce() -> SolidResult<RealMatrix>) -> SolidResult<RealMatrix> {
    if pre {
        build()
    } else {
        Ok(RealMatrix::null())
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::ElevMatrix { p, t } => {
            emit(&mut o, elevation_matrix(*p, *t), |o, m| push_band(o, &m, p + t, *t, *p));
        }
        Inputs::ProdMatrix { p, q } => {
            emit(&mut o, product_matrix(*p, *q), |o, m| push_band(o, &m, p + q, *q, *p));
        }
        Inputs::Elevate { p, t, f, l, pre, pw } => {
            let pw = to_cpts(&pw.first(p + 1)?);
            let mut qw = vec![CPoint::ZERO; l + 1];
            let r = pre_matrix(*pre, || elevation_matrix(*p, *t))
                .and_then(|dm| elevate_degree(&pw, *p, *t, &dm, *f, *l, &mut qw));
            emit(&mut o, r, |o, ()| {
                for c in qw.iter().skip(*f) {
                    push_cpt(o, *c);
                }
            });
        }
        Inputs::FuncFunc {
            p,
            q,
            s,
            e,
            pre,
            f,
            g,
        } => {
            let mut fg = vec![0.0; (*e).max(p + q) + 1];
            let r = pre_matrix(*pre, || product_matrix(*p, *q)).and_then(|pm| {
                func_multiply_func(
                    f.get(..=*p).unwrap_or(&[]),
                    *p,
                    g.get(..=*q).unwrap_or(&[]),
                    *q,
                    &pm,
                    *s,
                    *e,
                    &mut fg,
                )
            });
            emit(&mut o, r, |o, ()| {
                o.extend(fg.iter().skip(*s).take(e + 1 - s));
            });
        }
        Inputs::FuncCrv {
            p,
            q,
            s,
            e,
            pre,
            four_d,
            f,
            pw,
        } => {
            let pw = to_cpts(&pw.first(q + 1)?);
            let mut qw = vec![CPoint::ZERO; (*e).max(p + q) + 1];
            let fv = f.get(..=*p).unwrap_or(&[]);
            let r = pre_matrix(*pre, || product_matrix(*p, *q)).and_then(|pm| {
                if *four_d {
                    func_multiply_crv_4d(fv, *p, &pw, *q, &pm, *s, *e, &mut qw)
                } else {
                    func_multiply_crv(fv, *p, &pw, *q, &pm, *s, *e, &mut qw)
                }
            });
            emit(&mut o, r, |o, ()| {
                for c in qw.iter().skip(*s).take(e + 1 - s) {
                    push_cpt(o, *c);
                }
            });
        }
        Inputs::SrfElevate {
            r,
            t,
            dir,
            f,
            l,
            roc,
            pre,
            net,
        } => {
            let d = Direction::from_flag(*dir).ok_or_else(|| harness("bad direction"))?;
            let pw = to_net(net);
            let rows = net.len();
            let cols = net.first().map_or(0, Vec::len);
            let qr = if d == Direction::U { l + 1 } else { rows };
            let qc = if d == Direction::V { l + 1 } else { cols };
            let mut qw = vec![vec![CPoint::ZERO; qc]; qr];
            let res = pre_matrix(*pre, || elevation_matrix(*r, *t))
                .and_then(|dm| srf_elevate_degree(&pw, *r, *t, &dm, d, *f, *l, *roc, &mut qw));
            emit(&mut o, res, |o, ()| {
                for k in *f..=*l {
                    let c = if d == Direction::U {
                        qw.get(k).and_then(|row| row.get(*roc))
                    } else {
                        qw.get(*roc).and_then(|row| row.get(k))
                    };
                    push_cpt(o, c.copied().unwrap_or(CPoint::new(f64::NAN, f64::NAN, f64::NAN, f64::NAN)));
                }
            });
        }
        Inputs::OneBasis { p, i, u } => {
            emit(&mut o, eval_one_basis(*p, *i, *u), Vec::push);
        }
        Inputs::ReduceCoefs { p } => {
            let c = degree_reduce_coefs(*p);
            let r = mid(*p);
            o.push(0.0);
            o.extend(c.alf.iter().take(r + 1).skip(1));
            o.extend(c.oma.iter().take(r + 1).skip(1));
            if *p >= 2 {
                o.extend(c.bet.iter().take(p - 1).skip(r));
                o.extend(c.omb.iter().take(p - 1).skip(r));
            }
        }
        Inputs::Reduce { p, pw } => {
            let pw = to_cpts(&pw.first(p + 1)?);
            emit(&mut o, reduce_degree(&pw, *p, &degree_reduce_coefs(*p)), |o, red| {
                for c in red.qw {
                    push_cpt(o, c);
                }
                o.push(red.err);
            });
        }
        Inputs::Split { p, u, pw } => {
            let pw = to_cpts(&pw.first(p + 1)?);
            emit(&mut o, split(&pw, *p, *u), |o, (q, r)| {
                for c in q.into_iter().chain(r) {
                    push_cpt(o, c);
                }
            });
        }
        Inputs::ArcLength { p, eps, len, pw } => {
            let pw = to_cpts(&pw.first(p + 1)?);
            emit(&mut o, calc_arc_length(&pw, *p, *eps, *len), |o, a| {
                o.extend([a.eps, a.len]);
            });
        }
        Inputs::Length { p, tol, pw } => {
            let pw = to_cpts(&pw.first(p + 1)?);
            emit(&mut o, calc_length(&pw, *p, *tol), Vec::push);
        }
        Inputs::CrvExtend { p, whr, rev, pw } => {
            let whr = ExtendAt::from_flag(*whr).ok_or_else(|| harness("bad whr flag"))?;
            let pw = to_cpts(&pw.first(p + 1)?);
            emit(&mut o, crv_extend(&pw, *p, whr, *rev), |o, q| {
                for c in q {
                    push_cpt(o, c);
                }
            });
        }
        Inputs::SrfExtend {
            dir, whr, rev, sur, ..
        } => {
            let d = Direction::from_flag(*dir).ok_or_else(|| harness("bad direction"))?;
            let whr = ExtendAt::from_flag(*whr).ok_or_else(|| harness("bad whr flag"))?;
            let s = build_srf(&resolve(sur)?)?;
            emit(&mut o, srf_extend_4d(&s, d, whr, *rev), |o, q| {
                let (n, m, r, ss) = q.array_sizes();
                let (p, qq) = q.degrees();
                o.extend([n, m, p, qq, r, ss].map(|k| k as f64));
                for row in q.rows() {
                    for c in row {
                        push_cpt(o, *c);
                    }
                }
                o.extend_from_slice(q.u_knots());
                o.extend_from_slice(q.v_knots());
            });
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation -------------------------------------------------------------------

fn coord(rng: &mut Rng) -> f64 {
    match rng.below(12) {
        0 => 0.0,
        1 => rng.uniform(-1e4, 1e4),
        2 => rng.uniform(-1.0, 1.0) * 1e-6,
        _ => rng.uniform(-10.0, 10.0),
    }
}

fn weight(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 1.0,
        1 => *rng.choose(&[1e-3, 100.0, 0.0, -0.5]).unwrap_or(&1.0),
        _ => rng.uniform(0.3, 3.0),
    }
}

/// `k` random control points: non-rational, rational, 2D, or mixed.
fn gen_cpts(rng: &mut Rng, k: usize) -> Vec<[f64; 4]> {
    let rational = rng.chance(0.5);
    let two_d = rng.chance(0.1);
    let mixed = rng.chance(0.05);
    let collinear = rng.chance(0.08);
    let (a, b) = ([coord(rng), coord(rng), coord(rng)], [coord(rng), coord(rng), coord(rng)]);
    (0..k)
        .map(|i| {
            let (x, y, z) = if collinear {
                let s = if k > 1 { i as f64 / (k - 1) as f64 } else { 0.0 };
                (
                    a[0] + s * (b[0] - a[0]),
                    a[1] + s * (b[1] - a[1]),
                    a[2] + s * (b[2] - a[2]),
                )
            } else {
                (coord(rng), coord(rng), coord(rng))
            };
            let z = if two_d || (mixed && rng.chance(0.3)) { NOZ } else { z };
            let rat = if mixed { rng.chance(0.5) } else { rational };
            if rat {
                let w = weight(rng);
                let zw = if z == NOZ { NOZ } else { z * w };
                [x * w, y * w, zw, w]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect()
}

fn small(rng: &mut Rng, hi: u64) -> usize {
    rng.below(hi + 1) as usize
}

/// A degree pair with sum at most 17 (mostly small).
fn pair(rng: &mut Rng) -> (usize, usize) {
    if rng.chance(0.15) {
        let a = small(rng, 17);
        (a, small(rng, (17 - a) as u64))
    } else {
        let a = small(rng, 6);
        (a, small(rng, 5))
    }
}

/// A parameter: mostly in (0, 1), sometimes an end, outside, or NaN.
fn param(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        0 => 0.0,
        1 => 1.0,
        2 => rng.uniform(-0.5, 0.0),
        3 => rng.uniform(1.0, 1.5),
        4 => *rng.choose(&[f64::NAN, 0.5, 1e-300, 1.0 - 1e-16]).unwrap_or(&0.5),
        _ => rng.unit(),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(15) {
        0 => {
            let (p, t) = pair(rng);
            Inputs::ElevMatrix { p, t }
        }
        1 => {
            let (p, q) = pair(rng);
            Inputs::ProdMatrix { p, q }
        }
        2 => {
            let (p, t) = pair(rng);
            let q = p + t;
            let (f, l) = if rng.chance(0.7) {
                (0, q)
            } else {
                let l = small(rng, q as u64);
                (small(rng, l as u64 + 1), l)
            };
            Inputs::Elevate {
                p,
                t,
                f,
                l,
                pre: rng.chance(0.3),
                pw: Cpts::Four(gen_cpts(rng, p + 1)),
            }
        }
        3 | 4 => {
            let (p, q) = pair(rng);
            let n = p + q;
            let (s, e) = if rng.chance(0.7) {
                (0, n)
            } else if rng.chance(0.8) {
                let e = small(rng, n as u64);
                (small(rng, e as u64), e)
            } else {
                // NL_INP_ERR: s > e or e > p + q.
                (small(rng, n as u64 + 3), small(rng, n as u64 + 3))
            };
            let f: Vec<f64> = (0..=p).map(|_| coord(rng)).collect();
            let pre = rng.chance(0.3);
            if rng.below(3) == 0 {
                Inputs::FuncFunc {
                    p,
                    q,
                    s,
                    e,
                    pre,
                    f,
                    g: (0..=q).map(|_| coord(rng)).collect(),
                }
            } else {
                Inputs::FuncCrv {
                    p,
                    q,
                    s,
                    e,
                    pre,
                    four_d: rng.chance(0.5),
                    f,
                    pw: Cpts::Four(gen_cpts(rng, q + 1)),
                }
            }
        }
        5 => {
            let (r, t) = pair(rng);
            let dir = if rng.chance(0.05) { 3 } else { 1 + rng.below(2) as i32 };
            let roc = small(rng, 3);
            let (rows, cols) = match dir {
                1 => (r + 1 + small(rng, 1), roc + 1 + small(rng, 1)),
                2 => (roc + 1 + small(rng, 1), r + 1 + small(rng, 1)),
                _ => (1 + small(rng, 3), 1 + small(rng, 3)),
            };
            let q = r + t;
            let (f, l) = if rng.chance(0.7) {
                (0, q)
            } else {
                let l = small(rng, q as u64);
                (small(rng, l as u64 + 1), l)
            };
            let flat = gen_cpts(rng, rows * cols);
            let net = flat.chunks(cols).map(<[[f64; 4]]>::to_vec).collect();
            Inputs::SrfElevate {
                r,
                t,
                dir,
                f,
                l,
                roc,
                pre: rng.chance(0.3),
                net,
            }
        }
        6 => {
            let p = if rng.chance(0.9) { small(rng, 10) } else { small(rng, 40) };
            let i = if rng.chance(0.9) { small(rng, p as u64) } else { small(rng, p as u64 + 3) };
            Inputs::OneBasis { p, i, u: param(rng) }
        }
        7 => Inputs::ReduceCoefs { p: small(rng, 25) },
        8 => {
            let p = 2 + if rng.chance(0.9) { small(rng, 8) } else { small(rng, 25) };
            Inputs::Reduce {
                p,
                pw: Cpts::Four(gen_cpts(rng, p + 1)),
            }
        }
        9 => {
            let p = if rng.chance(0.9) { small(rng, 8) } else { small(rng, 25) };
            Inputs::Split {
                p,
                u: param(rng),
                pw: Cpts::Four(gen_cpts(rng, p + 1)),
            }
        }
        10 | 11 => {
            let p = small(rng, 5);
            let pw = gen_cpts(rng, p + 1);
            // Relative tolerances keep the subdivision shallow (<= ~2^14 leaves).
            let scale = pw
                .iter()
                .flat_map(|c| c.iter().take(2).copied())
                .fold(0.0f64, |a, x| a.max(x.abs()))
                .max(1e-3);
            let tol = match rng.below(10) {
                0 => 0.0,
                1 => *rng.choose(&[1e300, f64::NAN, -1.0]).unwrap_or(&1.0),
                _ => scale * *rng.choose(&[1e-1, 1e-2, 1e-3, 5e-4]).unwrap_or(&1e-2),
            };
            if rng.chance(0.5) {
                Inputs::Length {
                    p,
                    tol,
                    pw: Cpts::Four(pw),
                }
            } else {
                Inputs::ArcLength {
                    p,
                    eps: tol,
                    len: if rng.chance(0.5) { 0.0 } else { rng.uniform(0.0, 100.0) },
                    pw: Cpts::Four(pw),
                }
            }
        }
        12 => {
            let p = if rng.chance(0.85) { small(rng, 6) } else { small(rng, 17) };
            Inputs::CrvExtend {
                p,
                whr: 1 + rng.below(2) as i32,
                rev: rng.chance(0.5),
                pw: Cpts::Four(gen_cpts(rng, p + 1)),
            }
        }
        _ => {
            let dir = 1 + rng.below(2) as i32;
            let (p, q) = (small(rng, 5), small(rng, 5));
            let bez = rng.chance(0.9);
            let n = if dir == 1 && bez { p } else { small(rng, 6) };
            let m = if dir == 2 && bez { q } else { small(rng, 6) };
            let flat = gen_cpts(rng, (n + 1) * (m + 1));
            let net = flat.chunks(m + 1).map(<[[f64; 4]]>::to_vec).collect();
            let u: Vec<f64> = (0..n + p + 2).map(|k| k as f64).collect();
            let v: Vec<f64> = (0..m + q + 2).map(|k| rng.uniform(0.0, 1.0) + k as f64).collect();
            Inputs::SrfExtend {
                dir,
                whr: 1 + rng.below(2) as i32,
                rev: rng.chance(0.5),
                in_place: rng.chance(0.3),
                sur: SrfIn::Inline(InlineSrf {
                    net,
                    p,
                    q,
                    u,
                    v,
                    used: None,
                }),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.bezier"
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
