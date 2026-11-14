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
//! | [`OffsetFailure::Collapses`]           | `SM_ERR` (#2350, solidcore-native)        |
//! | [`OffsetFailure::WallsTouch`]          | `SM_ERR_OFFSET_WALLS_TOUCH` (fork a759622) |
//! | [`OffsetFailure::LoopsCross`]          | `SM_ERR_BAD_INTERSECTIONS` (WR-345)       |
//! | [`OffsetFailure::EdgeVanishes`]        | `SM_ERR_OFFSET_EDGE_VANISHES` (fork 34318f4) |
//! | [`OffsetFailure::DistanceUnresolved`]  | `SM_ERR_INVALID_INPUT` (solidcore-native) |
//! | [`OffsetFailure::ToleranceUnresolved`] | `SM_ERR_INVALID_INPUT` (solidcore-native, WR-749) |
//! | [`OffsetFailure::SeamOpened`]          | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::ExactCurveUncut`]     | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::TangentBranchUncertified`] | `SM_ERR` (solidcore-native)            |
//! | [`OffsetFailure::TangentEndOffCurve`]  | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::FacesApart`]          | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::ReachUncertified`]    | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::LoopGapUncertified`]  | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::Unsound`]             | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::Failed`]              | `SM_ERR`                                  |
//! | [`OffsetFailure::Missing`]             | `SM_ERR_NULL_POINTER`                     |
//! | [`OffsetFailure::InvalidValue`]        | `SM_ERR_INVALID_INPUT`                    |
//! | [`OffsetFailure::Inconsistent`]        | `SM_ERR_ASSERT_FAILURE`                   |
//!
//! Deviations: not upstream. Entity ids are those of the operation's input B-rep. The
//! last four describe, in a phrase, the failures of offset routines that returned a bare
//! status ([`OffsetFailure::replaces_upstream_status`]); the failure's `operation` path
//! says which routine and its entity, where the site named one, which face. The files
//! with open work elsewhere (`offset_executive/{p1,p2,step_gaps,reach}`,
//! `offset_faces`, `offset_geometry_creation`) do not use them yet.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_BAD_INTERSECTIONS,
    SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER, SM_ERR_OFFSET_EDGE_VANISHES,
    SM_ERR_OFFSET_WALLS_TOUCH,
};
use crate::topology::{EdgeId, FaceId};

/// Why an offset, shell, thicken or local operation is refused.
///
/// A caller outside the crate matches it with a wildcard: a reason it does not name yet still
/// has its class ([`OffsetFailure::kind`]) and says whether it is a phrase
/// ([`OffsetFailure::is_phrase`]) or a defect ([`OffsetFailure::is_defect`]), which every reason
/// must answer here.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum OffsetFailure {
    /// The offset of `face` by `distance` folds: the wall is thicker than a radius of
    /// curvature of the face on its side (WR-718; every offset but the local operation).
    Folds {
        /// The face whose offset folds.
        face: FaceId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
        /// The largest wall that works.
        largest: f64,
    },
    /// The offset of `face` by `distance` is not proved regular, and no wall size would help:
    /// the face does not bend towards that side anywhere the sizing looked, so no radius of
    /// curvature limits the wall. The proof ran out where the face's normal vanishes (a flat
    /// patch whose boundary curves leave a corner tangent to each other) or out of patches.
    NotCertified {
        /// The face whose offset is not certified.
        face: FaceId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
    },
    /// The offset of `face` by `distance` collapses onto a line or point: the face has a
    /// radius of curvature `radius` on the displaced side that the distance reaches, within
    /// the B-rep's tolerance (a round inset by its radius). The offset is no surface there,
    /// and the faces beside it would have to meet across it, which the offset does not
    /// build (#2350).
    Collapses {
        /// The face whose offset collapses.
        face: FaceId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
        /// The radius of curvature the face attains on the displaced side.
        radius: f64,
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
    /// A local operation gives `face`, which meets itself across edges (seams of its
    /// closed surface), a surface on which a seam's two UV lines (the face's loops are
    /// copied onto it) do not lift to one curve: it is not closed in the seam's direction,
    /// or the lines are not its seam, one period apart. Each moved edge is found again where the new surfaces on its two sides
    /// meet; across a seam both sides are the face's own new surface, which a surface open
    /// across it cannot close onto itself, so the face has no edge there (the topology the
    /// operation keeps cannot be built).
    SeamOpened {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// The extended surfaces of the faces beside an edge meet on an exact curve (an
    /// analytic special case's line or conic) that could not be cut into its pieces inside
    /// both extended domains with their UV curves
    /// ([`crate::surface::surface::SurfaceIntersection::clip_exact_curves`]; the failure's
    /// cause says why). The edge's offsets cannot be mated on it.
    ExactCurveUncut,
    /// In a local operation, an edge's new curve ends beside an edge that was tangent in
    /// the input and whose new surfaces cross, and that edge, already mated, has no new
    /// vertex on the curve: the end is not where the edges meet, and no cut chosen by
    /// distance stands in for it.
    TangentEndOffCurve,
    /// Two curved faces tangent at the old edge now cross, but the intersection
    /// branch on their retained sides has not been certified.
    TangentBranchUncertified,
    /// In a local operation, the new surfaces of `face` and of its neighbour across an
    /// edge meet nowhere near the edge, so no extension of either makes them meet there
    /// and the edge cannot be found again. (The reach,
    /// [`crate::offset::offset_executive::reach::local_extension_reach`], no longer refuses
    /// it: it lists such edges in `LocalReach::apart`, where a step's wall closes the gap
    /// or the edge's offset fails.)
    FacesApart {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// In a local operation, a surface beside an edge or at a vertex of `face` moves, and
    /// how far the surfaces there must be extended to meet has no certified bound
    /// ([`crate::offset::offset_executive::reach::local_extension_reach`]): a pair or corner
    /// of kinds it does not bound, or an edge piece left undecided. Refused before anything
    /// changes.
    ReachUncertified {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// An offset face has a gap in its loops, and the straight segment that would bridge
    /// it could not be established within tolerance (the fork's
    /// `SmOffsetExecutive::CloseLoopGap`). Refused where the bridge construction is
    /// uncertified, before the result's soundness is judged.
    LoopGapUncertified {
        /// The face, of the operation's input, whose offset left the gap, where it comes
        /// from one.
        face: Option<FaceId>,
    },
    /// A local operation built a body that fails what its input did not
    /// ([`crate::topology::brep::validation::soundness`] of both): refused, the input B-rep
    /// left as it came in.
    Unsound {
        /// What the built body fails.
        what: Unsoundness,
    },
    /// An offset routine could not continue, as the phrase says (for example "the face
    /// has no surface").
    Failed(&'static str),
    /// Something an offset routine needs is missing, as the phrase says (for example "the
    /// offset B-rep").
    Missing(&'static str),
    /// A value an offset routine was given is not one it can use, as the phrase says.
    InvalidValue(&'static str),
    /// A consistency check of an offset routine's own data did not hold, as the phrase
    /// says.
    Inconsistent(&'static str),
}

/// What a local operation's or a solid offset's built body fails ([`OffsetFailure::Unsound`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unsoundness {
    /// The check at `SM_LEVEL_2` reports `failures` failure lines more than the input's
    /// check did, counted per class
    /// ([`crate::topology::brep::validation::BrepCheck::failures_beyond`]); the first such
    /// class is test `test_index` of the reporting type `reporting_type` (`None` for a line
    /// without a report: a failed status).
    Check {
        /// The number of failure lines beyond the input's.
        failures: usize,
        /// The first failed report's reporting type and test index.
        first: Option<(i64, u64)>,
    },
    /// The body's pointer check failed, so its geometry was not checked (`AssertValid`
    /// is skipped then).
    GeometryUnchecked,
    /// The input was a manifold solid and the body is not.
    NotManifoldSolid,
    /// The body is a manifold solid whose volume's enclosure lies below 0.
    VolumeNotPositive,
    /// The body is a manifold solid whose volume's enclosure still holds 0 after its
    /// budget: its sign is not proved.
    VolumeUndecided,
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
            Self::Folds { face, .. }
            | Self::NotCertified { face, .. }
            | Self::Collapses { face, .. }
            | Self::SeamOpened { face }
            | Self::FacesApart { face }
            | Self::ReachUncertified { face } => Some(EntityRef::Face(face)),
            Self::WallsTouch { face, .. } | Self::LoopsCross { face, .. } => {
                face.map(EntityRef::Face)
            }
            Self::EdgeVanishes { edge, face } => edge
                .map(EntityRef::Edge)
                .or_else(|| face.map(EntityRef::Face)),
            Self::LoopGapUncertified { face } => face.map(EntityRef::Face),
            Self::ExactCurveUncut
            | Self::TangentEndOffCurve
            | Self::TangentBranchUncertified
            | Self::Unsound { .. }
            | Self::DistanceUnresolved { .. }
            | Self::ToleranceUnresolved { .. }
            | Self::Failed(_)
            | Self::Missing(_)
            | Self::InvalidValue(_)
            | Self::Inconsistent(_) => None,
        }
    }

    /// True for a failure that replaced a bare status an offset routine returned
    /// ([`OffsetFailure::Failed`], [`OffsetFailure::Missing`],
    /// [`OffsetFailure::InvalidValue`], [`OffsetFailure::Inconsistent`]): it answers with
    /// that status as the `Upstream` it replaced did. The refusals replaced none.
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        matches!(
            self,
            Self::Failed(_) | Self::Missing(_) | Self::InvalidValue(_) | Self::Inconsistent(_)
        )
    }

    /// True for a phrase cause ([`OffsetFailure::Failed`], [`OffsetFailure::Missing`],
    /// [`OffsetFailure::InvalidValue`], [`OffsetFailure::Inconsistent`]): a routine's failure
    /// not yet given a cause of its own, told apart only by its words. Its class is its status's
    /// kind; each moves to a cause of its own, as the blend's phrase causes do
    /// ([`crate::error::blend::BlendFailure`]).
    #[must_use]
    pub const fn is_phrase(&self) -> bool {
        match self {
            Self::Folds { .. }
            | Self::NotCertified { .. }
            | Self::Collapses { .. }
            | Self::WallsTouch { .. }
            | Self::LoopsCross { .. }
            | Self::EdgeVanishes { .. }
            | Self::DistanceUnresolved { .. }
            | Self::ToleranceUnresolved { .. }
            | Self::SeamOpened { .. }
            | Self::ExactCurveUncut
            | Self::TangentEndOffCurve
            | Self::TangentBranchUncertified
            | Self::FacesApart { .. }
            | Self::ReachUncertified { .. }
            | Self::LoopGapUncertified { .. }
            | Self::Unsound { .. } => false,
            Self::Failed(_) | Self::Missing(_) | Self::InvalidValue(_) | Self::Inconsistent(_) => {
                true
            }
        }
    }

    /// True for a failure that is the kernel's fault, not a fact of the input: a consistency
    /// check of a routine's own data that did not hold ([`OffsetFailure::Inconsistent`]).
    /// Report it. Said per cause, as its class ([`ErrorKind::Failed`]) does not say it, until
    /// offset failures move onto a class of their own.
    #[must_use]
    pub const fn is_defect(&self) -> bool {
        match self {
            Self::Inconsistent(_) => true,
            Self::Folds { .. }
            | Self::NotCertified { .. }
            | Self::Collapses { .. }
            | Self::WallsTouch { .. }
            | Self::LoopsCross { .. }
            | Self::EdgeVanishes { .. }
            | Self::DistanceUnresolved { .. }
            | Self::ToleranceUnresolved { .. }
            | Self::SeamOpened { .. }
            | Self::ExactCurveUncut
            | Self::TangentEndOffCurve
            | Self::TangentBranchUncertified
            | Self::FacesApart { .. }
            | Self::ReachUncertified { .. }
            | Self::LoopGapUncertified { .. }
            | Self::Unsound { .. }
            | Self::Failed(_)
            | Self::Missing(_)
            | Self::InvalidValue(_) => false,
        }
    }

    /// The status the C++ reports for the refusal (difftest's comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Folds { .. }
            | Self::NotCertified { .. }
            | Self::Collapses { .. }
            | Self::SeamOpened { .. }
            | Self::FacesApart { .. }
            | Self::ReachUncertified { .. }
            | Self::ExactCurveUncut
            | Self::TangentEndOffCurve
            | Self::TangentBranchUncertified
            | Self::LoopGapUncertified { .. }
            | Self::Unsound { .. }
            | Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) => SM_ERR_INVALID_INPUT,
            Self::Inconsistent(_) => SM_ERR_ASSERT_FAILURE,
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
            Self::Folds { .. } | Self::NotCertified { .. } => ErrorKind::OffsetFolds,
            Self::Collapses { .. } => ErrorKind::OffsetCollapses,
            Self::WallsTouch { .. } => ErrorKind::OffsetWallsTouch,
            Self::LoopsCross { .. } => ErrorKind::OffsetLoopsCross,
            Self::EdgeVanishes { .. } => ErrorKind::OffsetEdgeVanishes,
            Self::SeamOpened { .. }
            | Self::FacesApart { .. }
            | Self::ReachUncertified { .. }
            | Self::ExactCurveUncut
            | Self::TangentEndOffCurve
            | Self::TangentBranchUncertified
            | Self::LoopGapUncertified { .. }
            | Self::Unsound { .. } => ErrorKind::Unsupported,
            Self::DistanceUnresolved { .. } | Self::ToleranceUnresolved { .. } => {
                ErrorKind::InvalidInput
            }
            Self::Failed(_) | Self::Missing(_) | Self::InvalidValue(_) | Self::Inconsistent(_) => {
                ErrorKind::of_status(self.comparison_status())
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

impl core::fmt::Display for LoopCrossing {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            LoopCrossing::AtLoopVertex => "they cross at a loop vertex",
            LoopCrossing::MeetAtLoopVertex => "they meet at a loop vertex",
            LoopCrossing::RunAlong => "they run along one another",
            LoopCrossing::SharesEdge => "the face shares an edge with another face",
            LoopCrossing::ThreeAtOnePoint => "three loops cross at one point",
            LoopCrossing::StretchUnclassified => {
                "a stretch of a loop is neither inside nor outside another loop"
            }
            LoopCrossing::LeaveMoreThanOneWay => "the loops leave a crossing more than one way",
            LoopCrossing::LeaveNoWay => "the loops leave a crossing no way",
            LoopCrossing::ArriveMoreThanOneWay => {
                "the loops arrive at a crossing more than one way"
            }
            LoopCrossing::ComesApartAroundHole => "the face comes apart around a hole",
        })
    }
}

impl core::fmt::Display for Unsoundness {
    /// What the body built fails, without the refusal [`OffsetFailure`]'s words add.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Unsoundness::Check { failures, first } => {
                write!(
                    f,
                    "the local operation's body fails its check where its input did not \
                     ({failures} failure lines more"
                )?;
                if let Some((reporting_type, test_index)) = first {
                    write!(f, ", the first test #{test_index} of type {reporting_type}")?;
                }
                f.write_str(")")
            }
            Unsoundness::GeometryUnchecked => f.write_str(
                "the local operation's body fails its pointer check, so its geometry cannot \
                 be checked",
            ),
            Unsoundness::NotManifoldSolid => {
                f.write_str("the body built is not a manifold solid, as its input was")
            }
            Unsoundness::VolumeNotPositive => f.write_str(
                "the local operation's body is a manifold solid whose volume is not positive",
            ),
            Unsoundness::VolumeUndecided => f.write_str(
                "the local operation's body is a manifold solid whose volume's sign could \
                 not be proved",
            ),
        }
    }
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
            Self::NotCertified { face, distance } => write!(
                f,
                "the offset of face {} by {distance} is not proved regular: its parametrisation \
                 defeats the proof, and no radius of curvature limits the wall, so no size is \
                 named",
                face.index()
            ),
            Self::Collapses {
                face,
                distance,
                radius,
            } => write!(
                f,
                "the offset of face {} by {distance} collapses: the face has a radius of \
                 curvature {radius} on that side, which the offset reaches (a round offset by \
                 its radius is a line, not a face; the faces beside it would have to meet \
                 across it, which the offset does not build)",
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
            Self::LoopsCross { face, crossing } => write!(
                f,
                "the loops of {} cross in a way the offset cannot resolve: {crossing}",
                offset_of(face)
            ),
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
            Self::SeamOpened { face } => write!(
                f,
                "face {} meets itself across a seam whose two UV lines do not lift to one \
                 curve on the surface it is given (not closed that way, or the lines not on \
                 its seam one period apart): the seam would have no edge",
                face.index()
            ),
            Self::FacesApart { face } => write!(
                f,
                "the new surfaces of face {} and of a neighbour meet nowhere near their \
                 edge: no extension makes them meet there",
                face.index()
            ),
            Self::ReachUncertified { face } => write!(
                f,
                "a surface beside an edge or at a vertex of face {} moves, and how far the \
                 surfaces there must be extended to meet has no certified bound",
                face.index()
            ),
            Self::Unsound { what } => match what {
                Unsoundness::NotManifoldSolid => write!(f, "{what}: refused"),
                _ => write!(f, "{what}: refused, the input unchanged"),
            },
            Self::LoopGapUncertified { face } => write!(
                f,
                "{} leaves a gap in its loops, and the straight segment that would bridge \
                 it could not be established within tolerance: the gap stays open",
                offset_of(face)
            ),
            Self::ExactCurveUncut => f.write_str(
                "the extended faces beside an edge meet on an exact curve that could not be \
                 cut into its pieces with their UV curves, so the edge's offsets cannot be \
                 mated",
            ),
            Self::TangentBranchUncertified => f.write_str(
                "two curved faces tangent at the old edge now cross, but the intersection \
                 branch on their retained sides has no certificate",
            ),
            Self::TangentEndOffCurve => f.write_str(
                "an edge ends beside a tangent edge whose new faces now cross, and that edge's new \
                 vertex is not on the edge's new curve, so the edge cannot end there",
            ),
            Self::ToleranceUnresolved { tolerance } => write!(
                f,
                "invalid input: offset tolerance {tolerance:e} is nonzero but at or below the \
                 effective zero (1e-12): no B-rep can hold a tolerance that small"
            ),
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::InvalidValue(what) => write!(f, "invalid input: {what}"),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for an offset routine that fails with `failure`.
    #[must_use]
    pub(crate) fn offset(operation: &'static str, failure: OffsetFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for something an offset routine needs that is not
    /// there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)`
    /// on a `NULL` pointer).
    #[must_use]
    pub(crate) fn offset_missing(operation: &'static str, what: &'static str) -> Self {
        Self::offset(operation, OffsetFailure::Missing(what))
    }

    /// The offset failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Offset`].
    #[must_use]
    pub fn offset_failure(&self) -> Option<&OffsetFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Offset(o) => Some(o),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SolidError;

    /// The offset failures that replaced a bare status keep the status, kind, operation
    /// and entity of the `Upstream` they replace; a sample of each converted file
    /// (`offset_executive/crossing_loops`, `offset_executive/cavity`, `draft`).
    #[test]
    fn offset_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let face = Some(EntityRef::Face(FaceId::from_index(3)));
        let samples = [
            (
                "offset::offset_executive::crossing_loops",
                OffsetFailure::Inconsistent(
                    "an edge use's UV trim curve does not span its edge's interval",
                ),
                SM_ERR_ASSERT_FAILURE,
                None,
            ),
            (
                "offset::offset_executive::crossing_loops",
                OffsetFailure::Missing("an entity or curve of the offset face"),
                SM_ERR_NULL_POINTER,
                None,
            ),
            (
                "offset::offset_executive::cavity::is_cavity_region",
                OffsetFailure::Missing("the face's surface"),
                SM_ERR_NULL_POINTER,
                None,
            ),
            (
                "offset::draft",
                OffsetFailure::InvalidValue("the face has no surface or no outer face use"),
                SM_ERR_INVALID_INPUT,
                face,
            ),
            ("offset::op", OffsetFailure::Failed("x"), SM_ERR, None),
        ];
        for (operation, failure, code, entity) in samples {
            assert!(failure.replaces_upstream_status());
            let typed = match entity {
                Some(e) => Failure::new(operation, failure).at(e).into_error(),
                None => SolidError::offset(operation, failure),
            };
            let before = SolidError::Upstream {
                operation,
                code,
                entity,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(super::super::upstream_status(&typed), Some(code));
            assert_eq!(typed.kind(), before.kind(), "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), before.entity(), "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(
                typed.is_unported_or_cancelled(),
                before.is_unported_or_cancelled()
            );
            assert_eq!(typed.offset_failure(), Some(&failure));
        }
        assert_eq!(
            Failure::new(
                "offset::draft",
                OffsetFailure::InvalidValue("the face has no surface or no outer face use"),
            )
            .at(EntityRef::Face(FaceId::from_index(3)))
            .into_error()
            .to_string(),
            "offset::draft: invalid input: the face has no surface or no outer face use (face 3)"
        );
        assert_eq!(
            SolidError::offset_missing("offset::op", "the face's surface").to_string(),
            "offset::op: the face's surface is missing"
        );
        // a refusal replaced no status and does not answer as one
        let folds = SolidError::offset(
            "offset::op",
            OffsetFailure::Folds {
                face: FaceId::from_index(1),
                distance: 2.0,
                largest: 1.0,
            },
        );
        assert_eq!(folds.upstream_status(), None);
        assert_eq!(SolidError::sm_err("offset::op").offset_failure(), None);
    }

    /// Every offset failure's class, and whether it is a phrase (a routine's failure not yet
    /// given a cause of its own) or a defect of the kernel, pinned. A new reason must answer
    /// all three (`kind`, `is_phrase` and `is_defect` match exhaustively) and adds its arm to
    /// `row` and its row here, so a caller outside the crate, which has to match with a
    /// wildcard, still gets its class and never only its text (#15035).
    #[test]
    fn every_offset_failure_keeps_its_class() {
        use OffsetFailure as O;
        let face = FaceId::from_index(1);
        // (the failure, its class, a phrase, a defect)
        let pinned = [
            (
                O::Folds {
                    face,
                    distance: 2.0,
                    largest: 1.0,
                },
                ErrorKind::OffsetFolds,
                false,
                false,
            ),
            (
                O::NotCertified {
                    face,
                    distance: 2.0,
                },
                ErrorKind::OffsetFolds,
                false,
                false,
            ),
            (
                O::Collapses {
                    face,
                    distance: -5.0,
                    radius: 5.0,
                },
                ErrorKind::OffsetCollapses,
                false,
                false,
            ),
            (
                O::WallsTouch {
                    face: None,
                    contact: WallContact::Touch,
                },
                ErrorKind::OffsetWallsTouch,
                false,
                false,
            ),
            (
                O::LoopsCross {
                    face: None,
                    crossing: LoopCrossing::SharesEdge,
                },
                ErrorKind::OffsetLoopsCross,
                false,
                false,
            ),
            (
                O::EdgeVanishes {
                    edge: None,
                    face: None,
                },
                ErrorKind::OffsetEdgeVanishes,
                false,
                false,
            ),
            (
                O::DistanceUnresolved {
                    distance: 1e-9,
                    resolution: 1e-6,
                },
                ErrorKind::InvalidInput,
                false,
                false,
            ),
            (
                O::ToleranceUnresolved { tolerance: 1e-13 },
                ErrorKind::InvalidInput,
                false,
                false,
            ),
            (O::SeamOpened { face }, ErrorKind::Unsupported, false, false),
            (O::ExactCurveUncut, ErrorKind::Unsupported, false, false),
            (O::TangentEndOffCurve, ErrorKind::Unsupported, false, false),
            (O::FacesApart { face }, ErrorKind::Unsupported, false, false),
            (
                O::ReachUncertified { face },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (
                O::LoopGapUncertified { face: None },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (
                O::Unsound {
                    what: Unsoundness::NotManifoldSolid,
                },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (O::Failed("x"), ErrorKind::Failed, true, false),
            (O::Missing("x"), ErrorKind::Missing, true, false),
            (O::InvalidValue("x"), ErrorKind::InvalidInput, true, false),
            // a consistency check of the kernel's own data that did not hold: its fault
            (O::Inconsistent("x"), ErrorKind::Failed, true, true),
            (
                O::TangentBranchUncertified,
                ErrorKind::Unsupported,
                false,
                false,
            ),
        ];
        let row = |o: &O| -> usize {
            match o {
                O::Folds { .. } => 0,
                O::NotCertified { .. } => 1,
                O::Collapses { .. } => 2,
                O::WallsTouch { .. } => 3,
                O::LoopsCross { .. } => 4,
                O::EdgeVanishes { .. } => 5,
                O::DistanceUnresolved { .. } => 6,
                O::ToleranceUnresolved { .. } => 7,
                O::SeamOpened { .. } => 8,
                O::ExactCurveUncut => 9,
                O::TangentEndOffCurve => 10,
                O::FacesApart { .. } => 11,
                O::ReachUncertified { .. } => 12,
                O::LoopGapUncertified { .. } => 13,
                O::Unsound { .. } => 14,
                O::Failed(_) => 15,
                O::Missing(_) => 16,
                O::InvalidValue(_) => 17,
                O::Inconsistent(_) => 18,
                O::TangentBranchUncertified => 19,
            }
        };
        for (i, (o, class, phrase, defect)) in pinned.iter().enumerate() {
            assert_eq!(row(o), i, "{o:?} is listed once, in order");
            assert_eq!(o.kind(), *class, "{o:?}");
            assert_eq!(o.is_phrase(), *phrase, "{o:?}");
            assert_eq!(o.is_defect(), *defect, "{o:?}");
            // a phrase is what replaced a bare status, and only a phrase
            assert_eq!(o.replaces_upstream_status(), *phrase, "{o:?}");
        }
    }

    /// The offset routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still
    /// hold) whose conversion waits on other open work.
    #[test]
    fn the_offset_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting(
            "offset",
            20,
            &[
                ("offset/offset_executive/p1.rs", 9),
                ("offset/offset_executive/p2.rs", 9),
                ("offset/offset_executive/reach.rs", 1),
                ("offset/offset_executive/step_gaps.rs", 1),
                ("offset/offset_faces.rs", 4),
                ("offset/offset_geometry_creation.rs", 17),
            ],
        );
    }
}
