//! FFI wrappers for unit `heal.stitch_p1`.
//!
//! Shim: `harness/cpp/shim/heal_stitch_p1.cpp` (`sc_heal_stitch_p1_eval`).
//! Upstream: `source/SMLib/src/SmStitch.cpp`, `source/SMLib/inc/SmStitch.h`,
//! `source/SM_API/src/SmApiBrep.cpp`.
//!
//! Every op starts with a B-rep block (the shim builds a fresh `SmBrep` per call):
//! `0` + the B-rep block of [`super::topology_edgeuse_p1`] (`.smb` file, wire edge,
//! face, `prog_test` wire network); `1 n (surface u0 v0 u1 v1)*n` with `1 <= n <=`
//! [`MAX_FACES`], one face per surface in one B-rep (unstitched sheets); or
//! `2 P[3] l w h` (`SmApiCreateBox`). Indices refer to the export's traversal order; a
//! target is `ty idx` with `ty` an `SmTopoTypes.h` code (vertex, edge or face). Every
//! output starts with `0`, or with `-1 status` when the B-rep could not be built
//! (nothing else follows). `-2` in a status slot is "upstream would dereference null or
//! free twice": the shim did not call it.
//!
//! | [`Op`]      | parameters after the block                          | outputs |
//! |-------------|-----------------------------------------------------|---------|
//! | `Export`    |                                                     | the `Export` of unit `topology.brep_p1` (its extras, then the graph) |
//! | `Select`    | `ty1 idx1 ty2 idx2 dist`                            | `status [keep_first max_distance]` (`SmStitchCallback::SelectTopologyToKeep`) |
//! | `Interior`  | `edge t tol`                                        | `status [interior]` (`sm_CheckInteriorEdge`) |
//! | `Split`     | `edge P[3] tol`                                     | `status [reason dist split [digest]]` (`sm_TestAndSplitEdge`) |
//! | `VPairs`    | `tol ignore n1 v*n1 n2 v*n2`                        | `status [max_gap min_gap processed deleted list1 list2 [digest]]` (`StitchVertexPairs`; lists `n idx*n`, `-1` for null) |
//! | `Edges`     | `tol manifold fast ignore nesting n v*n`            | `status [max_gap min_gap glued deleted [digest]]` (`StitchEdgesOfVertices`) |
//! | `Simple`    | `api tol nk f*nk nd f*nd`                           | `status [max_vertex_gap max_edge_gap stitched digest]` (`DoSimpleFaceStitch`, or `SmApiSimpleFaceStitch` with the B-rep tolerance) |
//! | `Unify`     | `api tol face n_samples`                            | `status [flipped (u0 v0 u1 v1)*n]` (`UnifyNormals`, or `SmApiUnifyNormals`) |
//! | `Api`       | `which null_brep ..`                                | which 0 `SmApiStitch(brep)`: `status`; 1 `face+1` `SmApiStitch(brep, face)` (only with a null B-rep or face: otherwise `SmMerge`'s constructor dereferences the null other B-rep, UB-167): `status`; 2 `SmApiStitchIntoSolid`: `status [solid n vgap egap]`; 3 `well_formed ratio` `SmApiStitchIntoShell`: `status [n vgap egap]`; 4 `face+1 n` `SmApiUnifyNormals`: `status [flipped]`; 5 `SmApiSimpleFaceStitch` (empty lists): `status`; 6 `tol squeeze split manifold slivers` `SmApiStitchAdvanced`: `status [n lamina vgap egap]` |
//!
//! A digest is the B-rep after a mutating op: `n_faces n_edges n_vertices` then each
//! vertex point in list order. `Simple` reports `-2` when the face lists are equally
//! long and non-empty and a face repeats in the delete list or appears in both lists, a
//! vertex is shared by both lists, or a vertex of the faces to delete has no other
//! face (upstream frees it, then glues it).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: the B-rep
//! block rules, indices inside the B-rep's export (it builds the B-rep once more to
//! count), `StitchVertexPairs` lists without repeats or shared vertices (a vertex glued
//! twice is freed twice), finite values, sample and list counts up to [`MAX_LIST`].

use crate::units::topology_brep_p1::counts;
use crate::units::topology_edgeuse_p1::{bad, brep_block, face_block, Reader, MAX_LIST};
use crate::{call_with_buffer, FfiError};

/// Most faces of an unstitched-sheets block.
pub const MAX_FACES: usize = 8;

/// `SmTopoTypes.h` codes of the targets: vertex, edge, face.
pub const TY_VERTEX: i64 = 16021;
/// `SmEdge_TYPE`.
pub const TY_EDGE: i64 = 16016;
/// `SmFace_TYPE`.
pub const TY_FACE: i64 = 16002;

/// Most `SmApi*` entry points (`which` of `Api`).
pub const MAX_API: usize = 6;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Select = 1,
    Interior = 2,
    Split = 3,
    VPairs = 4,
    Edges = 5,
    Simple = 6,
    Unify = 7,
    Api = 8,
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
        pub fn sc_heal_stitch_p1_eval(
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

/// A stitch B-rep block (shared with unit `heal.stitch_p2`).
pub(crate) fn block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    match rd.index(2)? {
        0 => brep_block(rd, path),
        1 => {
            let n = rd.index(MAX_FACES)?;
            if n == 0 {
                return Err(bad("no faces"));
            }
            for _ in 0..n {
                // a sheet face's plane domain is bounded as in the face block's non-export ops
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

/// Entity counts of the export: `(vertices, edges, faces)`.
fn vef(c: [usize; 10]) -> (usize, usize, usize) {
    (c[9], c[7], c[3])
}

fn target(rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let (nv, ne, nf) = vef(c);
    let ty = rd.num()?;
    let n = if ty == TY_VERTEX as f64 {
        nv
    } else if ty == TY_EDGE as f64 {
        ne
    } else if ty == TY_FACE as f64 {
        nf
    } else {
        return Err(bad("target type must be a vertex, edge or face"));
    };
    rd.below(n)?;
    Ok(())
}

/// `n idx*n` indices below `count`, each once; returns them.
fn list(rd: &mut Reader<'_>, count: usize) -> Result<Vec<usize>, FfiError> {
    let n = rd.index(MAX_LIST)?;
    let mut v = Vec::new();
    for _ in 0..n {
        let i = rd.below(count)?;
        if v.contains(&i) {
            return Err(bad("index repeated"));
        }
        v.push(i);
    }
    Ok(v)
}

/// `n idx*n` indices below `count` (repeats allowed).
fn any_list(rd: &mut Reader<'_>, count: usize) -> Result<(), FfiError> {
    let n = rd.index(MAX_LIST)?;
    for _ in 0..n {
        rd.below(count)?;
    }
    Ok(())
}

fn op_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let (nv, ne, nf) = vef(c);
    match op {
        Op::Export => {}
        Op::Select => {
            target(rd, c)?;
            target(rd, c)?;
            rd.finite()?;
        }
        Op::Interior => {
            rd.below(ne)?;
            rd.finite()?;
            rd.finite()?;
        }
        Op::Split => {
            rd.below(ne)?;
            rd.vec3()?;
            rd.finite()?;
        }
        Op::VPairs => {
            rd.finite()?;
            rd.flag()?;
            let a = list(rd, nv)?;
            let b = list(rd, nv)?;
            if a.iter().any(|i| b.contains(i)) {
                return Err(bad("a vertex in both lists"));
            }
        }
        Op::Edges => {
            rd.finite()?;
            for _ in 0..4 {
                rd.flag()?;
            }
            any_list(rd, nv)?;
        }
        Op::Simple => {
            rd.flag()?;
            rd.finite()?;
            any_list(rd, nf)?;
            any_list(rd, nf)?;
        }
        Op::Unify => {
            rd.flag()?;
            rd.finite()?;
            rd.below(nf)?;
            rd.index(MAX_LIST)?;
        }
        Op::Api => {
            let which = rd.index(MAX_API)?;
            let null_brep = rd.flag()?;
            match which {
                1 => {
                    // SmApiStitch(brep, face) builds SmMerge(ctx, brep, NULL) with a zero
                    // tolerance, whose constructor dereferences the null other B-rep
                    // (UB-167): with a B-rep and a face it always crashes upstream, so
                    // only the null guards run
                    let face = rd.index(nf)?;
                    if !null_brep && face != 0 {
                        return Err(bad("SmApiStitch(brep, face): SmMerge dereferences the null other B-rep (UB-167)"));
                    }
                }
                3 => {
                    rd.flag()?;
                    rd.finite()?;
                }
                4 => {
                    rd.index(nf)?;
                    rd.index(MAX_LIST)?;
                }
                6 => {
                    rd.finite()?;
                    for _ in 0..4 {
                        rd.flag()?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_heal_stitch_p1_eval(
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
    // HANG-30: the full upstream stitches behind SmApiStitch / StitchIntoSolid /
    // StitchIntoShell / StitchAdvanced never return on sheets with coordinates near the
    // top of the double range (a control point at -1e300); bound every block value there
    if op == Op::Api && input.first() == Some(&1.0) {
        let which = input.get(block_in.len()).copied().unwrap_or(-1.0);
        if [0.0, 2.0, 3.0, 6.0].contains(&which)
            && block_in.iter().any(|v| v.abs() > MAX_STITCH_COORD)
        {
            return Err(bad("full stitch on a sheet value beyond 1e150 (HANG-30)"));
        }
    }
    raw(op, path, input)
}

/// The largest block value the full upstream stitches are given (HANG-30).
pub const MAX_STITCH_COORD: f64 = 1e150;
