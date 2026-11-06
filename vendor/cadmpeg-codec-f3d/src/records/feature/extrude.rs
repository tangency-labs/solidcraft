// SPDX-License-Identifier: Apache-2.0
//! Extrude operations, extents, starts and the extrude prologue.

use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_operation_prefix_marker,
    u8,
    "operation_prefix_marker"
);
cadmpeg_core::named_optional_field!(
    deserialize_operation_prefix_marker_offset,
    u64,
    "operation_prefix_marker_offset"
);
/// Boolean result operation stored by an Extrude parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignExtrudeOperation {
    /// Union the swept volume with the selected bodies.
    Join,
    /// Subtract the swept volume from the selected bodies.
    Cut,
    /// Retain the intersection of the swept volume and selected bodies.
    Intersect,
    /// Create an independent body.
    NewBody,
}

/// Decoded Extrude travel form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignExtrudeExtent {
    /// Travel a signed fixed distance on the first side of the profile.
    OneSidedDistance,
    /// Travel on the first side until reaching a selected face or shape.
    OneSidedToFace,
    /// Travel on both sides until each side reaches its selected face.
    TwoSidedToFaces,
    /// Travel independent fixed distances on both sides of the profile.
    TwoSidedDistance,
    /// Travel a fixed distance on the first side and to a selected face on the second side.
    TwoSidedDistanceToFace,
    /// Travel one fixed total distance symmetrically around the profile plane.
    SymmetricDistance,
    /// Travel symmetrically through all material on both sides of the profile plane.
    SymmetricThroughAll,
    /// Travel on the first side until the next material region is exited.
    OneSidedThroughNext,
    /// Travel on the first side through all material.
    OneSidedThroughAll,
}

/// Starting support selected by the fixed Extrude prologue enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignExtrudeStart {
    /// Start on the selected sketch's plane.
    ProfilePlane,
    /// Start on a parallel offset from the selected sketch's plane.
    OffsetProfilePlane,
    /// Start on a selected face.
    FromFace,
}

/// Indexed-record prefix preceding a reference-aware Extrude prologue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignExtrudePrologueReferenceWire",
    into = "DesignExtrudePrologueReferenceWire"
)]
pub(crate) struct DesignExtrudePrologueReference {
    /// Referenced Design record.
    pub(crate) record_index: u32,
    /// Byte offset of `record_index`.
    pub(crate) record_index_offset: u64,
    /// Number of zero bytes between `record_index` and the operation or its marker.
    pub(crate) trailing_zero_count: u8,
    /// Byte offset of the optional marker 1 before the operation.
    pub(crate) operation_prefix_marker_offset: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct DesignExtrudePrologueReferenceWire {
    /// Referenced Design record.
    record_index: u32,
    /// Byte offset of `record_index`.
    record_index_offset: u64,
    /// Number of zero bytes between `record_index` and the operation or its marker.
    trailing_zero_count: u8,
    /// Optional marker byte between the zero run and the operation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_operation_prefix_marker"
    )]
    operation_prefix_marker: Option<u8>,
    /// Byte offset of `operation_prefix_marker` when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_operation_prefix_marker_offset"
    )]
    operation_prefix_marker_offset: Option<u64>,
}

impl TryFrom<DesignExtrudePrologueReferenceWire> for DesignExtrudePrologueReference {
    type Error = String;
    fn try_from(wire: DesignExtrudePrologueReferenceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            record_index_offset: wire.record_index_offset,
            trailing_zero_count: wire.trailing_zero_count,
            operation_prefix_marker_offset: match (wire.operation_prefix_marker, wire.operation_prefix_marker_offset) {
                (None, None) => None,
                (Some(1), Some(offset)) => Some(offset),
                _ => return Err("operation_prefix_marker must be 1 and occur with operation_prefix_marker_offset".into()),
            },
        })
    }
}

impl From<DesignExtrudePrologueReference> for DesignExtrudePrologueReferenceWire {
    fn from(record: DesignExtrudePrologueReference) -> Self {
        Self {
            record_index: record.record_index,
            record_index_offset: record.record_index_offset,
            trailing_zero_count: record.trailing_zero_count,
            operation_prefix_marker: record.operation_prefix_marker_offset.map(|_| 1),
            operation_prefix_marker_offset: record.operation_prefix_marker_offset,
        }
    }
}

/// Scope-reference ordinal repeated before a whole-body Extrude target extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignExtrudeTargetOrdinal {
    /// Zero-based ordinal in the enclosing scope reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Byte offset of `scope_reference_ordinal`.
    pub(crate) scope_reference_ordinal_offset: u64,
}

/// Fixed fields preceding an Extrude parameter scope's reference table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignExtrudePrologueWire",
    into = "DesignExtrudePrologueWire"
)]
pub(crate) enum DesignExtrudePrologue {
    /// Early distance-only layout with a nullable prefix field.
    LegacyDistance {
        /// Byte offset of the fixed zero prefix when its marker is present.
        prefix_zero_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Byte offset of the fixed extent-kind value `2`.
        #[serde(alias = "extent_discriminator_offset")]
        extent_kind_offset: u64,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of the geometry-kind integer.
        solid_operation_offset: u64,
    },
    /// Reference-aware layout with an optional indexed-reference prefix.
    ReferenceAware {
        /// Indexed-record prefix, when present.
        reference: Option<DesignExtrudePrologueReference>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored after the profile normal and reference slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Repeated target-group ordinal in the whole-body target form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_side_target_ordinal: Option<DesignExtrudeTargetOrdinal>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted reference-aware two-sided face-target layout.
    ShiftedReferenceAware {
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored in the fixed legacy tail.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted layout without the reference-aware prefix.
    LegacyShifted {
        /// Byte offset of the optional marker 1 before the operation.
        operation_prefix_marker_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side termination discriminators stored after the profile-normal slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<DesignExtrudeExtent>,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "layout")]
enum DesignExtrudePrologueWire {
    /// Early distance-only layout with a nullable prefix field.
    LegacyDistance {
        /// Value of the nullable prefix field when its marker is present.
        prefix_value: Option<u32>,
        /// Byte offset of `prefix_value` when present.
        prefix_value_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw extent-kind value (`2 = one-sided distance`).
        #[serde(alias = "extent_discriminator")]
        extent_kind: u32,
        /// Byte offset of `extent_kind`.
        #[serde(alias = "extent_discriminator_offset")]
        extent_kind_offset: u64,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Raw geometry-kind discriminator (`0 = sheet`, `1 = solid`).
        geometry_kind: u32,
        /// Byte offset of `geometry_kind`.
        geometry_kind_offset: u64,
    },
    /// Reference-aware layout with an optional indexed-reference prefix.
    ReferenceAware {
        /// Indexed-record prefix, when present.
        reference: Option<DesignExtrudePrologueReference>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored after the profile normal and reference slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Repeated target-group ordinal in the whole-body target form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_side_target_ordinal: Option<DesignExtrudeTargetOrdinal>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted reference-aware two-sided face-target layout.
    ShiftedReferenceAware {
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored in the fixed legacy tail.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted layout without the reference-aware prefix.
    LegacyShifted {
        /// Optional marker immediately before the operation fields.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_prefix_marker: Option<u8>,
        /// Byte offset of `operation_prefix_marker` when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_prefix_marker_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side termination discriminators stored after the profile-normal slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<DesignExtrudeExtent>,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
}

impl TryFrom<DesignExtrudePrologueWire> for DesignExtrudePrologue {
    type Error = String;
    fn try_from(wire: DesignExtrudePrologueWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignExtrudePrologueWire::LegacyDistance { prefix_value, prefix_value_offset, operation, operation_offset, extent_kind, extent_kind_offset, direction_reversed, direction_reversed_offset, geometry_kind, geometry_kind_offset } => {
                let prefix_zero_offset = match (prefix_value, prefix_value_offset) {
                    (None, None) => None,
                    (Some(0), Some(offset)) => Some(offset),
                    _ => return Err("prefix_value must be zero and occur with prefix_value_offset".into()),
                };
                if extent_kind != 2 {
                    return Err("extent_kind must be 2 in a legacy distance prologue".into());
                }
                let solid_operation = match geometry_kind {
                    0 => false,
                    1 => true,
                    _ => return Err("geometry_kind must be 0 or 1".into()),
                };
                Self::LegacyDistance { prefix_zero_offset, operation, operation_offset, extent_kind_offset, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset: geometry_kind_offset }
            },
            DesignExtrudePrologueWire::ReferenceAware { reference, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, first_side_target_ordinal, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::ReferenceAware { reference, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, first_side_target_ordinal, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
            DesignExtrudePrologueWire::ShiftedReferenceAware { operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::ShiftedReferenceAware { operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
            DesignExtrudePrologueWire::LegacyShifted { operation_prefix_marker, operation_prefix_marker_offset, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::LegacyShifted { operation_prefix_marker_offset: match (operation_prefix_marker, operation_prefix_marker_offset) {
                (None, None) => None,
                (Some(1), Some(offset)) => Some(offset),
                _ => return Err("operation_prefix_marker must be 1 and occur with operation_prefix_marker_offset".into()),
            }, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
        })
    }
}

impl From<DesignExtrudePrologue> for DesignExtrudePrologueWire {
    fn from(record: DesignExtrudePrologue) -> Self {
        match record {
            DesignExtrudePrologue::LegacyDistance {
                prefix_zero_offset,
                operation,
                operation_offset,
                extent_kind_offset,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
            } => Self::LegacyDistance {
                prefix_value: prefix_zero_offset.map(|_| 0),
                prefix_value_offset: prefix_zero_offset,
                operation,
                operation_offset,
                extent_kind: 2,
                extent_kind_offset,
                direction_reversed,
                direction_reversed_offset,
                geometry_kind: u32::from(solid_operation),
                geometry_kind_offset: solid_operation_offset,
            },
            DesignExtrudePrologue::ReferenceAware {
                reference,
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                first_side_target_ordinal,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            } => Self::ReferenceAware {
                reference,
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                first_side_target_ordinal,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            },
            DesignExtrudePrologue::ShiftedReferenceAware {
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            } => Self::ShiftedReferenceAware {
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            },
            DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker_offset,
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            } => Self::LegacyShifted {
                operation_prefix_marker: operation_prefix_marker_offset.map(|_| 1),
                operation_prefix_marker_offset,
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            },
        }
    }
}

impl DesignExtrudePrologue {
    /// Boolean result operation.
    pub(crate) fn operation(self) -> DesignExtrudeOperation {
        match self {
            Self::LegacyDistance { operation, .. }
            | Self::ReferenceAware { operation, .. }
            | Self::ShiftedReferenceAware { operation, .. }
            | Self::LegacyShifted { operation, .. } => operation,
        }
    }

    /// Decoded extent form.
    pub(crate) fn extent(self) -> Option<DesignExtrudeExtent> {
        match self {
            Self::LegacyDistance { .. } => Some(DesignExtrudeExtent::OneSidedDistance),
            Self::ReferenceAware { extent, .. } => Some(extent),
            Self::ShiftedReferenceAware { extent, .. } => Some(extent),
            Self::LegacyShifted { extent, .. } => extent,
        }
    }

    /// Per-side extent values selected by the prologue.
    pub(crate) fn side_extent_discriminators(self) -> Option<[u32; 2]> {
        match self {
            Self::LegacyDistance { .. } => None,
            Self::ReferenceAware {
                side_extent_discriminators,
                ..
            }
            | Self::ShiftedReferenceAware {
                side_extent_discriminators,
                ..
            }
            | Self::LegacyShifted {
                side_extent_discriminators,
                ..
            } => Some(side_extent_discriminators),
        }
    }

    /// Direction-reversal state.
    pub(crate) fn direction_reversed(self) -> bool {
        match self {
            Self::LegacyDistance {
                direction_reversed, ..
            }
            | Self::ReferenceAware {
                direction_reversed, ..
            }
            | Self::ShiftedReferenceAware {
                direction_reversed, ..
            }
            | Self::LegacyShifted {
                direction_reversed, ..
            } => direction_reversed,
        }
    }

    /// Whether the operation creates solid rather than sheet geometry.
    pub(crate) fn solid_operation(self) -> bool {
        match self {
            Self::LegacyDistance {
                solid_operation, ..
            }
            | Self::ReferenceAware {
                solid_operation, ..
            }
            | Self::ShiftedReferenceAware {
                solid_operation, ..
            }
            | Self::LegacyShifted {
                solid_operation, ..
            } => solid_operation,
        }
    }

    /// Starting support.
    pub(crate) fn start(self) -> DesignExtrudeStart {
        match self {
            Self::LegacyDistance { .. } => DesignExtrudeStart::ProfilePlane,
            Self::ReferenceAware { start, .. }
            | Self::ShiftedReferenceAware { start, .. }
            | Self::LegacyShifted { start, .. } => start,
        }
    }
}

mod identity_rewrite;
