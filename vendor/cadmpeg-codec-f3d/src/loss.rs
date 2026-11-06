// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for Fusion `.f3d` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`F3dLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`F3dLossCode::note`] is the single practical construction path for a
//! [`LossNote`] in this crate: it fixes the loss category and severity from
//! the code so the two cannot drift apart across sites, and it leaves only
//! the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `f3d` namespace.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

/// A stable, machine-readable identifier for one Fusion `.f3d` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`F3dLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum F3dLossCode {
    /// A direct operation-table pose is applied uniformly to its definition.
    OperationPoseApplied,
    /// A scope namespace has no unique neutral product definition.
    ScopeOwnerUnresolved,
    /// A history output body belongs to another product definition.
    ScopeOutputOwnerMismatch,
    /// A bound sketch collection belongs to another product definition.
    ScopeSketchOwnerMismatch,
    /// Component-path constraints prevent a uniform operation-table pose.
    OperationPoseDisagrees,
    /// An operation table has no unique admitted neutral placement.
    OperationPoseNative,
    /// Payload-bearing Design dimension companions have no typed locus frame.
    DimensionCompanionUntyped,
    /// A whole-body reference tail has no admitted standard reference-run frame.
    BodyReferenceTailUntyped,
    /// A placement construction has no unambiguous registered member layout.
    PlacementConstructionUntyped,
    /// A work plane has no unique checked solved placement.
    WorkPlaneFrameUnbound,
    /// A cylinder primitive has no unique checked solved placement.
    CylinderPlacementUnbound,
    /// A construction tracking path has no matching indexed type registration.
    ConstructionTrackingPathUntyped,
    /// Design configuration JSON members have no assigned neutral semantics.
    ConfigurationMemberUnassigned,
    /// Design configuration rules have no unambiguous activation target.
    ConfigurationRuleUnbound,
    /// Design configuration parameter overrides have no parameter identity.
    ConfigurationParameterOverrideUnbound,
    /// Design configuration feature states have no feature identity.
    ConfigurationFeatureSuppressionUnbound,
    /// Design configuration root records name no active row of a unique table document.
    ConfigurationActiveRowUnbound,
    /// ACT component links have no resolved neutral product role.
    ActComponentLinkUnresolved,
    /// Local component occurrences whose container or definition the source does not settle.
    LocalOccurrenceUnplaced,
    /// A local occurrence owner joins no unique component definition.
    LocalOccurrenceOwnerUnbound,
    /// A solved body has no unique explicit component owner.
    ComponentBodyOwnerUnresolved,
    /// A body without explicit membership uses entity-cluster component ownership.
    ComponentBodyOwnerFallback,
    /// The component owner graph contains a cycle.
    LocalOccurrenceOwnerCycle,
    /// Repeated local instance trees exceed the decode limits.
    LocalOccurrenceExpansionLimit,
    /// A component feature names no unique expanded occurrence.
    FeatureOccurrencePathUnresolved,
    /// A sketch collection owner does not bind one component definition.
    SketchOwnerUnresolved,
    /// Unresolved `Component Insert` occurrences whose placement has no finite millimetre transform.
    ComponentInsertUnplaced,
    /// Unresolved `Component Insert` occurrences dropped because no authored timeline order tells
    /// top-level inserts from internal operands.
    ComponentInsertOrderUnknown,
    /// Component definitions stored as revised records that each bind a context
    /// UUID; the earliest record defines the component.
    ComponentDefinitionRevised,
    /// Registered component entities with no context-UUID binding in this
    /// document; their naming spaces are dropped.
    ComponentNamingSpaceUnbound,
    /// An empty-path root frame has no decoded component-definition reference.
    RootInstanceDefinitionUnbound,
    /// Alternate occurrence frames without a unique primary-indexed selection.
    LocalOccurrenceEnvelopeSuperseded,
    /// An identity carrier has an unresolved later ordinal.
    LocalOccurrenceIdentityOrdinalUnresolved,
    /// Saved free-edge bodies without a product-definition owner.
    SavedFreeEdgeBodyUnowned,
    /// Design assembly joint scopes whose operands, frames, limits, or alignment values do not
    /// project.
    AssemblyJointUnprojected,
    /// Partial assembly alignments with unresolved motion binding or operand paths.
    AssemblyAlignmentPartial,
    /// An ordinary rigid-group frame has no admitted grammar.
    AssemblyRigidGroupFrameUnparsed,
    /// Design assembly joints projected as fixed because their freedom codes or
    /// limit lanes do not map to a joint kind.
    AssemblyJointMotionUndecoded,
    /// Projected Design assembly joints whose stored rest is malformed or
    /// enabled. The neutral joint carries no rest; the native record retains
    /// the valid rest or the malformed-rest mark.
    AssemblyJointRestUnprojected,
    /// Rotation limits retained natively because the zero-lane freedom value is unsettled.
    AssemblyJointAngularLimitsNative,
    /// Projected Design assembly joints whose operand paths carry occurrence
    /// identity pairs of unknown effect on the joint, retained natively.
    AssemblyOperandPathPairsNative,
    /// Root operand identity GUIDs retained natively without a neutral interpretation.
    AssemblyRootPathIdentitiesNative,
    /// A projected joint operand descent selects multiple merged child occurrences.
    AssemblyOperandDescentAmbiguous,
    /// A complete joint occurrence path has no unique merged insertion instance.
    AssemblyOperandDescentUnresolved,
    /// An F3Z drawing root was omitted while its derived 3D model transferred.
    DrawingDocumentOmitted,
    /// ASM history estimated retained size exceeded the session's remaining retained-byte allowance.
    HistoryBindingBudgetExceeded,
    /// ASM history topology binding exceeded its local work cap.
    HistoryBindingWorkExceeded,
    /// A Design recipe side-sequence search exceeded its local work cap.
    RecipeSideSearchBudgetExceeded,
    /// Ordering one owner scope's Design parameters exceeded its local work cap.
    ParameterOrderBudgetExceeded,
    /// An ASM history span stays opaque because record framing failed.
    HistoryRecordFramingFailed,
    /// Design body-map pairs do not resolve to a body in the named BREP blob.
    DesignBodyBindingUnresolved,
    /// Design body-map pairs name a body outside the current model.
    HistoryOnlyBodyOmitted,
    /// Reference-image timeline objects retain native Canvas records.
    ReferenceImageNativeRetained,
    /// Decal timeline objects retain native image and mapping records.
    DecalNativeRetained,
    /// Source parametric edge references marked lost have no independent proof.
    EdgeReferenceLostUnrepaired,
    /// Feature scopes have no complete neutral feature definition.
    FeatureDefinitionIncomplete,
    /// A historical Thread selection has no complete proof of its live faces.
    ThreadLiveFaceUnproven,
    /// A stored-body operation has an empty proven body set.
    FeatureStoredBodiesEmpty,
    /// A decoded body snapshot supplies no construction recipe.
    FeatureStoredBodiesRecipeAbsent,
    /// A Base Feature has no complete unique body binding.
    FeatureStoredBodiesUnbound,
    /// A feature scope has no complete unique ASM output transition interval.
    FeatureOutputStateChainUnbound,
    /// An affected body slot has no unique active body in the selected BREP.
    FeatureOutputBodyUnbound,
    /// A derived base scope stores a copy of a source and frame that the archive does not contain.
    FeatureDerivedBaseUnresolved,
    /// A box scope has no fully decoded dimensions and local frame.
    FeatureBoxPrimitiveUnresolved,
    /// Sheet-metal base flanges projected with their stored thickness copy because
    /// their rule states no thickness.
    SheetThicknessUnruled,
    /// Decoded feature scopes have no neutral construction-history feature.
    FeatureScopeUnprojected,
    /// A Move scope has no neutral `MoveBody` feature.
    MoveUnprojected,
    /// Hole extent controls do not define an admitted termination.
    HoleExtentUnproven,
    /// A Hole carrier and its point-data records do not define an admitted placement.
    HolePlacementUnproven,
    /// Align operand roles and motion have no neutral binding.
    AlignOperandsUnresolved,
    /// Align has no admitted specialized prefix.
    AlignEnvelopeUnparsed,
    /// The referenced Align constructor has no admitted layout.
    AlignConstructionUnparsed,
    /// A decoded native feature envelope has no interpreted specialized payload.
    FeatureScopePayloadUninterpreted,
    /// Emboss controls, native pairs, and selected contours have no neutral mapping.
    EmbossControlsNative,
    /// An Emboss prefix matches no admitted native construction form.
    EmbossPrefixUnparsed,
    /// A non-equivalent duplicate scope envelope was superseded by the
    /// primary-indexed envelope.
    FeatureScopeEnvelopeSuperseded,
    /// Decoded Design parameters have no neutral parameter.
    ParameterUnprojected,
    /// Typed compact parameter frames match no parameter grammar.
    ParameterRecordUnparsed,
    /// Design parameter owner bindings have no recognized feature scope.
    ParameterOwnerUnrecognized,
    /// Design parameters retain owner frames that match no admitted grammar.
    ParameterOwnerUnparsed,
    /// Design parameters retain unit tokens without a settled quantity kind.
    ParameterUnitUntyped,
    /// Material Distance properties retain unit tags without a length conversion.
    MaterialDistanceUnitUntyped,
    /// A face material GUID names distinct visual tokens without a selection rule.
    FaceAppearanceAmbiguous,
    /// Parameter expression symbols name same-stream parameters without an edge.
    ParameterExpressionUnbound,
    /// Feature history-state dependency links were not projected.
    HistoryDependencyUnprojected,
    /// Feature history-state dependency links have multiple source scopes.
    HistoryDependencyAmbiguous,
    /// A historical face selection cannot bind uniquely to emitted input topology members.
    HistoricalFaceSelectionUnbound,
    /// Historical edge midpoint geometry could not be resolved from feature selections.
    HistoricalEdgeMidpointsUnresolved,
    /// Historical body bounding geometry could not be computed from point carriers.
    HistoricalBodyBoundsUnresolved,
    /// Sketch relations retain native operands; no unique neutral relation.
    SketchRelationNativeRetained,
    /// Sketch dimensions retain native operands; no unique neutral dimension.
    SketchDimensionNativeRetained,
    /// Decoded Sketch placements have no neutral sketch.
    SketchPlacementUnprojected,
    /// Decoded sketch points have no neutral sketch entity.
    SketchPointUnprojected,
    /// Decoded sketch curves have no neutral sketch entity.
    SketchCurveUnprojected,
    /// Decoded sketch surfaces have no neutral spatial sketch entity.
    SketchSurfaceUnprojected,
    /// Decoded sketch text records have no neutral sketch entity.
    SketchTextUnprojected,
    /// Sketch text retains style or orientation state absent from neutral text.
    SketchTextStyleUnprojected,
    /// Decoded sketch relations have no neutral constraint.
    SketchRelationUnprojected,
    /// Sketch patterns retain unrepresented symmetry, suppression or flag state.
    SketchPatternStateUnprojected,
    /// A spatial sketch's closed-profile search stopped at a local cap.
    SketchProfileSearchCapped,
    /// Non-construction sketch curves form no closed profile loop.
    SketchCurveUnprofiled,
    /// Design dimension parameters have no parameter-backed sketch constraint.
    DimensionUnprojected,
    /// Feature profile selections retain native identities; no unique profile.
    FeatureProfileSelectionNative,
    /// Multiple Sketch-profile operands retain an unresolved group composition.
    FeatureExtrudeProfileGroupNative,
    /// Extrude profile selections skipped the sketch arrangement search at a local cap.
    FeatureProfileArrangementCapped,
    /// Extrude profile selections skipped a certified profile test at a local cap.
    FeatureProfileDensificationCapped,
    /// Feature path selections retain native identities; no unique path.
    FeaturePathSelectionNative,
    /// Feature face selections retain native candidates; no unique face.
    FeatureFaceSelectionNative,
    /// Legacy face operands use a current active face after no historical slot proof.
    FeatureFaceSelectionActiveSubstituted,
    /// Feature body selections retain native identities; no unique body.
    FeatureBodySelectionNative,
    /// Combine body refinement violates operand arity or disjoint membership.
    FeatureCombineSelectionNative,
    /// Fixed Fillet scalar lanes have no complete group assignment.
    FeatureFixedFilletAssignmentNative,
    /// Extrude scope retains an unassigned extent payload.
    FeatureExtrudeExtentNative,
    /// Feature face operands stay unresolved inside historical selections.
    FeatureFaceOperandUnresolved,
    /// Edge-treatment selections retain native recipes; no historical edge.
    FeatureEdgeSelectionNative,
    /// Edge-treatment operands stay unresolved inside historical selections.
    FeatureEdgeOperandUnresolved,
    /// Edge-treatment selections are unresolved because source edges were lost.
    FeatureEdgeSelectionLost,
    /// A rectangular-pattern instance search stopped at a local cap.
    FeaturePatternInstanceSearchCapped,
    /// Design-referenced BREP blobs could not be decoded.
    BrepBlobUndecoded,
    /// A mesh geometry container decoded without a complete Design body join.
    MeshContainerUnjoined,
    /// A mesh geometry container was not decoded.
    MeshContainerUndecoded,
    /// A Design mesh body names a container that did not uniquely join.
    MeshContainerMissing,
    /// Mesh attribute channels have no settled stored-element layout.
    MeshAttributeNotTransferred,
    /// The external-reference table was present but not decoded.
    XrefTableUndecoded,
    /// A typed occurrence placement failed its frame grammar.
    XrefPlacementUndecoded,
    /// An external reference has no admitted occurrence placement.
    XrefOccurrenceUnplaced,
    /// An external reference keeps the identity fallback without placement proof.
    XrefPlacementIdentityAssumed,
    /// Legacy placement identities remain in their complete native frames.
    XrefPlacementMetadataNative,
    /// A legacy placement role has no containing reference-table entry.
    XrefPlacementRoleUnbound,
    /// Root external occurrences take their component-path carrier pose; the
    /// placement matrix stays native.
    XrefPlacementCurrentPose,
    /// An owned external occurrence keeps its placement matrix; its
    /// component-path carrier pose stays native.
    XrefCurrentPoseNative,
    /// Two or more component-path carriers name one placement; its placement
    /// matrix stays in place.
    XrefCurrentPoseAmbiguous,
    /// Local occurrences at the document root take their component-path
    /// carrier pose; the carrier's stored placement stays native.
    LocalOccurrenceCurrentPose,
    /// Owned local occurrences keep their stored placement; their
    /// component-path carrier pose stays native.
    LocalOccurrenceCurrentPoseNative,
    /// Two or more component-path carriers name one local occurrence; its
    /// stored placement stays in place.
    LocalOccurrenceCurrentPoseAmbiguous,
    /// Nested local occurrences take the world pose of their component-path
    /// carrier; the stored local placement is replaced.
    LocalOccurrencePathPose,
    /// Component-path carriers that name no unique expanded local occurrence
    /// are not applied.
    LocalOccurrencePathPoseUnresolved,
    /// Two component-path carriers give one local occurrence different
    /// poses; the stored placement stays in place.
    LocalOccurrencePathPoseDisagrees,
    /// Component-path carriers that each name several expanded local
    /// occurrences take the one assignment to distinct occurrences under which
    /// every definition-local pose agrees. The assignment is an inference;
    /// the bytes do not store it.
    LocalOccurrencePathPoseInferred,
    /// External occurrence instances take the pose their contained
    /// occurrences' component-path carriers store; the placement matrix stays
    /// native.
    XrefContainedPose,
    /// The contained occurrences' component-path carriers of an instance give
    /// different poses; it keeps its placement.
    XrefContainedPoseDisagrees,
    /// Contained-occurrence component-path carriers name no merged instance
    /// with their owner chain.
    XrefContainedPoseNative,
    /// External occurrence instances below local owners take the pose their own
    /// component-path carriers store; the placement matrix stays native.
    XrefOwnedPose,
    /// An instance's own component-path carriers give different poses; it keeps
    /// its placement.
    XrefOwnedPoseDisagrees,
    /// Own component-path carriers that place no instance stay native.
    XrefOwnedPoseNative,
    /// A Design stream has no type and record index for placement admission.
    XrefPlacementMetadataMissing,
    /// Structured placement records were superseded by scope-bound carriers.
    XrefPlacementSuperseded,
    /// An external occurrence owner has no unique local component instance.
    XrefOwnerUnresolved,
    /// Inserted-design occurrences that store the identity and are placed by
    /// their joints; the joint-solved pose is not computed.
    JointPlacedOccurrenceUnsolved,
    /// Mesh body geometry stores vertex coordinates at f32 precision.
    MeshVertexPrecisionReduced,
    /// A bodyless design's sketches or reference images are its complete carrier.
    BodylessDesignCarrier,
    /// An assembly document's geometry lives in external references.
    AssemblyComponentsExternal,
    /// Spline surface records were decoded into NURBS carriers from cache.
    NurbsSurfaceCarrier,
    /// Procedural curve records were decoded into NURBS carriers from cache.
    NurbsCurveCarrier,
    /// Faces were omitted because their required surface reference was dangling.
    FaceSurfaceReferenceDangling,
    /// Faces rest on spline or procedural surfaces whose shape was not decoded.
    SurfaceShapeNotDecoded,
    /// Faces use zero-payload `mesh_surface` sentinels; exact surfaces are absent.
    MeshSurfaceSentinel,
    /// Sketch-region body records of a keyless BREP blob are not model bodies.
    SketchRegionBodyOmitted,
    /// Edges reference a procedural 3D curve with no decodable B-spline cache.
    ProceduralCurveUndecoded,
    /// Vertex tolerance is widened to cover exact cache endpoints.
    VertexToleranceWidened,
    /// A cache-first spring support pcurve has no admitted endpoint relation.
    SpringSupportPcurveUnresolved,
    /// A native support pcurve has no surface in its construction chart.
    ProceduralSupportChartUnresolved,
    /// Coedges carry a UV pcurve reference with no decodable 2D carrier.
    PcurveUndecoded,
    /// Rolling-ball blend definitions resolved only one of two native supports.
    BlendSupportPartial,
    /// Solved-record application or refinement records were not transferred.
    SolvedRecordUntyped,
    /// Materials and appearances were not transferred.
    MaterialNotTransferred,
    /// ASM BREP geometry was not transferred from the container.
    GeometryNotTransferred,
    /// B-rep topology graph was not built from the container.
    TopologyNotTransferred,
    /// No ASM BREP stream was found or selected as the document geometry stream.
    MissingGeometryStream,
    /// An `XREF` path cycles through a member already on the resolution stack.
    XrefCycle,
    /// An `XREF` member is not present in the archive.
    XrefMemberMissing,
    /// An `XREF` member failed to decode.
    XrefMemberUndecoded,
    /// T-spline records were retained without typed semantics.
    TsplineRecordUntyped,
    /// Radial symmetry pairs have no unambiguous neutral topology correspondence.
    TsplineRadialMapUnresolved,
    /// A T-spline control cage was not decoded.
    TsplineCageUndecoded,
    /// Preserved source image required for a byte-exact write was unavailable.
    SourcePreservedImageUnavailable,
    /// The document was read with a grammar its own declarations do not select.
    SourceDialectUnverified,
    /// Export omits records named by the counted loss message.
    WriterAppearancePropertiesOmitted,
    /// The selected write target differs from the same-format source dialect.
    SourceDialectDisplaced,
    /// An embedded kernel carrier was read with an unverified ACIS grammar.
    KernelDialectUnverified,
    /// An embedded kernel carrier could not be framed for dialect inspection.
    KernelCarrierUnparseable,
    /// Two F3Z member layers resolved to the same core dialect identity.
    DialectLayerCollision,
    /// A framed text B-rep carries malformed header metadata or tolerances.
    TextHeaderNoncanonical,
    /// An optional kernel tolerance cannot enter the canonical document units.
    KernelHeaderToleranceUnresolved,
    /// An independently framed binary kernel product string is unreadable.
    KernelHeaderMetadataUnresolved,
    /// A bounded descriptive manifest payload is unreadable.
    ManifestMetadataUnresolved,
    /// An optional bounded archive payload is unreadable.
    ArchiveEntryUnreadable,
    /// A Design `BulkStream` stores a record run after its last indexed
    /// record; the run is not decoded and its bytes are retained natively.
    UnindexedRecordsRetained,
    /// Design-intent decoding failed; its sketches, features and history were dropped.
    DesignIntentUndecoded,
}

impl F3dLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [F3dLossCode] = &[
        Self::OperationPoseApplied,
        Self::ScopeOwnerUnresolved,
        Self::ScopeOutputOwnerMismatch,
        Self::ScopeSketchOwnerMismatch,
        Self::OperationPoseDisagrees,
        Self::OperationPoseNative,
        Self::DimensionCompanionUntyped,
        Self::BodyReferenceTailUntyped,
        Self::PlacementConstructionUntyped,
        Self::WorkPlaneFrameUnbound,
        Self::CylinderPlacementUnbound,
        Self::ConstructionTrackingPathUntyped,
        Self::ConfigurationMemberUnassigned,
        Self::ConfigurationRuleUnbound,
        Self::ConfigurationParameterOverrideUnbound,
        Self::ConfigurationFeatureSuppressionUnbound,
        Self::ConfigurationActiveRowUnbound,
        Self::ActComponentLinkUnresolved,
        Self::LocalOccurrenceUnplaced,
        Self::LocalOccurrenceOwnerUnbound,
        Self::ComponentBodyOwnerUnresolved,
        Self::ComponentBodyOwnerFallback,
        Self::LocalOccurrenceOwnerCycle,
        Self::LocalOccurrenceExpansionLimit,
        Self::FeatureOccurrencePathUnresolved,
        Self::SketchOwnerUnresolved,
        Self::ComponentInsertUnplaced,
        Self::ComponentInsertOrderUnknown,
        Self::ComponentDefinitionRevised,
        Self::ComponentNamingSpaceUnbound,
        Self::RootInstanceDefinitionUnbound,
        Self::LocalOccurrenceEnvelopeSuperseded,
        Self::LocalOccurrenceIdentityOrdinalUnresolved,
        Self::SavedFreeEdgeBodyUnowned,
        Self::AssemblyJointUnprojected,
        Self::AssemblyAlignmentPartial,
        Self::AssemblyRigidGroupFrameUnparsed,
        Self::AssemblyJointMotionUndecoded,
        Self::AssemblyJointRestUnprojected,
        Self::AssemblyJointAngularLimitsNative,
        Self::AssemblyOperandPathPairsNative,
        Self::AssemblyRootPathIdentitiesNative,
        Self::AssemblyOperandDescentAmbiguous,
        Self::AssemblyOperandDescentUnresolved,
        Self::DrawingDocumentOmitted,
        Self::HistoryBindingBudgetExceeded,
        Self::HistoryBindingWorkExceeded,
        Self::RecipeSideSearchBudgetExceeded,
        Self::ParameterOrderBudgetExceeded,
        Self::HistoryRecordFramingFailed,
        Self::DesignBodyBindingUnresolved,
        Self::HistoryOnlyBodyOmitted,
        Self::ReferenceImageNativeRetained,
        Self::DecalNativeRetained,
        Self::EdgeReferenceLostUnrepaired,
        Self::FeatureDefinitionIncomplete,
        Self::ThreadLiveFaceUnproven,
        Self::FeatureStoredBodiesEmpty,
        Self::FeatureStoredBodiesRecipeAbsent,
        Self::FeatureStoredBodiesUnbound,
        Self::FeatureOutputStateChainUnbound,
        Self::FeatureOutputBodyUnbound,
        Self::FeatureDerivedBaseUnresolved,
        Self::FeatureBoxPrimitiveUnresolved,
        Self::SheetThicknessUnruled,
        Self::FeatureScopeUnprojected,
        Self::MoveUnprojected,
        Self::HoleExtentUnproven,
        Self::HolePlacementUnproven,
        Self::AlignOperandsUnresolved,
        Self::AlignEnvelopeUnparsed,
        Self::AlignConstructionUnparsed,
        Self::FeatureScopePayloadUninterpreted,
        Self::EmbossControlsNative,
        Self::EmbossPrefixUnparsed,
        Self::FeatureScopeEnvelopeSuperseded,
        Self::ParameterUnprojected,
        Self::ParameterRecordUnparsed,
        Self::ParameterOwnerUnrecognized,
        Self::ParameterOwnerUnparsed,
        Self::ParameterUnitUntyped,
        Self::MaterialDistanceUnitUntyped,
        Self::FaceAppearanceAmbiguous,
        Self::ParameterExpressionUnbound,
        Self::HistoryDependencyUnprojected,
        Self::HistoryDependencyAmbiguous,
        Self::HistoricalFaceSelectionUnbound,
        Self::HistoricalEdgeMidpointsUnresolved,
        Self::HistoricalBodyBoundsUnresolved,
        Self::SketchRelationNativeRetained,
        Self::SketchDimensionNativeRetained,
        Self::SketchPlacementUnprojected,
        Self::SketchPointUnprojected,
        Self::SketchCurveUnprojected,
        Self::SketchSurfaceUnprojected,
        Self::SketchTextUnprojected,
        Self::SketchTextStyleUnprojected,
        Self::SketchRelationUnprojected,
        Self::SketchPatternStateUnprojected,
        Self::SketchProfileSearchCapped,
        Self::SketchCurveUnprofiled,
        Self::DimensionUnprojected,
        Self::FeatureProfileSelectionNative,
        Self::FeatureExtrudeProfileGroupNative,
        Self::FeatureProfileArrangementCapped,
        Self::FeatureProfileDensificationCapped,
        Self::FeaturePathSelectionNative,
        Self::FeatureFaceSelectionNative,
        Self::FeatureFaceSelectionActiveSubstituted,
        Self::FeatureBodySelectionNative,
        Self::FeatureCombineSelectionNative,
        Self::FeatureFixedFilletAssignmentNative,
        Self::FeatureExtrudeExtentNative,
        Self::FeatureFaceOperandUnresolved,
        Self::FeatureEdgeSelectionNative,
        Self::FeatureEdgeOperandUnresolved,
        Self::FeatureEdgeSelectionLost,
        Self::FeaturePatternInstanceSearchCapped,
        Self::BrepBlobUndecoded,
        Self::MeshContainerUnjoined,
        Self::MeshContainerUndecoded,
        Self::MeshContainerMissing,
        Self::MeshAttributeNotTransferred,
        Self::XrefTableUndecoded,
        Self::XrefPlacementUndecoded,
        Self::XrefOccurrenceUnplaced,
        Self::XrefPlacementIdentityAssumed,
        Self::XrefPlacementMetadataNative,
        Self::XrefPlacementRoleUnbound,
        Self::XrefPlacementCurrentPose,
        Self::XrefCurrentPoseNative,
        Self::XrefCurrentPoseAmbiguous,
        Self::LocalOccurrenceCurrentPose,
        Self::LocalOccurrenceCurrentPoseNative,
        Self::LocalOccurrenceCurrentPoseAmbiguous,
        Self::LocalOccurrencePathPose,
        Self::LocalOccurrencePathPoseUnresolved,
        Self::LocalOccurrencePathPoseDisagrees,
        Self::LocalOccurrencePathPoseInferred,
        Self::XrefContainedPose,
        Self::XrefContainedPoseDisagrees,
        Self::XrefContainedPoseNative,
        Self::XrefOwnedPose,
        Self::XrefOwnedPoseDisagrees,
        Self::XrefOwnedPoseNative,
        Self::XrefPlacementMetadataMissing,
        Self::XrefPlacementSuperseded,
        Self::XrefOwnerUnresolved,
        Self::JointPlacedOccurrenceUnsolved,
        Self::MeshVertexPrecisionReduced,
        Self::BodylessDesignCarrier,
        Self::AssemblyComponentsExternal,
        Self::NurbsSurfaceCarrier,
        Self::SketchRegionBodyOmitted,
        Self::NurbsCurveCarrier,
        Self::FaceSurfaceReferenceDangling,
        Self::SurfaceShapeNotDecoded,
        Self::MeshSurfaceSentinel,
        Self::ProceduralCurveUndecoded,
        Self::VertexToleranceWidened,
        Self::SpringSupportPcurveUnresolved,
        Self::ProceduralSupportChartUnresolved,
        Self::PcurveUndecoded,
        Self::BlendSupportPartial,
        Self::SolvedRecordUntyped,
        Self::MaterialNotTransferred,
        Self::GeometryNotTransferred,
        Self::TopologyNotTransferred,
        Self::MissingGeometryStream,
        Self::XrefCycle,
        Self::XrefMemberMissing,
        Self::XrefMemberUndecoded,
        Self::TsplineRecordUntyped,
        Self::TsplineRadialMapUnresolved,
        Self::TsplineCageUndecoded,
        Self::SourcePreservedImageUnavailable,
        Self::SourceDialectUnverified,
        Self::WriterAppearancePropertiesOmitted,
        Self::SourceDialectDisplaced,
        Self::KernelDialectUnverified,
        Self::KernelCarrierUnparseable,
        Self::DialectLayerCollision,
        Self::TextHeaderNoncanonical,
        Self::KernelHeaderToleranceUnresolved,
        Self::KernelHeaderMetadataUnresolved,
        Self::ManifestMetadataUnresolved,
        Self::ArchiveEntryUnreadable,
        Self::UnindexedRecordsRetained,
        Self::DesignIntentUndecoded,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::ScopeOwnerUnresolved => "scope.owner-unresolved",
            Self::ScopeOutputOwnerMismatch => "scope.output-owner-mismatch",
            Self::ScopeSketchOwnerMismatch => "scope.sketch-owner-mismatch",
            Self::OperationPoseApplied => "assembly.operation-table-pose",
            Self::OperationPoseDisagrees => "assembly.operation-table-pose-disagrees",
            Self::OperationPoseNative => "assembly.operation-table-pose-native",
            Self::DimensionCompanionUntyped => "dimension.companion-untyped",
            Self::BodyReferenceTailUntyped => "body-reference.tail-untyped",
            Self::PlacementConstructionUntyped => "placement.construction-untyped",
            Self::WorkPlaneFrameUnbound => "work-plane.frame-unbound",
            Self::CylinderPlacementUnbound => "feature.cylinder-placement-unbound",
            Self::ConstructionTrackingPathUntyped => "feature.tracking-path-untyped",
            Self::ConfigurationMemberUnassigned => "configuration.member-unassigned",
            Self::ConfigurationRuleUnbound => "configuration.rule-unbound",
            Self::ConfigurationParameterOverrideUnbound => {
                "configuration.parameter-override-unbound"
            }
            Self::ConfigurationFeatureSuppressionUnbound => {
                "configuration.feature-suppression-unbound"
            }
            Self::ConfigurationActiveRowUnbound => "configuration.active-row-unbound",
            Self::ActComponentLinkUnresolved => "assembly.act-component-link-unresolved",
            Self::LocalOccurrenceUnplaced => "assembly.local-occurrence-unplaced",
            Self::LocalOccurrenceOwnerUnbound => "assembly.local-occurrence-owner-unbound",
            Self::ComponentBodyOwnerFallback => "assembly.component-body-owner-fallback",
            Self::ComponentBodyOwnerUnresolved => "assembly.component-body-owner-unresolved",
            Self::LocalOccurrenceOwnerCycle => "assembly.local-occurrence-owner-cycle",
            Self::LocalOccurrenceExpansionLimit => "assembly.local-occurrence-expansion-limit",
            Self::FeatureOccurrencePathUnresolved => "feature.occurrence-path-unresolved",
            Self::SketchOwnerUnresolved => "sketch.component-owner-unresolved",
            Self::ComponentInsertUnplaced => "assembly.component-insert-unplaced",
            Self::ComponentInsertOrderUnknown => "assembly.component-insert-order-unknown",
            Self::ComponentDefinitionRevised => "assembly.component-definition-revised",
            Self::ComponentNamingSpaceUnbound => "assembly.component-naming-space-unbound",
            Self::RootInstanceDefinitionUnbound => "assembly.root-instance-definition-unbound",
            Self::SavedFreeEdgeBodyUnowned => "assembly.saved-free-edge-body-unowned",
            Self::LocalOccurrenceIdentityOrdinalUnresolved => {
                "assembly.local-occurrence-identity-ordinal-unresolved"
            }
            Self::LocalOccurrenceEnvelopeSuperseded => {
                "assembly.local-occurrence-envelope-superseded"
            }
            Self::AssemblyAlignmentPartial => "assembly.alignment-partial",
            Self::AssemblyJointUnprojected => "assembly.joint-unprojected",
            Self::AssemblyRigidGroupFrameUnparsed => "assembly.rigid-group-frame-unparsed",
            Self::AssemblyJointMotionUndecoded => "assembly.joint-motion-undecoded",
            Self::AssemblyJointRestUnprojected => "assembly.joint-rest-unprojected",
            Self::AssemblyJointAngularLimitsNative => "assembly.joint-angular-limits-native",
            Self::AssemblyOperandPathPairsNative => "assembly.operand-path-pairs-native",
            Self::AssemblyRootPathIdentitiesNative => "assembly.root-path-identities-native",
            Self::AssemblyOperandDescentAmbiguous => "assembly.operand-descent-ambiguous",
            Self::AssemblyOperandDescentUnresolved => "assembly.operand-descent-unresolved",
            Self::DrawingDocumentOmitted => "drawing.document-omitted",
            Self::HistoryBindingBudgetExceeded => "history.binding-budget-exceeded",
            Self::HistoryBindingWorkExceeded => "history.binding-work-exceeded",
            Self::RecipeSideSearchBudgetExceeded => "feature.recipe-search-budget-exceeded",
            Self::ParameterOrderBudgetExceeded => "parameter.order-budget-exceeded",
            Self::HistoryRecordFramingFailed => "history.record-framing-failed",
            Self::DesignBodyBindingUnresolved => "design.body-binding-unresolved",
            Self::HistoryOnlyBodyOmitted => "design.history-only-body-omitted",
            Self::ReferenceImageNativeRetained => "appearance.reference-image-native",
            Self::DecalNativeRetained => "appearance.decal-native",
            Self::EdgeReferenceLostUnrepaired => "feature.edge-reference-lost",
            Self::FeatureStoredBodiesEmpty => "feature.stored-bodies-empty",
            Self::FeatureStoredBodiesRecipeAbsent => "feature.stored-bodies-recipe-absent",
            Self::FeatureStoredBodiesUnbound => "feature.stored-bodies-unbound",
            Self::FeatureOutputStateChainUnbound => "feature.output-state-chain-unbound",
            Self::FeatureOutputBodyUnbound => "feature.output-body-unbound",
            Self::FeatureDerivedBaseUnresolved => "feature.derived-base-unresolved",
            Self::FeatureBoxPrimitiveUnresolved => "feature.box-primitive-unresolved",
            Self::FeatureDefinitionIncomplete => "feature.definition-incomplete",
            Self::ThreadLiveFaceUnproven => "feature.thread-live-face-unproven",
            Self::SheetThicknessUnruled => "feature.sheet-thickness-unruled",
            Self::EmbossControlsNative => "feature.emboss-controls-native",
            Self::EmbossPrefixUnparsed => "feature.emboss-prefix-unparsed",
            Self::AlignOperandsUnresolved => "feature.align-operands-unresolved",
            Self::AlignEnvelopeUnparsed => "feature.align-envelope-unparsed",
            Self::AlignConstructionUnparsed => "feature.align-construction-unparsed",
            Self::FeatureScopePayloadUninterpreted => "feature.scope-payload-uninterpreted",
            Self::FeatureScopeEnvelopeSuperseded => "feature.scope-envelope-superseded",
            Self::HoleExtentUnproven => "feature.hole-extent-unproven",
            Self::HolePlacementUnproven => "feature.hole-placement-unproven",
            Self::FeatureScopeUnprojected => "feature.scope-unprojected",
            Self::MoveUnprojected => "feature.move-unprojected",
            Self::ParameterUnprojected => "parameter.unprojected",
            Self::ParameterRecordUnparsed => "parameter.record-unparsed",
            Self::ParameterOwnerUnrecognized => "parameter.owner-unrecognized",
            Self::ParameterOwnerUnparsed => "parameter.owner-unparsed",
            Self::ParameterUnitUntyped => "parameter.unit-untyped",
            Self::MaterialDistanceUnitUntyped => "material.distance-unit-untyped",
            Self::FaceAppearanceAmbiguous => "material.face-appearance-ambiguous",
            Self::ParameterExpressionUnbound => "parameter.expression-unbound",
            Self::HistoryDependencyUnprojected => "history.dependency-unprojected",
            Self::HistoricalFaceSelectionUnbound => "history.face-selection-unbound",
            Self::HistoricalEdgeMidpointsUnresolved => "history.edge-midpoints-unresolved",
            Self::HistoricalBodyBoundsUnresolved => "history.body-bounds-unresolved",
            Self::HistoryDependencyAmbiguous => "history.dependency-ambiguous",
            Self::SketchRelationNativeRetained => "sketch.relation-native",
            Self::SketchDimensionNativeRetained => "sketch.dimension-native",
            Self::SketchPlacementUnprojected => "sketch.placement-unprojected",
            Self::SketchPointUnprojected => "sketch.point-unprojected",
            Self::SketchCurveUnprojected => "sketch.curve-unprojected",
            Self::SketchSurfaceUnprojected => "sketch.surface-unprojected",
            Self::SketchTextUnprojected => "sketch.text-unprojected",
            Self::SketchTextStyleUnprojected => "sketch.text-style-unprojected",
            Self::SketchRelationUnprojected => "sketch.relation-unprojected",
            Self::SketchPatternStateUnprojected => "sketch.pattern-state-unprojected",
            Self::SketchProfileSearchCapped => "sketch.profile-search-capped",
            Self::SketchCurveUnprofiled => "sketch.curve-unprofiled",
            Self::DimensionUnprojected => "dimension.unprojected",
            Self::FeatureExtrudeProfileGroupNative => "feature.extrude-profile-group-native",
            Self::FeatureProfileSelectionNative => "feature.profile-selection-native",
            Self::FeatureProfileArrangementCapped => "feature.profile-arrangement-capped",
            Self::FeatureProfileDensificationCapped => "feature.profile-densification-capped",
            Self::FeaturePathSelectionNative => "feature.path-selection-native",
            Self::FeatureFaceSelectionNative => "feature.face-selection-native",
            Self::FeatureFaceSelectionActiveSubstituted => {
                "feature.face-selection-active-substituted"
            }
            Self::FeatureBodySelectionNative => "feature.body-selection-native",
            Self::FeatureCombineSelectionNative => "feature.combine-selection-native",
            Self::FeatureFixedFilletAssignmentNative => "feature.fixed-fillet-assignment-native",
            Self::FeatureExtrudeExtentNative => "feature.extrude-extent-native",
            Self::FeatureFaceOperandUnresolved => "feature.face-operand-unresolved",
            Self::FeatureEdgeSelectionNative => "feature.edge-selection-native",
            Self::FeatureEdgeOperandUnresolved => "feature.edge-operand-unresolved",
            Self::FeatureEdgeSelectionLost => "feature.edge-selection-lost",
            Self::FeaturePatternInstanceSearchCapped => "feature.pattern-instance-search-capped",
            Self::BrepBlobUndecoded => "geometry.brep-blob-undecoded",
            Self::MeshContainerUnjoined => "mesh.container-unjoined",
            Self::MeshContainerUndecoded => "mesh.container-undecoded",
            Self::MeshContainerMissing => "mesh.container-missing",
            Self::MeshAttributeNotTransferred => "mesh.attribute-not-transferred",
            Self::XrefTableUndecoded => "xref.table-undecoded",
            Self::XrefPlacementUndecoded => "xref.placement-undecoded",
            Self::XrefOccurrenceUnplaced => "xref.occurrence-unplaced",
            Self::XrefPlacementIdentityAssumed => "xref.placement-identity-assumed",
            Self::XrefPlacementMetadataNative => "xref.placement-metadata-native",
            Self::XrefPlacementRoleUnbound => "xref.placement-role-unbound",
            Self::XrefPlacementCurrentPose => "xref.placement-current-pose",
            Self::XrefCurrentPoseNative => "xref.current-pose-native",
            Self::XrefCurrentPoseAmbiguous => "xref.current-pose-ambiguous",
            Self::LocalOccurrenceCurrentPose => "assembly.local-occurrence-current-pose",
            Self::LocalOccurrenceCurrentPoseNative => {
                "assembly.local-occurrence-current-pose-native"
            }
            Self::LocalOccurrenceCurrentPoseAmbiguous => {
                "assembly.local-occurrence-current-pose-ambiguous"
            }
            Self::LocalOccurrencePathPose => "assembly.local-occurrence-path-pose",
            Self::LocalOccurrencePathPoseUnresolved => {
                "assembly.local-occurrence-path-pose-unresolved"
            }
            Self::LocalOccurrencePathPoseDisagrees => {
                "assembly.local-occurrence-path-pose-disagrees"
            }
            Self::LocalOccurrencePathPoseInferred => "assembly.local-occurrence-path-pose-inferred",
            Self::XrefContainedPose => "xref.contained-pose",
            Self::XrefContainedPoseDisagrees => "xref.contained-pose-disagrees",
            Self::XrefContainedPoseNative => "xref.contained-pose-native",
            Self::XrefOwnedPose => "xref.owned-pose",
            Self::XrefOwnedPoseDisagrees => "xref.owned-pose-disagrees",
            Self::XrefOwnedPoseNative => "xref.owned-pose-native",
            Self::XrefPlacementMetadataMissing => "xref.placement-metadata-missing",
            Self::XrefPlacementSuperseded => "xref.placement-superseded",
            Self::XrefOwnerUnresolved => "xref.owner-unresolved",
            Self::JointPlacedOccurrenceUnsolved => "assembly.joint-placed-occurrence-unsolved",
            Self::MeshVertexPrecisionReduced => "mesh.vertex-precision-reduced",
            Self::BodylessDesignCarrier => "design.bodyless-carrier",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::NurbsSurfaceCarrier => "geometry.nurbs-surface-carrier",
            Self::SketchRegionBodyOmitted => "geometry.sketch-region-body-omitted",
            Self::NurbsCurveCarrier => "geometry.nurbs-curve-carrier",
            Self::FaceSurfaceReferenceDangling => "geometry.face-surface-dangling",
            Self::SurfaceShapeNotDecoded => "geometry.surface-shape-not-decoded",
            Self::MeshSurfaceSentinel => "geometry.mesh-surface-sentinel",
            Self::ProceduralCurveUndecoded => "geometry.procedural-curve-undecoded",
            Self::VertexToleranceWidened => "geometry.vertex-tolerance-widened",
            Self::ProceduralSupportChartUnresolved => {
                "geometry.procedural-support-chart-unresolved"
            }
            Self::SpringSupportPcurveUnresolved => "geometry.spring-support-pcurve-unresolved",
            Self::PcurveUndecoded => "geometry.pcurve-undecoded",
            Self::BlendSupportPartial => "geometry.blend-support-partial",
            Self::SolvedRecordUntyped => "geometry.solved-record-untyped",
            Self::MaterialNotTransferred => "material.not-transferred",
            Self::GeometryNotTransferred => "geometry.not-transferred",
            Self::TopologyNotTransferred => "topology.not-transferred",
            Self::MissingGeometryStream => "container.missing-geometry-stream",
            Self::XrefCycle => "xref.cycle",
            Self::XrefMemberMissing => "xref.member-missing",
            Self::XrefMemberUndecoded => "xref.member-undecoded",
            Self::TsplineRecordUntyped => "tspline.record-untyped",
            Self::TsplineRadialMapUnresolved => "tspline.radial-map-unresolved",
            Self::TsplineCageUndecoded => "tspline.cage-undecoded",
            Self::SourcePreservedImageUnavailable => "source.preserved-image-unavailable",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::WriterAppearancePropertiesOmitted => "writer.appearance-properties-omitted",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::KernelCarrierUnparseable => "source.kernel-carrier-unparseable",
            Self::DialectLayerCollision => "source.dialect-layer-collision",
            Self::TextHeaderNoncanonical => "brep.text-header-noncanonical",
            Self::KernelHeaderToleranceUnresolved => "brep.header-tolerance-unresolved",
            Self::KernelHeaderMetadataUnresolved => "brep.header-metadata-unresolved",
            Self::ManifestMetadataUnresolved => "source.manifest-metadata-unresolved",
            Self::ArchiveEntryUnreadable => "source.archive-entry-unreadable",
            Self::UnindexedRecordsRetained => "source.unindexed-records-retained",
            Self::DesignIntentUndecoded => "design.intent-undecoded",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::ScopeOutputOwnerMismatch | Self::ScopeSketchOwnerMismatch => Severity::Warning,
            Self::WriterAppearancePropertiesOmitted => Severity::Warning,
            Self::ScopeOwnerUnresolved
            | Self::OperationPoseApplied
            | Self::OperationPoseNative
            | Self::BodylessDesignCarrier
            | Self::XrefPlacementMetadataNative
            | Self::XrefPlacementCurrentPose
            | Self::XrefCurrentPoseNative
            | Self::LocalOccurrenceCurrentPose
            | Self::LocalOccurrenceCurrentPoseNative
            | Self::XrefContainedPose
            | Self::XrefContainedPoseNative
            | Self::XrefOwnedPose
            | Self::XrefOwnedPoseNative
            | Self::AssemblyComponentsExternal
            | Self::NurbsSurfaceCarrier
            | Self::NurbsCurveCarrier
            | Self::MeshSurfaceSentinel
            | Self::SketchRegionBodyOmitted
            | Self::ThreadLiveFaceUnproven => Severity::Info,
            Self::HistoryBindingBudgetExceeded
            | Self::HistoryBindingWorkExceeded
            | Self::RecipeSideSearchBudgetExceeded
            | Self::ParameterOrderBudgetExceeded
            | Self::HistoryRecordFramingFailed
            | Self::MeshContainerUndecoded
            | Self::MissingGeometryStream
            | Self::XrefCycle
            | Self::XrefMemberMissing
            | Self::XrefMemberUndecoded
            | Self::TsplineCageUndecoded
            | Self::DesignIntentUndecoded => Severity::Error,
            Self::GeometryNotTransferred
            | Self::TopologyNotTransferred
            | Self::SourcePreservedImageUnavailable => Severity::Blocking,
            Self::OperationPoseDisagrees
            | Self::DimensionCompanionUntyped
            | Self::BodyReferenceTailUntyped
            | Self::WorkPlaneFrameUnbound
            | Self::CylinderPlacementUnbound
            | Self::PlacementConstructionUntyped
            | Self::ConstructionTrackingPathUntyped
            | Self::ConfigurationMemberUnassigned
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::ConfigurationActiveRowUnbound
            | Self::ActComponentLinkUnresolved
            | Self::ComponentBodyOwnerUnresolved
            | Self::ComponentBodyOwnerFallback
            | Self::LocalOccurrenceOwnerUnbound
            | Self::LocalOccurrenceOwnerCycle
            | Self::LocalOccurrenceExpansionLimit
            | Self::FeatureOccurrencePathUnresolved
            | Self::LocalOccurrenceUnplaced
            | Self::SketchOwnerUnresolved
            | Self::ComponentInsertUnplaced
            | Self::ComponentInsertOrderUnknown
            | Self::SavedFreeEdgeBodyUnowned
            | Self::LocalOccurrenceIdentityOrdinalUnresolved
            | Self::LocalOccurrenceEnvelopeSuperseded
            | Self::RootInstanceDefinitionUnbound
            | Self::ComponentDefinitionRevised
            | Self::ComponentNamingSpaceUnbound
            | Self::AssemblyAlignmentPartial
            | Self::AssemblyJointUnprojected
            | Self::AssemblyRigidGroupFrameUnparsed
            | Self::AssemblyJointMotionUndecoded
            | Self::AssemblyJointAngularLimitsNative
            | Self::AssemblyJointRestUnprojected
            | Self::AssemblyOperandPathPairsNative
            | Self::AssemblyOperandDescentUnresolved
            | Self::AssemblyOperandDescentAmbiguous
            | Self::AssemblyRootPathIdentitiesNative
            | Self::DrawingDocumentOmitted
            | Self::DesignBodyBindingUnresolved
            | Self::HistoryOnlyBodyOmitted
            | Self::ReferenceImageNativeRetained
            | Self::DecalNativeRetained
            | Self::EdgeReferenceLostUnrepaired
            | Self::FeatureStoredBodiesEmpty
            | Self::FeatureStoredBodiesRecipeAbsent
            | Self::FeatureOutputBodyUnbound
            | Self::FeatureOutputStateChainUnbound
            | Self::FeatureStoredBodiesUnbound
            | Self::FeatureDerivedBaseUnresolved
            | Self::FeatureBoxPrimitiveUnresolved
            | Self::FeatureDefinitionIncomplete
            | Self::SheetThicknessUnruled
            | Self::HoleExtentUnproven
            | Self::HolePlacementUnproven
            | Self::FeatureScopeUnprojected
            | Self::EmbossControlsNative
            | Self::EmbossPrefixUnparsed
            | Self::AlignOperandsUnresolved
            | Self::AlignEnvelopeUnparsed
            | Self::AlignConstructionUnparsed
            | Self::MoveUnprojected
            | Self::FeatureScopePayloadUninterpreted
            | Self::FeatureScopeEnvelopeSuperseded
            | Self::ParameterUnprojected
            | Self::ParameterOwnerUnparsed
            | Self::ParameterRecordUnparsed
            | Self::ParameterOwnerUnrecognized
            | Self::ParameterUnitUntyped
            | Self::MaterialDistanceUnitUntyped
            | Self::FaceAppearanceAmbiguous
            | Self::ParameterExpressionUnbound
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::HistoricalEdgeMidpointsUnresolved
            | Self::HistoricalBodyBoundsUnresolved
            | Self::SketchRelationNativeRetained
            | Self::SketchDimensionNativeRetained
            | Self::SketchPlacementUnprojected
            | Self::SketchPointUnprojected
            | Self::SketchCurveUnprojected
            | Self::SketchSurfaceUnprojected
            | Self::SketchTextUnprojected
            | Self::SketchTextStyleUnprojected
            | Self::SketchRelationUnprojected
            | Self::SketchPatternStateUnprojected
            | Self::SketchProfileSearchCapped
            | Self::SketchCurveUnprofiled
            | Self::DimensionUnprojected
            | Self::FeatureProfileSelectionNative
            | Self::FeatureExtrudeProfileGroupNative
            | Self::FeatureProfileArrangementCapped
            | Self::FeatureProfileDensificationCapped
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureBodySelectionNative
            | Self::FeatureCombineSelectionNative
            | Self::FeatureFixedFilletAssignmentNative
            | Self::FeatureExtrudeExtentNative
            | Self::FeatureFaceOperandUnresolved
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost
            | Self::FeaturePatternInstanceSearchCapped
            | Self::BrepBlobUndecoded
            | Self::MeshContainerUnjoined
            | Self::MeshContainerMissing
            | Self::MeshAttributeNotTransferred
            | Self::XrefTableUndecoded
            | Self::XrefPlacementUndecoded
            | Self::XrefOccurrenceUnplaced
            | Self::XrefPlacementIdentityAssumed
            | Self::XrefPlacementRoleUnbound
            | Self::XrefCurrentPoseAmbiguous
            | Self::LocalOccurrenceCurrentPoseAmbiguous
            | Self::LocalOccurrencePathPose
            | Self::LocalOccurrencePathPoseUnresolved
            | Self::LocalOccurrencePathPoseDisagrees
            | Self::LocalOccurrencePathPoseInferred
            | Self::XrefContainedPoseDisagrees
            | Self::XrefOwnedPoseDisagrees
            | Self::XrefPlacementMetadataMissing
            | Self::XrefPlacementSuperseded
            | Self::XrefOwnerUnresolved
            | Self::JointPlacedOccurrenceUnsolved
            | Self::MeshVertexPrecisionReduced
            | Self::FaceSurfaceReferenceDangling
            | Self::SurfaceShapeNotDecoded
            | Self::ProceduralCurveUndecoded
            | Self::VertexToleranceWidened
            | Self::ProceduralSupportChartUnresolved
            | Self::SpringSupportPcurveUnresolved
            | Self::PcurveUndecoded
            | Self::BlendSupportPartial
            | Self::SolvedRecordUntyped
            | Self::MaterialNotTransferred
            | Self::TsplineRecordUntyped
            | Self::TsplineRadialMapUnresolved
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::KernelDialectUnverified
            | Self::KernelCarrierUnparseable
            | Self::DialectLayerCollision
            | Self::TextHeaderNoncanonical
            | Self::KernelHeaderToleranceUnresolved
            | Self::KernelHeaderMetadataUnresolved
            | Self::ManifestMetadataUnresolved
            | Self::ArchiveEntryUnreadable
            | Self::UnindexedRecordsRetained => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ScopeOutputOwnerMismatch | Self::ScopeSketchOwnerMismatch => {
                LossTaxonomy::FeatureHistoryRetained
            }
            Self::WriterAppearancePropertiesOmitted => LossTaxonomy::AppearanceReduced,
            Self::DimensionCompanionUntyped
            | Self::BodyReferenceTailUntyped
            | Self::PlacementConstructionUntyped
            | Self::ConstructionTrackingPathUntyped
            | Self::ParameterOwnerUnparsed
            | Self::ParameterRecordUnparsed
            | Self::HistoryRecordFramingFailed
            | Self::SolvedRecordUntyped
            | Self::KernelCarrierUnparseable
            | Self::TsplineRecordUntyped => LossTaxonomy::RecordNotTyped,
            Self::ScopeOwnerUnresolved
            | Self::OperationPoseApplied
            | Self::OperationPoseDisagrees
            | Self::OperationPoseNative
            | Self::ConfigurationMemberUnassigned
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::ConfigurationActiveRowUnbound
            | Self::DrawingDocumentOmitted
            | Self::XrefPlacementMetadataNative
            | Self::XrefPlacementCurrentPose
            | Self::XrefCurrentPoseNative
            | Self::XrefCurrentPoseAmbiguous
            | Self::LocalOccurrenceCurrentPose
            | Self::LocalOccurrenceCurrentPoseNative
            | Self::LocalOccurrenceCurrentPoseAmbiguous
            | Self::LocalOccurrencePathPose
            | Self::LocalOccurrencePathPoseUnresolved
            | Self::LocalOccurrencePathPoseDisagrees
            | Self::LocalOccurrencePathPoseInferred
            | Self::XrefContainedPose
            | Self::XrefContainedPoseDisagrees
            | Self::XrefContainedPoseNative
            | Self::XrefOwnedPose
            | Self::XrefOwnedPoseDisagrees
            | Self::XrefOwnedPoseNative
            | Self::XrefTableUndecoded => LossTaxonomy::MetadataNotTransferred,
            Self::ActComponentLinkUnresolved
            | Self::AssemblyComponentsExternal
            | Self::XrefCycle
            | Self::XrefMemberMissing
            | Self::XrefMemberUndecoded
            | Self::XrefPlacementUndecoded
            | Self::XrefOccurrenceUnplaced
            | Self::XrefPlacementIdentityAssumed
            | Self::XrefPlacementRoleUnbound
            | Self::XrefPlacementMetadataMissing
            | Self::XrefPlacementSuperseded
            | Self::JointPlacedOccurrenceUnsolved => LossTaxonomy::AssemblyComponentsExternal,
            Self::HistoryBindingBudgetExceeded
            | Self::HistoryBindingWorkExceeded
            | Self::RecipeSideSearchBudgetExceeded
            | Self::ParameterOrderBudgetExceeded
            | Self::FeatureStoredBodiesEmpty
            | Self::FeatureStoredBodiesRecipeAbsent
            | Self::FeatureOutputBodyUnbound
            | Self::FeatureOutputStateChainUnbound
            | Self::FeatureStoredBodiesUnbound
            | Self::FeatureDerivedBaseUnresolved
            | Self::FeatureBoxPrimitiveUnresolved
            | Self::FeatureDefinitionIncomplete
            | Self::ThreadLiveFaceUnproven
            | Self::SheetThicknessUnruled
            | Self::WorkPlaneFrameUnbound
            | Self::CylinderPlacementUnbound
            | Self::HoleExtentUnproven
            | Self::HolePlacementUnproven
            | Self::FeatureScopeUnprojected
            | Self::EmbossControlsNative
            | Self::EmbossPrefixUnparsed
            | Self::AlignOperandsUnresolved
            | Self::AlignEnvelopeUnparsed
            | Self::AlignConstructionUnparsed
            | Self::MoveUnprojected
            | Self::FeatureScopePayloadUninterpreted
            | Self::FeatureScopeEnvelopeSuperseded
            | Self::ParameterUnprojected
            | Self::ParameterOwnerUnrecognized
            | Self::ParameterUnitUntyped
            | Self::ParameterExpressionUnbound
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::HistoricalEdgeMidpointsUnresolved
            | Self::HistoricalBodyBoundsUnresolved
            | Self::SketchRelationNativeRetained
            | Self::SketchDimensionNativeRetained
            | Self::SketchPlacementUnprojected
            | Self::SketchPointUnprojected
            | Self::SketchCurveUnprojected
            | Self::SketchSurfaceUnprojected
            | Self::SketchTextUnprojected
            | Self::SketchTextStyleUnprojected
            | Self::SketchRelationUnprojected
            | Self::SketchPatternStateUnprojected
            | Self::SketchProfileSearchCapped
            | Self::SketchCurveUnprofiled
            | Self::DimensionUnprojected
            | Self::FeatureProfileSelectionNative
            | Self::FeatureExtrudeProfileGroupNative
            | Self::FeatureProfileArrangementCapped
            | Self::FeatureProfileDensificationCapped
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureBodySelectionNative
            | Self::FeatureCombineSelectionNative
            | Self::FeatureFixedFilletAssignmentNative
            | Self::FeatureExtrudeExtentNative
            | Self::FeatureFaceOperandUnresolved
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost
            | Self::FeatureOccurrencePathUnresolved
            | Self::FeaturePatternInstanceSearchCapped
            | Self::AssemblyAlignmentPartial
            | Self::AssemblyJointUnprojected
            | Self::AssemblyRigidGroupFrameUnparsed
            | Self::AssemblyJointMotionUndecoded
            | Self::AssemblyJointAngularLimitsNative
            | Self::AssemblyJointRestUnprojected
            | Self::AssemblyOperandPathPairsNative
            | Self::AssemblyOperandDescentUnresolved
            | Self::AssemblyOperandDescentAmbiguous
            | Self::AssemblyRootPathIdentitiesNative
            | Self::HistoryOnlyBodyOmitted => LossTaxonomy::FeatureHistoryRetained,
            Self::MaterialDistanceUnitUntyped => LossTaxonomy::MaterialNotTransferred,
            Self::FaceAppearanceAmbiguous => LossTaxonomy::AppearanceReduced,
            Self::XrefOwnerUnresolved
            | Self::ComponentBodyOwnerUnresolved
            | Self::ComponentBodyOwnerFallback
            | Self::LocalOccurrenceOwnerUnbound
            | Self::LocalOccurrenceOwnerCycle
            | Self::LocalOccurrenceExpansionLimit
            | Self::LocalOccurrenceUnplaced
            | Self::ComponentInsertUnplaced
            | Self::ComponentInsertOrderUnknown
            | Self::SavedFreeEdgeBodyUnowned
            | Self::LocalOccurrenceIdentityOrdinalUnresolved
            | Self::LocalOccurrenceEnvelopeSuperseded
            | Self::RootInstanceDefinitionUnbound
            | Self::ComponentDefinitionRevised
            | Self::ComponentNamingSpaceUnbound => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::SketchOwnerUnresolved
            | Self::DesignBodyBindingUnresolved
            | Self::FaceSurfaceReferenceDangling
            | Self::PcurveUndecoded => LossTaxonomy::ReferenceGraphNotClosed,
            Self::ReferenceImageNativeRetained
            | Self::DecalNativeRetained
            | Self::EdgeReferenceLostUnrepaired
            | Self::MeshAttributeNotTransferred
            | Self::TsplineRadialMapUnresolved => LossTaxonomy::AttributesNotTransferred,
            Self::BrepBlobUndecoded
            | Self::SurfaceShapeNotDecoded
            | Self::GeometryNotTransferred
            | Self::TsplineCageUndecoded => LossTaxonomy::GeometryNotTransferred,
            Self::MeshContainerUnjoined | Self::MeshContainerMissing => {
                LossTaxonomy::AssetNotTransferred
            }
            Self::MeshContainerUndecoded
            | Self::DialectLayerCollision
            | Self::DesignIntentUndecoded => LossTaxonomy::DecodeDiagnostic,
            Self::MeshVertexPrecisionReduced => LossTaxonomy::MeshVertexPrecision,
            Self::BodylessDesignCarrier
            | Self::NurbsSurfaceCarrier
            | Self::NurbsCurveCarrier
            | Self::MeshSurfaceSentinel => LossTaxonomy::CarrierSummary,
            Self::VertexToleranceWidened | Self::SketchRegionBodyOmitted => {
                LossTaxonomy::DecodeDiagnostic
            }
            Self::ProceduralCurveUndecoded
            | Self::ProceduralSupportChartUnresolved
            | Self::SpringSupportPcurveUnresolved
            | Self::BlendSupportPartial => LossTaxonomy::ProceduralReduced,
            Self::MaterialNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::TopologyNotTransferred => LossTaxonomy::TopologyNotTransferred,
            Self::MissingGeometryStream => LossTaxonomy::MissingGeometryStream,
            Self::SourcePreservedImageUnavailable => LossTaxonomy::PreservedSourceUnavailable,
            Self::SourceDialectUnverified | Self::KernelDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::TextHeaderNoncanonical => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::KernelHeaderToleranceUnresolved => LossTaxonomy::GeometryNotTransferred,
            Self::KernelHeaderMetadataUnresolved => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ManifestMetadataUnresolved => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ArchiveEntryUnreadable | Self::UnindexedRecordsRetained => {
                LossTaxonomy::RecordNotTyped
            }
        }
    }

    /// Strict floor pinned by this local code.
    ///
    /// Defaults to the taxonomy floor. `DesignIntentUndecoded` pins Warning
    /// above its tolerable `DecodeDiagnostic` taxonomy: strict mode refuses a
    /// document whose design intent was dropped.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::DesignIntentUndecoded => Some(Severity::Warning),
            other => other.shared_taxonomy().strict_floor(),
        }
    }

    /// Namespaced [`LossKind`] for this local code (taxonomy + pinned floor).
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        NamespacedLossKind::new(
            cadmpeg_ir::loss_namespace!("f3d"),
            self.code(),
            self.shared_taxonomy(),
        )
        .with_strict_floor(self.strict_floor())
        .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `f3d/<local>`. Severity comes from the local
    /// code; the strict floor comes from [`Self::strict_floor`].
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::F3dLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = F3dLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "assembly.operation-table-pose",
                "scope.owner-unresolved",
                "scope.output-owner-mismatch",
                "scope.sketch-owner-mismatch",
                "assembly.operation-table-pose-disagrees",
                "assembly.operation-table-pose-native",
                "dimension.companion-untyped",
                "body-reference.tail-untyped",
                "placement.construction-untyped",
                "work-plane.frame-unbound",
                "feature.cylinder-placement-unbound",
                "feature.tracking-path-untyped",
                "configuration.member-unassigned",
                "configuration.rule-unbound",
                "configuration.parameter-override-unbound",
                "configuration.feature-suppression-unbound",
                "configuration.active-row-unbound",
                "assembly.act-component-link-unresolved",
                "assembly.local-occurrence-unplaced",
                "assembly.local-occurrence-owner-unbound",
                "assembly.component-body-owner-unresolved",
                "assembly.component-body-owner-fallback",
                "assembly.local-occurrence-owner-cycle",
                "assembly.local-occurrence-expansion-limit",
                "feature.occurrence-path-unresolved",
                "sketch.component-owner-unresolved",
                "assembly.component-insert-unplaced",
                "assembly.component-insert-order-unknown",
                "assembly.component-definition-revised",
                "assembly.component-naming-space-unbound",
                "assembly.root-instance-definition-unbound",
                "assembly.local-occurrence-envelope-superseded",
                "assembly.local-occurrence-identity-ordinal-unresolved",
                "assembly.saved-free-edge-body-unowned",
                "assembly.joint-unprojected",
                "assembly.alignment-partial",
                "assembly.rigid-group-frame-unparsed",
                "assembly.joint-motion-undecoded",
                "assembly.joint-rest-unprojected",
                "assembly.joint-angular-limits-native",
                "assembly.operand-path-pairs-native",
                "assembly.root-path-identities-native",
                "assembly.operand-descent-ambiguous",
                "assembly.operand-descent-unresolved",
                "drawing.document-omitted",
                "history.binding-budget-exceeded",
                "history.binding-work-exceeded",
                "feature.recipe-search-budget-exceeded",
                "parameter.order-budget-exceeded",
                "history.record-framing-failed",
                "design.body-binding-unresolved",
                "design.history-only-body-omitted",
                "appearance.reference-image-native",
                "appearance.decal-native",
                "feature.edge-reference-lost",
                "feature.definition-incomplete",
                "feature.thread-live-face-unproven",
                "feature.stored-bodies-empty",
                "feature.stored-bodies-recipe-absent",
                "feature.stored-bodies-unbound",
                "feature.output-state-chain-unbound",
                "feature.output-body-unbound",
                "feature.derived-base-unresolved",
                "feature.box-primitive-unresolved",
                "feature.sheet-thickness-unruled",
                "feature.scope-unprojected",
                "feature.move-unprojected",
                "feature.hole-extent-unproven",
                "feature.hole-placement-unproven",
                "feature.align-operands-unresolved",
                "feature.align-envelope-unparsed",
                "feature.align-construction-unparsed",
                "feature.scope-payload-uninterpreted",
                "feature.emboss-controls-native",
                "feature.emboss-prefix-unparsed",
                "feature.scope-envelope-superseded",
                "parameter.unprojected",
                "parameter.record-unparsed",
                "parameter.owner-unrecognized",
                "parameter.owner-unparsed",
                "parameter.unit-untyped",
                "material.distance-unit-untyped",
                "material.face-appearance-ambiguous",
                "parameter.expression-unbound",
                "history.dependency-unprojected",
                "history.dependency-ambiguous",
                "history.face-selection-unbound",
                "history.edge-midpoints-unresolved",
                "history.body-bounds-unresolved",
                "sketch.relation-native",
                "sketch.dimension-native",
                "sketch.placement-unprojected",
                "sketch.point-unprojected",
                "sketch.curve-unprojected",
                "sketch.surface-unprojected",
                "sketch.text-unprojected",
                "sketch.text-style-unprojected",
                "sketch.relation-unprojected",
                "sketch.pattern-state-unprojected",
                "sketch.profile-search-capped",
                "sketch.curve-unprofiled",
                "dimension.unprojected",
                "feature.profile-selection-native",
                "feature.extrude-profile-group-native",
                "feature.profile-arrangement-capped",
                "feature.profile-densification-capped",
                "feature.path-selection-native",
                "feature.face-selection-native",
                "feature.face-selection-active-substituted",
                "feature.body-selection-native",
                "feature.combine-selection-native",
                "feature.fixed-fillet-assignment-native",
                "feature.extrude-extent-native",
                "feature.face-operand-unresolved",
                "feature.edge-selection-native",
                "feature.edge-operand-unresolved",
                "feature.edge-selection-lost",
                "feature.pattern-instance-search-capped",
                "geometry.brep-blob-undecoded",
                "mesh.container-unjoined",
                "mesh.container-undecoded",
                "mesh.container-missing",
                "mesh.attribute-not-transferred",
                "xref.table-undecoded",
                "xref.placement-undecoded",
                "xref.occurrence-unplaced",
                "xref.placement-identity-assumed",
                "xref.placement-metadata-native",
                "xref.placement-role-unbound",
                "xref.placement-current-pose",
                "xref.current-pose-native",
                "xref.current-pose-ambiguous",
                "assembly.local-occurrence-current-pose",
                "assembly.local-occurrence-current-pose-native",
                "assembly.local-occurrence-current-pose-ambiguous",
                "assembly.local-occurrence-path-pose",
                "assembly.local-occurrence-path-pose-unresolved",
                "assembly.local-occurrence-path-pose-disagrees",
                "assembly.local-occurrence-path-pose-inferred",
                "xref.contained-pose",
                "xref.contained-pose-disagrees",
                "xref.contained-pose-native",
                "xref.owned-pose",
                "xref.owned-pose-disagrees",
                "xref.owned-pose-native",
                "xref.placement-metadata-missing",
                "xref.placement-superseded",
                "xref.owner-unresolved",
                "assembly.joint-placed-occurrence-unsolved",
                "mesh.vertex-precision-reduced",
                "design.bodyless-carrier",
                "assembly.components-external",
                "geometry.nurbs-surface-carrier",
                "geometry.sketch-region-body-omitted",
                "geometry.nurbs-curve-carrier",
                "geometry.face-surface-dangling",
                "geometry.surface-shape-not-decoded",
                "geometry.mesh-surface-sentinel",
                "geometry.procedural-curve-undecoded",
                "geometry.vertex-tolerance-widened",
                "geometry.spring-support-pcurve-unresolved",
                "geometry.procedural-support-chart-unresolved",
                "geometry.pcurve-undecoded",
                "geometry.blend-support-partial",
                "geometry.solved-record-untyped",
                "material.not-transferred",
                "geometry.not-transferred",
                "topology.not-transferred",
                "container.missing-geometry-stream",
                "xref.cycle",
                "xref.member-missing",
                "xref.member-undecoded",
                "tspline.record-untyped",
                "tspline.radial-map-unresolved",
                "tspline.cage-undecoded",
                "source.preserved-image-unavailable",
                "source.dialect-unverified",
                "writer.appearance-properties-omitted",
                "target.source-dialect-displaced",
                "source.kernel-dialect-unverified",
                "source.kernel-carrier-unparseable",
                "source.dialect-layer-collision",
                "brep.text-header-noncanonical",
                "brep.header-tolerance-unresolved",
                "brep.header-metadata-unresolved",
                "source.manifest-metadata-unresolved",
                "source.archive-entry-unreadable",
                "source.unindexed-records-retained",
                "design.intent-undecoded",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in F3dLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in F3dLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
