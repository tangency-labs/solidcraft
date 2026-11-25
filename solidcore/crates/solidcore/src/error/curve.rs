//! Why a curve query refuses, and why a curve routine fails, in solidcore's terms (#3184).
//!
//! Upstream: none. A curve's certified length ([`crate::curve::curve::Curve::length_bound`])
//! is a bracket whose width cannot fall below the rounding of the pieces it is summed over:
//! asked for a narrower one, it refuses and names the narrowest it proves. Asked for one
//! the curve would need more than the cap of pieces for, it refuses and names the width
//! reached at the cap. A piece found outside the knot span it is blossomed in (which the
//! halving never makes, and the rounding bound assumes) is refused, named (#14434). The other
//! failures describe the curve routines (`crate::curve`: the
//! curve kinds' evaluation, editing, reading and conversion, their caches, solvers and
//! property functions), each recording, for difftest only, the status the C++ reports for it
//! (crate-private `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                 | Status difftest compares |
//! |--------------------------------------|--------------------------|
//! | [`CurveFailure::AuxiliaryAlongProjection`] | `SM_ERR_INVALID_INPUT`  |
//! | [`CurveFailure::Defect`]             | the site's (`SM_ERR`, or another through `with_status`) |
//! | [`CurveFailure::Failed`]             | `SM_ERR`                 |
//! | [`CurveFailure::InfiniteTangent`], [`CurveFailure::ZeroTangent`], [`CurveFailure::PointOnCurveWhenMaximising`] | `SM_ERR` |
//! | [`CurveFailure::LengthBelowFloor`]   | `SM_ERR`                 |
//! | [`CurveFailure::LengthPastPieceCap`] | `SM_ERR`                 |
//! | [`CurveFailure::Missing`]            | `SM_ERR_NULL_POINTER`    |
//! | [`CurveFailure::PieceOutsideSpan`]   | `SM_ERR`                 |
//! | [`CurveFailure::PointSolveOptionMissing`] | `SM_ERR_NULL_POINTER` |
//! | [`CurveFailure::SingleLinearCurve`]  | `SM_ERR`                 |
//! | [`CurveFailure::StageFailed`]        | the site's (`SM_ERR`, or `SM_ERR_NULL_POINTER` for an `NER`) |
//! | [`CurveFailure::ZeroDirection`]      | `SM_ERR_INVALID_INPUT`   |
//! | the other named causes ([`CurveFailure::TooManyDerivatives`], [`CurveFailure::BaseCurveNotOwned`], ...) | `SM_ERR` |
//!
//! **The record** (#15034). A check that refuses builds a typed cause with its numbers
//! ([`CurveFailure::SingleLinearCurve`], [`CurveFailure::InfiniteTangent`], ...); a stage that
//! fails with no reason the kernel names is a [`CurveFailure::StageFailed`], a defect
//! ([`ErrorKind::StageFailed`]), keeping the error of the call it made as its cause where it
//! has one. The phrase causes ([`CurveFailure::Failed`], [`CurveFailure::Missing`],
//! [`CurveFailure::ZeroDirection`]) are sites not yet converted, and so is a
//! [`CurveFailure::StageFailed`] not yet reviewed: a reviewed one is a
//! [`CurveFailure::Defect`] with its reason (the conversion guide:
//! `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. No failure names an entity: the sites they
//! replace named none. A check of a caller's argument that reported a status is a named
//! cause of kind [`ErrorKind::InvalidInput`] keeping that status; the checks that reported
//! none stay [`crate::SolidError::InvalidInput`].

use super::{ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER};

/// Why a curve query refuses.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CurveFailure {
    /// The span's quadratic for the angle `angle_deg` has no root in `[0, 1]`: rounding at a
    /// span of 180 degrees or more (its discriminant is a square, never negative exactly),
    /// or a span whose end angles are not those of one rational quadratic arc in standard
    /// form. [`ErrorKind::Failed`].
    AngleHasNoNurbParameter {
        /// The angle, in degrees.
        angle_deg: f64,
    },
    /// The angle `angle_deg` is outside the arc from `start_deg` to `end_deg` it is
    /// converted in: the caller's angle (the conversion's own call passes an angle within
    /// its table's span, so there it would mean a table whose angles do not increase).
    /// [`ErrorKind::InvalidInput`].
    AngleOutsideArc {
        /// The angle given, in degrees.
        angle_deg: f64,
        /// The arc's start angle, in degrees.
        start_deg: f64,
        /// The arc's end angle, in degrees.
        end_deg: f64,
    },
    /// A rotation's auxiliary vector is parallel to the projection vector, so it has no
    /// direction normal to it. Status `SM_ERR_INVALID_INPUT`. [`ErrorKind::InvalidInput`].
    AuxiliaryAlongProjection,
    /// A rotation's auxiliary vector is unset or zero. [`ErrorKind::InvalidInput`].
    AuxiliaryVectorZero,
    /// The curve kind leaves `method` to the curve base class, which does not implement it.
    /// [`ErrorKind::Unsupported`].
    BaseCurveMethod {
        /// The method.
        method: BaseCurveMethod,
    },
    /// The curve refers to a base curve it does not own, so the routine may not `edit`
    /// it. The caller's curve as supplied: [`ErrorKind::InvalidInput`].
    BaseCurveNotOwned {
        /// The edit refused.
        edit: CurveEdit,
    },
    /// The curve's cached breaks and continuities are not current: the caller asked before
    /// the breaks were computed (a new, trimmed or reparametrised curve), or loaded a cache
    /// whose counts differ. [`ErrorKind::InvalidInput`].
    BreaksNotCurrent,
    /// The curve cache a solve needs could not be built (the build's error is the cause).
    /// Status `SM_ERR_NULL_POINTER`. [`ErrorKind::Failed`]; whether it is a defect or a
    /// refusal is its cause's.
    CacheNotBuilt,
    /// The cache asked of a curve is not a curve cache. [`ErrorKind::InvalidInput`].
    CacheTypeNotCurve,
    /// The cached continuity at the break `at` is undefined: the kernel never caches one,
    /// so it came with the curve the caller loaded. [`ErrorKind::InvalidInput`].
    ContinuityUndefined {
        /// The break.
        at: f64,
    },
    /// The span given to the span measure has `points` control points, fewer than the
    /// two its base line needs. [`ErrorKind::InvalidInput`].
    ControlPolygonTooShort {
        /// The control points there are.
        points: usize,
    },
    /// An offset corner by curve extension is asked of the corner builder, which builds
    /// line, fillet and chamfer corners only (the taper extrude continues each wall
    /// itself). [`ErrorKind::Unsupported`].
    CornerByCurveExtension,
    /// For an angle property, a corner of the span's pseudo box projects onto the view
    /// origin, where it has no angle: the property solve stops there (upstream's status 1,
    /// set by its site). [`ErrorKind::Failed`].
    CornerOnViewAxis,
    /// A curve tree node the solver subdivides has no child: a curve cache's tree gives
    /// every node it subdivides two, so the tree was the caller's. Status
    /// `SM_ERR_NULL_POINTER`. [`ErrorKind::InvalidInput`].
    CurveNodeWithoutChild,
    /// A curve tree node of the solver's branch has no span data: a curve cache's tree
    /// carries a span at every node it builds, so the tree was the caller's. Status
    /// `SM_ERR_NULL_POINTER`. [`ErrorKind::InvalidInput`].
    CurveNodeWithoutData,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The ellipse has no NURB, which its polar conversion and its canonical form are made
    /// from: an empty ellipse, or one whose NURB build failed (the constructors ignore that
    /// status). [`ErrorKind::Degenerate`].
    EllipseWithoutNurb,
    /// The ellipse's NURB does not start and end in knots of full multiplicity (3), so
    /// its spans do not start at its ends and the polar conversion does not apply.
    /// [`ErrorKind::Unsupported`].
    EndKnotsNotClamped,
    /// The point of the tangent field's end curve at its mated parameter lies `gap` from the
    /// end of the base curve, farther than the tolerance `tolerance`.
    /// [`ErrorKind::NotWithinTolerance`].
    EndsApart {
        /// The distance between the two ends.
        gap: f64,
        /// The tolerance given.
        tolerance: f64,
    },
    /// A curve routine could not continue, as the phrase says (for example "the split
    /// parameter is outside the curve's interval").
    Failed(&'static str),
    /// The fast point drop does not take this curve or this solve (the callers then run
    /// the general point solver). [`ErrorKind::Unsupported`].
    FastDropUnavailable {
        /// What the fast drop lacks.
        reason: FastDropBlock,
    },
    /// A curve's property or solve function is undefined at the curve parameter `t`, for
    /// the reason `why` (the solver that evaluates it there decides what follows).
    /// [`ErrorKind::Failed`], as [`CurveFailure::ZeroTangent`].
    FunctionUndefined {
        /// The curve parameter (of the curve the reason concerns).
        t: f64,
        /// Why the function is undefined there.
        why: FunctionGap,
    },
    /// The curve's tangent at `t` is infinite, so the closest-point function cannot be
    /// scaled down to evaluate there.
    InfiniteTangent {
        /// The curve parameter.
        t: f64,
    },
    /// The approximation's step `[lo, hi]` cannot be split further in floating point (its
    /// midpoint is an end, or not finite), and its span still does not pass the tolerance.
    /// [`ErrorKind::NotWithinTolerance`].
    IntervalNotSplittable {
        /// The step's start.
        lo: f64,
        /// The step's end.
        hi: f64,
    },
    /// The offset curve's interval is not within its base curve's natural interval.
    /// The caller's curve: [`ErrorKind::InvalidInput`].
    IntervalOutsideBase,
    /// The curve to join on differs from this one by `off` in `check`, more than the
    /// tolerance `tolerance`: the two are not pieces of one curve.
    /// [`ErrorKind::NotWithinTolerance`].
    JoinMismatch {
        /// What differs.
        check: JoinCheck,
        /// By how much (a distance, or for an axis the sine of the angle between them).
        off: f64,
        /// The tolerance.
        tolerance: f64,
    },
    /// The curve to join on is not of this curve's kind. [`ErrorKind::Unsupported`].
    JoinOtherKind,
    /// The curve to join on a line is not straight within the tolerance `tolerance`.
    /// [`ErrorKind::NotWithinTolerance`].
    JoinOtherNotStraight {
        /// The tolerance.
        tolerance: f64,
    },
    /// The knot `knot` the caller gave lies past the curve's last cached break.
    /// [`ErrorKind::InvalidInput`].
    KnotPastBreaks {
        /// The knot given.
        knot: f64,
    },
    /// A length bracket was asked `asked` wide, below `floor`, the sum over the `pieces` the
    /// curve was halved into of each piece's rounding bound: halving further only widens it.
    LengthBelowFloor {
        /// The width asked.
        asked: f64,
        /// The narrowest width the bracket proves: the pieces' rounding, summed.
        floor: f64,
        /// The pieces the halving used.
        pieces: usize,
    },
    /// A length bracket was asked `asked` wide, and the curve was halved into its most
    /// pieces, `pieces`, with its bracket still `reached` wide: it could narrow further but
    /// is not halved past the cap.
    LengthPastPieceCap {
        /// The width asked.
        asked: f64,
        /// The bracket's width at the cap.
        reached: f64,
        /// The pieces at the cap.
        pieces: usize,
    },
    /// The distance under which a gap gets a line, `distance`, is neither zero nor above the
    /// same-point tolerance `tolerance`. [`ErrorKind::InvalidInput`].
    LineDistanceNotAboveTolerance {
        /// The distance given.
        distance: f64,
        /// The same-point tolerance given.
        tolerance: f64,
    },
    /// A solver's local solve failed (its error is the cause). [`ErrorKind::Failed`];
    /// whether it is a defect or a refusal is its cause's.
    LocalSolveFailed,
    /// A hyperbola's arc is numerically degenerate for its rational quadratic form: the
    /// line from its middle point toward the end tangents' meeting point cuts the chord at
    /// `at` of the chord's length, within the kernel's zero of an end (exactly, it cuts it
    /// at the middle; only rounding moves it there). [`ErrorKind::Degenerate`].
    MidTangentOffChord {
        /// Where the line cuts the chord, as a fraction of the chord from its start.
        at: f64,
    },
    /// Something a curve routine needs is missing, as the phrase says (for example "the
    /// curve's cache").
    Missing(&'static str),
    /// A composite curve is evaluated through its segments; it has no evaluation of its own
    /// at a parameter of the whole. [`ErrorKind::Unsupported`].
    NoEvaluationOfItsOwn,
    /// An evaluation into the caller's slice was given an empty slice.
    /// [`ErrorKind::InvalidInput`].
    NoEvaluationSlots,
    /// No curve gave an offset: there were no segments, or every segment's offset was
    /// degenerate and dropped. [`ErrorKind::Degenerate`].
    NoOffsetCurve,
    /// The base curve's tangent at `t` is zero, or parallel to the offset normal (their
    /// binormal is zero), so the offset has no direction there. [`ErrorKind::Degenerate`].
    NoOffsetDirection {
        /// The base curve's parameter the evaluation was asked at.
        t: f64,
    },
    /// The property function was asked for a property it has no equation for (plane
    /// classification is answered without one). [`ErrorKind::Unsupported`].
    NoPropertyFunction,
    /// The curve has no span of non-zero length to build its cache or Bezier spans
    /// from (none within the cache's interval, or its NURB has none).
    /// [`ErrorKind::Degenerate`].
    NoSpans,
    /// An analytic curve (a conic) is asked to scale by `scale`, which is not the same
    /// along every axis: its kind keeps its shape only under a uniform scale.
    /// [`ErrorKind::Unsupported`].
    NonUniformScale {
        /// The scale asked for, per axis.
        scale: crate::geom::vector3d::Vec3,
    },
    /// The ellipse's NURB could not be transformed (the transform's error is the cause).
    /// [`ErrorKind::Failed`]; whether it is a defect is its cause's.
    NurbNotTransformed,
    /// The offset direction given for a curve, `direction`, is not 1 (left), 2 (right)
    /// or 3 (both sides). [`ErrorKind::InvalidInput`].
    OffsetDirectionUnknown {
        /// The direction given.
        direction: u64,
    },
    /// The circle's offset in its plane would have the radius `offset_radius`, below zero
    /// by more than the tolerance `tolerance`: the offset distance passes the centre. An
    /// offset distance that leaves the radius at least `-tolerance` passes.
    /// [`ErrorKind::InvalidInput`].
    OffsetRadiusNegative {
        /// The radius the offset would have.
        offset_radius: f64,
        /// The tolerance.
        tolerance: f64,
    },
    /// The offset curve has no base curve: it was made empty. Status
    /// `SM_ERR_NULL_POINTER`. The caller's curve: [`ErrorKind::InvalidInput`].
    OffsetWithoutBase,
    /// The curve/curve solve filled all `limit` of its parameter guess slots.
    /// [`ErrorKind::Unsupported`].
    ParameterGuessCap {
        /// The most guesses.
        limit: usize,
    },
    /// The piece added to a piecewise Bezier curve does not start where the curve ends: its
    /// start is `knot_gap` from the last knot and `point_gap` from the last control point,
    /// one of them past its tolerance. [`ErrorKind::InvalidInput`].
    PieceNotContinuous {
        /// The distance from the last knot to the piece's start parameter.
        knot_gap: f64,
        /// The distance from the last control point to the piece's start point.
        point_gap: f64,
    },
    /// A piece `[a, b]` of a length bracket lies outside the knot span
    /// `[span_start, span_end]` it is blossomed in: the bracket's rounding bound holds only
    /// for a piece within its span (a port invariant: the halving keeps every piece within).
    PieceOutsideSpan {
        /// The piece's start.
        a: f64,
        /// The piece's end.
        b: f64,
        /// The knot span's start.
        span_start: f64,
        /// The knot span's end.
        span_end: f64,
    },
    /// The ellipse's placement, scaled, could not be set (the placement's error is the
    /// cause). [`ErrorKind::Failed`]; whether it is a defect is its cause's.
    PlacementNotSet,
    /// The point lies on the curve at `t`, where a solve for the farthest point finds no
    /// extremum (the gap is zero there).
    PointOnCurveWhenMaximising {
        /// The curve parameter.
        t: f64,
    },
    /// A ray solve ([`crate::curve::curve::SolverOperation::Rayfire`] or
    /// `Signed3dDirectedMinimize`) was called without `option`, which it requires: the
    /// caller's argument, so of kind [`ErrorKind::InvalidInput`], not a defect.
    PointSolveOptionMissing {
        /// The option left out.
        option: PointSolveOption,
    },
    /// A polar conversion is asked of a table that was never set up from an ellipse.
    /// Status `SM_ERR_NULL_POINTER`. The caller's state: [`ErrorKind::InvalidInput`].
    PolarConversionNotSetUp,
    /// A projected curve's projection vector is unset or zero. [`ErrorKind::InvalidInput`].
    ProjectionVectorZero,
    /// A property or solve function was called without `option`, which its property or
    /// operation reads. The caller's options: [`ErrorKind::InvalidInput`].
    PropertyOptionMissing {
        /// The option left out.
        option: PropertyOption,
    },
    /// A circle's radius given, `radius`, is not above the kernel's zero (or is not a
    /// number). [`ErrorKind::InvalidInput`].
    RadiusNotPositive {
        /// The radius given.
        radius: f64,
    },
    /// A range solution given to the curve/curve refinement has `variables` variables, not
    /// the two (one per curve) it refines. The caller's solutions:
    /// [`ErrorKind::InvalidInput`].
    RangeSolutionVariables {
        /// The solution's variables.
        variables: usize,
    },
    /// The property function could not be evaluated at the root `t` its solve found (the
    /// evaluation's error is the cause). [`ErrorKind::Failed`]; whether it is a defect or a
    /// refusal is its cause's.
    RootNotEvaluated {
        /// The root's parameter.
        t: f64,
    },
    /// The same-point tolerance given, `tolerance`, is not positive.
    /// [`ErrorKind::InvalidInput`].
    SamePointToleranceNotPositive {
        /// The tolerance given.
        tolerance: f64,
    },
    /// A surface-projected curve intersection was asked of a second curve that is not a
    /// curve on a surface. [`ErrorKind::InvalidInput`].
    SecondCurveNotOnSurface,
    /// The only curve whose area normal is asked is a line: one line spans no plane.
    SingleLinearCurve,
    /// Object `object` (0 or 1) of a curve/curve solution is no curve the caller's lookup
    /// knows. Status `SM_ERR_NULL_POINTER`. The caller's solutions and lookup:
    /// [`ErrorKind::InvalidInput`].
    SolutionWithoutCurve {
        /// The solution's object, 0 or 1.
        object: usize,
    },
    /// The span given to the span measure is not a B-spline curve.
    /// [`ErrorKind::InvalidInput`].
    SpanNotBSpline,
    /// The split parameter `split` is at the curve's end `end`, within
    /// `100 * SM_EFF_ZERO * (1 + |end|)` of it: one piece would be empty.
    /// [`ErrorKind::InvalidInput`].
    SplitAtEnd {
        /// The split parameter given.
        split: f64,
        /// The end it is at (the interval's start or end).
        end: f64,
    },
    /// The split parameter `split` is outside the curve's interval `[start, end]`.
    /// [`ErrorKind::InvalidInput`].
    SplitOutsideInterval {
        /// The split parameter given.
        split: f64,
        /// The interval's start.
        start: f64,
        /// The interval's end.
        end: f64,
    },
    /// The approximation stack holds `values` values, fewer than the 7 of one
    /// `[t, position, tangent]` entry. [`ErrorKind::InvalidInput`].
    StackTooShort {
        /// The values on the stack.
        values: usize,
    },
    /// A stage of a curve routine failed with no reason the kernel names: a defect
    /// ([`ErrorKind::StageFailed`]). The failure holds the operation, the status the C++
    /// reports (for the goldens only), where it was built, and the error of the call it made
    /// where it has one. The form not yet reviewed: new code builds
    /// [`CurveFailure::Defect`], a named cause or a refusal instead.
    StageFailed,
    /// The point of the tangent field's start curve at its mated parameter lies `gap` from
    /// the start of the base curve, farther than the tolerance `tolerance`.
    /// [`ErrorKind::NotWithinTolerance`].
    StartsApart {
        /// The distance between the two starts.
        gap: f64,
        /// The tolerance given.
        tolerance: f64,
    },
    /// The approximation subdivided its steps `limit` times, its cap, and a span still does
    /// not pass the tolerance. [`ErrorKind::NotWithinTolerance`].
    SubdivisionCap {
        /// The most subdivisions.
        limit: usize,
    },
    /// The deformation's target point did not drop onto the curve's surface.
    /// [`ErrorKind::NotFound`].
    TargetDidNotDrop,
    /// An at-distance solve was called without its target distance. Status
    /// `SM_ERR_NULL_POINTER`. The caller's options: [`ErrorKind::InvalidInput`].
    TargetDistanceMissing,
    /// The approximation was given `given` breakpoints, fewer than the two that bound the
    /// curve's interval. [`ErrorKind::InvalidInput`].
    TooFewBreakpoints {
        /// The breakpoints given.
        given: usize,
    },
    /// The curve, or the knot list given, has `knots` knots, fewer than the two a span
    /// needs: there is nothing to span, so it is degenerate, whoever gave the list (a
    /// control polygon too short to measure is a malformed span instead,
    /// [`CurveFailure::ControlPolygonTooShort`]). [`ErrorKind::Degenerate`].
    TooFewKnots {
        /// The knots there are.
        knots: usize,
    },
    /// The routine evaluates at most `most` derivatives, and the caller asked for
    /// `asked`. The caller's argument: [`ErrorKind::InvalidInput`].
    TooManyDerivatives {
        /// The derivatives asked for.
        asked: usize,
        /// The most the routine evaluates.
        most: usize,
    },
    /// The curve type read, `curve_type`, is not one the reader knows: the file's data.
    /// [`ErrorKind::InvalidInput`].
    UnknownCurveType {
        /// The type read.
        curve_type: i64,
    },
    /// A direction a curve routine needs is the zero vector, as the phrase says. No site
    /// builds it any more (the projected curve's was the last); it goes with the other phrase
    /// forms once every count is zero.
    ZeroDirection(&'static str),
    /// The uniform scale asked for, `scale`, is within the kernel's zero: a curve scaled to
    /// nothing has no shape. [`ErrorKind::InvalidInput`].
    ZeroScale {
        /// The scale asked for.
        scale: f64,
    },
    /// The curve's tangent at `t` is zero (within the kernel's zero there) where the curve
    /// is not degenerate: a singularity, where the closest-point, property or solve
    /// function has no direction to step.
    ZeroTangent {
        /// The curve parameter.
        t: f64,
    },
}

/// What [`CurveFailure::JoinMismatch`] found different.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum JoinCheck {
    /// The centre.
    Centre,
    /// The line through both: the join point's distance from it.
    Collinear,
    /// The points where the two are joined.
    JoinPoint,
    /// The normal, up to sign.
    Normal,
    /// The radius along the X axis.
    RadiusX,
    /// The radius along the Y axis.
    RadiusY,
    /// The X axis, up to sign.
    XAxis,
    /// The Y axis, up to sign.
    YAxis,
}

impl core::fmt::Display for JoinCheck {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Centre => "the centre",
            Self::Collinear => "the line through both",
            Self::JoinPoint => "the join point",
            Self::Normal => "the normal",
            Self::RadiusX => "the X radius",
            Self::RadiusY => "the Y radius",
            Self::XAxis => "the X axis",
            Self::YAxis => "the Y axis",
        })
    }
}

/// What the fast point drop ([`CurveFailure::FastDropUnavailable`]) lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FastDropBlock {
    /// The curve is neither a line nor an ellipse.
    CurveKind,
    /// The ellipse's polar conversion is not set up.
    NoPolarConversion,
    /// The ellipse is not a circle.
    NotACircle,
    /// The solve is neither a minimisation nor an intersection.
    Operation,
}

impl core::fmt::Display for FastDropBlock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::CurveKind => "the curve kind has no fast point drop",
            Self::NoPolarConversion => "the ellipse's polar conversion is not set up",
            Self::NotACircle => "the ellipse is not a circle",
            Self::Operation => "the solve is neither a minimisation nor an intersection",
        })
    }
}

/// An edit of a curve that [`CurveFailure::BaseCurveNotOwned`] refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurveEdit {
    /// A change or reversal of its parameterisation.
    Reparametrise,
    /// A transform.
    Transform,
    /// A trim.
    Trim,
}

impl core::fmt::Display for CurveEdit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Reparametrise => "reparametrised",
            Self::Transform => "transformed",
            Self::Trim => "trimmed",
        })
    }
}

/// A required option of [`crate::curve::curve::p3::PointSolveOptions`] that a ray solve
/// was called without ([`CurveFailure::PointSolveOptionMissing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointSolveOption {
    /// `distance_tolerance`, the largest ray / curve gap.
    DistanceTolerance,
    /// `vectors`, whose first entry is the ray's direction.
    Vectors,
}

impl core::fmt::Display for PointSolveOption {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::DistanceTolerance => "distance tolerance",
            Self::Vectors => "vectors",
        })
    }
}

/// A method of the curve interface that [`CurveFailure::BaseCurveMethod`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BaseCurveMethod {
    /// `CalculateContinuities`.
    CalculateContinuities,
    /// `ConvertTo2D`.
    ConvertTo2d,
    /// `Copy`.
    Copy,
    /// `CreateMirrorCurve`.
    CreateMirrorCurve,
    /// `EditParameterization`.
    EditParameterization,
    /// `ReverseParameterization`.
    ReverseParameterization,
    /// `StepInversion`.
    StepInversion,
    /// `Transform`.
    Transform,
    /// `Trim`.
    Trim,
}

impl core::fmt::Display for BaseCurveMethod {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::CalculateContinuities => "the continuity calculation",
            Self::ConvertTo2d => "the conversion to 2D",
            Self::Copy => "the copy",
            Self::CreateMirrorCurve => "the mirror curve",
            Self::EditParameterization => "the parameterisation edit",
            Self::ReverseParameterization => "the parameterisation reversal",
            Self::StepInversion => "the step inversion",
            Self::Transform => "the transform",
            Self::Trim => "the trim",
        })
    }
}

/// An option of a curve property or solve function that
/// [`CurveFailure::PropertyOptionMissing`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PropertyOption {
    /// The property value.
    Value,
    /// The option vectors (all of them, or the one the property reads).
    Vectors,
}

impl core::fmt::Display for PropertyOption {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Value => "property value",
            Self::Vectors => "option vectors",
        })
    }
}

/// Why a curve's property or solve function is undefined at a parameter
/// ([`CurveFailure::FunctionUndefined`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FunctionGap {
    /// The curve's first and second derivatives are parallel: it has no curvature there.
    NoCurvature,
    /// The vector between the two curves, projected, is shorter than the tolerance.
    ProjectedGapShort,
    /// The tangent's component along the axis and its derivative are both zero.
    StationaryComponent,
    /// The tangent is perpendicular to the view direction (the vector, or the line to the
    /// point), so the curve has no silhouette there.
    TangentAcrossView,
    /// The curve's second derivative is zero.
    ZeroSecondDerivative,
}

impl core::fmt::Display for FunctionGap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::NoCurvature => "the curve has no curvature",
            Self::ProjectedGapShort => {
                "the projected vector between the curves is shorter than the tolerance"
            }
            Self::StationaryComponent => {
                "the tangent's component along the axis and its derivative are zero"
            }
            Self::TangentAcrossView => "the tangent is perpendicular to the view direction",
            Self::ZeroSecondDerivative => "the second derivative is zero",
        })
    }
}

impl CurveFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`, except
    /// `SM_ERR_NULL_POINTER` for [`CurveFailure::Missing`] and
    /// [`CurveFailure::PointSolveOptionMissing`], and `SM_ERR_INVALID_INPUT` for
    /// [`CurveFailure::ZeroDirection`] and [`CurveFailure::AuxiliaryAlongProjection`].
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::AngleHasNoNurbParameter { .. }
            | Self::AngleOutsideArc { .. }
            | Self::AuxiliaryVectorZero
            | Self::BaseCurveMethod { .. }
            | Self::BaseCurveNotOwned { .. }
            | Self::BreaksNotCurrent
            | Self::CacheTypeNotCurve
            | Self::ContinuityUndefined { .. }
            | Self::ControlPolygonTooShort { .. }
            | Self::CornerByCurveExtension
            | Self::CornerOnViewAxis
            | Self::Defect { .. }
            | Self::EllipseWithoutNurb
            | Self::EndKnotsNotClamped
            | Self::EndsApart { .. }
            | Self::Failed(_)
            | Self::FastDropUnavailable { .. }
            | Self::FunctionUndefined { .. }
            | Self::InfiniteTangent { .. }
            | Self::IntervalNotSplittable { .. }
            | Self::IntervalOutsideBase
            | Self::JoinMismatch { .. }
            | Self::JoinOtherKind
            | Self::JoinOtherNotStraight { .. }
            | Self::KnotPastBreaks { .. }
            | Self::LengthBelowFloor { .. }
            | Self::LengthPastPieceCap { .. }
            | Self::LineDistanceNotAboveTolerance { .. }
            | Self::LocalSolveFailed
            | Self::MidTangentOffChord { .. }
            | Self::NoEvaluationOfItsOwn
            | Self::NoEvaluationSlots
            | Self::NoOffsetCurve
            | Self::NoOffsetDirection { .. }
            | Self::NoPropertyFunction
            | Self::NoSpans
            | Self::NonUniformScale { .. }
            | Self::NurbNotTransformed
            | Self::OffsetDirectionUnknown { .. }
            | Self::OffsetRadiusNegative { .. }
            | Self::ParameterGuessCap { .. }
            | Self::PieceNotContinuous { .. }
            | Self::PieceOutsideSpan { .. }
            | Self::PlacementNotSet
            | Self::PointOnCurveWhenMaximising { .. }
            | Self::ProjectionVectorZero
            | Self::PropertyOptionMissing { .. }
            | Self::RadiusNotPositive { .. }
            | Self::RangeSolutionVariables { .. }
            | Self::RootNotEvaluated { .. }
            | Self::SamePointToleranceNotPositive { .. }
            | Self::SecondCurveNotOnSurface
            | Self::SingleLinearCurve
            | Self::SpanNotBSpline
            | Self::SplitAtEnd { .. }
            | Self::SplitOutsideInterval { .. }
            | Self::StackTooShort { .. }
            | Self::StageFailed
            | Self::StartsApart { .. }
            | Self::SubdivisionCap { .. }
            | Self::TargetDidNotDrop
            | Self::TooFewBreakpoints { .. }
            | Self::TooFewKnots { .. }
            | Self::TooManyDerivatives { .. }
            | Self::UnknownCurveType { .. }
            | Self::ZeroScale { .. }
            | Self::ZeroTangent { .. } => SM_ERR,
            Self::CacheNotBuilt
            | Self::CurveNodeWithoutChild
            | Self::CurveNodeWithoutData
            | Self::Missing(_)
            | Self::OffsetWithoutBase
            | Self::PointSolveOptionMissing { .. }
            | Self::PolarConversionNotSetUp
            | Self::SolutionWithoutCurve { .. }
            | Self::TargetDistanceMissing => SM_ERR_NULL_POINTER,
            Self::ZeroDirection(_) | Self::AuxiliaryAlongProjection => SM_ERR_INVALID_INPUT,
        }
    }

    /// Its stable [`ErrorKind`]: [`ErrorKind::NotWithinTolerance`] for a length bracket's
    /// refusal (the accuracy asked is finer than the computation resolves); for a routine's
    /// failure, the kind of the status it replaced ([`ErrorKind::Failed`],
    /// [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`]) and that of the status of a typed
    /// cause that replaced one; [`ErrorKind::StageFailed`] for a stage that failed, reviewed
    /// or not, a defect;
    /// [`ErrorKind::PortInvariant`] for a piece outside its span, a port invariant broken;
    /// [`ErrorKind::InvalidInput`] for a check of the caller's argument or curve,
    /// [`ErrorKind::Unsupported`] for what the routine does not do,
    /// [`ErrorKind::Degenerate`] for a curve, arc or offset with no extent to work on,
    /// [`ErrorKind::NotWithinTolerance`] for curves whose ends lie apart, and
    /// [`ErrorKind::Failed`] for a step whose error is the cause or a conversion with no
    /// root.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::AngleHasNoNurbParameter { .. }
            | Self::CacheNotBuilt
            | Self::CornerOnViewAxis
            | Self::FunctionUndefined { .. }
            | Self::LocalSolveFailed
            | Self::NurbNotTransformed
            | Self::PlacementNotSet
            | Self::RootNotEvaluated { .. } => ErrorKind::Failed,
            Self::AngleOutsideArc { .. }
            | Self::AuxiliaryAlongProjection
            | Self::AuxiliaryVectorZero
            | Self::BaseCurveNotOwned { .. }
            | Self::BreaksNotCurrent
            | Self::CacheTypeNotCurve
            | Self::ContinuityUndefined { .. }
            | Self::ControlPolygonTooShort { .. }
            | Self::CurveNodeWithoutChild
            | Self::CurveNodeWithoutData
            | Self::IntervalOutsideBase
            | Self::KnotPastBreaks { .. }
            | Self::LineDistanceNotAboveTolerance { .. }
            | Self::NoEvaluationSlots
            | Self::OffsetDirectionUnknown { .. }
            | Self::OffsetRadiusNegative { .. }
            | Self::OffsetWithoutBase
            | Self::PieceNotContinuous { .. }
            | Self::PointSolveOptionMissing { .. }
            | Self::PolarConversionNotSetUp
            | Self::ProjectionVectorZero
            | Self::PropertyOptionMissing { .. }
            | Self::RadiusNotPositive { .. }
            | Self::RangeSolutionVariables { .. }
            | Self::SamePointToleranceNotPositive { .. }
            | Self::SecondCurveNotOnSurface
            | Self::SolutionWithoutCurve { .. }
            | Self::SpanNotBSpline
            | Self::SplitAtEnd { .. }
            | Self::SplitOutsideInterval { .. }
            | Self::StackTooShort { .. }
            | Self::TargetDistanceMissing
            | Self::TooFewBreakpoints { .. }
            | Self::TooManyDerivatives { .. }
            | Self::UnknownCurveType { .. }
            | Self::ZeroScale { .. } => ErrorKind::InvalidInput,
            Self::BaseCurveMethod { .. }
            | Self::CornerByCurveExtension
            | Self::EndKnotsNotClamped
            | Self::FastDropUnavailable { .. }
            | Self::JoinOtherKind
            | Self::NoEvaluationOfItsOwn
            | Self::NoPropertyFunction
            | Self::NonUniformScale { .. }
            | Self::ParameterGuessCap { .. } => ErrorKind::Unsupported,
            Self::EllipseWithoutNurb
            | Self::MidTangentOffChord { .. }
            | Self::NoOffsetCurve
            | Self::NoOffsetDirection { .. }
            | Self::NoSpans
            | Self::TooFewKnots { .. } => ErrorKind::Degenerate,
            Self::EndsApart { .. }
            | Self::IntervalNotSplittable { .. }
            | Self::JoinMismatch { .. }
            | Self::JoinOtherNotStraight { .. }
            | Self::LengthBelowFloor { .. }
            | Self::LengthPastPieceCap { .. }
            | Self::StartsApart { .. }
            | Self::SubdivisionCap { .. } => ErrorKind::NotWithinTolerance,
            Self::Failed(_)
            | Self::InfiniteTangent { .. }
            | Self::Missing(_)
            | Self::PointOnCurveWhenMaximising { .. }
            | Self::SingleLinearCurve
            | Self::ZeroDirection(_)
            | Self::ZeroTangent { .. } => ErrorKind::of_status(self.comparison_status()),
            Self::PieceOutsideSpan { .. } => ErrorKind::PortInvariant,
            Self::StageFailed | Self::Defect { .. } => ErrorKind::StageFailed,
            Self::TargetDidNotDrop => ErrorKind::NotFound,
        }
    }

    /// True for a failure that replaced a C++ status the routine returns (every cause but
    /// the length bracket's refusals and a piece outside its span):
    /// it answers with that status the way the `Upstream` it replaced did
    /// ([`crate::error::upstream_status`]). A length bracket's refusal, and a piece outside its
    /// span, are solidcore's own and replaced none.
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        match self {
            Self::AngleHasNoNurbParameter { .. }
            | Self::AngleOutsideArc { .. }
            | Self::AuxiliaryAlongProjection
            | Self::AuxiliaryVectorZero
            | Self::BaseCurveMethod { .. }
            | Self::BaseCurveNotOwned { .. }
            | Self::BreaksNotCurrent
            | Self::CacheNotBuilt
            | Self::CacheTypeNotCurve
            | Self::ContinuityUndefined { .. }
            | Self::ControlPolygonTooShort { .. }
            | Self::CornerByCurveExtension
            | Self::CornerOnViewAxis
            | Self::CurveNodeWithoutChild
            | Self::CurveNodeWithoutData
            | Self::Defect { .. }
            | Self::EllipseWithoutNurb
            | Self::EndKnotsNotClamped
            | Self::EndsApart { .. }
            | Self::Failed(_)
            | Self::FastDropUnavailable { .. }
            | Self::FunctionUndefined { .. }
            | Self::InfiniteTangent { .. }
            | Self::IntervalNotSplittable { .. }
            | Self::IntervalOutsideBase
            | Self::JoinMismatch { .. }
            | Self::JoinOtherKind
            | Self::JoinOtherNotStraight { .. }
            | Self::KnotPastBreaks { .. }
            | Self::LineDistanceNotAboveTolerance { .. }
            | Self::LocalSolveFailed
            | Self::MidTangentOffChord { .. }
            | Self::Missing(_)
            | Self::NoEvaluationOfItsOwn
            | Self::NoEvaluationSlots
            | Self::NoOffsetCurve
            | Self::NoOffsetDirection { .. }
            | Self::NoPropertyFunction
            | Self::NoSpans
            | Self::NonUniformScale { .. }
            | Self::NurbNotTransformed
            | Self::OffsetDirectionUnknown { .. }
            | Self::OffsetRadiusNegative { .. }
            | Self::OffsetWithoutBase
            | Self::ParameterGuessCap { .. }
            | Self::PieceNotContinuous { .. }
            | Self::PlacementNotSet
            | Self::PointOnCurveWhenMaximising { .. }
            | Self::PointSolveOptionMissing { .. }
            | Self::PolarConversionNotSetUp
            | Self::ProjectionVectorZero
            | Self::PropertyOptionMissing { .. }
            | Self::RadiusNotPositive { .. }
            | Self::RangeSolutionVariables { .. }
            | Self::RootNotEvaluated { .. }
            | Self::SamePointToleranceNotPositive { .. }
            | Self::SecondCurveNotOnSurface
            | Self::SingleLinearCurve
            | Self::SolutionWithoutCurve { .. }
            | Self::SpanNotBSpline
            | Self::SplitAtEnd { .. }
            | Self::SplitOutsideInterval { .. }
            | Self::StackTooShort { .. }
            | Self::StageFailed
            | Self::StartsApart { .. }
            | Self::SubdivisionCap { .. }
            | Self::TargetDidNotDrop
            | Self::TargetDistanceMissing
            | Self::TooFewBreakpoints { .. }
            | Self::TooFewKnots { .. }
            | Self::TooManyDerivatives { .. }
            | Self::UnknownCurveType { .. }
            | Self::ZeroDirection(_)
            | Self::ZeroScale { .. }
            | Self::ZeroTangent { .. } => true,
            Self::LengthBelowFloor { .. }
            | Self::LengthPastPieceCap { .. }
            | Self::PieceOutsideSpan { .. } => false,
        }
    }
}

impl core::fmt::Display for CurveFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::LengthBelowFloor {
                asked,
                floor,
                pieces,
            } => write!(
                f,
                "a length bracket {asked:e} wide was asked, below the {floor:e} its rounding over {pieces} pieces allows"
            ),
            Self::LengthPastPieceCap {
                asked,
                reached,
                pieces,
            } => write!(
                f,
                "a length bracket {asked:e} wide was asked; at its cap of {pieces} pieces it is {reached:e} wide"
            ),
            Self::PieceOutsideSpan {
                a,
                b,
                span_start,
                span_end,
            } => write!(
                f,
                "a piece [{a:e}, {b:e}] of a length bracket lies outside its knot span [{span_start:e}, {span_end:e}]"
            ),
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::ZeroDirection(what) => write!(f, "{what} is the zero vector"),
            Self::SingleLinearCurve => f.write_str("the only curve is a line, which spans no plane"),
            Self::InfiniteTangent { t } => write!(f, "the curve's tangent at t = {t} is infinite"),
            Self::ZeroTangent { t } => write!(
                f,
                "the curve's tangent at t = {t} is zero where the curve is not degenerate"
            ),
            Self::PointOnCurveWhenMaximising { t } => write!(
                f,
                "the point lies on the curve at t = {t}, where a farthest-point solve finds no extremum"
            ),
            Self::StageFailed => write!(f, "{}", ErrorKind::StageFailed),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::PointSolveOptionMissing { option } => {
                write!(f, "the ray solve was called without its {option}")
            }
            Self::TooManyDerivatives { asked, most } => write!(
                f,
                "{asked} derivatives were asked, more than the {most} the routine evaluates"
            ),
            Self::NonUniformScale { scale } => write!(
                f,
                "the scale ({}, {}, {}) is not uniform, and an analytic curve scales only uniformly",
                scale.x, scale.y, scale.z
            ),
            Self::BaseCurveNotOwned { edit } => write!(
                f,
                "the curve does not own its base curve, so it cannot be {edit}"
            ),
            Self::NoEvaluationOfItsOwn => f.write_str(
                "a composite curve is evaluated through its segments and has no evaluation of its own",
            ),
            Self::CornerByCurveExtension => f.write_str(
                "an offset corner by curve extension is not built here, only line, fillet and chamfer corners",
            ),
            Self::SamePointToleranceNotPositive { tolerance } => {
                write!(f, "the same-point tolerance {tolerance:e} is not positive")
            }
            Self::LineDistanceNotAboveTolerance {
                distance,
                tolerance,
            } => write!(
                f,
                "the distance to create a line {distance:e} is neither zero nor above the same-point tolerance {tolerance:e}"
            ),
            Self::MidTangentOffChord { at } => write!(
                f,
                "the hyperbola's arc is numerically degenerate for its rational form: its middle tangent line cuts the chord at {at} of its length, at an end"
            ),
            Self::BreaksNotCurrent => {
                f.write_str("the curve's cached breaks and continuities are not current")
            }
            Self::KnotPastBreaks { knot } => {
                write!(f, "the knot {knot} lies past the curve's last cached break")
            }
            Self::ContinuityUndefined { at } => {
                write!(f, "the cached continuity at the break {at} is undefined")
            }
            Self::ProjectionVectorZero => {
                f.write_str("the projected curve's projection vector is unset or zero")
            }
            Self::AuxiliaryVectorZero => {
                f.write_str("the rotation's auxiliary vector is unset or zero")
            }
            Self::AuxiliaryAlongProjection => f.write_str(
                "the rotation's auxiliary vector is parallel to the projection vector",
            ),
            Self::StartsApart { gap, tolerance } => write!(
                f,
                "the start curve's mated point lies {gap:e} from the base curve's start, more than the tolerance {tolerance:e}"
            ),
            Self::EndsApart { gap, tolerance } => write!(
                f,
                "the end curve's mated point lies {gap:e} from the base curve's end, more than the tolerance {tolerance:e}"
            ),
            Self::OffsetDirectionUnknown { direction } => {
                write!(f, "the offset direction {direction} is not 1, 2 or 3")
            }
            Self::NoOffsetCurve => f.write_str(
                "no curve gave an offset: there were no segments, or every offset was degenerate",
            ),
            Self::NoSpans => f.write_str("the curve has no span of non-zero length"),
            Self::TooFewKnots { knots } => {
                write!(f, "there are {knots} knots, fewer than the two a span needs")
            }
            Self::SpanNotBSpline => f.write_str("the span is not a B-spline curve"),
            Self::ControlPolygonTooShort { points } => write!(
                f,
                "the span's control polygon has {points} points, fewer than two"
            ),
            Self::AngleOutsideArc {
                angle_deg,
                start_deg,
                end_deg,
            } => write!(
                f,
                "the angle {angle_deg} degrees is outside the arc from {start_deg} to {end_deg} degrees"
            ),
            Self::AngleHasNoNurbParameter { angle_deg } => write!(
                f,
                "the span's quadratic has no root in [0, 1] for the angle {angle_deg} degrees"
            ),
            Self::FastDropUnavailable { reason } => {
                write!(f, "the fast point drop does not apply: {reason}")
            }
            Self::ZeroScale { scale } => write!(f, "the scale {scale:e} is within zero"),
            Self::PlacementNotSet => f.write_str("the ellipse's scaled placement could not be set"),
            Self::NurbNotTransformed => f.write_str("the ellipse's NURB could not be transformed"),
            Self::EllipseWithoutNurb => {
                f.write_str("the ellipse has no NURB")
            }
            Self::EndKnotsNotClamped => {
                f.write_str("the ellipse's NURB does not end in knots of full multiplicity")
            }
            Self::PolarConversionNotSetUp => {
                f.write_str("the polar conversion was not set up from an ellipse")
            }
            Self::RadiusNotPositive { radius } => {
                write!(f, "the radius {radius:e} is not above zero")
            }
            Self::OffsetRadiusNegative {
                offset_radius,
                tolerance,
            } => write!(
                f,
                "the offset circle's radius would be {offset_radius:e}, below zero by more than the tolerance {tolerance:e}"
            ),
            Self::JoinOtherKind => {
                f.write_str("the curve to join on is not of this curve's kind")
            }
            Self::JoinMismatch {
                check,
                off,
                tolerance,
            } => write!(
                f,
                "the curve to join on differs in {check} by {off:e}, more than the tolerance {tolerance:e}"
            ),
            Self::JoinOtherNotStraight { tolerance } => write!(
                f,
                "the curve to join on is not straight within the tolerance {tolerance:e}"
            ),
            Self::OffsetWithoutBase => f.write_str("the offset curve has no base curve"),
            Self::NoOffsetDirection { t } => write!(
                f,
                "the base curve's tangent at t = {t} is zero or along the offset normal, so the offset has no direction"
            ),
            Self::IntervalOutsideBase => {
                f.write_str("the offset curve's interval is not within its base curve's")
            }
            Self::RangeSolutionVariables { variables } => {
                write!(f, "a range solution has {variables} variables, not two")
            }
            Self::SolutionWithoutCurve { object } => {
                write!(f, "object {object} of a solution is no known curve")
            }
            Self::TooFewBreakpoints { given } => write!(
                f,
                "{given} breakpoints were given, fewer than the two that bound the curve's interval"
            ),
            Self::SubdivisionCap { limit } => write!(
                f,
                "the approximation reached its cap of {limit} subdivisions with a span still outside the tolerance"
            ),
            Self::IntervalNotSplittable { lo, hi } => write!(
                f,
                "the step [{lo}, {hi}] cannot be split further, and its span is still outside the tolerance"
            ),
            Self::CurveNodeWithoutData => {
                f.write_str("a curve tree node of the solver's branch has no span data")
            }
            Self::CurveNodeWithoutChild => {
                f.write_str("a curve tree node the solver subdivides has no child")
            }
            Self::CacheNotBuilt => f.write_str("the curve cache could not be built"),
            Self::CacheTypeNotCurve => f.write_str("the cache asked for is not a curve cache"),
            Self::TargetDistanceMissing => {
                f.write_str("the at-distance solve was called without its target distance")
            }
            Self::PropertyOptionMissing { option } => {
                write!(f, "the property function was called without its {option}")
            }
            Self::SecondCurveNotOnSurface => f.write_str(
                "the surface-projected intersection's second curve is not a curve on a surface",
            ),
            Self::NoEvaluationSlots => f.write_str("there is no slot to evaluate into"),
            Self::SplitOutsideInterval { split, start, end } => write!(
                f,
                "the split parameter {split} is outside the curve's interval [{start}, {end}]"
            ),
            Self::SplitAtEnd { split, end } => {
                write!(f, "the split parameter {split} is at the curve's end {end}")
            }
            Self::UnknownCurveType { curve_type } => {
                write!(f, "the curve type {curve_type} is unknown")
            }
            Self::StackTooShort { values } => write!(
                f,
                "the stack holds {values} values, fewer than the 7 of an entry"
            ),
            Self::PieceNotContinuous {
                knot_gap,
                point_gap,
            } => write!(
                f,
                "the added piece starts {knot_gap:e} in parameter and {point_gap:e} in space from where the curve ends"
            ),
            Self::BaseCurveMethod { method } => {
                write!(f, "the curve kind does not implement {method}")
            }
            Self::ParameterGuessCap { limit } => {
                write!(f, "the solve filled all {limit} of its parameter guess slots")
            }
            Self::LocalSolveFailed => f.write_str("the local solve failed"),
            Self::RootNotEvaluated { t } => write!(
                f,
                "the property function could not be evaluated at its root t = {t}"
            ),
            Self::FunctionUndefined { t, why } => {
                write!(f, "the solve function is undefined at t = {t}: {why}")
            }
            Self::NoPropertyFunction => {
                f.write_str("the property function has no equation for the property")
            }
            Self::TargetDidNotDrop => {
                f.write_str("the target point did not drop onto the curve's surface")
            }
            Self::CornerOnViewAxis => {
                f.write_str("a pseudo box corner projects onto the view origin")
            }
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a curve routine that fails with `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn curve(operation: &'static str, failure: CurveFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// The curve failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Curve`].
    #[must_use]
    pub fn curve_failure(&self) -> Option<&CurveFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Curve(c) => Some(c),
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

    /// A curve stage failure with `status`, as a stage site builds one.
    fn stage_failed(operation: &'static str, status: i32) -> SolidError {
        Failure::new(operation, CurveFailure::StageFailed)
            .with_status(status)
            .into_error()
    }

    #[test]
    fn curve_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (CurveFailure::Failed("x"), SM_ERR),
            (CurveFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (CurveFailure::ZeroDirection("x"), SM_ERR_INVALID_INPUT),
        ];
        for (failure, code) in cases {
            let typed = SolidError::curve("curve::op", failure);
            let before = SolidError::Upstream {
                operation: "curve::op",
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
            assert!(failure.replaces_upstream_status());
            assert_eq!(typed.curve_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::curve("curve::op", CurveFailure::Missing("the curve's cache")).to_string(),
            "curve::op: the curve's cache is missing"
        );
        assert_eq!(
            SolidError::curve(
                "curve::op",
                CurveFailure::Failed("the scale is not uniform")
            )
            .to_string(),
            "curve::op: the scale is not uniform"
        );
        assert_eq!(
            SolidError::curve("curve::op", CurveFailure::ZeroDirection("the direction"))
                .to_string(),
            "curve::op: the direction is the zero vector"
        );
        assert_eq!(SolidError::sm_err("curve::op").curve_failure(), None);
    }

    /// A stage that failed is a defect that keeps its site's status; a typed cause keeps
    /// the status and kind of the bare status it replaced.
    #[test]
    fn a_stage_failure_is_a_defect_and_a_typed_cause_keeps_its_status() {
        super::super::tests::assert_stage_failure(
            &stage_failed("curve::op", SM_ERR_NULL_POINTER),
            "curve::op",
            SM_ERR_NULL_POINTER,
        );
        super::super::tests::assert_stage_failure(
            &stage_failed("curve::op", SM_ERR),
            "curve::op",
            SM_ERR,
        );
        for cause in [
            CurveFailure::SingleLinearCurve,
            CurveFailure::InfiniteTangent { t: 0.25 },
            CurveFailure::ZeroTangent { t: 0.5 },
            CurveFailure::PointOnCurveWhenMaximising { t: 0.75 },
        ] {
            let e = SolidError::curve("curve::op", cause);
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{cause:?}");
            assert_eq!(e.kind(), ErrorKind::Failed, "{cause:?}");
            assert!(!e.is_defect(), "{cause:?}");
            assert!(e.is_status_answer(), "{cause:?}");
        }
        assert_eq!(
            SolidError::curve("curve::op", CurveFailure::InfiniteTangent { t: 0.25 }).to_string(),
            "curve::op: the curve's tangent at t = 0.25 is infinite"
        );
    }

    /// A reviewed defect is of class 127 with the stage failure's status, and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the cache rebuilt a moment ago has no span at a parameter inside the curve";
        let defect = CurveFailure::Defect { why };
        super::super::tests::assert_defect(
            &SolidError::curve("curve::op", defect),
            &stage_failed("curve::op", SM_ERR),
            "curve::op",
            SM_ERR,
            &format!("curve::op: stage_failed: {why}"),
        );
        let missing = Failure::new("curve::op", defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &stage_failed("curve::op", SM_ERR_NULL_POINTER),
            "curve::op",
            SM_ERR_NULL_POINTER,
            &format!("curve::op: stage_failed: {why}"),
        );
        assert_eq!(missing.curve_failure(), Some(&defect));
        assert!(defect.replaces_upstream_status());
    }

    /// A length bracket's refusals report what they reported before the curve routines'
    /// failures joined them: status `SM_ERR`, kind `not_within_tolerance`, no upstream status
    /// (they replaced none), and the same text.
    #[test]
    fn a_length_bracket_refusal_reports_as_before() {
        let refusals = [
            (
                CurveFailure::LengthBelowFloor {
                    asked: 1e-12,
                    floor: 1e-10,
                    pieces: 8,
                },
                "curve::length_bound: a length bracket 1e-12 wide was asked, below the 1e-10 its rounding over 8 pieces allows",
            ),
            (
                CurveFailure::LengthPastPieceCap {
                    asked: 1e-9,
                    reached: 1e-6,
                    pieces: 4096,
                },
                "curve::length_bound: a length bracket 1e-9 wide was asked; at its cap of 4096 pieces it is 1e-6 wide",
            ),
        ];
        for (refusal, text) in refusals {
            let e = Failure::new("curve::length_bound", refusal).into_error();
            assert_eq!(e.sm_status(), Some(SM_ERR));
            assert_eq!(e.kind(), ErrorKind::NotWithinTolerance);
            assert_eq!(e.upstream_status(), None);
            assert_eq!(super::super::upstream_status(&e), None);
            assert_eq!(e.operation(), "curve::length_bound");
            assert_eq!(e.entity(), None);
            assert!(!refusal.replaces_upstream_status());
            assert!(e.is_status_answer());
            assert!(!e.is_port_failure());
            assert_eq!(e.to_string(), text);
            assert_eq!(e.curve_failure(), Some(&refusal));
        }
    }

    /// The named causes keep the status of the phrase they replace, and `upstream_status`
    /// reads it, with the kind of what they check.
    #[test]
    fn the_named_causes_keep_their_sites_statuses() {
        use crate::geom::vector3d::Vec3;
        let cases = [
            (
                CurveFailure::TooManyDerivatives { asked: 4, most: 3 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::NonUniformScale {
                    scale: Vec3::new(1.0, 2.0, 1.0),
                },
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::BaseCurveNotOwned {
                    edit: CurveEdit::Trim,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::NoEvaluationOfItsOwn,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::CornerByCurveExtension,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::SamePointToleranceNotPositive { tolerance: 0.0 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::LineDistanceNotAboveTolerance {
                    distance: 0.5,
                    tolerance: 1.0,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::MidTangentOffChord { at: 0.0 },
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                CurveFailure::BreaksNotCurrent,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::KnotPastBreaks { knot: 2.0 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::ContinuityUndefined { at: 0.5 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::ProjectionVectorZero,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::AuxiliaryVectorZero,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::AuxiliaryAlongProjection,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::StartsApart {
                    gap: 1.0,
                    tolerance: 0.1,
                },
                SM_ERR,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CurveFailure::EndsApart {
                    gap: 1.0,
                    tolerance: 0.1,
                },
                SM_ERR,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CurveFailure::OffsetDirectionUnknown { direction: 4 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (CurveFailure::NoOffsetCurve, SM_ERR, ErrorKind::Degenerate),
            (CurveFailure::NoSpans, SM_ERR, ErrorKind::Degenerate),
            (
                CurveFailure::TooFewKnots { knots: 1 },
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                CurveFailure::SpanNotBSpline,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::ControlPolygonTooShort { points: 1 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::AngleOutsideArc {
                    angle_deg: 100.0,
                    start_deg: 0.0,
                    end_deg: 90.0,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::AngleHasNoNurbParameter { angle_deg: 45.0 },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                CurveFailure::FastDropUnavailable {
                    reason: FastDropBlock::NotACircle,
                },
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::ZeroScale { scale: 0.0 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (CurveFailure::PlacementNotSet, SM_ERR, ErrorKind::Failed),
            (CurveFailure::NurbNotTransformed, SM_ERR, ErrorKind::Failed),
            (
                CurveFailure::EllipseWithoutNurb,
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                CurveFailure::EndKnotsNotClamped,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::PolarConversionNotSetUp,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::RadiusNotPositive { radius: 0.0 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::OffsetRadiusNegative {
                    offset_radius: -0.5,
                    tolerance: 1e-4,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (CurveFailure::JoinOtherKind, SM_ERR, ErrorKind::Unsupported),
            (
                CurveFailure::JoinMismatch {
                    check: JoinCheck::Centre,
                    off: 1.0,
                    tolerance: 1e-6,
                },
                SM_ERR,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CurveFailure::JoinOtherNotStraight { tolerance: 1e-6 },
                SM_ERR,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CurveFailure::OffsetWithoutBase,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::NoOffsetDirection { t: 0.5 },
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                CurveFailure::IntervalOutsideBase,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::RangeSolutionVariables { variables: 3 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::SolutionWithoutCurve { object: 1 },
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::TooFewBreakpoints { given: 1 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::SubdivisionCap { limit: 8192 },
                SM_ERR,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CurveFailure::IntervalNotSplittable { lo: 0.5, hi: 0.5 },
                SM_ERR,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CurveFailure::CurveNodeWithoutData,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::CurveNodeWithoutChild,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::CacheNotBuilt,
                SM_ERR_NULL_POINTER,
                ErrorKind::Failed,
            ),
            (
                CurveFailure::CacheTypeNotCurve,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::TargetDistanceMissing,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::PropertyOptionMissing {
                    option: PropertyOption::Vectors,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::SecondCurveNotOnSurface,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::NoEvaluationSlots,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::SplitOutsideInterval {
                    split: 2.0,
                    start: 0.0,
                    end: 1.0,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::SplitAtEnd {
                    split: 1.0,
                    end: 1.0,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::UnknownCurveType { curve_type: 99 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::StackTooShort { values: 6 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::PieceNotContinuous {
                    knot_gap: 0.5,
                    point_gap: 0.0,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::BaseCurveMethod {
                    method: BaseCurveMethod::Trim,
                },
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::ParameterGuessCap { limit: 1000 },
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (CurveFailure::LocalSolveFailed, SM_ERR, ErrorKind::Failed),
            (
                CurveFailure::RootNotEvaluated { t: 0.5 },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                CurveFailure::FunctionUndefined {
                    t: 0.5,
                    why: FunctionGap::NoCurvature,
                },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                CurveFailure::NoPropertyFunction,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (CurveFailure::TargetDidNotDrop, SM_ERR, ErrorKind::NotFound),
            (CurveFailure::CornerOnViewAxis, SM_ERR, ErrorKind::Failed),
        ];
        for (cause, status, kind) in cases {
            let e = SolidError::curve("curve::op", cause);
            assert_eq!(e.sm_status(), Some(status), "{cause:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status), "{cause:?}");
            assert_eq!(e.kind(), kind, "{cause:?}");
            assert!(cause.replaces_upstream_status(), "{cause:?}");
            assert!(!e.is_defect(), "{cause:?}");
            assert!(e.is_status_answer(), "{cause:?}");
            assert_eq!(e.curve_failure(), Some(&cause));
        }
        assert_eq!(
            SolidError::curve(
                "curve::op",
                CurveFailure::BaseCurveNotOwned {
                    edit: CurveEdit::Reparametrise
                }
            )
            .to_string(),
            "curve::op: the curve does not own its base curve, so it cannot be reparametrised"
        );
        assert_eq!(
            SolidError::curve(
                "curve::op",
                CurveFailure::TooManyDerivatives { asked: 4, most: 3 }
            )
            .to_string(),
            "curve::op: 4 derivatives were asked, more than the 3 the routine evaluates"
        );
    }

    /// The curve routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work.
    #[test]
    fn the_curve_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("curve", 31, &[]);
    }

    /// The sites that build a curve phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_curve_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("CurveFailure", "Failed"),
                Builder::Variant("CurveFailure", "Missing"),
                Builder::Variant("CurveFailure", "ZeroDirection"),
                Builder::Method("curve_failed", Some(1)),
                Builder::Method("curve_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[];
        assert_pinned("curve phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
