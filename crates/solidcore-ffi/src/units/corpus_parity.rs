//! FFI wrappers for unit `corpus.parity`.
//!
//! Shim: `harness/cpp/shim/corpus_parity.cpp` (`sc_corpus_parity_measure`): upstream's
//! `SmBrep::ReadFromFile` / `SmBrepData::ReadPartFromFile` on one model file, then per
//! B-rep `IsManifoldSolid` / `IsSheet`, the counts, the per-face area integration of
//! `SmApiFaceComputeArea` (without its accuracy clamp) and `SmApiBrepComputeVolume`.
//!
//! | Function    | Output |
//! |-------------|--------|
//! | [`measure`] | `read_status n`, then per B-rep `kind faces edges vertices loops status area volume` |
//! | [`roundtrip`] | as [`measure`], each B-rep measured after `SmBrep::WriteToFile` (ASCII) and `ReadFromFile` |
//!
//! `kind` is 0 empty, 1 solid, 2 sheet, 3 other; `status` is 0 or the first failing
//! measurement's `SmStatus`; `volume` is NaN unless the B-rep is a solid. The objects
//! read are leaked: call it from a short-lived process.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_corpus_parity_measure(
            path: *const std::ffi::c_char,
            accuracy: f64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_corpus_parity_roundtrip(
            path: *const std::ffi::c_char,
            tmp: *const std::ffi::c_char,
            accuracy: f64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Values per B-rep in [`measure`]'s output.
pub const PER_BREP: usize = 8;

/// Reads model file `path` with upstream's reader and measures every B-rep at
/// integration accuracy `accuracy` (see the module docs for the layout).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a path with a NUL byte or a non-positive or
/// non-finite accuracy; [`FfiError::Status`] for a shim failure.
pub fn measure(path: &str, accuracy: f64) -> Result<Vec<f64>, FfiError> {
    if !(accuracy.is_finite() && accuracy > 0.0) {
        return Err(FfiError::InvalidArgument("accuracy".into()));
    }
    let cpath = std::ffi::CString::new(path)
        .map_err(|_| FfiError::InvalidArgument("path has a NUL byte".into()))?;
    call_with_buffer(64, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; buf is valid for buf.len()
        // writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_corpus_parity_measure(
                cpath.as_ptr(),
                accuracy,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Reads model file `path` with upstream's reader, then writes every B-rep to the ASCII
/// file `tmp` (`SmBrep::WriteToFile`: `SmBrepData::FromBrep`, `WriteToFile`), reads it
/// back into a new B-rep (`SmBrep::ReadFromFile`) and measures that at integration
/// accuracy `accuracy`; `tmp` is removed afterwards. The layout is [`measure`]'s, a
/// failed write or read back giving its `SmStatus` as the B-rep's `status`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a path with a NUL byte or a non-positive or
/// non-finite accuracy; [`FfiError::Status`] for a shim failure.
pub fn roundtrip(path: &str, tmp: &str, accuracy: f64) -> Result<Vec<f64>, FfiError> {
    if !(accuracy.is_finite() && accuracy > 0.0) {
        return Err(FfiError::InvalidArgument("accuracy".into()));
    }
    let cpath = std::ffi::CString::new(path)
        .map_err(|_| FfiError::InvalidArgument("path has a NUL byte".into()))?;
    let ctmp = std::ffi::CString::new(tmp)
        .map_err(|_| FfiError::InvalidArgument("tmp has a NUL byte".into()))?;
    call_with_buffer(64, |buf, len| {
        // SAFETY: cpath and ctmp are valid NUL-terminated strings; buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_corpus_parity_roundtrip(
                cpath.as_ptr(),
                ctmp.as_ptr(),
                accuracy,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
