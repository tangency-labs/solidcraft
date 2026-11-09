// SPDX-License-Identifier: Apache-2.0
//! Scope references that name deleted members.

use super::sketch::{indexed_record_header_at, indexed_record_offsets, IndexedRecordHeader};
use crate::container::ContainerScan;
use crate::design::decode::text::design_record_id;
use crate::layout::record_timestamp_entry as entry;
use crate::layout::record_timestamp_table_prefix as table;
use crate::records::{
    decal::DesignRecordHeader,
    feature::deleted_members::{DesignDeletedMembers, DesignRecordTimestamp},
    feature::scope::DesignParameterScope,
    identity::Located,
};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::CodecError;

/// Record the reference members of each scope that name a deleted member,
/// with the scope's record-timestamp table as evidence, and add the table's
/// indexed header to `headers`.
pub(crate) fn mark_deleted_members(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &mut Vec<DesignRecordHeader>,
) -> Result<(), CodecError> {
    let held = headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    let mut table_headers = Vec::new();
    for stream_entry in scan.entries.iter().filter(|stream_entry| {
        super::sheet_metal_settings::is_settings_stream(
            scan,
            stream_entry,
            ContainerRole::Bulkstream,
        )
    }) {
        let stream = crate::ids::native_scope(&stream_entry.name);
        let mut bytes = None;
        for scope in scopes
            .iter_mut()
            .filter(|scope| crate::ids::native_stream(&scope.id) == Some(stream.as_str()))
        {
            let absent = scope
                .reference_members()
                .values()
                .copied()
                .filter(|index| !held.contains(&(stream.clone(), *index)))
                .collect::<Vec<_>>();
            if absent.is_empty() {
                continue;
            }
            let bytes = match bytes {
                Some(bytes) => bytes,
                None => *bytes.insert(scan.entry_bytes(&stream_entry.name)?),
            };
            let (Ok(start), Ok(end)) = (
                usize::try_from(scope.byte_offset()),
                usize::try_from(scope.paired_byte_offset()),
            ) else {
                continue;
            };
            let Some(evidence) = deleted_members(bytes, start..end, scope.record_index, &absent)
            else {
                continue;
            };
            table_headers.push(DesignRecordHeader {
                id: design_record_id(
                    &stream_entry.name,
                    ":design-record-header#",
                    evidence.table_byte_offset,
                ),
                record_index: evidence.table_record_index,
                class_tag: evidence.table_class_tag.clone(),
                byte_offset: evidence.table_byte_offset,
            });
            scope.deleted_members = Some(evidence);
        }
    }
    let mut held_offsets = headers
        .iter()
        .map(|header| header.id.clone())
        .collect::<std::collections::HashSet<_>>();
    for header in table_headers {
        if held_offsets.insert(header.id.clone()) {
            headers.push(header);
        }
    }
    headers.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// The record-timestamp evidence for the distinct absent indices of scope
/// `owner` that have no indexed header and whose u32 encoding occurs only in `frame` or as
/// an entry of the scope's record-timestamp table.
fn deleted_members(
    bytes: &[u8],
    frame: std::ops::Range<usize>,
    owner: u32,
    absent: &[u32],
) -> Option<DesignDeletedMembers> {
    let table = owner_timestamp_table(bytes, owner)?;
    let mut seen = std::collections::HashSet::new();
    let members = absent
        .iter()
        .copied()
        .filter(|index| seen.insert(*index))
        .filter(|index| {
            indexed_record_offsets(bytes).all(|header| header.record_index != *index)
                && memchr::memmem::find_iter(bytes, &index.to_le_bytes()).all(|at| {
                    (at >= frame.start && at.checked_add(4).is_some_and(|last| last <= frame.end))
                        || table.entries.contains(&at)
                })
        })
        .filter(|index| {
            table
                .entries
                .iter()
                .any(|at| View::u32_le_at(bytes, *at) == Some(*index))
        })
        .collect::<Vec<_>>();
    let table_bytes = bytes.get(table.header.offset..table.end)?.to_vec();
    let evidence = table_evidence(&table_bytes, u64_from_index(table.header.offset))?;
    (!members.is_empty()).then(|| DesignDeletedMembers {
        table_record_index: evidence.record_index,
        table_byte_offset: u64_from_index(table.header.offset),
        table_class_tag: evidence.class_tag,
        table_bytes: table_bytes.into(),
        entries: evidence.entries,
        owner: evidence.owner,
        members,
    })
}

/// The header fields, counted entries, and owner entry of one stored
/// record-timestamp table.
#[derive(Debug, PartialEq)]
pub(crate) struct TimestampTableEvidence {
    pub(crate) record_index: u32,
    pub(crate) class_tag: crate::records::references::DesignClassTag,
    pub(crate) entries: Vec<DesignRecordTimestamp>,
    pub(crate) owner: DesignRecordTimestamp,
}

/// The record-timestamp table that `table_bytes` stores exactly, from its
/// indexed header through its owner entry, with entry offsets in the stream
/// where the header starts at `table_byte_offset`.
pub(crate) fn table_evidence(
    table_bytes: &[u8],
    table_byte_offset: u64,
) -> Option<TimestampTableEvidence> {
    let header = indexed_record_header_at(table_bytes, 0)?;
    let record_index = header.record_index;
    let class_tag = header.class_tag.clone();
    let (_, table) = timestamp_table(table_bytes, header)?;
    if table.end != table_bytes.len() {
        return None;
    }
    let timestamp = |at: usize| {
        let timestamp_at = at
            .checked_sub(entry::RECORD_INDEX)?
            .checked_add(entry::TIMESTAMP)?;
        Some(DesignRecordTimestamp {
            record_index: Located {
                value: View::u32_le_at(table_bytes, at)?,
                offset: table_byte_offset.checked_add(u64_from_index(at))?,
            },
            timestamp: View::u64_le_at(table_bytes, timestamp_at)?,
        })
    };
    Some(TimestampTableEvidence {
        record_index,
        class_tag,
        entries: table
            .entries
            .iter()
            .map(|at| timestamp(*at))
            .collect::<Option<Vec<_>>>()?,
        owner: timestamp(table.owner_offset)?,
    })
}

/// A record-timestamp table: its indexed header, the offsets of its entries'
/// record indices, the offset of its owner entry's record index, and the end
/// of its owner entry.
struct TimestampTable {
    header: IndexedRecordHeader,
    entries: Vec<usize>,
    owner_offset: usize,
    end: usize,
}

/// The one record-timestamp table whose owner entry names record `owner`.
fn owner_timestamp_table(bytes: &[u8], owner: u32) -> Option<TimestampTable> {
    let mut tables = indexed_record_offsets(bytes)
        .filter_map(|header| timestamp_table(bytes, header))
        .filter(|(table_owner, _)| *table_owner == owner);
    let (_, table) = tables.next()?;
    tables.next().is_none().then_some(table)
}

pub(in crate::design::decode) fn timestamp_table_spans_by_owner(
    bytes: &[u8],
) -> std::collections::HashMap<u32, Option<std::ops::Range<usize>>> {
    let mut spans = std::collections::HashMap::new();
    for (owner, table) in
        indexed_record_offsets(bytes).filter_map(|header| timestamp_table(bytes, header))
    {
        spans
            .entry(owner)
            .and_modify(|span| *span = None)
            .or_insert(Some(table.header.offset..table.end));
    }
    spans
}

/// The owner and layout of a record-timestamp table whose indexed header is
/// `header`.
fn timestamp_table(bytes: &[u8], header: IndexedRecordHeader) -> Option<(u32, TimestampTable)> {
    let start = header.offset;
    let zero_run = start.checked_add(table::ZERO_RUN)?..start.checked_add(table::FIRST_MARKER)?;
    let prefix_valid = bytes.get(zero_run)?.iter().all(|byte| *byte == 0)
        && *bytes.get(start.checked_add(table::FIRST_MARKER)?)? == table::FIRST_MARKER_VALUE
        && *bytes.get(start.checked_add(table::SECOND_MARKER)?)? == table::SECOND_MARKER_VALUE;
    if !prefix_valid {
        return None;
    }
    let count = usize::try_from(View::u32_le_at(
        bytes,
        start.checked_add(table::ENTRY_COUNT)?,
    )?)
    .ok()?;
    let first = start.checked_add(table::LEN)?;
    let owner_entry = count.checked_mul(entry::LEN)?.checked_add(first)?;
    let end = owner_entry.checked_add(entry::LEN)?;
    if end > bytes.len() {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    for at in (first..owner_entry).step_by(entry::LEN) {
        entries.push(entry_record_index(bytes, at)?.1);
    }
    let (table_owner, owner_offset) = entry_record_index(bytes, owner_entry)?;
    Some((
        table_owner,
        TimestampTable {
            header,
            entries,
            owner_offset,
            end,
        },
    ))
}

/// Record index of the entry at `at` and the offset of its encoding.
fn entry_record_index(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let index_at = at.checked_add(entry::RECORD_INDEX)?;
    let zero_run = at.checked_add(entry::ZERO_RUN)?..at.checked_add(entry::TIMESTAMP)?;
    (*bytes.get(at.checked_add(entry::MARKER)?)? == entry::MARKER_VALUE
        && bytes.get(zero_run)?.iter().all(|byte| *byte == 0))
    .then_some((View::u32_le_at(bytes, index_at)?, index_at))
}

#[cfg(test)]
mod tests;
