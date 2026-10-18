//! FFI wrappers for unit `nlib.srf_tool_p2`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_tool_p2.cpp` (`sc_nlib_srf_tool_p2_eval`).
//! Upstream: `source/SMLib/src/NL_SrfTool.cpp`.
//!
//! | [`Op`]               | upstream                     |
//! |----------------------|------------------------------|
//! | `RemoveConditional`  | `N_SrfRemoveKnotConditional` |
//! | `RemoveOneKnot`      | `N_SrfRemoveOneKnot`         |
//! | `ElevateDegree`      | `N_SrfElevateDegree`         |
//!
//! Input encoding (flat doubles) and output records are described in the shim.
//! [`eval`] parses the whole input and rejects anything that could make upstream
//! index outside its arrays: the surface must have degrees `1..=NL_MAXDEG`,
//! `r = n + p + 1`, `s = m + q + 1` and valid clamped knot vectors
//! (`N_KnotVectorIsValid`: end multiplicities `p + 1`, non-decreasing, interior
//! multiplicities at most `p`); every coordinate and parameter must be finite;
//! direction flags must be `1..=3`. `N_SrfRemoveOneKnot` checks nothing, so its knot
//! index `r` and multiplicity `s` must satisfy `deg < r`, `1 <= s <= deg`,
//! `r - s < n_dir` (`deg`, `n_dir`: degree and highest net index in the direction,
//! u for the `NL_UVDIR` flag) and `f, l` must be net indices in the other direction.

use crate::{call_with_buffer, FfiError};

/// Largest highest index (`n`, `m`) accepted by [`eval`].
pub const MAX_INDEX: usize = 512;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest removal count / elevation increment accepted by [`eval`].
pub const MAX_COUNT: usize = 64;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_SrfRemoveKnotConditional`; then `t nt tol dir inplace`.
    RemoveConditional,
    /// `N_SrfRemoveOneKnot`; then `r s f l dir mr`.
    RemoveOneKnot,
    /// `N_SrfElevateDegree`; then `t dir inplace`.
    ElevateDegree,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::RemoveConditional => 0,
            Op::RemoveOneKnot => 1,
            Op::ElevateDegree => 2,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_tool_p2_eval(
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
            return Err(bad(format!("bad index/count {v}")));
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
    // Runs of equal knots: the ends have exactly p + 1, interior ones at most p.
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

/// Surface `(n, m, p, q)` after validation.
fn surface(rd: &mut Reader<'_>) -> Result<[usize; 4], FfiError> {
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
    Ok([n, m, p, q])
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let [n, m, p, q] = surface(&mut rd)?;
    match op {
        Op::RemoveConditional => {
            rd.num()?;
            rd.index(0, MAX_COUNT)?;
            rd.num()?;
            rd.index(1, 3)?;
            rd.index(0, 1)?;
        }
        Op::RemoveOneKnot => {
            let r = rd.index(0, 2 * MAX_INDEX)?;
            let s = rd.index(1, MAX_DEGREE)?;
            let f = rd.index(0, MAX_INDEX)?;
            let l = rd.index(0, MAX_INDEX)?;
            let dir = rd.index(1, 3)?;
            rd.num()?;
            let (deg, hi, other) = if dir == 2 { (q, m, n) } else { (p, n, m) };
            if r <= deg || s > deg || r - s >= hi {
                return Err(bad("knot index / multiplicity outside the arrays"));
            }
            if f > other || l > other {
                return Err(bad("row range outside the net"));
            }
        }
        Op::ElevateDegree => {
            rd.index(0, MAX_COUNT)?;
            rd.index(1, 3)?;
            rd.index(0, 1)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including the surface shape, its knot
        // vectors and every parameter upstream uses for indexing); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_tool_p2_eval(
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
