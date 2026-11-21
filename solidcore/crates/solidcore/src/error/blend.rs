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
//! | [`BlendFailure::RemovalRefused`]       | the reason's ([`removal_class`]), 130 or 131  |
//! | [`BlendFailure::StationDidNotConverge`]| [`ErrorKind::StationDidNotConverge`] (125)    |
//! | [`BlendFailure::TrimLoopOpen`], [`BlendFailure::TrimLoopDoesNotSeparate`] | [`ErrorKind::TrimLeaks`] (126) |
//! | [`BlendFailure::StageFailed`], [`BlendFailure::Defect`], [`BlendFailure::SkinMeasureNotFinite`], [`BlendFailure::PlaneCurveStepVanished`] | [`ErrorKind::StageFailed`] (127) |
//! | [`BlendFailure::SkinPastStationCap`], [`BlendFailure::PlaneCurveFitCap`] | [`ErrorKind::NotWithinTolerance`] (9) |
//! | [`BlendFailure::PlaneCurveTouches`]    | [`ErrorKind::Degenerate`] (3)                |
//! | [`BlendFailure::PlaneCurveEndOff`]     | [`ErrorKind::NotFound`] (2)                  |
//! | [`BlendFailure::PlaneCurveLeavesDomain`] | [`ErrorKind::OutsideDomain`] (8)           |
//! | [`BlendFailure::BallsNest`]            | [`ErrorKind::BallsFold`] (105)                |
//! | [`BlendFailure::CreasePieceEndUnbuildable`] | [`ErrorKind::Unsupported`] (5)           |
//! | [`BlendFailure::NotCertified`]         | [`ErrorKind::NotWithinTolerance`] (9): neither defect nor refusal |
//! | [`BlendFailure::BandInputMissing`]     | [`ErrorKind::Missing`]: neither defect nor refusal |
//! | the phrase causes ([`BlendFailure::Failed`], [`BlendFailure::Missing`], ...) | their status's kind, as before the record; neither defect nor refusal |
//!
//! **Statuses.** Each failure also records, for difftest and the goldens only, the status
//! the C++ reports for it (crate-private, read through [`crate::SolidError::sm_status`] and
//! `upstream_status`); it never decides the type. The status a cause gives unless its
//! site says otherwise:
//!
//! | Rust                                   | Status difftest compares          |
//! |----------------------------------------|-----------------------------------|
//! | [`BlendFailure::NoRoom`], [`BlendFailure::RemovalRefused`], [`BlendFailure::StationDidNotConverge`], [`BlendFailure::TrimLoopOpen`], [`BlendFailure::TrimLoopDoesNotSeparate`], [`BlendFailure::StageFailed`], [`BlendFailure::Defect`], [`BlendFailure::SkinPastStationCap`], [`BlendFailure::SkinMeasureNotFinite`], [`BlendFailure::BallsNest`], [`BlendFailure::CreasePieceEndUnbuildable`], [`BlendFailure::PlaneCurveTouches`], [`BlendFailure::PlaneCurveEndOff`], [`BlendFailure::PlaneCurveLeavesDomain`], [`BlendFailure::PlaneCurveStepVanished`], [`BlendFailure::PlaneCurveFitCap`] | `SM_ERR` |
//! | [`BlendFailure::Failed`]               | `SM_ERR`                          |
//! | [`BlendFailure::Missing`], [`BlendFailure::BandInputMissing`] | `SM_ERR_NULL_POINTER` |
//! | [`BlendFailure::InvalidValue`]        | `SM_ERR_INVALID_INPUT`            |
//! | [`BlendFailure::Inconsistent`]         | `SM_ERR_ASSERT_FAILURE`           |
//! | [`BlendFailure::BadIntersections`]     | `SM_ERR_BAD_INTERSECTIONS`        |
//! | [`BlendFailure::NotConverged`]         | `SM_ERR_NOT_CONVERGING`           |
//! | [`BlendFailure::OutsideDomain`]        | `SM_ERR_OUTSIDE_OF_DOMAIN`        |
//! | [`BlendFailure::NotWithinTolerance`]   | `SM_ERR_NOT_WITHIN_TOLERANCE`     |
//! | [`BlendFailure::PassedOn`]             | the status of the stage passed on |
//! | [`BlendFailure::NotCertified`]         | `SM_ERR`                          |
//!
//! **The phrase causes** ([`BlendFailure::Failed`] to [`BlendFailure::PassedOn`]) are the
//! shape the blend sites were converted to before the record, some of them refusals not yet
//! moved onto it. Each keeps the kind of its status, as before the record, and reads as
//! neither a defect nor a refusal ([`BlendFailure::is_phrase`]); its phrase stays in its
//! message. They go class by class: a site that has a finer cause moves to it, and one that
//! is the kernel's fault to [`BlendFailure::Defect`], with its reason. A test counts the
//! library sites that build a phrase cause, per file, and lets the count only fall.
//! [`BlendFailure::StageFailed`] is the form of a defect not yet reviewed, also counted per
//! file. The conversion guide: `docs/failure-codes-and-reproduction.md` §6.
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
    /// The interior section did not divide the periodic blend face. Status `SM_ERR`.
    ClosedCliffWouldNotSplit {
        /// The blend face in the sheet being trimmed.
        face: crate::topology::FaceId,
    },
    /// A cliff face lacks geometry needed to trim the blend. Status `SM_ERR_NULL_POINTER`.
    CliffInputMissing {
        /// The face that lacks the input.
        face: crate::topology::FaceId,
        /// The missing geometry.
        input: CliffInput,
    },
    /// A fillet removal refuses the face it is asked to remove, for the reason named
    /// (classes 130 and 131; [`removal_class`]). A refusal that no size decides.
    RemovalRefused(RemovalRefusal),
    /// The solve of the ball's position at a station of the blend did not converge: a defect,
    /// never "no root" (a solve that stops proves nothing about the root).
    StationDidNotConverge {
        /// The station: the blend's parameter along its spine.
        s: f64,
        /// The residual the solve stopped at (`None` where the solve does not report it).
        residual: Option<f64>,
        /// The tolerance it had to reach.
        tolerance: f64,
        /// The last step it took (`None` where the solve does not report it).
        step: Option<f64>,
        /// The iterations it took (`None` where the solve does not report them).
        iterations: Option<u32>,
    },
    /// The boundary loop of the faces the rails enclose is open: the failure's site vertex
    /// is where. A defect ([`ErrorKind::TrimLeaks`]).
    TrimLoopOpen,
    /// The collection of the faces the rails enclose reached past a rail: the failure's site
    /// face is the face it reached. A defect ([`ErrorKind::TrimLeaks`]).
    TrimLoopDoesNotSeparate,
    /// A stage of the blend failed with no reason the kernel names: a defect. The failure
    /// holds the operation, the status (for the goldens only) and where in solidcore's source
    /// it was built ([`Failure::origin`]). The form not yet reviewed: new code builds
    /// [`BlendFailure::Defect`], a named cause or a refusal instead.
    StageFailed,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// A variable round's skin of balls' touch circles was still measured farther than
    /// `asked` from its balls when its refinement reached its most rounds or stations
    /// (#15256): the worst distance measured was `reached` (infinite where a span's section
    /// could not be placed on the skin to be measured), with `stations` stations after
    /// `rounds` rounds. Not within the tolerance ([`ErrorKind::NotWithinTolerance`]): no
    /// tolerance is loosened and the skin is not kept.
    SkinPastStationCap {
        /// The tolerance asked: the solver's approximation tolerance.
        asked: f64,
        /// The largest distance measured from the skin to its balls at the cap.
        reached: f64,
        /// The stations the skin had at the cap.
        stations: usize,
        /// The refinement rounds run.
        rounds: usize,
    },
    /// A quantity a variable round's skin check computes (a distance from the skin to its
    /// balls, or the balls' radius rate and centre speed at a section) was still not a finite
    /// number, last at the section at `s` along the trace, when the refinement reached its
    /// most rounds or stations (#15256): a defect ([`ErrorKind::StageFailed`]). A value that
    /// is not finite before the cap splits its span instead.
    SkinMeasureNotFinite {
        /// The section's parameter along the trace.
        s: f64,
    },
    /// At the section at `s` along the trace a variable round's balls change their radius at
    /// least as fast as their centres move (`|r'| = rate >= |c'| = speed`): each ball holds
    /// the next, so no rolling-ball surface exists there (#15256). A refusal
    /// ([`ErrorKind::BallsFold`]).
    BallsNest {
        /// The section's parameter along the trace.
        s: f64,
        /// `|r'|`: how fast the balls' radius changes along the trace.
        rate: f64,
        /// `|c'|`: how fast the balls' centres move along the trace.
        speed: f64,
    },
    /// The crease trim (#17673) has no construction to end a crease piece of curve kind
    /// `kind` exactly on the meet at its start or end (`at_end`), at parameter `t` of the
    /// crease: a curve with no control polygon (an offset, projected, composite, ...
    /// curve). Unsupported
    /// ([`ErrorKind::Unsupported`]): never a piece left ending short of its meet.
    CreasePieceEndUnbuildable {
        /// The piece's curve kind.
        kind: &'static str,
        /// True for the piece's end, false for its start.
        at_end: bool,
        /// The end's parameter on the crease.
        t: f64,
    },
    /// Tracing a surface/plane crossing found a station where the surface touches the
    /// plane: its tangent along the crossing has no length, so the crossing has no direction.
    /// Degenerate ([`ErrorKind::Degenerate`]); status `SM_ERR`.
    PlaneCurveTouches {
        /// The station's surface parameter u.
        u: f64,
        /// The station's surface parameter v.
        v: f64,
    },
    /// An end given to a surface/plane crossing could not be put on the crossing within
    /// tolerance. An empty search ([`ErrorKind::NotFound`]); status `SM_ERR`.
    PlaneCurveEndOff {
        /// True for the crossing's end, false for its start.
        at_end: bool,
        /// The larger of the distance from the given end to the crossing point found and
        /// its distance to the plane; `None` when the drop or projection found no point.
        distance: Option<f64>,
    },
    /// The crossing's trace leaves the surface's parameter domain in a direction it is not
    /// closed in, before reaching its end. Outside the domain ([`ErrorKind::OutsideDomain`]);
    /// status `SM_ERR`.
    PlaneCurveLeavesDomain {
        /// The station's surface parameter u.
        u: f64,
        /// The station's surface parameter v.
        v: f64,
    },
    /// A surface/plane trace's step was halved below its minimum at a station without a
    /// step being accepted: the march stopped. A defect ([`ErrorKind::StageFailed`]);
    /// status `SM_ERR`.
    PlaneCurveStepVanished {
        /// The last accepted station's surface parameter u.
        u: f64,
        /// The last accepted station's surface parameter v.
        v: f64,
        /// The step after halving.
        step: f64,
        /// The minimum step allowed.
        minimum: f64,
    },
    /// The crossing's Hermite fit still has spans farther than a tenth of the tolerance
    /// from the exact curve when its station cap or its 20 refinement passes ran out.
    /// Not within tolerance ([`ErrorKind::NotWithinTolerance`]); status `SM_ERR`.
    PlaneCurveFitCap {
        /// The stations held when fitting stopped.
        stations: usize,
        /// The refinement passes completed.
        passes: usize,
    },
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
    /// A blend surface not certified within its tolerance in the work its certificate is
    /// allowed: the G^n section's certificate would halve spans past its budget
    /// ([`crate::blend::fillet_section::section::GN_HALVING_BUDGET`]), or has more stations
    /// than the fork's stop (4096), with spans still over its target (a tenth of the
    /// tolerance). The round is refused (not built), but not as a fact of its geometry: the
    /// budget, not the geometry, decides it, so its class is [`ErrorKind::NotWithinTolerance`],
    /// not one of the refusal classes ([`ErrorKind::is_refusal`] is false), and no size is
    /// named (the budget says nothing about which size would certify). Like a cancellation, no fallback turns it into
    /// another result ([`crate::SolidError::is_unported_or_cancelled`]). Status `SM_ERR`: it
    /// stands for the fork's own stop, the status `SM_ERR` with "the blend could not be
    /// certified within its tolerance", which the port returned as
    /// [`BlendFailure::Failed`] before it was typed.
    NotCertified {
        /// The spans halved when it was refused.
        halvings: usize,
        /// The certificate's stations when it was refused.
        stations: usize,
    },
    /// An input the planar-band contact query reads is not there (a support, an edge use, an
    /// offset, a foot of a drop, ...): the input named. Status `SM_ERR_NULL_POINTER` and the
    /// kind of that status ([`ErrorKind::Missing`]), as the phrase cause it replaces; typed,
    /// so not a phrase cause ([`BlendFailure::is_phrase`]).
    BandInputMissing(BandInput),
}

/// A face input required to trim a blend at a cliff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CliffInput {
    /// The face's surface.
    Surface,
    /// The planar support extending the wall.
    Plane,
    /// The face use facing out of the body.
    OutwardFaceUse,
}

impl CliffInput {
    const fn what(self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::Plane => "plane",
            Self::OutwardFaceUse => "outward face use",
        }
    }
}

/// Which input of the planar-band contact query is missing ([`BlendFailure::BandInputMissing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BandInput {
    /// The extrusion's control point in its stored net.
    ExtrusionControlPoint,
    /// The exact product of the extrusion's translation.
    ExtrusionTranslationProduct,
    /// A solver's edge use.
    EdgeUse,
    /// The face the two bands share.
    SharedFace,
    /// The planar support.
    PlanarSupport,
    /// The support on the other side of the blended edge.
    OtherSupport,
    /// The chamfer's setback.
    ChamferSetback,
    /// The other offset of the chamfer.
    OtherOffset,
    /// The first offset of the chamfer.
    FirstOffset,
    /// The blended edge.
    BlendedEdge,
    /// The blended edge's curve.
    BlendedEdgeCurve,
    /// The selected edge.
    SelectedEdge,
    /// The foot of a drop on the other support.
    FootOnOtherSupport,
    /// An endpoint of the blended edge.
    BlendedEdgeEndpoint,
    /// The closest feet of the two bands.
    ClosestBandFeet,
    /// An endpoint of the line.
    LineEndpoint,
    /// The edge of an edge use.
    EdgeUseEdge,
}

impl BandInput {
    /// What is missing, as a phrase ("the shared face").
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::ExtrusionControlPoint => "the extrusion control point",
            Self::ExtrusionTranslationProduct => "the exact extrusion translation product",
            Self::EdgeUse => "the edge use",
            Self::SharedFace => "the shared face",
            Self::PlanarSupport => "the planar support",
            Self::OtherSupport => "the other support",
            Self::ChamferSetback => "the chamfer setback",
            Self::OtherOffset => "the other offset",
            Self::FirstOffset => "the first offset",
            Self::BlendedEdge => "the blended edge",
            Self::BlendedEdgeCurve => "the blended edge's curve",
            Self::SelectedEdge => "the selected edge",
            Self::FootOnOtherSupport => "the foot on the other support",
            Self::BlendedEdgeEndpoint => "the blended edge's endpoint",
            Self::ClosestBandFeet => "the closest band feet",
            Self::LineEndpoint => "the line endpoint",
            Self::EdgeUseEdge => "the edge use's edge",
        }
    }
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
        // a Join's: the round's ball does not fit its face where its contact line must cross
        // the edge between the rounds' faces
        R::BallDoesNotFit | R::JoinRailMissesSideEdge => ErrorKind::BallDoesNotFit,
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
        // an End the round overflows: past its side edge, or over its end face whole
        R::EndOverflowsSideEdge | R::EndPassesOverEndFace => ErrorKind::EndOverflowsSideEdge,
        R::EndRollsIntoSmoothEnd => ErrorKind::EndRollsIntoSmoothEnd,
        R::EndAtSeamVertex => ErrorKind::EndAtSeamVertex,
        R::TangentContinuationEndUnsupported => ErrorKind::TangentContinuationEnd,
        R::OverflowKeepsNothing => ErrorKind::OverflowKeepsNothing,
    }
}

/// Why a fillet removal (`crate::blend::fillet_removal`, its ring removal included) refuses
/// the face it is asked to remove. Each is a fact of the face and its neighbours, not a
/// defect; the class is [`removal_class`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemovalRefusal {
    /// A face the removal extends would grow past its surface's domain, which only a plane
    /// continues exactly.
    FaceGrowsPastDomain,
    /// A corner of more than three edges keeps its end edge, which would need a fitted
    /// intersection to run on to the new corner.
    CornerNeedsFittedIntersection,
    /// The side edge that would run on to the new corner is not a straight B-spline curve,
    /// so it cannot run on exactly.
    SideEdgeNotStraight,
    /// The new corner is off the line of the side edge that would run on to it.
    CornerOffSideLine,
    /// The tangent surfaces of a ring fillet do not meet in a closed curve near its rails
    /// within their domains: the sharp edge would lie on a continuation of one of them.
    TangentSurfacesDoNotMeet,
    /// A seam's UV trim, run on, does not reach the new vertex on its face's surface.
    SeamTrimMissesNewVertex,
    /// The fillet face does not have four edges.
    NotFourEdges,
    /// Two adjacent edges of the fillet face are both tangent.
    AdjacentEdgesTangent,
    /// The edge opposite the fillet face's tangent edge is not tangent.
    OppositeEdgeNotTangent,
    /// Neither of the fillet face's first two edges is tangent.
    NoTangentEdge,
    /// A tangent or end edge of the fillet face does not have two vertices.
    EdgeWithoutTwoVertices,
    /// An edge of the fillet face does not border exactly two faces.
    EdgeNotBetweenTwoFaces,
    /// A tangent face's surface is not a B-spline surface.
    TangentFaceNotBSpline,
    /// An end face's surface is not a B-spline surface.
    EndFaceNotBSpline,
    /// The face is not a ring fillet.
    NotARing,
    /// The ring fillet has more than two rails.
    MoreThanTwoRails,
    /// The ring fillet does not have two rails between two other faces.
    RailsNotBetweenTwoFaces,
    /// The ring fillet's seam does not join the two rails' vertices.
    SeamDoesNotJoinRails,
    /// A rail's vertex has an edge other than one open seam of its tangent face.
    RailVertexHasOtherEdges,
    /// A tangent face's seam crosses the sharp edge at more than one place.
    SeamCrossesSharpEdgeTwice,
    /// The two tangent faces' seams cross the sharp edge at different places.
    SeamsCrossApart,
    /// A seam has more than two edge uses with their own UV trim curves.
    SeamHasMoreThanTwoTrims,
    /// A seam has no edge use with its own UV trim curve.
    SeamHasNoOwnTrim,
}

/// The class of a fillet removal's refusal, for `reason`: [`ErrorKind::RemovalNotExact`] (130)
/// where the removal would continue a face or an edge past its geometry, and
/// [`ErrorKind::NotARemovableFillet`] (131) where the face and its neighbours are not a fillet
/// the removal recognises. Both are refusals no size decides.
#[must_use]
pub const fn removal_class(reason: RemovalRefusal) -> ErrorKind {
    use RemovalRefusal as R;
    match reason {
        R::FaceGrowsPastDomain
        | R::CornerNeedsFittedIntersection
        | R::SideEdgeNotStraight
        | R::CornerOffSideLine
        | R::TangentSurfacesDoNotMeet
        | R::SeamTrimMissesNewVertex => ErrorKind::RemovalNotExact,
        R::NotFourEdges
        | R::AdjacentEdgesTangent
        | R::OppositeEdgeNotTangent
        | R::NoTangentEdge
        | R::EdgeWithoutTwoVertices
        | R::EdgeNotBetweenTwoFaces
        | R::TangentFaceNotBSpline
        | R::EndFaceNotBSpline
        | R::NotARing
        | R::MoreThanTwoRails
        | R::RailsNotBetweenTwoFaces
        | R::SeamDoesNotJoinRails
        | R::RailVertexHasOtherEdges
        | R::SeamCrossesSharpEdgeTwice
        | R::SeamsCrossApart
        | R::SeamHasMoreThanTwoTrims
        | R::SeamHasNoOwnTrim => ErrorKind::NotARemovableFillet,
    }
}

impl BlendFailure {
    /// The status the C++ reports for the failure unless its site says otherwise (a
    /// comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::NoRoom(_)
            | Self::RemovalRefused(_)
            | Self::StationDidNotConverge { .. }
            | Self::TrimLoopOpen
            | Self::TrimLoopDoesNotSeparate
            | Self::ClosedCliffWouldNotSplit { .. }
            | Self::StageFailed
            | Self::Defect { .. }
            | Self::SkinPastStationCap { .. }
            | Self::SkinMeasureNotFinite { .. }
            | Self::BallsNest { .. }
            | Self::CreasePieceEndUnbuildable { .. }
            | Self::PlaneCurveTouches { .. }
            | Self::PlaneCurveEndOff { .. }
            | Self::PlaneCurveLeavesDomain { .. }
            | Self::PlaneCurveStepVanished { .. }
            | Self::PlaneCurveFitCap { .. }
            | Self::NotCertified { .. }
            | Self::Failed(_) => SM_ERR,
            Self::Missing(_) | Self::BandInputMissing(_) | Self::CliffInputMissing { .. } => {
                SM_ERR_NULL_POINTER
            }
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
    /// class for [`BlendFailure::NoRoom`] ([`no_room_class`]) and
    /// [`BlendFailure::RemovalRefused`] ([`removal_class`]),
    /// [`ErrorKind::StationDidNotConverge`], [`ErrorKind::TrimLeaks`] for an open or leaking
    /// trim loop, [`ErrorKind::StageFailed`] for [`BlendFailure::StageFailed`],
    /// [`BlendFailure::Defect`], [`BlendFailure::SkinMeasureNotFinite`] and
    /// [`BlendFailure::PlaneCurveStepVanished`], [`ErrorKind::NotWithinTolerance`] for
    /// [`BlendFailure::SkinPastStationCap`], [`BlendFailure::PlaneCurveFitCap`] and for a
    /// surface not certified within its tolerance in its work budget
    /// ([`BlendFailure::NotCertified`]), [`ErrorKind::Degenerate`] for
    /// [`BlendFailure::PlaneCurveTouches`], [`ErrorKind::NotFound`] for
    /// [`BlendFailure::PlaneCurveEndOff`], [`ErrorKind::OutsideDomain`] for
    /// [`BlendFailure::PlaneCurveLeavesDomain`], [`ErrorKind::BallsFold`] for
    /// [`BlendFailure::BallsNest`], and
    /// [`ErrorKind::Unsupported`] for [`BlendFailure::CreasePieceEndUnbuildable`]. A phrase
    /// cause keeps the kind of its status ([`ErrorKind::Failed`], [`ErrorKind::Missing`],
    /// [`ErrorKind::InvalidInput`], ...), as before the record ([`BlendFailure::is_phrase`]).
    #[must_use]
    pub const fn class(&self) -> ErrorKind {
        match self {
            Self::NoRoom(reason) => no_room_class(*reason),
            Self::RemovalRefused(reason) => removal_class(*reason),
            Self::StationDidNotConverge { .. } => ErrorKind::StationDidNotConverge,
            Self::TrimLoopOpen | Self::TrimLoopDoesNotSeparate => ErrorKind::TrimLeaks,
            Self::ClosedCliffWouldNotSplit { .. } => ErrorKind::Failed,
            Self::StageFailed
            | Self::Defect { .. }
            | Self::SkinMeasureNotFinite { .. }
            | Self::PlaneCurveStepVanished { .. } => ErrorKind::StageFailed,
            Self::SkinPastStationCap { .. }
            | Self::NotCertified { .. }
            | Self::PlaneCurveFitCap { .. } => ErrorKind::NotWithinTolerance,
            Self::PlaneCurveTouches { .. } => ErrorKind::Degenerate,
            Self::PlaneCurveEndOff { .. } => ErrorKind::NotFound,
            Self::PlaneCurveLeavesDomain { .. } => ErrorKind::OutsideDomain,
            Self::BallsNest { .. } => ErrorKind::BallsFold,
            Self::CreasePieceEndUnbuildable { .. } => ErrorKind::Unsupported,
            Self::BandInputMissing(_) | Self::CliffInputMissing { .. } => ErrorKind::Missing,
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
            | Self::RemovalRefused(_)
            | Self::StationDidNotConverge { .. }
            | Self::TrimLoopOpen
            | Self::TrimLoopDoesNotSeparate
            | Self::ClosedCliffWouldNotSplit { .. }
            | Self::StageFailed
            | Self::Defect { .. }
            | Self::SkinPastStationCap { .. }
            | Self::SkinMeasureNotFinite { .. }
            | Self::BallsNest { .. }
            | Self::CreasePieceEndUnbuildable { .. }
            | Self::PlaneCurveTouches { .. }
            | Self::PlaneCurveEndOff { .. }
            | Self::PlaneCurveLeavesDomain { .. }
            | Self::PlaneCurveStepVanished { .. }
            | Self::PlaneCurveFitCap { .. }
            | Self::BandInputMissing(_)
            | Self::CliffInputMissing { .. }
            | Self::NotCertified { .. } => false,
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
    /// while the cause, not the status, decides the type. [`BlendFailure::NotCertified`]
    /// stands for the fork's `SM_ERR` at its certificate's stop (and, past the work budget
    /// solidcore adds, for the same stop reached sooner).
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        match self {
            Self::NoRoom(_)
            | Self::RemovalRefused(_)
            | Self::StationDidNotConverge { .. }
            | Self::TrimLoopOpen
            | Self::TrimLoopDoesNotSeparate
            | Self::ClosedCliffWouldNotSplit { .. }
            | Self::StageFailed
            | Self::Defect { .. }
            | Self::SkinPastStationCap { .. }
            | Self::SkinMeasureNotFinite { .. }
            | Self::BallsNest { .. }
            | Self::CreasePieceEndUnbuildable { .. }
            | Self::PlaneCurveTouches { .. }
            | Self::PlaneCurveEndOff { .. }
            | Self::PlaneCurveLeavesDomain { .. }
            | Self::PlaneCurveStepVanished { .. }
            | Self::PlaneCurveFitCap { .. }
            | Self::BandInputMissing(_)
            | Self::CliffInputMissing { .. }
            | Self::Failed(_)
            | Self::Missing(_)
            | Self::InvalidValue(_)
            | Self::Inconsistent(_)
            | Self::BadIntersections(_)
            | Self::NotConverged(_)
            | Self::OutsideDomain(_)
            | Self::NotWithinTolerance(_)
            | Self::PassedOn { .. }
            | Self::NotCertified { .. } => true,
        }
    }
}

impl core::fmt::Display for BlendFailure {
    /// A typed cause is worded from its class and its numbers; a phrase cause by its phrase.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoRoom(reason) => write!(f, "{}: {reason:?}", self.class()),
            Self::ClosedCliffWouldNotSplit { face } => write!(
                f,
                "the closed cliff blend face {face:?} does not divide at its interior section"
            ),
            Self::RemovalRefused(reason) => write!(f, "{}: {reason:?}", self.class()),
            Self::StationDidNotConverge {
                s,
                residual,
                tolerance,
                step,
                iterations,
            } => {
                let shown = |x: Option<String>| x.unwrap_or_else(|| "unreported".into());
                write!(
                    f,
                    "{}: the station at s = {s} did not converge: residual {} > {tolerance} after {} iterations (last step {})",
                    self.class(),
                    shown(residual.map(|r| r.to_string())),
                    shown(iterations.map(|n| n.to_string())),
                    shown(step.map(|d| d.to_string())),
                )
            }
            Self::TrimLoopOpen => write!(f, "{}: the trim loop is open", self.class()),
            Self::TrimLoopDoesNotSeparate => {
                write!(f, "{}: the trim loop does not separate", self.class())
            }
            Self::StageFailed => write!(f, "{}", self.class()),
            Self::Defect { why } => write!(f, "{}: {why}", self.class()),
            Self::SkinPastStationCap {
                asked,
                reached,
                stations,
                rounds,
            } => write!(
                f,
                "{}: the round's skin is {reached} from its balls, past {asked}, with {stations} stations after {rounds} rounds",
                self.class()
            ),
            Self::SkinMeasureNotFinite { s } => write!(
                f,
                "{}: the skin check computed a value that is not finite at s = {s}",
                self.class()
            ),
            Self::BallsNest { s, rate, speed } => write!(
                f,
                "{}: at s = {s} the balls' radius changes at {rate}, as fast as their centres move ({speed}): no rolling-ball surface exists there",
                self.class()
            ),
            Self::CreasePieceEndUnbuildable { kind, at_end, t } => write!(
                f,
                "{}: the crease piece's {} at t = {t} cannot be ended on its meet: no construction for a {kind} curve there",
                self.class(),
                if *at_end { "end" } else { "start" },
            ),
            Self::PlaneCurveTouches { u, v } => write!(
                f,
                "{}: the surface touches the plane at (u, v) = ({u}, {v}): the crossing has no direction",
                self.class()
            ),
            Self::PlaneCurveEndOff { at_end, distance } => {
                write!(
                    f,
                    "{}: the crossing's {} is off the surface or the plane",
                    self.class(),
                    if *at_end { "end" } else { "start" },
                )?;
                match distance {
                    Some(distance) => write!(f, " by {distance}"),
                    None => f.write_str(": the drop or projection found no point"),
                }
            }
            Self::PlaneCurveLeavesDomain { u, v } => write!(
                f,
                "{}: the crossing leaves the surface's domain at (u, v) = ({u}, {v}) before its end",
                self.class()
            ),
            Self::PlaneCurveStepVanished { u, v, step, minimum } => write!(
                f,
                "{}: the crossing's step at (u, v) = ({u}, {v}) was halved to {step}, below its minimum {minimum}, without an accepted step",
                self.class()
            ),
            Self::PlaneCurveFitCap { stations, passes } => write!(
                f,
                "{}: the crossing's Hermite fit is still off the exact curve by more than a tenth of the tolerance with {stations} stations after {passes} passes",
                self.class()
            ),
            Self::Failed(what)
            | Self::InvalidValue(what)
            | Self::BadIntersections(what)
            | Self::NotConverged(what)
            | Self::OutsideDomain(what)
            | Self::NotWithinTolerance(what)
            | Self::PassedOn { what, .. } => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::BandInputMissing(input) => write!(f, "{} is missing", input.what()),
            Self::CliffInputMissing { face, input } => write!(f, "cliff face {face:?} is missing its {}", input.what()),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
            Self::NotCertified { halvings, stations } => write!(
                f,
                "the blend could not be certified within its tolerance in the work allowed \
                 ({halvings} spans halved, {stations} stations)"
            ),
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

    /// [`crate::SolidError::Failed`] for an input of the planar-band contact query that is
    /// not there ([`BlendFailure::BandInputMissing`]), with status [`SM_ERR_NULL_POINTER`].
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_band_input_missing(operation: &'static str, input: BandInput) -> Self {
        Self::blend(operation, BlendFailure::BandInputMissing(input))
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
                BlendFailure::NoRoom(R::JoinRailMissesSideEdge),
                ErrorKind::BallDoesNotFit,
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
                BlendFailure::NoRoom(R::EndPassesOverEndFace),
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
                    residual: Some(3e-6),
                    tolerance: 1e-9,
                    step: Some(1e-4),
                    iterations: Some(50),
                },
                ErrorKind::StationDidNotConverge,
            ),
            (BlendFailure::TrimLoopOpen, ErrorKind::TrimLeaks),
            (BlendFailure::TrimLoopDoesNotSeparate, ErrorKind::TrimLeaks),
            (BlendFailure::StageFailed, ErrorKind::StageFailed),
            (BlendFailure::Defect { why: "x" }, ErrorKind::StageFailed),
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
    fn a_fillet_removal_refusal_is_a_refusal_no_size_decides() {
        use RemovalRefusal as R;
        let cases = [
            (R::FaceGrowsPastDomain, ErrorKind::RemovalNotExact),
            (R::CornerNeedsFittedIntersection, ErrorKind::RemovalNotExact),
            (R::SideEdgeNotStraight, ErrorKind::RemovalNotExact),
            (R::CornerOffSideLine, ErrorKind::RemovalNotExact),
            (R::TangentSurfacesDoNotMeet, ErrorKind::RemovalNotExact),
            (R::SeamTrimMissesNewVertex, ErrorKind::RemovalNotExact),
            (R::NotFourEdges, ErrorKind::NotARemovableFillet),
            (R::AdjacentEdgesTangent, ErrorKind::NotARemovableFillet),
            (R::OppositeEdgeNotTangent, ErrorKind::NotARemovableFillet),
            (R::NoTangentEdge, ErrorKind::NotARemovableFillet),
            (R::EdgeWithoutTwoVertices, ErrorKind::NotARemovableFillet),
            (R::EdgeNotBetweenTwoFaces, ErrorKind::NotARemovableFillet),
            (R::TangentFaceNotBSpline, ErrorKind::NotARemovableFillet),
            (R::EndFaceNotBSpline, ErrorKind::NotARemovableFillet),
            (R::NotARing, ErrorKind::NotARemovableFillet),
            (R::MoreThanTwoRails, ErrorKind::NotARemovableFillet),
            (R::RailsNotBetweenTwoFaces, ErrorKind::NotARemovableFillet),
            (R::SeamDoesNotJoinRails, ErrorKind::NotARemovableFillet),
            (R::RailVertexHasOtherEdges, ErrorKind::NotARemovableFillet),
            (R::SeamCrossesSharpEdgeTwice, ErrorKind::NotARemovableFillet),
            (R::SeamsCrossApart, ErrorKind::NotARemovableFillet),
            (R::SeamHasMoreThanTwoTrims, ErrorKind::NotARemovableFillet),
            (R::SeamHasNoOwnTrim, ErrorKind::NotARemovableFillet),
        ];
        for (reason, class) in cases {
            let cause = BlendFailure::RemovalRefused(reason);
            assert_eq!(cause.class(), class, "{reason:?}");
            assert!(!cause.is_phrase() && cause.replaces_upstream_status());
            let e = SolidError::blend("blend::fillet_removal", cause);
            assert_eq!(e.kind(), class, "{reason:?}");
            // the status the C++ reports stays, for the goldens
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{reason:?}");
            assert!(e.is_refusal() && !e.is_defect() && !e.sized(), "{reason:?}");
        }
        assert_eq!(
            SolidError::blend(
                "blend::fillet_removal",
                BlendFailure::RemovalRefused(R::FaceGrowsPastDomain)
            )
            .to_string(),
            "blend::fillet_removal: removal_not_exact: FaceGrowsPastDomain"
        );
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

    /// A reviewed defect is of class 127 with the stage failure's status, and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why =
            "the station solve was started off its own previous station, which it never leaves";
        let defect = BlendFailure::Defect { why };
        assert!(!defect.is_phrase() && defect.replaces_upstream_status());
        for status in [SM_ERR, SM_ERR_NULL_POINTER] {
            let e = Failure::new("blend::op", defect)
                .with_status(status)
                .into_error();
            super::super::tests::assert_defect(
                &e,
                &Failure::new("blend::op", BlendFailure::StageFailed)
                    .with_status(status)
                    .into_error(),
                "blend::op",
                status,
                &format!("blend::op: stage_failed: {why}"),
            );
            assert_eq!(e.blend_failure(), Some(&defect));
        }
        assert_eq!(
            SolidError::blend("blend::op", defect).sm_status(),
            Some(SM_ERR)
        );
    }

    #[test]
    fn typed_causes_are_worded_from_their_numbers() {
        use crate::blend::fillet_executive::NoRoomReason as R;
        let station = SolidError::blend(
            "blend::op",
            BlendFailure::StationDidNotConverge {
                s: 0.42,
                residual: Some(3e-6),
                tolerance: 1e-9,
                step: Some(0.0001),
                iterations: Some(50),
            },
        );
        assert_eq!(
            station.to_string(),
            "blend::op: station_did_not_converge: the station at s = 0.42 did not converge: residual 0.000003 > 0.000000001 after 50 iterations (last step 0.0001)"
        );
        // a solve that reports only whether it converged (#15256's skin check)
        let unreported = SolidError::blend(
            "blend::op",
            BlendFailure::StationDidNotConverge {
                s: 0.5,
                residual: None,
                tolerance: 1e-9,
                step: None,
                iterations: None,
            },
        );
        assert_eq!(
            unreported.to_string(),
            "blend::op: station_did_not_converge: the station at s = 0.5 did not converge: residual unreported > 0.000000001 after unreported iterations (last step unreported)"
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

    fn assert_plane_curve_cause(cause: BlendFailure, class: ErrorKind, message: &str) {
        assert_eq!(cause.class(), class);
        assert!(!cause.is_phrase());
        assert!(cause.replaces_upstream_status());
        assert_eq!(variant_index_and_phrase_name(&cause).1, None);
        assert_eq!(cause.comparison_status(), SM_ERR);
        let e = SolidError::blend("blend::op", cause);
        assert_eq!(e.kind(), class);
        assert_eq!(e.operation(), "blend::op");
        assert_eq!(e.blend_failure(), Some(&cause));
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.is_defect(), class.is_defect());
        assert!(!e.is_refusal() && !e.sized());
        assert_eq!(e.to_string(), format!("blend::op: {class}: {message}"));
    }

    #[test]
    fn a_plane_curve_touch_names_its_station() {
        assert_plane_curve_cause(
            BlendFailure::PlaneCurveTouches { u: 0.25, v: 0.5 },
            ErrorKind::Degenerate,
            "the surface touches the plane at (u, v) = (0.25, 0.5): the crossing has no direction",
        );
    }

    #[test]
    fn a_plane_curve_end_off_names_the_end_and_distance_when_found() {
        for (at_end, end) in [(false, "start"), (true, "end")] {
            for (distance, detail) in [
                (Some(0.125), " by 0.125"),
                (None, ": the drop or projection found no point"),
            ] {
                assert_plane_curve_cause(
                    BlendFailure::PlaneCurveEndOff { at_end, distance },
                    ErrorKind::NotFound,
                    &format!("the crossing's {end} is off the surface or the plane{detail}"),
                );
            }
        }
    }

    #[test]
    fn a_plane_curve_leaving_the_domain_names_its_station() {
        assert_plane_curve_cause(
            BlendFailure::PlaneCurveLeavesDomain { u: -0.25, v: 0.5 },
            ErrorKind::OutsideDomain,
            "the crossing leaves the surface's domain at (u, v) = (-0.25, 0.5) before its end",
        );
    }

    #[test]
    fn a_plane_curve_vanished_step_is_a_defect_with_its_numbers() {
        assert_plane_curve_cause(
            BlendFailure::PlaneCurveStepVanished { u: 0.25, v: 0.5, step: 0.0000000005, minimum: 0.000000001 },
            ErrorKind::StageFailed,
            "the crossing's step at (u, v) = (0.25, 0.5) was halved to 0.0000000005, below its minimum 0.000000001, without an accepted step",
        );
    }

    #[test]
    fn a_plane_curve_fit_cap_names_its_stations_and_passes() {
        assert_plane_curve_cause(
            BlendFailure::PlaneCurveFitCap { stations: 128, passes: 20 },
            ErrorKind::NotWithinTolerance,
            "the crossing's Hermite fit is still off the exact curve by more than a tenth of the tolerance with 128 stations after 20 passes",
        );
    }

    #[test]
    fn a_stage_failure_keeps_its_operation_status_and_origin_but_shows_neither() {
        let before = line!();
        let f =
            Failure::new("blend::op", BlendFailure::StageFailed).with_status(SM_ERR_NULL_POINTER);
        let after = line!();
        assert!((before..after).contains(&f.origin().line()));
        // the path is written with the host's separator (`\` on Windows), so compare its
        // components, not its text
        assert!(
            std::path::Path::new(f.origin().file()).ends_with("error/blend.rs"),
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

    /// The number of `BlendFailure` variants. Adding a variant means adding an arm to
    /// `variant_index_and_phrase_name` (the compiler asks for it), with the next index, and
    /// raising this number; the test below then fails until `causes` holds the new variant.
    const VARIANTS: usize = 29;

    /// Each variant's distinct index in `0..VARIANTS`, and the variant name of a phrase cause
    /// (`None` for any other cause). The match has no wildcard, so it is the inventory of
    /// every variant there is.
    fn variant_index_and_phrase_name(cause: &BlendFailure) -> (usize, Option<&'static str>) {
        match cause {
            BlendFailure::NoRoom(_) => (0, None),
            BlendFailure::StationDidNotConverge { .. } => (1, None),
            BlendFailure::TrimLoopOpen => (2, None),
            BlendFailure::TrimLoopDoesNotSeparate => (3, None),
            BlendFailure::RemovalRefused(_) => (4, None),
            BlendFailure::StageFailed => (5, None),
            BlendFailure::Defect { .. } => (6, None),
            BlendFailure::SkinPastStationCap { .. } => (7, None),
            BlendFailure::SkinMeasureNotFinite { .. } => (8, None),
            BlendFailure::BallsNest { .. } => (9, None),
            BlendFailure::CreasePieceEndUnbuildable { .. } => (10, None),
            BlendFailure::BandInputMissing(_) => (11, None),
            BlendFailure::NotCertified { .. } => (12, None),
            BlendFailure::Failed(_) => (13, Some("Failed")),
            BlendFailure::Missing(_) => (14, Some("Missing")),
            BlendFailure::InvalidValue(_) => (15, Some("InvalidValue")),
            BlendFailure::Inconsistent(_) => (16, Some("Inconsistent")),
            BlendFailure::BadIntersections(_) => (17, Some("BadIntersections")),
            BlendFailure::NotConverged(_) => (18, Some("NotConverged")),
            BlendFailure::OutsideDomain(_) => (19, Some("OutsideDomain")),
            BlendFailure::NotWithinTolerance(_) => (20, Some("NotWithinTolerance")),
            BlendFailure::PassedOn { .. } => (21, Some("PassedOn")),
            BlendFailure::PlaneCurveTouches { .. } => (22, None),
            BlendFailure::PlaneCurveEndOff { .. } => (23, None),
            BlendFailure::PlaneCurveLeavesDomain { .. } => (24, None),
            BlendFailure::PlaneCurveStepVanished { .. } => (25, None),
            BlendFailure::PlaneCurveFitCap { .. } => (26, None),
            BlendFailure::ClosedCliffWouldNotSplit { .. } => (27, None),
            BlendFailure::CliffInputMissing { .. } => (28, None),
        }
    }

    /// `causes` holds every variant once, `variant_index_and_phrase_name` names a cause exactly when it is a phrase cause, and the
    /// `BlendFailure` variants the per-file scan (`phrase_sites::BLEND`) counts are exactly
    /// those names, so the per-file pins in `error/phrase_sites.rs` see every phrase cause.
    #[test]
    fn the_phrase_scan_lists_exactly_the_phrase_variants() {
        use super::super::site_scan::Builder;
        use crate::blend::fillet_executive::NoRoomReason as R;
        let scanned: Vec<&str> = super::super::phrase_sites::BLEND
            .builders
            .iter()
            .filter_map(|b| match b {
                Builder::Variant("BlendFailure", v) => Some(*v),
                _ => None,
            })
            .collect();
        let causes = [
            BlendFailure::NoRoom(R::BallDoesNotFit),
            BlendFailure::StationDidNotConverge {
                s: 0.0,
                residual: Some(0.0),
                tolerance: 0.0,
                step: Some(0.0),
                iterations: Some(0),
            },
            BlendFailure::SkinPastStationCap {
                asked: 0.0,
                reached: 0.0,
                stations: 0,
                rounds: 0,
            },
            BlendFailure::SkinMeasureNotFinite { s: 0.0 },
            BlendFailure::BallsNest {
                s: 0.0,
                rate: 0.0,
                speed: 0.0,
            },
            BlendFailure::CreasePieceEndUnbuildable {
                kind: "offset",
                at_end: true,
                t: 0.0,
            },
            BlendFailure::PlaneCurveTouches { u: 0.0, v: 0.0 },
            BlendFailure::PlaneCurveEndOff {
                at_end: false,
                distance: None,
            },
            BlendFailure::PlaneCurveLeavesDomain { u: 0.0, v: 0.0 },
            BlendFailure::PlaneCurveStepVanished {
                u: 0.0,
                v: 0.0,
                step: 0.0,
                minimum: 0.0,
            },
            BlendFailure::PlaneCurveFitCap {
                stations: 0,
                passes: 0,
            },
            BlendFailure::BandInputMissing(BandInput::SharedFace),
            BlendFailure::TrimLoopOpen,
            BlendFailure::TrimLoopDoesNotSeparate,
            BlendFailure::RemovalRefused(RemovalRefusal::FaceGrowsPastDomain),
            BlendFailure::ClosedCliffWouldNotSplit {
                face: crate::topology::FaceId::from_index(0),
            },
            BlendFailure::CliffInputMissing {
                face: crate::topology::FaceId::from_index(0),
                input: CliffInput::Plane,
            },
            BlendFailure::StageFailed,
            BlendFailure::Defect { why: "x" },
            BlendFailure::NotCertified {
                halvings: 0,
                stations: 0,
            },
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
        let mut names = Vec::new();
        let mut indices = Vec::new();
        for cause in &causes {
            let (index, name) = variant_index_and_phrase_name(cause);
            indices.push(index);
            assert_eq!(name.is_some(), cause.is_phrase(), "{cause:?}");
            names.extend(name);
        }
        indices.sort_unstable();
        assert_eq!(
            indices,
            (0..VARIANTS).collect::<Vec<_>>(),
            "`causes` must hold each BlendFailure variant exactly once"
        );
        names.sort_unstable();
        let mut scanned = scanned;
        scanned.sort_unstable();
        assert_eq!(
            scanned, names,
            "phrase_sites::BLEND must list exactly the phrase variants of BlendFailure"
        );
    }
}
