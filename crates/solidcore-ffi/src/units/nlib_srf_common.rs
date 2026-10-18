//! FFI wrappers for unit `nlib.srf_common`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_common.cpp` (`sc_nlib_srf_common_eval`).
//! Upstream: `source/SMLib/src/NL_SrfCommon.cpp`.
//!
//! | [`Op`]        | upstream                 | input                                  |
//! |---------------|--------------------------|----------------------------------------|
//! | `CornerPts`   | `N_CreateSrfCornerPts`   | `P00 P10 P01 P11`                      |
//! | `ExtrudeCrv`  | `N_CreateSrfExtrudeCrv`  | curve, `W d dir`                       |
//! | `CylCone`     | `N_CreateCylCone`        | `C X Y rb rt as ae h ctp dir`          |
//! | `RuledSrf`    | `N_CreateRuledSrf`       | curve A, curve B, `dir`                |
//! | `RevolvedSrf` | `N_CreateRevolvedSrf`    | curve, `S T al ctp`                    |
//!
//! Points / vectors are three doubles. A curve is `n p m`, `n+1` control points
//! `x y z w`, `m+1` knots. [`eval`] accepts only finite values, well-formed clamped
//! curves (`m = n+p+1`, `1 <= p <= n`, consistent dimension and rationality, positive
//! weights, non-decreasing knots with end multiplicity exactly `p+1` and interior
//! multiplicity at most `p`), `dir` in `{1, 2, 3}` (`3` takes upstream's error
//! branch), `ctp` in `{2, 4, 5}` and angles of magnitude at most [`MAX_ANGLE`]
//! (upstream loops forever on huge angles). A quartic revolution of 241 degrees or
//! more is rejected: `N_CreateRevolvedSrf` writes its 13 circle weights into `wi[9]`.
//!
//! The output record is the routine's status (`0` ok, else the `NL_*_ERR` number),
//! every input curve after the call, then on success the surface (`n m p q r s`, the
//! net row by row as `x y z w`, the u knots, the v knots).

use crate::{call_with_buffer, FfiError};

/// Largest angle magnitude in degrees accepted by [`eval`].
pub const MAX_ANGLE: f64 = 1.0e6;
/// Largest control point index of an input curve.
pub const MAX_INDEX: usize = 4096;
/// Largest degree of an input curve (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CreateSrfCornerPts`.
    CornerPts,
    /// `N_CreateSrfExtrudeCrv`.
    ExtrudeCrv,
    /// `N_CreateCylCone`.
    CylCone,
    /// `N_CreateRuledSrf`.
    RuledSrf,
    /// `N_CreateRevolvedSrf`.
    RevolvedSrf,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::CornerPts => 0,
            Op::ExtrudeCrv => 1,
            Op::CylCone => 2,
            Op::RuledSrf => 3,
            Op::RevolvedSrf => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_common_eval(
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
    fn finite(&mut self) -> Result<f64, FfiError> {
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

    fn finites(&mut self, k: usize) -> Result<(), FfiError> {
        for _ in 0..k {
            self.finite()?;
        }
        Ok(())
    }

    fn angle(&mut self) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v.abs() > MAX_ANGLE {
            return Err(bad("angle magnitude above MAX_ANGLE"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.finite()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    fn dir(&mut self) -> Result<(), FfiError> {
        let v = self.finite()?;
        if v != 1.0 && v != 2.0 && v != 3.0 {
            return Err(bad(format!("bad direction flag {v}")));
        }
        Ok(())
    }

    fn ctp(&mut self) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v != 2.0 && v != 4.0 && v != 5.0 {
            return Err(bad(format!("bad conic type {v}")));
        }
        Ok(v)
    }
}

/// Validates one curve.
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
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
        if w != -f64::MAX && w <= 0.0 {
            return Err(bad("curve: non-positive weight"));
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

/// The sweep `N_CrvGetConicData(ctp, 0.0, &al, ...)` computes for `al`.
fn sweep(al: f64) -> f64 {
    let tol = 1.0e-6;
    let mut ae = al;
    while ae < tol {
        ae += 360.0;
    }
    while ae > 360.0 - tol {
        ae -= 360.0;
    }
    if ae < tol {
        ae = 360.0;
    }
    ae
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::CornerPts => rd.finites(12)?,
        Op::ExtrudeCrv => {
            curve(&mut rd)?;
            rd.finites(4)?;
            rd.dir()?;
        }
        Op::CylCone => {
            rd.finites(11)?;
            rd.angle()?;
            rd.angle()?;
            rd.finite()?;
            rd.ctp()?;
            rd.dir()?;
        }
        Op::RuledSrf => {
            curve(&mut rd)?;
            curve(&mut rd)?;
            rd.dir()?;
        }
        Op::RevolvedSrf => {
            curve(&mut rd)?;
            rd.finites(6)?;
            let al = rd.angle()?;
            let ctp = rd.ctp()?;
            if ctp == 4.0 && sweep(al) >= 241.0 {
                return Err(bad(
                    "quartic revolution of 241 degrees or more overflows wi[9]",
                ));
            }
        }
    }
    rd.done()
}

/// Runs `op` on `input` (layout in the module docs); returns the shim's output record
/// (status, input curves after the call, then the surface on success).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_common_eval(
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
