//! Runner for unit `nlib.srf_basic`: the NLib surface object (`NL_SURFACE`):
//! validity checks, accessors, storage sizing and copying, u/v swap, rationality
//! conversions, closedness, equality, cheap flatness and average lengths.
//!
//! C++: `solidcore_ffi::units::nlib_srf_basic` (upstream
//! `source/SMLib/src/NL_SrfBasic.cpp`, `N_SrfGetAverageLen` of `NL_SrfGeom.cpp`).
//! Rust: `solidcore::nurbs::srf_basic`.
//!
//! Each case runs one *op* (`inputs.op`, a group of upstream calls). Both sides
//! produce the same flat record of doubles, split into keys by [`to_output`].
//! Statuses are inlined: `0` ok, else the `NL_*_ERR` number (`-1` on the Rust side
//! for a non-NLib error, which never matches). Outputs upstream leaves unwritten are
//! NaN on both sides. A surface is dumped as `dims = [n, m, p, q, r, s]`, the control
//! points in use (`net`, x y z w row-major) and the knots in use (`u`, `v`).
//!
//! A surface input is either inline (`net` storage rows of `[x, y, z, w]`, degrees,
//! knot storage, optional indices in use below the storage) or an upstream `.sms`
//! file (`{"sms": {"$upstream_file": "Surface.sms"}}`, read here by
//! [`read_sms`]). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::geom::Rectangle;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::{
    make_rat_compatible, size_arrays, Direction, Surface, SurfaceIndices,
};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_basic::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.srf_basic` runner.
pub struct Runner;

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
    SwapUv,
    NonRatToRat,
    MakeNonRat,
    PruneRat,
    Reparam { rect: [f64; 4], dir: i32 },
    SetSize { idx: [usize; 6] },
    CopyNull,
    CopyInto { dst: SrfIn },
    SizeArrays { idx: [usize; 6] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Inspect {
        sur: SrfIn,
        eps: f64,
        sized: [usize; 4],
    },
    Enet {
        sur: SrfIn,
        idx: [usize; 4],
    },
    Modify {
        sur: SrfIn,
        #[serde(flatten)]
        kind: Kind,
    },
    Equal {
        a: SrfIn,
        b: SrfIn,
        tol3d: f64,
        tol1d: f64,
    },
    RatCompatible {
        surfaces: Vec<SrfIn>,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- surface inputs ---------------------------------------------------------------

#[path = "fixtures/nlib_srf_basic_sms.rs"]
mod sms;

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let sms::SmsSurface { net, p, q, u, v } = sms::parse_sms(&text, path)?;
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

fn encode(s: &InlineSrf, o: &mut Vec<f64>) -> Result<(), HarnessError> {
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

/// `[n, m, p, q, r, s]` as [`SurfaceIndices`].
fn indices(idx: &[usize; 6]) -> SurfaceIndices {
    let [n, m, p, q, r, s] = *idx;
    SurfaceIndices { n, m, p, q, r, s }
}

fn build(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let (_, used) = shape(s)?;
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
    ("counts_are_valid", 1),
    ("weights_are_valid", 1),
    ("is_valid", 1),
    ("is_sized", 1),
    ("is_rat", 1),
    ("is_closed_u_v_uv", 3),
    ("are_weights_equal", 1),
    ("parameter_bounds", 4),
    ("sizes_n_m_r_s_p_q", 6),
    ("average_len", 4),
    ("is_flat_cheap", 3),
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

/// Splits a surface dump starting at `at` into `<pfx>dims`, `<pfx>net`, `<pfx>u`,
/// `<pfx>v`.
fn split_dump(flat: &[f64], at: &mut usize, pfx: &str, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 6)?;
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m, r, s) = (dim(0)?, dim(1)?, dim(4)?, dim(5)?);
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    let net = take(flat, at, 4 * (n + 1) * (m + 1))?;
    out = out.values(&format!("{pfx}net"), net.to_vec());
    let u = take(flat, at, r + 1)?;
    out = out.values(&format!("{pfx}u"), u.to_vec());
    let v = take(flat, at, s + 1)?;
    Ok(out.values(&format!("{pfx}v"), v.to_vec()))
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
        Inputs::Enet { .. } => {
            out = out.values("status", take(flat, &mut at, 1)?.to_vec());
            let rest = flat.len() - at;
            out = out.values("pts", take(flat, &mut at, rest)?.to_vec());
        }
        Inputs::Modify { kind, .. } => {
            out = out.values("status", take(flat, &mut at, 1)?.to_vec());
            if let Kind::SizeArrays { .. } = kind {
                out = out.values("dims", take(flat, &mut at, 6)?.to_vec());
            } else {
                out = split_dump(flat, &mut at, "", out)?;
            }
        }
        Inputs::Equal { .. } => {
            out = out.values("equal", take(flat, &mut at, 1)?.to_vec());
        }
        Inputs::RatCompatible { surfaces } => {
            // The record length is each side's own output; it keeps an empty
            // surface list (no dumps at all) from being a vacuous comparison.
            out = out.int("record_len", flat.len() as i64);
            for k in 0..surfaces.len() {
                out = split_dump(flat, &mut at, &format!("s{k}_"), out)?;
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
        Inputs::Inspect { sur, eps, sized } => {
            encode(&resolve(sur)?, &mut o)?;
            o.push(*eps);
            o.extend(sized.iter().map(|&k| k as f64));
            Op::Inspect
        }
        Inputs::Enet { sur, idx } => {
            encode(&resolve(sur)?, &mut o)?;
            o.extend(idx.iter().map(|&k| k as f64));
            Op::Enet
        }
        Inputs::Modify { sur, kind } => {
            encode(&resolve(sur)?, &mut o)?;
            match kind {
                Kind::Compress => o.push(0.0),
                Kind::SwapUv => o.push(1.0),
                Kind::NonRatToRat => o.push(2.0),
                Kind::MakeNonRat => o.push(3.0),
                Kind::PruneRat => o.push(4.0),
                Kind::Reparam { rect, dir } => {
                    o.push(5.0);
                    o.extend_from_slice(rect);
                    o.push(f64::from(*dir));
                }
                Kind::SetSize { idx } => {
                    o.push(6.0);
                    o.extend(idx.iter().map(|&k| k as f64));
                }
                Kind::CopyNull => o.push(7.0),
                Kind::CopyInto { dst } => {
                    o.push(8.0);
                    encode(&resolve(dst)?, &mut o)?;
                }
                Kind::SizeArrays { idx } => {
                    o.push(9.0);
                    o.extend(idx.iter().map(|&k| k as f64));
                }
            }
            Op::Modify
        }
        Inputs::Equal { a, b, tol3d, tol1d } => {
            encode(&resolve(a)?, &mut o)?;
            encode(&resolve(b)?, &mut o)?;
            o.push(*tol3d);
            o.push(*tol1d);
            Op::Equal
        }
        Inputs::RatCompatible { surfaces } => {
            o.push(surfaces.len() as f64);
            for s in surfaces {
                encode(&resolve(s)?, &mut o)?;
            }
            Op::RatCompatible
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

fn dump(o: &mut Vec<f64>, s: &Surface) {
    let (n, m, r, ss) = s.array_sizes();
    let (p, q) = s.degrees();
    o.extend([n, m, p, q, r, ss].map(|k| k as f64));
    for row in s.rows() {
        for c in row {
            o.extend_from_slice(&c.to_array());
        }
    }
    o.extend_from_slice(s.u_knots());
    o.extend_from_slice(s.v_knots());
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::Inspect { sur, eps, sized } => {
            let s = build(&resolve(sur)?)?;
            o.push(status(&s.counts_are_valid()));
            o.push(status(&s.weights_are_valid()));
            o.push(status(&s.is_valid()));
            o.push(status(&s.is_sized(sized[0], sized[1], sized[2], sized[3])));
            o.push(flag(s.is_rational()));
            o.push(flag(s.is_closed(Direction::U)));
            o.push(flag(s.is_closed(Direction::V)));
            o.push(flag(s.is_closed(Direction::UV)));
            o.push(flag(s.are_weights_equal()));
            let b = s.parameter_bounds();
            o.extend([b.ul, b.ur, b.vb, b.vt]);
            let (n, m, r, ss) = s.array_sizes();
            let (p, q) = s.degrees();
            o.extend([n, m, r, ss, p, q].map(|k| k as f64));
            let len = s.average_len();
            o.extend([0.0, len.area, len.lu, len.lv]);
            let f = s.is_flat_cheap(*eps);
            o.push(status(&f));
            match f {
                Ok(f) => o.extend([flag(f.flat), f64::from(f.dir.flag())]),
                Err(_) => o.extend([f64::NAN, f64::NAN]),
            }
        }
        Inputs::Enet { sur, idx } => {
            let s = build(&resolve(sur)?)?;
            let r = s.enet(idx[0], idx[1], idx[2], idx[3]);
            o.push(status(&r));
            if let Ok(rows) = r {
                for p in rows.iter().flatten() {
                    o.extend_from_slice(&p.to_array());
                }
            }
        }
        Inputs::Modify { sur, kind } => {
            let mut s = build(&resolve(sur)?)?;
            let mut st = 0.0;
            match kind {
                Kind::Compress => s.compress(),
                Kind::SwapUv => s.swap_uv(),
                Kind::NonRatToRat => s.non_rat_to_rat(),
                Kind::MakeNonRat => s.make_non_rat(),
                Kind::PruneRat => s.prune_rat(),
                Kind::Reparam { rect, dir } => {
                    let dir = Direction::from_flag(*dir)
                        .ok_or_else(|| harness("reparam: bad direction flag"))?;
                    let rect = Rectangle::new(rect[0], rect[1], rect[2], rect[3]);
                    st = status(&s.reparam_to_interval(rect, dir));
                }
                Kind::SetSize { idx } => {
                    st = status(&s.set_size_indices(indices(idx)));
                }
                Kind::CopyNull => {
                    let mut slot = None;
                    s.copy_into(&mut slot);
                    s = slot.ok_or_else(|| harness("copy_into left None"))?;
                }
                Kind::CopyInto { dst } => {
                    let mut slot = Some(build(&resolve(dst)?)?);
                    s.copy_into(&mut slot);
                    s = slot.ok_or_else(|| harness("copy_into left None"))?;
                }
                Kind::SizeArrays { idx } => {
                    let mut slot = Some(s);
                    let sized = match size_arrays(&mut slot, indices(idx)) {
                        Ok(sized) => sized,
                        Err(e) => {
                            // only an allocation failure (the FFI rejects such sizes)
                            o.push(status(&Err::<(), _>(e)));
                            return Ok(o);
                        }
                    };
                    let (n, m, r, ss) = sized.array_sizes();
                    let (p, q) = sized.degrees();
                    o.push(0.0);
                    o.extend([n, m, p, q, r, ss].map(|k| k as f64));
                    return Ok(o);
                }
            }
            o.push(st);
            dump(&mut o, &s);
        }
        Inputs::Equal { a, b, tol3d, tol1d } => {
            let a = build(&resolve(a)?)?;
            let b = build(&resolve(b)?)?;
            o.push(flag(a.equals(&b, *tol3d, *tol1d)));
        }
        Inputs::RatCompatible { surfaces } => {
            let mut srfs = surfaces
                .iter()
                .map(|s| build(&resolve(s)?))
                .collect::<Result<Vec<_>, _>>()?;
            make_rat_compatible(&mut srfs);
            for s in &srfs {
                dump(&mut o, s);
            }
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
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

/// Clamped knot vector of `cnt` knots for degree `p` (multiplicities may exceed
/// `p` or the ends may be wrong, now and then), or random knots.
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
                *y = *x; // a double knot
            }
        }
    }
    if rng.chance(0.05) && cnt > 1 {
        if let Some(k0) = k.first_mut() {
            *k0 = a - 1.0; // broken end multiplicity
        }
    }
    k
}

fn weight(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        0 => *rng.choose(&[1e-4, 5e-4, 150.0, 1e-3, 100.0, 0.0, -1.0]).unwrap_or(&1.0),
        1..=4 => 1.0,
        _ => rng.uniform(0.5, 2.0),
    }
}

/// A random surface, inline, with optional extra storage.
fn gen_srf(rng: &mut Rng) -> InlineSrf {
    let n = rng.below(6) as usize;
    let m = rng.below(6) as usize;
    let p = if rng.chance(0.9) {
        rng.below(n as u64 + 1) as usize
    } else {
        rng.below(20) as usize
    };
    let q = if rng.chance(0.9) {
        rng.below(m as u64 + 1) as usize
    } else {
        rng.below(20) as usize
    };
    let r = if rng.chance(0.9) { n + p + 1 } else { rng.below(12) as usize };
    let s = if rng.chance(0.9) { m + q + 1 } else { rng.below(12) as usize };
    let rational = rng.chance(0.5);
    let two_d = rng.chance(0.1);
    let flat_z = rng.chance(0.3);
    let all_one = rational && rng.chance(0.15);
    let mut net: Vec<Vec<[f64; 4]>> = (0..=n)
        .map(|_| {
            (0..=m)
                .map(|_| {
                    let (x, y) = (coord(rng), coord(rng));
                    let z = if two_d {
                        NOZ
                    } else if flat_z {
                        rng.uniform(-1.0, 1.0) * 1e-4
                    } else {
                        coord(rng)
                    };
                    if rational {
                        let w = if all_one { 1.0 } else { weight(rng) };
                        let zw = if z == NOZ { NOZ } else { z * w };
                        [x * w, y * w, zw, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    // Mixed rationality.
    if rng.chance(0.05) {
        let i = rng.below(n as u64 + 1) as usize;
        let j = rng.below(m as u64 + 1) as usize;
        if let Some(c) = net.get_mut(i).and_then(|row| row.get_mut(j)) {
            c[3] = if rational { NOW } else { 1.0 };
        }
    }
    // Closed in u or v (exactly, within NL_MTOL, or just outside).
    let close = |rng: &mut Rng| -> f64 {
        match rng.below(3) {
            0 => 0.0,
            1 => rng.uniform(-1.0, 1.0) * 5e-8,
            _ => rng.uniform(-1.0, 1.0) * 1e-6,
        }
    };
    if rng.chance(0.2) {
        let e = close(rng);
        let mut first = net.first().cloned().unwrap_or_default();
        for c in &mut first {
            c[0] += e;
        }
        if let Some(last) = net.get_mut(n) {
            *last = first;
        }
    }
    if rng.chance(0.2) {
        let e = close(rng);
        for row in &mut net {
            if let Some(&c) = row.first() {
                if let Some(last) = row.get_mut(m) {
                    *last = [c[0], c[1] + e, c[2], c[3]];
                }
            }
        }
    }
    let mut u = knots(rng, r + 1, p);
    let mut v = knots(rng, s + 1, q);
    let mut used = None;
    if rng.chance(0.2) {
        // Extra storage beyond the indices in use, filled with junk.
        let (en, em) = (rng.below(3) as usize, rng.below(3) as usize);
        for row in &mut net {
            for _ in 0..em {
                row.push([coord(rng), coord(rng), coord(rng), weight(rng)]);
            }
        }
        for _ in 0..en {
            net.push((0..=m + em).map(|_| [coord(rng), coord(rng), coord(rng), NOW]).collect());
        }
        for _ in 0..rng.below(3) {
            u.push(rng.uniform(-9.0, 9.0));
        }
        for _ in 0..rng.below(3) {
            v.push(rng.uniform(-9.0, 9.0));
        }
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

fn storage(s: &InlineSrf) -> [usize; 4] {
    [
        s.net.len().saturating_sub(1),
        s.net.first().map_or(0, Vec::len).saturating_sub(1),
        s.u.len().saturating_sub(1),
        s.v.len().saturating_sub(1),
    ]
}

fn in_use(s: &InlineSrf) -> [usize; 4] {
    s.used.unwrap_or_else(|| storage(s))
}

/// A perturbed copy of `s` (for equality tests).
fn near_copy(rng: &mut Rng, s: &InlineSrf) -> InlineSrf {
    let mut t = s.clone();
    match rng.below(6) {
        0 => {}
        1 => {
            let i = rng.below(t.net.len() as u64) as usize;
            let j = rng.below(t.net.first().map_or(1, Vec::len) as u64) as usize;
            let k = rng.below(4) as usize;
            let d = rng.uniform(-1.0, 1.0) * 1e-5;
            if let Some(x) = t.net.get_mut(i).and_then(|row| row.get_mut(j)).and_then(|c| c.get_mut(k)) {
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
        3 => {
            let i = rng.below(t.v.len() as u64) as usize;
            let d = rng.uniform(-1.0, 1.0) * 1e-5;
            if let Some(x) = t.v.get_mut(i) {
                *x += d;
            }
        }
        4 => t.p += 1,
        _ => return gen_srf(rng),
    }
    t
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let sur = gen_srf(rng);
    let [n, m, r, s] = in_use(&sur);
    match rng.below(5) {
        0 => {
            let eps = *rng.choose(&[1e-3, 1e-6, 0.0, 1e-2, 1.0]).unwrap_or(&1e-3);
            let pick = |k: usize, rng: &mut Rng| -> usize {
                if rng.chance(0.5) {
                    k
                } else {
                    rng.below(k as u64 + 3) as usize
                }
            };
            let sized = [pick(n, rng), pick(m, rng), pick(r, rng), pick(s, rng)];
            Inputs::Inspect {
                sur: SrfIn::Inline(sur),
                eps,
                sized,
            }
        }
        1 => {
            let ends = |k: usize, rng: &mut Rng| -> [usize; 2] {
                let a = rng.below(k as u64 + 2) as usize;
                let b = if rng.chance(0.8) {
                    a + rng.below((k + 1).saturating_sub(a) as u64 + 1) as usize
                } else {
                    rng.below(k as u64 + 3) as usize
                };
                [a, b]
            };
            let [ku, lu] = ends(n, rng);
            let [kv, lv] = ends(m, rng);
            Inputs::Enet {
                sur: SrfIn::Inline(sur),
                idx: [ku, lu, kv, lv],
            }
        }
        2 => {
            let big = storage(&sur);
            let kind = match rng.below(10) {
                0 => Kind::Compress,
                1 => Kind::SwapUv,
                2 => Kind::NonRatToRat,
                3 => Kind::MakeNonRat,
                4 => Kind::PruneRat,
                5 => {
                    // Upstream writes outside the knots when r < p (s < q).
                    let mut dir = 1 + rng.below(3) as i32;
                    if r < sur.p {
                        dir &= 2;
                    }
                    if s < sur.q {
                        dir &= 1;
                    }
                    let (ur, us) = (sur.u.get(r).copied(), sur.v.get(s).copied());
                    let rect = match rng.below(4) {
                        0 => [
                            sur.u.first().copied().unwrap_or(0.0),
                            ur.unwrap_or(1.0),
                            sur.v.first().copied().unwrap_or(0.0),
                            us.unwrap_or(1.0),
                        ],
                        1 => [0.0, 1.0, 0.0, 1.0],
                        _ => {
                            let a = rng.uniform(-10.0, 10.0);
                            let c = rng.uniform(-10.0, 10.0);
                            [a, a + rng.uniform(0.1, 20.0), c, c + rng.uniform(0.1, 20.0)]
                        }
                    };
                    if dir == 0 {
                        Kind::Compress
                    } else {
                        Kind::Reparam { rect, dir }
                    }
                }
                6 => Kind::SetSize {
                    idx: [
                        rng.below(big[0] as u64 + 1) as usize,
                        rng.below(big[1] as u64 + 1) as usize,
                        rng.below(6) as usize,
                        rng.below(6) as usize,
                        rng.below(big[2] as u64 + 1) as usize,
                        rng.below(big[3] as u64 + 1) as usize,
                    ],
                },
                7 => Kind::CopyNull,
                8 => Kind::CopyInto {
                    dst: SrfIn::Inline(gen_srf(rng)),
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
                        idx: [
                            pick(n, rng),
                            pick(m, rng),
                            rng.below(6) as usize,
                            rng.below(6) as usize,
                            pick(r, rng),
                            pick(s, rng),
                        ],
                    }
                }
            };
            Inputs::Modify {
                sur: SrfIn::Inline(sur),
                kind,
            }
        }
        3 => {
            let b = near_copy(rng, &sur);
            let tol3d = *rng.choose(&[0.0, 1e-6, 1e-4, 1e-3]).unwrap_or(&0.0);
            let tol1d = *rng.choose(&[0.0, 1e-6, 1e-4, 1e-3]).unwrap_or(&0.0);
            Inputs::Equal {
                a: SrfIn::Inline(sur),
                b: SrfIn::Inline(b),
                tol3d,
                tol1d,
            }
        }
        _ => {
            let k = rng.below(4) as usize;
            let mut surfaces = vec![SrfIn::Inline(sur)];
            for _ in 0..k {
                surfaces.push(SrfIn::Inline(gen_srf(rng)));
            }
            Inputs::RatCompatible { surfaces }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_basic"
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
