//! FFI wrappers for unit `heal.stitch_p2`.
//!
//! Shim: `harness/cpp/shim/heal_stitch_p2.cpp` (`sc_heal_stitch_p2_eval`).
//! Upstream: `source/SMLib/src/SmStitch.cpp`, `source/SMLib/inc/SmStitch.h`.
//!
//! Every op starts with the B-rep block of unit `heal.stitch_p1` (the shim builds a
//! fresh `SmBrep` per call): `0` + the B-rep block of [`super::topology_edgeuse_p1`]
//! (`.smb` file, wire edge, face, `prog_test` wire network); `1 n (surface u0 v0 u1
//! v1)*n` with `1 <= n <=` [`MAX_FACES`], one face per surface in one B-rep
//! (unstitched sheets); or `2 P[3] l w h` (`SmApiCreateBox`). Indices refer to the
//! `Export`'s traversal order; `-1` in a list is a null entry. Every output starts with `0`, or
//! with `-1 status` when the B-rep could not be built (nothing else follows). In a
//! status slot, `-3` is "upstream would never return": the shim did not call it.
//!
//! | [`Op`]   | parameters after the block | outputs |
//! |----------|----------------------------|---------|
//! | `Export` |                            | the `Export` of unit `topology.brep_p1` (its extras, then the graph) |
//! | `Do`     | `tol nesting ignore fast squeeze split manifold slivers want_vgap want_egap has_cv n v*n has_ce n e*n` | `status [stitched lamina max_vgap max_egap [min_vgap] [min_egap] digest]` (`DoStitching`; a candidate list is passed only with its `has_` flag) |
//! | `Solid`  |                            | `status [solid stitched max_vgap max_egap digest regions]` (`StitchIntoSolid`; `regions` is the region count and the sorted `IsVoid` flags of `GetRegions`; `-3` when the tolerance doubling cannot end: a tenth of the B-rep tolerance at or below zero and below `1/200` of the box diagonal; a NaN diagonal skips the loop, an infinite one ends it once the tolerance doubles to infinity) |
//! | `Shell`  | `well_formed max_tol`      | `status [stitched max_vgap max_egap digest]` (`StitchIntoShell`; `-3` when `max_tol / 100` underflows to zero) |
//! | `VList`  | `tol manifold squeeze n v*n` | `status [max_gap min_gap surviving deleted deleted_edges list [digest]]` (one-list `StitchVertexPairs`; the digest when a vertex was deleted) |
//! | `Split`  | `tol manifold ignore n v*n n e*n editing` | `status [max_gap min_gap processed split new verts edges [digest]]` (`SplitEdgesWithVertices`, on the B-rep with `m_bEditingEnabled` set when `editing`; the digest when it processed a vertex or made an edge) |
//!
//! Output lists are `n idx*n` with `-1` for null and `-3` for an entity the op created.
//! A digest is the B-rep after the op: `n_faces n_edges n_vertices`, each vertex's
//! point and tolerance in list order, then the topology of
//! `harness/cpp/shim/sc_stitch_digest.h` (as `heal.stitch_p1`).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: the B-rep
//! block rules of `heal.stitch_p1`, indices inside the B-rep's export, finite values
//! (any `Shell` `max_tol`: a non-finite one skips the doubling loop), list counts up to
//! [`MAX_LIST`], and a `VList` list without a repeated vertex (each copy of a coincident
//! vertex is glued, the second one after it was freed: UB-178 in
//! `plan/upstream-issues.md`). The `-3` guards are HANG-33 and HANG-24 (a zero or
//! negative B-rep tolerance). They test only the necessary condition (the doubling
//! cannot reach its bound), so they also refuse a B-rep that upstream returns from
//! because its first `DoStitching` pass leaves no lamina edge; the runner applies the
//! same tests. Narrowing them needs that first pass run on the C++ side first.

use crate::units::topology_brep_p1::counts;
use crate::units::topology_edgeuse_p1::{bad, brep_block, face_block, Reader, MAX_LIST};
use crate::{call_with_buffer, FfiError};

/// Most faces of an unstitched-sheets block (the generated grids use at most six; the
/// shim's build has no bound of its own).
pub const MAX_FACES: usize = 16;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Do = 1,
    Solid = 2,
    Shell = 3,
    VList = 4,
    Split = 5,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

mod sys {
    extern "C" {
        pub fn sc_heal_stitch_p2_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// A stitch B-rep block (as unit `heal.stitch_p1`'s).
fn block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    match rd.index(2)? {
        0 => brep_block(rd, path),
        1 => {
            let n = rd.index(MAX_FACES)?;
            if n == 0 {
                return Err(bad("no faces"));
            }
            for _ in 0..n {
                // a sheet face's plane domain is bounded as in heal.stitch_p1's block
                face_block(rd, 1e4)?;
            }
            Ok(())
        }
        _ => {
            rd.vec3()?;
            for _ in 0..3 {
                let d = rd.finite()?;
                if !(1e-3..=1e6).contains(&d) {
                    return Err(bad("box size out of range"));
                }
            }
            Ok(())
        }
    }
}

/// `n idx*n` indices below `count` or `-1`; returns the non-null ones.
fn list(rd: &mut Reader<'_>, count: usize) -> Result<Vec<usize>, FfiError> {
    let n = rd.index(MAX_LIST)?;
    let mut v = Vec::new();
    for _ in 0..n {
        let x = rd.num()?;
        if x == -1.0 {
            continue;
        }
        rd.at -= 1;
        v.push(rd.below(count)?);
    }
    Ok(v)
}

fn op_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let (nv, ne) = (c[9], c[7]);
    match op {
        // no parameters after the block
        Op::Export | Op::Solid => {}
        Op::Do => {
            rd.finite()?;
            for _ in 0..10 {
                rd.flag()?;
            }
            list(rd, nv)?;
            rd.flag()?;
            list(rd, ne)?;
        }
        // a non-finite max_tol skips the doubling loop, as upstream
        Op::Shell => {
            rd.flag()?;
            rd.num()?;
        }
        Op::VList => {
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
            let v = list(rd, nv)?;
            for (i, x) in v.iter().enumerate() {
                if v.iter().take(i).any(|y| y == x) {
                    return Err(bad("vertex repeated in the list"));
                }
            }
        }
        Op::Split => {
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
            list(rd, nv)?;
            list(rd, ne)?;
            rd.flag()?;
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_heal_stitch_p2_eval(
                op.code(),
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim would read out of bounds or that
/// is undefined upstream; the shim's status otherwise.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    block(&mut rd, path)?;
    let block_in = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = raw(Op::Export, path, block_in)?;
    if op == Op::Export {
        rd.done()?;
        return Ok(export);
    }
    let Some((c, _)) = counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    op_args(op, &mut rd, c)?;
    rd.done()?;
    raw(op, path, input)
}
