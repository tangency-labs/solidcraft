// SPDX-License-Identifier: Apache-2.0
//! Parse Form cage references and bind committed neutral cages.

use crate::container::ContainerScan;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::ids::{self, native_stream};
use crate::layout::{
    form_class_325_cage_entry, form_class_325_cage_table, form_class_328_cage_group,
    form_class_328_metadata_group, form_class_328_reference_entry, form_class_328_scope,
    form_class_350_member_owner_tail, form_serializer_frame_132,
};
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

fn distinct_form_cage_ids<T: Eq + Hash>(ids: &[T]) -> bool {
    let mut distinct = HashSet::new();
    for id in ids {
        if distinct.contains(id) {
            return false;
        }

        // discarded-value: the duplicate case returned above.
        let _ = distinct.insert(id);
    }
    true
}

fn push_form_cage_id(ids: &mut Vec<cadmpeg_ir::ids::SubdId>, id: &cadmpeg_ir::ids::SubdId) {
    ids.push(id.clone());
}

/// Replace a resolved Form scope's native definition with its committed cages.
///
/// The Form's cage-list record owns ordered cage-object references. Each object
/// reaches a surface record, and the serializer naming that surface supplies
/// the archive entry identity of the neutral cage.
pub(crate) fn bind_form_cages(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    features: &mut [cadmpeg_ir::features::Feature],
    cages: &[cadmpeg_ir::subd::SubdSurface],
) -> Result<(), CodecError> {
    'scope: for scope in scopes
        .iter()
        .filter(|scope| scope.kind() == crate::records::feature::scope::DesignFeatureKind::Form)
    {
        let Some(stream) =
            native_stream(&scope.id).and_then(|stream| stream.strip_prefix(ids::SCHEME_PREFIX))
        else {
            continue;
        };
        let bytes = scan.entry_bytes(stream)?;
        let records = IndexedRecordOffsets::build(bytes);
        let cage_lists = form_cage_lists(
            bytes,
            &records,
            scope.reference_members().values().copied(),
            scope.record_index,
        );
        if scope.class_tag.as_str() == "325" {
            if let Some(cage_objects) = form_class_325_cage_objects(
                bytes,
                &records,
                scope.record_index,
                scope.reference_members().values().copied(),
            )? {
                let serializers = form_cage_serializers(ctx, bytes, &records)?;
                let mut resolved = Vec::new();
                let mut valid = true;
                for object in cage_objects {
                    let Some(surface) = form_class_325_cage_surface(bytes, &records, object) else {
                        valid = false;
                        break;
                    };
                    let Some(entry_name) = serializers.entry_name(surface) else {
                        valid = false;
                        break;
                    };
                    let mut matches = cages.iter().filter(|cage| {
                        cage.source_object
                            .as_ref()
                            .and_then(|source| source.object_id.as_str().rsplit('/').next())
                            == Some(entry_name)
                    });
                    let Some(cage) = matches.next() else {
                        continue;
                    };
                    if matches.next().is_some() {
                        valid = false;
                        break;
                    }
                    push_form_cage_id(&mut resolved, &cage.id);
                }
                if valid && !resolved.is_empty() && distinct_form_cage_ids(&resolved) {
                    let feature_id = crate::ids::neutral_feature_id(scope);
                    if let Some(feature) =
                        features.iter_mut().find(|feature| feature.id == feature_id)
                    {
                        if matches!(
                            feature.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(
                                cadmpeg_ir::features::FeatureOperation::Native { .. }
                            )
                        ) {
                            feature.evaluation.set_definition(
                                cadmpeg_ir::features::FeatureDefinition::Operation(
                                    cadmpeg_ir::features::FeatureOperation::Form {
                                        cages: resolved,
                                    },
                                ),
                            );
                        }
                    }
                }
                continue;
            }
        }
        if scope.class_tag.as_str() == "328"
            && scopes
                .iter()
                .filter(|candidate| {
                    candidate.kind() == crate::records::feature::scope::DesignFeatureKind::Form
                })
                .count()
                == 1
            && form_class_328_envelope(bytes, &records, scope)
        {
            let serializers = form_cage_serializers(ctx, bytes, &records)?;
            let mut resolved = Vec::new();
            let mut valid = true;
            for surface in &serializers.ordered {
                let Some(entry_name) = serializers.entry_name(*surface) else {
                    valid = false;
                    break;
                };
                let mut matches = cages.iter().filter(|cage| {
                    cage.source_object
                        .as_ref()
                        .and_then(|source| source.object_id.as_str().rsplit('/').next())
                        == Some(entry_name)
                });
                let Some(cage) = matches.next() else {
                    continue;
                };
                if matches.next().is_some() {
                    valid = false;
                    break;
                }
                push_form_cage_id(&mut resolved, &cage.id);
            }
            if valid
                && !resolved.is_empty()
                && resolved.len() == cages.len()
                && distinct_form_cage_ids(&resolved)
            {
                let feature_id = crate::ids::neutral_feature_id(scope);
                if let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id)
                {
                    if matches!(
                        feature.evaluation.definition(),
                        cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::Native { .. }
                        )
                    ) {
                        feature.evaluation.set_definition(
                            cadmpeg_ir::features::FeatureDefinition::Operation(
                                cadmpeg_ir::features::FeatureOperation::Form { cages: resolved },
                            ),
                        );
                    }
                }
            }
            continue;
        }
        let [cage_objects] = cage_lists.as_slice() else {
            continue;
        };
        let Some(surfaces) = form_cage_surfaces(bytes, &records, cage_objects, scope.record_index)
        else {
            continue;
        };
        let serializers = form_cage_serializers(ctx, bytes, &records)?;
        let mut resolved = Vec::new();
        for surface in &surfaces {
            let Some(entry_name) = serializers.entry_name(*surface) else {
                continue 'scope;
            };
            let mut matches = cages.iter().filter(|cage| {
                cage.source_object
                    .as_ref()
                    .and_then(|source| source.object_id.as_str().rsplit('/').next())
                    == Some(entry_name)
            });
            let Some(cage) = matches.next() else {
                continue 'scope;
            };
            if matches.next().is_some() {
                continue 'scope;
            }
            push_form_cage_id(&mut resolved, &cage.id);
        }
        if !distinct_form_cage_ids(&resolved) {
            continue;
        }
        let feature_id = crate::ids::neutral_feature_id(scope);
        let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) else {
            continue;
        };
        if matches!(
            feature.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native { .. }
            )
        ) {
            feature
                .evaluation
                .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Form { cages: resolved },
                ));
        }
    }
    Ok(())
}

fn form_class_328_envelope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> bool {
    let Some((scope_start, scope_paired)) = one_indexed_frame(records, scope.record_index) else {
        return false;
    };
    if scope_paired.checked_sub(scope_start) != Some(form_class_328_scope::LEN)
        || bytes.get(scope_start + 4..scope_start + 7) != Some(b"328")
        || bytes.get(scope_paired + 4..scope_paired + 7) != Some(b"267")
    {
        return false;
    }
    if scope.reference_members().len() != 2 {
        return false;
    }
    let Some(group_record) = scope
        .reference_members()
        .values()
        .copied()
        .find(|record_index| {
            records.frames(*record_index).any(|(start, paired)| {
                bytes.get(start + 4..start + 7) == Some(b"417")
                    && bytes.get(paired + 4..paired + 7) == Some(b"267")
            })
        })
    else {
        return false;
    };
    let Some(metadata_record) = scope
        .reference_members()
        .values()
        .copied()
        .find(|record_index| {
            *record_index != group_record
                && records.frames(*record_index).any(|(start, paired)| {
                    bytes.get(start + 4..start + 7) == Some(b"341")
                        && bytes.get(paired + 4..paired + 7) == Some(b"267")
                })
        })
    else {
        return false;
    };
    let Some((group_start, group_paired)) = records.frames(group_record).find(|(start, paired)| {
        bytes.get(*start + 4..*start + 7) == Some(b"417")
            && bytes.get(*paired + 4..*paired + 7) == Some(b"267")
    }) else {
        return false;
    };
    if group_paired.checked_sub(group_start) != Some(form_class_328_cage_group::LEN)
        || bytes.get(
            group_start + form_class_328_cage_group::ZERO_RUN_14
                ..group_start + form_class_328_cage_group::OWNER_MARKER,
        ) != Some(&[0; 14])
        || bytes.get(group_start + form_class_328_cage_group::OWNER_MARKER)
            != Some(&form_class_328_cage_group::OWNER_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            group_start + form_class_328_cage_group::OWNER_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            group_start + form_class_328_cage_group::ZERO_RUN_14_AFTER_OWNER
                ..group_start + form_class_328_cage_group::MEMBER_COUNT,
        ) != Some(&[0; 14])
        || View::u32_le_at(bytes, group_start + form_class_328_cage_group::MEMBER_COUNT)
            != Some(form_class_328_cage_group::MEMBER_COUNT_VALUE)
        || View::u32_le_at(bytes, group_start + form_class_328_cage_group::TERMINAL_U32)
            != Some(form_class_328_cage_group::TERMINAL_U32_VALUE)
        || bytes.get(group_start + form_class_328_cage_group::FIRST_TAIL_MARKER)
            != Some(&form_class_328_cage_group::FIRST_TAIL_MARKER_VALUE)
        || bytes.get(
            group_start + form_class_328_cage_group::FIRST_TAIL_ZERO_RUN
                ..group_start + form_class_328_cage_group::SECOND_TAIL_MARKER,
        ) != Some(&[0; 4])
        || bytes.get(group_start + form_class_328_cage_group::SECOND_TAIL_MARKER)
            != Some(&form_class_328_cage_group::SECOND_TAIL_MARKER_VALUE)
        || bytes.get(
            group_start + form_class_328_cage_group::SECOND_TAIL_ZERO_RUN
                ..group_start + form_class_328_cage_group::FINAL_SCOPE_MARKER,
        ) != Some(&[0; 3])
        || bytes.get(group_start + form_class_328_cage_group::FINAL_SCOPE_MARKER)
            != Some(&form_class_328_cage_group::FINAL_SCOPE_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            group_start + form_class_328_cage_group::FINAL_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            group_start + form_class_328_cage_group::PAIRED_ZERO_TAIL
                ..group_start + form_class_328_cage_group::LEN,
        ) != Some(&[0; 2])
    {
        return false;
    }
    let Some(group_class_272) = View::u64_le_at(
        bytes,
        group_start + form_class_328_cage_group::FIRST_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    let Some(group_class_404) = View::u64_le_at(
        bytes,
        group_start + form_class_328_cage_group::SECOND_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    if !unique_record_has_class(bytes, records, group_class_272, b"272")
        || !unique_record_has_class(bytes, records, group_class_404, b"404")
    {
        return false;
    }
    let mut members = [None; 4];
    for ordinal in 0..members.len() {
        let at = group_start
            + form_class_328_cage_group::MEMBER_ENTRIES
            + ordinal * form_class_328_reference_entry::LEN;
        if bytes.get(at) != Some(&form_class_328_reference_entry::MARKER_VALUE)
            || bytes.get(
                at + form_class_328_reference_entry::ZERO_TAIL
                    ..at + form_class_328_reference_entry::LEN,
            ) != Some(&[0; 2])
        {
            return false;
        }
        let Some(member) =
            View::u64_le_at(bytes, at + form_class_328_reference_entry::RECORD_INDEX)
                .and_then(|value| u32::try_from(value).ok())
        else {
            return false;
        };
        if members[..ordinal].contains(&Some(member)) {
            return false;
        }
        members[ordinal] = Some(member);
        let Some((member_start, member_paired)) = one_indexed_frame(records, member) else {
            return false;
        };
        if bytes.get(member_start + 4..member_start + 7) != Some(b"350")
            || bytes.get(member_paired + 4..member_paired + 7) != Some(b"351")
            || member_paired < member_start + form_class_350_member_owner_tail::LEN
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::OWNER_MARKER,
            ) != Some(&form_class_350_member_owner_tail::OWNER_MARKER_VALUE)
            || View::u64_le_at(
                bytes,
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::OWNER_GROUP_RECORD_INDEX,
            ) != Some(u64::from(group_record))
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::ZERO_RUN_3
                    ..member_paired - form_class_350_member_owner_tail::LEN
                        + form_class_350_member_owner_tail::PAIRED_MARKER,
            ) != Some(&[0; 3])
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::PAIRED_MARKER,
            ) != Some(&form_class_350_member_owner_tail::PAIRED_MARKER_VALUE)
        {
            return false;
        }
    }
    let Some((metadata_start, metadata_paired)) =
        records.frames(metadata_record).find(|(start, paired)| {
            bytes.get(*start + 4..*start + 7) == Some(b"341")
                && bytes.get(*paired + 4..*paired + 7) == Some(b"267")
        })
    else {
        return false;
    };
    if metadata_paired.checked_sub(metadata_start) != Some(form_class_328_metadata_group::LEN)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::ZERO_RUN_10
                ..metadata_start + form_class_328_metadata_group::OWNER_MARKER,
        ) != Some(&[0; 10])
        || bytes.get(metadata_start + form_class_328_metadata_group::OWNER_MARKER)
            != Some(&form_class_328_metadata_group::OWNER_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::OWNER_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            metadata_start + form_class_328_metadata_group::ZERO_RUN_2
                ..metadata_start + form_class_328_metadata_group::MEMBER_COUNT,
        ) != Some(&[0; 2])
        || View::u32_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::MEMBER_COUNT,
        ) != Some(form_class_328_metadata_group::MEMBER_COUNT_VALUE)
        || View::u32_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::TAIL_U32,
        ) != Some(form_class_328_metadata_group::TAIL_U32_VALUE)
        || View::f64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::TAIL_SCALAR,
        )
        .is_none_or(|value| !value.is_finite())
        || bytes.get(metadata_start + form_class_328_metadata_group::FIRST_TAIL_MARKER)
            != Some(&form_class_328_metadata_group::FIRST_TAIL_MARKER_VALUE)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::FIRST_TAIL_ZERO_RUN
                ..metadata_start + form_class_328_metadata_group::SECOND_TAIL_MARKER,
        ) != Some(&[0; 4])
        || bytes.get(metadata_start + form_class_328_metadata_group::SECOND_TAIL_MARKER)
            != Some(&form_class_328_metadata_group::SECOND_TAIL_MARKER_VALUE)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::SECOND_TAIL_ZERO_RUN
                ..metadata_start + form_class_328_metadata_group::FINAL_SCOPE_MARKER,
        ) != Some(&[0; 3])
        || bytes.get(metadata_start + form_class_328_metadata_group::FINAL_SCOPE_MARKER)
            != Some(&form_class_328_metadata_group::FINAL_SCOPE_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::FINAL_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            metadata_start + form_class_328_metadata_group::PAIRED_ZERO_TAIL
                ..metadata_start + form_class_328_metadata_group::LEN,
        ) != Some(&[0; 2])
    {
        return false;
    }
    let Some(metadata_class_259) = View::u64_le_at(
        bytes,
        metadata_start + form_class_328_metadata_group::FIRST_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    let Some(metadata_class_404) = View::u64_le_at(
        bytes,
        metadata_start + form_class_328_metadata_group::SECOND_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    if !unique_record_has_class(bytes, records, metadata_class_259, b"259")
        || !unique_record_has_class(bytes, records, metadata_class_404, b"404")
    {
        return false;
    }
    let mut metadata_members = [None; 19];
    for ordinal in 0..metadata_members.len() {
        let at = metadata_start
            + form_class_328_metadata_group::MEMBER_ENTRIES
            + ordinal * form_class_328_reference_entry::LEN;
        if bytes.get(at) != Some(&form_class_328_reference_entry::MARKER_VALUE)
            || bytes.get(
                at + form_class_328_reference_entry::ZERO_TAIL
                    ..at + form_class_328_reference_entry::LEN,
            ) != Some(&[0; 2])
        {
            return false;
        }
        let Some(member) =
            View::u64_le_at(bytes, at + form_class_328_reference_entry::RECORD_INDEX)
                .and_then(|value| u32::try_from(value).ok())
        else {
            return false;
        };
        if metadata_members[..ordinal].contains(&Some(member))
            || records.offsets(member).len() != 1
            || records
                .offsets(member)
                .first()
                .and_then(|offset| bytes.get(*offset + 4..*offset + 7))
                != Some(b"320")
        {
            return false;
        }
        metadata_members[ordinal] = Some(member);
    }
    true
}

fn unique_record_has_class(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class: &[u8],
) -> bool {
    let [offset] = records.offsets(record_index) else {
        return false;
    };
    bytes.get(*offset + 4..*offset + 7) == Some(class)
}

fn one_indexed_frame(records: &IndexedRecordOffsets, record_index: u32) -> Option<(usize, usize)> {
    let mut frames = records.frames(record_index);
    let frame = frames.next()?;
    frames.next().is_none().then_some(frame)
}

fn form_class_325_cage_objects(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    mut owner_record_indices: impl Iterator<Item = u32>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let parsed = (|| {
        const CAGE_COUNT: usize = 32;
        const TYPE_DISCRIMINATOR_FIRST: u32 = 307;

        let mut frames = records.frames(scope_record_index);
        let (start, paired) = frames.next()?;
        if frames.next().is_some() {
            return None;
        }
        if bytes.get(start + 4..start + 7) != Some(b"325")
            || bytes.get(paired + 4..paired + 7) != Some(b"258")
            || paired.checked_sub(start)? != form_class_325_cage_table::LEN
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_9
                    ..start + form_class_325_cage_table::LIST_MARKER,
            )? != [0; 9]
            || bytes.get(start + form_class_325_cage_table::LIST_MARKER) != Some(&1)
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_5
                    ..start + form_class_325_cage_table::OWNER_MARKER,
            )? != [0; 5]
            || bytes.get(start + form_class_325_cage_table::OWNER_MARKER) != Some(&1)
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_2
                    ..start + form_class_325_cage_table::CAGE_COUNT,
            )? != [0; 2]
        {
            return None;
        }
        let owner_record = u32::try_from(View::u64_le_at(
            bytes,
            start + form_class_325_cage_table::OWNER_RESULT_RECORD_INDEX,
        )?)
        .ok()?;
        let [owner_at, ..] = records.offsets(owner_record) else {
            return None;
        };
        if !owner_record_indices.any(|index| index == owner_record)
            || bytes.get(*owner_at + 4..*owner_at + 7) != Some(b"407")
        {
            return None;
        }
        let count = bounded_len(
            u64::from(View::u32_le_at(
                bytes,
                start + form_class_325_cage_table::CAGE_COUNT,
            )?),
            form_class_325_cage_entry::LEN,
            paired.checked_sub(start + form_class_325_cage_table::CAGE_ENTRIES)?,
        )?;
        if count != CAGE_COUNT {
            return None;
        }
        let mut objects = Vec::new();
        let mut seen_type_discriminators = [false; CAGE_COUNT];
        for ordinal in 0..count {
            let entry = start
                .checked_add(form_class_325_cage_table::CAGE_ENTRIES)?
                .checked_add(form_class_325_cage_entry::LEN.checked_mul(ordinal)?)?;
            if bytes.get(entry + form_class_325_cage_entry::CAGE_OBJECT_MARKER) != Some(&1)
                || bytes.get(
                    entry + form_class_325_cage_entry::CAGE_OBJECT_ZERO
                        ..entry + form_class_325_cage_entry::TYPE_DISCRIMINATOR,
                )? != [0, 0]
                || bytes.get(entry + form_class_325_cage_entry::COMPANION_MARKER) != Some(&1)
                || bytes.get(
                    entry + form_class_325_cage_entry::COMPANION_ZERO
                        ..entry + form_class_325_cage_entry::LEN,
                )? != [0, 0]
            {
                return None;
            }
            let type_discriminator = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::TYPE_DISCRIMINATOR,
            )?)
            .ok()?;
            let type_slot =
                usize::try_from(type_discriminator.checked_sub(TYPE_DISCRIMINATOR_FIRST)?).ok()?;
            if type_slot >= CAGE_COUNT || seen_type_discriminators[type_slot] {
                return None;
            }
            seen_type_discriminators[type_slot] = true;
            let object = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::CAGE_OBJECT_RECORD_INDEX,
            )?)
            .ok()?;
            let companion = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::COMPANION_RECORD_INDEX,
            )?)
            .ok()?;
            let mut object_frames = records
                .frames(object)
                .filter(|(_, paired)| bytes.get(paired + 4..paired + 7) == Some(b"258"));
            let (object_at, _) = object_frames.next()?;
            if object_frames.next().is_some() {
                return None;
            }
            let [companion_at, ..] = records.offsets(companion) else {
                return None;
            };
            if bytes.get(object_at + 4..object_at + 7) != Some(b"289")
                || bytes.get(*companion_at + 4..*companion_at + 7) != Some(b"273")
            {
                return None;
            }
            objects.push(object);
        }
        Some(Ok(objects))
    })();
    parsed.transpose()
}

fn form_class_325_cage_surface(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
) -> Option<u32> {
    let mut frames = records
        .frames(object_record)
        .filter(|(_, paired)| bytes.get(paired + 4..paired + 7) == Some(b"258"));
    let (start, paired) = frames.next()?;
    if frames.next().is_some() {
        return None;
    }
    if bytes.get(start + 4..start + 7) != Some(b"289") {
        return None;
    }
    let mut surface = None;
    for at in start.checked_add(11)?..paired {
        if bytes.get(at) != Some(&1) {
            continue;
        }
        let target = View::u32_le_at(bytes, at + 1)?;
        let [target_at] = records.offsets(target) else {
            continue;
        };
        if bytes.get(target_at + 4..target_at + 7) == Some(b"310")
            && surface.replace(target).is_some()
        {
            return None;
        }
    }
    surface
}

fn form_cage_objects(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<Vec<u32>> {
    let parsed = (|| -> Option<(usize, usize)> {
        let mut frames = records.frames(record_index).filter(|(_, paired)| {
            matches!(bytes.get(paired + 4..paired + 7), Some(b"258" | b"264"))
        });
        let (offset, paired) = frames.next()?;
        if frames.next().is_some() {
            return None;
        }
        if View::u64_le_at(bytes, offset + 7)? != u64::from(record_index)
            || bytes.get(offset + 15..offset + 21)? != [0; 6]
            || bytes.get(offset + 21) != Some(&1)
            || View::u64_le_at(bytes, offset + 22)? != u64::from(scope_record_index)
            || bytes.get(offset + 30..offset + 32)? != [0, 0]
        {
            return None;
        }
        let count = usize::try_from(View::u32_le_at(bytes, offset + 32)?).ok()?;
        if paired.checked_sub(offset)? != 88usize.checked_add(11usize.checked_mul(count)?)? {
            return None;
        }
        Some((count, offset.checked_add(36)?))
    })();
    let (count, mut cursor) = parsed?;

    let mut objects = Vec::new();

    for _ in 0..count {
        if bytes.get(cursor) != Some(&1) {
            return None;
        }
        let object =
            View::u64_le_at(bytes, cursor + 1).and_then(|value| u32::try_from(value).ok())?;
        objects.push(object);
        if bytes.get(cursor + 9..cursor + 11) != Some(&[0, 0][..]) {
            return None;
        }
        let next = cursor.checked_add(11)?;
        cursor = next;
    }
    Some(objects)
}

fn form_cage_lists(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    references: impl Iterator<Item = u32>,
    scope_record_index: u32,
) -> Vec<Vec<u32>> {
    references
        .filter_map(|record_index| {
            form_cage_objects(bytes, records, record_index, scope_record_index)
        })
        .collect()
}

fn form_cage_surface(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
    scope_record: u32,
) -> Option<u32> {
    let [object_at] = records.offsets(object_record) else {
        return None;
    };
    if bytes.get(object_at + 4..object_at + 7) != Some(b"301")
        || next_indexed_record_offset(bytes, object_at + 1)? != object_at + 200
        || bytes.get(object_at + 189) != Some(&1)
    {
        return None;
    }
    let first_wrapper = u32::try_from(View::u64_le_at(bytes, object_at + 190)?).ok()?;
    let [first_at] = records.offsets(first_wrapper) else {
        return None;
    };
    if bytes.get(first_at + 4..first_at + 7) != Some(b"373")
        || next_indexed_record_offset(bytes, first_at + 1)? != first_at + 33
        || bytes.get(first_at + 11..first_at + 21)? != [0; 10]
        || bytes.get(first_at + 21) != Some(&1)
        || bytes.get(first_at + 30..first_at + 33)? != [0; 3]
    {
        return None;
    }
    let second_wrapper = u32::try_from(View::u64_le_at(bytes, first_at + 22)?).ok()?;
    let [second_at] = records.offsets(second_wrapper) else {
        return None;
    };
    if bytes.get(second_at + 4..second_at + 7) != Some(b"362")
        || next_indexed_record_offset(bytes, second_at + 1)? != second_at + 29
        || bytes.get(second_at + 11..second_at + 21)? != [0; 10]
    {
        return None;
    }
    let carrier = u32::try_from(View::u64_le_at(bytes, second_at + 21)?).ok()?;
    let mut carrier_frames = records.frames(carrier);
    let (carrier_at, carrier_paired) = carrier_frames.next()?;
    if carrier_frames.next().is_some() {
        return None;
    }
    if carrier_paired.checked_sub(carrier_at)? != 665
        || bytes.get(carrier_at + 4..carrier_at + 7) != Some(b"457")
        || bytes.get(carrier_paired + 4..carrier_paired + 7) != Some(b"264")
        || bytes.get(carrier_at + 317) != Some(&1)
        || View::u64_le_at(bytes, carrier_at + 318)? != u64::from(scope_record)
        || bytes.get(carrier_at + 339) != Some(&1)
    {
        return None;
    }
    u32::try_from(View::u64_le_at(bytes, carrier_at + 340)?).ok()
}

fn form_cage_surfaces(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    objects: &[u32],
    scope_record_index: u32,
) -> Option<Vec<u32>> {
    let mut surfaces = Vec::new();
    for object in objects {
        let surface = form_cage_surface(bytes, records, *object, scope_record_index)?;
        surfaces.push(surface);
    }
    Some(surfaces)
}

struct FormCageSerializers {
    ordered: Vec<u32>,
    entries: HashMap<u32, FormCageEntry>,
}

enum FormCageEntry {
    Unique(String),
    Duplicate,
}

impl FormCageSerializers {
    fn entry_name(&self, surface: u32) -> Option<&str> {
        match self.entries.get(&surface)? {
            FormCageEntry::Unique(name) => Some(name),
            FormCageEntry::Duplicate => None,
        }
    }
}

fn form_cage_serializers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
) -> Result<FormCageSerializers, CodecError> {
    let mut offsets = Vec::new();
    for (_, record_offsets) in records.records() {
        for offset in record_offsets {
            offsets.push(*offset);
        }
    }
    offsets.sort_unstable_by(Ord::cmp);
    let mut ordered = Vec::new();
    let mut entries = HashMap::new();
    for offset in offsets {
        let is_class_335 = bytes.get(offset + 4..offset + 7) == Some(b"335");
        if !matches!(
            bytes.get(offset + 4..offset + 7),
            Some(b"315" | b"335" | b"349" | b"360" | b"431" | b"446")
        ) || bytes.get(
            offset + form_serializer_frame_132::ZERO_RUN_10
                ..offset + form_serializer_frame_132::ENTRY_NAME_LENGTH,
        ) != Some(&[0; 10])
        {
            continue;
        }
        let Some(next) = next_indexed_record_offset(bytes, offset + 1) else {
            continue;
        };
        if next != offset + form_serializer_frame_132::LEN
            || (is_class_335 && bytes.get(next + 4..next + 7) != Some(b"331"))
        {
            continue;
        }
        let name_at = offset + form_serializer_frame_132::ENTRY_NAME_LENGTH;
        let Some(name_units) = View::u32_le_at(bytes, name_at)
            .filter(|count| (1..=256).contains(count))
            .and_then(|count| usize::try_from(count).ok())
        else {
            continue;
        };
        let Some(after_name) = name_at.checked_add(4).and_then(|start| {
            name_units
                .checked_mul(2)
                .and_then(|length| start.checked_add(length))
        }) else {
            continue;
        };
        let Some(raw_name) = bytes.get(name_at + 4..after_name) else {
            continue;
        };
        let entry_name = match ctx.utf16le_text(
            raw_name,
            name_units,
            false,
            "f3d form serializer name materialization",
        ) {
            Ok(text) => text,
            Err(CodecError::Malformed(_)) => continue,
            Err(error) => return Err(error),
        };
        if !entry_name.starts_with("TSpline.")
            || !std::path::Path::new(&entry_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("tsm"))
            || bytes.get(after_name) != Some(&1)
            || bytes.get(after_name + 9..after_name + 11) != Some(&[0, 0])
        {
            continue;
        }
        let Some(surface) =
            View::u64_le_at(bytes, after_name + 1).and_then(|surface| u32::try_from(surface).ok())
        else {
            continue;
        };
        if is_class_335
            && (bytes
                .get(after_name + 11..offset + form_serializer_frame_132::LEN)
                .is_none_or(|tail| tail.iter().any(|byte| *byte != 0))
                || !records.offsets(surface).iter().any(|surface_offset| {
                    bytes.get(*surface_offset + 4..*surface_offset + 7) == Some(b"358")
                }))
        {
            continue;
        }
        if !is_class_335 && after_name + 11 != offset + form_serializer_frame_132::LEN {
            continue;
        }
        if let Some(entry) = entries.get_mut(&surface) {
            *entry = FormCageEntry::Duplicate;
        } else {
            entries.insert(surface, FormCageEntry::Unique(entry_name));
            ordered.push(surface);
        }
    }
    Ok(FormCageSerializers { ordered, entries })
}

#[cfg(test)]
mod tests;
