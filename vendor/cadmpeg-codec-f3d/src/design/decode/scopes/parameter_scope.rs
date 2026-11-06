// SPDX-License-Identifier: Apache-2.0
//! Decode parameter scopes and parse one scope payload.

mod family;

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::decode::u64_from_index;

use super::assembly_alignment::exact_assembly_alignment_with_types;
use super::axial_assembly::bind_axial_assembly_operand_targets;
use super::axial_assembly::bind_joint_origin_frames_from_assemblies;
use super::base_feature::{
    exact_base_feature_construction, exact_timeline_owned_base_feature, timeline_body_owners,
};
use super::coil::bind_coil_extent_from_parameters;
use super::coil::exact_coil_discriminators;
use super::coil::exact_coil_placement;
use super::coil::exact_long_coil_transform;
use super::combine::exact_combine_operation;
use super::component_constructions::bind_component_pattern_occurrences;
use super::component_constructions::exact_component_insert_construction;
use super::component_constructions::exact_copy_paste_component_operation;
use super::component_constructions::exact_derived_instance_construction;
use super::copy_paste_bodies::exact_copy_paste_bodies_operation;
use super::direct_face::exact_direct_face_operation;
use super::direct_face::exact_move_face_operation;
use super::direct_face::exact_move_operation;
use super::direct_face::exact_scale_operation;
use super::draft::exact_draft_operation_with_owners;
use super::emboss::exact_emboss_construction;
use super::extrude::{exact_extrude_prologue, exact_typed_extrude_prologue};
use super::fixed_parameters::exact_fixed_chamfer_parameters;
use super::fixed_parameters::exact_fixed_extrude_parameters;
use super::fixed_parameters::exact_fixed_fillet_parameters;
use super::hole::exact_hole_construction;
use super::path_feature::exact_path_feature_construction;
use super::pattern::exact_circular_pattern_construction_with_owners;
use super::pattern::exact_rectangular_pattern_construction;
use super::point_data::exact_work_point_construction;
use super::sheet_metal::bind_hem_operation_from_parameters;
use super::sheet_metal::exact_base_flange_operation;
use super::sheet_metal::exact_edge_flange_operation;
use super::solid_primitive::exact_solid_primitive;
use super::surfaces::exact_ruled_surface_operation;
use super::surfaces::exact_surface_extend_operation;
use super::surfaces::exact_surface_offset_operation;
use super::surfaces::exact_surface_stitch_operation;
use super::thread::exact_thread_construction;
use super::work_geometry::exact_joint_origin_frame;
use super::work_geometry::exact_work_axis_construction;
use super::work_geometry::exact_work_plane_frame;
use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::decode::assembly::exact_legacy_as_built_421_operands;
use crate::design::decode::canvas::typed_canvas_reference;
use crate::design::decode::operands::RecordFrame;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::design_record_id;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids;
use crate::ids::native_stream;
use crate::layout::canvas_scope_tail_98 as canvas_tail;
use crate::layout::canvas_scope_typed_tail as typed_canvas_tail;
use crate::layout::thread_bound_hole_tail as hole_tail;
use crate::layout::typed_reference_parameter_scope_tail as typed_tail;
use crate::records::feature::assembly;
use crate::records::feature::coil;
use crate::records::feature::direct_face;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Design records that parameter-scope decoding reads.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ParameterScopeSources<'a> {
    pub(crate) entities: &'a [crate::records::entity_header::DesignEntityHeader],
    pub(crate) types: &'a [crate::records::entity_header::SegmentType],
    pub(crate) parameters: &'a [crate::records::parameters::DesignParameter],
    pub(crate) parameter_owners: &'a [crate::records::parameters::DesignParameterOwner],
    pub(crate) component_occurrences:
        &'a [crate::records::feature::assembly_features::DesignComponentOccurrence],
    pub(crate) recipes: &'a [crate::records::recipes::ConstructionRecipe],
}

#[derive(Debug, Default)]
pub(crate) struct DecodedParameterScopes {
    pub(crate) scopes: Vec<DesignParameterScope>,
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Decode every canonical sketch or construction-operation scope, including
/// scopes that own no parameters and therefore have no owner-frame backlink.
pub(crate) fn decode_parameter_scopes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    sources: &ParameterScopeSources<'_>,
) -> Result<DecodedParameterScopes, CodecError> {
    let &ParameterScopeSources {
        entities,
        types,
        parameters,
        parameter_owners,
        component_occurrences,
        recipes,
    } = sources;
    let mut out = Vec::new();
    let mut losses = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = crate::ids::native_scope(&entry.name);
        let records = IndexedRecordOffsets::build(bytes);
        let stream_types = crate::design::decode::meta::stream_types_by_entity(types, &entry.name);
        let class_types =
            crate::design::decode::meta::stream_types_by_class_tag(types, &entry.name);
        let body_owners = timeline_body_owners(bytes, &records, class_types.values().copied());
        let stream_scope_start = out.len();
        for header in parameter_scope_candidate_headers(bytes, &records)? {
            let Some(mut scope) = parse_parameter_scope_with_type_guid(
                bytes,
                &records,
                header.record_index,
                &header.class_tag,
                header.byte_offset,
                header
                    .class_tag
                    .as_str()
                    .parse::<u32>()
                    .ok()
                    .and_then(|tag| class_types.get(&tag))
                    .map(|row| row.type_guid.as_str()),
            )?
            else {
                if let Some(tail_length) = unparsed_rigid_group_tail_length(
                    bytes,
                    &records,
                    header.record_index,
                    header.byte_offset,
                ) {
                    losses.push(crate::loss::F3dLossCode::AssemblyRigidGroupFrameUnparsed.note(
                        format!("Design rigid-group frame {} in {} at {} has no admitted layout ({tail_length}-byte tail).", header.record_index, entry.name, header.byte_offset),
                    ));
                }
                continue;
            };
            scope.id =
                design_record_id(&entry.name, ":design-parameter-scope#", scope.byte_offset());
            let normal = super::super::patch::surface_patch_normal(
                bytes,
                &scope,
                stream_types.get(&u64::from(scope.record_index)).copied(),
            );
            if let scope::DesignScopePayloadMut::SurfacePatch(patch) = scope.payload_mut() {
                patch.normal = normal;
            }
            let emboss = exact_emboss_construction(ctx, bytes, &scope)?;
            if let scope::DesignScopePayloadMut::Emboss(slot) = scope.payload_mut() {
                *slot = emboss;
            }
            if let Some(loss) = extrude_extent_native_loss(&scope) {
                losses.push(loss);
            }
            bind_coil_extent_from_parameters(&mut scope, parameters, parameter_owners);
            bind_hem_operation_from_parameters(bytes, &mut scope, parameters, parameter_owners);
            if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch) {
                let start = usize::try_from(scope.byte_offset()).ok();
                let end = usize::try_from(scope.paired_byte_offset()).ok();
                let frame = start
                    .zip(end)
                    .and_then(|(start, end)| bytes.get(start..end));
                let mut unique_match = None;
                let mut multiple_matches = false;
                if let Some(frame) = frame {
                    // One pass over the frame: the first offset of every
                    // marked reference (a one byte, a u32 suffix, six zero
                    // bytes), then each eligible entity looks its suffix up.
                    let first_at = first_marked_reference_offsets(frame);
                    for entity in entities {
                        if native_stream(&entity.id) != Some(stream.as_str())
                            || !entity.in_sketch_module()
                            || entity.entity_id.suffix() > u64::from(u32::MAX)
                        {
                            continue;
                        }
                        let Ok(suffix) = u32::try_from(entity.entity_id.suffix()) else {
                            continue;
                        };
                        if let Some(at) = first_at.get(&suffix) {
                            if unique_match.replace((entity, at + 1)).is_some() {
                                multiple_matches = true;
                            }
                        }
                    }
                }
                if let Some((entity, relative_offset)) = unique_match.filter(|_| !multiple_matches)
                {
                    let Some(entity_reference_offset) = scope
                        .byte_offset()
                        .checked_add(u64_from_index(relative_offset))
                    else {
                        continue;
                    };
                    if let scope::DesignScopePayloadMut::Sketch(slot)
                    | scope::DesignScopePayloadMut::Esquisse(slot)
                    | scope::DesignScopePayloadMut::Skizze(slot)
                    | scope::DesignScopePayloadMut::Esboco(slot)
                    | scope::DesignScopePayloadMut::Boceto(slot)
                    | scope::DesignScopePayloadMut::SketchJa(slot) = scope.payload_mut()
                    {
                        let entity_id = entity.entity_id.clone();
                        *slot = Some(scope::DesignSketchEntityBinding {
                            entity_id,
                            entity_reference_offset,
                        });
                    }
                }
            }
            if matches!(
                scope.kind(),
                scope::DesignFeatureKind::WorkPlane | scope::DesignFeatureKind::PlanoDeObra
            ) {
                if let Some(frame) = exact_work_plane_frame(bytes, &records, &scope) {
                    if let scope::DesignScopePayloadMut::WorkPlane(slot)
                    | scope::DesignScopePayloadMut::PlanoDeObra(slot) = scope.payload_mut()
                    {
                        *slot = Some(scope::DesignWorkPlaneTransform {
                            work_plane_transform: frame.transform,
                            work_plane_transform_offset: frame.transform_offset,
                            reference: frame.reference.map(|(record_index, offset)| {
                                scope::DesignWorkPlaneReference {
                                    work_plane_reference: record_index,
                                    work_plane_reference_offset: offset,
                                }
                            }),
                            work_plane_construction: None,
                        });
                    }
                }
            }
            if let Some(construction) = exact_work_axis_construction(bytes, &records, &scope) {
                if let scope::DesignScopePayloadMut::WorkAxis(slot) = scope.payload_mut() {
                    *slot = Some(construction);
                }
            }
            if scope.kind() == scope::DesignFeatureKind::JointOrigin {
                if let Some(frame) = exact_joint_origin_frame(bytes, &records, &scope) {
                    if let scope::DesignScopePayloadMut::JointOrigin(slot) = scope.payload_mut() {
                        *slot = Some(scope::DesignJointOriginTransform {
                            joint_origin_transform: frame.transform,
                            joint_origin_transform_offset: frame.transform_offset,
                            reference: frame.reference.map(|(record_index, offset)| {
                                scope::DesignJointOriginReference {
                                    joint_origin_reference: record_index,
                                    joint_origin_reference_offset: offset,
                                }
                            }),
                        });
                    }
                }
            }
            {
                let construction =
                    exact_work_point_construction(bytes, &records, &scope, &stream_types);
                if let scope::DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_hole_construction(
                    bytes,
                    &records,
                    &scope,
                    &stream_types,
                    &scope::DesignFeatureKind::Hole,
                )?;
                let extent = super::hole::hole_extent(
                    bytes,
                    &scope,
                    scope
                        .class_tag
                        .as_str()
                        .parse::<u32>()
                        .ok()
                        .and_then(|tag| class_types.get(&tag))
                        .map(|row| row.version),
                );
                if let scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
                    slot.point = construction;
                    slot.extent = extent;
                }
            }
            if let Some(placement) = exact_coil_placement(ctx, bytes, &records, &scope, recipes)? {
                if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
                | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
                {
                    slot.get_or_insert_with(Default::default).placement = Some(placement);
                }
            }
            if let Some(construction) =
                exact_solid_primitive(bytes, &records, &scope, parameter_owners)
            {
                match (scope.payload_mut(), construction) {
                    (
                        scope::DesignScopePayloadMut::SpherePrimitive(slot)
                        | scope::DesignScopePayloadMut::EsferaPrimitiva(slot),
                        crate::records::feature::primitives::DesignSolidPrimitive::Sphere(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::TorusPrimitive(slot),
                        crate::records::feature::primitives::DesignSolidPrimitive::Torus(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::BoxPrimitive(slot),
                        crate::records::feature::primitives::DesignSolidPrimitive::Box(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::CylinderPrimitive(slot),
                        crate::records::feature::primitives::DesignSolidPrimitive::Cylinder(value),
                    ) => *slot = Some(value),
                    _ => {
                        return Err(CodecError::NotImplemented(
                            "F3D solid primitive payload kind mismatch".into(),
                        ))
                    }
                }
            }
            {
                let construction = exact_direct_face_operation(bytes, &records, &scope);
                match (scope.payload_mut(), construction) {
                    (
                        scope::DesignScopePayloadMut::OffsetFaces(slot)
                        | scope::DesignScopePayloadMut::DecalerLesFaces(slot),
                        Some(direct_face::DesignDirectFaceOperation::OffsetFaces(value)),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Shell(slot)
                        | scope::DesignScopePayloadMut::Schale(slot)
                        | scope::DesignScopePayloadMut::Vaciado(slot),
                        Some(direct_face::DesignDirectFaceOperation::Shell(value)),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Thicken(slot),
                        Some(direct_face::DesignDirectFaceOperation::Thicken(value)),
                    ) => *slot = Some(value),
                    _ => {}
                }
            }
            if scope.kind() == scope::DesignFeatureKind::Align {
                let mut envelope = super::align::decode_align_envelope(bytes, &scope);
                if let Some(envelope) = envelope.as_mut() {
                    envelope.construction = super::align::construction::decode_align_construction(
                        bytes, &records, &scope, envelope,
                    );
                }
                if let scope::DesignScopePayloadMut::Align(slot) = scope.payload_mut() {
                    *slot = envelope;
                }
            }
            {
                let construction = exact_move_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::Move(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_move_face_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::MoveFace(slot)
                | scope::DesignScopePayloadMut::DeplacerFace(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_scale_operation(bytes, &records, &scope, &stream_types)?;
                if let scope::DesignScopePayloadMut::Scale(slot)
                | scope::DesignScopePayloadMut::Massstab(slot)
                | scope::DesignScopePayloadMut::Escala(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_surface_extend_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::SurfaceExtend(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_surface_offset_operation(bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::SurfaceOffset(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            if let Some(parameters) = exact_fixed_extrude_parameters(
                bytes,
                &records,
                &scope,
                parameters,
                parameter_owners,
            ) {
                {
                    let value = Some(parameters);
                    if let scope::DesignScopePayloadMut::Extrude(slot)
                    | scope::DesignScopePayloadMut::Extrusion(slot)
                    | scope::DesignScopePayloadMut::Extrusao(slot)
                    | scope::DesignScopePayloadMut::Extruir(slot)
                    | scope::DesignScopePayloadMut::ExtrudeJa(slot) = scope.payload_mut()
                    {
                        slot.get_or_insert_with(Default::default)
                            .fixed_extrude_parameters = value;
                    }
                }
            }
            {
                let construction = exact_fixed_fillet_parameters(
                    bytes,
                    &records,
                    &scope,
                    parameters,
                    parameter_owners,
                );
                if let scope::DesignScopePayloadMut::Fillet(slot)
                | scope::DesignScopePayloadMut::Conge(slot)
                | scope::DesignScopePayloadMut::Abrundung(slot)
                | scope::DesignScopePayloadMut::Arredondamento(slot)
                | scope::DesignScopePayloadMut::Empalme(slot)
                | scope::DesignScopePayloadMut::FilletJa(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_fixed_chamfer_parameters(bytes, &records, &scope, parameter_owners);
                if let scope::DesignScopePayloadMut::Chamfer(slot)
                | scope::DesignScopePayloadMut::Chanfrein(slot)
                | scope::DesignScopePayloadMut::Chaflan(slot)
                | scope::DesignScopePayloadMut::ChamferJa(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            if let Some(construction) =
                exact_path_feature_construction(bytes, &records, &scope, parameter_owners)
            {
                match (scope.payload_mut(), construction) {
                    (
                        scope::DesignScopePayloadMut::Revolve(slot),
                        crate::records::feature::path_features::DesignPathFeatureConstruction::Revolve(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Loft(slot)
                        | scope::DesignScopePayloadMut::Solevacion(slot),
                        crate::records::feature::path_features::DesignPathFeatureConstruction::Loft(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Sweep(slot),
                        crate::records::feature::path_features::DesignPathFeatureConstruction::Sweep(value),
                    ) => *slot = Some(scope::DesignSweepScope {
                        construction: Some(value),
                        sweep_profile: None,
                    }),
                    _ => return Err(CodecError::NotImplemented("F3D path feature payload kind mismatch".into())),
                }
            }
            {
                let construction = exact_combine_operation(bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_thread_construction(bytes, &scope, parameter_owners);
                if let scope::DesignScopePayloadMut::Thread(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_draft_operation_with_owners(bytes, &records, &scope, parameter_owners);
                if let scope::DesignScopePayloadMut::Draft(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_circular_pattern_construction_with_owners(
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                )?;
                if let scope::DesignScopePayloadMut::CPattern(slot)
                | scope::DesignScopePayloadMut::CircularPattern(slot)
                | scope::DesignScopePayloadMut::ReseauC(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_rectangular_pattern_construction(
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                )?;
                if let scope::DesignScopePayloadMut::RPattern(slot)
                | scope::DesignScopePayloadMut::RectangularPattern(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_assembly_alignment_with_types(
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                    &stream_types,
                )?;
                if let scope::DesignScopePayloadMut::Assemble(slot)
                | scope::DesignScopePayloadMut::AsBuilt(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            let legacy_form = scope
                .assembly_alignment()
                .and_then(|alignment| {
                    let assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                        solved_frame,
                        limits,
                    } = alignment.form.as_ref()?
                    else {
                        return None;
                    };
                    let carriers = match exact_legacy_as_built_421_operands(
                        ctx,
                        bytes,
                        &records,
                        &scope,
                        &stream_types,
                        recipes,
                        solved_frame,
                    ) {
                        Ok(Some(carriers)) => carriers,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    Some(Ok(
                        assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                            carriers,
                            solved_frame: solved_frame.clone(),
                            limits: limits.clone(),
                            frames_field_present: true,
                        },
                    ))
                })
                .transpose()?;
            if let (Some(alignment), Some(form)) = (scope.assembly_alignment_mut(), legacy_form) {
                alignment.form = Some(form);
            }
            {
                let construction = exact_component_insert_construction(bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::ComponentInsert(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_derived_instance_construction(
                    bytes,
                    &records,
                    &scope,
                    component_occurrences,
                )?;
                if let scope::DesignScopePayloadMut::DerivedInstance(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_copy_paste_component_operation(
                    bytes,
                    &records,
                    &scope,
                    component_occurrences,
                )?;
                if let scope::DesignScopePayloadMut::CopyPaste(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            scope.occurrence_transfer = super::occurrence_transfer::exact_occurrence_transfer(
                bytes,
                &records,
                &scope,
                &stream_types,
            );
            bind_component_pattern_occurrences(&mut scope, component_occurrences)?;
            {
                let construction = exact_copy_paste_bodies_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::CopyPasteBodies(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let owned = body_owners
                    .get(&scope.record_index)
                    .and_then(|owner| exact_timeline_owned_base_feature(bytes, &scope, *owner));
                let construction = match owned {
                    Some(owned) => Some(owned),
                    None => exact_base_feature_construction(bytes, &scope)?,
                };
                if let scope::DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }

            out.push(scope);
        }
        bind_joint_origin_frames_from_assemblies(bytes, &mut out[stream_scope_start..]);
        bind_axial_assembly_operand_targets(bytes, &records, &mut out[stream_scope_start..])?;
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    for scope in out
        .iter()
        .filter(|scope| scope.kind() == scope::DesignFeatureKind::Align)
    {
        losses.push(super::align::align_native_loss(
            scope,
            scope.align_envelope(),
        ));
    }
    Ok(DecodedParameterScopes {
        scopes: out,
        losses,
    })
}

fn first_marked_reference_offsets(frame: &[u8]) -> HashMap<u32, usize> {
    let mut first_at = HashMap::new();
    for at in memchr::memchr_iter(1, frame) {
        if at + 11 <= frame.len() && frame[at + 5..at + 11] == [0; 6] {
            if let Some(suffix) = View::u32_le_at(frame, at + 1) {
                first_at.entry(suffix).or_insert(at);
            }
        }
    }
    first_at
}

/// Byte offsets of the primary record frames that each Design stream's
/// `MetaStream` record index names, keyed by native stream scope.
pub(crate) fn design_primary_frame_offsets(
    scan: &crate::container::ContainerScan,
) -> Result<std::collections::HashSet<(String, u64)>, CodecError> {
    let mut out = std::collections::HashSet::new();
    for entry in scan.entries.iter().filter(|entry| {
        scan.is_design_stream(entry, cadmpeg_core::container::ContainerRole::Bulkstream)
    }) {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(meta) = crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let scope = crate::ids::native_scope(&entry.name);
        for frame in crate::metastream::primary_record_frames(&meta, bytes.len())? {
            out.insert((scope.clone(), u64_from_index(frame.start)));
        }
    }
    Ok(out)
}

/// Admit one envelope for every logical scope identity.
///
/// Some Design streams retain more than one complete envelope for one record
/// index. An envelope is history-bound when it resolves to one ASM state pair
/// through the history header links. Binding output is not read, so selection
/// does not depend on whether history binding ran. Exactly one history-bound
/// envelope is authoritative. An unresolved group remains an error.
pub(crate) fn admit_history_bound_scope_variants(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut Vec<DesignParameterScope>,
    histories: &[crate::history_records::AsmHistory],
    primary_frames: &std::collections::HashSet<(String, u64)>,
) -> Result<usize, CodecError> {
    let mut admitted = vec![true; scopes.len()];
    let mut superseded = 0usize;
    let mut groups = HashMap::<(&str, u32), Vec<usize>>::new();
    for (index, scope) in scopes.iter().enumerate() {
        let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
        let key = (stream, scope.record_index);
        groups.entry(key).or_default().push(index);
    }

    for indices in groups.values() {
        let [first, following @ ..] = indices.as_slice() else {
            continue;
        };
        if following.is_empty() {
            continue;
        }
        let mut history_bound = None;
        let mut multiple_history_bounds = false;
        for index in indices {
            let scope = &scopes[*index];
            let Some(state_id) = scope.history_state_id() else {
                continue;
            };
            let Some(previous_state_id) =
                crate::history::header_scope_previous_history_state_id(scope, histories)
            else {
                continue;
            };
            if crate::history::unique_header_history_state_pair(
                histories,
                state_id,
                previous_state_id,
            )
            .is_some()
                && history_bound.replace(*index).is_some()
            {
                multiple_history_bounds = true;
                break;
            }
        }
        let mut equivalent_payload = history_bound.is_none() && !multiple_history_bounds;
        if equivalent_payload {
            for index in following {
                if !equivalent_scope_variant_payload(ctx, &scopes[*first], &scopes[*index])? {
                    equivalent_payload = false;
                    break;
                }
            }
        }
        // Without a history binding, the envelope that the stream's primary
        // record index names is the live one when it is unique in the group.
        let mut primary = indices.iter().copied().filter(|index| {
            let scope = &scopes[*index];
            native_stream(&scope.id).is_some_and(|stream| {
                primary_frames.contains(&(stream.to_owned(), scope.byte_offset()))
            })
        });
        let primary_indexed = match (primary.next(), primary.next()) {
            (Some(index), None) => Some(index),
            _ => None,
        };
        let keep = match (history_bound, multiple_history_bounds, primary_indexed) {
            (Some(keep), false, _) => keep,
            (None, false, _) if equivalent_payload => {
                following.iter().copied().fold(*first, |keep, index| {
                    if scopes[index].byte_offset() >= scopes[keep].byte_offset() {
                        index
                    } else {
                        keep
                    }
                })
            }
            // Several envelopes resolve to ASM transitions: the one the primary
            // record index names is live; the others are retained revisions.
            (Some(_), true, Some(keep)) | (None, false, Some(keep)) => {
                // The other envelopes are not equivalent serializations, so
                // dropping them loses their payload.
                superseded = superseded.checked_add(following.len()).ok_or_else(|| {
                    CodecError::Malformed("Design scope envelope count overflows".into())
                })?;
                keep
            }
            _ => {
                return Err(CodecError::Malformed(
                    "Design scope record identity has unresolved duplicate envelopes".into(),
                ));
            }
        };
        for index in indices {
            admitted[*index] = *index == keep;
        }
    }

    drop(groups);

    let mut retained = Vec::new();
    for (index, scope) in std::mem::take(scopes).into_iter().enumerate() {
        if admitted[index] {
            retained.push(scope);
        }
    }
    *scopes = retained;
    Ok(superseded)
}

/// Compare two same-index scope envelopes after removing source-location and
/// dynamic-class fields. An equivalent envelope is one serialization of the
/// same logical scope; the later envelope supersedes the earlier one when no
/// decoded ASM state pair can select a revision.
fn equivalent_scope_variant_payload(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    left: &DesignParameterScope,
    right: &DesignParameterScope,
) -> Result<bool, CodecError> {
    let Some(mut left) = scope_variant_json(ctx, left)? else {
        return Ok(false);
    };
    let Some(mut right) = scope_variant_json(ctx, right)? else {
        return Ok(false);
    };
    strip_scope_variant_provenance(ctx, &mut left, true)?;
    strip_scope_variant_provenance(ctx, &mut right, true)?;
    Ok(left == right)
}

fn scope_variant_json(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &DesignParameterScope,
) -> Result<Option<serde_json::Value>, CodecError> {
    use serde::de::DeserializeSeed;

    let Ok(bytes) = serde_json::to_vec(scope) else {
        return Ok(None);
    };
    let mut refusal = None;
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let parsed = crate::design::json_value::ValueSeed {
        ctx,
        refusal: &mut refusal,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| {
        deserializer.end()?;
        Ok(value)
    });
    match parsed {
        Ok(value) => Ok(Some(value)),
        Err(_) => match refusal {
            Some(error) => Err(error),
            None => Ok(None),
        },
    }
}

fn strip_scope_variant_provenance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut serde_json::Value,
    top_level: bool,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("f3d scope variant provenance depth")?;
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                strip_scope_variant_provenance(ctx, item, false)?;
            }
        }
        serde_json::Value::Object(fields) => {
            fields.retain(|key, _| {
                if key.ends_with("_offset") || key.ends_with("_offsets") {
                    return false;
                }
                if top_level
                    && matches!(
                        key.as_str(),
                        "id" | "class_tag"
                            | "source_kind"
                            | "frame_length"
                            | "history_state_id"
                            | "previous_history_state_id"
                            | "paired_class_tag"
                    )
                {
                    return false;
                }
                true
            });
            for field in fields.values_mut() {
                strip_scope_variant_provenance(ctx, field, false)?;
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
    Ok(())
}

/// Skip the payload prologue at `at`: a leading-block presence byte, a property
/// presence byte, and the property block that byte gates. The leading-block
/// byte belongs to classes that write one, so this reader only steps over it.
pub(in crate::design::decode) fn payload_prologue(
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Option<usize> {
    read_payload_prologue(bytes, at, end).map(|prologue| prologue.end)
}

pub(super) struct PayloadPrologue {
    pub(super) end: usize,
    pub(super) property_count: u32,
    pub(super) saved_hole_depth:
        Option<crate::records::identity::Located<cadmpeg_ir::scalar::FiniteReal>>,
}

pub(super) fn read_payload_prologue(
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Option<PayloadPrologue> {
    fn graphic_ascii_at(bytes: &[u8], at: usize) -> Option<(&[u8], usize)> {
        let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        if !(1..=64).contains(&length) {
            return None;
        }
        let start = at.checked_add(4)?;
        let end = start.checked_add(length)?;
        let raw = bytes.get(start..end)?;
        raw.iter().all(u8::is_ascii_graphic).then_some((raw, end))
    }
    let bytes = bytes.get(..end)?;
    let mut cursor = at.checked_add(1)?;
    let present = *bytes.get(cursor)?;
    cursor = cursor.checked_add(1)?;
    let count = match present {
        0 => 0,
        1 => {
            let count = View::u32_le_at(bytes, cursor)?;
            cursor = cursor.checked_add(4)?;
            count
        }
        _ => return None,
    };
    if count > 16 {
        return None;
    }
    let mut saved_hole_depth = None;
    for _ in 0..count {
        let (key, after_key) = graphic_ascii_at(bytes, cursor)?;
        let (type_name, after_type) = graphic_ascii_at(bytes, after_key)?;
        match type_name {
            b"IntrinsicMetaTypeuint64" => {
                View::u64_le_at(bytes, after_type)?;
            }
            b"IntrinsicMetaTypedouble" if key == b"SavedHoleDepthInNonDistanceExtentsCase" => {
                let value =
                    cadmpeg_ir::scalar::FiniteReal::try_from(View::f64_le_at(bytes, after_type)?)
                        .ok()?;
                if saved_hole_depth
                    .replace(crate::records::identity::Located {
                        value,
                        offset: u64::try_from(after_type).ok()?,
                    })
                    .is_some()
                {
                    return None;
                }
            }
            _ => return None,
        }
        cursor = after_type.checked_add(8)?;
    }
    (cursor <= end).then_some(PayloadPrologue {
        end: cursor,
        property_count: count,
        saved_hole_depth,
    })
}

/// Every indexed-record header that can open a parameter scope: a scope is
/// delimited by two headers carrying its record index, so the last header of an
/// index opens nothing.
pub(super) fn parameter_scope_candidate_headers(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
) -> Result<Vec<RecordFrame>, CodecError> {
    let mut headers = Vec::new();
    for (record_index, offsets) in records.records() {
        for at in offsets.windows(2).map(|pair| &pair[0]) {
            let Some((class_tag, _)) =
                lp_ascii_filtered_view(bytes, *at, 3..=3, u8::is_ascii_digit)
            else {
                continue;
            };
            let Ok(class_tag) = crate::design::decode::text::class_tag_from_view(class_tag) else {
                continue;
            };
            let byte_offset = u64::try_from(*at)
                .map_err(|_| CodecError::malformed("f3d Design scope header offset exceeds u64"))?;

            headers.push(RecordFrame {
                record_index,
                class_tag,
                byte_offset,
            });
        }
    }
    Ok(headers)
}

/// Whether a reference table counted at `count_at`, holding `count` entries of
/// `stride` bytes, ends exactly at `end`. The scan reads a count at every
/// offset, so most counts are arbitrary bytes: one whose table would not fit
/// the address space (on a 32-bit target `count * stride` overflows for any
/// large value) is not this table, and must not end the whole scope parse.
pub(super) fn reference_table_ends_at(
    count_at: usize,
    count: usize,
    stride: usize,
    end: usize,
) -> bool {
    count != 0
        && count
            .checked_mul(stride)
            .and_then(|length| count_at.checked_add(4)?.checked_add(length))
            == Some(end)
}

pub(crate) fn parameter_scope_tail_length_is_valid(
    kind: impl AsRef<str>,
    tail_length: usize,
) -> bool {
    let kind = kind.as_ref();
    if (80..=590).contains(&tail_length) && tail_length.is_multiple_of(2) {
        return true;
    }
    match kind {
        "CopyPasteBodies" => tail_length == 110,
        "Hole" => matches!(tail_length, 72 | 76 | 77 | 78 | 87 | 109),
        "CoilPrimitive" => matches!(tail_length, 72 | 76 | 77 | 78 | 87 | 88),
        _ => matches!(tail_length, 72 | 76 | 77 | 78 | 87),
    }
}

/// Tail byte that begins the second marked reference of an 87-byte fixed tail.
const FIXED_TAIL_SECOND_REFERENCE: usize = 30;

pub(crate) fn parameter_scope_previous_history_offset(
    kind: impl AsRef<str>,
    tail_length: usize,
    rigid_group: bool,
    second_tail_reference: bool,
) -> Option<usize> {
    parameter_scope_previous_history_offset_for_form(
        kind.as_ref(),
        tail_length,
        ScopeTailForm::Fixed,
        rigid_group,
        second_tail_reference,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeTailForm {
    Fixed,
    Named,
    /// The 76-byte fixed tail with its three references in the typed spelling.
    TypedFixed,
}

fn unparsed_rigid_group_tail_length(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    byte_offset: u64,
) -> Option<usize> {
    use crate::layout::assembly_alternate_rigid_group_prefix as alternate;
    use crate::layout::assembly_rigid_as_built_scope as ordinary;
    let start = usize::try_from(byte_offset).ok()?;
    let paired =
        records.first_at_or_after(start.checked_add(ordinary::ZERO_RUN_8)?, record_index)?;
    let stride = ordinary::SECOND_LOCATOR_REFERENCE - ordinary::FIRST_LOCATOR_REFERENCE;
    let known_tail = [
        crate::layout::ordinary_rigid_group_tail_76::LEN,
        crate::layout::ordinary_rigid_group_tail_77::LEN,
        crate::layout::ordinary_rigid_group_tail_87::LEN,
        crate::layout::ordinary_rigid_group_tail_88::LEN,
        crate::layout::assembly_typed_rigid_group_scope::LEN
            - crate::layout::assembly_typed_rigid_group_scope::SCOPE_TAIL,
    ]
    .into_iter()
    .find_map(|tail_length| {
        let kind_end = paired.checked_sub(tail_length)?;
        let kind_at = kind_end.checked_sub(ordinary::SCOPE_TAIL - ordinary::KIND)?;
        let table = kind_at.checked_sub(ordinary::KIND - ordinary::REFERENCE_COUNT)?;
        if table < start.checked_add(ordinary::ZERO_RUN_8)? || View::u32_le_at(bytes, table)? != 0 {
            return None;
        }
        let (kind, end) = lp_utf16_bounded(bytes.get(..paired)?, kind_at, 8..=8)?;
        if kind != "As-built" || end != kind_end {
            return None;
        }
        let guid_at = kind_at.checked_sub(ordinary::KIND - ordinary::JOINT_REFERENCE - stride)?;
        let guid_end = crate::design::decode::text::fixed_guid_end(bytes, guid_at)?;
        let padding = ordinary::REFERENCE_COUNT - ordinary::ZERO_RUN_150;
        (guid_end.checked_add(padding)? == table
            && bytes.get(guid_end..table)?.iter().all(|byte| *byte == 0))
        .then_some(tail_length)
    });
    if known_tail.is_some() {
        return known_tail;
    }
    let prologue_shift = if crate::bytes::local_reference_target(
        bytes,
        start.checked_add(ordinary::PATHLESS_LOCATOR_REFERENCE)?,
        false,
    )
    .is_some()
    {
        0
    } else if crate::bytes::local_reference_target(
        bytes,
        start.checked_add(alternate::PATHLESS_LOCATOR_REFERENCE)?,
        false,
    )
    .is_some()
    {
        alternate::PATHLESS_LOCATOR_REFERENCE - ordinary::PATHLESS_LOCATOR_REFERENCE
    } else {
        return None;
    };
    let first = start
        .checked_add(ordinary::FIRST_LOCATOR_REFERENCE)?
        .checked_add(prologue_shift)?;
    let members = cadmpeg_core::decode::bounded_len(
        u64::from(View::u32_le_at(
            bytes,
            start
                .checked_add(ordinary::LOCATOR_COUNT)?
                .checked_add(prologue_shift)?,
        )?),
        stride,
        paired.checked_sub(first)?,
    )?;
    let shift = members
        .checked_sub(2)?
        .checked_mul(stride)?
        .checked_add(prologue_shift)?;
    let kind_at = start.checked_add(ordinary::KIND)?.checked_add(shift)?;
    let (kind, kind_end) = lp_utf16_bounded(bytes.get(..paired)?, kind_at, 8..=8)?;
    if kind != "As-built" || View::u32_le_at(bytes, kind_end)? == 0 {
        return None;
    }
    let target =
        |at| crate::bytes::local_reference_target(bytes, at, false).is_some_and(|value| value != 0);
    let table = start
        .checked_add(ordinary::REFERENCE_COUNT)?
        .checked_add(shift)?;
    let joint = start
        .checked_add(ordinary::JOINT_REFERENCE)?
        .checked_add(shift)?;
    if !target(
        start
            .checked_add(ordinary::PATHLESS_LOCATOR_REFERENCE)?
            .checked_add(prologue_shift)?,
    ) || bytes
        .get(
            start
                .checked_add(ordinary::ZERO_RUN_33)?
                .checked_add(prologue_shift)?
                ..start
                    .checked_add(ordinary::LOCATOR_COUNT)?
                    .checked_add(prologue_shift)?,
        )?
        .iter()
        .any(|byte| *byte != 0)
        || (0..members).any(|member| !target(first + member * stride))
        || !target(joint)
        || crate::design::decode::text::fixed_guid_end(bytes, joint.checked_add(stride)?)?
            .checked_add(ordinary::REFERENCE_COUNT - ordinary::ZERO_RUN_150)?
            != table
        || bytes
            .get(
                start
                    .checked_add(ordinary::ZERO_RUN_150)?
                    .checked_add(shift)?..table,
            )?
            .iter()
            .any(|byte| *byte != 0)
        || View::u32_le_at(bytes, table)? != 0
    {
        return None;
    }
    paired.checked_sub(kind_end)
}

/// Whether `bytes[kind_end..paired_at]` is the typed-reference 196-byte tail:
/// u32 ordinal, fifteen zero bytes, a typed reference, the preceding
/// history-state identity, u32 + f64 + u32, a typed reference, two zero
/// bytes, a typed reference, and two zero bytes.
fn typed_fixed_scope_tail_is_valid(bytes: &[u8], kind_end: usize, paired_at: usize) -> bool {
    use crate::bytes::typed_local_reference_target as target;
    (|| {
        let tail = bytes.get(kind_end..paired_at)?;
        (tail.len() == typed_tail::LEN
            && tail.get(typed_tail::ZERO_RUN_15..typed_tail::FIRST_REFERENCE)? == [0; 15]
            && target(tail, typed_tail::FIRST_REFERENCE).is_some()
            && target(tail, typed_tail::SECOND_REFERENCE).is_some()
            && tail.get(typed_tail::SECOND_ZERO_RUN_2..typed_tail::THIRD_REFERENCE)? == [0; 2]
            && target(tail, typed_tail::THIRD_REFERENCE).is_some()
            && tail.get(typed_tail::THIRD_ZERO_RUN_2..typed_tail::LEN)? == [0; 2])
            .then_some(())
    })()
    .is_some()
}

fn parameter_scope_previous_history_offset_for_form(
    kind: &str,
    tail_length: usize,
    tail_form: ScopeTailForm,
    rigid_group: bool,
    second_tail_reference: bool,
) -> Option<usize> {
    match tail_form {
        ScopeTailForm::Named => return None,
        ScopeTailForm::TypedFixed => return Some(typed_tail::PREVIOUS_HISTORY_STATE_ID),
        ScopeTailForm::Fixed => {}
    }
    if rigid_group && kind == "As-built" {
        return crate::design::assembly::OrdinaryRigidGroupTail::for_length(tail_length)
            .map(|tail| tail.previous_history);
    }
    match (kind, tail_length) {
        ("As-built", crate::layout::ordinary_rigid_group_tail_88::LEN) => {
            Some(crate::layout::ordinary_rigid_group_tail_88::PREVIOUS_HISTORY_STATE_ID)
        }
        ("CopyPasteBodies", 110) | ("Hole", 109) => Some(53),
        ("CoilPrimitive", 88) => None,
        (_, 72 | 76) => Some(30),
        (_, 77 | 78) => Some(31),
        (_, 87) if second_tail_reference => Some(41),
        (_, 87) => Some(31),
        _ => None,
    }
}

#[cfg(test)]
pub(in crate::design::decode) fn parse_parameter_scope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &crate::records::references::DesignClassTag,
    byte_offset: u64,
) -> Result<Option<DesignParameterScope>, CodecError> {
    parse_parameter_scope_with_type_guid(bytes, records, record_index, class_tag, byte_offset, None)
}

pub(in crate::design::decode) fn parse_parameter_scope_with_type_guid(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &crate::records::references::DesignClassTag,
    byte_offset: u64,
    type_guid: Option<&str>,
) -> Result<Option<DesignParameterScope>, CodecError> {
    (|| {
        let type_family = type_guid.and_then(family::from_type_guid);
        let start = usize::try_from(byte_offset).ok()?;
        let paired_at = records.first_at_or_after(start.checked_add(11)?, record_index)?;
        let (paired_class_tag, _) =
            lp_ascii_filtered_view(bytes, paired_at, 3..=3, u8::is_ascii_digit)?;
        let mut fixed_candidate = None;
        let mut fixed_ambiguous = false;
        let mut named_candidate = None;
        let mut named_ambiguous = false;
        let kind_scan_end = paired_at.checked_sub(72)?;
        for at in (start.checked_add(11)?..paired_at)
            .rev()
            .take(590 + 4 + 2 * 256)
            .rev()
            .take_while(|at| *at < kind_scan_end)
        {
            let Some((kind, kind_end)) = lp_utf16_bounded(bytes, at, 1..=256) else {
                continue;
            };
            if !kind.chars().all(|character| !character.is_control()) {
                continue;
            }
            let selected = scope_feature_kind(&kind, type_family.as_ref());
            let kind = selected
                .as_ref()
                .map_or(kind.as_str(), scope::DesignFeatureKind::as_str);
            let Some(tail_length) = paired_at.checked_sub(kind_end) else {
                continue;
            };
            let fixed_tail = matches!(tail_length, 72 | 76 | 77 | 78 | 82 | 87 | 88 | 104 | 110)
                || (kind == "Canvas"
                    && (exact_canvas_scope_tail(bytes.get(kind_end..paired_at)?, record_index)
                        || exact_typed_canvas_scope_tail(bytes.get(kind_end..paired_at)?)));
            let fixed_tail = fixed_tail
                || (kind == "Hole"
                    && exact_thread_bound_hole_tail(bytes.get(kind_end..paired_at)?, record_index));
            if fixed_tail
                && parameter_scope_tail_length_is_valid(kind, tail_length)
                && fixed_candidate
                    .replace((at, kind_end, tail_length, ScopeTailForm::Fixed))
                    .is_some()
            {
                fixed_ambiguous = true;
            }
            if kind != "Canvas"
                && tail_length == typed_tail::LEN
                && typed_fixed_scope_tail_is_valid(bytes, kind_end, paired_at)
                && fixed_candidate
                    .replace((at, kind_end, tail_length, ScopeTailForm::TypedFixed))
                    .is_some()
            {
                fixed_ambiguous = true;
            }
            let named_tail_possible = (78..=590).contains(&tail_length)
                && tail_length.is_multiple_of(2)
                && (parameter_scope_tail_length_is_valid(kind, tail_length) || tail_length == 78);
            let named_tail = if named_tail_possible {
                named_parameter_scope_tail_is_valid(bytes, kind_end, paired_at, tail_length)
                    .unwrap_or_default()
            } else {
                false
            };
            if named_tail
                && named_candidate
                    .replace((at, kind_end, tail_length, ScopeTailForm::Named))
                    .is_some()
            {
                named_ambiguous = true;
            }
        }
        let candidate = if named_ambiguous {
            None
        } else if named_candidate.is_some() {
            named_candidate
        } else if fixed_ambiguous {
            None
        } else {
            fixed_candidate
        };
        let (kind_at, kind_end, tail_length, tail_form) = candidate?;
        let (kind_text, confirmed_kind_end) = lp_utf16_bounded(bytes, kind_at, 1..=256)?;
        if confirmed_kind_end != kind_end {
            return None;
        }
        let kind = scope_feature_kind(&kind_text, type_family.as_ref())?;
        let source_kind = (kind_text != kind.as_str()).then_some(kind_text);
        let reference_table_end = kind_at.checked_sub(4)?;
        let feature_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, kind_end)?)?;
        let history_state_id_offset = reference_table_end;
        let history_state_id = match View::u32_le_at(bytes, history_state_id_offset)? {
            u32::MAX => None,
            state_id => Some(i64::from(state_id)),
        };
        // Every reference in one scope frame uses one spelling; the typed
        // tail selects the 51-byte typed reference-table entries.
        let typed_canvas =
            kind == scope::DesignFeatureKind::Canvas && tail_length == typed_canvas_tail::LEN;
        let typed_references = tail_form == ScopeTailForm::TypedFixed || typed_canvas;
        let stride = if typed_references {
            crate::bytes::TYPED_LOCAL_REFERENCE_LEN
        } else {
            11
        };
        let mut reference_table = None;
        for count_at in start + 11..reference_table_end {
            let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
            if !reference_table_ends_at(count_at, count, stride, reference_table_end) {
                continue;
            }
            let first = count_at.checked_add(4)?;

            let mut members = Vec::new();

            let mut offsets = Vec::new();
            for ordinal in 0..count {
                let marker = first.checked_add(ordinal.checked_mul(stride)?)?;
                let member = if typed_canvas {
                    if count == 2 {
                        let guid = if ordinal == 0 {
                            "5A1BF548-241F-46FD-9FB5-E4B05126EB9D"
                        } else {
                            "7B6C3C2D-4096-4E6D-B225-699E8C2D9355"
                        };
                        typed_canvas_reference(bytes, marker, guid)
                    } else {
                        None
                    }
                } else if typed_references {
                    crate::bytes::typed_local_reference_target(bytes, marker)
                } else if bytes.get(marker) == Some(&1)
                    && bytes.get(marker + 5..marker + 11)? == [0; 6]
                {
                    View::u32_le_at(bytes, marker + 1)
                } else {
                    None
                };
                let Some(member) = member else {
                    members.clear();
                    break;
                };
                members.push(member);
                offsets.push(u64::try_from(marker + 1).ok()?);
            }
            if members.len() == count
                && reference_table
                    .replace((count_at, members, offsets))
                    .is_some()
            {
                return None;
            }
        }
        if reference_table.is_none() && kind == scope::DesignFeatureKind::AsBuilt {
            use crate::layout::assembly_rigid_as_built_scope as ordinary;
            let guid_padding = ordinary::REFERENCE_COUNT - ordinary::ZERO_RUN_150;
            let frame_length = u64::try_from(paired_at.checked_sub(start)?).ok()?;
            let layout = crate::design::assembly::RigidGroupLayout::new(
                frame_length,
                u64::try_from(tail_length).ok()?,
            )?;
            let typed = layout.stride == crate::bytes::TYPED_LOCAL_REFERENCE_LEN;
            if !typed
                && crate::design::assembly::OrdinaryRigidGroupTail::read(
                    bytes.get(kind_end..paired_at)?,
                )
                .is_none()
            {
                return None;
            }
            let target = |at| {
                crate::bytes::local_reference_target(bytes, at, typed).filter(|target| *target != 0)
            };
            let pathless_at = start.checked_add(layout.pathless_reference)?;
            let count_at = start.checked_add(layout.empty_table)?;
            let first = start.checked_add(layout.first_reference)?;
            let members_end = first.checked_add(layout.members.checked_mul(layout.stride)?)?;
            if !layout.prologue_matches(View::over_retained(bytes.get(start..paired_at)?))
                || target(pathless_at).is_none()
                || bytes.get(
                    pathless_at.checked_add(layout.stride)?..start.checked_add(layout.count_at)?,
                )? != [0; 4]
                || usize::try_from(View::u32_le_at(bytes, start.checked_add(layout.count_at)?)?)
                    .ok()?
                    != layout.members
                || (0..layout.members)
                    .any(|member| target(first + member * layout.stride).is_none())
                || target(members_end).is_none()
                || crate::design::decode::text::fixed_guid_end(
                    bytes,
                    members_end.checked_add(layout.stride)?,
                )?
                .checked_add(guid_padding)?
                    != count_at
                || bytes
                    .get(count_at.checked_sub(guid_padding)?..count_at)?
                    .iter()
                    .any(|byte| *byte != 0)
                || count_at.checked_add(4)? != reference_table_end
                || View::u32_le_at(bytes, count_at)? != 0
            {
                return None;
            }
            reference_table = Some((count_at, Vec::new(), Vec::new()));
        }
        let reference_table = reference_table?;
        let (reference_count_at, reference_members, reference_member_offsets) = &reference_table;
        let previous_history_state_id_offset =
            match parameter_scope_previous_history_offset_for_form(
                kind.as_str(),
                tail_length,
                tail_form,
                kind == scope::DesignFeatureKind::AsBuilt && reference_members.is_empty(),
                bytes.get(kind_end.checked_add(FIXED_TAIL_SECOND_REFERENCE)?) == Some(&1),
            ) {
                Some(offset) => Some(kind_end.checked_add(offset)?),
                None => None,
            };
        let previous_history_state_id = previous_history_state_id_offset.and_then(|offset| {
            match View::u32_le_at(bytes, offset)? {
                u32::MAX => None,
                state_id => Some(i64::from(state_id)),
            }
        });
        let surface_stitch_operation = if kind == scope::DesignFeatureKind::SurfaceStitch {
            exact_surface_stitch_operation(bytes, records, record_index, reference_members)
        } else {
            None
        };
        let surface_patch_boundaries = if kind == scope::DesignFeatureKind::SurfacePatch {
            crate::design::decode::patch::surface_patch_boundaries(
                bytes,
                records,
                reference_members,
            )
        } else {
            Vec::new()
        };
        let base_flange_operation = if kind == scope::DesignFeatureKind::BaseFlange {
            exact_base_flange_operation(bytes, start, paired_at, reference_members)
        } else {
            None
        };
        let edge_flange_operation = if kind == scope::DesignFeatureKind::EdgeFlange {
            match exact_edge_flange_operation(
                bytes,
                start,
                paired_at,
                class_tag.as_str(),
                paired_class_tag,
                reference_members,
            ) {
                Ok(operation) => operation,
                Err(error) => return Some(Err(error)),
            }
        } else {
            None
        };
        let ruled_surface_operation = if kind == scope::DesignFeatureKind::SurfaceRuled {
            match exact_ruled_surface_operation(
                bytes,
                start,
                paired_at,
                *reference_count_at,
                reference_members,
            ) {
                Ok(operation) => operation,
                Err(error) => return Some(Err(error)),
            }
        } else {
            None
        };
        let family = design_feature_family(&kind);
        // A `Sketch` scope carries either the single entity-suffix reference form
        // or, when the stream's sketch entity headers use the `EntityGenesis`
        // form, the generic ordered reference table. Both parse here; the entity
        // binding in `decode_parameter_scopes` requires a unique suffix match.
        let extrude_prologue = if family == Some(DesignFeatureFamily::Extrude) {
            // The generic scope envelope is independently self-delimiting. An
            // unrecognized Extrude prologue therefore withholds only the typed
            // fields, not the scope and its ordered reference table.
            if typed_references {
                exact_typed_extrude_prologue(bytes, start, *reference_count_at, reference_members)
            } else {
                exact_extrude_prologue(
                    bytes,
                    start,
                    paired_at,
                    class_tag.as_str(),
                    paired_class_tag,
                    *reference_count_at,
                    reference_members,
                )
            }
        } else {
            None
        };
        let coil_discriminators = if family == Some(DesignFeatureFamily::Coil) {
            exact_coil_discriminators(bytes, start, paired_at, &kind, reference_members)
        } else {
            None
        };
        let coil_transform = if family == Some(DesignFeatureFamily::Coil) {
            exact_long_coil_transform(bytes, start, paired_at, &kind, reference_members)
        } else {
            None
        };
        let coil = if family == Some(DesignFeatureFamily::Coil) {
            Some(coil::DesignCoilScope {
                operation: coil_discriminators.as_ref().map(|fields| {
                    crate::records::identity::RecordedValue {
                        value: fields.operation,
                        offset: fields.operation_offset,
                    }
                }),
                extent: coil_discriminators
                    .as_ref()
                    .and_then(|fields| fields.extent),
                section: coil_discriminators
                    .as_ref()
                    .map(|fields| match fields.section_offset {
                        Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                            crate::records::identity::RecordedValue {
                                value: fields.section,
                                offset,
                            },
                        ),
                        None => {
                            crate::records::identity::MaybeRecordedValue::Unlocated(fields.section)
                        }
                    }),
                section_placement: coil_discriminators.as_ref().map(|fields| {
                    match fields.section_placement_offset {
                        Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                            crate::records::identity::RecordedValue {
                                value: fields.section_placement,
                                offset,
                            },
                        ),
                        None => crate::records::identity::MaybeRecordedValue::Unlocated(
                            fields.section_placement,
                        ),
                    }
                }),
                clockwise: coil_discriminators.as_ref().map(|fields| {
                    match fields.clockwise_offset {
                        Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                            crate::records::identity::RecordedValue {
                                value: fields.clockwise,
                                offset,
                            },
                        ),
                        None => crate::records::identity::MaybeRecordedValue::Unlocated(
                            fields.clockwise,
                        ),
                    }
                }),
                placement: None,
                transform: coil_transform,
            })
        } else {
            None
        };

        let mut located_references = Vec::new();
        located_references.extend(
            reference_members
                .iter()
                .copied()
                .zip(reference_member_offsets.iter().copied())
                .map(|(value, offset)| crate::records::identity::Located { value, offset }),
        );
        let mut scope = DesignParameterScope::try_new(scope::DesignParameterScopeDraft {
            id: String::new(),
            byte_offset,
            class_tag: class_tag.clone(),
            record_index,
            frame_length: u64::try_from(paired_at.checked_sub(start)?).ok()?,
            kind_offset: u64::try_from(kind_at.checked_add(4)?).ok()?,
            feature_ordinal,
            feature_ordinal_offset: u64::try_from(kind_end).ok()?,
            history_state_id,

            previous_history_state_id,
            previous_history_state_id_offset: previous_history_state_id_offset
                .and_then(|offset| u64::try_from(offset).ok())
                .filter(|&offset| offset != 0),
            reference_count_offset: u64::try_from(*reference_count_at).ok()?,
            reference_members: crate::records::identity::ReferenceRun::located(located_references),
            payload: match kind {
                scope::DesignFeatureKind::SurfaceStitch => {
                    scope::DesignScopePayload::SurfaceStitch(surface_stitch_operation?)
                }
                scope::DesignFeatureKind::SurfaceRuled => {
                    scope::DesignScopePayload::SurfaceRuled(ruled_surface_operation?)
                }
                kind => kind.try_into().ok()?,
            },
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::design::decode::text::class_tag_from_view(paired_class_tag)
                .ok()?,
            paired_byte_offset: u64_from_index(paired_at),
        })
        .ok()?;
        scope.source_kind = source_kind;
        if let Some(prologue) = extrude_prologue {
            {
                let construction = Some(scope::DesignExtrudeScope {
                    extrude_prologue: Some(prologue),
                    ..scope::DesignExtrudeScope::default()
                });
                if let scope::DesignScopePayloadMut::Extrude(slot)
                | scope::DesignScopePayloadMut::Extrusion(slot)
                | scope::DesignScopePayloadMut::Extrusao(slot)
                | scope::DesignScopePayloadMut::Extruir(slot)
                | scope::DesignScopePayloadMut::ExtrudeJa(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
        }
        if let Some(coil) = coil {
            {
                let construction = Some(coil);
                if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
                | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
        }
        if !surface_patch_boundaries.is_empty() {
            {
                let construction = surface_patch_boundaries;
                if let scope::DesignScopePayloadMut::SurfacePatch(slot) = scope.payload_mut() {
                    slot.boundaries = construction;
                }
            }
        }
        if let Some(operation) = base_flange_operation {
            {
                let construction = Some(scope::DesignBaseFlangeScope {
                    base_flange_operation: Some(operation),
                    ..scope::DesignBaseFlangeScope::default()
                });
                if let scope::DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
        }
        if let Some(operation) = edge_flange_operation {
            {
                let construction = Some(operation);
                if let scope::DesignScopePayloadMut::EdgeFlange(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
        }
        Some(Ok(scope))
    })()
    .transpose()
}

fn exact_thread_bound_hole_tail(tail: &[u8], hole: u32) -> bool {
    if tail.len() != hole_tail::LEN {
        return false;
    }
    let reference = |at| super::shared_frames::marked_record_reference(tail, at);
    let Some(thread) =
        reference(hole_tail::THREAD_REFERENCE).filter(|thread| *thread > 3 && *thread != hole)
    else {
        return false;
    };
    View::u32_le_at(tail, hole_tail::FEATURE_ORDINAL).is_some_and(|ordinal| ordinal != 0)
        && tail.get(hole_tail::ZERO_PREFIX..hole_tail::MEMBER_COUNT) == Some(&[0; 8])
        && View::u32_le_at(tail, hole_tail::MEMBER_COUNT) == Some(2)
        && reference(hole_tail::SELF_REFERENCE) == Some(hole)
        && tail.get(hole_tail::ZERO_BEFORE_COMPANION..hole_tail::FIRST_COMPANION) == Some(&[0; 3])
        && reference(hole_tail::FIRST_COMPANION) == thread.checked_sub(1)
        && tail.get(hole_tail::ZERO_BEFORE_HISTORY) == Some(&0)
        && View::u32_le_at(tail, hole_tail::PREVIOUS_HISTORY_STATE).is_some()
        && View::f64_le_at(tail, hole_tail::DURATION)
            .is_some_and(|duration| duration.is_finite() && duration >= 0.0)
        && View::u32_le_at(tail, hole_tail::EPOCH)
            == View::u32_le_at(tail, hole_tail::REPEATED_EPOCH)
        && reference(hole_tail::SECOND_COMPANION) == thread.checked_sub(2)
        && tail.get(hole_tail::ZERO_BEFORE_THIRD..hole_tail::THIRD_COMPANION) == Some(&[0; 2])
        && reference(hole_tail::THIRD_COMPANION) == thread.checked_sub(3)
        && reference(hole_tail::CLOSING_SELF_REFERENCE) == Some(hole)
        && tail.get(hole_tail::ZERO_TAIL) == Some(&0)
}

fn exact_typed_canvas_scope_tail(tail: &[u8]) -> bool {
    tail.len() == typed_canvas_tail::LEN
        && View::u32_le_at(tail, typed_canvas_tail::FEATURE_ORDINAL)
            .is_some_and(|ordinal| ordinal != 0)
        && tail.get(typed_canvas_tail::ZERO_PREFIX..typed_canvas_tail::FIRST_COMPANION)
            == Some(&[0; 15])
        && typed_canvas_reference(
            tail,
            typed_canvas_tail::FIRST_COMPANION,
            "B7F34D7B-DAC4-41A5-B67D-D672537C3BFA",
        )
        .is_some_and(|index| index != 0)
        && View::u32_le_at(tail, typed_canvas_tail::HISTORY_STATE) == Some(u32::MAX)
        && View::f64_le_at(tail, typed_canvas_tail::DURATION)
            .is_some_and(|duration| duration.is_finite() && duration >= 0.0)
        && View::u32_le_at(tail, typed_canvas_tail::EPOCH)
            == View::u32_le_at(tail, typed_canvas_tail::REPEATED_EPOCH)
        && typed_canvas_reference(
            tail,
            typed_canvas_tail::SECOND_COMPANION,
            "5FBE538E-139B-46D4-BCAF-CC796D1E1F78",
        )
        .is_some_and(|index| index != 0)
        && tail.get(typed_canvas_tail::ZERO_BEFORE_THIRD..typed_canvas_tail::THIRD_COMPANION)
            == Some(&[0; 2])
        && typed_canvas_reference(
            tail,
            typed_canvas_tail::THIRD_COMPANION,
            "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
        )
        .is_some_and(|index| index != 0)
        && tail.get(typed_canvas_tail::ZERO_TAIL..) == Some(&[0; 2])
}

fn exact_canvas_scope_tail(tail: &[u8], record_index: u32) -> bool {
    if tail.len() != canvas_tail::LEN {
        return false;
    }
    let reference = |at| super::shared_frames::marked_record_reference(tail, at);
    View::u32_le_at(tail, canvas_tail::FEATURE_ORDINAL).is_some_and(|ordinal| ordinal != 0)
        && tail.get(canvas_tail::ZERO_PREFIX..canvas_tail::MEMBER_COUNT) == Some(&[0; 8])
        && View::u32_le_at(tail, canvas_tail::MEMBER_COUNT) == Some(canvas_tail::MEMBER_COUNT_VALUE)
        && reference(canvas_tail::SELF_REFERENCE) == Some(record_index)
        && tail.get(canvas_tail::ZERO_BEFORE_COMPANION..canvas_tail::FIRST_COMPANION)
            == Some(&[0; 3])
        && reference(canvas_tail::FIRST_COMPANION).is_some_and(|index| index != 0)
        && tail.get(canvas_tail::ZERO_BEFORE_HISTORY)
            == Some(&canvas_tail::ZERO_BEFORE_HISTORY_VALUE)
        && View::u32_le_at(tail, canvas_tail::HISTORY_STATE)
            == Some(canvas_tail::HISTORY_STATE_VALUE)
        && View::f64_le_at(tail, canvas_tail::DURATION)
            .is_some_and(|value| value.is_finite() && value >= 0.0)
        && View::u32_le_at(tail, canvas_tail::EPOCH)
            == View::u32_le_at(tail, canvas_tail::REPEATED_EPOCH)
        && reference(canvas_tail::SECOND_COMPANION).is_some_and(|index| index != 0)
        && tail.get(canvas_tail::ZERO_BEFORE_THIRD..canvas_tail::THIRD_COMPANION) == Some(&[0; 2])
        && reference(canvas_tail::THIRD_COMPANION).is_some_and(|index| index != 0)
        && reference(canvas_tail::CLOSING_SELF_REFERENCE) == Some(record_index)
        && tail.get(canvas_tail::ZERO_TAIL) == Some(&canvas_tail::ZERO_TAIL_VALUE)
}

/// Largest repeated lane value of a named-scope lane block with a nonempty
/// label: the high byte of the three descending 16-bit field targets. An empty
/// label takes only the values zero and one with class word 252, because that
/// tail is also the 78-byte fixed tail.
const NAMED_LANE_VALUE_MAX: u64 = 0xff;
/// Three-digit dynamic class numbers admitted for the two equal class words of
/// a named-scope lane block.
const NAMED_LANE_CLASS_WORDS: std::ops::RangeInclusive<u32> = 100..=999;

fn named_parameter_scope_tail_is_valid(
    bytes: &[u8],
    kind_end: usize,
    paired_at: usize,
    tail_length: usize,
) -> Option<bool> {
    let label_at = kind_end.checked_add(8)?;
    let (label, label_end) = lp_utf16_bounded(bytes, label_at, 0..=256)?;
    (|| {
        let label_code_units = label.encode_utf16().count();
        if tail_length != 78usize.checked_add(label_code_units.checked_mul(2)?)?
            || label_end.checked_add(7)? != kind_end.checked_add(19 + label_code_units * 2)?
            || label.chars().any(char::is_control)
        {
            return Some(false);
        }
        let marker = kind_end.checked_add(19 + label_code_units.checked_mul(2)?)?;
        if marker.checked_add(59)? != paired_at || bytes.get(label_end..marker)? != [0; 7] {
            return Some(false);
        }
        let first_lane_value = View::u64_le_at(bytes, marker + 2)?;
        let second_lane_value = View::u64_le_at(bytes, marker + 34)?;
        let third_lane_value = View::u64_le_at(bytes, marker + 48)?;
        let class_word = View::u32_le_at(bytes, marker + 16)?;
        Some(
            bytes.get(kind_end + 4..kind_end + 8)? == [0; 4]
                && bytes.get(marker) == Some(&1)
                && bytes.get(marker + 1).is_some_and(|field_id| *field_id != 0)
                && ((first_lane_value <= 1 && class_word == 0xfc)
                    || (label_code_units > 0
                        && first_lane_value <= NAMED_LANE_VALUE_MAX
                        && NAMED_LANE_CLASS_WORDS.contains(&class_word)))
                && second_lane_value == first_lane_value
                && third_lane_value == first_lane_value
                && bytes.get(marker + 10..marker + 12)? == [0; 2]
                && View::u32_le_at(bytes, marker + 12)? > 0
                && View::f64_le_at(bytes, marker + 20)?.is_finite()
                && View::u32_le_at(bytes, marker + 28)? == class_word
                && bytes.get(marker + 32) == Some(&1)
                && bytes
                    .get(marker + 33)
                    .is_some_and(|field_id| *field_id != 0)
                && bytes.get(marker + 42..marker + 46)? == [0, 1, 0, 0]
                && bytes.get(marker + 46) == Some(&1)
                && bytes
                    .get(marker + 47)
                    .is_some_and(|field_id| *field_id != 0)
                && bytes.get(marker + 56..marker + 59)? == [0; 3],
        )
    })()
}

/// Select the family of a stored family name. A name that names no family
/// takes the family of the scope class type identity when one is listed.
fn scope_feature_kind(
    stored: &str,
    type_family: Option<&scope::DesignFeatureKind>,
) -> Option<scope::DesignFeatureKind> {
    let literal = scope::DesignFeatureKind::try_from(stored.to_owned()).ok()?;
    Some(match (literal, type_family) {
        (scope::DesignFeatureKind::Native(_), Some(family)) => family.clone(),
        (literal, _) => literal,
    })
}

pub(crate) fn parameter_scope_payload_length(scope: &DesignParameterScope) -> Option<u64> {
    let kind_bytes = u64::try_from(scope.source_kind_name().encode_utf16().count())
        .ok()?
        .checked_mul(2)?;
    scope.frame_length().checked_sub(kind_bytes)
}

fn extrude_extent_native_loss(
    scope: &DesignParameterScope,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude)
        && scope.extrude_prologue().is_none())
    .then(|| {
        crate::loss::F3dLossCode::FeatureExtrudeExtentNative.note(format!(
            "Extrude scope {} retains its unassigned extent payload and ordered references.",
            scope.id
        ))
    })
}

#[cfg(test)]
mod tests;
