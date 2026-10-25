//! FFI wrapper for the STEP corpus driver (`difftest --step-corpus`; not a port unit).
//!
//! Shim: `harness/cpp/shim/step_import.cpp` (`sc_step_import_build`): builds one SMLib
//! B-rep from a body of the clean-room STEP importer, encoded by
//! [`step_import::flat::encode`], with `SM_API`/`SMLib` calls only, and reports the stages
//! the Rust side (`crates/difftest/src/step.rs`) reports for the same body.
//!
//! Output: stage blocks `id n value*n`, ids [`STAGES`] (layout in the shim's header).
//! [`build`] accepts only an input [`step_import::flat::decode`] accepts (header,
//! counts against the data, indices, flags, finite numbers, B-spline invariants) and a
//! finite accuracy; everything else the shim reads is checked there.

use crate::{call_with_buffer, FfiError};

/// Stage names by id (`1..`).
pub const STAGES: [&str; 10] = [
    "faces",
    "stitch",
    "counts",
    "manifold",
    "area",
    "volume",
    "heal",
    "healed_counts",
    "healed_manifold",
    "healed_volume",
];

mod sys {
    extern "C" {
        pub fn sc_step_import_build(
            input: *const f64,
            n_in: i64,
            accuracy: f64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Builds the encoded body in C++ and returns its stage blocks.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input [`step_import::flat::decode`] rejects or a
/// non-finite accuracy; [`FfiError::Status`] for a shim failure (a C++ exception).
pub fn build(body: &[f64], accuracy: f64) -> Result<Vec<f64>, FfiError> {
    step_import::flat::decode(body).map_err(FfiError::InvalidArgument)?;
    if !accuracy.is_finite() {
        return Err(FfiError::InvalidArgument("accuracy not finite".into()));
    }
    let n = i64::try_from(body.len()).map_err(|_| FfiError::InvalidArgument("too long".into()))?;
    call_with_buffer(64, |buf, len| {
        // SAFETY: body is valid for n reads; buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_step_import_build(
                body.as_ptr(),
                n,
                accuracy,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Splits stage blocks into `(name, values)`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed block list.
pub fn stages(out: &[f64]) -> Result<Vec<(&'static str, Vec<f64>)>, FfiError> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < out.len() {
        let (Some(&id), Some(&n)) = (out.get(i), out.get(i + 1)) else {
            return Err(FfiError::InvalidArgument("truncated stage".into()));
        };
        let name = (id >= 1.0 && id.fract() == 0.0)
            .then(|| STAGES.get(id as usize - 1))
            .flatten()
            .ok_or_else(|| FfiError::InvalidArgument(format!("stage id {id}")))?;
        if !(n >= 0.0 && n.fract() == 0.0) {
            return Err(FfiError::InvalidArgument(format!("stage length {n}")));
        }
        let end = (i + 2)
            .checked_add(n as usize)
            .ok_or_else(|| FfiError::InvalidArgument("stage length".into()))?;
        let vals = out
            .get(i + 2..end)
            .ok_or_else(|| FfiError::InvalidArgument("truncated stage".into()))?;
        v.push((*name, vals.to_vec()));
        i = end;
    }
    Ok(v)
}
