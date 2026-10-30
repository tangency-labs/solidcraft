//! Safe native fixtures and `SmFace::IdentifyFillet` for `blend.identify_fillet`.
//!
//! Shim: `harness/cpp/shim/blend_identify_fillet.cpp`. Upstream: `SmFace.cpp`.
//! The B-rep block is the shared `topology.edgeuse_p1` block (an upstream `.smb` file, a
//! wire, a face or the wire network). [`eval`] runs `IdentifyFillet(SM_ZONE_TOL_3D, 5.0,
//! flags & 1, flags & 2, ...)` on the face at an index of `GetFaces`, with `pMinMaxRadii`
//! when `flags & 4`, and returns `status is_fillet cross_section solver_type rail_dir radius
//! dist [radii_min radii_max] n_min idx* n_max idx*` (all after the status only on
//! success), the rail edges as positions in the face's outer-loop `GetEdges` list. A failed
//! fixture build returns `-999 status`; a face index past the B-rep's faces `-998`.

use super::topology_edgeuse_p1::{brep_block, Reader};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_blend_identify_fillet_eval(
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            face: i64,
            flags: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Runs `IdentifyFillet` on the face at index `face` (`GetFaces` order) of the B-rep that
/// `block` builds, with the flag bits `flags` (see the module docs).
pub fn eval(path: &str, block: &[f64], face: u32, flags: u32) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    if flags > 7 {
        return Err(bad("flags"));
    }
    let mut rd = Reader {
        input: block,
        at: 0,
    };
    brep_block(&mut rd, path)?;
    rd.done()?;
    let path = CString::new(path).map_err(|_| bad("path"))?;
    call_with_buffer(1 << 12, |out, len| {
        // SAFETY: the validated block stays live through the call; the output and length
        // pointers come from call_with_buffer.
        unsafe {
            sys::sc_blend_identify_fillet_eval(
                path.as_ptr(),
                block.as_ptr(),
                block.len() as i64,
                i64::from(face),
                i64::from(flags),
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
