// SPDX-License-Identifier: Apache-2.0
//! Record patchers that apply validated edit sets to archive bytes.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use std::collections::{BTreeMap, BTreeSet};

use crate::records::{
    act::{ActComponentLink, ActEntity},
    references::{DesignMaterialAssignment, LostEdgeReference},
    sketch_geometry::SketchCurveGeometry,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use super::edits::{
    BodyMemberEdit, ByteEdit, ConstructionRecipeEdit, DesignTypeEdit, EntityHeaderEdit,
    HistoryEdits, PersistentReferenceEdit, SketchCurveEdit, SketchPointEdit,
};
use cadmpeg_asm::edit::AsmEditSet;
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;

const EPS_RECORDS_LINE_SCALAR_COUNT_E9: f64 = 1.0e-9;

pub(super) fn patch_material_assignments(
    bytes: &mut [u8],
    edits: &[DesignMaterialAssignment],
) -> Result<(), CodecError> {
    for assignment in edits {
        let suffix_start = usize::try_from(assignment.entity_suffix_offset).map_err(|_| {
            CodecError::Malformed("material-assignment suffix offset exceeds address space".into())
        })?;
        bytes
            .get_mut(suffix_start..suffix_start + 8)
            .ok_or_else(|| CodecError::Malformed("material-assignment suffix is truncated".into()))?
            .copy_from_slice(&assignment.entity_id.suffix().to_le_bytes());
        patch_utf16_if_changed(
            bytes,
            assignment.entity_id_offset,
            assignment.entity_id.as_str(),
            "material-assignment entity id",
        )?;
        patch_utf16_if_changed(
            bytes,
            assignment.visual_guid_offset,
            &assignment.visual_guid,
            "material-assignment visual token",
        )?;
        if let Some(field) = &assignment.physical_token {
            patch_utf16_if_changed(
                bytes,
                field.offset,
                &field.value,
                "material-assignment physical token",
            )?;
        }
        if let Some(field) = &assignment.visual_preset {
            patch_utf16_if_changed(
                bytes,
                field.offset,
                &field.value,
                "material-assignment visual preset",
            )?;
        }
    }
    Ok(())
}

pub(super) fn patch_lost_edge_references(
    bytes: &mut [u8],
    edits: &[LostEdgeReference],
) -> Result<(), CodecError> {
    for reference in edits {
        patch_bytes_at(
            bytes,
            reference.class_tag_offset(),
            reference.class_tag.as_bytes(),
            "lost-edge class tag",
        )?;
        patch_u32_at(
            bytes,
            reference.record_index_offset(),
            reference.record_index,
            "lost-edge record index",
        )?;
    }
    Ok(())
}

pub(super) fn patch_act_entities(bytes: &mut [u8], edits: &[ActEntity]) -> Result<(), CodecError> {
    for entity in edits {
        let encoded_id = entity
            .entity_id()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        for offset in [
            entity.table_entity_id_offset(),
            entity.channel_entity_id_offset(),
        ]
        .into_iter()
        .flatten()
        {
            patch_bytes_at(bytes, offset, &encoded_id, "ACT entity id")?;
        }
        for guid in entity.channel_group().channels().values() {
            let encoded = guid
                .value
                .as_str()
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            patch_bytes_at(bytes, guid.offset, &encoded, "ACT channel GUID")?;
        }
    }
    Ok(())
}

pub(super) fn patch_act_guids(bytes: &mut [u8], edits: &[ByteEdit]) -> Result<(), CodecError> {
    for edit in edits {
        patch_bytes_at(bytes, edit.offset, &edit.value, "ACT GUID")?;
    }
    Ok(())
}

pub(super) fn patch_act_links(
    bytes: &mut [u8],
    edits: &[ActComponentLink],
) -> Result<(), CodecError> {
    for root in edits {
        for (offset, value, field) in [
            (
                root.layout().instance_root_record_offset(),
                root.references.instance_root,
                "ACT instance-root reference",
            ),
            (
                root.layout().tracked_entity_record_offset(),
                root.references.tracked_entity,
                "ACT tracked-entity reference",
            ),
            (
                root.layout().components_root_record_offset(),
                root.references.components_root,
                "ACT components-root reference",
            ),
            (
                root.layout().registry_flag_offset(),
                root.registry_flag.code(),
                "ACT registry flag",
            ),
        ] {
            patch_u32_at(bytes, offset, value, field)?;
        }
        patch_utf16_if_changed(
            bytes,
            root.layout().entity_id_offset(),
            root.layout().entity_id(),
            "ACT component-link entity id",
        )?;
        patch_utf16_if_changed(
            bytes,
            root.layout().display_name_offset(),
            root.layout().display_name(),
            "ACT component-link display name",
        )?;
    }
    Ok(())
}

fn patch_utf16_if_changed(
    bytes: &mut [u8],
    offset: u64,
    value: &str,
    field: &str,
) -> Result<(), CodecError> {
    let encoded = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    patch_bytes_at(bytes, offset, &encoded, field)
}

pub(crate) fn native_stream(id: &str, delimiter: &str) -> Result<String, CodecError> {
    let Some((stream, _)) = id
        .strip_prefix(crate::ids::SCHEME_PREFIX)
        .and_then(|id| id.rsplit_once(delimiter))
    else {
        return Err(CodecError::malformed(format_args!(
            "invalid native record id {id}"
        )));
    };
    crate::ids::decode_identity_key_component(stream)
        .ok_or_else(|| CodecError::malformed(format_args!("invalid native record id {id}")))
}

fn patch_bytes_at(
    bytes: &mut [u8],
    offset: u64,
    encoded: &[u8],
    field: &str,
) -> Result<(), CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::malformed(format_args!("{field} offset exceeds address space")))?;
    bytes
        .get_mut(start..start + encoded.len())
        .ok_or_else(|| CodecError::malformed(format_args!("{field} is truncated")))?
        .copy_from_slice(encoded);
    Ok(())
}

pub(super) fn patch_design_types(
    bytes: &mut [u8],
    edits: &[DesignTypeEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        for (offset, encoded) in edit.integers.iter().chain(&edit.strings) {
            let start = usize::try_from(*offset).map_err(|_| {
                CodecError::Malformed("design-type offset exceeds address space".into())
            })?;
            bytes
                .get_mut(start..start + encoded.len())
                .ok_or_else(|| CodecError::Malformed("design-type field is truncated".into()))?
                .copy_from_slice(encoded);
        }
    }
    Ok(())
}

pub(super) fn patch_entity_headers(
    bytes: &mut [u8],
    edits: &[EntityHeaderEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        if let Some(reference) = &edit.record_reference {
            patch_u32_at(
                bytes,
                reference.offset,
                reference.value,
                "entity-header record reference",
            )?;
        }
        for reference in &edit.references {
            patch_u32_at(
                bytes,
                reference.offset,
                reference.value,
                "entity-header child reference",
            )?;
        }
    }
    Ok(())
}

fn patch_u32_at(bytes: &mut [u8], offset: u64, value: u32, field: &str) -> Result<(), CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::malformed(format_args!("{field} offset exceeds address space")))?;
    bytes
        .get_mut(start..start + 4)
        .ok_or_else(|| CodecError::malformed(format_args!("{field} is truncated")))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

pub(super) fn patch_body_members(
    bytes: &mut [u8],
    edits: &[BodyMemberEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        let offset = edit.offset;
        let entity_suffix = edit.entity_suffix;
        let flags = edit.flags;
        let start = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("design-body-member offset exceeds address space".into())
        })?;
        if bytes.get(start) != Some(&1) {
            return Err(CodecError::malformed(format_args!(
                "design-body-member at byte {start} has no presence marker"
            )));
        }
        bytes
            .get_mut(start + 1..start + 9)
            .ok_or_else(|| CodecError::Malformed("design-body-member id is truncated".into()))?
            .copy_from_slice(&entity_suffix.to_le_bytes());
        bytes
            .get_mut(start + 9..start + 11)
            .ok_or_else(|| CodecError::Malformed("design-body-member flags are truncated".into()))?
            .copy_from_slice(&flags.to_le_bytes());
    }
    Ok(())
}

pub(super) fn patch_body_visibilities(
    bytes: &mut [u8],
    edits: &[(u64, bool)],
) -> Result<(), CodecError> {
    for &(offset, visible) in edits {
        let at = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("body-visibility offset exceeds address space".into())
        })?;
        let flag = bytes
            .get_mut(at)
            .filter(|flag| **flag <= 1)
            .ok_or_else(|| {
                CodecError::Malformed("body-visibility flag is missing or invalid".into())
            })?;
        *flag = u8::from(!visible);
    }
    Ok(())
}

pub(super) fn patch_design_body_keys(
    bytes: &mut [u8],
    edits: &BTreeSet<(u64, u64)>,
) -> Result<(), CodecError> {
    for &(offset, key) in edits {
        let at = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("Design body-key offset exceeds address space".into())
        })?;
        bytes
            .get_mut(at..at + 8)
            .ok_or_else(|| CodecError::Malformed("Design body-map key is truncated".into()))?
            .copy_from_slice(&key.to_le_bytes());
    }
    Ok(())
}

pub(super) fn patch_body_native_keys(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, i64>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    AsmEditSet::apply(bytes, |bytes, asm_edits| {
        for (record_index, key) in edits {
            let record = asm_edits.record(*record_index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D body-key record {record_index} is missing"
                ))
            })?;
            if record.head() != "body" {
                return Err(CodecError::malformed(format_args!(
                    "F3D body-key record {record_index} is not a body"
                )));
            }
            asm_edits.patch_integer_field(bytes, record, 1, 0x04, *key)?;
        }
        Ok(())
    })
}

pub(super) fn patch_transform_hints(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, [bool; 3]>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    AsmEditSet::apply(bytes, |bytes, asm_edits| {
        for (record_index, flags) in edits {
            let record = asm_edits.record(*record_index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D transform-hint record {record_index} is missing"
                ))
            })?;
            if !record.name.ends_with("transform") {
                return Err(CodecError::malformed(format_args!(
                    "F3D transform-hint record {record_index} is {}, not a transform",
                    record.head()
                )));
            }
            for (index, flag) in (5usize..=7).zip(flags) {
                asm_edits.patch_boolean_field(bytes, record, index, *flag)?;
            }
        }
        Ok(())
    })
}

pub(super) fn patch_tolerant_coedge_parameters(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, [f64; 2]>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    AsmEditSet::apply(bytes, |bytes, asm_edits| {
        for (record_index, range) in edits {
            let record = asm_edits.record(*record_index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D tolerant-coedge record {record_index} is missing"
                ))
            })?;
            if record.head() != "tcoedge" {
                return Err(CodecError::malformed(format_args!(
                    "F3D tolerant-coedge record {record_index} is {}",
                    record.head()
                )));
            }
            for (index, value) in [(11usize, range[0]), (12, range[1])] {
                let offset = asm_edits.required_payload_field(bytes, record, index, 0x06)?;
                AsmEditSet::patch_f64_payload(bytes, offset + 1, value)?;
            }
        }
        Ok(())
    })
}

pub(super) fn patch_wire_topologies(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, cadmpeg_asm::brep::records::WireSide>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    AsmEditSet::apply(bytes, |bytes, asm_edits| {
        for (record_index, side) in edits {
            let record = asm_edits.record(*record_index).ok_or_else(|| {
                CodecError::malformed(format_args!("F3D wire record {record_index} is missing"))
            })?;
            if record.head() != "wire" {
                return Err(CodecError::malformed(format_args!(
                    "F3D wire record {record_index} is {}",
                    record.head()
                )));
            }
            let is_in = matches!(side, cadmpeg_asm::brep::records::WireSide::In);
            asm_edits.patch_boolean_field(bytes, record, 7, is_in)?;
        }
        Ok(())
    })
}

pub(super) fn patch_edge_ownerships(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, i64>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    AsmEditSet::apply(bytes, |bytes, asm_edits| {
        for (record_index, owner) in edits {
            let record = asm_edits.record(*record_index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D edge-ownership record {record_index} is missing"
                ))
            })?;
            if !matches!(record.head(), "edge" | "tedge") {
                return Err(CodecError::malformed(format_args!(
                    "F3D edge-ownership record {record_index} is {}",
                    record.head()
                )));
            }
            asm_edits.patch_integer_field(bytes, record, 7, 0x0c, *owner)?;
        }
        Ok(())
    })
}

pub(super) fn patch_construction_recipes(
    bytes: &mut [u8],
    edits: &[ConstructionRecipeEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        if let Some(record_index) = &edit.record_index {
            let offset = record_index.offset;
            let record_index = record_index.value;
            let start = usize::try_from(offset).map_err(|_| {
                CodecError::Malformed("construction-recipe offset exceeds address space".into())
            })?;
            bytes
                .get_mut(start..start + 4)
                .ok_or_else(|| {
                    CodecError::Malformed("construction-recipe record index is truncated".into())
                })?
                .copy_from_slice(&record_index.to_le_bytes());
        }
        if let Some(design_id) = &edit.design_id {
            let offset = design_id.offset;
            let encoded = &design_id.value;
            let start = usize::try_from(offset).map_err(|_| {
                CodecError::Malformed(
                    "construction-recipe design-id offset exceeds address space".into(),
                )
            })?;
            bytes
                .get_mut(start..start + encoded.len())
                .ok_or_else(|| {
                    CodecError::Malformed("construction-recipe design id is truncated".into())
                })?
                .copy_from_slice(encoded);
        }
    }
    Ok(())
}

pub(super) fn patch_persistent_references(
    bytes: &mut [u8],
    edits: &[PersistentReferenceEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        let record_offset = edit.offset;
        let value_offset = edit.identity_offset;
        let value = edit.identity;
        let start = usize::try_from(record_offset)
            .ok()
            .and_then(|offset| offset.checked_add(index_from_u32(value_offset)))
            .ok_or_else(|| {
                CodecError::Malformed("persistent-reference offset exceeds address space".into())
            })?;
        bytes
            .get_mut(start..start + 8)
            .ok_or_else(|| CodecError::Malformed("persistent-reference value is truncated".into()))?
            .copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

pub(super) fn patch_history_states(
    bytes: &mut [u8],
    edits: &HistoryEdits,
) -> Result<(), CodecError> {
    const DELTA_HEADER_LEN: usize = b"\x11\x0d\x0bdelta_state".len();
    const PREAMBLE_LEN: usize = b"\x0d\x0ehistory_stream".len();
    if let Some(history) = &edits.preamble {
        let start = history
            .byte_offset
            .checked_add(u64_from_index(PREAMBLE_LEN))
            .ok_or_else(|| {
                CodecError::Malformed("ASM preamble offset exceeds address space".into())
            })?;
        let size = history.stream_size;
        let entry_count = history.history_entry_count;
        for (ordinal, value) in [(0, size), (1, size), (3, entry_count)] {
            AsmEditSet::patch_tagged_i64(bytes, start, ordinal, 0x04, value)?;
        }
    }
    for state in &edits.states {
        let first_tag = state
            .byte_offset
            .checked_add(u64_from_index(DELTA_HEADER_LEN))
            .ok_or_else(|| {
                CodecError::Malformed("ASM history offset exceeds address space".into())
            })?;
        let values = [
            (0, 0x04, state.state_id),
            (1, 0x04, state.version_flag),
            (2, 0x04, state.state_flag),
            (3, 0x0c, state.previous_ref.unwrap_or(-1)),
            (4, 0x0c, state.next_ref.unwrap_or(-1)),
            (5, 0x0c, state.node_index),
            (6, 0x0c, state.partner_ref.unwrap_or(-1)),
            (7, 0x0c, state.owner_ref),
        ];
        for (ordinal, expected_tag, value) in values {
            AsmEditSet::patch_tagged_i64(bytes, first_tag, ordinal, expected_tag, value)?;
        }
    }
    for board in &edits.boards {
        AsmEditSet::patch_tagged_i64(bytes, board.byte_offset, 1, 0x0c, board.owner_ref)?;
        AsmEditSet::patch_tagged_i64(bytes, board.byte_offset, 2, 0x04, board.number)?;
    }
    for change in &edits.changes {
        AsmEditSet::patch_tagged_i64(
            bytes,
            change.byte_offset,
            1,
            0x0c,
            change.old_ref().unwrap_or(-1),
        )?;
        AsmEditSet::patch_tagged_i64(
            bytes,
            change.byte_offset,
            2,
            0x0c,
            change.new_ref().unwrap_or(-1),
        )?;
    }
    Ok(())
}

pub(super) fn patch_sketch_points(
    bytes: &mut [u8],
    edits: &[SketchPointEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        let record_offset = edit.offset;
        let coordinate_offset = edit.coordinate_offset;
        let coordinates = &edit.coordinates;
        let start = usize::try_from(record_offset)
            .ok()
            .and_then(|record| record.checked_add(index_from_u32(coordinate_offset)))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-point offset exceeds address space".into())
            })?;
        let payload = bytes.get_mut(start..start + 16).ok_or_else(|| {
            CodecError::Malformed("sketch-point coordinate payload is outside BulkStream".into())
        })?;
        payload[..8].copy_from_slice(&(coordinates.u / LEN_TO_MM).to_le_bytes());
        payload[8..].copy_from_slice(&(coordinates.v / LEN_TO_MM).to_le_bytes());
    }
    Ok(())
}

pub(crate) fn patch_sketch_curves(
    bytes: &mut [u8],
    edits: &[SketchCurveEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        let record_offset = edit.offset;
        let geometry_offset = edit.geometry_offset;
        let geometry = &edit.geometry;
        let start = usize::try_from(record_offset)
            .ok()
            .and_then(|record| record.checked_add(index_from_u32(geometry_offset)))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-curve offset exceeds address space".into())
            })?;
        let (values, scalar_count) = match geometry {
            SketchCurveGeometry::Line {
                start: line_start,
                end,
                direction,
                normal,
            } => {
                let line_start = line_start.get();
                let end = end.get();
                let direction = direction.as_raw();
                let normal = normal.as_raw();
                let scalar_count = line_scalar_count(bytes, start)?;
                if scalar_count == 9 && (normal.x != 0.0 || normal.y != 0.0 || normal.z != 1.0) {
                    return Err(CodecError::NotImplemented(
                        "F3D compact planar-line edits require the implicit +Z normal".into(),
                    ));
                }
                (
                    [
                        line_start.x / LEN_TO_MM,
                        line_start.y / LEN_TO_MM,
                        line_start.z / LEN_TO_MM,
                        (end.x - line_start.x) / LEN_TO_MM,
                        (end.y - line_start.y) / LEN_TO_MM,
                        (end.z - line_start.z) / LEN_TO_MM,
                        direction.x,
                        direction.y,
                        direction.z,
                        normal.x,
                        normal.y,
                        normal.z,
                    ],
                    scalar_count,
                )
            }
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => {
                let center = center.as_raw();
                let normal = normal.as_raw();
                let reference_direction = reference_direction.as_raw();
                (
                    [
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
                    ],
                    12,
                )
            }
            SketchCurveGeometry::Nurbs { geometry, .. } => {
                let knots = geometry.knots();
                patch_sketch_nurbs(
                    bytes,
                    start,
                    geometry.fit_tolerance().get(),
                    knots,
                    geometry.poles(),
                )?;
                continue;
            }
        };
        let payload = bytes
            .get_mut(start..start + scalar_count * 8)
            .ok_or_else(|| {
                CodecError::Malformed("sketch-curve analytic payload is outside BulkStream".into())
            })?;
        for (ordinal, value) in values.into_iter().take(scalar_count).enumerate() {
            payload[ordinal * 8..ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

fn line_scalar_count(bytes: &[u8], values_at: usize) -> Result<usize, CodecError> {
    let Some(marker_at) = values_at.checked_add(9 * 8) else {
        return Err(CodecError::Malformed(
            "sketch-line scalar offset exceeds address space".into(),
        ));
    };
    let mut view = View::over_retained(bytes);
    let full_normal = view
        .seek(marker_at)
        .and_then(|()| Some([view.f64_le()?, view.f64_le()?, view.f64_le()?]));
    if let Some(normal) = full_normal {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if normal.iter().all(|value| value.is_finite())
            && (length - 1.0).abs() <= EPS_RECORDS_LINE_SCALAR_COUNT_E9
        {
            return Ok(12);
        }
    }
    if bytes.get(marker_at) == Some(&1) && bytes.get(marker_at + 5..marker_at + 11) == Some(&[0; 6])
    {
        return Ok(9);
    }
    Err(CodecError::Malformed(
        "sketch-line payload matches neither full nor compact planar layout".into(),
    ))
}

fn patch_sketch_nurbs(
    bytes: &mut [u8],
    start: usize,
    fit_tolerance: f64,
    knots: &[f64],
    poles: &crate::records::sketch_geometry::SketchNurbsPoles,
) -> Result<(), CodecError> {
    let fit_at = start + 94;
    let knots_at = start + 114;
    let weights_header = knots_at + knots.len() * 8;
    let weights_at = weights_header + 12;
    let points_header = weights_at + poles.weights().len() * 8;
    let points_at = points_header + 12;
    let end = points_at + poles.point_count() * 24;
    if end > bytes.len() {
        return Err(CodecError::Malformed(
            "sketch NURBS arrays extend beyond BulkStream".into(),
        ));
    }
    bytes[fit_at..fit_at + 8].copy_from_slice(&(fit_tolerance / LEN_TO_MM).to_le_bytes());
    for (ordinal, value) in knots.iter().enumerate() {
        let at = knots_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, value) in poles.weights().enumerate() {
        let at = weights_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, point) in poles.points().enumerate() {
        let at = points_at + ordinal * 24;
        for (component, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            let component_at = at + component * 8;
            bytes[component_at..component_at + 8]
                .copy_from_slice(&(value / LEN_TO_MM).to_le_bytes());
        }
    }
    Ok(())
}

pub(super) fn patch_sketch_relations(
    bytes: &mut [u8],
    edits: &[Vec<ByteEdit>],
) -> Result<(), CodecError> {
    for edit in edits {
        for member in edit {
            patch_bytes_at(bytes, member.offset, &member.value, "sketch-relation value")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::patch_material_assignments;
    use crate::records::references::DesignMaterialAssignment;

    const SUFFIX_AT: usize = 0;
    const ENTITY_ID_AT: usize = 8;
    const VISUAL_GUID_AT: usize = 18;
    const PHYSICAL_TOKEN_AT: usize = 90;

    fn utf16(value: &str) -> Vec<u8> {
        value.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn assignment_document(located: bool) -> serde_json::Value {
        let mut document = serde_json::json!({
            "id": "material#0",
            "asm_body_key": 42,
            "asm_body_key_offset": 200,
            "entity_suffix_offset": SUFFIX_AT,
            "entity_id": "0_985",
            "entity_id_offset": ENTITY_ID_AT,
            "visual_guid": "11111111-2222-3333-4444-555555555555",
            "visual_guid_offset": VISUAL_GUID_AT,
            "physical_token": "Prism-002"
        });
        if located {
            document["physical_token_offset"] = PHYSICAL_TOKEN_AT.into();
        }
        document
    }

    #[test]
    fn every_admitted_material_physical_token_is_patched_into_the_source_bytes() {
        let error = serde_json::from_value::<DesignMaterialAssignment>(assignment_document(false))
            .expect_err("a token the patch cannot write is not admitted")
            .to_string();
        assert!(error.contains("physical_token"), "{error}");
        assert!(error.contains("physical_token_offset"), "{error}");

        let assignment: DesignMaterialAssignment =
            serde_json::from_value(assignment_document(true)).expect("located material token");
        let mut bytes = vec![0u8; 128];
        patch_material_assignments(&mut bytes, std::slice::from_ref(&assignment))
            .expect("patch material assignment");
        let token = utf16("Prism-002");
        assert_eq!(
            &bytes[PHYSICAL_TOKEN_AT..PHYSICAL_TOKEN_AT + token.len()],
            &token[..]
        );
        assert_eq!(&bytes[ENTITY_ID_AT..VISUAL_GUID_AT], &utf16("0_985")[..]);
        assert_eq!(&bytes[SUFFIX_AT..SUFFIX_AT + 8], &985u64.to_le_bytes()[..]);
    }
}
