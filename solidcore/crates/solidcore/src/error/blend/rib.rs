//! The points of a tangent end rib a blend failure names.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// A point of a tangent end rib ([`BlendFailure::RibPointNotDropped`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RibPoint {
    /// The other end, on the other rail.
    OtherEnd,
    /// The stop between the rails, where the rib passes onto the next face.
    Stop,
    /// The tangent end, where the fillet surface touches the end surface.
    TangentEnd,
}

impl RibPoint {
    /// The point, as a phrase ("tangent end").
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::OtherEnd => "other end",
            Self::Stop => "stop",
            Self::TangentEnd => "tangent end",
        }
    }
}
