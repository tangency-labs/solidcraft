//! Safe wrapper for unit `nlib.crv_fit_p4`: `N_FitCrvApproxDerivs`,
//! `SmBSplineCurve::CreateApproximatingCurve` and `SmApiCreateCurveApproxPoints` (shim
//! `harness/cpp/shim/nlib_crv_fit_p4.cpp`, which documents the layouts).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_fit_p4_eval(
            op: i32,
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Largest point, control point and vector count accepted.
pub const MAX_COUNT: usize = 4096;
/// Largest degree accepted where upstream evaluates basis functions (`NL_MAXDEG`).
pub const MAX_BASIS_DEGREE: usize = 17;
/// Largest end derivative order (upstream's `Ds[5]` / `De[5]` arrays).
pub const MAX_END_DERIV: usize = 4;

fn bad(what: &str) -> FfiError {
    FfiError::InvalidArgument(format!("crv_fit_p4: {what}"))
}

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let x = *self.v.get(self.at).ok_or_else(|| bad("short block"))?;
        self.at += 1;
        if !x.is_finite() || x.abs() > 1e15 {
            return Err(bad("non-finite or huge value"));
        }
        Ok(x)
    }
    fn int(&mut self, lo: usize, hi: usize) -> Result<usize, FfiError> {
        let x = self.num()?;
        if x.fract() != 0.0 || x < lo as f64 || x > hi as f64 {
            return Err(bad("bad count or index"));
        }
        Ok(x as usize)
    }
    fn xyz(&mut self, count: usize) -> Result<(), FfiError> {
        for _ in 0..count * 3 {
            self.num()?;
        }
        Ok(())
    }
}

fn check(op: i32, block: &[f64]) -> Result<(), FfiError> {
    let mut rd = Rd { v: block, at: 0 };
    match op {
        0 => {
            rd.int(0, MAX_COUNT)?; // n
            rd.int(0, MAX_BASIS_DEGREE)?; // p
            rd.int(1, 3)?; // par
            let k = rd.int(0, MAX_END_DERIV)?;
            let l = rd.int(0, MAX_END_DERIV)?;
            let c = rd.int(0, MAX_COUNT)?;
            rd.xyz(c + k + l)?;
        }
        1 => {
            rd.int(0, MAX_COUNT)?; // control points
            rd.int(0, 5)?; // parameterization
            rd.int(2, 3)?; // dimension
            rd.int(0, MAX_BASIS_DEGREE)?; // degree
            let c = rd.int(0, MAX_COUNT)?;
            rd.xyz(c)?;
            let nvec = rd.int(0, 3)?;
            rd.xyz(nvec)?;
            rd.int(0, 1)?; // has more vectors
            let nmore = rd.int(0, MAX_END_DERIV)?;
            rd.xyz(nmore)?;
        }
        2 => {
            let c = rd.int(0, MAX_COUNT)?;
            rd.xyz(c)?;
        }
        _ => return Err(bad("unknown op")),
    }
    if rd.at != block.len() {
        return Err(bad("trailing values"));
    }
    Ok(())
}

/// Runs operation `op` on `block` and returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block the shim cannot take safely (counts above
/// [`MAX_COUNT`], degrees above [`MAX_BASIS_DEGREE`], end derivative orders above
/// [`MAX_END_DERIV`], non-finite values); the shim's status otherwise.
pub fn eval(op: i32, block: &[f64]) -> Result<Vec<f64>, FfiError> {
    check(op, block)?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: the validated block outlives the call; output and length pointers come
        // from call_with_buffer.
        unsafe {
            sys::sc_nlib_crv_fit_p4_eval(
                op,
                block.as_ptr(),
                block.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
