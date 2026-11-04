// SPDX-License-Identifier: Apache-2.0
//! Exact point-data levels and work-point constructions.

use super::parameter_scope::payload_prologue;
use crate::bytes::finite_reals_at;
use crate::bytes::take_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignWorkPointConstruction;
use crate::records::feature::work_geometry::DesignWorkPointInput;
use crate::records::feature::work_geometry::DesignWorkPointRule;
use cadmpeg_core::decode::View;
use std::collections::HashMap;
use std::ops::RangeInclusive;

/// Type GUID of the point-data class a `WorkPoint` scope references. Every
/// record of this class carries the `point3d` member sequence below.
pub(super) const POINT_DATA_TYPE_GUID: &str = "69EE2FA7-BCC7-449E-9CA9-976CEFDFED44";

fn graphic_ascii_end(bytes: &[u8], at: usize, bounds: RangeInclusive<usize>) -> Option<usize> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&length) {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(length)?;
    bytes
        .get(start..end)?
        .iter()
        .all(u8::is_ascii_graphic)
        .then_some(end)
}

/// The base class level of a point-data record, read under one record version.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PointDataLevel {
    /// Byte offset of `point3d`'s first coordinate.
    position_at: usize,
    /// Construction rule that produced the point.
    reference_type: u32,
    /// Byte offset of the serialized construction rule.
    reference_type_at: usize,
    /// Counted input-reference run that closes the level.
    inputs: Vec<DesignWorkPointInput>,
}

/// Read the base class level of the point-data class at `start` under one
/// record version.
///
/// The level closes with a counted reference run. The serialized count is the
/// only framing authority for that run: `reference_type` selects the
/// construction rule, but its rule-specific arity is not encoded in the class
/// member order. The count is bounded by the frame before allocation and each
/// marked reference must resolve to a record index.
fn point_data_level(
    bytes: &[u8],
    start: usize,
    end: usize,
    version: u32,
) -> Option<PointDataLevel> {
    let body = bytes.get(..end)?;
    let mut cursor = payload_prologue(bytes, start, end)?;
    if version >= 2 {
        cursor = cursor.checked_add(4)?;
    }
    cursor = cursor.checked_add(16)?;
    if version >= 1 {
        take_reference(body, &mut cursor)?;
    }
    let position_at = cursor;
    cursor = cursor.checked_add(24)?;
    let reference_type_at = cursor;
    let reference_type = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    if version >= 3 {
        cursor = cursor.checked_add(24)?;
    }
    let arity = usize::try_from(View::u32_le_at(body, cursor)?).ok()?;
    cursor = cursor.checked_add(4)?;
    if arity == 0 || arity > end.checked_sub(cursor)? {
        return None;
    }

    let mut inputs = Vec::new();
    for _ in 0..arity {
        let reference_offset = cursor.checked_add(1)?;
        let reference = take_reference(body, &mut cursor)?;
        inputs.push(
            DesignWorkPointInput::try_new(
                u32::try_from(reference.target()?).ok()?,
                u64::try_from(reference_offset).ok()?,
                None,
            )
            .ok()?,
        );
    }
    Some(PointDataLevel {
        position_at,
        reference_type,
        reference_type_at,
        inputs,
    })
}

/// The coordinate of a `WorkPoint`'s point-data record.
///
/// The versions differ by members before and after `point3d`, so a version that
/// moves `point3d` names a different coordinate. `stream_types` carries the
/// record's own type GUID and version from its segment's type table: the GUID
/// identifies the point-data class and the version settles the frame outright.
/// Where the record's entity is not registered there, the class cannot be named
/// and every version whose member sequence fits the frame stays a candidate, so
/// the frame is read only when they agree on the offset.
pub(super) fn exact_work_point_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignWorkPointConstruction> {
    if scope.kind() != scope::DesignFeatureKind::WorkPoint {
        return None;
    }
    exact_point_data_construction(
        bytes,
        records,
        scope.reference_members().values(),
        stream_types,
    )
}

pub(in crate::design::decode) fn exact_point_data_construction<'a>(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    point_record_indices: impl IntoIterator<Item = &'a u32>,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignWorkPointConstruction> {
    let mut candidate = None;
    for record_index in point_record_indices {
        for (start, paired) in records.frames(*record_index) {
            let Some(after_tag) = graphic_ascii_end(bytes, start, 0..=2000) else {
                continue;
            };
            if View::u32_le_at(bytes, after_tag) != Some(*record_index) {
                continue;
            }
            // A class tag is `256` plus an index into the segment's own type
            // table, so it names a different class in every segment and cannot
            // select the point-data class. The type GUID can.
            let stored = stream_types.get(&u64::from(*record_index)).copied();
            if stored.is_some_and(|(type_guid, _)| type_guid != POINT_DATA_TYPE_GUID) {
                continue;
            }
            // The payload begins after the record name that closes the header.
            let Some(payload_at) = graphic_ascii_end(bytes, after_tag + 8, 0..=256) else {
                continue;
            };
            let versions = match stored {
                Some((_, version)) => [Some(version), None, None, None],
                None => [Some(0), Some(1), Some(2), Some(3)],
            };
            let mut unique_level = None;
            let mut ambiguous_level = false;
            for version in versions.into_iter().flatten() {
                if let Some(level) = point_data_level(bytes, payload_at, paired, version) {
                    match &unique_level {
                        Some(previous) if previous != &level => {
                            ambiguous_level = true;
                            break;
                        }
                        Some(_) => {}
                        None => unique_level = Some(level),
                    }
                }
            }
            // Agreement is over the levels the fitting versions name, so the
            // duplicates are removed by value regardless of version order.
            let Some(level) = unique_level.filter(|_| !ambiguous_level) else {
                continue;
            };
            if let Some(position) = finite_reals_at(bytes, level.position_at) {
                let construction = DesignWorkPointConstruction {
                    point_record_index: *record_index,
                    point_record_byte_offset: u64::try_from(start).ok()?,
                    position,
                    position_offset: u64::try_from(level.position_at).ok()?,
                    rule: DesignWorkPointRule::from_serialized(level.reference_type, level.inputs)
                        .ok()?,
                    reference_type_offset: u64::try_from(level.reference_type_at).ok()?,
                };
                if candidate.replace(construction).is_some() {
                    return None;
                }
            }
        }
    }
    candidate
}

#[cfg(test)]
mod tests;
