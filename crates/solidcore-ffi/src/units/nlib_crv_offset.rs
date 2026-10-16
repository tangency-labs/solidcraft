//! FFI wrappers for unit `nlib.crv_offset`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_offset.cpp` (`sc_nlib_crv_offset_eval`).
//! Upstream: `source/SMLib/src/NL_CrvOffset.cpp` (`N_CrvOffsetPtSampling`).
//!
//! Input: a curve (`n p m`, `n+1` control points `x y z w`, `m+1` knots), then
//! `B[3] d deg tan par tol`. Output record: status (`0` ok, else the `NL_*_ERR`
//! number, `1000` for a bare error flag), then on success the offset curve (`n p m`,
//! control points, knots).
//!
//! [`offset_pt_sampling`] accepts only well-formed clamped curves (`m = n+p+1`,
//! `1 <= p <= n`, `p <= NL_MAXDEG`, finite data, consistent dimension and rationality,
//! positive weights, non-decreasing knots with end multiplicity exactly `p+1` and
//! interior multiplicity at most `p`), finite `B` and `d` (`|d| <= MAX_DIST`),
//! `1 <= deg <= MAX_DEGREE`, `tan` `0`/`1`, `par` `1..=4` and `0 <= tol <= 1` (a
//! negative tolerance makes upstream cast NaN to an index). The shim also refuses
//! (status `SC_ERR_ARG`) inputs whose sample count is not finite or exceeds
//! [`MAX_SAMPLES`], which it recomputes exactly as upstream before the call.

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted.
pub const MAX_INDEX: usize = 512;
/// Largest degree of the input curve and of the approximation (`NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Largest absolute offset distance.
pub const MAX_DIST: f64 = 1e6;
/// Largest total number of sample points (the shim refuses more).
pub const MAX_SAMPLES: usize = 4000;

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_offset_eval(
            input: *const f64,
            n_in: i64,
            max_samples: f64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

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
        Ok(v)
    }

    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() {
            return Err(bad("non-finite parameter"));
        }
        Ok(v)
    }

    fn index(&mut self, min: usize, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < min as f64 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    fn take(&mut self, k: usize) -> Result<&'a [f64], FfiError> {
        let end = self.at.checked_add(k).ok_or_else(|| bad("overflow"))?;
        let s = self
            .input
            .get(self.at..end)
            .ok_or_else(|| bad("input too short"))?;
        self.at = end;
        Ok(s)
    }
}

/// Reads and validates a curve.
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(0, MAX_INDEX)?;
    let p = rd.index(0, MAX_DEGREE)?;
    let m = rd.index(0, MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in cpts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
        if w != -f64::MAX && w <= 0.0 {
            return Err(bad("curve: non-positive weight"));
        }
        let zz = if z == f64::MAX { 0.0 } else { z };
        let ww = if w == -f64::MAX { 1.0 } else { w };
        if [x / ww, y / ww, zz / ww]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > 1e100)
        {
            return Err(bad("curve: Euclidean point out of range"));
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
    Ok(())
}

fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    curve(&mut rd)?;
    for _ in 0..3 {
        rd.finite()?;
    }
    let d = rd.finite()?;
    if d.abs() > MAX_DIST {
        return Err(bad("offset distance out of range"));
    }
    rd.index(1, MAX_DEGREE)?;
    rd.index(0, 1)?;
    rd.index(1, 4)?;
    let tol = rd.finite()?;
    if !(0.0..=1.0).contains(&tol) {
        return Err(bad("tolerance out of range"));
    }
    if rd.at != input.len() {
        return Err(bad("trailing input"));
    }
    Ok(())
}

/// Runs upstream `N_CrvOffsetPtSampling` on `input` (curve, then
/// `B[3] d deg tan par tol`); returns the shim's record (status, then the curve).
pub fn offset_pt_sampling(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly the validated
        // curve and parameters the shim reads; buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_offset_eval(
                input.as_ptr(),
                input.len() as i64,
                MAX_SAMPLES as f64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
