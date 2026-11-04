// SPDX-License-Identifier: Apache-2.0
//! Exact legacy class 383, 388 and 412 assembly operand paths.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::{fixed_guid_end, fixed_relaxed_guid_text, fixed_utf16_ascii_eq};
use crate::layout::assembly_class_383_258_frame_359_identity as class_383_identity;
use crate::layout::assembly_class_383_258_frame_378_carrier as class_383_carrier;
use crate::layout::assembly_class_383_258_frame_387_child as class_383_child;
use crate::layout::assembly_class_383_258_frame_387_leading as class_383_leading;
use crate::layout::assembly_class_383_258_frame_394 as class_383_face;
use crate::layout::assembly_class_383_258_scope_1011 as class_383_scope;
use crate::layout::assembly_class_388_266_scope_968 as class_388_assemble;
use crate::layout::assembly_legacy_class_369_path_wrapper_one as class_369_wrapper_one;
use crate::layout::assembly_legacy_class_369_path_wrapper_two as class_369_wrapper_two;
use crate::layout::assembly_legacy_class_412_path_425 as class_412_path;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

pub(super) fn exact_legacy_class_388_scope(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<()> {
    if scope.class_tag.as_str() != "388"
        || scope.paired_class_tag.as_str() != "266"
        || scope.frame_length() != u64_from_index(class_388_assemble::LEN)
        || scope.reference_members().len()
            != index_from_u32(class_388_assemble::REFERENCE_COUNT_VALUE)
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let paired = usize::try_from(scope.paired_byte_offset()).ok()?;
    let zero_prefix = bytes.get(start + 11..start + class_388_assemble::SCOPE_FLAGS)?;
    let scope_flags = bytes.get(
        start + class_388_assemble::SCOPE_FLAGS..start + class_388_assemble::SCOPE_FLAGS + 6,
    )?;
    let zero_operand_prefix =
        bytes.get(start + 26..start + class_388_assemble::FIRST_OPERAND_REFERENCE)?;
    let first_separator = bytes.get(start + 39);
    let second_separator = bytes.get(start + 179);
    let operand_path_locator_count = View::u32_le_at(
        bytes,
        start + class_388_assemble::OPERAND_PATH_LOCATOR_COUNT,
    )?;
    let reference_trailer = bytes.get(
        start + class_388_assemble::REFERENCE_TRAILER
            ..start + class_388_assemble::REFERENCE_TRAILER + 4,
    )?;
    let kind_code_unit_count =
        View::u32_le_at(bytes, start + class_388_assemble::KIND_CODE_UNIT_COUNT)?;
    if paired != start.checked_add(class_388_assemble::LEN)? {
        return None;
    }
    if zero_prefix != [0; class_388_assemble::SCOPE_FLAGS - 11] {
        return None;
    }
    if scope_flags != class_388_assemble::SCOPE_FLAGS_VALUE {
        return None;
    }
    if zero_operand_prefix != [0; 2] {
        return None;
    }
    if first_separator != Some(&0) {
        return None;
    }
    if second_separator != Some(&0) {
        return None;
    }
    if operand_path_locator_count != class_388_assemble::OPERAND_PATH_LOCATOR_COUNT_VALUE {
        return None;
    }
    if reference_trailer != class_388_assemble::REFERENCE_TRAILER_VALUE {
        return None;
    }
    if kind_code_unit_count != class_388_assemble::KIND_CODE_UNIT_COUNT_VALUE {
        return None;
    }
    let operand_path_locator_references = [
        start + class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES,
        start + class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES + 11,
    ];
    let [Some(first_locator), Some(second_locator)] =
        operand_path_locator_references.map(|at| marked_record_reference(bytes, at))
    else {
        return None;
    };
    if first_locator == 0 || second_locator == 0 || first_locator == second_locator {
        return None;
    }
    let external_component = marked_record_reference(
        bytes,
        start + class_388_assemble::EXTERNAL_COMPONENT_REFERENCE,
    )?;
    if external_component == 0 {
        return None;
    }
    let identity_end = fixed_guid_end(bytes, start + class_388_assemble::COMPONENT_IDENTITY)?;
    if identity_end != start + class_388_assemble::COMPONENT_IDENTITY + 76 {
        return None;
    }
    let kind_end = fixed_utf16_ascii_eq(
        bytes,
        start + class_388_assemble::KIND_CODE_UNIT_COUNT,
        "Assemble",
    )?;
    if kind_end != start + class_388_assemble::FEATURE_ORDINAL
        || View::u32_le_at(bytes, start + class_388_assemble::FEATURE_ORDINAL)?
            != scope.feature_ordinal.get()
    {
        return None;
    }
    for (ordinal, record_index) in scope.reference_members().values().enumerate() {
        let at = start
            .checked_add(class_388_assemble::REFERENCE_ENTRIES)?
            .checked_add(ordinal.checked_mul(ASSEMBLY_MARKED_REFERENCE_LEN)?)?;
        if marked_record_reference(bytes, at) != Some(*record_index) {
            return None;
        }
    }
    Some(())
}

pub(super) const ASSEMBLY_MARKED_REFERENCE_LEN: usize = 11;

pub(super) const CLASS_388_OWNER_REFERENCE_ORDINALS: [usize; 28] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 30, 31,
    32, 33,
];

pub(super) const CLASS_383_OWNER_REFERENCE_ORDINALS: [usize; 20] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 20, 21, 22, 23, 33, 34, 35, 36,
];

#[derive(Clone, Copy)]
struct LegacyClass383OperandSpec {
    leading_ordinal: usize,
    leading_identity_ordinal: usize,
    child_ordinal: usize,
    child_identity_ordinal: usize,
    first_face_ordinal: usize,
    first_face_identity_ordinal: usize,
    second_face_ordinal: usize,
    second_face_identity_ordinal: usize,
    placement_owner_start: usize,
    carrier_ordinal: usize,
    scope_operand_reference_offset: usize,
}

const CLASS_383_OPERAND_SPECS: [LegacyClass383OperandSpec; 2] = [
    LegacyClass383OperandSpec {
        leading_ordinal: 12,
        leading_identity_ordinal: 13,
        child_ordinal: 14,
        child_identity_ordinal: 15,
        first_face_ordinal: 16,
        first_face_identity_ordinal: 17,
        second_face_ordinal: 18,
        second_face_identity_ordinal: 19,
        placement_owner_start: 20,
        carrier_ordinal: 24,
        scope_operand_reference_offset: class_383_scope::FIRST_OPERAND_REFERENCE,
    },
    LegacyClass383OperandSpec {
        leading_ordinal: 25,
        leading_identity_ordinal: 26,
        child_ordinal: 27,
        child_identity_ordinal: 28,
        first_face_ordinal: 29,
        first_face_identity_ordinal: 30,
        second_face_ordinal: 31,
        second_face_identity_ordinal: 32,
        placement_owner_start: 33,
        carrier_ordinal: 37,
        scope_operand_reference_offset: class_383_scope::SECOND_OPERAND_REFERENCE,
    },
];

pub(super) fn exact_legacy_class_383_operand_paths(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frames: &[DesignAssemblyOperandFrame; 2],
) -> Result<Option<[DesignAssemblyOperandPath; 2]>, CodecError> {
    (|| {
        if !crate::design::assembly::legacy_class_383_258_scope(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        ) || scope.reference_members().len() != 38
        {
            return None;
        }
        let [first, second] = CLASS_383_OPERAND_SPECS;
        Some(Ok([
            match exact_legacy_class_383_operand_path(bytes, records, scope, &frames[0], first) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            },
            match exact_legacy_class_383_operand_path(bytes, records, scope, &frames[1], second) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            },
        ]))
    })()
    .transpose()
}

fn exact_legacy_class_383_operand_path(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
    spec: LegacyClass383OperandSpec,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    (|| {
        let member = |ordinal| scope.reference_members().values().nth(ordinal).copied();
        let leading_record_index = member(spec.leading_ordinal)?;
        let leading_identity_record_index = member(spec.leading_identity_ordinal)?;
        let child_record_index = member(spec.child_ordinal)?;
        let child_identity_record_index = member(spec.child_identity_ordinal)?;
        let first_face_record_index = member(spec.first_face_ordinal)?;
        let first_face_identity_record_index = member(spec.first_face_identity_ordinal)?;
        let second_face_record_index = member(spec.second_face_ordinal)?;
        let second_face_identity_record_index = member(spec.second_face_identity_ordinal)?;
        let carrier_record_index = member(spec.carrier_ordinal)?;
        let [Some(a), Some(b), Some(c), Some(d)] =
            [0, 1, 2, 3].map(|ordinal| member(spec.placement_owner_start.checked_add(ordinal)?))
        else {
            return None;
        };
        let placement_owners = [a, b, c, d];
        let (leading_at, leading_paired_at) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            leading_record_index,
            "387",
            class_383_leading::LEN,
        )?;
        let (leading_identity_at, _) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            leading_identity_record_index,
            "359",
            class_383_identity::LEN,
        )?;
        let (child_at, child_paired_at) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            child_record_index,
            "387",
            class_383_child::LEN,
        )?;
        let (child_identity_at, _) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            child_identity_record_index,
            "359",
            class_383_identity::LEN,
        )?;
        let (first_face_at, _) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            first_face_record_index,
            "394",
            class_383_face::LEN,
        )?;
        let (first_face_identity_at, _) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            first_face_identity_record_index,
            "359",
            class_383_identity::LEN,
        )?;
        let (second_face_at, _) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            second_face_record_index,
            "394",
            class_383_face::LEN,
        )?;
        let (second_face_identity_at, _) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            second_face_identity_record_index,
            "359",
            class_383_identity::LEN,
        )?;
        let (carrier_at, carrier_paired_at) = exact_legacy_class_383_record_frame(
            bytes,
            records,
            carrier_record_index,
            "378",
            class_383_carrier::LEN,
        )?;
        let structural_checks = [
            leading_paired_at == leading_at.checked_add(class_383_leading::LEN)?,
            child_paired_at == child_at.checked_add(class_383_child::LEN)?,
            marked_record_reference(
                bytes,
                leading_at.checked_add(class_383_leading::IDENTITY_REFERENCE)?,
            ) == Some(leading_identity_record_index),
            marked_record_reference(
                bytes,
                leading_at.checked_add(class_383_leading::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            marked_record_reference(
                bytes,
                child_at.checked_add(class_383_child::IDENTITY_REFERENCE)?,
            ) == Some(child_identity_record_index),
            marked_record_reference(
                bytes,
                child_at.checked_add(class_383_child::LEADING_REFERENCE)?,
            ) == Some(leading_record_index),
            marked_record_reference(
                bytes,
                child_at.checked_add(class_383_child::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            marked_record_reference(
                bytes,
                first_face_at.checked_add(class_383_face::IDENTITY_REFERENCE)?,
            ) == Some(first_face_identity_record_index),
            marked_record_reference(
                bytes,
                first_face_at.checked_add(class_383_face::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            marked_record_reference(
                bytes,
                second_face_at.checked_add(class_383_face::IDENTITY_REFERENCE)?,
            ) == Some(second_face_identity_record_index),
            marked_record_reference(
                bytes,
                second_face_at.checked_add(class_383_face::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            marked_record_reference(
                bytes,
                leading_identity_at.checked_add(class_383_identity::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            marked_record_reference(
                bytes,
                child_identity_at.checked_add(class_383_identity::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            marked_record_reference(
                bytes,
                first_face_identity_at.checked_add(class_383_identity::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            marked_record_reference(
                bytes,
                second_face_identity_at.checked_add(class_383_identity::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            marked_record_reference(
                bytes,
                carrier_at.checked_add(class_383_carrier::CHILD_REFERENCE)?,
            ) == Some(child_record_index),
            marked_record_reference(
                bytes,
                carrier_at.checked_add(class_383_carrier::SECOND_FACE_REFERENCE)?,
            ) == Some(second_face_record_index),
            marked_record_reference(
                bytes,
                carrier_at.checked_add(class_383_carrier::FIRST_FACE_REFERENCE)?,
            ) == Some(first_face_record_index),
            marked_record_reference(
                bytes,
                carrier_at.checked_add(class_383_carrier::REPEATED_CHILD_REFERENCE)?,
            ) == Some(child_record_index),
            marked_record_reference(
                bytes,
                carrier_at.checked_add(class_383_carrier::REPEATED_FIRST_FACE_REFERENCE)?,
            ) == Some(first_face_record_index),
            marked_record_reference(
                bytes,
                carrier_at.checked_add(class_383_carrier::REPEATED_SECOND_FACE_REFERENCE)?,
            ) == Some(second_face_record_index),
            marked_record_reference(
                bytes,
                carrier_at.checked_add(class_383_carrier::SCOPE_REFERENCE)?,
            ) == Some(scope.record_index),
            carrier_paired_at == carrier_at.checked_add(class_383_carrier::LEN)?,
            rigid_transform_at(bytes, carrier_at.checked_add(class_383_carrier::TRANSFORM)?)?
                == frame.transform,
        ];
        if structural_checks.iter().any(|check| !check) {
            return None;
        }
        for (ordinal, record_index) in placement_owners.into_iter().enumerate() {
            if marked_record_reference(
                bytes,
                carrier_at.checked_add(
                    class_383_carrier::PLACEMENT_OWNER_REFERENCES
                        .checked_add(ordinal * ASSEMBLY_MARKED_REFERENCE_LEN)?,
                )?,
            ) != Some(record_index)
            {
                return None;
            }
        }
        let (
            leading_occurrence_guid,
            leading_identity_guid,
            occurrence_guid_offset,
            identity_guid_offset,
        ) = match exact_legacy_class_383_identity_guids(bytes, leading_identity_at) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        for identity_at in [
            child_identity_at,
            first_face_identity_at,
            second_face_identity_at,
        ] {
            let (occurrence_guid, identity_guid, _, _) =
                match exact_legacy_class_383_identity_guids(bytes, identity_at) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
            if occurrence_guid != leading_occurrence_guid || identity_guid != leading_identity_guid
            {
                return None;
            }
        }
        let scope_at = usize::try_from(scope.byte_offset()).ok()?;
        let locator_reference_at = scope_at.checked_add(spec.scope_operand_reference_offset)?;
        let (locator_record_index, locator_reference_offset) =
            exact_same_segment_record_reference(bytes, locator_reference_at)?;
        if locator_record_index != carrier_record_index {
            return None;
        }
        let (scope_record_index, locator_scope_reference_offset) =
            exact_same_segment_record_reference(
                bytes,
                carrier_at.checked_add(class_383_carrier::SCOPE_REFERENCE)?,
            )?;
        if scope_record_index != scope.record_index {
            return None;
        }
        let (_, wrapper_reference_offset) = exact_same_segment_record_reference(
            bytes,
            leading_at.checked_add(class_383_leading::IDENTITY_REFERENCE)?,
        )?;
        DesignAssemblyOperandPath::try_new(
            DesignAssemblyOperandPathLink {
                locator_reference_offset,
                locator_record_index,
                locator_class_tag: "378".to_owned().try_into().ok()?,
                locator_byte_offset: u64::try_from(carrier_at).ok()?,
                locator_scope_reference_offset,
                wrapper_record_index: leading_identity_record_index,
                wrapper_reference_offset,
                wrapper_class_tag: "359".to_owned().try_into().ok()?,
                wrapper_byte_offset: u64::try_from(leading_identity_at).ok()?,
                path_reference_offset: occurrence_guid_offset,
            },
            leading_identity_record_index,
            "386".to_owned().try_into().ok()?,
            u64::try_from(leading_identity_at).ok()?,
            vec![crate::records::identity::Located {
                value: leading_occurrence_guid.into(),
                offset: occurrence_guid_offset,
            }],
            vec![crate::records::identity::Located {
                value: leading_identity_guid,
                offset: identity_guid_offset,
            }],
        )
        .ok()
        .map(Ok)
    })()
    .transpose()
}

fn exact_legacy_class_383_record_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &str,
    frame_length: usize,
) -> Option<(usize, usize)> {
    let mut candidates = records.frames(record_index).filter(|(start, paired_at)| {
        Some(*paired_at) == start.checked_add(frame_length)
            && exact_indexed_header_at(bytes, *start, record_index).as_deref() == Some(class_tag)
            && exact_indexed_header_at(bytes, *paired_at, record_index).as_deref() == Some("258")
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn exact_legacy_class_383_identity_guids(
    bytes: &[u8],
    start: usize,
) -> Result<
    Option<(
        crate::records::mesh::DesignRelaxedGuidText,
        crate::records::mesh::DesignRelaxedGuidText,
        u64,
        u64,
    )>,
    CodecError,
> {
    (|| {
        let first_at = start.checked_add(class_383_identity::OCCURRENCE_GUID)?;
        let second_at = start.checked_add(class_383_identity::IDENTITY_GUID)?;
        let (occurrence_guid, after_occurrence) = match fixed_relaxed_guid_text(bytes, first_at) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let (identity_guid, after_identity) = match fixed_relaxed_guid_text(bytes, second_at) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if after_occurrence != second_at
            || after_identity
                != start
                    .checked_add(class_383_identity::IDENTITY_GUID)?
                    .checked_add(76)?
        {
            return None;
        }
        Some(Ok((
            occurrence_guid,
            identity_guid,
            u64::try_from(first_at.checked_add(4)?).ok()?,
            u64::try_from(second_at.checked_add(4)?).ok()?,
        )))
    })()
    .transpose()
}

struct LegacyClass412Path {
    record_index: u32,
    byte_offset: u64,
    occurrence_guid: crate::records::identity::Located<crate::records::mesh::DesignRelaxedGuidText>,
    identity_guids:
        Vec<crate::records::identity::Located<crate::records::mesh::DesignRelaxedGuidText>>,
}

pub(super) fn exact_legacy_class_388_operand_paths(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<[DesignAssemblyOperandPath; 2]>, CodecError> {
    let parsed = (|| {
        if !matches!(
            crate::design::assembly::AssemblyScopeGeneration::new(
                scope.frame_length(),
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str()
            )
            .operand_frame_variant(),
            Some(crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388)
        ) || scope.reference_members().len()
            != index_from_u32(class_388_assemble::REFERENCE_COUNT_VALUE)
        {
            return None;
        }
        let scope_at = usize::try_from(scope.byte_offset()).ok()?;
        let search_start = usize::try_from(scope.paired_byte_offset())
            .ok()?
            .checked_add(11)?;
        let locator_offsets = [
            class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES,
            class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES + 11,
        ];
        let mut paths = [None, None];
        for (slot, relative_offset) in paths.iter_mut().zip(locator_offsets) {
            let locator_reference_at = scope_at.checked_add(relative_offset)?;
            let (locator_record_index, locator_reference_offset) =
                exact_same_segment_record_reference(bytes, locator_reference_at)?;
            let mut candidate = None;
            for locator_at in records
                .offsets(locator_record_index)
                .iter()
                .copied()
                .filter(|locator_at| *locator_at >= search_start)
            {
                let path = match exact_legacy_class_388_operand_path_envelope(
                    bytes,
                    records,
                    scope,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                ) {
                    Ok(path) => path,
                    Err(error) => return Some(Err(error)),
                };
                if let Some(path) = path {
                    if candidate.is_some() {
                        return None;
                    }
                    candidate = Some(path);
                }
            }
            *slot = candidate;
        }
        let [Some(first), Some(second)] = paths else {
            return None;
        };
        let wrapper_end = |path: &DesignAssemblyOperandPath| {
            let wrapper_at = usize::try_from(path.link().wrapper_byte_offset).ok()?;
            next_indexed_record_offset(bytes, wrapper_at.checked_add(1)?)
        };
        let first_start = usize::try_from(first.link().locator_byte_offset).ok()?;
        let first_end = wrapper_end(&first)?;
        let second_start = usize::try_from(second.link().locator_byte_offset).ok()?;
        let second_end = wrapper_end(&second)?;
        if first.link().locator_record_index == second.link().locator_record_index
            || first.link().wrapper_record_index == second.link().wrapper_record_index
            || (first_start < second_end && second_start < first_end)
        {
            return None;
        }
        Some(Ok([first, second]))
    })();
    parsed.transpose()
}

fn exact_legacy_class_388_operand_path_envelope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    locator_record_index: u32,
    locator_reference_offset: u64,
    locator_at: usize,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    let parsed = (|| {
        let locator_class_tag = exact_indexed_header_at(bytes, locator_at, locator_record_index)?;
        if locator_class_tag != "451"
            || next_indexed_record_offset(bytes, locator_at.checked_add(1)?)?
                != locator_at.checked_add(path_locator::LEN)?
            || bytes.get(
                locator_at.checked_add(path_locator::ZERO_RUN_10)?
                    ..locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
            )? != [0; 10]
            || exact_same_segment_record_reference(
                bytes,
                locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
            )?
            .0 == 0
            || bytes.get(locator_at.checked_add(path_locator::ZERO_32)?) != Some(&0)
            || rigid_transform_at(bytes, locator_at.checked_add(path_locator::TRANSFORM)?).is_none()
            || bytes.get(locator_at.checked_add(path_locator::ZERO_161)?) != Some(&0)
        {
            return None;
        }
        let (scope_record_index, locator_scope_reference_offset) =
            exact_same_segment_record_reference(
                bytes,
                locator_at.checked_add(path_locator::SCOPE_BACKLINK)?,
            )?;
        if scope_record_index != scope.record_index {
            return None;
        }
        let (wrapper_record_index, wrapper_reference_offset) = exact_same_segment_record_reference(
            bytes,
            locator_at.checked_add(path_locator::WRAPPER_REFERENCE)?,
        )?;
        if wrapper_record_index == 0
            || View::u32_le_at(bytes, locator_at.checked_add(path_locator::CONSTANT_TWO)?)? != 2
            || bytes.get(
                locator_at.checked_add(path_locator::ZERO_TAIL_2)?
                    ..locator_at.checked_add(path_locator::LEN)?,
            )? != [0; 2]
        {
            return None;
        }
        let locator_end = locator_at.checked_add(path_locator::LEN)?;
        let mut wrapper_candidates = records
            .offsets(wrapper_record_index)
            .iter()
            .copied()
            .filter(|wrapper_at| *wrapper_at >= locator_end)
            .filter(|wrapper_at| {
                exact_indexed_header_at(bytes, *wrapper_at, wrapper_record_index).as_deref()
                    == Some("369")
            });
        let wrapper_at = wrapper_candidates.next()?;
        if wrapper_candidates.next().is_some() {
            return None;
        }
        let wrapper_class_tag = exact_indexed_header_at(bytes, wrapper_at, wrapper_record_index)?;
        if bytes.get(
            wrapper_at.checked_add(11)?
                ..wrapper_at.checked_add(class_369_wrapper_one::WRAPPER_MARKER)?,
        )? != [0; 10]
            || bytes.get(wrapper_at.checked_add(class_369_wrapper_one::WRAPPER_MARKER)?) != Some(&1)
        {
            return None;
        }
        let path_count = View::u32_le_at(
            bytes,
            wrapper_at.checked_add(class_369_wrapper_one::PATH_COUNT)?,
        )?;
        let (wrapper_length, path_reference_offset) = match path_count {
            value if value == class_369_wrapper_one::PATH_COUNT_VALUE => (
                class_369_wrapper_one::LEN,
                class_369_wrapper_one::PATH_REFERENCE,
            ),
            value if value == class_369_wrapper_two::PATH_COUNT_VALUE => (
                class_369_wrapper_two::LEN,
                class_369_wrapper_two::PATH_REFERENCES,
            ),
            _ => return None,
        };
        let wrapper_end = wrapper_at.checked_add(wrapper_length)?;
        if next_indexed_record_offset(bytes, wrapper_at.checked_add(1)?)? != wrapper_end
            || wrapper_record_index
                != locator_record_index
                    .checked_add(path_count)?
                    .checked_add(1)?
        {
            return None;
        }
        let mut path_at = next_indexed_record_offset(bytes, locator_end)?;
        let mut path_records = [None, None];
        let mut final_path_reference_offset = None;
        for ordinal in 0..path_count {
            let path_record_index = locator_record_index.checked_add(ordinal)?.checked_add(1)?;
            if exact_indexed_header_at(bytes, path_at, path_record_index).as_deref() != Some("412")
            {
                return None;
            }
            let path_end = next_indexed_record_offset(bytes, path_at.checked_add(1)?)?;
            let path =
                match exact_legacy_class_412_path(bytes, path_at, path_record_index, path_end) {
                    Ok(Some(path)) => path,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
            path_records[usize::try_from(ordinal).ok()?] = Some(path);
            let (referenced_path_record_index, reference_offset) =
                exact_same_segment_record_reference(
                    bytes,
                    wrapper_at
                        .checked_add(path_reference_offset)?
                        .checked_add(usize::try_from(ordinal).ok()?.checked_mul(11)?)?,
                )?;
            if referenced_path_record_index != path_record_index {
                return None;
            }
            if ordinal + 1 == path_count {
                if path_end != wrapper_at {
                    return None;
                }
                final_path_reference_offset = Some(reference_offset);
            } else {
                path_at = path_end;
            }
        }
        let final_index = usize::try_from(path_count).ok()?.checked_sub(1)?;
        let final_path = path_records[final_index].take()?;

        let mut occurrence_guids = Vec::new();
        for path in path_records.into_iter().flatten() {
            occurrence_guids.push(crate::records::identity::Located {
                value: path.occurrence_guid.value.into(),
                offset: path.occurrence_guid.offset,
            });
        }
        occurrence_guids.push(crate::records::identity::Located {
            value: final_path.occurrence_guid.value.into(),
            offset: final_path.occurrence_guid.offset,
        });
        DesignAssemblyOperandPath::try_new(
            DesignAssemblyOperandPathLink {
                locator_reference_offset,
                locator_record_index,
                locator_class_tag: locator_class_tag.try_into().ok()?,
                locator_byte_offset: u64::try_from(locator_at).ok()?,
                locator_scope_reference_offset,
                wrapper_record_index,
                wrapper_reference_offset,
                wrapper_class_tag: wrapper_class_tag.try_into().ok()?,
                wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
                path_reference_offset: final_path_reference_offset?,
            },
            final_path.record_index,
            "412".to_owned().try_into().ok()?,
            final_path.byte_offset,
            occurrence_guids,
            final_path.identity_guids,
        )
        .ok()
        .map(Ok)
    })();
    parsed.transpose()
}

fn exact_legacy_class_412_path(
    bytes: &[u8],
    start: usize,
    record_index: u32,
    end: usize,
) -> Result<Option<LegacyClass412Path>, CodecError> {
    let parsed = (|| {
        if exact_indexed_header_at(bytes, start, record_index)?.as_str() != "412"
            || end != start.checked_add(class_412_path::LEN)?
            || bytes.get(start.checked_add(11)?..start.checked_add(class_412_path::PATH_MARKER)?)?
                != [0; 10]
            || bytes.get(start.checked_add(class_412_path::PATH_MARKER)?)
                != Some(&class_412_path::PATH_MARKER_VALUE)
            || bytes.get(
                start.checked_add(class_412_path::PATH_MARKER + 1)?
                    ..start.checked_add(class_412_path::OCCURRENCE_GUID)?,
            )? != [0; 3]
            || View::u64_le_at(
                bytes,
                start.checked_add(class_412_path::IDENTITY_SEPARATOR)?,
            )? != class_412_path::IDENTITY_SEPARATOR_VALUE
            || View::u32_le_at(bytes, start.checked_add(class_412_path::PATH_TAIL_COUNT)?)?
                != class_412_path::PATH_TAIL_COUNT_VALUE
            || bytes.get(start.checked_add(class_412_path::PATH_TAIL_COUNT + 4)?..end)? != [0; 8]
        {
            return None;
        }
        let (occurrence_guid, occurrence_end) = match fixed_relaxed_guid_text(
            bytes,
            start.checked_add(class_412_path::OCCURRENCE_GUID)?,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if occurrence_end != start.checked_add(class_412_path::FIRST_IDENTITY_GUID)? {
            return None;
        }
        let identity_offsets = [
            class_412_path::FIRST_IDENTITY_GUID,
            class_412_path::SECOND_IDENTITY_GUID,
            class_412_path::THIRD_IDENTITY_GUID,
            class_412_path::FOURTH_IDENTITY_GUID,
        ];
        let mut identity_guids = Vec::new();
        for (ordinal, relative_offset) in identity_offsets.iter().copied().enumerate() {
            let identity_at = start.checked_add(relative_offset)?;
            let (identity_guid, identity_end) = match fixed_relaxed_guid_text(bytes, identity_at) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            let expected_end = match ordinal {
                0 => class_412_path::SECOND_IDENTITY_GUID,
                1 => class_412_path::IDENTITY_SEPARATOR,
                2 => class_412_path::FOURTH_IDENTITY_GUID,
                3 => class_412_path::PATH_TAIL_COUNT,
                _ => return None,
            };
            if identity_end != start.checked_add(expected_end)? {
                return None;
            }
            identity_guids.push(crate::records::identity::Located {
                value: identity_guid,
                offset: u64::try_from(identity_at.checked_add(4)?).ok()?,
            });
        }
        Some(Ok(LegacyClass412Path {
            record_index,
            byte_offset: u64::try_from(start).ok()?,
            occurrence_guid: crate::records::identity::Located {
                value: occurrence_guid,
                offset: u64::try_from(start.checked_add(class_412_path::OCCURRENCE_GUID + 4)?)
                    .ok()?,
            },
            identity_guids,
        }))
    })();
    parsed.transpose()
}

#[cfg(test)]
mod tests;
