// SPDX-License-Identifier: Apache-2.0
//! Parse the legacy class-397 symmetric-distance Extrude grammar.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use crate::bytes::f64s_at;
use crate::design::decode::text::fixed_guid_end;
use crate::layout::legacy_class_397_symmetric_extrude_frame as symmetric;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use cadmpeg_core::decode::View;

/// Checked class, frame length, and reference-run layout for the class-397 grammar.
#[derive(Clone, Copy)]
pub(crate) struct Class397SymmetricFrame(());

impl Class397SymmetricFrame {
    /// Admit the fixed class-397 symmetric-distance frame layout.
    pub(crate) fn new(
        class_tag: &str,
        paired_class_tag: &str,
        frame_length: u64,
        reference_count_offset: u64,
        reference_count: usize,
    ) -> Option<Self> {
        (class_tag == "397"
            && paired_class_tag == "262"
            && frame_length == u64_from_index(symmetric::LEN)
            && reference_count_offset == u64_from_index(symmetric::REFERENCE_COUNT)
            && reference_count == index_from_u32(symmetric::REFERENCE_COUNT_VALUE))
        .then_some(Self(()))
    }

    /// The symmetric-distance extent admitted by this frame grammar.
    pub(crate) fn extent(
        self,
        direction: u32,
        side_extent_discriminators: [u32; 2],
    ) -> Option<DesignExtrudeExtent> {
        match (self, direction, side_extent_discriminators) {
            (Self(()), 3, [1, 1]) => Some(DesignExtrudeExtent::SymmetricDistance),
            _ => None,
        }
    }
}

pub(super) fn exact_symmetric_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

    let frame = Class397SymmetricFrame::new(
        class_tag,
        paired_class_tag,
        u64::try_from(paired_at.checked_sub(start)?).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    )?;
    if View::u32_le_at(bytes, start.checked_add(symmetric::PREFIX_CONSTANT)?)?
        != symmetric::PREFIX_CONSTANT_VALUE
        || bytes.get(
            start.checked_add(symmetric::PREFIX_CONSTANT + 4)?
                ..start.checked_add(symmetric::OPERATION)?,
        )? != [0; 3]
    {
        return None;
    }

    let operation_offset = start.checked_add(symmetric::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let direction_face_extend_offsets = [
        start.checked_add(symmetric::DIRECTION)?,
        start.checked_add(symmetric::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values != [symmetric::DIRECTION_VALUE, symmetric::FACE_EXTEND_VALUE] {
        return None;
    }

    let direction_reversed_offset = start.checked_add(symmetric::DIRECTION_REVERSED)?;
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(symmetric::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(symmetric::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };

    let profile_normal = f64s_at::<3>(bytes, start.checked_add(symmetric::PROFILE_NORMAL)?)?;
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

    let mut slot_offset = start.checked_add(symmetric::REFERENCE_SLOTS)?;
    for expected_present in [true, true, true, true, false, true, false] {
        let present = match bytes.get(slot_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        if present != expected_present {
            return None;
        }
        if present {
            let record_index = super::shared_frames::marked_record_reference(bytes, slot_offset)?;
            if !reference_members.contains(&record_index) {
                return None;
            }
            slot_offset = slot_offset.checked_add(11)?;
        } else {
            slot_offset = slot_offset.checked_add(1)?;
        }
    }
    if slot_offset != start.checked_add(symmetric::FIRST_SIDE_EXTENT)? {
        return None;
    }

    let first_side_extent_offset = start.checked_add(symmetric::FIRST_SIDE_EXTENT)?;
    let second_side_extent_offset = start.checked_add(symmetric::SECOND_SIDE_EXTENT)?;
    let side_extent_discriminators = [
        View::u32_le_at(bytes, first_side_extent_offset)?,
        View::u32_le_at(bytes, second_side_extent_offset)?,
    ];
    let extent = frame.extent(direction_face_extend_values[0], side_extent_discriminators)?;
    if bytes.get(first_side_extent_offset.checked_add(4)?..second_side_extent_offset)? != [0; 9] {
        return None;
    }

    let guid_offset = start.checked_add(symmetric::GUID)?;
    let guid_end = fixed_guid_end(bytes, guid_offset)?;
    let reference_count_offset = start.checked_add(symmetric::REFERENCE_COUNT)?;
    if guid_end != guid_offset.checked_add(76)?
        || bytes.get(guid_end..reference_count_offset)? != [0; 3]
        || View::u32_le_at(bytes, reference_count_offset)? != symmetric::REFERENCE_COUNT_VALUE
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
            u64::try_from(first_side_extent_offset).ok()?,
            u64::try_from(second_side_extent_offset).ok()?,
        ],
        extent: Some(extent),
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
