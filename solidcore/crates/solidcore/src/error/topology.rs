//! Why a topology routine fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the topology subsystem (`crate::topology`:
//! the B-rep constructor, cache and data streams, faces, edges, edge uses, loop uses,
//! vertex uses, shells, trimming tools, cutting, mirroring, planar sections, vertex fans,
//! projection, traversal and the file wrappers). Each also records, for difftest only,
//! the status the C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                 | Status difftest compares      |
//! |--------------------------------------|-------------------------------|
//! | [`TopologyFailure::AbsorbedCurveAtNoFiniteDistance`] | `SM_ERR` |
//! | [`TopologyFailure::AllocationFailed`] | `SM_ERR_OUT_OF_MEMORY` |
//! | [`TopologyFailure::AttributeSlotEmpty`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::BinormalStaysZero`] | `SM_ERR` |
//! | [`TopologyFailure::BlendRecordNotProved`] | `SM_ERR` |
//! | [`TopologyFailure::BrepSideNotOneOrTwo`] | `SM_ERR` |
//! | [`TopologyFailure::CacheTargetNotBrep`] | `SM_ERR` |
//! | [`TopologyFailure::ClassificationOffTopology`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::ClassificationParametersUnset`] | `SM_ERR` |
//! | [`TopologyFailure::ClosedEdge`] | `SM_ERR` |
//! | [`TopologyFailure::ConstructorNotOpen`] | `SM_ERR` |
//! | [`TopologyFailure::ConstructorStillOpen`] | `SM_ERR` |
//! | [`TopologyFailure::CurveEndWithoutTangent`] | `SM_ERR` |
//! | [`TopologyFailure::CurveIndexOutOfRange`] | `SM_ERR` |
//! | [`TopologyFailure::CurveLoopsInvalid`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::CurveNotApproximated`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::CurveNotBSpline`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::CurveNotInLoops`] | `SM_ERR` |
//! | [`TopologyFailure::CurveOwnerUnresolved`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::CurveSlotEmpty`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::CurveStoreFull`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::CurvesNotAdjacent`] | `SM_ERR` |
//! | [`TopologyFailure::CurvesNotInOneLoop`] | `SM_ERR` |
//! | [`TopologyFailure::CurvesNotJoined`] | `SM_ERR` |
//! | [`TopologyFailure::Defect`]              | `SM_ERR` (or the site's, through `with_status`) |
//! | [`TopologyFailure::DefinitionWithoutSourceTopology`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::DegenerateEdgeInterval`] | `SM_ERR` |
//! | [`TopologyFailure::DegenerateUvDomain`] | `SM_ERR` |
//! | [`TopologyFailure::DropFailed`]      | the drop's own recorded status |
//! | [`TopologyFailure::EdgeCurveAlreadySet`] | `SM_ERR` |
//! | [`TopologyFailure::EdgeCurveNotJoinable`] | `SM_ERR` |
//! | [`TopologyFailure::EdgeNotDeletable`] | `SM_ERR` |
//! | [`TopologyFailure::EdgeNotOnVertex`] | `SM_ERR` |
//! | [`TopologyFailure::EdgeNotSplit`] | `SM_ERR` |
//! | [`TopologyFailure::EdgeUseNotClassified`] | `SM_ERR` |
//! | [`TopologyFailure::EdgeUseNotInLoop`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::EdgeUseNotOnFace`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::EdgeUseNotOnLoopUse`] | `SM_ERR` |
//! | [`TopologyFailure::EdgeWithoutCurve`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::EdgeWithoutEdgeuses`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::EdgesNotChained`] | `SM_ERR` |
//! | [`TopologyFailure::EdgesNotSharingVertices`] | `SM_ERR` |
//! | [`TopologyFailure::EdgesOnDifferentBreps`] | `SM_ERR_ASSERT_FAILURE` |
//! | [`TopologyFailure::EditingDisabled`] | `SM_ERR` |
//! | [`TopologyFailure::FaceListedTwice`] | `SM_ERR` |
//! | [`TopologyFailure::FaceNotMade`] | `SM_ERR` |
//! | [`TopologyFailure::FaceStitchFailed`] | `SM_ERR` |
//! | [`TopologyFailure::FaceUseNotInShell`] | `SM_ERR` |
//! | [`TopologyFailure::FaceUseWithoutShell`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::FaceWithoutEdges`] | `SM_ERR` |
//! | [`TopologyFailure::FaceWithoutLoops`] | `SM_ERR` |
//! | [`TopologyFailure::FaceWithoutSurface`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::FaceWithoutUses`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::FacesNotCopiedOnce`] | `SM_ERR` |
//! | [`TopologyFailure::Failed`]          | `SM_ERR`                      |
//! | [`TopologyFailure::GatheredBothSides`] | `SM_ERR_ASSERT_FAILURE` |
//! | [`TopologyFailure::Inconsistent`]    | `SM_ERR_ASSERT_FAILURE`       |
//! | [`TopologyFailure::InfiniteRegionNotFound`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::InnerLoopsOnBothSides`] | `SM_ERR` |
//! | [`TopologyFailure::IntersectingProtoEdgeEmpty`] | `SM_ERR` |
//! | [`TopologyFailure::InvalidValue`]    | `SM_ERR_INVALID_INPUT`        |
//! | [`TopologyFailure::IsoLineNotMade`] | `SM_ERR` |
//! | [`TopologyFailure::JoinedVertexReaches`] | `SM_ERR` (not upstream, #15718) |
//! | [`TopologyFailure::LaminaEdgeIsStrut`] | `SM_ERR` |
//! | [`TopologyFailure::LoopEdgeNotLamina`] | `SM_ERR` |
//! | [`TopologyFailure::LoopIndexOutOfRange`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::LoopIsOuter`] | `SM_ERR` |
//! | [`TopologyFailure::LoopJumpsInUv`] | `SM_ERR` |
//! | [`TopologyFailure::LoopMatesDisagree`] | `SM_ERR` |
//! | [`TopologyFailure::LoopNotOnFace`] | `SM_ERR` |
//! | [`TopologyFailure::LoopOrientationUndecided`] | `SM_ERR` |
//! | [`TopologyFailure::LoopRecordWithoutEdgeuse`] | `SM_ERR` |
//! | [`TopologyFailure::LoopSplitFaceEarly`] | `SM_ERR` |
//! | [`TopologyFailure::LoopSplitNoFace`] | `SM_ERR` |
//! | [`TopologyFailure::LoopTooLong`] | `SM_ERR` |
//! | [`TopologyFailure::LoopWithoutNormal`] | `SM_ERR` |
//! | [`TopologyFailure::LoopsMisoriented`] | `SM_ERR` |
//! | [`TopologyFailure::LoopsNotClosedOrPlanar`] | `SM_ERR` |
//! | [`TopologyFailure::MergeGaveNoVertex`] | `SM_ERR` |
//! | [`TopologyFailure::Missing`]         | `SM_ERR_NULL_POINTER`         |
//! | [`TopologyFailure::NewPrimaryNotOnEdge`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::NoBrepGiven`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::NoClassifiablePoint`] | `SM_ERR` |
//! | [`TopologyFailure::NoCurveGiven`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::NoCurvesAskedFor`] | `SM_ERR` |
//! | [`TopologyFailure::NoDataGiven`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::NoEdgeToAimAt`] | `SM_ERR` |
//! | [`TopologyFailure::NoFaceGiven`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::NoFaceUses`] | `SM_ERR` |
//! | [`TopologyFailure::NoFinalVertexAtEnd`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::NoFiniteRegion`] | `SM_ERR` |
//! | [`TopologyFailure::NoFootOnCurve`] | `SM_ERR` |
//! | [`TopologyFailure::NoFootOnSeam`] | `SM_ERR` |
//! | [`TopologyFailure::NoFootOnSurface`] | `SM_ERR` |
//! | [`TopologyFailure::NoInfiniteRegion`] | `SM_ERR` |
//! | [`TopologyFailure::NoInputStream`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::NoInteriorPoint`] | `SM_ERR` |
//! | [`TopologyFailure::NoIsoLineCrossesFace`] | `SM_ERR` |
//! | [`TopologyFailure::NoLoops`] | `SM_ERR` |
//! | [`TopologyFailure::NoPlaneProjection`] | `SM_ERR` |
//! | [`TopologyFailure::NoPlaneThroughCurves`] | `SM_ERR` |
//! | [`TopologyFailure::NoProjectedCurves`] | `SM_ERR` |
//! | [`TopologyFailure::NoRadialCandidate`] | `SM_ERR` |
//! | [`TopologyFailure::NoRadialSectorBetween`] | `SM_ERR` |
//! | [`TopologyFailure::NoRegionGiven`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::NoRegionsToKeepGiven`] | `SM_ERR` |
//! | [`TopologyFailure::NoSampleOnSurface`] | `SM_ERR` |
//! | [`TopologyFailure::NoSectorForNewEdgeuse`] | `SM_ERR` |
//! | [`TopologyFailure::NoStartEdgeUse`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::NoStepInsideInterval`] | `SM_ERR` |
//! | [`TopologyFailure::NoSurfaceGiven`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::NoTrimmedBSpline`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::NoVertexGiven`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::NoVertexSector`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::NotAMatedPair`] | `SM_ERR` |
//! | [`TopologyFailure::NotASheet`] | `SM_ERR` |
//! | [`TopologyFailure::NotAnEdgeVertexUse`] | `SM_ERR` |
//! | [`TopologyFailure::NotFourEdgeUses`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::NotInBrep`] | `SM_ERR` |
//! | [`TopologyFailure::NotOnOwnerList`] | `SM_ERR` |
//! | [`TopologyFailure::NothingToLink`] | `SM_ERR` |
//! | [`TopologyFailure::OldEdgeNotAmongSplit`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::OutOfMemory`]     | `SM_ERR_OUT_OF_MEMORY`        |
//! | [`TopologyFailure::OverlapBelowSplitResolution`] | `SM_ERR` |
//! | [`TopologyFailure::OwnerListBroken`] | `SM_ERR` |
//! | [`TopologyFailure::OwnerListOverCount`] | `SM_ERR` |
//! | [`TopologyFailure::PartNotEmpty`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::ProtoEdgeWithoutDefinitions`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::ProtoVertexNotAnEnd`] | `SM_ERR` |
//! | [`TopologyFailure::RadialCapWithoutPlane`] | `SM_ERR` |
//! | [`TopologyFailure::RadialFaceWithoutSurface`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::RadialRimWithoutEdge`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::RadialRimsNotAnnulus`] | `SM_ERR` |
//! | [`TopologyFailure::RadialRimsNotEllipses`] | `SM_ERR` |
//! | [`TopologyFailure::RadialRimsOffEndPlane`] | `SM_ERR` |
//! | [`TopologyFailure::RadialSweepNeedsCones`] | `SM_ERR` |
//! | [`TopologyFailure::RadialSweepNotOutward`] | `SM_ERR` |
//! | [`TopologyFailure::RayFoundNoRegion`] | `SM_ERR` |
//! | [`TopologyFailure::RayHitsAtStart`] | `SM_ERR` |
//! | [`TopologyFailure::RecipientIsVertexShell`] | `SM_ERR` |
//! | [`TopologyFailure::RecordTypeUnknown`] | `SM_ERR` |
//! | [`TopologyFailure::SeamSplitFailed`] | `SM_ERR` |
//! | [`TopologyFailure::SheetReoriented`] | `1` (upstream `TRUE`) |
//! | [`TopologyFailure::ShellHasNoFaceUses`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::ShellSideUndecided`] | `SM_ERR` |
//! | [`TopologyFailure::SolveViewWithoutCache`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::SourceIsVertexShell`] | `SM_ERR` |
//! | [`TopologyFailure::SplitDefinitionNotAdded`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::SteppedOffFace`] | `SM_ERR` |
//! | [`TopologyFailure::SteppedOutsideUvDomain`] | `SM_ERR` |
//! | [`TopologyFailure::SurfaceDiscontinuous`] | `SM_ERR` |
//! | [`TopologyFailure::SurfaceNotTransformable`] | `SM_ERR` |
//! | [`TopologyFailure::SurfaceWithoutGenerator`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::SurfaceWithoutNurbs`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::SurfaceWithoutOwningFace`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::TooManyOutputSlots`] | `SM_ERR_OUT_OF_MEMORY` |
//! | [`TopologyFailure::TooManyTangentFaces`] | `SM_ERR` |
//! | [`TopologyFailure::TrimBoundaryNotBuilt`] | `SM_ERR` |
//! | [`TopologyFailure::UnknownShellType`] | `SM_ERR` |
//! | [`TopologyFailure::UnorientedLoopsOnNonG1Surface`] | `SM_ERR` |
//! | [`TopologyFailure::UvDomainOutsideSurface`] | `SM_ERR` |
//! | [`TopologyFailure::UvTrimCurveNotMade`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::UvTrimCurvesNotMade`] | `SM_ERR` |
//! | [`TopologyFailure::VertexHasEdges`] | `SM_ERR` |
//! | [`TopologyFailure::VertexLoopInSharedFace`] | `SM_ERR` |
//! | [`TopologyFailure::VertexNotOnEdge`] | `SM_ERR` |
//! | [`TopologyFailure::VertexNotOnTwoEdges`] | `SM_ERR` |
//! | [`TopologyFailure::VertexUseNotOnFace`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::VertexWithoutUses`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::VertexusesNotGiven`] | `SM_ERR` |
//! | [`TopologyFailure::VertexusesOnDifferentFaces`] | `SM_ERR` |
//! | [`TopologyFailure::VerticesShareNoRegion`] | `SM_ERR_NULL_POINTER` |
//! | [`TopologyFailure::WireEndSqueezed`] | `SM_ERR` |
//! | [`TopologyFailure::ZeroLengthVector`] | `SM_ERR_INVALID_INPUT` |
//! | [`TopologyFailure::ZeroSurfaceTangent`] | `SM_ERR` |
//!
//! A named cause keeps the status of the phrase it replaced, unless its site overrides it
//! through `with_status`.
//!
//! [`TopologyFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says in a phrase (or by its variant) what went
//! wrong; the failure's `operation` path says which routine, and its entity is the one
//! the site it replaces named (a vertex use for the vertex-use routines, none
//! elsewhere). A check of a caller's argument that reports no status (upstream
//! dereferences null) stays [`crate::SolidError::InvalidInput`]; one that reports a status
//! is a named cause of class `invalid_input`, keeping that status.
//! `brep/p9`'s face-orientation repair keeps its final status 1 (upstream `TRUE`) in
//! [`TopologyFailure::SheetReoriented`], for a caller that stops on it.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_INVALID_INPUT,
    SM_ERR_NULL_POINTER, SM_ERR_OUT_OF_MEMORY,
};

/// Why a topology routine fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum TopologyFailure {
    /// A vertex definition that absorbed a curve cannot move to the position: a point of
    /// that curve is at no finite distance from it (a non-finite position, or a distance
    /// that overflows). Class `failed`.
    AbsorbedCurveAtNoFiniteDistance,
    /// Memory for an output array could not be allocated. Class `failed`.
    AllocationFailed,
    /// The file names an attribute whose slot in the attribute table is empty.
    /// [`ErrorKind::InvalidInput`].
    AttributeSlotEmpty,
    /// The binormal is still of zero length after every step-off into the face: the surface's
    /// normal and the edge's tangent give no direction there.
    BinormalStaysZero,
    /// A procedural blend read from the file is not proved within its bound of its
    /// definition; the proof's failure is the cause. Class `failed`; whether it is a
    /// defect or a refusal is its cause's.
    BlendRecordNotProved,
    /// The caller named B-rep `which` of a vertex definition; there are only 1 and 2.
    /// [`ErrorKind::InvalidInput`].
    BrepSideNotOneOrTwo {
        /// The B-rep the caller named.
        which: i32,
    },
    /// A B-rep cache was asked for a type that is neither a B-rep nor a poly B-rep.
    /// [`ErrorKind::InvalidInput`].
    CacheTargetNotBrep,
    /// A point classification or intersection the caller gave is not on a topology object
    /// (a vertex, edge, face, loop or region). [`ErrorKind::InvalidInput`].
    ClassificationOffTopology,
    /// Neither point classification the caller gave has its parameters set, so there is
    /// no point to place the position at. [`ErrorKind::InvalidInput`].
    ClassificationParametersUnset,
    /// The edge starts and ends at one vertex, which the routine does not take.
    /// [`ErrorKind::InvalidInput`].
    ClosedEdge,
    /// A B-rep constructor call (`call`, the upstream name) needs `part` open, and it is
    /// not: the caller's call order. [`ErrorKind::InvalidInput`].
    ConstructorNotOpen {
        /// The constructor call, by its upstream name (`EndBrep`, `StartEdge`, ...).
        call: &'static str,
        /// What the call works in.
        part: ConstructorPart,
    },
    /// A B-rep constructor call (`call`, the upstream name) needs `part` closed, and it is
    /// still open: the caller's call order. [`ErrorKind::InvalidInput`].
    ConstructorStillOpen {
        /// The constructor call, by its upstream name.
        call: &'static str,
        /// What is still open.
        part: ConstructorPart,
    },
    /// A curve meeting a loop vertex has a zero tangent at its end there, so the curves round
    /// the vertex cannot be ordered by direction. [`ErrorKind::Unsupported`].
    CurveEndWithoutTangent,
    /// A curve index the caller gave lies in no loop. [`ErrorKind::InvalidInput`].
    CurveIndexOutOfRange {
        /// The index given.
        index: usize,
    },
    /// The loops the caller's counts make of the curves are not valid.
    /// [`ErrorKind::InvalidInput`].
    CurveLoopsInvalid,
    /// The edge's curve could be neither lifted nor approximated as a B-spline (the
    /// approximation's failure is the cause). Class `failed`; whether it is a defect or a
    /// refusal is its cause's ([`crate::SolidError::is_defect`] follows the chain).
    CurveNotApproximated,
    /// A curve the routine needs as a B-spline is of a kind with no B-spline form here.
    /// [`ErrorKind::Unsupported`].
    CurveNotBSpline {
        /// The curve's owner, in the B-rep the routine read it from, where it has one.
        entity: Option<EntityRef>,
    },
    /// A curve the caller named is not in the loops. [`ErrorKind::InvalidInput`].
    CurveNotInLoops,
    /// An edge's curve resolves to no owning edge: it has no owner, or the caller's owner
    /// map does not resolve the edge use or generated curve that owns it.
    /// [`ErrorKind::InvalidInput`].
    CurveOwnerUnresolved,
    /// A curve of a loop the caller gave is missing. [`ErrorKind::InvalidInput`].
    CurveSlotEmpty,
    /// The B-rep's store holds no more curves (its handles are full), so a copied curve
    /// cannot be stored. [`ErrorKind::Unsupported`].
    CurveStoreFull,
    /// Two curves the caller named are not adjacent in their loop. [`ErrorKind::InvalidInput`].
    CurvesNotAdjacent,
    /// Two curves the caller named are not in one loop. [`ErrorKind::InvalidInput`].
    CurvesNotInOneLoop,
    /// Joining the two edges' B-spline copies at the deleted vertex failed; the join's
    /// failure is the cause. Class `failed`; whether it is a defect or a refusal is its
    /// cause's ([`crate::SolidError::is_defect`] follows the chain).
    CurvesNotJoined,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// A vertex definition the caller gave has no source topology on one of its B-reps.
    /// [`ErrorKind::InvalidInput`].
    DefinitionWithoutSourceTopology,
    /// The edge's parameter interval is a single point, so there is no length of it to measure a
    /// gap along. A fact of the body as supplied, so of kind [`ErrorKind::InvalidInput`].
    DegenerateEdgeInterval,
    /// The face's UV domain has no extent in u or in v, so no point can be sampled inside it. A
    /// fact of the body as supplied, so of kind [`ErrorKind::InvalidInput`].
    DegenerateUvDomain,
    /// Dropping the edge use's curve onto its face failed earlier, with `status`, and the
    /// edge use keeps that failure.
    DropFailed {
        /// The status the drop recorded (a comparison detail).
        status: i32,
    },
    /// The edge already has its curve, so a second `SetEdgeCurve` for it sets nothing.
    /// [`ErrorKind::InvalidInput`].
    EdgeCurveAlreadySet,
    /// The curve of the edge (`entity`) has no exact B-spline form over the edge, so the two edges' curves
    /// cannot be joined (WR-598): it is neither of a B-spline kind nor an iso line of a
    /// B-spline surface, or it is an iso line of a procedural blend whose net is not within
    /// the joined edge's tolerance of the blend's definition. [`ErrorKind::Unsupported`].
    EdgeCurveNotJoinable {
        /// The edge whose curve has no B-spline form.
        entity: Option<EntityRef>,
    },
    /// The edge to delete is neither a wire, a lamina nor a manifold edge (for example a
    /// spine edge, which more than two faces share): the deletion takes only those.
    /// [`ErrorKind::Unsupported`].
    EdgeNotDeletable,
    /// The edge the caller chose to delete is not one of the vertex's two edges.
    /// [`ErrorKind::InvalidInput`].
    EdgeNotOnVertex,
    /// An edge could not be split at the parameter. Class `failed`.
    EdgeNotSplit,
    /// Classifying the given edge use against a radial candidate failed; the classification's own
    /// error is kept as the cause.
    EdgeUseNotClassified,
    /// The caller asked for the binormal of an edge use that is not in a loop; only an edge use in
    /// a loop has one.
    EdgeUseNotInLoop,
    /// The routine works in a face, and the caller's edge use lies on none (an edge use of a
    /// wire). Its argument, so of kind [`ErrorKind::InvalidInput`].
    EdgeUseNotOnFace,
    /// The edge use the caller gave as the loop use's first is not on that loop use. Its argument,
    /// so of kind [`ErrorKind::InvalidInput`].
    EdgeUseNotOnLoopUse,
    /// An edge of the body as supplied has no curve, which the routine needs: the caller's body,
    /// so of kind [`ErrorKind::InvalidInput`].
    EdgeWithoutCurve,
    /// An edge of the caller's body has no edge use, so it is on no loop or wire (an edge
    /// added and never linked, or one no edge-use record of a file names).
    /// [`ErrorKind::InvalidInput`].
    EdgeWithoutEdgeuses,
    /// The caller's sequence of edges is not a chain: the edge at `index` shares no vertex with
    /// the end of the edge before it. Its argument, so of kind [`ErrorKind::InvalidInput`].
    EdgesNotChained {
        /// The index, in the caller's sequence, of the edge that does not continue from the one
        /// before it.
        index: usize,
    },
    /// The two edges the caller gave to glue do not share both their vertices.
    /// [`ErrorKind::InvalidInput`].
    EdgesNotSharingVertices,
    /// The two edges the caller gave are on different B-reps. [`ErrorKind::InvalidInput`].
    EdgesOnDifferentBreps,
    /// The routine edits the B-rep, and the caller has not enabled editing on it
    /// (`Brep::set_editing_enabled`). The caller's state: [`ErrorKind::InvalidInput`].
    EditingDisabled,
    /// A face the caller listed to copy is listed more than once.
    /// [`ErrorKind::InvalidInput`].
    FaceListedTwice,
    /// The face could not be made; the error that stopped it is the cause, and its class is
    /// the cause's.
    FaceNotMade {
        /// The class of the error that stopped it, the cause.
        class: ErrorKind,
    },
    /// Stitching the squeezed face's vertices and edges into their neighbours failed; the
    /// first stitch step's failure is the cause. Class `failed`.
    FaceStitchFailed,
    /// The face use the caller asked to remove from a shell belongs to another shell, or to none.
    /// Its argument, so of kind [`ErrorKind::InvalidInput`].
    FaceUseNotInShell,
    /// The face use the caller asked to move to another shell is in no shell (it was taken out of
    /// its shell before). Its argument, so of kind [`ErrorKind::InvalidInput`].
    FaceUseWithoutShell,
    /// The face's upward face use has no edge use (its loops are vertex loops, or it has
    /// none), so no edge gives a direction to vote with. [`ErrorKind::Unsupported`].
    FaceWithoutEdges,
    /// The face has no loops. [`ErrorKind::InvalidInput`].
    FaceWithoutLoops,
    /// A face of the body as supplied has no surface, which the routine needs: the caller's body,
    /// so of kind [`ErrorKind::InvalidInput`]. `SM_ERR_NULL_POINTER`, or the site's status.
    FaceWithoutSurface,
    /// The face the caller gave has no face use, so no loop can be put on it.
    /// [`ErrorKind::InvalidInput`].
    FaceWithoutUses,
    /// A requested face has no copy, or more than one, after the copy: the merge passed
    /// over a face it failed to make, or split one. Class `failed`.
    FacesNotCopiedOnce,
    /// The routine could not continue, as the phrase says (for example "the loop does
    /// not close").
    Failed(&'static str),
    /// Gathering the face uses connected to one side of a face reached more face uses than
    /// faces remain to be oriented (upstream's "Nonorientable Brep" assert): the body is not
    /// orientable, or it is an open sheet the closed-solid orientation does not take. Class
    /// `failed`.
    GatheredBothSides,
    /// A consistency check of the routine's own data did not hold, as the phrase says.
    Inconsistent(&'static str),
    /// The search for the infinite region found no region outside every shell. Class
    /// `failed`.
    InfiniteRegionNotFound,
    /// Both sides of the manifold edge to delete are two different inner loops: the deletion
    /// merges faces only through an outer loop. As upstream, it finds this after the edge
    /// is deleted, so the B-rep is left edited. [`ErrorKind::Unsupported`].
    InnerLoopsOnBothSides,
    /// One of two intersecting proto edges to split against each other has no edge
    /// definitions: an earlier split, whose failure the Boolean only warns on, left it
    /// empty. Class `failed`.
    IntersectingProtoEdgeEmpty,
    /// A value the routine is given or computes with is not usable, as the phrase says
    /// (for example "the plane normal has zero length"). Upstream returns
    /// `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// The UV line segment across the face's domain at one iso-parameter could not be made; the
    /// segment constructor's error is kept as the cause.
    IsoLineNotMade,
    /// A vertex at a joined edge needs tolerance `needed` to cover the gap the join left
    /// between it and the edge's curve end (twice the gap), and that ball would reach the far
    /// vertex of one of its edges, which it does not bound. A tolerance below `fits_below`
    /// fits (the distance less that vertex's tolerance, the smallest over its edges); `None`
    /// when none does.
    JoinedVertexReaches {
        /// The tolerance the vertex needs.
        needed: f64,
        /// The tolerance that fits, if any.
        fits_below: Option<f64>,
    },
    /// The lamina edge to delete is a strut at an end (its two edge uses follow each other
    /// in the loop there), which the lamina deletion does not take.
    /// [`ErrorKind::Unsupported`].
    LaminaEdgeIsStrut,
    /// An edge of the inner loop the caller asked to delete is shared with another face (it is not
    /// a lamina edge), so the loop cannot be deleted alone. Its argument, so of kind
    /// [`ErrorKind::InvalidInput`].
    LoopEdgeNotLamina,
    /// A loop index the caller gave is not below the number of loops.
    /// [`ErrorKind::InvalidInput`].
    LoopIndexOutOfRange {
        /// The index given.
        index: usize,
        /// The number of loops.
        loops: usize,
    },
    /// The loop the caller asked to delete as an inner loop is the face's outer loop. Its
    /// argument, so of kind [`ErrorKind::InvalidInput`].
    LoopIsOuter,
    /// Between two consecutive edge uses a loop's UV jumps more than a tenth of the
    /// surface's domain, with neither end singular and the gap not certified.
    /// [`ErrorKind::InvalidInput`].
    LoopJumpsInUv,
    /// An edge use's clockwise neighbour's mate is not its mate's counter-clockwise
    /// neighbour: the caller's `StartEdge` orientations or joined edges run against their
    /// loops. [`ErrorKind::InvalidInput`].
    LoopMatesDisagree,
    /// The loop the caller named is not one of the face's loops, or is on no face at all (its loop
    /// uses lie on no face use: `SM_ERR_NULL_POINTER` at those sites). Its argument, so of kind
    /// [`ErrorKind::InvalidInput`].
    LoopNotOnFace,
    /// The orientation of a face's loop is undecided: its area is zero or unresolved, or
    /// the walk that measures it failed. Class `failed`.
    LoopOrientationUndecided,
    /// A loop record of the caller's data names no edge use that was built.
    /// [`ErrorKind::InvalidInput`].
    LoopRecordWithoutEdgeuse,
    /// A loop of the caller's curves split a face off before the loop closed. Class `failed`.
    LoopSplitFaceEarly,
    /// A closed loop of the caller's curves split no new face off. Class `failed`.
    LoopSplitNoFace,
    /// A walk round a loop visited more than `limit` edge uses without closing (upstream's bound
    /// against a loop that never returns to its start).
    LoopTooLong {
        /// The most edge uses a loop walk visits.
        limit: u64,
    },
    /// A loop's sample points (each curve's ends and middle) give no normal: they lie on
    /// one line, or the area they span is below the zero test. [`ErrorKind::InvalidInput`].
    LoopWithoutNormal,
    /// The face's first loop is not outer, or a later loop is not inner.
    /// [`ErrorKind::InvalidInput`].
    LoopsMisoriented,
    /// The caller's curves do not close into loops, or one lies off the loops' plane.
    /// [`ErrorKind::InvalidInput`].
    LoopsNotClosedOrPlanar {
        /// Whether every loop closed.
        closed: bool,
        /// Whether every curve lies on the loops' plane.
        on_plane: bool,
    },
    /// Merging a vertex definition's point into its B-rep made `new_vertices` vertices, not
    /// one, and the point is not on a vertex there. Class `failed`.
    MergeGaveNoVertex {
        /// The number of vertices the merge made.
        new_vertices: usize,
    },
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// The edge use the caller gave as the edge's new primary is not one of the edge's.
    NewPrimaryNotOnEdge,
    /// The caller gave no B-rep. [`ErrorKind::InvalidInput`].
    NoBrepGiven,
    /// No point stepped off the face use's first edge could be classified against the
    /// shells: the step leaves the face (a face narrower than the step) or the
    /// classification fails, at every step tried. Class `failed`.
    NoClassifiablePoint,
    /// The caller gave no curve. [`ErrorKind::InvalidInput`].
    NoCurveGiven,
    /// The caller asked for neither the 3D nor the UV curves of the face, so there is nothing to
    /// make. Its argument, so of kind [`ErrorKind::InvalidInput`].
    NoCurvesAskedFor,
    /// The caller gave no file contents to read. [`ErrorKind::InvalidInput`].
    NoDataGiven,
    /// The search for the infinite region has no edge to aim its ray at: the B-rep or the
    /// shell's faces have none, or every face of the shell has both sides in it.
    /// [`ErrorKind::Unsupported`].
    NoEdgeToAimAt,
    /// The caller gave no face. [`ErrorKind::InvalidInput`].
    NoFaceGiven,
    /// The caller gave an empty list of face uses to decide a shell's side from. Its argument, so
    /// of kind [`ErrorKind::InvalidInput`].
    NoFaceUses,
    /// No vertex imprinted at the proto vertex at an end of the edge definition is
    /// connected to the definition's source topology (an imprint of that end failed
    /// earlier, and its status was ignored). Class `failed`.
    NoFinalVertexAtEnd,
    /// The caller's B-rep has no region but the infinite one to keep.
    /// [`ErrorKind::InvalidInput`].
    NoFiniteRegion,
    /// An edge limit point drops onto no point of the edge's curve, even at a hundred
    /// times the edge's tolerance. Class `failed`.
    NoFootOnCurve,
    /// A vertex definition's position drops onto no point of its vertex's seam curve.
    /// Class `failed`.
    NoFootOnSeam,
    /// A search for the point's foot on the face's surface found none.
    /// Class `failed`.
    NoFootOnSurface,
    /// The B-rep the caller gave has no infinite region. [`ErrorKind::InvalidInput`].
    NoInfiniteRegion,
    /// The database reader has no input stream: nothing was opened for reading.
    /// [`ErrorKind::InvalidInput`].
    NoInputStream,
    /// No sampled line across the face's UV domain has an interval that classifies inside the
    /// face: an empty search.
    NoInteriorPoint,
    /// No sampled iso-parameter line crosses the inside of the face: an empty search.
    NoIsoLineCrossesFace,
    /// The curve loops hold no loop. [`ErrorKind::InvalidInput`].
    NoLoops,
    /// The curve is of a kind with no plane projection here (a Hermite, projected, iso,
    /// offset, conic, in-volume or composite curve). [`ErrorKind::Unsupported`].
    NoPlaneProjection,
    /// The plane through the curves could not be made; the error that stopped it is the cause,
    /// and its class is the cause's.
    NoPlaneThroughCurves {
        /// The class of the error that stopped it, the cause.
        class: ErrorKind,
    },
    /// The projection of the curve onto the B-rep along the direction gives no curves: the
    /// curve's sweep along it misses the faces, or meets them only in pieces the trim drops
    /// as degenerate. Class `failed`.
    NoProjectedCurves,
    /// No radial edge use of the edge has the region and orientation asked for, so there is no
    /// sector to look between: an empty search, not a missing link.
    NoRadialCandidate,
    /// No sample along the edge classifies the given edge use between two radial neighbours: an
    /// empty search, which upstream calls a valid case.
    NoRadialSectorBetween,
    /// The caller gave no region. [`ErrorKind::InvalidInput`].
    NoRegionGiven,
    /// The caller's list of regions to keep is empty. [`ErrorKind::InvalidInput`].
    NoRegionsToKeepGiven,
    /// None of the sample points of the edge's curve drops onto the face's surface, so no UV trim
    /// curve can be fitted through their feet.
    NoSampleOnSurface,
    /// No radial sector of an edge takes a new face's edge use there; the face is still
    /// made. Class `failed`.
    NoSectorForNewEdgeuse,
    /// The caller gave no edge use to start the loop walk from: its argument, so of kind
    /// [`ErrorKind::InvalidInput`].
    NoStartEdgeUse,
    /// Neither a step forward nor a step back from the parameter stays inside the edge's interval,
    /// so no second point gives the binormal's direction.
    NoStepInsideInterval,
    /// The caller gave no surface. [`ErrorKind::InvalidInput`].
    NoSurfaceGiven,
    /// An edge gave no B-spline copy of its curve trimmed to its interval: it has no curve,
    /// its curve has no B-spline form, or the trim failed. Class `failed`.
    NoTrimmedBSpline,
    /// The caller gave no vertex. [`ErrorKind::InvalidInput`].
    NoVertexGiven,
    /// No sector of the vertex at the curve's start (its end, with `at_end`) takes the
    /// curve into the face. Class `failed`.
    NoVertexSector {
        /// Whether the search was at the curve's end.
        at_end: bool,
    },
    /// The two objects whose relationship is to be removed are not a mated pair (an
    /// intersector without relationships has none). Class `failed`.
    NotAMatedPair,
    /// The B-rep the caller gave is not a sheet. [`ErrorKind::InvalidInput`].
    NotASheet,
    /// The caller asked for the curve parameter of a vertex use that is not on an edge use; only
    /// an edge vertex use has one. Its argument, so of kind [`ErrorKind::InvalidInput`].
    NotAnEdgeVertexUse,
    /// The routine works on a manifold edge, which has four edge uses, and the caller's edge has
    /// `found`.
    NotFourEdgeUses {
        /// The number of edge uses the edge has.
        found: usize,
    },
    /// An entity the caller named (a region, a face) is not one of the B-rep's.
    /// [`ErrorKind::InvalidInput`].
    NotInBrep,
    /// The object to remove is not on this owner's list: an owning topology's members, or
    /// a proto vertex's, a proto edge's or the proto topology manager's lists.
    /// [`ErrorKind::InvalidInput`].
    NotOnOwnerList,
    /// The object to link, or the member to link it after, was not given.
    /// [`ErrorKind::InvalidInput`].
    NothingToLink,
    /// Neither of the two new edges the caller gave after a split is the old edge.
    /// [`ErrorKind::InvalidInput`].
    OldEdgeNotAmongSplit,
    /// Memory for the data could not be had, as the phrase says.
    OutOfMemory(&'static str),
    /// Two proto edges overlap along a piece whose both ends fall where an end of either
    /// edge's definitions refuses to split (within ten times their tolerance of it), so
    /// there is no piece of that edge to combine. Class `failed`.
    OverlapBelowSplitResolution,
    /// An owner's list of members is not a closed ring: a link is null, or a successor does
    /// not point back. [`ErrorKind::InvalidInput`], as every broken list the owner walks.
    OwnerListBroken,
    /// An owner's list holds more members than its count `list_size`.
    /// [`ErrorKind::InvalidInput`].
    OwnerListOverCount {
        /// The owner's recorded member count.
        list_size: u64,
    },
    /// The part to read into already holds curves, surfaces, Boolean tree nodes or B-reps.
    /// [`ErrorKind::InvalidInput`].
    PartNotEmpty,
    /// The proto edge the caller gave has no edge definitions to evaluate or split.
    /// [`ErrorKind::InvalidInput`].
    ProtoEdgeWithoutDefinitions,
    /// The proto vertex the caller named is neither end of the proto edge.
    /// [`ErrorKind::InvalidInput`].
    ProtoVertexNotAnEnd,
    /// The plane through a radial sweep's rims is not a plane. Class `failed`, status
    /// `SM_ERR`.
    RadialCapWithoutPlane,
    /// The face being drafted has no surface. [`ErrorKind::Missing`], status
    /// `SM_ERR_NULL_POINTER`.
    RadialFaceWithoutSurface,
    /// A rim edge use has no edge. [`ErrorKind::Missing`], status `SM_ERR_NULL_POINTER`.
    RadialRimWithoutEdge,
    /// The two rims of a radial sweep's cap are not concentric circles in one stored plane
    /// with the outer one the larger, so they bound no annulus. Class `failed`, status
    /// `SM_ERR`.
    RadialRimsNotAnnulus,
    /// The two rim curves of a radial sweep's cap are not both ellipses. Class `failed`,
    /// status `SM_ERR`.
    RadialRimsNotEllipses,
    /// The plane through a radial sweep's rims is not exactly the plane of the support's
    /// fixed end. Class `failed`, status `SM_ERR`.
    RadialRimsOffEndPlane,
    /// A radial sweep between two supports was asked for supports that are not both cones
    /// (a cylinder is a cone). [`ErrorKind::Unsupported`], status `SM_ERR`.
    RadialSweepNeedsCones,
    /// The closed body a radial sweep stitched is not an outward solid. Class `failed`,
    /// status `SM_ERR`.
    RadialSweepNotOutward,
    /// No ray toward the B-rep's edges found the region it starts in. Class `failed`.
    RayFoundNoRegion,
    /// The ray fired at the shell hits it at the ray's own start, so the hit does not say which
    /// side the start is on.
    RayHitsAtStart,
    /// The shell the caller asked to move members into is a vertex shell, which holds only its
    /// vertex, and the source has members to move. Its argument, so of kind
    /// [`ErrorKind::InvalidInput`].
    RecipientIsVertexShell,
    /// The file holds a curve or surface record of a type the read does not take.
    /// [`ErrorKind::InvalidInput`].
    RecordTypeUnknown {
        /// The record's type code.
        type_code: i64,
    },
    /// The surface could not be split at the new seam parameter; the split's error is kept as the
    /// cause.
    SeamSplitFailed,
    /// Not a failure: the sheet's faces were turned to agree
    /// ([`crate::topology::brep::p9::OrientSheetNote`]), as an error for a caller that stops on
    /// it, with the note's status `1` (upstream `TRUE`). Class `failed`.
    SheetReoriented,
    /// The shell the caller gave as the outer shell is a shell of faces that lists no face use.
    /// Its argument, so of kind [`ErrorKind::InvalidInput`].
    ShellHasNoFaceUses,
    /// The rays fired from the shell's faces do not decide whether the shell is inner: `forward`
    /// and `backward` hits are neither all one way nor odd in number.
    ShellSideUndecided {
        /// The rays that left through a face seen from its front.
        forward: u64,
        /// The rays that left through a face seen from its back.
        backward: u64,
    },
    /// The topology solver asked for the B-rep's cache on a view the caller made without
    /// one. [`ErrorKind::InvalidInput`].
    SolveViewWithoutCache,
    /// The shell the caller asked to move members out of is a vertex shell, whose vertex cannot
    /// move to a shell of faces or wires. Its argument, so of kind [`ErrorKind::InvalidInput`].
    SourceIsVertexShell,
    /// The vertex definition at the split point of an edge definition could not be found
    /// or added; that failure is the cause. Class `failed`; whether it is a defect or a
    /// refusal is its cause's.
    SplitDefinitionNotAdded,
    /// The point stepped off the edge into the face has no foot on the face's surface: the step
    /// left the surface.
    SteppedOffFace,
    /// The point stepped off the edge into the face drops outside the face's UV domain.
    SteppedOutsideUvDomain,
    /// The surface is discontinuous in a parameter direction, which the trim does not take.
    /// [`ErrorKind::Unsupported`].
    SurfaceDiscontinuous,
    /// The surface of the face (`entity`) could not be transformed in place, and its kind has no
    /// B-spline base to transform instead (a curve-bounded, STEP, offset or in-volume
    /// surface); the in-place transform's failure is the cause. [`ErrorKind::Unsupported`].
    SurfaceNotTransformable {
        /// The face whose surface could not be transformed.
        entity: Option<EntityRef>,
    },
    /// A surface of revolution or extrusion has no generator curve, which its record
    /// needs (only the `empty()` constructors leave it out). [`ErrorKind::InvalidInput`].
    SurfaceWithoutGenerator,
    /// A surface has no NURBS form. [`ErrorKind::Unsupported`].
    SurfaceWithoutNurbs {
        /// The face the surface belongs to, where it has one.
        entity: Option<EntityRef>,
    },
    /// A face's surface has no owning face. [`ErrorKind::InvalidInput`].
    SurfaceWithoutOwningFace,
    /// The caller asked for more curve output slots than fit below `SM_BIG_ULONG` entries
    /// (or than the platform can index). [`ErrorKind::InvalidInput`].
    TooManyOutputSlots {
        /// The slots asked for.
        asked: u64,
        /// A number of slots below this fits.
        fits_below: u64,
    },
    /// The corner calculation handles a vertex where up to three tangent faces meet, and this
    /// vertex has more.
    TooManyTangentFaces,
    /// The face's oriented trim boundary could not be built; the builder's error is kept as the
    /// cause.
    TrimBoundaryNotBuilt,
    /// A shell record of the caller's data has a type no shell has.
    /// [`ErrorKind::InvalidInput`].
    UnknownShellType {
        /// The record's type.
        shell_type: u64,
    },
    /// The surface to trim is not G1 and the caller's loops are not oriented, which the trim
    /// does not take: it orients loops only on a G1 surface. [`ErrorKind::Unsupported`].
    UnorientedLoopsOnNonG1Surface,
    /// The UV domain the caller gave is not inside the surface's natural domain.
    /// [`ErrorKind::InvalidInput`].
    UvDomainOutsideSurface,
    /// The edge use has no UV trim curve and making one failed; the error of the fetch that tried
    /// is kept as the cause.
    UvTrimCurveNotMade,
    /// The UV trim curves of a face could not be made; the error that stopped them, where
    /// there is one, is the cause. Class `failed`; whether it is a defect or a refusal is
    /// its cause's.
    UvTrimCurvesNotMade,
    /// The vertex to delete still has edges; the caller deletes them first.
    /// [`ErrorKind::InvalidInput`].
    VertexHasEdges,
    /// The two vertices to combine share a face in which the surviving vertex is a vertex
    /// loop, so no edge between them can be made in that face. [`ErrorKind::Unsupported`].
    VertexLoopInSharedFace,
    /// The vertex the caller named to survive is not a vertex of the edge.
    /// [`ErrorKind::InvalidInput`].
    VertexNotOnEdge,
    /// The vertex to delete between two edges is on `edges` edges, not two.
    /// [`ErrorKind::InvalidInput`].
    VertexNotOnTwoEdges {
        /// The number of edges at the vertex.
        edges: usize,
    },
    /// The caller gave a vertex use that lies on no face (a shell or wire vertex use) where the
    /// routine needs one on a face. Its argument, so of kind [`ErrorKind::InvalidInput`].
    /// `SM_ERR_NULL_POINTER`, or the site's status.
    VertexUseNotOnFace,
    /// A vertex of the caller's data that a single-vertex loop record names has no vertex use.
    /// [`ErrorKind::InvalidInput`].
    VertexWithoutUses,
    /// The caller did not give both vertex uses of the new edge. [`ErrorKind::InvalidInput`].
    VertexusesNotGiven,
    /// The vertex uses the caller gave are on different face uses. [`ErrorKind::InvalidInput`].
    VertexusesOnDifferentFaces,
    /// The two vertices to glue have no region in common: no shell of one is in a region
    /// with a shell of the other. [`ErrorKind::InvalidInput`].
    VerticesShareNoRegion,
    /// The edge to squeeze ends a wire at the vertex to delete, so no other edge there can
    /// take the head of the wire's shell. [`ErrorKind::Unsupported`].
    WireEndSqueezed,
    /// A vector the routine is given, or computes from what it is given, has zero length, so it
    /// has no direction and no angle (upstream's vector operation returns `SM_ERR_INVALID_INPUT`).
    ZeroLengthVector,
    /// The surface has a zero-length first derivative at the point where the edge's gap to the
    /// face is measured (a pole or a degenerate patch), so the gap function has no value there.
    ZeroSurfaceTangent,
}

/// What a B-rep constructor call works in ([`TopologyFailure::ConstructorNotOpen`],
/// [`TopologyFailure::ConstructorStillOpen`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstructorPart {
    /// The B-rep (`StartBrep`).
    Brep,
    /// An edge and its edge use (`StartEdge`).
    Edge,
    /// The open edge's curve (`SetEdgeCurve`).
    EdgeCurve,
    /// A face (`StartFace`).
    Face,
    /// A loop (`StartLoop`).
    Loop,
    /// A region (`StartRegion`).
    Region,
    /// A shell (`StartShell`).
    Shell,
}

impl core::fmt::Display for ConstructorPart {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Brep => "B-rep",
            Self::Edge => "edge",
            Self::EdgeCurve => "edge curve",
            Self::Face => "face",
            Self::Loop => "loop",
            Self::Region => "region",
            Self::Shell => "shell",
        })
    }
}

impl TopologyFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::AbsorbedCurveAtNoFiniteDistance
            | Self::BinormalStaysZero
            | Self::BlendRecordNotProved
            | Self::BrepSideNotOneOrTwo { .. }
            | Self::CacheTargetNotBrep
            | Self::ClassificationParametersUnset
            | Self::ClosedEdge
            | Self::ConstructorNotOpen { .. }
            | Self::ConstructorStillOpen { .. }
            | Self::CurveEndWithoutTangent
            | Self::CurveIndexOutOfRange { .. }
            | Self::CurveNotInLoops
            | Self::CurvesNotAdjacent
            | Self::CurvesNotInOneLoop
            | Self::CurvesNotJoined
            | Self::Defect { .. }
            | Self::DegenerateEdgeInterval
            | Self::DegenerateUvDomain
            | Self::EdgeCurveAlreadySet
            | Self::EdgeCurveNotJoinable { .. }
            | Self::EdgeNotDeletable
            | Self::EdgeNotOnVertex
            | Self::EdgeNotSplit
            | Self::EdgeUseNotClassified
            | Self::EdgeUseNotOnLoopUse
            | Self::EdgesNotChained { .. }
            | Self::EdgesNotSharingVertices
            | Self::EditingDisabled
            | Self::FaceListedTwice
            | Self::FaceNotMade { .. }
            | Self::FaceStitchFailed
            | Self::FaceUseNotInShell
            | Self::FaceWithoutEdges
            | Self::FaceWithoutLoops
            | Self::FacesNotCopiedOnce
            | Self::Failed(_)
            | Self::InnerLoopsOnBothSides
            | Self::IntersectingProtoEdgeEmpty
            | Self::IsoLineNotMade
            | Self::JoinedVertexReaches { .. }
            | Self::LaminaEdgeIsStrut
            | Self::LoopEdgeNotLamina
            | Self::LoopIsOuter
            | Self::LoopJumpsInUv
            | Self::LoopMatesDisagree
            | Self::LoopNotOnFace
            | Self::LoopOrientationUndecided
            | Self::LoopRecordWithoutEdgeuse
            | Self::LoopSplitFaceEarly
            | Self::LoopSplitNoFace
            | Self::LoopTooLong { .. }
            | Self::LoopWithoutNormal
            | Self::LoopsMisoriented
            | Self::LoopsNotClosedOrPlanar { .. }
            | Self::MergeGaveNoVertex { .. }
            | Self::NoClassifiablePoint
            | Self::NoCurvesAskedFor
            | Self::NoEdgeToAimAt
            | Self::NoFaceUses
            | Self::NoFiniteRegion
            | Self::NoFootOnCurve
            | Self::NoFootOnSeam
            | Self::NoFootOnSurface
            | Self::NoInfiniteRegion
            | Self::NoInteriorPoint
            | Self::NoIsoLineCrossesFace
            | Self::NoLoops
            | Self::NoPlaneProjection
            | Self::NoPlaneThroughCurves { .. }
            | Self::NoProjectedCurves
            | Self::NoRadialCandidate
            | Self::NoRadialSectorBetween
            | Self::NoRegionsToKeepGiven
            | Self::NoSampleOnSurface
            | Self::NoSectorForNewEdgeuse
            | Self::NoStepInsideInterval
            | Self::NotAMatedPair
            | Self::NotASheet
            | Self::NotAnEdgeVertexUse
            | Self::NotInBrep
            | Self::NotOnOwnerList
            | Self::NothingToLink
            | Self::OverlapBelowSplitResolution
            | Self::OwnerListBroken
            | Self::OwnerListOverCount { .. }
            | Self::ProtoVertexNotAnEnd
            | Self::RadialCapWithoutPlane
            | Self::RadialRimsNotAnnulus
            | Self::RadialRimsNotEllipses
            | Self::RadialRimsOffEndPlane
            | Self::RadialSweepNeedsCones
            | Self::RadialSweepNotOutward
            | Self::RayFoundNoRegion
            | Self::RayHitsAtStart
            | Self::RecipientIsVertexShell
            | Self::RecordTypeUnknown { .. }
            | Self::SeamSplitFailed
            | Self::ShellSideUndecided { .. }
            | Self::SourceIsVertexShell
            | Self::SteppedOffFace
            | Self::SteppedOutsideUvDomain
            | Self::SurfaceDiscontinuous
            | Self::SurfaceNotTransformable { .. }
            | Self::TooManyTangentFaces
            | Self::TrimBoundaryNotBuilt
            | Self::UnknownShellType { .. }
            | Self::UnorientedLoopsOnNonG1Surface
            | Self::UvDomainOutsideSurface
            | Self::UvTrimCurvesNotMade
            | Self::VertexHasEdges
            | Self::VertexLoopInSharedFace
            | Self::VertexNotOnEdge
            | Self::VertexNotOnTwoEdges { .. }
            | Self::VertexusesNotGiven
            | Self::VertexusesOnDifferentFaces
            | Self::WireEndSqueezed
            | Self::ZeroSurfaceTangent => SM_ERR,
            Self::AttributeSlotEmpty
            | Self::ClassificationOffTopology
            | Self::CurveNotApproximated
            | Self::CurveNotBSpline { .. }
            | Self::CurveOwnerUnresolved
            | Self::CurveSlotEmpty
            | Self::CurveStoreFull
            | Self::DefinitionWithoutSourceTopology
            | Self::EdgeUseNotOnFace
            | Self::EdgeWithoutCurve
            | Self::EdgeWithoutEdgeuses
            | Self::FaceUseWithoutShell
            | Self::FaceWithoutSurface
            | Self::FaceWithoutUses
            | Self::InfiniteRegionNotFound
            | Self::Missing(_)
            | Self::NoFaceGiven
            | Self::NoFinalVertexAtEnd
            | Self::NoInputStream
            | Self::NoStartEdgeUse
            | Self::NoSurfaceGiven
            | Self::NoVertexGiven
            | Self::NoVertexSector { .. }
            | Self::OldEdgeNotAmongSplit
            | Self::RadialFaceWithoutSurface
            | Self::RadialRimWithoutEdge
            | Self::ShellHasNoFaceUses
            | Self::SolveViewWithoutCache
            | Self::SplitDefinitionNotAdded
            | Self::SurfaceWithoutGenerator
            | Self::SurfaceWithoutNurbs { .. }
            | Self::SurfaceWithoutOwningFace
            | Self::UvTrimCurveNotMade
            | Self::VertexUseNotOnFace
            | Self::VertexWithoutUses
            | Self::VerticesShareNoRegion => SM_ERR_NULL_POINTER,
            Self::CurveLoopsInvalid
            | Self::EdgeUseNotInLoop
            | Self::InvalidValue(_)
            | Self::LoopIndexOutOfRange { .. }
            | Self::NewPrimaryNotOnEdge
            | Self::NoBrepGiven
            | Self::NoCurveGiven
            | Self::NoDataGiven
            | Self::NoRegionGiven
            | Self::NoTrimmedBSpline
            | Self::NotFourEdgeUses { .. }
            | Self::PartNotEmpty
            | Self::ProtoEdgeWithoutDefinitions
            | Self::ZeroLengthVector => SM_ERR_INVALID_INPUT,
            Self::DropFailed { status } => *status,
            Self::Inconsistent(_) | Self::GatheredBothSides | Self::EdgesOnDifferentBreps => {
                SM_ERR_ASSERT_FAILURE
            }
            Self::OutOfMemory(_) | Self::TooManyOutputSlots { .. } | Self::AllocationFailed => {
                SM_ERR_OUT_OF_MEMORY
            }
            // upstream's `TRUE`: the repair's note, not an error status
            Self::SheetReoriented => 1,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`TopologyFailure::Missing`], [`TopologyFailure::RadialFaceWithoutSurface`] and
    /// [`TopologyFailure::RadialRimWithoutEdge`], [`ErrorKind::InvalidInput`] for
    /// [`TopologyFailure::InvalidValue`] and for a check of the caller's arguments, state or
    /// body, [`ErrorKind::Unsupported`] for a kind or configuration the routine does not
    /// handle, the drop status's kind for [`TopologyFailure::DropFailed`],
    /// [`ErrorKind::Failed`] otherwise. [`TopologyFailure::Defect`] is
    /// [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::AbsorbedCurveAtNoFiniteDistance
            | Self::AllocationFailed
            | Self::BlendRecordNotProved
            | Self::CurveNotApproximated
            | Self::CurvesNotJoined
            | Self::FaceStitchFailed
            | Self::InfiniteRegionNotFound
            | Self::IntersectingProtoEdgeEmpty
            | Self::LoopOrientationUndecided
            | Self::MergeGaveNoVertex { .. }
            | Self::NoFinalVertexAtEnd
            | Self::NoFootOnCurve
            | Self::NoFootOnSeam
            | Self::NoProjectedCurves
            | Self::NoTrimmedBSpline
            | Self::NoVertexSector { .. }
            | Self::NotAMatedPair
            | Self::OverlapBelowSplitResolution
            | Self::SplitDefinitionNotAdded => ErrorKind::Failed,
            Self::AttributeSlotEmpty
            | Self::BrepSideNotOneOrTwo { .. }
            | Self::CacheTargetNotBrep
            | Self::ClassificationOffTopology
            | Self::ClassificationParametersUnset
            | Self::ClosedEdge
            | Self::ConstructorNotOpen { .. }
            | Self::ConstructorStillOpen { .. }
            | Self::CurveIndexOutOfRange { .. }
            | Self::CurveLoopsInvalid
            | Self::CurveNotInLoops
            | Self::CurveOwnerUnresolved
            | Self::CurveSlotEmpty
            | Self::CurvesNotAdjacent
            | Self::CurvesNotInOneLoop
            | Self::DefinitionWithoutSourceTopology
            | Self::DegenerateEdgeInterval
            | Self::DegenerateUvDomain
            | Self::EdgeCurveAlreadySet
            | Self::EdgeNotOnVertex
            | Self::EdgeUseNotInLoop
            | Self::EdgeUseNotOnFace
            | Self::EdgeUseNotOnLoopUse
            | Self::EdgeWithoutCurve
            | Self::EdgeWithoutEdgeuses
            | Self::EdgesNotChained { .. }
            | Self::EdgesNotSharingVertices
            | Self::EdgesOnDifferentBreps
            | Self::EditingDisabled
            | Self::FaceListedTwice
            | Self::FaceUseNotInShell
            | Self::FaceUseWithoutShell
            | Self::FaceWithoutLoops
            | Self::FaceWithoutSurface
            | Self::FaceWithoutUses
            | Self::LoopEdgeNotLamina
            | Self::LoopIndexOutOfRange { .. }
            | Self::LoopIsOuter
            | Self::LoopJumpsInUv
            | Self::LoopMatesDisagree
            | Self::LoopNotOnFace
            | Self::LoopRecordWithoutEdgeuse
            | Self::LoopWithoutNormal
            | Self::LoopsMisoriented
            | Self::LoopsNotClosedOrPlanar { .. }
            | Self::NewPrimaryNotOnEdge
            | Self::NoBrepGiven
            | Self::NoCurveGiven
            | Self::NoCurvesAskedFor
            | Self::NoDataGiven
            | Self::NoFaceGiven
            | Self::NoFaceUses
            | Self::NoFiniteRegion
            | Self::NoInfiniteRegion
            | Self::NoInputStream
            | Self::NoLoops
            | Self::NoRegionGiven
            | Self::NoRegionsToKeepGiven
            | Self::NoStartEdgeUse
            | Self::NoSurfaceGiven
            | Self::NoVertexGiven
            | Self::NotASheet
            | Self::NotAnEdgeVertexUse
            | Self::NotFourEdgeUses { .. }
            | Self::NotInBrep
            | Self::NotOnOwnerList
            | Self::NothingToLink
            | Self::OldEdgeNotAmongSplit
            | Self::OwnerListBroken
            | Self::OwnerListOverCount { .. }
            | Self::PartNotEmpty
            | Self::ProtoEdgeWithoutDefinitions
            | Self::ProtoVertexNotAnEnd
            | Self::RecipientIsVertexShell
            | Self::RecordTypeUnknown { .. }
            | Self::ShellHasNoFaceUses
            | Self::SolveViewWithoutCache
            | Self::SourceIsVertexShell
            | Self::SurfaceWithoutGenerator
            | Self::SurfaceWithoutOwningFace
            | Self::TooManyOutputSlots { .. }
            | Self::UnknownShellType { .. }
            | Self::UvDomainOutsideSurface
            | Self::VertexHasEdges
            | Self::VertexNotOnEdge
            | Self::VertexNotOnTwoEdges { .. }
            | Self::VertexUseNotOnFace
            | Self::VertexWithoutUses
            | Self::VertexusesNotGiven
            | Self::VertexusesOnDifferentFaces
            | Self::VerticesShareNoRegion
            | Self::ZeroLengthVector => ErrorKind::InvalidInput,
            Self::CurveEndWithoutTangent
            | Self::CurveNotBSpline { .. }
            | Self::CurveStoreFull
            | Self::EdgeCurveNotJoinable { .. }
            | Self::EdgeNotDeletable
            | Self::FaceWithoutEdges
            | Self::InnerLoopsOnBothSides
            | Self::LaminaEdgeIsStrut
            | Self::NoEdgeToAimAt
            | Self::NoPlaneProjection
            | Self::RadialSweepNeedsCones
            | Self::SurfaceDiscontinuous
            | Self::SurfaceNotTransformable { .. }
            | Self::SurfaceWithoutNurbs { .. }
            | Self::TooManyTangentFaces
            | Self::UnorientedLoopsOnNonG1Surface
            | Self::VertexLoopInSharedFace
            | Self::WireEndSqueezed => ErrorKind::Unsupported,
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::NoPlaneThroughCurves { class } | Self::FaceNotMade { class } => *class,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for TopologyFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AbsorbedCurveAtNoFiniteDistance => f.write_str(
                "a vertex definition that absorbed a curve cannot move to a position at no \
                 finite distance from it",
            ),
            Self::AllocationFailed => f.write_str("memory for the output could not be allocated"),
            Self::AttributeSlotEmpty => {
                f.write_str("the file names an attribute whose table slot is empty")
            }
            Self::BinormalStaysZero => f.write_str("the binormal stays zero after every step-off"),
            Self::BlendRecordNotProved => f.write_str(
                "a procedural blend read from the file is not proved within its bound of its \
                 definition",
            ),
            Self::BrepSideNotOneOrTwo { which } => {
                write!(f, "there is no B-rep {which}, only 1 and 2")
            }
            Self::CacheTargetNotBrep => {
                f.write_str("a B-rep cache is only for a B-rep or a poly B-rep")
            }
            Self::ClassificationOffTopology => {
                f.write_str("the classification is not on a topology object")
            }
            Self::ClassificationParametersUnset => {
                f.write_str("neither point classification has its parameters set")
            }
            Self::ClosedEdge => f.write_str("the edge starts and ends at one vertex"),
            Self::ConstructorNotOpen {
                call,
                part: ConstructorPart::EdgeCurve,
            } => write!(
                f,
                "the B-rep constructor's {call}: the edge has no curve yet"
            ),
            Self::ConstructorNotOpen { call, part } => {
                write!(f, "the B-rep constructor's {call}: no {part} is open")
            }
            Self::ConstructorStillOpen { call, part } => {
                write!(f, "the B-rep constructor's {call}: a {part} is still open")
            }
            Self::CurveEndWithoutTangent => f.write_str("a curve has a zero tangent at its end"),
            Self::CurveIndexOutOfRange { index } => {
                write!(f, "curve index {index} lies in no loop")
            }
            Self::CurveLoopsInvalid => f.write_str("the curve loops are not valid"),
            Self::CurveNotApproximated => {
                f.write_str("the edge's curve could not be made a B-spline")
            }
            Self::CurveNotBSpline { entity: None } => f.write_str("a curve has no B-spline form"),
            Self::CurveNotBSpline { entity: Some(e) } => {
                write!(f, "a curve has no B-spline form ({e})")
            }
            Self::CurveNotInLoops => f.write_str("the curve is not in the loops"),
            Self::CurveOwnerUnresolved => {
                f.write_str("the edge's curve resolves to no owning edge")
            }
            Self::CurveSlotEmpty => f.write_str("a curve of a loop is missing"),
            Self::CurveStoreFull => f.write_str("the B-rep's curve store is full"),
            Self::CurvesNotAdjacent => f.write_str("the two curves are not adjacent in their loop"),
            Self::CurvesNotInOneLoop => f.write_str("the curves are not in one loop"),
            Self::CurvesNotJoined => f.write_str("the edges' curves could not be joined"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::DefinitionWithoutSourceTopology => {
                f.write_str("the definition has no source topology")
            }
            Self::DegenerateEdgeInterval => f.write_str("the edge's interval is a single point"),
            Self::DegenerateUvDomain => f.write_str("the face's UV domain is degenerate"),
            Self::DropFailed { .. } => {
                f.write_str("the edge use's curve could not be dropped onto its face")
            }
            Self::EdgeCurveAlreadySet => f.write_str("the edge already has its curve"),
            Self::EdgeCurveNotJoinable { entity: None } => {
                f.write_str("an edge's curve has no exact B-spline form to join")
            }
            Self::EdgeCurveNotJoinable { entity: Some(e) } => {
                write!(
                    f,
                    "an edge's curve has no exact B-spline form to join ({e})"
                )
            }
            Self::EdgeNotDeletable => {
                f.write_str("only a wire, lamina or manifold edge can be deleted")
            }
            Self::EdgeNotOnVertex => f.write_str("the edge to delete is not on the vertex"),
            Self::EdgeNotSplit => f.write_str("the edge could not be split"),
            Self::EdgeUseNotClassified => f.write_str("an edge use could not be classified"),
            Self::EdgeUseNotInLoop => f.write_str("only an edge use in a loop has a binormal"),
            Self::EdgeUseNotOnFace => f.write_str("the edge use is not on a face"),
            Self::EdgeUseNotOnLoopUse => f.write_str("the edge use is not on this edge loop use"),
            Self::EdgeWithoutCurve => f.write_str("the edge has no curve"),
            Self::EdgeWithoutEdgeuses => f.write_str("the edge has no edge use"),
            Self::EdgesNotChained { index } => write!(
                f,
                "edge {index} of the sequence does not continue from the edge before it"
            ),
            Self::EdgesNotSharingVertices => {
                f.write_str("the edges to glue do not share both vertices")
            }
            Self::EdgesOnDifferentBreps => f.write_str("the edges are on different B-reps"),
            Self::EditingDisabled => f.write_str("editing is not enabled on the B-rep"),
            Self::FaceListedTwice => f.write_str("a face to copy is listed twice"),
            Self::FaceNotMade { .. } => f.write_str("the face could not be made"),
            Self::FaceStitchFailed => f.write_str("stitching the face's vertices and edges failed"),
            Self::FaceUseNotInShell => f.write_str("the face use is not in the shell"),
            Self::FaceUseWithoutShell => f.write_str("the face use is in no shell"),
            Self::FaceWithoutEdges => f.write_str("the face has no edges"),
            Self::FaceWithoutLoops => f.write_str("the face has no loops"),
            Self::FaceWithoutSurface => f.write_str("the face has no surface"),
            Self::FaceWithoutUses => f.write_str("the face has no face use"),
            Self::FacesNotCopiedOnce => {
                f.write_str("a requested face has no copy, or more than one")
            }
            Self::Failed(what) | Self::InvalidValue(what) => f.write_str(what),
            Self::GatheredBothSides => {
                f.write_str("more face uses are connected than faces remain to be oriented")
            }
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
            Self::InfiniteRegionNotFound => f.write_str("no infinite region was found"),
            Self::InnerLoopsOnBothSides => {
                f.write_str("both sides of the edge are inner loops of different faces")
            }
            Self::IntersectingProtoEdgeEmpty => {
                f.write_str("a proto edge of the intersecting pair has no edge definitions")
            }
            Self::IsoLineNotMade => f.write_str("the iso-parameter UV line could not be made"),
            Self::JoinedVertexReaches {
                needed,
                fits_below: Some(fits),
            } => write!(
                f,
                "a joined edge's vertex needs tolerance {needed} to cover the join's gap, which \
                 reaches the far vertex of one of its edges; a tolerance below {fits} fits"
            ),
            Self::JoinedVertexReaches {
                needed,
                fits_below: None,
            } => write!(
                f,
                "a joined edge's vertex needs tolerance {needed} to cover the join's gap, which \
                 reaches the far vertex of one of its edges; no tolerance fits"
            ),
            Self::LaminaEdgeIsStrut => f.write_str("the lamina edge to delete is a strut"),
            Self::LoopEdgeNotLamina => f.write_str("an edge of the loop is not a lamina edge"),
            Self::LoopIndexOutOfRange { index, loops } => {
                write!(f, "loop index {index} is past the last of {loops} loops")
            }
            Self::LoopIsOuter => f.write_str("the loop is the face's outer loop"),
            Self::LoopJumpsInUv => f.write_str(
                "a loop's UV jumps more than a tenth of the surface's domain between two \
                 edge uses",
            ),
            Self::LoopMatesDisagree => f.write_str(
                "an edge use's clockwise neighbour's mate is not its mate's counter-clockwise \
                 neighbour",
            ),
            Self::LoopNotOnFace => f.write_str("the loop is not one of the face's loops"),
            Self::LoopOrientationUndecided => {
                f.write_str("a loop's orientation on the face is undecided")
            }
            Self::LoopRecordWithoutEdgeuse => f.write_str("a loop record names no built edge use"),
            Self::LoopSplitFaceEarly => f.write_str("a face was split off before its loop closed"),
            Self::LoopSplitNoFace => f.write_str("the closed loop split off no new face"),
            Self::LoopTooLong { limit } => write!(f, "the loop has more than {limit} edge uses"),
            Self::LoopWithoutNormal => f.write_str("a loop's sample points give no normal"),
            Self::LoopsMisoriented => {
                f.write_str("the first loop is not outer, or a later loop is not inner")
            }
            Self::LoopsNotClosedOrPlanar { closed, on_plane } => write!(
                f,
                "the curves do not form closed loops in one plane \
                 (closed: {closed}, on the plane: {on_plane})"
            ),
            Self::MergeGaveNoVertex { new_vertices } => write!(
                f,
                "merging the point made {new_vertices} vertices, not one, and it is not on a \
                 vertex"
            ),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::NewPrimaryNotOnEdge => {
                f.write_str("the new primary edge use is not one of the edge's")
            }
            Self::NoBrepGiven => f.write_str("no B-rep was given"),
            Self::NoClassifiablePoint => {
                f.write_str("no point stepped off the face use could be classified")
            }
            Self::NoCurveGiven => f.write_str("no curve was given"),
            Self::NoCurvesAskedFor => f.write_str("neither 3D nor UV curves are asked for"),
            Self::NoDataGiven => f.write_str("no file contents were given"),
            Self::NoEdgeToAimAt => f.write_str("there is no edge to aim the ray at"),
            Self::NoFaceGiven => f.write_str("no face was given"),
            Self::NoFaceUses => f.write_str("there are no face uses"),
            Self::NoFinalVertexAtEnd => f.write_str(
                "no vertex imprinted at the proto vertex is connected to the edge definition",
            ),
            Self::NoFiniteRegion => f.write_str("the B-rep has no finite region to keep"),
            Self::NoFootOnCurve => {
                f.write_str("the limit point does not drop onto the edge's curve")
            }
            Self::NoFootOnSeam => {
                f.write_str("the vertex definition's position drops onto no point of its seam")
            }
            Self::NoFootOnSurface => {
                f.write_str("the point does not project onto the face's surface")
            }
            Self::NoInfiniteRegion => f.write_str("the B-rep has no infinite region"),
            Self::NoInputStream => f.write_str("the database has no input stream"),
            Self::NoInteriorPoint => f.write_str("no line interval classifies into the face"),
            Self::NoIsoLineCrossesFace => f.write_str("no iso-parameter line crosses the face"),
            Self::NoLoops => f.write_str("there are no loops"),
            Self::NoPlaneProjection => f.write_str("the curve kind has no plane projection"),
            Self::NoPlaneThroughCurves { .. } => {
                f.write_str("the plane through the curves could not be made")
            }
            Self::NoProjectedCurves => {
                f.write_str("the curve's projection onto the B-rep gives no curves")
            }
            Self::NoRadialCandidate => f.write_str("no radial edge use is left after culling"),
            Self::NoRadialSectorBetween => f.write_str("no radial sector lies between the faces"),
            Self::NoRegionGiven => f.write_str("no region was given"),
            Self::NoRegionsToKeepGiven => f.write_str("no regions to keep were given"),
            Self::NoSampleOnSurface => {
                f.write_str("no sample point of the curve drops onto the surface")
            }
            Self::NoSectorForNewEdgeuse => {
                f.write_str("no radial sector of the edge takes the new edge use")
            }
            Self::NoStartEdgeUse => f.write_str("no edge use to start the loop from was given"),
            Self::NoStepInsideInterval => {
                f.write_str("no step from the parameter stays inside the edge's interval")
            }
            Self::NoSurfaceGiven => f.write_str("no surface was given"),
            Self::NoTrimmedBSpline => {
                f.write_str("an edge gave no B-spline of its curve trimmed to its interval")
            }
            Self::NoVertexGiven => f.write_str("no vertex was given"),
            Self::NoVertexSector { at_end: false } => {
                f.write_str("no sector of the vertex at the curve's start takes the curve")
            }
            Self::NoVertexSector { at_end: true } => {
                f.write_str("no sector of the vertex at the curve's end takes the curve")
            }
            Self::NotAMatedPair => f.write_str("the two are not a mated pair"),
            Self::NotASheet => f.write_str("the B-rep is not a sheet"),
            Self::NotAnEdgeVertexUse => {
                f.write_str("only an edge vertex use has a curve parameter")
            }
            Self::NotFourEdgeUses { found } => write!(
                f,
                "the edge has {found} edge uses, not the four of a manifold edge"
            ),
            Self::NotInBrep => f.write_str("the entity given is not one of the B-rep's"),
            Self::NotOnOwnerList => f.write_str("the object is not on this list"),
            Self::NothingToLink => f.write_str("an object to link is missing"),
            Self::OldEdgeNotAmongSplit => {
                f.write_str("neither new edge of the split is the old edge")
            }
            Self::OutOfMemory(what) => write!(f, "out of memory for {what}"),
            Self::OverlapBelowSplitResolution => {
                f.write_str("two proto edges overlap along less than their definitions can split")
            }
            Self::OwnerListBroken => {
                f.write_str("the owner's list of members is not a closed ring")
            }
            Self::OwnerListOverCount { list_size } => {
                write!(
                    f,
                    "the owner's list holds more than its {list_size} members"
                )
            }
            Self::PartNotEmpty => f.write_str("the part to read into is not empty"),
            Self::ProtoEdgeWithoutDefinitions => {
                f.write_str("the proto edge has no edge definitions")
            }
            Self::ProtoVertexNotAnEnd => {
                f.write_str("the proto vertex is neither end of the proto edge")
            }
            Self::RadialCapWithoutPlane => f.write_str("the radial cap has no plane"),
            Self::RadialFaceWithoutSurface => f.write_str("the drafted face has no surface"),
            Self::RadialRimWithoutEdge => f.write_str("a rim edge use has no edge"),
            Self::RadialRimsNotAnnulus => {
                f.write_str("the radial rims do not define a concentric annulus")
            }
            Self::RadialRimsNotEllipses => f.write_str("the radial rims are not ellipses"),
            Self::RadialRimsOffEndPlane => {
                f.write_str("the radial circles leave the exact end plane")
            }
            Self::RadialSweepNeedsCones => {
                f.write_str("the radial sweep requires two conical supports")
            }
            Self::RadialSweepNotOutward => f.write_str("the radial sweep is not an outward solid"),
            Self::RayFoundNoRegion => f.write_str("no ray toward the edges found the region"),
            Self::RayHitsAtStart => f.write_str("the ray hits the shell at its start"),
            Self::RecipientIsVertexShell => {
                f.write_str("the recipient is a vertex shell and the source is not empty")
            }
            Self::RecordTypeUnknown { type_code } => write!(
                f,
                "the file holds a record of type {type_code}, which the read does not take"
            ),
            Self::SeamSplitFailed => {
                f.write_str("the surface could not be split at the new seam parameter")
            }
            Self::SheetReoriented => f.write_str("the sheet's faces were turned to agree"),
            Self::ShellHasNoFaceUses => f.write_str("the outer shell has no face uses"),
            Self::ShellSideUndecided { forward, backward } => write!(
                f,
                "the rays do not decide whether the shell is inner ({forward} forward, \
                 {backward} backward)"
            ),
            Self::SolveViewWithoutCache => {
                f.write_str("the solve needs the B-rep's cache, and the view has none")
            }
            Self::SourceIsVertexShell => f.write_str("the source is a vertex shell"),
            Self::SplitDefinitionNotAdded => {
                f.write_str("the vertex definition at the split could not be added")
            }
            Self::SteppedOffFace => f.write_str("the stepped-off point does not lie on the face"),
            Self::SteppedOutsideUvDomain => {
                f.write_str("the stepped-off point is outside the face's UV domain")
            }
            Self::SurfaceDiscontinuous => f.write_str("the surface is discontinuous"),
            Self::SurfaceNotTransformable { entity: None } => {
                f.write_str("a face's surface could not be transformed")
            }
            Self::SurfaceNotTransformable { entity: Some(e) } => {
                write!(f, "a face's surface could not be transformed ({e})")
            }
            Self::SurfaceWithoutGenerator => {
                f.write_str("the swept surface has no generator curve")
            }
            Self::SurfaceWithoutNurbs { entity: None } => {
                f.write_str("a surface has no NURBS form")
            }
            Self::SurfaceWithoutNurbs { entity: Some(e) } => {
                write!(f, "a surface has no NURBS form ({e})")
            }
            Self::SurfaceWithoutOwningFace => f.write_str("the face's surface has no owning face"),
            Self::TooManyOutputSlots { asked, fits_below } => write!(
                f,
                "{asked} curve output slots were asked for; fewer than {fits_below} fit"
            ),
            Self::TooManyTangentFaces => f.write_str("the vertex has more than three faces"),
            Self::TrimBoundaryNotBuilt => {
                f.write_str("the face's trim boundary could not be built")
            }
            Self::UnknownShellType { shell_type } => {
                write!(f, "a shell record has the unknown type {shell_type}")
            }
            Self::UnorientedLoopsOnNonG1Surface => {
                f.write_str("the surface is not G1 and the loops are not oriented")
            }
            Self::UvDomainOutsideSurface => {
                f.write_str("the UV domain is not inside the surface's natural domain")
            }
            Self::UvTrimCurveNotMade => {
                f.write_str("the edge use's UV trim curve could not be made")
            }
            Self::UvTrimCurvesNotMade => f.write_str("the face's UV trim curves could not be made"),
            Self::VertexHasEdges => f.write_str("the vertex still has edges"),
            Self::VertexLoopInSharedFace => f.write_str(
                "the vertices share a face in which the surviving vertex is a vertex loop",
            ),
            Self::VertexNotOnEdge => f.write_str("the surviving vertex is not on the edge"),
            Self::VertexNotOnTwoEdges { edges } => {
                write!(f, "the vertex is on {edges} edges, not two")
            }
            Self::VertexUseNotOnFace => f.write_str("the vertex use is not on a face"),
            Self::VertexWithoutUses => f.write_str("a vertex of a vertex loop has no vertex use"),
            Self::VertexusesNotGiven => f.write_str("both vertex uses are needed"),
            Self::VertexusesOnDifferentFaces => {
                f.write_str("the vertex uses are on different face uses")
            }
            Self::VerticesShareNoRegion => f.write_str("the vertices share no region"),
            Self::WireEndSqueezed => {
                f.write_str("the edge to squeeze ends its wire at the vertex to delete")
            }
            Self::ZeroLengthVector => f.write_str("a vector of zero length has no direction"),
            Self::ZeroSurfaceTangent => f.write_str("the surface has a zero-length tangent"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a topology routine that fails with `failure`.
    #[must_use]
    pub(crate) fn topology(operation: &'static str, failure: TopologyFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a topology routine that fails with `failure`
    /// about `entity`.
    #[must_use]
    pub(crate) fn topology_at(
        operation: &'static str,
        failure: TopologyFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a topology routine that could not continue, as
    /// the phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn topology_failed(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a topology routine needs that is not
    /// there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream
    /// `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn topology_missing(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a value a topology routine computes with that has
    /// no defined result, as the phrase says, with status [`SM_ERR_INVALID_INPUT`]
    /// (upstream `return(SM_ERR_INVALID_INPUT)`, or `SER` of a vector operation that
    /// returns it).
    #[must_use]
    pub(crate) fn topology_invalid_value(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::InvalidValue(what))
    }

    /// The topology failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Topology`].
    #[must_use]
    pub fn topology_failure(&self) -> Option<&TopologyFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Topology(t) => Some(t),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SM_ERR_OUTSIDE_OF_DOMAIN;
    use crate::topology::VertexuseId;
    use crate::SolidError;

    #[test]
    fn topology_failures_keep_the_status_kind_and_entity_of_the_upstream_they_replace() {
        let vu = Some(EntityRef::Vertexuse(VertexuseId::from_index(3)));
        let cases = [
            (TopologyFailure::Failed("x"), SM_ERR),
            (TopologyFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (TopologyFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (TopologyFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
            (TopologyFailure::OutOfMemory("x"), SM_ERR_OUT_OF_MEMORY),
            (
                TopologyFailure::DropFailed {
                    status: SM_ERR_OUTSIDE_OF_DOMAIN,
                },
                SM_ERR_OUTSIDE_OF_DOMAIN,
            ),
        ];
        for (failure, code) in cases {
            for entity in [None, vu] {
                let typed = match entity {
                    Some(e) => SolidError::topology_at("topology::op", failure, e),
                    None => SolidError::topology("topology::op", failure),
                };
                let before = SolidError::Upstream {
                    operation: "topology::op",
                    code,
                    entity,
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
                assert_eq!(typed.topology_failure(), Some(&failure));
            }
        }
        assert_eq!(
            SolidError::topology_missing("topology::op", "the edge's curve").to_string(),
            "topology::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::topology_at(
                "topology::op",
                TopologyFailure::Failed("the vertex use has no point"),
                EntityRef::Vertexuse(VertexuseId::from_index(3)),
            )
            .to_string(),
            format!(
                "topology::op: the vertex use has no point ({})",
                EntityRef::Vertexuse(VertexuseId::from_index(3))
            )
        );
        assert_eq!(
            SolidError::topology_invalid_value("topology::op", "x").kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            SolidError::topology_failed("topology::op", "x").kind(),
            ErrorKind::Failed
        );
        assert_eq!(SolidError::sm_err("topology::op").topology_failure(), None);
    }

    /// A sample of the second set of converted files (`brep/p1`, `p5` to `p10`,
    /// `face/p2`): each keeps the operation, status, kind and entity of its `Upstream`.
    #[test]
    fn the_brep_edit_and_face_samples_keep_their_upstream_answers() {
        let samples = [
            (
                "topology::brep::p1",
                TopologyFailure::InvalidValue("no new infinite region was given"),
                SM_ERR_INVALID_INPUT,
            ),
            (
                "topology::brep::p5",
                TopologyFailure::Failed("editing is not enabled"),
                SM_ERR,
            ),
            (
                crate::topology::brep::p5::NO_VERTEX_SECTOR_OP,
                TopologyFailure::Missing("a vertex sector at the curve's start"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::brep::p6",
                TopologyFailure::Missing("the face's upward face use"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::brep::p7::unstitch",
                TopologyFailure::Failed("not every face could be copied"),
                SM_ERR,
            ),
            (
                "topology::brep::p8",
                TopologyFailure::InvalidValue("the curve loops are not valid"),
                SM_ERR_INVALID_INPUT,
            ),
            (
                "topology::brep::p9",
                TopologyFailure::Missing("the face use's mate"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::face::trim_curve_breaks",
                TopologyFailure::Failed("the trim curve has fewer than two knots"),
                SM_ERR,
            ),
        ];
        for (operation, failure, code) in samples {
            let typed = SolidError::topology(operation, failure);
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
    }

    /// The named causes of the B-rep edits keep the status of the phrase each replaced, and
    /// have their class.
    #[test]
    fn the_brep_edit_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let op = "topology::op";
        let cases = [
            (TopologyFailure::EditingDisabled, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::NoBrepGiven,
                SM_ERR_INVALID_INPUT,
                K::InvalidInput,
            ),
            (TopologyFailure::NotInBrep, SM_ERR, K::InvalidInput),
            (TopologyFailure::VertexNotOnEdge, SM_ERR, K::InvalidInput),
            (TopologyFailure::ClosedEdge, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::CurveNotBSpline { entity: None },
                SM_ERR_NULL_POINTER,
                K::Unsupported,
            ),
            (
                TopologyFailure::SurfaceWithoutNurbs { entity: None },
                SM_ERR_NULL_POINTER,
                K::Unsupported,
            ),
            (TopologyFailure::FaceWithoutEdges, SM_ERR, K::Unsupported),
            (TopologyFailure::WireEndSqueezed, SM_ERR, K::Unsupported),
            (TopologyFailure::NoFootOnSurface, SM_ERR, K::Failed),
            (
                TopologyFailure::CurveNotApproximated,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                TopologyFailure::GatheredBothSides,
                SM_ERR_ASSERT_FAILURE,
                K::Failed,
            ),
            (
                TopologyFailure::RadialFaceWithoutSurface,
                SM_ERR_NULL_POINTER,
                K::Missing,
            ),
            (
                TopologyFailure::RadialRimWithoutEdge,
                SM_ERR_NULL_POINTER,
                K::Missing,
            ),
            (
                TopologyFailure::RadialSweepNeedsCones,
                SM_ERR,
                K::Unsupported,
            ),
            (TopologyFailure::RadialRimsNotEllipses, SM_ERR, K::Failed),
            (TopologyFailure::RadialRimsNotAnnulus, SM_ERR, K::Failed),
            (TopologyFailure::RadialCapWithoutPlane, SM_ERR, K::Failed),
            (TopologyFailure::RadialRimsOffEndPlane, SM_ERR, K::Failed),
            (TopologyFailure::RadialSweepNotOutward, SM_ERR, K::Failed),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology(op, failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        assert_eq!(
            SolidError::topology(op, TopologyFailure::RadialFaceWithoutSurface).to_string(),
            "topology::op: the drafted face has no surface"
        );
        assert_eq!(
            SolidError::topology(op, TopologyFailure::RadialRimWithoutEdge).to_string(),
            "topology::op: a rim edge use has no edge"
        );
        // the approximation's failure stays its cause, and decides defect or refusal
        let why = "a step the kernel got wrong";
        let cause = SolidError::topology(op, TopologyFailure::Defect { why });
        let e = Failure::new(op, TopologyFailure::CurveNotApproximated)
            .caused_by(cause.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&cause));
        assert!(e.is_defect());
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
    }

    /// The named causes of the B-rep edits of `brep/p4` keep the status of the phrase each
    /// replaced, and have their class.
    #[test]
    fn the_brep_p4_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let cases = [
            (TopologyFailure::EdgeNotDeletable, SM_ERR, K::Unsupported),
            (
                TopologyFailure::InnerLoopsOnBothSides,
                SM_ERR,
                K::Unsupported,
            ),
            (TopologyFailure::LaminaEdgeIsStrut, SM_ERR, K::Unsupported),
            (TopologyFailure::VertexHasEdges, SM_ERR, K::InvalidInput),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology("topology::op", failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
    }

    /// The named causes of the Boolean's proto topology keep the status of the phrase each
    /// replaced, and have their class.
    #[test]
    fn the_proto_topology_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        use TopologyFailure as T;
        let op = "topology::op";
        let cases = [
            (T::NoFootOnSeam, SM_ERR, K::Failed),
            (T::ClassificationParametersUnset, SM_ERR, K::InvalidInput),
            (
                T::EdgesOnDifferentBreps,
                SM_ERR_ASSERT_FAILURE,
                K::InvalidInput,
            ),
            (T::AbsorbedCurveAtNoFiniteDistance, SM_ERR, K::Failed),
            (T::BrepSideNotOneOrTwo { which: 3 }, SM_ERR, K::InvalidInput),
            (T::ProtoVertexNotAnEnd, SM_ERR, K::InvalidInput),
            (
                T::ProtoEdgeWithoutDefinitions,
                SM_ERR_INVALID_INPUT,
                K::InvalidInput,
            ),
            (
                T::DefinitionWithoutSourceTopology,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (T::MergeGaveNoVertex { new_vertices: 0 }, SM_ERR, K::Failed),
            (T::NoFinalVertexAtEnd, SM_ERR_NULL_POINTER, K::Failed),
            (
                T::ClassificationOffTopology,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                T::OldEdgeNotAmongSplit,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (T::SplitDefinitionNotAdded, SM_ERR_NULL_POINTER, K::Failed),
            (T::CurveStoreFull, SM_ERR_NULL_POINTER, K::Unsupported),
            (T::OverlapBelowSplitResolution, SM_ERR, K::Failed),
            (T::IntersectingProtoEdgeEmpty, SM_ERR, K::Failed),
            (T::NotAMatedPair, SM_ERR, K::Failed),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology(op, failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        // the split's vertex definition keeps the failure that kept it out as its cause,
        // which decides defect or not
        let why = "a step the kernel got wrong";
        let cause = SolidError::topology(op, T::Defect { why });
        let e = Failure::new(op, T::SplitDefinitionNotAdded)
            .caused_by(cause.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&cause));
        assert!(e.is_defect());
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        let cause = SolidError::topology(op, T::ClassificationOffTopology);
        let e = Failure::new(op, T::SplitDefinitionNotAdded)
            .caused_by(cause.clone())
            .into_error();
        assert!(!e.is_defect(), "{e:?}");
        assert_eq!(e.kind(), K::Failed);
    }

    /// The named causes of the B-rep constructor, the file and projection wrappers and the
    /// owner lists keep the status of the phrase each replaced, and have their class.
    #[test]
    fn the_construction_causes_keep_their_status_and_have_their_class() {
        use ConstructorPart as P;
        use ErrorKind as K;
        use TopologyFailure as T;
        let op = "topology::op";
        let slots = T::TooManyOutputSlots {
            asked: 9,
            fits_below: 3,
        };
        let not_open = T::ConstructorNotOpen {
            call: "EndShell",
            part: P::Shell,
        };
        let still_open = T::ConstructorStillOpen {
            call: "EndShell",
            part: P::Face,
        };
        let cases = [
            (T::NoDataGiven, SM_ERR_INVALID_INPUT, K::InvalidInput),
            (T::NoCurveGiven, SM_ERR_INVALID_INPUT, K::InvalidInput),
            (T::NoSurfaceGiven, SM_ERR_NULL_POINTER, K::InvalidInput),
            (T::PartNotEmpty, SM_ERR_INVALID_INPUT, K::InvalidInput),
            (slots, SM_ERR_OUT_OF_MEMORY, K::InvalidInput),
            (T::AllocationFailed, SM_ERR_OUT_OF_MEMORY, K::Failed),
            (T::NoProjectedCurves, SM_ERR, K::Failed),
            (T::NoPlaneProjection, SM_ERR, K::Unsupported),
            (not_open, SM_ERR, K::InvalidInput),
            (still_open, SM_ERR, K::InvalidInput),
            (T::EdgeCurveAlreadySet, SM_ERR, K::InvalidInput),
            (T::NoFootOnCurve, SM_ERR, K::Failed),
            (T::LoopMatesDisagree, SM_ERR, K::InvalidInput),
            (T::OwnerListBroken, SM_ERR, K::InvalidInput),
            (
                T::OwnerListOverCount { list_size: 2 },
                SM_ERR,
                K::InvalidInput,
            ),
            (T::NothingToLink, SM_ERR, K::InvalidInput),
            (T::NotOnOwnerList, SM_ERR, K::InvalidInput),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology(op, failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        assert_eq!(
            SolidError::topology(op, not_open).to_string(),
            "topology::op: the B-rep constructor's EndShell: no shell is open"
        );
        assert_eq!(
            SolidError::topology(op, still_open).to_string(),
            "topology::op: the B-rep constructor's EndShell: a face is still open"
        );
        let curve = T::ConstructorNotOpen {
            call: "SetLimits",
            part: P::EdgeCurve,
        };
        assert_eq!(
            SolidError::topology(op, curve).to_string(),
            "topology::op: the B-rep constructor's SetLimits: the edge has no curve yet"
        );
    }

    /// The named causes of the B-rep edits of `brep/p5` keep the status of the phrase each
    /// replaced, and have their class.
    #[test]
    fn the_brep_p5_causes_keep_their_status_and_have_their_class() {
        use crate::topology::{EdgeId, FaceId};
        use ErrorKind as K;
        let op = "topology::op";
        let edge = Some(EntityRef::Edge(EdgeId::from_index(2)));
        let face = Some(EntityRef::Face(FaceId::from_index(1)));
        let cases = [
            (
                TopologyFailure::NoVertexGiven,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                TopologyFailure::NoFaceGiven,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (TopologyFailure::NoInfiniteRegion, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::VertexLoopInSharedFace,
                SM_ERR,
                K::Unsupported,
            ),
            (
                TopologyFailure::VertexNotOnTwoEdges { edges: 3 },
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::EdgeNotOnVertex, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::EdgeCurveNotJoinable { entity: edge },
                SM_ERR,
                K::Unsupported,
            ),
            (TopologyFailure::CurvesNotJoined, SM_ERR, K::Failed),
            (
                TopologyFailure::EdgeWithoutCurve,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                TopologyFailure::InfiniteRegionNotFound,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                TopologyFailure::VerticesShareNoRegion,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                TopologyFailure::NoVertexSector { at_end: true },
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                TopologyFailure::NoRegionsToKeepGiven,
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::NoFiniteRegion, SM_ERR, K::InvalidInput),
            (TopologyFailure::FaceStitchFailed, SM_ERR, K::Failed),
            (
                TopologyFailure::SurfaceNotTransformable { entity: face },
                SM_ERR,
                K::Unsupported,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology(op, failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        // the entity a cause carries is in its message, not the failure's entity
        let e = SolidError::topology(op, TopologyFailure::EdgeCurveNotJoinable { entity: edge });
        assert_eq!(e.entity(), None);
        assert!(e
            .to_string()
            .contains(&EntityRef::Edge(EdgeId::from_index(2)).to_string()));
        // the ignored step's failure stays the cause, and decides defect or refusal
        let why = "a step the kernel got wrong";
        let step = SolidError::topology(op, TopologyFailure::Defect { why });
        let e = Failure::new(op, TopologyFailure::FaceStitchFailed)
            .caused_by(step.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&step));
        assert!(e.is_defect());
        assert_eq!(e.upstream_status(), Some(SM_ERR));
    }

    /// The named causes of the B-rep cache and the file reads and writes keep the status of
    /// the phrase each replaced, and have their class.
    #[test]
    fn the_cache_and_file_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        use TopologyFailure as T;
        let op = "topology::op";
        let cases = [
            (
                T::SurfaceWithoutGenerator,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (T::EdgeWithoutEdgeuses, SM_ERR_NULL_POINTER, K::InvalidInput),
            (
                T::CurveOwnerUnresolved,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                T::SurfaceWithoutOwningFace,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                T::SolveViewWithoutCache,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (T::CacheTargetNotBrep, SM_ERR, K::InvalidInput),
            (T::NoInputStream, SM_ERR_NULL_POINTER, K::InvalidInput),
            (
                T::RecordTypeUnknown { type_code: 7 },
                SM_ERR,
                K::InvalidInput,
            ),
            (T::AttributeSlotEmpty, SM_ERR_NULL_POINTER, K::InvalidInput),
            (T::BlendRecordNotProved, SM_ERR, K::Failed),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology(op, failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        // the blend proof's failure stays the cause, and decides defect or refusal
        let why = "a step the kernel got wrong";
        let cause = SolidError::topology(op, T::Defect { why });
        let e = Failure::new(op, T::BlendRecordNotProved)
            .caused_by(cause.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&cause));
        assert!(e.is_defect());
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        // a net the proof refuses as input (`recertify`'s `InvalidInput`) reads as no defect
        let cause = SolidError::invalid_input(op, "the net is not within its bound");
        let e = Failure::new(op, T::BlendRecordNotProved)
            .caused_by(cause.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&cause));
        assert!(!e.is_defect(), "{e:?}");
        assert_eq!(e.kind(), K::Failed);
        assert_eq!(e.upstream_status(), Some(SM_ERR));
    }

    /// The topology routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the file listed (with the bare sites it may still hold).
    #[test]
    fn the_topology_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("topology", 53, &[]);
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the edge use the loop walk just reached has no loop use, which the B-rep's links always give";
        let defect = TopologyFailure::Defect { why };
        let op = "topology::op";
        super::super::tests::assert_defect(
            &SolidError::topology(op, defect),
            &SolidError::topology(op, TopologyFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::topology(op, TopologyFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.topology_failure(), Some(&defect));
    }

    /// The named causes of the topology entity routines (edges, edge uses, faces, loop uses,
    /// shells, vertex uses, the traverser) keep the status of the phrase cause each replaced,
    /// answer with it the way an `Upstream` does, and have the kind their doc says.
    #[test]
    fn the_topology_entity_causes_keep_their_statuses() {
        let op = "topology::op";
        let cases = [
            (
                TopologyFailure::FaceWithoutSurface,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::EdgeWithoutCurve,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::NoStartEdgeUse,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::NotAnEdgeVertexUse,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::EdgeUseNotInLoop,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::NewPrimaryNotOnEdge,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::NotFourEdgeUses { found: 5 },
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::ZeroLengthVector,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (TopologyFailure::NoFaceUses, SM_ERR, ErrorKind::InvalidInput),
            (
                TopologyFailure::NoCurvesAskedFor,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::LoopIsOuter,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::LoopNotOnFace,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::LoopEdgeNotLamina,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::FaceUseNotInShell,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::RecipientIsVertexShell,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::SourceIsVertexShell,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::EdgeUseNotOnLoopUse,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::EdgesNotChained { index: 5 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::DegenerateEdgeInterval,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::DegenerateUvDomain,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::TooManyTangentFaces,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                TopologyFailure::LoopTooLong { limit: 5 },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                TopologyFailure::NoRadialCandidate,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                TopologyFailure::NoRadialSectorBetween,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                TopologyFailure::EdgeUseNotClassified,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                TopologyFailure::ShellSideUndecided {
                    forward: 5,
                    backward: 5,
                },
                SM_ERR,
                ErrorKind::Failed,
            ),
            (TopologyFailure::NoInteriorPoint, SM_ERR, ErrorKind::Failed),
            (
                TopologyFailure::NoIsoLineCrossesFace,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (TopologyFailure::IsoLineNotMade, SM_ERR, ErrorKind::Failed),
            (
                TopologyFailure::NoSampleOnSurface,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (TopologyFailure::SteppedOffFace, SM_ERR, ErrorKind::Failed),
            (
                TopologyFailure::SteppedOutsideUvDomain,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                TopologyFailure::NoStepInsideInterval,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                TopologyFailure::BinormalStaysZero,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                TopologyFailure::ZeroSurfaceTangent,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (TopologyFailure::RayHitsAtStart, SM_ERR, ErrorKind::Failed),
            (
                TopologyFailure::TrimBoundaryNotBuilt,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (TopologyFailure::SeamSplitFailed, SM_ERR, ErrorKind::Failed),
            (
                TopologyFailure::VertexUseNotOnFace,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::EdgeUseNotOnFace,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::FaceUseWithoutShell,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::ShellHasNoFaceUses,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                TopologyFailure::UvTrimCurveNotMade,
                SM_ERR_NULL_POINTER,
                ErrorKind::Missing,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology(op, failure);
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(
                super::super::upstream_status(&e),
                Some(status),
                "{failure:?}"
            );
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.operation(), op, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
            assert!(e.is_status_answer(), "{failure:?}");
            assert!(e.to_string().starts_with("topology::op: "), "{failure:?}");
        }
        // A site whose phrase cause had another status than the named cause's keeps it.
        let at_site = Failure::new(op, TopologyFailure::FaceWithoutSurface)
            .with_status(SM_ERR)
            .into_error();
        assert_eq!(at_site.sm_status(), Some(SM_ERR));
        assert_eq!(super::super::upstream_status(&at_site), Some(SM_ERR));
        assert_eq!(at_site.kind(), ErrorKind::InvalidInput);
        // A reviewed consistency defect keeps the assertion status of the check it replaced.
        let asserted = Failure::new(op, TopologyFailure::Defect { why: "x" })
            .with_status(SM_ERR_ASSERT_FAILURE)
            .into_error();
        assert_eq!(asserted.sm_status(), Some(SM_ERR_ASSERT_FAILURE));
        assert_eq!(asserted.kind(), ErrorKind::StageFailed);
        assert!(asserted.is_defect());
    }

    /// The named causes of the later B-rep edits (face copy and surface transfer) keep the
    /// status of the phrase each replaced, and have their class.
    #[test]
    fn the_face_copy_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let op = "topology::op";
        let cases = [
            (
                TopologyFailure::NoTrimmedBSpline,
                SM_ERR_INVALID_INPUT,
                K::Failed,
            ),
            (TopologyFailure::FaceListedTwice, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::EdgesNotSharingVertices,
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::NoSectorForNewEdgeuse, SM_ERR, K::Failed),
            (TopologyFailure::NoClassifiablePoint, SM_ERR, K::Failed),
            (TopologyFailure::UvTrimCurvesNotMade, SM_ERR, K::Failed),
            (TopologyFailure::FacesNotCopiedOnce, SM_ERR, K::Failed),
            (
                TopologyFailure::NoRegionGiven,
                SM_ERR_INVALID_INPUT,
                K::InvalidInput,
            ),
            (
                TopologyFailure::UnorientedLoopsOnNonG1Surface,
                SM_ERR,
                K::Unsupported,
            ),
            (TopologyFailure::FaceWithoutLoops, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::LoopsNotClosedOrPlanar {
                    closed: false,
                    on_plane: true,
                },
                SM_ERR,
                K::InvalidInput,
            ),
            (
                TopologyFailure::CurveEndWithoutTangent,
                SM_ERR,
                K::Unsupported,
            ),
            (TopologyFailure::LoopWithoutNormal, SM_ERR, K::InvalidInput),
            (TopologyFailure::LoopOrientationUndecided, SM_ERR, K::Failed),
            (TopologyFailure::LoopsMisoriented, SM_ERR, K::InvalidInput),
            (TopologyFailure::LoopJumpsInUv, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::SurfaceDiscontinuous,
                SM_ERR,
                K::Unsupported,
            ),
            (
                TopologyFailure::LoopIndexOutOfRange { index: 2, loops: 2 },
                SM_ERR_INVALID_INPUT,
                K::InvalidInput,
            ),
            (
                TopologyFailure::CurveIndexOutOfRange { index: 7 },
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::CurveNotInLoops, SM_ERR, K::InvalidInput),
            (TopologyFailure::NoLoops, SM_ERR, K::InvalidInput),
            (TopologyFailure::CurvesNotInOneLoop, SM_ERR, K::InvalidInput),
            (TopologyFailure::CurvesNotAdjacent, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::CurveSlotEmpty,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                TopologyFailure::UvDomainOutsideSurface,
                SM_ERR,
                K::InvalidInput,
            ),
            (
                TopologyFailure::CurveLoopsInvalid,
                SM_ERR_INVALID_INPUT,
                K::InvalidInput,
            ),
            (TopologyFailure::NotASheet, SM_ERR, K::InvalidInput),
            (TopologyFailure::VertexusesNotGiven, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::VertexusesOnDifferentFaces,
                SM_ERR,
                K::InvalidInput,
            ),
            (
                TopologyFailure::UnknownShellType { shell_type: 9 },
                SM_ERR,
                K::InvalidInput,
            ),
            (
                TopologyFailure::VertexWithoutUses,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                TopologyFailure::FaceWithoutUses,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                TopologyFailure::LoopRecordWithoutEdgeuse,
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::NoEdgeToAimAt, SM_ERR, K::Unsupported),
            (TopologyFailure::RayFoundNoRegion, SM_ERR, K::Failed),
            // the class is the cause's
            (
                TopologyFailure::NoPlaneThroughCurves {
                    class: K::Unsupported,
                },
                SM_ERR,
                K::Unsupported,
            ),
            (
                TopologyFailure::FaceNotMade {
                    class: K::InvalidInput,
                },
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::LoopSplitNoFace, SM_ERR, K::Failed),
            (TopologyFailure::LoopSplitFaceEarly, SM_ERR, K::Failed),
            (TopologyFailure::EdgeNotSplit, SM_ERR, K::Failed),
            (TopologyFailure::SheetReoriented, 1, K::Failed),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology(op, failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        // the trim curves' failure stays their cause, and decides defect or refusal
        let why = "a step the kernel got wrong";
        let cause = SolidError::topology(op, TopologyFailure::Defect { why });
        let e = Failure::new(op, TopologyFailure::UvTrimCurvesNotMade)
            .caused_by(cause.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&cause));
        assert!(e.is_defect());
        assert_eq!(e.upstream_status(), Some(SM_ERR));
    }

    /// The sites that build a topology phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_topology_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("TopologyFailure", "Failed"),
                Builder::Variant("TopologyFailure", "Missing"),
                Builder::Variant("TopologyFailure", "InvalidValue"),
                Builder::Variant("TopologyFailure", "Inconsistent"),
                Builder::Variant("TopologyFailure", "OutOfMemory"),
                Builder::Method("topology_failed", Some(1)),
                Builder::Method("topology_missing", Some(1)),
                Builder::Method("topology_invalid_value", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/topology/brep/cutting.rs", 11),
            ("solidcore/src/topology/brep/local_operation.rs", 5),
            ("solidcore/src/topology/brep/mirror.rs", 2),
            ("solidcore/src/topology/brep/p1.rs", 42),
            ("solidcore/src/topology/brep/p2.rs", 16),
            ("solidcore/src/topology/brep/planar_section.rs", 3),
            ("solidcore/src/topology/brep/sheet.rs", 4),
            ("solidcore/src/topology/brep/validation/assert_valid.rs", 1),
            ("solidcore/src/topology/brep/validation/assert_valid/seams.rs", 1),
            ("solidcore/src/topology/brep/validation/pointers.rs", 2),
            ("solidcore/src/topology/brep/vertex_fans.rs", 6),
        ];
        assert_pinned(
            "topology phrase-cause sites",
            &crate_sites(&PHRASES),
            PINNED,
        );
    }
}
