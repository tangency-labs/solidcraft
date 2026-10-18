//! FFI wrappers for unit `nlib.srf_offset`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_offset.cpp` (`sc_nlib_srf_offset_eval`).
//! Upstream: `source/SMLib/src/NL_SrfOffset.cpp` (`N_SrfOffset`).
//!
//! The input is a surface encoded `n m p q`, `(n+1)(m+1)` control points `x y z w`
//! (row-major), `n+p+2` u knots, `m+q+2` v knots, then `d pp qq par eps tol`. [`eval`]
//! accepts only well-formed clamped surfaces (degrees `1..=NL_MAXDEG`, at least
//! degree + 1 control points, finite data, positive weights or `NL_NOW`,
//! non-decreasing knots with end multiplicity exactly `p+1` and interior multiplicity
//! at most `p`), output degrees `1..=NL_MAXDEG`, `par` `1..=4`, `eps > 0` and
//! `tol` in [`MIN_TOL`]`..=1` (upstream casts `ceil(sqrt(area / tol))` to `NL_INDEX`
//! unchecked), with coordinates and `|d|` at most [`MAX_COORD`].

use crate::validate::{bad, NOW};
use crate::{call_with_buffer, FfiError};

/// Largest highest net index (`n`, `m`) of a surface accepted by [`eval`].
pub const MAX_INDEX: usize = 64;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Smallest approximation tolerance accepted by [`eval`].
pub const MIN_TOL: f64 = 1.0e-10;
/// Largest coordinate magnitude and offset distance accepted by [`eval`].
pub const MAX_COORD: f64 = 1.0e4;
mod sys {
    extern "C" {
        pub fn sc_nlib_srf_offset_eval(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

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

fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(1, MAX_INDEX)?;
    let m = rd.index(1, MAX_INDEX)?;
    let p = rd.index(1, MAX_DEGREE)?;
    let q = rd.index(1, MAX_DEGREE)?;
    if p > n || q > m {
        return Err(bad("degree above the highest net index"));
    }
    let net = rd.slice(4 * (n + 1) * (m + 1))?;
    let (pts, _) = net.as_chunks::<4>();
    for &[x, y, z, w] in pts {
        if [x, y, z]
            .iter()
            .any(|v| v.abs() > MAX_COORD * w.abs().max(1.0))
        {
            return Err(bad("coordinate too large"));
        }
        if !(w == NOW || w > 0.0) {
            return Err(bad("non-positive weight"));
        }
    }
    let u = rd.slice(n + p + 2)?;
    let v = rd.slice(m + q + 2)?;
    if !valid_knots(u, p) || !valid_knots(v, q) {
        return Err(bad("invalid knot vector"));
    }
    Ok(())
}

fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    surface(&mut rd)?;
    let d = rd.num()?;
    if d.abs() > MAX_COORD {
        return Err(bad("offset distance too large"));
    }
    rd.index(1, MAX_DEGREE)?;
    rd.index(1, MAX_DEGREE)?;
    rd.index(1, 4)?;
    let eps = rd.num()?;
    if eps <= 0.0 {
        return Err(bad("non-positive eps"));
    }
    let tol = rd.num()?;
    if !(MIN_TOL..=1.0).contains(&tol) {
        return Err(bad("tolerance out of range"));
    }
    rd.done()
}

/// Runs `N_SrfOffset` on `input`; returns the shim's output record (status, then the
/// offset surface on success).
pub fn eval(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(1 << 16, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: surface shape, knot vectors, weights, flags and
        // tolerances); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_offset_eval(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
