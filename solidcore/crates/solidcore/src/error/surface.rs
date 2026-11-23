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
//! | the named causes of the volume surfaces and the implicit checks, [`SurfaceFailure::TooFewKnots`] to [`SurfaceFailure::CallFailed`] | `SM_ERR`, except [`SurfaceFailure::NoNurb`]: `SM_ERR_NULL_POINTER` (a site that reported `SM_ERR` keeps it through `with_status`) |
//! | [`SurfaceFailure::NoNurb`], [`SurfaceFailure::InputNotGiven`], [`SurfaceFailure::TrimmedCacheHasNoFace`], [`SurfaceFailure::SurfaceCacheHasNoTree`] | `SM_ERR_NULL_POINTER` (class [`ErrorKind::InvalidInput`]) |
//! | [`SurfaceFailure::SurfaceCacheNotBuilt`], [`SurfaceFailure::SurfaceCacheTreeNotBuilt`], [`SurfaceFailure::CurveCacheNotBuilt`], [`SurfaceFailure::CurveCacheTreeNotBuilt`] | `SM_ERR_NULL_POINTER` (class [`ErrorKind::Failed`]; defect-ness follows the cause) |
//! | [`SurfaceFailure::NormalsCancel`]  | `SM_ERR_INVALID_INPUT` (class [`ErrorKind::Degenerate`]) |
//! | [`SurfaceFailure::ScaleNotUniform`], [`SurfaceFailure::CurveWithoutNurb`], [`SurfaceFailure::BallDefinitionUnreadable`], [`SurfaceFailure::PointOnAxis`], [`SurfaceFailure::IsoCircleUndefined`], [`SurfaceFailure::ExtensionNotMadeInBracket`] | `SM_ERR` |
//! | [`SurfaceFailure::NoInputStream`] | `SM_ERR_NULL_POINTER` (or the site's, through `with_status`) |
//! | [`SurfaceFailure::BlendCurveNotBSpline`], [`SurfaceFailure::BlendDefinitionUnreadable`], [`SurfaceFailure::NetNotCertified`], [`SurfaceFailure::CurveDimensionTooLarge`], [`SurfaceFailure::NoBSplineForm`], [`SurfaceFailure::DoesNotContinueAsItself`], [`SurfaceFailure::OffsetNestedTooDeep`], [`SurfaceFailure::NoNormal`], [`SurfaceFailure::InvertedPointOffOffset`], [`SurfaceFailure::ExtensionTowardFailed`], [`SurfaceFailure::RevolutionArcOutOfRange`], [`SurfaceFailure::ExtensionSpanNotPositive`], [`SurfaceFailure::ExtensionSpansPastBound`], [`SurfaceFailure::AsItselfNetUnexpected`], [`SurfaceFailure::RevolutionAxisDegenerate`], [`SurfaceFailure::GeneratorCircleUndefined`], [`SurfaceFailure::AsItselfWeightNotPositive`], [`SurfaceFailure::RotationSenseUndefined`], [`SurfaceFailure::NoNonsingularStepoff`] | `SM_ERR` |
//! | [`SurfaceFailure::RecordDoesNotRebuild`] | `SM_ERR_INVALID_INPUT` |
//! | [`SurfaceFailure::SurfaceNotBSplineKind`] | `SM_ERR_NULL_POINTER` (class [`ErrorKind::Unsupported`]) |
//! | the other named causes, from [`SurfaceFailure::ScaleNotUniform`] on | the status of the phrase each replaced (`comparison_status`), or the site's through `with_status` |
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
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NOT_WITHIN_TOLERANCE,
    SM_ERR_NULL_POINTER, SM_ERR_OUTSIDE_OF_DOMAIN,
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
    /// The surface (a surface in volume, or an offset surface) does not own its base
    /// surface, so it cannot edit it (swap its UV, transform or trim it).
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
    /// A curve of a procedural blend's definition in the file is not a B-spline curve.
    /// [`ErrorKind::InvalidInput`].
    BlendCurveNotBSpline,
    /// A procedural blend's definition (its order, flags and numbers, or its chamfer's kind
    /// and sizes) could not be read from the file's ASCII record.
    /// [`ErrorKind::InvalidInput`].
    BlendDefinitionUnreadable,
    /// A blend face's net is not proved within its bound of its definition (or is not a net
    /// of its definition's stations), as a body rebuilt from a record must be before its
    /// faces are operands. Class `failed`; the re-certification's own error is its cause.
    NetNotCertified,
    /// The curve given has a dimension past what a record stores (`u32`).
    /// [`ErrorKind::InvalidInput`].
    CurveDimensionTooLarge,
    /// A face's procedural blend record does not rebuild (its rebuild's error is the
    /// cause). [`ErrorKind::InvalidInput`].
    RecordDoesNotRebuild,
    /// The surface given is of a kind with no B-spline form (an offset, a curve-bounded, a
    /// STEP or an in-volume surface), which a record stores: a kind the conversion has no
    /// code for. [`ErrorKind::Unsupported`].
    NoBSplineForm,
    /// The surface given is not one whose offset continues as itself (a cone, cylinder,
    /// sphere or torus). [`ErrorKind::InvalidInput`].
    DoesNotContinueAsItself,
    /// The offset surface given has a base that is an offset surface whose own base is an
    /// offset surface: offsets nest one deep at most. [`ErrorKind::InvalidInput`].
    OffsetNestedTooDeep,
    /// The surface has no normal at the point ([`ErrorKind::Degenerate`], `SM_ERR`).
    NoNormal,
    /// The caller's surface is not of a B-spline kind, which the routine reads as one
    /// ([`ErrorKind::Unsupported`], `SM_ERR_NULL_POINTER`).
    SurfaceNotBSplineKind,
    /// The point the base surface's inversion gives is farther from the offset surface than
    /// the tolerance. Class `failed`.
    InvertedPointOffOffset,
    /// The NLib G1R extension of the base toward the point failed (its error is the cause).
    /// Class `failed`.
    ExtensionTowardFailed,
    /// An arc of the surface of revolution (round its axis, outside `(0, 360]` degrees, or
    /// along a generator that is a circle, not positive) or its rotation's parameter length
    /// is out of range. Class `failed`.
    RevolutionArcOutOfRange,
    /// A span the extension as itself adds has a length that is not positive. Class
    /// `failed`.
    ExtensionSpanNotPositive,
    /// The extension as itself needs more spans at one end than its bound. Class `failed`.
    ExtensionSpansPastBound,
    /// The surface's NURB is not of the shape extending as itself reads: at least two
    /// control points each way with a weight per point, end knots of full multiplicity,
    /// degree 2 round the axis and 2 (a circle) or 1 (a line) along the generator, and a
    /// last span of positive length. Class `failed`.
    AsItselfNetUnexpected,
    /// The axis of the surface of revolution has no direction. Class `failed`.
    RevolutionAxisDegenerate,
    /// The generator's end control point, its inner one and the circle's centre are
    /// collinear or not finite, so they orient no circle. Class `failed`.
    GeneratorCircleUndefined,
    /// A weight the extension as itself reads (the reference control point's) or carries on
    /// (a line's next control point) is not positive. Class `failed`.
    AsItselfWeightNotPositive,
    /// The end row of the net and its inner row give no sense to turn about the axis. Class
    /// `failed`.
    RotationSenseUndefined,
    /// No step from the pole toward the domain's middle, in either direction, gives a first
    /// derivative longer than the tolerance. Class `failed`.
    NoNonsingularStepoff,
    /// The boundary curves of a plane on ellipses are not all ellipses, so they have no
    /// common ellipse plane to take. Class `failed`, status `SM_ERR`.
    NoCommonEllipsePlane,
    /// A curve of a plane on ellipses has no certified bounding box, so the plane's canvas
    /// cannot enclose it. Class `failed`, status `SM_ERR`.
    EllipseBoundsNotCertified,
    /// The ellipses of a plane's boundary do not all lie in the first one's stored plane,
    /// exactly: they are refused rather than fitted within a tolerance. Class `failed`,
    /// status `SM_ERR`.
    EllipsesNotOnOneExactPlane,
    /// The canvas that encloses an analytic plane's boundary curves is degenerate or not
    /// finite. Class `failed`, status `SM_ERR`.
    PlaneCanvasNotFinite,
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
    /// The scale given is not uniform (or, where the routine says so, not positive, or zero
    /// or not finite): an analytic surface takes only a uniform scale that keeps it a
    /// surface. [`ErrorKind::InvalidInput`].
    ScaleNotUniform,
    /// The surface's NURB form could not be made (the make's failure is the cause, where it
    /// gave one). Class `failed`; whether it is a defect or a refusal is its cause's
    /// ([`crate::SolidError::is_defect`] follows the chain).
    NurbNotMade,
    /// The radius given is not above the kernel's zero. [`ErrorKind::InvalidInput`].
    RadiusNotPositive,
    /// The swept surface given has no generator curve (an empty surface whose data was
    /// never read or set). [`ErrorKind::InvalidInput`].
    SurfaceWithoutGenerator,
    /// The NURB is not of the analytic kind the routine tests it for or rebuilds from it
    /// (an edited NURB that is no longer its surface's kind, or a NURB recognised as one):
    /// `check` says which test it failed. [`ErrorKind::Unsupported`].
    NurbNotAnalytic {
        /// The test of the analytic kind the NURB failed.
        check: AnalyticCheck,
    },
    /// The generator circle of the radius and placement given could not be made (the
    /// circle's failure, such as a radius not above the kernel's zero, is the cause).
    /// Status `SM_ERR_NULL_POINTER` (upstream's null circle); class `failed`, and whether it
    /// is a defect or a refusal is its cause's.
    GeneratorNotMade,
    /// The point is the centre of a sphere no larger than the tolerance: it lies on the
    /// sphere within the tolerance, but no direction from the centre gives it parameters.
    /// Class `failed`.
    CentreHasNoParameters,
    /// The cone given lies within the kernel's zero of its axis at the middle of its
    /// straight boundary line, so that line and the axis span no plane (a cone too thin
    /// there to intersect). [`ErrorKind::InvalidInput`].
    ConeBoundaryOnAxis,
    /// The database reader given has no input stream to read from.
    /// [`ErrorKind::InvalidInput`].
    NoInputStream,
    /// A ball patch's definition (its sphere's centre and radius) could not be read from
    /// the file's record. [`ErrorKind::InvalidInput`].
    BallDefinitionUnreadable,
    /// A point the construction takes a direction from the analytic's axis through lies on
    /// that axis (a pole, an apex, or every row of the side collapsed onto it), or its
    /// distance from the axis is not finite. Class `failed`.
    PointOnAxis,
    /// The three points of the source's iso across the side that orient its circle (its
    /// far end, its halfway point and its point on the side) are collinear or not finite,
    /// so they give no plane. Class `failed`.
    IsoCircleUndefined,
    /// An extension asked inside the solve's bracket could not be made (a weight that is not
    /// positive, a half turn on the analytic path, or its points, its world net or its bound
    /// not finite), though the bracket's upper end was made (whether an extension can be made
    /// is not monotone in the ask). Class `failed`.
    ExtensionNotMadeInBracket,
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

/// The test of an analytic kind a NURB failed ([`SurfaceFailure::NurbNotAnalytic`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnalyticCheck {
    /// Its degrees and rationality are not the kind's (a cone's are 1 by 2, rational).
    DegreeAndRationality,
    /// Its iso curves are not the kind's circles: those one way on parallel planes and
    /// those the other way with a common centre (a sphere's, its meridians at most a half
    /// turn) or on planes through a common line (a torus's).
    IsoCirclesNotOfKind,
    /// A meridian at an end of its sweep has no control point off the axis on the side of
    /// the fitted sweep, so the sweep has no start or end direction.
    MeridianOnAxis,
    /// Its generator circle, built from its first iso curve, is degenerate.
    GeneratorDegenerate,
}

impl core::fmt::Display for AnalyticCheck {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::DegreeAndRationality => "its degrees or rationality are not the kind's",
            Self::IsoCirclesNotOfKind => "its iso curves are not the kind's circles",
            Self::MeridianOnAxis => {
                "a meridian at an end of its sweep has no control point off the axis on its side"
            }
            Self::GeneratorDegenerate => "its generator circle is degenerate",
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
            | Self::SurfaceWithoutGenerator
            | Self::GeneratorNotMade
            | Self::NoInputStream
            | Self::SurfaceNotBSplineKind => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_)
            | Self::LineDirectionZero
            | Self::NormalsCancel
            | Self::RecordDoesNotRebuild => SM_ERR_INVALID_INPUT,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::OffSurface(_) => SM_ERR_NOT_WITHIN_TOLERANCE,
            _ => SM_ERR,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::InvalidInput`] for a check of the
    /// caller's argument, surface, curve or file, otherwise the kind of the status it
    /// replaced ([`ErrorKind::Failed`], [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`],
    /// [`ErrorKind::OutsideDomain`], [`ErrorKind::NotWithinTolerance`]).
    /// [`SurfaceFailure::Defect`] is [`ErrorKind::StageFailed`], a defect. Each named
    /// cause has the class its doc names.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } | Self::ApproximationNotConverged { .. } => ErrorKind::StageFailed,
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
            | Self::CurveWithoutControlPoints
            | Self::ParameterOutsideInterval { .. }
            | Self::TooFewSolverVariables { .. }
            | Self::InputNotGiven { .. }
            | Self::TrimmedCacheHasNoFace
            | Self::SurfaceCacheHasNoTree
            | Self::EndPointOutsideDomain
            | Self::InvalidDerivativeSurface
            | Self::ScaleNotUniform
            | Self::RadiusNotPositive
            | Self::SurfaceWithoutGenerator
            | Self::ConeBoundaryOnAxis
            | Self::NoInputStream
            | Self::BallDefinitionUnreadable
            | Self::BlendCurveNotBSpline
            | Self::BlendDefinitionUnreadable
            | Self::CurveDimensionTooLarge
            | Self::RecordDoesNotRebuild
            | Self::DoesNotContinueAsItself
            | Self::OffsetNestedTooDeep => ErrorKind::InvalidInput,
            Self::BoxOutsideBendHalfSpace { .. }
            | Self::BoxOutsideZeroPeriod { .. }
            | Self::PointOutsideVolumeDomain { .. }
            | Self::PointOutsideGivenDomain { .. }
            | Self::BoxOutsideParamDomain
            | Self::PseudoBoxOutsideDomain => ErrorKind::OutsideDomain,
            Self::IsoCurveKindNotLiftable
            | Self::DerivativeCombinationUnsupported { .. }
            | Self::PlainVolume
            | Self::BSplineVolumeUnsupported
            | Self::UnsupportedCacheKind
            | Self::UnsupportedTangencyType
            | Self::AtPole
            | Self::NurbNotAnalytic { .. }
            | Self::SurfaceNotBSplineKind
            | Self::NoBSplineForm => ErrorKind::Unsupported,
            Self::PointInBendSingularity
            | Self::ZeroVolumeTangent { .. }
            | Self::ZeroLengthTangent
            | Self::NoNormal
            | Self::ParallelNormals
            | Self::NormalsCancel
            | Self::TangentAlongNormal
            | Self::TooFewUniqueKnots
            | Self::ReferencePointOnBoundary => ErrorKind::Degenerate,
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
            | Self::CallFailed
            | Self::SurfaceCacheNotBuilt
            | Self::SurfaceCacheTreeNotBuilt
            | Self::CurveCacheNotBuilt
            | Self::CurveCacheTreeNotBuilt
            | Self::PointDoesNotDrop
            | Self::NoParameterDirection
            | Self::NoTangentTowardDirection
            | Self::NurbNotMade
            | Self::GeneratorNotMade
            | Self::CentreHasNoParameters => ErrorKind::Failed,
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
                f.write_str("the surface does not own its base surface")
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
            Self::BlendCurveNotBSpline => f.write_str("a curve of the blend's definition is not a B-spline"),
            Self::BlendDefinitionUnreadable => f.write_str("the procedural blend's definition could not be read"),
            Self::NetNotCertified => f.write_str("the blend face's net is not proved within its bound of its definition"),
            Self::CurveDimensionTooLarge => f.write_str("the curve's dimension is past what a record stores"),
            Self::RecordDoesNotRebuild => f.write_str("the face's procedural blend record does not rebuild"),
            Self::NoBSplineForm => f.write_str("the surface is of a kind with no B-spline form"),
            Self::DoesNotContinueAsItself => f.write_str("only a cone, cylinder, sphere or torus continues as itself"),
            Self::OffsetNestedTooDeep => f.write_str("the base of the offset surface's base is an offset surface"),
            Self::NoNormal => f.write_str("the surface has no normal at the point"),
            Self::SurfaceNotBSplineKind => f.write_str("the surface is not of a B-spline kind"),
            Self::InvertedPointOffOffset => f.write_str("the inverted point is farther from the offset surface than the tolerance"),
            Self::ExtensionTowardFailed => f.write_str("the G1R extension toward the point failed"),
            Self::RevolutionArcOutOfRange => f.write_str("an arc of the surface of revolution or its parameter length is out of range"),
            Self::ExtensionSpanNotPositive => f.write_str("the span length of the extension is not positive"),
            Self::ExtensionSpansPastBound => f.write_str("the extension needs more spans than its bound"),
            Self::AsItselfNetUnexpected => f.write_str("the surface's NURB is not of the shape and degree a surface extending as itself has"),
            Self::RevolutionAxisDegenerate => f.write_str("the axis of the surface of revolution has no direction"),
            Self::GeneratorCircleUndefined => f.write_str("the generator's end orients no circle"),
            Self::AsItselfWeightNotPositive => f.write_str("a weight of the extension as itself is not positive"),
            Self::RotationSenseUndefined => f.write_str("the end rows give no sense to turn about the axis"),
            Self::NoNonsingularStepoff => f.write_str("neither direction gives a nonsingular stepoff"),
            Self::NoCommonEllipsePlane => {
                f.write_str("the boundary curves are not all ellipses with a common plane")
            }
            Self::EllipseBoundsNotCertified => {
                f.write_str("a boundary curve has no certified bounding box")
            }
            Self::EllipsesNotOnOneExactPlane => {
                f.write_str("the ellipse boundaries do not have one exact plane")
            }
            Self::PlaneCanvasNotFinite => {
                f.write_str("the analytic plane's bounding canvas is not finite")
            }
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
                f.write_str("the surface has fewer than two unique knots in the other direction")
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
            Self::ScaleNotUniform => f.write_str("an analytic surface takes only a uniform scale"),
            Self::NurbNotMade => f.write_str("the surface's NURB form could not be made"),
            Self::RadiusNotPositive => f.write_str("the radius is not positive"),
            Self::SurfaceWithoutGenerator => f.write_str("the surface has no generator curve"),
            Self::NurbNotAnalytic { check } => {
                write!(f, "the NURB is not of the analytic kind: {check}")
            }
            Self::GeneratorNotMade => f.write_str("the generator circle could not be made"),
            Self::CentreHasNoParameters => f.write_str(
                "the point is the centre of a sphere within the tolerance and has no parameters",
            ),
            Self::ConeBoundaryOnAxis => {
                f.write_str("the cone is on its axis at the middle of its boundary line")
            }
            Self::NoInputStream => f.write_str("the database has no input stream"),
            Self::BallDefinitionUnreadable => {
                f.write_str("the ball patch's definition could not be read")
            }
            Self::PointOnAxis => f.write_str("a point the construction turns about is on the axis"),
            Self::IsoCircleUndefined => {
                f.write_str("the source's iso across the side orients no circle")
            }
            Self::ExtensionNotMadeInBracket => f.write_str(
                "an extension asked inside the solve's bracket could not be made, though its upper end was made",
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a surface routine that fails with `failure`.
    #[must_use]
    pub(crate) fn surface(operation: &'static str, failure: SurfaceFailure) -> Self {
        Failure::new(operation, failure).into_error()
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

    /// [`SurfaceFailure::NurbNotMade`] with status `status`, for a routine whose NURB form
    /// is still missing after `made`, the result of the call that was to make it: its
    /// error, when it failed, is the cause.
    #[must_use]
    pub(crate) fn surface_nurb_not_made(
        operation: &'static str,
        status: i32,
        made: Result<(), Self>,
    ) -> Self {
        let failure = Failure::new(operation, SurfaceFailure::NurbNotMade).with_status(status);
        match made {
            Err(e) => failure.caused_by(e),
            Ok(()) => failure,
        }
        .into_error()
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
    use crate::error::EntityRef;
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
                SurfaceFailure::InvalidValue("the face's domain and the surface's are disjoint"),
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
        let typed = Failure::new(
            "surface::procedural_blend::recertify_faces",
            SurfaceFailure::NetNotCertified,
        )
        .at(face)
        .into_error();
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

    /// The named causes of the offset and procedural blend surfaces keep the status of the
    /// phrase causes they replaced, with the kind their condition is: the caller's input, or
    /// a failure of the geometry.
    #[test]
    fn the_named_offset_and_blend_causes_keep_the_status_they_replaced() {
        use crate::error::ErrorKind as K;
        let cases = [
            (
                SurfaceFailure::BlendCurveNotBSpline,
                SM_ERR,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::BlendDefinitionUnreadable,
                SM_ERR,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::CurveDimensionTooLarge,
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::NoBSplineForm, SM_ERR, K::Unsupported),
            (
                SurfaceFailure::DoesNotContinueAsItself,
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::OffsetNestedTooDeep, SM_ERR, K::InvalidInput),
            (
                SurfaceFailure::DerivativeOrderTooHigh { max: 3 },
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::BaseSurfaceNotOwned, SM_ERR, K::InvalidInput),
            (
                SurfaceFailure::RecordDoesNotRebuild,
                SM_ERR_INVALID_INPUT,
                K::InvalidInput,
            ),
            (SurfaceFailure::NetNotCertified, SM_ERR, K::Failed),
            (SurfaceFailure::NoNormal, SM_ERR, K::Degenerate),
            (
                SurfaceFailure::SurfaceNotBSplineKind,
                SM_ERR_NULL_POINTER,
                K::Unsupported,
            ),
            (SurfaceFailure::InvertedPointOffOffset, SM_ERR, K::Failed),
            (SurfaceFailure::ExtensionTowardFailed, SM_ERR, K::Failed),
            (SurfaceFailure::RevolutionArcOutOfRange, SM_ERR, K::Failed),
            (SurfaceFailure::ExtensionSpanNotPositive, SM_ERR, K::Failed),
            (SurfaceFailure::ExtensionSpansPastBound, SM_ERR, K::Failed),
            (SurfaceFailure::AsItselfNetUnexpected, SM_ERR, K::Failed),
            (SurfaceFailure::RevolutionAxisDegenerate, SM_ERR, K::Failed),
            (SurfaceFailure::GeneratorCircleUndefined, SM_ERR, K::Failed),
            (SurfaceFailure::AsItselfWeightNotPositive, SM_ERR, K::Failed),
            (SurfaceFailure::RotationSenseUndefined, SM_ERR, K::Failed),
            (SurfaceFailure::NoNonsingularStepoff, SM_ERR, K::Failed),
        ];
        for (failure, code, kind) in cases {
            let e = SolidError::surface("surface::op", failure);
            assert_eq!(e.sm_status(), Some(code), "{failure:?}");
            assert_eq!(e.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(code), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.surface_failure(), Some(&failure));
            assert!(!e.is_defect(), "{failure:?}");
        }
    }

    /// The named causes of a plane on ellipses have status `SM_ERR` and class `failed`.
    #[test]
    fn the_plane_on_ellipses_causes_have_their_status_and_class() {
        let op = "surface::op";
        for failure in [
            SurfaceFailure::NoCommonEllipsePlane,
            SurfaceFailure::EllipseBoundsNotCertified,
            SurfaceFailure::EllipsesNotOnOneExactPlane,
            SurfaceFailure::PlaneCanvasNotFinite,
        ] {
            let e = SolidError::surface(op, failure);
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{failure:?}");
            assert_eq!(e.kind(), ErrorKind::Failed, "{failure:?}");
            assert_eq!(e.surface_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
    }

    /// The named causes of the surface extensions and the ball patch keep the status of the
    /// phrase causes they replaced (a site whose status differs from the cause's own keeps
    /// it through `with_status`), with the kind their condition is: the caller's input, or a
    /// failure of the geometry.
    #[test]
    fn the_named_extension_causes_keep_the_status_they_replaced() {
        use crate::error::ErrorKind as K;
        let cases = [
            (
                SurfaceFailure::ScaleNotUniform,
                None,
                SM_ERR,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::NoNurb,
                Some(SM_ERR),
                SM_ERR,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::CurveWithoutNurb,
                None,
                SM_ERR,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::NoInputStream,
                None,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::BallDefinitionUnreadable,
                None,
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::PointOnAxis, None, SM_ERR, K::Failed),
            (SurfaceFailure::IsoCircleUndefined, None, SM_ERR, K::Failed),
            (
                SurfaceFailure::ExtensionNotMadeInBracket,
                None,
                SM_ERR,
                K::Failed,
            ),
        ];
        for (failure, status, code, kind) in cases {
            let mut f = Failure::new("surface::op", failure);
            if let Some(s) = status {
                f = f.with_status(s);
            }
            let e = f.into_error();
            assert_eq!(e.sm_status(), Some(code), "{failure:?}");
            assert_eq!(e.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(code), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.surface_failure(), Some(&failure));
            assert!(!e.is_defect(), "{failure:?}");
        }
        assert_eq!(
            SurfaceFailure::NoNurb.comparison_status(),
            SM_ERR_NULL_POINTER
        );
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

    /// Each named cause keeps the status of the phrase it replaced and says its class; it
    /// is neither a defect nor a refusal by itself.
    #[test]
    fn the_named_causes_keep_their_status_and_say_their_class() {
        let op = "surface::op";
        let analytic = |check| SurfaceFailure::NurbNotAnalytic { check };
        let cases = [
            (
                SurfaceFailure::ScaleNotUniform,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::NoNurb,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (SurfaceFailure::NurbNotMade, SM_ERR, ErrorKind::Failed),
            (
                SurfaceFailure::RadiusNotPositive,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::SurfaceWithoutGenerator,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::DerivativeOrderTooHigh { max: 3 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                analytic(AnalyticCheck::DegreeAndRationality),
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                analytic(AnalyticCheck::IsoCirclesNotOfKind),
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                analytic(AnalyticCheck::MeridianOnAxis),
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                analytic(AnalyticCheck::GeneratorDegenerate),
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                SurfaceFailure::GeneratorNotMade,
                SM_ERR_NULL_POINTER,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::CentreHasNoParameters,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::ConeBoundaryOnAxis,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::surface(op, failure);
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.operation(), op);
            assert_eq!(e.surface_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        // a NURB that could not be made keeps the site's status and chains the make's error
        let made = SolidError::surface(op, SurfaceFailure::RadiusNotPositive);
        let e = SolidError::surface_nurb_not_made(op, SM_ERR_NULL_POINTER, Err(made.clone()));
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.surface_failure(), Some(&SurfaceFailure::NurbNotMade));
        assert_eq!(e.cause(), Some(&made));
        let e = SolidError::surface_nurb_not_made(op, SM_ERR, Ok(()));
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert!(e.cause().is_none());
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
            ("solidcore/src/surface/cone/p1.rs", 12),
            ("solidcore/src/surface/cone/p2.rs", 14),
            ("solidcore/src/surface/cone/p3.rs", 3),
            ("solidcore/src/surface/plane/p1.rs", 3),
            ("solidcore/src/surface/plane/p2.rs", 9),
            ("solidcore/src/surface/surf_of_extrusion/p1.rs", 16),
            ("solidcore/src/surface/surf_of_extrusion/p2.rs", 13),
            ("solidcore/src/surface/surf_of_revolution/p1.rs", 10),
            ("solidcore/src/surface/surf_of_revolution/p2.rs", 6),
            ("solidcore/src/surface/surf_of_revolution/p3.rs", 21),
            ("solidcore/src/surface/surface/p2.rs", 21),
            ("solidcore/src/surface/surface_cache/face_context.rs", 1),
            ("solidcore/src/surface/surface_cache/p1.rs", 15),
            ("solidcore/src/surface/surface_cache/p2.rs", 5),
            ("solidcore/src/surface/surface_cache/p3.rs", 1),
            ("solidcore/src/surface/surface_point_solve/p1.rs", 5),
            ("solidcore/src/surface/surface_point_solve/p2.rs", 8),
            ("solidcore/src/surface/trim_srf_cache.rs", 4),
        ];
        assert_pinned("surface phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
