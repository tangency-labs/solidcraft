//! FFI wrappers for unit `nlib.srf_tool_p3`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_tool_p3.cpp` (`sc_nlib_srf_tool_p3_eval`).
//! Upstream: `source/SMLib/src/NL_SrfTool.cpp`.
//!
//! | [`Op`]            | upstream               |
//! |-------------------|------------------------|
//! | `RemoveAllKnots`  | `N_SrfRemoveAllKnots`  |
//! | `Join`            | `N_SrfJoin`            |
//! | `MakeCompatible`  | `N_MakeSrfsCompatible` |
//!
//! Input encoding (flat doubles) and output records are described in the shim.
//! [`eval`] parses the whole input and rejects anything that could make upstream
//! index outside its arrays: every surface must have degrees `1..=NL_MAXDEG`,
//! `r = n + p + 1`, `s = m + q + 1` and valid clamped knot vectors
//! (`N_KnotVectorIsValid`: end multiplicities `p + 1`, non-decreasing, interior
//! multiplicities at most `p`); every coordinate and parameter must be finite;
//! direction flags must be `1..=3`. `N_MakeSrfsCompatible` takes `1..=MAX_SURFACES`
//! surfaces.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest highest index (`n`, `m`) accepted by [`eval`].
pub const MAX_INDEX: usize = 512;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest number of surfaces of [`Op::MakeCompatible`].
pub const MAX_SURFACES: usize = 16;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_SrfRemoveAllKnots`: surface, then `tol dir inplace`.
    RemoveAllKnots,
    /// `N_SrfJoin`: two surfaces, then `dir tol mode` (`0` new output, `1` into the
    /// first surface, `2` into the second).
    Join,
    /// `N_MakeSrfsCompatible`: count, that many surfaces, then `dir`.
    MakeCompatible,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::RemoveAllKnots => 0,
            Op::Join => 1,
            Op::MakeCompatible => 2,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_tool_p3_eval(
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
    match op {
        Op::RemoveAllKnots => {
            surface(&mut rd)?;
            rd.num()?;
            rd.index(1, 3)?;
            rd.index(0, 1)?;
        }
        Op::Join => {
            surface(&mut rd)?;
            surface(&mut rd)?;
            rd.index(1, 3)?;
            rd.num()?;
            rd.index(0, 2)?;
        }
        Op::MakeCompatible => {
            let cnt = rd.index(1, MAX_SURFACES)?;
            for _ in 0..cnt {
                surface(&mut rd)?;
            }
            rd.index(1, 3)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(16384, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including every surface's shape and
        // knot vectors and every parameter upstream uses for indexing); buf is valid
        // for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_tool_p3_eval(
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
