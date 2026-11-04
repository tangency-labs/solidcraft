//! Phase 2 of the Design graph: sketches, ASM history binding, features and
//! constraints.
//!
//! Phase 2 reads the product prerequisites and the decoded geometry. It does
//! not write the session; the caller merges [`DesignOutputs`].

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    assets::Asset,
    document::Model,
    features::{DesignConfiguration, DesignParameter, Feature, FeatureInputTopology},
    geometry::Surface,
    products::{AssemblyJoint, Occurrence},
    report::loss::LossNote,
    sketches::{
        Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchConstraint,
        SpatialSketchEntity,
    },
    subd::SubdSurface,
    topology::{Body, Face, Region, Shell},
    units::Tolerances,
};

use super::prereqs::{
    append_related_record_headers, collect_related_indices, history_ref_width, ProductPrereqs,
};
use super::{push_loss_vec, F3dLossCode, MeshProjection};
use crate::container::{self, ContainerScan};
use crate::history::binding::MAX_HISTORY_BINDING_WORK;
use crate::history_records::AsmHistory;
use crate::native::F3dNative;
use crate::records::{
    bodies::DesignBodyBounds,
    canvas::DesignCanvasImage,
    configuration::DesignConfiguration as NativeDesignConfiguration,
    decal::{DesignDecalImage, DesignRecordHeader},
    dimension_locus_arenas::{DesignDimensionLocusPairs, DesignDimensionNullLocusPairs},
    dimensions::{
        DesignDimensionAnnotationFrame, DesignDimensionLocusGroup,
        DesignDimensionPresentationFrame, DesignDimensionRecipeRecord,
    },
    entity_header::DesignFeatureTimeline,
    feature::{
        scope::DesignParameterScope, surface_ops::DesignSurfaceTrimOperation,
        work_geometry::DesignEdgeTreatmentVertexOperand,
    },
    parameters::DesignParameterCompanion,
    references::{LostEdgeReference, PersistentReference},
    sketch_geometry::{SketchCurveIdentity, SketchPoint, SketchSurface, SketchText},
    sketch_links::{PersistentDesignLink, PersistentSubentityTag},
    sketch_placement::DesignSketchPlacement,
    sketch_relations::SketchRelation,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandIdentity, edge_identity::DesignEdgeIdentityOperand,
        edge_identity::DesignEdgeOperand, entity_selection::DesignEntitySelectionOperand,
        entity_selection::DesignLoftLegacyBodyCarrier,
        extrude_selection::DesignExtrudeSelectionGroup,
        extrude_selection::DesignExtrudeSelectionMember, face::DesignFaceOperand,
        face::DesignFaceSourceGroup, fillet::DesignFilletRadiusGroup,
        historical_context::AsmFaceBoundary,
    },
};

/// Everything phase 2 reads.
pub(super) struct DesignInputs<'a> {
    pub(super) ctx: &'a DecodeContext<'a>,
    pub(super) scan: &'a ContainerScan<'a>,
    pub(super) prereqs: &'a ProductPrereqs,
    /// The decoded phase-1 timelines. Phase 2 does not run without them.
    pub(super) timelines: &'a [DesignFeatureTimeline],
    pub(super) products: DesignProducts<'a>,
    pub(super) geometry: DesignGeometry<'a>,
}

/// Product structure that Design features name.
pub(super) struct DesignProducts<'a> {
    pub(super) occurrences: &'a [Occurrence],
    pub(super) assembly_joints: &'a [AssemblyJoint],
}

/// Decoded geometry that Design features bind to.
pub(super) struct DesignGeometry<'a> {
    pub(super) primary_brep: Option<&'a str>,
    pub(super) bodies: &'a [Body],
    pub(super) regions: &'a [Region],
    pub(super) shells: &'a [Shell],
    pub(super) faces: &'a [Face],
    pub(super) surfaces: &'a [Surface],
    pub(super) subds: &'a [SubdSurface],
    pub(super) assets: &'a [Asset],
    pub(super) tolerances: &'a Tolerances,
    pub(super) persistent_design_links: &'a [PersistentDesignLink],
    pub(super) persistent_subentity_tags: &'a [PersistentSubentityTag],
    /// Mesh bodies, projected before phase 2 on both paths.
    pub(super) mesh_projection: &'a MeshProjection,
    pub(super) path: DesignPath,
}

/// Session path as phase 2 sees it. Phase 2 discards the history projection
/// caches on the geometry path only.
#[derive(Clone, Copy)]
pub(super) enum DesignPath {
    /// A B-rep was transferred.
    Geometry,
    /// No B-rep.
    Bodyless,
}

/// A contained phase-2 failure. A document that carries one keeps its product
/// and geometry records and holds none of the records phase 2 produces.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesignIntentFailure {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// The error that stopped design-intent decoding.
    pub(crate) reason: String,
}

/// Native Design records that phase 2 produces.
#[derive(Debug, Default)]
pub(super) struct DesignNative {
    asm_histories: Vec<AsmHistory>,
    asm_face_boundaries: Vec<AsmFaceBoundary>,
    persistent_references: Vec<PersistentReference>,
    lost_edge_references: Vec<LostEdgeReference>,
    sketch_relations: Vec<SketchRelation>,
    sketch_points: Vec<SketchPoint>,
    sketch_texts: Vec<SketchText>,
    sketch_curve_identities: Vec<SketchCurveIdentity>,
    sketch_surfaces: Vec<SketchSurface>,
    pub(super) design_parameter_companions: Vec<DesignParameterCompanion>,
    design_face_source_groups: Vec<DesignFaceSourceGroup>,
    design_canvas_images: Vec<DesignCanvasImage>,
    design_decal_images: Vec<DesignDecalImage>,
    design_construction_operand_groups: Vec<DesignConstructionOperandGroup>,
    design_loft_legacy_body_carriers: Vec<DesignLoftLegacyBodyCarrier>,
    design_extrude_selection_groups: Vec<DesignExtrudeSelectionGroup>,
    design_construction_operand_identities: Vec<DesignConstructionOperandIdentity>,
    design_edge_identity_operands: Vec<DesignEdgeIdentityOperand>,
    design_fillet_radius_groups: Vec<DesignFilletRadiusGroup>,
    design_extrude_selection_members: Vec<DesignExtrudeSelectionMember>,
    design_entity_selection_operands: Vec<DesignEntitySelectionOperand>,
    design_profile_curve_selections:
        Vec<crate::records::topology::entity_selection::DesignProfileCurveSelection>,
    design_body_recipe_operands: Vec<DesignBodyRecipeOperand>,
    design_body_reference_tails: Vec<crate::records::body_reference_tail::DesignBodyReferenceTail>,
    design_edge_operands: Vec<DesignEdgeOperand>,
    design_edge_treatment_vertex_operands: Vec<DesignEdgeTreatmentVertexOperand>,
    design_face_operands: Vec<DesignFaceOperand>,
    design_sketch_placements: Vec<DesignSketchPlacement>,
    design_placement_constructions:
        Vec<crate::records::placement_construction::DesignPlacementConstruction>,
    pub(super) design_dimension_recipe_records: Vec<DesignDimensionRecipeRecord>,
    pub(super) design_dimension_locus_pairs: DesignDimensionLocusPairs,
    pub(super) design_dimension_annotation_frames: Vec<DesignDimensionAnnotationFrame>,
    design_dimension_presentation_frames: Vec<DesignDimensionPresentationFrame>,
    pub(super) design_dimension_locus_groups: Vec<DesignDimensionLocusGroup>,
    pub(super) design_dimension_null_locus_pairs: DesignDimensionNullLocusPairs,
    design_body_bounds: Vec<DesignBodyBounds>,
    pub(super) design_configurations: Vec<NativeDesignConfiguration>,
    /// Count of root records that no table document receives.
    pub(super) configuration_roots_unbound: usize,
    /// Phase-1 scopes with their design-intent bindings. Supersedes the
    /// product-prerequisite scopes in native storage.
    design_parameter_scopes: Vec<DesignParameterScope>,
    /// While phase 2 runs, every header it reads, starting from the phase-1
    /// headers. Phase 2 returns only the headers it added.
    design_record_headers: Vec<DesignRecordHeader>,
    /// Surface-trim operations of the admitted scopes.
    design_surface_trim_operations: Vec<DesignSurfaceTrimOperation>,
    design_sheet_metal_settings:
        Vec<crate::records::sheet_metal_settings::DesignSheetMetalSettings>,
}

impl DesignNative {
    /// Move every record into native storage, after the product prerequisites.
    /// Record headers join the phase-1 headers.
    pub(super) fn store(self, native: &mut F3dNative) {
        let Self {
            asm_histories,
            asm_face_boundaries,
            persistent_references,
            lost_edge_references,
            sketch_relations,
            sketch_points,
            sketch_texts,
            sketch_curve_identities,
            sketch_surfaces,
            design_parameter_companions,
            design_face_source_groups,
            design_canvas_images,
            design_decal_images,
            design_construction_operand_groups,
            design_loft_legacy_body_carriers,
            design_extrude_selection_groups,
            design_construction_operand_identities,
            design_edge_identity_operands,
            design_fillet_radius_groups,
            design_extrude_selection_members,
            design_entity_selection_operands,
            design_profile_curve_selections,
            design_body_recipe_operands,
            design_body_reference_tails,
            design_edge_operands,
            design_edge_treatment_vertex_operands,
            design_face_operands,
            design_sketch_placements,
            design_placement_constructions,
            design_dimension_recipe_records,
            design_dimension_locus_pairs,
            design_dimension_annotation_frames,
            design_dimension_presentation_frames,
            design_dimension_locus_groups,
            design_dimension_null_locus_pairs,
            design_body_bounds,
            design_configurations,
            configuration_roots_unbound: _,
            design_parameter_scopes,
            design_record_headers,
            design_surface_trim_operations,
            design_sheet_metal_settings,
        } = self;
        native.asm_histories = asm_histories;
        native.asm_face_boundaries = asm_face_boundaries;
        native.persistent_references = persistent_references;
        native.lost_edge_references = lost_edge_references;
        native.sketch_relations = sketch_relations;
        native.sketch_points = sketch_points;
        native.sketch_texts = sketch_texts;
        native.sketch_curve_identities = sketch_curve_identities;
        native.sketch_surfaces = sketch_surfaces;
        native.design_parameter_companions = design_parameter_companions;
        native.design_face_source_groups = design_face_source_groups;
        native.design_canvas_images = design_canvas_images;
        native.design_decal_images = design_decal_images;
        native.design_construction_operand_groups = design_construction_operand_groups;
        native.design_loft_legacy_body_carriers = design_loft_legacy_body_carriers;
        native.design_extrude_selection_groups = design_extrude_selection_groups;
        native.design_construction_operand_identities = design_construction_operand_identities;
        native.design_edge_identity_operands = design_edge_identity_operands;
        native.design_fillet_radius_groups = design_fillet_radius_groups;
        native.design_extrude_selection_members = design_extrude_selection_members;
        native.design_entity_selection_operands = design_entity_selection_operands;
        native.design_profile_curve_selections = design_profile_curve_selections;
        native.design_body_recipe_operands = design_body_recipe_operands;
        native.design_body_reference_tails = design_body_reference_tails;
        native.design_edge_operands = design_edge_operands;
        native.design_edge_treatment_vertex_operands = design_edge_treatment_vertex_operands;
        native.design_face_operands = design_face_operands;
        native.design_sketch_placements = design_sketch_placements;
        native.design_placement_constructions = design_placement_constructions;
        native.design_dimension_recipe_records = design_dimension_recipe_records;
        native.design_dimension_locus_pairs = design_dimension_locus_pairs;
        native.design_dimension_annotation_frames = design_dimension_annotation_frames;
        native.design_dimension_presentation_frames = design_dimension_presentation_frames;
        native.design_dimension_locus_groups = design_dimension_locus_groups;
        native.design_dimension_null_locus_pairs = design_dimension_null_locus_pairs;
        native.design_body_bounds = design_body_bounds;
        native.design_configurations = design_configurations;
        native.design_parameter_scopes = design_parameter_scopes;
        native.design_record_headers.extend(design_record_headers);
        native.design_record_headers.sort_by(|a, b| a.id.cmp(&b.id));
        native.design_surface_trim_operations = design_surface_trim_operations;
        native.design_sheet_metal_settings = design_sheet_metal_settings;
    }
}

/// Neutral Design arenas that phase 2 produces. Geometry arenas are absent:
/// phase 2 reads them and does not change them.
#[derive(Debug, Default)]
pub(super) struct DesignModel {
    pub(super) configurations: Vec<DesignConfiguration>,
    pub(super) features: Vec<Feature>,
    pub(super) parameters: Vec<DesignParameter>,
    /// Canvas and decal assets that the geometry assets do not hold yet.
    pub(super) assets: Vec<Asset>,
    pub(super) feature_input_topologies: Vec<FeatureInputTopology>,
    pub(super) sketches: Vec<Sketch>,
    pub(super) sketch_entities: Vec<SketchEntity>,
    pub(super) sketch_constraints: Vec<SketchConstraint>,
    pub(super) spatial_sketches: Vec<SpatialSketch>,
    pub(super) spatial_sketch_entities: Vec<SpatialSketchEntity>,
    pub(super) spatial_sketch_constraints: Vec<SpatialSketchConstraint>,
}

impl DesignModel {
    /// Move every arena into the neutral model.
    pub(super) fn store(self, model: &mut Model) {
        let Self {
            configurations,
            features,
            parameters,
            assets,
            feature_input_topologies,
            sketches,
            sketch_entities,
            sketch_constraints,
            spatial_sketches,
            spatial_sketch_entities,
            spatial_sketch_constraints,
        } = self;
        model.configurations = configurations;
        model.features = features;
        model.parameters = parameters;
        model.assets.extend(assets);
        model.feature_input_topologies = feature_input_topologies;
        model.sketches = sketches;
        model.sketch_entities = sketch_entities;
        model.sketch_constraints = sketch_constraints;
        model.spatial_sketches = spatial_sketches;
        model.spatial_sketch_entities = spatial_sketch_entities;
        model.spatial_sketch_constraints = spatial_sketch_constraints;
    }
}

/// Everything phase 2 produces.
#[derive(Debug)]
pub(super) struct DesignOutputs {
    pub(super) native: DesignNative,
    pub(super) model: DesignModel,
    /// Losses in decode order. They follow the geometry losses.
    pub(super) losses: Vec<LossNote>,
}

/// Add design assets that neither the geometry nor earlier design assets hold.
fn extend_design_assets(
    geometry: &[Asset],
    assets: &mut Vec<Asset>,
    incoming: Vec<Asset>,
) -> Result<(), CodecError> {
    for asset in incoming {
        match geometry
            .iter()
            .chain(assets.iter())
            .find(|existing| existing.id == asset.id)
        {
            Some(existing) if existing != &asset => {
                return Err(CodecError::malformed(format_args!(
                    "F3D embedded asset {} has conflicting projections",
                    asset.id.as_str()
                )));
            }
            Some(_) => {}
            None => assets.push(asset),
        }
    }
    Ok(())
}

fn decode_asm_history(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    history_brep: &container::BrepFacts,
    remaining_retained_bytes: &mut u64,
    binding_work: &WorkBudget,
) -> Result<Option<AsmHistory>, CodecError> {
    let bytes = scan.entry_bytes(&history_brep.name)?;
    crate::history::decode(
        ctx,
        bytes,
        &history_brep.name,
        history_ref_width(history_brep),
        remaining_retained_bytes,
        binding_work,
    )
}

#[cfg(test)]
thread_local! {
    static INJECTED_FAILURE: std::cell::Cell<Option<fn() -> CodecError>> =
        const { std::cell::Cell::new(None) };
}

/// Make the next phase-2 run on this thread fail with `failure()`.
#[cfg(test)]
pub(crate) fn inject_failure(failure: fn() -> CodecError) {
    INJECTED_FAILURE.with(|slot| slot.set(Some(failure)));
}

/// Decode design intent over the product prerequisites.
pub(super) fn decode(inputs: &DesignInputs<'_>) -> Result<DesignOutputs, CodecError> {
    #[cfg(test)]
    if let Some(failure) = INJECTED_FAILURE.with(std::cell::Cell::take) {
        return Err(failure());
    }
    let &DesignInputs {
        ctx,
        scan,
        prereqs,
        timelines,
        ref products,
        ref geometry,
    } = inputs;
    let mut design = DesignNative {
        design_parameter_scopes: prereqs.design_parameter_scopes.clone(),
        design_record_headers: prereqs.record_headers(),
        ..DesignNative::default()
    };
    let mut model = DesignModel::default();
    let mut losses = Vec::new();
    let mut remaining_retained_bytes = prereqs.history_retained_allowance;
    let mut remaining_binding_work = MAX_HISTORY_BINDING_WORK;
    let mut unbound_histories = 0_usize;
    for history_brep in container::history_breps(scan) {
        let binding_work = WorkBudget::new(remaining_binding_work);
        let history = decode_asm_history(
            ctx,
            scan,
            history_brep,
            &mut remaining_retained_bytes,
            &binding_work,
        )?;
        unbound_histories += usize::from(binding_work.exhausted());
        remaining_binding_work = binding_work.remaining();
        if let Some(history) = history {
            design.asm_histories.push(history);
        }
    }
    if unbound_histories != 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::HistoryBindingWorkExceeded,
            format_args!(
                "{unbound_histories} ASM history stream(s) keep their bulletin boards and history records without historical topology because binding exceeded the local work cap MAX_HISTORY_BINDING_WORK ({MAX_HISTORY_BINDING_WORK} decoded history records)."
            ),
        );
    }
    design.persistent_references =
        crate::design::decode::sketch::decode_persistent_references(scan)?;
    design.lost_edge_references = crate::design::decode::sketch::decode_lost_edge_references(scan)?;
    design.sketch_relations = crate::design::decode::sketch::decode_sketch_relations(
        scan,
        &prereqs.design_record_headers,
    )?;
    let (untyped_tracking_paths, profile_losses) =
        extend_related_design_records(ctx, scan, prereqs, timelines, geometry, &mut design)?;
    losses.extend(profile_losses);
    if untyped_tracking_paths != 0 {
        push_loss_vec(&mut losses, F3dLossCode::ConstructionTrackingPathUntyped,
            format_args!("{untyped_tracking_paths} construction tracking path reference(s) remain source-only because no matching indexed registration was admitted."));
    }
    for history in &mut design.asm_histories {
        history.release_topologies();
    }
    design.sketch_points = crate::design::decode::sketch::decode_sketch_points(scan)?;
    design.sketch_texts = crate::design::decode::sketch::decode_sketch_texts(scan)?;
    design.sketch_curve_identities =
        crate::design::decode::sketch::decode_sketch_curve_identities(scan)?;
    design.sketch_surfaces = crate::design::decode::sketch::decode_sketch_surfaces(scan)?;
    crate::design::decode::sketch::bind_sketch_graph(
        &prereqs.design_entity_headers,
        &mut design.sketch_points,
        &mut design.sketch_curve_identities,
        &mut design.sketch_surfaces,
        &mut design.sketch_relations,
    )?;
    let placements = crate::design::decode::placement_construction::decode(ctx, scan)?;
    design.design_placement_constructions = placements.records;
    if placements.unparsed != 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::PlacementConstructionUntyped,
            format_args!("{} placement construction record(s) remain source-only because their registration or bounded member layout is unsupported or ambiguous.", placements.unparsed),
        );
    }

    losses.extend(
        crate::design::decode::placement_construction::work_planes::bind(
            &mut design.design_parameter_scopes,
            &design.design_placement_constructions,
        ),
    );
    losses.extend(
        crate::design::decode::placement_construction::cylinders::bind(
            &mut design.design_parameter_scopes,
            &design.design_placement_constructions,
        ),
    );
    crate::design::decode::operands::bind_work_point_input_carriers(
        scan,
        &mut design.design_parameter_scopes,
        &design.design_record_headers,
        &prereqs.construction_recipes,
        &design.design_edge_operands,
        &design.sketch_points,
    )?;
    crate::design::decode::operands::bind_hole_location_sketch_points(
        scan,
        &mut design.design_parameter_scopes,
        &design.design_record_headers,
        &design.sketch_points,
    )?;
    crate::design::decode::operands::bind_extrude_selection_geometry(
        &mut design.design_extrude_selection_members,
        &design.design_extrude_selection_groups,
        &design.design_parameter_scopes,
        &design.sketch_points,
        &design.sketch_curve_identities,
    );
    let dimension_inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
        scan,
        placements: &design.design_sketch_placements,
        parameters: &prereqs.design_parameters,
        owners: &prereqs.design_parameter_owners,
        companions: &design.design_parameter_companions,
        scopes: &design.design_parameter_scopes,
        headers: &design.design_record_headers,
        points: &design.sketch_points,
        curves: &design.sketch_curve_identities,
    };
    design.design_dimension_locus_pairs =
        crate::design::decode::dimension_frames::decode_dimension_locus_pairs(&dimension_inputs)?
            .try_into()
            .map_err(|error: String| CodecError::malformed(format_args!("{error}")))?;
    design.design_dimension_annotation_frames =
        crate::design::decode::dimension_frames::decode_dimension_annotation_frames(
            &dimension_inputs,
            &prereqs.design_entity_headers,
        )?;
    design.design_dimension_presentation_frames =
        crate::design::decode::dimension_frames::decode_dimension_presentation_frames(
            &dimension_inputs,
            &prereqs.design_entity_headers,
        )?;
    design.design_dimension_locus_groups =
        crate::design::decode::dimension_frames::decode_dimension_locus_groups(
            &dimension_inputs,
            &prereqs.design_entity_headers,
        )?;
    design.design_dimension_null_locus_pairs =
        crate::design::decode::dimension_frames::decode_dimension_null_locus_pairs(
            &dimension_inputs,
            &design.design_dimension_locus_pairs,
            &design.design_dimension_locus_groups,
        )?
        .try_into()
        .map_err(|error: String| CodecError::malformed(format_args!("{error}")))?;
    crate::design::dimensions::remove_dimension_frame_relations(
        &mut design.sketch_relations,
        &design.design_dimension_locus_pairs,
        &design.design_dimension_locus_groups,
        &design.design_dimension_null_locus_pairs,
    );
    crate::design::dimensions::bind_dimension_loci(
        crate::design::dimensions::DimensionLocusInputs {
            placements: &design.design_sketch_placements,
            owners: &prereqs.design_parameter_owners,
            pairs: &design.design_dimension_locus_pairs,
            groups: &design.design_dimension_locus_groups,
            annotation_frames: &design.design_dimension_annotation_frames,
            null_pairs: &design.design_dimension_null_locus_pairs,
        },
        &mut design.sketch_points,
        &mut design.sketch_curve_identities,
    )?;
    design.design_body_bounds =
        crate::design::decode::body::decode_body_bounds(scan, &prereqs.design_entity_headers)?;
    crate::design::decode::body::bind_body_bounds(
        &mut design.design_body_bounds,
        &prereqs.design_body_bindings,
    )?;
    let configurations = crate::design::configurations::decode_configurations(ctx, scan)?;
    design.design_configurations = configurations.documents;
    design.configuration_roots_unbound = configurations.unbound_roots;
    design.design_sheet_metal_settings =
        crate::design::decode::sheet_metal_settings::decode_settings(scan)?;
    let projection_parameters = crate::design::decode::parameters::parameters_for_projection(
        &prereqs.design_parameters,
        &prereqs.design_parameter_owner_unparsed,
    );
    let projected = crate::design::feature_project::project_parameter_design_with_edge_identities(
        ctx,
        &crate::design::feature_project::ProjectInputs {
            native: &projection_parameters,
            owners: &prereqs.design_parameter_owners,
            scopes: &design.design_parameter_scopes,
            timelines,
            construction_groups: &design.design_construction_operand_groups,
            fillet_radius_groups: &design.design_fillet_radius_groups,
            edge_operands: &design.design_edge_operands,
            edge_identity_operands: &design.design_edge_identity_operands,
            edge_treatment_vertex_operands: &design.design_edge_treatment_vertex_operands,
            entity_selection_operands: &design.design_entity_selection_operands,
            curve_identities: &design.sketch_curve_identities,
            face_operands: &design.design_face_operands,
            body_recipe_operands: &design.design_body_recipe_operands,
            legacy_loft_body_carriers: &design.design_loft_legacy_body_carriers,
            placements: &design.design_sketch_placements,
            body_bindings: &prereqs.design_body_bindings,
            component_naming_spaces: &prereqs.design_component_naming_spaces,
            histories: &design.asm_histories,
            face_boundaries: &design.asm_face_boundaries,
            sheet_metal_settings: &design.design_sheet_metal_settings,
        },
    )?;
    losses.extend(projected.unruled_flange_thickness_loss());
    losses.extend(projected.hole_losses);
    losses.extend(
        crate::design::decode::operands::fixed_fillet_assignment_losses(
            &design.design_parameter_scopes,
            &design.design_construction_operand_groups,
        ),
    );
    model.features = projected.features;
    model.parameters = projected.parameters;
    if projected.unordered_parameter_scopes != 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::ParameterOrderBudgetExceeded,
            format_args!(
                "{} Design parameter owner scope(s) keep their unique ordinal slots and drop their same-scope dependency edges because ordering exceeded the local work cap PARAMETER_ORDER_WORK ({}).",
                projected.unordered_parameter_scopes,
                crate::design::feature_project::PARAMETER_ORDER_WORK,
            ),
        );
    }
    let tails = crate::design::decode::body_reference_tails::decode(ctx, scan)?;
    design.design_body_reference_tails = tails.records;
    if tails.unparsed != 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::BodyReferenceTailUntyped,
            format_args!("{} whole-body reference tail(s) remain source-only because no complete standard reference-run frame was admitted.", tails.unparsed),
        );
    }

    let indices = crate::design::decode::sheet_metal_settings::linked_record_indices(
        &design.design_sheet_metal_settings,
    );
    append_related_record_headers(scan, &mut design.design_record_headers, &indices)?;
    design.design_surface_trim_operations =
        crate::design::decode::surface_trim::decode_surface_trim_operations(
            scan,
            &design.design_parameter_scopes,
        )?;
    crate::design::feature_project::bind_surface_trim_cell_selections(
        &mut model.features,
        &design.design_parameter_scopes,
        &design.design_surface_trim_operations,
    );
    super::bind_mesh_feature_definitions(
        &mut model.features,
        &design.design_parameter_scopes,
        geometry.mesh_projection,
    )?;
    crate::design::feature_project::form_cages::bind_form_cages(
        ctx,
        scan,
        &design.design_parameter_scopes,
        &mut model.features,
        geometry.subds,
    )?;
    let canvas_assets = crate::design::decode::canvas::project_canvas_images(
        ctx,
        scan,
        &design.design_parameter_scopes,
        &design.design_canvas_images,
        &mut model.features,
    )?;
    extend_design_assets(geometry.assets, &mut model.assets, canvas_assets)?;
    let decal_assets = crate::design::decode::decal::project_decal_images(
        ctx,
        scan,
        &design.design_parameter_scopes,
        &design.design_decal_images,
        &design.design_construction_operand_groups,
        &design.design_body_recipe_operands,
        &mut model.features,
    )?;
    extend_design_assets(geometry.assets, &mut model.assets, decal_assets)?;
    let mut scope_histories = crate::history::bind_scope_histories(
        &design.design_parameter_scopes,
        &prereqs.design_body_bindings,
        &design.design_body_recipe_operands,
        &design.asm_histories,
    );
    let owner_sources = crate::design::decode::operands::compact_owner::source_selectors(
        scan,
        &design.design_construction_operand_groups,
        &design.design_construction_operand_identities,
        &prereqs.design_body_bindings,
    )?;
    let _lineage_scopes = crate::history::compact_lineage::bind(
        crate::history::compact_lineage::Inputs {
            scopes: &design.design_parameter_scopes,
            groups: &design.design_construction_operand_groups,
            identities: &design.design_construction_operand_identities,
            operands: &design.design_edge_identity_operands,
            recipes: &design.design_body_recipe_operands,
            owners: &owner_sources,
            histories: &design.asm_histories,
        },
        &mut scope_histories,
    );
    crate::history::fillet_blends::bind_fillet_blend_edges(
        &mut model.features,
        crate::history::fillet_blends::FilletBlendInputs {
            scopes: &design.design_parameter_scopes,
            groups: &design.design_construction_operand_groups,
            histories: &mut design.asm_histories,
        },
    );
    model.feature_input_topologies = crate::history::project_feature_input_topologies(
        &model.features,
        &design.design_parameter_scopes,
        &mut design.asm_histories,
        &design.design_edge_operands,
        &scope_histories,
    );
    losses.extend(crate::history::bind_feature_outputs(
        &mut model.features,
        &design.design_parameter_scopes,
        &mut design.asm_histories,
        geometry.bodies,
        &scope_histories,
    )?);
    crate::history::bind_sweep_result_modes(&mut model.features, geometry.bodies);
    losses.extend(crate::history::bind_feature_body_selections(
        &mut model.features,
        &crate::history::FeatureBodySelectionInputs {
            scopes: &design.design_parameter_scopes,
            groups: &design.design_construction_operand_groups,
            body_recipe_operands: &design.design_body_recipe_operands,
            construction_recipes: &prereqs.construction_recipes,
            persistent_design_links: geometry.persistent_design_links,
            histories: &design.asm_histories,
            bodies: geometry.bodies,
            regions: geometry.regions,
            shells: geometry.shells,
        },
    ));
    crate::history::bind_feature_face_selections(
        &mut model.features,
        &mut model.feature_input_topologies,
        crate::history::FeatureFaceSelectionInputs {
            scopes: &design.design_parameter_scopes,
            groups: &design.design_construction_operand_groups,
            operands: &design.design_face_operands,
            entity_operands: &design.design_entity_selection_operands,
            body_recipe_operands: &design.design_body_recipe_operands,
            histories: &mut design.asm_histories,
        },
    );
    losses.extend(crate::history::thread_face::bind(
        &mut model.features,
        &crate::history::thread_face::Inputs {
            scopes: &design.design_parameter_scopes,
            topologies: &model.feature_input_topologies,
            histories: &design.asm_histories,
            faces: geometry.faces,
            primary_brep: geometry.primary_brep,
        },
    ));
    crate::history::bind_feature_path_selections(
        &mut model.features,
        &design.design_parameter_scopes,
        &design.design_construction_operand_groups,
        &design.design_entity_selection_operands,
    );
    crate::design::feature_project::bind_revolve_face_axes(
        &mut model.features,
        &design.design_parameter_scopes,
        &design.design_construction_operand_groups,
        &design.design_entity_selection_operands,
        &design.design_face_operands,
        geometry.faces,
        geometry.surfaces,
    );
    let planar = crate::design::sketch_project::project_sketch_design(
        ctx,
        &crate::design::sketch_project::SketchProjectInputs::new(
            &design.design_sketch_placements,
            &design.sketch_points,
            &design.sketch_curve_identities,
            &design.sketch_relations,
            &design.sketch_texts,
        )
        .with_scan(inputs.scan),
        geometry.tolerances.linear.get(),
    )?;
    model.sketches = planar.sketches;
    model.sketch_entities = planar.entities;
    losses.extend(planar.losses);
    let spatial = crate::design::sketch_project::project_spatial_sketch_design(
        ctx,
        &design.design_sketch_placements,
        &design.sketch_points,
        &design.sketch_curve_identities,
        &design.sketch_surfaces,
        &design.sketch_relations,
        geometry.tolerances.linear.get(),
    )?;
    model.spatial_sketches = spatial.sketches;
    model.spatial_sketch_entities = spatial.entities;
    losses.extend(spatial.losses);
    crate::design::feature_project::bind_work_point_sketch_point_constructions(
        &mut model.features,
        &design.design_parameter_scopes,
        &model.sketch_entities,
        &model.spatial_sketch_entities,
    );
    crate::design::feature_project::bind_hole_sketch_point_profiles(
        &mut model.features,
        &design.design_parameter_scopes,
        &model.sketch_entities,
    );
    let profile_work = crate::design::geometry::ProfileGeometryWork::new();
    crate::design::profile_select::bind_sweep_sketch_selections(
        &mut model.features,
        &crate::design::profile_select::SketchCurveSelectionResolution {
            scopes: &design.design_parameter_scopes,
            profile_curves: &design.design_profile_curve_selections,
            groups: &design.design_construction_operand_groups,
            operands: &design.design_entity_selection_operands,
            placements: &design.design_sketch_placements,
            curve_identities: &design.sketch_curve_identities,
            sketches: &model.sketches,
            sketch_entities: &model.sketch_entities,
            spatial_sketches: &model.spatial_sketches,
            spatial_sketch_entities: &model.spatial_sketch_entities,
        },
    )?;
    crate::design::profile_select::bind_split_face_sketch_selections(
        &mut model.features,
        &crate::design::profile_select::SketchCurveSelectionResolution {
            scopes: &design.design_parameter_scopes,
            profile_curves: &design.design_profile_curve_selections,
            groups: &design.design_construction_operand_groups,
            operands: &design.design_entity_selection_operands,
            placements: &design.design_sketch_placements,
            curve_identities: &design.sketch_curve_identities,
            sketches: &model.sketches,
            sketch_entities: &model.sketch_entities,
            spatial_sketches: &model.spatial_sketches,
            spatial_sketch_entities: &model.spatial_sketch_entities,
        },
    );
    crate::design::profile_select::bind_surface_trim_sketch_selections(
        &mut model.features,
        &crate::design::profile_select::SketchCurveSelectionResolution {
            scopes: &design.design_parameter_scopes,
            profile_curves: &design.design_profile_curve_selections,
            groups: &design.design_construction_operand_groups,
            operands: &design.design_entity_selection_operands,
            placements: &design.design_sketch_placements,
            curve_identities: &design.sketch_curve_identities,
            sketches: &model.sketches,
            sketch_entities: &model.sketch_entities,
            spatial_sketches: &model.spatial_sketches,
            spatial_sketch_entities: &model.spatial_sketch_entities,
        },
    );
    crate::design::profile_select::bind_loft_and_revolve_sketch_selections(
        scan,
        &design.design_construction_operand_groups,
        &design.design_record_headers,
        &crate::design::profile_select::SketchProfileResolution {
            entities: &prereqs.design_entity_headers,
            entity_selection_operands: &design.design_entity_selection_operands,
            placements: &design.design_sketch_placements,
            curve_identities: &design.sketch_curve_identities,
            sketches: &model.sketches,
            sketch_entities: &model.sketch_entities,
            spatial_sketches: &model.spatial_sketches,
            spatial_sketch_entities: &model.spatial_sketch_entities,
            linear_tolerance: geometry.tolerances.linear.get(),
            angular_tolerance: geometry.tolerances.angular.get(),
        },
        &mut model.features,
    )?;
    crate::design::feature_project::orient_revolve_axes(
        &mut model.features,
        &design.design_parameter_scopes,
        &model.sketches,
        &model.sketch_entities,
        (
            geometry.tolerances.linear.get(),
            geometry.tolerances.angular.get(),
        ),
    );
    crate::design::feature_project::bind_sketch_feature_geometry(
        &mut model.features,
        &design.design_parameter_scopes,
        &design.design_sketch_placements,
        &model.sketches,
        &model.spatial_sketches,
    )?;
    model.spatial_sketch_constraints =
        crate::design::sketch_project::project_spatial_sketch_constraints(
            &design.design_sketch_placements,
            &design.sketch_relations,
            &design.sketch_points,
            &design.sketch_curve_identities,
            &design.sketch_surfaces,
            &model.spatial_sketch_entities,
        );
    crate::design::profile_select::bind_extrude_profile_selections(
        &mut model.features,
        &design.design_parameter_scopes,
        &design.design_extrude_selection_groups,
        &design.design_extrude_selection_members,
        &model.sketches,
        &crate::design::profile_select::SketchCurveSelectionResolution {
            scopes: &design.design_parameter_scopes,
            profile_curves: &design.design_profile_curve_selections,
            groups: &design.design_construction_operand_groups,
            operands: &design.design_entity_selection_operands,
            placements: &design.design_sketch_placements,
            curve_identities: &design.sketch_curve_identities,
            sketches: &model.sketches,
            sketch_entities: &model.sketch_entities,
            spatial_sketches: &model.spatial_sketches,
            spatial_sketch_entities: &model.spatial_sketch_entities,
        },
        crate::design::profile_select::ExtrudeProfileResolution {
            entities: &model.sketch_entities,
            spatial_sketches: &model.spatial_sketches,
            spatial_entities: &model.spatial_sketch_entities,
            histories: &design.asm_histories,
            scope_histories: &scope_histories,
            linear_tolerance: geometry.tolerances.linear.get(),
            angular_tolerance: geometry.tolerances.angular.get(),
            profile_work: &profile_work,
            ctx,
        },
    )?;
    losses.extend(profile_work.losses());
    if matches!(geometry.path, DesignPath::Geometry) {
        crate::history::discard_projection_caches(&mut design.asm_histories);
    }
    let mut extrude_face_resolution = crate::design::face_resolve::ExtrudeFaceResolution {
        faces: geometry.faces,
        surfaces: geometry.surfaces,
        groups: &design.design_construction_operand_groups,
        operands: &mut design.design_face_operands,
        linear_tolerance: geometry.tolerances.linear.get(),
        angular_tolerance: geometry.tolerances.angular.get(),
    };
    crate::design::face_resolve::bind_extrude_start_planes(
        &mut model.features,
        &model.sketches,
        &mut extrude_face_resolution,
    );
    crate::design::face_resolve::bind_extrude_target_faces(
        &mut model.features,
        &model.sketches,
        &mut extrude_face_resolution,
    );
    model.sketch_constraints = crate::design::constraints::project_sketch_constraints(
        ctx,
        &design.design_sketch_placements,
        &prereqs.design_parameters,
        (
            &design.sketch_points,
            &design.sketch_curve_identities,
            &design.sketch_texts,
        ),
        &design.sketch_relations,
        &model.sketch_entities,
    )?;
    let constraint_inputs = crate::design::dimensions::DimensionConstraintInputs {
        placements: &design.design_sketch_placements,
        parameters: &prereqs.design_parameters,
        owners: &prereqs.design_parameter_owners,
        pairs: &design.design_dimension_locus_pairs,
        groups: &design.design_dimension_locus_groups,
        annotation_frames: &design.design_dimension_annotation_frames,
        null_pairs: &design.design_dimension_null_locus_pairs,
        companions: &design.design_parameter_companions,
        recipe_records: &design.design_dimension_recipe_records,
        points: &design.sketch_points,
        curves: &design.sketch_curve_identities,
        entities: &model.sketch_entities,
    };
    let dimension_constraints = if design.design_dimension_presentation_frames.is_empty() {
        crate::design::dimensions::project_dimension_constraints(
            &constraint_inputs,
            &model.spatial_sketches,
            geometry.tolerances.linear.get(),
        )
    } else {
        crate::design::dimensions::project_dimension_constraints_with_presentations(
            &constraint_inputs,
            &design.design_dimension_presentation_frames,
            &model.spatial_sketches,
            geometry.tolerances.linear.get(),
        )
    };
    model.sketch_constraints.extend(dimension_constraints);
    let spatial_dimension_constraints =
        crate::design::dimensions::project_spatial_dimension_constraints(
            &constraint_inputs,
            &model.spatial_sketches,
            &model.spatial_sketch_entities,
            geometry.tolerances.linear.get(),
        );
    model
        .spatial_sketch_constraints
        .extend(spatial_dimension_constraints);
    crate::design::dimensions::bind_offset_dimension_parameters(
        &mut model.sketch_constraints,
        &prereqs.design_parameters,
    );
    model.sketch_constraints.sort_by(|a, b| a.id.cmp(&b.id));
    model
        .spatial_sketch_constraints
        .sort_by(|a, b| a.id.cmp(&b.id));
    crate::history::face_admission::admit_feature_input_faces(
        &mut model.features,
        &model.feature_input_topologies,
        geometry.faces,
        &mut losses,
    )?;
    if let Some(table) = prereqs.xref_table() {
        losses.extend(crate::xref::bind_component_insert_features(
            &mut model.features,
            &design.design_parameter_scopes,
            table,
            products.occurrences,
        ));
    }
    let occurrence_paths = crate::design::components::paths::LocalOccurrencePaths::new(
        &prereqs.design_component_occurrences,
        prereqs
            .xref_table()
            .map_or(&[], |table| table.references.as_slice()),
        products.occurrences,
    );
    crate::design::components::project_derived_instance_features(
        &mut model.features,
        &design.design_parameter_scopes,
    );
    losses.extend(crate::design::components::paths::bind_feature_paths(
        &mut model.features,
        &occurrence_paths,
    ));
    crate::design::components::bind_unresolved_component_insert_features(
        &mut model.features,
        &design.design_parameter_scopes,
        products.occurrences,
    );
    let mut types_by_stream = std::collections::HashMap::new();
    let bound_at_merge = design
        .design_parameter_scopes
        .iter()
        .filter(|scope| {
            let stream = crate::ids::native_stream(&scope.id).unwrap_or_default();
            let types = types_by_stream.entry(stream).or_insert_with(|| {
                stream
                    .strip_prefix("f3d:")
                    .and_then(crate::ids::decode_identity_key_component)
                    .map(|entry| {
                        crate::design::decode::meta::stream_types_by_entity(
                            &prereqs.design_types,
                            &entry,
                        )
                    })
                    .unwrap_or_default()
            });
            crate::design::decode::scopes::assembly_v3::joint_binds_after_merge(scope, types)
        })
        .map(|scope| scope.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let bound_now = products
        .assembly_joints
        .iter()
        .filter(|joint| {
            !joint
                .native_ref
                .as_deref()
                .is_some_and(|scope| bound_at_merge.contains(scope))
        })
        .cloned()
        .collect::<Vec<_>>();
    crate::design::assembly::bind_assembly_joint_features(&mut model.features, &bound_now);
    crate::history::retain_selected_edge_geometry(
        inputs.ctx,
        &model.features,
        &mut model.feature_input_topologies,
    )?;
    model.configurations = crate::design::configurations::project_configurations(
        &design.design_configurations,
        &crate::design::configurations::ConfigurationTargets {
            parameters: &prereqs.design_parameters,
            owners: &prereqs.design_parameter_owners,
            scopes: &design.design_parameter_scopes,
            neutral_parameters: &model.parameters,
            neutral_features: &model.features,
        },
    )?;
    let phase_one_headers = prereqs
        .design_record_headers
        .iter()
        .chain(&prereqs.design_owner_record_headers)
        .map(|header| header.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    design
        .design_record_headers
        .retain(|header| !phase_one_headers.contains(header.id.as_str()));
    Ok(DesignOutputs {
        native: design,
        model,
        losses,
    })
}

/// Indexed records named by parameter scopes: every scope reference and the
/// record named by a reference-bearing compact Thread trailer, which need not
/// occur in the scope reference table.
pub(super) fn scope_related_indices(
    scopes: &[crate::records::feature::scope::DesignParameterScope],
) -> Vec<(String, u32)> {
    collect_related_indices(scopes.iter().flat_map(|scope| {
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let thread_trailer =
            scope
                .thread_construction()
                .and_then(|construction| match &construction.form {
                    crate::records::feature::thread::DesignThreadForm::Compact(Some(reference))
                    | crate::records::feature::thread::DesignThreadForm::CompactExtended(reference) => {
                        Some(reference.value.get())
                    }
                    _ => None,
                });
        scope
            .reference_members()
            .values()
            .copied()
            .chain(thread_trailer)
            .chain(
                scope
                    .occurrence_transfer
                    .as_ref()
                    .into_iter()
                    .flat_map(crate::records::feature::occurrence_transfer::DesignOccurrenceTransfer::record_indices),
            )
            .map(move |record_index| (stream, record_index))
    }))
}

/// Indexed records named by parameter owners: the scope, parameter and
/// companion records. The legacy owner forms store scope `0` as a sentinel
/// that names no record.
pub(super) fn owner_related_indices(
    owners: &[crate::records::parameters::DesignParameterOwner],
) -> Vec<(String, u32)> {
    collect_related_indices(owners.iter().flat_map(|owner| {
        let stream = crate::ids::native_stream(owner.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
        [
            Some(owner.scope_record_index()).filter(|scope| *scope != 0),
            Some(owner.parameter_record_index()),
            Some(owner.companion_record_index()),
        ]
        .into_iter()
        .flatten()
        .map(move |record_index| (stream, record_index))
    }))
}

fn extend_related_design_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    prereqs: &ProductPrereqs,
    timelines: &[DesignFeatureTimeline],
    geometry: &DesignGeometry<'_>,
    design: &mut DesignNative,
) -> Result<(usize, Vec<LossNote>), CodecError> {
    let indices = collect_related_indices(design.sketch_relations.iter().flat_map(|relation| {
        let scope = crate::ids::native_stream(&relation.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        relation
            .all_member_indices()
            .map(move |record_index| (scope, record_index))
    }));
    append_related_record_headers(scan, &mut design.design_record_headers, &indices)?;
    let indices = owner_related_indices(&prereqs.design_parameter_owners);
    append_related_record_headers(scan, &mut design.design_record_headers, &indices)?;
    design.design_parameter_companions =
        crate::design::decode::parameters::decode_parameter_companions(
            scan,
            &prereqs.design_parameter_owners,
            &design.design_record_headers,
        )?;
    design.design_face_source_groups = crate::design::decode::operands::decode_face_source_groups(
        ctx,
        scan,
        &prereqs.design_types,
        &design.design_parameter_scopes,
    )?;
    design.design_canvas_images =
        crate::design::decode::canvas::decode_canvas_images(scan, &design.design_parameter_scopes)?;
    design.design_decal_images =
        crate::design::decode::decal::decode_decal_images(scan, &design.design_parameter_scopes)?;
    crate::design::decode::operands::disambiguate_fixed_fillet_parameters(
        &mut design.design_parameter_scopes,
        &prereqs.design_parameter_owners,
    );
    let mut existing = design
        .design_record_headers
        .iter()
        .filter_map(|record| Some((crate::ids::native_stream(&record.id)?, record.record_index)))
        .collect::<std::collections::HashSet<_>>();
    let mut scope_headers = Vec::new();
    for scope in &design.design_parameter_scopes {
        let Some(stream) = crate::ids::native_stream(&scope.id) else {
            continue;
        };
        if !existing.contains(&(stream, scope.record_index)) {
            existing.insert((stream, scope.record_index));
            let id = format!("{stream}:design-record-header#{}", scope.byte_offset());
            scope_headers.push(crate::records::decal::DesignRecordHeader {
                id,
                record_index: scope.record_index,
                class_tag: scope.class_tag.clone(),
                byte_offset: scope.byte_offset(),
            });
        }
        if let Some(operation) = scope.copy_paste_bodies_operation() {
            if !existing.contains(&(stream, operation.relation_record_index)) {
                existing.insert((stream, operation.relation_record_index));
                let id = format!(
                    "{stream}:design-record-header#{}",
                    operation.relation_byte_offset()
                );
                scope_headers.push(crate::records::decal::DesignRecordHeader {
                    id,
                    record_index: operation.relation_record_index,
                    class_tag: operation.relation_class_tag.clone(),
                    byte_offset: operation.relation_byte_offset(),
                });
            }
        }
    }
    drop(existing);
    design.design_record_headers.extend(scope_headers);
    let indices = scope_related_indices(&design.design_parameter_scopes);
    append_related_record_headers(scan, &mut design.design_record_headers, &indices)?;
    let profile_losses = crate::design::decode::operands::bind_sketch_profiles(
        scan,
        &mut design.design_parameter_scopes,
        &design.design_record_headers,
        &prereqs.design_entity_headers,
    )?;
    design.design_construction_operand_groups =
        crate::design::decode::operands::decode_construction_operand_groups(
            scan,
            &mut design.design_parameter_scopes,
            &design.design_record_headers,
        )?;
    crate::design::decode::scopes::pipe::bind_constructions(
        scan,
        &crate::design::decode::scopes::pipe::PipeSources {
            types: &prereqs.design_types,
            headers: &design.design_record_headers,
            owners: &prereqs.design_parameter_owners,
            groups: &design.design_construction_operand_groups,
        },
        &mut design.design_parameter_scopes,
    )?;
    design.design_loft_legacy_body_carriers =
        crate::design::decode::operands::decode_loft_legacy_body_carriers(
            scan,
            &design.design_parameter_scopes,
            &design.design_record_headers,
        )?;
    crate::design::decode::scopes::mirror::bind_mirror_constructions(
        ctx,
        scan,
        &mut design.design_parameter_scopes,
        &design.design_construction_operand_groups,
        &design.design_record_headers,
        &prereqs.design_parameter_owners,
        &prereqs.construction_recipes,
    )?;
    design.design_extrude_selection_groups =
        crate::design::decode::operands::decode_extrude_selection_groups(
            scan,
            &design.design_parameter_scopes,
            &design.design_record_headers,
        )?;
    let indices = collect_related_indices(
        design
            .design_extrude_selection_groups
            .iter()
            .flat_map(|group| {
                let stream =
                    crate::ids::native_stream(&group.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                group
                    .members()
                    .iter()
                    .map(move |record_index| (stream, record_index.value))
            })
            .chain(
                design
                    .design_construction_operand_groups
                    .iter()
                    .flat_map(|group| {
                        let stream = crate::ids::native_stream(&group.id)
                            .unwrap_or(crate::ids::DEFAULT_STREAM);
                        group
                            .members()
                            .iter()
                            .map(|member| member.value)
                            .chain(
                                group
                                    .frame
                                    .trailing_records()
                                    .iter()
                                    .map(|record| &record.value)
                                    .flat_map(|record_index| {
                                        std::iter::once(*record_index)
                                            .chain(record_index.checked_add(1))
                                            .chain(record_index.checked_add(2))
                                            .chain(record_index.checked_add(3))
                                    }),
                            )
                            .chain(
                                group
                                    .frame
                                    .auxiliary_records
                                    .iter()
                                    .map(|record| &record.value)
                                    .flat_map(|record_index| {
                                        std::iter::once(*record_index)
                                            .chain(record_index.checked_add(1))
                                            .chain(record_index.checked_add(2))
                                    }),
                            )
                            .map(move |record_index| (stream, record_index))
                    }),
            ),
    );
    append_related_record_headers(scan, &mut design.design_record_headers, &indices)?;
    crate::design::decode::operands::bind_construction_operand_trailing_records(
        scan,
        &mut design.design_construction_operand_groups,
        &design.design_record_headers,
    )?;
    crate::design::decode::operands::bind_construction_operand_paths(
        scan,
        &mut design.design_construction_operand_groups,
        &design.design_record_headers,
    )?;
    let (identities, untyped_tracking_paths) =
        crate::design::decode::operands::decode_construction_operand_identities(
            scan,
            &design.design_construction_operand_groups,
            &design.design_record_headers,
        )?;
    design.design_construction_operand_identities = identities;
    let scopes = design
        .design_parameter_scopes
        .iter()
        .filter_map(|scope| {
            Some((
                (crate::ids::native_stream(&scope.id)?, scope.record_index),
                scope.kind(),
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let identified_groups = design
        .design_construction_operand_identities
        .iter()
        .filter_map(|identity| {
            Some((
                crate::ids::native_stream(&identity.id)?,
                identity.group_record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    design.design_edge_identity_operands =
        crate::design::decode::operands::decode_edge_identity_operands(
            scan,
            &design.design_parameter_scopes,
            &design.design_construction_operand_groups,
            &design.design_record_headers,
        )?;
    let identity_member_groups = design
        .design_edge_identity_operands
        .iter()
        .filter_map(|operand| {
            Some((
                crate::ids::native_stream(&operand.id)?,
                operand.group_record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    design.design_construction_operand_groups.retain(|group| {
        let Some(stream) = crate::ids::native_stream(&group.id) else {
            return true;
        };
        let kind = scopes.get(&(stream, group.scope_record_index));
        crate::design::decode::operands::construction_operand_group_is_retained(
            kind,
            identified_groups.contains(&(stream, group.record_index))
                || identity_member_groups.contains(&(stream, group.record_index)),
        )
    });
    design.design_fillet_radius_groups =
        crate::design::decode::operands::decode_fillet_radius_groups(
            &design.design_parameter_scopes,
            &design.design_construction_operand_groups,
            &prereqs.design_parameter_owners,
            &prereqs.design_parameters,
        );
    crate::design::decode::operands::bind_lost_edge_groups(
        &mut design.design_construction_operand_groups,
        &design.design_construction_operand_identities,
        &design.lost_edge_references,
    );
    let indices = collect_related_indices(
        design
            .design_construction_operand_identities
            .iter()
            .flat_map(|identity| {
                let stream =
                    crate::ids::native_stream(&identity.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                identity
                    .wrappers()
                    .iter()
                    .map(|wrapper| wrapper.record_index)
                    .chain(std::iter::once(identity.following_record_index()))
                    .map(move |record_index| (stream, record_index))
            })
            .chain(
                design
                    .design_construction_operand_groups
                    .iter()
                    .filter_map(|group| {
                        let stream = crate::ids::native_stream(&group.id)?;
                        Some(
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .map(move |record_index| (stream, record_index)),
                        )
                    })
                    .flatten(),
            ),
    );
    append_related_record_headers(scan, &mut design.design_record_headers, &indices)?;
    design.design_extrude_selection_members =
        crate::design::decode::operands::decode_extrude_selection_members(
            scan,
            &design.design_parameter_scopes,
            &design.design_extrude_selection_groups,
            &design.design_record_headers,
        )?;
    design.design_entity_selection_operands =
        crate::design::decode::operands::decode_entity_selection_operands(
            scan,
            &design.design_construction_operand_groups,
            &design.design_record_headers,
        )?;
    design.design_profile_curve_selections =
        crate::design::decode::operands::decode_profile_curve_selections(
            ctx,
            scan,
            &design.design_construction_operand_groups,
            &design.design_record_headers,
        )?;
    crate::history::selection::bind_entity_selection_history(
        &mut design.design_entity_selection_operands,
        &design.design_parameter_scopes,
        &design.asm_histories,
    );
    crate::history::selection::bind_hole_selection_history(
        &mut design.design_parameter_scopes,
        &design.asm_histories,
    );
    design.design_body_recipe_operands =
        crate::design::decode::operands::decode_body_recipe_operands(
            scan,
            &design.design_parameter_scopes,
            &design.design_construction_operand_groups,
            &design.design_record_headers,
            &prereqs.construction_recipes,
        )?;
    crate::design::decode::operands::release_extrude_body_target_groups(
        &mut design.design_construction_operand_groups,
        &design.design_body_recipe_operands,
    );
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        &mut design.design_body_recipe_operands,
        &prereqs.construction_recipes,
        geometry.persistent_subentity_tags,
        &design.design_parameter_scopes,
    );
    crate::history::bind_body_recipe_operand_history_candidates(
        &mut design.design_body_recipe_operands,
        &prereqs.construction_recipes,
        &design.design_parameter_scopes,
        crate::history::BindingHistories::Release(&mut design.asm_histories),
    )?;
    crate::design::decode::operands::bind_extrude_selection_identities(
        &mut design.design_extrude_selection_members,
        &design.design_construction_operand_identities,
    );
    crate::history::selection::bind_extrude_selection_history(
        &mut design.design_extrude_selection_members,
        &prereqs.design_component_naming_spaces,
        &prereqs.design_body_bindings,
        &design.asm_histories,
    );
    let mut scope_histories = crate::history::bind_scope_histories(
        &design.design_parameter_scopes,
        &prereqs.design_body_bindings,
        &design.design_body_recipe_operands,
        &design.asm_histories,
    );
    let owner_sources = crate::design::decode::operands::compact_owner::source_selectors(
        scan,
        &design.design_construction_operand_groups,
        &design.design_construction_operand_identities,
        &prereqs.design_body_bindings,
    )?;
    let lineage_scopes = crate::history::compact_lineage::bind(
        crate::history::compact_lineage::Inputs {
            scopes: &design.design_parameter_scopes,
            groups: &design.design_construction_operand_groups,
            identities: &design.design_construction_operand_identities,
            operands: &design.design_edge_identity_operands,
            recipes: &design.design_body_recipe_operands,
            owners: &owner_sources,
            histories: &design.asm_histories,
        },
        &mut scope_histories,
    );
    crate::history::selection::bind_circular_pattern_axes(
        &mut design.design_parameter_scopes,
        &design.asm_histories,
        &scope_histories,
    );
    crate::history::selection::bind_edge_identity_history(
        &mut design.design_edge_identity_operands,
        &design.design_construction_operand_identities,
        &design.design_parameter_scopes,
        crate::history::BindingHistories::Release(&mut design.asm_histories),
        &scope_histories,
        &lineage_scopes,
    );
    design.design_edge_operands = crate::design::decode::operands::decode_edge_operands(
        ctx,
        scan,
        &design.design_parameter_scopes,
        &design.design_construction_operand_groups,
        &design.design_record_headers,
        &prereqs.construction_recipes,
    )?;
    crate::design::decode::operands::bind_edge_operand_candidates(
        &mut design.design_edge_operands,
        &prereqs.construction_recipes,
        geometry.persistent_subentity_tags,
    );
    design.asm_face_boundaries = crate::history::bind_edge_operand_history_candidates(
        &mut design.design_edge_operands,
        &design.design_parameter_scopes,
        &prereqs.construction_recipes,
        crate::history::BindingHistories::Release(&mut design.asm_histories),
        &scope_histories,
    )?;
    design.design_edge_treatment_vertex_operands =
        crate::design::decode::operands::decode_edge_treatment_vertex_operands(
            scan,
            &design.design_parameter_scopes,
            &design.design_construction_operand_groups,
            &design.design_record_headers,
            &prereqs.construction_recipes,
        )?;
    crate::design::decode::operands::bind_edge_treatment_vertex_candidates(
        &mut design.design_edge_treatment_vertex_operands,
        geometry.persistent_subentity_tags,
    );
    crate::history::bind_edge_treatment_vertex_history(
        &mut design.design_edge_treatment_vertex_operands,
        &design.design_parameter_scopes,
        &design.asm_histories,
        &scope_histories,
    )?;
    crate::design::decode::operands::bind_work_plane_constructions(
        scan,
        &mut design.design_parameter_scopes,
        &design.design_record_headers,
        &prereqs.construction_recipes,
        &prereqs.design_parameter_owners,
        &prereqs.design_parameters,
    )?;
    crate::design::decode::operands::bind_vertex_recipe_candidates(
        &mut design.design_parameter_scopes,
        geometry.persistent_subentity_tags,
    );
    crate::history::bind_vertex_recipe_history(
        &mut design.design_parameter_scopes,
        timelines,
        &design.asm_histories,
    )?;
    design.design_face_operands = crate::design::decode::operands::decode_face_operands(
        ctx,
        scan,
        &design.design_parameter_scopes,
        &design.design_construction_operand_groups,
        &design.design_record_headers,
        &prereqs.construction_recipes,
    )?;
    crate::design::decode::operands::bind_face_operand_candidates(
        &mut design.design_face_operands,
        &prereqs.construction_recipes,
        geometry.persistent_subentity_tags,
    );
    crate::history::bind_face_operand_history_candidates(
        ctx,
        &mut design.design_face_operands,
        &design.design_parameter_scopes,
        &design.design_construction_operand_groups,
        &prereqs.construction_recipes,
        crate::history::BindingHistories::Release(&mut design.asm_histories),
        &scope_histories,
    )?;
    crate::history::selection::bind_mirror_selection_planes(
        &mut design.design_parameter_scopes,
        &design.design_construction_operand_groups,
        &design.design_entity_selection_operands,
        &design.design_face_operands,
        &design.design_construction_operand_identities,
        &design.asm_histories,
    );
    crate::history::selection::bind_edge_identity_bounded_face_rules(
        &mut design.design_edge_identity_operands,
        &design.design_face_operands,
    );
    design.design_sketch_placements = crate::design::decode::sketch::decode_sketch_placements(
        scan,
        &design.design_parameter_scopes,
        &prereqs.design_entity_headers,
    )?;
    let mut stream_lengths = std::collections::HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = crate::ids::native_scope(&entry.name);
        stream_lengths.insert(stream, bytes.len());
    }
    design.design_parameter_companions =
        crate::design::decode::parameters::bind_parameter_companion_payloads(
            std::mem::take(&mut design.design_parameter_companions),
            &crate::design::decode::parameters::ParameterCompanionInputs {
                parameters: &prereqs.design_parameters,
                owners: &prereqs.design_parameter_owners,
                scopes: &design.design_parameter_scopes,
                entities: &prereqs.design_entity_headers,
                headers: &design.design_record_headers,
                recipes: &prereqs.construction_recipes,
                stream_lengths: &stream_lengths,
            },
        )?;
    design.design_dimension_recipe_records =
        crate::design::decode::dimension_frames::decode_dimension_recipe_records(
            scan,
            &prereqs.design_parameters,
            &prereqs.design_parameter_owners,
            &design.design_parameter_companions,
            &prereqs.construction_recipes,
        )?;
    crate::design::decode::dimension_frames::bind_dimension_recipe_reference_candidates(
        &mut design.design_dimension_recipe_records,
        geometry.persistent_subentity_tags,
    );
    crate::design::decode::dimension_frames::bind_dimension_recipe_edge_operands(
        &mut design.design_dimension_recipe_records,
        &design.design_edge_operands,
    );
    Ok((untyped_tracking_paths, profile_losses))
}
