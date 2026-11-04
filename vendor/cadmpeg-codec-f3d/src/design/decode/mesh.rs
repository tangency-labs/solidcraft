// SPDX-License-Identifier: Apache-2.0
//! Join mesh-geometry containers to their bodies through the Design segment.
//!
//! A mesh body's geometry lives in a `.paramesh` container ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers)),
//! and a typed Design graph joins the container, mesh body, owning feature,
//! optional texture resources, and Scene state ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::DecodeContext;

use crate::bytes::{lp_ascii_strict, take_reference};
use crate::container::ContainerScan;
use crate::design::decode::meta::{
    metadata_for_bulk_stream, typed_primary_frames, TypedPrimaryFrame,
};
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope_with_type_guid;
use crate::design::decode::sketch::IndexedRecordOffsets;

use crate::layout::indexed_design_record_header as indexed_header;
use crate::layout::paramesh_body_wrapper as body_wrapper;
use crate::layout::paramesh_collection_owner_backlink_prefix as collection_owner;
use crate::layout::paramesh_collection_owner_v17 as collection_owner_v17;
use crate::layout::paramesh_entry_name_prefix as entry_name_prefix;
use crate::layout::paramesh_feature_scope_base as feature_scope_base;
use crate::layout::paramesh_feature_scope_prefix as feature_scope;
use crate::layout::paramesh_flagged_mesh_collection_prefix as flagged_collection;
use crate::layout::paramesh_guid_join_prefix as guid_join;
use crate::layout::paramesh_mesh_body_join_prefix as mesh_body;
use crate::layout::paramesh_mesh_collection_base_prefix as mesh_collection_base;
use crate::layout::paramesh_mesh_collection_prefix as mesh_collection;
use crate::layout::paramesh_scene_node as scene_node;
use crate::layout::paramesh_scene_node_placed as placed_scene_node;
use crate::layout::paramesh_scene_state as scene_state;
use crate::layout::paramesh_texture_filename_prefix as texture_filename;
use crate::layout::paramesh_texture_table_prefix as texture_table;
use crate::paramesh::{decode_mesh_container, MeshContainer};
use crate::records::mesh::{
    DesignGuidText, DesignMeshBody, DesignMeshCollection, DesignMeshCollectionOwner,
    DesignMeshEntryName, DesignMeshFeature, DesignMeshFixedRecord, DesignMeshGuid,
    DesignMeshPlacement, DesignMeshRecordIdentity, DesignMeshSceneBounds, DesignMeshSceneNode,
    DesignMeshSceneState, DesignMeshScope, DesignMeshTextureResource, DesignMeshTextureTable,
    MeshAffineTransform,
};
use cadmpeg_core::decode::{bounded_len, u64_from_index, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::units::UnitVector3;
use std::collections::{HashMap, HashSet};

const PARAMESH_MODULE: &str = "ParaMesh";
const COMMON_DATA_MODULE: &str = "CommonData";
const DATA_MODEL_MODULE: &str = "DataModel";
const FUSION_MODULE: &str = "Fusion";
const SCENE_MODULE: &str = "Scene";
const MESH_ENTRY_NAME_TYPE_GUID: &str = "A1BAA3F6-4B67-4A0D-BACC-75F38A2230F3";
const MESH_ENTRY_NAME_BASE_TYPE_GUID: &str = "130A0711-4E92-4FCD-AADE-B9C82238BB27";
const MESH_ENTRY_NAME_TYPE_VERSION: u32 = 0;
const MESH_GUID_TYPE_GUID: &str = "A8338A26-5436-433C-8BAC-C3CF024AD595";
const MESH_GUID_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_GUID_TYPE_VERSION: u32 = 1;
const MESH_BODY_TYPE_GUID: &str = "EA90DA22-556C-4C61-89BB-20C2681B7A9D";
const MESH_BODY_BASE_TYPE_GUID: &str = "CB844AB6-240D-4FC9-9C9F-3679DC896D6F";
const MESH_BODY_TYPE_VERSION: u32 = 7;
const MESH_COLLECTION_TYPE_GUID: &str = "443807AD-8025-41A3-8A50-5157579C3D78";
const MESH_COLLECTION_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const MESH_COLLECTION_TYPE_VERSION: u32 = 0;
const MESH_COLLECTION_BASE_BASE_TYPE_GUID: &str = "834C9DEF-4C39-4587-A08D-A5BD1267B7B4";
const MESH_COLLECTION_BASE_TYPE_VERSION: u32 = 0;
const MESH_TEXTURE_TABLE_TYPE_GUID: &str = "6FC173DC-C7E3-402C-A8C0-891A26DADF8D";
const MESH_TEXTURE_TABLE_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_TEXTURE_TABLE_TYPE_VERSION: u32 = 0;
const MESH_WRAPPER_TYPE_GUID: &str = "E5B3F49A-D8D0-4EEF-BC2B-FCDDAEF9745E";
const MESH_WRAPPER_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_WRAPPER_TYPE_VERSION: u32 = 0;
const MESH_FEATURE_SCOPE_TYPE_GUID: &str = "99F6967E-ED35-4222-B906-5CCF0AC70B53";
const MESH_FEATURE_SCOPE_BASE_TYPE_GUID: &str = "2FCB0587-233E-449B-9724-9AAE5AA23647";
const MESH_FEATURE_SCOPE_TYPE_VERSION: u32 = 0;
const MESH_SCOPE_BASE_RECORD_TYPE_GUID: &str = "CB844AB6-240D-4FC9-9C9F-3679DC896D6F";
const MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_SCOPE_BASE_RECORD_TYPE_VERSION: u32 = 1;
const MESH_SCENE_STATE_TYPE_GUID: &str = "F85F2E62-7627-4922-A16D-53E1275D2AAC";
const MESH_SCENE_STATE_BASE_TYPE_GUID: &str = "D0EDEF7C-5879-45A4-9651-900659CC4FDD";
const MESH_SCENE_STATE_TYPE_VERSION: u32 = 0;
const SCENE_NODE_TYPE_GUID: &str = "702B9CD2-537C-429E-8CC4-22BEEEB98C37";
const SCENE_NODE_BASE_TYPE_GUID: &str = "EB7847AF-E60D-4AB0-A736-4AC00C1F1D21";
const SCENE_NODE_TYPE_VERSION: u32 = 1;
const SCENE_AUXILIARY_TYPE_GUID: &str = "2343B7AB-A2E0-4C66-8B74-99A05E4C670B";
const SCENE_AUXILIARY_BASE_TYPE_GUID: &str = "74D7FEF9-44E9-494D-A25D-81EC33D2841E";
const SCENE_AUXILIARY_TYPE_VERSION: u32 = 1;
const MESH_TEXTURE_FILENAME_TYPE_GUID: &str = "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06573";
const MESH_TEXTURE_FILENAME_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_TEXTURE_FILENAME_TYPE_VERSION: u32 = 0;
const MESH_COLLECTION_OWNER_TYPE_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
const MESH_COLLECTION_OWNER_BASE_TYPE_GUID: &str = "42054630-20A0-40E1-B969-CFE9E742F5C9";
const MESH_COLLECTION_OWNER_TYPE_VERSIONS: [u32; 3] = [15, 17, 23];
const MESH_BODY_OWNER_TYPE_GUID: &str = "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA";
const MESH_BODY_OWNER_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const MESH_BODY_OWNER_TYPE_VERSION: u32 = 4;

/// Row-major 4x4 f64 matrix byte length.
#[cfg(test)]
const MATRIX_BYTES: usize = 128;
const SAME_SEGMENT_REFERENCE_BYTES: usize = indexed_header::LEN;
const SCENE_FOOTER_BYTES: usize = scene_state::LEN - scene_state::FOOTER_MARKER;

/// One mesh body's geometry, in model millimetres.
pub(crate) struct MeshBody {
    /// Deterministic native identifier, keyed by the mesh-body record.
    pub(crate) id: String,
    /// Vertex positions in model millimetres.
    pub(crate) vertices: Vec<FinitePoint3>,
    /// Triangle corner indices into `vertices`.
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Source-classified feature edges as ascending vertex-index pairs.
    pub(crate) feature_edges: Vec<[u32; 2]>,
    /// One transformed unit normal per flattened triangle corner.
    pub(crate) corner_normals: Option<Vec<UnitVector3>>,
    /// Source face groups as an ordered partition of triangle ordinals.
    pub(crate) triangle_groups: Vec<crate::paramesh::MeshTriangleGroup>,
    /// One texture-table selector per triangle, when authored.
    pub(crate) texture_ids: Option<Vec<u32>>,
    /// The attribute channels the container's registry declares.
    pub(crate) attributes: Vec<crate::paramesh::MeshAttribute>,
}

impl MeshAffineTransform {
    fn parse(bytes: &[u8], at: usize) -> Option<Self> {
        let mut cells = [0.0; 16];
        for (index, cell) in cells.iter_mut().enumerate() {
            *cell = View::f64_le_at(bytes, at.checked_add(index.checked_mul(8)?)?)?;
        }
        Self::new(cells).ok()
    }

    fn transform_point(self, point: FinitePoint3) -> Result<FinitePoint3, CodecError> {
        let point = point.get();
        let (x, y, z) = (point.x, point.y, point.z);
        let cells = self.cells();
        let point = cadmpeg_ir::math::Point3::new(
            (cells[0] * x + cells[1] * y + cells[2] * z + cells[3])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
            (cells[4] * x + cells[5] * y + cells[6] * z + cells[7])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
            (cells[8] * x + cells[9] * y + cells[10] * z + cells[11])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
        );
        FinitePoint3::new(point).ok_or_else(|| {
            CodecError::Malformed("F3D mesh placement produces a non-finite vertex".into())
        })
    }

    /// Transform an oriented surface normal with the cofactor of the linear
    /// map. The determinant sign keeps the normal aligned with the unchanged
    /// triangle tuple under a reflection.
    fn transform_normal(self, normal: UnitVector3) -> Result<UnitVector3, CodecError> {
        let transform = self.transform();
        transform
            .apply_normal(*normal.as_raw())
            .zip(transform.orientation())
            .map(|(normal, orientation)| {
                if orientation.is_sign_negative() {
                    normal.reversed()
                } else {
                    normal
                }
            })
            .ok_or_else(|| CodecError::malformed("F3D mesh placement produces a degenerate normal"))
    }
}

/// The two equal affine maps stored by a mesh-body class record.
fn mesh_body_transform(payload: &[u8]) -> Option<MeshAffineTransform> {
    let first = MeshAffineTransform::parse(payload, mesh_body::FIRST_TRANSFORM)?;
    let second = MeshAffineTransform::parse(payload, mesh_body::SECOND_TRANSFORM)?;
    (first == second).then_some(first)
}

/// Result of decoding and joining one `.paramesh` entry.
pub(crate) enum MeshContainerOutcome {
    /// Geometry and its Design body record were decoded and joined.
    Joined(MeshBody),
    /// The container decoded, but no complete Design join named it.
    Unjoined {
        /// Native archive entry name.
        entry_name: String,
    },
    /// Reading or parsing the container failed independently of other entries.
    Failed {
        /// Native archive entry name.
        entry_name: String,
        /// Exact read or parse failure.
        error: CodecError,
    },
    /// A complete Design mesh body names no archive entry.
    Missing {
        /// Entry basename stored by the Design record.
        entry_name: String,
    },
}

/// Complete mesh decode: per-container outcomes plus typed Design features.
pub(crate) struct MeshDecode {
    pub(crate) outcomes: Vec<MeshContainerOutcome>,
    pub(crate) features: Vec<DesignMeshFeature>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshEntryNameRecord {
    entry: DesignMeshEntryName,
    guid_record_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshGuidRecord {
    guid: DesignMeshGuid,
    entry_name_record_index: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct MeshBodyRecord {
    placement: DesignMeshPlacement,
    guid_record_index: u32,
    scope_record_index: u32,
    wrapper_record_index: u32,
    owner_record_index: u32,
    scene_node_record_index: u32,
    collection_record_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshCollectionRecord {
    collection: DesignMeshCollection,
    texture_table_record_index: u32,
    body_records: Vec<u32>,
    owner_record_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshTextureMapEntry {
    ordinal: u32,
    resource_guid: DesignGuidText,
    value: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshTextureFilenameEntry {
    ordinal: u32,
    resource_guid: DesignGuidText,
    filename_record_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshTextureTableRecord {
    identity: DesignMeshRecordIdentity,
    flags: Vec<MeshTextureMapEntry>,
    filenames: Vec<MeshTextureFilenameEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshWrapperRecord {
    identity: DesignMeshFixedRecord<{ u64_from_index(body_wrapper::LEN) }>,
    body_record_index: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct MeshSceneNodeRecord {
    node: DesignMeshSceneNode,
    state_record_index: u32,
    auxiliary_record_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshScopeRecord {
    scope: DesignMeshScope,
    body_records: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshCollectionOwnerRecord {
    owner: DesignMeshCollectionOwner,
    collection_record_index: u32,
}

impl MeshBody {
    /// Project one decoded container through its joined Design body record.
    fn from_container(
        entry_name: &str,
        body_byte_offset: u64,
        transform: MeshAffineTransform,
        container: MeshContainer,
    ) -> Result<Self, CodecError> {
        let MeshContainer {
            fusion_uuid: _,
            mesh_uuid: _,
            vertices,
            triangles,
            feature_edges,
            corner_normals,
            triangle_groups,
            texture_ids,
            attributes,
        } = container;
        let id = format!(
            "{}:mesh-body#{body_byte_offset}",
            crate::ids::native_scope(entry_name)
        );
        let mut placed_vertices = Vec::new();
        for point in vertices {
            placed_vertices.push(transform.transform_point(point)?);
        }
        let placed_normals = if let Some(normals) = corner_normals {
            let mut placed = Vec::new();
            for normal in normals {
                placed.push(transform.transform_normal(normal)?);
            }
            Some(placed)
        } else {
            None
        };
        Ok(Self {
            id,
            vertices: placed_vertices,
            // Placement does not change indexing. Triangle tuples, feature
            // edges, and corner selectors remain in serialized order.
            triangles,
            feature_edges,
            corner_normals: placed_normals,
            triangle_groups,
            texture_ids,
            attributes,
        })
    }
}

fn validate_mesh_registration(
    frame: TypedPrimaryFrame<'_>,
    expected_version: u32,
    expected_base_type_guid: &str,
    expected_module: &str,
    record_kind: &str,
) -> Result<(), CodecError> {
    if frame.design_type.version != expected_version {
        return Err(CodecError::NotImplemented(format!(
            "F3D Design {record_kind} record version {} is unsupported",
            frame.design_type.version
        )));
    }
    if frame.design_type.module != expected_module
        || !frame
            .design_type
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            .is_some_and(|base| base.eq_ignore_ascii_case(expected_base_type_guid))
    {
        return Err(crate::design::text::malformed_design(format_args!(
            "F3D Design {record_kind} entity {} has incompatible registration metadata",
            frame.entity_id
        )));
    }
    Ok(())
}

fn exact_record_index(
    record: &[u8],
    frame: TypedPrimaryFrame<'_>,
    record_kind: &str,
) -> Result<u32, CodecError> {
    View::u32_le_at(record, 7)
        .filter(|record_index| u64::from(*record_index) == frame.entity_id)
        .ok_or_else(|| {
            crate::design::text::malformed_design(format_args!(
                "F3D Design {record_kind} entity {} has an invalid record index",
                frame.entity_id
            ))
        })
}

fn malformed_frame(record_kind: &str, entity_id: u64) -> CodecError {
    crate::design::text::malformed_design(format_args!(
        "F3D Design {record_kind} entity {entity_id} has an invalid primary frame"
    ))
}

fn source_offset(frame_start: usize, relative: usize) -> Option<u64> {
    u64::try_from(frame_start.checked_add(relative)?).ok()
}

fn indexed_class_tag(
    record: &[u8],
    at: usize,
) -> Option<crate::records::references::DesignClassTag> {
    (View::u32_le_at(record, at) == Some(3)).then_some(())?;
    let tag = std::str::from_utf8(record.get(at.checked_add(4)?..at.checked_add(7)?)?).ok()?;
    crate::records::references::DesignClassTag::try_from(tag.to_owned()).ok()
}

fn record_identity(
    record: &[u8],
    frame: TypedPrimaryFrame<'_>,
    record_kind: &str,
) -> Result<DesignMeshRecordIdentity, CodecError> {
    let record_index = exact_record_index(record, frame, record_kind)?;
    let class_tag = indexed_class_tag(record, 0)
        .ok_or_else(|| malformed_frame(record_kind, frame.entity_id))?;
    DesignMeshRecordIdentity::new(
        class_tag,
        record_index,
        u64::try_from(frame.start).map_err(|_| malformed_frame(record_kind, frame.entity_id))?,
        u64::try_from(
            frame
                .end
                .checked_sub(frame.start)
                .ok_or_else(|| malformed_frame(record_kind, frame.entity_id))?,
        )
        .map_err(|_| malformed_frame(record_kind, frame.entity_id))?,
    )
    .map_err(|_| malformed_frame(record_kind, frame.entity_id))
}

fn validate_design_type(
    design_type: &crate::records::entity_header::SegmentTypeData,
    expected_type_guid: &str,
    expected_base_type_guid: &str,
    expected_version: u32,
    expected_module: &str,
) -> bool {
    design_type
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(expected_type_guid)
        && design_type.version == expected_version
        && design_type.module == expected_module
        && design_type
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            .is_some_and(|base| base.eq_ignore_ascii_case(expected_base_type_guid))
}

#[derive(Clone, Copy)]
struct MeshRecordType<'a> {
    type_guid: &'a str,
    base_type_guid: &'a str,
    version: u32,
    module: &'a str,
}

#[derive(Clone, Copy)]
struct NestedMeshRecordFrame {
    frame_start: usize,
    at: usize,
    end: usize,
    record_index: u32,
}

fn nested_record_identity(
    record: &[u8],
    frame: NestedMeshRecordFrame,
    meta: &crate::metastream::MetaStream,
    input: MeshRecordType<'_>,
) -> Option<DesignMeshRecordIdentity> {
    let NestedMeshRecordFrame {
        frame_start,
        at,
        end,
        record_index,
    } = frame;
    let MeshRecordType {
        type_guid: expected_type_guid,
        base_type_guid: expected_base_type_guid,
        version: expected_version,
        module: expected_module,
    } = input;
    let class_tag = indexed_class_tag(record, at)?;
    (View::u32_le_at(record, at.checked_add(indexed_header::RECORD_INDEX)?) == Some(record_index))
        .then_some(())?;
    let tag = class_tag.as_str().parse::<u32>().ok()?;
    let ordinal = usize::try_from(tag.checked_sub(256)?).ok()?;
    validate_design_type(
        meta.types.get(ordinal)?,
        expected_type_guid,
        expected_base_type_guid,
        expected_version,
        expected_module,
    )
    .then_some(())?;
    DesignMeshRecordIdentity::new(
        class_tag,
        record_index,
        source_offset(frame_start, at)?,
        u64::try_from(end.checked_sub(at)?).ok()?,
    )
    .ok()
}

fn exact_local_record_index(record: &[u8], at: usize) -> Option<u32> {
    let mut cursor = at;
    let reference = take_reference(record, &mut cursor)?;
    if cursor != at.checked_add(SAME_SEGMENT_REFERENCE_BYTES)? {
        return None;
    }
    u32::try_from(reference.local()?.0)
        .ok()
        .filter(|target| *target != 0)
}

fn counted_local_record_indices(record: &[u8], count_at: usize) -> Option<(Vec<u32>, usize)> {
    let raw_count = View::u32_le_at(record, count_at)?;
    let mut at = count_at.checked_add(4)?;
    let count = record.len().checked_sub(at).and_then(|remaining| {
        bounded_len(
            u64::from(raw_count),
            SAME_SEGMENT_REFERENCE_BYTES,
            remaining,
        )
    })?;

    let mut references = Vec::new();
    for _ in 0..count {
        let index = exact_local_record_index(record, at)?;
        references.push(index);
        let next_at = at.checked_add(SAME_SEGMENT_REFERENCE_BYTES)?;
        at = next_at;
    }
    Some((references, at))
}

fn parse_mesh_entry_name_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshEntryNameRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_ENTRY_NAME_TYPE_VERSION,
        MESH_ENTRY_NAME_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-entry-name",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-entry-name")?;
    if record.get(entry_name_prefix::ZERO_RUN_10..entry_name_prefix::GUID_RECORD_REFERENCE)
        != Some(&[0; 10])
    {
        return Err(malformed_frame("mesh-entry-name", frame.entity_id));
    }
    let guid_record_index =
        exact_local_record_index(record, entry_name_prefix::GUID_RECORD_REFERENCE)
            .ok_or_else(|| malformed_frame("mesh-entry-name", frame.entity_id))?;
    let (entry_name, _) = lp_utf16_bounded(record, entry_name_prefix::LEN, 1..=1024)
        .ok_or_else(|| malformed_frame("mesh-entry-name", frame.entity_id))?;
    Ok(MeshEntryNameRecord {
        entry: DesignMeshEntryName::new(identity, entry_name)
            .map_err(|_| malformed_frame("mesh-entry-name", frame.entity_id))?,
        guid_record_index,
    })
}

fn parse_mesh_guid_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshGuidRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_GUID_TYPE_VERSION,
        MESH_GUID_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-GUID",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-GUID")?;
    let parsed = (|| {
        (record.get(guid_join::ZERO_RUN_21..guid_join::FUSION_UUID) == Some(&[0; 21]))
            .then_some(())?;
        let (fusion_uuid, end) = lp_ascii_strict(record, guid_join::FUSION_UUID, 36..=36)?;
        (end == guid_join::ENTRY_NAME_BACKLINK).then_some(())?;
        crate::bytes::is_guid_hyphenated(fusion_uuid).then_some(())?;
        let entry_name_record_index =
            exact_local_record_index(record, guid_join::ENTRY_NAME_BACKLINK)?;
        Some((fusion_uuid, entry_name_record_index))
    })();
    let (guid, entry_name_record_index) =
        parsed.ok_or_else(|| malformed_frame("mesh-GUID", frame.entity_id))?;
    let guid = DesignGuidText::try_from(guid.to_owned())
        .map_err(|_| malformed_frame("mesh-GUID", frame.entity_id))?;
    Ok(MeshGuidRecord {
        guid: DesignMeshGuid::new(identity, guid)
            .map_err(|_| malformed_frame("mesh-GUID", frame.entity_id))?,
        entry_name_record_index,
    })
}

fn parse_mesh_body_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshBodyRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_BODY_TYPE_VERSION,
        MESH_BODY_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-body",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-body")?;
    let parsed = (|| {
        (record.get(mesh_body::ZERO_RUN_10..mesh_body::ZERO_RUN_10 + 10) == Some(&[0; 10]))
            .then_some(())?;
        let transform = mesh_body_transform(record)?;
        let placement = DesignMeshPlacement::new(identity, transform).ok()?;
        let scope_record_index =
            exact_local_record_index(record, mesh_body::FEATURE_SCOPE_REFERENCE)?;
        let wrapper_record_index = exact_local_record_index(record, mesh_body::WRAPPER_REFERENCE)?;
        let owner_record_index = exact_local_record_index(record, mesh_body::BODY_OWNER_REFERENCE)?;
        let guid_record_index =
            exact_local_record_index(record, mesh_body::CONTAINER_GUID_REFERENCE)?;
        let scene_node_record_index =
            exact_local_record_index(record, mesh_body::SCENE_NODE_REFERENCE)?;
        let collection_reference_at = record.len().checked_sub(SAME_SEGMENT_REFERENCE_BYTES)?;
        let collection_record_index = exact_local_record_index(record, collection_reference_at)?;
        Some(MeshBodyRecord {
            placement,
            guid_record_index,
            scope_record_index,
            wrapper_record_index,
            owner_record_index,
            scene_node_record_index,
            collection_record_index,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-body", frame.entity_id))
}

/// Parse one mesh-collection record. `None` for an empty collection, an empty
/// `ParaMesh` registry with no mesh graph.
fn parse_mesh_collection_record(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    frame: TypedPrimaryFrame<'_>,
) -> Result<Option<MeshCollectionRecord>, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_COLLECTION_TYPE_VERSION,
        MESH_COLLECTION_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-collection",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-collection")?;
    let flagged = record.get(flagged_collection::FLAG) == Some(&flagged_collection::FLAG_VALUE);
    let (prefix_len, zero_run, constant_at, texture_at) = if flagged {
        (
            flagged_collection::LEN,
            flagged_collection::ZERO_RUN_9..flagged_collection::FLAG,
            flagged_collection::CONSTANT_01_01,
            flagged_collection::TEXTURE_TABLE_REFERENCE,
        )
    } else {
        (
            mesh_collection::LEN,
            mesh_collection::ZERO_RUN_10..mesh_collection::BODY_COUNT,
            mesh_collection::CONSTANT_01_01,
            mesh_collection::TEXTURE_TABLE_REFERENCE,
        )
    };
    let counted_bodies =
        counted_local_record_indices(record, prefix_len + mesh_collection_base::BODY_COUNT);
    let parsed = (|| {
        record
            .get(zero_run)?
            .iter()
            .all(|byte| *byte == 0)
            .then_some(())?;
        (record.get(constant_at..texture_at) == Some(&[1, 1])).then_some(())?;
        let first_count =
            usize::try_from(View::u32_le_at(record, mesh_collection::BODY_COUNT)?).ok()?;
        let texture_table_record_index = exact_local_record_index(record, texture_at)?;
        let base_record = nested_record_identity(
            record,
            crate::design::decode::mesh::NestedMeshRecordFrame {
                frame_start: frame.start,
                at: prefix_len,
                end: record.len(),
                record_index: identity.record_index(),
            },
            meta,
            crate::design::decode::mesh::MeshRecordType {
                type_guid: MESH_COLLECTION_BASE_TYPE_GUID,
                base_type_guid: MESH_COLLECTION_BASE_BASE_TYPE_GUID,
                version: MESH_COLLECTION_BASE_TYPE_VERSION,
                module: COMMON_DATA_MODULE,
            },
        )?;
        (record.get(
            prefix_len + mesh_collection_base::ZERO_RUN_9
                ..prefix_len + mesh_collection_base::BODY_COUNT,
        ) == Some(&[0; 9]))
        .then_some(())?;
        let (body_records, owner_at) = counted_bodies?;
        (first_count == body_records.len()).then_some(())?;
        (!flagged || body_records.is_empty()).then_some(())?;
        // An empty collection can store a zero u32 before its owner reference.
        let owner_at = if body_records.is_empty() && View::u32_le_at(record, owner_at) == Some(0) {
            owner_at.checked_add(4)?
        } else {
            owner_at
        };
        let owner_record_index = exact_local_record_index(record, owner_at)?;
        (owner_at.checked_add(SAME_SEGMENT_REFERENCE_BYTES)? == record.len()).then_some(())?;
        if body_records.is_empty() {
            return Some(None);
        }
        Some(Some(MeshCollectionRecord {
            collection: DesignMeshCollection::new(identity, base_record).ok()?,
            texture_table_record_index,
            body_records,
            owner_record_index,
        }))
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-collection", frame.entity_id))
}

fn parse_mesh_texture_table_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshTextureTableRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_TEXTURE_TABLE_TYPE_VERSION,
        MESH_TEXTURE_TABLE_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-texture-table",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-texture-table")?;
    let parsed = (|| -> Result<Option<MeshTextureTableRecord>, CodecError> {
        if record.get(texture_table::ZERO_RUN_10..texture_table::FLAGS_MAP_COUNT)
            != Some(&[0; 10][..])
        {
            return Ok(None);
        }
        let Some(raw_flags_count) = View::u32_le_at(record, texture_table::FLAGS_MAP_COUNT) else {
            return Ok(None);
        };
        let Some(mut at) = texture_table::FLAGS_MAP_COUNT.checked_add(4) else {
            return Ok(None);
        };
        let Some(flags_count) = record
            .len()
            .checked_sub(at)
            .and_then(|remaining| bounded_len(u64::from(raw_flags_count), 44, remaining))
        else {
            return Ok(None);
        };

        let mut flags = Vec::new();

        let mut flag_keys = HashSet::new();
        for ordinal in 0..flags_count {
            let Some((resource_guid, end)) = lp_ascii_strict(record, at, 36..=36) else {
                return Ok(None);
            };
            if !crate::bytes::is_guid_hyphenated(resource_guid) {
                return Ok(None);
            }
            let key: [u8; 36] = resource_guid
                .as_bytes()
                .try_into()
                .map_err(|_| CodecError::malformed("mesh resource GUID width"))?;
            let key = key.map(|byte| byte.to_ascii_uppercase());
            let resource_guid = DesignGuidText::try_from(resource_guid.to_owned())
                .map_err(CodecError::malformed)?;
            if !flag_keys.insert(key) {
                return Ok(None);
            }
            at = end;
            let Some(value) = View::u32_le_at(record, at) else {
                return Ok(None);
            };
            let Some(next_at) = at.checked_add(4) else {
                return Ok(None);
            };
            at = next_at;
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return Ok(None);
            };
            flags.push(MeshTextureMapEntry {
                ordinal,
                resource_guid,
                value,
            });
        }
        let Some(raw_filename_count) = View::u32_le_at(record, at) else {
            return Ok(None);
        };
        let Some(next_at) = at.checked_add(4) else {
            return Ok(None);
        };
        at = next_at;
        let Some(filename_count) = record
            .len()
            .checked_sub(at)
            .and_then(|remaining| bounded_len(u64::from(raw_filename_count), 51, remaining))
        else {
            return Ok(None);
        };

        let mut filenames = Vec::new();

        let mut filename_keys = HashSet::new();
        for ordinal in 0..filename_count {
            let Some((resource_guid, end)) = lp_ascii_strict(record, at, 36..=36) else {
                return Ok(None);
            };
            if !crate::bytes::is_guid_hyphenated(resource_guid) {
                return Ok(None);
            }
            let key: [u8; 36] = resource_guid
                .as_bytes()
                .try_into()
                .map_err(|_| CodecError::malformed("mesh resource GUID width"))?;
            let key = key.map(|byte| byte.to_ascii_uppercase());
            let resource_guid = DesignGuidText::try_from(resource_guid.to_owned())
                .map_err(CodecError::malformed)?;
            if !filename_keys.insert(key) {
                return Ok(None);
            }
            at = end;
            let Some(filename_record_index) = exact_local_record_index(record, at) else {
                return Ok(None);
            };
            let Some(next_at) = at.checked_add(SAME_SEGMENT_REFERENCE_BYTES) else {
                return Ok(None);
            };
            at = next_at;
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return Ok(None);
            };
            filenames.push(MeshTextureFilenameEntry {
                ordinal,
                resource_guid,
                filename_record_index,
            });
        }
        if at != record.len() || flag_keys != filename_keys {
            return Ok(None);
        }
        Ok(Some(MeshTextureTableRecord {
            identity,
            flags,
            filenames,
        }))
    })()?;
    parsed.ok_or_else(|| malformed_frame("mesh-texture-table", frame.entity_id))
}

fn parse_mesh_wrapper_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshWrapperRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_WRAPPER_TYPE_VERSION,
        MESH_WRAPPER_BASE_TYPE_GUID,
        PARAMESH_MODULE,
        "mesh-wrapper",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = DesignMeshFixedRecord::try_from(record_identity(record, frame, "mesh-wrapper")?)
        .map_err(|_| malformed_frame("mesh-wrapper", frame.entity_id))?;
    let parsed = (|| {
        (record.get(body_wrapper::ZERO_RUN_10..body_wrapper::BODY_REFERENCE) == Some(&[0; 10]))
            .then_some(())?;
        let body_record_index = exact_local_record_index(record, body_wrapper::BODY_REFERENCE)?;
        (record.get(body_wrapper::ZERO_TAIL_8..body_wrapper::LEN) == Some(&[0; 8])).then_some(())?;
        Some(MeshWrapperRecord {
            identity,
            body_record_index,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-wrapper", frame.entity_id))
}

fn scene_state_mask_is_exact(mask: &[u8]) -> bool {
    mask.len() == 49
        && mask.iter().enumerate().all(|(index, byte)| {
            *byte
                == match index {
                    6 | 14 | 22 | 30 | 38 | 46 => 0xef,
                    31 | 39 | 47 => 0x7f,
                    48 => 0x01,
                    _ => 0xff,
                }
        })
}

enum SceneBoundsPayload {
    Invalid,
    Absent,
    Present(DesignMeshSceneBounds),
}

fn parse_scene_footer(record: &[u8], at: usize) -> SceneBoundsPayload {
    if at.checked_add(SCENE_FOOTER_BYTES) != Some(record.len()) || record.get(at) != Some(&1) {
        return SceneBoundsPayload::Invalid;
    }
    let Some(payload_at) = at.checked_add(1) else {
        return SceneBoundsPayload::Invalid;
    };
    parse_scene_bounds_payload(record, payload_at)
}

fn parse_scene_bounds_payload(record: &[u8], payload_at: usize) -> SceneBoundsPayload {
    let Some(payload) = record.get(payload_at..) else {
        return SceneBoundsPayload::Invalid;
    };
    if scene_state_mask_is_exact(payload) {
        return SceneBoundsPayload::Absent;
    }
    if payload.len() != 49 || payload[48] != 1 {
        return SceneBoundsPayload::Invalid;
    }
    let mut values = [0.0; 6];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(offset) = ordinal
            .checked_mul(8)
            .and_then(|relative| payload_at.checked_add(relative))
        else {
            return SceneBoundsPayload::Invalid;
        };
        let Some(parsed) = View::f64_le_at(record, offset) else {
            return SceneBoundsPayload::Invalid;
        };
        *value = parsed;
    }
    match DesignMeshSceneBounds::new(
        [values[0], values[1], values[2]],
        [values[3], values[4], values[5]],
    ) {
        Ok(bounds) => SceneBoundsPayload::Present(bounds),
        Err(_) => SceneBoundsPayload::Invalid,
    }
}

fn parse_mesh_scene_state_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<DesignMeshSceneState, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_SCENE_STATE_TYPE_VERSION,
        MESH_SCENE_STATE_BASE_TYPE_GUID,
        SCENE_MODULE,
        "mesh-scene-state",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity =
        DesignMeshFixedRecord::try_from(record_identity(record, frame, "mesh-scene-state")?)
            .map_err(|_| malformed_frame("mesh-scene-state", frame.entity_id))?;
    if record.get(scene_state::ZERO_RUN_34..scene_state::FOOTER_MARKER) != Some(&[0; 34]) {
        return Err(malformed_frame("mesh-scene-state", frame.entity_id));
    }
    let bounds = match parse_scene_footer(record, scene_state::FOOTER_MARKER) {
        SceneBoundsPayload::Invalid => {
            return Err(malformed_frame("mesh-scene-state", frame.entity_id))
        }
        SceneBoundsPayload::Absent => None,
        SceneBoundsPayload::Present(bounds) => Some(bounds),
    };
    Ok(DesignMeshSceneState::new(identity, bounds))
}

fn parse_scene_node_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshSceneNodeRecord, CodecError> {
    validate_mesh_registration(
        frame,
        SCENE_NODE_TYPE_VERSION,
        SCENE_NODE_BASE_TYPE_GUID,
        SCENE_MODULE,
        "mesh-scene-node",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-scene-node")?;
    let parsed = (|| {
        (record.get(scene_node::ZERO_RUN_14..scene_node::CONSTANT_TWO_A) == Some(&[0; 14])
            && View::u32_le_at(record, scene_node::CONSTANT_TWO_A) == Some(2)
            && View::u32_le_at(record, scene_node::CONSTANT_TWO_B) == Some(2)
            && View::u32_le_at(record, scene_node::CONSTANT_THREE) == Some(3))
        .then_some(())?;
        let (bounds, transform) = if record.len() == scene_node::LEN
            && record.get(scene_node::ZERO_RUN_24..scene_node::FOOTER_MARKER) == Some(&[0; 24])
        {
            (parse_scene_footer(record, scene_node::FOOTER_MARKER), None)
        } else if record.len() == placed_scene_node::LEN
            && record.get(placed_scene_node::ZERO_RUN_25..placed_scene_node::TRANSFORM)
                == Some(&[0; 25])
        {
            (
                parse_scene_bounds_payload(record, placed_scene_node::FOOTER_MASK),
                Some(MeshAffineTransform::parse(
                    record,
                    placed_scene_node::TRANSFORM,
                )?),
            )
        } else {
            return None;
        };
        let bounds = match bounds {
            SceneBoundsPayload::Invalid => return None,
            SceneBoundsPayload::Absent => None,
            SceneBoundsPayload::Present(bounds) => Some(bounds),
        };
        Some(MeshSceneNodeRecord {
            node: DesignMeshSceneNode::new(identity, bounds, transform).ok()?,
            state_record_index: exact_local_record_index(
                record,
                scene_node::SCENE_STATE_REFERENCE,
            )?,
            auxiliary_record_index: exact_local_record_index(
                record,
                scene_node::AUXILIARY_RECORD_REFERENCE,
            )?,
        })
    })();
    parsed.ok_or_else(|| malformed_frame("mesh-scene-node", frame.entity_id))
}

fn parse_typed_identity(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
    expected_version: u32,
    expected_base_type_guid: &str,
    expected_module: &str,
    record_kind: &str,
) -> Result<DesignMeshRecordIdentity, CodecError> {
    validate_mesh_registration(
        frame,
        expected_version,
        expected_base_type_guid,
        expected_module,
        record_kind,
    )?;
    record_identity(&bytes[frame.start..frame.end], frame, record_kind)
}

fn parse_mesh_scope_record(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    records: &IndexedRecordOffsets,
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshScopeRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_FEATURE_SCOPE_TYPE_VERSION,
        MESH_FEATURE_SCOPE_BASE_TYPE_GUID,
        FUSION_MODULE,
        "mesh-feature-scope",
    )?;
    let record = &bytes[frame.start..frame.end];
    let identity = record_identity(record, frame, "mesh-feature-scope")?;
    let counted_bodies = counted_local_record_indices(record, feature_scope::BODY_COUNT);
    let parsed = (|| {
        (record.get(feature_scope::ZERO_RUN_10..feature_scope::BODY_COUNT) == Some(&[0; 10]))
            .then_some(())?;
        let (body_records, body_list_end) = counted_bodies?;
        let scope = match parse_parameter_scope_with_type_guid(
            bytes,
            records,
            identity.record_index(),
            identity.class_tag(),
            u64::try_from(frame.start).ok()?,
            Some(frame.design_type.type_guid.as_str()),
        ) {
            Ok(Some(scope)) => scope,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        (scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature
            && scope.byte_offset() == u64::try_from(frame.start).ok()?)
        .then_some(())?;
        let paired_at = usize::try_from(scope.paired_byte_offset()).ok()?;
        let paired_relative = paired_at.checked_sub(frame.start)?;
        (body_list_end <= paired_relative
            && paired_relative.checked_add(feature_scope_base::LEN) == Some(record.len()))
        .then_some(())?;
        let base_record = nested_record_identity(
            record,
            crate::design::decode::mesh::NestedMeshRecordFrame {
                frame_start: frame.start,
                at: paired_relative,
                end: record.len(),
                record_index: identity.record_index(),
            },
            meta,
            crate::design::decode::mesh::MeshRecordType {
                type_guid: MESH_SCOPE_BASE_RECORD_TYPE_GUID,
                base_type_guid: MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID,
                version: MESH_SCOPE_BASE_RECORD_TYPE_VERSION,
                module: DATA_MODEL_MODULE,
            },
        )?;
        (record.get(
            paired_relative + feature_scope_base::ZERO_RUN_8
                ..paired_relative + feature_scope_base::SCOPE_OWNER_REFERENCE,
        ) == Some(&[0; 8]))
        .then_some(())?;
        let owner_at = paired_relative.checked_add(feature_scope_base::SCOPE_OWNER_REFERENCE)?;
        let owner_record_index = exact_local_record_index(record, owner_at)?;
        Some(Ok(MeshScopeRecord {
            scope: DesignMeshScope::new(identity, base_record, owner_record_index).ok()?,
            body_records,
        }))
    })();
    parsed
        .transpose()?
        .ok_or_else(|| malformed_frame("mesh-feature-scope", frame.entity_id))
}

fn parse_mesh_collection_owner_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<Option<MeshCollectionOwnerRecord>, CodecError> {
    let admitted_version =
        if MESH_COLLECTION_OWNER_TYPE_VERSIONS.contains(&frame.design_type.version) {
            frame.design_type.version
        } else {
            MESH_COLLECTION_OWNER_TYPE_VERSIONS[2]
        };
    let identity = parse_typed_identity(
        bytes,
        frame,
        admitted_version,
        MESH_COLLECTION_OWNER_BASE_TYPE_GUID,
        FUSION_MODULE,
        "mesh-collection-owner",
    )?;
    let record = &bytes[frame.start..frame.end];
    let Some(backlink_at) = (match frame.design_type.version {
        15 => record.len().checked_sub(11),
        17 if record.len() >= collection_owner_v17::LEN => {
            Some(collection_owner_v17::COLLECTION_BACKLINK)
        }
        23 => Some(collection_owner::COLLECTION_BACKLINK),
        _ => None,
    }) else {
        return Ok(None);
    };
    let Some(collection_record_index) = exact_local_record_index(record, backlink_at) else {
        return Ok(None);
    };
    Ok(Some(MeshCollectionOwnerRecord {
        owner: DesignMeshCollectionOwner::new(
            identity,
            source_offset(frame.start, backlink_at)
                .ok_or_else(|| malformed_frame("mesh-collection-owner", frame.entity_id))?,
        )
        .map_err(|_| malformed_frame("mesh-collection-owner", frame.entity_id))?,
        collection_record_index,
    }))
}

fn parse_mesh_texture_filename_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<(DesignMeshRecordIdentity, String), CodecError> {
    let identity = parse_typed_identity(
        bytes,
        frame,
        MESH_TEXTURE_FILENAME_TYPE_VERSION,
        MESH_TEXTURE_FILENAME_BASE_TYPE_GUID,
        "",
        "mesh-texture-filename",
    )?;
    let record = &bytes[frame.start..frame.end];
    if record.get(texture_filename::ZERO_RUN_10..texture_filename::BASENAME_CODE_UNIT_COUNT)
        != Some(&[0; 10])
    {
        return Err(malformed_frame("mesh-texture-filename", frame.entity_id));
    }
    let (filename, end) =
        lp_utf16_bounded(record, texture_filename::BASENAME_CODE_UNIT_COUNT, 1..=1024)
            .ok_or_else(|| malformed_frame("mesh-texture-filename", frame.entity_id))?;
    if end != record.len() {
        return Err(malformed_frame("mesh-texture-filename", frame.entity_id));
    }
    Ok((identity, filename))
}

fn unique_record_map<T>(
    records: Vec<T>,
    record_index: impl Fn(&T) -> u32,
    record_kind: &str,
) -> Result<HashMap<u32, T>, CodecError> {
    let mut out = HashMap::new();

    for record in records {
        let index = record_index(&record);
        if out.insert(index, record).is_some() {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D Design {record_kind} record index {index} is not unique"
            )));
        }
    }
    Ok(out)
}

fn typed_frame_map<'a>(
    frames: Vec<TypedPrimaryFrame<'a>>,
    record_kind: &str,
) -> Result<HashMap<u32, TypedPrimaryFrame<'a>>, CodecError> {
    let mut out = HashMap::new();

    for frame in frames {
        let index = u32::try_from(frame.entity_id).map_err(|_| {
            crate::design::text::malformed_design(format_args!(
                "F3D Design {record_kind} entity {} exceeds the indexed-record domain",
                frame.entity_id
            ))
        })?;
        if out.insert(index, frame).is_some() {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D Design {record_kind} record index {index} is not unique"
            )));
        }
    }
    Ok(out)
}

fn malformed_mesh_graph(stream: &str, invariant: &str) -> CodecError {
    crate::design::text::malformed_design(format_args!(
        "F3D Design mesh feature graph violates `{invariant}` in {stream}"
    ))
}

fn mesh_diagnostic(message: std::fmt::Arguments<'_>) -> CodecError {
    CodecError::Malformed(message.to_string())
}

fn mesh_collection_indices(collections: &[MeshCollectionRecord]) -> HashSet<u32> {
    let mut indices = HashSet::new();
    for collection in collections {
        indices.insert(collection.collection.record().record_index());
    }
    indices
}

fn mesh_filename_entries(
    filenames: &[MeshTextureFilenameEntry],
) -> HashMap<String, &MeshTextureFilenameEntry> {
    let mut entries = HashMap::new();
    for entry in filenames {
        entries.insert(entry.resource_guid.as_str().to_ascii_uppercase(), entry);
    }
    entries
}

fn parse_mesh_design_records<F>(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    source_entry_name: &str,
    asset_for_filename: &mut F,
) -> Result<Vec<DesignMeshFeature>, CodecError>
where
    F: FnMut(&str) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError>,
{
    let stream = crate::ids::native_scope(source_entry_name);
    let records = IndexedRecordOffsets::build(bytes);
    let collection_frames =
        typed_primary_frames(bytes, meta, MESH_COLLECTION_TYPE_GUID, "mesh-collection")?;
    if collection_frames.is_empty() {
        return Ok(Vec::new());
    }
    let collections = collection_frames
        .into_iter()
        .map(|frame| parse_mesh_collection_record(bytes, meta, frame))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if collections.is_empty() {
        return Ok(Vec::new());
    }
    let mut entry_names = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_ENTRY_NAME_TYPE_GUID, "mesh-entry-name")?
            .into_iter()
            .map(|frame| parse_mesh_entry_name_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.entry.record().record_index(),
        "mesh-entry-name",
    )?;
    let mut guids = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_GUID_TYPE_GUID, "mesh-GUID")?
            .into_iter()
            .map(|frame| parse_mesh_guid_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.guid.record().record_index(),
        "mesh-GUID",
    )?;
    let mut bodies = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_BODY_TYPE_GUID, "mesh-body")?
            .into_iter()
            .map(|frame| parse_mesh_body_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.placement.record().record_index(),
        "mesh-body",
    )?;
    let collection_record_indices = mesh_collection_indices(&collections);
    let mut texture_tables = unique_record_map(
        typed_primary_frames(
            bytes,
            meta,
            MESH_TEXTURE_TABLE_TYPE_GUID,
            "mesh-texture-table",
        )?
        .into_iter()
        .map(|frame| parse_mesh_texture_table_record(bytes, frame))
        .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index(),
        "mesh-texture-table",
    )?;
    let mut wrappers = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_WRAPPER_TYPE_GUID, "mesh-wrapper")?
            .into_iter()
            .map(|frame| parse_mesh_wrapper_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.identity.record_index(),
        "mesh-wrapper",
    )?;
    let mut scopes = unique_record_map(
        typed_primary_frames(
            bytes,
            meta,
            MESH_FEATURE_SCOPE_TYPE_GUID,
            "mesh-feature-scope",
        )?
        .into_iter()
        .map(|frame| parse_mesh_scope_record(bytes, meta, &records, frame))
        .collect::<Result<Vec<_>, _>>()?,
        |record| record.scope.record().record_index(),
        "mesh-feature-scope",
    )?;
    let mut states = unique_record_map(
        typed_primary_frames(bytes, meta, MESH_SCENE_STATE_TYPE_GUID, "mesh-scene-state")?
            .into_iter()
            .map(|frame| parse_mesh_scene_state_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.record().record_index(),
        "mesh-scene-state",
    )?;
    let mut scene_nodes = unique_record_map(
        typed_primary_frames(bytes, meta, SCENE_NODE_TYPE_GUID, "mesh-scene-node")?
            .into_iter()
            .map(|frame| parse_scene_node_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?,
        |record| record.node.record_index(),
        "mesh-scene-node",
    )?;
    let mut scene_auxiliary_frames = typed_frame_map(
        typed_primary_frames(
            bytes,
            meta,
            SCENE_AUXILIARY_TYPE_GUID,
            "mesh-scene-auxiliary",
        )?,
        "mesh-scene-auxiliary",
    )?;
    let filename_frames = typed_frame_map(
        typed_primary_frames(
            bytes,
            meta,
            MESH_TEXTURE_FILENAME_TYPE_GUID,
            "mesh-texture-filename",
        )?,
        "mesh-texture-filename",
    )?;
    let mut owner_records = Vec::new();
    for frame in typed_primary_frames(
        bytes,
        meta,
        MESH_COLLECTION_OWNER_TYPE_GUID,
        "mesh-collection-owner",
    )? {
        if let Some(owner) = parse_mesh_collection_owner_record(bytes, frame)? {
            if collection_record_indices.contains(&owner.collection_record_index) {
                owner_records.push(owner);
            }
        }
    }
    let mut collection_owners = unique_record_map(
        owner_records,
        |record| record.owner.record().record_index(),
        "mesh-collection-owner",
    )?;
    let body_owner_frames = typed_frame_map(
        typed_primary_frames(bytes, meta, MESH_BODY_OWNER_TYPE_GUID, "mesh-body-owner")?,
        "mesh-body-owner",
    )?;

    let mut features = Vec::new();
    for collection in collections {
        let stream_error = |invariant| malformed_mesh_graph(&stream, invariant);
        let mut candidate_scopes = scopes
            .iter()
            .filter(|(_, scope)| scope.body_records == collection.body_records)
            .filter(|(scope_index, _)| {
                collection.body_records.iter().all(|body_index| {
                    bodies.get(body_index).is_some_and(|body| {
                        body.scope_record_index == **scope_index
                            && body.collection_record_index
                                == collection.collection.record().record_index()
                    })
                })
            })
            .map(|(record_index, _)| *record_index);
        let candidate = candidate_scopes.next();
        let second_candidate = candidate_scopes.next();
        let Some(scope_record_index) = candidate.filter(|_| second_candidate.is_none()) else {
            let mut scope_lists = Vec::new();
            for (index, scope) in &scopes {
                let mut body_records = Vec::new();
                body_records.extend_from_slice(&scope.body_records);
                scope_lists.push((*index, body_records));
            }
            scope_lists.sort_unstable_by_key(|(index, _)| *index);
            let mut body_links = Vec::new();
            for index in &collection.body_records {
                if let Some(body) = bodies.get(index) {
                    body_links.push((
                        *index,
                        body.scope_record_index,
                        body.collection_record_index,
                    ));
                }
            }
            return Err(mesh_diagnostic(format_args!(
                "F3D Design mesh feature graph violates `each mesh collection has exactly one scope with the same ordered body list` in {stream}: collection {} bodies {:?}, scope lists {:?}, body links {:?}",
                collection.collection.record().record_index(),
                collection.body_records,
                scope_lists,
                body_links,
            )));
        };
        let scope = scopes.remove(&scope_record_index).ok_or_else(|| {
            stream_error("a mesh feature scope belongs to exactly one mesh collection")
        })?;
        let texture_table = texture_tables
            .remove(&collection.texture_table_record_index)
            .ok_or_else(|| {
                stream_error("a mesh texture table belongs to exactly one mesh collection")
            })?;
        let collection_owner = collection_owners
            .remove(&collection.owner_record_index)
            .filter(|owner| {
                owner.collection_record_index == collection.collection.record().record_index()
            })
            .ok_or_else(|| {
                stream_error("each mesh collection has one unused owner with a reciprocal backlink")
            })?;

        let mut filename_entries = mesh_filename_entries(&texture_table.filenames);
        let mut textures = Vec::new();
        for flag in &texture_table.flags {
            let filename_entry = filename_entries
                .remove(&flag.resource_guid.as_str().to_ascii_uppercase())
                .ok_or_else(|| {
                    stream_error("texture flag and filename maps have identical GUID keys")
                })?;
            let filename_frame = filename_frames
                .get(&filename_entry.filename_record_index)
                .copied()
                .ok_or_else(|| {
                    stream_error("each texture filename reference targets a filename record")
                })?;
            let (filename_record, filename) =
                parse_mesh_texture_filename_record(bytes, filename_frame)?;
            let (archive_entry_name, asset) = asset_for_filename(&filename)?;
            textures.push(DesignMeshTextureResource {
                ordinal: flag.ordinal,
                resource_guid: flag.resource_guid.clone(),
                flags: flag.value,
                filename_ordinal: filename_entry.ordinal,
                file: crate::records::mesh::DesignMeshTextureFile::new(
                    filename_record,
                    &filename,
                    archive_entry_name,
                )
                .map_err(|message| malformed_mesh_graph(&stream, &message))?,
                asset,
            });
        }
        if !filename_entries.is_empty() {
            return Err(stream_error(
                "texture flag and filename maps have identical GUID keys",
            ));
        }

        let mut feature_bodies = Vec::new();
        for body_record_index in &collection.body_records {
            let body = bodies.remove(body_record_index).ok_or_else(|| {
                stream_error("each collection body reference targets one unused mesh body")
            })?;
            let wrapper = wrappers
                .remove(&body.wrapper_record_index)
                .filter(|wrapper| {
                    wrapper.body_record_index == body.placement.record().record_index()
                })
                .ok_or_else(|| stream_error("each mesh body has one unused reciprocal wrapper"))?;
            let guid = guids
                .remove(&body.guid_record_index)
                .ok_or_else(|| stream_error("each mesh body has one unused GUID record"))?;
            let entry_name = entry_names
                .remove(&guid.entry_name_record_index)
                .filter(|entry| entry.guid_record_index == guid.guid.record().record_index())
                .ok_or_else(|| {
                    stream_error("each mesh GUID has one unused reciprocal entry-name record")
                })?;
            let scene_node = scene_nodes
                .remove(&body.scene_node_record_index)
                .ok_or_else(|| stream_error("each mesh body has one unused Scene node"))?;
            let scene_state = states
                .remove(&scene_node.state_record_index)
                .ok_or_else(|| stream_error("each Scene node has one unused Scene state"))?;
            let scene_auxiliary_frame = scene_auxiliary_frames
                .remove(&scene_node.auxiliary_record_index)
                .ok_or_else(|| {
                    stream_error("each Scene node has one unused Scene auxiliary record")
                })?;
            let scene_auxiliary = parse_typed_identity(
                bytes,
                scene_auxiliary_frame,
                SCENE_AUXILIARY_TYPE_VERSION,
                SCENE_AUXILIARY_BASE_TYPE_GUID,
                SCENE_MODULE,
                "mesh-scene-auxiliary",
            )?;
            let body_owner_frame = body_owner_frames
                .get(&body.owner_record_index)
                .copied()
                .ok_or_else(|| stream_error("each mesh body references a typed Body owner"))?;
            let body_owner = parse_typed_identity(
                bytes,
                body_owner_frame,
                MESH_BODY_OWNER_TYPE_VERSION,
                MESH_BODY_OWNER_BASE_TYPE_GUID,
                "Body",
                "mesh-body-owner",
            )?;
            feature_bodies.push(DesignMeshBody {
                placement: body.placement,
                entry: entry_name.entry,
                guid: guid.guid,
                wrapper_record: wrapper.identity,
                scene_state,
                scene_node: scene_node.node,
                scene_auxiliary_record: scene_auxiliary,
                owner_record: body_owner,
                container_mesh_uuid: None,
                tessellation_id: None,
            });
        }
        let scope_offset = usize::try_from(scope.scope.record().byte_offset()).map_err(|_| {
            stream_error("mesh feature scope byte offsets fit the platform address domain")
        })?;
        features.push(
            DesignMeshFeature::new(
                mesh_feature_id(&stream, scope_offset),
                scope.scope,
                collection.collection,
                DesignMeshTextureTable::new(texture_table.identity, textures).map_err(|error| {
                    match error {
                        CodecError::Malformed(message) => malformed_mesh_graph(&stream, &message),
                        other => other,
                    }
                })?,
                collection_owner.owner,
                feature_bodies,
            )
            .map_err(|message| malformed_mesh_graph(&stream, &message))?,
        );
    }
    if !entry_names.is_empty()
        || !guids.is_empty()
        || !bodies.is_empty()
        || !texture_tables.is_empty()
        || !wrappers.is_empty()
        || !scopes.is_empty()
    {
        return Err(malformed_mesh_graph(
            &stream,
            "all typed mesh graph records belong to exactly one feature",
        ));
    }
    Ok(features)
}

fn decode_mesh_design_records(
    scan: &ContainerScan,
) -> Result<Vec<Vec<DesignMeshFeature>>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let mut asset_for_filename = |filename: &str| mesh_image_asset(scan, filename);
        let records = parse_mesh_design_records(
            scan.entry_bytes(&entry.name)?,
            &meta,
            &entry.name,
            &mut asset_for_filename,
        )?;
        if !records.is_empty() {
            out.push(records);
        }
    }
    Ok(out)
}

fn mesh_image_asset(
    scan: &ContainerScan,
    filename: &str,
) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError> {
    let mut matches = scan.entries.iter().filter(|candidate| {
        scan.is_design_asset_entry(candidate, ContainerRole::Image)
            && candidate.name.rsplit('/').next() == Some(filename)
    });
    let (Some(asset), None) = (matches.next(), matches.next()) else {
        return Err(crate::design::text::malformed_design(format_args!(
            "F3D Design mesh texture `{filename}` does not resolve to one embedded image"
        )));
    };
    Ok((
        asset.name.clone(),
        crate::ids::neutral_asset_id(&asset.name),
    ))
}

fn flatten_mesh_features(design_records: Vec<Vec<DesignMeshFeature>>) -> Vec<DesignMeshFeature> {
    design_records.into_iter().flatten().collect()
}

fn mesh_feature_id(stream: &str, offset: usize) -> String {
    format!("{stream}:design-mesh-feature#{offset}")
}

fn resolve_mesh_body(
    records: &[Vec<DesignMeshFeature>],
    entry_name: &str,
    fusion_uuid: &str,
) -> Option<(usize, usize, usize)> {
    let mut joined = None;
    for (design_ordinal, design) in records.iter().enumerate() {
        for (feature_ordinal, feature) in design.iter().enumerate() {
            for (body_ordinal, body) in feature.bodies().iter().enumerate() {
                if body.tessellation_id.is_none()
                    && body.entry.name() == entry_name
                    && body.guid.value().eq_ignore_ascii_case(fusion_uuid)
                {
                    if joined.is_some() {
                        return None;
                    }
                    joined = Some((design_ordinal, feature_ordinal, body_ordinal));
                }
            }
        }
    }
    joined
}

/// Decode every mesh body: one per `.paramesh` container joined to the
/// mesh-body record that names its GUID record.
pub(crate) fn decode_mesh_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<MeshDecode, CodecError> {
    let mut design_records = decode_mesh_design_records(scan)?;
    let mut outcomes = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, ContainerRole::Paramesh))
    {
        let container = match scan
            .entry_bytes(&entry.name)
            .and_then(|bytes| decode_mesh_container(ctx, bytes))
        {
            Ok(container) => container,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                outcomes.push(MeshContainerOutcome::Failed {
                    entry_name: entry.name.clone(),
                    error,
                });
                continue;
            }
        };
        let name = &entry.name;
        let base = name.rsplit('/').next().unwrap_or(name);
        let Some((design_ordinal, feature_ordinal, body_ordinal)) =
            resolve_mesh_body(&design_records, base, &container.fusion_uuid)
        else {
            outcomes.push(MeshContainerOutcome::Unjoined {
                entry_name: entry.name.clone(),
            });
            continue;
        };
        design_records[design_ordinal][feature_ordinal].bodies_mut()[body_ordinal]
            .container_mesh_uuid = Some(container.mesh_uuid.clone());
        let body = &design_records[design_ordinal][feature_ordinal].bodies()[body_ordinal];
        let projected = match MeshBody::from_container(
            &entry.name,
            body.placement.record().byte_offset(),
            body.placement.transform(),
            container,
        ) {
            Ok(projected) => projected,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                outcomes.push(MeshContainerOutcome::Failed {
                    entry_name: entry.name.clone(),
                    error,
                });
                continue;
            }
        };
        design_records[design_ordinal][feature_ordinal].bodies_mut()[body_ordinal]
            .tessellation_id = Some(projected.id.clone());
        outcomes.push(MeshContainerOutcome::Joined(projected));
    }
    for body in design_records
        .iter()
        .flatten()
        .flat_map(crate::records::mesh::DesignMeshFeature::bodies)
        .filter(|body| body.tessellation_id.is_none())
    {
        outcomes.push(MeshContainerOutcome::Missing {
            entry_name: body.entry.name().to_owned(),
        });
    }
    let features = flatten_mesh_features(design_records);
    Ok(MeshDecode { outcomes, features })
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    mod placement;
    use super::{
        parse_mesh_collection_owner_record, parse_mesh_scene_state_record,
        parse_mesh_texture_table_record, parse_mesh_wrapper_record, parse_scene_node_record,
        resolve_mesh_body, MeshBody, COMMON_DATA_MODULE, DATA_MODEL_MODULE, FUSION_MODULE,
        MATRIX_BYTES, MESH_BODY_BASE_TYPE_GUID, MESH_BODY_OWNER_BASE_TYPE_GUID,
        MESH_BODY_OWNER_TYPE_GUID, MESH_BODY_OWNER_TYPE_VERSION, MESH_BODY_TYPE_GUID,
        MESH_BODY_TYPE_VERSION, MESH_COLLECTION_BASE_BASE_TYPE_GUID,
        MESH_COLLECTION_BASE_TYPE_GUID, MESH_COLLECTION_BASE_TYPE_VERSION,
        MESH_COLLECTION_OWNER_BASE_TYPE_GUID, MESH_COLLECTION_OWNER_TYPE_GUID,
        MESH_COLLECTION_OWNER_TYPE_VERSIONS, MESH_COLLECTION_TYPE_GUID,
        MESH_COLLECTION_TYPE_VERSION, MESH_ENTRY_NAME_BASE_TYPE_GUID, MESH_ENTRY_NAME_TYPE_GUID,
        MESH_ENTRY_NAME_TYPE_VERSION, MESH_FEATURE_SCOPE_BASE_TYPE_GUID,
        MESH_FEATURE_SCOPE_TYPE_GUID, MESH_FEATURE_SCOPE_TYPE_VERSION, MESH_GUID_BASE_TYPE_GUID,
        MESH_GUID_TYPE_GUID, MESH_GUID_TYPE_VERSION, MESH_SCENE_STATE_BASE_TYPE_GUID,
        MESH_SCENE_STATE_TYPE_GUID, MESH_SCENE_STATE_TYPE_VERSION,
        MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID, MESH_SCOPE_BASE_RECORD_TYPE_GUID,
        MESH_SCOPE_BASE_RECORD_TYPE_VERSION, MESH_TEXTURE_FILENAME_BASE_TYPE_GUID,
        MESH_TEXTURE_FILENAME_TYPE_GUID, MESH_TEXTURE_FILENAME_TYPE_VERSION,
        MESH_TEXTURE_TABLE_BASE_TYPE_GUID, MESH_TEXTURE_TABLE_TYPE_GUID,
        MESH_TEXTURE_TABLE_TYPE_VERSION, MESH_WRAPPER_BASE_TYPE_GUID, MESH_WRAPPER_TYPE_GUID,
        MESH_WRAPPER_TYPE_VERSION, PARAMESH_MODULE, SAME_SEGMENT_REFERENCE_BYTES,
        SCENE_AUXILIARY_BASE_TYPE_GUID, SCENE_AUXILIARY_TYPE_GUID, SCENE_AUXILIARY_TYPE_VERSION,
        SCENE_MODULE, SCENE_NODE_BASE_TYPE_GUID, SCENE_NODE_TYPE_GUID, SCENE_NODE_TYPE_VERSION,
    };
    use crate::design::decode::meta::{typed_primary_frames, TypedPrimaryFrame};
    use crate::design::test_support::{design_type, primary_record};
    use crate::layout::{
        paramesh_collection_owner_backlink_prefix as collection_owner,
        paramesh_collection_owner_v17 as collection_owner_v17,
        paramesh_mesh_body_join_prefix as mesh_body,
        paramesh_mesh_collection_base_prefix as mesh_collection_base,
        paramesh_mesh_collection_prefix as mesh_collection, paramesh_scene_node as scene_node,
        paramesh_scene_node_placed as placed_scene_node,
        paramesh_texture_table_prefix as texture_table,
    };
    use crate::paramesh::MeshContainer;
    use crate::test_support::{lp_ascii, lp_utf16};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::units::UnitVector3;

    fn parse_mesh_design_records<F>(
        bytes: &[u8],
        meta: &crate::metastream::MetaStream,
        source_entry_name: &str,
        asset_for_filename: &mut F,
    ) -> Result<Vec<crate::records::mesh::DesignMeshFeature>, CodecError>
    where
        F: FnMut(&str) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError>,
    {
        super::parse_mesh_design_records(bytes, meta, source_entry_name, asset_for_filename)
    }

    #[test]
    fn anisotropic_mesh_normal_preserves_orientation_without_cofactor_overflow() {
        for sign in [-1.0, 1.0] {
            let transform = crate::records::mesh::MeshAffineTransform::new([
                sign * 1e200,
                0.0,
                0.0,
                0.0,
                0.0,
                1e200,
                0.0,
                0.0,
                0.0,
                0.0,
                1e-200,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ])
            .unwrap();
            assert_eq!(
                *transform
                    .transform_normal(UnitVector3::Z_AXIS)
                    .unwrap()
                    .as_raw(),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, sign)
            );
        }
    }

    fn matrix(cells: [f64; 16]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(MATRIX_BYTES);
        for value in cells {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn push_indexed_header(bytes: &mut Vec<u8>, class_tag: u32, record_index: u32) {
        let class_tag = class_tag.to_string();
        assert_eq!(class_tag.len(), 3);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag.as_bytes());
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn put_reference(bytes: &mut [u8], at: usize, target: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 9].copy_from_slice(&u64::from(target).to_le_bytes());
        bytes[at + 9..at + 11].copy_from_slice(&[0, 0]);
    }

    fn push_reference(bytes: &mut Vec<u8>, target: u32) {
        let at = bytes.len();
        bytes.resize(at + SAME_SEGMENT_REFERENCE_BYTES, 0);
        put_reference(bytes, at, target);
    }

    fn mesh_entry_record(
        class_tag: u32,
        record_index: u32,
        guid_record_index: u32,
        entry_name: &str,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        push_reference(&mut bytes, guid_record_index);
        lp_utf16(&mut bytes, entry_name);
        bytes
    }

    fn mesh_guid_record(
        class_tag: u32,
        record_index: u32,
        entry_name_record_index: u32,
        fusion_uuid: &str,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 21]);
        lp_ascii(&mut bytes, fusion_uuid);
        push_reference(&mut bytes, entry_name_record_index);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    #[allow(clippy::too_many_arguments)] // One argument per serialized body-graph reference.
    fn mesh_body_record(
        class_tag: u32,
        record_index: u32,
        guid_record_index: u32,
        scope_record_index: u32,
        wrapper_record_index: u32,
        owner_record_index: u32,
        scene_node_record_index: u32,
        collection_record_index: u32,
    ) -> Vec<u8> {
        let identity = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.resize(600, 0);
        bytes[mesh_body::FIRST_TRANSFORM..mesh_body::FIRST_TRANSFORM + MATRIX_BYTES]
            .copy_from_slice(&matrix(identity));
        bytes[mesh_body::SECOND_TRANSFORM..mesh_body::SECOND_TRANSFORM + MATRIX_BYTES]
            .copy_from_slice(&matrix(identity));
        put_reference(
            &mut bytes,
            mesh_body::FEATURE_SCOPE_REFERENCE,
            scope_record_index,
        );
        put_reference(
            &mut bytes,
            mesh_body::WRAPPER_REFERENCE,
            wrapper_record_index,
        );
        put_reference(
            &mut bytes,
            mesh_body::BODY_OWNER_REFERENCE,
            owner_record_index,
        );
        put_reference(
            &mut bytes,
            mesh_body::CONTAINER_GUID_REFERENCE,
            guid_record_index,
        );
        put_reference(
            &mut bytes,
            mesh_body::SCENE_NODE_REFERENCE,
            scene_node_record_index,
        );
        let tail_at = bytes.len() - SAME_SEGMENT_REFERENCE_BYTES;
        put_reference(&mut bytes, tail_at, collection_record_index);
        bytes
    }

    fn push_scene_footer(bytes: &mut Vec<u8>) {
        bytes.push(1);
        for index in 0..49 {
            bytes.push(match index {
                6 | 14 | 22 | 30 | 38 | 46 => 0xef,
                31 | 39 | 47 => 0x7f,
                48 => 0x01,
                _ => 0xff,
            });
        }
    }

    /// Prefix and owner forms of a mesh collection.
    #[derive(Clone, Copy)]
    enum CollectionForm {
        Plain,
        /// A zero u32 before the owner reference.
        OwnerGap,
        /// Flag byte `01` at offset 20 and this u32 at offset 25.
        Flagged(u32),
    }

    fn mesh_collection_record(
        class_tag: u32,
        base_class_tag: u32,
        record_index: u32,
        texture_table_record_index: u32,
        body_record_indices: &[u32],
        owner_record_index: u32,
        form: CollectionForm,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(u8::from(matches!(form, CollectionForm::Flagged(_))));
        bytes.extend_from_slice(
            &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        if let CollectionForm::Flagged(word) = form {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, 1]);
        push_reference(&mut bytes, texture_table_record_index);
        push_indexed_header(&mut bytes, base_class_tag, record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.extend_from_slice(
            &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        for body in body_record_indices {
            push_reference(&mut bytes, *body);
        }
        if matches!(form, CollectionForm::OwnerGap) {
            bytes.extend_from_slice(&[0; 4]);
        }
        push_reference(&mut bytes, owner_record_index);
        bytes
    }

    fn mesh_texture_table_record(
        class_tag: u32,
        record_index: u32,
        flags: &[(&str, u32)],
        filenames: &[(&str, u32)],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(
            &(u32::try_from(flags.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for (guid, value) in flags {
            lp_ascii(&mut bytes, guid);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(
            &(u32::try_from(filenames.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for (guid, target) in filenames {
            lp_ascii(&mut bytes, guid);
            push_reference(&mut bytes, *target);
        }
        bytes
    }

    fn mesh_wrapper_record(class_tag: u32, record_index: u32, body_record_index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        push_reference(&mut bytes, body_record_index);
        bytes.extend_from_slice(&[0; 8]);
        bytes
    }

    fn mesh_scene_state_record(class_tag: u32, record_index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 34]);
        push_scene_footer(&mut bytes);
        bytes
    }

    fn mesh_scene_node_record(
        class_tag: u32,
        record_index: u32,
        state_record_index: u32,
        auxiliary_record_index: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 14]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        push_reference(&mut bytes, state_record_index);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        push_reference(&mut bytes, auxiliary_record_index);
        bytes.extend_from_slice(&[0; 24]);
        push_scene_footer(&mut bytes);
        bytes
    }

    fn placed_mesh_scene_node_record(
        class_tag: u32,
        record_index: u32,
        state_record_index: u32,
        auxiliary_record_index: u32,
        transform: [f64; 16],
        bounds: [f64; 6],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 14]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        push_reference(&mut bytes, state_record_index);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        push_reference(&mut bytes, auxiliary_record_index);
        bytes.extend_from_slice(&[0; 25]);
        for value in transform {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in bounds {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(1);
        bytes
    }

    fn mesh_scope_record(
        class_tag: u32,
        base_class_tag: u32,
        record_index: u32,
        body_record_indices: &[u32],
        member_record_index: u32,
        owner_record_index: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(
            &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        for body in body_record_indices {
            push_reference(&mut bytes, *body);
        }
        bytes.extend_from_slice(&[0; 24]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        push_reference(&mut bytes, member_record_index);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, "Base Mesh Feature");
        let mut tail = [0; 78];
        tail[..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        push_indexed_header(&mut bytes, base_class_tag, record_index);
        bytes.extend_from_slice(&[0; 8]);
        push_reference(&mut bytes, owner_record_index);
        bytes
    }

    fn collection_owner_record(
        class_tag: u32,
        record_index: u32,
        collection_record_index: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.resize(collection_owner::COLLECTION_BACKLINK, 0);
        push_reference(&mut bytes, collection_record_index);
        bytes
    }

    fn identity_record(class_tag: u32, record_index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes
    }

    fn texture_filename_record(class_tag: u32, record_index: u32, filename: &str) -> Vec<u8> {
        let mut bytes = identity_record(class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, filename);
        bytes
    }

    fn push_primary_frame(
        bytes: &mut Vec<u8>,
        records: &mut Vec<crate::metastream::RecordIndexEntry>,
        entity_id: u32,
        frame: Vec<u8>,
    ) {
        records.push(primary_record(u64::from(entity_id), bytes.len()));
        bytes.extend(frame);
    }

    struct SyntheticMeshGraph {
        bytes: Vec<u8>,
        meta: crate::metastream::MetaStream,
    }

    fn no_texture_asset(
        _filename: &str,
    ) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError> {
        panic!("empty texture table must not resolve an asset")
    }

    fn synthetic_mesh_graph_with_body_count(
        with_textures: bool,
        body_count: usize,
    ) -> SyntheticMeshGraph {
        synthetic_mesh_graph_with_collection(with_textures, body_count, CollectionForm::Plain)
    }

    fn synthetic_mesh_graph_with_collection(
        with_textures: bool,
        body_count: usize,
        form: CollectionForm,
    ) -> SyntheticMeshGraph {
        const ENTRY: u32 = 102;
        const GUID: u32 = 103;
        const BODY: u32 = 104;
        const STATE: u32 = 105;
        const AUXILIARY: u32 = 106;
        const NODE: u32 = 107;
        const WRAPPER: u32 = 108;
        const SCOPE: u32 = 109;
        const COLLECTION_OWNER: u32 = 110;
        const BODY_OWNER: u32 = 111;
        const SCOPE_OWNER: u32 = 112;
        const FILENAME_A: u32 = 113;
        const FILENAME_B: u32 = 114;
        const SECOND_ENTRY: u32 = 115;
        const SECOND_GUID: u32 = 116;
        const SECOND_BODY: u32 = 117;
        const SECOND_STATE: u32 = 118;
        const SECOND_AUXILIARY: u32 = 119;
        const SECOND_NODE: u32 = 120;
        const SECOND_WRAPPER: u32 = 121;
        const COLLECTION: u32 = 100;
        const TEXTURES: u32 = 101;
        const ENTRIES: [u32; 2] = [ENTRY, SECOND_ENTRY];
        const GUIDS: [u32; 2] = [GUID, SECOND_GUID];
        const BODIES: [u32; 2] = [BODY, SECOND_BODY];
        const STATES: [u32; 2] = [STATE, SECOND_STATE];
        const AUXILIARIES: [u32; 2] = [AUXILIARY, SECOND_AUXILIARY];
        const NODES: [u32; 2] = [NODE, SECOND_NODE];
        const WRAPPERS: [u32; 2] = [WRAPPER, SECOND_WRAPPER];
        const ENTRY_NAMES: [&str; 2] = [
            "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh",
            "ParaMeshGeometry.66666666-7777-4888-8999-AAAAAAAAAAAA.paramesh",
        ];
        const FUSION_UUIDS: [&str; 2] = [
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
            "BBBBBBBB-CCCC-4DDD-8EEE-FFFFFFFFFFFF",
        ];
        const RESOURCE_A: &str = "10000000-0000-4000-8000-000000000001";
        const RESOURCE_B: &str = "20000000-0000-4000-8000-000000000002";

        const TAG_ENTRY: u32 = 256;
        const TAG_GUID: u32 = 257;
        const TAG_BODY: u32 = 258;
        const TAG_COLLECTION: u32 = 259;
        const TAG_COLLECTION_BASE: u32 = 260;
        const TAG_TEXTURES: u32 = 261;
        const TAG_WRAPPER: u32 = 262;
        const TAG_SCOPE: u32 = 263;
        const TAG_SCOPE_BASE: u32 = 264;
        const TAG_STATE: u32 = 265;
        const TAG_NODE: u32 = 266;
        const TAG_AUXILIARY: u32 = 267;
        const TAG_COLLECTION_OWNER: u32 = 268;
        const TAG_BODY_OWNER: u32 = 269;
        const TAG_FILENAME: u32 = 270;

        assert!(body_count <= 2);
        let entity_ids = |indices: &[u32]| {
            indices[..body_count]
                .iter()
                .copied()
                .map(u64::from)
                .collect()
        };

        let filename_ids = if with_textures {
            vec![u64::from(FILENAME_A), u64::from(FILENAME_B)]
        } else {
            Vec::new()
        };
        let types = vec![
            design_type(
                MESH_ENTRY_NAME_TYPE_GUID,
                Some(MESH_ENTRY_NAME_BASE_TYPE_GUID),
                MESH_ENTRY_NAME_TYPE_VERSION,
                PARAMESH_MODULE,
                entity_ids(&ENTRIES),
            ),
            design_type(
                MESH_GUID_TYPE_GUID,
                Some(MESH_GUID_BASE_TYPE_GUID),
                MESH_GUID_TYPE_VERSION,
                PARAMESH_MODULE,
                entity_ids(&GUIDS),
            ),
            design_type(
                MESH_BODY_TYPE_GUID,
                Some(MESH_BODY_BASE_TYPE_GUID),
                MESH_BODY_TYPE_VERSION,
                PARAMESH_MODULE,
                entity_ids(&BODIES),
            ),
            design_type(
                MESH_COLLECTION_TYPE_GUID,
                Some(MESH_COLLECTION_BASE_TYPE_GUID),
                MESH_COLLECTION_TYPE_VERSION,
                PARAMESH_MODULE,
                vec![u64::from(COLLECTION)],
            ),
            design_type(
                MESH_COLLECTION_BASE_TYPE_GUID,
                Some(MESH_COLLECTION_BASE_BASE_TYPE_GUID),
                MESH_COLLECTION_BASE_TYPE_VERSION,
                COMMON_DATA_MODULE,
                Vec::new(),
            ),
            design_type(
                MESH_TEXTURE_TABLE_TYPE_GUID,
                Some(MESH_TEXTURE_TABLE_BASE_TYPE_GUID),
                MESH_TEXTURE_TABLE_TYPE_VERSION,
                PARAMESH_MODULE,
                vec![u64::from(TEXTURES)],
            ),
            design_type(
                MESH_WRAPPER_TYPE_GUID,
                Some(MESH_WRAPPER_BASE_TYPE_GUID),
                MESH_WRAPPER_TYPE_VERSION,
                PARAMESH_MODULE,
                entity_ids(&WRAPPERS),
            ),
            design_type(
                MESH_FEATURE_SCOPE_TYPE_GUID,
                Some(MESH_FEATURE_SCOPE_BASE_TYPE_GUID),
                MESH_FEATURE_SCOPE_TYPE_VERSION,
                FUSION_MODULE,
                vec![u64::from(SCOPE)],
            ),
            design_type(
                MESH_SCOPE_BASE_RECORD_TYPE_GUID,
                Some(MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID),
                MESH_SCOPE_BASE_RECORD_TYPE_VERSION,
                DATA_MODEL_MODULE,
                Vec::new(),
            ),
            design_type(
                MESH_SCENE_STATE_TYPE_GUID,
                Some(MESH_SCENE_STATE_BASE_TYPE_GUID),
                MESH_SCENE_STATE_TYPE_VERSION,
                SCENE_MODULE,
                entity_ids(&STATES),
            ),
            design_type(
                SCENE_NODE_TYPE_GUID,
                Some(SCENE_NODE_BASE_TYPE_GUID),
                SCENE_NODE_TYPE_VERSION,
                SCENE_MODULE,
                entity_ids(&NODES),
            ),
            design_type(
                SCENE_AUXILIARY_TYPE_GUID,
                Some(SCENE_AUXILIARY_BASE_TYPE_GUID),
                SCENE_AUXILIARY_TYPE_VERSION,
                SCENE_MODULE,
                entity_ids(&AUXILIARIES),
            ),
            design_type(
                MESH_COLLECTION_OWNER_TYPE_GUID,
                Some(MESH_COLLECTION_OWNER_BASE_TYPE_GUID),
                MESH_COLLECTION_OWNER_TYPE_VERSIONS[2],
                FUSION_MODULE,
                vec![u64::from(COLLECTION_OWNER)],
            ),
            design_type(
                MESH_BODY_OWNER_TYPE_GUID,
                Some(MESH_BODY_OWNER_BASE_TYPE_GUID),
                MESH_BODY_OWNER_TYPE_VERSION,
                "Body",
                vec![u64::from(BODY_OWNER)],
            ),
            design_type(
                MESH_TEXTURE_FILENAME_TYPE_GUID,
                Some(MESH_TEXTURE_FILENAME_BASE_TYPE_GUID),
                MESH_TEXTURE_FILENAME_TYPE_VERSION,
                "",
                filename_ids,
            ),
        ];
        assert_eq!(types.len(), 15);

        let mut bytes = Vec::new();
        let mut records = Vec::new();
        push_primary_frame(
            &mut bytes,
            &mut records,
            COLLECTION,
            mesh_collection_record(
                TAG_COLLECTION,
                TAG_COLLECTION_BASE,
                COLLECTION,
                TEXTURES,
                &BODIES[..body_count],
                COLLECTION_OWNER,
                form,
            ),
        );
        let flags = if with_textures {
            vec![(RESOURCE_A, 2), (RESOURCE_B, 258)]
        } else {
            Vec::new()
        };
        let filenames = if with_textures {
            vec![(RESOURCE_B, FILENAME_B), (RESOURCE_A, FILENAME_A)]
        } else {
            Vec::new()
        };
        push_primary_frame(
            &mut bytes,
            &mut records,
            TEXTURES,
            mesh_texture_table_record(TAG_TEXTURES, TEXTURES, &flags, &filenames),
        );
        if with_textures {
            push_primary_frame(
                &mut bytes,
                &mut records,
                FILENAME_A,
                texture_filename_record(TAG_FILENAME, FILENAME_A, "mesh-a.png"),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                FILENAME_B,
                texture_filename_record(TAG_FILENAME, FILENAME_B, "mesh-b.jpg"),
            );
        }
        for ordinal in 0..body_count {
            push_primary_frame(
                &mut bytes,
                &mut records,
                ENTRIES[ordinal],
                mesh_entry_record(
                    TAG_ENTRY,
                    ENTRIES[ordinal],
                    GUIDS[ordinal],
                    ENTRY_NAMES[ordinal],
                ),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                GUIDS[ordinal],
                mesh_guid_record(
                    TAG_GUID,
                    GUIDS[ordinal],
                    ENTRIES[ordinal],
                    FUSION_UUIDS[ordinal],
                ),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                STATES[ordinal],
                mesh_scene_state_record(TAG_STATE, STATES[ordinal]),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                AUXILIARIES[ordinal],
                identity_record(TAG_AUXILIARY, AUXILIARIES[ordinal]),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                NODES[ordinal],
                mesh_scene_node_record(
                    TAG_NODE,
                    NODES[ordinal],
                    STATES[ordinal],
                    AUXILIARIES[ordinal],
                ),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                WRAPPERS[ordinal],
                mesh_wrapper_record(TAG_WRAPPER, WRAPPERS[ordinal], BODIES[ordinal]),
            );
            push_primary_frame(
                &mut bytes,
                &mut records,
                BODIES[ordinal],
                mesh_body_record(
                    TAG_BODY,
                    BODIES[ordinal],
                    GUIDS[ordinal],
                    SCOPE,
                    WRAPPERS[ordinal],
                    BODY_OWNER,
                    NODES[ordinal],
                    COLLECTION,
                ),
            );
        }
        push_primary_frame(
            &mut bytes,
            &mut records,
            SCOPE,
            mesh_scope_record(
                TAG_SCOPE,
                TAG_SCOPE_BASE,
                SCOPE,
                &BODIES[..body_count],
                140,
                SCOPE_OWNER,
            ),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            COLLECTION_OWNER,
            collection_owner_record(TAG_COLLECTION_OWNER, COLLECTION_OWNER, COLLECTION),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            BODY_OWNER,
            identity_record(TAG_BODY_OWNER, BODY_OWNER),
        );
        SyntheticMeshGraph {
            bytes,
            meta: crate::metastream::MetaStream {
                types,
                records,
                secondary_records: Vec::new(),
            },
        }
    }

    fn synthetic_mesh_graph(with_textures: bool) -> SyntheticMeshGraph {
        synthetic_mesh_graph_with_body_count(with_textures, 1)
    }

    fn sole_typed_frame<'a>(
        graph: &'a SyntheticMeshGraph,
        type_guid: &str,
    ) -> TypedPrimaryFrame<'a> {
        let frames = typed_primary_frames(&graph.bytes, &graph.meta, type_guid, "test record")
            .expect("typed test frame");
        let [frame] = frames.as_slice() else {
            panic!("one typed test frame");
        };
        *frame
    }

    #[test]
    fn typed_mesh_feature_graph_closes_every_body_join() {
        const ENTRY_NAME: &str = "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh";
        const FUSION_UUID: &str = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
        let graph = synthetic_mesh_graph(false);
        let mut no_asset = no_texture_asset;
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("complete typed mesh graph");
        let [feature] = design.as_slice() else {
            panic!("one mesh feature");
        };
        assert_eq!(feature.scope().record().record_index(), 109);
        assert_eq!(feature.collection().record().record_index(), 100);
        assert_eq!(feature.texture_table.record().record_index(), 101);
        assert_eq!(
            feature
                .bodies()
                .iter()
                .map(|body| body.placement.record().record_index())
                .collect::<Vec<_>>(),
            [104]
        );
        assert!(feature.texture_table.resources().is_empty());
        let [body] = feature.bodies() else {
            panic!("one mesh body");
        };
        assert_eq!(body.entry.name(), ENTRY_NAME);
        assert_eq!(body.guid.value(), FUSION_UUID);
        assert_eq!(body.wrapper_record.record_index(), 108);
        assert_eq!(body.scene_state.record().record_index(), 105);
        assert_eq!(body.scene_node.record_index(), 107);
        assert_eq!(body.scene_auxiliary_record.record_index(), 106);
        assert_eq!(body.owner_record.record_index(), 111);
        assert_eq!(
            resolve_mesh_body(&[design], ENTRY_NAME, FUSION_UUID),
            Some((0, 0, 0))
        );
    }

    /// An empty collection can store a zero u32 before its owner reference; it is
    /// still an empty registry. A nonzero value there is not a collection.
    #[test]
    fn empty_mesh_collection_with_a_zero_word_before_its_owner_is_an_empty_registry() {
        let graph = synthetic_mesh_graph_with_collection(false, 0, CollectionForm::OwnerGap);
        let mut no_asset = no_texture_asset;
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("empty mesh registry");
        assert!(design.is_empty());

        let mut graph = synthetic_mesh_graph_with_collection(false, 0, CollectionForm::OwnerGap);
        let owner_gap = 38 + 11 + 9 + 4;
        graph.bytes[owner_gap] = 1;
        let mut no_asset = no_texture_asset;
        assert!(parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .is_err());
    }

    /// An empty collection can store a flag byte and a u32 that move the rest
    /// of its prefix by four bytes; a flagged collection with bodies is not a
    /// collection.
    #[test]
    fn flagged_empty_mesh_collection_is_an_empty_registry() {
        let graph = synthetic_mesh_graph_with_collection(false, 0, CollectionForm::Flagged(1));
        let mut no_asset = no_texture_asset;
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("flagged empty mesh registry");
        assert!(design.is_empty());

        let graph = synthetic_mesh_graph_with_collection(false, 1, CollectionForm::Flagged(1));
        let mut no_asset = no_texture_asset;
        assert!(parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .is_err());
    }

    #[test]
    fn empty_mesh_collection_does_not_enter_feature_graph() {
        let graph = synthetic_mesh_graph_with_body_count(false, 0);
        let mut no_asset = no_texture_asset;

        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("empty mesh registry");
        assert!(design.is_empty());
    }

    #[test]
    fn mesh_registrations_without_a_collection_do_not_form_a_graph() {
        let mut graph = synthetic_mesh_graph(false);
        let collection_type = graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_TYPE_GUID)
            })
            .expect("mesh-collection type");
        let entities = collection_type
            .entities
            .values()
            .copied()
            .collect::<Vec<_>>();
        let [collection_entity] = entities.as_slice() else {
            panic!("one mesh-collection entity");
        };
        let collection_entity = *collection_entity;
        collection_type.entities = crate::records::identity::ReferenceRun::unlocated(Vec::new());
        graph
            .meta
            .records
            .retain(|record| record.entity_id != collection_entity);
        let mut no_asset = no_texture_asset;

        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("no mesh collection");
        assert!(design.is_empty());
    }

    #[test]
    fn typed_mesh_feature_allows_bodies_to_share_one_body_owner() {
        let graph = synthetic_mesh_graph_with_body_count(false, 2);
        let mut no_asset = no_texture_asset;
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("two-body mesh feature graph");
        let [feature] = design.as_slice() else {
            panic!("one mesh feature");
        };
        let [first, second] = feature.bodies() else {
            panic!("two mesh bodies");
        };

        assert_eq!(
            feature
                .bodies()
                .iter()
                .map(|body| body.placement.record().record_index())
                .collect::<Vec<_>>(),
            [104, 117]
        );
        assert_eq!(first.owner_record, second.owner_record);
        assert_ne!(first.placement.record(), second.placement.record());
    }

    #[test]
    fn mesh_texture_maps_join_by_guid_independently_of_map_order() {
        let graph = synthetic_mesh_graph(true);
        let mut asset = |filename: &str| {
            let entry = format!("Synthetic/Textures/{filename}");
            Ok((entry.clone(), crate::ids::neutral_asset_id(&entry)))
        };
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut asset,
        )
        .expect("textured mesh graph");
        let textures = design[0].texture_table.resources();
        assert_eq!(textures.len(), 2);
        assert_eq!(textures[0].flags, 2);
        assert_eq!(textures[0].file.filename(), "mesh-a.png");
        assert_eq!(textures[0].filename_ordinal, 1);
        assert_eq!(textures[1].flags, 258);
        assert_eq!(textures[1].file.filename(), "mesh-b.jpg");
        assert_eq!(textures[1].filename_ordinal, 0);
    }

    #[test]
    fn mesh_image_asset_resolves_entry_and_identifier() {
        use std::io::{Cursor, Write};
        use zip::CompressionMethod;

        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png";
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"PNG").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let expected = crate::ids::neutral_asset_id(ENTRY);
            let admitted = super::mesh_image_asset(scan, "mark.png").unwrap();
            assert_eq!(admitted.0, ENTRY);
            assert_eq!(admitted.1, expected);
        });
    }

    #[test]
    fn mesh_texture_resources_can_share_one_filename_record() {
        let mut graph = synthetic_mesh_graph(true);
        let frames = typed_primary_frames(
            &graph.bytes,
            &graph.meta,
            MESH_TEXTURE_TABLE_TYPE_GUID,
            "mesh-texture-table",
        )
        .expect("texture-table frame");
        let [frame] = frames.as_slice() else {
            panic!("one texture-table frame");
        };
        let table =
            parse_mesh_texture_table_record(&graph.bytes, *frame).expect("original texture table");
        let [first, second] = table.filenames.as_slice() else {
            panic!("two texture resources");
        };
        let second_reference = usize::try_from(
            table.identity.byte_offset()
                + u64_from_index(texture_table::LEN)
                + u64_from_index(table.flags.len()) * 44
                + 4
                + u64::from(second.ordinal) * 51
                + 40,
        )
        .expect("test reference offset");
        put_reference(
            &mut graph.bytes,
            second_reference,
            first.filename_record_index,
        );
        let mut asset = |filename: &str| {
            let entry = format!("Synthetic/Textures/{filename}");
            Ok((entry.clone(), crate::ids::neutral_asset_id(&entry)))
        };

        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut asset,
        )
        .expect("shared texture filename record");
        let textures = design[0].texture_table.resources();
        assert_eq!(textures.len(), 2);
        assert_eq!(textures[0].file.record(), textures[1].file.record());
        assert_eq!(textures[0].asset, textures[1].asset);
    }

    #[test]
    fn mesh_graph_rejects_body_count_disagreement() {
        let mut graph = synthetic_mesh_graph(false);
        graph.bytes[mesh_collection::LEN + mesh_collection_base::BODY_COUNT
            ..mesh_collection::LEN + mesh_collection_base::BODY_COUNT + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        let mut no_asset = no_texture_asset;
        assert!(matches!(
            parse_mesh_design_records(
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_texture_table_rejects_duplicate_resource_keys() {
        const RESOURCE: &str = "10000000-0000-4000-8000-000000000001";
        let graph = synthetic_mesh_graph(false);
        let bytes = mesh_texture_table_record(
            261,
            101,
            &[(RESOURCE, 2), (RESOURCE, 258)],
            &[(RESOURCE, 113)],
        );
        let frame = TypedPrimaryFrame {
            entity_id: 101,
            start: 0,
            end: bytes.len(),
            design_type: &graph.meta.types[5],
        };
        assert!(matches!(
            { parse_mesh_texture_table_record(&bytes, frame) },
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_texture_table_decodes_one_flag_and_filename() {
        const RESOURCE: &str = "10000000-0000-4000-8000-000000000001";
        let graph = synthetic_mesh_graph(false);
        let bytes = mesh_texture_table_record(261, 101, &[(RESOURCE, 2)], &[(RESOURCE, 113)]);
        let frame = TypedPrimaryFrame {
            entity_id: 101,
            start: 0,
            end: bytes.len(),
            design_type: &graph.meta.types[5],
        };
        {
            let table = parse_mesh_texture_table_record(&bytes, frame).unwrap();
            assert_eq!(table.flags.len(), 1);
            assert_eq!(table.filenames.len(), 1);
        };
    }

    #[test]
    fn mesh_feature_identifier_matches_native_key() {
        let stream = crate::ids::native_scope("Synthetic/BulkStream.dat");
        {
            let id = super::mesh_feature_id(&stream, 100);
            assert_eq!(
                id,
                crate::ids::native_design_mesh_feature_id("Synthetic/BulkStream.dat", 100)
            );
        };
    }

    #[test]
    fn mesh_wrapper_rejects_a_truncated_exact_frame() {
        let graph = synthetic_mesh_graph(false);
        let bytes = mesh_wrapper_record(262, 108, 104);
        let frame = TypedPrimaryFrame {
            entity_id: 108,
            start: 0,
            end: bytes.len() - 1,
            design_type: &graph.meta.types[6],
        };
        assert!(matches!(
            parse_mesh_wrapper_record(&bytes, frame),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_scene_state_rejects_a_mutated_footer_mask() {
        let mut graph = synthetic_mesh_graph(false);
        let start = sole_typed_frame(&graph, MESH_SCENE_STATE_TYPE_GUID).start;
        graph.bytes[start + 52] ^= 1;
        let frame = sole_typed_frame(&graph, MESH_SCENE_STATE_TYPE_GUID);

        assert!(matches!(
            { parse_mesh_scene_state_record(&graph.bytes, frame) },
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_scene_node_rejects_a_mutated_fixed_lane() {
        let mut graph = synthetic_mesh_graph(false);
        let start = sole_typed_frame(&graph, SCENE_NODE_TYPE_GUID).start;
        graph.bytes[start + 44..start + 48].copy_from_slice(&4u32.to_le_bytes());
        let frame = sole_typed_frame(&graph, SCENE_NODE_TYPE_GUID);

        assert!(matches!(
            parse_scene_node_record(&graph.bytes, frame),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_scene_node_transfers_finite_footer_bounds() {
        let mut graph = synthetic_mesh_graph(false);
        let start = sole_typed_frame(&graph, SCENE_NODE_TYPE_GUID).start;
        let payload_at = start + scene_node::FOOTER_MARKER + 1;
        let values: [f64; 6] = [1.0, 2.0, 3.0, -4.0, -5.0, -6.0];
        for (ordinal, value) in values.iter().enumerate() {
            let at = payload_at + ordinal * 8;
            graph.bytes[at..at + 8].copy_from_slice(&(*value).to_le_bytes());
        }
        graph.bytes[payload_at + 48] = 1;
        let frame = sole_typed_frame(&graph, SCENE_NODE_TYPE_GUID);

        let parsed = parse_scene_node_record(&graph.bytes, frame).expect("finite Scene bounds");
        let bounds = parsed.node.bounds().expect("present bounds");
        assert_eq!(bounds.maximum(), [1.0, 2.0, 3.0]);
        assert_eq!(bounds.minimum(), [-4.0, -5.0, -6.0]);
        assert_eq!(
            parsed.node.bounds_offsets(),
            [
                u64::try_from(payload_at).unwrap(),
                u64::try_from(payload_at + 24).unwrap()
            ]
        );
    }

    #[test]
    fn mesh_scene_node_transfers_placed_transform_and_bounds() {
        let graph = synthetic_mesh_graph(false);
        let transform = [
            -1.0, 0.0, 0.0, 4.0, 0.0, -1.0, 0.0, 5.0, 0.0, 0.0, 1.0, 6.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let bytes = placed_mesh_scene_node_record(
            267,
            107,
            105,
            106,
            transform,
            [4.0, 5.0, 6.0, 1.0, 2.0, 3.0],
        );
        let frame = TypedPrimaryFrame {
            entity_id: 107,
            start: 0,
            end: bytes.len(),
            design_type: graph
                .meta
                .types
                .iter()
                .find(|design_type| {
                    design_type
                        .type_guid
                        .as_str()
                        .eq_ignore_ascii_case(SCENE_NODE_TYPE_GUID)
                })
                .expect("Scene-node type"),
        };

        let parsed = parse_scene_node_record(&bytes, frame).expect("placed Scene node");
        assert_eq!(
            parsed
                .node
                .transform()
                .expect("placed transform")
                .value
                .cells(),
            transform
        );
        assert_eq!(
            parsed.node.transform().map(|located| located.offset),
            Some(u64_from_index(placed_scene_node::TRANSFORM))
        );
        let bounds = parsed.node.bounds().expect("placed bounds");
        assert_eq!(bounds.maximum(), [4.0, 5.0, 6.0]);
        assert_eq!(bounds.minimum(), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn mesh_graph_rejects_a_nonreciprocal_collection_owner() {
        let mut graph = synthetic_mesh_graph(false);
        let start = sole_typed_frame(&graph, MESH_COLLECTION_OWNER_TYPE_GUID).start;
        put_reference(
            &mut graph.bytes,
            start + collection_owner::COLLECTION_BACKLINK,
            101,
        );
        let mut no_asset = no_texture_asset;

        assert!(matches!(
            parse_mesh_design_records(
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_graph_rejects_reused_scene_auxiliary() {
        let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
        let frames = typed_primary_frames(
            &graph.bytes,
            &graph.meta,
            SCENE_NODE_TYPE_GUID,
            "mesh-scene-node",
        )
        .unwrap();
        let second = frames.iter().find(|frame| frame.entity_id == 120).unwrap();
        put_reference(
            &mut graph.bytes,
            second.start + scene_node::AUXILIARY_RECORD_REFERENCE,
            106,
        );
        let mut no_asset = no_texture_asset;
        assert!(matches!(
            parse_mesh_design_records(
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_collection_owner_accepts_legacy_version_fifteen() {
        const EXPECTED_COLLECTION: u32 = 100;
        let mut graph = synthetic_mesh_graph(false);
        graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type")
            .version = MESH_COLLECTION_OWNER_TYPE_VERSIONS[0];
        let design_type = graph
            .meta
            .types
            .iter()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type");
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, 270, 110);
        bytes.resize(859, 0);
        push_reference(&mut bytes, EXPECTED_COLLECTION);
        let frame = TypedPrimaryFrame {
            entity_id: 110,
            start: 0,
            end: bytes.len(),
            design_type,
        };

        let owner = parse_mesh_collection_owner_record(&bytes, frame)
            .expect("valid legacy owner frame")
            .expect("legacy collection owner");
        assert_eq!(owner.collection_record_index, EXPECTED_COLLECTION);
        assert_eq!(owner.owner.backlink_offset(), 859);
    }

    #[test]
    fn mesh_collection_owner_accepts_version_seventeen_frame() {
        const EXPECTED_COLLECTION: u32 = 100;
        let mut graph = synthetic_mesh_graph(false);
        let design_type = graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type");
        design_type.version = MESH_COLLECTION_OWNER_TYPE_VERSIONS[1];
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, 270, 110);
        bytes.resize(collection_owner_v17::COLLECTION_BACKLINK, 0);
        push_reference(&mut bytes, EXPECTED_COLLECTION);
        bytes.extend_from_slice(&[0; 64]);
        let frame = TypedPrimaryFrame {
            entity_id: 110,
            start: 0,
            end: bytes.len(),
            design_type,
        };

        let owner = parse_mesh_collection_owner_record(&bytes, frame)
            .expect("valid version-17 owner frame")
            .expect("version-17 collection owner");
        assert_eq!(owner.collection_record_index, EXPECTED_COLLECTION);
        assert_eq!(
            owner.owner.backlink_offset(),
            u64_from_index(collection_owner_v17::COLLECTION_BACKLINK)
        );
    }

    #[test]
    fn mesh_collection_owner_ignores_non_owner_class_members() {
        let mut graph = synthetic_mesh_graph(false);
        let design_type = graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type");
        design_type.version = MESH_COLLECTION_OWNER_TYPE_VERSIONS[1];
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, 270, 110);
        bytes.resize(collection_owner_v17::LEN, 0);
        let frame = TypedPrimaryFrame {
            entity_id: 110,
            start: 0,
            end: bytes.len(),
            design_type,
        };

        assert!(parse_mesh_collection_owner_record(&bytes, frame)
            .expect("valid generic owner frame")
            .is_none());
    }

    #[test]
    fn mesh_graph_rejects_disagreeing_ordered_body_lists() {
        let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
        let start = sole_typed_frame(&graph, MESH_FEATURE_SCOPE_TYPE_GUID).start;
        put_reference(&mut graph.bytes, start + 36, 104);
        let mut no_asset = no_texture_asset;

        assert!(matches!(
            parse_mesh_design_records(
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            ),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn mesh_join_rejects_multiple_typed_body_candidates() {
        let graph = synthetic_mesh_graph(false);
        let mut no_asset = no_texture_asset;
        let design = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect("mesh graph");
        assert!(resolve_mesh_body(
            &[design.clone(), design],
            "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh",
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE"
        )
        .is_none());
    }

    #[test]
    fn mesh_body_projection_places_identifier_and_attributes() {
        let transform = crate::records::mesh::MeshAffineTransform::new([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ])
        .unwrap();
        let container = || MeshContainer {
            fusion_uuid: "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".into(),
            mesh_uuid: crate::records::mesh::DesignMeshUuid::try_from(
                "11111111-2222-4333-8444-555555555555".to_owned(),
            )
            .unwrap(),
            vertices: vec![FinitePoint3::ZERO],
            triangles: Vec::new(),
            feature_edges: Vec::new(),
            corner_normals: Some(vec![UnitVector3::Z_AXIS]),
            triangle_groups: Vec::new(),
            texture_ids: None,
            attributes: Vec::new(),
        };
        {
            let body =
                MeshBody::from_container("mesh.paramesh", 100, transform, container()).unwrap();
            assert_eq!(
                body.id,
                crate::ids::native_mesh_body_id("mesh.paramesh", 100)
            );
            assert_eq!(body.vertices.len(), 1);
            assert_eq!(body.corner_normals.unwrap().len(), 1);
        };
    }
}
