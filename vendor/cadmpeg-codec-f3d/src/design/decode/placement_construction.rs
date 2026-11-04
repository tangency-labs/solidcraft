// SPDX-License-Identifier: Apache-2.0
//! Retain placement construction members by semantic type registration.

use crate::container::ContainerScan;
use crate::layout::placement_indexed_prefix as long;
use crate::metastream::primary_record_frames;
use crate::records::identity::Located;
use crate::records::placement_construction::{
    DesignPlacementConstruction, PlacementConstructionWire,
};
use crate::records::references::DesignClassTag;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) mod cylinders;
mod fields;
pub(crate) mod work_planes;

const TYPE_GUID: &str = "7B6C3C2D-4096-4E6D-B225-699E8C2D9355";
const BASE_GUID: &str = "2CA5A1CD-C99B-4C9B-91AF-57989148E841";

pub(super) fn checked_solved_frame(
    placement: &DesignPlacementConstruction,
) -> Option<Located<crate::records::sketch_placement::SketchPlacementMatrix>> {
    if placement.fields.inferred_x_axis.value != 0
        || placement.fields.inferred_z_axis.value != 0
        || placement.fields.z_flip.value != 0
        || placement
            .fields
            .axis_flipped
            .as_ref()
            .is_some_and(|value| value.value != 0)
        || placement
            .fields
            .use_u_direction
            .as_ref()
            .is_some_and(|value| {
                value.value != 0 && !(placement.type_version == 16 && value.value == 1)
            })
        || placement
            .fields
            .periodic_face_mode
            .as_ref()
            .is_some_and(|value| value.value != 0)
    {
        return None;
    }
    let (matrix, offset) = match (&placement.fields.matrix, &placement.fields.identity) {
        (Some(matrix), _) => (matrix.value, matrix.offset),
        (None, Some(identity)) if identity.value => (
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            identity.offset,
        ),
        _ => return None,
    };
    crate::design::feature_project::work_plane_datum_frame(matrix.rows())?;
    Some(Located {
        value: matrix,
        offset,
    })
}

pub(crate) struct DecodedPlacements {
    pub(crate) records: Vec<DesignPlacementConstruction>,
    pub(crate) unparsed: usize,
}

fn prefix(mut view: View<'_>, length: usize) -> Option<View<'_>> {
    let start = view.start();
    let zeros = view.child(
        start.checked_add(long::ZERO_PREFIX)?,
        start.checked_add(length)?,
    )?;
    if !zeros.window().iter().all(|value| *value == 0) {
        return None;
    }
    view.seek(start.checked_add(length)?)?;
    Some(view)
}

pub(crate) fn decode_frame(
    ctx: &DecodeContext<'_>,
    id: String,
    mut frame: View<'_>,
    version: u32,
) -> Result<Option<DesignPlacementConstruction>, CodecError> {
    if !crate::records::placement_construction::supported_version(version) {
        return Ok(None);
    }
    let byte_offset = u64_from_index(frame.start());
    let paired_byte_offset = u64_from_index(frame.end());
    let Some(length) = frame.u32_le() else {
        return Ok(None);
    };
    if length != 3 {
        return Ok(None);
    }
    let Some(raw_class) = frame.take(3) else {
        return Ok(None);
    };
    let Some(class_tag) = std::str::from_utf8(raw_class)
        .ok()
        .and_then(|text| DesignClassTag::try_from(text.to_owned()).ok())
    else {
        return Ok(None);
    };
    let Some(record_index) = frame.u32_le().map(u64::from) else {
        return Ok(None);
    };
    let Some(members) = prefix(frame, long::LEN) else {
        return Ok(None);
    };
    let marked = fields::scan(members, version, true);
    let unmarked = if version == 4 {
        fields::scan(members, version, false)
    } else {
        None
    };
    let ((Some((raw, base)), None) | (None, Some((raw, base)))) = (marked, unmarked) else {
        return Ok(None);
    };
    let Some(member_offset) = byte_offset.checked_add(u64_from_index(long::LEN)) else {
        return Ok(None);
    };
    let fields = fields::retain(ctx, &raw)?;
    let mut base_bytes = ctx.alloc_filled(base.remaining(), 0u8, "placement base-level bytes")?;
    base_bytes.copy_from_slice(base.unread());
    DesignPlacementConstruction::try_from(PlacementConstructionWire {
        id,
        record_index,
        class_tag,
        type_version: version,
        byte_offset,
        member_offset,
        paired_byte_offset,
        fields,
        base_level: Located {
            offset: u64_from_index(base.position()),
            value: base_bytes.into(),
        },
    })
    .map(Some)
    .map_err(CodecError::Malformed)
}

fn paired_header(mut view: View<'_>, entity: u64) -> Option<()> {
    (view.u32_le()? == 3).then_some(())?;
    view.take(3)?.iter().all(u8::is_ascii_digit).then_some(())?;
    (u64::from(view.u32_le()?) == entity).then_some(())
}

pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<DecodedPlacements, CodecError> {
    let mut result = DecodedPlacements {
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
            if !kind.type_guid.as_str().eq_ignore_ascii_case(TYPE_GUID) {
                continue;
            }
            let registration_valid = kind
                .base_type_guid
                .value()
                .is_some_and(|guid| guid.as_str().eq_ignore_ascii_case(BASE_GUID))
                && kind.module == "Fusion";
            for entity in kind.entities.values() {
                let Some(frame) = by_entity.get(entity).copied() else {
                    result.unparsed += 1;
                    continue;
                };
                if !registration_valid
                    || root
                        .child(frame.member_end, frame.end)
                        .and_then(|view| paired_header(view, frame.entity_id))
                        .is_none()
                {
                    result.unparsed += 1;
                    continue;
                }
                let Some(view) = root.child(frame.start, frame.member_end) else {
                    result.unparsed += 1;
                    continue;
                };
                if View::u32_le_at(
                    view.window(),
                    crate::layout::indexed_design_record_header::RECORD_INDEX,
                )
                .map(u64::from)
                    != Some(frame.entity_id)
                {
                    result.unparsed += 1;
                    continue;
                }
                let id = super::text::design_record_id(
                    &bulk_name,
                    ":design-placement-construction#",
                    u64_from_index(frame.start),
                );
                let Some(record) = decode_frame(ctx, id, view, kind.version)? else {
                    result.unparsed += 1;
                    continue;
                };
                if record.record_index != frame.entity_id
                    || record.class_tag.dynamic_ordinal() != Some(ordinal)
                {
                    result.unparsed += 1;
                    continue;
                }
                result.records.push(record);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
