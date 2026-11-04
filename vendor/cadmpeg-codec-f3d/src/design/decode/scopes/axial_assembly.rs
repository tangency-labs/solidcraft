// SPDX-License-Identifier: Apache-2.0
//! Bind axial assembly operand targets and joint-origin frames from assemblies.

use super::combine::take_external_reference_identity;
use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use super::work_geometry::ScopePlacementFrame;
use crate::bytes::take_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::layout::assembly_axial_construction_carrier as axial_carrier;
use crate::layout::assembly_axial_role_prefix as axial_role;
use crate::layout::assembly_axial_selector_prefix as axial_selector;
use crate::records::feature::assembly;
use crate::records::feature::assembly::DesignAssemblyAxialOperandTarget;
use crate::records::feature::assembly::DesignAssemblyAxialSelectorIdentity;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandQualifier;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

pub(super) fn bind_joint_origin_frames_from_assemblies(
    bytes: &[u8],
    scopes: &mut [DesignParameterScope],
) {
    let mut candidates = Vec::new();
    let mut envelopes = Vec::new();
    for scope in scopes.iter() {
        if scope.kind() != scope::DesignFeatureKind::Assemble {
            continue;
        }
        if let Some(frames) = scope
            .assembly_alignment()
            .and_then(assembly::DesignAssemblyAlignment::operand_frames)
        {
            for frame in frames {
                candidates.push((
                    frame.reference_record_index,
                    frame.transform,
                    frame.transform_offset,
                    None,
                ));
            }
        }
        if let Some((joint_origin, frame)) = exact_single_joint_origin_frame(bytes, scope) {
            envelopes.push((scope.record_index, joint_origin, frame.transform));

            candidates.push((
                joint_origin,
                frame.transform,
                frame.transform_offset,
                frame.reference,
            ));
        }
    }
    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == scope::DesignFeatureKind::JointOrigin
            && scope.joint_origin_frame().is_none()
    }) {
        let mut matches = candidates
            .iter()
            .filter(|(record_index, ..)| *record_index == scope.record_index);
        let Some((_, transform, transform_offset, reference)) = matches.next() else {
            continue;
        };
        if matches.any(|(_, other_transform, _, other_reference)| {
            other_transform != transform
                || other_reference.map(|(record_index, _)| record_index)
                    != reference.map(|(record_index, _)| record_index)
        }) {
            continue;
        }
        {
            let construction = Some(scope::DesignJointOriginTransform {
                joint_origin_transform: *transform,
                joint_origin_transform_offset: *transform_offset,
                reference: reference.map(|(record_index, offset)| {
                    scope::DesignJointOriginReference {
                        joint_origin_reference: record_index,
                        joint_origin_reference_offset: offset,
                    }
                }),
            });
            if let scope::DesignScopePayloadMut::JointOrigin(slot) = scope.payload_mut() {
                *slot = construction;
            }
        }
    }
    let mut resolved_origins = HashMap::new();
    for scope in scopes
        .iter()
        .filter(|scope| scope.kind() == scope::DesignFeatureKind::JointOrigin)
    {
        let Some(transform) = scope.joint_origin_transform() else {
            continue;
        };
        resolved_origins.insert(scope.record_index, transform);
    }
    for (assembly_record_index, joint_origin_record_index, transform) in envelopes {
        if resolved_origins.get(&joint_origin_record_index) != Some(&transform) {
            continue;
        }
        let mut assemblies = scopes.iter_mut().filter(|scope| {
            scope.kind() == scope::DesignFeatureKind::Assemble
                && scope.record_index == assembly_record_index
        });
        let Some(assembly) = assemblies.next() else {
            continue;
        };
        if assemblies.next().is_some() {
            continue;
        }
        if let Some(alignment) = assembly.assembly_alignment_mut() {
            alignment.form = Some(assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                joint_origin_scope_record_index: joint_origin_record_index,
            });
        }
    }
}

/// Bind the pathless axial assembly selectors after every scope in the Design
/// stream has decoded its own construction.
pub(super) fn bind_axial_assembly_operand_targets(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    let mut bindings = Vec::new();
    for (ordinal, scope) in scopes.iter().enumerate() {
        if !matches!(scope.frame_length(), 705 | 772) {
            continue;
        }
        let Some(alignment) = scope.assembly_alignment() else {
            continue;
        };
        let Some(assembly::DesignAssemblyAlignmentForm::Frames { frames }) =
            alignment.form.as_ref()
        else {
            continue;
        };
        let Some(first) =
            exact_assembly_axial_operand_target(bytes, records, scope, &frames[0], scopes)?
        else {
            continue;
        };
        let Some(second) =
            exact_assembly_axial_operand_target(bytes, records, scope, &frames[1], scopes)?
        else {
            continue;
        };

        bindings.push((
            ordinal,
            assembly::DesignAssemblyAlignmentForm::qualified(
                frames.clone(),
                [first, second]
                    .map(|target| DesignAssemblyOperandQualifier::AxialTarget { target }),
            ),
        ));
    }

    for (ordinal, form) in bindings {
        if let Some(alignment) = scopes[ordinal].assembly_alignment_mut() {
            alignment.form = Some(form);
        }
    }
    Ok(())
}

struct AxialComponentOperand {
    construction_record_index: u32,
    construction_class_tag: String,
    construction_byte_offset: u64,
    construction_transform_offset: u64,
    axis_record_index_offsets: [u64; 2],
    construction_paired_class_tag: String,
    construction_paired_byte_offset: u64,
    selectors: Box<[DesignAssemblyAxialSelectorIdentity; 2]>,
}

struct ExactIndexedRecordPair {
    record_index: u32,
    class_tag: String,
    byte_offset: usize,
    paired_class_tag: String,
    paired_byte_offset: usize,
}

fn exact_assembly_axial_operand_target(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    assembly: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
    scopes: &[DesignParameterScope],
) -> Result<Option<DesignAssemblyAxialOperandTarget>, CodecError> {
    let component = exact_assembly_axial_component_operand(bytes, records, assembly, frame)?
        .and_then(|component| {
            let role = &component.selectors[0].occurrence_role;
            let mut matches = scopes.iter().filter(|scope| {
                scope.kind() == scope::DesignFeatureKind::ComponentInsert
                    && scope
                        .component_insert_construction()
                        .is_some_and(|construction| {
                            construction
                                .neutron_role
                                .eq_ignore_ascii_case(role.as_str())
                        })
            });
            let component_insert = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            Some(
                DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                    component_insert_scope_record_index: component_insert.record_index,
                    construction_record_index: component.construction_record_index,
                    construction_class_tag: component.construction_class_tag.try_into().ok()?,
                    construction_byte_offset: component.construction_byte_offset,
                    construction_transform_offset: component.construction_transform_offset,
                    axis_record_index_offsets: component.axis_record_index_offsets,
                    construction_paired_class_tag: component
                        .construction_paired_class_tag
                        .try_into()
                        .ok()?,
                    construction_paired_byte_offset: component.construction_paired_byte_offset,
                    selectors: component.selectors,
                },
            )
        });
    let mut origins = scopes.iter().filter(|scope| {
        scope.kind() == scope::DesignFeatureKind::JointOrigin
            && scope.record_index == frame.reference_record_index
            && scope.joint_origin_transform() == Some(frame.transform)
    });
    let root = match (origins.next(), origins.next()) {
        (Some(origin), None) => Some(DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
            scope_record_index: origin.record_index,
        }),
        _ => None,
    };
    Ok(match (component, root) {
        (Some(target), None) | (None, Some(target)) => Some(target),
        _ => None,
    })
}

fn exact_assembly_axial_component_operand(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
) -> Result<Option<AxialComponentOperand>, CodecError> {
    if !matches!(scope.frame_length(), 705 | 772)
        || scope
            .reference_members()
            .values()
            .filter(|record_index| **record_index == frame.reference_record_index)
            .count()
            != 1
    {
        return Ok(None);
    }
    let Some(search_start) = usize::try_from(scope.paired_byte_offset()).ok() else {
        return Ok(None);
    };
    let mut candidate = None;
    for start in records
        .offsets(frame.reference_record_index)
        .iter()
        .copied()
        .filter(|start| *start >= search_start)
    {
        if let Some(next) =
            exact_assembly_axial_component_operand_at(bytes, records, scope, frame, start)?
        {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some(next);
        }
    }
    Ok(candidate)
}

fn exact_assembly_axial_component_operand_at(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
    start: usize,
) -> Result<Option<AxialComponentOperand>, CodecError> {
    (|| {
        let construction_class_tag =
            exact_indexed_header_at(bytes, start, frame.reference_record_index)?;
        let paired_at = start.checked_add(axial_carrier::PAIRED_INDEXED_HEADER)?;
        let construction_paired_class_tag =
            exact_indexed_header_at(bytes, paired_at, frame.reference_record_index)?;
        let construction_transform_at = start.checked_add(axial_carrier::OPERAND_TRANSFORM)?;
        if rigid_transform_at(bytes, construction_transform_at)? != frame.transform {
            return None;
        }
        let (first_axis_record_index, first_axis_record_index_offset) =
            exact_same_segment_record_reference(
                bytes,
                start.checked_add(axial_carrier::FIRST_AXIS_RECORD_REFERENCE)?,
            )?;
        let (second_axis_record_index, second_axis_record_index_offset) =
            exact_same_segment_record_reference(
                bytes,
                start.checked_add(axial_carrier::SECOND_AXIS_RECORD_REFERENCE)?,
            )?;
        if first_axis_record_index == second_axis_record_index {
            return None;
        }
        let first_selector_record_index = first_axis_record_index.checked_add(3)?;
        let second_selector_record_index = second_axis_record_index.checked_add(3)?;
        for pair in [
            [first_axis_record_index, first_selector_record_index],
            [second_axis_record_index, second_selector_record_index],
        ] {
            if scope
                .reference_members()
                .values()
                .zip(scope.reference_members().values().skip(1))
                .filter(|(first, second)| [**first, **second] == pair)
                .count()
                != 1
                || pair.iter().any(|record_index| {
                    scope
                        .reference_members()
                        .values()
                        .filter(|member| *member == record_index)
                        .count()
                        != 1
                })
            {
                return None;
            }
        }
        let search_start = usize::try_from(scope.paired_byte_offset()).ok()?;
        let first_axis = exact_paired_indexed_record_between(
            bytes,
            records,
            first_axis_record_index,
            search_start,
            start,
        )?;
        let second_axis = exact_paired_indexed_record_between(
            bytes,
            records,
            second_axis_record_index,
            search_start,
            start,
        )?;
        if first_axis.byte_offset >= second_axis.byte_offset {
            return None;
        }
        let second_axis_at = second_axis.byte_offset;
        let first = match exact_assembly_axial_selector(bytes, records, first_axis, second_axis_at)
        {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let second = match exact_assembly_axial_selector(bytes, records, second_axis, start) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if !first.selects_same_object(&second)
            || !first
                .occurrence_role
                .as_str()
                .eq_ignore_ascii_case(second.occurrence_role.as_str())
        {
            return None;
        }
        Some(Ok(AxialComponentOperand {
            construction_record_index: frame.reference_record_index,
            construction_class_tag,
            construction_byte_offset: u64::try_from(start).ok()?,
            construction_transform_offset: u64::try_from(construction_transform_at).ok()?,
            axis_record_index_offsets: [
                first_axis_record_index_offset,
                second_axis_record_index_offset,
            ],
            construction_paired_class_tag,
            construction_paired_byte_offset: u64::try_from(paired_at).ok()?,
            selectors: Box::new([first, second]),
        }))
    })()
    .transpose()
}

fn exact_assembly_axial_selector(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    axis: ExactIndexedRecordPair,
    limit: usize,
) -> Result<Option<DesignAssemblyAxialSelectorIdentity>, CodecError> {
    (|| {
        let axis_record_index = axis.record_index;
        let selector_record_index = axis_record_index.checked_add(3)?;
        let mut selector_offsets = records
            .offsets(selector_record_index)
            .iter()
            .copied()
            .filter(|offset| *offset > axis.paired_byte_offset && *offset < limit);
        let (Some(selector_at), Some(selector_paired_at), None) = (
            selector_offsets.next(),
            selector_offsets.next(),
            selector_offsets.next(),
        ) else {
            return None;
        };
        let selector_class_tag =
            exact_indexed_header_at(bytes, selector_at, selector_record_index)?;
        let selector_paired_class_tag =
            exact_indexed_header_at(bytes, selector_paired_at, selector_record_index)?;
        if bytes.get(
            selector_at.checked_add(axial_selector::ZERO_RUN_11)?
                ..selector_at.checked_add(axial_selector::NESTED_RECORD_REFERENCE)?,
        )? != [0; 11]
        {
            return None;
        }
        let mut cursor = selector_at.checked_add(axial_selector::NESTED_RECORD_REFERENCE)?;
        let nested_record_index_offset = cursor.checked_add(1)?;
        let (nested_record_index, _) = exact_same_segment_record_reference(bytes, cursor)?;
        cursor = cursor.checked_add(11)?;
        if nested_record_index != selector_record_index.checked_add(3)?
            || View::u32_le_at(bytes, cursor)? != 1
        {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let selector_asset_at = cursor;
        let (selector_asset_id, after_selector_asset_id) =
            match fixed_relaxed_guid_text(bytes, selector_asset_at) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        let selector_context_at = after_selector_asset_id;
        let (selector_context_id, after_selector_context_id) =
            match fixed_relaxed_guid_text(bytes, selector_context_at) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        if View::u32_le_at(bytes, after_selector_context_id)? != 2
            || View::u32_le_at(bytes, after_selector_context_id.checked_add(4)?)? != 0
            || View::u32_le_at(bytes, after_selector_context_id.checked_add(8)?)? != 1
        {
            return None;
        }
        cursor = after_selector_context_id.checked_add(12)?;
        let occurrence_reference_offset = cursor.checked_add(1)?;
        let occurrence = take_reference(bytes, &mut cursor)?;
        let (occurrence_reference, _) = occurrence.local()?;
        if View::u32_le_at(bytes, cursor)? != 1 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let external = match take_external_reference_identity(bytes, &mut cursor) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if !external
            .asset_id
            .as_str()
            .eq_ignore_ascii_case(selector_asset_id.as_str())
            || cursor > selector_paired_at
        {
            return None;
        }

        let role_record_index = selector_record_index.checked_add(5)?;
        let mut role_offsets = records
            .offsets(role_record_index)
            .iter()
            .copied()
            .filter(|offset| *offset > selector_paired_at && *offset < limit);
        let (Some(role_at), None) = (role_offsets.next(), role_offsets.next()) else {
            return None;
        };
        let role_class_tag = exact_indexed_header_at(bytes, role_at, role_record_index)?;
        if bytes.get(
            role_at.checked_add(axial_role::ZERO_RUN_10)?
                ..role_at.checked_add(axial_role::CONSTANT_ONE)?,
        )? != [0; 10]
            || View::u32_le_at(bytes, role_at.checked_add(axial_role::CONSTANT_ONE)?)? != 1
        {
            return None;
        }
        let occurrence_role_at = role_at.checked_add(axial_role::ROLE_CODE_UNIT_COUNT)?;
        let (occurrence_role, after_occurrence_role) =
            match fixed_relaxed_guid_text(bytes, occurrence_role_at) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        if after_occurrence_role > limit {
            return None;
        }

        Some(Ok(DesignAssemblyAxialSelectorIdentity {
            axis_record_index,
            axis_class_tag: axis.class_tag.try_into().ok()?,
            axis_byte_offset: u64::try_from(axis.byte_offset).ok()?,
            axis_paired_class_tag: axis.paired_class_tag.try_into().ok()?,
            axis_paired_byte_offset: u64::try_from(axis.paired_byte_offset).ok()?,
            selector_record_index,
            selector_class_tag: selector_class_tag.try_into().ok()?,
            selector_byte_offset: u64::try_from(selector_at).ok()?,
            selector_paired_class_tag: selector_paired_class_tag.try_into().ok()?,
            selector_paired_byte_offset: u64::try_from(selector_paired_at).ok()?,
            nested_record_index,
            nested_record_index_offset: u64::try_from(nested_record_index_offset).ok()?,
            selector_asset_id,
            selector_asset_id_offset: u64::try_from(selector_asset_at.checked_add(4)?).ok()?,
            selector_context_id,
            selector_context_id_offset: u64::try_from(selector_context_at.checked_add(4)?).ok()?,
            occurrence_reference,
            occurrence_reference_offset: u64::try_from(occurrence_reference_offset).ok()?,
            external_object_reference: external.target,
            external_object_reference_offset: external.target_offset,
            external_segment: external.segment,
            external_segment_offset: external.segment_offset,
            external_asset_id: external.asset_id,
            external_asset_id_offset: external.asset_id_offset,
            external_link_name: external.link_name,
            external_link_name_offset: external.link_name_offset,
            external_version: external.version,
            role_record_index,
            role_class_tag: role_class_tag.try_into().ok()?,
            role_byte_offset: u64::try_from(role_at).ok()?,
            occurrence_role,
            occurrence_role_offset: u64::try_from(occurrence_role_at.checked_add(4)?).ok()?,
        }))
    })()
    .transpose()
}

fn exact_paired_indexed_record_between(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    start: usize,
    end: usize,
) -> Option<ExactIndexedRecordPair> {
    let mut offsets = records
        .offsets(record_index)
        .iter()
        .copied()
        .filter(|offset| *offset >= start && *offset < end);
    let (Some(primary_at), Some(paired_at), None) =
        (offsets.next(), offsets.next(), offsets.next())
    else {
        return None;
    };
    let class_tag = exact_indexed_header_at(bytes, primary_at, record_index)?;
    let paired_class_tag = exact_indexed_header_at(bytes, paired_at, record_index)?;
    Some(ExactIndexedRecordPair {
        record_index,
        class_tag,
        byte_offset: primary_at,
        paired_class_tag,
        paired_byte_offset: paired_at,
    })
}

fn exact_single_joint_origin_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(u32, ScopePlacementFrame)> {
    if scope.kind() != scope::DesignFeatureKind::Assemble
        || scope.class_tag.as_str() != "276"
        || scope.paired_class_tag.as_str() != "258"
        || scope.frame_length() != 604
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if usize::try_from(scope.paired_byte_offset()).ok()? != start.checked_add(604)?
        || bytes.get(start + 11..start + 24)? != [0; 13]
        || bytes.get(start + 29..start + 36)? != [0; 7]
        || bytes.get(start + 169..start + 175)? != [0; 6]
        || View::u32_le_at(bytes, start + 175)? != 1
    {
        return None;
    }
    let reference_record_index = marked_record_reference(bytes, start + 24)?;
    let joint_origin_record_index = marked_record_reference(bytes, start + 164)?;
    if reference_record_index == joint_origin_record_index {
        return None;
    }
    let transform = rigid_transform_at(bytes, start + 36)?;
    Some((
        joint_origin_record_index,
        ScopePlacementFrame {
            transform,
            transform_offset: u64::try_from(start + 36).ok()?,
            reference: Some((reference_record_index, u64::try_from(start + 25).ok()?)),
        },
    ))
}
