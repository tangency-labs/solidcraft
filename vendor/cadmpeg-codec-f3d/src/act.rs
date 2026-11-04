// SPDX-License-Identifier: Apache-2.0
//! Fusion ACT entity table and change-version channel groups.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_hyphenated, lp_ascii_strict, lp_utf16_bounded};
use crate::container::ContainerScan;
use crate::metastream::MetaStream;
use crate::records::{
    act::{
        ActChannelGroup, ActComponentLink, ActEntity, ActGuid, ActRegistryChannel,
        ActStreamResidue, ActTableReference, ActTableRow,
    },
    identity::Located,
    references::DesignClassTag,
};

pub(crate) struct DecodedAct {
    pub(crate) entities: Vec<ActEntity>,
    pub(crate) guids: Vec<ActGuid>,
    pub(crate) registry_channels: Vec<ActRegistryChannel>,
    pub(crate) component_links: Vec<ActComponentLink>,
    pub(crate) table_references: Vec<ActTableReference>,
}

struct RecordFrame {
    start: usize,
    end: usize,
    record_index: u32,
    record_index_offset: usize,
    payload_offset: usize,
    class_tag: DesignClassTag,
}

fn decode_record_frames(
    bytes: &[u8],
    meta: &MetaStream,
    stream: &str,
) -> Result<Vec<RecordFrame>, CodecError> {
    if meta.records.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "F3D ACT MetaStream has no primary record index: {stream}"
        )));
    }
    if meta
        .records
        .windows(2)
        .any(|pair| pair[0].bulk_offset >= pair[1].bulk_offset)
    {
        return Err(CodecError::malformed(format_args!(
            "F3D ACT primary record offsets are not strictly increasing: {stream}"
        )));
    }

    let mut record_indices = BTreeSet::new();
    meta.records
        .iter()
        .enumerate()
        .map(|(ordinal, record)| {
            let start = usize::try_from(record.bulk_offset).map_err(|_| {
                CodecError::malformed(format_args!(
                    "F3D ACT record offset exceeds usize: {stream}"
                ))
            })?;
            let end = if let Some(next) = meta.records.get(ordinal + 1) {
                usize::try_from(next.bulk_offset).map_err(|_| {
                    CodecError::malformed(format_args!(
                        "F3D ACT record offset exceeds usize: {stream}"
                    ))
                })?
            } else {
                bytes.len()
            };
            if start >= end || end > bytes.len() {
                return Err(CodecError::malformed(format_args!(
                    "F3D ACT record extent is outside its BulkStream: {stream}"
                )));
            }
            let expected_index = u32::try_from(record.entity_id).map_err(|_| {
                CodecError::malformed(format_args!("F3D ACT record index exceeds u32: {stream}"))
            })?;
            if !record_indices.insert(expected_index) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate F3D ACT primary record index {expected_index}: {stream}"
                )));
            }
            let (class_tag, after_tag) = lp_ascii_strict(bytes, start, 3..=3)
                .and_then(|(tag, after)| {
                    DesignClassTag::try_from(tag.to_owned())
                        .ok()
                        .map(|tag| (tag, after))
                })
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "F3D ACT record lacks a dynamic class tag: {stream}@{start}"
                    ))
                })?;
            let payload_offset = after_tag.checked_add(4).ok_or_else(|| {
                CodecError::malformed(format_args!("F3D ACT record header overflows: {stream}"))
            })?;
            if payload_offset > end || View::u32_le_at(bytes, after_tag) != Some(expected_index) {
                return Err(CodecError::malformed(format_args!(
                    "F3D ACT record header conflicts with its MetaStream index: {stream}@{start}"
                )));
            }
            let class_index = class_tag.dynamic_ordinal().ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D ACT class tag is outside the dynamic registry: {stream}@{start}"
                ))
            })?;
            if !meta.types.get(class_index).is_some_and(|record_type| {
                record_type
                    .entities
                    .values()
                    .any(|registered| *registered == record.entity_id)
            }) {
                return Err(CodecError::malformed(format_args!(
                    "F3D ACT class tag conflicts with its MetaStream type: {stream}@{start}"
                )));
            }
            Ok(RecordFrame {
                start,
                end,
                record_index: expected_index,
                record_index_offset: after_tag,
                payload_offset,
                class_tag,
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

fn sibling_meta_name(stream: &str) -> Option<String> {
    let prefix = stream.strip_suffix("BulkStream.dat")?;
    Some(format!("{prefix}MetaStream.dat"))
}

pub(crate) fn decode(scan: &ContainerScan<'_>) -> Result<DecodedAct, CodecError> {
    let mut entities = Vec::new();
    let mut guids = Vec::new();
    let mut registry_channels = Vec::new();
    let mut component_links = Vec::new();
    let mut table_references = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_act_stream(entry))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let meta_name = sibling_meta_name(&entry.name).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D ACT BulkStream has no sibling MetaStream name: {}",
                entry.name
            ))
        })?;
        let meta_entry = scan
            .entries
            .iter()
            .find(|candidate| {
                candidate.role == ContainerRole::Metastream && candidate.name == meta_name
            })
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D ACT BulkStream has no sibling MetaStream: {}",
                    entry.name
                ))
            })?;
        let meta = crate::metastream::parse(scan.entry_bytes(&meta_entry.name)?, &meta_entry.name)?;
        let frames = decode_record_frames(bytes, &meta, &entry.name)?;
        let table_frames = frames
            .iter()
            .filter_map(|frame| table_payload_offset(bytes, frame).map(|payload| (frame, payload)))
            .collect::<Vec<_>>();
        let [(table_frame, table_payload)] = table_frames.as_slice() else {
            return Err(CodecError::malformed(format_args!(
                "F3D ACT segment must have exactly one indexed ACTTable record: {}",
                entry.name
            )));
        };
        let DecodedTable {
            entries: table,
            guids: stream_guids,
            references: stream_table_references,
            registry_channels: stream_registry_channels,
        } = decode_table(bytes, table_frame, *table_payload, &entry.name)?;
        let mut frame_indices = BTreeSet::new();
        for frame in &frames {
            frame_indices.insert(frame.record_index);
        }
        if let Some(reference) = stream_table_references
            .iter()
            .find(|reference| !frame_indices.contains(&reference.target_record))
        {
            return Err(CodecError::malformed(format_args!(
                "F3D ACTTable reference targets absent record {}: {}",
                reference.target_record, entry.name
            )));
        }
        let mut groups = Vec::new();
        for frame in &frames {
            if let Some(group) = decode_channel_group(bytes, frame, &entry.name)? {
                groups.push(group);
            }
        }
        for frame in &frames {
            if let Some(link) = decode_component_link(bytes, frame, &entry.name)? {
                component_links.push(link);
            }
        }

        for entity in merge_entities(&entry.name, table, groups)? {
            entities.push(entity);
        }
        for guid in stream_guids {
            guids.push(guid);
        }
        for reference in stream_table_references {
            table_references.push(reference);
        }
        for channel in stream_registry_channels {
            registry_channels.push(channel);
        }
    }
    Ok(DecodedAct {
        entities,
        guids,
        registry_channels,
        component_links,
        table_references,
    })
}

fn table_payload_offset(bytes: &[u8], frame: &RecordFrame) -> Option<usize> {
    let name_offset = frame.payload_offset.checked_add(4)?;
    if name_offset > frame.end || bytes.get(frame.payload_offset..name_offset)? != [0; 4] {
        return None;
    }
    let name_len = usize::try_from(View::u32_le_at(bytes, name_offset)?).ok()?;
    if name_len != b"ACTTable".len() {
        return None;
    }
    let name_start = name_offset.checked_add(4)?;
    let payload = name_start.checked_add(name_len)?;
    (bytes.get(name_start..payload)? == b"ACTTable" && payload <= frame.end).then_some(payload)
}

struct TableEntry {
    record_index: u32,
    row: ActTableRow,
    entity_id: String,
}

struct DecodedTable {
    entries: Vec<TableEntry>,
    guids: Vec<ActGuid>,
    references: Vec<ActTableReference>,
    registry_channels: Vec<ActRegistryChannel>,
}

fn decode_table(
    bytes: &[u8],
    frame: &RecordFrame,
    payload: usize,
    stream: &str,
) -> Result<DecodedTable, CodecError> {
    let malformed = |detail: &str| {
        CodecError::malformed(format_args!(
            "invalid F3D ACTTable {detail}: {stream}@{}",
            frame.start
        ))
    };
    let count_offset = payload.checked_add(2).ok_or_else(|| malformed("offset"))?;
    let mut cursor = count_offset
        .checked_add(4)
        .ok_or_else(|| malformed("offset"))?;
    if cursor > frame.end || bytes.get(payload..count_offset) != Some(&[0, 0]) {
        return Err(malformed("prologue"));
    }
    let count =
        usize::try_from(View::u32_le_at(bytes, count_offset).ok_or_else(|| malformed("count"))?)
            .map_err(|_| malformed("count"))?;
    if frame
        .end
        .checked_sub(cursor)
        .is_none_or(|left| count > left / 15)
    {
        return Err(malformed("entry count"));
    }
    let mut entries = Vec::new();
    for _ in 0..count {
        let index_offset = cursor.checked_add(1).ok_or_else(|| malformed("entry"))?;
        let entity_length_offset = cursor.checked_add(11).ok_or_else(|| malformed("entry"))?;
        if bytes.get(cursor) != Some(&1)
            || bytes.get(cursor + 5..entity_length_offset) != Some(&[0; 6])
        {
            return Err(malformed("entry reference"));
        }
        let record_index =
            View::u32_le_at(bytes, index_offset).ok_or_else(|| malformed("entry index"))?;
        let (entity_id, end) = lp_utf16_bounded(bytes, entity_length_offset, 1..=1024)
            .filter(|(_, end)| *end <= frame.end)
            .filter(|(entity_id, _)| is_entity_key(entity_id))
            .ok_or_else(|| malformed("entity key"))?;
        entries.push(TableEntry {
            record_index,
            row: ActTableRow::new(u64_from_index(index_offset)).map_err(CodecError::malformed)?,
            entity_id,
        });
        cursor = end;
    }

    let mut guids = Vec::new();
    while let Some((guid, end)) = lp_utf16_bounded(bytes, cursor, 36..=36)
        .filter(|(guid, end)| *end <= frame.end && is_guid_hyphenated(guid))
    {
        let byte_offset = cursor;
        let ordinal = u32::try_from(guids.len()).map_err(|_| malformed("GUID ordinal"))?;
        guids.push(
            ActGuid::new(
                crate::ids::native_scoped_id(stream, "act-guid", byte_offset),
                u64_from_index(byte_offset),
                ordinal,
                guid,
            )
            .map_err(|_| malformed("GUID offset"))?,
        );
        cursor = end;
    }

    let reference_count = usize::try_from(
        View::u32_le_at(bytes, cursor).ok_or_else(|| malformed("table-reference count"))?,
    )
    .map_err(|_| malformed("table-reference count"))?;
    cursor = cursor
        .checked_add(4)
        .ok_or_else(|| malformed("table-reference count"))?;
    if frame
        .end
        .checked_sub(cursor)
        .is_none_or(|left| reference_count > left / 11)
    {
        return Err(malformed("table-reference count"));
    }
    let mut table_references = Vec::new();
    for ordinal in 0..reference_count {
        let byte_offset = cursor;
        let (target_record, end) =
            marker_ref(bytes, cursor, 6, frame.end).ok_or_else(|| malformed("table reference"))?;
        table_references.push(
            ActTableReference::new(
                crate::ids::native_scoped_id(stream, "act-table-reference", byte_offset),
                u32::try_from(ordinal).map_err(|_| malformed("table-reference ordinal"))?,
                u64_from_index(byte_offset),
                target_record,
            )
            .map_err(CodecError::malformed)?,
        );
        cursor = end;
    }

    let registry_count = usize::try_from(
        View::u32_le_at(bytes, cursor).ok_or_else(|| malformed("channel-registry count"))?,
    )
    .map_err(|_| malformed("channel-registry count"))?;
    cursor = cursor
        .checked_add(4)
        .ok_or_else(|| malformed("channel-registry count"))?;
    if frame
        .end
        .checked_sub(cursor)
        .is_none_or(|left| registry_count > left / 81)
    {
        return Err(malformed("channel-registry count"));
    }
    let mut registry_names = BTreeSet::new();
    let mut registry_channels = Vec::new();
    for ordinal in 0..registry_count {
        let byte_offset = cursor;
        let (name, after_name) = lp_ascii_strict(bytes, cursor, 1..=128)
            .filter(|(name, end)| *end <= frame.end && name.is_ascii())
            .ok_or_else(|| malformed("channel-registry name"))?;
        if !registry_names.insert(name) {
            return Err(malformed("duplicate channel-registry name"));
        }
        let (guid, end) = lp_utf16_bounded(bytes, after_name, 36..=36)
            .filter(|(guid, end)| *end <= frame.end && is_guid_hyphenated(guid))
            .ok_or_else(|| malformed("channel-registry GUID"))?;
        registry_channels.push(
            ActRegistryChannel::new(
                crate::ids::native_scoped_id(stream, "act-registry-channel", byte_offset),
                u32::try_from(ordinal).map_err(|_| malformed("channel-registry ordinal"))?,
                u64_from_index(byte_offset),
                name.to_owned(),
                guid,
            )
            .map_err(|_| malformed("channel-registry entry"))?,
        );
        cursor = end;
    }
    if cursor != frame.end {
        return Err(malformed("channel-registry extent"));
    }
    Ok(DecodedTable {
        entries,
        guids,
        references: table_references,
        registry_channels,
    })
}

struct ChannelGroup {
    record_index: u32,
    record_index_offset: usize,
    entity_id: Option<Located<String, usize>>,
    class_tag: DesignClassTag,
    channels: BTreeMap<String, Located<crate::records::mesh::DesignGuidText>>,
    class_tail: Option<ActStreamResidue>,
}

fn merge_entities(
    stream: &str,
    table: Vec<TableEntry>,
    groups: Vec<ChannelGroup>,
) -> Result<Vec<ActEntity>, CodecError> {
    let mut table_by_index = BTreeMap::new();
    for item in table {
        let record_index = item.record_index;
        if table_by_index.insert(record_index, item).is_some() {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D ACTTable change-group reference {record_index}: {stream}"
            )));
        }
    }
    let mut by_index = BTreeMap::new();
    for group in groups {
        let record_index = group.record_index;
        if by_index.contains_key(&record_index) {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D ACT change group {stream}:{record_index}"
            )));
        }
        let (entity_id, row) = if let Some(item) = table_by_index.remove(&record_index) {
            if group
                .entity_id
                .as_ref()
                .is_some_and(|group_id| item.entity_id != group_id.value)
            {
                return Err(CodecError::malformed(format_args!("F3D ACTTable entity key conflicts with its change group: {stream}:{record_index}")));
            }
            (item.entity_id, Some(item.row))
        } else if let Some(entity_id) = &group.entity_id {
            (entity_id.value.clone(), None)
        } else {
            continue;
        };
        let channel_group = ActChannelGroup::try_new(
            u64_from_index(group.record_index_offset),
            group.entity_id.as_ref().map(|id| u64_from_index(id.offset)),
            group.class_tag,
            group.channels,
            group.class_tail,
        )
        .map_err(CodecError::malformed)?;
        let entity = ActEntity::try_new(
            crate::ids::native_scoped_id(stream, "act-entity", record_index),
            record_index,
            entity_id,
            row,
            channel_group,
        )
        .map_err(CodecError::malformed)?;
        by_index.insert(record_index, entity);
    }
    if let Some(record_index) = table_by_index.keys().next() {
        return Err(CodecError::malformed(format_args!(
            "F3D ACTTable reference has no change group: {stream}:{record_index}"
        )));
    }
    Ok(by_index.into_values().collect::<Vec<_>>())
}

fn decode_channel_group(
    bytes: &[u8],
    frame: &RecordFrame,
    stream: &str,
) -> Result<Option<ChannelGroup>, CodecError> {
    let count_offset = frame.payload_offset.checked_add(10).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "F3D ACT channel-group offset overflows: {stream}"
        ))
    })?;
    if count_offset
        .checked_add(4)
        .is_none_or(|end| end > frame.end)
        || bytes.get(frame.payload_offset..count_offset) != Some(&[0; 10])
    {
        return Ok(None);
    }
    let Some(count) = View::u32_le_at(bytes, count_offset).filter(|count| (1..=8).contains(count))
    else {
        return Ok(None);
    };
    let mut cursor = count_offset + 4;
    let mut channels = BTreeMap::new();
    for _ in 0..count {
        let Some((name, after_name)) = lp_ascii_strict(bytes, cursor, 1..=128)
            .filter(|(name, after)| *after <= frame.end && name.is_ascii())
        else {
            return Ok(None);
        };
        let Some((guid, after_guid)) = lp_utf16_bounded(bytes, after_name, 36..=36)
            .filter(|(guid, after)| *after <= frame.end && is_guid_hyphenated(guid))
        else {
            return Ok(None);
        };
        if channels
            .insert(
                name.to_owned(),
                Located {
                    value: guid.try_into().map_err(CodecError::malformed)?,
                    offset: u64_from_index(after_name + 4),
                },
            )
            .is_some()
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D ACT channel {name:?}: {stream}@{}",
                frame.start
            )));
        }
        cursor = after_guid;
    }
    let (entity_id, end) = if let Some((entity_id, end)) = lp_utf16_bounded(bytes, cursor, 1..=1024)
        .filter(|(entity_id, end)| *end <= frame.end && is_entity_key(entity_id))
    {
        (
            Some(Located {
                value: entity_id,
                offset: cursor + 4,
            }),
            end,
        )
    } else {
        (None, cursor)
    };
    let remainder = &bytes[end..frame.end];
    let class_tail = if remainder.iter().all(|byte| *byte == 0) {
        None
    } else {
        Some(
            ActStreamResidue::new(remainder.to_vec(), u64_from_index(end))
                .map_err(CodecError::malformed)?,
        )
    };
    Ok(Some(ChannelGroup {
        record_index: frame.record_index,
        record_index_offset: frame.record_index_offset,
        entity_id,
        class_tag: frame.class_tag.clone(),
        channels,
        class_tail,
    }))
}

fn decode_component_link(
    bytes: &[u8],
    frame: &RecordFrame,
    stream: &str,
) -> Result<Option<ActComponentLink>, CodecError> {
    macro_rules! some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let mut cursor = some!(frame.payload_offset.checked_add(10));
    if cursor > frame.end || some!(bytes.get(frame.payload_offset..cursor)) != [0; 10] {
        return Ok(None);
    }
    let (instance_root_record, next) = some!(marker_ref(bytes, cursor, 6, frame.end));
    cursor = next;
    let (entity_id, next) = some!(lp_utf16_bounded(bytes, cursor, 1..=1024));
    if next > frame.end || !is_entity_key(&entity_id) {
        return Ok(None);
    }
    cursor = next;
    let (tracked_entity_record, next) = some!(marker_ref(bytes, cursor, 5, frame.end));
    cursor = next;
    let (registry_flag, next) = some!(marker_ref(bytes, cursor, 0, frame.end));
    cursor = next;
    let (display_name, next) = some!(lp_utf16_bounded(bytes, cursor, 0..=1024));
    if next > frame.end {
        return Ok(None);
    }
    cursor = next;
    let mut components_marker = cursor;
    while components_marker < frame.end
        && bytes.get(components_marker) == Some(&0)
        && components_marker - cursor < 8
    {
        components_marker += 1;
    }
    let (components_root_record, end) = some!(marker_value(bytes, components_marker, frame.end));
    if !some!(bytes.get(end..frame.end))
        .iter()
        .all(|byte| *byte == 0)
    {
        return Ok(None);
    }
    let registry_flag = some!(crate::records::act::ActRegistryFlag::from_code(
        registry_flag
    ));
    let layout = crate::records::act::ActComponentLayout::new(
        u64_from_index(frame.start),
        entity_id,
        display_name,
        u64_from_index(components_marker - cursor),
    )
    .ok();
    let layout = some!(layout);
    Ok(Some(some!(ActComponentLink::try_new(
        crate::ids::native_scoped_id(stream, "act-component-link", frame.start),
        frame.record_index,
        frame.class_tag.clone(),
        crate::records::act::ActComponentReferences {
            instance_root: instance_root_record,
            tracked_entity: tracked_entity_record,
            components_root: components_root_record,
        },
        registry_flag,
        layout,
    )
    .ok())))
}

/// Whether `key` has the ACT entity-key form `<segment id>_<entity id>`.
pub(crate) fn is_entity_key(key: &str) -> bool {
    let Some((segment, entity)) = key.split_once('_') else {
        return false;
    };
    !segment.is_empty()
        && !entity.is_empty()
        && segment.bytes().all(|byte| byte.is_ascii_digit())
        && entity.bytes().all(|byte| byte.is_ascii_digit())
}

fn marker_ref(
    bytes: &[u8],
    position: usize,
    zero_count: usize,
    frame_end: usize,
) -> Option<(u32, usize)> {
    if bytes.get(position) != Some(&1) {
        return None;
    }
    let value = View::u32_le_at(bytes, position + 1)?;
    let end = position.checked_add(5)?.checked_add(zero_count)?;
    if end > frame_end {
        return None;
    }
    bytes
        .get(position + 5..end)?
        .iter()
        .all(|byte| *byte == 0)
        .then_some((value, end))
}

fn marker_value(bytes: &[u8], position: usize, frame_end: usize) -> Option<(u32, usize)> {
    if bytes.get(position) != Some(&1) || position.checked_add(5)? > frame_end {
        return None;
    }
    Some((View::u32_le_at(bytes, position + 1)?, position + 5))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use super::{
        decode_channel_group, decode_component_link, merge_entities, ChannelGroup, RecordFrame,
        TableEntry,
    };
    use crate::records::act::ActTableRow;
    use crate::records::identity::Located;
    use crate::test_support::{lp_ascii, lp_utf16};
    use std::collections::BTreeMap;

    fn table_entry(entity_id: &str) -> TableEntry {
        TableEntry {
            record_index: 7,
            row: ActTableRow::new(20).unwrap(),
            entity_id: entity_id.into(),
        }
    }

    fn channel_group(entity_id: &str) -> ChannelGroup {
        ChannelGroup {
            record_index: 7,
            record_index_offset: 100,
            entity_id: Some(Located {
                value: entity_id.into(),
                offset: 200,
            }),
            class_tag: "261".to_owned().try_into().unwrap(),
            channels: BTreeMap::from([(
                "Appearance".into(),
                Located {
                    value: String::from("11111111-2222-3333-4444-555555555555")
                        .try_into()
                        .unwrap(),
                    offset: 120,
                },
            )]),
            class_tail: None,
        }
    }

    #[test]
    fn record_index_joins_exactly_one_matching_act_change_group() {
        let stream = "Synthetic/FusionACTSegmentType1/BulkStream.dat";
        let entities = merge_entities(
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_985")],
        )
        .expect("matching table and change group");
        assert_eq!(entities.len(), 1);
        assert!(entities[0].in_table());
        assert_eq!(entities[0].record_index(), 7);

        let mismatch = merge_entities(
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_986")],
        )
        .expect_err("table and change-group keys must agree");
        assert!(mismatch.to_string().contains("entity key conflicts"));

        let duplicate = merge_entities(
            stream,
            vec![table_entry("0_985")],
            vec![channel_group("0_985"), channel_group("0_985")],
        )
        .expect_err("one record index cannot own two change groups");
        assert!(duplicate
            .to_string()
            .contains("duplicate F3D ACT change group"));

        let table_only = merge_entities(stream, vec![table_entry("0_985")], Vec::new())
            .expect_err("every table reference must resolve to a change group");
        assert!(table_only.to_string().contains("has no change group"));

        let group_only = merge_entities(stream, Vec::new(), vec![channel_group("0_985")])
            .expect("a change group need not have an inline ACTTable row");
        assert!(!group_only[0].in_table());
        assert!(group_only[0].table_record_index_offset().is_none());

        let mut table_keyed_group = channel_group("0_985");
        table_keyed_group.entity_id = None;
        let entities = merge_entities(stream, vec![table_entry("0_985")], vec![table_keyed_group])
            .expect("the table can supply an omitted group key");
        assert_eq!(entities[0].entity_id(), "0_985");
        assert!(entities[0].channel_entity_id_offset().is_none());
    }

    /// A document-root component link: `padding` zero bytes sit between the
    /// display name and the components-root reference.
    fn root_link(padding: usize) -> (Vec<u8>, RecordFrame) {
        let payload_offset = 11;
        let mut bytes = vec![0; payload_offset + 10];
        bytes.extend_from_slice(&[1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "1_2");
        bytes.extend_from_slice(&[1, 3, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(&[1, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "F01");
        bytes.extend(std::iter::repeat_n(0, padding));
        bytes.extend_from_slice(&[1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let frame = RecordFrame {
            start: 0,
            end: bytes.len(),
            record_index: 2,
            record_index_offset: 7,
            payload_offset,
            class_tag: "270".to_owned().try_into().unwrap(),
        };
        (bytes, frame)
    }

    #[test]
    fn root_component_link_stores_zero_through_eight_padding_bytes() {
        for padding in [0, 1, 8] {
            let (bytes, frame) = root_link(padding);
            let link = decode_component_link(&bytes, &frame, "synthetic").unwrap();
            assert!(
                matches!(link, Some(ref root) if root.record_index == 2),
                "document-root link with {padding} padding bytes"
            );
        }
        let (bytes, frame) = root_link(9);
        assert!(decode_component_link(&bytes, &frame, "synthetic")
            .unwrap()
            .is_none());
    }

    #[test]
    fn channel_group_retains_opaque_residue_after_its_semantic_payload() {
        let payload_offset = 11;
        let mut bytes = vec![0; payload_offset + 10];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_ascii(&mut bytes, "Appearance");
        lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
        let entity_at = bytes.len();
        lp_utf16(&mut bytes, "0_985");
        let tail_at = bytes.len();
        bytes.extend_from_slice(&[0; 11]);
        let mut frame = RecordFrame {
            start: 0,
            end: bytes.len(),
            record_index: 7,
            record_index_offset: 7,
            payload_offset,
            class_tag: "261".to_owned().try_into().unwrap(),
        };

        let group = decode_channel_group(&bytes, &frame, "synthetic")
            .expect("well-framed group")
            .expect("zero padding belongs to the group frame");
        assert_eq!(group.record_index, 7);
        assert_eq!(
            group.entity_id.as_ref().map(|id| id.value.as_str()),
            Some("0_985")
        );
        assert!(group.class_tail.is_none());

        let keyless_frame = RecordFrame {
            start: frame.start,
            end: entity_at,
            record_index: frame.record_index,
            record_index_offset: frame.record_index_offset,
            payload_offset: frame.payload_offset,
            class_tag: frame.class_tag.clone(),
        };
        let keyless = decode_channel_group(&bytes, &keyless_frame, "synthetic")
            .expect("well-framed keyless group")
            .expect("table-keyed group");
        assert!(keyless.entity_id.is_none());

        let class_tail = b"\0synthetic-stream-residue\x01";
        bytes.truncate(tail_at);
        bytes.extend_from_slice(class_tail);
        frame.end = bytes.len();
        let group = decode_channel_group(&bytes, &frame, "synthetic")
            .expect("well-framed group with stream residue")
            .expect("residue follows the complete channel grammar");
        let tail = group.class_tail.as_ref().unwrap();
        assert_eq!(tail.bytes(), class_tail);
        assert_eq!(tail.offset(), u64_from_index(tail_at));
        for class_tag in ["258", "269", "271"] {
            frame.class_tag = class_tag.to_owned().try_into().unwrap();
            let retained = decode_channel_group(&bytes, &frame, "synthetic")
                .unwrap()
                .unwrap();
            assert_eq!(retained.channels, group.channels);
            assert_eq!(retained.entity_id, group.entity_id);
            assert_eq!(retained.class_tail, group.class_tail);
        }
    }
}
