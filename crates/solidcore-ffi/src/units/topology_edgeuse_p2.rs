//! FFI wrappers for unit `topology.edgeuse_p2`.
//!
//! Shim: `harness/cpp/shim/topology_edgeuse_p2.cpp` (`sc_topology_edgeuse_p2_eval`).
//! Upstream: `source/SMLib/src/SmEdgeuse.cpp`.
//!
//! Every op starts with a B-rep block of unit `topology.edgeuse_p1` (see
//! [`super::topology_edgeuse_p1`]; its `Export` op exports the graph) followed by the
//! edge use's index in the export's traversal order. Outputs start with `0`, or with
//! `-1 status` when the B-rep could not be built. `-2` in an output is "upstream would
//! dereference null" (the op, through `EvaluateBinormal` of unit `topology.edgeuse_p3`,
//! would dereference a null pointer, so the shim does not call it).
//!
//! | [`Op`]     | parameters after `eu`                         | outputs |
//! |------------|-----------------------------------------------|---------|
//! | `Tangent`  | `tol_deg coincidence ok_make`                 | `IsTangentSector` flag or `-2` |
//! | `Convex`   | `n step_off tol_deg`                          | `IsConvexRadialSector` flag or `-2` |
//! | `StepOff`  | `frac dist has_c C[3] has_n N[3] ok_make`     | `-2`, or `status P[3] B[3] T[3] N[3] u v` (`EvaluateBinormalStepOff` at the edge parameter `frac` of the interval) |
//! | `Rebuild`  |                                               | `-2`, or `status` and the edge use's state |
//! | `CheckSet` | `adjust has_fail fail_status curve`           | `-2`, or `status dmax replaced`, the edge use's and its mate's state, the edge tolerance, the record's B-rep flag (`dmax` is `-1` when not set) |
//! | `Notify`   | `operation`                                   | `-2`, or `0`, the edge use's and its mate's state, their edge/face gap flags |
//!
//! Blocks with a value above [`super::topology_edgeuse_p2::MAX_BLOCK_MAGNITUDE`] are
//! refused first (HANG-29).
//!
//! An edge use state is that of the `topology.edgeuse_p1` `SetUv` op. [`eval`] accepts
//! what [`super::topology_edgeuse_p1::eval`] accepts for the block, an edge use index
//! inside the export, a well-formed clamped B-spline curve (up to [`MAX_UV_INDEX`]), finite parameters, a
//! sample count up to [`MAX_SAMPLES`] and the notify operations `0..=16` except
//! `SM_NO_REG_PROPAGATION` (14, which dereferences region attributes of the caller).

use super::topology_edgeuse_p1::{self as p1, bad, brep_block, curve_up_to, Reader};
use crate::{call_with_buffer, FfiError};

/// Largest magnitude of a B-rep block value (the `±f64::MAX` markers `NL_NOW` / no-`z`
/// aside) that [`eval`] (and the `topology.edgeuse_p3`, `topology.loopuse` and
/// `topology.face_p3` wrappers) accept. HANG-29: with a surface or curve coordinate whose
/// square overflows (from about `1.34e154`), building the face (`CreateUVTrimCurve` ->
/// `SmCurve::GlobalCoincidenceChecker` -> `DropPoint`) never returns; with weights in
/// `1e-3..=1e3` this bound keeps every Euclidean point under that regime.
pub const MAX_BLOCK_MAGNITUDE: f64 = 1.0e150;

/// Rejects a B-rep block holding a value above [`MAX_BLOCK_MAGNITUDE`] (HANG-29). Runs
/// before the block's `Export`, which already builds the B-rep in upstream.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for such a value.
pub fn check_block_magnitudes(block: &[f64]) -> Result<(), FfiError> {
    if block.iter().any(|v| {
        let a = v.abs();
        a > MAX_BLOCK_MAGNITUDE && a < f64::MAX
    }) {
        return Err(bad("B-rep value above 1e150 (HANG-29)"));
    }
    Ok(())
}

/// Largest control point index of a `CheckSet` UV curve (UV trim curves of the
/// upstream test files reach ~70).
pub const MAX_UV_INDEX: usize = 4096;

/// Largest sample count of `Convex`.
pub const MAX_SAMPLES: usize = 64;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Tangent = 1,
    Convex = 2,
    StepOff = 3,
    Rebuild = 4,
    CheckSet = 5,
    Notify = 6,
}

mod sys {
    extern "C" {
        pub fn sc_topology_edgeuse_p2_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn args(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Tangent => {
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Convex => {
            rd.index(MAX_SAMPLES)?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::StepOff => {
            rd.finite()?;
            rd.finite()?;
            rd.flag()?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
            rd.flag()?;
        }
        Op::Rebuild => {}
        Op::CheckSet => {
            rd.flag()?;
            rd.flag()?;
            let s = rd.finite()?;
            if s.fract() != 0.0 || !(-1.0e6..=1.0e6).contains(&s) {
                return Err(bad("fail status not an integer"));
            }
            curve_up_to(rd, MAX_UV_INDEX)?;
        }
        Op::Notify => {
            let o = rd.index(16)?;
            if o == 14 {
                return Err(bad("SM_NO_REG_PROPAGATION on an edge use"));
            }
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_topology_edgeuse_p2_eval(
                op as i32,
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    check_block_magnitudes(block)?;
    let export = p1::eval(p1::Op::Export, path, block)?;
    let Some(c) = p1::counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    rd.below(n_eu)?;
    args(op, &mut rd)?;
    rd.done()?;
    raw(op, path, input)
}

