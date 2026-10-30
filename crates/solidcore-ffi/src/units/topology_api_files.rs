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
//!
//! # Byte-for-byte write
//!
//! [`write`] (`sc_topology_api_files_write`) builds a B-rep from a [`Source`] (an upstream
//! test file read by `SmBrep::ReadFromFile`, ASCII, or an `SM_API` primitive:
//! `SmApiCreateBox`, `SmApiCreateSphere`, and the `SmBrep*&` overloads of
//! `SmApiCreateCylinder`, `SmApiCreateCone`, `SmApiCreateTorus`, `SmApiCreatePlane`),
//! writes it with `SmApiWriteBrepToFile` to a file the caller names and returns the file's
//! bytes (see [`write`]).

use std::ffi::CString;
use std::path::{Component, Path};

use super::primitive_api_primitives::{guard, Op};
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
        pub fn sc_topology_api_files_write(
            path: *const std::ffi::c_char,
            tmp: *const std::ffi::c_char,
            kind: i32,
            args: *const f64,
            n_args: i64,
            ascii: i32,
            rebuild: i32,
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

/// The B-rep [`write`] writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// `SmBrep::ReadFromFile(ctx, path, SM_ASCII, rebuild)` of an upstream test file.
    File {
        /// The file's absolute path.
        path: String,
        /// Rebuild the UV trim curves on reading.
        rebuild: bool,
    },
    /// `SmApiCreateBox(origin, size[0], size[1], size[2])`.
    Box {
        /// Lower-left corner.
        origin: [f64; 3],
        /// Length, width and height.
        size: [f64; 3],
    },
    /// `SmApiCreateSphere(center, radius)`.
    Sphere {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateCylinder(origin, radius, height)`.
    Cylinder {
        /// Base center.
        origin: [f64; 3],
        /// Radius.
        radius: f64,
        /// Height.
        height: f64,
    },
    /// `SmApiCreateCone(origin, radius_base, radius_top, height)`.
    Cone {
        /// Base center.
        origin: [f64; 3],
        /// Base radius.
        radius_base: f64,
        /// Top radius.
        radius_top: f64,
        /// Height.
        height: f64,
    },
    /// `SmApiCreateTorus(center, major, minor)`.
    Torus {
        /// Center.
        center: [f64; 3],
        /// Major radius.
        major: f64,
        /// Minor radius.
        minor: f64,
    },
    /// `SmApiCreatePlane(origin, x, y)`: a rectangular sheet.
    Plane {
        /// Lower-left corner.
        origin: [f64; 3],
        /// Extents along x and y.
        size: [f64; 2],
    },
}

/// The kind, the constructor's [`Op`] (for the guard) and the arguments of a primitive
/// source; `None` for a file.
fn primitive(s: &Source) -> Option<(i32, Op, Vec<f64>)> {
    let with = |o: &[f64; 3], rest: &[f64]| {
        let mut v = o.to_vec();
        v.extend_from_slice(rest);
        v
    };
    Some(match s {
        Source::File { .. } => return None,
        Source::Box { origin, size } => (1, Op::Box, with(origin, size)),
        Source::Sphere { center, radius } => (2, Op::Sphere, with(center, &[*radius])),
        Source::Cylinder {
            origin,
            radius,
            height,
        } => (3, Op::Cylinder, with(origin, &[*radius, *height])),
        Source::Cone {
            origin,
            radius_base,
            radius_top,
            height,
        } => (
            4,
            Op::Cone,
            with(origin, &[*radius_base, *radius_top, *height]),
        ),
        Source::Torus {
            center,
            major,
            minor,
        } => (5, Op::Torus, with(center, &[*major, *minor])),
        Source::Plane { origin, size } => (6, Op::PlaneBrep, with(origin, size)),
    })
}

/// `SmApiWriteBrepToFile` of the B-rep of `source`, ASCII (`ascii`) or binary, to the
/// file `tmp` (read back and removed by the shim): `source_status`, then on success
/// `write_status`, then on success `n` and the file's `n` bytes (one value per byte).
/// Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`. Primitive arguments must be finite
/// and pass the constructor guards of `primitive.api_primitives` ([`guard`]: the inputs
/// on which the constructors crash or hang upstream). Runs in this process (no static
/// state is latched).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a non-finite or guarded primitive argument or a
/// NUL byte in a path; the shim's status otherwise.
pub fn write(source: &Source, tmp: &str, ascii: bool) -> Result<Vec<f64>, FfiError> {
    let (kind, args, path, rebuild) = match primitive(source) {
        Some((kind, op, args)) => {
            if !args.iter().all(|a| a.is_finite()) {
                return Err(bad("non-finite primitive argument"));
            }
            guard(op, &args, None)?;
            (kind, args, String::new(), false)
        }
        None => match source {
            Source::File { path, rebuild } => {
                upstream_file(path, false)?;
                (0, Vec::new(), path.clone(), *rebuild)
            }
            _ => return Err(bad("source")),
        },
    };
    let cpath = CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    let ctmp = CString::new(tmp).map_err(|_| bad("tmp has a NUL byte"))?;
    call_with_buffer(1 << 16, |buf: &mut [f64], len| {
        // SAFETY: cpath, ctmp and args stay live through the call (args valid for
        // args.len() reads); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid; nothing is retained.
        unsafe {
            sys::sc_topology_api_files_write(
                cpath.as_ptr(),
                ctmp.as_ptr(),
                kind,
                args.as_ptr(),
                args.len() as i64,
                i32::from(ascii),
                i32::from(rebuild),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
