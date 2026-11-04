// SPDX-License-Identifier: Apache-2.0
//! Bulkstream and sketch/design record encoders for source-less generation.

use crate::records::{
    recipes::ConstructionRecipeKind,
    references::PersistentReferenceKind,
    sketch_geometry::{SketchCurveGeometry, SketchPointRecordForm, SketchText},
};
use crate::writer::primitives::zero_fill_to;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CoedgeId;

use super::{
    index::NativeGenerationIndex,
    native_bytes::{native_f64, native_i64, native_ref},
    native_geometry::native_nurbs_curve,
    presentation::GeneratedDesignRegistry,
};
use crate::native::F3dNative;
use crate::writer::primitives::native_bool;
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;

/// Generated Design `BulkStream` and the primary offsets known while writing it.
pub(in crate::writer::generate) struct EncodedDesignBulkStream {
    pub(super) bytes: Vec<u8>,
    pub(super) primary_records: Vec<crate::metastream::RecordIndexEntry>,
}

pub(super) fn tolerant_coedge_range(
    index: &NativeGenerationIndex<'_>,
    coedge: &CoedgeId,
) -> Option<[f64; 2]> {
    index
        .tolerant_coedges
        .get(coedge.as_str())
        .map(|parameters| parameters.parameter_range.get())
}

pub(super) fn native_tolerant_coedge_extension(
    records: &mut Vec<u8>,
    target: &CadIr,
    index: &NativeGenerationIndex<'_>,
    coedge: &CoedgeId,
) -> Result<(), CodecError> {
    let extension = index
        .tolerant_coedges
        .get(coedge.as_str())
        .map(|parameters| &parameters.extension);
    match extension {
        None
        | Some(
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::None {}
            | cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: None },
        ) => {
            native_ref(records, -1);
            native_i64(records, 0);
            native_i64(records, 0);
            Ok(())
        }
        Some(cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
            target: None,
            curve_reversed,
            parameter_range,
            ..
        }) => {
            let model_coedge = target
                .model
                .coedges
                .iter()
                .find(|candidate| candidate.id == *coedge)
                .ok_or_else(|| CodecError::malformed(format_args!("missing coedge {coedge}")))?;
            let use_curve = model_coedge.use_curve.as_ref().ok_or_else(|| {
                CodecError::malformed(format_args!("tolerant coedge {coedge} has no use curve"))
            })?;
            let curve = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == use_curve.curve)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("missing use curve {}", use_curve.curve))
                })?;
            let Some(SolvedCurveGeometry::Nurbs(curve)) = curve.geometry.solved() else {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D tolerant coedge {coedge} requires a NURBS use curve"
                )));
            };
            let mut native_curve = curve.clone();
            if *curve_reversed {
                native_curve.reverse_parameterization();
            }
            native_ref(records, -1);
            native_i64(records, 1);
            records.push(native_bool(*curve_reversed));
            records.push(0x0f);
            native_nurbs_curve(records, &native_curve)?;
            records.push(0x10);
            if let Some(range) = *parameter_range {
                let [start, end] = range.get();
                records.push(0x0a);
                native_f64(records, start);
                records.push(0x0a);
                native_f64(records, end);
            } else {
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            native_i64(records, 0);
            Ok(())
        }
        _ => Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize nonempty tolerant-coedge extension for {coedge}"
        ))),
    }
}

pub(super) fn encode_design_bulkstream(
    target: &CadIr,
    native: &F3dNative,
    registry: &GeneratedDesignRegistry,
    parameter_bytes: Vec<u8>,
) -> Result<Option<EncodedDesignBulkStream>, CodecError> {
    let decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let decode_ctx = &decode_ctx;

    let projected_parameters =
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &native.design_parameters,
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                timelines: &native.design_feature_timelines,
                construction_groups: &native.design_construction_operand_groups,
                fillet_radius_groups: &native.design_fillet_radius_groups,
                edge_operands: &native.design_edge_operands,
                edge_identity_operands: &native.design_edge_identity_operands,
                edge_treatment_vertex_operands: &native.design_edge_treatment_vertex_operands,
                entity_selection_operands: &native.design_entity_selection_operands,
                curve_identities: &native.sketch_curve_identities,
                face_operands: &native.design_face_operands,
                body_recipe_operands: &native.design_body_recipe_operands,
                legacy_loft_body_carriers: &native.design_loft_legacy_body_carriers,
                placements: &native.design_sketch_placements,
                body_bindings: &native.design_body_bindings,
                component_naming_spaces: &native.design_component_naming_spaces,
                histories: &native.asm_histories,
                face_boundaries: &native.asm_face_boundaries,
                sheet_metal_settings: &native.design_sheet_metal_settings,
            },
        )?
        .parameters;
    if target.model.parameters != projected_parameters {
        return Err(CodecError::Malformed(
            "neutral F3D parameters must equal the projection of native Design parameters".into(),
        ));
    }
    if !native.design_parameter_companions.is_empty()
        || !native.design_dimension_annotation_frames.is_empty()
        || !native.design_dimension_presentation_frames.is_empty()
        || !native.design_face_source_groups.is_empty()
        || !native.design_dimension_locus_pairs.is_empty()
        || !native.design_dimension_locus_groups.is_empty()
        || !native.design_dimension_null_locus_pairs.is_empty()
        || !native.design_dimension_recipe_records.is_empty()
        || !native.design_parameter_owners.is_empty()
        || !native.design_parameter_scopes.is_empty()
        || !native.design_sketch_placements.is_empty()
        || native
            .design_entity_headers
            .iter()
            .any(|header| header.member_values().next().is_some())
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D Design parameter records are not writable".into(),
        ));
    }
    let has_body_visibility = target
        .model
        .bodies
        .iter()
        .any(|body| body.visible.is_some());
    if native.design_parameters.is_empty()
        && native.construction_recipes.is_empty()
        && native.persistent_references.is_empty()
        && native.lost_edge_references.is_empty()
        && native.design_body_members.is_empty()
        && native.design_entity_headers.is_empty()
        && native.design_record_headers.is_empty()
        && native.design_face_source_groups.is_empty()
        && native.design_material_assignments.is_empty()
        && native.sketch_points.is_empty()
        && native.sketch_curve_identities.is_empty()
        && native.sketch_relations.is_empty()
        && native.sketch_texts.is_empty()
        && !has_body_visibility
    {
        return Ok(None);
    }

    let mut out = parameter_bytes;
    let mut primary_records = Vec::new();
    if let Some(body_map) = &registry.body_map {
        let class_tag = body_map.class_tag.as_str();
        let record_index = body_map.record_index;
        primary_records.push(primary_record(record_index, out.len())?);
        native_lp_ascii(&mut out, class_tag)?;
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
        let count = u32::try_from(body_map.entries.as_map().len())
            .map_err(|_| CodecError::NotImplemented("Design body map exceeds u32::MAX".into()))?;
        out.extend_from_slice(&count.to_le_bytes());
        for (&body_key, &entity_suffix) in body_map.entries.as_map() {
            out.extend_from_slice(&body_key.to_le_bytes());
            out.extend_from_slice(&entity_suffix.to_le_bytes());
        }
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        native_lp_utf16(&mut out, "BREP.generated.smbh")?;
    }
    encode_browser_nodes(&mut out, &mut primary_records, registry)?;
    for recipe in &native.construction_recipes {
        let name = construction_recipe_name(recipe.kind);
        let mut prefix = [0u8; 27];
        if let Some(design) = &recipe.design {
            let design_id = &design.id.value;
            if design_id.len() != 3 || !design_id.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(CodecError::NotImplemented(format!(
                    "source-less Design recipe id must be three ASCII digits: {design_id}"
                )));
            }
            prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
            prefix[4..7].copy_from_slice(design_id.as_bytes());
        }
        // A generated Design recipe states its own record index; a native
        // recipe that states none has no index word to regenerate.
        let Some(record_index) = recipe.record_index else {
            return Err(CodecError::NotImplemented(
                "source-less F3D construction recipe states no record index".into(),
            ));
        };
        prefix[11..15].copy_from_slice(&record_index.value.to_le_bytes());
        prefix[23..27].copy_from_slice(
            &u32::try_from(name.len())
                .map_err(|_| {
                    CodecError::NotImplemented("Design recipe name exceeds u32::MAX".into())
                })?
                .to_le_bytes(),
        );
        out.extend_from_slice(&prefix);
        out.extend_from_slice(name);
        out.extend_from_slice(&(-1i64).to_le_bytes());
        for value in [2i32, 0, -1, 1, -1] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    if !native.design_body_members.is_empty() {
        native_lp_ascii(&mut out, "BodiesRoot")?;
        out.extend_from_slice(&0u16.to_le_bytes());
        native_lp_ascii(&mut out, "BodiesRoot")?;
        let count = u32::try_from(native.design_body_members.len()).map_err(|_| {
            CodecError::NotImplemented("Design BodiesRoot exceeds u32::MAX members".into())
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for member in &native.design_body_members {
            out.push(1);
            out.extend_from_slice(&member.entity_suffix.to_le_bytes());
            out.extend_from_slice(&member.flags.to_le_bytes());
        }
        out.push(0);
    }
    for header in &native.design_record_headers {
        native_lp_ascii(&mut out, header.class_tag.as_str())?;
        out.extend_from_slice(&header.record_index.to_le_bytes());
    }
    for header in &native.design_entity_headers {
        primary_records.push(primary_record_u64(header.entity_id.suffix(), out.len())?);
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(header.class_tag.as_bytes());
        out.extend_from_slice(&header.entity_id.suffix().to_le_bytes());
        out.extend_from_slice(&[0; 5]);
        out.push(u8::from(header.optional_slot_present));
        if header.optional_slot_present {
            out.extend_from_slice(&[0; 4]);
        }
        native_lp_utf16(&mut out, header.entity_id.as_str())?;
        if header.in_sketch_module() {
            let count = u32::try_from(header.reference_values().count()).map_err(|_| {
                CodecError::NotImplemented(
                    "Design sketch header exceeds u32::MAX references".into(),
                )
            })?;
            match header
                .sketch_references()
                .and_then(|list| list.record_reference)
            {
                Some(record_reference) => {
                    out.extend_from_slice(&record_reference.to_le_bytes());
                    out.extend_from_slice(&[0; 4]);
                }
                // The sentinel base-record slot of a sketch with no base record.
                None => out.extend_from_slice(&[0xFF; 8]),
            }
            out.push(1);
            out.extend_from_slice(&count.to_le_bytes());
            for reference in header.reference_values() {
                out.push(1);
                out.extend_from_slice(&reference.to_le_bytes());
                out.extend_from_slice(&[0; 6]);
            }
        }
    }
    for point in &native.sketch_points {
        point
            .validate_for_write(&native.sketch_curve_identities)
            .map_err(CodecError::Malformed)?;
        let expected = crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE;
        let mut companion_types = registry
            .types
            .iter()
            .enumerate()
            .filter(|(_, design_type)| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(expected.0)
                    && design_type.version == expected.1
                    && design_type.module == expected.2
                    && design_type
                        .entity_ids
                        .contains(&u64::from(point.paired_reference))
            });
        let companion_type_ordinal = companion_types
            .next()
            .map(|(ordinal, _)| ordinal)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "generated sketch point {} has no registered companion type",
                    point.id
                ))
            })?;
        if companion_types.next().is_some() {
            return Err(CodecError::malformed(format_args!(
                "generated sketch point {} has multiple registered companion types",
                point.id
            )));
        }
        let companion_class_tag = super::presentation::dynamic_class_tag(companion_type_ordinal)?;
        primary_records.push(primary_record(point.record_index, out.len())?);
        encode_sketch_point(&mut out, point)?;
        primary_records.push(primary_record(point.paired_reference, out.len())?);
        encode_sketch_point_companion(
            &mut out,
            &companion_class_tag,
            point.paired_reference,
            point.record_index,
            point.companion(),
        )?;
    }
    for curve in &native.sketch_curve_identities {
        primary_records.push(primary_record(curve.record_index, out.len())?);
        encode_sketch_curve_identity(&mut out, curve)?;
    }
    for text in &native.sketch_texts {
        primary_records.push(primary_record(text.record_index, out.len())?);
        encode_sketch_text(&mut out, text)?;
    }
    for relation in &native.sketch_relations {
        primary_records.push(primary_record(relation.record_index, out.len())?);
        encode_sketch_relation(&mut out, relation)?;
    }
    for reference in &native.persistent_references {
        out.extend_from_slice(persistent_reference_name(reference.kind));
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&14u32.to_le_bytes());
        out.extend_from_slice(&[0; 14]);
        out.extend_from_slice(&23u32.to_le_bytes());
        out.extend_from_slice(b"IntrinsicMetaTypeuint64");
        out.extend_from_slice(&reference.value.to_le_bytes());
    }
    for (ordinal, reference) in native.lost_edge_references.iter().enumerate() {
        if let Some(previous) = ordinal
            .checked_sub(1)
            .and_then(|ordinal| native.lost_edge_references.get(ordinal))
        {
            if previous.next_class_tag != reference.class_tag
                || previous.next_record_index != reference.record_index
            {
                return Err(CodecError::malformed(format_args!(
                    "F3D lost-edge record {} does not continue the preceding indexed run",
                    reference.id
                )));
            }
        } else {
            native_lp_ascii(&mut out, reference.class_tag.as_str())?;
            out.extend_from_slice(&reference.record_index.to_le_bytes());
        }
        out.extend_from_slice(&[0; 14]);
        out.extend_from_slice(&19u32.to_le_bytes());
        out.extend_from_slice(b"EDGE_REFERENCE_LOST");
        native_lp_ascii(&mut out, reference.next_class_tag.as_str())?;
        out.extend_from_slice(&reference.next_record_index.to_le_bytes());
    }
    Ok(Some(EncodedDesignBulkStream {
        bytes: out,
        primary_records,
    }))
}

fn primary_record(
    entity_id: u32,
    bulk_offset: usize,
) -> Result<crate::metastream::RecordIndexEntry, CodecError> {
    primary_record_u64(u64::from(entity_id), bulk_offset)
}

fn primary_record_u64(
    entity_id: u64,
    bulk_offset: usize,
) -> Result<crate::metastream::RecordIndexEntry, CodecError> {
    Ok(crate::metastream::RecordIndexEntry {
        entity_id,
        bulk_offset: u64::try_from(bulk_offset).map_err(|_| {
            CodecError::NotImplemented("generated Design record offset exceeds u64".into())
        })?,
    })
}

pub(super) fn encode_document_parameters(
    parameters: &[crate::records::parameters::DesignParameter],
) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    let mut parameter_indices = std::collections::BTreeSet::new();
    let mut parameter_ordinals = std::collections::BTreeSet::new();
    for parameter in parameters {
        let expected_discriminator =
            crate::design::decode::parameters::design_parameter_discriminator(
                parameter.source_kind_str(),
            );
        if parameter
            .family_discriminator()
            .map(|value| value.value.code())
            != Some(expected_discriminator)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design parameter {} has discriminator {:?}, expected {expected_discriminator} for {}",
                parameter.id, parameter.family_discriminator().map(|value| value.value.code()), parameter.source_kind()
            )));
        }
        let crate::records::parameters::DesignParameterSource::User {
            family_discriminator: Some(family_discriminator),
            ..
        } = parameter.source()
        else {
            return Err(CodecError::NotImplemented(
                "source-less F3D owned Design parameter records are not writable".into(),
            ));
        };
        if !parameter_indices.insert(parameter.record_index)
            || !parameter_ordinals.insert(parameter.source_ordinal)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design parameter {} duplicates a record index or source ordinal",
                parameter.id
            )));
        }
        native_lp_ascii(&mut out, parameter.class_tag.as_str())?;
        out.extend_from_slice(&parameter.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 11]);
        out.extend_from_slice(&family_discriminator.value.code().to_le_bytes());
        out.push(0);
        out.extend_from_slice(&parameter.source_ordinal.to_le_bytes());
        out.push(0);
        native_lp_utf16(&mut out, parameter.expression())?;
        out.extend_from_slice(&[0; 8]);
        out.push(1);
        native_lp_utf16(&mut out, "User Parameter")?;
        out.extend_from_slice(&0u32.to_le_bytes());
        if let Some(unit) = parameter.unit() {
            native_lp_utf16(&mut out, unit.value.as_str())?;
        } else {
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        native_lp_utf16(&mut out, parameter.name())?;
        out.extend_from_slice(&parameter.evaluated_value().get().to_le_bytes());
        out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
    Ok(out)
}

fn encode_sketch_record_header(
    out: &mut [u8],
    class_tag: &crate::records::references::DesignClassTag,
    record_index: u32,
) {
    out[0..4].copy_from_slice(&3u32.to_le_bytes());
    out[4..7].copy_from_slice(class_tag.as_bytes());
    out[7..11].copy_from_slice(&record_index.to_le_bytes());
}

fn encode_sketch_point(
    out: &mut Vec<u8>,
    point: &crate::records::sketch_geometry::SketchPoint,
) -> Result<(), CodecError> {
    let owner_reference = point.owner_reference.ok_or_else(|| {
        CodecError::malformed(format_args!(
            "source-less sketch point {} has no direct owner",
            point.id
        ))
    })?;
    let SketchPointRecordForm::Version11 {
        padded_paired_reference,
        derived_type: false,
        entity_genesis,
        depth,
        persistent_id,
        flags,
        closure,
        ..
    } = point.record_form()
    else {
        return Err(CodecError::NotImplemented(format!(
            "source-less sketch point {} requires the version-11 member sequence",
            point.id
        )));
    };
    let shift = usize::from(entity_genesis.is_some()) * 52;
    let mut record = Vec::new();
    zero_fill_to(&mut record, 105 + shift);
    encode_sketch_record_header(&mut record, &point.class_tag, point.record_index);
    record[20] = 1;
    record[21..25].copy_from_slice(&(1 + u32::from(entity_genesis.is_some())).to_le_bytes());
    if let Some(entity_genesis) = entity_genesis {
        encode_entity_genesis(&mut record, *entity_genesis);
    }
    record[25 + shift..29 + shift].copy_from_slice(&6u32.to_le_bytes());
    record[29 + shift..35 + shift].copy_from_slice(b"pt_tag");
    record[35 + shift..39 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[39 + shift..62 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[62 + shift..70 + shift].copy_from_slice(&persistent_id.get().to_le_bytes());
    record[70 + shift] = 1;
    record[71 + shift..75 + shift].copy_from_slice(&point.paired_reference.to_le_bytes());
    record[81 + shift..89 + shift].copy_from_slice(&flags.map(u8::from));
    record[89 + shift..97 + shift]
        .copy_from_slice(&(point.coordinates().u / LEN_TO_MM).to_le_bytes());
    record[97 + shift..105 + shift]
        .copy_from_slice(&(point.coordinates().v / LEN_TO_MM).to_le_bytes());
    record.extend_from_slice(&(depth.get() / LEN_TO_MM).to_le_bytes());
    record.extend_from_slice(&closure.selector().to_le_bytes());
    record.push(closure.state());
    record.extend_from_slice(&[0; 12]);
    record.extend_from_slice(&1.0f32.to_le_bytes());
    record.extend_from_slice(&1.0f32.to_le_bytes());
    record.extend_from_slice(&[0, 1, 0, 0, 0]);
    write_reference(&mut record, point.paired_reference);
    if *padded_paired_reference {
        record.extend_from_slice(&[0; 4]);
    }
    write_reference(&mut record, owner_reference);
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_sketch_point_companion(
    out: &mut Vec<u8>,
    class_tag: &crate::records::references::DesignClassTag,
    record_index: u32,
    point_record_index: u32,
    companion: crate::records::sketch_geometry::SketchPointCompanionRef<'_>,
) -> Result<(), CodecError> {
    let prefix_present_zero = companion.prefix_present_zero;
    let incident_curves = companion.incident_curves;
    let count = u32::try_from(incident_curves.len()).map_err(|_| {
        CodecError::NotImplemented(
            "source-less sketch point companion exceeds u32::MAX curves".into(),
        )
    })?;
    let prefix_len = if prefix_present_zero { 25 } else { 21 };
    let mut record = Vec::new();
    zero_fill_to(&mut record, prefix_len);
    encode_sketch_record_header(&mut record, class_tag, record_index);
    if prefix_present_zero {
        record[20] = 1;
    }
    record.extend_from_slice(&count.to_le_bytes());
    for incident_curve in incident_curves {
        write_reference(&mut record, *incident_curve);
    }
    record.push(0);
    write_reference(&mut record, point_record_index);
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_sketch_curve_identity(
    out: &mut Vec<u8>,
    curve: &crate::records::sketch_geometry::SketchCurveIdentity,
) -> Result<(), CodecError> {
    let owner_reference = curve.owner_reference.ok_or_else(|| {
        CodecError::malformed(format_args!(
            "source-less sketch curve {} has no direct owner",
            curve.id
        ))
    })?;
    let shift = usize::from(curve.entity_genesis.is_some()) * 52;
    let mut record = Vec::new();
    zero_fill_to(&mut record, 133 + shift);
    encode_sketch_record_header(&mut record, &curve.class_tag, curve.record_index);
    record[20] = 1;
    record[21..25].copy_from_slice(&(2 + u32::from(curve.entity_genesis.is_some())).to_le_bytes());
    if let Some(entity_genesis) = curve.entity_genesis {
        encode_entity_genesis(&mut record, entity_genesis);
    }
    record[25 + shift..29 + shift].copy_from_slice(&14u32.to_le_bytes());
    record[29 + shift..43 + shift].copy_from_slice(b"crv_primary_id");
    record[43 + shift..47 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[47 + shift..70 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[70 + shift..78 + shift].copy_from_slice(&curve.primary_id.get().to_le_bytes());
    record[78 + shift..82 + shift].copy_from_slice(&16u32.to_le_bytes());
    record[82 + shift..98 + shift].copy_from_slice(b"crv_secondary_id");
    record[98 + shift..102 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[102 + shift..125 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[125 + shift..133 + shift].copy_from_slice(&curve.secondary_id.to_le_bytes());
    match curve.geometry.as_ref() {
        Some(SketchCurveGeometry::Line {
            start,
            end,
            direction,
            normal,
        }) => {
            let start = start.get();
            let end = end.get();
            let direction = direction.as_raw();
            let normal = normal.as_raw();
            let values = [
                start.x / LEN_TO_MM,
                start.y / LEN_TO_MM,
                start.z / LEN_TO_MM,
                (end.x - start.x) / LEN_TO_MM,
                (end.y - start.y) / LEN_TO_MM,
                (end.z - start.z) / LEN_TO_MM,
                direction.x,
                direction.y,
                direction.z,
                normal.x,
                normal.y,
                normal.z,
            ];
            encode_f64_sequence(&mut record, &values);
        }
        Some(SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        }) => {
            let center = center.as_raw();
            let normal = normal.as_raw();
            let reference_direction = reference_direction.as_raw();
            let values = [
                center.x / LEN_TO_MM,
                center.y / LEN_TO_MM,
                center.z / LEN_TO_MM,
                normal.x,
                normal.y,
                normal.z,
                reference_direction.x,
                reference_direction.y,
                reference_direction.z,
                radius.get() / LEN_TO_MM,
                start_angle.get(),
                end_angle.get(),
            ];
            encode_f64_sequence(&mut record, &values);
        }
        Some(SketchCurveGeometry::Nurbs {
            carrier_reference,
            subtype_class_tag,
            subtype_record_index,
            geometry,
        }) => encode_sketch_nurbs(
            &mut record,
            *carrier_reference,
            subtype_class_tag,
            *subtype_record_index,
            geometry,
        )?,
        None => {
            return Err(CodecError::NotImplemented(format!(
                "source-less sketch curve {} has no writable geometry",
                curve.id
            )))
        }
    }
    write_reference(&mut record, owner_reference);
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_entity_genesis(record: &mut [u8], entity_genesis: u64) {
    record[25..29].copy_from_slice(&13u32.to_le_bytes());
    record[29..42].copy_from_slice(b"EntityGenesis");
    record[42..46].copy_from_slice(&23u32.to_le_bytes());
    record[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[69..77].copy_from_slice(&entity_genesis.to_le_bytes());
}

fn encode_f64_sequence(out: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// The sketch NURBS null carrier reference.
///
/// `docs/formats/f3d.md`: the payload begins with "either an eight-byte
/// all-`0xff` null sentinel or a non-null u64 carrier reference".
const NULL_CARRIER_REFERENCE: u64 = u64::MAX;

fn encode_sketch_nurbs(
    record: &mut Vec<u8>,
    carrier_reference: Option<u64>,
    subtype_class_tag: &crate::records::references::DesignClassTag,
    subtype_record_index: u32,
    geometry: &crate::records::sketch_geometry::SketchNurbsGeometry,
) -> Result<(), CodecError> {
    let knots = geometry.knots();
    let poles = geometry.poles();
    record.extend_from_slice(
        &carrier_reference
            .unwrap_or(NULL_CARRIER_REFERENCE)
            .to_le_bytes(),
    );
    record.extend_from_slice(&3u32.to_le_bytes());
    record.extend_from_slice(subtype_class_tag.as_bytes());
    record.extend_from_slice(&subtype_record_index.to_le_bytes());
    zero_fill_to(record, 133 + 88);
    record.push(1);
    record.push(0);
    record.extend_from_slice(&geometry.degree().to_le_bytes());
    record.extend_from_slice(&(geometry.fit_tolerance().get() / LEN_TO_MM).to_le_bytes());
    let knot_count = u32::try_from(knots.len())
        .map_err(|_| CodecError::NotImplemented("sketch NURBS has too many knots".into()))?;
    record.extend_from_slice(&knot_count.to_le_bytes());
    record.extend_from_slice(&knot_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    encode_f64_sequence(record, knots);
    let weight_count = u32::try_from(poles.weights().len())
        .map_err(|_| CodecError::NotImplemented("sketch NURBS has too many weights".into()))?;
    record.extend_from_slice(&weight_count.to_le_bytes());
    record.extend_from_slice(&weight_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    encode_f64_sequence(record, &poles.weights().collect::<Vec<_>>());
    let point_count = u32::try_from(poles.point_count()).map_err(|_| {
        CodecError::NotImplemented("sketch NURBS has too many control points".into())
    })?;
    record.extend_from_slice(&point_count.to_le_bytes());
    record.extend_from_slice(&point_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    let coordinates = poles
        .points()
        .flat_map(|point| {
            [
                point.x / LEN_TO_MM,
                point.y / LEN_TO_MM,
                point.z / LEN_TO_MM,
            ]
        })
        .collect::<Vec<_>>();
    encode_f64_sequence(record, &coordinates);
    Ok(())
}

fn encode_sketch_text(out: &mut Vec<u8>, text: &SketchText) -> Result<(), CodecError> {
    let decoded = crate::design::decode::sketch::decode_sketch_text_record(
        &text.raw_bytes,
        "Design/BulkStream.dat",
        text.class_tag.clone(),
        text.class_version,
        text.record_index,
        0,
    )?
    .ok_or_else(|| {
        CodecError::malformed(format_args!("invalid raw sketch-text record {}", text.id))
    })?;
    let common_header_matches = text.raw_bytes.get(0..4) == Some(&3u32.to_le_bytes())
        && text.raw_bytes.get(4..7) == Some(text.class_tag.as_bytes());
    let legacy_index_matches =
        text.raw_bytes.get(7..15) == Some(&u64::from(text.record_index).to_le_bytes());
    let indexed_index_matches = text.raw_bytes.get(7..11) == Some(&text.record_index.to_le_bytes())
        && text.raw_bytes.get(11..20) == Some(&[0; 9]);
    let header_matches = common_header_matches && (legacy_index_matches || indexed_index_matches);
    let fields_match = decoded.owner_reference == text.owner_reference
        && decoded.entity_genesis == text.entity_genesis
        && decoded.persistent_id == text.persistent_id
        && decoded.base_id == text.base_id
        && decoded.text == text.text
        && decoded.font_family == text.font_family
        && decoded.height == text.height
        && decoded.color == text.color
        && decoded.layout == text.layout;
    if !header_matches || !fields_match {
        return Err(CodecError::malformed(format_args!(
            "sketch-text record {} fields disagree with its raw bytes",
            text.id
        )));
    }
    out.extend_from_slice(&text.raw_bytes);
    Ok(())
}

/// Least sketch-relation record the Design `BulkStream` writer emits. A
/// relation record ends at its trailing zero byte, and the reader requires
/// every byte from there to the next indexed-record header to be zero, so the
/// filler carries no field. It is a fixed width of the writer's own and bounds
/// no value the IR states.
const SKETCH_RELATION_RECORD_FILLED_LEN: usize = 101;

fn encode_sketch_relation(
    out: &mut Vec<u8>,
    relation: &crate::records::sketch_relations::SketchRelation,
) -> Result<(), CodecError> {
    let mut record = vec![0u8; 19];
    encode_sketch_record_header(&mut record, &relation.class_tag, relation.record_index);
    record.push(1);
    let member_count = u32::try_from(relation.members().len()).map_err(|_| {
        CodecError::InvalidInput(
            "sketch relation members: the Design record states a u32 member count".into(),
        )
    })?;
    record.extend_from_slice(&member_count.to_le_bytes());
    for member in relation.members().iter() {
        write_reference(&mut record, member.reference.record_index());
        // The member's ordinal is a stated u32 in the record, and `0` is a
        // stated ordinal there, not an absent one. A member that retains none
        // has no bytes to write.
        let relation_ordinal = member.relation_ordinal.ok_or_else(|| {
            CodecError::InvalidInput(
                "sketch relation member relation_ordinal: the Design record states a u32 \
                 ordinal for every member, and this member retains none"
                    .into(),
            )
        })?;
        record.extend_from_slice(&relation_ordinal.to_le_bytes());
    }
    // The base level's property-block presence byte, then the block when the
    // relation carries an `EntityGenesis` origin.
    match relation.entity_genesis {
        Some(genesis) => {
            record.push(1);
            record.extend_from_slice(&1u32.to_le_bytes());
            native_lp_ascii(&mut record, "EntityGenesis")?;
            native_lp_ascii(&mut record, "IntrinsicMetaTypeuint64")?;
            record.extend_from_slice(&genesis.to_le_bytes());
        }
        None => record.push(0),
    }
    for reference in relation
        .auxiliary_references()
        .values()
        .chain(std::iter::once(&relation.owner_reference))
    {
        write_reference(&mut record, *reference);
    }
    record.extend_from_slice(&relation.definition.state().to_le_bytes());
    let return_count = u32::try_from(relation.return_members().len()).map_err(|_| {
        CodecError::InvalidInput(
            "sketch relation return_members: the Design record states a u32 return count".into(),
        )
    })?;
    record.extend_from_slice(&return_count.to_le_bytes());
    for member in relation.return_members().iter() {
        write_reference(&mut record, member.reference.record_index());
    }
    record.push(0);
    if record.len() < SKETCH_RELATION_RECORD_FILLED_LEN {
        zero_fill_to(&mut record, SKETCH_RELATION_RECORD_FILLED_LEN);
    }
    out.extend_from_slice(&record);
    Ok(())
}

/// Write one same-segment reference ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata) "**References.**"): the
/// presence byte, the u64 target entity id, and the two zero flag bytes.
fn write_reference(out: &mut Vec<u8>, target: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(target).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
}

fn construction_recipe_name(kind: ConstructionRecipeKind) -> &'static [u8] {
    match kind {
        ConstructionRecipeKind::Body => b"body_recipe_data",
        ConstructionRecipeKind::Face => b"face_recipe_data",
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data",
        ConstructionRecipeKind::Edge => b"edge_recipe_data",
        ConstructionRecipeKind::Vertex => b"vertex_recipe_data",
    }
}

fn persistent_reference_name(kind: PersistentReferenceKind) -> &'static [u8] {
    match kind {
        PersistentReferenceKind::Point => b"pt_tag",
        PersistentReferenceKind::CurvePrimary => b"crv_primary_id",
        PersistentReferenceKind::CurveSecondary => b"crv_secondary_id",
    }
}

pub(super) fn encode_design_metastream(
    registry: &GeneratedDesignRegistry,
    primary_records: &[crate::metastream::RecordIndexEntry],
) -> Result<Option<Vec<u8>>, CodecError> {
    if registry.types.is_empty() {
        return Ok(None);
    }

    // A generated segment writes the modern header shape, the type table, and
    // the primary offsets captured while writing its sibling BulkStream.
    let mut out = Vec::new();
    native_lp_ascii(&mut out, "Design")?;
    out.extend_from_slice(&0u32.to_le_bytes());
    native_lp_utf16(&mut out, GENERATED_ASSET_GUID)?;
    out.extend_from_slice(&1234u32.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    native_lp_ascii(&mut out, "FusionDesignSegmentType")?;
    native_lp_ascii(&mut out, "Fusion")?;
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let type_count = u32::try_from(registry.types.len()).map_err(|_| {
        CodecError::NotImplemented("Design MetaStream registers more than u32::MAX types".into())
    })?;
    out.extend_from_slice(&type_count.to_le_bytes());
    let mut next_entity_id = 1u64;
    for design_type in &registry.types {
        native_lp_ascii(&mut out, design_type.type_guid.as_str())?;
        match &design_type.base_type_guid {
            Some(base) => {
                native_lp_ascii(&mut out, base.as_str())?;
            }
            None => out.extend_from_slice(&0u32.to_le_bytes()),
        }
        out.extend_from_slice(&design_type.version.to_le_bytes());
        if crate::bytes::is_guid_relaxed(&design_type.module) {
            return Err(CodecError::NotImplemented(format!(
                "Design type module name is GUID-shaped: {}",
                design_type.module
            )));
        }
        native_lp_ascii(&mut out, &design_type.module)?;
        let count = u32::try_from(design_type.entity_ids.len()).map_err(|_| {
            CodecError::NotImplemented("Design type owns more than u32::MAX entities".into())
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for entity_id in &design_type.entity_ids {
            out.extend_from_slice(&entity_id.to_le_bytes());
            let next = entity_id.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("generated Design next entity id overflows".into())
            })?;
            if next > next_entity_id {
                next_entity_id = next;
            }
        }
    }
    // Named-entity list, primary record index, and secondary index.
    out.extend_from_slice(&0u32.to_le_bytes());
    let record_count = u32::try_from(primary_records.len()).map_err(|_| {
        CodecError::NotImplemented("Design primary index exceeds u32::MAX records".into())
    })?;
    out.extend_from_slice(&record_count.to_le_bytes());
    let registered_entities = registry
        .types
        .iter()
        .flat_map(|design_type| design_type.entity_ids.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    let mut indexed_entities = std::collections::BTreeSet::new();
    let mut previous_offset = None;
    for record in primary_records {
        if !registered_entities.contains(&record.entity_id) {
            return Err(CodecError::malformed(format_args!(
                "generated Design primary entity {} has no type registration",
                record.entity_id
            )));
        }
        if !indexed_entities.insert(record.entity_id) {
            return Err(CodecError::malformed(format_args!(
                "generated Design primary entity {} is indexed more than once",
                record.entity_id
            )));
        }
        if previous_offset.is_some_and(|previous| previous >= record.bulk_offset) {
            return Err(CodecError::Malformed(
                "generated Design primary offsets are not strictly increasing".into(),
            ));
        }
        previous_offset = Some(record.bulk_offset);
        out.extend_from_slice(&record.entity_id.to_le_bytes());
        out.extend_from_slice(&record.bulk_offset.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&next_entity_id.to_le_bytes());
    // Trailing flag and empty property block.
    out.extend_from_slice(&[0; 8]);
    Ok(Some(out))
}

fn encode_browser_nodes(
    out: &mut Vec<u8>,
    primary_records: &mut Vec<crate::metastream::RecordIndexEntry>,
    registry: &GeneratedDesignRegistry,
) -> Result<(), CodecError> {
    let Some(browser_nodes) = &registry.browser_nodes else {
        return Ok(());
    };
    let node_class_tag = browser_nodes.class_tag.as_str();
    for node in &browser_nodes.nodes {
        primary_records.push(primary_record(node.record_index, out.len())?);
        native_lp_ascii(out, node_class_tag)?;
        out.extend_from_slice(&node.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        native_lp_utf16(out, &node.node_guid)?;
        out.push(u8::from(!node.visible));
        out.extend_from_slice(&[0x01, 0x01]);
        out.extend_from_slice(&node.entity_suffix.to_le_bytes());
    }
    Ok(())
}

/// Asset GUID written into a generated Design `MetaStream`, which has no source
/// asset to name.
const GENERATED_ASSET_GUID: &str = "00000000-0000-0000-0000-000000000000";

fn native_lp_ascii(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(CodecError::NotImplemented(
            "Design MetaStream strings must contain printable ASCII".into(),
        ));
    }
    let length = u32::try_from(value.len()).map_err(|_| {
        CodecError::NotImplemented("Design MetaStream string exceeds u32::MAX".into())
    })?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn native_lp_utf16(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    let length = u32::try_from(units.len())
        .map_err(|_| CodecError::NotImplemented("Design UTF-16 string exceeds u32::MAX".into()))?;
    out.extend_from_slice(&length.to_le_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Hand-built relations assert the written bytes; the drafts are valid by construction.
    #![allow(clippy::unwrap_used)]
    use super::{encode_sketch_relation, SKETCH_RELATION_RECORD_FILLED_LEN};
    use crate::records::{
        identity::ReferenceRun,
        references::DesignClassTag,
        sketch_relations::{
            SketchRelation, SketchRelationDefinition, SketchRelationDraft, SketchRelationMember,
            SketchRelationReturnMember,
        },
    };

    fn registry_with_entity(entity_id: u64) -> super::GeneratedDesignRegistry {
        super::GeneratedDesignRegistry {
            types: vec![super::super::presentation::GeneratedDesignType {
                type_guid: "11111111-2222-3333-4444-555555555555"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                base_type_guid: None,
                version: 1,
                module: "Fusion".into(),
                entity_ids: vec![entity_id],
            }],
            body_map: None,
            browser_nodes: None,
        }
    }

    #[test]
    fn metastream_rejects_overflowed_next_entity_id() {
        let error = super::encode_design_metastream(&registry_with_entity(u64::MAX), &[])
            .expect_err("the next identity cannot be represented");
        assert!(
            matches!(error, cadmpeg_core::CodecError::Malformed(ref message)
            if message == "generated Design next entity id overflows")
        );
    }

    #[test]
    fn metastream_preserves_largest_representable_next_entity_id() {
        let bytes = super::encode_design_metastream(&registry_with_entity(u64::MAX - 1), &[])
            .unwrap()
            .unwrap();
        let next_offset = bytes.len() - 16;
        assert_eq!(
            cadmpeg_core::decode::View::u64_le_at(&bytes, next_offset),
            Some(u64::MAX)
        );
    }

    /// One member whose ordinal the wire states, which is what the writer
    /// needs: `SketchRelationMember::from_index` retains none.
    fn stated_member(record_index: u32) -> SketchRelationMember {
        SketchRelationMember {
            relation_ordinal: Some(0),
            ..SketchRelationMember::from_index(record_index)
        }
    }

    fn relation(member_count: u32) -> SketchRelation {
        let members = (1..=member_count).collect::<Vec<_>>();
        SketchRelation::try_new(SketchRelationDraft {
            id: "f3d:native:sketch-relation#filler".into(),
            record_index: 10,
            class_tag: DesignClassTag::try_from("300".to_owned()).unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: None,
            owner_reference_offset: 0,
            auxiliary_references: ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: members
                .iter()
                .copied()
                .map(stated_member)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            definition: SketchRelationDefinition::new(1, None).unwrap(),
            entity_genesis: None,
            return_members: members
                .iter()
                .copied()
                .map(SketchRelationReturnMember::from_index)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            raw_bytes: vec![0; 160],
        })
        .unwrap()
    }

    fn encoded(member_count: u32) -> Vec<u8> {
        let mut out = Vec::new();
        encode_sketch_relation(&mut out, &relation(member_count)).unwrap();
        out
    }

    #[test]
    fn the_relation_record_filler_is_a_writer_width_and_never_bounds_a_stated_count() {
        // A short relation is filled to the writer's own width with the zero
        // bytes the reader requires between the record's trailing zero and the
        // next indexed-record header.
        let short = encoded(1);
        assert_eq!(short.len(), SKETCH_RELATION_RECORD_FILLED_LEN);

        // Every byte the relation itself does not state is zero: take the same
        // record one member longer and the shared prefix is identical up to
        // the point the counts differ, with zeros beyond each content end.
        let content_end = short
            .iter()
            .rposition(|byte| *byte != 0)
            .expect("the record states nonzero bytes")
            + 1;
        assert!(content_end < SKETCH_RELATION_RECORD_FILLED_LEN);
        assert!(short[content_end..].iter().all(|byte| *byte == 0));

        // The stated member count reaches the bytes unfloored, and a relation
        // whose own content passes the filler width is written whole: the
        // width is a floor on the record, never a ceiling on a stated count.
        assert_eq!(
            u32::from_le_bytes(short[20..24].try_into().unwrap()),
            1,
            "the member count the relation states"
        );
        let long = encoded(6);
        assert!(long.len() > SKETCH_RELATION_RECORD_FILLED_LEN);
        assert_eq!(u32::from_le_bytes(long[20..24].try_into().unwrap()), 6);
        let longer = encoded(7);
        assert!(longer.len() > long.len());
        assert_eq!(u32::from_le_bytes(longer[20..24].try_into().unwrap()), 7);
    }
}

#[cfg(test)]
mod relation_ordinal_tests {
    #![allow(clippy::unwrap_used)]
    use super::encode_sketch_relation;
    use crate::records::{
        identity::ReferenceRun,
        references::DesignClassTag,
        sketch_relations::{
            SketchRelation, SketchRelationDefinition, SketchRelationDraft, SketchRelationMember,
            SketchRelationReturnMember,
        },
    };

    /// The record states a u32 ordinal for every member, so a member that
    /// retains none has no bytes to write. Writing `0` there would state the
    /// ordinal `0`, which the reader reads back as a stated ordinal.
    #[test]
    fn a_member_with_no_retained_ordinal_is_refused_by_name() {
        let draft = |relation_ordinal: Option<u32>| SketchRelationDraft {
            id: "f3d:native:sketch-relation#ordinal".into(),
            record_index: 10,
            class_tag: DesignClassTag::try_from("300".to_owned()).unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: None,
            owner_reference_offset: 0,
            auxiliary_references: ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: vec![SketchRelationMember {
                relation_ordinal,
                ..SketchRelationMember::from_index(1)
            }]
            .try_into()
            .unwrap(),
            definition: SketchRelationDefinition::new(1, None).unwrap(),
            entity_genesis: None,
            return_members: vec![SketchRelationReturnMember::from_index(1)]
                .try_into()
                .unwrap(),
            raw_bytes: vec![0; 160],
        };

        let mut out = Vec::new();
        encode_sketch_relation(&mut out, &SketchRelation::try_new(draft(Some(0))).unwrap())
            .expect("a stated ordinal writes");

        let mut out = Vec::new();
        let error =
            encode_sketch_relation(&mut out, &SketchRelation::try_new(draft(None)).unwrap())
                .expect_err("a member that retains no ordinal has no bytes to write");
        assert!(
            error.to_string().contains("relation_ordinal"),
            "the refusal names the field: {error}"
        );
    }
}
