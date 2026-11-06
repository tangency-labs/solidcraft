// SPDX-License-Identifier: Apache-2.0
//! Component insert, derived instance, occurrence and copy-paste component features.

use crate::records::identity::{Located, IDENTITY_MATRIX};
use crate::records::mesh::{DesignOccurrenceKey, DesignRelaxedGuidText};
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

pub(crate) const OCCURRENCE_PLACEMENT_TYPE_GUID: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";

cadmpeg_core::named_optional_field!(
    deserialize_carrier_transform_offset,
    u64,
    "carrier_transform_offset"
);
cadmpeg_core::named_optional_field!(deserialize_occurrence_identity, u64, "occurrence_identity");
cadmpeg_core::named_optional_field!(deserialize_transform, SketchPlacementMatrix, "transform");
cadmpeg_core::named_optional_field!(deserialize_transform_offset, u64, "transform_offset");
/// External occurrence and placement joined through a `Component Insert` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignComponentInsertConstructionWire")]
pub(crate) struct DesignComponentInsertConstruction {
    /// Scope-owned relation record.
    pub(crate) relation_record_index: u32,
    /// Grouped occurrence carrier named by the relation record.
    pub(crate) carrier_record_index: u32,
    /// Eight-byte occurrence identity carried by the scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) occurrence_identity: Option<u64>,
    /// Occurrence role joining the carrier to the external-reference table.
    /// A role can be a GUID, a GUID with a URN suffix, or a version URN.
    pub(crate) neutron_role: String,
    /// Byte offset of the occurrence-role string payload.
    pub(crate) neutron_role_offset: u64,
    /// Explicit scope-local placement and its optional repeated carrier location.
    /// Absence is the encoded identity form.
    pub(crate) placement: Option<DesignComponentInsertMatrix>,
}

#[cfg(test)]
thread_local! {
    static COMPONENT_INSERT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignComponentInsertConstruction {
    fn clone(&self) -> Self {
        COMPONENT_INSERT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            relation_record_index: self.relation_record_index,
            carrier_record_index: self.carrier_record_index,
            occurrence_identity: self.occurrence_identity,
            neutron_role: self.neutron_role.clone(),
            neutron_role_offset: self.neutron_role_offset,
            placement: self.placement.clone(),
        }
    }
}

/// Scope-local matrix with an optional equal matrix in the grouped carrier.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignComponentInsertMatrix {
    pub(crate) scope: Located<SketchPlacementMatrix>,
    pub(crate) carrier_offset: Option<u64>,
}

impl DesignComponentInsertConstruction {
    #[must_use]
    pub(crate) fn transform(&self) -> &SketchPlacementMatrix {
        self.placement
            .as_ref()
            .map_or(&SketchPlacementMatrix::IDENTITY, |matrix| {
                &matrix.scope.value
            })
    }

    #[must_use]
    pub(crate) fn transform_offset(&self) -> Option<u64> {
        self.placement.as_ref().map(|matrix| matrix.scope.offset)
    }

    #[must_use]
    pub(crate) fn carrier_transform_offset(&self) -> Option<u64> {
        self.placement
            .as_ref()
            .and_then(|matrix| matrix.carrier_offset)
    }
}

/// External occurrence and placement joined through a `Component Insert` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignComponentInsertConstructionWire {
    /// Scope-owned relation record.
    relation_record_index: u32,
    /// Grouped occurrence carrier named by the relation record.
    carrier_record_index: u32,
    /// Eight-byte occurrence identity carried by the scope prologue.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_occurrence_identity"
    )]
    occurrence_identity: Option<u64>,
    /// Occurrence-role GUID joining the carrier to the external-reference table.
    /// The role also accepts a GUID prefix followed by an underscore and URN, beyond relaxed GUID text.
    neutron_role: String,
    /// Byte offset of the occurrence-role string payload.
    neutron_role_offset: u64,
    /// Row-major local occurrence transform in centimetres.
    transform: SketchPlacementMatrix,
    /// Byte offset of the first scope-local transform scalar. `None` is the
    /// stored identity form, which has no scalar block.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
    /// Byte offset of the equal transform's first scalar in the grouped
    /// carrier; absent when the carrier stores no scalar block.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_carrier_transform_offset"
    )]
    carrier_transform_offset: Option<u64>,
}

impl Serialize for DesignComponentInsertConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            relation_record_index: u32,
            carrier_record_index: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            occurrence_identity: Option<u64>,
            neutron_role: &'a str,
            neutron_role_offset: u64,
            transform: SketchPlacementMatrix,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform_offset: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            carrier_transform_offset: Option<u64>,
        }
        WireRef {
            relation_record_index: self.relation_record_index,
            carrier_record_index: self.carrier_record_index,
            occurrence_identity: self.occurrence_identity,
            neutron_role: &self.neutron_role,
            neutron_role_offset: self.neutron_role_offset,
            transform: *self.transform(),
            transform_offset: self.transform_offset(),
            carrier_transform_offset: self.carrier_transform_offset(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignComponentInsertConstructionWire> for DesignComponentInsertConstruction {
    type Error = String;
    fn try_from(wire: DesignComponentInsertConstructionWire) -> Result<Self, Self::Error> {
        let placement = match (wire.transform_offset, wire.carrier_transform_offset) {
            (Some(offset), carrier_offset) => Some(DesignComponentInsertMatrix {
                scope: Located {
                    value: wire.transform,
                    offset,
                },
                carrier_offset,
            }),
            (None, None) => {
                if wire
                    .transform
                    .iter()
                    .flatten()
                    .zip(IDENTITY_MATRIX.iter().flatten())
                    .any(|(value, identity)| value.to_bits() != identity.to_bits())
                {
                    return Err("transform must be identity when transform_offset is absent".into());
                }
                None
            }
            (None, Some(_)) => {
                return Err("carrier_transform_offset requires transform_offset".into())
            }
        };
        Ok(Self {
            relation_record_index: wire.relation_record_index,
            carrier_record_index: wire.carrier_record_index,
            occurrence_identity: wire.occurrence_identity,
            neutron_role: wire.neutron_role,
            neutron_role_offset: wire.neutron_role_offset,
            placement,
        })
    }
}

#[cfg(test)]
impl From<DesignComponentInsertConstruction> for DesignComponentInsertConstructionWire {
    fn from(record: DesignComponentInsertConstruction) -> Self {
        let transform = *record.transform();
        let transform_offset = record.transform_offset();
        let carrier_transform_offset = record.carrier_transform_offset();
        Self {
            relation_record_index: record.relation_record_index,
            carrier_record_index: record.carrier_record_index,
            occurrence_identity: record.occurrence_identity,
            neutron_role: record.neutron_role,
            neutron_role_offset: record.neutron_role_offset,
            transform,
            transform_offset,
            carrier_transform_offset,
        }
    }
}

/// Local component occurrence joined through a `DerivedInstance` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignDerivedInstanceConstruction {
    /// Scope prologue record referenced by the fixed field at scope offset 22.
    pub(crate) reference_record_index: u32,
    /// Scope-owned class-310 relation record.
    pub(crate) relation_record_index: u32,
    /// Class-380 component-occurrence carrier named by the relation.
    pub(crate) carrier_record_index: u32,
    /// Component definition GUID carried by the joined occurrence.
    pub(crate) component_guid: DesignRelaxedGuidText,
    /// Placed occurrence GUID carried by the joined occurrence.
    pub(crate) occurrence_guid: DesignOccurrenceKey,
    /// Row-major local-to-model placement in centimetres.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first scope-local transform scalar.
    pub(crate) transform_offset: u64,
}

/// Owner-entity offset in the identity-placement frame.
const BASE_OWNER_OFFSET: u64 = 219;
/// Owner-entity offset in the matrix-placement frame.
const PLACED_OWNER_OFFSET: u64 = 347;

/// One exact local component-occurrence carrier.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignComponentOccurrenceWire")]
pub(crate) struct DesignComponentOccurrence {
    /// Stable native record identity.
    pub(crate) id: String,
    /// Indexed-record class carrying this occurrence.
    pub(crate) class_tag: DesignClassTag,
    /// Indexed carrier record.
    pub(crate) record_index: u32,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Referenced component-definition record.
    pub(crate) component_record_index: u64,
    /// Carrier GUID.
    pub(crate) component_guid: DesignRelaxedGuidText,
    /// Stable placed-occurrence identity: a GUID or a decimal key.
    pub(crate) occurrence_guid: DesignOccurrenceKey,
    /// Base occurrence or a placed occurrence with its ordinal and matrix.
    placement: DesignComponentOccurrencePlacement,
    /// Root entity of the component naming space that contains this occurrence.
    pub(crate) owner_entity: u64,
    /// The carrier frame form.
    frame: DesignComponentOccurrenceFrame,
    pub(crate) path_discriminator: Option<NonZeroU32>,
    pub(crate) owner_type_guid: Option<DesignRelaxedGuidText>,
    /// The current world pose from a component-path carrier.
    pub(crate) current_pose: Option<crate::records::xref::XrefCurrentPose>,
    /// The definition-local pose retained from a direct operation table.
    pub(crate) operation_pose: Option<crate::records::xref::XrefOperationPose>,
}

pub(crate) const OCCURRENCE_OWNER_TYPE_GUID: &str = "904F885E-D859-4755-A7EF-6EBED8732FAD";

/// The frame form of a local component-occurrence carrier.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DesignComponentOccurrenceFrame {
    /// The 229-byte identity frame or the 357-byte matrix frame.
    Fixed,
    EmptyAttributes,
    /// A fixed frame with typed definition and owner references.
    FixedTyped {
        separated: bool,
    },
    /// The attributed frame, with a counted attribute run and an occurrence
    /// key that can be a decimal key.
    Attributed(Box<DesignAttributedOccurrenceFrame>),
}

/// The members of an attributed occurrence frame that the fixed frames do not
/// have, and its field offsets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAttributedOccurrenceFrame {
    /// The counted attribute run, in record order.
    pub(crate) attributes: Vec<DesignOccurrenceAttribute>,
    /// The little-endian u16 before the occurrence ordinal.
    pub(crate) ordinal_prefix: u16,
    pub(crate) path_discriminator: u32,
    /// Marked record reference after the occurrence key.
    pub(crate) key_record_index: u64,
    pub(crate) key_segment_index: u32,
    /// Byte offset of the carrier GUID payload.
    pub(crate) component_guid_offset: u64,
    /// Byte offset of the occurrence key payload.
    pub(crate) occurrence_guid_offset: u64,
    /// Byte offset of the matrix, when the frame stores one.
    pub(crate) transform_offset: Option<u64>,
    /// Byte offset of the owner-entity value.
    pub(crate) owner_entity_offset: u64,
}

/// One named attribute of an attributed occurrence frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignOccurrenceAttribute {
    /// LP-ASCII attribute name.
    pub(crate) name: String,
    /// The attribute value.
    pub(crate) value: DesignOccurrenceAttributeValue,
}

/// The value of an occurrence attribute, by its stored type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub(crate) enum DesignOccurrenceAttributeValue {
    /// `IntrinsicMetaTypebool`: one byte, `0` or `1`.
    Bool(bool),
    /// `IntrinsicMetaTypeHString`: an LP-UTF16 string.
    Text(String),
}

#[cfg(test)]
thread_local! {
    static COMPONENT_OCCURRENCE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignComponentOccurrence {
    fn clone(&self) -> Self {
        COMPONENT_OCCURRENCE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            component_record_index: self.component_record_index,
            component_guid: self.component_guid.clone(),
            occurrence_guid: self.occurrence_guid.clone(),
            placement: self.placement,
            owner_entity: self.owner_entity,
            frame: self.frame.clone(),
            path_discriminator: self.path_discriminator,
            owner_type_guid: self.owner_type_guid.clone(),
            current_pose: self.current_pose.clone(),
            operation_pose: self.operation_pose.clone(),
        }
    }
}

/// Local occurrence payload before checked frame admission.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignComponentOccurrenceDraft {
    /// Stable native record identity.
    pub(crate) id: String,
    /// Indexed-record class carrying this occurrence.
    pub(crate) class_tag: DesignClassTag,
    /// Indexed carrier record.
    pub(crate) record_index: u32,
    /// Byte offset of the indexed header.
    pub(crate) byte_offset: u64,
    /// Referenced component-definition record.
    pub(crate) component_record_index: u64,
    /// Carrier GUID.
    pub(crate) component_guid: DesignRelaxedGuidText,
    /// Stable placed-occurrence identity: a GUID or a decimal key.
    pub(crate) occurrence_guid: DesignOccurrenceKey,
    /// Base occurrence or a placed occurrence with its ordinal and matrix.
    pub(crate) placement: DesignComponentOccurrencePlacement,
    /// Root entity of the component naming space that contains this occurrence.
    pub(crate) owner_entity: u64,
}

/// Placement envelope of a local component occurrence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DesignComponentOccurrencePlacement {
    /// Identity placement of the first occurrence; the frame stores no matrix.
    Base,
    /// A later occurrence with a stored identity marker.
    Identity { ordinal: NonZeroU32 },
    /// Explicit matrix and one-based occurrence ordinal.
    Explicit {
        ordinal: NonZeroU32,
        transform: SketchPlacementMatrix,
    },
}

impl DesignComponentOccurrence {
    /// Admit a local occurrence with representable GUID and placement offsets.
    pub(crate) fn try_new(draft: DesignComponentOccurrenceDraft) -> Result<Self, String> {
        if matches!(draft.placement, DesignComponentOccurrencePlacement::Identity { ordinal } if ordinal.get() == 1)
        {
            return Err("a later identity occurrence must have an ordinal greater than one".into());
        }
        let last_offset = match draft.placement {
            DesignComponentOccurrencePlacement::Base
            | DesignComponentOccurrencePlacement::Identity { .. } => BASE_OWNER_OFFSET,
            DesignComponentOccurrencePlacement::Explicit { .. } => PLACED_OWNER_OFFSET,
        };
        draft
            .byte_offset
            .checked_add(last_offset)
            .ok_or("component occurrence offsets overflow byte_offset")?;
        Ok(Self {
            id: draft.id,
            class_tag: draft.class_tag,
            record_index: draft.record_index,
            byte_offset: draft.byte_offset,
            component_record_index: draft.component_record_index,
            component_guid: draft.component_guid,
            occurrence_guid: draft.occurrence_guid,
            placement: draft.placement,
            owner_entity: draft.owner_entity,
            frame: DesignComponentOccurrenceFrame::Fixed,
            path_discriminator: None,
            owner_type_guid: None,
            current_pose: None,
            operation_pose: None,
        })
    }

    pub(crate) fn has_unresolved_identity_ordinal(&self) -> bool {
        matches!(
            self.frame,
            DesignComponentOccurrenceFrame::Fixed
                | DesignComponentOccurrenceFrame::EmptyAttributes
                | DesignComponentOccurrenceFrame::FixedTyped { .. }
        ) && matches!(
            self.placement,
            DesignComponentOccurrencePlacement::Identity { .. }
        )
    }

    pub(crate) fn with_empty_attributes(mut self) -> Result<Self, String> {
        self.owner_entity_offset()
            .checked_add(Self::empty_attributes_shift())
            .ok_or("component occurrence offsets overflow byte_offset")?;
        self.frame = DesignComponentOccurrenceFrame::EmptyAttributes;
        Ok(self)
    }

    fn empty_attributes_shift() -> u64 {
        cadmpeg_core::decode::u64_from_index(
            crate::layout::local_component_occurrence_empty_attributes_prefix::LEN
                - crate::layout::local_component_occurrence_prefix::LEN,
        )
    }

    pub(crate) fn try_new_typed(
        draft: DesignComponentOccurrenceDraft,
        separated: bool,
    ) -> Result<Self, String> {
        let last_offset = Self::typed_owner_offset(&draft.placement, separated);
        draft
            .byte_offset
            .checked_add(last_offset)
            .ok_or("component occurrence offsets overflow byte_offset")?;
        let mut record = Self::try_new(draft)?;
        record.frame = DesignComponentOccurrenceFrame::FixedTyped { separated };
        record.owner_type_guid = Some(OCCURRENCE_OWNER_TYPE_GUID.to_owned().try_into()?);
        Ok(record)
    }

    fn typed_owner_offset(placement: &DesignComponentOccurrencePlacement, separated: bool) -> u64 {
        use crate::layout::{
            local_component_occurrence_typed_compact_prefix as compact,
            local_component_occurrence_typed_identity_tail as identity,
            local_component_occurrence_typed_matrix_tail as matrix,
            local_component_occurrence_typed_prefix as typed,
        };
        cadmpeg_core::decode::u64_from_index(match placement {
            DesignComponentOccurrencePlacement::Explicit { .. } => {
                (if separated { typed::LEN } else { compact::LEN }) + matrix::OWNER_REFERENCE + 1
            }
            _ if separated => typed::LEN + identity::OWNER_REFERENCE + 1,
            _ => compact::LEN + identity::OWNER_REFERENCE + 1,
        })
    }

    /// Admit a local occurrence read from an attributed frame. Its field
    /// offsets strictly increase after the indexed header, and a placed
    /// occurrence stores its matrix offset.
    pub(crate) fn try_new_attributed(
        draft: DesignComponentOccurrenceDraft,
        frame: DesignAttributedOccurrenceFrame,
    ) -> Result<Self, String> {
        let placed = matches!(
            draft.placement,
            DesignComponentOccurrencePlacement::Explicit { .. }
        );
        if placed != frame.transform_offset.is_some() {
            return Err("transform_offset must be present exactly for a placed occurrence".into());
        }
        let offsets = [
            Some(draft.byte_offset),
            Some(frame.component_guid_offset),
            Some(frame.occurrence_guid_offset),
            frame.transform_offset,
            Some(frame.owner_entity_offset),
        ];
        if !offsets
            .iter()
            .flatten()
            .collect::<Vec<_>>()
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err("attributed occurrence offsets must strictly increase".into());
        }
        let mut record = Self::try_new(draft)?;
        record.frame = DesignComponentOccurrenceFrame::Attributed(Box::new(frame));
        Ok(record)
    }

    /// Attributed-frame members on the wire, for an attributed frame.
    fn attributed_wire(&self) -> Option<DesignAttributedOccurrenceWire> {
        match &self.frame {
            DesignComponentOccurrenceFrame::Fixed
            | DesignComponentOccurrenceFrame::EmptyAttributes
            | DesignComponentOccurrenceFrame::FixedTyped { .. } => None,
            DesignComponentOccurrenceFrame::Attributed(frame) => {
                Some(DesignAttributedOccurrenceWire {
                    attributes: frame.attributes.clone(),
                    ordinal_prefix: frame.ordinal_prefix,
                    path_discriminator: frame.path_discriminator,
                    key_record_index: frame.key_record_index,
                    key_segment_index: frame.key_segment_index,
                })
            }
        }
    }

    /// Indexed header byte offset.
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    /// Component GUID byte offset.
    pub(crate) fn component_guid_offset(&self) -> u64 {
        match &self.frame {
            DesignComponentOccurrenceFrame::Fixed => self.byte_offset + 48,
            DesignComponentOccurrenceFrame::EmptyAttributes => self.byte_offset
                + cadmpeg_core::decode::u64_from_index(
                    crate::layout::local_component_occurrence_empty_attributes_prefix::CARRIER_GUID
                        + 4,
                ),
            DesignComponentOccurrenceFrame::FixedTyped { .. } => {
                self.byte_offset
                    + cadmpeg_core::decode::u64_from_index(
                        crate::layout::local_component_occurrence_typed_prefix::CARRIER_GUID + 4,
                    )
            }
            DesignComponentOccurrenceFrame::Attributed(frame) => frame.component_guid_offset,
        }
    }

    /// Occurrence GUID byte offset.
    fn occurrence_guid_offset(&self) -> u64 {
        match &self.frame {
            DesignComponentOccurrenceFrame::Fixed => self.byte_offset + 124,
            DesignComponentOccurrenceFrame::EmptyAttributes => {
                self.byte_offset + cadmpeg_core::decode::u64_from_index(crate::layout::local_component_occurrence_empty_attributes_prefix::OCCURRENCE_GUID + 4)
            }
            DesignComponentOccurrenceFrame::FixedTyped { .. } => {
                self.byte_offset
                    + cadmpeg_core::decode::u64_from_index(
                        crate::layout::local_component_occurrence_typed_prefix::OCCURRENCE_GUID + 4,
                    )
            }
            DesignComponentOccurrenceFrame::Attributed(frame) => frame.occurrence_guid_offset,
        }
    }

    /// Owner-entity byte offset.
    fn owner_entity_offset(&self) -> u64 {
        match &self.frame {
            DesignComponentOccurrenceFrame::Fixed
            | DesignComponentOccurrenceFrame::EmptyAttributes => {
                self.byte_offset
                    + if matches!(self.frame, DesignComponentOccurrenceFrame::EmptyAttributes) {
                        Self::empty_attributes_shift()
                    } else {
                        0
                    }
                    + match self.placement {
                        DesignComponentOccurrencePlacement::Base
                        | DesignComponentOccurrencePlacement::Identity { .. } => BASE_OWNER_OFFSET,
                        DesignComponentOccurrencePlacement::Explicit { .. } => PLACED_OWNER_OFFSET,
                    }
            }
            DesignComponentOccurrenceFrame::FixedTyped { separated } => {
                self.byte_offset + Self::typed_owner_offset(&self.placement, *separated)
            }
            DesignComponentOccurrenceFrame::Attributed(frame) => frame.owner_entity_offset,
        }
    }

    /// Base or explicit local placement.
    pub(crate) fn placement(&self) -> &DesignComponentOccurrencePlacement {
        &self.placement
    }

    #[must_use]
    pub(crate) fn occurrence_ordinal(&self) -> u32 {
        match self.placement {
            DesignComponentOccurrencePlacement::Base => 1,
            DesignComponentOccurrencePlacement::Identity { ordinal } => ordinal.get(),
            DesignComponentOccurrencePlacement::Explicit { ordinal, .. } => ordinal.get(),
        }
    }

    #[must_use]
    pub(crate) fn transform(&self) -> Option<Located<SketchPlacementMatrix>> {
        match self.placement {
            DesignComponentOccurrencePlacement::Base
            | DesignComponentOccurrencePlacement::Identity { .. } => None,
            DesignComponentOccurrencePlacement::Explicit { transform, .. } => Some(Located {
                value: transform,
                offset: match &self.frame {
                    DesignComponentOccurrenceFrame::Attributed(frame) => {
                        frame.transform_offset.unwrap_or(self.byte_offset + 209)
                    }
                    DesignComponentOccurrenceFrame::Fixed => self.byte_offset + 209,
                    DesignComponentOccurrenceFrame::EmptyAttributes => self.byte_offset
                        + cadmpeg_core::decode::u64_from_index(
                            crate::layout::local_component_occurrence_empty_attributes_prefix::LEN,
                        ),
                    DesignComponentOccurrenceFrame::FixedTyped { separated } => {
                        self.byte_offset
                            + cadmpeg_core::decode::u64_from_index(if *separated {
                                crate::layout::local_component_occurrence_typed_prefix::LEN
                            } else {
                                crate::layout::local_component_occurrence_typed_compact_prefix::LEN
                            })
                    }
                },
            }),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct DesignComponentOccurrenceWire {
    /// Stable native record identity.
    id: String,
    /// Indexed-record class carrying this occurrence.
    class_tag: String,
    /// Indexed carrier record.
    record_index: u32,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Referenced component-definition record.
    component_record_index: u64,
    /// Carrier GUID at frame offset 44.
    component_guid: DesignRelaxedGuidText,
    /// Byte offset of the component GUID payload.
    component_guid_offset: u64,
    /// Stable placed-occurrence identity: a GUID or a decimal key.
    occurrence_guid: DesignOccurrenceKey,
    /// Byte offset of the occurrence GUID payload.
    occurrence_guid_offset: u64,
    /// One-based occurrence ordinal within the component definition.
    occurrence_ordinal: u32,
    /// Explicit local-to-model placement for placed occurrences.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the explicit placement.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
    /// Root entity of the component naming space that contains this occurrence.
    owner_entity: u64,
    /// Byte offset of the owner-entity value.
    owner_entity_offset: u64,
    /// Members of an attributed frame; absent for the fixed frames.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_attributed_frame"
    )]
    attributed: Option<DesignAttributedOccurrenceWire>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fixed_path_discriminator"
    )]
    path_discriminator: Option<NonZeroU32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_occurrence_owner_type_guid"
    )]
    owner_type_guid: Option<DesignRelaxedGuidText>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_occurrence_current_pose"
    )]
    current_pose: Option<crate::records::xref::XrefCurrentPose>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_occurrence_operation_pose"
    )]
    operation_pose: Option<crate::records::xref::XrefOperationPose>,
}

/// Members of an attributed occurrence frame on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignAttributedOccurrenceWire {
    /// The counted attribute run, in record order.
    attributes: Vec<DesignOccurrenceAttribute>,
    /// The little-endian u16 before the occurrence ordinal.
    ordinal_prefix: u16,
    path_discriminator: u32,
    /// Marked record reference after the occurrence key.
    key_record_index: u64,
    key_segment_index: u32,
}

cadmpeg_core::named_optional_field!(
    deserialize_attributed_frame,
    DesignAttributedOccurrenceWire,
    "attributed"
);

cadmpeg_core::named_optional_field!(
    deserialize_fixed_path_discriminator,
    NonZeroU32,
    "path_discriminator"
);
cadmpeg_core::named_optional_field!(
    deserialize_occurrence_owner_type_guid,
    DesignRelaxedGuidText,
    "owner_type_guid"
);
cadmpeg_core::named_optional_field!(
    deserialize_occurrence_current_pose,
    crate::records::xref::XrefCurrentPose,
    "current_pose"
);

cadmpeg_core::named_optional_field!(
    deserialize_occurrence_operation_pose,
    crate::records::xref::XrefOperationPose,
    "operation_pose"
);

impl Serialize for DesignComponentOccurrence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            class_tag: &'a str,
            record_index: u32,
            byte_offset: u64,
            component_record_index: u64,
            component_guid: &'a DesignRelaxedGuidText,
            component_guid_offset: u64,
            occurrence_guid: &'a DesignOccurrenceKey,
            occurrence_guid_offset: u64,
            occurrence_ordinal: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform: Option<SketchPlacementMatrix>,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform_offset: Option<u64>,
            owner_entity: u64,
            owner_entity_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            attributed: Option<DesignAttributedOccurrenceWire>,
            #[serde(skip_serializing_if = "Option::is_none")]
            path_discriminator: Option<NonZeroU32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            owner_type_guid: Option<&'a DesignRelaxedGuidText>,
            #[serde(skip_serializing_if = "Option::is_none")]
            current_pose: Option<&'a crate::records::xref::XrefCurrentPose>,
            #[serde(skip_serializing_if = "Option::is_none")]
            operation_pose: Option<&'a crate::records::xref::XrefOperationPose>,
        }
        let transform = self.transform();
        WireRef {
            id: &self.id,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            component_record_index: self.component_record_index,
            component_guid: &self.component_guid,
            component_guid_offset: self.component_guid_offset(),
            occurrence_guid: &self.occurrence_guid,
            occurrence_guid_offset: self.occurrence_guid_offset(),
            occurrence_ordinal: self.occurrence_ordinal(),
            transform: transform.map(|frame| frame.value),
            transform_offset: transform.map(|frame| frame.offset),
            owner_entity: self.owner_entity,
            owner_entity_offset: self.owner_entity_offset(),
            attributed: self.attributed_wire(),
            path_discriminator: self.path_discriminator,
            owner_type_guid: self.owner_type_guid.as_ref(),
            current_pose: self.current_pose.as_ref(),
            operation_pose: self.operation_pose.as_ref(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<DesignComponentOccurrence> for DesignComponentOccurrenceWire {
    fn from(value: DesignComponentOccurrence) -> Self {
        let occurrence_ordinal = value.occurrence_ordinal();
        let attributed = value.attributed_wire();
        let component_guid_offset = value.component_guid_offset();
        let occurrence_guid_offset = value.occurrence_guid_offset();
        let owner_entity_offset = value.owner_entity_offset();
        let transform = value.transform();
        Self {
            id: value.id,
            class_tag: value.class_tag.into(),
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            component_record_index: value.component_record_index,
            component_guid: value.component_guid,
            component_guid_offset,
            occurrence_guid: value.occurrence_guid,
            occurrence_guid_offset,
            occurrence_ordinal,
            transform: transform.map(|frame| frame.value),
            transform_offset: transform.map(|frame| frame.offset),
            owner_entity: value.owner_entity,
            owner_entity_offset,
            attributed,
            path_discriminator: value.path_discriminator,
            owner_type_guid: value.owner_type_guid,
            current_pose: value.current_pose,
            operation_pose: value.operation_pose,
        }
    }
}

impl TryFrom<DesignComponentOccurrenceWire> for DesignComponentOccurrence {
    type Error = String;
    fn try_from(value: DesignComponentOccurrenceWire) -> Result<Self, Self::Error> {
        if value.owner_type_guid.as_ref().is_some_and(|guid| {
            !guid
                .as_str()
                .eq_ignore_ascii_case(OCCURRENCE_OWNER_TYPE_GUID)
        }) {
            return Err("owner_type_guid disagrees with the occurrence owner type".into());
        }
        let transform = Located::from_wire(value.transform, value.transform_offset, "transform")?;
        let placement = match (value.occurrence_ordinal, transform) {
            (1, None) => DesignComponentOccurrencePlacement::Base,
            (ordinal, None) => DesignComponentOccurrencePlacement::Identity {
                ordinal: NonZeroU32::new(ordinal).ok_or("occurrence_ordinal must be nonzero")?,
            },
            (ordinal, Some(transform)) => DesignComponentOccurrencePlacement::Explicit {
                ordinal: NonZeroU32::new(ordinal).ok_or("occurrence_ordinal must be nonzero")?,
                transform: transform.value,
            },
        };
        let draft = DesignComponentOccurrenceDraft {
            id: value.id,
            class_tag: value.class_tag.try_into()?,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            component_record_index: value.component_record_index,
            component_guid: value.component_guid,
            occurrence_guid: value.occurrence_guid,
            placement,
            owner_entity: value.owner_entity,
        };
        if let Some(attributed) = value.attributed {
            if value.path_discriminator.is_some() {
                return Err("an attributed frame stores its discriminator in attributed".into());
            }
            let mut record = Self::try_new_attributed(
                draft,
                DesignAttributedOccurrenceFrame {
                    attributes: attributed.attributes,
                    ordinal_prefix: attributed.ordinal_prefix,
                    path_discriminator: attributed.path_discriminator,
                    key_record_index: attributed.key_record_index,
                    key_segment_index: attributed.key_segment_index,
                    component_guid_offset: value.component_guid_offset,
                    occurrence_guid_offset: value.occurrence_guid_offset,
                    transform_offset: value.transform_offset,
                    owner_entity_offset: value.owner_entity_offset,
                },
            )?;
            record.owner_type_guid = value.owner_type_guid;
            record.current_pose = value.current_pose;
            record.operation_pose = value.operation_pose;
            return Ok(record);
        }
        let mut record = if value.component_guid_offset.checked_sub(value.byte_offset)
            == Some(cadmpeg_core::decode::u64_from_index(
                crate::layout::local_component_occurrence_typed_prefix::CARRIER_GUID + 4,
            )) {
            let separated = value.owner_entity_offset.checked_sub(value.byte_offset)
                == Some(Self::typed_owner_offset(&draft.placement, true));
            Self::try_new_typed(draft, separated)?
        } else if value.component_guid_offset.checked_sub(value.byte_offset)
            == Some(cadmpeg_core::decode::u64_from_index(
                crate::layout::local_component_occurrence_empty_attributes_prefix::CARRIER_GUID + 4,
            ))
        {
            Self::try_new(draft)?.with_empty_attributes()?
        } else {
            Self::try_new(draft)?
        };
        if record.has_unresolved_identity_ordinal() {
            return Err("occurrence_ordinal must be 1 when transform is absent".into());
        }
        if value.component_guid_offset != record.component_guid_offset() {
            return Err("component_guid_offset disagrees with byte_offset".into());
        }
        if value.occurrence_guid_offset != record.occurrence_guid_offset() {
            return Err("occurrence_guid_offset disagrees with byte_offset".into());
        }
        if value.transform_offset != record.transform().map(|transform| transform.offset) {
            return Err("transform_offset disagrees with byte_offset".into());
        }
        if value.owner_entity_offset != record.owner_entity_offset() {
            return Err("owner_entity_offset disagrees with byte_offset".into());
        }
        record.path_discriminator = value.path_discriminator;
        if value.owner_type_guid.is_some() {
            record.owner_type_guid = value.owner_type_guid;
        }
        record.current_pose = value.current_pose;
        record.operation_pose = value.operation_pose;
        Ok(record)
    }
}

/// Legacy component copy/paste construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignCopyPasteComponentOperation {
    /// Scope-owned relation record.
    pub(crate) relation_record_index: u32,
    /// Existing source occurrence carrier.
    pub(crate) source_occurrence_record_index: u32,
    /// Newly copied occurrence carrier.
    pub(crate) copied_occurrence_record_index: u32,
    /// Reusable component definition shared by source and copy.
    pub(crate) component_guid: DesignRelaxedGuidText,
    /// Existing source occurrence identity.
    pub(crate) source_occurrence_guid: DesignOccurrenceKey,
    /// Newly copied occurrence identity.
    pub(crate) copied_occurrence_guid: DesignOccurrenceKey,
    /// Source placement embedded by the scope.
    pub(crate) source_transform: SketchPlacementMatrix,
    /// Byte offset of the source placement.
    pub(crate) source_transform_offset: u64,
    /// Copied placement embedded by both scope and occurrence carrier.
    pub(crate) copied_transform: SketchPlacementMatrix,
    /// Byte offset of the scope-local copied placement.
    pub(crate) copied_transform_offset: u64,
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
