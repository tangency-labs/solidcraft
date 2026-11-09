// SPDX-License-Identifier: Apache-2.0
//! Fixed settings of `PathPattern` scopes.

use super::shared_frames::marked_record_reference;
use crate::bytes::finite_reals_at;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::layout::{path_pattern_settings as settings_layout, path_pattern_start as start_layout};
use crate::records::feature::patterns::{
    self, PATH_PATTERN_SETTINGS_TYPE, PATH_PATTERN_START_TYPE,
};
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{u64_from_index, View};

/// Fixed settings of a `PathPattern` scope: the settings record that the scope
/// prefix names and the path-start record in the scope reference table.
pub(super) fn exact_path_pattern_settings(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    class_types: &std::collections::HashMap<u32, &crate::records::entity_header::SegmentType>,
) -> Option<patterns::DesignPathPatternSettings> {
    use crate::records::identity::Located;

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::PathPattern {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let prefix_end = usize::try_from(scope.reference_count_offset()).ok()?;
    let prefix = bytes.get(..prefix_end)?;
    let reference_at = if prefix.get(start + 11..start + 20)? == [0; 9]
        && View::u32_le_at(prefix, start + 20)? == 1
        && *prefix.get(start + 24)? == 0
    {
        start + 25
    } else if prefix.get(start + 11..start + 21)? == [0; 10] {
        start + 21
    } else {
        return None;
    };
    let settings_record_index = marked_record_reference(prefix, reference_at)?;
    let settings_at = typed_record_frame(
        bytes,
        records,
        class_types,
        settings_record_index,
        PATH_PATTERN_SETTINGS_TYPE,
        settings_layout::LEN,
    )?;
    let settings = bytes.get(settings_at..settings_at.checked_add(settings_layout::LEN)?)?;
    let flag = |offset: usize| {
        let value = match settings.get(offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        Some(Located {
            value,
            offset: u64_from_index(settings_at.checked_add(offset)?),
        })
    };
    if *settings.get(settings_layout::SETTINGS_MARKER)? != settings_layout::SETTINGS_MARKER_VALUE
        || settings.get(settings_layout::DISTANCE_TYPE_MARKER..settings_layout::DISTANCE_TYPE)?
            != [0xff; 4]
    {
        return None;
    }
    let total_extent = flag(settings_layout::DISTANCE_TYPE)?;
    let flipped = flag(settings_layout::FLIPPED)?;
    let path_oriented = flag(settings_layout::PATH_ORIENTED)?;
    let path_group_record_index =
        marked_record_reference(settings, settings_layout::PATH_GROUP_REFERENCE)?;
    if !scope
        .reference_members()
        .values()
        .any(|member| *member == path_group_record_index)
    {
        return None;
    }
    let mut starts = scope.reference_members().values().filter(|record_index| {
        record_frames_of_type(
            bytes,
            records,
            class_types,
            **record_index,
            PATH_PATTERN_START_TYPE.0,
        )
        .next()
        .is_some()
    });
    let start_record_index = *starts.next()?;
    if starts.next().is_some() {
        return None;
    }
    let start_record_at = typed_record_frame(
        bytes,
        records,
        class_types,
        start_record_index,
        PATH_PATTERN_START_TYPE,
        start_layout::LEN,
    )?;
    let start_parameter_at = start_record_at.checked_add(start_layout::START_PARAMETER)?;
    let [start_parameter] = finite_reals_at::<1>(bytes, start_parameter_at)?;
    Some(patterns::DesignPathPatternSettings {
        settings_record_index,
        settings_byte_offset: u64_from_index(settings_at),
        total_extent,
        flipped,
        path_oriented,
        path_group_record_index,
        start_record_index,
        start_parameter: Located {
            value: start_parameter,
            offset: u64_from_index(start_parameter_at),
        },
    })
}

/// Header offsets of the frames of `record_index` whose class tag names
/// `type_guid`, at any version.
fn record_frames_of_type<'a>(
    bytes: &'a [u8],
    records: &'a IndexedRecordOffsets,
    class_types: &'a std::collections::HashMap<u32, &crate::records::entity_header::SegmentType>,
    record_index: u32,
    type_guid: &'a str,
) -> impl Iterator<Item = (usize, &'a crate::records::entity_header::SegmentType)> + 'a {
    records.offsets(record_index).iter().filter_map(move |at| {
        let row = bytes
            .get(at + 4..at + 7)
            .and_then(|tag| std::str::from_utf8(tag).ok())
            .and_then(|tag| tag.parse::<u32>().ok())
            .and_then(|tag| class_types.get(&tag))?;
        row.type_guid
            .as_str()
            .eq_ignore_ascii_case(type_guid)
            .then_some((*at, *row))
    })
}

/// Header offset of the frame of `record_index` whose class tag names the
/// type GUID. That frame must be unique, carry `version`, and end at the next
/// indexed header `length` bytes after its own.
fn typed_record_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    class_types: &std::collections::HashMap<u32, &crate::records::entity_header::SegmentType>,
    record_index: u32,
    (type_guid, version): (&str, u32),
    length: usize,
) -> Option<usize> {
    let mut frames = record_frames_of_type(bytes, records, class_types, record_index, type_guid);
    let (at, row) = frames.next()?;
    if frames.next().is_some()
        || row.version != version
        || records.offsets(record_index).first() != Some(&at)
    {
        return None;
    }
    let frame_end = at.checked_add(length)?;
    let window = bytes.get(..frame_end.checked_add(11)?.min(bytes.len()))?;
    (next_indexed_record_offset(window, at.checked_add(11)?) == Some(frame_end)).then_some(at)
}

#[cfg(test)]
mod tests;
