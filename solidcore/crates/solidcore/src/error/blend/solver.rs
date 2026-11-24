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
    /// Offset surface `side`'s base is neither of a B-spline class nor an offset of one: a
    /// kind the routine cannot handle ([`ErrorKind::Unsupported`]); status
    /// `SM_ERR_NULL_POINTER`.
    BaseSurfaceNotBSpline {
        /// The offset surface, 0 or 1.
        side: usize,
        /// The kind (type string) of the surface found not of a B-spline class: the base,
        /// or, when the base is an offset surface, the base's own base.
        kind: &'static str,
    },
    /// The fillet geometry's centre line is of a curve kind with no B-spline form: a kind
    /// the routine cannot handle ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER`.
    CenterLineNotBSpline {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
    },
    /// The caller's classification context classed a point as on an edge with an object
    /// that is not an edge: the caller's input ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER`.
    ClassificationEdgePointNotOnEdge,
    /// The last piece of the roll-over runs off its face over a corner of the body, as a
    /// cliff it does not trim; the last piece is not split, so no vertex starts the
    /// corner's support edge: a case the roll-over does not handle
    /// ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER`.
    CliffCornerOnLastPiece,
    /// The clipped-radius search's iso curve at radius `radius` does not meet the other
    /// offset surface: an empty search ([`ErrorKind::NotFound`]).
    ClippedIsoMissesSurface {
        /// The radius the offsets were set to.
        radius: f64,
    },
    /// A closed rail runs off its face, and the corner at its start is not one closed
    /// blend meeting itself (of type `corner_type`): the full roll-over handles only that
    /// corner ([`ErrorKind::Unsupported`]).
    ClosedBlendDoesNotMeetItself {
        /// The corner's type.
        corner_type: crate::blend::fillet_geom::p1::FilletCornerType,
    },
    /// The corner point does not drop onto the side edge's curve: an empty search
    /// ([`ErrorKind::NotFound`]).
    CornerDoesNotDropOnSideCurve,
    /// The solve of the roll-over's corner vertex did not converge (the vertex is marked
    /// so): no numbers are kept ([`ErrorKind::NotConverged`]).
    CornerVertexNotConverged,
    /// The corner's cross section does not meet the extended side surface: an empty search
    /// ([`ErrorKind::NotFound`]).
    CrossSectionMissesSideSurface,
    /// The solve for the fillet's cross section on the plane through the failure's site
    /// found no section: an empty search ([`ErrorKind::NotFound`]).
    CrossSectionNotFound,
    /// The solve for the fillet's cross-section point on the plane through a rail's point
    /// at parameter `t` returned an error: that error is the failure's cause, and decides
    /// whether it is a defect or a refusal ([`ErrorKind::Failed`]); status `SM_ERR`.
    CrossSectionPointFailed {
        /// The parameter of the curve the plane is taken on.
        t: f64,
    },
    /// The solve for the fillet's cross-section point on the plane through a rail's point
    /// at parameter `t` found no point: an empty search ([`ErrorKind::NotFound`]).
    CrossSectionPointNotFound {
        /// The parameter of the curve the plane is taken on.
        t: f64,
    },
    /// A rail's crossing of its side edge does not drop onto the end section (the
    /// intersection of the fillet surface and the end face's surface): an empty search
    /// ([`ErrorKind::NotFound`]).
    CrossingDoesNotDropOnEndSection,
    /// A rail's crossing of its side edge drops onto the end section `distance` away,
    /// farther than `limit` (ten times the solver's 3D tolerance): not within tolerance
    /// ([`ErrorKind::NotWithinTolerance`]).
    CrossingOffEndSection {
        /// The distance from the crossing to its foot on the end section.
        distance: f64,
        /// The largest distance accepted.
        limit: f64,
    },
    /// The edge bounds no material (no use of it lies in a loop of a shell of a region
    /// that is not a void, as a sheet's edge), so it is neither concave nor convex: a case
    /// the concavity test does not handle ([`ErrorKind::Unsupported`]). Also the answer,
    /// as before #19829, for an edge with no such use and a loop use without a shell or in
    /// a shell without a region (`edge_bounds_material_linked`).
    EdgeBoundsNoMaterial {
        /// The edge.
        edge: crate::topology::EdgeId,
    },
    /// The cache of the curve of `edge`, an edge of the caller's body the solver is loaded on,
    /// could not be built: the cache's error is the failure's cause, and decides whether it
    /// is a defect or a refusal ([`ErrorKind::Failed`]); status `SM_ERR_NULL_POINTER`.
    EdgeCurveCacheNotBuilt {
        /// The edge.
        edge: crate::topology::EdgeId,
    },
    /// The edge to fillet has a curve not of a B-spline class: a kind the routine cannot
    /// handle ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER`.
    EdgeCurveNotBSpline {
        /// The edge.
        edge: crate::topology::EdgeId,
        /// The curve's kind (its type string).
        kind: &'static str,
    },
    /// An end of the filleted edge lies on cylinder `side`'s axis, so the ball's contact
    /// there has no direction from the axis ([`ErrorKind::Degenerate`]).
    EdgeEndOnAxis {
        /// The cylinder, 0 or 1.
        side: usize,
    },
    /// The filleted edge's middle does not drop onto a rail that is nowhere on its face:
    /// an empty search ([`ErrorKind::NotFound`]).
    EdgeMiddleDoesNotDropOnRail,
    /// An edge use of the caller's body has no face: the caller's input
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    EdgeUseWithoutFace,
    /// An edge use of the caller's body has no mate: the caller's input
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    EdgeUseWithoutMate,
    /// An edge of the caller's body, or a rail of the caller's fillet geometry, lacks a
    /// start or end vertex: the caller's input ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER`.
    EdgeWithoutVertex {
        /// The edge.
        edge: crate::topology::EdgeId,
    },
    /// The end cap of the caller's corner is not a plane: the caller's corner
    /// ([`ErrorKind::InvalidInput`]).
    EndCapNotPlane,
    /// The extension flags need a vertex's corner and the caller gave no corners (a
    /// standalone entry point): the caller's input ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER`.
    ExtensionCornerMissing,
    /// The caller's fillet geometry has no centre line: the caller's solver set-up
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    FilletGeomWithoutCenterLine {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
    },
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
    /// A point the law intersection traced through has curve parameter `parameter`, below
    /// zero: it lies before the start of the curve the trace builds
    /// ([`ErrorKind::OutsideDomain`]). A refusal, not a defect: the trace follows the
    /// offset surfaces' intersection, which the caller's geometry can carry back past the
    /// curve's start.
    IntersectionPointBeforeCurve {
        /// The point's curve parameter.
        parameter: f64,
    },
    /// A solver's equations could not be loaded for the solve: the solver's error is the
    /// failure's cause, and decides whether it is a defect or a refusal
    /// ([`ErrorKind::Failed`]).
    JacobianNotLoaded,
    /// The rail's point where it leaves its face does not drop onto the filleted edge: an
    /// empty search ([`ErrorKind::NotFound`]).
    LeavePointDoesNotDropOnEdge,
    /// The intersection point has no stepped neighbour on either side to take its
    /// curve's tangent from: an empty search ([`ErrorKind::NotFound`]).
    NeighbourPointsNotFound,
    /// The trace stepped no part of the last fillet geometry's centre line, so the cusp
    /// recovery has no end to start from: an empty search ([`ErrorKind::NotFound`]);
    /// status `SM_ERR_NULL_POINTER`.
    NoCenterLineTraced,
    /// The nearest-point search on a curve found `count` nearest points where one is
    /// needed: an empty or ambiguous search ([`ErrorKind::NotFound`]).
    NoSingleCurveFoot {
        /// The number of nearest points found.
        count: usize,
    },
    /// The trace left no cross-section points to clean at its ends: an empty search
    /// ([`ErrorKind::NotFound`]).
    NoTracePoints,
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
    /// The two offset surfaces' normals are parallel at a point the curve is to start from
    /// (a tangent intersection point, or the solver's two uv guesses), so their cross
    /// product gives the curve no direction ([`ErrorKind::Degenerate`]).
    OffsetNormalsParallel,
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
    /// The point drops onto no edge of the face that has a curve: an empty search
    /// ([`ErrorKind::NotFound`]); status `SM_ERR_NULL_POINTER`.
    PointDropsOnNoFaceEdge,
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
    /// Rail `side` of the caller's fillet geometry has `count` edge uses where two are
    /// needed: the caller's solver set-up ([`ErrorKind::InvalidInput`]).
    RailEdgeUseCount {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
        /// The rail side, 0 or 1.
        side: usize,
        /// The rail's edge uses.
        count: usize,
    },
    /// A rail of the caller's fillet geometry lacks its primary edge use or that use's
    /// mate: the caller's solver set-up ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER`.
    RailEdgeUseMissing {
        /// The rail.
        rail: crate::topology::EdgeId,
    },
    /// The mate edge use of rail `side` of the caller's fillet geometry has no uv trim
    /// curve: the caller's solver set-up ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER`.
    RailMateWithoutUvCurve {
        /// The fillet geometry.
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
        /// The rail side, 0 or 1.
        side: usize,
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
    /// The roll-over was given a rail with no curve classification: the caller's input
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    RailNotClassified {
        /// The rail, 0 or 1.
        which: usize,
    },
    /// A rail curve's tangent has no length at parameter `t`, so no step along it is
    /// defined there ([`ErrorKind::Degenerate`]).
    RailTangentZero {
        /// The rail curve's parameter.
        t: f64,
    },
    /// A rail of the caller's fillet geometry records no original face: the caller's
    /// solver set-up ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    RailWithoutFace {
        /// The rail.
        rail: crate::topology::EdgeId,
    },
    /// A rail of the caller's fillet geometry has no uv curve on its face: the caller's
    /// solver set-up ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    RailWithoutUvCurve {
        /// The rail.
        rail: crate::topology::EdgeId,
    },
    /// The closed roll-over needs the corner at rail 0's start vertex, and the caller's
    /// fillet vertex there has none: the caller's input ([`ErrorKind::InvalidInput`]);
    /// status `SM_ERR_NULL_POINTER`.
    RolloverCornerMissing,
    /// The rail rolls over side edge `edge`, which has no face beyond the rail's face (an
    /// edge of a sheet's boundary): nothing to roll onto, a case the roll-over does not
    /// handle ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER`.
    RolloverHasNoFaceBeyond {
        /// The side edge.
        edge: crate::topology::EdgeId,
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
    /// The split point does not drop onto the rail's curve at the dropping tolerances
    /// tried (1, 10 and 20 times `tol`): an empty search ([`ErrorKind::NotFound`]).
    SplitPointDoesNotDropOnRail {
        /// The trace's 3D approximation tolerance the dropping tolerances scale.
        tol: f64,
    },
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
    /// The trace ran more than `limit` passes (too many self-intersections or other
    /// restarts) without finishing ([`ErrorKind::NotConverged`]).
    TraceLoopLimit {
        /// The passes allowed.
        limit: u32,
    },
    /// The trace touched a base surface's boundary again after tracing on past it once, a
    /// case the trace does not handle ([`ErrorKind::Unsupported`]).
    TraceTouchesBoundaryAgain,
    /// A vertex use of the caller's body has no vertex: the caller's input
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    VertexUseWithoutVertex,
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
            Self::BaseSurfaceNotBSpline { .. } => ErrorKind::Unsupported,
            Self::CenterLineNotBSpline { .. } => ErrorKind::Unsupported,
            Self::ClassificationEdgePointNotOnEdge => ErrorKind::InvalidInput,
            Self::CliffCornerOnLastPiece => ErrorKind::Unsupported,
            Self::ClippedIsoMissesSurface { .. } => ErrorKind::NotFound,
            Self::ClosedBlendDoesNotMeetItself { .. } => ErrorKind::Unsupported,
            Self::CornerDoesNotDropOnSideCurve => ErrorKind::NotFound,
            Self::CornerVertexNotConverged => ErrorKind::NotConverged,
            Self::CrossSectionMissesSideSurface => ErrorKind::NotFound,
            Self::CrossSectionNotFound => ErrorKind::NotFound,
            Self::CrossSectionPointFailed { .. } => ErrorKind::Failed,
            Self::CrossSectionPointNotFound { .. } => ErrorKind::NotFound,
            Self::CrossingDoesNotDropOnEndSection => ErrorKind::NotFound,
            Self::CrossingOffEndSection { .. } => ErrorKind::NotWithinTolerance,
            Self::EdgeBoundsNoMaterial { .. } => ErrorKind::Unsupported,
            Self::EdgeCurveCacheNotBuilt { .. } => ErrorKind::Failed,
            Self::EdgeCurveNotBSpline { .. } => ErrorKind::Unsupported,
            Self::EdgeEndOnAxis { .. } => ErrorKind::Degenerate,
            Self::EdgeMiddleDoesNotDropOnRail => ErrorKind::NotFound,
            Self::EdgeUseWithoutFace => ErrorKind::InvalidInput,
            Self::EdgeUseWithoutMate => ErrorKind::InvalidInput,
            Self::EdgeWithoutVertex { .. } => ErrorKind::InvalidInput,
            Self::EndCapNotPlane => ErrorKind::InvalidInput,
            Self::ExtensionCornerMissing => ErrorKind::InvalidInput,
            Self::FilletGeomWithoutCenterLine { .. } => ErrorKind::InvalidInput,
            Self::FilletGeomWithoutRail { .. } => ErrorKind::InvalidInput,
            Self::FilletGeomWithoutSolver { .. } => ErrorKind::InvalidInput,
            Self::FilletGeomWithoutSurface { .. } => ErrorKind::InvalidInput,
            Self::FootNotFound { .. } => ErrorKind::NotFound,
            Self::GeneratorBuildsNoSurface => ErrorKind::Unsupported,
            Self::GnSideInputMissing { .. } => ErrorKind::InvalidInput,
            Self::GuessDoesNotDropOnSideEdge => ErrorKind::NotFound,
            Self::IntersectionPointBeforeCurve { .. } => ErrorKind::OutsideDomain,
            Self::JacobianNotLoaded => ErrorKind::Failed,
            Self::LeavePointDoesNotDropOnEdge => ErrorKind::NotFound,
            Self::NeighbourPointsNotFound => ErrorKind::NotFound,
            Self::NoCenterLineTraced => ErrorKind::NotFound,
            Self::NoSingleCurveFoot { .. } => ErrorKind::NotFound,
            Self::NoTracePoints => ErrorKind::NotFound,
            Self::OffsetNormalVanishes { .. } => ErrorKind::Degenerate,
            Self::OffsetNormalsParallel => ErrorKind::Degenerate,
            Self::OffsetSurfaceMissing => ErrorKind::InvalidInput,
            Self::PiecesTooFarApart { .. } => ErrorKind::NotWithinTolerance,
            Self::PointDoesNotDropOnSurface { .. } => ErrorKind::NotFound,
            Self::PointDropsOnNoFaceEdge => ErrorKind::NotFound,
            Self::RadiusNotConstant { .. } => ErrorKind::Unsupported,
            Self::RadiusVanishes { .. } => ErrorKind::Degenerate,
            Self::RailEdgeUseCount { .. } => ErrorKind::InvalidInput,
            Self::RailEdgeUseMissing { .. } => ErrorKind::InvalidInput,
            Self::RailMateWithoutUvCurve { .. } => ErrorKind::InvalidInput,
            Self::RailMissesSideEdge => ErrorKind::NotFound,
            Self::RailNotBSpline { .. } => ErrorKind::Unsupported,
            Self::RailNotClassified { .. } => ErrorKind::InvalidInput,
            Self::RailTangentZero { .. } => ErrorKind::Degenerate,
            Self::RailWithoutFace { .. } => ErrorKind::InvalidInput,
            Self::RailWithoutUvCurve { .. } => ErrorKind::InvalidInput,
            Self::RolloverCornerMissing => ErrorKind::InvalidInput,
            Self::RolloverHasNoFaceBeyond { .. } => ErrorKind::Unsupported,
            Self::RolloverIntervalNotAtEdge { .. } => ErrorKind::Unsupported,
            Self::ShortIntervalsNotRemoved { .. } => ErrorKind::Unsupported,
            Self::SideEdgeNotOnRailFace => ErrorKind::NotFound,
            Self::SideSurfaceMissesEndPlane => ErrorKind::NotFound,
            Self::SolverEdgeUseMissing { .. } => ErrorKind::InvalidInput,
            Self::SpineStationNotFound => ErrorKind::NotFound,
            Self::SplitPointDoesNotDropOnRail { .. } => ErrorKind::NotFound,
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
            Self::TraceLoopLimit { .. } => ErrorKind::NotConverged,
            Self::TraceTouchesBoundaryAgain => ErrorKind::Unsupported,
            Self::VertexUseWithoutVertex => ErrorKind::InvalidInput,
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
            Self::BaseSurfaceNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::CenterLineNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::ClassificationEdgePointNotOnEdge => SM_ERR_NULL_POINTER,
            Self::CliffCornerOnLastPiece => SM_ERR_NULL_POINTER,
            Self::ClippedIsoMissesSurface { .. } => SM_ERR,
            Self::ClosedBlendDoesNotMeetItself { .. } => SM_ERR,
            Self::CornerDoesNotDropOnSideCurve => SM_ERR,
            Self::CornerVertexNotConverged => SM_ERR,
            Self::CrossSectionMissesSideSurface => SM_ERR,
            Self::CrossSectionNotFound => SM_ERR,
            Self::CrossSectionPointFailed { .. } => SM_ERR,
            Self::CrossSectionPointNotFound { .. } => SM_ERR,
            Self::CrossingDoesNotDropOnEndSection => SM_ERR,
            Self::CrossingOffEndSection { .. } => SM_ERR,
            Self::EdgeBoundsNoMaterial { .. } => SM_ERR,
            Self::EdgeCurveCacheNotBuilt { .. } => SM_ERR_NULL_POINTER,
            Self::EdgeCurveNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::EdgeEndOnAxis { .. } => SM_ERR,
            Self::EdgeMiddleDoesNotDropOnRail => SM_ERR,
            Self::EdgeUseWithoutFace => SM_ERR_NULL_POINTER,
            Self::EdgeUseWithoutMate => SM_ERR_NULL_POINTER,
            Self::EdgeWithoutVertex { .. } => SM_ERR_NULL_POINTER,
            Self::EndCapNotPlane => SM_ERR,
            Self::ExtensionCornerMissing => SM_ERR_NULL_POINTER,
            Self::FilletGeomWithoutCenterLine { .. } => SM_ERR_NULL_POINTER,
            Self::FilletGeomWithoutRail { .. } => SM_ERR_NULL_POINTER,
            Self::FilletGeomWithoutSolver { .. } => SM_ERR_NULL_POINTER,
            Self::FilletGeomWithoutSurface { .. } => SM_ERR_NULL_POINTER,
            Self::FootNotFound { .. } => SM_ERR,
            Self::GeneratorBuildsNoSurface => SM_ERR,
            Self::GnSideInputMissing { .. } => SM_ERR_NULL_POINTER,
            Self::GuessDoesNotDropOnSideEdge => SM_ERR,
            Self::IntersectionPointBeforeCurve { .. } => SM_ERR,
            Self::JacobianNotLoaded => SM_ERR,
            Self::LeavePointDoesNotDropOnEdge => SM_ERR,
            Self::NeighbourPointsNotFound => SM_ERR,
            Self::NoCenterLineTraced => SM_ERR_NULL_POINTER,
            Self::NoSingleCurveFoot { .. } => SM_ERR,
            Self::NoTracePoints => SM_ERR,
            Self::OffsetNormalVanishes { .. } => SM_ERR,
            Self::OffsetNormalsParallel => SM_ERR,
            Self::OffsetSurfaceMissing => SM_ERR,
            Self::PiecesTooFarApart { .. } => SM_ERR,
            Self::PointDoesNotDropOnSurface { .. } => SM_ERR,
            Self::PointDropsOnNoFaceEdge => SM_ERR_NULL_POINTER,
            Self::RadiusNotConstant { .. } => SM_ERR,
            Self::RadiusVanishes { .. } => SM_ERR,
            Self::RailEdgeUseCount { .. } => SM_ERR,
            Self::RailEdgeUseMissing { .. } => SM_ERR_NULL_POINTER,
            Self::RailMateWithoutUvCurve { .. } => SM_ERR_NULL_POINTER,
            Self::RailMissesSideEdge => SM_ERR,
            Self::RailNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::RailNotClassified { .. } => SM_ERR_NULL_POINTER,
            Self::RailTangentZero { .. } => SM_ERR,
            Self::RailWithoutFace { .. } => SM_ERR_NULL_POINTER,
            Self::RailWithoutUvCurve { .. } => SM_ERR_NULL_POINTER,
            Self::RolloverCornerMissing => SM_ERR_NULL_POINTER,
            Self::RolloverHasNoFaceBeyond { .. } => SM_ERR_NULL_POINTER,
            Self::RolloverIntervalNotAtEdge { .. } => SM_ERR,
            Self::ShortIntervalsNotRemoved { .. } => SM_ERR,
            Self::SideEdgeNotOnRailFace => SM_ERR_NULL_POINTER,
            Self::SideSurfaceMissesEndPlane => SM_ERR_NULL_POINTER,
            Self::SolverEdgeUseMissing { .. } => SM_ERR_NULL_POINTER,
            Self::SpineStationNotFound => SM_ERR,
            Self::SplitPointDoesNotDropOnRail { .. } => SM_ERR,
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
            Self::TraceLoopLimit { .. } => SM_ERR,
            Self::TraceTouchesBoundaryAgain => SM_ERR,
            Self::VertexUseWithoutVertex => SM_ERR_NULL_POINTER,
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
            Self::BaseSurfaceNotBSpline { side, kind } => write!(
                f,
                "offset surface {side}'s base (or, for an offset base, its base) is a {kind} \
                 surface, not of a B-spline class"
            ),
            Self::CenterLineNotBSpline { fillet_geom } => write!(
                f,
                "the centre line of fillet geometry {} has no B-spline form",
                fillet_geom.0
            ),
            Self::ClassificationEdgePointNotOnEdge => {
                f.write_str("the classification put a point on an object that is not an edge")
            }
            Self::CliffCornerOnLastPiece => f.write_str(
                "the last piece of the roll-over runs over a corner, with no split vertex to \
                 start the corner's support edge",
            ),
            Self::ClippedIsoMissesSurface { radius } => write!(
                f,
                "at radius {radius} the iso curve does not meet the other offset surface"
            ),
            Self::ClosedBlendDoesNotMeetItself { corner_type } => write!(
                f,
                "the corner of the closed roll-over ({corner_type:?}) is not one blend meeting \
                 itself"
            ),
            Self::CornerDoesNotDropOnSideCurve => {
                f.write_str("the corner point does not drop onto the side edge's curve")
            }
            Self::CornerVertexNotConverged => {
                f.write_str("the solve of the roll-over's corner vertex did not converge")
            }
            Self::CrossSectionMissesSideSurface => {
                f.write_str("the corner's cross section does not meet the extended side surface")
            }
            Self::CrossSectionNotFound => {
                f.write_str("no cross section of the fillet was found on the plane")
            }
            Self::CrossSectionPointFailed { t } => write!(
                f,
                "the solve for the fillet's cross-section point on the plane at t = {t} failed"
            ),
            Self::CrossSectionPointNotFound { t } => write!(
                f,
                "no cross-section point of the fillet was found on the plane at t = {t}"
            ),
            Self::CrossingDoesNotDropOnEndSection => {
                f.write_str("the rail's crossing does not drop onto the end section")
            }
            Self::CrossingOffEndSection { distance, limit } => write!(
                f,
                "the rail's crossing drops onto the end section {distance} away, farther than \
                 {limit}"
            ),
            Self::EdgeBoundsNoMaterial { edge } => write!(
                f,
                "edge {} bounds no material, so it is neither concave nor convex",
                edge.index()
            ),
            Self::EdgeCurveCacheNotBuilt { edge } => write!(
                f,
                "the cache of the curve of edge {} could not be built",
                edge.index()
            ),
            Self::EdgeCurveNotBSpline { edge, kind } => write!(
                f,
                "the {kind} curve of edge {edge:?} to fillet is not of a B-spline class"
            ),
            Self::EdgeEndOnAxis { side } => {
                write!(
                    f,
                    "an end of the filleted edge lies on cylinder {side}'s axis"
                )
            }
            Self::EdgeMiddleDoesNotDropOnRail => {
                f.write_str("the filleted edge's middle does not drop onto the rail")
            }
            Self::EdgeUseWithoutFace => f.write_str("an edge use of the body has no face"),
            Self::EdgeUseWithoutMate => f.write_str("an edge use of the body has no mate"),
            Self::EdgeWithoutVertex { edge } => {
                write!(f, "edge {} of the body lacks a vertex", edge.index())
            }
            Self::EndCapNotPlane => f.write_str("the corner's end cap is not a plane"),
            Self::ExtensionCornerMissing => {
                f.write_str("the extension flags need a vertex's corner and no corners were given")
            }
            Self::FilletGeomWithoutCenterLine { fillet_geom } => {
                write!(f, "fillet geometry {fillet_geom:?} has no centre line")
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
            Self::IntersectionPointBeforeCurve { parameter } => write!(
                f,
                "a point the intersection traced has curve parameter {parameter}, before the \
                 curve's start"
            ),
            Self::JacobianNotLoaded => f.write_str("the solver's equations could not be loaded"),
            Self::LeavePointDoesNotDropOnEdge => f.write_str(
                "the point where the rail leaves its face does not drop onto the filleted edge",
            ),
            Self::NeighbourPointsNotFound => {
                f.write_str("the intersection point has no stepped neighbour on either side")
            }
            Self::NoCenterLineTraced => {
                f.write_str("the trace stepped no part of the fillet's centre line")
            }
            Self::NoSingleCurveFoot { count } => write!(
                f,
                "the nearest-point search on the curve found {count} points where one is needed"
            ),
            Self::NoTracePoints => f.write_str("the trace left no cross-section points"),
            Self::OffsetNormalVanishes { side, u, v } => write!(
                f,
                "offset surface {side} has no normal at (u, v) = ({u}, {v})"
            ),
            Self::OffsetNormalsParallel => {
                f.write_str("the offset surfaces' normals are parallel where the curve is to start")
            }
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
            Self::PointDropsOnNoFaceEdge => f.write_str("the point drops onto no edge of the face"),
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
            Self::RailEdgeUseCount {
                fillet_geom,
                side,
                count,
            } => write!(
                f,
                "rail {side} of fillet geometry {fillet_geom:?} has {count} edge uses, not two"
            ),
            Self::RailEdgeUseMissing { rail } => write!(
                f,
                "rail {} of the fillet geometry lacks its primary edge use or that use's mate",
                rail.index()
            ),
            Self::RailMateWithoutUvCurve { fillet_geom, side } => write!(
                f,
                "the mate edge use of rail {side} of fillet geometry {fillet_geom:?} has no uv \
                 trim curve"
            ),
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
            Self::RailNotClassified { which } => {
                write!(
                    f,
                    "rail {which} of the roll-over has no curve classification"
                )
            }
            Self::RailTangentZero { t } => {
                write!(f, "the rail's tangent has no length at t = {t}")
            }
            Self::RailWithoutFace { rail } => write!(
                f,
                "rail {} of the fillet geometry records no original face",
                rail.index()
            ),
            Self::RailWithoutUvCurve { rail } => write!(
                f,
                "rail {} of the fillet geometry has no uv curve on its face",
                rail.index()
            ),
            Self::RolloverCornerMissing => {
                f.write_str("the closed roll-over's start vertex has no corner")
            }
            Self::RolloverHasNoFaceBeyond { edge } => write!(
                f,
                "side edge {} has no face beyond the rail's face to roll onto",
                edge.index()
            ),
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
            Self::SplitPointDoesNotDropOnRail { tol } => write!(
                f,
                "the split point does not drop onto the rail at 1, 10 or 20 times {tol}"
            ),
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
            Self::TraceLoopLimit { limit } => {
                write!(f, "the trace did not finish within {limit} passes")
            }
            Self::TraceTouchesBoundaryAgain => {
                f.write_str("the trace touched a base surface's boundary a second time")
            }
            Self::VertexUseWithoutVertex => f.write_str("a vertex use of the body has no vertex"),
        }
    }
}
