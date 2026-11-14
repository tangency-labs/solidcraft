//! Why a Boolean operation fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the Boolean subsystem (`crate::boolean`).
//! Each also records, for difftest only, the status the C++ reports for it (crate-private
//! `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                | Status difftest compares            |
//! |-------------------------------------|-------------------------------------|
//! | [`BooleanFailure::StageFailed`]     | `SM_ERR`                            |
//! | [`BooleanFailure::EntityMissing`]   | `SM_ERR_NULL_POINTER`               |
//! | [`BooleanFailure::SolidLeaks`]      | `SM_ERR` (WR-619, solidcore-native) |
//! | [`BooleanFailure::RelateConflict`]  | `SM_ERR`                            |
//!
//! Deviations: not upstream. A stage's failure names the stage ([`BooleanStage`]) and
//! says in a phrase what went wrong; the handles the stages work on are the
//! intersector's own, not ids of the caller's B-rep, so only [`BooleanFailure::SolidLeaks`]
//! names an entity; [`BooleanFailure::RelateConflict`] places its three by their points
//! (#16098). The failures that are checks of a caller's handles stay
//! [`crate::SolidError::InvalidInput`].

use super::{EntityRef, ErrorKind, SM_ERR, SM_ERR_NULL_POINTER};
use crate::math::Point3;
use crate::topology::FaceId;

/// The stage of a Boolean operation a failure comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BooleanStage {
    /// Recording which entity of one operand matches which of the other.
    Relate,
    /// Imprinting an intersection point or curve on a face.
    Imprint,
    /// Splitting edges where they meet.
    Split,
    /// Closing an intersection loop at a vertex.
    CloseLoop,
    /// Resolving surfaces, edges or faces that coincide.
    Coincidence,
}

impl core::fmt::Display for BooleanStage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Relate => "relating the operands' entities",
            Self::Imprint => "imprinting on a face",
            Self::Split => "splitting edges",
            Self::CloseLoop => "closing an intersection loop",
            Self::Coincidence => "resolving coincident geometry",
        })
    }
}

/// Which operand of the intersector an entity of a [`BooleanFailure::RelateConflict`] is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelateSide {
    /// The B-rep the intersector relates from (`Relate`'s first object).
    Brep,
    /// The other B-rep (`Relate`'s second object).
    Other,
}

impl core::fmt::Display for RelateSide {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Brep => "the B-rep",
            Self::Other => "the other B-rep",
        })
    }
}

/// An entity of an operand, placed by its points for a [`BooleanFailure::RelateConflict`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlacedEntity {
    /// A vertex at the point.
    Vertex(Point3),
    /// An edge from its start vertex's point to its end vertex's.
    Edge(Point3, Point3),
    /// An entity of another kind, or one whose points could not be read.
    Unplaced,
}

impl core::fmt::Display for PlacedEntity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let p = |p: &Point3| format!("({}, {}, {})", p.x, p.y, p.z);
        match self {
            Self::Vertex(at) => write!(f, "the vertex at {}", p(at)),
            Self::Edge(a, b) => write!(f, "the edge from {} to {}", p(a), p(b)),
            Self::Unplaced => f.write_str("an entity"),
        }
    }
}

/// Why a Boolean operation fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum BooleanFailure {
    /// A stage could not continue, as the phrase says (for example "a different entity is
    /// already related to it").
    StageFailed {
        /// The stage.
        stage: BooleanStage,
        /// What went wrong.
        what: &'static str,
    },
    /// A stage needs an entity that the operands' B-reps do not have, as the phrase says
    /// (for example "a vertex at the end of the edge").
    EntityMissing {
        /// The stage.
        stage: BooleanStage,
        /// What is missing.
        what: &'static str,
    },
    /// A union would delete `face`, which bounds a solid against the outside, so the solid
    /// would merge into the outside (opt-in `fixes.boolean_union_refuses_solid_leak`).
    SolidLeaks {
        /// The face, of the B-rep the operation edits.
        face: FaceId,
    },
    /// The intersector was asked to relate `entity`, of `side`, to `asked`, but `entity` is
    /// already related to `mate` (upstream `SmTopologyIntersector::Relate`'s `SM_ERR`; the
    /// mates are one to one). The stage before asked for one entity to have two mates: the
    /// points tell which (two coincident edges of one operand, two pieces of one edge, a
    /// vertex where intersection curves cross).
    RelateConflict {
        /// The operand `entity` is on.
        side: RelateSide,
        /// The entity that has a mate.
        entity: PlacedEntity,
        /// Its mate, on the other operand.
        mate: PlacedEntity,
        /// The entity of the other operand it was asked to be related to.
        asked: PlacedEntity,
    },
}

impl BooleanFailure {
    /// The entity the failure names, if any.
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match *self {
            Self::SolidLeaks { face } => Some(EntityRef::Face(face)),
            Self::StageFailed { .. } | Self::EntityMissing { .. } | Self::RelateConflict { .. } => {
                None
            }
        }
    }

    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::StageFailed { .. } | Self::SolidLeaks { .. } | Self::RelateConflict { .. } => {
                SM_ERR
            }
            Self::EntityMissing { .. } => SM_ERR_NULL_POINTER,
        }
    }

    /// The failure's stable [`ErrorKind`].
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::StageFailed { .. } | Self::SolidLeaks { .. } | Self::RelateConflict { .. } => {
                ErrorKind::Failed
            }
            Self::EntityMissing { .. } => ErrorKind::Missing,
        }
    }
}

impl core::fmt::Display for BooleanFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::StageFailed { stage, what } => write!(f, "{stage} failed: {what}"),
            Self::EntityMissing { stage, what } => {
                write!(f, "{stage} needs {what}, which the B-rep does not have")
            }
            Self::SolidLeaks { face } => write!(
                f,
                "the union would delete face {} and merge the solid it bounds into the outside",
                face.index()
            ),
            Self::RelateConflict {
                side,
                entity,
                mate,
                asked,
            } => write!(
                f,
                "{} failed: {entity} of {side} is related to {mate} and was asked for {asked}: \
                 another entity is already related to the other one",
                BooleanStage::Relate
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a Boolean stage that could not continue, with
    /// status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    pub(crate) fn boolean_stage_failed(
        operation: &'static str,
        stage: BooleanStage,
        what: &'static str,
    ) -> Self {
        super::Failure::new(operation, BooleanFailure::StageFailed { stage, what }).into_error()
    }

    /// [`crate::SolidError::Failed`] for an entity a Boolean stage needs that is not there,
    /// with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a `NULL` pointer).
    pub(crate) fn boolean_entity_missing(
        operation: &'static str,
        stage: BooleanStage,
        what: &'static str,
    ) -> Self {
        super::Failure::new(operation, BooleanFailure::EntityMissing { stage, what }).into_error()
    }

    /// [`crate::SolidError::Failed`] for a relation refused because `entity` already has a
    /// different mate, with status [`SM_ERR`] (upstream `Relate`'s `return SE(SM_ERR)`).
    pub(crate) fn boolean_relate_conflict(
        operation: &'static str,
        side: RelateSide,
        entity: PlacedEntity,
        mate: PlacedEntity,
        asked: PlacedEntity,
    ) -> Self {
        super::Failure::new(
            operation,
            BooleanFailure::RelateConflict {
                side,
                entity,
                mate,
                asked,
            },
        )
        .into_error()
    }
}
