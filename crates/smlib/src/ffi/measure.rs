//! `smlib_brep_mass_properties` and `smlib_brep_area` (the fork's `capi/src/smlib_measure.cpp`), declared by
//! hand. Part of the FFI boundary, like its parent: called only through [`crate::measure`].

#![allow(unsafe_code)]

use super::Handle;
#[cfg(not(feature = "native"))]
use super::NOT_BUILT;
#[cfg(feature = "native")]
use super::{OK, RawBrep};

#[cfg(feature = "native")]
unsafe extern "C" {
    pub fn smlib_brep_mass_properties(brep: *const RawBrep, relative_accuracy: f64, out: *mut f64) -> i32;
    pub fn smlib_brep_area(brep: *const RawBrep, relative_accuracy: f64, out: *mut f64) -> i32;
}

#[cfg(feature = "native")]
pub fn mass_properties(b: &Handle, relative_accuracy: f64) -> Result<[f64; 11], i32> {
    let mut m = [0.0; 11];
    match unsafe { smlib_brep_mass_properties(b.raw(), relative_accuracy, m.as_mut_ptr()) } {
        OK => Ok(m),
        s => Err(s),
    }
}

#[cfg(feature = "native")]
pub fn area(b: &Handle, relative_accuracy: f64) -> Result<f64, i32> {
    let mut a = 0.0;
    match unsafe { smlib_brep_area(b.raw(), relative_accuracy, &mut a) } {
        OK => Ok(a),
        s => Err(s),
    }
}

#[cfg(not(feature = "native"))]
pub fn mass_properties(_: &Handle, _: f64) -> Result<[f64; 11], i32> {
    Err(NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn area(_: &Handle, _: f64) -> Result<f64, i32> {
    Err(NOT_BUILT)
}
