//! FFI wrappers for unit `topology.loopuse`.
//!
//! Shim: `harness/cpp/shim/topology_loopuse.cpp` (`sc_topology_loopuse_eval`).
//! Upstream: `source/SMLib/src/SmLoop.cpp`, `source/SMLib/src/SmLoopuse.cpp`.
//!
//! Every op starts with a B-rep block of unit `topology.edgeuse_p1` (see
//! [`super::topology_edgeuse_p1`]; its `Export` op exports the graph). Indices refer to
//! the export's traversal order; a target is `type index` (an `SmTopoTypes.h` code and an
//! index of that kind, `-1` or an unknown code for null). Outputs start with `0`, or with
//! `-1 status` when the B-rep could not be built. In an output slot `-2` is "upstream
//! would dereference null" and `-3` "not run" (the Rust side defers there). A list is
//! `n i*n`; a list slot is the list or `-2`.
//!
//! | [`Op`]          | parameters after the block                   | outputs |
//! |-----------------|----------------------------------------------|---------|
//! | `LuQuery`       | `lu`                                         | `vu fu loop orient mate start_type start lu_type is_vertex is_edge shell closed_ptrs type kind_of*8 type_string class_string`, then the lists edges, edge uses, vertex uses, vertices |
//! | `LuLists`       | `lu use_attr n (type index)*n conn_type conn_index` | the four lists with the attribute filter (attribute 777 attached to the `n` objects) and the connection target |
//! | `LuConnected`   | `lu type index`                              | `IsConnectedTo` |
//! | `LoopQuery`     | `l`                                          | `lu face lu1 lu2 outer closed_ptrs type class_type kind_of*8 type_string class_string`, then the lists vertices, edges, edge uses |
//! | `LoopConnected` | `l type index`                               | `IsConnectedTo` |
//! | `Closed3d`      | `of_loop index check_all want_eu want_gap want_tol set_tol edge_tol` (`set_tol`: every edge's `m_sZoneTol3d = edge_tol` first) | `closed eu gap xsect_tol` (unwritten out-params: `-5`, `-7`, `777`) or `-2` |
//! | `MissingSeam`   | `lu`                                         | `IsMissingSeamAtVertices` or `-2` |
//! | `UvCorners`     | `l u0 v0 u1 v1 tol`                          | `boundaries` and the lists edges, sides, then `n params*n`; or `-2` / `-3` |
//! | `Flip`          | `l`                                          | `status` and the use state, or `-2` |
//! | `Collect`       | `lu eu`                                      | `status` and the use state |
//! | `SetEdgeuse`    | `lu eu`                                      | `status` and the use state |
//! | `SetOrient`     | `lu orientation`                             | `orientation` and the use state |
//! | `SetLoop`       | `lu loop`                                    | `loop` and the use state |
//! | `FaceFu`        |                                              | each face's `m_pFU` (a list; the export lacks it) |
//!
//! The use state is, in the export's order, per loop use `type start orientation loop
//! faceuse`, per edge use `owner_type owner`, per vertex use `owner_type owner`.
//!
//! [`eval`] accepts what [`super::topology_edgeuse_p1::eval`] accepts for the block,
//! indices inside the export (`-1` where a null argument is allowed), finite domains and
//! tolerances, orientations `0..=2` and at most [`MAX_ATTRIBUTED`] attributed objects.

use super::topology_edgeuse_p1::{self as p1, bad, brep_block, Reader};
use crate::{call_with_buffer, FfiError};

/// Largest number of objects `LuLists` attaches the attribute to.
pub const MAX_ATTRIBUTED: usize = 256;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    LuQuery = 1,
    LuLists = 2,
    LuConnected = 3,
    LoopQuery = 4,
    LoopConnected = 5,
    Closed3d = 6,
    MissingSeam = 7,
    UvCorners = 8,
    Flip = 9,
    Collect = 10,
    SetEdgeuse = 11,
    SetOrient = 12,
    SetLoop = 13,
    FaceFu = 14,
}

mod sys {
    extern "C" {
        pub fn sc_topology_loopuse_eval(
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

/// `SmTopoTypes.h` codes of the kinds a target may be, with their export slot.
const TARGET_TYPES: [(i64, usize); 10] = [
    (16012, 0), // SmRegion_TYPE
    (16013, 1), // SmShell_TYPE
    (16022, 2), // SmFaceuse_TYPE
    (16002, 3), // SmFace_TYPE
    (16004, 4), // SmLoopuse_TYPE
    (16003, 5), // SmLoop_TYPE
    (16005, 6), // SmEdgeuse_TYPE
    (16016, 7), // SmEdge_TYPE
    (16006, 8), // SmVertexuse_TYPE
    (16021, 9), // SmVertex_TYPE
];

/// A target `type index`: a known kind with an index inside the export or `-1`, or any
/// other integer code (null).
fn target(rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let t = rd.num()?;
    let i = rd.num()?;
    if i.fract() != 0.0 || i < -1.0 {
        return Err(bad("bad target index"));
    }
    if let Some(&(_, slot)) = TARGET_TYPES.iter().find(|(code, _)| *code as f64 == t) {
        let n = c.get(slot).copied().unwrap_or(0);
        if i >= 0.0 && i as usize >= n {
            return Err(bad("target index out of range"));
        }
    } else if t.fract() != 0.0 || !(-1.0..=1.0e6).contains(&t) {
        return Err(bad("bad target type"));
    }
    Ok(())
}

fn args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let [_, _, _, _, n_lu, n_l, n_eu, _, _, _] = c;
    match op {
        Op::LuQuery | Op::MissingSeam => {
            rd.below(n_lu)?;
        }
        Op::LuLists => {
            rd.below(n_lu)?;
            rd.flag()?;
            let n = rd.index(MAX_ATTRIBUTED)?;
            for _ in 0..n {
                target(rd, c)?;
            }
            target(rd, c)?;
        }
        Op::LuConnected => {
            rd.below(n_lu)?;
            target(rd, c)?;
        }
        Op::LoopQuery | Op::Flip => {
            rd.below(n_l)?;
        }
        Op::LoopConnected => {
            rd.below(n_l)?;
            target(rd, c)?;
        }
        Op::Closed3d => {
            let of_loop = rd.flag()?;
            rd.below(if of_loop { n_l } else { n_lu })?;
            for _ in 0..5 {
                rd.flag()?;
            }
            rd.finites(1)?;
        }
        Op::UvCorners => {
            rd.below(n_l)?;
            rd.finites(5)?;
        }
        Op::Collect | Op::SetEdgeuse => {
            rd.below(n_lu)?;
            rd.below_or_none(n_eu)?;
        }
        Op::SetOrient => {
            rd.below(n_lu)?;
            rd.index(2)?;
        }
        Op::SetLoop => {
            rd.below(n_lu)?;
            rd.below_or_none(n_l)?;
        }
        Op::FaceFu => {}
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
            sys::sc_topology_loopuse_eval(
                op as i32,
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
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = p1::eval(p1::Op::Export, path, block)?;
    let Some(c) = p1::counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    args(op, &mut rd, c)?;
    rd.done()?;
    raw(op, path, input)
}

mod sys_topology_loopuse_side_in_surface {
    extern "C" {
        pub fn sc_topology_loopuse_side_in_surface(
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// `SmLoopuse::IsLoopuseSideInSurface(periodic_u, periodic_v)` (`-1`: NULL) of every loop
/// use of the B-rep built from `block` (shim `topology_loopuse_side.cpp`):
/// `0 n (side on_boundary index)*n`, `-2 -2 -2` where upstream dereferences null.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed argument or B-rep block (the checks of
/// [`super::topology_vertexuse::validate_block`], the whole input consumed);
/// [`FfiError::Status`] for a shim failure.
pub fn side_in_surface(path: &str, periodic_u: f64, periodic_v: f64, block: &[f64]) -> Result<Vec<f64>, FfiError> {
    for p in [periodic_u, periodic_v] {
        if p != -1.0 && p != 0.0 && p != 1.0 {
            return Err(FfiError::InvalidArgument("periodic flag must be -1, 0 or 1".into()));
        }
    }
    super::topology_vertexuse::validate_block(path, block)?;
    if super::topology_vertexuse::brep_block_at(block, path)? != block.len() {
        return Err(FfiError::InvalidArgument("trailing values after the B-rep block".into()));
    }
    let mut input: Vec<f64> = vec![periodic_u, periodic_v];
    input.extend_from_slice(block);
    let cpath = std::ffi::CString::new(path)
        .map_err(|_| FfiError::InvalidArgument("path has a NUL byte".into()))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for input.len()
        // reads and holds the validated arguments then a validated B-rep block (exactly
        // what the shim reads); buf is valid for buf.len() writes and the shim honours
        // the capacity; len is valid.
        unsafe {
            sys_topology_loopuse_side_in_surface::sc_topology_loopuse_side_in_surface(
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
