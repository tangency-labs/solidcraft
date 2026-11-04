// SPDX-License-Identifier: Apache-2.0
//! Parse Design segment metadata and the ordered feature timeline.

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::container::{ContainerEntry, ContainerRole};

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{take_reference, Reference};
use crate::container::ContainerScan;
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::records::{
    entity_header::{DesignFeatureTimeline, SegmentType, SegmentTypeData, DESIGN_MODULE_FUSION},
    recipes::DesignComponentNamingSpace,
};

const COMPONENT_MODULE: &str = "Component";
const COMPONENT_NAMING_SPACE_BASE_TYPE_GUID: &str = "21F379C8-CAFD-4985-B461-767673A4C502";
const COMPONENT_UUID_RESERVED_LENGTHS: [usize; 2] = [2, 3];
/// Type of the entity a linked component binding names between the
/// component reference and its context UUID.
const COMPONENT_BINDING_LINK_TYPE_GUID: &str = "8ACBA2E5-5A4C-4438-8351-276DE96E6982";
/// Zero bytes after each reference of a linked component binding.
const COMPONENT_BINDING_LINK_RESERVED_LEN: usize = 2;

/// Stable Design type identity of the record that owns the ordered feature
/// scope list.
pub(crate) const FEATURE_TIMELINE_TYPE_GUID: &str = "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B";
pub(crate) const FEATURE_TIMELINE_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
pub(crate) const FEATURE_TIMELINE_TYPE_VERSIONS: [u32; 2] = [2, 3];

/// Whether a type-table row has the exact registration metadata of a supported
/// feature-timeline frame.
pub(crate) fn is_supported_feature_timeline_type(design_type: &SegmentTypeData) -> bool {
    FEATURE_TIMELINE_TYPE_VERSIONS.contains(&design_type.version)
        && design_type.module == DESIGN_MODULE_FUSION
        && design_type
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            .is_some_and(|base| base.eq_ignore_ascii_case(FEATURE_TIMELINE_BASE_TYPE_GUID))
}

struct MetaStreamEntry<'a> {
    entry: &'a ContainerEntry,
    prefix: &'a str,
}

impl<'a> MetaStreamEntry<'a> {
    fn from_design_entry(scan: &ContainerScan, entry: &'a ContainerEntry) -> Option<Self> {
        if !scan.is_design_stream(entry, ContainerRole::Metastream) {
            return None;
        }
        Some(Self {
            entry,
            prefix: entry.name.strip_suffix("MetaStream.dat")?,
        })
    }
}

fn paired_bulk_entry_name<'a>(
    scan: &'a ContainerScan<'_>,
    prefix: &str,
) -> Result<&'a str, CodecError> {
    if let Some(entry) = scan
        .entries
        .iter()
        .find(|entry| entry.name.strip_prefix(prefix) == Some("BulkStream.dat"))
    {
        return Ok(&entry.name);
    }
    Err(CodecError::malformed(format_args!(
        "entry {prefix}BulkStream.dat not found"
    )))
}

/// Decode Design type tables and the flat-pattern table needed by settings.
pub(crate) fn decode_types(scan: &ContainerScan) -> Result<Vec<SegmentType>, CodecError> {
    let mut out = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        super::sheet_metal_settings::is_settings_stream(scan, entry, ContainerRole::Metastream)
    }) {
        let meta = scan.parsed_metastream(&entry.name)?;
        if !scan.is_design_stream(entry, ContainerRole::Metastream)
            && !super::sheet_metal_settings::has_supported_settings(&meta)
        {
            continue;
        }
        for design_type in &meta.types {
            let id = design_record_id(&entry.name, ":design-type#", design_type.byte_offset);
            out.push(SegmentType::try_new(id, design_type.clone()).map_err(CodecError::Malformed)?);
        }
    }
    Ok(out)
}

/// The header offset and record index of the indexed record holding `at`.
fn enclosing_record(headers: &[(usize, u32)], at: usize) -> Option<(usize, u32)> {
    headers
        .get(
            headers
                .partition_point(|(offset, _)| *offset <= at)
                .checked_sub(1)?,
        )
        .copied()
}

/// One component's context bindings, each with the indexed record holding it.
type ComponentBindings = HashMap<u64, Vec<(DesignComponentNamingSpace, Option<(usize, u32)>)>>;

/// Where a binding starts: its marker offset and the indexed record holding it.
#[derive(Clone, Copy)]
struct BindingSite {
    marker: usize,
    record: Option<(usize, u32)>,
}

fn insert_component_naming_space(
    by_component: &mut ComponentBindings,
    bulk_name: &str,
    site: BindingSite,
    component_record_index: u64,
    context_uuid: crate::records::mesh::DesignRelaxedGuidText,
    context_uuid_offset: usize,
    name: Option<(String, usize)>,
) -> Result<(), CodecError> {
    let BindingSite { marker, record } = site;
    let name_offset = name
        .as_ref()
        .map(|(_, at)| u64::try_from(*at))
        .transpose()
        .map_err(|_| CodecError::malformed("f3d component name offset exceeds u64"))?;
    let byte_offset = u64::try_from(marker)
        .map_err(|_| CodecError::malformed("f3d component naming marker offset exceeds u64"))?;
    let context_uuid_offset = u64::try_from(context_uuid_offset)
        .map_err(|_| CodecError::malformed("f3d component naming UUID offset exceeds u64"))?;
    let bindings = by_component.entry(component_record_index).or_default();
    if let Some((existing, existing_record)) = bindings
        .iter_mut()
        .find(|(existing, _)| existing.context_uuid == context_uuid)
    {
        // A named hit is the binding in the definition record, so its site
        // replaces the unnamed hit's.
        if existing.name.is_none() && name.is_some() {
            existing.name = name.map(|(name, _)| name);
            existing.name_offset = name_offset;
            existing.byte_offset = byte_offset;
            existing.context_uuid_offset = context_uuid_offset;
            existing.id =
                design_record_id(bulk_name, ":design-component-naming-space#", byte_offset);
            *existing_record = record;
        }
        return Ok(());
    }
    // A component definition stored as revisions, in different records that
    // repeat one record index and each carry the component's name, binds one
    // context UUID in each. Any other second UUID conflicts.
    let new_name = name.as_ref().map(|(name, _)| name);
    let revision = |binding: &(DesignComponentNamingSpace, Option<(usize, u32)>)| {
        binding.0.name.as_ref() == new_name
            && match (binding.1, record) {
                (Some((offset, index)), Some((new_offset, new_index))) => {
                    index == new_index && offset != new_offset
                }
                _ => false,
            }
    };
    if !bindings.is_empty() && (name.is_none() || !bindings.iter().all(revision)) {
        return Err(crate::design::text::malformed_design(format_args!(
            "Design component {component_record_index} has conflicting context UUID bindings"
        )));
    }

    let id = design_record_id(bulk_name, ":design-component-naming-space#", byte_offset);
    bindings.push((
        DesignComponentNamingSpace {
            id,
            byte_offset,
            component_record_index,
            context_uuid,
            context_uuid_offset,
            name: name.map(|(name, _)| name),
            name_offset,
        },
        record,
    ));
    Ok(())
}

/// The component name after a binding's context UUID: an LP-UTF16 name and
/// two zero bytes that end the indexed record. Other binding sites store no
/// name.
fn component_name_after_context_uuid(bytes: &[u8], uuid_offset: usize) -> Option<(String, usize)> {
    let name_at = uuid_offset.checked_add(4 + 2 * 36)?;
    let (name, end) = lp_utf16_bounded(bytes, name_at, 1..=256)?;
    if name.chars().any(char::is_control) || bytes.get(end..end.checked_add(2)?)? != [0, 0] {
        return None;
    }
    let after = end.checked_add(2)?;
    let record_ends = after == bytes.len()
        || (View::u32_le_at(bytes, after) == Some(3)
            && bytes
                .get(after.checked_add(4)?..after.checked_add(7)?)
                .is_some_and(|tag| tag.iter().all(u8::is_ascii_digit)));
    record_ends.then_some((name, name_at))
}

/// The counted context UUID after a typed component reference and its offset.
///
/// The reference's two-byte local tail is the shorter of the reserved fields an
/// untyped binding carries ([`COMPONENT_UUID_RESERVED_LENGTHS`]); the longer
/// one adds a zero byte before the UUID.
fn typed_component_context_uuid(bytes: &[u8], after_reference: usize) -> Option<(String, usize)> {
    [after_reference, after_reference.checked_add(1)?]
        .into_iter()
        .filter(|&at| {
            bytes
                .get(after_reference..at)
                .is_some_and(|reserved| reserved.iter().all(|byte| *byte == 0))
        })
        .find_map(|at| Some((lp_utf16_bounded(bytes, at, 36..=36)?.0, at)))
}

/// The entities a linked component binding can name between its component
/// reference and its context UUID: an entity registered to the link type, or
/// an entity the Design stream registers to no type and indexes no record for.
struct LinkEntities {
    linked: HashSet<u64>,
    registered: HashSet<u64>,
}

impl LinkEntities {
    fn admits(&self, entity: u64) -> bool {
        self.linked.contains(&entity) || !self.registered.contains(&entity)
    }
}

/// The component, context UUID, and UUID offset of a linked binding at
/// `marker`: a component reference and a reference to a link entity, each
/// followed by its reserved zero bytes, then the context UUID.
fn linked_component_binding(
    bytes: &[u8],
    marker: usize,
    component_entities: &HashSet<u64>,
    link_entities: &LinkEntities,
) -> Option<(u64, crate::records::mesh::DesignRelaxedGuidText, usize)> {
    let reference_len = 1 + 8 + COMPONENT_BINDING_LINK_RESERVED_LEN;
    let reference = |at: usize, target: &dyn Fn(u64) -> bool| {
        let entity = View::u64_le_at(bytes, at.checked_add(1)?)?;
        let reserved = bytes.get(at.checked_add(9)?..at.checked_add(reference_len)?)?;
        (bytes.get(at) == Some(&1) && target(entity) && reserved.iter().all(|byte| *byte == 0))
            .then_some(entity)
    };
    if marker > 0 && bytes.get(marker - 1) == Some(&1) {
        return None;
    }
    let component_record_index = reference(marker, &|entity| component_entities.contains(&entity))?;
    let link = marker.checked_add(reference_len)?;
    reference(link, &|entity| link_entities.admits(entity))?;
    let uuid_offset = link.checked_add(reference_len)?;
    let (context_uuid, _) = lp_utf16_bounded(bytes, uuid_offset, 36..=36)?;
    let context_uuid = crate::records::mesh::DesignRelaxedGuidText::try_from(context_uuid).ok()?;
    Some((component_record_index, context_uuid, uuid_offset))
}

/// A run of records after a Design `BulkStream`'s last indexed record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnindexedRecordRun {
    /// Offset of the run's first indexed header, where the indexed extent ends.
    pub(crate) start: usize,
    /// Record index of the run's first header.
    pub(crate) first_record_index: u32,
    /// Indexed headers in the run.
    pub(crate) headers: usize,
}

/// The unindexed suffix whose first header has a lower record index than
/// the header at the greatest offset in either `MetaStream` index.
pub(crate) fn unindexed_record_run(
    bulk: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Option<UnindexedRecordRun> {
    let last_indexed = usize::try_from(
        meta.records
            .iter()
            .chain(&meta.secondary_records)
            .map(|record| record.bulk_offset)
            .max()?,
    )
    .ok()?;
    let preceding = super::sketch::indexed_record_header_at(bulk, last_indexed)?;
    let tail_start = last_indexed.checked_add(1)?;
    let mut headers = super::sketch::indexed_record_offsets(bulk.get(tail_start..)?);
    let first = headers.next()?;
    (first.record_index < preceding.record_index).then(|| UnindexedRecordRun {
        start: tail_start + first.offset,
        first_record_index: first.record_index,
        headers: 1 + headers.count(),
    })
}

/// Component naming spaces decoded from one document, with the count of
/// registered component entities that bind no context UUID in this document.
/// A component registered locally whose binding lives in another document
/// stays unbound; its occurrences stay unplaced downstream.
#[derive(Debug)]
pub(crate) struct DecodedComponentNamingSpaces {
    pub(crate) spaces: Vec<DesignComponentNamingSpace>,
    pub(crate) unbound: usize,
}

/// Decode each component entity's UUID-bound local naming space.
pub(crate) fn decode_component_naming_spaces(
    scan: &ContainerScan,
) -> Result<DecodedComponentNamingSpaces, CodecError> {
    let mut out = Vec::new();
    let mut unbound = 0;
    for meta_entry in scan
        .entries
        .iter()
        .filter_map(|entry| MetaStreamEntry::from_design_entry(scan, entry))
    {
        let meta = scan.parsed_metastream(&meta_entry.entry.name)?;
        let mut component_entities = HashSet::new();
        for design_type in meta.types.iter().filter(|design_type| {
            design_type.module == COMPONENT_MODULE
                && design_type
                    .base_type_guid
                    .value()
                    .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                    .is_some_and(|base| {
                        base.eq_ignore_ascii_case(COMPONENT_NAMING_SPACE_BASE_TYPE_GUID)
                    })
        }) {
            for &entity_id in design_type.entities.values() {
                component_entities.insert(entity_id);
            }
        }
        if component_entities.is_empty() {
            continue;
        }
        let bulk_name = paired_bulk_entry_name(scan, meta_entry.prefix)?;
        let bytes = scan.entry_bytes(bulk_name)?;
        let headers = super::sketch::indexed_record_offsets(bytes)
            .map(|header| (header.offset, header.record_index))
            .collect::<Vec<_>>();
        let mut by_component = ComponentBindings::new();
        for reserved_len in COMPONENT_UUID_RESERVED_LENGTHS {
            let prefix_len = 1 + 8 + reserved_len;
            for uuid_offset in bytes
                .len()
                .checked_sub(4)
                .into_iter()
                .flat_map(|last| prefix_len..last)
            {
                let marker = uuid_offset - prefix_len;
                if bytes[marker] != 1
                    || (marker > 0 && bytes[marker - 1] == 1)
                    || !bytes[marker + 9..uuid_offset].iter().all(|byte| *byte == 0)
                {
                    continue;
                }
                let Some(component_record_index) = View::u64_le_at(bytes, marker + 1) else {
                    continue;
                };
                if !component_entities.contains(&component_record_index) {
                    continue;
                }
                let Some((context_uuid, _)) = lp_utf16_bounded(bytes, uuid_offset, 36..=36) else {
                    continue;
                };
                let Ok(context_uuid) =
                    crate::records::mesh::DesignRelaxedGuidText::try_from(context_uuid)
                else {
                    continue;
                };
                insert_component_naming_space(
                    &mut by_component,
                    bulk_name,
                    BindingSite {
                        marker,
                        record: enclosing_record(&headers, marker),
                    },
                    component_record_index,
                    context_uuid,
                    uuid_offset,
                    component_name_after_context_uuid(bytes, uuid_offset),
                )?;
            }
        }
        let link_entities = LinkEntities {
            linked: meta
                .types
                .iter()
                .filter(|design_type| {
                    design_type.module == DESIGN_MODULE_FUSION
                        && design_type
                            .type_guid
                            .as_str()
                            .eq_ignore_ascii_case(COMPONENT_BINDING_LINK_TYPE_GUID)
                })
                .flat_map(|design_type| design_type.entities.values().copied())
                .collect(),
            registered: meta
                .types
                .iter()
                .flat_map(|design_type| design_type.entities.values().copied())
                .chain(
                    meta.records
                        .iter()
                        .chain(&meta.secondary_records)
                        .map(|record| record.entity_id),
                )
                .collect(),
        };
        for marker in 0..bytes.len() {
            let Some((component_record_index, context_uuid, uuid_offset)) =
                linked_component_binding(bytes, marker, &component_entities, &link_entities)
            else {
                continue;
            };
            insert_component_naming_space(
                &mut by_component,
                bulk_name,
                BindingSite {
                    marker,
                    record: enclosing_record(&headers, marker),
                },
                component_record_index,
                context_uuid,
                uuid_offset,
                component_name_after_context_uuid(bytes, uuid_offset),
            )?;
        }
        for marker in 0..bytes.len() {
            let mut uuid_offset = marker;
            let Some(reference) = take_reference(bytes, &mut uuid_offset) else {
                continue;
            };
            let Some((component_record_index, Some(inline_type_guid))) = reference.local() else {
                continue;
            };
            if !meta.types.iter().any(|design_type| {
                design_type.module == COMPONENT_MODULE
                    && design_type
                        .base_type_guid
                        .value()
                        .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                        .is_some_and(|base| {
                            base.eq_ignore_ascii_case(COMPONENT_NAMING_SPACE_BASE_TYPE_GUID)
                        })
                    && design_type
                        .type_guid
                        .as_str()
                        .eq_ignore_ascii_case(inline_type_guid)
                    && design_type
                        .entities
                        .values()
                        .any(|registered| *registered == component_record_index)
            }) {
                continue;
            }
            let Some((context_uuid, uuid_offset)) =
                typed_component_context_uuid(bytes, uuid_offset)
            else {
                continue;
            };
            let Ok(context_uuid) =
                crate::records::mesh::DesignRelaxedGuidText::try_from(context_uuid)
            else {
                continue;
            };
            insert_component_naming_space(
                &mut by_component,
                bulk_name,
                BindingSite {
                    marker,
                    record: enclosing_record(&headers, marker),
                },
                component_record_index,
                context_uuid,
                uuid_offset,
                component_name_after_context_uuid(bytes, uuid_offset),
            )?;
        }
        unbound += component_entities
            .iter()
            .filter(|entity| !by_component.contains_key(entity))
            .count();
        out.extend(by_component.into_values().flatten().map(|(space, _)| space));
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(DecodedComponentNamingSpaces {
        spaces: out,
        unbound,
    })
}

/// Parse the `MetaStream` paired with one Design `BulkStream`.
pub(crate) fn metadata_for_bulk_stream(
    scan: &ContainerScan,
    bulk_entry_name: &str,
) -> Result<Option<std::sync::Arc<crate::metastream::MetaStream>>, CodecError> {
    let prefix = bulk_entry_name
        .strip_suffix("BulkStream.dat")
        .ok_or_else(|| CodecError::Malformed("Design stream has no BulkStream suffix".into()))?;
    let Some(meta_entry) = scan
        .entries
        .iter()
        .find(|entry| entry.name.strip_prefix(prefix) == Some("MetaStream.dat"))
    else {
        return Ok(None);
    };
    scan.parsed_metastream(&meta_entry.name).map(Some)
}

/// One live Design record selected by the primary index and resolved through
/// its segment-local class tag.
#[derive(Clone)]
pub(in crate::design::decode) struct DesignPrimaryFrame<'a> {
    pub(super) entity_id: u64,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(super) start: usize,
    /// End of the primary class-member sequence: the secondary nested header,
    /// or `end` when the record has none.
    pub(super) member_end: usize,
    pub(super) end: usize,
    pub(super) design_type: &'a SegmentTypeData,
}

pub(super) fn dynamic_type<'a>(
    meta: &'a crate::metastream::MetaStream,
    class_tag: &crate::records::references::DesignClassTag,
) -> Option<(usize, &'a SegmentTypeData)> {
    let ordinal = class_tag.as_str().parse::<usize>().ok()?.checked_sub(256)?;
    Some((ordinal, meta.types.get(ordinal)?))
}

pub(super) fn record_header_class_tag(
    bytes: &[u8],
    at: usize,
    end: usize,
    expected_entity_id: u64,
) -> Option<crate::records::references::DesignClassTag> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(bytes, at, 3..=3, u8::is_ascii_digit)?;
    let indexed_matches = after_tag
        .checked_add(4)
        .filter(|entity_end| *entity_end <= end)
        .and_then(|_| View::u32_le_at(bytes, after_tag))
        .is_some_and(|entity_id| u64::from(entity_id) == expected_entity_id);
    let named_matches = after_tag
        .checked_add(8)
        .filter(|entity_end| *entity_end <= end)
        .and_then(|_| View::u64_le_at(bytes, after_tag))
        == Some(expected_entity_id);
    if !indexed_matches && !named_matches {
        return None;
    }
    crate::design::decode::text::class_tag_from_view(class_tag).ok()
}

/// Resolve every live sibling record from the primary index. The primary
/// header must repeat the indexed entity ID and select a type-table row that
/// registers that entity. A secondary entry supplies the exact end of the
/// primary class-member sequence and must point to a nested header for the same
/// entity.
pub(super) fn design_primary_frames<'a>(
    bytes: &[u8],
    meta: &'a crate::metastream::MetaStream,
) -> Result<Vec<DesignPrimaryFrame<'a>>, CodecError> {
    let indexed = crate::metastream::primary_record_frames(meta, bytes.len())?;
    let mut registered_entities = HashSet::new();
    for (ordinal, design_type) in meta.types.iter().enumerate() {
        for &entity_id in design_type.entities.values() {
            registered_entities.insert((ordinal, entity_id));
        }
    }
    let mut frames = Vec::new();
    for frame in indexed {
        let entity_id = frame.entity_id;
        let Some(class_tag) = record_header_class_tag(bytes, frame.start, frame.end, entity_id)
        else {
            return Err(CodecError::Malformed(
                "F3D primary record index points to an invalid record header".into(),
            ));
        };
        let Some((type_ordinal, design_type)) = dynamic_type(meta, &class_tag) else {
            return Err(CodecError::Malformed(
                "F3D primary record class tag is outside its type table".into(),
            ));
        };
        if !registered_entities.contains(&(type_ordinal, entity_id)) {
            return Err(CodecError::Malformed(
                "F3D primary record type does not register its indexed entity ID".into(),
            ));
        }
        if frame.member_end < frame.end {
            let Some(nested_class_tag) =
                record_header_class_tag(bytes, frame.member_end, frame.end, entity_id)
            else {
                return Err(CodecError::Malformed(
                    "F3D secondary record index points to an invalid nested header".into(),
                ));
            };
            if dynamic_type(meta, &nested_class_tag).is_none() {
                return Err(CodecError::Malformed(
                    "F3D secondary record header is incompatible with its primary record".into(),
                ));
            }
        }
        frames.push(DesignPrimaryFrame {
            entity_id,
            class_tag,
            start: frame.start,
            member_end: frame.member_end,
            end: frame.end,
            design_type,
        });
    }
    Ok(frames)
}

/// One primary `BulkStream` frame selected through a registered Design type.
#[derive(Clone, Copy)]
pub(super) struct TypedPrimaryFrame<'a> {
    pub(super) entity_id: u64,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) design_type: &'a SegmentTypeData,
}

/// Resolve every entity registered to `type_guid` through the sibling
/// `MetaStream` primary index and verify its dynamic class tag.
pub(super) fn typed_primary_frames<'a>(
    bytes: &[u8],
    meta: &'a crate::metastream::MetaStream,
    type_guid: &str,
    record_kind: &str,
) -> Result<Vec<TypedPrimaryFrame<'a>>, CodecError> {
    let mut typed_entities = HashSet::new();
    for design_type in &meta.types {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(type_guid)
        {
            continue;
        }
        for &entity_id in design_type.entities.values() {
            if typed_entities.contains(&entity_id) {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design {record_kind} entity {entity_id} is registered more than once"
                )));
            }

            typed_entities.insert(entity_id);
        }
    }

    let mut resolved_entities = HashSet::new();
    let mut frames = Vec::new();
    for primary_frame in design_primary_frames(bytes, meta)? {
        if !primary_frame
            .design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(type_guid)
        {
            continue;
        }
        if !resolved_entities.contains(&primary_frame.entity_id) {
            resolved_entities.insert(primary_frame.entity_id);
        }

        frames.push(TypedPrimaryFrame {
            entity_id: primary_frame.entity_id,
            start: primary_frame.start,
            end: primary_frame.end,
            design_type: primary_frame.design_type,
        });
    }
    if let Some(entity_id) = typed_entities.difference(&resolved_entities).min() {
        return Err(crate::design::text::malformed_design(format_args!(
            "F3D Design {record_kind} entity {entity_id} has no primary record of its registered class"
        )));
    }
    Ok(frames)
}

/// Type GUID and record version keyed by the Design entity ids that carry the
/// type in the sibling `BulkStream`.
pub(crate) fn stream_types_by_entity<'a>(
    types: &'a [SegmentType],
    bulk_entry_name: &str,
) -> HashMap<u64, (&'a str, u32)> {
    let mut by_entity = HashMap::new();
    for design_type in types.iter().filter(|design_type| {
        native_stream(design_type.id())
            .is_some_and(|scope| meta_scope_matches_bulk(scope, bulk_entry_name))
    }) {
        for &entity_id in design_type.entities.values() {
            by_entity.insert(
                entity_id,
                (design_type.type_guid.as_str(), design_type.version),
            );
        }
    }
    by_entity
}

/// Complete type-table row keyed by the segment-local dynamic class tag.
pub(super) fn stream_types_by_class_tag<'a>(
    types: &'a [SegmentType],
    bulk_entry_name: &str,
) -> HashMap<u32, &'a SegmentType> {
    let mut by_class_tag = HashMap::new();
    for (ordinal, design_type) in types
        .iter()
        .filter(|design_type| {
            native_stream(design_type.id())
                .is_some_and(|scope| meta_scope_matches_bulk(scope, bulk_entry_name))
        })
        .enumerate()
    {
        let Some(class_tag) = u32::try_from(ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(256))
        else {
            continue;
        };

        by_class_tag.insert(class_tag, design_type);
    }
    by_class_tag
}

/// Compare an encoded native `MetaStream` scope with the `BulkStream`'s sibling
/// name without materializing either name.
fn meta_scope_matches_bulk(scope: &str, bulk_entry_name: &str) -> bool {
    let Some(prefix) = bulk_entry_name.strip_suffix("BulkStream.dat") else {
        return false;
    };
    let Some(encoded) = scope
        .strip_prefix("f3d:")
        .and_then(|scope| scope.strip_suffix("MetaStream.dat"))
    else {
        return false;
    };
    let mut observed = encoded.bytes();
    for character in prefix.chars() {
        let mut buffer = [0; 4];
        let bytes = character.encode_utf8(&mut buffer).as_bytes();
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for byte in bytes {
                if observed.next() != Some(b'%')
                    || observed.next() != Some(HEX[usize::from(byte >> 4)])
                    || observed.next() != Some(HEX[usize::from(byte & 0x0f)])
                {
                    return false;
                }
            }
        } else if !bytes.iter().all(|byte| observed.next() == Some(*byte)) {
            return false;
        }
    }
    observed.next().is_none()
}

fn local_reference(
    reference: &Reference<&str, crate::bytes::utf16::Utf16View<'_>>,
    type_guids_by_entity: &HashMap<u64, Vec<&str>>,
) -> Option<u64> {
    let (target, inline_type_guid) = reference.local()?;
    if let Some(inline_type_guid) = inline_type_guid {
        let registered_type_guids = type_guids_by_entity.get(&target)?;
        if !registered_type_guids
            .iter()
            .any(|registered| registered.eq_ignore_ascii_case(inline_type_guid))
        {
            return None;
        }
    }
    Some(target)
}

fn parse_feature_timeline_record(
    bytes: &[u8],
    stream: &str,
    frame: std::ops::Range<usize>,
    expected: (&str, u64),
    source_ordinal: u32,
    type_guids_by_entity: &HashMap<u64, Vec<&str>>,
) -> Option<DesignFeatureTimeline> {
    let (expected_class_tag, expected_entity_id) = expected;
    let (
        class_tag,
        mut at,
        context_reference_offset,
        context_record_index,
        item_count_offset,
        count,
    ) = (|| {
        let (start, end) = (frame.start, frame.end);
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if class_tag != expected_class_tag
            || View::u64_le_at(bytes, after_tag)? != expected_entity_id
        {
            return None;
        }
        let (_, payload) = lp_ascii_filtered_view(
            bytes,
            after_tag.checked_add(8)?,
            0..=2000,
            u8::is_ascii_graphic,
        )?;
        if bytes.get(payload..payload.checked_add(2)?)? != [0, 0] {
            return None;
        }
        let mut at = payload.checked_add(2)?;
        let context_reference_offset = at.checked_add(1)?;
        let context_record_index = std::num::NonZeroU64::new(local_reference(
            &take_reference(bytes, &mut at)?,
            type_guids_by_entity,
        )?)?;
        let item_count_offset = at;
        let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        at = at.checked_add(4)?;
        if count > end.checked_sub(at)? {
            return None;
        }
        Some((
            class_tag,
            at,
            context_reference_offset,
            context_record_index,
            item_count_offset,
            count,
        ))
    })()?;

    let mut items = Vec::new();
    let mut null_items = 0_usize;
    for _ in 0..count {
        let target_offset = at.checked_add(1)?;
        let reference = take_reference(bytes, &mut at)?;
        if matches!(reference, Reference::Null) {
            null_items += 1;
            continue;
        }
        if null_items > 0 {
            return None;
        }
        let target = local_reference(&reference, type_guids_by_entity)?;
        let target_offset = u64::try_from(target_offset).ok()?;
        items.push(crate::records::identity::Located {
            value: target,
            offset: target_offset,
        });
    }
    if at != frame.end {
        return None;
    }

    let source_start = frame.start;
    let frame_start = u64::try_from(source_start).ok()?;
    let frame_length = frame
        .end
        .checked_sub(frame.start)
        .and_then(|value| u64::try_from(value).ok())?;
    let context_reference_offset = u64::try_from(context_reference_offset).ok()?;
    let item_count_offset = u64::try_from(item_count_offset).ok()?;
    let Ok(frame) = crate::records::entity_header::DesignTimelineFrame::new(
        frame_start,
        frame_length,
        context_reference_offset,
        item_count_offset,
        items,
    ) else {
        return None;
    };
    let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
    let record_index = std::num::NonZeroU64::new(expected_entity_id)?;
    let id = design_record_id(stream, ":design-feature-timeline#", frame_start);
    DesignFeatureTimeline::try_new(
        id,
        frame,
        class_tag,
        record_index,
        source_ordinal,
        context_record_index,
    )
    .ok()
}

/// Decode the exact counted scope list that carries authored feature order.
pub(crate) fn decode_feature_timelines(
    scan: &ContainerScan,
) -> Result<Vec<DesignFeatureTimeline>, CodecError> {
    let mut out = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        super::sheet_metal_settings::is_settings_stream(scan, entry, ContainerRole::Metastream)
    }) {
        let meta = crate::metastream::parse(scan.entry_bytes(&entry.name)?, &entry.name)?;
        if !scan.is_design_stream(entry, ContainerRole::Metastream)
            && !super::sheet_metal_settings::has_supported_settings(&meta)
        {
            continue;
        }
        let Some(prefix) = entry.name.strip_suffix("MetaStream.dat") else {
            continue;
        };
        let is_timeline_type = |design_type: &SegmentTypeData| {
            design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(FEATURE_TIMELINE_TYPE_GUID)
        };
        if !meta.types.iter().any(is_timeline_type) {
            continue;
        }
        if meta
            .types
            .iter()
            .filter(|design_type| is_timeline_type(design_type))
            .any(|design_type| !FEATURE_TIMELINE_TYPE_VERSIONS.contains(&design_type.version))
        {
            return Err(CodecError::NotImplemented(
                "unsupported Design feature-timeline record version".into(),
            ));
        }
        if meta
            .types
            .iter()
            .filter(|design_type| is_timeline_type(design_type))
            .any(|design_type| !is_supported_feature_timeline_type(design_type))
        {
            return Err(CodecError::Malformed(
                "Design feature-timeline type has incompatible registration metadata".into(),
            ));
        }
        if meta
            .records
            .windows(2)
            .any(|pair| pair[0].bulk_offset >= pair[1].bulk_offset)
        {
            return Err(CodecError::Malformed(
                "Design MetaStream record offsets are not strictly increasing".into(),
            ));
        }
        let bulk_name = paired_bulk_entry_name(scan, prefix)?;
        let bytes = scan.entry_bytes(bulk_name)?;
        let mut type_guids_by_entity = HashMap::<u64, Vec<&str>>::new();
        for design_type in &meta.types {
            for entity_id in design_type.entities.values() {
                type_guids_by_entity
                    .entry(*entity_id)
                    .or_default()
                    .push(design_type.type_guid.as_str());
            }
        }
        let mut source_ordinal = 0_u32;
        for (type_ordinal, design_type) in meta
            .types
            .iter()
            .enumerate()
            .filter(|(_, design_type)| is_timeline_type(design_type))
        {
            let expected_class_tag = u32::try_from(type_ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_add(256))
                .filter(|class_tag| *class_tag <= 999)
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Design feature-timeline class tag is not three digits".into(),
                    )
                })?
                .to_string();
            for entity_id in design_type.entities.values() {
                let entity_source_ordinal = source_ordinal;
                source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                    CodecError::Malformed("Design feature-timeline ordinal exceeds u32".into())
                })?;
                let mut matches = meta
                    .records
                    .iter()
                    .enumerate()
                    .filter(|(_, record)| record.entity_id == *entity_id);
                let Some((record_ordinal, record)) = matches.next() else {
                    return Err(CodecError::Malformed(
                        "Design feature timeline has no unique primary record-index entry".into(),
                    ));
                };
                if matches.next().is_some() {
                    return Err(CodecError::Malformed(
                        "Design feature timeline has no unique primary record-index entry".into(),
                    ));
                }
                let start = usize::try_from(record.bulk_offset).map_err(|_| {
                    CodecError::Malformed("Design feature-timeline offset exceeds usize".into())
                })?;
                let end = meta.records.get(record_ordinal + 1).map_or_else(
                    || Ok(bytes.len()),
                    |next| {
                        usize::try_from(next.bulk_offset).map_err(|_| {
                            CodecError::Malformed(
                                "Design feature-timeline end exceeds usize".into(),
                            )
                        })
                    },
                )?;
                if start >= end || end > bytes.len() {
                    return Err(CodecError::Malformed(
                        "Design feature-timeline record extent is outside its BulkStream".into(),
                    ));
                }
                let timeline = parse_feature_timeline_record(
                    bytes,
                    bulk_name,
                    start..end,
                    (&expected_class_tag, *entity_id),
                    entity_source_ordinal,
                    &type_guids_by_entity,
                )
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Design feature-timeline record does not match its exact frame".into(),
                    )
                })?;

                out.push(timeline);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
