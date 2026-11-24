//! The inputs of the planar-band contact query a blend failure names missing.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// Which input of the planar-band contact query is missing ([`BlendFailure::BandInputMissing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BandInput {
    /// The blended edge.
    BlendedEdge,
    /// The blended edge's curve.
    BlendedEdgeCurve,
    /// An endpoint of the blended edge.
    BlendedEdgeEndpoint,
    /// The chamfer's setback.
    ChamferSetback,
    /// The closest feet of the two bands.
    ClosestBandFeet,
    /// A solver's edge use.
    EdgeUse,
    /// The extrusion's control point in its stored net.
    ExtrusionControlPoint,
    /// The exact product of the extrusion's translation.
    ExtrusionTranslationProduct,
    /// The first offset of the chamfer.
    FirstOffset,
    /// The foot of a drop on the other support.
    FootOnOtherSupport,
    /// An endpoint of the line.
    LineEndpoint,
    /// The other offset of the chamfer.
    OtherOffset,
    /// The support on the other side of the blended edge.
    OtherSupport,
    /// The planar support.
    PlanarSupport,
    /// The selected edge.
    SelectedEdge,
    /// The face the two bands share.
    SharedFace,
}

impl BandInput {
    /// What is missing, as a phrase ("the shared face").
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::BlendedEdge => "the blended edge",
            Self::BlendedEdgeCurve => "the blended edge's curve",
            Self::BlendedEdgeEndpoint => "the blended edge's endpoint",
            Self::ChamferSetback => "the chamfer setback",
            Self::ClosestBandFeet => "the closest band feet",
            Self::EdgeUse => "the edge use",
            Self::ExtrusionControlPoint => "the extrusion control point",
            Self::ExtrusionTranslationProduct => "the exact extrusion translation product",
            Self::FirstOffset => "the first offset",
            Self::FootOnOtherSupport => "the foot on the other support",
            Self::LineEndpoint => "the line endpoint",
            Self::OtherOffset => "the other offset",
            Self::OtherSupport => "the other support",
            Self::PlanarSupport => "the planar support",
            Self::SelectedEdge => "the selected edge",
            Self::SharedFace => "the shared face",
        }
    }
}
