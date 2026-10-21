//! FFI wrappers for unit `blend.fillet_corner_p2`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_corner_p2_static.inc`
//! (`sc_blend_fillet_corner_p2_find_concave`), included at the end of
//! `blend_fillet_corner_p1.cpp`, the one translation unit that compiles
//! `source/SMLib/src/SmFilletCorner.cpp` (to reach its file-static helpers).
//!
//! | Function        | Upstream                 | Input                              | Output |
//! |-----------------|--------------------------|------------------------------------|--------|
//! | [`find_concave`]| `sm_FindConcaveEdgeuses` | B-rep block, `v n eu*n` (`n >= 1`) | `0 (-2 \| count eu*count)` |
//!
//! The B-rep block is unit `topology.edgeuse_p1`'s (`path` is the `.smb` file of a file
//! block); `v` and the edge uses are indices in that unit's export. The output is
//! `-1 status` when the B-rep could not be built; `-2` means "refused": a pair of edge
//! uses the loop reaches has an edge use without radial, vertex use or edge, which
//! upstream dereferences.
//!
//! Inputs are validated before the call: the block, the index ranges and a non-empty
//! edge use list (upstream reads `[GetSize() - 1]` unchecked).

use crate::{call_with_buffer, FfiError};

use super::topology_edgeuse_p1::{self as eu1, bad, brep_block, Reader};

/// The largest edge use count of [`find_concave`].
pub const MAX_EDGEUSES: usize = 64;

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_corner_p2_find_concave(
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn raw(path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads (validated by
        // `validate_find_concave`); buf is valid for buf.len() writes and the shim honours
        // the capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_corner_p2_find_concave(
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

/// Validates [`find_concave`]'s input without running it.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; the errors of building and
/// exporting the B-rep (which runs the shim of unit `topology.edgeuse_p1`).
pub fn validate_find_concave(path: &str, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = eu1::eval(eu1::Op::Export, path, block)?;
    let Some(c) = eu1::counts(&export) else {
        // the B-rep cannot be built: the shim reports it; the arguments are not checked
        return Ok(());
    };
    let (n_eu, n_v) = (c[6], c[9]);
    rd.below(n_v)?;
    let n = rd.index(MAX_EDGEUSES)?;
    if n == 0 {
        return Err(bad(
            "find_concave: no edge uses (upstream indexes past the array)",
        ));
    }
    for _ in 0..n {
        rd.below(n_eu)?;
    }
    rd.done()
}

/// `sm_FindConcaveEdgeuses` on a built B-rep (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn find_concave(path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_find_concave(path, input)?;
    raw(path, input)
}
