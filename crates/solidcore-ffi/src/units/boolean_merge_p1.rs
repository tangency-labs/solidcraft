//! Safe wrappers for `boolean.merge_p1`; shim: `harness/cpp/shim/boolean_merge_p1.cpp`.
//!
//! Operations 1..9 use the validated B-rep block of `topology_vertexuse`. Selectors
//! are nonnegative integers below 32 and resolved modulo actual entity counts by
//! the shim. Operations requiring absent entities reject the fixture before call.
use super::topology_vertexuse::{self as fixture, Op};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;
mod sys {
    extern "C" {
        pub fn sc_boolean_merge_p1_eval(
            op: i32,
            a: i32,
            b: i32,
            c: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            other_path: *const std::ffi::c_char,
            other_input: *const f64,
            other_n: i64,
            edit_tolerances: i32,
            primary_tolerance: f64,
            other_tolerance: f64,
            parameter: f64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}
/// A validated topology fixture used as one operand.
#[derive(Clone, Copy)]
pub struct Operand<'a> {
    /// Path used by a file fixture, or empty for procedural geometry.
    pub path: &'a str,
    /// Fixture construction program.
    pub block: &'a [f64],
}

/// Inputs to a merge helper; operations 8 and 9 compare cross-operand radial queries.
#[derive(Clone, Copy)]
pub struct Request<'a> {
    /// Operation selector, 0..=9.
    pub op: u8,
    /// Entity and branch selectors, each 0..=31.
    pub args: [u8; 3],
    /// Primary topology construction program.
    pub primary: Operand<'a>,
    /// Independently constructed other operand.
    pub other: Operand<'a>,
    /// Optional primary/source vertex tolerances, applied after fixture Relate.
    pub tolerances: Option<[f64; 2]>,
    /// Normalized parameter for operation 8.
    pub parameter: f64,
}

/// Whether the primary and other fixtures (entity counts from
/// [`fixture::counts`]) have the entities operation `op` selects; [`eval`] rejects
/// the fixtures otherwise (the selectors are taken modulo these counts).
#[must_use]
pub fn selection_valid(op: u8, a: &[usize; 10], b: &[usize; 10]) -> bool {
    match op {
        2 => a[3] > 0 && b[3] > 0,
        3 => a[9] > 0 && b[9] > 0 && a[0] > 0,
        4 => a[0] > 0 && b[0] > 0,
        5 => b[2] > 0,
        6 => a[7] > 0,
        8 | 9 => a[6] > 0 && b[6] > 0,
        _ => true,
    }
}

/// Call a merge helper. Result: status, record length, record, then a raw graph
/// export in the `topology_vertexuse` format (the primary B-rep's, after the call).
/// Op 0 returns only control flags. Op 2 (`FixFoundFaceCoincidence`, which can edit
/// both B-reps) ends its record with the other B-rep's graph after the call: the
/// export's length, then the export itself in the same format.
pub fn eval(request: Request<'_>) -> Result<Vec<f64>, FfiError> {
    let Request {
        op,
        args,
        primary,
        other,
        tolerances,
        parameter,
    } = request;
    if op > 9
        || args.iter().any(|&a| a > 31)
        || !parameter.is_finite()
        || tolerances.is_some_and(|t| t.iter().any(|v| !v.is_finite() || *v < 0.0))
    {
        return Err(FfiError::InvalidArgument(
            "merge op/selectors/tolerances/parameter".into(),
        ));
    }
    if op != 0 {
        let ac = fixture::eval(Op::Export, primary.path, primary.block)?;
        let bc = fixture::eval(Op::Export, other.path, other.block)?;
        let a = fixture::counts(&ac)
            .ok_or_else(|| FfiError::InvalidArgument("primary fixture rejected".into()))?;
        let b = fixture::counts(&bc)
            .ok_or_else(|| FfiError::InvalidArgument("other fixture rejected".into()))?;
        if !selection_valid(op, &a, &b) {
            return Err(FfiError::InvalidArgument(
                "fixture lacks selected entity".into(),
            ));
        }
    } else if !primary.block.is_empty() || !other.block.is_empty() {
        return Err(FfiError::InvalidArgument("control takes no fixture".into()));
    }
    let path =
        CString::new(primary.path).map_err(|_| FfiError::InvalidArgument("NUL in path".into()))?;
    let other_path = CString::new(other.path)
        .map_err(|_| FfiError::InvalidArgument("NUL in other path".into()))?;
    let [a, b, c] = args;
    let [pt, ot] = tolerances.unwrap_or([0.0, 0.0]);
    call_with_buffer(4096, |out, len| {
        // SAFETY: both construction programs are validated above; the shim guards
        // selected radial geometry before dereferencing it. All buffers and paths
        // remain live for the call, and no C++ pointer escapes.
        unsafe {
            sys::sc_boolean_merge_p1_eval(
                i32::from(op),
                i32::from(a),
                i32::from(b),
                i32::from(c),
                path.as_ptr(),
                primary.block.as_ptr(),
                primary.block.len() as i64,
                other_path.as_ptr(),
                other.block.as_ptr(),
                other.block.len() as i64,
                i32::from(tolerances.is_some()),
                pt,
                ot,
                parameter,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
