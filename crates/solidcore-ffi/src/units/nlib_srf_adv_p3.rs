//! FFI wrappers for unit `nlib.srf_adv_p3`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_adv_p3.cpp` (`sc_nlib_srf_adv_p3_eval`).
//! Upstream: `source/SMLib/src/NL_SrfAdv.cpp`.
//!
//! | [`Op`]          | upstream                  | input                                          |
//! |-----------------|---------------------------|------------------------------------------------|
//! | `Grid`          | `N_SrfGetGridPtsNormals`  | surface, `nu`, `u`, `nv`, `v`, `ufl vfl`       |
//! | `Extend`        | `N_SrfExtendByParamDist`  | surface, `par dflg end cont inplace`           |
//! | `Swung`         | `N_CreateSwungSrf`        | curve P, curve T, `alf`                        |
//! | `Skin`          | `N_CreateSkinSrf`         | `k`, curves `0..=k`, `cmp cld deg dir knotTol` |
//! | `TransSweep`    | `N_CreateTransSweepSrf`   | curve T, curve S                               |
//!
//! A surface is encoded `n m p q`, `(n+1)(m+1)` control points `x y z w` (row-major),
//! `n+p+2` u knots, `m+q+2` v knots; a curve `n p`, `n+1` control points, `n+p+2`
//! knots. [`eval`] accepts only well-formed clamped surfaces and curves (degrees
//! `1..=NL_MAXDEG`, at least degree + 1 control points, finite data, non-decreasing
//! knots with end multiplicity exactly `p+1` and interior multiplicity at most `p`)
//! and rejects every argument that would make upstream index outside its arrays:
//!
//! - `Grid`: at least one parameter in each direction; flags `1..=2`; inner
//!   parameters (not the first or last, which upstream range-checks) inside the knot
//!   range, and below its end for `NL_LEFT` (the span walk of `N_BasisDerivsArray`
//!   reads past the knots otherwise).
//! - `Extend`: `dflg` `1..=3`, `end` `1..=2`, `cont` one of `2, 3, 4, 6`; `par` strictly
//!   outside the domain on the side of `end`, at most [`MAX_EXTENSION`] knot ranges
//!   away (upstream splits long G1/G2 extensions by unbounded recursion).
//! - `Skin`: `1 <= k <= MAX_CURVES - 1`, `1 <= deg <= k`, `dir` `1..=3`,
//!   `knotTol >= 0`; without `cmp` the curves must already share degree, control point
//!   count and knots (upstream copies `cur[0]`'s count from every curve).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest highest net index (`n`, `m`) of a surface accepted by [`eval`].
pub const MAX_INDEX: usize = 256;
/// Largest highest control point index of a curve accepted by [`eval`].
pub const MAX_CRV_INDEX: usize = 4096;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest number of grid parameters per direction.
pub const MAX_PARAMS: usize = 256;
/// Largest number of cross sections of `Skin`.
pub const MAX_CURVES: usize = 32;
/// Largest extension of `Extend`, in knot ranges.
pub const MAX_EXTENSION: f64 = 4.0;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_SrfGetGridPtsNormals`.
    Grid,
    /// `N_SrfExtendByParamDist`.
    Extend,
    /// `N_CreateSwungSrf`.
    Swung,
    /// `N_CreateSkinSrf`.
    Skin,
    /// `N_CreateTransSweepSrf`.
    TransSweep,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Grid => 0,
            Op::Extend => 1,
            Op::Swung => 2,
            Op::Skin => 3,
            Op::TransSweep => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_adv_p3_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!();

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        if !v.is_finite() {
            return Err(bad("non-finite input"));
        }
        Ok(v)
    }

    fn index(&mut self, lo: usize, hi: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad(format!("bad index/count/flag {v}")));
        }
        Ok(v as usize)
    }
}

/// `N_KnotVectorIsValid` for degree `p` (end multiplicities exactly `p + 1`,
/// non-decreasing, interior multiplicities at most `p`).
fn valid_knots(u: &[f64], p: usize) -> bool {
    let (Some(&first), Some(&last)) = (u.first(), u.last()) else {
        return false;
    };
    if u.len() < 2 * p + 2 || first >= last || u.windows(2).any(|w| w.first() > w.get(1)) {
        return false;
    }
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for &x in u {
        match runs.last_mut() {
            Some((y, k)) if *y == x => *k += 1,
            _ => runs.push((x, 1)),
        }
    }
    runs.iter().all(|&(x, k)| {
        if x == first || x == last {
            k == p + 1
        } else {
            k <= p
        }
    })
}

/// A validated surface: `(p, q, u knots, v knots)`.
type SrfShape<'a> = (usize, usize, &'a [f64], &'a [f64]);

fn surface<'a>(rd: &mut Reader<'a>) -> Result<SrfShape<'a>, FfiError> {
    let n = rd.index(1, MAX_INDEX)?;
    let m = rd.index(1, MAX_INDEX)?;
    let p = rd.index(1, MAX_DEGREE)?;
    let q = rd.index(1, MAX_DEGREE)?;
    if p > n || q > m {
        return Err(bad("degree above the highest net index"));
    }
    rd.slice(4 * (n + 1) * (m + 1))?;
    let u = rd.slice(n + p + 2)?;
    let v = rd.slice(m + q + 2)?;
    if !valid_knots(u, p) || !valid_knots(v, q) {
        return Err(bad("invalid knot vector"));
    }
    Ok((p, q, u, v))
}

/// A validated curve: `(n, p, knots)`.
fn curve<'a>(rd: &mut Reader<'a>) -> Result<(usize, usize, &'a [f64]), FfiError> {
    let n = rd.index(1, MAX_CRV_INDEX)?;
    let p = rd.index(1, MAX_DEGREE)?;
    if p > n {
        return Err(bad("degree above the highest control point index"));
    }
    rd.slice(4 * (n + 1))?;
    let u = rd.slice(n + p + 2)?;
    if !valid_knots(u, p) {
        return Err(bad("invalid knot vector"));
    }
    Ok((n, p, u))
}

/// Grid parameters of one direction.
fn grid_params(rd: &mut Reader<'_>) -> Result<Vec<f64>, FfiError> {
    let cnt = rd.index(1, MAX_PARAMS)?;
    Ok(rd.slice(cnt)?.to_vec())
}

fn check_inner(t: &[f64], k: &[f64], left: bool) -> Result<(), FfiError> {
    let (Some(&a), Some(&b)) = (k.first(), k.last()) else {
        return Err(bad("empty knots"));
    };
    let inner = t.get(1..t.len().saturating_sub(1)).unwrap_or(&[]);
    if inner.iter().any(|&x| x < a || x > b || (left && x >= b)) {
        return Err(bad("inner grid parameter outside the domain"));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::Grid => {
            let (_, _, uk, vk) = surface(&mut rd)?;
            let u = grid_params(&mut rd)?;
            let v = grid_params(&mut rd)?;
            let ufl = rd.index(1, 2)?;
            let vfl = rd.index(1, 2)?;
            check_inner(&u, uk, ufl == 1)?;
            check_inner(&v, vk, vfl == 1)?;
        }
        Op::Extend => {
            let (_, _, uk, vk) = surface(&mut rd)?;
            let par = rd.num()?;
            let dflg = rd.index(1, 3)?;
            let end = rd.index(1, 2)?;
            let cont = rd.index(2, 6)?;
            if cont == 5 {
                return Err(bad("unsupported continuity flag"));
            }
            rd.index(0, 1)?;
            let k = if dflg == 1 { uk } else { vk };
            let (Some(&a), Some(&b)) = (k.first(), k.last()) else {
                return Err(bad("empty knots"));
            };
            let ext = if end == 1 { a - par } else { par - b };
            if !(ext > 0.0 && ext <= MAX_EXTENSION * (b - a)) {
                return Err(bad("extension parameter on the wrong side or too far"));
            }
        }
        Op::Swung | Op::TransSweep => {
            curve(&mut rd)?;
            curve(&mut rd)?;
            if op == Op::Swung {
                rd.num()?;
            }
        }
        Op::Skin => {
            let k = rd.index(1, MAX_CURVES - 1)?;
            let mut shapes = Vec::with_capacity(k + 1);
            for _ in 0..=k {
                shapes.push(curve(&mut rd)?);
            }
            let cmp = rd.index(0, 1)?;
            rd.index(0, 1)?;
            rd.index(1, k.min(MAX_DEGREE))?;
            rd.index(1, 3)?;
            let tol = rd.num()?;
            if tol < 0.0 {
                return Err(bad("negative knot tolerance"));
            }
            if cmp == 0 {
                let first = shapes.first().ok_or_else(|| bad("no curves"))?;
                if shapes.iter().any(|s| s != first) {
                    return Err(bad("incompatible cross sections without cmp"));
                }
            }
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including surface and curve shapes,
        // knot vectors, flags and every parameter upstream uses for indexing); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_adv_p3_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
