//! Tests the real polynomial wrapper's guards without linking the C++ kernel.

#![cfg(not(feature = "cpp"))]
#![allow(unsafe_code)] // Same FFI scope as lib.rs: includes the real extern wrapper.

use solidcore_ffi::{check, FfiError};

#[path = "../src/units/geom_polynomial.rs"]
mod geom_polynomial;

// A distinctive error records dispatch in the wrapper's returned result, without
// shared mutable state or touching the output pointers. These tests check only
// the FFI guard, not upstream's numerical results.
const STUB_DISPATCHED: i32 = -12345;

#[no_mangle]
extern "C" fn sc_geom_polynomial_solve(
    _degree: i32,
    _c: *const f64,
    _tol: f64,
    _out: *mut f64,
    _out_n: *mut i64,
    _out_status: *mut i32,
) -> i32 {
    STUB_DISPATCHED
}

#[test]
fn zero_quartic_coefficient_dispatches_to_lower_degree_fallback() {
    assert_eq!(
        geom_polynomial::solve(&[-8.0, 4.0, 0.0, 0.0, 0.0], 2.0),
        Err(FfiError::Status(STUB_DISPATCHED))
    );
}

#[test]
fn nonzero_quartic_coefficient_below_tolerance_dispatches() {
    for leading in [-1.0, 1.0] {
        assert_eq!(
            geom_polynomial::solve(&[-8.0, 4.0, 0.0, 0.0, leading], 2.0),
            Err(FfiError::Status(STUB_DISPATCHED))
        );
    }
}

#[test]
fn quartic_coefficient_at_tolerance_keeps_resolvent_guard() {
    for leading in [-2.0, 2.0] {
        assert_eq!(
            geom_polynomial::solve(&[-8.0, 4.0, 0.0, 0.0, leading], 2.0),
            Err(FfiError::InvalidArgument("quartic tol 2".into()))
        );
    }
}

#[test]
fn known_unsafe_quartic_is_rejected_before_dispatch() {
    assert_eq!(
        geom_polynomial::solve(&[4.0, 0.0, 0.0, 0.0, 2.0], 2.0),
        Err(FfiError::InvalidArgument("quartic tol 2".into()))
    );
}

#[test]
fn nan_quartic_coefficient_keeps_resolvent_guard() {
    assert_eq!(
        geom_polynomial::solve(&[-8.0, 4.0, 0.0, 0.0, f64::NAN], 2.0),
        Err(FfiError::InvalidArgument("quartic tol 2".into()))
    );
}

#[test]
fn unsupported_coefficient_lengths_are_rejected_before_dispatch() {
    for coefficients in [&[][..], &[1.0][..], &[1.0; 2][..], &[1.0; 6][..]] {
        assert_eq!(
            geom_polynomial::solve(coefficients, 2.0),
            Err(FfiError::InvalidArgument(format!(
                "{} coefficients",
                coefficients.len()
            )))
        );
    }
}
