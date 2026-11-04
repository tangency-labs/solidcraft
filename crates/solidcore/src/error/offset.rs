//! Why an offset, shell, thicken or local operation is refused, in solidcore's terms.
//!
//! Upstream: none. These describe the refusals of the offset subsystem
//! (`crate::offset`). Each also records, for difftest only, the status the C++ (or the
//! SMLib fork the refusal comes from) reports for it (crate-private
//! `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                   | Status difftest compares                  |
//! |----------------------------------------|-------------------------------------------|
//! | [`OffsetFailure::Folds`]               | `SM_ERR` (WR-718, solidcore-native)       |
//! | [`OffsetFailure::WallsTouch`]          | `SM_ERR_OFFSET_WALLS_TOUCH` (fork a759622) |
//! | [`OffsetFailure::LoopsCross`]          | `SM_ERR_BAD_INTERSECTIONS` (WR-345)       |
//! | [`OffsetFailure::EdgeVanishes`]        | `SM_ERR_OFFSET_EDGE_VANISHES` (fork 34318f4) |
//! | [`OffsetFailure::DistanceUnresolved`]  | `SM_ERR_INVALID_INPUT` (solidcore-native) |
//! | [`OffsetFailure::ToleranceUnresolved`] | `SM_ERR_INVALID_INPUT` (solidcore-native, WR-749) |
//!
//! Deviations: not upstream. Entity ids are those of the operation's input B-rep.

use super::{
    EntityRef, ErrorKind, SM_ERR, SM_ERR_BAD_INTERSECTIONS, SM_ERR_INVALID_INPUT,
    SM_ERR_OFFSET_EDGE_VANISHES, SM_ERR_OFFSET_WALLS_TOUCH,
};
use crate::topology::{EdgeId, FaceId};

/// Why an offset, shell, thicken or local operation is refused.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum OffsetFailure {
    /// The offset of `face` by `distance` folds: the wall is thicker than a radius of
    /// curvature of the face on its side (opt-in `fixes.shell_one_construction`).
    Folds {
        /// The face whose offset folds.
        face: FaceId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
        /// The largest wall that works.
        largest: f64,
    },
    /// Loops of the offset of `face` touch without crossing, so the offset walls they
    /// come from meet there (opt-in `fixes.offset_walls_touch_refused`).
    WallsTouch {
        /// The face whose offset's loops touch, when known.
        face: Option<FaceId>,
        /// How they touch.
        contact: WallContact,
    },
    /// Loops of the offset of `face` cross in a way the offset does not resolve (opt-in
    /// `fixes.offset_resolve_crossing_loops`).
    LoopsCross {
        /// The face whose offset's loops cross, when known.
        face: Option<FaceId>,
        /// How they cross.
        crossing: LoopCrossing,
    },
    /// The offset moves a vertex along an edge to or past the edge's other end: the
    /// edge would vanish, a change of topology the offset does not make.
    EdgeVanishes {
        /// The edge, when known.
        edge: Option<EdgeId>,
        /// A face the edge bounds, when the edge itself is not known.
        face: Option<FaceId>,
    },
    /// The offset distance is not a finite length at least the input B-rep's tolerance,
    /// the distance below which the model does not tell two points apart: an offset
    /// that close lies within tolerance of the face it comes from.
    DistanceUnresolved {
        /// The offset distance asked for (its magnitude, as the executive takes it).
        distance: f64,
        /// The smallest distance the input resolves: its B-rep tolerance.
        resolution: f64,
    },
    /// The offset B-rep's tolerance would be nonzero but at or below the effective zero
    /// (`SM_EFF_ZERO`, 1e-12): a tolerance no B-rep can hold (`ValidatePointers` test 0
    /// needs more), as double arithmetic places edges and vertices farther apart than
    /// that (WR-749). It is the magnitude of the member approximation tolerance, or, for
    /// the static and API offsets, the source's B-rep tolerance.
    ToleranceUnresolved {
        /// The tolerance asked for, as given (the executive takes its magnitude).
        tolerance: f64,
    },
}

/// How loops of an offset face touch without crossing ([`OffsetFailure::WallsTouch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WallContact {
    /// They run along one another.
    RunAlong,
    /// They meet without crossing (tangentially, or coming up to one another and going
    /// back on the same side).
    Touch,
}

/// How loops of an offset face cross in a way the offset does not resolve
/// ([`OffsetFailure::LoopsCross`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoopCrossing {
    /// They cross at a vertex of a loop.
    AtLoopVertex,
    /// They meet at a vertex of a loop.
    MeetAtLoopVertex,
    /// They run along one another.
    RunAlong,
    /// The face shares an edge with another face.
    SharesEdge,
    /// Three loops cross at one point.
    ThreeAtOnePoint,
    /// A stretch of a loop is neither inside nor outside another loop.
    StretchUnclassified,
    /// The loops leave a crossing more than one way.
    LeaveMoreThanOneWay,
    /// The loops leave a crossing no way.
    LeaveNoWay,
    /// The loops arrive at a crossing more than one way.
    ArriveMoreThanOneWay,
    /// The face comes apart around a hole.
    ComesApartAroundHole,
}

impl OffsetFailure {
    /// The entity the refusal names: the face, or the edge (else its face).
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match *self {
            Self::Folds { face, .. } => Some(EntityRef::Face(face)),
            Self::WallsTouch { face, .. } | Self::LoopsCross { face, .. } => {
                face.map(EntityRef::Face)
            }
            Self::EdgeVanishes { edge, face } => edge
                .map(EntityRef::Edge)
                .or_else(|| face.map(EntityRef::Face)),
            Self::DistanceUnresolved { .. } | Self::ToleranceUnresolved { .. } => None,
        }
    }

    /// The status the C++ reports for the refusal (difftest's comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Folds { .. } => SM_ERR,
            Self::WallsTouch { .. } => SM_ERR_OFFSET_WALLS_TOUCH,
            Self::LoopsCross { .. } => SM_ERR_BAD_INTERSECTIONS,
            Self::EdgeVanishes { .. } => SM_ERR_OFFSET_EDGE_VANISHES,
            Self::DistanceUnresolved { .. } | Self::ToleranceUnresolved { .. } => {
                SM_ERR_INVALID_INPUT
            }
        }
    }

    /// The refusal's stable [`ErrorKind`].
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Folds { .. } => ErrorKind::OffsetFolds,
            Self::WallsTouch { .. } => ErrorKind::OffsetWallsTouch,
            Self::LoopsCross { .. } => ErrorKind::OffsetLoopsCross,
            Self::EdgeVanishes { .. } => ErrorKind::OffsetEdgeVanishes,
            Self::DistanceUnresolved { .. } | Self::ToleranceUnresolved { .. } => {
                ErrorKind::InvalidInput
            }
        }
    }

    /// The same refusal about `face` where it names no face yet (the face of the input
    /// an offset face comes from, known only to the caller of the stage that refused).
    #[must_use]
    pub const fn on_face(self, face: FaceId) -> Self {
        match self {
            Self::WallsTouch {
                face: None,
                contact,
            } => Self::WallsTouch {
                face: Some(face),
                contact,
            },
            Self::LoopsCross {
                face: None,
                crossing,
            } => Self::LoopsCross {
                face: Some(face),
                crossing,
            },
            Self::EdgeVanishes {
                edge: None,
                face: None,
            } => Self::EdgeVanishes {
                edge: None,
                face: Some(face),
            },
            other => other,
        }
    }
}

/// "the offset of face 3" or "an offset face".
fn offset_of(face: Option<FaceId>) -> String {
    face.map_or_else(
        || "an offset face".to_owned(),
        |f| format!("the offset of face {}", f.index()),
    )
}

impl core::fmt::Display for OffsetFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Folds {
                face,
                distance,
                largest,
            } => write!(
                f,
                "the offset of face {} by {distance} folds: the wall is thicker than a \
                 radius of curvature of the face on that side (the largest wall that works \
                 is {largest})",
                face.index()
            ),
            Self::WallsTouch { face, contact } => {
                let how = match contact {
                    WallContact::RunAlong => "run along one another",
                    WallContact::Touch => "touch without crossing",
                };
                write!(
                    f,
                    "the loops of {} {how}, so the offset walls would meet there",
                    offset_of(face)
                )
            }
            Self::LoopsCross { face, crossing } => {
                let how = match crossing {
                    LoopCrossing::AtLoopVertex => "they cross at a loop vertex",
                    LoopCrossing::MeetAtLoopVertex => "they meet at a loop vertex",
                    LoopCrossing::RunAlong => "they run along one another",
                    LoopCrossing::SharesEdge => "the face shares an edge with another face",
                    LoopCrossing::ThreeAtOnePoint => "three loops cross at one point",
                    LoopCrossing::StretchUnclassified => {
                        "a stretch of a loop is neither inside nor outside another loop"
                    }
                    LoopCrossing::LeaveMoreThanOneWay => {
                        "the loops leave a crossing more than one way"
                    }
                    LoopCrossing::LeaveNoWay => "the loops leave a crossing no way",
                    LoopCrossing::ArriveMoreThanOneWay => {
                        "the loops arrive at a crossing more than one way"
                    }
                    LoopCrossing::ComesApartAroundHole => "the face comes apart around a hole",
                };
                write!(
                    f,
                    "the loops of {} cross in a way the offset cannot resolve: {how}",
                    offset_of(face)
                )
            }
            Self::EdgeVanishes { edge, face } => {
                let which = match (edge, face) {
                    (Some(e), _) => format!("edge {}", e.index()),
                    (None, Some(face)) => format!("an edge of face {}", face.index()),
                    (None, None) => "an edge".to_owned(),
                };
                write!(
                    f,
                    "the offset moves a vertex of {which} to or past the edge's other end: \
                     the edge would vanish, a change of topology the offset does not make"
                )
            }
            Self::DistanceUnresolved {
                distance,
                resolution,
            } => write!(
                f,
                "invalid input: offset distance {distance:e} is not a finite length of at \
                 least {resolution:e}, the B-rep tolerance (the smallest distance the model \
                 resolves)"
            ),
            Self::ToleranceUnresolved { tolerance } => write!(
                f,
                "invalid input: offset tolerance {tolerance:e} is nonzero but at or below the \
                 effective zero (1e-12): no B-rep can hold a tolerance that small"
            ),
        }
    }
}
