// SPDX-License-Identifier: Apache-2.0
//! Decode fixed local component-occurrence carriers.

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::container::ContainerRole;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::decode::sketch::next_indexed_record_offset;

use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, OCCURRENCE_OWNER_TYPE_GUID as OWNER_TYPE_GUID,
    OCCURRENCE_PLACEMENT_TYPE_GUID,
};

use crate::layout::local_component_occurrence_attributed_prefix as attributed_prefix;
use crate::layout::local_component_occurrence_empty_attributes_prefix as empty_prefix;
use crate::layout::local_component_occurrence_identity_tail as identity_tail;
use crate::layout::local_component_occurrence_matrix_tail as matrix_tail;
use crate::layout::local_component_occurrence_prefix as prefix;
use crate::layout::local_component_occurrence_typed_attributed_prefix as typed_attributed_prefix;
use crate::layout::local_component_occurrence_typed_compact_prefix as compact_prefix;
use crate::layout::local_component_occurrence_typed_identity_tail as typed_identity_tail;
use crate::layout::local_component_occurrence_typed_matrix_tail as typed_matrix_tail;
use crate::layout::local_component_occurrence_typed_prefix as typed_prefix;
use crate::layout::root_instance_empty_path_frame as empty_root_frame;
use crate::layout::root_instance_empty_path_prefix as empty_root_instance;
use crate::layout::root_instance_empty_path_separated_frame as separated_root_frame;
use crate::layout::root_instance_prefix as root_instance;

const BASE_FRAME_LENGTH: usize = prefix::LEN + identity_tail::LEN;
const PLACED_FRAME_LENGTH: usize = prefix::LEN + matrix_tail::LEN;
/// Width of the row-major 4x4 f64 placement matrix.
const MATRIX_BYTES: usize = 16 * 8;
/// UTF-16 code units of a hyphenated GUID.
const GUID_UNITS: u32 = 36;
/// Width of a marked u64 reference: marker byte and value.
const MARKED_U64: usize = 9;

pub(crate) struct DecodedComponentOccurrences {
    pub(crate) occurrences: Vec<DesignComponentOccurrence>,
    pub(crate) superseded: usize,
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Occurrences that two or more component-path carriers name.
    pub(crate) ambiguous_current_poses: usize,
    /// Component-path carriers whose elements are all local records.
    pub(crate) paths: Vec<crate::records::component_path::DesignComponentPath>,
}

/// Decode exact local component-occurrence records from every Design bulk stream.
pub(crate) fn decode_component_occurrences(
    scan: &ContainerScan,
) -> Result<DecodedComponentOccurrences, CodecError> {
    let mut occurrences = Vec::new();
    let mut superseded = 0;
    let mut losses = Vec::new();
    let mut ambiguous_current_poses = 0;
    let mut paths = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope = crate::ids::native_scope(&entry.name);
        let (frames, typed_frames, poses, path_carriers) =
            if let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? {
                let meta_name = format!(
                    "{}MetaStream.dat",
                    entry
                        .name
                        .strip_suffix("BulkStream.dat")
                        .unwrap_or_default()
                );
                let magic =
                    crate::metastream::serializer_magic(scan.entry_bytes(&meta_name)?, &meta_name)?;
                (
                    crate::metastream::primary_record_frames(&meta, bytes.len())?,
                    occurrence_frames(bytes, &meta)?,
                    crate::xref::local_component_path_poses(bytes, &meta, Some(magic))?,
                    crate::xref::component_path_carriers(bytes, &meta, Some(magic))?,
                )
            } else {
                (Vec::new(), Vec::new(), Vec::new(), Vec::new())
            };
        let typed_primary = typed_frames
            .iter()
            .map(|frame| {
                (
                    frame.entity_id,
                    cadmpeg_core::decode::u64_from_index(frame.start),
                )
            })
            .collect();
        let primary = frames
            .iter()
            .map(|frame| {
                (
                    frame.entity_id,
                    cadmpeg_core::decode::u64_from_index(frame.start),
                )
            })
            .collect();
        let frame_ends = frames
            .iter()
            .map(|frame| (frame.start, frame.member_end))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut stream_occurrences = Vec::new();
        let mut at = 0;
        while let Some(start) = next_indexed_record_offset(bytes, at) {
            let end = frame_ends
                .get(&start)
                .copied()
                .or_else(|| next_indexed_record_offset(bytes, start.checked_add(1)?));
            if let Some(occurrence) = end
                .and_then(|end| exact_component_occurrence_frame(bytes, start, end, &scope))
                .or_else(|| attributed_component_occurrence(bytes, start, &scope))
            {
                if let Some(loss) = unproven_identity_loss(&occurrence) {
                    losses.push(loss);
                } else {
                    stream_occurrences.push(occurrence);
                }
            }
            let Some(next_at) = start.checked_add(1) else {
                break;
            };
            at = next_at;
        }
        superseded += admit_occurrence_envelopes(&mut stream_occurrences, &primary, &typed_primary);
        ambiguous_current_poses += bind_current_poses(&mut stream_occurrences, &poses);
        paths.extend(component_paths(&scope, &stream_occurrences, path_carriers));
        occurrences.extend(stream_occurrences);
    }
    occurrences.sort_by(|a, b| a.id.cmp(&b.id));
    occurrences.dedup_by(|left, right| left.id == right.id);
    Ok(DecodedComponentOccurrences {
        occurrences,
        superseded,
        losses,
        ambiguous_current_poses,
        paths,
    })
}

/// The native component paths of one Design stream. An element is an
/// occurrence when a decoded carrier has its record index, a component when a
/// decoded carrier names it as its definition, and otherwise unclassified.
fn component_paths(
    scope: &str,
    occurrences: &[DesignComponentOccurrence],
    carriers: Vec<crate::xref::ComponentPathCarrier>,
) -> Vec<crate::records::component_path::DesignComponentPath> {
    use crate::records::component_path::{
        ComponentPathTransform, DesignComponentPath, DesignComponentPathElement,
        DesignComponentPathElementKind as Kind,
    };
    let occurrence_records = occurrences
        .iter()
        .map(|occurrence| u64::from(occurrence.record_index))
        .collect::<std::collections::BTreeSet<_>>();
    let component_records = occurrences
        .iter()
        .map(|occurrence| occurrence.component_record_index)
        .collect::<std::collections::BTreeSet<_>>();
    carriers
        .into_iter()
        .filter_map(|carrier| {
            let transform = carrier
                .transform
                .map(ComponentPathTransform::try_from)
                .transpose()
                .ok()?;
            Some(DesignComponentPath {
                id: format!(
                    "{scope}:design-component-path#{}",
                    carrier.carrier_record_index
                ),
                carrier_record_index: carrier.carrier_record_index,
                elements: carrier
                    .elements
                    .into_iter()
                    .map(|(record_index, discriminator)| DesignComponentPathElement {
                        record_index,
                        kind: if occurrence_records.contains(&record_index) {
                            Kind::Occurrence
                        } else if component_records.contains(&record_index) {
                            Kind::Component
                        } else {
                            Kind::Other
                        },
                        discriminator,
                    })
                    .collect(),
                transform,
                frame_bytes: carrier.frame_bytes.into(),
            })
        })
        .collect()
}

fn unproven_identity_loss(
    occurrence: &DesignComponentOccurrence,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    occurrence.has_unresolved_identity_ordinal()
        .then(|| crate::loss::F3dLossCode::LocalOccurrenceIdentityOrdinalUnresolved.note(format!(
            "local component occurrence {} has identity flag 1 and ordinal {}; its placement remains unresolved (DR-146)", occurrence.id, occurrence.occurrence_ordinal()
        )))
}

/// Give each occurrence the pose of the one component-path carrier in its
/// Design stream that names its component and carrier records; return how
/// many occurrences two or more carriers name.
pub(crate) fn bind_current_poses(
    occurrences: &mut [DesignComponentOccurrence],
    poses: &[crate::xref::LocalComponentPathPose],
) -> usize {
    let mut ambiguous = 0;
    for occurrence in occurrences {
        let matching = poses
            .iter()
            .filter(|pose| {
                pose.component_record_index == occurrence.component_record_index
                    && pose.occurrence_record_index == u64::from(occurrence.record_index)
            })
            .collect::<Vec<_>>();
        let pose = match matching.as_slice() {
            [] => continue,
            [pose] => pose,
            _ => {
                ambiguous += 1;
                continue;
            }
        };
        let Ok(transform) = pose
            .transform
            .map(crate::records::xref::XrefPlacementTransform::try_from)
            .transpose()
        else {
            continue;
        };
        occurrence.current_pose = Some(crate::records::xref::XrefCurrentPose {
            carrier_record_index: pose.carrier_record_index,
            frame_bytes: pose.frame_bytes.clone().into(),
            transform,
        });
    }
    ambiguous
}

/// The local occurrences that two or more component-path carriers name.
pub(crate) fn ambiguous_current_poses_loss(
    count: usize,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (count != 0).then(|| crate::loss::F3dLossCode::LocalOccurrenceCurrentPoseAmbiguous.note(format!(
        "{count} local component occurrence(s) are named by two or more component-path carriers; they keep their stored placement unless a component-path pose resolves them to one expanded occurrence"
    )))
}

fn occurrence_frames(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<crate::metastream::PrimaryRecordFrame>, CodecError> {
    let mut placement_entities = std::collections::BTreeMap::new();
    for (ordinal, design_type) in meta.types.iter().enumerate().filter(|(_, row)| {
        row.module == "Component"
            && row
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(OCCURRENCE_PLACEMENT_TYPE_GUID)
    }) {
        for &entity in design_type.entities.values() {
            placement_entities.insert(entity, ordinal);
        }
    }
    let mut frames = Vec::new();
    for frame in crate::metastream::primary_record_frames(meta, bytes.len())? {
        let Some(&ordinal) = placement_entities.get(&frame.entity_id) else {
            continue;
        };
        let header = &bytes[frame.start..frame.member_end];
        let parsed_header = lp_ascii_filtered_view(header, 0, 3..=3, u8::is_ascii_digit);
        let class_matches = parsed_header
            .and_then(|(tag, _)| tag.parse::<usize>().ok())
            .and_then(|tag| tag.checked_sub(256))
            == Some(ordinal);
        let entity_matches = parsed_header
            .and_then(|(_, after_tag)| View::u32_le_at(header, after_tag))
            .map(u64::from)
            == Some(frame.entity_id);
        if !class_matches || !entity_matches {
            return Err(CodecError::malformed(
                "F3D local occurrence header disagrees with its registered primary index",
            ));
        }
        frames.push(frame);
    }
    Ok(frames)
}

fn admit_occurrence_envelopes(
    occurrences: &mut Vec<DesignComponentOccurrence>,
    primary: &std::collections::BTreeSet<(u64, u64)>,
    typed_primary: &std::collections::BTreeSet<(u64, u64)>,
) -> usize {
    let before = occurrences.len();
    occurrences.retain(|occurrence| {
        let admitted = if occurrence.owner_type_guid.is_some() {
            typed_primary
        } else {
            primary
        };
        admitted.contains(&(u64::from(occurrence.record_index), occurrence.byte_offset()))
    });
    before - occurrences.len()
}

pub(crate) fn superseded_occurrences_loss(
    count: usize,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (count != 0).then(|| crate::loss::F3dLossCode::LocalOccurrenceEnvelopeSuperseded.note(format!(
        "{count} local component-occurrence frame(s) were not decoded because their offset and entity were not selected by the primary record index or, for typed references, the occurrence-placement type row."
    )))
}

/// Most attributes an attributed occurrence frame can carry.
const MAX_OCCURRENCE_ATTRIBUTES: u32 = 16;
/// Read stored local occurrence attributes and its occurrence key.
fn attributed_component_occurrence(
    bytes: &[u8],
    start: usize,
    stream: &str,
) -> Option<DesignComponentOccurrence> {
    use crate::records::feature::assembly_features::{
        DesignAttributedOccurrenceFrame, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement, DesignOccurrenceAttribute,
        DesignOccurrenceAttributeValue,
    };
    let (class_tag, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    let record_index = View::u32_le_at(bytes, after_tag)?;
    let end = next_indexed_record_offset(bytes, start.checked_add(1)?)?;
    let frame = bytes.get(start..end)?;
    if frame.get(attributed_prefix::EMPTY_INSTANCE_NAME..attributed_prefix::PATH_MARKER)? != [0; 8]
        || frame.get(attributed_prefix::PATH_MARKER) != Some(&attributed_prefix::PATH_MARKER_VALUE)
        || View::u32_le_at(frame, attributed_prefix::PATH_COUNT)?
            != attributed_prefix::PATH_COUNT_VALUE
    {
        return None;
    }
    let mut reference_end = attributed_prefix::COMPONENT_REFERENCE;
    let component = component_reference(frame, &mut reference_end)?;
    let (discriminator_at, marker_at, count_at, mut at) = if component.1.is_some() {
        (
            typed_attributed_prefix::PATH_DISCRIMINATOR,
            typed_attributed_prefix::ATTRIBUTES_MARKER,
            typed_attributed_prefix::ATTRIBUTE_COUNT,
            typed_attributed_prefix::LEN,
        )
    } else {
        (
            attributed_prefix::PATH_DISCRIMINATOR,
            attributed_prefix::ATTRIBUTES_MARKER,
            attributed_prefix::ATTRIBUTE_COUNT,
            attributed_prefix::LEN,
        )
    };
    let marker_value = if component.1.is_some() {
        typed_attributed_prefix::ATTRIBUTES_MARKER_VALUE
    } else {
        attributed_prefix::ATTRIBUTES_MARKER_VALUE
    };
    if reference_end != discriminator_at || frame.get(marker_at) != Some(&marker_value) {
        return None;
    }
    let component_record_index = component.0;
    let count = View::u32_le_at(frame, count_at)?;
    if !(1..=MAX_OCCURRENCE_ATTRIBUTES).contains(&count) {
        return None;
    }
    let mut attributes = Vec::new();
    for _ in 0..count {
        let (name, after_name) =
            lp_ascii_filtered_view(frame, at, 1..=64, u8::is_ascii_alphanumeric)?;
        let (kind, after_kind) =
            lp_ascii_filtered_view(frame, after_name, 1..=64, u8::is_ascii_alphanumeric)?;
        let (value, after_value) = match kind {
            "IntrinsicMetaTypebool" => match frame.get(after_kind)? {
                0 => (DesignOccurrenceAttributeValue::Bool(false), after_kind + 1),
                1 => (DesignOccurrenceAttributeValue::Bool(true), after_kind + 1),
                _ => return None,
            },
            "IntrinsicMetaTypeHString" => {
                let (text, after_text) = lp_utf16_bounded(frame, after_kind, 0..=256)?;
                (DesignOccurrenceAttributeValue::Text(text), after_text)
            }
            _ => return None,
        };
        attributes.push(DesignOccurrenceAttribute {
            name: name.to_owned(),
            value,
        });
        at = after_value;
    }
    let ordinal_prefix = View::u16_le_at(frame, at)?;
    let ordinal = std::num::NonZeroU32::new(View::u32_le_at(frame, at + 2)?)?;
    let path_discriminator = View::u32_le_at(frame, discriminator_at)?;
    let component_guid_at = at + 6;
    let (component_guid, after_component) = lp_utf16_bounded(frame, component_guid_at, 36..=36)?;
    let occurrence_key_at = after_component;
    let (occurrence_key, after_key) = lp_utf16_bounded(frame, occurrence_key_at, 1..=36)?;
    let mut key_end = after_key;
    let crate::bytes::Reference::CrossSegment {
        target: key_record_index,
        inline_type_guid: None,
        segment: key_segment_index,
    } = crate::bytes::take_owned_reference(frame, &mut key_end)?
    else {
        return None;
    };
    let mut repeat_end = key_end + 1;
    if frame.get(key_end) != Some(&0) || component_reference(frame, &mut repeat_end)? != component {
        return None;
    }
    let flag_at = repeat_end;
    let (placement, transform_offset, tail_at) = match frame.get(flag_at)? {
        1 if ordinal.get() == 1 => (DesignComponentOccurrencePlacement::Base, None, flag_at + 1),
        1 => (
            DesignComponentOccurrencePlacement::Identity { ordinal },
            None,
            flag_at + 1,
        ),
        0 => {
            let transform = super::scopes::shared_frames::rigid_transform_at(frame, flag_at + 1)?;
            (
                DesignComponentOccurrencePlacement::Explicit { ordinal, transform },
                Some(flag_at + 1),
                flag_at + 1 + MATRIX_BYTES,
            )
        }
        _ => return None,
    };
    if frame.get(tail_at..tail_at + 9)? != [0; 9] {
        return None;
    }
    let mut owner_end = tail_at + 9;
    let owner_reference = crate::bytes::take_reference(frame, &mut owner_end)?;
    let (owner_entity, owner_type) = owner_reference.local()?;
    if owner_type.is_some() != component.1.is_some()
        || owner_end != frame.len()
        || owner_type.is_some_and(|guid| !guid.eq_ignore_ascii_case(OWNER_TYPE_GUID))
    {
        return None;
    }
    let absolute = |offset: usize| u64::try_from(start.checked_add(offset)?).ok();
    let mut occurrence = DesignComponentOccurrence::try_new_attributed(
        DesignComponentOccurrenceDraft {
            id: format!("{stream}:design-component-occurrence#{start}"),
            class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
            record_index,
            byte_offset: u64::try_from(start).ok()?,
            component_record_index,
            component_guid: crate::records::mesh::DesignRelaxedGuidText::try_from(component_guid)
                .ok()?,
            occurrence_guid: crate::records::mesh::DesignOccurrenceKey::try_from(occurrence_key)
                .ok()?,
            placement,
            owner_entity,
        },
        DesignAttributedOccurrenceFrame {
            attributes,
            ordinal_prefix,
            path_discriminator,
            key_record_index,
            key_segment_index,
            component_guid_offset: absolute(component_guid_at + 4)?,
            occurrence_guid_offset: absolute(occurrence_key_at + 4)?,
            transform_offset: match transform_offset {
                Some(offset) => Some(absolute(offset)?),
                None => None,
            },
            owner_entity_offset: absolute(tail_at + 10)?,
        },
    )
    .ok()?;
    occurrence.owner_type_guid = owner_type
        .map(|guid| guid.to_owned().try_into())
        .transpose()
        .ok()?;
    Some(occurrence)
}

fn has_empty_attributes(frame: &[u8]) -> bool {
    frame.get(empty_prefix::ATTRIBUTES_MARKER) == Some(&empty_prefix::ATTRIBUTES_MARKER_VALUE)
        && View::u32_le_at(frame, empty_prefix::ATTRIBUTE_COUNT)
            == Some(empty_prefix::ATTRIBUTE_COUNT_VALUE)
}

fn exact_component_occurrence_frame(
    bytes: &[u8],
    start: usize,
    end: usize,
    stream: &str,
) -> Option<DesignComponentOccurrence> {
    const SUFFIX: &str = ":design-component-occurrence#";

    let frame = bytes.get(start..end)?;
    let empty_attributes = has_empty_attributes(frame);
    let shift = if empty_attributes {
        empty_prefix::LEN - prefix::LEN
    } else {
        0
    };
    let offset = |at: usize| {
        if at >= prefix::OCCURRENCE_ORDINAL {
            at + shift
        } else {
            at
        }
    };
    if !matches!(
        frame.len().checked_sub(shift)?,
        BASE_FRAME_LENGTH | PLACED_FRAME_LENGTH
    ) {
        return typed_component_occurrence(frame, start, stream);
    }
    let parsed = (|| {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if after_tag != start.checked_add(7)? {
            return None;
        }
        let record_index = View::u32_le_at(bytes, after_tag)?;
        let frame_length = end.checked_sub(start)?.checked_sub(shift)?;
        let at = |relative: usize| start + offset(relative);
        if !matches!(frame_length, BASE_FRAME_LENGTH | PLACED_FRAME_LENGTH)
            || bytes.get(at(prefix::EMPTY_INSTANCE_NAME)..at(prefix::PATH_MARKER))? != [0; 8]
            || bytes.get(at(prefix::PATH_MARKER)) != Some(&prefix::PATH_MARKER_VALUE)
            || View::u32_le_at(bytes, at(prefix::PATH_COUNT))? != prefix::PATH_COUNT_VALUE
            || (!empty_attributes
                && bytes.get(at(prefix::PATH_TERMINATOR)) != Some(&prefix::PATH_TERMINATOR_VALUE))
            || bytes.get(at(prefix::COMPONENT_REFERENCE)) != Some(&1)
            || bytes.get(at(prefix::COMPONENT_REFERENCE_REPEAT) - 1) != Some(&0)
            || bytes.get(at(prefix::COMPONENT_REFERENCE_REPEAT)) != Some(&1)
        {
            return None;
        }
        let component_record_index = View::u64_le_at(bytes, at(prefix::COMPONENT_REFERENCE) + 1)?;
        if View::u64_le_at(bytes, at(prefix::COMPONENT_REFERENCE_REPEAT) + 1)?
            != component_record_index
        {
            return None;
        }
        let occurrence_ordinal =
            std::num::NonZeroU32::new(View::u32_le_at(bytes, at(prefix::OCCURRENCE_ORDINAL))?)?;
        if View::u32_le_at(bytes, at(prefix::CARRIER_GUID)) != Some(GUID_UNITS)
            || bytes
                .get(at(prefix::CARRIER_GUID) + 4..at(prefix::OCCURRENCE_GUID))
                .is_none()
            || View::u32_le_at(bytes, at(prefix::OCCURRENCE_GUID)) != Some(GUID_UNITS)
            || bytes
                .get(at(prefix::OCCURRENCE_GUID) + 4..at(prefix::COMPONENT_REFERENCE_REPEAT) - 1)
                .is_none()
            || bytes.get(
                at(prefix::COMPONENT_REFERENCE_REPEAT) + MARKED_U64..at(prefix::IDENTITY_FLAG),
            )? != [0; 2]
        {
            return None;
        }
        let tail = at(prefix::LEN);
        let (placement, owner_entity) = match frame_length {
            BASE_FRAME_LENGTH => {
                if bytes.get(at(prefix::IDENTITY_FLAG)) != Some(&1)
                    || bytes.get(tail..tail + identity_tail::OWNER_MARKER)? != [0; 9]
                    || bytes.get(tail + identity_tail::OWNER_MARKER)
                        != Some(&identity_tail::OWNER_MARKER_VALUE)
                    || bytes.get(tail + identity_tail::OWNER_ENTITY + 8..at(BASE_FRAME_LENGTH))?
                        != [0; 2]
                {
                    return None;
                }
                (
                    if occurrence_ordinal.get() == 1 {
                        crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base
                    } else {
                        crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Identity { ordinal: occurrence_ordinal }
                    },
                    View::u64_le_at(bytes, tail + identity_tail::OWNER_ENTITY)?,
                )
            }
            PLACED_FRAME_LENGTH => {
                if (class_tag == "256" && occurrence_ordinal.get() < 2)
                    || bytes.get(at(prefix::IDENTITY_FLAG)) != Some(&0)
                    || bytes.get(tail + MATRIX_BYTES..tail + matrix_tail::OWNER_MARKER)? != [0; 9]
                    || bytes.get(tail + matrix_tail::OWNER_MARKER)
                        != Some(&matrix_tail::OWNER_MARKER_VALUE)
                    || bytes.get(tail + matrix_tail::OWNER_ENTITY + 8..at(PLACED_FRAME_LENGTH))?
                        != [0; 2]
                {
                    return None;
                }
                let transform = super::scopes::shared_frames::rigid_transform_at(bytes, tail)?;
                (
                    crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                        ordinal: occurrence_ordinal,
                        transform,
                    },
                    View::u64_le_at(bytes, tail + matrix_tail::OWNER_ENTITY)?,
                )
            }
            _ => return None,
        };
        let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
        let byte_offset = u64::try_from(start).ok()?;
        Some((
            class_tag,
            record_index,
            byte_offset,
            component_record_index,
            placement,
            owner_entity,
        ))
    })();
    let (class_tag, record_index, byte_offset, component_record_index, placement, owner_entity) =
        parsed?;
    let (component_guid, after_component) =
        lp_utf16_bounded(bytes, start + offset(prefix::CARRIER_GUID), 36..=36)?;
    let (occurrence_guid, after_occurrence) =
        lp_utf16_bounded(bytes, start + offset(prefix::OCCURRENCE_GUID), 36..=36)?;
    if after_component != start + offset(prefix::OCCURRENCE_GUID)
        || after_occurrence != start + offset(prefix::COMPONENT_REFERENCE_REPEAT) - 1
    {
        return None;
    }
    let Ok(component_guid) = crate::records::mesh::DesignRelaxedGuidText::try_from(component_guid)
    else {
        return None;
    };
    let Ok(occurrence_guid) =
        crate::records::mesh::DesignRelaxedGuidText::try_from(occurrence_guid)
    else {
        return None;
    };
    let id = format!("{stream}{SUFFIX}{start}");
    let mut occurrence = DesignComponentOccurrence::try_new(
        crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
            id,
            class_tag,
            record_index,
            byte_offset,
            component_record_index,
            component_guid,
            occurrence_guid: occurrence_guid.into(),
            placement,
            owner_entity,
        },
    )
    .ok()?;
    if empty_attributes {
        occurrence = occurrence.with_empty_attributes().ok()?;
    }
    occurrence.path_discriminator =
        std::num::NonZeroU32::new(View::u32_le_at(bytes, start + prefix::PATH_DISCRIMINATOR)?);
    Some(occurrence)
}

fn component_reference<'a>(bytes: &'a [u8], at: &mut usize) -> Option<(u64, Option<&'a str>)> {
    let crate::bytes::Reference::Local {
        target,
        inline_type_guid: inline_type,
    } = crate::bytes::take_reference(bytes, at)?
    else {
        return None;
    };
    if inline_type.is_some_and(|guid| {
        !guid.eq_ignore_ascii_case(crate::records::feature::occurrence_transfer::COMPONENT_TYPE)
    }) {
        return None;
    }
    Some((target, inline_type))
}

fn typed_component_occurrence(
    frame: &[u8],
    start: usize,
    stream: &str,
) -> Option<DesignComponentOccurrence> {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
    };
    let (repeat_at, flag_at) = match frame.len() {
        length
            if length == typed_prefix::LEN + typed_identity_tail::LEN
                || length == typed_prefix::LEN + typed_matrix_tail::LEN =>
        {
            if frame.get(typed_prefix::SEPARATOR) != Some(&typed_prefix::SEPARATOR_VALUE) {
                return None;
            }
            (
                typed_prefix::COMPONENT_REFERENCE_REPEAT,
                typed_prefix::IDENTITY_FLAG,
            )
        }
        length
            if length == compact_prefix::LEN + typed_identity_tail::LEN
                || length == compact_prefix::LEN + typed_matrix_tail::LEN =>
        {
            (
                compact_prefix::COMPONENT_REFERENCE_REPEAT,
                compact_prefix::IDENTITY_FLAG,
            )
        }
        _ => return None,
    };
    let (class_tag, after_tag) = lp_ascii_filtered_view(frame, 0, 3..=3, u8::is_ascii_digit)?;
    let record_index = View::u32_le_at(frame, after_tag)?;
    if frame.get(typed_prefix::EMPTY_INSTANCE_NAME..typed_prefix::PATH_MARKER)? != [0; 8]
        || frame.get(typed_prefix::PATH_MARKER) != Some(&typed_prefix::PATH_MARKER_VALUE)
        || View::u32_le_at(frame, typed_prefix::PATH_COUNT)? != typed_prefix::PATH_COUNT_VALUE
    {
        return None;
    }
    let mut at = typed_prefix::COMPONENT_REFERENCE;
    let component = component_reference(frame, &mut at)?;
    if component.1.is_none()
        || at != typed_prefix::PATH_DISCRIMINATOR
        || frame.get(typed_prefix::PATH_TERMINATOR) != Some(&typed_prefix::PATH_TERMINATOR_VALUE)
    {
        return None;
    }
    let ordinal =
        std::num::NonZeroU32::new(View::u32_le_at(frame, typed_prefix::OCCURRENCE_ORDINAL)?)?;
    let (component_guid, after_component) =
        lp_utf16_bounded(frame, typed_prefix::CARRIER_GUID, 36..=36)?;
    let (occurrence_guid, after_occurrence) =
        lp_utf16_bounded(frame, typed_prefix::OCCURRENCE_GUID, 36..=36)?;
    if after_component != typed_prefix::OCCURRENCE_GUID
        || after_occurrence != compact_prefix::COMPONENT_REFERENCE_REPEAT
    {
        return None;
    }
    at = repeat_at;
    if component_reference(frame, &mut at)? != component || at != flag_at {
        return None;
    }
    at += 1;
    let placement = match frame.get(flag_at)? {
        1 if ordinal.get() == 1 => DesignComponentOccurrencePlacement::Base,
        0 if class_tag != "256" || ordinal.get() > 1 => {
            let transform = super::scopes::shared_frames::rigid_transform_at(frame, at)?;
            at += MATRIX_BYTES;
            DesignComponentOccurrencePlacement::Explicit { ordinal, transform }
        }
        _ => return None,
    };
    let owner_at = at.checked_add(typed_identity_tail::OWNER_REFERENCE)?;
    if frame.get(at..owner_at)? != [0; 9] {
        return None;
    }
    at = owner_at;
    let crate::bytes::Reference::Local {
        target: owner_entity,
        inline_type_guid: Some(owner_type),
    } = crate::bytes::take_reference(frame, &mut at)?
    else {
        return None;
    };
    if at != frame.len() || !owner_type.eq_ignore_ascii_case(OWNER_TYPE_GUID) {
        return None;
    }
    let mut occurrence = DesignComponentOccurrence::try_new_typed(
        DesignComponentOccurrenceDraft {
            id: format!("{stream}:design-component-occurrence#{start}"),
            class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
            record_index,
            byte_offset: u64::try_from(start).ok()?,
            component_record_index: component.0,
            component_guid: crate::records::mesh::DesignRelaxedGuidText::try_from(component_guid)
                .ok()?,
            occurrence_guid: crate::records::mesh::DesignRelaxedGuidText::try_from(occurrence_guid)
                .ok()?
                .into(),
            placement,
            owner_entity,
        },
        flag_at == typed_prefix::IDENTITY_FLAG,
    )
    .ok()?;
    occurrence.path_discriminator =
        std::num::NonZeroU32::new(View::u32_le_at(frame, typed_prefix::PATH_DISCRIMINATOR)?);
    occurrence.owner_type_guid = Some(owner_type.to_owned().try_into().ok()?);
    Some(occurrence)
}

/// Instance name of the occurrence-placement record that places the root component.
pub(crate) const ROOT_INSTANCE_NAME: &[u8] = b"rootInstance";

/// Naming space of one local component definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComponentDefinition {
    /// Root entity of the component naming space.
    pub(crate) root_entity: u64,
    /// UUID that selects the component naming space.
    pub(crate) context_uuid: crate::records::mesh::DesignRelaxedGuidText,
    /// The component's name, when its definition record stores one.
    pub(crate) name: Option<String>,
}

/// Local component definitions of one Design stream.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StreamComponents {
    pub(crate) root_entities: std::collections::BTreeSet<u64>,
    /// Component definition record to the naming space whose binding it stores.
    pub(crate) roots: std::collections::BTreeMap<u64, ComponentDefinition>,
    /// Component definition placed by the root instance, when exactly one names it.
    pub(crate) root_component: Option<u64>,
    /// Later revisions of a component definition record whose context binding
    /// is not the one that defines the component.
    pub(crate) revised_definitions: usize,
    /// Named empty-path root records without a decoded definition reference.
    pub(crate) unbound_root_instances: usize,
}

impl StreamComponents {
    pub(crate) fn cluster_owner(&self, entity: u64) -> Option<&ComponentDefinition> {
        let start = self
            .root_entities
            .iter()
            .copied()
            .chain(self.roots.values().map(|root| root.root_entity))
            .filter(|start| *start <= entity)
            .max()?;
        self.roots.values().find(|root| root.root_entity == start)
    }

    fn bind_roots(&mut self, bytes: &[u8], frames: impl IntoIterator<Item = (usize, usize)>) {
        let mut roots = std::collections::BTreeSet::new();
        for (start, end) in frames {
            if let Some(root) = root_instance_component(bytes, start, end) {
                roots.insert(root);
            } else if root_instance_path_count(bytes, start, end) == Some(0) {
                self.unbound_root_instances += 1;
            }
        }
        self.root_component = if roots.len() == 1 {
            roots.into_iter().next()
        } else {
            None
        };
    }

    /// Bind each definition frame to the naming space of its earliest
    /// binding. A later revision's binding selects the same naming space and
    /// is counted in `revised_definitions`.
    fn bind_definitions<'a>(
        &mut self,
        definitions: impl IntoIterator<
            Item = (u64, &'a crate::records::recipes::DesignComponentNamingSpace),
        >,
    ) {
        let mut root_offsets = std::collections::BTreeMap::<u64, u64>::new();
        for (frame_entity, space) in definitions {
            if let Some(earlier) = root_offsets.get(&frame_entity) {
                self.revised_definitions += 1;
                if *earlier < space.byte_offset {
                    continue;
                }
            }
            root_offsets.insert(frame_entity, space.byte_offset);
            self.roots.insert(
                frame_entity,
                ComponentDefinition {
                    root_entity: space.component_record_index,
                    context_uuid: space.context_uuid.clone(),
                    name: space.name.clone(),
                },
            );
        }
    }
}

/// The loss note for later component definition revisions across every
/// Design stream, when any stream has one.
pub(crate) fn revised_definitions_loss(
    components: &std::collections::BTreeMap<String, StreamComponents>,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let revised = components
        .values()
        .map(|components| components.revised_definitions)
        .sum::<usize>();
    (revised != 0).then(|| {
        crate::loss::F3dLossCode::ComponentDefinitionRevised.note(format!(
            "{revised} later revision(s) of component definition records bind their own context UUID; the earliest record defines each component, and every bound UUID selects its naming space."
        ))
    })
}

/// Registered component entities with no context-UUID binding in this
/// document; the session reports the dropped naming spaces.
pub(crate) fn unbound_naming_spaces_loss(
    count: usize,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (count != 0).then(|| crate::loss::F3dLossCode::ComponentNamingSpaceUnbound.note(format!(
        "{count} registered Design component(s) have no context-UUID binding in this document; their naming spaces were dropped and occurrences naming them stay unplaced."
    )))
}

pub(crate) fn unbound_roots_loss(
    components: &std::collections::BTreeMap<String, StreamComponents>,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let count = components
        .values()
        .map(|components| components.unbound_root_instances)
        .sum::<usize>();
    (count != 0).then(|| crate::loss::F3dLossCode::RootInstanceDefinitionUnbound.note(format!(
        "{count} named root-instance record(s) have empty paths without a decoded component-definition reference; these records do not bind a root definition."
    )))
}

/// Join every Design stream's component definition records to their naming
/// spaces and identify the root component. The map key is the native stream
/// scope of the Design `BulkStream`.
pub(crate) fn decode_local_components(
    scan: &ContainerScan,
    naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
) -> Result<std::collections::BTreeMap<String, StreamComponents>, CodecError> {
    let mut out = std::collections::BTreeMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let scope = crate::ids::native_scope(&entry.name);
        let spaces = naming_spaces
            .iter()
            .filter(|space| crate::ids::native_stream(&space.id) == Some(scope.as_str()))
            .collect::<Vec<_>>();
        // A stream with no bound naming space contributes no entry: an empty
        // StreamComponents holds no definition and no root, so every lookup
        // misses the same way with or without it. Occurrences that name its
        // dropped components stay unplaced through the existing
        // NoRootComponent and DefinitionUnbound counts.
        if spaces.is_empty() {
            continue;
        }
        let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Ok(frames) = super::meta::design_primary_frames(bytes, &meta) else {
            continue;
        };
        let mut components = StreamComponents::default();
        let mut definitions = Vec::new();
        for space in spaces {
            components
                .root_entities
                .insert(space.component_record_index);
            let Ok(offset) = usize::try_from(space.byte_offset) else {
                continue;
            };
            let Some(frame) = frames
                .get(frames.partition_point(|frame| frame.end <= offset))
                .filter(|frame| frame.start <= offset)
            else {
                continue;
            };
            definitions.push((frame.entity_id, space));
        }
        components.bind_definitions(definitions);
        let root_frames = frames.iter().filter(|frame| {
            frame.design_type.module == "Component"
                && frame
                    .design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(OCCURRENCE_PLACEMENT_TYPE_GUID)
        });
        components.bind_roots(bytes, root_frames.map(|frame| (frame.start, frame.end)));
        out.insert(scope, components);
    }
    Ok(out)
}

fn root_instance_path_count(bytes: &[u8], start: usize, end: usize) -> Option<u32> {
    let frame = bytes.get(start..end)?;
    if frame.len() < empty_root_instance::LEN
        || View::u32_le_at(frame, root_instance::LEADING_ZERO)? != root_instance::LEADING_ZERO_VALUE
        || View::u32_le_at(frame, root_instance::INSTANCE_NAME_LENGTH)?
            != root_instance::INSTANCE_NAME_LENGTH_VALUE
        || frame.get(root_instance::INSTANCE_NAME..root_instance::PATH_MARKER)?
            != ROOT_INSTANCE_NAME
        || frame.get(root_instance::PATH_MARKER) != Some(&root_instance::PATH_MARKER_VALUE)
    {
        return None;
    }
    View::u32_le_at(frame, root_instance::PATH_COUNT)
}

/// The component definition named by a root-instance placement record.
fn root_instance_component(bytes: &[u8], start: usize, end: usize) -> Option<u64> {
    let count = root_instance_path_count(bytes, start, end)?;
    let frame = bytes.get(start..end)?;
    if count == empty_root_frame::PATH_COUNT_VALUE {
        let (reference_at, flag_at, zero_at) = match frame.len() {
            empty_root_frame::LEN => (
                empty_root_frame::COMPONENT_REFERENCE,
                empty_root_frame::IDENTITY_FLAG,
                empty_root_frame::ZERO_TAIL,
            ),
            separated_root_frame::LEN
                if frame.get(separated_root_frame::REFERENCE_SEPARATOR)
                    == Some(&separated_root_frame::REFERENCE_SEPARATOR_VALUE) =>
            {
                (
                    separated_root_frame::COMPONENT_REFERENCE,
                    separated_root_frame::IDENTITY_FLAG,
                    separated_root_frame::ZERO_TAIL,
                )
            }
            _ => return None,
        };
        if frame.get(empty_root_frame::SEPARATOR) != Some(&empty_root_frame::SEPARATOR_VALUE)
            || View::u32_le_at(frame, empty_root_frame::WORD)? != empty_root_frame::WORD_VALUE
            || frame.get(flag_at) != Some(&empty_root_frame::IDENTITY_FLAG_VALUE)
            || frame.get(zero_at..)? != [0; 10]
        {
            return None;
        }
        let (carrier_guid, after_carrier) =
            lp_utf16_bounded(frame, empty_root_frame::CARRIER_GUID, 36..=36)?;
        let (occurrence_guid, after_occurrence) =
            lp_utf16_bounded(frame, empty_root_frame::OCCURRENCE_GUID, 36..=36)?;
        crate::records::mesh::DesignRelaxedGuidText::try_from(carrier_guid).ok()?;
        crate::records::mesh::DesignRelaxedGuidText::try_from(occurrence_guid).ok()?;
        if after_carrier != empty_root_frame::OCCURRENCE_GUID
            || after_occurrence != empty_root_frame::COMPONENT_REFERENCE
        {
            return None;
        }
        let mut at = reference_at;
        let (component, Some(_)) = component_reference(frame, &mut at)? else {
            return None;
        };
        return (at == flag_at).then_some(component);
    }
    if count != root_instance::PATH_COUNT_VALUE {
        return None;
    }
    if frame.len() < root_instance::LEN
        || frame.get(root_instance::COMPONENT_MARKER)
            != Some(&root_instance::COMPONENT_MARKER_VALUE)
    {
        return None;
    }
    View::u64_le_at(frame, root_instance::COMPONENT_REFERENCE)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::trivially_copy_pass_by_ref)]

    mod component_path;
    mod current_pose;
    mod original;

    use super::{attributed_component_occurrence, root_instance_component};
    use crate::test_support::indexed_header;

    const COMPONENT: &str = "a989beb9-467b-4afa-9e90-a9329a2ca258";
    const OCCURRENCE: &str = "f2371d14-7339-4f5c-82a1-50ec8fca5597";

    fn guid(bytes: &mut [u8], at: usize, value: &str) {
        bytes[at..at + 4].copy_from_slice(&36_u32.to_le_bytes());
        for (ordinal, unit) in value.encode_utf16().enumerate() {
            bytes[at + 4 + ordinal * 2..at + 6 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    fn exact_component_occurrence(
        bytes: &[u8],
        start: usize,
        stream: &str,
    ) -> Option<crate::records::feature::assembly_features::DesignComponentOccurrence> {
        let end = super::next_indexed_record_offset(bytes, start.checked_add(1)?)?;
        super::exact_component_occurrence_frame(bytes, start, end, stream)
    }

    fn common(frame_length: usize, ordinal: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"256", 20);
        bytes.resize(frame_length, 0);
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[24] = 1;
        bytes[25..33].copy_from_slice(&10_u64.to_le_bytes());
        bytes[40..44].copy_from_slice(&ordinal.to_le_bytes());
        guid(&mut bytes, 44, COMPONENT);
        guid(&mut bytes, 120, OCCURRENCE);
        bytes[197] = 1;
        bytes[198..206].copy_from_slice(&10_u64.to_le_bytes());
        bytes
    }

    fn empty_attribute_frame(matrix: bool) -> Vec<u8> {
        let mut frame = common(if matrix { 357 } else { 229 }, if matrix { 2 } else { 1 });
        if matrix {
            for (i, value) in [
                1.0_f64, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 3.0, 0.0, 0.0, 1.0, 4.0, 0.0, 0.0, 0.0, 1.0,
            ]
            .into_iter()
            .enumerate()
            {
                frame[209 + i * 8..217 + i * 8].copy_from_slice(&value.to_le_bytes());
            }
            frame[346] = 1;
        } else {
            frame[208] = 1;
            frame[218] = 1;
        }
        frame[39] = 1;
        frame.splice(40..40, [0; 4]);
        indexed_header(&mut frame, *b"333", 21);
        frame
    }

    #[test]
    fn empty_attribute_occurrence_forms_validate_offsets_and_refuse_missing_bytes() {
        for matrix in [false, true] {
            let frame = empty_attribute_frame(matrix);
            let record =
                exact_component_occurrence(&frame, 0, "f3d:Design/BulkStream.dat").unwrap();
            assert_eq!(record.component_guid.as_str(), COMPONENT);
            assert_eq!(record.occurrence_guid.as_str(), OCCURRENCE);
            assert_eq!(record.occurrence_ordinal(), if matrix { 2 } else { 1 });
            assert_eq!(record.transform().map(|v| v.offset), matrix.then_some(213));
            if let Some(transform) = record.transform() {
                let expected: crate::records::sketch_placement::SketchPlacementMatrix = [
                    [1.0, 0.0, 0.0, 2.0],
                    [0.0, 1.0, 0.0, 3.0],
                    [0.0, 0.0, 1.0, 4.0],
                    [0.0, 0.0, 0.0, 1.0],
                ]
                .try_into()
                .unwrap();
                assert_eq!(transform.value, expected);
            }
            let wire = serde_json::to_value(&record).unwrap();
            assert_eq!(wire["component_guid_offset"], 52);
            assert_eq!(wire["occurrence_guid_offset"], 128);
            assert_eq!(wire["owner_entity_offset"], if matrix { 351 } else { 223 });
            let decoded: super::DesignComponentOccurrence =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(decoded, record);
            for field in [
                "component_guid_offset",
                "occurrence_guid_offset",
                "owner_entity_offset",
            ] {
                let mut changed = wire.clone();
                changed[field] = serde_json::json!(wire[field].as_u64().unwrap() + 1);
                assert!(
                    serde_json::from_value::<super::DesignComponentOccurrence>(changed).is_err()
                );
            }
            if matrix {
                let mut changed = wire.clone();
                changed["transform_offset"] = serde_json::json!(214);
                assert!(
                    serde_json::from_value::<super::DesignComponentOccurrence>(changed).is_err()
                );
            }
            let mut nonzero = frame.clone();
            nonzero[40..44].copy_from_slice(&1_u32.to_le_bytes());
            assert!(exact_component_occurrence(&nonzero, 0, "f3d:Design/BulkStream.dat").is_none());
            assert!(
                attributed_component_occurrence(&nonzero, 0, "f3d:Design/BulkStream.dat").is_none()
            );
            let mut truncated = frame;
            truncated.remove(truncated.len() - 12);
            assert!(
                exact_component_occurrence(&truncated, 0, "f3d:Design/BulkStream.dat").is_none()
            );
        }
    }

    #[test]
    fn fixed_identity_later_ordinal_has_an_exact_loss() {
        let mut frame = common(229, 2);
        frame[208] = 1;
        frame[218] = 1;
        indexed_header(&mut frame, *b"333", 21);
        let record = exact_component_occurrence(&frame, 0, "f3d:Design/BulkStream.dat").unwrap();
        let loss = super::unproven_identity_loss(&record).unwrap();
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::LocalOccurrenceIdentityOrdinalUnresolved.kind()
        );
        assert!(loss.message.contains("ordinal 2"));
        assert!(loss.message.contains(&record.id));
        let wire = serde_json::to_value(&record).unwrap();
        assert!(serde_json::from_value::<super::DesignComponentOccurrence>(wire).is_err());
    }

    #[test]
    fn fixed_component_occurrence_frames_distinguish_seed_and_generated_placements() {
        let mut seed = common(229, 1);
        seed[208] = 1;
        seed[218] = 1;
        indexed_header(&mut seed, *b"333", 21);
        let seed = exact_component_occurrence(&seed, 0, "f3d:Design/BulkStream.dat")
            .expect("seed occurrence");
        assert_eq!(seed.component_guid.as_str(), COMPONENT);
        assert_eq!(seed.occurrence_guid.as_str(), OCCURRENCE);
        assert_eq!(seed.occurrence_ordinal(), 1);
        assert_eq!(seed.transform(), None);

        let mut generated = common(357, 2);
        let transform: [[f64; 4]; 4] = [
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            generated[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        generated[346] = 1;
        indexed_header(&mut generated, *b"325", 21);
        let generated = exact_component_occurrence(&generated, 0, "f3d:Design/BulkStream.dat")
            .expect("generated occurrence");
        assert_eq!(generated.occurrence_ordinal(), 2);
        assert_eq!(
            generated.transform().map(|frame| frame.value),
            Some(transform.try_into().unwrap())
        );
        assert_eq!(generated.transform().map(|frame| frame.offset), Some(209));

        let mut legacy = common(229, 1);
        legacy[4..7].copy_from_slice(b"327");
        legacy[208] = 1;
        legacy[218] = 1;
        indexed_header(&mut legacy, *b"333", 21);
        let legacy = exact_component_occurrence(&legacy, 0, "f3d:Design/BulkStream.dat")
            .expect("legacy occurrence");
        assert_eq!(legacy.component_guid.as_str(), COMPONENT);
        assert_eq!(legacy.occurrence_guid.as_str(), OCCURRENCE);

        let mut legacy_placed = common(357, 1);
        legacy_placed[4..7].copy_from_slice(b"327");
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            legacy_placed[209 + ordinal * 8..217 + ordinal * 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        legacy_placed[346] = 1;
        indexed_header(&mut legacy_placed, *b"325", 21);
        let legacy_placed =
            exact_component_occurrence(&legacy_placed, 0, "f3d:Design/BulkStream.dat")
                .expect("legacy placed occurrence");
        assert_eq!(legacy_placed.occurrence_ordinal(), 1);
        assert_eq!(
            legacy_placed.transform().map(|frame| frame.value),
            Some(transform.try_into().unwrap())
        );

        // The carrier class tag is a per-file dynamic value, so the fixed frame
        // alone identifies the carrier and a third tag reads the same members.
        let mut dynamic_tag = common(357, 1);
        dynamic_tag[4..7].copy_from_slice(b"336");
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            dynamic_tag[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        dynamic_tag[346] = 1;
        indexed_header(&mut dynamic_tag, *b"325", 21);
        let dynamic_tag = exact_component_occurrence(&dynamic_tag, 0, "f3d:Design/BulkStream.dat")
            .expect("dynamic-tag placed occurrence");
        assert_eq!(dynamic_tag.class_tag.as_str(), "336");
        assert_eq!(dynamic_tag.occurrence_ordinal(), 1);
        assert_eq!(
            dynamic_tag.transform().map(|frame| frame.value),
            Some(transform.try_into().unwrap())
        );

        // A class-256 carrier still cannot use a placed frame for ordinal one.
        let mut placed_seed = common(357, 1);
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            placed_seed[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        placed_seed[346] = 1;
        indexed_header(&mut placed_seed, *b"325", 21);
        assert!(exact_component_occurrence(&placed_seed, 0, "f3d:Design/BulkStream.dat").is_none());
    }

    #[test]
    fn component_occurrence_frames_store_their_containing_component_root() {
        let mut identity = common(229, 1);
        identity[208] = 1;
        identity[218] = 1;
        identity[219..227].copy_from_slice(&277_u64.to_le_bytes());
        indexed_header(&mut identity, *b"333", 21);
        let identity = exact_component_occurrence(&identity, 0, "f3d:Design/BulkStream.dat")
            .expect("identity occurrence");
        assert_eq!(identity.owner_entity, 277);

        let mut placed = common(357, 2);
        for (ordinal, value) in [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
        .into_iter()
        .enumerate()
        {
            placed[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&f64::to_le_bytes(value));
        }
        placed[346] = 1;
        placed[347..355].copy_from_slice(&277_u64.to_le_bytes());
        indexed_header(&mut placed, *b"325", 21);
        let placed = exact_component_occurrence(&placed, 0, "f3d:Design/BulkStream.dat")
            .expect("placed occurrence");
        assert_eq!(placed.owner_entity, 277);
    }

    #[test]
    fn root_instance_record_names_the_root_component_definition() {
        let mut record = Vec::new();
        indexed_header(&mut record, *b"339", 548);
        record.extend_from_slice(&0_u32.to_le_bytes());
        record.extend_from_slice(&12_u32.to_le_bytes());
        record.extend_from_slice(b"rootInstance");
        record.push(1);
        record.extend_from_slice(&1_u32.to_le_bytes());
        record.push(1);
        record.extend_from_slice(&276_u64.to_le_bytes());
        record.extend_from_slice(&[0; 7]);
        assert_eq!(root_instance_component(&record, 0, record.len()), Some(276));

        let mut empty_attributes = record.clone();
        *empty_attributes.last_mut().unwrap() = 1;
        empty_attributes.extend_from_slice(&0_u32.to_le_bytes());
        assert_eq!(
            root_instance_component(&empty_attributes, 0, empty_attributes.len()),
            Some(276)
        );
        let mut renamed = record.clone();
        renamed[19..31].copy_from_slice(b"rootInstancf");
        assert_eq!(root_instance_component(&renamed, 0, renamed.len()), None);
        let mut two_elements = record;
        two_elements[32..36].copy_from_slice(&2_u32.to_le_bytes());
        assert_eq!(
            root_instance_component(&two_elements, 0, two_elements.len()),
            None
        );
    }

    fn push_ascii(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(
            &u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes(),
        );
        for unit in value.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }

    /// An attributed occurrence frame with a decimal occurrence key, its
    /// second occurrence (index 1), and a matrix.
    fn attributed(key: &str, index: u32, ordinal: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"373", 20);
        bytes.extend_from_slice(&[0; 8]);
        bytes.push(1);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&187_u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        push_ascii(&mut bytes, "GatedByParent");
        push_ascii(&mut bytes, "IntrinsicMetaTypebool");
        bytes.push(0);
        push_ascii(&mut bytes, "ResolutionTime");
        push_ascii(&mut bytes, "IntrinsicMetaTypeHString");
        push_utf16(&mut bytes, "");
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        push_utf16(&mut bytes, COMPONENT);
        push_utf16(&mut bytes, key);
        bytes.push(1);
        bytes.extend_from_slice(&139_u64.to_le_bytes());
        bytes.extend_from_slice(&[0, 1, 1, 0, 0, 0, 0]);
        bytes.push(1);
        bytes.extend_from_slice(&187_u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        bytes.push(0);
        let transform: [[f64; 4]; 4] = [
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for value in transform.into_iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&6_u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        indexed_header(&mut bytes, *b"262", 20);
        bytes
    }

    #[test]
    fn attributed_component_occurrence_frame_reads_a_decimal_occurrence_key() {
        let bytes = attributed("34", 1, 2);
        let occurrence = attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat")
            .expect("attributed occurrence");
        assert_eq!(occurrence.occurrence_guid.as_str(), "34");
        assert_eq!(occurrence.component_guid.as_str(), COMPONENT);
        assert_eq!(occurrence.component_record_index, 187);
        assert_eq!(occurrence.occurrence_ordinal(), 2);
        assert_eq!(occurrence.owner_entity, 6);
        let transform = occurrence.transform().expect("matrix");
        assert_eq!(transform.value.rows()[2][3], 4.0);
        let wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(wire["attributed"]["key_record_index"], 139);
        assert_eq!(wire["attributed"]["key_segment_index"], 1);
        assert_eq!(wire["attributed"]["attributes"][0]["name"], "GatedByParent");
        let round_trip: crate::records::feature::assembly_features::DesignComponentOccurrence =
            serde_json::from_value(wire).unwrap();
        assert_eq!(round_trip, occurrence);

        // A path discriminator is independent of the occurrence ordinal.
        let bytes = attributed(OCCURRENCE, 1, 2);
        assert!(attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").is_some());
        let bytes = attributed("34", 0, 2);
        let occurrence = attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat")
            .expect("independent path discriminator");
        let wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(wire["attributed"]["path_discriminator"], 0);
        assert_eq!(wire["occurrence_ordinal"], 2);
    }

    #[test]
    fn fixture_attributed_occurrence_retains_all_three_typed_properties() {
        let bytes = crate::test_support::verification::bytes(
            "xref/attributed-occurrence.bin",
            include_bytes!("../../test_support/xref/attributed-occurrence.bin"),
        );
        let occurrence = attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat")
            .expect("original attributed occurrence");
        let wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(wire["attributed"]["key_segment_index"], 10);
        assert_eq!(
            wire["attributed"]["attributes"],
            serde_json::json!([
                {"name":"GatedByParent","value":{"type":"bool","value":false}},
                {"name":"ResolutionTime","value":{"type":"text","value":""}},
                {"name":"SelfGated","value":{"type":"bool","value":false}}
            ])
        );
        let decoded: crate::records::feature::assembly_features::DesignComponentOccurrence =
            serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, occurrence);
    }

    #[test]
    fn fixture_named_root_instance_uses_the_root_component_reader() {
        let bytes = crate::test_support::verification::bytes(
            "xref/root-instance.bin",
            include_bytes!("../../test_support/xref/root-instance.bin"),
        );
        assert_eq!(root_instance_component(&bytes, 0, bytes.len()), Some(3));
        assert!(attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").is_none());
    }

    #[test]
    fn earliest_definition_binding_defines_the_component() {
        let space = |byte_offset: u64, context_uuid: &str| {
            crate::records::recipes::DesignComponentNamingSpace {
                id: format!(
                    "f3d:Design/BulkStream.dat:design-component-naming-space#{byte_offset}"
                ),
                byte_offset,
                component_record_index: 17,
                context_uuid: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    context_uuid.to_owned(),
                )
                .unwrap(),
                context_uuid_offset: byte_offset + 12,
                name: Some("Feet".to_owned()),
                name_offset: Some(byte_offset + 88),
            }
        };
        let later = space(400, OCCURRENCE);
        let earlier = space(100, COMPONENT);
        let mut components = super::StreamComponents::default();
        components.bind_definitions([(16, &later), (16, &earlier)]);
        assert_eq!(components.roots[&16].context_uuid.as_str(), COMPONENT);
        assert_eq!(components.revised_definitions, 1);

        let streams = std::collections::BTreeMap::from([("f3d:Design".to_owned(), components)]);
        let loss = super::revised_definitions_loss(&streams).expect("revision loss");
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::ComponentDefinitionRevised.kind()
        );
        let unrevised = std::collections::BTreeMap::from([(
            "f3d:Design".to_owned(),
            super::StreamComponents::default(),
        )]);
        assert!(super::revised_definitions_loss(&unrevised).is_none());
    }

    fn typed_reference(bytes: &mut Vec<u8>, target: u64, kind: &str) {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        push_ascii(bytes, kind);
        bytes.extend_from_slice(&[0; 2]);
    }

    fn typed_fixed(separated: bool, matrix: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"311", 20);
        bytes.extend_from_slice(&[0; 8]);
        bytes.push(1);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        let kind = crate::records::feature::occurrence_transfer::COMPONENT_TYPE;
        typed_reference(&mut bytes, 10, kind);
        bytes.extend_from_slice(&[0; 5]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        push_utf16(&mut bytes, COMPONENT);
        push_utf16(&mut bytes, OCCURRENCE);
        if separated {
            bytes.push(0);
        }
        typed_reference(&mut bytes, 10, kind);
        bytes.push(u8::from(!matrix));
        if matrix {
            for value in [
                1.0_f64, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 3.0, 0.0, 0.0, 1.0, 4.0, 0.0, 0.0, 0.0, 1.0,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&[0; 9]);
        typed_reference(&mut bytes, 277, super::OWNER_TYPE_GUID);
        indexed_header(&mut bytes, *b"333", 21);
        bytes
    }

    #[test]
    fn typed_component_occurrence_frames_read_both_separator_forms() {
        for (separated, matrix, length, owner_offset) in [
            (true, false, 349, 299),
            (true, true, 477, 427),
            (false, false, 348, 298),
            (false, true, 476, 426),
        ] {
            let frame = typed_fixed(separated, matrix);
            assert_eq!(frame.len(), length + 11);
            let mut bytes = vec![0; 17];
            bytes.extend_from_slice(&frame);
            let occurrence = exact_component_occurrence(&bytes, 17, "f3d:Design/BulkStream.dat")
                .expect("typed occurrence");
            assert_eq!(occurrence.component_record_index, 10);
            assert_eq!(occurrence.owner_entity, 277);
            assert_eq!(occurrence.component_guid.as_str(), COMPONENT);
            assert_eq!(occurrence.occurrence_guid.as_str(), OCCURRENCE);
            assert_eq!(occurrence.occurrence_ordinal(), 1);
            let wire = serde_json::to_value(&occurrence).unwrap();
            assert_eq!(wire["component_guid_offset"], 17 + 88);
            assert_eq!(wire["occurrence_guid_offset"], 17 + 164);
            assert_eq!(wire["owner_entity_offset"], 17 + owner_offset);
            assert_eq!(wire["owner_type_guid"], super::OWNER_TYPE_GUID);
            let restored: crate::records::feature::assembly_features::DesignComponentOccurrence =
                serde_json::from_value(wire).unwrap();
            assert_eq!(restored, occurrence);
            if matrix {
                let transform = occurrence.transform().expect("matrix");
                assert_eq!(transform.offset, 17 + 288 + u64::from(separated));
                assert_eq!(transform.value.rows()[0][3], 2.0);
                assert_eq!(transform.value.rows()[1][3], 3.0);
                assert_eq!(transform.value.rows()[2][3], 4.0);
            } else {
                assert!(occurrence.transform().is_none());
            }
        }
    }

    #[test]
    fn separated_typed_matrix_occurrence_passes_native_validation() {
        let frame = typed_fixed(true, true);
        let occurrence = exact_component_occurrence(&frame, 0, "f3d:Design/BulkStream.dat")
            .expect("separated matrix occurrence");
        let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube");
        crate::test_support::native_test::f3d_native_mut(&mut ir)
            .design_component_occurrences
            .push(occurrence);
        let findings = crate::test_support::with_decode_context(|ctx| {
            crate::validate::validate_native(ctx, &ir).expect("native validation")
        });
        assert!(findings.iter().all(|finding| {
            !finding.message.contains("component occurrence")
                && !finding.message.contains("occurrence definition pose")
        }));
    }

    #[test]
    fn typed_component_occurrence_requires_equal_local_references_and_exact_closure() {
        for (separated, matrix, repeat_at, flag_at, owner_at) in [
            (true, false, 237, 288, 298),
            (true, true, 237, 288, 426),
            (false, false, 236, 287, 297),
            (false, true, 236, 287, 425),
        ] {
            let frame = typed_fixed(separated, matrix);
            for changed_at in [
                repeat_at + 1,
                repeat_at + 13,
                flag_at + 1 + usize::from(matrix) * 128,
                owner_at + 13,
                owner_at + 49,
                79,
            ] {
                let mut changed = frame.clone();
                changed[changed_at] ^= 1;
                assert!(
                    exact_component_occurrence(&changed, 0, "f3d:Design/BulkStream.dat").is_none(),
                    "changed {changed_at}"
                );
            }
            let owner_guid = owner_at + 13..owner_at + 49;
            assert_eq!(
                &frame[owner_guid.clone()],
                super::OWNER_TYPE_GUID.as_bytes()
            );
            let mut other_owner = frame.clone();
            other_owner[owner_guid].copy_from_slice(
                crate::records::feature::occurrence_transfer::COMPONENT_TYPE.as_bytes(),
            );
            assert!(
                exact_component_occurrence(&other_owner, 0, "f3d:Design/BulkStream.dat").is_none()
            );
            let mut truncated = frame.clone();
            truncated.remove(truncated.len() - 12);
            assert!(
                exact_component_occurrence(&truncated, 0, "f3d:Design/BulkStream.dat").is_none()
            );
            let mut extended = frame;
            extended.insert(extended.len() - 11, 0);
            assert!(
                exact_component_occurrence(&extended, 0, "f3d:Design/BulkStream.dat").is_none()
            );
        }
        let mut bad_separator = typed_fixed(true, false);
        bad_separator[236] = 1;
        assert!(
            exact_component_occurrence(&bad_separator, 0, "f3d:Design/BulkStream.dat").is_none()
        );
        let mut bad_matrix = typed_fixed(false, true);
        bad_matrix[288..296].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(exact_component_occurrence(&bad_matrix, 0, "f3d:Design/BulkStream.dat").is_none());
    }

    #[test]
    fn attributed_occurrence_accepts_a_typed_prefix_and_closing_owner() {
        let mut bytes = attributed("34", 0, 2);
        let owner_at = bytes.len() - 22;
        let repeat_at = owner_at - 9 - 128 - 1 - 11;
        for (at, kind) in [
            (owner_at + 9, super::OWNER_TYPE_GUID),
            (
                repeat_at + 9,
                crate::records::feature::occurrence_transfer::COMPONENT_TYPE,
            ),
            (
                33,
                crate::records::feature::occurrence_transfer::COMPONENT_TYPE,
            ),
        ] {
            let mut inline = Vec::new();
            push_ascii(&mut inline, kind);
            bytes.splice(at..at, inline);
        }
        let occurrence = attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat")
            .expect("typed attributed occurrence");
        assert_eq!(occurrence.component_record_index, 187);
        assert_eq!(occurrence.owner_entity, 6);
        assert_eq!(occurrence.occurrence_guid.as_str(), "34");
        let wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(
            wire["attributed"]["attributes"].as_array().unwrap().len(),
            2
        );
        assert_eq!(wire["attributed"]["path_discriminator"], 0);
        assert_eq!(wire["attributed"]["key_record_index"], 139);
        assert_eq!(wire["attributed"]["key_segment_index"], 1);
        let matrix_at = usize::try_from(occurrence.transform().unwrap().offset).unwrap();
        let mut unequal = bytes.clone();
        unequal[matrix_at - 51] ^= 1;
        assert!(
            attributed_component_occurrence(&unequal, 0, "f3d:Design/BulkStream.dat").is_none()
        );
        bytes[matrix_at - 1] = 1;
        bytes.drain(matrix_at..matrix_at + 128);
        let identity = attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat")
            .expect("typed attributed identity");
        assert_eq!(identity.occurrence_ordinal(), 2);
        assert!(identity.transform().is_none());
    }

    #[test]
    fn empty_root_path_uses_its_separate_typed_definition_reference() {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"311", 20);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        push_ascii(&mut bytes, "rootInstance");
        bytes.push(1);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        push_utf16(&mut bytes, COMPONENT);
        push_utf16(&mut bytes, OCCURRENCE);
        typed_reference(
            &mut bytes,
            10,
            crate::records::feature::occurrence_transfer::COMPONENT_TYPE,
        );
        bytes.push(1);
        bytes.extend_from_slice(&[0; 10]);
        assert_eq!(bytes.len(), 255);
        assert_eq!(root_instance_component(&bytes, 0, bytes.len()), Some(10));
        let mut components = super::StreamComponents::default();
        components.bind_roots(&bytes, [(0, bytes.len())]);
        assert_eq!(components.root_component, Some(10));
        assert_eq!(components.unbound_root_instances, 0);
        for at in [36, 37, 202, 244, 245] {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            assert_eq!(root_instance_component(&changed, 0, changed.len()), None);
        }
        let mut separated = bytes.clone();
        separated.insert(193, 0);
        assert_eq!(separated.len(), 256);
        assert_eq!(
            root_instance_component(&separated, 0, separated.len()),
            Some(10)
        );
        separated[193] = 1;
        assert_eq!(
            root_instance_component(&separated, 0, separated.len()),
            None
        );
        bytes.push(0);
        assert_eq!(root_instance_component(&bytes, 0, bytes.len()), None);
    }

    #[test]
    fn empty_root_path_does_not_bind_a_definition_and_has_a_loss() {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"311", 20);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        push_ascii(&mut bytes, "rootInstance");
        bytes.push(1);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        typed_reference(&mut bytes, 10, COMPONENT);
        assert_eq!(
            super::root_instance_path_count(&bytes, 0, bytes.len()),
            Some(0)
        );
        assert_eq!(root_instance_component(&bytes, 0, bytes.len()), None);
        assert_eq!(super::root_instance_path_count(&bytes, 0, 35), None);
        let mut components = super::StreamComponents::default();
        let unbound_end = bytes.len();
        indexed_header(&mut bytes, *b"311", 21);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        push_ascii(&mut bytes, "rootInstance");
        bytes.push(1);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&10_u64.to_le_bytes());
        components.bind_roots(&bytes, [(0, unbound_end), (unbound_end, bytes.len())]);
        assert_eq!(components.root_component, Some(10));
        assert_eq!(components.unbound_root_instances, 1);
        let streams = std::collections::BTreeMap::from([("f3d:Design".to_owned(), components)]);
        let loss = super::unbound_roots_loss(&streams).expect("empty root path loss");
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::RootInstanceDefinitionUnbound.kind()
        );
        assert!(!loss.message.contains("their root definitions are unbound"));
        assert!(loss
            .message
            .contains("these records do not bind a root definition"));
        assert!(super::unbound_roots_loss(&std::collections::BTreeMap::default()).is_none());
    }
    #[test]
    fn primary_index_selects_one_occurrence_envelope_and_reports_the_other() {
        let mut bytes = typed_fixed(false, false);
        let second = bytes.len();
        bytes.extend_from_slice(&typed_fixed(false, true));
        let parse = || {
            vec![
                exact_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").unwrap(),
                exact_component_occurrence(&bytes, second, "f3d:Design/BulkStream.dat").unwrap(),
            ]
        };
        for selected in [0, u64::try_from(second).unwrap()] {
            let mut occurrences = parse();
            let count = super::admit_occurrence_envelopes(
                &mut occurrences,
                &std::collections::BTreeSet::from([(20, selected)]),
                &std::collections::BTreeSet::from([(20, selected)]),
            );
            assert_eq!(count, 1);
            assert_eq!(occurrences.len(), 1);
            assert_eq!(occurrences[0].byte_offset(), selected);
            let loss = super::superseded_occurrences_loss(count).unwrap();
            assert_eq!(
                loss.code,
                crate::loss::F3dLossCode::LocalOccurrenceEnvelopeSuperseded.kind()
            );
        }
        let mut occurrences = parse();
        assert_eq!(
            super::admit_occurrence_envelopes(
                &mut occurrences,
                &std::collections::BTreeSet::default(),
                &std::collections::BTreeSet::default()
            ),
            2
        );
        assert!(occurrences.is_empty());
        let mut unique = vec![parse().remove(0)];
        assert_eq!(
            super::admit_occurrence_envelopes(
                &mut unique,
                &std::collections::BTreeSet::from([(20, 17)]),
                &std::collections::BTreeSet::from([(20, 17)])
            ),
            1
        );
        assert!(unique.is_empty());
        assert!(super::superseded_occurrences_loss(0).is_none());
    }

    #[test]
    fn only_typed_occurrences_require_the_placement_type_row() {
        let mut bytes = common(229, 1);
        bytes[208] = 1;
        bytes[218] = 1;
        indexed_header(&mut bytes, *b"269", 21);
        let legacy = exact_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").unwrap();
        let typed =
            exact_component_occurrence(&typed_fixed(false, false), 0, "f3d:Design/BulkStream.dat")
                .unwrap();
        let mut occurrences = vec![legacy, typed];
        let primary = std::collections::BTreeSet::from([(20, 0)]);
        assert_eq!(
            super::admit_occurrence_envelopes(
                &mut occurrences,
                &primary,
                &std::collections::BTreeSet::default(),
            ),
            1
        );
        assert_eq!(occurrences.len(), 1);
        assert!(occurrences[0].owner_type_guid.is_none());
        assert_eq!(
            super::admit_occurrence_envelopes(
                &mut occurrences,
                &std::collections::BTreeSet::default(),
                &primary,
            ),
            1
        );
        assert!(occurrences.is_empty());
    }

    #[test]
    fn fixed_occurrences_retain_path_discriminators_natively() {
        for discriminator in [0_u32, 1, 2, 3, 0x0102_0304] {
            for (typed, matrix) in [(false, false), (false, true), (true, false), (true, true)] {
                let mut bytes = if typed {
                    typed_fixed(false, matrix)
                } else {
                    let mut bytes =
                        common(if matrix { 357 } else { 229 }, if matrix { 2 } else { 1 });
                    if matrix {
                        for (index, value) in crate::design::test_support::identity_matrix()
                            .into_iter()
                            .flatten()
                            .enumerate()
                        {
                            bytes[209 + index * 8..217 + index * 8]
                                .copy_from_slice(&value.to_le_bytes());
                        }
                        bytes[346] = 1;
                    } else {
                        bytes[208] = 1;
                        bytes[218] = 1;
                    }
                    indexed_header(&mut bytes, *b"333", 21);
                    bytes
                };
                let at = if typed { 75 } else { 35 };
                bytes[at..at + 4].copy_from_slice(&discriminator.to_le_bytes());
                let occurrence =
                    exact_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").unwrap();
                assert_eq!(
                    occurrence
                        .path_discriminator
                        .map_or(0, std::num::NonZeroU32::get),
                    discriminator
                );
                let wire = serde_json::to_value(&occurrence).unwrap();
                let restored: crate::records::feature::assembly_features::DesignComponentOccurrence = serde_json::from_value(wire).unwrap();
                assert_eq!(restored, occurrence);
            }
        }
    }

    #[test]
    fn fixed_occurrence_admission_requires_the_registered_primary_frame() {
        use crate::metastream::{MetaStream, RecordIndexEntry};
        use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
        use crate::records::identity::ReferenceRun;
        let mut bytes = typed_fixed(false, false);
        bytes[4..7].copy_from_slice(b"256");
        let occurrence =
            exact_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").unwrap();
        bytes[355..359].copy_from_slice(&20_u32.to_le_bytes());
        let mut meta = MetaStream {
            types: vec![SegmentTypeData {
                byte_offset: 0,
                type_guid: super::OCCURRENCE_PLACEMENT_TYPE_GUID
                    .to_owned()
                    .try_into()
                    .unwrap(),
                type_guid_offset: 0,
                base_type_guid: BaseTypeGuid::Absent,
                version: 0,
                version_offset: 0,
                module: "Component".to_owned(),
                entities: ReferenceRun::unlocated(vec![20]),
            }],
            records: vec![RecordIndexEntry {
                entity_id: 20,
                bulk_offset: 0,
            }],
            secondary_records: vec![RecordIndexEntry {
                entity_id: 20,
                bulk_offset: 348,
            }],
        };
        let frames = super::occurrence_frames(&bytes, &meta).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].member_end, 348);
        meta.types[0].module = "Geometry".to_owned();
        assert!(super::occurrence_frames(&bytes, &meta).unwrap().is_empty());
        meta.types[0].module = "Component".to_owned();
        meta.types[0].entities = ReferenceRun::unlocated(vec![21]);
        assert!(super::occurrence_frames(&bytes, &meta).unwrap().is_empty());
        meta.types[0].entities = ReferenceRun::unlocated(vec![20]);
        meta.records[0].bulk_offset = 17;
        assert!(super::occurrence_frames(&bytes, &meta).is_err());
        bytes[352..355].copy_from_slice(b"256");
        meta.records[0].bulk_offset = 348;
        meta.secondary_records.clear();
        let frames = super::occurrence_frames(&bytes, &meta).unwrap();
        assert_eq!(frames[0].start, 348);
        let primary = frames
            .iter()
            .map(|frame| {
                (
                    frame.entity_id,
                    cadmpeg_core::decode::u64_from_index(frame.start),
                )
            })
            .collect();
        let mut occurrences = vec![occurrence];
        assert_eq!(
            super::admit_occurrence_envelopes(&mut occurrences, &primary, &primary),
            1
        );
        assert!(occurrences.is_empty());
        assert_eq!(
            super::superseded_occurrences_loss(1).unwrap().code,
            crate::loss::F3dLossCode::LocalOccurrenceEnvelopeSuperseded.kind()
        );
    }

    #[test]
    fn typed_class_256_matrix_requires_a_generated_ordinal() {
        let mut bytes = typed_fixed(false, true);
        bytes[4..7].copy_from_slice(b"256");
        assert!(exact_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").is_none());
        bytes[80..84].copy_from_slice(&2_u32.to_le_bytes());
        assert!(exact_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").is_some());
    }
}
