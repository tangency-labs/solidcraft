// SPDX-License-Identifier: Apache-2.0
//! Decode the auxiliary BRep-cell carrier of a `SurfaceTrim` operation.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::deleted_members::timestamp_table_spans_by_owner;
use crate::design::decode::scopes::shared_frames::exact_indexed_header_at;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::{
    cached_owned_record_offsets, indexed_record_header_at, next_indexed_record_offset,
    IndexedRecordOffsets,
};
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::layout::indexed_design_record_header as header;
use crate::layout::surface_trim_cell_entry as cell_entry;
use crate::layout::surface_trim_cell_table_prefix as cell_table;
use crate::records::feature::{
    scope::DesignParameterScope,
    surface_ops::{
        DesignSurfaceTrimCellEntry, DesignSurfaceTrimChainRecord, DesignSurfaceTrimOperation,
    },
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Decode the exact auxiliary BRep-cell carrier of a `SurfaceTrim` scope.
///
/// The scope's record-timestamp table and one further indexed record precede
/// the cell table. The table has one 19-byte entry for each marked cell
/// reference, and its trailing value bounds every entry value.
fn exact_surface_trim_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    timestamp_tables: &HashMap<u32, Option<Range<usize>>>,
    scope: &DesignParameterScope,
) -> Option<DesignSurfaceTrimOperation> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceTrim {
        return None;
    }
    let timestamp_table = timestamp_tables.get(&scope.record_index)?.clone()?;
    let following_start = next_indexed_record_offset(bytes, timestamp_table.end)?;
    let cell_table_byte_offset =
        next_indexed_record_offset(bytes, following_start.checked_add(header::LEN)?)?;
    let chain_record = |start: usize, end: usize| {
        let parsed = indexed_record_header_at(bytes, start)?;
        Some(DesignSurfaceTrimChainRecord {
            record_index: parsed.record_index,
            byte_offset: u64::try_from(start).ok()?,
            class_tag: parsed.class_tag,
            frame_length: u64::try_from(end.checked_sub(start)?).ok()?,
        })
    };
    let chain_records = [
        chain_record(timestamp_table.start, following_start)?,
        chain_record(following_start, cell_table_byte_offset)?,
    ];

    let cell_table = indexed_record_header_at(bytes, cell_table_byte_offset)?;
    let (primary, paired) = records
        .frames(cell_table.record_index)
        .find(|(primary, _)| *primary == cell_table_byte_offset)?;
    let cell_table_paired_class_tag =
        exact_indexed_header_at(bytes, paired, cell_table.record_index)?;
    let cell_count_offset = primary.checked_add(cell_table::CELL_COUNT)?;
    if bytes
        .get(primary.checked_add(cell_table::ZERO_RUN)?..cell_count_offset)?
        .iter()
        .any(|byte| *byte != 0)
    {
        return None;
    }
    let cell_count = usize::try_from(View::u32_le_at(bytes, cell_count_offset)?).ok()?;
    let entries_start = primary.checked_add(cell_table::LEN)?;
    let trailing_value_offset =
        entries_start.checked_add(cell_count.checked_mul(cell_entry::LEN)?)?;
    let trailing_zero_offset = trailing_value_offset.checked_add(4)?;
    if paired != trailing_zero_offset.checked_add(4)?
        || View::u32_le_at(bytes, trailing_zero_offset) != Some(0)
    {
        return None;
    }
    let trailing_value = View::u32_le_at(bytes, trailing_value_offset)?;
    let total_cells = u64::from(trailing_value);
    let mut cell_record_indices = HashSet::new();
    let mut cell_entries = Vec::with_capacity(cell_count);
    for entry_index in 0..cell_count {
        let entry_start = entries_start.checked_add(entry_index.checked_mul(cell_entry::LEN)?)?;
        let reference_start = entry_start.checked_add(cell_entry::RECORD_REFERENCE)?;
        let cell_record_index = marked_record_reference(bytes, reference_start)?;
        let ordinal_offset = entry_start.checked_add(cell_entry::VALUE)?;
        let ordinal_value = View::u64_le_at(bytes, ordinal_offset)?;
        if !cell_record_indices.insert(cell_record_index)
            || records.offsets(cell_record_index).is_empty()
            || ordinal_value == 0
            || ordinal_value > total_cells
        {
            return None;
        }
        cell_entries.push(DesignSurfaceTrimCellEntry {
            record_index: cell_record_index,
            record_reference_offset: u64::try_from(reference_start.checked_add(1)?).ok()?,
            ordinal: ordinal_value,
            ordinal_offset: u64::try_from(ordinal_offset).ok()?,
        });
    }
    DesignSurfaceTrimOperation::try_from(
        crate::records::feature::surface_ops::DesignSurfaceTrimOperationWire {
            id: String::new(),
            scope_record_index: scope.record_index,
            chain_records,
            cell_table_record_index: cell_table.record_index,
            cell_table_byte_offset: u64::try_from(primary).ok()?,
            cell_table_class_tag: cell_table.class_tag,
            cell_table_frame_length: u64::try_from(paired.checked_sub(primary)?).ok()?,
            cell_table_paired_class_tag: cell_table_paired_class_tag.try_into().ok()?,
            cell_table_paired_byte_offset: u64::try_from(paired).ok()?,
            cell_count_offset: u64::try_from(cell_count_offset).ok()?,
            cell_entries,
            trailing_value,
            trailing_value_offset: u64::try_from(trailing_value_offset).ok()?,
            trailing_zero_offset: u64::try_from(trailing_zero_offset).ok()?,
        },
    )
    .ok()
}

/// Decode every exact `SurfaceTrim` BRep-cell carrier into its own native arena.
pub(crate) fn decode_surface_trim_operations(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignSurfaceTrimOperation>, CodecError> {
    let mut record_offsets = HashMap::<String, IndexedRecordOffsets>::new();
    let mut timestamp_tables = HashMap::<String, HashMap<u32, Option<Range<usize>>>>::new();
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_owned_record_offsets(&mut record_offsets, stream, bytes);
        let timestamp_tables = timestamp_tables
            .entry(stream.to_owned())
            .or_insert_with(|| timestamp_table_spans_by_owner(bytes));
        let Some(mut operation) =
            exact_surface_trim_operation(bytes, records, timestamp_tables, scope)
        else {
            continue;
        };
        operation.id = design_record_id(
            &entry.name,
            ":design-surface-trim-operation#",
            scope.byte_offset(),
        );

        out.push(operation);
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

#[cfg(test)]
mod tests;
