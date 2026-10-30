//! FFI wrapper for unit `topology.brep_constructor`.
//!
//! Shim: `harness/cpp/shim/topology_brep_constructor.cpp` (`sc_topology_brep_constructor_run`):
//! runs a constructor script ([`step_import::script`]) on upstream `SmBrepConstructor` in
//! a forked child and returns its blocks `tag n value*n` (layout in the shim's header).
//! [`run`] accepts only a script [`step_import::script::decode`] accepts (header, counts
//! against the data, validated geometry, each curve and surface used once, references to
//! earlier results only), mode `0` (dump) or `1` (corpus stages) and a finite accuracy.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_topology_brep_constructor_run(
            input: *const f64,
            n_in: i64,
            mode: i32,
            accuracy: f64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Runs the encoded script in C++ and returns its output blocks.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a script [`step_import::script::decode`] rejects, a
/// mode other than 0 or 1 or a non-finite accuracy; [`FfiError::Status`] when the shim
/// fails (it cannot fork, or a C++ exception).
pub fn run(script: &[f64], mode: i32, accuracy: f64) -> Result<Vec<f64>, FfiError> {
    step_import::script::decode(script).map_err(FfiError::InvalidArgument)?;
    if mode != 0 && mode != 1 {
        return Err(FfiError::InvalidArgument(format!("mode {mode}")));
    }
    if !accuracy.is_finite() {
        return Err(FfiError::InvalidArgument("accuracy not finite".into()));
    }
    let n =
        i64::try_from(script.len()).map_err(|_| FfiError::InvalidArgument("too long".into()))?;
    call_with_buffer(1 << 16, |buf, len| {
        // SAFETY: script is valid for n reads; buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_topology_brep_constructor_run(
                script.as_ptr(),
                n,
                mode,
                accuracy,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
