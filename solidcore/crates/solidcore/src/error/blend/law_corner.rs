//! What the exact round at a radius law's corner looks for and does not find.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// What the exact round at a radius law's corner looked for and did not find
/// ([`BlendFailure::LawCornerNotFound`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LawCornerSearch {
    /// The edge of a side's face between the corner's rail points.
    CornerEdge,
    /// The curve where the sides of a peak cross, from rail point to rail point.
    Crossing,
    /// A side's corner curve, oriented from its edge's start vertex to its end vertex.
    CurveBetweenEnds,
    /// The edge the faces on either side of the corner share.
    SharedEdge,
}

impl LawCornerSearch {
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::CornerEdge => "edge between the rail points on a side's face",
            Self::Crossing => "crossing of the sides between the rail points",
            Self::CurveBetweenEnds => "corner curve between its edge's end vertices",
            Self::SharedEdge => "edge shared by the faces on either side",
        }
    }
}
