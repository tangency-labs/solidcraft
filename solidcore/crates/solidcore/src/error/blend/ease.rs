//! Why the ease of a round before a setback fails: its missing inputs, stops and geometry.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;
use crate::error::ErrorKind;

/// What a thing the ease of a round before a setback reads lacks
/// ([`BlendFailure::EaseInputMissing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EaseInput {
    /// A round's centre line.
    CentreLine,
    /// An eased corner's filleted vertex.
    CornerVertex,
    /// An edge's curve (a rail's or the filleted edge's).
    EdgeCurve,
    /// An edge use's face.
    EdgeUseFace,
    /// A face's surface.
    FaceSurface,
    /// A round's rail edge.
    RoundRail,
    /// A round's solver.
    RoundSolver,
    /// A round's fillet surface.
    RoundSurface,
    /// A face for the point a setback vertex classifies on (it classifies on no face).
    SetbackFace,
    /// A round solver's offset surface for a rail (made when the solver is loaded).
    SolverSurface,
    /// A round solver's surface generator.
    SurfaceGenerator,
}

impl EaseInput {
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::CentreLine => "an eased round has no centre line",
            Self::CornerVertex => "an eased corner has no filleted vertex",
            Self::EdgeCurve => "an edge the ease reads has no curve",
            Self::EdgeUseFace => "an edge use the ease reads has no face",
            Self::FaceSurface => "a face the ease reads has no surface",
            Self::RoundRail => "an eased round has no rail edge on a side",
            Self::RoundSolver => "an eased round has no solver",
            Self::RoundSurface => "an eased round has no fillet surface",
            Self::SetbackFace => "a setback vertex of an eased corner classifies on no face",
            Self::SolverSurface => "an eased round's solver has no offset surface for a rail",
            Self::SurfaceGenerator => "an eased round's solver has no surface generator",
        }
    }
}

/// Where the ease of a round before a setback stops ([`BlendFailure::EaseNotBuilt`]), each
/// with its kind ([`EaseStop::class`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EaseStop {
    /// The start of the ease drops onto the rail where the setback's section does: the
    /// ease has no length ([`ErrorKind::Degenerate`]).
    EaseHasNoLength,
    /// The setback plus the ease runs past the far end of the filleted edge
    /// ([`ErrorKind::OutsideDomain`]).
    EasePastEdgeEnd,
    /// The start of the ease does not drop onto the rail ([`ErrorKind::NotFound`]).
    EaseStartOffRail,
    /// Neither direction of the round's surface runs along its rails
    /// ([`ErrorKind::Unsupported`]).
    NoDirectionAlongRails,
    /// No section of the round lies in the plane at the eased setback
    /// ([`ErrorKind::NotFound`]).
    NoSectionInPlane,
    /// The origin of the plane at the eased setback has other than one closest point on an
    /// offset surface of the round ([`ErrorKind::NotFound`]).
    PlaneOriginNotOneClosest,
    /// A rail does not drop onto its face's surface as one UV curve
    /// ([`ErrorKind::Unsupported`]).
    RailNotOneUvCurve,
    /// The round's sections are neither circular nor curvature-continuous: the ease eases
    /// a circular round only ([`ErrorKind::Unsupported`]).
    SectionsNotCircular,
    /// The face a setback vertex classifies on carries neither of the round's rails
    /// ([`ErrorKind::NotFound`]).
    SetbackFaceCarriesNoRail,
    /// A setback vertex lies on other than exactly one fillet geometry: the ease eases one
    /// round per setback vertex ([`ErrorKind::Unsupported`]).
    SetbackNotOnOneRound,
    /// A setback vertex of the eased corner is not yet placed (not processed): the ease
    /// does not place it ([`ErrorKind::Unsupported`]).
    SetbackVertexNotPlaced,
    /// The setback vertex does not drop onto the round's rail ([`ErrorKind::NotFound`]).
    SetbackVertexOffRail,
    /// A rail's UV curve does not run over the rail's own interval
    /// ([`ErrorKind::Unsupported`]).
    UvCurveOffRailInterval,
}

impl EaseStop {
    /// The stop's kind.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::EaseHasNoLength => ErrorKind::Degenerate,
            Self::EasePastEdgeEnd => ErrorKind::OutsideDomain,
            Self::EaseStartOffRail => ErrorKind::NotFound,
            Self::NoDirectionAlongRails => ErrorKind::Unsupported,
            Self::NoSectionInPlane => ErrorKind::NotFound,
            Self::PlaneOriginNotOneClosest => ErrorKind::NotFound,
            Self::RailNotOneUvCurve => ErrorKind::Unsupported,
            Self::SectionsNotCircular => ErrorKind::Unsupported,
            Self::SetbackFaceCarriesNoRail => ErrorKind::NotFound,
            Self::SetbackNotOnOneRound => ErrorKind::Unsupported,
            Self::SetbackVertexNotPlaced => ErrorKind::Unsupported,
            Self::SetbackVertexOffRail => ErrorKind::NotFound,
            Self::UvCurveOffRailInterval => ErrorKind::Unsupported,
        }
    }

    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::EaseHasNoLength => {
                "the ease's start drops onto the rail where the setback's section does"
            }
            Self::EasePastEdgeEnd => {
                "the setback plus the ease runs past the end of the filleted edge"
            }
            Self::EaseStartOffRail => "the ease's start does not drop onto the rail",
            Self::NoDirectionAlongRails => {
                "neither direction of the round's surface runs along its rails"
            }
            Self::NoSectionInPlane => {
                "no section of the round lies in the plane at the eased setback"
            }
            Self::PlaneOriginNotOneClosest => {
                "the plane's origin at the eased setback has no single closest point on an \
                 offset surface"
            }
            Self::RailNotOneUvCurve => {
                "a rail does not drop onto its face's surface as one UV curve"
            }
            Self::SectionsNotCircular => {
                "an eased round's sections are neither circular nor curvature-continuous"
            }
            Self::SetbackFaceCarriesNoRail => {
                "the face a setback vertex classifies on carries neither rail of the round"
            }
            Self::SetbackNotOnOneRound => {
                "a setback vertex of an eased corner is not on exactly one round"
            }
            Self::SetbackVertexNotPlaced => "a setback vertex of an eased corner is not placed",
            Self::SetbackVertexOffRail => "a setback vertex does not drop onto the round's rail",
            Self::UvCurveOffRailInterval => {
                "a rail's UV curve does not run over the rail's own interval"
            }
        }
    }
}

/// Which curve or surface the ease of a round before a setback finds without a B-spline
/// form ([`BlendFailure::EaseGeometryNotBSpline`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EaseGeometry {
    /// A rail's UV curve dropped onto its face's surface.
    DroppedUvCurve,
    /// A rail's curve.
    RailCurve,
    /// The eased round's fillet surface.
    RoundSurface,
    /// A rail's UV curve as the trace left it.
    TracedUvCurve,
}

impl EaseGeometry {
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::DroppedUvCurve => "a rail's UV curve dropped onto its face has no B-spline form",
            Self::RailCurve => "an eased round's rail curve has no B-spline form",
            Self::RoundSurface => "an eased round's fillet surface has no B-spline form",
            Self::TracedUvCurve => "a rail's traced UV curve has no B-spline form",
        }
    }
}
