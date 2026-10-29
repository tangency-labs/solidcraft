//! Safe differential fixtures for `SmBrep::AssertValid` and the capi
//! `smlib_brep_check` sequence. The shim uses the pinned kernel unchanged.
use super::topology_edgeuse_p1::{brep_block, Reader};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;
mod sys {
    extern "C" {
        pub fn sc_topology_assert_valid_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// The number of values of a primitive source block (`200` sphere, `201` cylinder,
/// `202` cone, `203` torus), `None` for another kind.
fn primitive_len(kind: f64) -> Option<usize> {
    match kind {
        200.0 => Some(5),
        201.0 | 203.0 => Some(6),
        202.0 => Some(7),
        _ => None,
    }
}

/// `AssertValid` (op 0), the capi sequence (op 1) or the level-2 tests one by one (op
/// 2: `sm_AssertTestFace32/33/38` per face, `sm_AssertTestEdge13` per edge,
/// `sm_AssertTestVertex1` per vertex) or the capi sequence on the body's
/// `SmApiBrepCopy` (op 3, without mutation) of the B-rep built from `block` (a
/// `sc_edgeuse_graph.h` source or a primitive), after mutation `args[2]` (kinds 1-10 of
/// `topology_brep_validation`). Arguments: level (0-4), walk, mutation kind, index,
/// value. Op 2 refuses the null-pointer mutations 8-10: the tests dereference the face
/// surface, edge curve and edge use links without the `ValidatePointers` guard
/// `AssertValid` runs first.
pub fn eval(op: u8, path: &str, block: &[f64], args: [f64; 5]) -> Result<Vec<f64>, FfiError> {
    let bad = || FfiError::InvalidArgument("invalid AssertValid arguments".into());
    if op > 3
        || (op == 2 && args[2] >= 8.0)
        || (op == 3 && args[2] != 0.0)
        || !args.iter().all(|x| x.is_finite())
        || args[0].fract() != 0.0
        || !(0.0..=4.0).contains(&args[0])
        || !(args[1] == 0.0 || args[1] == 1.0)
        || args[2].fract() != 0.0
        || !(0.0..=10.0).contains(&args[2])
        || args[3].fract() != 0.0
        || !(0.0..=1e6).contains(&args[3])
        || args[4].abs() > 1e6
    {
        return Err(bad());
    }
    if (args[2] == 4.0 || args[2] == 5.0)
        && (!(0.0..=2.0).contains(&args[4]) || args[4].fract() != 0.0)
    {
        return Err(bad());
    }
    if let Some(n) = block.first().copied().and_then(primitive_len) {
        if block.len() != n || !block.iter().all(|x| x.is_finite() && x.abs() <= 1e6) {
            return Err(bad());
        }
    } else {
        let mut rd = Reader {
            input: block,
            at: 0,
        };
        brep_block(&mut rd, path)?;
        rd.done()?;
    }
    let path = CString::new(path).map_err(|_| bad())?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: validated block and fixed-length arguments remain live throughout
        // the call; output and length pointers are supplied by call_with_buffer.
        unsafe {
            sys::sc_topology_assert_valid_eval(
                i32::from(op),
                path.as_ptr(),
                block.as_ptr(),
                block.len() as i64,
                args.as_ptr(),
                5,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
