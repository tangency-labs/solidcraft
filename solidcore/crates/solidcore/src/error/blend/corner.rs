//! Why a fillet corner fails: the shapes of a filleted vertex's edges the fillet corner
//! topology does not build, the corner input a corner step is given otherwise than the
//! topology makes it, and the ball, chamfer, Mixed, Swept turn and End corner steps with no
//! construction.
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
    /// An End whose end face (the face of the side edge the rail missed that is not the
    /// fillet's other face) is a face of the blended edge.
    EndFaceIsBlendFace,
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
            Self::EndFaceIsBlendFace => SM_ERR,
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
    /// A corner edge of the pseudo-B-rep with no vertex, or no vertex use, at an end.
    EdgeEndNotSet,
    /// An edge at a squeezed corner edge's deleted vertex, not a rail, that is not among the
    /// corner's edges.
    EdgeNotInCorner,
    /// An edge of a Mixed corner that does not meet the corner's vertex.
    EdgeOffVertex,
    /// A corner edge's or a rail's edge use with no fillet geometry (a cross-section's is
    /// [`FilletSetUp::CrossSectionGeom`]).
    EdgeUseWithoutFilletGeometry,
    /// A corner edge of the pseudo-B-rep with no curve (a rail's is
    /// [`FilletSetUp::RailCurve`], a target-body edge's [`BlendFailure::EdgeWithoutCurve`]).
    EdgeWithoutCurve,
    /// An edge of the body at a chamfer or setback corner's vertex, or an End's side edge,
    /// with no vertex at an end.
    EdgeWithoutVertex,
    /// A corner whose event or patches do not hold together: a facet without crossings or a
    /// unit normal, a patch order that does not run round its crossings, patches and
    /// loops that do not pair up, or a setback corner whose setbacks, least setbacks, room or fill
    /// patches are not one per edge (two per edge for the patches).
    EventMalformed,
    /// A ball, chamfer, Mixed or setback corner, a Swept turn or an End with no event of its
    /// kind (ball members, chamfer, Mixed, setback or Swept event, or an End's cap).
    EventNotSet,
    /// A ring face of a ball corner whose kind or offset is not the one the corner's
    /// classification took, or a setback corner whose faces were not classified.
    FaceNotAsClassified,
    /// A chamfer or Mixed corner with no faces' geometry for its patch step, a setback corner
    /// with no fill for it, or a Swept turn with no torus for it.
    FacesNotBuilt,
    /// A chamfer facet event with no normal.
    FacetNormalNotSet,
    /// A fill given to a setback containment bound that is not one the setback split
    /// builds: two one-span patches per round with positive weights, and a round and a face
    /// per edge.
    FillNotFromSetbackSplit,
    /// A corner patch step given no fillet B-rep.
    FilletBrepNotGiven,
    /// A corner of kind `kind` with no filleted edge uses (an Nx2 corner's two, an end
    /// corner's one).
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
    /// An end corner whose vertices hold none but the end rib's vertex whose rail missed its
    /// side edge.
    NoOtherCornerVertex,
    /// A concave corner with no rail-cross-edge-use vertex between two fillet geometries.
    NoRailXEdgeUseVertex,
    /// A ring edge use of a ball or setback corner whose edge (or its mate's) has no fillet
    /// solver.
    RingEdgeUseWithoutSolver,
    /// A ball or setback corner or an End with no ring of edge uses round its vertex (a
    /// setback corner's: all of them, or those of its solvers).
    RingNotSet,
    /// A round given to the setback fill whose surface has degree 0 across its rails (a
    /// B-spline surface's degrees are not checked when it is made from data).
    RoundOfDegreeZeroAcross,
    /// A Roll-over's seam cap whose corner edges do not close one loop through the seam's
    /// split vertex.
    SeamCapLoopOpen,
    /// A squeezed corner edge neither of whose vertices is to be deleted.
    SqueezedEdgeKeepsBothVertices,
    /// A ball, chamfer, Mixed or setback corner, a Swept turn or an End whose vertices or
    /// edges are not the count or kind its topology makes (an End without its end rib, or
    /// whose vertex's edges do not hold its blended edge's two others).
    TopologyMismatch,
    /// A Swept turn whose vertex has no edge use besides its two rounds' (none to find the
    /// side edge from).
    TurnRingWithoutSideEdgeUse,
    /// A Swept turn whose two rounds' edge uses share no face of the body with its side edge
    /// use.
    TurnRoundsShareNoFace,
    /// A Swept turn given `given` blended edge uses, fewer than its two rounds'.
    TurnWithoutTwoBlendedEdgeUses {
        /// The blended edge uses it holds.
        given: usize,
    },
    /// A squeezed corner edge's deleted vertex that is not among the corner's vertices.
    VertexNotInCorner,
    /// A vertex of a chamfer, Mixed or setback corner, of a Swept turn or of an End's cap
    /// that is not processed.
    VertexNotProcessed,
    /// A vertex of a convex `NxN` corner of at most three vertices that is not where two
    /// rails cross.
    VertexNotRailXRail,
    /// A rail-end fillet vertex use whose intersection point holds no edge parameter.
    VertexUseWithoutEdgeParameter,
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
            Self::EdgeEndNotSet => SM_ERR_NULL_POINTER,
            Self::EdgeNotInCorner => SM_ERR,
            Self::EdgeOffVertex => SM_ERR,
            Self::EdgeUseWithoutFilletGeometry => SM_ERR_NULL_POINTER,
            Self::EdgeWithoutCurve => SM_ERR_NULL_POINTER,
            Self::EdgeWithoutVertex => SM_ERR_NULL_POINTER,
            Self::EventMalformed => SM_ERR,
            Self::EventNotSet => SM_ERR_NULL_POINTER,
            Self::FaceNotAsClassified => SM_ERR,
            Self::FacesNotBuilt => SM_ERR_NULL_POINTER,
            Self::FacetNormalNotSet => SM_ERR_NULL_POINTER,
            Self::FillNotFromSetbackSplit => SM_ERR,
            Self::FilletBrepNotGiven => SM_ERR_NULL_POINTER,
            Self::FilletedEdgeUsesNotGiven { .. } => SM_ERR_NULL_POINTER,
            Self::FilletedEdgeUsesNotTwo { .. } => SM_ERR,
            Self::MixedCornerEdgesDoNotMeet => SM_ERR,
            Self::MixedCornerVertexNotInCorner => SM_ERR,
            Self::MixedCornerWithoutOnEdgeEdge => SM_ERR_NULL_POINTER,
            Self::MixedCornerWithoutOnExtendedEdgeEdge => SM_ERR_NULL_POINTER,
            Self::NoOtherCornerVertex => SM_ERR_NULL_POINTER,
            Self::NoRailXEdgeUseVertex => SM_ERR_NULL_POINTER,
            Self::RingEdgeUseWithoutSolver => SM_ERR_NULL_POINTER,
            Self::RingNotSet => SM_ERR_NULL_POINTER,
            Self::RoundOfDegreeZeroAcross => SM_ERR,
            Self::SeamCapLoopOpen => SM_ERR,
            Self::SqueezedEdgeKeepsBothVertices => SM_ERR,
            Self::TopologyMismatch => SM_ERR,
            Self::TurnRingWithoutSideEdgeUse => SM_ERR_NULL_POINTER,
            Self::TurnRoundsShareNoFace => SM_ERR_NULL_POINTER,
            Self::TurnWithoutTwoBlendedEdgeUses { .. } => SM_ERR_NULL_POINTER,
            Self::VertexNotInCorner => SM_ERR,
            Self::VertexNotProcessed => SM_ERR,
            Self::VertexNotRailXRail => SM_ERR,
            Self::VertexUseWithoutEdgeParameter => SM_ERR_NULL_POINTER,
        }
    }
}

/// The step of a ball, chamfer or setback corner's, a Swept turn's or a blend end's
/// construction that has no result for the corner's geometry
/// ([`BlendFailure::CornerUnbuildable`]).
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
    /// A centre of the ball an End's rail is sized by does not drop onto the fillet's
    /// other face. Not found.
    EndCentreOffOtherFace,
    /// The side edge's far end does not drop onto the base surface of the fillet's offset
    /// on the side face, where the End's ball is sized. Not found.
    EndPointOffBaseSurface,
    /// The ball solve finds that the End's rail at the round's radius does not run past
    /// its side edge's far end, though the rail's end was not found on the side edge: the
    /// two tests differ within rounding. Not within tolerance.
    EndRailNotPastSideEdge,
    /// The End's side edge, walked back across seams' vertices from where the rail's end
    /// was last looked for, has no continuation towards the corner's vertex. Not found.
    EndSideEdgeChainBroken,
    /// A side face of an End at a smooth vertex does not meet the cap's plane between the
    /// cap edge's ends. Not found.
    EndSideFaceMissesCap,
    /// An open setback corner's two rails meet the fill's end section along parallel
    /// directions, so its springs have no crossing. Degenerate.
    OpenSetbackRailsParallel,
    /// A setback corner's round has its rail square to its edge at the end section (or the
    /// two directions' product is not finite), so the corner's side of the section is
    /// undecided. Degenerate.
    RailSquareToEdge,
    /// A setback fill's round is not one Bezier span along its rails beside its end
    /// section once its knots there are raised (a span of no length, or rows past its
    /// net). Unsupported.
    RoundEndSpanNotBezier,
    /// A round's end section has its two offset points farther apart than the tolerance.
    /// Not within tolerance.
    RoundSectionOffsetsApart,
    /// A setback fill's round has a fillet surface with no B-spline form to take its end
    /// section from. Unsupported.
    RoundSurfaceNotBSpline,
    /// A setback fill's normal is not certified regular and to one side of its rounds'
    /// (the fill may fold). Unsupported.
    SetbackFillMayFold,
    /// A G2 setback fill whose every interior map up to degree 9 that meets its setback
    /// patches' conditions leaves a patch whose control net turns back. Unsupported.
    SetbackG2NetTurnsBack,
    /// A setback corner grown beside a curved face does not place its rounds' ends again at
    /// the scale its growth certified (the setback vertices placed afresh from the trials'
    /// last positions). Not within tolerance.
    SetbackGrowthUnplaced,
    /// No interior map of a setback fill up to degree 9 meets its setback patches'
    /// conditions. Not found.
    SetbackNoInteriorMap,
    /// A setback patch of a fill does not meet the fill's interior within the tolerance.
    /// Not within tolerance.
    SetbackPatchMissesInterior,
    /// A patch of a setback fill has a weight that is not positive. Degenerate.
    SetbackPatchWeightNotPositive,
    /// A setback fill's spring meets the side of its round farther than the tolerance from
    /// the end of the rail it springs from. Not within tolerance.
    SetbackSpringSideOffRailEnd,
    /// No face of the side edge an End or a Join sizes its ball on, besides the fillet's
    /// other face, is one of the blend solver's faces: no fillet surface lies on the side
    /// face. Not found.
    SideEdgeOffSolverFaces,
    /// The arc of the Swept turn's torus was not found among the torus's boundary curves
    /// for the corner's arc edge. Not found.
    SweptArcNotFound,
    /// The Swept turn's two balls (at its plate vertices) have radii farther apart than the
    /// tolerance, though its classification took them to be equal. Not within tolerance.
    SweptBallRadiiDiffer,
    /// The Swept turn's two ball centres are at distances from the side edge's axis farther
    /// apart than the tolerance. Not within tolerance.
    SweptCentreAxisDistancesDiffer,
    /// A Swept turn's ball centre is within the tolerance of the side edge's axis: the torus
    /// has no major radius. Degenerate.
    SweptCentreOnAxis,
    /// The feet of the Swept turn's two ball centres on the side edge's axis are farther
    /// apart than the tolerance. Not within tolerance.
    SweptFeetApart,
    /// The Swept turn's trimmed torus does not have three boundary curves (its two ends and
    /// the arc). Unsupported.
    SweptLoopNotThreeCurves,
    /// The Swept turn's ball centres do not turn about the side edge's axis (the sine of
    /// the angle between them is not above zero). Degenerate.
    SweptNoTurn,
    /// The Swept turn's side edge is not a line within the tolerance. Unsupported.
    SweptSideEdgeNotLine,
    /// A corner of the Swept turn's torus is farther than the tolerance from its vertex.
    /// Not within tolerance.
    SweptTorusOffVertex,
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
            Self::EndCentreOffOtherFace => ErrorKind::NotFound,
            Self::EndPointOffBaseSurface => ErrorKind::NotFound,
            Self::EndRailNotPastSideEdge => ErrorKind::NotWithinTolerance,
            Self::EndSideEdgeChainBroken => ErrorKind::NotFound,
            Self::EndSideFaceMissesCap => ErrorKind::NotFound,
            Self::OpenSetbackRailsParallel => ErrorKind::Degenerate,
            Self::RailSquareToEdge => ErrorKind::Degenerate,
            Self::RoundEndSpanNotBezier => ErrorKind::Unsupported,
            Self::RoundSectionOffsetsApart => ErrorKind::NotWithinTolerance,
            Self::RoundSurfaceNotBSpline => ErrorKind::Unsupported,
            Self::SetbackFillMayFold => ErrorKind::Unsupported,
            Self::SetbackG2NetTurnsBack => ErrorKind::Unsupported,
            Self::SetbackGrowthUnplaced => ErrorKind::NotWithinTolerance,
            Self::SetbackNoInteriorMap => ErrorKind::NotFound,
            Self::SetbackPatchMissesInterior => ErrorKind::NotWithinTolerance,
            Self::SetbackPatchWeightNotPositive => ErrorKind::Degenerate,
            Self::SetbackSpringSideOffRailEnd => ErrorKind::NotWithinTolerance,
            Self::SideEdgeOffSolverFaces => ErrorKind::NotFound,
            Self::SweptArcNotFound => ErrorKind::NotFound,
            Self::SweptBallRadiiDiffer => ErrorKind::NotWithinTolerance,
            Self::SweptCentreAxisDistancesDiffer => ErrorKind::NotWithinTolerance,
            Self::SweptCentreOnAxis => ErrorKind::Degenerate,
            Self::SweptFeetApart => ErrorKind::NotWithinTolerance,
            Self::SweptLoopNotThreeCurves => ErrorKind::Unsupported,
            Self::SweptNoTurn => ErrorKind::Degenerate,
            Self::SweptSideEdgeNotLine => ErrorKind::Unsupported,
            Self::SweptTorusOffVertex => ErrorKind::NotWithinTolerance,
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
            Self::EndCentreOffOtherFace => "a centre of the End's sizing ball does not drop onto the fillet's other face",
            Self::EndPointOffBaseSurface => "the side edge's far end does not drop onto the fillet's base surface on the side face",
            Self::EndRailNotPastSideEdge => "the End's rail at the round's radius does not run past its side edge's far end",
            Self::EndSideEdgeChainBroken => "the End's side edge chain has no continuation towards the corner's vertex",
            Self::EndSideFaceMissesCap => "a side face of the End does not meet the cap's plane between the cap edge's ends",
            Self::OpenSetbackRailsParallel => "an open setback corner's rails meet the fill's end section along parallel directions",
            Self::RailSquareToEdge => "a setback round's rail is square to its edge at the end section",
            Self::RoundEndSpanNotBezier => "a setback fill's round is not one Bezier span along its rails beside its end section",
            Self::RoundSectionOffsetsApart => "a round's end section has its offset points farther apart than the tolerance",
            Self::RoundSurfaceNotBSpline => "a setback fill's round has a fillet surface with no B-spline form",
            Self::SetbackFillMayFold => "a setback fill's normal is not certified regular and to one side of its rounds' (the fill may fold)",
            Self::SetbackG2NetTurnsBack => "every G2 setback fill up to degree 9 has a patch whose control net turns back",
            Self::SetbackGrowthUnplaced => "a setback corner grown beside a curved face does not place its rounds' ends at the scale it certified",
            Self::SetbackNoInteriorMap => "no interior map of a setback fill up to degree 9 meets its setback patches' conditions",
            Self::SetbackPatchMissesInterior => "a setback patch does not meet the fill's interior within the tolerance",
            Self::SetbackPatchWeightNotPositive => "a patch of a setback fill has a weight that is not positive",
            Self::SetbackSpringSideOffRailEnd => "a setback fill's spring meets its round's side farther than the tolerance from the rail's end",
            Self::SideEdgeOffSolverFaces => "no fillet surface lies on the face of the side edge the ball is sized on",
            Self::SweptArcNotFound => "the Swept turn's torus has no boundary curve for the corner's arc edge",
            Self::SweptBallRadiiDiffer => "the Swept turn's two balls have radii farther apart than the tolerance",
            Self::SweptCentreAxisDistancesDiffer => "the Swept turn's ball centres are at distances from the side edge's axis farther apart than the tolerance",
            Self::SweptCentreOnAxis => "a Swept turn's ball centre is within the tolerance of the side edge's axis",
            Self::SweptFeetApart => "the feet of the Swept turn's ball centres on the side edge's axis are farther apart than the tolerance",
            Self::SweptLoopNotThreeCurves => "the Swept turn's trimmed torus does not have three boundary curves",
            Self::SweptNoTurn => "the Swept turn's ball centres do not turn about the side edge's axis",
            Self::SweptSideEdgeNotLine => "the Swept turn's side edge is not a line within the tolerance",
            Self::SweptTorusOffVertex => "a corner of the Swept turn's torus is farther than the tolerance from its vertex",
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

/// What a fillet corner's geometry, surface, patch or face step
/// (`crate::blend::fillet_corner`) did not make ([`BlendFailure::CornerPatchNotMade`]). The
/// variable-radius misses stop the corner's conversion to a variable-radius fillet
/// (`MakeVariableRadiusFillet`), which the analytic corner surface reads as no surface made,
/// so the corner gets its patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CornerPatchMiss {
    /// An end of a cross section of the variable-radius corner drops onto its blend's centre
    /// line at other than one point.
    CentreLineFootNotUnique,
    /// The constant-radius corner's shortest edge is not a circular arc.
    ConstantRadiusBaseNotArc,
    /// The constant-radius corner's two sides come from blend-curve generators with
    /// different thumbweights or continuities.
    ConstantRadiusBlendCurvesDiffer,
    /// A point of the constant-radius corner (an end of its shortest edge, or the vertex
    /// opposite it) drops onto a side's fillet surface at `solutions` points, not one.
    ConstantRadiusFootNotUnique {
        /// The points the drop found.
        solutions: usize,
    },
    /// The constant-radius corner's two sides come from surface generators of different
    /// kinds.
    ConstantRadiusGeneratorsDiffer,
    /// The constant-radius corner's sides come from linear (chamfer) generators: no
    /// constant-radius fillet is made between them.
    ConstantRadiusLinearGenerator,
    /// A corner for the constant-radius fillet that does not have three edges.
    ConstantRadiusNotThreeSides,
    /// The re-solve of the constant-radius fillet failed (its error is kept as the cause).
    ConstantRadiusReSolveFailed,
    /// An edge of the constant-radius corner that is not a cross section.
    ConstantRadiusSideNotCrossSection,
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
    /// A corner edge's geometry could not be calculated, and the edge cannot be left to the
    /// global merge (its calculator's error is kept as the cause).
    EdgeGeometryNotCalculated,
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
    /// The point that orients a corner face's loop does not drop onto the face's surface.
    OrientationPointOffSurface,
    /// A planar corner patch given `points` points, not three or four.
    PlanarPatchPointCount {
        /// The points given.
        points: usize,
    },
    /// The variable-radius fillet's rail that replaces the opposite edge leaves the opposite
    /// fillet's surface.
    RailLeavesOppositeFillet,
    /// A rail at the interpolating edge's vertex that its fillet geometry does not name as
    /// either of its rails.
    RailNotNamedByItsGeometry,
    /// The variable-radius fillet's rail-rail curve crosses the concave corner.
    RailRailCurveCrossesCorner,
    /// The corner's vertex drops onto a trial rail-rail curve at `solutions` points, not one.
    RailRailCurveFootNotUnique {
        /// The points the drop found.
        solutions: usize,
    },
    /// The four-sided corner's major and opposite arcs do not share an axis, or their angle
    /// spans differ by more than 2 degrees: no one surface of revolution.
    RevolutionArcsDiffer,
    /// The surface of revolution's major curve is not a circular arc.
    RevolutionMajorNotArc,
    /// The surface of revolution does not meet the curve after its major arc within the
    /// tolerance.
    RevolutionMissesOtherCurve,
    /// The four-sided corner's curve opposite the major arc is not a circular arc.
    RevolutionOppositeNotArc,
    /// The loop of a part of a Roll-over's seam cap has no Newell normal: it encloses no
    /// area.
    SeamCapPartNormalVanished,
    /// A Roll-over's seam point does not drop onto the inside of any side edge of the seam
    /// cap within the corner's tolerance.
    SeamPointOffSideEdges,
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
    /// A corner vertex's geometry could not be calculated (its calculator's error is kept as
    /// the cause).
    VertexGeometryNotCalculated,
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
            Self::ConstantRadiusBaseNotArc => ErrorKind::Unsupported,
            Self::ConstantRadiusBlendCurvesDiffer => ErrorKind::Unsupported,
            Self::ConstantRadiusFootNotUnique { .. } => ErrorKind::NotFound,
            Self::ConstantRadiusGeneratorsDiffer => ErrorKind::Unsupported,
            Self::ConstantRadiusLinearGenerator => ErrorKind::Unsupported,
            Self::ConstantRadiusNotThreeSides => ErrorKind::Unsupported,
            Self::ConstantRadiusReSolveFailed => ErrorKind::Failed,
            Self::ConstantRadiusSideNotCrossSection => ErrorKind::Unsupported,
            Self::CrossSectionEndsAligned => ErrorKind::Degenerate,
            Self::CrossSectionRadiusVaries => ErrorKind::Unsupported,
            Self::CrossSectionWithoutCentreLine => ErrorKind::Unsupported,
            Self::CurveNotBSpline { .. } => ErrorKind::Unsupported,
            Self::EdgeGeometryNotCalculated => ErrorKind::Failed,
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
            Self::OrientationPointOffSurface => ErrorKind::NotFound,
            Self::PlanarPatchPointCount { .. } => ErrorKind::Unsupported,
            Self::RailLeavesOppositeFillet => ErrorKind::Unsupported,
            Self::RailNotNamedByItsGeometry => ErrorKind::Unsupported,
            Self::RailRailCurveCrossesCorner => ErrorKind::Unsupported,
            Self::RailRailCurveFootNotUnique { .. } => ErrorKind::NotFound,
            Self::RevolutionArcsDiffer => ErrorKind::Unsupported,
            Self::RevolutionMajorNotArc => ErrorKind::Unsupported,
            Self::RevolutionMissesOtherCurve => ErrorKind::Unsupported,
            Self::RevolutionOppositeNotArc => ErrorKind::Unsupported,
            Self::SeamCapPartNormalVanished => ErrorKind::Degenerate,
            Self::SeamPointOffSideEdges => ErrorKind::NotFound,
            Self::TooFewBoundaryCurves { .. } => ErrorKind::Unsupported,
            Self::VariableRadiusBlendCurvesDiffer => ErrorKind::Unsupported,
            Self::VariableRadiusGeneratorsDiffer => ErrorKind::Unsupported,
            Self::VariableRadiusNotFourSides => ErrorKind::Unsupported,
            Self::VariableRadiusReSolveFailed => ErrorKind::Failed,
            Self::VariableRadiusSideNotCrossSection => ErrorKind::Unsupported,
            Self::VariableRadiusWithoutInterpolation => ErrorKind::Unsupported,
            Self::VertexGeometryNotCalculated => ErrorKind::Failed,
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
            Self::ConstantRadiusBaseNotArc => SM_ERR,
            Self::ConstantRadiusBlendCurvesDiffer => SM_ERR,
            Self::ConstantRadiusFootNotUnique { .. } => SM_ERR,
            Self::ConstantRadiusGeneratorsDiffer => SM_ERR,
            Self::ConstantRadiusLinearGenerator => SM_ERR,
            Self::ConstantRadiusNotThreeSides => SM_ERR,
            Self::ConstantRadiusReSolveFailed => SM_ERR,
            Self::ConstantRadiusSideNotCrossSection => SM_ERR,
            Self::CrossSectionEndsAligned => SM_ERR_INVALID_INPUT,
            Self::CrossSectionRadiusVaries => SM_ERR,
            Self::CrossSectionWithoutCentreLine => SM_ERR_NULL_POINTER,
            Self::CurveNotBSpline { .. } => SM_ERR,
            Self::EdgeGeometryNotCalculated => SM_ERR,
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
            Self::OrientationPointOffSurface => SM_ERR,
            Self::PlanarPatchPointCount { .. } => SM_ERR,
            Self::RailLeavesOppositeFillet => SM_ERR,
            Self::RailNotNamedByItsGeometry => SM_ERR,
            Self::RailRailCurveCrossesCorner => SM_ERR,
            Self::RailRailCurveFootNotUnique { .. } => SM_ERR,
            Self::RevolutionArcsDiffer => SM_ERR,
            Self::RevolutionMajorNotArc => SM_ERR,
            Self::RevolutionMissesOtherCurve => SM_ERR,
            Self::RevolutionOppositeNotArc => SM_ERR,
            Self::SeamCapPartNormalVanished => SM_ERR,
            Self::SeamPointOffSideEdges => SM_ERR,
            Self::TooFewBoundaryCurves { .. } => SM_ERR,
            Self::VariableRadiusBlendCurvesDiffer => SM_ERR,
            Self::VariableRadiusGeneratorsDiffer => SM_ERR,
            Self::VariableRadiusNotFourSides => SM_ERR,
            Self::VariableRadiusReSolveFailed => SM_ERR,
            Self::VariableRadiusSideNotCrossSection => SM_ERR,
            Self::VariableRadiusWithoutInterpolation => SM_ERR,
            Self::VertexGeometryNotCalculated => SM_ERR,
        }
    }
}
