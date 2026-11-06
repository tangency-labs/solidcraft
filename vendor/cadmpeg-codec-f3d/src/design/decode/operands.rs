// SPDX-License-Identifier: Apache-2.0
//! Parse edge, face, and body operand frames and recipe structure.

mod fillet_variable_groups;

use crate::bytes::lp_utf16_bounded;
use crate::records::topology::{
    construction::DesignConstructionOperandRole, extrude_selection::DesignExtrudeFaceEncoding,
};

use cadmpeg_core::container::ContainerRole;

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{is_guid_relaxed, take_reference};
use crate::container::ContainerScan;
use crate::design::decode::dimension_frames::{
    bind_recipe_reference_candidates, contiguous_i32_program, decode_recipe_references,
    recipe_record_prefix,
};
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_scope;
use crate::design::decode::scopes::parameter_scope::payload_prologue;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::{
    cached_borrowed_record_offsets, cached_owned_record_offsets, indexed_record_header_at,
    next_indexed_record_offset, next_indexed_record_offset_with_index, IndexedRecordOffsets,
};
use crate::design::decode::text::design_record_id;
use crate::design::decode::text::relaxed_guid_end;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::native_stream;
use crate::layout::body_recipe_family_name_frame as body_recipe_end;
use crate::layout::class_338_sketch_curve_identity as class_338_curve;
use crate::layout::coil_compact_face_selection_prefix as coil_face_sel;
use crate::layout::coil_compact_persistent_selection_prefix as coil_persist_sel;
use crate::layout::coil_modern_selection_prefix as coil_modern_sel;
use crate::layout::construction_operand_affine_transform_frame as trailing_affine;
use crate::layout::construction_operand_extended_path_prefix as extended_path;
use crate::layout::extrude_selection_member_fixed_frame as extrude_member;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::layout::legacy_loft_body_carrier_class_322 as legacy_loft_322;
use crate::layout::legacy_loft_body_carrier_class_322_tail as legacy_loft_322_tail;
use crate::layout::legacy_loft_body_carrier_class_411 as legacy_loft_411;
use crate::layout::sheet_metal_region_carrier_prefix as region_carrier;
use crate::layout::sheet_metal_region_carrier_scalar_lane as region_scalar;
use crate::layout::sheet_metal_region_identity_reference as region_reference;
use crate::layout::sketch_profile_region_member as region_member;
use crate::layout::sketch_profile_region_selection_prefix as region_selection;
use crate::layout::typed_reference_extrude_selection_group_prefix as typed_group;
use crate::layout::typed_reference_profile_chain_record as typed_chain;
use crate::layout::typed_reference_profile_identity_record as typed_identity;
use crate::layout::typed_reference_profile_selection_prefix as typed_selection;
use crate::layout::work_point_sketch_point_identity as sketch_point_identity;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::{DesignEntityHeader, SegmentType},
    feature::{
        extrude::{DesignExtrudeExtent, DesignExtrudePrologue, DesignExtrudeStart},
        hole::DesignHoleConstruction,
        scope::DesignParameterScope,
        surface_ops::DesignSurfaceOffsetSupport,
        work_geometry::{
            DesignEdgeTreatmentVertexOperand, DesignVertexRecipe, DesignWorkPlaneConstruction,
            DesignWorkPointInputCarrier, DesignWorkPointPlaneSelection, DesignWorkPointRule,
            DesignWorkPointSketchPointSelection,
        },
    },
    parameters::{DesignParameter, DesignParameterOwner},
    recipes::{ConstructionRecipe, ConstructionRecipeKind},
    references::LostEdgeReference,
    sketch_geometry::{SketchCurveIdentity, SketchPoint},
    sketch_links::PersistentSubentityTag,
    sketch_relations::SketchRelationOperand,
    topology::{
        body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
        body_recipe::DesignOperandOwner, construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandGroupFrame,
        construction::DesignConstructionOperandIdentity,
        construction::DesignConstructionPersistentIdentity,
        construction::DesignConstructionTrackingPath, edge_identity::DesignEdgeIdentityOperand,
        edge_identity::DesignEdgeOperand, edge_recipe::DesignTopologyRecipeEntry,
        edge_recipe::DesignTopologyRecipeSide, edge_recipe::DesignTopologyRecipeTriplet,
        entity_selection::DesignEntitySelectionOperand,
        entity_selection::DesignLoftLegacyBodyCarrier, extrude_selection::DesignExtrudeFaceRole,
        extrude_selection::DesignExtrudeOperandRole,
        extrude_selection::DesignExtrudeSelectionGroup,
        extrude_selection::DesignExtrudeSelectionMember,
        extrude_selection::DesignExtrudeSelectionScopeTail, extrude_selection::DesignOperandRole,
        face::DesignFaceOperand, face::DesignFaceSourceGroup, face::DesignFaceSourceMember,
        fillet::DesignFilletRadiusGroup, fillet::DesignFilletRadiusLaw,
        sketch_profile::DesignSketchProfileOperand, sketch_profile::DesignSketchProfileRegion,
        sketch_profile::DesignSketchProfileRegionMember,
        sketch_profile::DesignSketchProfileRegionSelection,
    },
};
use cadmpeg_core::decode::{bounded_len, index_from_u32, DecodeContext, View, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode edge-recipe operand frames named by edge-selecting feature scopes.
pub(crate) fn decode_edge_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeOperand>, CodecError> {
    let record_headers = headers;
    let mut headers = HashMap::new();
    for header in record_headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        headers.insert(key, header);
    }
    let mut terminal_group_members = HashSet::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(member) = group.members().last() else {
            continue;
        };
        let key = (stream, group.scope_record_index, member.value);
        terminal_group_members.insert(key);
    }
    let mut stream_offsets: HashMap<&str, Vec<u64>> = HashMap::new();
    for header in record_headers {
        if let Some(stream) = native_stream(&header.id) {
            stream_offsets
                .entry(stream)
                .or_default()
                .push(header.byte_offset);
        }
    }
    for offsets in stream_offsets.values_mut() {
        offsets.sort_unstable();
    }
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| has_edge_recipe_operands(&scope.kind()))
    {
        let mut member_indices = HashSet::new();
        for group in groups.iter().filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
        }) {
            for member in group.members() {
                member_indices.insert(member.value);
            }
        }
        if let Some(operation) = scope.surface_extend_operation() {
            for &index in &operation.edge_record_indices {
                member_indices.insert(index);
            }
        }
        if let Some(operation) = scope.surface_offset_operation() {
            if let DesignSurfaceOffsetSupport::BoundaryCarrier {
                edge_record_indices,
                ..
            } = &operation.support
            {
                for &index in edge_record_indices {
                    member_indices.insert(index);
                }
            }
        }
        if let Some(construction) = scope.work_point_construction() {
            for input in construction.rule.inputs() {
                member_indices.insert(input.record_index());
            }
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            if !member_indices.contains(&record_index) {
                continue;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let terminal_group_member =
                terminal_group_members.contains(&(stream, scope.record_index, header.record_index));
            let stream_end = u64::try_from(bytes.len()).map_err(|_| {
                CodecError::malformed("Fusion Design BulkStream exceeds the addressable range")
            })?;
            let terminal_group_limit = terminal_group_member.then(|| {
                stream_offsets
                    .get(stream)
                    .and_then(|offsets| {
                        let at = offsets.partition_point(|offset| *offset <= header.byte_offset);
                        offsets.get(at).copied()
                    })
                    .unwrap_or(stream_end)
            });
            let Some(operand) = parse_edge_operand(
                ctx,
                bytes,
                records,
                scope,
                (ordinal, header),
                recipes,
                terminal_group_limit,
            ) else {
                continue;
            };
            let operand = operand?;

            out.push(operand);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode vertex-recipe members retained inside edge-treatment groups.
pub(crate) fn decode_edge_treatment_vertex_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeTreatmentVertexOperand>, CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| has_edge_recipe_operands(&scope.kind()))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        for (scope_reference_ordinal, record_index) in
            scope.reference_members().values().copied().enumerate()
        {
            let mut matches = groups
                .iter()
                .filter(|group| {
                    native_stream(&group.id) == Some(stream)
                        && group.scope_record_index == scope.record_index
                })
                .filter_map(|group| {
                    let mut ordinals = group
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .enumerate()
                        .filter(|(_, member)| **member == record_index);
                    let (ordinal, _) = ordinals.next()?;
                    ordinals.next().is_none().then_some((group, ordinal))
                });
            let Some((group, group_member_ordinal)) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            let Some(header) = header_index.get(&(stream, record_index)) else {
                continue;
            };
            let Some(recipe) = parse_vertex_recipe(bytes, records, stream, header, recipes) else {
                continue;
            };
            let (Ok(scope_reference_ordinal), Ok(group_member_ordinal)) = (
                u32::try_from(scope_reference_ordinal),
                u32::try_from(group_member_ordinal),
            ) else {
                continue;
            };

            out.push(DesignEdgeTreatmentVertexOperand {
                id: design_record_id(
                    stream,
                    ":edge-treatment-vertex-operand#",
                    header.byte_offset,
                ),
                scope_record_index: scope.record_index,
                scope_reference_ordinal,
                group_record_index: group.record_index,
                group_member_ordinal,
                recipe,
            });
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

/// Bind each `WorkPoint` input to its exact edge, vertex, or `WorkPlane` carrier.
pub(crate) fn bind_work_point_input_carriers(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    edge_operands: &[DesignEdgeOperand],
    sketch_points: &[SketchPoint],
) -> Result<(), CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut work_planes: HashMap<String, HashMap<u32, u32>> = HashMap::new();
    for scope in scopes.iter().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::WorkPlane
                | crate::records::feature::scope::DesignFeatureKind::PlanoDeObra
        )
    }) {
        let (Some(stream), Some(preceding_index)) =
            (native_stream(&scope.id), scope.record_index.checked_sub(1))
        else {
            continue;
        };
        work_planes
            .entry(stream.to_owned())
            .or_default()
            .insert(preceding_index, scope.record_index);
    }
    let mut record_offset_index: HashMap<String, IndexedRecordOffsets> = HashMap::new();

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPoint
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let stream = stream.to_owned();
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, &stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_owned_record_offsets(&mut record_offset_index, &stream, bytes);
        let scope_record_index = scope.record_index;
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let mut inputs = construction.rule.inputs().to_vec();
        for input in &mut inputs {
            let mut edge_matches = edge_operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream.as_str())
                    && operand.scope_record_index == scope_record_index
                    && operand.record_index() == input.record_index()
            });
            let edge_match = edge_matches.next();
            if let Some(operand) = edge_match.filter(|_| edge_matches.next().is_none()) {
                input
                    .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::EdgeRecipe {
                        operand_id: operand.id.clone(),
                    })))
                    .map_err(CodecError::Malformed)?;
                continue;
            }
            let Some(header) = header_index.get(&(stream.as_str(), input.record_index())) else {
                continue;
            };
            if let Some(recipe) = parse_vertex_recipe(bytes, records, &stream, header, recipes) {
                input
                    .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                        recipe,
                    })))
                    .map_err(CodecError::Malformed)?;
                continue;
            }
            if let Some(frame) =
                parse_work_point_sketch_point_frame(bytes, input.record_index(), header.byte_offset)
            {
                if let Some(selection) = resolve_sketch_point_selection(
                    frame,
                    header,
                    input.record_index(),
                    &stream,
                    sketch_points,
                )? {
                    input
                        .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::SketchPoint {
                            selection,
                        })))
                        .map_err(CodecError::Malformed)?;
                }
                continue;
            }
            let Some(selection) = parse_entity_selection_frame(
                bytes,
                input.record_index(),
                header.byte_offset,
                header.class_tag.as_str(),
            ) else {
                continue;
            };
            let Ok(primary_identity) = u32::try_from(selection.primary_identity) else {
                continue;
            };
            let Some(work_plane_scope_record_index) = work_planes
                .get(stream.as_str())
                .and_then(|indices| indices.get(&primary_identity))
                .copied()
            else {
                continue;
            };
            if selection.secondary.is_some() {
                continue;
            }
            let (Ok(asset_id), Ok(context_id)) = (
                crate::records::mesh::DesignRelaxedGuidText::try_from(selection.asset_id),
                crate::records::mesh::DesignRelaxedGuidText::try_from(selection.context_id),
            ) else {
                continue;
            };
            input
                .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::WorkPlane {
                    selection: DesignWorkPointPlaneSelection::try_new(
                        input.record_index(),
                        crate::records::feature::work_geometry::DesignWorkPointPlaneSelectionDraft {
                        class_tag: header.class_tag.clone(),
                        asset_id,
                        asset_id_offset: selection.asset_id_offset,
                        context_id,
                        context_id_offset: selection.context_id_offset,
                        identity_record_index: selection.identity_record_index,
                        identity_record_offset: selection.identity_record_offset,
                        primary_identity: selection.primary_identity,
                        primary_identity_offset: selection.primary_identity_offset,
                        work_plane_scope_record_index,
                        next_record_index: selection.next_record_index,
                        next_byte_offset: selection.next_byte_offset,
                        },
                    )
                    .map_err(CodecError::Malformed)?,
                })))
                .map_err(CodecError::Malformed)?;
        }
        construction.rule =
            DesignWorkPointRule::from_serialized(construction.rule.reference_type(), inputs)
                .map_err(CodecError::Malformed)?;
    }
    Ok(())
}

/// Bind each vertex-rule input of a `Hole` location that is a direct
/// sketch-point envelope to the sketch point it names.
pub(crate) fn bind_hole_location_sketch_points(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    sketch_points: &[SketchPoint],
) -> Result<(), CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        header_index.insert((stream, header.record_index), header);
    }
    for scope in scopes.iter_mut() {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let stream = stream.to_owned();
        let Some(construction) = scope.hole_construction_mut() else {
            continue;
        };
        if construction
            .locations
            .iter()
            .all(|location| location.vertex_input.is_none())
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, &stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        bind_hole_construction_sketch_points(
            bytes,
            &stream,
            construction,
            &header_index,
            sketch_points,
        )?;
    }
    Ok(())
}

fn bind_hole_construction_sketch_points(
    bytes: &[u8],
    stream: &str,
    construction: &mut DesignHoleConstruction,
    headers: &HashMap<(&str, u32), &DesignRecordHeader>,
    sketch_points: &[SketchPoint],
) -> Result<(), CodecError> {
    for input in construction
        .locations
        .iter_mut()
        .filter_map(|location| location.vertex_input.as_mut())
    {
        let Some(header) = headers.get(&(stream, input.record_index())) else {
            continue;
        };
        let Some(frame) =
            parse_work_point_sketch_point_frame(bytes, input.record_index(), header.byte_offset)
        else {
            continue;
        };
        if let Some(selection) = resolve_sketch_point_selection(
            frame,
            header,
            input.record_index(),
            stream,
            sketch_points,
        )? {
            input
                .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::SketchPoint {
                    selection,
                })))
                .map_err(CodecError::Malformed)?;
        }
    }
    Ok(())
}

/// The selection of the one decoded sketch point that a direct sketch-point
/// envelope at input `record_index` names by its Sketch record and
/// persistent id.
fn resolve_sketch_point_selection(
    frame: WorkPointSketchPointFrame,
    header: &DesignRecordHeader,
    record_index: u32,
    stream: &str,
    sketch_points: &[SketchPoint],
) -> Result<Option<DesignWorkPointSketchPointSelection>, CodecError> {
    let (Ok(asset_id), Ok(context_id)) = (
        crate::records::mesh::DesignRelaxedGuidText::try_from(frame.asset_id),
        crate::records::mesh::DesignRelaxedGuidText::try_from(frame.context_id),
    ) else {
        return Ok(None);
    };
    let mut point_matches = sketch_points.iter().filter(|point| {
        native_stream(&point.id) == Some(stream)
            && point.owner_reference == Some(frame.sketch_record_index)
            && point.persistent_id() == Some(frame.point_persistent_id)
    });
    let (Some(point), None) = (point_matches.next(), point_matches.next()) else {
        return Ok(None);
    };
    DesignWorkPointSketchPointSelection::try_new(
        record_index,
        crate::records::feature::work_geometry::DesignWorkPointSketchPointSelectionDraft {
            class_tag: header.class_tag.clone(),
            asset_id,
            asset_id_offset: frame.asset_id_offset,
            context_id,
            context_id_offset: frame.context_id_offset,
            identity_record_index: frame.identity_record_index,
            identity_record_offset: frame.identity_record_offset,
            sketch_record_index: frame.sketch_record_index,
            sketch_record_index_offset: frame.sketch_record_index_offset,
            point_persistent_id: frame.point_persistent_id,
            point_persistent_id_offset: frame.point_persistent_id_offset,
            point_native_id: point.id.clone(),
            next_record_index: frame.next_record_index,
            next_byte_offset: frame.next_byte_offset,
        },
    )
    .map(Some)
    .map_err(CodecError::Malformed)
}

/// Bind the exact three-vertex construction carried by a `WorkPlane` scope.
pub(crate) fn bind_work_plane_constructions(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Result<(), CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut record_offset_index: HashMap<String, IndexedRecordOffsets> = HashMap::new();

    for scope in scopes.iter_mut().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::WorkPlane
                | crate::records::feature::scope::DesignFeatureKind::PlanoDeObra
        )
    }) {
        if let Some(frame) = scope.work_plane_frame_mut() {
            frame.work_plane_construction = None;
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let stream = stream.to_owned();
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, &stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_owned_record_offsets(&mut record_offset_index, &stream, bytes);
        let Some([placement_record_index, first, second, third, extra_offset]) =
            scope.reference_members().values_array()
        else {
            continue;
        };
        if scope.work_plane_frame().is_none() || scope.work_plane_reference() != Some(*extra_offset)
        {
            continue;
        }
        let Some(owner) = owners.iter().find(|owner| {
            native_stream(owner.id()) == Some(stream.as_str())
                && owner.record_index() == *extra_offset
                && owner.scope_record_index() == scope.record_index
                && owner.evaluated_value().get() == 0.0
        }) else {
            continue;
        };
        if !parameters.iter().any(|parameter| {
            native_stream(&parameter.id) == Some(stream.as_str())
                && parameter.record_index == owner.parameter_record_index()
                && parameter.owner_record_index() == Some(owner.record_index())
                && parameter.source_kind() == "ExtraOffset"
                && parameter.evaluated_value().get() == 0.0
        }) {
            continue;
        }
        let [Some(first_input), Some(second_input), Some(third_input)] = [first, second, third]
            .map(|record_index| {
                parse_vertex_recipe(
                    bytes,
                    records,
                    &stream,
                    header_index.get(&(stream.as_str(), *record_index))?,
                    recipes,
                )
            })
        else {
            continue;
        };
        let inputs = [first_input, second_input, third_input];
        let placement_record_index = *placement_record_index;
        if let Some(frame) = scope.work_plane_frame_mut() {
            frame.work_plane_construction = Some(
                DesignWorkPlaneConstruction::try_new(placement_record_index, Box::new(inputs))
                    .map_err(CodecError::malformed)?,
            );
        }
    }
    Ok(())
}

/// Bind persistent subentity candidates carried by decoded vertex recipes.
pub(crate) fn bind_vertex_recipe_candidates(
    scopes: &mut [DesignParameterScope],
    tags: &[PersistentSubentityTag],
) {
    for scope in scopes {
        let scope_id = scope.id.clone();
        if let Some(construction) = scope.work_plane_construction_mut() {
            for reference in construction.recipe_references_mut() {
                bind_recipe_reference_candidates(reference, tags, Some(&scope_id));
            }
        }
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        for recipe in construction.rule.vertex_recipes_mut() {
            for reference in &mut recipe.recipe_references {
                bind_recipe_reference_candidates(reference, tags, Some(&scope_id));
            }
        }
    }
}

/// Bind active fallback candidates for edge-treatment corner recipes.
pub(crate) fn bind_edge_treatment_vertex_candidates(
    operands: &mut [DesignEdgeTreatmentVertexOperand],
    tags: &[PersistentSubentityTag],
) {
    for operand in operands {
        for reference in &mut operand.recipe.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
    }
}

/// Whether a feature family owns edge-recipe operands directly or through a
/// counted construction-operand group.
pub(crate) fn has_edge_recipe_operands(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> bool {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    matches!(
        design_feature_family(kind),
        Some(
            DesignFeatureFamily::Fillet
                | DesignFeatureFamily::Chamfer
                | DesignFeatureFamily::Revolve
                | DesignFeatureFamily::Loft
                | DesignFeatureFamily::Sweep
                | DesignFeatureFamily::Pipe
                | DesignFeatureFamily::SurfacePatch
                | DesignFeatureFamily::SurfaceExtend
                | DesignFeatureFamily::SurfaceOffset
                | DesignFeatureFamily::SurfaceRuled
        )
    ) || matches!(kind, Kind::EdgeFlange | Kind::Hem | Kind::WorkPoint)
}

/// Indexed-record distance from an edge-recipe primary record to its terminal
/// record for the owning consumer.
pub(crate) fn edge_recipe_terminal_delta(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> u32 {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    match design_feature_family(kind) {
        Some(DesignFeatureFamily::Sweep) => 7,
        _ if matches!(kind, Kind::WorkPoint) => 5,
        _ => 4,
    }
}

/// Decode persistent selection identities named by Fillet and Chamfer groups.
pub(crate) fn decode_edge_identity_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEdgeIdentityOperand>, CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| {
            native_stream(&scope.id) == Some(stream)
                && scope.record_index == group.scope_record_index
                && matches!(
                    design_feature_family(&scope.kind()),
                    Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
                )
        }) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Some(header) = header_index.get(&(stream, record_index)) else {
                continue;
            };
            let Ok(start) = usize::try_from(header.byte_offset) else {
                continue;
            };
            let Some(parsed) = parse_edge_identity_member(bytes, start) else {
                continue;
            };
            let Ok(group_member_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let (Ok(asset_id), Ok(context_id)) = (
                crate::records::mesh::DesignRelaxedGuidText::try_from(parsed.asset_id),
                crate::records::mesh::DesignRelaxedGuidText::try_from(parsed.context_id),
            ) else {
                continue;
            };

            out.push(
                DesignEdgeIdentityOperand::try_new(
                    crate::records::topology::edge_identity::DesignEdgeIdentityOperandDraft {
                        id: design_record_id(
                            &entry.name,
                            ":design-edge-identity-operand#",
                            header.byte_offset,
                        ),
                        scope_record_index: scope.record_index,
                        group_record_index: group.record_index,
                        group_member_ordinal,
                        record_index,
                        byte_offset: header.byte_offset,
                        class_tag: header.class_tag.clone(),
                        layout: parsed.layout,
                        local_id: parsed.local_id,
                        asset_id,
                        asset_id_offset: parsed.asset_id_offset,
                        context_id,
                        context_id_offset: parsed.context_id_offset,
                        historical: None,
                        treatment_radius_candidates: Vec::new(),
                        transition_edge_candidates: Vec::new(),
                        resolved_edge_slots: Vec::new(),
                        resolved_edge_slot: None,
                        resolution_identity_id: None,
                    },
                )
                .map_err(CodecError::Malformed)?,
            );
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode face-recipe operand frames named by grouped and direct feature references.
pub(crate) fn decode_face_operands(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignFaceOperand>, CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut scope_index = HashMap::new();
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let key = (stream, scope.record_index);
        scope_index.insert(key, scope);
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        seen.insert((stream, group.scope_record_index, group.record_index));
        let Some(scope) = scope_index.get(&(stream, group.scope_record_index)) else {
            continue;
        };
        let is_extrude_operand = matches!(
            group.extrude_role(),
            Some(DesignExtrudeOperandRole::Profile | DesignExtrudeOperandRole::Faces(_))
        );
        let is_offset_faces_operand = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::OffsetFaces)
            && group.role() == DesignOperandRole::ROLE_0X10;
        let is_shell_operand = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Shell)
            && group.role() == DesignOperandRole::ROLE_0X10;
        let is_loft_profile = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Loft)
            && matches!(
                group.role(),
                DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
            );
        let is_sweep_guide_surface = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Sweep)
            && group.role() == DesignOperandRole::FACES;
        let is_revolve_axis = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Revolve)
            && group.role() == DesignOperandRole::ROLE_0X21;
        let is_edge_treatment_support = matches!(
            design_feature_family(&scope.kind()),
            Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
        );
        let is_circular_pattern_seed = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::CircularPattern)
            && group.role() == DesignOperandRole::BODIES_B;
        let is_mirror_seed = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Mirror)
            && group.role() == DesignOperandRole::BODIES_B;
        let is_mirror_plane = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::Mirror)
            && group.role() == DesignOperandRole::ROLE_0X5;
        let is_split_face_operand =
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace;
        let is_delete_face_operand = matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::MoveFace
                | crate::records::feature::scope::DesignFeatureKind::DeplacerFace
                | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
        );
        let is_emboss_face = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::Emboss
            && group.role() == DesignOperandRole::ROLE_0X10
            && scope
                .emboss_operand_construction()
                .is_some_and(|construction| {
                    group.record_index == construction.face_group.value
                        && group.scope_reference_ordinal == 2
                });
        let is_thread_face = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::Thread
            && group.role() == DesignOperandRole::ROLE_0X10;
        let is_hole_face = scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
            && group.role() == DesignOperandRole::BODIES_A;
        let is_draft_operand =
            design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Draft);
        let is_replace_face_operand = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::ReplaceFace)
            && group.role() == DesignOperandRole::ROLE_0X10;
        let is_surface_offset_operand = design_feature_family(&scope.kind())
            == Some(DesignFeatureFamily::SurfaceOffset)
            && group.role() == DesignOperandRole::PROFILE;
        if !is_extrude_operand
            && !is_offset_faces_operand
            && !is_shell_operand
            && !is_loft_profile
            && !is_sweep_guide_surface
            && !is_revolve_axis
            && !is_edge_treatment_support
            && !is_circular_pattern_seed
            && !is_mirror_seed
            && !is_mirror_plane
            && !is_split_face_operand
            && !is_delete_face_operand
            && !is_emboss_face
            && !is_thread_face
            && !is_hole_face
            && !is_draft_operand
            && !is_replace_face_operand
            && !is_surface_offset_operand
        {
            continue;
        }
        if group.extrude_role() == Some(DesignExtrudeOperandRole::Profile)
            && scope.extrude_profile().is_some()
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        for (group_member_index, record_index) in group
            .members()
            .iter()
            .map(|member| &member.value)
            .enumerate()
        {
            if !seen.insert((stream, scope.record_index, *record_index)) {
                continue;
            }
            let Ok(group_member_ordinal) = u32::try_from(group_member_index) else {
                continue;
            };
            let Some(header) = header_index.get(&(stream, *record_index)) else {
                continue;
            };
            let next_byte_offset = group
                .members()
                .get(group_member_index + 1)
                .and_then(|record| header_index.get(&(stream, record.value)))
                .copied()
                .map(|header| header.byte_offset)
                .or_else(|| {
                    if !is_offset_faces_operand && !is_shell_operand {
                        return None;
                    }
                    scope
                        .reference_members()
                        .values()
                        .position(|candidate| candidate == record_index)
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal + 1))
                        .and_then(|record_index| header_index.get(&(stream, *record_index)))
                        .map(|header| header.byte_offset)
                });
            if let Some(operand) = parse_face_operand(
                ctx,
                bytes,
                records,
                crate::design::decode::operands::FaceOperandFrame {
                    scope,
                    scope_reference_ordinal: group.scope_reference_ordinal,
                    group_ownership: Some((group.record_index, group_member_ordinal)),
                    next_byte_offset,
                    header,
                },
                recipes,
            ) {
                out.push(operand?);
            }
        }
    }
    for scope in scope_index.values().filter(|scope| {
        let is_legacy_as_built_421 = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::AsBuilt
            && crate::design::assembly::legacy_as_built_421_generation(
                scope.frame_length(),
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            )
            .is_some();
        matches!(
            design_feature_family(&scope.kind()),
            Some(
                DesignFeatureFamily::OffsetFaces
                    | DesignFeatureFamily::Shell
                    | DesignFeatureFamily::Thicken
                    | DesignFeatureFamily::Split
                    | DesignFeatureFamily::ReplaceFace
            )
        ) || matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::SplitFace
                | crate::records::feature::scope::DesignFeatureKind::Hole
        ) || is_legacy_as_built_421
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        let legacy_as_built = scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::AsBuilt
            && crate::design::assembly::legacy_as_built_421_generation(
                scope.frame_length(),
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            )
            .is_some();
        for ordinal in (0..scope.reference_members().len())
            .filter(|ordinal| !legacy_as_built || matches!(*ordinal, 1 | 3))
        {
            let Some(record_index) = scope.reference_members().values().nth(ordinal).copied()
            else {
                continue;
            };
            if !seen.insert((stream, scope.record_index, record_index)) {
                continue;
            }
            let (Ok(scope_reference_ordinal), Some(header)) = (
                u32::try_from(ordinal),
                header_index.get(&(stream, record_index)),
            ) else {
                continue;
            };
            let next_byte_offset = if scope.kind()
                == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                && crate::design::assembly::legacy_as_built_421_generation(
                    scope.frame_length(),
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                )
                .is_some()
            {
                scope
                    .reference_members()
                    .values()
                    .nth(ordinal + 1)
                    .and_then(|record_index| header_index.get(&(stream, *record_index)))
                    .map(|header| header.byte_offset)
            } else {
                None
            };
            if let Some(operand) = parse_face_operand(
                ctx,
                bytes,
                records,
                crate::design::decode::operands::FaceOperandFrame {
                    scope,
                    scope_reference_ordinal,
                    group_ownership: None,
                    next_byte_offset,
                    header,
                },
                recipes,
            ) {
                out.push(operand?);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode ordered sheet-metal source-region identities.
pub(crate) fn decode_face_source_groups(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    types: &[SegmentType],
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignFaceSourceGroup>, CodecError> {
    let mut out = Vec::new();
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    for scope in scopes
        .iter()
        .filter(|scope| scope.kind() == crate::records::feature::scope::DesignFeatureKind::Face)
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        if !face_source_record_is_registered(
            types,
            &scope.id,
            scope.record_index,
            scope.class_tag.as_str(),
            FaceSourceType::Scope,
        ) {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        let Ok(scope_start) = usize::try_from(scope.byte_offset()) else {
            continue;
        };
        let reference_headers = face_source_reference_headers(
            bytes,
            scope_start,
            scope.reference_members().values(),
            records,
        );
        for (carrier_ordinal, carrier) in reference_headers.iter().enumerate() {
            let Some(FaceSourceReferenceHeader {
                record_index: carrier_record_index,
                byte_offset: carrier_byte_offset,
                class_tag: carrier_class_tag,
            }) = carrier
            else {
                continue;
            };
            if !face_source_record_is_registered(
                types,
                &scope.id,
                *carrier_record_index,
                carrier_class_tag,
                FaceSourceType::Carrier,
            ) {
                continue;
            }
            let Some(FaceSourceReferenceHeader {
                record_index: paired_record_index,
                byte_offset: paired_byte_offset,
                class_tag: paired_class_tag,
            }) = reference_headers
                .get(carrier_ordinal + 1)
                .and_then(Option::as_ref)
            else {
                continue;
            };
            if !face_source_record_is_registered(
                types,
                &scope.id,
                *paired_record_index,
                paired_class_tag,
                FaceSourceType::Settings,
            ) {
                continue;
            }
            let Some(source_reference_offsets) = parse_face_source_carrier_prefix(
                ctx,
                bytes,
                *carrier_byte_offset,
                *paired_byte_offset,
                scope.record_index,
            )?
            else {
                continue;
            };
            let pair_count = face_source_profile_pair_count(types, scope, |record| {
                reference_headers
                    .iter()
                    .flatten()
                    .find(|header| header.record_index == record)
                    .map(|header| header.class_tag)
            });
            if pair_count != Some(source_reference_offsets.len()) {
                continue;
            }
            let parsed_source_members = source_reference_offsets.into_iter()
                .map(|(offset, source_record_index)| -> Result<Option<_>, CodecError> {
                    let Some(source_byte_offset) = carrier_byte_offset.checked_add(indexed_header::LEN).and_then(|at| records.first_at_or_after(at, source_record_index)) else { return Ok(None); };
                    let Some((source_class_tag, _)) =
                        lp_ascii_filtered_view(bytes, source_byte_offset, 3..=3, u8::is_ascii_digit) else { return Ok(None); };
                    let source_class = source_class_tag;
                    if !face_source_record_is_registered(types, &scope.id, source_record_index, source_class, FaceSourceType::Identity) { return Ok(None); }
                    let Some(member) = parse_extrude_identity_member(bytes, source_byte_offset, None) else { return Ok(None); };
                    let Ok(source_byte_offset_u64) = u64::try_from(source_byte_offset) else { return Ok(None); };
                    let Ok(offset) = u64::try_from(offset) else { return Ok(None); };
                    let Ok(class_tag) = crate::design::decode::text::class_tag_from_view(source_class_tag) else { return Ok(None); };
                    let (Ok(asset_id), Ok(context_id)) = (member.asset_id.try_into(), member.context_id.try_into()) else { return Ok(None); };
                    let Some(persistent_identity) = DesignConstructionPersistentIdentity::try_new(crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
                        local_id: member.local_id,
                        local_id_offset: member.local_id_offset,
                        asset_id,
                        asset_id_offset: member.asset_id_offset,
                        context_id,
                        context_id_offset: member.context_id_offset,
                        tail_slot_present: member.tail_slot_present,
                        tail_slot_offset: member.tail_slot_offset,
                        next_record_index: member.next_record_index,
                        next_byte_offset: member.next_byte_offset,
                    }).ok() else { return Ok(None); };
                    Ok(Some(crate::records::identity::Located {
                        offset,
                        value: DesignFaceSourceMember {
                            record_index: source_record_index,
                            byte_offset: source_byte_offset_u64,
                            class_tag,
                            persistent_identity,
                        },
                    }))
                });
            let mut source_members = Vec::new();
            let mut complete = true;
            for member in parsed_source_members {
                match member? {
                    Some(member) => source_members.push(member),
                    None => complete = false,
                }
            }
            if !complete {
                continue;
            }
            let Ok(carrier_reference_ordinal) = u32::try_from(carrier_ordinal) else {
                continue;
            };
            let (Ok(carrier_start), Ok(carrier_end)) = (
                u64::try_from(*carrier_byte_offset),
                u64::try_from(*paired_byte_offset),
            ) else {
                continue;
            };
            let Some(carrier_span) =
                crate::records::identity::NonEmptyByteSpan::new(carrier_start, carrier_end)
            else {
                continue;
            };
            let (Ok(carrier_class_tag), Ok(paired_class_tag)) = (
                crate::design::decode::text::class_tag_from_view(carrier_class_tag),
                crate::design::decode::text::class_tag_from_view(paired_class_tag),
            ) else {
                continue;
            };
            out.push(DesignFaceSourceGroup {
                id: design_record_id(&entry.name, ":design-face-source-group#", carrier_start),
                scope_record_index: scope.record_index,
                carrier_reference_ordinal,
                carrier_record_index: *carrier_record_index,
                carrier_span,
                carrier_class_tag,
                paired_record_index: *paired_record_index,
                paired_class_tag,
                source_members,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

struct FaceSourceReferenceHeader<'a> {
    record_index: u32,
    byte_offset: usize,
    class_tag: &'a str,
}

fn face_source_reference_headers<'a, 'r>(
    bytes: &'a [u8],
    scope_start: usize,
    references: impl Iterator<Item = &'r u32>,
    records: &IndexedRecordOffsets,
) -> Vec<Option<FaceSourceReferenceHeader<'a>>> {
    references
        .map(|record_index| {
            scope_start
                .checked_add(indexed_header::LEN)
                .and_then(|at| records.first_at_or_after(at, *record_index))
                .and_then(|byte_offset| {
                    lp_ascii_filtered_view(bytes, byte_offset, 3..=3, u8::is_ascii_digit).map(
                        |(class_tag, _)| FaceSourceReferenceHeader {
                            record_index: *record_index,
                            byte_offset,
                            class_tag,
                        },
                    )
                })
        })
        .collect()
}

/// Counted source-reference run followed by an opaque scalar lane.
#[derive(Clone, Copy)]
pub(crate) struct FaceSourceCarrierLayout {
    pub(crate) source_count: usize,
    pub(crate) source_reference_offset: usize,
    pub(crate) scalar_offset: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum FaceSourceType {
    Scope,
    Carrier,
    Settings,
    Identity,
    SourceGroup,
    Profile,
}

impl FaceSourceType {
    fn matches(self, registration: &crate::records::entity_header::SegmentTypeData) -> bool {
        let (guid, module, versions): (&str, &str, &[u32]) = match self {
            Self::Scope => ("7AFA0A65-B53C-472C-ACA4-5C7C0A65001C", "Fusion", &[3]),
            Self::Carrier => ("CAA55628-037B-4D01-9C74-26118F70B63E", "Fusion", &[0]),
            Self::Settings => ("E75C86D6-76D8-4F1F-BC16-AB8CB74958B0", "Fusion", &[2, 3]),
            Self::Identity => (
                "C46D3EEB-40E1-43FA-83DC-9CB204335417",
                "EntityTracking",
                &[1],
            ),
            Self::SourceGroup => ("2CA5A1CD-C99B-4C9B-91AF-57989148E841", "Fusion", &[1]),
            Self::Profile => ("4BD53E5A-0B3E-45E5-AE8B-02044306485A", "Fusion", &[0]),
        };
        registration.type_guid.as_str().eq_ignore_ascii_case(guid)
            && registration.module == module
            && versions.contains(&registration.version)
    }
}

pub(crate) fn face_source_record_is_registered(
    types: &[SegmentType],
    source_id: &str,
    record_index: u32,
    class_tag: &str,
    role: FaceSourceType,
) -> bool {
    let Some(segment) = crate::ids::design_segment(source_id) else {
        return false;
    };
    let mut matches = types.iter().filter(|row| {
        crate::ids::design_segment(row.id()) == Some(segment)
            && role.matches(row)
            && row
                .entities
                .values()
                .any(|entity| *entity == u64::from(record_index))
    });
    let Some(row) = matches.next() else {
        return false;
    };
    if matches.next().is_some() {
        return false;
    }
    let ordinal = types
        .iter()
        .filter(|other| {
            crate::ids::design_segment(other.id()) == Some(segment)
                && other.byte_offset < row.byte_offset
        })
        .count();
    ordinal
        .checked_add(256)
        .is_some_and(|value| value.to_string() == class_tag)
}

pub(crate) fn face_source_profile_pair_count<'a>(
    types: &[SegmentType],
    scope: &DesignParameterScope,
    mut class_for: impl FnMut(u32) -> Option<&'a str>,
) -> Option<usize> {
    let mut count = 0usize;
    let mut references = scope.reference_members().values().peekable();
    while let Some(group) = references.next() {
        let Some(class) = class_for(*group) else {
            continue;
        };
        if !face_source_record_is_registered(
            types,
            &scope.id,
            *group,
            class,
            FaceSourceType::SourceGroup,
        ) {
            continue;
        }
        let member = **references.peek()?;
        if !face_source_record_is_registered(
            types,
            &scope.id,
            member,
            class_for(member)?,
            FaceSourceType::Profile,
        ) {
            return None;
        }
        count = count.checked_add(1)?;
    }
    (count != 0).then_some(count)
}

pub(crate) fn face_source_carrier_spec(source_count: usize) -> Option<FaceSourceCarrierLayout> {
    (source_count != 0).then_some(())?;
    Some(FaceSourceCarrierLayout {
        source_count,
        source_reference_offset: region_carrier::LEN,
        scalar_offset: region_carrier::LEN
            .checked_add(source_count.checked_mul(region_reference::LEN)?)?,
    })
}

fn parse_face_source_carrier_prefix(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    scope_record_index: u32,
) -> Result<Option<Vec<(usize, u32)>>, CodecError> {
    let layout = (|| {
        if end > bytes.len() {
            return None;
        }
        let remaining = end.checked_sub(
            start
                .checked_add(region_carrier::LEN)?
                .checked_add(region_scalar::LEN)?,
        )?;
        let count = View::u32_le_at(bytes, start.checked_add(region_carrier::IDENTITY_COUNT)?)?;
        let count = bounded_len(u64::from(count), region_reference::LEN, remaining)?;
        let layout = face_source_carrier_spec(count)?;
        let scalar = start.checked_add(layout.scalar_offset)?;
        if bytes.get(
            start.checked_add(region_carrier::ZERO_RUN_10)?
                ..start.checked_add(region_carrier::OWNER_MARKER)?,
        )? != [0; 10]
            || marked_face_source_reference(
                bytes,
                start.checked_add(region_carrier::OWNER_MARKER)?,
            )? != scope_record_index
            || !View::f64_le_at(bytes, scalar.checked_add(region_scalar::SCALAR)?)?.is_finite()
            || View::u32_le_at(bytes, scalar.checked_add(region_scalar::DISCRIMINATOR)?)?
                != View::u32_le_at(
                    bytes,
                    scalar.checked_add(region_scalar::REPEATED_DISCRIMINATOR)?,
                )?
        {
            return None;
        }
        Some(layout)
    })();
    let Some(layout) = layout else {
        return Ok(None);
    };
    let mut references = ctx.alloc_filled(
        layout.source_count,
        (0, 0),
        "F3D sheet-metal source-region references",
    )?;
    for (ordinal, slot) in references.iter_mut().enumerate() {
        let offset = start + layout.source_reference_offset + ordinal * region_reference::LEN;
        let Some(record) = marked_face_source_reference(bytes, offset) else {
            return Ok(None);
        };
        *slot = (offset, record);
    }
    Ok(Some(references))
}

fn marked_face_source_reference(bytes: &[u8], offset: usize) -> Option<u32> {
    if bytes.get(offset.checked_add(region_reference::MARKER)?) != Some(&1)
        || bytes.get(
            offset.checked_add(region_reference::ZERO_TRAILER)?
                ..offset.checked_add(region_reference::LEN)?,
        )? != [0; 6]
    {
        return None;
    }
    View::u32_le_at(bytes, offset.checked_add(region_reference::RECORD_INDEX)?)
        .filter(|record| *record != 0)
}

/// Index recipes by their final record for each native recipe identity.
fn indexed_operand_recipes(recipes: &[ConstructionRecipe]) -> HashMap<&str, &ConstructionRecipe> {
    let mut indexed = HashMap::new();
    for recipe in recipes {
        indexed.insert(recipe.id.as_str(), recipe);
    }
    indexed
}

fn indexed_operand_headers(
    headers: &[DesignRecordHeader],
) -> HashMap<(&str, u32), &DesignRecordHeader> {
    let mut indexed = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        indexed.insert(key, header);
    }
    indexed
}

fn referenced_operand_faces(
    references: &[crate::records::dimensions::DesignRecipeReference],
    design_reference: i64,
) -> HashSet<&cadmpeg_ir::ids::FaceId> {
    let mut referenced = HashSet::new();
    for face in references
        .iter()
        .filter(|reference| reference.design_reference == design_reference)
        .flat_map(|reference| &reference.candidate_faces)
    {
        referenced.insert(face);
    }
    referenced
}

/// Join each face recipe's persistent Design reference to active solved faces.
pub(crate) fn bind_face_operand_candidates(
    operands: &mut [DesignFaceOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let recipes = indexed_operand_recipes(recipes);
    for operand in operands {
        operand.alternate_selector_candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        operand.candidate_faces.clear();
        for tag in tags.iter().filter(|tag| {
            tag.design_references.contains(&design_reference)
                && crate::ids::same_native_occurrence(&tag.id, &operand.id)
        }) {
            if let AttributeTarget::Face(face) = &tag.target {
                operand.candidate_faces.push(face.clone());
            }
        }
        operand
            .candidate_faces
            .sort_by(|left, right| left.as_str().cmp(right.as_str()));
        operand.candidate_faces.dedup();
        let referenced = referenced_operand_faces(&operand.recipe_references, design_reference);
        operand.unreferenced_candidate_faces.clear();
        for face in operand
            .candidate_faces
            .iter()
            .filter(|face| !referenced.contains(face))
        {
            operand.unreferenced_candidate_faces.push(face.clone());
        }
        operand.alternate_selector_candidate_faces.extend(
            operand
                .recipe_references
                .iter()
                .filter(|reference| reference.design_reference == design_reference)
                .flat_map(|reference| &reference.alternate_selector_faces)
                .cloned(),
        );
        operand
            .alternate_selector_candidate_faces
            .sort_by(|left, right| left.as_str().cmp(right.as_str()));
        operand.alternate_selector_candidate_faces.dedup();
    }
}

/// Join each edge recipe's persistent Design reference to active solved faces.
pub(crate) fn bind_edge_operand_candidates(
    operands: &mut [DesignEdgeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    let recipes = indexed_operand_recipes(recipes);
    for operand in operands {
        operand.candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        for tag in tags.iter().filter(|tag| {
            tag.design_references.contains(&design_reference)
                && crate::ids::same_native_occurrence(&tag.id, &operand.id)
        }) {
            if let cadmpeg_ir::attributes::AttributeTarget::Face(face) = &tag.target {
                operand.candidate_faces.push(face.clone());
            }
        }
        operand
            .candidate_faces
            .sort_by(|a, b| a.as_str().cmp(b.as_str()));
        operand.candidate_faces.dedup();
    }
}

pub(crate) fn edge_operand_candidate_faces(
    design_reference: i64,
    tags: &[PersistentSubentityTag],
    owner_id: Option<&str>,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| {
            tag.design_references.contains(&design_reference)
                && owner_id
                    .is_none_or(|owner_id| crate::ids::same_native_occurrence(&tag.id, owner_id))
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(id) => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    faces
}

/// Resolve the unique sketch-profile frame named by profile-based scopes.
pub(crate) fn bind_sketch_profiles(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut losses = Vec::new();
    for scope in scopes.iter_mut().filter(|scope| {
        design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sweep)
            || scope.emboss_operand_construction().is_some()
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFlange
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut profiles = Vec::new();
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            if scope.emboss_operand_construction().is_some() && ordinal != 1 {
                continue;
            }
            let (Ok(ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            let Some(profile) = parse_sketch_profile(bytes, stream, ordinal, header, entities)
            else {
                continue;
            };
            profiles.push(profile);
        }
        if profiles.len() > 1 {
            if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
                scope.payload_mut()
            {
                losses.push(crate::loss::F3dLossCode::FeatureExtrudeProfileGroupNative.note(
                    format!("{} Sketch-profile operands remain native because their group composition is unresolved.", profiles.len()),
                ));
                slot.get_or_insert_with(Default::default)
                    .extrude_native_profiles = Some(profiles);
            }
            continue;
        }
        let Some(profile) = profiles.pop() else {
            continue;
        };
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFlange {
            if let crate::records::feature::scope::DesignScopePayloadMut::BaseFlange(slot) =
                scope.payload_mut()
            {
                slot.get_or_insert_with(Default::default)
                    .base_flange_profile = Some(profile);
            }
        } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Emboss {
            if profile.scope_reference_ordinal == 1 {
                if let crate::records::feature::scope::DesignScopePayloadMut::Emboss(Some(
                    construction,
                )) = scope.payload_mut()
                {
                    construction.sketch_profile = Some(profile);
                }
            }
        } else if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sweep) {
            if let crate::records::feature::scope::DesignScopePayloadMut::Sweep(slot) =
                scope.payload_mut()
            {
                slot.get_or_insert_with(Default::default).sweep_profile = Some(profile);
            }
        } else if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extruir(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::ExtrudeJa(slot) =
            scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extrude_profile = Some(profile);
        }
    }
    Ok(losses)
}

/// Decode compact counted selections in Extrude and Emboss scopes.
pub(crate) fn decode_extrude_selection_groups(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionGroup>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| {
        design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude)
            || scope.emboss_operand_construction().is_some()
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            if scope.emboss_operand_construction().is_some() && ordinal != 8 {
                continue;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(group) = parse_extrude_selection_group(bytes, scope, ordinal, header)? {
                push_extrude_selection_group(&mut out, group, &entry.name, header.byte_offset);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn push_extrude_selection_group(
    out: &mut Vec<DesignExtrudeSelectionGroup>,
    mut group: DesignExtrudeSelectionGroup,
    stream: &str,
    offset: u64,
) {
    group.id = design_record_id(stream, ":design-extrude-selection-group#", offset);
    out.push(group);
}

/// Decode counted construction-operand groups named by feature scopes.
///
/// A scope reference member whose record opens the group grammar but does not
/// close it is recorded on the owning scope, so a group the grammar cannot read
/// is distinguishable from a reference member that is not a group at all.
pub(crate) fn decode_construction_operand_groups(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandGroup>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut out = Vec::new();
    for scope in scopes.iter_mut().filter(|scope| {
        design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Coil)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Loft)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sweep)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Pipe)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::OffsetFaces)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Revolve)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Shell)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Thicken)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Move)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::SurfacePatch)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::SurfaceRuled)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::BoundaryFill)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Split)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Draft)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::ReplaceFace)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::SurfaceOffset)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::SurfaceTrim)
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Scale)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::CircularPattern)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::RectangularPattern)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Mirror)
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::RemoveBody
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::MoveFace
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::DeplacerFace
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::DeleteFace
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal
            || (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Emboss
                && scope.emboss_operand_construction().is_some())
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Thread
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
            || matches!(
                scope.kind(),
                crate::records::feature::scope::DesignFeatureKind::BaseFlange
                    | crate::records::feature::scope::DesignFeatureKind::EdgeFlange
                    | crate::records::feature::scope::DesignFeatureKind::Hem
            )
            || has_typed_edge_treatment_group(&scope.kind())
    }) {
        let scope_group_start = out.len();
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut unclosed = Vec::new();
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            if scope.emboss_operand_construction().is_some() && !matches!(ordinal, 0 | 2) {
                continue;
            }
            let (Ok(ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            match parse_construction_operand_group(
                bytes,
                scope,
                ordinal,
                &RecordFrame::from(*header),
            ) {
                ConstructionOperandGroupParse::Complete(group) => {
                    push_construction_operand_group(
                        &mut out,
                        group,
                        &entry.name,
                        header.byte_offset,
                    );
                }
                ConstructionOperandGroupParse::Unclosed => {
                    unclosed.push(record_index);
                }
                ConstructionOperandGroupParse::NotAGroup => {}
            }
        }
        scope.unclosed_construction_operand_groups = unclosed;
        if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude) {
            assign_extrude_face_roles(scope, &mut out[scope_group_start..]);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn push_construction_operand_group(
    out: &mut Vec<DesignConstructionOperandGroup>,
    mut group: Box<DesignConstructionOperandGroup>,
    stream: &str,
    offset: u64,
) {
    group.id = design_record_id(stream, ":design-construction-operand-group#", offset);
    out.push(*group);
}

/// Decode the fixed role-less body carrier used by the legacy Boolean-Loft
/// envelopes. The ordinary role-`0x8` body group is admitted only when this
/// exact carrier is present at scope-reference ordinal zero.
pub(crate) fn decode_loft_legacy_body_carriers(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignLoftLegacyBodyCarrier>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| {
        matches!(
                &scope.payload(),
                crate::records::feature::scope::DesignScopePayload::Loft(Some(crate::records::feature::path_features::DesignLoftConstruction { operation, .. }))
                    if *operation != crate::records::feature::extrude::DesignExtrudeOperation::NewBody
            )
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            if ordinal != 0 {
                continue;
            }
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Some(carrier) = parse_loft_legacy_body_carrier(bytes, scope, header) else {
                continue;
            };
            push_loft_legacy_body_carrier(&mut out, carrier, &entry.name, header.byte_offset);
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    out.dedup_by(|left, right| left.id == right.id);
    Ok(out)
}

fn push_loft_legacy_body_carrier(
    out: &mut Vec<DesignLoftLegacyBodyCarrier>,
    mut carrier: DesignLoftLegacyBodyCarrier,
    stream: &str,
    offset: u64,
) {
    carrier.id = design_record_id(stream, ":design-loft-legacy-body-carrier#", offset);
    out.push(carrier);
}

/// Parse one class-`322`/`262` or class-`411`/`266` legacy Loft body carrier.
fn parse_loft_legacy_body_carrier(
    bytes: &[u8],
    scope: &DesignParameterScope,
    header: &DesignRecordHeader,
) -> Option<DesignLoftLegacyBodyCarrier> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let (paired_class, frame_length, has_trailing_scope) = match header.class_tag.as_str() {
        "322" => {
            let short_paired_offset = start.checked_add(legacy_loft_322::LEN)?;
            let long_paired_offset = start.checked_add(legacy_loft_322_tail::LEN)?;
            let pair_matches = |at: usize| {
                indexed_record_header_at(bytes, at).is_some_and(|paired| {
                    paired.record_index == header.record_index && paired.class_tag.as_str() == "262"
                })
            };
            let short_pair_matches = pair_matches(short_paired_offset);
            let long_pair_matches = pair_matches(long_paired_offset);
            if short_pair_matches {
                ("262", legacy_loft_322::LEN, false)
            } else if long_pair_matches {
                ("262", legacy_loft_322_tail::LEN, true)
            } else {
                return None;
            }
        }
        "411" => ("266", legacy_loft_411::LEN, true),
        _ => return None,
    };
    if indexed_record_header_at(bytes, start)
        .is_none_or(|parsed| parsed.record_index != header.record_index)
        || bytes
            .get(start + legacy_loft_322::ZERO_RUN_10..start + legacy_loft_322::ZERO_RUN_10 + 10)?
            != [0; 10]
        || bytes.get(start + legacy_loft_322::PRESENCE)? != &1
        || View::u32_le_at(bytes, start + legacy_loft_322::OWNER_SCOPE_RECORD_INDEX)?
            != scope.record_index
        || bytes
            .get(start + legacy_loft_322::ZERO_RUN_6..start + legacy_loft_322::ZERO_RUN_6 + 6)?
            != [0; 6]
        || View::u32_le_at(bytes, start + legacy_loft_322::MEMBER_COUNT)? != 1
    {
        return None;
    }
    let mut cursor = start.checked_add(legacy_loft_322::MEMBER_REFERENCE)?;
    let member_offset = cursor;
    let (member, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::OPAQUE_INDEX)? {
        return None;
    }
    let opaque_index = u8::try_from(View::u32_le_at(bytes, cursor)?)
        .ok()
        .and_then(std::num::NonZeroU8::new)?;
    cursor = cursor.checked_add(4)?;
    let opaque_scalar = cadmpeg_ir::scalar::FiniteReal::new(View::f64_le_at(bytes, cursor)?)?;
    cursor = cursor.checked_add(8)?;
    let repeated_opaque_index = View::u32_le_at(bytes, cursor)?;
    if repeated_opaque_index != u32::from(opaque_index.get()) {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let (next_next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::FLAGS)?
        || bytes.get(cursor..cursor + 2)? != [0, 0]
    {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::LEN)? {
        return None;
    }
    let trailing_scope_reference_offset = if has_trailing_scope {
        if cursor != start + legacy_loft_322_tail::TAIL_ZERO || bytes.get(cursor)? != &0 {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        if cursor != start + legacy_loft_322_tail::TRAILING_SCOPE_REFERENCE {
            return None;
        }
        let reference_offset = cursor;
        let (record_index, _) = take_record_reference(bytes, &mut cursor)?;
        if record_index != scope.record_index {
            return None;
        }
        Some(u64::try_from(reference_offset).ok()?)
    } else {
        None
    };
    let paired_byte_offset = start.checked_add(frame_length)?;
    if cursor != paired_byte_offset
        || indexed_record_header_at(bytes, paired_byte_offset)
            .is_none_or(|parsed| parsed.record_index != header.record_index)
    {
        return None;
    }
    let paired_class_tag = std::str::from_utf8(bytes.get(
        paired_byte_offset + indexed_header::CLASS_TAG
            ..paired_byte_offset + indexed_header::CLASS_TAG + 3,
    )?)
    .ok()?;
    if paired_class_tag != paired_class {
        return None;
    }
    Some(DesignLoftLegacyBodyCarrier {
        id: String::new(),
        scope_record_index: scope.record_index,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        owner_scope_record_index_offset: u64::try_from(
            start + legacy_loft_322::OWNER_SCOPE_RECORD_INDEX,
        )
        .ok()?,
        member,
        member_offset: u64::try_from(member_offset).ok()?,
        member_count_offset: u64::try_from(start + legacy_loft_322::MEMBER_COUNT).ok()?,
        opaque_index,
        opaque_index_offset: u64::try_from(start + legacy_loft_322::OPAQUE_INDEX).ok()?,
        opaque_scalar,
        opaque_scalar_offset: u64::try_from(start + legacy_loft_322::OPAQUE_SCALAR).ok()?,
        repeated_opaque_index_offset: u64::try_from(start + legacy_loft_322::REPEATED_OPAQUE_INDEX)
            .ok()?,
        next_next_record_index,
        next_next_reference_offset: u64::try_from(start + legacy_loft_322::NEXT_NEXT_REFERENCE)
            .ok()?,
        flags_offset: u64::try_from(start + legacy_loft_322::FLAGS).ok()?,
        next_record_index,
        next_reference_offset: u64::try_from(start + legacy_loft_322::NEXT_REFERENCE).ok()?,
        trailing_scope_reference_offset,
        paired_class_tag: paired_class_tag.to_owned().try_into().ok()?,
        paired_byte_offset: u64::try_from(paired_byte_offset).ok()?,
    })
}

pub(in crate::design) fn assign_extrude_face_roles(
    scope: &DesignParameterScope,
    groups: &mut [DesignConstructionOperandGroup],
) {
    let mut face_groups =
        groups
            .iter_mut()
            .filter_map(|group| match extrude_operand_role(scope, group.role()) {
                Some(PendingExtrudeRole::Faces(encoding)) => Some((group, encoding)),
                _ => None,
            });
    if scope.extrude_prologue().map(DesignExtrudePrologue::start)
        == Some(DesignExtrudeStart::FromFace)
    {
        if let Some((group, encoding)) = face_groups.next() {
            group.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
                encoding,
                usage: DesignExtrudeFaceRole::Start,
            };
        }
    }
    for (group, encoding) in face_groups {
        group.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
            encoding,
            usage: DesignExtrudeFaceRole::Termination,
        };
    }
}

/// Return the Extrude groups that select bodies to their source role.
///
/// A start-from-face Extrude reads every `0x5` group after the first as a
/// termination face run. A `0x5` group whose members are all body recipe
/// operands selects bodies, not faces, so it is not a face run.
pub(crate) fn release_extrude_body_target_groups(
    groups: &mut [DesignConstructionOperandGroup],
    operands: &[DesignBodyRecipeOperand],
) {
    for group in groups.iter_mut() {
        let DesignConstructionOperandRole::ExtrudeFaces {
            encoding: DesignExtrudeFaceEncoding::SelectedStart,
            usage: DesignExtrudeFaceRole::Termination,
        } = group.operand_role
        else {
            continue;
        };
        let stream = native_stream(&group.id);
        let selects_bodies = !group.members().is_empty()
            && group.members().iter().enumerate().all(|(ordinal, member)| {
                u32::try_from(ordinal).is_ok_and(|ordinal| {
                    operands.iter().any(|operand| {
                        native_stream(&operand.id) == stream
                            && operand.scope_record_index == group.scope_record_index
                            && operand.owner.group() == Some((group.record_index, ordinal))
                            && operand.record_index() == member.value
                    })
                })
            });
        if selects_bodies {
            group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
        }
    }
}

/// Pair Fillet construction-operand groups with their radius inputs.
fn push_fillet_radius_group(
    out: &mut Vec<DesignFilletRadiusGroup>,
    stream: &str,
    group_ordinal: u32,
    group: &DesignConstructionOperandGroup,
    law: DesignFilletRadiusLaw,
    tangency_weight_parameter_record_index: Option<u32>,
) {
    const SUFFIX: &str = ":design-fillet-radius-group#";

    let edge_operand_record_indices = group
        .members()
        .iter()
        .map(|member| member.value)
        .collect::<Vec<_>>();
    let id = format!("{stream}{SUFFIX}{}", group.record_index);
    out.push(DesignFilletRadiusGroup {
        id,
        scope_record_index: group.scope_record_index,
        group_ordinal,
        group_record_index: group.record_index,
        edge_operand_record_indices,
        law,
        tangency_weight_parameter_record_index,
    });
}

/// The edge group and the face group of a Fillet selection set that stores its
/// face selections in a separate group, in scope-reference order.
pub(crate) fn fillet_edge_and_face_groups<'a>(
    scope_groups: &[&'a DesignConstructionOperandGroup],
) -> Option<[&'a DesignConstructionOperandGroup; 2]> {
    let [first, second] = scope_groups else {
        return None;
    };
    let roles = [first.role(), second.role()];
    (roles.contains(&DesignOperandRole::BODIES_B) && roles.contains(&DesignOperandRole::ROLE_0X10))
        .then_some([*first, *second])
}

/// Whether fixed scalar laws leave several construction groups without a pairing.
pub(crate) fn fixed_fillet_assignment_is_unresolved(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> bool {
    let Some(fixed) = scope.fixed_fillet_parameters() else {
        return false;
    };
    let stream = native_stream(&scope.id);
    let scope_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == stream && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    scope_groups.len() > 1
        && scope_groups.len() != fixed.groups.len()
        && !(fixed.groups.len() == 1 && fillet_edge_and_face_groups(&scope_groups).is_some())
}

pub(crate) fn fixed_fillet_assignment_losses(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    scopes.iter().filter(|scope| fixed_fillet_assignment_is_unresolved(scope, groups))
        .map(|scope| crate::loss::F3dLossCode::FeatureFixedFilletAssignmentNative.note(format!(
            "scope {} retains its fixed Fillet scalar lanes and operand groups because their radius assignment is unresolved", scope.id
        )))
        .collect()
}

pub(crate) fn decode_fillet_radius_groups(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Vec<DesignFilletRadiusGroup> {
    let mut parameter_index = HashMap::new();
    for parameter in parameters {
        let Some(stream) = native_stream(&parameter.id) else {
            continue;
        };
        let key = (stream, parameter.record_index);
        parameter_index.insert(key, parameter);
    }
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Fillet))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let mut scope_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        scope_groups.sort_by_key(|group| group.scope_reference_ordinal);
        let mut owned_parameters = owners
            .iter()
            .filter(|owner| native_stream(owner.id()) == Some(stream) && scope.names_owner(owner))
            .filter_map(|owner| {
                Some((
                    owner.local_ordinal(),
                    *parameter_index.get(&(stream, owner.parameter_record_index()))?,
                ))
            })
            .collect::<Vec<_>>();
        owned_parameters.sort_by_key(|(ordinal, _)| *ordinal);
        let radii = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind() == "Radius").then_some(*parameter)
            })
            .collect::<Vec<_>>();
        let weights = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind() == "TangencyWeight").then_some(*parameter)
            })
            .collect::<Vec<_>>();
        if owned_parameters.len() == radii.len() + weights.len()
            && scope_groups.len() == radii.len()
            && (weights.is_empty() || weights.len() == scope_groups.len())
        {
            for (ordinal, (group, radius)) in scope_groups.into_iter().zip(radii).enumerate() {
                let Ok(group_ordinal) = u32::try_from(ordinal) else {
                    continue;
                };
                push_fillet_radius_group(
                    &mut out,
                    stream,
                    group_ordinal,
                    group,
                    DesignFilletRadiusLaw::Constant {
                        radius_parameter_record_index: radius.record_index,
                    },
                    weights.get(ordinal).map(|parameter| parameter.record_index),
                );
            }
            continue;
        }
        // Several edge sets with face selections: the edge groups pair with
        // the radii in scope-reference order. A face group states no radius
        // of its own and receives no assignment.
        let edge_groups = scope_groups
            .iter()
            .copied()
            .filter(|group| group.role() == DesignOperandRole::BODIES_B)
            .collect::<Vec<_>>();
        let face_group_count = scope_groups
            .iter()
            .filter(|group| group.role() == DesignOperandRole::ROLE_0X10)
            .count();
        if edge_groups.len() > 1
            && face_group_count > 0
            && edge_groups.len() + face_group_count == scope_groups.len()
            && owned_parameters.len() == radii.len() + weights.len()
            && edge_groups.len() == radii.len()
            && (weights.is_empty() || weights.len() == edge_groups.len())
        {
            for (ordinal, (group, radius)) in edge_groups.into_iter().zip(radii).enumerate() {
                let Ok(group_ordinal) = u32::try_from(ordinal) else {
                    continue;
                };
                push_fillet_radius_group(
                    &mut out,
                    stream,
                    group_ordinal,
                    group,
                    DesignFilletRadiusLaw::Constant {
                        radius_parameter_record_index: radius.record_index,
                    },
                    weights.get(ordinal).map(|parameter| parameter.record_index),
                );
            }
            continue;
        }
        if let (Some(pair), [radius]) =
            (fillet_edge_and_face_groups(&scope_groups), radii.as_slice())
        {
            if owned_parameters.len() == 1 + weights.len() && weights.len() <= 1 {
                for (ordinal, group) in (0_u32..).zip(pair) {
                    push_fillet_radius_group(
                        &mut out,
                        stream,
                        ordinal,
                        group,
                        DesignFilletRadiusLaw::Constant {
                            radius_parameter_record_index: radius.record_index,
                        },
                        weights.first().map(|parameter| parameter.record_index),
                    );
                }
                continue;
            }
        }
        if scope_groups.len() > 1 {
            if let Some(assignments) =
                fillet_variable_groups::assign(stream, &scope_groups, &owned_parameters)
            {
                out.extend(assignments);
            }
            continue;
        }
        let [group] = scope_groups.as_slice() else {
            continue;
        };
        let chord_lengths = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind() == "ChordLen").then_some(parameter.record_index)
            })
            .collect::<Vec<_>>();
        // TangencyWeight is optional for the chordal law; older records carry
        // only the required ChordLen input.
        if (weights.is_empty() && owned_parameters.len() == 1)
            || (weights.len() == 1 && owned_parameters.len() == 2)
        {
            let [chord_length] = chord_lengths.as_slice() else {
                continue;
            };
            push_fillet_radius_group(
                &mut out,
                stream,
                0,
                group,
                DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index: *chord_length,
                },
                weights.first().map(|parameter| parameter.record_index),
            );
            continue;
        }
        let asymmetric_offsets = |kind: &str| {
            owned_parameters
                .iter()
                .filter_map(|(_, parameter)| {
                    (parameter.source_kind() == kind).then_some(parameter.record_index)
                })
                .collect::<Vec<_>>()
        };
        let (offset_one, offset_two) = (
            asymmetric_offsets("EdgeOffset1"),
            asymmetric_offsets("EdgeOffset2"),
        );
        if owned_parameters.len() == 3 {
            if let ([offset_one], [offset_two], [weight]) = (
                offset_one.as_slice(),
                offset_two.as_slice(),
                weights.as_slice(),
            ) {
                push_fillet_radius_group(
                    &mut out,
                    stream,
                    0,
                    group,
                    DesignFilletRadiusLaw::Asymmetric {
                        offset_one_parameter_record_index: *offset_one,
                        offset_two_parameter_record_index: *offset_two,
                    },
                    Some(weight.record_index),
                );
                continue;
            }
        }
        let records = |kind: &str| {
            owned_parameters
                .iter()
                .filter_map(|(_, parameter)| {
                    (parameter.source_kind() == kind).then_some(parameter.record_index)
                })
                .collect::<Vec<_>>()
        };
        let (start, end, middle_radii, middle_parameters) = (
            records("StartRadius"),
            records("EndRadius"),
            records("MidRadius"),
            records("MidParams"),
        );
        let ([start], [end]) = (start.as_slice(), end.as_slice()) else {
            continue;
        };
        // TangencyWeight is optional for the variable-radius law. Older
        // records carry only the endpoint and midpoint radius parameters.
        let variable_parameter_count =
            2 + middle_radii.len() + middle_parameters.len() + weights.len();
        if middle_radii.len() != middle_parameters.len()
            || weights.len() > 1
            || owned_parameters.len() != variable_parameter_count
        {
            continue;
        }
        let middle = middle_radii
            .into_iter()
            .zip(middle_parameters)
            .map(|(radius_parameter_record_index, parameter_record_index)| {
                crate::records::topology::fillet::DesignFilletMidpoint {
                    radius_parameter_record_index,
                    parameter_record_index,
                }
            })
            .collect::<Vec<_>>();
        push_fillet_radius_group(
            &mut out,
            stream,
            0,
            group,
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index: *start,
                end_radius_parameter_record_index: *end,
                middle,
            },
            weights.first().map(|parameter| parameter.record_index),
        );
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Remove fixed Fillet interpretations of frames that are indexed parameter owners.
pub(crate) fn disambiguate_fixed_fillet_parameters(
    scopes: &mut [DesignParameterScope],
    owners: &[DesignParameterOwner],
) {
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        if owners
            .iter()
            .any(|owner| native_stream(owner.id()) == Some(stream) && scope.names_owner(owner))
        {
            if let crate::records::feature::scope::DesignScopePayloadMut::Fillet(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Conge(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Abrundung(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Arredondamento(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Empalme(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::FilletJa(slot) =
                scope.payload_mut()
            {
                *slot = None;
            }
        }
    }
}

/// Outcome of reading a scope reference member as a construction-operand group.
pub(super) enum ConstructionOperandGroupParse {
    /// The record does not open a construction-operand group.
    NotAGroup,
    /// The record opens a group the grammar does not close.
    Unclosed,
    /// A complete group.
    Complete(Box<DesignConstructionOperandGroup>),
}

impl ConstructionOperandGroupParse {
    /// The group, where the record carried a complete one.
    #[cfg(test)]
    fn complete(self) -> Option<DesignConstructionOperandGroup> {
        match self {
            Self::Complete(group) => Some(*group),
            Self::NotAGroup | Self::Unclosed => None,
        }
    }
}

/// Extrude operand role before the ordered face groups are separated into
/// start and termination uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingExtrudeRole {
    BodiesA,
    BodiesB,
    Profile,
    Faces(DesignExtrudeFaceEncoding),
}

/// Interpret the role of a counted group owned by an Extrude scope.
///
/// The `0x12` face-group role is a legacy spelling of the one-sided-to-face
/// termination group. Class-296 two-sided-to-faces scopes use the same role
/// for their termination groups. It is also a valid Thicken role, so the
/// extent and exact-layout gates are part of this admission rule rather than
/// a global role alias.
pub(crate) fn extrude_has_selected_termination(scope: &DesignParameterScope) -> bool {
    matches!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::ReferenceAware {
            extent: DesignExtrudeExtent::OneSidedToFace | DesignExtrudeExtent::TwoSidedToFaces,
            start: DesignExtrudeStart::ProfilePlane,
            first_side_target_ordinal: None,
            ..
        })
    )
}

fn extrude_operand_role(
    scope: &DesignParameterScope,
    role: DesignOperandRole,
) -> Option<PendingExtrudeRole> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Extrude) {
        return None;
    }
    match role {
        DesignOperandRole::BODIES_A => Some(PendingExtrudeRole::BodiesA),
        DesignOperandRole::BODIES_B => Some(PendingExtrudeRole::BodiesB),
        DesignOperandRole::PROFILE => Some(PendingExtrudeRole::Profile),
        DesignOperandRole::FACES => {
            Some(PendingExtrudeRole::Faces(DesignExtrudeFaceEncoding::Faces))
        }
        DesignOperandRole::ROLE_0X5
            if scope.extrude_prologue().map(DesignExtrudePrologue::start)
                == Some(DesignExtrudeStart::FromFace) =>
        {
            Some(PendingExtrudeRole::Faces(
                DesignExtrudeFaceEncoding::SelectedStart,
            ))
        }
        DesignOperandRole::ROLE_0X5 if extrude_has_selected_termination(scope) => Some(
            PendingExtrudeRole::Faces(DesignExtrudeFaceEncoding::SelectedStart),
        ),
        DesignOperandRole::ROLE_0X12
            if scope
                .extrude_prologue()
                .and_then(DesignExtrudePrologue::extent)
                == Some(DesignExtrudeExtent::OneSidedToFace) =>
        {
            Some(PendingExtrudeRole::Faces(
                DesignExtrudeFaceEncoding::LegacyTermination,
            ))
        }
        DesignOperandRole::ROLE_0X12 if is_class_296_two_sided_to_faces_scope(scope) => Some(
            PendingExtrudeRole::Faces(DesignExtrudeFaceEncoding::LegacyTermination),
        ),
        _ => None,
    }
}

/// Indexed frame identity and stream position.
#[derive(Clone, Debug)]
pub(super) struct RecordFrame {
    pub(super) record_index: u32,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(super) byte_offset: u64,
}

impl From<&DesignRecordHeader> for RecordFrame {
    fn from(header: &DesignRecordHeader) -> Self {
        Self {
            record_index: header.record_index,
            class_tag: header.class_tag.clone(),
            byte_offset: header.byte_offset,
        }
    }
}

/// Read the construction-operand group at `header`.
///
/// The record's members are a leading-block presence byte, the property block
/// its presence byte gates, the counted member run, two optional references,
/// the counted trailing-reference run, a zero u32 and the u32 role, ten zero bytes, an
/// ordinal, a duration, and a repeat of the ordinal that one container
/// generation omits. The class-328 Move form has one null and one present
/// auxiliary reference, a zero trailing count, and a retained null trailing
/// slot before the role. The ordinary tail is a reference to record `N + 2`,
/// a flag block, a reference to record `N + 1`, a zero byte, the owning scope's
/// reference, and the same-index paired header. The class-328 Move tail has a
/// leading zero, the flag block, an unmarked `N + 1` u64 reference with three
/// zero bytes, and the owning-scope reference. The flag block's last byte is
/// zero; one container generation prefixes it with a further byte. Neither
/// the repeated ordinal nor the prefix byte is announced, so the tail settles
/// both: exactly one of the four readings reaches a paired header carrying
/// this record's own index.
pub(super) fn parse_construction_operand_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &RecordFrame,
) -> ConstructionOperandGroupParse {
    use ConstructionOperandGroupParse::{Complete, NotAGroup, Unclosed};

    let Ok(start) = usize::try_from(header.byte_offset) else {
        return NotAGroup;
    };
    // The indexed header is the three-digit class tag, the u64 entity id whose
    // low word is the record index, and the record's own empty name.
    if bytes.get(start + 11..start + 19) != Some(&[0; 8]) {
        return NotAGroup;
    }
    let Some(mut cursor) = payload_prologue(bytes, start + 19, bytes.len()) else {
        return NotAGroup;
    };
    let member_count_at = cursor;
    let Some(member_count) = View::u32_le_at(bytes, cursor) else {
        return NotAGroup;
    };
    cursor += 4;
    // A reference is at least one byte, so a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if bytes
        .len()
        .checked_sub(cursor)
        .is_none_or(|remaining| index_from_u32(member_count) > remaining)
    {
        return NotAGroup;
    }
    let members_at = cursor;
    for _ in 0..member_count {
        if take_record_reference(bytes, &mut cursor).is_none() {
            return NotAGroup;
        }
    }
    let mut auxiliary_slots = [None; 2];
    let mut auxiliary_reference_slots = [false; 2];
    for (present, slot) in auxiliary_reference_slots
        .iter_mut()
        .zip(&mut auxiliary_slots)
    {
        if bytes.get(cursor) == Some(&0) {
            cursor += 1;
            continue;
        }
        *present = true;
        let Some((record_index, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };

        *slot = Some(crate::records::identity::Located {
            value: record_index,
            offset,
        });
    }
    let Some(trailing_count) = View::u32_le_at(bytes, cursor) else {
        return NotAGroup;
    };
    cursor += 4;
    if bytes
        .len()
        .checked_sub(cursor)
        .is_none_or(|remaining| index_from_u32(trailing_count) > remaining)
    {
        return NotAGroup;
    }
    let trailing_at = cursor;
    for _ in 0..trailing_count {
        if take_record_reference(bytes, &mut cursor).is_none() {
            return NotAGroup;
        }
    }
    let legacy_move_class_328 = scope.kind()
        == crate::records::feature::scope::DesignFeatureKind::Move
        && header.class_tag.as_str() == "328"
        && auxiliary_reference_slots == [false, true]
        && header.record_index.checked_add(13).is_some_and(|expected| {
            auxiliary_slots[1].is_some_and(|record| record.value == expected)
        })
        && trailing_count == 0;
    if legacy_move_class_328 {
        if bytes.get(cursor) != Some(&0) {
            return NotAGroup;
        }
        cursor += 1;
    }
    // The role occupies the high word of a u64 whose low word is zero.
    let role_at = cursor;
    let (Some(0), Some(role)) = (
        View::u32_le_at(bytes, role_at),
        View::u64_le_at(bytes, role_at),
    ) else {
        return NotAGroup;
    };
    let role = DesignOperandRole::from_raw(role);
    cursor += 8;
    if bytes.get(cursor..cursor + 10) != Some(&[0; 10]) {
        return NotAGroup;
    }
    cursor += 10;
    let opaque_index_at = cursor;
    let (Some(opaque_index), Some(opaque_scalar)) = (
        View::u32_le_at(bytes, cursor),
        View::f64_le_at(bytes, cursor + 4),
    ) else {
        return NotAGroup;
    };
    let Some(opaque_index) = std::num::NonZeroU32::new(opaque_index) else {
        return NotAGroup;
    };
    let Some(opaque_scalar) = cadmpeg_ir::scalar::FiniteReal::new(opaque_scalar) else {
        return NotAGroup;
    };
    cursor += 12;

    // Past this point the record has opened the group grammar, so a tail that
    // does not close is a group this reader cannot name.
    let mut closed = None;
    for repeats_ordinal in [true, false] {
        let mut tail = cursor;
        if repeats_ordinal {
            if View::u32_le_at(bytes, tail) != Some(opaque_index.get()) {
                continue;
            }
            tail += 4;
        }
        if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
            != header.record_index.checked_add(2)
        {
            continue;
        }
        for flag_bytes in [2usize, 3] {
            let Some(flags) = bytes.get(tail..tail + flag_bytes) else {
                continue;
            };
            let closing_flags: &[u8] = if flag_bytes == 3 { &[0, 1] } else { &[0] };
            if !flags
                .last()
                .is_some_and(|last| closing_flags.contains(last))
            {
                continue;
            }
            // The wider block prefixes the narrower one, so the variant flag is
            // always the byte before the terminating zero.
            let variant = flags[flag_bytes - 2] != 0;
            let closing_flag = flags[flag_bytes - 1] != 0;
            let mut after = tail + flag_bytes;
            if take_record_reference(bytes, &mut after).map(|(index, _)| index)
                != header.record_index.checked_add(1)
            {
                continue;
            }
            if bytes.get(after) != Some(&0) {
                continue;
            }
            after += 1;
            if take_record_reference(bytes, &mut after).map(|(index, _)| index)
                != Some(scope.record_index)
            {
                continue;
            }
            let Some((paired_class_tag, after_tag)) =
                lp_ascii_filtered_view(bytes, after, 3..=3, u8::is_ascii_digit)
            else {
                continue;
            };
            if View::u32_le_at(bytes, after_tag) != Some(header.record_index) {
                continue;
            }
            if closed
                .replace((variant, closing_flag, after, paired_class_tag.to_owned()))
                .is_some()
            {
                return Unclosed;
            }
        }
    }
    if let Some((variant, paired_at, paired_class_tag)) =
        legacy_body_group_tail(bytes, scope, header, cursor, opaque_index.get())
    {
        if closed
            .replace((variant, false, paired_at, paired_class_tag))
            .is_some()
        {
            return Unclosed;
        }
    }
    let Some((variant, closing_flag, paired_at, paired_class_tag)) = closed else {
        return Unclosed;
    };

    // Face groups take their start/termination use from their ordered position
    // in the scope, which `assign_extrude_face_roles` resolves once every group
    // of the scope is decoded.
    let operand_role = match extrude_operand_role(scope, role) {
        Some(PendingExtrudeRole::BodiesA) => DesignConstructionOperandRole::ExtrudeBodiesA,
        Some(PendingExtrudeRole::BodiesB) => DesignConstructionOperandRole::ExtrudeBodiesB,
        Some(PendingExtrudeRole::Profile) => DesignConstructionOperandRole::ExtrudeProfile,
        Some(PendingExtrudeRole::Faces(_)) | None => DesignConstructionOperandRole::Other(role),
    };
    let (Ok(member_count_offset), Ok(role_offset), Ok(opaque_index_offset), Ok(paired_byte_offset)) = (
        u64::try_from(member_count_at),
        u64::try_from(role_at),
        u64::try_from(opaque_index_at),
        u64::try_from(paired_at),
    ) else {
        return Unclosed;
    };
    let Ok(paired_class_tag) =
        crate::records::references::DesignClassTag::try_from(paired_class_tag)
    else {
        return Unclosed;
    };
    let Ok(opaque_scalar) = cadmpeg_ir::scalar::NonNegativeReal::try_from(opaque_scalar) else {
        return Unclosed;
    };
    // Candidate records share this prologue. Allocate the counted references
    // only after the role and paired frame establish the complete group grammar.
    if trailing_count > 1 {
        return Unclosed;
    }
    let mut members = Vec::new();
    let mut cursor = members_at;
    for _ in 0..member_count {
        let Some((value, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };
        members.push(crate::records::identity::Located { value, offset });
    }
    let auxiliary_records = auxiliary_slots.into_iter().flatten().collect::<Vec<_>>();
    let mut trailing_records = Vec::new();
    let mut cursor = trailing_at;
    for _ in 0..trailing_count {
        let Some((value, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };
        trailing_records.push(crate::records::identity::Located { value, offset });
    }
    let Ok(frame) = DesignConstructionOperandGroupFrame::from_parts(
        crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
            member_count_offset,
            auxiliary_records,
            auxiliary_paths: Vec::new(),
            trailing_records,
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index,
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset: opaque_index_offset + 4,
            variant,
            closing_flag,
        },
    ) else {
        return Unclosed;
    };
    let Ok(group) = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: String::new(),
            scope_record_index: scope.record_index,
            scope_reference_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            members,
            lost_edge_references: Vec::new(),
            frame,
            operand_role,
            role_offset,
            paired_class_tag,
            paired_byte_offset,
        },
    ) else {
        return Unclosed;
    };
    Complete(Box::new(group))
}

/// Read the legacy Move/RemoveBody tail whose two flag bytes have no
/// terminating zero. The class and feature gates keep this admission separate
/// from the terminated flag-block grammar used by other construction groups.
fn legacy_body_group_tail(
    bytes: &[u8],
    scope: &DesignParameterScope,
    header: &RecordFrame,
    cursor: usize,
    opaque_index: u32,
) -> Option<(bool, usize, String)> {
    let body_scope = design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Move)
        || scope.kind() == crate::records::feature::scope::DesignFeatureKind::RemoveBody;
    let (flag_pair, variant) = match header.class_tag.as_str() {
        "257" | "323" | "338" if body_scope => ([1, 1], true),
        "328" if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Move => {
            ([1, 1], true)
        }
        "282" | "302" if body_scope => ([0, 1], false),
        _ => return None,
    };
    let mut tail = cursor;
    if View::u32_le_at(bytes, tail)? != opaque_index {
        return None;
    }
    tail += 4;
    if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
        != header.record_index.checked_add(2)
    {
        return None;
    }
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Move
        && header.class_tag.as_str() == "328"
    {
        if bytes.get(tail) != Some(&0) {
            return None;
        }
        tail += 1;
        if bytes.get(tail..tail + 2) != Some(&flag_pair) {
            return None;
        }
        tail += 2;
        if View::u64_le_at(bytes, tail)? != u64::from(header.record_index.checked_add(1)?)
            || bytes.get(tail + 8..tail + 11) != Some(&[0; 3])
        {
            return None;
        }
        tail += 11;
    } else {
        if bytes.get(tail..tail + 2) != Some(&flag_pair) {
            return None;
        }
        tail += 2;
        if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
            != header.record_index.checked_add(1)
        {
            return None;
        }
        if bytes.get(tail) != Some(&0) {
            return None;
        }
        tail += 1;
    }
    if take_record_reference(bytes, &mut tail).map(|(index, _)| index) != Some(scope.record_index) {
        return None;
    }
    let (paired_class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, tail, 3..=3, u8::is_ascii_digit)?;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Move
        && header.class_tag.as_str() == "328"
        && paired_class_tag != "263"
    {
        return None;
    }
    if View::u32_le_at(bytes, after_tag) != Some(header.record_index) {
        return None;
    }
    Some((variant, tail, paired_class_tag.to_owned()))
}

fn construction_tracking_registry<'cache>(
    scan: &ContainerScan,
    bulk_name: &str,
    registries: &'cache mut HashMap<String, Option<tracking_registration::Registry>>,
) -> Result<Option<&'cache tracking_registration::Registry>, CodecError> {
    let registry = match registries.entry(bulk_name.to_owned()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(slot) => slot.insert(
            super::meta::metadata_for_bulk_stream(scan, bulk_name)?
                .map(tracking_registration::Registry::new),
        ),
    };
    Ok(registry.as_ref())
}

/// Bind exact typed records selected by construction-group trailing runs.
pub(crate) fn bind_construction_operand_trailing_records(
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut registries = HashMap::new();
    for group in groups {
        group
            .frame
            .try_set_trailing_transforms(Vec::new())
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_dual_transforms(Vec::new())
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_flags(Vec::new())
            .map_err(CodecError::Malformed)?;
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut trailing_transforms = Vec::new();
        let mut trailing_dual_transforms = Vec::new();
        let mut trailing_flags = Vec::new();
        for record in group.frame.trailing_records() {
            let Some(header) = headers.get(&(stream, record.value)) else {
                continue;
            };
            if let Some(transform) = parse_construction_operand_transform(bytes, header) {
                if transform.record_index().checked_add(1)
                    != Some(transform.following_record_index())
                    && !construction_tracking_registry(scan, &entry.name, &mut registries)?
                        .is_some_and(|registry| registry.supports_transform(&transform))
                {
                    continue;
                }
                trailing_transforms.push(transform);
            } else if let Some(transform) = parse_construction_operand_dual_transform(bytes, header)
            {
                trailing_dual_transforms.push(transform);
            } else if let Some(flag) = parse_construction_operand_flag(bytes, header) {
                trailing_flags.push(flag);
            }
        }
        group
            .frame
            .try_set_trailing_transforms(trailing_transforms)
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_dual_transforms(trailing_dual_transforms)
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_flags(trailing_flags)
            .map_err(CodecError::Malformed)?;
    }
    Ok(())
}

fn parse_construction_operand_flag(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::topology::construction::DesignConstructionOperandFlag> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || bytes.get(start + 23) != Some(&0)
    {
        return None;
    }
    let value = match bytes.get(start + 22)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(
        crate::records::topology::construction::DesignConstructionOperandFlag {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            value,
            value_offset: u64::try_from(start + 22).ok()?,
        },
    )
}

/// Bind exact persistent-entity path records selected by construction groups.
pub(crate) fn bind_construction_operand_paths(
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(headers);
    for group in groups {
        group
            .frame
            .try_set_auxiliary_paths(Vec::new())
            .map_err(CodecError::Malformed)?;
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut auxiliary_paths = Vec::new();
        for record in &group.frame.auxiliary_records {
            let Some(header) = headers.get(&(stream, record.value)) else {
                continue;
            };
            if let Some(path) =
                parse_construction_operand_path(bytes, group.scope_record_index, header)
            {
                auxiliary_paths.push(path);
            }
        }
        group
            .frame
            .try_set_auxiliary_paths(auxiliary_paths)
            .map_err(CodecError::Malformed)?;
    }
    Ok(())
}

fn parse_construction_operand_path(
    bytes: &[u8],
    expected_scope_record_index: u32,
    header: &DesignRecordHeader,
) -> Option<crate::records::topology::construction::DesignConstructionOperandPath> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let entity_at =
        if bytes.get(start + 11..start + 21)? == [0; 10] && bytes.get(start + 21) == Some(&1) {
            start + 22
        } else if extended_construction_path_prefix(bytes, start)? {
            start + extended_path::ENTITY_REFERENCE
        } else {
            return None;
        };
    let entity_ref = View::u64_le_at(bytes, entity_at)?;
    let entity_ref_offset = u64::try_from(entity_at).ok()?;
    let placement_at = entity_at + 8;
    let (placement, mut cursor) = if bytes.get(placement_at..placement_at + 3)? == [0; 3] {
        let transform = rigid_transform_at(bytes, placement_at + 3)?;
        if bytes.get(placement_at + 131) != Some(&0) {
            return None;
        }
        (
            crate::records::topology::construction::DesignConstructionPathPlacement::Transform(
                transform,
            ),
            placement_at + 132,
        )
    } else {
        let variant = match bytes.get(placement_at..placement_at + 4)? {
            [0, 0, variant @ (0 | 1), 0] => *variant != 0,
            _ => return None,
        };
        (
            crate::records::topology::construction::DesignConstructionPathPlacement::Compact(
                variant,
            ),
            placement_at + 4,
        )
    };
    let (scope_record_index, scope_record_index_offset) =
        take_record_reference(bytes, &mut cursor)?;
    if scope_record_index != expected_scope_record_index {
        return None;
    }
    let (nested_record_index, nested_record_index_offset) =
        take_record_reference(bytes, &mut cursor)?;
    if nested_record_index != header.record_index.checked_add(2)?
        || bytes.get(cursor..cursor + 6)? != [0; 6]
    {
        return None;
    }
    let following_at = cursor.checked_add(6)?;
    let (following_class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_tag)?;
    if following_record_index != header.record_index.checked_add(1)? {
        return None;
    }
    crate::records::topology::construction::DesignConstructionOperandPath::try_new(
        crate::records::topology::construction::DesignConstructionOperandPathDraft {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            entity_ref,
            entity_ref_offset,
            placement,
            scope_record_index,
            scope_record_index_offset,
            nested_record_index,
            nested_record_index_offset,
            following_record_index,
            following_byte_offset: u64::try_from(following_at).ok()?,
            following_class_tag: crate::design::decode::text::class_tag_from_view(
                following_class_tag,
            )
            .ok()?,
        },
    )
    .ok()
}

fn extended_construction_path_prefix(bytes: &[u8], start: usize) -> Option<bool> {
    let zero_run = start + extended_path::ZERO_RUN;
    let trailing_zero_run = start + extended_path::TRAILING_ZERO_RUN;
    Some(
        bytes
            .get(zero_run..start + extended_path::PREFIX_VALUE)?
            .iter()
            .all(|byte| *byte == 0)
            && View::u32_le_at(bytes, start + extended_path::PREFIX_VALUE)?
                == extended_path::PREFIX_VALUE_VALUE
            && bytes
                .get(trailing_zero_run..start + extended_path::ENTITY_MARKER)?
                .iter()
                .all(|byte| *byte == 0)
            && *bytes.get(start + extended_path::ENTITY_MARKER)?
                == extended_path::ENTITY_MARKER_VALUE,
    )
}

fn parse_construction_operand_transform(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::topology::construction::DesignConstructionOperandTransform> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + indexed_header::LEN..start + trailing_affine::MATRIX)?
        != [0; trailing_affine::MATRIX - indexed_header::LEN]
        || bytes.get(start + trailing_affine::TRAILER..start + trailing_affine::LEN)? != [1, 0]
    {
        return None;
    }
    let transform_at = start.checked_add(trailing_affine::MATRIX)?;
    let transform = rigid_transform_at(bytes, transform_at)?;
    let following_at = start.checked_add(trailing_affine::LEN)?;
    let (following_class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_tag)?;
    if following_record_index <= header.record_index {
        return None;
    }
    crate::records::topology::construction::DesignConstructionOperandTransform::try_new(
        crate::records::topology::construction::DesignConstructionOperandTransformDraft {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            transform,
            transform_offset: u64::try_from(transform_at).ok()?,
            following_record_index,
            following_byte_offset: u64::try_from(following_at).ok()?,
            following_class_tag: crate::design::decode::text::class_tag_from_view(
                following_class_tag,
            )
            .ok()?,
        },
    )
    .ok()
}

fn parse_construction_operand_dual_transform(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::topology::construction::DesignConstructionOperandDualTransform> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10] || bytes.get(start + 277) != Some(&0) {
        return None;
    }
    let first_at = start.checked_add(21)?;
    let second_at = start.checked_add(149)?;
    Some(
        crate::records::topology::construction::DesignConstructionOperandDualTransform {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            first_transform: rigid_transform_at(bytes, first_at)?,
            first_transform_offset: u64::try_from(first_at).ok()?,
            second_transform: rigid_transform_at(bytes, second_at)?,
            second_transform_offset: u64::try_from(second_at).ok()?,
        },
    )
}

fn rigid_transform_at(
    bytes: &[u8],
    at: usize,
) -> Option<crate::records::sketch_placement::SketchPlacementMatrix> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut transform = [[0.0; 4]; 4];
    for row in &mut transform {
        for cell in row {
            *cell = view.f64_le()?;
        }
    }
    transform.try_into().ok()
}

/// Take one reference naming a record of the same segment, advancing `at` past
/// every byte it owns. Returns the record index and the byte offset of the low
/// word of the target entity id.
fn take_record_reference(bytes: &[u8], at: &mut usize) -> Option<(u32, u64)> {
    let target_at = at.checked_add(1)?;
    let reference = take_reference(bytes, at)?;
    let (target, _) = reference.local()?;
    Some((u32::try_from(target).ok()?, u64::try_from(target_at).ok()?))
}

pub(crate) mod compact_owner;
mod tracking_registration;

/// Decode the persistent identity frame named by each construction-operand group.
pub(crate) fn decode_construction_operand_identities(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(Vec<DesignConstructionOperandIdentity>, usize), CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut registries = HashMap::new();
    let mut untyped = 0;
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let Some(trailing_record_index) = group
            .frame
            .trailing_records()
            .first()
            .map(|record| &record.value)
        else {
            continue;
        };
        let Some(wrapper_header) = headers.get(&(stream, *trailing_record_index)) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let registry = construction_tracking_registry(scan, &entry.name, &mut registries)?;
        if let Some(mut identity) =
            parse_construction_operand_identity(bytes, group, wrapper_header, registry)
        {
            if identity
                .tracking_path()
                .is_some_and(|path| !registry.is_some_and(|registry| registry.supports(path)))
            {
                untyped += 1;
                continue;
            }
            identity.id = design_record_id(
                &entry.name,
                ":design-construction-operand-identity#",
                wrapper_header.byte_offset,
            );

            out.push(identity);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|left, right| left.id == right.id);
    Ok((out, untyped))
}

/// Collect one Design stream's lost-edge records in source byte order.
fn collect_stream_lost_edges<'a>(
    lost_edges: &'a [LostEdgeReference],
    stream: &str,
) -> Vec<&'a LostEdgeReference> {
    let mut edges = lost_edges
        .iter()
        .filter(|edge| native_stream(&edge.id) == Some(stream))
        .collect::<Vec<_>>();
    edges.sort_by_key(|edge| edge.record_byte_offset());
    edges
}

/// Bind a contiguous unresolved-edge run to the construction group whose
/// first identity wrapper terminates that run.
///
/// A group binds only when it has one identity chain, one terminating run,
/// and a run cardinality equal to its operand count. Any other group keeps no
/// lost-edge references; its unresolved records stay unrepaired and are
/// reported as lost edge references.
pub(crate) fn bind_lost_edge_groups(
    groups: &mut [DesignConstructionOperandGroup],
    identities: &[DesignConstructionOperandIdentity],
    lost_edges: &[LostEdgeReference],
) {
    for group in groups {
        group.lost_edge_references.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut identity_matches = identities.iter().filter(|identity| {
            native_stream(&identity.id) == Some(stream)
                && identity.group_record_index == group.record_index
        });
        let Some(identity) = identity_matches.next() else {
            continue;
        };
        if identity_matches.next().is_some() {
            continue;
        }
        let Some(wrapper) = identity.wrappers().first() else {
            continue;
        };
        let wrapper_record_index = wrapper.record_index;
        let wrapper_byte_offset = wrapper.byte_offset;
        let wrapper_class_tag = wrapper.class_tag.as_str();
        let stream_edges = collect_stream_lost_edges(lost_edges, stream);
        let mut terminal = None;
        let mut multiple_terminals = false;
        for (ordinal, edge) in stream_edges.iter().enumerate() {
            if edge.next_record_index == wrapper_record_index
                && edge.next_byte_offset() == wrapper_byte_offset
                && edge.next_class_tag.as_str() == wrapper_class_tag
                && terminal.replace(ordinal).is_some()
            {
                multiple_terminals = true;
            }
        }
        if multiple_terminals {
            continue;
        }
        let Some(terminal) = terminal else {
            continue;
        };
        let mut start = terminal;
        while start > 0 {
            let previous = stream_edges[start - 1];
            let current = stream_edges[start];
            if previous.next_byte_offset() != current.record_byte_offset()
                || previous.next_record_index != current.record_index
                || previous.next_class_tag != current.class_tag
            {
                break;
            }
            start -= 1;
        }
        let run = &stream_edges[start..=terminal];
        if run.len() != group.members().len() {
            continue;
        }
        group.lost_edge_references = run.iter().map(|edge| edge.id.clone()).collect();
    }
}

fn parse_construction_operand_identity(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    wrapper_header: &DesignRecordHeader,
    registry: Option<&tracking_registration::Registry>,
) -> Option<DesignConstructionOperandIdentity> {
    let mut current_at = usize::try_from(wrapper_header.byte_offset).ok()?;
    let mut current_record_index = wrapper_header.record_index;
    let mut current_class_tag = wrapper_header.class_tag.clone();
    let mut chain_started = false;
    if let Some(transform) = parse_construction_operand_transform(bytes, wrapper_header) {
        if transform.record_index().checked_add(1) != Some(transform.following_record_index())
            && !registry.is_some_and(|registry| registry.supports_transform(&transform))
        {
            return None;
        }
        current_at = usize::try_from(transform.following_byte_offset()).ok()?;
        current_record_index = transform.following_record_index();
        current_class_tag = transform.following_class_tag;
        chain_started = true;
    }
    let mut wrappers = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if parse_construction_tracking_path(
            bytes,
            current_at,
            current_record_index,
            &current_class_tag,
        )
        .is_some()
            || bytes.get(current_at + 11..current_at + 21)? != [0; 10]
            || bytes.get(current_at + 21..current_at + 24)? != [1, 1, 0]
        {
            break;
        }
        if seen.contains(&(current_record_index, current_at)) {
            return None;
        }

        seen.insert((current_record_index, current_at));

        wrappers.push(
            crate::records::topology::construction::DesignIdentityWrapper {
                record_index: current_record_index,
                byte_offset: u64::try_from(current_at).ok()?,
                class_tag: current_class_tag,
            },
        );
        current_at = current_at.checked_add(24)?;
        let (next_class_tag, after_next_tag) =
            lp_ascii_filtered_view(bytes, current_at, 0..=2000, u8::is_ascii_graphic)?;
        if next_class_tag.len() != 3 {
            return None;
        }
        current_record_index = View::u32_le_at(bytes, after_next_tag)?;
        current_class_tag =
            crate::design::decode::text::class_tag_from_view(next_class_tag).ok()?;
        chain_started = true;
    }
    let tracking_path = parse_construction_tracking_path(
        bytes,
        current_at,
        current_record_index,
        &current_class_tag,
    );
    if let Some(path) = &tracking_path {
        current_at = usize::try_from(path.following_byte_offset()).ok()?;
        current_record_index = path.following_record_index();
        current_class_tag.clone_from(&path.following_class_tag);
        chain_started = true;
    }
    if !chain_started {
        return None;
    }
    let persistent_identity = match parse_extrude_identity_member(bytes, current_at, None) {
        Some(member) => DesignConstructionPersistentIdentity::try_new(
            crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
                local_id: member.local_id,
                local_id_offset: member.local_id_offset,
                asset_id: member.asset_id.try_into().ok()?,
                asset_id_offset: member.asset_id_offset,
                context_id: member.context_id.try_into().ok()?,
                context_id_offset: member.context_id_offset,
                tail_slot_present: member.tail_slot_present,
                tail_slot_offset: member.tail_slot_offset,
                next_record_index: member.next_record_index,
                next_byte_offset: member.next_byte_offset,
            },
        )
        .ok(),
        None => None,
    };
    DesignConstructionOperandIdentity::try_new(
        crate::records::topology::construction::DesignConstructionOperandIdentityDraft {
            id: String::new(),
            group_record_index: group.record_index,
            wrappers,
            following_record_index: current_record_index,
            following_byte_offset: u64::try_from(current_at).ok()?,
            following_class_tag: current_class_tag,
            tracking_path,
            persistent_identity,
        },
    )
    .ok()
}

fn parse_construction_tracking_path(
    bytes: &[u8],
    wrapper_at: usize,
    wrapper_record_index: u32,
    wrapper_class_tag: &crate::records::references::DesignClassTag,
) -> Option<DesignConstructionTrackingPath> {
    if bytes.get(wrapper_at + 11..wrapper_at + 21)? != [0; 10]
        || bytes.get(wrapper_at + 21) != Some(&1)
        || View::u64_le_at(bytes, wrapper_at + 22)?
            != u64::from(wrapper_record_index.checked_add(1)?)
        || bytes.get(wrapper_at + 30..wrapper_at + 33)? != [0; 3]
    {
        return None;
    }
    let carrier_at = wrapper_at.checked_add(33)?;
    let (carrier_class_tag, after_carrier_tag) =
        lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    let carrier_record_index = View::u32_le_at(bytes, after_carrier_tag)?;
    if carrier_record_index != wrapper_record_index.checked_add(1)?
        || bytes.get(carrier_at + 11..carrier_at + 21)? != [0; 10]
        || View::u32_le_at(bytes, carrier_at + 21)? != 1
        || View::u32_le_at(bytes, carrier_at + 25)? != 0
        || View::u32_le_at(bytes, carrier_at + 29)? != 1
        || !matches!(View::u32_le_at(bytes, carrier_at + 33)?, 2 | 3)
        || View::u64_le_at(bytes, carrier_at + 45)? != 0
        || View::u32_le_at(bytes, carrier_at + 53)? != 1
        || View::u64_le_at(bytes, carrier_at + 65)? != 0
    {
        return None;
    }
    let primary_identity = View::u64_le_at(bytes, carrier_at + 37)?;
    let selector = View::i32_le_at(bytes, carrier_at + 57)?;
    let kind = View::u32_le_at(bytes, carrier_at + 61)?;
    let mut cursor = carrier_at.checked_add(73)?;
    let first_related_identity = match take_optional_tracking_identity(bytes, &mut cursor) {
        TrackingIdentityField::Invalid => return None,
        TrackingIdentityField::Absent => None,
        TrackingIdentityField::Present(value) => Some(value),
    };
    let second_related_identity = match take_optional_tracking_identity(bytes, &mut cursor) {
        TrackingIdentityField::Invalid => return None,
        TrackingIdentityField::Absent => None,
        TrackingIdentityField::Present(value) => Some(value),
    };
    let following_at = cursor;
    let (following_class_tag, after_following_tag) =
        lp_ascii_filtered_view(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_following_tag)?;
    if following_record_index != carrier_record_index.checked_add(1)? {
        return None;
    }
    DesignConstructionTrackingPath::try_new(
        crate::records::topology::construction::DesignConstructionTrackingPathDraft {
            wrapper_record_index,
            wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
            wrapper_class_tag: wrapper_class_tag.clone(),
            carrier_record_index,
            carrier_byte_offset: u64::try_from(carrier_at).ok()?,
            carrier_class_tag: crate::design::decode::text::class_tag_from_view(carrier_class_tag)
                .ok()?,
            primary_identity,
            primary_identity_offset: u64::try_from(carrier_at + 37).ok()?,
            selector,
            selector_offset: u64::try_from(carrier_at + 57).ok()?,
            kind,
            kind_offset: u64::try_from(carrier_at + 61).ok()?,
            first_related_identity,
            second_related_identity,
            following_record_index,
            following_byte_offset: u64::try_from(following_at).ok()?,
            following_class_tag: crate::design::decode::text::class_tag_from_view(
                following_class_tag,
            )
            .ok()?,
        },
    )
    .ok()
}

enum TrackingIdentityField {
    Invalid,
    Absent,
    Present(crate::records::identity::Located<u64>),
}

fn take_optional_tracking_identity(bytes: &[u8], cursor: &mut usize) -> TrackingIdentityField {
    match View::u32_le_at(bytes, *cursor) {
        Some(0) => {
            let Some(next) = (*cursor).checked_add(4) else {
                return TrackingIdentityField::Invalid;
            };
            *cursor = next;
            TrackingIdentityField::Absent
        }
        Some(1) => {
            let Some(value_at) = (*cursor).checked_add(4) else {
                return TrackingIdentityField::Invalid;
            };
            let Some(value) = View::u64_le_at(bytes, value_at) else {
                return TrackingIdentityField::Invalid;
            };
            let Some(next) = value_at.checked_add(8) else {
                return TrackingIdentityField::Invalid;
            };
            *cursor = next;
            let Ok(offset) = u64::try_from(value_at) else {
                return TrackingIdentityField::Invalid;
            };
            TrackingIdentityField::Present(crate::records::identity::Located { value, offset })
        }
        _ => TrackingIdentityField::Invalid,
    }
}

fn parse_extrude_selection_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> Result<Option<DesignExtrudeSelectionGroup>, CodecError> {
    let wire = (|| -> Option<
        Result<
            crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire,
            CodecError,
        >,
    > {
        let start = usize::try_from(header.byte_offset).ok()?;
        let scope_reference_at = start + typed_group::SCOPE_REFERENCE;
        if bytes.get(start + typed_group::ZERO_RUN_10..scope_reference_at)? != [0; 10] {
            return None;
        }
        // Every reference of one group uses one spelling; the scope
        // reference selects it.
        let typed = crate::bytes::typed_local_reference_target(bytes, scope_reference_at).is_some();
        let width = if typed {
            crate::bytes::TYPED_LOCAL_REFERENCE_LEN
        } else {
            11
        };
        let reference = |at: usize| crate::bytes::local_reference_target(bytes, at, typed);
        if reference(scope_reference_at)? != scope.record_index {
            return None;
        }
        let member_count_at = scope_reference_at + width;
        let member_count = usize::try_from(View::u32_le_at(bytes, member_count_at)?).ok()?;
        let mut position = member_count_at.checked_add(4)?;
        // A count the remaining bytes cannot supply is corrupt and must not
        // reach the allocator.
        if member_count == 0 || member_count > bytes.len().checked_sub(position)? / width {
            return None;
        }
        let mut members = Vec::new();
        let mut member_offsets = Vec::new();
        for _ in 0..member_count {
            members.push(reference(position)?);
            member_offsets.push(u64::try_from(position + 1).ok()?);
            position = position.checked_add(width)?;
        }
        let opaque_index = View::u32_le_at(bytes, position)?;
        let opaque_scalar = View::f64_le_at(bytes, position + 4)?;
        if View::u32_le_at(bytes, position + 12)? != opaque_index
            || reference(position + 16)? != header.record_index.checked_add(2)?
        {
            return None;
        }
        // The plain form stores `01 + u8 variant + 00` after the first nested
        // reference; the typed form stores two zero bytes and no variant.
        let after_first = position + 16 + width;
        let (variant, second_at) = if typed {
            if bytes.get(after_first..after_first + 2)? != [0; 2] {
                return None;
            }
            (None, after_first + 2)
        } else {
            if bytes.get(after_first) != Some(&1)
                || !matches!(bytes.get(after_first + 1), Some(0 | 1))
                || bytes.get(after_first + 2) != Some(&0)
            {
                return None;
            }
            (Some(bytes[after_first + 1] != 0), after_first + 3)
        };
        let scope_at = second_at + width + 1;
        if reference(second_at)? != header.record_index.checked_add(1)?
            || bytes.get(second_at + width) != Some(&0)
            || reference(scope_at)? != scope.record_index
        {
            return None;
        }
        let paired_at = scope_at.checked_add(width)?;
        let (paired_class_tag, after_paired_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_paired_tag)? != header.record_index {
            return None;
        }
        let paired_class_tag =
            crate::design::decode::text::class_tag_from_view(paired_class_tag).ok()?;
        Some(Ok(
            crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
                id: String::new(),
                scope_record_index: scope.record_index,
                scope_reference_ordinal,
                record_index: header.record_index,
                byte_offset: header.byte_offset,
                class_tag: header.class_tag.clone().into(),
                member_count_offset: u64::try_from(member_count_at).ok()?,
                members,
                member_offsets,
                opaque_index,
                opaque_index_offset: u64::try_from(position).ok()?,
                opaque_scalar,
                opaque_scalar_offset: u64::try_from(position + 4).ok()?,
                variant,
                typed_references: typed,
                paired_class_tag: paired_class_tag.into(),
                paired_byte_offset: u64::try_from(paired_at).ok()?,
            },
        ))
    })();
    let Some(wire) = wire else {
        return Ok(None);
    };
    Ok(DesignExtrudeSelectionGroup::try_from(wire?).ok())
}

/// Decode the fixed-width records named by Extrude selection groups.
pub(crate) fn decode_extrude_selection_members(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignExtrudeSelectionGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionMember>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let extrude_scopes = scopes
        .iter()
        .filter_map(|scope| {
            (design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude))
                .then(|| native_stream(&scope.id).map(|stream| (stream, scope.record_index)))
                .flatten()
        })
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        if !extrude_scopes.contains(&(stream, group.scope_record_index)) {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut member) = parse_extrude_selection_member(bytes, group, ordinal, header)
            {
                member.id = design_record_id(
                    &entry.name,
                    ":design-extrude-selection-member#",
                    header.byte_offset,
                );

                out.push(member);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode nested persistent-entity frames named by construction groups.
pub(crate) fn decode_entity_selection_operands(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEntitySelectionOperand>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut operand) = parse_entity_selection_operand(bytes, group, ordinal, header)
            {
                operand.id = design_record_id(
                    &entry.name,
                    ":design-entity-selection-operand#",
                    header.byte_offset,
                );

                out.push(operand);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(super) fn parse_entity_selection_operand(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignEntitySelectionOperand> {
    let frame = parse_entity_selection_frame(
        bytes,
        header.record_index,
        header.byte_offset,
        header.class_tag.as_str(),
    )?;
    DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: String::new(),
            scope_record_index: group.scope_record_index,
            group_record_index: group.record_index,
            group_member_ordinal,
            record_index: frame.record_index,
            byte_offset: frame.byte_offset,
            class_tag: frame.class_tag,
            asset_id: frame.asset_id.try_into().ok()?,
            asset_id_offset: frame.asset_id_offset,
            context_id: frame.context_id.try_into().ok()?,
            context_id_offset: frame.context_id_offset,
            identity_record_index: frame.identity_record_index,
            identity_record_offset: frame.identity_record_offset,
            primary_identity: frame.primary_identity,
            primary_identity_offset: frame.primary_identity_offset,
            secondary: frame.secondary,
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: frame.next_record_index,
            next_byte_offset: frame.next_byte_offset,
        },
    )
    .ok()
}

/// Persistent identity payload shared by entity-selection consumers that do
/// not belong to a construction-operand group.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::design::decode) struct EntitySelectionFrame {
    pub(super) record_index: u32,
    pub(super) byte_offset: u64,
    class_tag: crate::records::references::DesignClassTag,
    pub(super) asset_id: String,
    pub(super) asset_id_offset: u64,
    pub(super) context_id: String,
    pub(super) context_id_offset: u64,
    pub(super) identity_record_index: u32,
    pub(super) identity_record_offset: u64,
    pub(super) primary_identity: u64,
    pub(super) primary_identity_offset: u64,
    pub(super) secondary: Option<
        crate::records::identity::DesignSecondaryIdentity<crate::records::identity::Located<u64>>,
    >,
    pub(super) next_record_index: u32,
    pub(super) next_byte_offset: u64,
}

pub(in crate::design::decode) struct EntitySelectionPrefix {
    pub(super) asset_id: String,
    pub(super) asset_id_offset: u64,
    pub(super) context_id: String,
    pub(super) context_id_offset: u64,
    after_context_id: usize,
}

pub(super) fn parse_entity_selection_prefix(
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Option<EntitySelectionPrefix> {
    // Persistent selections use a ten-byte prelude and a u32 presence value.
    // Face-recipe selections add two prelude bytes, encode presence as one
    // byte, and add three zero bytes before the first UTF-16 length.
    let asset_start = if bytes.get(start + 11..start + coil_persist_sel::NESTED_SELECTION_MARKER)?
        == [0; 10]
        && bytes.get(start + coil_persist_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_persist_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(start + 26..start + coil_persist_sel::ASSET_PRESENCE)? == [0; 6]
        && View::u32_le_at(bytes, start + coil_persist_sel::ASSET_PRESENCE)? == 1
    {
        start.checked_add(coil_persist_sel::ASSET_UUID_LENGTH)?
    } else if bytes.get(start + 11..start + coil_modern_sel::NESTED_SELECTION_MARKER)? == [0; 11]
        && bytes.get(start + coil_modern_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_modern_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(
            start + coil_modern_sel::NESTED_RECORD_INDEX + 4
                ..start + coil_modern_sel::ASSET_PRESENCE,
        )? == [0; 6]
        && View::u32_le_at(bytes, start + coil_modern_sel::ASSET_PRESENCE)? == 1
    {
        start.checked_add(coil_modern_sel::ASSET_UUID_LENGTH)?
    } else if bytes.get(start + 11..start + coil_face_sel::NESTED_SELECTION_MARKER)? == [0; 12]
        && bytes.get(start + coil_face_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_face_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(start + 28..start + coil_face_sel::ASSET_PRESENCE)? == [0; 6]
        && bytes.get(start + coil_face_sel::ASSET_PRESENCE) == Some(&1)
    {
        start.checked_add(coil_face_sel::ASSET_UUID_LENGTH)?
    } else {
        return None;
    };
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, asset_start, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
        || bytes.get(after_context_id + 4..after_context_id + 8)? != [0; 4]
    {
        return None;
    }
    Some(EntitySelectionPrefix {
        asset_id,
        asset_id_offset: u64::try_from(asset_start.checked_add(4)?).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id.checked_add(4)?).ok()?,
        after_context_id,
    })
}

/// Match the selected curve identity carried by a nested entity-selection frame.
pub(in crate::design) fn entity_selection_matches_curve(
    operand: &DesignEntitySelectionOperand,
    curve: &SketchCurveIdentity,
) -> bool {
    operand.secondary().is_some_and(|secondary| {
        curve.primary_id.get() == secondary.identity.value
            && secondary
                .curve_identity
                .is_none_or(|identity| curve.secondary_id == identity.value)
    })
}

/// Direct sketch-point identity carried by a `WorkPoint` input.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkPointSketchPointFrame {
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    identity_record_index: u32,
    identity_record_offset: u64,
    sketch_record_index: u32,
    sketch_record_index_offset: u64,
    point_persistent_id: u64,
    point_persistent_id_offset: u64,
    next_record_index: u32,
    next_byte_offset: u64,
}

/// Parse the direct sketch-point identity variant of a `WorkPoint` input.
///
/// The outer prefix is shared with persistent entity selections. Its identity
/// record is a separate four-record envelope: nine zero bytes, a one-byte
/// presence marker, two marked `u32` slots separated by zero `u32` values,
/// and the following point-data record.
fn parse_work_point_sketch_point_frame(
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
) -> Option<WorkPointSketchPointFrame> {
    let start = usize::try_from(byte_offset).ok()?;
    let prefix = parse_entity_selection_prefix(bytes, start, record_index)?;
    let paired_at = next_indexed_record_offset(bytes, prefix.after_context_id + 8)?;
    let nested_one_at = next_indexed_record_offset(bytes, paired_at + indexed_header::LEN)?;
    let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + indexed_header::LEN)?;
    let identity_at = next_indexed_record_offset(bytes, nested_two_at + indexed_header::LEN)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + indexed_header::LEN)?;
    let expected = [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ];
    for (offset, expected) in [paired_at, nested_one_at, nested_two_at, identity_at]
        .into_iter()
        .zip(expected)
    {
        let (_, after_tag) = lp_ascii_filtered_view(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_tag)? != expected {
            return None;
        }
    }
    let (_, after_next_tag) =
        lp_ascii_filtered_view(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
    let next_record_index = View::u32_le_at(bytes, after_next_tag)?;
    if next_record_index != record_index.checked_add(4)?
        || bytes
            .get(identity_at + indexed_header::LEN..identity_at + sketch_point_identity::PRESENCE)?
            != [0; 9]
        || bytes.get(identity_at + sketch_point_identity::PRESENCE) != Some(&1)
        || View::u32_le_at(bytes, identity_at + sketch_point_identity::PRESENCE + 1)? != 0
        || View::u32_le_at(
            bytes,
            identity_at + sketch_point_identity::SKETCH_RECORD_INDEX + 4,
        )? != 0
        || identity_at.checked_add(sketch_point_identity::LEN)? != next_at
    {
        return None;
    }
    let sketch_record_index = View::u32_le_at(
        bytes,
        identity_at + sketch_point_identity::SKETCH_RECORD_INDEX,
    )?;
    let point_persistent_id = u64::from(View::u32_le_at(
        bytes,
        identity_at + sketch_point_identity::POINT_PERSISTENT_ID,
    )?);
    Some(WorkPointSketchPointFrame {
        asset_id: prefix.asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id: prefix.context_id,
        context_id_offset: prefix.context_id_offset,
        identity_record_index: record_index.checked_add(3)?,
        identity_record_offset: u64::try_from(identity_at).ok()?,
        sketch_record_index,
        sketch_record_index_offset: u64::try_from(
            identity_at.checked_add(sketch_point_identity::SKETCH_RECORD_INDEX)?,
        )
        .ok()?,
        point_persistent_id,
        point_persistent_id_offset: u64::try_from(
            identity_at.checked_add(sketch_point_identity::POINT_PERSISTENT_ID)?,
        )
        .ok()?,
        next_record_index,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

/// Parse the nested persistent-entity frame without assigning group ownership.
pub(super) fn parse_entity_selection_frame(
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
    class_tag: &str,
) -> Option<EntitySelectionFrame> {
    let start = usize::try_from(byte_offset).ok()?;
    let prefix = parse_entity_selection_prefix(bytes, start, record_index)?;
    let paired_at = next_indexed_record_offset(bytes, prefix.after_context_id + 8)?;
    let nested_one_at = next_indexed_record_offset(bytes, paired_at + 11)?;
    let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + 11)?;
    let identity_at = next_indexed_record_offset(bytes, nested_two_at + 11)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + 11)?;
    let expected = [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ];
    for (offset, expected) in [paired_at, nested_one_at, nested_two_at, identity_at]
        .into_iter()
        .zip(expected)
    {
        let (_, after_tag) = lp_ascii_filtered_view(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_tag)? != expected {
            return None;
        }
    }
    let (identity_class_tag, _) =
        lp_ascii_filtered_view(bytes, identity_at, 0..=2000, u8::is_ascii_graphic)?;
    let (_, after_next_tag) =
        lp_ascii_filtered_view(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
    let next_record_index = View::u32_le_at(bytes, after_next_tag)?;
    let (primary_identity_offset, primary_identity, secondary) = if class_tag == "338"
        && identity_class_tag == "361"
        && bytes.get(
            identity_at + class_338_curve::ZERO_PREFIX..identity_at + class_338_curve::PRESENCE,
        )? == [0; 9]
        && bytes.get(identity_at + class_338_curve::PRESENCE) == Some(&1)
        && bytes.get(
            identity_at + class_338_curve::PRESENCE + 1
                ..identity_at + class_338_curve::OWNER_RECORD_INDEX,
        )? == [0; 12]
        && View::u32_le_at(bytes, identity_at + class_338_curve::OWNER_HIGH_ZERO)? == 0
        && View::u32_le_at(bytes, identity_at + class_338_curve::CURVE_HIGH_ZERO)? == 0
        && identity_at.checked_add(class_338_curve::LEN)? == next_at
        && next_record_index == record_index.checked_add(4)?
    {
        let primary_identity = u64::from(View::u32_le_at(
            bytes,
            identity_at + class_338_curve::OWNER_RECORD_INDEX,
        )?);
        let secondary_identity = u64::from(View::u32_le_at(
            bytes,
            identity_at + class_338_curve::CURVE_PERSISTENT_ID,
        )?);
        (
            identity_at.checked_add(class_338_curve::OWNER_RECORD_INDEX)?,
            primary_identity,
            Some(crate::records::identity::DesignSecondaryIdentity {
                identity: crate::records::identity::Located {
                    value: secondary_identity,
                    offset: u64::try_from(
                        identity_at.checked_add(class_338_curve::CURVE_PERSISTENT_ID)?,
                    )
                    .ok()?,
                },
                curve_identity: None,
            }),
        )
    } else if bytes.get(identity_at + 11..identity_at + 21)? == [0; 10]
        && identity_at.checked_add(45)? == next_at
    {
        let curve_secondary_identity_offset = identity_at.checked_add(21)?;
        let primary_identity_offset = identity_at.checked_add(29)?;
        let secondary_identity_offset = identity_at.checked_add(37)?;
        (
            primary_identity_offset,
            View::u64_le_at(bytes, primary_identity_offset)?,
            Some(crate::records::identity::DesignSecondaryIdentity {
                identity: crate::records::identity::Located {
                    value: View::u64_le_at(bytes, secondary_identity_offset)?,
                    offset: u64::try_from(secondary_identity_offset).ok()?,
                },
                curve_identity: Some(crate::records::identity::Located {
                    value: View::u64_le_at(bytes, curve_secondary_identity_offset)?,
                    offset: u64::try_from(curve_secondary_identity_offset).ok()?,
                })
                .filter(|identity| identity.value != 0),
            }),
        )
    } else if bytes.get(identity_at + 11..identity_at + 21)? == [0; 10]
        && identity_at.checked_add(29)? == next_at
    {
        let primary_identity_offset = identity_at.checked_add(21)?;
        (
            primary_identity_offset,
            View::u64_le_at(bytes, primary_identity_offset)?,
            None,
        )
    } else {
        return None;
    };
    Some(EntitySelectionFrame {
        record_index,
        byte_offset,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        asset_id: prefix.asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id: prefix.context_id,
        context_id_offset: prefix.context_id_offset,
        identity_record_index: record_index.checked_add(3)?,
        identity_record_offset: u64::try_from(identity_at).ok()?,
        primary_identity,
        primary_identity_offset: u64::try_from(primary_identity_offset).ok()?,
        secondary,
        next_record_index,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

/// Decode whole-body construction operands that contain one persistent body recipe.
pub(crate) fn decode_body_recipe_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignBodyRecipeOperand>, CodecError> {
    let mut headers_by_identity = HashMap::<_, Option<&DesignRecordHeader>>::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        if let Some(existing) = headers_by_identity.get_mut(&key) {
            *existing = None;
        } else {
            headers_by_identity.insert(key, Some(header));
        }
    }
    let mut body_recipes_by_stream = HashMap::<_, Vec<&ConstructionRecipe>>::new();
    for recipe in recipes
        .iter()
        .filter(|recipe| recipe.kind == ConstructionRecipeKind::Body)
    {
        let Some(stream) = native_stream(&recipe.id) else {
            continue;
        };
        body_recipes_by_stream
            .entry(stream)
            .or_default()
            .push(recipe);
    }
    for stream_recipes in body_recipes_by_stream.values_mut() {
        stream_recipes.sort_by_key(|recipe| recipe.byte_offset);
    }
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        if scopes.iter().any(|scope| {
            scope.record_index == group.scope_record_index
                && native_stream(&scope.id) == Some(stream)
                && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
        }) {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(Some(header)) = headers_by_identity.get(&(stream, record_index)) else {
                continue;
            };
            let Some(recipe) = unique_body_recipe_with_index(
                bytes,
                records,
                header,
                body_recipes_by_stream
                    .get(stream)
                    .map_or(&[], Vec::as_slice),
            ) else {
                continue;
            };
            if let Some(operand) =
                parse_body_recipe_operand_with_index(bytes, records, group, ordinal, header, recipe)
            {
                push_body_recipe_operand(&mut out, operand, &entry.name, header.byte_offset);
            }
        }
    }
    for scope in scopes.iter().filter(|scope| {
        scope.combine_operation().is_some()
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_borrowed_record_offsets(&mut record_offset_index, stream, bytes);
        let operation = scope.combine_operation();
        let record_indexes = operation
            .into_iter()
            .flat_map(|operation| {
                std::iter::once(&operation.target_record_index)
                    .chain(operation.tools.iter().map(|tool| &tool.record_index))
            })
            .chain(
                scope
                    .reference_members()
                    .values()
                    .filter(|_| operation.is_none()),
            );
        for record_index in record_indexes {
            let mut ordinals = scope
                .reference_members()
                .values()
                .enumerate()
                .filter(|(_, member)| *member == record_index)
                .filter_map(|(ordinal, _)| u32::try_from(ordinal).ok());
            let Some(scope_reference_ordinal) = ordinals.next() else {
                continue;
            };
            if ordinals.next().is_some()
                || scope
                    .combine_operation()
                    .is_some_and(|_| scope_reference_ordinal.is_multiple_of(2))
            {
                continue;
            }
            let Some(Some(header)) = headers_by_identity.get(&(stream, *record_index)) else {
                continue;
            };
            let Some(recipe) = unique_body_recipe_with_index(
                bytes,
                records,
                header,
                body_recipes_by_stream
                    .get(stream)
                    .map_or(&[], Vec::as_slice),
            ) else {
                continue;
            };
            let owner = DesignOperandOwner::ScopeReference {
                scope_reference_ordinal,
            };
            if let Some(operand) = parse_body_recipe_operand_frame_with_index(
                bytes,
                records,
                scope.record_index,
                owner,
                header,
                recipe,
            ) {
                push_body_recipe_operand(&mut out, operand, &entry.name, header.byte_offset);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    let mut owner_counts = HashMap::<String, u32>::new();
    for operand in &out {
        if let Some(count) = owner_counts.get_mut(&operand.id) {
            // Only a count of exactly one is kept, so a count that cannot
            // grow further already means more than one.
            if let Some(next) = count.checked_add(1) {
                *count = next;
            }
        } else {
            owner_counts.insert(operand.id.clone(), 1_u32);
        }
    }
    out.retain(|operand| owner_counts.get(&operand.id) == Some(&1));
    Ok(out)
}

fn push_body_recipe_operand(
    out: &mut Vec<DesignBodyRecipeOperand>,
    mut operand: DesignBodyRecipeOperand,
    stream: &str,
    offset: u64,
) {
    operand.id = design_record_id(stream, ":design-body-recipe-operand#", offset);

    out.push(operand);
}

/// Select the sole body recipe in the structural interval after the `N+3`
/// header and before the enclosing `N+4` header. The bounded Design stream,
/// rather than an arbitrary byte distance, limits the interval. A stream with
/// no `N+4` header after the prologue admits the first body recipe that
/// `body_recipe_operand_end_with_index` closes.
#[cfg(test)]
fn unique_body_recipe<'a>(
    bytes: &[u8],
    header: &DesignRecordHeader,
    recipes: &'a [&'a ConstructionRecipe],
) -> Option<&'a ConstructionRecipe> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    unique_body_recipe_with_index(bytes, &records, header, recipes)
}

fn unique_body_recipe_with_index<'a>(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    header: &DesignRecordHeader,
    recipes: &'a [&'a ConstructionRecipe],
) -> Option<&'a ConstructionRecipe> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let prologue_end = body_recipe_prologue_end_with_index(records, start, header.record_index)?;
    let lower = u64::try_from(prologue_end).ok()?;
    let first = recipes.partition_point(|recipe| recipe.byte_offset < lower);
    let Some(next_at) =
        records.first_at_or_after(prologue_end, header.record_index.checked_add(4)?)
    else {
        let recipe = recipes.get(first)?;
        let recipe_at = usize::try_from(recipe.byte_offset).ok()?;
        body_recipe_operand_end_with_index(
            bytes,
            records,
            prologue_end,
            header.record_index,
            recipe_at,
        )?;
        return Some(recipe);
    };
    let upper = u64::try_from(next_at).ok()?;
    let matching = &recipes[first..recipes.partition_point(|recipe| recipe.byte_offset < upper)];
    let [recipe] = matching else {
        return None;
    };
    Some(recipe)
}

/// Offset past the four consecutively indexed records that open a body-recipe
/// operand, or `None` when the records after `start` do not carry
/// `record_index` through `record_index + 3` in order.
///
/// The prologue depends only on the operand header, so a caller weighing many
/// candidate recipes against one header resolves it once.
#[cfg(test)]
fn body_recipe_prologue_end(bytes: &[u8], start: usize, record_index: u32) -> Option<usize> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    body_recipe_prologue_end_with_index(&records, start, record_index)
}

fn body_recipe_prologue_end_with_index(
    records: &IndexedRecordOffsets,
    start: usize,
    record_index: u32,
) -> Option<usize> {
    let mut search = start.checked_add(11)?;
    for expected in [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ] {
        let at = records.first_at_or_after(search, expected)?;
        search = at.checked_add(11)?;
    }
    Some(search)
}

/// Record index and offset of the indexed header that closes a body-recipe
/// operand whose recipe sits at `recipe_at`. The recipe must follow the
/// prologue that ends at `prologue_end`.
///
/// The header is record `record_index + 4` when the stream stores that record
/// after the prologue. Otherwise the recipe record is the only indexed record
/// after the prologue, and its successor must start at the recipe's fixed end.
#[cfg(test)]
fn body_recipe_operand_end(
    bytes: &[u8],
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Option<(u32, usize)> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    body_recipe_operand_end_with_index(bytes, &records, prologue_end, record_index, recipe_at)
}

fn body_recipe_operand_end_with_index(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Option<(u32, usize)> {
    if recipe_at < prologue_end {
        return None;
    }
    let successor = record_index.checked_add(4)?;
    if records.first_at_or_after(prologue_end, successor).is_some() {
        return Some((successor, records.first_at_or_after(recipe_at, successor)?));
    }
    let recipe_record_at = next_indexed_record_offset(bytes, prologue_end)?;
    let close_at =
        next_indexed_record_offset(bytes, recipe_record_at.checked_add(indexed_header::LEN)?)?;
    if recipe_record_at >= recipe_at || close_at != recipe_at.checked_add(body_recipe_end::LEN)? {
        return None;
    }
    Some((
        View::u32_le_at(bytes, close_at.checked_add(indexed_header::RECORD_INDEX)?)?,
        close_at,
    ))
}

#[cfg(test)]
fn parse_body_recipe_operand(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    parse_body_recipe_operand_with_index(
        bytes,
        &records,
        group,
        group_member_ordinal,
        header,
        recipe,
    )
}

fn parse_body_recipe_operand_with_index(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    parse_body_recipe_operand_frame_with_index(
        bytes,
        records,
        group.scope_record_index,
        DesignOperandOwner::Group {
            group_record_index: group.record_index,
            group_member_ordinal,
        },
        header,
        recipe,
    )
}

fn parse_body_recipe_operand_frame_with_index(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    owner: DesignOperandOwner,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let recipe_at = usize::try_from(recipe.byte_offset).ok()?;
    let prologue_end = body_recipe_prologue_end_with_index(records, start, header.record_index)?;
    let (next_record_index, next_at) = body_recipe_operand_end_with_index(
        bytes,
        records,
        prologue_end,
        header.record_index,
        recipe_at,
    )?;
    let reference_count = usize::try_from(View::u32_le_at(bytes, start + 21)?).ok()?;
    // The legacy Combine form permits an empty persistent-reference table;
    // its marker then starts at the ordinary post-count cursor. The history
    // binder keeps that operand native because an empty identity cannot prove
    // a body selection.
    if start >= recipe_at || recipe_at >= next_at || bytes.get(start + 11..start + 21)? != [0; 10] {
        return None;
    }
    let mut cursor = start.checked_add(25)?;
    // Each reference consumes 12 bytes; a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if reference_count > bytes.len().checked_sub(cursor)? / 12 {
        return None;
    }

    let mut references = Vec::new();
    for _ in 0..reference_count {
        references.push(DesignBodyRecipeReference {
            design_reference: View::u64_le_at(bytes, cursor)?,
            design_reference_offset: u64::try_from(cursor).ok()?,
            form: View::u32_le_at(bytes, cursor + 8)?,
            form_offset: u64::try_from(cursor + 8).ok()?,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        });
        cursor = cursor.checked_add(12)?;
    }
    if bytes.get(cursor) != Some(&1)
        || bytes.get(cursor + 9..cursor + 11)? != [0; 2]
        || View::u32_le_at(bytes, cursor + 11)? != 1
    {
        return None;
    }
    let nested_record_index = View::u64_le_at(bytes, cursor + 1)?;
    let asset_id_at = cursor.checked_add(15)?;
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, asset_id_at, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let selector_tail_at = after_context_id.checked_add(4)?;
    let selector_tail: [u8; 4] = bytes
        .get(selector_tail_at..selector_tail_at.checked_add(4)?)?
        .try_into()
        .ok()?;
    let selector_tail_is_valid = match header.class_tag.as_str() {
        // The class-365 member is a four-byte generation-dependent value.
        // Retain it until DR-24 identifies its neutral meaning.
        "365" => true,
        "367" => selector_tail == [1, 0, 0, 0],
        _ => selector_tail == [0; 4],
    };
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
        || !selector_tail_is_valid
        || nested_record_index != u64::from(header.record_index.checked_add(3)?)
    {
        return None;
    }
    let recipe_id = recipe.id.clone();
    DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: String::new(),
            scope_record_index,
            owner,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            asset_id: asset_id.try_into().ok()?,
            asset_id_offset: u64::try_from(asset_id_at + 4).ok()?,
            context_id: context_id.try_into().ok()?,
            context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
            selector_tail: Some(crate::records::identity::Located {
                value: selector_tail,
                offset: u64::try_from(selector_tail_at).ok()?,
            }),
            references,
            nested_record_index,
            nested_record_index_offset: u64::try_from(cursor + 1).ok()?,
            recipe_id,
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index,
            next_byte_offset: u64::try_from(next_at).ok()?,
        },
    )
    .ok()
}

/// Join body-recipe Design references to solved persistent face tags.
pub(crate) fn bind_body_recipe_operand_candidates(
    operands: &mut [DesignBodyRecipeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
    scopes: &[DesignParameterScope],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut recipes_by_id = HashMap::<_, Option<&ConstructionRecipe>>::new();
    for recipe in recipes {
        if let Some(existing) = recipes_by_id.get_mut(recipe.id.as_str()) {
            *existing = None;
        } else {
            recipes_by_id.insert(recipe.id.as_str(), Some(recipe));
        }
    }
    for operand in operands {
        // The recipe selector is a persistent-tag selector for Combine's
        // form-three clauses. In the class-365 body-member grammar it names
        // the enclosing N+4 record instead, so each clause joins by its own
        // Design reference and the history pass performs the body proof.
        let form_three_uses_recipe_selector = {
            let mut matching_scopes = scopes.iter().filter(|scope| {
                scope.record_index == operand.scope_record_index
                    && native_stream(&scope.id) == native_stream(&operand.id)
            });
            match (matching_scopes.next(), matching_scopes.next()) {
                (Some(scope), None) => scope.combine_operation().is_some(),
                _ => true,
            }
        };
        let tag_selector = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe)
            .and_then(|recipe| recipe.design.as_ref()?.selector)
            .map(|selector| i64::from(selector.value));
        let operand_id = operand.id.clone();
        for reference in operand.reference_bindings_mut() {
            reference.candidate_faces.clear();
            let Ok(design_reference) = i64::try_from(reference.design_reference) else {
                continue;
            };
            for tag in tags.iter().filter(|tag| {
                tag.design_references.contains(&design_reference)
                    && crate::ids::same_native_occurrence(&tag.id, &operand_id)
                    && (reference.form != 3
                        || !form_three_uses_recipe_selector
                        || tag_selector == Some(tag.selector))
            }) {
                if let AttributeTarget::Face(face) = &tag.target {
                    reference.candidate_faces.push(face.clone());
                }
            }
            reference
                .candidate_faces
                .sort_by(|left, right| left.as_str().cmp(right.as_str()));
            reference.candidate_faces.dedup();
        }
    }
}

/// Resolve selection-member local identities against persistent point and
/// curve identities owned by the Extrude scope's selected Sketch.
pub(crate) fn bind_extrude_selection_geometry(
    members: &mut [DesignExtrudeSelectionMember],
    groups: &[DesignExtrudeSelectionGroup],
    scopes: &[DesignParameterScope],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) {
    let mut selected_sketches = HashMap::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| {
            native_stream(&scope.id) == Some(stream)
                && scope.record_index == group.scope_record_index
        }) else {
            continue;
        };
        let Some(profile) = scope.extrude_profile() else {
            continue;
        };
        let key = (stream, group.record_index);
        selected_sketches.insert(key, profile.entity_id.suffix());
    }
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let Some(entity_suffix) = selected_sketches.get(&(stream, member.group_record_index))
        else {
            continue;
        };
        let Ok(entity_suffix) = u32::try_from(*entity_suffix) else {
            continue;
        };
        let point_operands = points.iter().filter_map(|point| {
            (native_stream(&point.id) == Some(stream)
                && (point.owner_reference.is_none()
                    || point.owner_reference == Some(entity_suffix))
                && point.persistent_id() == Some(member.local_id))
            .then_some(SketchRelationOperand::Point {
                record_index: point.record_index,
                persistent_id: point.persistent_id(),
            })
        });
        let curve_operands = curves.iter().filter_map(|curve| {
            (native_stream(&curve.id) == Some(stream)
                && (curve.owner_reference.is_none()
                    || curve.owner_reference == Some(entity_suffix))
                && (curve.primary_id.get() == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                })
        });
        let mut matches = point_operands.chain(curve_operands);
        if let (Some(resolved), None) = (matches.next(), matches.next()) {
            member.resolved_geometry = Some(resolved);
        } else {
            member.resolved_geometry = None;
        }
    }
}

/// Bind selection members to construction-operand identity chains that
/// terminate at the same fixed persistent identity record.
pub(crate) fn bind_extrude_selection_identities(
    members: &mut [DesignExtrudeSelectionMember],
    identities: &[DesignConstructionOperandIdentity],
) {
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let mut matches = identities
            .iter()
            .filter(|identity| {
                native_stream(&identity.id) == Some(stream)
                    && identity.following_record_index() == member.record_index()
                    && identity.following_byte_offset() == member.byte_offset()
                    && identity.persistent_identity().is_some_and(|persistent| {
                        persistent.local_id == member.local_id
                            && persistent.asset_id == member.asset_id
                            && persistent.context_id == member.context_id
                    })
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|identity| {
            identity
                .wrappers()
                .first()
                .map(|wrapper| wrapper.byte_offset)
        });
        member.operand_identity_ids = matches
            .into_iter()
            .map(|identity| identity.id.clone())
            .collect();
    }
}

fn parse_extrude_selection_member(
    bytes: &[u8],
    group: &DesignExtrudeSelectionGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignExtrudeSelectionMember> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let member = parse_extrude_identity_member(bytes, start, Some(group.scope_record_index))?;
    DesignExtrudeSelectionMember::try_new(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionMemberDraft {
            id: String::new(),
            group_record_index: group.record_index,
            group_member_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            local_id: member.local_id,
            local_id_offset: member.local_id_offset,
            asset_id: member.asset_id.try_into().ok()?,
            asset_id_offset: member.asset_id_offset,
            context_id: member.context_id.try_into().ok()?,
            context_id_offset: member.context_id_offset,
            tail_slot_present: member.tail_slot_present,
            tail_slot_offset: member.tail_slot_offset,
            scope_tail: member.scope_tail,
            resolved_geometry: None,
            operand_identity_ids: Vec::new(),
            historical: None,
            next_record_index: member.next_record_index,
            next_byte_offset: member.next_byte_offset,
        },
    )
    .ok()
}

/// Width of a same-segment marked reference.
const MARKED_REFERENCE_LEN: usize = 11;
/// Bytes between the two chain references of a selection-member scope tail.
const EXTRUDE_SCOPE_TAIL_SEPARATOR: [u8; 3] = [1, 0, 0];

struct ParsedExtrudeIdentityMember {
    local_id: u64,
    local_id_offset: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    tail_slot_present: bool,
    tail_slot_offset: u64,
    scope_tail: Option<DesignExtrudeSelectionScopeTail>,
    next_record_index: u32,
    next_byte_offset: u64,
}

/// Parse the fifty-byte scope tail that follows a 190-byte member frame.
///
/// Returns the tail and the offset where the tail ends. The closing marked
/// reference has to name `scope_record_index`.
fn parse_extrude_selection_scope_tail(
    bytes: &[u8],
    tail_start: usize,
    scope_record_index: u32,
) -> Option<(DesignExtrudeSelectionScopeTail, usize)> {
    use crate::layout::extrude_selection_member_scope_tail as tail;
    let tail_end = tail_start.checked_add(tail::LEN)?;
    bytes.get(tail_start..tail_end)?;
    let marked_reference = |offset: usize| -> Option<u32> {
        let mut cursor = tail_start.checked_add(offset)?;
        let (record_index, _) = take_record_reference(bytes, &mut cursor)?;
        (cursor
            == tail_start
                .checked_add(offset)?
                .checked_add(MARKED_REFERENCE_LEN)?)
        .then_some(record_index)
    };
    if bytes.get(tail_start + tail::LEADING_ZERO) != Some(&tail::LEADING_ZERO_VALUE)
        || View::u32_le_at(bytes, tail_start + tail::OPAQUE_WORD)? != tail::OPAQUE_WORD_VALUE
        || bytes.get(tail_start + tail::SEPARATOR..tail_start + tail::CHAIN_REFERENCE_SECOND)?
            != EXTRUDE_SCOPE_TAIL_SEPARATOR
        || bytes.get(tail_start + tail::SECOND_SEPARATOR) != Some(&tail::SECOND_SEPARATOR_VALUE)
    {
        return None;
    }
    let first = marked_reference(tail::CHAIN_REFERENCE_FIRST)?;
    let second = marked_reference(tail::CHAIN_REFERENCE_SECOND)?;
    let scope = marked_reference(tail::SCOPE_REFERENCE)?;
    if scope != scope_record_index {
        return None;
    }
    Some((
        DesignExtrudeSelectionScopeTail {
            opaque_scalar_bits: View::u64_le_at(bytes, tail_start + tail::OPAQUE_SCALAR)?,
            opaque_word: tail::OPAQUE_WORD_VALUE,
            chain_references: [first, second],
            scope_record_index: scope,
        },
        tail_end,
    ))
}

/// Fields shared by every selection-member tail form: the identity run up to
/// and including the optional-slot marker.
struct ExtrudeIdentityFrame {
    local_id: u64,
    asset_id: String,
    after_asset_id: usize,
    context_id: String,
    after_context_id: usize,
    tail_slot_offset: usize,
    tail_slot_present: bool,
}

fn parse_extrude_identity_frame(bytes: &[u8], start: usize) -> Option<ExtrudeIdentityFrame> {
    if bytes.get(start + extrude_member::ZERO_RUN_10..start + extrude_member::LOCAL_IDENTITY)?
        != [0; 10]
    {
        return None;
    }
    let local_id = View::u64_le_at(bytes, start + extrude_member::LOCAL_IDENTITY)?;
    let (asset_id, after_asset_id) =
        lp_utf16_bounded(bytes, start + extrude_member::ASSET_UUID_LENGTH, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let tail_slot_offset = after_context_id.checked_add(4)?;
    let tail_slot_present = match bytes.get(tail_slot_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
    {
        return None;
    }
    Some(ExtrudeIdentityFrame {
        local_id,
        asset_id,
        after_asset_id,
        context_id,
        after_context_id,
        tail_slot_offset,
        tail_slot_present,
    })
}

/// Longest extended selection-member tail that a following record header ends.
const EXTRUDE_EXTENDED_TAIL_MAX: usize = 1024;

/// End of an extended selection-member tail that starts at `tail_start`.
///
/// The tail runs to the next indexed record header: u32 `3`, three ASCII
/// digits and a u32 record index. When no header follows, the
/// tail runs to the end of the stream and the record index is zero. Returns
/// the record index and the end offset. The fourteen-byte and fifty-byte
/// lengths belong to the typed tail forms and are not extended.
fn extended_extrude_tail_end(bytes: &[u8], tail_start: usize) -> Option<(u32, usize)> {
    let is_header = |at: usize| -> Option<u32> {
        let header = bytes.get(at..at.checked_add(11)?)?;
        (header[..4] == 3u32.to_le_bytes() && header[4..7].iter().all(u8::is_ascii_digit))
            .then(|| View::u32_le_at(bytes, at + 7))?
    };
    let typed_length = |end: usize| matches!(end - tail_start, 14 | 50);
    for at in tail_start.checked_add(1)?..bytes.len() {
        if let Some(index) = is_header(at) {
            return (at - tail_start <= EXTRUDE_EXTENDED_TAIL_MAX && !typed_length(at))
                .then_some((index, at));
        }
    }
    (bytes.len() > tail_start && !typed_length(bytes.len())).then_some((0, bytes.len()))
}

/// Parse a selection-member identity frame.
///
/// `scope_record_index` names the scope that owns the selection group; the
/// scope-tail and extended-tail forms are admitted only when it is given.
fn parse_extrude_identity_member(
    bytes: &[u8],
    start: usize,
    scope_record_index: Option<u32>,
) -> Option<ParsedExtrudeIdentityMember> {
    parse_extrude_identity_member_exact(bytes, start, scope_record_index).or_else(|| {
        scope_record_index?;
        let frame = parse_extrude_identity_frame(bytes, start)?;
        let fixed_end = start.checked_add(extrude_member::LEN)?;
        if View::u32_le_at(bytes, frame.tail_slot_offset + 1) != Some(0)
            || frame.after_context_id.checked_add(9)? != fixed_end
        {
            return None;
        }
        let (next_record_index, tail_end) = extended_extrude_tail_end(bytes, fixed_end)?;
        Some(ParsedExtrudeIdentityMember {
            local_id: frame.local_id,
            local_id_offset: u64::try_from(start + extrude_member::LOCAL_IDENTITY).ok()?,
            asset_id: frame.asset_id,
            asset_id_offset: u64::try_from(start + extrude_member::ASSET_UUID_UTF16).ok()?,
            context_id: frame.context_id,
            context_id_offset: u64::try_from(frame.after_asset_id + 4).ok()?,
            tail_slot_present: frame.tail_slot_present,
            tail_slot_offset: u64::try_from(frame.tail_slot_offset).ok()?,
            scope_tail: None,
            next_record_index,
            next_byte_offset: u64::try_from(tail_end).ok()?,
        })
    })
}

fn parse_extrude_identity_member_exact(
    bytes: &[u8],
    start: usize,
    scope_record_index: Option<u32>,
) -> Option<ParsedExtrudeIdentityMember> {
    let ExtrudeIdentityFrame {
        local_id,
        asset_id,
        after_asset_id,
        context_id,
        after_context_id,
        tail_slot_offset,
        tail_slot_present,
    } = parse_extrude_identity_frame(bytes, start)?;
    let fixed_end = start.checked_add(extrude_member::LEN)?;
    if let Some(scope_record_index) = scope_record_index {
        if !tail_slot_present
            && View::u32_le_at(bytes, tail_slot_offset + 1) == Some(0)
            && after_context_id.checked_add(9)? == fixed_end
            && bytes.get(fixed_end) == Some(&0)
        {
            if let Some((scope_tail, tail_end)) =
                parse_extrude_selection_scope_tail(bytes, fixed_end, scope_record_index)
            {
                let (next_record_index, next_byte_offset) = if tail_end == bytes.len() {
                    (0, u64::try_from(tail_end).ok()?)
                } else {
                    let (_, after_next_tag) =
                        lp_ascii_filtered_view(bytes, tail_end, 1..=2000, u8::is_ascii_graphic)?;
                    (
                        View::u32_le_at(bytes, after_next_tag)?,
                        u64::try_from(tail_end).ok()?,
                    )
                };
                return Some(ParsedExtrudeIdentityMember {
                    local_id,
                    local_id_offset: u64::try_from(start + extrude_member::LOCAL_IDENTITY).ok()?,
                    asset_id,
                    asset_id_offset: u64::try_from(start + extrude_member::ASSET_UUID_UTF16)
                        .ok()?,
                    context_id,
                    context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
                    tail_slot_present,
                    tail_slot_offset: u64::try_from(tail_slot_offset).ok()?,
                    scope_tail: Some(scope_tail),
                    next_record_index,
                    next_byte_offset,
                });
            }
        }
    }
    let fixed_end = if tail_slot_present
        && View::u32_le_at(bytes, tail_slot_offset + 1) == Some(0)
        && after_context_id.checked_add(9)? == fixed_end
        && bytes.get(fixed_end) == Some(&0)
    {
        use crate::layout::extrude_selection_member_long_tail as tail;
        if bytes.get(fixed_end + tail::LEADING_ZERO) != Some(&tail::LEADING_ZERO_VALUE)
            || View::u32_le_at(bytes, fixed_end + tail::KIND) != Some(tail::KIND_VALUE)
            || View::u64_le_at(bytes, fixed_end + tail::VALUE) != Some(tail::VALUE_VALUE)
            || bytes.get(fixed_end + tail::TRAILING_ZERO) != Some(&tail::TRAILING_ZERO_VALUE)
        {
            return None;
        }
        fixed_end.checked_add(tail::LEN)?
    } else {
        fixed_end
    };
    let (next_record_index, next_byte_offset) = if View::u32_le_at(bytes, tail_slot_offset + 1)
        == Some(0)
        && after_context_id.checked_add(9)? == start.checked_add(extrude_member::LEN)?
    {
        if fixed_end == bytes.len() {
            (0, u64::try_from(fixed_end).ok()?)
        } else {
            let (_, after_next_tag) =
                lp_ascii_filtered_view(bytes, fixed_end, 1..=2000, u8::is_ascii_graphic)?;
            (
                View::u32_le_at(bytes, after_next_tag)?,
                u64::try_from(fixed_end).ok()?,
            )
        }
    } else if bytes.get(tail_slot_offset + 1..tail_slot_offset + 4)? == [0; 3] {
        let mut cursor = tail_slot_offset.checked_add(4)?;
        let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
        let next_at = cursor;
        let (_, after_next_tag) =
            lp_ascii_filtered_view(bytes, next_at, 1..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_next_tag)? != next_record_index {
            return None;
        }
        (next_record_index, u64::try_from(next_at).ok()?)
    } else {
        return None;
    };
    Some(ParsedExtrudeIdentityMember {
        local_id,
        local_id_offset: u64::try_from(start + extrude_member::LOCAL_IDENTITY).ok()?,
        asset_id,
        asset_id_offset: u64::try_from(start + extrude_member::ASSET_UUID_UTF16).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
        tail_slot_present,
        tail_slot_offset: u64::try_from(tail_slot_offset).ok()?,
        scope_tail: None,
        next_record_index,
        next_byte_offset,
    })
}

struct ParsedEdgeIdentityMember {
    layout: crate::records::topology::edge_identity::DesignEdgeIdentityLayout,
    local_id: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
}

fn parse_edge_identity_member(bytes: &[u8], start: usize) -> Option<ParsedEdgeIdentityMember> {
    use crate::records::topology::edge_identity::DesignEdgeIdentityLayout;
    let layout = if bytes.get(start + 11..start + 23) == Some(&[0; 12]) {
        DesignEdgeIdentityLayout::Full
    } else if bytes.get(start + 11..start + 22) == Some(&[0; 11]) {
        DesignEdgeIdentityLayout::Compact
    } else if bytes.get(start + 11..start + 21) == Some(&[0; 10]) {
        DesignEdgeIdentityLayout::Shortest
    } else {
        return None;
    };
    let marker_offset = usize::try_from(layout.marker_offset()).ok()?;
    let local_id_offset = marker_offset + 1;
    let asset_offset = marker_offset + 15;
    if bytes.get(start + marker_offset) != Some(&1)
        || bytes.get(start + marker_offset + 5..start + marker_offset + 11)? != [0; 6]
        || View::u32_le_at(bytes, start + marker_offset + 11)? != 1
    {
        return None;
    }
    let local_id = u64::from(View::u32_le_at(bytes, start + local_id_offset)?);
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + asset_offset, 1..=256)?;
    let (context_id, _after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !is_guid_relaxed(&asset_id) || !is_guid_relaxed(&context_id) {
        return None;
    }
    Some(ParsedEdgeIdentityMember {
        layout,
        local_id,
        asset_id,
        asset_id_offset: u64::try_from(start + asset_offset + 4).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
    })
}

fn utf16_decimal_u64(bytes: &[u8], at: usize) -> Option<(u64, usize)> {
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !(1..=256).contains(&count) {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(count.checked_mul(2)?)?;
    bytes.get(start..end)?;
    let mut value = 0u64;
    let mut digits = 0usize;
    for index in 0..count {
        let unit = View::u16_le_at(bytes, start.checked_add(index.checked_mul(2)?)?)?;
        if index == 0 && unit == u16::from(b'+') {
            continue;
        }
        let digit = u8::try_from(unit).ok()?.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(digit))?;
        digits += 1;
    }
    (digits > 0).then_some((value, end))
}

pub(in crate::design) fn parse_sketch_profile(
    bytes: &[u8],
    stream: &str,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    entities: &[DesignEntityHeader],
) -> Option<DesignSketchProfileOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || View::u32_le_at(bytes, start + 22)? != header.record_index.checked_add(3)?
        || bytes.get(start + 26..start + 32)? != [0; 6]
        || View::u32_le_at(bytes, start + 32)? != 1
    {
        return None;
    }
    relaxed_guid_end(bytes, start + 36)?;
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 36, 1..=256)?;
    let (entity_suffix, after_entity_suffix) = utf16_decimal_u64(bytes, after_asset_id)?;
    let paired_at = next_indexed_record_offset(bytes, start + 11)?;
    let (paired_class_tag, after_paired_tag) =
        lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    let tail_length = paired_at.checked_sub(after_entity_suffix)?;
    if View::u32_le_at(bytes, after_paired_tag)? != header.record_index
        || !matches!(tail_length, 83 | 89 | 93 | 94)
    {
        return None;
    }
    if tail_length == 83 {
        // The 83-byte tail omits the marked reference that follows the third
        // u32 of the 94-byte tail, which is then zero.
        let tail = after_entity_suffix;
        if View::u32_le_at(bytes, tail)? != 1
            || View::u32_le_at(bytes, tail + 4)? != 1
            || bytes.get(tail + 8..tail + 16)? != [0; 8]
            || View::u32_le_at(bytes, tail + 16)? != 0x41
            || marked_record_reference(bytes, tail + 46) != header.record_index.checked_add(2)
            || marked_record_reference(bytes, tail + 60) != header.record_index.checked_add(1)
            || bytes.get(tail + 71) != Some(&0)
            || marked_record_reference(bytes, tail + 72).is_none()
        {
            return None;
        }
    }
    if matches!(tail_length, 89 | 93) {
        let tail = after_entity_suffix;
        let (nested_two_at, nested_one_at, scope_at) = if tail_length == 89 {
            (53, 66, 78)
        } else {
            (57, 70, 82)
        };
        if bytes.get(tail..tail + 8) != Some(&[1, 0, 0, 0, 0, 0, 0, 0])
            || View::u32_le_at(bytes, tail + 8) != Some(1)
            || marked_record_reference(bytes, tail + nested_two_at)
                != header.record_index.checked_add(2)
            || bytes.get(tail + nested_one_at - 2..tail + nested_one_at) != Some(&[0; 2])
            || marked_record_reference(bytes, tail + nested_one_at)
                != header.record_index.checked_add(1)
            || bytes.get(tail + scope_at - 1) != Some(&0)
            || marked_record_reference(bytes, tail + scope_at).is_none()
            || View::u32_le_at(bytes, tail + 41) == Some(0)
            || (tail_length == 93
                && View::u32_le_at(bytes, tail + 41) != View::u32_le_at(bytes, tail + 53))
        {
            return None;
        }
    }
    let mut matches = entities.iter().filter(|entity| {
        native_stream(&entity.id) == Some(stream)
            && entity.in_sketch_module()
            && entity.entity_id.suffix() == entity_suffix
    });
    let entity = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let region_selection =
        parse_sketch_profile_region_selection(bytes, header.record_index, paired_at);
    let entity_id_text = entity.entity_id.as_str().to_owned();
    DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            scope_reference_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            asset_id: asset_id.try_into().ok()?,
            asset_id_offset: u64::try_from(start + 40).ok()?,
            entity_id: crate::records::identity::DesignEntityId::try_from(entity_id_text).ok()?,
            entity_reference_offset: u64::try_from(after_asset_id + 4).ok()?,
            region_selection,
            paired_class_tag: crate::design::decode::text::class_tag_from_view(paired_class_tag)
                .ok()?,
            paired_byte_offset: u64::try_from(paired_at).ok()?,
        },
    )
    .ok()
}

fn parse_sketch_profile_region_selection(
    bytes: &[u8],
    profile_record_index: u32,
    paired_at: usize,
) -> Option<DesignSketchProfileRegionSelection> {
    const REGION_MARKER_LEN: usize = 1;
    const REGION_COUNT_LEN: usize = 4;
    const TERMINATOR_LEN: usize = 5;

    let next_header = |position, expected| {
        let at = next_indexed_record_offset(bytes, position)?;
        (indexed_record_header_at(bytes, at)?.record_index == expected).then_some(at)
    };
    let nested_one_at = next_header(
        paired_at.checked_add(indexed_header::LEN)?,
        profile_record_index.checked_add(1)?,
    )?;
    let nested_two_at = next_header(
        nested_one_at.checked_add(indexed_header::LEN)?,
        profile_record_index.checked_add(2)?,
    )?;
    let selection_record_index = profile_record_index.checked_add(3)?;
    let selection_at = next_header(
        nested_two_at.checked_add(indexed_header::LEN)?,
        selection_record_index,
    )?;
    let (class_tag, after_class_tag) =
        lp_ascii_filtered_view(bytes, selection_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_class_tag)? != selection_record_index
        || bytes.get(
            selection_at + region_selection::ZERO_RUN_10
                ..selection_at + region_selection::PROFILE_REFERENCE_MARKER,
        )? != [0; 10]
        || marked_record_reference(
            bytes,
            selection_at + region_selection::PROFILE_REFERENCE_MARKER,
        ) != Some(profile_record_index)
        || bytes.get(
            selection_at + region_selection::ZERO_RUN_6
                ..selection_at + region_selection::FORMAT_VERSION,
        )? != [0; 6]
        || View::u32_le_at(bytes, selection_at + region_selection::FORMAT_VERSION)? != 1
    {
        return None;
    }
    let region_count = usize::try_from(View::u32_le_at(
        bytes,
        selection_at + region_selection::REGION_COUNT,
    )?)
    .ok()?;
    if region_count == 0 {
        return None;
    }
    let minimum_regions_len = region_count
        .checked_mul(REGION_COUNT_LEN.checked_add(region_member::LEN)?)?
        .checked_add(
            region_count
                .checked_sub(1)?
                .checked_mul(REGION_MARKER_LEN)?,
        )?
        .checked_add(TERMINATOR_LEN)?
        .checked_add(indexed_header::LEN)?;
    let mut cursor = selection_at.checked_add(region_selection::LEN)?;
    if cursor.checked_add(minimum_regions_len)? > bytes.len() {
        return None;
    }

    let mut regions = Vec::new();
    for region_ordinal in 0..region_count {
        if region_ordinal != 0 {
            if bytes.get(cursor) != Some(&1) {
                return None;
            }
            cursor = cursor.checked_add(1)?;
        }
        let member_count_offset = cursor;
        let member_count = usize::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
        cursor = cursor.checked_add(REGION_COUNT_LEN)?;
        if member_count == 0 {
            return None;
        }
        let remaining_regions = region_count.checked_sub(region_ordinal.checked_add(1)?)?;
        let trailing_minimum_len = remaining_regions
            .checked_mul(
                REGION_MARKER_LEN
                    .checked_add(REGION_COUNT_LEN)?
                    .checked_add(region_member::LEN)?,
            )?
            .checked_add(TERMINATOR_LEN)?
            .checked_add(indexed_header::LEN)?;
        let member_run_len = member_count.checked_mul(region_member::LEN)?;
        if cursor
            .checked_add(member_run_len)?
            .checked_add(trailing_minimum_len)?
            > bytes.len()
        {
            return None;
        }

        let mut members = Vec::new();
        for _ in 0..member_count {
            let kind_offset = cursor;
            let kind = View::u32_le_at(bytes, cursor)?;
            let curve_primary_id = std::num::NonZeroU32::new(View::u32_le_at(
                bytes,
                cursor.checked_add(region_member::CURVE_PRIMARY_ID)?,
            )?)?;
            let incidence_words_offset = cursor.checked_add(region_member::ZERO_WORDS_3)?;
            let mut incidence_words = [0; 8];
            for (ordinal, word) in incidence_words.iter_mut().enumerate() {
                *word = View::u32_le_at(
                    bytes,
                    incidence_words_offset.checked_add(ordinal.checked_mul(4)?)?,
                )?;
            }
            if kind != 3
                || incidence_words[..3] != [0; 3]
                || !matches!(incidence_words[3], 0 | 1)
                || incidence_words[6..] != [0; 2]
            {
                return None;
            }
            members.push(DesignSketchProfileRegionMember {
                kind_offset: u64::try_from(kind_offset).ok()?,
                curve_primary_id,
                curve_primary_id_offset: u64::try_from(
                    cursor.checked_add(region_member::CURVE_PRIMARY_ID)?,
                )
                .ok()?,
                incidence_flag: incidence_words[3] == 1,
                incidence_values: [
                    crate::records::topology::sketch_profile::DesignRegionIncidence::try_from(
                        incidence_words[4],
                    )
                    .ok()?,
                    crate::records::topology::sketch_profile::DesignRegionIncidence::try_from(
                        incidence_words[5],
                    )
                    .ok()?,
                ],
                incidence_words_offset: u64::try_from(incidence_words_offset).ok()?,
            });
            cursor = cursor.checked_add(region_member::LEN)?;
        }
        regions.push(DesignSketchProfileRegion {
            member_count_offset: u64::try_from(member_count_offset).ok()?,
            members,
        });
    }
    let companion_at = cursor.checked_add(TERMINATOR_LEN)?;
    if !matches!(bytes.get(cursor..companion_at)?, [0 | 1, 0, 0, 0, 0])
        || next_header(companion_at, selection_record_index)? != companion_at
    {
        return None;
    }
    let (companion_class_tag, after_companion_class_tag) =
        lp_ascii_filtered_view(bytes, companion_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_companion_class_tag)? != selection_record_index {
        return None;
    }
    Some(DesignSketchProfileRegionSelection {
        record_index: selection_record_index,
        byte_offset: u64::try_from(selection_at).ok()?,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        region_count_offset: u64::try_from(
            selection_at.checked_add(region_selection::REGION_COUNT)?,
        )
        .ok()?,
        regions,
        companion_class_tag: crate::design::decode::text::class_tag_from_view(companion_class_tag)
            .ok()?,
        companion_byte_offset: u64::try_from(companion_at).ok()?,
    })
}

struct ParsedRecipeOperand {
    paired_byte_offset: u64,
    paired_class_tag: crate::records::references::DesignClassTag,
    recipe_record_index: u32,
    recipe_record_byte_offset: u64,
    recipe_id: String,
    recipe_prefix_offset: u64,
    recipe_prefix_bytes: Vec<u8>,
    recipe_references: Vec<crate::records::dimensions::DesignRecipeReference>,
    recipe_program_offset: u64,
    recipe_program: Vec<i32>,
    next_record_index: u32,
    next_byte_offset: u64,
}

#[derive(Clone, Copy)]
enum RecipeOperandTerminator {
    RecordDelta(u32),
    NextIndexedAfterRecipe { limit: u64 },
}

/// Parse one exact persistent vertex-recipe envelope.
fn parse_vertex_recipe(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<DesignVertexRecipe> {
    let parsed = parse_recipe_operand(
        bytes,
        records,
        stream,
        header,
        recipes,
        (
            ConstructionRecipeKind::Vertex,
            RecipeOperandTerminator::RecordDelta(5),
        ),
    )?;
    DesignVertexRecipe::try_new(
        crate::records::feature::work_geometry::DesignVertexRecipeDraft {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            paired_byte_offset: parsed.paired_byte_offset,
            paired_class_tag: parsed.paired_class_tag,
            recipe_record_index: parsed.recipe_record_index,
            recipe_record_byte_offset: parsed.recipe_record_byte_offset,
            recipe_id: parsed.recipe_id,
            recipe_prefix_offset: parsed.recipe_prefix_offset,
            recipe_prefix_bytes: parsed.recipe_prefix_bytes,
            recipe_references: parsed.recipe_references,
            recipe_program_offset: parsed.recipe_program_offset,
            recipe_program: parsed.recipe_program,
            resolution: None,
            next_record_index: parsed.next_record_index,
            next_byte_offset: parsed.next_byte_offset,
        },
    )
    .ok()
}

/// Parse the indexed-record envelope shared by topology recipe operands.
fn parse_recipe_operand(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
    (recipe_kind, terminator): (ConstructionRecipeKind, RecipeOperandTerminator),
) -> Option<ParsedRecipeOperand> {
    let family_name = crate::design::RECIPES
        .iter()
        .find_map(|(name, kind)| (*kind == recipe_kind).then_some(*name))?;
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut offsets = [0usize; 5];
    let mut position = start.checked_add(11)?;
    for (delta, slot) in offsets[..4].iter_mut().enumerate() {
        let record_index = header
            .record_index
            .checked_add(u32::try_from(delta).ok()?)?;
        let offset = records.first_at_or_after(position, record_index)?;
        *slot = offset;
        position = offset.checked_add(11)?;
    }
    offsets[4] = match terminator {
        RecipeOperandTerminator::RecordDelta(delta) => {
            records.first_at_or_after(position, header.record_index.checked_add(delta)?)?
        }
        RecipeOperandTerminator::NextIndexedAfterRecipe { limit } => {
            let recipe_record_byte_offset = u64::try_from(offsets[3]).ok()?;
            let recipe = recipes
                .iter()
                .filter(|recipe| {
                    recipe.byte_offset > recipe_record_byte_offset
                        && recipe.byte_offset < limit
                        && recipe.kind == recipe_kind
                        && native_stream(&recipe.id) == Some(stream)
                })
                .min_by_key(|recipe| recipe.byte_offset)?;
            let recipe_program_at = usize::try_from(recipe.byte_offset)
                .ok()?
                .checked_add(family_name.len())?;
            let next = next_indexed_record_offset(bytes, recipe_program_at)?;
            (u64::try_from(next).ok()? <= limit).then_some(next)?
        }
    };
    let mut indexed = [("", 0u32); 5];
    for (offset, slot) in offsets.iter().zip(&mut indexed) {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, *offset, 0..=2000, u8::is_ascii_graphic)?;
        *slot = (class_tag, View::u32_le_at(bytes, after_tag)?);
    }
    let recipe_record_index = header.record_index.checked_add(3)?;
    let expected_prefix = [
        header.record_index,
        header.record_index.checked_add(1)?,
        header.record_index.checked_add(2)?,
        recipe_record_index,
    ];
    if !indexed
        .iter()
        .take(4)
        .zip(expected_prefix)
        .all(|((_, actual), expected)| *actual == expected)
    {
        return None;
    }
    let next_record_index = indexed[4].1;
    if let RecipeOperandTerminator::RecordDelta(delta) = terminator {
        if next_record_index != header.record_index.checked_add(delta)? {
            return None;
        }
    }
    let recipe_record_byte_offset = u64::try_from(offsets[3]).ok()?;
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let mut matches = recipes.iter().filter(|recipe| {
        recipe.byte_offset > recipe_record_byte_offset
            && recipe.byte_offset < next_byte_offset
            && recipe.kind == recipe_kind
            && native_stream(&recipe.id) == Some(stream)
    });
    let recipe = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        family_name.len(),
    )?;
    let recipe_prefix_offset = u64::try_from(recipe_prefix_at).ok()?;
    let recipe_references = decode_recipe_references(recipe_prefix_bytes, recipe_prefix_offset);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(family_name.len())?;
    let recipe_program_end = usize::try_from(next_byte_offset).ok()?;
    if recipe_program_end.checked_sub(recipe_program_at)? > 64 * 1024 {
        return None;
    }
    let recipe_program = contiguous_i32_program(bytes, recipe_program_at, recipe_program_end)?;
    let recipe_prefix_bytes = recipe_prefix_bytes.to_vec();
    let recipe_id = recipe.id.clone();
    Some(ParsedRecipeOperand {
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: crate::design::decode::text::class_tag_from_view(indexed[0].0).ok()?,
        recipe_record_index,
        recipe_record_byte_offset,
        recipe_id,
        recipe_prefix_offset,
        recipe_prefix_bytes,
        recipe_references,
        recipe_program_offset: u64::try_from(recipe_program_at).ok()?,
        recipe_program,
        next_record_index,
        next_byte_offset,
    })
}

fn parse_edge_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    (scope_reference_ordinal, header): (u32, &DesignRecordHeader),
    recipes: &[ConstructionRecipe],
    terminal_group_limit: Option<u64>,
) -> Option<Result<DesignEdgeOperand, CodecError>> {
    let next_record_delta = edge_recipe_terminal_delta(&scope.kind());
    let stream = native_stream(&scope.id)?;
    let parsed = parse_recipe_operand(
        bytes,
        records,
        stream,
        header,
        recipes,
        (
            ConstructionRecipeKind::Edge,
            RecipeOperandTerminator::RecordDelta(next_record_delta),
        ),
    )
    .or_else(|| {
        let limit = terminal_group_limit?;
        parse_recipe_operand(
            bytes,
            records,
            stream,
            header,
            recipes,
            (
                ConstructionRecipeKind::Edge,
                RecipeOperandTerminator::NextIndexedAfterRecipe { limit },
            ),
        )
    })?;
    let recipe_structure = match edge_recipe_structure(ctx, &parsed.recipe_program) {
        Ok(structure) => structure,
        Err(error) => return Some(Err(error)),
    };
    let surface_patch_recipe_structure =
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch {
            surface_patch_recipe_structure(&parsed.recipe_program, parsed.recipe_references.len())
        } else {
            None
        };
    let local_topology_references = recipe_structure.as_ref().and_then(|structure| {
        edge_recipe_local_topology_references(structure, parsed.recipe_references.len())
    });
    let id = design_record_id(
        stream
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .unwrap_or(stream),
        ":design-edge-operand#",
        header.byte_offset,
    );
    DesignEdgeOperand::try_new(
        crate::records::topology::edge_identity::DesignEdgeOperandDraft {
            id,
            scope_record_index: scope.record_index,
            scope_reference_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            paired_byte_offset: parsed.paired_byte_offset,
            paired_class_tag: parsed.paired_class_tag,
            recipe_record_index: parsed.recipe_record_index,
            recipe_record_byte_offset: parsed.recipe_record_byte_offset,
            recipe_id: parsed.recipe_id,
            recipe_prefix_offset: parsed.recipe_prefix_offset,
            recipe_prefix_bytes: parsed.recipe_prefix_bytes.into(),
            recipe_references: parsed.recipe_references,
            recipe_program_offset: parsed.recipe_program_offset,
            recipe_program: parsed.recipe_program,
            recipe_structure,
            surface_patch_recipe_structure,
            local_topology_references,
            candidate_faces: Vec::new(),
            result_candidate_faces: Vec::new(),
            result_boundary_edge_slots: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            terminal_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            preceding_boundary_edge_slots: Vec::new(),
            terminal_boundary_edge_slots: Vec::new(),
            changed_boundary_edge_slots: Vec::new(),
            deleted_boundary_edge_slots: Vec::new(),
            updated_boundary_edge_slots: Vec::new(),
            treatment_radius_candidates: Vec::new(),
            changed_boundary_edge_contexts: Vec::new(),
            terminal_boundary_edge_contexts: Vec::new(),
            terminal_reference_edge_slots: Vec::new(),
            recipe_reference_contexts: Vec::new(),
            recipe_selectors: Vec::new(),
            recipe_state_id: None,
            resolved_edge_slot: None,
            resolved_axis: None,
            next_record_index: parsed.next_record_index,
            next_byte_offset: parsed.next_byte_offset,
        },
    )
    .ok()
    .map(Ok)
}

/// Work cap for the side-sequence search of one recipe program.
///
/// Each search level tries every payload-prefix split of a candidate side,
/// and each candidate side branches the search, so a crafted program grows
/// the work combinatorially. Decoded Design recipe programs stay several
/// orders of magnitude below this cap.
pub(crate) const RECIPE_SIDE_SEARCH_WORK: usize = 1 << 22;

pub(crate) fn edge_recipe_structure(
    ctx: &DecodeContext<'_>,
    program: &[i32],
) -> Result<Option<crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>, CodecError> {
    edge_recipe_structure_within(ctx, &WorkBudget::new(RECIPE_SIDE_SEARCH_WORK), program)
}

/// Count the recipe programs whose side-sequence search stops at
/// [`RECIPE_SIDE_SEARCH_WORK`]. Their recipe structures stay unresolved.
pub(crate) fn recipe_side_searches_exhausted(
    ctx: &DecodeContext<'_>,
    edge_operands: &[DesignEdgeOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
) -> Result<usize, CodecError> {
    let mut exhausted = 0;
    for operand in edge_operands
        .iter()
        .filter(|operand| operand.recipe_structure.is_none())
    {
        let budget = WorkBudget::new(RECIPE_SIDE_SEARCH_WORK);
        edge_recipe_structure_within(ctx, &budget, &operand.recipe_program)?;
        exhausted += usize::from(budget.exhausted());
    }
    for node in face_operands
        .iter()
        .flat_map(|operand| &operand.recipe_nodes)
        .filter(|node| node.recipe_structure.is_none())
    {
        let Some(tail) = node.program.get(3..) else {
            continue;
        };
        let budget = WorkBudget::new(RECIPE_SIDE_SEARCH_WORK);
        face_recipe_structure_within(ctx, &budget, tail)?;
        exhausted += usize::from(budget.exhausted());
    }
    Ok(exhausted)
}

/// Decode the edge-recipe structure within `budget`. A search that exhausts
/// the budget yields no structure.
pub(crate) fn edge_recipe_structure_within(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget,
    program: &[i32],
) -> Result<Option<crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>, CodecError> {
    let Some(prelude_length) = edge_recipe_prelude_length(program, budget) else {
        return Ok(None);
    };
    let Some(tail) = program.get(prelude_length..) else {
        return Ok(None);
    };
    let structure = edge_recipe_structure_tail(ctx, budget, tail)?;
    Ok(structure.filter(|_| !budget.exhausted()))
}

pub(in crate::design::decode) fn edge_recipe_prelude_length(
    program: &[i32],
    budget: &WorkBudget,
) -> Option<usize> {
    let tag = |word: i32| matches!(word, -1..=1);
    if let Some([-1, -1, 2, 0, first, 1, second]) = program.get(..7) {
        return (budget.charge_by(7) && tag(*first) && tag(*second)).then_some(7);
    }
    let [0, first_tag, count] = program.get(..3)? else {
        return None;
    };
    let count = usize::try_from(*count).ok().filter(|count| *count != 0)?;
    let length = count.checked_mul(2)?.checked_add(3)?;
    if !tag(*first_tag) || length > program.len() || !budget.charge_by(length) {
        return None;
    }
    for (ordinal, pair) in program[3..length].chunks_exact(2).enumerate() {
        if pair[0] != i32::try_from(ordinal.checked_add(1)?).ok()? || !tag(pair[1]) {
            return None;
        }
    }
    Some(length)
}

/// Decode the alternate two-clause edge-recipe grammar owned by `SurfacePatch`.
///
/// The six fields in each clause are delimiter-bounded by `-1`. The first and
/// second fields name face references, the third and fifth fields name edge
/// references, and the fourth and sixth fields are zero pairs. The final field
/// is a counted sequence of the standard eight-word topology entries.
pub(crate) fn surface_patch_recipe_structure(
    program: &[i32],
    reference_count: usize,
) -> Option<crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure> {
    let mut remaining = program.get(7..)?;
    let (&root, tail) = remaining.split_first()?;
    if root != 2 {
        return None;
    }
    remaining = tail;
    let mut clauses = Vec::new();
    for _ in 0..2 {
        let mut fields = Vec::new();
        for _ in 0..6 {
            let delimiter_at = remaining.iter().position(|word| *word == -1)?;
            if delimiter_at > 2 {
                return None;
            }
            let field = remaining.get(..delimiter_at)?;
            let field = field.to_vec();
            if field.is_empty() || field.iter().any(|word| *word < 0) {
                return None;
            }
            let tail = remaining.get(delimiter_at + 1..)?;
            remaining = tail;
            fields.push(field);
        }
        let (&payload_entry_count, tail) = remaining.split_first()?;
        let Ok(payload_entry_count) = u32::try_from(payload_entry_count) else {
            return None;
        };
        let payload_word_count = usize::try_from(payload_entry_count)
            .ok()
            .and_then(|count| count.checked_mul(8))?;
        let payload = tail.get(..payload_word_count)?;
        let entries = edge_recipe_entries(payload)?;
        if u32::try_from(entries.len()).ok() != Some(payload_entry_count) {
            return None;
        }
        let (&delimiter, tail) = tail
            .get(payload_word_count..)
            .and_then(|tail| tail.split_first())?;
        if delimiter != -1 {
            return None;
        }
        remaining = tail;
        let [first, second, third, fourth, fifth, sixth] = fields.as_slice() else {
            return None;
        };
        if !(first.len() == 1 || first.len() == 2 && first[0] == 2)
            || second.len() != 1
            || third.len() != 2
            || third[0] != 2
            || fourth.as_slice() != [0, 0]
            || fifth.len() != 1
            || sixth.as_slice() != [0, 0]
        {
            return None;
        }
        let ordinal = |field: &[i32], position: usize| {
            let ordinal = usize::try_from(*field.get(position)?).ok()?;
            (ordinal < reference_count).then_some(u32::try_from(ordinal).ok()?)
        };
        let face_reference_ordinals =
            (|| Some([ordinal(first, first.len() - 1)?, ordinal(second, 0)?]))()?;
        let edge_reference_ordinals = (|| Some([ordinal(third, 1)?, ordinal(fifth, 0)?]))()?;
        clauses.push(
            crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                fields,
                face_reference_ordinals,
                edge_reference_ordinals,
                entries,
            },
        );
    }
    if let Some(&delimiter) = remaining.first() {
        if delimiter != 0 {
            return None;
        }
        remaining = &remaining[1..];
    }
    if !remaining.is_empty() {
        return None;
    }
    clauses.try_into().ok().map(|clauses| {
        crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure { clauses }
    })
}

fn edge_recipe_local_topology_references(
    structure: &crate::records::topology::edge_recipe::DesignEdgeRecipeStructure,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    topology_recipe_references(
        structure.sides.iter().flat_map(|side| {
            std::iter::once(side.header_value).chain(side.scalars.iter().copied())
        }),
        reference_count,
    )
}

fn edge_recipe_structure_tail(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget,
    program: &[i32],
) -> Result<Option<crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>, CodecError> {
    let Some((&root, mut remaining)) = program.split_first() else {
        return Ok(None);
    };
    let Ok(side_count) = usize::try_from(root) else {
        return Ok(None);
    };
    if side_count == 0 {
        return Ok(None);
    }
    let Some(delimited) = edge_recipe_delimiter(remaining) else {
        return Ok(None);
    };
    remaining = delimited;
    let mut structure = None;
    for RecipeSideSequence {
        sides,
        remaining: tail,
    } in
        edge_recipe_side_sequences(ctx, budget, remaining, side_count, RecipeSideGrammar::Edge)?
    {
        if !matches!(tail, [-1..=1]) {
            continue;
        }
        if structure.is_some() {
            return Ok(None);
        }
        structure =
            Some(crate::records::topology::edge_recipe::DesignEdgeRecipeStructure { root, sides });
    }
    Ok(structure)
}

struct RecipeSideSequence<'a> {
    sides: Vec<DesignTopologyRecipeSide>,
    remaining: &'a [i32],
}

fn edge_recipe_side_sequences<'w>(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget,
    words: &'w [i32],
    side_count: usize,
    grammar: RecipeSideGrammar,
) -> Result<Vec<RecipeSideSequence<'w>>, CodecError> {
    let _depth = ctx.enter_nested("f3d recipe side recursion")?;
    if !budget.charge() {
        return Ok(Vec::new());
    }
    if side_count == 0 {
        return Ok(vec![RecipeSideSequence {
            sides: Vec::new(),
            remaining: words,
        }]);
    }
    let mut out = Vec::new();
    for (side, tail) in edge_recipe_counted_side_candidates(budget, words, grammar) {
        let remaining = if side_count == 1 {
            tail
        } else if let Some(remaining) = grammar.delimiter(tail) {
            remaining
        } else {
            continue;
        };
        for RecipeSideSequence {
            sides: mut following,
            remaining: tail,
        } in edge_recipe_side_sequences(ctx, budget, remaining, side_count - 1, grammar)?
        {
            if !budget.charge() {
                return Ok(Vec::new());
            }
            following.insert(0, side.clone());
            out.push(RecipeSideSequence {
                sides: following,
                remaining: tail,
            });
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RecipeSideGrammar {
    Edge,
    Face,
}

impl RecipeSideGrammar {
    fn delimiter(self, words: &[i32]) -> Option<&[i32]> {
        match self {
            Self::Edge => edge_recipe_delimiter(words),
            Self::Face => recipe_delimiter(words),
        }
    }
}

fn edge_recipe_delimiter(words: &[i32]) -> Option<&[i32]> {
    matches!(words.first(), Some(-1..=1)).then(|| &words[1..])
}

fn recipe_delimiter(words: &[i32]) -> Option<&[i32]> {
    matches!(words.first(), Some(-1 | 0)).then(|| &words[1..])
}

fn complete_recipe_payload_prefix(budget: &WorkBudget, prefix: &[i32]) -> bool {
    if prefix == [0] {
        return true;
    }
    if prefix.is_empty() {
        return false;
    }
    let mut pending = vec![prefix];
    while let Some(remaining) = pending.pop() {
        if remaining.is_empty() {
            return true;
        }
        if !budget.charge_by(remaining.len()) {
            return false;
        }
        let Some(at) = remaining.iter().position(|word| *word < 0) else {
            continue;
        };
        if remaining[at] != -1 {
            continue;
        }
        if at >= 1 && remaining.get(at + 1..at + 4) == Some(&[0, 0, -1]) {
            pending.push(&remaining[at + 4..]);
        }
        if at >= 4 && remaining.get(at - 3..at) == Some(&[0, 0, 0]) {
            pending.push(&remaining[at + 1..]);
        }
    }
    false
}

fn complete_face_recipe_payload_prefix(prefix: &[i32]) -> bool {
    if prefix == [0] {
        return true;
    }
    let mut remaining = prefix;
    let mut field_count = 0;
    while !remaining.is_empty() {
        let Some(delimiter_at) = remaining.iter().position(|word| *word == -1) else {
            return false;
        };
        let field = &remaining[..delimiter_at];
        if field.is_empty() || field[0] <= 0 || field.iter().any(|word| *word < 0) {
            return false;
        }
        remaining = &remaining[delimiter_at + 1..];
        if !matches!(remaining.get(..3), Some([0, 0, -1])) {
            return false;
        }
        remaining = &remaining[3..];
        field_count += 1;
    }
    field_count > 0
}

fn edge_recipe_counted_side_candidates<'w>(
    budget: &WorkBudget,
    words: &'w [i32],
    grammar: RecipeSideGrammar,
) -> Vec<(DesignTopologyRecipeSide, &'w [i32])> {
    let Some(field_count) = words
        .first()
        .and_then(|word| u32::try_from(*word).ok())
        .and_then(std::num::NonZeroU32::new)
    else {
        return Vec::new();
    };
    if grammar == RecipeSideGrammar::Face && field_count.get() < 2 {
        return Vec::new();
    }
    let Some(scalar_count) = usize::try_from(field_count.get())
        .ok()
        .and_then(|count| count.checked_sub(1))
    else {
        return Vec::new();
    };
    let Some(&header_value) = words.get(1) else {
        return Vec::new();
    };
    let Some(mut remaining) = words.get(2..).and_then(|tail| grammar.delimiter(tail)) else {
        return Vec::new();
    };
    // Each scalar consumes at least one remaining word; a larger count is
    // corrupt and must not reach the allocator.
    if scalar_count > remaining.len() {
        return Vec::new();
    }
    let mut scalars = Vec::new();
    for _ in 0..scalar_count {
        let Some((&scalar, tail)) = remaining.split_first() else {
            return Vec::new();
        };
        scalars.push(scalar);
        let Some(tail) = grammar.delimiter(tail) else {
            return Vec::new();
        };
        remaining = tail;
    }
    let mut candidates = Vec::new();
    for entry_count_at in 0..remaining.len() {
        // The prefix check scans `entry_count_at` words.
        if !budget.charge_by(entry_count_at + 1) {
            return Vec::new();
        }
        let prefix = &remaining[..entry_count_at];
        let complete_prefix = match grammar {
            RecipeSideGrammar::Edge => complete_recipe_payload_prefix(budget, prefix),
            RecipeSideGrammar::Face => complete_face_recipe_payload_prefix(prefix),
        };
        if !complete_prefix {
            continue;
        }
        let candidate = (|| {
            let payload_entry_count = u32::try_from(*remaining.get(entry_count_at)?).ok()?;
            if grammar == RecipeSideGrammar::Face && entry_count_at != 1 && payload_entry_count == 0
            {
                return None;
            }
            let payload_len = usize::try_from(payload_entry_count).ok()?.checked_mul(8)?;
            let entries_at = entry_count_at.checked_add(1)?;
            let entries_end = entries_at.checked_add(payload_len)?;
            Some((entries_at, entries_end, remaining.get(entries_end..)?))
        })();
        let Some((entries_at, entries_end, tail)) = candidate else {
            continue;
        };
        let Some(entries) = edge_recipe_entries(&remaining[entries_at..entries_end]) else {
            continue;
        };
        let side = DesignTopologyRecipeSide {
            header_value,
            scalars: scalars.clone(),
            payload_prefix: remaining[..entry_count_at].to_vec(),
            entries,
        };
        candidates.push((side, tail));
    }
    candidates
}

pub(crate) fn face_recipe_structure(
    ctx: &DecodeContext<'_>,
    program: &[i32],
) -> Result<Option<crate::records::topology::face::DesignFaceRecipeStructure>, CodecError> {
    face_recipe_structure_within(ctx, &WorkBudget::new(RECIPE_SIDE_SEARCH_WORK), program)
}

/// Decode the face-recipe structure within `budget`. A search that exhausts
/// the budget yields no structure.
pub(crate) fn face_recipe_structure_within(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget,
    program: &[i32],
) -> Result<Option<crate::records::topology::face::DesignFaceRecipeStructure>, CodecError> {
    let Some((&root, remaining)) = program.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let Some((&first_prelude, remaining)) = remaining.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let Some((&second_prelude, remaining)) = remaining.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let mut structure = None;
    for RecipeSideSequence {
        sides,
        remaining: tail,
    } in edge_recipe_side_sequences(ctx, budget, remaining, 2, RecipeSideGrammar::Face)?
    {
        let postlude = match tail {
            [] | [-1 | 0] => None,
            [-1, value, -1, 0, 0, -1] => Some(*value),
            _ => continue,
        };
        let Ok(sides) = sides.try_into() else {
            continue;
        };
        if structure.is_some() {
            return Ok(None);
        }
        structure = Some(crate::records::topology::face::DesignFaceRecipeStructure {
            root,
            prelude: [first_prelude, second_prelude],
            sides,
            postlude_value: postlude,
        });
    }
    Ok(structure.filter(|_| !budget.exhausted()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaceRecipeProgramKind {
    Terminal,
    Counted { header_value: usize },
}

pub(crate) fn face_recipe_program_kind(program: &[i32]) -> Option<FaceRecipeProgramKind> {
    if matches!(program, [0, -1 | 0]) {
        return Some(FaceRecipeProgramKind::Terminal);
    }
    if !matches!(program.get(0..2), Some([0, -1 | 0])) {
        return None;
    }
    let header_value = usize::try_from(*program.get(2)?).ok()?;
    (header_value > 0 && header_value <= 100_000)
        .then_some(FaceRecipeProgramKind::Counted { header_value })
}

fn face_recipe_nodes(
    ctx: &DecodeContext<'_>,
    recipe_program: &[i32],
    recipe_program_at: usize,
    program_kind: FaceRecipeProgramKind,
) -> Result<Option<Vec<crate::records::topology::face::DesignFaceRecipeNode>>, CodecError> {
    let mut recipe_node_indices = Vec::new();
    for (index, values) in recipe_program.windows(3).enumerate() {
        if values != [-1, -1, 2] {
            continue;
        }
        recipe_node_indices.push(index);
    }
    if recipe_node_indices.first().is_some_and(|index| *index < 3) {
        return Ok(None);
    }
    if program_kind == FaceRecipeProgramKind::Terminal && !recipe_node_indices.is_empty() {
        return Ok(None);
    }
    let node_ranges = recipe_node_indices.iter().copied().zip(
        recipe_node_indices
            .iter()
            .copied()
            .skip(1)
            .chain(std::iter::once(recipe_program.len())),
    );
    let mut recipe_nodes = Vec::new();
    for (start, end) in node_ranges {
        let Some(program) = recipe_program.get(start..end) else {
            return Ok(None);
        };
        let Some(byte_offset) = start
            .checked_mul(4)
            .and_then(|offset| recipe_program_at.checked_add(offset))
            .and_then(|offset| u64::try_from(offset).ok())
        else {
            return Ok(None);
        };
        let Some(end_byte_offset) = end
            .checked_mul(4)
            .and_then(|offset| recipe_program_at.checked_add(offset))
            .and_then(|offset| u64::try_from(offset).ok())
        else {
            return Ok(None);
        };
        let recipe_structure = match program.get(3..) {
            Some(tail) => face_recipe_structure(ctx, tail)?,
            None => None,
        };
        let program = program.to_vec();
        recipe_nodes.push(crate::records::topology::face::DesignFaceRecipeNode {
            byte_offset,
            end_byte_offset,
            program,
            recipe_structure,
        });
    }
    Ok(Some(recipe_nodes))
}

fn topology_recipe_references(
    words: impl IntoIterator<Item = i32>,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    let mut references = Vec::new();
    for word in words {
        if word == 0 {
            continue;
        }
        let ordinal = u32::try_from(word)
            .ok()
            .and_then(std::num::NonZeroU32::new)?;
        if usize::try_from(ordinal.get())
            .ok()
            .is_none_or(|index| index > reference_count)
        {
            return None;
        }
        references.push(ordinal);
    }
    Some(references)
}

fn edge_recipe_entries(words: &[i32]) -> Option<Vec<DesignTopologyRecipeEntry>> {
    let mut entries = Vec::new();
    for entry in words.chunks_exact(8) {
        let parsed = (|| {
            let selector = entry[0];
            if selector < 0 {
                return None;
            }
            let boundary_edge_count = std::num::NonZeroU32::new(u32::try_from(entry[1]).ok()?)?;
            let topology_triplets = [
                edge_recipe_topology_triplet(&entry[2..5], boundary_edge_count)?,
                edge_recipe_topology_triplet(&entry[5..8], boundary_edge_count)?,
            ];
            topology_triplets
                .iter()
                .all(|triplet| triplet.outer.get() <= boundary_edge_count.get())
                .then_some(DesignTopologyRecipeEntry {
                    selector,
                    boundary_edge_count,
                    topology_triplets,
                })
        })()?;
        if entries
            .last()
            .is_some_and(|last: &DesignTopologyRecipeEntry| last.selector >= parsed.selector)
        {
            return None;
        }
        entries.push(parsed);
    }
    Some(entries)
}

fn edge_recipe_topology_triplet(
    words: &[i32],
    boundary_edge_count: std::num::NonZeroU32,
) -> Option<DesignTopologyRecipeTriplet> {
    let [outer, middle, repeated_outer] = words else {
        return None;
    };
    if outer != repeated_outer {
        return None;
    }
    let outer = std::num::NonZeroU32::new(u32::try_from(*outer).ok()?)?;
    let vertex_ordinal = outer.get().checked_sub(1)?;
    let incident = if *middle == i32::try_from(outer.get()).ok()? {
        Some((
            crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Following,
            vertex_ordinal,
        ))
    } else if *middle >= 0 && middle.checked_add(1) == i32::try_from(outer.get()).ok() {
        Some((
            crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Preceding,
            vertex_ordinal
                .checked_add(boundary_edge_count.get())?
                .checked_sub(1)?
                % boundary_edge_count.get(),
        ))
    } else {
        None
    };
    Some(DesignTopologyRecipeTriplet {
        outer,
        middle: *middle,
        incident: incident.map(|(side, ordinal)| {
            crate::records::topology::edge_recipe::DesignTopologyIncident { ordinal, side }
        }),
    })
}

/// Find the indexed header that terminates a face-recipe member.
///
/// The ordinary envelope terminates at `N+4`. One serialized generation
/// omits that header and terminates at `N+5`. When neither expected index is
/// present within the enclosing boundary, the first following valid indexed
/// header terminates the envelope; its record index has no fixed delta from
/// `N`. Select an expected continuation before applying that fallback.
fn face_recipe_next_boundary(
    bytes: &[u8],
    position: usize,
    record_index: u32,
    limit: Option<u64>,
) -> Option<(usize, u32)> {
    let within_limit = |offset: usize| {
        limit.is_none_or(|limit| {
            usize::try_from(limit)
                .ok()
                .is_some_and(|limit| offset <= limit)
        })
    };
    [record_index.checked_add(4)?, record_index.checked_add(5)?]
        .into_iter()
        .filter_map(|expected_index| {
            next_indexed_record_offset_with_index(bytes, position, expected_index)
                .map(|offset| (expected_index, offset))
        })
        .filter(|(_, offset)| within_limit(*offset))
        .min_by_key(|(_, offset)| *offset)
        .or_else(|| {
            let offset = next_indexed_record_offset(bytes, position)?;
            let record_index = indexed_record_header_at(bytes, offset)?.record_index;
            within_limit(offset).then_some((record_index, offset))
        })
        .map(|(record_index, offset)| (offset, record_index))
}

#[derive(Clone, Copy)]
pub(crate) struct FaceOperandFrame<'a> {
    pub(crate) scope: &'a DesignParameterScope,
    pub(crate) scope_reference_ordinal: u32,
    pub(crate) group_ownership: Option<(u32, u32)>,
    pub(crate) next_byte_offset: Option<u64>,
    pub(crate) header: &'a DesignRecordHeader,
}

pub(super) fn parse_face_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    input: FaceOperandFrame<'_>,
    recipes: &[ConstructionRecipe],
) -> Option<Result<DesignFaceOperand, CodecError>> {
    let FaceOperandFrame {
        scope,
        scope_reference_ordinal,
        group_ownership,
        next_byte_offset,
        header,
    } = input;
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut offsets = [0usize; 5];
    let mut position = start.checked_add(11)?;
    for (delta, slot) in offsets[..4].iter_mut().enumerate() {
        let record_index = header
            .record_index
            .checked_add(u32::try_from(delta).ok()?)?;
        let offset = records.first_at_or_after(position, record_index)?;
        *slot = offset;
        position = offset.checked_add(11)?;
    }
    let stream = native_stream(&scope.id)?;
    let recipe_start = u64::try_from(offsets[3]).ok()?;
    let first_recipe = recipes
        .iter()
        .filter(|recipe| {
            native_stream(&recipe.id) == Some(stream)
                && recipe.byte_offset > recipe_start
                && next_byte_offset.is_none_or(|end| recipe.byte_offset < end)
        })
        .min_by_key(|recipe| recipe.byte_offset)?;
    let family_name_len = match first_recipe.kind {
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        _ => return None,
    };
    let program_start = usize::try_from(first_recipe.byte_offset)
        .ok()?
        .checked_add(family_name_len)?;
    let (immediate_next, next_record_index) =
        face_recipe_next_boundary(bytes, program_start, header.record_index, next_byte_offset)?;
    offsets[4] = immediate_next;
    let mut indexed = [("", 0u32); 5];
    for (offset, slot) in offsets.iter().zip(&mut indexed) {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, *offset, 0..=2000, u8::is_ascii_graphic)?;
        *slot = (class_tag, View::u32_le_at(bytes, after_tag)?);
    }
    let recipe_record_index = header.record_index.checked_add(3)?;
    if indexed[0].1 != header.record_index
        || indexed[1].1 != header.record_index.checked_add(1)?
        || indexed[2].1 != header.record_index.checked_add(2)?
        || indexed[3].1 != recipe_record_index
        || indexed[4].1 != next_record_index
    {
        return None;
    }
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let mut matches = recipes.iter().filter(|recipe| {
        recipe.byte_offset > recipe_start
            && recipe.byte_offset < next_byte_offset
            && matches!(
                recipe.kind,
                ConstructionRecipeKind::Face | ConstructionRecipeKind::BoundedFace
            )
            && native_stream(&recipe.id) == Some(stream)
    });
    let recipe = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let family_name_len = match recipe.kind {
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        _ => return None,
    };
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        family_name_len,
    )?;
    let recipe_references =
        decode_recipe_references(recipe_prefix_bytes, u64::try_from(recipe_prefix_at).ok()?);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(family_name_len)?;
    let recipe_program_end = usize::try_from(next_byte_offset).ok()?;
    if recipe_program_end.checked_sub(recipe_program_at)? > 64 * 1024 {
        return None;
    }
    let recipe_program = contiguous_i32_program(bytes, recipe_program_at, recipe_program_end)?;
    let recipe_prefix_bytes = recipe_prefix_bytes.to_vec();
    let program_kind = face_recipe_program_kind(&recipe_program)?;
    let recipe_program_offset = u64::try_from(recipe_program_at).ok()?;
    let recipe_nodes =
        match face_recipe_nodes(ctx, &recipe_program, recipe_program_at, program_kind) {
            Ok(Some(nodes)) => nodes,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
    let recipe_id = recipe.id.clone();
    let id = design_record_id(
        stream
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .unwrap_or(stream),
        ":design-face-operand#",
        header.byte_offset,
    );
    DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
        id,
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        group: group_ownership.map(|(group_record_index, group_member_ordinal)| {
            crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index,
                group_member_ordinal,
            }
        }),
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: crate::design::decode::text::class_tag_from_view(indexed[0].0).ok()?,
        recipe_record_index,
        recipe_record_byte_offset: recipe_start,
        recipe_id,
        recipe_prefix_offset: u64::try_from(recipe_prefix_at).ok()?,
        recipe_prefix_bytes,
        recipe_references,
        recipe_kind: recipe.kind,
        recipe_program_offset,
        recipe_program,
        recipe_nodes,
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index,
        next_byte_offset,
    })
    .ok()
    .map(Ok)
}

pub(in crate::design) fn has_typed_edge_treatment_group(
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> bool {
    matches!(
        design_feature_family(kind),
        Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
    )
}

/// Apply the selection-identity completeness rule to a parsed group candidate.
///
/// A localized edge-treatment reference table can contain selections that do
/// not use counted groups. Such a reference is a group only when its parsed
/// candidate also resolves through one of the selection-identity grammars.
pub(crate) fn construction_operand_group_is_retained(
    scope_kind: Option<&crate::records::feature::scope::DesignFeatureKind>,
    has_selection_identity: bool,
) -> bool {
    !scope_kind.is_some_and(crate::design::is_localized_edge_treatment_kind)
        || has_selection_identity
}

#[cfg(test)]
mod tests;

/// Chain-record bytes after its fourteen zero bytes and before the typed
/// Sketch scope reference.
const TYPED_PROFILE_CHAIN_MARKER: [u8; 6] = [1, 1, 1, 0, 0, 0];

/// Decode the typed-reference profile selections named by construction
/// operand groups: a selection prefix, a chain record naming the owning
/// Sketch scope, and an identity record listing curve loops.
pub(crate) fn decode_profile_curve_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<crate::records::topology::entity_selection::DesignProfileCurveSelection>, CodecError>
{
    let headers = indexed_operand_headers(headers);
    let mut records_by_stream = HashMap::new();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Ok(start) = usize::try_from(header.byte_offset) else {
                continue;
            };
            // Only a typed selection prefix selects this form.
            if crate::bytes::typed_local_reference_target(
                bytes,
                start + typed_selection::IDENTITY_REFERENCE,
            )
            .is_none()
            {
                continue;
            }
            let records = records_by_stream
                .entry(stream)
                .or_insert_with(|| IndexedRecordOffsets::build(bytes));
            let Some(mut selection) =
                parse_typed_profile_curve_selection(ctx, bytes, records, group, ordinal, header)?
            else {
                continue;
            };
            selection.id = design_record_id(
                &entry.name,
                ":design-profile-curve-selection#",
                header.byte_offset,
            );
            out.push(selection);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Parse one typed profile selection and its chain records.
fn parse_typed_profile_curve_selection_prefix(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<(
    crate::records::topology::entity_selection::DesignProfileCurveSelection,
    usize,
)> {
    use crate::bytes::typed_local_reference_target as target;
    use crate::records::topology::entity_selection::DesignProfileCurveSelection;
    let Ok(start) = usize::try_from(header.byte_offset) else {
        return None;
    };
    let record_index = header.record_index;
    let (Some(scope_index), Some(identity_index)) =
        (record_index.checked_add(1), record_index.checked_add(3))
    else {
        return None;
    };
    // Selection prefix: ten zero bytes, a typed reference to the identity
    // record, u32 1, the LP-UTF16 asset GUID, and the LP-UTF16 decimal
    // owner text.
    if bytes.get(start + typed_selection::ZERO_RUN_10..start + typed_selection::IDENTITY_REFERENCE)
        != Some(&[0; 10][..])
        || target(bytes, start + typed_selection::IDENTITY_REFERENCE) != Some(identity_index)
        || View::u32_le_at(bytes, start + typed_selection::CONSTANT_ONE) != Some(1)
    {
        return None;
    }
    let (_, after_asset) = lp_utf16_bounded(bytes, start + typed_selection::LEN, 36..=36)?;
    let (owner_text, _) = lp_utf16_bounded(bytes, after_asset, 1..=20)?;
    if !owner_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // Chain record at index plus one names the owning Sketch scope.
    let &scope_at = records.offsets(scope_index).first()?;
    if bytes.get(scope_at + typed_chain::ZERO_RUN_14..scope_at + typed_chain::CHAIN_MARKER)
        != Some(&[0; 14][..])
        || bytes.get(
            scope_at + typed_chain::CHAIN_MARKER..scope_at + typed_chain::SKETCH_SCOPE_REFERENCE,
        ) != Some(&TYPED_PROFILE_CHAIN_MARKER[..])
    {
        return None;
    }
    let sketch_scope_record_index = target(bytes, scope_at + typed_chain::SKETCH_SCOPE_REFERENCE)?;
    // Identity record at index plus three: ten zero bytes, a typed reference
    // to the selection record, then two counted loop lists.
    let &identity_at = records.offsets(identity_index).first()?;
    if bytes.get(
        identity_at + typed_identity::ZERO_RUN_10
            ..identity_at + typed_identity::SELECTION_REFERENCE,
    ) != Some(&[0; 10][..])
        || target(bytes, identity_at + typed_identity::SELECTION_REFERENCE) != Some(record_index)
    {
        return None;
    }
    let identity_end =
        next_indexed_record_offset(bytes, identity_at + typed_identity::ZERO_RUN_10)?;
    Some((
        DesignProfileCurveSelection {
            id: String::new(),
            scope_record_index: group.scope_record_index,
            group_record_index: group.record_index,
            group_member_ordinal,
            record_index,
            byte_offset: header.byte_offset,
            owner_text_offset: cadmpeg_core::decode::u64_from_index(after_asset + 4),
            owner_text,
            sketch_scope_record_index,
            sketch_scope_offset: cadmpeg_core::decode::u64_from_index(
                scope_at + typed_chain::SKETCH_SCOPE_REFERENCE + 1,
            ),
            identity_record_offset: cadmpeg_core::decode::u64_from_index(identity_at),
            identity_end_offset: cadmpeg_core::decode::u64_from_index(identity_end),
            loop_lists: [Vec::new(), Vec::new()],
        },
        identity_end,
    ))
}

fn parse_typed_profile_curve_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: &DesignConstructionOperandGroup,
    ordinal: u32,
    header: &DesignRecordHeader,
) -> Result<
    Option<crate::records::topology::entity_selection::DesignProfileCurveSelection>,
    CodecError,
> {
    let Some((mut selection, end)) =
        parse_typed_profile_curve_selection_prefix(bytes, records, group, ordinal, header)
    else {
        return Ok(None);
    };
    let start = usize::try_from(selection.identity_record_offset)
        .map_err(|_| CodecError::malformed("profile identity offset exceeds address space"))?;
    let Some(lists) =
        parse_profile_loop_lists(ctx, bytes, start + typed_identity::FIRST_REGION_COUNT, end)?
    else {
        return Ok(None);
    };
    selection.loop_lists = lists;
    Ok(selection.valid_layout().then_some(selection))
}

fn parse_profile_loop_lists(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<
    Option<[Vec<Vec<crate::records::topology::entity_selection::DesignProfileCurveLoop>>; 2]>,
    CodecError,
> {
    use crate::records::topology::entity_selection::{
        DesignProfileCurveEntry, DesignProfileCurveLoop,
    };
    macro_rules! field {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let mut view = field!(View::over_retained(bytes).child(start, end));
    let mut lists = [Vec::new(), Vec::new()];
    for (list_index, regions) in lists.iter_mut().enumerate() {
        let count = field!(view.u32_le());
        let count = field!(view.counted(u64::from(count), 49)).get();
        if list_index == 0 && count == 0 {
            return Ok(None);
        }
        *regions = ctx.alloc_filled(count, Vec::new(), "F3D profile regions")?;
        for loops in regions {
            let count = field!(view.u32_le());
            let count = field!(view.counted(u64::from(count), 45)).get();
            if count == 0 {
                return Ok(None);
            }
            *loops = ctx.alloc_filled(
                count,
                DesignProfileCurveLoop::default(),
                "F3D profile loops",
            )?;
            for loop_ in loops {
                let count = field!(view.u32_le());
                let count = field!(view.counted(u64::from(count), 40)).get();
                if count == 0 {
                    return Ok(None);
                }
                loop_.entries = ctx.alloc_filled(
                    count,
                    DesignProfileCurveEntry::default(),
                    "F3D profile curve entries",
                )?;
                for entry in &mut loop_.entries {
                    if view.u32_le() != Some(2) {
                        return Ok(None);
                    }
                    let primary_id_offset = cadmpeg_core::decode::u64_from_index(view.position());
                    *entry = DesignProfileCurveEntry {
                        primary_id: field!(view.u64_le()),
                        primary_id_offset,
                        secondary_id: field!(view.u64_le()),
                        entry_fields: [
                            field!(view.u32_le()),
                            field!(view.u32_le()),
                            field!(view.u32_le()),
                        ],
                    };
                    if view.take(8) != Some(&[0; 8][..]) {
                        return Ok(None);
                    }
                }
                loop_.outer = match view.u8() {
                    Some(0) => false,
                    Some(1) => true,
                    _ => return Ok(None),
                };
            }
        }
    }
    Ok((view.remaining() == 0).then_some(lists))
}
