//! Error type shared by every solidcore operation.
//!
//! Upstream: `source/SMLib/inc/SmMessages.h` (`SmStatus`, `SM_SUCCESS`, `SM_ERR_*`
//! codes and the `SE`/`SER`/`NER` error-propagation macros).
//!
//! | Rust                        | Upstream                                         |
//! |-----------------------------|--------------------------------------------------|
//! | [`SolidError`]              | non-`SM_SUCCESS` `SmStatus` values               |
//! | [`SolidError::InvalidInput`]| `SM_ERR_INVALID_INPUT`                           |
//! | [`SolidError::Upstream`]    | any other `SM_ERR_*` code, carried numerically   |
//! | `?` operator                | `SER(...)` / `NER(...)` (return on error)        |
//!
//! Every variant names the `operation` (a `&'static str`, by convention the Rust path
//! of the failing function, e.g. `"math::Vec3::unitize"`) so that errors surfaced to
//! callers stay actionable without a backtrace.

use crate::topology::{BrepId, EdgeId, FaceId, LoopId, ShellId, VertexId};

/// Shorthand for `core::result::Result<T, SolidError>`.
pub type Result<T, E = SolidError> = core::result::Result<T, E>;

/// A reference to one topological entity, used as error context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityRef {
    /// A whole B-rep.
    Brep(BrepId),
    /// A shell.
    Shell(ShellId),
    /// A face.
    Face(FaceId),
    /// A loop (face boundary).
    Loop(LoopId),
    /// An edge.
    Edge(EdgeId),
    /// A vertex.
    Vertex(VertexId),
}

impl core::fmt::Display for EntityRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Brep(id) => write!(f, "brep {}", id.index()),
            Self::Shell(id) => write!(f, "shell {}", id.index()),
            Self::Face(id) => write!(f, "face {}", id.index()),
            Self::Loop(id) => write!(f, "loop {}", id.index()),
            Self::Edge(id) => write!(f, "edge {}", id.index()),
            Self::Vertex(id) => write!(f, "vertex {}", id.index()),
        }
    }
}

/// Errors returned by solidcore operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum SolidError {
    /// An argument was out of range, malformed or inconsistent
    /// (upstream `SM_ERR_INVALID_INPUT`).
    #[error("{operation}: invalid input: {reason}")]
    InvalidInput {
        /// Operation that rejected the input.
        operation: &'static str,
        /// Human-readable description of what was wrong.
        reason: &'static str,
    },

    /// A typed id did not refer to a live entity of the B-rep it was used with.
    #[error("{operation}: {entity} not found")]
    NotFound {
        /// Operation that looked the entity up.
        operation: &'static str,
        /// The missing entity.
        entity: EntityRef,
    },

    /// Geometry was degenerate where a non-degenerate value was required
    /// (zero-length vector, collapsed edge, singular matrix, ...).
    #[error("{operation}: degenerate geometry{}", fmt_entity(*.entity))]
    Degenerate {
        /// Operation that hit the degeneracy.
        operation: &'static str,
        /// Entity involved, when known.
        entity: Option<EntityRef>,
    },

    /// An iterative solver did not converge.
    #[error("{operation}: did not converge after {iterations} iterations")]
    NotConverged {
        /// Operation whose solver failed.
        operation: &'static str,
        /// Iterations performed before giving up.
        iterations: u32,
    },

    /// The operation is not supported for this combination of inputs
    /// (e.g. a geometry type upstream does not handle).
    #[error("{operation}: unsupported: {reason}")]
    Unsupported {
        /// Operation that does not support the input.
        operation: &'static str,
        /// What is unsupported.
        reason: &'static str,
    },

    /// An upstream `SM_ERR_*` status with no dedicated variant, kept numerically so
    /// difftests can compare it with the C++ status code.
    #[error("{operation}: upstream status {code}{}", fmt_entity(*.entity))]
    Upstream {
        /// Operation that failed.
        operation: &'static str,
        /// Upstream `SmStatus` value.
        code: i32,
        /// Entity involved, when known.
        entity: Option<EntityRef>,
    },
}

fn fmt_entity(entity: Option<EntityRef>) -> String {
    entity.map_or_else(String::new, |e| format!(" at {e}"))
}

impl SolidError {
    /// The operation that produced this error.
    #[must_use]
    pub fn operation(&self) -> &'static str {
        match self {
            Self::InvalidInput { operation, .. }
            | Self::NotFound { operation, .. }
            | Self::Degenerate { operation, .. }
            | Self::NotConverged { operation, .. }
            | Self::Unsupported { operation, .. }
            | Self::Upstream { operation, .. } => operation,
        }
    }

    /// The entity this error refers to, if any.
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match self {
            Self::NotFound { entity, .. } => Some(*entity),
            Self::Degenerate { entity, .. } | Self::Upstream { entity, .. } => *entity,
            Self::InvalidInput { .. } | Self::NotConverged { .. } | Self::Unsupported { .. } => {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_operation_and_entity() {
        let e = SolidError::NotFound {
            operation: "topology::Brep::face",
            entity: EntityRef::Face(FaceId::from_index(7)),
        };
        assert_eq!(e.to_string(), "topology::Brep::face: face 7 not found");
        assert_eq!(e.operation(), "topology::Brep::face");
        assert_eq!(e.entity(), Some(EntityRef::Face(FaceId::from_index(7))));

        let d = SolidError::Degenerate {
            operation: "op",
            entity: None,
        };
        assert_eq!(d.to_string(), "op: degenerate geometry");
    }
}
