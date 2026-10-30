//! Safe wrapper of `SmApiCut` (`SmBrepCutting::DoCut` with an `SmPlaneCutter`) for
//! `topology.brep_cutting`.
//!
//! Shim: `harness/cpp/shim/topology_brep_cutting.cpp`. Upstream: `SmApiBrep.cpp`,
//! `SmBrepCutting.cpp`, `SmCutter.cpp`. The shim builds the body itself (an upstream `.smb`
//! file read by `ReadFromFile`, or `SmApiCreateBox` optionally filleted by
//! `SmApiCircularFillet`) and runs the cut in a fresh child process (the fillet solver
//! latches a static step, ND-3). The output is `record_len status geometry... 0 graph`: the
//! status, the B-rep's edge curve and face surface samples in list order (as
//! `topology.brep_local_operation`'s), then the raw graph export of the B-rep after the call
//! (`sc_topology_fixture.h`). A failed body build returns `-999 status`.

use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_topology_brep_cutting_eval(
            path: *const std::ffi::c_char,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The number of arguments: body kind, box size (3), fillet radius, plane point (3), plane
/// normal (3).
pub const NARGS: usize = 11;

/// Runs `SmApiCut` on the body `args` and `path` describe, in a fresh child process.
///
/// Guards: every argument finite; the body kind 0 (file, `path` non-empty) or 1 (box: sizes
/// in `1e-6..=1e6`, the fillet radius `0` or positive and below a quarter of the smallest
/// size); the plane point and normal within `1e6`. A zero normal is allowed (`SmApiCut`
/// rejects it with `SM_ERR_INVALID_INPUT` before touching the body).
pub fn eval(path: &str, args: &[f64; NARGS]) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    if !args.iter().all(|x| x.is_finite()) {
        return Err(bad("non-finite argument"));
    }
    if ![0.0, 1.0].contains(&args[0]) {
        return Err(bad("body kind"));
    }
    if args[0] == 0.0 {
        if path.is_empty() {
            return Err(bad("file body without path"));
        }
    } else {
        let sized = |x: f64| (1e-6..=1e6).contains(&x);
        if !(sized(args[1]) && sized(args[2]) && sized(args[3])) {
            return Err(bad("box size"));
        }
        let smallest = args[1].min(args[2]).min(args[3]);
        if args[4] < 0.0 || args[4] >= 0.25 * smallest {
            return Err(bad("fillet radius"));
        }
    }
    if !args[5..].iter().all(|x| x.abs() <= 1e6) {
        return Err(bad("plane point or normal"));
    }
    let path = CString::new(path).map_err(|_| bad("path"))?;
    let args = *args;
    crate::isolate::fresh_process(move || {
        call_with_buffer(1 << 20, |out, len| {
            // SAFETY: the path and the fixed-length arguments stay live through the call;
            // the output and length pointers come from call_with_buffer.
            unsafe {
                sys::sc_topology_brep_cutting_eval(
                    path.as_ptr(),
                    args.as_ptr(),
                    NARGS as i64,
                    out.as_mut_ptr(),
                    out.len() as i64,
                    &raw mut *len,
                )
            }
        })
    })
}
