//! Why a Boolean operation fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the Boolean subsystem (`crate::boolean`).
//! Each also records, for difftest only, the status the C++ reports for it (crate-private
//! `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                | Status difftest compares            |
//! |-------------------------------------|-------------------------------------|
//! | [`BooleanFailure::Defect`]          | `SM_ERR` (or the site's, through `with_status`) |
//! | [`BooleanFailure::RelateConflict`]  | `SM_ERR`                            |
//! | [`BooleanFailure::SolidLeaks`]      | `SM_ERR` (WR-619, solidcore-native) |
//! | the named causes, from [`BooleanFailure::FaceWithoutSurface`] on | the status of the site each replaced (`comparison_status`) |
//!
//! [`BooleanFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with
//! its reason: a defect ([`ErrorKind::StageFailed`], class 127). A check of the caller's
//! argument or body (a face without surface, an edge without curve, a tree operator
//! without operands) is a named cause of class `invalid_input`; it stays a
//! [`crate::SolidError::Failed`] with its status, so the intersector's handling of
//! ignored statuses (which passes [`crate::SolidError::InvalidInput`] on) is unchanged.
//!
//! Deviations: not upstream. A defect names the stage ([`BooleanStage`]) and says why it
//! is the kernel's fault; the handles the stages work on are the
//! intersector's own, not ids of the caller's B-rep, so only [`BooleanFailure::SolidLeaks`]
//! names an entity; [`BooleanFailure::RelateConflict`] places its three by their points
//! (#16098). The failures that are checks of a caller's handles stay
//! [`crate::SolidError::InvalidInput`].

use super::{EntityRef, ErrorKind, SM_ERR, SM_ERR_NULL_POINTER};
use crate::math::Point3;
use crate::topology::FaceId;

/// The stage of a Boolean operation a failure comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BooleanStage {
    /// Classifying the operands' faces, edges and vertices into the result.
    Classify,
    /// Closing an intersection loop at a vertex.
    CloseLoop,
    /// Resolving surfaces, edges or faces that coincide.
    Coincidence,
    /// Imprinting an intersection point or curve on a face.
    Imprint,
    /// Intersecting the faces of the operands and imprinting the curves on them.
    Intersect,
    /// Merging the kept topology of the other operand into the primary.
    Merge,
    /// Merging the curve classifications of an intersection into the operands.
    MergeClassifications,
    /// Recording which entity of one operand matches which of the other.
    Relate,
    /// Splitting edges where they meet.
    Split,
    /// Evaluating a Boolean tree or a merge of several B-reps.
    Tree,
}

impl core::fmt::Display for BooleanStage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Classify => "classifying the operands into the result",
            Self::CloseLoop => "closing an intersection loop",
            Self::Coincidence => "resolving coincident geometry",
            Self::Imprint => "imprinting on a face",
            Self::Intersect => "intersecting the operands' faces",
            Self::Merge => "merging the other operand into the primary",
            Self::MergeClassifications => "merging the curve classifications",
            Self::Relate => "relating the operands' entities",
            Self::Split => "splitting edges",
            Self::Tree => "evaluating the Boolean tree",
        })
    }
}

/// Which operand of the intersector an entity of a [`BooleanFailure::RelateConflict`] is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelateSide {
    /// The B-rep the intersector relates from (`Relate`'s first object).
    Brep,
    /// The other B-rep (`Relate`'s second object).
    Other,
}

impl core::fmt::Display for RelateSide {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Brep => "the B-rep",
            Self::Other => "the other B-rep",
        })
    }
}

/// The step of merging two curve classifications that failed, for a
/// [`BooleanFailure::ClassificationStepFailed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClassificationStep {
    /// Removing the short intervals that may be coincident and validating the rest.
    Cleanup,
    /// Fixing up the end points of the B-rep's classification (edge to vertex, short
    /// intervals merged).
    FixupFirst,
    /// Making each point classified to a related entity classify to its mate on the
    /// other side.
    FixupMates,
    /// Fixing up the end points of the other B-rep's classification.
    FixupSecond,
    /// Making the two classifications consistent, interval for interval.
    Homogenize,
}

impl core::fmt::Display for ClassificationStep {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Cleanup => "removing the short intervals and validating the rest",
            Self::FixupFirst => "fixing up the end points of the B-rep's classification",
            Self::FixupMates => "making the points classified to related entities agree",
            Self::FixupSecond => "fixing up the end points of the other B-rep's classification",
            Self::Homogenize => "making the two classifications consistent",
        })
    }
}

/// An entity of an operand, placed by its points for a [`BooleanFailure::RelateConflict`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlacedEntity {
    /// An edge from its start vertex's point to its end vertex's.
    Edge(Point3, Point3),
    /// An entity of another kind, or one whose points could not be read.
    Unplaced,
    /// A vertex at the point.
    Vertex(Point3),
}

impl core::fmt::Display for PlacedEntity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let p = |p: &Point3| format!("({}, {}, {})", p.x, p.y, p.z);
        match self {
            Self::Edge(a, b) => write!(f, "the edge from {} to {}", p(a), p(b)),
            Self::Unplaced => f.write_str("an entity"),
            Self::Vertex(at) => write!(f, "the vertex at {}", p(at)),
        }
    }
}

/// Why a Boolean operation fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum BooleanFailure {
    /// Growing a vertex definition or a proto vertex to cover the curves it absorbed would
    /// reach another vertex definition its tolerance kept clear of: vertices of the operands
    /// closer than their tolerances (#18312). Class `failed`.
    AbsorbedCoverReachesVertex {
        /// The tolerance that covers what it absorbed.
        needed: f64,
    },
    /// A step of merging two curve classifications failed; the step's error is the cause.
    ClassificationStepFailed {
        /// The step.
        step: ClassificationStep,
    },
    /// The new edge made where an edge lies on a face could not be glued to that edge; the
    /// glue's error is the cause.
    CoincidenceUnresolved,
    /// Gluing the ends of two coincident edges deleted one of the edges.
    CoincidentEdgeCollapsed,
    /// The ends of two coincident edges are each related to a different vertex already.
    CoincidentEdgeEndMatesConflict,
    /// The ends of two coincident edges do not pair up: neither assignment of one edge's
    /// vertices to the other's lies within their tolerances, or both do.
    CoincidentEdgeEndsDiffer,
    /// Merging the curves of two coincident faces into each other still changes the
    /// topology after the port's bound on the passes (upstream repeats without bound).
    CoincidentFacesNotSettled {
        /// The bound on the passes.
        passes: usize,
    },
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step of `stage` that failed and why that is the kernel's fault, not
    /// the geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its
    /// site says otherwise.
    Defect {
        /// The stage.
        stage: BooleanStage,
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// A degenerate intersection curve was given with an edge instead of a pair of
    /// surfaces, a case the merge does not take.
    DegenerateCurveOnEdges,
    /// The edges split off while two edge lists are intersected exceed the port's bound
    /// on each working edge list (upstream grows them without bound).
    EdgeListLimit {
        /// The bound on the edges of each list.
        limit: usize,
    },
    /// The split of an edge at the parameter the intersector picked made no new vertex.
    /// The intersector picks the parameter after its own interior test, and the split
    /// refuses it under a different one (the parameter's zero test, then the length to
    /// the nearer end against the vertex's tolerance), so the two can disagree on valid
    /// input; the parameter is never the caller's (class `failed`).
    EdgeNotSplit,
    /// Two edges an intersection interval lies along could not be related, and neither
    /// could be deleted as an edge between two faces of one surface; the relation's
    /// refusal is the cause.
    EdgeRelationNotRepaired,
    /// An edge of an operand has no curve (the caller's body; class `invalid_input`).
    EdgeWithoutCurve,
    /// The Boolean of the surface and B-rep lists left nothing: its result has no shell.
    EmptyResult,
    /// The vertex is already related to a vertex that lies closer than the one asked for,
    /// so the squeeze keeps the existing relation and refuses the new one.
    ExistingRelationBetter,
    /// A face use of the other B-rep lies in no region of the primary: the search over the
    /// primary's regions found none.
    FaceInNoRegion,
    /// The faces split off while two face lists are intersected exceed the port's bound
    /// on the working face lists (upstream grows them without bound).
    FaceListLimit {
        /// The bound on the faces of both lists.
        limit: usize,
    },
    /// A face of an operand has no surface (the caller's body; class `invalid_input`).
    FaceWithoutSurface,
    /// A registered intersection curve is not a B-spline curve or a conic, so it has no
    /// B-spline copy for the intersection table (an unsupported curve kind; the curve is
    /// the intersector's own handle, not an entity of the caller's B-rep).
    IntersectionCurveNotCopyable,
    /// A vertex an intersection loop ends at, taken to close the loop or a gap, has no
    /// related vertex in the other B-rep (an earlier closing in the pass, whose failure
    /// the merge ignores, can have removed its relation).
    LoopVertexUnrelated,
    /// The merge was asked to run without its primary or its other B-rep (class
    /// `invalid_input`).
    MergeWithoutOperands,
    /// A vertex the imprint makes one from exactly placed points within their summed
    /// tolerances needs tolerance `needed` to cover the gaps that construction leaves (twice
    /// the largest certified gap to a curve end or plane at it), and that ball would reach a
    /// vertex it does not bound (the far vertex of one of its edges). A merged vertex fits
    /// only when its tolerance plus that vertex's is less than their distance: a tolerance
    /// below `fits_below` (the distance less the other vertex's tolerance, the smallest over
    /// the vertices it would reach), so its points must lie less than half of it apart.
    /// `None` when no tolerance fits (a vertex it would reach is closer than its own
    /// tolerance).
    MergedVertexReaches {
        /// The tolerance the merged vertex needs.
        needed: f64,
        /// The tolerance that fits: anything below it reaches no vertex it does not bound.
        fits_below: Option<f64>,
    },
    /// The intersection curve lies along two partly coincident edges that have no common
    /// sub-interval.
    NoCommonSubinterval,
    /// The radial sector search at a common edge found neither a coincident face use nor a
    /// radial edge use of the other operand; a failed search is the cause where there is
    /// one.
    NoRadialEdgeUse,
    /// The caller listed no surfaces to merge (class `invalid_input`).
    NoSurfacesToMerge,
    /// The two entities whose relation is to be removed are not related to each other.
    NotRelatedPair,
    /// An operand of the planar Boolean has no faces, so the Boolean made no result (class
    /// `invalid_input`).
    OperandWithoutFaces,
    /// Not upstream (#18927): a step of making the proto edge of an interval of a face
    /// pair's intersection failed, so the intersection lacks that edge and no result built
    /// from it bounds the operands. The failed step's error is the cause. Class `failed`.
    IntervalEdgeNotMade,
    /// Replacing a group of overlapping intersection curves failed after the group's
    /// curves were taken out of the intersection: what is left records neither the group
    /// nor its replacement, so no result built from it bounds the operands' overlap. The
    /// failed step's error is the cause. Class `failed`.
    OverlapGroupNotReplaced,
    /// The intersector was asked to relate `entity`, of `side`, to `asked`, but `entity` is
    /// already related to `mate` (upstream `SmTopologyIntersector::Relate`'s `SM_ERR`; the
    /// mates are one to one). The stage before asked for one entity to have two mates: the
    /// points tell which (two coincident edges of one operand, two pieces of one edge, a
    /// vertex where intersection curves cross).
    RelateConflict {
        /// The operand `entity` is on.
        side: RelateSide,
        /// The entity that has a mate.
        entity: PlacedEntity,
        /// Its mate, on the other operand.
        mate: PlacedEntity,
        /// The entity of the other operand it was asked to be related to.
        asked: PlacedEntity,
    },
    /// An intersection curve from a vertex back to it leaves the reach of its ends (the
    /// vertex definition's tolerance, the proto edge's and the distance to its ends), so it
    /// can be neither split onto the vertex nor dropped (WR-214). Class `failed`.
    SelfLoopLeavesReach {
        /// The reach of the definition's curve that the curve leaves.
        reach: f64,
    },
    /// An intersection curve from a vertex back to it (a self-loop) was absorbed by a vertex
    /// definition, and splitting the loop would move that definition so far that its
    /// tolerance, grown to cover the curve, reaches another vertex definition (or the curve
    /// is at no finite distance from the new place): vertices of the operands closer than
    /// their tolerances (WR-306). Class `failed`.
    SelfLoopSplitReachesVertex {
        /// The tolerance that covers the curve from the new place; `None` when the curve is
        /// at no finite distance from it.
        needed: Option<f64>,
    },
    /// A slice was asked with another operand that is not a sheet (class
    /// `invalid_input`).
    SliceNeedsSheet,
    /// A union deleted `face`, which bounded a solid against the outside, and that solid's
    /// side was left in no solid region: the solid merged into the outside (WR-619).
    SolidLeaks {
        /// The face, of the B-rep the operation edits.
        face: FaceId,
    },
    /// A Boolean of the tree failed; its error is the cause.
    TreeBooleanFailed,
    /// An operator of the Boolean tree has fewer than two operands before it (the caller's
    /// tree; class `invalid_input`).
    TreeOperatorWithoutOperands,
    /// An operator code of the Boolean tree is none of union (-1) to slice (-6) (the
    /// caller's tree; class `invalid_input`).
    UnknownTreeOperator {
        /// The code.
        code: i64,
    },
    /// A shell vertex of one operand classifies to no region of the other: it lies on the
    /// other's boundary (a face, an edge or a vertex) or the classification found nothing.
    VertexInNoRegion,
    /// Neither vertex of a related pair that splits a short interval drops onto the
    /// intersection curve: the distances the drops stopped at.
    VertexPairNotDropped {
        /// The distance of the B-rep's vertex from the curve where its drop stopped.
        brep_gap: f64,
        /// The distance of the other B-rep's vertex from the curve where its drop stopped.
        other_gap: f64,
    },
}

impl BooleanFailure {
    /// The entity the failure names, if any.
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match *self {
            Self::SolidLeaks { face } => Some(EntityRef::Face(face)),
            Self::RelateConflict { .. }
            | Self::MergedVertexReaches { .. }
            | Self::SelfLoopSplitReachesVertex { .. }
            | Self::AbsorbedCoverReachesVertex { .. }
            | Self::SelfLoopLeavesReach { .. }
            | Self::Defect { .. }
            | Self::FaceWithoutSurface
            | Self::EdgeWithoutCurve
            | Self::MergeWithoutOperands
            | Self::TreeOperatorWithoutOperands
            | Self::UnknownTreeOperator { .. }
            | Self::OperandWithoutFaces
            | Self::NoSurfacesToMerge
            | Self::SliceNeedsSheet
            | Self::EdgeNotSplit
            | Self::TreeBooleanFailed
            | Self::FaceInNoRegion
            | Self::VertexInNoRegion
            | Self::NoRadialEdgeUse
            | Self::EmptyResult
            | Self::IntersectionCurveNotCopyable
            | Self::ClassificationStepFailed { .. }
            | Self::ExistingRelationBetter
            | Self::CoincidenceUnresolved
            | Self::VertexPairNotDropped { .. }
            | Self::CoincidentEdgeEndsDiffer
            | Self::CoincidentEdgeEndMatesConflict
            | Self::CoincidentEdgeCollapsed
            | Self::EdgeRelationNotRepaired
            | Self::DegenerateCurveOnEdges
            | Self::NoCommonSubinterval
            | Self::NotRelatedPair
            | Self::LoopVertexUnrelated
            | Self::FaceListLimit { .. }
            | Self::EdgeListLimit { .. }
            | Self::CoincidentFacesNotSettled { .. }
            | Self::IntervalEdgeNotMade
            | Self::OverlapGroupNotReplaced => None,
        }
    }

    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::AbsorbedCoverReachesVertex { .. }
            | Self::ClassificationStepFailed { .. }
            | Self::CoincidenceUnresolved
            | Self::CoincidentEdgeCollapsed
            | Self::CoincidentEdgeEndMatesConflict
            | Self::CoincidentEdgeEndsDiffer
            | Self::CoincidentFacesNotSettled { .. }
            | Self::Defect { .. }
            | Self::DegenerateCurveOnEdges
            | Self::EdgeListLimit { .. }
            | Self::EdgeNotSplit
            | Self::EdgeRelationNotRepaired
            | Self::EmptyResult
            | Self::ExistingRelationBetter
            | Self::FaceInNoRegion
            | Self::FaceListLimit { .. }
            | Self::IntersectionCurveNotCopyable
            | Self::MergedVertexReaches { .. }
            | Self::NoCommonSubinterval
            | Self::NoRadialEdgeUse
            | Self::NoSurfacesToMerge
            | Self::NotRelatedPair
            | Self::IntervalEdgeNotMade
            | Self::OverlapGroupNotReplaced
            | Self::RelateConflict { .. }
            | Self::SelfLoopLeavesReach { .. }
            | Self::SelfLoopSplitReachesVertex { .. }
            | Self::SliceNeedsSheet
            | Self::SolidLeaks { .. }
            | Self::TreeBooleanFailed
            | Self::TreeOperatorWithoutOperands
            | Self::UnknownTreeOperator { .. }
            | Self::VertexInNoRegion
            | Self::VertexPairNotDropped { .. } => SM_ERR,
            Self::EdgeWithoutCurve
            | Self::FaceWithoutSurface
            | Self::LoopVertexUnrelated
            | Self::MergeWithoutOperands
            | Self::OperandWithoutFaces => SM_ERR_NULL_POINTER,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::StageFailed`] for
    /// [`BooleanFailure::Defect`], a defect; [`ErrorKind::InvalidInput`] for a check of the
    /// caller's argument or body; [`ErrorKind::Unsupported`] for a curve kind or case the
    /// Boolean does not take; [`ErrorKind::NotConverged`] for drops that stopped;
    /// [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::AbsorbedCoverReachesVertex { .. }
            | Self::ClassificationStepFailed { .. }
            | Self::CoincidenceUnresolved
            | Self::CoincidentEdgeCollapsed
            | Self::CoincidentEdgeEndMatesConflict
            | Self::CoincidentEdgeEndsDiffer
            | Self::EdgeNotSplit
            | Self::EdgeRelationNotRepaired
            | Self::EmptyResult
            | Self::ExistingRelationBetter
            | Self::FaceInNoRegion
            | Self::LoopVertexUnrelated
            | Self::MergedVertexReaches { .. }
            | Self::NoCommonSubinterval
            | Self::NoRadialEdgeUse
            | Self::NotRelatedPair
            | Self::IntervalEdgeNotMade
            | Self::OverlapGroupNotReplaced
            | Self::RelateConflict { .. }
            | Self::SelfLoopLeavesReach { .. }
            | Self::SelfLoopSplitReachesVertex { .. }
            | Self::SolidLeaks { .. }
            | Self::TreeBooleanFailed
            | Self::VertexInNoRegion => ErrorKind::Failed,
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::DegenerateCurveOnEdges
            | Self::EdgeListLimit { .. }
            | Self::FaceListLimit { .. }
            | Self::IntersectionCurveNotCopyable => ErrorKind::Unsupported,
            Self::EdgeWithoutCurve
            | Self::FaceWithoutSurface
            | Self::MergeWithoutOperands
            | Self::NoSurfacesToMerge
            | Self::OperandWithoutFaces
            | Self::SliceNeedsSheet
            | Self::TreeOperatorWithoutOperands
            | Self::UnknownTreeOperator { .. } => ErrorKind::InvalidInput,
            Self::VertexPairNotDropped { .. } | Self::CoincidentFacesNotSettled { .. } => {
                ErrorKind::NotConverged
            }
        }
    }
}

impl core::fmt::Display for BooleanFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AbsorbedCoverReachesVertex { needed } => write!(
                f,
                "{} failed: covering what a vertex absorbed would grow it to tolerance \
                 {needed}, which reaches another vertex",
                BooleanStage::Relate
            ),
            Self::ClassificationStepFailed { step } => {
                write!(
                    f,
                    "{} failed: {step} failed",
                    BooleanStage::MergeClassifications
                )
            }
            Self::CoincidenceUnresolved => f.write_str(
                "the coincidence of an edge with a face could not be resolved: the new edge \
                 did not glue to it",
            ),
            Self::CoincidentEdgeCollapsed => {
                f.write_str("gluing the ends of the coincident edges deleted one of them")
            }
            Self::CoincidentEdgeEndMatesConflict => f.write_str(
                "the ends of the coincident edges are related to different vertices already",
            ),
            Self::CoincidentEdgeEndsDiffer => {
                f.write_str("the ends of the coincident edges do not pair up")
            }
            Self::CoincidentFacesNotSettled { passes } => write!(
                f,
                "the coincident faces still change after {passes} passes of merging their curves"
            ),
            Self::Defect { stage, why } => {
                write!(f, "{}: {stage} failed: {why}", ErrorKind::StageFailed)
            }
            Self::DegenerateCurveOnEdges => f.write_str(
                "a degenerate intersection curve was given with an edge, not a pair of surfaces",
            ),
            Self::EdgeListLimit { limit } => write!(
                f,
                "the edges split off exceed the bound of {limit} edges on an edge list"
            ),
            Self::EdgeNotSplit => f.write_str(
                "the edge was not split at the parameter the intersector picked: the split \
                 finds it at an end of the edge",
            ),
            Self::EdgeRelationNotRepaired => f.write_str(
                "the edges along the intersection could not be related, and neither could be \
                 deleted",
            ),
            Self::EdgeWithoutCurve => f.write_str("an edge of an operand has no curve"),
            Self::EmptyResult => f.write_str("the Boolean left nothing: the result has no shell"),
            Self::ExistingRelationBetter => f.write_str(
                "the vertex is already related to a closer vertex, which the squeeze keeps",
            ),
            Self::FaceInNoRegion => {
                f.write_str("a face use of the other B-rep lies in no region of the primary")
            }
            Self::FaceListLimit { limit } => write!(
                f,
                "the faces split off exceed the bound of {limit} faces on the face lists"
            ),
            Self::FaceWithoutSurface => f.write_str("a face of an operand has no surface"),
            Self::IntersectionCurveNotCopyable => f.write_str(
                "a registered intersection curve is not a B-spline curve or a conic, so it \
                 cannot be copied",
            ),
            Self::LoopVertexUnrelated => f.write_str(
                "the vertex the intersection loop ends at has no related vertex in the other \
                 B-rep",
            ),
            Self::MergeWithoutOperands => {
                f.write_str("the merge has no primary or no other B-rep to work on")
            }
            Self::MergedVertexReaches {
                needed,
                fits_below: Some(fits),
            } => write!(
                f,
                "{} failed: a merged vertex needs tolerance {needed} to cover its certified \
                 gaps, which reaches a vertex it does not bound; a tolerance below {fits} fits",
                BooleanStage::Imprint
            ),
            Self::MergedVertexReaches {
                needed,
                fits_below: None,
            } => write!(
                f,
                "{} failed: a merged vertex needs tolerance {needed} to cover its certified \
                 gaps, which reaches a vertex it does not bound; no tolerance fits (a vertex \
                 it would reach lies within its own tolerance of it)",
                BooleanStage::Imprint
            ),
            Self::NoCommonSubinterval => {
                f.write_str("the partly coincident edges have no common sub-interval")
            }
            Self::NoRadialEdgeUse => f.write_str(
                "the radial sector search at a common edge found no face use of the other \
                 operand",
            ),
            Self::NoSurfacesToMerge => f.write_str("no surfaces were given to merge"),
            Self::NotRelatedPair => f.write_str("the two entities are not related to each other"),
            Self::OperandWithoutFaces => f.write_str(
                "an operand of the planar Boolean has no faces, so it made no result B-rep",
            ),
            Self::IntervalEdgeNotMade => f.write_str(
                "making the edge of an interval of a face pair's intersection failed",
            ),
            Self::OverlapGroupNotReplaced => f.write_str(
                "replacing a group of overlapping intersection curves failed after the group was taken out of the intersection",
            ),
            Self::RelateConflict {
                side,
                entity,
                mate,
                asked,
            } => write!(
                f,
                "{} failed: {entity} of {side} is related to {mate} and was asked for {asked}: \
                 another entity is already related to the other one",
                BooleanStage::Relate
            ),
            Self::SelfLoopLeavesReach { reach } => write!(
                f,
                "{} failed: an intersection curve from a vertex back to it leaves the reach \
                 of its ends ({reach}), so it can be neither split onto the vertex nor dropped",
                BooleanStage::Relate
            ),
            Self::SelfLoopSplitReachesVertex {
                needed: Some(needed),
            } => write!(
                f,
                "{} failed: a self-loop's split would move a vertex that absorbed a curve so \
                 far that covering the curve (tolerance {needed}) reaches another vertex",
                BooleanStage::Relate
            ),
            Self::SelfLoopSplitReachesVertex { needed: None } => write!(
                f,
                "{} failed: a self-loop's split would move a vertex that absorbed a curve to \
                 a place at no finite distance from the curve",
                BooleanStage::Relate
            ),
            Self::SliceNeedsSheet => {
                f.write_str("a slice needs a sheet, and the other B-rep is not one")
            }
            Self::SolidLeaks { face } => write!(
                f,
                "the union deleted face {} and the solid it bounded merged into the outside",
                face.index()
            ),
            Self::TreeBooleanFailed => f.write_str("a Boolean of the tree failed"),
            Self::TreeOperatorWithoutOperands => {
                f.write_str("an operator of the Boolean tree has fewer than two operands")
            }
            Self::UnknownTreeOperator { code } => write!(
                f,
                "the Boolean tree's operator code {code} is none of union (-1) to slice (-6)"
            ),
            Self::VertexInNoRegion => f.write_str(
                "a shell vertex classifies to no region of the other B-rep: it lies on that \
                 B-rep's boundary",
            ),
            Self::VertexPairNotDropped {
                brep_gap,
                other_gap,
            } => write!(
                f,
                "neither vertex of the related pair drops onto the curve: the drops stopped \
                 {brep_gap} and {other_gap} from it"
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for `failure`, with its status.
    #[track_caller]
    pub(crate) fn boolean(operation: &'static str, failure: BooleanFailure) -> Self {
        super::Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a reviewed [`BooleanFailure::Defect`] of `stage`,
    /// with status [`SM_ERR`].
    #[track_caller]
    pub(crate) fn boolean_defect(
        operation: &'static str,
        stage: BooleanStage,
        why: &'static str,
    ) -> Self {
        Self::boolean(operation, BooleanFailure::Defect { stage, why })
    }

    /// [`crate::SolidError::Failed`] for a reviewed [`BooleanFailure::Defect`] of `stage`
    /// where a thing the kernel built is not there, with status [`SM_ERR_NULL_POINTER`]
    /// (upstream `NER(ptr)` on a `NULL` pointer).
    #[track_caller]
    pub(crate) fn boolean_null_defect(
        operation: &'static str,
        stage: BooleanStage,
        why: &'static str,
    ) -> Self {
        super::Failure::new(operation, BooleanFailure::Defect { stage, why })
            .with_status(SM_ERR_NULL_POINTER)
            .into_error()
    }

    /// [`crate::SolidError::Failed`] for a relation refused because `entity` already has a
    /// different mate, with status [`SM_ERR`] (upstream `Relate`'s `return SE(SM_ERR)`).
    pub(crate) fn boolean_relate_conflict(
        operation: &'static str,
        side: RelateSide,
        entity: PlacedEntity,
        mate: PlacedEntity,
        asked: PlacedEntity,
    ) -> Self {
        super::Failure::new(
            operation,
            BooleanFailure::RelateConflict {
                side,
                entity,
                mate,
                asked,
            },
        )
        .into_error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SolidError;

    /// A reviewed defect is of class 127 with the stage failure's status, and names its
    /// stage and why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why =
            "the loop the imprint just closed has a free end, which its own walk never leaves";
        let defect = BooleanFailure::Defect {
            stage: BooleanStage::CloseLoop,
            why,
        };
        let op = "boolean::op";
        let e = super::super::Failure::new(op, defect).into_error();
        super::super::tests::assert_defect(
            &e,
            &SolidError::boolean_defect(op, BooleanStage::CloseLoop, "x"),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: closing an intersection loop failed: {why}"),
        );
        assert_eq!(e.entity(), None);
        assert!(matches!(
            e.failure().map(|f| &f.reason),
            Some(super::super::Reason::Boolean(b)) if *b == defect
        ));
    }

    /// Each named cause of the merge and the topology intersector (#19856) keeps the
    /// status its site had and has its class; the two defect forms keep theirs.
    #[test]
    fn the_merge_causes_keep_their_statuses_and_classes() {
        use super::super::{ErrorKind as K, SM_ERR_NULL_POINTER as NULL};
        use BooleanFailure as F;
        let step = ClassificationStep::Homogenize;
        let cases = [
            (F::FaceWithoutSurface, NULL, K::InvalidInput),
            (F::EdgeWithoutCurve, NULL, K::InvalidInput),
            (F::MergeWithoutOperands, NULL, K::InvalidInput),
            (F::OperandWithoutFaces, NULL, K::InvalidInput),
            (F::TreeOperatorWithoutOperands, SM_ERR, K::InvalidInput),
            (F::UnknownTreeOperator { code: 9 }, SM_ERR, K::InvalidInput),
            (F::NoSurfacesToMerge, SM_ERR, K::InvalidInput),
            (F::SliceNeedsSheet, SM_ERR, K::InvalidInput),
            (F::EdgeNotSplit, SM_ERR, K::Failed),
            (F::TreeBooleanFailed, SM_ERR, K::Failed),
            (F::FaceInNoRegion, SM_ERR, K::Failed),
            (F::VertexInNoRegion, SM_ERR, K::Failed),
            (F::NoRadialEdgeUse, SM_ERR, K::Failed),
            (F::EmptyResult, SM_ERR, K::Failed),
            (F::ClassificationStepFailed { step }, SM_ERR, K::Failed),
            (F::ExistingRelationBetter, SM_ERR, K::Failed),
            (F::CoincidenceUnresolved, SM_ERR, K::Failed),
            (F::CoincidentEdgeEndsDiffer, SM_ERR, K::Failed),
            (F::CoincidentEdgeEndMatesConflict, SM_ERR, K::Failed),
            (F::CoincidentEdgeCollapsed, SM_ERR, K::Failed),
            (F::EdgeRelationNotRepaired, SM_ERR, K::Failed),
            (F::NoCommonSubinterval, SM_ERR, K::Failed),
            (F::NotRelatedPair, SM_ERR, K::Failed),
            (F::IntervalEdgeNotMade, SM_ERR, K::Failed),
            (F::OverlapGroupNotReplaced, SM_ERR, K::Failed),
            (
                F::SelfLoopSplitReachesVertex { needed: Some(0.5) },
                SM_ERR,
                K::Failed,
            ),
            (
                F::SelfLoopSplitReachesVertex { needed: None },
                SM_ERR,
                K::Failed,
            ),
            (
                F::AbsorbedCoverReachesVertex { needed: 0.5 },
                SM_ERR,
                K::Failed,
            ),
            (F::SelfLoopLeavesReach { reach: 0.5 }, SM_ERR, K::Failed),
            (F::LoopVertexUnrelated, NULL, K::Failed),
            (F::IntersectionCurveNotCopyable, SM_ERR, K::Unsupported),
            (F::DegenerateCurveOnEdges, SM_ERR, K::Unsupported),
            (F::FaceListLimit { limit: 4 }, SM_ERR, K::Unsupported),
            (F::EdgeListLimit { limit: 4 }, SM_ERR, K::Unsupported),
            (
                F::VertexPairNotDropped {
                    brep_gap: 0.5,
                    other_gap: 0.25,
                },
                SM_ERR,
                K::NotConverged,
            ),
            (
                F::CoincidentFacesNotSettled { passes: 4 },
                SM_ERR,
                K::NotConverged,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::boolean("boolean::op", failure);
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(failure.kind(), kind, "{failure:?}");
            assert!(!e.is_defect(), "{failure:?}");
            assert_eq!(e.to_string(), format!("boolean::op: {failure}"));
        }
        // the numbers a cause carries are in its words
        let e = SolidError::boolean(
            "boolean::op",
            F::VertexPairNotDropped {
                brep_gap: 0.5,
                other_gap: 0.25,
            },
        );
        assert!(e.to_string().contains("0.5 and 0.25"), "{e}");
        assert!(F::ClassificationStepFailed { step }
            .to_string()
            .contains(&step.to_string()));
        // the two defect forms: the stage failure's status, and the null pointer's
        let why = "a link the kernel keeps is gone";
        for (e, status) in [
            (
                SolidError::boolean_defect("boolean::op", BooleanStage::Merge, why),
                SM_ERR,
            ),
            (
                SolidError::boolean_null_defect("boolean::op", BooleanStage::Merge, why),
                NULL,
            ),
        ] {
            assert!(e.is_defect(), "{e:?}");
            assert_eq!(e.sm_status(), Some(status), "{e:?}");
            assert!(e.to_string().ends_with(why), "{e}");
        }
    }
}
