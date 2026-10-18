//! FFI wrappers for unit `nlib.srf_tool_p4`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_tool_p4.cpp` (`sc_nlib_srf_tool_p4_eval`).
//! Upstream: `source/SMLib/src/NL_SrfTool.cpp`.
//!
//! | [`Op`]                     | upstream                      |
//! |----------------------------|-------------------------------|
//! | `RemoveKnotsConstraints`   | `N_SrfRemoveKnotsConstraints` |
//! | `MakeCompatibleUv`         | `N_MakeSrfsCompatibleUV`      |
//!
//! Input encoding (flat doubles) and output records are described in the shim.
//! [`eval`] parses the whole input and rejects anything that could make upstream
//! index outside its arrays: every surface must have degrees `1..=NL_MAXDEG`,
//! `r = n + p + 1`, `s = m + q + 1` and valid clamped knot vectors
//! (`N_KnotVectorIsValid`: end multiplicities `p + 1`, non-decreasing, interior
//! multiplicities at most `p`); every coordinate and parameter must be finite;
//! direction flags must be `1..=3`; a non-empty knot constraint list (at most
//! [`MAX_CONSTRAINTS`] entries) must hold an entry other than the surface's first
//! knot in that direction (upstream skips leading copies of it without a bound).
//! `N_MakeSrfsCompatibleUV` takes `1..=MAX_SURFACES` surfaces.

use crate::{call_with_buffer, FfiError};

/// Largest highest index (`n`, `m`) accepted by [`eval`].
pub const MAX_INDEX: usize = 512;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest number of surfaces of [`Op::MakeCompatibleUv`].
pub const MAX_SURFACES: usize = 16;
/// Largest length of a knot constraint list of [`Op::RemoveKnotsConstraints`].
pub const MAX_CONSTRAINTS: usize = 1024;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_SrfRemoveKnotsConstraints`: surface, `cu` and `cu` u knots, `cv` and `cv` v
    /// knots (a count of `0` passes `NULL`), then `tol dir inplace`.
    RemoveKnotsConstraints,
    /// `N_MakeSrfsCompatibleUV`: count, then that many surfaces.
    MakeCompatibleUv,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::RemoveKnotsConstraints => 0,
            Op::MakeCompatibleUv => 1,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_tool_p4_eval(
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

/// Validates a surface; returns its first u and v knots.
fn surface(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
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
    let (Some(&u0), Some(&v0)) = (u.first(), v.first()) else {
        return Err(bad("empty knot vector"));
    };
    Ok((u0, v0))
}

/// A knot constraint list: count, then the knots; `first` is the surface's first
/// knot in that direction.
fn constraints(rd: &mut Reader<'_>, first: f64) -> Result<(), FfiError> {
    let c = rd.index(0, MAX_CONSTRAINTS)?;
    let k = rd.slice(c)?;
    if c > 0 && k.iter().all(|&x| x == first) {
        return Err(bad("constraint list holds only the first knot"));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::RemoveKnotsConstraints => {
            let (u0, v0) = surface(&mut rd)?;
            constraints(&mut rd, u0)?;
            constraints(&mut rd, v0)?;
            rd.num()?;
            rd.index(1, 3)?;
            rd.index(0, 1)?;
        }
        Op::MakeCompatibleUv => {
            let cnt = rd.index(1, MAX_SURFACES)?;
            for _ in 0..cnt {
                surface(&mut rd)?;
            }
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
            sys::sc_nlib_srf_tool_p4_eval(
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
