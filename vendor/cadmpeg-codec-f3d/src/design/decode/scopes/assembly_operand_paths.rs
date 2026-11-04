// SPDX-License-Identifier: Apache-2.0
//! Exact assembly operand paths and their locator envelopes.

use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::assembly::RigidGroupLocatorLayout;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::layout::assembly_operand_path_locator_reference_run as path_locator_run;
use crate::layout::assembly_operand_path_wrapper as path_wrapper;
use crate::layout::assembly_variable_reference_operand_path_locator as variable_path_locator;
use crate::layout::assembly_wide_operand_path_identity_locator as wide_identity;
use crate::layout::assembly_wide_operand_path_locator as wide_locator;
use crate::layout::assembly_wide_operand_path_matrix_locator as wide_matrix;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;

/// Type GUID of the operand-path locator of the 945-byte `Assemble` scope.
pub(super) const WIDE_PATH_LOCATOR_TYPE_GUID: &str = "2E3AC990-483D-4A55-8662-C064B976A957";
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Record types of one Design stream by entity id: type GUID and version.
pub(super) type StreamTypes<'a> = HashMap<u64, (&'a str, u32)>;

pub(super) fn exact_assembly_operand_paths(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &StreamTypes<'_>,
) -> Result<Option<[DesignAssemblyOperandPath; 2]>, CodecError> {
    (|| {
        let scope_at = usize::try_from(scope.byte_offset()).ok()?;
        let search_start = usize::try_from(scope.paired_byte_offset())
            .ok()?
            .checked_add(11)?;
        let rigid = crate::design::assembly::RigidGroupLayout::for_scope(scope);
        let v3 = super::assembly_v3::parse(bytes, scope, stream_types);
        let locator_offsets = v3
            .as_ref()
            .map(|grammar| grammar.locator_offsets)
            .or_else(|| {
                crate::design::assembly::AssemblyScopeGeneration::for_scope(scope)
                    .operand_path_locator_offsets()
            })?;
        let count_at = scope_at.checked_add(rigid.map_or(
            locator_offsets[0].checked_sub(path_locator_run::FIRST_LOCATOR_REFERENCE)?,
            |layout| layout.count_at,
        ))?;
        let expected = rigid.map_or(2, |layout| layout.members);
        let typed_references = v3
            .as_ref()
            .is_some_and(|grammar| grammar.stride == crate::bytes::TYPED_LOCAL_REFERENCE_LEN)
            || rigid.is_some_and(|layout| layout.stride == crate::bytes::TYPED_LOCAL_REFERENCE_LEN);
        if usize::try_from(View::u32_le_at(bytes, count_at)?).ok()? != expected {
            return None;
        }
        let mut paths = [None, None];
        for (ordinal, relative_offset) in locator_offsets.into_iter().enumerate() {
            let locator_reference_at = scope_at.checked_add(relative_offset)?;
            let (locator_record_index, locator_reference_offset) =
                local_record_reference(bytes, locator_reference_at, typed_references)?;
            let candidates = records
                .offsets(locator_record_index)
                .iter()
                .copied()
                .filter(|locator_at| *locator_at >= search_start);
            let mut candidate = None;
            for locator_at in candidates {
                let parsed = match exact_assembly_operand_path_envelope(
                    bytes,
                    scope,
                    stream_types,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                ) {
                    Ok(parsed) => parsed,
                    Err(error) => return Some(Err(error)),
                };
                if let Some(parsed) = parsed {
                    if candidate.replace(parsed).is_some() {
                        return None;
                    }
                }
            }
            paths[ordinal] = Some(candidate?);
        }
        let [Some(first), Some(second)] = paths else {
            return None;
        };
        let first_start = usize::try_from(first.link().locator_byte_offset).ok()?;
        let first_end = next_indexed_record_offset(
            bytes,
            usize::try_from(first.link().wrapper_byte_offset)
                .ok()?
                .checked_add(1)?,
        )?;
        let second_start = usize::try_from(second.link().locator_byte_offset).ok()?;
        let second_end = next_indexed_record_offset(
            bytes,
            usize::try_from(second.link().wrapper_byte_offset)
                .ok()?
                .checked_add(1)?,
        )?;
        if first.link().locator_record_index == second.link().locator_record_index
            || (first_start < second_end && second_start < first_end)
        {
            return None;
        }
        Some(Ok([first, second]))
    })()
    .transpose()
}

/// The operand paths of a rigid group's members after its first two, in
/// locator order. Each member's locator reference follows the previous one;
/// every locator is a distinct record whose envelope overlaps no other
/// member's.
pub(super) fn exact_rigid_group_member_paths(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &StreamTypes<'_>,
    leading: &[DesignAssemblyOperandPath; 2],
) -> Result<Option<Vec<DesignAssemblyOperandPath>>, CodecError> {
    (|| {
        let layout = crate::design::assembly::RigidGroupLayout::for_scope(scope)?;
        let typed_references = layout.stride == crate::bytes::TYPED_LOCAL_REFERENCE_LEN;
        let scope_at = usize::try_from(scope.byte_offset()).ok()?;
        let search_start = usize::try_from(scope.paired_byte_offset())
            .ok()?
            .checked_add(11)?;
        let mut paths = Vec::new();
        for member in 2..layout.members {
            let locator_reference_at = scope_at
                .checked_add(layout.first_reference)?
                .checked_add(member.checked_mul(layout.stride)?)?;
            let (locator_record_index, locator_reference_offset) =
                local_record_reference(bytes, locator_reference_at, typed_references)?;
            let mut candidate = None;
            for locator_at in records
                .offsets(locator_record_index)
                .iter()
                .copied()
                .filter(|locator_at| *locator_at >= search_start)
            {
                let parsed = match exact_assembly_operand_path_envelope(
                    bytes,
                    scope,
                    stream_types,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                ) {
                    Ok(parsed) => parsed,
                    Err(error) => return Some(Err(error)),
                };
                if let Some(parsed) = parsed {
                    if candidate.replace(parsed).is_some() {
                        return None;
                    }
                }
            }
            paths.push(candidate?);
        }
        let mut spans = Vec::new();
        for path in leading.iter().chain(&paths) {
            let start = usize::try_from(path.link().locator_byte_offset).ok()?;
            let end = next_indexed_record_offset(
                bytes,
                usize::try_from(path.link().wrapper_byte_offset)
                    .ok()?
                    .checked_add(1)?,
            )?;
            if spans.iter().any(|(index, other_start, other_end)| {
                *index == path.link().locator_record_index
                    || (start < *other_end && *other_start < end)
            }) {
                return None;
            }
            spans.push((path.link().locator_record_index, start, end));
        }
        Some(Ok(paths))
    })()
    .transpose()
}

fn local_record_reference(bytes: &[u8], at: usize, typed: bool) -> Option<(u32, u64)> {
    Some((
        crate::bytes::local_reference_target(bytes, at, typed)?,
        u64::try_from(at.checked_add(1)?).ok()?,
    ))
}

fn exact_assembly_operand_path_envelope(
    bytes: &[u8],
    scope: &DesignParameterScope,
    stream_types: &StreamTypes<'_>,
    locator_record_index: u32,
    locator_reference_offset: u64,
    locator_at: usize,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    (|| {
        let locator_class_tag = exact_indexed_header_at(bytes, locator_at, locator_record_index)?;
        let variable_reference = !super::assembly_v3::registered(scope, stream_types)
            && !crate::design::assembly::rigid_as_built_scope(scope)
            && crate::design::assembly::variable_reference_assembly_generation(
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            );
        let wide_family = crate::design::assembly::AssemblyScopeGeneration::for_scope(scope)
            .operand_frame_variant()
            .is_some_and(crate::design::assembly::AssemblyOperandFrameVariant::is_wide_family)
            || crate::design::assembly::wide_as_built_scope(scope);
        let registered_locator = stream_types
            .get(&u64::from(locator_record_index))
            .filter(|(guid, _)| guid.eq_ignore_ascii_case(WIDE_PATH_LOCATOR_TYPE_GUID));
        let locator_end = next_indexed_record_offset(bytes, locator_at.checked_add(1)?)?;
        let assemble = super::assembly_v3::registered(scope, stream_types);
        let group_layout = if crate::design::assembly::rigid_as_built_scope(scope)
            || super::assembly_v3::registered(scope, stream_types)
        {
            Some(if assemble {
                RigidGroupLocatorLayout::admit_assemble(
                    registered_locator.copied(),
                    locator_end.checked_sub(locator_at)?,
                )?
            } else {
                RigidGroupLocatorLayout::admit(
                    registered_locator.copied(),
                    locator_end.checked_sub(locator_at)?,
                )?
            })
        } else {
            None
        };
        let wide = group_layout.map_or(wide_family && registered_locator.is_some(), |layout| {
            layout.wide
        });
        let typed = group_layout.is_some_and(|layout| layout.typed);
        let reference_stride = group_layout.map_or(11, RigidGroupLocatorLayout::reference_stride);
        if let Some(layout) = group_layout {
            if bytes.get(locator_at.checked_add(layout.identity_flag)?)?
                != &u8::from(layout.transform.is_none())
            {
                return None;
            }
        }
        let (locator_length, scope_backlink, wrapper_reference, constant_two, zero_tail) =
            if let Some(layout) = group_layout.filter(|layout| layout.wide && assemble) {
                if bytes
                    .get(
                        locator_at.checked_add(wide_locator::ZERO_RUN_28)?
                            ..locator_at.checked_add(wide_locator::PREFIX_WORD)?,
                    )?
                    .iter()
                    .any(|byte| *byte != 0)
                    || !matches!(
                        View::u32_le_at(bytes, locator_at.checked_add(wide_locator::PREFIX_WORD)?)?,
                        crate::layout::assembly_locator_prefix_two::WORD_VALUE
                            | crate::layout::assembly_locator_prefix_three::WORD_VALUE
                    )
                    || bytes
                        .get(
                            locator_at.checked_add(wide_locator::ZERO_RUN_13)?
                                ..locator_at.checked_add(layout.construction_reference)?,
                        )?
                        .iter()
                        .any(|byte| *byte != 0)
                    || if layout.null_construction {
                        bytes.get(locator_at.checked_add(layout.construction_reference)?)? != &0
                    } else {
                        exact_same_segment_record_reference(
                            bytes,
                            locator_at.checked_add(layout.construction_reference)?,
                        )?
                        .0 == 0
                    }
                    || bytes.get(locator_at.checked_add(layout.scope_backlink.checked_sub(1)?)?)
                        != Some(&0)
                {
                    return None;
                }
                if let Some(transform) = layout.transform {
                    rigid_transform_at(bytes, locator_at.checked_add(transform)?)?;
                }
                layout.offsets()
            } else if wide {
                if bytes
                    .get(
                        locator_at.checked_add(wide_locator::ZERO_RUN_28)?
                            ..locator_at.checked_add(wide_locator::PREFIX_WORD)?,
                    )?
                    .iter()
                    .any(|byte| *byte != 0)
                    || View::u32_le_at(bytes, locator_at.checked_add(wide_locator::PREFIX_WORD)?)?
                        != 3
                    || bytes
                        .get(
                            locator_at.checked_add(wide_locator::ZERO_RUN_13)?
                                ..locator_at.checked_add(wide_locator::NONZERO_RECORD_REFERENCE)?,
                        )?
                        .iter()
                        .any(|byte| *byte != 0)
                    || exact_same_segment_record_reference(
                        bytes,
                        locator_at.checked_add(wide_locator::NONZERO_RECORD_REFERENCE)?,
                    )?
                    .0 == 0
                {
                    return None;
                }
                let (offsets, separator) =
                    match bytes.get(locator_at.checked_add(wide_locator::IDENTITY_FLAG)?)? {
                        1 => (
                            (
                                wide_identity::LEN,
                                wide_identity::SCOPE_BACKLINK,
                                wide_identity::WRAPPER_REFERENCE,
                                wide_identity::DISCRIMINATOR,
                                wide_identity::ZERO_TAIL,
                            ),
                            wide_identity::ZERO_SEPARATOR,
                        ),
                        0 => {
                            rigid_transform_at(
                                bytes,
                                locator_at.checked_add(wide_locator::TRANSFORM)?,
                            )?;
                            (
                                (
                                    wide_matrix::LEN,
                                    wide_matrix::SCOPE_BACKLINK,
                                    wide_matrix::WRAPPER_REFERENCE,
                                    wide_matrix::DISCRIMINATOR,
                                    wide_matrix::ZERO_TAIL,
                                ),
                                wide_matrix::ZERO_SEPARATOR,
                            )
                        }
                        _ => return None,
                    };
                if bytes.get(locator_at.checked_add(separator)?) != Some(&0)
                    || group_layout.is_some_and(|layout| layout.offsets() != offsets)
                {
                    return None;
                }
                offsets
            } else if let Some(layout) = group_layout {
                let construction_at = locator_at.checked_add(layout.construction_reference)?;
                if bytes.get(locator_at.checked_add(path_locator::ZERO_RUN_10)?..construction_at)?
                    != [0; 10]
                    || if layout.null_construction {
                        bytes.get(construction_at)? != &0
                    } else {
                        local_record_reference(bytes, construction_at, typed)?.0 == 0
                    }
                    || (!typed
                        && bytes
                            .get(locator_at.checked_add(layout.scope_backlink.checked_sub(1)?)?)
                            != Some(&0))
                {
                    return None;
                }
                if let Some(transform) = layout.transform {
                    rigid_transform_at(bytes, locator_at.checked_add(transform)?)?;
                }
                layout.offsets()
            } else if variable_reference {
                if locator_class_tag != "390"
                    || bytes
                        .get(
                            locator_at.checked_add(variable_path_locator::ZERO_SEPARATOR)?
                                ..locator_at.checked_add(variable_path_locator::SCOPE_BACKLINK)?,
                        )?
                        .iter()
                        .any(|byte| *byte != 0)
                    || rigid_transform_at(
                        bytes,
                        locator_at.checked_add(variable_path_locator::TRANSFORM)?,
                    )
                    .is_none()
                {
                    return None;
                }
                (
                    variable_path_locator::LEN,
                    variable_path_locator::SCOPE_BACKLINK,
                    variable_path_locator::WRAPPER_REFERENCE,
                    variable_path_locator::CONSTANT_TWO,
                    variable_path_locator::ZERO_TAIL,
                )
            } else {
                if bytes.get(
                    locator_at.checked_add(path_locator::ZERO_RUN_10)?
                        ..locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
                )? != [0; 10]
                    || exact_same_segment_record_reference(
                        bytes,
                        locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
                    )?
                    .0 == 0
                    || bytes.get(locator_at.checked_add(path_locator::ZERO_32)?) != Some(&0)
                    || rigid_transform_at(bytes, locator_at.checked_add(path_locator::TRANSFORM)?)
                        .is_none()
                    || bytes.get(locator_at.checked_add(path_locator::ZERO_161)?) != Some(&0)
                {
                    return None;
                }
                (
                    path_locator::LEN,
                    path_locator::SCOPE_BACKLINK,
                    path_locator::WRAPPER_REFERENCE,
                    path_locator::CONSTANT_TWO,
                    path_locator::ZERO_TAIL_2,
                )
            };
        let (scope_record_index, locator_scope_reference_offset) =
            local_record_reference(bytes, locator_at.checked_add(scope_backlink)?, typed)?;
        let (wrapper_record_index, wrapper_reference_offset) =
            local_record_reference(bytes, locator_at.checked_add(wrapper_reference)?, typed)?;
        let path_record_index = locator_record_index.checked_add(1)?;
        // A variable-reference or wide locator's path can continue over
        // further path records before its wrapper.
        let counted_wrapper = variable_reference || wide || group_layout.is_some();
        if scope_record_index != scope.record_index
            || if counted_wrapper {
                !(locator_record_index.checked_add(2)?..=locator_record_index.checked_add(65)?)
                    .contains(&wrapper_record_index)
            } else {
                wrapper_record_index != locator_record_index.checked_add(2)?
            }
            || View::u32_le_at(bytes, locator_at.checked_add(constant_two)?)? != 2
            || bytes
                .get(locator_at.checked_add(zero_tail)?..locator_at.checked_add(locator_length)?)?
                != [0; 2]
        {
            return None;
        }
        let path_at = locator_at.checked_add(locator_length)?;
        if locator_end != path_at {
            return None;
        }
        if wrapper_record_index < path_record_index {
            return None;
        }

        let mut path_spans = Vec::new();
        let mut record_index = path_record_index;
        let mut record_at = path_at;
        let wrapper_at = loop {
            if record_index == wrapper_record_index {
                break record_at;
            }
            let next = next_indexed_record_offset(bytes, record_at.checked_add(1)?)?;
            path_spans.push((record_index, record_at, next));
            record_index = record_index.checked_add(1)?;
            record_at = next;
        };
        let wrapper_class_tag = exact_indexed_header_at(bytes, wrapper_at, wrapper_record_index)?;
        let wrapper_end = next_indexed_record_offset(bytes, wrapper_at.checked_add(1)?)?;
        let expected_wrapper_length = if counted_wrapper {
            path_wrapper::PATH_REFERENCE
                .checked_add(path_spans.len().checked_mul(reference_stride)?)?
        } else {
            path_wrapper::LEN
        };
        if variable_reference && wrapper_class_tag != "397"
            || bytes.get(
                wrapper_at.checked_add(path_wrapper::ZERO_RUN_10)?
                    ..wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)?,
            )? != [0; 10]
            || bytes.get(wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)?) != Some(&1)
            || View::u32_le_at(
                bytes,
                wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_WORD)?,
            )? != if counted_wrapper {
                u32::try_from(path_spans.len()).ok()?
            } else {
                1
            }
            || wrapper_end != wrapper_at.checked_add(expected_wrapper_length)?
        {
            return None;
        }
        let (referenced_path_record_index, path_reference_offset) = local_record_reference(
            bytes,
            wrapper_at.checked_add(path_wrapper::PATH_REFERENCE)?,
            typed,
        )?;
        if referenced_path_record_index != path_record_index
            || path_spans
                .iter()
                .enumerate()
                .any(|(ordinal, (index, _, _))| {
                    local_record_reference(
                        bytes,
                        wrapper_at + path_wrapper::PATH_REFERENCE + ordinal * reference_stride,
                        typed,
                    )
                    .map(|reference| reference.0)
                        != Some(*index)
                })
        {
            return None;
        }
        let link = DesignAssemblyOperandPathLink {
            locator_reference_offset,
            locator_record_index,
            locator_class_tag: locator_class_tag.try_into().ok()?,
            locator_byte_offset: u64::try_from(locator_at).ok()?,
            locator_scope_reference_offset,
            wrapper_record_index,
            wrapper_reference_offset,
            wrapper_class_tag: wrapper_class_tag.try_into().ok()?,
            wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
            path_reference_offset,
        };
        let mut paths = path_spans.into_iter().map(|(record_index, start, limit)| {
            if super::assembly_v3::registered(scope, stream_types)
                && !stream_types
                    .get(&u64::from(record_index))
                    .is_some_and(|(guid, version)| {
                        guid.eq_ignore_ascii_case(TYPED_OCCURRENCE_PATH_TYPE_GUID) && *version == 4
                    })
            {
                return Ok(None);
            }
            let type_guid = stream_types
                .get(&u64::from(record_index))
                .map(|(guid, _)| *guid)
                .filter(|guid| guid.eq_ignore_ascii_case(TYPED_OCCURRENCE_PATH_TYPE_GUID));
            exact_assembly_operand_path(bytes, start, record_index, limit, link.clone(), type_guid)
        });
        let mut path = match paths.next()? {
            Ok(Some(path)) => path,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if variable_reference && path.class_tag().as_str() != "330" {
            return None;
        }
        for continuation in paths {
            let continuation = match continuation {
                Ok(Some(path)) => path,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            let continues = if variable_reference {
                continuation.class_tag().as_str() == "330"
            } else {
                (wide || group_layout.is_some())
                    && path.admitted_by_type()
                    && continuation.admitted_by_type()
            };
            if !continues {
                return None;
            }
            path = path.try_append(continuation)?;
        }
        Some(Ok(path))
    })()
    .transpose()
}

/// Read one path record. `type_guid` is the record's type GUID when it is the
/// identity-qualified counted path type, which admits it whatever its class.
fn exact_assembly_operand_path(
    bytes: &[u8],
    start: usize,
    record_index: u32,
    limit: usize,
    link: DesignAssemblyOperandPathLink,
    type_guid: Option<&str>,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    (|| {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 1..=8, u8::is_ascii_digit)?;
        if View::u64_le_at(bytes, after_tag)? != u64::from(record_index) {
            return None;
        }
        let mut occurrence_guids = Vec::new();
        let mut identity_guids = Vec::new();
        let mut paired_occurrence_keys = Vec::new();
        match class_tag {
            "294" | "299" | "307" if type_guid.is_none() => {
                let end = next_indexed_record_offset(bytes, start + 1)?;
                if end != limit
                    || bytes.get(after_tag + 8..after_tag + 14)? != [0; 6]
                    || bytes.get(after_tag + 14) != Some(&1)
                    || bytes.get(after_tag + 15..after_tag + 18)? != [0; 3]
                {
                    return None;
                }
                let mut position = after_tag + 18;
                let (occurrence, after_occurrence) =
                    match fixed_relaxed_guid_text(bytes.get(..end)?, position) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                occurrence_guids.push(crate::records::identity::Located {
                    value: occurrence.into(),
                    offset: u64::try_from(position + 4).ok()?,
                });
                position = after_occurrence;
                for _ in 0..2 {
                    let (guid, after_guid) =
                        match fixed_relaxed_guid_text(bytes.get(..end)?, position) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                    identity_guids.push(crate::records::identity::Located {
                        value: guid,
                        offset: u64::try_from(position + 4).ok()?,
                    });
                    position = after_guid;
                }
                if View::u64_le_at(bytes, position)? != 2 {
                    return None;
                }
                position += 8;
                for _ in 0..2 {
                    let (guid, after_guid) =
                        match fixed_relaxed_guid_text(bytes.get(..end)?, position) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                    identity_guids.push(crate::records::identity::Located {
                        value: guid,
                        offset: u64::try_from(position + 4).ok()?,
                    });
                    position = after_guid;
                }
                if View::u32_le_at(bytes, position)? != 2
                    || !bytes.get(position + 4..end)?.iter().all(|byte| *byte == 0)
                {
                    return None;
                }
            }
            tag if type_guid.is_some() || matches!(tag, "329" | "330" | "386" | "390") => {
                if bytes.get(after_tag + 8..after_tag + 14)? != [0; 6] {
                    return None;
                }
                let count = usize::try_from(View::u32_le_at(bytes, after_tag + 14)?).ok()?;
                if count == 0 && type_guid.is_some() {
                    use crate::layout::assembly_root_operand_path as root;
                    if start.checked_add(root::LEN)? != limit
                        || View::u32_le_at(bytes, start.checked_add(root::EMPTY_REFERENCE)?)? != 0
                        || View::u64_le_at(bytes, start.checked_add(root::CONSTANT_ONE)?)? != 1
                        || View::u32_le_at(bytes, start.checked_add(root::CONSTANT_TWO)?)? != 2
                        || bytes.get(start.checked_add(root::ZERO_TAIL)?..limit)? != [0; 8]
                    {
                        return None;
                    }
                    let (key, _) =
                        match fixed_relaxed_guid_text(bytes, start.checked_add(root::ROOT_KEY)?) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                    if key.as_str() != "00000000-0000-0000-0000-000000000000" {
                        return None;
                    }
                    occurrence_guids.push(crate::records::identity::Located {
                        value: key.into(),
                        offset: u64::try_from(start.checked_add(root::ROOT_KEY + 4)?).ok()?,
                    });
                    for relative in [root::FIRST_IDENTITY, root::SECOND_IDENTITY] {
                        let at = start.checked_add(relative)?;
                        let (guid, _) = match fixed_relaxed_guid_text(bytes, at) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                        identity_guids.push(crate::records::identity::Located {
                            value: guid,
                            offset: u64::try_from(at.checked_add(4)?).ok()?,
                        });
                    }
                } else {
                    if !(1..=64).contains(&count) {
                        return None;
                    }

                    let mut position = after_tag + 18;
                    for _ in 0..count {
                        let (guid, after_guid) =
                            match crate::design::decode::text::occurrence_key_text(
                                bytes.get(..limit)?,
                                position,
                            ) {
                                Ok(Some(value)) => value,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                        occurrence_guids.push(crate::records::identity::Located {
                            value: guid,
                            offset: u64::try_from(position + 4).ok()?,
                        });
                        position = after_guid;
                    }
                    if position == limit {
                        if !matches!(class_tag, "329" | "330") {
                            return None;
                        }
                    } else {
                        for _ in 0..2 {
                            let (guid, after_guid) =
                                match fixed_relaxed_guid_text(bytes.get(..limit)?, position) {
                                    Ok(Some(value)) => value,
                                    Ok(None) => return None,
                                    Err(error) => return Some(Err(error)),
                                };
                            identity_guids.push(crate::records::identity::Located {
                                value: guid,
                                offset: u64::try_from(position + 4).ok()?,
                            });
                            position = after_guid;
                        }
                        if View::u64_le_at(bytes, position)? != 2 {
                            return None;
                        }
                        position += 8;
                        for _ in 0..2 {
                            let (guid, after_guid) =
                                match fixed_relaxed_guid_text(bytes.get(..limit)?, position) {
                                    Ok(Some(value)) => value,
                                    Ok(None) => return None,
                                    Err(error) => return Some(Err(error)),
                                };
                            identity_guids.push(crate::records::identity::Located {
                                value: guid,
                                offset: u64::try_from(position + 4).ok()?,
                            });
                            position = after_guid;
                        }
                        if View::u32_le_at(bytes, position)? != 2 {
                            return None;
                        }
                        position += 4;
                        // A typed path can end in a zero word, a pair count, and
                        // per pair the u32 2 and two occurrence identities; a zero
                        // tail has no pairs, and only a typed path has pairs.
                        if !bytes.get(position..limit)?.iter().all(|byte| *byte == 0) {
                            if type_guid.is_none() || View::u32_le_at(bytes, position)? != 0 {
                                return None;
                            }
                            let pairs =
                                usize::try_from(View::u32_le_at(bytes, position + 4)?).ok()?;
                            if !(1..=64).contains(&pairs) {
                                return None;
                            }
                            position += 8;
                            for _ in 0..pairs {
                                if View::u32_le_at(bytes, position)? != 2 {
                                    return None;
                                }
                                position += 4;
                                for _ in 0..2 {
                                    let (key, after_key) =
                                        match crate::design::decode::text::occurrence_key_text(
                                            bytes.get(..limit)?,
                                            position,
                                        ) {
                                            Ok(Some(value)) => value,
                                            Ok(None) => return None,
                                            Err(error) => return Some(Err(error)),
                                        };
                                    paired_occurrence_keys.push(
                                        crate::records::identity::Located {
                                            value: key,
                                            offset: u64::try_from(position + 4).ok()?,
                                        },
                                    );
                                    position = after_key;
                                }
                            }
                            if !bytes.get(position..limit)?.iter().all(|byte| *byte == 0) {
                                return None;
                            }
                        }
                    }
                }
            }
            _ => return None,
        }
        let type_guid = match type_guid {
            Some(guid) => Some(guid.to_owned().try_into().ok()?),
            None => None,
        };
        DesignAssemblyOperandPath::try_new_typed(
            link,
            record_index,
            crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
            type_guid,
            u64::try_from(start).ok()?,
            occurrence_guids,
            identity_guids,
        )
        .and_then(|path| path.with_paired_occurrence_keys(paired_occurrence_keys))
        .ok()
        .map(Ok)
    })()
    .transpose()
}

#[cfg(test)]
mod tests;
