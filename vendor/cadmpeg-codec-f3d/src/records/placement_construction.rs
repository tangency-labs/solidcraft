// SPDX-License-Identifier: Apache-2.0
//! Versioned placement operands and their complete source encodings.

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

use crate::bytes::take_reference;
use crate::records::identity::Located;
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;

mod fields;

cadmpeg_core::named_optional_field!(
    deserialize_periodic_face_mode,
    Located<u8>,
    "periodic_face_mode"
);

pub(crate) fn supported_version(version: u32) -> bool {
    matches!(version, 4 | 5 | 6 | 8 | 9 | 12 | 14 | 15 | 16)
}

const ALIGN_UP_SQUARED_NORM_TOLERANCE: f64 = 1e-10;

pub(crate) fn valid_align_up_direction(value: &[FiniteReal; 3]) -> bool {
    value.iter().all(|value| value.get() == 0.0)
        || (value
            .iter()
            .map(|value| value.get() * value.get())
            .sum::<f64>()
            - 1.0)
            .abs()
            <= ALIGN_UP_SQUARED_NORM_TOLERANCE
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PlacementReferenceWire", into = "PlacementReferenceWire")]
pub(crate) struct PlacementReference {
    pub(crate) offset: u64,
    encoding: NativeBytes<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacementReferenceWire {
    offset: u64,
    encoding: NativeBytes<Vec<u8>>,
}

impl PlacementReference {
    pub(crate) fn null() -> Self {
        Self {
            offset: 0,
            encoding: vec![0].into(),
        }
    }

    pub(crate) fn new(offset: u64, encoding: Vec<u8>) -> Result<Self, String> {
        let mut cursor = 0;
        if take_reference(&encoding, &mut cursor).is_none() || cursor != encoding.len() {
            return Err("placement reference requires one complete Design reference".into());
        }
        Ok(Self {
            offset,
            encoding: encoding.into(),
        })
    }

    #[cfg(test)]
    pub(crate) fn local_target(&self) -> Option<u64> {
        take_reference(&self.encoding, &mut 0)?
            .local()
            .map(|value| value.0)
    }

    fn end(&self) -> Option<u64> {
        self.offset.checked_add(u64_from_index(self.encoding.len()))
    }
}

impl TryFrom<PlacementReferenceWire> for PlacementReference {
    type Error = String;
    fn try_from(value: PlacementReferenceWire) -> Result<Self, String> {
        Self::new(value.offset, value.encoding.into_inner())
    }
}

impl From<PlacementReference> for PlacementReferenceWire {
    fn from(value: PlacementReference) -> Self {
        Self {
            offset: value.offset,
            encoding: value.encoding,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlacementReferenceList {
    pub(crate) count: Located<u32>,
    pub(crate) members: Vec<PlacementReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlacementFields<Ref = PlacementReference, List = PlacementReferenceList> {
    pub(crate) linear_offset_on_periodic_face: Option<Located<u8>>,
    pub(crate) align_up_direction: Option<Located<[FiniteReal; 3]>>,
    pub(crate) arc_length_for_periodic_face: Option<Located<FiniteReal>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_periodic_face_mode"
    )]
    pub(crate) periodic_face_mode: Option<Located<u8>>,
    pub(crate) axis_flipped: Option<Located<u8>>,
    pub(crate) use_u_direction: Option<Located<u8>>,
    pub(crate) definer_creator: Option<Ref>,
    pub(crate) extra_offset: Option<Ref>,
    pub(crate) global_parameters: Option<[Ref; 4]>,
    pub(crate) inferred_x_axis: Located<u8>,
    pub(crate) inferred_z_axis: Located<u8>,
    pub(crate) z_flip: Located<u8>,
    pub(crate) identity: Option<Located<bool>>,
    pub(crate) matrix: Option<Located<SketchPlacementMatrix>>,
    pub(crate) old_rotation: Option<Located<FiniteReal>>,
    pub(crate) pick_point_parameter: Option<Located<FiniteReal>>,
    pub(crate) pick_point: Option<Located<[FiniteReal; 2]>>,
    pub(crate) edge_references: Option<List>,
    pub(crate) origin: Ref,
    pub(crate) plane_references: List,
    pub(crate) x_axis: Ref,
    pub(crate) z_axis: Ref,
    pub(crate) ref_type: Located<u32>,
    pub(crate) resize_type: Option<Located<u8>>,
    pub(crate) values: List,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "PlacementConstructionWire",
    into = "PlacementConstructionWire"
)]
pub(crate) struct DesignPlacementConstruction {
    pub(crate) id: String,
    pub(crate) record_index: u64,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) type_version: u32,
    pub(crate) byte_offset: u64,
    pub(crate) member_offset: u64,
    pub(crate) paired_byte_offset: u64,
    pub(crate) fields: PlacementFields,
    pub(crate) base_level: Located<NativeBytes<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlacementConstructionWire {
    pub(crate) id: String,
    pub(crate) record_index: u64,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) type_version: u32,
    pub(crate) byte_offset: u64,
    pub(crate) member_offset: u64,
    pub(crate) paired_byte_offset: u64,
    pub(crate) fields: PlacementFields,
    pub(crate) base_level: Located<NativeBytes<Vec<u8>>>,
}

impl TryFrom<PlacementConstructionWire> for DesignPlacementConstruction {
    type Error = String;
    fn try_from(value: PlacementConstructionWire) -> Result<Self, String> {
        use crate::layout::placement_indexed_prefix as long;
        let prefix = value
            .member_offset
            .checked_sub(value.byte_offset)
            .ok_or("placement members precede their indexed header")?;
        if value.record_index == 0
            || u32::try_from(value.record_index).is_err()
            || !supported_version(value.type_version)
            || prefix != u64_from_index(long::LEN)
        {
            return Err("placement registration or primary prefix is unsupported".into());
        }
        let end = fields::validate(&value.fields, value.type_version, value.member_offset)?;
        if value.base_level.offset != end
            || end.checked_add(u64_from_index(value.base_level.value.len()))
                != Some(value.paired_byte_offset)
        {
            return Err("placement base level disagrees with its paired frame".into());
        }
        Ok(Self {
            id: value.id,
            record_index: value.record_index,
            class_tag: value.class_tag,
            type_version: value.type_version,
            byte_offset: value.byte_offset,
            member_offset: value.member_offset,
            paired_byte_offset: value.paired_byte_offset,
            fields: value.fields,
            base_level: value.base_level,
        })
    }
}

impl From<DesignPlacementConstruction> for PlacementConstructionWire {
    fn from(value: DesignPlacementConstruction) -> Self {
        Self {
            id: value.id,
            record_index: value.record_index,
            class_tag: value.class_tag,
            type_version: value.type_version,
            byte_offset: value.byte_offset,
            member_offset: value.member_offset,
            paired_byte_offset: value.paired_byte_offset,
            fields: value.fields,
            base_level: value.base_level,
        }
    }
}

mod identity_rewrite;
#[cfg(test)]
mod tests;
