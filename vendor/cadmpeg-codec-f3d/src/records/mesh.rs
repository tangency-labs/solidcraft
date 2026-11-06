// SPDX-License-Identifier: Apache-2.0
//! Mesh records: texture tables, scene state and nodes, mesh bodies, collections and mesh features.

use cadmpeg_core::decode::u64_from_index;

use super::identity::Located;
use super::references::DesignClassTag;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::AssetId;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::transform::Transform;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_container_mesh_uuid,
    DesignMeshUuid,
    "container_mesh_uuid"
);
cadmpeg_core::named_optional_field!(
    deserialize_scene_node_bounds,
    DesignMeshSceneBoundsWire,
    "scene_node_bounds"
);
cadmpeg_core::named_optional_field!(
    deserialize_scene_node_transform,
    MeshAffineTransform,
    "scene_node_transform"
);
cadmpeg_core::named_optional_field!(
    deserialize_scene_node_transform_offset,
    u64,
    "scene_node_transform_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_scene_state_bounds,
    DesignMeshSceneBoundsWire,
    "scene_state_bounds"
);
cadmpeg_core::named_optional_field!(deserialize_tessellation_id, String, "tessellation_id");
/// A hyphenated hexadecimal GUID with its original letter case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct DesignGuidText(String);

impl DesignGuidText {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DesignGuidText {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !crate::bytes::is_guid_hyphenated(&value) {
            return Err("GUID must be 36 hyphenated hexadecimal characters".into());
        }
        Ok(Self(value))
    }
}

impl From<DesignGuidText> for String {
    fn from(value: DesignGuidText) -> Self {
        value.0
    }
}

/// The identity of a placed component occurrence: a relaxed GUID, or the
/// positive decimal key that the attributed occurrence frame can store.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct DesignOccurrenceKey(cadmpeg_ir::ids::IdentityKey);

/// Longest decimal occurrence key.
const DECIMAL_OCCURRENCE_KEY_DIGITS: usize = 9;

impl Serialize for DesignOccurrenceKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl DesignOccurrenceKey {
    /// The original key text.
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The key with ASCII letters converted to lowercase.
    pub(crate) fn identity_key(&self) -> cadmpeg_ir::ids::IdentityKey {
        self.0.to_ascii_lowercase()
    }

    /// Whether `value` is a decimal occurrence key rather than a GUID.
    pub(crate) fn is_decimal_key(value: &str) -> bool {
        (1..=DECIMAL_OCCURRENCE_KEY_DIGITS).contains(&value.len())
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && !value.starts_with('0')
    }
}

impl TryFrom<String> for DesignOccurrenceKey {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !Self::is_decimal_key(&value) && !crate::bytes::is_guid_relaxed(&value) {
            return Err("occurrence key must be a relaxed GUID or a positive decimal key".into());
        }
        cadmpeg_ir::ids::IdentityKey::try_new(value)
            .map(Self)
            .map_err(|error| error.to_string())
    }
}

impl From<DesignRelaxedGuidText> for DesignOccurrenceKey {
    fn from(value: DesignRelaxedGuidText) -> Self {
        Self(value.0)
    }
}

/// Text that identifies a placed component occurrence.
pub(crate) trait OccurrenceIdentity {
    /// The original text.
    fn identity_text(&self) -> &str;
    /// The identity with ASCII letters converted to lowercase.
    fn occurrence_identity_key(&self) -> cadmpeg_ir::ids::IdentityKey;
}

impl OccurrenceIdentity for DesignOccurrenceKey {
    fn identity_text(&self) -> &str {
        self.as_str()
    }
    fn occurrence_identity_key(&self) -> cadmpeg_ir::ids::IdentityKey {
        self.identity_key()
    }
}

impl OccurrenceIdentity for DesignRelaxedGuidText {
    fn identity_text(&self) -> &str {
        self.as_str()
    }
    fn occurrence_identity_key(&self) -> cadmpeg_ir::ids::IdentityKey {
        self.identity_key()
    }
}

/// A relaxed GUID with its original text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct DesignRelaxedGuidText(cadmpeg_ir::ids::IdentityKey);

impl Serialize for DesignRelaxedGuidText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl DesignRelaxedGuidText {
    /// The original GUID text.
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The admitted GUID with ASCII letters converted to lowercase.
    pub(crate) fn identity_key(&self) -> cadmpeg_ir::ids::IdentityKey {
        self.0.to_ascii_lowercase()
    }
}

impl TryFrom<String> for DesignRelaxedGuidText {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !crate::bytes::is_guid_relaxed(&value) {
            return Err(
                "GUID must be 36 through 38 alphanumeric, hyphen, or underscore characters".into(),
            );
        }
        cadmpeg_ir::ids::IdentityKey::try_new(value)
            .map(Self)
            .map_err(|error| error.to_string())
    }
}

impl From<DesignRelaxedGuidText> for String {
    fn from(value: DesignRelaxedGuidText) -> Self {
        value.0.into_string()
    }
}

/// One texture resource owned by a Design mesh feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshTextureResource {
    /// Zero-based position in the serialized flags map.
    pub(crate) ordinal: u32,
    /// Stable resource GUID used as the key in both texture maps.
    pub(crate) resource_guid: DesignGuidText,
    /// Opaque resource flags retained without reinterpretation.
    pub(crate) flags: u32,
    /// Zero-based position of the same GUID in the serialized filename map.
    pub(crate) filename_ordinal: u32,
    /// Filename record joined to its archive entry.
    pub(crate) file: DesignMeshTextureFile,
    /// Neutral embedded asset projected from the matching archive entry.
    pub(crate) asset: AssetId,
}

/// A filename record and the archive entry with its exact basename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshTextureFile {
    record: DesignMeshRecordIdentity,
    archive_entry_name: String,
}

impl DesignMeshTextureFile {
    pub(crate) fn new(
        record: DesignMeshRecordIdentity,
        filename: &str,
        archive_entry_name: String,
    ) -> Result<Self, String> {
        let value = Self {
            record,
            archive_entry_name,
        };
        if filename.is_empty() || value.filename() != filename {
            return Err("archive_entry_name basename must match nonempty filename".into());
        }
        let byte_length = u64::try_from(filename.encode_utf16().count())
            .ok()
            .and_then(|units| units.checked_mul(2))
            .and_then(|bytes| {
                bytes.checked_add(u64_from_index(
                    crate::layout::paramesh_texture_filename_prefix::LEN,
                ))
            });
        if byte_length != Some(value.record.frame_length()) {
            return Err("filename_record.frame_length must contain exactly filename".into());
        }
        Ok(value)
    }
    pub(crate) fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub(crate) fn filename(&self) -> &str {
        self.archive_entry_name
            .rsplit_once('/')
            .map_or(self.archive_entry_name.as_str(), |(_, basename)| basename)
    }
    pub(crate) fn archive_entry_name(&self) -> &str {
        &self.archive_entry_name
    }
    fn filename_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_texture_filename_prefix::LEN)
    }
}

/// One texture resource owned by a Design mesh feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignMeshTextureResourceWire {
    /// Zero-based position in the serialized flags map.
    ordinal: u32,
    /// Stable resource GUID used as the key in both texture maps.
    resource_guid: DesignGuidText,
    /// Byte offset of the flags-map GUID payload.
    flags_guid_offset: u64,
    /// Opaque resource flags retained without reinterpretation.
    flags: u32,
    /// Byte offset of `flags`.
    flags_offset: u64,
    /// Zero-based position of the same GUID in the serialized filename map.
    filename_ordinal: u32,
    /// Byte offset of the filename-map GUID payload.
    filename_guid_offset: u64,
    /// Record storing the archive-entry basename.
    filename_record: DesignMeshRecordIdentity,
    /// Byte offset of the filename-record reference.
    filename_record_reference_offset: u64,
    /// Archive-entry basename stored by `filename_record`.
    filename: String,
    /// Byte offset of the UTF-16LE filename code units.
    filename_offset: u64,
    /// Complete matching archive-entry name.
    archive_entry_name: String,
    /// Neutral embedded asset projected from the matching archive entry.
    asset: AssetId,
}

const MESH_TEXTURE_FLAGS_ENTRY_BYTES: u64 = 4 + 36 + 4;

const MESH_TEXTURE_FILENAME_ENTRY_BYTES: u64 = 4 + 36 + 11;

/// Texture resources with complete flags and filename ordinal permutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshTextureTable {
    record: DesignMeshRecordIdentity,
    resources: Vec<DesignMeshTextureResource>,
}

impl DesignMeshTextureTable {
    pub(crate) fn new(
        record: DesignMeshRecordIdentity,
        resources: Vec<DesignMeshTextureResource>,
    ) -> Result<Self, CodecError> {
        Self::new_inner(record, resources).map_err(CodecError::Malformed)
    }

    fn new_inner(
        record: DesignMeshRecordIdentity,
        resources: Vec<DesignMeshTextureResource>,
    ) -> Result<Self, String> {
        let count =
            u32::try_from(resources.len()).map_err(|_| "textures exceeds the u32 map count")?;
        let expected = u64_from_index(crate::layout::paramesh_texture_table_prefix::LEN)
            + 4
            + u64::from(count)
                * (MESH_TEXTURE_FLAGS_ENTRY_BYTES + MESH_TEXTURE_FILENAME_ENTRY_BYTES);
        if record.frame_length() != expected {
            return Err(
                "texture_table_record.frame_length must contain exactly both texture maps".into(),
            );
        }
        let mut flags = std::collections::HashSet::new();
        let mut filenames = std::collections::HashSet::new();
        let mut guids = std::collections::HashSet::new();
        for resource in &resources {
            if resource.ordinal >= count || flags.contains(&resource.ordinal) {
                return Err("textures.ordinal must be a complete map permutation".into());
            }
            if resource.filename_ordinal >= count || filenames.contains(&resource.filename_ordinal)
            {
                return Err("textures.filename_ordinal must be a complete map permutation".into());
            }
            let guid = resource.resource_guid.as_str().to_ascii_uppercase();
            if guids.contains(&guid) {
                return Err("textures.resource_guid must be unique ignoring letter case".into());
            }

            flags.insert(resource.ordinal);
            filenames.insert(resource.filename_ordinal);
            guids.insert(guid);
        }
        Ok(Self { record, resources })
    }
    pub(crate) fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub(crate) fn resources(&self) -> &[DesignMeshTextureResource] {
        &self.resources
    }
    /// Borrow resources in the serialized flags-map order.
    pub(crate) fn resources_in_flags_order(&self) -> Vec<&DesignMeshTextureResource> {
        let mut resources = Vec::new();
        resources.extend(&self.resources);
        resources.sort_by_key(|a| a.ordinal);
        resources
    }
    fn flags_count_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_texture_table_prefix::FLAGS_MAP_COUNT)
    }
    fn filename_count_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_texture_table_prefix::LEN)
            + MESH_TEXTURE_FLAGS_ENTRY_BYTES * u64_from_index(self.resources.len())
    }
    // Output cardinalities are bounded by already-materialized input vectors.
    fn from_wire(
        record: DesignMeshRecordIdentity,
        flags_count_offset: u64,
        filename_count_offset: u64,
        rows: Vec<DesignMeshTextureResourceWire>,
    ) -> Result<Self, String> {
        let count = u32::try_from(rows.len()).map_err(|_| "textures exceeds the u32 map count")?;
        let flags_start = record.byte_offset().checked_add(u64_from_index(
            crate::layout::paramesh_texture_table_prefix::LEN,
        ));
        let filenames_start = flags_start.and_then(|start| {
            start.checked_add(MESH_TEXTURE_FLAGS_ENTRY_BYTES * u64::from(count) + 4)
        });
        let mut resources = {
            let mut storage = Vec::new();
            storage.try_reserve_exact(rows.len()).map(|()| storage)
        }
        .map_err(|error| error.to_string())?;
        for row in rows {
            let flags_guid = flags_start.and_then(|start| {
                start.checked_add(MESH_TEXTURE_FLAGS_ENTRY_BYTES * u64::from(row.ordinal) + 4)
            });
            let filename_guid = filenames_start.and_then(|start| {
                start.checked_add(
                    MESH_TEXTURE_FILENAME_ENTRY_BYTES * u64::from(row.filename_ordinal) + 4,
                )
            });
            if flags_guid != Some(row.flags_guid_offset)
                || flags_guid.and_then(|offset| offset.checked_add(36)) != Some(row.flags_offset)
            {
                return Err(
                    "flags_guid_offset/flags_offset must match the texture map ordinal".into(),
                );
            }
            if filename_guid != Some(row.filename_guid_offset)
                || filename_guid.and_then(|offset| offset.checked_add(36))
                    != Some(row.filename_record_reference_offset)
            {
                return Err("filename_guid_offset/filename_record_reference_offset must match the texture map ordinal".into());
            }
            let file = DesignMeshTextureFile::new(
                row.filename_record,
                &row.filename,
                row.archive_entry_name,
            )?;
            if file.filename_offset() != row.filename_offset {
                return Err("filename_offset must follow filename_record header".into());
            }
            resources.push(DesignMeshTextureResource {
                ordinal: row.ordinal,
                resource_guid: row.resource_guid,
                flags: row.flags,
                filename_ordinal: row.filename_ordinal,
                file,
                asset: row.asset,
            });
        }
        let table = Self::new_inner(record, resources)?;
        if table.flags_count_offset() != flags_count_offset
            || table.filename_count_offset() != filename_count_offset
        {
            return Err("texture_flags_count_offset/texture_filename_count_offset must match the texture maps".into());
        }
        Ok(table)
    }
    #[cfg(test)]
    fn into_wire(
        self,
    ) -> (
        DesignMeshRecordIdentity,
        u64,
        u64,
        Vec<DesignMeshTextureResourceWire>,
    ) {
        let flags_count_offset = self.flags_count_offset();
        let filename_count_offset = self.filename_count_offset();
        let flags_start = self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_texture_table_prefix::LEN);
        let filenames_start = filename_count_offset + 4;
        let rows = self
            .resources
            .into_iter()
            .map(|value| {
                let flags_guid_offset =
                    flags_start + MESH_TEXTURE_FLAGS_ENTRY_BYTES * u64::from(value.ordinal) + 4;
                let filename_guid_offset = filenames_start
                    + MESH_TEXTURE_FILENAME_ENTRY_BYTES * u64::from(value.filename_ordinal)
                    + 4;
                DesignMeshTextureResourceWire {
                    ordinal: value.ordinal,
                    resource_guid: value.resource_guid,
                    flags_guid_offset,
                    flags: value.flags,
                    flags_offset: flags_guid_offset + 36,
                    filename_ordinal: value.filename_ordinal,
                    filename_guid_offset,
                    filename_record_reference_offset: filename_guid_offset + 36,
                    filename: value.file.filename().to_owned(),
                    filename_offset: value.file.filename_offset(),
                    filename_record: value.file.record,
                    archive_entry_name: value.file.archive_entry_name,
                    asset: value.asset,
                }
            })
            .collect();
        (self.record, flags_count_offset, filename_count_offset, rows)
    }
}

/// One finite axis-aligned bound stored by a mesh Scene record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignMeshSceneBounds {
    maximum: [f64; 3],
    minimum: [f64; 3],
}

impl DesignMeshSceneBounds {
    pub(crate) fn new(maximum: [f64; 3], minimum: [f64; 3]) -> Result<Self, String> {
        let maximum = FinitePoint3::new(maximum.into())
            .ok_or("scene bounds maximum and minimum must be finite")?;
        let minimum = FinitePoint3::new(minimum.into())
            .ok_or("scene bounds maximum and minimum must be finite")?;
        Self::from_parts(maximum, minimum)
    }

    pub(crate) fn from_parts(maximum: FinitePoint3, minimum: FinitePoint3) -> Result<Self, String> {
        let maximum = [maximum.x, maximum.y, maximum.z];
        let minimum = [minimum.x, minimum.y, minimum.z];
        if minimum
            .iter()
            .zip(maximum)
            .any(|(minimum, maximum)| *minimum > maximum)
        {
            return Err("scene bounds minimum exceeds maximum".into());
        }
        Ok(Self { maximum, minimum })
    }
    pub(crate) fn maximum(&self) -> [f64; 3] {
        self.maximum
    }
    pub(crate) fn minimum(&self) -> [f64; 3] {
        self.minimum
    }
    fn from_wire(wire: &DesignMeshSceneBoundsWire, offsets: [u64; 2]) -> Result<Self, String> {
        if wire.offsets != offsets {
            return Err("scene bounds offsets must match their owning record".into());
        }
        Self::new(wire.maximum, wire.minimum)
    }
    fn into_wire(self, offsets: [u64; 2]) -> DesignMeshSceneBoundsWire {
        DesignMeshSceneBoundsWire {
            maximum: self.maximum(),
            minimum: self.minimum(),
            offsets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignMeshSceneBoundsWire {
    /// Component-wise upper corner, serialized first.
    maximum: [f64; 3],
    /// Component-wise lower corner, serialized second.
    minimum: [f64; 3],
    /// Byte offsets of the serialized upper and lower corners.
    pub(super) offsets: [u64; 2],
}

/// A fixed Scene-state record and its optional finite bounds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignMeshSceneState {
    record: DesignMeshFixedRecord<95>,
    bounds: Option<DesignMeshSceneBounds>,
}

impl DesignMeshSceneState {
    pub(crate) fn new(
        record: DesignMeshFixedRecord<95>,
        bounds: Option<DesignMeshSceneBounds>,
    ) -> Self {
        Self { record, bounds }
    }
    pub(crate) fn record(&self) -> &DesignMeshFixedRecord<95> {
        &self.record
    }
    fn bounds_offsets(&self) -> [u64; 2] {
        let start = self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_scene_state::FOOTER_MASK);
        [start, start + 24]
    }
    fn from_wire(
        record: DesignMeshRecordIdentity,
        bounds: Option<DesignMeshSceneBoundsWire>,
    ) -> Result<Self, String> {
        let record = DesignMeshFixedRecord::try_from(record)?;
        let mut state = Self::new(record, None);
        state.bounds = bounds
            .map(|bounds| DesignMeshSceneBounds::from_wire(&bounds, state.bounds_offsets()))
            .transpose()?;
        Ok(state)
    }
    fn into_wire(self) -> (DesignMeshRecordIdentity, Option<DesignMeshSceneBoundsWire>) {
        let bounds = self
            .bounds
            .map(|bounds| bounds.into_wire(self.bounds_offsets()));
        (self.record.into(), bounds)
    }
}

/// A compact or placed Scene-node record with bounds at the form's fixed location.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignMeshSceneNode {
    form: DesignMeshSceneNodeForm,
    bounds: Option<DesignMeshSceneBounds>,
}

#[derive(Debug, Clone, PartialEq)]
enum DesignMeshSceneNodeForm {
    Compact(DesignMeshFixedRecord<133>),
    Placed {
        record: DesignMeshFixedRecord<261>,
        transform: MeshAffineTransform,
    },
}

impl DesignMeshSceneNode {
    pub(crate) fn new(
        record: DesignMeshRecordIdentity,
        bounds: Option<DesignMeshSceneBounds>,
        transform: Option<MeshAffineTransform>,
    ) -> Result<Self, String> {
        let form = match transform {
            None => DesignMeshSceneNodeForm::Compact(record.try_into()?),
            Some(transform) => DesignMeshSceneNodeForm::Placed {
                record: record.try_into()?,
                transform,
            },
        };
        Ok(Self { form, bounds })
    }
    pub(crate) fn record_index(&self) -> u32 {
        match &self.form {
            DesignMeshSceneNodeForm::Compact(record) => record.record_index(),
            DesignMeshSceneNodeForm::Placed { record, .. } => record.record_index(),
        }
    }
    fn byte_offset(&self) -> u64 {
        match &self.form {
            DesignMeshSceneNodeForm::Compact(record) => record.byte_offset(),
            DesignMeshSceneNodeForm::Placed { record, .. } => record.byte_offset(),
        }
    }
    pub(crate) fn frame_length(&self) -> u64 {
        match &self.form {
            DesignMeshSceneNodeForm::Compact(_) => {
                u64_from_index(crate::layout::paramesh_scene_node::LEN)
            }
            DesignMeshSceneNodeForm::Placed { .. } => {
                u64_from_index(crate::layout::paramesh_scene_node_placed::LEN)
            }
        }
    }
    pub(crate) fn bounds(&self) -> Option<&DesignMeshSceneBounds> {
        self.bounds.as_ref()
    }
    pub(crate) fn bounds_offsets(&self) -> [u64; 2] {
        let relative = match &self.form {
            DesignMeshSceneNodeForm::Compact(_) => crate::layout::paramesh_scene_node::FOOTER_MASK,
            DesignMeshSceneNodeForm::Placed { .. } => {
                crate::layout::paramesh_scene_node_placed::FOOTER_MASK
            }
        };
        let start = self.byte_offset() + u64_from_index(relative);
        [start, start + 24]
    }
    pub(crate) fn transform(&self) -> Option<Located<MeshAffineTransform>> {
        match &self.form {
            DesignMeshSceneNodeForm::Compact(_) => None,
            DesignMeshSceneNodeForm::Placed { record, transform } => Some(Located {
                value: *transform,
                offset: record.byte_offset()
                    + u64_from_index(crate::layout::paramesh_scene_node_placed::TRANSFORM),
            }),
        }
    }
    fn state_reference_offset(&self) -> u64 {
        self.byte_offset()
            + u64_from_index(crate::layout::paramesh_scene_node::SCENE_STATE_REFERENCE)
    }
    fn auxiliary_reference_offset(&self) -> u64 {
        self.byte_offset()
            + u64_from_index(crate::layout::paramesh_scene_node::AUXILIARY_RECORD_REFERENCE)
    }
    fn from_wire(
        record: DesignMeshRecordIdentity,
        bounds: Option<DesignMeshSceneBoundsWire>,
        transform: Option<Located<MeshAffineTransform>>,
    ) -> Result<Self, String> {
        let mut node = Self::new(record, None, transform.map(|located| located.value))?;
        if node.transform().map(|located| located.offset) != transform.map(|located| located.offset)
        {
            return Err("scene_node_transform_offset must match the placed record layout".into());
        }
        node.bounds = bounds
            .map(|bounds| DesignMeshSceneBounds::from_wire(&bounds, node.bounds_offsets()))
            .transpose()?;
        Ok(node)
    }
    fn into_wire(
        self,
    ) -> (
        DesignMeshRecordIdentity,
        Option<DesignMeshSceneBoundsWire>,
        Option<Located<MeshAffineTransform>>,
    ) {
        let transform = self.transform();
        let bounds = self
            .bounds()
            .copied()
            .map(|bounds| bounds.into_wire(self.bounds_offsets()));
        let record = match self.form {
            DesignMeshSceneNodeForm::Compact(record) => record.into(),
            DesignMeshSceneNodeForm::Placed { record, .. } => record.into(),
        };
        (record, bounds, transform)
    }
}

/// A lowercase RFC 4122 version-4 UUID from the mesh registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct DesignMeshUuid(DesignGuidText);

impl DesignMeshUuid {
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for DesignMeshUuid {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = Self(DesignGuidText::try_from(value)?);
        let bytes = value.as_str().as_bytes();
        if bytes.iter().any(u8::is_ascii_uppercase)
            || bytes[14] != b'4'
            || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        {
            return Err("mesh UUID must be a lowercase version-4 UUID".into());
        }
        Ok(value)
    }
}

impl From<DesignMeshUuid> for String {
    fn from(value: DesignMeshUuid) -> Self {
        value.0.into()
    }
}

/// A container GUID in a record with the complete fixed join prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshGuid {
    record: DesignMeshRecordIdentity,
    value: DesignGuidText,
}

impl DesignMeshGuid {
    pub(crate) fn new(
        record: DesignMeshRecordIdentity,
        value: DesignGuidText,
    ) -> Result<Self, String> {
        if record.frame_length() < u64_from_index(crate::layout::paramesh_guid_join_prefix::LEN) {
            return Err(
                "guid_record.frame_length must contain the complete GUID join prefix".into(),
            );
        }
        Ok(Self { record, value })
    }
    pub(crate) fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub(crate) fn value(&self) -> &str {
        self.value.as_str()
    }
    fn value_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_guid_join_prefix::FUSION_UUID)
            + 4
    }
    fn entry_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_guid_join_prefix::ENTRY_NAME_BACKLINK)
    }
}

/// An entry-name record whose UTF-16 name ends at the record boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshEntryName {
    record: DesignMeshRecordIdentity,
    name: String,
}

impl DesignMeshEntryName {
    pub(crate) fn new(record: DesignMeshRecordIdentity, name: String) -> Result<Self, String> {
        let expected = u64::try_from(name.encode_utf16().count())
            .ok()
            .and_then(|units| units.checked_mul(2))
            .and_then(|bytes| {
                bytes
                    .checked_add(u64_from_index(crate::layout::paramesh_entry_name_prefix::LEN) + 4)
            });
        if name.is_empty() || expected != Some(record.frame_length()) {
            return Err(
                "entry_name_record.frame_length must contain exactly its nonempty entry_name"
                    .into(),
            );
        }
        Ok(Self { record, name })
    }
    pub(crate) fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    fn name_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_entry_name_prefix::LEN)
            + 4
    }
    fn guid_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_entry_name_prefix::GUID_RECORD_REFERENCE)
    }
}

/// Mesh-body placement and the complete prefix plus terminal collection reference.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignMeshPlacement {
    record: DesignMeshRecordIdentity,
    transform: MeshAffineTransform,
}

impl DesignMeshPlacement {
    pub(crate) fn new(
        record: DesignMeshRecordIdentity,
        transform: MeshAffineTransform,
    ) -> Result<Self, String> {
        if record.frame_length()
            < u64_from_index(crate::layout::paramesh_mesh_body_join_prefix::LEN) + 11
        {
            return Err("body_record.frame_length must contain the join prefix and final collection reference".into());
        }
        Ok(Self { record, transform })
    }
    pub(crate) fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub(crate) fn transform(&self) -> MeshAffineTransform {
        self.transform
    }
    fn transform_offsets(&self) -> [u64; 2] {
        [
            self.record.byte_offset()
                + u64_from_index(crate::layout::paramesh_mesh_body_join_prefix::FIRST_TRANSFORM),
            self.record.byte_offset()
                + u64_from_index(crate::layout::paramesh_mesh_body_join_prefix::SECOND_TRANSFORM),
        ]
    }
    fn scope_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_mesh_body_join_prefix::FEATURE_SCOPE_REFERENCE)
    }
    fn wrapper_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_mesh_body_join_prefix::WRAPPER_REFERENCE)
    }
    fn owner_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_mesh_body_join_prefix::BODY_OWNER_REFERENCE)
    }
    fn guid_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(
                crate::layout::paramesh_mesh_body_join_prefix::CONTAINER_GUID_REFERENCE,
            )
    }
    fn scene_node_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_mesh_body_join_prefix::SCENE_NODE_REFERENCE)
    }
    fn collection_reference_offset(&self) -> u64 {
        self.record.byte_offset() + self.record.frame_length() - 11
    }
}

/// One mesh body and its complete Design identity graph.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignMeshBody {
    /// Mesh-body record carrying placement and graph references.
    pub(crate) placement: DesignMeshPlacement,
    /// Entry-name record joining the body to one `.paramesh` archive entry.
    pub(crate) entry: DesignMeshEntryName,
    /// GUID record joining the body to the container's `fusion_uuid`.
    pub(crate) guid: DesignMeshGuid,
    /// One-to-one `ParaMesh` wrapper around the mesh-body record.
    pub(crate) wrapper_record: DesignMeshFixedRecord<40>,
    /// Fixed Scene-state record and optional bounds.
    pub(crate) scene_state: DesignMeshSceneState,
    /// Compact or placed Scene node with its optional bounds.
    pub(crate) scene_node: DesignMeshSceneNode,
    /// Separately typed Scene auxiliary cache reached through the Scene node.
    pub(crate) scene_auxiliary_record: DesignMeshRecordIdentity,
    /// Typed Design body-owner record referenced by the mesh-body record.
    /// Multiple mesh bodies can reference the same owner.
    pub(crate) owner_record: DesignMeshRecordIdentity,
    /// Container-local version-4 mesh UUID from protobuf registry field 12,
    /// when the geometry container joined this Design body.
    pub(crate) container_mesh_uuid: Option<DesignMeshUuid>,
    /// Neutral tessellation projected from the joined container, when present.
    pub(crate) tessellation_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct DesignMeshBodyWire {
    /// Mesh-body record carrying placement and graph references.
    body_record: DesignMeshRecordIdentity,
    /// Entry-name record joining the body to one `.paramesh` archive entry.
    entry_name_record: DesignMeshRecordIdentity,
    /// GUID record joining the body to the container's `fusion_uuid`.
    guid_record: DesignMeshRecordIdentity,
    /// One-to-one `ParaMesh` wrapper around `body_record`.
    wrapper_record: DesignMeshRecordIdentity,
    /// Fixed Scene-state record owned by this mesh body.
    scene_state_record: DesignMeshRecordIdentity,
    /// Finite bound carried by the Scene-state footer; absent for its unset sentinel.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scene_state_bounds"
    )]
    scene_state_bounds: Option<DesignMeshSceneBoundsWire>,
    /// Scene node connecting `body_record` to its state and auxiliary cache.
    scene_node_record: DesignMeshRecordIdentity,
    /// Finite bound carried by the Scene-node footer; absent for its unset sentinel.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scene_node_bounds"
    )]
    scene_node_bounds: Option<DesignMeshSceneBoundsWire>,
    /// Optional row-major affine transform carried by the placed Scene-node form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scene_node_transform"
    )]
    scene_node_transform: Option<MeshAffineTransform>,
    /// Byte offset of `scene_node_transform` when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scene_node_transform_offset"
    )]
    scene_node_transform_offset: Option<u64>,
    /// Separately typed Scene auxiliary cache reached through the Scene node.
    scene_auxiliary_record: DesignMeshRecordIdentity,
    /// Typed Design body-owner record referenced by `body_record`.
    /// Multiple mesh bodies can reference the same owner.
    owner_record: DesignMeshRecordIdentity,
    /// Stored `.paramesh` archive-entry basename.
    entry_name: String,
    /// Byte offset of the UTF-16LE entry-name code units.
    entry_name_offset: u64,
    /// Container identity stored by both Design and `.paramesh` payloads.
    fusion_uuid: DesignGuidText,
    /// Container-local version-4 mesh UUID from protobuf registry field 12,
    /// when the geometry container joined this Design body.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_container_mesh_uuid"
    )]
    container_mesh_uuid: Option<DesignMeshUuid>,
    /// Byte offset of the ASCII `fusion_uuid` payload.
    fusion_uuid_offset: u64,
    /// Equal row-major container-to-model-centimetre affine transform.
    transform: MeshAffineTransform,
    /// Byte offsets of the two equal serialized transform blocks.
    transform_offsets: [u64; 2],
    /// Byte offset of the body-to-feature-scope reference.
    scope_reference_offset: u64,
    /// Byte offset of the body-to-wrapper reference.
    wrapper_reference_offset: u64,
    /// Byte offset of the body-to-owner reference.
    owner_reference_offset: u64,
    /// Byte offset of the body-to-GUID reference.
    guid_reference_offset: u64,
    /// Byte offset of the body-to-Scene-node reference.
    scene_node_reference_offset: u64,
    /// Byte offset of the body's final collection backlink.
    collection_reference_offset: u64,
    /// Byte offset of the wrapper's reciprocal body reference.
    wrapper_body_reference_offset: u64,
    /// Byte offset of the entry-name record's GUID reference.
    entry_guid_reference_offset: u64,
    /// Byte offset of the GUID record's entry-name backlink.
    guid_entry_reference_offset: u64,
    /// Byte offset of the Scene node's state-record reference.
    scene_state_reference_offset: u64,
    /// Byte offset of the Scene node's auxiliary-record reference.
    scene_auxiliary_reference_offset: u64,
    /// Neutral tessellation projected from the joined container, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tessellation_id"
    )]
    tessellation_id: Option<String>,
}

impl From<DesignMeshBody> for DesignMeshBodyWire {
    fn from(value: DesignMeshBody) -> Self {
        let wrapper_body_reference_offset = value.wrapper_body_reference_offset();
        let fusion_uuid_offset = value.guid.value_offset();
        let guid_entry_reference_offset = value.guid.entry_reference_offset();
        let entry_name_offset = value.entry.name_offset();
        let entry_guid_reference_offset = value.entry.guid_reference_offset();
        let transform_offsets = value.placement.transform_offsets();
        let scope_reference_offset = value.placement.scope_reference_offset();
        let wrapper_reference_offset = value.placement.wrapper_reference_offset();
        let owner_reference_offset = value.placement.owner_reference_offset();
        let guid_reference_offset = value.placement.guid_reference_offset();
        let scene_node_reference_offset = value.placement.scene_node_reference_offset();
        let collection_reference_offset = value.placement.collection_reference_offset();
        let scene_state_reference_offset = value.scene_node.state_reference_offset();
        let scene_auxiliary_reference_offset = value.scene_node.auxiliary_reference_offset();
        let (scene_state_record, scene_state_bounds) = value.scene_state.into_wire();
        let (scene_node_record, scene_node_bounds, scene_node_transform) =
            value.scene_node.into_wire();
        Self {
            body_record: value.placement.record,
            entry_name_record: value.entry.record,
            guid_record: value.guid.record,
            wrapper_record: value.wrapper_record.into(),
            scene_state_record,
            scene_state_bounds,
            scene_node_record,
            scene_node_bounds,
            scene_node_transform: scene_node_transform.map(|located| located.value),
            scene_node_transform_offset: scene_node_transform.map(|located| located.offset),
            scene_auxiliary_record: value.scene_auxiliary_record,
            owner_record: value.owner_record,
            entry_name: value.entry.name,
            entry_name_offset,
            fusion_uuid: value.guid.value,
            container_mesh_uuid: value.container_mesh_uuid,
            fusion_uuid_offset,
            transform: value.placement.transform,
            transform_offsets,
            scope_reference_offset,
            wrapper_reference_offset,
            owner_reference_offset,
            guid_reference_offset,
            scene_node_reference_offset,
            collection_reference_offset,
            wrapper_body_reference_offset,
            entry_guid_reference_offset,
            guid_entry_reference_offset,
            scene_state_reference_offset,
            scene_auxiliary_reference_offset,
            tessellation_id: value.tessellation_id,
        }
    }
}

impl DesignMeshBody {
    fn wrapper_body_reference_offset(&self) -> u64 {
        self.wrapper_record.byte_offset()
            + u64_from_index(crate::layout::paramesh_body_wrapper::BODY_REFERENCE)
    }
    fn from_wire(value: DesignMeshBodyWire) -> Result<Self, String> {
        let wrapper_record = DesignMeshFixedRecord::try_from(value.wrapper_record)?;
        if wrapper_record.byte_offset()
            + u64_from_index(crate::layout::paramesh_body_wrapper::BODY_REFERENCE)
            != value.wrapper_body_reference_offset
        {
            return Err("wrapper_body_reference_offset must match wrapper_record layout".into());
        }
        let scene_state =
            DesignMeshSceneState::from_wire(value.scene_state_record, value.scene_state_bounds)?;
        let scene_node = DesignMeshSceneNode::from_wire(
            value.scene_node_record,
            value.scene_node_bounds,
            Located::from_wire(
                value.scene_node_transform,
                value.scene_node_transform_offset,
                "scene_node_transform",
            )?,
        )?;
        if scene_node.state_reference_offset() != value.scene_state_reference_offset
            || scene_node.auxiliary_reference_offset() != value.scene_auxiliary_reference_offset
        {
            return Err("scene_state_reference_offset/scene_auxiliary_reference_offset must match scene_node_record layout".into());
        }
        let guid = DesignMeshGuid::new(value.guid_record, value.fusion_uuid)?;
        if value.fusion_uuid_offset != guid.value_offset()
            || value.guid_entry_reference_offset != guid.entry_reference_offset()
        {
            return Err(
                "fusion_uuid_offset/guid_entry_reference_offset must match guid_record layout"
                    .into(),
            );
        }
        let entry = DesignMeshEntryName::new(value.entry_name_record, value.entry_name)?;
        if entry.name_offset() != value.entry_name_offset
            || entry.guid_reference_offset() != value.entry_guid_reference_offset
        {
            return Err(
                "entry_name_offset/entry_guid_reference_offset must match entry_name_record layout"
                    .into(),
            );
        }
        let placement = DesignMeshPlacement::new(value.body_record, value.transform)?;
        if placement.transform_offsets() != value.transform_offsets {
            return Err("transform_offsets must match body_record layout".into());
        }
        if placement.scope_reference_offset() != value.scope_reference_offset {
            return Err("scope_reference_offset must match body_record layout".into());
        }
        if placement.wrapper_reference_offset() != value.wrapper_reference_offset {
            return Err("wrapper_reference_offset must match body_record layout".into());
        }
        if placement.owner_reference_offset() != value.owner_reference_offset {
            return Err("owner_reference_offset must match body_record layout".into());
        }
        if placement.guid_reference_offset() != value.guid_reference_offset {
            return Err("guid_reference_offset must match body_record layout".into());
        }
        if placement.scene_node_reference_offset() != value.scene_node_reference_offset {
            return Err("scene_node_reference_offset must match body_record layout".into());
        }
        if placement.collection_reference_offset() != value.collection_reference_offset {
            return Err("collection_reference_offset must match body_record layout".into());
        }
        Ok(Self {
            placement,
            entry,
            guid,
            wrapper_record,
            scene_state,
            scene_node,
            scene_auxiliary_record: value.scene_auxiliary_record,
            owner_record: value.owner_record,
            container_mesh_uuid: value.container_mesh_uuid,
            tessellation_id: value.tessellation_id,
        })
    }
}

/// A finite, nonsingular row-major affine map. The IR transform holds the
/// three affine rows; the stored bottom row keeps the source spelling of the
/// constant row, including the sign of each zero.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct MeshAffineTransform {
    affine: Transform,
    bottom: [f64; 4],
}

impl MeshAffineTransform {
    /// Check finite coefficients, an affine last row, and a nonzero finite determinant.
    pub(crate) fn new(cells: [f64; 16]) -> Result<Self, String> {
        let rows: [[f64; 4]; 4] = std::array::from_fn(|row| {
            [
                cells[4 * row],
                cells[4 * row + 1],
                cells[4 * row + 2],
                cells[4 * row + 3],
            ]
        });
        let affine = (rows[3] == [0.0, 0.0, 0.0, 1.0])
            .then(|| Transform::affine([rows[0], rows[1], rows[2]]))
            .flatten()
            .ok_or("transform must be finite and affine")?;
        let determinant = rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
            - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
            + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]);
        if !determinant.is_finite() || determinant == 0.0 {
            return Err("transform must have a nonzero finite determinant".into());
        }
        Ok(Self {
            affine,
            bottom: rows[3],
        })
    }

    /// Row-major coefficients.
    pub(crate) fn cells(self) -> [f64; 16] {
        let rows = self.rows();
        std::array::from_fn(|cell| rows[cell / 4][cell % 4])
    }

    /// The affine map. Admission checked every coefficient, so no second
    /// admission is needed.
    pub(crate) fn transform(self) -> Transform {
        self.affine
    }

    /// Four row-major rows.
    fn rows(self) -> [[f64; 4]; 4] {
        let [first, second, third] = self.affine.affine_rows();
        [first, second, third, self.bottom]
    }
}

impl TryFrom<[[f64; 4]; 4]> for MeshAffineTransform {
    type Error = String;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        Self::new(std::array::from_fn(|i| rows[i / 4][i % 4]))
    }
}

impl From<MeshAffineTransform> for [[f64; 4]; 4] {
    fn from(value: MeshAffineTransform) -> Self {
        value.rows()
    }
}

/// Collection-owner record with a fixed-prefix or terminal backlink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshCollectionOwner {
    record: DesignMeshRecordIdentity,
    backlink: DesignMeshCollectionBacklink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesignMeshCollectionBacklink {
    Fixed241,
    Fixed262,
    Terminal,
}

impl DesignMeshCollectionOwner {
    pub(crate) fn new(
        record: DesignMeshRecordIdentity,
        backlink_offset: u64,
    ) -> Result<Self, String> {
        let relative = backlink_offset
            .checked_sub(record.byte_offset())
            .ok_or("collection_owner_backlink_offset precedes its record")?;
        let backlink = if relative
            == u64_from_index(crate::layout::paramesh_collection_owner_v17::COLLECTION_BACKLINK)
            && record.frame_length()
                >= u64_from_index(crate::layout::paramesh_collection_owner_v17::LEN)
        {
            DesignMeshCollectionBacklink::Fixed241
        } else if relative
            == u64_from_index(
                crate::layout::paramesh_collection_owner_backlink_prefix::COLLECTION_BACKLINK,
            )
            && record.frame_length()
                >= u64_from_index(crate::layout::paramesh_collection_owner_backlink_prefix::LEN)
        {
            DesignMeshCollectionBacklink::Fixed262
        } else if relative == record.frame_length() - 11 {
            DesignMeshCollectionBacklink::Terminal
        } else {
            return Err("collection_owner_backlink_offset must identify a complete fixed-prefix or terminal reference".into());
        };
        Ok(Self { record, backlink })
    }
    pub(crate) fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub(crate) fn backlink_offset(&self) -> u64 {
        let relative = match self.backlink {
            DesignMeshCollectionBacklink::Fixed241 => {
                u64_from_index(crate::layout::paramesh_collection_owner_v17::COLLECTION_BACKLINK)
            }
            DesignMeshCollectionBacklink::Fixed262 => u64_from_index(
                crate::layout::paramesh_collection_owner_backlink_prefix::COLLECTION_BACKLINK,
            ),
            DesignMeshCollectionBacklink::Terminal => self.record.frame_length() - 11,
        };
        self.record.byte_offset() + relative
    }
}

/// Feature-scope record with its same-index closing base and owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshScope {
    record: DesignMeshRecordIdentity,
    base_class_tag: DesignClassTag,
    owner_record_index: std::num::NonZeroU32,
}

impl DesignMeshScope {
    pub(crate) fn new(
        record: DesignMeshRecordIdentity,
        base_record: DesignMeshRecordIdentity,
        owner_record_index: u32,
    ) -> Result<Self, String> {
        let base_length = u64_from_index(crate::layout::paramesh_feature_scope_base::LEN);
        if record.frame_length()
            < u64_from_index(crate::layout::paramesh_feature_scope_prefix::LEN) + base_length
        {
            return Err(
                "scope_record.frame_length() must contain the prefix and closing base".into(),
            );
        }
        if base_record.record_index() != record.record_index()
            || base_record.frame_length() != base_length
            || base_record.byte_offset()
                != record.byte_offset() + record.frame_length() - base_length
        {
            return Err(
                "scope_base_record must be the same-index closing base of scope_record".into(),
            );
        }
        let owner_record_index = std::num::NonZeroU32::new(owner_record_index)
            .ok_or("scope_owner_record_index must be nonzero")?;
        Ok(Self {
            record,
            base_class_tag: base_record.class_tag,
            owner_record_index,
        })
    }
    pub(crate) fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    pub(crate) fn base_record(&self) -> DesignMeshRecordIdentity {
        let frame_length = u64_from_index(crate::layout::paramesh_feature_scope_base::LEN);
        DesignMeshRecordIdentity {
            class_tag: self.base_class_tag.clone(),
            record_index: self.record.record_index,
            byte_offset: self.record.byte_offset() + self.record.frame_length() - frame_length,
            frame_length,
        }
    }
    fn owner_record_index(&self) -> u32 {
        self.owner_record_index.get()
    }
    fn owner_reference_offset(&self) -> u64 {
        self.record.byte_offset() + self.record.frame_length()
            - u64_from_index(crate::layout::paramesh_feature_scope_base::LEN)
            + u64_from_index(crate::layout::paramesh_feature_scope_base::SCOPE_OWNER_REFERENCE)
    }
}

/// Mesh collection with its same-index nested base and complete body-reference run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshCollection {
    record: DesignMeshRecordIdentity,
    base_class_tag: DesignClassTag,
}

impl DesignMeshCollection {
    pub(crate) fn new(
        record: DesignMeshRecordIdentity,
        base_record: DesignMeshRecordIdentity,
    ) -> Result<Self, String> {
        let prefix = u64_from_index(crate::layout::paramesh_mesh_collection_prefix::LEN);
        let fixed_length =
            prefix + u64_from_index(crate::layout::paramesh_mesh_collection_base_prefix::LEN) + 11;
        let body_bytes = record.frame_length().checked_sub(fixed_length).ok_or(
            "collection_record.frame_length must contain both prefixes and the owner reference",
        )?;
        if body_bytes % 11 != 0 || u32::try_from(body_bytes / 11).is_err() {
            return Err(
                "collection_record.frame_length must contain a u32-counted body-reference run"
                    .into(),
            );
        }
        if base_record.record_index() != record.record_index()
            || base_record.byte_offset() != record.byte_offset() + prefix
            || base_record.frame_length() != record.frame_length() - prefix
        {
            return Err(
                "collection_base_record must be the same-index nested base of collection_record"
                    .into(),
            );
        }
        Ok(Self {
            record,
            base_class_tag: base_record.class_tag,
        })
    }
    pub(crate) fn record(&self) -> &DesignMeshRecordIdentity {
        &self.record
    }
    #[cfg(test)]
    fn base_record(&self) -> DesignMeshRecordIdentity {
        let prefix = u64_from_index(crate::layout::paramesh_mesh_collection_prefix::LEN);
        DesignMeshRecordIdentity {
            class_tag: self.base_class_tag.clone(),
            record_index: self.record.record_index,
            byte_offset: self.record.byte_offset() + prefix,
            frame_length: self.record.frame_length() - prefix,
        }
    }
    fn body_count(&self) -> u64 {
        (self.record.frame_length()
            - u64_from_index(crate::layout::paramesh_mesh_collection_prefix::LEN)
            - u64_from_index(crate::layout::paramesh_mesh_collection_base_prefix::LEN)
            - 11)
            / 11
    }
    fn texture_table_reference_offset(&self) -> u64 {
        self.record.byte_offset()
            + u64_from_index(
                crate::layout::paramesh_mesh_collection_prefix::TEXTURE_TABLE_REFERENCE,
            )
    }
    fn owner_reference_offset(&self) -> u64 {
        self.record.byte_offset() + self.record.frame_length() - 11
    }
}

/// One complete `Base Mesh Feature` Design graph.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignMeshFeatureWire")]
pub(crate) struct DesignMeshFeature {
    #[cfg(test)]
    clone_probe: MeshFeatureCloneProbe,
    /// Globally unique deterministic identity keyed by the feature-scope record.
    pub(crate) id: String,
    /// Feature scope and its closing owner reference.
    scope: DesignMeshScope,
    /// Mesh collection and its nested base.
    collection: DesignMeshCollection,
    /// Typed `ParaMesh` texture-table record owned by the collection.
    pub(crate) texture_table: DesignMeshTextureTable,
    /// Typed Design owner of the mesh-body collection.
    pub(crate) collection_owner: DesignMeshCollectionOwner,
    /// Mesh bodies in the source collection order.
    bodies: Vec<DesignMeshBody>,
}

#[cfg(test)]
thread_local! {
    static MESH_FEATURE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct MeshFeatureCloneProbe;

#[cfg(test)]
impl Clone for MeshFeatureCloneProbe {
    fn clone(&self) -> Self {
        MESH_FEATURE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self
    }
}

impl DesignMeshFeature {
    pub(crate) fn new(
        id: String,
        scope: DesignMeshScope,
        collection: DesignMeshCollection,
        texture_table: DesignMeshTextureTable,
        collection_owner: DesignMeshCollectionOwner,
        bodies: Vec<DesignMeshBody>,
    ) -> Result<Self, String> {
        let body_count = u32::try_from(bodies.len()).map_err(|_| "bodies count must fit u32")?;
        if collection.body_count() != u64::from(body_count) {
            return Err("bodies count must match collection_record.frame_length".into());
        }
        let scope_body_bytes = u64::from(body_count) * 11;
        let scope_prefix = u64_from_index(crate::layout::paramesh_feature_scope_prefix::LEN);
        let scope_base_length = u64_from_index(crate::layout::paramesh_feature_scope_base::LEN);
        if scope_body_bytes > scope.record().frame_length() - scope_prefix - scope_base_length {
            return Err("bodies reference run must end before scope_base_record".into());
        }
        Ok(Self {
            #[cfg(test)]
            clone_probe: MeshFeatureCloneProbe,
            id,
            scope,
            collection,
            texture_table,
            collection_owner,
            bodies,
        })
    }
    pub(crate) fn scope(&self) -> &DesignMeshScope {
        &self.scope
    }
    pub(crate) fn collection(&self) -> &DesignMeshCollection {
        &self.collection
    }
    pub(crate) fn bodies(&self) -> &[DesignMeshBody] {
        &self.bodies
    }
    pub(crate) fn bodies_mut(&mut self) -> &mut [DesignMeshBody] {
        &mut self.bodies
    }
    fn body_count_offsets(&self) -> [u64; 3] {
        [
            self.scope.record().byte_offset()
                + u64_from_index(crate::layout::paramesh_feature_scope_prefix::BODY_COUNT),
            self.collection.record().byte_offset()
                + u64_from_index(crate::layout::paramesh_mesh_collection_prefix::BODY_COUNT),
            self.collection.record().byte_offset()
                + u64_from_index(crate::layout::paramesh_mesh_collection_prefix::LEN)
                + u64_from_index(crate::layout::paramesh_mesh_collection_base_prefix::BODY_COUNT),
        ]
    }
    fn scope_body_reference_offsets(&self) -> impl Iterator<Item = u64> + '_ {
        let start = self.scope.record().byte_offset()
            + u64_from_index(crate::layout::paramesh_feature_scope_prefix::LEN);
        (0..self.collection.body_count()).map(move |ordinal| start + 11 * ordinal)
    }
    fn collection_body_reference_offsets(&self) -> impl Iterator<Item = u64> + '_ {
        let start = self.collection.record().byte_offset()
            + u64_from_index(crate::layout::paramesh_mesh_collection_prefix::LEN)
            + u64_from_index(crate::layout::paramesh_mesh_collection_base_prefix::LEN);
        (0..self.collection.body_count()).map(move |ordinal| start + 11 * ordinal)
    }
}

/// One complete `Base Mesh Feature` Design graph.
#[derive(Serialize, Deserialize)]
struct DesignMeshFeatureWire {
    /// Globally unique deterministic identity keyed by the feature-scope record.
    id: String,
    /// Typed `Base Mesh Feature` scope record.
    scope_record: DesignMeshRecordIdentity,
    /// Paired same-index base record closing the feature scope.
    scope_base_record: DesignMeshRecordIdentity,
    /// Typed `ParaMesh` body-collection record.
    collection_record: DesignMeshRecordIdentity,
    /// Paired same-index base record inside the collection.
    collection_base_record: DesignMeshRecordIdentity,
    /// Typed `ParaMesh` texture-table record owned by the collection.
    texture_table_record: DesignMeshRecordIdentity,
    /// Three equal body counts: scope, collection prefix, collection base.
    body_count_offsets: [u64; 3],
    /// Ordered mesh-body record identities owned by the feature.
    body_record_indices: Vec<u32>,
    /// Scope body-reference offsets parallel to `body_record_indices`.
    scope_body_reference_offsets: Vec<u64>,
    /// Collection body-reference offsets parallel to `body_record_indices`.
    collection_body_reference_offsets: Vec<u64>,
    /// Byte offset of the collection's texture-table reference.
    texture_table_reference_offset: u64,
    /// Typed Design owner of the mesh-body collection.
    collection_owner_record: DesignMeshRecordIdentity,
    /// Byte offset of the collection's owner reference.
    collection_owner_reference_offset: u64,
    /// Byte offset of the owner's reciprocal collection reference.
    collection_owner_backlink_offset: u64,
    /// Design owner of the feature scope.
    scope_owner_record_index: u32,
    /// Byte offset of the paired scope record's owner reference.
    scope_owner_reference_offset: u64,
    /// Byte offset of the texture flags-map count.
    texture_flags_count_offset: u64,
    /// Byte offset of the texture filename-map count.
    texture_filename_count_offset: u64,
    /// Mesh bodies in the source collection order.
    bodies: Vec<DesignMeshBodyWire>,
    /// Texture resources in flags-map order.
    textures: Vec<DesignMeshTextureResourceWire>,
}

impl TryFrom<DesignMeshFeatureWire> for DesignMeshFeature {
    type Error = String;
    fn try_from(wire: DesignMeshFeatureWire) -> Result<Self, Self::Error> {
        if wire.scope_body_reference_offsets.len() != wire.bodies.len() {
            return Err("scope_body_reference_offsets must match bodies".into());
        }
        if wire.collection_body_reference_offsets.len() != wire.bodies.len() {
            return Err("collection_body_reference_offsets must match bodies".into());
        }
        if !wire.body_record_indices.iter().copied().eq(wire
            .bodies
            .iter()
            .map(|body| body.body_record.record_index()))
        {
            return Err(
                "body_record_indices must repeat bodies.body_record.record_index in order".into(),
            );
        }
        let mut bodies = {
            let mut storage = Vec::new();
            storage
                .try_reserve_exact(wire.bodies.len())
                .map(|()| storage)
        }
        .map_err(|error| error.to_string())?;
        for body in wire.bodies {
            bodies.push(DesignMeshBody::from_wire(body)?);
        }
        let scope = DesignMeshScope::new(
            wire.scope_record,
            wire.scope_base_record,
            wire.scope_owner_record_index,
        )?;
        if scope.owner_reference_offset() != wire.scope_owner_reference_offset {
            return Err(
                "scope_owner_reference_offset must locate the closing base owner reference".into(),
            );
        }
        let collection =
            DesignMeshCollection::new(wire.collection_record, wire.collection_base_record)?;
        if collection.texture_table_reference_offset() != wire.texture_table_reference_offset {
            return Err(
                "texture_table_reference_offset must locate the collection texture-table reference"
                    .into(),
            );
        }
        if collection.owner_reference_offset() != wire.collection_owner_reference_offset {
            return Err("collection_owner_reference_offset must locate the terminal collection owner reference".into());
        }
        let feature = Self::new(
            wire.id,
            scope,
            collection,
            DesignMeshTextureTable::from_wire(
                wire.texture_table_record,
                wire.texture_flags_count_offset,
                wire.texture_filename_count_offset,
                wire.textures,
            )?,
            DesignMeshCollectionOwner::new(
                wire.collection_owner_record,
                wire.collection_owner_backlink_offset,
            )?,
            bodies,
        )?;
        if wire.body_count_offsets != feature.body_count_offsets() {
            return Err("body_count_offsets must locate the scope and collection counts".into());
        }
        if !wire
            .scope_body_reference_offsets
            .into_iter()
            .eq(feature.scope_body_reference_offsets())
        {
            return Err(
                "scope_body_reference_offsets must locate the ordered scope body references".into(),
            );
        }
        if !wire
            .collection_body_reference_offsets
            .into_iter()
            .eq(feature.collection_body_reference_offsets())
        {
            return Err("collection_body_reference_offsets must locate the ordered collection body references".into());
        }
        Ok(feature)
    }
}

#[cfg(test)]
impl From<DesignMeshFeature> for DesignMeshFeatureWire {
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn from(value: DesignMeshFeature) -> Self {
        let mut body_record_indices = Vec::with_capacity(value.bodies.len());
        let body_count_offsets = value.body_count_offsets();
        let scope_body_reference_offsets = value.scope_body_reference_offsets().collect();
        let collection_body_reference_offsets = value.collection_body_reference_offsets().collect();
        let mut bodies = Vec::with_capacity(value.bodies.len());
        for body in value.bodies {
            body_record_indices.push(body.placement.record().record_index());
            bodies.push(body.into());
        }
        let collection_owner_backlink_offset = value.collection_owner.backlink_offset();
        let (
            texture_table_record,
            texture_flags_count_offset,
            texture_filename_count_offset,
            textures,
        ) = value.texture_table.into_wire();
        Self {
            body_record_indices,
            scope_body_reference_offsets,
            collection_body_reference_offsets,
            bodies,
            id: value.id,
            scope_record: value.scope.record().clone(),
            scope_base_record: value.scope.base_record(),
            collection_record: value.collection.record().clone(),
            collection_base_record: value.collection.base_record(),
            texture_table_record,
            body_count_offsets,
            texture_table_reference_offset: value.collection.texture_table_reference_offset(),
            collection_owner_record: value.collection_owner.record,
            collection_owner_reference_offset: value.collection.owner_reference_offset(),
            collection_owner_backlink_offset,
            scope_owner_record_index: value.scope.owner_record_index(),
            scope_owner_reference_offset: value.scope.owner_reference_offset(),
            texture_flags_count_offset,
            texture_filename_count_offset,
            textures,
        }
    }
}

/// Exact identity and source extent of one indexed Design mesh record.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignMeshRecordIdentityWire")]
pub(crate) struct DesignMeshRecordIdentity {
    class_tag: DesignClassTag,
    record_index: std::num::NonZeroU32,
    byte_offset: u64,
    frame_length: u64,
}

#[cfg(test)]
thread_local! {
    static MESH_RECORD_IDENTITY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignMeshRecordIdentity {
    fn clone(&self) -> Self {
        MESH_RECORD_IDENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            frame_length: self.frame_length,
        }
    }
}

impl DesignMeshRecordIdentity {
    pub(crate) fn new(
        class_tag: DesignClassTag,
        record_index: u32,
        byte_offset: u64,
        frame_length: u64,
    ) -> Result<Self, String> {
        let record_index =
            std::num::NonZeroU32::new(record_index).ok_or("mesh record_index must be nonzero")?;
        if frame_length < 11 {
            return Err("mesh frame_length must contain the indexed header".into());
        }
        byte_offset
            .checked_add(frame_length)
            .ok_or("mesh frame_length overflows byte_offset")?;
        Ok(Self {
            class_tag,
            record_index,
            byte_offset,
            frame_length,
        })
    }
    pub(crate) fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index.get()
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    fn frame_length(&self) -> u64 {
        self.frame_length
    }
}

/// Exact identity and source extent of one indexed Design mesh record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignMeshRecordIdentityWire {
    /// Source per-file dynamic three-digit ASCII class tag.
    class_tag: String,
    /// Stream-local indexed-record identity.
    record_index: u32,
    /// Byte offset of the indexed header in the Design `BulkStream`.
    byte_offset: u64,
    /// Complete primary or nested record length in bytes.
    frame_length: u64,
}

impl Serialize for DesignMeshRecordIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            class_tag: &'a str,
            record_index: u32,
            byte_offset: u64,
            frame_length: u64,
        }
        WireRef {
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index.get(),
            byte_offset: self.byte_offset,
            frame_length: self.frame_length,
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignMeshRecordIdentityWire> for DesignMeshRecordIdentity {
    type Error = String;
    fn try_from(wire: DesignMeshRecordIdentityWire) -> Result<Self, Self::Error> {
        Self::new(
            DesignClassTag::try_from(wire.class_tag)?,
            wire.record_index,
            wire.byte_offset,
            wire.frame_length,
        )
    }
}

#[cfg(test)]
impl From<DesignMeshRecordIdentity> for DesignMeshRecordIdentityWire {
    fn from(record: DesignMeshRecordIdentity) -> Self {
        Self {
            class_tag: record.class_tag.into(),
            record_index: record.record_index.get(),
            byte_offset: record.byte_offset,
            frame_length: record.frame_length,
        }
    }
}

/// An indexed mesh record with a fixed byte length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignMeshFixedRecord<const LENGTH: u64> {
    class_tag: DesignClassTag,
    record_index: std::num::NonZeroU32,
    byte_offset: u64,
}

impl<const LENGTH: u64> DesignMeshFixedRecord<LENGTH> {
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index.get()
    }
    fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
}

impl<const LENGTH: u64> TryFrom<DesignMeshRecordIdentity> for DesignMeshFixedRecord<LENGTH> {
    type Error = String;
    fn try_from(record: DesignMeshRecordIdentity) -> Result<Self, Self::Error> {
        if record.frame_length() != LENGTH {
            return Err(format!("record.frame_length must be {LENGTH}"));
        }
        Ok(Self {
            class_tag: record.class_tag,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
        })
    }
}

impl<const LENGTH: u64> From<DesignMeshFixedRecord<LENGTH>> for DesignMeshRecordIdentity {
    fn from(record: DesignMeshFixedRecord<LENGTH>) -> Self {
        Self {
            class_tag: record.class_tag,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            frame_length: LENGTH,
        }
    }
}

#[cfg(test)]
mod tests;

mod serialize;

mod identity_rewrite;
