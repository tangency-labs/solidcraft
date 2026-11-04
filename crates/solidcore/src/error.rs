//! Error type shared by every solidcore operation.
//!
//! Upstream: `source/SMLib/inc/SmMessages.h` (`SmStatus`, `SM_SUCCESS`, `SM_ERR_*`
//! codes and the `SE`/`SER`/`NER` error-propagation macros).
//!
//! | Rust                        | Upstream                                         |
//! |-----------------------------|--------------------------------------------------|
//! | [`SolidError`]              | non-`SM_SUCCESS` `SmStatus` values               |
//! | [`SolidError::InvalidInput`]| `SM_ERR_INVALID_INPUT`                           |
//! | [`SolidError::Upstream`]    | any other `SM_ERR_*` code, carried numerically   |
//! | [`SolidError::PortInvariant`] | none: a port-side invariant or adapter failure  |
//! | [`SolidError::sm_status`]   | the `SmStatus` an error stands for, when exact    |
//! | [`SM_ERR`], [`SM_ERR_WARNING`], [`SolidError::sm_err`] | `SM_ERR`, `SM_ERR_WARNING`, `SE(SM_ERR)` |
//! | [`SM_SUCCESS`] .. [`SM_ERR_TESS_FAIL_FACE`] | every `SmStatus` number in `SmMessages.h` (the one definition; other modules re-export these) |
//! | [`SM_ERR_OFFSET_WALLS_TOUCH`], [`SM_ERR_OFFSET_EDGE_VANISHES`], [`SolidError::is_offset_edge_vanishes`] | the fork's `SmMessages.h` statuses 1042, 1043 |
//! | [`SM_TRUE`], [`SM_FALSE`], [`SM_UNSURE`] | `TRUE`, `FALSE`, `UNSURE` of an `SmBoolean` (`SmTypes.h`) |
//! | [`SM_ERR_NULL_POINTER`], [`SolidError::null_pointer`] | `SM_ERR_NULL_POINTER`, `NER(...)` on `NULL` |
//! | [`SolidError::Nlib`], [`SolidError::nlib`] | NLib error flag + `NL_ERROR.eno` (`N_ErrSet`) |
//! | [`SolidError::Unported`], [`SolidError::unported`], [`SolidError::is_unported`] | (none: call into a unit not ported yet) |
//! | [`SolidError::Cancelled`], [`SolidError::cancelled`], [`SolidError::is_cancelled`] | (none: cooperative cancellation on [`crate::Kernel`]) |
//! | [`SolidError::invalid_input`] | (none: out-of-range reads / null dereferences upstream) |
//! | `ignored`, `ignore_status` (crate) | a call whose `SmStatus` upstream ignores    |
//! | `checked_or_ignored` (crate) | per-call opt-in status policy (#483); legacy helpers unchanged |
//! | `native_failure` (crate)  | a call whose failing `SmStatus` upstream handles (internal errors propagate) |
//! | [`NlibCode`]                | `NL_*_ERR` numbers (`source/SMLib/inc/NL_DataStruct.h`) |
//! | `?` operator                | `SER(...)` / `NER(...)` (return on error)        |
//! | [`SolidError::Failed`], [`Failure`], [`Reason`] | none: a failure in solidcore's own terms (#2615) |
//! | [`ErrorKind`], [`SolidError::kind`] | none: solidcore's stable error codes (#2615) |
//! | [`offset::OffsetFailure`]   | none: the offset subsystem's refusals (#2615)    |
//!
//! NLib routines (`NL_*.cpp`) report failure by returning an error flag and storing an
//! error number in the process-wide `NL_ERROR` record; solidcore returns
//! [`SolidError::Nlib`] carrying that number as an [`NlibCode`] instead.
//!
//! Every variant names the `operation` (a `&'static str`, by convention the Rust path
//! of the failing function, e.g. `"geom::vector3d::Vec3::unitize"`) so that errors surfaced to
//! callers stay actionable without a backtrace.
//!
//! # Errors in solidcore's own terms (#2615)
//!
//! solidcore has no upstream: its errors are its own. [`SolidError::Failed`] is the
//! error a converted subsystem returns. Its [`Failure`] says what failed in solidcore's
//! terms and never shows a C++ status number, type or symbol:
//!
//! - `operation`: the solidcore path of the failing stage
//!   (`"offset::offset_executive::crossing_loops::resolve_crossing_loops"`);
//! - `reason`: a typed [`Reason`] naming what failed, on which entity, with the key
//!   values (a size, a distance, a gap, a tolerance). Each subsystem's reasons are one
//!   enum in a submodule of this module ([`offset::OffsetFailure`]), so the taxonomy
//!   stays in one place;
//! - `entity`: the entity the failure is about, in the caller's B-rep where there is one;
//! - `cause`: the error of the stage this failure comes from. Where a caller turns a
//!   stage's failure into a generic one, the stage's error is kept here, so the caller
//!   still sees it (#2511).
//!
//! [`SolidError::kind`] gives every error a stable solidcore [`ErrorKind`] (a number and
//! a name). Callers match on it and on the [`Reason`], never on message text or a
//! status: each reason carries everything a caller needs (the sizes, the entities, the
//! distinctions between cases). [`SolidError::is_port_failure`] tells a solidcore defect
//! or missing port from a failure of the operation on its input (#1452).
//!
//! **Comparison with the pinned C++.** difftest compares each failure with the status
//! the pinned C++ reports for the same input. A failure records that status privately,
//! where it is made (the status depends on the call path, not only on the reason), and
//! [`SolidError::sm_status`] is the one hook that reads it, for difftest (and the FFI
//! side) only. It is not a public field, never part of a message, and not for callers.
//!
//! **Retiring "upstream".** The goal is no [`SolidError::Upstream`] (nor `Nlib`,
//! `FilletRefused`) variant and no "upstream status" text in the public API. They are
//! the pre-#2615 shape: subsystems move their sites to [`SolidError::Failed`] one PR at
//! a time (offset first), and the variants go when their last site does. Then
//! `sm_status` and the status numbers here move to a comparison-only module (or to
//! difftest and the FFI), since nothing but the comparison needs them.

pub mod offset;

use crate::topology::{
    BrepId, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, RegionId, ShellId, VertexId,
    VertexuseId,
};

/// Shorthand for `core::result::Result<T, SolidError>`.
pub type Result<T, E = SolidError> = core::result::Result<T, E>;

/// The value of a call whose failing status upstream ignores: `Some` on success,
/// `None` on failure; unported dependencies, cooperative cancellations, and preparation
/// requests still propagate.
pub(crate) fn ignored<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e)
            if e.is_unported_or_cancelled()
                || matches!(e, SolidError::PreparationRequired { .. }) =>
        {
            Err(e)
        }
        Err(_) => Ok(None),
    }
}

/// The value of a call whose failing status upstream handles as an ordinary failure
/// (e.g. a missed ray it retries): `Some` on success, `None` on an error that may stand
/// for an upstream status. Errors with no upstream status propagate, so a port defect
/// cannot pass for that failure: [`SolidError::Unported`], [`SolidError::Cancelled`],
/// [`SolidError::PreparationRequired`], [`SolidError::PortInvariant`] (an owner, handle
/// or identity the port could not resolve) and [`SolidError::NotFound`] (a stale typed
/// id, where upstream follows a pointer).
pub(crate) fn native_failure<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(
            e @ (SolidError::Unported { .. }
            | SolidError::Cancelled { .. }
            | SolidError::PreparationRequired { .. }
            | SolidError::PortInvariant { .. }
            | SolidError::FaceTrimsUnavailable { .. }
            | SolidError::NotFound { .. }),
        ) => Err(e),
        Err(_) => Ok(None),
    }
}

/// The status of a call upstream ignores: `Ok` for any result but
/// [`SolidError::Unported`], [`SolidError::Cancelled`], or
/// [`SolidError::PreparationRequired`].
pub(crate) fn ignore_status<T>(r: Result<T>) -> Result<()> {
    ignored(r).map(|_| ())
}

/// Select the status policy of one audited call. Checked mode returns the original
/// error before its output is read; legacy mode preserves `ignored` unchanged.
pub(crate) fn checked_or_ignored<T>(r: Result<T>, checked: bool) -> Result<Option<T>> {
    if checked {
        r.map(Some)
    } else {
        ignored(r)
    }
}

/// One call whose status SMLib drops and the fork passes on (`SER(...)`): with `pass_on`
/// its error is returned to the caller (through the outer `?`); without it the result
/// is handed back unchanged for the site's own ignore policy, so
/// `ignore_status(passed_on(r, false)?)` is `ignore_status(r)`.
pub(crate) fn passed_on<T>(r: Result<T>, pass_on: bool) -> Result<Result<T>> {
    match r {
        Err(e) if pass_on || e.is_unported_or_cancelled() => Err(e),
        r => Ok(r),
    }
}

/// [`ignore_status`] of a call whose status SMLib drops and the fork passes on: with
/// `pass_on` its error is returned, without it the status is ignored as upstream.
pub(crate) fn ignore_or_pass_on<T>(r: Result<T>, pass_on: bool) -> Result<()> {
    ignore_status(passed_on(r, pass_on)?)
}

/// [`ignored`] of a call whose status SMLib drops and the fork passes on: with
/// `pass_on` its error is returned, without it the value is `None` as upstream.
pub(crate) fn ignored_or_pass_on<T>(r: Result<T>, pass_on: bool) -> Result<Option<T>> {
    ignored(passed_on(r, pass_on)?)
}

/// A status a port helper hands back in its value (`None` for `SM_SUCCESS`) where SMLib
/// drops it and the fork passes it on (`SER(...)`): returned as the error with
/// `pass_on`, dropped without (except [`SolidError::Unported`] and
/// [`SolidError::Cancelled`], which always propagate).
pub(crate) fn status_passed_on(status: Option<SolidError>, pass_on: bool) -> Result<()> {
    match status {
        Some(e) if pass_on || e.is_unported_or_cancelled() => Err(e),
        _ => Ok(()),
    }
}

/// A reference to one topological entity, used as error context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityRef {
    /// A whole B-rep.
    Brep(BrepId),
    /// A shell.
    Shell(ShellId),
    /// A face.
    Face(FaceId),
    /// A loop (face boundary).
    Loop(LoopId),
    /// An edge.
    Edge(EdgeId),
    /// A vertex.
    Vertex(VertexId),
    /// A region.
    Region(RegionId),
    /// A face use.
    Faceuse(FaceuseId),
    /// A loop use.
    Loopuse(LoopuseId),
    /// An edge use.
    Edgeuse(EdgeuseId),
    /// A vertex use.
    Vertexuse(VertexuseId),
}

impl core::fmt::Display for EntityRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Brep(id) => write!(f, "brep {}", id.index()),
            Self::Shell(id) => write!(f, "shell {}", id.index()),
            Self::Face(id) => write!(f, "face {}", id.index()),
            Self::Loop(id) => write!(f, "loop {}", id.index()),
            Self::Edge(id) => write!(f, "edge {}", id.index()),
            Self::Vertex(id) => write!(f, "vertex {}", id.index()),
            Self::Region(id) => write!(f, "region {}", id.index()),
            Self::Faceuse(id) => write!(f, "faceuse {}", id.index()),
            Self::Loopuse(id) => write!(f, "loopuse {}", id.index()),
            Self::Edgeuse(id) => write!(f, "edgeuse {}", id.index()),
            Self::Vertexuse(id) => write!(f, "vertexuse {}", id.index()),
        }
    }
}

/// Errors returned by solidcore operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum SolidError {
    /// An argument was out of range, malformed or inconsistent
    /// (upstream `SM_ERR_INVALID_INPUT`).
    #[error("{operation}: invalid input: {reason}")]
    InvalidInput {
        /// Operation that rejected the input.
        operation: &'static str,
        /// Human-readable description of what was wrong.
        reason: &'static str,
    },

    /// A typed id did not refer to a live entity of the B-rep it was used with.
    #[error("{operation}: {entity} not found")]
    NotFound {
        /// Operation that looked the entity up.
        operation: &'static str,
        /// The missing entity.
        entity: EntityRef,
    },

    /// Geometry was degenerate where a non-degenerate value was required
    /// (zero-length vector, collapsed edge, singular matrix, ...).
    #[error("{operation}: degenerate geometry{}", fmt_entity(*.entity))]
    Degenerate {
        /// Operation that hit the degeneracy.
        operation: &'static str,
        /// Entity involved, when known.
        entity: Option<EntityRef>,
    },

    /// An iterative solver did not converge.
    #[error("{operation}: did not converge after {iterations} iterations")]
    NotConverged {
        /// Operation whose solver failed.
        operation: &'static str,
        /// Iterations performed before giving up.
        iterations: u32,
    },

    /// The operation is not supported for this combination of inputs
    /// (e.g. a geometry type upstream does not handle).
    #[error("{operation}: unsupported: {reason}")]
    Unsupported {
        /// Operation that does not support the input.
        operation: &'static str,
        /// What is unsupported.
        reason: &'static str,
    },

    /// A face's current trims could not be implanted into its surface's cache while its
    /// B-rep is being edited (an open or inconsistent loop), so a point cannot be
    /// classified against the face as it is now. Not upstream, which classifies
    /// against the untrimmed surface then (WR-732, #2503).
    #[error("{operation}: the trims of {entity} cannot be implanted while it is edited")]
    FaceTrimsUnavailable {
        /// Operation that classified against the face.
        operation: &'static str,
        /// The face.
        entity: EntityRef,
    },

    /// An upstream `SM_ERR_*` status with no dedicated variant, kept numerically so
    /// difftests can compare it with the C++ status code.
    #[error("{operation}: upstream status {code}{}", fmt_entity(*.entity))]
    Upstream {
        /// Operation that failed.
        operation: &'static str,
        /// Upstream `SmStatus` value.
        code: i32,
        /// Entity involved, when known.
        entity: Option<EntityRef>,
    },

    /// An NLib routine failed (upstream: returned its error flag after `N_ErrSet`
    /// stored `code` in the global `NL_ERROR.eno`).
    #[error("{operation}: NLib error {code:?} ({})", .code.code())]
    Nlib {
        /// Operation that failed.
        operation: &'static str,
        /// Upstream NLib error number.
        code: NlibCode,
    },

    /// A port-side invariant or adapter failure with no upstream `SmStatus`: an owner,
    /// handle or identity the port could not resolve where upstream follows a pointer
    /// (e.g. a B-rep cache owner that is not in the source B-rep under the caller's
    /// identities). It signals a caller error or a solidcore defect, never a status an
    /// upstream caller handles, so [`SolidError::sm_status`] is `None`. Code that
    /// treated these as [`SolidError::InvalidInput`] before the variant existed matches
    /// both, so results and difftest statuses are unchanged.
    #[error("{operation}: port invariant: {reason}")]
    PortInvariant {
        /// Operation whose invariant failed.
        operation: &'static str,
        /// What did not hold.
        reason: &'static str,
    },

    /// An operation-local read-only solve needs its mutable caller to prepare geometry.
    /// This control signal is not an unported dependency or a native status.
    #[error("{operation}: geometry preparation required")]
    PreparationRequired {
        /// The read-only operation that must be resumed after preparation.
        operation: &'static str,
    },

    /// The caller's cooperative cancellation check on [`crate::Kernel`] fired.
    ///
    /// Propagates past status-ignoring fallbacks alongside [`SolidError::Unported`], so a
    /// cancelled call never returns a partial or different geometric result.
    #[error("operation cancelled in {operation}")]
    Cancelled {
        /// Operation `path::to::fn` that observed cancellation.
        operation: &'static str,
    },

    /// A fillet refused as too large, with the largest size that builds it (opt-in
    /// `fixes.fillet_gn_section_generator`; the fork's `GetFitRadius`, `GetFitEdge` and
    /// `GetFitKind`, which its capi's `smlib_blend_fit_size` reads, tangency-labs/smlib#79,
    /// WR-696). Returned in place of [`SolidError::Upstream`] with the same `code` (so
    /// [`SolidError::sm_status`] is unchanged) only when the refusal's cause has a size; a
    /// radius law has none. The fillet session's error info holds the reason and comment.
    #[error("{operation}: upstream status {code}: refused; the largest size that builds is {}", .fit.size)]
    FilletRefused {
        /// Operation that failed.
        operation: &'static str,
        /// Upstream `SmStatus` value (`SM_ERR`).
        code: i32,
        /// Entity involved, when known (the [`SolidError::Upstream`]'s).
        entity: Option<EntityRef>,
        /// Why there is no room, when the refusal names it.
        reason: Option<crate::blend::fillet_executive::NoRoomReason>,
        /// The largest size that builds: a radius, a factor on two offsets, or a chord
        /// width ([`crate::blend::fillet_executive::FitKind`]), for the blend of `fit.edge`.
        fit: crate::blend::fillet_executive::FitSize<EdgeId>,
    },

    /// A failure in solidcore's own terms: the stage, what failed, the entity and the
    /// key values, and the upstream status it stands for (kept for
    /// [`SolidError::sm_status`], never shown). See [`Failure`] and the module docs.
    #[error("{0}")]
    Failed(Box<Failure>),

    /// The operation needs an upstream routine whose unit is not ported yet (a
    /// plan back-edge). Returned instead of panicking or faking a result; every
    /// site carries a `// UNPORTED(<unit id>): <C++ symbol>` comment so the
    /// target unit's porter can wire it up. difftest classifies a case whose Rust
    /// side returns this as *deferred* (neither pass nor fail). See `AGENTS.md`
    /// section 4a.
    #[error("unported: upstream `{upstream}` (unit {unit}) is not ported yet")]
    Unported {
        /// Upstream C++ symbol that would be called (e.g. `"SmCurve::Evaluate"`).
        upstream: &'static str,
        /// Plan unit id that will port it (e.g. `"geom.curve"`).
        unit: &'static str,
    },
}

/// NLib error numbers (`NL_*_ERR`, `source/SMLib/inc/NL_DataStruct.h`); the
/// discriminant is the upstream value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NlibCode {
    /// `NL_CUR_ERR`: curve definition error (e.g. `n+p+1 != m`).
    Curve = 1,
    /// `NL_SUR_ERR`: surface definition error.
    Surface = 2,
    /// `NL_DEG_ERR`: degree limit error (`p > NL_MAXDEG`).
    Degree = 3,
    /// `NL_KNT_ERR`: knot vector error (e.g. `U[i] > U[i+1]`).
    Knot = 4,
    /// `NL_WEI_ERR`: weight error (negative or too small).
    Weight = 5,
    /// `NL_PAR_ERR`: parameter value error (e.g. `u < U[0]`).
    Parameter = 6,
    /// `NL_IND_ERR`: index range error (e.g. `i < 0`).
    Index = 7,
    /// `NL_DER_ERR`: derivative value error (e.g. `C'(u) = 0`).
    Derivative = 8,
    /// `NL_STO_ERR`: insufficient storage.
    Storage = 9,
    /// `NL_MAT_ERR`: math function error (e.g. domain error).
    Math = 10,
    /// `NL_NUM_ERR`: numerical failure (e.g. zero division).
    Numerical = 11,
    /// `NL_CON_ERR`: convergence failure.
    Convergence = 12,
    /// `NL_CAL_ERR`: error in called routine.
    Call = 13,
    /// `NL_INP_ERR`: input data is corrupt.
    Input = 14,
    /// `NL_MEM_ERR`: memory allocation error.
    Memory = 15,
    /// `NL_SEQ_ERR`: system of equations error (e.g. singular).
    Equations = 16,
    /// `NL_GEO_ERR`: geometric operation failure.
    Geometry = 17,
    /// `NL_FIL_ERR`: cannot open file.
    File = 18,
    /// `NL_TOL_ERR`: tolerance error.
    Tolerance = 19,
    /// `NL_MXD_ERR`: derivative limit error (`der > NL_MAXDER`).
    MaxDerivative = 20,
    /// `NL_KML_ERR`: knot multiplicity error (multiplicity > degree).
    KnotMultiplicity = 21,
    /// `NL_CUR_REV`: curve end control points reverse direction.
    CurveReversed = 22,
    /// `NL_SCN_ERR`: scan (read) error.
    Scan = 23,
}

impl NlibCode {
    /// The upstream `NL_*_ERR` number.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

// ---- SmStatus codes -------------------------------------------------------------------
//
// The one definition of every `SmStatus` number; other modules re-export or `use` these.
// The values are pinned against `SmMessages.h` by `tests::status_codes_match_upstream`.

/// Upstream `SmStatus` value `SM_SUCCESS` (`1000L`, `SmMessages.h`): success: the status every `SE`/`SER` macro tests against. No [`SolidError`] carries it; it is the value of status fields such as `BrepCheck::pointers_status` on success.
pub const SM_SUCCESS: i32 = 1000;

/// Upstream `SmStatus` value `SM_ERR` (`1001L`, `SmMessages.h`): generic failure.
pub const SM_ERR: i32 = 1001;

/// Upstream `SmStatus` value `SM_ERR_OUT_OF_MEMORY` (`1002L`, `SmMessages.h`): an allocation failed (where an `SM_API` wrapper returns it for an output array that cannot grow).
pub const SM_ERR_OUT_OF_MEMORY: i32 = 1002;

/// Upstream `SmStatus` value `SM_ERR_UNKNOWN` (`1003L`, `SmMessages.h`): unknown error (`ERR_MSG`).
pub const SM_ERR_UNKNOWN: i32 = 1003;

/// Upstream `SmStatus` value `SM_ERR_FATAL` (`1004L`, `SmMessages.h`): fatal error.
pub const SM_ERR_FATAL: i32 = 1004;

/// Upstream `SmStatus` value `SM_ERR_ASSERT_FAILURE` (`1005L`, `SmMessages.h`): an assertion failed (`AE`/`AER`/`AERN`).
pub const SM_ERR_ASSERT_FAILURE: i32 = 1005;

/// Upstream `SmStatus` value `SM_ERR_NULL_POINTER` (`1006L`, `SmMessages.h`): a `NULL` pointer (`NER`).
pub const SM_ERR_NULL_POINTER: i32 = 1006;

/// Upstream `SmStatus` value `SM_ERR_INVALID_INPUT` (`1007L`, `SmMessages.h`): invalid input. Its usual mapping is [`SolidError::InvalidInput`]; it is carried by [`SolidError::Upstream`] where a function returns it as a status that callers ignore.
pub const SM_ERR_INVALID_INPUT: i32 = 1007;

/// Upstream `SmStatus` value `SM_ERR_NON_NULL_OUTPUT_POINTER` (`1008L`, `SmMessages.h`): an output pointer that should be `NULL` is not.
pub const SM_ERR_NON_NULL_OUTPUT_POINTER: i32 = 1008;

/// Upstream `SmStatus` value `SM_ERR_ASSERTVALID_FAILURE` (`1010L`, `SmMessages.h`): an `AssertValid` check failed.
pub const SM_ERR_ASSERTVALID_FAILURE: i32 = 1010;

/// Upstream `SmStatus` value `SM_ERR_OUTSIDE_OF_DOMAIN` (`1020L`, `SmMessages.h`): a parameter outside the domain.
pub const SM_ERR_OUTSIDE_OF_DOMAIN: i32 = 1020;

/// Upstream `SmStatus` value `SM_ERR_NOT_WITHIN_TOLERANCE` (`1021L`, `SmMessages.h`): a result not within tolerance.
pub const SM_ERR_NOT_WITHIN_TOLERANCE: i32 = 1021;

/// Upstream `SmStatus` value `SM_ERR_NOT_CONVERGING` (`1022L`, `SmMessages.h`): an iteration did not converge.
pub const SM_ERR_NOT_CONVERGING: i32 = 1022;

/// Upstream `SmStatus` value `SM_ERR_WARNING` (`1023L`, `SmMessages.h`): a warning (`WARN`).
pub const SM_ERR_WARNING: i32 = 1023;

/// Upstream `SmStatus` value `SM_ERR_MESSAGE` (`1024L`, `SmMessages.h`): a message (`MSG`).
pub const SM_ERR_MESSAGE: i32 = 1024;

/// Upstream `SmStatus` value `SM_ERR_AXIS_INSIDE_FACE` (`1025L`, `SmMessages.h`): a revolution axis inside the face.
pub const SM_ERR_AXIS_INSIDE_FACE: i32 = 1025;

/// Upstream `SmStatus` value `SM_ERR_LICENSE_EXPIRED` (`1026L`, `SmMessages.h`): the licence expired.
pub const SM_ERR_LICENSE_EXPIRED: i32 = 1026;

/// Upstream `SmStatus` value `SM_ERR_BAD_INTERSECTIONS` (`1027L`, `SmMessages.h`): bad intersections.
pub const SM_ERR_BAD_INTERSECTIONS: i32 = 1027;

/// Upstream `SmStatus` value `SM_ERR_BAD_COINCIDENT_VERTICES` (`1028L`, `SmMessages.h`): bad coincident vertices.
pub const SM_ERR_BAD_COINCIDENT_VERTICES: i32 = 1028;

/// Upstream `SmStatus` value `SM_ERR_BAD_SURFACE_POINT` (`1030L`, `SmMessages.h`): a bad surface point.
pub const SM_ERR_BAD_SURFACE_POINT: i32 = 1030;

/// Upstream `SmStatus` value `SM_ERR_BAD_TANGENT_DROP` (`1032L`, `SmMessages.h`): a bad tangent drop.
pub const SM_ERR_BAD_TANGENT_DROP: i32 = 1032;

/// Upstream `SmStatus` value `SM_ERR_DEGENERATE_SURFACE` (`1034L`, `SmMessages.h`): a degenerate surface.
pub const SM_ERR_DEGENERATE_SURFACE: i32 = 1034;

/// Upstream `SmStatus` value `SM_ERR_BAD_FIND_DEGEN_PARAM` (`1036L`, `SmMessages.h`): a degenerate parameter could not be found.
pub const SM_ERR_BAD_FIND_DEGEN_PARAM: i32 = 1036;

/// Upstream `SmStatus` value `SM_ERR_METHOD_FAILURE_QUITING` (`1038L`, `SmMessages.h`): a method failed; quitting.
pub const SM_ERR_METHOD_FAILURE_QUITING: i32 = 1038;

/// Upstream `SmStatus` value `SM_ERR_NOTYET_HEAL_FACE` (`1040L`, `SmMessages.h`): a face not healed yet.
pub const SM_ERR_NOTYET_HEAL_FACE: i32 = 1040;

/// Upstream `SmStatus` value `SM_ERR_TESS_FAIL_FACE` (`1041L`, `SmMessages.h`): face tessellation failed.
pub const SM_ERR_TESS_FAIL_FACE: i32 = 1041;

/// SMLib fork status `SM_ERR_OFFSET_WALLS_TOUCH` (tangency-labs/smlib a759622,
/// `SmMessages.h`; not in the pinned upstream), as carried by [`SolidError::Upstream`]:
/// an offset (a shell, a solid offset) whose walls would meet, because loops of an offset
/// face touch without crossing. Returned only with the opt-in
/// `Kernel::fixes.offset_walls_touch_refused`.
pub const SM_ERR_OFFSET_WALLS_TOUCH: i32 = 1042;

/// SMLib fork status `SM_ERR_OFFSET_EDGE_VANISHES` (tangency-labs/smlib 34318f4,
/// `SmMessages.h`; not in the pinned upstream), as carried by [`SolidError::Upstream`]:
/// an offset moves a vertex along an edge to or past the edge's other end, a change of
/// topology it does not make, so the operation is refused. Test for it with
/// [`SolidError::is_offset_edge_vanishes`].
pub const SM_ERR_OFFSET_EDGE_VANISHES: i32 = 1043;

// ---- SmBoolean values (`SmTypes.h`) ---------------------------------------------------

/// `TRUE` (`1`, `SmTypes.h`) of an `SmBoolean`, such as `SmShell::IsInnerShellOf` returns.
pub const SM_TRUE: i32 = 1;

/// `FALSE` (`0`, `SmTypes.h`) of an `SmBoolean`. Not a valid `SmStatus`.
pub const SM_FALSE: i32 = 0;

/// `UNSURE` (`2`, `SmTypes.h`) of an `SmBoolean`: undecided.
pub const SM_UNSURE: i32 = 2;

fn fmt_entity(entity: Option<EntityRef>) -> String {
    entity.map_or_else(String::new, |e| format!(" at {e}"))
}

/// A failure in solidcore's own terms, carried by [`SolidError::Failed`].
///
/// Build one with [`Failure::new`] and its `at` and `caused_by` methods, then make it a
/// [`SolidError`] with [`Failure::into_error`]. Its message is
/// `"{operation}: {reason}"`, followed by `"; caused by {cause}"` when it keeps the error
/// of the stage it comes from.
///
/// It also records, privately, the status the pinned C++ reports for the same failure,
/// so that difftest can compare the two ([`SolidError::sm_status`], the comparison
/// hook). That number is not part of the failure's public fields or its message.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Failure {
    /// The solidcore path of the failing stage (e.g.
    /// `"offset::offset_executive::p2::refuse_folding_offset"`).
    pub operation: &'static str,
    /// The entity the failure is about, when known: in the caller's B-rep where the
    /// failure is about one of its entities.
    pub entity: Option<EntityRef>,
    /// What failed.
    pub reason: Reason,
    /// The status the pinned C++ reports for this failure: a comparison detail, read
    /// only through [`SolidError::sm_status`]. It is recorded where the failure is made
    /// because it depends on the call path, not only on the reason (a refusal the C++
    /// passes on keeps its own status; after a generic collapse it is `SM_ERR`).
    pub(crate) status: i32,
    /// The error of the stage this failure comes from, kept where a caller reports a
    /// stage's failure as its own, generic one (#2511).
    pub cause: Option<SolidError>,
}

impl Failure {
    /// A failure of `operation` for `reason`, with the reason's entity and status and no
    /// cause.
    #[must_use]
    pub fn new(operation: &'static str, reason: impl Into<Reason>) -> Self {
        let reason = reason.into();
        Self {
            operation,
            entity: reason.entity(),
            status: reason.comparison_status(),
            reason,
            cause: None,
        }
    }

    /// The same failure about `entity`.
    #[must_use]
    pub fn at(mut self, entity: EntityRef) -> Self {
        self.entity = Some(entity);
        self
    }

    /// The failure's stable [`ErrorKind`].
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.reason.kind_given(self.status)
    }

    /// The same failure, keeping `cause`, the error of the stage it comes from.
    #[must_use]
    pub fn caused_by(mut self, cause: SolidError) -> Self {
        self.cause = Some(cause);
        self
    }

    /// The [`SolidError::Failed`] of this failure. (Not a `From` impl: a second
    /// `From` for [`SolidError`] would make `?` in closures ambiguous.)
    #[must_use]
    pub fn into_error(self) -> SolidError {
        SolidError::Failed(Box::new(self))
    }
}

impl core::fmt::Display for Failure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.operation, self.reason)?;
        // a reason that names no entity of its own is followed by the failure's
        if let (Reason::Failed(_), Some(entity)) = (&self.reason, self.entity) {
            write!(f, " ({entity})")?;
        }
        if let Some(cause) = &self.cause {
            write!(f, "; caused by {cause}")?;
        }
        Ok(())
    }
}

/// What failed, in solidcore's terms: one variant per subsystem whose errors are
/// described, plus [`Reason::Failed`] for a stage whose failure is described by a phrase.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Reason {
    /// A stage failed, as the phrase says (e.g. "the offset could not be built"). Its
    /// status is `SM_ERR` unless the failure says otherwise; its cause, when kept, says
    /// why.
    Failed(&'static str),
    /// An offset, shell, thicken or local operation refused.
    Offset(offset::OffsetFailure),
}

impl Reason {
    /// The entity the reason names, if any.
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match self {
            Self::Failed(_) => None,
            Self::Offset(o) => o.entity(),
        }
    }

    /// The status the C++ reports for the reason unless the failure says otherwise (a
    /// comparison detail).
    pub(crate) fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) => SM_ERR,
            Self::Offset(o) => o.comparison_status(),
        }
    }

    /// The reason's stable [`ErrorKind`]; `status` (the failure's comparison status)
    /// gives the kind of a [`Reason::Failed`].
    pub(crate) fn kind_given(&self, status: i32) -> ErrorKind {
        match self {
            Self::Failed(_) => ErrorKind::of_status(status),
            Self::Offset(o) => o.kind(),
        }
    }
}

impl core::fmt::Display for Reason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Offset(o) => o.fmt(f),
        }
    }
}

impl From<offset::OffsetFailure> for Reason {
    fn from(o: offset::OffsetFailure) -> Self {
        Self::Offset(o)
    }
}

/// A stable solidcore code for the kind of an error, from [`SolidError::kind`]: match
/// on it (or on its [`ErrorKind::code`] or [`ErrorKind::name`]) instead of message
/// text. Codes and names never change meaning once released; new kinds are added as
/// subsystems describe their failures. General kinds are numbered from 1, solidcore's
/// own (no upstream status) from 90, blend kinds from 100 and offset kinds from 200.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
#[repr(u16)]
pub enum ErrorKind {
    /// An argument was out of range, malformed or inconsistent.
    InvalidInput = 1,
    /// A typed id did not refer to a live entity.
    NotFound = 2,
    /// Geometry was degenerate where it may not be.
    Degenerate = 3,
    /// An iterative solve did not converge.
    NotConverged = 4,
    /// The inputs are a combination the operation does not support.
    Unsupported = 5,
    /// The operation failed, and the error does not describe how yet (a generic
    /// status of a subsystem not converted yet, or a stage's generic failure whose
    /// cause says more).
    Failed = 6,
    /// A required entity, geometry or value was missing.
    Missing = 7,
    /// A parameter was outside its domain.
    OutsideDomain = 8,
    /// A result was not within tolerance.
    NotWithinTolerance = 9,
    /// A port-side invariant or adapter failure (no upstream status).
    PortInvariant = 90,
    /// A call into a part of the kernel not ported yet.
    Unported = 91,
    /// The caller cancelled the operation.
    Cancelled = 92,
    /// A read-only solve needs its caller to prepare geometry first.
    PreparationRequired = 93,
    /// A face's trims could not be used while its B-rep is edited.
    FaceTrimsUnavailable = 94,
    /// A fillet or chamfer is too large to build; the error names the largest size
    /// that builds.
    FilletTooLarge = 100,
    /// An offset face's offset folds: the wall is thicker than a radius of curvature.
    OffsetFolds = 200,
    /// Loops of an offset face touch without crossing: the offset walls would meet.
    OffsetWallsTouch = 201,
    /// Loops of an offset face cross in a way the offset does not resolve.
    OffsetLoopsCross = 202,
    /// An offset would move a vertex to or past the other end of its edge.
    OffsetEdgeVanishes = 203,
}

impl ErrorKind {
    /// The kind's stable number.
    #[must_use]
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// True for a solidcore-side failure, not a failure of the operation on its input: a
    /// port invariant that did not hold, a call into a part not ported yet, a solve that
    /// needs its caller to prepare geometry, or trims unavailable while editing. A caller
    /// that skips an entity where the operation fails on it must not skip on these
    /// (#1452). A cancellation is the caller's own and is not one.
    #[must_use]
    pub const fn is_port_failure(self) -> bool {
        matches!(
            self,
            Self::PortInvariant
                | Self::Unported
                | Self::PreparationRequired
                | Self::FaceTrimsUnavailable
        )
    }

    /// The kind's stable name (`snake_case`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::NotFound => "not_found",
            Self::Degenerate => "degenerate",
            Self::NotConverged => "not_converged",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
            Self::Missing => "missing",
            Self::OutsideDomain => "outside_domain",
            Self::NotWithinTolerance => "not_within_tolerance",
            Self::PortInvariant => "port_invariant",
            Self::Unported => "unported",
            Self::Cancelled => "cancelled",
            Self::PreparationRequired => "preparation_required",
            Self::FaceTrimsUnavailable => "face_trims_unavailable",
            Self::FilletTooLarge => "fillet_too_large",
            Self::OffsetFolds => "offset_folds",
            Self::OffsetWallsTouch => "offset_walls_touch",
            Self::OffsetLoopsCross => "offset_loops_cross",
            Self::OffsetEdgeVanishes => "offset_edge_vanishes",
        }
    }

    /// The kind of an error that carries only a C++ status ([`SolidError::Upstream`],
    /// until it is retired): the status's kind where solidcore has one for it,
    /// [`ErrorKind::Failed`] otherwise.
    pub(crate) const fn of_status(status: i32) -> Self {
        match status {
            SM_ERR_INVALID_INPUT => Self::InvalidInput,
            SM_ERR_NULL_POINTER => Self::Missing,
            SM_ERR_OUTSIDE_OF_DOMAIN => Self::OutsideDomain,
            SM_ERR_NOT_WITHIN_TOLERANCE => Self::NotWithinTolerance,
            SM_ERR_NOT_CONVERGING => Self::NotConverged,
            SM_ERR_OFFSET_WALLS_TOUCH => Self::OffsetWallsTouch,
            SM_ERR_OFFSET_EDGE_VANISHES => Self::OffsetEdgeVanishes,
            _ => Self::Failed,
        }
    }
}

impl core::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

impl SolidError {
    /// [`SolidError::Upstream`] with code [`SM_ERR`] and no entity (upstream
    /// `return SE(SM_ERR)`).
    #[must_use]
    pub const fn sm_err(operation: &'static str) -> Self {
        Self::Upstream {
            operation,
            code: SM_ERR,
            entity: None,
        }
    }

    /// [`SolidError::Upstream`] with code [`SM_ERR_NULL_POINTER`] and no entity
    /// (upstream `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub const fn null_pointer(operation: &'static str) -> Self {
        Self::Upstream {
            operation,
            code: SM_ERR_NULL_POINTER,
            entity: None,
        }
    }

    /// Maps the error of a failed NLib call inside an `Sm*` method to `SM_ERR` (upstream
    /// `if (NL_YES == N_...) SER(SM_ERR)`), letting [`SolidError::Unported`] and
    /// [`SolidError::Cancelled`] through.
    /// Use as `.map_err(SolidError::nl_ser(OP))`.
    pub(crate) fn nl_ser(operation: &'static str) -> impl Fn(Self) -> Self {
        move |e| match e {
            Self::Unported { .. } | Self::Cancelled { .. } | Self::PreparationRequired { .. } => e,
            _ => Self::sm_err(operation),
        }
    }

    /// [`SolidError::InvalidInput`]: input upstream would not survive (out-of-range
    /// reads, null dereferences, malformed data).
    #[must_use]
    pub const fn invalid_input(operation: &'static str, reason: &'static str) -> Self {
        Self::InvalidInput { operation, reason }
    }

    /// [`SolidError::PortInvariant`]: a port-side invariant or adapter failure with no
    /// upstream status.
    #[must_use]
    pub const fn port_invariant(operation: &'static str, reason: &'static str) -> Self {
        Self::PortInvariant { operation, reason }
    }

    /// The comparison hook, for difftest only: the status the pinned C++ reports for this
    /// failure, when it reports one exactly. Callers use [`SolidError::kind`], the
    /// [`Reason`] and [`SolidError::is_port_failure`] instead; this moves to a
    /// comparison-only module once the pre-#2615 variants are gone (see the module docs).
    ///
    /// By variant:
    ///
    /// - [`SolidError::Upstream`]: its `code`.
    /// - [`SolidError::Failed`]: its failure's `status`.
    /// - [`SolidError::Nlib`]: [`SM_ERR`]. The `Sm*` callers of an NLib routine
    ///   surface its failure as `SER(SM_ERR)` (`if (NL_YES == N_...) SER(SM_ERR)`;
    ///   `nl_ser`), never as the `NL_*_ERR` number.
    /// - Every other variant: `None`. [`SolidError::InvalidInput`],
    ///   [`SolidError::NotFound`], [`SolidError::Degenerate`],
    ///   [`SolidError::NotConverged`] and [`SolidError::Unsupported`] are used both for
    ///   mapped upstream statuses and for port-side checks, so they are not exact; a
    ///   port that must surface a status exactly returns [`SolidError::Upstream`] with
    ///   it. [`SolidError::PortInvariant`], [`SolidError::Unported`] and
    ///   [`SolidError::PreparationRequired`] never stand for one.
    #[must_use]
    pub const fn sm_status(&self) -> Option<i32> {
        match self {
            Self::Upstream { code, .. } | Self::FilletRefused { code, .. } => Some(*code),
            Self::Failed(f) => Some(f.status),
            Self::Nlib { .. } => Some(SM_ERR),
            Self::InvalidInput { .. }
            | Self::NotFound { .. }
            | Self::Degenerate { .. }
            | Self::NotConverged { .. }
            | Self::Unsupported { .. }
            | Self::PortInvariant { .. }
            | Self::PreparationRequired { .. }
            | Self::Cancelled { .. }
            | Self::FaceTrimsUnavailable { .. }
            | Self::Unported { .. } => None,
        }
    }

    /// [`SolidError::Cancelled`]: cooperative cancellation observed in `operation`.
    #[must_use]
    pub const fn cancelled(operation: &'static str) -> Self {
        Self::Cancelled { operation }
    }

    /// True when this error is [`SolidError::Cancelled`].
    #[must_use]
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    /// True when this error must never be swallowed or remapped by an upstream
    /// status-ignoring fallback ([`SolidError::Unported`], [`SolidError::Cancelled`]
    /// or [`SolidError::FaceTrimsUnavailable`], which no upstream status stands for).
    #[must_use]
    pub(crate) const fn is_unported_or_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Unported { .. } | Self::Cancelled { .. } | Self::FaceTrimsUnavailable { .. }
        )
    }

    /// [`SolidError::Unported`]: a call into `upstream` (a C++ symbol) of `unit`,
    /// which is not ported yet. Every site carries `// UNPORTED(<unit>): <symbol>`.
    #[must_use]
    pub const fn unported(upstream: &'static str, unit: &'static str) -> Self {
        Self::Unported { upstream, unit }
    }

    /// True only for a genuinely unported dependency; preparation requests return false.
    #[must_use]
    pub const fn is_unported(&self) -> bool {
        matches!(self, Self::Unported { .. })
    }

    /// True for the refusal of an offset that would move a vertex past the other end of
    /// its edge ([`ErrorKind::OffsetEdgeVanishes`]: [`offset::OffsetFailure::EdgeVanishes`],
    /// or an [`SolidError::Upstream`] with code [`SM_ERR_OFFSET_EDGE_VANISHES`]).
    #[must_use]
    pub fn is_offset_edge_vanishes(&self) -> bool {
        self.kind() == ErrorKind::OffsetEdgeVanishes
    }

    /// The stable solidcore [`ErrorKind`] of this error. A [`SolidError::Failed`] has its
    /// reason's kind; an error that carries only an upstream status has that status's
    /// kind (1007 is `invalid_input`, 1006 `missing`, 1022 `not_converged`, ...).
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidInput { .. } => ErrorKind::InvalidInput,
            Self::NotFound { .. } => ErrorKind::NotFound,
            Self::Degenerate { .. } => ErrorKind::Degenerate,
            Self::NotConverged { .. } => ErrorKind::NotConverged,
            Self::Unsupported { .. } => ErrorKind::Unsupported,
            Self::FaceTrimsUnavailable { .. } => ErrorKind::FaceTrimsUnavailable,
            Self::Upstream { code, .. } => ErrorKind::of_status(*code),
            Self::Nlib { code, .. } => match code {
                NlibCode::Convergence => ErrorKind::NotConverged,
                _ => ErrorKind::Failed,
            },
            Self::PortInvariant { .. } => ErrorKind::PortInvariant,
            Self::PreparationRequired { .. } => ErrorKind::PreparationRequired,
            Self::Cancelled { .. } => ErrorKind::Cancelled,
            Self::FilletRefused { .. } => ErrorKind::FilletTooLarge,
            Self::Failed(f) => f.kind(),
            Self::Unported { .. } => ErrorKind::Unported,
        }
    }

    /// True for a solidcore-side failure rather than a failure of the operation on its
    /// input ([`ErrorKind::is_port_failure`], #1452).
    #[must_use]
    pub fn is_port_failure(&self) -> bool {
        self.kind().is_port_failure()
    }

    /// The failure in solidcore's terms, for a [`SolidError::Failed`].
    #[must_use]
    pub fn failure(&self) -> Option<&Failure> {
        match self {
            Self::Failed(f) => Some(f),
            _ => None,
        }
    }

    /// The error of the stage this one comes from, where it was kept
    /// ([`Failure::cause`]).
    #[must_use]
    pub fn cause(&self) -> Option<&SolidError> {
        self.failure().and_then(|f| f.cause.as_ref())
    }

    /// The first error in the chain of [`SolidError::cause`]s: the stage where the
    /// failure began (this error itself when it keeps no cause).
    #[must_use]
    pub fn root_cause(&self) -> &SolidError {
        let mut e = self;
        while let Some(cause) = e.cause() {
            e = cause;
        }
        e
    }

    /// [`SolidError::Failed`] for a stage whose failure upstream reports as a generic
    /// `SER(SM_ERR)`: status [`SM_ERR`], keeping `cause`, the stage's error (#2511).
    pub(crate) fn failed_after(
        operation: &'static str,
        what: &'static str,
        cause: SolidError,
    ) -> Self {
        Failure::new(operation, Reason::Failed(what))
            .caused_by(cause)
            .into_error()
    }

    /// [`SolidError::Nlib`] with the upstream error number `code` (NLib's
    /// `N_ErrSet(.., code, ..)` followed by an error return).
    #[must_use]
    pub const fn nlib(operation: &'static str, code: NlibCode) -> Self {
        Self::Nlib { operation, code }
    }

    /// The operation that produced this error (for [`SolidError::Unported`], the
    /// upstream symbol that is not ported yet).
    #[must_use]
    pub fn operation(&self) -> &'static str {
        match self {
            Self::InvalidInput { operation, .. }
            | Self::NotFound { operation, .. }
            | Self::Degenerate { operation, .. }
            | Self::NotConverged { operation, .. }
            | Self::Unsupported { operation, .. }
            | Self::PortInvariant { operation, .. }
            | Self::Upstream { operation, .. }
            | Self::FilletRefused { operation, .. }
            | Self::Nlib { operation, .. }
            | Self::PreparationRequired { operation }
            | Self::FaceTrimsUnavailable { operation, .. }
            | Self::Cancelled { operation } => operation,
            Self::Failed(f) => f.operation,
            Self::Unported { upstream, .. } => upstream,
        }
    }

    /// The entity this error refers to, if any.
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match self {
            Self::NotFound { entity, .. } | Self::FaceTrimsUnavailable { entity, .. } => {
                Some(*entity)
            }
            Self::Degenerate { entity, .. }
            | Self::Upstream { entity, .. }
            | Self::FilletRefused { entity, .. } => *entity,
            Self::Failed(f) => f.entity,
            Self::InvalidInput { .. }
            | Self::NotConverged { .. }
            | Self::Unsupported { .. }
            | Self::PortInvariant { .. }
            | Self::Nlib { .. }
            | Self::Unported { .. }
            | Self::Cancelled { .. }
            | Self::PreparationRequired { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_failures_keep_upstream_statuses_and_propagate_port_errors() {
        assert_eq!(native_failure(Ok(3)), Ok(Some(3)));
        let native = [
            SolidError::sm_err("ray"),
            SolidError::invalid_input("ray", "native"),
            SolidError::Degenerate {
                operation: "ray",
                entity: None,
            },
            SolidError::nlib("ray", NlibCode::Numerical),
        ];
        for e in native {
            assert_eq!(native_failure::<()>(Err(e)), Ok(None));
        }
        let internal = [
            SolidError::unported("SmBrep::RayFire", "topology.brep_p2"),
            SolidError::PreparationRequired { operation: "ray" },
            SolidError::port_invariant("topology::brep_cache", "owner not in source B-rep"),
            SolidError::NotFound {
                operation: "ray",
                entity: EntityRef::Face(FaceId::from_index(4)),
            },
        ];
        for e in internal {
            // `ignored` hid the last two as an ordinary failed call.
            assert_eq!(native_failure::<()>(Err(e.clone())), Err(e));
        }
    }

    #[test]
    fn preparation_requests_are_never_ignored_or_reported_as_unported() {
        let request = SolidError::PreparationRequired { operation: "ray" };
        assert!(!request.is_unported());
        assert_eq!(ignored::<()>(Err(request.clone())), Err(request.clone()));
        assert_eq!(
            ignore_status::<()>(Err(request.clone())),
            Err(request.clone())
        );
        assert_eq!(SolidError::nl_ser("native")(request.clone()), request);
    }

    #[test]
    fn sm_status_is_some_only_for_exact_upstream_statuses() {
        let f = FaceId::from_index(1);
        let cases = [
            (SolidError::sm_err("op"), Some(SM_ERR)),
            (SolidError::null_pointer("op"), Some(SM_ERR_NULL_POINTER)),
            (
                SolidError::Upstream {
                    operation: "op",
                    code: SM_ERR_INVALID_INPUT,
                    entity: Some(EntityRef::Face(f)),
                },
                Some(1007),
            ),
            (
                SolidError::Upstream {
                    operation: "op",
                    code: SM_ERR_WARNING,
                    entity: None,
                },
                Some(1023),
            ),
            (SolidError::nlib("op", NlibCode::Knot), Some(SM_ERR)),
            (SolidError::nlib("op", NlibCode::Convergence), Some(SM_ERR)),
            (SolidError::invalid_input("op", "bad"), None),
            (
                SolidError::NotFound {
                    operation: "op",
                    entity: EntityRef::Face(f),
                },
                None,
            ),
            (
                SolidError::Degenerate {
                    operation: "op",
                    entity: None,
                },
                None,
            ),
            (
                SolidError::NotConverged {
                    operation: "op",
                    iterations: 3,
                },
                None,
            ),
            (
                SolidError::Unsupported {
                    operation: "op",
                    reason: "r",
                },
                None,
            ),
            (SolidError::port_invariant("op", "owner"), None),
            (SolidError::PreparationRequired { operation: "op" }, None),
            (SolidError::unported("SmX::Y", "geom.x"), None),
        ];
        for (e, status) in cases {
            assert_eq!(e.sm_status(), status, "{e:?}");
        }
    }

    #[test]
    fn port_invariant_display_and_accessors() {
        let e =
            SolidError::port_invariant("topology::brep_cache", "edge owner is not in source B-rep");
        assert_eq!(
            e.to_string(),
            "topology::brep_cache: port invariant: edge owner is not in source B-rep"
        );
        assert_eq!(e.operation(), "topology::brep_cache");
        assert_eq!(e.entity(), None);
        assert!(!e.is_unported());
        // an ignored upstream status swallows it as it did the `InvalidInput` it was
        assert_eq!(ignored::<()>(Err(e.clone())), Ok(None));
        assert_eq!(
            SolidError::nl_ser("native")(e),
            SolidError::sm_err("native")
        );
    }

    fn walls_touch(face: Option<FaceId>) -> SolidError {
        Failure::new(
            "offset::offset_executive::crossing_loops::resolve_crossing_loops",
            offset::OffsetFailure::WallsTouch {
                face,
                contact: offset::WallContact::RunAlong,
            },
        )
        .into_error()
    }

    #[test]
    fn failures_keep_their_status_for_difftest_but_never_show_it() {
        let e = walls_touch(Some(FaceId::from_index(3)));
        assert_eq!(e.sm_status(), Some(SM_ERR_OFFSET_WALLS_TOUCH));
        assert_eq!(e.kind(), ErrorKind::OffsetWallsTouch);
        assert_eq!(e.kind().code(), 201);
        assert_eq!(e.kind().name(), "offset_walls_touch");
        assert_eq!(e.entity(), Some(EntityRef::Face(FaceId::from_index(3))));
        assert_eq!(
            e.operation(),
            "offset::offset_executive::crossing_loops::resolve_crossing_loops"
        );
        assert_eq!(
            e.to_string(),
            "offset::offset_executive::crossing_loops::resolve_crossing_loops: the loops of the \
             offset of face 3 run along one another, so the offset walls would meet there"
        );
        // a failure stands for an upstream status (#1452): ignored and handled as one
        assert_eq!(native_failure::<()>(Err(e.clone())), Ok(None));
        assert_eq!(ignored::<()>(Err(e.clone())), Ok(None));
        assert!(!e.is_unported_or_cancelled());
        assert_eq!(e.cause(), None);
        assert_eq!(e.root_cause(), &e);
        // without a face it says so
        assert_eq!(
            walls_touch(None).to_string(),
            "offset::offset_executive::crossing_loops::resolve_crossing_loops: the loops of an \
             offset face run along one another, so the offset walls would meet there"
        );
        assert_eq!(walls_touch(None).entity(), None);
    }

    #[test]
    fn a_generic_failure_keeps_the_stage_error_it_comes_from() {
        let stage = walls_touch(Some(FaceId::from_index(3)));
        let e = SolidError::failed_after(
            "topology::brep::local_operation::local_operation",
            "the offset of the moved faces could not be built",
            stage.clone(),
        );
        // the status upstream returns (SER(SM_ERR)), so difftest is unchanged
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert_eq!(e.kind(), ErrorKind::Failed);
        assert_eq!(e.cause(), Some(&stage));
        assert_eq!(e.root_cause(), &stage);
        assert_eq!(
            e.to_string(),
            format!(
                "topology::brep::local_operation::local_operation: the offset of the moved faces \
                 could not be built; caused by {stage}"
            )
        );
        // a described failure about an entity names it
        let mut at = Failure::new("op", Reason::Failed("the section has no points"))
            .at(EntityRef::Edge(EdgeId::from_index(12)));
        at.status = SM_ERR_NOT_CONVERGING;
        let at = at.into_error();
        assert_eq!(at.to_string(), "op: the section has no points (edge 12)");
        assert_eq!(at.kind(), ErrorKind::NotConverged);
        assert_eq!(at.sm_status(), Some(SM_ERR_NOT_CONVERGING));
        assert_eq!(at.entity(), Some(EntityRef::Edge(EdgeId::from_index(12))));
    }

    #[test]
    fn every_error_has_a_stable_kind() {
        let f = FaceId::from_index(1);
        let up = |code| SolidError::Upstream {
            operation: "op",
            code,
            entity: None,
        };
        let cases = [
            (
                SolidError::invalid_input("op", "bad"),
                ErrorKind::InvalidInput,
                1,
            ),
            (
                SolidError::NotFound {
                    operation: "op",
                    entity: EntityRef::Face(f),
                },
                ErrorKind::NotFound,
                2,
            ),
            (
                SolidError::Degenerate {
                    operation: "op",
                    entity: None,
                },
                ErrorKind::Degenerate,
                3,
            ),
            (
                SolidError::NotConverged {
                    operation: "op",
                    iterations: 3,
                },
                ErrorKind::NotConverged,
                4,
            ),
            (
                SolidError::Unsupported {
                    operation: "op",
                    reason: "r",
                },
                ErrorKind::Unsupported,
                5,
            ),
            (SolidError::sm_err("op"), ErrorKind::Failed, 6),
            (SolidError::null_pointer("op"), ErrorKind::Missing, 7),
            (up(SM_ERR_INVALID_INPUT), ErrorKind::InvalidInput, 1),
            (up(SM_ERR_OUTSIDE_OF_DOMAIN), ErrorKind::OutsideDomain, 8),
            (
                up(SM_ERR_NOT_WITHIN_TOLERANCE),
                ErrorKind::NotWithinTolerance,
                9,
            ),
            (up(SM_ERR_NOT_CONVERGING), ErrorKind::NotConverged, 4),
            (up(SM_ERR_BAD_INTERSECTIONS), ErrorKind::Failed, 6),
            (
                up(SM_ERR_OFFSET_WALLS_TOUCH),
                ErrorKind::OffsetWallsTouch,
                201,
            ),
            (
                up(SM_ERR_OFFSET_EDGE_VANISHES),
                ErrorKind::OffsetEdgeVanishes,
                203,
            ),
            (SolidError::nlib("op", NlibCode::Knot), ErrorKind::Failed, 6),
            (
                SolidError::nlib("op", NlibCode::Convergence),
                ErrorKind::NotConverged,
                4,
            ),
            (
                SolidError::port_invariant("op", "owner"),
                ErrorKind::PortInvariant,
                90,
            ),
            (
                SolidError::unported("X::y", "geom.x"),
                ErrorKind::Unported,
                91,
            ),
            (SolidError::cancelled("op"), ErrorKind::Cancelled, 92),
            (
                SolidError::PreparationRequired { operation: "op" },
                ErrorKind::PreparationRequired,
                93,
            ),
            (
                SolidError::FaceTrimsUnavailable {
                    operation: "op",
                    entity: EntityRef::Face(f),
                },
                ErrorKind::FaceTrimsUnavailable,
                94,
            ),
            (walls_touch(None), ErrorKind::OffsetWallsTouch, 201),
        ];
        for (e, kind, code) in cases {
            // #1452 in solidcore terms: only the 90s but a cancellation are port-side
            assert_eq!(
                e.is_port_failure(),
                (90..=94).contains(&code) && kind != ErrorKind::Cancelled,
                "{e:?}"
            );
            assert_eq!(e.kind(), kind, "{e:?}");
            assert_eq!(kind.code(), code, "{e:?}");
            assert_eq!(kind.to_string(), kind.name());
        }
        let edge = offset::OffsetFailure::EdgeVanishes {
            edge: Some(EdgeId::from_index(7)),
            face: None,
        };
        let e = Failure::new("op", edge).into_error();
        assert!(e.is_offset_edge_vanishes());
        assert_eq!(e.entity(), Some(EntityRef::Edge(EdgeId::from_index(7))));
        assert_eq!(e.sm_status(), Some(SM_ERR_OFFSET_EDGE_VANISHES));
        assert!(up(SM_ERR_OFFSET_EDGE_VANISHES).is_offset_edge_vanishes());
        assert!(!walls_touch(None).is_offset_edge_vanishes());
    }

    #[test]
    fn offset_refusals_name_their_entities_and_statuses() {
        use offset::{LoopCrossing, OffsetFailure};
        let face = FaceId::from_index(4);
        let folds = OffsetFailure::Folds {
            face,
            distance: -11.0,
            largest: 10.0,
        };
        assert_eq!(folds.comparison_status(), SM_ERR);
        assert_eq!(folds.kind(), ErrorKind::OffsetFolds);
        assert_eq!(folds.entity(), Some(EntityRef::Face(face)));
        assert_eq!(
            folds.to_string(),
            "the offset of face 4 by -11 folds: the wall is thicker than a radius of curvature \
             of the face on that side (the largest wall that works is 10)"
        );
        let cross = OffsetFailure::LoopsCross {
            face: None,
            crossing: LoopCrossing::ThreeAtOnePoint,
        };
        assert_eq!(cross.comparison_status(), SM_ERR_BAD_INTERSECTIONS);
        assert_eq!(cross.entity(), None);
        assert_eq!(cross.on_face(face).entity(), Some(EntityRef::Face(face)));
        assert_eq!(
            cross.on_face(face).to_string(),
            "the loops of the offset of face 4 cross in a way the offset cannot resolve: three \
             loops cross at one point"
        );
        // a face already named, or an edge, is kept
        let other = FaceId::from_index(9);
        assert_eq!(cross.on_face(face).on_face(other), cross.on_face(face));
        let edge = OffsetFailure::EdgeVanishes {
            edge: Some(EdgeId::from_index(2)),
            face: None,
        };
        assert_eq!(edge.on_face(face), edge);
        assert_eq!(
            OffsetFailure::EdgeVanishes {
                edge: None,
                face: None
            }
            .on_face(face)
            .to_string(),
            "the offset moves a vertex of an edge of face 4 to or past the edge's other end: the \
             edge would vanish, a change of topology the offset does not make"
        );
    }

    #[test]
    fn display_includes_operation_and_entity() {
        let e = SolidError::NotFound {
            operation: "topology::Brep::face",
            entity: EntityRef::Face(FaceId::from_index(7)),
        };
        assert_eq!(e.to_string(), "topology::Brep::face: face 7 not found");
        assert_eq!(e.operation(), "topology::Brep::face");
        assert_eq!(e.entity(), Some(EntityRef::Face(FaceId::from_index(7))));

        let d = SolidError::Degenerate {
            operation: "op",
            entity: None,
        };
        assert_eq!(d.to_string(), "op: degenerate geometry");

        let u = SolidError::Unported {
            upstream: "SmCurve::Evaluate",
            unit: "geom.curve",
        };
        assert_eq!(
            u.to_string(),
            "unported: upstream `SmCurve::Evaluate` (unit geom.curve) is not ported yet"
        );
        assert_eq!(u.operation(), "SmCurve::Evaluate");
        assert_eq!(u.entity(), None);

        let c = SolidError::cancelled("boolean::merge::p4::manifold_boolean");
        assert!(c.is_cancelled());
        assert!(!c.is_unported());
        assert!(c.is_unported_or_cancelled());
        assert_eq!(c.sm_status(), None);
        assert_eq!(c.operation(), "boolean::merge::p4::manifold_boolean");
        assert_eq!(c.entity(), None);
        assert_eq!(
            c.to_string(),
            "operation cancelled in boolean::merge::p4::manifold_boolean"
        );
        assert_eq!(ignored::<()>(Err(c.clone())), Err(c.clone()));
        assert_eq!(ignore_status::<()>(Err(c.clone())), Err(c.clone()));
        assert_eq!(native_failure::<()>(Err(c.clone())), Err(c.clone()));
        assert_eq!(SolidError::nl_ser("native")(c.clone()), c);
    }
}
