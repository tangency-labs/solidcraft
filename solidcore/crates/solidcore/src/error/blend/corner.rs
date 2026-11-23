//! The shapes of a filleted vertex's edges the fillet corner topology does not build.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;
use crate::error::{SM_ERR, SM_ERR_NULL_POINTER};

/// A shape of the edges at a filleted vertex that the fillet corner topology
/// (`crate::blend::fillet_corner::topology`) does not build
/// ([`BlendFailure::CornerShapeNotBuilt`]). Each is a fact of the body's edges at the vertex;
/// none is a defect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CornerShape {
    /// A corner of four edges whose two blended edges are next to each other round the vertex.
    AdjacentBlendedEdges,
    /// A bevel corner (every edge blended, the blends not blending) of other than three edges.
    BevelNotOfThreeEdges,
    /// A blended edge use the walk round the vertex left out of the ring of the corner's
    /// edges: the walk keeps one use of each edge, so an edge that meets the vertex twice (a
    /// closed edge) is in the ring by its other use.
    BlendedEdgeOffRing,
    /// A corner of two edges alone at the vertex whose first blended edge does not lie
    /// between exactly two faces: the blends' crossing has an end on each of its faces.
    EdgeNotBetweenTwoFaces,
    /// An end corner (one blended edge) of at most three edges whose two side edges share
    /// no face.
    EndSideEdgesShareNoFace,
    /// A corner of two blended edges among more than four edges.
    MoreThanFourEdges,
    /// A concave corner of four edges, two blended, whose side edges are both on the concave
    /// side of the blends: no side edge is convex.
    NoConvexSideEdge,
    /// A Join at one closed blended edge where another edge lies on neither of the blend's
    /// faces.
    RingEdgeOffClosedBlendFaces,
    /// Two edges next to each other in the ring of the corner's edges that share no face:
    /// the ring keeps one use of each edge, so an edge that meets the vertex twice stands
    /// between edges that are not on one face.
    RingEdgesShareNoFace,
    /// A Join whose one edge between the blends on a side does not meet the first blended
    /// edge in a loop of either of its faces.
    SideEdgeNotBesideBlend,
}

impl CornerShape {
    /// The status the site the shape replaces returned: `SM_ERR_NULL_POINTER` where the C++
    /// dereferences the face or edge use it did not find (`NER`), else `SM_ERR`.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn upstream_status(self) -> i32 {
        match self {
            Self::AdjacentBlendedEdges => SM_ERR,
            Self::BevelNotOfThreeEdges => SM_ERR,
            Self::BlendedEdgeOffRing => SM_ERR,
            Self::EdgeNotBetweenTwoFaces => SM_ERR,
            Self::EndSideEdgesShareNoFace => SM_ERR,
            Self::MoreThanFourEdges => SM_ERR,
            Self::NoConvexSideEdge => SM_ERR_NULL_POINTER,
            Self::RingEdgeOffClosedBlendFaces => SM_ERR,
            Self::RingEdgesShareNoFace => SM_ERR_NULL_POINTER,
            Self::SideEdgeNotBesideBlend => SM_ERR_NULL_POINTER,
        }
    }
}
