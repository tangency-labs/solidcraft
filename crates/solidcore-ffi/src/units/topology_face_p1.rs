//! FFI wrappers for unit `topology.face_p1`.
//!
//! Shim: `harness/cpp/shim/topology_face_p1.cpp` (`sc_topology_face_p1_eval`).
//! Upstream: `source/SMLib/src/SmFace.cpp`.
//!
//! The input is `face args.. brep_block`: a face index (into the traversal-order face list
//! of the B-rep's export), the op's fixed-size arguments, then a B-rep block of
//! [`super::topology_vertexuse`] (validated by that wrapper, which also exports the graph
//! the Rust side imports). The output is `0` then the op record (`-1 status` when the
//! B-rep cannot be built). A curve is `dim t0 t1 (x y z)*5` (samples at `t0 + i/4 (t1 -
//! t0)`), or `-1`; a domain `u0 v0 u1 v1`. "orientation" is, per face, its upward face use;
//! per face use its orientation, loop use count and loop uses; per loop its primary loop
//! use. `-2` marks a call the shim skips because upstream would dereference null. A seam
//! call that can reach upstream's `SplitAtSeam` crash (UB-137) runs in a forked child:
//! its record is `-4` when the child crashed, `-3` when the shim could not fork.
//!
//! | [`Op`]          | args | record |
//! |-----------------|------|--------|
//! | `UvDomain`      | `limit want_max want_2nd` | `status domain gap gap2` (`CalculateUVDomainFromUVTrimCurves`) |
//! | `UvDomainAfterTrimShift` | `limit want_max want_2nd u0 v0 u1 v1 du dv` | as `UvDomain`, after setting the face domain and translating its real UV trims |
//! | `Oriented`      | `param fill loop` (`loop` `-1` none) | `-2 \| status nl counts*nl nc curve*nc` (`CreateOrientedTrimBoundary`) |
//! | `Rect`          | - | `status domain rect natural` (`FindIfRectangularTrim`) |
//! | `Flip`          | - | `status orientation` (`FlipFaceOrientation`) |
//! | `SetSurface`    | `delete` | `-2 \| 0 domain owned` (`SetSurface` of a copy of the face's surface) |
//! | `LoopRoundTrip` | `loop` | `-2 \| remove_status insert_status orientation` (`RemoveLoop`, `InsertLoop`) |
//! | `RemoveUv`      | - | `-2 \| status curve*n_eu` (`RemoveUVTrimCurves`) |
//! | `SwapUv`        | - | `-2 \| status domain orientation curve*n_eu` (`SwapUV`) |
//! | `Shrink`        | - | `status domain (surface_domain \| -1)` (`ShrinkGeometry`) |
//! | `MoveSeam`      | `props crossed missing near closed_u closed_v center split` | `-2 \| -3 \| -4 \| seam status res_u res_v n_faces n_edges n_vertices` |
//! | `SplitAtSeam`   | as `MoveSeam` | `-2 \| -3 \| -4 \| seam status n_faces n_edges n_vertices` |
//!
//! The seam ops classify the face's seams with `HasSeamProblem` first, record what it
//! found (`seam` = `has crossed missing near_miss class_u class_v`, a class being
//! `tgt_only n mid_class*n`) and pass its classifications and enums on; with `props` an `SmFaceProps` carrying the given
//! `m_eBadCrossedSeam`, `m_eBadMissingSeam`, `m_eBadNearMissSeam` (`SmSurfParamType`
//! values) and `m_bClosedU` / `m_bClosedV` (`SmBoolean`) is passed too.

use super::topology_vertexuse as fixture;
use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    UvDomain = 1,
    Oriented = 2,
    Rect = 3,
    Flip = 4,
    SetSurface = 5,
    LoopRoundTrip = 6,
    RemoveUv = 7,
    SwapUv = 8,
    Shrink = 9,
    MoveSeam = 11,
    SplitAtSeam = 12,
    UvDomainAfterTrimShift = 13,
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
            Self::UvDomainAfterTrimShift => 9,
            Self::UvDomain | Self::Oriented => 3,
            Self::SetSurface | Self::LoopRoundTrip => 1,
            Self::MoveSeam | Self::SplitAtSeam => 8,
            _ => 0,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_face_p1_eval(
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

fn index(v: f64, max: usize, what: &str) -> Result<usize, FfiError> {
    if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
        return Err(bad(format!("{what}: bad index {v} (max {max})")));
    }
    Ok(v as usize)
}

fn flag(v: f64, what: &str) -> Result<(), FfiError> {
    index(v, 1, what).map(|_| ())
}

/// `SmSurfParamType` values (`SM_SP_U` .. `SM_SP_OUT`).
fn surf_param(v: f64) -> Result<(), FfiError> {
    index(v, 9, "SmSurfParamType").map(|_| ())
}

/// `SmBoolean` values `FALSE`, `TRUE`, `UNSURE`.
fn tri(v: f64) -> Result<(), FfiError> {
    index(v, 2, "SmBoolean").map(|_| ())
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
    let [_, _, _, n_f, _, n_l, ..] = c;
    let [face, args @ ..] = head else {
        return Err(bad("input too short"));
    };
    if index(*face, usize::MAX >> 12, "face")? >= n_f {
        return Err(bad("face index out of range"));
    }
    match (op, args) {
        (Op::UvDomain, [a, b, c]) => {
            flag(*a, "limit")?;
            flag(*b, "want_max")?;
            flag(*c, "want_2nd")?;
        }
        (Op::UvDomainAfterTrimShift, [a, b, c, u0, v0, u1, v1, du, dv]) => {
            flag(*a, "limit")?;
            flag(*b, "want_max")?;
            flag(*c, "want_2nd")?;
            if [u0, v0, u1, v1, du, dv].iter().any(|x| !x.is_finite()) || u0 > u1 || v0 > v1 {
                return Err(bad("invalid trim-shift preparation"));
            }
        }
        (Op::Oriented, [a, b, l]) => {
            flag(*a, "param")?;
            flag(*b, "fill")?;
            if *l != -1.0 && index(*l, usize::MAX >> 12, "loop")? >= n_l {
                return Err(bad("loop index out of range"));
            }
        }
        (Op::SetSurface, [d]) => flag(*d, "delete")?,
        (Op::LoopRoundTrip, [l]) => {
            if index(*l, usize::MAX >> 12, "loop")? >= n_l {
                return Err(bad("loop index out of range"));
            }
        }
        (Op::MoveSeam | Op::SplitAtSeam, [p, c, m, nm, cu, cv, center, split]) => {
            flag(*p, "props")?;
            surf_param(*c)?;
            surf_param(*m)?;
            surf_param(*nm)?;
            tri(*cu)?;
            tri(*cv)?;
            flag(*center, "center")?;
            flag(*split, "split")?;
        }
        (Op::Rect | Op::Flip | Op::RemoveUv | Op::SwapUv | Op::Shrink, []) => {}
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
            sys::sc_topology_face_p1_eval(
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
