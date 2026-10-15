//! FFI wrappers for unit `heal.heal_data_p1`.
//!
//! Shim: `harness/cpp/shim/heal_heal_data_p1.cpp` (`sc_heal_heal_data_p1_api`).
//! Upstream: `source/SM_API/src/SmApiHeal.cpp` (`SmApiHealBrep`).
//!
//! The `SmHealData` members of the unit run as steps `23..=31` of the heal scripts of
//! [`super::heal_heal_data_p2::heal`] (documented with
//! [`super::heal_heal_data_p2::check_heal`]), which builds the healer on a real B-rep.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_heal_heal_data_p1_api(
            kind: i32,
            dx: f64,
            dy: f64,
            dz: f64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn call(kind: i32, d: [f64; 3]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(8, |buf, len| {
        // SAFETY: buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid; the scalars are plain values.
        unsafe {
            sys::sc_heal_heal_data_p1_api(
                kind,
                d[0],
                d[1],
                d[2],
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// `SmApiHealBrep(NULL)`: outputs `0 heal_status`.
///
/// # Errors
///
/// [`FfiError::Status`] for a shim failure.
pub fn api_heal_null() -> Result<Vec<f64>, FfiError> {
    call(0, [0.0; 3])
}

/// `SmApiHealBrep` on `SmApiCreateBox(origin, dx, dy, dz)`: outputs
/// `create_status heal_status` (`heal_status` 0 when no box was made).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a side that is not finite; [`FfiError::Status`]
/// for a shim failure.
pub fn api_heal_box(d: [f64; 3]) -> Result<Vec<f64>, FfiError> {
    if d.iter().any(|x| !x.is_finite()) {
        return Err(FfiError::InvalidArgument(format!("bad box sides {d:?}")));
    }
    call(1, d)
}
