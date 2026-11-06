// SPDX-License-Identifier: Apache-2.0
//! The record header every design entity carries: module and base-type tags, segment types and the feature timeline.

#[cfg(test)]
use cadmpeg_core::decode::u64_from_index;

use super::identity::{DesignEntityId, Located, NativeRecordId, ReferenceRun};
use super::mesh::DesignRelaxedGuidText;
use super::references::DesignClassTag;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_base_type_guid, String, "base_type_guid");
cadmpeg_core::named_optional_field!(
    deserialize_base_type_guid_offset,
    u64,
    "base_type_guid_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_declared_reference_count,
    usize,
    "declared_reference_count"
);
cadmpeg_core::named_optional_field!(deserialize_module, String, "module");
cadmpeg_core::named_optional_field!(deserialize_record_reference, u32, "record_reference");
cadmpeg_core::named_optional_field!(
    deserialize_record_reference_offset,
    u64,
    "record_reference_offset"
);
/// Add-in module that registers the Design sketch types.
pub(crate) const DESIGN_MODULE_SKETCH: &str = "MSketch";

/// Add-in module that registers the Design body types.
pub(crate) const DESIGN_MODULE_BODY: &str = "Body";

/// Add-in module that registers the Design geometry types.
pub(crate) const DESIGN_MODULE_GEOMETRY: &str = "Geometry";

/// Add-in module that registers the Design component types.
pub(crate) const DESIGN_MODULE_COMPONENT: &str = "Component";

/// Add-in module that registers the root Fusion document types.
pub(crate) const DESIGN_MODULE_FUSION: &str = "Fusion";

/// The base-type-GUID field of a type-table entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BaseTypeGuid {
    /// The entry stores no base-type-GUID field.
    Absent,
    /// The entry stores an explicit empty root GUID at this location.
    EmptyRoot {
        /// Byte offset of the empty base-GUID bytes in the `MetaStream`.
        offset: u64,
    },
    /// The entry names a base type at this location.
    Guid {
        /// GUID naming the base type.
        value: DesignRelaxedGuidText,
        /// Byte offset of the base-GUID bytes in the `MetaStream`.
        offset: u64,
    },
}

impl BaseTypeGuid {
    /// The named base type, when the entry names one.
    pub(crate) fn value(&self) -> Option<&DesignRelaxedGuidText> {
        match self {
            Self::Absent | Self::EmptyRoot { .. } => None,
            Self::Guid { value, .. } => Some(value),
        }
    }

    /// Byte offset of the stored base-GUID bytes, when the entry stores them.
    pub(crate) fn offset(&self) -> Option<u64> {
        match *self {
            Self::Absent => None,
            Self::EmptyRoot { offset } | Self::Guid { offset, .. } => Some(offset),
        }
    }

    fn from_wire(value: Option<String>, offset: Option<u64>) -> Result<Self, String> {
        match (value, offset) {
            (None, None) => Ok(Self::Absent),
            (Some(value), Some(offset)) if value.is_empty() => Ok(Self::EmptyRoot { offset }),
            (Some(value), Some(offset)) => Ok(Self::Guid {
                value: DesignRelaxedGuidText::try_from(value)
                    .map_err(|error| format!("base_type_guid: {error}"))?,
                offset,
            }),
            (Some(_), None) => Err("base_type_guid requires base_type_guid_offset".into()),
            (None, Some(_)) => Err("base_type_guid_offset requires base_type_guid".into()),
        }
    }

    #[cfg(test)]
    fn into_wire(self) -> (Option<String>, Option<u64>) {
        match self {
            Self::Absent => (None, None),
            Self::EmptyRoot { offset } => (Some(String::new()), Some(offset)),
            Self::Guid { value, offset } => (Some(value.into()), Some(offset)),
        }
    }
}

/// One framed type registration before it receives an archive-scoped identity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SegmentTypeData {
    /// Byte offset of this type-table entry in its `MetaStream`.
    pub(crate) byte_offset: u64,
    /// GUID naming this entry's record type. Class tags are segment-local, so
    /// this GUID is the only discriminator that is stable across files.
    pub(crate) type_guid: DesignRelaxedGuidText,
    /// Byte offset of the type-GUID bytes in the `MetaStream`.
    pub(crate) type_guid_offset: u64,
    /// Base-type-GUID field and location.
    pub(crate) base_type_guid: BaseTypeGuid,
    /// Record version of this type.
    pub(crate) version: u32,
    /// Byte offset of `version` in the Design `MetaStream`.
    pub(crate) version_offset: u64,
    /// Add-in module that registers this type, e.g. `Fusion`, `MSketch`, or
    /// `Body`. Every type a module registers repeats the module name, so it
    /// classifies a type but does not identify one. Some types record no module.
    pub(crate) module: String,
    /// Entity ids whose records carry this type, in source `MetaStream` order;
    /// a count rather than a fixed-arity list, so length varies per entry.
    pub(crate) entities: ReferenceRun<u64>,
}

/// An identified type registration whose native key binds its `MetaStream` offset.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "SegmentTypeWire")]
pub(crate) struct SegmentType {
    id: NativeRecordId,
    data: SegmentTypeData,
}

impl SegmentType {
    pub(crate) fn try_new(id: String, data: SegmentTypeData) -> Result<Self, String> {
        let id = NativeRecordId::try_f3d_new(id, "design-type", data.byte_offset)?;
        if !id.stream().ends_with("/MetaStream.dat") {
            return Err("type id must name its containing MetaStream".into());
        }
        Ok(Self { id, data })
    }
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    #[cfg(test)]
    pub(crate) fn set_module(&mut self, module: String) {
        self.data.module = module;
    }
    #[cfg(test)]
    pub(crate) fn set_entities(&mut self, entities: ReferenceRun<u64>) {
        self.data.entities = entities;
    }
    #[cfg(test)]
    pub(crate) fn set_base_type_guid(&mut self, base: BaseTypeGuid) {
        self.data.base_type_guid = base;
    }
    #[cfg(test)]
    pub(crate) fn set_version(&mut self, version: u32) {
        self.data.version = version;
    }
    #[cfg(test)]
    pub(crate) fn set_type_guid(&mut self, guid: DesignRelaxedGuidText) {
        self.data.type_guid = guid;
    }
}

impl std::ops::Deref for SegmentType {
    type Target = SegmentTypeData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

#[cfg(test)]
thread_local! {
    static SEGMENT_TYPE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for SegmentType {
    fn clone(&self) -> Self {
        SEGMENT_TYPE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            data: self.data.clone(),
        }
    }
}

struct SegmentValues<'a>(&'a ReferenceRun<u64>);

impl Serialize for SegmentValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.values())
    }
}

struct SegmentOffsets<'a>(&'a ReferenceRun<u64>);

impl Serialize for SegmentOffsets<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.offsets())
    }
}

#[derive(Serialize)]
struct SegmentTypeWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    type_guid: &'a DesignRelaxedGuidText,
    type_guid_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_type_guid: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_type_guid_offset: Option<u64>,
    version: u32,
    version_offset: u64,
    module: &'a str,
    entity_ids: SegmentValues<'a>,
    entity_id_offsets: SegmentOffsets<'a>,
}

impl Serialize for SegmentType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let base_type_guid = match &self.base_type_guid {
            BaseTypeGuid::Absent => None,
            BaseTypeGuid::EmptyRoot { .. } => Some(""),
            BaseTypeGuid::Guid { value, .. } => Some(value.as_str()),
        };
        SegmentTypeWireRef {
            id: self.id.text(),
            byte_offset: self.byte_offset,
            type_guid: &self.type_guid,
            type_guid_offset: self.type_guid_offset,
            base_type_guid,
            base_type_guid_offset: self.base_type_guid.offset(),
            version: self.version,
            version_offset: self.version_offset,
            module: &self.module,
            entity_ids: SegmentValues(&self.entities),
            entity_id_offsets: SegmentOffsets(&self.entities),
        }
        .serialize(serializer)
    }
}

/// One type-table entry from a `MetaStream` segment header. The entry registers
/// a record type and lists the entities whose sibling `BulkStream` records
/// carry it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct SegmentTypeWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of this type-table entry in its `MetaStream`.
    byte_offset: u64,
    /// GUID naming this entry's record type. Class tags are segment-local, so
    /// this GUID is the only discriminator that is stable across files.
    type_guid: DesignRelaxedGuidText,
    /// Byte offset of the type-GUID bytes in the `MetaStream`.
    type_guid_offset: u64,
    /// GUID of this type's base type; `None` for a root type, whose stored base
    /// GUID is the empty string.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_type_guid"
    )]
    base_type_guid: Option<String>,
    /// Byte offset of the base-type-GUID bytes, when the entry names one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_type_guid_offset"
    )]
    base_type_guid_offset: Option<u64>,
    /// Record version of this type.
    version: u32,
    /// Byte offset of `version` in the Design `MetaStream`.
    version_offset: u64,
    /// Add-in module that registers this type, e.g. `Fusion`, `MSketch`, or
    /// `Body`. Every type a module registers repeats the module name, so it
    /// classifies a type but does not identify one. Some types record no module.
    module: String,
    /// Entity ids whose records carry this type, in source `MetaStream` order;
    /// a count rather than a fixed-arity list, so length varies per entry.
    entity_ids: Vec<u64>,
    /// Byte offsets parallel to `entity_ids`.
    entity_id_offsets: Vec<u64>,
}

impl TryFrom<SegmentTypeWire> for SegmentType {
    type Error = String;
    /// Nonempty `base_type_guid` text outside the relaxed GUID domain is not decoder-producible and is rejected deliberately.
    fn try_from(wire: SegmentTypeWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.id,
            SegmentTypeData {
                byte_offset: wire.byte_offset,
                type_guid: wire.type_guid,
                type_guid_offset: wire.type_guid_offset,
                version: wire.version,
                version_offset: wire.version_offset,
                module: wire.module,
                entities: ReferenceRun::from_columns(
                    wire.entity_ids,
                    wire.entity_id_offsets,
                    "entity_ids/entity_id_offsets",
                )?,
                base_type_guid: BaseTypeGuid::from_wire(
                    wire.base_type_guid,
                    wire.base_type_guid_offset,
                )?,
            },
        )
    }
}

#[cfg(test)]
impl From<SegmentType> for SegmentTypeWire {
    fn from(value: SegmentType) -> Self {
        let id = value.id.into_string();
        let value = value.data;
        let (entity_ids, entity_id_offsets) = value.entities.into_wire();
        let (base_type_guid, base_type_guid_offset) = value.base_type_guid.into_wire();
        Self {
            id,
            byte_offset: value.byte_offset,
            type_guid: value.type_guid,
            type_guid_offset: value.type_guid_offset,
            version: value.version,
            version_offset: value.version_offset,
            module: value.module,
            entity_ids,
            entity_id_offsets,
            base_type_guid_offset,
            base_type_guid,
        }
    }
}

/// Source timeline frame with bounded, ordered reference locations.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignTimelineFrame {
    byte_offset: u64,
    frame_length: u64,
    context_record_index_offset: u64,
    item_count_offset: u64,
    items: Vec<Located<u64>>,
}

impl DesignTimelineFrame {
    pub(crate) fn new(
        byte_offset: u64,
        frame_length: u64,
        context_record_index_offset: u64,
        item_count_offset: u64,
        items: Vec<Located<u64>>,
    ) -> Result<Self, String> {
        let end = byte_offset
            .checked_add(frame_length)
            .ok_or("timeline.frame_length overflows byte_offset")?;
        if context_record_index_offset <= byte_offset
            || context_record_index_offset
                .checked_add(10)
                .is_none_or(|after| after > item_count_offset)
        {
            return Err("timeline.context_record_index_offset is outside the context span".into());
        }
        if item_count_offset
            .checked_add(4)
            .is_none_or(|after| after > end)
        {
            return Err("timeline.item_count_offset is outside the frame".into());
        }
        if items
            .first()
            .is_some_and(|item| item_count_offset.checked_add(5) != Some(item.offset))
        {
            return Err(
                "timeline.item_record_index_offsets must start after the item count".into(),
            );
        }
        if items.windows(2).any(|pair| {
            pair[0]
                .offset
                .checked_add(11)
                .is_none_or(|minimum| pair[1].offset < minimum)
        }) || items
            .iter()
            .any(|item| item.offset.checked_add(10).is_none_or(|after| after > end))
        {
            return Err("timeline.item_record_index_offsets overlap or exceed the frame".into());
        }
        // The offset check above leaves `items` in ascending offset order.
        let mut seen = std::collections::BTreeMap::new();
        for item in &items {
            let repeated = seen.insert(item.value, true).is_some();
            if item.value == 0 || repeated {
                return Err("timeline.item_record_indices must be nonzero and unique".into());
            }
        }
        Ok(Self {
            byte_offset,
            frame_length,
            context_record_index_offset,
            item_count_offset,
            items,
        })
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    #[cfg(test)]
    pub(crate) fn frame_length(&self) -> u64 {
        self.frame_length
    }
    pub(crate) fn items(&self) -> &[Located<u64>] {
        &self.items
    }

    #[cfg(test)]
    pub(crate) fn test_items(byte_offset: u64, items: Vec<Located<u64>>) -> Self {
        let items = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| Located {
                value: item.value,
                offset: byte_offset + 35 + u64_from_index(index) * 11,
            })
            .collect::<Vec<_>>();
        Self::new(
            byte_offset,
            34 + u64_from_index(items.len()) * 11,
            byte_offset + 20,
            byte_offset + 30,
            items,
        )
        .unwrap()
    }
}

/// Counted Design timeline-item list that carries authored feature order.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignFeatureTimelineWire")]
pub(crate) struct DesignFeatureTimeline {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    segment_end: usize,
    /// Checked source frame and ordered item locations.
    frame: DesignTimelineFrame,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Design entity identity of the timeline record.
    pub(crate) record_index: std::num::NonZeroU64,
    /// Zero-based position in the `MetaStream` timeline-record list.
    pub(crate) source_ordinal: u32,
    /// Same-segment context record referenced before the scope list.
    pub(crate) context_record_index: std::num::NonZeroU64,
}

#[cfg(test)]
thread_local! {
    static TIMELINE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignFeatureTimeline {
    fn clone(&self) -> Self {
        TIMELINE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            segment_end: self.segment_end,
            frame: self.frame.clone(),
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            source_ordinal: self.source_ordinal,
            context_record_index: self.context_record_index,
        }
    }
}

impl DesignFeatureTimeline {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the timeline source frame.
    pub(crate) fn frame(&self) -> &DesignTimelineFrame {
        &self.frame
    }
    /// Returns the Design segment encoded in the identity.
    pub(crate) fn segment(&self) -> &str {
        &self.id.text()[..self.segment_end]
    }
    /// Admits a record whose identity matches its source location.
    pub(crate) fn try_new(
        id: String,
        frame: DesignTimelineFrame,
        class_tag: DesignClassTag,
        record_index: std::num::NonZeroU64,
        source_ordinal: u32,
        context_record_index: std::num::NonZeroU64,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "design-feature-timeline", frame.byte_offset())?;
        let segment_end = crate::ids::design_segment(id.text())
            .ok_or("timeline.id must contain a Design segment")?
            .len();
        Ok(Self {
            id,
            segment_end,
            frame,
            class_tag,
            record_index,
            source_ordinal,
            context_record_index,
        })
    }
}

/// Counted Design timeline-item list that carries authored feature order.
struct TimelineValues<'a>(&'a [Located<u64>]);

impl Serialize for TimelineValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|item| item.value))
    }
}

struct TimelineOffsets<'a>(&'a [Located<u64>]);

impl Serialize for TimelineOffsets<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|item| item.offset))
    }
}

#[derive(Serialize)]
struct DesignFeatureTimelineWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    class_tag: &'a str,
    record_index: u64,
    source_ordinal: u32,
    frame_length: u64,
    context_record_index: u64,
    context_record_index_offset: u64,
    item_count_offset: u64,
    item_record_indices: TimelineValues<'a>,
    item_record_index_offsets: TimelineOffsets<'a>,
}

impl Serialize for DesignFeatureTimeline {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignFeatureTimelineWireRef {
            id: self.id.text(),
            byte_offset: self.frame.byte_offset,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index.get(),
            source_ordinal: self.source_ordinal,
            frame_length: self.frame.frame_length,
            context_record_index: self.context_record_index.get(),
            context_record_index_offset: self.frame.context_record_index_offset,
            item_count_offset: self.frame.item_count_offset,
            item_record_indices: TimelineValues(&self.frame.items),
            item_record_index_offsets: TimelineOffsets(&self.frame.items),
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DesignFeatureTimelineWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of this record in its Design `BulkStream`.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    class_tag: String,
    /// Design entity identity of the timeline record.
    record_index: u64,
    /// Zero-based position in the `MetaStream` timeline-record list.
    source_ordinal: u32,
    /// Complete top-level record length.
    frame_length: u64,
    /// Same-segment context record referenced before the scope list.
    context_record_index: u64,
    /// Byte offset of `context_record_index`.
    context_record_index_offset: u64,
    /// Byte offset of the timeline-item count.
    item_count_offset: u64,
    /// Design record indices in authored timeline order.
    item_record_indices: Vec<u64>,
    /// Byte offsets parallel to `item_record_indices`.
    item_record_index_offsets: Vec<u64>,
}

impl TryFrom<DesignFeatureTimelineWire> for DesignFeatureTimeline {
    type Error = String;
    fn try_from(wire: DesignFeatureTimelineWire) -> Result<Self, Self::Error> {
        if wire.item_record_indices.len() != wire.item_record_index_offsets.len() {
            return Err(
                "item_record_indices and item_record_index_offsets must have equal lengths".into(),
            );
        }
        let items = wire
            .item_record_indices
            .into_iter()
            .zip(wire.item_record_index_offsets)
            .map(|(value, offset)| Located { value, offset })
            .collect();
        Self::try_new(
            wire.id,
            DesignTimelineFrame::new(
                wire.byte_offset,
                wire.frame_length,
                wire.context_record_index_offset,
                wire.item_count_offset,
                items,
            )?,
            DesignClassTag::try_from(wire.class_tag)?,
            std::num::NonZeroU64::new(wire.record_index)
                .ok_or("timeline.record_index must be nonzero")?,
            wire.source_ordinal,
            std::num::NonZeroU64::new(wire.context_record_index)
                .ok_or("timeline.context_record_index must be nonzero")?,
        )
    }
}

#[cfg(test)]
impl From<DesignFeatureTimeline> for DesignFeatureTimelineWire {
    fn from(value: DesignFeatureTimeline) -> Self {
        let (item_record_indices, item_record_index_offsets) = value
            .frame
            .items
            .into_iter()
            .map(|item| (item.value, item.offset))
            .unzip();
        Self {
            item_record_indices,
            item_record_index_offsets,
            id: value.id.into_string(),
            byte_offset: value.frame.byte_offset,
            class_tag: value.class_tag.into(),
            record_index: value.record_index.get(),
            source_ordinal: value.source_ordinal,
            frame_length: value.frame.frame_length,
            context_record_index: value.context_record_index.get(),
            context_record_index_offset: value.frame.context_record_index_offset,
            item_count_offset: value.frame.item_count_offset,
        }
    }
}

/// Self-validating entity-bound header in the Design `BulkStream`.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignEntityHeaderWire")]
pub(crate) struct DesignEntityHeader {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of this entity header in its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Full UTF-16LE-decoded design-entity id string for this header.
    pub(crate) entity_id: DesignEntityId,
    /// Source per-file dynamic three-digit ASCII class tag naming this header's record type.
    pub(crate) class_tag: DesignClassTag,
    /// Whether the flag-selected four-byte optional slot is present.
    pub(crate) optional_slot_present: bool,
    /// Module registration and its sketch-owned data.
    pub(crate) registration: DesignEntityRegistration,
}

#[cfg(test)]
thread_local! {
    static ENTITY_HEADER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignEntityHeader {
    fn clone(&self) -> Self {
        ENTITY_HEADER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            entity_id: self.entity_id.clone(),
            class_tag: self.class_tag.clone(),
            optional_slot_present: self.optional_slot_present,
            registration: self.registration.clone(),
        }
    }
}

/// A sketch header's located reference-list slot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchHeaderReferences {
    /// Owning record, absent for the no-base-record sentinel.
    pub(crate) record_reference: Option<u32>,
    /// Byte offset of the owning-record slot.
    pub(crate) record_reference_offset: u64,
    /// Located references in the counted list.
    pub(crate) references: Vec<Located<u32>>,
}

/// Module registration with data owned only by sketch headers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignEntityRegistration(DesignEntityRegistrationKind);

#[derive(Debug, Clone, PartialEq)]
enum DesignEntityRegistrationKind {
    Other(Option<String>),
    Sketch {
        references: Option<SketchHeaderReferences>,
        members: ReferenceRun<u32>,
    },
}

impl DesignEntityRegistration {
    /// Construct a module registration and its sketch data.
    pub(crate) fn new(
        module: Option<String>,
        references: Option<SketchHeaderReferences>,
        members: ReferenceRun<u32>,
    ) -> Result<Self, String> {
        if module.as_deref() == Some(DESIGN_MODULE_SKETCH) {
            Ok(Self(DesignEntityRegistrationKind::Sketch {
                references,
                members,
            }))
        } else if references.is_some() || !members.is_empty() {
            Err("module must be MSketch for sketch references or members".into())
        } else {
            Ok(Self(DesignEntityRegistrationKind::Other(module)))
        }
    }
}

impl DesignEntityHeader {
    /// Declared reference count for a present sketch reference list.
    pub(crate) fn declared_reference_count(&self) -> Option<usize> {
        self.sketch_references().map(|list| list.references.len())
    }

    /// Registered module name.
    pub(crate) fn module(&self) -> Option<&str> {
        match &self.registration.0 {
            DesignEntityRegistrationKind::Other(module) => module.as_deref(),
            DesignEntityRegistrationKind::Sketch { .. } => Some(DESIGN_MODULE_SKETCH),
        }
    }

    /// Located sketch reference-list slot.
    pub(crate) fn sketch_references(&self) -> Option<&SketchHeaderReferences> {
        match &self.registration.0 {
            DesignEntityRegistrationKind::Sketch { references, .. } => references.as_ref(),
            DesignEntityRegistrationKind::Other(_) => None,
        }
    }

    /// Mutable located sketch reference-list slot.
    pub(crate) fn sketch_references_mut(&mut self) -> Option<&mut SketchHeaderReferences> {
        match &mut self.registration.0 {
            DesignEntityRegistrationKind::Sketch { references, .. } => references.as_mut(),
            DesignEntityRegistrationKind::Other(_) => None,
        }
    }

    /// Referenced record indices.
    pub(crate) fn reference_values(&self) -> impl Iterator<Item = &u32> {
        self.sketch_references()
            .into_iter()
            .flat_map(|list| list.references.iter().map(|row| &row.value))
    }

    /// Member record indices.
    pub(crate) fn member_values(&self) -> impl Iterator<Item = &u32> {
        match &self.registration.0 {
            DesignEntityRegistrationKind::Sketch { members, .. } => Some(members),
            DesignEntityRegistrationKind::Other(_) => None,
        }
        .into_iter()
        .flat_map(ReferenceRun::values)
    }

    /// Whether the entity belongs to the sketch module.
    pub(crate) fn in_sketch_module(&self) -> bool {
        matches!(
            self.registration.0,
            DesignEntityRegistrationKind::Sketch { .. }
        )
    }
}

/// Self-validating entity-bound header in the Design `BulkStream`.
struct HeaderReferenceValues<'a>(Option<&'a SketchHeaderReferences>);

impl Serialize for HeaderReferenceValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .into_iter()
                .flat_map(|list| list.references.iter().map(|item| item.value)),
        )
    }
}

struct HeaderReferenceOffsets<'a>(Option<&'a SketchHeaderReferences>);

impl Serialize for HeaderReferenceOffsets<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .into_iter()
                .flat_map(|list| list.references.iter().map(|item| item.offset)),
        )
    }
}

struct HeaderMemberValues<'a>(Option<&'a ReferenceRun<u32>>);

impl HeaderMemberValues<'_> {
    fn is_empty(&self) -> bool {
        self.0.is_none_or(ReferenceRun::is_empty)
    }
}

impl Serialize for HeaderMemberValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.into_iter().flat_map(ReferenceRun::values))
    }
}

struct HeaderMemberOffsets<'a>(Option<&'a ReferenceRun<u32>>);

impl HeaderMemberOffsets<'_> {
    fn is_empty(&self) -> bool {
        self.0.is_none_or(|members| members.offsets().len() == 0)
    }
}

impl Serialize for HeaderMemberOffsets<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.into_iter().flat_map(ReferenceRun::offsets))
    }
}

#[derive(Serialize)]
struct DesignEntityHeaderWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    entity_id: &'a str,
    class_tag: &'a str,
    optional_slot_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_reference: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_reference_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_reference_count: Option<usize>,
    reference_indices: HeaderReferenceValues<'a>,
    reference_offsets: HeaderReferenceOffsets<'a>,
    #[serde(skip_serializing_if = "HeaderMemberValues::is_empty")]
    member_indices: HeaderMemberValues<'a>,
    #[serde(skip_serializing_if = "HeaderMemberOffsets::is_empty")]
    member_offsets: HeaderMemberOffsets<'a>,
}

impl Serialize for DesignEntityHeader {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let references = self.sketch_references();
        let members = match &self.registration.0 {
            DesignEntityRegistrationKind::Other(_) => None,
            DesignEntityRegistrationKind::Sketch { members, .. } => Some(members),
        };
        DesignEntityHeaderWireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            entity_id: self.entity_id.as_str(),
            class_tag: self.class_tag.as_str(),
            optional_slot_present: self.optional_slot_present,
            module: self.module(),
            record_reference: references.and_then(|list| list.record_reference),
            record_reference_offset: references.map(|list| list.record_reference_offset),
            declared_reference_count: self.declared_reference_count(),
            reference_indices: HeaderReferenceValues(references),
            reference_offsets: HeaderReferenceOffsets(references),
            member_indices: HeaderMemberValues(members),
            member_offsets: HeaderMemberOffsets(members),
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DesignEntityHeaderWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of this entity header in its Design `BulkStream`.
    byte_offset: u64,
    /// Full UTF-16LE-decoded design-entity id string for this header.
    entity_id: String,
    /// Source per-file dynamic three-digit ASCII class tag naming this header's record type.
    class_tag: String,
    /// Whether the flag-selected four-byte optional slot is present.
    optional_slot_present: bool,
    /// Add-in module of the `MetaStream` type whose entity-id list contains this
    /// header's entity, when the `MetaStream` registers that entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_module"
    )]
    module: Option<String>,
    /// Index of an associated `BulkStream` record, when the header carries one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_record_reference"
    )]
    record_reference: Option<u32>,
    /// Byte offset of `record_reference` in the Design `BulkStream`, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_record_reference_offset"
    )]
    record_reference_offset: Option<u64>,
    /// Declared count of reference entries the header claims to own, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_declared_reference_count"
    )]
    declared_reference_count: Option<usize>,
    /// Padded record-reference run owned by a sketch entity container.
    #[serde(default)]
    reference_indices: Vec<u32>,
    /// Byte offsets parallel to `reference_indices`.
    #[serde(default)]
    reference_offsets: Vec<u64>,
    /// Counted member-record run from the paired same-index container record
    /// of an `EntityGenesis`-form sketch entity header.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    member_indices: Vec<u32>,
    /// Byte offsets parallel to `member_indices`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    member_offsets: Vec<u64>,
}

impl TryFrom<DesignEntityHeaderWire> for DesignEntityHeader {
    type Error = String;
    /// Header references without paired `record_reference_offset` and `declared_reference_count` metadata are not decoder-producible and are rejected deliberately.
    fn try_from(wire: DesignEntityHeaderWire) -> Result<Self, Self::Error> {
        if wire
            .declared_reference_count
            .is_some_and(|count| count != wire.reference_indices.len())
        {
            return Err("declared_reference_count must match reference_indices".into());
        }
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        let references = match (wire.record_reference_offset, wire.declared_reference_count) {
            (Some(offset), Some(_)) => {
                if wire.reference_indices.len() != wire.reference_offsets.len() {
                    return Err("reference_offsets must locate every reference_indices entry".into());
                }
                Some(SketchHeaderReferences {
                    record_reference: wire.record_reference,
                    record_reference_offset: offset,
                    references: wire.reference_indices.into_iter().zip(wire.reference_offsets)
                        .map(|(value, offset)| Located { value, offset }).collect(),
                })
            }
            (None, None) if wire.record_reference.is_none() && wire.reference_indices.is_empty() && wire.reference_offsets.is_empty() => None,
            _ => return Err("record_reference_offset and declared_reference_count must accompany reference_indices and record_reference".into()),
        };
        let members = ReferenceRun::from_columns(
            wire.member_indices,
            wire.member_offsets,
            "member_indices/member_offsets",
        )?;
        Ok(Self {
            registration: DesignEntityRegistration::new(wire.module, references, members)?,
            id: wire.id,
            byte_offset: wire.byte_offset,
            entity_id,
            class_tag: DesignClassTag::try_from(wire.class_tag)?,
            optional_slot_present: wire.optional_slot_present,
        })
    }
}

#[cfg(test)]
impl From<DesignEntityHeader> for DesignEntityHeaderWire {
    fn from(header: DesignEntityHeader) -> Self {
        let declared_reference_count = header.declared_reference_count();
        let (module, references, members) = match header.registration.0 {
            DesignEntityRegistrationKind::Other(module) => {
                (module, None, ReferenceRun::unlocated(Vec::new()))
            }
            DesignEntityRegistrationKind::Sketch {
                references,
                members,
            } => (Some(DESIGN_MODULE_SKETCH.to_owned()), references, members),
        };
        let (record_reference, record_reference_offset, reference_indices, reference_offsets) =
            match references {
                Some(list) => {
                    let (values, offsets) = list
                        .references
                        .into_iter()
                        .map(|row| (row.value, row.offset))
                        .unzip();
                    (
                        list.record_reference,
                        Some(list.record_reference_offset),
                        values,
                        offsets,
                    )
                }
                None => (None, None, Vec::new(), Vec::new()),
            };
        let (member_indices, member_offsets) = members.into_wire();
        Self {
            declared_reference_count,
            reference_indices,
            reference_offsets,
            member_indices,
            member_offsets,
            id: header.id,
            byte_offset: header.byte_offset,
            entity_id: header.entity_id.into_string(),
            class_tag: header.class_tag.into(),
            optional_slot_present: header.optional_slot_present,
            module,
            record_reference,
            record_reference_offset,
        }
    }
}

#[cfg(test)]
mod tests;
