// SPDX-License-Identifier: Apache-2.0
//! Parse exact legacy As-built assembly alignment frames.

use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::operands::{parse_entity_selection_prefix, parse_face_operand};
use crate::layout::assembly_as_built_421_frame_297 as as_built_421_frame_297;
use crate::layout::assembly_as_built_421_frame_327 as as_built_421_frame_327;
use crate::layout::assembly_as_built_421_frame_376 as as_built_421_frame_376;
use crate::layout::assembly_as_built_421_frame_448 as as_built_421_frame_448;
use crate::layout::assembly_as_built_421_scope as as_built_421;
use crate::records::{
    decal::DesignRecordHeader,
    feature::{
        assembly::{
            DesignAssemblyLegacyOperand, DesignAssemblyLegacyOperands,
            DesignAssemblyLegacySelection, DesignAssemblyLimits, DesignAssemblyLimitsWire,
            DesignAssemblySolvedFrame,
        },
        scope::DesignParameterScope,
    },
    parameters::DesignParameterOwner,
    recipes::ConstructionRecipe,
};
use cadmpeg_core::decode::View;
use std::collections::HashMap;

use super::scopes::hole::exact_hole_construction;
use super::scopes::point_data::exact_point_data_construction;
use super::scopes::shared_frames::exact_indexed_header_at;
use super::scopes::shared_frames::marked_record_reference;
use super::scopes::shared_frames::rigid_transform_at;
use super::sketch::IndexedRecordOffsets;

pub(in crate::design::decode) struct LegacyAsBuilt421Alignment {
    pub(super) angle: f64,
    pub(super) offset: [f64; 3],
    pub(super) owners: Vec<crate::records::identity::Located<u32>>,
    pub(super) limits: DesignAssemblyLimits,
}

pub(super) fn exact_legacy_as_built_421_alignment(
    bytes: &[u8],
    scope: &DesignParameterScope,
    lanes: &[&DesignParameterOwner],
) -> Option<LegacyAsBuilt421Alignment> {
    let generation = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    )?;
    let references = scope.reference_members().located_rows()?;
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::AsBuilt
        || lanes.len() != 6
        || references.len() != 11
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if usize::try_from(scope.paired_byte_offset()).ok()? != start.checked_add(as_built_421::LEN)?
        || scope.reference_count_offset()
            != u64::try_from(start.checked_add(as_built_421::REFERENCE_COUNT)?).ok()?
        || View::u32_le_at(bytes, start.checked_add(as_built_421::REFERENCE_COUNT)?)?
            != as_built_421::REFERENCE_COUNT_VALUE
        || View::u32_le_at(bytes, start.checked_add(as_built_421::KIND_LENGTH)?)?
            != as_built_421::KIND_LENGTH_VALUE
        || scope.feature_ordinal_offset()
            != u64::try_from(start.checked_add(as_built_421::FEATURE_ORDINAL)?).ok()?
    {
        return None;
    }
    for (ordinal, reference) in references.iter().enumerate() {
        let reference_at = start
            .checked_add(as_built_421::REFERENCE_ENTRIES.checked_add(ordinal.checked_mul(11)?)?)?;
        if marked_record_reference(bytes, reference_at)? != reference.value
            || bytes.get(reference_at.checked_add(5)?..reference_at.checked_add(11)?)? != [0; 6]
            || reference.offset != u64::try_from(reference_at.checked_add(1)?).ok()?
        {
            return None;
        }
    }
    if bytes.get(
        start.checked_add(as_built_421::KIND_LENGTH)?..start.checked_add(as_built_421::KIND)?,
    )? != as_built_421::KIND_LENGTH_VALUE.to_le_bytes()
        || bytes.get(
            start.checked_add(as_built_421::REFERENCE_TRAILER)?
                ..start.checked_add(as_built_421::KIND_LENGTH)?,
        )? != as_built_421::REFERENCE_TRAILER_VALUE
    {
        return None;
    }
    if lanes.iter().any(|owner| {
        owner.class_tag().as_str() != generation.owner_class_tag() || owner.frame_length() != 103
    }) {
        return None;
    }
    let [offset_x, offset_y, offset_z, angle, limit_first, limit_second] = lanes else {
        return None;
    };
    let alignment_owner_record_indices = [
        offset_x.record_index(),
        offset_y.record_index(),
        offset_z.record_index(),
        angle.record_index(),
    ];
    let source_limit_owner_record_indices =
        [limit_first.record_index(), limit_second.record_index()];
    if !scope
        .reference_members()
        .values()
        .skip(4)
        .take(4)
        .eq(alignment_owner_record_indices.iter())
        || !scope
            .reference_members()
            .values()
            .skip(9)
            .take(2)
            .eq(source_limit_owner_record_indices.iter())
    {
        return None;
    }
    let (minimum_owner, maximum_owner) = if generation.reverse_limit_order() {
        (limit_second, limit_first)
    } else {
        (limit_first, limit_second)
    };
    let kind = generation.limit_kind();
    let minimum = minimum_owner.evaluated_value().get();
    let maximum = maximum_owner.evaluated_value().get();
    let limit_owner_record_indices = [minimum_owner.record_index(), maximum_owner.record_index()];
    let limit_value_offsets = [
        minimum_owner.evaluated_value_offset(),
        maximum_owner.evaluated_value_offset(),
    ];
    Some(LegacyAsBuilt421Alignment {
        angle: angle.evaluated_value().get(),
        offset: [
            offset_x.evaluated_value().get(),
            offset_y.evaluated_value().get(),
            offset_z.evaluated_value().get(),
        ],
        owners: [angle, offset_x, offset_y, offset_z]
            .into_iter()
            .map(|owner| crate::records::identity::Located {
                value: owner.record_index(),
                offset: owner.evaluated_value_offset(),
            })
            .collect(),
        limits: DesignAssemblyLimitsWire {
            kind,
            minimum,
            maximum,
            owner_record_indices: limit_owner_record_indices,
            value_offsets: limit_value_offsets,
        }
        .try_into()
        .ok()?,
    })
}

pub(super) fn exact_legacy_as_built_421_solved_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignAssemblySolvedFrame> {
    let generation = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    )?;
    let references = scope.reference_members().located_rows()?;
    let [_, _, _, _, _, _, _, _, frame_reference, _, _] = references else {
        return None;
    };
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::AsBuilt {
        return None;
    }
    let frame_record_index = frame_reference.value;
    let expected_class_tag = generation.frame_class_tag();
    let mut frame_candidates =
        records
            .offsets(frame_record_index)
            .iter()
            .copied()
            .filter(|frame_start| {
                exact_indexed_header_at(bytes, *frame_start, frame_record_index).as_deref()
                    == Some(expected_class_tag)
            });
    let frame_start = frame_candidates.next()?;
    if frame_candidates.next().is_some() {
        return None;
    }
    let frame_length = generation.frame_length();
    let matrix_prefix = generation.matrix_prefix();
    let transform_offset = generation.matrix_offset();
    let matrix_prefix_value = match generation {
        crate::design::assembly::LegacyAsBuilt421Generation::Class364 => {
            as_built_421_frame_376::MATRIX_PREFIX_VALUE
        }
        crate::design::assembly::LegacyAsBuilt421Generation::Class420 => {
            as_built_421_frame_327::MATRIX_PREFIX_VALUE
        }
        crate::design::assembly::LegacyAsBuilt421Generation::Class417 => {
            as_built_421_frame_448::MATRIX_PREFIX_VALUE
        }
        crate::design::assembly::LegacyAsBuilt421Generation::Class457 => {
            as_built_421_frame_297::MATRIX_PREFIX_VALUE
        }
    };
    if exact_indexed_header_at(
        bytes,
        frame_start.checked_add(frame_length)?,
        frame_record_index,
    )
    .as_deref()
        != Some(generation.frame_paired_class_tag())
    {
        return None;
    }
    if bytes
        .get(frame_start.checked_add(matrix_prefix)?..frame_start.checked_add(transform_offset)?)?
        != matrix_prefix_value
    {
        return None;
    }
    let transform_at = frame_start.checked_add(transform_offset)?;
    Some(DesignAssemblySolvedFrame {
        reference_record_index: frame_record_index,
        reference_offset: frame_reference.offset,
        record_byte_offset: u64::try_from(frame_start).ok()?,
        class_tag: expected_class_tag.to_owned().try_into().ok()?,
        transform: rigid_transform_at(bytes, transform_at)?,
        transform_offset: u64::try_from(transform_at).ok()?,
    })
}

/// Maximum component error accepted when a legacy hole direction is compared
/// with the solved connector frame's third basis column.
const EPS_LEGACY_AS_BUILT_DIRECTION: f64 = 1.0e-10;

/// Decode the two ordered construction/face-selection pairs of a 421-byte
/// `As-built` scope and derive their local frames from the stored solved frame.
pub(super) fn exact_legacy_as_built_421_operands(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
    recipes: &[ConstructionRecipe],
    solved_frame: &DesignAssemblySolvedFrame,
) -> Result<Option<DesignAssemblyLegacyOperands>, cadmpeg_core::CodecError> {
    (|| {
    let generation = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    )?;
    let references = scope.reference_members().located_rows()?;
    let [point_reference, first_selection_reference, hole_reference, second_selection_reference, _, _, _, _, frame_reference, _, _] =
        references
    else {
        return None;
    };
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::AsBuilt
        || solved_frame.reference_record_index != frame_reference.value
    {
        return None;
    }
    let point_record_index = point_reference.value;
    let first_selection_record_index = first_selection_reference.value;
    let hole_record_index = hole_reference.value;
    let second_selection_record_index = second_selection_reference.value;
    let point = exact_point_data_construction(bytes, records, &[point_record_index], stream_types)?;
    let hole = match exact_hole_construction(bytes, records, scope, stream_types, &crate::records::feature::scope::DesignFeatureKind::AsBuilt) {
        Ok(Some(hole)) => hole,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    if hole.point_record_index != hole_record_index
        || !hole
            .input_records
            .iter()
            .map(|reference| reference.value)
            .eq([second_selection_record_index])
        || !point
            .rule
            .inputs()
            .iter()
            .map(crate::records::feature::work_geometry::DesignWorkPointInput::record_index)
            .eq([first_selection_record_index])
    {
        return None;
    }
    let solved_direction = [
        solved_frame.transform[0][2],
        solved_frame.transform[1][2],
        solved_frame.transform[2][2],
    ];
    if hole
        .direction
        .into_iter()
        .zip(solved_direction)
        .any(|(actual, expected)| (actual.get() - expected).abs() > EPS_LEGACY_AS_BUILT_DIRECTION)
    {
        return None;
    }
    let selection_class_tag = match generation {
        crate::design::assembly::LegacyAsBuilt421Generation::Class364 => "307",
        crate::design::assembly::LegacyAsBuilt421Generation::Class420 => "273",
        crate::design::assembly::LegacyAsBuilt421Generation::Class417 => "332",
        crate::design::assembly::LegacyAsBuilt421Generation::Class457 => "264",
    };
    let first_selection = match exact_legacy_as_built_face_selection(
ctx,
bytes,
records,
scope,
1,
(first_selection_record_index, selection_class_tag),
recipes,
) {
        Ok(Some(selection)) => selection,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let second_selection = match exact_legacy_as_built_face_selection(
ctx,
bytes,
records,
scope,
3,
(second_selection_record_index, selection_class_tag),
recipes,
) {
        Ok(Some(selection)) => selection,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let point_class_tag = indexed_class_at(bytes, point.point_record_byte_offset)?;
    let hole_class_tag = indexed_class_at(bytes, hole.point_record_byte_offset)?;
    Some(Ok(
        crate::records::feature::assembly::DesignAssemblyLegacyOperands::new(
            DesignAssemblyLegacyOperand {
                construction_class_tag: point_class_tag.try_into().ok()?,
                construction: Box::new(point),
                selection: first_selection,
                reference_offset: point_reference.offset,
            },
            DesignAssemblyLegacyOperand {
                construction_class_tag: hole_class_tag.try_into().ok()?,
                construction: Box::new(hole),
                selection: second_selection,
                reference_offset: hole_reference.offset,
            },
        ),
    ))
    })().transpose()
}

fn indexed_class_at(bytes: &[u8], byte_offset: u64) -> Option<String> {
    let start = usize::try_from(byte_offset).ok()?;
    let (class_tag, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    (after_tag == start.checked_add(7)?).then(|| class_tag.to_owned())
}

fn exact_legacy_as_built_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    (record_index, expected_class_tag): (u32, &str),
    recipes: &[ConstructionRecipe],
) -> Result<Option<DesignAssemblyLegacySelection>, cadmpeg_core::CodecError> {
    let Some(scope_start) = usize::try_from(scope.byte_offset()).ok() else {
        return Ok(None);
    };
    let next_byte_offset = (|| {
        scope
            .reference_members()
            .values()
            .nth(
                usize::try_from(scope_reference_ordinal)
                    .ok()?
                    .checked_add(1)?,
            )
            .and_then(|record_index| {
                records
                    .offsets(*record_index)
                    .iter()
                    .copied()
                    .find(|offset| *offset > scope_start)
            })
            .and_then(|offset| u64::try_from(offset).ok())
    })();
    let mut candidates = records
        .offsets(record_index)
        .iter()
        .copied()
        .filter_map(|byte_offset| {
            let class_tag = indexed_class_at(bytes, u64::try_from(byte_offset).ok()?)?;
            if class_tag != expected_class_tag {
                return None;
            }
            let copied_id = scope.id.as_bytes().to_vec();
            let id = match String::from_utf8(copied_id) {
                Ok(id) => id,
                Err(error) => {
                    return Some(Err(cadmpeg_core::CodecError::NotImplemented(
                        error.to_string(),
                    )))
                }
            };
            let header = DesignRecordHeader {
                id,
                record_index,
                class_tag: class_tag.clone().try_into().ok()?,
                byte_offset: u64::try_from(byte_offset).ok()?,
            };
            let operand = parse_face_operand(
                ctx,
                bytes,
                records,
                crate::design::decode::operands::FaceOperandFrame {
                    scope,
                    scope_reference_ordinal,
                    group_ownership: None,
                    next_byte_offset,
                    header: &header,
                },
                recipes,
            )?;
            let operand = match operand {
                Ok(operand) => operand,
                Err(error) => return Some(Err(error)),
            };
            let prefix = parse_entity_selection_prefix(bytes, byte_offset, record_index)?;
            let next_byte_offset = operand.next_byte_offset();
            Some(Ok(DesignAssemblyLegacySelection {
                record_index,
                byte_offset: u64::try_from(byte_offset).ok()?,
                class_tag: header.class_tag,
                asset_id: prefix.asset_id.try_into().ok()?,
                asset_id_offset: prefix.asset_id_offset,
                context_id: prefix.context_id.try_into().ok()?,
                context_id_offset: prefix.context_id_offset,
                recipe_record_index: operand.recipe_record_index(),
                recipe_record_byte_offset: operand.recipe_record_byte_offset(),
                recipe_id: operand.recipe_id,
                recipe_kind: operand.recipe_kind,
                recipe_references: operand.recipe_references,
                next_byte_offset,
            }))
        });
    let Some(candidate) = candidates.next() else {
        return Ok(None);
    };
    let candidate = candidate?;
    if candidates.next().transpose()?.is_some() {
        return Ok(None);
    }
    Ok(Some(candidate))
}
