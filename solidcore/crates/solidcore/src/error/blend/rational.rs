//! The shapes, nodal arcs and weight steps of the exact fillet's rational form that a blend
//! failure names.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;
use crate::error::ErrorKind;

/// Why a nodal quartic has no usable arc ([`BlendFailure::NodalArcUnavailable`]).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodalArc {
    /// Both ends of the trace lie at the node.
    BothEndsAtNode,
    /// The tangent cone vanishes on the tangent plane: higher-order contact.
    HigherOrderContact,
    /// The tangent cone has negative discriminant: the node is isolated.
    IsolatedNode,
    /// No candidate passed the sampled sign and endpoint checks.
    NoFiniteRegularArc,
}

impl NodalArc {
    /// The class of the failure: [`ErrorKind::Degenerate`] for an isolated node, a
    /// higher-order contact or both ends at the node, [`ErrorKind::NotFound`] where no
    /// finite regular arc passed the checks.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::BothEndsAtNode => ErrorKind::Degenerate,
            Self::HigherOrderContact => ErrorKind::Degenerate,
            Self::IsolatedNode => ErrorKind::Degenerate,
            Self::NoFiniteRegularArc => ErrorKind::NotFound,
        }
    }
}

/// The unusable shape of a plane's quadric section ([`BlendFailure::SectionNotConic`]).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionShape {
    /// The section has no real points.
    Empty,
    /// The section consists of one or two lines, not a regular conic.
    Lines,
    /// No quadratic form remains in the plane: unsupported by the conic builder.
    NoQuadraticForm,
    /// The centred section has collapsed to a point.
    Point,
}

impl SectionShape {
    /// The class of the failure: [`ErrorKind::NotFound`] for an empty section,
    /// [`ErrorKind::Degenerate`] for lines or a point, [`ErrorKind::Unsupported`] where no
    /// quadratic form remains.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::Empty => ErrorKind::NotFound,
            Self::Lines => ErrorKind::Degenerate,
            Self::NoQuadraticForm => ErrorKind::Unsupported,
            Self::Point => ErrorKind::Degenerate,
        }
    }
}

/// Which rational weight construction stopped ([`BlendFailure::RationalWeightsUnavailable`]).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightStep {
    /// At least one segment endpoint weight is not positive.
    EndpointWeights,
    /// Subdivision reached its depth bound without positive coefficients.
    PositiveWeights,
    /// The spine needs a non-finite or excessive number of segments.
    SegmentPartition,
    /// The surface's minimum weight is not positive or its weight ratio exceeds the cap.
    WeightRange,
}
