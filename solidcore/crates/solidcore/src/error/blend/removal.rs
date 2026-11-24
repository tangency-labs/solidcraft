//! Why a fillet removal refuses or stops, and the class of each cause.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;
use crate::blend::fillet_exact::BlendSpine;
use crate::error::{ErrorKind, SM_ERR, SM_ERR_NULL_POINTER};

/// Why a fillet removal (`crate::blend::fillet_removal`, its ring removal included) refuses
/// the face it is asked to remove. Each is a fact of the face and its neighbours, not a
/// defect; the class is [`removal_class`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemovalRefusal {
    /// Two adjacent edges of the fillet face are both tangent.
    AdjacentEdgesTangent,
    /// A corner of more than three edges keeps its end edge, which would need a fitted
    /// intersection to run on to the new corner.
    CornerNeedsFittedIntersection,
    /// The new corner is off the line of the side edge that would run on to it.
    CornerOffSideLine,
    /// An edge of the fillet face does not border exactly two faces.
    EdgeNotBetweenTwoFaces,
    /// A tangent or end edge of the fillet face does not have two vertices.
    EdgeWithoutTwoVertices,
    /// An end edge of the fillet face shares a vertex with neither tangent edge, so the
    /// four edges do not meet at four corners.
    EndEdgeMissesTangentEdges,
    /// An end face's surface is not a B-spline surface.
    EndFaceNotBSpline,
    /// A face the removal extends would grow past its surface's domain, which only a plane
    /// continues exactly.
    FaceGrowsPastDomain,
    /// The ring fillet has more than two rails.
    MoreThanTwoRails,
    /// Neither of the fillet face's first two edges is tangent.
    NoTangentEdge,
    /// The face is not a ring fillet.
    NotARing,
    /// The fillet face does not have four edges.
    NotFourEdges,
    /// The selected faces do not form one connected strip with two rails.
    NotOneTangentChain,
    /// The edge opposite the fillet face's tangent edge is not tangent.
    OppositeEdgeNotTangent,
    /// The new sharp edge's UV trim could not be certified within the original rail tolerance.
    RailTrimOutsideTolerance,
    /// A rail's vertex has an edge other than one open seam of its tangent face.
    RailVertexHasOtherEdges,
    /// The ring fillet does not have two rails between two other faces.
    RailsNotBetweenTwoFaces,
    /// A tangent face's seam crosses the sharp edge at more than one place.
    SeamCrossesSharpEdgeTwice,
    /// The ring fillet's seam does not join the two rails' vertices.
    SeamDoesNotJoinRails,
    /// A seam has more than two edge uses with their own UV trim curves.
    SeamHasMoreThanTwoTrims,
    /// A seam has no edge use with its own UV trim curve.
    SeamHasNoOwnTrim,
    /// A seam's UV trim, run on, does not reach the new vertex on its face's surface.
    SeamTrimMissesNewVertex,
    /// The two tangent faces' seams cross the sharp edge at different places.
    SeamsCrossApart,
    /// The side edge that would run on to the new corner is not a straight B-spline curve,
    /// so it cannot run on exactly.
    SideEdgeNotStraight,
    /// A tangent face's surface is not a B-spline surface.
    TangentFaceNotBSpline,
    /// The tangent surfaces of a ring fillet do not meet in a closed curve near its rails
    /// within their domains: the sharp edge would lie on a continuation of one of them.
    TangentSurfacesDoNotMeet,
    /// The support intersection has a shape the removal cannot rebuild as a sharp edge.
    UnsupportedSharpSpine(BlendSpine),
}

/// The class of a fillet removal's refusal, for `reason`: [`ErrorKind::RemovalNotExact`] (130)
/// where the removal would continue a face or an edge past its geometry, and
/// [`ErrorKind::NotARemovableFillet`] (131) where the face and its neighbours are not a fillet
/// the removal recognises. Both are refusals no size decides.
#[must_use]
#[allow(
    clippy::match_same_arms,
    reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
)]
pub const fn removal_class(reason: RemovalRefusal) -> ErrorKind {
    use RemovalRefusal as R;
    match reason {
        R::AdjacentEdgesTangent => ErrorKind::NotARemovableFillet,
        R::CornerNeedsFittedIntersection => ErrorKind::RemovalNotExact,
        R::CornerOffSideLine => ErrorKind::RemovalNotExact,
        R::EdgeNotBetweenTwoFaces => ErrorKind::NotARemovableFillet,
        R::EdgeWithoutTwoVertices => ErrorKind::NotARemovableFillet,
        R::EndEdgeMissesTangentEdges => ErrorKind::NotARemovableFillet,
        R::EndFaceNotBSpline => ErrorKind::NotARemovableFillet,
        R::FaceGrowsPastDomain => ErrorKind::RemovalNotExact,
        R::MoreThanTwoRails => ErrorKind::NotARemovableFillet,
        R::NoTangentEdge => ErrorKind::NotARemovableFillet,
        R::NotARing => ErrorKind::NotARemovableFillet,
        R::NotFourEdges => ErrorKind::NotARemovableFillet,
        R::NotOneTangentChain => ErrorKind::NotARemovableFillet,
        R::OppositeEdgeNotTangent => ErrorKind::NotARemovableFillet,
        R::RailTrimOutsideTolerance => ErrorKind::RemovalNotExact,
        R::RailVertexHasOtherEdges => ErrorKind::NotARemovableFillet,
        R::RailsNotBetweenTwoFaces => ErrorKind::NotARemovableFillet,
        R::SeamCrossesSharpEdgeTwice => ErrorKind::NotARemovableFillet,
        R::SeamDoesNotJoinRails => ErrorKind::NotARemovableFillet,
        R::SeamHasMoreThanTwoTrims => ErrorKind::NotARemovableFillet,
        R::SeamHasNoOwnTrim => ErrorKind::NotARemovableFillet,
        R::SeamTrimMissesNewVertex => ErrorKind::RemovalNotExact,
        R::SeamsCrossApart => ErrorKind::NotARemovableFillet,
        R::SideEdgeNotStraight => ErrorKind::RemovalNotExact,
        R::TangentFaceNotBSpline => ErrorKind::NotARemovableFillet,
        R::TangentSurfacesDoNotMeet => ErrorKind::RemovalNotExact,
        R::UnsupportedSharpSpine(_) => ErrorKind::RemovalNotExact,
    }
}

/// What a thing of the body a fillet removal reads lacks
/// ([`BlendFailure::RemovalInputMissing`]): a link of the topology the removal walks. The
/// removal reads the caller's body and the topology its own edits leave alike, so the
/// cause names what is missing and leaves the blame open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemovalInput {
    /// An edge use has no face.
    EdgeUseFace,
    /// An edge use has no loop use.
    EdgeUseLoopUse,
    /// An edge use has no mate.
    EdgeUseMate,
    /// An edge use has no neighbour in its loop (clockwise or counterclockwise).
    EdgeUseNeighbour,
    /// An edge has no edge use on a face it borders.
    EdgeUseOnFace,
    /// An edge use has no radial edge use.
    EdgeUseRadial,
    /// An edge use has no vertex use.
    EdgeUseVertexUse,
    /// The ring fillet removal has no face to delete: its face was already removed by an
    /// earlier call of the same removal.
    FaceToDelete,
    /// A rail edge has no primary edge use.
    PrimaryEdgeUse,
    /// A shell has no region.
    ShellRegion,
    /// A corner vertex of three edges has no third edge beside the end and tangent edges.
    SideEdge,
    /// A corner vertex has no vertex use on a face around it.
    VertexUseOnFace,
}

impl RemovalInput {
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::EdgeUseFace => "an edge use the fillet removal reads has no face",
            Self::EdgeUseLoopUse => "an edge use the fillet removal relinks has no loop use",
            Self::EdgeUseMate => "an edge use the fillet removal reads has no mate",
            Self::EdgeUseNeighbour => {
                "an edge use the fillet removal relinks has no neighbour in its loop"
            }
            Self::EdgeUseOnFace => "an edge the fillet removal reads has no edge use on its face",
            Self::EdgeUseRadial => "an edge use the fillet removal relinks has no radial edge use",
            Self::EdgeUseVertexUse => "an edge use the fillet removal reads has no vertex use",
            Self::FaceToDelete => "the ring fillet removal has no face left to delete",
            Self::PrimaryEdgeUse => "a rail of the ring fillet has no primary edge use",
            Self::ShellRegion => "a shell the ring fillet removal joins has no region",
            Self::SideEdge => {
                "a corner of three edges has no side edge beside its end and tangent edges"
            }
            Self::VertexUseOnFace => "a corner vertex has no vertex use on a face around it",
        }
    }
}

/// Where a fillet removal stops ([`BlendFailure::RemovalStopped`]), each with its kind
/// ([`RemovalStop::class`]) and status ([`RemovalStop::status`]). A drop or a search that
/// stopped, or found nothing, proves nothing about the geometry (#15034 rule 5), so it is
/// neither a refusal nor a defect; a call that failed is kept as the cause and decides.
/// The stops also name the arguments of the removal's public edits that do not hold
/// (an edge not on the face given, no vertex given: [`ErrorKind::InvalidInput`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemovalStop {
    /// The local intersection of an end face's and the two tangent faces' surfaces finds no
    /// new corner ([`ErrorKind::NotFound`]).
    CornerNotSolved,
    /// The three surfaces at a new corner are farther apart than the removal's tolerance
    /// ([`ErrorKind::NotWithinTolerance`]).
    CornerSurfacesApart,
    /// A curve continued to a far parameter comes back empty: the parameter lies within its
    /// interval, the continuation has no length ([`ErrorKind::Degenerate`];
    /// `SM_ERR_NULL_POINTER`, upstream `NER`).
    CurveNotExtended,
    /// The edge given has no edge use on the face given ([`ErrorKind::InvalidInput`]).
    EdgeNotOnFace,
    /// The edge given to be glued away is not a lamina edge ([`ErrorKind::InvalidInput`]).
    EdgeToDeleteNotLamina,
    /// A sphere's or torus's net, extended to the new vertex, is not recognised as the
    /// same sphere or torus ([`ErrorKind::NotFound`]).
    ExtendedSupportNotRecognised,
    /// A sphere's or torus's net, extended to the new vertex, deviates from the surface by
    /// more than the tolerance ([`ErrorKind::NotWithinTolerance`]).
    ExtensionDeviates,
    /// The drop of a side edge's far vertex onto its joined curve failed
    /// ([`ErrorKind::Failed`]; the drop's error is kept as the cause).
    FarVertexDropFailed,
    /// The drop of a side edge's far vertex onto its joined curve stopped
    /// ([`ErrorKind::NotConverged`]).
    FarVertexDropStopped,
    /// A side edge's far vertex lies farther than the tolerance from its joined curve
    /// ([`ErrorKind::NotWithinTolerance`]).
    FarVertexOffSideEdge,
    /// A sphere's or torus's net, extended to the new vertex, is not a B-spline net
    /// ([`ErrorKind::Unsupported`]).
    GrownSurfaceNotBSpline,
    /// A curve has no certified bounding box over its span ([`ErrorKind::Unsupported`];
    /// `SM_ERR_NULL_POINTER`, upstream `NER`).
    NoCertifiedBox,
    /// The edge given has no edge use oriented with it on the face given
    /// ([`ErrorKind::InvalidInput`]).
    NoSameUseOnFace,
    /// The drop of a rail's sample onto the other rail stopped
    /// ([`ErrorKind::NotConverged`]).
    RailDropStopped,
    /// The drop of the new corner onto a side edge's run-on line failed
    /// ([`ErrorKind::Failed`]; the drop's error is kept as the cause).
    RunOnDropFailed,
    /// The drop of the new corner onto a side edge's run-on line stopped
    /// ([`ErrorKind::NotConverged`]).
    RunOnDropStopped,
    /// The end of a straight edge to run on has no speed ([`ErrorKind::Degenerate`]).
    RunOnEndHasNoSpeed,
    /// A curved seam has no crossing with the sharp edge and no continuation the removal
    /// proves ([`ErrorKind::NotFound`]).
    SeamContinuationNotFound,
    /// The global intersection of the sharp edge with a continued seam finds no crossing
    /// ([`ErrorKind::NotFound`]).
    SeamCrossingNotFound,
    /// The drop of the new vertex onto a seam's circle stopped
    /// ([`ErrorKind::NotConverged`]).
    SeamDropStopped,
    /// The ends of a seam's circle map to edge parameters that are not finite
    /// ([`ErrorKind::Degenerate`]).
    SeamParameterNotFinite,
    /// A drop onto a candidate sharp curve stopped, so no candidate is decided
    /// ([`ErrorKind::NotConverged`]).
    SharpCurveNotDecided,
    /// The drop of rail 0's middle onto the sharp curve stopped
    /// ([`ErrorKind::NotConverged`]).
    SharpDropStopped,
    /// The drop of the new corner onto a side edge failed ([`ErrorKind::Failed`]; the
    /// drop's error is kept as the cause).
    SideEdgeDropFailed,
    /// The drop of the new corner onto a side edge stopped ([`ErrorKind::NotConverged`]).
    SideEdgeDropStopped,
    /// The intersection of an end face and a tangent face from the old corner to the new
    /// one finds no curve ([`ErrorKind::NotFound`]).
    SideExtensionNotFound,
    /// The intersection of the two tangent faces through the new corners finds no curve
    /// ([`ErrorKind::NotFound`]).
    TangentIntersectionNotFound,
    /// An edge use's UV trim curve is not made ([`ErrorKind::Missing`];
    /// `SM_ERR_NULL_POINTER`).
    UvTrimNotMade,
    /// The vertex at which to join shells is not given ([`ErrorKind::InvalidInput`];
    /// `SM_ERR_NULL_POINTER`).
    VertexNotGiven,
    /// The vertex to move is neither end of the edge to move it along
    /// ([`ErrorKind::InvalidInput`]; `SM_ERR_NULL_POINTER`).
    VertexNotOnEdge,
}

impl RemovalStop {
    /// The kind of the stop.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::CornerNotSolved => ErrorKind::NotFound,
            Self::CornerSurfacesApart => ErrorKind::NotWithinTolerance,
            Self::CurveNotExtended => ErrorKind::Degenerate,
            Self::EdgeNotOnFace => ErrorKind::InvalidInput,
            Self::EdgeToDeleteNotLamina => ErrorKind::InvalidInput,
            Self::ExtendedSupportNotRecognised => ErrorKind::NotFound,
            Self::ExtensionDeviates => ErrorKind::NotWithinTolerance,
            Self::FarVertexDropFailed => ErrorKind::Failed,
            Self::FarVertexDropStopped => ErrorKind::NotConverged,
            Self::FarVertexOffSideEdge => ErrorKind::NotWithinTolerance,
            Self::GrownSurfaceNotBSpline => ErrorKind::Unsupported,
            Self::NoCertifiedBox => ErrorKind::Unsupported,
            Self::NoSameUseOnFace => ErrorKind::InvalidInput,
            Self::RailDropStopped => ErrorKind::NotConverged,
            Self::RunOnDropFailed => ErrorKind::Failed,
            Self::RunOnDropStopped => ErrorKind::NotConverged,
            Self::RunOnEndHasNoSpeed => ErrorKind::Degenerate,
            Self::SeamContinuationNotFound => ErrorKind::NotFound,
            Self::SeamCrossingNotFound => ErrorKind::NotFound,
            Self::SeamDropStopped => ErrorKind::NotConverged,
            Self::SeamParameterNotFinite => ErrorKind::Degenerate,
            Self::SharpCurveNotDecided => ErrorKind::NotConverged,
            Self::SharpDropStopped => ErrorKind::NotConverged,
            Self::SideEdgeDropFailed => ErrorKind::Failed,
            Self::SideEdgeDropStopped => ErrorKind::NotConverged,
            Self::SideExtensionNotFound => ErrorKind::NotFound,
            Self::TangentIntersectionNotFound => ErrorKind::NotFound,
            Self::UvTrimNotMade => ErrorKind::Missing,
            Self::VertexNotGiven => ErrorKind::InvalidInput,
            Self::VertexNotOnEdge => ErrorKind::InvalidInput,
        }
    }

    /// The upstream status the stop keeps: `SM_ERR_NULL_POINTER` where upstream's `NER`
    /// returns it, else `SM_ERR`.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn status(self) -> i32 {
        match self {
            Self::CurveNotExtended => SM_ERR_NULL_POINTER,
            Self::NoCertifiedBox => SM_ERR_NULL_POINTER,
            Self::UvTrimNotMade => SM_ERR_NULL_POINTER,
            Self::VertexNotGiven => SM_ERR_NULL_POINTER,
            Self::VertexNotOnEdge => SM_ERR_NULL_POINTER,
            _ => SM_ERR,
        }
    }

    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::CornerNotSolved => {
                "the end and tangent surfaces' intersection finds no new corner"
            }
            Self::CornerSurfacesApart => {
                "the three surfaces at a new corner are farther apart than the tolerance"
            }
            Self::CurveNotExtended => "a curve continued to its far parameter has no length",
            Self::EdgeNotOnFace => "the edge given has no edge use on the face given",
            Self::EdgeToDeleteNotLamina => "the edge to glue away is not a lamina edge",
            Self::ExtendedSupportNotRecognised => {
                "the extended net of a sphere or torus is not recognised as that surface"
            }
            Self::ExtensionDeviates => {
                "the extended net of a sphere or torus deviates by more than the tolerance"
            }
            Self::FarVertexDropFailed => {
                "the drop of a side edge's far vertex onto its curve failed"
            }
            Self::FarVertexDropStopped => {
                "the drop of a side edge's far vertex onto its curve stopped"
            }
            Self::FarVertexOffSideEdge => {
                "a side edge's far vertex lies off its joined curve by more than the tolerance"
            }
            Self::GrownSurfaceNotBSpline => {
                "the extended net of a sphere or torus is not a B-spline net"
            }
            Self::NoCertifiedBox => "a curve has no certified bounding box",
            Self::NoSameUseOnFace => {
                "the edge given has no edge use oriented with it on the face given"
            }
            Self::RailDropStopped => "the drop of a rail's sample onto the other rail stopped",
            Self::RunOnDropFailed => {
                "the drop of the new corner onto a side edge's run-on line failed"
            }
            Self::RunOnDropStopped => {
                "the drop of the new corner onto a side edge's run-on line stopped"
            }
            Self::RunOnEndHasNoSpeed => "the end of a straight edge to run on has no speed",
            Self::SeamContinuationNotFound => {
                "a curved seam has no crossing with the sharp edge and no proved continuation"
            }
            Self::SeamCrossingNotFound => {
                "the sharp edge and a continued seam have no crossing found"
            }
            Self::SeamDropStopped => "the drop of the new vertex onto a seam's circle stopped",
            Self::SeamParameterNotFinite => {
                "the ends of a seam's circle map to edge parameters that are not finite"
            }
            Self::SharpCurveNotDecided => {
                "a drop onto a candidate sharp curve stopped, so no candidate is decided"
            }
            Self::SharpDropStopped => "the drop of a rail onto the sharp curve stopped",
            Self::SideEdgeDropFailed => "the drop of the new corner onto a side edge failed",
            Self::SideEdgeDropStopped => "the drop of the new corner onto a side edge stopped",
            Self::SideExtensionNotFound => {
                "the end and tangent faces' intersection to the new corner finds no curve"
            }
            Self::TangentIntersectionNotFound => {
                "the tangent faces' intersection through the new corners finds no curve"
            }
            Self::UvTrimNotMade => "an edge use's UV trim curve is not made",
            Self::VertexNotGiven => "the vertex at which to join shells is not given",
            Self::VertexNotOnEdge => "the vertex to move is neither end of the edge given",
        }
    }
}
