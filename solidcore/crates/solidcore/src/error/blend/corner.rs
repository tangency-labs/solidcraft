//! Why a fillet corner fails: the shapes of a filleted vertex's edges the fillet corner
//! topology does not build, the corner input a corner step is given otherwise than the
//! topology makes it, and the ball, chamfer and Mixed corner steps with no construction.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::{BlendFailure, FilletSetUp, SolverFailure};
use crate::error::{ErrorKind, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER};

/// A shape of the edges at a filleted vertex that the fillet corner topology
/// (`crate::blend::fillet_corner::topology`) does not build
/// ([`BlendFailure::CornerShapeNotBuilt`]). Each is a fact of the body's edges at the vertex;
/// none is a defect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CornerShape {
    /// A corner of four edges whose two blended edges are next to each other round the vertex.
    AdjacentBlendedEdges,
    /// A bevel corner (every edge blended, the blends not blending) of other than three edges.
    BevelNotOfThreeEdges,
    /// A blended edge use the walk round the vertex left out of the ring of the corner's
    /// edges: the walk keeps one use of each edge, so an edge that meets the vertex twice (a
    /// closed edge) is in the ring by its other use.
    BlendedEdgeOffRing,
    /// A corner of two edges alone at the vertex whose first blended edge does not lie
    /// between exactly two faces: the blends' crossing has an end on each of its faces.
    EdgeNotBetweenTwoFaces,
    /// An end corner (one blended edge) of at most three edges whose two side edges share
    /// no face.
    EndSideEdgesShareNoFace,
    /// A corner of two blended edges among more than four edges.
    MoreThanFourEdges,
    /// A concave corner of four edges, two blended, whose side edges are both on the concave
    /// side of the blends: no side edge is convex.
    NoConvexSideEdge,
    /// A Join at one closed blended edge where another edge lies on neither of the blend's
    /// faces.
    RingEdgeOffClosedBlendFaces,
    /// Two edges next to each other in the ring of the corner's edges that share no face:
    /// the ring keeps one use of each edge, so an edge that meets the vertex twice stands
    /// between edges that are not on one face.
    RingEdgesShareNoFace,
    /// A Roll-over's cap across the cliff's tangent seam whose end face cannot take the side
    /// face's extended surface (the other side edge does not lie on it): the cap is closed
    /// only by that end patch.
    SeamCapEndFaceNotExtended,
    /// A Join whose one edge between the blends on a side does not meet the first blended
    /// edge in a loop of either of its faces.
    SideEdgeNotBesideBlend,
}

impl CornerShape {
    /// The status the site the shape replaces returned: `SM_ERR_NULL_POINTER` where the C++
    /// dereferences the face or edge use it did not find (`NER`), else `SM_ERR`.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn upstream_status(self) -> i32 {
        match self {
            Self::AdjacentBlendedEdges => SM_ERR,
            Self::BevelNotOfThreeEdges => SM_ERR,
            Self::BlendedEdgeOffRing => SM_ERR,
            Self::EdgeNotBetweenTwoFaces => SM_ERR,
            Self::EndSideEdgesShareNoFace => SM_ERR,
            Self::MoreThanFourEdges => SM_ERR,
            Self::NoConvexSideEdge => SM_ERR_NULL_POINTER,
            Self::RingEdgeOffClosedBlendFaces => SM_ERR,
            Self::RingEdgesShareNoFace => SM_ERR_NULL_POINTER,
            Self::SeamCapEndFaceNotExtended => SM_ERR,
            Self::SideEdgeNotBesideBlend => SM_ERR_NULL_POINTER,
        }
    }
}

/// What a fillet corner given to a corner step lacks, or holds otherwise than the corner
/// topology makes it ([`BlendFailure::CornerInputInvalid`]): the conditions only the corner
/// steps check. A missing link of the fillet structures that other routines read too (a
/// fillet edge's primary edge use or its mate, a rail's end vertex use, a fillet vertex's
/// face, a cross-section's fillet geometry, a rail's curve) is
/// [`BlendFailure::FilletSetUpInvalid`] instead, a fillet geometry without its fillet
/// surface the solver's ([`SolverFailure::FilletGeomWithoutSurface`]), and a target-body
/// edge without curve is [`BlendFailure::EdgeWithoutCurve`].
///
/// [`CornerInput::upstream_status`] is each kind's default status. A site whose C++ reports
/// the condition otherwise (a failed state check, `SE(SM_ERR)`, where the default is
/// `SM_ERR_NULL_POINTER`) overrides it with that status, so the status is always the site's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CornerInput {
    /// An edge of a Mixed corner that does not meet the corner's vertex.
    EdgeOffVertex,
    /// A corner edge's or a rail's edge use with no fillet geometry (a cross-section's is
    /// [`FilletSetUp::CrossSectionGeom`]).
    EdgeUseWithoutFilletGeometry,
    /// A corner edge of the pseudo-B-rep with no curve (a rail's is
    /// [`FilletSetUp::RailCurve`], a target-body edge's [`BlendFailure::EdgeWithoutCurve`]).
    EdgeWithoutCurve,
    /// An edge of the body at a chamfer corner's vertex with no vertex at an end.
    EdgeWithoutVertex,
    /// A corner whose event or patches do not hold together: a facet without crossings or a
    /// unit normal, a patch order that does not run round its crossings, or patches and
    /// loops that do not pair up.
    EventMalformed,
    /// A ball, chamfer or Mixed corner with no event of its kind (ball members, chamfer or
    /// Mixed event).
    EventNotSet,
    /// A ring face of a ball corner whose kind or offset is not the one the corner's
    /// classification took.
    FaceNotAsClassified,
    /// A chamfer or Mixed corner with no faces' geometry for its patch step.
    FacesNotBuilt,
    /// A chamfer facet event with no normal.
    FacetNormalNotSet,
    /// A corner patch step given no fillet B-rep.
    FilletBrepNotGiven,
    /// An Nx2 corner of kind `kind` with no filleted edge uses.
    FilletedEdgeUsesNotGiven {
        /// The corner's kind.
        kind: crate::blend::fillet_corner::p3::FilletCornerKind,
    },
    /// An Nx2 corner of kind `kind` with `given` filleted edge uses, not two.
    FilletedEdgeUsesNotTwo {
        /// The corner's kind.
        kind: crate::blend::fillet_corner::p3::FilletCornerKind,
        /// The filleted edge uses it holds.
        given: usize,
    },
    /// A 3x2 corner whose on-edge edge does not start at an on-vertex vertex where the
    /// on-extended-edge edge ends.
    MixedCornerEdgesDoNotMeet,
    /// A 3x2 corner whose on-vertex vertex is not among its vertices.
    MixedCornerVertexNotInCorner,
    /// A 3x2 corner of four edges with no on-edge edge.
    MixedCornerWithoutOnEdgeEdge,
    /// A 3x2 corner of four edges with no on-extended-edge edge.
    MixedCornerWithoutOnExtendedEdgeEdge,
    /// A concave corner with no rail-cross-edge-use vertex between two fillet geometries.
    NoRailXEdgeUseVertex,
    /// A ball corner with no ring of edge uses.
    RingNotSet,
    /// A ball, chamfer or Mixed corner whose vertices or edges are not the count or kind its
    /// topology makes.
    TopologyMismatch,
    /// A vertex of a chamfer or Mixed corner that is not processed.
    VertexNotProcessed,
    /// A vertex of a convex `NxN` corner of at most three vertices that is not where two
    /// rails cross.
    VertexNotRailXRail,
}

impl CornerInput {
    /// The kind's default status: `SM_ERR` where the C++ reports a failed state check
    /// (`SE(SM_ERR)`), else `SM_ERR_NULL_POINTER` for what it dereferences and does not
    /// find (`NER`). A site that reports the condition otherwise overrides it (see
    /// [`CornerInput`]).
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn upstream_status(self) -> i32 {
        match self {
            Self::EdgeOffVertex => SM_ERR,
            Self::EdgeUseWithoutFilletGeometry => SM_ERR_NULL_POINTER,
            Self::EdgeWithoutCurve => SM_ERR_NULL_POINTER,
            Self::EdgeWithoutVertex => SM_ERR_NULL_POINTER,
            Self::EventMalformed => SM_ERR,
            Self::EventNotSet => SM_ERR_NULL_POINTER,
            Self::FaceNotAsClassified => SM_ERR,
            Self::FacesNotBuilt => SM_ERR_NULL_POINTER,
            Self::FacetNormalNotSet => SM_ERR_NULL_POINTER,
            Self::FilletBrepNotGiven => SM_ERR_NULL_POINTER,
            Self::FilletedEdgeUsesNotGiven { .. } => SM_ERR_NULL_POINTER,
            Self::FilletedEdgeUsesNotTwo { .. } => SM_ERR,
            Self::MixedCornerEdgesDoNotMeet => SM_ERR,
            Self::MixedCornerVertexNotInCorner => SM_ERR,
            Self::MixedCornerWithoutOnEdgeEdge => SM_ERR_NULL_POINTER,
            Self::MixedCornerWithoutOnExtendedEdgeEdge => SM_ERR_NULL_POINTER,
            Self::NoRailXEdgeUseVertex => SM_ERR_NULL_POINTER,
            Self::RingNotSet => SM_ERR_NULL_POINTER,
            Self::TopologyMismatch => SM_ERR,
            Self::VertexNotProcessed => SM_ERR,
            Self::VertexNotRailXRail => SM_ERR,
        }
    }
}

/// The step of a ball or chamfer corner's construction that has no result for the corner's
/// geometry ([`BlendFailure::CornerUnbuildable`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CornerStep {
    /// The line where two of the ball's offset planes meet does not meet the third face's
    /// offset quadric as computed: the rounds' ball lies on all three offsets, so the miss
    /// is rounding's (the line tangent to the offset, or in it). Not within tolerance.
    BallCentreLineMissesOffset,
    /// The Krawczyk test did not prove where the three offsets meet, in a box of rounding
    /// of the part's size around the rounds' ball: the faces' normals there are too close
    /// to dependent for the proof at that size. Not within tolerance.
    BallCentreNotProved,
    /// The offsets' meeting point is farther than the tolerance from round 0's ball. Not
    /// within tolerance.
    BallCentreOffRound,
    /// The point solve found no foot of the ball's centre on a round's offset surface.
    /// Not found.
    BallContactFootNotFound,
    /// A round's offset surface at its point solve's foot is farther than the tolerance
    /// from the ball's centre. Not within tolerance.
    BallContactOffOffset,
    /// A ball contact is farther than the tolerance from its corner vertex. Not within
    /// tolerance.
    BallContactOffVertex,
    /// The ball's first three offset planes do not meet in one point (two parallel, or the
    /// third parallel to the first two's line) at the corner's tolerance. Degenerate.
    BallOffsetPlanesDegenerate,
    /// Neither turn of the ball's triangular patch has its normal out of the material at
    /// the test point: the normal there is along the material's boundary. Degenerate.
    BallPatchNormalUndecided,
    /// A corner of the ball's triangular patch is farther than the tolerance from its
    /// vertex. Not within tolerance.
    BallPatchOffVertex,
    /// A ring plane past the third has an offset farther than the tolerance from the
    /// ball's centre. Not within tolerance.
    BallPlaneOffCentre,
    /// A chamfer corner's solved rail crossing is farther than the corner's tolerance from
    /// its exact point. Not within tolerance.
    ChamferCrossingOffExact,
    /// The chamfer facet's plane does not hold a side's line within rounding. Not within
    /// tolerance.
    ChamferFacetOffPlane,
    /// A chamfer's surface does not hold a corner edge's exact line within the corner's
    /// tolerance (no foot of an end on it, or the surface is not affine in its parameters
    /// along the line). Unsupported.
    ChamferSurfaceOffEdgeLine,
    /// The Mixed corner's common face is not planar to the corner's tolerance. Not within
    /// tolerance.
    CommonFaceNotPlanar,
    /// A round's end section has its two offset points farther apart than the tolerance.
    /// Not within tolerance.
    RoundSectionOffsetsApart,
}

impl CornerStep {
    /// The step's class: [`ErrorKind::NotWithinTolerance`] for a solve that ended within
    /// rounding without a result (neither a defect nor a refusal),
    /// [`ErrorKind::Unsupported`] for geometry the construction does not build.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::BallCentreLineMissesOffset => ErrorKind::NotWithinTolerance,
            Self::BallCentreNotProved => ErrorKind::NotWithinTolerance,
            Self::BallCentreOffRound => ErrorKind::NotWithinTolerance,
            Self::BallContactFootNotFound => ErrorKind::NotFound,
            Self::BallContactOffOffset => ErrorKind::NotWithinTolerance,
            Self::BallContactOffVertex => ErrorKind::NotWithinTolerance,
            Self::BallOffsetPlanesDegenerate => ErrorKind::Degenerate,
            Self::BallPatchNormalUndecided => ErrorKind::Degenerate,
            Self::BallPatchOffVertex => ErrorKind::NotWithinTolerance,
            Self::BallPlaneOffCentre => ErrorKind::NotWithinTolerance,
            Self::ChamferCrossingOffExact => ErrorKind::NotWithinTolerance,
            Self::ChamferFacetOffPlane => ErrorKind::NotWithinTolerance,
            Self::ChamferSurfaceOffEdgeLine => ErrorKind::Unsupported,
            Self::CommonFaceNotPlanar => ErrorKind::NotWithinTolerance,
            Self::RoundSectionOffsetsApart => ErrorKind::NotWithinTolerance,
        }
    }

    /// What has no result, as a phrase.
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::BallCentreLineMissesOffset => "the line of two of the ball's offset planes misses the third face's offset within rounding",
            Self::BallCentreNotProved => "the ball's centre was not proved where the three offsets meet, within rounding of the part's size",
            Self::BallCentreOffRound => "the offsets' meeting point is farther than the tolerance from round 0's ball",
            Self::BallContactFootNotFound => "the point solve found no foot of the ball's centre on a round's offset surface",
            Self::BallContactOffOffset => "a round's offset surface at the ball contact's foot is farther than the tolerance from the ball's centre",
            Self::BallContactOffVertex => "a ball contact is farther than the tolerance from its corner vertex",
            Self::BallOffsetPlanesDegenerate => "the ball's first three offset planes do not meet in one point at the corner's tolerance",
            Self::BallPatchNormalUndecided => "neither turn of the ball's triangular patch has its normal out of the material",
            Self::BallPatchOffVertex => "a corner of the ball's triangular patch is farther than the tolerance from its vertex",
            Self::BallPlaneOffCentre => "a ring plane past the third has its offset farther than the tolerance from the ball's centre",
            Self::ChamferCrossingOffExact => "a solved rail crossing is farther than the chamfer corner's tolerance from its exact point",
            Self::ChamferFacetOffPlane => "the chamfer facet's plane does not hold a side's line within rounding",
            Self::ChamferSurfaceOffEdgeLine => "a chamfer's surface does not hold a corner edge's exact line within the corner's tolerance",
            Self::CommonFaceNotPlanar => "the Mixed corner's common face is not planar to the corner's tolerance",
            Self::RoundSectionOffsetsApart => "a round's end section has its offset points farther apart than the tolerance",
        }
    }
}

/// The step of the Mixed corner's construction that has no construction for the corner's
/// geometry ([`BlendFailure::MixedCornerUnbuildable`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MixedStep {
    /// A blend's surface has no B-spline form for the corner's exact algebra.
    /// Unsupported.
    BlendSurfaceNotBSpline,
    /// Three points of the concave blend's edge lie on one line: the edge has no circle
    /// to take the corner's axis from. Degenerate.
    ConcaveEdgeStraight,
    /// A face's patch has a weight that is not positive. Unsupported.
    FaceWeightNotPositive,
    /// The inflection's end tangents do not meet between the rails' crossing and the
    /// vertex. Unsupported.
    InflectionTangentsMiss,
    /// The inflection's end tangents (from the rails' crossing and at the vertex) are
    /// parallel. Degenerate.
    InflectionTangentsParallel,
    /// A curve's C1 reparametrization at a break has singular normal equations.
    /// Degenerate.
    JoinSingular,
    /// A curve's C1 reparametrization at a break needs a span that is not positive.
    /// Unsupported.
    JoinSpanNotPositive,
    /// The corner's shape at the rails' crossing has no asymptotic direction between the
    /// trimlines. Unsupported.
    NoAsymptoticDirection,
    /// The closest-point solve over a rail's interval returned no point for a corner
    /// point (the tree solve's own failures are dropped, so it can end empty). Not found.
    NoClosestPointOnRail,
    /// A corner point has no foot on a wall face, inside the face, to take the face's
    /// normal at. Not found.
    NoFootOnWallFace,
    /// A blend's rail on its wall does not meet the other blend's rail on the common face
    /// swept along the face's normal (the corner's vertex A or B). Not found.
    RailMissesRailWall,
    /// A side's tangent leaves its neighbour's tangent plane at a corner of a face.
    /// Unsupported.
    SideLeavesTangentPlane,
    /// The faces' tangent plane would turn the wrong way along the inflection (the twists'
    /// rate is not positive). Unsupported.
    TangentPlaneTurns,
    /// The first trimline runs along the convex edge at the rails' crossing: the second
    /// trimline's direction has no coordinates in them. Degenerate.
    TrimlineAlongConvexEdge,
    /// A trimline's parameter curve has no B-spline form for the corner's exact algebra.
    /// Unsupported.
    TrimlineNotBSpline,
    /// A trimline has no speed at the rails' crossing: it has no tangent there.
    /// Degenerate.
    TrimlineStalls,
    /// The corner's twists are not compatible. Unsupported.
    TwistsIncompatible,
    /// A blend's rail on its wall does not cross the seam's line on its side of the vertex
    /// (the corner's vertex P or Q). Not found.
    WallRailMissesSeamLine,
}

impl MixedStep {
    /// The step's class: [`ErrorKind::NotFound`] for a crossing or foot not found,
    /// [`ErrorKind::Degenerate`] for a degenerate shape, [`ErrorKind::Unsupported`] for a
    /// shape the construction does not build.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::BlendSurfaceNotBSpline => ErrorKind::Unsupported,
            Self::ConcaveEdgeStraight => ErrorKind::Degenerate,
            Self::FaceWeightNotPositive => ErrorKind::Unsupported,
            Self::InflectionTangentsMiss => ErrorKind::Unsupported,
            Self::InflectionTangentsParallel => ErrorKind::Degenerate,
            Self::JoinSingular => ErrorKind::Degenerate,
            Self::JoinSpanNotPositive => ErrorKind::Unsupported,
            Self::NoAsymptoticDirection => ErrorKind::Unsupported,
            Self::NoClosestPointOnRail => ErrorKind::NotFound,
            Self::NoFootOnWallFace => ErrorKind::NotFound,
            Self::RailMissesRailWall => ErrorKind::NotFound,
            Self::SideLeavesTangentPlane => ErrorKind::Unsupported,
            Self::TangentPlaneTurns => ErrorKind::Unsupported,
            Self::TrimlineAlongConvexEdge => ErrorKind::Degenerate,
            Self::TrimlineNotBSpline => ErrorKind::Unsupported,
            Self::TrimlineStalls => ErrorKind::Degenerate,
            Self::TwistsIncompatible => ErrorKind::Unsupported,
            Self::WallRailMissesSeamLine => ErrorKind::NotFound,
        }
    }

    /// What has no construction, as a phrase ("a rail does not meet ...").
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::BlendSurfaceNotBSpline => "a blend's surface has no B-spline form",
            Self::ConcaveEdgeStraight => "the concave edge has no circle: its points lie on a line",
            Self::FaceWeightNotPositive => "a face's patch has a weight that is not positive",
            Self::InflectionTangentsMiss => "the inflection's end tangents do not meet between the rails' crossing and the vertex",
            Self::InflectionTangentsParallel => "the inflection's end tangents are parallel",
            Self::JoinSingular => "a curve's C1 join at a break has singular equations",
            Self::JoinSpanNotPositive => "a curve's C1 join at a break needs a span that is not positive",
            Self::NoAsymptoticDirection => "the shape at the rails' crossing has no asymptotic direction between the trimlines",
            Self::NoClosestPointOnRail => "the closest-point solve over a rail returned no point for a corner point",
            Self::NoFootOnWallFace => "a corner point has no foot inside a wall face",
            Self::RailMissesRailWall => "a rail on its wall does not meet the other blend's rail swept along the common face's normal",
            Self::SideLeavesTangentPlane => "a side's tangent leaves its neighbour's tangent plane at a face's corner",
            Self::TangentPlaneTurns => "the faces' tangent plane would turn the wrong way along the inflection",
            Self::TrimlineAlongConvexEdge => "the first trimline runs along the convex edge at the rails' crossing",
            Self::TrimlineNotBSpline => "a trimline's parameter curve has no B-spline form",
            Self::TrimlineStalls => "a trimline has no tangent at the rails' crossing",
            Self::TwistsIncompatible => "the corner's twists are not compatible",
            Self::WallRailMissesSeamLine => "a rail on its wall does not cross the seam's line on its side of the vertex",
        }
    }
}

/// What a fillet corner's surface, patch or face step (`crate::blend::fillet_corner`) did not
/// make ([`BlendFailure::CornerPatchNotMade`]). The variable-radius misses stop the corner's
/// conversion to a variable-radius fillet (`MakeVariableRadiusFillet`), which the analytic
/// corner surface reads as no surface made, so the corner gets its patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CornerPatchMiss {
    /// An end of a cross section of the variable-radius corner drops onto its blend's centre
    /// line at other than one point.
    CentreLineFootNotUnique,
    /// A cross section of the variable-radius corner whose ends point the same or opposite
    /// ways from the centre line: its arc has no axis.
    CrossSectionEndsAligned,
    /// A cross section of the variable-radius corner whose ends lie at different distances
    /// from its blend's centre line: not a circular arc about it.
    CrossSectionRadiusVaries,
    /// A cross section of the variable-radius corner whose blend has no centre line (a
    /// section that is not an arc about one).
    CrossSectionWithoutCentreLine,
    /// A corner edge's curve of kind `kind` has no B-spline form.
    CurveNotBSpline {
        /// The curve's kind.
        kind: &'static str,
    },
    /// The mixed corner's extended edge does not drop onto its original face as one curve.
    ExtendedEdgeDropNotOneCurve {
        /// The curves the drop gave.
        curves: usize,
    },
    /// The corner's face could not be made on its surface (its error is kept as the cause
    /// where the step reports one).
    FaceNotMade,
    /// The patch of a mitre's gap (WR-644) does not have four edges.
    GapPatchNotFourEdges {
        /// The edges the gap's patch has.
        edges: usize,
    },
    /// The opposite fillet is a chamfer (a linear generator): no variable-radius law is made
    /// against it.
    LinearOppositeFillet,
    /// A corner of mixed convexity (3x2) that does not have four edges when its patch is made.
    MixedCornerNotFourEdges {
        /// The edges the corner has.
        edges: usize,
    },
    /// No rail meets the interpolating edge's start vertex.
    NoRailAtInterpolationStart,
    /// The edge opposite the variable-radius corner's interpolating edge is not a cross
    /// section.
    OppositeEdgeNotCrossSection,
    /// The opposite edge's ends do not lie across the opposite fillet's parameter domain.
    OppositeEndsNotAcrossFillet,
    /// An end of the opposite edge drops onto the opposite fillet's surface at other than one
    /// point.
    OppositeFootNotUnique,
    /// The opposite fillet's normal vanishes at the opposite edge's end.
    OppositeNormalVanished,
    /// The opposite fillet's radius is not larger than both cross sections' radii.
    OppositeRadiusNotLarger,
    /// The variable-radius fillet's rail that replaces the opposite edge leaves the opposite
    /// fillet's surface.
    RailLeavesOppositeFillet,
    /// A rail at the interpolating edge's vertex that its fillet geometry does not name as
    /// either of its rails.
    RailNotNamedByItsGeometry,
    /// The variable-radius fillet's rail-rail curve crosses the concave corner.
    RailRailCurveCrossesCorner,
    /// The corner's patch has fewer than four boundary curves.
    TooFewBoundaryCurves {
        /// The boundary curves the patch has.
        curves: usize,
    },
    /// The variable-radius corner's two cross sections come from blend-curve generators with
    /// different thumbweights or continuities.
    VariableRadiusBlendCurvesDiffer,
    /// The variable-radius corner's two cross sections come from surface generators of
    /// different kinds.
    VariableRadiusGeneratorsDiffer,
    /// A corner for the variable-radius fillet that does not have four edges.
    VariableRadiusNotFourSides,
    /// The re-solve of the variable-radius fillet failed (its error is kept as the cause).
    VariableRadiusReSolveFailed,
    /// An edge beside the variable-radius corner's interpolating edge that is not a cross
    /// section.
    VariableRadiusSideNotCrossSection,
    /// A corner for the variable-radius fillet with no rail-rail interpolating edge.
    VariableRadiusWithoutInterpolation,
}

impl CornerPatchMiss {
    /// The miss's class: [`ErrorKind::NotFound`] for a search that found no single answer,
    /// [`ErrorKind::Degenerate`] for a vanished direction, [`ErrorKind::Failed`] for a step
    /// that failed, else [`ErrorKind::Unsupported`] (a corner the step does not make).
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::CentreLineFootNotUnique => ErrorKind::NotFound,
            Self::CrossSectionEndsAligned => ErrorKind::Degenerate,
            Self::CrossSectionRadiusVaries => ErrorKind::Unsupported,
            Self::CrossSectionWithoutCentreLine => ErrorKind::Unsupported,
            Self::CurveNotBSpline { .. } => ErrorKind::Unsupported,
            Self::ExtendedEdgeDropNotOneCurve { .. } => ErrorKind::NotFound,
            Self::FaceNotMade => ErrorKind::Failed,
            Self::GapPatchNotFourEdges { .. } => ErrorKind::Unsupported,
            Self::LinearOppositeFillet => ErrorKind::Unsupported,
            Self::MixedCornerNotFourEdges { .. } => ErrorKind::Unsupported,
            Self::NoRailAtInterpolationStart => ErrorKind::NotFound,
            Self::OppositeEdgeNotCrossSection => ErrorKind::Unsupported,
            Self::OppositeEndsNotAcrossFillet => ErrorKind::Unsupported,
            Self::OppositeFootNotUnique => ErrorKind::NotFound,
            Self::OppositeNormalVanished => ErrorKind::Degenerate,
            Self::OppositeRadiusNotLarger => ErrorKind::Unsupported,
            Self::RailLeavesOppositeFillet => ErrorKind::Unsupported,
            Self::RailNotNamedByItsGeometry => ErrorKind::Unsupported,
            Self::RailRailCurveCrossesCorner => ErrorKind::Unsupported,
            Self::TooFewBoundaryCurves { .. } => ErrorKind::Unsupported,
            Self::VariableRadiusBlendCurvesDiffer => ErrorKind::Unsupported,
            Self::VariableRadiusGeneratorsDiffer => ErrorKind::Unsupported,
            Self::VariableRadiusNotFourSides => ErrorKind::Unsupported,
            Self::VariableRadiusReSolveFailed => ErrorKind::Failed,
            Self::VariableRadiusSideNotCrossSection => ErrorKind::Unsupported,
            Self::VariableRadiusWithoutInterpolation => ErrorKind::Unsupported,
        }
    }

    /// The status the site the miss replaces returned unless the site says otherwise:
    /// `SM_ERR_NULL_POINTER` for a centre line the C++ does not find (`NER`),
    /// `SM_ERR_INVALID_INPUT` for a vanished direction (`SER(v.Unitize())`), else `SM_ERR`.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn upstream_status(self) -> i32 {
        match self {
            Self::CentreLineFootNotUnique => SM_ERR,
            Self::CrossSectionEndsAligned => SM_ERR_INVALID_INPUT,
            Self::CrossSectionRadiusVaries => SM_ERR,
            Self::CrossSectionWithoutCentreLine => SM_ERR_NULL_POINTER,
            Self::CurveNotBSpline { .. } => SM_ERR,
            Self::ExtendedEdgeDropNotOneCurve { .. } => SM_ERR,
            Self::FaceNotMade => SM_ERR,
            Self::GapPatchNotFourEdges { .. } => SM_ERR,
            Self::LinearOppositeFillet => SM_ERR,
            Self::MixedCornerNotFourEdges { .. } => SM_ERR,
            Self::NoRailAtInterpolationStart => SM_ERR,
            Self::OppositeEdgeNotCrossSection => SM_ERR,
            Self::OppositeEndsNotAcrossFillet => SM_ERR,
            Self::OppositeFootNotUnique => SM_ERR,
            Self::OppositeNormalVanished => SM_ERR_INVALID_INPUT,
            Self::OppositeRadiusNotLarger => SM_ERR,
            Self::RailLeavesOppositeFillet => SM_ERR,
            Self::RailNotNamedByItsGeometry => SM_ERR,
            Self::RailRailCurveCrossesCorner => SM_ERR,
            Self::TooFewBoundaryCurves { .. } => SM_ERR,
            Self::VariableRadiusBlendCurvesDiffer => SM_ERR,
            Self::VariableRadiusGeneratorsDiffer => SM_ERR,
            Self::VariableRadiusNotFourSides => SM_ERR,
            Self::VariableRadiusReSolveFailed => SM_ERR,
            Self::VariableRadiusSideNotCrossSection => SM_ERR,
            Self::VariableRadiusWithoutInterpolation => SM_ERR,
        }
    }
}
