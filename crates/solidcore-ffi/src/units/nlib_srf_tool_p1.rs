//! FFI wrappers for unit `nlib.srf_tool_p1`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_tool_p1.cpp` (`sc_nlib_srf_tool_p1_eval`).
//! Upstream: `source/SMLib/src/NL_SrfTool.cpp`.
//!
//! | [`Op`]          | upstream               |
//! |-----------------|------------------------|
//! | `InsertKnot`    | `N_SrfInsertKnot`      |
//! | `Split`         | `N_SrfSplit`           |
//! | `Decompose`     | `N_SrfDecomposeToBez`  |
//! | `IsoCurve`      | `N_SrfExtractIsoCrv`   |
//! | `InsertKnots`   | `N_SrfInsertKnots`     |
//! | `ExtractPatch`  | `N_SrfExtractPatch`    |
//!
//! Input encoding (flat doubles) and output records are described in the shim.
//! [`eval`] parses the whole input and rejects anything that could make upstream
//! index outside its arrays: the surface must have degrees `1..=NL_MAXDEG`,
//! `r = n + p + 1`, `s = m + q + 1` and valid clamped knot vectors
//! (`N_KnotVectorIsValid`: end multiplicities `p + 1`, non-decreasing, interior
//! multiplicities at most `p`); every coordinate and parameter must be finite;
//! refinement knots must not decrease; direction flags must be `1..=3`.

use crate::{call_with_buffer, FfiError};

/// Largest highest index (`n`, `m`) accepted by [`eval`].
pub const MAX_INDEX: usize = 512;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest insertion count / refinement knot count accepted by [`eval`].
pub const MAX_INSERT: usize = 256;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_SrfInsertKnot`; then `t mt dir inplace`.
    InsertKnot,
    /// `N_SrfSplit`; then `t dir`.
    Split,
    /// `N_SrfDecomposeToBez`.
    Decompose,
    /// `N_SrfExtractIsoCrv`; then `t dir`.
    IsoCurve,
    /// `N_SrfInsertKnots`; then `dir inplace k X[0..k)`.
    InsertKnots,
    /// `N_SrfExtractPatch`; then `ul ur vl vr inplace`.
    ExtractPatch,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::InsertKnot => 0,
            Op::Split => 1,
            Op::Decompose => 2,
            Op::IsoCurve => 3,
            Op::InsertKnots => 4,
            Op::ExtractPatch => 5,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_tool_p1_eval(
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
            return Err(bad(format!("bad index/count {v}")));
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

/// Surface `(p, q, u knots, v knots)` after validation.
fn surface<'a>(rd: &mut Reader<'a>) -> Result<(&'a [f64], &'a [f64]), FfiError> {
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
    Ok((u, v))
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    surface(&mut rd)?;
    match op {
        Op::InsertKnot => {
            rd.num()?;
            rd.index(0, MAX_INSERT)?;
            rd.index(1, 3)?;
            rd.index(0, 1)?;
        }
        Op::Split | Op::IsoCurve => {
            rd.num()?;
            rd.index(1, 3)?;
        }
        Op::Decompose => {}
        Op::InsertKnots => {
            rd.index(1, 3)?;
            rd.index(0, 1)?;
            let k = rd.index(0, MAX_INSERT)?;
            let x = rd.slice(k)?;
            if x.windows(2).any(|w| w.first() > w.get(1)) {
                return Err(bad("refinement knots decrease"));
            }
        }
        Op::ExtractPatch => {
            for _ in 0..4 {
                rd.num()?;
            }
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
            sys::sc_nlib_srf_tool_p1_eval(
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

#[cfg(test)]
mod tests {
    use super::valid_knots;

    #[test]
    fn knot_validation() {
        assert!(valid_knots(&[0.0, 0.0, 1.0, 1.0], 1));
        assert!(valid_knots(&[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], 2));
        assert!(!valid_knots(
            &[0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
            2
        ));
        assert!(!valid_knots(&[0.0, 0.0, 1.0, 1.0, 1.0], 1));
        assert!(!valid_knots(&[0.0, 0.0, 0.0, 0.0], 1));
        assert!(!valid_knots(&[0.0, 0.5, 0.2, 1.0], 1));
    }
}
