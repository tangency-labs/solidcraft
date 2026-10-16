//! FFI wrappers for unit `nlib.spiral`.
//!
//! Shim: `harness/cpp/shim/nlib_spiral.cpp` (`sc_nlib_spiral_eval`).
//! Upstream: `source/SMLib/src/NL_Spiral.cpp`, `source/SMLib/src/SmNurbsVol.cpp`.
//!
//! | wrapper            | upstream                                                   |
//! |--------------------|------------------------------------------------------------|
//! | [`approx_spiral`]  | `N_CrvApproxSpiral`                                        |
//! | [`volume_copy`]    | `sm_AllocateNurbVolume` (`sm_ComputeNurbVolumeSize`, `sm_InitNurbVolumeMemory`), `sm_AllocateAndCopyNurbVolume` |
//!
//! [`approx_spiral`] returns the status (`0` or the `NL_*_ERR` number) and on
//! success `*NumPoints`, `*tol` and the curve as `n p m` + control points `x y z w` +
//! knots. [`volume_copy`] returns two volumes (the fresh allocation, then the copy of
//! it after filling), each as `l m n p q r ku kv kw`, the knots `U V W` and the
//! control points `x y z w` (`w` index fastest).
//!
//! Inputs are validated so upstream stays inside its arrays and finishes quickly:
//! finite values of magnitude at most [`MAX_MAGNITUDE`], at most [`MAX_TURNS`] turns
//! (the initial span count is `8 * turns`), at most [`MAX_POINTS`] points; volume
//! sizes at most [`MAX_VOLUME_INDEX`] and data of exactly the declared lengths.
//!
//! [`approx_spiral`] also rejects the inputs of upstream's early input checks
//! ([`hits_early_input_error`]): those `NL_ERROR` before `N_InitNurbs(&SL)` and the
//! exit path then runs `N_EndNurbs(&SL)` on the uninitialised local stacks, which
//! crashes the process (`free(): invalid pointer`; see `plan/findings.md`).

use crate::{call_with_buffer, FfiError};

/// Largest magnitude accepted for heights, radii and tolerances.
pub const MAX_MAGNITUDE: f64 = 1.0e18;
/// Largest number of turns accepted.
pub const MAX_TURNS: f64 = 1000.0;
/// Largest `num_points` accepted (smaller values, including negative ones, fit).
pub const MAX_POINTS: i64 = 100_000;
/// Largest highest index accepted for volume control points, knots and degrees.
pub const MAX_VOLUME_INDEX: usize = 64;

mod sys {
    extern "C" {
        pub fn sc_nlib_spiral_eval(
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

/// Runs `op` on an input the caller has validated.
fn run(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly the record
        // the shim reads for `op` (built and validated by the public wrappers); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_spiral_eval(
                op,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

fn check_value(v: f64, what: &str) -> Result<(), FfiError> {
    if v.is_finite() && v.abs() <= MAX_MAGNITUDE {
        Ok(())
    } else {
        Err(bad(format!("{what}: not finite or too large")))
    }
}

/// Upstream `NL_MTOL` (default).
const MTOL: f64 = 1.0e-07;

/// True when `N_CrvApproxSpiral` fails one of the input checks made before it
/// initialises its local stacks (and then crashes freeing them).
#[must_use]
pub fn hits_early_input_error(height: f64, rad0: f64, rad1: f64, turns: f64) -> bool {
    let r0 = if rad0 < MTOL { 0.0 } else { rad0 };
    let r1 = if rad1 < MTOL { 0.0 } else { rad1 };
    height < MTOL || rad0 < -MTOL || rad1 < -MTOL || (r0 + r1) < MTOL || turns < MTOL
}

/// `N_CrvApproxSpiral(height, rad0, rad1, turns, rorl, &num_points, &tol, ..)`.
pub fn approx_spiral(
    height: f64,
    rad0: f64,
    rad1: f64,
    turns: f64,
    rorl: i64,
    num_points: i64,
    tol: f64,
) -> Result<Vec<f64>, FfiError> {
    check_value(height, "height")?;
    check_value(rad0, "rad0")?;
    check_value(rad1, "rad1")?;
    check_value(tol, "tol")?;
    if !turns.is_finite() || turns.abs() > MAX_TURNS {
        return Err(bad("turns: not finite or too large"));
    }
    if hits_early_input_error(height, rad0, rad1, turns) {
        return Err(bad("input error before N_InitNurbs(&SL): upstream crashes"));
    }
    if !(-MAX_POINTS..=MAX_POINTS).contains(&num_points) {
        return Err(bad("num_points out of range"));
    }
    if rorl.unsigned_abs() > 1 << 40 {
        return Err(bad("rorl out of range"));
    }
    run(
        0,
        &[
            height,
            rad0,
            rad1,
            turns,
            rorl as f64,
            num_points as f64,
            tol,
        ],
    )
}

/// `sm_AllocateNurbVolume(l, m, n, p, q, r, ku, kv, kw)`, dumped, then filled with
/// `knots` (`U[0..=ku]`, `V[0..=kv]`, `W[0..=kw]`) and `cpts` (`x y z w` each, `w`
/// index fastest), copied with `sm_AllocateAndCopyNurbVolume` and the copy dumped.
pub fn volume_copy(
    cp_hi: [usize; 3],
    degree: [usize; 3],
    knot_hi: [usize; 3],
    knots: &[f64],
    cpts: &[[f64; 4]],
) -> Result<Vec<f64>, FfiError> {
    if cp_hi
        .iter()
        .chain(&degree)
        .chain(&knot_hi)
        .any(|&x| x > MAX_VOLUME_INDEX)
    {
        return Err(bad("volume size out of range"));
    }
    let n_knots: usize = knot_hi.iter().map(|k| k + 1).sum();
    let n_cpts: usize = cp_hi.iter().map(|k| k + 1).product();
    if knots.len() != n_knots || cpts.len() != n_cpts {
        return Err(bad("volume data of the wrong length"));
    }
    if !knots
        .iter()
        .chain(cpts.iter().flatten())
        .all(|v| v.is_finite())
    {
        return Err(bad("volume data not finite"));
    }
    let mut o: Vec<f64> = cp_hi
        .iter()
        .chain(&degree)
        .chain(&knot_hi)
        .map(|&x| x as f64)
        .collect();
    o.extend_from_slice(knots);
    for c in cpts {
        o.extend_from_slice(c);
    }
    run(1, &o)
}
