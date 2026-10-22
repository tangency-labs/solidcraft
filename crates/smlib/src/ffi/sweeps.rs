//! The sweeps block of `capi/include/smlib.h` (`smlib_extrude`, `smlib_revolve`, `smlib_sweep`,
//! `smlib_sweep_placed`, `smlib_loft`, `smlib_loft_to_point`, `smlib_screw_sweep`, `smlib_sweep_stations`, `smlib_planar_faces`; the fork's
//! `capi/src/smlib_sweeps.cpp`), declared by hand.
//! Part of the FFI boundary, like its parent: called only through [`crate::sweeps`], which builds
//! the curve lists.

#![allow(unsafe_code)]

use super::Handle;
#[cfg(not(feature = "native"))]
use super::NOT_BUILT;
#[cfg(feature = "native")]
use super::{ERR_ARG, RawBrep, new_brep};

pub const CURVE_LINE: i64 = 1;
pub const CURVE_ARC: i64 = 2;
pub const CURVE_NURBS: i64 = 3;
pub const CURVE_MAX_CURVES: usize = 4096;
pub const CURVE_MAX_DEGREE: usize = 25;
pub const CURVE_MAX_POINTS: usize = 100_000;

pub const SWEEP_FOLLOW: i32 = 0;
pub const SWEEP_TRANSLATE: i32 = 1;

pub const ENDS_OPEN: i32 = 0;
pub const ENDS_CAPPED: i32 = 1;
pub const ENDS_JOINED: i32 = 2;
pub const SWEEP_MAX_STATIONS: usize = 4096;
pub const SWEEP_MAX_STATION_CURVES: usize = 400_000;

/// A curve list as the C API reads it: kinds (with a NURBS curve's degree and point count) and
/// each curve's numbers.
#[cfg(feature = "native")]
pub struct List<'a> {
    kinds: &'a [i64],
    reals: &'a [f64],
}

/// Without SMLib nothing reads the list, so it carries nothing.
#[cfg(not(feature = "native"))]
pub struct List<'a>(std::marker::PhantomData<&'a ()>);

impl<'a> List<'a> {
    #[cfg(feature = "native")]
    pub fn new(kinds: &'a [i64], reals: &'a [f64]) -> Self {
        List { kinds, reals }
    }

    #[cfg(not(feature = "native"))]
    pub fn new(_: &'a [i64], _: &'a [f64]) -> Self {
        List(std::marker::PhantomData)
    }
}

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_planar_faces(kinds: *const i64, n_kinds: i64, reals: *const f64, n_reals: i64, out: *mut *mut RawBrep) -> i32;
    fn smlib_extrude(
        kinds: *const i64,
        n_kinds: i64,
        reals: *const f64,
        n_reals: i64,
        direction: *const f64,
        distance: f64,
        draft: f64,
        cap: i32,
        out: *mut *mut RawBrep,
    ) -> i32;
    fn smlib_revolve(
        kinds: *const i64,
        n_kinds: i64,
        reals: *const f64,
        n_reals: i64,
        origin: *const f64,
        direction: *const f64,
        angle: f64,
        cap: i32,
        out: *mut *mut RawBrep,
    ) -> i32;
    fn smlib_sweep(
        kinds: *const i64,
        n_kinds: i64,
        reals: *const f64,
        n_reals: i64,
        path_kinds: *const i64,
        n_path_kinds: i64,
        path_reals: *const f64,
        n_path_reals: i64,
        frame: i32,
        cap: i32,
        out: *mut *mut RawBrep,
    ) -> i32;
    fn smlib_sweep_placed(
        kinds: *const i64,
        n_kinds: i64,
        reals: *const f64,
        n_reals: i64,
        path_kinds: *const i64,
        n_path_kinds: i64,
        path_reals: *const f64,
        n_path_reals: i64,
        frame: i32,
        cap: i32,
        out: *mut *mut RawBrep,
        placed: *mut f64,
    ) -> i32;
    fn smlib_loft(
        kinds: *const i64,
        n_kinds: i64,
        reals: *const f64,
        n_reals: i64,
        section_sizes: *const i64,
        n_sections: i64,
        degree: i32,
        cap: i32,
        out: *mut *mut RawBrep,
    ) -> i32;
    fn smlib_loft_to_point(kinds: *const i64, n_kinds: i64, reals: *const f64, n_reals: i64, apex: *const f64, out: *mut *mut RawBrep) -> i32;
    fn smlib_screw_sweep(
        kinds: *const i64,
        n_kinds: i64,
        reals: *const f64,
        n_reals: i64,
        origin: *const f64,
        direction: *const f64,
        pitch: f64,
        turns: f64,
        tolerance: f64,
        out: *mut *mut RawBrep,
    ) -> i32;
    fn smlib_sweep_stations(
        kinds: *const i64,
        n_kinds: i64,
        reals: *const f64,
        n_reals: i64,
        maps: *const f64,
        n_stations: i64,
        ends: i32,
        out: *mut *mut RawBrep,
    ) -> i32;
}

/// The list's arrays and lengths as the C API takes them.
#[cfg(feature = "native")]
fn raw(l: &List) -> Result<(*const i64, i64, *const f64, i64), i32> {
    let nk = i64::try_from(l.kinds.len()).map_err(|_| ERR_ARG)?;
    let nr = i64::try_from(l.reals.len()).map_err(|_| ERR_ARG)?;
    Ok((l.kinds.as_ptr(), nk, l.reals.as_ptr(), nr))
}

#[cfg(feature = "native")]
pub fn planar_faces(loops: &List) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(loops)?;
    new_brep(|out| unsafe { smlib_planar_faces(k, nk, r, nr, out) })
}

#[cfg(feature = "native")]
pub fn extrude(profile: &List, direction: &[f64; 3], distance: f64, draft: f64, cap: bool) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(profile)?;
    new_brep(|out| unsafe { smlib_extrude(k, nk, r, nr, direction.as_ptr(), distance, draft, i32::from(cap), out) })
}

#[cfg(feature = "native")]
pub fn revolve(profile: &List, origin: &[f64; 3], direction: &[f64; 3], angle: f64, cap: bool) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(profile)?;
    new_brep(|out| unsafe { smlib_revolve(k, nk, r, nr, origin.as_ptr(), direction.as_ptr(), angle, i32::from(cap), out) })
}

#[cfg(feature = "native")]
pub fn sweep(profile: &List, path: &List, frame: i32, cap: bool) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(profile)?;
    let (pk, npk, pr, npr) = raw(path)?;
    new_brep(|out| unsafe { smlib_sweep(k, nk, r, nr, pk, npk, pr, npr, frame, i32::from(cap), out) })
}

/// [`sweep`], with the three rigid motions placing the profile (12 numbers each: a matrix by
/// columns, then a translation) written to `placed`.
#[cfg(feature = "native")]
pub fn sweep_placed(profile: &List, path: &List, frame: i32, cap: bool, placed: &mut [f64; 36]) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(profile)?;
    let (pk, npk, pr, npr) = raw(path)?;
    let at = placed.as_mut_ptr();
    new_brep(|out| unsafe { smlib_sweep_placed(k, nk, r, nr, pk, npk, pr, npr, frame, i32::from(cap), out, at) })
}

#[cfg(feature = "native")]
pub fn loft(sections: &List, sizes: &[i64], degree: i32, cap: bool) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(sections)?;
    let ns = i64::try_from(sizes.len()).map_err(|_| ERR_ARG)?;
    new_brep(|out| unsafe { smlib_loft(k, nk, r, nr, sizes.as_ptr(), ns, degree, i32::from(cap), out) })
}

#[cfg(feature = "native")]
pub fn loft_to_point(section: &List, apex: &[f64; 3]) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(section)?;
    new_brep(|out| unsafe { smlib_loft_to_point(k, nk, r, nr, apex.as_ptr(), out) })
}

#[cfg(feature = "native")]
pub fn screw_sweep(profile: &List, origin: &[f64; 3], direction: &[f64; 3], pitch: f64, turns: f64, tolerance: f64) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(profile)?;
    new_brep(|out| unsafe { smlib_screw_sweep(k, nk, r, nr, origin.as_ptr(), direction.as_ptr(), pitch, turns, tolerance, out) })
}

/// `maps` holds 12 numbers per station (a matrix by columns, then a translation).
#[cfg(feature = "native")]
pub fn sweep_stations(profile: &List, maps: &[f64], ends: i32) -> Result<Handle, i32> {
    let (k, nk, r, nr) = raw(profile)?;
    if !maps.len().is_multiple_of(12) {
        return Err(ERR_ARG);
    }
    let n = i64::try_from(maps.len() / 12).map_err(|_| ERR_ARG)?;
    new_brep(|out| unsafe { smlib_sweep_stations(k, nk, r, nr, maps.as_ptr(), n, ends, out) })
}

#[cfg(not(feature = "native"))]
pub fn planar_faces(_: &List) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn extrude(_: &List, _: &[f64; 3], _: f64, _: f64, _: bool) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn revolve(_: &List, _: &[f64; 3], _: &[f64; 3], _: f64, _: bool) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn sweep(_: &List, _: &List, _: i32, _: bool) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn sweep_placed(_: &List, _: &List, _: i32, _: bool, _: &mut [f64; 36]) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn loft(_: &List, _: &[i64], _: i32, _: bool) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn loft_to_point(_: &List, _: &[f64; 3]) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn screw_sweep(_: &List, _: &[f64; 3], _: &[f64; 3], _: f64, _: f64, _: f64) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn sweep_stations(_: &List, _: &[f64], _: i32) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}
