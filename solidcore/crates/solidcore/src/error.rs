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
//! | [`SolidError::Unported`], [`SolidError::unported`], [`SolidError::is_unported`] | (none: call into a unit not ported yet) |
//! | [`SolidError::Cancelled`], [`SolidError::cancelled`], [`SolidError::is_cancelled`] | (none: cooperative cancellation on [`crate::Kernel`]) |
//! | [`SolidError::invalid_input`] | (none: out-of-range reads / null dereferences upstream) |
//! | `ignored`, `ignore_status` (crate) | a call whose `SmStatus` upstream ignores    |
//! | `checked_or_ignored` (crate) | per-call opt-in status policy (#483); legacy helpers unchanged |
//! | `native_failure` (crate)  | a call whose failing `SmStatus` upstream handles (internal errors propagate) |
//! | `?` operator                | `SER(...)` / `NER(...)` (return on error)        |
//! | [`SolidError::Failed`], [`Failure`], [`Reason`] | none: a failure in solidcore's own terms (#2615) |
//! | [`ErrorKind`], [`SolidError::kind`] | none: solidcore's stable error codes (#2615) |
//! | [`offset::OffsetFailure`]   | none: the offset subsystem's refusals (#2615)    |
//! | [`boolean::BooleanFailure`] | none: the Boolean operation's failures           |
//! | [`nurbs::NurbsFailure`], [`nurbs::BSplineFailure`] | NLib error flag + `NL_ERROR.eno` (`N_ErrSet`): the `NL_*_ERR` numbers, in solidcore's terms |
//! | [`defeature::DefeatureFailure`] | none: why a defeature keeps a feature (#3218) |
//! | [`sweep::SweepFailure`]     | none: why a sweep or loft refuses, and the sweep routines' failures |
//! | [`curve::CurveFailure`]     | none: why a curve query refuses, and the curve routines' failures |
//! | [`query::QueryFailure`]     | none: the query routines' failures               |
//! | [`topology::TopologyFailure`] | none: the topology routines' failures          |
//! | [`surface::SurfaceFailure`] | none: the surface routines' failures             |
//! | [`blend::BlendFailure`]     | none: the blend routines' failures               |
//!
//! NLib routines (`NL_*.cpp`) report failure by returning an error flag and storing an
//! error number in the process-wide `NL_ERROR` record; solidcore returns a
//! [`SolidError::Failed`] with a [`nurbs::NurbsFailure`] reason instead.
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
//!   enum in a submodule of this module ([`offset::OffsetFailure`],
//!   [`boolean::BooleanFailure`]), so the taxonomy
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
//! **Retiring "upstream".** The goal is no [`SolidError::Upstream`] (nor
//! `FilletRefused`) variant and no "upstream status" text in the public API. They are
//! the pre-#2615 shape: subsystems move their sites to [`SolidError::Failed`] one PR at
//! a time (offset first), and the variants go when their last site does. Then
//! `sm_status` and the status numbers here move to a comparison-only module (or to
//! difftest and the FFI), since nothing but the comparison needs them.

#[cfg(test)]
mod bare_scan;
pub mod blend;
pub mod boolean;
pub mod curve;
pub mod defeature;
pub mod geom;
pub mod heal;
pub mod intersect;
pub mod mass;
pub mod nurbs;
pub mod offset;
#[cfg(test)]
mod phrase_sites;
pub mod primitive;
pub mod query;
pub mod surface;
pub mod sweep;
pub mod topology;

use crate::math::Point3;
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

    /// A region whose side the kernel is unsure of (`SmRegion::IsVoid`'s `UNSURE`), where
    /// the answer depends on whether it is material: refused, never read as material or
    /// void. Not upstream (upstream's `TRUE` test reads `UNSURE` as void); its
    /// [`SolidError::sm_status`] is `None`.
    #[error("{operation}: unsure whether region {} is material", .region.index())]
    UnsureRegion {
        /// Operation that needed the region's side.
        operation: &'static str,
        /// The region.
        region: RegionId,
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

    /// A fillet refused as too large, with the largest size that builds it (the fork's
    /// `GetFitRadius`, `GetFitEdge` and
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
    #[error(transparent)]
    Failed(Box<Failure>),

    /// A blend refused on one of its edges or corners, with what an application words it
    /// with ([`crate::blend::blend_refusal::BlendRefusal`]): an edge with no blend, a round
    /// too small to change the part, a blend that cannot end where its edge runs on
    /// tangentially, a setback too short or too long, a chamfer's angle past the other
    /// face. `code` is the status the refusal stood for before it was typed (`SM_ERR_INVALID_INPUT` for an edge with no blend,
    /// `SM_ERR` for the others), so [`SolidError::sm_status`] is unchanged.
    #[error("{operation}: upstream status {code}: refused ({refusal:?})")]
    BlendRefused {
        /// Operation that refused.
        operation: &'static str,
        /// Upstream `SmStatus` value.
        code: i32,
        /// What was refused, and why.
        refusal: crate::blend::blend_refusal::BlendRefusal,
    },

    /// A round refused for the room it leaves or takes, with what the refusal found
    /// ([`crate::blend::round_room::RoundRefusal`]): rounds whose result crosses a face's
    /// boundary over itself, or a variable round along a straight edge between planes whose
    /// band does not fit its faces. Not upstream (SolidCraft's checks, made the kernel's).
    /// `code` is the status (`SM_ERR`) so [`SolidError::sm_status`] reads as an ordinary
    /// failure.
    #[error("{operation}: upstream status {code}: refused ({refusal:?})")]
    RoundRefused {
        /// Operation that refused.
        operation: &'static str,
        /// Upstream `SmStatus` value (`SM_ERR`).
        code: i32,
        /// Why, and what it found.
        refusal: crate::blend::round_room::RoundRefusal,
    },

    /// A surface extension refused (not upstream: [`crate::surface::extend`]), naming the
    /// longest distance that works when there is one.
    #[error("{operation}: extension refused ({refusal:?}){}", fmt_size(*.distance))]
    ExtendRefused {
        /// Operation that refused.
        operation: &'static str,
        /// Why.
        refusal: crate::surface::extend::ExtendRefusal,
        /// The longest distance across the side that works, when there is one.
        distance: Option<f64>,
    },

    /// A blend face's definition, read from a record, whose net it does not certify: the net is
    /// not proved within the definition's bound `eps` of S (docs/definitions-and-certificates.md
    /// §4.5), so the face is refused, never kept with a larger bound. Not upstream; its
    /// [`SolidError::sm_status`] is `None`.
    #[error("{operation}: the definition of {face} does not certify its net within {eps}")]
    DefinitionNotCertified {
        /// Operation that read the definition.
        operation: &'static str,
        /// The face.
        face: EntityRef,
        /// The bound ε the definition states.
        eps: f64,
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
/// face touch without crossing. Returned only with
/// `Kernel::fixes.offset_resolve_crossing_loops`.
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

/// An edge of the caller's body (the target an operation edits), as a failure's [`Site`]
/// names it. Built only by the mapping from an operation's own objects to the caller's
/// body (for the blend, [`TargetEdge::of_fillet_object`], the mapping the fillet session's
/// `fit_size` uses), so an id of an intermediate body cannot be named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetEdge(EdgeId);

/// A face of the caller's body, as a failure's [`Site`] names it (see [`TargetEdge`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetFace(FaceId);

/// A vertex of the caller's body, as a failure's [`Site`] names it (see [`TargetEdge`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetVertex(VertexId);

/// The topology of the caller's body a blend object is, when it is that: an object of the
/// fillet session's target body ([`crate::blend::fillet_classification::FilletBody::Target`]).
fn target_entity(
    object: &crate::blend::fillet_executive::session::FilletObject,
) -> Option<EntityRef> {
    use crate::blend::fillet_classification::FilletBody;
    use crate::blend::fillet_executive::session::FilletObject;
    match *object {
        FilletObject::Topology(FilletBody::Target, entity) => Some(entity),
        _ => None,
    }
}

impl TargetEdge {
    /// The edge of the caller's body a blend object is: `Some` only for an edge of the fillet
    /// session's target body.
    #[must_use]
    pub(crate) fn of_fillet_object(
        object: &crate::blend::fillet_executive::session::FilletObject,
    ) -> Option<Self> {
        match target_entity(object)? {
            EntityRef::Edge(e) => Some(Self(e)),
            _ => None,
        }
    }

    /// The edge's id in the caller's body.
    #[must_use]
    pub const fn id(self) -> EdgeId {
        self.0
    }
}

impl TargetFace {
    /// The face of the caller's body a blend object is: `Some` only for a face of the fillet
    /// session's target body.
    #[must_use]
    pub(crate) fn of_fillet_object(
        object: &crate::blend::fillet_executive::session::FilletObject,
    ) -> Option<Self> {
        match target_entity(object)? {
            EntityRef::Face(f) => Some(Self(f)),
            _ => None,
        }
    }

    /// The face's id in the caller's body.
    #[must_use]
    pub const fn id(self) -> FaceId {
        self.0
    }
}

impl TargetVertex {
    /// The vertex of the caller's body a blend object is: `Some` only for a vertex of the
    /// fillet session's target body.
    #[must_use]
    pub(crate) fn of_fillet_object(
        object: &crate::blend::fillet_executive::session::FilletObject,
    ) -> Option<Self> {
        match target_entity(object)? {
            EntityRef::Vertex(v) => Some(Self(v)),
            _ => None,
        }
    }

    /// The vertex's id in the caller's body.
    #[must_use]
    pub const fn id(self) -> VertexId {
        self.0
    }
}

/// Where a failure is, on the caller's body: the edge, face and vertex it is about and a
/// point, each when known. Every area's [`Failure`] carries one. Only entities of the
/// caller's body are named (the typed ids are built only by the mapping to it); a failure on
/// an intermediate body carries a point. Lists (rounds that cross, edges that overflow) stay
/// in the cause.
///
/// Filling a site never changes an outcome: no decision of the kernel reads it.
/// [`Site::or`] and [`SolidError::or_at`] fill its empty slots on the way up and never
/// overwrite one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct Site {
    /// The edge of the caller's body the failure is about.
    pub edge: Option<TargetEdge>,
    /// The face of the caller's body the failure is about.
    pub face: Option<TargetFace>,
    /// The vertex of the caller's body the failure is about.
    pub vertex: Option<TargetVertex>,
    /// The point where the failure is.
    pub at: Option<Point3>,
}

impl Site {
    /// A site at `edge`.
    #[must_use]
    pub fn edge(edge: TargetEdge) -> Self {
        Self {
            edge: Some(edge),
            ..Self::default()
        }
    }

    /// A site at `face`.
    #[must_use]
    pub fn face(face: TargetFace) -> Self {
        Self {
            face: Some(face),
            ..Self::default()
        }
    }

    /// A site at `vertex`.
    #[must_use]
    pub fn vertex(vertex: TargetVertex) -> Self {
        Self {
            vertex: Some(vertex),
            ..Self::default()
        }
    }

    /// A site at the point `at`.
    #[must_use]
    pub fn point(at: Point3) -> Self {
        Self {
            at: Some(at),
            ..Self::default()
        }
    }

    /// True when the site names nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.edge.is_none() && self.face.is_none() && self.vertex.is_none() && self.at.is_none()
    }

    /// This site with its empty slots filled from `other`: a slot already set is kept.
    #[must_use]
    pub fn or(self, other: Self) -> Self {
        Self {
            edge: self.edge.or(other.edge),
            face: self.face.or(other.face),
            vertex: self.vertex.or(other.vertex),
            at: self.at.or(other.at),
        }
    }
}

impl core::fmt::Display for Site {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut parts = Vec::new();
        if let Some(e) = self.edge {
            parts.push(format!("edge {}", e.id().index()));
        }
        if let Some(face) = self.face {
            parts.push(format!("face {}", face.id().index()));
        }
        if let Some(v) = self.vertex {
            parts.push(format!("vertex {}", v.id().index()));
        }
        if let Some(p) = self.at {
            parts.push(format!("({}, {}, {})", p.x, p.y, p.z));
        }
        f.write_str(&parts.join(", "))
    }
}

/// What a failure's [`Fit`] sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FitMeasure {
    /// A round's radius, or an equal chamfer's offset.
    Radius,
    /// A factor on a chamfer's two offsets.
    Scale,
    /// A chord round's width.
    Chord,
    /// A corner's setback.
    Setback,
    /// An offset's wall (its distance).
    Wall,
    /// A distance (an extension's, a chamfer's).
    Distance,
}

impl FitMeasure {
    /// The measure's name, as a message words it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Radius => "radius",
            Self::Scale => "scale",
            Self::Chord => "chord",
            Self::Setback => "setback",
            Self::Wall => "wall",
            Self::Distance => "distance",
        }
    }
}

impl From<crate::blend::fillet_executive::FitKind> for FitMeasure {
    fn from(kind: crate::blend::fillet_executive::FitKind) -> Self {
        use crate::blend::fillet_executive::FitKind;
        match kind {
            FitKind::Radius => Self::Radius,
            FitKind::Scale => Self::Scale,
            FitKind::Chord => Self::Chord,
        }
    }
}

/// The size that builds where a refusal computed one: the largest (a radius, a scale, a
/// chord, a wall, a distance) or, for a setback, the least.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Fit {
    /// The size: `> 0`, or `0` when no size builds.
    pub size: f64,
    /// What it sizes.
    pub measure: FitMeasure,
    /// The edge of the caller's body whose blend it sizes, when it is one's.
    pub edge: Option<TargetEdge>,
}

impl Fit {
    /// A fit of `size`, measured as `measure`, for the blend of `edge` when there is one.
    #[must_use]
    pub const fn new(size: f64, measure: FitMeasure, edge: Option<TargetEdge>) -> Self {
        Self {
            size,
            measure,
            edge,
        }
    }
}

/// Where a [`Failure`] was built in solidcore's source (`#[track_caller]`): a diagnostic, so
/// a reported defect can be traced without a debugger. It is compile-time data (file, line,
/// column), so deterministic and wasm-safe; it is not part of what failed, so two failures
/// that differ only in it are equal, and no message (nor any golden) shows it.
#[derive(Clone, Copy)]
pub struct Origin(&'static core::panic::Location<'static>);

impl Origin {
    /// The source file the failure was built in.
    #[must_use]
    pub fn file(self) -> &'static str {
        self.0.file()
    }

    /// The line it was built on.
    #[must_use]
    pub fn line(self) -> u32 {
        self.0.line()
    }
}

impl PartialEq for Origin {
    /// Always true: where a failure was built is not part of what failed.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl core::fmt::Debug for Origin {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}", self.0.file(), self.0.line())
    }
}

/// A failure in solidcore's own terms, carried by [`SolidError::Failed`]: the record every
/// area returns.
///
/// Build one with [`Failure::new`] and its `at` and `caused_by` methods, then make it a
/// [`SolidError`] with [`Failure::into_error`]. Its message is
/// `"{operation}: {reason}"`, followed by the site and the fit when known and
/// `"; caused by {cause}"` when it keeps the error of the stage it comes from. The cause is
/// also its [`std::error::Error::source`].
///
/// Its class is [`Failure::kind`], derived from the reason, so class and cause cannot
/// disagree. It also records, privately, the status the pinned C++ reports for the same
/// failure, so that difftest and the goldens can compare the two
/// ([`SolidError::sm_status`], the comparison hook). That number is not part of the
/// failure's public fields or its message.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Failure {
    /// The solidcore path of the failing stage (e.g.
    /// `"offset::offset_executive::p2::refuse_folding_offset"`).
    pub operation: &'static str,
    /// What failed: the cause, typed, with the numbers the check computed.
    pub reason: Reason,
    /// Where it failed, on the caller's body ([`Site`]).
    pub site: Site,
    /// The size that builds, where the check computed one.
    pub fit: Option<Fit>,
    /// The error of the stage this failure comes from, kept where a caller reports a
    /// stage's failure as its own, generic one (#2511).
    pub cause: Option<SolidError>,
    /// The entity a site not yet moved onto [`Failure::site`] names, untyped (its body is
    /// not known to be the caller's): read through [`SolidError::entity`], and by the
    /// decisions that read it before the record existed. [`SolidError::or_at`] never
    /// writes it, so no outcome depends on filling a site. It goes as the sites move onto
    /// [`Failure::site`].
    pub(crate) entity: Option<EntityRef>,
    /// The status the pinned C++ reports for this failure: a comparison detail, read
    /// only through [`SolidError::sm_status`]. It is recorded where the failure is made
    /// because it depends on the call path, not only on the reason (a refusal the C++
    /// passes on keeps its own status; after a generic collapse it is `SM_ERR`).
    pub(crate) status: i32,
    /// Where the failure was built ([`Origin`]).
    pub(crate) origin: Origin,
}

impl Failure {
    /// A failure of `operation` for `reason`, with the reason's entity and status, an empty
    /// site, no fit and no cause, built here ([`Failure::origin`]).
    #[must_use]
    #[track_caller]
    pub fn new(operation: &'static str, reason: impl Into<Reason>) -> Self {
        let reason = reason.into();
        Self {
            operation,
            entity: reason.entity(),
            status: reason.comparison_status(),
            reason,
            site: Site::default(),
            fit: None,
            cause: None,
            origin: Origin(core::panic::Location::caller()),
        }
    }

    /// The same failure about `entity`, untyped (a site not yet moved onto
    /// [`Failure::site`]; see [`SolidError::entity`]).
    #[must_use]
    pub fn at(mut self, entity: EntityRef) -> Self {
        self.entity = Some(entity);
        self
    }

    /// The same failure with the empty slots of its site filled from `site`: a slot
    /// already set is kept, and nothing else changes.
    #[must_use]
    pub fn or_at(mut self, site: Site) -> Self {
        self.site = self.site.or(site);
        self
    }

    /// The same failure with `fit`, the size that builds.
    #[must_use]
    pub fn with_fit(mut self, fit: Fit) -> Self {
        self.fit = Some(fit);
        self
    }

    /// The same failure, with `status` as the one the pinned C++ reports for it (a
    /// comparison detail, for a failure whose phrase is not a typed reason).
    #[must_use]
    pub(crate) fn with_status(mut self, status: i32) -> Self {
        self.status = status;
        self
    }

    /// The failure's stable [`ErrorKind`]: its class.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.reason.kind_given(self.status)
    }

    /// The entity a site not yet moved onto [`Failure::site`] names (see
    /// [`SolidError::entity`]).
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        self.entity
    }

    /// Where in solidcore's source the failure was built (a diagnostic: never in a message).
    #[must_use]
    pub fn origin(&self) -> Origin {
        self.origin
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
        let names_none = match &self.reason {
            Reason::Failed(_) | Reason::Topology(_) | Reason::Blend(_) => true,
            Reason::Offset(o) => o.replaces_upstream_status(),
            _ => false,
        };
        if let (true, Some(entity)) = (names_none, self.entity) {
            write!(f, " ({entity})")?;
        }
        if !self.site.is_empty() {
            write!(f, " at {}", self.site)?;
        }
        if let Some(fit) = self.fit {
            write!(
                f,
                "; the {} that builds is {}",
                fit.measure.name(),
                fit.size
            )?;
        }
        if let Some(cause) = &self.cause {
            write!(f, "; caused by {cause}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Failure {
    /// The error of the stage this failure comes from ([`Failure::cause`]).
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause
            .as_ref()
            .map(|c| c as &(dyn std::error::Error + 'static))
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
    /// A NURBS routine failed.
    Nurbs(nurbs::NurbsFailure),
    /// A Boolean operation failed.
    Boolean(boolean::BooleanFailure),
    /// A defeature kept a feature on its base: its patch is not built (#3218).
    Defeature(defeature::DefeatureFailure),
    /// An intersection or classification failed.
    Intersect(intersect::IntersectFailure),
    /// A primitive, extrusion, revolution, skin or taper failed.
    Primitive(primitive::PrimitiveFailure),
    /// A sweep or loft refused (#3140).
    Sweep(sweep::SweepFailure),
    /// A mass-property integration or enclosure computation failed.
    Mass(mass::MassFailure),
    /// A healing or stitching step failed.
    Heal(heal::HealFailure),
    /// A geometry utility (linear algebra, transforms, boxes, solvers, tolerances,
    /// attributes) failed.
    Geom(geom::GeomFailure),
    /// A curve query refused (#3184).
    Curve(curve::CurveFailure),
    /// A query failed.
    Query(query::QueryFailure),
    /// A topology routine failed.
    Topology(topology::TopologyFailure),
    /// A surface routine failed.
    Surface(surface::SurfaceFailure),
    /// A blend routine failed.
    Blend(blend::BlendFailure),
    /// A blend certificate's feet box runs past a support's net in a direction the
    /// support's kind does not continue (not upstream, tangency-labs/solidcraft#14284):
    /// support `support` (0 or 1) over the leader's piece `s`, the box `uv` against the net's
    /// `domain` (each `[[u_lo, u_hi], [v_lo, v_hi]]`). Nothing past the net is the face
    /// there, so the piece is refused, never assumed.
    FootPastDomainEnd {
        /// Which support.
        support: usize,
        /// The leader's piece the feet were enclosed over.
        s: [f64; 2],
        /// The feet's box on that support.
        uv: [[f64; 2]; 2],
        /// The support net's domain.
        domain: [[f64; 2]; 2],
    },
}

impl Reason {
    /// The entity the reason names, if any.
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match self {
            Self::Failed(_)
            | Self::Nurbs(_)
            | Self::Defeature(_)
            | Self::Intersect(_)
            | Self::Primitive(_)
            | Self::Sweep(_)
            | Self::Mass(_)
            | Self::Heal(_)
            | Self::Geom(_)
            | Self::Curve(_)
            | Self::Query(_)
            | Self::Topology(_)
            | Self::Surface(_)
            | Self::Blend(_)
            | Self::FootPastDomainEnd { .. } => None,
            Self::Offset(o) => o.entity(),
            Self::Boolean(b) => b.entity(),
        }
    }

    /// The status the C++ reports for the reason unless the failure says otherwise (a
    /// comparison detail).
    pub(crate) fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) | Self::FootPastDomainEnd { .. } => SM_ERR,
            Self::Offset(o) => o.comparison_status(),
            Self::Nurbs(n) => n.comparison_status(),
            Self::Boolean(b) => b.comparison_status(),
            Self::Defeature(d) => d.comparison_status(),
            Self::Intersect(i) => i.comparison_status(),
            Self::Primitive(p) => p.comparison_status(),
            Self::Sweep(w) => w.comparison_status(),
            Self::Mass(m) => m.comparison_status(),
            Self::Heal(h) => h.comparison_status(),
            Self::Geom(g) => g.comparison_status(),
            Self::Curve(c) => c.comparison_status(),
            Self::Query(q) => q.comparison_status(),
            Self::Topology(t) => t.comparison_status(),
            Self::Surface(s) => s.comparison_status(),
            Self::Blend(b) => b.comparison_status(),
        }
    }

    /// The reason's stable [`ErrorKind`]; `status` (the failure's comparison status)
    /// gives the kind of a [`Reason::Failed`].
    pub(crate) fn kind_given(&self, status: i32) -> ErrorKind {
        match self {
            Self::Failed(_) => ErrorKind::of_status(status),
            Self::FootPastDomainEnd { .. } => ErrorKind::OutsideDomain,
            Self::Offset(o) => o.kind(),
            Self::Nurbs(n) => n.kind(),
            Self::Boolean(b) => b.kind(),
            Self::Defeature(d) => d.kind(),
            Self::Intersect(i) => i.kind(),
            Self::Primitive(p) => p.kind(),
            Self::Sweep(w) => w.kind(),
            Self::Mass(m) => m.kind(),
            Self::Heal(h) => h.kind(),
            Self::Geom(g) => g.kind(),
            Self::Curve(c) => c.kind(),
            Self::Query(q) => q.kind(),
            Self::Topology(t) => t.kind(),
            Self::Surface(s) => s.kind(),
            Self::Blend(b) => b.class(),
        }
    }
}

impl core::fmt::Display for Reason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Offset(o) => o.fmt(f),
            Self::Nurbs(n) => n.fmt(f),
            Self::Boolean(b) => b.fmt(f),
            Self::Defeature(d) => d.fmt(f),
            Self::Intersect(i) => i.fmt(f),
            Self::Primitive(p) => p.fmt(f),
            Self::Sweep(w) => w.fmt(f),
            Self::Mass(m) => m.fmt(f),
            Self::Heal(h) => h.fmt(f),
            Self::Geom(g) => g.fmt(f),
            Self::Curve(c) => c.fmt(f),
            Self::Query(q) => q.fmt(f),
            Self::Topology(t) => t.fmt(f),
            Self::Surface(s) => s.fmt(f),
            Self::Blend(b) => b.fmt(f),
            Self::FootPastDomainEnd {
                support,
                s,
                uv,
                domain,
            } => write!(
                f,
                "a foot past its face's domain end: support {support} over the piece s in [{}, {}], its box u [{}, {}] v [{}, {}] past the net's domain u [{}, {}] v [{}, {}], in a direction its kind does not continue",
                s[0], s[1], uv[0][0], uv[0][1], uv[1][0], uv[1][1], domain[0][0], domain[0][1], domain[1][0], domain[1][1]
            ),
        }
    }
}

impl From<boolean::BooleanFailure> for Reason {
    fn from(b: boolean::BooleanFailure) -> Self {
        Self::Boolean(b)
    }
}

impl From<nurbs::NurbsFailure> for Reason {
    fn from(n: nurbs::NurbsFailure) -> Self {
        Self::Nurbs(n)
    }
}

impl From<defeature::DefeatureFailure> for Reason {
    fn from(d: defeature::DefeatureFailure) -> Self {
        Self::Defeature(d)
    }
}

impl From<intersect::IntersectFailure> for Reason {
    fn from(i: intersect::IntersectFailure) -> Self {
        Self::Intersect(i)
    }
}

impl From<primitive::PrimitiveFailure> for Reason {
    fn from(p: primitive::PrimitiveFailure) -> Self {
        Self::Primitive(p)
    }
}

impl From<mass::MassFailure> for Reason {
    fn from(m: mass::MassFailure) -> Self {
        Self::Mass(m)
    }
}

impl From<heal::HealFailure> for Reason {
    fn from(h: heal::HealFailure) -> Self {
        Self::Heal(h)
    }
}

impl From<geom::GeomFailure> for Reason {
    fn from(g: geom::GeomFailure) -> Self {
        Self::Geom(g)
    }
}

impl From<offset::OffsetFailure> for Reason {
    fn from(o: offset::OffsetFailure) -> Self {
        Self::Offset(o)
    }
}

impl From<sweep::SweepFailure> for Reason {
    fn from(w: sweep::SweepFailure) -> Self {
        Self::Sweep(w)
    }
}

impl From<curve::CurveFailure> for Reason {
    fn from(c: curve::CurveFailure) -> Self {
        Self::Curve(c)
    }
}

impl From<query::QueryFailure> for Reason {
    fn from(q: query::QueryFailure) -> Self {
        Self::Query(q)
    }
}

impl From<topology::TopologyFailure> for Reason {
    fn from(t: topology::TopologyFailure) -> Self {
        Self::Topology(t)
    }
}

impl From<surface::SurfaceFailure> for Reason {
    fn from(s: surface::SurfaceFailure) -> Self {
        Self::Surface(s)
    }
}

impl From<blend::BlendFailure> for Reason {
    fn from(b: blend::BlendFailure) -> Self {
        Self::Blend(b)
    }
}

/// A stable solidcore code for the kind of an error, from [`SolidError::kind`]: match
/// on it (or on its [`ErrorKind::code`] or [`ErrorKind::name`]) instead of message
/// text. Codes and names never change meaning once released; new kinds are added as
/// subsystems describe their failures. General kinds are numbered from 1, solidcore's
/// own (no upstream status) from 90, blend kinds from 100 and offset kinds from 200.
///
/// Each kind is also a class: [`ErrorKind::is_defect`] (the kernel's fault: report it),
/// [`ErrorKind::is_refusal`] (a fact of the geometry: change the input) and
/// [`ErrorKind::sized`] (a size that builds is the way out).
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
    // The blend's classes, 101 to 128 (the classes of the typed blend failure record,
    // `error::blend`), in their order: the record's numbers less 200, then the classes added
    // after them.
    /// The rolling ball does not fit a face it rolls on.
    BallDoesNotFit = 101,
    /// A contact line leaves the face it rolls on, and the kernel does not build over the cliff beyond.
    RailLeavesFace = 102,
    /// The blends take the whole of a face they roll on.
    FaceConsumed = 103,
    /// One blend's contact line runs inside the band another takes off a face they share.
    BandsOverlap = 104,
    /// The balls fold over one another along the edge.
    BallsFold = 105,
    /// A face curves tighter than a chamfer's offset along it: the blend's counterpart of the
    /// offset's [`ErrorKind::OffsetFolds`] (200), the same geometric event with a different way
    /// out (a smaller chamfer, not a thinner wall).
    ChamferOffsetFolds = 106,
    /// The blend would take all of the part between its contact lines and cut it in pieces.
    CutsThePart = 107,
    /// The blend's section would break through a wall into the hollow behind it.
    BreaksThrough = 108,
    /// The ball is as wide as a face it rolls on, so its centre stays put: the blend's
    /// counterpart of the offset's [`ErrorKind::OffsetCollapses`] (204), the same geometric
    /// event with a different way out (a smaller ball, not a shorter distance).
    BallOffsetCollapses = 109,
    /// The blend's corners take the whole of its edge.
    CornersTakeWholeEdge = 110,
    /// A blend past its faces runs off a face into the material.
    RunsIntoMaterial = 111,
    /// A blend whose contact line leaves its face while other blends, or a rollover, decide the overflow against it.
    CliffOverflowRefused = 112,
    /// A blend that overflows its face ends on a face that is not planar.
    CliffEndFaceNotPlanar = 113,
    /// A blend that overflows its face in a shape the kernel does not decide.
    CliffOverflowShape = 114,
    /// Removing the overflow left no manifold solid: a defect.
    CliffSubtractionLeavesNoSolid = 115,
    /// A blend past its faces creases a face that runs on tangentially.
    CreaseAcrossTangentEdge = 116,
    /// A rail would end within the part's resolution of its start without closing.
    RailBelowResolution = 117,
    /// A rail would lie on the far side of an arc's axis from its edge.
    RailPastAxis = 118,
    /// Two blends running on into each other with different offsets.
    JoinOffsetsDiffer = 119,
    /// The blend would leave a strip of the body thinner than the part resolves.
    StripBelowResolution = 120,
    /// An End's rail runs past its side edge's far end, onto the faces beyond.
    EndPastSideEdge = 121,
    /// An End of mixed convexity whose rail misses the side edge of the other convexity.
    EndOverflowsSideEdge = 122,
    /// An End's rail rolls over its face's tangent edge where the round ends at a smooth vertex.
    EndRollsIntoSmoothEnd = 123,
    /// A round's edge runs on across a seam's vertex into an edge that is not blended.
    EndAtSeamVertex = 124,
    /// The solve of the ball's position at a station of the blend did not converge: a defect.
    StationDidNotConverge = 125,
    /// The faces the blend's rails enclose reach past its rails: a defect.
    TrimLeaks = 126,
    /// A stage of the blend failed with no reason the kernel names: a defect.
    StageFailed = 127,
    /// A round that ends where its edge runs on tangentially into an edge that is not
    /// blended, in a shape the kernel does not build. Not a matter of size: the way out is to
    /// blend the edge it runs on into with it
    /// ([`crate::blend::blend_refusal::BlendRefusal::ContinuesInto`]).
    TangentContinuationEnd = 128,
    /// An offset face's offset folds: the wall is thicker than a radius of curvature.
    OffsetFolds = 200,
    /// Loops of an offset face touch without crossing: the offset walls would meet.
    OffsetWallsTouch = 201,
    /// Loops of an offset face cross in a way the offset does not resolve.
    OffsetLoopsCross = 202,
    /// An offset would move a vertex to or past the other end of its edge.
    OffsetEdgeVanishes = 203,
    /// An offset face's offset collapses onto a line or point: the distance is a radius
    /// of curvature of the face on that side (a round inset by its radius).
    OffsetCollapses = 204,
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

    /// True for a failure that is the kernel's fault, not a fact of the geometry: report it.
    /// The blend's defect classes, and a port invariant that did not hold. The blend's
    /// [`ErrorKind::StageFailed`] is one: a stage failed with no reason the kernel names.
    #[must_use]
    pub const fn is_defect(self) -> bool {
        matches!(
            self,
            Self::CliffSubtractionLeavesNoSolid
                | Self::StationDidNotConverge
                | Self::TrimLeaks
                | Self::StageFailed
                | Self::PortInvariant
        )
    }

    /// True for a refusal: the operation does not build on this input, a fact of the
    /// geometry, so the way out is a different input. The blend's refusal classes, a fillet
    /// too large, and the offset's refusals.
    #[must_use]
    pub const fn is_refusal(self) -> bool {
        matches!(
            self,
            Self::FilletTooLarge
                | Self::BallDoesNotFit
                | Self::RailLeavesFace
                | Self::FaceConsumed
                | Self::BandsOverlap
                | Self::BallsFold
                | Self::ChamferOffsetFolds
                | Self::CutsThePart
                | Self::BreaksThrough
                | Self::BallOffsetCollapses
                | Self::CornersTakeWholeEdge
                | Self::RunsIntoMaterial
                | Self::CliffOverflowRefused
                | Self::CliffEndFaceNotPlanar
                | Self::CliffOverflowShape
                | Self::CreaseAcrossTangentEdge
                | Self::RailBelowResolution
                | Self::RailPastAxis
                | Self::JoinOffsetsDiffer
                | Self::StripBelowResolution
                | Self::EndPastSideEdge
                | Self::EndOverflowsSideEdge
                | Self::EndRollsIntoSmoothEnd
                | Self::EndAtSeamVertex
                | Self::TangentContinuationEnd
                | Self::OffsetFolds
                | Self::OffsetWallsTouch
                | Self::OffsetLoopsCross
                | Self::OffsetEdgeVanishes
                | Self::OffsetCollapses
        )
    }

    /// True for a refusal a size that builds is the way out of (false for a missing
    /// construction and for a defect): every refusal class but a contact line that leaves its
    /// face, a round that ends where its edge runs on tangentially, and the offset's that the
    /// size of its wall does not decide.
    #[must_use]
    pub const fn sized(self) -> bool {
        self.is_refusal()
            && !matches!(
                self,
                Self::RailLeavesFace
                    | Self::TangentContinuationEnd
                    | Self::OffsetWallsTouch
                    | Self::OffsetLoopsCross
                    | Self::OffsetEdgeVanishes
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
            Self::BallDoesNotFit => "ball_does_not_fit",
            Self::RailLeavesFace => "rail_leaves_face",
            Self::FaceConsumed => "face_consumed",
            Self::BandsOverlap => "bands_overlap",
            Self::BallsFold => "balls_fold",
            Self::ChamferOffsetFolds => "chamfer_offset_folds",
            Self::CutsThePart => "cuts_the_part",
            Self::BreaksThrough => "breaks_through",
            Self::BallOffsetCollapses => "ball_offset_collapses",
            Self::CornersTakeWholeEdge => "corners_take_whole_edge",
            Self::RunsIntoMaterial => "runs_into_material",
            Self::CliffOverflowRefused => "cliff_overflow_refused",
            Self::CliffEndFaceNotPlanar => "cliff_end_face_not_planar",
            Self::CliffOverflowShape => "cliff_overflow_shape",
            Self::CliffSubtractionLeavesNoSolid => "cliff_subtraction_leaves_no_solid",
            Self::CreaseAcrossTangentEdge => "crease_across_tangent_edge",
            Self::RailBelowResolution => "rail_below_resolution",
            Self::RailPastAxis => "rail_past_axis",
            Self::JoinOffsetsDiffer => "join_offsets_differ",
            Self::StripBelowResolution => "strip_below_resolution",
            Self::EndPastSideEdge => "end_past_side_edge",
            Self::EndOverflowsSideEdge => "end_overflows_side_edge",
            Self::EndRollsIntoSmoothEnd => "end_rolls_into_smooth_end",
            Self::EndAtSeamVertex => "end_at_seam_vertex",
            Self::StationDidNotConverge => "station_did_not_converge",
            Self::TrimLeaks => "trim_leaks",
            Self::StageFailed => "stage_failed",
            Self::TangentContinuationEnd => "tangent_continuation_end",
            Self::OffsetFolds => "offset_folds",
            Self::OffsetWallsTouch => "offset_walls_touch",
            Self::OffsetLoopsCross => "offset_loops_cross",
            Self::OffsetEdgeVanishes => "offset_edge_vanishes",
            Self::OffsetCollapses => "offset_collapses",
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

/// The largest size (a distance) that works, as a refusal's message ends with it, when there
/// is one.
fn fmt_size(size: Option<f64>) -> String {
    size.map_or_else(String::new, |s| {
        format!("; the largest size that works is {s}")
    })
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
    /// [`SolidError::Cancelled`] through. Being replaced by
    /// [`SolidError::nurbs_routine`], which keeps the error as the cause.
    /// Pass `nl_ser(OP)` to `map_err`.
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
            Self::Upstream { code, .. }
            | Self::FilletRefused { code, .. }
            | Self::BlendRefused { code, .. }
            | Self::RoundRefused { code, .. } => Some(*code),
            Self::Failed(f) => Some(f.status),
            Self::InvalidInput { .. }
            | Self::NotFound { .. }
            | Self::Degenerate { .. }
            | Self::NotConverged { .. }
            | Self::Unsupported { .. }
            | Self::PortInvariant { .. }
            | Self::PreparationRequired { .. }
            | Self::Cancelled { .. }
            | Self::FaceTrimsUnavailable { .. }
            | Self::UnsureRegion { .. }
            | Self::ExtendRefused { .. }
            | Self::DefinitionNotCertified { .. }
            | Self::Unported { .. } => None,
        }
    }

    /// The status of an error that answers with an `SmStatus` the way a
    /// [`SolidError::Upstream`] does: its `code`, or the status of a failure that replaced
    /// an `Upstream` site ([`nurbs::BSplineFailure`], [`intersect::IntersectFailure`],
    /// [`primitive::PrimitiveFailure`], [`mass::MassFailure`], [`heal::HealFailure`],
    /// [`geom::GeomFailure`], [`query::QueryFailure`], [`topology::TopologyFailure`],
    /// [`surface::SurfaceFailure`], a [`sweep::SweepFailure`], [`curve::CurveFailure`] or
    /// [`offset::OffsetFailure`] or [`blend::BlendFailure`] that replaced one
    /// ([`sweep::SweepFailure::replaces_upstream_status`],
    /// [`curve::CurveFailure::replaces_upstream_status`],
    /// [`offset::OffsetFailure::replaces_upstream_status`],
    /// [`blend::BlendFailure::replaces_upstream_status`])). The callers that branch on an
    /// upstream status use it so that they treat both alike; it goes with `Upstream` in
    /// the last PR of the typed-errors work.
    #[must_use]
    pub(crate) fn upstream_status(&self) -> Option<i32> {
        match self {
            Self::Upstream { code, .. } => Some(*code),
            Self::Failed(f) => match &f.reason {
                Reason::Nurbs(nurbs::NurbsFailure::BSpline(_))
                | Reason::Intersect(_)
                | Reason::Primitive(_)
                | Reason::Mass(_)
                | Reason::Heal(_)
                | Reason::Geom(_)
                | Reason::Query(_)
                | Reason::Topology(_)
                | Reason::Surface(_) => Some(f.status),
                Reason::Sweep(w) if w.replaces_upstream_status() => Some(f.status),
                Reason::Curve(c) if c.replaces_upstream_status() => Some(f.status),
                Reason::Offset(o) if o.replaces_upstream_status() => Some(f.status),
                Reason::Blend(b) if b.replaces_upstream_status() => Some(f.status),
                _ => None,
            },
            _ => None,
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
            Self::InvalidInput { .. } | Self::DefinitionNotCertified { .. } => {
                ErrorKind::InvalidInput
            }
            Self::NotFound { .. } => ErrorKind::NotFound,
            Self::Degenerate { .. } => ErrorKind::Degenerate,
            Self::NotConverged { .. } => ErrorKind::NotConverged,
            // a region the kernel cannot classify, or a side it cannot extend: unsupported
            Self::Unsupported { .. } | Self::UnsureRegion { .. } | Self::ExtendRefused { .. } => {
                ErrorKind::Unsupported
            }
            Self::FaceTrimsUnavailable { .. } => ErrorKind::FaceTrimsUnavailable,
            Self::Upstream { code, .. } => ErrorKind::of_status(*code),
            Self::PortInvariant { .. } => ErrorKind::PortInvariant,
            Self::PreparationRequired { .. } => ErrorKind::PreparationRequired,
            Self::Cancelled { .. } => ErrorKind::Cancelled,
            Self::FilletRefused { .. } => ErrorKind::FilletTooLarge,
            Self::BlendRefused { code, .. } | Self::RoundRefused { code, .. } => {
                ErrorKind::of_status(*code)
            }
            Self::Failed(f) => f.kind(),
            Self::Unported { .. } => ErrorKind::Unported,
        }
    }

    /// True for a solidcore-side failure rather than a failure of the operation on its
    /// input ([`ErrorKind::is_port_failure`], #1452). It follows the cause chain: a
    /// failure whose kept cause ([`SolidError::cause`], down to
    /// [`SolidError::root_cause`]) is a solidcore-side failure is one too, so a caller that
    /// skips an entity where the operation fails on it does not skip when the stage that
    /// failed was an unported call or a missing preparation.
    #[must_use]
    pub fn is_port_failure(&self) -> bool {
        let mut e = self;
        loop {
            if e.kind().is_port_failure() {
                return true;
            }
            match e.cause() {
                Some(cause) => e = cause,
                None => return false,
            }
        }
    }

    /// The class that decides whether this error is a defect or a refusal: that of the
    /// deepest error in its cause chain ([`SolidError::cause`], down to
    /// [`SolidError::root_cause`]) whose kind is a defect or a refusal, so a stage failure
    /// over a refusal reads as the refusal. A blend phrase cause
    /// ([`blend::BlendFailure::is_phrase`]) is neither until its site is converted, whatever
    /// its status's kind; `None` when no error in the chain is classed.
    fn deciding_class(&self) -> Option<ErrorKind> {
        std::iter::successors(Some(self), |e| e.cause())
            .filter(|e| {
                !e.blend_failure()
                    .is_some_and(blend::BlendFailure::is_phrase)
            })
            .map(Self::kind)
            .filter(|k| k.is_defect() || k.is_refusal())
            .last()
    }

    /// True when this error is the kernel's fault ([`ErrorKind::is_defect`]): report it. The
    /// deepest classed error in its cause chain decides, as [`SolidError::is_port_failure`]
    /// follows the chain; a blend phrase cause is not one. Never true together with
    /// [`SolidError::is_refusal`].
    #[must_use]
    pub fn is_defect(&self) -> bool {
        self.deciding_class().is_some_and(ErrorKind::is_defect)
    }

    /// True when this error is a refusal ([`ErrorKind::is_refusal`]): a fact of the geometry,
    /// so the way out is a different input. The deepest classed error in its cause chain
    /// decides; a blend phrase cause is not one.
    #[must_use]
    pub fn is_refusal(&self) -> bool {
        self.deciding_class().is_some_and(ErrorKind::is_refusal)
    }

    /// True when this error is a refusal a size that builds is the way out of
    /// ([`ErrorKind::sized`]), decided as [`SolidError::is_refusal`] is.
    #[must_use]
    pub fn sized(&self) -> bool {
        self.deciding_class().is_some_and(ErrorKind::sized)
    }

    /// Where the failure is on the caller's body, for a [`SolidError::Failed`] ([`Site`]).
    #[must_use]
    pub fn site(&self) -> Option<&Site> {
        self.failure().map(|f| &f.site)
    }

    /// The size that builds, for a [`SolidError::Failed`] whose check computed one.
    #[must_use]
    pub fn fit(&self) -> Option<Fit> {
        self.failure().and_then(|f| f.fit)
    }

    /// This error with the empty slots of its site filled from `site` (a
    /// [`SolidError::Failed`]; other errors carry no site and are returned as they are). A
    /// slot already set is kept and nothing else changes, so a caller fills what it knows on
    /// the way up without changing what failed or any outcome.
    #[must_use]
    pub fn or_at(self, site: Site) -> Self {
        match self {
            Self::Failed(mut f) => {
                f.site = f.site.or(site);
                Self::Failed(f)
            }
            other => other,
        }
    }

    /// True when the error is a kernel status the operation answers with, and so stands for
    /// "no answer" rather than a failure of the call: it is exactly a status
    /// ([`SolidError::sm_status`]) and nothing in its cause chain ([`SolidError::cause`]) is a
    /// solidcore-side failure ([`SolidError::is_port_failure`]) or a cancellation
    /// ([`SolidError::is_cancelled`]). A stage that failed because a call was unported, or
    /// because the caller cancelled, is no answer of the operation on its input.
    #[must_use]
    pub fn is_status_answer(&self) -> bool {
        self.sm_status().is_some()
            && !self.is_port_failure()
            && !std::iter::successors(Some(self), |e| e.cause()).any(Self::is_cancelled)
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

    /// [`SolidError::Failed`] for a stage that failed, as the phrase says, with status
    /// [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    pub(crate) fn stage_failed(operation: &'static str, what: &'static str) -> Self {
        Failure::new(operation, Reason::Failed(what)).into_error()
    }

    /// [`SolidError::Failed`] for an entity a stage needs that is not there, as the phrase
    /// says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a `NULL`
    /// pointer); its kind is [`ErrorKind::Missing`].
    pub(crate) fn entity_missing(operation: &'static str, what: &'static str) -> Self {
        Failure::new(operation, Reason::Failed(what))
            .with_status(SM_ERR_NULL_POINTER)
            .into_error()
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
            | Self::UnsureRegion { operation, .. }
            | Self::PortInvariant { operation, .. }
            | Self::Upstream { operation, .. }
            | Self::FilletRefused { operation, .. }
            | Self::BlendRefused { operation, .. }
            | Self::RoundRefused { operation, .. }
            | Self::ExtendRefused { operation, .. }
            | Self::DefinitionNotCertified { operation, .. }
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
            Self::NotFound { entity, .. }
            | Self::FaceTrimsUnavailable { entity, .. }
            | Self::DefinitionNotCertified { face: entity, .. } => Some(*entity),
            Self::UnsureRegion { region, .. } => Some(EntityRef::Region(*region)),
            Self::Degenerate { entity, .. }
            | Self::Upstream { entity, .. }
            | Self::FilletRefused { entity, .. } => *entity,
            Self::BlendRefused { refusal, .. } => Some(refusal.entity()),
            Self::Failed(f) => f.entity,
            Self::InvalidInput { .. }
            | Self::NotConverged { .. }
            | Self::Unsupported { .. }
            | Self::PortInvariant { .. }
            | Self::ExtendRefused { .. }
            | Self::Unported { .. }
            | Self::Cancelled { .. }
            | Self::RoundRefused { .. }
            | Self::PreparationRequired { .. } => None,
        }
    }
}

/// The status `error` answers with the way a [`SolidError::Upstream`] does: an
/// `Upstream`'s code, or the status of a failure that replaced an `Upstream` site
/// ([`nurbs::BSplineFailure`], [`intersect::IntersectFailure`],
/// [`primitive::PrimitiveFailure`], [`mass::MassFailure`], [`heal::HealFailure`],
/// [`geom::GeomFailure`], [`query::QueryFailure`], [`topology::TopologyFailure`],
/// [`surface::SurfaceFailure`], a [`sweep::SweepFailure`], [`curve::CurveFailure`] or
/// [`offset::OffsetFailure`] or [`blend::BlendFailure`] that replaced one
/// ([`sweep::SweepFailure::replaces_upstream_status`],
/// [`curve::CurveFailure::replaces_upstream_status`],
/// [`offset::OffsetFailure::replaces_upstream_status`],
/// [`blend::BlendFailure::replaces_upstream_status`])).
///
/// For difftest's comparison only: it is removed with `Upstream` and `sm_status` in the
/// last PR of the typed-errors work. Callers match [`SolidError::kind`] and the
/// [`Reason`].
#[must_use]
pub fn upstream_status(error: &SolidError) -> Option<i32> {
    error.upstream_status()
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
            SolidError::nurbs("ray", nurbs::NurbsFailure::Numerical),
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
        assert_eq!(
            SolidError::nurbs_routine("native")(request.clone()),
            request
        );
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
            (
                SolidError::nurbs("op", nurbs::NurbsFailure::Knots),
                Some(SM_ERR),
            ),
            (
                SolidError::nurbs("op", nurbs::NurbsFailure::NotConverged),
                Some(SM_ERR),
            ),
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
            SolidError::nl_ser("native")(e.clone()),
            SolidError::sm_err("native")
        );
        // a B-spline method keeps it as the cause of its own failure
        let wrapped = SolidError::nurbs_routine("native")(e.clone());
        assert_eq!(wrapped.sm_status(), Some(SM_ERR));
        assert_eq!(wrapped.cause(), Some(&e));
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
    fn is_port_failure_follows_the_cause_chain() {
        let unported =
            SolidError::unported("topology::brep::p1::update_vertex_edge_intervals", "u");
        assert!(unported.is_port_failure());
        // a stage that failed because a call was not ported is not a failure of the
        // operation on its input, though the error on top is a generic one
        let wrapped = SolidError::failed_after("op", "a stage failed", unported.clone());
        assert_eq!(wrapped.kind(), ErrorKind::Failed);
        assert!(wrapped.is_port_failure());
        let deeper = SolidError::failed_after("outer", "the operation failed", wrapped);
        assert!(deeper.is_port_failure());
        assert_eq!(deeper.root_cause(), &unported);
        // a chain of ordinary failures is not one, and neither is a cancellation
        let ordinary = SolidError::failed_after(
            "outer",
            "the operation failed",
            SolidError::failed_after("op", "a stage failed", SolidError::sm_err("inner")),
        );
        assert!(!ordinary.is_port_failure());
        let cancelled =
            SolidError::failed_after("op", "a stage failed", SolidError::cancelled("c"));
        assert!(!cancelled.is_port_failure());
        // the port-failure kinds on top still count
        assert!(SolidError::port_invariant("op", "broken").is_port_failure());
        assert!(!SolidError::sm_err("op").is_port_failure());
    }

    #[test]
    fn boolean_failures_describe_the_stage_in_solidcore_terms() {
        use boolean::{BooleanFailure, BooleanStage, PlacedEntity, RelateSide};

        let face = FaceId::from_index(7);
        let cases = [
            (
                SolidError::boolean_stage_failed(
                    "boolean::topology_intersector::p1::relate",
                    BooleanStage::Relate,
                    "another entity is already related to the other one",
                ),
                "boolean::topology_intersector::p1::relate: relating the operands' entities \
                 failed: another entity is already related to the other one",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                SolidError::boolean_entity_missing(
                    "boolean::topology_intersector::p2::close_intersection_loop",
                    BooleanStage::CloseLoop,
                    "a vertex to close the loop at",
                ),
                "boolean::topology_intersector::p2::close_intersection_loop: closing an \
                 intersection loop needs a vertex to close the loop at, which the B-rep does \
                 not have",
                ErrorKind::Missing,
                SM_ERR_NULL_POINTER,
            ),
            (
                Failure::new(
                    "boolean::merge::p4::manifold_boolean",
                    BooleanFailure::SolidLeaks { face },
                )
                .into_error(),
                "boolean::merge::p4::manifold_boolean: the union would delete face 7 and merge \
                 the solid it bounds into the outside",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                SolidError::boolean_relate_conflict(
                    "boolean::topology_intersector::p1::relate",
                    RelateSide::Brep,
                    PlacedEntity::Edge(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 6.0, 0.0)),
                    PlacedEntity::Edge(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 6.0, 0.0)),
                    PlacedEntity::Edge(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 6.0, 0.5)),
                ),
                "boolean::topology_intersector::p1::relate: relating the operands' entities \
                 failed: the edge from (0, 0, 0) to (0, 6, 0) of the B-rep is related to the \
                 edge from (0, 0, 0) to (0, 6, 0) and was asked for the edge from (0, 0, 0) to \
                 (0, 6, 0.5): another entity is already related to the other one",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                SolidError::boolean_relate_conflict(
                    "boolean::topology_intersector::p1::relate",
                    RelateSide::Other,
                    PlacedEntity::Vertex(Point3::new(1.5, -2.0, 0.25)),
                    PlacedEntity::Unplaced,
                    PlacedEntity::Vertex(Point3::new(1.5, -2.0, 0.2501)),
                ),
                "boolean::topology_intersector::p1::relate: relating the operands' entities \
                 failed: the vertex at (1.5, -2, 0.25) of the other B-rep is related to an \
                 entity and was asked for the vertex at (1.5, -2, 0.2501): another entity is \
                 already related to the other one",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                SolidError::stage_failed("boolean::merge::p3", "the Boolean tree has no result"),
                "boolean::merge::p3: the Boolean tree has no result",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                SolidError::entity_missing("boolean::merge::p1", "the shell lies in no region"),
                "boolean::merge::p1: the shell lies in no region",
                ErrorKind::Missing,
                SM_ERR_NULL_POINTER,
            ),
        ];
        for (error, message, kind, status) in cases {
            assert_eq!(error.to_string(), message);
            assert_eq!(error.kind(), kind, "{error}");
            // the comparison status is unchanged, and never part of the message
            assert_eq!(error.sm_status(), Some(status), "{error}");
            let text = error.to_string();
            for forbidden in ["Sm", "NL_", "SM_ERR", "1001", "1006", "status"] {
                assert!(!text.contains(forbidden), "{forbidden} in {text}");
            }
            assert!(!error.is_port_failure());
        }
        let leak = Failure::new("op", BooleanFailure::SolidLeaks { face }).into_error();
        assert_eq!(leak.entity(), Some(EntityRef::Face(face)));
    }

    /// Every kind's code and name, pinned: a code or name never changes meaning once
    /// released. A new kind adds its row here (the match below does not compile without it).
    #[test]
    fn every_kind_keeps_its_code_and_name() {
        const PINNED: &[(ErrorKind, u16, &str)] = &[
            (ErrorKind::InvalidInput, 1, "invalid_input"),
            (ErrorKind::NotFound, 2, "not_found"),
            (ErrorKind::Degenerate, 3, "degenerate"),
            (ErrorKind::NotConverged, 4, "not_converged"),
            (ErrorKind::Unsupported, 5, "unsupported"),
            (ErrorKind::Failed, 6, "failed"),
            (ErrorKind::Missing, 7, "missing"),
            (ErrorKind::OutsideDomain, 8, "outside_domain"),
            (ErrorKind::NotWithinTolerance, 9, "not_within_tolerance"),
            (ErrorKind::PortInvariant, 90, "port_invariant"),
            (ErrorKind::Unported, 91, "unported"),
            (ErrorKind::Cancelled, 92, "cancelled"),
            (ErrorKind::PreparationRequired, 93, "preparation_required"),
            (
                ErrorKind::FaceTrimsUnavailable,
                94,
                "face_trims_unavailable",
            ),
            (ErrorKind::FilletTooLarge, 100, "fillet_too_large"),
            (ErrorKind::BallDoesNotFit, 101, "ball_does_not_fit"),
            (ErrorKind::RailLeavesFace, 102, "rail_leaves_face"),
            (ErrorKind::FaceConsumed, 103, "face_consumed"),
            (ErrorKind::BandsOverlap, 104, "bands_overlap"),
            (ErrorKind::BallsFold, 105, "balls_fold"),
            (ErrorKind::ChamferOffsetFolds, 106, "chamfer_offset_folds"),
            (ErrorKind::CutsThePart, 107, "cuts_the_part"),
            (ErrorKind::BreaksThrough, 108, "breaks_through"),
            (ErrorKind::BallOffsetCollapses, 109, "ball_offset_collapses"),
            (
                ErrorKind::CornersTakeWholeEdge,
                110,
                "corners_take_whole_edge",
            ),
            (ErrorKind::RunsIntoMaterial, 111, "runs_into_material"),
            (
                ErrorKind::CliffOverflowRefused,
                112,
                "cliff_overflow_refused",
            ),
            (
                ErrorKind::CliffEndFaceNotPlanar,
                113,
                "cliff_end_face_not_planar",
            ),
            (ErrorKind::CliffOverflowShape, 114, "cliff_overflow_shape"),
            (
                ErrorKind::CliffSubtractionLeavesNoSolid,
                115,
                "cliff_subtraction_leaves_no_solid",
            ),
            (
                ErrorKind::CreaseAcrossTangentEdge,
                116,
                "crease_across_tangent_edge",
            ),
            (ErrorKind::RailBelowResolution, 117, "rail_below_resolution"),
            (ErrorKind::RailPastAxis, 118, "rail_past_axis"),
            (ErrorKind::JoinOffsetsDiffer, 119, "join_offsets_differ"),
            (
                ErrorKind::StripBelowResolution,
                120,
                "strip_below_resolution",
            ),
            (ErrorKind::EndPastSideEdge, 121, "end_past_side_edge"),
            (
                ErrorKind::EndOverflowsSideEdge,
                122,
                "end_overflows_side_edge",
            ),
            (
                ErrorKind::EndRollsIntoSmoothEnd,
                123,
                "end_rolls_into_smooth_end",
            ),
            (ErrorKind::EndAtSeamVertex, 124, "end_at_seam_vertex"),
            (
                ErrorKind::StationDidNotConverge,
                125,
                "station_did_not_converge",
            ),
            (ErrorKind::TrimLeaks, 126, "trim_leaks"),
            (ErrorKind::StageFailed, 127, "stage_failed"),
            (
                ErrorKind::TangentContinuationEnd,
                128,
                "tangent_continuation_end",
            ),
            (ErrorKind::OffsetFolds, 200, "offset_folds"),
            (ErrorKind::OffsetWallsTouch, 201, "offset_walls_touch"),
            (ErrorKind::OffsetLoopsCross, 202, "offset_loops_cross"),
            (ErrorKind::OffsetEdgeVanishes, 203, "offset_edge_vanishes"),
            (ErrorKind::OffsetCollapses, 204, "offset_collapses"),
        ];
        let row = |k: ErrorKind| -> usize {
            match k {
                ErrorKind::InvalidInput => 0,
                ErrorKind::NotFound => 1,
                ErrorKind::Degenerate => 2,
                ErrorKind::NotConverged => 3,
                ErrorKind::Unsupported => 4,
                ErrorKind::Failed => 5,
                ErrorKind::Missing => 6,
                ErrorKind::OutsideDomain => 7,
                ErrorKind::NotWithinTolerance => 8,
                ErrorKind::PortInvariant => 9,
                ErrorKind::Unported => 10,
                ErrorKind::Cancelled => 11,
                ErrorKind::PreparationRequired => 12,
                ErrorKind::FaceTrimsUnavailable => 13,
                ErrorKind::FilletTooLarge => 14,
                ErrorKind::BallDoesNotFit => 15,
                ErrorKind::RailLeavesFace => 16,
                ErrorKind::FaceConsumed => 17,
                ErrorKind::BandsOverlap => 18,
                ErrorKind::BallsFold => 19,
                ErrorKind::ChamferOffsetFolds => 20,
                ErrorKind::CutsThePart => 21,
                ErrorKind::BreaksThrough => 22,
                ErrorKind::BallOffsetCollapses => 23,
                ErrorKind::CornersTakeWholeEdge => 24,
                ErrorKind::RunsIntoMaterial => 25,
                ErrorKind::CliffOverflowRefused => 26,
                ErrorKind::CliffEndFaceNotPlanar => 27,
                ErrorKind::CliffOverflowShape => 28,
                ErrorKind::CliffSubtractionLeavesNoSolid => 29,
                ErrorKind::CreaseAcrossTangentEdge => 30,
                ErrorKind::RailBelowResolution => 31,
                ErrorKind::RailPastAxis => 32,
                ErrorKind::JoinOffsetsDiffer => 33,
                ErrorKind::StripBelowResolution => 34,
                ErrorKind::EndPastSideEdge => 35,
                ErrorKind::EndOverflowsSideEdge => 36,
                ErrorKind::EndRollsIntoSmoothEnd => 37,
                ErrorKind::EndAtSeamVertex => 38,
                ErrorKind::StationDidNotConverge => 39,
                ErrorKind::TrimLeaks => 40,
                ErrorKind::StageFailed => 41,
                ErrorKind::TangentContinuationEnd => 42,
                ErrorKind::OffsetFolds => 43,
                ErrorKind::OffsetWallsTouch => 44,
                ErrorKind::OffsetLoopsCross => 45,
                ErrorKind::OffsetEdgeVanishes => 46,
                ErrorKind::OffsetCollapses => 47,
            }
        };
        for (i, &(kind, code, name)) in PINNED.iter().enumerate() {
            assert_eq!(row(kind), i, "{kind:?} is listed once, in order");
            assert_eq!(kind.code(), code, "{kind:?}");
            assert_eq!(kind.name(), name, "{kind:?}");
            assert_eq!(kind.to_string(), name);
        }
        assert!(
            PINNED.windows(2).all(|w| w[0].1 < w[1].1),
            "codes are unique and ascending"
        );
        // the blend's classes: 101 to 127 are the record's numbers less 200, with no gap; 128
        // follows them
        let blend: Vec<u16> = PINNED
            .iter()
            .map(|r| r.1)
            .filter(|c| (101..200).contains(c))
            .collect();
        assert_eq!(blend, (101..=128).collect::<Vec<u16>>());
        // defects, refusals and the sized ones
        let defects = [115, 125, 126, 127];
        let unsized_refusals = [102, 128];
        for &(kind, code, _) in PINNED.iter().filter(|r| (101..=128).contains(&r.1)) {
            assert_eq!(kind.is_defect(), defects.contains(&code), "{kind:?}");
            assert_eq!(kind.is_refusal(), !defects.contains(&code), "{kind:?}");
            assert_eq!(
                kind.sized(),
                !defects.contains(&code) && !unsized_refusals.contains(&code),
                "{kind:?}"
            );
            assert!(!kind.is_port_failure(), "{kind:?}");
        }
        // 128, a round that ends where its edge runs on tangentially, is refused whatever its
        // size (the way out is to blend the edge it runs on into with it): it is not sized,
        // and its failure, as the cause builds it, offers no size that fits
        let tangent = ErrorKind::TangentContinuationEnd;
        assert_eq!(tangent.code(), 128);
        assert!(tangent.is_refusal() && !tangent.sized());
        let refused = SolidError::blend(
            "blend::op",
            blend::BlendFailure::NoRoom(
                crate::blend::fillet_executive::NoRoomReason::TangentContinuationEndUnsupported,
            ),
        );
        assert_eq!(refused.kind(), tangent);
        assert!(refused.is_refusal() && !refused.sized());
        assert_eq!(refused.fit(), None);
        assert!(ErrorKind::OffsetFolds.sized() && ErrorKind::OffsetCollapses.sized());
        assert!(ErrorKind::FilletTooLarge.sized());
        assert!(ErrorKind::PortInvariant.is_defect());
        for kind in [
            ErrorKind::Failed,
            ErrorKind::Missing,
            ErrorKind::Unported,
            ErrorKind::Cancelled,
        ] {
            assert!(
                !kind.is_defect() && !kind.is_refusal() && !kind.sized(),
                "{kind:?}"
            );
        }
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
            (
                SolidError::nurbs("op", nurbs::NurbsFailure::Knots),
                ErrorKind::Failed,
                6,
            ),
            (
                SolidError::nurbs("op", nurbs::NurbsFailure::NotConverged),
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
    fn nurbs_failures_describe_the_routine_in_solidcore_terms() {
        use nurbs::NurbsFailure as N;

        // (failure, message, kind, the NL number difftest compares)
        let cases = [
            (
                N::CurveDefinition,
                "the curve's control point, knot and degree counts do not agree",
                ErrorKind::Failed,
                1,
            ),
            (
                N::SurfaceDefinition,
                "the surface's control net, knot and degree counts do not agree",
                ErrorKind::Failed,
                2,
            ),
            (
                N::Degree,
                "the degree is not allowed here",
                ErrorKind::Failed,
                3,
            ),
            (
                N::DegreeTooHigh { given: 21, max: 16 },
                "degree 21 is above the largest degree, 16",
                ErrorKind::Failed,
                3,
            ),
            (
                N::Knots,
                "the knot vector is not valid here",
                ErrorKind::Failed,
                4,
            ),
            (
                N::KnotsDecrease { index: 4 },
                "the knot vector decreases at knot 4",
                ErrorKind::Failed,
                4,
            ),
            (
                N::Weight,
                "a weight is not valid here",
                ErrorKind::Failed,
                5,
            ),
            (
                N::WeightOutOfRange {
                    index: 1,
                    weight: -2.0,
                    min: 0.5,
                    max: 8.0,
                },
                "the weight of control point 1 is -2, outside [0.5, 8]",
                ErrorKind::Failed,
                5,
            ),
            (
                N::Parameter,
                "a parameter is outside the domain",
                ErrorKind::OutsideDomain,
                6,
            ),
            (
                N::ParameterOutside {
                    parameter: 2.5,
                    lo: 0.0,
                    hi: 1.0,
                },
                "parameter 2.5 is outside the domain [0, 1]",
                ErrorKind::OutsideDomain,
                6,
            ),
            (N::Index, "an index is out of range", ErrorKind::Failed, 7),
            (
                N::IndexOutOfRange { index: 9, len: 4 },
                "index 9 is out of range for 4 items",
                ErrorKind::Failed,
                7,
            ),
            (
                N::ZeroDerivative,
                "a derivative is zero where a direction is needed",
                ErrorKind::Failed,
                8,
            ),
            (
                N::Storage,
                "a buffer supplied by the caller is too small",
                ErrorKind::Failed,
                9,
            ),
            (
                N::Numerical,
                "a numerical failure (a division by zero or overflow)",
                ErrorKind::Failed,
                11,
            ),
            (
                N::NotConverged,
                "the iteration did not converge",
                ErrorKind::NotConverged,
                12,
            ),
            (
                N::CallFailed,
                "a routine it calls rejected its arguments, or the direction is not handled",
                ErrorKind::Failed,
                13,
            ),
            (
                N::Input,
                "the input data is not valid",
                ErrorKind::Failed,
                14,
            ),
            (
                N::Memory,
                "the memory for the result cannot be allocated",
                ErrorKind::Failed,
                15,
            ),
            (
                N::Singular,
                "the system of equations is singular",
                ErrorKind::Failed,
                16,
            ),
            (
                N::Geometry,
                "the geometry does not allow the operation",
                ErrorKind::Failed,
                17,
            ),
            (
                N::NoDerivativeRows,
                "the output has no row for the point itself",
                ErrorKind::Failed,
                20,
            ),
            (
                N::DerivativeOrder { given: 7, max: 4 },
                "derivative order 7 is above the largest order, 4",
                ErrorKind::Failed,
                20,
            ),
        ];
        for (failure, message, kind, number) in cases {
            let error = SolidError::nurbs("nurbs::knots::insert", failure);
            assert_eq!(
                error.to_string(),
                format!("nurbs::knots::insert: {message}")
            );
            assert_eq!(error.kind(), kind, "{error}");
            assert_eq!(error.operation(), "nurbs::knots::insert");
            assert_eq!(error.nurbs_failure(), Some(&failure));
            assert!(error.is_nurbs_failure());
            assert!(!error.is_port_failure());
            // the comparison hooks: the status the Sm* callers report, never the NL number
            assert_eq!(error.sm_status(), Some(SM_ERR), "{error}");
            assert_eq!(nurbs::nlib_number(&error), Some(number), "{error}");
            // no status number, NLib name or SMLib name in the message
            let text = error.to_string();
            for forbidden in ["NL_", "Nlib", "NLib", "Sm", "SM_ERR", "status"] {
                assert!(!text.contains(forbidden), "{forbidden} in {text}");
            }
        }
        // a failure that is not a NURBS one has no NL number
        assert_eq!(nurbs::nlib_number(&SolidError::sm_err("op")), None);
        assert!(!SolidError::sm_err("op").is_nurbs_failure());
        // a NURBS failure collapsed to a generic one keeps the NURBS failure as the cause
        let collapsed = SolidError::failed_after(
            "op",
            "a stage failed",
            SolidError::nurbs("inner", N::Singular),
        );
        assert!(!collapsed.is_nurbs_failure());
        assert_eq!(collapsed.root_cause().nurbs_failure(), Some(&N::Singular));
    }

    #[test]
    fn b_spline_failures_keep_the_status_their_methods_returned() {
        use nurbs::{BSplineFailure as B, NurbsFailure as N};

        // (failure, message, kind, the status the C++ method returns)
        let cases = [
            (
                B::RoutineFailed,
                "a NURBS routine it calls failed",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                B::NoNurbsForm,
                "it holds no NURBS form",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                B::Missing("the spine rail"),
                "the spine rail is missing",
                ErrorKind::Missing,
                SM_ERR_NULL_POINTER,
            ),
            (
                B::ZeroAxis,
                "the axis direction is a zero vector",
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                B::DegreeOutside {
                    given: 1,
                    min: 2,
                    max: 17,
                },
                "degree 1 is outside [2, 17]",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                B::ParameterOutside {
                    parameter: 1.5,
                    lo: 0.0,
                    hi: 1.0,
                },
                "parameter 1.5 is outside the domain [0, 1]",
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                B::Failed("the arcs have different radii"),
                "the arcs have different radii",
                ErrorKind::Failed,
                SM_ERR,
            ),
        ];
        for (failure, message, kind, status) in cases {
            let op = "nurbs::b_spline_surface::p2::BSplineSurface::create_sphere_from_arcs";
            let error = SolidError::b_spline(op, failure);
            assert_eq!(error.to_string(), format!("{op}: {message}"));
            assert_eq!(error.operation(), op);
            assert_eq!(error.nurbs_failure(), Some(&N::BSpline(failure)));
            // the same kind and statuses as the `Upstream` the method returned before
            let before = SolidError::Upstream {
                operation: op,
                code: status,
                entity: None,
            };
            assert_eq!(error.kind(), kind, "{error}");
            assert_eq!(error.kind(), before.kind(), "{error}");
            assert_eq!(error.sm_status(), before.sm_status(), "{error}");
            assert_eq!(error.upstream_status(), Some(status), "{error}");
            assert_eq!(error.is_port_failure(), before.is_port_failure());
            assert_eq!(error.entity(), None);
            // not an NLib routine's failure: no NL number, and no caller drops it as one
            assert_eq!(nurbs::nlib_number(&error), None);
            assert!(!error.is_nurbs_failure());
            let text = error.to_string();
            let reason = text.trim_start_matches(op);
            for forbidden in ["NL_", "Nlib", "NLib", "Sm", "SM_ERR", "status"] {
                assert!(!reason.contains(forbidden), "{forbidden} in {text}");
            }
        }
        // only `Upstream` and the B-spline failures answer with an upstream status
        assert_eq!(
            SolidError::null_pointer("op").upstream_status(),
            Some(SM_ERR_NULL_POINTER)
        );
        assert_eq!(SolidError::nurbs("op", N::Knots).upstream_status(), None);
        assert_eq!(
            SolidError::failed_after("op", "a stage failed", SolidError::sm_err("in"))
                .upstream_status(),
            None
        );
        assert_eq!(
            SolidError::invalid_input("op", "bad").upstream_status(),
            None
        );
    }

    /// A B-spline method whose NLib call fails keeps that failure as its cause, with the
    /// status `SM_ERR` it returned before; the answers no upstream status stands for pass
    /// through unwrapped, so callers (and SolidCraft through solidcore-ops) still see
    /// them at the top.
    #[test]
    fn a_failed_nurbs_routine_is_kept_as_the_cause_and_port_answers_pass_through() {
        use nurbs::{BSplineFailure as B, NurbsFailure as N};
        let op = "nurbs::b_spline_curve_n_lib::BSplineCurve::degree_reduction";
        let wrap = SolidError::nurbs_routine(op);

        let inner = SolidError::nurbs("nurbs::crv_approx::reduce_degree", N::Singular);
        let e = wrap(inner.clone());
        assert_eq!(e.nurbs_failure(), Some(&N::BSpline(B::RoutineFailed)));
        assert_eq!(e.cause(), Some(&inner));
        assert_eq!(e.root_cause().nurbs_failure(), Some(&N::Singular));
        assert_eq!(e.operation(), op);
        // what `nl_ser` gave: SM_ERR, kind Failed, no NL number, not an NLib failure
        let collapsed = SolidError::nl_ser(op)(inner.clone());
        assert_eq!(e.sm_status(), collapsed.sm_status());
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert_eq!(e.kind(), collapsed.kind());
        assert_eq!(e.upstream_status(), collapsed.upstream_status());
        assert_eq!(nurbs::nlib_number(&e), None);
        assert!(!e.is_nurbs_failure());
        assert!(!e.is_port_failure());
        assert_eq!(
            e.to_string(),
            format!(
                "{op}: a NURBS routine it calls failed; caused by \
                 nurbs::crv_approx::reduce_degree: the system of equations is singular"
            )
        );

        // any other error is kept as the cause the same way
        let invalid = SolidError::invalid_input("nurbs::knots::insert", "bad knot");
        assert_eq!(wrap(invalid.clone()).cause(), Some(&invalid));
        assert_eq!(wrap(invalid).sm_status(), Some(SM_ERR));

        // the answers no upstream status stands for come through as they are
        let passed = [
            SolidError::unported(
                "SmBSplineCurve::DegreeReduction",
                "nurbs.b_spline_curve_n_lib",
            ),
            SolidError::cancelled("nurbs::crv_approx::reduce_degree"),
            SolidError::PreparationRequired {
                operation: "nurbs::crv_approx::reduce_degree",
            },
        ];
        for p in passed {
            let out = wrap(p.clone());
            assert_eq!(out, p);
            assert_eq!(out, SolidError::nl_ser(op)(p.clone()));
            assert_eq!(out.kind(), p.kind());
            assert_eq!(out.sm_status(), None);
            assert!(out.cause().is_none());
        }
        assert!(wrap(SolidError::cancelled("x")).is_cancelled());
        assert!(wrap(SolidError::unported("SmX", "x")).is_unported());
        // a routine that failed on its input is a status answer, as the collapsed SM_ERR was
        assert!(e.is_status_answer());
        // a solidcore-side failure kept as the cause makes the method's failure one too
        // (is_port_failure follows the cause chain): it is no answer of the operation
        let port = wrap(SolidError::port_invariant("nurbs::crv_basic", "no curve"));
        assert!(port.is_port_failure());
        assert!(!port.is_status_answer());
        assert_eq!(port.sm_status(), Some(SM_ERR));
    }

    /// The NURBS routines report their failures as `NurbsFailure`s, and `SolidError::Nlib`,
    /// `SolidError::nlib` and `NlibCode` are gone: no library source file of the crate
    /// builds, matches or names an NLib error number variant any more.
    #[test]
    fn no_source_builds_an_nlib_error() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .expect("read_dir")
                .map(|e| e.expect("entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|x| x == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(files.len() > 400, "{} files under {root:?}", files.len());
        assert!(files.iter().any(|p| p.ends_with("nurbs/crv_common.rs")));
        for path in files {
            // a checkout with CRLF line endings reads the same
            let text = std::fs::read_to_string(&path)
                .expect("read")
                .replace("\r\n", "\n");
            // this test's own needles sit in its test module
            let library = bare_scan::library_text(&text);
            for needle in [
                "SolidError::Nlib",
                "Self::Nlib",
                "Nlib {",
                "NlibCode",
                "SolidError::nlib(",
                "fn nlib(",
            ] {
                assert!(!library.contains(needle), "{needle} in {path:?}");
            }
        }
    }

    /// The B-spline methods describe their failures ([`nurbs::BSplineFailure`]): their
    /// library code builds no bare `Upstream` status, and no source file but the one
    /// held back collapses an NLib failure without keeping it (`nl_ser`).
    #[test]
    fn the_b_spline_methods_build_no_bare_statuses() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let library = |rel: &str| {
            let text = std::fs::read_to_string(src.join(rel))
                .expect("read")
                .replace("\r\n", "\n");
            bare_scan::library_code(&text)
        };
        for rel in [
            "nurbs/b_spline_curve_n_lib.rs",
            "nurbs/b_spline_surface_n_lib/p1.rs",
            "nurbs/b_spline_surface_n_lib/p2.rs",
            "nurbs/b_spline_surface_n_lib/approx.rs",
            "nurbs/b_spline_surface/p1.rs",
            "nurbs/b_spline_surface/p2.rs",
            "nurbs/b_spline_curve/p2.rs",
            "nurbs/nurbs_crv.rs",
        ] {
            let text = library(rel);
            for needle in [
                "SolidError::sm_err(",
                "SolidError::null_pointer(",
                "SolidError::Upstream {",
                "nl_ser(",
            ] {
                assert!(!text.contains(needle), "{needle} in {rel}");
            }
        }
        for rel in [
            "surface/surface/approximate.rs",
            "surface/step_window.rs",
            "surface/surface_cache/p2.rs",
            "surface/surface/p9.rs",
            "curve/curve_cache.rs",
            "curve/ellipse/p1.rs",
            "offset/offset_executive/reach.rs",
            "mass/integrator.rs",
        ] {
            assert!(!library(rel).contains("nl_ser("), "nl_ser in {rel}");
        }
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
        assert_eq!(SolidError::nurbs_routine("native")(c.clone()), c);
    }
}

/// Samples for the tests of the decisions that read [`SolidError::upstream_status`] (#13790):
/// typed failures that replaced an [`SolidError::Upstream`], and typed failures that never
/// stood for an upstream status.
#[cfg(test)]
pub(crate) mod replaced_upstream_samples {
    use super::SolidError;

    /// The typed failures of five areas that replaced an `Upstream` with `code` (#13790):
    /// an intersection's, a geometry utility's, a topology routine's, an offset routine's and
    /// a blend routine's, at `entity` when one is given.
    pub(crate) fn replaced_upstream(
        operation: &'static str,
        code: i32,
        entity: Option<crate::error::EntityRef>,
    ) -> [SolidError; 5] {
        use crate::error::{
            blend::BlendFailure, geom::GeomFailure, intersect::IntersectFailure,
            offset::OffsetFailure, topology::TopologyFailure, Failure, Reason,
        };
        let typed = |reason: Reason| {
            let f = Failure::new(operation, reason).with_status(code);
            match entity {
                Some(e) => f.at(e),
                None => f,
            }
            .into_error()
        };
        [
            typed(Reason::Intersect(IntersectFailure::Failed(
                "replaced an upstream status",
            ))),
            typed(Reason::Geom(GeomFailure::Failed(
                "replaced an upstream status",
            ))),
            typed(Reason::Topology(TopologyFailure::Failed(
                "replaced an upstream status",
            ))),
            typed(Reason::Offset(OffsetFailure::Failed(
                "replaced an upstream status",
            ))),
            typed(Reason::Blend(BlendFailure::Failed(
                "replaced an upstream status",
            ))),
        ]
    }

    /// Typed failures with `code` that never stood for an upstream status (#13790): a sweep's
    /// station cap and a curve length's floor.
    pub(crate) fn native_failures(operation: &'static str, code: i32) -> [SolidError; 2] {
        use crate::error::{curve::CurveFailure, sweep::SweepFailure, Failure, Reason};
        let typed = |reason: Reason| {
            Failure::new(operation, reason)
                .with_status(code)
                .into_error()
        };
        let natives = [
            typed(Reason::Sweep(SweepFailure::RailPastStationCap {
                asked: 1e-6,
                reached: 1e-3,
                wires: 64,
            })),
            typed(Reason::Curve(CurveFailure::LengthBelowFloor {
                asked: 1e-12,
                floor: 1e-9,
                pieces: 8,
            })),
        ];
        for e in &natives {
            assert_eq!(e.upstream_status(), None, "{e:?}");
        }
        natives
    }
}
