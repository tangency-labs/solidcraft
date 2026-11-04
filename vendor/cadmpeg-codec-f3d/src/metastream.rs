// SPDX-License-Identifier: Apache-2.0
//! Shared Fusion `MetaStream` segment framing.

use cadmpeg_core::decode::{bounded_len, u64_from_index};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_hyphenated, lp_ascii_strict, lp_utf16_bounded};
use crate::records::entity_header::SegmentTypeData;

/// Serializer magic that selects the modern `MetaStream` header group.
pub(crate) const MODERN_SERIALIZER_MAGIC: u32 = 1234;

/// A short version context without zero prefixes: u64 token, two LP-UTF16
/// 36-unit GUIDs, and a u32 revision.
const MIN_VERSION_CONTEXT_LEN: usize = 8 + 2 * (4 + 36 * 2) + 4;

/// One record-index entry locating a header in the sibling `BulkStream`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordIndexEntry {
    pub(crate) entity_id: u64,
    pub(crate) bulk_offset: u64,
}

/// One completely framed `MetaStream` segment.
#[derive(Clone)]
pub(crate) struct MetaStream {
    pub(crate) types: Vec<SegmentTypeData>,
    /// Live sibling records, in strictly increasing `BulkStream` order.
    pub(crate) records: Vec<RecordIndexEntry>,
    /// Nested class-record headers, in strictly increasing `BulkStream` order.
    pub(crate) secondary_records: Vec<RecordIndexEntry>,
}

/// One exact sibling-BulkStream extent from the primary record index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrimaryRecordFrame {
    pub(crate) entity_id: u64,
    pub(crate) start: usize,
    /// End of the class-member sequence, before a secondary indexed header.
    pub(crate) member_end: usize,
    /// End of the complete top-level primary record.
    pub(crate) end: usize,
}

/// Resolve the primary index to nonempty, strictly ordered sibling-BulkStream
/// extents.
pub(crate) fn primary_record_frames(
    meta: &MetaStream,
    bulk_len: usize,
) -> Result<Vec<PrimaryRecordFrame>, CodecError> {
    let mut frames = Vec::new();

    let mut primary_by_entity = std::collections::HashMap::new();

    for (ordinal, record) in meta.records.iter().enumerate() {
        if primary_by_entity
            .insert(record.entity_id, ordinal)
            .is_some()
        {
            return Err(CodecError::Malformed(
                "F3D primary record index repeats an entity ID".into(),
            ));
        }
        let start = usize::try_from(record.bulk_offset)
            .map_err(|_| CodecError::Malformed("F3D primary record offset exceeds usize".into()))?;
        let end = meta.records.get(ordinal + 1).map_or_else(
            || Ok(bulk_len),
            |next| {
                usize::try_from(next.bulk_offset).map_err(|_| {
                    CodecError::Malformed("F3D primary record end exceeds usize".into())
                })
            },
        )?;
        if start >= end || end > bulk_len {
            return Err(CodecError::Malformed(
                "F3D primary record extents are not nonempty and strictly increasing within the BulkStream"
                    .into(),
            ));
        }
        frames.push(PrimaryRecordFrame {
            entity_id: record.entity_id,
            start,
            member_end: end,
            end,
        });
    }

    let mut previous_secondary_offset = None;

    let mut secondary_entities = std::collections::HashSet::new();

    for record in &meta.secondary_records {
        let secondary = usize::try_from(record.bulk_offset).map_err(|_| {
            CodecError::Malformed("F3D secondary record offset exceeds usize".into())
        })?;
        if previous_secondary_offset.is_some_and(|previous| secondary <= previous) {
            return Err(CodecError::Malformed(
                "F3D secondary record offsets are not strictly increasing".into(),
            ));
        }
        previous_secondary_offset = Some(secondary);
        if !secondary_entities.insert(record.entity_id) {
            return Err(CodecError::Malformed(
                "F3D secondary record index repeats an entity ID".into(),
            ));
        }
        let Some(&ordinal) = primary_by_entity.get(&record.entity_id) else {
            return Err(CodecError::Malformed(
                "F3D secondary record index names an entity absent from the primary index".into(),
            ));
        };
        let frame = &mut frames[ordinal];
        if secondary <= frame.start || secondary >= frame.end {
            return Err(CodecError::Malformed(
                "F3D secondary record offset is not strictly inside its primary record".into(),
            ));
        }
        frame.member_end = secondary;
    }
    Ok(frames)
}

fn take_counted_run(bytes: &[u8], at: &mut usize, stride: usize) -> Option<()> {
    let count = usize::try_from(View::u32_le_at(bytes, *at)?).ok()?;
    let start = at.checked_add(4)?;
    let end = count.checked_mul(stride)?.checked_add(start)?;
    bytes.get(start..end)?;
    *at = end;
    Some(())
}

fn take_record_index(bytes: &[u8], at: &mut usize) -> Option<Vec<RecordIndexEntry>> {
    let count = View::u32_le_at(bytes, *at)?;
    let records_at = at.checked_add(4)?;
    let Ok(count_usize) = usize::try_from(count) else {
        return None;
    };
    let records_end = count_usize
        .checked_mul(16)
        .and_then(|size| records_at.checked_add(size))?;
    bytes.get(records_at..records_end)?;

    let mut records = Vec::new();

    let mut view = View::over_retained(bytes);
    view.seek(records_at)?;
    for _ in 0..count {
        let entity_id = view.u64_le()?;
        let bulk_offset = view.u64_le()?;
        records.push(RecordIndexEntry {
            entity_id,
            bulk_offset,
        });
    }
    *at = view.position();
    Some(records)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParseFailure {
    field: &'static str,
    offset: usize,
}

fn lp_graphic(
    bytes: &[u8],
    at: usize,
    bounds: std::ops::RangeInclusive<usize>,
) -> Option<(&str, usize)> {
    lp_ascii_strict(bytes, at, bounds)
        .filter(|(value, _)| value.as_bytes().iter().all(u8::is_ascii_graphic))
}

fn require<T>(value: Option<T>, field: &'static str, offset: usize) -> Result<T, ParseFailure> {
    value.ok_or(ParseFailure { field, offset })
}

fn take_version_guid(
    bytes: &[u8],
    at: &mut usize,
    field: &'static str,
    allow_zero_prefix: bool,
) -> Result<(), ParseFailure> {
    let initial = *at;
    for prefix_len in [0, 4] {
        if prefix_len != 0 && (!allow_zero_prefix || View::u32_le_at(bytes, initial) != Some(0)) {
            continue;
        }
        let Some(guid_at) = initial.checked_add(prefix_len) else {
            continue;
        };
        let Some((guid, next)) = lp_utf16_bounded(bytes, guid_at, 36..=36) else {
            continue;
        };
        if is_guid_hyphenated(&guid) {
            *at = next;
            return Ok(());
        }
    }
    Err(ParseFailure {
        field,
        offset: initial,
    })
}

fn take_version_urn(bytes: &[u8], at: &mut usize) -> Result<(), ParseFailure> {
    let initial = *at;
    for prefix_len in [0, 4] {
        if prefix_len != 0 && View::u32_le_at(bytes, initial) != Some(0) {
            continue;
        }
        let Some(urn_at) = initial.checked_add(prefix_len) else {
            continue;
        };
        let Some((urn, next)) = lp_utf16_bounded(bytes, urn_at, 1..=1024) else {
            continue;
        };
        let urn = urn.as_bytes();
        if urn.len() > 4
            && urn[..4].eq_ignore_ascii_case(b"urn:")
            && urn[4..].iter().all(u8::is_ascii_graphic)
        {
            *at = next;
            return Ok(());
        }
    }
    Err(ParseFailure {
        field: "version-context version URN",
        offset: initial,
    })
}

fn take_version_context(bytes: &[u8], at: &mut usize) -> Result<(), ParseFailure> {
    let count_at = *at;
    let count = require(View::u32_le_at(bytes, *at), "version-context count", *at)?;
    *at = require(at.checked_add(4), "version-context count", *at)?;
    let remaining = require(
        bytes.len().checked_sub(*at),
        "version-context count",
        count_at,
    )?;
    require(
        bounded_len(u64::from(count), MIN_VERSION_CONTEXT_LEN, remaining),
        "version-context count",
        count_at,
    )?;
    for _ in 0..count {
        let token_end = require(at.checked_add(8), "version-context token", *at)?;
        require(bytes.get(*at..token_end), "version-context token", *at)?;
        *at = token_end;
        take_version_guid(bytes, at, "version-context asset GUID", true)?;

        // Legacy full contexts omit the separate revision GUID and place the
        // version URN directly after the asset GUID.
        let legacy_full_at = *at;
        if take_version_urn(bytes, at).is_ok() {
            take_version_guid(bytes, at, "version-context asset revision GUID", true)?;
            require(View::u32_le_at(bytes, *at), "version-context revision", *at)?;
            *at = require(at.checked_add(4), "version-context revision", *at)?;
            continue;
        }
        *at = legacy_full_at;
        take_version_guid(bytes, at, "version-context revision GUID", true)?;

        let full_at = *at;
        let full: Result<(), ParseFailure> = (|| {
            take_version_urn(bytes, at)?;
            take_version_guid(bytes, at, "version-context asset revision GUID", true)?;
            require(View::u32_le_at(bytes, *at), "version-context revision", *at)?;
            *at = require(at.checked_add(4), "version-context revision", *at)?;
            Ok(())
        })();
        if full.is_err() {
            *at = full_at;
            require(View::u32_le_at(bytes, *at), "version-context revision", *at)?;
            *at = require(at.checked_add(4), "version-context revision", *at)?;
        }
    }
    Ok(())
}

fn parse_segment_header(bytes: &[u8]) -> Result<(u32, usize), ParseFailure> {
    let (_, at) = require(lp_graphic(bytes, 0, 1..=256), "short segment type name", 0)?;
    let at = require(at.checked_add(4), "segment id", at)?;
    let (_, at) = require(lp_utf16_bounded(bytes, at, 0..=256), "asset GUID", at)?;
    let magic = require(View::u32_le_at(bytes, at), "serializer magic", at)?;
    let at = require(
        at.checked_add(if magic == MODERN_SERIALIZER_MAGIC {
            16
        } else {
            8
        }),
        "serializer integer group",
        at,
    )?;
    require(
        bytes.get(..at),
        "serializer integer group",
        at.min(bytes.len()),
    )?;
    Ok((magic, at))
}

fn parse_error(failure: ParseFailure, stream: &str) -> CodecError {
    CodecError::malformed(format_args!(
        "invalid F3D MetaStream {} at byte {}: {stream}",
        failure.field, failure.offset
    ))
}

/// Read the serializer magic from a `MetaStream` header.
pub(crate) fn serializer_magic(bytes: &[u8], stream: &str) -> Result<u32, CodecError> {
    parse_segment_header(bytes)
        .map(|(magic, _)| magic)
        .map_err(|failure| parse_error(failure, stream))
}

fn parse_inner(bytes: &[u8]) -> Result<MetaStream, ParseFailure> {
    // Header: short segment type name, segment id, asset GUID, serializer
    // magic and its magic-gated integer group, full segment type name, add-in
    // name, and the segment type code.
    let (_, at) = parse_segment_header(bytes)?;
    let (_, at) = require(lp_graphic(bytes, at, 1..=256), "full segment type name", at)?;
    let (_, at) = require(lp_graphic(bytes, at, 0..=256), "add-in name", at)?;
    let mut at = require(at.checked_add(8), "segment type code", at)?;
    require(bytes.get(..at), "segment type code", at.min(bytes.len()))?;

    let count = require(View::u32_le_at(bytes, at), "type count", at)?;
    at = require(at.checked_add(4), "type count", at)?;

    let mut types = Vec::new();

    for _ in 0..count {
        let entry_at = at;
        let type_guid_offset = require(at.checked_add(4), "type GUID", at)?;
        let (type_guid, next) = require(
            lp_graphic(bytes, at, 1..=256).and_then(|(guid, next)| {
                crate::records::mesh::DesignRelaxedGuidText::try_from(guid.to_owned())
                    .ok()
                    .map(|guid| (guid, next))
            }),
            "type GUID",
            at,
        )?;
        at = next;
        let base_type_guid_offset = require(at.checked_add(4), "base type GUID", at)?;
        let (base_type_guid, next) = require(
            lp_graphic(bytes, at, 0..=256).and_then(|(guid, next)| {
                if guid.is_empty() {
                    Some((None, next))
                } else {
                    crate::records::mesh::DesignRelaxedGuidText::try_from(guid.to_owned())
                        .ok()
                        .map(|guid| (Some(guid), next))
                }
            }),
            "base type GUID",
            at,
        )?;
        at = next;
        let version_offset = at;
        let version = require(View::u32_le_at(bytes, at), "type version", at)?;
        at = require(at.checked_add(4), "type version", at)?;
        let (module, next) = require(lp_graphic(bytes, at, 0..=256), "type module", at)?;
        at = next;
        let id_count = usize::try_from(require(
            View::u32_le_at(bytes, at),
            "type entity count",
            at,
        )?)
        .map_err(|_| ParseFailure {
            field: "type entity count",
            offset: at,
        })?;
        let ids_at = require(at.checked_add(4), "type entity count", at)?;
        let ids_end = require(
            id_count
                .checked_mul(8)
                .and_then(|length| length.checked_add(ids_at)),
            "type entity ids",
            ids_at,
        )?;
        require(bytes.get(ids_at..ids_end), "type entity ids", ids_at)?;

        let mut entity_rows = Vec::new();

        let mut id_view = View::over_retained(bytes);
        require(id_view.seek(ids_at), "type entity ids", ids_at)?;
        for index in 0..id_count {
            let value = require(id_view.u64_le(), "type entity ids", id_view.position())?;
            let offset = ids_at
                .checked_add(index.checked_mul(8).ok_or(ParseFailure {
                    field: "type entity ids",
                    offset: ids_at,
                })?)
                .ok_or(ParseFailure {
                    field: "type entity ids",
                    offset: ids_at,
                })?;
            entity_rows.push(crate::records::identity::Located {
                value,
                offset: u64::try_from(offset).map_err(|_| ParseFailure {
                    field: "type entity ids",
                    offset: ids_at,
                })?,
            });
        }
        at = ids_end;
        types.push(SegmentTypeData {
            byte_offset: u64_from_index(entry_at),
            type_guid,
            type_guid_offset: u64_from_index(type_guid_offset),
            base_type_guid: base_type_guid.map_or(
                crate::records::entity_header::BaseTypeGuid::Absent,
                |value| crate::records::entity_header::BaseTypeGuid::Guid {
                    value,
                    offset: u64_from_index(base_type_guid_offset),
                },
            ),
            version,
            version_offset: u64_from_index(version_offset),
            module: module.to_owned(),
            entities: crate::records::identity::ReferenceRun::located(entity_rows),
        });
    }

    // Named entities, the primary record index, and the secondary index.
    let named_entities_at = at;
    require(
        take_counted_run(bytes, &mut at, 8),
        "named-entity index",
        named_entities_at,
    )?;
    let primary_index_at = at;
    let records = require(
        take_record_index(bytes, &mut at),
        "primary record index",
        primary_index_at,
    )?;
    let secondary_index_at = at;
    let secondary_records = require(
        take_record_index(bytes, &mut at),
        "secondary record index",
        secondary_index_at,
    )?;

    // A legacy segment can end after the secondary index, after the
    // next-entity counter, or after the version-context/property suffix.
    if at < bytes.len() {
        let end = require(at.checked_add(8), "next-entity counter", at)?;
        require(bytes.get(at..end), "next-entity counter", at)?;
        at += 8;
    }
    if at < bytes.len() {
        take_version_context(bytes, &mut at)?;
        if at < bytes.len() {
            let properties = require(View::u32_le_at(bytes, at), "property count", at)?;
            at = require(at.checked_add(4), "property count", at)?;
            for _ in 0..properties {
                let (_, next) = require(lp_graphic(bytes, at, 0..=256), "property name", at)?;
                at = require(next.checked_add(4), "property value", next)?;
                require(bytes.get(..at), "property value", at.min(bytes.len()))?;
            }
        }
    }
    if at != bytes.len() {
        return Err(ParseFailure {
            field: "trailing bytes",
            offset: at,
        });
    }
    Ok(MetaStream {
        types,
        records,
        secondary_records,
    })
}

/// Parse one complete `MetaStream` segment and reject any unframed remainder.
pub(crate) fn parse(bytes: &[u8], stream: &str) -> Result<MetaStream, CodecError> {
    parse_inner(bytes).map_err(|failure| parse_error(failure, stream))
}

#[cfg(test)]
mod tests {
    use super::{MetaStream, RecordIndexEntry};
    use crate::test_support::streams_test::design_metastream;
    use crate::test_support::{lp_ascii, lp_utf16};

    use super::primary_record_frames;

    use super::parse;

    fn stream_prefix() -> Vec<u8> {
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "ACT");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
        bytes.extend_from_slice(&1234u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 12]);
        lp_ascii(&mut bytes, "FusionACTSegmentType");
        lp_ascii(&mut bytes, "Fusion");
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&[0; 16]);
        bytes
    }

    #[test]
    fn rejects_unframed_empty_input() {
        assert!(parse(&[], "empty").is_err());
    }

    #[test]
    fn parses_present_version_context_before_properties() {
        let mut bytes = stream_prefix();
        bytes.extend_from_slice(&15u64.to_le_bytes());
        let presence_at = bytes.len();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        let context_at = bytes.len();
        bytes.extend_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
        let asset_guid_at = bytes.len();
        for value in [
            "11111111-2222-3333-4444-555555555555",
            "66666666-7777-8888-9999-aaaaaaaaaaaa",
        ] {
            lp_utf16(&mut bytes, value);
        }
        lp_utf16(&mut bytes, "urn:synthetic:version:2");
        lp_utf16(&mut bytes, "bbbbbbbb-cccc-dddd-eeee-ffffffffffff");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        let properties_at = bytes.len();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for (name, value) in [("Application", 1u32), ("Server", 1)] {
            lp_ascii(&mut bytes, name);
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        let parsed = parse(&bytes, "version-context").expect("framed version context");
        assert!(parsed.types.is_empty());
        assert!(parsed.records.is_empty());

        let mut padded = bytes.clone();
        padded.splice(asset_guid_at..asset_guid_at, [0; 4]);
        parse(&padded, "padded-version-context").expect("zero-padded version context");

        let mut alternate_presence = bytes.clone();
        alternate_presence[presence_at..presence_at + 4].copy_from_slice(&4u32.to_le_bytes());
        let context = bytes[context_at..properties_at].to_vec();
        for _ in 0..3 {
            alternate_presence.splice(properties_at..properties_at, context.iter().copied());
        }
        parse(&alternate_presence, "alternate-version-context").expect("four version contexts");

        let mut short = stream_prefix();
        short.extend_from_slice(&15u64.to_le_bytes());
        short.extend_from_slice(&1u32.to_le_bytes());
        short.extend_from_slice(&0x8877_6655_4433_2211u64.to_le_bytes());
        for guid in [
            "11111111-2222-3333-4444-555555555555",
            "66666666-7777-8888-9999-aaaaaaaaaaaa",
        ] {
            short.extend_from_slice(&0u32.to_le_bytes());
            lp_utf16(&mut short, guid);
        }
        short.extend_from_slice(&0u32.to_le_bytes());
        parse(&short, "short-version-context").expect("short version context");

        let mut legacy_full = stream_prefix();
        legacy_full.extend_from_slice(&15u64.to_le_bytes());
        legacy_full.extend_from_slice(&1u32.to_le_bytes());
        legacy_full.extend_from_slice(&0x7766_5544_3322_1100u64.to_le_bytes());
        legacy_full.extend_from_slice(&0u32.to_le_bytes());
        lp_utf16(&mut legacy_full, "11111111-2222-3333-4444-555555555555");
        lp_utf16(&mut legacy_full, "urn:synthetic:legacy-version:3");
        lp_utf16(&mut legacy_full, "bbbbbbbb-cccc-dddd-eeee-ffffffffffff");
        legacy_full.extend_from_slice(&3u32.to_le_bytes());
        legacy_full.extend_from_slice(&0u32.to_le_bytes());
        parse(&legacy_full, "legacy-full-version-context")
            .expect("legacy full version context without a revision GUID");

        let mut invalid_presence = stream_prefix();
        invalid_presence.extend_from_slice(&15u64.to_le_bytes());
        invalid_presence.extend_from_slice(&65u32.to_le_bytes());
        assert!(parse(&invalid_presence, "invalid-version-context").is_err());
        assert!(parse(&bytes[..bytes.len() - 1], "truncated-version-context").is_err());
    }

    #[test]
    fn rejects_primary_index_count_beyond_the_stream_extent() {
        let mut bytes = stream_prefix();
        let primary_count = bytes.len() - 8;
        bytes[primary_count..primary_count + 4].copy_from_slice(&u32::MAX.to_le_bytes());

        assert!(parse(&bytes, "oversized-primary-index").is_err());
    }

    fn version_contexts(count: u32) -> (Vec<u8>, usize) {
        let mut bytes = stream_prefix();
        bytes.extend_from_slice(&17u64.to_le_bytes());
        let count_at = bytes.len();
        bytes.extend_from_slice(&count.to_le_bytes());
        for ordinal in 0..count {
            bytes.extend_from_slice(&19u64.to_le_bytes());
            lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
            lp_utf16(&mut bytes, "66666666-7777-8888-9999-aaaaaaaaaaaa");
            if ordinal % 2 == 0 {
                lp_utf16(&mut bytes, "urn:synthetic:version:23");
                lp_utf16(&mut bytes, "bbbbbbbb-cccc-dddd-eeee-ffffffffffff");
            }
            bytes.extend_from_slice(&23u32.to_le_bytes());
        }
        (bytes, count_at)
    }

    #[test]
    fn parses_version_context_count_above_64() {
        let (mut bytes, _) = version_contexts(67);
        parse(&bytes, "counted-contexts").expect("contexts close the stream");

        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_ascii(&mut bytes, "Synthetic");
        bytes.extend_from_slice(&29u32.to_le_bytes());
        let parsed =
            parse(&bytes, "counted-contexts-properties").expect("properties close the stream");
        assert!(parsed.types.is_empty());
        assert!(parsed.records.is_empty());
        assert!(parsed.secondary_records.is_empty());
    }

    #[test]
    fn rejects_version_context_count_mismatches_and_unframed_tail() {
        let (bytes, count_at) = version_contexts(67);
        for count in [66u32, 68, u32::MAX] {
            let mut invalid = bytes.clone();
            invalid[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
            assert!(parse(&invalid, "context-count-mismatch").is_err());
        }
        assert!(parse(&bytes[..bytes.len() - 1], "truncated-contexts").is_err());
        let mut trailing = bytes;
        trailing.push(0);
        assert!(parse(&trailing, "context-trailing-byte").is_err());
    }

    #[test]
    fn retains_both_record_indexes() {
        let mut bytes = stream_prefix();
        bytes.truncate(bytes.len() - 8);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&11u64.to_le_bytes());
        bytes.extend_from_slice(&3u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&11u64.to_le_bytes());
        bytes.extend_from_slice(&5u64.to_le_bytes());

        let parsed = parse(&bytes, "indexed").expect("both indexes are framed");
        assert_eq!(
            parsed.records,
            [RecordIndexEntry {
                entity_id: 11,
                bulk_offset: 3,
            }]
        );
        assert_eq!(
            parsed.secondary_records,
            [RecordIndexEntry {
                entity_id: 11,
                bulk_offset: 5,
            }]
        );
    }

    #[test]
    fn primary_index_frames_use_the_secondary_header_as_the_member_boundary() {
        let meta = MetaStream {
            types: Vec::new(),
            records: vec![
                RecordIndexEntry {
                    entity_id: 11,
                    bulk_offset: 3,
                },
                RecordIndexEntry {
                    entity_id: 12,
                    bulk_offset: 9,
                },
            ],
            secondary_records: vec![RecordIndexEntry {
                entity_id: 11,
                bulk_offset: 7,
            }],
        };

        let frames = primary_record_frames(&meta, 14).expect("ordered primary extents");
        assert_eq!(frames[0].start, 3);
        assert_eq!(frames[0].member_end, 7);
        assert_eq!(frames[0].end, 9);
        assert_eq!(frames[1].start, 9);
        assert_eq!(frames[1].member_end, 14);
        assert_eq!(frames[1].end, 14);

        let empty = MetaStream {
            types: Vec::new(),
            records: Vec::new(),
            secondary_records: Vec::new(),
        };
        assert!(primary_record_frames(&empty, 0)
            .expect("an empty primary index has no frames")
            .is_empty());

        let empty_last = MetaStream {
            types: Vec::new(),
            records: vec![RecordIndexEntry {
                entity_id: 11,
                bulk_offset: 14,
            }],
            secondary_records: Vec::new(),
        };
        assert!(primary_record_frames(&empty_last, 14).is_err());

        let repeated_offset = MetaStream {
            types: Vec::new(),
            records: vec![
                RecordIndexEntry {
                    entity_id: 11,
                    bulk_offset: 3,
                },
                RecordIndexEntry {
                    entity_id: 12,
                    bulk_offset: 3,
                },
            ],
            secondary_records: Vec::new(),
        };
        assert!(primary_record_frames(&repeated_offset, 14).is_err());

        let secondary_outside_primary = MetaStream {
            types: Vec::new(),
            records: vec![RecordIndexEntry {
                entity_id: 11,
                bulk_offset: 3,
            }],
            secondary_records: vec![RecordIndexEntry {
                entity_id: 11,
                bulk_offset: 14,
            }],
        };
        assert!(matches!(
            primary_record_frames(&secondary_outside_primary, 14),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
    #[test]
    fn design_type_table_attributes_each_entry_to_its_own_type() {
        let first = "11111111-1111-1111-1111-111111111111";
        let second = "22222222-2222-2222-2222-222222222222";
        let third = "33333333-3333-3333-3333-333333333333";
        let base = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        // The middle entry is a root type: its base GUID is the empty string, so
        // its length prefix is a four-byte zero run rather than a GUID.
        let bytes = design_metastream(&[
            (first, base, 3, "Fusion", &[10, 11]),
            (second, "", 7, "MSketch", &[20]),
            (third, second, 11, "Body", &[30, 31, 32]),
        ]);
        let types = parse(&bytes, "synthetic MetaStream")
            .expect("a segment closing on its own end parses")
            .types;
        assert_eq!(types.len(), 3);

        // Every field of an entry belongs to that entry, not to its successor.
        assert_eq!(types[0].type_guid.as_str(), first);
        assert_eq!(
            types[0]
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str),
            Some(base)
        );
        assert_eq!(types[0].version, 3);
        assert_eq!(types[0].module, "Fusion");
        assert_eq!(
            types[0].entities.values().copied().collect::<Vec<_>>(),
            [10, 11]
        );

        assert_eq!(types[1].type_guid.as_str(), second);
        assert_eq!(
            types[1].base_type_guid,
            crate::records::entity_header::BaseTypeGuid::Absent
        );

        assert_eq!(types[1].version, 7);
        assert_eq!(
            types[1].module,
            crate::records::entity_header::DESIGN_MODULE_SKETCH
        );
        assert_eq!(
            types[1].entities.values().copied().collect::<Vec<_>>(),
            [20]
        );

        assert_eq!(types[2].type_guid.as_str(), third);
        assert_eq!(
            types[2]
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str),
            Some(second)
        );
        assert_eq!(types[2].version, 11);
        assert_eq!(
            types[2].module,
            crate::records::entity_header::DESIGN_MODULE_BODY
        );
        assert_eq!(
            types[2].entities.values().copied().collect::<Vec<_>>(),
            [30, 31, 32]
        );

        // Every reported offset addresses the field it names.
        let string_at = |offset: u64, length: usize| {
            std::str::from_utf8(
                &bytes[usize::try_from(offset).expect("fixture offset fits address space")
                    ..usize::try_from(offset).expect("fixture offset fits address space") + length],
            )
            .expect("ASCII field")
            .to_owned()
        };
        let u32_at = |offset: u64| {
            u32::from_le_bytes(
                bytes[usize::try_from(offset).expect("fixture offset fits address space")
                    ..usize::try_from(offset).expect("fixture offset fits address space") + 4]
                    .try_into()
                    .expect("4-byte field"),
            )
        };
        for design_type in &types {
            assert!(design_type.byte_offset < design_type.type_guid_offset);
            assert_eq!(
                string_at(design_type.type_guid_offset, 36),
                design_type.type_guid.as_str()
            );
            assert_eq!(u32_at(design_type.version_offset), design_type.version);
            if let crate::records::entity_header::BaseTypeGuid::Guid { value, offset } =
                &design_type.base_type_guid
            {
                assert_eq!(string_at(*offset, 36), value.as_str());
            }
            let Some(entities) = design_type.entities.located_rows() else {
                panic!("parsed entity locations");
            };
            for crate::records::identity::Located {
                value: entity_id,
                offset,
            } in entities
            {
                assert_eq!(
                    u64::from_le_bytes(
                        bytes[usize::try_from(*offset).expect("fixture offset fits address space")
                            ..usize::try_from(*offset).expect("fixture offset fits address space")
                                + 8]
                            .try_into()
                            .expect("8-byte field")
                    ),
                    *entity_id
                );
            }
        }

        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(parse(&trailing, "trailing MetaStream").is_err());
        assert!(parse(&bytes[..bytes.len() - 1], "truncated MetaStream").is_err());
        assert!(parse(&bytes[..bytes.len() - 4], "property-free MetaStream").is_ok());
    }
}
