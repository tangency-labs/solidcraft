// SPDX-License-Identifier: Apache-2.0
//! Exact mirror scopes and mirror construction binding.

use crate::bytes::lp_utf16_bounded;

use super::shared_frames::marked_record_reference;
use crate::container::ContainerScan;
use crate::design::decode::operands::parse_face_operand;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::{cached_owned_record_offsets, IndexedRecordOffsets};

use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::layout::design_mirror_scope_class369_tail as mirror_369;
use crate::layout::design_mirror_scope_class391_tail as mirror_391;
use crate::layout::design_mirror_scope_class413_tail as mirror_413;
use crate::layout::design_mirror_scope_class440_tail as mirror_440;
use crate::layout::design_mirror_scope_class441_count_owner as mirror_441_count;
use crate::layout::design_mirror_scope_class441_tail as mirror_441;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::mirror;
use crate::records::feature::mirror::DesignMirrorConstruction;
use crate::records::feature::mirror::DesignMirrorScopeTolerance;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use crate::records::recipes::ConstructionRecipe;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;
use std::collections::HashMap;

/// Parse the class-441 Mirror count owner carried outside the ordinary owner
/// arena.
///
/// The scope's fourth reference names a class-426 frame paired with class 267.
/// Its exact compact scalar envelope carries count two and has no decoded
/// Design-parameter backlink.
fn exact_legacy_mirror_scope_count(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<(u32, u64)> {
    if (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) != ("441", "267") {
        return None;
    }
    let count_record_index = *scope.reference_members().values().nth(3)?;
    let [start, paired] = records.offsets(count_record_index) else {
        return None;
    };
    if paired.checked_sub(*start)? != mirror_441_count::LEN {
        return None;
    }
    let (paired_class_tag, paired_after_tag) = borrowed_ascii_tag(bytes, *paired)?;
    if paired_class_tag != b"267"
        || paired_after_tag != paired.checked_add(7)?
        || View::u32_le_at(bytes, paired_after_tag) != Some(count_record_index)
    {
        return None;
    }
    let frame = bytes.get(*start..*paired)?;
    let owner = crate::design::decode::parameters::parse_parameter_owner(frame)?;
    if owner.class_tag.as_str() != "426"
        || owner.record_index != count_record_index
        || owner.scope_record_index != scope.record_index
        || owner.local_ordinal != mirror_441_count::LOCAL_ORDINAL_VALUE
        || owner.owned_ordinal != mirror_441_count::OWNED_ORDINAL_VALUE
        || owner.evaluated_value.get() != f64::from(mirror_441_count::COUNT_VALUE)
        || owner.frame_length != u64::try_from(mirror_441_count::LEN).ok()?
        || owner.parameter_record_index != count_record_index.checked_add(2)?
        || owner.companion_record_index != count_record_index.checked_add(1)?
        || owner.evaluated_value_offset
            != crate::design::decode::parameters::FrameRelative(
                i128::try_from(mirror_441_count::COUNT).ok()?,
            )
    {
        return None;
    }
    Some((
        count_record_index,
        owner
            .evaluated_value_offset
            .absolute(u64::try_from(*start).ok()?)?,
    ))
}

/// Parse a legacy Mirror scalar lane.
///
/// These forms have no ordinal-one parameter owner. Their positive stitch
/// tolerance is carried after the preceding-history field, with two marked
/// references naming the adjacent legacy records. The class pair selects the
/// generation-specific scalar marker; the remaining tail offsets are shared.
fn exact_legacy_mirror_scope_tolerance(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(PositiveReal, u64, DesignMirrorScopeTolerance)> {
    let (
        tail_length,
        previous_state,
        scalar_marker,
        stitch_tolerance,
        repeated_marker,
        first_reference,
        second_reference,
        marker_value,
    ) = match (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) {
        ("369", "261") => (
            mirror_369::LEN,
            mirror_369::PREVIOUS_HISTORY_STATE,
            mirror_369::SCALAR_MARKER,
            mirror_369::STITCH_TOLERANCE,
            Some(mirror_369::REPEATED_SCALAR_MARKER),
            mirror_369::FIRST_REFERENCE,
            mirror_369::SECOND_REFERENCE,
            mirror_369::SCALAR_MARKER_VALUE,
        ),
        ("391", "261") => (
            mirror_391::LEN,
            mirror_391::PREVIOUS_HISTORY_STATE,
            mirror_391::SCALAR_MARKER,
            mirror_391::STITCH_TOLERANCE,
            Some(mirror_391::REPEATED_SCALAR_MARKER),
            mirror_391::FIRST_REFERENCE,
            mirror_391::SECOND_REFERENCE,
            mirror_391::SCALAR_MARKER_VALUE,
        ),
        ("413", "262") => (
            mirror_413::LEN,
            mirror_413::PREVIOUS_HISTORY_STATE,
            mirror_413::SCALAR_MARKER,
            mirror_413::STITCH_TOLERANCE,
            Some(mirror_413::REPEATED_SCALAR_MARKER),
            mirror_413::FIRST_REFERENCE,
            mirror_413::SECOND_REFERENCE,
            mirror_413::SCALAR_MARKER_VALUE,
        ),
        ("440", "258") => (
            mirror_440::LEN,
            mirror_440::PREVIOUS_HISTORY_STATE,
            mirror_440::SCALAR_MARKER,
            mirror_440::STITCH_TOLERANCE,
            Some(mirror_440::REPEATED_SCALAR_MARKER),
            mirror_440::FIRST_REFERENCE,
            mirror_440::SECOND_REFERENCE,
            mirror_440::SCALAR_MARKER_VALUE,
        ),
        ("441", "267") => (
            mirror_441::LEN,
            mirror_441::PREVIOUS_HISTORY_STATE,
            mirror_441::SCALAR_MARKER,
            mirror_441::STITCH_TOLERANCE,
            None,
            mirror_441::FIRST_REFERENCE,
            mirror_441::SECOND_REFERENCE,
            mirror_441::SCALAR_MARKER_VALUE,
        ),
        _ => return None,
    };
    let kind_code_units = scope.source_kind_name().encode_utf16().count();
    let kind_end = usize::try_from(scope.kind_offset())
        .ok()?
        .checked_add(kind_code_units.checked_mul(2)?)?;
    let previous = usize::try_from(scope.previous_history_state_id_offset()?).ok()?;
    if previous != kind_end.checked_add(previous_state)? {
        return None;
    }
    let paired = usize::try_from(scope.paired_byte_offset()).ok()?;
    if paired != kind_end.checked_add(tail_length)?
        || paired.checked_sub(usize::try_from(scope.byte_offset()).ok()?)?
            != usize::try_from(scope.frame_length()).ok()?
    {
        return None;
    }
    let marker_offset = kind_end.checked_add(scalar_marker)?;
    let value_offset = kind_end.checked_add(stitch_tolerance)?;
    let repeated_marker_offset = repeated_marker.and_then(|offset| kind_end.checked_add(offset));
    let marker = View::u32_le_at(bytes, marker_offset)?;
    if marker != marker_value {
        return None;
    }
    if let Some(repeated_marker_offset) = repeated_marker_offset {
        if View::u32_le_at(bytes, repeated_marker_offset)? != marker_value {
            return None;
        }
    }
    let value = PositiveReal::new(View::f64_le_at(bytes, value_offset)?)?;
    let first_reference_offset = kind_end.checked_add(first_reference)?;
    let second_reference_offset = kind_end.checked_add(second_reference)?;
    let reference_slot = second_reference - first_reference;
    if bytes.get(first_reference_offset + 11..first_reference_offset + reference_slot)? != [0; 2]
        || bytes.get(second_reference_offset + 11..second_reference_offset + reference_slot)?
            != [0; 2]
        || second_reference_offset.checked_add(reference_slot)? != paired
    {
        return None;
    }
    let first_reference = marked_record_reference(bytes, first_reference_offset)?;
    let second_reference = marked_record_reference(bytes, second_reference_offset)?;
    if first_reference != scope.record_index.checked_add(2)?
        || second_reference != scope.record_index.checked_add(1)?
    {
        return None;
    }
    Some((
        value,
        u64::try_from(value_offset).ok()?,
        DesignMirrorScopeTolerance {
            marker: mirror::DesignMirrorToleranceMarker::try_from((
                marker,
                repeated_marker_offset.map(u64::try_from).transpose().ok()?,
            ))
            .ok()?,
            marker_offset: u64::try_from(marker_offset).ok()?,
            first_reference,
            first_reference_offset: u64::try_from(first_reference_offset).ok()?,
            second_reference,
            second_reference_offset: u64::try_from(second_reference_offset).ok()?,
        },
    ))
}

/// Count matching records without retaining the input-sized run.
enum UniqueMatch<T> {
    Zero,
    One(T),
    Many,
}

fn unique_match<I: Iterator>(mut items: I) -> UniqueMatch<I::Item> {
    match (items.next(), items.next()) {
        (None, _) => UniqueMatch::Zero,
        (Some(item), None) => UniqueMatch::One(item),
        (Some(_), Some(_)) => UniqueMatch::Many,
    }
}

/// Join a Mirror scope's two operand groups and fixed parameters with either a
/// referenced `WorkPlane` or a persistent plane-face selection.
pub(crate) fn bind_mirror_constructions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    owners: &[DesignParameterOwner],
    recipes: &[ConstructionRecipe],
) -> Result<(), CodecError> {
    let mut headers_by_record = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        headers_by_record.insert(key, header);
    }
    let mut record_offset_index: HashMap<String, IndexedRecordOffsets> = HashMap::new();
    for index in 0..scopes.len() {
        if design_feature_family(&scopes[index].kind()) != Some(DesignFeatureFamily::Mirror) {
            continue;
        }
        let Some(stream) = native_stream(&scopes[index].id) else {
            continue;
        };
        let stream = stream.to_owned();
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, &stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope_record_index = scopes[index].record_index;
        let seed_groups = unique_match(
            groups
                .iter()
                .filter(|group| {
                    native_stream(&group.id) == Some(stream.as_str())
                        && group.scope_record_index == scope_record_index
                })
                .filter(|group| {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                    )
                }),
        );
        let plane_groups = unique_match(groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream.as_str())
                && group.scope_record_index == scope_record_index
                && group.role() == DesignOperandRole::ROLE_0X5
        }));
        let (UniqueMatch::One(seed_group), UniqueMatch::One(plane_group)) =
            (seed_groups, plane_groups)
        else {
            continue;
        };
        let [crate::records::identity::Located {
            value: plane_member,
            ..
        }] = plane_group.members()
        else {
            continue;
        };
        let Some(plane_header) = headers_by_record.get(&(stream.as_str(), *plane_member)) else {
            continue;
        };
        let work_plane = compact_feature_reference(bytes, plane_header).and_then(
            |(plane_reference, plane_reference_offset)| {
                plane_reference
                    .checked_add(1)
                    .filter(|record_index| {
                        scopes.iter().any(|scope| {
                            native_stream(&scope.id) == Some(stream.as_str())
                                && scope.record_index == *record_index
                                && scope.kind() == scope::DesignFeatureKind::WorkPlane
                                && scope.work_plane_frame().is_some()
                        })
                    })
                    .map(|record_index| (record_index, plane_reference_offset))
            },
        );
        let face_recipe = {
            let records = cached_owned_record_offsets(&mut record_offset_index, &stream, bytes);
            parse_face_operand(
                ctx,
                bytes,
                records,
                crate::design::decode::operands::FaceOperandFrame {
                    scope: &scopes[index],
                    scope_reference_ordinal: plane_group.scope_reference_ordinal,
                    group_ownership: Some((plane_group.record_index, 0)),
                    next_byte_offset: None,
                    header: plane_header,
                },
                recipes,
            )
            .transpose()?
            .is_some()
        };
        let (plane_scope_record_index, plane_selection_record_index) =
            if let Some((plane_scope_record_index, plane_reference_offset)) = work_plane {
                (
                    Some(crate::records::identity::Located {
                        value: plane_scope_record_index,
                        offset: plane_reference_offset,
                    }),
                    None,
                )
            } else if crate::design::decode::operands::parse_entity_selection_operand(
                bytes,
                plane_group,
                0,
                plane_header,
            )
            .is_some()
                || face_recipe
            {
                (None, Some(*plane_member))
            } else {
                continue;
            };
        let seed_feature = match seed_group.members() {
            _ if seed_group.role() != DesignOperandRole::BODIES_B => None,
            [crate::records::identity::Located { value: member, .. }] => {
                match headers_by_record.get(&(stream.as_str(), *member)) {
                    Some(header) => {
                        compact_feature_reference(bytes, header).filter(|(record_index, _)| {
                            scopes.iter().any(|scope| {
                                native_stream(&scope.id) == Some(stream.as_str())
                                    && scope.record_index == *record_index
                            })
                        })
                    }
                    None => None,
                }
            }
            _ => None,
        };
        let records = cached_owned_record_offsets(&mut record_offset_index, &stream, bytes);
        let count = unique_match(owners.iter().filter(|owner| {
            native_stream(owner.id()) == Some(stream.as_str())
                && owner.scope_record_index() == scope_record_index
                && owner.local_ordinal() == 0
                && owner.evaluated_value().get() == 2.0
        }));
        let inline_count = exact_legacy_mirror_scope_count(bytes, records, &scopes[index]);
        let inline_tolerance = exact_legacy_mirror_scope_tolerance(bytes, &scopes[index]);
        let tolerance = unique_match(
            owners
                .iter()
                .filter(|owner| {
                    native_stream(owner.id()) == Some(stream.as_str())
                        && owner.scope_record_index() == scope_record_index
                })
                .filter(|owner| owner.local_ordinal() == 1)
                .filter_map(|owner| {
                    Some((owner, PositiveReal::try_from(owner.evaluated_value()).ok()?))
                }),
        );
        let (count, tolerance_source) = (
            match (count, inline_count) {
                (UniqueMatch::One(count), None) => {
                    Some((count.record_index(), count.evaluated_value_offset()))
                }
                (UniqueMatch::Zero, Some(count)) => Some(count),
                _ => None,
            },
            match (tolerance, inline_tolerance) {
                (UniqueMatch::One((tolerance, value)), None) => Some((
                    value,
                    tolerance.evaluated_value_offset(),
                    mirror::DesignMirrorToleranceSource::Owner {
                        record_index: tolerance.record_index(),
                    },
                )),
                (UniqueMatch::Zero, Some((value, value_offset, scope_tail))) => Some((
                    value,
                    value_offset,
                    mirror::DesignMirrorToleranceSource::Scope(scope_tail),
                )),
                _ => None,
            },
        );
        let Some((count_record_index, count_offset)) = count else {
            continue;
        };
        let Some((stitch_tolerance, stitch_tolerance_offset, tolerance_source)) = tolerance_source
        else {
            continue;
        };
        {
            let construction = Some(DesignMirrorConstruction {
                count_record_index,
                count_offset,
                stitch_tolerance,
                stitch_tolerance_offset,
                tolerance_source,
                seed_group_record_index: seed_group.record_index,
                plane_group_record_index: plane_group.record_index,
                seed_feature_scope_record_index: seed_feature
                    .map(|(value, offset)| crate::records::identity::Located { value, offset }),
                plane_scope_record_index,
                plane_selection_record_index,
                plane: None,
            });
            if let scope::DesignScopePayloadMut::Mirror(slot)
            | scope::DesignScopePayloadMut::SymetrieMiroir(slot) = scopes[index].payload_mut()
            {
                *slot = construction;
            }
        }
    }
    Ok(())
}

fn borrowed_ascii_tag(bytes: &[u8], at: usize) -> Option<(&[u8], usize)> {
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if count > 2000 {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(count)?;
    let raw = bytes.get(start..end)?;
    raw.iter().all(u8::is_ascii_graphic).then_some((raw, end))
}

fn compact_feature_reference(bytes: &[u8], header: &DesignRecordHeader) -> Option<(u32, u64)> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21) != Some(&[0; 10][..])
        || bytes.get(start + 21) != Some(&1)
        || View::u32_le_at(bytes, start + 22) != header.record_index.checked_add(3)
        || bytes.get(start + 26..start + 32) != Some(&[0; 6][..])
        || View::u32_le_at(bytes, start + 32) != Some(1)
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 36, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !crate::bytes::is_guid_relaxed(&asset_id)
        || !crate::bytes::is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id) != Some(2)
        || bytes.get(after_context_id + 4..after_context_id + 8) != Some(&[0; 4][..])
    {
        return None;
    }
    (|| {
        let paired_at = next_indexed_record_offset(bytes, after_context_id + 8)?;
        let nested_one_at = next_indexed_record_offset(bytes, paired_at + 11)?;
        let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + 11)?;
        let identity_at = next_indexed_record_offset(bytes, nested_two_at + 11)?;
        let next_at = next_indexed_record_offset(bytes, identity_at + 11)?;
        for (offset, expected) in [
            (paired_at, header.record_index),
            (nested_one_at, header.record_index.checked_add(1)?),
            (nested_two_at, header.record_index.checked_add(2)?),
            (identity_at, header.record_index.checked_add(3)?),
            (next_at, header.record_index.checked_add(4)?),
        ] {
            let (_, after_tag) = borrowed_ascii_tag(bytes, offset)?;
            if View::u32_le_at(bytes, after_tag)? != expected {
                return None;
            }
        }
        if identity_at.checked_add(29)? != next_at
            || bytes.get(identity_at + 11..identity_at + 21)? != [0; 10]
            || bytes.get(identity_at + 25..identity_at + 29)? != [0; 4]
        {
            return None;
        }
        Some((
            View::u32_le_at(bytes, identity_at + 21)?,
            u64::try_from(identity_at + 21).ok()?,
        ))
    })()
}

#[cfg(test)]
mod tests;
