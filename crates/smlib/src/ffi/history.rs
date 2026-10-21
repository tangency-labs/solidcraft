//! The history block of `capi/include/smlib.h` (`smlib_history_tag`, `smlib_history_read`),
//! declared by hand. Part of the FFI boundary, like its parent: called only through
//! [`crate::history`].
#![allow(unsafe_code)]

use super::Handle;
#[cfg(feature = "native")]
use super::{ERR_BUFFER, OK, RawBrep, new_brep};

/// `SMLIB_HISTORY_VERSION` this crate reads.
pub const VERSION: i64 = 1;

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_history_tag(brep: *const RawBrep, operand: i32, out: *mut *mut RawBrep) -> i32;
    fn smlib_history_read(result: *const RawBrep, buf: *mut i64, cap: i64, out_len: *mut i64) -> i32;
}

#[cfg(feature = "native")]
pub fn tag(b: &Handle, operand: i32) -> Result<Handle, i32> {
    new_brep(|out| unsafe { smlib_history_tag(b.raw(), operand, out) })
}

/// The result's history records, sized by asking first.
#[cfg(feature = "native")]
pub fn read(b: &Handle) -> Result<Vec<i64>, i32> {
    let mut len = 0i64;
    let mut buf: Vec<i64> = Vec::new();
    loop {
        let cap = i64::try_from(buf.len()).map_err(|_| ERR_BUFFER)?;
        let ptr = if buf.is_empty() { std::ptr::null_mut() } else { buf.as_mut_ptr() };
        match unsafe { smlib_history_read(b.raw(), ptr, cap, &mut len) } {
            OK => {
                buf.truncate(usize::try_from(len).map_err(|_| ERR_BUFFER)?);
                return Ok(buf);
            }
            // Asked again with room for what it said; a second shortfall would mean the body
            // changed, which a handle never does.
            ERR_BUFFER if buf.is_empty() => buf.resize(usize::try_from(len).map_err(|_| ERR_BUFFER)?, 0),
            s => return Err(s),
        }
    }
}

#[cfg(not(feature = "native"))]
pub fn tag(_: &Handle, _: i32) -> Result<Handle, i32> {
    Err(super::NOT_BUILT)
}

#[cfg(not(feature = "native"))]
pub fn read(_: &Handle) -> Result<Vec<i64>, i32> {
    Err(super::NOT_BUILT)
}
