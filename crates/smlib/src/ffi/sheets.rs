//! The sheets block of `capi/include/smlib.h` (`smlib_stitch`, `smlib_offset_sheet`; the fork's
//! `capi/src/smlib_sheets.cpp`), declared by hand. Part of the FFI boundary, like its parent:
//! called only through [`crate::sheets`].

#![allow(unsafe_code)]

#[cfg(not(feature = "solidcore"))]
use super::Handle;
#[cfg(not(any(feature = "native", feature = "solidcore")))]
use super::NOT_BUILT;
#[cfg(feature = "native")]
use super::{RawBrep, new_brep};

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_stitch(brep: *const RawBrep, tolerance: f64, out_closed: *mut i32, out: *mut *mut RawBrep) -> i32;
    fn smlib_offset_sheet(sheet: *const RawBrep, distance: f64, out: *mut *mut RawBrep) -> i32;
}

/// The stitched body and whether it closed into a solid.
#[cfg(feature = "native")]
pub fn stitch(b: &Handle, tolerance: f64) -> Result<(Handle, bool), i32> {
    let mut closed = 0i32;
    let h = new_brep(|out| unsafe { smlib_stitch(b.raw(), tolerance, &mut closed, out) })?;
    Ok((h, closed != 0))
}

#[cfg(feature = "native")]
pub fn offset_sheet(b: &Handle, distance: f64) -> Result<Handle, i32> {
    new_brep(|out| unsafe { smlib_offset_sheet(b.raw(), distance, out) })
}

#[cfg(not(any(feature = "native", feature = "solidcore")))]
pub fn stitch(_: &Handle, _: f64) -> Result<(Handle, bool), i32> {
    Err(NOT_BUILT)
}

#[cfg(not(any(feature = "native", feature = "solidcore")))]
pub fn offset_sheet(_: &Handle, _: f64) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(feature = "solidcore")]
pub use super::rs_shell::{offset_sheet, stitch};
