// SPDX-License-Identifier: Apache-2.0
//! Frame whole-body reference tails by stable `MetaStream` registration.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_relaxed, lp_ascii_filtered_view, lp_utf16_bounded, take_reference};
use crate::container::ContainerScan;
use crate::layout::body_reference_persistent_pair as persistent_pair;
use crate::layout::body_reference_tail_prefix as fixed_prefix;
use crate::metastream::{primary_record_frames, PrimaryRecordFrame};
use crate::records::body_reference_tail::{
    BodyTailReference, DesignBodyReferenceTail, DesignBodyReferenceTailWire,
};
use crate::records::references::DesignClassTag;

mod tail;

const TYPE_GUID: &str = "9716F783-676D-42E0-93F9-EBF273E7C035";
const BASE_GUID: &str = "6144EA59-3ACA-4172-9F14-A6531E94D69B";

pub(crate) struct DecodedTails {
    pub(crate) records: Vec<DesignBodyReferenceTail>,
    pub(crate) unparsed: usize,
}

fn reference(view: &mut View<'_>) -> Option<()> {
    let mut cursor = view.position().checked_sub(view.start())?;
    take_reference(view.window(), &mut cursor)?;
    view.seek(view.start().checked_add(cursor)?)
}

fn uuid(view: &mut View<'_>) -> Option<()> {
    let (value, end) = lp_utf16_bounded(view.unread(), 0, 36..=36)?;
    if !is_guid_relaxed(&value) {
        return None;
    }
    view.skip(end)
}

fn prefix(mut view: View<'_>, frame: PrimaryRecordFrame, type_ordinal: usize) -> Option<View<'_>> {
    let (class, end) = lp_ascii_filtered_view(view.unread(), 0, 3..=3, u8::is_ascii_digit)?;
    let class = DesignClassTag::try_from(class.to_owned()).ok()?;
    if class.dynamic_ordinal() != Some(type_ordinal) {
        return None;
    }
    view.skip(end)?;
    if u64::from(view.u32_le()?) != frame.entity_id
        || view.take(fixed_prefix::REFERENCE_COUNT - fixed_prefix::ZERO_PREFIX)?
            != [0; fixed_prefix::REFERENCE_COUNT - fixed_prefix::ZERO_PREFIX]
    {
        return None;
    }
    let count = view.u32_le()?;
    let count = view.counted(u64::from(count), persistent_pair::LEN)?.get();
    view.skip(count.checked_mul(persistent_pair::LEN)?)?;
    let mut cursor = view.position().checked_sub(view.start())?;
    let nested = take_reference(view.window(), &mut cursor)?.into_local()?.0;
    if !matches!(nested.checked_sub(frame.entity_id), Some(2 | 3)) {
        return None;
    }
    view.seek(view.start().checked_add(cursor)?)?;
    if view.u32_le()? != 1 {
        return None;
    }
    uuid(&mut view)?;
    uuid(&mut view)?;
    if view.u32_le()? != 2 {
        return None;
    }
    Some(view)
}

fn references(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    span: tail::ReferenceRunSpan,
) -> Result<Vec<BodyTailReference>, CodecError> {
    let start = usize::try_from(span.start)
        .map_err(|_| CodecError::malformed("body-tail reference start exceeds usize"))?;
    let end = usize::try_from(span.end)
        .map_err(|_| CodecError::malformed("body-tail reference end exceeds usize"))?;
    let mut view = root
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("body-tail reference run is outside its source"))?;
    let count = view
        .counted(u64::from(span.count.value), 1)
        .ok_or_else(|| CodecError::malformed("body-tail reference count exceeds its run"))?;
    let mut values = ctx.alloc_filled(
        count.get(),
        BodyTailReference::null(),
        "body-tail references",
    )?;
    for value in &mut values {
        let start = view.position();
        reference(&mut view)
            .ok_or_else(|| CodecError::malformed("body-tail reference is incomplete"))?;
        let end = view.position();
        let bytes = root
            .child(start, end)
            .ok_or_else(|| CodecError::malformed("body-tail reference is outside its source"))?;
        let mut encoding = ctx.alloc_filled(bytes.remaining(), 0u8, "body-tail reference bytes")?;
        encoding.copy_from_slice(bytes.unread());
        *value = BodyTailReference::new(u64_from_index(start), encoding)
            .map_err(CodecError::Malformed)?;
    }
    Ok(values)
}

fn is_selection_type(kind: &crate::records::entity_header::SegmentTypeData) -> bool {
    kind.type_guid.as_str().eq_ignore_ascii_case(TYPE_GUID)
        && kind
            .base_type_guid
            .value()
            .is_some_and(|guid| guid.as_str().eq_ignore_ascii_case(BASE_GUID))
        && kind.version == 1
        && kind.module == "Fusion"
}

/// Visit each framed selection record with the local targets of its second
/// reference run. A record outside this frame form is skipped.
pub(super) fn for_each_selection(
    root: View<'_>,
    meta: &crate::metastream::MetaStream,
    frames: &std::collections::HashMap<u64, PrimaryRecordFrame>,
    mut visit: impl FnMut(PrimaryRecordFrame, &[u64]),
) {
    for (ordinal, kind) in meta.types.iter().enumerate() {
        if !is_selection_type(kind) {
            continue;
        }
        for entity in kind.entities.values() {
            let Some(frame) = frames.get(entity).copied() else {
                continue;
            };
            let targets = root
                .child(frame.start, frame.member_end)
                .and_then(|view| prefix(view, frame, ordinal))
                .and_then(|view| tail::parse(view, false))
                .and_then(|fields| local_targets(root, fields.second));
            if let Some(targets) = targets {
                visit(frame, &targets);
            }
        }
    }
}

fn local_targets(root: View<'_>, span: tail::ReferenceRunSpan) -> Option<Vec<u64>> {
    let run = root.child(
        usize::try_from(span.start).ok()?,
        usize::try_from(span.end).ok()?,
    )?;
    let mut cursor = 0;
    let mut targets = Vec::new();
    while cursor < run.remaining() {
        targets.push(take_reference(run.window(), &mut cursor)?.into_local()?.0);
    }
    Some(targets)
}

pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<DecodedTails, CodecError> {
    let mut result = DecodedTails {
        records: Vec::new(),
        unparsed: 0,
    };
    for entry in &scan.entries {
        let Some(prefix_name) = entry.name.strip_suffix("MetaStream.dat") else {
            continue;
        };
        let Some(meta_view) = scan.entry_view(&entry.name) else {
            continue;
        };
        if !meta_view
            .window()
            .windows(TYPE_GUID.len())
            .any(|bytes| bytes.eq_ignore_ascii_case(TYPE_GUID.as_bytes()))
        {
            continue;
        }
        let meta = scan.parsed_metastream(&entry.name)?;
        let bulk_name = format!("{prefix_name}BulkStream.dat");
        let Some(root) = scan.entry_view(&bulk_name) else {
            continue;
        };
        let frames = primary_record_frames(&meta, root.remaining())?;
        let by_entity: std::collections::HashMap<_, _> = frames
            .into_iter()
            .map(|frame| (frame.entity_id, frame))
            .collect();
        for (ordinal, kind) in meta.types.iter().enumerate() {
            if !is_selection_type(kind) {
                continue;
            }
            for entity in kind.entities.values() {
                let Some(frame) = by_entity.get(entity).copied() else {
                    result.unparsed += 1;
                    continue;
                };
                let fields = root
                    .child(frame.start, frame.member_end)
                    .and_then(|view| prefix(view, frame, ordinal))
                    .and_then(|view| {
                        tail::parse(view, bulk_name.starts_with("SimGenerativeModelSubtype/"))
                    });
                let Some(fields) = fields else {
                    result.unparsed += 1;
                    continue;
                };
                let record = DesignBodyReferenceTail::try_from(DesignBodyReferenceTailWire {
                    id: super::text::design_record_id(
                        &bulk_name,
                        ":design-body-reference-tail#",
                        u64_from_index(frame.start),
                    ),
                    record_index: *entity,
                    byte_offset: u64_from_index(frame.start),
                    serialization_form: fields.form,
                    first_count_offset: fields.first.count.offset,
                    first_references: references(ctx, root, fields.first)?,
                    second_count_offset: fields.second.count.offset,
                    second_references: references(ctx, root, fields.second)?,
                    extra_zero: fields.extra_zero,
                    trailing_form: fields.trailing,
                })
                .map_err(CodecError::Malformed)?;
                result.records.push(record);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
