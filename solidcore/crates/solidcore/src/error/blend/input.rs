//! The caller-supplied inputs of a blend routine a blend failure names missing.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// Which caller-supplied input of a blend routine is missing ([`BlendFailure::InputMissing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlendInput {
    /// The evaluation curve of an edge in the caller's body.
    EdgeCurve {
        /// The edge whose evaluation curve is missing.
        edge: crate::topology::EdgeId,
    },
    /// The edge of an edge use in the caller's body.
    EdgeuseEdge {
        /// The edge use whose edge is missing.
        edgeuse: crate::topology::EdgeuseId,
    },
    /// The solver's edge use on a rail.
    SolverEdgeuse {
        /// The rail's index.
        rail: usize,
    },
    /// The solver's support surface on a rail.
    SupportSurface {
        /// The rail's index.
        rail: usize,
    },
    /// The solver's surface generator.
    SurfaceGenerator,
}

impl BlendInput {
    /// What is missing, including the rail or topology id where known.
    #[must_use]
    pub fn what(self) -> String {
        match self {
            Self::EdgeCurve { edge } => {
                format!("the edge's evaluation curve (edge {})", edge.index())
            }
            Self::EdgeuseEdge { edgeuse } => {
                format!("the edge use's edge (edge use {})", edgeuse.index())
            }
            Self::SolverEdgeuse { rail } => format!("the solver's edge use on rail {rail}"),
            Self::SupportSurface { rail } => format!("the solver's support surface on rail {rail}"),
            Self::SurfaceGenerator => "the solver's surface generator".into(),
        }
    }
}
