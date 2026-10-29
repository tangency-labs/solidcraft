//! FFI wrappers for unit `topology.face_p4`.
//!
//! Shim: `harness/cpp/shim/topology_face_p4.cpp` (`sc_topology_face_p4_eval`).
//! Upstream: `source/SMLib/src/SmFace.cpp`.
//!
//! The input is `face args.. brep_block`: a face index (into the traversal-order face list
//! of the B-rep's export), the op's fixed-size arguments, then a B-rep block of
//! [`super::topology_vertexuse`] (validated by that wrapper, which also exports the graph
//! the Rust side imports). The output is `0` then the op record (`-1 status` when the
//! B-rep cannot be built). Entity lists are `n idx*n` (traversal indices). `-2` marks a
//! call the shim skips because upstream would dereference null.
//!
//! | [`Op`]           | args | record |
//! |------------------|------|--------|
//! | `Brep`           | - | `has_brep` (`GetBrep() != NULL`) |
//! | `Edges`          | `tgt_only loop` (`loop` `-1`: no target array) | `-2 \| edges` (`GetEdges`, no attribute) |
//! | `Vertices`       | as `Edges` | `-2 \| vertices` (`GetVertices`) |
//! | `UpLoopuses`     | as `Edges` | `-2 \| loopuses` (`GetUpwardLoopuses`) |
//! | `UpVertexuses`   | as `Edges` | `-2 \| vertexuses` (`GetUpwardVertexuses`) |
//! | `UpEdgeuses`     | as `Edges` | `-2 \| edgeuses` (`GetUpwardEdgeuses`) |
//! | `Connected`      | `kind index` (`kind` an export slot `0..=9`, `-1`: NULL) | `-2 \| connected` (`IsConnectedTo`) |
//! | `Line`           | `p(3) v(3) has_ivl t0 t1 fire face_tol tol` | `-2 \| status n (type s(3) e(3))*n` (`GlobalLineIntersect` over the face's UV domain; `face_tol`: the face's tolerance) |
//! | `Sector`         | `vertex edge use_uv at_start opposite tol validate uv_ok face_tol` | `-2 \| status vertexuse problem cause` (`FindVertexSector` of the edge's curve from its start or end, the primary edge use's UV trim curve with `use_uv`) |
//! | `Face37`         | - | `-2 \| flag` (`sm_AssertTestFace37`; `-2` for a face without surface with two or more loops) |
//! | `Seam`           | `outer periodic_u periodic_v` (`-1`: no known periodicity) | `has crossed missing near_miss class_u class_v` (`HasSeamProblem` without face properties; a class is `tgt_only n (start mid end)*n` point classes) |

use super::topology_vertexuse as fixture;
use crate::{call_with_buffer, FfiError};

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Brep = 1,
    Edges = 2,
    Vertices = 3,
    UpLoopuses = 4,
    UpVertexuses = 5,
    UpEdgeuses = 6,
    Connected = 7,
    Line = 8,
    Sector = 9,
    Seam = 10,
    Face37 = 11,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }

    /// The number of arguments after the face index.
    #[must_use]
    pub fn n_args(self) -> usize {
        match self {
            Self::Brep | Self::Face37 => 0,
            Self::Edges
            | Self::Vertices
            | Self::UpLoopuses
            | Self::UpVertexuses
            | Self::UpEdgeuses
            | Self::Connected => 2,
            Self::Line => 12,
            Self::Sector => 9,
            Self::Seam => 3,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_face_p4_eval(
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

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn index(v: f64, max: usize, what: &str) -> Result<usize, FfiError> {
    if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
        return Err(bad(format!("{what}: bad index {v} (max {max})")));
    }
    Ok(v as usize)
}

fn flag(v: f64, what: &str) -> Result<(), FfiError> {
    index(v, 1, what).map(|_| ())
}

fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: not finite")))
    }
}

/// An index into a list of `n` entities, or `-1` when `allow_none`.
fn entity(v: f64, n: usize, allow_none: bool, what: &str) -> Result<(), FfiError> {
    if allow_none && v == -1.0 {
        return Ok(());
    }
    if index(v, usize::MAX >> 12, what)? >= n {
        return Err(bad(format!("{what} index out of range")));
    }
    Ok(())
}

/// Runs `op` (see the module docs); `path` is the `.smb` file of a file B-rep block
/// (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input or an index outside the B-rep;
/// [`FfiError::Status`] for a shim failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let n = 1 + op.n_args();
    let (head, block) = (
        input.get(..n).ok_or_else(|| bad("input too short"))?,
        input.get(n..).ok_or_else(|| bad("input too short"))?,
    );
    // the topology.vertexuse wrapper validates the block itself and exports the graph
    let export = fixture::eval(fixture::Op::Export, path, block)?;
    let Some(c) = fixture::counts(&export) else {
        // the B-rep could not be built: the op reports the same failure (`-1 status`)
        return Ok(export);
    };
    let [face, args @ ..] = head else {
        return Err(bad("input too short"));
    };
    entity(*face, c[3], false, "face")?;
    let n_l = c[5];
    match (op, args) {
        (Op::Brep | Op::Face37, []) => {}
        (
            Op::Edges | Op::Vertices | Op::UpLoopuses | Op::UpVertexuses | Op::UpEdgeuses,
            [tgt_only, l],
        ) => {
            flag(*tgt_only, "tgt_only")?;
            entity(*l, n_l, true, "loop")?;
        }
        (Op::Connected, [kind, i]) => {
            if *kind != -1.0 {
                let k = index(*kind, 9, "kind")?;
                let count = c.get(k).copied().ok_or_else(|| bad("kind"))?;
                entity(*i, count, false, "target")?;
            }
        }
        (Op::Line, [p @ .., has_ivl, t0, t1, fire, face_tol, tol]) => {
            finite(p, "line")?;
            finite(&[*t0, *t1, *tol], "line interval / tolerance")?;
            flag(*has_ivl, "has_ivl")?;
            flag(*fire, "fire")?;
            flag(*face_tol, "face_tol")?;
            if p.len() != 6 {
                return Err(bad("line point and vector"));
            }
            // a zero line vector divides by zero in the surface intersector
            if p.get(3..6).is_some_and(|v| v.iter().all(|x| *x == 0.0)) {
                return Err(bad("zero line vector"));
            }
        }
        (Op::Sector, [v, e, use_uv, at_start, opposite, tol, validate, uv_ok, face_tol]) => {
            entity(*v, c[9], false, "vertex")?;
            entity(*e, c[7], false, "edge")?;
            flag(*use_uv, "use_uv")?;
            flag(*at_start, "at_start")?;
            flag(*opposite, "opposite")?;
            finite(&[*tol], "tolerance")?;
            flag(*validate, "validate")?;
            flag(*uv_ok, "uv_ok")?;
            flag(*face_tol, "face_tol")?;
        }
        (Op::Seam, [outer, pu, pv]) => {
            flag(*outer, "outer")?;
            for p in [pu, pv] {
                if *p != -1.0 {
                    flag(*p, "periodic")?;
                }
            }
        }
        _ => return Err(bad("wrong argument count")),
    }
    raw(op, path, input)
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_topology_face_p4_eval(
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
