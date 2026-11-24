//! Why a NURBS routine fails, in solidcore's terms.
//!
//! Upstream: `source/SMLib/inc/NL_DataStruct.h` (the `NL_*_ERR` numbers), none otherwise.
//! These describe the failures of the NURBS routines (`crate::nurbs`: knots, bases,
//! curve and surface fitting, knot insertion, degree elevation, ...). NLib routines
//! report failure by returning an error flag and storing a number in the process-wide
//! `NL_ERROR` record; solidcore returns a [`NurbsFailure`] instead. Each also records,
//! for difftest only, the `NL_*_ERR` number the C++ stores for it (crate-private
//! `nlib_number`, read through [`nlib_number`]) and the status its `Sm*` callers
//! report for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]: `SM_ERR`, never the `NL_*_ERR` number).
//!
//! | Rust                                  | Compared number                 |
//! |---------------------------------------|---------------------------------|
//! | [`NurbsFailure::BSpline`]             | none: a [`BSplineFailure`]      |
//! | [`NurbsFailure::CallFailed`]          | `NL_CAL_ERR` (13)               |
//! | [`NurbsFailure::CurveDefinition`]     | `NL_CUR_ERR` (1)                |
//! | [`NurbsFailure::Degree`], [`NurbsFailure::DegreeTooHigh`] | `NL_DEG_ERR` (3) |
//! | [`NurbsFailure::DerivativeOrder`], [`NurbsFailure::NoDerivativeRows`] | `NL_MXD_ERR` (20) |
//! | [`NurbsFailure::Geometry`]            | `NL_GEO_ERR` (17)               |
//! | [`NurbsFailure::Index`], [`NurbsFailure::IndexOutOfRange`] | `NL_IND_ERR` (7) |
//! | [`NurbsFailure::Input`]               | `NL_INP_ERR` (14)               |
//! | [`NurbsFailure::Knots`], [`NurbsFailure::KnotsDecrease`]  | `NL_KNT_ERR` (4) |
//! | [`NurbsFailure::Memory`]              | `NL_MEM_ERR` (15)               |
//! | [`NurbsFailure::NotConverged`]        | `NL_CON_ERR` (12)               |
//! | [`NurbsFailure::Numerical`]           | `NL_NUM_ERR` (11)               |
//! | [`NurbsFailure::Parameter`], [`NurbsFailure::ParameterOutside`] | `NL_PAR_ERR` (6) |
//! | [`NurbsFailure::Singular`]            | `NL_SEQ_ERR` (16)               |
//! | [`NurbsFailure::Storage`]             | `NL_STO_ERR` (9)                |
//! | [`NurbsFailure::SurfaceDefinition`]   | `NL_SUR_ERR` (2)                |
//! | [`NurbsFailure::Weight`], [`NurbsFailure::WeightOutOfRange`] | `NL_WEI_ERR` (5) |
//! | [`NurbsFailure::ZeroDerivative`]      | `NL_DER_ERR` (8)                |
//!
//! [`BSplineFailure`] describes the failures of the B-spline curve and surface methods
//! built on those routines (`BSplineCurve`, `BSplineSurface`: fitting, skinning, Coons
//! patches, trimming, evaluation); upstream they return an `SmStatus` (`SER(SM_ERR)`,
//! `NER(...)`), which each records as its comparison status. A method whose NLib call
//! fails keeps that call's failure as its cause ([`BSplineFailure::RoutineFailed`]). A
//! check that refuses builds a typed cause with its numbers ([`BSplineFailure::EndsApart`],
//! ...); a stage that fails as the kernel's fault is a [`BSplineFailure::Defect`] with its
//! reason, a defect ([`ErrorKind::StageFailed`], #15034).
//!
//! Deviations: not upstream. A failure of a routine that has the data (a degree, an
//! index, a parameter and its domain) carries it; the others are unit variants, and the
//! failure's `operation` path says which routine. `NL_MAT_ERR`, `NL_TOL_ERR`,
//! `NL_KML_ERR`, `NL_CUR_REV`, `NL_FIL_ERR` and `NL_SCN_ERR` have no failure here: no
//! routine of the port stores them.

use super::{ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER};
use crate::geom::geom_utility::p2::ContinuityType;
use crate::nurbs::b_spline_surface::SurfParamType;

/// Why a NURBS routine fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum NurbsFailure {
    /// A B-spline curve or surface method failed (not an NLib routine itself).
    BSpline(BSplineFailure),
    /// A routine it calls rejected its arguments, or the direction is not one the
    /// routine handles.
    CallFailed,
    /// The curve's control point, knot and degree counts do not agree (`n + p + 1 != m`).
    CurveDefinition,
    /// The degree is not allowed here (too low, too high, or not one the routine
    /// handles).
    Degree,
    /// The degree `given` is above the largest `max` the routines handle.
    DegreeTooHigh {
        /// The degree asked for.
        given: usize,
        /// The largest degree.
        max: usize,
    },
    /// The derivative order `given` is above the largest `max` the routines evaluate.
    DerivativeOrder {
        /// The order asked for.
        given: usize,
        /// The largest order.
        max: usize,
    },
    /// The geometry does not allow the operation (a degenerate or non-planar input).
    Geometry,
    /// An index is out of range.
    Index,
    /// The index `index` is not below the length `len`.
    IndexOutOfRange {
        /// The index.
        index: usize,
        /// The number of items.
        len: usize,
    },
    /// The input data is not valid.
    Input,
    /// The knot vector is not valid here.
    Knots,
    /// The knot vector decreases at knot `index` (it is smaller than the knot before it).
    KnotsDecrease {
        /// The index of the knot that is smaller than its predecessor.
        index: usize,
    },
    /// The memory for the result cannot be allocated.
    Memory,
    /// The output has no row for even the point itself, so no derivative order can be
    /// evaluated into it.
    NoDerivativeRows,
    /// An iteration did not converge.
    NotConverged,
    /// A numerical failure (a division by zero, an overflow, a value that is not finite).
    Numerical,
    /// A parameter is outside the domain.
    Parameter,
    /// The parameter is outside the curve or surface's domain `[lo, hi]`.
    ParameterOutside {
        /// The parameter.
        parameter: f64,
        /// The start of the domain.
        lo: f64,
        /// The end of the domain.
        hi: f64,
    },
    /// The system of equations has no unique solution (it is singular).
    Singular,
    /// A buffer supplied by the caller is too small for the result.
    Storage,
    /// The surface's control net, knot and degree counts do not agree in a direction.
    SurfaceDefinition,
    /// A weight is not valid here.
    Weight,
    /// The weight of control point `index` is `weight`, outside the range `[min, max]`
    /// the routines accept.
    WeightOutOfRange {
        /// The control point.
        index: usize,
        /// Its weight.
        weight: f64,
        /// The smallest weight accepted.
        min: f64,
        /// The largest weight accepted.
        max: f64,
    },
    /// A derivative is zero where a direction is needed.
    ZeroDerivative,
}

/// Why a B-spline curve or surface method fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum BSplineFailure {
    /// An arc's centre is `distance` from the first arc's, more than `tolerance`.
    /// [`ErrorKind::InvalidInput`].
    ArcCentresApart {
        /// The distance between the centres.
        distance: f64,
        /// The tolerance they must agree within.
        tolerance: f64,
    },
    /// An arc is of a curve kind with no B-spline or ellipse data to revolve.
    /// [`ErrorKind::Unsupported`].
    ArcKindUnsupported,
    /// The four arcs' common `radius`, or the length `facing` of the sum of their ends'
    /// directions from the centre (which says the side of the sphere the loop faces), is not
    /// above `tolerance`, so no sphere follows from them. [`ErrorKind::Degenerate`].
    ArcLoopDegenerate {
        /// The arcs' radius.
        radius: f64,
        /// The length of the sum of the end directions.
        facing: f64,
        /// The tolerance both must exceed.
        tolerance: f64,
    },
    /// An arc's radius `other` differs from the first arc's `radius` by more than
    /// `tolerance`. [`ErrorKind::InvalidInput`].
    ArcRadiiDiffer {
        /// The first arc's radius.
        radius: f64,
        /// This arc's radius.
        other: f64,
        /// The tolerance they must agree within.
        tolerance: f64,
    },
    /// The arc's sweep of `angle` radians is (nearly) zero or half a turn, where the centre
    /// and the two points are on one line. [`ErrorKind::Degenerate`].
    ArcSweepDegenerate {
        /// The sweep, in radians.
        angle: f64,
    },
    /// The arcs do not meet end to end: with three, the largest arc and its larger
    /// neighbour share no end; with four, an end does not meet exactly one other arc's end.
    /// [`ErrorKind::InvalidInput`].
    ArcsDoNotMeet,
    /// A boundary curve of the corner blend is not given. [`ErrorKind::InvalidInput`];
    /// status `SM_ERR_NULL_POINTER`.
    BoundaryCurveMissing,
    /// A boundary curve comes without its parameter curve or neighbouring surface where the
    /// corner blend needs them (all of them for more than four sides, all but one for four).
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR_NULL_POINTER`.
    BoundaryWithoutSurface,
    /// The continuity asked of a curve extension is not one it keeps (G1, G1G2 or
    /// C-infinity). [`ErrorKind::InvalidInput`].
    ContinuityNotExtendable {
        /// The continuity asked for.
        continuity: ContinuityType,
    },
    /// The curves must have the same number of control points: this one has `ours`, the
    /// other `theirs`. [`ErrorKind::InvalidInput`].
    ControlPointCountsDiffer {
        /// This curve's control points.
        ours: usize,
        /// The other curve's control points.
        theirs: usize,
    },
    /// The control point `index` is past the last of the `count` control points.
    /// [`ErrorKind::InvalidInput`].
    ControlPointIndex {
        /// The index given.
        index: usize,
        /// The number of control points.
        count: usize,
    },
    /// The span `span` of a degree-`degree` curve reads control points `span - degree` to
    /// `span`, and the curve has only `count`: its control points, knots and degree do not
    /// agree. [`ErrorKind::InvalidInput`].
    ControlPointsShort {
        /// The span the parameter falls in.
        span: usize,
        /// The curve's degree.
        degree: usize,
        /// The curve's number of control points.
        count: usize,
    },
    /// A curve to bound the plane stands farther than `tolerance` from the plane of the
    /// ruled surface between the two curves (its tangent plane at the middle).
    /// [`ErrorKind::InvalidInput`].
    CurvesNotCoplanar {
        /// The distance a curve may stand off the plane.
        tolerance: f64,
    },
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The degree `given` is below the curve's degree `current`; an elevation only raises
    /// it. [`ErrorKind::InvalidInput`].
    DegreeBelowCurrent {
        /// The degree asked for.
        given: usize,
        /// The curve's degree.
        current: usize,
    },
    /// The degree cannot be reduced by one within the `tolerance` asked for: the reduction's
    /// error bound reached `error` (for a rational curve the bound is that of the homogeneous
    /// points, measured against the tolerance scaled by the weights), or a weight is outside
    /// the range the reduction takes. [`ErrorKind::NotWithinTolerance`].
    DegreeNotReducible {
        /// The error bound of the reduction.
        error: f64,
        /// The tolerance asked for.
        tolerance: f64,
    },
    /// The degree `given` is outside the range `[min, max]` the method handles.
    DegreeOutside {
        /// The degree asked for.
        given: i64,
        /// The smallest degree.
        min: i64,
        /// The largest degree.
        max: i64,
    },
    /// The degree `given` does not fit this platform's index type. [`ErrorKind::InvalidInput`].
    DegreeTooLarge {
        /// The degree given.
        given: u64,
    },
    /// The curve is of degree `degree`, 1 or less, and has no lower degree to reduce to.
    /// [`ErrorKind::InvalidInput`].
    DegreeTooLowToReduce {
        /// The curve's degree.
        degree: usize,
    },
    /// A Coons patch that is not bilinear needs four derivative curves, and none are given.
    /// [`ErrorKind::InvalidInput`].
    DerivativeCurvesMissing,
    /// The points are said to be of dimension `dim`; the method takes 2 or 3.
    /// [`ErrorKind::InvalidInput`].
    DimensionNot2Or3 {
        /// The dimension given.
        dim: u64,
    },
    /// The direction is `direction`; the method takes u or v. [`ErrorKind::InvalidInput`].
    DirectionNotUOrV {
        /// The direction given.
        direction: SurfParamType,
    },
    /// The grid of points has `rows` rows and `columns` columns, and the fit needs at least
    /// one of each. [`ErrorKind::InvalidInput`].
    EmptyGrid {
        /// The rows given.
        rows: usize,
        /// The columns given.
        columns: usize,
    },
    /// The end tangent is asked to be the one given, and none is given.
    /// [`ErrorKind::InvalidInput`].
    EndTangentNotGiven,
    /// The ends to join are `distance` apart, farther than the tolerance.
    EndsApart {
        /// The distance between the ends.
        distance: f64,
        /// The tolerance they must be within.
        tolerance: f64,
    },
    /// The curve's ends coincide (it is closed), so no scaling takes one end to the other.
    EndsCoincide,
    /// Expanding the surface's boundary span changed its control point or knot counts,
    /// which the expansion is to keep; the surface is left unchanged. Class `failed`.
    ExpansionChangesCounts,
    /// The blend's derivatives are faired for two points with a tangent each, and `points`
    /// points and `tangents` tangents are given. [`ErrorKind::InvalidInput`].
    FairBlendInputs {
        /// The points given.
        points: usize,
        /// The tangents given.
        tangents: usize,
    },
    /// A boundary curve without a neighbouring surface is taken only when it is degenerate
    /// (a point), and this one is not. [`ErrorKind::InvalidInput`].
    FreeBoundaryNotDegenerate,
    /// Every control point of the curve to revolve is on the axis, so the revolution sweeps
    /// no area. [`ErrorKind::Degenerate`].
    GeneratorOnAxis,
    /// The approximation's interval [`lo`, `hi`] has no floating-point midpoint strictly
    /// inside it (or one that is not finite), and its piece still does not fit.
    /// [`ErrorKind::NotWithinTolerance`].
    IntervalNotSplittable {
        /// The interval's start.
        lo: f64,
        /// The interval's end.
        hi: f64,
    },
    /// The spans the interval meets hold no control point of the curve (for example a point
    /// interval at an interior knot of multiplicity degree + 1). [`ErrorKind::InvalidInput`].
    IntervalSelectsNoControlPoints,
    /// An end weight of the curves to join is zero, so no scaling makes the weights at the
    /// joint agree. [`ErrorKind::InvalidInput`].
    JoinEndWeightZero,
    /// Joining the curves into one failed after the join's own check of the gap passed; the
    /// failure's cause says why (for example ends farther apart than the construction takes).
    /// Class `failed`; whether it is a defect or a refusal is its cause's.
    JoinNotBuilt,
    /// The control point (`u_index`, `v_index`) is past the last one, (`u_last`, `v_last`).
    /// [`ErrorKind::InvalidInput`].
    NetIndexOutside {
        /// The u index given.
        u_index: usize,
        /// The v index given.
        v_index: usize,
        /// The last u index.
        u_last: usize,
        /// The last v index.
        v_last: usize,
    },
    /// The fit is asked for no control points (a count of 0). [`ErrorKind::InvalidInput`].
    NoControlPoints,
    /// The point's search found no point of the surface within `tolerance` of it.
    /// [`ErrorKind::NotWithinTolerance`].
    NoFootWithinTolerance {
        /// The distance searched within.
        tolerance: f64,
    },
    /// No NURB is given to take the curve from. [`ErrorKind::InvalidInput`].
    NoNurbGiven,
    /// The curve or surface holds no NURBS form to work on.
    NoNurbsForm,
    /// The evaluation's output has no slot, not even one for the point.
    /// [`ErrorKind::InvalidInput`].
    NoOutputSlot,
    /// A curve given as an arc is not one (within the tolerance). [`ErrorKind::InvalidInput`].
    NotAnArc,
    /// The method works only on a surface-intersection approximation (a piecewise C0 curve
    /// of degree 3), and the curve is not one. [`ErrorKind::Unsupported`].
    NotIntersectionCurve,
    /// The method takes only a rational curve, and this one is not.
    /// [`ErrorKind::Unsupported`].
    NotRational,
    /// The parameter is outside the domain `[lo, hi]`.
    ParameterOutside {
        /// The parameter.
        parameter: f64,
        /// The start of the domain.
        lo: f64,
        /// The end of the domain.
        hi: f64,
    },
    /// The corner patch has `u_count` x `v_count` control points of degrees `u_degree`,
    /// `v_degree`, and the projection of its side rows needs at least five control points
    /// and degree 1 in each direction (a patch whose domain collapses in a direction is not
    /// refined there). [`ErrorKind::Degenerate`].
    PatchTooCoarse {
        /// Control points in u.
        u_count: usize,
        /// Control points in v.
        v_count: usize,
        /// Degree in u.
        u_degree: usize,
        /// Degree in v.
        v_degree: usize,
    },
    /// The corner patch has fewer than two distinct knots in a direction: its domain there is
    /// a single value, so it has no span to refine or halve. [`ErrorKind::Degenerate`].
    PatchWithoutSpan,
    /// The projection onto its surface of a point the method puts on the curve leaves the
    /// residual `residual` (the surface point solve's value), more than `tolerance`.
    /// [`ErrorKind::NotWithinTolerance`].
    PointOffSurface {
        /// The residual of the point's projection onto the surface.
        residual: f64,
        /// The residual allowed.
        tolerance: f64,
    },
    /// Rails are given (or the first is the spine) without the sections' intersections with
    /// them. [`ErrorKind::InvalidInput`]; status `SM_ERR_NULL_POINTER`, or `SM_ERR` where
    /// upstream reports that for rails without a spine.
    RailIntersectionsMissing,
    /// A NURBS routine the method calls failed; the failure's cause says why.
    RoutineFailed,
    /// A sphere is made from three arcs (or four that close a loop), and `given` are given.
    /// [`ErrorKind::InvalidInput`].
    SphereArcCount {
        /// The number of curves given.
        given: usize,
    },
    /// The first rail is to be the spine, and no first rail is given.
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR_NULL_POINTER`.
    SpineRailMissing,
    /// The start tangent is asked to be the one given, and none is given.
    /// [`ErrorKind::InvalidInput`].
    StartTangentNotGiven,
    /// The approximation of a tangent field used up its `limit` subdivisions before every
    /// piece fit; the field may fit with more, or not at all (a jump in it).
    /// [`ErrorKind::NotWithinTolerance`].
    SubdivisionCap {
        /// The subdivisions allowed.
        limit: usize,
    },
    /// A first derivative vanishes at the point, and no point nearby has both long enough to
    /// stand in for them. [`ErrorKind::Degenerate`].
    TangentsVanish,
    /// A corner blend is bounded by at least four curves, and `given` are given.
    /// [`ErrorKind::InvalidInput`].
    TooFewBoundaryCurves {
        /// The curves given.
        given: usize,
    },
    /// The curve to approximate has `given` breakpoints, and its interval needs two.
    /// [`ErrorKind::InvalidInput`].
    TooFewBreakpoints {
        /// The breakpoints given.
        given: usize,
    },
    /// `given` end vectors are given; the fit takes at most two (a start and an end
    /// derivative), or, when interpolating more than two points, one per point.
    /// [`ErrorKind::InvalidInput`].
    TooManyVectors {
        /// The number of vectors given.
        given: usize,
    },
    /// The trim domain is not inside the surface's domain. [`ErrorKind::InvalidInput`].
    TrimDomainOutside,
    /// A weight is given for the end of a curve that is not rational.
    /// [`ErrorKind::InvalidInput`].
    WeightOnNonRational,
    /// The axis direction is a zero vector.
    ZeroAxis,
}

impl BSplineFailure {
    /// The status the C++ method returns for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::BoundaryCurveMissing
            | Self::BoundaryWithoutSurface
            | Self::RailIntersectionsMissing
            | Self::SpineRailMissing => SM_ERR_NULL_POINTER,
            Self::ZeroAxis => SM_ERR_INVALID_INPUT,
            _ => SM_ERR,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::StageFailed`] for
    /// [`BSplineFailure::Defect`], a defect;
    /// [`ErrorKind::InvalidInput`] for a check of the caller's arguments or curve,
    /// [`ErrorKind::Unsupported`] for a curve the method does not take,
    /// [`ErrorKind::NotWithinTolerance`] for a result outside the tolerance asked for; the
    /// kind of its status otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::ArcCentresApart { .. }
            | Self::ArcRadiiDiffer { .. }
            | Self::ArcsDoNotMeet
            | Self::BoundaryCurveMissing
            | Self::BoundaryWithoutSurface
            | Self::ContinuityNotExtendable { .. }
            | Self::ControlPointCountsDiffer { .. }
            | Self::ControlPointIndex { .. }
            | Self::ControlPointsShort { .. }
            | Self::CurvesNotCoplanar { .. }
            | Self::DegreeBelowCurrent { .. }
            | Self::DegreeTooLarge { .. }
            | Self::DegreeTooLowToReduce { .. }
            | Self::DerivativeCurvesMissing
            | Self::DimensionNot2Or3 { .. }
            | Self::DirectionNotUOrV { .. }
            | Self::EmptyGrid { .. }
            | Self::EndTangentNotGiven
            | Self::FairBlendInputs { .. }
            | Self::FreeBoundaryNotDegenerate
            | Self::IntervalSelectsNoControlPoints
            | Self::JoinEndWeightZero
            | Self::NetIndexOutside { .. }
            | Self::NoControlPoints
            | Self::NoNurbGiven
            | Self::NoOutputSlot
            | Self::NotAnArc
            | Self::RailIntersectionsMissing
            | Self::SphereArcCount { .. }
            | Self::SpineRailMissing
            | Self::StartTangentNotGiven
            | Self::TooFewBoundaryCurves { .. }
            | Self::TooFewBreakpoints { .. }
            | Self::TooManyVectors { .. }
            | Self::TrimDomainOutside
            | Self::WeightOnNonRational => ErrorKind::InvalidInput,
            Self::ArcLoopDegenerate { .. }
            | Self::ArcSweepDegenerate { .. }
            | Self::GeneratorOnAxis
            | Self::PatchTooCoarse { .. }
            | Self::PatchWithoutSpan
            | Self::TangentsVanish => ErrorKind::Degenerate,
            Self::DegreeNotReducible { .. }
            | Self::IntervalNotSplittable { .. }
            | Self::NoFootWithinTolerance { .. }
            | Self::PointOffSurface { .. }
            | Self::SubdivisionCap { .. } => ErrorKind::NotWithinTolerance,
            Self::NotRational | Self::NotIntersectionCurve | Self::ArcKindUnsupported => {
                ErrorKind::Unsupported
            }
            Self::Defect { .. } => ErrorKind::StageFailed,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for BSplineFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ArcCentresApart {
                distance,
                tolerance,
            } => write!(
                f,
                "the arcs' centres are {distance} apart, farther than {tolerance}"
            ),
            Self::ArcKindUnsupported => f.write_str("an arc is neither a B-spline nor an ellipse"),
            Self::ArcLoopDegenerate {
                radius,
                facing,
                tolerance,
            } => write!(
                f,
                "the arcs' radius {radius} or their facing {facing} is not above {tolerance}"
            ),
            Self::ArcRadiiDiffer {
                radius,
                other,
                tolerance,
            } => write!(
                f,
                "the arcs have radii {radius} and {other}, farther apart than {tolerance}"
            ),
            Self::ArcSweepDegenerate { angle } => write!(
                f,
                "the arc's sweep of {angle} radians is nearly zero or half a turn"
            ),
            Self::ArcsDoNotMeet => f.write_str("the arcs do not meet end to end"),
            Self::BoundaryCurveMissing => f.write_str("a boundary curve is missing"),
            Self::BoundaryWithoutSurface => {
                f.write_str("a boundary's parameter curve or neighbouring surface is missing")
            }
            Self::ContinuityNotExtendable { continuity } => write!(
                f,
                "an extension keeps G1, G1G2 or C-infinity continuity, not {continuity:?}"
            ),
            Self::ControlPointCountsDiffer { ours, theirs } => write!(
                f,
                "the curves have {ours} and {theirs} control points, not the same number"
            ),
            Self::ControlPointIndex { index, count } => write!(
                f,
                "control point {index} is past the last of {count} control points"
            ),
            Self::ControlPointsShort {
                span,
                degree,
                count,
            } => write!(
                f,
                "span {span} of a degree-{degree} curve needs more than its {count} control \
                 points"
            ),
            Self::CurvesNotCoplanar { tolerance } => {
                write!(f, "a curve is not on the plane within {tolerance}")
            }
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::DegreeBelowCurrent { given, current } => {
                write!(f, "degree {given} is below the curve's degree {current}")
            }
            Self::DegreeNotReducible { error, tolerance } => write!(
                f,
                "the degree cannot be reduced by one within the tolerance {tolerance} (error \
                 bound {error})"
            ),
            Self::DegreeOutside { given, min, max } => {
                write!(f, "degree {given} is outside [{min}, {max}]")
            }
            Self::DegreeTooLarge { given } => {
                write!(f, "degree {given} does not fit an index here")
            }
            Self::DegreeTooLowToReduce { degree } => {
                write!(f, "a curve of degree {degree} has no lower degree")
            }
            Self::DerivativeCurvesMissing => f.write_str("the derivative curves are missing"),
            Self::DimensionNot2Or3 { dim } => {
                write!(f, "the points are of dimension {dim}, not 2 or 3")
            }
            Self::DirectionNotUOrV { direction } => {
                write!(f, "the direction is {direction:?}, not U or V")
            }
            Self::EmptyGrid { rows, columns } => {
                write!(f, "the grid has {rows} rows and {columns} columns")
            }
            Self::EndTangentNotGiven => f.write_str("the end tangent is specified but not given"),
            Self::EndsApart {
                distance,
                tolerance,
            } => write!(
                f,
                "the ends to join are {distance} apart, farther than the tolerance {tolerance}"
            ),
            Self::EndsCoincide => f.write_str("the curve's ends coincide"),
            Self::ExpansionChangesCounts => {
                f.write_str("expanding the boundary span changes the surface's counts")
            }
            Self::FairBlendInputs { points, tangents } => write!(
                f,
                "{points} points and {tangents} tangents are given, not two of each"
            ),
            Self::FreeBoundaryNotDegenerate => {
                f.write_str("a boundary curve without a surface is not degenerate")
            }
            Self::GeneratorOnAxis => {
                f.write_str("every control point of the generating curve is on the axis")
            }
            Self::IntervalNotSplittable { lo, hi } => write!(
                f,
                "the parameter interval [{lo}, {hi}] cannot be subdivided further"
            ),
            Self::IntervalSelectsNoControlPoints => {
                f.write_str("the interval selects no control point")
            }
            Self::JoinEndWeightZero => f.write_str("an end weight of the curves to join is zero"),
            Self::JoinNotBuilt => f.write_str("the curves could not be joined into one"),
            Self::NetIndexOutside {
                u_index,
                v_index,
                u_last,
                v_last,
            } => write!(
                f,
                "control point ({u_index}, {v_index}) is past the last, ({u_last}, {v_last})"
            ),
            Self::NoControlPoints => f.write_str("no control points are asked for"),
            Self::NoFootWithinTolerance { tolerance } => {
                write!(
                    f,
                    "no point of the surface was found within {tolerance} of the point"
                )
            }
            Self::NoNurbGiven => f.write_str("no NURB is given"),
            Self::NoNurbsForm => f.write_str("it holds no NURBS form"),
            Self::NoOutputSlot => f.write_str("the output has no slot for the point"),
            Self::NotAnArc => f.write_str("a boundary curve is not an arc"),
            Self::NotIntersectionCurve => {
                f.write_str("the curve is not a piecewise degree-3 intersection curve")
            }
            Self::NotRational => f.write_str("the curve is not rational"),
            Self::ParameterOutside { parameter, lo, hi } => {
                write!(
                    f,
                    "parameter {parameter} is outside the domain [{lo}, {hi}]"
                )
            }
            Self::PatchTooCoarse {
                u_count,
                v_count,
                u_degree,
                v_degree,
            } => write!(
                f,
                "the corner patch has {u_count} x {v_count} control points of degrees \
                 {u_degree}, {v_degree}, fewer than five or degree 0 in a direction"
            ),
            Self::PatchWithoutSpan => {
                f.write_str("the corner patch has fewer than two knots in a direction")
            }
            Self::PointOffSurface {
                residual,
                tolerance,
            } => write!(
                f,
                "a point projects onto its surface with residual {residual}, more than \
                 {tolerance}"
            ),
            Self::RailIntersectionsMissing => {
                f.write_str("the rails are given without their intersections")
            }
            Self::RoutineFailed => f.write_str("a NURBS routine it calls failed"),
            Self::SphereArcCount { given } => {
                write!(f, "a sphere is made from three or four arcs, not {given}")
            }
            Self::SpineRailMissing => f.write_str("the spine rail is missing"),
            Self::StartTangentNotGiven => {
                f.write_str("the start tangent is specified but not given")
            }
            Self::SubdivisionCap { limit } => {
                write!(f, "the tangent field needs more than {limit} subdivisions")
            }
            Self::TangentsVanish => f.write_str("both first derivatives cannot be made nonzero"),
            Self::TooFewBoundaryCurves { given } => write!(
                f,
                "a corner blend needs at least four boundary curves, not {given}"
            ),
            Self::TooFewBreakpoints { given } => {
                write!(f, "the curve has {given} breakpoints, fewer than two")
            }
            Self::TooManyVectors { given } => {
                write!(f, "{given} end vectors are given, more than two")
            }
            Self::TrimDomainOutside => {
                f.write_str("the trim domain is not inside the surface's domain")
            }
            Self::WeightOnNonRational => {
                f.write_str("a weight is given for a curve that is not rational")
            }
            Self::ZeroAxis => f.write_str("the axis direction is a zero vector"),
        }
    }
}

impl NurbsFailure {
    /// The status the C++ reports for the failure (a comparison detail): the `Sm*`
    /// callers surface an NLib failure as `SER(SM_ERR)`; a B-spline method's failure
    /// has its own.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::BSpline(b) => b.comparison_status(),
            _ => SM_ERR,
        }
    }

    /// The `NL_*_ERR` number the C++ stores for the failure (a comparison detail);
    /// `None` for a B-spline method's failure, which stores none.
    pub(crate) const fn nlib_number(&self) -> Option<i32> {
        Some(match self {
            Self::BSpline(_) => return None,
            Self::CallFailed => 13,
            Self::CurveDefinition => 1,
            Self::Degree | Self::DegreeTooHigh { .. } => 3,
            Self::DerivativeOrder { .. } | Self::NoDerivativeRows => 20,
            Self::Geometry => 17,
            Self::Index | Self::IndexOutOfRange { .. } => 7,
            Self::Input => 14,
            Self::Knots | Self::KnotsDecrease { .. } => 4,
            Self::Memory => 15,
            Self::NotConverged => 12,
            Self::Numerical => 11,
            Self::Parameter | Self::ParameterOutside { .. } => 6,
            Self::Singular => 16,
            Self::Storage => 9,
            Self::SurfaceDefinition => 2,
            Self::Weight | Self::WeightOutOfRange { .. } => 5,
            Self::ZeroDerivative => 8,
        })
    }

    /// The failure's stable [`ErrorKind`].
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::BSpline(b) => b.kind(),
            Self::NotConverged => ErrorKind::NotConverged,
            Self::Parameter | Self::ParameterOutside { .. } => ErrorKind::OutsideDomain,
            _ => ErrorKind::Failed,
        }
    }
}

impl core::fmt::Display for NurbsFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BSpline(b) => b.fmt(f),
            Self::CallFailed => f.write_str(
                "a routine it calls rejected its arguments, or the direction is not handled",
            ),
            Self::CurveDefinition => {
                f.write_str("the curve's control point, knot and degree counts do not agree")
            }
            Self::Degree => f.write_str("the degree is not allowed here"),
            Self::DegreeTooHigh { given, max } => {
                write!(f, "degree {given} is above the largest degree, {max}")
            }
            Self::DerivativeOrder { given, max } => {
                write!(
                    f,
                    "derivative order {given} is above the largest order, {max}"
                )
            }
            Self::Geometry => f.write_str("the geometry does not allow the operation"),
            Self::Index => f.write_str("an index is out of range"),
            Self::IndexOutOfRange { index, len } => {
                write!(f, "index {index} is out of range for {len} items")
            }
            Self::Input => f.write_str("the input data is not valid"),
            Self::Knots => f.write_str("the knot vector is not valid here"),
            Self::KnotsDecrease { index } => {
                write!(f, "the knot vector decreases at knot {index}")
            }
            Self::Memory => f.write_str("the memory for the result cannot be allocated"),
            Self::NoDerivativeRows => f.write_str("the output has no row for the point itself"),
            Self::NotConverged => f.write_str("the iteration did not converge"),
            Self::Numerical => f.write_str("a numerical failure (a division by zero or overflow)"),
            Self::Parameter => f.write_str("a parameter is outside the domain"),
            Self::ParameterOutside { parameter, lo, hi } => {
                write!(
                    f,
                    "parameter {parameter} is outside the domain [{lo}, {hi}]"
                )
            }
            Self::Singular => f.write_str("the system of equations is singular"),
            Self::Storage => f.write_str("a buffer supplied by the caller is too small"),
            Self::SurfaceDefinition => {
                f.write_str("the surface's control net, knot and degree counts do not agree")
            }
            Self::Weight => f.write_str("a weight is not valid here"),
            Self::WeightOutOfRange {
                index,
                weight,
                min,
                max,
            } => write!(
                f,
                "the weight of control point {index} is {weight}, outside [{min}, {max}]"
            ),
            Self::ZeroDerivative => f.write_str("a derivative is zero where a direction is needed"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a NURBS routine that fails with `failure`; the
    /// status its `Sm*` callers report for it is `SM_ERR` (NLib's `N_ErrSet` followed by
    /// an error return).
    #[must_use]
    pub(crate) fn nurbs(operation: &'static str, failure: NurbsFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// The NURBS failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Nurbs`].
    #[must_use]
    pub fn nurbs_failure(&self) -> Option<&NurbsFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Nurbs(n) => Some(n),
                _ => None,
            },
            _ => None,
        }
    }

    /// True for the failure of a NURBS routine ([`Reason::Nurbs`], not a
    /// [`BSplineFailure`]). The callers that ignore an NLib failure's flag, as upstream
    /// does, drop it by this test.
    #[must_use]
    pub(crate) fn is_nurbs_failure(&self) -> bool {
        self.nurbs_failure()
            .is_some_and(|n| !matches!(n, NurbsFailure::BSpline(_)))
    }

    /// [`crate::SolidError::Failed`] for a B-spline curve or surface method that fails
    /// with `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn b_spline(operation: &'static str, failure: BSplineFailure) -> Self {
        Failure::new(operation, NurbsFailure::BSpline(failure)).into_error()
    }

    /// [`crate::SolidError::b_spline`] reporting `status` instead of the cause's own: a site
    /// whose status upstream differs from the cause's (for example `SM_ERR_NULL_POINTER` for
    /// a [`BSplineFailure::NoNurbsForm`] where upstream's `NER` returns it).
    #[must_use]
    #[track_caller]
    pub(crate) fn b_spline_with_status(
        operation: &'static str,
        failure: BSplineFailure,
        status: i32,
    ) -> Self {
        Failure::new(operation, NurbsFailure::BSpline(failure))
            .with_status(status)
            .into_error()
    }

    /// Maps the error of a failed NLib call inside a B-spline method (upstream
    /// `if (NL_YES == N_...) SER(SM_ERR)`) to a [`BSplineFailure::RoutineFailed`] of
    /// `operation` that keeps the call's error as its cause. [`crate::SolidError::Unported`],
    /// [`crate::SolidError::Cancelled`], [`crate::SolidError::PreparationRequired`] and a
    /// blend certificate past its budget pass through unchanged: no upstream status stands
    /// for them, and callers match them at the top. Use as
    /// `.map_err(SolidError::nurbs_routine(OP))`.
    pub(crate) fn nurbs_routine(operation: &'static str) -> impl Fn(Self) -> Self {
        move |e| match e {
            Self::Unported { .. } | Self::Cancelled { .. } | Self::PreparationRequired { .. } => e,
            e if e.is_not_certified() => e,
            _ => Failure::new(
                operation,
                NurbsFailure::BSpline(BSplineFailure::RoutineFailed),
            )
            .caused_by(e)
            .into_error(),
        }
    }
}

/// The `NL_*_ERR` number a NURBS failure stands for, when `error` is one.
///
/// For difftest's comparison only. Callers match [`crate::SolidError::kind`] and the
/// [`Reason`].
#[must_use]
pub fn nlib_number(error: &crate::SolidError) -> Option<i32> {
    match error {
        crate::SolidError::Failed(f) => match &f.reason {
            Reason::Nurbs(n) => n.nlib_number(),
            _ => None,
        },
        _ => None,
    }
}

/// [`super::upstream_status`], kept at the path the goldens runners first read it from.
pub use super::upstream_status;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SolidError;

    /// The bare status a reviewed defect at `status` replaced.
    fn upstream(code: i32) -> SolidError {
        SolidError::Upstream {
            operation: "nurbs::op",
            code,
            entity: None,
        }
    }

    /// A typed cause keeps the status of the bare status it replaced.
    #[test]
    fn a_typed_cause_keeps_its_status() {
        for cause in [
            BSplineFailure::EndsCoincide,
            BSplineFailure::EndsApart {
                distance: 0.5,
                tolerance: 0.01,
            },
        ] {
            let e = SolidError::b_spline("nurbs::op", cause);
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{cause:?}");
            assert_eq!(e.kind(), ErrorKind::Failed, "{cause:?}");
            assert!(!e.is_defect() && e.is_status_answer(), "{cause:?}");
        }
    }

    /// The named causes keep the status of the bare status they replace, so
    /// `upstream_status()` reads it, and carry the class of what they check.
    #[test]
    fn the_named_causes_keep_their_status_and_name_their_class() {
        let invalid = [
            BSplineFailure::ContinuityNotExtendable {
                continuity: ContinuityType::C0,
            },
            BSplineFailure::DimensionNot2Or3 { dim: 4 },
            BSplineFailure::TooManyVectors { given: 3 },
            BSplineFailure::NoControlPoints,
            BSplineFailure::DegreeTooLowToReduce { degree: 1 },
            BSplineFailure::StartTangentNotGiven,
            BSplineFailure::EndTangentNotGiven,
            BSplineFailure::IntervalSelectsNoControlPoints,
            BSplineFailure::JoinEndWeightZero,
            BSplineFailure::ControlPointCountsDiffer { ours: 5, theirs: 2 },
            BSplineFailure::DegreeBelowCurrent {
                given: 2,
                current: 3,
            },
            BSplineFailure::ControlPointIndex { index: 9, count: 5 },
            BSplineFailure::ControlPointsShort {
                span: 2,
                degree: 3,
                count: 2,
            },
            BSplineFailure::SphereArcCount { given: 2 },
            BSplineFailure::NotAnArc,
            BSplineFailure::ArcRadiiDiffer {
                radius: 1.0,
                other: 2.0,
                tolerance: 1e-6,
            },
            BSplineFailure::ArcCentresApart {
                distance: 1.0,
                tolerance: 1e-6,
            },
            BSplineFailure::ArcsDoNotMeet,
            BSplineFailure::CurvesNotCoplanar { tolerance: 1e-6 },
            BSplineFailure::NetIndexOutside {
                u_index: 5,
                v_index: 0,
                u_last: 2,
                v_last: 2,
            },
            BSplineFailure::DirectionNotUOrV {
                direction: SurfParamType::Both,
            },
            BSplineFailure::DegreeTooLarge { given: u64::MAX },
            BSplineFailure::EmptyGrid {
                rows: 0,
                columns: 4,
            },
            BSplineFailure::TooFewBreakpoints { given: 1 },
            BSplineFailure::TooFewBoundaryCurves { given: 3 },
            BSplineFailure::TrimDomainOutside,
            BSplineFailure::FreeBoundaryNotDegenerate,
            BSplineFailure::NoOutputSlot,
            BSplineFailure::WeightOnNonRational,
            BSplineFailure::NoNurbGiven,
            BSplineFailure::FairBlendInputs {
                points: 2,
                tangents: 1,
            },
            BSplineFailure::DerivativeCurvesMissing,
        ];
        let unsupported = [
            BSplineFailure::NotRational,
            BSplineFailure::NotIntersectionCurve,
            BSplineFailure::ArcKindUnsupported,
        ];
        let degenerate = [
            BSplineFailure::ArcLoopDegenerate {
                radius: 0.0,
                facing: 1.0,
                tolerance: 1e-6,
            },
            BSplineFailure::GeneratorOnAxis,
            BSplineFailure::TangentsVanish,
            BSplineFailure::PatchWithoutSpan,
            BSplineFailure::PatchTooCoarse {
                u_count: 4,
                v_count: 9,
                u_degree: 3,
                v_degree: 3,
            },
            BSplineFailure::ArcSweepDegenerate { angle: 0.0 },
        ];
        let tolerance = [
            BSplineFailure::DegreeNotReducible {
                error: 0.5,
                tolerance: 0.01,
            },
            BSplineFailure::PointOffSurface {
                residual: 0.5,
                tolerance: 1e-8,
            },
            BSplineFailure::SubdivisionCap { limit: 8192 },
            BSplineFailure::IntervalNotSplittable { lo: 1.0, hi: 1.0 },
            BSplineFailure::NoFootWithinTolerance { tolerance: 1e-6 },
        ];
        // a result the method could not build from valid input: class `failed`
        let failed = [BSplineFailure::ExpansionChangesCounts];
        // the caller's missing inputs keep upstream's `NER` status
        for cause in [
            BSplineFailure::BoundaryWithoutSurface,
            BSplineFailure::BoundaryCurveMissing,
            BSplineFailure::SpineRailMissing,
            BSplineFailure::RailIntersectionsMissing,
        ] {
            let e = SolidError::b_spline("nurbs::op", cause);
            assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER), "{cause:?}");
            assert_eq!(e.kind(), ErrorKind::InvalidInput, "{cause:?}");
        }
        for (causes, kind) in [
            (&invalid[..], ErrorKind::InvalidInput),
            (&unsupported[..], ErrorKind::Unsupported),
            (&degenerate[..], ErrorKind::Degenerate),
            (&tolerance[..], ErrorKind::NotWithinTolerance),
            (&failed[..], ErrorKind::Failed),
        ] {
            for &cause in causes {
                let e = SolidError::b_spline("nurbs::op", cause);
                assert_eq!(e.upstream_status(), Some(SM_ERR), "{cause:?}");
                assert_eq!(e.sm_status(), Some(SM_ERR), "{cause:?}");
                assert_eq!(e.kind(), kind, "{cause:?}");
                assert!(!e.is_defect() && !e.is_refusal(), "{cause:?}");
                assert_eq!(nlib_number(&e), None, "{cause:?}");
            }
        }
        // the numbers land in the text in their order
        assert_eq!(
            BSplineFailure::ControlPointCountsDiffer { ours: 5, theirs: 2 }.to_string(),
            "the curves have 5 and 2 control points, not the same number"
        );
        assert_eq!(
            BSplineFailure::ControlPointIndex { index: 9, count: 5 }.to_string(),
            "control point 9 is past the last of 5 control points"
        );
    }

    /// A reviewed defect is of class 127 with its site's status, and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the knot vector the insertion just built does not rise, which insertion keeps";
        let defect = BSplineFailure::Defect { why };
        let e = SolidError::b_spline("nurbs::op", defect);
        super::super::tests::assert_defect(
            &e,
            &upstream(SM_ERR),
            "nurbs::op",
            SM_ERR,
            &format!("nurbs::op: stage_failed: {why}"),
        );
        assert_eq!(e.nurbs_failure(), Some(&NurbsFailure::BSpline(defect)));
        assert_eq!(nlib_number(&e), None);
        let missing = Failure::new("nurbs::op", NurbsFailure::BSpline(defect))
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &upstream(SM_ERR_NULL_POINTER),
            "nurbs::op",
            SM_ERR_NULL_POINTER,
            &format!("nurbs::op: stage_failed: {why}"),
        );
    }
}
