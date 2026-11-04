// SPDX-License-Identifier: Apache-2.0
//! Exact assembly alignment scopes.

use super::assembly_operand_frames::exact_as_built_operand_frames;
use super::assembly_operand_frames::exact_assembly_operand_frames;
use super::assembly_operand_paths::exact_assembly_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_383_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_388_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_388_scope;
use super::legacy_operand_paths::CLASS_383_OWNER_REFERENCE_ORDINALS;
use super::legacy_operand_paths::CLASS_388_OWNER_REFERENCE_ORDINALS;
use crate::design::decode::assembly::exact_legacy_as_built_421_alignment;
use crate::design::decode::assembly::exact_legacy_as_built_421_solved_frame;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::records::feature::assembly;
use crate::records::feature::assembly::DesignAssemblyAlignment;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandQualifier;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

struct AsBuiltAlignmentDraft {
    paths: [DesignAssemblyOperandPath; 2],
    frames: Option<[DesignAssemblyOperandFrame; 2]>,
}

impl TryFrom<AsBuiltAlignmentDraft> for assembly::DesignAssemblyAlignmentForm {
    type Error = &'static str;

    fn try_from(draft: AsBuiltAlignmentDraft) -> Result<Self, Self::Error> {
        Ok(Self::qualified(
            draft.frames.ok_or("as-built operand_frames are required")?,
            draft
                .paths
                .map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }),
        ))
    }
}

/// [`exact_assembly_alignment_with_types`] without the stream's record types.
#[cfg(test)]
pub(super) fn exact_assembly_alignment(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignAssemblyAlignment>, CodecError> {
    exact_assembly_alignment_with_types(
        bytes,
        records,
        scope,
        parameter_owners,
        &std::collections::HashMap::new(),
    )
}

use crate::layout::assembly_as_built_scope_owner_run as as_built;

/// Whether a placement-ordered `As-built` scope stores its owner run: the u32
/// additional-owner count at offset 43 is the count its frame length implies, the
/// marked references after it name the owners after the four alignment owners
/// in local order, the reference table has nine entries more than that count,
/// the four alignment owners appear once and contiguously in it, and every
/// additional owner appears in it.
fn placement_ordered_owner_run(
    bytes: &[u8],
    scope: &DesignParameterScope,
    lanes: &[&DesignParameterOwner],
    owners: &[crate::records::identity::Located<u32>],
) -> bool {
    let Some(count) =
        crate::design::assembly::as_built_additional_owner_count(scope.frame_length())
    else {
        return false;
    };
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return false;
    };
    let stored = View::u32_le_at(bytes, start + as_built::ADDITIONAL_OWNER_COUNT);
    let additional_owners = lanes.get(4..).unwrap_or_default();
    let table = scope.reference_members();
    let unextended_positions_hold = count != 0
        || table
            .values_in(5..9)
            .is_some_and(|values| values.eq(owners.iter().map(|owner| &owner.value)));
    unextended_positions_hold
        && stored.and_then(|stored| usize::try_from(stored).ok()) == Some(count)
        && table.len() == 9 + count
        && additional_owners.len() == count
        && additional_owners
            .iter()
            .enumerate()
            .all(|(ordinal, owner)| {
                super::shared_frames::marked_record_reference(
                    bytes,
                    start + as_built::ADDITIONAL_OWNER_REFERENCES + 11 * ordinal,
                ) == Some(owner.record_index())
                    && table.values().any(|value| *value == owner.record_index())
            })
        && (0..table.len())
            .filter(|&at| {
                table
                    .values_in(at..at + owners.len())
                    .is_some_and(|values| values.eq(owners.iter().map(|owner| &owner.value)))
            })
            .count()
            == 1
}

/// The alignment of an assembly scope. `stream_types` names the stream's
/// record types, which admit typed operand-path records.
pub(super) fn exact_assembly_alignment_with_types(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
    stream_types: &super::assembly_operand_paths::StreamTypes<'_>,
) -> Result<Option<DesignAssemblyAlignment>, CodecError> {
    if stream_types
        .get(&u64::from(scope.record_index))
        .is_some_and(|(guid, version)| {
            *version == 5 && guid.eq_ignore_ascii_case(super::assembly_v3::TYPE_GUID)
        })
    {
        return super::assembly_v3::alignment(
            bytes,
            records,
            scope,
            parameter_owners,
            stream_types,
        );
    }
    let existing =
        legacy_assembly_alignment(bytes, records, scope, parameter_owners, stream_types)?;
    if existing.is_some() {
        return Ok(existing);
    }
    super::assembly_v3::alignment(bytes, records, scope, parameter_owners, stream_types)
}

fn legacy_assembly_alignment(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
    stream_types: &super::assembly_operand_paths::StreamTypes<'_>,
) -> Result<Option<DesignAssemblyAlignment>, CodecError> {
    use assembly::DesignAssemblyAlignmentForm;
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Assemble) {
        return Ok(None);
    }
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let matching = |owner: &&DesignParameterOwner| {
        native_stream(owner.id()) == Some(stream)
            && owner.scope_record_index() == scope.record_index
    };

    let mut lanes = Vec::new();
    lanes.extend(parameter_owners.iter().filter(matching));
    lanes.sort_by(|left, right| {
        let left_key = {
            let owner = left;
            owner.local_ordinal()
        };
        let right_key = {
            let owner = right;
            owner.local_ordinal()
        };
        left_key.cmp(&right_key)
    });
    (|| -> Option<Result<DesignAssemblyAlignment, CodecError>> {
        if lanes
            .iter()
            .enumerate()
            .any(|(ordinal, owner)| u32::try_from(ordinal) != Ok(owner.local_ordinal()))
        {
            return None;
        }
        let as_built_421 = crate::design::assembly::legacy_as_built_421_generation(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        )
        .is_some();
        let legacy_class_383 = crate::design::assembly::legacy_class_383_258_scope(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        );
        let generation = crate::design::assembly::AssemblyScopeGeneration::for_scope(scope);
        let legacy_class_388 = matches!(
            generation.operand_frame_variant(),
            Some(crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388)
        );
        if legacy_class_388 {
            exact_legacy_class_388_scope(bytes, scope)?;
        }

        if as_built_421 {
            let exact = exact_legacy_as_built_421_alignment(bytes, scope, &lanes)?;
            let form = match exact_legacy_as_built_421_solved_frame(bytes, records, scope) {
                Some(solved_frame) => DesignAssemblyAlignmentForm::SolvedOnly {
                    solved_frame,
                    limits: Some(exact.limits),
                },
                None => DesignAssemblyAlignmentForm::LimitsOnly {
                    limits: exact.limits,
                },
            };
            return DesignAssemblyAlignment::try_new(
                exact.angle,
                exact.offset,
                exact.owners,
                Some(form),
            )
            .ok()
            .map(Ok);
        }
        let (angle, offset, owners) = {
            if matches!(scope.frame_length(), 671 | 744 | 748)
                && generation.operand_frame_variant().is_none()
            {
                return None;
            }
            // Only an `As-built` scope has the additional-owner run.
            if matches!(scope.frame_length(), 422 | 444)
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::AsBuilt
            {
                return None;
            }
            let (alignment_start, alignment_end) = generation.alignment_lane_bounds(lanes.len())?;
            let alignment_lanes = lanes.get(alignment_start..alignment_end)?;
            let placement_ordered = crate::design::assembly::placement_ordered_as_built_scope(scope);
            let (angle, offset) = match alignment_lanes {
                [offset_x, offset_y, offset_z, angle] if placement_ordered => (
                    angle.evaluated_value().get(),
                    [
                        offset_x.evaluated_value().get(),
                        offset_y.evaluated_value().get(),
                        offset_z.evaluated_value().get(),
                    ],
                ),
                [angle, offset_x, offset_y, offset_z] => (
                    angle.evaluated_value().get(),
                    [
                        offset_x.evaluated_value().get(),
                        offset_y.evaluated_value().get(),
                        offset_z.evaluated_value().get(),
                    ],
                ),
                [angle, axial_offset] => (
                    angle.evaluated_value().get(),
                    [0.0, 0.0, axial_offset.evaluated_value().get()],
                ),
                // A rigid as-built joint has no alignment owners.
                [] if crate::design::assembly::rigid_as_built_scope(scope) =>
                {
                    (0.0, [0.0; 3])
                }
                _ => return None,
            };
            // Both supported alignment lanes use one fixed four-owner allocation.
            let mut owners = Vec::new();
            for owner in alignment_lanes {
                owners.push(crate::records::identity::Located {
                    value: owner.record_index(),
                    offset: owner.evaluated_value_offset(),
                });
            }
            if legacy_class_388 {
                let owner_reference_order_matches = CLASS_388_OWNER_REFERENCE_ORDINALS
                    .into_iter()
                    .zip(lanes.iter())
                    .all(|(scope_ordinal, owner)| {
                        scope.reference_members().values().nth(scope_ordinal)
                            == Some(&owner.record_index())
                    });
                if lanes
                    .iter()
                    .any(|owner| owner.class_tag().as_str() != "282" || owner.frame_length() != 103)
                    || !owner_reference_order_matches
                    || !scope
                        .reference_members()
                        .values()
                        .skip(4)
                        .take(4)
                        .eq(owners.iter().map(|owner| &owner.value))
                {
                    return None;
                }
            } else if legacy_class_383 {
                let owner_reference_order_matches = CLASS_383_OWNER_REFERENCE_ORDINALS
                    .into_iter()
                    .zip(lanes.iter())
                    .all(|(scope_ordinal, owner)| {
                        scope.reference_members().values().nth(scope_ordinal)
                            == Some(&owner.record_index())
                    });
                if lanes
                    .iter()
                    .any(|owner| owner.class_tag().as_str() != "284" || owner.frame_length() != 103)
                    || !owner_reference_order_matches
                    || !scope
                        .reference_members()
                        .values()
                        .skip(8)
                        .take(4)
                        .eq(owners.iter().map(|owner| &owner.value))
                {
                    return None;
                }
            } else if !crate::design::assembly::rigid_as_built_scope(scope)
                && crate::design::assembly::variable_reference_assembly_generation(
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            ) {
                if lanes
                    .iter()
                    .any(|owner| owner.class_tag().as_str() != "289" || owner.frame_length() != 103)
                    || (0..scope.reference_members().len())
                        .filter(|&start| {
                            scope
                                .reference_members()
                                .values_in(start..start + owners.len())
                                .is_some_and(|values| {
                                    values.eq(owners.iter().map(|owner| &owner.value))
                                })
                        })
                        .count()
                        != 1
                {
                    return None;
                }
            } else if generation
                .operand_frame_variant()
                .is_some_and(crate::design::assembly::AssemblyOperandFrameVariant::is_wide_family)
            {
                // The wide scope lists its four owners first in its reference table.
                if !scope
                    .reference_members()
                    .values()
                    .take(owners.len())
                    .eq(owners.iter().map(|owner| &owner.value))
                {
                    return None;
                }
            } else if placement_ordered {
                if !placement_ordered_owner_run(bytes, scope, &lanes, &owners) {
                    return None;
                }
            } else if !scope
                .reference_members()
                .values()
                .rev()
                .take(owners.len())
                .eq(owners.iter().map(|owner| &owner.value).rev())
            {
                return None;
            }
            (angle, offset, owners)
        };
        let mut additional_members = Vec::new();
        let form = if scope.kind() == scope::DesignFeatureKind::AsBuilt {
            let paths = match exact_assembly_operand_paths(bytes, records, scope, stream_types) {
                Ok(paths) => paths,
                Err(error) => return Some(Err(error)),
            };
            // A rigid group's members after its first two tie to its first.
            if let Some(leading) = paths.as_ref().filter(|_| {
                crate::design::assembly::rigid_as_built_scope(scope)
            }) {
                let member_paths = match super::assembly_operand_paths::exact_rigid_group_member_paths(
                    bytes,
                    records,
                    scope,
                    stream_types,
                    leading,
                ) {
                    Ok(member_paths) => member_paths?,
                    Err(error) => return Some(Err(error)),
                };
                for path in member_paths {
                    let frame = super::assembly_operand_frames::exact_as_built_operand_frame(
                        bytes, &path, scope, stream_types,
                    )?;
                    additional_members.push(assembly::DesignQualifiedAssemblyOperand {
                        frame,
                        qualifier: DesignAssemblyOperandQualifier::OccurrencePath { path },
                    });
                }
            }
            paths
                .map(|paths| {
                    DesignAssemblyAlignmentForm::try_from(AsBuiltAlignmentDraft {
                        frames: exact_as_built_operand_frames(
                            bytes,
                            &paths,
                            scope,
                            stream_types,
                        ),
                        paths,
                    })
                })
                .transpose()
                .ok()?
        } else if let Some(frames) = exact_assembly_operand_frames(bytes, scope) {
            let qualifiers = if legacy_class_383 {
                let paths = match exact_legacy_class_383_operand_paths(bytes, records, scope, &frames) {
                    Ok(paths) => paths,
                    Err(error) => return Some(Err(error)),
                };
                paths.map(|paths| paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }))
            } else if legacy_class_388 {
                let paths = match exact_legacy_class_388_operand_paths(bytes, records, scope) {
                    Ok(paths) => paths,
                    Err(error) => return Some(Err(error)),
                };
                paths.map(|paths| {
                    paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })
                })
            } else if !crate::design::assembly::rigid_as_built_scope(scope)
                && crate::design::assembly::variable_reference_assembly_generation(
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            ) {
                let direct = match super::assembly_carrier_paths::exact_variable_reference_operand_qualifiers(bytes, records, scope, &frames) {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                };
                if direct.is_some() {
                    direct
                } else {
                    let paths = match exact_assembly_operand_paths(bytes, records, scope, stream_types) {
                        Ok(paths) => paths,
                        Err(error) => return Some(Err(error)),
                    };
                    paths.map(|paths| {
                        paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })
                    })
                }
            } else {
                let paths = match exact_assembly_operand_paths(bytes, records, scope, stream_types) {
                    Ok(paths) => paths,
                    Err(error) => return Some(Err(error)),
                };
                paths.map(|paths| {
                    paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })
                })
            };
            Some(match qualifiers {
                Some(qualifiers) => DesignAssemblyAlignmentForm::qualified(frames, qualifiers),
                None => DesignAssemblyAlignmentForm::Frames { frames },
            })
        } else {
            None
        };
        // The wide forms store how the operand frames mate: 1 opposed, 0
        // aligned.
        let sense_at = match generation.operand_frame_variant() {
            Some(crate::design::assembly::AssemblyOperandFrameVariant::Wide) => {
                Some(crate::layout::assembly_wide_scope::FRAME_SENSE)
            }
            Some(crate::design::assembly::AssemblyOperandFrameVariant::CompactWide) => {
                Some(crate::layout::assembly_compact_wide_scope::OPERAND_FLAG)
            }
            _ => None,
        };
        let frame_sense = sense_at
            .and_then(|at| usize::try_from(scope.byte_offset()).ok()?.checked_add(at))
            .and_then(|at| match bytes.get(at) {
                Some(1) => Some(assembly::DesignJointFrameSense::Opposed),
                Some(0) => Some(assembly::DesignJointFrameSense::Aligned),
                _ => None,
            });
        let additional_owner_run = if crate::design::assembly::placement_ordered_as_built_scope(scope) {
            let start = usize::try_from(scope.byte_offset()).ok()?;
            let count_at = start + as_built::ADDITIONAL_OWNER_COUNT;
            let count = View::u32_le_at(bytes, count_at)?;
            let references = lanes.iter().skip(4).enumerate().map(|(ordinal, _)| {
                let at = start + as_built::ADDITIONAL_OWNER_REFERENCES + 11 * ordinal;
                Some(crate::records::identity::Located {
                    value: super::shared_frames::marked_record_reference(bytes, at)?,
                    offset: cadmpeg_core::decode::u64_from_index(at),
                })
            }).collect::<Option<Vec<_>>>()?;
            Some(assembly::DesignAssemblyOwnerRun {
                count: crate::records::identity::Located {
                    value: count,
                    offset: cadmpeg_core::decode::u64_from_index(count_at),
                },
                references,
            })
        } else {
            None
        };
        DesignAssemblyAlignment::try_new(angle, offset, owners, form)
            .and_then(|mut alignment| {
                alignment.additional_owner_run = additional_owner_run;
                alignment
                    .with_frame_sense(frame_sense)
                    .with_additional_members(additional_members)
            })
            .ok()
            .map(Ok)
    })()
    .transpose()
}
