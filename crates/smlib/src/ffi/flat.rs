//! The flat-array block of `capi/include/smlib.h`, declared by hand. Part of the FFI boundary,
//! like its parent: called only through [`crate::flat`].
#![allow(unsafe_code)]

#[cfg(not(feature = "solidcore"))]
use super::Handle;
#[cfg(feature = "native")]
use super::{OK, RawBrep, new_brep};

/// `smlib_flat`, opaque.
#[cfg_attr(feature = "solidcore", allow(dead_code))]
#[repr(C)]
pub struct RawFlat {
    _private: [u8; 0],
}

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_flat_new() -> *mut RawFlat;
    fn smlib_flat_free(flat: *mut RawFlat);
    fn smlib_brep_export(brep: *const RawBrep, out: *mut *mut RawFlat) -> i32;
    fn smlib_brep_import(flat: *const RawFlat, out: *mut *mut RawBrep) -> i32;
    fn smlib_flat_get_i64(flat: *const RawFlat, id: i32, data: *mut *const i64, len: *mut i64) -> i32;
    fn smlib_flat_get_f64(flat: *const RawFlat, id: i32, data: *mut *const f64, len: *mut i64) -> i32;
    fn smlib_flat_set_i64(flat: *mut RawFlat, id: i32, data: *const i64, len: i64) -> i32;
    fn smlib_flat_set_f64(flat: *mut RawFlat, id: i32, data: *const f64, len: i64) -> i32;
    fn smlib_brep_is_manifold_solid(brep: *const RawBrep, out: *mut i32) -> i32;
}

/// An owned `smlib_flat`, freed once on drop.
#[cfg(not(feature = "solidcore"))]
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub struct FlatHandle(std::ptr::NonNull<RawFlat>);

// Only ever used through the C API, which serialises every call.
#[cfg(not(feature = "solidcore"))]
unsafe impl Send for FlatHandle {}
#[cfg(not(feature = "solidcore"))]
unsafe impl Sync for FlatHandle {}

#[cfg(not(feature = "solidcore"))]
impl Drop for FlatHandle {
    fn drop(&mut self) {
        #[cfg(feature = "native")]
        unsafe {
            smlib_flat_free(self.0.as_ptr())
        };
    }
}

#[cfg(feature = "native")]
mod calls {
    use super::*;
    use crate::ffi::{ERR_ARG, ERR_EMPTY, ERR_EXCEPTION};

    pub fn new() -> Result<FlatHandle, i32> {
        std::ptr::NonNull::new(unsafe { smlib_flat_new() }).map(FlatHandle).ok_or(ERR_EXCEPTION)
    }
    pub fn export(b: &Handle) -> Result<FlatHandle, i32> {
        let mut out: *mut RawFlat = std::ptr::null_mut();
        match (unsafe { smlib_brep_export(b.raw(), &mut out) }, std::ptr::NonNull::new(out)) {
            (OK, Some(p)) => Ok(FlatHandle(p)),
            (OK, None) => Err(ERR_EMPTY),
            (s, _) => Err(s),
        }
    }
    pub fn import(f: &FlatHandle) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_brep_import(f.0.as_ptr(), out) })
    }
    /// Copies the array out: the C side's buffer lives only as long as the arrays are unchanged.
    fn copy<T: Copy>(s: i32, data: *const T, len: i64) -> Result<Vec<T>, i32> {
        if s != OK {
            return Err(s);
        }
        let n = usize::try_from(len).map_err(|_| ERR_ARG)?;
        if n == 0 || data.is_null() {
            return Ok(Vec::new());
        }
        Ok(unsafe { std::slice::from_raw_parts(data, n) }.to_vec())
    }
    pub fn get_i64(f: &FlatHandle, id: i32) -> Result<Vec<i64>, i32> {
        let (mut data, mut len) = (std::ptr::null(), 0i64);
        let s = unsafe { smlib_flat_get_i64(f.0.as_ptr(), id, &mut data, &mut len) };
        copy(s, data, len)
    }
    pub fn get_f64(f: &FlatHandle, id: i32) -> Result<Vec<f64>, i32> {
        let (mut data, mut len) = (std::ptr::null(), 0i64);
        let s = unsafe { smlib_flat_get_f64(f.0.as_ptr(), id, &mut data, &mut len) };
        copy(s, data, len)
    }
    pub fn set_i64(f: &mut FlatHandle, id: i32, v: &[i64]) -> i32 {
        let Ok(n) = i64::try_from(v.len()) else { return ERR_ARG };
        unsafe { smlib_flat_set_i64(f.0.as_ptr(), id, v.as_ptr(), n) }
    }
    pub fn set_f64(f: &mut FlatHandle, id: i32, v: &[f64]) -> i32 {
        let Ok(n) = i64::try_from(v.len()) else { return ERR_ARG };
        unsafe { smlib_flat_set_f64(f.0.as_ptr(), id, v.as_ptr(), n) }
    }
    pub fn is_manifold_solid(b: &Handle) -> Result<bool, i32> {
        let mut out = 0i32;
        match unsafe { smlib_brep_is_manifold_solid(b.raw(), &mut out) } {
            OK => Ok(out != 0),
            s => Err(s),
        }
    }
}

/// The same calls when SMLib is not built: each one fails with `NOT_BUILT`.
#[cfg(not(any(feature = "native", feature = "solidcore")))]
mod calls {
    use super::*;
    use crate::ffi::NOT_BUILT;

    pub fn new() -> Result<FlatHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn export(_: &Handle) -> Result<FlatHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn import(_: &FlatHandle) -> Result<Handle, i32> {
        Err(NOT_BUILT)
    }
    pub fn get_i64(_: &FlatHandle, _: i32) -> Result<Vec<i64>, i32> {
        Err(NOT_BUILT)
    }
    pub fn get_f64(_: &FlatHandle, _: i32) -> Result<Vec<f64>, i32> {
        Err(NOT_BUILT)
    }
    pub fn set_i64(_: &mut FlatHandle, _: i32, _: &[i64]) -> i32 {
        NOT_BUILT
    }
    pub fn set_f64(_: &mut FlatHandle, _: i32, _: &[f64]) -> i32 {
        NOT_BUILT
    }
    pub fn is_manifold_solid(_: &Handle) -> Result<bool, i32> {
        Err(NOT_BUILT)
    }
}

#[cfg(feature = "solidcore")]
pub use super::rs_flat::*;
#[cfg(not(feature = "solidcore"))]
pub use calls::*;
