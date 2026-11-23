//! Why a sweep or loft refuses, in solidcore's terms (#3140).
//!
//! Upstream: none. A railed loft ([`crate::sweep::loft_rails`]) certifies each guided edge
//! within the caller's tolerance of its rail, adding stations where the bound is above it; past
//! the caller's most wires it refuses and names the bound reached. The other failures describe
//! the sweep subsystem's routines (`crate::sweep`: the topology sweep and its translational and
//! rotational geometry, the screw sweep, the cornered and segmented sweeps and the sweep API's
//! surfaces), each recording, for difftest only, the status the C++ reports for it
//! (crate-private `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                  | Status difftest compares |
//! |---------------------------------------|--------------------------|
//! | [`SweepFailure::RailPastStationCap`]  | `SM_ERR`                 |
//! | [`SweepFailure::LoftSectionsTooLarge`] | `SM_ERR`                |
//! | [`SweepFailure::LoftMatchTooLarge`]   | `SM_ERR`                 |
//! | [`SweepFailure::NullTarget`]          | `SM_ERR_NULL_POINTER`    |
//! | [`SweepFailure::NullProfileCurve`]    | `SM_ERR_NULL_POINTER`    |
//! | [`SweepFailure::EdgeWithoutCurve`]    | `SM_ERR_NULL_POINTER` (or the site's) |
//! | [`SweepFailure::FaceWithoutSurface`]  | `SM_ERR_NULL_POINTER`    |
//! | [`SweepFailure::BrepWithoutInfiniteRegion`] | `SM_ERR_NULL_POINTER` |
//! | [`SweepFailure::UntaggedSilhouetteEdge`] | `SM_ERR_NULL_POINTER` |
//! | [`SweepFailure::FaceCopyByFullTurn`]  | `SM_ERR_NULL_POINTER`    |
//! | [`SweepFailure::UntaggedFace`]        | `SM_ERR_NULL_POINTER`    |
//! | [`SweepFailure::UntaggedEdge`]        | `SM_ERR_NULL_POINTER`    |
//! | [`SweepFailure::MergeOtherBodyConsumed`] | `SM_ERR_NULL_POINTER` |
//! | [`SweepFailure::SweptEdgeCurveDropped`] | `SM_ERR_NULL_POINTER`  |
//! | [`SweepFailure::Defect`]              | `SM_ERR` (or the site's, through `with_status`) |
//! | every other cause                     | `SM_ERR` (or the site's, through `with_status`) |
//!
//! [`SweepFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). Every other cause names what
//! happened, with its class (the conversion guide: `docs/failure-codes-and-reproduction.md`
//! §6).
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. The failures
//! that are checks of a caller's arguments the routines made before this record stay
//! [`crate::SolidError::InvalidInput`].

use super::{EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_NULL_POINTER};
use crate::topology::{EdgeId, FaceId};

/// What a [`SweepFailure::MergeOtherBodyConsumed`] says.
pub(crate) const MERGE_OTHER_CONSUMED: &str =
    "the merge's other body was consumed by an earlier union or merge";

/// The sweep record both entities a stitch would merge already carry
/// ([`SweepFailure::MergeOfTwoSweptEntities`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SweepRecord {
    /// Each has a swept image of its own dimension (`GetToSame`).
    SweptImage,
    /// Each is the image of a swept entity (`GetFromSame`).
    ImageOfSwept,
    /// Each comes from sweeping an entity of one dimension lower (`GetFromLower`).
    FromLowerSweep,
    /// Each has a swept image of one dimension higher (`GetToHigher`).
    HigherSweptImage,
}

/// The kind of a topology sweep entity ([`SweepFailure::SweepImageOfOtherKind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SweptKind {
    /// A vertex.
    Vertex,
    /// An edge.
    Edge,
    /// A face.
    Face,
    /// A region.
    Region,
}

/// The step on a profile curve that failed ([`SweepFailure::ProfileCurveStepFailed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProfileCurveStep {
    /// Its canonical NURBS form.
    Nurbs,
    /// Its copy for a cap.
    Copy,
    /// Its placement at a cap.
    Place,
}

/// Why a sweep or loft refuses.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SweepFailure {
    /// A railed loft's guided edges were asked within `asked` of their rails, and at the most
    /// wires asked, `wires`, the bound proved was still `reached`.
    RailPastStationCap {
        /// The tolerance asked.
        asked: f64,
        /// The largest bound proved between a rail and the skin at the cap.
        reached: f64,
        /// The wires (sections and stations) at the cap.
        wires: usize,
    },
    /// Two loft sections too large to pair: the band their pairing is searched in
    /// ([`crate::sweep::loft_sections`]) would have no room within its cells, with `first` and
    /// `second` segments. Sections of equal counts up to `largest` segments pair.
    LoftSectionsTooLarge {
        /// The first section's segments.
        first: usize,
        /// The second section's segments.
        second: usize,
        /// The most segments two sections of equal counts may have.
        largest: usize,
    },
    /// Loft sections whose match ([`crate::sweep::loft_sections`]) would give a section
    /// `segments` segments, more than the caller's bound `most`.
    LoftMatchTooLarge {
        /// The segments the section would get (at least).
        segments: usize,
        /// The caller's bound.
        most: usize,
    },
    /// The caller's sweep targets name a null entity: an entry of a target array, or the
    /// B-rep to sweep. [`ErrorKind::InvalidInput`].
    NullTarget,
    /// A profile curve the caller passed is null. [`ErrorKind::InvalidInput`].
    NullProfileCurve,
    /// An edge of the caller's profile has no curve. [`ErrorKind::InvalidInput`].
    EdgeWithoutCurve {
        /// The edge.
        edge: EdgeId,
    },
    /// A face of the caller's profile has no surface. [`ErrorKind::InvalidInput`].
    FaceWithoutSurface {
        /// The face.
        face: FaceId,
    },
    /// A B-rep the sweep reads or builds into has no infinite region (a B-rep made without
    /// one). [`ErrorKind::InvalidInput`].
    BrepWithoutInfiniteRegion {
        /// The B-rep.
        brep: EntityRef,
    },
    /// A tagging sweep of a whole B-rep copies each swept edge's tag to its images, and an
    /// edge of the silhouette it sweeps has none: the silhouette's edges come from the
    /// hidden-curve scene's faces, which carry no tags, so tagging them is not supported.
    /// [`ErrorKind::Unsupported`].
    UntaggedSilhouetteEdge {
        /// The edge.
        edge: EdgeId,
    },
    /// A face to copy by a rotation of a full turn: its image is the face itself, so the
    /// rotation gives no surface to copy. [`ErrorKind::InvalidInput`].
    FaceCopyByFullTurn,
    /// A face given to a tagging face copy (`TopologySweep::sweep_face_same` with tagging on)
    /// has no tag: `tag_profile` was not run on it. [`ErrorKind::InvalidInput`].
    UntaggedFace {
        /// The face.
        face: FaceId,
    },
    /// An edge given to a tagging edge sweep (`TopologySweep::sweep_edges_higher` with
    /// tagging on) has no tag: `tag_profile` was not run on it. [`ErrorKind::InvalidInput`].
    /// (An untagged silhouette edge of a whole-B-rep sweep is
    /// [`SweepFailure::UntaggedSilhouetteEdge`].)
    UntaggedEdge {
        /// The edge.
        edge: EdgeId,
    },
    /// A store call on a sweep merge whose other body an earlier union or merge consumed
    /// (deleted, or merged into the primary): the caller's use of a spent merge, as an
    /// unknown or released merge is. [`ErrorKind::InvalidInput`].
    MergeOtherBodyConsumed,
    /// A same-dimension image recorded in the sweep's maps (which a caller can fill through
    /// `TopologySweep::set_same` or the public maps) is of another kind than the entity it
    /// is the image of. [`ErrorKind::InvalidInput`].
    SweepImageOfOtherKind {
        /// The recorded image.
        image: EntityRef,
        /// The kind it should have: its original's.
        expected: SweptKind,
        /// The kind it has.
        found: SweptKind,
    },
    /// A face the caller asked to sweep was already swept in this sweep (it is listed
    /// twice, or as the image of another). [`ErrorKind::InvalidInput`].
    FaceAlreadySwept,
    /// The sweep moves an entity `moved`, less than the `least` its tolerance needs
    /// (twice the tolerance for a translation, the vertex tolerance for a rotation).
    /// [`ErrorKind::InvalidInput`].
    SweepTooShort {
        /// How far the sweep moves the entity.
        moved: f64,
        /// The least move its tolerance needs.
        least: f64,
    },
    /// A repeated rotation turns `degrees` in all, more than a full turn.
    /// [`ErrorKind::InvalidInput`].
    RotationPastFullTurn {
        /// The degrees turned.
        degrees: f64,
    },
    /// A point to rotate is `distance` from the rotation axis, within the `least` that
    /// gives it a direction away from the axis. [`ErrorKind::InvalidInput`].
    PointOnRotationAxis {
        /// The point's distance from the axis.
        distance: f64,
        /// The least distance that gives a direction.
        least: f64,
    },
    /// A profile edge crosses the rotation axis: its curve meets the axis away from its
    /// ends, or runs along only part of the axis between its ends.
    /// [`ErrorKind::InvalidInput`].
    EdgeCrossesRotationAxis,
    /// An edge's vertex is `distance` from the end of its curve (or of the surface swept
    /// from it), more than the `tolerance` the sweep matches them within.
    /// [`ErrorKind::InvalidInput`].
    EdgeEndsOffCurve {
        /// The distance between the vertex and the curve's end.
        distance: f64,
        /// The tolerance.
        tolerance: f64,
    },
    /// A base edge to make in a face has a curve whose two ends are within tolerance of one
    /// face vertex. [`ErrorKind::InvalidInput`].
    BaseCurveEndsCoincide,
    /// Two vertices of the face a base edge is made in are within tolerance of one end of
    /// the edge's curve. [`ErrorKind::InvalidInput`].
    FaceVerticesCoincide,
    /// A profile curve's NURBS form has `dim` coordinates, not three.
    /// [`ErrorKind::InvalidInput`].
    ProfileCurveNotThreeDimensional {
        /// Its dimension.
        dim: u64,
    },
    /// The base sweep geometry, which only its translational and rotational kinds extend,
    /// builds no geometry and has no copy. [`ErrorKind::Unsupported`].
    BaseGeometryBuildsNothing,
    /// A curve the sweep needs as a B-spline is of a kind with no B-spline form.
    /// [`ErrorKind::Unsupported`].
    CurveWithoutBSplineForm {
        /// The curve's edge, where it has one.
        entity: Option<EntityRef>,
    },
    /// Both entities a stitch would merge already carry the same sweep record, so neither
    /// can be kept for the other. [`ErrorKind::Unsupported`].
    MergeOfTwoSweptEntities {
        /// The record both carry.
        record: SweepRecord,
    },
    /// A profile line perpendicular to the rotation axis within the angle tolerance whose
    /// ends' feet on the axis are `apart`, more than `eps`: the plane it sweeps is not
    /// defined. [`ErrorKind::Unsupported`].
    NearlyPerpendicularLine {
        /// The distance between the ends' feet on the axis.
        apart: f64,
        /// The tolerance.
        eps: f64,
    },
    /// The swept body is not a closed manifold solid. [`ErrorKind::Unsupported`].
    SweptBodyNotManifoldSolid,
    /// The plane a line perpendicular to the axis sweeps and the revolution of that line
    /// have normals whose cosine is `dot`, below 0.8 in size, at their domains' first
    /// corners. [`ErrorKind::Degenerate`].
    SweptSurfaceNormalsDisagree {
        /// The cosine between the normals.
        dot: f64,
    },
    /// The edge's curve is tangent to the translation somewhere, where the surface it
    /// sweeps degenerates. [`ErrorKind::Degenerate`].
    CurveTangentToSweep,
    /// The cylinder piece a profile line parallel to the screw axis sweeps built `faces`
    /// faces, not one: the line is too short to bound a face. [`ErrorKind::Degenerate`].
    CylinderPieceNotOneFace {
        /// The faces it built.
        faces: usize,
    },
    /// The surface an edge sweeps has no boundary curves left to trim it.
    /// [`ErrorKind::Degenerate`].
    SweptFaceWithoutBoundary,
    /// The face builder dropped the swept edge's own curve as degenerate or coincident
    /// with another. [`ErrorKind::Degenerate`].
    SweptEdgeCurveDropped,
    /// The revolved curve sweeps no face (a curve along the axis sweeps none).
    /// [`ErrorKind::Degenerate`].
    RevolutionSweepsNoFace,
    /// A step on a profile curve failed; the error it chains says why.
    /// [`ErrorKind::Failed`].
    ProfileCurveStepFailed {
        /// The step.
        step: ProfileCurveStep,
    },
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
}

impl SweepFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR_NULL_POINTER` for
    /// a null or missing input, `SM_ERR` otherwise.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::NullTarget
            | Self::NullProfileCurve
            | Self::EdgeWithoutCurve { .. }
            | Self::FaceWithoutSurface { .. }
            | Self::BrepWithoutInfiniteRegion { .. }
            | Self::UntaggedSilhouetteEdge { .. }
            | Self::FaceCopyByFullTurn
            | Self::UntaggedFace { .. }
            | Self::UntaggedEdge { .. }
            | Self::MergeOtherBodyConsumed
            | Self::SweptEdgeCurveDropped => SM_ERR_NULL_POINTER,
            Self::RailPastStationCap { .. }
            | Self::LoftSectionsTooLarge { .. }
            | Self::LoftMatchTooLarge { .. }
            | Self::FaceAlreadySwept
            | Self::SweepImageOfOtherKind { .. }
            | Self::SweepTooShort { .. }
            | Self::RotationPastFullTurn { .. }
            | Self::PointOnRotationAxis { .. }
            | Self::EdgeCrossesRotationAxis
            | Self::EdgeEndsOffCurve { .. }
            | Self::BaseCurveEndsCoincide
            | Self::FaceVerticesCoincide
            | Self::ProfileCurveNotThreeDimensional { .. }
            | Self::BaseGeometryBuildsNothing
            | Self::CurveWithoutBSplineForm { .. }
            | Self::MergeOfTwoSweptEntities { .. }
            | Self::NearlyPerpendicularLine { .. }
            | Self::SweptBodyNotManifoldSolid
            | Self::SweptSurfaceNormalsDisagree { .. }
            | Self::CurveTangentToSweep
            | Self::CylinderPieceNotOneFace { .. }
            | Self::SweptFaceWithoutBoundary
            | Self::RevolutionSweepsNoFace
            | Self::ProfileCurveStepFailed { .. }
            | Self::Defect { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`], as each variant's doc says.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::RailPastStationCap { .. } => ErrorKind::NotWithinTolerance,
            Self::NullTarget
            | Self::NullProfileCurve
            | Self::EdgeWithoutCurve { .. }
            | Self::FaceWithoutSurface { .. }
            | Self::BrepWithoutInfiniteRegion { .. }
            | Self::FaceCopyByFullTurn
            | Self::UntaggedFace { .. }
            | Self::UntaggedEdge { .. }
            | Self::MergeOtherBodyConsumed
            | Self::FaceAlreadySwept
            | Self::SweepImageOfOtherKind { .. }
            | Self::SweepTooShort { .. }
            | Self::RotationPastFullTurn { .. }
            | Self::PointOnRotationAxis { .. }
            | Self::EdgeCrossesRotationAxis
            | Self::EdgeEndsOffCurve { .. }
            | Self::BaseCurveEndsCoincide
            | Self::FaceVerticesCoincide
            | Self::ProfileCurveNotThreeDimensional { .. } => ErrorKind::InvalidInput,
            Self::LoftSectionsTooLarge { .. }
            | Self::LoftMatchTooLarge { .. }
            | Self::BaseGeometryBuildsNothing
            | Self::CurveWithoutBSplineForm { .. }
            | Self::MergeOfTwoSweptEntities { .. }
            | Self::NearlyPerpendicularLine { .. }
            | Self::UntaggedSilhouetteEdge { .. }
            | Self::SweptBodyNotManifoldSolid => ErrorKind::Unsupported,
            Self::SweptSurfaceNormalsDisagree { .. }
            | Self::CurveTangentToSweep
            | Self::CylinderPieceNotOneFace { .. }
            | Self::SweptFaceWithoutBoundary
            | Self::SweptEdgeCurveDropped
            | Self::RevolutionSweepsNoFace => ErrorKind::Degenerate,
            Self::ProfileCurveStepFailed { .. } => ErrorKind::Failed,
            Self::Defect { .. } => ErrorKind::StageFailed,
        }
    }

    /// True for a failure that replaced a C++ status the routine returns (every cause but
    /// a loft's own refusals): it answers with that status the way the `Upstream` it
    /// replaced did ([`crate::error::upstream_status`]). A loft's refusal is solidcore's own
    /// and replaced none.
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        !matches!(
            self,
            Self::RailPastStationCap { .. }
                | Self::LoftSectionsTooLarge { .. }
                | Self::LoftMatchTooLarge { .. }
        )
    }
}

impl core::fmt::Display for SweepRecord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::SweptImage => "a swept image",
            Self::ImageOfSwept => "the record of being a swept entity's image",
            Self::FromLowerSweep => "the record of coming from a lower-dimension sweep",
            Self::HigherSweptImage => "a higher-dimension swept image",
        })
    }
}

impl core::fmt::Display for SweptKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Vertex => "vertex",
            Self::Edge => "edge",
            Self::Face => "face",
            Self::Region => "region",
        })
    }
}

impl SweptKind {
    /// The kind's name with its indefinite article ("a vertex", "an edge").
    const fn with_article(self) -> &'static str {
        match self {
            Self::Vertex => "a vertex",
            Self::Edge => "an edge",
            Self::Face => "a face",
            Self::Region => "a region",
        }
    }
}

impl core::fmt::Display for ProfileCurveStep {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Nurbs => "NURBS form",
            Self::Copy => "copy for a cap",
            Self::Place => "placement at a cap",
        })
    }
}

impl core::fmt::Display for SweepFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::NullTarget => f.write_str("the sweep's targets name a null entity"),
            Self::NullProfileCurve => f.write_str("a profile curve is null"),
            Self::EdgeWithoutCurve { edge } => write!(f, "edge {} has no curve", edge.index()),
            Self::FaceWithoutSurface { face } => {
                write!(f, "face {} has no surface", face.index())
            }
            Self::BrepWithoutInfiniteRegion { brep } => {
                write!(f, "{brep} has no infinite region")
            }
            Self::UntaggedSilhouetteEdge { edge } => write!(
                f,
                "silhouette edge {} has no tag to copy: tagging a whole B-rep's silhouette is not supported",
                edge.index()
            ),
            Self::FaceCopyByFullTurn => f.write_str(
                "a rotation of a full turn copies no face: the face's image is the face itself",
            ),
            Self::UntaggedFace { face } => write!(
                f,
                "face {} has no tag to copy: tag the sweep's targets before a tagging sweep",
                face.index()
            ),
            Self::UntaggedEdge { edge } => write!(
                f,
                "edge {} has no tag to copy: tag the sweep's targets before a tagging sweep",
                edge.index()
            ),
            Self::MergeOtherBodyConsumed => f.write_str(MERGE_OTHER_CONSUMED),
            Self::FaceAlreadySwept => f.write_str("the face was already swept"),
            Self::SweepImageOfOtherKind {
                image,
                expected,
                found,
            } => write!(
                f,
                "the recorded same-dimension image {image} is {}, not {}",
                found.with_article(),
                expected.with_article()
            ),
            Self::SweepTooShort { moved, least } => write!(
                f,
                "the sweep moves an entity {moved:e}, less than the {least:e} its tolerance needs"
            ),
            Self::RotationPastFullTurn { degrees } => write!(
                f,
                "the repeated rotation turns {degrees} degrees, more than a full turn"
            ),
            Self::PointOnRotationAxis { distance, least } => write!(
                f,
                "the point is {distance:e} from the rotation axis, within {least:e} of it"
            ),
            Self::EdgeCrossesRotationAxis => f.write_str("the edge crosses the rotation axis"),
            Self::EdgeEndsOffCurve {
                distance,
                tolerance,
            } => write!(
                f,
                "the edge's vertex is {distance:e} from its curve's end, more than {tolerance:e}"
            ),
            Self::BaseCurveEndsCoincide => {
                f.write_str("a face vertex is within tolerance of both ends of the base curve")
            }
            Self::FaceVerticesCoincide => {
                f.write_str("two face vertices are within tolerance of one end of the base curve")
            }
            Self::ProfileCurveNotThreeDimensional { dim } => {
                write!(f, "a profile curve has {dim} coordinates, not three")
            }
            Self::BaseGeometryBuildsNothing => {
                f.write_str("the base sweep geometry builds no geometry")
            }
            Self::CurveWithoutBSplineForm { entity: None } => {
                f.write_str("the curve has no B-spline form")
            }
            Self::CurveWithoutBSplineForm { entity: Some(e) } => {
                write!(f, "the curve of {e} has no B-spline form")
            }
            Self::MergeOfTwoSweptEntities { record } => {
                write!(f, "both entities to merge already have {record}")
            }
            Self::NearlyPerpendicularLine { apart, eps } => write!(
                f,
                "the line's ends lie on points of the rotation axis {apart:e} apart, more than {eps:e}"
            ),
            Self::SweptBodyNotManifoldSolid => {
                f.write_str("the swept body is not a closed manifold solid")
            }
            Self::SweptSurfaceNormalsDisagree { dot } => write!(
                f,
                "the swept plane's and the revolution's normals have cosine {dot}, below 0.8 in size"
            ),
            Self::CurveTangentToSweep => {
                f.write_str("the edge's curve is tangent to the translation somewhere")
            }
            Self::CylinderPieceNotOneFace { faces } => write!(
                f,
                "the cylinder piece a profile line sweeps built {faces} faces, not one"
            ),
            Self::SweptFaceWithoutBoundary => {
                f.write_str("the surface the edge sweeps has no boundary curves")
            }
            Self::SweptEdgeCurveDropped => {
                f.write_str("the face builder dropped the swept edge's own curve")
            }
            Self::RevolutionSweepsNoFace => f.write_str("the revolved curve sweeps no face"),
            Self::ProfileCurveStepFailed { step } => {
                write!(f, "the profile curve's {step} failed")
            }
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::RailPastStationCap {
                asked,
                reached,
                wires,
            } => write!(
                f,
                "a railed loft's guided edges were asked within {asked:e} of their rails; at {wires} wires the bound proved is {reached:e}"
            ),
            Self::LoftSectionsTooLarge {
                first,
                second,
                largest,
            } => write!(
                f,
                "loft sections of {first} and {second} segments are too large to pair; sections of equal counts up to {largest} segments pair"
            ),
            Self::LoftMatchTooLarge { segments, most } => write!(
                f,
                "matching loft sections would give a section {segments} segments, more than the {most} allowed"
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a sweep routine that fails with `failure`.
    #[must_use]
    pub(crate) fn sweep(operation: &'static str, failure: SweepFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// A sweep failure that answers with `status` in place of its cause's default (rule 1:
    /// the site's status is kept).
    #[must_use]
    pub(crate) fn sweep_with_status(
        operation: &'static str,
        failure: SweepFailure,
        status: i32,
    ) -> Self {
        Failure::new(operation, failure)
            .with_status(status)
            .into_error()
    }

    /// A reviewed sweep defect ([`SweepFailure::Defect`]) with status [`SM_ERR`].
    #[must_use]
    pub(crate) fn sweep_defect(operation: &'static str, why: &'static str) -> Self {
        Self::sweep(operation, SweepFailure::Defect { why })
    }

    /// A reviewed sweep defect ([`SweepFailure::Defect`]) with status
    /// [`SM_ERR_NULL_POINTER`]: a link or result the kernel keeps is missing where upstream
    /// dereferences a `NULL` pointer.
    #[must_use]
    pub(crate) fn sweep_null_defect(operation: &'static str, why: &'static str) -> Self {
        Self::sweep_with_status(operation, SweepFailure::Defect { why }, SM_ERR_NULL_POINTER)
    }

    /// The sweep failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Sweep`].
    #[must_use]
    pub fn sweep_failure(&self) -> Option<&SweepFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Sweep(w) => Some(w),
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

    /// Each sweep cause keeps the status of the upstream `return` it replaced, answers
    /// with it as that `Upstream` did, and has its class.
    #[test]
    fn the_sweep_causes_keep_their_status_and_have_their_class() {
        use crate::topology::{BrepId, EdgeId, FaceId};
        let edge = EdgeId::from_index(3);
        let cases = [
            (
                SweepFailure::NullTarget,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::NullProfileCurve,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::EdgeWithoutCurve { edge },
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::FaceWithoutSurface {
                    face: FaceId::from_index(2),
                },
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::BrepWithoutInfiniteRegion {
                    brep: EntityRef::Brep(BrepId::from_index(1)),
                },
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::UntaggedSilhouetteEdge { edge },
                SM_ERR_NULL_POINTER,
                ErrorKind::Unsupported,
            ),
            (
                SweepFailure::FaceCopyByFullTurn,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::UntaggedFace {
                    face: FaceId::from_index(2),
                },
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::UntaggedEdge { edge },
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::MergeOtherBodyConsumed,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::FaceAlreadySwept,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::SweepImageOfOtherKind {
                    image: EntityRef::Edge(edge),
                    expected: SweptKind::Vertex,
                    found: SweptKind::Edge,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::SweepTooShort {
                    moved: 1e-7,
                    least: 2e-6,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::RotationPastFullTurn { degrees: 450.0 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::PointOnRotationAxis {
                    distance: 0.0,
                    least: 1e-12,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::EdgeCrossesRotationAxis,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::EdgeEndsOffCurve {
                    distance: 1e-3,
                    tolerance: 1e-6,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::BaseCurveEndsCoincide,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::FaceVerticesCoincide,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::ProfileCurveNotThreeDimensional { dim: 2 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SweepFailure::BaseGeometryBuildsNothing,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                SweepFailure::CurveWithoutBSplineForm {
                    entity: Some(EntityRef::Edge(edge)),
                },
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                SweepFailure::MergeOfTwoSweptEntities {
                    record: SweepRecord::SweptImage,
                },
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                SweepFailure::NearlyPerpendicularLine {
                    apart: 1e-3,
                    eps: 1e-6,
                },
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                SweepFailure::SweptBodyNotManifoldSolid,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                SweepFailure::SweptSurfaceNormalsDisagree { dot: 0.5 },
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                SweepFailure::CurveTangentToSweep,
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                SweepFailure::CylinderPieceNotOneFace { faces: 0 },
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                SweepFailure::SweptFaceWithoutBoundary,
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                SweepFailure::SweptEdgeCurveDropped,
                SM_ERR_NULL_POINTER,
                ErrorKind::Degenerate,
            ),
            (
                SweepFailure::RevolutionSweepsNoFace,
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                SweepFailure::ProfileCurveStepFailed {
                    step: ProfileCurveStep::Nurbs,
                },
                SM_ERR,
                ErrorKind::Failed,
            ),
        ];
        for (failure, code, kind) in cases {
            let typed = SolidError::sweep("sweep::op", failure);
            let before = SolidError::Upstream {
                operation: "sweep::op",
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(super::super::upstream_status(&typed), Some(code));
            assert_eq!(typed.kind(), kind, "{failure:?}");
            assert_eq!(failure.kind(), kind, "{failure:?}");
            assert_eq!(typed.operation(), before.operation(), "{failure:?}");
            assert!(!typed.is_port_failure(), "{failure:?}");
            assert_eq!(typed.entity(), before.entity(), "{failure:?}");
            assert_eq!(
                typed.is_unported_or_cancelled(),
                before.is_unported_or_cancelled(),
                "{failure:?}"
            );
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert!(failure.replaces_upstream_status(), "{failure:?}");
            assert_eq!(typed.sweep_failure(), Some(&failure));
        }
        // a site whose status is not its cause's default keeps it
        let kept = SolidError::sweep_with_status(
            "sweep::op",
            SweepFailure::EdgeWithoutCurve { edge },
            SM_ERR,
        );
        assert_eq!(kept.upstream_status(), Some(SM_ERR));
        assert_eq!(kept.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            SolidError::sweep("sweep::op", SweepFailure::EdgeWithoutCurve { edge }).to_string(),
            "sweep::op: edge 3 has no curve"
        );
        assert_eq!(
            SolidError::sweep("sweep::op", SweepFailure::FaceAlreadySwept).to_string(),
            "sweep::op: the face was already swept"
        );
        assert_eq!(SolidError::sm_err("sweep::op").sweep_failure(), None);
    }

    /// A railed loft's refusal reports what it reported before the sweep routines' failures
    /// joined it: status `SM_ERR`, kind `not_within_tolerance`, and no upstream status (it
    /// replaced none).
    #[test]
    fn a_railed_loft_refusal_reports_as_before() {
        let cap = SweepFailure::RailPastStationCap {
            asked: 1e-6,
            reached: 1e-3,
            wires: 64,
        };
        let e = SolidError::sweep("sweep::loft_rails", cap);
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert_eq!(e.kind(), ErrorKind::NotWithinTolerance);
        assert_eq!(e.upstream_status(), None);
        assert_eq!(super::super::upstream_status(&e), None);
        assert!(!cap.replaces_upstream_status());
        assert!(e.is_status_answer());
        assert!(!e.is_port_failure());
    }

    /// The sweep routines describe their failures: their library code builds no bare
    /// `Upstream` status (the count is the bare sites a file may still hold while a site
    /// waits on other work).
    #[test]
    fn the_sweep_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("sweep", 21, &[]);
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why =
            "the swept edge the sweep just made has no curve, which the sweep always gives it";
        let defect = SweepFailure::Defect { why };
        let op = "sweep::op";
        super::super::tests::assert_defect(
            &SolidError::sweep_defect(op, why),
            &SolidError::sweep(op, SweepFailure::FaceAlreadySwept),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = SolidError::sweep_null_defect(op, why);
        super::super::tests::assert_defect(
            &missing,
            &SolidError::sweep(op, SweepFailure::NullTarget),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.sweep_failure(), Some(&defect));
    }

    /// The sites that build a sweep phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_sweep_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("SweepFailure", "Failed"),
                Builder::Variant("SweepFailure", "Missing"),
                Builder::Method("sweep_failed", Some(1)),
                Builder::Method("sweep_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        // every sweep site is converted (TE-O-22): none may come back
        const PINNED: &[(&str, usize)] = &[];
        assert_pinned("sweep phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
