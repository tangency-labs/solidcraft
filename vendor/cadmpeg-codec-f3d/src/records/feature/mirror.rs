// SPDX-License-Identifier: Apache-2.0
//! Mirror constructions and the scope tolerance they state.

use super::patterns::DesignPlane;
use crate::records::identity::Located;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::scalar::PositiveReal;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_plane_normal, FiniteVector3, "plane_normal");
cadmpeg_core::named_optional_field!(deserialize_plane_origin, FinitePoint3, "plane_origin");
cadmpeg_core::named_optional_field!(
    deserialize_plane_reference_offset,
    u64,
    "plane_reference_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_plane_scope_record_index,
    u32,
    "plane_scope_record_index"
);
cadmpeg_core::named_optional_field!(
    deserialize_plane_selection_record_index,
    u32,
    "plane_selection_record_index"
);
cadmpeg_core::named_optional_field!(
    deserialize_repeated_marker_offset,
    u64,
    "repeated_marker_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_seed_feature_reference_offset,
    u64,
    "seed_feature_reference_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_seed_feature_scope_record_index,
    u32,
    "seed_feature_scope_record_index"
);
cadmpeg_core::named_optional_field!(
    deserialize_stitch_tolerance_record_index,
    u32,
    "stitch_tolerance_record_index"
);
cadmpeg_core::named_optional_field!(
    deserialize_stitch_tolerance_scope,
    DesignMirrorScopeTolerance,
    "stitch_tolerance_scope"
);
/// Exact construction carried by a Mirror scope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignMirrorConstructionWire",
    into = "DesignMirrorConstructionWire"
)]
pub(crate) struct DesignMirrorConstruction {
    /// Parameter-owner record carrying the fixed count two.
    pub(crate) count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    pub(crate) count_offset: u64,
    /// Model-space stitch tolerance in source centimetres.
    pub(crate) stitch_tolerance: PositiveReal,
    /// Byte offset of the evaluated stitch-tolerance scalar.
    pub(crate) stitch_tolerance_offset: u64,
    /// Owner-backed or inline scope-frame tolerance carrier.
    pub(crate) tolerance_source: DesignMirrorToleranceSource,
    /// Seed group selected by the source operation.
    pub(crate) seed_group_record_index: u32,
    /// Role-`0x5` mirror-plane group.
    pub(crate) plane_group_record_index: u32,
    /// Referenced seed feature scope when the seed is a complete feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) seed_feature_scope_record_index: Option<Located<u32>>,
    /// Referenced `WorkPlane` scope, when the plane operand names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plane_scope_record_index: Option<Located<u32>>,
    /// Persistent entity-selection record used as the mirror plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plane_selection_record_index: Option<u32>,
    /// Proven selected-face mirror plane, when exact.
    pub(crate) plane: Option<DesignPlane>,
}

/// Native carrier of a Mirror stitch tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DesignMirrorToleranceSource {
    Owner { record_index: u32 },
    Scope(DesignMirrorScopeTolerance),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignMirrorConstructionWire {
    /// Fixed instance count, including the seed.
    count: u32,
    /// Parameter-owner record carrying `count`.
    count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    count_offset: u64,
    /// Positive model-space stitch tolerance in source centimetres.
    stitch_tolerance: f64,
    /// Parameter-owner record carrying `stitch_tolerance`, when the source
    /// stores the scalar in a separate owner record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stitch_tolerance_record_index"
    )]
    stitch_tolerance_record_index: Option<u32>,
    /// Byte offset of the evaluated stitch-tolerance scalar.
    stitch_tolerance_offset: u64,
    /// Inline scope-frame carrier used by the legacy Mirror envelope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stitch_tolerance_scope"
    )]
    stitch_tolerance_scope: Option<DesignMirrorScopeTolerance>,
    /// Seed group selected by the source operation.
    seed_group_record_index: u32,
    /// Role-`0x5` mirror-plane group.
    plane_group_record_index: u32,
    /// Referenced seed feature scope when the seed is a complete feature.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_seed_feature_scope_record_index"
    )]
    seed_feature_scope_record_index: Option<u32>,
    /// Byte offset of the optional seed-feature reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_seed_feature_reference_offset"
    )]
    seed_feature_reference_offset: Option<u64>,
    /// Referenced `WorkPlane` scope, when the plane operand names one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plane_scope_record_index"
    )]
    plane_scope_record_index: Option<u32>,
    /// Byte offset of the optional `WorkPlane` reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plane_reference_offset"
    )]
    plane_reference_offset: Option<u64>,
    /// Persistent entity-selection record used as the mirror plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plane_selection_record_index"
    )]
    plane_selection_record_index: Option<u32>,
    /// Proven selected-face mirror plane, when exact.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plane_origin"
    )]
    plane_origin: Option<FinitePoint3>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plane_normal"
    )]
    plane_normal: Option<FiniteVector3>,
}

impl TryFrom<DesignMirrorConstructionWire> for DesignMirrorConstruction {
    type Error = String;
    fn try_from(wire: DesignMirrorConstructionWire) -> Result<Self, Self::Error> {
        if wire.count != 2 {
            return Err("count must be 2 for Mirror".into());
        }
        let tolerance_source = match (wire.stitch_tolerance_record_index, wire.stitch_tolerance_scope) {
            (Some(record_index), None) => DesignMirrorToleranceSource::Owner { record_index },
            (None, Some(scope)) => DesignMirrorToleranceSource::Scope(scope),
            _ => return Err("stitch_tolerance_record_index and stitch_tolerance_scope require exactly one carrier".into()),
        };
        Ok(Self {
            count_record_index: wire.count_record_index,
            count_offset: wire.count_offset,
            stitch_tolerance: PositiveReal::new(wire.stitch_tolerance)
                .ok_or("stitch_tolerance must be positive and finite")?,
            stitch_tolerance_offset: wire.stitch_tolerance_offset,
            tolerance_source,
            seed_group_record_index: wire.seed_group_record_index,
            plane_group_record_index: wire.plane_group_record_index,
            seed_feature_scope_record_index: Located::from_wire(wire.seed_feature_scope_record_index, wire.seed_feature_reference_offset, "seed_feature_scope_record_index").map_err(|_| "seed_feature_scope_record_index and seed_feature_reference_offset must occur together")?,
            plane_scope_record_index: Located::from_wire(wire.plane_scope_record_index, wire.plane_reference_offset, "plane_scope_record_index").map_err(|_| "plane_scope_record_index and plane_reference_offset must occur together")?,
            plane_selection_record_index: wire.plane_selection_record_index,
            plane: match (wire.plane_origin, wire.plane_normal) {
                (None, None) => None,
                (Some(origin), Some(normal)) => Some(DesignPlane::from_parts(origin, normal).ok_or("plane_normal must be a unit vector")?),
                _ => return Err("plane_origin and plane_normal must occur together".into()),
            },
        })
    }
}

impl From<DesignMirrorConstruction> for DesignMirrorConstructionWire {
    fn from(record: DesignMirrorConstruction) -> Self {
        let (stitch_tolerance_record_index, stitch_tolerance_scope) = match record.tolerance_source
        {
            DesignMirrorToleranceSource::Owner { record_index } => (Some(record_index), None),
            DesignMirrorToleranceSource::Scope(scope) => (None, Some(scope)),
        };
        Self {
            count: 2,
            count_record_index: record.count_record_index,
            count_offset: record.count_offset,
            stitch_tolerance: record.stitch_tolerance.get(),
            stitch_tolerance_record_index,
            stitch_tolerance_offset: record.stitch_tolerance_offset,
            stitch_tolerance_scope,
            seed_group_record_index: record.seed_group_record_index,
            plane_group_record_index: record.plane_group_record_index,
            seed_feature_scope_record_index: record
                .seed_feature_scope_record_index
                .map(|reference| reference.value),
            seed_feature_reference_offset: record
                .seed_feature_scope_record_index
                .map(|reference| reference.offset),
            plane_scope_record_index: record
                .plane_scope_record_index
                .map(|reference| reference.value),
            plane_reference_offset: record
                .plane_scope_record_index
                .map(|reference| reference.offset),
            plane_selection_record_index: record.plane_selection_record_index,
            plane_origin: record.plane.map(DesignPlane::origin),
            plane_normal: record
                .plane
                .map(|plane| FiniteVector3::from(plane.normal())),
        }
    }
}

/// Exact inline carrier for a legacy Mirror stitch tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignMirrorScopeToleranceWire",
    into = "DesignMirrorScopeToleranceWire"
)]
pub(crate) struct DesignMirrorScopeTolerance {
    /// Fixed scalar-lane marker preceding the tolerance value.
    pub(crate) marker: DesignMirrorToleranceMarker,
    /// Byte offset of the first scalar-lane marker.
    pub(crate) marker_offset: u64,
    /// First marked reference in the scalar lane.
    pub(crate) first_reference: u32,
    /// Byte offset of the first marked reference.
    pub(crate) first_reference_offset: u64,
    /// Second marked reference in the scalar lane.
    pub(crate) second_reference: u32,
    /// Byte offset of the second marked reference.
    pub(crate) second_reference_offset: u64,
}

/// Scalar-lane marker and its required repeated location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignMirrorToleranceMarker {
    Single61,
    Repeated89(u64),
    Repeated94(u64),
    Repeated100(u64),
}

impl DesignMirrorToleranceMarker {
    #[must_use]
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::Single61 => 61,
            Self::Repeated89(_) => 89,
            Self::Repeated94(_) => 94,
            Self::Repeated100(_) => 100,
        }
    }

    #[must_use]
    pub(crate) fn repeated_offset(self) -> Option<u64> {
        match self {
            Self::Single61 => None,
            Self::Repeated89(offset) | Self::Repeated94(offset) | Self::Repeated100(offset) => {
                Some(offset)
            }
        }
    }
}

impl TryFrom<(u32, Option<u64>)> for DesignMirrorToleranceMarker {
    type Error = String;
    fn try_from((marker, offset): (u32, Option<u64>)) -> Result<Self, Self::Error> {
        match (marker, offset) {
            (61, None) => Ok(Self::Single61),
            (89, Some(offset)) => Ok(Self::Repeated89(offset)),
            (94, Some(offset)) => Ok(Self::Repeated94(offset)),
            (100, Some(offset)) => Ok(Self::Repeated100(offset)),
            _ => Err("marker and repeated_marker_offset must identify a single 61 or repeated 89, 94, or 100 lane".into()),
        }
    }
}

/// Exact inline carrier for a legacy Mirror stitch tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignMirrorScopeToleranceWire {
    /// Fixed scalar-lane marker preceding the tolerance value.
    marker: u32,
    /// Byte offset of the first scalar-lane marker.
    marker_offset: u64,
    /// Byte offset of the repeated scalar-lane marker, when this generation
    /// carries the marker twice.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_repeated_marker_offset"
    )]
    repeated_marker_offset: Option<u64>,
    /// First marked reference in the scalar lane.
    first_reference: u32,
    /// Byte offset of the first marked reference.
    first_reference_offset: u64,
    /// Second marked reference in the scalar lane.
    second_reference: u32,
    /// Byte offset of the second marked reference.
    second_reference_offset: u64,
}

impl TryFrom<DesignMirrorScopeToleranceWire> for DesignMirrorScopeTolerance {
    type Error = String;
    fn try_from(wire: DesignMirrorScopeToleranceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            marker: DesignMirrorToleranceMarker::try_from((
                wire.marker,
                wire.repeated_marker_offset,
            ))?,
            marker_offset: wire.marker_offset,
            first_reference: wire.first_reference,
            first_reference_offset: wire.first_reference_offset,
            second_reference: wire.second_reference,
            second_reference_offset: wire.second_reference_offset,
        })
    }
}

impl From<DesignMirrorScopeTolerance> for DesignMirrorScopeToleranceWire {
    fn from(record: DesignMirrorScopeTolerance) -> Self {
        Self {
            marker: record.marker.code(),
            marker_offset: record.marker_offset,
            repeated_marker_offset: record.marker.repeated_offset(),
            first_reference: record.first_reference,
            first_reference_offset: record.first_reference_offset,
            second_reference: record.second_reference,
            second_reference_offset: record.second_reference_offset,
        }
    }
}

mod identity_rewrite;
