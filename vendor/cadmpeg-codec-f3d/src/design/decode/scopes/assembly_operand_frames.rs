// SPDX-License-Identifier: Apache-2.0
//! Exact assembly operand frames, including as-built frames.

use crate::layout::assembly_compact_wide_limit_owner_run as limit_run;
use crate::layout::assembly_compact_wide_scope as compact_wide;
use cadmpeg_core::decode::{u64_from_index, View};

use super::legacy_operand_paths::exact_legacy_class_388_scope;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::f64s_at;
use crate::layout::assembly_class_383_258_scope_1011 as class_383_scope;
use crate::layout::assembly_class_388_266_scope_968 as class_388_assemble;
use crate::layout::assembly_class_406_261_scope_671 as class_406_assemble;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::layout::assembly_wide_operand_path_locator as wide_locator;
use crate::layout::assembly_wide_scope as wide_scope;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::scope::DesignParameterScope;

/// Operand reference and transform offsets of a wide-family scope:
/// first reference, first transform, second reference, second transform.
pub(crate) fn wide_family_frame_offsets(
    variant: crate::design::assembly::AssemblyOperandFrameVariant,
) -> Option<[usize; 4]> {
    match variant {
        crate::design::assembly::AssemblyOperandFrameVariant::Wide => Some([37, 55, 259, 277]),
        crate::design::assembly::AssemblyOperandFrameVariant::CompactWide => Some([
            compact_wide::FIRST_OPERAND_REFERENCE,
            compact_wide::FIRST_OPERAND_TRANSFORM,
            compact_wide::SECOND_OPERAND_REFERENCE,
            compact_wide::SECOND_OPERAND_TRANSFORM,
        ]),
        _ => None,
    }
}

pub(super) fn exact_assembly_operand_frames(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let frame_variant = crate::design::assembly::AssemblyScopeGeneration::new(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    )
    .operand_frame_variant()?;
    let frame_offsets = match frame_variant {
        crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388 => (
            class_388_assemble::FIRST_OPERAND_REFERENCE,
            class_388_assemble::FIRST_OPERAND_TRANSFORM,
            class_388_assemble::SECOND_OPERAND_REFERENCE,
            class_388_assemble::SECOND_OPERAND_TRANSFORM,
        ),
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
            if scope.class_tag.as_str() == "383" && scope.paired_class_tag.as_str() == "258" =>
        {
            (
                class_383_scope::FIRST_OPERAND_REFERENCE,
                class_383_scope::FIRST_OPERAND_TRANSFORM,
                class_383_scope::SECOND_OPERAND_REFERENCE,
                class_383_scope::SECOND_OPERAND_TRANSFORM,
            )
        }
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
            if scope.class_tag.as_str() == "406" && scope.paired_class_tag.as_str() == "261" =>
        {
            (
                class_406_assemble::FIRST_OPERAND_REFERENCE,
                class_406_assemble::FIRST_OPERAND_TRANSFORM,
                class_406_assemble::SECOND_OPERAND_REFERENCE,
                class_406_assemble::SECOND_OPERAND_TRANSFORM,
            )
        }
        crate::design::assembly::AssemblyOperandFrameVariant::Standard => (28, 40, 168, 180),
        crate::design::assembly::AssemblyOperandFrameVariant::Compact => (24, 36, 164, 176),
        crate::design::assembly::AssemblyOperandFrameVariant::Axial => (28, 39, 167, 178),
        crate::design::assembly::AssemblyOperandFrameVariant::Wide
        | crate::design::assembly::AssemblyOperandFrameVariant::CompactWide => {
            let [first_reference, first_transform, second_reference, second_transform] =
                wide_family_frame_offsets(frame_variant)?;
            (
                first_reference,
                first_transform,
                second_reference,
                second_transform,
            )
        }
    };
    if usize::try_from(scope.paired_byte_offset()).ok()?
        != start.checked_add(usize::try_from(scope.frame_length()).ok()?)?
        || bytes.get(start + 11..start + 20)? != [0; 9]
    {
        return None;
    }
    if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388
    ) {
        exact_legacy_class_388_scope(bytes, scope)?;
        if bytes.get(start + 11..start + class_388_assemble::SCOPE_FLAGS)?
            != [0; class_388_assemble::SCOPE_FLAGS - 11]
            || bytes.get(
                start + class_388_assemble::SCOPE_FLAGS
                    ..start + class_388_assemble::SCOPE_FLAGS + 6,
            )? != class_388_assemble::SCOPE_FLAGS_VALUE
            || bytes.get(start + 26..start + class_388_assemble::FIRST_OPERAND_REFERENCE)? != [0; 2]
            || bytes.get(start + 39) != Some(&0)
            || bytes.get(start + 168 + 11..start + class_388_assemble::SECOND_OPERAND_TRANSFORM)?
                != [0; 1]
        {
            return None;
        }
    } else if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
    ) {
        let standard_tail_marker_offset = if scope.frame_length()
            == u64_from_index(class_383_scope::LEN)
            && scope.class_tag.as_str() == "383"
            && scope.paired_class_tag.as_str() == "258"
        {
            class_383_scope::STANDARD_TAIL_MARKER
        } else {
            308
        };
        if bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
            || !matches!(bytes.get(start + 25), Some(0 | 1))
            || bytes.get(start + 26..start + 28)? != [0; 2]
            || bytes.get(start + 33..start + 40)? != [0; 7]
            || bytes.get(start + 173..start + 180)? != [0; 7]
            || !crate::design::assembly::variable_reference_assembly_generation(
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            ) && bytes
                .get(start + standard_tail_marker_offset..start + standard_tail_marker_offset + 4)?
                != [0; 4]
        {
            return None;
        }
    } else if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::CompactWide
    ) {
        if bytes.get(start + compact_wide::ZERO_RUN_11..start + compact_wide::OPERAND_FLAG)?
            != [0; compact_wide::OPERAND_FLAG - compact_wide::ZERO_RUN_11]
            || !matches!(bytes.get(start + compact_wide::OPERAND_FLAG), Some(0 | 1))
            || bytes.get(
                start + compact_wide::ZERO_RUN_22..start + compact_wide::FIRST_OPERAND_REFERENCE,
            )? != [0; compact_wide::FIRST_OPERAND_REFERENCE - compact_wide::ZERO_RUN_22]
            || bytes.get(start + 43..start + 50)? != [0; 7]
            || bytes.get(start + 265..start + 272)? != [0; 7]
        {
            return None;
        }
        // The limit-owner run: the 850-byte scope names its two limit owners,
        // owner-local ordinals 4 and 5, and the other forms store count 0.
        let limit_owners = if scope.frame_length() == 850 { 2 } else { 0 };
        let run = start + 526;
        let stride =
            limit_run::SECOND_LIMIT_OWNER_REFERENCE - limit_run::FIRST_LIMIT_OWNER_REFERENCE;
        if usize::try_from(View::u32_le_at(bytes, run + limit_run::LIMIT_OWNER_COUNT)?).ok()?
            != limit_owners
            || (0..limit_owners).any(|ordinal| {
                marked_record_reference(
                    bytes,
                    run + limit_run::FIRST_LIMIT_OWNER_REFERENCE + stride * ordinal,
                ) != scope.reference_members().values().nth(4 + ordinal).copied()
            })
        {
            return None;
        }
    } else if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::Wide
    ) {
        if bytes.get(start + wide_scope::WIDE_MARKER) != Some(&1)
            || bytes
                .get(start + wide_scope::ZERO_RUN_21..start + wide_scope::FRAME_SENSE)?
                .iter()
                .any(|byte| *byte != 0)
            || !matches!(bytes.get(start + wide_scope::FRAME_SENSE), Some(0 | 1))
            || bytes
                .get(start + wide_scope::ZERO_RUN_26..start + frame_offsets.0)?
                .iter()
                .any(|byte| *byte != 0)
            || bytes.get(start + 48..start + 55)? != [0; 7]
            || bytes.get(start + 270..start + 277)? != [0; 7]
        {
            return None;
        }
    } else if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::Compact
    ) {
        let compact_flags = bytes.get(start + 20..start + 24)?;
        if (compact_flags != [0; 4] && compact_flags != [0, 1, 0, 0])
            || bytes.get(start + 29..start + 36)? != [0; 7]
            || bytes.get(start + 169..start + 176)? != [0; 7]
            || bytes.get(start + 304..start + 308)? != [0; 4]
        {
            return None;
        }
    } else if bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || !matches!(bytes.get(start + 25), Some(0 | 1))
        || bytes.get(start + 26..start + 28)? != [0; 2]
        || bytes.get(start + 33..start + 39)? != [0; 6]
        || bytes.get(start + 172..start + 178)? != [0; 6]
        || bytes.get(start + 306..start + 310)? != [0; 4]
    {
        return None;
    }
    let frame = |reference_at: usize, transform_at: usize| {
        let reference_record_index = marked_record_reference(bytes, reference_at)?;
        let values = f64s_at::<16>(bytes, transform_at)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        let transform =
            crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform).ok()?;
        Some(DesignAssemblyOperandFrame {
            reference_record_index,
            reference_offset: u64_from_index(reference_at + 1),
            transform,
            transform_offset: u64_from_index(transform_at),
        })
    };
    let first = frame(start + frame_offsets.0, start + frame_offsets.1)?;
    let second = frame(start + frame_offsets.2, start + frame_offsets.3)?;
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}

/// Read the operand frames from admitted as-built locator envelopes.
pub(super) fn exact_as_built_operand_frames(
    bytes: &[u8],
    paths: &[DesignAssemblyOperandPath; 2],
    scope: &DesignParameterScope,
    stream_types: &super::assembly_operand_paths::StreamTypes<'_>,
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let frames = paths
        .each_ref()
        .map(|path| exact_as_built_operand_frame(bytes, path, scope, stream_types));
    let [Some(first), Some(second)] = frames else {
        return None;
    };
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}

/// The operand frame one as-built locator carries, as
/// [`exact_as_built_operand_frames`] reads it.
pub(super) fn exact_as_built_operand_frame(
    bytes: &[u8],
    path: &DesignAssemblyOperandPath,
    scope: &DesignParameterScope,
    stream_types: &super::assembly_operand_paths::StreamTypes<'_>,
) -> Option<DesignAssemblyOperandFrame> {
    let locator_at = usize::try_from(path.link().locator_byte_offset).ok()?;
    let length = usize::try_from(
        path.byte_offset()
            .checked_sub(path.link().locator_byte_offset)?,
    )
    .ok()?;
    let wide = crate::design::assembly::wide_as_built_scope(scope);
    let group_layout = if crate::design::assembly::rigid_as_built_scope(scope) {
        Some(crate::design::assembly::RigidGroupLocatorLayout::admit(
            stream_types
                .get(&u64::from(path.link().locator_record_index))
                .copied(),
            length,
        )?)
    } else {
        None
    };
    let (reference_at, transform_at, transform, reference_record_index) =
        if let Some(layout) = group_layout {
            if bytes.get(locator_at.checked_add(layout.identity_flag)?)?
                != &u8::from(layout.transform.is_none())
            {
                return None;
            }
            let (reference_at, reference_record_index) = if layout.null_construction {
                (
                    locator_at
                        .checked_add(crate::layout::indexed_design_record_header::RECORD_INDEX)?,
                    path.link().locator_record_index,
                )
            } else {
                let at = locator_at.checked_add(layout.construction_reference)?;
                (
                    at.checked_add(1)?,
                    crate::bytes::local_reference_target(bytes, at, layout.typed)?,
                )
            };
            let (transform_at, transform) = match layout.transform {
                Some(transform) => {
                    let at = locator_at.checked_add(transform)?;
                    (at, rigid_transform_at(bytes, at)?)
                }
                None => (
                    locator_at.checked_add(layout.identity_flag)?,
                    cadmpeg_ir::transform::Transform::identity()
                        .rows()
                        .try_into()
                        .ok()?,
                ),
            };
            (
                reference_at,
                transform_at,
                transform,
                reference_record_index,
            )
        } else if wide {
            if !matches!(length, 97 | 225) {
                return None;
            }
            let (reference, flag, matrix) = (
                wide_locator::NONZERO_RECORD_REFERENCE,
                wide_locator::IDENTITY_FLAG,
                wide_locator::TRANSFORM,
            );
            let reference_at = locator_at.checked_add(reference)?;
            let flag_at = locator_at.checked_add(flag)?;
            let transform_at = locator_at.checked_add(matrix)?;
            let (transform_at, transform) = match bytes.get(flag_at)? {
                0 => (transform_at, rigid_transform_at(bytes, transform_at)?),
                1 => (
                    flag_at,
                    cadmpeg_ir::transform::Transform::identity()
                        .rows()
                        .try_into()
                        .ok()?,
                ),
                _ => return None,
            };
            (
                reference_at.checked_add(1)?,
                transform_at,
                transform,
                crate::bytes::local_reference_target(bytes, reference_at, false)?,
            )
        } else {
            let reference_at = locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?;
            let transform_at = locator_at.checked_add(path_locator::TRANSFORM)?;
            (
                reference_at.checked_add(1)?,
                transform_at,
                rigid_transform_at(bytes, transform_at)?,
                marked_record_reference(bytes, reference_at)?,
            )
        };
    Some(DesignAssemblyOperandFrame {
        reference_record_index,
        reference_offset: u64::try_from(reference_at).ok()?,
        transform,
        transform_offset: u64::try_from(transform_at).ok()?,
    })
}
