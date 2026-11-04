// SPDX-License-Identifier: Apache-2.0
//! Exact draft operation scopes.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::layout::draft_operation_prefix as prefix;
use crate::records::feature::direct_face::DesignDraftFlags;
use crate::records::feature::direct_face::DesignDraftOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;

const MODE_SINGLE_ANGLE: u32 = 1;

pub(super) fn exact_draft_operation_with_owners(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignDraftOperation> {
    // The frame is variable-length and carries six or more references, so no
    // frame length or reference count identifies the record. The ordered
    // reference table is in record-index order, so the two scalar lanes hold no
    // fixed position in it either: they sort before the operand groups in one
    // document and after them in another. The lanes are identified by their own
    // properties instead. They are the only scope-owned fixed scalars among the
    // references, and their local ordinals order them.
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Draft)
        || scope.reference_members().len() < 6
    {
        return None;
    }
    let scope_stream = native_stream(&scope.id);
    let mut lanes = scope
        .reference_members()
        .values()
        .filter_map(|record_index| {
            if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index) {
                return (scalar.owner_record_index == Some(scope.record_index)).then_some((
                    *record_index,
                    u32::from(scalar.ordinal),
                    scalar.value,
                    scalar.value_offset,
                ));
            }
            let mut owners = parameter_owners.iter().filter(|owner| {
                owner.record_index() == *record_index
                    && owner.scope_record_index() == scope.record_index
                    && scope_stream.is_none_or(|stream| native_stream(owner.id()) == Some(stream))
            });
            let owner = owners.next()?;
            if owners.next().is_some() {
                return None;
            }
            Some((
                *record_index,
                owner.local_ordinal(),
                owner.evaluated_value(),
                owner.evaluated_value_offset(),
            ))
        });
    let (Some(mut first), Some(mut second), None) = (lanes.next(), lanes.next(), lanes.next())
    else {
        return None;
    };
    if first.1 > second.1 {
        std::mem::swap(&mut first, &mut second);
    }
    let (angle_record_index, angle_ordinal, angle, angle_offset) = first;
    let (opposite_angle_record_index, opposite_ordinal, opposite, opposite_offset) = second;
    if angle_ordinal != 0 || opposite_ordinal != 1 || opposite.get() != 0.0 {
        return None;
    }
    let flags = draft_flags(
        bytes,
        scope,
        angle_record_index,
        opposite_angle_record_index,
    )
    .ok()?;
    Some(DesignDraftOperation {
        angle: cadmpeg_ir::scalar::Angle::from_assigned_real(angle),
        angle_record_index,
        angle_offset,
        opposite_angle_record_index,
        opposite_angle_offset: opposite_offset,
        flags,
    })
}

/// Read the prologue that names both angle lanes. A frame without this prologue
/// states no flags; a frame with it admits only the single-angle mode.
fn draft_flags(
    bytes: &[u8],
    scope: &DesignParameterScope,
    angle_record_index: u32,
    opposite_angle_record_index: u32,
) -> Result<Option<DesignDraftFlags>, ()> {
    let start = usize::try_from(scope.byte_offset()).map_err(|_| ())?;
    let at = |offset: usize| start + offset;
    let names_lanes = bytes.get(at(prefix::ZERO_RUN_10)..at(prefix::FORM)) == Some(&[0; 10])
        && View::u32_le_at(bytes, at(prefix::FORM)) == Some(prefix::FORM_VALUE)
        && View::u32_le_at(bytes, at(prefix::CONSTANT_ONE)) == Some(1)
        && marked_record_reference(bytes, at(prefix::ANGLE_REFERENCE)) == Some(angle_record_index)
        && marked_record_reference(bytes, at(prefix::OPPOSITE_ANGLE_REFERENCE))
            == Some(opposite_angle_record_index);
    if !names_lanes {
        return Ok(None);
    }
    let flag = |offset| match bytes.get(at(offset)) {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(()),
    };
    let flipped = flag(prefix::FLIPPED)?;
    let tangent_chain = flag(prefix::TANGENT_CHAIN)?;
    if View::u32_le_at(bytes, at(prefix::MODE)) != Some(MODE_SINGLE_ANGLE) {
        return Err(());
    }
    Ok(Some(DesignDraftFlags {
        flipped,
        tangent_chain,
    }))
}

pub(super) fn contains_consecutive_guid_pair(bytes: &[u8]) -> bool {
    let relaxed_guid_end = |at: usize| {
        let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        if !(36..=38).contains(&count) {
            return None;
        }
        let start = at.checked_add(4)?;
        let end = start.checked_add(count.checked_mul(2)?)?;
        let units = bytes.get(start..end)?;
        units
            .chunks_exact(2)
            .all(|unit| {
                unit[1] == 0 && (unit[0].is_ascii_alphanumeric() || matches!(unit[0], b'-' | b'_'))
            })
            .then_some(end)
    };
    (0..bytes.len()).any(|at| {
        relaxed_guid_end(at).is_some_and(|after_first| relaxed_guid_end(after_first).is_some())
    })
}
