// SPDX-License-Identifier: Apache-2.0
//! Parse Design sketch placements, headers, relations, and geometry.

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::decode::u64_from_index;

use crate::records::sketch_placement::{
    DesignSketchFrame, DesignSketchFrameForm, SketchPlacementMatrix,
};

use crate::layout::sketch_typed_compact_member_head as typed_compact;
use cadmpeg_core::container::ContainerRole;

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{f64s_at, take_reference, Reference};
use crate::container::ContainerScan;
use crate::design::decode::text::design_record_id;

use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::layout::sketch_container_visibility_member_prefix as visibility_member;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::{DesignEntityHeader, DESIGN_MODULE_SKETCH},
    feature::scope::DesignParameterScope,
    references::{LostEdgeReference, PersistentReference, PersistentReferenceKind},
    sketch_geometry::{
        SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchPointClosure,
        SketchPointCompanion, SketchPointCompanionReferenceEncoding, SketchPointRecordForm,
        SketchSurface, SketchSurfaceGeometry, SketchText, SketchTextAlignment, SketchTextLayout,
    },
    sketch_placement::{DesignSketchPlacement, DesignSketchVisibility},
    sketch_relations::{SketchGlyphTransform, SketchRelation, SketchRelationOperand},
};
use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, FiniteReal, NonNegativeLength, NonNegativeReal, PositiveLength};
use cadmpeg_ir::sketches::TextPlacement;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::units::{FinitePoint2, UnitVector3};
use std::collections::HashMap;

use super::meta::{
    decode_types, design_primary_frames, metadata_for_bulk_stream, stream_types_by_class_tag,
};

const EPS_SKETCH_DECODE_LINE_COMPONENTS_E12: f64 = 1.0e-12;

/// Byte offsets of every indexed-record header in one `BulkStream`, grouped by
/// the record index carried at header offset seven.
pub(in crate::design) struct IndexedRecordOffsets {
    by_record_index: HashMap<u32, Vec<usize>>,
}

impl IndexedRecordOffsets {
    /// Index every exact indexed-record header in `bytes` in one forward pass.
    pub(in crate::design) fn build(bytes: &[u8]) -> Self {
        let mut by_record_index = HashMap::<u32, Vec<usize>>::new();
        for header in indexed_record_offsets(bytes) {
            by_record_index
                .entry(header.record_index)
                .or_default()
                .push(header.offset);
        }
        Self { by_record_index }
    }

    /// Ascending header offsets carrying `record_index`.
    pub(in crate::design) fn offsets(&self, record_index: u32) -> &[usize] {
        self.by_record_index
            .get(&record_index)
            .map_or(&[], Vec::as_slice)
    }

    /// Record indexes and their ascending header offsets, in ascending record
    /// index order, so that output built from them does not vary between runs.
    pub(in crate::design) fn records(&self) -> impl Iterator<Item = (u32, &[usize])> {
        let mut records = self
            .by_record_index
            .iter()
            .map(|(record_index, offsets)| (*record_index, offsets.as_slice()))
            .collect::<Vec<_>>();
        records.sort_unstable_by_key(|(record_index, _)| *record_index);
        records.into_iter()
    }

    /// The first header at or after `position` that carries `record_index`.
    pub(super) fn first_at_or_after(&self, position: usize, record_index: u32) -> Option<usize> {
        let offsets = self.offsets(record_index);
        offsets
            .get(offsets.partition_point(|offset| *offset < position))
            .copied()
    }

    /// Consecutive header offsets carrying `record_index`, each pair delimiting
    /// one frame of that record.
    pub(in crate::design) fn frames(
        &self,
        record_index: u32,
    ) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.offsets(record_index)
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
    }
}

fn entity_meta_scope(bulk_scope: &str) -> Option<String> {
    let prefix = bulk_scope.strip_suffix("BulkStream.dat")?;
    Some(format!("{prefix}MetaStream.dat"))
}

/// Cache a stream index under its borrowed identity.
pub(in crate::design) fn cached_borrowed_record_offsets<'a, 's>(
    cache: &'a mut HashMap<&'s str, IndexedRecordOffsets>,
    stream: &'s str,
    bytes: &'a [u8],
) -> &'a IndexedRecordOffsets {
    match cache.entry(stream) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(IndexedRecordOffsets::build(bytes))
        }
    }
}

/// Cache a stream index under owned stream text.
pub(in crate::design) fn cached_owned_record_offsets<'a>(
    cache: &'a mut HashMap<String, IndexedRecordOffsets>,
    stream: &str,
    bytes: &[u8],
) -> &'a IndexedRecordOffsets {
    match cache.entry(stream.to_owned()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(IndexedRecordOffsets::build(bytes))
        }
    }
}

/// Decode the unique local-to-model placement frame referenced by every
/// parameter-owning sketch scope, and every member-run head placement. A
/// localized Sketch scope follows its entity container within the same
/// stream interval even though its generic reference table does not repeat
/// the entity suffix.
pub(crate) fn decode_sketch_placements(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignSketchPlacement>, CodecError> {
    let mut out = Vec::new();
    let mut record_offsets = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope = crate::ids::native_scope(&entry.name);
        record_offsets.insert(scope, IndexedRecordOffsets::build(bytes));
    }
    let mut visibilities = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let scope = crate::ids::native_scope(&entry.name);
        let Some((key, _)) = record_offsets.get_key_value(&scope) else {
            continue;
        };
        for (entity_suffix, visibility) in decode_sketch_visibilities_in_stream(bytes, &metadata)? {
            if visibilities.contains_key(&(key.as_str(), entity_suffix)) {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design stream {} repeats sketch visibility for entity {entity_suffix}",
                    entry.name
                )));
            }

            visibilities.insert((key.as_str(), entity_suffix), visibility);
        }
    }
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch))
    {
        let Some(binding) = scope.sketch_entity() else {
            continue;
        };
        let entity_id = &binding.entity_id;
        let mut entry = None;
        for candidate in scan
            .entries
            .iter()
            .filter(|candidate| scan.is_design_stream(candidate, ContainerRole::Bulkstream))
        {
            let candidate_scope = crate::ids::native_scope(&candidate.name);
            if scope
                .id
                .strip_prefix(candidate_scope.as_str())
                .is_some_and(|tail| tail.starts_with(':'))
            {
                entry = Some(candidate);
                break;
            }
        }
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream_scope = crate::ids::native_scope(&entry.name);
        let Some(records) = record_offsets.get(&stream_scope) else {
            continue;
        };
        let start = usize::try_from(scope.byte_offset()).ok();
        let end = usize::try_from(scope.paired_byte_offset()).ok();
        let Some(frame) = start
            .zip(end)
            .and_then(|(start, end)| bytes.get(start..end))
        else {
            continue;
        };
        let mut referenced_indices = Vec::new();
        for window in frame.windows(11) {
            if window[0] == 1 && window[5..11] == [0; 6] {
                let Some(record_index) = View::u32_le_at(window, 1) else {
                    continue;
                };
                if !referenced_indices.contains(&record_index) {
                    referenced_indices.push(record_index);
                }
            }
        }
        let mut candidates = Vec::new();
        for record_index in referenced_indices {
            let parsed = parse_sketch_placement_candidates(
                bytes,
                scope.record_index,
                entity_id,
                record_index,
                records,
            );
            candidates.extend(parsed);
        }
        if candidates.len() == 1 {
            let Some(mut placement) = candidates.pop() else {
                continue;
            };
            placement.id = design_record_id(
                &entry.name,
                ":design-sketch-placement#",
                placement.byte_offset(),
            );

            out.push(placement);
        }
    }
    // A sketch entity header pairs with a same-index member-run record whose
    // leading marked reference names a head record carrying the row-major
    // 4×4 placement. A Sketch scope whose entity binding names the entity owns
    // it. Otherwise a localized Sketch scope belongs to the preceding sketch
    // entity interval: it follows that entity and precedes the next sketch
    // entity in the same stream. Some member-run sketches have no scope.
    let initial_placements = out.len();
    for entity in entities.iter().filter(|entity| entity.in_sketch_module()) {
        let Some(stream) = native_stream(&entity.id) else {
            continue;
        };
        if out.iter().take(initial_placements).any(|placement| {
            native_stream(&placement.id) == Some(stream)
                && placement.entity_id.suffix() == entity.entity_id.suffix()
        }) {
            continue;
        }
        let Some(entry_name) = stream.strip_prefix(ids::SCHEME_PREFIX) else {
            continue;
        };
        let bytes = scan.entry_bytes(entry_name)?;
        let Some(records) = record_offsets.get(stream) else {
            continue;
        };
        let Some(mut placement) =
            parse_member_run_head_placement(bytes, entity.byte_offset, &entity.entity_id, records)
        else {
            continue;
        };
        let next_entity_offset = entities
            .iter()
            .filter(|candidate| {
                candidate.in_sketch_module()
                    && native_stream(&candidate.id) == Some(stream)
                    && candidate.byte_offset > entity.byte_offset
            })
            .map(|candidate| candidate.byte_offset)
            .min();
        let owner = member_run_sketch_scope(scopes, stream, entity, next_entity_offset);
        if let Some(scope) = owner {
            placement.scope_record_index = Some(scope.record_index);
        }
        placement.id = design_record_id(
            entry_name,
            ":design-sketch-placement#",
            placement.byte_offset(),
        );

        out.push(placement);
    }
    for placement in &mut out {
        let Some(stream) = native_stream(&placement.id) else {
            continue;
        };
        placement.visibility = visibilities
            .get(&(stream, placement.entity_id.suffix()))
            .copied();
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

const CURRENT_SKETCH_CONTAINER_VERSION: u32 = 18;
const SKETCH_CONTAINER_MEMBER_TYPE_GUID: &str = "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9";
const SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const SKETCH_CONTAINER_MEMBER_VERSION: u32 = 4;

/// Decode the direct display flag in every current sketch container's typed
/// Geometry member. Other sketch-container versions do not expose this member
/// layout and therefore leave neutral visibility unknown.
fn decode_sketch_visibilities_in_stream(
    bytes: &[u8],
    metadata: &crate::metastream::MetaStream,
) -> Result<Vec<(u64, DesignSketchVisibility)>, CodecError> {
    let mut out = Vec::new();
    for frame in super::meta::typed_primary_frames(
        bytes,
        metadata,
        SKETCH_CONTAINER_TYPE_GUID,
        "sketch-container",
    )? {
        if frame.design_type.version != CURRENT_SKETCH_CONTAINER_VERSION {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_SKETCH
            || !frame
                .design_type
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                .is_some_and(|base| base.eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_TYPE_GUID))
        {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch container {} has incompatible registration metadata",
                frame.entity_id
            )));
        }
        let Some(NamedEntityHeader {
            entity_id,
            end: header_end,
            ..
        }) = (match parse_settled_entity_header(&bytes[..frame.end], frame.start) {
            Some(header) => Some(header),
            None => parse_genesis_entity_header(&bytes[..frame.end], frame.start),
        })
        else {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch container {} has an invalid entity header",
                frame.entity_id
            )));
        };
        let entity_suffix = entity_id.suffix();
        if entity_suffix != frame.entity_id {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch container {} disagrees with its entity header {entity_suffix}",
                frame.entity_id
            )));
        }
        let Some(member_at) = next_indexed_record_offset(&bytes[..frame.end], header_end) else {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch container {entity_suffix} has no typed Geometry member"
            )));
        };
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered_view(bytes, member_at, 3..=3, u8::is_ascii_digit)
        else {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch container {entity_suffix} has an invalid Geometry-member class tag"
            )));
        };
        let member_type = class_tag
            .parse::<usize>()
            .ok()
            .and_then(|tag| tag.checked_sub(256))
            .and_then(|ordinal| metadata.types.get(ordinal));
        if after_tag != member_at + 7
            || !member_type.is_some_and(|member_type| {
                member_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_TYPE_GUID)
                    && member_type.version == SKETCH_CONTAINER_MEMBER_VERSION
                    && member_type.module == "Geometry"
                    && member_type
                        .base_type_guid
                        .value()
                        .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                        .is_some_and(|base| {
                            base.eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID)
                        })
            })
        {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch container {entity_suffix} has an incompatible Geometry member"
            )));
        }
        let Some(visibility) =
            decode_sketch_visibility_member(&bytes[..frame.end], member_at, entity_suffix)
        else {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch container {entity_suffix} has an invalid visibility member"
            )));
        };

        out.push((entity_suffix, visibility));
    }
    if out.is_empty() {
        return Ok(out);
    }
    let visibilities = out
        .iter()
        .copied()
        .collect::<std::collections::HashMap<_, _>>();
    let mut listed = std::collections::HashSet::new();
    let mut collections = std::collections::HashSet::new();
    for frame in super::meta::typed_primary_frames(
        bytes,
        metadata,
        "5275CBA4-3D7D-40DC-A651-1DFF3FC8AFBF",
        "sketch ordinal collection",
    )? {
        let members = ([3, 4].contains(&frame.design_type.version)
            && frame.design_type.module == "Geometry"
            && frame
                .design_type
                .base_type_guid
                .value()
                .is_some_and(|base| {
                    base.as_str()
                        .eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID)
                }))
        .then(|| {
            ordinal_collection_members(&bytes[frame.start..frame.end], frame.design_type.version)
        })
        .flatten();
        let Some(members) = members else {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch ordinal collection {} has an invalid member run",
                frame.entity_id
            )));
        };
        collections.insert(frame.entity_id);
        for target in members {
            if !listed.insert(target)
                || !visibilities.get(&target).is_some_and(|visibility| {
                    visibility.ordinal_collection.map(std::num::NonZeroU64::get)
                        == Some(frame.entity_id)
                })
            {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D sketch ordinal collection {} has an invalid sketch member {target}",
                    frame.entity_id
                )));
            }
        }
    }
    for (entity, visibility) in &out {
        let collection_valid = match visibility.ordinal_collection {
            Some(collection) => collections.contains(&collection.get()) && listed.contains(entity),
            None => !listed.contains(entity),
        };
        if !collection_valid {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch container {entity} has an invalid ordinal collection"
            )));
        }
    }
    Ok(out)
}

fn ordinal_collection_members(
    bytes: &[u8],
    version: u32,
) -> Option<impl Iterator<Item = u64> + '_> {
    use crate::layout::sketch_geometry_ordinal_collection_reference as reference;
    use crate::layout::sketch_ordinal_collection_prefix as collection;
    if bytes.get(collection::ZERO_RUN..collection::LEN)? != [0; 9] {
        return None;
    }
    let mut cursor = collection::LEN;
    read_property_block(bytes, &mut cursor)?.find("crv_tag_id_idx")?;
    match version {
        3 => {
            cursor = cursor
                .checked_add(crate::layout::sketch_owner_collection_legacy_list_prefix::LEN)?;
        }
        4 => {}
        _ => return None,
    }
    let count = View::u32_le_at(bytes, cursor)?;
    cursor += 4;
    let count = cadmpeg_core::decode::bounded_len(
        u64::from(count),
        reference::LEN,
        bytes.len().checked_sub(cursor)?,
    )?;
    let end = cursor.checked_add(count.checked_mul(reference::LEN)?)?;
    let run = bytes.get(cursor..end)?;
    for member in run.chunks_exact(reference::LEN) {
        let mut at = 0;
        if !matches!(take_reference(member, &mut at)?.local(), Some((target, None)) if target != 0)
        {
            return None;
        }
    }
    Some(
        run.chunks_exact(reference::LEN)
            .filter_map(|member| View::u64_le_at(member, reference::TARGET)),
    )
}

fn decode_sketch_visibility_member(
    bytes: &[u8],
    member_at: usize,
    entity_suffix: u64,
) -> Option<DesignSketchVisibility> {
    let record_index = View::u64_le_at(bytes, member_at + visibility_member::ENTITY_SUFFIX)?;
    if record_index != entity_suffix
        || bytes.get(
            member_at + visibility_member::ZERO_RUN..member_at + visibility_member::OWNER_REFERENCE,
        ) != Some(&[0; 4])
    {
        return None;
    }
    let mut cursor = member_at + visibility_member::OWNER_REFERENCE;
    let owner = take_reference(bytes, &mut cursor)?;
    if cursor != member_at + visibility_member::STREAM_ORDINAL
        || !matches!(owner.local(), Some((target, None)) if target != 0)
    {
        return None;
    }
    let stream_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, cursor)?)?;
    if bytes.get(member_at + visibility_member::RESERVED_ZERO) != Some(&0) {
        return None;
    }
    let stream_ordinal_offset = cursor;
    let visible_offset = member_at + visibility_member::VISIBLE;
    let visible = match bytes.get(visible_offset) {
        Some(0) => false,
        Some(1) => true,
        _ => return None,
    };
    let mut visibility = DesignSketchVisibility::new(
        stream_ordinal,
        u64_from_index(stream_ordinal_offset),
        visible,
    )
    .ok()?;
    let mut collection_at = member_at + visibility_member::TAIL_MARKER;
    let reference = take_reference(bytes, &mut collection_at)?;
    if matches!(reference, Reference::Null) {
        return Some(visibility);
    }
    let (target, None) = reference.local()? else {
        return None;
    };
    visibility.ordinal_collection = Some(std::num::NonZeroU64::new(target)?);
    Some(visibility)
}

/// Byte length of a member-run head carrying an explicit 4×4 transform.
const MEMBER_RUN_HEAD_FRAME: usize = 162;

/// The explicit head frame whose closing reference at offset 151 carries an
/// inline type GUID: forty bytes longer than the plain frame.
const TYPED_MEMBER_RUN_HEAD_FRAME: usize = MEMBER_RUN_HEAD_FRAME + 40;

/// The Sketch scope that owns a member-run sketch entity.
fn member_run_sketch_scope<'a>(
    scopes: &'a [DesignParameterScope],
    stream: &str,
    entity: &DesignEntityHeader,
    next_entity_offset: Option<u64>,
) -> Option<&'a DesignParameterScope> {
    let sketch_scopes = || {
        scopes.iter().filter(|scope| {
            design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch)
                && native_stream(&scope.id) == Some(stream)
        })
    };
    let mut bound_scopes = sketch_scopes().filter(|scope| {
        scope
            .sketch_entity()
            .is_some_and(|binding| binding.entity_id == entity.entity_id)
    });
    let mut interval_scopes = sketch_scopes().filter(|scope| {
        scope.sketch_entity().is_none()
            && scope.byte_offset() > entity.byte_offset
            && next_entity_offset.is_none_or(|end| scope.byte_offset() < end)
    });
    match (bound_scopes.next(), bound_scopes.next()) {
        (Some(scope), None) => Some(scope),
        (Some(_), Some(_)) => None,
        (None, _) => match (interval_scopes.next(), interval_scopes.next()) {
            (Some(scope), None) => Some(scope),
            _ => None,
        },
    }
}

/// Parse a member-run head placement: the paired same-index record after the
/// sketch's entity header opens with a marked
/// reference naming a head record. A 34-byte head denotes the identity
/// placement. A 162-byte head stores eleven zero bytes and the row-major 4×4
/// local-to-model transform at offset 22.
fn parse_member_run_head_placement(
    bytes: &[u8],
    entity_byte_offset: u64,
    entity_id: &crate::records::identity::DesignEntityId,
    records: &IndexedRecordOffsets,
) -> Option<DesignSketchPlacement> {
    let parsed = (|| {
        let start = usize::try_from(entity_byte_offset).ok()?;
        // Locate the paired same-index record after the entity header.
        let entity_index = u32::try_from(entity_id.suffix()).ok()?;
        let paired_at = records.first_at_or_after(start.checked_add(1)?, entity_index)?;
        let (paired_class_tag, paired_after_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
        let paired_class_tag =
            crate::design::decode::text::class_tag_from_view(paired_class_tag).ok()?;
        // The paired record's prologue: the u32 index, zero bytes to offset 19,
        // then a marked u64 reference naming the head record.
        if paired_after_tag != paired_at + 7
            || bytes.get(paired_at + 11..paired_at + 19) != Some(&[0u8; 8][..])
            || bytes.get(paired_at + 19) != Some(&1)
        {
            return None;
        }
        let head_index = View::u32_le_at(bytes, paired_at + 20)?;
        if bytes.get(paired_at + 24..paired_at + 28) != Some(&[0u8; 4][..]) {
            return None;
        }
        // Locate the head record and decode its transform.
        let head_at = records.offsets(head_index).first().copied()?;
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, head_at, 0..=2000, u8::is_ascii_graphic)?;
        let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
        if after_tag != head_at + 7 {
            return None;
        }
        let head_end = next_indexed_record_offset(bytes, head_at + 11).unwrap_or(bytes.len());
        let frame_length = head_end.checked_sub(head_at)?;
        let form = match frame_length {
            typed_compact::LEN
                if bytes.get(head_at + typed_compact::ZERO_RUN..head_at + typed_compact::FLAGS)
                    == Some(&typed_compact::ZERO_RUN_VALUE[..])
                    && bytes.get(
                        head_at + typed_compact::FLAGS..head_at + typed_compact::REFERENCE,
                    ) == Some(&typed_compact::FLAGS_VALUE[..])
                    && crate::bytes::typed_local_reference_target(
                        bytes,
                        head_at + typed_compact::REFERENCE,
                    )
                    .is_some() =>
            {
                DesignSketchFrameForm::MemberTypedCompact {
                    paired_byte_offset: u64_from_index(paired_at),
                }
            }
            34 if bytes.get(head_at + 11..head_at + 21) == Some(&[0u8; 10][..])
                && bytes.get(head_at + 21..head_at + 24) == Some(&[1, 0, 1][..])
                && bytes.get(head_at + 28..head_at + 34) == Some(&[0u8; 6][..]) =>
            {
                DesignSketchFrameForm::MemberCompact {
                    paired_byte_offset: u64_from_index(paired_at),
                }
            }
            MEMBER_RUN_HEAD_FRAME | TYPED_MEMBER_RUN_HEAD_FRAME
                if bytes.get(head_at + 11..head_at + 22) == Some(&[0u8; 11][..])
                    && (frame_length == MEMBER_RUN_HEAD_FRAME
                        || crate::bytes::typed_local_reference_target(bytes, head_at + 151)
                            .is_some()) =>
            {
                let values = f64s_at::<16>(bytes, head_at + 22)?;
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.iter().copied().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if bytes.get(head_at + 150..head_at + 152) != Some(&[0, 1][..]) {
                    return None;
                }
                DesignSketchFrameForm::MemberExplicit {
                    paired_byte_offset: u64_from_index(paired_at),
                    transform: SketchPlacementMatrix::try_from(transform).ok()?,
                    typed_reference: frame_length == TYPED_MEMBER_RUN_HEAD_FRAME,
                }
            }
            _ => return None,
        };
        Some((head_at, form, class_tag, paired_class_tag, head_index))
    })();
    let (head_at, form, class_tag, paired_class_tag, head_index) = parsed?;
    let frame = DesignSketchFrame::new(u64_from_index(head_at), form).ok()?;
    Some(DesignSketchPlacement {
        id: String::new(),
        scope_record_index: None,
        entity_id: entity_id.clone(),
        visibility: None,
        frame,
        class_tag,
        record_index: head_index,
        paired_class_tag,
    })
}

fn parse_sketch_placement_candidates(
    bytes: &[u8],
    scope_record_index: u32,
    entity_id: &crate::records::identity::DesignEntityId,
    record_index: u32,
    records: &IndexedRecordOffsets,
) -> Vec<DesignSketchPlacement> {
    let mut out = Vec::new();
    for pair in records.offsets(record_index).windows(2) {
        let start = pair[0];
        let paired_at = pair[1];
        let Some(frame_length) = paired_at.checked_sub(start) else {
            continue;
        };
        if !matches!(frame_length, 201 | 213 | 305 | 325 | 329 | 341) {
            continue;
        }
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        let Some((paired_class_tag, paired_after_tag)) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        if after_tag != start + 7
            || paired_after_tag != paired_at + 7
            || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
        {
            continue;
        }
        let Ok(class_tag) = crate::design::decode::text::class_tag_from_view(class_tag) else {
            continue;
        };
        let Ok(paired_class_tag) =
            crate::design::decode::text::class_tag_from_view(paired_class_tag)
        else {
            continue;
        };
        let form = match frame_length {
            201 => DesignSketchFrameForm::ScopeCompact,
            305 | 325 => {
                let Some(values) = f64s_at::<16>(bytes, start + 48) else {
                    continue;
                };
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.iter().copied().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                let Ok(transform) = SketchPlacementMatrix::try_from(transform) else {
                    continue;
                };
                if frame_length == 305 {
                    DesignSketchFrameForm::ScopeLegacy305(transform)
                } else {
                    DesignSketchFrameForm::ScopeLegacy325(transform)
                }
            }
            329 => {
                let Some(values) = f64s_at::<16>(bytes, start + 55) else {
                    continue;
                };
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.iter().copied().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                let Ok(transform) = SketchPlacementMatrix::try_from(transform) else {
                    continue;
                };
                DesignSketchFrameForm::ScopeExplicit(transform)
            }
            // The `EntityGenesis`-flavor frame: `0x01` at offset 55, nine
            // zero bytes, and a form byte at offset 65. Form `0x01` is the
            // identity transform; form `0x00` is followed by the row-major
            // 4×4 f64 matrix at offset 66. The WorkPlane sibling of this
            // record class carries a marked record reference at offset 57
            // and fails the zero-run check.
            213 | 341 => {
                if bytes.get(start + 55) != Some(&1)
                    || bytes.get(start + 56..start + 65) != Some(&[0u8; 9][..])
                {
                    continue;
                }
                match (frame_length, bytes.get(start + 65)) {
                    (213, Some(&1)) => DesignSketchFrameForm::ScopeGenesisCompact,
                    (341, Some(&0)) => {
                        let Some(values) = f64s_at::<16>(bytes, start + 66) else {
                            continue;
                        };
                        let mut transform = [[0.0; 4]; 4];
                        for (ordinal, value) in values.iter().copied().enumerate() {
                            transform[ordinal / 4][ordinal % 4] = value;
                        }
                        let Ok(transform) = SketchPlacementMatrix::try_from(transform) else {
                            continue;
                        };
                        DesignSketchFrameForm::ScopeGenesisExplicit(transform)
                    }
                    _ => continue,
                }
            }
            _ => continue,
        };
        let Ok(frame) = DesignSketchFrame::new(u64_from_index(start), form) else {
            continue;
        };

        out.push(DesignSketchPlacement {
            id: String::new(),
            scope_record_index: Some(scope_record_index),
            entity_id: entity_id.clone(),
            visibility: None,
            frame,
            class_tag,
            record_index,
            paired_class_tag,
        });
    }
    out
}

/// Decode the persistent u64 point and curve identity references
/// (`pt_tag`, `crv_primary_id`, `crv_secondary_id`, each typed
/// `IntrinsicMetaTypeuint64`) from every design `BulkStream` entry in `scan`,
/// sorted by stream offset.
pub(crate) fn decode_persistent_references(
    scan: &ContainerScan,
) -> Result<Vec<PersistentReference>, CodecError> {
    let mut out = Vec::new();
    for (entry_ordinal, entry) in scan
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_persistent_references_from_stream(entry_ordinal, &entry.name, bytes, &mut out)?;
    }
    Ok(finish_persistent_references(out))
}

fn finish_persistent_references(
    mut out: Vec<(usize, PersistentReference)>,
) -> Vec<PersistentReference> {
    out.sort_by(|left, right| {
        let left_key = {
            let (entry_ordinal, reference) = left;
            {
                (*entry_ordinal, reference.byte_offset)
            }
        };
        let right_key = {
            let (entry_ordinal, reference) = right;
            {
                (*entry_ordinal, reference.byte_offset)
            }
        };
        left_key.cmp(&right_key)
    });

    let mut references = Vec::new();
    references.extend(out.into_iter().map(|(_, reference)| reference));
    references
}

fn decode_persistent_references_from_stream(
    entry_ordinal: usize,
    entry_name: &str,
    bytes: &[u8],
    out: &mut Vec<(usize, PersistentReference)>,
) -> Result<(), CodecError> {
    for &(name, kind) in &[
        (b"pt_tag".as_slice(), PersistentReferenceKind::Point),
        (
            b"crv_primary_id".as_slice(),
            PersistentReferenceKind::CurvePrimary,
        ),
        (
            b"crv_secondary_id".as_slice(),
            PersistentReferenceKind::CurveSecondary,
        ),
    ] {
        let mut cursor = 0;
        while let Some(offset) = find_from(bytes, name, cursor) {
            cursor = offset + name.len();
            let compact_type_offset = offset + name.len();
            let type_offset = if View::u32_le_at(bytes, compact_type_offset) == Some(23) {
                compact_type_offset
            } else if View::u32_le_at(bytes, compact_type_offset) == Some(2)
                && View::u32_le_at(bytes, compact_type_offset + 4) == Some(14)
                && bytes
                    .get(compact_type_offset + 8..compact_type_offset + 22)
                    .is_some()
                && View::u32_le_at(bytes, compact_type_offset + 22) == Some(23)
            {
                compact_type_offset + 22
            } else {
                continue;
            };
            if View::u32_le_at(bytes, type_offset) != Some(23) {
                continue;
            }
            let type_name = b"IntrinsicMetaTypeuint64";
            if bytes.get(type_offset + 4..type_offset + 4 + type_name.len()) != Some(type_name) {
                continue;
            }
            let value_offset = type_offset + 4 + type_name.len();
            let Some(value) = View::u64_le_at(bytes, value_offset) else {
                continue;
            };

            out.push((
                entry_ordinal,
                PersistentReference {
                    id: design_record_id(
                        entry_name,
                        ":persistent-reference#",
                        u64_from_index(offset),
                    ),
                    byte_offset: u64_from_index(offset),
                    value_offset: u32::try_from(value_offset - offset).map_err(|_| {
                        CodecError::malformed(
                            "f3d persistent reference relative offset exceeds u32",
                        )
                    })?,
                    kind,
                    value,
                },
            ));
        }
    }
    Ok(())
}

/// Decode every indexed `EDGE_REFERENCE_LOST` record from each design
/// `BulkStream` entry in `scan`.
pub(crate) fn decode_lost_edge_references(
    scan: &ContainerScan,
) -> Result<Vec<LostEdgeReference>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_lost_edge_references_from_stream(&entry.name, bytes, &mut out)?;
    }
    Ok(out)
}

fn decode_lost_edge_references_from_stream(
    entry_name: &str,
    bytes: &[u8],
    out: &mut Vec<LostEdgeReference>,
) -> Result<(), CodecError> {
    let marker = b"EDGE_REFERENCE_LOST";
    let mut cursor = 0;
    while let Some(relative) = bytes[cursor..]
        .windows(marker.len())
        .position(|window| window == marker)
    {
        let offset = cursor + relative;
        cursor = offset + marker.len();
        let Some(header_offset) = offset.checked_sub(29) else {
            continue;
        };
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered_view(bytes, header_offset, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        if after_tag != header_offset + 7
            || bytes.get(header_offset + 11..header_offset + 25) != Some(&[0; 14])
            || View::u32_le_at(bytes, header_offset + 25)
                != Some(u32::try_from(marker.len()).map_err(|_| {
                    CodecError::malformed("f3d lost edge marker length exceeds u32")
                })?)
        {
            continue;
        }
        let Some(record_index) = View::u32_le_at(bytes, after_tag) else {
            continue;
        };
        let next_byte_offset = offset + marker.len();
        let Some((next_class_tag, after_next_tag)) =
            lp_ascii_filtered_view(bytes, next_byte_offset, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        if after_next_tag != next_byte_offset + 7 {
            continue;
        }
        let Some(next_record_index) = View::u32_le_at(bytes, after_next_tag) else {
            continue;
        };
        if !class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || !next_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            continue;
        }

        let Ok(reference) = LostEdgeReference::new(
            design_record_id(
                entry_name,
                ":lost-edge-reference#",
                u64_from_index(header_offset),
            ),
            u64_from_index(header_offset),
            class_tag.to_owned(),
            record_index,
            next_class_tag.to_owned(),
            next_record_index,
        ) else {
            continue;
        };
        out.push(reference);
    }
    Ok(())
}

/// Parse the fixed entity-header layout at `start`: a u64 entity suffix, five
/// zero bytes, an optional slot, and the UTF-16LE entity id whose numeric
/// suffix equals the header's entity suffix.
pub(super) fn parse_settled_entity_header(bytes: &[u8], start: usize) -> Option<NamedEntityHeader> {
    let entity_suffix = View::u64_le_at(bytes, start + 7)?;
    if entity_suffix == 0 || bytes.get(start + 15..start + 20) != Some(&[0u8; 5]) {
        return None;
    }
    let (optional_slot_present, string_offset) = match bytes.get(start + 20) {
        Some(0) => (false, start + 21),
        Some(1) if bytes.get(start + 21..start + 25) == Some(&[0u8; 4]) => (true, start + 25),
        _ => return None,
    };
    let (entity_id, end) = lp_utf16_bounded(bytes, string_offset, 1..=256)?;
    let Ok(entity_id) = crate::records::identity::DesignEntityId::try_from(entity_id) else {
        return None;
    };
    (entity_id.suffix() == entity_suffix).then_some(NamedEntityHeader {
        entity_id,
        entity_id_offset: string_offset + 4,
        optional_slot_present,
        end,
    })
}

/// An admitted entity identity and its source header locations.
pub(super) struct NamedEntityHeader {
    pub(super) entity_id: crate::records::identity::DesignEntityId,
    pub(super) entity_id_offset: usize,
    optional_slot_present: bool,
    pub(super) end: usize,
}

/// Parse the `EntityGenesis` entity-header layout at `start`: the u32 record
/// index doubles as the entity suffix and is followed by a zero run, a
/// `0x01`-marked u32 1, the `EntityGenesis` and `IntrinsicMetaTypeuint64`
/// key strings, the u64 origin bitfield, and the UTF-16LE entity id whose
/// numeric suffix equals the record index.
pub(super) fn parse_genesis_entity_header(bytes: &[u8], start: usize) -> Option<NamedEntityHeader> {
    let entity_suffix = View::u32_le_at(bytes, start + 7).map(u64::from)?;
    if entity_suffix == 0 {
        return None;
    }
    let mut cursor = start + 11;
    while bytes.get(cursor) == Some(&0) && cursor < start + 35 {
        cursor += 1;
    }
    if cursor == start + 11
        || bytes.get(cursor) != Some(&1)
        || View::u32_le_at(bytes, cursor + 1) != Some(1)
    {
        return None;
    }
    let after_key = lp_ascii_matches(bytes, cursor + 5, b"EntityGenesis")?;
    let after_type = lp_ascii_matches(bytes, after_key, b"IntrinsicMetaTypeuint64")?;
    let (entity_id, end) = lp_utf16_bounded(bytes, after_type + 8, 1..=256)?;
    let Ok(entity_id) = crate::records::identity::DesignEntityId::try_from(entity_id) else {
        return None;
    };
    (entity_id.suffix() == entity_suffix).then_some(NamedEntityHeader {
        entity_id,
        entity_id_offset: after_type + 12,
        optional_slot_present: false,
        end,
    })
}

fn lp_ascii_matches(bytes: &[u8], at: usize, expected: &[u8]) -> Option<usize> {
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if count != expected.len() {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(count)?;
    (bytes.get(start..end) == Some(expected)).then_some(end)
}

/// Parse the counted member-record run of the paired same-index container
/// record that follows an `EntityGenesis`-form sketch entity header: the u32
/// member count at paired-record offset 52, the marked reference to the
/// sketch's base-point record, and `count` entries of `0x01 + u32
/// record_index + six zero bytes` naming the sketch's owned records. The
/// base-point reference is returned as the first member.
fn parse_sketch_member_run(
    bytes: &[u8],
    from: usize,
    entity_suffix: u64,
) -> Vec<crate::records::identity::Located<u32>> {
    let empty = Vec::new();
    let Some(paired) = next_indexed_record_offset(bytes, from) else {
        return empty;
    };
    if View::u32_le_at(bytes, paired + 7).map(u64::from) != Some(entity_suffix) {
        return empty;
    }
    let Some(count) =
        View::u32_le_at(bytes, paired + 52).and_then(|count| usize::try_from(count).ok())
    else {
        return empty;
    };
    if count == 0
        || bytes.get(paired + 56) != Some(&1)
        || bytes.get(paired + 61..paired + 67) != Some(&[0u8; 6][..])
    {
        return empty;
    }
    let Some(base_point_index) = View::u32_le_at(bytes, paired + 57) else {
        return empty;
    };
    let Some(run_end) = count
        .checked_mul(11)
        .and_then(|length| paired.checked_add(67)?.checked_add(length))
    else {
        return empty;
    };
    if run_end > bytes.len() {
        return empty;
    }

    let mut members = Vec::new();
    members.push(crate::records::identity::Located {
        value: base_point_index,
        offset: u64_from_index(paired + 57),
    });
    for ordinal in 0..count {
        let marker = paired + 67 + ordinal * 11;
        if bytes.get(marker) != Some(&1)
            || bytes.get(marker + 5..marker + 11) != Some(&[0u8; 6][..])
        {
            return empty;
        }
        let Some(record_index) = View::u32_le_at(bytes, marker + 1) else {
            return empty;
        };
        members.push(crate::records::identity::Located {
            value: record_index,
            offset: u64_from_index(marker + 1),
        });
    }
    members
}

/// Parse the counted member-record run of a legacy sketch container's paired
/// same-index record. The paired record stores its head-placement reference
/// at offset 19, six zero bytes, a u32 sketch ordinal and seven bytes of
/// state, then the member count at offset 41. Each member is a padded marked
/// reference.
fn parse_legacy_sketch_member_run(
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
) -> Option<Vec<crate::records::identity::Located<u32>>> {
    parse_legacy_sketch_member_run_with(bytes, primary_at, entity_suffix, false)
        .or_else(|| parse_legacy_sketch_member_run_with(bytes, primary_at, entity_suffix, true))
}

/// Target of the local reference at `at` in one spelling: the eleven-byte
/// `01 + u32 + six zero bytes` or the 51-byte typed reference.
fn legacy_sketch_reference_target(bytes: &[u8], at: usize, typed: bool) -> Option<u32> {
    if typed {
        crate::bytes::typed_local_reference_target(bytes, at)
    } else if bytes.get(at) == Some(&1) && bytes.get(at + 5..at + 11) == Some(&[0u8; 6][..]) {
        View::u32_le_at(bytes, at + 1)
    } else {
        None
    }
}

/// Parse the legacy counted sketch member run with one reference spelling.
///
/// The paired header is followed by eight zero bytes, a reference at offset
/// 19, eleven bytes, the u32 member count, and that many references; every
/// reference in the run uses the same spelling.
fn parse_legacy_sketch_member_run_with(
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
    typed: bool,
) -> Option<Vec<crate::records::identity::Located<u32>>> {
    let width = if typed {
        crate::bytes::TYPED_LOCAL_REFERENCE_LEN
    } else {
        11
    };
    let parsed = (|| {
        let paired_at = next_indexed_record_offset(bytes, primary_at + 11)?;
        if View::u32_le_at(bytes, paired_at + 7) != Some(entity_suffix)
            || bytes.get(paired_at + 11..paired_at + 19) != Some(&[0u8; 8][..])
            || legacy_sketch_reference_target(bytes, paired_at + 19, typed).is_none()
        {
            return None;
        }
        let (paired_class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
        if after_tag != paired_at + 7
            || paired_class_tag.len() != 3
            || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let count_at = paired_at + 19 + width + 11;
        let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
        if count == 0 {
            return None;
        }
        let run_end = (count_at + 4).checked_add(count.checked_mul(width)?)?;
        if run_end > bytes.len() {
            return None;
        }
        Some((count_at + 4, count))
    })();
    let (first, count) = parsed?;
    let mut members = Vec::new();
    for ordinal in 0..count {
        let marker = first + ordinal * width;
        members.push(crate::records::identity::Located {
            value: legacy_sketch_reference_target(bytes, marker, typed)?,
            offset: u64_from_index(marker + 1),
        });
    }
    Some(members)
}

/// Recognize either legacy sketch-container tail. A counted container owns
/// its complete member run. A localized container omits that run and is
/// accepted only when its paired record names an exact placement-head frame.
fn parse_legacy_sketch_container_members(
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
    records: &IndexedRecordOffsets,
) -> Option<Vec<crate::records::identity::Located<u32>>> {
    if let Some(members) = parse_legacy_sketch_member_run(bytes, primary_at, entity_suffix) {
        return Some(members);
    }
    let entity_id =
        crate::records::identity::DesignEntityId::from_parts("Sketch", u64::from(entity_suffix));
    parse_member_run_head_placement(bytes, u64_from_index(primary_at), &entity_id, records)
        .map(|_| Vec::new())
}

/// Decode every self-validating per-entity design `BulkStream` header (spec
/// [§3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header. Headers occur in
/// the fixed layout or in the `EntityGenesis` layout.
pub(crate) fn decode_entity_headers(
    scan: &ContainerScan,
) -> Result<Vec<DesignEntityHeader>, CodecError> {
    let mut out = Vec::new();
    // Entity ids are unique per Design stream, not archive-wide.
    let mut entity_modules = HashMap::<&str, HashMap<u64, &str>>::new();
    let types = decode_types(scan)?;
    let mut legacy_sketch_candidates = HashMap::<&str, std::collections::HashSet<u32>>::new();
    for design_type in &types {
        if let Some(stream) = native_stream(design_type.id()) {
            for &entity_id in design_type.entities.values() {
                insert_entity_module(&mut entity_modules, stream, entity_id, &design_type.module);
            }
        }
        if design_type.module == DESIGN_MODULE_SKETCH {
            let Some(stream) = native_stream(design_type.id()) else {
                continue;
            };
            let Some(prefix) = stream
                .strip_prefix(ids::SCHEME_PREFIX)
                .and_then(|name| name.strip_suffix("MetaStream.dat"))
            else {
                continue;
            };
            for identity in design_type
                .entities
                .values()
                .copied()
                .filter_map(|id| u32::try_from(id).ok())
            {
                insert_legacy_candidate(&mut legacy_sketch_candidates, prefix, identity);
            }
        }
    }
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        // Modules come from the type table of this stream's own `MetaStream`.
        let scope = crate::ids::native_scope(&entry.name);
        let meta_scope = entity_meta_scope(&scope);
        let stream_modules = meta_scope
            .as_ref()
            .and_then(|name| entity_modules.get(name.as_str()));
        for header in indexed_record_offsets(bytes) {
            let start = header.offset;
            let class_tag = header.class_tag.clone();
            let settled = parse_settled_entity_header(bytes, start);
            let genesis_form = settled.is_none();
            let Some(NamedEntityHeader {
                entity_id,
                optional_slot_present,
                end,
                ..
            }) = (match settled {
                Some(header) => Some(header),
                None => parse_genesis_entity_header(bytes, start),
            })
            else {
                continue;
            };
            let entity_suffix = entity_id.suffix();
            let module = stream_modules
                .and_then(|modules| modules.get(&entity_suffix))
                .map(|module| (*module).to_owned());
            let in_sketch_module = module.as_deref() == Some(DESIGN_MODULE_SKETCH);
            let list = if in_sketch_module {
                decode_reference_list(bytes, end)
            } else {
                None
            };
            let record_end = list.as_ref().map_or(end, |list| list.end);
            let references =
                list.map(
                    |list| crate::records::entity_header::SketchHeaderReferences {
                        record_reference: list.record_reference.value,
                        record_reference_offset: list.record_reference.offset,
                        references: list.references,
                    },
                );
            let members = if genesis_form && in_sketch_module {
                parse_sketch_member_run(bytes, record_end, entity_suffix)
            } else {
                Vec::new()
            };
            out.push(DesignEntityHeader {
                id: design_record_id(&entry.name, ":design-entity-header#", u64_from_index(start)),
                byte_offset: u64_from_index(start),
                entity_id,
                class_tag,
                optional_slot_present,
                registration: crate::records::entity_header::DesignEntityRegistration::new(
                    module,
                    references,
                    crate::records::identity::ReferenceRun::located(members),
                )
                .map_err(CodecError::Malformed)?,
            });
        }

        // Legacy Design streams do not carry textual entity headers. Their
        // MSketch metadata names candidate record indices; only actual sketch
        // containers have a consecutive same-index pair with the legacy
        // counted member run. Materialize the same ownership abstraction used
        // by later entity-header forms so downstream binding remains uniform.
        let candidates = entry
            .name
            .strip_suffix("BulkStream.dat")
            .and_then(|prefix| legacy_sketch_candidates.get(prefix));
        let Some(candidates) = candidates.filter(|candidates| !candidates.is_empty()) else {
            continue;
        };
        let records = IndexedRecordOffsets::build(bytes);
        let mut existing = std::collections::HashSet::new();
        for entity in out
            .iter()
            .filter(|entity| native_stream(&entity.id) == Some(scope.as_str()))
        {
            let Ok(index) = u32::try_from(entity.entity_id.suffix()) else {
                continue;
            };
            existing.insert(index);
        }
        for header in indexed_record_offsets(bytes) {
            let start = header.offset;
            let entity_suffix = header.record_index;
            if !candidates.contains(&entity_suffix) || existing.contains(&entity_suffix) {
                continue;
            }
            let class_tag = header.class_tag.clone();
            let Some(members) =
                parse_legacy_sketch_container_members(bytes, start, entity_suffix, &records)
            else {
                continue;
            };
            existing.insert(entity_suffix);
            out.push(DesignEntityHeader {
                id: design_record_id(&entry.name, ":design-entity-header#", u64_from_index(start)),
                byte_offset: u64_from_index(start),
                entity_id: crate::records::identity::DesignEntityId::from_parts(
                    "Sketch",
                    u64::from(entity_suffix),
                ),
                class_tag,
                optional_slot_present: false,
                registration: crate::records::entity_header::DesignEntityRegistration::new(
                    Some(DESIGN_MODULE_SKETCH.to_owned()),
                    None,
                    crate::records::identity::ReferenceRun::located(members),
                )
                .map_err(CodecError::Malformed)?,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn insert_entity_module<'a>(
    entity_modules: &mut HashMap<&'a str, HashMap<u64, &'a str>>,
    stream: &'a str,
    entity_id: u64,
    module: &'a str,
) {
    let stream_modules = entity_modules.entry(stream).or_default();
    stream_modules.entry(entity_id).or_insert(module);
}

fn insert_legacy_candidate<'a>(
    streams: &mut HashMap<&'a str, std::collections::HashSet<u32>>,
    prefix: &'a str,
    identity: u32,
) {
    let candidates = streams.entry(prefix).or_default();
    candidates.insert(identity);
}

/// Decode the indexed dynamic-class record headers ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)) that `entities`'
/// reference-list entries point at: a `u32` record index and a three-digit
/// class tag, for each record index named by any [`DesignEntityHeader`] in
/// `entities`.
pub(crate) fn decode_record_headers(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    // Sketch-relation records name their sketch through `ParentNode`; a
    // legacy counted member run does not list them. The stream's type table
    // registers every relation-class entity, and an entity ID repeats its
    // record index, so those registrations name the relation records.
    let types = decode_types(scan)?;
    let mut relation_records = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let scope = crate::ids::native_scope(&entry.name);
        let stream_types = stream_types_by_class_tag(&types, &entry.name);
        for record_index in registered_relation_record_indices(stream_types.into_values()) {
            relation_records.push((scope.clone(), record_index));
        }
    }
    let references = entities
        .iter()
        .flat_map(|entity| {
            native_stream(&entity.id)
                .into_iter()
                .flat_map(move |scope| {
                    entity
                        .reference_values()
                        .map(move |record_index| (scope, *record_index))
                })
        })
        .chain(
            relation_records
                .iter()
                .map(|(scope, record_index)| (scope.as_str(), *record_index)),
        );
    let wanted = wanted_record_indices(references);
    decode_headers_for_indices(scan, &wanted)
}

/// Record indices of the sketch-relation records a type table registers:
/// every entity of a sketch-relation class type, whose entity ID repeats its
/// record index.
pub(in crate::design) fn registered_relation_record_indices<'a>(
    types: impl IntoIterator<Item = &'a crate::records::entity_header::SegmentType>,
) -> Vec<u32> {
    let mut out = Vec::new();
    for design_type in types {
        if SketchRelationClass::of(design_type.type_guid.as_str(), design_type.version).is_none() {
            continue;
        }
        for &entity_id in design_type.entities.values() {
            let Ok(record_index) = u32::try_from(entity_id) else {
                continue;
            };
            out.push(record_index);
        }
    }
    out
}

/// Decode the indexed dynamic-class record headers ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)) named by
/// `indices` directly, bypassing entity reference lists. Used to fetch record
/// headers referenced by records other than [`DesignEntityHeader`] (for
/// example, sketch relation records).
pub(crate) fn decode_related_record_headers(
    scan: &ContainerScan,
    indices: &[(String, u32)],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = wanted_record_indices(
        indices
            .iter()
            .map(|(scope, index)| (scope.as_str(), *index)),
    );
    decode_headers_for_indices(scan, &wanted)
}

fn wanted_record_indices<'a>(
    indices: impl IntoIterator<Item = (&'a str, u32)>,
) -> std::collections::HashSet<(&'a str, u32)> {
    let mut wanted = std::collections::HashSet::new();
    for index in indices {
        if !wanted.contains(&index) {
            wanted.insert(index);
        }
    }
    wanted
}

fn decode_headers_for_indices(
    scan: &ContainerScan,
    wanted: &std::collections::HashSet<(&str, u32)>,
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        super::sheet_metal_settings::is_settings_stream(scan, entry, ContainerRole::Bulkstream)
    }) {
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope = crate::ids::native_scope(&entry.name);
        decode_headers_for_indices_from_stream(&entry.name, &scope, bytes, wanted, &mut out);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn decode_headers_for_indices_from_stream(
    stream_name: &str,
    scope: &str,
    bytes: &[u8],
    wanted: &std::collections::HashSet<(&str, u32)>,
    out: &mut Vec<DesignRecordHeader>,
) {
    let mut emitted = std::collections::HashSet::new();
    for header in indexed_record_offsets(bytes) {
        let position = header.offset;
        let record_index = header.record_index;
        if wanted.contains(&(scope, record_index)) && !emitted.contains(&record_index) {
            emitted.insert(record_index);

            out.push(DesignRecordHeader {
                id: design_record_id(
                    stream_name,
                    ":design-record-header#",
                    u64_from_index(position),
                ),
                record_index,
                class_tag: header.class_tag,
                byte_offset: u64_from_index(position),
            });
        }
    }
}

/// Decode the sketch-relation body at each `records` entry's offset: the
/// owning sketch relation's member reference list, owner reference, state,
/// and return-member list. `records` supplies the byte offsets and class tags
/// (typically from [`decode_related_record_headers`]).
pub(crate) fn decode_sketch_relations(
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    // A record carries no class identity of its own: its class tag selects an
    // entry in its segment's own type table, and only that entry's GUID names
    // the class across segments.
    let types = decode_types(scan)?;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream_types = stream_types_by_class_tag(&types, &entry.name);
        let scope = ids::native_scope(&entry.name);
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut framed_offsets = std::collections::HashSet::new();
        if let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? {
            for frame in design_primary_frames(bytes, &meta)? {
                let design_type = frame.design_type;
                if !matches!(
                    SketchRelationClass::of(design_type.type_guid.as_str(), design_type.version),
                    Some(
                        SketchRelationClass::Tangent
                            | SketchRelationClass::CircularPattern
                            | SketchRelationClass::RectangularPattern
                            | SketchRelationClass::TextFrame
                    )
                ) {
                    continue;
                }
                let record_index = u32::try_from(frame.entity_id).map_err(|_| {
                    CodecError::malformed("F3D sketch-relation entity ID exceeds u32")
                })?;
                let record = DesignRecordHeader {
                    id: design_record_id(
                        &entry.name,
                        ":design-record-header#",
                        u64_from_index(frame.start),
                    ),
                    record_index,
                    class_tag: frame.class_tag,
                    byte_offset: u64_from_index(frame.start),
                };
                framed_offsets.insert(record.byte_offset);
                decode_sketch_relation_record(
                    &mut out,
                    &entry.name,
                    &record,
                    &bytes[frame.start..frame.end],
                    design_type.type_guid.as_str(),
                    design_type.version,
                )?;
            }
        }
        for record in records
            .iter()
            .filter(|record| native_stream(&record.id) == Some(scope.as_str()))
        {
            if framed_offsets.contains(&record.byte_offset) {
                continue;
            }
            let Ok(at) = usize::try_from(record.byte_offset) else {
                continue;
            };
            let record_end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            let Some(payload) = bytes.get(at..record_end) else {
                continue;
            };
            let Some(design_type) = stream_types.get(&record.class_tag.code()) else {
                continue;
            };
            decode_sketch_relation_record(
                &mut out,
                &entry.name,
                record,
                payload,
                design_type.type_guid.as_str(),
                design_type.version,
            )?;
        }
    }
    Ok(out)
}

pub(crate) fn decode_sketch_relation_record(
    out: &mut Vec<SketchRelation>,
    stream: &str,
    record: &DesignRecordHeader,
    payload: &[u8],
    type_guid: &str,
    version: u32,
) -> Result<(), CodecError> {
    let Some(class) = SketchRelationClass::of(type_guid, version) else {
        return Ok(());
    };
    let Some(mut parsed) = parse_classed_sketch_relation(payload, class)? else {
        return Ok(());
    };
    if payload
        .get(parsed.parsed_end..)
        .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
    {
        return Ok(());
    }
    let pattern = decode_pattern_definition(payload, &mut parsed);
    let Ok(definition) =
        crate::records::sketch_relations::SketchRelationDefinition::new(parsed.state, pattern)
    else {
        return Ok(());
    };
    admit_sketch_relation(out, stream, record, payload, parsed, definition)
}

pub(crate) fn rectangular_pattern_flags(payload: &[u8]) -> Option<[u8; 3]> {
    let parsed =
        parse_classed_sketch_relation(payload, SketchRelationClass::RectangularPattern).ok()??;
    if parsed.state != 0x2000_0000
        || payload
            .get(parsed.parsed_end..)?
            .iter()
            .any(|byte| *byte != 0)
    {
        return None;
    }
    let RelationClassMembers::Rectangular { flags, .. } = parsed.class_members else {
        return None;
    };
    Some(flags)
}

pub(crate) fn sketch_pattern_neutral_state(relation: &SketchRelation) -> Option<bool> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    let class = match relation.definition.pattern()? {
        SketchPatternDefinition::Circular { .. } => SketchRelationClass::CircularPattern,
        SketchPatternDefinition::Rectangular { .. } => SketchRelationClass::RectangularPattern,
        _ => return None,
    };
    let parsed = parse_classed_sketch_relation(relation.raw_bytes(), class).ok()??;
    if parsed.state != relation.definition.state()
        || relation
            .raw_bytes()
            .get(parsed.parsed_end..)?
            .iter()
            .any(|byte| *byte != 0)
    {
        return None;
    }
    match parsed.class_members {
        RelationClassMembers::CircularPattern {
            has_suppression, ..
        } => Some(!has_suppression),
        RelationClassMembers::Rectangular {
            flags,
            has_suppression,
            ..
        } => Some(!has_suppression && matches!(flags, [0 | 1, 0, 0])),
        _ => None,
    }
}

fn admit_sketch_relation(
    out: &mut Vec<SketchRelation>,
    stream: &str,
    record: &DesignRecordHeader,
    payload: &[u8],
    parsed: ParsedSketchRelation,
    definition: crate::records::sketch_relations::SketchRelationDefinition,
) -> Result<(), CodecError> {
    let rectangular_counted_reference_count = match &parsed.class_members {
        RelationClassMembers::Rectangular {
            reference_count, ..
        } => Some(*reference_count),
        _ => None,
    };
    let members = crate::records::sketch_relations::SketchRelationMembers::from_indices(
        parsed.members.into_iter().map(|member| {
            (
                member.reference.value,
                member.reference.offset,
                member.relation_ordinal,
            )
        }),
    )?;
    let return_members =
        crate::records::sketch_relations::SketchRelationReturnMembers::from_indices(
            parsed
                .return_members
                .into_iter()
                .map(|member| (member.value, member.offset)),
        )?;

    let mut auxiliary_references = Vec::new();
    for row in parsed.auxiliary_references {
        auxiliary_references.push(crate::records::identity::Located {
            value: row.value,
            offset: u32::try_from(row.offset).map_err(|_| {
                CodecError::malformed("f3d sketch relation auxiliary offset exceeds u32")
            })?,
        });
    }
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: design_record_id(stream, ":sketch-relation#", u64::from(record.record_index)),
        record_index: record.record_index,
        class_tag: record.class_tag.clone(),
        byte_offset: record.byte_offset,
        state_offset: u32::try_from(parsed.state_offset)
            .map_err(|_| CodecError::malformed("f3d sketch relation state offset exceeds u32"))?,
        owner_reference: parsed.owner_reference,
        owner_entity_id: None,
        owner_reference_offset: u32::try_from(parsed.owner_reference_offset)
            .map_err(|_| CodecError::malformed("f3d sketch relation owner offset exceeds u32"))?,
        auxiliary_references: crate::records::identity::ReferenceRun::located(auxiliary_references),
        rectangular_counted_reference_count,
        members,
        definition,
        entity_genesis: parsed.entity_genesis,
        return_members,
        raw_bytes: payload.to_vec(),
    })
    .map_err(|error| CodecError::malformed(error.to_string()))?;
    out.push(relation);
    Ok(())
}

/// Decode the pattern definition a relation's class members carry, reading them
/// beside the auxiliary references the parse recorded. Circular patterns store
/// optional angle- and count-parameter references, the evaluated f64 total
/// angle, and the evaluated u32 instance count. Rectangular direction clauses
/// store an evaluated count, optional count reference, unit direction, evaluated
/// signed distance, and optional distance reference. Text-frame relations retain
/// the second class reference as the text entity.
fn decode_pattern_definition(
    payload: &[u8],
    parsed: &mut ParsedSketchRelation,
) -> Option<crate::records::sketch_relations::SketchPatternDefinition> {
    use crate::records::sketch_relations::{SketchPatternDefinition, SketchPatternDirection};
    match &mut parsed.class_members {
        RelationClassMembers::CircularPattern {
            angle_parameter,
            count_parameter,
            evaluated_offset,
            ..
        } => {
            if parsed.state != 0x1000_0000 {
                return None;
            }
            let evaluated_angle = FiniteReal::new(View::f64_le_at(payload, *evaluated_offset)?)?;
            let evaluated_count = crate::records::sketch_relations::SketchPatternCount::try_from(
                View::u32_le_at(payload, *evaluated_offset + 8)?,
            )
            .ok()?;
            return Some(SketchPatternDefinition::Circular {
                angle_parameter: *angle_parameter,
                count_parameter: *count_parameter,
                evaluated_angle,
                evaluated_count,
            });
        }
        RelationClassMembers::Rectangular { clauses, .. } => {
            if parsed.state != 0x2000_0000 {
                return None;
            }
            let directions = clauses.map(|clause| {
                let evaluated_count =
                    crate::records::sketch_relations::SketchPatternCount::try_from(
                        View::u32_le_at(payload, clause.count_offset)?,
                    )
                    .ok()?;
                let direction = [
                    View::f64_le_at(payload, clause.direction_offset)?,
                    View::f64_le_at(payload, clause.direction_offset + 8)?,
                    View::f64_le_at(payload, clause.direction_offset + 16)?,
                ];
                Some(SketchPatternDirection {
                    evaluated_count,
                    count_parameter: clause.count_parameter,
                    direction: direction.try_into().ok()?,
                    evaluated_distance: FiniteReal::new(View::f64_le_at(
                        payload,
                        clause.direction_offset + 24,
                    )?)?,
                    distance_parameter: clause.distance_parameter,
                })
            });
            let [first, second] = directions;
            return Some(SketchPatternDefinition::Rectangular {
                directions: [first?, second?],
            });
        }
        RelationClassMembers::TextFrame { text_reference } => {
            let text_reference = (*text_reference)?;
            if parsed.state == 0x100_0000_0000
                && parsed
                    .members
                    .iter()
                    .any(|member| member.reference.value == text_reference)
            {
                return Some(SketchPatternDefinition::TextFrame { text_reference });
            }
        }
        RelationClassMembers::TextPath { glyph_transforms } => {
            if parsed.state == 0x200_0000_0000
                && parsed.auxiliary_references.len() == 1
                && parsed
                    .members
                    .iter()
                    .any(|member| member.reference.value == parsed.auxiliary_references[0].value)
            {
                return Some(SketchPatternDefinition::TextPath {
                    text_reference: parsed.auxiliary_references[0].value,
                    glyph_transforms: std::mem::take(glyph_transforms),
                });
            }
        }
        RelationClassMembers::Plain | RelationClassMembers::Tangent => {}
    }
    None
}

fn trailing_sketch_owner_reference(record: &[u8]) -> Option<u32> {
    let tail = record.len().checked_sub(11)?;
    if record.get(tail) != Some(&1) || record.get(tail + 5..tail + 11) != Some(&[0u8; 6][..]) {
        return None;
    }
    View::u32_le_at(record, tail + 1)
}

fn decode_sketch_points_from_stream(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchPoint>, CodecError> {
    let frames = design_primary_frames(bytes, meta)?;
    let mut frames_by_entity = HashMap::new();
    let mut types_by_entity = HashMap::new();
    for frame in &frames {
        let Ok(entity_id) = u32::try_from(frame.entity_id) else {
            continue;
        };
        frames_by_entity.insert(entity_id, frame);
        types_by_entity.insert(
            entity_id,
            (
                frame.design_type.type_guid.as_str(),
                frame.design_type.version,
                frame.design_type.module.as_str(),
            ),
        );
    }
    let mut out = Vec::new();
    for frame in &frames {
        let derived = is_derived_sketch_point_type(frame.design_type);
        if !derived
            && (!frame
                .design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(SKETCH_POINT_TYPE_GUID)
                || frame.design_type.module != CURRENT_SKETCH_POINT_TYPE.2
                || ![0, 8, 10, CURRENT_SKETCH_POINT_TYPE.1].contains(&frame.design_type.version))
        {
            continue;
        }
        let payload = &bytes[frame.start..frame.end];
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-point entity ID exceeds u32".into()))?;
        let decoded = if derived {
            decode_derived_sketch_point_record(bytes, meta, frame)
        } else {
            decode_sketch_point_record(payload, frame.design_type.version)
        }
        .ok_or_else(|| {
            crate::design::text::malformed_design(format_args!(
                "F3D sketch point {record_index} has an invalid version-{} member sequence",
                frame.design_type.version
            ))
        })?;
        let (u, v) = (decoded.coordinates[0] * 10.0, decoded.coordinates[1] * 10.0);
        if !point_target_has_guid(
            &types_by_entity,
            decoded.trailing_reference(),
            SKETCH_CONTAINER_TYPE_GUID,
        ) {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D sketch point {record_index} has an invalid trailing container reference"
            )));
        }
        let (record_form, companion) = frames_by_entity
            .get(&decoded.paired_reference)
            .filter(|companion_frame| {
                let design_type = companion_frame.design_type;
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0)
                    && design_type.version == SKETCH_POINT_COMPANION_TYPE.1
                    && design_type.module == SKETCH_POINT_COMPANION_TYPE.2
            })
            .and_then(|companion_frame| {
                decode_sketch_point_companion(
                    &bytes[companion_frame.start..companion_frame.end],
                    record_index,
                    decoded.record_form.clone(),
                    &types_by_entity,
                )
            })
            .ok_or_else(|| {
                crate::design::text::malformed_design(format_args!(
                    "F3D sketch point {record_index} has no valid inverse companion"
                ))
            })?;

        out.push(
            SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
                id: design_record_id(stream, ":sketch-point#", u64_from_index(frame.start)),
                record_index,
                owner_reference: decoded.owner_reference,
                class_tag: frame.class_tag.clone(),
                byte_offset: u64_from_index(frame.start),
                coordinate_offset: decoded.coordinate_offset,
                record_form,
                companion,
                paired_reference: decoded.paired_reference,
                coordinates: Point2::new(u, v),
            })
            .map_err(CodecError::Malformed)?,
        );
    }
    Ok(out)
}

/// Decode every sketch-point record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `pt_tag`) from each design
/// `BulkStream` entry in `scan`: its versioned identity, flags, coordinates,
/// closure, trailing reference, and paired reverse curve-incidence record,
/// converted centimetre→millimetre. Class version 0 supplies `(u,v)` and no
/// persistent identity; later forms supply `(u,v,w)` and `pt_tag`. A known
/// point record with a malformed or non-finite member sequence makes the
/// stream malformed.
pub(crate) fn decode_sketch_points(scan: &ContainerScan) -> Result<Vec<SketchPoint>, CodecError> {
    decode_sketch_streams(scan, decode_sketch_points_from_stream)
}

struct SketchProperty<'a> {
    name: &'a str,
    value: u64,
}

/// Read a class property block: a presence byte, and when it is `01`, a u32
/// count and that many `(key, type name, value)` triples.
///
/// Which keys a record carries varies by record, so a caller addresses a
/// property by name. Reading the block by fixed offset misframes every record
/// whose key set differs from the one the offsets were taken from.
struct SketchProperties<'a> {
    payload: &'a [u8],
    first: usize,
    count: usize,
}

impl SketchProperties<'_> {
    fn find(&self, key: &str) -> Option<u64> {
        let mut cursor = self.first;
        for _ in 0..self.count {
            let (name, after_name) =
                lp_ascii_filtered_view(self.payload, cursor, 0..=256, u8::is_ascii_graphic)?;
            let (_, after_type) =
                lp_ascii_filtered_view(self.payload, after_name, 0..=256, u8::is_ascii_graphic)?;
            let value = View::u64_le_at(self.payload, after_type)?;
            if name == key {
                return Some(value);
            }
            cursor = after_type.checked_add(8)?;
        }
        None
    }

    fn first_two(&self) -> Option<(SketchProperty<'_>, Option<SketchProperty<'_>>)> {
        if !matches!(self.count, 1 | 2) {
            return None;
        }
        let (first_name, after_name) =
            lp_ascii_filtered_view(self.payload, self.first, 0..=256, u8::is_ascii_graphic)?;
        let (_, after_type) =
            lp_ascii_filtered_view(self.payload, after_name, 0..=256, u8::is_ascii_graphic)?;
        let first_value = View::u64_le_at(self.payload, after_type)?;
        let second = if self.count == 2 {
            let at = after_type.checked_add(8)?;
            let (name, after_name) =
                lp_ascii_filtered_view(self.payload, at, 0..=256, u8::is_ascii_graphic)?;
            let (_, after_type) =
                lp_ascii_filtered_view(self.payload, after_name, 0..=256, u8::is_ascii_graphic)?;
            Some(SketchProperty {
                name,
                value: View::u64_le_at(self.payload, after_type)?,
            })
        } else {
            None
        };
        Some((
            SketchProperty {
                name: first_name,
                value: first_value,
            },
            second,
        ))
    }
}

fn read_property_block<'a>(payload: &'a [u8], cursor: &mut usize) -> Option<SketchProperties<'a>> {
    let mut count = 0;
    let mut first = *cursor;
    match payload.get(*cursor)? {
        0 => *cursor += 1,
        1 => {
            *cursor += 1;
            count = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
            if count > MAX_RELATION_RUN {
                return None;
            }
            *cursor += 4;
            first = *cursor;
            for _ in 0..count {
                let (_, after_key) =
                    lp_ascii_filtered_view(payload, *cursor, 0..=256, u8::is_ascii_graphic)?;
                let (type_name, after_type) =
                    lp_ascii_filtered_view(payload, after_key, 0..=256, u8::is_ascii_graphic)?;
                if type_name != "IntrinsicMetaTypeuint64" {
                    return None;
                }
                View::u64_le_at(payload, after_type)?;
                *cursor = after_type.checked_add(8)?;
            }
        }
        _ => return None,
    }
    Some(SketchProperties {
        payload,
        first,
        count,
    })
}

const SKETCH_TEXT_TYPE_GUIDS: [&str; 2] = [
    "E0618268-3A06-450E-9E94-7CF4C2E66802",
    "F0B1AFA3-3BAF-42D0-B2F3-94B95662F2A9",
];

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
fn decode_sketch_texts_from_stream(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchText>, CodecError> {
    let mut out = Vec::new();
    for frame in design_primary_frames(bytes, meta)? {
        if !SKETCH_TEXT_TYPE_GUIDS.iter().any(|type_guid| {
            frame
                .design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(type_guid)
        }) {
            continue;
        }
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-text entity ID exceeds u32".into()))?;
        let class_tag = frame.class_tag;
        let payload = &bytes[frame.start..frame.end];
        if let Some(text) = decode_sketch_text_record(
            payload,
            stream,
            class_tag,
            frame.design_type.version,
            record_index,
            frame.start,
        )? {
            out.push(text);
        }
    }
    Ok(out)
}

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
pub(crate) fn decode_sketch_texts(scan: &ContainerScan) -> Result<Vec<SketchText>, CodecError> {
    decode_sketch_streams(scan, decode_sketch_texts_from_stream)
}

/// Whether a sketch-text record carries one of the two parameter-reference
/// members. A record either writes the member, whose own presence byte then
/// says whether it targets a parameter, or omits the member entirely; nothing
/// ahead of the slot distinguishes the two.
#[derive(Clone, Copy)]
enum TextReferenceSlot {
    Omitted,
    Written,
}

const TEXT_REFERENCE_SLOTS: [TextReferenceSlot; 2] =
    [TextReferenceSlot::Omitted, TextReferenceSlot::Written];

/// Bytes between a sketch-text record's last class member and its
/// owning-sketch reference, in either identity form.
const SKETCH_TEXT_TRAILING_RUN: usize = 30;

/// How far a placement-transform element may sit from the constant a planar
/// rigid placement writes there. The transform is composed in floating point,
/// so its constants arrive rounded; the bound is far tighter than a run of
/// misframed bytes would meet.
const TEXT_PLACEMENT_TOLERANCE: f64 = 1.0e-9;

/// Bytes between the end of the stored `txt_tag` rotation and the four f32 RGBA
/// components: one style byte and four residual bytes.
const TXT_TAG_POST_ROTATION_RUN: usize = 5;

/// Bytes between the text-anchor coordinates and the u32 text count in the
/// `txt_tag` form, from [`TXT_TAG_ANCHOR_MEMBER_VERSION`] onward.
const TXT_TAG_ANCHOR_RUN: usize = 11;

/// The `txt_tag` class version that adds the eleventh byte of the run between
/// the text anchor and the text count. Below it the run is ten bytes.
const TXT_TAG_ANCHOR_MEMBER_VERSION: u32 = 4;

/// The `txt_tag` class version from which a record writes its persistent
/// identity as a property-block key. A property block carrying neither identity
/// key belongs to a `txt_tag` record below this version, which stores no
/// persistent identity at all.
const TXT_TAG_IDENTITY_KEY_VERSION: u32 = 4;

/// Three unknown bytes, one i32 font weight, and eight further bytes between
/// the `txt_tag` form's counted reference run and the thirty-byte class tail.
const TXT_TAG_MEMBER_RUN: usize = 15;
const TXT_TAG_FONT_WEIGHT_AT: usize = 3;

/// Which identity key a sketch-text record carries. The key selects the layout
/// the record uses from the property block onward.
#[derive(Clone, Copy, PartialEq)]
enum SketchTextIdentity {
    /// `textex_tag`: a `1` byte, the width factor, a zero byte between the font
    /// family and the height, and the anchor inside a placement transform.
    TextexTag { width_factor: NonNegativeReal },
    /// `txt_tag`: a `0` byte, no width factor, the height directly after the
    /// font family, and the anchor stored on its own.
    TxtTag { rotation: Angle },
}

/// Read one parameter-reference slot in the given form, advancing `cursor` by
/// what that form occupies. An omitted member reads as a null reference.
fn read_text_reference<'a>(
    payload: &'a [u8],
    cursor: &mut usize,
    slot: TextReferenceSlot,
) -> Option<Reference<&'a str, crate::bytes::utf16::Utf16View<'a>>> {
    match slot {
        TextReferenceSlot::Omitted => Some(Reference::Null),
        TextReferenceSlot::Written => take_reference(payload, cursor),
    }
}

/// Read the four f32 RGBA colour components both sketch-text classes store,
/// advancing `cursor` past them. Every component is a fraction of full
/// intensity, so a run leaving `[0, 1]` says the record is misframed rather
/// than that the text carries an out-of-range colour.
fn read_sketch_text_color(payload: &[u8], cursor: &mut usize) -> Option<Color> {
    let mut view = View::over_retained(payload);
    view.seek(*cursor)?;
    let mut components = [0f32; 4];
    for component in &mut components {
        let value = view.f32_le()?;
        (0.0..=1.0).contains(&value).then_some(())?;
        *component = value;
    }
    *cursor = view.position();
    let [r, g, b, a] = components;
    Color::new(r, g, b, a)
}

/// Placement a frame-text record's 4×4 transform describes.
enum FrameTextPlacement {
    /// A planar rigid placement: an anchor in millimetres and a rotation.
    Planar(TextPlacement<FinitePoint2>),
    /// The sketch plane turned over: the text is mirrored in the sketch.
    Mirrored,
}

/// Read the row-major 4×4 f64 placement transform a frame-text record stores,
/// advancing `cursor` past it.
///
/// The transform is a rigid placement with the bottom row `(0, 0, 0, 1)` and
/// its translation in the last column. Its third row is `(0, 0, 1, 0)` with a
/// 2×2 rotation basis of determinant `+1`, or `(0, 0, -1, 0)` with a 2×2
/// reflection basis of determinant `-1`. A run failing that is not a
/// placement, so the record is misframed.
fn read_text_placement(payload: &[u8], cursor: &mut usize) -> Option<FrameTextPlacement> {
    let elements = f64s_at::<16>(payload, *cursor)?;
    *cursor = cursor.checked_add(128)?;
    let at = |row: usize, column: usize| elements[row * 4 + column];
    let constant = |value: f64, expected: f64| (value - expected).abs() <= TEXT_PLACEMENT_TOLERANCE;
    let normal = at(2, 2);
    let mirrored = constant(normal, -1.0);
    let planar = [
        (0, 2, 0.0),
        (1, 2, 0.0),
        (2, 0, 0.0),
        (2, 1, 0.0),
        (2, 3, 0.0),
        (3, 0, 0.0),
        (3, 1, 0.0),
        (3, 2, 0.0),
        (3, 3, 1.0),
    ]
    .into_iter()
    .all(|(row, column, expected)| constant(at(row, column), expected));
    let determinant = at(0, 0) * at(1, 1) - at(0, 1) * at(1, 0);
    (planar && (mirrored || constant(normal, 1.0)) && constant(determinant, normal))
        .then_some(())?;
    if mirrored {
        return Some(FrameTextPlacement::Mirrored);
    }
    let anchor = Point2::new(at(0, 3) * 10.0, at(1, 3) * 10.0);
    let anchor = FinitePoint2::new(anchor)?;
    Some(FrameTextPlacement::Planar(TextPlacement {
        anchor,
        rotation: Angle::new(at(1, 0).atan2(at(0, 0)))?,
    }))
}

/// The record index a reference names, absent when the reference is null.
fn reference_index<G: AsRef<str>, L>(reference: &Reference<G, L>) -> Option<u32> {
    reference
        .target()
        .and_then(|target| u32::try_from(target).ok())
}

/// Class-level fields of a sketch-text record, ending at the text height.
struct SketchTextHead {
    entity_genesis: Option<u64>,
    persistent_id: Option<u64>,
    base_id: Option<u64>,
    font_family: String,
    height: PositiveLength,
    color: Color,
    style: u8,
    cursor: usize,
}

/// Fields following the height, whose framing depends on the two slot forms.
struct SketchTextTail {
    layout: SketchTextLayout,
    text: String,
    font_weight: i32,
    owner_reference: u32,
    neutral_orientation: bool,
}

/// Read the class-defined leading block: the presence byte, and when it is
/// `01`, a u32 count and that many `(reference, u32)` pairs. The `txt_tag` form
/// writes such a block; the `textex_tag` form writes the `00` byte alone.
fn read_sketch_text_leading_block(payload: &[u8], cursor: &mut usize) -> Option<()> {
    match payload.get(*cursor)? {
        0 => *cursor += 1,
        1 => {
            *cursor += 1;
            let count = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
            if count > MAX_RELATION_RUN {
                return None;
            }
            *cursor = cursor.checked_add(4)?;
            for _ in 0..count {
                take_reference(payload, cursor)?;
                View::u32_le_at(payload, *cursor)?;
                *cursor = cursor.checked_add(4)?;
            }
        }
        _ => return None,
    }
    Some(())
}

/// Read the record prefix, property block, and the metrics up to the height.
fn decode_sketch_text_head(
    payload: &[u8],
    class_version: u32,
) -> Result<Option<(SketchTextHead, SketchTextIdentity)>, CodecError> {
    (|| {
        // Record prefix: the LP-ASCII class tag, the u64 entity ID, and the
        // LP-ASCII record name.
        let (_, after_tag) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit)?;
        let (_, mut cursor) = lp_ascii_filtered_view(
            payload,
            after_tag.checked_add(8)?,
            0..=256,
            u8::is_ascii_graphic,
        )?;
        read_sketch_text_leading_block(payload, &mut cursor)?;
        // The block carries the text identities under keys that vary by record, so
        // each is addressed by name. The identity key is `textex_tag` or `txt_tag`,
        // and which of the two the record carries selects the layout that follows.
        // A `txt_tag` record below TXT_TAG_IDENTITY_KEY_VERSION writes neither key
        // and carries no persistent identity, so its class version selects the
        // layout in place of a key.
        let properties = read_property_block(payload, &mut cursor)?;
        let property = |key: &str| properties.find(key);
        let is_txt_tag = match (property("textex_tag"), property("txt_tag")) {
            (Some(_), _) => false,
            (None, Some(_)) => true,
            (None, None) if class_version < TXT_TAG_IDENTITY_KEY_VERSION => true,
            (None, None) => return None,
        };
        let (identity, color, persistent_id, style) = if is_txt_tag {
            let rotation = Angle::new(View::f64_le_at(payload, cursor)?)?;
            cursor = cursor.checked_add(8)?;
            let style = *payload.get(cursor)?;
            cursor = cursor.checked_add(TXT_TAG_POST_ROTATION_RUN)?;
            let color = read_sketch_text_color(payload, &mut cursor)?;
            (
                SketchTextIdentity::TxtTag { rotation },
                color,
                property("txt_tag"),
                style,
            )
        } else {
            (payload.get(cursor)? == &1).then_some(())?;
            cursor += 1;
            let width_factor = NonNegativeReal::new(View::f64_le_at(payload, cursor)?)?;
            cursor = cursor.checked_add(8)?;
            let color = read_sketch_text_color(payload, &mut cursor)?;
            (
                SketchTextIdentity::TextexTag { width_factor },
                color,
                property("textex_tag"),
                0,
            )
        };
        let font_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if font_count == 0 || font_count > 1_024 {
            return None;
        }
        let (font_family, after_font) = lp_utf16_bounded(payload, cursor, font_count..=font_count)?;
        cursor = after_font;
        if matches!(identity, SketchTextIdentity::TextexTag { .. }) {
            (payload.get(cursor)? == &0).then_some(())?;
            cursor += 1;
        }
        let height = PositiveLength::new(View::f64_le_at(payload, cursor)? * 10.0)?;
        cursor = cursor.checked_add(8)?;
        Some(Ok((
            SketchTextHead {
                entity_genesis: property("EntityGenesis"),
                persistent_id,
                base_id: property("txt_tag_base"),
                font_family,
                height,
                color,
                style,
                cursor,
            },
            identity,
        )))
    })()
    .transpose()
}

/// Read the indexed Design form of a `textex_tag` record. It has no leading
/// relation block or record-name string: the indexed header is followed by a
/// nine-byte zero entity lane and the ordinary property block. Its one-byte
/// width prefix is zero, unlike the legacy class form's one-byte prefix of
/// one; the f64 width factor and the remaining metrics have the same roles.
fn decode_indexed_sketch_text_head(
    payload: &[u8],
) -> Result<Option<(SketchTextHead, NonNegativeReal)>, CodecError> {
    (|| {
        let (_, after_tag) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit)?;
        if after_tag != 7
            || View::u32_le_at(payload, after_tag).is_none()
            || payload.get(11..20)?.iter().any(|byte| *byte != 0)
        {
            return None;
        }
        let mut cursor = 20;
        let properties = read_property_block(payload, &mut cursor)?;
        let property = |key: &str| properties.find(key);
        let persistent_id = property("textex_tag")?;
        (payload.get(cursor)? == &0).then_some(())?;
        cursor += 1;
        let width_factor = NonNegativeReal::new(View::f64_le_at(payload, cursor)?)?;
        cursor = cursor.checked_add(8)?;
        let color = read_sketch_text_color(payload, &mut cursor)?;
        let font_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if font_count == 0 || font_count > 1_024 {
            return None;
        }
        let (font_family, after_font) = lp_utf16_bounded(payload, cursor, font_count..=font_count)?;
        cursor = after_font;
        (payload.get(cursor)? == &0).then_some(())?;
        cursor += 1;
        let height = PositiveLength::new(View::f64_le_at(payload, cursor)? * 10.0)?;
        cursor = cursor.checked_add(8)?;
        Some(Ok((
            SketchTextHead {
                entity_genesis: property("EntityGenesis"),
                persistent_id: Some(persistent_id),
                base_id: property("txt_tag_base"),
                font_family,
                height,
                color,
                style: 0,
                cursor,
            },
            width_factor,
        )))
    })()
    .transpose()
}

/// Read the alignment fields, text content, and class tail under one pair of
/// slot forms, requiring the walk to end exactly on the owning-sketch
/// reference.
fn decode_sketch_text_tail(
    payload: &[u8],
    mut cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
    width_factor: NonNegativeReal,
) -> Result<Option<SketchTextTail>, CodecError> {
    (|| {
        let first_reference = read_text_reference(payload, &mut cursor, first_slot)?;
        // Horizontal alignment enum and three flag bytes.
        let horizontal_alignment = View::u32_le_at(payload, cursor)?;
        let horizontal_neutral = payload.get(cursor + 4..cursor + 7)? == [0; 3];
        cursor = cursor.checked_add(7)?;
        let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if text_count == 0 || text_count > 1_048_576 {
            return None;
        }
        let (text, after_text) = lp_utf16_bounded(payload, cursor, text_count..=text_count)?;
        cursor = after_text;
        let second_reference = read_text_reference(payload, &mut cursor, second_slot)?;
        // Vertical alignment enum, one flag byte, and the font weight.
        let vertical_alignment = View::u32_le_at(payload, cursor)?;
        let vertical_neutral = payload.get(cursor + 4)? == &0;
        let font_weight = View::i32_le_at(payload, cursor.checked_add(5)?)?;
        cursor = cursor.checked_add(9)?;
        // The class tail opens with the text-type enum, which gates the placement
        // transform: frame text stores a 4x4 transform, path text stores none. One
        // flag byte follows the enum and repeats it, so a slot form that has
        // desynchronized fails here instead of framing a transform out of whatever
        // bytes the walk landed on.
        let text_type = View::u32_le_at(payload, cursor)?;
        (u32::from(*payload.get(cursor.checked_add(4)?)?) == text_type).then_some(())?;
        cursor = cursor.checked_add(5)?;
        let (placement, mirrored) = match text_type {
            0 => match read_text_placement(payload, &mut cursor)? {
                FrameTextPlacement::Planar(placement) => (Some(placement), false),
                FrameTextPlacement::Mirrored => (None, true),
            },
            1 => (None, false),
            _ => return None,
        };
        cursor = cursor.checked_add(SKETCH_TEXT_TRAILING_RUN)?;
        let owner = take_reference(payload, &mut cursor)?;
        (cursor == payload.len()).then_some(())?;
        Some(Ok(SketchTextTail {
            layout: SketchTextLayout::TextexTag {
                width_factor,
                alignment: Some(SketchTextAlignment {
                    horizontal: horizontal_alignment,
                    vertical: vertical_alignment,
                }),
                first_reference: reference_index(&first_reference),
                second_reference: reference_index(&second_reference),
                placement,
            },
            text,
            font_weight,
            owner_reference: reference_index(&owner)?,
            neutral_orientation: horizontal_neutral && vertical_neutral && !mirrored,
        }))
    })()
    .transpose()
}

/// Read the `txt_tag` form's members from the height to the end of the record.
/// Two bytes separate the height from the anchor coordinates, which this form
/// stores directly rather than in a placement transform. The form writes no
/// parameter-reference slot: an eleven-byte unclassified member run, ten bytes
/// below [`TXT_TAG_ANCHOR_MEMBER_VERSION`], follows the anchor. The text string
/// is followed by a counted reference run, fifteen bytes, and the trailing run
/// and owning-sketch reference that close both forms.
fn decode_txt_tag_sketch_text_tail(
    payload: &[u8],
    mut cursor: usize,
    class_version: u32,
    rotation: Angle,
) -> Result<Option<SketchTextTail>, CodecError> {
    (|| {
        let neutral_orientation = payload.get(cursor..cursor + 2)? == [0; 2];
        cursor = cursor.checked_add(2)?;
        let anchor = Point2::new(
            View::f64_le_at(payload, cursor)? * 10.0,
            View::f64_le_at(payload, cursor.checked_add(8)?)? * 10.0,
        );
        let anchor = FinitePoint2::new(anchor)?;
        let anchor_run = if class_version < TXT_TAG_ANCHOR_MEMBER_VERSION {
            TXT_TAG_ANCHOR_RUN - 1
        } else {
            TXT_TAG_ANCHOR_RUN
        };
        cursor = cursor.checked_add(16 + anchor_run)?;
        let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if text_count == 0 || text_count > 1_048_576 {
            return None;
        }
        let (text, after_text) = lp_utf16_bounded(payload, cursor, text_count..=text_count)?;
        cursor = after_text;
        let references = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if references > MAX_RELATION_RUN {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        for _ in 0..references {
            take_reference(payload, &mut cursor)?;
        }
        let font_weight = View::i32_le_at(payload, cursor.checked_add(TXT_TAG_FONT_WEIGHT_AT)?)?;
        cursor = cursor.checked_add(TXT_TAG_MEMBER_RUN + SKETCH_TEXT_TRAILING_RUN)?;
        let owner = take_reference(payload, &mut cursor)?;
        (cursor == payload.len()).then_some(())?;
        Some(Ok(SketchTextTail {
            layout: SketchTextLayout::TxtTag {
                placement: TextPlacement { anchor, rotation },
            },
            text,
            font_weight,
            owner_reference: reference_index(&owner)?,
            neutral_orientation,
        }))
    })()
    .transpose()
}

/// Read the indexed `textex_tag` tail. The member and alignment fields match
/// the common text body. The indexed class then writes a fixed 35-byte suffix:
/// a text-type u32, three fixed u32 values, a zero u32, a two-byte zero run,
/// two positive f32 scales, and a five-byte zero run before the owning-sketch
/// reference. The text-type values are the same frame (`0`) and path (`1`)
/// discriminators used by the legacy class tail. The indexed suffix carries no
/// neutral anchor or rotation; the source record is retained in full.
fn decode_indexed_sketch_text_tail(
    payload: &[u8],
    mut cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
    width_factor: NonNegativeReal,
) -> Result<Option<SketchTextTail>, CodecError> {
    (|| {
        let first_reference = read_text_reference(payload, &mut cursor, first_slot)?;
        let horizontal_alignment = View::u32_le_at(payload, cursor)?;
        let horizontal_neutral = payload.get(cursor + 4..cursor + 7)? == [0; 3];
        cursor = cursor.checked_add(7)?;
        let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if text_count == 0 || text_count > 1_048_576 {
            return None;
        }
        let (text, after_text) = lp_utf16_bounded(payload, cursor, text_count..=text_count)?;
        cursor = after_text;
        let second_reference = read_text_reference(payload, &mut cursor, second_slot)?;
        let vertical_alignment = View::u32_le_at(payload, cursor)?;
        let vertical_neutral = payload.get(cursor + 4)? == &0;
        let font_weight = View::i32_le_at(payload, cursor.checked_add(5)?)?;
        cursor = cursor.checked_add(9)?;
        if !matches!(View::u32_le_at(payload, cursor)?, 0 | 1)
            || View::u32_le_at(payload, cursor.checked_add(4)?)? != 1
            || View::u32_le_at(payload, cursor.checked_add(8)?)? != 256
            || View::u32_le_at(payload, cursor.checked_add(12)?)? != 0
            || View::u32_le_at(payload, cursor.checked_add(16)?)? != 0
            || payload
                .get(cursor.checked_add(20)?..cursor.checked_add(22)?)?
                .iter()
                .any(|byte| *byte != 0)
        {
            return None;
        }
        let mut scale_view = View::over_retained(payload);
        scale_view.seek(cursor.checked_add(22)?)?;
        let scale_u = scale_view.f32_le()?;
        let scale_v = scale_view.f32_le()?;
        if !scale_u.is_finite() || !scale_v.is_finite() || scale_u <= 0.0 || scale_v <= 0.0 {
            return None;
        }
        (payload
            .get(scale_view.position()..cursor.checked_add(35)?)?
            .iter()
            .all(|byte| *byte == 0))
        .then_some(())?;
        cursor = cursor.checked_add(35)?;
        let owner = take_reference(payload, &mut cursor)?;
        (cursor == payload.len()).then_some(())?;
        Some(Ok(SketchTextTail {
            layout: SketchTextLayout::TextexTag {
                width_factor,
                alignment: Some(SketchTextAlignment {
                    horizontal: horizontal_alignment,
                    vertical: vertical_alignment,
                }),
                first_reference: reference_index(&first_reference),
                second_reference: reference_index(&second_reference),
                placement: None,
            },
            text,
            font_weight,
            owner_reference: reference_index(&owner)?,
            neutral_orientation: horizontal_neutral && vertical_neutral,
        }))
    })()
    .transpose()
}

fn decode_indexed_sketch_text_record_tail(
    payload: &[u8],
    cursor: usize,
    width_factor: NonNegativeReal,
) -> Result<Option<SketchTextTail>, CodecError> {
    let mut closed = None;
    for first_slot in TEXT_REFERENCE_SLOTS {
        for second_slot in TEXT_REFERENCE_SLOTS {
            let Some(tail) = decode_indexed_sketch_text_tail(
                payload,
                cursor,
                first_slot,
                second_slot,
                width_factor,
            )?
            else {
                continue;
            };
            if closed.replace(tail).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(closed)
}

fn decode_legacy_sketch_text_record_tail(
    payload: &[u8],
    cursor: usize,
    identity: SketchTextIdentity,
    class_version: u32,
) -> Result<Option<SketchTextTail>, CodecError> {
    match identity {
        SketchTextIdentity::TxtTag { rotation } => {
            decode_txt_tag_sketch_text_tail(payload, cursor, class_version, rotation)
        }
        SketchTextIdentity::TextexTag { width_factor } => {
            let mut closed = None;
            for first_slot in TEXT_REFERENCE_SLOTS {
                for second_slot in TEXT_REFERENCE_SLOTS {
                    if let Some(tail) = decode_sketch_text_tail(
                        payload,
                        cursor,
                        first_slot,
                        second_slot,
                        width_factor,
                    )? {
                        if closed.replace(tail).is_some() {
                            return Ok(None);
                        }
                    }
                }
            }
            Ok(closed)
        }
    }
}

pub(crate) fn sketch_text_neutral_state(text: &SketchText) -> Option<bool> {
    let payload = text.raw_bytes.as_ref();
    if let Some((head, identity)) = decode_sketch_text_head(payload, text.class_version).ok()? {
        if let Some(tail) = decode_legacy_sketch_text_record_tail(
            payload,
            head.cursor,
            identity,
            text.class_version,
        )
        .ok()?
        {
            return Some(
                head.style & !1 == 0
                    && (head.style & 1 == 0 || text.font_weight == 750)
                    && tail.neutral_orientation,
            );
        }
    }
    let (head, width_factor) = decode_indexed_sketch_text_head(payload).ok()??;
    let tail =
        decode_indexed_sketch_text_record_tail(payload, head.cursor, width_factor).ok()??;
    Some(tail.neutral_orientation)
}

struct SketchTextRecord<'a> {
    stream: &'a str,
    class_tag: crate::records::references::DesignClassTag,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
    head: SketchTextHead,
    tail: SketchTextTail,
}

fn assemble_sketch_text(
    payload: &[u8],
    input: SketchTextRecord<'_>,
) -> Result<SketchText, CodecError> {
    let SketchTextRecord {
        stream,
        class_tag,
        class_version,
        record_index,
        byte_offset,
        head,
        tail,
    } = input;
    let id = design_record_id(
        stream,
        ":sketch-text#",
        u64::try_from(byte_offset)
            .map_err(|_| CodecError::malformed("f3d sketch text offset exceeds u64"))?,
    );
    let raw_bytes = payload.to_vec();
    Ok(SketchText {
        id,
        record_index,
        owner_reference: tail.owner_reference,
        class_tag,
        class_version,
        byte_offset: u64_from_index(byte_offset),
        entity_genesis: head.entity_genesis,
        persistent_id: head.persistent_id,
        base_id: head.base_id,
        text: tail.text,
        font_family: head.font_family,
        font_weight: tail.font_weight,
        height: head.height,
        color: head.color,
        layout: tail.layout,
        raw_bytes: raw_bytes.into(),
    })
}

pub(crate) fn decode_sketch_text_record(
    payload: &[u8],
    stream: &str,
    class_tag: crate::records::references::DesignClassTag,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
) -> Result<Option<SketchText>, CodecError> {
    (|| {
        let legacy_head = match decode_sketch_text_head(payload, class_version) {
            Ok(head) => head,
            Err(error) => return Some(Err(error)),
        };
        if let Some((head, identity)) = legacy_head {
            let tail = match decode_legacy_sketch_text_record_tail(
                payload,
                head.cursor,
                identity,
                class_version,
            ) {
                Ok(tail) => tail,
                Err(error) => return Some(Err(error)),
            };
            if let Some(tail) = tail {
                return Some(assemble_sketch_text(
                    payload,
                    crate::design::decode::sketch::SketchTextRecord {
                        stream,
                        class_tag,
                        class_version,
                        record_index,
                        byte_offset,
                        head,
                        tail,
                    },
                ));
            }
        }
        let (head, width_factor) = match decode_indexed_sketch_text_head(payload) {
            Ok(head) => head?,
            Err(error) => return Some(Err(error)),
        };
        let tail = match decode_indexed_sketch_text_record_tail(payload, head.cursor, width_factor)
        {
            Ok(tail) => tail?,
            Err(error) => return Some(Err(error)),
        };
        Some(assemble_sketch_text(
            payload,
            crate::design::decode::sketch::SketchTextRecord {
                stream,
                class_tag,
                class_version,
                record_index,
                byte_offset,
                head,
                tail,
            },
        ))
    })()
    .transpose()
}

#[derive(Debug)]
struct DecodedSketchPoint {
    owner_reference: Option<u32>,
    coordinate_offset: u32,
    record_form: SketchPointRecordForm,
    paired_reference: u32,
    coordinates: [f64; 2],
}

impl DecodedSketchPoint {
    fn trailing_reference(&self) -> Option<u32> {
        match self.record_form {
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference, ..
            }
            | SketchPointRecordForm::Version11InlineTyped {
                trailing_reference, ..
            } => Some(trailing_reference),
            _ => self.owner_reference,
        }
    }
}

fn take_local_sketch_reference<'a>(
    payload: &'a [u8],
    cursor: &mut usize,
) -> Option<(u32, Option<&'a str>)> {
    let reference = take_reference(payload, cursor)?;
    let (target, inline_type_guid) = reference.into_local()?;
    Some((u32::try_from(target).ok()?, inline_type_guid))
}

fn take_same_segment_sketch_reference(payload: &[u8], cursor: &mut usize) -> Option<u32> {
    let (target, inline_type_guid) = take_local_sketch_reference(payload, cursor)?;
    inline_type_guid.is_none().then_some(target)
}

fn decode_version_zero_sketch_point(
    payload: &[u8],
    header_end: usize,
) -> Option<DecodedSketchPoint> {
    let mut cursor = header_end;
    let prefix_end = cursor.checked_add(10)?;
    if payload.get(cursor..prefix_end) != Some(&[0; 10][..]) {
        return None;
    }
    cursor = prefix_end;
    let paired_reference = take_same_segment_sketch_reference(payload, &mut cursor)?;
    let flag = *payload.get(cursor)?;
    if flag > 1 {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    let coordinate_offset = u32::try_from(cursor).ok()?;
    let x = View::f64_le_at(payload, cursor)?;
    let y = View::f64_le_at(payload, cursor.checked_add(8)?)?;
    cursor = cursor.checked_add(16)?;
    if payload.get(cursor..cursor.checked_add(20)?) != Some(&[0; 20][..])
        || View::f32_le_at(payload, cursor + 20) != Some(1.0)
        || payload.get(cursor + 24..cursor + 36) != Some(&[0; 12][..])
        || View::f32_le_at(payload, cursor + 36) != Some(1.0)
        || View::f32_le_at(payload, cursor + 40) != Some(1.0)
        || payload.get(cursor + 44..cursor + 54) != Some(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0][..])
    {
        return None;
    }
    cursor = cursor.checked_add(54)?;
    if take_same_segment_sketch_reference(payload, &mut cursor)? != paired_reference {
        return None;
    }
    let owner_reference = take_same_segment_sketch_reference(payload, &mut cursor)?;
    if cursor != payload.len() {
        return None;
    }
    Some(DecodedSketchPoint {
        owner_reference: Some(owner_reference),
        coordinate_offset,
        record_form: SketchPointRecordForm::Version0 { flag: flag == 1 },
        paired_reference,
        coordinates: [x, y],
    })
}

/// Whether a Design type is the derived sketch-point type: its primary frame
/// carries the point property block, and its nested base-type frame carries the
/// version-11 point record.
fn is_derived_sketch_point_type(
    design_type: &crate::records::entity_header::SegmentTypeData,
) -> bool {
    design_type
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(DERIVED_SKETCH_POINT_TYPE.0)
        && design_type.version == DERIVED_SKETCH_POINT_TYPE.1
        && design_type.module == DERIVED_SKETCH_POINT_TYPE.2
        && design_type
            .base_type_guid
            .value()
            .is_some_and(|base| base.as_str().eq_ignore_ascii_case(SKETCH_POINT_TYPE_GUID))
}

fn decode_derived_sketch_point_record(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    frame: &super::meta::DesignPrimaryFrame<'_>,
) -> Option<DecodedSketchPoint> {
    let primary = &bytes[frame.start..frame.end];
    let (_, after_class_tag) = lp_ascii_filtered_view(primary, 0, 3..=3, u8::is_ascii_digit)?;
    let mut cursor = after_class_tag.checked_add(4)?;
    if primary.get(cursor..cursor.checked_add(9)?) != Some(&[0; 9][..]) {
        return None;
    }
    cursor = cursor.checked_add(9)?;
    let properties = read_property_block(primary, &mut cursor)?;
    let nested_at = frame.start.checked_add(cursor)?;
    if properties.count == 0
        || nested_at >= frame.end
        || (frame.member_end != frame.end && frame.member_end != nested_at)
    {
        return None;
    }
    let nested_class_tag =
        super::meta::record_header_class_tag(bytes, nested_at, frame.end, frame.entity_id)?;
    let (_, base_type) = super::meta::dynamic_type(meta, &nested_class_tag)?;
    if !base_type
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(SKETCH_POINT_TYPE_GUID)
        || base_type.version != CURRENT_SKETCH_POINT_TYPE.1
        || base_type.module != CURRENT_SKETCH_POINT_TYPE.2
    {
        return None;
    }
    let mut decoded = decode_sketch_point_members(
        &bytes[nested_at..frame.end],
        CURRENT_SKETCH_POINT_TYPE.1,
        Some(properties),
    )?;
    let SketchPointRecordForm::Version11 { derived_type, .. } = &mut decoded.record_form else {
        return None;
    };
    *derived_type = true;
    decoded.coordinate_offset = decoded
        .coordinate_offset
        .checked_add(u32::try_from(cursor).ok()?)?;
    Some(decoded)
}

fn decode_sketch_point_record(payload: &[u8], class_version: u32) -> Option<DecodedSketchPoint> {
    decode_sketch_point_members(payload, class_version, None)
}

/// Decode one sketch-point member sequence. `inherited` is the property block
/// of a derived point's primary frame; the member sequence then stores eight
/// zero bytes and no property block.
fn decode_sketch_point_members(
    payload: &[u8],
    class_version: u32,
    inherited: Option<SketchProperties<'_>>,
) -> Option<DecodedSketchPoint> {
    let (_, after_class_tag) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit)?;
    let header_end = after_class_tag.checked_add(4)?;
    if class_version == 0 {
        return decode_version_zero_sketch_point(payload, header_end);
    }
    let zero_run = if inherited.is_some() { 8 } else { 9 };
    if !matches!(class_version, 8 | 10 | 11)
        || payload.get(header_end..header_end.checked_add(zero_run)?) != Some(&[0; 9][..zero_run])
    {
        return None;
    }
    let mut cursor = header_end.checked_add(zero_run)?;
    let properties = match inherited {
        Some(inherited) => inherited,
        None => read_property_block(payload, &mut cursor)?,
    };
    let (entity_genesis, persistent_id) = match properties.first_two()? {
        (
            SketchProperty {
                name: "pt_tag",
                value: persistent_id,
            },
            None,
        ) => (None, persistent_id),
        (
            SketchProperty {
                name: "EntityGenesis",
                value: entity_genesis,
            },
            Some(SketchProperty {
                name: "pt_tag",
                value: persistent_id,
            }),
        ) if class_version == 11 => (Some(entity_genesis), persistent_id),
        _ => return None,
    };
    let persistent_id = std::num::NonZeroU64::new(persistent_id)?;
    let (paired_reference, paired_type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
    let inline_typed = match (class_version, paired_type_guid) {
        (8 | 10 | 11, None) => false,
        (10 | 11, Some(type_guid))
            if type_guid.eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0) =>
        {
            true
        }
        _ => return None,
    };
    let flag_count = if class_version == 11 { 8 } else { 7 };
    let flags_end = cursor.checked_add(flag_count)?;
    let source_flags = payload.get(cursor..flags_end)?;
    if source_flags.iter().any(|flag| *flag > 1) {
        return None;
    }
    let mut flags = [0; 8];
    flags[..flag_count].copy_from_slice(source_flags);
    cursor = flags_end;
    let coordinate_offset = u32::try_from(cursor).ok()?;
    let coordinates = [
        View::f64_le_at(payload, cursor)?,
        View::f64_le_at(payload, cursor.checked_add(8)?)?,
    ];
    let depth = View::f64_le_at(payload, cursor.checked_add(16)?)? * 10.0;
    cursor = cursor.checked_add(24)?;
    let selector = View::u64_le_at(payload, cursor)?;
    let state = *payload.get(cursor.checked_add(8)?)?;
    let reserved_zero_count = if class_version == 8 { 8 } else { 12 };
    let floats_at = cursor.checked_add(9 + reserved_zero_count)?;
    if payload
        .get(cursor + 9..floats_at)?
        .iter()
        .any(|byte| *byte != 0)
        || View::f32_le_at(payload, floats_at) != Some(1.0)
        || View::f32_le_at(payload, floats_at + 4) != Some(1.0)
        || payload.get(floats_at + 8..floats_at + 13) != Some(&[0, 1, 0, 0, 0][..])
    {
        return None;
    }
    cursor = floats_at.checked_add(13)?;
    let (repeated_reference, repeated_type_guid) =
        take_local_sketch_reference(payload, &mut cursor)?;
    let repeated_encoding_matches = match repeated_type_guid {
        None => !inline_typed,
        Some(type_guid) => {
            inline_typed && type_guid.eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0)
        }
    };
    if repeated_reference != paired_reference || !repeated_encoding_matches {
        return None;
    }
    let closure = SketchPointClosure::from_pair(selector, state)?;
    let mut seven = [0; 7];
    seven.copy_from_slice(&flags[..7]);
    let (record_form, owner_reference) = match (class_version, inline_typed) {
        (8, false) => {
            if closure != SketchPointClosure::Selector0State0 {
                return None;
            }
            let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
            (
                SketchPointRecordForm::Version8 {
                    depth,
                    persistent_id,
                    flags: seven.map(|flag| flag == 1),
                },
                Some(owner),
            )
        }
        (10, false) => {
            let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
            (
                SketchPointRecordForm::Version10 {
                    depth,
                    persistent_id,
                    flags: seven.map(|flag| flag == 1),
                    closure: crate::records::sketch_geometry::SketchPointClosure10::from_closure(
                        closure,
                    )?,
                },
                Some(owner),
            )
        }
        (10, true) => {
            let (trailing_reference, type_guid) =
                take_local_sketch_reference(payload, &mut cursor)?;
            if type_guid
                .is_none_or(|type_guid| !type_guid.eq_ignore_ascii_case(SKETCH_CONTAINER_TYPE_GUID))
            {
                return None;
            }
            (
                SketchPointRecordForm::Version10InlineTyped {
                    depth,
                    trailing_reference,
                    persistent_id,
                    flags: seven.map(|flag| flag == 1),
                    closure:
                        crate::records::sketch_geometry::SketchPointClosure10Inline::from_closure(
                            closure,
                        )?,
                },
                None,
            )
        }
        (11, true) => {
            let (trailing_reference, type_guid) =
                take_local_sketch_reference(payload, &mut cursor)?;
            if type_guid
                .is_none_or(|type_guid| !type_guid.eq_ignore_ascii_case(SKETCH_CONTAINER_TYPE_GUID))
            {
                return None;
            }
            (
                SketchPointRecordForm::Version11InlineTyped {
                    depth,
                    entity_genesis,
                    trailing_reference,
                    companion_prefix_present_zero: false,
                    persistent_id,
                    flags: flags.map(|flag| flag == 1),
                    closure,
                },
                None,
            )
        }
        (11, false) => {
            let padded_paired_reference = match payload.len().checked_sub(cursor)? {
                11 => false,
                15 if payload.get(cursor..cursor + 4) == Some(&[0; 4][..]) => {
                    cursor += 4;
                    true
                }
                _ => return None,
            };
            let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
            (
                SketchPointRecordForm::Version11 {
                    depth,
                    entity_genesis,
                    padded_paired_reference,
                    derived_type: false,
                    companion_prefix_present_zero: false,
                    persistent_id,
                    flags: flags.map(|flag| flag == 1),
                    closure,
                },
                Some(owner),
            )
        }
        _ => return None,
    };
    if cursor != payload.len() {
        return None;
    }
    Some(DecodedSketchPoint {
        owner_reference,
        coordinate_offset,
        record_form,
        paired_reference,
        coordinates,
    })
}

fn point_target_has_guid(
    types_by_entity: &HashMap<u32, (&str, u32, &str)>,
    target: Option<u32>,
    expected_guid: &str,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    types_by_entity
        .get(&target)
        .is_some_and(|(type_guid, _, _)| type_guid.eq_ignore_ascii_case(expected_guid))
}

fn decode_sketch_point_companion(
    payload: &[u8],
    point_record_index: u32,
    record_form: SketchPointRecordForm,
    types_by_entity: &HashMap<u32, (&str, u32, &str)>,
) -> Option<(SketchPointRecordForm, SketchPointCompanion)> {
    let parsed = (|| {
        let reference_encoding = SketchPointCompanionReferenceEncoding::for_form(&record_form);
        let (prefix_present_zero, mut cursor) = if payload.get(11..21) == Some(&[0; 10][..]) {
            (false, 21)
        } else if payload.get(11..20) == Some(&[0; 9][..])
            && payload.get(20..25) == Some(&[1, 0, 0, 0, 0][..])
        {
            (true, 25)
        } else {
            return None;
        };
        let record_form = record_form.with_companion_prefix_present_zero(prefix_present_zero)?;
        let count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if count > MAX_RELATION_RUN {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        Some((reference_encoding, record_form, cursor, count))
    })();
    let (reference_encoding, record_form, mut cursor, count) = parsed?;

    let mut incident_curves = Vec::new();
    let parsed = (|| {
        for _ in 0..count {
            let (target, type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
            let registered_type = types_by_entity.get(&target)?;
            match reference_encoding {
                SketchPointCompanionReferenceEncoding::SameSegment if type_guid.is_none() => {}
                SketchPointCompanionReferenceEncoding::InlineTyped
                    if type_guid.is_some_and(|type_guid| {
                        type_guid.eq_ignore_ascii_case(registered_type.0)
                    }) => {}
                _ => return None,
            }
            incident_curves.push(target);
        }
        if payload.get(cursor) != Some(&0) {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        let (inverse, inverse_type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
        let inverse_encoding_matches = match reference_encoding {
            SketchPointCompanionReferenceEncoding::SameSegment => inverse_type_guid.is_none(),
            SketchPointCompanionReferenceEncoding::InlineTyped => inverse_type_guid
                .is_some_and(|type_guid| type_guid.eq_ignore_ascii_case(SKETCH_POINT_TYPE_GUID)),
        };
        if inverse != point_record_index || !inverse_encoding_matches || cursor != payload.len() {
            return None;
        }
        Some((record_form, SketchPointCompanion { incident_curves }))
    })();
    parsed
}

const SKETCH_POINT_TYPE_GUID: &str = "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB";
pub(crate) const CURRENT_SKETCH_POINT_TYPE: (&str, u32, &str) =
    (SKETCH_POINT_TYPE_GUID, 11, "Geometry");
const DERIVED_SKETCH_POINT_TYPE: (&str, u32, &str) =
    ("00D157C2-2332-4BD5-BB53-048C4081D1C3", 0, "MSketch");
pub(crate) const SKETCH_POINT_COMPANION_TYPE: (&str, u32, &str) =
    ("362B7EC3-0F09-47C8-A3BE-DC066715CDAE", 0, "Geometry");
pub(crate) const SKETCH_CONTAINER_TYPE_GUID: &str = "44A64366-4BD3-4B24-881A-F94C206E8F2D";
pub(crate) const CURRENT_SKETCH_LINE_TYPE: (&str, u32, &str) =
    ("DCA267ED-D615-4934-B64F-AD805E8003E2", 2, "Geometry");
pub(crate) const CURRENT_SKETCH_CIRCULAR_TYPE: (&str, u32, &str) =
    ("F0130424-8B7E-4092-93C9-1CA807482534", 0, "Geometry");
pub(crate) const CURRENT_SKETCH_NURBS_TYPE: (&str, u32, &str) =
    ("D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B", 3, "MSketch");

const SKETCH_LINE_TYPES: [(&str, u32, &str); 6] = [
    CURRENT_SKETCH_LINE_TYPE,
    ("DCA267ED-D615-4934-B64F-AD805E8003E2", 1, "Geometry"),
    ("EA3B930A-3383-4AD3-BE25-4B2814EA3985", 0, "Geometry"),
    ("AE42BAB6-643F-4169-A33C-529C8E0A4D84", 0, "Geometry"),
    ("F279874A-17AB-43DA-BF8E-80259802D06E", 0, "Geometry"),
    ("58751243-FEA8-41E6-BBC9-37960EB8164B", 0, "Geometry"),
];
const SKETCH_CIRCULAR_TYPES: [(&str, u32, &str); 2] = [
    CURRENT_SKETCH_CIRCULAR_TYPE,
    ("FF23079A-D99C-47AB-940E-2F4E18F022AB", 2, "Geometry"),
];
const SKETCH_TEXT_FRAME_LINE_TYPE_GUID: &str = "16DEFC4D-1816-4FB0-8E39-9BDA23954248";

/// Geometry grammar selected by a sketch curve's stable type identity and
/// record version. Dynamic class tags are segment-local table ordinals and do
/// not identify a family without this resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SketchCurveClass {
    Line,
    Circular,
    Nurbs,
    TextFrameLine,
}

impl SketchCurveClass {
    fn of(type_guid: &str, version: u32, module: &str) -> Option<Self> {
        let matches = |(known_guid, known_version, known_module): &(&str, u32, &str)| {
            version == *known_version
                && module == *known_module
                && type_guid.eq_ignore_ascii_case(known_guid)
        };
        if SKETCH_LINE_TYPES.iter().any(matches) {
            Some(Self::Line)
        } else if SKETCH_CIRCULAR_TYPES.iter().any(matches) {
            Some(Self::Circular)
        } else if type_guid.eq_ignore_ascii_case(CURRENT_SKETCH_NURBS_TYPE.0)
            && version == CURRENT_SKETCH_NURBS_TYPE.1
            && module == CURRENT_SKETCH_NURBS_TYPE.2
        {
            Some(Self::Nurbs)
        } else if type_guid.eq_ignore_ascii_case(SKETCH_TEXT_FRAME_LINE_TYPE_GUID)
            && version == 0
            && module == "MSketch"
        {
            Some(Self::TextFrameLine)
        } else {
            None
        }
    }
}

/// Decode every sketch-curve record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
fn decode_sketch_curve_identities_from_stream(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    for frame in design_primary_frames(bytes, meta)? {
        let payload = &bytes[frame.start..frame.end];
        let Some((primary_id, secondary_id, geometry_shift, entity_genesis)) =
            decode_sketch_curve_identity(payload)
        else {
            continue;
        };
        let Some(primary_id) = std::num::NonZeroU64::new(primary_id) else {
            continue;
        };
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-curve entity ID exceeds u32".into()))?;
        let curve_class = SketchCurveClass::of(
            frame.design_type.type_guid.as_str(),
            frame.design_type.version,
            &frame.design_type.module,
        );
        let parsed_geometry = if let Some(curve_class) = curve_class {
            decode_sketch_curve_geometry(
                payload,
                geometry_shift,
                record_index,
                curve_class,
                frame.start,
            )?
        } else {
            None
        };
        let (geometry, geometry_offset, planar_z_noise) =
            parsed_geometry.map_or((None, geometry_shift + 133, None), |parsed| {
                (
                    Some(parsed.geometry),
                    parsed.geometry_offset,
                    parsed.planar_z_noise,
                )
            });

        out.push(SketchCurveIdentity {
            id: design_record_id(
                stream,
                ":sketch-curve-identity#",
                u64_from_index(frame.start),
            ),
            record_index,
            owner_reference: trailing_sketch_owner_reference(payload),
            class_tag: frame.class_tag,
            byte_offset: u64_from_index(frame.start),
            geometry_offset: u32::try_from(geometry_offset).map_err(|_| {
                CodecError::malformed("f3d sketch curve geometry offset exceeds u32")
            })?,
            entity_genesis,
            primary_id,
            secondary_id,
            geometry,
            planar_z_noise,
        });
    }
    Ok(out)
}

/// Decode every sketch-curve record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
pub(crate) fn decode_sketch_curve_identities(
    scan: &ContainerScan,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    decode_sketch_streams(scan, decode_sketch_curve_identities_from_stream)
}

#[derive(Debug)]
struct ParsedSketchSurface {
    entity_genesis: Option<u64>,
    persistent_id: std::num::NonZeroU64,
    geometry: SketchSurfaceGeometry,
}

struct SketchSurfaceFrame {
    entity_genesis: Option<u64>,
    persistent_id: std::num::NonZeroU64,
    u_degree: u32,
    v_degree: u32,
    u_knots_at: usize,
    u_knot_count: usize,
    v_knots_at: usize,
    v_knot_count: usize,
    coordinate_count: usize,
    v_count: usize,
}

fn parse_sketch_surface_frame(payload: &[u8]) -> Option<SketchSurfaceFrame> {
    if payload.get(20) != Some(&1)
        || View::u32_le_at(payload, 21) != Some(2)
        || View::u32_le_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || View::u32_le_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
        || View::u32_le_at(payload, 77) != Some(11)
        || payload.get(81..92) != Some(b"surface_tag")
        || View::u32_le_at(payload, 92) != Some(23)
        || payload.get(96..119) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = View::u64_le_at(payload, 69);
    let persistent_id = std::num::NonZeroU64::new(View::u64_le_at(payload, 119)?)?;
    let point_count = usize::try_from(View::u32_le_at(payload, 127)?).ok()?;
    if point_count == 0 || point_count > 100_000 {
        return None;
    }
    let coordinate_count = point_count.checked_mul(3)?;
    let coordinate_bytes = point_count.checked_mul(24)?;
    payload.get(131..131usize.checked_add(coordinate_bytes)?)?;
    let degrees_at = 131usize.checked_add(coordinate_bytes)?;
    let u_degree = View::u32_le_at(payload, degrees_at)?;
    let v_degree = View::u32_le_at(payload, degrees_at.checked_add(4)?)?;
    let u_knot_count =
        usize::try_from(View::u32_le_at(payload, degrees_at.checked_add(8)?)?).ok()?;
    let u_knots_at = degrees_at.checked_add(12)?;
    payload.get(u_knots_at..u_knots_at.checked_add(u_knot_count.checked_mul(8)?)?)?;
    let v_count_at = u_knots_at.checked_add(u_knot_count.checked_mul(8)?)?;
    let v_knot_count = usize::try_from(View::u32_le_at(payload, v_count_at)?).ok()?;
    let v_knots_at = v_count_at.checked_add(4)?;
    payload.get(v_knots_at..v_knots_at.checked_add(v_knot_count.checked_mul(8)?)?)?;
    let grid_at = v_knots_at.checked_add(v_knot_count.checked_mul(8)?)?;
    let u_count = usize::try_from(View::u32_le_at(payload, grid_at)?).ok()?;
    let v_count = usize::try_from(View::u32_le_at(payload, grid_at.checked_add(4)?)?).ok()?;
    // The grid count determines safe row framing. Geometry admission follows
    // after source coordinates are scaled to millimetres.
    if u_count.checked_mul(v_count) != Some(point_count) {
        return None;
    }
    Some(SketchSurfaceFrame {
        entity_genesis,
        persistent_id,
        u_degree,
        v_degree,
        u_knots_at,
        u_knot_count,
        v_knots_at,
        v_knot_count,
        coordinate_count,
        v_count,
    })
}

fn sketch_scalar_values(payload: &[u8], offset: usize, count: usize) -> Option<Vec<f64>> {
    let end = offset.checked_add(count.checked_mul(8)?)?;
    payload.get(offset..end)?;
    (0..count)
        .map(|index| View::f64_le_at(payload, offset + index * 8))
        .collect()
}

fn parse_sketch_surface(
    payload: &[u8],
    record_at: usize,
) -> Result<Option<ParsedSketchSurface>, CodecError> {
    let Some(frame) = parse_sketch_surface_frame(payload) else {
        return Ok(None);
    };
    let Some(coordinates) = sketch_scalar_values(payload, 131, frame.coordinate_count) else {
        return Ok(None);
    };
    let Some(u_knots) = sketch_scalar_values(payload, frame.u_knots_at, frame.u_knot_count) else {
        return Ok(None);
    };
    let Some(v_knots) = sketch_scalar_values(payload, frame.v_knots_at, frame.v_knot_count) else {
        return Ok(None);
    };

    let mut points = Vec::new();
    for (ordinal, values) in coordinates.chunks_exact(3).enumerate() {
        let Some(source) = FinitePoint3::new(Point3::new(values[0], values[1], values[2])) else {
            return Ok(None);
        };
        let point = scaled_sketch_point(source).ok_or_else(|| {
            crate::design::text::malformed_design(format_args!(
                "F3D sketch surface at byte {record_at} control point {ordinal} overflows millimetres"
            ))
        })?;
        points.push(point);
    }

    let mut control_points = Vec::new();
    for row in points.chunks(frame.v_count) {
        let mut row_points = Vec::new();
        row_points.extend_from_slice(row);
        control_points.push(row_points);
    }
    let Ok(geometry) = SketchSurfaceGeometry::from_checked_parts(
        frame.u_degree,
        frame.v_degree,
        u_knots,
        v_knots,
        control_points,
    ) else {
        return Ok(None);
    };
    Ok(Some(ParsedSketchSurface {
        entity_genesis: frame.entity_genesis,
        persistent_id: frame.persistent_id,
        geometry,
    }))
}

/// Decode tensor-product surface entities owned by spatial Design sketches.
pub(crate) fn decode_sketch_surfaces(
    scan: &ContainerScan,
) -> Result<Vec<SketchSurface>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while let Some(record_at) = next_indexed_record_offset(bytes, at) {
            at = record_at + 1;
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered_view(bytes, record_at, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            let Some(record_index) = View::u32_le_at(bytes, after_tag) else {
                continue;
            };
            let payload = &bytes[record_at..];
            let Some(surface) = parse_sketch_surface(payload, record_at)? else {
                continue;
            };

            out.push(SketchSurface {
                id: ids::native_sketch_surface_id(&entry.name, record_at),
                record_index,
                owner_reference: None,
                class_tag: crate::design::decode::text::class_tag_from_view(class_tag)
                    .map_err(CodecError::Malformed)?,
                byte_offset: u64_from_index(record_at),
                entity_genesis: surface.entity_genesis,
                persistent_id: surface.persistent_id,
                geometry: surface.geometry,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Bind relation-connected sketch geometry to its unique owning sketch.
pub(crate) fn bind_sketch_graph(
    entities: &[DesignEntityHeader],
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
    surfaces: &mut [SketchSurface],
    relations: &mut [SketchRelation],
) -> Result<(), CodecError> {
    let mut sketch_owners = HashMap::new();
    for entity in entities.iter().filter(|entity| entity.in_sketch_module()) {
        let (Some(scope), Ok(suffix)) = (
            native_stream(&entity.id),
            u32::try_from(entity.entity_id.suffix()),
        ) else {
            continue;
        };
        sketch_owners.insert((scope, suffix), entity.entity_id.as_str());
    }

    let mut scoped_relations = Vec::new();
    for relation in relations.iter_mut() {
        let scope = native_stream(&relation.id).ok_or_else(|| {
            crate::design::text::malformed_design(format_args!(
                "Fusion sketch relation {} has no Design stream identity",
                relation.record_index
            ))
        })?;
        let owner = sketch_owners
            .get(&(scope, relation.owner_reference))
            .ok_or_else(|| {
                crate::design::text::malformed_design(format_args!(
                    "Fusion sketch relation {} in {scope} has no owning Design entity {}",
                    relation.record_index, relation.owner_reference,
                ))
            })?;

        relation.owner_entity_id = Some(
            cadmpeg_core::text::NonBlankString::new((*owner).to_owned()).ok_or_else(|| {
                crate::design::text::malformed_design(format_args!(
                    "Fusion sketch relation {} has an empty owner_entity_id",
                    relation.record_index,
                ))
            })?,
        );
        scoped_relations.push((
            scope,
            relation.owner_reference,
            relation.members(),
            relation.return_members(),
        ));
    }
    let typed_record_keys = points
        .iter()
        .filter_map(|point| Some((native_stream(&point.id)?, point.record_index)))
        .chain(
            curves
                .iter()
                .filter_map(|curve| Some((native_stream(&curve.id)?, curve.record_index))),
        )
        .chain(
            surfaces
                .iter()
                .filter_map(|surface| Some((native_stream(&surface.id)?, surface.record_index))),
        );
    let mut typed_records = std::collections::HashSet::new();
    for key in typed_record_keys {
        if !typed_records.contains(&key) {
            typed_records.insert(key);
        }
    }
    let mut owners = std::collections::HashMap::new();
    let direct_owners = points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            curves
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            surfaces
                .iter()
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
        .filter_map(|(id, record_index, owner_reference)| {
            Some((native_stream(id)?, record_index, owner_reference?))
        });
    for (scope, record_index, owner_reference) in direct_owners {
        if let Some((owner_scope, _)) = sketch_owners
            .keys()
            .find(|(owner_scope, owner)| *owner_scope == scope && *owner == owner_reference)
        {
            owners.insert((*owner_scope, record_index), owner_reference);
        }
    }
    for &(scope, owner_reference, members, returned) in &scoped_relations {
        for record_index in members
            .iter()
            .map(|member| member.reference.record_index())
            .chain(
                returned
                    .iter()
                    .map(|member| member.reference.record_index()),
            )
        {
            if !typed_records.contains(&(scope, record_index)) {
                continue;
            }
            if owners
                .insert((scope, record_index), owner_reference)
                .is_some_and(|owner| owner != owner_reference)
            {
                return Err(crate::design::text::malformed_design(format_args!(
                    "Fusion sketch record {record_index} in {scope} belongs to multiple sketches"
                )));
            }
        }
    }
    // A direct backlink, a typed relation, and the sketch container's counted
    // member run are the three independent ownership joins. Payload-internal
    // references in an otherwise unowned Geometry record do not name a Sketch.
    // Apply the container join after relations and require all joins to agree.
    for entity in entities.iter().filter(|entity| entity.in_sketch_module()) {
        let (Some(scope), Ok(suffix)) = (
            native_stream(&entity.id),
            u32::try_from(entity.entity_id.suffix()),
        ) else {
            continue;
        };
        for record_index in entity.member_values() {
            if !typed_records.contains(&(scope, *record_index)) {
                continue;
            }
            if owners
                .insert((scope, *record_index), suffix)
                .is_some_and(|owner| owner != suffix)
            {
                return Err(crate::design::text::malformed_design(format_args!(
                    "Fusion sketch record {record_index} in {scope} belongs to multiple sketches"
                )));
            }
        }
    }
    for point in points.iter_mut() {
        point.owner_reference = native_stream(&point.id)
            .and_then(|scope| owners.get(&(scope, point.record_index)))
            .copied();
    }
    for curve in curves.iter_mut() {
        curve.owner_reference = native_stream(&curve.id)
            .and_then(|scope| owners.get(&(scope, curve.record_index)))
            .copied();
    }
    for surface in surfaces.iter_mut() {
        surface.owner_reference = native_stream(&surface.id)
            .and_then(|scope| owners.get(&(scope, surface.record_index)))
            .copied();
    }
    let operand_entries = points
        .iter()
        .filter_map(|point| {
            Some((
                (native_stream(&point.id)?, point.record_index),
                SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id(),
                },
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?, curve.record_index),
                SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                },
            ))
        }))
        .chain(surfaces.iter().filter_map(|surface| {
            Some((
                (native_stream(&surface.id)?, surface.record_index),
                SketchRelationOperand::Surface {
                    record_index: surface.record_index,
                    persistent_id: surface.persistent_id.get(),
                },
            ))
        }));
    let mut operands = HashMap::new();
    for (key, operand) in operand_entries {
        operands.insert(key, operand);
    }
    for relation in relations {
        let scope = native_stream(&relation.id).ok_or_else(|| {
            crate::design::text::malformed_design(format_args!(
                "invalid sketch relation id {}",
                relation.id
            ))
        })?;
        let owned_scope = scope.to_owned();
        let resolve = |record_index| {
            operands
                .get(&(owned_scope.as_str(), record_index))
                .cloned()
                .unwrap_or(SketchRelationOperand::Record { record_index })
        };
        relation.resolve_members(resolve);
    }
    Ok(())
}

fn decode_sketch_curve_identity(payload: &[u8]) -> Option<(u64, u64, usize, Option<u64>)> {
    if let Some((primary, secondary)) = decode_sketch_curve_identity_variant(payload, 0, 2) {
        return Some((primary, secondary, 0, None));
    }
    if View::u32_le_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || View::u32_le_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = View::u64_le_at(payload, 69)?;
    decode_sketch_curve_identity_variant(payload, 52, 3)
        .map(|(primary, secondary)| (primary, secondary, 52, Some(entity_genesis)))
}

fn decode_sketch_curve_identity_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u64)> {
    if payload.get(20) != Some(&1)
        || View::u32_le_at(payload, 21) != Some(property_count)
        || View::u32_le_at(payload, 25 + shift) != Some(14)
        || payload.get(29 + shift..43 + shift) != Some(b"crv_primary_id")
        || View::u32_le_at(payload, 43 + shift) != Some(23)
        || payload.get(47 + shift..70 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || View::u32_le_at(payload, 78 + shift) != Some(16)
        || payload.get(82 + shift..98 + shift) != Some(b"crv_secondary_id")
        || View::u32_le_at(payload, 98 + shift) != Some(23)
        || payload.get(102 + shift..125 + shift) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    Some((
        View::u64_le_at(payload, 70 + shift)?,
        View::u64_le_at(payload, 125 + shift)?,
    ))
}

struct DecodedSketchCurveGeometry {
    geometry: SketchCurveGeometry,
    geometry_offset: usize,
    planar_z_noise: Option<[f64; 3]>,
}

fn decode_sketch_curve_geometry(
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    class: SketchCurveClass,
    record_at: usize,
) -> Result<Option<DecodedSketchCurveGeometry>, CodecError> {
    let Some(geometry_payload) = payload.get(geometry_shift..) else {
        return Ok(None);
    };
    let mut planar_z_noise = None;
    let decoded = match class {
        SketchCurveClass::Line => {
            if let Some((geometry, form)) = decode_line_family(geometry_payload, record_at)? {
                if form == COMPACT_PLANAR_LINE_VALUES {
                    planar_z_noise = compact_planar_line_z_noise(geometry_payload);
                }
                Some((geometry, 133))
            } else if let Some(referenced) = referenced_analytic_payload(geometry_payload) {
                let decoded = decode_line_family(referenced, record_at)?;
                if decoded
                    .as_ref()
                    .is_some_and(|(_, form)| *form == COMPACT_PLANAR_LINE_VALUES)
                {
                    planar_z_noise = compact_planar_line_z_noise(referenced);
                }
                decoded.map(|(geometry, _)| (geometry, 11 + 133))
            } else {
                None
            }
        }
        SketchCurveClass::Circular => {
            if let Some(geometry) = decode_circular_arc(geometry_payload, record_at)? {
                Some((geometry, 133))
            } else if let Some(referenced) = referenced_analytic_payload(geometry_payload) {
                decode_circular_arc(referenced, record_at)?.map(|geometry| (geometry, 11 + 133))
            } else {
                None
            }
        }
        SketchCurveClass::Nurbs => {
            let legacy = decode_legacy_sketch_nurbs(geometry_payload, record_at).transpose()?;
            let geometry = match legacy {
                Some(geometry) => Some(geometry),
                None => decode_sketch_nurbs(geometry_payload, record_at).transpose()?,
            };
            geometry.map(|(geometry, _)| (geometry, 133))
        }
        SketchCurveClass::TextFrameLine => {
            decode_text_frame_line(payload, geometry_shift, record_index, record_at)?.and_then(
                |(geometry, end)| {
                    end.checked_sub(geometry_shift + 12 * 8)
                        .map(|offset| (geometry, offset))
                },
            )
        }
    };
    Ok(
        decoded.map(|(geometry, offset)| DecodedSketchCurveGeometry {
            geometry,
            geometry_offset: geometry_shift + offset,
            planar_z_noise,
        }),
    )
}

fn decode_circular_arc(
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let mut values = [0.0; 12];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, 133 + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    let (Some(center_cm), Some(normal), Some(reference_direction), Some(radius_cm)) = (
        FinitePoint3::new(Point3::new(values[0], values[1], values[2])),
        UnitVector3::new(Vector3::new(values[3], values[4], values[5])),
        UnitVector3::new(Vector3::new(values[6], values[7], values[8])),
        PositiveLength::new(values[9]),
    ) else {
        return Ok(None);
    };
    let (Some(start_angle), Some(end_angle)) = (Angle::new(values[10]), Angle::new(values[11]))
    else {
        return Ok(None);
    };
    let center = scale_sketch_point(center_cm, record_at, "arc")?;
    let radius = PositiveLength::new(radius_cm.get() * 10.0);
    let Some(radius) = radius else {
        return Err(crate::design::text::malformed_design(format_args!(
            "F3D sketch arc at byte {record_at} overflows millimetres"
        )));
    };
    Ok(SketchCurveGeometry::arc_from_parts(
        center,
        normal,
        reference_direction,
        radius,
        start_angle,
        end_angle,
    )
    .ok())
}

fn scale_sketch_point(
    point_centimetres: FinitePoint3,
    record_at: usize,
    kind: &str,
) -> Result<FinitePoint3, CodecError> {
    scaled_sketch_point(point_centimetres).ok_or_else(|| {
        crate::design::text::malformed_design(format_args!(
            "F3D sketch {kind} at byte {record_at} overflows millimetres"
        ))
    })
}

fn scaled_sketch_point(point_centimetres: FinitePoint3) -> Option<FinitePoint3> {
    let point = point_centimetres.get();
    FinitePoint3::new(Point3::new(point.x * 10.0, point.y * 10.0, point.z * 10.0))
}

fn referenced_analytic_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.get(133) != Some(&1) || payload.get(138..144) != Some(&[0; 6]) {
        return None;
    }
    payload.get(11..)
}

/// Decode a text-frame boundary line after its two point references and
/// inline analytic-curve record. The first point reference has a trailing
/// null-role byte in addition to its six-byte reference padding. The inline
/// record repeats the enclosing record index and carries eight zero bytes
/// before the line values.
fn decode_text_frame_line(
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    record_at: usize,
) -> Result<Option<(SketchCurveGeometry, usize)>, CodecError> {
    let values_at = (|| {
        let mut cursor = geometry_shift.checked_add(133)?;
        for zero_count in [7, 6] {
            let (_, end) = marked_u32(payload, cursor)?;
            if !payload
                .get(end..end + zero_count)?
                .iter()
                .all(|byte| *byte == 0)
            {
                return None;
            }
            cursor = end + zero_count;
        }
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(payload, cursor, 0..=2000, u8::is_ascii_graphic)?;
        if class_tag.len() != 3
            || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || View::u32_le_at(payload, after_tag) != Some(record_index)
            || payload.get(after_tag + 4..after_tag + 12) != Some(&[0; 8])
        {
            return None;
        }
        after_tag.checked_add(12)
    })();
    let Some(values_at) = values_at else {
        return Ok(None);
    };
    let Some(end) = values_at.checked_add(12 * 8) else {
        return Ok(None);
    };
    Ok(decode_line_values(payload, values_at, record_at)?.map(|geometry| (geometry, end)))
}

fn decode_sketch_nurbs(
    payload: &[u8],
    record_at: usize,
) -> Option<Result<(SketchCurveGeometry, usize), CodecError>> {
    let base = 133usize;
    let carrier = View::u64_le_at(payload, base)?;
    let carrier_reference = (carrier != u64::MAX).then_some(carrier);
    if View::u32_le_at(payload, base + 8) != Some(3) || payload.get(base + 88) != Some(&1) {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    let subtype_class_tag =
        crate::records::references::DesignClassTag::try_from(subtype_class_tag).ok()?;
    let degree = View::u32_le_at(payload, base + 90)?;
    let fit_tolerance = View::f64_le_at(payload, base + 94)?;
    let knot_count = usize::try_from(View::u32_le_at(payload, base + 102)?).ok()?;
    if usize::try_from(View::u32_le_at(payload, base + 106)?).ok()? != knot_count
        || View::u32_le_at(payload, base + 110)? != 8
        || knot_count > 100_000
    {
        return None;
    }
    let knots = sketch_scalar_values(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(View::u32_le_at(payload, weights_at)?).ok()?;
    if usize::try_from(View::u32_le_at(payload, weights_at + 4)?).ok()? != weight_count
        || View::u32_le_at(payload, weights_at + 8)? != 8
        || weight_count > 100_000
    {
        return None;
    }
    let weights = sketch_scalar_values(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(View::u32_le_at(payload, points_at)?).ok()?;
    if usize::try_from(View::u32_le_at(payload, points_at + 4)?).ok()? != point_count
        || View::u32_le_at(payload, points_at + 8)? != 8
        || knot_count
            != point_count
                .checked_add(usize::try_from(degree).ok()?)?
                .checked_add(1)?
    {
        return None;
    }
    let coordinates = sketch_scalar_values(payload, points_at + 12, point_count.checked_mul(3)?)?;
    let geometry = match admit_source_sketch_nurbs(
        degree,
        fit_tolerance,
        knots,
        weights,
        &coordinates,
        record_at,
    ) {
        Ok(Some(geometry)) => geometry,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok((
        SketchCurveGeometry::nurbs_from_parts(
            carrier_reference,
            subtype_class_tag,
            View::u32_le_at(payload, base + 15)?,
            geometry,
        ),
        points_at + 12 + point_count * 24,
    )))
}

fn decode_legacy_sketch_nurbs(
    payload: &[u8],
    record_at: usize,
) -> Option<Result<(SketchCurveGeometry, usize), CodecError>> {
    let base = 133usize;
    let carrier = View::u64_le_at(payload, base)?;
    let carrier_reference = (carrier != u64::MAX).then_some(carrier);
    if View::u32_le_at(payload, base + 8) != Some(3)
        || payload.get(base + 19..base + 27) != Some(&[0; 8])
        || payload.get(base + 27) != Some(&1)
        || payload.get(base + 32..base + 42) != Some(&[0; 10])
        || payload.get(base + 50..base + 55) != Some(&[0; 5])
        || payload.get(base + 55) != Some(&1)
        || payload.get(base + 60..base + 66) != Some(&[0; 6])
        || payload.get(base + 66) != Some(&1)
        || payload.get(base + 71..base + 77) != Some(&[0; 6])
        || payload.get(base + 77) != Some(&1)
        || payload.get(base + 80..base + 88) != Some(&[0; 8])
        || payload.get(base + 88).is_none_or(|value| *value > 1)
        || payload.get(base + 89).is_none_or(|value| *value > 1)
        || payload.get(base + 94..base + 102)
            != Some(&[0x95, 0xd6, 0x26, 0xe8, 0x0b, 0x2e, 0x11, 0x3e])
    {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    let subtype_class_tag =
        crate::records::references::DesignClassTag::try_from(subtype_class_tag).ok()?;
    let degree = View::u32_le_at(payload, base + 90)?;
    let fit_tolerance = View::f64_le_at(payload, base + 42)?;
    let knot_count = usize::try_from(View::u32_le_at(payload, base + 102)?).ok()?;
    let knot_capacity = usize::try_from(View::u32_le_at(payload, base + 106)?).ok()?;
    if degree == 0
        || knot_capacity < knot_count
        || View::u32_le_at(payload, base + 110)? != 8
        || knot_capacity > 100_000
    {
        return None;
    }
    let knots = sketch_scalar_values(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(View::u32_le_at(payload, weights_at)?).ok()?;
    let weight_capacity = usize::try_from(View::u32_le_at(payload, weights_at + 4)?).ok()?;
    if weight_capacity < weight_count
        || View::u32_le_at(payload, weights_at + 8)? != 8
        || weight_capacity > 100_000
    {
        return None;
    }
    let weights = sketch_scalar_values(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(View::u32_le_at(payload, points_at)?).ok()?;
    let point_capacity = usize::try_from(View::u32_le_at(payload, points_at + 4)?).ok()?;
    if point_capacity < point_count
        || point_capacity > 100_000
        || View::u32_le_at(payload, points_at + 8)? != 8
        || knot_count
            != point_count
                .checked_add(usize::try_from(degree).ok()?)?
                .checked_add(1)?
    {
        return None;
    }
    let coordinates = sketch_scalar_values(payload, points_at + 12, point_count.checked_mul(3)?)?;
    let geometry = match admit_source_sketch_nurbs(
        degree,
        fit_tolerance,
        knots,
        weights,
        &coordinates,
        record_at,
    ) {
        Ok(Some(geometry)) => geometry,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok((
        SketchCurveGeometry::nurbs_from_parts(
            carrier_reference,
            subtype_class_tag,
            View::u32_le_at(payload, base + 15)?,
            geometry,
        ),
        points_at + 12 + point_count * 24,
    )))
}

fn admit_source_sketch_nurbs(
    degree: u32,
    fit_tolerance_cm: f64,
    knots: Vec<f64>,
    weights: Vec<f64>,
    coordinates: &[f64],
    record_at: usize,
) -> Result<Option<crate::records::sketch_geometry::SketchNurbsGeometry>, CodecError> {
    let Some(fit_tolerance_cm) = NonNegativeLength::new(fit_tolerance_cm) else {
        return Ok(None);
    };
    let fit_tolerance_mm = fit_tolerance_cm.get() * 10.0;
    let Some(fit_tolerance_mm) = NonNegativeLength::new(fit_tolerance_mm) else {
        return Err(crate::design::text::malformed_design(format_args!(
            "F3D sketch NURBS at byte {record_at} fit tolerance overflows millimetres"
        )));
    };

    let mut control_points = Vec::new();
    for point in coordinates.chunks_exact(3) {
        let Some(source) = FinitePoint3::new(Point3::new(point[0], point[1], point[2])) else {
            return Ok(None);
        };
        control_points.push(scale_sketch_point(source, record_at, "NURBS")?);
    }
    let Ok(poles) = crate::records::sketch_geometry::SketchNurbsPoles::from_checked_points(
        control_points,
        weights,
    ) else {
        return Ok(None);
    };
    Ok(
        crate::records::sketch_geometry::SketchNurbsGeometry::from_checked_parts(
            degree,
            fit_tolerance_mm,
            knots,
            poles,
        )
        .ok(),
    )
}

fn decode_line(
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    decode_line_values(payload, 133, record_at)
}

/// Value count that `decode_line_family` reports for the compact planar line.
const COMPACT_PLANAR_LINE_VALUES: usize = 9;

fn decode_compact_planar_line(
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let values_at = 133;
    let mut values = [0.0; 9];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, values_at + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    // Each z component is zero or floating-point noise below the resolution
    // of the same vector's own coordinates; the projection reads it as zero.
    for z in [2, 5, 8] {
        if !planar_z_is_zero(values[z - 2], values[z - 1], values[z]) {
            return Ok(None);
        }
        values[z] = 0.0;
    }
    // The values are followed by one reference: the plain eleven-byte
    // spelling or the 51-byte typed spelling.
    let reference_at = values_at + 9 * 8;
    let plain = marked_u32(payload, reference_at).is_some_and(|(_, reference_end)| {
        payload.get(reference_end..reference_end + 6) == Some(&[0; 6])
    });
    if !plain && crate::bytes::typed_local_reference_target(payload, reference_at).is_none() {
        return Ok(None);
    }
    decode_line_components(&values, Vector3::new(0.0, 0.0, 1.0), record_at)
}

/// Whether `z` is zero for a planar vector `(x, y, z)` stored in centimetres:
/// exactly zero, or at most `f64::EPSILON * max(|x|, |y|, 1 cm)`.
pub(in crate::design) fn planar_z_is_zero(x: f64, y: f64, z: f64) -> bool {
    z == 0.0 || z.abs() <= f64::EPSILON * x.abs().max(y.abs()).max(1.0)
}

/// Raw z components of a compact planar line at `payload` offset 133 when
/// at least one is nonzero noise admitted by [`planar_z_is_zero`].
pub(in crate::design) fn compact_planar_line_z_noise(payload: &[u8]) -> Option<[f64; 3]> {
    let raw = [2usize, 5, 8].map(|index| View::f64_le_at(payload, 133 + index * 8));
    let raw = [raw[0]?, raw[1]?, raw[2]?];
    raw.iter().any(|z| *z != 0.0).then_some(raw)
}

fn decode_line_family(
    payload: &[u8],
    record_at: usize,
) -> Result<Option<(SketchCurveGeometry, usize)>, CodecError> {
    if let Some(geometry) = decode_line(payload, record_at)? {
        return Ok(Some((geometry, 12)));
    }
    Ok(decode_compact_planar_line(payload, record_at)?
        .map(|geometry| (geometry, COMPACT_PLANAR_LINE_VALUES)))
}

fn decode_line_values(
    payload: &[u8],
    values_at: usize,
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let mut values = [0.0; 12];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, values_at + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    let stored_normal = Vector3::new(values[9], values[10], values[11]);
    decode_line_components(&values, stored_normal, record_at)
}

fn decode_line_components(
    values: &[f64],
    stored_normal: Vector3,
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let displacement = Vector3::new(values[3], values[4], values[5]);
    let (Some(_source_direction), Some(_stored_direction), Some(stored_normal)) = (
        UnitVector3::normalized(displacement),
        UnitVector3::new(Vector3::new(values[6], values[7], values[8])),
        UnitVector3::new(stored_normal),
    ) else {
        return Ok(None);
    };
    // Start plus displacement carries the bounded line and is corroborated by
    // the persistent endpoint records. Imported sketches can retain a stale
    // auxiliary unit direction, so derive the neutral tangent from the
    // admitted endpoints just as the normal is orthogonalized below.
    let Some(start_cm) = FinitePoint3::new(Point3::new(values[0], values[1], values[2])) else {
        return Ok(None);
    };
    let start = scale_sketch_point(start_cm, record_at, "line")?;
    let end = FinitePoint3::new(start.get().translated(displacement, 10.0)).ok_or_else(|| {
        crate::design::text::malformed_design(format_args!(
            "F3D sketch line at byte {record_at} overflows millimetres"
        ))
    })?;
    let start_raw = start.get();
    let end_raw = end.get();
    let displacement_mm = FiniteVector3::new(Vector3::new(
        end_raw.x - start_raw.x,
        end_raw.y - start_raw.y,
        end_raw.z - start_raw.z,
    ))
    .ok_or_else(|| {
        crate::design::text::malformed_design(format_args!(
            "F3D sketch line at byte {record_at} has an overflowing displacement"
        ))
    })?;
    let Some(direction) = UnitVector3::normalized(displacement_mm.get()) else {
        return Ok(None);
    };
    // The stored line normal is an auxiliary orientation vector. Imported
    // legacy sketches can retain a small component along the line direction;
    // remove that component so the typed carrier maintains its orthonormal
    // invariant without changing the line's endpoints or orientation side.
    let dot = direction.as_raw().dot(*stored_normal.as_raw());
    let projected_normal = *stored_normal.as_raw() - direction.as_raw().scale(dot);
    let projected_length = projected_normal.norm();
    let normal = if projected_length.is_finite()
        && projected_length > EPS_SKETCH_DECODE_LINE_COMPONENTS_E12
    {
        let Some(normal) = UnitVector3::normalized(projected_normal) else {
            return Ok(None);
        };
        normal
    } else {
        // Spatial line carriers can store a unit auxiliary vector parallel to
        // the line. The neutral spatial-line geometry has no plane normal;
        // retain the bounded carrier and choose a stable perpendicular basis
        // vector so the native geometry record still satisfies its typed
        // invariant.
        let basis = [
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ]
        .into_iter()
        .min_by(|left, right| {
            direction
                .as_raw()
                .dot(*left)
                .abs()
                .total_cmp(&direction.as_raw().dot(*right).abs())
        });
        let Some(basis) = basis else {
            return Ok(None);
        };
        let Some(normal) = UnitVector3::normalized(direction.as_raw().cross(basis)) else {
            return Ok(None);
        };
        normal
    };
    Ok(SketchCurveGeometry::line_from_parts(start, end, direction, normal).ok())
}

struct ParsedSketchRelationMember {
    reference: crate::records::identity::Located<u32, usize>,
    relation_ordinal: u32,
}

struct ParsedSketchRelation {
    members: Vec<ParsedSketchRelationMember>,
    auxiliary_references: Vec<crate::records::identity::Located<u32, usize>>,
    owner_reference: u32,
    owner_reference_offset: usize,
    state: u64,
    state_offset: usize,
    entity_genesis: Option<u64>,
    class_members: RelationClassMembers,
    return_members: Vec<crate::records::identity::Located<u32, usize>>,
    parsed_end: usize,
}

/// Type GUID of the shared sketch-relation class, which adds no member of its
/// own between the property block and `ParentNode`.
const RELATION_TYPE_GUID: &str = "60403D47-0C49-49B0-BDE8-1679608164A2";
/// Type GUID of the offset class, whose relations carry mask `0x2000000000`.
const OFFSET_RELATION_TYPE_GUID: &str = "D3BD153B-EB8A-405E-9D29-69EE0C3D227C";
/// Type GUID of the spline-group class, whose relations carry mask `0x80000000`.
const SPLINE_RELATION_TYPE_GUID: &str = "73762C3B-82DC-4632-93B0-B8FE1CC5282F";
/// Type GUID of the tangency class, whose relations carry mask `0x100`.
const TANGENT_RELATION_TYPE_GUID: &str = "24DB790E-3DCD-4336-AFA3-6F119EF2239B";
/// Type GUID of the circular-pattern class, mask `0x10000000`.
const CIRCULAR_PATTERN_RELATION_TYPE_GUID: &str = "8269E861-0BB7-47E0-9911-5AE3EC475058";
/// Type GUID of the rectangular-pattern class, mask `0x20000000`.
const RECTANGULAR_PATTERN_RELATION_TYPE_GUID: &str = "40800FB9-C2BE-494E-A047-7D76E82B9F6C";
/// Type GUID of the text-frame class, mask `0x10000000000`.
const TEXT_FRAME_RELATION_TYPE_GUID: &str = "8B369926-123F-4F9D-878E-6D4C076128D3";
/// Type GUID of the text-path class, mask `0x20000000000`.
const TEXT_PATH_RELATION_TYPE_GUID: &str = "9D30FCDC-EA07-4141-93E2-918B1A59E962";

/// A sketch-relation record class, named by the type GUID its segment's type
/// table carries for the record's entity. The class fixes the members written
/// between the property block and the base class's `ParentNode`. A class tag
/// cannot name it: a tag is `256` plus an index into the segment's own type
/// table, so one tag names different relation classes in different segments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SketchRelationClass {
    /// A class whose most-derived level adds no member: the shared relation
    /// class, the offset class, and the spline-group class.
    Plain,
    /// Three `u8` flags, each `0` or `1`.
    Tangent,
    /// The angle-parameter reference, the count-parameter reference, the
    /// evaluated f64 total angle in radians, the evaluated u32 instance count,
    /// the pattern tables, and one `u8`.
    CircularPattern,
    /// Three `u8` flags, a u32-counted run of references, the pattern tables,
    /// and two direction clauses.
    RectangularPattern,
    /// Two references.
    TextFrame,
    /// The text-entity reference, a u32 character count, and that many glyph
    /// blocks. `leading_flag` is the `u8` the class writes before the
    /// reference, which it does from class version 1.
    TextPath {
        /// Whether the class version writes the leading `u8`.
        leading_flag: bool,
    },
}

impl SketchRelationClass {
    /// The relation class `type_guid` names at class version `version`, or
    /// `None` where the GUID is not a sketch-relation class.
    fn of(type_guid: &str, version: u32) -> Option<Self> {
        let matches = |known: &str| type_guid.eq_ignore_ascii_case(known);
        if matches(RELATION_TYPE_GUID)
            || matches(OFFSET_RELATION_TYPE_GUID)
            || matches(SPLINE_RELATION_TYPE_GUID)
        {
            return Some(Self::Plain);
        }
        if matches(TANGENT_RELATION_TYPE_GUID) {
            return Some(Self::Tangent);
        }
        if matches(CIRCULAR_PATTERN_RELATION_TYPE_GUID) {
            return Some(Self::CircularPattern);
        }
        if matches(RECTANGULAR_PATTERN_RELATION_TYPE_GUID) {
            return Some(Self::RectangularPattern);
        }
        if matches(TEXT_FRAME_RELATION_TYPE_GUID) {
            return Some(Self::TextFrame);
        }
        if matches(TEXT_PATH_RELATION_TYPE_GUID) {
            return Some(Self::TextPath {
                leading_flag: version >= 1,
            });
        }
        None
    }
}

/// Largest plausible counted run inside a sketch relation; a larger count is a
/// misparse rather than a record that owns that many members.
const MAX_RELATION_RUN: usize = 4096;

/// Serialized width selected by a sketch relation's leading-block member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SketchRelationMaskWidth {
    U32,
    U64,
}

impl SketchRelationMaskWidth {
    fn from_leading_block(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::U32),
            1 => Some(Self::U64),
            _ => None,
        }
    }

    fn has_paired_member_run(self) -> bool {
        self == Self::U64
    }
}

/// Read the relation's mask width from its leading-block presence member.
pub(crate) fn relation_mask_width(record: &[u8]) -> Option<SketchRelationMaskWidth> {
    let (_, start) = lp_ascii_filtered_view(record, 15, 0..=256, u8::is_ascii_graphic)?;
    SketchRelationMaskWidth::from_leading_block(*record.get(start)?)
}

/// Take one reference member at `cursor`, returning its 32-bit target and the
/// byte offset of the target within the reference. Relation members address
/// records in the relation's own segment, so a reference whose target does not
/// fit a `u32` is a misparse.
fn take_relation_reference(
    payload: &[u8],
    cursor: &mut usize,
) -> Option<crate::records::identity::Located<u32, usize>> {
    let at = *cursor;
    let reference = take_reference(payload, cursor)?;
    Some(crate::records::identity::Located {
        value: u32::try_from(reference.target()?).ok()?,
        offset: at + 1,
    })
}

#[derive(Clone, Copy)]
enum AuxiliaryRelationReference {
    Absent,
    Present(crate::records::identity::Located<u32, usize>),
}

impl AuxiliaryRelationReference {
    fn value(self) -> Option<u32> {
        match self {
            Self::Absent => None,
            Self::Present(reference) => Some(reference.value),
        }
    }
}

/// Take one reference member that the class may leave absent, recording it in
/// the auxiliary run when it is present. An absent reference is one zero byte
/// and names nothing, so it contributes no entry.
fn take_auxiliary_relation_reference(
    payload: &[u8],
    cursor: &mut usize,
    auxiliary_references: &mut Vec<crate::records::identity::Located<u32, usize>>,
) -> Option<AuxiliaryRelationReference> {
    let at = *cursor;
    let reference = take_reference(payload, cursor)?;
    let Some(target) = reference.target() else {
        return Some(AuxiliaryRelationReference::Absent);
    };
    let value = u32::try_from(target).ok()?;
    let located = crate::records::identity::Located {
        value,
        offset: at + 1,
    };

    auxiliary_references.push(located);
    Some(AuxiliaryRelationReference::Present(located))
}

/// Skip the two tables both pattern classes write after their own leading
/// members: a u32-counted map whose entry is a u64 key, a u32 count, and that
/// many u64 values; then a u32-counted run of u32.
fn skip_pattern_tables(payload: &[u8], cursor: &mut usize) -> Option<usize> {
    let entries = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
    if entries > MAX_RELATION_RUN {
        return None;
    }
    *cursor += 4;
    for _ in 0..entries {
        let values = usize::try_from(View::u32_le_at(payload, *cursor + 8)?).ok()?;
        if values > MAX_RELATION_RUN {
            return None;
        }
        *cursor += 12 + values * 8;
    }
    let ordinals = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
    if ordinals > MAX_RELATION_RUN {
        return None;
    }
    *cursor += 4 + ordinals * 4;
    (*cursor <= payload.len()).then_some(ordinals)
}

#[derive(Clone, Copy)]
struct ParsedSketchPatternDirection {
    count_offset: usize,
    count_parameter: Option<u32>,
    direction_offset: usize,
    distance_parameter: Option<u32>,
}

/// What a sketch-relation subclass leaves behind after its own members.
enum RelationClassMembers {
    Plain,
    Tangent,
    CircularPattern {
        angle_parameter: Option<u32>,
        count_parameter: Option<u32>,
        evaluated_offset: usize,
        has_suppression: bool,
    },
    Rectangular {
        flags: [u8; 3],
        reference_count: u32,
        clauses: [ParsedSketchPatternDirection; 2],
        has_suppression: bool,
    },
    TextFrame {
        text_reference: Option<u32>,
    },
    TextPath {
        glyph_transforms: Vec<SketchGlyphTransform>,
    },
}

/// Consume the members `class` writes between the property block and the base
/// class's `ParentNode`, advancing `cursor` past them and recording every
/// present reference among them in the auxiliary run
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
fn parse_relation_class_members(
    payload: &[u8],
    cursor: &mut usize,
    class: SketchRelationClass,
    auxiliary_references: &mut Vec<crate::records::identity::Located<u32, usize>>,
) -> Result<Option<RelationClassMembers>, CodecError> {
    let parsed = (|| {
        macro_rules! take {
            () => {
                take_auxiliary_relation_reference(payload, cursor, auxiliary_references)
            };
        }
        Some(Ok(match class {
            SketchRelationClass::Plain => RelationClassMembers::Plain,
            SketchRelationClass::Tangent => {
                for _ in 0..3 {
                    // A flag outside `{0, 1}` is a misparse, not a third state.
                    if !matches!(payload.get(*cursor)?, 0 | 1) {
                        return None;
                    }
                    *cursor += 1;
                }
                RelationClassMembers::Tangent
            }
            SketchRelationClass::TextFrame => {
                take!()?;
                let text_reference = take!()?.value();
                RelationClassMembers::TextFrame { text_reference }
            }
            SketchRelationClass::CircularPattern => {
                let angle_parameter = take!()?.value();
                let count_parameter = take!()?.value();
                let evaluated_offset = *cursor;
                *cursor += 12;
                let has_suppression = skip_pattern_tables(payload, cursor)? != 0;
                if payload.get(*cursor)? != &0 {
                    return None;
                }
                *cursor += 1;
                RelationClassMembers::CircularPattern {
                    angle_parameter,
                    count_parameter,
                    evaluated_offset,
                    has_suppression,
                }
            }
            SketchRelationClass::RectangularPattern => {
                let flags = payload
                    .get(*cursor..cursor.checked_add(3)?)?
                    .try_into()
                    .ok()?;
                *cursor += 3;
                let reference_count = View::u32_le_at(payload, *cursor)?;
                let references = usize::try_from(reference_count).ok()?;
                if references > MAX_RELATION_RUN {
                    return None;
                }
                *cursor += 4;
                for _ in 0..references {
                    take!()?;
                }
                let has_suppression = skip_pattern_tables(payload, cursor)? != 0;
                let mut clauses = [ParsedSketchPatternDirection {
                    count_offset: 0,
                    count_parameter: None,
                    direction_offset: 0,
                    distance_parameter: None,
                }; 2];
                for clause in &mut clauses {
                    clause.count_offset = *cursor;
                    *cursor += 4;
                    clause.count_parameter = take!()?.value();
                    clause.direction_offset = *cursor;
                    *cursor += 32;
                    clause.distance_parameter = take!()?.value();
                }
                RelationClassMembers::Rectangular {
                    flags,
                    reference_count,
                    clauses,
                    has_suppression,
                }
            }

            SketchRelationClass::TextPath { leading_flag } => {
                if leading_flag {
                    if payload.get(*cursor)? != &1 {
                        return None;
                    }
                    *cursor += 1;
                }
                let (text_reference, transforms, end) = match parse_text_glyph_run(payload, *cursor)
                {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                auxiliary_references.push(crate::records::identity::Located {
                    value: text_reference,
                    offset: *cursor + 1,
                });
                *cursor = end;
                RelationClassMembers::TextPath {
                    glyph_transforms: transforms,
                }
            }
        }))
    })();
    parsed.transpose()
}

/// Parse one sketch-relation record body whose class is known
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
///
/// The payload is `u8 1`, a u32 count and that many `(reference, u32 relation
/// ordinal)` pairs, the property-block presence byte and its block, the
/// class-defined members, the `ParentNode` reference naming the owning sketch,
/// a u64 constraint mask, a u32 count and that many bare references, and one
/// zero byte. At relation base-class version 0 the leading byte is zero, the
/// pair list is absent, and the mask is a u32. Both reference runs hold the
/// same members; only the second is in semantic order.
fn parse_classed_sketch_relation(
    payload: &[u8],
    class: SketchRelationClass,
) -> Result<Option<ParsedSketchRelation>, CodecError> {
    let parsed = (|| {
        // The record header is the LP-ASCII class tag, the u64 entity id, and the
        // LP-ASCII record name; the member payload follows it.
        let (_, start) = lp_ascii_filtered_view(payload, 15, 0..=256, u8::is_ascii_graphic)?;
        let mask_width = SketchRelationMaskWidth::from_leading_block(*payload.get(start)?)?;
        let paired_run = mask_width.has_paired_member_run();
        let mut cursor = start + 1;
        let mut members = Vec::new();
        if paired_run {
            let member_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
            if member_count > MAX_RELATION_RUN {
                return None;
            }
            cursor += 4;

            for _ in 0..member_count {
                let reference = take_relation_reference(payload, &mut cursor)?;
                members.push(ParsedSketchRelationMember {
                    reference,
                    relation_ordinal: View::u32_le_at(payload, cursor)?,
                });
                cursor += 4;
            }
        }
        // The base class level opens with its property-block presence byte. The
        // block is `u32 count` and that many `(key, type name, value)` triples;
        // `EntityGenesis` is one such key.
        let entity_genesis = read_property_block(payload, &mut cursor)?.find("EntityGenesis");
        let mut auxiliary_references = Vec::new();
        let class_members = match parse_relation_class_members(
            payload,
            &mut cursor,
            class,
            &mut auxiliary_references,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let owner = take_relation_reference(payload, &mut cursor)?;
        let state_offset = cursor;
        // The constraint mask follows `ParentNode` directly. It is a u64 in the
        // paired-run form and a u32 at relation base-class version 0.
        let (state, mut cursor) = match mask_width {
            SketchRelationMaskWidth::U64 => {
                (View::u64_le_at(payload, state_offset)?, state_offset + 8)
            }
            SketchRelationMaskWidth::U32 => (
                u64::from(View::u32_le_at(payload, state_offset)?),
                state_offset + 4,
            ),
        };
        let return_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if return_count > MAX_RELATION_RUN {
            return None;
        }
        cursor += 4;

        let mut return_members = Vec::new();
        for _ in 0..return_count {
            return_members.push(take_relation_reference(payload, &mut cursor)?);
        }
        if payload.get(cursor) != Some(&0) {
            return None;
        }
        let parsed_end = cursor + 1;
        Some(Ok(ParsedSketchRelation {
            members,
            auxiliary_references,
            owner_reference: owner.value,
            owner_reference_offset: owner.offset,
            state,
            state_offset,
            entity_genesis,
            class_members,
            return_members,
            parsed_end,
        }))
    })();
    parsed.transpose()
}

/// Parse a text-path glyph run at `at`: the marked text-entity reference,
/// six zero bytes, a u32 character count, and that many blocks of `u32 16`
/// followed by sixteen finite f64 values forming a row-major 4×4 character
/// placement transform. Returns the text reference, the transforms in
/// character order, and the offset directly after the last block.
type TextGlyphRun = (u32, Vec<SketchGlyphTransform>, usize);

fn parse_text_glyph_run(payload: &[u8], at: usize) -> Result<Option<TextGlyphRun>, CodecError> {
    let parsed = (|| {
        let (text_reference, end) = marked_u32(payload, at)?;
        let mut view = View::over_retained(payload);
        view.seek(end)?;
        if view.take(6)? != [0u8; 6] {
            return None;
        }
        let count = usize::try_from(view.u32_le()?).ok()?;
        if !(1..=4096).contains(&count) {
            return None;
        }

        let mut transforms = Vec::new();
        for _ in 0..count {
            if view.u32_le()? != 16 {
                return None;
            }
            let mut transform = [[0.0; 4]; 4];
            for row in &mut transform {
                for cell in row {
                    *cell = view.f64_le()?;
                }
            }
            transforms.push(SketchGlyphTransform::try_from(transform).ok()?);
        }
        Some(Ok((text_reference, transforms, view.position())))
    })();
    parsed.transpose()
}

/// Validated indexed-record identity and byte offset.
pub(in crate::design::decode) struct IndexedRecordHeader {
    pub(super) offset: usize,
    pub(super) record_index: u32,
    pub(super) class_tag: crate::records::references::DesignClassTag,
}

pub(super) fn indexed_record_header_at(bytes: &[u8], at: usize) -> Option<IndexedRecordHeader> {
    if View::u32_le_at(bytes, at)? != 3 {
        return None;
    }
    let class_tag = std::str::from_utf8(bytes.get(at + 4..at + 7)?)
        .ok()?
        .to_owned()
        .try_into()
        .ok()?;
    Some(IndexedRecordHeader {
        offset: at,
        record_index: View::u32_le_at(bytes, at + 7)?,
        class_tag,
    })
}

pub(in crate::design) fn next_indexed_record_offset(
    bytes: &[u8],
    position: usize,
) -> Option<usize> {
    indexed_record_offsets(bytes.get(position..)?)
        .next()
        .map(|header| position + header.offset)
}

/// Header offsets of every indexed record whose class tag is three characters.
///
/// A class tag is `256` plus an index into the segment's own type table, so a
/// tag reaches four characters only in a segment registering more than 744
/// types. No segment registers that many.
pub(super) fn indexed_record_offsets(
    bytes: &[u8],
) -> impl Iterator<Item = IndexedRecordHeader> + '_ {
    memchr::memmem::find_iter(bytes, &[3, 0, 0, 0])
        .filter_map(|at| indexed_record_header_at(bytes, at))
}

pub(super) fn next_indexed_record_offset_with_index(
    bytes: &[u8],
    mut position: usize,
    record_index: u32,
) -> Option<usize> {
    loop {
        let offset = next_indexed_record_offset(bytes, position)?;
        if indexed_record_header_at(bytes, offset)
            .is_some_and(|header| header.record_index == record_index)
        {
            return Some(offset);
        }
        position = offset.checked_add(1)?;
    }
}

fn marked_u32(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    (bytes.get(position) == Some(&1))
        .then_some((View::u32_le_at(bytes, position + 1)?, position + 5))
}

struct SketchReferenceList {
    record_reference: crate::records::identity::Located<Option<u32>>,
    references: Vec<crate::records::identity::Located<u32>>,
    end: usize,
}

fn decode_reference_list(bytes: &[u8], position: usize) -> Option<SketchReferenceList> {
    // The eight-byte base-record slot is either a u32 record reference with a
    // zero high half or the all-ones sentinel marking a sketch with no base
    // record; the list grammar is identical in both forms.
    let parsed = (|| {
        let mut view = View::over_retained(bytes);
        view.seek(position)?;
        let low = view.u32_le()?;
        let high = view.u32_le()?;
        let record_reference = match (low, high) {
            (u32::MAX, u32::MAX) => None,
            (reference, 0) => Some(reference),
            _ => return None,
        };
        if view.u8()? != 1 {
            return None;
        }
        let declared_count = usize::try_from(view.u32_le()?).ok()?;
        Some((view, record_reference, declared_count))
    })();
    let (mut view, record_reference, declared_count) = parsed?;
    let required_bytes = declared_count.checked_mul(11)?;
    let remaining_bytes = bytes.len().checked_sub(view.position())?;
    if remaining_bytes < required_bytes {
        return None;
    }

    let mut references = Vec::new();
    loop {
        let mut probe = view;
        if probe.u8() != Some(1) {
            break;
        }
        let offset = probe.position();
        let Some(reference) = probe.u32_le() else {
            break;
        };
        if probe.take(6) != Some(&[0; 6]) {
            break;
        }
        if references.len() == declared_count {
            return None;
        }
        references.push(crate::records::identity::Located {
            value: reference,
            offset: u64_from_index(offset),
        });
        view = probe;
    }
    (references.len() == declared_count).then_some(SketchReferenceList {
        record_reference: crate::records::identity::Located {
            value: record_reference,
            offset: u64_from_index(position),
        },
        references,
        end: view.position(),
    })
}

fn decode_sketch_streams<T>(
    scan: &ContainerScan,
    decode: impl Fn(&[u8], &crate::metastream::MetaStream, &str) -> Result<Vec<T>, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        out.extend(decode(bytes, &meta, &entry.name)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
