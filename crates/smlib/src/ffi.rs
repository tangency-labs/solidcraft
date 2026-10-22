//! The C API of `capi/include/smlib.h` (tangency-labs/smlib), declared by hand. The only place in
//! the crate with `unsafe`: every function here is called through the safe wrappers in the crate
//! root, which check their arguments and own the handles.
#![allow(unsafe_code)]

#[cfg(feature = "native")]
use std::ffi::{c_char, c_void};

/// `SMLIB_ABI_VERSION` this crate was written against.
pub const ABI_VERSION: i32 = 3;

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

#[cfg(feature = "native")]
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
    pub fn smlib_brep_transform(brep: *const RawBrep, matrix: *const [f64; 4], out: *mut *mut RawBrep) -> i32;
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
    /// Routes SMLib's log lines to `sink` (`None`: stops them).
    pub fn set_log(sink: Option<fn(i32, String)>) {
        let log: Option<LogFn> = match sink {
            Some(sink) => {
                // The first sink stays: init sets it once, and lines keep a single destination.
                let _ = SINK.set(sink);
                Some(trampoline)
            }
            None => None,
        };
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
    pub fn transform(b: &Handle, matrix: &[[f64; 4]; 4]) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_brep_transform(b.raw(), matrix.as_ptr(), out) })
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
    pub fn set_log(_: Option<fn(i32, String)>) {}
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
    pub fn transform(_: &Handle, _: &[[f64; 4]; 4]) -> Result<Handle, i32> {
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

pub mod check;
pub mod flat;
pub mod history;
pub mod measure;
pub mod ops;
pub mod split;
pub mod sheets;
pub mod ssi;
pub mod sweeps;
pub mod trim;

/// Where [`set_log`] sends SMLib's log lines.
#[cfg(feature = "native")]
static SINK: std::sync::OnceLock<fn(i32, String)> = std::sync::OnceLock::new();

/// The log callback the C API calls: `line` is NUL-terminated and valid for the call's duration
/// (smlib.h), so it is read here, inside the one module that touches raw pointers.
#[cfg(feature = "native")]
extern "C" fn trampoline(_: *mut c_void, level: i32, line: *const c_char) {
    let text = if line.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(line) }.to_string_lossy().into_owned() };
    if let Some(sink) = SINK.get() {
        sink(level, text);
    }
}

// ---- Primitives (smlib_primitives.cpp) -----------------------------------------------------

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_make_cylinder(origin: *const f64, radius: f64, height: f64, out: *mut *mut RawBrep) -> i32;
    fn smlib_make_sphere(center: *const f64, radius: f64, out: *mut *mut RawBrep) -> i32;
    fn smlib_make_cone(origin: *const f64, base_radius: f64, top_radius: f64, height: f64, out: *mut *mut RawBrep) -> i32;
    fn smlib_make_torus(center: *const f64, major_radius: f64, minor_radius: f64, out: *mut *mut RawBrep) -> i32;
}

#[cfg(feature = "native")]
mod primitive_calls {
    use super::*;

    pub fn make_cylinder(origin: &[f64; 3], radius: f64, height: f64) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_make_cylinder(origin.as_ptr(), radius, height, out) })
    }
    pub fn make_sphere(center: &[f64; 3], radius: f64) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_make_sphere(center.as_ptr(), radius, out) })
    }
    pub fn make_cone(origin: &[f64; 3], base_radius: f64, top_radius: f64, height: f64) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_make_cone(origin.as_ptr(), base_radius, top_radius, height, out) })
    }
    pub fn make_torus(center: &[f64; 3], major_radius: f64, minor_radius: f64) -> Result<Handle, i32> {
        new_brep(|out| unsafe { smlib_make_torus(center.as_ptr(), major_radius, minor_radius, out) })
    }
}

/// The same calls when SMLib is not built: each one fails with [`NOT_BUILT`].
#[cfg(not(feature = "native"))]
mod primitive_calls {
    use super::*;

    pub fn make_cylinder(_: &[f64; 3], _: f64, _: f64) -> Result<Handle, i32> {
        Err(NOT_BUILT)
    }
    pub fn make_sphere(_: &[f64; 3], _: f64) -> Result<Handle, i32> {
        Err(NOT_BUILT)
    }
    pub fn make_cone(_: &[f64; 3], _: f64, _: f64, _: f64) -> Result<Handle, i32> {
        Err(NOT_BUILT)
    }
    pub fn make_torus(_: &[f64; 3], _: f64, _: f64) -> Result<Handle, i32> {
        Err(NOT_BUILT)
    }
}

pub use primitive_calls::*;
