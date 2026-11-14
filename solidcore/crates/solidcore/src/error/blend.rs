//! Why a blend fails, in solidcore's terms: the cause of the typed blend failure record.
//!
//! Upstream: none. These describe failures of the blend subsystem (`crate::blend`: the
//! fillet and feature executives, the fillet geometry, solvers, sections, corners,
//! overflows, intersectors, outlines, removal and the exact and rolling-ball fillets).
//!
//! **The record.** A blend failure is a [`Failure`] (shared by every area) whose reason is
//! a [`BlendFailure`], the cause: typed, with the numbers the check computed. Its class is
//! [`BlendFailure::class`], an [`ErrorKind`] derived from the cause, so class and cause
//! cannot disagree; [`ErrorKind::is_defect`], [`ErrorKind::is_refusal`] and
//! [`ErrorKind::sized`] say what kind of failure it is. The failure's [`super::Site`] says
//! where it is on the caller's body and its [`super::Fit`] the size that builds, where the
//! check computed one.
//!
//! | Cause                                  | Class ([`ErrorKind`])                         |
//! |----------------------------------------|-----------------------------------------------|
//! | [`BlendFailure::NoRoom`]               | the reason's ([`no_room_class`]), 101 to 124, 128 or 129 |
//! | [`BlendFailure::StationDidNotConverge`]| [`ErrorKind::StationDidNotConverge`] (125)    |
//! | [`BlendFailure::TrimLoopOpen`], [`BlendFailure::TrimLoopDoesNotSeparate`] | [`ErrorKind::TrimLeaks`] (126) |
//! | [`BlendFailure::StageFailed`]          | [`ErrorKind::StageFailed`] (127)              |
//! | the phrase causes ([`BlendFailure::Failed`], [`BlendFailure::Missing`], ...) | their status's kind, as before the record; neither defect nor refusal |
//!
//! **Statuses.** Each failure also records, for difftest and the goldens only, the status
//! the C++ reports for it (crate-private, read through [`crate::SolidError::sm_status`] and
//! `upstream_status`); it never decides the type. The status a cause gives unless its
//! site says otherwise:
//!
//! | Rust                                   | Status difftest compares          |
//! |----------------------------------------|-----------------------------------|
//! | [`BlendFailure::NoRoom`], [`BlendFailure::StationDidNotConverge`], [`BlendFailure::TrimLoopOpen`], [`BlendFailure::TrimLoopDoesNotSeparate`], [`BlendFailure::StageFailed`] | `SM_ERR` |
//! | [`BlendFailure::Failed`]               | `SM_ERR`                          |
//! | [`BlendFailure::Missing`]              | `SM_ERR_NULL_POINTER`             |
//! | [`BlendFailure::InvalidValue`]         | `SM_ERR_INVALID_INPUT`            |
//! | [`BlendFailure::Inconsistent`]         | `SM_ERR_ASSERT_FAILURE`           |
//! | [`BlendFailure::BadIntersections`]     | `SM_ERR_BAD_INTERSECTIONS`        |
//! | [`BlendFailure::NotConverged`]         | `SM_ERR_NOT_CONVERGING`           |
//! | [`BlendFailure::OutsideDomain`]        | `SM_ERR_OUTSIDE_OF_DOMAIN`        |
//! | [`BlendFailure::NotWithinTolerance`]   | `SM_ERR_NOT_WITHIN_TOLERANCE`     |
//! | [`BlendFailure::PassedOn`]             | the status of the stage passed on |
//!
//! **The phrase causes** ([`BlendFailure::Failed`] to [`BlendFailure::PassedOn`]) are the
//! shape the blend sites were converted to before the record, some of them refusals not yet
//! moved onto it. Each keeps the kind of its status, as before the record, and reads as
//! neither a defect nor a refusal ([`BlendFailure::is_phrase`]); its phrase stays in its
//! message. They go class by class: a site that has a finer cause moves to it, and one that
//! has none to [`BlendFailure::StageFailed`], built on purpose. A test counts the library
//! sites that build a phrase cause, per file, and lets the count only fall.
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. No cause
//! names an entity of its own: a failure is at the entity the site it replaces named
//! (`SolidError::blend_at`), until the site moves onto the typed [`super::Site`]. The
//! failures that are checks of a caller's arguments stay
//! [`crate::SolidError::InvalidInput`]. Every cause stands in for the status its site
//! returned ([`BlendFailure::replaces_upstream_status`]), so the decisions that read an
//! upstream status read these alike. Only part of `crate::blend` uses these yet; the files
//! that still build bare statuses are listed, with their counts, in this module's ratchet
//! test.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_BAD_INTERSECTIONS,
    SM_ERR_INVALID_INPUT, SM_ERR_NOT_CONVERGING, SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER,
    SM_ERR_OUTSIDE_OF_DOMAIN,
};
use crate::blend::fillet_executive::NoRoomReason;

/// Why a blend fails: the cause of the typed blend failure record. Its class is
/// [`BlendFailure::class`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum BlendFailure {
    /// A blend larger than its faces allow, for the reason named (classes 101 to 124, 128
    /// and 129; [`no_room_class`]). A refusal: the failure's fit, where the
    /// check computed one, is the size that builds.
    NoRoom(NoRoomReason),
    /// The solve of the ball's position at a station of the blend did not converge: a defect,
    /// never "no root" (a solve that stops proves nothing about the root).
    StationDidNotConverge {
        /// The station: the blend's parameter along its spine.
        s: f64,
        /// The residual the solve stopped at.
        residual: f64,
        /// The tolerance it had to reach.
        tolerance: f64,
        /// The last step it took.
        step: f64,
        /// The iterations it took.
        iterations: u32,
    },
    /// The boundary loop of the faces the rails enclose is open: the failure's site vertex
    /// is where. A defect ([`ErrorKind::TrimLeaks`]).
    TrimLoopOpen,
    /// The collection of the faces the rails enclose reached past a rail: the failure's site
    /// face is the face it reached. A defect ([`ErrorKind::TrimLeaks`]).
    TrimLoopDoesNotSeparate,
    /// A stage of the blend failed with no reason the kernel names: a defect. The failure
    /// holds the operation, the status (for the goldens only) and where in solidcore's source
    /// it was built ([`Failure::origin`]).
    StageFailed,
    /// The routine could not continue, as the phrase says (for example "the fillet's law
    /// has no value at the base"). Upstream `return SE(SM_ERR)`. A phrase cause
    /// ([`BlendFailure::is_phrase`]).
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the edge
    /// use's edge"). Upstream `NER(ptr)` on a `NULL` pointer, or a `NULL` it would
    /// dereference. A phrase cause ([`BlendFailure::is_phrase`]).
    Missing(&'static str),
    /// A value the routine is given or computes with is not usable, as the phrase says
    /// (for example "a G^n section's order"). Upstream returns `SM_ERR_INVALID_INPUT` as a
    /// status here, so its kind is [`ErrorKind::InvalidInput`]. A phrase cause
    /// ([`BlendFailure::is_phrase`]); an argument check is [`crate::SolidError::InvalidInput`].
    InvalidValue(&'static str),
    /// A consistency check of the routine's own data did not hold, as the phrase says
    /// (upstream `AERN`, `SM_ERR_ASSERT_FAILURE`). A phrase cause
    /// ([`BlendFailure::is_phrase`]).
    Inconsistent(&'static str),
    /// Intersections the routine needs are not usable, as the phrase says (for example "the
    /// two surfaces meet in more than two points"); upstream `SM_ERR_BAD_INTERSECTIONS`. A
    /// phrase cause ([`BlendFailure::is_phrase`]).
    BadIntersections(&'static str),
    /// An iteration did not converge, as the phrase says; upstream
    /// `SM_ERR_NOT_CONVERGING`. A phrase cause ([`BlendFailure::is_phrase`]).
    NotConverged(&'static str),
    /// A parameter lies outside its domain, as the phrase says; upstream
    /// `SM_ERR_OUTSIDE_OF_DOMAIN`. A phrase cause ([`BlendFailure::is_phrase`]).
    OutsideDomain(&'static str),
    /// A result is farther than the tolerance from where it must be, as the phrase says;
    /// upstream `SM_ERR_NOT_WITHIN_TOLERANCE`. A phrase cause
    /// ([`BlendFailure::is_phrase`]).
    NotWithinTolerance(&'static str),
    /// A stage the routine calls failed, as the phrase says, and the routine answers with
    /// that stage's `status` (upstream `SER` of the stage's status, or a status chosen at
    /// run time). The stage's error is not kept as a cause: it may be a solidcore-side
    /// failure, which the bare status it replaces did not carry. A phrase cause
    /// ([`BlendFailure::is_phrase`]).
    PassedOn {
        /// What failed.
        what: &'static str,
        /// The status the routine answers with (a comparison detail).
        status: i32,
    },
}

/// The class of a blend larger than its faces allow, for `reason`: one of the blend's
/// refusal classes 101 to 124 (the cliff overflow's own refusals grouped by what decides
/// them), [`ErrorKind::TangentContinuationEnd`] (128) or [`ErrorKind::OverflowKeepsNothing`]
/// (129).
/// [`ErrorKind::CliffSubtractionLeavesNoSolid`] is the one defect among them.
#[must_use]
pub const fn no_room_class(reason: NoRoomReason) -> ErrorKind {
    use NoRoomReason as R;
    match reason {
        R::BallDoesNotFit => ErrorKind::BallDoesNotFit,
        R::RailLeavesFace | R::ConcaveOverflow => ErrorKind::RailLeavesFace,
        R::FaceConsumed => ErrorKind::FaceConsumed,
        R::BandsOverlap => ErrorKind::BandsOverlap,
        R::BallsFold => ErrorKind::BallsFold,
        R::OffsetFolds => ErrorKind::ChamferOffsetFolds,
        R::CutsThePart => ErrorKind::CutsThePart,
        R::BreaksThrough => ErrorKind::BreaksThrough,
        R::OffsetCollapses => ErrorKind::BallOffsetCollapses,
        R::CornersTakeWholeEdge => ErrorKind::CornersTakeWholeEdge,
        R::RoundIntoMaterial | R::RailRunsPastFaceIntoMaterial => ErrorKind::RunsIntoMaterial,
        R::OverflowWithConcaveBlend | R::OverflowWithRollover | R::OverflowWithGlobalMerge => {
            ErrorKind::CliffOverflowRefused
        }
        R::CliffEndFaceNotPlanar => ErrorKind::CliffEndFaceNotPlanar,
        R::CliffLawNotHeld
        | R::CliffEndNotCut
        | R::CliffBoundaryOnTwoFaces
        | R::CliffBoundaryBranches
        | R::CliffBoundaryClosed
        | R::CliffBoundaryLeavesAlongNoSideEdge
        | R::CliffClosingFaceUnbounded => ErrorKind::CliffOverflowShape,
        R::CliffSubtractionLeavesNoSolid => ErrorKind::CliffSubtractionLeavesNoSolid,
        R::CreaseAcrossTangentEdge => ErrorKind::CreaseAcrossTangentEdge,
        R::RailBelowResolution => ErrorKind::RailBelowResolution,
        R::RailPastAxis => ErrorKind::RailPastAxis,
        R::JoinOffsetsDiffer => ErrorKind::JoinOffsetsDiffer,
        R::StripBelowResolution => ErrorKind::StripBelowResolution,
        R::EndPastSideEdge => ErrorKind::EndPastSideEdge,
        R::EndOverflowsSideEdge => ErrorKind::EndOverflowsSideEdge,
        R::EndRollsIntoSmoothEnd => ErrorKind::EndRollsIntoSmoothEnd,
        R::EndAtSeamVertex => ErrorKind::EndAtSeamVertex,
        R::TangentContinuationEndUnsupported => ErrorKind::TangentContinuationEnd,
        R::OverflowKeepsNothing => ErrorKind::OverflowKeepsNothing,
    }
}

impl BlendFailure {
    /// The status the C++ reports for the failure unless its site says otherwise (a
    /// comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::NoRoom(_)
            | Self::StationDidNotConverge { .. }
            | Self::TrimLoopOpen
            | Self::TrimLoopDoesNotSeparate
            | Self::StageFailed
            | Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) => SM_ERR_INVALID_INPUT,
            Self::Inconsistent(_) => SM_ERR_ASSERT_FAILURE,
            Self::BadIntersections(_) => SM_ERR_BAD_INTERSECTIONS,
            Self::NotConverged(_) => SM_ERR_NOT_CONVERGING,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::NotWithinTolerance(_) => SM_ERR_NOT_WITHIN_TOLERANCE,
            Self::PassedOn { status, .. } => *status,
        }
    }

    /// The failure's class, its stable [`ErrorKind`], derived from the cause: the reason's
    /// class for [`BlendFailure::NoRoom`] ([`no_room_class`]),
    /// [`ErrorKind::StationDidNotConverge`], [`ErrorKind::TrimLeaks`] for an open or leaking
    /// trim loop, and [`ErrorKind::StageFailed`] for [`BlendFailure::StageFailed`]. A phrase
    /// cause keeps the kind of its status ([`ErrorKind::Failed`], [`ErrorKind::Missing`],
    /// [`ErrorKind::InvalidInput`], ...), as before the record ([`BlendFailure::is_phrase`]).
    #[must_use]
    pub const fn class(&self) -> ErrorKind {
        match self {
            Self::NoRoom(reason) => no_room_class(*reason),
            Self::StationDidNotConverge { .. } => ErrorKind::StationDidNotConverge,
            Self::TrimLoopOpen | Self::TrimLoopDoesNotSeparate => ErrorKind::TrimLeaks,
            Self::StageFailed => ErrorKind::StageFailed,
            Self::Failed(_)
            | Self::Missing(_)
            | Self::InvalidValue(_)
            | Self::Inconsistent(_)
            | Self::BadIntersections(_)
            | Self::NotConverged(_)
            | Self::OutsideDomain(_)
            | Self::NotWithinTolerance(_)
            | Self::PassedOn { .. } => ErrorKind::of_status(self.comparison_status()),
        }
    }

    /// True for a phrase cause ([`BlendFailure::Failed`] to [`BlendFailure::PassedOn`]): a
    /// site not yet converted to the record. Its class is its status's kind, as before the
    /// record, and it reads as neither a defect nor a refusal
    /// ([`crate::SolidError::is_defect`], [`crate::SolidError::is_refusal`]) until it moves to
    /// a typed cause.
    #[must_use]
    pub const fn is_phrase(&self) -> bool {
        match self {
            Self::NoRoom(_)
            | Self::StationDidNotConverge { .. }
            | Self::TrimLoopOpen
            | Self::TrimLoopDoesNotSeparate
            | Self::StageFailed => false,
            Self::Failed(_)
            | Self::Missing(_)
            | Self::InvalidValue(_)
            | Self::Inconsistent(_)
            | Self::BadIntersections(_)
            | Self::NotConverged(_)
            | Self::OutsideDomain(_)
            | Self::NotWithinTolerance(_)
            | Self::PassedOn { .. } => true,
        }
    }

    /// True when the failure stands in for a bare status its site returned (an
    /// [`crate::SolidError::Upstream`] before solidcore described it): the decisions that
    /// read an upstream status ([`crate::error::upstream_status`]) treat it as that status,
    /// and the goldens read it. Every cause does: the status stays reachable for the goldens
    /// while the cause, not the status, decides the type.
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        match self {
            Self::NoRoom(_)
            | Self::StationDidNotConverge { .. }
            | Self::TrimLoopOpen
            | Self::TrimLoopDoesNotSeparate
            | Self::StageFailed
            | Self::Failed(_)
            | Self::Missing(_)
            | Self::InvalidValue(_)
            | Self::Inconsistent(_)
            | Self::BadIntersections(_)
            | Self::NotConverged(_)
            | Self::OutsideDomain(_)
            | Self::NotWithinTolerance(_)
            | Self::PassedOn { .. } => true,
        }
    }
}

impl core::fmt::Display for BlendFailure {
    /// A typed cause is worded from its class and its numbers; a phrase cause by its phrase.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoRoom(reason) => write!(f, "{}: {reason:?}", self.class()),
            Self::StationDidNotConverge {
                s,
                residual,
                tolerance,
                step,
                iterations,
            } => write!(
                f,
                "{}: the station at s = {s} did not converge: residual {residual} > {tolerance} after {iterations} iterations (last step {step})",
                self.class()
            ),
            Self::TrimLoopOpen => write!(f, "{}: the trim loop is open", self.class()),
            Self::TrimLoopDoesNotSeparate => {
                write!(f, "{}: the trim loop does not separate", self.class())
            }
            Self::StageFailed => write!(f, "{}", self.class()),
            Self::Failed(what)
            | Self::InvalidValue(what)
            | Self::BadIntersections(what)
            | Self::NotConverged(what)
            | Self::OutsideDomain(what)
            | Self::NotWithinTolerance(what)
            | Self::PassedOn { what, .. } => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a blend routine that fails with `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend(operation: &'static str, failure: BlendFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a blend routine that fails with `failure` about
    /// `entity` (where the bare status it replaces named one).
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_at(
        operation: &'static str,
        failure: BlendFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a blend routine that could not continue, as the
    /// phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`): a phrase
    /// cause.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_failed(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a blend routine needs that is not there,
    /// as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a
    /// `NULL` pointer): a phrase cause.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_missing(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a value a blend routine is given or computes with
    /// that is not usable, as the phrase says, with status [`SM_ERR_INVALID_INPUT`]
    /// (upstream `return(SM_ERR_INVALID_INPUT)`): a phrase cause.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_invalid_value(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::InvalidValue(what))
    }

    /// The blend failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Blend`].
    #[must_use]
    pub fn blend_failure(&self) -> Option<&BlendFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Blend(b) => Some(b),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SolidError;

    #[test]
    fn blend_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (BlendFailure::Failed("x"), SM_ERR),
            (BlendFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (BlendFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (BlendFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
            (
                BlendFailure::BadIntersections("x"),
                SM_ERR_BAD_INTERSECTIONS,
            ),
            (BlendFailure::NotConverged("x"), SM_ERR_NOT_CONVERGING),
            (BlendFailure::OutsideDomain("x"), SM_ERR_OUTSIDE_OF_DOMAIN),
            (
                BlendFailure::NotWithinTolerance("x"),
                SM_ERR_NOT_WITHIN_TOLERANCE,
            ),
            (
                BlendFailure::PassedOn {
                    what: "x",
                    status: SM_ERR_NOT_CONVERGING,
                },
                SM_ERR_NOT_CONVERGING,
            ),
            (
                BlendFailure::PassedOn {
                    what: "x",
                    status: SM_ERR_NULL_POINTER,
                },
                SM_ERR_NULL_POINTER,
            ),
        ];
        let edge = EntityRef::Edge(crate::topology::EdgeId::from_index(4));
        for (failure, code) in cases {
            for entity in [None, Some(edge)] {
                let typed = match entity {
                    Some(e) => SolidError::blend_at("blend::op", failure, e),
                    None => SolidError::blend("blend::op", failure),
                };
                let before = SolidError::Upstream {
                    operation: "blend::op",
                    code,
                    entity,
                };
                assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
                assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
                assert_eq!(super::super::upstream_status(&typed), Some(code));
                // a phrase cause keeps its status's kind, and is neither a defect nor a refusal
                // until its site is converted
                assert_eq!(typed.kind(), before.kind(), "{failure:?}");
                assert!(failure.is_phrase(), "{failure:?}");
                assert!(!typed.is_defect() && !typed.is_refusal() && !typed.sized());
                assert_eq!(typed.operation(), before.operation(), "{failure:?}");
                assert_eq!(typed.entity(), before.entity(), "{failure:?}");
                assert_eq!(typed.is_port_failure(), before.is_port_failure());
                assert_eq!(typed.is_status_answer(), before.is_status_answer());
                assert_eq!(
                    typed.is_unported_or_cancelled(),
                    before.is_unported_or_cancelled()
                );
                assert!(failure.replaces_upstream_status());
                assert_eq!(typed.blend_failure(), Some(&failure));
            }
        }
        assert_eq!(
            SolidError::blend_missing("blend::op", "the edge use's edge").to_string(),
            "blend::op: the edge use's edge is missing"
        );
        assert_eq!(
            SolidError::blend_failed("blend::op", "the law has no value").to_string(),
            "blend::op: the law has no value"
        );
        assert_eq!(
            SolidError::blend_at(
                "blend::op",
                BlendFailure::Failed("the result is not a manifold solid"),
                edge
            )
            .to_string(),
            "blend::op: the result is not a manifold solid (edge 4)"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::Inconsistent("two loops")).to_string(),
            "blend::op: a consistency check failed: two loops"
        );
        assert_eq!(
            SolidError::blend_invalid_value("blend::op", "x").kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(SolidError::sm_err("blend::op").blend_failure(), None);
    }

    fn target_edge(i: u32) -> crate::error::TargetEdge {
        use crate::blend::fillet_classification::FilletBody;
        use crate::blend::fillet_executive::session::FilletObject;
        let object = FilletObject::Topology(
            FilletBody::Target,
            EntityRef::Edge(crate::topology::EdgeId::from_index(i)),
        );
        crate::error::TargetEdge::of_fillet_object(&object).unwrap()
    }

    #[test]
    fn only_topology_of_the_target_body_is_a_site() {
        use crate::blend::fillet_classification::FilletBody;
        use crate::blend::fillet_executive::session::FilletObject;
        use crate::error::{TargetEdge, TargetFace, TargetVertex};
        let e = EntityRef::Edge(crate::topology::EdgeId::from_index(3));
        let f = EntityRef::Face(crate::topology::FaceId::from_index(5));
        let v = EntityRef::Vertex(crate::topology::VertexId::from_index(7));
        let t = |entity| FilletObject::Topology(FilletBody::Target, entity);
        assert_eq!(target_edge(3).id().index(), 3);
        assert_eq!(TargetFace::of_fillet_object(&t(f)).unwrap().id().index(), 5);
        assert_eq!(
            TargetVertex::of_fillet_object(&t(v)).unwrap().id().index(),
            7
        );
        assert_eq!(TargetEdge::of_fillet_object(&t(f)), None);
        for body in [FilletBody::Pseudo, FilletBody::SecondBase] {
            assert_eq!(
                TargetEdge::of_fillet_object(&FilletObject::Topology(body, e)),
                None
            );
        }
        assert_eq!(TargetEdge::of_fillet_object(&FilletObject::Executive), None);
    }

    #[test]
    fn the_class_is_derived_from_the_cause() {
        use crate::blend::fillet_executive::NoRoomReason as R;
        let cases = [
            (
                BlendFailure::NoRoom(R::BallDoesNotFit),
                ErrorKind::BallDoesNotFit,
            ),
            (
                BlendFailure::NoRoom(R::RailLeavesFace),
                ErrorKind::RailLeavesFace,
            ),
            (
                BlendFailure::NoRoom(R::ConcaveOverflow),
                ErrorKind::RailLeavesFace,
            ),
            (
                BlendFailure::NoRoom(R::FaceConsumed),
                ErrorKind::FaceConsumed,
            ),
            (
                BlendFailure::NoRoom(R::BandsOverlap),
                ErrorKind::BandsOverlap,
            ),
            (BlendFailure::NoRoom(R::BallsFold), ErrorKind::BallsFold),
            (
                BlendFailure::NoRoom(R::OffsetFolds),
                ErrorKind::ChamferOffsetFolds,
            ),
            (BlendFailure::NoRoom(R::CutsThePart), ErrorKind::CutsThePart),
            (
                BlendFailure::NoRoom(R::BreaksThrough),
                ErrorKind::BreaksThrough,
            ),
            (
                BlendFailure::NoRoom(R::OffsetCollapses),
                ErrorKind::BallOffsetCollapses,
            ),
            (
                BlendFailure::NoRoom(R::CornersTakeWholeEdge),
                ErrorKind::CornersTakeWholeEdge,
            ),
            (
                BlendFailure::NoRoom(R::RoundIntoMaterial),
                ErrorKind::RunsIntoMaterial,
            ),
            (
                BlendFailure::NoRoom(R::RailRunsPastFaceIntoMaterial),
                ErrorKind::RunsIntoMaterial,
            ),
            (
                BlendFailure::NoRoom(R::OverflowWithConcaveBlend),
                ErrorKind::CliffOverflowRefused,
            ),
            (
                BlendFailure::NoRoom(R::OverflowWithRollover),
                ErrorKind::CliffOverflowRefused,
            ),
            (
                BlendFailure::NoRoom(R::OverflowWithGlobalMerge),
                ErrorKind::CliffOverflowRefused,
            ),
            (
                BlendFailure::NoRoom(R::CliffEndFaceNotPlanar),
                ErrorKind::CliffEndFaceNotPlanar,
            ),
            (
                BlendFailure::NoRoom(R::CliffLawNotHeld),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffEndNotCut),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffBoundaryOnTwoFaces),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffBoundaryBranches),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffBoundaryClosed),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffBoundaryLeavesAlongNoSideEdge),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffClosingFaceUnbounded),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffSubtractionLeavesNoSolid),
                ErrorKind::CliffSubtractionLeavesNoSolid,
            ),
            (
                BlendFailure::NoRoom(R::CreaseAcrossTangentEdge),
                ErrorKind::CreaseAcrossTangentEdge,
            ),
            (
                BlendFailure::NoRoom(R::RailBelowResolution),
                ErrorKind::RailBelowResolution,
            ),
            (
                BlendFailure::NoRoom(R::RailPastAxis),
                ErrorKind::RailPastAxis,
            ),
            (
                BlendFailure::NoRoom(R::JoinOffsetsDiffer),
                ErrorKind::JoinOffsetsDiffer,
            ),
            (
                BlendFailure::NoRoom(R::StripBelowResolution),
                ErrorKind::StripBelowResolution,
            ),
            (
                BlendFailure::NoRoom(R::EndPastSideEdge),
                ErrorKind::EndPastSideEdge,
            ),
            (
                BlendFailure::NoRoom(R::EndOverflowsSideEdge),
                ErrorKind::EndOverflowsSideEdge,
            ),
            (
                BlendFailure::NoRoom(R::EndRollsIntoSmoothEnd),
                ErrorKind::EndRollsIntoSmoothEnd,
            ),
            (
                BlendFailure::NoRoom(R::EndAtSeamVertex),
                ErrorKind::EndAtSeamVertex,
            ),
            (
                BlendFailure::NoRoom(R::TangentContinuationEndUnsupported),
                ErrorKind::TangentContinuationEnd,
            ),
            (
                BlendFailure::NoRoom(R::OverflowKeepsNothing),
                ErrorKind::OverflowKeepsNothing,
            ),
            (
                BlendFailure::StationDidNotConverge {
                    s: 0.42,
                    residual: 3e-6,
                    tolerance: 1e-9,
                    step: 1e-4,
                    iterations: 50,
                },
                ErrorKind::StationDidNotConverge,
            ),
            (BlendFailure::TrimLoopOpen, ErrorKind::TrimLeaks),
            (BlendFailure::TrimLoopDoesNotSeparate, ErrorKind::TrimLeaks),
            (BlendFailure::StageFailed, ErrorKind::StageFailed),
        ];
        for (cause, class) in cases {
            assert_eq!(cause.class(), class, "{cause:?}");
            let e = SolidError::blend("blend::op", cause);
            assert_eq!(e.kind(), class, "{cause:?}");
            // every cause stands in for its site's status, which the goldens read
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{cause:?}");
            assert_eq!(e.is_defect(), class.is_defect(), "{cause:?}");
            assert_eq!(e.is_refusal(), class.is_refusal(), "{cause:?}");
            assert_eq!(e.sized(), class.sized(), "{cause:?}");
            assert!(!e.is_port_failure(), "{cause:?}");
        }
    }

    #[test]
    fn only_a_stage_failure_built_on_purpose_is_a_defect() {
        // a refusal not yet on the record, as `record_no_room` returns it: a phrase cause
        let refusal = SolidError::blend_failed(
            "blend::fillet_executive::p3",
            "the blend is too large for its faces",
        );
        assert_eq!(refusal.kind(), ErrorKind::Failed);
        assert!(!refusal.is_defect() && !refusal.is_refusal() && !refusal.sized());
        let missing = SolidError::blend_missing("blend::op", "the edge use's edge");
        assert_eq!(missing.kind(), ErrorKind::Missing);
        assert!(!missing.is_defect() && !missing.is_refusal());
        // a phrase cause passing on an offset status keeps that status's kind but is still not
        // classed
        let passed = SolidError::blend(
            "blend::op",
            BlendFailure::PassedOn {
                what: "x",
                status: crate::error::SM_ERR_OFFSET_WALLS_TOUCH,
            },
        );
        assert_eq!(passed.kind(), ErrorKind::OffsetWallsTouch);
        assert!(!passed.is_defect() && !passed.is_refusal());
        // a stage failure built on purpose is class 127, a defect
        let stage = SolidError::blend("blend::op", BlendFailure::StageFailed);
        assert_eq!(stage.kind(), ErrorKind::StageFailed);
        assert_eq!(stage.kind().code(), 127);
        assert!(stage.is_defect() && !stage.is_refusal() && !stage.sized());
    }

    #[test]
    fn typed_causes_are_worded_from_their_numbers() {
        use crate::blend::fillet_executive::NoRoomReason as R;
        let station = SolidError::blend(
            "blend::op",
            BlendFailure::StationDidNotConverge {
                s: 0.42,
                residual: 3e-6,
                tolerance: 1e-9,
                step: 0.0001,
                iterations: 50,
            },
        );
        assert_eq!(
            station.to_string(),
            "blend::op: station_did_not_converge: the station at s = 0.42 did not converge: residual 0.000003 > 0.000000001 after 50 iterations (last step 0.0001)"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::NoRoom(R::OverflowWithRollover))
                .to_string(),
            "blend::op: cliff_overflow_refused: OverflowWithRollover"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::TrimLoopOpen).to_string(),
            "blend::op: trim_leaks: the trim loop is open"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::StageFailed).to_string(),
            "blend::op: stage_failed"
        );
        // the site and the fit, when known, follow the cause
        let sited = Failure::new("blend::op", BlendFailure::NoRoom(R::BallDoesNotFit))
            .or_at(crate::error::Site::edge(target_edge(4)))
            .with_fit(crate::error::Fit::new(
                2.5,
                crate::error::FitMeasure::Radius,
                Some(target_edge(4)),
            ))
            .into_error();
        assert_eq!(
            sited.to_string(),
            "blend::op: ball_does_not_fit: BallDoesNotFit at edge 4; the radius that builds is 2.5"
        );
        assert_eq!(sited.fit().map(|f| f.size), Some(2.5));
        assert!(sited.sized());
    }

    #[test]
    fn a_stage_failure_keeps_its_operation_status_and_origin_but_shows_neither() {
        let before = line!();
        let f =
            Failure::new("blend::op", BlendFailure::StageFailed).with_status(SM_ERR_NULL_POINTER);
        let after = line!();
        assert!((before..after).contains(&f.origin().line()));
        assert!(
            f.origin().file().ends_with("error/blend.rs"),
            "{}",
            f.origin().file()
        );
        let e = f.into_error();
        assert_eq!(e.operation(), "blend::op");
        assert_eq!(e.sm_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.kind(), ErrorKind::StageFailed);
        assert!(e.is_defect());
        let shown = e.to_string();
        assert!(!shown.contains("1006") && !shown.contains(".rs"), "{shown}");
        // where a failure was built is not part of what failed
        let other = Failure::new("blend::op", BlendFailure::StageFailed)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        assert_eq!(e, other);
        // the helpers record their caller, not themselves
        let before = line!();
        let helper = SolidError::blend_failed("blend::op", "x");
        let after = line!();
        let at = helper.failure().unwrap().origin().line();
        assert!((before..after).contains(&at), "{at}");
    }

    #[test]
    fn the_stage_error_is_the_source_and_the_classes_follow_the_cause_chain() {
        use crate::blend::fillet_executive::NoRoomReason as R;
        use std::error::Error as _;
        let refusal = SolidError::blend("blend::stage", BlendFailure::NoRoom(R::BandsOverlap));
        let collapsed = Failure::new("blend::op", BlendFailure::Failed("the stage failed"))
            .caused_by(refusal.clone())
            .into_error();
        assert_eq!(
            collapsed.source().map(ToString::to_string),
            Some(refusal.to_string())
        );
        assert!(refusal.source().is_none());
        // a phrase collapse over a refusal reads as the refusal, not also as a defect
        assert!(!collapsed.is_defect() && collapsed.is_refusal() && collapsed.sized());
        assert!(!refusal.is_defect() && refusal.is_refusal());
        // so does a stage failure built on purpose over a refusal: the deepest class decides
        let stage = Failure::new("blend::op", BlendFailure::StageFailed)
            .caused_by(refusal.clone())
            .into_error();
        assert!(!stage.is_defect() && stage.is_refusal() && stage.sized());
        // and a refusal over a stage failure reads as the defect
        let defect = SolidError::blend("blend::stage", BlendFailure::StageFailed);
        assert!(defect.is_defect() && !defect.is_refusal());
        let over = Failure::new("blend::op", BlendFailure::NoRoom(R::BallDoesNotFit))
            .caused_by(defect)
            .into_error();
        assert!(over.is_defect() && !over.is_refusal() && !over.sized());
        // the message still shows the chain, as before
        assert_eq!(
            collapsed.to_string(),
            "blend::op: the stage failed; caused by blend::stage: bands_overlap: BandsOverlap"
        );
    }

    #[test]
    fn or_at_fills_empty_slots_and_never_changes_an_outcome() {
        use crate::error::Site;
        let edge = EntityRef::Edge(crate::topology::EdgeId::from_index(9));
        let point = crate::math::Point3::new(1.0, 2.0, 3.0);
        let at_edge = Site::edge(target_edge(1));
        let e = SolidError::blend("blend::op", BlendFailure::StageFailed)
            .or_at(at_edge)
            .or_at(Site::edge(target_edge(2)).or(Site::point(point)));
        let site = e.site().copied().unwrap();
        assert_eq!(site.edge, Some(target_edge(1)), "a set slot is kept");
        assert_eq!(site.at, Some(point), "an empty slot is filled");
        assert_eq!(
            e.to_string(),
            "blend::op: stage_failed at edge 1, (1, 2, 3)"
        );
        // errors without a record are returned as they are
        assert_eq!(
            SolidError::sm_err("blend::op").or_at(at_edge),
            SolidError::sm_err("blend::op")
        );
        // the decisions that read a failure's entity read the untyped entity, which `or_at`
        // never writes
        let open_base = |entity| {
            let failure = BlendFailure::Failed("a kept feature would leave the base open");
            match entity {
                Some(at) => SolidError::blend_at("blend::feature_executive", failure, at),
                None => SolidError::blend("blend::feature_executive", failure),
            }
        };
        for entity in [None, Some(edge)] {
            let e = open_base(entity);
            let filled = e.clone().or_at(at_edge.or(Site::point(point)));
            assert_eq!(filled.entity(), e.entity());
            assert_eq!(filled.upstream_status(), e.upstream_status());
            assert_eq!(filled.kind(), e.kind());
            assert_eq!(
                crate::blend::feature_executive::is_open_base_refusal(&filled),
                crate::blend::feature_executive::is_open_base_refusal(&e)
            );
        }
        assert!(crate::blend::feature_executive::is_open_base_refusal(
            &open_base(Some(edge))
        ));
        assert!(!crate::blend::feature_executive::is_open_base_refusal(
            &open_base(None)
        ));
    }

    /// The blend routines describe their failures: their library code builds no bare
    /// `Upstream` status, except in the files listed, each with the exact number of bare
    /// sites it still holds. A part of the typed-errors work that converts a file edits
    /// only that file's line: lowers its count, or removes the line once the file holds
    /// none.
    #[test]
    fn the_blend_routines_build_no_bare_statuses() {
        const WAITING: &[(&str, usize)] = &[("blend/fillet_section/recertify.rs", 14)];
        let found = super::super::query::tests::bare_statuses_in("blend", 100);
        let waiting: Vec<(String, usize)> = WAITING
            .iter()
            .map(|(file, n)| ((*file).to_owned(), *n))
            .collect();
        assert_eq!(found, waiting, "bare statuses per blend file");
        for (file, n) in WAITING {
            assert!(*n > 0, "{file} holds no bare status: remove its line");
        }
        assert!(
            WAITING.windows(2).all(|w| w[0].0 < w[1].0),
            "WAITING is sorted by path, one file per line"
        );
    }

    /// What builds a phrase cause in source: the helpers, and each phrase cause by name. A
    /// new cause is placed here (the match has no wildcard), so the ratchet below counts
    /// every phrase cause there is.
    fn phrase_needle(cause: &BlendFailure) -> Option<&'static str> {
        match cause {
            BlendFailure::NoRoom(_)
            | BlendFailure::StationDidNotConverge { .. }
            | BlendFailure::TrimLoopOpen
            | BlendFailure::TrimLoopDoesNotSeparate
            | BlendFailure::StageFailed => None,
            BlendFailure::Failed(_) => Some("BlendFailure::Failed("),
            BlendFailure::Missing(_) => Some("BlendFailure::Missing("),
            BlendFailure::InvalidValue(_) => Some("BlendFailure::InvalidValue("),
            BlendFailure::Inconsistent(_) => Some("BlendFailure::Inconsistent("),
            BlendFailure::BadIntersections(_) => Some("BlendFailure::BadIntersections("),
            BlendFailure::NotConverged(_) => Some("BlendFailure::NotConverged("),
            BlendFailure::OutsideDomain(_) => Some("BlendFailure::OutsideDomain("),
            BlendFailure::NotWithinTolerance(_) => Some("BlendFailure::NotWithinTolerance("),
            BlendFailure::PassedOn { .. } => Some("BlendFailure::PassedOn {"),
        }
    }

    /// The blend sites not yet converted to a typed cause: the library code of the blend
    /// routines builds exactly this many phrase causes (read by the scanner the bare-status
    /// ratchet uses). A phrase cause is neither a defect nor a refusal, so this is the set of
    /// blend failures nobody has classed yet, and it may only shrink: a change that converts
    /// sites lowers the pin to the new count, and a new site builds a typed cause.
    #[test]
    fn the_phrase_cause_sites_only_go_down() {
        const PHRASE_CAUSE_SITES: usize = 1406;
        use crate::blend::fillet_executive::NoRoomReason as R;
        let causes = [
            BlendFailure::NoRoom(R::BallDoesNotFit),
            BlendFailure::StationDidNotConverge {
                s: 0.0,
                residual: 0.0,
                tolerance: 0.0,
                step: 0.0,
                iterations: 0,
            },
            BlendFailure::TrimLoopOpen,
            BlendFailure::TrimLoopDoesNotSeparate,
            BlendFailure::StageFailed,
            BlendFailure::Failed("x"),
            BlendFailure::Missing("x"),
            BlendFailure::InvalidValue("x"),
            BlendFailure::Inconsistent("x"),
            BlendFailure::BadIntersections("x"),
            BlendFailure::NotConverged("x"),
            BlendFailure::OutsideDomain("x"),
            BlendFailure::NotWithinTolerance("x"),
            BlendFailure::PassedOn {
                what: "x",
                status: SM_ERR,
            },
        ];
        let mut needles = vec![
            "SolidError::blend_failed(",
            "SolidError::blend_missing(",
            "SolidError::blend_invalid_value(",
        ];
        for cause in &causes {
            let needle = phrase_needle(cause);
            assert_eq!(needle.is_some(), cause.is_phrase(), "{cause:?}");
            needles.extend(needle);
        }
        let per_file: Vec<(String, usize)> = super::super::bare_scan::library_files("blend", 100)
            .into_iter()
            .filter_map(|(relative, text)| {
                let code = super::super::bare_scan::library_code(&text);
                let n: usize = needles.iter().map(|n| code.matches(n).count()).sum();
                (n > 0).then_some((relative, n))
            })
            .collect();
        let found: usize = per_file.iter().map(|(_, n)| n).sum();
        assert_eq!(
            found, PHRASE_CAUSE_SITES,
            "the blend routines build {found} phrase causes, pinned at {PHRASE_CAUSE_SITES}: \
             the number may only go down. Converting sites to a typed cause lowers the pin to \
             the new count; a new site builds a typed cause, not a phrase. Per file: \
             {per_file:?}"
        );
    }
}
