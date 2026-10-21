//! The C API of `capi/include/smlib.h` (tangency-labs/smlib), declared by hand. The only place in
//! the crate with `unsafe`: every function here is called through the safe wrappers in the crate
//! root, which check their arguments and own the handles.
#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};

/// `SMLIB_ABI_VERSION` this crate was written against.
pub const ABI_VERSION: i32 = 1;

pub const OK: i32 = 0;
pub const ERR_EXCEPTION: i32 = -1000;
pub const ERR_BUFFER: i32 = -1001;
pub const ERR_ARG: i32 = -1002;
pub const ERR_EMPTY: i32 = -1003;

/// `smlib_brep`, opaque.
#[repr(C)]
pub struct RawBrep {
    _private: [u8; 0],
}

pub type LogFn = extern "C" fn(user: *mut c_void, level: i32, line: *const c_char);

#[cfg(feature = "native")]
unsafe extern "C" {
    pub fn smlib_abi_version() -> i32;
    pub fn smlib_init() -> i32;
    pub fn smlib_set_log(log: Option<LogFn>, user: *mut c_void);
    pub fn smlib_set_dump_dir(dir: *const c_char) -> i32;
    pub fn smlib_current_call(buf: *mut c_char, cap: i64) -> i64;
    pub fn smlib_brep_read(path: *const c_char, ascii: i32, out: *mut *mut RawBrep) -> i32;
    pub fn smlib_brep_write(brep: *const RawBrep, path: *const c_char, ascii: i32) -> i32;
    pub fn smlib_brep_free(brep: *mut RawBrep);
    pub fn smlib_brep_copy(brep: *const RawBrep, out: *mut *mut RawBrep) -> i32;
    pub fn smlib_make_box(origin: *const f64, size: *const f64, out: *mut *mut RawBrep) -> i32;
    pub fn smlib_brep_volume(brep: *const RawBrep, relative_accuracy: f64, out: *mut f64) -> i32;
    pub fn smlib_brep_counts(brep: *const RawBrep, out: *mut i64) -> i32;
}

/// Returned by every call when the crate was built without the `native` feature.
pub const NOT_BUILT: i32 = -2000;

/// An owned `smlib_brep`. Only made here, from a pointer the C API returned; freed once on drop.
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub struct Handle(std::ptr::NonNull<RawBrep>);

// SMLib serialises every call behind its own lock, and a handle is only ever used through it.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        #[cfg(feature = "native")]
        unsafe {
            smlib_brep_free(self.0.as_ptr())
        };
    }
}

impl Handle {
    #[cfg_attr(not(feature = "native"), allow(dead_code))]
    fn raw(&self) -> *const RawBrep {
        self.0.as_ptr()
    }
}

/// Runs a call that returns a new B-rep through `out`.
#[cfg(feature = "native")]
fn new_brep(f: impl FnOnce(*mut *mut RawBrep) -> i32) -> Result<Handle, i32> {
    let mut out: *mut RawBrep = std::ptr::null_mut();
    let s = f(&mut out);
    match (s, std::ptr::NonNull::new(out)) {
        (OK, Some(p)) => Ok(Handle(p)),
        (OK, None) => Err(ERR_EMPTY),
        (s, _) => Err(s),
    }
}

#[cfg(feature = "native")]
mod calls {
    use super::*;
    use std::ffi::CStr;

    pub fn abi_version() -> i32 {
        unsafe { smlib_abi_version() }
    }
    pub fn init() -> i32 {
        unsafe { smlib_init() }
    }
    pub fn set_log(log: Option<LogFn>) {
        unsafe { smlib_set_log(log, std::ptr::null_mut()) }
    }
    pub fn set_dump_dir(dir: Option<&CStr>) -> i32 {
        unsafe { smlib_set_dump_dir(dir.map_or(std::ptr::null(), CStr::as_ptr)) }
    }
    pub fn current_call(buf: &mut [u8]) -> usize {
        let cap = i64::try_from(buf.len()).unwrap_or(i64::MAX);
        let n = unsafe { smlib_current_call(buf.as_mut_ptr().cast(), cap) };
        usize::try_from(n).unwrap_or(0).min(buf.len().saturating_sub(1))
    }
    pub fn read(path: &CStr, ascii: bool) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_brep_read(path.as_ptr(), i32::from(ascii), out) })
    }
    pub fn write(b: &Handle, path: &CStr, ascii: bool) -> i32 {
        unsafe { smlib_brep_write(b.raw(), path.as_ptr(), i32::from(ascii)) }
    }
    pub fn copy(b: &Handle) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_brep_copy(b.raw(), out) })
    }
    pub fn make_box(origin: &[f64; 3], size: &[f64; 3]) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_make_box(origin.as_ptr(), size.as_ptr(), out) })
    }
    pub fn volume(b: &Handle, relative_accuracy: f64) -> Result<f64, i32> {
        let mut v = 0.0;
        match unsafe { smlib_brep_volume(b.raw(), relative_accuracy, &mut v) } {
            OK => Ok(v),
            s => Err(s),
        }
    }
    pub fn counts(b: &Handle) -> Result<[i64; 3], i32> {
        let mut c = [0i64; 3];
        match unsafe { smlib_brep_counts(b.raw(), c.as_mut_ptr()) } {
            OK => Ok(c),
            s => Err(s),
        }
    }
}

/// The same calls when SMLib is not built: each one fails with [`NOT_BUILT`].
#[cfg(not(feature = "native"))]
mod calls {
    use super::*;
    use std::ffi::CStr;

    pub fn abi_version() -> i32 {
        NOT_BUILT
    }
    pub fn init() -> i32 {
        NOT_BUILT
    }
    pub fn set_log(_: Option<LogFn>) {}
    pub fn set_dump_dir(_: Option<&CStr>) -> i32 {
        NOT_BUILT
    }
    pub fn current_call(_: &mut [u8]) -> usize {
        0
    }
    pub fn read(_: &CStr, _: bool) -> Result<Handle, i32> {
        Err(NOT_BUILT)
    }
    pub fn write(_: &Handle, _: &CStr, _: bool) -> i32 {
        NOT_BUILT
    }
    pub fn copy(_: &Handle) -> Result<Handle, i32> {
        Err(NOT_BUILT)
    }
    pub fn make_box(_: &[f64; 3], _: &[f64; 3]) -> Result<Handle, i32> {
        Err(NOT_BUILT)
    }
    pub fn volume(_: &Handle, _: f64) -> Result<f64, i32> {
        Err(NOT_BUILT)
    }
    pub fn counts(_: &Handle) -> Result<[i64; 3], i32> {
        Err(NOT_BUILT)
    }
}

pub use calls::*;

/// A log line from the C API: NUL-terminated and valid for the callback's duration.
pub fn line(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
}
