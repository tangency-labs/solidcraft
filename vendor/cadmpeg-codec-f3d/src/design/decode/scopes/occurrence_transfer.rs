// SPDX-License-Identifier: Apache-2.0
//! Read component paste relations without requiring a neutral occurrence join.

use super::shared_frames::rigid_transform_at;
use crate::bytes::lp_utf16_bounded;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::layout::paste_context_owner_tail as context_tail;
use crate::records::feature::occurrence_transfer::{
    DesignOccurrenceTransfer, DesignOccurrenceTransferSource, DesignPasteOwner, COMPONENT_TYPE,
    CONTEXT_TYPE, OWNER_TYPE,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::View;

fn zeros(view: &mut View<'_>, count: usize) -> Option<()> {
    view.take(count)?
        .iter()
        .all(|byte| *byte == 0)
        .then_some(())
}

fn reference(view: &mut View<'_>) -> Option<u32> {
    if view.u8()? != 1 {
        return None;
    }
    let index = view.u32_le()?;
    zeros(view, 6)?;
    (index != 0).then_some(index)
}

fn owner(bytes: &[u8], paired: usize, index: u32) -> Option<u64> {
    (View::u32_le_at(bytes, paired + 7)? == index).then_some(())?;
    let mut view = View::over_retained(bytes);
    view.seek(paired + 11)?;
    zeros(&mut view, 8)?;
    if view.u8()? != 1 {
        return None;
    }
    let value = view.u64_le()?;
    zeros(&mut view, 2)?;
    Some(value)
}

fn relation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    index: u32,
) -> Option<(usize, u32, u32, u64)> {
    let at = records.first_at_or_after(0, index)?;
    let mut view = View::over_retained(bytes);
    view.seek(at + 11)?;
    zeros(&mut view, 10)?;
    let occurrence = reference(&mut view)?;
    zeros(&mut view, 2)?;
    reference(&mut view)?;
    zeros(&mut view, 1)?;
    let scope = reference(&mut view)?;
    let paired = view.position();
    if next_indexed_record_offset(bytes, at + 1)? != paired {
        return None;
    }
    Some((at, occurrence, scope, owner(bytes, paired, index)?))
}

fn component_owner(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &std::collections::HashMap<u64, (&str, u32)>,
    collection: u64,
) -> Option<DesignPasteOwner> {
    if !types.get(&collection)?.0.eq_ignore_ascii_case(OWNER_TYPE) {
        return None;
    }
    let collection_record_index = u32::try_from(collection).ok()?;
    let collection_at = records.first_at_or_after(0, collection_record_index)?;
    let mut found = None;
    for (&context, &(kind, _)) in types {
        if !kind.eq_ignore_ascii_case(CONTEXT_TYPE) {
            continue;
        }
        let context_record_index = u32::try_from(context).ok()?;
        for (_, paired) in records.frames(context_record_index) {
            if paired.checked_add(context_tail::LEN - context_tail::PAIRED_HEADER)? != collection_at
            {
                continue;
            }
            let mut view = View::over_retained(bytes);
            view.seek(paired.checked_sub(context_tail::PAIRED_HEADER)?)?;
            if reference(&mut view)? != collection_record_index {
                continue;
            }
            reference(&mut view)?;
            let component = owner(bytes, paired, context_record_index)?;
            if !types
                .get(&component)?
                .0
                .eq_ignore_ascii_case(COMPONENT_TYPE)
            {
                return None;
            }
            let candidate = DesignPasteOwner {
                collection: collection_record_index,
                context: context_record_index,
                component: u32::try_from(component).ok()?,
            };
            if found.replace(candidate).is_some() {
                return None;
            }
        }
    }
    found
}

pub(super) fn exact_occurrence_transfer(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &std::collections::HashMap<u64, (&str, u32)>,
) -> Option<DesignOccurrenceTransfer> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let (shift, is_cut, identity) = match (scope.kind_name(), scope.frame_length()) {
        ("CutPaste", 277) => (0, true, true),
        ("CutPaste", 405) => (0, true, false),
        ("CutPaste", 409) => (4, true, false),
        ("CopyPaste", 525) => (0, false, false),
        ("CopyPaste", 529) => (4, false, false),
        _ => return None,
    };
    let relation_index = *scope.reference_members().values().next()?;
    let (relation_at, occurrence_index, scope_index, target_owner) =
        relation(bytes, records, relation_index)?;
    if scope_index != scope.record_index || relation_at >= start {
        return None;
    }
    let carrier = records.first_at_or_after(0, occurrence_index)?;
    if carrier >= relation_at {
        return None;
    }
    let mut carrier_view = View::over_retained(bytes);
    carrier_view.seek(carrier + 11)?;
    zeros(&mut carrier_view, 8)?;
    if carrier_view.u8()? != 1 || carrier_view.u32_le()? != 1 {
        return None;
    }
    let component = reference(&mut carrier_view)?;
    if !types
        .get(&u64::from(component))?
        .0
        .eq_ignore_ascii_case(COMPONENT_TYPE)
    {
        return None;
    }
    let mut view = View::over_retained(bytes);
    view.seek(start + 11)?;
    let (target_transform, target_transform_offset, source) = if is_cut {
        if shift == 4 {
            zeros(&mut view, 9)?;
            if view.u8()? != 1 {
                return None;
            }
            zeros(&mut view, 4)?;
        } else {
            zeros(&mut view, 10)?;
        }
        let removal_index = reference(&mut view)?;
        reference(&mut view)?;
        if view.u8()? != 1 {
            return None;
        }
        reference(&mut view)?;
        zeros(&mut view, 4)?;
        if reference(&mut view)? != relation_index
            || view.u8()? != 1
            || view.u8()? != u8::from(identity)
        {
            return None;
        }
        let transform_at = view.position();
        let transform = if identity {
            SketchPlacementMatrix::IDENTITY
        } else {
            let transform = rigid_transform_at(bytes, transform_at)?;
            view.take(128)?;
            transform
        };
        let (key, after_key) = lp_utf16_bounded(bytes, view.position(), 36..=36)?;
        view.seek(after_key)?;
        zeros(&mut view, 3)?;
        let removal = records.first_at_or_after(0, removal_index)?;
        if removal >= start || relation_at >= removal {
            return None;
        }
        let mut removed = View::over_retained(bytes);
        removed.seek(removal + 11)?;
        zeros(&mut removed, 22)?;
        let source_relation = reference(&mut removed)?;
        let (removed_key, after_removed_key) =
            lp_utf16_bounded(bytes, removed.position(), 36..=36)?;
        if removed_key != key {
            return None;
        }
        removed.seek(after_removed_key)?;
        zeros(&mut removed, 7)?;
        removed.u32_le()?;
        let (kind, _) = lp_utf16_bounded(bytes, removed.position(), 14..=14)?;
        if kind != "RemoveInstance" {
            return None;
        }
        let paired = records.first_at_or_after(removal + 11, removal_index)?;
        let source_owner = owner(bytes, paired, removal_index)?;
        let (source_at, source_occurrence, _, source_relation_owner) =
            relation(bytes, records, source_relation)?;
        if source_at >= relation_at
            || source_occurrence != occurrence_index
            || source_relation_owner != source_owner
        {
            return None;
        }
        (
            transform,
            (!identity)
                .then(|| u64::try_from(transform_at).ok())
                .flatten(),
            DesignOccurrenceTransferSource::Cut {
                removal_record_index: removal_index,
                source_relation_record_index: source_relation,
                source_owner: component_owner(bytes, records, types, source_owner)?,
                occurrence_key: key.try_into().ok()?,
            },
        )
    } else {
        zeros(&mut view, 11 + shift)?;
        reference(&mut view)?;
        zeros(&mut view, 1)?;
        let source_at = view.position();
        let source_transform = rigid_transform_at(bytes, source_at)?;
        view.take(128)?;
        reference(&mut view)?;
        zeros(&mut view, 4)?;
        if reference(&mut view)? != relation_index || view.u8()? != 1 || view.u8()? != 0 {
            return None;
        }
        let target_at = view.position();
        let target_transform = rigid_transform_at(bytes, target_at)?;
        view.take(128)?;
        let (key, after_key) = lp_utf16_bounded(bytes, view.position(), 36..=36)?;
        if key != "00000000-0000-0000-0000-000000000000" {
            return None;
        }
        view.seek(after_key)?;
        zeros(&mut view, 3)?;
        (
            target_transform,
            Some(u64::try_from(target_at).ok()?),
            DesignOccurrenceTransferSource::Copy {
                source_transform,
                source_transform_offset: u64::try_from(source_at).ok()?,
            },
        )
    };
    if u64::try_from(view.position()).ok()? != scope.reference_count_offset() {
        return None;
    }
    let value = DesignOccurrenceTransfer {
        relation_record_index: relation_index,
        occurrence_record_index: occurrence_index,
        component_record_index: u64::from(component),
        target_owner: component_owner(bytes, records, types, target_owner)?,
        target_transform,
        target_transform_offset,
        source,
    };
    value
        .links_valid(
            scope,
            |index| u64::try_from(records.first_at_or_after(0, index)?).ok(),
            |index, guid| {
                types
                    .get(&u64::from(index))
                    .is_some_and(|(kind, _)| kind.eq_ignore_ascii_case(guid))
            },
        )
        .then_some(value)
}

#[cfg(test)]
mod tests;
