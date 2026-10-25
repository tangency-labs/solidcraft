//! Coincident-curve guards exercised without linking the C++ kernel.

#![allow(unsafe_code, dead_code)]

use solidcore_ffi::{call_with_buffer, FfiError};
use std::cell::Cell;

#[path = "../src/units/nlib_crv_basic.rs"]
mod nlib_crv_basic;
#[path = "../src/reader.rs"]
mod reader;
#[path = "../src/validate.rs"]
mod validate;

thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

const SENTINEL: f64 = 930.0;

#[no_mangle]
unsafe extern "C" fn sc_nlib_crv_basic_eval(
    op: i32,
    _input: *const f64,
    _n_in: i64,
    out: *mut f64,
    cap: i64,
    out_len: *mut i64,
) -> i32 {
    CALLS.with(|calls| calls.set(calls.get() + 1));
    if op != 6 || cap < 1 {
        return solidcore_ffi::SC_ERR_ARG;
    }
    // SAFETY: eval supplies writable output storage and a valid length pointer.
    unsafe {
        out.write(SENTINEL);
        out_len.write(1);
    }
    solidcore_ffi::SC_OK
}

fn coincident_segments(weight: f64) -> Vec<f64> {
    let mut input = Vec::new();
    for _ in 0..2 {
        // Storage N=1, K=3; active n=1, degree=1, m=3.
        input.extend([1.0, 3.0, 1.0, 1.0, 3.0]);
        let x = if weight == -f64::MAX { 1.0 } else { weight };
        input.extend([0.0, 0.0, 0.0, weight, x, 0.0, 0.0, weight]);
        input.extend([0.0, 0.0, 1.0, 1.0]);
    }
    input.push(1e-9);
    input
}

fn assert_accepted(weight: f64) {
    CALLS.with(|calls| calls.set(0));
    assert_eq!(
        nlib_crv_basic::eval(nlib_crv_basic::Op::Coincident, &coincident_segments(weight)),
        Ok(vec![SENTINEL])
    );
    CALLS.with(|calls| assert_eq!(calls.get(), 1));
}

fn assert_rejected(weight: f64) {
    CALLS.with(|calls| calls.set(0));
    assert!(matches!(
        nlib_crv_basic::eval(nlib_crv_basic::Op::Coincident, &coincident_segments(weight)),
        Err(FfiError::InvalidArgument(_))
    ));
    CALLS.with(|calls| assert_eq!(calls.get(), 0));
}

#[test]
fn lower_weight_bound_dispatches() {
    assert_accepted(0.001);
}

#[test]
fn upper_weight_bound_dispatches() {
    assert_accepted(100.0);
}

#[test]
fn weight_below_lower_bound_does_not_dispatch() {
    assert_rejected(0.0009);
}

#[test]
fn weight_above_upper_bound_does_not_dispatch() {
    assert_rejected(100.0001);
}

#[test]
fn nan_weight_does_not_dispatch() {
    assert_rejected(f64::NAN);
}

#[test]
fn non_rational_segments_dispatch() {
    assert_accepted(-f64::MAX);
}
