// SPDX-License-Identifier: Apache-2.0
//! Borrowed native wire for the complete Design mesh feature graph.

use cadmpeg_core::decode::u64_from_index;

use super::{
    DesignMeshBody, DesignMeshCollection, DesignMeshFeature, DesignMeshFixedRecord,
    DesignMeshRecordIdentity, DesignMeshSceneBoundsWire, DesignMeshSceneNodeForm, DesignMeshScope,
    DesignMeshTextureResource, DesignMeshTextureTable, DesignMeshUuid, MeshAffineTransform,
    MESH_TEXTURE_FILENAME_ENTRY_BYTES, MESH_TEXTURE_FLAGS_ENTRY_BYTES,
};
use crate::records::serde_column::SliceColumn;
use serde::Serialize;

#[derive(Serialize)]
struct RecordRef<'a> {
    class_tag: &'a str,
    record_index: u32,
    byte_offset: u64,
    frame_length: u64,
}

impl<'a> RecordRef<'a> {
    fn from_identity(record: &'a DesignMeshRecordIdentity) -> Self {
        Self {
            class_tag: record.class_tag.as_str(),
            record_index: record.record_index.get(),
            byte_offset: record.byte_offset,
            frame_length: record.frame_length,
        }
    }

    fn from_fixed<const LENGTH: u64>(record: &'a DesignMeshFixedRecord<LENGTH>) -> Self {
        Self {
            class_tag: record.class_tag.as_str(),
            record_index: record.record_index.get(),
            byte_offset: record.byte_offset,
            frame_length: LENGTH,
        }
    }

    fn scope_base(scope: &'a DesignMeshScope) -> Self {
        let frame_length = u64_from_index(crate::layout::paramesh_feature_scope_base::LEN);
        Self {
            class_tag: scope.base_class_tag.as_str(),
            record_index: scope.record.record_index.get(),
            byte_offset: scope.record.byte_offset + scope.record.frame_length - frame_length,
            frame_length,
        }
    }

    fn collection_base(collection: &'a DesignMeshCollection) -> Self {
        let prefix = u64_from_index(crate::layout::paramesh_mesh_collection_prefix::LEN);
        Self {
            class_tag: collection.base_class_tag.as_str(),
            record_index: collection.record.record_index.get(),
            byte_offset: collection.record.byte_offset + prefix,
            frame_length: collection.record.frame_length - prefix,
        }
    }
}

struct BodyRef<'a>(&'a DesignMeshBody);

impl Serialize for BodyRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            body_record: RecordRef<'a>,
            entry_name_record: RecordRef<'a>,
            guid_record: RecordRef<'a>,
            wrapper_record: RecordRef<'a>,
            scene_state_record: RecordRef<'a>,
            #[serde(skip_serializing_if = "Option::is_none")]
            scene_state_bounds: Option<DesignMeshSceneBoundsWire>,
            scene_node_record: RecordRef<'a>,
            #[serde(skip_serializing_if = "Option::is_none")]
            scene_node_bounds: Option<DesignMeshSceneBoundsWire>,
            #[serde(skip_serializing_if = "Option::is_none")]
            scene_node_transform: Option<MeshAffineTransform>,
            #[serde(skip_serializing_if = "Option::is_none")]
            scene_node_transform_offset: Option<u64>,
            scene_auxiliary_record: RecordRef<'a>,
            owner_record: RecordRef<'a>,
            entry_name: &'a str,
            entry_name_offset: u64,
            fusion_uuid: &'a super::DesignGuidText,
            #[serde(skip_serializing_if = "Option::is_none")]
            container_mesh_uuid: Option<&'a DesignMeshUuid>,
            fusion_uuid_offset: u64,
            transform: MeshAffineTransform,
            transform_offsets: [u64; 2],
            scope_reference_offset: u64,
            wrapper_reference_offset: u64,
            owner_reference_offset: u64,
            guid_reference_offset: u64,
            scene_node_reference_offset: u64,
            collection_reference_offset: u64,
            wrapper_body_reference_offset: u64,
            entry_guid_reference_offset: u64,
            guid_entry_reference_offset: u64,
            scene_state_reference_offset: u64,
            scene_auxiliary_reference_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            tessellation_id: Option<&'a String>,
        }
        let body = self.0;
        let node_record = match &body.scene_node.form {
            DesignMeshSceneNodeForm::Compact(record) => RecordRef::from_fixed(record),
            DesignMeshSceneNodeForm::Placed { record, .. } => RecordRef::from_fixed(record),
        };
        let node_transform = body.scene_node.transform();
        Wire {
            body_record: RecordRef::from_identity(&body.placement.record),
            entry_name_record: RecordRef::from_identity(&body.entry.record),
            guid_record: RecordRef::from_identity(&body.guid.record),
            wrapper_record: RecordRef::from_fixed(&body.wrapper_record),
            scene_state_record: RecordRef::from_fixed(&body.scene_state.record),
            scene_state_bounds: body
                .scene_state
                .bounds
                .map(|bounds| bounds.into_wire(body.scene_state.bounds_offsets())),
            scene_node_record: node_record,
            scene_node_bounds: body
                .scene_node
                .bounds
                .map(|bounds| bounds.into_wire(body.scene_node.bounds_offsets())),
            scene_node_transform: node_transform.map(|located| located.value),
            scene_node_transform_offset: node_transform.map(|located| located.offset),
            scene_auxiliary_record: RecordRef::from_identity(&body.scene_auxiliary_record),
            owner_record: RecordRef::from_identity(&body.owner_record),
            entry_name: &body.entry.name,
            entry_name_offset: body.entry.name_offset(),
            fusion_uuid: &body.guid.value,
            container_mesh_uuid: body.container_mesh_uuid.as_ref(),
            fusion_uuid_offset: body.guid.value_offset(),
            transform: body.placement.transform,
            transform_offsets: body.placement.transform_offsets(),
            scope_reference_offset: body.placement.scope_reference_offset(),
            wrapper_reference_offset: body.placement.wrapper_reference_offset(),
            owner_reference_offset: body.placement.owner_reference_offset(),
            guid_reference_offset: body.placement.guid_reference_offset(),
            scene_node_reference_offset: body.placement.scene_node_reference_offset(),
            collection_reference_offset: body.placement.collection_reference_offset(),
            wrapper_body_reference_offset: body.wrapper_body_reference_offset(),
            entry_guid_reference_offset: body.entry.guid_reference_offset(),
            guid_entry_reference_offset: body.guid.entry_reference_offset(),
            scene_state_reference_offset: body.scene_node.state_reference_offset(),
            scene_auxiliary_reference_offset: body.scene_node.auxiliary_reference_offset(),
            tessellation_id: body.tessellation_id.as_ref(),
        }
        .serialize(serializer)
    }
}

struct TextureRef<'a> {
    table: &'a DesignMeshTextureTable,
    resource: &'a DesignMeshTextureResource,
}

impl Serialize for TextureRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            ordinal: u32,
            resource_guid: &'a super::DesignGuidText,
            flags_guid_offset: u64,
            flags: u32,
            flags_offset: u64,
            filename_ordinal: u32,
            filename_guid_offset: u64,
            filename_record: RecordRef<'a>,
            filename_record_reference_offset: u64,
            filename: &'a str,
            filename_offset: u64,
            archive_entry_name: &'a str,
            asset: &'a super::AssetId,
        }
        let resource = self.resource;
        let flags_start = self.table.record.byte_offset()
            + u64_from_index(crate::layout::paramesh_texture_table_prefix::LEN);
        let filenames_start = self.table.filename_count_offset() + 4;
        let flags_guid_offset =
            flags_start + MESH_TEXTURE_FLAGS_ENTRY_BYTES * u64::from(resource.ordinal) + 4;
        let filename_guid_offset = filenames_start
            + MESH_TEXTURE_FILENAME_ENTRY_BYTES * u64::from(resource.filename_ordinal)
            + 4;
        Wire {
            ordinal: resource.ordinal,
            resource_guid: &resource.resource_guid,
            flags_guid_offset,
            flags: resource.flags,
            flags_offset: flags_guid_offset + 36,
            filename_ordinal: resource.filename_ordinal,
            filename_guid_offset,
            filename_record: RecordRef::from_identity(resource.file.record()),
            filename_record_reference_offset: filename_guid_offset + 36,
            filename: resource.file.filename(),
            filename_offset: resource.file.filename_offset(),
            archive_entry_name: resource.file.archive_entry_name(),
            asset: &resource.asset,
        }
        .serialize(serializer)
    }
}

struct TextureRows<'a>(&'a DesignMeshTextureTable);

impl Serialize for TextureRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.resources.iter().map(|resource| TextureRef {
            table: self.0,
            resource,
        }))
    }
}

struct BodyReferenceOffsets {
    start: u64,
    count: u32,
}

impl Serialize for BodyReferenceOffsets {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq((0..self.count).map(|index| self.start + 11 * u64::from(index)))
    }
}

impl Serialize for DesignMeshFeature {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            id: &'a str,
            scope_record: RecordRef<'a>,
            scope_base_record: RecordRef<'a>,
            collection_record: RecordRef<'a>,
            collection_base_record: RecordRef<'a>,
            texture_table_record: RecordRef<'a>,
            body_count_offsets: [u64; 3],
            body_record_indices: SliceColumn<'a, DesignMeshBody, u32>,
            scope_body_reference_offsets: BodyReferenceOffsets,
            collection_body_reference_offsets: BodyReferenceOffsets,
            texture_table_reference_offset: u64,
            collection_owner_record: RecordRef<'a>,
            collection_owner_reference_offset: u64,
            collection_owner_backlink_offset: u64,
            scope_owner_record_index: u32,
            scope_owner_reference_offset: u64,
            texture_flags_count_offset: u64,
            texture_filename_count_offset: u64,
            bodies: SliceColumn<'a, DesignMeshBody, BodyRef<'a>>,
            textures: TextureRows<'a>,
        }
        let scope_start = self.scope.record().byte_offset()
            + u64_from_index(crate::layout::paramesh_feature_scope_prefix::LEN);
        let collection_start = self.collection.record().byte_offset()
            + u64_from_index(crate::layout::paramesh_mesh_collection_prefix::LEN)
            + u64_from_index(crate::layout::paramesh_mesh_collection_base_prefix::LEN);
        let body_count = u32::try_from(self.bodies.len())
            .map_err(|_| serde::ser::Error::custom("mesh body count exceeds u32"))?;
        Wire {
            id: &self.id,
            scope_record: RecordRef::from_identity(self.scope.record()),
            scope_base_record: RecordRef::scope_base(&self.scope),
            collection_record: RecordRef::from_identity(self.collection.record()),
            collection_base_record: RecordRef::collection_base(&self.collection),
            texture_table_record: RecordRef::from_identity(self.texture_table.record()),
            body_count_offsets: self.body_count_offsets(),
            body_record_indices: SliceColumn::new(&self.bodies, |body| {
                body.placement.record.record_index()
            }),
            scope_body_reference_offsets: BodyReferenceOffsets {
                start: scope_start,
                count: body_count,
            },
            collection_body_reference_offsets: BodyReferenceOffsets {
                start: collection_start,
                count: body_count,
            },
            texture_table_reference_offset: self.collection.texture_table_reference_offset(),
            collection_owner_record: RecordRef::from_identity(&self.collection_owner.record),
            collection_owner_reference_offset: self.collection.owner_reference_offset(),
            collection_owner_backlink_offset: self.collection_owner.backlink_offset(),
            scope_owner_record_index: self.scope.owner_record_index(),
            scope_owner_reference_offset: self.scope.owner_reference_offset(),
            texture_flags_count_offset: self.texture_table.flags_count_offset(),
            texture_filename_count_offset: self.texture_table.filename_count_offset(),
            bodies: SliceColumn::new(&self.bodies, BodyRef),
            textures: TextureRows(&self.texture_table),
        }
        .serialize(serializer)
    }
}
