//! Why a surface routine fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the surface routines (`crate::surface`: the
//! surface kinds' evaluation, inversion, solvers, trimming, conversion and approximation,
//! their caches, and the volume and deformation surfaces). Each also records, for difftest
//! only, the status the C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                               | Status difftest compares       |
//! |------------------------------------|--------------------------------|
//! | [`SurfaceFailure::Failed`]         | `SM_ERR`                       |
//! | [`SurfaceFailure::Missing`]        | `SM_ERR_NULL_POINTER`          |
//! | [`SurfaceFailure::InvalidValue`]   | `SM_ERR_INVALID_INPUT`         |
//! | [`SurfaceFailure::OutsideDomain`]  | `SM_ERR_OUTSIDE_OF_DOMAIN`     |
//! | [`SurfaceFailure::OffSurface`]     | `SM_ERR_NOT_WITHIN_TOLERANCE`  |
//! | [`SurfaceFailure::Defect`]         | `SM_ERR` (or the site's, through `with_status`) |
//! | the named causes of the surface solvers, [`SurfaceFailure::SolverWithout`] to [`SurfaceFailure::NothingLifted`] | the status of the phrase each replaced (`comparison_status`), or the site's through `with_status` |
//! | the named causes of the volume surfaces and the implicit checks, from [`SurfaceFailure::TooFewKnots`] on | `SM_ERR`, except [`SurfaceFailure::NoNurb`]: `SM_ERR_NULL_POINTER` (a site that reported `SM_ERR` keeps it through `with_status`) |
//! | [`SurfaceFailure::NoNurb`], [`SurfaceFailure::InputNotGiven`], [`SurfaceFailure::TrimmedCacheHasNoFace`], [`SurfaceFailure::SurfaceCacheHasNoTree`], [`SurfaceFailure::CurvesNotCached`], [`SurfaceFailure::FaceHasNoSurface`], [`SurfaceFailure::EdgeuseHasNoTrimCurve`], [`SurfaceFailure::NodeHasNoSurface`] | `SM_ERR_NULL_POINTER` (class [`ErrorKind::InvalidInput`]) |
//! | [`SurfaceFailure::SurfaceNotBSplineKind`], [`SurfaceFailure::CurveNotBSplineKind`] | `SM_ERR_NULL_POINTER` (class [`ErrorKind::Unsupported`]) |
//! | [`SurfaceFailure::SurfaceCacheNotBuilt`], [`SurfaceFailure::SurfaceCacheTreeNotBuilt`], [`SurfaceFailure::CurveCacheNotBuilt`], [`SurfaceFailure::CurveCacheTreeNotBuilt`] | `SM_ERR_NULL_POINTER` (class [`ErrorKind::Failed`]; defect-ness follows the cause) |
//! | [`SurfaceFailure::NormalsCancel`]  | `SM_ERR_INVALID_INPUT` (class [`ErrorKind::Degenerate`]) |
//! | [`SurfaceFailure::DisjointFaceDomain`] | `SM_ERR_INVALID_INPUT` (class [`ErrorKind::InvalidInput`]) |
//! | every other named cause            | `SM_ERR`, with the class its doc names |
//!
//! [`SurfaceFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! **Classes of the named causes.** A check of the caller's argument or object that is
//! malformed is [`ErrorKind::InvalidInput`]; a box or point outside the domain it must lie
//! in is [`ErrorKind::OutsideDomain`]; a kind or an ask the routine has no code for is
//! [`ErrorKind::Unsupported`]; geometry degenerate where the routine needs it not to be is
//! [`ErrorKind::Degenerate`]; a routine whose callee failed keeps that error as its cause
//! ([`SurfaceFailure::CallFailed`]).
//!
//! Deviations: not upstream. A failure says in a phrase (or by its variant) what went wrong;
//! the failure's `operation` path says which routine. A failure names an entity only where the
//! site it replaces named one (a blend face whose net fails its re-certification). A check of a
//! caller's argument that reports no status stays [`crate::SolidError::InvalidInput`]; one that
//! reports a status is a named cause of class `invalid_input`, keeping that status. Only part of
//! `crate::surface` uses these yet; the files that still build bare statuses are listed in this
//! module's ratchet test.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT,
    SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER, SM_ERR_OUTSIDE_OF_DOMAIN,
};

/// Why a surface routine fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SurfaceFailure {
    /// The routine could not continue, as the phrase says (for example "the generator
    /// curve has no NURB form").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// surface's cache").
    Missing(&'static str),
    /// A value the routine computes with has no defined result, as the phrase says (for
    /// example "the line direction is the zero vector"). Upstream returns
    /// `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// A parameter or domain lies outside the surface's domain, as the phrase says (for
    /// example "the point is outside the sweep of the surface of revolution").
    OutsideDomain(&'static str),
    /// A point is farther from the surface than the tolerance, as the phrase says (for
    /// example "the point is off the surface"): an inversion's answer.
    OffSurface(&'static str),
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The solver or solve context given lacks `part`, which the routine reads (a solver
    /// built without its cache, curve or tree). [`ErrorKind::InvalidInput`].
    SolverWithout {
        /// What the solver lacks.
        part: SolverPart,
    },
    /// The cache a solve works on, or its tree, could not be made (the build's failure is
    /// the cause). Status `SM_ERR_NULL_POINTER` (upstream's null cache or tree); class
    /// `failed`, and whether it is a defect or a refusal is its cause's.
    SolveCacheNotMade,
    /// The line direction given is the zero vector. Status `SM_ERR_INVALID_INPUT` (or the
    /// site's, through `with_status`); [`ErrorKind::InvalidInput`].
    LineDirectionZero,
    /// A surface given has an empty natural domain. [`ErrorKind::InvalidInput`].
    EmptyDomain,
    /// The blend definition given is not of a surface boundary curve, the only kind the
    /// routine builds. [`ErrorKind::InvalidInput`].
    BlendDefinitionNotFromBoundary,
    /// The blend surface between the rails could not be made (the construction's failure
    /// is the cause). Class `failed`, and whether it is a defect or a refusal is its cause's.
    BlendSurfaceNotMade,
    /// Merging a coincident line range with the next one left a range whose start is past
    /// its end, on the line's parameter. Class `failed`.
    MergedRangeReversed {
        /// The merged range's start parameter.
        start: f64,
        /// The merged range's end parameter.
        end: f64,
    },
    /// A knot read with its continuity has none, or a continuity read with its knot has
    /// none: the two lists' lengths disagree. Class `failed`.
    KnotsAndContinuitiesDisagree {
        /// The number of knots.
        knots: usize,
        /// The number of continuities.
        continuities: usize,
    },
    /// No degenerate parameter toward the point was found near a singular boundary (the
    /// search did not converge, and no start of it reported found). Class `failed`.
    DegenerateParameterNotFound,
    /// A distance from the test point near a singular boundary has no enclosure (it is not
    /// a number, or it is at a point past a rational net's knots), so it cannot be
    /// compared. Class `failed`.
    DistanceNotEnclosed,
    /// The point found near a singular boundary is proven farther from the test point than
    /// the guess. Class `failed`.
    FartherThanGuess {
        /// The lower bound of the found point's distance.
        found: f64,
        /// The upper bound of the guess's distance.
        guess: f64,
    },
    /// A point of the singular boundary is proven nearer the test point than the point
    /// found, so the boundary is not a pole. Class `failed`.
    SingularBoundaryNotPole {
        /// The upper bound of the boundary point's distance.
        boundary: f64,
        /// The lower bound of the found point's distance.
        found: f64,
    },
    /// The local surface/surface intersection recursed past its depth bound. Class
    /// `failed`.
    RecursionPastBound {
        /// The depth bound.
        bound: usize,
    },
    /// The iso curve extracted to lift is of a kind the lift does not take.
    /// [`ErrorKind::Unsupported`].
    IsoCurveKindNotLiftable,
    /// A segment of the UV curve to lift has no surface domain at least C1 around its
    /// start. Class `failed`.
    NoContinuousDomain,
    /// Every segment of the UV curve to lift was too small or degenerate, so nothing was
    /// lifted. Class `failed`.
    NothingLifted,
    /// A knot vector of the caller's volume has fewer than two distinct knots.
    /// [`ErrorKind::InvalidInput`].
    TooFewKnots {
        /// The distinct knots.
        knots: usize,
    },
    /// The parameter box given to a bend volume's trim has a NaN bound, a degenerate
    /// infinite v range or a negative volume, or the bend's own numbers are NaN.
    /// [`ErrorKind::InvalidInput`].
    BendBoxMalformed,
    /// The surface in volume does not own its base surface, so it cannot edit it.
    /// [`ErrorKind::InvalidInput`].
    BaseSurfaceNotOwned,
    /// A derivative of an order above `max` was asked of a form that evaluates up to `max`.
    /// [`ErrorKind::InvalidInput`].
    DerivativeOrderTooHigh {
        /// The highest order the form evaluates.
        max: usize,
    },
    /// The volume type read from a file is none the reader knows.
    /// [`ErrorKind::InvalidInput`].
    UnknownVolumeType {
        /// The type read.
        volume_type: i64,
    },
    /// The surface given has no NURB form, which the routine reads.
    /// [`ErrorKind::InvalidInput`].
    NoNurb,
    /// The caller's B-spline curve has no NURB data. [`ErrorKind::InvalidInput`].
    CurveWithoutNurb,
    /// The caller's B-spline curve has no control points. [`ErrorKind::InvalidInput`].
    CurveWithoutControlPoints,
    /// The parameter box reaches no further out than the bend's singularity: its largest
    /// u, `u_max`, is below the bend's legal minimum `limit`.
    /// [`ErrorKind::OutsideDomain`].
    BoxOutsideBendHalfSpace {
        /// The box's largest u.
        u_max: f64,
        /// The bend's singularity tolerance.
        limit: f64,
    },
    /// A bend of zero period takes only v = 0, and the box's v range misses it by more
    /// than the kernel's zero. [`ErrorKind::OutsideDomain`].
    BoxOutsideZeroPeriod {
        /// The box's smallest v.
        v_min: f64,
        /// The box's largest v.
        v_max: f64,
    },
    /// The point is outside the volume's parameter domain.
    /// [`ErrorKind::OutsideDomain`].
    PointOutsideVolumeDomain {
        /// The point.
        point: [f64; 3],
    },
    /// The point is outside the parameter domain the caller gave.
    /// [`ErrorKind::OutsideDomain`].
    PointOutsideGivenDomain {
        /// The point.
        point: [f64; 3],
    },
    /// The parameter box is outside the volume's natural parameter domain.
    /// [`ErrorKind::OutsideDomain`].
    BoxOutsideParamDomain,
    /// The in-space pseudo box maps outside the volume's natural parameter domain.
    /// [`ErrorKind::OutsideDomain`].
    PseudoBoxOutsideDomain,
    /// The surface in volume evaluates its u and v derivatives up to orders `u` and `v`
    /// only together as upstream supports them (equal orders with the upper half only or
    /// at most 1, or one of them 0). [`ErrorKind::Unsupported`].
    DerivativeCombinationUnsupported {
        /// The highest u derivative asked.
        u: usize,
        /// The highest v derivative asked.
        v: usize,
    },
    /// A plain volume (upstream's base `SmVolume`) has no code for this: only the derived
    /// volumes copy, evaluate and invert. [`ErrorKind::Unsupported`].
    PlainVolume,
    /// B-spline volumes are not supported: upstream's build has no volume NURBS library
    /// (`USE_VOLUMES`), so they do not evaluate, transform, read or write.
    /// [`ErrorKind::Unsupported`].
    BSplineVolumeUnsupported,
    /// The point lies within the bend's singularity (its distance from the bend axis is
    /// below the bend's legal minimum), where the bend has no inverse.
    /// [`ErrorKind::Degenerate`].
    PointInBendSingularity,
    /// A tangent of the volume has zero length at the point: its squared length is below
    /// the kernel's zero squared. [`ErrorKind::Degenerate`].
    ZeroVolumeTangent {
        /// The shortest tangent's squared length.
        length_squared: f64,
    },
    /// The volume's inversion of the point found no parameter point.
    /// [`ErrorKind::Failed`].
    NoVolumeInverse,
    /// A routine this one calls failed; the failure keeps that routine's error as its
    /// cause. [`ErrorKind::Failed`].
    CallFailed,
    /// The surface cache is of a kind that does not do this: the base cache answers the
    /// trimmed cache's queries with a refusal ([`ErrorKind::Unsupported`], `SM_ERR`).
    UnsupportedCacheKind,
    /// The surface's cache could not be made; the failure that stopped it is the cause
    /// (`SM_ERR_NULL_POINTER`, class [`ErrorKind::Failed`]: its defect-ness follows the
    /// cause).
    SurfaceCacheNotBuilt,
    /// The surface cache's tree could not be built; the failure that stopped it, where
    /// there was one, is the cause (`SM_ERR_NULL_POINTER`, class [`ErrorKind::Failed`]).
    SurfaceCacheTreeNotBuilt,
    /// The surface cache the caller gave has no tree to solve on
    /// ([`ErrorKind::InvalidInput`], `SM_ERR_NULL_POINTER`).
    SurfaceCacheHasNoTree,
    /// The curve's cache could not be made; the failure that stopped it is the cause
    /// (`SM_ERR_NULL_POINTER`, class [`ErrorKind::Failed`]).
    CurveCacheNotBuilt,
    /// The curve cache has no tree (`SM_ERR_NULL_POINTER`, class [`ErrorKind::Failed`]).
    CurveCacheTreeNotBuilt,
    /// The cubic approximation of a surface was still farther from it than the tolerance
    /// after the last subdivision pass: a solve that stops with its numbers, a defect
    /// (guide §6 rule 3; class [`ErrorKind::StageFailed`], `SM_ERR`).
    ApproximationNotConverged {
        /// The largest distance of the approximation from the surface it sampled.
        achieved: f64,
        /// The distance asked for.
        tolerance: f64,
    },
    /// The caller's parameter lies in none of the curve's segments, which cover its
    /// interval ([`ErrorKind::InvalidInput`], `SM_ERR`).
    ParameterOutsideInterval {
        /// The parameter.
        param: f64,
    },
    /// A first derivative (tangent) of the curve or surface is of zero length where the
    /// routine needs its direction ([`ErrorKind::Degenerate`], `SM_ERR`).
    ZeroLengthTangent,
    /// The surface has no normal at the point ([`ErrorKind::Degenerate`], `SM_ERR`).
    NoNormal,
    /// The two surfaces' normals are parallel, so they span no plane
    /// ([`ErrorKind::Degenerate`], `SM_ERR`).
    ParallelNormals,
    /// The normals around a singular point sum to zero, so they give no normal there
    /// ([`ErrorKind::Degenerate`]; upstream's status `SM_ERR_INVALID_INPUT`).
    NormalsCancel,
    /// The curve's tangent is along the surface normal, so there is no binormal
    /// ([`ErrorKind::Degenerate`], `SM_ERR`).
    TangentAlongNormal,
    /// The solver's caller gave fewer variables than its equations take
    /// ([`ErrorKind::InvalidInput`], `SM_ERR`).
    TooFewSolverVariables {
        /// The number of variables the equations take.
        needed: usize,
    },
    /// An input the routine needs was not given by its caller
    /// ([`ErrorKind::InvalidInput`], `SM_ERR_NULL_POINTER`).
    InputNotGiven {
        /// Which input.
        what: SolveInput,
    },
    /// The trimmed surface cache that point testing reads has no face to test against
    /// ([`ErrorKind::InvalidInput`], `SM_ERR_NULL_POINTER`).
    TrimmedCacheHasNoFace,
    /// An end point of the caller's UV line lies outside the surface's UV domain
    /// ([`ErrorKind::InvalidInput`], `SM_ERR`).
    EndPointOutsideDomain,
    /// The surface has fewer than two distinct knots in a direction, so it has no span
    /// there ([`ErrorKind::Degenerate`], `SM_ERR`).
    TooFewUniqueKnots,
    /// The derivative surface definition's tangency type is not one a derivative surface
    /// is made for ([`ErrorKind::Unsupported`], `SM_ERR`).
    UnsupportedTangencyType,
    /// The caller's derivative surface definition fails its validity check
    /// ([`ErrorKind::InvalidInput`], `SM_ERR`).
    InvalidDerivativeSurface,
    /// A point of the boundary curve does not drop onto the surface
    /// ([`ErrorKind::Failed`], `SM_ERR`).
    PointDoesNotDrop,
    /// A 3d direction has no direction in the surface's parameters at the point; the
    /// failure is the cause ([`ErrorKind::Failed`], `SM_ERR`).
    NoParameterDirection,
    /// The tangent reference curve's point lies on the boundary curve, so it gives no
    /// direction ([`ErrorKind::Degenerate`], `SM_ERR`).
    ReferencePointOnBoundary,
    /// No sampled tangent of the singular direction points along the given direction
    /// ([`ErrorKind::Failed`], `SM_ERR`).
    NoTangentTowardDirection,
    /// The point is at a pole of the surface, where the seam test does not apply
    /// ([`ErrorKind::Unsupported`], `SM_ERR`).
    AtPole,
    /// The caller's surface is not of a B-spline kind, which the routine reads as one
    /// ([`ErrorKind::Unsupported`], `SM_ERR_NULL_POINTER`).
    SurfaceNotBSplineKind,
    /// The caller's curve is not of a B-spline kind, which the routine reads as one
    /// ([`ErrorKind::Unsupported`], `SM_ERR_NULL_POINTER`).
    CurveNotBSplineKind,
    /// The point solver's surface is not a B-spline surface, so it has no control net
    /// to guess from ([`ErrorKind::Unsupported`], `SM_ERR`).
    SolverSurfaceNotBSpline,
    /// The B-spline surface's control points could not be read; the failure is the
    /// cause (`SM_ERR`, class [`ErrorKind::Failed`]: its defect-ness follows the cause).
    NoControlPoints,
    /// The three points' pairwise normals do not all point one way, so they give no
    /// plane ([`ErrorKind::Degenerate`], `SM_ERR`).
    PointsDoNotTurnOneWay,
    /// The alternate cache holds no curves of the kind (3D or UV) asked for
    /// ([`ErrorKind::InvalidInput`], `SM_ERR_NULL_POINTER`).
    CurvesNotCached,
    /// The surface's kind has no override of the routine; the base body refuses
    /// ([`ErrorKind::Unsupported`], `SM_ERR`).
    UnsupportedSurfaceKind,
    /// The caller's face has no surface ([`ErrorKind::InvalidInput`],
    /// `SM_ERR_NULL_POINTER`).
    FaceHasNoSurface,
    /// The solver's operation is one this step does not help with
    /// ([`ErrorKind::Unsupported`], `SM_ERR`).
    UnsupportedSolverOperation,
    /// Neither the control-net guess nor the grid guess gave a starting point; the
    /// failure, where there was one, is the cause ([`ErrorKind::NotFound`], `SM_ERR`).
    NoRoughGuess,
    /// The stepper's guess lies outside the domain and it has no step to move back along
    /// ([`ErrorKind::OutsideDomain`], `SM_ERR`).
    ZeroStepOutsideDomain,
    /// The caller asked a line search for the zero of an absolute error norm, which
    /// cannot change sign ([`ErrorKind::InvalidInput`], `SM_ERR`).
    LineSearchNormUnsigned,
    /// The caller's two points do not bracket a zero of the error norm: it has one sign
    /// at both ([`ErrorKind::InvalidInput`], `SM_ERR`).
    LineSearchNotBracketed,
    /// The face's UV domain, grown from its edge uses, and the surface's are disjoint
    /// ([`ErrorKind::InvalidInput`]; upstream's status `SM_ERR_INVALID_INPUT`, which the
    /// cache factory falls back from).
    DisjointFaceDomain,
    /// The caller's edge use has no UV trim curve ([`ErrorKind::InvalidInput`],
    /// `SM_ERR_NULL_POINTER`).
    EdgeuseHasNoTrimCurve,
    /// The surface cache already holds [`MAX_ALT_CACHES`](crate::surface::surface_cache::MAX_ALT_CACHES)
    /// alternate caches or more, so a new one has no room: the list is the caller's through
    /// `alt_caches_mut` ([`ErrorKind::InvalidInput`], `SM_ERR`).
    AltCachesFull,
    /// The caller's node is not a surface node, so it has no patch to test
    /// ([`ErrorKind::InvalidInput`], `SM_ERR`).
    NodeNotSurfaceNode,
    /// The caller's node already has children, so it is not a leaf to test
    /// ([`ErrorKind::InvalidInput`], `SM_ERR`).
    NodeHasChildren,
    /// The caller's node has no surface to subdivide
    /// ([`ErrorKind::InvalidInput`], `SM_ERR_NULL_POINTER`).
    NodeHasNoSurface,
}

/// An input of a surface routine that its caller did not give
/// ([`SurfaceFailure::InputNotGiven`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SolveInput {
    /// The surface.
    Surface,
    /// The first of two surfaces.
    FirstSurface,
    /// The second of two surfaces.
    SecondSurface,
    /// The plane of the solve.
    Plane,
    /// The surface cache of the solve.
    SurfaceCache,
    /// The target distance of a solve at a distance.
    TargetDistance,
    /// The direction vector of a directed minimize.
    DirectionVector,
    /// The tangent reference curve of a derivative surface.
    TangentReference,
}

impl core::fmt::Display for SolveInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Surface => "the surface",
            Self::FirstSurface => "the first surface",
            Self::SecondSurface => "the second surface",
            Self::Plane => "the plane",
            Self::SurfaceCache => "the surface cache",
            Self::TargetDistance => "the target distance",
            Self::DirectionVector => "the direction vector",
            Self::TangentReference => "the tangent reference curve",
        })
    }
}

/// What a solver lacks ([`SurfaceFailure::SolverWithout`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SolverPart {
    /// The cache of the surface solved for.
    SurfaceCache,
    /// One of the two surfaces' caches.
    SurfaceCaches,
    /// The curve solved for.
    Curve,
    /// The curve cache's tree.
    CurveTree,
}

impl core::fmt::Display for SolverPart {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::SurfaceCache => "the surface's cache",
            Self::SurfaceCaches => "a surface's cache",
            Self::Curve => "the curve",
            Self::CurveTree => "the curve cache's tree",
        })
    }
}

impl SurfaceFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_)
            | Self::Defect { .. }
            | Self::DerivativeOrderTooHigh { .. }
            | Self::UnsupportedCacheKind
            | Self::ApproximationNotConverged { .. }
            | Self::ParameterOutsideInterval { .. }
            | Self::ZeroLengthTangent
            | Self::NoNormal
            | Self::ParallelNormals
            | Self::TangentAlongNormal
            | Self::TooFewSolverVariables { .. }
            | Self::EndPointOutsideDomain
            | Self::TooFewUniqueKnots
            | Self::UnsupportedTangencyType
            | Self::InvalidDerivativeSurface
            | Self::PointDoesNotDrop
            | Self::NoParameterDirection
            | Self::ReferencePointOnBoundary
            | Self::NoTangentTowardDirection
            | Self::AtPole
            | Self::SolverSurfaceNotBSpline
            | Self::NoControlPoints
            | Self::PointsDoNotTurnOneWay
            | Self::UnsupportedSurfaceKind
            | Self::UnsupportedSolverOperation
            | Self::NoRoughGuess
            | Self::ZeroStepOutsideDomain
            | Self::LineSearchNormUnsigned
            | Self::LineSearchNotBracketed
            | Self::AltCachesFull
            | Self::NodeNotSurfaceNode
            | Self::NodeHasChildren => SM_ERR,
            Self::Missing(_)
            | Self::SolverWithout { .. }
            | Self::SolveCacheNotMade
            | Self::NoNurb
            | Self::SurfaceCacheNotBuilt
            | Self::SurfaceCacheTreeNotBuilt
            | Self::CurveCacheNotBuilt
            | Self::CurveCacheTreeNotBuilt
            | Self::InputNotGiven { .. }
            | Self::TrimmedCacheHasNoFace
            | Self::SurfaceCacheHasNoTree
            | Self::SurfaceNotBSplineKind
            | Self::CurveNotBSplineKind
            | Self::CurvesNotCached
            | Self::FaceHasNoSurface
            | Self::EdgeuseHasNoTrimCurve
            | Self::NodeHasNoSurface => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_)
            | Self::LineDirectionZero
            | Self::NormalsCancel
            | Self::DisjointFaceDomain => SM_ERR_INVALID_INPUT,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::OffSurface(_) => SM_ERR_NOT_WITHIN_TOLERANCE,
            _ => SM_ERR,
        }
    }

    /// The failure's stable [`ErrorKind`]: the kind of the status it replaced
    /// ([`ErrorKind::Failed`], [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`],
    /// [`ErrorKind::OutsideDomain`], [`ErrorKind::NotWithinTolerance`]).
    /// [`SurfaceFailure::Defect`] is [`ErrorKind::StageFailed`], a defect. Each named
    /// cause has the class its doc names.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::SolverWithout { .. }
            | Self::LineDirectionZero
            | Self::EmptyDomain
            | Self::BlendDefinitionNotFromBoundary
            | Self::TooFewKnots { .. }
            | Self::BendBoxMalformed
            | Self::BaseSurfaceNotOwned
            | Self::DerivativeOrderTooHigh { .. }
            | Self::UnknownVolumeType { .. }
            | Self::NoNurb
            | Self::CurveWithoutNurb
            | Self::CurveWithoutControlPoints => ErrorKind::InvalidInput,
            Self::BoxOutsideBendHalfSpace { .. }
            | Self::BoxOutsideZeroPeriod { .. }
            | Self::PointOutsideVolumeDomain { .. }
            | Self::PointOutsideGivenDomain { .. }
            | Self::BoxOutsideParamDomain
            | Self::PseudoBoxOutsideDomain => ErrorKind::OutsideDomain,
            Self::IsoCurveKindNotLiftable
            | Self::DerivativeCombinationUnsupported { .. }
            | Self::PlainVolume
            | Self::BSplineVolumeUnsupported => ErrorKind::Unsupported,
            Self::PointInBendSingularity | Self::ZeroVolumeTangent { .. } => ErrorKind::Degenerate,
            Self::SolveCacheNotMade
            | Self::BlendSurfaceNotMade
            | Self::MergedRangeReversed { .. }
            | Self::KnotsAndContinuitiesDisagree { .. }
            | Self::DegenerateParameterNotFound
            | Self::DistanceNotEnclosed
            | Self::FartherThanGuess { .. }
            | Self::SingularBoundaryNotPole { .. }
            | Self::RecursionPastBound { .. }
            | Self::NoContinuousDomain
            | Self::NothingLifted
            | Self::NoVolumeInverse
            | Self::CallFailed => ErrorKind::Failed,
            Self::ApproximationNotConverged { .. } => ErrorKind::StageFailed,
            Self::ParameterOutsideInterval { .. }
            | Self::TooFewSolverVariables { .. }
            | Self::InputNotGiven { .. }
            | Self::TrimmedCacheHasNoFace
            | Self::SurfaceCacheHasNoTree
            | Self::EndPointOutsideDomain
            | Self::InvalidDerivativeSurface
            | Self::CurvesNotCached
            | Self::FaceHasNoSurface
            | Self::LineSearchNormUnsigned
            | Self::LineSearchNotBracketed
            | Self::DisjointFaceDomain
            | Self::EdgeuseHasNoTrimCurve
            | Self::AltCachesFull
            | Self::NodeNotSurfaceNode
            | Self::NodeHasChildren
            | Self::NodeHasNoSurface => ErrorKind::InvalidInput,
            Self::UnsupportedCacheKind
            | Self::UnsupportedTangencyType
            | Self::AtPole
            | Self::SurfaceNotBSplineKind
            | Self::CurveNotBSplineKind
            | Self::SolverSurfaceNotBSpline
            | Self::UnsupportedSurfaceKind
            | Self::UnsupportedSolverOperation => ErrorKind::Unsupported,
            Self::SurfaceCacheNotBuilt
            | Self::SurfaceCacheTreeNotBuilt
            | Self::CurveCacheNotBuilt
            | Self::CurveCacheTreeNotBuilt
            | Self::PointDoesNotDrop
            | Self::NoParameterDirection
            | Self::NoTangentTowardDirection
            | Self::NoControlPoints => ErrorKind::Failed,
            Self::ZeroLengthTangent
            | Self::NoNormal
            | Self::ParallelNormals
            | Self::NormalsCancel
            | Self::TangentAlongNormal
            | Self::TooFewUniqueKnots
            | Self::ReferencePointOnBoundary
            | Self::PointsDoNotTurnOneWay => ErrorKind::Degenerate,
            Self::NoRoughGuess => ErrorKind::NotFound,
            Self::ZeroStepOutsideDomain => ErrorKind::OutsideDomain,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for SurfaceFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what)
            | Self::InvalidValue(what)
            | Self::OutsideDomain(what)
            | Self::OffSurface(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::SolverWithout { part } => write!(f, "the solver lacks {part}"),
            Self::SolveCacheNotMade => f.write_str("the solve's cache or its tree could not be made"),
            Self::LineDirectionZero => f.write_str("the line direction is the zero vector"),
            Self::EmptyDomain => f.write_str("a surface has an empty domain"),
            Self::BlendDefinitionNotFromBoundary => {
                f.write_str("the blend definition is not from a surface boundary curve")
            }
            Self::BlendSurfaceNotMade => f.write_str("the blend surface could not be made"),
            Self::MergedRangeReversed { start, end } => {
                write!(f, "a merged solution range is reversed: from {start} to {end}")
            }
            Self::KnotsAndContinuitiesDisagree { knots, continuities } => write!(
                f,
                "the knots and their continuities disagree: {knots} knots, {continuities} continuities"
            ),
            Self::DegenerateParameterNotFound => {
                f.write_str("no degenerate parameter toward the point was found")
            }
            Self::DistanceNotEnclosed => {
                f.write_str("a distance from the test point has no enclosure")
            }
            Self::FartherThanGuess { found, guess } => write!(
                f,
                "the point found is farther from the test point than the guess: at least {found}, the guess at most {guess}"
            ),
            // The phrase the site reported before it was named, kept word for word: the
            // wr57 replay tape (`goldens` `wr57_replay`, `heal.heal_data_p3`) hashes this
            // error's message as the answer at its substituted calls. The bounds stay in
            // the fields.
            Self::SingularBoundaryNotPole { .. } => f.write_str(
                "a point of the singular boundary is nearer the test point than the point found: the boundary is not a pole",
            ),
            Self::RecursionPastBound { bound } => {
                write!(f, "the local intersection recursed past its depth bound {bound}")
            }
            Self::IsoCurveKindNotLiftable => f.write_str("the iso curve's kind cannot be lifted"),
            Self::NoContinuousDomain => {
                f.write_str("a segment has no surface domain at least C1 around it")
            }
            Self::NothingLifted => f.write_str("no segment was lifted"),
            Self::TooFewKnots { knots } => write!(
                f,
                "the knot vector has {knots} distinct knots, fewer than two"
            ),
            Self::BendBoxMalformed => f.write_str(
                "the box has a NaN bound, a degenerate infinite v range or a negative volume, or the bend's numbers are NaN",
            ),
            Self::BaseSurfaceNotOwned => {
                f.write_str("the surface in volume does not own its base surface")
            }
            Self::DerivativeOrderTooHigh { max } => {
                write!(f, "a derivative order above {max} was asked")
            }
            Self::UnknownVolumeType { volume_type } => {
                write!(f, "the volume type {volume_type} is unknown")
            }
            Self::NoNurb => f.write_str("the surface has no NURB form"),
            Self::CurveWithoutNurb => f.write_str("the B-spline curve has no NURB"),
            Self::CurveWithoutControlPoints => f.write_str("the curve has no control points"),
            Self::BoxOutsideBendHalfSpace { u_max, limit } => write!(
                f,
                "the box's largest u, {u_max:e}, is below the bend's singularity tolerance {limit:e}"
            ),
            Self::BoxOutsideZeroPeriod { v_min, v_max } => write!(
                f,
                "the box's v range [{v_min:e}, {v_max:e}] misses the bend's zero period"
            ),
            Self::PointOutsideVolumeDomain { point: [x, y, z] } => write!(
                f,
                "the point ({x}, {y}, {z}) is outside the volume's parameter domain"
            ),
            Self::PointOutsideGivenDomain { point: [x, y, z] } => write!(
                f,
                "the point ({x}, {y}, {z}) is outside the given parameter domain"
            ),
            Self::BoxOutsideParamDomain => {
                f.write_str("the parameter box is outside the parameter domain")
            }
            Self::PseudoBoxOutsideDomain => {
                f.write_str("the pseudo box maps outside the in-space domain")
            }
            Self::DerivativeCombinationUnsupported { u, v } => write!(
                f,
                "derivatives up to order {u} in u and {v} in v are not supported together"
            ),
            Self::PlainVolume => f.write_str(
                "a plain volume has no code for this; only the derived volumes do",
            ),
            Self::BSplineVolumeUnsupported => f.write_str(
                "B-spline volumes are not supported (upstream's build has no volume NURBS library)",
            ),
            Self::PointInBendSingularity => {
                f.write_str("the point lies within the bend's singularity")
            }
            Self::ZeroVolumeTangent { length_squared } => write!(
                f,
                "a volume tangent has zero length (its squared length is {length_squared:e})"
            ),
            Self::NoVolumeInverse => f.write_str("the volume's inversion found no point"),
            Self::CallFailed => f.write_str("a routine it calls failed"),
            Self::UnsupportedCacheKind => {
                f.write_str("the surface cache's kind does not support the operation")
            }
            Self::SurfaceCacheNotBuilt => f.write_str("the surface's cache could not be made"),
            Self::SurfaceCacheTreeNotBuilt => {
                f.write_str("the surface cache's tree could not be built")
            }
            Self::SurfaceCacheHasNoTree => f.write_str("the surface cache given has no tree"),
            Self::CurveCacheNotBuilt => f.write_str("the curve's cache could not be made"),
            Self::CurveCacheTreeNotBuilt => f.write_str("the curve cache has no tree"),
            Self::ApproximationNotConverged {
                achieved,
                tolerance,
            } => write!(
                f,
                "the approximation is {achieved:e} from the surface after its last pass, above the tolerance {tolerance:e}"
            ),
            Self::ParameterOutsideInterval { param } => {
                write!(f, "the parameter {param} lies in none of the curve's segments")
            }
            Self::ZeroLengthTangent => f.write_str("a tangent has zero length"),
            Self::NoNormal => f.write_str("the surface has no normal at the point"),
            Self::ParallelNormals => f.write_str("the surface normals are parallel"),
            Self::NormalsCancel => f.write_str("the normals around the singularity sum to zero"),
            Self::TangentAlongNormal => {
                f.write_str("the curve tangent is along the surface normal")
            }
            Self::TooFewSolverVariables { needed } => {
                write!(f, "the solver was given fewer than {needed} variables")
            }
            Self::InputNotGiven { what } => write!(f, "{what} was not given"),
            Self::TrimmedCacheHasNoFace => f.write_str("the trimmed surface cache has no face"),
            Self::EndPointOutsideDomain => {
                f.write_str("an end point is outside the surface's UV domain")
            }
            Self::TooFewUniqueKnots => {
                f.write_str("the surface has fewer than two distinct knots in the direction")
            }
            Self::UnsupportedTangencyType => {
                f.write_str("the tangency type has no derivative surface")
            }
            Self::InvalidDerivativeSurface => {
                f.write_str("the derivative surface definition is not valid")
            }
            Self::PointDoesNotDrop => {
                f.write_str("the curve point does not drop onto the surface")
            }
            Self::NoParameterDirection => {
                f.write_str("the direction has no direction in the surface's parameters")
            }
            Self::ReferencePointOnBoundary => {
                f.write_str("the reference curve point lies on the boundary curve")
            }
            Self::NoTangentTowardDirection => {
                f.write_str("every sample tangent points away from the direction")
            }
            Self::AtPole => f.write_str("the point is at a pole"),
            Self::SurfaceNotBSplineKind => f.write_str("the surface is not of a B-spline kind"),
            Self::CurveNotBSplineKind => f.write_str("the curve is not of a B-spline kind"),
            Self::SolverSurfaceNotBSpline => {
                f.write_str("the solver's surface is not a B-spline surface")
            }
            Self::NoControlPoints => {
                f.write_str("the B-spline surface's control points could not be read")
            }
            Self::PointsDoNotTurnOneWay => f.write_str("the three points do not turn one way"),
            Self::CurvesNotCached => {
                f.write_str("the alternate cache holds no curves of the kind asked for")
            }
            Self::UnsupportedSurfaceKind => {
                f.write_str("the surface's kind does not support the operation")
            }
            Self::FaceHasNoSurface => f.write_str("the face has no surface"),
            Self::UnsupportedSolverOperation => {
                f.write_str("the solver's operation gets no help from this step")
            }
            Self::NoRoughGuess => f.write_str("no rough guess was found"),
            Self::ZeroStepOutsideDomain => {
                f.write_str("the guess is outside the domain and the step is zero")
            }
            Self::LineSearchNormUnsigned => {
                f.write_str("the error norm is an absolute norm, which cannot change sign")
            }
            Self::LineSearchNotBracketed => {
                f.write_str("the error norm does not change sign over the step")
            }
            Self::DisjointFaceDomain => {
                f.write_str("the face's domain and the surface's are disjoint")
            }
            Self::EdgeuseHasNoTrimCurve => f.write_str("the edge use has no UV trim curve"),
            Self::AltCachesFull => {
                f.write_str("the surface cache holds as many alternate caches as its limit")
            }
            Self::NodeNotSurfaceNode => f.write_str("the node is not a surface node"),
            Self::NodeHasChildren => f.write_str("the node already has children"),
            Self::NodeHasNoSurface => f.write_str("the node has no surface"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a surface routine that fails with `failure`.
    #[must_use]
    pub(crate) fn surface(operation: &'static str, failure: SurfaceFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a surface routine that fails with `failure` about
    /// `entity`.
    #[must_use]
    pub(crate) fn surface_at(
        operation: &'static str,
        failure: SurfaceFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a surface routine that could not continue, as the
    /// phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn surface_failed(operation: &'static str, what: &'static str) -> Self {
        Self::surface(operation, SurfaceFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a surface routine needs that is not
    /// there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)`
    /// on a `NULL` pointer).
    #[must_use]
    pub(crate) fn surface_missing(operation: &'static str, what: &'static str) -> Self {
        Self::surface(operation, SurfaceFailure::Missing(what))
    }

    /// The surface failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Surface`].
    #[must_use]
    pub fn surface_failure(&self) -> Option<&SurfaceFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Surface(s) => Some(s),
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
    fn surface_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (SurfaceFailure::Failed("x"), SM_ERR),
            (SurfaceFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (SurfaceFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (SurfaceFailure::OutsideDomain("x"), SM_ERR_OUTSIDE_OF_DOMAIN),
            (SurfaceFailure::OffSurface("x"), SM_ERR_NOT_WITHIN_TOLERANCE),
        ];
        for (failure, code) in cases {
            let typed = SolidError::surface("surface::op", failure);
            let before = SolidError::Upstream {
                operation: "surface::op",
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
            assert_eq!(typed.surface_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::surface_missing("surface::op", "the surface's cache").to_string(),
            "surface::op: the surface's cache is missing"
        );
        assert_eq!(
            SolidError::surface_failed("surface::op", "the generator has no NURB form").to_string(),
            "surface::op: the generator has no NURB form"
        );
        assert_eq!(
            SolidError::surface(
                "surface::op",
                SurfaceFailure::OffSurface("the point is off")
            )
            .to_string(),
            "surface::op: the point is off"
        );
        assert_eq!(SolidError::sm_err("surface::op").surface_failure(), None);
    }

    /// The named causes of the general surface evaluation (TE-O-07) keep the status of the
    /// phrase cause each replaced, with their own class.
    #[test]
    fn the_surface_evaluation_causes_keep_their_status_with_their_class() {
        use ErrorKind as K;
        let cases = [
            (
                SurfaceFailure::DerivativeOrderTooHigh { max: 3 },
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::NoNurb, SM_ERR_NULL_POINTER, K::InvalidInput),
            (SurfaceFailure::UnsupportedCacheKind, SM_ERR, K::Unsupported),
            (
                SurfaceFailure::SurfaceCacheNotBuilt,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                SurfaceFailure::SurfaceCacheTreeNotBuilt,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                SurfaceFailure::SurfaceCacheHasNoTree,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::CurveCacheNotBuilt,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                SurfaceFailure::CurveCacheTreeNotBuilt,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                SurfaceFailure::ApproximationNotConverged {
                    achieved: 2e-3,
                    tolerance: 1e-3,
                },
                SM_ERR,
                K::StageFailed,
            ),
            (
                SurfaceFailure::ParameterOutsideInterval { param: 2.0 },
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::ZeroLengthTangent, SM_ERR, K::Degenerate),
            (SurfaceFailure::NoNormal, SM_ERR, K::Degenerate),
            (SurfaceFailure::ParallelNormals, SM_ERR, K::Degenerate),
            (
                SurfaceFailure::NormalsCancel,
                SM_ERR_INVALID_INPUT,
                K::Degenerate,
            ),
            (SurfaceFailure::TangentAlongNormal, SM_ERR, K::Degenerate),
            (
                SurfaceFailure::TooFewSolverVariables { needed: 2 },
                SM_ERR,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::InputNotGiven {
                    what: SolveInput::Plane,
                },
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::TrimmedCacheHasNoFace,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::EndPointOutsideDomain,
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::TooFewUniqueKnots, SM_ERR, K::Degenerate),
            (
                SurfaceFailure::UnsupportedTangencyType,
                SM_ERR,
                K::Unsupported,
            ),
            (
                SurfaceFailure::InvalidDerivativeSurface,
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::PointDoesNotDrop, SM_ERR, K::Failed),
            (SurfaceFailure::NoParameterDirection, SM_ERR, K::Failed),
            (
                SurfaceFailure::ReferencePointOnBoundary,
                SM_ERR,
                K::Degenerate,
            ),
            (SurfaceFailure::NoTangentTowardDirection, SM_ERR, K::Failed),
            (SurfaceFailure::AtPole, SM_ERR, K::Unsupported),
        ];
        for (failure, code, kind) in cases {
            let typed = SolidError::surface("surface::op", failure);
            assert_eq!(typed.sm_status(), Some(code), "{failure:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(typed.kind(), kind, "{failure:?}");
            assert!(typed.is_status_answer(), "{failure:?}");
            assert_eq!(typed.is_defect(), kind == K::StageFailed, "{failure:?}");
            assert_eq!(typed.surface_failure(), Some(&failure));
            assert!(!typed.to_string().is_empty());
        }
        assert_eq!(
            SolidError::surface(
                "surface::op",
                SurfaceFailure::InputNotGiven {
                    what: SolveInput::TargetDistance
                }
            )
            .to_string(),
            "surface::op: the target distance was not given"
        );
        // a cache failure's defect-ness follows its cause
        let defect =
            SolidError::surface("surface::inner", SurfaceFailure::Defect { why: "the step" });
        let chained = Failure::new("surface::op", SurfaceFailure::SurfaceCacheNotBuilt)
            .caused_by(defect)
            .into_error();
        assert!(chained.is_defect());
        assert_eq!(chained.sm_status(), Some(SM_ERR_NULL_POINTER));
    }

    /// A sample of the second set of converted files (`extend` and its sides, `side`,
    /// `implicit`, the volumes, `srf_in_volume`, `procedural_blend` and its offset,
    /// `trim_srf_cache`): each keeps the operation, status, kind and entity of its
    /// `Upstream`.
    #[test]
    fn the_extend_volume_and_blend_samples_keep_their_upstream_answers() {
        let samples = [
            (
                "surface::extend::analytic::analytic_extended_sides",
                SurfaceFailure::Failed("the joint is not an interior knot"),
                SM_ERR,
            ),
            (
                "surface::extend::ball",
                SurfaceFailure::Failed("the B-spline surface has no NURB"),
                SM_ERR,
            ),
            (
                "surface::volume::p1::Volume::copy",
                SurfaceFailure::Failed("a plain volume cannot be copied"),
                SM_ERR,
            ),
            (
                "surface::srf_in_volume",
                SurfaceFailure::Failed("the volume's inversion failed"),
                SM_ERR,
            ),
            (
                "surface::procedural_blend::ProceduralBlend::read_from_db",
                SurfaceFailure::Missing("the input stream"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "surface::trim_srf_cache::TrimSrfCache::build_tree_with_face",
                SurfaceFailure::DisjointFaceDomain,
                SM_ERR_INVALID_INPUT,
            ),
            (
                "surface::side",
                SurfaceFailure::Failed("the iso curve has no NURB"),
                SM_ERR,
            ),
        ];
        for (operation, failure, code) in samples {
            let typed = SolidError::surface(operation, failure);
            let before = SolidError::Upstream {
                operation,
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(typed.kind(), before.kind(), "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
        }
        // the re-certification refusal keeps the face the bare status named
        let face = EntityRef::Face(crate::topology::FaceId::from_index(3));
        let typed = SolidError::surface_at(
            "surface::procedural_blend::recertify_faces",
            SurfaceFailure::Failed(
                "the face's net is not proved within its bound of its definition",
            ),
            face,
        );
        let before = SolidError::Upstream {
            operation: "surface::procedural_blend::recertify_faces",
            code: SM_ERR,
            entity: Some(face),
        };
        assert_eq!(typed.sm_status(), before.sm_status());
        assert_eq!(typed.upstream_status(), Some(SM_ERR));
        assert_eq!(typed.kind(), before.kind());
        assert_eq!(typed.operation(), before.operation());
        assert_eq!(typed.entity(), Some(face));
        assert_eq!(typed.is_status_answer(), before.is_status_answer());
    }

    /// Each named cause keeps the status of the phrase it replaced and says its class; a
    /// cause that chains the stage's error keeps it.
    #[test]
    fn the_solver_causes_keep_their_status_and_say_their_class() {
        let op = "surface::op";
        let cases: [(SurfaceFailure, i32, ErrorKind); 17] = [
            (
                SurfaceFailure::SolverWithout {
                    part: SolverPart::SurfaceCache,
                },
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::SolveCacheNotMade,
                SM_ERR_NULL_POINTER,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::LineDirectionZero,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (SurfaceFailure::EmptyDomain, SM_ERR, ErrorKind::InvalidInput),
            (
                SurfaceFailure::BlendDefinitionNotFromBoundary,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::BlendSurfaceNotMade,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::MergedRangeReversed {
                    start: 1.0,
                    end: 0.5,
                },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::KnotsAndContinuitiesDisagree {
                    knots: 3,
                    continuities: 4,
                },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::DegenerateParameterNotFound,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::DistanceNotEnclosed,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::FartherThanGuess {
                    found: 2.0,
                    guess: 1.0,
                },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::SingularBoundaryNotPole {
                    boundary: 1.0,
                    found: 2.0,
                },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::RecursionPastBound { bound: 16 },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::IsoCurveKindNotLiftable,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                SurfaceFailure::NoContinuousDomain,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (SurfaceFailure::NothingLifted, SM_ERR, ErrorKind::Failed),
            (
                SurfaceFailure::SolverWithout {
                    part: SolverPart::CurveTree,
                },
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::surface(op, failure);
            assert_eq!(e.upstream_status(), Some(status), "{e:?}");
            assert_eq!(e.kind(), kind, "{e:?}");
            assert_eq!(e.operation(), op, "{e:?}");
            assert_eq!(e.surface_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{e:?}");
            assert!(e.to_string().starts_with(op), "{e}");
        }
        // the site's status, through `with_status`, keeps the class
        let e = Failure::new(op, SurfaceFailure::LineDirectionZero)
            .with_status(SM_ERR)
            .into_error();
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        // a chained cause is kept
        let stage = SolidError::invalid_input("stage::op", "bad");
        let e = Failure::new(op, SurfaceFailure::SolveCacheNotMade)
            .caused_by(stage.clone())
            .into_error();
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.cause(), Some(&stage));
    }

    /// The surface routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work or a later part of the slice.
    #[test]
    fn the_surface_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("surface", 51, &[]);
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the B-spline form the surface just converted to has no control net, which conversion always gives";
        let defect = SurfaceFailure::Defect { why };
        let op = "surface::op";
        super::super::tests::assert_defect(
            &SolidError::surface(op, defect),
            &SolidError::surface(op, SurfaceFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::surface(op, SurfaceFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.surface_failure(), Some(&defect));
    }

    /// The named causes of the surface caches and point solvers keep the status of the
    /// phrase they replace, with their own class.
    #[test]
    fn the_surface_cache_and_solver_causes_keep_their_status_with_their_class() {
        use crate::surface::surface::p7::chained;
        use SurfaceFailure as F;
        let cases = [
            (F::UnsupportedCacheKind, SM_ERR, ErrorKind::Unsupported),
            (
                F::TrimmedCacheHasNoFace,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                F::SurfaceNotBSplineKind,
                SM_ERR_NULL_POINTER,
                ErrorKind::Unsupported,
            ),
            (
                F::CurveNotBSplineKind,
                SM_ERR_NULL_POINTER,
                ErrorKind::Unsupported,
            ),
            (F::SolverSurfaceNotBSpline, SM_ERR, ErrorKind::Unsupported),
            (F::NoControlPoints, SM_ERR, ErrorKind::Failed),
            (F::PointsDoNotTurnOneWay, SM_ERR, ErrorKind::Degenerate),
            (
                F::CurvesNotCached,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (F::UnsupportedSurfaceKind, SM_ERR, ErrorKind::Unsupported),
            (
                F::FaceHasNoSurface,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                F::UnsupportedSolverOperation,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (F::NoRoughGuess, SM_ERR, ErrorKind::NotFound),
            (F::ZeroStepOutsideDomain, SM_ERR, ErrorKind::OutsideDomain),
            (F::LineSearchNormUnsigned, SM_ERR, ErrorKind::InvalidInput),
            (F::LineSearchNotBracketed, SM_ERR, ErrorKind::InvalidInput),
            (
                F::DisjointFaceDomain,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                F::EdgeuseHasNoTrimCurve,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (F::AltCachesFull, SM_ERR, ErrorKind::InvalidInput),
            (F::NodeNotSurfaceNode, SM_ERR, ErrorKind::InvalidInput),
            (F::NodeHasChildren, SM_ERR, ErrorKind::InvalidInput),
            (
                F::NodeHasNoSurface,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
        ];
        for (failure, code, kind) in cases {
            let typed = SolidError::surface("surface::op", failure);
            assert_eq!(typed.sm_status(), Some(code), "{failure:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(typed.kind(), kind, "{failure:?}");
            assert!(!typed.is_defect(), "{failure:?}");
            assert!(!typed.is_port_failure(), "{failure:?}");
            assert!(typed.is_status_answer(), "{failure:?}");
            assert_eq!(typed.surface_failure(), Some(&failure));
            assert!(!typed.to_string().is_empty());
        }
        // a caught cause is chained, a port failure too (which makes the answer one)
        let cause = SolidError::surface("surface::inner", F::NoRoughGuess);
        let caught = chained("surface::op", F::NoRoughGuess, cause);
        assert_eq!(caught.sm_status(), Some(SM_ERR));
        assert!(caught.is_status_answer());
        assert_eq!(
            caught.cause().and_then(SolidError::surface_failure),
            Some(&F::NoRoughGuess)
        );
        let port = SolidError::unported("x", "y");
        let kept = chained("surface::op", F::NoControlPoints, port.clone());
        assert_eq!(kept.sm_status(), Some(SM_ERR));
        assert_eq!(kept.cause(), Some(&port));
        assert!(kept.is_port_failure());
        assert!(!kept.is_status_answer());
    }

    /// The sites that build a surface phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_surface_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("SurfaceFailure", "Failed"),
                Builder::Variant("SurfaceFailure", "Missing"),
                Builder::Variant("SurfaceFailure", "InvalidValue"),
                Builder::Variant("SurfaceFailure", "OutsideDomain"),
                Builder::Variant("SurfaceFailure", "OffSurface"),
                Builder::Method("surface_failed", Some(1)),
                // the WR-57 replay seam's own constructor of a taped answer (feature
                // `wr57-replay`), which only the goldens call: not a kernel site
                Builder::Shared(
                    "solidcore/src/surface/surface/replay",
                    "surface_failed",
                    Some(1),
                ),
                Builder::Method("surface_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/surface/ball_patch.rs", 8),
            ("solidcore/src/surface/cone/p1.rs", 12),
            ("solidcore/src/surface/cone/p2.rs", 14),
            ("solidcore/src/surface/cone/p3.rs", 3),
            ("solidcore/src/surface/extend.rs", 2),
            ("solidcore/src/surface/extend/analytic.rs", 25),
            ("solidcore/src/surface/extend/ball.rs", 16),
            ("solidcore/src/surface/extend/blend.rs", 6),
            ("solidcore/src/surface/extend/sides.rs", 2),
            ("solidcore/src/surface/offset_surface.rs", 27),
            ("solidcore/src/surface/plane/p1.rs", 3),
            ("solidcore/src/surface/plane/p2.rs", 9),
            ("solidcore/src/surface/procedural_blend.rs", 9),
            ("solidcore/src/surface/procedural_blend/convert.rs", 13),
            ("solidcore/src/surface/procedural_blend/offset.rs", 5),
            ("solidcore/src/surface/side.rs", 7),
            ("solidcore/src/surface/sphere.rs", 16),
            ("solidcore/src/surface/surf_of_extrusion/p1.rs", 16),
            ("solidcore/src/surface/surf_of_extrusion/p2.rs", 13),
            ("solidcore/src/surface/surf_of_revolution/p1.rs", 10),
            ("solidcore/src/surface/surf_of_revolution/p2.rs", 6),
            ("solidcore/src/surface/surf_of_revolution/p3.rs", 21),
            ("solidcore/src/surface/surface/p2.rs", 21),
            ("solidcore/src/surface/torus.rs", 15),
        ];
        assert_pinned("surface phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
