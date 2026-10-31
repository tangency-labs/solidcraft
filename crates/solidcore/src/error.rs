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
//! | [`SolidError::invalid_input`] | (none: out-of-range reads / null dereferences upstream) |
//! | `ignored`, `ignore_status` (crate) | a call whose `SmStatus` upstream ignores    |
//! | `checked_or_ignored` (crate) | per-call opt-in status policy (#483); legacy helpers unchanged |
//! | `native_failure` (crate)  | a call whose failing `SmStatus` upstream handles (internal errors propagate) |
//! | [`NlibCode`]                | `NL_*_ERR` numbers (`source/SMLib/inc/NL_DataStruct.h`) |
//! | `?` operator                | `SER(...)` / `NER(...)` (return on error)        |
//!
//! NLib routines (`NL_*.cpp`) report failure by returning an error flag and storing an
//! error number in the process-wide `NL_ERROR` record; solidcore returns
//! [`SolidError::Nlib`] carrying that number as an [`NlibCode`] instead.
//!
//! Every variant names the `operation` (a `&'static str`, by convention the Rust path
//! of the failing function, e.g. `"geom::vector3d::Vec3::unitize"`) so that errors surfaced to
//! callers stay actionable without a backtrace.

use crate::topology::{
    BrepId, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, RegionId, ShellId, VertexId,
    VertexuseId,
};

/// Shorthand for `core::result::Result<T, SolidError>`.
pub type Result<T, E = SolidError> = core::result::Result<T, E>;

/// The value of a call whose failing status upstream ignores: `Some` on success,
/// `None` on failure; unported dependencies and preparation requests still propagate.
pub(crate) fn ignored<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported() || matches!(e, SolidError::PreparationRequired { .. }) => Err(e),
        Err(_) => Ok(None),
    }
}

/// The value of a call whose failing status upstream handles as an ordinary failure
/// (e.g. a missed ray it retries): `Some` on success, `None` on an error that may stand
/// for an upstream status. Errors with no upstream status propagate, so a port defect
/// cannot pass for that failure: [`SolidError::Unported`],
/// [`SolidError::PreparationRequired`], [`SolidError::PortInvariant`] (an owner, handle
/// or identity the port could not resolve) and [`SolidError::NotFound`] (a stale typed
/// id, where upstream follows a pointer).
pub(crate) fn native_failure<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(
            e @ (SolidError::Unported { .. }
            | SolidError::PreparationRequired { .. }
            | SolidError::PortInvariant { .. }
            | SolidError::NotFound { .. }),
        ) => Err(e),
        Err(_) => Ok(None),
    }
}

/// The status of a call upstream ignores: `Ok` for any result but
/// [`SolidError::Unported`] or [`SolidError::PreparationRequired`].
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
        Err(e) if pass_on => Err(e),
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
/// `pass_on`, dropped without.
pub(crate) fn status_passed_on(status: Option<SolidError>, pass_on: bool) -> Result<()> {
    match status {
        Some(e) if pass_on => Err(e),
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
    /// `if (NL_YES == N_...) SER(SM_ERR)`), letting [`SolidError::Unported`] through.
    /// Use as `.map_err(SolidError::nl_ser(OP))`.
    pub(crate) fn nl_ser(operation: &'static str) -> impl Fn(Self) -> Self {
        move |e| match e {
            Self::Unported { .. } | Self::PreparationRequired { .. } => e,
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

    /// The upstream `SmStatus` this error stands for, when it stands for one exactly:
    ///
    /// - [`SolidError::Upstream`]: its `code`.
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
            Self::Upstream { code, .. } => Some(*code),
            Self::Nlib { .. } => Some(SM_ERR),
            Self::InvalidInput { .. }
            | Self::NotFound { .. }
            | Self::Degenerate { .. }
            | Self::NotConverged { .. }
            | Self::Unsupported { .. }
            | Self::PortInvariant { .. }
            | Self::PreparationRequired { .. }
            | Self::Unported { .. } => None,
        }
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

    /// True for the refusal [`SolidError::Upstream`] with code
    /// [`SM_ERR_OFFSET_EDGE_VANISHES`]: an offset that would move a vertex past the
    /// other end of its edge.
    #[must_use]
    pub const fn is_offset_edge_vanishes(&self) -> bool {
        matches!(
            self,
            Self::Upstream {
                code: SM_ERR_OFFSET_EDGE_VANISHES,
                ..
            }
        )
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
            | Self::Nlib { operation, .. }
            | Self::PreparationRequired { operation } => operation,
            Self::Unported { upstream, .. } => upstream,
        }
    }

    /// The entity this error refers to, if any.
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match self {
            Self::NotFound { entity, .. } => Some(*entity),
            Self::Degenerate { entity, .. } | Self::Upstream { entity, .. } => *entity,
            Self::InvalidInput { .. }
            | Self::NotConverged { .. }
            | Self::Unsupported { .. }
            | Self::PortInvariant { .. }
            | Self::Nlib { .. }
            | Self::Unported { .. }
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
    }
}
