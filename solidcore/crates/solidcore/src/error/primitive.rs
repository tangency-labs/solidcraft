//! Why a primitive, extrusion, revolution, skin or taper fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the primitive-creation subsystem
//! (`crate::primitive`: the primitive solids and sheets, extrusions, revolutions, curve
//! and planar-path sweeps, skins, tapered walls and the curve and surface helpers of
//! the primitive API). Each also records, for difftest only, the status the C++ reports
//! for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                   | Status difftest compares      |
//! |----------------------------------------|-------------------------------|
//! | [`PrimitiveFailure::Failed`]           | `SM_ERR`                      |
//! | [`PrimitiveFailure::Missing`]          | `SM_ERR_NULL_POINTER`         |
//! | [`PrimitiveFailure::HelixDeviates`]    | `SM_ERR_NOT_WITHIN_TOLERANCE` |
//! | [`PrimitiveFailure::AxisInsideFace`]   | `SM_ERR_AXIS_INSIDE_FACE`     |
//! | [`PrimitiveFailure::PlaneCannotCover`] | `SM_ERR_INVALID_INPUT`        |
//! | [`PrimitiveFailure::Defect`]           | `SM_ERR` (or the site's, through `with_status`) |
//! | [`PrimitiveFailure::EdgeWithoutCurve`], [`PrimitiveFailure::NoCurveGiven`], [`PrimitiveFailure::NoRegion`], [`PrimitiveFailure::TreeLeftNoBrep`], [`PrimitiveFailure::MergeOperandMissing`], [`PrimitiveFailure::MergeGaveNoBrep`], [`PrimitiveFailure::PlanarDifferenceEmpty`], [`PrimitiveFailure::FaceWithoutSurface`], [`PrimitiveFailure::EdgeWithoutFace`], [`PrimitiveFailure::SphereEdgeNotDropped`] | `SM_ERR_NULL_POINTER` |
//! | [`PrimitiveFailure::HeightTooSmall`], [`PrimitiveFailure::TreeLeavesMoreBreps`], [`PrimitiveFailure::PathTurnsBack`], [`PrimitiveFailure::SweepDoesNotClose`], [`PrimitiveFailure::SweepEnclosesNoMaterial`], [`PrimitiveFailure::SweepAngleTooSmall`], [`PrimitiveFailure::ConeSideTooShort`], [`PrimitiveFailure::ConeHasNoRadius`], [`PrimitiveFailure::ProfileFrameDegenerate`], [`PrimitiveFailure::TooFewProfiles`], [`PrimitiveFailure::ProfilesDoNotMatch`], [`PrimitiveFailure::ProfileMakesSeveralFaces`], [`PrimitiveFailure::ProfileNotOneComposite`], [`PrimitiveFailure::OffsetMakesNoCurves`], [`PrimitiveFailure::SeamWeightNotPositive`], [`PrimitiveFailure::SeamLeavesSpan`], [`PrimitiveFailure::CurvesMakeNoLoop`], [`PrimitiveFailure::OffsetMakesNoSurface`], [`PrimitiveFailure::HelixNotEvaluated`], [`PrimitiveFailure::PolygonSideNotMade`], [`PrimitiveFailure::SheetHasNoFace`], [`PrimitiveFailure::CircleNotMade`], [`PrimitiveFailure::CylinderDomainNotSet`], [`PrimitiveFailure::ConeSurfaceNotMade`], [`PrimitiveFailure::AnalyticSweepRefused`], [`PrimitiveFailure::SweptSurfaceNotMade`], [`PrimitiveFailure::SweptFaceNotMade`] | `SM_ERR` |
//!
//! [`PrimitiveFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says in a phrase (or its data) what went wrong;
//! the failure's `operation` path says which routine. No failure names an entity: the
//! sites they replace named none. The checks of a caller's arguments that report no
//! status stay [`crate::SolidError::InvalidInput`]; those that report one are causes of
//! class [`ErrorKind::InvalidInput`] keeping it. [`PrimitiveFailure::PlaneCannotCover`] is
//! not upstream: it is the refusal of WR-536's fix (`Boolean2D` refuses a plane it cannot grow
//! to cover the other faces).

use super::{
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_AXIS_INSIDE_FACE, SM_ERR_INVALID_INPUT,
    SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER,
};

/// Why a primitive, extrusion, revolution, skin or taper fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PrimitiveFailure {
    /// The routine could not continue, as the phrase says (for example "a cone side has
    /// zero length").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// face's surface").
    Missing(&'static str),
    /// The fitted helix strays from the analytic helix by more than the caller's
    /// tolerance.
    HelixDeviates {
        /// The largest distance found between the fitted and the analytic helix.
        deviation: f64,
        /// The caller's tolerance.
        tolerance: f64,
    },
    /// A revolution's axis runs inside the face it would turn, so the revolved solid
    /// would cross itself.
    AxisInsideFace,
    /// A planar 2D boolean's face lies on a plane that cannot be grown to cover the
    /// other faces, so the faces rebuilt on it would carry edges far off their surface
    /// (not upstream: WR-536's fix refuses these, where upstream builds them).
    PlaneCannotCover,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// An edge of the caller's body has no curve. [`ErrorKind::InvalidInput`], status
    /// `SM_ERR_NULL_POINTER`.
    EdgeWithoutCurve,
    /// The caller gave no curve to build from. [`ErrorKind::InvalidInput`], status
    /// `SM_ERR_NULL_POINTER`.
    NoCurveGiven,
    /// The caller's height is below the zero tolerance. [`ErrorKind::InvalidInput`],
    /// status `SM_ERR`.
    HeightTooSmall {
        /// The caller's height.
        height: f64,
        /// The smallest height that passes (`SM_EFF_ZERO`).
        minimum: f64,
    },
    /// There is no region to build the primitive's faces in: the primitive creation was
    /// made without one, or the caller's B-rep has no infinite region.
    /// [`ErrorKind::InvalidInput`], status `SM_ERR_NULL_POINTER`.
    NoRegion,
    /// The caller's Boolean tree leaves more B-reps than it was given: it names a B-rep
    /// twice and combines neither leaf. [`ErrorKind::InvalidInput`], status `SM_ERR`.
    TreeLeavesMoreBreps {
        /// The B-reps the tree leaves.
        left: usize,
        /// The B-reps it was given.
        given: usize,
    },
    /// A slot the caller's Boolean tree leaves holds no B-rep: a Boolean of the tree gave
    /// no result, or the tree names a B-rep twice. Class `failed`, status
    /// `SM_ERR_NULL_POINTER`.
    TreeLeftNoBrep,
    /// The caller's path turns back on itself at a corner, within 2 degrees, where no
    /// mitre can be cut (not upstream: WR-793). [`ErrorKind::Unsupported`], status
    /// `SM_ERR`.
    PathTurnsBack,
    /// The sweep of the caller's closed profile meant as a solid does not close, for
    /// example at a corner too sharp for its mitre (not upstream: WR-793).
    /// [`ErrorKind::Unsupported`], status `SM_ERR`.
    SweepDoesNotClose,
    /// The swept pieces bound a manifold whose regions are all void. Class `failed`,
    /// status `SM_ERR`.
    SweepEnclosesNoMaterial,
    /// The caller's sweep angle is below the zero tolerance. [`ErrorKind::InvalidInput`],
    /// status `SM_ERR`.
    SweepAngleTooSmall {
        /// The caller's sweep angle, in degrees.
        angle: f64,
        /// The smallest angle that passes (`SM_EFF_ZERO`).
        minimum: f64,
    },
    /// A side of the cone's profile, from the caller's height and radii, is no longer
    /// than the tolerance. [`ErrorKind::InvalidInput`], status `SM_ERR`.
    ConeSideTooShort {
        /// The side's length.
        length: f64,
        /// The length a side must exceed (the profile's scaled zero).
        minimum: f64,
    },
    /// Both of the caller's cone radii are within the tolerance, so the profile has no
    /// side away from the axis. [`ErrorKind::InvalidInput`], status `SM_ERR`.
    ConeHasNoRadius {
        /// The base radius as built (the base's distance from the axis).
        base: f64,
        /// The top radius as built.
        top: f64,
        /// The radius one of them must exceed (the profile's scaled zero).
        minimum: f64,
    },
    /// The frame of the revolved profile, from the caller's frame and start angle, is
    /// degenerate (the frame's error is the cause). [`ErrorKind::InvalidInput`], status
    /// `SM_ERR`.
    ProfileFrameDegenerate,
    /// The caller gave a skin fewer than two profiles. [`ErrorKind::InvalidInput`],
    /// status `SM_ERR`.
    TooFewProfiles {
        /// The caller's profiles.
        profiles: usize,
    },
    /// A profile of the caller's skin is not of the first profile's kind: it bounds no
    /// planar face where the first does (that construction's error is the cause), or it
    /// is open where the profiles are closed wires. [`ErrorKind::InvalidInput`], status
    /// `SM_ERR`.
    ProfilesDoNotMatch {
        /// The profile's index among the caller's.
        profile: usize,
    },
    /// A planar profile of the caller's skin bounds more than one face.
    /// [`ErrorKind::InvalidInput`], status `SM_ERR`.
    ProfileMakesSeveralFaces {
        /// The faces it bounds.
        faces: usize,
    },
    /// The caller's profile curves do not join into one composite curve.
    /// [`ErrorKind::InvalidInput`], status `SM_ERR`.
    ProfileNotOneComposite {
        /// The composite curves they make.
        composites: usize,
    },
    /// The untrimmed offset of the taper's profile makes no curves (every offset curve
    /// failed its B-spline approximation). Class `failed`, status `SM_ERR`.
    OffsetMakesNoCurves,
    /// A cone wall's seam pcurve has a weight that is not positive in one of the cone's
    /// spans, so it is no rational B-spline. Class `failed`, status `SM_ERR`.
    SeamWeightNotPositive {
        /// The weight.
        weight: f64,
    },
    /// A piece of a cone wall's seam pcurve leaves the span of the cone it was cut for.
    /// Class `failed`, status `SM_ERR`.
    SeamLeavesSpan {
        /// The piece's parameter across its span, outside 0 to 1.
        tau: f64,
    },
    /// The caller's curves were ordered, and none of them closes a loop.
    /// [`ErrorKind::InvalidInput`], status `SM_ERR`.
    CurvesMakeNoLoop,
    /// The offset of the caller's surface by the caller's distance collapses, so it makes
    /// no surface (for example a cone offset past its apex). [`ErrorKind::Degenerate`],
    /// status `SM_ERR`.
    OffsetMakesNoSurface,
    /// The fitted helix could not be evaluated where its deviation is measured (the
    /// evaluation's error is the cause). Class `failed`, status `SM_ERR`.
    HelixNotEvaluated,
    /// A side of the polygon could not be made from its two corners (the line's error is
    /// the cause, and decides defect or refusal). Class `failed`, status `SM_ERR`.
    PolygonSideNotMade,
    /// A merge stage was given no operand on one side (`other` is false for the
    /// primary): an earlier stage's Boolean consumed it, or the merge released it. Class
    /// `failed`, status `SM_ERR_NULL_POINTER`.
    MergeOperandMissing {
        /// The missing operand is the other (second) one.
        other: bool,
    },
    /// A merge's Boolean returned without a result B-rep. Class `failed`, status
    /// `SM_ERR_NULL_POINTER`.
    MergeGaveNoBrep,
    /// A planar difference had an operand with no faces, so it made no B-rep (the offset
    /// profile collapsed to nothing). [`ErrorKind::Degenerate`], status
    /// `SM_ERR_NULL_POINTER`.
    PlanarDifferenceEmpty,
    /// A face of the caller's body has no surface. [`ErrorKind::InvalidInput`], status
    /// `SM_ERR_NULL_POINTER`.
    FaceWithoutSurface,
    /// An edge of the caller's body that the other body shares bounds no face (a wire
    /// edge). [`ErrorKind::InvalidInput`], status `SM_ERR_NULL_POINTER`.
    EdgeWithoutFace,
    /// The sheet's constructor returned a B-rep without a face: the rectangle's ignores its
    /// face constructor's status, as upstream, and the disc's face maker found no face.
    /// Class `failed`, status `SM_ERR`.
    SheetHasNoFace,
    /// The circle or its planar face could not be made (the constructor's error is the
    /// cause). Class `failed`, status `SM_ERR`.
    CircleNotMade,
    /// The cylinder could not be bounded to the caller's height (the bound's error is the
    /// cause). Class `failed`, status `SM_ERR`.
    CylinderDomainNotSet,
    /// The elliptic-ended cone's surface could not be made from the caller's ends (the
    /// constructor's error is the cause). Class `failed`, status `SM_ERR`.
    ConeSurfaceNotMade,
    /// The caller's curve cannot be revolved about the axis into one analytic face.
    /// [`ErrorKind::InvalidInput`], status `SM_ERR`.
    AnalyticSweepRefused(AnalyticSweepRefusal),
    /// The analytic surface of the revolved curve could not be made (its constructor's
    /// status is ignored as upstream). Class `failed`, status `SM_ERR`.
    SweptSurfaceNotMade,
    /// The face on the revolved curve's surface could not be made (the face's error is
    /// the cause). Class `failed`, status `SM_ERR`.
    SweptFaceNotMade,
    /// The drop of a band patch's edge onto a cap patch of the pole-less sphere found no
    /// UV curve at the caller's tolerance. Class `failed`, status `SM_ERR_NULL_POINTER`.
    SphereEdgeNotDropped {
        /// The caller's tolerance the drop used.
        tolerance: f64,
    },
}

/// Why the caller's curve cannot be revolved about the axis into one analytic face
/// ([`PrimitiveFailure::AnalyticSweepRefused`]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum AnalyticSweepRefusal {
    /// The line lies on the axis.
    LineOnAxis,
    /// The line is skew to the axis: it sweeps a hyperboloid, not a cone (not upstream,
    /// WR-776).
    LineSkewToAxis,
    /// The curve crosses the axis inside its ends.
    CrossesAxis,
    /// The curve runs along the axis.
    RunsAlongAxis,
    /// An end of the half circle is off the axis by more than the B-rep's tolerance.
    HalfCircleEndOffAxis {
        /// The end's distance from the axis.
        distance: f64,
        /// The B-rep's tolerance, the largest distance that passes.
        tolerance: f64,
    },
    /// The full circle meets the axis.
    FullCircleMeetsAxis,
    /// The arc is neither a half nor a full circle.
    ArcNotHalfOrFull {
        /// The arc's angle, in degrees.
        angle: f64,
    },
    /// The curve is neither a line nor an arc.
    NotLineOrArc,
}

impl core::fmt::Display for AnalyticSweepRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LineOnAxis => f.write_str("the line lies on the axis"),
            Self::LineSkewToAxis => {
                f.write_str("the line is skew to the axis: it sweeps a hyperboloid, not a cone")
            }
            Self::CrossesAxis => f.write_str("the curve crosses the axis inside its ends"),
            Self::RunsAlongAxis => f.write_str("the curve runs along the axis"),
            Self::HalfCircleEndOffAxis {
                distance,
                tolerance,
            } => write!(
                f,
                "an end of the half circle is {distance} off the axis, more than the tolerance {tolerance}"
            ),
            Self::FullCircleMeetsAxis => f.write_str("the full circle meets the axis"),
            Self::ArcNotHalfOrFull { angle } => {
                write!(f, "the arc of {angle} degrees is neither a half nor a full circle")
            }
            Self::NotLineOrArc => f.write_str("the curve is neither a line nor an arc"),
        }
    }
}

impl PrimitiveFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_)
            | Self::Defect { .. }
            | Self::HeightTooSmall { .. }
            | Self::TreeLeavesMoreBreps { .. }
            | Self::PathTurnsBack
            | Self::SweepDoesNotClose
            | Self::SweepEnclosesNoMaterial
            | Self::SweepAngleTooSmall { .. }
            | Self::ConeSideTooShort { .. }
            | Self::ConeHasNoRadius { .. }
            | Self::ProfileFrameDegenerate
            | Self::TooFewProfiles { .. }
            | Self::ProfilesDoNotMatch { .. }
            | Self::ProfileMakesSeveralFaces { .. }
            | Self::ProfileNotOneComposite { .. }
            | Self::OffsetMakesNoCurves
            | Self::SeamWeightNotPositive { .. }
            | Self::SeamLeavesSpan { .. }
            | Self::CurvesMakeNoLoop
            | Self::OffsetMakesNoSurface
            | Self::HelixNotEvaluated
            | Self::PolygonSideNotMade
            | Self::SheetHasNoFace
            | Self::CircleNotMade
            | Self::CylinderDomainNotSet
            | Self::ConeSurfaceNotMade
            | Self::AnalyticSweepRefused(_)
            | Self::SweptSurfaceNotMade
            | Self::SweptFaceNotMade => SM_ERR,
            Self::Missing(_)
            | Self::EdgeWithoutCurve
            | Self::NoCurveGiven
            | Self::NoRegion
            | Self::TreeLeftNoBrep
            | Self::MergeOperandMissing { .. }
            | Self::MergeGaveNoBrep
            | Self::PlanarDifferenceEmpty
            | Self::FaceWithoutSurface
            | Self::EdgeWithoutFace
            | Self::SphereEdgeNotDropped { .. } => SM_ERR_NULL_POINTER,
            Self::HelixDeviates { .. } => SM_ERR_NOT_WITHIN_TOLERANCE,
            Self::AxisInsideFace => SM_ERR_AXIS_INSIDE_FACE,
            Self::PlaneCannotCover => SM_ERR_INVALID_INPUT,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`PrimitiveFailure::Missing`], [`ErrorKind::NotWithinTolerance`] for
    /// [`PrimitiveFailure::HelixDeviates`], [`ErrorKind::InvalidInput`] for
    /// [`PrimitiveFailure::PlaneCannotCover`] and the checks of the caller's arguments and
    /// body, [`ErrorKind::Unsupported`] for a path or a sweep the planar-path sweep does not
    /// take, [`ErrorKind::Degenerate`] for a collapsed offset or planar difference,
    /// [`ErrorKind::Failed`] otherwise.
    /// [`PrimitiveFailure::Defect`] is [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::EdgeWithoutCurve
            | Self::NoCurveGiven
            | Self::HeightTooSmall { .. }
            | Self::NoRegion
            | Self::TreeLeavesMoreBreps { .. }
            | Self::SweepAngleTooSmall { .. }
            | Self::ConeSideTooShort { .. }
            | Self::ConeHasNoRadius { .. }
            | Self::ProfileFrameDegenerate
            | Self::TooFewProfiles { .. }
            | Self::ProfilesDoNotMatch { .. }
            | Self::ProfileMakesSeveralFaces { .. }
            | Self::ProfileNotOneComposite { .. }
            | Self::CurvesMakeNoLoop
            | Self::FaceWithoutSurface
            | Self::EdgeWithoutFace
            | Self::AnalyticSweepRefused(_) => ErrorKind::InvalidInput,
            Self::PathTurnsBack | Self::SweepDoesNotClose => ErrorKind::Unsupported,
            Self::OffsetMakesNoSurface | Self::PlanarDifferenceEmpty => ErrorKind::Degenerate,
            Self::TreeLeftNoBrep
            | Self::SweepEnclosesNoMaterial
            | Self::OffsetMakesNoCurves
            | Self::SeamWeightNotPositive { .. }
            | Self::SeamLeavesSpan { .. }
            | Self::MergeOperandMissing { .. }
            | Self::MergeGaveNoBrep
            | Self::SphereEdgeNotDropped { .. } => ErrorKind::Failed,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for PrimitiveFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::HelixDeviates {
                deviation,
                tolerance,
            } => write!(
                f,
                "the fitted helix strays {deviation} from the helix, more than the tolerance {tolerance}"
            ),
            Self::AxisInsideFace => {
                f.write_str("the revolution axis runs inside the face it would turn")
            }
            Self::PlaneCannotCover => {
                f.write_str("a face's plane cannot be grown to cover the other faces")
            }
            Self::EdgeWithoutCurve => f.write_str("the edge has no curve"),
            Self::NoCurveGiven => f.write_str("no curve was given"),
            Self::HeightTooSmall { height, minimum } => write!(
                f,
                "the height {height} is below the zero tolerance {minimum}"
            ),
            Self::NoRegion => f.write_str("there is no region to build the faces in"),
            Self::TreeLeavesMoreBreps { left, given } => write!(
                f,
                "the Boolean tree leaves {left} B-reps, more than the {given} it was given"
            ),
            Self::TreeLeftNoBrep => f.write_str("a slot the Boolean tree leaves holds no B-rep"),
            Self::PathTurnsBack => f.write_str("the path turns back on itself at a corner"),
            Self::SweepDoesNotClose => f.write_str("the sweep does not close"),
            Self::SweepEnclosesNoMaterial => f.write_str("the sweep encloses no material"),
            Self::SweepAngleTooSmall { angle, minimum } => write!(
                f,
                "the sweep angle {angle} is below the zero tolerance {minimum}"
            ),
            Self::ConeSideTooShort { length, minimum } => write!(
                f,
                "a cone side of length {length} is not longer than the tolerance {minimum}"
            ),
            Self::ConeHasNoRadius { base, top, minimum } => write!(
                f,
                "the cone's radii {base} and {top} are both within the tolerance {minimum}"
            ),
            Self::ProfileFrameDegenerate => f.write_str("the profile's frame is degenerate"),
            Self::TooFewProfiles { profiles } => {
                write!(f, "a skin needs at least two profiles, not {profiles}")
            }
            Self::ProfilesDoNotMatch { profile } => {
                write!(f, "profile {profile} does not match the first")
            }
            Self::ProfileMakesSeveralFaces { faces } => {
                write!(f, "a profile makes {faces} faces, not one")
            }
            Self::ProfileNotOneComposite { composites } => write!(
                f,
                "the profile's curves make {composites} composite curves, not one"
            ),
            Self::OffsetMakesNoCurves => f.write_str("the untrimmed offset makes no curves"),
            Self::SeamWeightNotPositive { weight } => {
                write!(f, "a seam pcurve weight {weight} is not positive")
            }
            Self::SeamLeavesSpan { tau } => {
                write!(f, "a seam piece leaves its span, at {tau} across it")
            }
            Self::CurvesMakeNoLoop => f.write_str("the curves make no loop"),
            Self::OffsetMakesNoSurface => {
                f.write_str("the offset collapses the surface, so it makes no surface")
            }
            Self::HelixNotEvaluated => f.write_str("the fitted helix could not be evaluated"),
            Self::PolygonSideNotMade => f.write_str("a polygon side could not be made"),
            Self::MergeOperandMissing { other: false } => {
                f.write_str("the merge stage has no primary operand")
            }
            Self::MergeOperandMissing { other: true } => {
                f.write_str("the merge stage has no other operand")
            }
            Self::MergeGaveNoBrep => f.write_str("the merge gave no result B-rep"),
            Self::PlanarDifferenceEmpty => f.write_str(
                "the planar difference had an operand with no faces, so it made no B-rep",
            ),
            Self::FaceWithoutSurface => f.write_str("the face has no surface"),
            Self::EdgeWithoutFace => f.write_str("the edge bounds no face"),
            Self::SheetHasNoFace => f.write_str("the sheet's face could not be made"),
            Self::CircleNotMade => f.write_str("the circle could not be built"),
            Self::CylinderDomainNotSet => {
                f.write_str("the cylinder could not be bounded to the height")
            }
            Self::ConeSurfaceNotMade => f.write_str("the cone surface could not be built"),
            Self::AnalyticSweepRefused(refusal) => refusal.fmt(f),
            Self::SweptSurfaceNotMade => f.write_str("no analytic surface could be built"),
            Self::SweptFaceNotMade => f.write_str("the analytic face could not be built"),
            Self::SphereEdgeNotDropped { tolerance } => write!(
                f,
                "the drop of a sphere patch's edge onto the cap patch found no UV curve at the tolerance {tolerance}"
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a primitive-creation routine that fails with
    /// `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn primitive(operation: &'static str, failure: PrimitiveFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a primitive-creation routine that could not
    /// continue, as the phrase says, with status [`SM_ERR`] (upstream
    /// `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn primitive_failed(operation: &'static str, what: &'static str) -> Self {
        Self::primitive(operation, PrimitiveFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a primitive-creation routine needs
    /// that is not there, as the phrase says, with status [`SM_ERR_NULL_POINTER`]
    /// (upstream `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn primitive_missing(operation: &'static str, what: &'static str) -> Self {
        Self::primitive(operation, PrimitiveFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a reviewed defect of a primitive-creation routine
    /// ([`PrimitiveFailure::Defect`]), with the status [`SM_ERR_NULL_POINTER`] of the
    /// `NER` it replaces: `why` names the link of the kernel's own that is missing, and
    /// why that is the kernel's fault.
    #[must_use]
    #[track_caller]
    pub(crate) fn primitive_broken_link(operation: &'static str, why: &'static str) -> Self {
        Failure::new(operation, PrimitiveFailure::Defect { why })
            .with_status(SM_ERR_NULL_POINTER)
            .into_error()
    }

    /// The primitive-creation failure behind this error, for a
    /// [`crate::SolidError::Failed`] with a [`Reason::Primitive`].
    #[must_use]
    pub fn primitive_failure(&self) -> Option<&PrimitiveFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Primitive(p) => Some(p),
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

    #[test]
    fn primitive_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (PrimitiveFailure::Failed("x"), SM_ERR),
            (PrimitiveFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (
                PrimitiveFailure::HelixDeviates {
                    deviation: 2.0,
                    tolerance: 1.0,
                },
                SM_ERR_NOT_WITHIN_TOLERANCE,
            ),
            (PrimitiveFailure::AxisInsideFace, SM_ERR_AXIS_INSIDE_FACE),
            (PrimitiveFailure::PlaneCannotCover, SM_ERR_INVALID_INPUT),
        ];
        for (failure, code) in cases {
            let typed = SolidError::primitive("primitive::op", failure);
            let before = SolidError::Upstream {
                operation: "primitive::op",
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(super::super::upstream_status(&typed), Some(code));
            assert_eq!(typed.kind(), before.kind(), "{failure:?}");
            assert_eq!(typed.operation(), before.operation(), "{failure:?}");
            assert_eq!(typed.entity(), before.entity(), "{failure:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(
                typed.is_unported_or_cancelled(),
                before.is_unported_or_cancelled()
            );
            assert_eq!(typed.primitive_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::primitive_missing("primitive::op", "the face's surface").to_string(),
            "primitive::op: the face's surface is missing"
        );
        assert_eq!(
            SolidError::primitive_failed("primitive::op", "the step could not go on").to_string(),
            "primitive::op: the step could not go on"
        );
        assert_eq!(
            SolidError::primitive(
                "primitive::op",
                PrimitiveFailure::HelixDeviates {
                    deviation: 0.5,
                    tolerance: 0.25,
                }
            )
            .kind(),
            ErrorKind::NotWithinTolerance
        );
        assert_eq!(
            SolidError::primitive_failed("primitive::op", "x").primitive_failure(),
            Some(&PrimitiveFailure::Failed("x"))
        );
        assert_eq!(
            SolidError::sm_err("primitive::op").primitive_failure(),
            None
        );
    }

    /// The primitive-creation routines describe their failures: their library code builds
    /// no bare `Upstream` status.
    #[test]
    fn the_primitive_routines_build_no_bare_statuses() {
        assert_eq!(
            super::super::query::tests::bare_statuses_in("primitive", 25),
            Vec::<(String, usize)>::new()
        );
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the face the extrusion just built has no loop, which its own construction always gives";
        let defect = PrimitiveFailure::Defect { why };
        let op = "primitive::op";
        super::super::tests::assert_defect(
            &SolidError::primitive(op, defect),
            &SolidError::primitive(op, PrimitiveFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::primitive(op, PrimitiveFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.primitive_failure(), Some(&defect));
    }

    /// The named causes of the profile-driven primitives keep the status of the phrase each
    /// replaced, and have their class; a caught error stays the cause.
    #[test]
    fn the_profile_primitive_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let op = "primitive::op";
        let cases = [
            (
                PrimitiveFailure::EdgeWithoutCurve,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
                "the edge has no curve",
            ),
            (
                PrimitiveFailure::NoCurveGiven,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
                "no curve was given",
            ),
            (
                PrimitiveFailure::HeightTooSmall {
                    height: 0.0,
                    minimum: 1e-12,
                },
                SM_ERR,
                K::InvalidInput,
                "the height 0 is below the zero tolerance 0.000000000001",
            ),
            (
                PrimitiveFailure::NoRegion,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
                "there is no region to build the faces in",
            ),
            (
                PrimitiveFailure::TreeLeavesMoreBreps { left: 2, given: 1 },
                SM_ERR,
                K::InvalidInput,
                "the Boolean tree leaves 2 B-reps, more than the 1 it was given",
            ),
            (
                PrimitiveFailure::TreeLeftNoBrep,
                SM_ERR_NULL_POINTER,
                K::Failed,
                "a slot the Boolean tree leaves holds no B-rep",
            ),
            (
                PrimitiveFailure::PathTurnsBack,
                SM_ERR,
                K::Unsupported,
                "the path turns back on itself at a corner",
            ),
            (
                PrimitiveFailure::SweepDoesNotClose,
                SM_ERR,
                K::Unsupported,
                "the sweep does not close",
            ),
            (
                PrimitiveFailure::SweepEnclosesNoMaterial,
                SM_ERR,
                K::Failed,
                "the sweep encloses no material",
            ),
            (
                PrimitiveFailure::SweepAngleTooSmall {
                    angle: 0.0,
                    minimum: 0.5,
                },
                SM_ERR,
                K::InvalidInput,
                "the sweep angle 0 is below the zero tolerance 0.5",
            ),
            (
                PrimitiveFailure::ConeSideTooShort {
                    length: 0.25,
                    minimum: 0.5,
                },
                SM_ERR,
                K::InvalidInput,
                "a cone side of length 0.25 is not longer than the tolerance 0.5",
            ),
            (
                PrimitiveFailure::ConeHasNoRadius {
                    base: 0.0,
                    top: 0.25,
                    minimum: 0.5,
                },
                SM_ERR,
                K::InvalidInput,
                "the cone's radii 0 and 0.25 are both within the tolerance 0.5",
            ),
            (
                PrimitiveFailure::ProfileFrameDegenerate,
                SM_ERR,
                K::InvalidInput,
                "the profile's frame is degenerate",
            ),
            (
                PrimitiveFailure::TooFewProfiles { profiles: 1 },
                SM_ERR,
                K::InvalidInput,
                "a skin needs at least two profiles, not 1",
            ),
            (
                PrimitiveFailure::ProfilesDoNotMatch { profile: 2 },
                SM_ERR,
                K::InvalidInput,
                "profile 2 does not match the first",
            ),
            (
                PrimitiveFailure::ProfileMakesSeveralFaces { faces: 2 },
                SM_ERR,
                K::InvalidInput,
                "a profile makes 2 faces, not one",
            ),
            (
                PrimitiveFailure::ProfileNotOneComposite { composites: 0 },
                SM_ERR,
                K::InvalidInput,
                "the profile's curves make 0 composite curves, not one",
            ),
            (
                PrimitiveFailure::OffsetMakesNoCurves,
                SM_ERR,
                K::Failed,
                "the untrimmed offset makes no curves",
            ),
            (
                PrimitiveFailure::SeamWeightNotPositive { weight: -0.5 },
                SM_ERR,
                K::Failed,
                "a seam pcurve weight -0.5 is not positive",
            ),
            (
                PrimitiveFailure::SeamLeavesSpan { tau: 1.5 },
                SM_ERR,
                K::Failed,
                "a seam piece leaves its span, at 1.5 across it",
            ),
        ];
        for (failure, status, kind, message) in cases {
            let e = SolidError::primitive(op, failure);
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.primitive_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
            assert_eq!(e.to_string(), format!("{op}: {message}"));
        }
        // a caught error stays the cause; the status and class stay the site's
        let caught = SolidError::invalid_input(op, "an axis has zero length");
        let e = Failure::new(op, PrimitiveFailure::ProfileFrameDegenerate)
            .caused_by(caught)
            .into_error();
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.kind(), K::InvalidInput);
    }

    /// The named causes keep the status of the phrase each replaced, and have their class;
    /// a caught error stays the cause, and decides defect or refusal.
    #[test]
    fn the_named_primitive_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let op = "primitive::op";
        let cases = [
            (
                PrimitiveFailure::CurvesMakeNoLoop,
                SM_ERR,
                K::InvalidInput,
                "the curves make no loop",
            ),
            (
                PrimitiveFailure::OffsetMakesNoSurface,
                SM_ERR,
                K::Degenerate,
                "the offset collapses the surface, so it makes no surface",
            ),
            (
                PrimitiveFailure::HelixNotEvaluated,
                SM_ERR,
                K::Failed,
                "the fitted helix could not be evaluated",
            ),
            (
                PrimitiveFailure::PolygonSideNotMade,
                SM_ERR,
                K::Failed,
                "a polygon side could not be made",
            ),
            (
                PrimitiveFailure::MergeOperandMissing { other: false },
                SM_ERR_NULL_POINTER,
                K::Failed,
                "the merge stage has no primary operand",
            ),
            (
                PrimitiveFailure::MergeOperandMissing { other: true },
                SM_ERR_NULL_POINTER,
                K::Failed,
                "the merge stage has no other operand",
            ),
            (
                PrimitiveFailure::MergeGaveNoBrep,
                SM_ERR_NULL_POINTER,
                K::Failed,
                "the merge gave no result B-rep",
            ),
            (
                PrimitiveFailure::PlanarDifferenceEmpty,
                SM_ERR_NULL_POINTER,
                K::Degenerate,
                "the planar difference had an operand with no faces, so it made no B-rep",
            ),
            (
                PrimitiveFailure::FaceWithoutSurface,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
                "the face has no surface",
            ),
            (
                PrimitiveFailure::EdgeWithoutFace,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
                "the edge bounds no face",
            ),
            (
                PrimitiveFailure::SheetHasNoFace,
                SM_ERR,
                K::Failed,
                "the sheet's face could not be made",
            ),
            (
                PrimitiveFailure::CircleNotMade,
                SM_ERR,
                K::Failed,
                "the circle could not be built",
            ),
            (
                PrimitiveFailure::CylinderDomainNotSet,
                SM_ERR,
                K::Failed,
                "the cylinder could not be bounded to the height",
            ),
            (
                PrimitiveFailure::ConeSurfaceNotMade,
                SM_ERR,
                K::Failed,
                "the cone surface could not be built",
            ),
            (
                PrimitiveFailure::AnalyticSweepRefused(AnalyticSweepRefusal::ArcNotHalfOrFull {
                    angle: 90.0,
                }),
                SM_ERR,
                K::InvalidInput,
                "the arc of 90 degrees is neither a half nor a full circle",
            ),
            (
                PrimitiveFailure::AnalyticSweepRefused(
                    AnalyticSweepRefusal::HalfCircleEndOffAxis {
                        distance: 0.5,
                        tolerance: 0.25,
                    },
                ),
                SM_ERR,
                K::InvalidInput,
                "an end of the half circle is 0.5 off the axis, more than the tolerance 0.25",
            ),
            (
                PrimitiveFailure::AnalyticSweepRefused(AnalyticSweepRefusal::LineOnAxis),
                SM_ERR,
                K::InvalidInput,
                "the line lies on the axis",
            ),
            (
                PrimitiveFailure::AnalyticSweepRefused(AnalyticSweepRefusal::LineSkewToAxis),
                SM_ERR,
                K::InvalidInput,
                "the line is skew to the axis: it sweeps a hyperboloid, not a cone",
            ),
            (
                PrimitiveFailure::AnalyticSweepRefused(AnalyticSweepRefusal::CrossesAxis),
                SM_ERR,
                K::InvalidInput,
                "the curve crosses the axis inside its ends",
            ),
            (
                PrimitiveFailure::AnalyticSweepRefused(AnalyticSweepRefusal::RunsAlongAxis),
                SM_ERR,
                K::InvalidInput,
                "the curve runs along the axis",
            ),
            (
                PrimitiveFailure::AnalyticSweepRefused(AnalyticSweepRefusal::FullCircleMeetsAxis),
                SM_ERR,
                K::InvalidInput,
                "the full circle meets the axis",
            ),
            (
                PrimitiveFailure::AnalyticSweepRefused(AnalyticSweepRefusal::NotLineOrArc),
                SM_ERR,
                K::InvalidInput,
                "the curve is neither a line nor an arc",
            ),
            (
                PrimitiveFailure::SphereEdgeNotDropped { tolerance: 0.5 },
                SM_ERR_NULL_POINTER,
                K::Failed,
                "the drop of a sphere patch's edge onto the cap patch found no UV curve at the tolerance 0.5",
            ),
            (
                PrimitiveFailure::SweptSurfaceNotMade,
                SM_ERR,
                K::Failed,
                "no analytic surface could be built",
            ),
            (
                PrimitiveFailure::SweptFaceNotMade,
                SM_ERR,
                K::Failed,
                "the analytic face could not be built",
            ),
        ];
        for (failure, status, kind, message) in cases {
            let e = SolidError::primitive(op, failure);
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.primitive_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
            assert_eq!(e.to_string(), format!("{op}: {message}"));
        }
        // a caught error stays the cause: it decides defect, and the status stays the site's
        // (`SM_ERR`), not the cause's (`SM_ERR_NULL_POINTER`)
        let why = "a step the kernel got wrong";
        let caught = SolidError::primitive_broken_link(op, why);
        let e = Failure::new(op, PrimitiveFailure::PolygonSideNotMade)
            .caused_by(caught)
            .into_error();
        assert!(e.is_defect(), "{e}");
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.kind(), K::Failed);
        // a broken link of the kernel's own is a defect keeping `SM_ERR_NULL_POINTER`
        let link = SolidError::primitive_broken_link(op, why);
        assert!(link.is_defect(), "{link}");
        assert_eq!(link.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(
            link.primitive_failure(),
            Some(&PrimitiveFailure::Defect { why })
        );
        assert_eq!(link.to_string(), format!("{op}: stage_failed: {why}"));
    }

    /// The sites that build a primitive phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_primitive_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("PrimitiveFailure", "Failed"),
                Builder::Variant("PrimitiveFailure", "Missing"),
                Builder::Method("primitive_failed", Some(1)),
                Builder::Method("primitive_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/primitive/primitive_creation/p3/construct.rs", 7),
            ("solidcore/src/primitive/primitive_creation/p3/geometry.rs", 5),
            ("solidcore/src/primitive/primitive_creation/p3/ruled.rs", 9),
            ("solidcore/src/primitive/primitive_creation/p3/sweeps.rs", 10),
        ];
        assert_pinned(
            "primitive phrase-cause sites",
            &crate_sites(&PHRASES),
            PINNED,
        );
    }
}
