//! The inputs of a rail crossing's judgement a blend failure names missing.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// Which input of a rail crossing's judgement is missing ([`BlendFailure::CrossingInputMissing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CrossingInput {
    /// The place on the side edge where the rail crosses it.
    CrossingOnSideEdge,
    /// The radial edge use of the edge use that bounds material.
    MaterialEdgeUseRadial,
    /// The tangency budget of the round a sheet face belongs to.
    SheetFaceTangencyBudget,
    /// The curve of the side edge a rail crosses.
    SideEdgeCurve,
}

impl CrossingInput {
    /// What is missing, as a phrase ("the side edge's curve").
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::CrossingOnSideEdge => "the rollover event on the side edge",
            Self::MaterialEdgeUseRadial => "the material edge use's radial",
            Self::SheetFaceTangencyBudget => "the sheet face's tangency budget",
            Self::SideEdgeCurve => "the side edge's curve",
        }
    }
}
