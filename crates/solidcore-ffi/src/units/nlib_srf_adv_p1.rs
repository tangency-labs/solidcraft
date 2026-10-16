//! FFI wrappers for unit `nlib.srf_adv_p1`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_adv_p1.cpp` (`sc_nlib_srf_adv_p1_eval`).
//! Upstream: `source/SMLib/src/NL_SrfAdv.cpp`.
//!
//! | [`Op`]          | upstream                           | input                                   |
//! |-----------------|------------------------------------|-----------------------------------------|
//! | `ExtendByDist`  | `N_SrfExtendByDist`                | surface, `dist dflg end cont inplace`   |
//! | `MaxDeriv`      | `N_SrfOffsetGetMaxDeriv`           | surface, `d`                            |
//! | `Unbounded`     | `N_SrfEvalPtDerivsUnbounded`       | surface, `cflg ndr cnt`, `cnt` `(u, v)` |
//! | `SkinSpine`     | `N_CreateSkinSpine`                | `k`, curves `0..=k`, spine, params `0..=k`, `cmp align z0x z0y z0z subdiv dflag deg dir` |
//! | `VectorField`   | `N_CrossBoundaryDerivsVectorField` | curve, vector field curve, `bndy`       |
//!
//! A surface is encoded `n m p q`, `(n+1)(m+1)` control points `x y z w` (row-major),
//! `n+p+2` u knots, `m+q+2` v knots; a curve `n p`, `n+1` control points, `n+p+2`
//! knots. [`eval`] accepts only well-formed clamped surfaces and curves (degrees
//! `1..=NL_MAXDEG`, at least degree + 1 control points, finite data, non-decreasing
//! knots with end multiplicity exactly `p+1` and interior multiplicity at most `p`)
//! and rejects every argument that would make upstream index outside its arrays or
//! loop without end:
//!
//! - `ExtendByDist`: `dflg` `1..=2`, `end` `1..=2`, `cont` one of `2, 3, 4, 6`.
//! - `Unbounded`: `cflg` one of `2, 3, 4, 6`; `ndr` `0..=`[`MAX_NDR`]; `1..=`[`MAX_EVALS`]
//!   evaluations; each parameter either within [`MAX_REACH`] domain lengths of the
//!   domain (bounding the periodic wrap loops) or beyond `1e12` in magnitude
//!   (upstream's immediate error exit).
//! - `SkinSpine`: `1 <= k <= MAX_CURVES - 1`; `1 <= deg <= NL_MAXDEG`; `dflag`
//!   `1..=3`; `dir` `1..=3`; `subdiv` `0..=`[`MAX_SUBDIV`]; spine parameters inside
//!   the spine's domain; without `cmp` the profiles must already share degree,
//!   control point count and knots (upstream copies `cur[0]`'s count from every
//!   profile).
//! - `VectorField`: `bndy` `1..=4`. Two valid curves with equal knot vectors have
//!   equal degrees and control point counts, so upstream never reads past the
//!   vector field's control points.

use crate::{call_with_buffer, FfiError};

/// Largest highest net index (`n`, `m`) of a surface accepted by [`eval`].
pub const MAX_INDEX: usize = 256;
/// Largest highest control point index of a curve accepted by [`eval`].
pub const MAX_CRV_INDEX: usize = 4096;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest number of profiles of `SkinSpine`.
pub const MAX_CURVES: usize = 32;
/// Largest derivative order of `Unbounded`.
pub const MAX_NDR: usize = 4;
/// Largest number of evaluations of one `Unbounded` call.
pub const MAX_EVALS: usize = 64;
/// Largest distance of an `Unbounded` parameter from the domain, in domain lengths.
pub const MAX_REACH: f64 = 16.0;
/// Largest subdivision level of `SkinSpine`.
pub const MAX_SUBDIV: usize = 16;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_SrfExtendByDist`.
    ExtendByDist,
    /// `N_SrfOffsetGetMaxDeriv`.
    MaxDeriv,
    /// `N_SrfEvalPtDerivsUnbounded` (a sequence of calls sharing `esur`).
    Unbounded,
    /// `N_CreateSkinSpine`.
    SkinSpine,
    /// `N_CrossBoundaryDerivsVectorField`.
    VectorField,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::ExtendByDist => 0,
            Op::MaxDeriv => 1,
            Op::Unbounded => 2,
            Op::SkinSpine => 3,
            Op::VectorField => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_adv_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input.
struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl<'a> Reader<'a> {
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

    fn slice(&mut self, k: usize) -> Result<&'a [f64], FfiError> {
        let end = self.at.checked_add(k).ok_or_else(|| bad("overflow"))?;
        let s = self
            .input
            .get(self.at..end)
            .ok_or_else(|| bad("input too short"))?;
        if s.iter().any(|x| !x.is_finite()) {
            return Err(bad("non-finite input"));
        }
        self.at = end;
        Ok(s)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
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

fn cont_flag(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let cont = rd.index(2, 6)?;
    if cont == 5 {
        return Err(bad("unsupported continuity flag"));
    }
    Ok(cont)
}

fn ends(k: &[f64]) -> Result<(f64, f64), FfiError> {
    match (k.first(), k.last()) {
        (Some(&a), Some(&b)) => Ok((a, b)),
        _ => Err(bad("empty knots")),
    }
}

fn check_reach(t: f64, k: &[f64]) -> Result<(), FfiError> {
    let (a, b) = ends(k)?;
    let len = b - a;
    if t.abs() > 1.0e12 || (t >= a - MAX_REACH * len && t <= b + MAX_REACH * len) {
        Ok(())
    } else {
        Err(bad(
            "unbounded evaluation parameter too far from the domain",
        ))
    }
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    match op {
        Op::ExtendByDist => {
            surface(&mut rd)?;
            rd.num()?;
            rd.index(1, 2)?;
            rd.index(1, 2)?;
            cont_flag(&mut rd)?;
            rd.index(0, 1)?;
        }
        Op::MaxDeriv => {
            surface(&mut rd)?;
            rd.num()?;
        }
        Op::Unbounded => {
            let (_, _, uk, vk) = surface(&mut rd)?;
            cont_flag(&mut rd)?;
            rd.index(0, MAX_NDR)?;
            let cnt = rd.index(1, MAX_EVALS)?;
            for _ in 0..cnt {
                let u = rd.num()?;
                let v = rd.num()?;
                check_reach(u, uk)?;
                check_reach(v, vk)?;
            }
        }
        Op::SkinSpine => {
            let k = rd.index(1, MAX_CURVES - 1)?;
            let mut shapes = Vec::with_capacity(k + 1);
            for _ in 0..=k {
                shapes.push(curve(&mut rd)?);
            }
            let (_, _, sk) = curve(&mut rd)?;
            let (a, b) = ends(sk)?;
            let params = rd.slice(k + 1)?;
            if params.iter().any(|&t| t < a || t > b) {
                return Err(bad("spine parameter outside the spine's domain"));
            }
            let cmp = rd.index(0, 1)?;
            rd.index(0, 1)?;
            rd.slice(3)?;
            rd.index(0, MAX_SUBDIV)?;
            rd.index(1, 3)?;
            rd.index(1, MAX_DEGREE)?;
            rd.index(1, 3)?;
            if cmp == 0 {
                let first = shapes.first().ok_or_else(|| bad("no curves"))?;
                if shapes.iter().any(|s| s != first) {
                    return Err(bad("incompatible profiles without cmp"));
                }
            }
        }
        Op::VectorField => {
            curve(&mut rd)?;
            curve(&mut rd)?;
            rd.index(1, 4)?;
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
        // knot vectors, flags and every parameter upstream uses for indexing or
        // looping); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_adv_p1_eval(
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
