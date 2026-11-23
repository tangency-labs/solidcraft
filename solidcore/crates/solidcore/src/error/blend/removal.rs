//! Why a fillet removal refuses, and the class of each refusal.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

use crate::blend::fillet_exact::BlendSpine;
use crate::error::ErrorKind;

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
