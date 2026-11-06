// SPDX-License-Identifier: Apache-2.0

use super::{legacy_edge_flange_operation_at, LegacyEdgeFlangeLayout};
use crate::layout::edge_flange_corner_operation as layout;
use crate::records::feature::sheet_metal::{
    DesignEdgeFlangeOperation, DesignEdgeFlangeWidthParameterSource, DesignEdgeWidthMode,
};
use cadmpeg_core::decode::{u64_from_index, View};

const LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: layout::LEN,
    bend_position_offset: layout::BEND_POSITION,
    edge_count_offset: layout::EDGE_COUNT,
    edge_columns: &[
        (
            layout::EDGE_WRAPPER_ONE_REFERENCE,
            layout::EDGE_GROUP_ONE_REFERENCE,
        ),
        (
            layout::EDGE_WRAPPER_TWO_REFERENCE,
            layout::EDGE_GROUP_TWO_REFERENCE,
        ),
    ],
    settings_offset: layout::SETTINGS_REFERENCE,
    height_datum_offset: layout::HEIGHT_DATUM,
    angle_owner_offset: layout::ANGLE_OWNER_REFERENCE,
    height_owner_offset: layout::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: layout::INSIDE_BEND_RADIUS,
    result_count_offset: layout::RESULT_COUNT,
    result_reference_start: layout::RESULT_ONE_REFERENCE,
    result_trailer_start: layout::RESULT_ONE_TRAILER,
    result_separator_offset: layout::RESULT_SEPARATOR,
    aggregate_group_offset: layout::AGGREGATE_GROUP_REFERENCE,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::FullEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[
        layout::RESULT_ONE_TRAILER_VALUE,
        layout::RESULT_TWO_TRAILER_VALUE,
        layout::RESULT_THREE_TRAILER_VALUE,
    ],
};

pub(super) fn operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
) -> Option<DesignEdgeFlangeOperation> {
    if references.len() != LAYOUT.reference_count()
        || references.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return None;
    }
    for (offset, expected) in [
        (layout::BEND_POSITION, layout::BEND_POSITION_VALUE),
        (layout::HEIGHT_DATUM, layout::HEIGHT_DATUM_VALUE),
        (layout::REFERENCE_SIDE, layout::REFERENCE_SIDE_VALUE),
        (layout::EDGE_GROUP_COUNT, layout::EDGE_GROUP_COUNT_VALUE),
    ] {
        if View::u32_le_at(bytes, start.checked_add(offset)?)? != expected {
            return None;
        }
    }
    let mut view = View::over_retained(bytes);
    for (offset, expected) in [
        (layout::SIDE_PADDING, layout::SIDE_PADDING_VALUE.as_slice()),
        (
            layout::RADIUS_PADDING,
            layout::RADIUS_PADDING_VALUE.as_slice(),
        ),
        (
            layout::GROUP_PADDING,
            layout::GROUP_PADDING_VALUE.as_slice(),
        ),
    ] {
        view.seek(start.checked_add(offset)?)?;
        if view.take(expected.len())? != expected {
            return None;
        }
    }
    let operation = legacy_edge_flange_operation_at(bytes, start, paired_at, references, LAYOUT)?;
    operation_holds(
        &operation,
        u64::try_from(start).ok()?,
        u64::try_from(paired_at.checked_sub(start)?).ok()?,
    )
    .then_some(operation)
}

pub(crate) fn operation_holds(
    operation: &DesignEdgeFlangeOperation,
    start: u64,
    frame_length: u64,
) -> bool {
    use crate::records::feature::sheet_metal::{
        DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeShape,
        DesignSheetMetalHeightDatum,
    };
    frame_length == u64_from_index(layout::LEN)
        && start.checked_add(u64_from_index(layout::INSIDE_BEND_RADIUS))
            == Some(operation.bend_radius_offset)
        && operation.bend_position == DesignBendPosition::Inside
        && operation.height_datum == DesignSheetMetalHeightDatum::OuterFaces
        && matches!(operation.selection.shape(), DesignEdgeFlangeShape::FullEdge { edges, height: DesignEdgeFlangeHeightExtent::Distance } if u32::try_from(edges.len()).ok() == Some(layout::EDGE_COUNT_VALUE))
        && operation.auxiliary_reference_record_indices.is_empty()
}

#[cfg(test)]
mod tests;
