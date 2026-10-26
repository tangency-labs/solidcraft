//! FFI wrapper for the part reader (`topology::part_file`, outside the plan).
//!
//! Shim: `harness/cpp/shim/topology_part_file.cpp`.
//! Upstream: `source/SMLib/src/SmBrepData.cpp` (`SmBrepData::ReadPartFromFile`).
//!
//! | Rust    | shim                         | upstream                                    |
//! |---------|------------------------------|---------------------------------------------|
//! | [`run`] | `sc_topology_part_file_run`  | `SmBrepData::ReadPartFromFile` and a dump   |
//!
//! # Output
//!
//! The read status (`0` = `SM_SUCCESS`), then on success: the curve count and per curve
//! its type, dimension and natural interval (2); the surface count and per surface its
//! type and natural UV domain (`umin vmin umax vmax`); the Boolean tree node count and
//! the nodes; the B-rep count and per B-rep its tolerance, `IsManifoldSolid` flag and
//! region, shell, face, loop (over the faces' `GetLoops`), edge and vertex counts.
//!
//! # Guard
//!
//! Upstream's part reader trusts every count and extraction of the file, so [`run`]
//! accepts only upstream's own test files: a path under
//! `upstream/TestFiles/pt_TestFiles` (after resolving it) whose contents start with
//! `//[Output Summary]`; anything else is [`FfiError::InvalidArgument`].

use std::path::{Component, Path};

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_topology_part_file_run(
            path: *const std::ffi::c_char,
            file_type: i32,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.to_string())
}

/// The guard of [`run`]: `path` names an upstream part test file.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for any other file.
pub fn validate(path: &str) -> Result<(), FfiError> {
    let p = Path::new(path);
    if p.components().any(|c| c == Component::ParentDir) {
        return Err(bad("path with `..`"));
    }
    let canon = p.canonicalize().map_err(|e| bad(&format!("{path}: {e}")))?;
    let under = canon
        .ancestors()
        .any(|a| a.ends_with(Path::new("upstream/TestFiles/pt_TestFiles")));
    if !under {
        return Err(bad("not an upstream test file"));
    }
    let bytes = std::fs::read(&canon).map_err(|e| bad(&format!("{path}: {e}")))?;
    if !bytes.starts_with(b"//[Output Summary]") {
        return Err(bad("not a part file"));
    }
    Ok(())
}

/// `SmBrepData::ReadPartFromFile(ctx, path, ..., SM_ASCII)` and the dump described in
/// the module docs.
///
/// # Errors
///
/// Those of [`validate`]; [`FfiError::Status`] for a shim failure.
pub fn run(path: &str) -> Result<Vec<f64>, FfiError> {
    validate(path)?;
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string naming a validated upstream
        // part file; buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_topology_part_file_run(
                cpath.as_ptr(),
                0,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
