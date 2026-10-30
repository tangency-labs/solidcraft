//! Safe native fixtures and `SmBrep::LocalOperation` for `topology.brep_local_operation`.
//!
//! Shim: `harness/cpp/shim/topology_brep_local_operation.cpp`. Upstream: `SmBrep.cpp`.
//! The B-rep block is the shared `topology.edgeuse_p1` block (an upstream `.smb` file, a
//! wire, a face or the wire network). Op 0 exports the fixture: `0 graph`. Op 1 runs
//! `LocalOperation` and returns `record_len record* 0 graph`, the record being the status,
//! then samples of every edge curve, its edge uses' UV trim curves and every face surface
//! (type, natural interval or domain, five or nine evaluated points) and the raw graph of
//! the changed B-rep. Op 3 is op 1 with every face (`GetFaces` order) and edge (`GetEdges`
//! order) first given its own `SmLongAttribute` (id `10007 + 500 kind + index`, kind 0
//! face, 1 edge, `SM_AB_COPY`), the record then ending with `nf (n id*)* ne (n id*)*`, the
//! ascending attribute ids each face and edge of the changed B-rep carries (issue #1462).
//! A failed fixture build returns `-999 status`; a replacement the shim
//! cannot build (face index out of range, face without surface, degenerate normal) `-998`.

use super::topology_edgeuse_p1::{brep_block, Reader};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_topology_brep_local_operation_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The most faces one call replaces.
pub const MAX_REPLACED: usize = 8;

/// One replaced face: the new surface is the plane through the face's surface point at the
/// middle of its UV domain, moved `offset` along the surface normal there, with the normal
/// plus `tilt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Replacement {
    /// Index of the face in `GetFaces` order.
    pub face: u32,
    /// Distance along the surface normal.
    pub offset: f64,
    /// Added to the surface normal to make the plane normal.
    pub tilt: [f64; 3],
}

/// Op 0 exports the fixture graph, op 1 runs `LocalOperation`, op 2 exports the native
/// face and vertex lists (`nf face* nv vertex*`, indices of the graph), op 3 runs the
/// tagged `LocalOperation`.
///
/// `block`: the B-rep block. `replaced`: at most [`MAX_REPLACED`] faces, every number
/// finite.
pub fn eval(
    op: u8,
    path: &str,
    block: &[f64],
    replaced: &[Replacement],
) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    let finite = |r: &Replacement| r.offset.is_finite() && r.tilt.iter().all(|x| x.is_finite());
    if op > 3 || replaced.len() > MAX_REPLACED || !replaced.iter().all(finite) {
        return Err(bad("replacement list"));
    }
    let mut args = vec![replaced.len() as f64];
    for r in replaced {
        args.extend([f64::from(r.face), r.offset, r.tilt[0], r.tilt[1], r.tilt[2]]);
    }
    let mut rd = Reader {
        input: block,
        at: 0,
    };
    brep_block(&mut rd, path)?;
    rd.done()?;
    let path = CString::new(path).map_err(|_| bad("path"))?;
    call_with_buffer(1 << 20, |out, len| {
        // SAFETY: the validated block and the argument vector stay live through the call;
        // the output and length pointers come from call_with_buffer.
        unsafe {
            sys::sc_topology_brep_local_operation_eval(
                i32::from(op),
                path.as_ptr(),
                block.as_ptr(),
                block.len() as i64,
                args.as_ptr(),
                args.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
