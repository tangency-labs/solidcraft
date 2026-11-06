// SPDX-License-Identifier: Apache-2.0
//! Assemble a `.f3d` archive into a [`CadIr`] document and [`DecodeBody`].
//!
//! [`crate::container`] scans the ZIP, reads ASM headers, finds the history
//! boundary. This module resolves Design body-to-blob bindings, frames every
//! referenced B-rep with [`cadmpeg_asm::sab`], builds topology and geometry through
//! [`crate::brep`], then
//! adds design, sketch, history, ACT, and appearance data.
//!
//! A framing failure or a stream without decoded geometry produces a
//! metadata-only document. The report marks geometry and topology as blocking,
//! and retained source data remains available for native replay.

#![cfg_attr(
    test,
    allow(clippy::default_trait_access, clippy::field_reassign_with_default)
)]
use cadmpeg_core::decode::u64_from_index;

use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

use cadmpeg_core::container::ContainerRole;

use crate::native::F3dNative;
use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{
    loss::{LossCategory, LossTaxonomy},
    Severity,
};
use cadmpeg_ir::units::{Tolerances, UnitVector3};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::brep::{self, Brep};
use crate::container::{self, BrepFacts, ContainerScan};
use crate::loss::F3dLossCode;
use crate::materials;
use cadmpeg_asm::{asm_header, sab};

fn index_selected_body_key(
    index: &mut std::collections::HashMap<String, std::collections::HashSet<u64>>,
    blob_name: &str,
    body_key: u64,
) {
    if let Some(keys) = index.get_mut(blob_name) {
        keys.insert(body_key);
        return;
    }

    let name = blob_name.to_owned();
    let mut keys = std::collections::HashSet::new();
    keys.insert(body_key);
    index.insert(name, keys);
}

fn body_visibility_for<'m>(
    index: &'m std::collections::HashMap<
        (String, u64),
        crate::design::decode::body::DecodedBodyVisibility,
    >,
    blob_name: &str,
    body_key: u64,
) -> Option<&'m crate::design::decode::body::DecodedBodyVisibility> {
    let name = blob_name.to_string();
    index.get(&(name, body_key))
}

fn join_text_brep_names(scan: &ContainerScan<'_>) -> String {
    let mut joined = String::new();
    for (index, name) in container::text_brep_names(scan).enumerate() {
        if index != 0 {
            joined.push_str("`, `");
        }
        joined.push_str(name);
    }
    joined
}

fn container_only_dimension_parameters(
    native: &F3dNative,
) -> std::collections::HashSet<cadmpeg_ir::features::ParameterId> {
    let container_only = crate::design::dimensions::container_only_dimension_companions(
        &native.design_dimension_locus_pairs,
        &native.design_dimension_null_locus_pairs,
        &native.design_dimension_annotation_frames,
        &native.design_dimension_locus_groups,
        &native.design_dimension_recipe_records,
    );
    let mut parameters_by_id = std::collections::HashSet::new();
    for owner in &native.design_parameter_owners {
        let stream = crate::ids::native_stream(owner.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
        if !container_only.contains(&(stream, owner.companion_record_index())) {
            continue;
        }
        let mut parameters = native.design_parameters.iter().filter(|parameter| {
            crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM) == stream
                && parameter.record_index == owner.parameter_record_index()
                && parameter.kind() == crate::records::parameters::DesignParameterKind::Dimension
        });
        let Some(parameter) = parameters.next() else {
            continue;
        };
        if parameters.next().is_none() {
            let id = crate::ids::neutral_parameter_id(parameter);
            parameters_by_id.insert(id);
        }
    }
    parameters_by_id
}

/// Design records that the dimension-companion count reads.
struct DimensionCompanionSources<'a> {
    parameters: &'a [crate::records::parameters::DesignParameter],
    owners: &'a [crate::records::parameters::DesignParameterOwner],
    companions: &'a [crate::records::parameters::DesignParameterCompanion],
    locus_pairs: &'a crate::records::dimension_locus_arenas::DesignDimensionLocusPairs,
    annotation_frames: &'a [crate::records::dimensions::DesignDimensionAnnotationFrame],
    locus_groups: &'a [crate::records::dimensions::DesignDimensionLocusGroup],
    null_locus_pairs: &'a crate::records::dimension_locus_arenas::DesignDimensionNullLocusPairs,
    recipe_records: &'a [crate::records::dimensions::DesignDimensionRecipeRecord],
}

impl<'a> DimensionCompanionSources<'a> {
    fn new(prereqs: &'a prereqs::ProductPrereqs, design: &'a design_intent::DesignNative) -> Self {
        Self {
            parameters: &prereqs.design_parameters,
            owners: &prereqs.design_parameter_owners,
            companions: &design.design_parameter_companions,
            locus_pairs: &design.design_dimension_locus_pairs,
            annotation_frames: &design.design_dimension_annotation_frames,
            locus_groups: &design.design_dimension_locus_groups,
            null_locus_pairs: &design.design_dimension_null_locus_pairs,
            recipe_records: &design.design_dimension_recipe_records,
        }
    }

    #[cfg(test)]
    fn from_native(native: &'a F3dNative) -> Self {
        Self {
            parameters: &native.design_parameters,
            owners: &native.design_parameter_owners,
            companions: &native.design_parameter_companions,
            locus_pairs: &native.design_dimension_locus_pairs,
            annotation_frames: &native.design_dimension_annotation_frames,
            locus_groups: &native.design_dimension_locus_groups,
            null_locus_pairs: &native.design_dimension_null_locus_pairs,
            recipe_records: &native.design_dimension_recipe_records,
        }
    }
}

fn unresolved_dimension_companion_count(
    sources: &DimensionCompanionSources<'_>,
    ir: &CadIr,
) -> usize {
    use std::collections::{HashMap, HashSet};

    let mut parameters = HashMap::new();

    for parameter in sources.parameters {
        parameters.insert(
            (
                crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM),
                parameter.record_index,
            ),
            parameter.kind(),
        );
    }
    let mut dimension_owners = HashSet::new();
    for owner in sources.owners {
        let stream = crate::ids::native_stream(owner.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
        if parameters.get(&(stream, owner.parameter_record_index()))
            == Some(&crate::records::parameters::DesignParameterKind::Dimension)
            && !dimension_owners.contains(&(stream, owner.record_index()))
        {
            dimension_owners.insert((stream, owner.record_index()));
        }
    }
    let mut typed = HashSet::new();
    let mut insert_typed = |key| {
        if !typed.contains(&key) {
            typed.insert(key);
        }
    };
    for pair in sources.locus_pairs {
        insert_typed((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.companion_record_index,
        ));
        insert_typed((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.governing_companion_record_index,
        ));
    }
    for frame in sources.annotation_frames {
        insert_typed((
            crate::ids::native_stream(&frame.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            frame.governing_companion_record_index,
        ));
    }
    for group in sources.locus_groups {
        insert_typed((
            crate::ids::native_stream(&group.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            group.companion_record_index,
        ));
    }
    for pair in sources.null_locus_pairs {
        insert_typed((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.companion_record_index,
        ));
        insert_typed((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.governing_companion_record_index,
        ));
    }
    for record in sources.recipe_records {
        insert_typed((
            crate::ids::native_stream(&record.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            record.companion_record_index,
        ));
    }
    for constraint in &ir.model.sketch_constraints {
        if !matches!(
            constraint.definition.kind(),
            cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { .. }
        ) {
            if let Some(native_ref) = &constraint.native_ref {
                if let Some(companion) = sources
                    .companions
                    .iter()
                    .find(|companion| companion.id() == *native_ref)
                {
                    insert_typed((
                        crate::ids::native_stream(native_ref).unwrap_or(crate::ids::DEFAULT_STREAM),
                        companion.record_index(),
                    ));
                }
            }
        }
    }
    sources
        .companions
        .iter()
        .filter(|companion| {
            let stream =
                crate::ids::native_stream(companion.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
            companion
                .payload()
                .is_some_and(|payload| payload.byte_length() > 0)
                && dimension_owners.contains(&(stream, companion.owner_record_index()))
                && !typed.contains(&(stream, companion.record_index()))
        })
        .count()
}

fn report_unresolved_dimension_companions(report: &mut DecodeBody, count: usize) {
    if count != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::DimensionCompanionUntyped, format_args!(
            "{count} payload-bearing Design dimension companion(s) were retained without a typed locus frame."
        ),);
    }
}

fn report_unresolved_configuration_rules(
    report: &mut DecodeBody,
    configurations: &[crate::records::configuration::DesignConfiguration],
    unbound_roots: usize,
    ir: &CadIr,
) {
    if unbound_roots != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::ConfigurationActiveRowUnbound, format_args!(
            "{unbound_roots} Design configuration root record(s) were retained without an admitted active row of a unique table document."
        ),);
    }
    let count =
        crate::design::configurations::unresolved_configuration_member_count(configurations);
    if count != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::ConfigurationMemberUnassigned, format_args!(
            "{count} Design configuration JSON member(s) were retained without assigned neutral configuration semantics."
        ),);
    }
    let count = crate::design::configurations::unresolved_configuration_rule_count(
        configurations,
        &ir.model.configurations,
    );
    if count != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::ConfigurationRuleUnbound, format_args!(
            "{count} nonempty Design configuration rule(s) were retained without an unambiguous neutral activation target."
        ),);
    }
    let count = crate::design::configurations::unresolved_configuration_parameter_override_count(
        &ir.model.configurations,
    );
    if count != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::ConfigurationParameterOverrideUnbound, format_args!(
            "{count} Design configuration parameter override(s) were retained without an unambiguous neutral parameter identity."
        ),);
    }
    let count = crate::design::configurations::unresolved_configuration_suppressed_feature_count(
        &ir.model.configurations,
    );
    if count != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::ConfigurationFeatureSuppressionUnbound, format_args!(
            "{count} Design configuration feature state(s) were retained without an unambiguous neutral feature identity."
        ),);
    }
}

fn report_untyped_material_distances(report: &mut DecodeBody, count: usize) {
    if count != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::MaterialDistanceUnitUntyped, format_args!(
            "{count} Protein texture Distance property value(s) retain an untyped unit tag; their typed texture carriers were omitted."
        ),);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DesignProjectionGaps {
    unresolved_body_bindings: usize,
    history_only_body_bindings: usize,
    incomplete_features: usize,
    native_reference_images: usize,
    native_decals: usize,
    unprojected_feature_scopes: usize,
    unprojected_parameters: usize,
    unresolved_parameter_owners: usize,
    untyped_parameter_units: usize,
    unresolved_expression_dependencies: usize,
    unprojected_history_dependencies: usize,
    ambiguous_history_dependencies: usize,
    native_sketch_relations: usize,
    native_dimensions: usize,
    unprojected_sketch_placements: usize,
    unprojected_sketch_points: usize,
    unprojected_sketch_curves: usize,
    unprojected_sketch_surfaces: usize,
    unprojected_sketch_texts: usize,
    unprojected_sketch_text_styles: usize,
    unprojected_sketch_relations: usize,
    unprojected_sketch_pattern_states: usize,
    unprojected_dimensions: usize,
    profile_selections: usize,
    path_selections: usize,
    face_selections: usize,
    active_face_substitutions: usize,
    body_selections: usize,
    partially_resolved_face_members: usize,
    native_edge_selections: usize,
    partially_resolved_edge_members: usize,
    unresolved_edge_selections: usize,
    unrepaired_lost_edge_references: usize,
}

/// Returns whether a face selection supplies the operation's neutral faces.
///
/// Native or absent selections leave the definition incomplete.
fn face_selection_is_resolved(selection: &cadmpeg_ir::features::FaceSelection) -> bool {
    use cadmpeg_ir::features::FaceSelection;

    match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => !faces.is_empty(),
        FaceSelection::Historical { .. } => true,
        FaceSelection::Generated { .. } => true,
        FaceSelection::HistoricalPartial { .. } => false,
        FaceSelection::Unresolved | FaceSelection::Native(_) => false,
    }
}

fn draft_neutral_plane_is_resolved(
    selection: &cadmpeg_ir::features::FaceSelection,
    pull_plane: Option<&cadmpeg_ir::features::FeatureId>,
    pull_direction: Option<&cadmpeg_ir::math::Vector3>,
) -> bool {
    face_selection_is_resolved(selection)
        || match selection {
            cadmpeg_ir::features::FaceSelection::Native(native) => {
                pull_plane.is_some_and(|plane| plane.as_str() == *native)
                    && pull_direction.is_some_and(|direction| direction.unit().is_some())
            }
            _ => false,
        }
}

fn edge_selection_is_resolved(selection: &cadmpeg_ir::features::EdgeSelection) -> bool {
    use cadmpeg_ir::features::EdgeSelection;

    match selection {
        EdgeSelection::All => true,
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => !edges.is_empty(),
        EdgeSelection::Historical { .. } => true,
        EdgeSelection::Generated { .. } => true,
        EdgeSelection::HistoricalPartial { .. } => false,
        EdgeSelection::Unresolved | EdgeSelection::Native(_) => false,
    }
}

fn datum_plane_reference_is_resolved(
    reference: &cadmpeg_ir::features::DatumPlaneReference,
) -> bool {
    match reference {
        cadmpeg_ir::features::DatumPlaneReference::Feature { .. } => true,
        cadmpeg_ir::features::DatumPlaneReference::Face { face } => {
            face_selection_is_resolved(face)
        }
        cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { .. } => true,
    }
}

fn datum_point_construction_is_resolved(
    construction: &cadmpeg_ir::features::DatumPointConstruction,
) -> bool {
    use cadmpeg_ir::features::{DatumPointConstruction, SketchPointSelection, VertexSelection};

    match construction {
        DatumPointConstruction::CircleCenter { edge } => edge_selection_is_resolved(edge),
        DatumPointConstruction::TwoEdgeIntersection { edges } => {
            edges.iter().all(edge_selection_is_resolved)
        }
        DatumPointConstruction::ThreePlaneIntersection { planes } => {
            planes.iter().all(datum_plane_reference_is_resolved)
        }
        DatumPointConstruction::Vertex { vertex } => matches!(
            vertex,
            VertexSelection::Generated { .. } | VertexSelection::Historical { .. }
        ),
        DatumPointConstruction::SketchPoint { point } => matches!(
            point,
            SketchPointSelection::Planar { .. } | SketchPointSelection::Spatial { .. }
        ),
        DatumPointConstruction::EdgePlaneIntersection { edge, plane } => {
            edge_selection_is_resolved(edge) && datum_plane_reference_is_resolved(plane)
        }
        DatumPointConstruction::DistanceOnEdge { edge, .. } => edge_selection_is_resolved(edge),
    }
}

fn body_selection_is_resolved(selection: &cadmpeg_ir::features::BodySelection) -> bool {
    use cadmpeg_ir::features::BodySelection;

    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            !bodies.is_empty()
        }
        BodySelection::Historical { bodies, .. } => !bodies.is_empty(),
        BodySelection::ResolvedSet { .. } | BodySelection::HistoricalSet { .. } => true,
        BodySelection::Generated { bodies, .. } => !bodies.is_empty(),
        BodySelection::Local { bodies, .. } => !bodies.is_empty(),
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_) => false,
    }
}

fn base_feature_body_selection_is_resolved(
    selection: &cadmpeg_ir::features::BodySelection,
) -> bool {
    body_selection_is_resolved(selection)
        || matches!(
            selection,
            cadmpeg_ir::features::BodySelection::Resolved { bodies, .. } if bodies.is_empty()
        )
}

fn datum_plane_frame_is_resolved(frame: cadmpeg_ir::features::FeatureDatumPlaneFrame) -> bool {
    const EPS_DATUM_PLANE_ORTHOGONAL: f64 = 1.0e-10;

    let (Some(normal), Some(u_axis)) = (
        UnitVector3::normalized(frame.normal().get()),
        UnitVector3::normalized(frame.u_axis().get()),
    ) else {
        return false;
    };
    normal.as_raw().dot(*u_axis.as_raw()).abs() <= EPS_DATUM_PLANE_ORTHOGONAL
}

fn axis_angle_is_resolved(axis_angle: &cadmpeg_ir::features::AxisAngle) -> bool {
    axis_angle.direction.unit().is_some()
}

fn face_motion_is_resolved(motion: &cadmpeg_ir::features::FaceMotion) -> bool {
    use cadmpeg_ir::features::FaceMotion;
    match motion {
        FaceMotion::Offset { .. } => true,
        FaceMotion::Translate { direction, .. } => direction.unit().is_some(),
        FaceMotion::Rotate { axis_dir, .. } => axis_dir.unit().is_some(),
    }
}

fn planar_profile_ref_is_resolved(profile: &PlanarProfileRef) -> bool {
    match profile {
        PlanarProfileRef::Unresolved(_)
        | PlanarProfileRef::Native(_)
        | PlanarProfileRef::SketchSelection { .. } => false,
        PlanarProfileRef::Sketch(_)
        | PlanarProfileRef::Feature(_)
        | PlanarProfileRef::SketchProfiles { .. }
        | PlanarProfileRef::SketchRegions { .. }
        | PlanarProfileRef::SketchEntities { .. }
        | PlanarProfileRef::HistoricalFaces { .. }
        | PlanarProfileRef::Generated { .. } => true,
        PlanarProfileRef::Faces(faces) => !faces.is_empty(),
    }
}

fn profile_ref_is_resolved(profile: &cadmpeg_ir::features::ProfileRef) -> bool {
    match profile {
        ProfileRef::SpatialSketchSelection { .. } => false,
        ProfileRef::SpatialSketchProfiles { .. } => true,
        ProfileRef::Planar(profile) => planar_profile_ref_is_resolved(profile),
    }
}

fn linear_termination_is_resolved(termination: &cadmpeg_ir::features::LinearTermination) -> bool {
    use cadmpeg_ir::features::{LinearTermination, VertexSelection};

    match termination {
        LinearTermination::Unresolved {} => false,
        LinearTermination::ToFace { face, .. }
        | LinearTermination::OffsetFromFace { face, .. }
        | LinearTermination::ToShape { target: face } => face_selection_is_resolved(face),
        LinearTermination::ToVertex { vertex } => matches!(
            vertex,
            VertexSelection::Generated { .. } | VertexSelection::Historical { .. }
        ),
        LinearTermination::Blind { .. }
        | LinearTermination::ThroughAll {}
        | LinearTermination::ThroughNext {}
        | LinearTermination::ToFirst {}
        | LinearTermination::ToLast {}
        | LinearTermination::ToPlane { .. } => true,
    }
}

fn angular_termination_is_resolved(termination: &cadmpeg_ir::features::AngularTermination) -> bool {
    use cadmpeg_ir::features::{AngularTermination, VertexSelection};

    match termination {
        AngularTermination::Unresolved {} => false,
        AngularTermination::ToFace { face, .. }
        | AngularTermination::OffsetFromFace { face, .. }
        | AngularTermination::ToShape { target: face } => face_selection_is_resolved(face),
        AngularTermination::ToVertex { vertex } => matches!(
            vertex,
            VertexSelection::Generated { .. } | VertexSelection::Historical { .. }
        ),
        AngularTermination::Angle { .. }
        | AngularTermination::ThroughAll {}
        | AngularTermination::ThroughNext {}
        | AngularTermination::ToFirst {}
        | AngularTermination::ToLast {} => true,
    }
}

fn loft_path_is_resolved(path: &cadmpeg_ir::features::PathRef) -> bool {
    use cadmpeg_ir::features::PathRef;

    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. } => {
            false
        }
        PathRef::Sketch(_) => true,
        PathRef::SketchCurves { .. } => true,
        PathRef::SpatialSketchCurves { .. } => true,
        PathRef::Edges(edges) => !edges.is_empty(),
        PathRef::Curves(curves) => !curves.is_empty(),
        PathRef::HistoricalEdges { .. } => true,
    }
}

fn feature_definition_is_incomplete(definition: &cadmpeg_ir::features::FeatureDefinition) -> bool {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};

    match definition {
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) => {
            !matches!(kind, NativeFeatureKind::Canvas | NativeFeatureKind::Decal)
        }
        FeatureDefinition::Operation(
            FeatureOperation::ReferenceImage { .. } | FeatureOperation::DatumPrincipalPlane { .. },
        ) => false,
        FeatureDefinition::Operation(FeatureOperation::MeshImport { tessellations }) => {
            tessellations.is_empty()
        }
        FeatureDefinition::Operation(FeatureOperation::Decal { faces, .. }) => {
            !face_selection_is_resolved(faces)
        }
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces, tool, keep, ..
        }) => {
            !face_selection_is_resolved(faces)
                || !loft_path_is_resolved(tool)
                || matches!(keep, cadmpeg_ir::features::TrimRegion::Unresolved)
        }
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face,
            standard,
            designation,
            diameter,
            pitch,
            modeled,
            extent
        }) => {
            !face_selection_is_resolved(face)
                || standard.is_none()
                || designation.is_none()
                || diameter.is_none()
                || pitch.is_none()
                || modeled.is_none()
                || extent.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::Unresolved { .. }) => true,
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => {
            !datum_plane_frame_is_resolved(*frame)
        }
        FeatureDefinition::Operation(FeatureOperation::DatumAxis { direction, .. }) => {
            direction.unit().is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { .. }) => false,
        FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane { frame, points }) => {
            !datum_plane_frame_is_resolved(*frame)
                || !points.iter().all(|point| {
                    matches!(
                        point,
                        cadmpeg_ir::features::VertexSelection::Generated { .. }
                            | cadmpeg_ir::features::VertexSelection::Historical { .. }
                    )
                })
        }
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { reference, .. }) => {
            reference
                .as_ref()
                .is_none_or(|reference| !datum_plane_reference_is_resolved(reference))
        }
        FeatureDefinition::Operation(
            FeatureOperation::Sphere { op, .. } | FeatureOperation::Torus { op, .. },
        ) => *op == cadmpeg_ir::features::BooleanOp::Unresolved,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile,
            start,
            extent,
            ..
        }) => {
            use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeStart};

            let start_is_resolved = match start {
                ExtrudeStart::Unresolved {} => false,
                ExtrudeStart::FromFace { face, .. } => face_selection_is_resolved(face),
                ExtrudeStart::ProfilePlane {}
                | ExtrudeStart::OffsetProfilePlane { .. }
                | ExtrudeStart::FromPlane { .. } => true,
            };
            let extent_is_resolved = match extent {
                ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                    linear_termination_is_resolved(&side.termination)
                }
                ExtrudeExtent::TwoSided { first, second } => {
                    linear_termination_is_resolved(&first.termination)
                        && linear_termination_is_resolved(&second.termination)
                }
            };
            !profile_ref_is_resolved(profile) || !start_is_resolved || !extent_is_resolved
        }
        FeatureDefinition::Operation(FeatureOperation::Revolve { construction, op }) => {
            use cadmpeg_ir::features::{RevolveConstruction, RevolveExtent};

            match construction {
                RevolveConstruction::Unresolved(_) => true,
                RevolveConstruction::Resolved {
                    profile,
                    axis,
                    extent,
                    ..
                } => {
                    let extent_is_resolved = match extent {
                        RevolveExtent::OneSided { termination }
                        | RevolveExtent::Symmetric { termination } => {
                            angular_termination_is_resolved(termination)
                        }
                        RevolveExtent::TwoSided { first, second } => {
                            angular_termination_is_resolved(first)
                                && angular_termination_is_resolved(second)
                        }
                    };
                    !planar_profile_ref_is_resolved(profile)
                        || axis.direction.unit().is_none()
                        || !extent_is_resolved
                        || *op == cadmpeg_ir::features::BooleanOp::Unresolved
                }
            }
        }
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,

            path,

            orientation,
            guide_rail,
            ..
        }) => {
            use cadmpeg_ir::features::{SweepMode, SweepOrientation};

            let mode = shape.mode();

            let sections_are_resolved = !shape.any_section_is_unresolved()
                && shape
                    .referenced_profiles()
                    .all(planar_profile_ref_is_resolved);
            let mode_is_resolved = match mode {
                SweepMode::Unresolved {} => false,
                SweepMode::Solid {
                    op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
                }
                | SweepMode::Solid { .. }
                | SweepMode::Surface {} => true,
            };
            let orientation_is_resolved = match orientation {
                Some(SweepOrientation::Auxiliary { path, .. }) => loft_path_is_resolved(path),
                Some(SweepOrientation::GuideSurface { faces }) => face_selection_is_resolved(faces),
                None
                | Some(
                    SweepOrientation::Binormal { .. }
                    | SweepOrientation::CorrectedFrenet {}
                    | SweepOrientation::Fixed {}
                    | SweepOrientation::Frenet {},
                ) => true,
            };
            let guide_is_resolved = guide_rail
                .as_ref()
                .is_none_or(|guide| loft_path_is_resolved(&guide.path));

            !sections_are_resolved
                || !path.as_ref().is_some_and(loft_path_is_resolved)
                || !mode_is_resolved
                || !orientation_is_resolved
                || !guide_is_resolved
        }
        FeatureDefinition::Operation(FeatureOperation::Hole {
            profile,
            face,
            placements,
            shape,
            extent,
            ..
        }) => {
            use cadmpeg_ir::features::holes::HolePlacement;

            let cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. } = shape.construction()
            else {
                return true;
            };
            let diameter = shape.diameter();

            let support_is_resolved = profile.as_ref().is_some_and(planar_profile_ref_is_resolved)
                || face.as_ref().is_some_and(face_selection_is_resolved);
            let placements_are_resolved = placements.as_ref().is_some_and(|placements| {
                !placements.is_empty()
                    && placements.iter().all(|placement| match placement {
                        HolePlacement::Directed { direction, .. } => direction.unit().is_some(),
                        HolePlacement::Axis { axis, .. } => axis.unit().is_some(),
                    })
            });

            !support_is_resolved
                || !placements_are_resolved
                || kind.is_unresolved()
                || diameter.is_none()
                || extent
                    .as_ref()
                    .is_none_or(|extent| !linear_termination_is_resolved(extent))
        }
        FeatureDefinition::Operation(FeatureOperation::Coil {
            construction,
            result,
        }) => {
            use cadmpeg_ir::features::{CoilPlacement, CoilResult};

            matches!(construction.placement, CoilPlacement::Native { .. })
                || match result {
                    CoilResult::NewBody {} => false,
                    CoilResult::Boolean { targets, .. } => !body_selection_is_resolved(targets),
                }
        }
        // The draft angle remains available when face recipes fail, but replay
        // also requires resolved selections and the material-side convention.
        FeatureDefinition::Operation(FeatureOperation::Draft {
            faces,
            anchor,
            angle,
            outward,
            ..
        }) => {
            angle.is_none()
                || outward.is_none()
                || !face_selection_is_resolved(faces)
                || match anchor {
                    cadmpeg_ir::features::DraftAnchor::PartingLine { tool, pull } => {
                        !face_selection_is_resolved(tool)
                            || pull.direction.unit().is_none()
                            || pull.plane.is_none()
                    }
                    cadmpeg_ir::features::DraftAnchor::NeutralPlane { plane, pull } => {
                        !draft_neutral_plane_is_resolved(
                            plane,
                            pull.as_ref().and_then(|pull| pull.plane.as_ref()),
                            pull.as_ref().map(|pull| &*pull.direction),
                        )
                    }
                }
        }
        FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) => sketch.id().is_none(),
        FeatureDefinition::Operation(FeatureOperation::DatumPoint { construction, .. }) => {
            construction
                .as_deref()
                .is_none_or(|construction| !datum_point_construction_is_resolved(construction))
        }
        FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch }) => {
            sketch.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::SketchBlockDefinition { sketch }) => {
            sketch.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::SketchBlockInstance { block, .. }) => {
            block.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::Form { cages }) => cages.is_empty(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies }) => {
            !base_feature_body_selection_is_resolved(bodies)
        }
        FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies }) => {
            !bodies.is_resolved()
        }
        FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, mode }) => {
            !body_selection_is_resolved(bodies)
                || *mode == cadmpeg_ir::features::BodyRetentionMode::Unresolved
        }
        FeatureDefinition::Operation(FeatureOperation::InsertComponent { .. }) => false,
        FeatureDefinition::Operation(FeatureOperation::AssemblyJoint { .. }) => false,
        FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies,
            removed_faces,
            thickness,
            outward,
            mode,
            ..
        }) => {
            let bodies_are_resolved = bodies.as_ref().is_none_or(body_selection_is_resolved);
            let empty_removed_faces_are_resolved =
                matches!(
                    removed_faces,
                    cadmpeg_ir::features::FaceSelection::Faces(faces) if faces.is_empty()
                ) && bodies.as_ref().is_some_and(body_selection_is_resolved);
            !bodies_are_resolved
                || (!face_selection_is_resolved(removed_faces) && !empty_removed_faces_are_resolved)
                || thickness.is_none()
                || (outward.is_none() && *mode != Some(cadmpeg_ir::features::ShellMode::BothSides))
        }
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces,
            thickness,
            side,
        }) => {
            !face_selection_is_resolved(faces)
                || thickness.is_none()
                || side.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
            faces,
            merge_entities,
            create_solid,
            gap_tolerance,
        }) => {
            !face_selection_is_resolved(faces)
                || merge_entities.is_none()
                || create_solid.is_none()
                || !gap_tolerance.is_some_and(|tolerance| tolerance.get() > 0.0)
        }
        FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions,
            placement,
            op,
        }) => {
            dimensions.is_none()
                || placement.is_none()
                || *op == cadmpeg_ir::features::BooleanOp::Unresolved
        }
        FeatureDefinition::Operation(FeatureOperation::Primitive { op, .. }) => {
            *op == cadmpeg_ir::features::BooleanOp::Unresolved
        }
        FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, motion }) => {
            !face_selection_is_resolved(faces) || !face_motion_is_resolved(motion)
        }
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies, rotation, ..
        }) => {
            !body_selection_is_resolved(bodies)
                || rotation
                    .as_ref()
                    .is_some_and(|rotation| !axis_angle_is_resolved(rotation))
        }
        FeatureDefinition::Operation(FeatureOperation::Scale {
            bodies,
            center,
            factors,
        }) => {
            !body_selection_is_resolved(bodies)
                || center.is_none()
                || center.as_ref().is_some_and(|center| {
                    matches!(center, cadmpeg_ir::features::ScaleCenter::Native(_))
                })
                || factors.resolved().is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) => {
            seeds.is_empty() || pattern.is_unresolved()
        }
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
            groups.is_empty()
                || groups.iter().any(|group| {
                    !edge_selection_is_resolved(&group.edges) || group.spec.is_unresolved()
                })
        }
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
            groups.is_empty()
                || groups
                    .iter()
                    .any(|group| !edge_selection_is_resolved(&group.edges))
        }
        FeatureDefinition::Operation(FeatureOperation::DeleteFace { faces, .. }) => {
            !face_selection_is_resolved(faces)
        }
        FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }) => {
            let targets = operands.targets();
            let replacements = operands.replacements();
            !face_selection_is_resolved(targets) || !face_selection_is_resolved(replacements)
        }
        FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, tool }) => {
            !body_selection_is_resolved(targets)
                || match tool {
                    cadmpeg_ir::features::SplitBodyTool::Faces(faces) => {
                        !face_selection_is_resolved(faces)
                    }
                    cadmpeg_ir::features::SplitBodyTool::Plane { .. } => false,
                }
        }
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces, distance, ..
        }) => !face_selection_is_resolved(faces) || distance.is_none(),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            profile, ..
        }) => !planar_profile_ref_is_resolved(profile),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange {
            edges,
            height,
            ..
        }) => {
            !edge_selection_is_resolved(edges)
                || matches!(
                    height,
                    cadmpeg_ir::features::SheetMetalFlangeHeight::ToObject {
                        target: cadmpeg_ir::features::SheetMetalFlangeHeightTarget::Native(_),
                        ..
                    }
                )
        }
        FeatureDefinition::Operation(FeatureOperation::SheetMetalHem {
            edges,
            form,
            direction,
            ..
        }) => {
            !edge_selection_is_resolved(edges)
                || matches!(
                    direction,
                    cadmpeg_ir::features::SheetMetalHemDirection::Unresolved
                )
                || matches!(
                    form,
                    cadmpeg_ir::features::SheetMetalHemForm::GapLength { .. }
                )
        }
        FeatureDefinition::Operation(FeatureOperation::SplitFace { targets, tool }) => {
            !face_selection_is_resolved(targets)
                || match tool {
                    cadmpeg_ir::features::SplitFaceTool::Plane { .. }
                    | cadmpeg_ir::features::SplitFaceTool::Planes { .. } => false,
                    cadmpeg_ir::features::SplitFaceTool::Path(path) => matches!(
                        path,
                        cadmpeg_ir::features::PathRef::Native(_)
                            | cadmpeg_ir::features::PathRef::Unresolved(_)
                    ),
                }
        }
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections, guidance, ..
        }) => {
            let guidance_incomplete = match guidance {
                cadmpeg_ir::features::LoftGuidance::Guides(paths) => {
                    paths.iter().any(|path| !loft_path_is_resolved(path))
                }
                cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                    !loft_path_is_resolved(path)
                }
            };
            sections.len() < 2
                || sections.iter().any(|section| match section {
                    cadmpeg_ir::features::LoftSection::Profile(profile) => {
                        !profile_ref_is_resolved(profile)
                    }
                    cadmpeg_ir::features::LoftSection::Point(
                        cadmpeg_ir::features::LoftPointSection::Native(_),
                    ) => true,
                    cadmpeg_ir::features::LoftSection::Point(
                        cadmpeg_ir::features::LoftPointSection::Point(_)
                        | cadmpeg_ir::features::LoftPointSection::Vertex(_),
                    ) => false,
                })
                || guidance_incomplete
        }
        FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary,
            support_faces,
            continuity,
            merge_result,
        }) => {
            use cadmpeg_ir::features::SurfaceBoundary;

            let boundary_is_resolved = match boundary {
                SurfaceBoundary::Edges(edges) => edge_selection_is_resolved(edges),
                SurfaceBoundary::Path(path) => loft_path_is_resolved(path),
            };
            let continuity_is_resolved = !continuity.is_unresolved();
            let needs_support = |continuity| {
                matches!(
                    continuity,
                    cadmpeg_ir::features::SurfaceContinuity::Tangent
                        | cadmpeg_ir::features::SurfaceContinuity::Curvature
                )
            };
            let support_is_required = continuity
                .resolved()
                .is_some_and(|continuity| {
                    continuity
                        .conditions
                        .iter()
                        .map(|condition| condition.continuity())
                        .any(needs_support)
                });

            !boundary_is_resolved
                || !continuity_is_resolved
                || (support_is_required && !face_selection_is_resolved(support_faces))
                || merge_result.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::FullRoundFillet { groups }) => {
            groups.is_empty()
                || groups.iter().any(|group| {
                    !face_selection_is_resolved(group.center_faces())
                        || matches!(
                            group.side_one_faces(),
                            cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Unresolved
                        )
                        || matches!(
                            group.side_two_faces(),
                            cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Unresolved
                        )
                        || matches!(
                            group.side_one_faces(),
                            cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(ref selection)
                                if !face_selection_is_resolved(selection)
                        )
                        || matches!(
                            group.side_two_faces(),
                            cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(ref selection)
                                if !face_selection_is_resolved(selection)
                        )
                })
        }
        FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) => {
            let target = operands.target();
            let tools = operands.tools();
            !body_selection_is_resolved(target) || !body_selection_is_resolved(tools)
        }
        // A typed family is not replayable until this match states and checks
        // its complete construction invariants.
        _ => true,
    }
}

fn incomplete_feature_families(ir: &CadIr) -> std::collections::BTreeMap<&str, usize> {
    let mut families = std::collections::BTreeMap::new();
    for feature in &ir.model.features {
        if !feature_definition_is_incomplete(feature.evaluation.definition()) {
            continue;
        }
        let family = feature.source_tag.as_deref().unwrap_or_else(|| {
            if let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native { kind, .. },
            ) = feature.evaluation.definition()
            {
                kind.as_str()
            } else {
                "<missing source tag>"
            }
        });
        *families.entry(family).or_default() += 1;
    }
    families
}

fn design_projection_gaps(
    ir: &CadIr,
    native: &F3dNative,
) -> Result<DesignProjectionGaps, CodecError> {
    use cadmpeg_ir::features::{
        BodySelection, EdgeSelection, ExtrudeExtent, ExtrudeStart, FaceSelection, LinearTermination,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, NativeFeatureKind, PathRef, PlanarProfileRef,
        ProfileRef,
    };
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;
    use std::collections::HashSet;

    let source_lost_edge_reference_ids = native
        .lost_edge_references
        .iter()
        .map(|reference| reference.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut complete_edge_selection_native_ids = HashSet::<String>::new();
    let projected_constraint_refs = ir
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketch_constraints
                .iter()
                .filter_map(|constraint| constraint.native_ref.as_deref()),
        )
        .collect::<std::collections::HashSet<_>>();
    let projected_sketch_refs = ir
        .model
        .sketches
        .iter()
        .filter_map(|sketch| sketch.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketches
                .iter()
                .filter_map(|sketch| sketch.native_ref.as_deref()),
        )
        .collect::<std::collections::HashSet<_>>();
    let projected_sketch_entity_refs = ir
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| entity.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .collect::<std::collections::HashSet<_>>();
    let projected_feature_refs = ir
        .model
        .features
        .iter()
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let projected_parameter_refs = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| parameter.native_ref.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let projected_features = ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut unprojected_history_dependencies = 0;
    let mut ambiguous_history_dependencies = 0;
    let scope_history = crate::design::feature_project::ScopeHistoryGraph::new(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.design_component_naming_spaces,
        &native.asm_histories,
    );
    for scope in &native.design_parameter_scopes {
        let Some(feature) = projected_features.get(scope.id.as_str()) else {
            continue;
        };
        let predecessor_scope = match scope_history.predecessor(scope, |candidate| {
            projected_features.contains_key(candidate.id.as_str())
        }) {
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Scope(predecessor)) => {
                predecessor
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Ambiguous) | Err(_) => {
                ambiguous_history_dependencies += 1;
                continue;
            }
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::None) => {
                continue;
            }
        };
        let Some(predecessor) = projected_features.get(predecessor_scope.id.as_str()) else {
            unprojected_history_dependencies += 1;
            continue;
        };
        if predecessor.id != feature.id && !feature.dependencies.contains(&predecessor.id) {
            unprojected_history_dependencies += 1;
        }
    }
    let projected_dimension_parameters = ir.model
            .sketch_constraints
            .iter()
            .flat_map(|constraint| {
                crate::design::dimensions::constraint_parameters(constraint.definition.kind())
            })
            .chain(
                ir.model.spatial_sketch_constraints.iter().filter_map(
                    |constraint| match constraint.definition.kind() {
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Native {
                            parameter,
                            ..
                        } => parameter.as_ref(),
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::PointDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::PointLineDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::ParallelLineDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::RepeatedParallelLineDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::LineLength {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::RepeatedLineLength {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::ParallelLineSetDistance {
                            parameter,
                            ..
                        } => Some(parameter),
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Offset {
                            parameter,
                            ..
                        } => parameter.as_ref().map(|parameter| &parameter.id),
                        _ => None,
                    },
                ),
            ).collect::<std::collections::HashSet<_>>();

    let native_sketch_relation_ids = native
        .sketch_relations
        .iter()
        .map(|relation| relation.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut native_sketch_relations = 0;
    let mut native_dimensions = 0;
    for constraint in &ir.model.sketch_constraints {
        if !matches!(
            constraint.definition.kind(),
            SketchConstraintDefinitionInput::Native { .. }
        ) {
            continue;
        }
        if constraint
            .native_ref
            .as_deref()
            .is_some_and(|native_ref| native_sketch_relation_ids.contains(native_ref))
        {
            native_sketch_relations += 1;
        } else {
            native_dimensions += 1;
        }
    }
    for constraint in &ir.model.spatial_sketch_constraints {
        if !matches!(
            constraint.definition.kind(),
            cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Native { .. }
        ) {
            continue;
        }
        if constraint
            .native_ref
            .as_deref()
            .is_some_and(|native_ref| native_sketch_relation_ids.contains(native_ref))
        {
            native_sketch_relations += 1;
        } else {
            native_dimensions += 1;
        }
    }

    let authored_scopes = match crate::design::feature_project::authored_scope_ordinals_per_stream(
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
    ) {
        Ok(scopes) => Some(scopes),
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => None,
    };
    let mut gaps = DesignProjectionGaps {
        unresolved_body_bindings: native
            .design_body_bindings
            .iter()
            .filter(|binding| binding.body.is_none() && !binding.history_only)
            .count(),
        history_only_body_bindings: native
            .design_body_bindings
            .iter()
            .filter(|binding| binding.history_only)
            .count(),
        unprojected_history_dependencies,
        ambiguous_history_dependencies,
        unprojected_feature_scopes: native
            .design_parameter_scopes
            .iter()
            .filter(|scope| {
                let authored = authored_scopes.as_ref().map_or_else(
                    || {
                        scope
                            .assembly_alignment()
                            .and_then(super::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index)
                            .is_none()
                    },
                    |ordinals| {
                        let stream = crate::ids::native_stream(&scope.id)
                            .unwrap_or(crate::ids::DEFAULT_STREAM);
                        ordinals.contains_key(&(stream, scope.record_index))
                    },
                );
                authored && !projected_feature_refs.contains(scope.id.as_str())
            })
            .count(),
        unprojected_parameters: native
            .design_parameters
            .iter()
            .filter(|parameter| !projected_parameter_refs.contains(parameter.id.as_str()))
            .count(),
        unresolved_parameter_owners: native
            .design_parameters
            .iter()
            .filter(|parameter| {
                let Some(owner_record_index) = parameter.owner_record_index() else {
                    return false;
                };
                let Some(stream) = crate::ids::native_stream(&parameter.id) else {
                    return true;
                };
                !native.design_parameter_owners.iter().any(|owner| {
                    crate::ids::native_stream(owner.id()) == Some(stream)
                        && owner.record_index() == owner_record_index
                        && native.design_parameter_scopes.iter().any(|scope| {
                            crate::ids::native_stream(&scope.id) == Some(stream)
                                && scope.record_index == owner.scope_record_index()
                        })
                })
            })
            .count(),
        untyped_parameter_units: crate::design::feature_project::untyped_parameter_unit_count(
            &native.design_parameters,
        ),
        unresolved_expression_dependencies:
            crate::design::dimensions::unresolved_parameter_expression_dependency_count(&native.design_parameters, &ir.model.parameters),
        native_sketch_relations,
        native_dimensions,
        unprojected_sketch_placements: native
            .design_sketch_placements
            .iter()
            .filter(|placement| !projected_sketch_refs.contains(placement.id.as_str()))
            .count(),
        unprojected_sketch_points: native
            .sketch_points
            .iter()
            .filter(|point| {
                point.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(point.id.as_str())
            })
            .count(),
        unprojected_sketch_curves: native
            .sketch_curve_identities
            .iter()
            .filter(|curve| {
                curve.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(curve.id.as_str())
            })
            .count(),
        unprojected_sketch_surfaces: native
            .sketch_surfaces
            .iter()
            .filter(|surface| {
                surface.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(surface.id.as_str())
            })
            .count(),
        unprojected_sketch_texts: native
            .sketch_texts
            .iter()
            .filter(|text| {
                !projected_sketch_entity_refs.contains(text.id.as_str())
                    && crate::design::decode::sketch::sketch_text_neutral_state(text) != Some(false)
            })
            .count(),
        unprojected_sketch_text_styles: native
            .sketch_texts
            .iter()
            .filter(|text| {
                !projected_sketch_entity_refs.contains(text.id.as_str())
                    && crate::design::decode::sketch::sketch_text_neutral_state(text) == Some(false)
            })
            .count(),
        unprojected_sketch_relations: native
            .sketch_relations
            .iter()
            .filter(|relation| {
                !projected_constraint_refs.contains(relation.id.as_str())
                    && crate::design::decode::sketch::sketch_pattern_neutral_state(relation) != Some(false)
            })
            .count(),
        unprojected_sketch_pattern_states: native.sketch_relations.iter().filter(|relation| {
            !projected_constraint_refs.contains(relation.id.as_str())
                && crate::design::decode::sketch::sketch_pattern_neutral_state(relation) == Some(false)
        }).count(),
        unprojected_dimensions: {
            let container_only = container_only_dimension_parameters(native);
            let relation_bearing_companions = native.design_parameter_companions
                .iter()
                .filter(|companion| {
                    companion
                        .payload()
                        .is_some_and(|payload| payload.byte_length() > 0)
                })
                .filter_map(|companion| {
                    Some((
                        crate::ids::native_stream(companion.id())?,
                        companion.record_index(),
                    ))
                })
                .chain(
                    native
                        .design_dimension_locus_pairs
                        .iter()
                        .filter_map(|pair| {
                            Some((
                                crate::ids::native_stream(&pair.id)?,
                                pair.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    native
                        .design_dimension_null_locus_pairs
                        .iter()
                        .filter_map(|pair| {
                            Some((
                                crate::ids::native_stream(&pair.id)?,
                                pair.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    native
                        .design_dimension_annotation_frames
                        .iter()
                        .filter_map(|frame| {
                            Some((
                                crate::ids::native_stream(&frame.id)?,
                                frame.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    native
                        .design_dimension_locus_groups
                        .iter()
                        .filter_map(|group| {
                            Some((
                                crate::ids::native_stream(&group.id)?,
                                group.companion_record_index,
                            ))
                        }),
                )
                .chain(
                    native
                        .design_dimension_recipe_records
                        .iter()
                        .filter_map(|record| {
                            Some((
                                crate::ids::native_stream(&record.id)?,
                                record.companion_record_index,
                            ))
                        }),
                ).collect::<std::collections::HashSet<_>>();
            let relation_bearing_parameters = native.design_parameter_owners.iter().filter_map(|owner| {
                    let stream = crate::ids::native_stream(owner.id())?;
                    relation_bearing_companions
                        .contains(&(stream, owner.companion_record_index()))
                        .then_some((stream, owner.parameter_record_index()))
                }).collect::<std::collections::HashSet<_>>();
            native
                .design_parameters
                .iter()
                .try_fold(0usize, |count, parameter| {
                    let stream = crate::ids::native_stream(&parameter.id)
                        .unwrap_or(crate::ids::DEFAULT_STREAM);
                    if parameter.kind() != crate::records::parameters::DesignParameterKind::Dimension
                        || !relation_bearing_parameters.contains(&(stream, parameter.record_index))
                    {
                        return Ok(count);
                    }
                    let id = crate::ids::neutral_parameter_id(parameter);
                    Ok::<_, CodecError>(count + usize::from(
                        !projected_dimension_parameters.contains(&id)
                            && !container_only.contains(&id),
                    ))
                })
                ?
        },
        active_face_substitutions: native
            .design_face_operands
            .iter()
            .filter(|operand| operand.resolved_active_face.is_some())
            .count(),
        ..DesignProjectionGaps::default()
    };
    let mut edge_selection = |selection: &EdgeSelection| match selection {
        EdgeSelection::Native(_) => gaps.native_edge_selections += 1,
        EdgeSelection::Unresolved => gaps.unresolved_edge_selections += 1,
        EdgeSelection::HistoricalPartial { unresolved, .. } => {
            gaps.partially_resolved_edge_members += unresolved
                .iter()
                .filter(|id| !source_lost_edge_reference_ids.contains(id.as_str()))
                .count();
        }
        EdgeSelection::Resolved { native, .. } => {
            complete_edge_selection_native_ids.insert(native.as_str().to_owned());
        }
        EdgeSelection::Generated { native, .. } => {
            complete_edge_selection_native_ids.insert(native.as_str().to_owned());
        }
        EdgeSelection::Historical { native, .. } => {
            complete_edge_selection_native_ids.insert(native.as_str().to_owned());
        }
        EdgeSelection::All | EdgeSelection::Edges(_) => {}
    };
    let mut face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Native(_) | FaceSelection::Unresolved => gaps.face_selections += 1,
        FaceSelection::HistoricalPartial { unresolved, .. } => {
            gaps.partially_resolved_face_members += unresolved.len();
        }
        FaceSelection::Faces(_)
        | FaceSelection::Resolved { .. }
        | FaceSelection::Generated { .. }
        | FaceSelection::Historical { .. } => {}
    };
    let native_body_selection_count = |selection: &BodySelection| match selection {
        BodySelection::Native(_) | BodySelection::Unresolved => 1,
        BodySelection::NativeSet(members) => members.len(),
        BodySelection::Bodies(_)
        | BodySelection::Resolved { .. }
        | BodySelection::ResolvedSet { .. }
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. } => 0,
    };
    for feature in &ir.model.features {
        gaps.incomplete_features += usize::from(feature_definition_is_incomplete(
            feature.evaluation.definition(),
        ));
        gaps.native_reference_images += usize::from(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: NativeFeatureKind::Canvas,
                ..
            })
        ));
        gaps.native_decals += usize::from(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: NativeFeatureKind::Decal,
                ..
            })
        ));
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies }) => {
                gaps.body_selections += native_body_selection_count(bodies);
            }
            FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies }) => {
                gaps.body_selections += usize::from(!bodies.is_resolved());
            }
            FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) => {
                let target = operands.target();
                let tools = operands.tools();
                gaps.body_selections +=
                    native_body_selection_count(target) + native_body_selection_count(tools);
            }
            FeatureDefinition::Operation(FeatureOperation::Coil {
                result: cadmpeg_ir::features::CoilResult::Boolean { targets, .. },
                ..
            }) => gaps.body_selections += native_body_selection_count(targets),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile,
                start,
                extent,
                ..
            }) => {
                if matches!(
                    profile,
                    ProfileRef::Planar(
                        PlanarProfileRef::Native(_) | PlanarProfileRef::SketchSelection { .. }
                    )
                ) {
                    gaps.profile_selections += 1;
                }
                if let ExtrudeStart::FromFace { face, .. } = start {
                    face_selection(face);
                }
                let (first, second) = match extent {
                    ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                        (side, None)
                    }
                    ExtrudeExtent::TwoSided { first, second } => (first, Some(second)),
                };
                for side in [Some(first), second].into_iter().flatten() {
                    if let LinearTermination::ToFace { face, .. } = &side.termination {
                        face_selection(face);
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                for group in groups {
                    edge_selection(&group.edges);
                }
            }
            FeatureDefinition::Operation(FeatureOperation::FullRoundFillet { groups }) => {
                for group in groups {
                    face_selection(group.center_faces());
                    for side in [group.side_one_faces(), group.side_two_faces()] {
                        if let cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(selection) =
                            side
                        {
                            face_selection(selection);
                        }
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
                for group in groups {
                    edge_selection(&group.edges);
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,

                path,
                guide_rail,
                ..
            }) => {
                if shape.any_section_names_an_unresolved_carrier() {
                    gaps.profile_selections += 1;
                }
                for profile in shape.referenced_profiles() {
                    if matches!(
                        profile,
                        PlanarProfileRef::Native(_)
                            | PlanarProfileRef::Unresolved(_)
                            | PlanarProfileRef::SketchSelection { .. }
                    ) {
                        gaps.profile_selections += 1;
                    }
                }
                if path.as_ref().is_some_and(|path| {
                    matches!(
                        path,
                        PathRef::Native(_)
                            | PathRef::Unresolved(_)
                            | PathRef::SpatialSketchSelection { .. }
                    )
                }) {
                    gaps.path_selections += 1;
                }
                if guide_rail.as_ref().is_some_and(|guide| {
                    matches!(
                        &guide.path,
                        PathRef::Native(_)
                            | PathRef::Unresolved(_)
                            | PathRef::SpatialSketchSelection { .. }
                    )
                }) {
                    gaps.path_selections += 1;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                construction: Some(construction),
                ..
            }) => {
                use cadmpeg_ir::features::{DatumPlaneReference, DatumPointConstruction};

                let mut plane = |reference: &DatumPlaneReference| {
                    if let DatumPlaneReference::Face { face } = reference {
                        face_selection(face);
                    }
                };
                match construction.as_ref() {
                    DatumPointConstruction::CircleCenter { edge }
                    | DatumPointConstruction::DistanceOnEdge { edge, .. } => edge_selection(edge),
                    DatumPointConstruction::TwoEdgeIntersection { edges } => {
                        for edge in edges {
                            edge_selection(edge);
                        }
                    }
                    DatumPointConstruction::ThreePlaneIntersection { planes } => {
                        planes.iter().for_each(&mut plane);
                    }
                    DatumPointConstruction::Vertex { .. }
                    | DatumPointConstruction::SketchPoint { .. } => {}
                    DatumPointConstruction::EdgePlaneIntersection {
                        edge,
                        plane: reference,
                    } => {
                        edge_selection(edge);
                        plane(reference);
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                boundary,
                support_faces,
                ..
            }) => {
                match boundary {
                    cadmpeg_ir::features::SurfaceBoundary::Edges(edges) => edge_selection(edges),
                    cadmpeg_ir::features::SurfaceBoundary::Path(path) => {
                        gaps.path_selections += usize::from(!loft_path_is_resolved(path));
                    }
                }
                face_selection(support_faces);
            }
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections, guidance, ..
            }) => {
                gaps.profile_selections += sections
                    .iter()
                    .filter(|section| {
                        matches!(
                            section,
                            cadmpeg_ir::features::LoftSection::Profile(profile)
                                if !profile_ref_is_resolved(profile)
                        )
                    })
                    .count();
                gaps.path_selections += match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(paths) => paths
                        .iter()
                        .filter(|path| !loft_path_is_resolved(path))
                        .count(),
                    cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                        usize::from(!loft_path_is_resolved(path))
                    }
                };
            }
            FeatureDefinition::Operation(FeatureOperation::Shell {
                bodies,
                removed_faces,
                ..
            }) => {
                if let Some(bodies) = bodies {
                    gaps.body_selections += native_body_selection_count(bodies);
                }
                face_selection(removed_faces);
            }
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread { face, .. }) => {
                face_selection(face);
            }
            FeatureDefinition::Operation(FeatureOperation::Decal { faces, .. }) => {
                face_selection(faces);
            }
            FeatureDefinition::Operation(
                FeatureOperation::DeleteFace { faces, .. }
                | FeatureOperation::OffsetSurface { faces, .. },
            ) => face_selection(faces),
            FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }) => {
                let targets = operands.targets();
                let replacements = operands.replacements();
                face_selection(targets);
                face_selection(replacements);
            }
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
                profile,
                ..
            }) => {
                gaps.profile_selections += usize::from(!planar_profile_ref_is_resolved(profile));
            }
            FeatureDefinition::Operation(
                FeatureOperation::SheetMetalEdgeFlange { edges, .. }
                | FeatureOperation::SheetMetalHem { edges, .. },
            ) => edge_selection(edges),
            FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, .. }) => {
                face_selection(faces);
            }
            _ => {}
        }
    }
    let repaired_lost_edge_reference_ids = native
        .design_construction_operand_groups
        .iter()
        .filter(|group| complete_edge_selection_native_ids.contains(group.id.as_str()))
        .flat_map(|group| group.lost_edge_references.iter().map(String::as_str))
        .collect::<std::collections::HashSet<_>>();
    gaps.unrepaired_lost_edge_references = native
        .lost_edge_references
        .iter()
        .filter(|reference| !repaired_lost_edge_reference_ids.contains(reference.id.as_str()))
        .count();
    Ok(gaps)
}

struct IncompleteFamilyCounts<'a>(&'a std::collections::BTreeMap<&'a str, usize>);

impl std::fmt::Display for IncompleteFamilyCounts<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (family, count)) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{family}={count}")?;
        }
        Ok(())
    }
}

fn push_loss_vec(
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    code: F3dLossCode,
    args: std::fmt::Arguments<'_>,
) {
    losses.push(code.note(args.to_string()));
}

fn report_unparsed_parameter_records(
    report: &mut DecodeBody,
    unparsed: &[crate::design::decode::parameters::UnparsedParameterRecords],
) {
    for records in unparsed {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::ParameterRecordUnparsed,
            format_args!(
                "{} parameter frame(s) in {} match no parameter grammar and were not decoded.",
                records.count, records.stream
            ),
        );
    }
}

fn report_unparsed_parameter_owners(report: &mut DecodeBody, count: usize) {
    if count != 0 {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::ParameterOwnerUnparsed,
            format_args!(
                "{count} Design parameter owner binding(s) match no owner grammar; their parameters remain native without interpreted owner bindings."
            ),
        );
    }
}

fn report_design_projection_gaps(
    report: &mut DecodeBody,
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let projected_moves: std::collections::HashSet<&str> = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::MoveBody { .. }
                )
            )
        })
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect();
    for scope in &native.design_parameter_scopes {
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Move
            && !projected_moves.contains(scope.id.as_str())
        {
            let reason = match scope.move_operation() {
                None => "no unique admitted rigid-transform record",
                Some(operation) if !operation.preserves_orientation() => {
                    "stored basis reverses orientation"
                }
                Some(_) => "no neutral MoveBody feature for its body selection",
            };
            report.losses.push(
                F3dLossCode::MoveUnprojected
                    .note(format!("Move {} remains native: {reason}.", scope.id)),
            );
        }
    }
    let gaps = design_projection_gaps(ir, native)?;
    report.losses.extend(crate::history::stored_bodies::losses(
        &ir.model.features,
        &native.design_parameter_scopes,
    ));
    let incomplete_families = incomplete_feature_families(ir);
    let history_budget_skips = native
        .asm_histories
        .iter()
        .filter(|history| history.binding_budget_exceeded)
        .count();
    if history_budget_skips != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::HistoryBindingBudgetExceeded, format_args!(
            "{history_budget_skips} ASM history stream(s) retain only their state headers, with no bulletin boards, history records or historical topology, because their estimated retained size exceeded the decode session's remaining retained-byte allowance."
        ),);
    }
    let recipe_search_skips = crate::design::decode::operands::recipe_side_searches_exhausted(
        ctx,
        &native.design_edge_operands,
        &native.design_face_operands,
    )?;
    for cap in crate::records::feature::patterns::RectangularPatternSearchCap::ALL {
        let capped_pattern_searches = native
            .design_parameter_scopes
            .iter()
            .filter_map(
                crate::records::feature::scope::DesignParameterScope::rectangular_pattern_construction,
            )
            .filter(|construction| construction.instance_search_cap() == Some(cap))
            .count();
        if capped_pattern_searches != 0 {
            push_loss_vec(&mut report.losses, F3dLossCode::FeaturePatternInstanceSearchCapped, format_args!(
                "{capped_pattern_searches} rectangular pattern(s) have no instance transforms or direction because their instance search exceeded the local cap {} ({}).",
                cap.name(),
                cap.limit(),
            ),);
        }
    }
    if recipe_search_skips != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::RecipeSideSearchBudgetExceeded, format_args!(
            "{recipe_search_skips} Design recipe program(s) retain no recipe structure because their side-sequence search exceeded the local work cap RECIPE_SIDE_SEARCH_WORK ({}).",
            crate::design::decode::operands::RECIPE_SIDE_SEARCH_WORK,
        ),);
    }
    for error in native
        .asm_histories
        .iter()
        .flat_map(crate::history_records::AsmHistory::states)
        .flat_map(|state| &state.records)
        .filter_map(|record| record.framing_error())
    {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::HistoryRecordFramingFailed,
            format_args!(
                "An ASM history span remains opaque because record framing failed: {error}."
            ),
        );
    }
    if gaps.unresolved_body_bindings != 0 {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::DesignBodyBindingUnresolved,
            format_args!(
                "{} Design body-map pair(s) do not resolve to a body in the named BREP blob.",
                gaps.unresolved_body_bindings
            ),
        );
    }
    if gaps.history_only_body_bindings != 0 {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::HistoryOnlyBodyOmitted,
            format_args!(
                "{} Design body-map pair(s) name a body outside the current model: every Body container omits its entity and its browser node is hidden. The body stays in the named BREP blob and in the native body map, and does not reach `model.bodies`.",
                gaps.history_only_body_bindings
            ),
        );
    }
    if gaps.native_reference_images != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::ReferenceImageNativeRetained, format_args!(
            "{} reference-image timeline object(s) retain native Canvas records because no neutral image-plane binding was resolved.",
            gaps.native_reference_images
        ),);
    }
    if gaps.native_decals != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::DecalNativeRetained, format_args!(
            "{} decal timeline object(s) retain native image and mapping records because no neutral decal binding was resolved.",
            gaps.native_decals
        ),);
    }
    if gaps.unrepaired_lost_edge_references != 0 {
        push_loss_vec(&mut report.losses, F3dLossCode::EdgeReferenceLostUnrepaired, format_args!(
            "{} source parametric edge reference(s) were marked EDGE_REFERENCE_LOST and have no independent complete selection proof.",
            gaps.unrepaired_lost_edge_references
        ),);
    }
    for scope in &native.design_parameter_scopes {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Emboss {
            continue;
        }
        let (code, reason) = if scope.emboss_construction().is_some() {
            (F3dLossCode::EmbossControlsNative,
             "control words, native pairs, scalar-owner links, selection links, contour membership, and placement have no neutral Wrap mapping")
        } else {
            (
                F3dLossCode::EmbossPrefixUnparsed,
                "the operation prefix does not match an admitted native Emboss form",
            )
        };
        report
            .losses
            .push(code.note(format!("{}: {reason}.", scope.id)));
    }
    let mut push = |code: F3dLossCode,
                    count: usize,
                    message: std::fmt::Arguments<'_>|
     -> Result<(), CodecError> {
        if count != 0 {
            push_loss_vec(&mut report.losses, code, message);
        }
        Ok(())
    };
    push(
        F3dLossCode::FeatureDefinitionIncomplete,
        gaps.incomplete_features,
        format_args!(
            "{} feature scope(s) have no complete neutral feature definition: {}.",
            gaps.incomplete_features,
            IncompleteFamilyCounts(&incomplete_families)
        ),
    )?;
    let mut uninterpreted_scopes = 0;
    for scope in &native.design_parameter_scopes {
        if scope.occurrence_transfer.is_none() && scope.payload().is_uninterpreted() {
            uninterpreted_scopes += 1;
        }
    }
    push(F3dLossCode::FeatureScopePayloadUninterpreted, uninterpreted_scopes,
        format_args!("{uninterpreted_scopes} native feature scope(s) retain their source envelope without interpreted specialized operation data."))?;
    push(
        F3dLossCode::FeatureScopeUnprojected,
        gaps.unprojected_feature_scopes,
        format_args!(
            "{} decoded feature scope(s) have no neutral construction-history feature.",
            gaps.unprojected_feature_scopes
        ),
    )?;
    push(
        F3dLossCode::ParameterUnprojected,
        gaps.unprojected_parameters,
        format_args!(
            "{} decoded Design parameter(s) have no neutral parameter.",
            gaps.unprojected_parameters
        ),
    )?;
    push(
        F3dLossCode::ParameterOwnerUnrecognized,
        gaps.unresolved_parameter_owners,
        format_args!(
            "{} decoded Design parameter owner binding(s) have no recognized feature scope.",
            gaps.unresolved_parameter_owners
        ),
    )?;
    push(
        F3dLossCode::ParameterUnitUntyped,
        gaps.untyped_parameter_units,
        format_args!(
            "{} decoded Design parameter(s) retain unit tokens without a settled neutral quantity kind.",
            gaps.untyped_parameter_units
        ),
    )?;
    push(
        F3dLossCode::ParameterExpressionUnbound,
        gaps.unresolved_expression_dependencies,
        format_args!(
            "{} decoded parameter expression symbol(s) name same-stream parameters without a neutral dependency edge.",
            gaps.unresolved_expression_dependencies
        ),
    )?;
    push(
        F3dLossCode::HistoryDependencyUnprojected,
        gaps.unprojected_history_dependencies,
        format_args!(
            "{} feature history-state dependency link(s) were not projected into neutral construction history.",
            gaps.unprojected_history_dependencies
        ),
    )?;
    push(
        F3dLossCode::HistoryDependencyAmbiguous,
        gaps.ambiguous_history_dependencies,
        format_args!(
            "{} feature history-state dependency link(s) have multiple source scopes for the preceding state identity.",
            gaps.ambiguous_history_dependencies
        ),
    )?;
    push(
        F3dLossCode::SketchRelationNativeRetained,
        gaps.native_sketch_relations,
        format_args!(
            "{} sketch relation(s) retain native operands because no unique neutral relation was resolved.",
            gaps.native_sketch_relations
        ),
    )?;
    push(
        F3dLossCode::SketchDimensionNativeRetained,
        gaps.native_dimensions,
        format_args!(
            "{} sketch dimension(s) retain native operands because no unique neutral dimension was resolved.",
            gaps.native_dimensions
        ),
    )?;
    push(
        F3dLossCode::SketchPlacementUnprojected,
        gaps.unprojected_sketch_placements,
        format_args!(
            "{} decoded Sketch placement(s) have no neutral sketch.",
            gaps.unprojected_sketch_placements
        ),
    )?;
    push(
        F3dLossCode::SketchPointUnprojected,
        gaps.unprojected_sketch_points,
        format_args!(
            "{} decoded sketch point(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_points
        ),
    )?;
    push(
        F3dLossCode::SketchCurveUnprojected,
        gaps.unprojected_sketch_curves,
        format_args!(
            "{} decoded sketch curve(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_curves
        ),
    )?;
    push(
        F3dLossCode::SketchSurfaceUnprojected,
        gaps.unprojected_sketch_surfaces,
        format_args!(
            "{} decoded sketch surface(s) have no neutral spatial sketch entity.",
            gaps.unprojected_sketch_surfaces
        ),
    )?;
    push(
        F3dLossCode::SketchTextUnprojected,
        gaps.unprojected_sketch_texts,
        format_args!(
            "{} decoded sketch text record(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_texts
        ),
    )?;
    push(
        F3dLossCode::SketchTextStyleUnprojected,
        gaps.unprojected_sketch_text_styles,
        format_args!(
            "{} decoded sketch text record(s) retain unsupported style or orientation state.",
            gaps.unprojected_sketch_text_styles
        ),
    )?;
    push(
        F3dLossCode::SketchPatternStateUnprojected,
        gaps.unprojected_sketch_pattern_states,
        format_args!(
            "{} sketch pattern(s) retain unsupported symmetry, suppression or flag state.",
            gaps.unprojected_sketch_pattern_states
        ),
    )?;
    push(
        F3dLossCode::SketchRelationUnprojected,
        gaps.unprojected_sketch_relations,
        format_args!(
            "{} decoded sketch relation(s) have no neutral constraint.",
            gaps.unprojected_sketch_relations
        ),
    )?;
    push(
        F3dLossCode::DimensionUnprojected,
        gaps.unprojected_dimensions,
        format_args!(
            "{} Design dimension parameter(s) have no parameter-backed neutral or native sketch constraint.",
            gaps.unprojected_dimensions
        ),
    )?;
    push(
        F3dLossCode::FeatureProfileSelectionNative,
        gaps.profile_selections,
        format_args!(
            "{} feature profile selection(s) retain native selection identities because no unique neutral profile was resolved.",
            gaps.profile_selections
        ),
    )?;
    push(
        F3dLossCode::FeaturePathSelectionNative,
        gaps.path_selections,
        format_args!(
            "{} feature path selection(s) retain native selection identities because no unique neutral path was resolved.",
            gaps.path_selections
        ),
    )?;
    push(
        F3dLossCode::FeatureFaceSelectionNative,
        gaps.face_selections,
        format_args!(
            "{} feature face selection(s) retain native candidates because no unique topological face was resolved.",
            gaps.face_selections
        ),
    )?;
    push(
        F3dLossCode::FeatureFaceSelectionActiveSubstituted,
        gaps.active_face_substitutions,
        format_args!(
            "{} legacy face operand(s) use a current active-BREP face because no unique preceding-state face slot resolved.",
            gaps.active_face_substitutions
        ),
    )?;
    push(
        F3dLossCode::FeatureBodySelectionNative,
        gaps.body_selections,
        format_args!(
            "{} feature body selection(s) retain native identities because no unique solved body was resolved.",
            gaps.body_selections
        ),
    )?;
    push(
        F3dLossCode::FeatureFaceOperandUnresolved,
        gaps.partially_resolved_face_members,
        format_args!(
            "{} feature face operand(s) remain unresolved inside state-bound historical selections.",
            gaps.partially_resolved_face_members
        ),
    )?;
    push(
        F3dLossCode::FeatureEdgeSelectionNative,
        gaps.native_edge_selections,
        format_args!(
            "{} edge-treatment selection(s) retain native construction recipes because no neutral historical edge selection was resolved.",
            gaps.native_edge_selections
        ),
    )?;
    push(
        F3dLossCode::FeatureEdgeOperandUnresolved,
        gaps.partially_resolved_edge_members,
        format_args!(
            "{} edge-treatment operand(s) remain unresolved inside state-bound historical selections.",
            gaps.partially_resolved_edge_members
        ),
    )?;
    push(
        F3dLossCode::FeatureEdgeSelectionLost,
        gaps.unresolved_edge_selections,
        format_args!(
            "{} edge-treatment selection(s) are unresolved because their source edge references were lost.",
            gaps.unresolved_edge_selections
        ),
    )?;
    Ok(())
}

fn model_brep_candidates<'s>(
    scan: &'s ContainerScan<'_>,
    blob_names: &[String],
) -> Result<Vec<&'s BrepFacts>, CodecError> {
    let mut candidates = Vec::new();
    for blob_name in blob_names {
        let mut matches = container::design_breps(scan)
            .filter(|brep| brep.name.rsplit('/').next() == Some(blob_name.as_str()));
        let Some(brep) = matches.next() else {
            return Err(CodecError::malformed(format_args!(
                "Design body map references missing BREP entry {blob_name}"
            )));
        };
        if matches.next().is_some() {
            return Err(CodecError::malformed(format_args!(
                "Design body map BREP basename is ambiguous: {blob_name}"
            )));
        }

        candidates.push(brep);
    }
    Ok(candidates)
}

/// Decode the document model from its text-encoded carriers.
///
/// The text encoding is the model carrier only when no binary stream decoded,
/// so the caller runs this after the binary candidate loop. Every `.sat` and
/// `.smt` entry that parses and produces geometry joins the merged graph;
/// with more than one contributing entry, each graph is qualified by its
/// entry basename.
fn try_decode_text_model(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Option<(BrepFacts, Brep)>, CodecError> {
    let mut parts: Vec<(BrepFacts, Brep)> = Vec::new();
    for name in container::text_brep_names(scan) {
        let bytes = scan.entry_bytes(name)?;
        let stream = match scan.text_breps.get(name) {
            Some(crate::container::TextBrepFraming::Parsed(stream)) => stream,
            Some(crate::container::TextBrepFraming::Unframed(error)) => {
                return Err(CodecError::malformed(format_args!(
                    "text BREP entry {name} failed to parse: {error}"
                )));
            }
            Some(crate::container::TextBrepFraming::Malformed(error)) => {
                return Err(CodecError::malformed(error));
            }
            Some(crate::container::TextBrepFraming::UnsupportedLength(error)) => {
                return Err(CodecError::NotImplemented(error.to_string()));
            }
            None => {
                return Err(CodecError::malformed(format_args!(
                    "text BREP entry {name} was not framed"
                )))
            }
        };
        let decoded = brep::decode_text(ctx, stream, bytes, name, crate::ids::ID_FORMAT)?;
        if decoded.asm.surfaces.is_empty()
            && decoded.asm.points.is_empty()
            && decoded.asm.faces.is_empty()
        {
            continue;
        }
        // Facts for the report and source attributes. The header carries the
        // stream's own unit; the decoded token values are already in the
        // centimetre convention.
        let mut header = stream.header.as_kernel_header();
        header.scale = match stream.header.units() {
            cadmpeg_asm::sat::TextUnits::Declared(scale) => Some(scale.get()),
            cadmpeg_asm::sat::TextUnits::Unspecified => None,
        };

        parts.push((
            BrepFacts {
                name: name.to_owned(),
                uncompressed_len: u64_from_index(bytes.len()),
                kernel: Some(crate::container::KernelFraming::Text {
                    header,
                    terminator: stream.terminator,
                }),
                sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(bytes),
            },
            decoded,
        ));
    }
    let qualify = parts.len() > 1;
    let mut merged: Option<(BrepFacts, Brep)> = None;
    for (facts, mut part) in parts {
        if qualify {
            let namespace = facts.name.rsplit('/').next().unwrap_or(&facts.name);
            part.qualify_ids(ctx, crate::ids::ID_FORMAT, namespace)?;
        }
        match &mut merged {
            None => merged = Some((facts, part)),
            Some((_, whole)) => whole.append(part),
        }
    }
    Ok(merged)
}

/// Assemble the complete document result from one decoded model B-rep graph.
///
/// Both encodings converge here: the binary candidate loop and the text-stream
/// decode hand over the merged graph, the facts of the primary carrier, the
/// resolved body visibilities, and the count of candidates whose decode
/// produced nothing.
fn finish_model_decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
    primary_model_brep: &BrepFacts,
    brep: Brep,
    body_visibilities: Vec<crate::records::bodies::BodyVisibility>,
    undecoded_candidates: usize,
    session_state: DecodeSessionState,
) -> Result<AuthoredDecoded, CodecError> {
    let (session, path) = F3dDecodeSession::from_geometry(
        ctx,
        scan,
        primary_model_brep,
        brep,
        body_visibilities,
        undecoded_candidates,
        session_state,
    )?;
    session.into_result(path)
}

/// Optional geometry index after a successful B-rep transfer.
///
/// Absence means the document is assembled from Design / mesh / presentation
/// content alone; product decoding still runs through the same session path.
struct GeometryIndex {
    primary_model_brep_name: String,
    annotation_records: Vec<cadmpeg_asm::brep::annotations::AnnotationRecord>,
    mesh_projection: MeshProjection,
}

/// State shared by every finalization path for one decoded F3D member.
#[derive(Debug)]
struct DecodeSessionState {
    admitted_entities: u64,
    report_scope: crate::report::ReportScope,
}

struct DeferredBodylessInputs {
    model_body_maps_empty: bool,
    has_appearance: bool,
    mesh_projection: MeshProjection,
}

/// Geometry-path inputs carried from construction to product decoding.
struct GeometrySessionPath {
    index: GeometryIndex,
    materials: materials::DecodedMaterials,
}

enum SessionPath {
    Geometry(Box<GeometrySessionPath>),
    Bodyless {
        mesh_projection: MeshProjection,
        model_body_maps_empty: bool,
    },
}

/// Phase-2 outcome as product projection and finalization see it.
enum DesignState {
    /// Phase 2 ran. Its native records wait for finalization.
    Projected(Box<design_intent::DesignNative>),
    /// No phase-2 output: the request is below [`DecodeDepth::Full`], or
    /// phase 2 failed and the containment loss and a native
    /// [`design_intent::DesignIntentFailure`] say so. The design reports read
    /// phase-2 records, so they do not run.
    Absent,
}

/// The inputs one decoded path hands to finalization.
enum FinalizePath {
    Geometry(GeometryIndex),
    Bodyless(DeferredBodylessInputs),
}

pub(crate) fn saved_free_edge_bodies_loss(
    model: &cadmpeg_ir::document::Model,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let owned = model
        .product_definitions
        .iter()
        .flat_map(|definition| &definition.bodies)
        .collect::<std::collections::BTreeSet<_>>();
    let count = model
        .bodies
        .iter()
        .filter(|body| {
            cadmpeg_asm::brep::emit::is_saved_free_edge_body(&body.id) && !owned.contains(&body.id)
        })
        .count();
    (count != 0).then(|| F3dLossCode::SavedFreeEdgeBodyUnowned.note(format!(
        "{count} saved free-edge bodies have no product-definition owner; their component placement is not assigned."
    )))
}

impl FinalizePath {
    fn component_bodies(
        &self,
        bodies: &[cadmpeg_ir::topology::Body],
        has_pending_body_source: bool,
    ) -> Option<std::collections::BTreeSet<cadmpeg_ir::ids::BodyId>> {
        match self {
            Self::Geometry(_) => Some(
                bodies
                    .iter()
                    .filter(|body| !cadmpeg_asm::brep::emit::is_saved_free_edge_body(&body.id))
                    .map(|body| body.id.clone())
                    .collect(),
            ),
            Self::Bodyless(inputs) if inputs.model_body_maps_empty && !has_pending_body_source => {
                Some(std::collections::BTreeSet::new())
            }
            Self::Bodyless(_) => None,
        }
    }
}

/// Private decode accumulator for one `.f3d` document.
struct F3dDecodeSession<'a> {
    ctx: &'a DecodeContext<'a>,
    scan: &'a ContainerScan<'a>,
    native: F3dNative,
    ir: CadIr,
    source_attributes: std::collections::BTreeMap<String, String>,
    report: DecodeBody,
    report_scope: crate::report::ReportScope,
    unknowns: Vec<UnknownRecord>,
    admitted_entities: u64,
}

impl<'a> F3dDecodeSession<'a> {
    fn from_geometry(
        ctx: &'a DecodeContext<'a>,
        scan: &'a ContainerScan<'a>,
        primary_model_brep: &BrepFacts,
        brep: Brep,
        body_visibilities: Vec<crate::records::bodies::BodyVisibility>,
        undecoded_candidates: usize,
        session_state: DecodeSessionState,
    ) -> Result<(Self, SessionPath), CodecError> {
        let DecodeSessionState {
            admitted_entities: _,
            report_scope,
        } = session_state;
        let mut report = crate::report::build_decode_report(
            scan,
            cadmpeg_ir::report::decode::DecodeTransfer::at(ctx.depth(), true),
            geometry_losses(&brep),
        )?;
        if undecoded_candidates != 0 {
            push_loss_vec(
                &mut report.losses,
                F3dLossCode::BrepBlobUndecoded,
                format_args!(
                    "{undecoded_candidates} Design-referenced BREP blob(s) could not be decoded."
                ),
            );
        }
        let design_body_bindings = crate::design::decode::body::decode_design_body_bindings(
            ctx,
            scan,
            Some(&primary_model_brep.name),
            &brep.asm.body_native_keys,
        )?;
        let geometry_materials =
            materials::decode_with_body_bindings(ctx, scan, &design_body_bindings)?;
        let (mut ir, source_attributes, mut native, mut asm_remainder) =
            build_geometry_ir(ctx, scan, primary_model_brep, brep)?;
        geometry_admission::admit(
            ctx,
            &mut ir,
            &mut report.losses,
            &mut asm_remainder.annotation_records,
        )?;
        // ASM transfer already charged its delta; keep the running counter in
        // sync so a later admit_entities call cannot double-count those bodies.
        let mut admitted_entities = u64_from_index(ir.model.entity_count());
        let AsmTransferRemainder {
            unknowns,
            stats: _,
            annotation_records,
        } = asm_remainder;
        let (subds, subd_losses) = crate::tsm::decode(ctx, scan)?;
        ir.model.subds = subds;
        let mesh_projection = project_mesh_bodies(ctx, scan, &mut ir, &mut native, &mut report)?;
        report.losses.extend(subd_losses);
        native.body_visibilities = body_visibilities;
        native.design_body_bindings = design_body_bindings;
        ctx.admit_entities(
            u64_from_index(ir.model.entity_count()),
            &mut admitted_entities,
            "admit F3D geometry entities",
        )?;
        Ok((
            Self {
                ctx,
                scan,
                native,
                ir,
                source_attributes,
                report,
                report_scope,
                unknowns,
                admitted_entities,
            },
            SessionPath::Geometry(Box::new(GeometrySessionPath {
                index: GeometryIndex {
                    primary_model_brep_name: primary_model_brep.name.clone(),
                    annotation_records,
                    mesh_projection,
                },
                materials: geometry_materials,
            })),
        ))
    }

    fn from_metadata(
        ctx: &'a DecodeContext<'a>,
        scan: &'a ContainerScan<'a>,
        session_state: DecodeSessionState,
        brep_outcome: &ModelBrepOutcome,
    ) -> Result<(Self, SessionPath), CodecError> {
        let DecodeSessionState {
            admitted_entities,
            report_scope,
        } = session_state;
        let MetadataIr {
            mut ir,
            source_attributes,
            unknowns,
        } = build_metadata_ir(scan)?;
        let mut native = F3dNative::default();
        let mut report = crate::report::build_decode_report(
            scan,
            cadmpeg_ir::report::decode::DecodeTransfer::at(ctx.depth(), false),
            container_losses(scan, brep_outcome),
        )?;
        let mesh_projection = project_mesh_bodies(ctx, scan, &mut ir, &mut native, &mut report)?;
        Ok((
            Self {
                ctx,
                scan,
                native,
                ir,
                source_attributes,
                report,
                report_scope,
                unknowns,
                admitted_entities,
            },
            SessionPath::Bodyless {
                mesh_projection,
                model_body_maps_empty: matches!(brep_outcome, ModelBrepOutcome::NoPhysicalModel),
            },
        ))
    }

    fn admit_model_entities(&mut self, operation: &'static str) -> Result<(), CodecError> {
        self.ctx.admit_entities(
            u64_from_index(self.ir.model.entity_count()),
            &mut self.admitted_entities,
            operation,
        )
    }

    /// Decode design graph, products, annotations, and the report.
    fn into_result(mut self, path: SessionPath) -> Result<AuthoredDecoded, CodecError> {
        self.admit_model_entities("admit F3D geometry entities")?;
        let prereqs = self.decode_product_prereqs(&path)?;
        self.report
            .losses
            .extend(prereqs.design_parameter_scope_losses.iter().cloned());
        report_unparsed_parameter_records(&mut self.report, &prereqs.design_parameter_unparsed);
        report_unparsed_parameter_owners(
            &mut self.report,
            prereqs.design_parameter_owner_unparsed.len(),
        );
        if prereqs.design_superseded_scope_envelopes != 0 {
            push_loss_vec(
                &mut self.report.losses,
                F3dLossCode::FeatureScopeEnvelopeSuperseded,
                format_args!(
                    "{} duplicate feature scope envelope(s) differ from the primary-indexed envelope of their record and were not decoded.",
                    prereqs.design_superseded_scope_envelopes
                ),
            );
        }
        self.report
            .losses
            .extend(prereqs.design_component_occurrence_losses.iter().cloned());
        self.report.losses.extend(
            crate::design::decode::components::superseded_occurrences_loss(
                prereqs.design_superseded_component_occurrences,
            ),
        );
        self.report.losses.extend(
            crate::design::decode::components::ambiguous_current_poses_loss(
                prereqs.design_ambiguous_current_poses,
            ),
        );
        self.report.losses.extend(
            crate::design::decode::components::unbound_naming_spaces_loss(
                prereqs.design_unbound_component_naming_spaces,
            ),
        );
        let path = self.decode_products(path, &prereqs)?;
        let design = self.decode_design_state(&path, &prereqs)?;
        self.finalize(path, prereqs, design)
    }

    /// Run phase 2 and merge its outputs, or contain its failure.
    ///
    /// This is the one point that decides whether design intent enters the
    /// document. Phase 2 does not run when the phase-1 timelines did not
    /// decode, nor below [`DecodeDepth::Full`] where
    /// [`Self::cuts_design_intent`] holds; where it does not, phase 2 runs and
    /// the session delivers `Full`. [`DesignState::Absent`] leaves a complete
    /// product and geometry document from phase 1. A resource refusal, or any
    /// error after the session fused, still refuses the document.
    fn decode_design_state(
        &mut self,
        path: &FinalizePath,
        prereqs: &prereqs::ProductPrereqs,
    ) -> Result<DesignState, CodecError> {
        if self.ctx.depth() < DecodeDepth::Full {
            if self.cuts_design_intent(path, prereqs) {
                return Ok(DesignState::Absent);
            }
            self.report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::at(
                DecodeDepth::Full,
                self.report.transfer.geometry_transferred(),
            );
        }
        let failure = match &prereqs.design_feature_timelines {
            Ok(timelines) => match self.decode_design_intent(path, prereqs, timelines) {
                Ok(design) => {
                    return Ok(DesignState::Projected(Box::new(self.merge_design(design))))
                }
                Err(error)
                    if matches!(error, CodecError::ResourceLimit(_))
                        || self.ctx.resource_refusal().is_some() =>
                {
                    return Err(error)
                }
                Err(error) => error.to_string(),
            },
            Err(error) => match self.ctx.resource_refusal() {
                Some(limit) => return Err(CodecError::ResourceLimit(limit)),
                None => error.to_string(),
            },
        };
        push_loss_vec(
            &mut self.report.losses,
            F3dLossCode::DesignIntentUndecoded,
            format_args!("Design intent was dropped because its decoding failed, so the dimension-companion and configuration reports were not produced: {failure}"),
        );
        self.native
            .design_intent_failures
            .push(design_intent::DesignIntentFailure {
                id: crate::ids::DESIGN_INTENT_FAILURE_ID.to_owned(),
                reason: failure,
            });
        Ok(DesignState::Absent)
    }

    /// Whether a decode below full depth can stop before phase 2; see
    /// [`cuts_design_intent`].
    fn cuts_design_intent(&self, path: &FinalizePath, prereqs: &prereqs::ProductPrereqs) -> bool {
        let bodyless_without_body_source = match path {
            FinalizePath::Geometry(_) => false,
            FinalizePath::Bodyless(inputs) => {
                inputs.mesh_projection.count == 0
                    && container::design_breps(self.scan).next().is_none()
                    && container::text_brep_names(self.scan).next().is_none()
                    && prereqs.design_body_bindings.is_empty()
                    && prereqs.design_body_members.is_empty()
            }
        };
        cuts_design_intent(
            &self.report_scope,
            &prereqs.design_parameter_scopes,
            bodyless_without_body_source,
        )
    }

    /// Phase 1: take the geometry body bindings and decode the other product
    /// prerequisites.
    fn decode_product_prereqs(
        &mut self,
        path: &SessionPath,
    ) -> Result<prereqs::ProductPrereqs, CodecError> {
        let design_body_bindings = match path {
            SessionPath::Geometry(_) => std::mem::take(&mut self.native.design_body_bindings),
            SessionPath::Bodyless { .. } => {
                crate::design::decode::body::decode_design_body_bindings(
                    self.ctx,
                    self.scan,
                    None,
                    &self.native.body_native_keys,
                )?
            }
        };
        prereqs::decode(self.ctx, self.scan, design_body_bindings)
    }

    /// Phase 2: decode design intent over the prerequisites and the geometry.
    fn decode_design_intent(
        &self,
        path: &FinalizePath,
        prereqs: &prereqs::ProductPrereqs,
        timelines: &[crate::records::entity_header::DesignFeatureTimeline],
    ) -> Result<design_intent::DesignOutputs, CodecError> {
        let model = &self.ir.model;
        design_intent::decode(&design_intent::DesignInputs {
            ctx: self.ctx,
            scan: self.scan,
            prereqs,
            timelines,
            products: design_intent::DesignProducts {
                occurrences: &model.occurrences,
                assembly_joints: &model.assembly_joints,
            },
            geometry: design_intent::DesignGeometry {
                primary_brep: match path {
                    FinalizePath::Geometry(index) => Some(&index.primary_model_brep_name),
                    FinalizePath::Bodyless(_) => None,
                },
                bodies: &model.bodies,
                regions: &model.regions,
                shells: &model.shells,
                faces: &model.faces,
                surfaces: &model.surfaces,
                subds: &model.subds,
                assets: &model.assets,
                tolerances: &self.ir.tolerances,
                persistent_design_links: &self.native.persistent_design_links,
                persistent_subentity_tags: &self.native.persistent_subentity_tags,
                mesh_projection: match path {
                    FinalizePath::Geometry(index) => &index.mesh_projection,
                    FinalizePath::Bodyless(inputs) => &inputs.mesh_projection,
                },
                path: match path {
                    FinalizePath::Geometry(_) => design_intent::DesignPath::Geometry,
                    FinalizePath::Bodyless(_) => design_intent::DesignPath::Bodyless,
                },
            },
        })
    }

    /// Add the phase-2 losses and neutral arenas. Native records wait for
    /// finalization, after the product prerequisites.
    fn merge_design(
        &mut self,
        design: design_intent::DesignOutputs,
    ) -> design_intent::DesignNative {
        let design_intent::DesignOutputs {
            native,
            model,
            losses,
        } = design;
        self.report.losses.extend(losses);
        model.store(&mut self.ir.model);
        native
    }

    fn decode_products(
        &mut self,
        path: SessionPath,
        prereqs: &prereqs::ProductPrereqs,
    ) -> Result<FinalizePath, CodecError> {
        let scan = self.scan;
        let act = crate::act::decode(scan)?;
        let unresolved_links = act
            .component_links
            .iter()
            .filter(|link| link.references.tracked_entity != 3)
            .count();
        if unresolved_links != 0 {
            push_loss_vec(&mut self.report.losses, F3dLossCode::ActComponentLinkUnresolved,
                format_args!("{unresolved_links} ACT component link(s) were retained natively without a resolved product-structure role."));
        }
        self.native.act_entities = act.entities;
        self.native.act_guids = act.guids;
        self.native.act_registry_channels = act.registry_channels;
        self.native.act_component_links = act.component_links;
        self.native.act_table_references = act.table_references;

        let finalize_path = match path {
            SessionPath::Geometry(geometry_path) => {
                let GeometrySessionPath { index, materials } = *geometry_path;
                report_untyped_material_distances(
                    &mut self.report,
                    materials.untyped_distance_properties,
                );
                self.report.notes.extend(materials.notes);
                self.ir.model.appearances = materials.appearances;
                self.ir.model.appearance_bindings = materials.bindings;
                let face_losses =
                    resolve_face_appearance_bindings(&mut self.ir, &materials.face_assignments)?;
                apply_appearance_base_colors(&mut self.ir);
                apply_body_presentation_names(&mut self.ir, scan, &prereqs.design_body_bindings)?;
                self.ir
                    .model
                    .appearance_bindings
                    .sort_by(|a, b| a.id.cmp(&b.id));
                reconcile_appearance_loss(
                    &mut self.report,
                    &self.ir,
                    materials.has_topology_assignments,
                );
                self.report.losses.extend(face_losses);
                annotate_docstruct(self.ctx, &mut self.source_attributes, scan)?;
                match &prereqs.xref {
                    Ok(Some(table)) => {
                        report_xref_placement_failures(&mut self.report, table);
                        report_xref_placement_overrides(&mut self.report, table);
                    }
                    Ok(None) => {}
                    Err(error) => report_xref_parse_loss(&mut self.report, error),
                }
                FinalizePath::Geometry(index)
            }
            SessionPath::Bodyless {
                mesh_projection,
                model_body_maps_empty,
            } => {
                let decoded_materials = materials::decode(self.ctx, scan)?;
                report_untyped_material_distances(
                    &mut self.report,
                    decoded_materials.untyped_distance_properties,
                );
                self.report.notes.extend(decoded_materials.notes);
                self.ir.model.appearances = decoded_materials.appearances;
                self.ir.model.appearance_bindings = decoded_materials.bindings;
                annotate_docstruct(self.ctx, &mut self.source_attributes, scan)?;
                if let Some(table) = prereqs.xref_table() {
                    report_xref_placement_failures(&mut self.report, table);
                    report_xref_placement_overrides(&mut self.report, table);
                }
                FinalizePath::Bodyless(DeferredBodylessInputs {
                    model_body_maps_empty,
                    has_appearance: decoded_materials.has_topology_assignments,
                    mesh_projection,
                })
            }
        };

        let local_components = crate::design::decode::components::decode_local_components(
            scan,
            &prereqs.design_component_naming_spaces,
        )?;
        self.report
            .losses
            .extend(crate::design::decode::components::revised_definitions_loss(
                &local_components,
            ));
        self.report
            .losses
            .extend(crate::design::decode::components::unbound_roots_loss(
                &local_components,
            ));
        let bodies = finalize_path.component_bodies(
            &self.ir.model.bodies,
            !prereqs.design_body_members.is_empty()
                || !prereqs.design_body_bindings.is_empty()
                || container::text_brep_names(scan).next().is_some(),
        );
        let hidden_bodies = self
            .ir
            .model
            .bodies
            .iter()
            .filter(|body| body.visible == Some(false))
            .map(|body| body.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let local_sources = crate::design::components::LocalComponentSources {
            scopes: &prereqs.design_parameter_scopes,
            occurrences: &prereqs.design_component_occurrences,
            paths: &prereqs.design_component_paths,
            components: &local_components,
            hidden_bodies: Some(&hidden_bodies),
            body_bindings: &prereqs.design_body_bindings,
            bodies: bodies.as_ref(),
            root_ordinal_start: prereqs
                .xref_table()
                .map_or(0, crate::xref::local_root_ordinal_start),
            limits: self.ctx.policy().limits,
        };
        let local = crate::design::components::project_local_components_with_types(
            &local_sources,
            &prereqs.design_types,
        )?;
        if let Some(table) = prereqs.xref_table() {
            let external =
                crate::xref::project_occurrences(table, &local_sources, &local.occurrences)?;
            self.ir.model.occurrences.extend(external.occurrences);
            self.report.losses.extend(external.losses);
        }
        self.ir.model.product_definitions.extend(local.definitions);
        self.ir.model.occurrences.extend(local.occurrences);
        self.report.losses.extend(local.losses);
        let mut component_inserts = crate::design::components::ComponentInserts::decide(
            &prereqs.design_parameter_scopes,
            prereqs.feature_timelines().unwrap_or_default(),
            prereqs
                .xref_table()
                .map(|table| table.references.as_slice())
                .unwrap_or_default(),
        );
        component_inserts.bind_external_paths(
            prereqs
                .xref_table()
                .map_or(&[], |table| table.references.as_slice()),
            &self.ir.model.occurrences,
        );
        let unresolved_component_inserts =
            component_inserts.unresolved_occurrences(self.ir.model.occurrences.len())?;
        self.ir
            .model
            .occurrences
            .extend(unresolved_component_inserts);
        self.report.losses.extend(component_inserts.losses());
        let joints = crate::design::assembly::project_assembly_joints_with_paths(
            &prereqs.design_parameter_scopes,
            &prereqs.design_component_occurrences,
            &component_inserts,
            &prereqs.design_joint_motions,
            &crate::design::components::paths::LocalOccurrencePaths::new(
                &prereqs.design_component_occurrences,
                prereqs
                    .xref_table()
                    .map_or(&[], |table| table.references.as_slice()),
                &self.ir.model.occurrences,
            ),
        );
        self.ir.model.assembly_joints = joints.joints;
        self.report.losses.extend(joints.losses);
        let joint_placed = crate::design::assembly::joint_placed_identity_occurrences(
            &self.ir.model.occurrences,
            self.ir
                .model
                .assembly_joints
                .iter()
                .flat_map(cadmpeg_ir::products::AssemblyJoint::connectors)
                .map(|connector| &connector.operand.container),
        );
        self.report
            .losses
            .extend(crate::design::assembly::joint_placed_occurrence_loss(
                joint_placed,
            ));
        Ok(finalize_path)
    }

    fn finalize(
        mut self,
        path: FinalizePath,
        prereqs: prereqs::ProductPrereqs,
        design: DesignState,
    ) -> Result<AuthoredDecoded, CodecError> {
        let scan = self.scan;
        let ctx = self.ctx;
        let geometry = match path {
            FinalizePath::Geometry(index) => {
                if let DesignState::Projected(design) = &design {
                    report_unresolved_dimension_companions(
                        &mut self.report,
                        unresolved_dimension_companion_count(
                            &DimensionCompanionSources::new(&prereqs, design),
                            &self.ir,
                        ),
                    );
                    report_unresolved_configuration_rules(
                        &mut self.report,
                        &design.design_configurations,
                        design.configuration_roots_unbound,
                        &self.ir,
                    );
                }
                if let Ok(Some(table)) = prereqs.store(&mut self.native) {
                    self.native.derived_references = table.derived_references;
                    self.native.xref_designs = table.designs;
                    self.native.xref_references = table.references;
                }
                if let DesignState::Projected(design) = design {
                    design.store(&mut self.native);
                    crate::design::decode::sketch_owners::attach(
                        ctx,
                        scan,
                        &self.native,
                        &mut self.ir.model,
                        &mut self.report.losses,
                    )?;
                    crate::design::owners::attach(
                        scan,
                        &self.native,
                        &mut self.ir.model,
                        &mut self.report.losses,
                    )?;
                    report_design_projection_gaps(&mut self.report, ctx, &self.ir, &self.native)?;
                }
                index
            }
            FinalizePath::Bodyless(inputs) => {
                let unresolved_companions = match &design {
                    DesignState::Projected(design) => unresolved_dimension_companion_count(
                        &DimensionCompanionSources::new(&prereqs, design),
                        &self.ir,
                    ),
                    DesignState::Absent => 0,
                };
                let xref = prereqs.store(&mut self.native);
                let projected = match design {
                    DesignState::Projected(design) => {
                        design.store(&mut self.native);
                        crate::design::decode::sketch_owners::attach(
                            ctx,
                            scan,
                            &self.native,
                            &mut self.ir.model,
                            &mut self.report.losses,
                        )?;
                        crate::design::owners::attach(
                            scan,
                            &self.native,
                            &mut self.ir.model,
                            &mut self.report.losses,
                        )?;
                        true
                    }
                    DesignState::Absent => false,
                };
                reconcile_appearance_loss(&mut self.report, &self.ir, inputs.has_appearance);
                let mesh_projection = inputs.mesh_projection;
                if projected {
                    report_design_projection_gaps(&mut self.report, ctx, &self.ir, &self.native)?;
                }
                ctx.admit_entities(
                    u64_from_index(self.ir.model.entity_count()),
                    &mut self.admitted_entities,
                    "admit F3D entities",
                )?;
                let assembly = match &xref {
                    Ok(Some(table)) => apply_assembly_classification(
                        self.ctx,
                        &mut self.report,
                        scan,
                        table,
                        inputs.model_body_maps_empty,
                    )?,
                    Ok(None) | Err(_) => false,
                };
                if mesh_projection.count > 0 {
                    apply_mesh_body_classification(&mut self.report, scan, mesh_projection.count);
                } else if !assembly {
                    apply_bodyless_design_classification(
                        &mut self.report,
                        if inputs.model_body_maps_empty {
                            0
                        } else {
                            container::design_breps(scan).count()
                        },
                        container::text_brep_names(scan).count(),
                        self.native.design_body_bindings.len()
                            + self.native.design_body_members.len(),
                        self.ir.model.sketch_entities.len()
                            + self.ir.model.spatial_sketch_entities.len(),
                        self.native.design_canvas_images.len(),
                    );
                }
                report_unresolved_dimension_companions(&mut self.report, unresolved_companions);
                match xref {
                    Ok(Some(table)) => {
                        self.native.derived_references = table.derived_references;
                        self.native.xref_designs = table.designs;
                        self.native.xref_references = table.references;
                    }
                    Ok(None) => {}
                    Err(error) => report_xref_parse_loss(&mut self.report, &error),
                }
                self.native
                    .store(ctx, self.ir.native.namespace_mut("f3d"))?;
                let annotations =
                    populate_annotations(&self.ir, scan, &self.native, None, &self.unknowns)?;
                let source_image = preserve_source_image(scan);
                let mut admitted_entities = self.admitted_entities;
                return decode_result(
                    ctx,
                    scan,
                    self.report_scope,
                    self.ir,
                    self.report,
                    RetainedArtifacts {
                        annotations,
                        unknowns: self.unknowns,
                        source_image,
                        source_attributes: self.source_attributes,
                    },
                    &mut admitted_entities,
                );
            }
        };

        ctx.admit_entities(
            u64_from_index(self.ir.model.entity_count()),
            &mut self.admitted_entities,
            "admit F3D entities",
        )?;
        self.native
            .store(ctx, self.ir.native.namespace_mut("f3d"))?;
        let annotations = populate_annotations(
            &self.ir,
            scan,
            &self.native,
            Some((
                &geometry.primary_model_brep_name,
                &geometry.annotation_records,
            )),
            &self.unknowns,
        )?;
        let source_image = preserve_source_image(scan);
        let mut admitted_entities = self.admitted_entities;
        decode_result(
            ctx,
            scan,
            self.report_scope,
            self.ir,
            self.report,
            RetainedArtifacts {
                annotations,
                unknowns: self.unknowns,
                source_image,
                source_attributes: self.source_attributes,
            },
            &mut admitted_entities,
        )
    }
}

/// Whether a decode below full depth can stop before phase 2 and still agree
/// with the full decode on every model arena and loss.
///
/// Phase 2 projects Canvas and Decal images into model assets. A bodyless
/// document without a mesh, BREP stream, or declared body classifies itself
/// by the sketch entities and canvas images phase 2 projects. An F3Z member
/// merges into one document with the rest of its archive, so no member decides
/// the archive's depth alone.
fn cuts_design_intent(
    report_scope: &crate::report::ReportScope,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    bodyless_without_body_source: bool,
) -> bool {
    use crate::records::feature::scope::DesignFeatureKind;
    matches!(report_scope, crate::report::ReportScope::Standalone)
        && !bodyless_without_body_source
        && !scopes.iter().any(|scope| {
            matches!(
                scope.kind(),
                DesignFeatureKind::Canvas | DesignFeatureKind::Decal
            )
        })
}

fn brep_identity_namespace(entry: &str) -> Option<&str> {
    entry.rsplit('/').next()?.strip_prefix("BREP.")
}

/// Decode an F3D or F3Z reader.
pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let scan = crate::f3z::observer::scan(ctx, root)?;
    match &scan.kind {
        container::F3dContainerKind::MultiDocument { .. } => crate::f3z::decode(ctx, &scan),
        container::F3dContainerKind::Document { .. } => {
            let mut decoded =
                decode_scanned_document(ctx, &scan, crate::report::ReportScope::Standalone)?;
            let poses = crate::xref::operation_pose::collect(
                ctx,
                &scan,
                &decoded.ir,
                &mut decoded.body.losses,
            )?;
            crate::xref::operation_pose::project(
                ctx,
                &mut decoded.ir,
                &poses,
                &std::collections::BTreeSet::new(),
                &mut decoded.body.losses,
            )?;
            crate::design::owners::release_unbound(&mut decoded.ir.model);
            decoded.ir.finalize();
            let hash = cadmpeg_ir::hash::document_local_sha256(
                ctx,
                &decoded.ir,
                Some(&decoded.source),
                "f3d",
                crate::ids::FILE_SOURCE_IMAGE_ID,
            )?;
            decoded.source.attributes.insert(
                cadmpeg_core::nonblank_const!(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
                hash,
            );
            Ok(decoded.into_decoded())
        }
    }
}

/// Decode one already-scanned F3Z member under archive-owned identity.
pub(crate) fn decode_archive_member<'a>(
    ctx: &DecodeContext<'a>,
    scan: &'a ContainerScan<'a>,
    dialects: &cadmpeg_core::dialect::DialectLayers,
) -> Result<AuthoredDecoded, CodecError> {
    let mut decoded = decode_scanned_document(
        ctx,
        scan,
        crate::report::ReportScope::ArchiveMember(dialects.clone()),
    )?;
    decoded.operation_poses =
        crate::xref::operation_pose::collect(ctx, scan, &decoded.ir, &mut decoded.body.losses)?;
    Ok(decoded)
}

fn decode_scanned_document<'a>(
    ctx: &DecodeContext<'a>,
    scan: &'a ContainerScan<'a>,
    report_scope: crate::report::ReportScope,
) -> Result<AuthoredDecoded, CodecError> {
    let mut admitted_entities = 0_u64;
    ctx.charge_entities(
        u64_from_index(scan.entries.len()),
        "admit F3D archive entries",
    )?;

    if ctx.container_only() {
        let MetadataIr {
            ir,
            mut source_attributes,
            unknowns,
        } = build_metadata_ir(scan)?;
        annotate_docstruct(ctx, &mut source_attributes, scan)?;
        let annotations = populate_annotations(&ir, scan, &F3dNative::default(), None, &unknowns)?;
        let source_image = preserve_source_image(scan);
        let mut report = crate::report::build_decode_report(
            scan,
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
            container_losses(scan, &ModelBrepOutcome::NotAttempted),
        )?;
        match crate::xref::decode(ctx, scan) {
            Ok(Some(table)) => {
                apply_assembly_classification(ctx, &mut report, scan, &table, false)?;
            }
            Ok(None) => {}
            Err(error @ (CodecError::ResourceLimit(_) | CodecError::NotImplemented(_))) => {
                return Err(error)
            }
            Err(error) => report_xref_parse_loss(&mut report, &error),
        }
        return decode_result(
            ctx,
            scan,
            report_scope,
            ir,
            report,
            RetainedArtifacts {
                annotations,
                unknowns,
                source_image,
                source_attributes,
            },
            &mut admitted_entities,
        );
    }

    let model_selection = crate::design::decode::body::design_model_selection(scan)?;
    let unbound_body_bindings =
        crate::design::decode::body::decode_design_body_bindings(ctx, scan, None, &[])?;
    let model_breps = model_brep_candidates(scan, &model_selection.blob_names)?;
    let only_history_bodies = !unbound_body_bindings.is_empty()
        && unbound_body_bindings
            .iter()
            .all(|binding| binding.history_only);
    let mut brep_outcome = if model_selection.explicitly_empty {
        ModelBrepOutcome::NoPhysicalModel
    } else {
        ModelBrepOutcome::NotAttempted
    };

    // Every Design body-map pair names its owning BREP blob. Decode the
    // complete referenced set; a document-level model is not confined to one
    // arbitrary `.smbh` entry.
    if !model_breps.is_empty() {
        let mut primary_model_brep = None;
        let qualify_ids = model_breps.len() > 1;
        let mut brep = Brep::default();
        let mut body_visibilities = Vec::new();
        let mut decoded_brep_count = 0usize;
        let all_body_visibility = crate::design::decode::body::decode_all_body_visibility(scan)?;
        let mut selected_body_keys =
            std::collections::HashMap::<String, std::collections::HashSet<u64>>::new();
        for binding in &unbound_body_bindings {
            if binding.history_only {
                selected_body_keys
                    .entry(binding.blob_name().to_owned())
                    .or_default();
                continue;
            }
            index_selected_body_key(
                &mut selected_body_keys,
                binding.blob_name(),
                binding.asm_body_key,
            );
        }
        for &candidate in &model_breps {
            // A stream that fails to frame is reported even when another
            // selected stream decoded with no geometry.
            let Some(mut part) = try_decode_brep(ctx, scan, candidate)? else {
                brep_outcome = ModelBrepOutcome::Failed {
                    stream: candidate.name.clone(),
                };
                continue;
            };
            let blob_name = candidate.name.rsplit('/').next().unwrap_or(&candidate.name);
            part.sketch_region_bodies_omitted = part.omit_sketch_region_bodies(
                ctx,
                selected_body_keys
                    .get(blob_name)
                    .map_or(0, std::collections::HashSet::len),
            )?;
            if part.sketch_region_bodies_omitted != 0 && part.asm.bodies.is_empty() {
                if !matches!(brep_outcome, ModelBrepOutcome::Failed { .. }) {
                    brep_outcome = ModelBrepOutcome::NoPhysicalModel;
                }
                continue;
            }
            if only_history_bodies && !matches!(brep_outcome, ModelBrepOutcome::Failed { .. }) {
                brep_outcome = ModelBrepOutcome::NoPhysicalModel;
            } else if !matches!(brep_outcome, ModelBrepOutcome::Failed { .. }) {
                brep_outcome = ModelBrepOutcome::Empty {
                    stream: candidate.name.clone(),
                };
            }
            if part.asm.surfaces.is_empty()
                && part.asm.points.is_empty()
                && part.asm.faces.is_empty()
            {
                continue;
            }
            if let Some(keys) = selected_body_keys.get(blob_name) {
                part.retain_body_keys(ctx, keys)?;
            }
            if part.asm.surfaces.is_empty()
                && part.asm.points.is_empty()
                && part.asm.faces.is_empty()
            {
                continue;
            }
            primary_model_brep.get_or_insert(candidate);
            let mut body_selectors = match selected_body_keys.get(blob_name) {
                Some(keys) => part.body_selectors_for(keys)?,
                None => part.body_selectors(),
            };
            for body in &mut part.asm.bodies {
                if let Some(visibility) = body_selectors.get(&body.id).and_then(|selector| {
                    body_visibility_for(&all_body_visibility, blob_name, *selector)
                }) {
                    body.visible = Some(visibility.visible);
                }
            }
            if qualify_ids {
                let namespace = brep_identity_namespace(&candidate.name).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "BREP entry has no stable blob identity: {}",
                        candidate.name
                    ))
                })?;
                part.qualify_ids(ctx, crate::ids::ID_FORMAT, namespace)?;
                body_selectors = match selected_body_keys.get(blob_name) {
                    Some(keys) => part.body_selectors_for(keys)?,
                    None => part.body_selectors(),
                };
            }
            for body in &part.asm.bodies {
                if let Some((body_selector, visibility)) =
                    body_selectors.get(&body.id).and_then(|selector| {
                        body_visibility_for(&all_body_visibility, blob_name, *selector)
                            .map(|visibility| (*selector, visibility))
                    })
                {
                    let id = crate::ids::native_scoped_id(
                        &candidate.name,
                        "body-visibility",
                        body_selector,
                    );

                    let visibility = crate::records::bodies::BodyVisibility::try_from(
                        crate::records::bodies::BodyVisibilityWire {
                            id,
                            body: body.id.clone(),
                            stream: visibility.stream.clone(),
                            byte_offset: visibility.byte_offset,
                            asm_body_key_offset: visibility.asm_body_key_offset,
                            asm_body_key: body_selector,
                            entity_suffix: visibility.entity_suffix,
                            visible: visibility.visible,
                        },
                    )
                    .map_err(CodecError::Malformed)?;
                    body_visibilities.push(visibility);
                }
            }
            brep.append(part);
            decoded_brep_count += 1;
        }
        if let Some(primary_model_brep) = primary_model_brep {
            return finish_model_decode(
                ctx,
                scan,
                primary_model_brep,
                brep,
                body_visibilities,
                model_breps.len() - decoded_brep_count,
                DecodeSessionState {
                    admitted_entities,
                    report_scope,
                },
            );
        }
    }

    // No binary stream decoded: the model may be carried only in the text
    // encoding.
    if let Some((text_facts, text_brep)) = try_decode_text_model(ctx, scan)? {
        return finish_model_decode(
            ctx,
            scan,
            &text_facts,
            text_brep,
            Vec::new(),
            0,
            DecodeSessionState {
                admitted_entities,
                report_scope,
            },
        );
    }

    // Keep the selected physical carrier outcome through metadata projection.
    let (session, path) = F3dDecodeSession::from_metadata(
        ctx,
        scan,
        DecodeSessionState {
            admitted_entities,
            report_scope,
        },
        &brep_outcome,
    )?;
    session.into_result(path)
}

/// Projected mesh geometry and the Design records that own it.
struct MeshProjection {
    /// Number of joined mesh-body containers.
    count: usize,
    /// Tessellation identities grouped by their Design stream and feature scope record.
    tessellations_by_scope: std::collections::HashMap<(String, u32), Vec<String>>,
}

fn extend_unique_assets(
    assets: &mut Vec<cadmpeg_ir::assets::Asset>,
    incoming: Vec<cadmpeg_ir::assets::Asset>,
) -> Result<(), CodecError> {
    for asset in incoming {
        match assets.iter().find(|existing| existing.id == asset.id) {
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

fn collect_mesh_outcome(
    bodies: &mut Vec<crate::design::decode::mesh::MeshBody>,
    report: &mut DecodeBody,
    outcome: crate::design::decode::mesh::MeshContainerOutcome,
) -> Result<(), CodecError> {
    use crate::design::decode::mesh::MeshContainerOutcome;
    match outcome {
        MeshContainerOutcome::Joined(body) => {
            bodies.push(body);
        }
        MeshContainerOutcome::Unjoined { entry_name } => push_loss_vec(&mut report.losses, F3dLossCode::MeshContainerUnjoined, format_args!(
                "mesh geometry container `{entry_name}` decoded but has no complete Design body join"
            ),),
        MeshContainerOutcome::Failed {
            error: error @ CodecError::ResourceLimit(_),
            ..
        } => return Err(error),
        MeshContainerOutcome::Failed { entry_name, error } => push_loss_vec(&mut report.losses, F3dLossCode::MeshContainerUndecoded, format_args!("mesh geometry container `{entry_name}` was not decoded: {error}"),),
        MeshContainerOutcome::Missing { entry_name } => push_loss_vec(&mut report.losses, F3dLossCode::MeshContainerMissing, format_args!(
                "Design mesh body names `{entry_name}`, but no unique geometry container joined it"
            ),),
    }
    Ok(())
}

fn insert_mesh_texture_table(
    tables: &mut std::collections::HashMap<String, Vec<(String, cadmpeg_ir::assets::AssetId)>>,
    tessellation_id: &str,
    table: &[(String, cadmpeg_ir::assets::AssetId)],
) -> Result<(), CodecError> {
    if tables.contains_key(tessellation_id) {
        return Err(CodecError::Malformed(
            "F3D mesh tessellation belongs to more than one texture table".into(),
        ));
    }

    let key = tessellation_id.to_owned();
    tables.insert(key, table.to_vec());
    Ok(())
}

fn insert_mesh_scope_tessellations<'a>(
    index: &mut std::collections::HashMap<(String, u32), Vec<String>>,
    stream: &str,
    record_index: u32,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), CodecError> {
    let mut tessellations = Vec::new();
    for id in ids {
        let copy = id.to_owned();
        tessellations.push(copy);
    }
    if tessellations.is_empty() {
        return Ok(());
    }
    let stream = stream.to_owned();
    let key = (stream, record_index);
    if index.contains_key(&key) {
        return Err(CodecError::Malformed(
            "F3D Design mesh feature scope is not unique".into(),
        ));
    }

    index.insert(key, tessellations);
    Ok(())
}

/// Embed the archive entry of one mesh texture file as a neutral asset.
fn mesh_texture_asset(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    file: &crate::records::mesh::DesignMeshTextureFile,
    asset: &cadmpeg_ir::assets::AssetId,
) -> Result<cadmpeg_ir::assets::Asset, CodecError> {
    let media_type = std::path::Path::new(file.filename())
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| {
            if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
                Some("image/jpeg")
            } else if extension.eq_ignore_ascii_case("png") {
                Some("image/png")
            } else {
                None
            }
        })
        .map(str::to_owned);
    let bytes = scan.entry_bytes(file.archive_entry_name())?;
    // The asset keeps a decoded copy of the source texture bytes.
    ctx.retain_bytes(u64_from_index(bytes.len()), "retain F3D mesh texture bytes")?;
    cadmpeg_ir::assets::Asset::try_new(
        asset.clone(),
        Some(file.filename().to_owned()),
        media_type,
        cadmpeg_ir::assets::AssetContent::Embedded {
            data: cadmpeg_ir::assets::AssetData::new(bytes.to_vec())
                .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
        },
        Some(crate::ids::native_scoped_id(
            file.archive_entry_name(),
            "mesh-texture",
            0,
        )),
    )
    .map_err(CodecError::Malformed)
}

/// Project each mesh body's container geometry into the tessellation arena.
///
/// A mesh body carries no B-rep topology: its geometry is a triangle list, and
/// the neutral tessellation arena is where a standalone triangle list belongs.
/// Returns the number of bodies projected and reports every mesh-geometry
/// container that no body claimed.
fn project_mesh_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    native: &mut F3dNative,
    report: &mut DecodeBody,
) -> Result<MeshProjection, CodecError> {
    let decoded = match crate::design::decode::mesh::decode_mesh_bodies(ctx, scan) {
        Ok(decoded) => decoded,
        Err(CodecError::ResourceLimit(e)) => return Err(CodecError::ResourceLimit(e)),
        Err(e) => {
            push_loss_vec(
                &mut report.losses,
                F3dLossCode::MeshContainerUndecoded,
                format_args!("mesh bodies could not be decoded: {e}"),
            );
            return Ok(MeshProjection {
                count: 0,
                tessellations_by_scope: std::collections::HashMap::new(),
            });
        }
    };
    native.design_mesh_features = decoded.features;
    let mut texture_assets = Vec::new();
    for texture in native
        .design_mesh_features
        .iter()
        .flat_map(|feature| feature.texture_table.resources())
    {
        if ir
            .model
            .assets
            .iter()
            .chain(&texture_assets)
            .any(|asset: &cadmpeg_ir::assets::Asset| asset.id == texture.asset)
        {
            continue;
        }
        texture_assets.push(mesh_texture_asset(
            ctx,
            scan,
            &texture.file,
            &texture.asset,
        )?);
    }
    extend_unique_assets(&mut ir.model.assets, texture_assets)?;
    let mut texture_tables = std::collections::HashMap::new();
    for feature in &native.design_mesh_features {
        let mut texture_table = Vec::new();
        for texture in feature.texture_table.resources_in_flags_order() {
            let source_id = texture.resource_guid.as_str().to_owned();
            let asset = texture.asset.clone();
            texture_table.push((source_id, asset));
        }
        for body in feature.bodies() {
            if let Some(tessellation_id) = &body.tessellation_id {
                insert_mesh_texture_table(&mut texture_tables, tessellation_id, &texture_table)?;
            }
        }
    }
    let mut bodies = Vec::new();
    for outcome in decoded.outcomes {
        collect_mesh_outcome(&mut bodies, report, outcome)?;
    }
    let mut unresolved = std::collections::BTreeMap::new();
    let mut projection = MeshProjection {
        count: bodies.len(),
        tessellations_by_scope: std::collections::HashMap::new(),
    };
    for mut body in bodies {
        let texture_table = texture_tables.remove(&body.id).ok_or_else(|| {
            CodecError::Malformed("F3D joined mesh body has no owning texture table".into())
        })?;
        let texture_assignments = mesh_texture_assignments(
            ctx,
            body.texture_ids.as_deref(),
            &texture_table,
            body.triangles.len(),
        )?;
        let mut triangle_groups = Vec::new();
        for group in std::mem::take(&mut body.triangle_groups) {
            triangle_groups.push(cadmpeg_ir::tessellation::TessellationTriangleGroup {
                source_id: Some(group.source_id),
                triangles: group.triangles,
            });
        }
        let channels = mesh_attribute_channels(
            &body.attributes,
            body.vertices.len(),
            &body.triangles,
            &mut unresolved,
        )?;
        // The paramesh registry states an unshaded mesh by carrying no
        // corner-normal channel, so the lane arrives absent, never empty.
        let corner_normals = match body.corner_normals.take() {
            Some(normals) => {
                let mut converted = Vec::new();
                for normal in normals {
                    converted.push(cadmpeg_ir::features::FiniteVector3::from(normal));
                }
                Some(converted)
            }
            None => None,
        };
        let mesh = cadmpeg_ir::tessellation::TessellationMesh::from_corner_lanes(
            body.vertices,
            body.triangles,
            corner_normals,
        )
        .map_err(|error| {
            CodecError::malformed(format_args!("paramesh body record {}: {error}", body.id))
        })?;
        let tessellation = cadmpeg_ir::tessellation::Tessellation::from_parts(
            cadmpeg_ir::tessellation::TessellationId::mint(body.id)
                .map_err(|error| CodecError::Malformed(error.to_string()))?,
            mesh,
            channels,
        )
        .map_err(|err| CodecError::Malformed(err.to_string()))?
        .with_feature_edges(body.feature_edges)
        .and_then(|mesh| mesh.with_triangle_groups(triangle_groups))
        .and_then(|mesh| mesh.with_texture_assignments(texture_assignments))
        .map_err(|err| CodecError::Malformed(err.to_string()))?;
        ir.model.tessellations.push(tessellation);
    }
    if !texture_tables.is_empty() {
        return Err(CodecError::Malformed(
            "F3D mesh texture table has no joined tessellation body".into(),
        ));
    }
    for feature in &native.design_mesh_features {
        let stream = crate::ids::native_stream(&feature.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        insert_mesh_scope_tessellations(
            &mut projection.tessellations_by_scope,
            stream,
            feature.scope().record().record_index(),
            feature
                .bodies()
                .iter()
                .filter_map(|body| body.tessellation_id.as_deref()),
        )?;
    }
    report_unresolved_mesh_attributes(report, &unresolved);
    Ok(projection)
}

/// Resolve one-based `tid` values through a Design texture table. Zero leaves
/// the triangle untextured.
fn mesh_texture_assignments(
    ctx: &DecodeContext<'_>,
    texture_ids: Option<&[u32]>,
    textures: &[(String, cadmpeg_ir::assets::AssetId)],
    triangle_count: usize,
) -> Result<Vec<cadmpeg_ir::tessellation::TessellationTextureAssignment>, CodecError> {
    let Some(texture_ids) = texture_ids else {
        return Ok(Vec::new());
    };
    if texture_ids.len() != triangle_count {
        return Err(CodecError::Malformed(
            "F3D mesh texture-id count differs from the triangle count".into(),
        ));
    }
    let mut triangles =
        ctx.alloc_filled(textures.len(), Vec::new(), "f3d mesh texture assignments")?;
    for (triangle, texture_id) in texture_ids.iter().enumerate() {
        if *texture_id == 0 {
            continue;
        }
        let index = texture_id
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < textures.len())
            .ok_or_else(|| {
                CodecError::Malformed(
                    "F3D mesh triangle texture id names no Design texture resource".into(),
                )
            })?;

        triangles[index].push(u32::try_from(triangle).map_err(|_| {
            CodecError::Malformed("F3D mesh triangle ordinal is out of range".into())
        })?);
    }
    let mut assignments = Vec::new();
    for ((source_id, texture), triangles) in textures.iter().zip(triangles) {
        if triangles.is_empty() {
            continue;
        }

        let source_id = source_id.to_owned();
        let texture = texture.clone();
        assignments.push(cadmpeg_ir::tessellation::TessellationTextureAssignment {
            source_id: Some(source_id),
            texture,
            triangles,
        });
    }
    Ok(assignments)
}

/// Replace the native definition of each mesh-import scope with its exact
/// tessellation identities.
fn mesh_feature_tessellations<'a>(
    projection: &'a MeshProjection,
    stream: &str,
    record_index: u32,
) -> Option<&'a [String]> {
    let key = stream.to_string();
    projection
        .tessellations_by_scope
        .get(&(key, record_index))
        .map(Vec::as_slice)
}

fn bind_mesh_feature_definitions(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    projection: &MeshProjection,
) -> Result<(), cadmpeg_core::CodecError> {
    for feature in features {
        if feature.source_tag.as_deref() != Some("Base Mesh Feature") {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| scope.id == native_ref) else {
            continue;
        };
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(tessellations) =
            mesh_feature_tessellations(projection, stream, scope.record_index)
        else {
            continue;
        };
        if tessellations.is_empty() {
            continue;
        }
        let mut copies = Vec::new();
        for tessellation in tessellations {
            let copy = tessellation.to_owned();
            copies.push(copy);
        }
        feature
            .evaluation
            .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::MeshImport {
                    tessellations: copies
                        .try_into()
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                },
            ));
    }

    Ok(())
}

/// Project channels with a settled element layout and count unresolved channels
/// by domain.
///
/// An indexed channel stores one default value per vertex followed by values
/// selected at the explicit corner positions in its index stream. The IR keeps
/// the value table and expands those selections into one selector per triangle
/// corner.
fn mesh_attribute_channels(
    attributes: &[crate::paramesh::MeshAttribute],
    vertices: usize,
    triangles: &[[u32; 3]],
    unresolved: &mut std::collections::BTreeMap<crate::paramesh::MeshAttributeDomain, usize>,
) -> Result<Vec<cadmpeg_ir::tessellation::TessellationChannel>, CodecError> {
    use crate::paramesh::MeshAttributeDomain;

    let mut channels = Vec::new();
    for attribute in attributes {
        match (
            attribute.addressing.domain(),
            attribute.item_size(),
            attribute.count(),
        ) {
            (MeshAttributeDomain::Vertex, Some(item_size), Some(_)) => {
                match cadmpeg_ir::tessellation::TessellationChannel::new(
                    cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                    item_size,
                    attribute.role,
                    attribute.element_code(),
                    attribute.values().to_vec(),
                ) {
                    Ok(channel) => {
                        channels.push(channel);
                    }
                    Err(_) => *unresolved.entry(MeshAttributeDomain::Vertex).or_default() += 1,
                }
            }
            (MeshAttributeDomain::Corner, Some(item_size), Some(_)) => {
                let Some(selectors) = attribute.corner_selectors(vertices, triangles) else {
                    *unresolved.entry(MeshAttributeDomain::Corner).or_default() += 1;
                    continue;
                };
                match cadmpeg_ir::tessellation::TessellationChannel::new(
                    cadmpeg_ir::tessellation::ChannelAddressing::Corner { indices: selectors },
                    item_size,
                    attribute.role,
                    attribute.element_code(),
                    attribute.values().to_vec(),
                ) {
                    Ok(channel) => {
                        channels.push(channel);
                    }
                    Err(_) => *unresolved.entry(MeshAttributeDomain::Corner).or_default() += 1,
                }
            }
            (MeshAttributeDomain::Triangle, Some(item_size), Some(count))
                if usize::try_from(count) == Ok(triangles.len()) && item_size == 4 =>
            {
                if u32::try_from(triangles.len()).is_err() {
                    *unresolved.entry(MeshAttributeDomain::Triangle).or_default() += 1;
                    continue;
                }

                let mut indices = Vec::new();

                for index in 0..triangles.len() {
                    indices.push(u32::try_from(index).map_err(|_| {
                        CodecError::Malformed("F3D mesh triangle selector exceeds u32".into())
                    })?);
                }
                match cadmpeg_ir::tessellation::TessellationChannel::new(
                    cadmpeg_ir::tessellation::ChannelAddressing::Triangle { indices },
                    item_size,
                    attribute.role,
                    attribute.element_code(),
                    attribute.values().to_vec(),
                ) {
                    Ok(channel) => {
                        channels.push(channel);
                    }
                    Err(_) => *unresolved.entry(MeshAttributeDomain::Triangle).or_default() += 1,
                }
            }
            (domain, _, _) => *unresolved.entry(domain).or_default() += 1,
        }
    }
    Ok(channels)
}

/// Report mesh attribute channels that the projector left unresolved, grouped by
/// domain.
fn report_unresolved_mesh_attributes(
    report: &mut DecodeBody,
    unresolved: &std::collections::BTreeMap<crate::paramesh::MeshAttributeDomain, usize>,
) {
    use crate::paramesh::MeshAttributeDomain;

    for (domain, count) in unresolved {
        let (addressing, reason) = match domain {
            MeshAttributeDomain::Corner => (
                "triangle corners",
                "their indexed value table or corner selector stream has no settled layout",
            ),
            MeshAttributeDomain::Triangle => (
                "triangles",
                "their element code or value count has no settled layout",
            ),
            MeshAttributeDomain::Vertex => (
                "vertices",
                "their element code does not settle a stored element layout of one value per \
                 vertex",
            ),
        };
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::MeshAttributeNotTransferred,
            format_args!(
                "{count} mesh attribute channel(s) addressing {addressing} were not transferred: \
             {reason}."
            ),
        );
    }
}

/// Record the `Properties.dat` docstruct declaration on the source metadata.
fn annotate_docstruct(
    ctx: &DecodeContext<'_>,
    attributes: &mut std::collections::BTreeMap<String, String>,
    scan: &ContainerScan,
) -> Result<(), CodecError> {
    let Some(docstruct) = crate::xref::docstruct(ctx, scan)? else {
        return Ok(());
    };
    attributes.insert("docstruct_type".into(), docstruct.doc_type);
    if let Some(subtype) = docstruct.subtype {
        attributes.insert("docstruct_subtype".into(), subtype);
    }
    Ok(())
}

/// A warning for a present but unparseable `RedirectionsStream.dat`.
fn report_xref_parse_loss(report: &mut DecodeBody, error: &CodecError) {
    push_loss_vec(
        &mut report.losses,
        F3dLossCode::XrefTableUndecoded,
        format_args!("external-reference table was not decoded: {error}"),
    );
}

/// Report typed occurrence placements whose role path was readable but whose
/// generation-specific payload did not close.
fn report_xref_placement_failures(report: &mut DecodeBody, table: &crate::xref::XrefTable) {
    for (placement, carriers) in &table.ambiguous_current_poses {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::XrefCurrentPoseAmbiguous,
            format_args!(
                "external occurrence placement {placement} is named by component-path carriers {carriers:?}; it keeps its occurrence-placement matrix"
            ),
        );
    }
    for placement in &table.unbound_legacy_placements {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::XrefPlacementRoleUnbound,
            format_args!(
                "legacy external occurrence carrier {:?} has no container reference for roles {:?}",
                placement.carrier_record_index, placement.link_names
            ),
        );
    }
    let native_metadata = table
        .references
        .iter()
        .filter_map(|reference| reference.placement_source.as_ref())
        .filter(|source| crate::xref::legacy_placement_metadata_native(source))
        .count();
    if native_metadata != 0 {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::XrefPlacementMetadataNative,
            format_args!("{native_metadata} legacy external placement frame(s) retain their source occurrence identity, auxiliary metadata UUID, path discriminator, ordinal and cross-segment metadata reference only in native source frames"),
        );
    }
    if table.placement_metadata_missing && !table.references.is_empty() {
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::XrefPlacementMetadataMissing,
            format_args!("external-reference placements were not inferred from Design streams without a paired MetaStream type table and record index"),
        );
    }
    for failure in &table.placement_failures {
        let target = table
            .references
            .iter()
            .find(|reference| {
                failure
                    .link_names
                    .iter()
                    .any(|role| role == reference.neutron_role.as_str())
            })
            .map_or("an unresolved target", |reference| {
                reference.relative_path.as_str()
            });
        push_loss_vec(
            &mut report.losses,
            F3dLossCode::XrefPlacementUndecoded,
            format_args!(
                "external occurrence {} has typed placement carrier {:?} for roles {:?} that did not close under its generation grammar",
                target, failure.carrier_record_index, failure.link_names
            ),
        );
    }
}

/// Report structured placements that were ignored because a scope-bound
/// Component Insert construction supplied the transform for the same carrier.
fn report_xref_placement_overrides(report: &mut DecodeBody, table: &crate::xref::XrefTable) {
    for override_ in &table.placement_overrides {
        let ordinal = override_.ordinal;
        let count = override_.count;
        let Some(reference) = table
            .references
            .iter()
            .find(|reference| reference.ordinal == ordinal)
        else {
            continue;
        };
        push_loss_vec(&mut report.losses, F3dLossCode::XrefPlacementSuperseded, format_args!(
                "{count} structured placement record(s) for external occurrence {} and role {} were superseded by scope-bound Component Insert carrier(s)",
                reference.relative_path, reference.neutron_role
            ),);
    }
}

/// Classify a mesh-body document.
///
/// Mesh bodies use tessellation as their geometry carrier. The report marks
/// geometry as transferred and records vertex precision.
fn apply_mesh_body_classification(report: &mut DecodeBody, scan: &ContainerScan, bodies: usize) {
    if container::design_breps(scan).next().is_some() {
        return;
    }
    report.losses.retain(|loss| {
        !matches!(
            loss.code.taxonomy(),
            LossTaxonomy::GeometryNotTransferred
                | LossTaxonomy::TopologyNotTransferred
                | LossTaxonomy::MissingGeometryStream
        )
    });
    report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::at(report.transfer.depth(), true);
    push_loss_vec(
        &mut report.losses,
        F3dLossCode::MeshVertexPrecisionReduced,
        format_args!(
            "{bodies} mesh body geometry container(s) store vertex coordinates at f32 precision"
        ),
    );
}

/// Classify a bodyless design whose transferred content requires no BREP.
///
/// The container has zero BREP streams and the Design segment has zero bodies.
/// Sketch entities can supply the complete geometry. Reference-image timeline
/// objects are presentation content and require no geometry carrier.
fn apply_bodyless_design_classification(
    report: &mut DecodeBody,
    brep_streams: usize,
    text_brep_streams: usize,
    declared_bodies: usize,
    sketch_entities: usize,
    reference_images: usize,
) {
    if brep_streams != 0
        || text_brep_streams != 0
        || declared_bodies != 0
        || (sketch_entities == 0 && reference_images == 0)
    {
        return;
    }
    report.losses.retain(|loss| {
        !matches!(
            loss.code.taxonomy(),
            LossTaxonomy::GeometryNotTransferred
                | LossTaxonomy::TopologyNotTransferred
                | LossTaxonomy::MissingGeometryStream
        )
    });
    report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::at(report.transfer.depth(), true);
    let message = match (sketch_entities, reference_images) {
        (0, _) => format!("presentation-only design: the document declares no body, and its {reference_images} reference-image timeline object(s) require no BREP geometry"),
        (_, 0) => format!("sketch-only design: the document declares no body, and its {sketch_entities} sketch entity(s) are its complete geometry"),
        _ => format!("bodyless design: the document declares no body; its {sketch_entities} sketch entity(s) are its complete geometry, and its {reference_images} reference-image timeline object(s) require no BREP geometry"),
    };

    report
        .losses
        .push(F3dLossCode::BodylessDesignCarrier.note(message));
}

/// Reclassify a BREP-less assembly document: its model is the placement of
/// its XREF targets, so producing no geometry is not a loss
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
fn apply_assembly_classification(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    scan: &ContainerScan,
    table: &crate::xref::XrefTable,
    model_body_maps_empty: bool,
) -> Result<bool, CodecError> {
    if !crate::xref::is_assembly(ctx, scan, Some(table), model_body_maps_empty)? {
        return Ok(false);
    }
    report.losses.retain(|loss| {
        !(loss.severity >= Severity::Error
            && matches!(
                loss.code.category(),
                LossCategory::Geometry | LossCategory::Topology
            ))
    });
    push_loss_vec(
        &mut report.losses,
        F3dLossCode::AssemblyComponentsExternal,
        format_args!(
            "assembly document: geometry is defined by {} external reference(s); decode the \
         containing .f3z archive to resolve them",
            table.references.len()
        ),
    );
    for reference in &table.references {
        let property_note = XrefPropertyNote(reference);
        match crate::xref::design_for(table, reference) {
            Some(design) => report.notes.push(format!(
                "xref {}: {} -> {} (lineage {}, version {}, {})",
                reference.ordinal,
                design.display_name,
                design.target_file_name,
                design.lineage_urn,
                design.version_urn,
                property_note
            )),
            None => report.notes.push(format!(
                "xref {}: -> {} ({})",
                reference.ordinal, reference.relative_path, property_note
            )),
        }
    }
    Ok(true)
}

struct XrefPropertyNote<'a>(&'a crate::records::xref::XrefReference);

impl std::fmt::Display for XrefPropertyNote<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "neutronRole {}", self.0.neutron_role)?;
        if !self.0.neutron_data.is_empty() && self.0.neutron_data != self.0.neutron_role.as_str() {
            write!(formatter, ", neutronData {}", self.0.neutron_data)?;
        }
        Ok(())
    }
}

/// A decoded member whose authored source remains available to archive composition.
pub(crate) struct AuthoredDecoded {
    pub(crate) operation_poses:
        std::collections::BTreeMap<String, crate::xref::operation_pose::OperationPose>,
    pub(crate) ir: CadIr,
    pub(crate) source: cadmpeg_ir::SourceMeta,
    pub(crate) body: DecodeBody,
    pub(crate) source_fidelity: cadmpeg_ir::SourceFidelity,
}

impl AuthoredDecoded {
    pub(crate) fn into_decoded(mut self) -> Decoded {
        self.ir.source = Some(self.source);
        Decoded {
            ir: self.ir,
            body: self.body,
            source_fidelity: self.source_fidelity,
        }
    }
}

struct RetainedArtifacts {
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
    source_image: UnknownRecord,
    source_attributes: std::collections::BTreeMap<String, String>,
}

fn decode_result(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    report_scope: crate::report::ReportScope,
    mut ir: CadIr,
    mut report: DecodeBody,
    retained: RetainedArtifacts,
    admitted_entities: &mut u64,
) -> Result<AuthoredDecoded, CodecError> {
    // ASM transfer already charged its delta; admit any remaining neutral entities
    // (sketches, appearances, products) before finalizing.
    ctx.admit_entities(
        u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit F3D entities",
    )?;
    report.losses.extend(saved_free_edge_bodies_loss(&ir.model));
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(retained.annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "f3d", retained.unknowns, ctx)?;
    source_fidelity.retain_unknown_records("f3d", [retained.source_image])?;
    let mut source = crate::report::classify_document(
        scan,
        report_scope,
        retained.source_attributes,
        &mut report,
    )?;
    // Stamped on the finalized, classified document, so the write path
    // compares against the exact document the sealed wrapper returns.
    ir.finalize();
    let hash = cadmpeg_ir::hash::document_local_sha256(
        ctx,
        &ir,
        Some(&source),
        "f3d",
        crate::ids::FILE_SOURCE_IMAGE_ID,
    )?;
    source.attributes.insert(
        cadmpeg_core::nonblank_const!(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
        hash,
    );
    Ok(AuthoredDecoded {
        operation_poses: std::collections::BTreeMap::new(),
        ir,
        source,
        body: report,
        source_fidelity,
    })
}

pub(crate) fn preserve_source_image(scan: &ContainerScan) -> UnknownRecord {
    UnknownRecord::retained(
        crate::ids::file_source_image_id(),
        0,
        scan.source_image.to_vec(),
        Vec::new(),
    )
}

/// Machine-local `document_local_sha256` for the F3D write-path edit oracle.
///
/// See [`cadmpeg_ir::hash::document_local_sha256`].
pub(crate) fn document_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let digest = cadmpeg_ir::hash::document_local_sha256(
        &ctx,
        ir,
        ir.source.as_ref(),
        "f3d",
        crate::ids::FILE_SOURCE_IMAGE_ID,
    )?;
    ctx.finish_session()?;
    Ok(digest)
}

fn annotation_stream(entry_name: &str) -> Result<StreamHandle, CodecError> {
    let name = crate::ids::native_scope(entry_name);
    let name = cadmpeg_ir::StreamName::try_from(name).map_err(CodecError::malformed)?;
    Ok(StreamHandle::new(name))
}

fn note_native_annotation(
    annotations: &mut AnnotationBuilder,
    stream: &StreamHandle,
    id: &str,
    tag: &str,
) {
    annotations.note(id, stream, trailing_offset(id), Some(tag));
}

fn populate_annotations(
    ir: &CadIr,
    scan: &ContainerScan,
    native: &F3dNative,
    brep: Option<(&str, &[cadmpeg_asm::brep::annotations::AnnotationRecord])>,
    unknowns: &[UnknownRecord],
) -> Result<cadmpeg_ir::Annotations, cadmpeg_core::CodecError> {
    use std::collections::HashMap;

    let mut annotations = AnnotationBuilder::new();
    if let Some((stream_name, records)) = brep {
        let stream = annotation_stream(stream_name)?;
        for record in records {
            annotations.note(
                &record.id,
                &stream,
                record.offset,
                Some(record.tag.as_str()),
            );
            for field in &record.derived_fields {
                annotations.derived(&record.id, field)?;
            }
        }
    }

    let mut constraints_by_native = HashMap::new();
    for constraint in &ir.model.sketch_constraints {
        if let Some(native_ref) = constraint.native_ref.as_deref() {
            if !constraints_by_native.contains_key(native_ref) {
                constraints_by_native.insert(native_ref, constraint.id.as_str());
            }
        }
    }
    let mut entities_by_native = HashMap::new();
    for entity in &ir.model.sketch_entities {
        if let Some(native_ref) = entity.native_ref.as_deref() {
            if !entities_by_native.contains_key(native_ref) {
                entities_by_native.insert(native_ref, entity.id().as_str());
            }
        }
    }
    let planar_sketches = ir
        .model
        .sketches
        .iter()
        .map(|sketch| sketch.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let spatial_sketches = ir
        .model
        .spatial_sketches
        .iter()
        .map(|sketch| sketch.id.as_str())
        .collect::<std::collections::HashSet<_>>();

    let native_stream = StreamHandle::new(cadmpeg_ir::stream_name!("f3d:native"));
    macro_rules! note {
        ($id:expr, $tag:expr $(,)?) => {{
            note_native_annotation(&mut annotations, &native_stream, $id, $tag);
        }};
    }
    {
        for entity in &native.construction_recipes {
            note!(&entity.id, "construction_recipe");
        }
        for entity in &native.persistent_references {
            note!(&entity.id, "persistent_reference");
        }
        for entity in &native.lost_edge_references {
            note!(&entity.id, "EDGE_REFERENCE_LOST");
        }
        for entity in &native.design_types {
            note!(entity.id(), "design_type");
        }
        for entity in &native.design_parameters {
            note!(&entity.id, "design_parameter");
        }
        for entity in &native.design_parameter_companions {
            note!(entity.id(), "design_parameter_companion");
        }
        for entity in &native.design_dimension_locus_pairs {
            note!(&entity.id, "design_dimension_locus_pair");
            if let Some(projected) = constraints_by_native.get(entity.id.as_str()) {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_annotation_frames {
            note!(&entity.id, "design_dimension_annotation_frame");
            if let Some(projected) = constraints_by_native.get(entity.id.as_str()) {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_presentation_frames {
            note!(&entity.id, "design_dimension_presentation_frame");
            let projected = native
                .design_parameter_companions
                .iter()
                .find_map(|companion| {
                    (crate::ids::native_stream(companion.id())
                        == crate::ids::native_stream(&entity.id)
                        && companion.record_index() == entity.governing_companion_record_index)
                        .then(|| constraints_by_native.get(companion.id()))
                        .flatten()
                });
            if let Some(projected) = projected {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_locus_groups {
            note!(&entity.id, "design_dimension_locus_group");
            if let Some(projected) = constraints_by_native.get(entity.id.as_str()) {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_null_locus_pairs {
            note!(&entity.id, "design_dimension_null_locus_pair");
            if let Some(projected) = constraints_by_native.get(entity.id.as_str()) {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_parameter_owners {
            note!(entity.id(), "design_parameter_owner");
        }
        for entity in &native.design_parameter_scopes {
            note!(&entity.id, "design_parameter_scope");
        }
        for entity in &native.design_edge_operands {
            note!(&entity.id, "design_edge_operand");
        }
        for entity in &native.design_face_operands {
            note!(&entity.id, "design_face_operand");
        }
        for entity in &native.design_face_source_groups {
            note!(&entity.id, "design_face_source_group");
        }
        for entity in &native.design_sketch_placements {
            note!(&entity.id, "design_sketch_placement");
            let planar = crate::ids::neutral_sketch_id(entity);
            if planar_sketches.contains(planar.as_str()) {
                note!(planar.as_str(), "sketch");
            }
            let spatial = crate::ids::neutral_spatial_sketch_id(entity);
            if spatial_sketches.contains(spatial.as_str()) {
                note!(spatial.as_str(), "spatial_sketch");
            }
        }
        for entity in &native.design_entity_headers {
            note!(&entity.id, "design_entity_header");
        }
        for entity in &native.design_record_headers {
            note!(&entity.id, "design_record_header");
        }
        for entity in &native.design_body_members {
            note!(entity.id(), "BodiesRoot");
        }
        for entity in &native.design_material_assignments {
            note!(&entity.id, "material_assignment");
        }
        for entity in &native.sketch_relations {
            note!(&entity.id, "sketch_relation");
            if constraints_by_native.contains_key(entity.id.as_str()) {
                let constraint =
                    crate::ids::neutral_sketch_constraint_id(&entity.id, entity.record_index);
                note!(constraint.as_str(), "sketch_constraint");
            }
        }
        for entity in &native.sketch_points {
            note!(&entity.id, "sketch_point");
            if let Some(projected) = entities_by_native.get(entity.id.as_str()) {
                note!(projected, "sketch_entity");
            }
        }
        for entity in &native.sketch_curve_identities {
            note!(&entity.id, "sketch_curve");
            if let Some(projected) = entities_by_native.get(entity.id.as_str()) {
                note!(projected, "sketch_entity");
                // Admitted z noise is read as zero, so the projected
                // endpoints are derived rather than byte-exact.
                if entity.planar_z_noise.is_some() {
                    for field in ["geometry.start", "geometry.end"] {
                        annotations.derived(projected, field)?;
                    }
                }
            }
        }
        for entity in &native.sketch_surfaces {
            note!(&entity.id, "sketch_surface");
        }
        for entity in &native.sketch_curve_links {
            note!(&entity.id, "sketch_curve_link");
        }
        for entity in &native.persistent_design_links {
            note!(&entity.id, "persistent_design_link");
        }
        for entity in &native.persistent_subentity_tags {
            note!(&entity.id, "persistent_subentity_tag");
        }
        for entity in &native.act_entities {
            note!(entity.id(), "ACTEntity");
        }
        for entity in &native.act_guids {
            note!(entity.id(), "ACTGuid");
        }
        for entity in &native.act_registry_channels {
            note!(entity.id(), "ACTRegistryChannel");
        }
        for entity in &native.act_component_links {
            note!(entity.id(), "ACTComponentLink");
        }
        for entity in &native.act_table_references {
            note!(entity.id(), "ACTTableReference");
        }
        for history in &native.asm_histories {
            note!(&history.id, "history_stream");
            for state in history.states() {
                note!(&state.id, "delta_state");
                for board in &state.bulletin_boards {
                    note!(&board.id, "BulletinBoard");
                    for change in &board.changes {
                        note!(&change.id, "entity_change");
                    }
                }
                for record in &state.records {
                    note!(&record.id, record.name());
                }
            }
        }
    }

    let appearance_stream = scan
        .entries
        .iter()
        .find(|entry| scan.is_design_asset_entry(entry, ContainerRole::ProteinAssets))
        .map(|entry| annotation_stream(&entry.name))
        .transpose()?;
    if let Some(stream) = appearance_stream {
        for appearance in &ir.model.appearances {
            annotations.note(
                appearance.id.as_str(),
                &stream,
                0,
                Some(appearance.schema.as_deref().unwrap_or("appearance")),
            );
        }
    }
    for binding in &ir.model.appearance_bindings {
        annotations.note(
            binding.id.as_str(),
            &native_stream,
            0,
            Some("appearance_binding"),
        );
    }
    if brep.is_none() {
        if let Some(fallback) = container::select_fallback_brep(scan) {
            let stream = annotation_stream(&fallback.name)?;
            for unknown in unknowns {
                annotations.note(
                    unknown.id().as_str(),
                    &stream,
                    unknown.offset(),
                    Some("opaque_brep"),
                );
            }
        }
    }
    Ok(annotations.build())
}

fn trailing_offset(id: &str) -> u64 {
    id.rsplit(':')
        .find_map(|part| part.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Frame and decode one Design-selected or explicit fallback BREP SAB stream.
///
/// Return `None` when the header or record framing cannot be read.
fn try_decode_brep(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    brep_entry: &BrepFacts,
) -> Result<Option<Brep>, CodecError> {
    let Some(crate::container::KernelFraming::Asm {
        header,
        solved_record_limit,
    }) = &brep_entry.kernel
    else {
        return Ok(None);
    };
    let width = header.width;

    let bytes = scan.entry_bytes(&brep_entry.name)?;
    let Some(start) = asm_header::record_stream_start(bytes) else {
        return Ok(None);
    };
    // A stream without a delta-state boundary is history-less: its final
    // `End-of-ASM-data` record ends at EOF without the `0x11` terminator, so
    // it needs the EOF-tolerant framer used for the history partition.
    let framed = match *solved_record_limit {
        Some(limit) => sab::frame(ctx, bytes, start, limit, width, None),
        None => sab::frame_history(ctx, bytes, start, bytes.len(), width, None),
    };
    let records = match framed {
        Ok(r) if !r.is_empty() => r,
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => return Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            return Err(error.into_codec_error())
        }
        _ => return Ok(None),
    };

    brep::decode(
        ctx,
        &records,
        bytes,
        &brep_entry.name,
        crate::ids::ID_FORMAT,
    )
    .map(Some)
}

/// Assemble the IR document from the decoded B-rep graph.
fn build_geometry_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    primary_model_brep: &BrepFacts,
    brep: Brep,
) -> Result<
    (
        CadIr,
        std::collections::BTreeMap<String, String>,
        F3dNative,
        AsmTransferRemainder,
    ),
    CodecError,
> {
    let mut ir = CadIr::empty();
    let (source_attributes, tolerances) =
        source_attributes_and_tolerances(scan, primary_model_brep)?;
    ir.tolerances = tolerances;
    let Brep {
        asm,
        sketch_curve_links,
        persistent_design_links,
        persistent_subentity_tags,
        creation_timestamps,
        sketch_region_bodies_omitted: _,
    } = brep;
    let (namespace, remainder) = transfer_into_ir(ctx, &mut ir, "f3d", asm)?;
    let mut native = F3dNative::load_inner(ctx, namespace)?;
    native.sketch_curve_links = sketch_curve_links;
    native.persistent_design_links = persistent_design_links;
    native.persistent_subentity_tags = persistent_subentity_tags;
    native.creation_timestamps = creation_timestamps;
    Ok((ir, source_attributes, native, remainder))
}

/// Smallest linear tolerance, in millimeters, that the analytic sketch, profile
/// and region comparisons in this crate act on.
const MIN_ANALYTIC_LINEAR_TOLERANCE_MM: f64 = 1.0e-7;

/// Admit each optional kernel tolerance independently. An unrepresentable value
/// leaves the document default; reports diagnose it from the retained header.
/// A valid `resabs` below the analytic floor remains unsupported because it
/// cannot drive profile and region matching at the declared precision.
fn admit_kernel_tolerances(
    resabs: Option<f64>,
    resnor: Option<f64>,
) -> Result<Tolerances, CodecError> {
    let mut tolerances = Tolerances::default();
    if let Some(linear) =
        resabs.and_then(|value| cadmpeg_ir::scalar::PositiveLength::new(value * 10.0))
    {
        if linear.get() < MIN_ANALYTIC_LINEAR_TOLERANCE_MM {
            return Err(CodecError::NotImplemented(format!(
                "kernel header resabs {} mm is below the analytic linear \
                 tolerance floor {MIN_ANALYTIC_LINEAR_TOLERANCE_MM} mm",
                linear.get()
            )));
        }
        tolerances.linear = linear;
    }
    if let Some(angular) = resnor.and_then(cadmpeg_ir::scalar::PositiveAngle::new) {
        tolerances.angular = angular;
    }
    Ok(tolerances)
}

/// Source metadata attributes and kernel tolerances from the primary model BREP header.
fn source_attributes_and_tolerances(
    scan: &ContainerScan,
    primary_model_brep: &BrepFacts,
) -> Result<(std::collections::BTreeMap<String, String>, Tolerances), CodecError> {
    let mut attributes = std::collections::BTreeMap::new();
    if let Some(folder) = scan.design_asset_folder() {
        {
            let copy = folder.to_owned();
            attributes.insert("asset_folder".to_owned(), copy);
        }
    }
    attributes.insert("zip_entry_count".to_owned(), scan.entries.len().to_string());
    {
        let copy = primary_model_brep.name.clone();
        attributes.insert("active_brep".to_owned(), copy);
    }
    {
        let copy = primary_model_brep.sha256.as_str().to_owned();
        attributes.insert("active_brep_sha256".to_owned(), copy);
    }
    if let Some(off) = primary_model_brep
        .kernel
        .as_ref()
        .and_then(crate::container::KernelFraming::solved_record_limit)
    {
        attributes.insert("solved_record_len".to_owned(), off.to_string());
    }
    if let Some(unit) = crate::design::decode::units::decode_document_length_unit(scan) {
        attributes.insert("modeling_length_unit".to_owned(), unit);
    }

    let mut tolerances = Tolerances::default();
    if let Some(h) = primary_model_brep
        .kernel
        .as_ref()
        .and_then(crate::container::KernelFraming::model_metadata)
    {
        if let Some(pf) = &h.product_family {
            {
                let copy = pf.to_owned();
                attributes.insert("product_family".to_owned(), copy);
            }
        }
        if let Some(pv) = &h.product_version {
            {
                let copy = pv.to_owned();
                attributes.insert("product_version".to_owned(), copy);
            }
        }
        if let Some(sd) = &h.save_date {
            {
                let copy = sd.to_owned();
                attributes.insert("save_date".to_owned(), copy);
            }
        }
        tolerances = admit_kernel_tolerances(h.linear, h.angular)?;
    }

    Ok((attributes, tolerances))
}

/// Loss report for a successful geometry decode.
struct KindCounts<'a>(&'a std::collections::BTreeMap<String, usize>);

impl std::fmt::Display for KindCounts<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (name, count)) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{name}={count}")?;
        }
        Ok(())
    }
}

fn geometry_losses(decoded: &Brep) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let s = &decoded.asm.stats;
    let mut losses = Vec::new();

    if s.nurbs_surfaces > 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::NurbsSurfaceCarrier,
            format_args!(
                "{} spline surface record(s) were decoded into NURBS carriers from their inline \
             cached B-spline block.",
                s.nurbs_surfaces
            ),
        );
    }
    if s.nurbs_curves > 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::NurbsCurveCarrier,
            format_args!(
                "{} procedural curve record(s) were decoded into NURBS carriers from their inline \
             cached 3D B-spline block.",
                s.nurbs_curves
            ),
        );
    }
    if decoded.sketch_region_bodies_omitted > 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::SketchRegionBodyOmitted,
            format_args!(
                "{} sketch-region body record(s) of a keyless BREP blob are not model bodies and stay in the BREP stream.",
                decoded.sketch_region_bodies_omitted
            ),
        );
    }
    if s.missing_face_surfaces() > 0 {
        push_loss_vec(&mut losses, F3dLossCode::FaceSurfaceReferenceDangling, format_args!(
            "{} face(s) were omitted because their required surface reference was null or dangling. Reference conditions: {}.",
            s.missing_face_surfaces(),
            KindCounts(&s.missing_face_surface_kinds)
        ),);
    }
    if s.unknown_surface_faces() > 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::SurfaceShapeNotDecoded,
            format_args!(
                "{} face(s) rest on spline/procedural surfaces whose shape was not decoded into a \
             typed carrier (no inline cached B-spline block: the cache is reached through a \
             subtype reference, or the record is a procedural form this codec does not \
             evaluate); the face, its loops, and trims are emitted with an unknown-geometry \
             surface linking to the preserved record bytes. Topology is transferred; the \
             underlying surface shape is not. Native kinds: {}.",
                s.unknown_surface_faces(),
                KindCounts(&s.unknown_surface_kinds)
            ),
        );
    }
    if s.mesh_surface_faces > 0 {
        push_loss_vec(&mut losses, F3dLossCode::MeshSurfaceSentinel, format_args!(
            "{} face(s) use zero-payload mesh_surface sentinels. Their exact surfaces are absent by definition; the emitted unknown surface preserves that distinction from tessellation attributes.",
            s.mesh_surface_faces
        ),);
    }
    if s.procedural_curve_edges() > 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::ProceduralCurveUndecoded,
            format_args!(
            "{} edge(s) reference a procedural intcurve/spline 3D curve with no decodable inline \
             B-spline cache; the edge was emitted with its vertices and parameter range but no \
             attributed curve carrier. Native kinds: {}.",
            s.procedural_curve_edges(),
            KindCounts(&s.procedural_curve_kinds)
        ),
        );
    }
    if s.undecoded_pcurve_refs() > 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::PcurveUndecoded,
            format_args!(
                "{} coedge(s) carry an explicit UV pcurve reference with no decodable 2D \
             carrier on the face surface's parameterization; those coedges were emitted \
             without a pcurve. Native kinds: {}.",
                s.undecoded_pcurve_refs(),
                KindCounts(&s.undecoded_pcurve_kinds)
            ),
        );
    }
    if s.partial_procedural_supports > 0 {
        push_loss_vec(&mut losses, F3dLossCode::BlendSupportPartial, format_args!(
            "{} rolling-ball blend definition(s) retain their signed radius and solved cache, but only one of two native supports resolved.",
            s.partial_procedural_supports
        ),);
    }
    if s.other_records() > 0 {
        push_loss_vec(
            &mut losses,
            F3dLossCode::SolvedRecordUntyped,
            format_args!(
                "{} solved-record application/refinement record(s) were not transferred: {}.",
                s.other_records(),
                KindCounts(&s.other_record_kinds)
            ),
        );
    }
    push_loss_vec(
        &mut losses,
        F3dLossCode::MaterialNotTransferred,
        format_args!(
            "Materials/appearances (.protein assets, ACT/design assignments) were not \
         transferred."
        ),
    );
    losses
}

struct MetadataIr {
    ir: CadIr,
    source_attributes: std::collections::BTreeMap<String, String>,
    unknowns: Vec<UnknownRecord>,
}

fn append_metadata_unknown(
    unknowns: &mut Vec<UnknownRecord>,
    brep: &BrepFacts,
) -> Result<(), CodecError> {
    let id = UnknownId::mint(crate::ids::native_scoped_id(&brep.name, "unknown", 0_u64)).map_err(
        |error| {
            CodecError::malformed(format_args!(
                "F3D BREP name cannot form an unknown-record identity: {error}"
            ))
        },
    )?;
    let digest = brep.sha256.as_str().to_owned();
    unknowns.push(UnknownRecord::unavailable(
        id,
        0,
        brep.uncompressed_len,
        digest,
        Vec::new(),
    ));
    Ok(())
}

fn build_metadata_ir(scan: &ContainerScan) -> Result<MetadataIr, CodecError> {
    let mut ir = CadIr::empty();
    let mut unknowns = Vec::new();

    let mut attributes = std::collections::BTreeMap::new();
    if let Some(folder) = scan.design_asset_folder() {
        {
            let copy = folder.to_owned();
            attributes.insert("asset_folder".to_owned(), copy);
        }
    }
    attributes.insert("zip_entry_count".to_owned(), scan.entries.len().to_string());
    if let Some(unit) = crate::design::decode::units::decode_document_length_unit(scan) {
        attributes.insert("modeling_length_unit".to_owned(), unit);
    }

    if let Some(brep) = container::select_fallback_brep(scan) {
        {
            let copy = brep.name.clone();
            attributes.insert("active_brep".to_owned(), copy);
        }
        {
            let copy = brep.sha256.as_str().to_owned();
            attributes.insert("active_brep_sha256".to_owned(), copy);
        }
        if let Some(off) = brep
            .kernel
            .as_ref()
            .and_then(crate::container::KernelFraming::solved_record_limit)
        {
            attributes.insert("solved_record_len".to_owned(), off.to_string());
        }
        if let Some(h) = brep
            .kernel
            .as_ref()
            .and_then(crate::container::KernelFraming::model_metadata)
        {
            if let Some(pf) = &h.product_family {
                {
                    let copy = pf.to_owned();
                    attributes.insert("product_family".to_owned(), copy);
                }
            }
            if let Some(pv) = &h.product_version {
                {
                    let copy = pv.to_owned();
                    attributes.insert("product_version".to_owned(), copy);
                }
            }
            if let Some(sd) = &h.save_date {
                {
                    let copy = sd.to_owned();
                    attributes.insert("save_date".to_owned(), copy);
                }
            }
            ir.tolerances = admit_kernel_tolerances(h.linear, h.angular)?;
        }

        append_metadata_unknown(&mut unknowns, brep)?;
    }

    Ok(MetadataIr {
        ir,
        source_attributes: attributes,
        unknowns,
    })
}

enum ModelBrepOutcome {
    NotAttempted,
    NoPhysicalModel,
    Failed { stream: String },
    Empty { stream: String },
}

/// Report the selected model carrier and its decode outcome.
fn container_losses(
    scan: &ContainerScan,
    outcome: &ModelBrepOutcome,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let brep_count = container::design_breps(scan).count();
    let selected = container::select_fallback_brep(scan);
    let text_count = container::text_brep_names(scan).count();

    let (geometry, topology) = match outcome {
        ModelBrepOutcome::NoPhysicalModel => (
            "No physical model BREP was selected: the modern model-body maps are explicitly empty or name only bodies outside the current model. Snapshot streams contain no selected physical model bodies.".to_owned(),
            "No physical B-rep topology was selected by the model-body maps.".to_owned(),
        ),
        ModelBrepOutcome::Empty { stream } => (
            format!("The selected model BREP stream `{stream}` decoded successfully; the selected model has no surfaces, faces, or points."),
            format!("The selected model BREP stream `{stream}` decoded successfully; no B-rep topology was transferred."),
        ),
        ModelBrepOutcome::Failed { stream } => (
            format!("ASM BREP geometry was not transferred: the selected stream `{stream}` is not a decodable BinaryFile4/BinaryFile8 SAB (or its framing failed)."),
            format!("B-rep topology was not built for the selected stream `{stream}`."),
        ),
        ModelBrepOutcome::NotAttempted => match (brep_count, selected) {
        // The text carrier is present but its decode produced no geometry.
        (0, _) if text_count != 0 => {
            let text_names = join_text_brep_names(scan);
            (
                format!("ASM BREP geometry was not transferred: the document's only geometry carrier is \
                 the text-encoded ASM stream(s) `{text_names}`, and their decode produced no surfaces, \
                 curves, or points."),
                format!("B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not \
                 built from the text-encoded carrier(s) `{text_names}`."),
            )
        }
        (0, _) => (
            "ASM BREP geometry was not transferred: the container declares no ASM BREP stream, so \
             no surfaces, curves, or points were produced."
                .to_string(),
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built: \
             the container declares no ASM BREP stream."
                .to_string(),
        ),
        (_, Some(brep)) => (
            format!("ASM BREP geometry was not transferred: the container stream `{}` was not decoded.", brep.name),
            format!("B-rep topology was not built for the container stream `{}`.", brep.name),
        ),
        (_, None) => (
            format!(
                "ASM BREP geometry was not transferred: {brep_count} BREP stream(s) were located, \
                 but none of them is the document's geometry stream. The Design body map that \
                 binds a body to its blob was not read, so the selection is ambiguous."
            ),
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built: \
             no BREP stream was selected."
                .to_string(),
        ),
        },
    };

    let mut losses = vec![
        F3dLossCode::GeometryNotTransferred.note(geometry),
        F3dLossCode::TopologyNotTransferred.note(topology),
        F3dLossCode::MaterialNotTransferred.note(
            "Materials/appearances (.protein assets, ACT/design assignments) were not \
             transferred.",
        ),
    ];

    // An absent carrier and an unselectable carrier produce different findings.
    // Full decode rejects an ambiguous selection before it builds this report.
    if selected.is_none() && matches!(outcome, ModelBrepOutcome::NotAttempted) {
        losses.push(F3dLossCode::MissingGeometryStream.note(
            if brep_count == 0 && text_count != 0 {
                format!(
                    "{text_count} ASM BREP stream(s) are present in the text encoding (.sat/.smt) and \
                     produced no geometry; no binary stream (.smb/.smbh) was found"
                )
            } else if brep_count == 0 {
                "no ASM BREP stream (.smb/.smbh) was found in the container".to_string()
            } else {
                format!(
                    "{brep_count} ASM BREP stream(s) are present, but none of them was selected as \
                     the document's geometry stream"
                )
            },
        ));
    }

    losses
}

/// Resolve the appearance loss note against the appearances in the IR.
///
/// The report adds this note before appearance decoding. This function removes
/// it when the IR carries a complete document-local catalog and keeps it when a
/// serialized assignment failed to resolve.
pub(crate) fn reconcile_appearance_loss(
    report: &mut DecodeBody,
    ir: &CadIr,
    has_topology_assignments: bool,
) {
    if ir.model.appearances.is_empty() {
        return;
    }
    if has_topology_assignments && ir.model.appearance_bindings.is_empty() {
        if let Some(loss) = report
            .losses
            .iter_mut()
            .find(|loss| loss.code.category() == LossCategory::Material)
        {
            loss.message = format!(
                "{} Protein appearance asset(s) were decoded, but no topology assignment was resolved.",
                ir.model.appearances.len()
            );
        }
        return;
    }
    report
        .losses
        .retain(|loss| loss.code.category() != LossCategory::Material);
}

/// Join per-face appearance assignments to BREP faces through the face GUID
/// carried by each face's `NEUTRON_Material_attrib_def` attribute
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
pub(crate) fn resolve_face_appearance_bindings(
    ir: &mut CadIr,
    face_assignments: &[materials::FaceAppearanceAssignment],
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use std::collections::btree_map::Entry;

    struct Assignment<'a> {
        visual_guid: Option<&'a crate::records::references::DesignVisualToken>,
        color: Option<cadmpeg_ir::topology::Color>,
    }

    if face_assignments.is_empty() {
        return Ok(Vec::new());
    }

    let mut assignments_by_guid = std::collections::BTreeMap::new();
    for assignment in face_assignments {
        match assignments_by_guid.entry(assignment.face_guid.as_str()) {
            Entry::Vacant(entry) => {
                entry.insert(Assignment {
                    visual_guid: Some(&assignment.visual_guid),
                    color: assignment.color,
                });
            }
            Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                if existing
                    .visual_guid
                    .is_some_and(|token| !token.matches(&assignment.visual_guid))
                {
                    existing.visual_guid = None;
                }
                match (existing.color, assignment.color) {
                    (Some(left), Some(right)) if left != right => {
                        return Err(CodecError::malformed(format_args!(
                            "F3D face material GUID {} carries conflicting neutral colors",
                            assignment.face_guid
                        )));
                    }
                    (None, Some(color)) => existing.color = Some(color),
                    _ => {}
                }
            }
        }
    }

    let mut faces_by_guid =
        std::collections::BTreeMap::<&str, Vec<&cadmpeg_ir::ids::FaceId>>::new();
    let mut guid_by_face = std::collections::BTreeMap::<&cadmpeg_ir::ids::FaceId, &str>::new();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(face) = &attribute.target else {
            continue;
        };
        let strings = || {
            attribute.values.iter().filter_map(|value| match value {
                AttributeValue::String(value) => Some(value.as_str()),
                _ => None,
            })
        };
        let material_name_count = strings()
            .filter(|value| *value == "NEUTRON_Material_attrib_def")
            .count();
        if material_name_count == 0 {
            continue;
        }
        if material_name_count != 1 {
            return Err(CodecError::Malformed(
                "F3D face material attribute repeats its attribute-definition name".into(),
            ));
        }
        let mut face_guids = strings().filter(|value| {
            crate::bytes::is_guid_hyphenated(value)
                && value.bytes().all(|byte| !byte.is_ascii_uppercase())
        });
        let Some(face_guid) = face_guids.next() else {
            return Err(CodecError::Malformed(
                "F3D face material attribute does not carry exactly one lower-case face GUID"
                    .into(),
            ));
        };
        if face_guids.next().is_some() {
            return Err(CodecError::Malformed(
                "F3D face material attribute does not carry exactly one lower-case face GUID"
                    .into(),
            ));
        }
        if let Some(previous) = guid_by_face.insert(face, face_guid) {
            if previous != face_guid {
                return Err(CodecError::malformed(format_args!(
                    "F3D face {face} carries multiple material GUIDs"
                )));
            }
        }

        let faces = match faces_by_guid.entry(face_guid) {
            Entry::Vacant(entry) => entry.insert(Vec::new()),
            Entry::Occupied(entry) => entry.into_mut(),
        };
        faces.push(face);
    }
    for faces in faces_by_guid.values_mut() {
        faces.sort_by(Ord::cmp);
        faces.dedup();
    }
    let mut bound_faces = ir
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| {
            let AppearanceTarget::Face(face) = &binding.target else {
                return None;
            };
            Some((face, &binding.appearance))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut face_indices = std::collections::HashMap::new();
    for (index, face) in ir.model.faces.iter().enumerate() {
        let id = face.id.as_str().to_owned();
        face_indices.insert(id, index);
    }
    let mut new_bindings = Vec::new();
    let mut losses = Vec::new();
    for (face_guid, assignment) in assignments_by_guid {
        let Some(faces) = faces_by_guid.get(face_guid) else {
            continue;
        };
        let appearance = if let Some(token) = assignment.visual_guid {
            materials::appearance_for_visual_token(&ir.model.appearances, token, None)?
        } else {
            losses.push(F3dLossCode::FaceAppearanceAmbiguous.note(format!(
                "{} face(s) share a material GUID with distinct visual tokens; \
                 the face appearance was not selected. Assignment records remain in the source image.",
                faces.len()
            )));
            None
        };
        for face in faces {
            if let Some(color) = assignment.color {
                if let Some(index) = face_indices.get(face.as_str()).copied() {
                    let target = &mut ir.model.faces[index];
                    if target.color.is_none() {
                        target.color = Some(color);
                    }
                }
            }
            let Some(appearance) = appearance.as_ref() else {
                continue;
            };
            let Some(visual_guid) = assignment.visual_guid else {
                continue;
            };
            if let Some(existing) = bound_faces.get(*face) {
                if *existing != &appearance.id {
                    return Err(CodecError::malformed(format_args!(
                        "F3D face {face} carries conflicting appearance assignments"
                    )));
                }
                continue;
            }

            bound_faces.insert(*face, &appearance.id);
            let target = AppearanceTarget::Face((*face).clone());
            let appearance_id = appearance.id.clone();
            let id =
                crate::ids::face_appearance_binding_id(face_guid, visual_guid.identity_key(), face)
                    .map_err(|error| {
                        CodecError::malformed(format_args!(
                            "F3D face appearance binding identity is invalid: {error}"
                        ))
                    })?;
            new_bindings.push(AppearanceBinding {
                // The face id completes the key: one appearance attribute GUID
                // reaches every face carrying it, so the assignment pair alone
                // repeats across those faces.
                id,
                target,
                appearance: appearance_id,
                source_entity_id: None,
                object_type: None,
                visible: None,
                channels: std::collections::BTreeMap::new(),
            });
        }
    }
    drop(bound_faces);
    ir.model.appearance_bindings.extend(new_bindings);
    Ok(losses)
}

/// Fill absent explicit topology colors from uniquely bound appearance assets.
/// Native RGB/truecolor attributes remain authoritative on the same target.
fn insert_appearance_color<'a, K: Eq + std::hash::Hash>(
    colors: &mut std::collections::HashMap<&'a K, Option<cadmpeg_ir::topology::Color>>,
    id: &'a K,
    color: cadmpeg_ir::topology::Color,
) {
    if let Some(existing) = colors.get_mut(id) {
        *existing = None;
    } else {
        colors.insert(id, Some(color));
    }
}

fn apply_appearance_base_colors(ir: &mut CadIr) {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let colors = ir
        .model
        .appearances
        .iter()
        .filter_map(|appearance| Some((&appearance.id, appearance.base_color?)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut body_colors = std::collections::HashMap::new();
    let mut face_colors = std::collections::HashMap::new();
    for binding in &ir.model.appearance_bindings {
        let Some(color) = colors.get(&binding.appearance).copied() else {
            continue;
        };
        match &binding.target {
            AppearanceTarget::Body(id) => insert_appearance_color(&mut body_colors, id, color),
            AppearanceTarget::Face(id) => insert_appearance_color(&mut face_colors, id, color),
            _ => {}
        }
    }
    for body in &mut ir.model.bodies {
        if body.color.is_none() {
            body.color = body_colors.get(&body.id).copied().flatten();
        }
    }
    for face in &mut ir.model.faces {
        if face.color.is_none() {
            face.color = face_colors.get(&face.id).copied().flatten();
        }
    }
}

pub(crate) fn is_default_body_name(name: &str) -> bool {
    name == "Body"
        || name
            .strip_prefix("Body")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

pub(crate) fn apply_body_presentation_names(
    ir: &mut CadIr,
    scan: &ContainerScan,
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
) -> Result<(), CodecError> {
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let presentations =
            crate::design::decode::presentation::body_presentations(bytes, &metadata)?;
        for presentation in presentations {
            let Some(name) = presentation.name.as_deref() else {
                continue;
            };
            if is_default_body_name(name) {
                continue;
            }
            let mut matching = body_bindings.iter().filter(|b| {
                b.stream() == entry.name && b.entity_suffix == presentation.entity_suffix
            });
            let (Some(binding), None) = (matching.next(), matching.next()) else {
                continue;
            };
            if let Some(body_id) = &binding.body {
                if let Some(body) = ir.model.bodies.iter_mut().find(|b| b.id == *body_id) {
                    if body.name.is_none() {
                        body.name = Some(name.to_string());
                    }
                }
            }
        }
    }
    Ok(())
}

pub(crate) mod design_intent;
mod geometry_admission;
mod prereqs;

#[cfg(test)]
mod tests;
