//! `smlib_brep_check` (the fork's `capi/src/smlib_check.cpp`), declared by hand.
//! Part of the FFI boundary, like its parent: called only through [`crate::check`].

#![allow(unsafe_code)]

use super::Handle;
#[cfg(not(feature = "native"))]
use super::NOT_BUILT;
#[cfg(feature = "native")]
use super::{OK, RawBrep};
#[cfg(feature = "native")]
use std::ffi::c_char;

#[cfg(feature = "native")]
unsafe extern "C" {
    pub fn smlib_brep_check(brep: *const RawBrep, level: i32, buf: *mut c_char, cap: i64, out_len: *mut i64, out_failures: *mut i64) -> i32;
}

/// Runs the check into `buf`: the report's length and the failure count, or a status
/// (`ERR_BUFFER` with the needed length when `buf` is too small).
#[cfg(feature = "native")]
pub fn check(b: &Handle, level: i32, buf: &mut [u8]) -> Result<(usize, i64), (i32, usize)> {
    let cap = i64::try_from(buf.len()).unwrap_or(i64::MAX);
    let (mut len, mut failures) = (0i64, 0i64);
    let s = unsafe { smlib_brep_check(b.raw(), level, buf.as_mut_ptr().cast(), cap, &mut len, &mut failures) };
    let len = usize::try_from(len).unwrap_or(0);
    match s {
        OK => Ok((len.min(buf.len()), failures)),
        s => Err((s, len)),
    }
}

#[cfg(not(feature = "native"))]
pub fn check(_: &Handle, _: i32, _: &mut [u8]) -> Result<(usize, i64), (i32, usize)> {
    Err((NOT_BUILT, 0))
}
