//! Safe file-backed native-face probes for `surface.trim_srf_cache`.
//!
//! Upstream: `source/SMLib/src/SmTrimSrfCache.cpp` (`BuildTree`, `PointTest`,
//! `PointClassify`). Shim: `harness/cpp/shim/surface_trim_srf_cache_faces.cpp`.
//!
//! Input: `surface_type loop_count occurrence build editing modified test_tol
//! boundary_probes sample_count (u_fraction v_fraction source_tolerance)*`.
//! The selector is the zero-based occurrence among faces of that **exact** concrete
//! surface type and upward-loop count in the shared native Graph walk.
//!
//! Output: `read_status face_index surface_type loops owned face_domain[4]
//! build_status editing modified boundary_enabled point_enabled have_trim containment
//! cache_domain[4] nodes probes`, followed by each probe's `mode edgeuse u v tolerance`
//! and four records `status result object_type object_index`: direct `PointTest`,
//! direct `PointClassify`, base-pointer `PointTest`, base-pointer `PointClassify`.
//! All statuses retain the native `SmStatus` value (`SM_SUCCESS = 1000`); skipped
//! construction is `-1`. Null identities are `-1 -1`; an unregistered identity keeps
//! its concrete type and index `-3`. `mode` is fraction (0), trim midpoint (1), or
//! trim start (2). Boundary probes visit the first upward edge use of each loop,
//! at tolerance `1e-5`; at most 32 loops. A read failure contains only its status;
//! a build failure contains the complete header with zero probes, never a rejection.

use std::ffi::CString;
use std::path::Path;

use crate::{call_with_buffer, FfiError};

/// Maximum number of explicit UV-fraction samples (automatic boundary probes are extra).
pub const MAX_SAMPLES: usize = 64;

mod sys {
    extern "C" {
        pub fn sc_surface_trim_srf_cache_faces_eval(
            path: *const std::ffi::c_char,
            input: *const f64,
            input_len: i64,
            output: *mut f64,
            capacity: i64,
            output_len: *mut i64,
        ) -> i32;
    }
}

fn bad(message: &str) -> FfiError {
    FfiError::InvalidArgument(format!("native face cache: {message}"))
}

/// Checks the entire narrow file-backed protocol before entering native code.
/// Fractions may be outside `[0, 1]` to test points outside the face bounding box.
pub fn validate(path: &str, input: &[f64]) -> Result<(), FfiError> {
    if path.contains('\0')
        || Path::new(path).extension().and_then(|s| s.to_str()) != Some("smb")
        || !Path::new(path).is_file()
    {
        return Err(bad("expected an existing .smb file without NUL bytes"));
    }
    let head = input.get(..9).ok_or_else(|| bad("truncated header"))?;
    if input.iter().any(|v| !v.is_finite()) {
        return Err(bad("all inputs must be finite"));
    }
    if !matches!(head.first(), Some(15011.0 | 15002.0)) {
        return Err(bad(
            "only exact SmPlane (15011) or SmBSplineSurface (15002) is supported",
        ));
    }
    for value in head.get(1..3).ok_or_else(|| bad("missing selector"))? {
        if value.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(value) {
            return Err(bad(
                "loop count and occurrence must be nonnegative u32 integers",
            ));
        }
    }
    if head
        .get(3..8)
        .ok_or_else(|| bad("missing flags"))?
        .iter()
        .any(|v| *v != 0.0 && *v != 1.0)
    {
        return Err(bad("flags must be 0 or 1"));
    }
    let count = *head.get(8).ok_or_else(|| bad("missing sample count"))?;
    if count.fract() != 0.0 || !(0.0..=MAX_SAMPLES as f64).contains(&count) {
        return Err(bad("sample count must be an integer in 0..=64"));
    }
    let samples = input.get(9..).ok_or_else(|| bad("missing samples"))?;
    if samples.len() != count as usize * 3 {
        return Err(bad("sample count does not match UV-fraction triples"));
    }
    if samples
        .as_chunks::<3>()
        .0
        .iter()
        .any(|p| p.get(2).is_none_or(|t| *t < 0.0))
    {
        return Err(bad("source tolerances must be nonnegative"));
    }
    Ok(())
}

/// Runs live native cache operations on a real face read from `path`.
/// Missing face selectors and invalid derived UV coordinates are also checked by
/// C++, before it dereferences the selected face or invokes a classifier.
pub fn eval(path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(path, input)?;
    let path = CString::new(path).map_err(|_| bad("path contains NUL"))?;
    // The bounded protocol needs at most 22 + (64 + 2*32)*21 doubles.
    call_with_buffer(4096, |buffer, length| {
        // SAFETY: path is NUL-terminated; validate checked every input field and
        // length. C++ checks selector existence and computed UVs; the output buffer
        // and length pointer remain valid and the shim honours their capacity.
        unsafe {
            sys::sc_surface_trim_srf_cache_faces_eval(
                path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *length,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../upstream/TestFiles/pt_TestFiles/Fillet/ext_w_hole.smb")
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn validates_layout_before_native_code() {
        let path = path();
        if !Path::new(&path).is_file() {
            return;
        }
        let good = vec![
            15011.0, 2.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.5, 0.5, 1e-5,
        ];
        assert!(validate(&path, &good).is_ok());
        for length in 0..good.len() {
            assert!(validate(&path, good.get(..length).unwrap_or_default()).is_err());
        }
        for (index, value) in [
            (0, 15030.0),
            (1, -1.0),
            (2, 0.5),
            (3, 2.0),
            (8, 65.0),
            (9, f64::INFINITY),
            (10, f64::NAN),
            (11, -1.0),
        ] {
            let mut input = good.clone();
            *input.get_mut(index).unwrap() = value;
            assert!(validate(&path, &input).is_err(), "index {index}");
        }
        assert!(validate("missing.smb", &good).is_err());
        assert!(validate("bad\0.smb", &good).is_err());
        let mut outside = good;
        *outside.get_mut(9).unwrap() = -0.25;
        assert!(validate(&path, &outside).is_ok());
    }
}
