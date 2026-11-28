//! Why a blend's overflow fails: its missing inputs and the drops that find no foot.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// What a face or edge a blend's overflow reads lacks ([`BlendFailure::OverflowInputMissing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OverflowInput {
    /// An edge's curve.
    EdgeCurve,
    /// An edge's end vertex.
    EdgeEndVertex,
    /// An edge's vertex at its other end from a vertex on it.
    EdgeOtherVertex,
    /// An edge's start vertex.
    EdgeStartVertex,
    /// A face's surface.
    FaceSurface,
}

impl OverflowInput {
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::EdgeCurve => "an edge a blend's overflow reads has no curve",
            Self::EdgeEndVertex => "an edge a blend's overflow reads has no end vertex",
            Self::EdgeOtherVertex => {
                "an edge a blend's overflow walks has no vertex at its other end"
            }
            Self::EdgeStartVertex => "an edge a blend's overflow reads has no start vertex",
            Self::FaceSurface => "a face a blend's overflow reads has no surface",
        }
    }
}

/// What a point of the cliff overflow does not drop onto
/// ([`BlendFailure::OverflowPointDoesNotDrop`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OverflowDrop {
    /// The middle of the filleted edge, onto a fillet face cut at an open end.
    EdgeMiddleOntoFilletFace,
    /// An end of a piece of the body's edge an extension runs along, onto the edge's curve
    /// (within ten tolerances).
    EdgePieceEndOntoEdge,
    /// A sample of a closing face's loop, onto the face's surface.
    LoopSampleOntoSurface,
}

impl OverflowDrop {
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::EdgeMiddleOntoFilletFace => {
                "the filleted edge's middle does not drop onto a fillet face cut at an open end"
            }
            Self::EdgePieceEndOntoEdge => "an end of an edge's piece does not drop onto the edge",
            Self::LoopSampleOntoSurface => {
                "a sample of a closing face's loop does not drop onto its surface"
            }
        }
    }
}
