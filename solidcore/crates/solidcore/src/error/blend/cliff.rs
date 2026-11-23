//! The inputs of a blend's trim at a cliff a blend failure names missing.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

/// A face input required to trim a blend at a cliff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CliffInput {
    /// The face use facing out of the body.
    OutwardFaceUse,
    /// The planar support extending the wall.
    Plane,
    /// The face's surface.
    Surface,
}

impl CliffInput {
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::OutwardFaceUse => "outward face use",
            Self::Plane => "plane",
            Self::Surface => "surface",
        }
    }
}
