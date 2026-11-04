// SPDX-License-Identifier: Apache-2.0
//! Design body members, bounds, bindings and visibility.

use super::identity::NativeRecordId;
use super::mesh::DesignMeshSceneBounds;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::Point3;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_body, BodyId, "body");
cadmpeg_core::named_optional_field!(
    deserialize_component_owner,
    super::body_ownership::BodyOwnership,
    "component_owner"
);
/// A nonblank containing Design `BulkStream` archive path.
#[derive(Debug, Clone, PartialEq)]
struct DesignBulkStreamPath(NonBlankString);

impl TryFrom<String> for DesignBulkStreamPath {
    type Error = String;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        let prefix = text
            .strip_suffix("/BulkStream.dat")
            .ok_or("stream must name a containing Design BulkStream")?;
        if prefix.is_empty()
            || text.chars().any(char::is_control)
            || prefix
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err("stream must name a containing Design BulkStream".into());
        }
        Ok(Self(
            NonBlankString::new(text).ok_or("stream must not be blank")?,
        ))
    }
}

impl DesignBulkStreamPath {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// A native reference to one admitted Design body-map pair.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct DesignBodyBindingId(NativeRecordId);

impl TryFrom<String> for DesignBodyBindingId {
    type Error = String;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        let key = text
            .rsplit_once('#')
            .ok_or("binding id requires a numeric key")?
            .1
            .parse::<u64>()
            .map_err(|_| "binding id requires a numeric key")?;
        Ok(Self(NativeRecordId::try_f3d_new(
            text,
            "design-body-binding",
            key,
        )?))
    }
}

impl Serialize for DesignBodyBindingId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.text().serialize(serializer)
    }
}

impl DesignBodyBindingId {
    fn as_str(&self) -> &str {
        self.0.text()
    }
}

/// One member of the Design `BulkStream` `BodiesRoot` list.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignBodyMemberWire")]
pub(crate) struct DesignBodyMember {
    id: NativeRecordId,
    byte_offset: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) flags: u16,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct DesignBodyMemberWire {
    pub(crate) id: String,
    pub(crate) byte_offset: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) flags: u16,
}

impl TryFrom<DesignBodyMemberWire> for DesignBodyMember {
    type Error = String;
    fn try_from(wire: DesignBodyMemberWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: NativeRecordId::try_f3d_new(wire.id, "design-body-member", wire.byte_offset)?,
            byte_offset: wire.byte_offset,
            entity_suffix: wire.entity_suffix,
            flags: wire.flags,
        })
    }
}

impl DesignBodyMember {
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
}

impl Serialize for DesignBodyMember {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("DesignBodyMember", 4)?;
        record.serialize_field("id", self.id.text())?;
        record.serialize_field("byte_offset", &self.byte_offset)?;
        record.serialize_field("entity_suffix", &self.entity_suffix)?;
        record.serialize_field("flags", &self.flags)?;
        record.end()
    }
}

/// Triplicated axis-aligned body bounds cached in the Design stream.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignBodyBoundsWire")]
pub(crate) struct DesignBodyBounds {
    /// Globally unique deterministic identifier for this native record set.
    id: NativeRecordId,
    /// Numeric suffix of the owning Design body entity.
    entity_suffix: u32,
    /// Byte offset of the owning Design entity header.
    entity_byte_offset: u64,
    /// Indexed-header byte offsets parallel to `record_indices`.
    record_byte_offsets: [u64; 3],
    /// First f64 byte of each repeated sextuple.
    value_byte_offsets: [u64; 3],
    /// Design BREP body-map pairs carrying this entity suffix, in stream order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    body_binding_ids: Vec<DesignBodyBindingId>,
    corners: DesignMeshSceneBounds,
}

#[cfg(test)]
std::thread_local! {
    static BODY_BOUNDS_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignBodyBounds {
    fn clone(&self) -> Self {
        BODY_BOUNDS_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            entity_suffix: self.entity_suffix,
            entity_byte_offset: self.entity_byte_offset,
            record_byte_offsets: self.record_byte_offsets,
            value_byte_offsets: self.value_byte_offsets,
            body_binding_ids: self.body_binding_ids.clone(),
            corners: self.corners,
        }
    }
}

#[derive(Serialize)]
struct DesignBodyBoundsWireRef<'a> {
    id: &'a str,
    entity_suffix: u64,
    entity_byte_offset: u64,
    record_indices: [u32; 3],
    record_byte_offsets: [u64; 3],
    value_byte_offsets: [u64; 3],
    #[serde(skip_serializing_if = "<[DesignBodyBindingId]>::is_empty")]
    body_binding_ids: &'a [DesignBodyBindingId],
    maximum: Point3,
    minimum: Point3,
}

impl Serialize for DesignBodyBounds {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let [x, y, z] = self.corners.maximum();
        let maximum = Point3::new(x, y, z);
        let [x, y, z] = self.corners.minimum();
        DesignBodyBoundsWireRef {
            id: self.id.text(),
            entity_suffix: self.entity_suffix(),
            entity_byte_offset: self.entity_byte_offset,
            record_indices: self.record_indices(),
            record_byte_offsets: self.record_byte_offsets,
            value_byte_offsets: self.value_byte_offsets,
            body_binding_ids: &self.body_binding_ids,
            maximum,
            minimum: Point3::new(x, y, z),
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(crate) struct DesignBodyBoundsWire<P = Point3> {
    /// Globally unique deterministic identifier for this native record set.
    pub(crate) id: String,
    /// Numeric suffix of the owning Design body entity.
    pub(crate) entity_suffix: u64,
    /// Byte offset of the owning Design entity header.
    pub(crate) entity_byte_offset: u64,
    /// Three consecutive indexed record identities carrying the cache.
    pub(crate) record_indices: [u32; 3],
    /// Indexed-header byte offsets parallel to `record_indices`.
    pub(crate) record_byte_offsets: [u64; 3],
    /// First f64 byte of each repeated sextuple.
    pub(crate) value_byte_offsets: [u64; 3],
    /// Design BREP body-map pairs carrying this entity suffix, in stream order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) body_binding_ids: Vec<DesignBodyBindingId>,
    /// Maximum model-space corner in millimetres.
    pub(crate) maximum: P,
    /// Minimum model-space corner in millimetres.
    pub(crate) minimum: P,
}

impl TryFrom<DesignBodyBoundsWire> for DesignBodyBounds {
    type Error = String;
    fn try_from(wire: DesignBodyBoundsWire) -> Result<Self, Self::Error> {
        Self::validate_layout(&wire)?;
        let maximum = FinitePoint3::new(wire.maximum)
            .ok_or("scene bounds maximum and minimum must be finite")?;
        let minimum = FinitePoint3::new(wire.minimum)
            .ok_or("scene bounds maximum and minimum must be finite")?;
        Self::from_parts(DesignBodyBoundsWire {
            id: wire.id,
            entity_suffix: wire.entity_suffix,
            entity_byte_offset: wire.entity_byte_offset,
            record_indices: wire.record_indices,
            record_byte_offsets: wire.record_byte_offsets,
            value_byte_offsets: wire.value_byte_offsets,
            body_binding_ids: wire.body_binding_ids,
            maximum,
            minimum,
        })
    }
}

impl DesignBodyBounds {
    fn validate_layout<P>(wire: &DesignBodyBoundsWire<P>) -> Result<u32, String> {
        let entity_suffix = u32::try_from(wire.entity_suffix)
            .map_err(|_| "entity_suffix exceeds indexed record range")?;
        let last = entity_suffix
            .checked_add(3)
            .ok_or("entity_suffix leaves no room for three cache records")?;
        if wire.record_indices != [last - 2, last - 1, last] {
            return Err("record_indices must follow entity_suffix consecutively".into());
        }
        if !wire
            .record_byte_offsets
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err("record_byte_offsets must be strictly increasing".into());
        }
        if !wire
            .value_byte_offsets
            .iter()
            .zip(wire.record_byte_offsets)
            .all(|(value, record)| *value > record)
        {
            return Err("value_byte_offsets must follow record_byte_offsets".into());
        }
        Ok(entity_suffix)
    }

    pub(crate) fn from_parts(wire: DesignBodyBoundsWire<FinitePoint3>) -> Result<Self, String> {
        let entity_suffix = Self::validate_layout(&wire)?;
        let corners = DesignMeshSceneBounds::from_parts(wire.maximum, wire.minimum)?;
        if wire.maximum == wire.minimum {
            return Err("maximum and minimum must not define a degenerate box".into());
        }
        Ok(Self {
            id: NativeRecordId::try_f3d_new(
                wire.id,
                "design-body-bounds",
                wire.entity_byte_offset,
            )?,
            entity_suffix,
            entity_byte_offset: wire.entity_byte_offset,
            record_byte_offsets: wire.record_byte_offsets,
            value_byte_offsets: wire.value_byte_offsets,
            body_binding_ids: wire.body_binding_ids,
            corners,
        })
    }
}

impl DesignBodyBounds {
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    pub(crate) fn entity_byte_offset(&self) -> u64 {
        self.entity_byte_offset
    }
    pub(crate) fn body_binding_ids(&self) -> impl ExactSizeIterator<Item = &str> {
        self.body_binding_ids
            .iter()
            .map(DesignBodyBindingId::as_str)
    }
    pub(crate) fn set_body_binding_ids(&mut self, ids: Vec<DesignBodyBindingId>) {
        self.body_binding_ids = ids;
    }
    pub(crate) fn entity_suffix(&self) -> u64 {
        u64::from(self.entity_suffix)
    }
    fn record_indices(&self) -> [u32; 3] {
        [
            self.entity_suffix + 1,
            self.entity_suffix + 2,
            self.entity_suffix + 3,
        ]
    }
}

#[cfg(test)]
impl From<DesignBodyBounds> for DesignBodyBoundsWire {
    fn from(value: DesignBodyBounds) -> Self {
        let [x, y, z] = value.corners.maximum();
        let maximum = Point3::new(x, y, z);
        let [x, y, z] = value.corners.minimum();
        Self {
            entity_suffix: value.entity_suffix(),
            record_indices: value.record_indices(),
            id: value.id.into_string(),
            entity_byte_offset: value.entity_byte_offset,
            record_byte_offsets: value.record_byte_offsets,
            value_byte_offsets: value.value_byte_offsets,
            body_binding_ids: value.body_binding_ids,
            maximum,
            minimum: Point3::new(x, y, z),
        }
    }
}

/// One ordered pair in a Design `BulkStream` BREP body-map record.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignBodyBindingWire")]
pub(crate) struct DesignBodyBinding {
    /// Globally unique deterministic identifier for this native map entry.
    id: NativeRecordId,
    /// Design `BulkStream` ZIP entry containing the map.
    stream: DesignBulkStreamPath,
    /// Number of pairs in the enclosing body map.
    pair_count: std::num::NonZeroU32,
    /// Zero-based position in the enclosing body map.
    pair_ordinal: u32,
    /// BREP body selector stored by this pair.
    pub(crate) asm_body_key: u64,
    /// Byte offset of `asm_body_key` within `stream`.
    asm_body_key_offset: u64,
    /// Numeric Design entity suffix stored by this pair.
    pub(crate) entity_suffix: u64,
    /// Basename of the BREP blob whose body namespace contains the key.
    blob_name: String,
    /// Byte offset of the UTF-16LE `blob_name` code units within `stream`.
    blob_name_offset: u64,
    /// Solved body in the BREP blob named by this pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) body: Option<BodyId>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component_owner"
    )]
    pub(crate) component_owner: Option<super::body_ownership::BodyOwnership>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) component_owner_unresolved: bool,
    /// The pair's body is outside the current model: complete Body-container
    /// memberships omit its entity and its browser node stores the hidden flag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) history_only: bool,
}

#[cfg(test)]
std::thread_local! {
    static BODY_BINDING_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignBodyBinding {
    fn clone(&self) -> Self {
        BODY_BINDING_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            stream: self.stream.clone(),
            pair_count: self.pair_count,
            pair_ordinal: self.pair_ordinal,
            asm_body_key: self.asm_body_key,
            asm_body_key_offset: self.asm_body_key_offset,
            entity_suffix: self.entity_suffix,
            blob_name: self.blob_name.clone(),
            blob_name_offset: self.blob_name_offset,
            body: self.body.clone(),
            component_owner: self.component_owner.clone(),
            component_owner_unresolved: self.component_owner_unresolved,
            history_only: self.history_only,
        }
    }
}

#[derive(Serialize)]
struct DesignBodyBindingWireRef<'a> {
    id: &'a str,
    stream: &'a str,
    pair_count: u32,
    pair_ordinal: u32,
    asm_body_key: u64,
    asm_body_key_offset: u64,
    entity_suffix: u64,
    entity_suffix_offset: u64,
    blob_name: &'a str,
    blob_name_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<&'a BodyId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    component_owner: Option<&'a super::body_ownership::BodyOwnership>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    component_owner_unresolved: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    history_only: bool,
}

impl Serialize for DesignBodyBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignBodyBindingWireRef {
            id: self.id.text(),
            stream: self.stream.as_str(),
            pair_count: self.pair_count(),
            pair_ordinal: self.pair_ordinal,
            asm_body_key: self.asm_body_key,
            asm_body_key_offset: self.asm_body_key_offset,
            entity_suffix: self.entity_suffix,
            entity_suffix_offset: self.entity_suffix_offset(),
            blob_name: &self.blob_name,
            blob_name_offset: self.blob_name_offset,
            body: self.body.as_ref(),
            component_owner: self.component_owner.as_ref(),
            component_owner_unresolved: self.component_owner_unresolved,
            history_only: self.history_only,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(crate) struct DesignBodyBindingWire {
    /// Globally unique deterministic identifier for this native map entry.
    pub(crate) id: String,
    /// Design `BulkStream` ZIP entry containing the map.
    pub(crate) stream: String,
    /// Number of pairs in the enclosing body map.
    pub(crate) pair_count: u32,
    /// Zero-based position in the enclosing body map.
    pub(crate) pair_ordinal: u32,
    /// BREP body selector stored by this pair.
    pub(crate) asm_body_key: u64,
    /// Byte offset of `asm_body_key` within `stream`.
    pub(crate) asm_body_key_offset: u64,
    /// Numeric Design entity suffix stored by this pair.
    pub(crate) entity_suffix: u64,
    /// Byte offset of `entity_suffix` within `stream`.
    pub(crate) entity_suffix_offset: u64,
    /// Basename of the BREP blob whose body namespace contains the key.
    pub(crate) blob_name: String,
    /// Byte offset of the UTF-16LE `blob_name` code units within `stream`.
    pub(crate) blob_name_offset: u64,
    /// Solved body in the BREP blob named by this pair.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_body"
    )]
    pub(crate) body: Option<BodyId>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component_owner"
    )]
    pub(crate) component_owner: Option<super::body_ownership::BodyOwnership>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) component_owner_unresolved: bool,
    /// The pair's body is outside the current model.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) history_only: bool,
}

impl TryFrom<DesignBodyBindingWire> for DesignBodyBinding {
    type Error = String;
    fn try_from(wire: DesignBodyBindingWire) -> Result<Self, Self::Error> {
        let pair_count =
            std::num::NonZeroU32::new(wire.pair_count).ok_or("pair_count must be nonzero")?;
        if wire.pair_ordinal >= pair_count.get() {
            return Err("pair_ordinal must be less than pair_count".into());
        }
        let entity_suffix_offset = wire
            .asm_body_key_offset
            .checked_add(8)
            .ok_or("asm_body_key_offset overflows entity_suffix_offset")?;
        if wire.entity_suffix_offset != entity_suffix_offset {
            return Err(
                "entity_suffix_offset must follow asm_body_key_offset by eight bytes".into(),
            );
        }
        if !wire.blob_name.starts_with("BREP.") {
            return Err("blob_name must start with BREP.".into());
        }
        if wire.blob_name_offset <= entity_suffix_offset {
            return Err("blob_name_offset must follow entity_suffix_offset".into());
        }
        if wire.history_only
            && (wire.body.is_some()
                || wire.component_owner.is_some()
                || wire.component_owner_unresolved)
        {
            return Err("a history-only pair must name no solved body and no owner".into());
        }
        let id =
            NativeRecordId::try_f3d_new(wire.id, "design-body-binding", wire.asm_body_key_offset)?;
        let stream = DesignBulkStreamPath::try_from(wire.stream)?;
        if !crate::ids::native_scope_matches(id.stream(), stream.as_str()) {
            return Err("id must identify its containing stream".into());
        }
        Ok(Self {
            pair_count,
            id,
            stream,
            pair_ordinal: wire.pair_ordinal,
            asm_body_key: wire.asm_body_key,
            asm_body_key_offset: wire.asm_body_key_offset,
            entity_suffix: wire.entity_suffix,
            blob_name: wire.blob_name,
            blob_name_offset: wire.blob_name_offset,
            body: wire.body,
            component_owner: wire.component_owner,
            component_owner_unresolved: wire.component_owner_unresolved,
            history_only: wire.history_only,
        })
    }
}

impl DesignBodyBinding {
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    pub(crate) fn stream(&self) -> &str {
        self.stream.as_str()
    }
    pub(crate) fn pair_count(&self) -> u32 {
        self.pair_count.get()
    }
    pub(crate) fn pair_ordinal(&self) -> u32 {
        self.pair_ordinal
    }
    pub(crate) fn asm_body_key_offset(&self) -> u64 {
        self.asm_body_key_offset
    }
    pub(crate) fn entity_suffix_offset(&self) -> u64 {
        self.asm_body_key_offset + 8
    }
    pub(crate) fn blob_name(&self) -> &str {
        &self.blob_name
    }
    pub(crate) fn blob_name_offset(&self) -> u64 {
        self.blob_name_offset
    }
}

#[cfg(test)]
impl From<DesignBodyBinding> for DesignBodyBindingWire {
    fn from(value: DesignBodyBinding) -> Self {
        Self {
            pair_count: value.pair_count(),
            entity_suffix_offset: value.entity_suffix_offset(),
            id: value.id.into_string(),
            stream: value.stream.0.into_string(),
            pair_ordinal: value.pair_ordinal,
            asm_body_key: value.asm_body_key,
            asm_body_key_offset: value.asm_body_key_offset,
            entity_suffix: value.entity_suffix,
            blob_name: value.blob_name,
            blob_name_offset: value.blob_name_offset,
            body: value.body,
            component_owner: value.component_owner,
            component_owner_unresolved: value.component_owner_unresolved,
            history_only: value.history_only,
        }
    }
}

/// Design browser-node visibility joined to one solved ASM body.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "BodyVisibilityWire")]
pub(crate) struct BodyVisibility {
    id: NativeRecordId,
    pub(crate) body: BodyId,
    stream: DesignBulkStreamPath,
    pub(crate) byte_offset: u64,
    pub(crate) asm_body_key_offset: u64,
    asm_body_key: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) visible: bool,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct BodyVisibilityWire {
    pub(crate) id: String,
    pub(crate) body: BodyId,
    pub(crate) stream: String,
    pub(crate) byte_offset: u64,
    pub(crate) asm_body_key_offset: u64,
    pub(crate) asm_body_key: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) visible: bool,
}

impl TryFrom<BodyVisibilityWire> for BodyVisibility {
    type Error = String;
    fn try_from(wire: BodyVisibilityWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: NativeRecordId::try_f3d_new(wire.id, "body-visibility", wire.asm_body_key)?,
            body: wire.body,
            stream: DesignBulkStreamPath::try_from(wire.stream)?,
            byte_offset: wire.byte_offset,
            asm_body_key_offset: wire.asm_body_key_offset,
            asm_body_key: wire.asm_body_key,
            entity_suffix: wire.entity_suffix,
            visible: wire.visible,
        })
    }
}

impl BodyVisibility {
    pub(crate) fn id(&self) -> &String {
        self.id.text()
    }
    pub(crate) fn stream(&self) -> &str {
        self.stream.as_str()
    }
    pub(crate) fn asm_body_key(&self) -> u64 {
        self.asm_body_key
    }
}

impl Serialize for BodyVisibility {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("BodyVisibility", 8)?;
        record.serialize_field("id", self.id.text())?;
        record.serialize_field("body", &self.body)?;
        record.serialize_field("stream", self.stream.as_str())?;
        record.serialize_field("byte_offset", &self.byte_offset)?;
        record.serialize_field("asm_body_key_offset", &self.asm_body_key_offset)?;
        record.serialize_field("asm_body_key", &self.asm_body_key)?;
        record.serialize_field("entity_suffix", &self.entity_suffix)?;
        record.serialize_field("visible", &self.visible)?;
        record.end()
    }
}

#[cfg(test)]
impl From<BodyVisibility> for BodyVisibilityWire {
    fn from(value: BodyVisibility) -> Self {
        Self {
            id: value.id.into_string(),
            body: value.body,
            stream: value.stream.0.into_string(),
            byte_offset: value.byte_offset,
            asm_body_key_offset: value.asm_body_key_offset,
            asm_body_key: value.asm_body_key,
            entity_suffix: value.entity_suffix,
            visible: value.visible,
        }
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    use super::{DesignBodyBinding, DesignBodyBindingWire, DesignBodyBounds, DesignBodyBoundsWire};

    #[test]
    fn native_reference_walk_visits_typed_body_without_copying_plain_text() {
        use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
        let value = super::BodyVisibility::try_from(super::BodyVisibilityWire {
            id: "f3d:Design/BulkStream.dat:body-visibility#1".to_owned(),
            body: super::BodyId::mint("f3d:model:body#1").unwrap(),
            stream: "Design/BulkStream.dat".to_owned(),
            byte_offset: 0,
            asm_body_key_offset: 0,
            asm_body_key: 1,
            entity_suffix: 1,
            visible: true,
        })
        .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut references = Vec::new();
        value
            .visit_identity_references(&ctx, &mut |id| {
                references.push(id.to_owned());
                Ok(())
            })
            .unwrap();
        assert_eq!(references, ["f3d:model:body#1"]);
        ctx.finish_session().unwrap();
    }

    fn bounds_fixture() -> DesignBodyBounds {
        serde_json::from_value(serde_json::json!({
            "id": "f3d:native:design-body-bounds#0",
            "entity_suffix": 10,
            "entity_byte_offset": 0,
            "record_indices": [11, 12, 13],
            "record_byte_offsets": [20, 40, 60],
            "value_byte_offsets": [21, 41, 61],
            "body_binding_ids": ["f3d:Design/BulkStream.dat:design-body-binding#10"],
            "maximum": {"x": 1.0, "y": 0.0, "z": 0.0},
            "minimum": {"x": 0.0, "y": 0.0, "z": 0.0}
        }))
        .unwrap()
    }

    fn binding_fixture() -> DesignBodyBinding {
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:design-body-binding#10",
            "stream": "Design/BulkStream.dat",
            "pair_count": 1,
            "pair_ordinal": 0,
            "asm_body_key": 0,
            "asm_body_key_offset": 10,
            "entity_suffix": 0,
            "entity_suffix_offset": 18,
            "blob_name": "BREP.",
            "blob_name_offset": 19
        }))
        .unwrap()
    }

    #[test]
    fn body_bounds_borrowed_wire_matches_owned_wire_bytes() {
        let bounds = bounds_fixture();
        let owned = DesignBodyBoundsWire::from(bounds.clone());
        assert_eq!(
            serde_json::to_vec(&bounds).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }

    #[test]
    fn body_bounds_native_retained_limit_refuses_before_binding_ids_clone() {
        let bounds = bounds_fixture();
        let needed = serde_json::to_vec(&bounds).unwrap().len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        super::BODY_BOUNDS_CLONE_COUNT.with(|count| count.set(0));
        let error = namespace
            .set_arena(
                &limited,
                "design_body_bounds",
                std::slice::from_ref(&bounds),
            )
            .unwrap_err();
        super::BODY_BOUNDS_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
        assert!(matches!(
            cadmpeg_core::CodecError::from(error),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "serialize native record"
        ));

        let (service, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        namespace
            .set_arena(
                &service,
                "design_body_bounds",
                std::slice::from_ref(&bounds),
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&namespace.arenas()["design_body_bounds"][0]).unwrap(),
            serde_json::to_value(&bounds).unwrap()
        );
    }

    #[test]
    fn body_binding_borrowed_wire_matches_owned_wire_bytes() {
        let binding = binding_fixture();
        let owned = DesignBodyBindingWire::from(binding.clone());
        assert_eq!(
            serde_json::to_vec(&binding).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }

    #[test]
    fn body_binding_native_retained_limit_refuses_before_stream_clone() {
        let binding = binding_fixture();
        let needed = serde_json::to_vec(&binding).unwrap().len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        super::BODY_BINDING_CLONE_COUNT.with(|count| count.set(0));
        let error = namespace
            .set_arena(
                &limited,
                "design_body_bindings",
                std::slice::from_ref(&binding),
            )
            .unwrap_err();
        super::BODY_BINDING_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
        assert!(matches!(
            cadmpeg_core::CodecError::from(error),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "serialize native record"
        ));

        let (service, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        namespace
            .set_arena(
                &service,
                "design_body_bindings",
                std::slice::from_ref(&binding),
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&namespace.arenas()["design_body_bindings"][0]).unwrap(),
            serde_json::to_value(&binding).unwrap()
        );
    }
    #[test]
    fn body_member_rejects_unbound_native_identity() {
        let wire = serde_json::json!({"id":"f3d:Design/BulkStream.dat:design-body-member#10", "byte_offset":10, "entity_suffix":7, "flags":0});
        let member: super::DesignBodyMember = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&member).unwrap(), wire);
        for id in [
            "",
            "f3d:Design/BulkStream.dat:other#10",
            "f3d:Design/BulkStream.dat:design-body-member#11",
            "stream:design-body-member#10",
        ] {
            let mut value = wire.clone();
            value["id"] = serde_json::json!(id);
            assert!(serde_json::from_value::<super::DesignBodyMember>(value.clone()).is_err());
            let raw: super::DesignBodyMemberWire = serde_json::from_value(value).unwrap();
            assert!(super::DesignBodyMember::try_from(raw).is_err());
        }
    }

    #[test]
    fn body_bounds_reject_unbound_record_and_binding_identities() {
        let valid = bounds_fixture();
        for (field, value) in [
            ("id", serde_json::json!("")),
            ("id", serde_json::json!("f3d:native:design-body-bounds#1")),
            ("id", serde_json::json!("f3d:native:other#0")),
            ("body_binding_ids", serde_json::json!([""])),
            (
                "body_binding_ids",
                serde_json::json!(["f3d:native:other#1"]),
            ),
            (
                "body_binding_ids",
                serde_json::json!(["f3d:native:design-body-binding#bad"]),
            ),
        ] {
            let mut wire = serde_json::to_value(&valid).unwrap();
            wire[field] = value;
            assert!(
                serde_json::from_value::<DesignBodyBounds>(wire).is_err(),
                "{field}"
            );
        }
        let mut wire = DesignBodyBoundsWire::from(valid);
        wire.id.clear();
        assert!(DesignBodyBounds::try_from(wire).is_err());
        assert!(super::DesignBodyBindingId::try_from(String::new()).is_err());
    }

    #[test]
    fn body_binding_rejects_unbound_record_and_stream_identities() {
        let valid = binding_fixture();
        for (field, value) in [
            ("id", ""),
            ("id", "f3d:Design/BulkStream.dat:design-body-binding#11"),
            ("id", "f3d:Design/BulkStream.dat:other#10"),
            ("stream", ""),
            ("stream", "Other/BulkStream.dat"),
            ("stream", "Design/MetaStream.dat"),
        ] {
            let mut wire = serde_json::to_value(&valid).unwrap();
            wire[field] = serde_json::json!(value);
            assert!(serde_json::from_value::<DesignBodyBinding>(wire.clone()).is_err());
            let raw: DesignBodyBindingWire = serde_json::from_value(wire).unwrap();
            assert!(DesignBodyBinding::try_from(raw).is_err());
        }
    }

    #[test]
    fn body_visibility_rejects_unbound_record_and_stream_identities() {
        let wire = serde_json::json!({"id":"f3d:Breps/BREP.synthetic.smbh:body-visibility#3", "body":"f3d:brep:entity#1", "stream":"Design/BulkStream.dat", "byte_offset":10, "asm_body_key_offset":20, "asm_body_key":3, "entity_suffix":7, "visible":true});
        let visibility: super::BodyVisibility = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&visibility).unwrap(), wire);
        for (field, value) in [
            ("id", ""),
            ("id", "f3d:Breps/BREP.synthetic.smbh:body-visibility#4"),
            ("id", "f3d:Breps/BREP.synthetic.smbh:other#3"),
            ("stream", ""),
            ("stream", "Design/MetaStream.dat"),
        ] {
            let mut value_wire = wire.clone();
            value_wire[field] = serde_json::json!(value);
            assert!(serde_json::from_value::<super::BodyVisibility>(value_wire.clone()).is_err());
            let raw: super::BodyVisibilityWire = serde_json::from_value(value_wire).unwrap();
            assert!(super::BodyVisibility::try_from(raw).is_err());
        }
    }
}

mod identity_rewrite;
