// SPDX-License-Identifier: Apache-2.0

use crate::layout::canvas_geometry_extra12_prefix as extra12;
use crate::layout::canvas_geometry_legacy_prefix as legacy;
use crate::layout::canvas_geometry_prefix as modern;
use crate::layout::canvas_geometry_transitional_prefix as transitional;
use crate::layout::canvas_geometry_typed_legacy_prefix as typed_legacy;
use crate::layout::canvas_geometry_typed_transitional_prefix as typed_transitional;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignCanvasGeometryLayout {
    Modern,
    Legacy,
    Transitional,
    Extra12,
    TypedLegacy,
    TypedTransitional,
}

#[derive(Clone, Copy)]
pub(crate) struct CanvasOffsets {
    pub(crate) first_boundary: usize,
    pub(crate) plane: usize,
    pub(crate) payload: usize,
    pub(crate) scope: usize,
    pub(crate) component: usize,
    pub(crate) asset: usize,
    pub(crate) state: usize,
    pub(crate) second_boundary: usize,
    pub(crate) label: usize,
    pub(crate) prefix: usize,
}
impl DesignCanvasGeometryLayout {
    pub(crate) const ALL: [Self; 6] = [
        Self::Modern,
        Self::Legacy,
        Self::Transitional,
        Self::Extra12,
        Self::TypedLegacy,
        Self::TypedTransitional,
    ];
    pub(crate) fn offsets(self) -> CanvasOffsets {
        match self {
            Self::Modern => CanvasOffsets {
                first_boundary: modern::FIRST_BOUNDARY,
                plane: modern::PLANE_REFERENCE,
                payload: modern::GEOMETRY_PAYLOAD,
                scope: modern::SCOPE_REFERENCE,
                component: modern::COMPONENT_REFERENCE,
                asset: modern::ASSET_REFERENCE,
                state: modern::SECOND_BOUNDARY_PRESENT,
                second_boundary: modern::SECOND_BOUNDARY,
                label: modern::LABEL_CODE_UNIT_COUNT,
                prefix: modern::LEN,
            },
            Self::Legacy => CanvasOffsets {
                first_boundary: legacy::FIRST_BOUNDARY,
                plane: legacy::PLANE_REFERENCE,
                payload: legacy::GEOMETRY_PAYLOAD,
                scope: legacy::SCOPE_REFERENCE,
                component: legacy::COMPONENT_REFERENCE,
                asset: legacy::ASSET_REFERENCE,
                state: legacy::SECOND_BOUNDARY_PRESENT,
                second_boundary: legacy::SECOND_BOUNDARY,
                label: legacy::LABEL_CODE_UNIT_COUNT,
                prefix: legacy::LEN,
            },
            Self::Transitional => CanvasOffsets {
                first_boundary: transitional::FIRST_BOUNDARY,
                plane: transitional::PLANE_REFERENCE,
                payload: transitional::GEOMETRY_PAYLOAD,
                scope: transitional::SCOPE_REFERENCE,
                component: transitional::COMPONENT_REFERENCE,
                asset: transitional::ASSET_REFERENCE,
                state: transitional::SECOND_BOUNDARY_PRESENT,
                second_boundary: transitional::SECOND_BOUNDARY,
                label: transitional::LABEL_CODE_UNIT_COUNT,
                prefix: transitional::LEN,
            },
            Self::Extra12 => CanvasOffsets {
                first_boundary: extra12::FIRST_BOUNDARY,
                plane: extra12::PLANE_REFERENCE,
                payload: extra12::GEOMETRY_PAYLOAD,
                scope: extra12::SCOPE_REFERENCE,
                component: extra12::COMPONENT_REFERENCE,
                asset: extra12::ASSET_REFERENCE,
                state: extra12::SECOND_BOUNDARY_PRESENT,
                second_boundary: extra12::SECOND_BOUNDARY,
                label: extra12::LABEL_CODE_UNIT_COUNT,
                prefix: extra12::LEN,
            },
            Self::TypedLegacy => CanvasOffsets {
                first_boundary: typed_legacy::FIRST_BOUNDARY,
                plane: typed_legacy::PLANE_REFERENCE,
                payload: typed_legacy::GEOMETRY_PAYLOAD,
                scope: typed_legacy::SCOPE_REFERENCE,
                component: typed_legacy::COMPONENT_REFERENCE,
                asset: typed_legacy::ASSET_REFERENCE,
                state: typed_legacy::SECOND_BOUNDARY_PRESENT,
                second_boundary: typed_legacy::SECOND_BOUNDARY,
                label: typed_legacy::LABEL_CODE_UNIT_COUNT,
                prefix: typed_legacy::LEN,
            },
            Self::TypedTransitional => CanvasOffsets {
                first_boundary: typed_transitional::FIRST_BOUNDARY,
                plane: typed_transitional::PLANE_REFERENCE,
                payload: typed_transitional::GEOMETRY_PAYLOAD,
                scope: typed_transitional::SCOPE_REFERENCE,
                component: typed_transitional::COMPONENT_REFERENCE,
                asset: typed_transitional::ASSET_REFERENCE,
                state: typed_transitional::SECOND_BOUNDARY_PRESENT,
                second_boundary: typed_transitional::SECOND_BOUNDARY,
                label: typed_transitional::LABEL_CODE_UNIT_COUNT,
                prefix: typed_transitional::LEN,
            },
        }
    }
    pub(crate) fn typed(self) -> bool {
        matches!(self, Self::TypedLegacy | Self::TypedTransitional)
    }
    pub(crate) fn reference_len(self) -> usize {
        if self.typed() {
            51
        } else {
            11
        }
    }
    pub(crate) fn paired_len(self) -> u64 {
        if self.typed() {
            70
        } else {
            30
        }
    }
    pub(crate) fn from_prefix(prefix: u64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|layout| cadmpeg_core::decode::u64_from_index(layout.offsets().prefix) == prefix)
    }
}
