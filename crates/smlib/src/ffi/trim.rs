//! The untrim block of `capi/include/smlib.h` (`smlib_untrim_face`; the fork's
//! `capi/src/smlib_untrim.cpp`), declared by hand. Part of the FFI boundary, like its parent: called only through [`crate::trim`].

#![allow(unsafe_code)]

use super::Handle;
#[cfg(not(feature = "native"))]
use super::NOT_BUILT;
#[cfg(feature = "native")]
use super::{RawBrep, new_brep};

/// The answer is not exact in the input's geometry (`SMLIB_ERR_NOT_EXACT`).
pub const ERR_NOT_EXACT: i32 = -1106;

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_untrim_face(
        body: *const RawBrep,
        point: *const f64,
        tol: f64,
        holes: *const f64,
        n_holes: i64,
        outer: i32,
        in_place: i32,
        out: *mut *mut RawBrep,
    ) -> i32;
}

/// `holes`: `None` for every hole.
#[cfg(feature = "native")]
pub fn untrim_face(body: &Handle, point: &[f64; 3], tol: f64, holes: Option<&[[f64; 3]]>, outer: bool, in_place: bool) -> Result<Handle, i32> {
    let (ptr, n) = match holes {
        Some(h) => (h.as_ptr().cast::<f64>(), i64::try_from(h.len()).map_err(|_| super::ERR_ARG)?),
        None => (std::ptr::null(), -1),
    };
    new_brep(|out| unsafe { smlib_untrim_face(body.raw(), point.as_ptr(), tol, ptr, n, i32::from(outer), i32::from(in_place), out) })
}

#[cfg(not(feature = "native"))]
pub fn untrim_face(_: &Handle, _: &[f64; 3], _: f64, _: Option<&[[f64; 3]]>, _: bool, _: bool) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}
