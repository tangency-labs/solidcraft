//! The operations block of `capi/include/smlib.h` (`smlib_boolean`, `smlib_fillet`,
//! `smlib_chamfer`, `smlib_shell`, `smlib_brep_solid_count`; the fork's `capi/src/smlib_ops.cpp`),
//! declared by hand. Part of the FFI boundary, like its parent: called only through
//! [`crate::ops`].

#![allow(unsafe_code)]

use super::Handle;
#[cfg(not(feature = "native"))]
use super::NOT_BUILT;
#[cfg(feature = "native")]
use super::{ERR_ARG, OK, RawBrep, new_brep};

pub const ERR_NOT_SOLID: i32 = -1100;
pub const ERR_PICK_NONE: i32 = -1101;
pub const ERR_PICK_AMBIGUOUS: i32 = -1102;

pub const BOOLEAN_UNION: i32 = 0;
pub const BOOLEAN_SUBTRACT: i32 = 1;
pub const BOOLEAN_INTERSECT: i32 = 2;

/// Which selection operation: the C functions share one signature.
#[derive(Clone, Copy)]
pub enum Pick {
    Fillet,
    Chamfer,
    Shell,
}

/// A failed selection operation: the status, and the index of the point that selected nothing
/// or several things (only for `ERR_PICK_*`).
pub type PickError = (i32, Option<usize>);

#[cfg(feature = "native")]
type PickFn = unsafe extern "C" fn(*const RawBrep, *const f64, i64, f64, f64, *mut *mut RawBrep, *mut i64) -> i32;

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_boolean(a: *const RawBrep, b: *const RawBrep, op: i32, out: *mut *mut RawBrep) -> i32;
    fn smlib_brep_solid_count(brep: *const RawBrep, out: *mut i64) -> i32;
    fn smlib_fillet(
        brep: *const RawBrep,
        points: *const f64,
        n_points: i64,
        tolerance: f64,
        radius: f64,
        out: *mut *mut RawBrep,
        out_failed_point: *mut i64,
    ) -> i32;
    fn smlib_chamfer(
        brep: *const RawBrep,
        points: *const f64,
        n_points: i64,
        tolerance: f64,
        distance: f64,
        out: *mut *mut RawBrep,
        out_failed_point: *mut i64,
    ) -> i32;
    fn smlib_shell(
        brep: *const RawBrep,
        points: *const f64,
        n_points: i64,
        tolerance: f64,
        thickness: f64,
        out: *mut *mut RawBrep,
        out_failed_point: *mut i64,
    ) -> i32;
}

#[cfg(feature = "native")]
pub fn boolean(a: &Handle, b: &Handle, op: i32) -> Result<Handle, i32> {
    new_brep(|out| unsafe { smlib_boolean(a.raw(), b.raw(), op, out) })
}

#[cfg(feature = "native")]
pub fn solid_count(b: &Handle) -> Result<i64, i32> {
    let mut n = 0i64;
    match unsafe { smlib_brep_solid_count(b.raw(), &mut n) } {
        OK => Ok(n),
        s => Err(s),
    }
}

/// Runs a selection operation; `points` are passed as the flat x, y, z array the C API reads.
#[cfg(feature = "native")]
pub fn pick_op(kind: Pick, b: &Handle, points: &[[f64; 3]], tolerance: f64, size: f64) -> Result<Handle, PickError> {
    let n = i64::try_from(points.len()).map_err(|_| (ERR_ARG, None))?;
    let f: PickFn = match kind {
        Pick::Fillet => smlib_fillet,
        Pick::Chamfer => smlib_chamfer,
        Pick::Shell => smlib_shell,
    };
    let flat = points.as_ptr().cast::<f64>();
    let mut failed = -1i64;
    new_brep(|out| unsafe { f(b.raw(), flat, n, tolerance, size, out, &mut failed) }).map_err(|s| {
        let at = (s == ERR_PICK_NONE || s == ERR_PICK_AMBIGUOUS).then(|| usize::try_from(failed).ok()).flatten();
        (s, at)
    })
}

#[cfg(not(feature = "native"))]
pub fn boolean(_: &Handle, _: &Handle, _: i32) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn solid_count(_: &Handle) -> Result<i64, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn pick_op(_: Pick, _: &Handle, _: &[[f64; 3]], _: f64, _: f64) -> Result<Handle, PickError> {
    Err((NOT_BUILT, None))
}
