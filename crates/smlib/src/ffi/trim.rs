//! The untrim and extend blocks of `capi/include/smlib.h` (`smlib_untrim_face`,
//! `smlib_extend_sheet`; the fork's `capi/src/smlib_untrim.cpp` and `smlib_extend.cpp`) and
//! `smlib_face_points` (`smlib_imprint.cpp`, the pieces a trim chooses from), declared by hand. Part of the FFI boundary, like its parent: called only through [`crate::trim`].

#![allow(unsafe_code)]

use super::Handle;
#[cfg(not(feature = "native"))]
use super::NOT_BUILT;
#[cfg(feature = "native")]
use super::{RawBrep, new_brep};

/// The answer is not exact in the input's geometry (`SMLIB_ERR_NOT_EXACT`).
pub const ERR_NOT_EXACT: i32 = -1106;

pub const EXTEND_NATURAL: i32 = 0;
pub const EXTEND_TANGENT: i32 = 1;
pub const EXTEND_CURVATURE: i32 = 2;

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
    fn smlib_face_points(body: *const RawBrep, out: *mut f64, n_faces: i64) -> i32;
    fn smlib_extend_sheet(sheet: *const RawBrep, point: *const f64, tol: f64, distance: f64, mode: i32, out: *mut *mut RawBrep) -> i32;
}

#[cfg(feature = "native")]
pub fn extend_sheet(sheet: &Handle, point: &[f64; 3], tol: f64, distance: f64, mode: i32) -> Result<Handle, i32> {
    new_brep(|out| unsafe { smlib_extend_sheet(sheet.raw(), point.as_ptr(), tol, distance, mode, out) })
}

#[cfg(not(feature = "native"))]
pub fn extend_sheet(_: &Handle, _: &[f64; 3], _: f64, _: f64, _: i32) -> Result<Handle, i32> {
    Err(NOT_BUILT)
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

#[cfg(feature = "native")]
pub fn face_points(body: &Handle, n_faces: usize) -> Result<Vec<[f64; 3]>, i32> {
    let n = i64::try_from(n_faces).map_err(|_| super::ERR_ARG)?;
    let mut out = vec![[0.0f64; 3]; n_faces];
    let s = unsafe { smlib_face_points(body.raw(), out.as_mut_ptr().cast::<f64>(), n) };
    if s == super::OK { Ok(out) } else { Err(s) }
}

#[cfg(not(feature = "native"))]
pub fn face_points(_: &Handle, _: usize) -> Result<Vec<[f64; 3]>, i32> {
    Err(NOT_BUILT)
}
