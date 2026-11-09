// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, View};

use crate::bytes::{f64s_at, lp_utf16_bounded};
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::layout::class_349_two_sided_through_all_extrude_frame as layout;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::DesignParameterScope;

const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

pub(crate) fn matches_frame(
    class: &str,
    paired_class: &str,
    length: u64,
    reference_count_offset: u64,
    reference_count: usize,
) -> bool {
    class == "349"
        && paired_class == "262"
        && length == u64_from_index(layout::LEN)
        && reference_count_offset == u64_from_index(layout::REFERENCE_COUNT)
        && u32::try_from(reference_count) == Ok(layout::REFERENCE_COUNT_VALUE)
}

pub(crate) fn matches_scope(scope: &DesignParameterScope) -> bool {
    if !matches!(scope.extrude_prologue(), Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None, operation_offset, ..
    }) if scope.byte_offset().checked_add(u64_from_index(layout::OPERATION)) == Some(operation_offset))
    {
        return false;
    }
    scope
        .reference_count_offset()
        .checked_sub(scope.byte_offset())
        .is_some_and(|offset| {
            matches_frame(
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
                scope.frame_length(),
                offset,
                scope.reference_members().len(),
            )
        })
}

pub(super) fn exact(
    bytes: &[u8],
    start: usize,
    references: &[u32],
) -> Option<DesignExtrudePrologue> {
    for (offset, expected) in [
        (layout::PREFIX_CONSTANT, layout::PREFIX_CONSTANT_VALUE),
        (layout::OPERATION, layout::OPERATION_VALUE),
        (layout::DIRECTION, layout::DIRECTION_VALUE),
        (layout::FACE_EXTEND, layout::FACE_EXTEND_VALUE),
        (layout::FIRST_SIDE_EXTENT, layout::FIRST_SIDE_EXTENT_VALUE),
        (layout::SECOND_SIDE_EXTENT, layout::SECOND_SIDE_EXTENT_VALUE),
        (
            layout::PROFILE_GROUP_COUNT,
            layout::PROFILE_GROUP_COUNT_VALUE,
        ),
        (layout::BODY_GROUP_COUNT, layout::BODY_GROUP_COUNT_VALUE),
        (layout::REFERENCE_COUNT, layout::REFERENCE_COUNT_VALUE),
    ] {
        if View::u32_le_at(bytes, start.checked_add(offset)?)? != expected {
            return None;
        }
    }
    for (offset, expected) in [
        (layout::DIRECTION_REVERSED, layout::DIRECTION_REVERSED_VALUE),
        (layout::GEOMETRY_KIND, layout::GEOMETRY_KIND_VALUE),
        (layout::START_SUPPORT, layout::START_SUPPORT_VALUE),
    ] {
        if bytes.get(start.checked_add(offset)?).copied()? != expected {
            return None;
        }
    }
    for (offset, expected) in [
        (layout::ZERO_RUN_9, layout::ZERO_RUN_9_VALUE.as_slice()),
        (layout::ZERO_RUN_3, layout::ZERO_RUN_3_VALUE.as_slice()),
        (
            layout::NORMAL_PADDING,
            layout::NORMAL_PADDING_VALUE.as_slice(),
        ),
        (layout::ABSENT_SLOTS, layout::ABSENT_SLOTS_VALUE.as_slice()),
        (
            layout::EXTENT_SEPARATOR,
            layout::EXTENT_SEPARATOR_VALUE.as_slice(),
        ),
        (
            layout::SECOND_TAPER_PADDING,
            layout::SECOND_TAPER_PADDING_VALUE.as_slice(),
        ),
        (
            layout::PROFILE_GROUP_PADDING,
            layout::PROFILE_GROUP_PADDING_VALUE.as_slice(),
        ),
        (
            layout::BODY_GROUP_PADDING,
            layout::BODY_GROUP_PADDING_VALUE.as_slice(),
        ),
        (
            layout::REFERENCE_COUNT_PADDING,
            layout::REFERENCE_COUNT_PADDING_VALUE.as_slice(),
        ),
    ] {
        let at = start.checked_add(offset)?;
        if bytes.get(at..at.checked_add(expected.len())?)? != expected {
            return None;
        }
    }
    for (offset, ordinal) in [
        (layout::TRAILING_REFERENCE, 10),
        (layout::SECOND_BODY_GROUP_REFERENCE, 7),
        (layout::FIRST_TAPER_REFERENCE, 6),
        (layout::FIRST_BODY_GROUP_REFERENCE, 4),
        (layout::SECOND_TAPER_REFERENCE, 9),
        (layout::PROFILE_GROUP_REFERENCE, 2),
        (layout::BODY_GROUP_REFERENCE, 0),
    ] {
        if marked_record_reference(bytes, start.checked_add(offset)?)?
            != *references.get(ordinal)?
        {
            return None;
        }
    }
    let normal = f64s_at::<3>(bytes, start.checked_add(layout::PROFILE_NORMAL)?)?;
    let squared = normal.iter().map(|value| value * value).sum::<f64>();
    if !squared.is_finite() || (squared - 1.0).abs() > PROFILE_NORMAL_UNIT_EPS {
        return None;
    }
    let (guid, guid_end) = lp_utf16_bounded(bytes, start.checked_add(layout::NULL_GUID)?, 36..=36)?;
    if guid != "00000000-0000-0000-0000-000000000000"
        || guid_end != start.checked_add(layout::REFERENCE_COUNT_PADDING)?
    {
        return None;
    }
    prologue(start)
}

pub(crate) fn valid_prologue(scope: &DesignParameterScope, value: DesignExtrudePrologue) -> bool {
    matches_scope(scope)
        && usize::try_from(scope.byte_offset()).ok().and_then(prologue) == Some(value)
}

fn prologue(start: usize) -> Option<DesignExtrudePrologue> {
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation: DesignExtrudeOperation::Cut,
        operation_offset: u64_from_index(start.checked_add(layout::OPERATION)?),
        direction_face_extend_values: [layout::DIRECTION_VALUE, layout::FACE_EXTEND_VALUE],
        side_extent_discriminators: [
            layout::FIRST_SIDE_EXTENT_VALUE,
            layout::SECOND_SIDE_EXTENT_VALUE,
        ],
        side_extent_discriminator_offsets: [
            u64_from_index(start.checked_add(layout::FIRST_SIDE_EXTENT)?),
            u64_from_index(start.checked_add(layout::SECOND_SIDE_EXTENT)?),
        ],
        extent: Some(DesignExtrudeExtent::TwoSidedThroughAll),
        direction_face_extend_offsets: [
            u64_from_index(start.checked_add(layout::DIRECTION)?),
            u64_from_index(start.checked_add(layout::FACE_EXTEND)?),
        ],
        direction_reversed: false,
        direction_reversed_offset: u64_from_index(start.checked_add(layout::DIRECTION_REVERSED)?),
        solid_operation: true,
        solid_operation_offset: u64_from_index(start.checked_add(layout::GEOMETRY_KIND)?),
        start: DesignExtrudeStart::ProfilePlane,
        start_offset: u64_from_index(start.checked_add(layout::START_SUPPORT)?),
    })
}

pub(crate) fn taper_parameters_match<'a>(
    scope: &DesignParameterScope,
    parameters: impl IntoIterator<Item = &'a crate::records::parameters::DesignParameter>,
) -> bool {
    let mut seen = [false; 2];
    for parameter in parameters {
        let (side, ordinal) = match parameter.source_kind().as_str() {
            "TaperAngle" => (0, 6),
            "Side2TaperAngle" => (1, 9),
            _ => return false,
        };
        let owner = scope.reference_members().values().nth(ordinal).copied();
        if seen[side] || owner.is_none() || parameter.owner_record_index() != owner {
            return false;
        }
        seen[side] = true;
    }
    seen == [true, true]
}

#[cfg(test)]
mod tests;
