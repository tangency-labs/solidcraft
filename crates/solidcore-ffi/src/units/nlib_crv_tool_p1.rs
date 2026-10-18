//! FFI wrappers for unit `nlib.crv_tool_p1`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_tool_p1.cpp` (`sc_nlib_crv_tool_p1_eval`).
//! Upstream: `source/SMLib/src/NL_CrvTool.cpp`.
//!
//! | [`Op`]        | upstream                 | parameters after the curve |
//! |---------------|--------------------------|----------------------------|
//! | `InsertKnot`  | `N_CrvInsertKnot`        | `u r inplace`              |
//! | `Split`       | `N_CrvSplit`             | `u`                        |
//! | `DecomposeBez`| `N_CrvDecomposeBez`      | (none)                     |
//! | `Refine`      | `N_CrvRefine`            | `nx x[0..nx) inplace`      |
//! | `ExtractSeg`  | `N_CrvExtractCrvSeg`     | `ul ur inplace`            |
//! | `RemoveKnot`  | `N_CrvRemoveKnot`        | `u nu tol inplace`         |
//! | `MaxErr`      | `N_CrvRemoveKnotMaxErr`  | `r s`                      |
//! | `Elevate`     | `N_CrvElevateDegree`     | `t inplace`                |
//!
//! A curve is encoded `n p m`, `n+1` control points `x y z w`, `m+1` knots. [`eval`]
//! accepts only well-formed clamped curves (`m = n+p+1`, `1 <= p <= n`, finite data,
//! consistent dimension and rationality, non-decreasing knots with end multiplicity
//! exactly `p+1` and interior multiplicity at most `p`) and parameters for which
//! upstream stays inside its arrays (see [`eval`]); everything else is rejected before
//! the call.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Largest count argument (`r`, `nu`, `t`) accepted by [`eval`].
pub const MAX_COUNT: usize = 64;
/// Largest number of refinement knots accepted by [`eval`].
pub const MAX_REFINE: usize = 1024;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CrvInsertKnot`.
    InsertKnot,
    /// `N_CrvSplit`.
    Split,
    /// `N_CrvDecomposeBez`.
    DecomposeBez,
    /// `N_CrvRefine`.
    Refine,
    /// `N_CrvExtractCrvSeg`.
    ExtractSeg,
    /// `N_CrvRemoveKnot`.
    RemoveKnot,
    /// `N_CrvRemoveKnotMaxErr`.
    MaxErr,
    /// `N_CrvElevateDegree`.
    Elevate,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::InsertKnot => 0,
            Op::Split => 1,
            Op::DecomposeBez => 2,
            Op::Refine => 3,
            Op::ExtractSeg => 4,
            Op::RemoveKnot => 5,
            Op::MaxErr => 6,
            Op::Elevate => 7,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_tool_p1_eval(
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
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() {
            return Err(bad("non-finite parameter"));
        }
        Ok(v)
    }

    fn flag(&mut self) -> Result<(), FfiError> {
        self.index(1).map(|_| ())
    }
}

/// A validated curve: degree, highest knot index and knots.
struct Crv<'a> {
    p: usize,
    m: usize,
    knots: &'a [f64],
}

fn curve<'a>(rd: &mut Reader<'a>) -> Result<Crv<'a>, FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (pts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *pts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in pts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
    }
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("curve: knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let short = || bad("curve: knots too short");
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad("curve: knots not clamped"));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad("curve: end multiplicity above p + 1"));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad("curve: interior multiplicity above the degree"));
        }
    }
    Ok(Crv { p, m, knots })
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let c = curve(&mut rd)?;
    let (Some(&u0), Some(&um)) = (c.knots.first(), c.knots.last()) else {
        return Err(bad("empty knots"));
    };
    match op {
        Op::InsertKnot => {
            rd.finite()?;
            rd.index(MAX_COUNT)?;
            rd.flag()?;
        }
        Op::Split => {
            rd.finite()?;
        }
        Op::DecomposeBez => {}
        Op::Refine => {
            let nx = rd.index(MAX_REFINE)?;
            if nx == 0 {
                return Err(bad("refine: needs at least one knot"));
            }
            let x = rd.take(nx)?;
            if x.iter().any(|v| !v.is_finite()) || x.windows(2).any(|w| w.first() > w.last()) {
                return Err(bad("refine: knots not non-decreasing"));
            }
            rd.flag()?;
        }
        Op::ExtractSeg => {
            let ul = rd.finite()?;
            let ur = rd.finite()?;
            if ul < u0 || ul > um || ur < u0 || ur > um {
                return Err(bad("extract: parameters outside the knot range"));
            }
            rd.flag()?;
        }
        Op::RemoveKnot => {
            rd.finite()?;
            rd.index(MAX_COUNT)?;
            rd.num()?;
            rd.flag()?;
        }
        Op::MaxErr => {
            let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
            let s = rd.index(MAX_DEGREE)?;
            let (p, m) = (c.p, c.m);
            if r < p + 1 || r + p + 1 > m || s == 0 || s > p {
                return Err(bad("max err: need p+1 <= r <= m-p-1 and 1 <= s <= p"));
            }
            if c.knots.get(r) == c.knots.get(r + 1) {
                return Err(bad("max err: U[r] == U[r+1]"));
            }
        }
        Op::Elevate => {
            rd.index(MAX_COUNT)?;
            rd.flag()?;
        }
    }
    rd.done()
}

/// Runs `op` on `input` (curve, then the op's parameters); returns the shim's output
/// record (status, then outputs on success).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including every index the shim passes
        // to upstream); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_tool_p1_eval(
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
