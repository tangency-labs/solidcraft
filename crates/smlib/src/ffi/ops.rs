//! The operations block of `capi/include/smlib.h` (`smlib_boolean`, `smlib_fillet`,
//! `smlib_chamfer`, `smlib_blend`, `smlib_fillet_variable`, `smlib_chamfer_sides`, `smlib_shell`, `smlib_offset_faces`,
//! `smlib_thicken`, `smlib_draft`, `smlib_merge_faces`, `smlib_brep_solid_count`, `smlib_imprint`; the fork's `capi/src/smlib_ops.cpp`), declared by
//! hand. Part of the FFI boundary, like its parent: called only through
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
pub const ERR_NO_ROOM: i32 = -1103;
pub const ERR_NO_CUT: i32 = -1104;
pub const ERR_UNSUPPORTED: i32 = -1105;
pub const ERR_NO_EFFECT: i32 = -1107;

pub const BOOLEAN_UNION: i32 = 0;
pub const BOOLEAN_SUBTRACT: i32 = 1;
pub const BOOLEAN_INTERSECT: i32 = 2;

pub const BLEND_ROUND: i32 = 0;
pub const BLEND_CURVATURE: i32 = 1;
pub const BLEND_CHORD: i32 = 2;
pub const BLEND_CHAMFER: i32 = 3;

/// Which selection operation: the C functions share one signature.
#[derive(Clone, Copy)]
pub enum Pick {
    Fillet,
    Chamfer,
    Shell,
    OffsetFaces,
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
    fn smlib_imprint(body: *const RawBrep, tool: *const RawBrep, out: *mut *mut RawBrep) -> i32;
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
    fn smlib_blend(
        brep: *const RawBrep,
        kind: i32,
        points: *const f64,
        sizes: *const f64,
        n_points: i64,
        tolerance: f64,
        out: *mut *mut RawBrep,
        out_failed_point: *mut i64,
    ) -> i32;
    fn smlib_fillet_variable(
        brep: *const RawBrep,
        point: *const f64,
        tolerance: f64,
        start: *const f64,
        degree: i32,
        knots: *const f64,
        n_knots: i64,
        radii: *const f64,
        n_radii: i64,
        out: *mut *mut RawBrep,
    ) -> i32;
    fn smlib_chamfer_sides(
        brep: *const RawBrep,
        points: *const f64,
        n_points: i64,
        tolerance: f64,
        distance: f64,
        second: f64,
        second_is_angle: i32,
        first: *const f64,
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
    fn smlib_offset_faces(
        brep: *const RawBrep,
        points: *const f64,
        n_points: i64,
        tolerance: f64,
        distance: f64,
        out: *mut *mut RawBrep,
        out_failed_point: *mut i64,
    ) -> i32;
    fn smlib_thicken(brep: *const RawBrep, thickness: f64, symmetric: i32, out: *mut *mut RawBrep) -> i32;
    fn smlib_merge_faces(
        brep: *const RawBrep,
        keep: *const f64,
        n_keep: i64,
        tolerance: f64,
        out: *mut *mut RawBrep,
        out_failed_point: *mut i64,
    ) -> i32;
    fn smlib_draft(
        brep: *const RawBrep,
        points: *const f64,
        n_points: i64,
        tolerance: f64,
        neutral_origin: *const f64,
        neutral_normal: *const f64,
        pull: *const f64,
        angle: f64,
        out: *mut *mut RawBrep,
        out_failed_point: *mut i64,
    ) -> i32;
}

#[cfg(feature = "native")]
pub fn boolean(a: &Handle, b: &Handle, op: i32) -> Result<Handle, i32> {
    new_brep(|out| unsafe { smlib_boolean(a.raw(), b.raw(), op, out) })
}

#[cfg(feature = "native")]
pub fn imprint(body: &Handle, tool: &Handle) -> Result<Handle, i32> {
    new_brep(|out| unsafe { smlib_imprint(body.raw(), tool.raw(), out) })
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
        Pick::OffsetFaces => smlib_offset_faces,
    };
    let flat = points.as_ptr().cast::<f64>();
    let mut failed = -1i64;
    new_brep(|out| unsafe { f(b.raw(), flat, n, tolerance, size, out, &mut failed) }).map_err(|s| {
        let at = (s == ERR_PICK_NONE || s == ERR_PICK_AMBIGUOUS).then(|| usize::try_from(failed).ok()).flatten();
        (s, at)
    })
}

#[cfg(feature = "native")]
pub fn thicken(b: &Handle, thickness: f64, symmetric: bool) -> Result<Handle, i32> {
    new_brep(|out| unsafe { smlib_thicken(b.raw(), thickness, i32::from(symmetric), out) })
}

#[cfg(not(feature = "native"))]
pub fn thicken(_: &Handle, _: f64, _: bool) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

/// `smlib_draft`: the faces under `points` tilted by `angle` radians about their lines on the
/// neutral plane (`origin`, `normal`), leaning in along `pull`.
#[cfg(feature = "native")]
pub fn draft(
    b: &Handle,
    points: &[[f64; 3]],
    tolerance: f64,
    origin: &[f64; 3],
    normal: &[f64; 3],
    pull: &[f64; 3],
    angle: f64,
) -> Result<Handle, PickError> {
    let n = i64::try_from(points.len()).map_err(|_| (ERR_ARG, None))?;
    let flat = points.as_ptr().cast::<f64>();
    let mut failed = -1i64;
    new_brep(|out| unsafe { smlib_draft(b.raw(), flat, n, tolerance, origin.as_ptr(), normal.as_ptr(), pull.as_ptr(), angle, out, &mut failed) })
        .map_err(|s| {
            let at = (s == ERR_PICK_NONE || s == ERR_PICK_AMBIGUOUS).then(|| usize::try_from(failed).ok()).flatten();
            (s, at)
        })
}

#[cfg(not(feature = "native"))]
pub fn draft(_: &Handle, _: &[[f64; 3]], _: f64, _: &[f64; 3], _: &[f64; 3], _: &[f64; 3], _: f64) -> Result<Handle, PickError> {
    Err((NOT_BUILT, None))
}

/// `smlib_merge_faces`: the body's topological edges and vertices removed, the edges under
/// `keep` kept.
#[cfg(feature = "native")]
pub fn merge_faces(b: &Handle, keep: &[[f64; 3]], tolerance: f64) -> Result<Handle, PickError> {
    let n = i64::try_from(keep.len()).map_err(|_| (ERR_ARG, None))?;
    let flat = keep.as_ptr().cast::<f64>();
    let mut failed = -1i64;
    new_brep(|out| unsafe { smlib_merge_faces(b.raw(), flat, n, tolerance, out, &mut failed) }).map_err(|s| {
        let at = (s == ERR_PICK_NONE || s == ERR_PICK_AMBIGUOUS).then(|| usize::try_from(failed).ok()).flatten();
        (s, at)
    })
}

#[cfg(not(feature = "native"))]
pub fn merge_faces(_: &Handle, _: &[[f64; 3]], _: f64) -> Result<Handle, PickError> {
    Err((NOT_BUILT, None))
}

/// `smlib_blend`: `sizes` holds one size per point (the caller checks the lengths match).
#[cfg(feature = "native")]
pub fn blend(kind: i32, b: &Handle, points: &[[f64; 3]], sizes: &[f64], tolerance: f64) -> Result<Handle, PickError> {
    let n = i64::try_from(points.len()).map_err(|_| (ERR_ARG, None))?;
    if sizes.len() != points.len() {
        return Err((ERR_ARG, None));
    }
    let flat = points.as_ptr().cast::<f64>();
    let mut failed = -1i64;
    new_brep(|out| unsafe { smlib_blend(b.raw(), kind, flat, sizes.as_ptr(), n, tolerance, out, &mut failed) }).map_err(|s| {
        let at = (s == ERR_PICK_NONE || s == ERR_PICK_AMBIGUOUS).then(|| usize::try_from(failed).ok()).flatten();
        (s, at)
    })
}

/// `smlib_fillet_variable`.
#[cfg(feature = "native")]
pub fn fillet_variable(
    b: &Handle,
    point: &[f64; 3],
    tolerance: f64,
    start: &[f64; 3],
    degree: i32,
    knots: &[f64],
    radii: &[f64],
) -> Result<Handle, i32> {
    let nk = i64::try_from(knots.len()).map_err(|_| ERR_ARG)?;
    let nr = i64::try_from(radii.len()).map_err(|_| ERR_ARG)?;
    new_brep(|out| unsafe {
        smlib_fillet_variable(b.raw(), point.as_ptr(), tolerance, start.as_ptr(), degree, knots.as_ptr(), nk, radii.as_ptr(), nr, out)
    })
}

/// `smlib_chamfer_sides`.
#[cfg(feature = "native")]
pub fn chamfer_sides(
    b: &Handle,
    points: &[[f64; 3]],
    tolerance: f64,
    distance: f64,
    second: f64,
    angle: bool,
    first: &[f64; 3],
) -> Result<Handle, PickError> {
    let n = i64::try_from(points.len()).map_err(|_| (ERR_ARG, None))?;
    let flat = points.as_ptr().cast::<f64>();
    let mut failed = -1i64;
    new_brep(|out| unsafe { smlib_chamfer_sides(b.raw(), flat, n, tolerance, distance, second, i32::from(angle), first.as_ptr(), out, &mut failed) })
        .map_err(|s| {
            let at = (s == ERR_PICK_NONE || s == ERR_PICK_AMBIGUOUS).then(|| usize::try_from(failed).ok()).flatten();
            (s, at)
        })
}

#[cfg(not(feature = "native"))]
pub fn chamfer_sides(_: &Handle, _: &[[f64; 3]], _: f64, _: f64, _: f64, _: bool, _: &[f64; 3]) -> Result<Handle, PickError> {
    Err((NOT_BUILT, None))
}

#[cfg(not(feature = "native"))]
pub fn blend(_: i32, _: &Handle, _: &[[f64; 3]], _: &[f64], _: f64) -> Result<Handle, PickError> {
    Err((NOT_BUILT, None))
}

#[cfg(not(feature = "native"))]
pub fn fillet_variable(_: &Handle, _: &[f64; 3], _: f64, _: &[f64; 3], _: i32, _: &[f64], _: &[f64]) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn boolean(_: &Handle, _: &Handle, _: i32) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn imprint(_: &Handle, _: &Handle) -> Result<Handle, i32> {
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
