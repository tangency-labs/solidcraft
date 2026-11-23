//! Why the fillet solver fails, with its class and status.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;
use crate::error::{ErrorKind, SM_ERR, SM_ERR_NULL_POINTER};

/// Why the fillet solver (`crate::blend::fillet_solver`) fails ([`BlendFailure::Solver`]).
/// Each names its class ([`SolverFailure::class`]); none is a defect: the solver's defects
/// are [`BlendFailure::Defect`], with their reason.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SolverFailure {
    /// The fillet is not of the analytic shape the routine builds (its cross-section,
    /// its edge's curve or its faces are of another kind): a kind the routine cannot
    /// handle ([`ErrorKind::Unsupported`]).
    AnalyticShapeDoesNotApply {
        /// The shape the routine builds.
        shape: AnalyticShape,
    },
    /// A method the base fillet solver (`SmFilletSolver` itself) does not implement was
    /// called on a solver that does not override it: a kind the routine cannot handle
    /// ([`ErrorKind::Unsupported`]).
    BaseSolverMethod(BaseSolverMethod),
    /// The fillet geometry's centre line is of a curve kind with no B-spline form: a kind
    /// the routine cannot handle ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER`.
    CenterLineNotBSpline {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
    },
    /// The clipped-radius search's iso curve at radius `radius` does not meet the other
    /// offset surface: an empty search ([`ErrorKind::NotFound`]).
    ClippedIsoMissesSurface {
        /// The radius the offsets were set to.
        radius: f64,
    },
    /// The corner point does not drop onto the side edge's curve: an empty search
    /// ([`ErrorKind::NotFound`]).
    CornerDoesNotDropOnSideCurve,
    /// The corner's cross section does not meet the extended side surface: an empty search
    /// ([`ErrorKind::NotFound`]).
    CrossSectionMissesSideSurface,
    /// The solve for the fillet's cross section on the plane through the failure's site
    /// found no section: an empty search ([`ErrorKind::NotFound`]).
    CrossSectionNotFound,
    /// The solve for the fillet's cross-section point on the plane through a rail's point
    /// at parameter `t` found no point: an empty search ([`ErrorKind::NotFound`]).
    CrossSectionPointNotFound {
        /// The parameter of the curve the plane is taken on.
        t: f64,
    },
    /// An end of the filleted edge lies on cylinder `side`'s axis, so the ball's contact
    /// there has no direction from the axis ([`ErrorKind::Degenerate`]).
    EdgeEndOnAxis {
        /// The cylinder, 0 or 1.
        side: usize,
    },
    /// An edge use of the caller's body has no face: the caller's input
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    EdgeUseWithoutFace,
    /// An edge use of the caller's body has no mate: the caller's input
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    EdgeUseWithoutMate,
    /// The extension flags need a vertex's corner and the caller gave no corners (a
    /// standalone entry point): the caller's input ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER`.
    ExtensionCornerMissing,
    /// The caller's fillet geometry has no rail on side `side`: the caller's solver set-up
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    FilletGeomWithoutRail {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
        /// The rail side, 0 or 1.
        side: usize,
    },
    /// The caller's fillet geometry names no fillet solver: the caller's solver set-up
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    FilletGeomWithoutSolver {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
    },
    /// The caller's fillet geometry has no fillet surface: the caller's solver set-up
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    FilletGeomWithoutSurface {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
    },
    /// The ball's centre has no foot on face `side`'s quadric: an empty search
    /// ([`ErrorKind::NotFound`]).
    FootNotFound {
        /// The face, 0 or 1.
        side: usize,
    },
    /// A surface was asked of the base surface generator (`SmFilletSurfaceGenerator`
    /// itself), which builds none: a kind the routine cannot handle
    /// ([`ErrorKind::Unsupported`]).
    GeneratorBuildsNoSurface,
    /// The G^n section generator was not given rail side `side`'s surface (or its uv curve,
    /// where `uv_curve`): the caller's input ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER`.
    GnSideInputMissing {
        /// The rail side, 0 or 1.
        side: usize,
        /// True for the side's uv curve, false for its surface.
        uv_curve: bool,
    },
    /// The guess point does not drop onto the side edge: an empty search
    /// ([`ErrorKind::NotFound`]).
    GuessDoesNotDropOnSideEdge,
    /// A solver's equations could not be loaded for the solve: the solver's error is the
    /// failure's cause, and decides whether it is a defect or a refusal
    /// ([`ErrorKind::Failed`]).
    JacobianNotLoaded,
    /// The nearest-point search on a curve found `count` nearest points where one is
    /// needed: an empty or ambiguous search ([`ErrorKind::NotFound`]).
    NoSingleCurveFoot {
        /// The number of nearest points found.
        count: usize,
    },
    /// Offset surface `side` has no normal at `(u, v)`: its partial derivatives and its
    /// evaluated normal both vanish there ([`ErrorKind::Degenerate`]).
    OffsetNormalVanishes {
        /// The offset surface, 0 or 1.
        side: usize,
        /// The surface parameter u.
        u: f64,
        /// The surface parameter v.
        v: f64,
    },
    /// The solver has not got both offset surfaces set: the caller's solver state
    /// ([`ErrorKind::InvalidInput`]).
    OffsetSurfaceMissing,
    /// The curve-blending self-intersection handler's previous and next pieces are
    /// `distance` apart where they come closest, farther than the `limit` it blends across
    /// (three step-back distances): not within tolerance ([`ErrorKind::NotWithinTolerance`]).
    PiecesTooFarApart {
        /// The closest distance between the pieces' rails.
        distance: f64,
        /// The largest distance blended across.
        limit: f64,
    },
    /// The point does not drop onto offset surface `side`: an empty search
    /// ([`ErrorKind::NotFound`]).
    PointDoesNotDropOnSurface {
        /// The surface, 0 or 1.
        side: usize,
    },
    /// The fillet's radius is `start` at its first section and `end` at its last: the
    /// analytic surfaces are built for a constant radius only, a kind the routine cannot
    /// handle ([`ErrorKind::Unsupported`]).
    RadiusNotConstant {
        /// The radius at the first section.
        start: f64,
        /// The radius at the last section.
        end: f64,
    },
    /// The fillet's radius `radius` is below the zero tolerance, so its section has no
    /// arc ([`ErrorKind::Degenerate`]).
    RadiusVanishes {
        /// The radius.
        radius: f64,
    },
    /// The rail does not cross the side edge from any guess: an empty search
    /// ([`ErrorKind::NotFound`]).
    RailMissesSideEdge,
    /// Rail `side` of the caller's fillet geometry has a curve not of a B-spline class (a
    /// rail edge with no curve is [`FilletSetUp::RailCurve`]): a kind the routine cannot
    /// handle ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER`.
    RailNotBSpline {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
        /// The rail side, 0 or 1.
        side: usize,
        /// The curve's kind (its type string).
        kind: &'static str,
    },
    /// A rail curve's tangent has no length at parameter `t`, so no step along it is
    /// defined there ([`ErrorKind::Degenerate`]).
    RailTangentZero {
        /// The rail curve's parameter.
        t: f64,
    },
    /// Interval `index` of the roll-over rail's classification does not end (or, the last,
    /// start) at an edge: a case the roll-over does not handle ([`ErrorKind::Unsupported`]).
    RolloverIntervalNotAtEdge {
        /// The interval's index.
        index: usize,
    },
    /// `count` short intervals of the rail's classification could not be squeezed out (no
    /// single side edge at both ends, no edge use on the rail's face, no guess on the edge,
    /// or wider than the edge's tolerance): a case the refinement does not handle
    /// ([`ErrorKind::Unsupported`]).
    ShortIntervalsNotRemoved {
        /// The number of intervals left.
        count: usize,
    },
    /// The side edge has no edge use on the rail's face: an empty search
    /// ([`ErrorKind::NotFound`]); status `SM_ERR_NULL_POINTER`.
    SideEdgeNotOnRailFace,
    /// The side surface and the plane through the side curve's end point do not meet in a
    /// curve: an empty search ([`ErrorKind::NotFound`]); status `SM_ERR_NULL_POINTER`.
    SideSurfaceMissesEndPlane,
    /// The solver was given no edge use `index` of the edge to fillet: the caller's solver
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    SolverEdgeUseMissing {
        /// The edge use's index, 0 or 1.
        index: usize,
    },
    /// The circular spine has no station in the half-plane through the point (the point is
    /// on the axis, or the offsets' meridians do not cross there): an empty search
    /// ([`ErrorKind::NotFound`]).
    SpineStationNotFound,
    /// The support edge's curve does not meet the fillet surface that ends it: an empty
    /// search ([`ErrorKind::NotFound`]).
    SupportCurveMissesSurface,
    /// The support edge's start (or end, where `at_end`) does not drop onto its curve: an
    /// empty search ([`ErrorKind::NotFound`]).
    SupportEndOffCurve {
        /// True for the end, false for the start.
        at_end: bool,
    },
    /// The intersection of the two fillet surfaces a support edge joins failed: the
    /// intersection's error is the failure's cause ([`ErrorKind::Failed`]).
    SupportIntersectionFailed,
    /// The two fillet surfaces a support edge joins do not meet in a curve: an empty search
    /// ([`ErrorKind::NotFound`]).
    SupportSurfacesDoNotMeet,
    /// The surface's cache could not be built: the cache's error is the failure's cause
    /// ([`ErrorKind::Failed`]); status `SM_ERR_NULL_POINTER`.
    SurfaceCacheNotBuilt,
    /// The fillet's section turns through `angle` radians, within the tolerance of half a
    /// turn or past it: the analytic surfaces are built for less, a kind the routine
    /// cannot handle ([`ErrorKind::Unsupported`]).
    SweepAngleNearHalfTurn {
        /// The turn, in radians.
        angle: f64,
    },
    /// The fillet's section turns through `angle` radians, too little for an arc
    /// ([`ErrorKind::Degenerate`]).
    SweepAngleTooSmall {
        /// The turn, in radians.
        angle: f64,
    },
    /// The roll-over meets a side edge that rolls over tangentially, which the full
    /// roll-over does not handle ([`ErrorKind::Unsupported`]).
    TangentRolloverUnhandled,
    /// The solver solves for more further values than a fillet point holds (`limit`): the
    /// caller's solver ([`ErrorKind::InvalidInput`]).
    TooManySolverValues {
        /// The values a fillet point holds.
        limit: usize,
    },
    /// An end of the trace lies `deviation` off the ball, farther than `tol`: not within
    /// tolerance ([`ErrorKind::NotWithinTolerance`]).
    TraceEndOffBall {
        /// The distance of the trace's end from the ball.
        deviation: f64,
        /// The tolerance.
        tol: f64,
    },
}

/// The analytic shape a fillet solver routine builds
/// ([`SolverFailure::AnalyticShapeDoesNotApply`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnalyticShape {
    /// The ball's triangle where one face's offset collapses.
    BallTriangle,
    /// A cone: a circular section along a line between faces that open.
    Cone,
    /// A cylinder: a circular section along a line.
    Cylinder,
    /// A sphere: the ball where two cylinders' axes meet.
    Sphere,
    /// A torus: a circular (or linear) section along a circular arc.
    Torus,
}

/// A method of the base fillet solver ([`SolverFailure::BaseSolverMethod`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BaseSolverMethod {
    /// `LoadInitialValues`.
    LoadInitialValues,
    /// `LoadJacobian`.
    LoadJacobian,
    /// `SetupOffsetValues`.
    SetupOffsetValues,
}

impl SolverFailure {
    /// The failure's class ([`ErrorKind`]): see each variant.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(&self) -> ErrorKind {
        match self {
            Self::AnalyticShapeDoesNotApply { .. } => ErrorKind::Unsupported,
            Self::BaseSolverMethod(_) => ErrorKind::Unsupported,
            Self::CenterLineNotBSpline { .. } => ErrorKind::Unsupported,
            Self::ClippedIsoMissesSurface { .. } => ErrorKind::NotFound,
            Self::CornerDoesNotDropOnSideCurve => ErrorKind::NotFound,
            Self::CrossSectionMissesSideSurface => ErrorKind::NotFound,
            Self::CrossSectionNotFound => ErrorKind::NotFound,
            Self::CrossSectionPointNotFound { .. } => ErrorKind::NotFound,
            Self::EdgeEndOnAxis { .. } => ErrorKind::Degenerate,
            Self::EdgeUseWithoutFace => ErrorKind::InvalidInput,
            Self::EdgeUseWithoutMate => ErrorKind::InvalidInput,
            Self::ExtensionCornerMissing => ErrorKind::InvalidInput,
            Self::FilletGeomWithoutRail { .. } => ErrorKind::InvalidInput,
            Self::FilletGeomWithoutSolver { .. } => ErrorKind::InvalidInput,
            Self::FilletGeomWithoutSurface { .. } => ErrorKind::InvalidInput,
            Self::FootNotFound { .. } => ErrorKind::NotFound,
            Self::GeneratorBuildsNoSurface => ErrorKind::Unsupported,
            Self::GnSideInputMissing { .. } => ErrorKind::InvalidInput,
            Self::GuessDoesNotDropOnSideEdge => ErrorKind::NotFound,
            Self::JacobianNotLoaded => ErrorKind::Failed,
            Self::NoSingleCurveFoot { .. } => ErrorKind::NotFound,
            Self::OffsetNormalVanishes { .. } => ErrorKind::Degenerate,
            Self::OffsetSurfaceMissing => ErrorKind::InvalidInput,
            Self::PiecesTooFarApart { .. } => ErrorKind::NotWithinTolerance,
            Self::PointDoesNotDropOnSurface { .. } => ErrorKind::NotFound,
            Self::RadiusNotConstant { .. } => ErrorKind::Unsupported,
            Self::RadiusVanishes { .. } => ErrorKind::Degenerate,
            Self::RailMissesSideEdge => ErrorKind::NotFound,
            Self::RailNotBSpline { .. } => ErrorKind::Unsupported,
            Self::RailTangentZero { .. } => ErrorKind::Degenerate,
            Self::RolloverIntervalNotAtEdge { .. } => ErrorKind::Unsupported,
            Self::ShortIntervalsNotRemoved { .. } => ErrorKind::Unsupported,
            Self::SideEdgeNotOnRailFace => ErrorKind::NotFound,
            Self::SideSurfaceMissesEndPlane => ErrorKind::NotFound,
            Self::SolverEdgeUseMissing { .. } => ErrorKind::InvalidInput,
            Self::SpineStationNotFound => ErrorKind::NotFound,
            Self::SupportCurveMissesSurface => ErrorKind::NotFound,
            Self::SupportEndOffCurve { .. } => ErrorKind::NotFound,
            Self::SupportIntersectionFailed => ErrorKind::Failed,
            Self::SupportSurfacesDoNotMeet => ErrorKind::NotFound,
            Self::SurfaceCacheNotBuilt => ErrorKind::Failed,
            Self::SweepAngleNearHalfTurn { .. } => ErrorKind::Unsupported,
            Self::SweepAngleTooSmall { .. } => ErrorKind::Degenerate,
            Self::TangentRolloverUnhandled => ErrorKind::Unsupported,
            Self::TooManySolverValues { .. } => ErrorKind::InvalidInput,
            Self::TraceEndOffBall { .. } => ErrorKind::NotWithinTolerance,
        }
    }

    /// The status the C++ reports unless the site says otherwise (a comparison detail).
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::AnalyticShapeDoesNotApply { .. } => SM_ERR,
            Self::BaseSolverMethod(_) => SM_ERR,
            Self::CenterLineNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::ClippedIsoMissesSurface { .. } => SM_ERR,
            Self::CornerDoesNotDropOnSideCurve => SM_ERR,
            Self::CrossSectionMissesSideSurface => SM_ERR,
            Self::CrossSectionNotFound => SM_ERR,
            Self::CrossSectionPointNotFound { .. } => SM_ERR,
            Self::EdgeEndOnAxis { .. } => SM_ERR,
            Self::EdgeUseWithoutFace => SM_ERR_NULL_POINTER,
            Self::EdgeUseWithoutMate => SM_ERR_NULL_POINTER,
            Self::ExtensionCornerMissing => SM_ERR_NULL_POINTER,
            Self::FilletGeomWithoutRail { .. } => SM_ERR_NULL_POINTER,
            Self::FilletGeomWithoutSolver { .. } => SM_ERR_NULL_POINTER,
            Self::FilletGeomWithoutSurface { .. } => SM_ERR_NULL_POINTER,
            Self::FootNotFound { .. } => SM_ERR,
            Self::GeneratorBuildsNoSurface => SM_ERR,
            Self::GnSideInputMissing { .. } => SM_ERR_NULL_POINTER,
            Self::GuessDoesNotDropOnSideEdge => SM_ERR,
            Self::JacobianNotLoaded => SM_ERR,
            Self::NoSingleCurveFoot { .. } => SM_ERR,
            Self::OffsetNormalVanishes { .. } => SM_ERR,
            Self::OffsetSurfaceMissing => SM_ERR,
            Self::PiecesTooFarApart { .. } => SM_ERR,
            Self::PointDoesNotDropOnSurface { .. } => SM_ERR,
            Self::RadiusNotConstant { .. } => SM_ERR,
            Self::RadiusVanishes { .. } => SM_ERR,
            Self::RailMissesSideEdge => SM_ERR,
            Self::RailNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::RailTangentZero { .. } => SM_ERR,
            Self::RolloverIntervalNotAtEdge { .. } => SM_ERR,
            Self::ShortIntervalsNotRemoved { .. } => SM_ERR,
            Self::SideEdgeNotOnRailFace => SM_ERR_NULL_POINTER,
            Self::SideSurfaceMissesEndPlane => SM_ERR_NULL_POINTER,
            Self::SolverEdgeUseMissing { .. } => SM_ERR_NULL_POINTER,
            Self::SpineStationNotFound => SM_ERR,
            Self::SupportCurveMissesSurface => SM_ERR,
            Self::SupportEndOffCurve { .. } => SM_ERR,
            Self::SupportIntersectionFailed => SM_ERR,
            Self::SupportSurfacesDoNotMeet => SM_ERR,
            Self::SurfaceCacheNotBuilt => SM_ERR_NULL_POINTER,
            Self::SweepAngleNearHalfTurn { .. } => SM_ERR,
            Self::SweepAngleTooSmall { .. } => SM_ERR,
            Self::TangentRolloverUnhandled => SM_ERR,
            Self::TooManySolverValues { .. } => SM_ERR,
            Self::TraceEndOffBall { .. } => SM_ERR,
        }
    }
}

impl core::fmt::Display for SolverFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let end = |at_end: bool| if at_end { "end" } else { "start" };
        match self {
            Self::AnalyticShapeDoesNotApply { shape } => {
                write!(f, "the fillet is not of the analytic shape {shape:?}")
            }
            Self::BaseSolverMethod(method) => {
                write!(f, "the base fillet solver does not implement {method:?}")
            }
            Self::CenterLineNotBSpline { fillet_geom } => write!(
                f,
                "the centre line of fillet geometry {} has no B-spline form",
                fillet_geom.0
            ),
            Self::ClippedIsoMissesSurface { radius } => write!(
                f,
                "at radius {radius} the iso curve does not meet the other offset surface"
            ),
            Self::CornerDoesNotDropOnSideCurve => {
                f.write_str("the corner point does not drop onto the side edge's curve")
            }
            Self::CrossSectionMissesSideSurface => {
                f.write_str("the corner's cross section does not meet the extended side surface")
            }
            Self::CrossSectionNotFound => {
                f.write_str("no cross section of the fillet was found on the plane")
            }
            Self::CrossSectionPointNotFound { t } => write!(
                f,
                "no cross-section point of the fillet was found on the plane at t = {t}"
            ),
            Self::EdgeEndOnAxis { side } => {
                write!(
                    f,
                    "an end of the filleted edge lies on cylinder {side}'s axis"
                )
            }
            Self::EdgeUseWithoutFace => f.write_str("an edge use of the body has no face"),
            Self::EdgeUseWithoutMate => f.write_str("an edge use of the body has no mate"),
            Self::ExtensionCornerMissing => {
                f.write_str("the extension flags need a vertex's corner and no corners were given")
            }
            Self::FilletGeomWithoutRail { fillet_geom, side } => {
                write!(
                    f,
                    "fillet geometry {fillet_geom:?} has no rail on side {side}"
                )
            }
            Self::FilletGeomWithoutSolver { fillet_geom } => {
                write!(f, "fillet geometry {fillet_geom:?} names no fillet solver")
            }
            Self::FilletGeomWithoutSurface { fillet_geom } => {
                write!(f, "fillet geometry {fillet_geom:?} has no fillet surface")
            }
            Self::FootNotFound { side } => {
                write!(f, "the ball's centre has no foot on face {side}'s quadric")
            }
            Self::GeneratorBuildsNoSurface => {
                f.write_str("the base surface generator builds no surface")
            }
            Self::GnSideInputMissing { side, uv_curve } => write!(
                f,
                "the G^n section generator was given no {} for rail side {side}",
                if *uv_curve { "uv curve" } else { "surface" }
            ),
            Self::GuessDoesNotDropOnSideEdge => {
                f.write_str("the guess point does not drop onto the side edge")
            }
            Self::JacobianNotLoaded => f.write_str("the solver's equations could not be loaded"),
            Self::NoSingleCurveFoot { count } => write!(
                f,
                "the nearest-point search on the curve found {count} points where one is needed"
            ),
            Self::OffsetNormalVanishes { side, u, v } => write!(
                f,
                "offset surface {side} has no normal at (u, v) = ({u}, {v})"
            ),
            Self::OffsetSurfaceMissing => {
                f.write_str("the solver has not got both offset surfaces set")
            }
            Self::PiecesTooFarApart { distance, limit } => write!(
                f,
                "the pieces to blend are {distance} apart, farther than {limit}"
            ),
            Self::PointDoesNotDropOnSurface { side } => {
                write!(f, "the point does not drop onto offset surface {side}")
            }
            Self::RadiusNotConstant { start, end } => write!(
                f,
                "the fillet's radius is {start} at its start and {end} at its end, not constant"
            ),
            Self::RadiusVanishes { radius } => {
                write!(
                    f,
                    "the fillet's radius {radius} is below the zero tolerance"
                )
            }
            Self::RailMissesSideEdge => f.write_str("the rail does not cross the side edge"),
            Self::RailNotBSpline {
                fillet_geom,
                side,
                kind,
            } => write!(
                f,
                "rail {side} of fillet geometry {fillet_geom:?} has a {kind} curve, not of a \
                 B-spline class"
            ),
            Self::RailTangentZero { t } => {
                write!(f, "the rail's tangent has no length at t = {t}")
            }
            Self::RolloverIntervalNotAtEdge { index } => write!(
                f,
                "interval {index} of the roll-over rail's classification is not bounded by an edge"
            ),
            Self::ShortIntervalsNotRemoved { count } => write!(
                f,
                "{count} short intervals of the rail's classification could not be removed"
            ),
            Self::SideEdgeNotOnRailFace => {
                f.write_str("the side edge has no edge use on the rail's face")
            }
            Self::SideSurfaceMissesEndPlane => f.write_str(
                "the side surface and the plane through the side curve's end do not meet",
            ),
            Self::SolverEdgeUseMissing { index } => {
                write!(
                    f,
                    "the solver has no edge use {index} of the edge to fillet"
                )
            }
            Self::SpineStationNotFound => {
                f.write_str("the circular spine has no station in the point's half-plane")
            }
            Self::SupportCurveMissesSurface => {
                f.write_str("the support edge's curve does not meet the fillet surface ending it")
            }
            Self::SupportEndOffCurve { at_end } => write!(
                f,
                "the support edge's {} does not drop onto its curve",
                end(*at_end)
            ),
            Self::SupportIntersectionFailed => {
                f.write_str("the intersection of the support edge's fillet surfaces failed")
            }
            Self::SupportSurfacesDoNotMeet => {
                f.write_str("the support edge's fillet surfaces do not meet")
            }
            Self::SurfaceCacheNotBuilt => f.write_str("the surface's cache could not be built"),
            Self::SweepAngleNearHalfTurn { angle } => write!(
                f,
                "the fillet's section turns through {angle} radians, about half a turn or more"
            ),
            Self::SweepAngleTooSmall { angle } => {
                write!(f, "the fillet's section turns through only {angle} radians")
            }
            Self::TangentRolloverUnhandled => {
                f.write_str("a side edge rolls over tangentially, which is not handled")
            }
            Self::TooManySolverValues { limit } => write!(
                f,
                "the solver solves for more further values than a fillet point holds ({limit})"
            ),
            Self::TraceEndOffBall { deviation, tol } => write!(
                f,
                "an end of the trace lies {deviation} off the ball, farther than {tol}"
            ),
        }
    }
}
