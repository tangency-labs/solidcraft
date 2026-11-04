// SPDX-License-Identifier: Apache-2.0
//! Exact surface extend, offset, boundary, stitch and ruled-surface operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::take_reference;
use crate::design::decode::operands::parse_construction_operand_group;
use crate::design::decode::operands::ConstructionOperandGroupParse;
use crate::design::decode::operands::RecordFrame;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignRuledSurfaceCorner;
use crate::records::feature::surface_ops::DesignRuledSurfaceMethod;
use crate::records::feature::surface_ops::DesignRuledSurfaceOperation;
use crate::records::feature::surface_ops::DesignSurfaceExtendMethod;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetSupport;
use crate::records::feature::surface_ops::DesignSurfaceStitchOperation;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashSet;

pub(super) fn exact_surface_extend_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignSurfaceExtendOperation> {
    let operation = exact_surface_boundary_operation(
        bytes,
        records,
        scope,
        DesignFeatureFamily::SurfaceExtend,
        8,
    )?;
    if operation.distance.get() <= 0.0 {
        return None;
    }
    let method = match operation.mode {
        0 => DesignSurfaceExtendMethod::Natural,
        1 => DesignSurfaceExtendMethod::Tangent,
        2 => DesignSurfaceExtendMethod::Perpendicular,
        _ => return None,
    };
    Some(DesignSurfaceExtendOperation {
        distance: operation.distance,
        distance_offset: operation.distance_offset,
        distance_record_index: operation.distance_record_index,
        method,
        method_offset: operation.mode_offset,
        boundary_record_index: operation.boundary_record_index,
        boundary_reference_record_index: operation.boundary_reference_record_index,
        boundary_reference_offset: operation.boundary_reference_offset,
        edge_record_indices: operation.edge_record_indices,
        tolerance: operation.tolerance,
        tolerance_offset: operation.tolerance_offset,
    })
}

pub(super) fn exact_surface_offset_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignSurfaceOffsetOperation>, CodecError> {
    if let Some(operation) = exact_surface_offset_face_groups(bytes, records, scope)? {
        return Ok(Some(operation));
    }
    let Some(operation) = exact_surface_boundary_operation(
        bytes,
        records,
        scope,
        DesignFeatureFamily::SurfaceOffset,
        65,
    ) else {
        return Ok(None);
    };
    Ok(
        (operation.mode == 1).then_some(DesignSurfaceOffsetOperation {
            distance: operation.distance,
            distance_offset: operation.distance_offset,
            distance_record_index: operation.distance_record_index,
            support: DesignSurfaceOffsetSupport::BoundaryCarrier {
                boundary_record_index: operation.boundary_record_index,
                boundary_reference_record_index: operation.boundary_reference_record_index,
                boundary_reference_offset: operation.boundary_reference_offset,
                edge_record_indices: operation.edge_record_indices,
                tolerance: operation.tolerance,
                tolerance_offset: operation.tolerance_offset,
            },
        }),
    )
}

fn exact_surface_offset_face_groups(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignSurfaceOffsetOperation>, CodecError> {
    let parsed = (|| {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::SurfaceOffset) {
            return None;
        }
        let distance_record_index = scope.reference_members().values().next()?;
        let support_reference_count = scope.reference_members().len() - 1;
        if support_reference_count == 0 {
            return None;
        }
        let scalar = exact_fixed_scalar(bytes, records, *distance_record_index)?;
        if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 0 {
            return None;
        }

        let mut group_record_indices = Vec::new();
        let mut covered_references = HashSet::new();
        for (scope_reference_ordinal, record_index) in scope
            .reference_members()
            .values()
            .copied()
            .enumerate()
            .skip(1)
        {
            let group = exact_construction_operand_group(
                bytes,
                records,
                scope,
                u32::try_from(scope_reference_ordinal).ok()?,
                record_index,
            );
            let Some(group) = group else {
                continue;
            };
            let group = match group {
                Ok(group) => group,
                Err(error) => return Some(Err(error)),
            };
            if group.role() != DesignOperandRole::PROFILE
                || group.frame.opaque_index.get() != 252
                || group.members().is_empty()
                || covered_references.contains(&group.record_index)
            {
                return None;
            }

            covered_references.insert(group.record_index);
            for member in group.members().iter().map(|member| &member.value) {
                if *member == *distance_record_index
                    || !scope
                        .reference_members()
                        .values()
                        .skip(1)
                        .any(|value| value == member)
                    || covered_references.contains(member)
                {
                    return None;
                }

                covered_references.insert(*member);
            }

            group_record_indices.push(group.record_index);
        }
        if group_record_indices.is_empty() || covered_references.len() != support_reference_count {
            return None;
        }

        Some(Ok(DesignSurfaceOffsetOperation {
            distance: scalar.value,
            distance_offset: scalar.value_offset,
            distance_record_index: *distance_record_index,
            support: DesignSurfaceOffsetSupport::FaceGroups {
                group_record_indices,
            },
        }))
    })();
    parsed.transpose()
}

fn exact_construction_operand_group(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    record_index: u32,
) -> Option<
    Result<crate::records::topology::construction::DesignConstructionOperandGroup, CodecError>,
> {
    let mut candidate = None;
    for (start, _) in records.frames(record_index) {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if after_tag != start + 7 {
            continue;
        }
        let header = RecordFrame {
            record_index,
            class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
            byte_offset: u64::try_from(start).ok()?,
        };
        match parse_construction_operand_group(bytes, scope, scope_reference_ordinal, &header) {
            ConstructionOperandGroupParse::Complete(group) => {
                if candidate.replace(*group).is_some() {
                    return None;
                }
            }
            ConstructionOperandGroupParse::NotAGroup | ConstructionOperandGroupParse::Unclosed => {}
        }
    }
    candidate.map(Ok)
}

#[derive(Clone)]
struct ExactSurfaceBoundaryOperation {
    distance: cadmpeg_ir::scalar::FiniteReal,
    distance_offset: u64,
    distance_record_index: u32,
    mode: u32,
    mode_offset: u64,
    boundary_record_index: u32,
    boundary_reference_record_index: u32,
    boundary_reference_offset: u64,
    edge_record_indices: Vec<u32>,
    tolerance: cadmpeg_ir::scalar::PositiveReal,
    tolerance_offset: u64,
}

fn exact_surface_boundary_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    family: DesignFeatureFamily,
    boundary_kind: u32,
) -> Option<ExactSurfaceBoundaryOperation> {
    let parsed = (|| {
        if design_feature_family(&scope.kind()) != Some(family) {
            return None;
        }
        let mut references = scope.reference_members().values();
        let distance_record_index = references.next()?;
        let boundary_record_index = references.next()?;
        let edge_record_indices = references;
        if edge_record_indices.len() == 0 {
            return None;
        }
        let scalar = exact_fixed_scalar(bytes, records, *distance_record_index)?;
        if scalar.owner_record_index != Some(scope.record_index)
            || scalar.ordinal != 0
            || records
                .frames(*distance_record_index)
                .filter(|(start, end)| {
                    end.checked_sub(*start) == Some(104)
                        && lp_ascii_filtered_view(bytes, *start, 0..=2000, u8::is_ascii_graphic)
                            .is_some_and(|(class_tag, after_tag)| {
                                after_tag == *start + 7
                                    && class_tag.len() == 3
                                    && class_tag.bytes().all(|byte| byte.is_ascii_digit())
                            })
                        && bytes.get(*start + 11..*start + 19) == Some(&[0; 8])
                        && bytes.get(*start + 19..*start + 24) == Some(&[1, 1, 0, 0, 0])
                        && marked_record_reference(bytes, *start + 24) == Some(scope.record_index)
                        && bytes.get(*start + 29..*start + 35) == Some(&[0; 6])
                        && bytes.get(*start + 35..*start + 40) == Some(&[0; 5])
                        && marked_record_reference(bytes, *start + 48)
                            == distance_record_index.checked_sub(1)
                        && bytes.get(*start + 53..*start + 59) == Some(&[0; 6])
                        && View::u32_le_at(bytes, *start + 59).is_some_and(|value| value != 0)
                        && bytes.get(*start + 63..*start + 67) == Some(&[0; 4])
                        && marked_record_reference(bytes, *start + 67) == Some(scope.record_index)
                        && bytes.get(*start + 72..*start + 78) == Some(&[0; 6])
                        && bytes.get(*start + 78..*start + 81) == Some(&[1, 0, 0])
                        && marked_record_reference(bytes, *start + 81)
                            == distance_record_index.checked_add(1)
                        && bytes.get(*start + 86..*start + 93) == Some(&[0; 7])
                        && marked_record_reference(bytes, *start + 93) == Some(scope.record_index)
                        && bytes.get(*start + 98..*start + 104) == Some(&[0; 6])
                })
                .count()
                != 1
        {
            return None;
        }
        let mut candidate = None;
        for (start, end) in records.frames(*boundary_record_index) {
            let parsed = (|| {
                let member_bytes = edge_record_indices.len().checked_mul(11)?;
                let tail = start.checked_add(25)?.checked_add(member_bytes)?;
                (end.checked_sub(start)? == 113usize.checked_add(member_bytes)?).then_some(())?;
                let (class_tag, after_tag) =
                    lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
                if after_tag != start + 7
                    || class_tag.len() != 3
                    || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                    || bytes.get(start + 11..start + 21)? != [0; 10]
                    || View::u32_le_at(bytes, start + 21)?
                        != u32::try_from(edge_record_indices.len()).ok()?
                    || edge_record_indices
                        .clone()
                        .enumerate()
                        .any(|(ordinal, record_index)| {
                            marked_record_reference(bytes, start + 25 + ordinal * 11)
                                != Some(*record_index)
                        })
                    || bytes.get(tail..tail + 2)? != [0; 2]
                    || bytes.get(tail + 11..tail + 21)? != [0; 10]
                    || View::u32_le_at(bytes, tail + 21)? != boundary_kind
                    || bytes.get(tail + 25..tail + 35)? != [0; 10]
                    || View::u32_le_at(bytes, tail + 35)? != 210
                    || View::u32_le_at(bytes, tail + 47)? != 210
                    || marked_record_reference(bytes, tail + 51)
                        != boundary_record_index.checked_add(2)
                    || bytes.get(tail + 56..tail + 62)? != [0; 6]
                    || bytes.get(tail + 62..tail + 65)? != [1, 0, 0]
                    || marked_record_reference(bytes, tail + 65)
                        != boundary_record_index.checked_add(1)
                    || bytes.get(tail + 70..tail + 77)? != [0; 7]
                    || marked_record_reference(bytes, tail + 77) != Some(scope.record_index)
                    || bytes.get(tail + 82..tail + 88)? != [0; 6]
                {
                    return None;
                }
                let mode = View::u32_le_at(bytes, tail + 2)?;
                let boundary_reference_record_index = marked_record_reference(bytes, tail + 6)?;
                let tolerance =
                    cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, tail + 39)?)?;
                Some((
                    mode,
                    u64::try_from(tail + 2).ok()?,
                    boundary_reference_record_index,
                    u64::try_from(tail + 6).ok()?,
                    tolerance,
                    u64::try_from(tail + 39).ok()?,
                ))
            })();
            if let Some(parsed) = parsed {
                if candidate.replace(parsed).is_some() {
                    return None;
                }
            }
        }
        let (
            mode,
            mode_offset,
            boundary_reference_record_index,
            boundary_reference_offset,
            tolerance,
            tolerance_offset,
        ) = candidate?;
        Some((
            scalar,
            *distance_record_index,
            *boundary_record_index,
            edge_record_indices,
            mode,
            mode_offset,
            boundary_reference_record_index,
            boundary_reference_offset,
            tolerance,
            tolerance_offset,
        ))
    })();
    let (
        scalar,
        distance_record_index,
        boundary_record_index,
        edge_record_indices,
        mode,
        mode_offset,
        boundary_reference_record_index,
        boundary_reference_offset,
        tolerance,
        tolerance_offset,
    ) = parsed?;

    let mut edges = Vec::new();
    edges.extend(edge_record_indices.copied());
    Some(ExactSurfaceBoundaryOperation {
        distance: scalar.value,
        distance_offset: scalar.value_offset,
        distance_record_index,
        mode,
        mode_offset,
        boundary_record_index,
        boundary_reference_record_index,
        boundary_reference_offset,
        edge_record_indices: edges,
        tolerance,
        tolerance_offset,
    })
}

pub(super) fn exact_surface_stitch_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    references: &[u32],
) -> Option<DesignSurfaceStitchOperation> {
    if references.len() < 4 || !references.len().is_multiple_of(2) {
        return None;
    }
    let tolerance_record_index = references[references.len() - 2];
    let settings_record_index = references[references.len() - 1];
    let scalar = exact_fixed_scalar(bytes, records, tolerance_record_index)?;
    if scalar.owner_record_index != Some(scope_record_index) || scalar.ordinal != 0 {
        return None;
    }
    let gap_tolerance = cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?;
    Some(DesignSurfaceStitchOperation {
        gap_tolerance,
        gap_tolerance_offset: scalar.value_offset,
        tolerance_record_index,
        settings_record_index,
    })
}

pub(super) fn exact_ruled_surface_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Result<Option<DesignRuledSurfaceOperation>, CodecError> {
    let parsed = (|| {
        if bytes.get(start.checked_add(11)?..start.checked_add(20)?)? != [0; 9] {
            return None;
        }
        let method_offset = start.checked_add(20)?;
        let method = match View::u32_le_at(bytes, method_offset)? {
            0 => DesignRuledSurfaceMethod::Tangent,
            1 => DesignRuledSurfaceMethod::Normal,
            2 => DesignRuledSurfaceMethod::Direction,
            _ => return None,
        };
        if bytes.get(start.checked_add(24)?..start.checked_add(27)?)? != [0; 3] {
            return None;
        }
        let alternate_face_offset = start.checked_add(27)?;
        let alternate_face = match bytes.get(alternate_face_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let fixed_reference = |at: usize| {
            let mut cursor = at;
            let reference = take_reference(bytes, &mut cursor)?;
            (cursor == at.checked_add(11)?).then(|| u32::try_from(reference.local()?.0).ok())?
        };
        let angle_owner_record_index = fixed_reference(start.checked_add(28)?)?;
        let distance_owner_record_index = fixed_reference(start.checked_add(39)?)?;
        let corner_offset = start.checked_add(50)?;
        let corner = match View::u32_le_at(bytes, corner_offset)? {
            0 => DesignRuledSurfaceCorner::Rounded,
            1 => DesignRuledSurfaceCorner::Mitered,
            _ => return None,
        };
        let take_reference_list = |mut cursor: usize| {
            let count = usize::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
            if count > 100_000 {
                return None;
            }
            cursor = cursor.checked_add(4)?;

            let mut records = Vec::new();
            for _ in 0..count {
                records.push(match fixed_reference(cursor) {
                    Some(record) => record,
                    None => return None,
                });
                cursor = cursor.checked_add(11)?;
            }
            Some(Ok((records, cursor)))
        };
        let (mut edge_group_record_indices, mut cursor) =
            match take_reference_list(start.checked_add(54)?)? {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
        if View::u32_le_at(bytes, cursor)? != 0 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let (auxiliary_record_indices, next) = match take_reference_list(cursor)? {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        cursor = next;
        if View::u32_le_at(bytes, cursor)? != 0 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let (trailing_edge_groups, next) = match take_reference_list(cursor)? {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        cursor = next;

        edge_group_record_indices.extend(trailing_edge_groups);
        let (direction_entity_id, direction_end) = match fixed_relaxed_guid_text(bytes, cursor) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let direction_absent =
            direction_entity_id.as_str() == "00000000-0000-0000-0000-000000000000";
        if direction_end.checked_add(3)? != reference_count_at
            || bytes.get(direction_end..reference_count_at)? != [0; 3]
            || paired_at <= reference_count_at
        {
            return None;
        }
        let direction_entity_id = if direction_absent {
            None
        } else {
            Some(direction_entity_id)
        };
        if reference_members.first() != Some(&distance_owner_record_index)
            || reference_members.get(1) != Some(&angle_owner_record_index)
            || edge_group_record_indices.is_empty()
            || edge_group_record_indices
                .iter()
                .any(|record_index| !reference_members.contains(record_index))
        {
            return None;
        }
        Some(Ok(DesignRuledSurfaceOperation {
            method,
            method_offset: u64_from_index(method_offset),
            corner,
            corner_offset: u64_from_index(corner_offset),
            alternate_face,
            alternate_face_offset: u64_from_index(alternate_face_offset),
            angle_owner_record_index,
            distance_owner_record_index,
            edge_group_record_indices,
            auxiliary_record_indices,
            direction_entity_id,
        }))
    })();
    parsed.transpose()
}
