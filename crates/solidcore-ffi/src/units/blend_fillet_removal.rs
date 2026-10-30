//! Safe native fixtures and `SmBrep::DeleteFilletFace` for `blend.fillet_removal`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_removal.cpp`. Upstream: `SmFilletRemoval.cpp`.
//! The B-rep block is the shared `topology.edgeuse_p1` block (an upstream `.smb` file, a
//! wire, a face or the wire network). [`eval`] runs `DeleteFilletFace` on the face at an
//! index of `GetFaces` with editing enabled for the call and returns
//! `record_len record* 0 graph`, the record being the status, then samples of every edge
//! curve, its edge uses' UV trim curves and every face surface (type, natural interval or
//! domain, five or nine evaluated points) and the raw graph of the B-rep after it. A failed
//! fixture build returns `-999 status`; a face index past the B-rep's faces `-998`.

use super::topology_edgeuse_p1::{brep_block, Reader};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_removal_eval(
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            face: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Runs `DeleteFilletFace` on the face at index `face` (`GetFaces` order) of the B-rep
/// that `block` builds (see the module docs).
pub fn eval(path: &str, block: &[f64], face: u32) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    let mut rd = Reader {
        input: block,
        at: 0,
    };
    brep_block(&mut rd, path)?;
    rd.done()?;
    let path = CString::new(path).map_err(|_| bad("path"))?;
    call_with_buffer(1 << 20, |out, len| {
        // SAFETY: the validated block stays live through the call; the output and length
        // pointers come from call_with_buffer.
        unsafe {
            sys::sc_blend_fillet_removal_eval(
                path.as_ptr(),
                block.as_ptr(),
                block.len() as i64,
                i64::from(face),
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
