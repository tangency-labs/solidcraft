// SPDX-License-Identifier: Apache-2.0
//! ACT registry records: tables, channels, entities and the root component and layout.

use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_core::decode::u64_from_index;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::identity::{Located, NativeRecordId};
use super::mesh::DesignGuidText;
use super::references::DesignClassTag;

cadmpeg_core::named_optional_field!(deserialize_channel_class_tag, String, "channel_class_tag");
cadmpeg_core::named_optional_field!(
    deserialize_channel_entity_id_offset,
    u64,
    "channel_entity_id_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_channel_record_index_offset,
    u64,
    "channel_record_index_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_table_entity_id_offset,
    u64,
    "table_entity_id_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_table_record_index_offset,
    u64,
    "table_record_index_offset"
);
/// ACT component-link registry flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum ActRegistryFlag {
    Off,
    On,
}

impl ActRegistryFlag {
    #[must_use]
    pub(crate) fn from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Off),
            1 => Some(Self::On),
            _ => None,
        }
    }

    #[must_use]
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::On => 1,
        }
    }
}

impl TryFrom<u32> for ActRegistryFlag {
    type Error = String;

    fn try_from(code: u32) -> Result<Self, Self::Error> {
        Self::from_code(code).ok_or_else(|| format!("act registry flag must be 0 or 1, not {code}"))
    }
}

impl From<ActRegistryFlag> for u32 {
    fn from(flag: ActRegistryFlag) -> Self {
        flag.code()
    }
}

/// Inline `ACTTable` row attached to one change group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActTableRow {
    record_index_offset: u64,
}

impl ActTableRow {
    pub(crate) fn new(record_index_offset: u64) -> Result<Self, String> {
        if record_index_offset.checked_add(14).is_none() {
            return Err("table_record_index_offset overflows table_entity_id_offset".into());
        }
        Ok(Self {
            record_index_offset,
        })
    }

    fn entity_id_offset(&self) -> u64 {
        self.record_index_offset + 14
    }
}

/// Opaque stream residue retained after the final ACT channel group.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActStreamResidue {
    bytes: Vec<u8>,
    offset: u64,
}

impl ActStreamResidue {
    pub(crate) fn new(bytes: Vec<u8>, offset: u64) -> Result<Self, String> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Err("channel_class_tail must contain non-padding bytes".into());
        }
        if u64::try_from(bytes.len())
            .ok()
            .and_then(|len| offset.checked_add(len))
            .is_none()
        {
            return Err("channel_class_tail_offset and channel_class_tail length overflow".into());
        }
        Ok(Self { bytes, offset })
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
}

/// Channel-group payload owned by one ACT entity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActChannelGroup {
    record_index_offset: u64,
    entity_id_offset: Option<u64>,
    class_tag: DesignClassTag,
    channels: BTreeMap<String, Located<DesignGuidText>>,
    class_tail: Option<ActStreamResidue>,
}

fn validate_act_channel_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 || !name.is_ascii() {
        return Err("ACT channel name must contain 1 through 128 ASCII bytes".into());
    }
    Ok(())
}

impl ActChannelGroup {
    pub(crate) fn try_new(
        record_index_offset: u64,
        entity_id_offset: Option<u64>,
        class_tag: DesignClassTag,
        channels: BTreeMap<String, Located<DesignGuidText>>,
        class_tail: Option<ActStreamResidue>,
    ) -> Result<Self, String> {
        if channels.is_empty() || channels.len() > 8 {
            return Err("ACT channels must contain 1 through 8 entries".into());
        }
        if entity_id_offset.is_some_and(|offset| offset <= record_index_offset) {
            return Err("channel_entity_id_offset must follow channel_record_index_offset".into());
        }
        for (name, guid) in &channels {
            validate_act_channel_name(name)?;
            let end = guid
                .offset
                .checked_add(72)
                .ok_or("channel_guid_offsets overflow")?;
            if guid.offset <= record_index_offset
                || entity_id_offset.is_some_and(|offset| end > offset)
            {
                return Err(
                    "channel_guid_offsets must follow the record index and precede the entity key"
                        .into(),
                );
            }
            if class_tail.as_ref().is_some_and(|tail| end > tail.offset()) {
                return Err("channel_guid_offsets must precede channel_class_tail_offset".into());
            }
        }
        if class_tail.as_ref().is_some_and(|tail| {
            record_index_offset >= tail.offset()
                || entity_id_offset.is_some_and(|offset| offset >= tail.offset())
        }) {
            return Err(
                "channel record and entity offsets must precede channel_class_tail_offset".into(),
            );
        }
        Ok(Self {
            record_index_offset,
            entity_id_offset,
            class_tag,
            channels,
            class_tail,
        })
    }
    pub(crate) fn channels(&self) -> &BTreeMap<String, Located<DesignGuidText>> {
        &self.channels
    }
}

/// One Fusion ACT change-version channel group and its optional inline table row.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActEntitySerde")]
pub(crate) struct ActEntity {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Record index shared by the channel group and its optional `ACTTable` row.
    record_index: u32,
    entity_id: String,
    table_row: Option<ActTableRow>,
    channel_group: ActChannelGroup,
}

#[cfg(test)]
std::thread_local! {
    static ACT_ENTITY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActEntity {
    fn clone(&self) -> Self {
        ACT_ENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            record_index: self.record_index,
            entity_id: self.entity_id.clone(),
            table_row: self.table_row.clone(),
            channel_group: self.channel_group.clone(),
        }
    }
}

struct ChannelValues<'a>(&'a BTreeMap<String, Located<DesignGuidText>>);

impl Serialize for ChannelValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, guid) in self.0 {
            map.serialize_entry(name, guid.value.as_str())?;
        }
        map.end()
    }
}

struct ChannelOffsets<'a>(&'a BTreeMap<String, Located<DesignGuidText>>);

impl Serialize for ChannelOffsets<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, guid) in self.0 {
            map.serialize_entry(name, &guid.offset)?;
        }
        map.end()
    }
}

#[derive(Serialize)]
struct ActEntityWireRef<'a> {
    id: &'a str,
    record_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    table_record_index_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_record_index_offset: Option<u64>,
    entity_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    table_entity_id_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_entity_id_offset: Option<u64>,
    in_table: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_class_tag: Option<&'a str>,
    channels: ChannelValues<'a>,
    channel_guid_offsets: ChannelOffsets<'a>,
    #[serde(skip_serializing_if = "<[u8]>::is_empty")]
    channel_class_tail: NativeBytes<&'a [u8]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_class_tail_offset: Option<u64>,
}

impl Serialize for ActEntity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (channel_class_tail, channel_class_tail_offset) = match &self.channel_group.class_tail {
            Some(tail) => (tail.bytes.as_slice(), Some(tail.offset)),
            None => (&[][..], None),
        };
        ActEntityWireRef {
            id: self.id.text(),
            record_index: self.record_index,
            table_record_index_offset: self.table_record_index_offset(),
            channel_record_index_offset: Some(self.channel_record_index_offset()),
            entity_id: &self.entity_id,
            table_entity_id_offset: self.table_entity_id_offset(),
            channel_entity_id_offset: self.channel_entity_id_offset(),
            in_table: self.in_table(),
            channel_class_tag: Some(self.channel_class_tag()),
            channels: ChannelValues(&self.channel_group.channels),
            channel_guid_offsets: ChannelOffsets(&self.channel_group.channels),
            channel_class_tail: (channel_class_tail).into(),
            channel_class_tail_offset,
        }
        .serialize(serializer)
    }
}

impl ActEntity {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the ACT entity record index.
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn try_new(
        id: String,
        record_index: u32,
        entity_id: String,
        table_row: Option<ActTableRow>,
        channel_group: ActChannelGroup,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-entity", record_index)?;
        if !crate::act::is_entity_key(&entity_id) {
            return Err("ACT entity_id must be a decimal segment_entity key".into());
        }
        if table_row.is_none() && channel_group.entity_id_offset.is_none() {
            return Err("channel_entity_id_offset is required without an ACTTable row".into());
        }
        Ok(Self {
            id,
            record_index,
            entity_id,
            table_row,
            channel_group,
        })
    }
    pub(crate) fn entity_id(&self) -> &str {
        &self.entity_id
    }
    pub(crate) fn try_set_entity_id(&mut self, entity_id: String) -> Result<(), String> {
        if !crate::act::is_entity_key(&entity_id) {
            return Err("ACT entity_id must be a decimal segment_entity key".into());
        }
        self.entity_id = entity_id;
        Ok(())
    }
    pub(crate) fn in_table(&self) -> bool {
        self.table_row.is_some()
    }
    pub(crate) fn channel_group(&self) -> &ActChannelGroup {
        &self.channel_group
    }
    pub(crate) fn set_channel_guid(
        &mut self,
        name: &str,
        value: DesignGuidText,
    ) -> Result<(), String> {
        let guid = self
            .channel_group
            .channels
            .get_mut(name)
            .ok_or("ACT channel does not exist")?;
        guid.value = value;
        Ok(())
    }
    pub(crate) fn table_record_index_offset(&self) -> Option<u64> {
        self.table_row.as_ref().map(|row| row.record_index_offset)
    }
    pub(crate) fn table_entity_id_offset(&self) -> Option<u64> {
        self.table_row.as_ref().map(ActTableRow::entity_id_offset)
    }
    fn channel_record_index_offset(&self) -> u64 {
        self.channel_group.record_index_offset
    }
    pub(crate) fn channel_entity_id_offset(&self) -> Option<u64> {
        self.channel_group.entity_id_offset
    }
    pub(crate) fn channel_class_tag(&self) -> &str {
        self.channel_group.class_tag.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct ActEntitySerde {
    id: String,
    record_index: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_table_record_index_offset"
    )]
    table_record_index_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_channel_record_index_offset"
    )]
    channel_record_index_offset: Option<u64>,
    entity_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_table_entity_id_offset"
    )]
    table_entity_id_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_channel_entity_id_offset"
    )]
    channel_entity_id_offset: Option<u64>,
    in_table: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_channel_class_tag"
    )]
    channel_class_tag: Option<String>,
    #[serde(default)]
    channels: BTreeMap<String, String>,
    #[serde(default)]
    channel_guid_offsets: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    channel_class_tail: NativeBytes<Vec<u8>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_channel_class_tail_offset"
    )]
    channel_class_tail_offset: Option<u64>,
}

// The tail and its offset are stated together, so the refusal names which
// half the document left unstated.
cadmpeg_core::named_optional_field!(
    deserialize_channel_class_tail_offset,
    u64,
    "channel_class_tail_offset"
);

impl TryFrom<ActEntitySerde> for ActEntity {
    type Error = String;

    fn try_from(wire: ActEntitySerde) -> Result<Self, Self::Error> {
        if wire.channels.keys().ne(wire.channel_guid_offsets.keys()) {
            return Err("channels and channel_guid_offsets must have identical keys".into());
        }
        let table = match (
            wire.in_table,
            wire.table_record_index_offset,
            wire.table_entity_id_offset,
        ) {
            (true, Some(record_index_offset), Some(entity_id_offset)) => {
                let row = ActTableRow::new(record_index_offset)?;
                if row.entity_id_offset() != entity_id_offset {
                    return Err(
                        "table_entity_id_offset must follow table_record_index_offset by 14 bytes"
                            .into(),
                    );
                }
                Some(row)
            }
            (false, None, None) => None,
            _ => {
                return Err(
                    "act entity in_table disagrees with table_record_index_offset/table_entity_id_offset"
                        .into(),
                );
            }
        };
        let group = match (wire.channel_class_tag, wire.channel_record_index_offset) {
            (None, None)
                if wire.channels.is_empty()
                    && wire.channel_guid_offsets.is_empty()
                    && wire.channel_class_tail.is_empty()
                    && wire.channel_entity_id_offset.is_none()
                    && wire.channel_class_tail_offset.is_none() =>
            {
                None
            }
            (Some(class_tag), Some(record_index_offset)) => Some(ActChannelGroup::try_new(
                record_index_offset,
                wire.channel_entity_id_offset,
                class_tag
                    .try_into()
                    .map_err(|error| format!("channel_class_tag: {error}"))?,
                wire.channels
                    .into_iter()
                    .zip(wire.channel_guid_offsets)
                    .map(|((name, value), (_, offset))| {
                        Ok((
                            name,
                            Located {
                                value: value.try_into()?,
                                offset,
                            },
                        ))
                    })
                    .collect::<Result<_, String>>()?,
                match (wire.channel_class_tail, wire.channel_class_tail_offset) {
                    (bytes, None) if bytes.is_empty() => None,
                    (bytes, Some(offset)) => {
                        Some(ActStreamResidue::new(bytes.into_inner(), offset)?)
                    }
                    _ => return Err("channel_class_tail requires channel_class_tail_offset".into()),
                },
            )?),
            _ => {
                return Err(
                    "act entity channel_class_tag disagrees with channel_record_index_offset"
                        .into(),
                );
            }
        };
        Self::try_new(
            wire.id,
            wire.record_index,
            wire.entity_id,
            table,
            group.ok_or("ACT entity requires a channel group")?,
        )
    }
}

#[cfg(test)]
impl From<ActEntity> for ActEntitySerde {
    fn from(entity: ActEntity) -> Self {
        let in_table = entity.in_table();
        let table_record_index_offset = entity.table_record_index_offset();
        let table_entity_id_offset = entity.table_entity_id_offset();
        let channel_record_index_offset = Some(entity.channel_record_index_offset());
        let channel_entity_id_offset = entity.channel_entity_id_offset();
        let channel_class_tag = Some(entity.channel_class_tag().to_owned());
        let group = entity.channel_group;
        let (channels, channel_guid_offsets) = group
            .channels
            .into_iter()
            .map(|(name, guid)| {
                (
                    (name.clone(), guid.value.as_str().to_owned()),
                    (name, guid.offset),
                )
            })
            .unzip();
        let (channel_class_tail, channel_class_tail_offset) = match group.class_tail {
            Some(tail) => (tail.bytes, Some(tail.offset)),
            None => (Vec::new(), None),
        };
        Self {
            id: entity.id.into_string(),
            record_index: entity.record_index,
            table_record_index_offset,
            channel_record_index_offset,
            entity_id: entity.entity_id,
            table_entity_id_offset,
            channel_entity_id_offset,
            in_table,
            channel_class_tag,
            channels,
            channel_guid_offsets,
            channel_class_tail: (channel_class_tail).into(),
            channel_class_tail_offset,
        }
    }
}

/// One GUID in the ordered ACT stream-wide asset/change-version pool.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActGuidWire")]
pub(crate) struct ActGuid {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Byte offset of the UTF-16 length prefix in the ACT `BulkStream`.
    byte_offset: u64,
    /// Position in the pool in source order; does not assign a GUID to one table entry.
    pub(crate) ordinal: u32,
    /// The pooled GUID string.
    pub(crate) guid: DesignGuidText,
}

#[cfg(test)]
std::thread_local! {
    static ACT_GUID_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActGuid {
    fn clone(&self) -> Self {
        ACT_GUID_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            ordinal: self.ordinal,
            guid: self.guid.clone(),
        }
    }
}

#[derive(Serialize)]
struct ActGuidWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    guid_offset: u64,
    ordinal: u32,
    guid: &'a str,
}

impl Serialize for ActGuid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActGuidWireRef {
            id: self.id.text(),
            byte_offset: self.byte_offset,
            guid_offset: self.guid_offset(),
            ordinal: self.ordinal,
            guid: self.guid.as_str(),
        }
        .serialize(serializer)
    }
}

impl ActGuid {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn new(
        id: String,
        byte_offset: u64,
        ordinal: u32,
        guid: String,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-guid", byte_offset)?;
        byte_offset
            .checked_add(4)
            .ok_or("ACT GUID byte_offset overflows guid_offset")?;
        Ok(Self {
            id,
            byte_offset,
            ordinal,
            guid: guid.try_into()?,
        })
    }

    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    pub(crate) fn guid_offset(&self) -> u64 {
        self.byte_offset + 4
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct ActGuidWire {
    id: String,
    byte_offset: u64,
    guid_offset: u64,
    ordinal: u32,
    guid: String,
}

impl TryFrom<ActGuidWire> for ActGuid {
    type Error = String;

    fn try_from(wire: ActGuidWire) -> Result<Self, Self::Error> {
        let guid = Self::new(wire.id, wire.byte_offset, wire.ordinal, wire.guid)?;
        if wire.guid_offset != guid.guid_offset() {
            return Err("ACT GUID guid_offset must follow byte_offset by four bytes".into());
        }
        Ok(guid)
    }
}

#[cfg(test)]
impl From<ActGuid> for ActGuidWire {
    fn from(guid: ActGuid) -> Self {
        let guid_offset = guid.guid_offset();
        Self {
            id: guid.id.into_string(),
            byte_offset: guid.byte_offset,
            guid_offset,
            ordinal: guid.ordinal,
            guid: guid.guid.into(),
        }
    }
}

/// One reference in the ACT table run between the GUID pool and channel registry.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActTableReferenceWire")]
pub(crate) struct ActTableReference {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Position in the counted reference run, in source order.
    pub(crate) ordinal: u32,
    /// Byte offset of the reference-presence marker in the ACT `BulkStream`.
    byte_offset: u64,
    /// Target ACT record index.
    pub(crate) target_record: u32,
}

#[cfg(test)]
std::thread_local! {
    static ACT_TABLE_REFERENCE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActTableReference {
    fn clone(&self) -> Self {
        ACT_TABLE_REFERENCE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            ordinal: self.ordinal,
            byte_offset: self.byte_offset,
            target_record: self.target_record,
        }
    }
}

#[derive(Serialize)]
struct ActTableReferenceWireRef<'a> {
    id: &'a str,
    ordinal: u32,
    byte_offset: u64,
    target_record: u32,
    target_record_offset: u64,
}

impl Serialize for ActTableReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActTableReferenceWireRef {
            id: self.id.text(),
            ordinal: self.ordinal,
            byte_offset: self.byte_offset,
            target_record: self.target_record,
            target_record_offset: self.target_record_offset(),
        }
        .serialize(serializer)
    }
}

impl ActTableReference {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn new(
        id: String,
        ordinal: u32,
        byte_offset: u64,
        target_record: u32,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-table-reference", byte_offset)?;
        byte_offset
            .checked_add(1)
            .ok_or("ACT table reference byte_offset overflows target_record_offset")?;
        Ok(Self {
            id,
            ordinal,
            byte_offset,
            target_record,
        })
    }

    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    fn target_record_offset(&self) -> u64 {
        self.byte_offset + 1
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct ActTableReferenceWire {
    id: String,
    ordinal: u32,
    byte_offset: u64,
    target_record: u32,
    target_record_offset: u64,
}

impl TryFrom<ActTableReferenceWire> for ActTableReference {
    type Error = String;

    fn try_from(wire: ActTableReferenceWire) -> Result<Self, Self::Error> {
        let reference = Self::new(wire.id, wire.ordinal, wire.byte_offset, wire.target_record)?;
        if wire.target_record_offset != reference.target_record_offset() {
            return Err("ACT table reference target_record_offset must follow byte_offset".into());
        }
        Ok(reference)
    }
}

#[cfg(test)]
impl From<ActTableReference> for ActTableReferenceWire {
    fn from(reference: ActTableReference) -> Self {
        let target_record_offset = reference.target_record_offset();
        Self {
            id: reference.id.into_string(),
            ordinal: reference.ordinal,
            byte_offset: reference.byte_offset,
            target_record: reference.target_record,
            target_record_offset,
        }
    }
}

/// One named entry in the ACT table's stream-wide channel registry.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActRegistryChannelWire")]
pub(crate) struct ActRegistryChannel {
    id: NativeRecordId,
    pub(crate) ordinal: u32,
    byte_offset: u64,
    name: String,
    pub(crate) guid: DesignGuidText,
}

#[cfg(test)]
std::thread_local! {
    static ACT_REGISTRY_CHANNEL_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActRegistryChannel {
    fn clone(&self) -> Self {
        ACT_REGISTRY_CHANNEL_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            ordinal: self.ordinal,
            byte_offset: self.byte_offset,
            name: self.name.clone(),
            guid: self.guid.clone(),
        }
    }
}

#[derive(Serialize)]
struct ActRegistryChannelWireRef<'a> {
    id: &'a str,
    ordinal: u32,
    byte_offset: u64,
    name: &'a str,
    name_offset: u64,
    guid: &'a str,
    guid_offset: u64,
}

impl Serialize for ActRegistryChannel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActRegistryChannelWireRef {
            id: self.id.text(),
            ordinal: self.ordinal,
            byte_offset: self.byte_offset,
            name: &self.name,
            name_offset: self.name_offset(),
            guid: self.guid.as_str(),
            guid_offset: self.guid_offset(),
        }
        .serialize(serializer)
    }
}

impl ActRegistryChannel {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    pub(crate) fn new(
        id: String,
        ordinal: u32,
        byte_offset: u64,
        name: String,
        guid: String,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-registry-channel", byte_offset)?;
        validate_act_channel_name(&name)?;
        byte_offset
            .checked_add(8 + u64_from_index(name.len()))
            .ok_or("ACT registry offset overflow")?;
        Ok(Self {
            id,
            ordinal,
            byte_offset,
            name,
            guid: guid.try_into()?,
        })
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    fn name_offset(&self) -> u64 {
        self.byte_offset + 4
    }
    pub(crate) fn guid_offset(&self) -> u64 {
        self.byte_offset + 8 + u64_from_index(self.name.len())
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct ActRegistryChannelWire {
    id: String,
    ordinal: u32,
    byte_offset: u64,
    name: String,
    name_offset: u64,
    guid: String,
    guid_offset: u64,
}

impl TryFrom<ActRegistryChannelWire> for ActRegistryChannel {
    type Error = String;
    fn try_from(wire: ActRegistryChannelWire) -> Result<Self, Self::Error> {
        let channel = Self::new(
            wire.id,
            wire.ordinal,
            wire.byte_offset,
            wire.name,
            wire.guid,
        )?;
        if wire.name_offset != channel.name_offset() || wire.guid_offset != channel.guid_offset() {
            return Err("ACT registry offsets must follow the stored name layout".into());
        }
        Ok(channel)
    }
}

#[cfg(test)]
impl From<ActRegistryChannel> for ActRegistryChannelWire {
    fn from(channel: ActRegistryChannel) -> Self {
        let name_offset = channel.name_offset();
        let guid_offset = channel.guid_offset();
        Self {
            id: channel.id.into_string(),
            ordinal: channel.ordinal,
            byte_offset: channel.byte_offset,
            name: channel.name,
            name_offset,
            guid: channel.guid.as_str().into(),
            guid_offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActComponentReferences {
    pub(crate) instance_root: u32,
    pub(crate) tracked_entity: u32,
    pub(crate) components_root: u32,
}

/// ACT link from a tracked Design entity to the instance/component registries.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ActComponentLinkWire")]
pub(crate) struct ActComponentLink {
    /// Globally unique deterministic identifier for this native record.
    id: NativeRecordId,
    /// Index of this record within the ACT `BulkStream`.
    pub(crate) record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    class_tag: DesignClassTag,
    pub(crate) references: ActComponentReferences,
    /// Source counter/registry flag; 0 and 1 are both valid.
    pub(crate) registry_flag: ActRegistryFlag,
    /// Checked source layout and the two variable-length strings.
    layout: ActComponentLayout,
}

#[cfg(test)]
std::thread_local! {
    static ACT_COMPONENT_LINK_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ActComponentLink {
    fn clone(&self) -> Self {
        ACT_COMPONENT_LINK_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            record_index: self.record_index,
            class_tag: self.class_tag.clone(),
            references: self.references.clone(),
            registry_flag: self.registry_flag,
            layout: self.layout.clone(),
        }
    }
}

#[derive(Serialize)]
struct ActComponentLinkWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    record_index: u32,
    record_index_offset: u64,
    class_tag: &'a str,
    instance_root_record: u32,
    instance_root_record_offset: u64,
    tracked_entity_record: u32,
    tracked_entity_record_offset: u64,
    components_root_record: u32,
    components_root_record_offset: u64,
    registry_flag: ActRegistryFlag,
    registry_flag_offset: u64,
    entity_id: &'a str,
    entity_id_offset: u64,
    display_name: &'a str,
    display_name_offset: u64,
}

impl Serialize for ActComponentLink {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ActComponentLinkWireRef {
            id: self.id.text(),
            byte_offset: self.layout.byte_offset(),
            record_index: self.record_index,
            record_index_offset: self.layout.record_index_offset(),
            class_tag: self.class_tag.as_str(),
            instance_root_record: self.references.instance_root,
            instance_root_record_offset: self.layout.instance_root_record_offset(),
            tracked_entity_record: self.references.tracked_entity,
            tracked_entity_record_offset: self.layout.tracked_entity_record_offset(),
            components_root_record: self.references.components_root,
            components_root_record_offset: self.layout.components_root_record_offset(),
            registry_flag: self.registry_flag,
            registry_flag_offset: self.layout.registry_flag_offset(),
            entity_id: self.layout.entity_id(),
            entity_id_offset: self.layout.entity_id_offset(),
            display_name: self.layout.display_name(),
            display_name_offset: self.layout.display_name_offset(),
        }
        .serialize(serializer)
    }
}

impl ActComponentLink {
    /// Returns the admitted native identity.
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    /// Returns the ACT component-link source layout.
    pub(crate) fn layout(&self) -> &ActComponentLayout {
        &self.layout
    }
    /// Returns the native stream encoded in the identity.
    pub(crate) fn stream(&self) -> &str {
        self.id.stream()
    }
    /// Admits a record whose identity matches its source location.
    pub(crate) fn try_new(
        id: String,
        record_index: u32,
        class_tag: DesignClassTag,
        references: ActComponentReferences,
        registry_flag: ActRegistryFlag,
        layout: ActComponentLayout,
    ) -> Result<Self, String> {
        let id = NativeRecordId::try_new(id, "act-component-link", layout.byte_offset())?;
        Ok(Self {
            id,
            record_index,
            class_tag,
            references,
            registry_flag,
            layout,
        })
    }
    /// Changes component-link strings without changing the identity offset.
    pub(crate) fn try_set_strings(
        &mut self,
        entity_id: String,
        display_name: String,
    ) -> Result<(), String> {
        self.layout = self.layout.with_strings(entity_id, display_name)?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct ActComponentLinkWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of this record in the ACT `BulkStream`.
    byte_offset: u64,
    /// Index of this record within the ACT `BulkStream`.
    record_index: u32,
    /// Byte offset of `record_index`.
    record_index_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    class_tag: String,
    /// Record index of the instance registry root.
    instance_root_record: u32,
    /// Byte offset of `instance_root_record`.
    instance_root_record_offset: u64,
    /// Record index of the Design entity tracked by this link.
    tracked_entity_record: u32,
    /// Byte offset of `tracked_entity_record`.
    tracked_entity_record_offset: u64,
    /// Record index of the components registry root.
    components_root_record: u32,
    /// Byte offset of `components_root_record`.
    components_root_record_offset: u64,
    /// Source counter/registry flag; 0 and 1 are both valid.
    registry_flag: ActRegistryFlag,
    /// Byte offset of `registry_flag`.
    registry_flag_offset: u64,
    /// UTF-16LE-decoded key of the tracked Design entity.
    entity_id: String,
    /// Byte offset of the UTF-16 `entity_id` code units.
    entity_id_offset: u64,
    /// Display name stored alongside this component link.
    display_name: String,
    /// Byte offset of the UTF-16 `display_name` code units.
    display_name_offset: u64,
}

impl TryFrom<ActComponentLinkWire> for ActComponentLink {
    type Error = String;

    fn try_from(wire: ActComponentLinkWire) -> Result<Self, Self::Error> {
        let display_bytes = u64::try_from(wire.display_name.encode_utf16().count())
            .ok()
            .and_then(|length| length.checked_mul(2))
            .ok_or("display_name length overflows")?;
        let padding = wire
            .display_name_offset
            .checked_add(display_bytes)
            .and_then(|end| wire.components_root_record_offset.checked_sub(end))
            .and_then(|gap| gap.checked_sub(1))
            .ok_or("components_root_record_offset does not follow display_name")?;
        let layout =
            ActComponentLayout::new(wire.byte_offset, wire.entity_id, wire.display_name, padding)?;
        if wire.record_index_offset != layout.record_index_offset() {
            return Err("record_index_offset disagrees with ACT component-link layout".into());
        }
        if wire.instance_root_record_offset != layout.instance_root_record_offset() {
            return Err(
                "instance_root_record_offset disagrees with ACT component-link layout".into(),
            );
        }
        if wire.tracked_entity_record_offset != layout.tracked_entity_record_offset() {
            return Err(
                "tracked_entity_record_offset disagrees with ACT component-link layout".into(),
            );
        }
        if wire.registry_flag_offset != layout.registry_flag_offset() {
            return Err("registry_flag_offset disagrees with ACT component-link layout".into());
        }
        if wire.entity_id_offset != layout.entity_id_offset() {
            return Err("entity_id_offset disagrees with ACT component-link layout".into());
        }
        if wire.display_name_offset != layout.display_name_offset() {
            return Err("display_name_offset disagrees with ACT component-link layout".into());
        }
        Self::try_new(
            wire.id,
            wire.record_index,
            wire.class_tag
                .try_into()
                .map_err(|error| format!("class_tag: {error}"))?,
            ActComponentReferences {
                instance_root: wire.instance_root_record,
                tracked_entity: wire.tracked_entity_record,
                components_root: wire.components_root_record,
            },
            wire.registry_flag,
            layout,
        )
    }
}

#[cfg(test)]
impl From<ActComponentLink> for ActComponentLinkWire {
    fn from(root: ActComponentLink) -> Self {
        Self {
            id: root.id.into_string(),
            record_index: root.record_index,
            class_tag: root.class_tag.into(),
            instance_root_record: root.references.instance_root,
            components_root_record: root.references.components_root,
            registry_flag: root.registry_flag,
            record_index_offset: root.layout.record_index_offset(),
            instance_root_record_offset: root.layout.instance_root_record_offset(),
            tracked_entity_record_offset: root.layout.tracked_entity_record_offset(),
            registry_flag_offset: root.layout.registry_flag_offset(),
            entity_id_offset: root.layout.entity_id_offset(),
            display_name_offset: root.layout.display_name_offset(),
            components_root_record_offset: root.layout.components_root_record_offset(),
            tracked_entity_record: root.references.tracked_entity,
            byte_offset: root.layout.byte_offset,
            entity_id: root.layout.entity_id,
            display_name: root.layout.display_name,
        }
    }
}

/// Source extent of an ACT component link. Offsets follow its fixed grammar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActComponentLayout {
    byte_offset: u64,
    entity_id: String,
    display_name: String,
    padding: u64,
}

impl ActComponentLayout {
    pub(crate) fn new(
        byte_offset: u64,
        entity_id: String,
        display_name: String,
        padding: u64,
    ) -> Result<Self, String> {
        if !crate::act::is_entity_key(&entity_id) {
            return Err("entity_id must be an ACT entity key".into());
        }
        if padding > 8 {
            return Err(
                "components_root_record_offset requires zero through eight padding bytes".into(),
            );
        }
        let string_bytes = entity_id
            .encode_utf16()
            .count()
            .checked_add(display_name.encode_utf16().count())
            .and_then(|length| u64::try_from(length).ok())
            .and_then(|length| length.checked_mul(2))
            .ok_or("ACT component-link string lengths overflow")?;
        byte_offset
            .checked_add(56)
            .and_then(|offset| offset.checked_add(string_bytes))
            .and_then(|offset| offset.checked_add(padding))
            .ok_or("components_root_record_offset overflows ACT component-link layout")?;
        Ok(Self {
            byte_offset,
            entity_id,
            display_name,
            padding,
        })
    }

    fn with_strings(&self, entity_id: String, display_name: String) -> Result<Self, String> {
        Self::new(self.byte_offset, entity_id, display_name, self.padding)
    }

    fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn entity_id(&self) -> &str {
        &self.entity_id
    }
    pub(crate) fn display_name(&self) -> &str {
        &self.display_name
    }
    fn record_index_offset(&self) -> u64 {
        self.byte_offset + 7
    }
    pub(crate) fn instance_root_record_offset(&self) -> u64 {
        self.byte_offset + 22
    }
    pub(crate) fn entity_id_offset(&self) -> u64 {
        self.byte_offset + 36
    }
    pub(crate) fn tracked_entity_record_offset(&self) -> u64 {
        self.entity_id_offset() + u64_from_index(self.entity_id.encode_utf16().count()) * 2 + 1
    }
    pub(crate) fn registry_flag_offset(&self) -> u64 {
        self.tracked_entity_record_offset() + 10
    }
    pub(crate) fn display_name_offset(&self) -> u64 {
        self.registry_flag_offset() + 8
    }
    pub(crate) fn components_root_record_offset(&self) -> u64 {
        self.display_name_offset()
            + u64_from_index(self.display_name.encode_utf16().count()) * 2
            + self.padding
            + 1
    }
}

#[cfg(test)]
mod tests;
