//! Safe wrapper of the `SM_API` files and conversion wrappers for `topology.api_files`:
//! `SmApiReadBrepFromFile`, `SmApiReadPartFromFile` and `SmApiTurnToNurbs`.
//!
//! Shim: `harness/cpp/shim/topology_api_files.cpp`. Upstream: `SmApiImportExport.cpp`,
//! `SmApiBrep.cpp`. Every call runs in a fresh child process (the fillet solver of a
//! filleted box latches a static step, ND-3).
//!
//! # Output
//!
//! The status (`0` = `SM_SUCCESS`), then on success the op's record:
//! - Op 1 (`SmApiReadBrepFromFile`) and op 4 (`SmApiWriteBrepToFile` of a box, then op
//!   1 on that file): the B-rep record: tolerance; region, shell, face, loop (over the
//!   faces' `GetLoops`), edge and vertex counts; per face (`GetFaces`) its surface type
//!   (`-1` for none); per edge (`GetEdges`) its curve type (`-1` for none) and interval;
//!   per vertex (`GetVertices`) its point.
//! - Op 2 (`SmApiReadPartFromFile`): the `topology.part_file` record (curves: type,
//!   dimension, natural interval; surfaces: type, natural UV domain; the Boolean tree
//!   nodes), with a B-rep record per B-rep.
//! - Op 3 (`SmApiTurnToNurbs`): the B-rep record, then per face with a surface its
//!   natural UV domain and nine `status x y z` points at `u, v` in `{0, 1/2, 1}` of it (u
//!   outer), per edge with a curve five `status x y z` points at `t` in `{0, 1/4, .., 1}`
//!   of the edge interval (`lo + (hi - lo) * s`; `status` alone on a failed evaluation).
//!
//! Ops 1, 3 and 4 then add the trim record: per edge (`GetEdges`) and per edge use
//! (`GetEdgeuses`) with a UV trim curve, its type, natural interval and three samples at
//! 0.17, 0.37 and 0.83 of it, then `support/primitive_record.h`'s `append` (vertices,
//! edges with their use and trim counts, faces); a sample is `0 x y z` or the failed
//! evaluation's status.
//!
//! A failed body build (op 3's file read, box, fillet or primitive) or op 4's write returns
//! `-999 status`.
//!
//! # Guard
//!
//! Every argument finite; the flags 0 or 1; the prefill bits in `0..=15`; the body a
//! whole number in `0..=7`. A file the shim reads (ops 1 and 2 with a path, op 3's file
//! body) must be an upstream test file (a path under `upstream/TestFiles/pt_TestFiles`,
//! without `..`), read ASCII only (upstream's binary readers trust the counts they read:
//! a text file read as binary crashes them), op 2's a part file (starting with
//! `//[Output Summary]`), the others not. Op 4's path is
//! a new file in the temporary directory (the shim writes and removes it). A box has
//! sizes in `1e-6..=1e6` and a fillet radius `0` or positive below a quarter of the
//! smallest size. A primitive body (op 3) has its sizes in `1e-6..=1e6`, a torus its
//! minor radius below its major one.

use std::ffi::CString;
use std::path::{Component, Path};

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_topology_api_files_eval(
            op: i32,
            path: *const std::ffi::c_char,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The number of arguments: `has_path`, `ascii`, `rebuild`, `prefill` (bits: 1 curves,
/// 2 surfaces, 4 Boolean tree nodes, 8 B-reps), `body` (0 null, 1 the file, 2 box, 3
/// cylinder `r h`, 4 cone `r_base r_top h`, 5 sphere `r`, 6 torus `R r`, 7 the mixed box),
/// sizes (3), fillet radius.
pub const NARGS: usize = 9;

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.to_string())
}

/// The guard of a file the shim reads: an upstream test file; `part` whether it must be
/// a part file (else it must not be one).
fn upstream_file(path: &str, part: bool) -> Result<(), FfiError> {
    let p = Path::new(path);
    if p.components().any(|c| c == Component::ParentDir) {
        return Err(bad("path with `..`"));
    }
    let canon = p.canonicalize().map_err(|e| bad(&format!("{path}: {e}")))?;
    let under = canon
        .ancestors()
        .any(|a| a.ends_with(Path::new("upstream/TestFiles/pt_TestFiles")));
    if !under {
        return Err(bad("not an upstream test file"));
    }
    let bytes = std::fs::read(&canon).map_err(|e| bad(&format!("{path}: {e}")))?;
    if bytes.starts_with(b"//[Output Summary]") != part {
        return Err(bad(if part {
            "not a part file"
        } else {
            "a part file"
        }));
    }
    Ok(())
}

/// Runs op `op` (see the module docs) on `args` and `path`, in a fresh child process.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs the guard refuses; [`FfiError::Status`] for
/// a shim failure or a crashed child.
pub fn eval(op: u8, path: &str, args: &[f64; NARGS]) -> Result<Vec<f64>, FfiError> {
    if !(1..=4).contains(&op) || !args.iter().all(|x| x.is_finite()) {
        return Err(bad("op or non-finite argument"));
    }
    let flag = |x: f64| x == 0.0 || x == 1.0;
    let [has_path, ascii, rebuild, prefill, body, sx, sy, sz, fillet] = *args;
    if !(flag(has_path) && flag(ascii) && flag(rebuild)) {
        return Err(bad("flag"));
    }
    if !(0.0..=15.0).contains(&prefill) || prefill.fract() != 0.0 {
        return Err(bad("prefill"));
    }
    if !(0.0..=7.0).contains(&body) || body.fract() != 0.0 {
        return Err(bad("body"));
    }
    let reads_file = match op {
        1 | 2 => has_path == 1.0,
        3 => body == 1.0,
        _ => false,
    };
    if reads_file {
        // (upstream's binary readers trust the counts they read: a text file read as
        // binary crashes the B-rep reader, a held memory-safety record)
        if ascii != 1.0 {
            return Err(bad("binary read of a text test file"));
        }
        upstream_file(path, op == 2)?;
    }
    if op == 4 {
        let p = Path::new(path);
        if !p.starts_with(std::env::temp_dir()) || p.exists() {
            return Err(bad("op 4 path is not a new temporary file"));
        }
    }
    let sized = |x: f64| (1e-6..=1e6).contains(&x);
    if op == 3 && body >= 3.0 {
        let ok = match body as u8 {
            3 => sized(sx) && sized(sy),
            5 => sized(sx),
            6 => sized(sx) && sized(sy) && sy < sx,
            _ => sized(sx) && sized(sy) && sized(sz),
        };
        if !ok {
            return Err(bad("primitive size"));
        }
    }
    if op == 4 || (op == 3 && body == 2.0) {
        if !(sized(sx) && sized(sy) && sized(sz)) {
            return Err(bad("box size"));
        }
        let smallest = sx.min(sy).min(sz);
        if fillet < 0.0 || fillet >= 0.25 * smallest {
            return Err(bad("fillet radius"));
        }
    }
    let path = CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    let args = *args;
    crate::isolate::fresh_process(move || {
        call_with_buffer(1 << 20, |out, len| {
            // SAFETY: the path and the fixed-length arguments stay live through the call;
            // the output and length pointers come from call_with_buffer.
            unsafe {
                sys::sc_topology_api_files_eval(
                    i32::from(op),
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
