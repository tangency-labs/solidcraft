//! The split block of `capi/include/smlib.h` (`smlib_split_faces`, `smlib_split_body`,
//! `smlib_split_body_by_sheet`; the
//! fork's `capi/src/smlib_split.cpp`), declared by hand. Part of the FFI boundary, like its
//! parent: called only through [`crate::split`].

#![allow(unsafe_code)]

use super::Handle;
#[cfg(not(feature = "native"))]
use super::NOT_BUILT;
use super::ssi::SurfaceHandle;
#[cfg(feature = "native")]
use super::{ERR_ARG, ERR_EMPTY, OK, RawBrep, new_brep, ssi::RawSurface};

/// `SMLIB_SPLIT_MAX_FACES`, `SMLIB_SPLIT_MAX_TOOLS`.
pub const MAX_FACES: usize = 100_000;
pub const MAX_TOOLS: usize = 4096;

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_split_faces(
        body: *const RawBrep,
        faces: *const i64,
        n_faces: i64,
        tools: *const *const RawSurface,
        n_tools: i64,
        out: *mut *mut RawBrep,
        made: *mut i64,
    ) -> i32;
    fn smlib_split_body(body: *const RawBrep, tool: *const RawSurface, back: *mut *mut RawBrep, front: *mut *mut RawBrep) -> i32;
    fn smlib_split_body_by_sheet(body: *const RawBrep, sheet: *const RawBrep, face: i64, back: *mut *mut RawBrep, front: *mut *mut RawBrep) -> i32;
}

/// `body` with `faces` cut by `tools` in turn, and per face how many faces it became besides
/// itself.
#[cfg(feature = "native")]
pub fn split_faces(body: &Handle, faces: &[i64], tools: &[&SurfaceHandle]) -> Result<(Handle, Vec<i64>), i32> {
    if faces.is_empty() || faces.len() > MAX_FACES || tools.is_empty() || tools.len() > MAX_TOOLS {
        return Err(ERR_ARG);
    }
    let raw: Vec<*const RawSurface> = tools.iter().map(|t| t.raw()).collect();
    let mut made = vec![0i64; faces.len()];
    let n = i64::try_from(faces.len()).map_err(|_| ERR_ARG)?;
    let m = i64::try_from(raw.len()).map_err(|_| ERR_ARG)?;
    // SAFETY: the arrays outlive the call and hold `n` and `m` entries; the handles are live.
    let h = new_brep(|out| unsafe { smlib_split_faces(body.raw(), faces.as_ptr(), n, raw.as_ptr(), m, out, made.as_mut_ptr()) })?;
    Ok((h, made))
}

/// The parts of `body` behind and in front of `tool`.
#[cfg(feature = "native")]
pub fn split_body(body: &Handle, tool: &SurfaceHandle) -> Result<(Handle, Handle), i32> {
    // SAFETY: the handles are live; the out-pointers are written only on success.
    two_parts(|back, front| unsafe { smlib_split_body(body.raw(), tool.raw(), back, front) })
}

/// The parts of `body` behind and in front of `sheet`, its sides those of its face `face`.
#[cfg(feature = "native")]
pub fn split_body_by_sheet(body: &Handle, sheet: &Handle, face: i64) -> Result<(Handle, Handle), i32> {
    // SAFETY: the handles are live; the out-pointers are written only on success.
    two_parts(|back, front| unsafe { smlib_split_body_by_sheet(body.raw(), sheet.raw(), face, back, front) })
}

/// The two bodies a call writing a back and a front part gives, owned.
#[cfg(feature = "native")]
fn two_parts(call: impl FnOnce(*mut *mut RawBrep, *mut *mut RawBrep) -> i32) -> Result<(Handle, Handle), i32> {
    let mut back: *mut RawBrep = std::ptr::null_mut();
    let mut front: *mut RawBrep = std::ptr::null_mut();
    let s = call(&mut back, &mut front);
    let (b, f) = (std::ptr::NonNull::new(back), std::ptr::NonNull::new(front));
    match (s, b, f) {
        (OK, Some(b), Some(f)) => Ok((Handle(b), Handle(f))),
        (OK, b, f) => {
            // Free whichever came back alone.
            drop(b.map(Handle));
            drop(f.map(Handle));
            Err(ERR_EMPTY)
        }
        (s, _, _) => Err(s),
    }
}

#[cfg(not(feature = "native"))]
pub fn split_faces(_: &Handle, _: &[i64], _: &[&SurfaceHandle]) -> Result<(Handle, Vec<i64>), i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn split_body(_: &Handle, _: &SurfaceHandle) -> Result<(Handle, Handle), i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn split_body_by_sheet(_: &Handle, _: &Handle, _: i64) -> Result<(Handle, Handle), i32> {
    Err(NOT_BUILT)
}
