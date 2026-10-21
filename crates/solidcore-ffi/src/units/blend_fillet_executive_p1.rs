//! FFI wrappers for unit `blend.fillet_executive_p1`. Shim:
//! `harness/cpp/shim/blend_fillet_executive_p1.cpp`. Upstream: `SmFilletExecutive.cpp`.
//!
//! [`law_eval`] runs the fillet laws (`SmFilletLaw`, `SmLinearFilletLaw`,
//! `SmBSplineFilletLaw`: constructors, `Evaluate`, `IsLinearLaw`, `GetType`, `IsKindOf`,
//! `GetExtendedCurve`). Its input layout (flat doubles, validated here):
//!
//! ```text
//! kind              0 linear `r0 r1`; 1 B-spline `n p m cpts(x y z w)... knots...`; 2 base
//! has_map e0 e1     edge map (swapped if reversed), used when has_map = 1 and kind != 2
//! ncalls            then per call `t i0 i1 reverse` (the interval, swapped if reversed)
//! nk kinds[nk]      type codes for IsKindOf
//! ```
//!
//! [`object_eval`] runs a script on an `SmFilletErrorInfo` ([`ObjectOp::ErrorScript`]:
//! `exec ncalls call*`, the info built with `exec` as its executive) or on a new
//! `SmFilletExecutive(ctx, target)` ([`ObjectOp::ExecScript`]: `target ncalls call*`).
//! Handles are integers in `[0, MAX_HANDLE]` the shim turns into pointers it never
//! dereferences (`0` is NULL); a text is `len c*` (`len = -1` is NULL; characters
//! `1..=255`). Calls:
//!
//! ```text
//! 0 code text                                             NoteError(eErrorCode, cComment)
//! 1 edge status code text                                 NoteError(pBadEdge, eStatus, ...)
//! 2 code status edge face fillet_edge surface text        NoteError(..., pSideFace, ...)
//! 3 code corner status name vertex s1 s2 fv1 fv2 text     NoteError(..., cCornerName, ...)
//! 4 code status fe_type fillet_edge name text             NoteError(..., cEdgeName, ...)
//! 5 code status edge ns s* nc c* ne e* x y z text         NoteError(..., rTsectPos, ...)
//! 10 flag | 11 flag | 12 h | 13 h | 14 h | 15 h           (ExecScript only) SetDoGlobalMerge,
//!                     SetDoTrimming, SetSelfIntersectionHandler, AddFilletSurfaceGenerator,
//!                     AddTopoEdges, RemoveFromTopoEdges
//! 16 v c | 17 eu s                                        (ExecScript only) insert into
//!                     m_vVertexCornerMap / m_vEdgeuseSolverMap
//! 18 v | 19 eu                                            (ExecScript only) query
//!                     GetFilletCornerOfVertex / GetFilletSolverOfEdgeuse
//! 20 code fe_type status text                             (ExecScript only) a real
//!                     SmFilletEdge of that type and status, then NoteFilletError(code, it, text)
//! ```
//!
//! In an `ExecScript` calls 2, 3 and 5 go through the matching `NoteFilletError`
//! overload.
//!
//! Codes stay inside their enums (`SmFilletErrorType` 0-7, `SmFilStatus` 0-12,
//! `SmFilletCornerType` 0-12, `SmFilletEdgeType` 0-14). Output: `0`, for `ExecScript`
//! first `piecewise insertion global trimming classification target fillet n_corners
//! n_solvers n_topo topo* n_fsg fsg* handler ignore_tangency approx_tol ang_tol
//! has_pseudo has_attribute`, then the error info `code status corner edge_type exec
//! vertex edge side end has_fv [fv1 fv2] ns s* ne e* nc c* has_loc [x y z] comment
//! name` (texts as `len c*`; a handle `-1` is the executive itself, `-2 - k` the k-th
//! fillet edge call 20 made; the fillet vertices and the location only once a call set
//! them, being uninitialised before), and for `ExecScript` last `nq q*`, the query
//! answers.
use crate::units::blend_fillet_standard_solver_p1::{bcurve, Reader, MAX_ARRAY, MAX_CALLS};
use crate::{call_with_buffer, FfiError};

/// Largest handle.
pub const MAX_HANDLE: usize = 1 << 20;
/// Most calls in a script.
pub const MAX_SCRIPT: usize = 64;
/// Most entries of a list argument.
pub const MAX_LIST: usize = 16;
/// Longest text.
pub const MAX_TEXT: usize = 1200;

/// [`object_eval`] operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectOp {
    /// Calls on an `SmFilletErrorInfo`.
    ErrorScript = 0,
    /// Calls on a new `SmFilletExecutive`.
    ExecScript = 1,
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_executive_p1_object_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_executive_p1_law_eval(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Reads an interval `a b` with `|a|, |b| <= MAX_PARAM` (`Reader::param`); a reversed
/// pair is fine, `SmExtent1d(a, b)` swaps it.
fn interval(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.param()?;
    rd.param()?;
    Ok(())
}

fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let rd = &mut rd;
    match rd.index(2)? {
        0 => {
            rd.finite()?;
            rd.finite()?;
        }
        1 => {
            bcurve(rd)?;
        }
        _ => {}
    }
    rd.flag()?;
    interval(rd)?;
    let calls = rd.index(MAX_CALLS)?;
    for _ in 0..calls {
        rd.param()?;
        interval(rd)?;
        rd.flag()?;
    }
    let nk = rd.index(MAX_ARRAY)?;
    for _ in 0..nk {
        rd.index(i32::MAX as usize)?;
    }
    rd.done()
}

/// Builds the law described by `input` (layout in the module docs), evaluates it at every
/// call and returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see (a malformed curve,
/// non-finite or oversized values, too many calls);
/// [`FfiError::Status`] for a shim failure.
pub fn law_eval(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: a well-formed clamped 3D curve, every count, flag and
        // index the shim passes to upstream); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_executive_p1_law_eval(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

fn handle(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.index(MAX_HANDLE).map(|_| ())
}

fn handles(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    for _ in 0..rd.index(MAX_LIST)? {
        handle(rd)?;
    }
    Ok(())
}

/// `len c*`: `-1` (NULL) or up to [`MAX_TEXT`] bytes `1..=255` (no NUL, which would end
/// the C string early).
fn text(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.input.get(rd.at).copied() == Some(-1.0) {
        rd.at += 1;
        return Ok(());
    }
    for _ in 0..rd.index(MAX_TEXT)? {
        if rd.index(255)? == 0 {
            return Err(crate::units::blend_fillet_standard_solver_p1::bad("NUL in text"));
        }
    }
    Ok(())
}

/// One call of the script; `exec` allows the executive calls.
fn call(rd: &mut Reader<'_>, exec: bool) -> Result<(), FfiError> {
    let (code, status, corner, fe_type) = (7, 12, 12, 14);
    match rd.index(20)? {
        0 => {
            rd.index(code)?;
            text(rd)
        }
        1 => {
            handle(rd)?;
            rd.index(status)?;
            rd.index(code)?;
            text(rd)
        }
        2 => {
            rd.index(code)?;
            rd.index(status)?;
            for _ in 0..4 {
                handle(rd)?;
            }
            text(rd)
        }
        3 => {
            rd.index(code)?;
            rd.index(corner)?;
            rd.index(status)?;
            text(rd)?;
            for _ in 0..5 {
                handle(rd)?;
            }
            text(rd)
        }
        4 => {
            rd.index(code)?;
            rd.index(status)?;
            rd.index(fe_type)?;
            handle(rd)?;
            text(rd)?;
            text(rd)
        }
        5 => {
            rd.index(code)?;
            rd.index(status)?;
            handle(rd)?;
            handles(rd)?;
            handles(rd)?;
            handles(rd)?;
            rd.finites(3)?;
            text(rd)
        }
        10 | 11 if exec => rd.flag().map(|_| ()),
        12..=15 | 18 | 19 if exec => handle(rd),
        16 | 17 if exec => {
            handle(rd)?;
            handle(rd)
        }
        20 if exec => {
            rd.index(code)?;
            // Only the types SmFilletEdge::GetName writes a name for; for the others
            // NoteFilletError prints an uninitialised buffer.
            if !matches!(rd.index(fe_type)?, 1 | 3..=7 | 9..=12) {
                return Err(crate::units::blend_fillet_standard_solver_p1::bad(
                    "fillet edge type without a name",
                ));
            }
            rd.index(status)?;
            text(rd)
        }
        _ => Err(crate::units::blend_fillet_standard_solver_p1::bad("bad call kind")),
    }
}

/// Runs the script `input` of `op` (layout in the module docs) and returns the shim's
/// output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see (unknown calls, codes
/// outside their enums, oversized handles, lists or texts, NUL characters, non-finite
/// locations); [`FfiError::Status`] for a shim failure.
pub fn object_eval(op: ObjectOp, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    handle(&mut rd)?;
    for _ in 0..rd.index(MAX_SCRIPT)? {
        call(&mut rd, op == ObjectOp::ExecScript)?;
    }
    rd.done()?;
    call_with_buffer(16384, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: every call, code, count and text the shim passes to
        // upstream; the handles become pointers the calls only store and compare); buf
        // is valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_executive_p1_object_eval(
                op as i32,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
