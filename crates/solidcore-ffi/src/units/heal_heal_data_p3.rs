//! FFI for `heal.heal_data_p3`, using the shared real-topology healer scripts.
//! Upstream: `source/SMLib/src/SmHealData.cpp`; shim: `heal_heal_data_p3.cpp`.

use super::heal_heal_data_p2::{check_block, check_heal};
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_heal_heal_data_p3_eval(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Runs a validated healer script, including steps 32..42 documented in
/// [`super::heal_heal_data_p2::check_heal`], on a real C++ B-rep.
pub fn heal(path: &str, block: &[f64], args: &[f64]) -> Result<Vec<f64>, FfiError> {
    check_block(path, block)?;
    check_heal(args)?;
    let path = std::ffi::CString::new(path)
        .map_err(|_| FfiError::InvalidArgument("path contains NUL".into()))?;
    let mut input = block.to_vec();
    input.extend_from_slice(args);
    call_with_buffer(8192, |buf, len| {
        // SAFETY: block and script lengths, counts and enums were validated;
        // path is NUL terminated and all slices are valid for the supplied sizes.
        // The native shim checks output capacity and catches exceptions.
        unsafe {
            sys::sc_heal_heal_data_p3_eval(
                path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
