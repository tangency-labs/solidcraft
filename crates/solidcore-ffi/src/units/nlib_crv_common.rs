//! FFI wrappers for unit `nlib.crv_common`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_common.cpp` (`sc_nlib_crv_common_eval`).
//! Upstream: `source/SMLib/src/NL_CrvCommon.cpp`.
//!
//! | [`Op`]              | upstream                          | input                              |
//! |---------------------|-----------------------------------|------------------------------------|
//! | `QuadraticArcTol`   | `N_CreateQuadraticArcTol`         | `C X Y r as ae at`                 |
//! | `QuarticArcTol`     | `N_CreateQuarticArcTol`           | `C X Y r as ae at`                 |
//! | `QuinticArcTol`     | `N_CreateQuinticArcTol`           | `C X Y r as ae at`                 |
//! | `CircArc`           | `N_CreateCircArc`                 | `C X Y r as ae ctp`                |
//! | `CircArcTol`        | `N_CreateCircArcTol`              | `C X Y r as ae at ctp`             |
//! | `CircWeights`       | `N_CalcCircWeights`               | `as ae ctp`                        |
//! | `EllipticalArcTol`  | `N_CreateEllipticalArcTol`        | `C X Y r1 r2 as ae at ctp`         |
//! | `EllipticalArcAngTypeTol` | `N_CreateEllipticalArcAngTypeTol` | `C X Y r1 r2 as ae at atp ctp` |
//!
//! `C`, `X`, `Y` are three doubles each. [`eval`] accepts only finite values, angles of
//! magnitude at most [`MAX_ANGLE`] (upstream loops forever on non-finite or huge
//! angles), `ctp` in `{2, 4, 5}`, `atp` in `{0, 1}` and non-zero ellipse radii.
//!
//! The output record is the routine's status (`0` ok, else the `NL_*_ERR` number),
//! then on success a curve (`n p m`, `n+1` control points `x y z w`, `m+1` knots) or,
//! for `CircWeights`, `k+1` and the weights `w[0..=k]`.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest angle magnitude in degrees accepted by [`eval`].
pub const MAX_ANGLE: f64 = 1.0e6;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CreateQuadraticArcTol`.
    QuadraticArcTol,
    /// `N_CreateQuarticArcTol`.
    QuarticArcTol,
    /// `N_CreateQuinticArcTol`.
    QuinticArcTol,
    /// `N_CreateCircArc`.
    CircArc,
    /// `N_CreateCircArcTol`.
    CircArcTol,
    /// `N_CalcCircWeights`.
    CircWeights,
    /// `N_CreateEllipticalArcTol`.
    EllipticalArcTol,
    /// `N_CreateEllipticalArcAngTypeTol`.
    EllipticalArcAngTypeTol,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::QuadraticArcTol => 0,
            Op::QuarticArcTol => 1,
            Op::QuinticArcTol => 2,
            Op::CircArc => 3,
            Op::CircArcTol => 4,
            Op::CircWeights => 5,
            Op::EllipticalArcTol => 6,
            Op::EllipticalArcAngTypeTol => 7,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_common_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn angles(a: &[f64]) -> Result<(), FfiError> {
    if a.iter().any(|v| v.abs() > MAX_ANGLE) {
        return Err(bad("angle magnitude above MAX_ANGLE"));
    }
    Ok(())
}

fn ctp(v: f64) -> Result<(), FfiError> {
    if v != 2.0 && v != 4.0 && v != 5.0 {
        return Err(bad(format!("bad conic type {v}")));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    if input.iter().any(|v| !v.is_finite()) {
        return Err(bad("non-finite input"));
    }
    let len = match op {
        Op::QuadraticArcTol | Op::QuarticArcTol | Op::QuinticArcTol | Op::CircArc => 13,
        Op::CircArcTol => 14,
        Op::CircWeights => 3,
        Op::EllipticalArcTol => 15,
        Op::EllipticalArcAngTypeTol => 16,
    };
    if input.len() != len {
        return Err(bad(format!("expected {len} inputs, got {}", input.len())));
    }
    let g = |i: usize| input.get(i).copied().ok_or_else(|| bad("input too short"));
    match op {
        Op::QuadraticArcTol | Op::QuarticArcTol | Op::QuinticArcTol => angles(&[g(10)?, g(11)?]),
        Op::CircArc => {
            angles(&[g(10)?, g(11)?])?;
            ctp(g(12)?)
        }
        Op::CircArcTol => {
            angles(&[g(10)?, g(11)?])?;
            ctp(g(13)?)
        }
        Op::CircWeights => {
            angles(&[g(0)?, g(1)?])?;
            ctp(g(2)?)
        }
        Op::EllipticalArcTol | Op::EllipticalArcAngTypeTol => {
            if g(9)? == 0.0 || g(10)? == 0.0 {
                return Err(bad("zero ellipse radius"));
            }
            angles(&[g(11)?, g(12)?])?;
            if op == Op::EllipticalArcTol {
                ctp(g(14)?)
            } else {
                let atp = g(14)?;
                if atp != 0.0 && atp != 1.0 {
                    return Err(bad(format!("bad angle type {atp}")));
                }
                ctp(g(15)?)
            }
        }
    }
}

/// Runs `op` on `input` (layout in the module docs); returns the shim's output record
/// (status, then outputs on success).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly the values the
        // shim reads for `op` (validated above); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_common_eval(
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
