// SPDX-License-Identifier: Apache-2.0
//! Parse body members, bounds, bindings, and visibility.

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::container::ContainerRole;

use crate::bytes::take_reference;
use crate::container::ContainerScan;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::text::design_record_id;
use crate::design::RECIPES;
use crate::ids::native_stream;
use crate::layout::indexed_design_record_header;
use crate::layout::snapshot_body_map_plain_prefix as snapshot_prefix;
use crate::layout::snapshot_body_map_plain_tail as snapshot_tail;
use crate::records::{
    bodies::{DesignBodyBinding, DesignBodyBounds, DesignBodyMember},
    entity_header::{DesignEntityHeader, DESIGN_MODULE_BODY},
    recipes::{ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector},
};
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::{bounded_len, index_from_u32, u64_from_index, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::{HashMap, HashSet};

/// Decode the `BodiesRoot` member list following the doubled `BodiesRoot`
/// marker in each design `BulkStream` entry in `scan`: each member's entity
/// suffix and flags. The decode is rejected (no members returned for that
/// stream) unless the declared count is fully consumed and immediately
/// followed by a zero byte.
pub(crate) fn decode_body_members(
    scan: &ContainerScan,
) -> Result<Vec<DesignBodyMember>, CodecError> {
    let mut out = Vec::new();
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    prefix.extend_from_slice(&0u16.to_le_bytes());
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
        else {
            continue;
        };
        let count_offset = start + prefix.len();
        let mut view = View::over_retained(bytes);
        if view.seek(count_offset).is_none() {
            continue;
        }
        let Some(count) = view.u32_le().map(index_from_u32) else {
            continue;
        };
        if count > 100_000 {
            continue;
        }
        let Some(count) = bounded_len(u64_from_index(count), 11, view.remaining()) else {
            continue;
        };

        let mut decoded = Vec::new();
        for _ in 0..count {
            let cursor = view.position();
            if view.u8() != Some(1) {
                decoded.clear();
                break;
            }
            let Some(entity_suffix) = view.u64_le() else {
                decoded.clear();
                break;
            };
            let Some(flags) = view.u16_le() else {
                decoded.clear();
                break;
            };
            let id = design_record_id(&entry.name, ":design-body-member#", u64_from_index(cursor));
            decoded.push(
                DesignBodyMember::try_from(crate::records::bodies::DesignBodyMemberWire {
                    id,
                    byte_offset: u64_from_index(cursor),
                    entity_suffix,
                    flags,
                })
                .map_err(CodecError::Malformed)?,
            );
        }
        if decoded.len() == count && bytes.get(view.position()) == Some(&0) {
            out.extend(decoded);
        }
    }
    Ok(out)
}

/// Decode the three consecutive indexed records that cache each Design body's
/// axis-aligned model-space bounds.
pub(crate) fn decode_body_bounds(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignBodyBounds>, CodecError> {
    let mut out = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| entity.module() == Some(DESIGN_MODULE_BODY))
    {
        let Some(stream) = native_stream(&entity.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = usize::try_from(entity.byte_offset).ok() else {
            continue;
        };
        let end = entities
            .iter()
            .filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.byte_offset > entity.byte_offset
            })
            .filter_map(|candidate| usize::try_from(candidate.byte_offset).ok())
            .min()
            .unwrap_or(bytes.len());
        let Ok(record_index) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        let Some(record_indices) = record_index
            .checked_add(1)
            .zip(record_index.checked_add(2))
            .zip(record_index.checked_add(3))
            .map(|((first, second), third)| [first, second, third])
        else {
            continue;
        };
        let mut record_offsets = [0; 3];
        let mut unique = true;
        for (ordinal, wanted) in record_indices.into_iter().enumerate() {
            let mut matches = indexed_headers_in(bytes, start, end)
                .filter(|(_, record_index)| *record_index == wanted)
                .map(|(offset, _)| offset);
            match (matches.next(), matches.next()) {
                (Some(offset), None) => record_offsets[ordinal] = offset,
                _ => {
                    unique = false;
                    break;
                }
            }
        }
        if !unique {
            continue;
        }
        let [first, second, third] = record_offsets;
        if !(first < second && second < third) {
            continue;
        }
        let Some(search_at) = third.checked_add(11) else {
            continue;
        };
        let third_end = next_indexed_record_offset(bytes, search_at)
            .filter(|offset| *offset <= end)
            .unwrap_or(end);
        let intervals = [(first, second), (second, third), (third, third_end)];
        let mut repeated = body_bound_candidates(bytes, intervals[0].0, intervals[0].1).filter_map(
            |(marker_offset, values)| {
                let frame = bytes.get(marker_offset..marker_offset + 49)?;
                let mut value_offsets = [marker_offset + 1, 0, 0];
                for (ordinal, (record_start, record_end)) in
                    intervals.iter().copied().enumerate().skip(1)
                {
                    let mut matches = body_bound_candidates(bytes, record_start, record_end)
                        .filter(|(offset, _)| {
                            offset
                                .checked_add(49)
                                .and_then(|end| bytes.get(*offset..end))
                                == Some(frame)
                        })
                        .map(|(offset, _)| offset + 1);
                    value_offsets[ordinal] = match (matches.next(), matches.next()) {
                        (Some(offset), None) => offset,
                        _ => return None,
                    };
                }
                Some((values, value_offsets))
            },
        );
        let Some((values, value_offsets)) = repeated.next() else {
            continue;
        };
        if repeated.any(|candidate| candidate.0 != values || candidate.1 != value_offsets) {
            continue;
        }
        let corner = |start: usize| {
            FinitePoint3::new(Point3::new(
                values[start].get() * 10.0,
                values[start + 1].get() * 10.0,
                values[start + 2].get() * 10.0,
            ))
            .ok_or_else(|| {
                CodecError::Malformed("scene bounds maximum and minimum must be finite".into())
            })
        };
        let id = design_record_id(&entry.name, ":design-body-bounds#", entity.byte_offset);
        let record = DesignBodyBounds::from_parts(crate::records::bodies::DesignBodyBoundsWire {
            id,
            entity_suffix: entity.entity_id.suffix(),
            entity_byte_offset: entity.byte_offset,
            record_indices,
            record_byte_offsets: [
                u64_from_index(first),
                u64_from_index(second),
                u64_from_index(third),
            ],
            value_byte_offsets: value_offsets.map(u64_from_index),
            body_binding_ids: Vec::new(),
            maximum: corner(0)?,
            minimum: corner(3)?,
        })
        .map_err(CodecError::Malformed)?;

        out.push(record);
    }
    out.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(out)
}

fn indexed_headers_in(
    bytes: &[u8],
    mut position: usize,
    end: usize,
) -> impl Iterator<Item = (usize, u32)> + '_ {
    std::iter::from_fn(move || {
        while position + 11 <= end {
            let at = position;
            position += 1;
            if View::u32_le_at(bytes, at) != Some(3) {
                continue;
            }
            let Some(class_tag) = bytes.get(at + 4..at + 7) else {
                continue;
            };
            if !class_tag.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let Some(record_index) = View::u32_le_at(bytes, at + 7) else {
                continue;
            };
            return Some((at, record_index));
        }
        None
    })
}

fn body_bound_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> impl Iterator<Item = (usize, [FiniteReal; 6])> + '_ {
    end.checked_sub(48)
        .into_iter()
        .flat_map(move |last| start..last)
        .filter_map(move |offset| {
            if bytes.get(offset) != Some(&1) {
                return None;
            }
            let values = [
                FiniteReal::new(View::f64_le_at(bytes, offset + 1)?)?,
                FiniteReal::new(View::f64_le_at(bytes, offset + 9)?)?,
                FiniteReal::new(View::f64_le_at(bytes, offset + 17)?)?,
                FiniteReal::new(View::f64_le_at(bytes, offset + 25)?)?,
                FiniteReal::new(View::f64_le_at(bytes, offset + 33)?)?,
                FiniteReal::new(View::f64_le_at(bytes, offset + 41)?)?,
            ];
            ((0..3).all(|axis| values[axis] >= values[axis + 3])
                && (0..3).any(|axis| values[axis] > values[axis + 3]))
            .then_some((offset, values))
        })
}

pub(super) fn decode_stream(
    bytes: &[u8],
    stream: &str,
    out: &mut Vec<ConstructionRecipe>,
) -> Result<(), CodecError> {
    let mut counters: HashMap<(ConstructionRecipeKind, Option<&str>), u32> = HashMap::new();
    for &(name, kind) in RECIPES {
        let mut cursor = 0;
        while let Some(offset) = find_from(bytes, name, cursor) {
            cursor = offset + 1;
            if kind == ConstructionRecipeKind::Face
                && offset >= 8
                && &bytes[offset - 8..offset] == b"bounded_"
            {
                continue;
            }
            let framed_name = offset
                .checked_sub(4)
                .and_then(|at| View::u32_le_at(bytes, at))
                .and_then(|length| usize::try_from(length).ok())
                == Some(name.len());
            if !framed_name {
                continue;
            }
            let parsed_design_id = recipe_design_id(bytes, offset, name);
            let design = parsed_design_id
                .map(|(value, design_id_at)| -> Result<_, CodecError> {
                    let selector = design_id_at
                        .checked_add(value.len())
                        .and_then(|selector_at| {
                            Some(ConstructionRecipeSelector {
                                value: View::u32_le_at(bytes, selector_at)?,
                                byte_offset: u64::try_from(selector_at).ok()?,
                            })
                        });
                    Ok(crate::records::recipes::ConstructionRecipeDesign {
                        id: crate::records::identity::RecordedValue {
                            value: value.to_owned(),
                            offset: u64_from_index(design_id_at),
                        },
                        selector,
                    })
                })
                .transpose()?;
            let key = (kind, parsed_design_id.map(|(value, _)| value));
            let counter = counters.entry(key).or_default();
            let recipe_index = *counter;
            *counter += 1;
            // The record index word precedes the family marker by sixteen
            // bytes. A marker within the first sixteen bytes of the stream has
            // no such word, so the stream states no record index for it; the
            // word itself always lies inside `bytes` when the marker does.
            let record_index = offset.checked_sub(16).and_then(|at| {
                Some(crate::records::identity::RecordedValue {
                    value: View::i32_le_at(bytes, at)?,
                    offset: u64_from_index(at),
                })
            });
            let recipe = ConstructionRecipe {
                id: design_record_id(stream, ":construction-recipe#", u64_from_index(offset)),
                byte_offset: u64_from_index(offset),
                kind,
                design,
                recipe_index,
                record_index,
            };

            out.push(recipe);
        }
    }
    out.sort_by(|left, right| {
        let left_key = {
            let recipe = left;
            {
                recipe.record_index.map(|index| index.value)
            }
        };
        let right_key = {
            let recipe = right;
            {
                recipe.record_index.map(|index| index.value)
            }
        };
        left_key.cmp(&right_key)
    });
    Ok(())
}

fn recipe_design_id<'a>(bytes: &'a [u8], offset: usize, name: &[u8]) -> Option<(&'a str, usize)> {
    let id_end = offset.checked_sub(20)?;
    for length in 1..=8usize {
        let Some(length_at) = id_end.checked_sub(4 + length) else {
            continue;
        };
        if let Some((id, value_offset)) = ascii_id_at(bytes, length_at) {
            if value_offset.checked_add(id.len()) == Some(id_end) {
                return Some((id, value_offset));
            }
        }
    }
    if offset >= 23 {
        let candidate = bytes.get(offset - 23..offset - 20)?;
        if candidate.iter().all(u8::is_ascii_digit) {
            return Some((std::str::from_utf8(candidate).ok()?, offset - 23));
        }
    }
    ascii_id_at(bytes, offset + name.len() + 8)
}

fn ascii_id_at(bytes: &[u8], length_offset: usize) -> Option<(&str, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, length_offset)?).ok()?;
    if !(1..=8).contains(&length) {
        return None;
    }
    let value = bytes.get(length_offset + 4..length_offset + 4 + length)?;
    if !value.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some((std::str::from_utf8(value).ok()?, length_offset + 4))
}

/// One `(asm_body_key, entity_suffix)` pair from a Design `BulkStream` BREP
/// body-map record, with the named B-rep blob the key resolves in and the
/// suffix's byte offset for native patching.
pub(crate) struct BodyBinding {
    /// The referenced ASM body key.
    pub(crate) asm_key: u64,
    /// Byte offset of `asm_key` within the stream.
    pub(crate) asm_key_offset: usize,
    /// The body's design-entity suffix.
    pub(crate) entity_suffix: u64,
}

impl BodyBinding {
    /// Byte offset of `entity_suffix`, which follows `asm_key` in the pair.
    pub(crate) fn entity_suffix_offset(&self) -> usize {
        self.asm_key_offset + 8
    }
}

/// One exactly framed Design body-map record.
///
/// The record owns the blob name and its location, and its ordered `bindings`
/// are the map's pairs: a binding's ordinal is its index and the pair count is
/// `bindings.len()`.
struct BodyMapRecord {
    blob_name: String,
    /// Byte offset of the BREP blob name's UTF-16LE code units.
    blob_name_offset: usize,
    bindings: Vec<BodyBinding>,
}

fn entity_has_type(meta: &crate::metastream::MetaStream, entity: u64, type_guid: &str) -> bool {
    meta.types.iter().any(|design_type| {
        design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(type_guid)
            && design_type
                .entities
                .values()
                .any(|registered| *registered == entity)
    })
}

#[derive(Clone, Copy)]
enum ReferencePadding {
    None,
    TwoZeros,
}

impl ReferencePadding {
    fn trailing_zeros(self) -> usize {
        match self {
            Self::None => 0,
            Self::TwoZeros => 2,
        }
    }
}

struct LocalReferenceCandidate {
    target: u64,
    end: usize,
    inline_type_guid: Option<String>,
    padding: ReferencePadding,
}

fn local_reference_candidates(
    bytes: &[u8],
    at: usize,
    allow_extra_zero: bool,
) -> Vec<LocalReferenceCandidate> {
    let mut candidates = Vec::new();
    let mut end = at;
    if let Some(reference) = take_reference(bytes, &mut end) {
        if let Some((target, inline_type_guid)) = reference.into_local() {
            candidates.push(LocalReferenceCandidate {
                target,
                end,
                inline_type_guid: inline_type_guid.map(std::borrow::ToOwned::to_owned),
                padding: ReferencePadding::TwoZeros,
            });
            if allow_extra_zero && bytes.get(end) == Some(&0) {
                candidates.push(LocalReferenceCandidate {
                    target,
                    end: end + 1,
                    inline_type_guid: inline_type_guid.map(std::borrow::ToOwned::to_owned),
                    padding: ReferencePadding::TwoZeros,
                });
            }
        }
    }
    if at.checked_add(2).and_then(|end| bytes.get(at..end)) == Some(&[1, 1]) {
        if let (Some(target_at), Some(end)) = (at.checked_add(2), at.checked_add(10)) {
            if let Some(target) = View::u64_le_at(bytes, target_at) {
                candidates.push(LocalReferenceCandidate {
                    target,
                    end,
                    inline_type_guid: None,
                    padding: ReferencePadding::None,
                });
                if allow_extra_zero && bytes.get(end) == Some(&0) {
                    candidates.push(LocalReferenceCandidate {
                        target,
                        end: end + 1,
                        inline_type_guid: None,
                        padding: ReferencePadding::None,
                    });
                }
            }
        }
    }
    candidates
}

fn reference_has_type(
    meta: &crate::metastream::MetaStream,
    reference: &LocalReferenceCandidate,
    expected_type_guid: &str,
) -> bool {
    reference
        .inline_type_guid
        .as_deref()
        .is_none_or(|guid| guid.eq_ignore_ascii_case(expected_type_guid))
        && entity_has_type(meta, reference.target, expected_type_guid)
}

/// Parse every exactly framed sibling body-map record that binds an `.smb`
/// snapshot. The carrier uses a bare entity header in every serializer band.
fn snapshot_body_map_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyMapRecord>, CodecError> {
    let frames = crate::metastream::primary_record_frames(meta, bytes.len())?;
    let mut primary_by_entity = HashMap::new();
    for (ordinal, frame) in frames.iter().enumerate() {
        primary_by_entity.insert(frame.entity_id, ordinal);
    }
    let mut out = Vec::new();
    for (type_ordinal, design_type) in meta.types.iter().enumerate() {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID)
        {
            continue;
        }
        if !crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_VERSIONS
            .contains(&design_type.version)
        {
            return Err(CodecError::NotImplemented(format!(
                "unsupported F3D Design snapshot body-map carrier version {}",
                design_type.version
            )));
        }
        if design_type.module != DESIGN_MODULE_BODY
            || !design_type
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                .is_some_and(|base| {
                    base.eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID)
                })
        {
            return Err(CodecError::malformed(
                "F3D Design snapshot body-map carrier has incompatible registration metadata",
            ));
        }
        let class_tag = u32::try_from(type_ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(256))
            .filter(|tag| *tag <= 999)
            .ok_or_else(|| {
                CodecError::malformed("F3D Design snapshot body-map class tag is not three digits")
            })?
            .to_string();
        for &entity in design_type.entities.values() {
            let Some(&frame_ordinal) = primary_by_entity.get(&entity) else {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design snapshot body-map entity {entity} has no primary record"
                )));
            };
            let frame = frames[frame_ordinal];
            if View::u32_le_at(bytes, frame.start) != Some(snapshot_prefix::TAG_LENGTH_VALUE)
                || bytes.get(
                    frame.start + snapshot_prefix::CLASS_TAG
                        ..frame.start + snapshot_prefix::ENTITY_ID,
                ) != Some(class_tag.as_bytes())
                || View::u64_le_at(bytes, frame.start + snapshot_prefix::ENTITY_ID) != Some(entity)
                || bytes.get(
                    frame.start + snapshot_prefix::RESERVED_ZERO_RUN
                        ..frame.start + snapshot_prefix::COMPANION_REFERENCE,
                ) != Some(snapshot_prefix::RESERVED_ZERO_RUN_VALUE.as_slice())
            {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design snapshot body-map entity {entity} has an invalid entity header"
                )));
            }
            if let Some(record) =
                parse_snapshot_body_map_frame(bytes, meta, frame.start, frame.end, entity)?
            {
                out.push(record);
            }
        }
    }
    Ok(out)
}

fn parse_snapshot_body_map_frame(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    start: usize,
    end: usize,
    entity: u64,
) -> Result<Option<BodyMapRecord>, CodecError> {
    let Some(companion_entity) = entity.checked_add(1) else {
        return Ok(None);
    };
    let Some(companion_at) = start.checked_add(snapshot_prefix::COMPANION_REFERENCE) else {
        return Ok(None);
    };
    let mut invalid_name = false;
    for companion in local_reference_candidates(bytes, companion_at, true) {
        let reserved_count = 2 - companion.padding.trailing_zeros();
        let Some(reserved_end) = companion.end.checked_add(reserved_count) else {
            continue;
        };
        if companion.target != companion_entity
            || !reference_has_type(
                meta,
                &companion,
                crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
            )
            || !bytes
                .get(companion.end..reserved_end)
                .is_some_and(|reserved| reserved.iter().all(|byte| *byte == 0))
        {
            continue;
        }
        let count_at = reserved_end;
        let Some(pair_count) = View::u32_le_at(bytes, count_at) else {
            continue;
        };
        let count = usize::try_from(pair_count)
            .map_err(|_| CodecError::malformed("F3D snapshot body-map count exceeds usize"))?;
        let Some(pairs_start) = count_at.checked_add(4) else {
            continue;
        };
        let Some(pairs_end) = count
            .checked_mul(16)
            .and_then(|span| pairs_start.checked_add(span))
        else {
            continue;
        };
        let Some(pairs) = bytes.get(pairs_start..pairs_end) else {
            continue;
        };
        if pairs.chunks_exact(16).any(|pair| {
            View::u64_le_at(pair, 8).is_none_or(|body_entity| {
                !entity_has_type(
                    meta,
                    body_entity,
                    crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                )
            })
        }) {
            continue;
        }
        for container in local_reference_candidates(bytes, pairs_end, false) {
            let reserved_count = 3 - container.padding.trailing_zeros();
            let Some(reserved_end) = container.end.checked_add(reserved_count) else {
                continue;
            };
            if !reference_has_type(
                meta,
                &container,
                crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            ) || !bytes
                .get(container.end..reserved_end)
                .is_some_and(|reserved| {
                    reserved
                        .iter()
                        .all(|byte| *byte == snapshot_tail::RESERVED_ZERO_VALUE)
                })
            {
                continue;
            }
            let name_at = reserved_end;
            let Some(max_chars) = name_at
                .checked_add(4)
                .and_then(|payload| end.checked_sub(payload))
                .map(|remaining| remaining / 2)
            else {
                invalid_name = true;
                continue;
            };
            let Some((blob_name, name_end)) = lp_utf16_bounded(bytes, name_at, 0..=max_chars)
            else {
                invalid_name = true;
                continue;
            };
            if name_end != end
                || (!blob_name.is_empty()
                    && (!is_brep_blob_basename(&blob_name)
                        || std::path::Path::new(&blob_name).extension()
                            != Some(std::ffi::OsStr::new("smb"))))
                || (blob_name.is_empty() && pair_count != 0)
            {
                continue;
            }
            let mut bindings = Vec::new();

            for (ordinal, pair) in pairs.chunks_exact(16).enumerate() {
                let mut pair = View::over_retained(pair);
                bindings.push(BodyBinding {
                    asm_key: pair.req_u64_le()?,
                    asm_key_offset: pairs_start + ordinal * 16,
                    entity_suffix: pair.req_u64_le()?,
                });
            }
            return Ok(Some(BodyMapRecord {
                blob_name,
                blob_name_offset: name_at + 4,
                bindings,
            }));
        }
    }
    if invalid_name {
        return Err(CodecError::malformed(
            "F3D snapshot body-map basename has invalid framing",
        ));
    }
    Ok(None)
}

/// Parse every exactly indexed BREP body-map record in a Design `BulkStream`.
///
/// The type GUID names a family with more than one record frame. The
/// `MetaStream` entity list and primary record index select exact candidate
/// extents. A candidate is a body map only when one supported reserved-zero
/// width makes its count, pair run, tail, and basename consume that extent.
fn body_map_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyMapRecord>, CodecError> {
    let record_frames = crate::metastream::primary_record_frames(meta, bytes.len())?;

    let mut primary_by_entity = HashMap::<u64, Option<usize>>::new();
    for (ordinal, record) in meta.records.iter().enumerate() {
        primary_by_entity
            .entry(record.entity_id)
            .and_modify(|record_ordinal| *record_ordinal = None)
            .or_insert(Some(ordinal));
    }

    let mut out = Vec::new();
    let mut typed_entities = HashSet::new();
    for (type_ordinal, design_type) in meta.types.iter().enumerate() {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_TYPE_GUID)
        {
            continue;
        }
        if design_type.version != crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION {
            return Err(CodecError::NotImplemented(format!(
                "unsupported F3D Design body-map carrier version {}",
                design_type.version
            )));
        }
        if design_type.module != DESIGN_MODULE_BODY
            || !design_type
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                .is_some_and(|base| {
                    base.eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID)
                })
        {
            return Err(CodecError::Malformed(
                "F3D Design body-map carrier type has incompatible registration metadata".into(),
            ));
        }
        let class_tag = u32::try_from(type_ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(256))
            .filter(|class_tag| *class_tag <= 999)
            .ok_or_else(|| {
                CodecError::Malformed(
                    "F3D Design body-map carrier class tag is not three digits".into(),
                )
            })?;
        let class_tag = class_tag.to_string();

        for &entity_id in design_type.entities.values() {
            if typed_entities.contains(&entity_id) {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design body-map carrier entity {entity_id} is registered more than once"
                )));
            }

            typed_entities.insert(entity_id);
            let record_ordinal = match primary_by_entity.get(&entity_id) {
                Some(Some(record_ordinal)) => *record_ordinal,
                Some(None) => {
                    return Err(crate::design::text::malformed_design(format_args!(
                        "F3D Design body-map carrier entity {entity_id} has multiple primary records"
                    )));
                }
                None => {
                    return Err(crate::design::text::malformed_design(format_args!(
                        "F3D Design body-map carrier entity {entity_id} has no primary record"
                    )));
                }
            };
            let frame = record_frames[record_ordinal];
            let start = frame.start;
            let end = frame.end;
            let record_index = u32::try_from(entity_id).map_err(|_| {
                crate::design::text::malformed_design(format_args!(
                    "F3D Design body-map carrier entity {entity_id} exceeds u32"
                ))
            })?;
            if View::u32_le_at(bytes, start) != Some(3)
                || bytes.get(
                    start + indexed_design_record_header::CLASS_TAG
                        ..start + indexed_design_record_header::RECORD_INDEX,
                ) != Some(class_tag.as_bytes())
                || View::u32_le_at(bytes, start + indexed_design_record_header::RECORD_INDEX)
                    != Some(record_index)
            {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design body-map carrier entity {entity_id} has an invalid indexed header"
                )));
            }

            let mut matched = None;
            for prefix_len in crate::design::body::BODY_MAP_ZERO_PREFIX_LENGTHS {
                let Some(bindings) = parse_body_map_frame(bytes, meta, start, end, prefix_len)?
                else {
                    continue;
                };
                if matched.replace(bindings).is_some() {
                    return Err(crate::design::text::malformed_design(format_args!(
                        "F3D Design body-map carrier entity {entity_id} has an ambiguous frame"
                    )));
                }
            }
            if let Some(record) = matched {
                out.push(record);
            }
        }
    }
    Ok(out)
}

pub(crate) fn body_bindings(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyBinding>, CodecError> {
    let records = body_map_records(bytes, meta)?;

    let mut bindings = Vec::new();
    for record in records {
        bindings.extend(record.bindings);
    }
    Ok(bindings)
}

fn selected_body_map_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyMapRecord>, CodecError> {
    let modern = body_map_records(bytes, meta)?;
    if modern.is_empty() {
        snapshot_body_map_records(bytes, meta)
    } else {
        Ok(modern)
    }
}

pub(crate) struct ModelBodySelection {
    pub(crate) blob_names: Vec<String>,
    pub(crate) explicitly_empty: bool,
}

/// Select model blobs independently in each Design stream.
pub(crate) fn design_model_selection(
    scan: &ContainerScan,
) -> Result<ModelBodySelection, CodecError> {
    let mut archive_counts = HashMap::<String, usize>::new();
    for entry in scan.entries.iter().filter(|entry| {
        scan.belongs_to_design_asset(&entry.name)
            && matches!(entry.role, ContainerRole::BrepSmb | ContainerRole::BrepSmbh)
    }) {
        let basename = entry.name.rsplit('/').next().unwrap_or(&entry.name);
        if let Some(count) = archive_counts.get_mut(basename) {
            *count += 1;
        } else {
            let name = basename.to_owned();

            archive_counts.insert(name, 1);
        }
    }
    let mut model_names = Vec::new();
    let mut carrier_counts = HashMap::<String, usize>::new();
    let mut saw_modern_map = false;
    let mut saw_snapshot_map = false;
    let mut selected_maps_empty = true;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let modern = body_map_records(bytes, &metadata)?;
        saw_modern_map |= !modern.is_empty();
        let mut snapshots = snapshot_body_map_records(bytes, &metadata)?;
        saw_snapshot_map |= !snapshots.is_empty();
        snapshots.retain(|record| {
            !record.bindings.is_empty()
                || record.blob_name.is_empty()
                || archive_counts.contains_key(&record.blob_name)
        });
        for record in modern.iter().chain(&snapshots) {
            if !record.blob_name.is_empty() {
                if let Some(count) = carrier_counts.get_mut(&record.blob_name) {
                    *count += 1;
                } else {
                    let name = record.blob_name.clone();

                    carrier_counts.insert(name, 1);
                }
            }
        }
        let selected = if modern.is_empty() {
            &snapshots
        } else {
            &modern
        };
        selected_maps_empty &= selected
            .iter()
            .all(|record| record.bindings.is_empty() && record.blob_name.is_empty());
        for record in selected
            .iter()
            .filter(|record| !record.blob_name.is_empty())
        {
            let name = record.blob_name.clone();

            model_names.push(name);
        }
    }

    if !saw_modern_map && !saw_snapshot_map && carrier_counts.is_empty() {
        let mut names = Vec::new();
        names.extend(archive_counts.into_keys());
        names.sort();
        return Ok(ModelBodySelection {
            blob_names: names,
            explicitly_empty: false,
        });
    }
    if carrier_counts != archive_counts || archive_counts.values().any(|count| *count != 1) {
        return Err(CodecError::malformed(
            "Design body-map carriers do not classify every binary BREP entry exactly once",
        ));
    }
    model_names.sort();
    model_names.dedup();
    Ok(ModelBodySelection {
        blob_names: model_names,
        explicitly_empty: saw_modern_map && selected_maps_empty,
    })
}

fn parse_body_map_frame(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    start: usize,
    end: usize,
    prefix_len: usize,
) -> Result<Option<BodyMapRecord>, CodecError> {
    let Some(count_at) = start
        .checked_add(indexed_design_record_header::LEN)
        .and_then(|payload| payload.checked_add(prefix_len))
    else {
        return Ok(None);
    };
    if !bytes
        .get(start + indexed_design_record_header::LEN..count_at)
        .is_some_and(|prefix| prefix.iter().all(|byte| *byte == 0))
    {
        return Ok(None);
    }
    let Some(pair_count) = View::u32_le_at(bytes, count_at) else {
        return Ok(None);
    };
    let count = usize::try_from(pair_count).map_err(|_| {
        crate::design::text::malformed_design(format_args!(
            "F3D Design body map at byte {start} pair count does not fit this platform"
        ))
    })?;
    let Some(pairs_start) = count_at.checked_add(4) else {
        return Ok(None);
    };
    let Some(pairs_end) = count
        .checked_mul(16)
        .and_then(|span| pairs_start.checked_add(span))
    else {
        return Ok(None);
    };
    let decode_name = |name_at: usize| -> Result<Option<(usize, String)>, CodecError> {
        let Some(max_name_chars) = name_at
            .checked_add(4)
            .and_then(|payload| end.checked_sub(payload))
            .map(|remaining| remaining / 2)
        else {
            return Ok(None);
        };
        let Some((blob_name, name_end)) = lp_utf16_bounded(bytes, name_at, 0..=max_name_chars)
        else {
            return Ok(None);
        };
        Ok((name_end == end
            && ((pair_count == 0 && blob_name.is_empty())
                || (pair_count > 0
                    && is_brep_blob_basename(&blob_name)
                    && std::path::Path::new(&blob_name).extension()
                        == Some(std::ffi::OsStr::new("smbh")))))
        .then_some((name_at, blob_name)))
    };
    let mut typed_name = None;
    for reference in local_reference_candidates(bytes, pairs_end, true) {
        if !reference_has_type(
            meta,
            &reference,
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
        ) {
            continue;
        }
        if let Some(name) = decode_name(reference.end)? {
            if typed_name.replace(name).is_some() {
                return Err(CodecError::malformed(
                    "F3D Design body-map frame has ambiguous typed reference tails",
                ));
            }
        }
    }
    let fixed_name = if typed_name.is_none()
        && View::u64_le_at(bytes, pairs_end).is_some()
        && View::u32_le_at(bytes, pairs_end + 8) == Some(0)
    {
        match pairs_end.checked_add(12) {
            Some(name_at) => decode_name(name_at)?,
            None => None,
        }
    } else {
        None
    };
    let Some((name_at, blob_name)) = typed_name.or(fixed_name) else {
        return Ok(None);
    };

    let mut bindings = Vec::new();

    for pair in 0..count {
        let at = pairs_start + pair * 16;
        let (Some(key), Some(suffix)) =
            (View::u64_le_at(bytes, at), View::u64_le_at(bytes, at + 8))
        else {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D Design body map at byte {start} has a truncated pair run"
            )));
        };
        bindings.push(BodyBinding {
            asm_key: key,
            asm_key_offset: at,
            entity_suffix: suffix,
        });
    }
    Ok(Some(BodyMapRecord {
        blob_name,
        blob_name_offset: name_at + 4,
        bindings,
    }))
}

fn is_brep_blob_basename(value: &str) -> bool {
    let extension = value.rsplit_once('.').map(|(_, extension)| extension);
    value.starts_with("BREP.")
        && matches!(extension, Some("smb" | "smbh"))
        && !value.contains(['/', '\\'])
}

/// Decode every ordered Design BREP body-map pair and resolve each pair in its
/// named blob's body-selector namespace.
pub(crate) fn decode_design_body_bindings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    active_brep_entry: Option<&str>,
    body_keys: &[BodyNativeKey],
) -> Result<Vec<DesignBodyBinding>, CodecError> {
    let naming_spaces = super::meta::decode_component_naming_spaces(scan)?;
    let components = super::components::decode_local_components(scan, &naming_spaces.spaces)?;
    let active_basename = active_brep_entry.and_then(|entry| entry.rsplit('/').next());
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let memberships = super::component_bodies::owners(ctx, bytes, &metadata)?;
        let hidden_by_entity = typed_browser_node_hidden_flags(bytes, &metadata)?;
        let consumed = match scan.entry_view(&entry.name) {
            Some(root) => super::body_consumers::consumed_body_entities(root, &metadata)?,
            None => std::collections::HashSet::new(),
        };
        for record in selected_body_map_records(bytes, &metadata)? {
            let pair_count = u32::try_from(record.bindings.len())
                .map_err(|_| CodecError::malformed("F3D Design body map exceeds u32::MAX pairs"))?;
            let mut source_bodies = Vec::new();
            if pair_count != 0 {
                for key in body_keys.iter().filter(|key| {
                    key.source_brep.as_deref().map_or_else(
                        || active_basename == Some(record.blob_name.as_str()),
                        |source| source == record.blob_name,
                    )
                }) {
                    source_bodies.push(key);
                }
            }
            for (ordinal, binding) in (0..pair_count).zip(&record.bindings) {
                let owner = memberships.owners.get(&binding.entity_suffix);
                let cluster_owner = components
                    .get(&crate::ids::native_scope(&entry.name))
                    .and_then(|components| components.cluster_owner(binding.entity_suffix));
                let hidden = hidden_by_entity
                    .get(&binding.entity_suffix)
                    .is_some_and(|node| node.hidden);
                let history_only = memberships.complete
                    && owner.is_none()
                    && (consumed.contains(&binding.entity_suffix)
                        || (hidden && cluster_owner.is_none()));
                let body = if history_only {
                    None
                } else {
                    crate::brep::resolve_body_selector(
                        source_bodies.iter().copied(),
                        binding.asm_key,
                    )?
                };
                let id = design_record_id(
                    &entry.name,
                    ":design-body-binding#",
                    u64_from_index(binding.asm_key_offset),
                );
                let record =
                    DesignBodyBinding::try_from(crate::records::bodies::DesignBodyBindingWire {
                        id,
                        stream: entry.name.clone(),
                        pair_count,
                        pair_ordinal: ordinal,
                        asm_body_key: binding.asm_key,
                        asm_body_key_offset: u64_from_index(binding.asm_key_offset),
                        entity_suffix: binding.entity_suffix,
                        entity_suffix_offset: u64_from_index(binding.entity_suffix_offset()),
                        blob_name: record.blob_name.clone(),
                        blob_name_offset: u64_from_index(record.blob_name_offset),
                        body: body.cloned(),
                        component_owner: owner
                            .and_then(super::component_bodies::OwnerResolution::admitted)
                            .cloned(),
                        component_owner_unresolved: owner
                            .is_some_and(|owner| owner.admitted().is_none()),
                        history_only,
                    })
                    .map_err(CodecError::Malformed)?;

                out.push(record);
            }
        }
    }
    out.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(out)
}

/// Bind each body cache to every BREP map pair carrying the same Design entity
/// suffix in the same stream.
pub(crate) fn bind_body_bounds(
    bounds: &mut [DesignBodyBounds],
    bindings: &[DesignBodyBinding],
) -> Result<(), CodecError> {
    for bounds in bounds {
        let Some(stream) = native_stream(bounds.id()) else {
            continue;
        };
        let mut matches = Vec::new();
        for binding in bindings {
            if binding.entity_suffix != bounds.entity_suffix()
                || stream != crate::ids::native_scope(binding.stream())
            {
                continue;
            }
            matches.push(binding);
        }
        matches.sort_by_key(|binding| binding.asm_body_key_offset());
        let mut ids = Vec::new();
        for binding in matches {
            ids.push(
                crate::records::bodies::DesignBodyBindingId::try_from(binding.id().to_owned())
                    .map_err(CodecError::Malformed)?,
            );
        }
        bounds.set_body_binding_ids(ids);
    }
    Ok(())
}

/// Decode per-body display visibility from the Design `BulkStream`.
///
/// Each BREP body-map record resolves blob-qualified body selectors to Design
/// entity suffixes, and each entity's browser-node record carries a hidden flag
/// directly after the node GUID
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
/// The result maps each blob and body selector to its display visibility;
/// bodies without records are absent.
#[derive(Debug, Clone)]
pub(crate) struct DecodedBodyVisibility {
    pub(crate) stream: String,
    pub(crate) byte_offset: u64,
    pub(crate) asm_body_key_offset: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) visible: bool,
}

pub(crate) fn decode_all_body_visibility(
    scan: &ContainerScan,
) -> Result<HashMap<(String, u64), DecodedBodyVisibility>, CodecError> {
    let mut out = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let hidden_by_entity = typed_browser_node_hidden_flags(bytes, &metadata)?;
        for record in selected_body_map_records(bytes, &metadata)? {
            for binding in record.bindings {
                let Some(node) = hidden_by_entity.get(&binding.entity_suffix) else {
                    continue;
                };
                let key = record.blob_name.clone();
                let stream = entry.name.clone();
                out.insert(
                    (key, binding.asm_key),
                    DecodedBodyVisibility {
                        stream,
                        byte_offset: node.byte_offset,
                        asm_body_key_offset: u64_from_index(binding.asm_key_offset),
                        entity_suffix: binding.entity_suffix,
                        visible: !node.hidden,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// Visibility selected from one typed browser-node record.
#[derive(Debug, Clone, Copy)]
struct BrowserNodeVisibility {
    byte_offset: u64,
    hidden: bool,
}

fn typed_browser_node_hidden_flags(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<HashMap<u64, BrowserNodeVisibility>, CodecError> {
    let nodes = crate::design::decode::presentation::browser_node_records(bytes, meta)?;
    let presentations = crate::design::decode::presentation::body_presentations(bytes, meta)?;
    let mut nodes_by_entity = HashMap::<u64, Vec<_>>::new();
    for node in &nodes {
        nodes_by_entity
            .entry(node.entity_suffix)
            .or_default()
            .push(node);
    }

    let mut out = HashMap::new();
    for (entity_suffix, candidates) in nodes_by_entity {
        let mut linked = Vec::new();
        for node in presentations
            .iter()
            .filter(|presentation| presentation.entity_suffix == entity_suffix)
            .filter_map(|presentation| presentation.browser_node.as_ref())
        {
            linked.push(node);
        }
        linked.sort_by(|left, right| {
            let left_key = {
                let node = left;
                node.record_index
            };
            let right_key = {
                let node = right;
                node.record_index
            };
            left_key.cmp(&right_key)
        });
        linked.dedup_by_key(|node| node.record_index);
        let selected = match linked.as_slice() {
            [node] => Some(*node),
            [] => match candidates.as_slice() {
                [node] => Some(*node),
                _ => None,
            },
            _ => None,
        };
        if let Some(node) = selected {
            out.insert(
                entity_suffix,
                BrowserNodeVisibility {
                    byte_offset: node.hidden_offset,
                    hidden: node.hidden,
                },
            );
        }
    }
    Ok(out)
}

/// Map each browser-node GUID to its Design entity suffix.
///
/// The GUID is the stable join between browser presentation records; the
/// adjacent entity suffix joins the node back to the Design body map.
pub(crate) fn scanned_browser_node_entities(bytes: &[u8]) -> HashMap<String, u64> {
    let mut entities = HashMap::new();
    let mut ambiguous = std::collections::HashSet::new();
    for record in scan_browser_node_identities(bytes) {
        let key = record.guid.to_ascii_lowercase();
        if let Some(previous) = entities.get(&key) {
            if *previous != record.entity_suffix && !ambiguous.contains(&key) {
                ambiguous.insert(key);
            }
        } else {
            entities.insert(key, record.entity_suffix);
        }
    }
    entities.retain(|guid, _| !ambiguous.contains(guid));
    entities
}

#[derive(Debug)]
struct ScannedBrowserNodeIdentity {
    guid: String,
    entity_suffix: u64,
}

fn scan_browser_node_identities(bytes: &[u8]) -> Vec<ScannedBrowserNodeIdentity> {
    const GUID_CHARS: usize = 36;
    const GUID_CHARS_U32: u32 = 36;
    const GUID_BYTES: usize = GUID_CHARS * 2;
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 4 + GUID_BYTES + 3 + 8 <= bytes.len() {
        if View::u32_le_at(bytes, at) != Some(GUID_CHARS_U32)
            || !is_utf16_guid(&bytes[at + 4..at + 4 + GUID_BYTES])
        {
            at += 1;
            continue;
        }
        let flag_at = at + 4 + GUID_BYTES;
        if bytes.get(flag_at + 1..flag_at + 3) == Some(&[0x01, 0x01]) {
            if let (0 | 1, Some(member)) = (bytes[flag_at], View::u64_le_at(bytes, flag_at + 3)) {
                let mut guid = String::new();
                for pair in bytes[at + 4..at + 4 + GUID_BYTES].chunks_exact(2) {
                    guid.push(char::from(pair[0]));
                }
                out.push(ScannedBrowserNodeIdentity {
                    guid,
                    entity_suffix: member,
                });
            }
        }
        at += 1;
    }
    out
}

fn is_utf16_guid(bytes: &[u8]) -> bool {
    bytes
        .chunks_exact(2)
        .all(|pair| pair[1] == 0 && (pair[0].is_ascii_hexdigit() || pair[0] == b'-'))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use std::io::{Cursor, Write};

    use zip::CompressionMethod;

    use super::{
        body_bindings, body_bound_candidates, parse_body_map_frame, snapshot_body_map_records,
        typed_browser_node_hidden_flags,
    };
    use crate::bytes::lp_utf16_bytes;
    use crate::bytes::take_reference;
    use crate::design::presentation::{
        APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_BASE_TYPE_GUID, BODY_PRESENTATION_TYPE_GUID,
        BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
        BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_BASE_TYPE_GUID,
        BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION, PHYSICAL_MATERIAL_LIBRARY_ID,
    };
    use crate::design::test_support::{design_type, primary_record};
    use crate::records::entity_header::DESIGN_MODULE_BODY;
    use crate::records::entity_header::DESIGN_MODULE_FUSION;
    use crate::test_support::indexed_header;
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::push_reference_u64;
    use crate::test_support::zip_test::with_scan;

    #[test]
    fn body_members_decode_one_member_identity() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut bulk = Vec::new();
        bulk.extend_from_slice(&10_u32.to_le_bytes());
        bulk.extend_from_slice(b"BodiesRoot");
        bulk.extend_from_slice(&0_u16.to_le_bytes());
        bulk.extend_from_slice(&10_u32.to_le_bytes());
        bulk.extend_from_slice(b"BodiesRoot");
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        let member_offset = bulk.len();
        bulk.push(1);
        bulk.extend_from_slice(&9_u64.to_le_bytes());
        bulk.extend_from_slice(&2_u16.to_le_bytes());
        bulk.push(0);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(&bulk).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            let members = super::decode_body_members(scan).unwrap();
            assert_eq!(members.len(), 1);
            assert_eq!(
                members[0].id(),
                &crate::ids::native_design_body_member_id(ENTRY, member_offset)
            );
        });
    }

    fn push_entity_header(out: &mut Vec<u8>, class_tag: &str, entity: u64) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend(
            lp_utf16_bytes(&format!("0_{entity}"))
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
    }

    fn body_map_bytes(prefix_len: usize, declared_count: u32, pairs: &[(u64, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        indexed_header(&mut out, *b"256", 900);
        out.extend(std::iter::repeat_n(0, prefix_len));
        out.extend_from_slice(&declared_count.to_le_bytes());
        for (key, suffix) in pairs {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&suffix.to_le_bytes());
        }
        out.extend_from_slice(&1793u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend(
            lp_utf16_bytes(if declared_count == 0 {
                ""
            } else {
                "BREP.synthetic.smbh"
            })
            .expect("fixture UTF-16 code-unit count fits u32"),
        );
        out
    }

    fn body_map_bytes_with_typed_tail(prefix_len: usize, pairs: &[(u64, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        indexed_header(&mut out, *b"256", 900);
        out.extend(std::iter::repeat_n(0, prefix_len));
        out.extend_from_slice(
            &(u32::try_from(pairs.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for (key, suffix) in pairs {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&suffix.to_le_bytes());
        }
        push_snapshot_reference(
            &mut out,
            700,
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            1,
        );
        out.push(0);
        out.extend(
            lp_utf16_bytes("BREP.synthetic.smbh").expect("fixture UTF-16 code-unit count fits u32"),
        );
        out
    }

    fn body_map_metadata() -> crate::metastream::MetaStream {
        crate::metastream::MetaStream {
            types: vec![
                crate::records::entity_header::SegmentTypeData {
                    byte_offset: 0,
                    type_guid: crate::design::body::BODY_MAP_CARRIER_TYPE_GUID
                        .to_owned()
                        .try_into()
                        .expect("type GUID"),
                    type_guid_offset: 0,
                    base_type_guid: crate::records::entity_header::BaseTypeGuid::Guid {
                        value: crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID
                            .to_owned()
                            .try_into()
                            .expect("base GUID"),
                        offset: 0,
                    },
                    version: crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                    version_offset: 0,
                    module: DESIGN_MODULE_BODY.into(),
                    entities: crate::records::identity::ReferenceRun::located(vec![
                        crate::records::identity::Located {
                            value: 900,
                            offset: 0,
                        },
                    ]),
                },
                design_type(
                    crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![700],
                ),
            ],
            records: vec![crate::metastream::RecordIndexEntry {
                entity_id: 900,
                bulk_offset: 0,
            }],
            secondary_records: Vec::new(),
        }
    }

    fn push_snapshot_reference(out: &mut Vec<u8>, target: u64, target_type: &str, form: u8) {
        match form {
            1 => {
                out.push(1);
                out.extend_from_slice(&target.to_le_bytes());
                out.extend_from_slice(
                    &(u32::try_from(target_type.len()).expect("fixture value fits u32"))
                        .to_le_bytes(),
                );
                out.extend_from_slice(target_type.as_bytes());
                out.extend_from_slice(&[0, 0]);
            }
            2 => {
                out.extend_from_slice(&[1, 1]);
                out.extend_from_slice(&target.to_le_bytes());
            }
            // Form 0 and every other stated form write the plain reference.
            _ => push_reference_u64(out, target),
        }
    }

    fn snapshot_body_map_bytes_with(
        form: u8,
        pair_count: u32,
        blob_name: &str,
        companion_type: &str,
    ) -> Vec<u8> {
        let entity = 900u64;
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"256");
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        push_snapshot_reference(&mut out, entity + 1, companion_type, form);
        if form == 2 {
            out.extend_from_slice(&[0, 0]);
        }
        out.extend_from_slice(&pair_count.to_le_bytes());
        if pair_count != 0 {
            out.extend_from_slice(&7u64.to_le_bytes());
            out.extend_from_slice(&500u64.to_le_bytes());
        }
        push_snapshot_reference(
            &mut out,
            700,
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            form,
        );
        if form == 2 {
            out.extend_from_slice(&[0, 0, 0]);
        } else {
            out.push(0);
        }
        out.extend(lp_utf16_bytes(blob_name).expect("fixture UTF-16 code-unit count fits u32"));
        out
    }

    fn snapshot_body_map_bytes(form: u8) -> Vec<u8> {
        snapshot_body_map_bytes_with(
            form,
            1,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        )
    }

    fn snapshot_body_map_metadata() -> crate::metastream::MetaStream {
        crate::metastream::MetaStream {
            types: vec![
                design_type(
                    crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID,
                    Some(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID),
                    1,
                    DESIGN_MODULE_BODY,
                    vec![900],
                ),
                design_type(
                    crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![901],
                ),
                design_type(
                    crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![500],
                ),
                design_type(
                    crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![700],
                ),
            ],
            records: vec![primary_record(900, 0)],
            secondary_records: Vec::new(),
        }
    }

    fn snapshot_selection_archive(
        carriers: &[(&str, u32)],
        archive_names: &[&str],
        truncate_last: bool,
    ) -> Vec<u8> {
        let mut bulk = Vec::new();
        let mut records = Vec::new();
        let mut entities = Vec::new();
        let mut companions = Vec::new();
        for (ordinal, (name, pair_count)) in carriers.iter().enumerate() {
            let entity = 900 + u64_from_index(ordinal) * 10;
            records.push((entity, u64_from_index(bulk.len())));
            entities.push(entity);
            companions.push(entity + 1);
            let mut frame = snapshot_body_map_bytes_with(
                0,
                *pair_count,
                name,
                crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
            );
            frame[7..15].copy_from_slice(&entity.to_le_bytes());
            frame[22..30].copy_from_slice(&(entity + 1).to_le_bytes());
            bulk.extend(frame);
        }
        if truncate_last {
            bulk.pop();
        }
        let meta = crate::test_support::streams_test::design_metastream_with_records(
            &[
                (
                    crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID,
                    crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                    1,
                    DESIGN_MODULE_BODY,
                    &entities,
                ),
                (
                    crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
                    "",
                    0,
                    DESIGN_MODULE_BODY,
                    &companions,
                ),
                (
                    BODY_PRESENTATION_TYPE_GUID,
                    "",
                    0,
                    DESIGN_MODULE_BODY,
                    &[500],
                ),
                (BREP_CONTAINER_TYPE_GUID, "", 0, DESIGN_MODULE_BODY, &[700]),
            ],
            &records,
        );
        let mut names = archive_names.to_vec();
        names.sort_unstable();
        names.dedup();
        let breps: Vec<_> = names.into_iter().map(|name| (name, &[0][..])).collect();
        crate::test_support::body_map_test::f3d_with_breps_and_design(&breps, &bulk, &meta)
    }

    #[test]
    fn snapshot_selection_classifies_each_archive_basename_once() {
        let archive = snapshot_selection_archive(
            &[("BREP.first.smb", 1), ("BREP.second.smb", 1)],
            &["BREP.first.smb", "BREP.second.smb"],
            false,
        );
        with_scan(&archive, |scan| {
            let selection = super::design_model_selection(scan).unwrap();
            assert_eq!(selection.blob_names, ["BREP.first.smb", "BREP.second.smb"]);
            assert!(!selection.explicitly_empty);
        });
    }

    #[test]
    fn snapshot_selection_admits_an_absent_zero_pair_blob() {
        let archive = snapshot_selection_archive(
            &[("BREP.first.smb", 1), ("BREP.empty.smb", 0)],
            &["BREP.first.smb"],
            false,
        );
        with_scan(&archive, |scan| {
            assert_eq!(
                super::design_model_selection(scan).unwrap().blob_names,
                ["BREP.first.smb"]
            );
        });
    }

    #[test]
    fn snapshot_selection_retains_a_present_zero_pair_blob() {
        let archive =
            snapshot_selection_archive(&[("BREP.empty.smb", 0)], &["BREP.empty.smb"], false);
        with_scan(&archive, |scan| {
            assert_eq!(
                super::design_model_selection(scan).unwrap().blob_names,
                ["BREP.empty.smb"]
            );
        });
    }

    #[test]
    fn snapshot_selection_refuses_an_absent_populated_blob() {
        let archive = snapshot_selection_archive(&[("BREP.absent.smb", 1)], &[], false);
        with_scan(&archive, |scan| {
            assert!(matches!(
                super::design_model_selection(scan),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn snapshot_selection_refuses_an_unclassified_archive_blob() {
        let archive = snapshot_selection_archive(
            &[("BREP.absent.smb", 0)],
            &["BREP.unclassified.smb"],
            false,
        );
        with_scan(&archive, |scan| {
            assert!(matches!(
                super::design_model_selection(scan),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn snapshot_selection_refuses_two_carriers_for_one_archive_basename() {
        let archive = snapshot_selection_archive(
            &[("BREP.synthetic.smb", 1); 2],
            &["BREP.synthetic.smb"],
            false,
        );
        with_scan(&archive, |scan| {
            assert!(matches!(
                super::design_model_selection(scan),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn snapshot_selection_refuses_matching_duplicate_carrier_and_archive_counts() {
        let archive = snapshot_selection_archive(
            &[("BREP.synthetic.smb", 1); 2],
            &["BREP.synthetic.smb", "other/BREP.synthetic.smb"],
            false,
        );
        with_scan(&archive, |scan| {
            assert!(matches!(
                super::design_model_selection(scan),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn snapshot_selection_refuses_a_zero_pair_duplicate_of_a_present_blob() {
        let archive = snapshot_selection_archive(
            &[("BREP.synthetic.smb", 1), ("BREP.synthetic.smb", 0)],
            &["BREP.synthetic.smb"],
            false,
        );
        with_scan(&archive, |scan| {
            assert!(matches!(
                super::design_model_selection(scan),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn snapshot_selection_refuses_a_truncated_absent_zero_pair_carrier() {
        let archive = snapshot_selection_archive(&[("BREP.absent.smb", 0)], &[], true);
        with_scan(&archive, |scan| {
            assert!(matches!(
                super::design_model_selection(scan),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn snapshot_selection_refuses_a_truncated_carrier() {
        let archive =
            snapshot_selection_archive(&[("BREP.synthetic.smb", 1)], &["BREP.synthetic.smb"], true);
        with_scan(&archive, |scan| {
            assert!(matches!(
                super::design_model_selection(scan),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn snapshot_body_map_accepts_every_reference_envelope() {
        for form in 0..=2 {
            let records = snapshot_body_map_records(
                &snapshot_body_map_bytes(form),
                &snapshot_body_map_metadata(),
            )
            .expect("typed snapshot body map");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
            assert_eq!(records[0].bindings.len(), 1);
            assert_eq!(records[0].bindings[0].asm_key, 7);
            assert_eq!(records[0].bindings[0].entity_suffix, 500);
        }
    }

    #[test]
    fn snapshot_body_map_accepts_three_zero_companion_variant() {
        let mut bytes = snapshot_body_map_bytes(0);
        let mut companion_end = 4 + 3 + 8 + 6;
        take_reference(&bytes, &mut companion_end).expect("ordinary companion reference");
        bytes.insert(companion_end, 0);

        let records = snapshot_body_map_records(&bytes, &snapshot_body_map_metadata())
            .expect("three-zero companion variant");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert_eq!(records[0].bindings.len(), 1);
    }

    #[test]
    fn snapshot_body_map_accepts_padded_doubled_companion() {
        let mut bytes = snapshot_body_map_bytes(2);
        let companion_end = 4 + 3 + 8 + 6 + 2 + 8;
        bytes.insert(companion_end, 0);

        let records = snapshot_body_map_records(&bytes, &snapshot_body_map_metadata())
            .expect("padded doubled companion");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert_eq!(records[0].bindings.len(), 1);
    }

    #[test]
    fn snapshot_body_map_requires_typed_pair_targets() {
        let mut metadata = snapshot_body_map_metadata();
        metadata.types[2].entities = crate::records::identity::ReferenceRun::unlocated(Vec::new());
        assert!(
            snapshot_body_map_records(&snapshot_body_map_bytes(0), &metadata)
                .expect("mixed carrier family")
                .is_empty()
        );
    }

    #[test]
    fn snapshot_body_map_retains_named_zero_pair_blob() {
        let bytes = snapshot_body_map_bytes_with(
            0,
            0,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        );
        let records = snapshot_body_map_records(&bytes, &snapshot_body_map_metadata())
            .expect("named zero-pair snapshot body map");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert!(records[0].bindings.is_empty());
    }

    #[test]
    fn snapshot_body_map_accepts_empty_zero_pair_record() {
        let bytes = snapshot_body_map_bytes_with(
            0,
            0,
            "",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        );
        let records = snapshot_body_map_records(&bytes, &snapshot_body_map_metadata())
            .expect("empty zero-pair snapshot body map");
        assert_eq!(records.len(), 1);
        assert!(records[0].blob_name.is_empty());
        assert!(records[0].bindings.is_empty());
    }

    #[test]
    fn snapshot_body_map_rejects_contradictory_inline_type() {
        let bytes = snapshot_body_map_bytes_with(
            1,
            1,
            "BREP.snapshot.smb",
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
        );
        assert!(
            snapshot_body_map_records(&bytes, &snapshot_body_map_metadata())
                .expect("mixed carrier family")
                .is_empty()
        );
    }

    #[test]
    fn body_map_count_is_bounded_by_the_stream_not_sixty_four_pairs() {
        let pairs = (0u64..65)
            .map(|ordinal| (1000 + ordinal, (1u64 << 40) + ordinal))
            .collect::<Vec<_>>();
        let bindings = body_bindings(&body_map_bytes(10, 65, &pairs), &body_map_metadata())
            .expect("65-pair body map");
        assert_eq!(bindings.len(), 65);
        assert_eq!(bindings[0].asm_key, 1000);
        assert_eq!(bindings[64].asm_key, 1064);
        assert_eq!(bindings[64].entity_suffix, (1u64 << 40) + 64);
    }

    #[test]
    fn body_map_accepts_typed_container_reference_tail() {
        let bytes = body_map_bytes_with_typed_tail(10, &[(2291, 7492), (2292, 7534)]);
        let bindings = body_bindings(&bytes, &body_map_metadata()).expect("typed reference tail");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].asm_key, 2291);
        assert_eq!(bindings[0].entity_suffix, 7492);
        assert_eq!(bindings[1].asm_key, 2292);
        assert_eq!(bindings[1].entity_suffix, 7534);
    }

    #[test]
    fn both_empty_body_map_prefixes_have_no_pairs_or_brep_basename() {
        for prefix_len in crate::design::body::BODY_MAP_ZERO_PREFIX_LENGTHS {
            let bytes = body_map_bytes(prefix_len, 0, &[]);
            let frame =
                parse_body_map_frame(&bytes, &body_map_metadata(), 0, bytes.len(), prefix_len)
                    .expect("empty body-map frame")
                    .expect("supported empty body-map variant");
            assert!(frame.bindings.is_empty());
            assert!(body_bindings(&bytes, &body_map_metadata())
                .expect("empty typed body map")
                .is_empty());
        }
    }

    #[test]
    fn empty_modern_map_without_any_brep_is_an_explicit_empty_selection() {
        let bulk = body_map_bytes(10, 0, &[]);
        let meta = crate::test_support::streams_test::design_metastream_with_records(
            &[(
                crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                DESIGN_MODULE_BODY,
                &[900],
            )],
            &[(900, 0)],
        );
        let archive =
            crate::test_support::body_map_test::f3d_with_breps_and_design(&[], &bulk, &meta);
        with_scan(&archive, |scan| {
            let selection = super::design_model_selection(scan).unwrap();
            assert!(selection.explicitly_empty);
            assert!(selection.blob_names.is_empty());
        });
    }

    #[test]
    fn body_map_header_prevents_a_high_word_count_alias() {
        let bytes = body_map_bytes(10, 2, &[(10, (1u64 << 32) + 77), (20, 30)]);
        let bindings =
            body_bindings(&bytes, &body_map_metadata()).expect("typed two-pair body map");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].entity_suffix, (1u64 << 32) + 77);
        assert_eq!(bindings[1].asm_key, 20);
    }

    #[test]
    fn truncated_body_map_frame_is_not_decoded() {
        let bytes = body_map_bytes(10, 2, &[(10, 20)]);
        assert!(body_bindings(&bytes, &body_map_metadata())
            .expect("typed carrier record")
            .is_empty());
    }

    #[test]
    fn body_map_parser_does_not_scan_an_unindexed_nested_header() {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"256", 900);
        bytes.extend_from_slice(&[0xff; 4]);
        bytes.extend(body_map_bytes(10, 1, &[(10, 20)]));

        assert!(body_bindings(&bytes, &body_map_metadata())
            .expect("outer typed carrier record")
            .is_empty());
    }

    #[test]
    fn composed_body_map_outputs_decode_visibility_names_and_bindings() {
        const PREFIX: &str = "FusionAssetName[Active]/Design1/";
        const BREP: &str = "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh";
        let mut bulk = body_map_bytes(10, 1, &[(7, 42)]);
        let browser_offset = bulk.len();
        push_browser_node(
            &mut bulk,
            100,
            "11111111-2222-8333-A444-555555555555",
            false,
            42,
        );
        let metadata = crate::test_support::streams_test::design_metastream_with_records(
            &[
                (
                    crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                    crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                    crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    &[900],
                ),
                (
                    BROWSER_NODE_TYPE_GUID,
                    BROWSER_NODE_BASE_TYPE_GUID,
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    &[100],
                ),
            ],
            &[(900, 0), (100, u64_from_index(browser_offset))],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(format!("{PREFIX}BulkStream.dat"), stored)
            .unwrap();
        zip.write_all(&bulk).unwrap();
        zip.start_file(format!("{PREFIX}MetaStream.dat"), stored)
            .unwrap();
        zip.write_all(&metadata).unwrap();
        zip.start_file(BREP, stored).unwrap();
        zip.write_all(&[0]).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            let visibility = super::decode_all_body_visibility(scan).unwrap();
            assert!(
                visibility
                    .get(&("BREP.synthetic.smbh".to_owned(), 7))
                    .unwrap()
                    .visible
            );
            let names = super::design_model_selection(scan).unwrap().blob_names;
            assert_eq!(names, ["BREP.synthetic.smbh"]);
            let bindings = super::decode_design_body_bindings(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                None,
                &[],
            )
            .unwrap();
            assert_eq!(bindings.len(), 1);
        });
    }

    fn bindings_with_consumer(
        pairs: &[(u64, u64, Option<bool>)],
        container: crate::test_support::component_body_test::HistoryOnlyContainer<'_>,
        bound_cluster: bool,
        consumer: Option<crate::test_support::component_body_test::BodyConsumer<'_>>,
    ) -> Vec<(u64, bool)> {
        const PREFIX: &str = "FusionAssetName[Active]/Design1/";
        const BLOB: &str = "BREP.synthetic.smbh";
        let (bulk, metadata) =
            crate::test_support::component_body_test::history_only_pairs_with_consumer(
                BLOB,
                pairs,
                container,
                bound_cluster,
                consumer,
            );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(format!("{PREFIX}BulkStream.dat"), stored)
            .unwrap();
        zip.write_all(&bulk).unwrap();
        zip.start_file(format!("{PREFIX}MetaStream.dat"), stored)
            .unwrap();
        zip.write_all(&metadata).unwrap();
        zip.start_file(
            format!("FusionAssetName[Active]/Breps.BlobParts/{BLOB}"),
            stored,
        )
        .unwrap();
        zip.write_all(&[0]).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let mut out = Vec::new();
        with_scan(&archive, |scan| {
            out = super::decode_design_body_bindings(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                None,
                &[],
            )
            .unwrap()
            .iter()
            .map(|binding| (binding.entity_suffix, binding.history_only))
            .collect();
        });
        out.sort_unstable();
        out
    }

    #[test]
    fn a_consuming_scope_removes_a_hidden_uncontained_pair_from_a_bound_cluster() {
        use crate::design::decode::body_consumers::{COMBINE_TYPE_GUID, REMOVE_BODY_TYPE_GUID};
        use crate::test_support::component_body_test::{
            BodyConsumer, HistoryOnlyContainer as Container, BODY_CONSUMER_SCOPE,
        };
        const MOVE_TYPE_GUID: &str = "64E40C8E-E564-469C-8690-5C4E77C1B02C";
        let consumer = |scope_type_guid| BodyConsumer {
            scope_type_guid,
            entity: 350,
            hidden: 1,
            flag: 1,
            selection_owner: Some(BODY_CONSUMER_SCOPE),
            registered_node: true,
        };
        for (cluster, container, row, selected, expected) in [
            (
                true,
                Container::Lists(&[]),
                None,
                Some(consumer(COMBINE_TYPE_GUID)),
                true,
            ),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(consumer(REMOVE_BODY_TYPE_GUID)),
                true,
            ),
            (
                false,
                Container::Lists(&[]),
                None,
                Some(consumer(COMBINE_TYPE_GUID)),
                true,
            ),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(BodyConsumer {
                    flag: 0,
                    ..consumer(COMBINE_TYPE_GUID)
                }),
                true,
            ),
            (
                false,
                Container::Lists(&[]),
                None,
                Some(BodyConsumer {
                    flag: 0,
                    ..consumer(REMOVE_BODY_TYPE_GUID)
                }),
                true,
            ),
            (true, Container::Lists(&[]), Some(true), None, false),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(consumer(MOVE_TYPE_GUID)),
                false,
            ),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(BodyConsumer {
                    hidden: 0,
                    ..consumer(COMBINE_TYPE_GUID)
                }),
                false,
            ),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(BodyConsumer {
                    flag: 2,
                    ..consumer(COMBINE_TYPE_GUID)
                }),
                false,
            ),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(BodyConsumer {
                    entity: 351,
                    ..consumer(COMBINE_TYPE_GUID)
                }),
                false,
            ),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(BodyConsumer {
                    selection_owner: None,
                    ..consumer(COMBINE_TYPE_GUID)
                }),
                false,
            ),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(BodyConsumer {
                    selection_owner: Some(BODY_CONSUMER_SCOPE + 1),
                    ..consumer(COMBINE_TYPE_GUID)
                }),
                false,
            ),
            (
                true,
                Container::Lists(&[]),
                None,
                Some(BodyConsumer {
                    registered_node: false,
                    ..consumer(COMBINE_TYPE_GUID)
                }),
                false,
            ),
            (
                true,
                Container::Lists(&[350]),
                None,
                Some(consumer(COMBINE_TYPE_GUID)),
                false,
            ),
            (
                true,
                Container::Malformed,
                None,
                Some(consumer(COMBINE_TYPE_GUID)),
                false,
            ),
            (
                true,
                Container::Absent,
                None,
                Some(consumer(COMBINE_TYPE_GUID)),
                false,
            ),
        ] {
            assert_eq!(
                bindings_with_consumer(&[(0, 350, row)], container, cluster, selected),
                [(350, expected)],
                "cluster {cluster} row {row:?} expected {expected}"
            );
        }
    }

    #[test]
    fn a_consumer_removes_only_the_pair_it_names() {
        use crate::design::decode::body_consumers::COMBINE_TYPE_GUID;
        use crate::test_support::component_body_test::{
            BodyConsumer, HistoryOnlyContainer as Container, BODY_CONSUMER_SCOPE,
        };
        assert_eq!(
            bindings_with_consumer(
                &[(0, 350, None), (1, 351, Some(true))],
                Container::Lists(&[]),
                true,
                Some(BodyConsumer {
                    scope_type_guid: COMBINE_TYPE_GUID,
                    entity: 350,
                    hidden: 1,
                    flag: 1,
                    selection_owner: Some(BODY_CONSUMER_SCOPE),
                    registered_node: true,
                }),
            ),
            [(350, true), (351, false)]
        );
    }

    #[test]
    fn history_only_pair_requires_complete_memberships_that_omit_a_hidden_entity() {
        use crate::test_support::component_body_test::{
            history_only_design, HistoryOnlyContainer as Container,
        };
        const PREFIX: &str = "FusionAssetName[Active]/Design1/";
        const BLOB: &str = "BREP.synthetic.smbh";
        for (hidden, container, history_only, owned) in [
            (Some(true), Container::Lists(&[]), true, false),
            (Some(false), Container::Lists(&[]), false, false),
            (None, Container::Lists(&[]), false, false),
            (Some(true), Container::Lists(&[350]), false, true),
            (Some(true), Container::Absent, false, false),
            (Some(true), Container::Malformed, false, false),
            (Some(true), Container::OtherVersion(&[]), false, false),
            (Some(true), Container::ListsAndMalformed, false, false),
        ] {
            let (bulk, metadata) = history_only_design(BLOB, hidden, container);
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let stored = crate::zip_write::file_options(CompressionMethod::Stored);
            write_synthetic_manifests(&mut zip, stored);
            zip.start_file(format!("{PREFIX}BulkStream.dat"), stored)
                .unwrap();
            zip.write_all(&bulk).unwrap();
            zip.start_file(format!("{PREFIX}MetaStream.dat"), stored)
                .unwrap();
            zip.write_all(&metadata).unwrap();
            zip.start_file(
                format!("FusionAssetName[Active]/Breps.BlobParts/{BLOB}"),
                stored,
            )
            .unwrap();
            zip.write_all(&[0]).unwrap();
            let archive = zip.finish().unwrap().into_inner();
            with_scan(&archive, |scan| {
                let bindings = super::decode_design_body_bindings(
                    &cadmpeg_test_support::service_decode_context(),
                    scan,
                    None,
                    &[],
                )
                .unwrap();
                let [binding] = bindings.as_slice() else {
                    panic!("one body-map pair: {}", bindings.len());
                };
                assert_eq!(binding.history_only, history_only, "{hidden:?}");
                assert_eq!(binding.component_owner.is_some(), owned, "{hidden:?}");
            });
        }
    }

    fn push_browser_node(
        out: &mut Vec<u8>,
        record_index: u32,
        guid: &str,
        hidden: bool,
        entity: u64,
    ) -> u64 {
        indexed_header(out, *b"257", record_index);
        out.extend_from_slice(&[0; 10]);
        out.extend(lp_utf16_bytes(guid).expect("fixture UTF-16 code-unit count fits u32"));
        let hidden_offset = u64_from_index(out.len());
        out.push(u8::from(hidden));
        out.extend_from_slice(&[1, 1]);
        out.extend_from_slice(&entity.to_le_bytes());
        hidden_offset
    }

    #[test]
    fn presentation_guid_selects_visibility_when_suffix_repeats() {
        let entity = 42u64;
        let selected_guid = "11111111-2222-8333-A444-555555555555";
        let competing_guid = "AAAAAAAA-BBBB-8CCC-9DDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        push_entity_header(&mut bytes, "256", entity);
        bytes.extend(
            lp_utf16_bytes(selected_guid).expect("fixture UTF-16 code-unit count fits u32"),
        );
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(
            lp_utf16_bytes("99999999-8888-8777-A666-555555555555")
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
        bytes.extend(
            lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID)
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
        bytes.extend(
            lp_utf16_bytes("PrismMaterial-001").expect("fixture UTF-16 code-unit count fits u32"),
        );
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        bytes.extend(lp_utf16_bytes("Body").expect("fixture UTF-16 code-unit count fits u32"));
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend(
            lp_utf16_bytes("12345678-1234-8234-A234-123456789ABC")
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
        bytes.extend(
            lp_utf16_bytes(APPEARANCE_LIBRARY_ID).expect("fixture UTF-16 code-unit count fits u32"),
        );
        let selected_start = bytes.len();
        let selected_offset = push_browser_node(&mut bytes, 100, selected_guid, false, entity);
        let competing_start = bytes.len();
        push_browser_node(&mut bytes, 101, competing_guid, true, entity);

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100, 101],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![
                primary_record(entity, 0),
                primary_record(100, selected_start),
                primary_record(101, competing_start),
            ],
            secondary_records: Vec::new(),
        };
        let visibility =
            { typed_browser_node_hidden_flags(&bytes, &meta) }.expect("typed presentation graph");
        let selected = visibility.get(&entity).expect("presentation-selected node");
        assert_eq!(selected.byte_offset, selected_offset);
        assert!(!selected.hidden);

        let mut nodes_only = Vec::new();
        let selected_start = nodes_only.len();
        push_browser_node(&mut nodes_only, 100, selected_guid, false, entity);
        let competing_start = nodes_only.len();
        push_browser_node(&mut nodes_only, 101, competing_guid, true, entity);
        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    Vec::new(),
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100, 101],
                ),
            ],
            records: vec![
                primary_record(100, selected_start),
                primary_record(101, competing_start),
            ],
            secondary_records: Vec::new(),
        };
        let visibility =
            { typed_browser_node_hidden_flags(&nodes_only, &meta) }.expect("typed browser nodes");
        assert!(
            !visibility.contains_key(&entity),
            "two unjoined typed nodes are ambiguous"
        );
    }

    #[test]
    fn browser_visibility_selects_the_hidden_node() {
        let mut bytes = Vec::new();
        push_browser_node(
            &mut bytes,
            100,
            "11111111-2222-8333-A444-555555555555",
            true,
            42,
        );
        let metadata = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    Vec::new(),
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100],
                ),
            ],
            records: vec![primary_record(100, 0)],
            secondary_records: Vec::new(),
        };
        let selected = typed_browser_node_hidden_flags(&bytes, &metadata).unwrap();
        assert!(selected.get(&42).unwrap().hidden);
    }

    #[test]
    fn scanned_browser_guids_drop_ambiguous_entities() {
        const GUID: &str = "AAAAAAAA-BBBB-8CCC-9DDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        push_browser_node(&mut bytes, 100, GUID, false, 42);
        push_browser_node(&mut bytes, 101, GUID, true, 43);
        let entities = super::scanned_browser_node_entities(&bytes);
        assert!(entities.is_empty());
    }

    #[test]
    fn body_bound_candidate_has_one_marker_and_six_ordered_f64_values() {
        let values: [f64; 6] = [4.0, 6.0, 1.5, -1.0, 0.0, -0.25];
        let mut bytes = vec![1];
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let candidates = body_bound_candidates(&bytes, 0, bytes.len()).collect::<Vec<_>>();
        assert_eq!(
            candidates
                .into_iter()
                .map(|(offset, values)| (offset, values.map(cadmpeg_ir::scalar::FiniteReal::get)))
                .collect::<Vec<_>>(),
            [(0, values)]
        );

        bytes[0] = 0;
        assert!(body_bound_candidates(&bytes, 0, bytes.len())
            .next()
            .is_none());
    }

    #[test]
    fn body_bounds_bind_matching_binding_identifier() {
        use crate::records::bodies::{
            DesignBodyBinding, DesignBodyBindingWire, DesignBodyBounds, DesignBodyBoundsWire,
        };
        const STREAM: &str = "Design/BulkStream.dat";
        let binding = DesignBodyBinding::try_from(DesignBodyBindingWire {
            component_owner: None,
            component_owner_unresolved: false,
            history_only: false,
            id: "f3d:Design/BulkStream.dat:design-body-binding#20".into(),
            stream: STREAM.into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 9,
            asm_body_key_offset: 20,
            entity_suffix: 7,
            entity_suffix_offset: 28,
            blob_name: "BREP.synthetic.smbh".into(),
            blob_name_offset: 40,
            body: None,
        })
        .unwrap();
        let make_bounds = || {
            DesignBodyBounds::try_from(DesignBodyBoundsWire {
                id: format!("{}:design-body-bounds#0", crate::ids::native_scope(STREAM)),
                entity_suffix: 7,
                entity_byte_offset: 0,
                record_indices: [8, 9, 10],
                record_byte_offsets: [10, 20, 30],
                value_byte_offsets: [11, 21, 31],
                body_binding_ids: Vec::new(),
                maximum: cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
                minimum: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            })
            .unwrap()
        };
        let mut bounds = [make_bounds()];
        super::bind_body_bounds(&mut bounds, &[binding]).unwrap();
        assert_eq!(
            bounds[0].body_binding_ids().collect::<Vec<_>>(),
            ["f3d:Design/BulkStream.dat:design-body-binding#20"]
        );
    }

    #[test]
    fn decoded_body_bounds_read_one_entity() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut bulk = Vec::new();
        indexed_header(&mut bulk, *b"256", 7);
        bulk.extend_from_slice(&[0; 5]);
        for record_index in [8, 9, 10] {
            indexed_header(&mut bulk, *b"257", record_index);
            bulk.push(1);
            for value in [4.0_f64, 6.0, 1.5, -1.0, 0.0, -0.25] {
                bulk.extend_from_slice(&value.to_le_bytes());
            }
        }
        let entity = crate::records::entity_header::DesignEntityHeader {
            id: format!("{}:design-entity-header#0", crate::ids::native_scope(ENTRY)),
            byte_offset: 0,
            entity_id: crate::records::identity::DesignEntityId::try_from("0_7".to_owned())
                .unwrap(),
            class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                .unwrap(),
            optional_slot_present: false,
            registration: crate::records::entity_header::DesignEntityRegistration::new(
                Some(DESIGN_MODULE_BODY.to_owned()),
                None,
                crate::records::identity::ReferenceRun::unlocated(Vec::new()),
            )
            .unwrap(),
        };
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(&bulk).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            let bounds = super::decode_body_bounds(scan, std::slice::from_ref(&entity)).unwrap();
            assert_eq!(bounds.len(), 1);
            assert_eq!(bounds[0].entity_suffix(), 7);
        });
    }

    #[test]
    fn a_recipe_marker_before_the_record_index_word_states_no_record_index() {
        // The family marker opens at offset four, so the stream holds no index
        // word sixteen bytes before it.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(b"body_recipe_data");
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(&bytes, "Design/BulkStream.dat", &mut recipes)
            .unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].record_index, None);
        let wire = serde_json::to_value(&recipes[0]).expect("recipe wire");
        assert_eq!(wire.get("record_index"), None);
        assert_eq!(wire.get("record_index_offset"), None);
    }

    #[test]
    fn bounded_face_record_identity_is_not_a_second_design_id() {
        let mut bytes = Vec::new();
        for _ in 0..2 {
            let mut prefix = [0u8; 27];
            prefix[11..15].copy_from_slice(&309i32.to_le_bytes());
            prefix[23..27].copy_from_slice(&24u32.to_le_bytes());
            bytes.extend_from_slice(&prefix);
            bytes.extend_from_slice(b"bounded_face_recipe_data");
            bytes.extend_from_slice(&(-1i64).to_le_bytes());
        }
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(&bytes, "Design/BulkStream.dat", &mut recipes)
            .unwrap();
        assert_eq!(recipes.len(), 2);
        assert!(recipes
            .iter()
            .all(|recipe| recipe.record_index.map(|index| index.value) == Some(309)));
        assert!(recipes.iter().all(|recipe| recipe.design.is_none()));
        assert_eq!(recipes[0].recipe_index, 0);
        assert_eq!(recipes[1].recipe_index, 1);

        let mut body = Vec::new();
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(b"2265");
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&[0; 12]);
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(b"body_recipe_data");
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(&body, "Design/BulkStream.dat", &mut recipes)
            .unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(
            recipes[0]
                .design
                .as_ref()
                .map(|design| design.id.value.as_str()),
            Some("2265")
        );
        assert_eq!(
            recipes[0].design.as_ref().map(|design| design.id.offset),
            Some(4)
        );
        assert_eq!(
            recipes[0]
                .design
                .as_ref()
                .and_then(|design| design.selector),
            Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 3,
                byte_offset: 8,
            })
        );
    }

    #[test]
    fn construction_recipe_counter_keys_borrow_short_design_ids() {
        for length in 1..=8 {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&u32::try_from(length).unwrap().to_le_bytes());
            bytes.extend_from_slice(&b"12345678"[..length]);
            let (id, offset) = super::ascii_id_at(&bytes, 0).unwrap();
            assert_eq!(offset, 4);
            assert_eq!(id.as_ptr(), bytes[4..].as_ptr());
            assert_eq!(id, std::str::from_utf8(&b"12345678"[..length]).unwrap());
        }
    }
}
