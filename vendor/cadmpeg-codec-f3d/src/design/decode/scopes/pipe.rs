// SPDX-License-Identifier: Apache-2.0
//! Exact source framing and complete native Pipe binding.

use super::point_data::exact_point_data_construction;
use super::shared_frames::{exact_fixed_scalar, extrude_operation_at, marked_record_reference};
use crate::container::ContainerScan;
use crate::design::decode::meta::{
    design_primary_frames, metadata_for_bulk_stream, DesignPrimaryFrame,
};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::ids::design_segment;
use crate::layout::fixed_pipe_operation_prefix as compact;
use crate::layout::legacy_pipe_operation_prefix as legacy;
use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::SegmentType;
use crate::records::feature::path_features::pipe::{self, PipeIndex};
use crate::records::feature::path_features::{DesignPipeConstruction, DesignPipePointSelection};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Native records a Pipe binding joins.
pub(crate) struct PipeSources<'a> {
    pub(crate) types: &'a [SegmentType],
    pub(crate) headers: &'a [DesignRecordHeader],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
}

/// Bind every Pipe scope whose source framing and complete native relation
/// are exact. Other Pipe scopes keep no construction.
pub(crate) fn bind_constructions(
    scan: &ContainerScan,
    sources: &PipeSources<'_>,
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    if !scopes
        .iter()
        .any(|scope| scope.kind() == DesignFeatureKind::Pipe)
    {
        return Ok(());
    }
    let index = PipeIndex::new(
        sources.types,
        sources.headers,
        sources.owners,
        sources.groups,
    );
    let mut bindings = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream = crate::ids::native_scope(&entry.name);
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Ok(frames) = design_primary_frames(bytes, &meta) else {
            continue;
        };
        // A record repeated in the primary index has no unique frame.
        let mut indexed = HashMap::new();
        for frame in &frames {
            indexed
                .entry(frame.entity_id)
                .and_modify(|slot| *slot = None)
                .or_insert(Some(frame));
        }
        let records = IndexedRecordOffsets::build(bytes);
        for (ordinal, scope) in scopes.iter().enumerate().filter(|(_, scope)| {
            scope.kind() == DesignFeatureKind::Pipe
                && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
        }) {
            if let Some(construction) = read_construction(bytes, scope, &indexed, &records, &index)
            {
                bindings.push((ordinal, construction));
            }
        }
    }
    for (ordinal, construction) in bindings {
        if let DesignScopePayloadMut::Pipe(slot) = scopes[ordinal].payload_mut() {
            *slot = Some(construction);
        }
    }
    Ok(())
}

/// Read the compact or legacy operation prefix of a Pipe scope starting at
/// `start`. `bytes` ends at the scope's reference table.
fn exact_pipe_prefix(bytes: &[u8], start: usize) -> Option<DesignPipeConstruction> {
    let frame = bytes.get(start..)?;
    let (hollow_at, operation_at, opaque_at) =
        if frame.get(compact::ZERO_RUN_10..compact::IS_HOLLOW) == Some(&[0; 10]) {
            (
                compact::IS_HOLLOW,
                compact::OPERATION,
                compact::OPAQUE_POST_OPERATION,
            )
        } else if frame.get(legacy::ZERO_RUN_9..legacy::PREFIX_MARKER) == Some(&[0; 9])
            && frame.get(legacy::PREFIX_MARKER) == Some(&legacy::PREFIX_MARKER_VALUE)
            && frame.get(legacy::ZERO_RUN_4..legacy::IS_HOLLOW) == Some(&[0; 4])
        {
            (
                legacy::IS_HOLLOW,
                legacy::OPERATION,
                legacy::OPAQUE_POST_OPERATION,
            )
        } else {
            return None;
        };
    let is_hollow = match *frame.get(hollow_at)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let at = |offset: usize| u64::try_from(start.checked_add(offset)?).ok();
    let opaque: [u8; 2] = frame
        .get(opaque_at..opaque_at.checked_add(2)?)?
        .try_into()
        .ok()?;
    Some(DesignPipeConstruction {
        operation: extrude_operation_at(frame, operation_at)?,
        operation_offset: at(operation_at)?,
        is_hollow,
        is_hollow_offset: at(hollow_at)?,
        opaque_post_operation: opaque.into(),
        opaque_post_operation_offset: at(opaque_at)?,
        record_indexes: [0; 4],
        point_selection: None,
    })
}

fn read_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    frames: &HashMap<u64, Option<&DesignPrimaryFrame<'_>>>,
    records: &IndexedRecordOffsets,
    index: &PipeIndex<'_>,
) -> Option<DesignPipeConstruction> {
    let frame = |id: u32| frames.get(&u64::from(id)).copied().flatten();
    let own = frame(scope.record_index)?;
    if !pipe::is_pipe(own.design_type)
        || u64_from_index(own.start) != scope.byte_offset()
        || u64_from_index(own.member_end) != scope.paired_byte_offset()
        || own.class_tag != scope.class_tag
    {
        return None;
    }
    let segment = design_segment(&scope.id)?;
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let table = usize::try_from(scope.reference_count_offset()).ok()?;
    let mut construction = exact_pipe_prefix(bytes.get(..table)?, start)?;
    let mut lanes = 0_usize;
    let mut point = None;
    for id in scope.reference_members().values().copied() {
        let selected = frame(id)?;
        if pipe::is_scalar(selected.design_type) {
            let scalar = exact_fixed_scalar(bytes, records, id)?;
            let owner = index.owner(segment, id)?;
            if lanes >= 4
                || usize::from(scalar.ordinal) != lanes
                || scalar.owner_record_index != Some(scope.record_index)
                || scalar.start != u64_from_index(selected.start)
                || scalar.end != u64_from_index(selected.member_end)
                || scalar.value.get().to_bits() != owner.evaluated_value().get().to_bits()
                || scalar.value_offset != owner.evaluated_value_offset()
                || owner.byte_offset() != scalar.start
                || owner.byte_offset().checked_add(owner.frame_length()) != Some(scalar.end)
                || owner.class_tag() != &selected.class_tag
                || owner.local_ordinal() != u32::from(scalar.ordinal)
                || owner.scope_record_index() != scope.record_index
            {
                return None;
            }
            construction.record_indexes[lanes] = id;
            lanes += 1;
        } else if pipe::is_point(selected.design_type) && point.replace((id, selected)).is_some() {
            return None;
        }
    }
    if lanes != 4 {
        return None;
    }
    for group in index.groups(segment, scope.record_index) {
        let selected = frame(group.record_index)?;
        if u64_from_index(selected.start) != group.byte_offset
            || u64_from_index(selected.member_end) != group.paired_byte_offset
            || selected.class_tag != group.class_tag
        {
            return None;
        }
    }
    if let Some((id, selected)) = point {
        let types = HashMap::from([(
            u64::from(id),
            (
                selected.design_type.type_guid.as_str(),
                selected.design_type.version,
            ),
        )]);
        let point_data =
            exact_point_data_construction(bytes, records, std::iter::once(&id), &types)?;
        if point_data.point_record_byte_offset != u64_from_index(selected.start)
            || !records
                .frames(id)
                .any(|(start, end)| start == selected.start && end == selected.member_end)
        {
            return None;
        }
        // Exactly one marked reference between the prefix and the reference
        // table names the point-data record.
        let after_prefix = usize::try_from(construction.opaque_post_operation_offset)
            .ok()?
            .checked_add(2)?;
        let mut prefix_reference_offset = None;
        for at in after_prefix..table {
            if at.checked_add(11).is_some_and(|after| after <= table)
                && marked_record_reference(bytes, at) == Some(id)
                && prefix_reference_offset
                    .replace(u64_from_index(at + 1))
                    .is_some()
            {
                return None;
            }
        }
        construction.point_selection = Some(DesignPipePointSelection {
            prefix_reference_offset: prefix_reference_offset?,
            point_data,
        });
    }
    index.check_relation(scope, &construction)?;
    Some(construction)
}

#[cfg(test)]
mod tests;
