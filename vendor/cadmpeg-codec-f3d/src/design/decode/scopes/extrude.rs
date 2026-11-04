// SPDX-License-Identifier: Apache-2.0
//! Exact extrude admission frames.

use cadmpeg_core::decode::u64_from_index;

use super::legacy_class_397;
use super::legacy_class_415;
use super::shared_frames::extrude_operation_at;
use super::shared_frames::marked_record_reference;
use crate::bytes::f64s_at;
use crate::design::decode::text::fixed_guid_end;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_54 as class_296_legacy_scalar_54;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_70 as class_296_legacy_scalar_70;
use crate::layout::class_296_261_legacy_one_sided_distance_tail as class_296_legacy_distance;
use crate::layout::class_296_261_legacy_one_sided_to_face_tail as class_296_legacy_to_face;
use crate::layout::class_296_261_one_sided_to_face_extrude_prefix as class_296_to_face;
use crate::layout::class_296_261_symmetric_distance_extrude_prefix as class_296_symmetric;
use crate::layout::class_296_261_two_sided_to_faces_extrude_prefix as class_296_two_faces;
use crate::layout::compact_shifted_extrude_extent_and_table_prefix as compact_extrude_extent;
use crate::layout::compact_shifted_extrude_mixed_extent_and_table_prefix as compact_extrude_mixed;
use crate::layout::compact_shifted_extrude_prologue as compact_extrude;
use crate::layout::current_extrude_non_target_extent_pair as extrude_extent_pair;
use crate::layout::current_extrude_operation_fields as extrude_fields;
use crate::layout::current_extrude_shape_target_extent_prefix as extrude_target;
use crate::layout::early_distance_extrude_absent_prefix as early_absent;
use crate::layout::early_distance_extrude_present_prefix as early_present;
use crate::layout::legacy_class_338_two_sided_distance_extrude_frame as class_338_legacy;
use crate::layout::legacy_class_415_symmetric_extrude_prefix as class_415;
use crate::layout::offset_24_extrude_prologue;
use crate::layout::shifted_extrude_offset_283_two_sided_tail as shifted_283;
use crate::layout::shifted_extrude_offset_profile_extent_lane as offset_lane;
use crate::layout::shifted_extrude_prologue as shifted_extrude;
use crate::layout::shifted_reference_aware_extrude_class_323_symmetric_prefix as shifted_reference_aware_323_symmetric;
use crate::layout::shifted_reference_aware_extrude_class_323_tail as shifted_reference_aware_323_tail;
use crate::layout::shifted_reference_aware_extrude_scope_prefix as shifted_reference_aware;
use crate::layout::shifted_typed_reference_extrude_prologue as shifted_typed;
use crate::layout::typed_reference_extrude_prologue as typed_prologue;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudePrologueReference;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::extrude::DesignExtrudeTargetOrdinal;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;

fn flag_byte_at(bytes: &[u8], offset: usize) -> Option<bool> {
    match bytes.get(offset)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn extrude_start_at(bytes: &[u8], offset: usize) -> Option<DesignExtrudeStart> {
    match bytes.get(offset)? {
        0 => Some(DesignExtrudeStart::ProfilePlane),
        1 => Some(DesignExtrudeStart::OffsetProfilePlane),
        2 => Some(DesignExtrudeStart::FromFace),
        _ => None,
    }
}

pub(super) fn exact_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    let legacy_class_415 = paired_at
        .checked_sub(start)
        .zip(reference_count_at.checked_sub(start))
        .and_then(|(frame_length, reference_count_delta)| {
            Some(legacy_class_415::is_symmetric_distance_layout(
                class_tag,
                paired_class_tag,
                u64::try_from(frame_length).ok()?,
                u64::try_from(reference_count_delta).ok()?,
                reference_members.len(),
            ))
        })
        .unwrap_or(false);
    if class_tag == "415" && paired_class_tag == "265" {
        if legacy_class_415 {
            return exact_current_extrude_prologue(
                bytes,
                start,
                reference_count_at,
                reference_members,
                true,
            );
        }
        return legacy_class_415::exact_one_sided_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        );
    }
    exact_current_extrude_prologue(
        bytes,
        start,
        reference_count_at,
        reference_members,
        legacy_class_415,
    )
    .or_else(|| {
        exact_shifted_reference_aware_extrude_prologue(
            bytes,
            start,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| {
        legacy_class_397::exact_symmetric_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| {
        exact_class_338_two_sided_distance_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| {
        exact_legacy_shifted_extrude_prologue(bytes, start, reference_count_at, reference_members)
    })
    .or_else(|| exact_compact_shifted_extrude_prologue(bytes, start, reference_count_at))
    .or_else(|| {
        exact_compact_shifted_extrude_mixed_prologue(
            bytes,
            start,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| {
        exact_class_296_one_sided_to_face_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| {
        exact_class_296_symmetric_distance_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| {
        exact_class_296_two_sided_to_faces_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| {
        exact_class_296_legacy_one_sided_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| exact_legacy_distance_extrude_prologue(bytes, start, reference_count_at))
}

pub(crate) fn is_class_296_one_sided_to_face_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_to_face::REFERENCE_COUNT)
        && matches!(
            (frame_length, reference_member_count),
            (440, 7) | (462, 9) | (473, 10)
        )
}

pub(crate) fn is_class_296_symmetric_distance_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_symmetric::REFERENCE_COUNT)
        && (frame_length, reference_member_count) == (450, 7)
}

pub(crate) fn is_class_296_two_sided_to_faces_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_two_faces::REFERENCE_COUNT)
        && (frame_length, reference_member_count) == (536, 13)
}

pub(crate) fn is_class_296_two_sided_to_faces_scope(scope: &DesignParameterScope) -> bool {
    let Some(reference_count_offset) = scope
        .reference_count_offset()
        .checked_sub(scope.byte_offset())
    else {
        return false;
    };
    is_class_296_two_sided_to_faces_layout(
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length(),
        reference_count_offset,
        scope.reference_members().len(),
    ) && scope
        .extrude_prologue()
        .and_then(DesignExtrudePrologue::extent)
        == Some(DesignExtrudeExtent::TwoSidedToFaces)
}

pub(crate) fn is_class_296_legacy_one_sided_to_face_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_legacy_to_face::REFERENCE_COUNT)
        && (frame_length, reference_member_count) == (515, 12)
}

pub(crate) fn is_class_296_legacy_one_sided_distance_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_legacy_distance::REFERENCE_COUNT)
        && (frame_length, reference_member_count) == (483, 10)
}

fn exact_compact_shifted_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
) -> Option<DesignExtrudePrologue> {
    if View::u32_le_at(bytes, start.checked_add(compact_extrude::PREFIX_CONSTANT)?)? != 1
        || bytes.get(
            start.checked_add(compact_extrude::ZERO_RUN_2)?
                ..start.checked_add(compact_extrude::OPERATION)?,
        )? != [0; 2]
        || reference_count_at.checked_sub(start)? != compact_extrude_extent::REFERENCE_COUNT
    {
        return None;
    }
    let operation_offset = start.checked_add(compact_extrude::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(compact_extrude::DIRECTION)?,
        start.checked_add(compact_extrude::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if !matches!(direction_face_extend_values[0], 1 | 3) {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(compact_extrude_extent::FIRST_SIDE_EXTENT)?,
        start.checked_add(compact_extrude_extent::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [1, 0] {
        return None;
    }
    let extent = exact_extrude_extent(direction_face_extend_values[0], side_extent_discriminators)?;
    let direction_reversed_offset = start.checked_add(compact_extrude::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(compact_extrude::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(compact_extrude::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64_from_index(operation_offset),
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64_from_index(side_extent_discriminator_offsets[0]),
            u64_from_index(side_extent_discriminator_offsets[1]),
        ],
        extent: Some(extent),
        direction_face_extend_offsets: [
            u64_from_index(direction_face_extend_offsets[0]),
            u64_from_index(direction_face_extend_offsets[1]),
        ],
        direction_reversed,
        direction_reversed_offset: u64_from_index(direction_reversed_offset),
        solid_operation,
        solid_operation_offset: u64_from_index(solid_operation_offset),
        start: start_support,
        start_offset: u64_from_index(start_offset),
    })
}

fn exact_compact_shifted_extrude_mixed_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    if View::u32_le_at(bytes, start.checked_add(compact_extrude::PREFIX_CONSTANT)?)? != 1
        || bytes.get(
            start.checked_add(compact_extrude::ZERO_RUN_2)?
                ..start.checked_add(compact_extrude::OPERATION)?,
        )? != [0; 2]
        || reference_count_at.checked_sub(start)? != compact_extrude_mixed::REFERENCE_COUNT
        || reference_members.len() != 11
    {
        return None;
    }
    let operation_offset = start.checked_add(compact_extrude::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(compact_extrude::DIRECTION)?,
        start.checked_add(compact_extrude::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values != [2, 0] {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(compact_extrude_mixed::FIRST_SIDE_EXTENT)?,
        start.checked_add(compact_extrude_mixed::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [1, 2] {
        return None;
    }
    let direction_reversed_offset = start.checked_add(compact_extrude::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(compact_extrude::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(compact_extrude::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(side_extent_discriminator_offsets[0]).ok()?,
            u64::try_from(side_extent_discriminator_offsets[1]).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::TwoSidedDistanceToFace),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

fn exact_class_296_one_sided_to_face_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    let frame_length = paired_at.checked_sub(start)?;
    if !is_class_296_one_sided_to_face_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        return None;
    }
    if View::u32_le_at(
        bytes,
        start.checked_add(class_296_to_face::PREFIX_CONSTANT)?,
    )? != 1
        || bytes.get(
            start.checked_add(class_296_to_face::ZERO_RUN_2)?
                ..start.checked_add(class_296_to_face::OPERATION)?,
        )? != [0; 2]
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_to_face::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_296_to_face::DIRECTION)?,
        start.checked_add(class_296_to_face::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values[0] != 1 || !matches!(direction_face_extend_values[1], 1 | 2) {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(class_296_to_face::FIRST_SIDE_EXTENT)?,
        start.checked_add(class_296_to_face::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [2, 0]
        || side_extent_discriminator_offsets[1].checked_add(4)? != reference_count_at
    {
        return None;
    }
    let direction_reversed_offset = start.checked_add(class_296_to_face::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_296_to_face::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_296_to_face::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(side_extent_discriminator_offsets[0]).ok()?,
            u64::try_from(side_extent_discriminator_offsets[1]).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::OneSidedToFace),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

fn exact_class_296_symmetric_distance_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    let frame_length = paired_at.checked_sub(start)?;
    if !is_class_296_symmetric_distance_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        return None;
    }
    if View::u32_le_at(
        bytes,
        start.checked_add(class_296_symmetric::PREFIX_CONSTANT)?,
    )? != 1
        || bytes.get(
            start.checked_add(class_296_symmetric::ZERO_RUN_2)?
                ..start.checked_add(class_296_symmetric::OPERATION)?,
        )? != [0; 2]
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_symmetric::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_296_symmetric::DIRECTION)?,
        start.checked_add(class_296_symmetric::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values != [3, 2] {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(class_296_symmetric::FIRST_SIDE_EXTENT)?,
        start.checked_add(class_296_symmetric::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [1, 0]
        || side_extent_discriminator_offsets[1].checked_add(4)? != reference_count_at
    {
        return None;
    }
    let direction_reversed_offset = start.checked_add(class_296_symmetric::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_296_symmetric::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_296_symmetric::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(side_extent_discriminator_offsets[0]).ok()?,
            u64::try_from(side_extent_discriminator_offsets[1]).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::SymmetricDistance),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

fn exact_class_296_two_sided_to_faces_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

    let frame_length = paired_at.checked_sub(start)?;
    if !is_class_296_two_sided_to_faces_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        return None;
    }
    if View::u32_le_at(
        bytes,
        start.checked_add(class_296_two_faces::PREFIX_CONSTANT)?,
    )? != 1
        || bytes.get(
            start.checked_add(class_296_two_faces::ZERO_RUN_2)?
                ..start.checked_add(class_296_two_faces::OPERATION)?,
        )? != [0; 2]
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_two_faces::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_296_two_faces::DIRECTION)?,
        start.checked_add(class_296_two_faces::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values[0] != 2 || !matches!(direction_face_extend_values[1], 1 | 2) {
        return None;
    }
    let direction_reversed_offset = start.checked_add(class_296_two_faces::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_296_two_faces::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_296_two_faces::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    if bytes.get(
        start.checked_add(class_296_two_faces::ZERO_RUN_3_AFTER_START)?
            ..start.checked_add(class_296_two_faces::PROFILE_NORMAL)?,
    )? != [0; 3]
    {
        return None;
    }
    let profile_normal = f64s_at::<3>(
        bytes,
        start.checked_add(class_296_two_faces::PROFILE_NORMAL)?,
    )?;
    let profile_normal_squared = profile_normal
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if profile_normal
        .iter()
        .any(|component| !component.is_finite())
        || (profile_normal_squared - 1.0).abs() > PROFILE_NORMAL_UNIT_EPS
    {
        return None;
    }
    let mut slot_offset = start.checked_add(class_296_two_faces::REFERENCE_SLOTS)?;
    for expected_present in [false, false, false, true, true, true, true] {
        let present = flag_byte_at(bytes, slot_offset)?;
        if present != expected_present {
            return None;
        }
        if present {
            let record_index = marked_record_reference(bytes, slot_offset)?;
            if !reference_members.contains(&record_index) {
                return None;
            }
            slot_offset = slot_offset.checked_add(11)?;
        } else {
            slot_offset = slot_offset.checked_add(1)?;
        }
    }
    if slot_offset != start.checked_add(class_296_two_faces::FIRST_SIDE_EXTENT)? {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(class_296_two_faces::FIRST_SIDE_EXTENT)?,
        start.checked_add(class_296_two_faces::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [2, 0]
        || side_extent_discriminator_offsets[1].checked_add(4)? != reference_count_at
    {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(side_extent_discriminator_offsets[0]).ok()?,
            u64::try_from(side_extent_discriminator_offsets[1]).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::TwoSidedToFaces),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

fn exact_class_296_legacy_one_sided_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

    let frame_length = paired_at.checked_sub(start)?;
    let (is_to_face, face_extend, first_extent) = if is_class_296_legacy_one_sided_to_face_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        (true, 1, 2)
    } else if is_class_296_legacy_one_sided_distance_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        (false, 2, 1)
    } else {
        return None;
    };
    if View::u32_le_at(
        bytes,
        start.checked_add(class_296_legacy_scalar_54::PREFIX_CONSTANT)?,
    )? != 1
        || bytes.get(
            start.checked_add(class_296_legacy_scalar_54::ZERO_BEFORE_REFERENCE)?
                ..start.checked_add(class_296_legacy_scalar_54::REFERENCE)?,
        )? != [0]
        || !reference_members.contains(&marked_record_reference(
            bytes,
            start.checked_add(class_296_legacy_scalar_54::REFERENCE)?,
        )?)
        || bytes.get(
            start
                .checked_add(class_296_legacy_scalar_54::REFERENCE)?
                .checked_add(5)?
                ..start.checked_add(class_296_legacy_scalar_54::OPERATION)?,
        )? != [0; 6]
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_legacy_scalar_54::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_296_legacy_scalar_54::DIRECTION)?,
        start.checked_add(class_296_legacy_scalar_54::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values != [1, face_extend] {
        return None;
    }
    let direction_reversed_offset =
        start.checked_add(class_296_legacy_scalar_54::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_296_legacy_scalar_54::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_296_legacy_scalar_54::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    if bytes.get(
        start.checked_add(class_296_legacy_scalar_54::ZERO_AFTER_START)?
            ..start.checked_add(class_296_legacy_scalar_54::PROFILE_SCALAR_AT_54)?,
    )? != [0; 3]
    {
        return None;
    }
    let profile_scalar_at_54 = f64s_at::<1>(
        bytes,
        start.checked_add(class_296_legacy_scalar_54::PROFILE_SCALAR_AT_54)?,
    )?
    .into_iter()
    .next()?;
    let profile_scalar_at_70 = f64s_at::<1>(
        bytes,
        start.checked_add(class_296_legacy_scalar_70::PROFILE_SCALAR_AT_70)?,
    )?
    .into_iter()
    .next()?;
    let scalar_at_54 = profile_scalar_at_54.is_finite()
        && (profile_scalar_at_54.abs() - 1.0).abs() <= PROFILE_NORMAL_UNIT_EPS
        && bytes.get(
            start.checked_add(class_296_legacy_scalar_54::ZERO_AFTER_SCALAR_AT_54)?
                ..start.checked_add(class_296_legacy_scalar_54::REFERENCE_SLOTS)?,
        )? == [0; 16];
    let scalar_at_70 = profile_scalar_at_70.is_finite()
        && (profile_scalar_at_70.abs() - 1.0).abs() <= PROFILE_NORMAL_UNIT_EPS
        && bytes.get(
            start.checked_add(class_296_legacy_scalar_70::ZERO_BEFORE_SCALAR_AT_70)?
                ..start.checked_add(class_296_legacy_scalar_70::PROFILE_SCALAR_AT_70)?,
        )? == [0; 16];
    if !scalar_at_54 && !scalar_at_70 {
        return None;
    }
    let mut slot_offset = start.checked_add(class_296_legacy_scalar_54::REFERENCE_SLOTS)?;
    let slot_presence = if is_to_face {
        [true, false, false, true, false, true, true]
    } else {
        [true, false, true, true, false, true, false]
    };
    for expected_present in slot_presence {
        let present = flag_byte_at(bytes, slot_offset)?;
        if present != expected_present {
            return None;
        }
        if present {
            let record_index = marked_record_reference(bytes, slot_offset)?;
            if !reference_members.contains(&record_index) {
                return None;
            }
            slot_offset = slot_offset.checked_add(11)?;
        } else {
            slot_offset = slot_offset.checked_add(1)?;
        }
    }
    let first_side_extent_offset =
        start.checked_add(class_296_legacy_scalar_54::FIRST_SIDE_EXTENT)?;
    if slot_offset != first_side_extent_offset
        || View::u32_le_at(bytes, first_side_extent_offset)? != first_extent
    {
        return None;
    }
    let second_side_extent_offset = reference_count_at.checked_sub(4)?;
    if View::u32_le_at(bytes, second_side_extent_offset)? != 0
        || second_side_extent_offset.checked_add(4)? != reference_count_at
    {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators: [first_extent, 0],
        side_extent_discriminator_offsets: [
            u64::try_from(first_side_extent_offset).ok()?,
            u64::try_from(second_side_extent_offset).ok()?,
        ],
        extent: Some(if is_to_face {
            DesignExtrudeExtent::OneSidedToFace
        } else {
            DesignExtrudeExtent::OneSidedDistance
        }),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

fn exact_legacy_distance_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
) -> Option<DesignExtrudePrologue> {
    let marker_offset = start.checked_add(early_absent::ABSENT_PREFIX)?;
    let (prefix_zero_offset, operation_offset, expected_reference_count_delta) =
        match bytes.get(marker_offset)? {
            0 => (None, start.checked_add(early_absent::OPERATION)?, 208),
            1 => {
                let prefix_value_offset = start.checked_add(early_present::PREFIX_VALUE)?;
                let prefix_value = View::u32_le_at(bytes, prefix_value_offset)?;
                if prefix_value != 0 {
                    return None;
                }
                (
                    Some(u64::try_from(prefix_value_offset).ok()?),
                    start.checked_add(early_present::OPERATION)?,
                    212,
                )
            }
            _ => return None,
        };
    if reference_count_at.checked_sub(start)? != expected_reference_count_delta {
        return None;
    }
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let extent_kind_offset = operation_offset.checked_add(4)?;
    let extent_kind = View::u32_le_at(bytes, extent_kind_offset)?;
    if extent_kind != 2 {
        return None;
    }
    let direction_reversed_offset = extent_kind_offset.checked_add(4)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let geometry_kind_offset = direction_reversed_offset.checked_add(1)?;
    let solid_operation = match View::u32_le_at(bytes, geometry_kind_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(DesignExtrudePrologue::LegacyDistance {
        prefix_zero_offset,
        operation,
        operation_offset: u64_from_index(operation_offset),
        extent_kind_offset: u64_from_index(extent_kind_offset),
        direction_reversed,
        direction_reversed_offset: u64_from_index(direction_reversed_offset),
        solid_operation,
        solid_operation_offset: u64_from_index(geometry_kind_offset),
    })
}

fn exact_current_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
    legacy_class_415_symmetric_distance: bool,
) -> Option<DesignExtrudePrologue> {
    exact_current_extrude_prologue_with(
        bytes,
        start,
        reference_count_at,
        reference_members,
        legacy_class_415_symmetric_distance,
        false,
    )
}

/// The Extrude prologue of a typed-reference scope, which has only the
/// typed form.
pub(super) fn exact_typed_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    exact_current_extrude_prologue_with(
        bytes,
        start,
        reference_count_at,
        reference_members,
        false,
        true,
    )
}

/// The current Extrude prologue in one reference spelling. The typed form
/// stores the operation fields at offset 22 with no prologue reference, and
/// its present extent slots are 51-byte typed references.
fn exact_current_extrude_prologue_with(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
    legacy_class_415_symmetric_distance: bool,
    typed: bool,
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

    if legacy_class_415_symmetric_distance
        && (View::u32_le_at(bytes, start.checked_add(class_415::PREFIX_CONSTANT)?)? != 1
            || bytes.get(
                start.checked_add(class_415::ZERO_RUN_3)?
                    ..start.checked_add(class_415::OPERATION_PREFIX_MARKER)?,
            )? != [0; 3]
            || bytes.get(start.checked_add(class_415::OPERATION_PREFIX_MARKER)?) != Some(&1))
    {
        return None;
    }
    let direct_offset = if typed {
        let marker = start.checked_add(shifted_typed::MARKER)?;
        if bytes.get(
            start.checked_add(typed_prologue::ZERO_RUN_11)?
                ..start.checked_add(typed_prologue::OPERATION)?,
        )? == [0; 11]
        {
            start.checked_add(typed_prologue::OPERATION)?
        } else if bytes.get(start.checked_add(shifted_typed::ZERO_RUN_9)?..marker)? == [0; 9]
            && View::u32_le_at(bytes, marker)? == 1
            && bytes.get(
                start.checked_add(shifted_typed::ZERO_RUN_2)?
                    ..start.checked_add(shifted_typed::OPERATION)?,
            )? == [0; 2]
        {
            start.checked_add(shifted_typed::OPERATION)?
        } else {
            return None;
        }
    } else if bytes.get(
        start.checked_add(offset_24_extrude_prologue::ZERO_RUN_13)?
            ..start.checked_add(offset_24_extrude_prologue::OPERATION)?,
    ) == Some(&[0; 13])
        && matches!(
            View::u32_le_at(
                bytes,
                start.checked_add(offset_24_extrude_prologue::OPERATION)?
            ),
            Some(1..=4)
        )
    {
        start.checked_add(offset_24_extrude_prologue::OPERATION)?
    } else {
        start.checked_add(28)?
    };
    let reference = if !typed
        && direct_offset != start.checked_add(offset_24_extrude_prologue::OPERATION)?
        && bytes.get(start.checked_add(25)?) == Some(&1)
    {
        let reference_record_index_offset = start.checked_add(26)?;
        let record_index = View::u32_le_at(bytes, reference_record_index_offset)?;
        let prefix_tail = start.checked_add(30)?;
        let mut candidates = [
            (start.checked_add(37)?, None),
            (start.checked_add(38)?, None),
            (start.checked_add(38)?, Some(start.checked_add(37)?)),
        ]
        .into_iter()
        .filter(|(operation_offset, marker_offset)| {
            let padding_end = marker_offset.unwrap_or(*operation_offset);
            bytes
                .get(prefix_tail..padding_end)
                .is_some_and(|padding| padding.iter().all(|byte| *byte == 0))
                && marker_offset.is_none_or(|offset| bytes.get(offset) == Some(&1))
                && reference_members.contains(&record_index)
                && matches!(View::u32_le_at(bytes, *operation_offset), Some(1..=4))
                && matches!(
                    operation_offset
                        .checked_add(4)
                        .and_then(|at| View::u32_le_at(bytes, at)),
                    Some(1..=3)
                )
                && operation_offset
                    .checked_add(8)
                    .and_then(|at| View::u32_le_at(bytes, at))
                    .is_some()
                && matches!(
                    operation_offset
                        .checked_add(12)
                        .and_then(|at| bytes.get(at)),
                    Some(0 | 1)
                )
                && matches!(
                    operation_offset
                        .checked_add(13)
                        .and_then(|at| bytes.get(at)),
                    Some(0 | 1)
                )
                && matches!(
                    operation_offset
                        .checked_add(14)
                        .and_then(|at| bytes.get(at)),
                    Some(0..=2)
                )
        });
        let (operation_offset, operation_marker_offset) = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        let padding_end = operation_marker_offset.unwrap_or(operation_offset);
        let trailing_zero_count = u8::try_from(padding_end.checked_sub(prefix_tail)?).ok()?;
        Some((
            operation_offset,
            DesignExtrudePrologueReference {
                record_index,
                record_index_offset: u64_from_index(reference_record_index_offset),
                trailing_zero_count,
                operation_prefix_marker_offset: operation_marker_offset
                    .and_then(|offset| u64::try_from(offset).ok()),
            },
        ))
    } else {
        None
    };
    let (operation_offset, reference) = reference
        .map_or((direct_offset, None), |(offset, reference)| {
            (offset, Some(reference))
        });
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_offset = operation_offset.checked_add(extrude_fields::DIRECTION)?;
    let face_extend_offset = operation_offset.checked_add(extrude_fields::FACE_EXTEND)?;
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_offset)?,
        View::u32_le_at(bytes, face_extend_offset)?,
    ];
    if !matches!(direction_face_extend_values[0], 1..=3) {
        return None;
    }
    let direction_reversed_offset =
        operation_offset.checked_add(extrude_fields::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = operation_offset.checked_add(extrude_fields::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = operation_offset.checked_add(extrude_fields::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    let profile_normal_offset = operation_offset.checked_add(extrude_fields::PROFILE_NORMAL)?;
    if bytes
        .get(operation_offset.checked_add(extrude_fields::ZERO_RUN_3)?..profile_normal_offset)?
        != [0; 3]
    {
        return None;
    }
    let profile_normal = f64s_at::<3>(bytes, profile_normal_offset)?;
    let profile_normal_squared = profile_normal
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if profile_normal
        .iter()
        .any(|component| !component.is_finite())
        || (profile_normal_squared - 1.0).abs() > PROFILE_NORMAL_UNIT_EPS
    {
        return None;
    }
    let mut extent_cursor = profile_normal_offset.checked_add(24)?;
    let mut final_slot_reference = None;
    let mut slot_presence = [false; 7];
    for (slot_ordinal, slot_present) in slot_presence.iter_mut().enumerate() {
        match bytes.get(extent_cursor)? {
            0 => extent_cursor = extent_cursor.checked_add(1)?,
            1 => {
                *slot_present = true;
                let (record_index, width) = if typed {
                    (
                        crate::bytes::typed_local_reference_target(bytes, extent_cursor)?,
                        crate::bytes::TYPED_LOCAL_REFERENCE_LEN,
                    )
                } else {
                    (marked_record_reference(bytes, extent_cursor)?, 11)
                };
                if !reference_members.contains(&record_index) {
                    return None;
                }
                if slot_ordinal == 6 {
                    final_slot_reference = Some(record_index);
                }
                extent_cursor = extent_cursor.checked_add(width)?;
            }
            _ => return None,
        }
    }
    if legacy_class_415_symmetric_distance
        && slot_presence != [false, true, true, true, false, true, false]
    {
        return None;
    }
    let first_side_target_ordinal = final_slot_reference.and_then(|record_index| {
        let scope_reference_ordinal = View::u32_le_at(bytes, extent_cursor)?;
        let ordinal = usize::try_from(scope_reference_ordinal).ok()?;
        if reference_members.get(ordinal) != Some(&record_index)
            || bytes.get(extent_cursor.checked_add(extrude_target::ZERO_SEPARATOR)?) != Some(&0)
            || View::u32_le_at(
                bytes,
                extent_cursor.checked_add(extrude_target::FIRST_SIDE_EXTENT)?,
            ) != Some(2)
        {
            return None;
        }
        Some(DesignExtrudeTargetOrdinal {
            scope_reference_ordinal,
            scope_reference_ordinal_offset: u64_from_index(extent_cursor),
        })
    });
    if first_side_target_ordinal.is_some() {
        extent_cursor = extent_cursor.checked_add(5)?;
    }
    let first_side_extent_offset = extent_cursor;
    let first_side_extent = View::u32_le_at(bytes, first_side_extent_offset)?;
    let second_side_extent_offset = if first_side_extent == 2 {
        let target_option = first_side_extent_offset
            .checked_add(crate::layout::current_extrude_face_target_payload::TARGET_OPTION)
            .and_then(|at| View::u32_le_at(bytes, at));
        let payload_second = first_side_extent_offset
            .checked_add(crate::layout::current_extrude_face_target_payload::SECOND_SIDE_EXTENT);
        let payload_second_value = payload_second.and_then(|at| View::u32_le_at(bytes, at));
        if direction_face_extend_values[0] == 2
            && first_side_target_ordinal.is_none()
            && target_option == Some(1)
        {
            match payload_second_value? {
                1 => return None,
                2 => payload_second?,
                _ => reference_count_at.checked_sub(4)?,
            }
        } else if direction_face_extend_values[0] == 2
            && first_side_target_ordinal.is_none()
            && target_option == Some(0)
            && payload_second_value.is_some_and(|value| value != 0)
        {
            // Target option zero with a second-side payload has no settled
            // assignment (DR-117); the scope stays native.
            return None;
        } else {
            reference_count_at.checked_sub(4)?
        }
    } else {
        let second_side_extent_offset =
            first_side_extent_offset.checked_add(extrude_extent_pair::SECOND_SIDE_EXTENT)?;
        if bytes.get(first_side_extent_offset.checked_add(4)?..second_side_extent_offset)? != [0; 9]
        {
            return None;
        }
        second_side_extent_offset
    };
    if second_side_extent_offset < first_side_extent_offset.checked_add(4)?
        || second_side_extent_offset.checked_add(4)? > reference_count_at
    {
        return None;
    }
    let side_extent_discriminators = [
        first_side_extent,
        View::u32_le_at(bytes, second_side_extent_offset)?,
    ];
    if legacy_class_415_symmetric_distance
        && (extent_cursor != start.checked_add(class_415::FIRST_SIDE_EXTENT)?
            || first_side_extent_offset != start.checked_add(class_415::FIRST_SIDE_EXTENT)?
            || second_side_extent_offset != start.checked_add(class_415::SECOND_SIDE_EXTENT)?
            || reference_count_at != start.checked_add(class_415::REFERENCE_COUNT)?
            || direction_face_extend_values != [3, 2]
            || side_extent_discriminators != [1, 1])
    {
        return None;
    }
    let extent = exact_extrude_extent(direction_face_extend_values[0], side_extent_discriminators)
        .or_else(|| {
            (legacy_class_415_symmetric_distance
                && direction_face_extend_values == [3, 2]
                && side_extent_discriminators == [1, 1])
            .then_some(DesignExtrudeExtent::SymmetricDistance)
        })
        .or_else(|| {
            // Dual distance lanes sharing one per-side distance: each side
            // travels it, as for the settled symmetric Revolve form.
            (direction_face_extend_values == [3, 2] && side_extent_discriminators == [1, 1])
                .then_some(DesignExtrudeExtent::SymmetricDistance)
        })?;
    Some(DesignExtrudePrologue::ReferenceAware {
        reference,
        operation,
        operation_offset: u64_from_index(operation_offset),
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64_from_index(first_side_extent_offset),
            u64_from_index(second_side_extent_offset),
        ],
        first_side_target_ordinal,
        extent,
        direction_face_extend_offsets: [
            u64_from_index(direction_offset),
            u64_from_index(face_extend_offset),
        ],
        direction_reversed,
        direction_reversed_offset: u64_from_index(direction_reversed_offset),
        solid_operation,
        solid_operation_offset: u64_from_index(solid_operation_offset),
        start: start_support,
        start_offset: u64_from_index(start_offset),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TailForm {
    Ordered,
    Unordered,
    SymmetricThroughAll,
}

#[derive(Clone, Copy)]
struct ShiftedReferenceAwareLayout {
    frame_length: usize,
    reference_count_offset: usize,
    reference_member_count: usize,
    expected_paired_class: &'static [u8; 3],
    trailing_reference_count_offset: usize,
    trailing_reference_offset: usize,
    trailing_reference_padding_offset: usize,
    guid_prefix_offset: usize,
    second_side_extent_offset: usize,
    tail_form: TailForm,
}

fn exact_shifted_reference_aware_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;
    const CLASS_TAG_OFFSET: usize = 4;
    const CLASS_TAG_LENGTH: usize = 3;

    let primary_class = bytes.get(
        start.checked_add(CLASS_TAG_OFFSET)?
            ..start.checked_add(CLASS_TAG_OFFSET + CLASS_TAG_LENGTH)?,
    )?;
    let has_reference_layout = |reference_count: usize, reference_count_at: usize| {
        reference_count_at.checked_sub(start) == Some(reference_count)
    };
    let ordered = ShiftedReferenceAwareLayout {
        frame_length: 538,
        reference_count_offset: shifted_reference_aware::REFERENCE_COUNT,
        reference_member_count: 13,
        expected_paired_class: b"258",
        trailing_reference_count_offset: shifted_reference_aware::BODY_GROUP_COUNT,
        trailing_reference_offset: shifted_reference_aware::BODY_GROUP_REFERENCE,
        trailing_reference_padding_offset: shifted_reference_aware::BODY_GROUP_REFERENCE + 11,
        guid_prefix_offset: shifted_reference_aware::BODY_GROUP_GUID_PREFIX,
        second_side_extent_offset: shifted_reference_aware::SECOND_SIDE_EXTENT,
        tail_form: TailForm::Ordered,
    };
    let ShiftedReferenceAwareLayout {
        frame_length,
        reference_count_offset,
        reference_member_count,
        expected_paired_class,
        trailing_reference_count_offset,
        trailing_reference_offset,
        trailing_reference_padding_offset,
        guid_prefix_offset,
        second_side_extent_offset,
        tail_form,
    } = match primary_class {
        b"357" => ordered,
        b"275" | b"361" | b"397" => ShiftedReferenceAwareLayout {
            expected_paired_class: b"262",
            ..ordered
        },
        b"349" => ShiftedReferenceAwareLayout {
            expected_paired_class: b"266",
            ..ordered
        },
        b"323"
            if has_reference_layout(
                shifted_reference_aware::REFERENCE_COUNT,
                reference_count_at,
            ) && reference_members.len() == 11 =>
        {
            ShiftedReferenceAwareLayout {
                frame_length: 516,
                reference_count_offset: shifted_reference_aware::REFERENCE_COUNT,
                reference_member_count: 11,
                expected_paired_class: b"263",
                trailing_reference_count_offset:
                    shifted_reference_aware_323_tail::TRAILING_REFERENCE_COUNT,
                trailing_reference_offset: shifted_reference_aware_323_tail::TRAILING_REFERENCE,
                trailing_reference_padding_offset:
                    shifted_reference_aware_323_tail::TRAILING_REFERENCE_PADDING,
                guid_prefix_offset: shifted_reference_aware::BODY_GROUP_GUID_PREFIX,
                second_side_extent_offset: shifted_reference_aware::SECOND_SIDE_EXTENT,
                tail_form: TailForm::Unordered,
            }
        }
        b"323"
            if has_reference_layout(
                shifted_reference_aware_323_symmetric::REFERENCE_COUNT,
                reference_count_at,
            ) && reference_members.len() == 10 =>
        {
            ShiftedReferenceAwareLayout {
                frame_length: 485,
                reference_count_offset: shifted_reference_aware_323_symmetric::REFERENCE_COUNT,
                reference_member_count: 10,
                expected_paired_class: b"263",
                trailing_reference_count_offset:
                    shifted_reference_aware_323_symmetric::TRAILING_REFERENCE_COUNT,
                trailing_reference_offset:
                    shifted_reference_aware_323_symmetric::TRAILING_REFERENCE,
                trailing_reference_padding_offset:
                    shifted_reference_aware_323_symmetric::GUID_PREFIX,
                guid_prefix_offset: shifted_reference_aware_323_symmetric::GUID_PREFIX,
                second_side_extent_offset:
                    shifted_reference_aware_323_symmetric::SECOND_SIDE_EXTENT,
                tail_form: TailForm::SymmetricThroughAll,
            }
        }
        _ => return None,
    };
    if reference_count_at.checked_sub(start)? != reference_count_offset
        || reference_members.len() != reference_member_count
        || View::u32_le_at(
            bytes,
            start.checked_add(shifted_reference_aware::PREFIX_CONSTANT)?,
        )? != 1
        || bytes.get(
            start.checked_add(shifted_reference_aware::ZERO_RUN_3)?
                ..start.checked_add(shifted_reference_aware::OPERATION)?,
        )? != [0; 3]
        || bytes.get(
            start.checked_add(shifted_reference_aware::ZERO_RUN_3_AFTER_START)?
                ..start.checked_add(shifted_reference_aware::PROFILE_NORMAL)?,
        )? != [0; 3]
    {
        return None;
    }
    let paired_start = start.checked_add(frame_length)?;
    let paired_class = bytes.get(
        paired_start.checked_add(CLASS_TAG_OFFSET)?
            ..paired_start.checked_add(CLASS_TAG_OFFSET + CLASS_TAG_LENGTH)?,
    )?;
    if paired_class != expected_paired_class {
        return None;
    }
    let operation_offset = start.checked_add(shifted_reference_aware::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(shifted_reference_aware::DIRECTION)?,
        start.checked_add(shifted_reference_aware::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    let expected_direction_face_extend = if tail_form == TailForm::SymmetricThroughAll {
        [3, 0]
    } else {
        [2, 1]
    };
    if direction_face_extend_values != expected_direction_face_extend {
        return None;
    }
    let direction_reversed_offset =
        start.checked_add(shifted_reference_aware::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(shifted_reference_aware::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(shifted_reference_aware::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    let profile_normal_offset = start.checked_add(shifted_reference_aware::PROFILE_NORMAL)?;
    let profile_normal = f64s_at::<3>(bytes, profile_normal_offset)?;
    let profile_normal_squared = profile_normal
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if profile_normal
        .iter()
        .any(|component| !component.is_finite())
        || (profile_normal_squared - 1.0).abs() > PROFILE_NORMAL_UNIT_EPS
    {
        return None;
    }
    let mut slot_offset = start.checked_add(shifted_reference_aware::REFERENCE_SLOTS)?;
    for expected_present in [false, false, false, true, true, true, true] {
        let present = flag_byte_at(bytes, slot_offset)?;
        if present != expected_present {
            return None;
        }
        if present {
            let record_index = marked_record_reference(bytes, slot_offset)?;
            if !reference_members.contains(&record_index) {
                return None;
            }
            slot_offset = slot_offset.checked_add(11)?;
        } else {
            slot_offset = slot_offset.checked_add(1)?;
        }
    }
    let first_side_extent_offset = start.checked_add(shifted_reference_aware::FIRST_SIDE_EXTENT)?;
    if slot_offset != first_side_extent_offset {
        return None;
    }
    let second_side_extent_offset = if tail_form == TailForm::SymmetricThroughAll {
        start.checked_add(second_side_extent_offset)?
    } else {
        reference_count_at.checked_sub(4)?
    };
    let side_extent_discriminators = [
        View::u32_le_at(bytes, first_side_extent_offset)?,
        View::u32_le_at(bytes, second_side_extent_offset)?,
    ];
    let extent = exact_extrude_extent(direction_face_extend_values[0], side_extent_discriminators)?;
    let expected_side_extent_discriminators = if tail_form == TailForm::SymmetricThroughAll {
        [4, 4]
    } else {
        [2, 0]
    };
    if side_extent_discriminators != expected_side_extent_discriminators {
        return None;
    }
    let ordered_tail_references_valid = if tail_form == TailForm::SymmetricThroughAll {
        [
            marked_record_reference(
                bytes,
                start.checked_add(
                    shifted_reference_aware_323_symmetric::SYMMETRIC_EXTENT_REFERENCE,
                )?,
            )?,
            marked_record_reference(
                bytes,
                start
                    .checked_add(shifted_reference_aware_323_symmetric::PROFILE_GROUP_REFERENCE)?,
            )?,
        ]
        .iter()
        .all(|record_index| reference_members.contains(record_index))
    } else {
        [
            marked_record_reference(
                bytes,
                start.checked_add(shifted_reference_aware::FIRST_SIDE_OWNER_REFERENCE)?,
            )?,
            marked_record_reference(
                bytes,
                start.checked_add(shifted_reference_aware::SECOND_SIDE_OFFSET_REFERENCE)?,
            )?,
            marked_record_reference(
                bytes,
                start.checked_add(shifted_reference_aware::SECOND_SIDE_TAPER_REFERENCE)?,
            )?,
            marked_record_reference(
                bytes,
                start.checked_add(shifted_reference_aware::PROFILE_GROUP_REFERENCE)?,
            )?,
        ]
        .iter()
        .all(|record_index| reference_members.contains(record_index))
    };
    let trailing_reference =
        marked_record_reference(bytes, start.checked_add(trailing_reference_offset)?)?;
    let trailing_reference_valid = match tail_form {
        TailForm::Unordered => {
            trailing_reference != 0 && !reference_members.contains(&trailing_reference)
        }
        TailForm::Ordered | TailForm::SymmetricThroughAll => {
            reference_members.contains(&trailing_reference)
        }
    };
    let zero_range = |range_start: usize, range_end: usize| {
        bytes
            .get(start + range_start..start + range_end)
            .is_some_and(|value| value.iter().all(|byte| *byte == 0))
    };
    let tail_fixed_valid = if tail_form == TailForm::SymmetricThroughAll {
        zero_range(
            shifted_reference_aware_323_symmetric::FIRST_SIDE_PADDING,
            shifted_reference_aware_323_symmetric::SECOND_SIDE_EXTENT,
        ) && View::u32_le_at(
            bytes,
            start + shifted_reference_aware_323_symmetric::FIRST_SIDE_EXTENT,
        ) == Some(shifted_reference_aware_323_symmetric::FIRST_SIDE_EXTENT_VALUE)
            && zero_range(
                shifted_reference_aware_323_symmetric::SECOND_SIDE_PADDING,
                shifted_reference_aware_323_symmetric::SYMMETRIC_EXTENT_REFERENCE,
            )
            && zero_range(
                shifted_reference_aware_323_symmetric::SYMMETRIC_EXTENT_PADDING,
                shifted_reference_aware_323_symmetric::PROFILE_GROUP_COUNT,
            )
            && View::u32_le_at(
                bytes,
                start + shifted_reference_aware_323_symmetric::PROFILE_GROUP_COUNT,
            ) == Some(shifted_reference_aware_323_symmetric::PROFILE_GROUP_COUNT_VALUE)
            && zero_range(
                shifted_reference_aware_323_symmetric::PROFILE_GROUP_PADDING,
                shifted_reference_aware_323_symmetric::TRAILING_REFERENCE_COUNT,
            )
            && View::u32_le_at(
                bytes,
                start + shifted_reference_aware_323_symmetric::TRAILING_REFERENCE_COUNT,
            ) == Some(shifted_reference_aware_323_symmetric::TRAILING_REFERENCE_COUNT_VALUE)
    } else {
        bytes
            .get(
                start + shifted_reference_aware::FIRST_SIDE_PADDING
                    ..start + shifted_reference_aware::FIRST_SIDE_DISCRIMINANT,
            )
            .is_some_and(|value| value == [0; 4])
            && View::u32_le_at(
                bytes,
                start + shifted_reference_aware::FIRST_SIDE_DISCRIMINANT,
            ) == Some(1)
            && View::u32_le_at(bytes, start + shifted_reference_aware::FIRST_SIDE_PAYLOAD)
                == Some(2)
            && bytes.get(start + shifted_reference_aware::FIRST_SIDE_SEPARATOR) == Some(&0)
            && bytes
                .get(
                    start + shifted_reference_aware::SECOND_SIDE_OFFSET_PADDING
                        ..start + shifted_reference_aware::SECOND_SIDE_TAPER_REFERENCE,
                )
                .is_some_and(|value| value == [0; 4])
            && bytes
                .get(
                    start + shifted_reference_aware::SECOND_SIDE_TAPER_PADDING
                        ..start + shifted_reference_aware::PROFILE_GROUP_COUNT,
                )
                .is_some_and(|value| value == [0; 5])
            && View::u32_le_at(bytes, start + shifted_reference_aware::PROFILE_GROUP_COUNT)
                == Some(1)
            && zero_range(
                shifted_reference_aware::PROFILE_GROUP_PADDING,
                trailing_reference_count_offset,
            )
            && View::u32_le_at(bytes, start + trailing_reference_count_offset) == Some(1)
            && zero_range(
                trailing_reference_padding_offset,
                shifted_reference_aware::BODY_GROUP_GUID_PREFIX,
            )
    };
    if !ordered_tail_references_valid || !trailing_reference_valid || !tail_fixed_valid {
        return None;
    }
    let guid_end = fixed_guid_end(bytes, start.checked_add(guid_prefix_offset)?)?;
    let expected_guid_end = if tail_form == TailForm::SymmetricThroughAll {
        start.checked_add(guid_prefix_offset)?.checked_add(76)?
    } else {
        second_side_extent_offset.checked_add(1)?
    };
    if guid_end != expected_guid_end || bytes.get(guid_end..reference_count_at)? != [0; 3] {
        return None;
    }
    Some(DesignExtrudePrologue::ShiftedReferenceAware {
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(first_side_extent_offset).ok()?,
            u64::try_from(second_side_extent_offset).ok()?,
        ],
        extent,
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

pub(crate) fn exact_extrude_extent(
    direction: u32,
    side_extent_discriminators: [u32; 2],
) -> Option<DesignExtrudeExtent> {
    match (direction, side_extent_discriminators) {
        (1, [1, 0]) => Some(DesignExtrudeExtent::OneSidedDistance),
        (1, [2, 0]) => Some(DesignExtrudeExtent::OneSidedToFace),
        (1, [3, 0]) => Some(DesignExtrudeExtent::OneSidedThroughNext),
        (1, [4, 0]) => Some(DesignExtrudeExtent::OneSidedThroughAll),
        (2, [2, 0 | 2]) => Some(DesignExtrudeExtent::TwoSidedToFaces),
        (2, [1, 1]) => Some(DesignExtrudeExtent::TwoSidedDistance),
        (3, [1, 0]) => Some(DesignExtrudeExtent::SymmetricDistance),
        (3, [4, 4]) => Some(DesignExtrudeExtent::SymmetricThroughAll),
        _ => None,
    }
}

fn exact_legacy_shifted_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    if View::u32_le_at(bytes, start.checked_add(shifted_extrude::PREFIX_CONSTANT)?)? != 1
        || bytes.get(
            start.checked_add(shifted_extrude::ZERO_RUN_3)?
                ..start.checked_add(shifted_extrude::OPERATION)?,
        )? != [0; 3]
    {
        return None;
    }
    let marker_offset = start.checked_add(shifted_extrude::OPERATION)?;
    let (operation_prefix_marker_offset, field_shift) =
        if matches!(View::u32_le_at(bytes, marker_offset), Some(1..=4)) {
            (None, 0)
        } else if bytes.get(marker_offset) == Some(&1)
            && matches!(
                View::u32_le_at(bytes, marker_offset.checked_add(1)?),
                Some(1..=4)
            )
        {
            (Some(u64_from_index(marker_offset)), 1)
        } else {
            return None;
        };
    let operation_offset = marker_offset.checked_add(field_shift)?;
    let reference_count_delta = reference_count_at
        .checked_sub(start)?
        .checked_sub(field_shift)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let first_extent_offset = operation_offset.checked_add(4)?;
    let second_extent_offset = operation_offset.checked_add(8)?;
    let direction_face_extend_values = [
        View::u32_le_at(bytes, first_extent_offset)?,
        View::u32_le_at(bytes, second_extent_offset)?,
    ];
    if !matches!(direction_face_extend_values[0], 1..=3) {
        return None;
    }
    let two_sided_offsets = || {
        if reference_count_delta == 283 {
            let first_parameter_at =
                start.checked_add(shifted_283::FIRST_PARAMETER_REFERENCE + field_shift)?;
            let first_side_extent_offset =
                start.checked_add(shifted_283::FIRST_SIDE_EXTENT + field_shift)?;
            let second_parameter_at =
                start.checked_add(shifted_283::SECOND_PARAMETER_REFERENCE + field_shift)?;
            let second_side_extent_offset =
                start.checked_add(shifted_283::SECOND_SIDE_EXTENT + field_shift)?;
            let compact_valid = bytes
                .get(start.checked_add(150 + field_shift)?..first_side_extent_offset)?
                == [0; 16]
                && bytes.get(start.checked_add(175 + field_shift)?..second_side_extent_offset)?
                    == [0; 6]
                && [first_parameter_at, second_parameter_at]
                    .into_iter()
                    .map(|offset| marked_record_reference(bytes, offset))
                    .all(|reference| {
                        reference.is_some_and(|value| reference_members.contains(&value))
                    })
                && marked_record_reference(
                    bytes,
                    start.checked_add(shifted_283::TRAILING_ENTITY_REFERENCE + field_shift)?,
                )
                .is_some()
                && bytes.get(
                    start.checked_add(shifted_283::ZERO_RUN_8 + field_shift)?
                        ..start.checked_add(shifted_283::LEN + field_shift)?,
                )? == [0; 8];
            if compact_valid {
                return Some([first_side_extent_offset, second_side_extent_offset]);
            }
        }
        let first_parameter_at = start.checked_add(139 + field_shift)?;
        let first_side_extent_offset = start.checked_add(155 + field_shift)?;
        let first_offset_at = start.checked_add(159 + field_shift)?;
        let second_side_extent_offset = start.checked_add(178 + field_shift)?;
        let second_parameter_at = start.checked_add(182 + field_shift)?;
        if second_parameter_at.checked_add(11)? > reference_count_at
            || bytes.get(start.checked_add(150 + field_shift)?..first_side_extent_offset)? != [0; 5]
            || bytes.get(start.checked_add(170 + field_shift)?..second_side_extent_offset)?
                != [0; 8]
            || [first_parameter_at, first_offset_at, second_parameter_at]
                .into_iter()
                .map(|offset| marked_record_reference(bytes, offset))
                .any(|reference| !reference.is_some_and(|value| reference_members.contains(&value)))
        {
            return None;
        }
        Some([first_side_extent_offset, second_side_extent_offset])
    };
    let candidate = |first_side_extent_offset: usize, default_second_offset: usize| {
        if first_side_extent_offset.checked_add(4)? > reference_count_at {
            return None;
        }
        let first_side_extent = View::u32_le_at(bytes, first_side_extent_offset)?;
        let second_side_extent_offset = if first_side_extent == 2 {
            reference_count_at.checked_sub(4)?
        } else {
            default_second_offset
        };
        if second_side_extent_offset.checked_add(4)? > reference_count_at {
            return None;
        }
        let offsets = [first_side_extent_offset, second_side_extent_offset];
        let discriminators = [
            View::u32_le_at(bytes, offsets[0])?,
            View::u32_le_at(bytes, offsets[1])?,
        ];
        let extent = exact_extrude_extent(direction_face_extend_values[0], discriminators)?;
        Some((offsets, discriminators, extent))
    };
    let (side_extent_discriminator_offsets, side_extent_discriminators, extent) =
        if direction_face_extend_values[0] == 2 {
            let offsets = two_sided_offsets()?;
            let discriminators = [
                View::u32_le_at(bytes, offsets[0])?,
                View::u32_le_at(bytes, offsets[1])?,
            ];
            (
                offsets,
                discriminators,
                exact_extrude_extent(direction_face_extend_values[0], discriminators)?,
            )
        } else {
            let (first_offset, second_offset) = match reference_count_delta {
                262 if bytes.get(operation_offset.checked_add(extrude_fields::START_SUPPORT)?)
                    == Some(&1) =>
                {
                    (
                        offset_lane::FIRST_SIDE_EXTENT,
                        offset_lane::SECOND_SIDE_EXTENT,
                    )
                }
                252 | 262 | 263 | 692 => (106, 110),
                272 | 276 | 283 => (
                    offset_lane::FIRST_SIDE_EXTENT,
                    offset_lane::SECOND_SIDE_EXTENT,
                ),
                294 => (116, 129),
                _ => return None,
            };
            candidate(
                start.checked_add(first_offset + field_shift)?,
                start.checked_add(second_offset + field_shift)?,
            )?
        };
    let direction_reversed_offset =
        operation_offset.checked_add(extrude_fields::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = operation_offset.checked_add(extrude_fields::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = operation_offset.checked_add(extrude_fields::START_SUPPORT)?;
    let start = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset,
        operation,
        operation_offset: u64_from_index(operation_offset),
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64_from_index(side_extent_discriminator_offsets[0]),
            u64_from_index(side_extent_discriminator_offsets[1]),
        ],
        extent: Some(extent),
        direction_face_extend_offsets: [
            u64_from_index(first_extent_offset),
            u64_from_index(second_extent_offset),
        ],
        direction_reversed,
        direction_reversed_offset: u64_from_index(direction_reversed_offset),
        solid_operation,
        solid_operation_offset: u64_from_index(solid_operation_offset),
        start,
        start_offset: u64_from_index(start_offset),
    })
}

fn exact_class_338_two_sided_distance_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

    if class_tag != "338"
        || paired_class_tag != "262"
        || paired_at.checked_sub(start)? != class_338_legacy::LEN
        || reference_count_at.checked_sub(start)? != class_338_legacy::REFERENCE_COUNT
        || reference_members.len() != 10
        || bytes.get(paired_at.checked_add(4)?..paired_at.checked_add(7)?)? != b"262"
        || View::u32_le_at(bytes, start.checked_add(class_338_legacy::PREFIX_CONSTANT)?)?
            != class_338_legacy::PREFIX_CONSTANT_VALUE
        || bytes.get(start.checked_add(24)?..start.checked_add(class_338_legacy::OPERATION)?)?
            != [0; 3]
        || bytes
            .get(start.checked_add(42)?..start.checked_add(class_338_legacy::PROFILE_NORMAL)?)?
            != [0; 3]
        || bytes.get(
            start.checked_add(class_338_legacy::NULL_SCOPE_SCALAR_LANE)?
                ..start.checked_add(class_338_legacy::NULL_SCOPE_SCALAR_LANE + 10)?,
        )? != [1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        || View::u32_le_at(bytes, reference_count_at)? != class_338_legacy::REFERENCE_COUNT_VALUE
    {
        return None;
    }
    let operation_offset = start.checked_add(class_338_legacy::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_338_legacy::DIRECTION)?,
        start.checked_add(class_338_legacy::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values
        != [
            class_338_legacy::DIRECTION_VALUE,
            class_338_legacy::FACE_EXTEND_VALUE,
        ]
    {
        return None;
    }
    let direction_reversed_offset = start.checked_add(class_338_legacy::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_338_legacy::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_338_legacy::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    let profile_normal = f64s_at::<3>(bytes, start.checked_add(class_338_legacy::PROFILE_NORMAL)?)?;
    let profile_normal_squared = profile_normal
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if profile_normal
        .iter()
        .any(|component| !component.is_finite())
        || (profile_normal_squared - 1.0).abs() > PROFILE_NORMAL_UNIT_EPS
    {
        return None;
    }
    for offset in [
        class_338_legacy::FIRST_SIDE_PARAMETER_REFERENCE,
        class_338_legacy::PROFILE_GROUP_REFERENCE,
        class_338_legacy::BODY_GROUP_REFERENCE,
    ] {
        let record_index = marked_record_reference(bytes, start.checked_add(offset)?)?;
        if !reference_members.contains(&record_index) {
            return None;
        }
    }
    if bytes
        .get(start.checked_add(160)?..start.checked_add(class_338_legacy::FIRST_SIDE_EXTENT)?)?
        != [0; 5]
        || View::u32_le_at(
            bytes,
            start.checked_add(class_338_legacy::FIRST_SIDE_EXTENT)?,
        )? != class_338_legacy::FIRST_SIDE_EXTENT_VALUE
        || bytes.get(
            start.checked_add(180)?..start.checked_add(class_338_legacy::SECOND_SIDE_EXTENT)?,
        )? != [0; 8]
        || View::u32_le_at(
            bytes,
            start.checked_add(class_338_legacy::SECOND_SIDE_EXTENT)?,
        )? != class_338_legacy::SECOND_SIDE_EXTENT_VALUE
    {
        return None;
    }
    let guid_end = fixed_guid_end(bytes, start.checked_add(class_338_legacy::GUID)?)?;
    let expected_guid_end = start.checked_add(279)?;
    if guid_end != expected_guid_end || bytes.get(guid_end..reference_count_at)? != [0; 3] {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators: [
            class_338_legacy::FIRST_SIDE_EXTENT_VALUE,
            class_338_legacy::SECOND_SIDE_EXTENT_VALUE,
        ],
        side_extent_discriminator_offsets: [
            u64::try_from(start.checked_add(class_338_legacy::FIRST_SIDE_EXTENT)?).ok()?,
            u64::try_from(start.checked_add(class_338_legacy::SECOND_SIDE_EXTENT)?).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::TwoSidedDistance),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

#[cfg(test)]
mod tests;
