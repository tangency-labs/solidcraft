// SPDX-License-Identifier: Apache-2.0
//! Base features, the body references they carry and the results they state.

use cadmpeg_ir::native::bytes::NativeBytes;

use crate::layout::base_feature_local_reference_field as local_field;
use crate::records::serde_column::SliceColumn;
use crate::records::{identity::Located, mesh::DesignRelaxedGuidText};
use serde::{Deserialize, Serialize};

pub(crate) fn local_reference_field(bytes: &[u8]) -> Option<[u8; 6]> {
    let field: [u8; 6] = bytes.try_into().ok()?;
    (field[local_field::CROSS_DOCUMENT] == local_field::CROSS_DOCUMENT_VALUE
        && field[local_field::DIFFERENT_SEGMENT] == local_field::DIFFERENT_SEGMENT_VALUE)
        .then_some(field)
}

fn require_local_fields(fields: &[NativeBytes<[u8; 6]>]) -> Result<(), String> {
    if fields
        .iter()
        .all(|field| local_reference_field(&**field).is_some())
    {
        Ok(())
    } else {
        Err("15-byte Base Feature elements require ordinary same-segment references".into())
    }
}

/// Encoded compact Base Feature mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum DesignBaseFeatureCompactMode {
    Zero = 0,
    One = 1,
}

impl From<DesignBaseFeatureCompactMode> for u8 {
    fn from(value: DesignBaseFeatureCompactMode) -> Self {
        match value {
            DesignBaseFeatureCompactMode::Zero => 0,
            DesignBaseFeatureCompactMode::One => 1,
        }
    }
}

impl TryFrom<u8> for DesignBaseFeatureCompactMode {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            _ => Err("mode must be 0 or 1"),
        }
    }
}

/// Layout of the legacy class-452/class-262 Base Feature envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignBaseFeatureBodyReferenceForm {
    /// One output body with an encoded compact mode.
    CompactOneBody {
        mode: Located<DesignBaseFeatureCompactMode>,
        body: DesignLegacyBaseFeatureBody,
    },
    /// Two output bodies with no mode slot.
    ExpandedTwoBody {
        bodies: [DesignLegacyBaseFeatureBody; 2],
    },
}

/// One body in a legacy Base Feature envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignLegacyBaseFeatureBody {
    pub(crate) entity: DesignBaseFeatureEntry<u32>,
    pub(crate) parameter_body: Located<u64>,
    pub(crate) auxiliary: Located<u64>,
}

impl DesignBaseFeatureBodyReferenceForm {
    fn bodies(&self) -> &[DesignLegacyBaseFeatureBody] {
        match self {
            Self::CompactOneBody { body, .. } => std::slice::from_ref(body),
            Self::ExpandedTwoBody { bodies } => bodies,
        }
    }
}

/// Typed construction data carried by a Fusion direct-modeling Base Feature.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignBaseFeatureConstructionWire")]
pub(crate) enum DesignBaseFeatureConstruction {
    /// Counted body, passive-reference, metadata, and result runs.
    ResultBodies {
        /// Ordered body, passive-reference, result, and optional repeated-field rows.
        bodies: DesignBaseFeatureResults,
        /// Shared passive-reference metadata record.
        metadata_record: u32,
        /// Byte offset of the shared metadata record.
        metadata_record_offset: u64,
        /// Variant-width source field following the metadata record.
        metadata_field: NativeBytes<Vec<u8>>,
    },
    /// Direct-modeling body-reference envelope used by the class-365/class-262 and
    /// class-377/class-259 forms.
    BodyBasedOnFaces {
        /// The single body suffix and the location shared by its reference views.
        body: Located<u32>,
        /// PM body-reference record named by the fixed envelope lane.
        parameter_body_record: u32,
        /// Byte offset of `parameter_body_record`.
        parameter_body_record_offset: u64,
        /// Auxiliary record named by the fixed envelope lane.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Legacy body-reference envelope used by the class-452/class-262 forms.
    LegacyBodyBasedOnFaces {
        /// Compact one-body or expanded two-body source envelope form.
        form: DesignBaseFeatureBodyReferenceForm,
        /// Scope record repeated by the envelope's explicit scope-reference lane.
        scope_reference: u64,
        /// Byte offset of `scope_reference`.
        scope_reference_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Body snapshot form used by the class-314/class-259 scope pair.
    BodySnapshot {
        /// Ordered snapshot bodies with their source fields.
        bodies: Vec<DesignBaseFeatureEntry<u64>>,
        /// Three LP-UTF-16 source GUIDs carried by the snapshot envelope.
        related_guids: [DesignRelaxedGuidText; 3],
        /// Byte offsets of the first code unit of each related GUID.
        related_guid_offsets: [u64; 3],
        /// Indexed record carried by the snapshot linkage tail.
        linkage_record: u32,
        /// Byte offset of `linkage_record`.
        linkage_record_offset: u64,
        /// Auxiliary indexed record carried by the snapshot linkage tail.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
    },
    /// Result bodies of a scope that a timeline object owns.
    TimelineOwnedBodies {
        /// Parameter-body records in source order.
        parameter_bodies: Vec<DesignBaseFeatureEntry<u64>>,
        /// Design body entity suffixes in source order.
        bodies: Vec<DesignBaseFeatureEntry<u64>>,
        /// Timeline object that names this scope.
        owner_record: u32,
        /// Byte offset of the object's reference to this scope.
        owner_reference_offset: u64,
    },
}

#[cfg(test)]
thread_local! {
    static BASE_FEATURE_CONSTRUCTION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignBaseFeatureConstruction {
    fn clone(&self) -> Self {
        #[cfg(test)]
        BASE_FEATURE_CONSTRUCTION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        match self {
            Self::ResultBodies {
                bodies,
                metadata_record,
                metadata_record_offset,
                metadata_field,
            } => Self::ResultBodies {
                bodies: bodies.clone(),
                metadata_record: *metadata_record,
                metadata_record_offset: *metadata_record_offset,
                metadata_field: metadata_field.clone(),
            },
            Self::BodyBasedOnFaces {
                body,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => Self::BodyBasedOnFaces {
                body: *body,
                parameter_body_record: *parameter_body_record,
                parameter_body_record_offset: *parameter_body_record_offset,
                auxiliary_record: *auxiliary_record,
                auxiliary_record_offset: *auxiliary_record_offset,
                envelope_guid: envelope_guid.clone(),
                envelope_guid_offset: *envelope_guid_offset,
                tag_body_based_on_faces_offset: *tag_body_based_on_faces_offset,
            },
            Self::LegacyBodyBasedOnFaces {
                form,
                scope_reference,
                scope_reference_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => Self::LegacyBodyBasedOnFaces {
                form: *form,
                scope_reference: *scope_reference,
                scope_reference_offset: *scope_reference_offset,
                envelope_guid: envelope_guid.clone(),
                envelope_guid_offset: *envelope_guid_offset,
                tag_body_based_on_faces_offset: *tag_body_based_on_faces_offset,
            },
            Self::BodySnapshot {
                bodies,
                related_guids,
                related_guid_offsets,
                linkage_record,
                linkage_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
            } => Self::BodySnapshot {
                bodies: bodies.clone(),
                related_guids: related_guids.clone(),
                related_guid_offsets: *related_guid_offsets,
                linkage_record: *linkage_record,
                linkage_record_offset: *linkage_record_offset,
                auxiliary_record: *auxiliary_record,
                auxiliary_record_offset: *auxiliary_record_offset,
            },
            Self::TimelineOwnedBodies {
                parameter_bodies,
                bodies,
                owner_record,
                owner_reference_offset,
            } => Self::TimelineOwnedBodies {
                parameter_bodies: parameter_bodies.clone(),
                bodies: bodies.clone(),
                owner_record: *owner_record,
                owner_reference_offset: *owner_reference_offset,
            },
        }
    }
}

struct ResultBodyColumn<'a, T> {
    bodies: &'a DesignBaseFeatureResults,
    value: fn(&DesignBaseFeatureResultBody) -> T,
}

impl<T: Serialize> Serialize for ResultBodyColumn<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.bodies.iter().map(self.value))
    }
}

struct RepeatedReferenceFields<'a>(&'a DesignBaseFeatureResults);

impl Serialize for RepeatedReferenceFields<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            DesignBaseFeatureResults::WithoutRepeatedFields(_) => {
                serializer.collect_seq(std::iter::empty::<[u8; 6]>())
            }
            DesignBaseFeatureResults::WithRepeatedFields { first, rest } => serializer.collect_seq(
                std::iter::once(first.1)
                    .chain(rest.iter().map(|row| row.1))
                    .map(NativeBytes::from),
            ),
        }
    }
}

impl Serialize for DesignBaseFeatureConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(untagged)]
        enum BorrowedWire<'a> {
            ResultBodies {
                body_entity_suffixes: ResultBodyColumn<'a, u64>,
                body_entity_suffix_offsets: ResultBodyColumn<'a, u64>,
                body_entity_fields: ResultBodyColumn<'a, NativeBytes<[u8; 6]>>,
                body_reference_records: ResultBodyColumn<'a, u32>,
                body_reference_record_offsets: ResultBodyColumn<'a, u64>,
                body_reference_fields: ResultBodyColumn<'a, NativeBytes<[u8; 6]>>,
                repeated_reference_fields: RepeatedReferenceFields<'a>,
                metadata_record: u32,
                metadata_record_offset: u64,
                metadata_field: NativeBytes<&'a [u8]>,
                result_records: ResultBodyColumn<'a, u32>,
                result_record_offsets: ResultBodyColumn<'a, u64>,
                result_fields: ResultBodyColumn<'a, NativeBytes<[u8; 6]>>,
            },
            BodyBasedOnFaces {
                body_entity_suffixes: [u64; 1],
                body_entity_suffix_offsets: [u64; 1],
                body_reference_records: [u32; 1],
                body_reference_record_offsets: [u64; 1],
                parameter_body_record: u32,
                parameter_body_record_offset: u64,
                auxiliary_record: u32,
                auxiliary_record_offset: u64,
                envelope_guid: &'a DesignRelaxedGuidText,
                envelope_guid_offset: u64,
                tag_body_based_on_faces: bool,
                tag_body_based_on_faces_offset: u64,
            },
            LegacyBodyBasedOnFaces {
                form: DesignBaseFeatureBodyReferenceFormWire,
                #[serde(skip_serializing_if = "Option::is_none")]
                mode: Option<u8>,
                #[serde(skip_serializing_if = "Option::is_none")]
                mode_offset: Option<u64>,
                body_entity_suffixes: SliceColumn<'a, DesignLegacyBaseFeatureBody, u64>,
                body_entity_suffix_offsets: SliceColumn<'a, DesignLegacyBaseFeatureBody, u64>,
                body_entity_fields:
                    SliceColumn<'a, DesignLegacyBaseFeatureBody, NativeBytes<[u8; 6]>>,
                body_reference_records: SliceColumn<'a, DesignLegacyBaseFeatureBody, u32>,
                body_reference_record_offsets: SliceColumn<'a, DesignLegacyBaseFeatureBody, u64>,
                parameter_body_records: SliceColumn<'a, DesignLegacyBaseFeatureBody, u64>,
                parameter_body_record_offsets: SliceColumn<'a, DesignLegacyBaseFeatureBody, u64>,
                auxiliary_records: SliceColumn<'a, DesignLegacyBaseFeatureBody, u64>,
                auxiliary_record_offsets: SliceColumn<'a, DesignLegacyBaseFeatureBody, u64>,
                scope_reference: u64,
                scope_reference_offset: u64,
                envelope_guid: &'a DesignRelaxedGuidText,
                envelope_guid_offset: u64,
                tag_body_based_on_faces: bool,
                tag_body_based_on_faces_offset: u64,
            },
            BodySnapshot {
                body_entity_suffixes: SliceColumn<'a, DesignBaseFeatureEntry<u64>, u64>,
                body_entity_suffix_offsets: SliceColumn<'a, DesignBaseFeatureEntry<u64>, u64>,
                body_entity_fields:
                    SliceColumn<'a, DesignBaseFeatureEntry<u64>, NativeBytes<[u8; 6]>>,
                related_guids: &'a [DesignRelaxedGuidText; 3],
                related_guid_offsets: [u64; 3],
                linkage_record: u32,
                linkage_record_offset: u64,
                auxiliary_record: u32,
                auxiliary_record_offset: u64,
            },
            TimelineOwnedBodies {
                parameter_body_records: SliceColumn<'a, DesignBaseFeatureEntry<u64>, u64>,
                parameter_body_record_offsets: SliceColumn<'a, DesignBaseFeatureEntry<u64>, u64>,
                parameter_body_fields:
                    SliceColumn<'a, DesignBaseFeatureEntry<u64>, NativeBytes<[u8; 6]>>,
                body_entity_suffixes: SliceColumn<'a, DesignBaseFeatureEntry<u64>, u64>,
                body_entity_suffix_offsets: SliceColumn<'a, DesignBaseFeatureEntry<u64>, u64>,
                body_entity_fields:
                    SliceColumn<'a, DesignBaseFeatureEntry<u64>, NativeBytes<[u8; 6]>>,
                owner_record: u32,
                owner_reference_offset: u64,
            },
        }
        let wire = match self {
            Self::ResultBodies {
                bodies,
                metadata_record,
                metadata_record_offset,
                metadata_field,
            } => BorrowedWire::ResultBodies {
                body_entity_suffixes: ResultBodyColumn {
                    bodies,
                    value: |row| row.entity.value,
                },
                body_entity_suffix_offsets: ResultBodyColumn {
                    bodies,
                    value: |row| row.entity.offset,
                },
                body_entity_fields: ResultBodyColumn {
                    bodies,
                    value: |row| row.entity.field.into(),
                },
                body_reference_records: ResultBodyColumn {
                    bodies,
                    value: |row| row.reference.value,
                },
                body_reference_record_offsets: ResultBodyColumn {
                    bodies,
                    value: |row| row.reference.offset,
                },
                body_reference_fields: ResultBodyColumn {
                    bodies,
                    value: |row| row.reference.field.into(),
                },
                repeated_reference_fields: RepeatedReferenceFields(bodies),
                metadata_record: *metadata_record,
                metadata_record_offset: *metadata_record_offset,
                metadata_field: metadata_field.into(),
                result_records: ResultBodyColumn {
                    bodies,
                    value: |row| row.result.value,
                },
                result_record_offsets: ResultBodyColumn {
                    bodies,
                    value: |row| row.result.offset,
                },
                result_fields: ResultBodyColumn {
                    bodies,
                    value: |row| row.result.field.into(),
                },
            },
            Self::BodyBasedOnFaces {
                body,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => BorrowedWire::BodyBasedOnFaces {
                body_entity_suffixes: [u64::from(body.value)],
                body_entity_suffix_offsets: [body.offset],
                body_reference_records: [body.value],
                body_reference_record_offsets: [body.offset],
                parameter_body_record: *parameter_body_record,
                parameter_body_record_offset: *parameter_body_record_offset,
                auxiliary_record: *auxiliary_record,
                auxiliary_record_offset: *auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset: *envelope_guid_offset,
                tag_body_based_on_faces: true,
                tag_body_based_on_faces_offset: *tag_body_based_on_faces_offset,
            },
            Self::LegacyBodyBasedOnFaces {
                form,
                scope_reference,
                scope_reference_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => {
                let bodies = form.bodies();
                let (wire_form, mode, mode_offset) = match form {
                    DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode, .. } => (
                        DesignBaseFeatureBodyReferenceFormWire::CompactOneBody,
                        Some(u8::from(mode.value)),
                        Some(mode.offset),
                    ),
                    DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { .. } => (
                        DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody,
                        None,
                        None,
                    ),
                };
                BorrowedWire::LegacyBodyBasedOnFaces {
                    form: wire_form,
                    mode,
                    mode_offset,
                    body_entity_suffixes: SliceColumn::new(bodies, |row| {
                        u64::from(row.entity.value)
                    }),
                    body_entity_suffix_offsets: SliceColumn::new(bodies, |row| row.entity.offset),
                    body_entity_fields: SliceColumn::new(bodies, |row| row.entity.field.into()),
                    body_reference_records: SliceColumn::new(bodies, |row| row.entity.value),
                    body_reference_record_offsets: SliceColumn::new(bodies, |row| {
                        row.entity.offset
                    }),
                    parameter_body_records: SliceColumn::new(bodies, |row| {
                        row.parameter_body.value
                    }),
                    parameter_body_record_offsets: SliceColumn::new(bodies, |row| {
                        row.parameter_body.offset
                    }),
                    auxiliary_records: SliceColumn::new(bodies, |row| row.auxiliary.value),
                    auxiliary_record_offsets: SliceColumn::new(bodies, |row| row.auxiliary.offset),
                    scope_reference: *scope_reference,
                    scope_reference_offset: *scope_reference_offset,
                    envelope_guid,
                    envelope_guid_offset: *envelope_guid_offset,
                    tag_body_based_on_faces: true,
                    tag_body_based_on_faces_offset: *tag_body_based_on_faces_offset,
                }
            }
            Self::BodySnapshot {
                bodies,
                related_guids,
                related_guid_offsets,
                linkage_record,
                linkage_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
            } => BorrowedWire::BodySnapshot {
                body_entity_suffixes: SliceColumn::new(bodies, |row| row.value),
                body_entity_suffix_offsets: SliceColumn::new(bodies, |row| row.offset),
                body_entity_fields: SliceColumn::new(bodies, |row| row.field.into()),
                related_guids,
                related_guid_offsets: *related_guid_offsets,
                linkage_record: *linkage_record,
                linkage_record_offset: *linkage_record_offset,
                auxiliary_record: *auxiliary_record,
                auxiliary_record_offset: *auxiliary_record_offset,
            },
            Self::TimelineOwnedBodies {
                parameter_bodies,
                bodies,
                owner_record,
                owner_reference_offset,
            } => BorrowedWire::TimelineOwnedBodies {
                parameter_body_records: SliceColumn::new(parameter_bodies, |row| row.value),
                parameter_body_record_offsets: SliceColumn::new(parameter_bodies, |row| row.offset),
                parameter_body_fields: SliceColumn::new(parameter_bodies, |row| row.field.into()),
                body_entity_suffixes: SliceColumn::new(bodies, |row| row.value),
                body_entity_suffix_offsets: SliceColumn::new(bodies, |row| row.offset),
                body_entity_fields: SliceColumn::new(bodies, |row| row.field.into()),
                owner_record: *owner_record,
                owner_reference_offset: *owner_reference_offset,
            },
        };
        wire.serialize(serializer)
    }
}

/// One aligned body, passive reference, and result record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignBaseFeatureResultBody {
    pub(crate) entity: DesignBaseFeatureEntry<u64>,
    pub(crate) reference: DesignBaseFeatureEntry<u32>,
    pub(crate) result: DesignBaseFeatureEntry<u32>,
}

/// Result-body runs with either no repeated fields or one field per body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DesignBaseFeatureResults {
    WithoutRepeatedFields(Vec<DesignBaseFeatureResultBody>),
    WithRepeatedFields {
        first: (DesignBaseFeatureResultBody, [u8; 6]),
        rest: Vec<(DesignBaseFeatureResultBody, [u8; 6])>,
    },
}

impl DesignBaseFeatureResults {
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &DesignBaseFeatureResultBody> {
        let count = match self {
            Self::WithoutRepeatedFields(bodies) => bodies.len(),
            Self::WithRepeatedFields { rest, .. } => 1 + rest.len(),
        };
        (0..count).map(move |index| match self {
            Self::WithoutRepeatedFields(bodies) => &bodies[index],
            Self::WithRepeatedFields { first, .. } if index == 0 => &first.0,
            Self::WithRepeatedFields { rest, .. } => &rest[index - 1].0,
        })
    }
}

/// One Base Feature reference value, its location, and its six-byte field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignBaseFeatureEntry<T> {
    pub(crate) value: T,
    pub(crate) offset: u64,
    pub(crate) field: [u8; 6],
}

/// Wire form of the legacy class-452/class-262 Base Feature body-reference
/// envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesignBaseFeatureBodyReferenceFormWire {
    /// One output body with 64-bit references in the legacy compact lanes.
    CompactOneBody,
    /// Two output bodies with counted 32-bit reference runs.
    ExpandedTwoBody,
}

/// Typed construction data carried by a Fusion direct-modeling Base Feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Untagged: required field sets are disjoint across variants.
#[serde(untagged)]
enum DesignBaseFeatureConstructionWire {
    /// Counted body, passive-reference, metadata, and result runs.
    ResultBodies {
        /// Ordered Design body entity suffixes exposed by the Base Feature.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<NativeBytes<[u8; 6]>>,
        /// Ordered passive body-reference records parallel to the body suffixes.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_reference_records`.
        body_reference_fields: Vec<NativeBytes<[u8; 6]>>,
        /// Six-byte source fields in the repeated passive-reference run.
        repeated_reference_fields: Vec<NativeBytes<[u8; 6]>>,
        /// Shared passive-reference metadata record.
        metadata_record: u32,
        /// Byte offset of `metadata_record`.
        metadata_record_offset: u64,
        /// Variant-width source field following `metadata_record`.
        metadata_field: NativeBytes<Vec<u8>>,
        /// Ordered result-body join records parallel to the body suffixes.
        result_records: Vec<u32>,
        /// Byte offsets parallel to `result_records`.
        result_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `result_records`.
        result_fields: Vec<NativeBytes<[u8; 6]>>,
    },
    /// Direct-modeling body-reference envelope used by the class-365/class-262 and
    /// class-377/class-259 forms.
    BodyBasedOnFaces {
        /// The Design body entity suffix exposed by the envelope.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Body suffixes used by history-to-BREP resolution for this form.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// PM body-reference record named by the fixed envelope lane.
        parameter_body_record: u32,
        /// Byte offset of `parameter_body_record`.
        parameter_body_record_offset: u64,
        /// Auxiliary record named by the fixed envelope lane.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Stored body-source property value.
        tag_body_based_on_faces: bool,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Legacy body-reference envelope used by the class-452/class-262 forms.
    LegacyBodyBasedOnFaces {
        /// Compact one-body or expanded two-body source envelope form.
        form: DesignBaseFeatureBodyReferenceFormWire,
        /// Compact-form mode byte. The expanded form has no mode byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u8>,
        /// Byte offset of the compact-form mode byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode_offset: Option<u64>,
        /// Ordered Design body entity suffixes exposed by the envelope.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<NativeBytes<[u8; 6]>>,
        /// Body suffixes used by history-to-BREP resolution for this form.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// Ordered PM body-reference records carried by the envelope.
        parameter_body_records: Vec<u64>,
        /// Byte offsets parallel to `parameter_body_records`.
        parameter_body_record_offsets: Vec<u64>,
        /// Ordered DM body-reference records carried by the envelope.
        auxiliary_records: Vec<u64>,
        /// Byte offsets parallel to `auxiliary_records`.
        auxiliary_record_offsets: Vec<u64>,
        /// Scope record repeated by the envelope's explicit scope-reference lane.
        scope_reference: u64,
        /// Byte offset of `scope_reference`.
        scope_reference_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Stored body-source property value.
        tag_body_based_on_faces: bool,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Body snapshot form used by the class-314/class-259 scope pair.
    BodySnapshot {
        /// Ordered Design body entity suffixes exposed by the snapshot.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<NativeBytes<[u8; 6]>>,
        /// Three LP-UTF-16 source GUIDs carried by the snapshot envelope.
        related_guids: [DesignRelaxedGuidText; 3],
        /// Byte offsets of the first code unit of each related GUID.
        related_guid_offsets: [u64; 3],
        /// Indexed record carried by the snapshot linkage tail.
        linkage_record: u32,
        /// Byte offset of `linkage_record`.
        linkage_record_offset: u64,
        /// Auxiliary indexed record carried by the snapshot linkage tail.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
    },
    /// Result bodies of a scope that a timeline object owns.
    TimelineOwnedBodies {
        /// Parameter-body records in source order.
        parameter_body_records: Vec<u64>,
        /// Byte offsets parallel to `parameter_body_records`.
        parameter_body_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `parameter_body_records`.
        parameter_body_fields: Vec<NativeBytes<[u8; 6]>>,
        /// Design body entity suffixes in source order.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<NativeBytes<[u8; 6]>>,
        /// Timeline object that names this scope.
        owner_record: u32,
        /// Byte offset of the object's reference to this scope.
        owner_reference_offset: u64,
    },
}

/// Join one column-wise run of 15-byte Base Feature references.
fn entry_rows(
    values: Vec<u64>,
    offsets: Vec<u64>,
    fields: Vec<NativeBytes<[u8; 6]>>,
) -> Result<Vec<DesignBaseFeatureEntry<u64>>, String> {
    if values.len() != offsets.len() || values.len() != fields.len() {
        return Err("Base Feature reference columns must be parallel".into());
    }
    require_local_fields(&fields)?;
    Ok(values
        .into_iter()
        .zip(offsets)
        .zip(fields)
        .map(|((value, offset), field)| DesignBaseFeatureEntry {
            value,
            offset,
            field: *field,
        })
        .collect())
}

impl TryFrom<DesignBaseFeatureConstructionWire> for DesignBaseFeatureConstruction {
    type Error = String;
    // Output cardinalities are bounded by already-materialized input vectors.
    fn try_from(wire: DesignBaseFeatureConstructionWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignBaseFeatureConstructionWire::ResultBodies {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                body_reference_records,
                body_reference_record_offsets,
                body_reference_fields,
                repeated_reference_fields,
                metadata_record,
                metadata_record_offset,
                metadata_field,
                result_records,
                result_record_offsets,
                result_fields,
            } => {
                let count = body_entity_suffixes.len();
                for (field, len) in [
                    (
                        "body_entity_suffix_offsets",
                        body_entity_suffix_offsets.len(),
                    ),
                    ("body_entity_fields", body_entity_fields.len()),
                    ("body_reference_records", body_reference_records.len()),
                    (
                        "body_reference_record_offsets",
                        body_reference_record_offsets.len(),
                    ),
                    ("body_reference_fields", body_reference_fields.len()),
                    ("result_records", result_records.len()),
                    ("result_record_offsets", result_record_offsets.len()),
                    ("result_fields", result_fields.len()),
                ] {
                    if len != count {
                        return Err(format!(
                            "{field} must have the same length as body_entity_suffixes"
                        ));
                    }
                }
                if !repeated_reference_fields.is_empty() && repeated_reference_fields.len() != count
                {
                    return Err("repeated_reference_fields must be empty or have the same length as body_entity_suffixes".into());
                }
                require_local_fields(&body_entity_fields)?;
                require_local_fields(&body_reference_fields)?;
                let bodies = (0..count).map(|index| DesignBaseFeatureResultBody {
                    entity: DesignBaseFeatureEntry {
                        value: body_entity_suffixes[index],
                        offset: body_entity_suffix_offsets[index],
                        field: *body_entity_fields[index],
                    },
                    reference: DesignBaseFeatureEntry {
                        value: body_reference_records[index],
                        offset: body_reference_record_offsets[index],
                        field: *body_reference_fields[index],
                    },
                    result: DesignBaseFeatureEntry {
                        value: result_records[index],
                        offset: result_record_offsets[index],
                        field: *result_fields[index],
                    },
                });
                let bodies = if repeated_reference_fields.is_empty() {
                    DesignBaseFeatureResults::WithoutRepeatedFields(bodies.collect())
                } else {
                    let mut repeated = bodies.zip(
                        repeated_reference_fields
                            .into_iter()
                            .map(NativeBytes::into_inner),
                    );
                    match repeated.next() {
                        Some(first) => DesignBaseFeatureResults::WithRepeatedFields {
                            first,
                            rest: repeated.collect(),
                        },
                        None => {
                            return Err(
                                "repeated_reference_fields require body_entity_suffixes".into()
                            )
                        }
                    }
                };
                Self::ResultBodies {
                    bodies,
                    metadata_record,
                    metadata_record_offset,
                    metadata_field,
                }
            }
            DesignBaseFeatureConstructionWire::BodyBasedOnFaces {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_reference_records,
                body_reference_record_offsets,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces,
                tag_body_based_on_faces_offset,
            } => {
                let ([suffix], [offset], [reference], [reference_offset]) = (
                    body_entity_suffixes.as_slice(),
                    body_entity_suffix_offsets.as_slice(),
                    body_reference_records.as_slice(),
                    body_reference_record_offsets.as_slice(),
                ) else {
                    return Err("body_entity_suffixes, body_entity_suffix_offsets, body_reference_records, and body_reference_record_offsets require one body".into());
                };
                if *suffix != u64::from(*reference) || offset != reference_offset {
                    return Err("body_reference_records and body_reference_record_offsets must match body_entity_suffixes and body_entity_suffix_offsets".into());
                }
                if !tag_body_based_on_faces {
                    return Err("tag_body_based_on_faces must be true for BodyBasedOnFaces".into());
                }
                Self::BodyBasedOnFaces {
                    body: Located {
                        value: *reference,
                        offset: *offset,
                    },
                    parameter_body_record,
                    parameter_body_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstructionWire::LegacyBodyBasedOnFaces {
                form,
                mode,
                mode_offset,
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                body_reference_records,
                body_reference_record_offsets,
                parameter_body_records,
                parameter_body_record_offsets,
                auxiliary_records,
                auxiliary_record_offsets,
                scope_reference,
                scope_reference_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces,
                tag_body_based_on_faces_offset,
            } => {
                let count = body_entity_suffixes.len();
                for (field, len) in [
                    (
                        "body_entity_suffix_offsets",
                        body_entity_suffix_offsets.len(),
                    ),
                    ("body_entity_fields", body_entity_fields.len()),
                    ("body_reference_records", body_reference_records.len()),
                    (
                        "body_reference_record_offsets",
                        body_reference_record_offsets.len(),
                    ),
                    ("parameter_body_records", parameter_body_records.len()),
                    (
                        "parameter_body_record_offsets",
                        parameter_body_record_offsets.len(),
                    ),
                    ("auxiliary_records", auxiliary_records.len()),
                    ("auxiliary_record_offsets", auxiliary_record_offsets.len()),
                ] {
                    if len != count {
                        return Err(format!(
                            "{field} must have the same length as body_entity_suffixes"
                        ));
                    }
                }
                require_local_fields(&body_entity_fields)?;
                if !tag_body_based_on_faces {
                    return Err(
                        "tag_body_based_on_faces must be true for LegacyBodyBasedOnFaces".into(),
                    );
                }
                let mut bodies = {
                    let mut storage = Vec::new();
                    storage.try_reserve_exact(count).map(|()| storage)
                }
                .map_err(|error| error.to_string())?;
                for index in 0..count {
                    if body_entity_suffixes[index] != u64::from(body_reference_records[index])
                        || body_entity_suffix_offsets[index] != body_reference_record_offsets[index]
                    {
                        return Err("body_reference_records and body_reference_record_offsets must match body_entity_suffixes and body_entity_suffix_offsets".into());
                    }
                    bodies.push(DesignLegacyBaseFeatureBody {
                        entity: DesignBaseFeatureEntry {
                            value: body_reference_records[index],
                            offset: body_entity_suffix_offsets[index],
                            field: *body_entity_fields[index],
                        },
                        parameter_body: Located {
                            value: parameter_body_records[index],
                            offset: parameter_body_record_offsets[index],
                        },
                        auxiliary: Located {
                            value: auxiliary_records[index],
                            offset: auxiliary_record_offsets[index],
                        },
                    });
                }
                let form = match (form, mode, mode_offset, bodies.as_slice()) {
                    (DesignBaseFeatureBodyReferenceFormWire::CompactOneBody, Some(value), Some(offset), [body]) => DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode: Located { value: DesignBaseFeatureCompactMode::try_from(value)?, offset }, body: *body },
                    (DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody, None, None, [first, second]) => DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { bodies: [*first, *second] },
                    _ => return Err("form requires one body with mode and mode_offset for compact_one_body, or two bodies without mode for expanded_two_body".into()),
                };
                Self::LegacyBodyBasedOnFaces {
                    form,
                    scope_reference,
                    scope_reference_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstructionWire::BodySnapshot {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                related_guids,
                related_guid_offsets,
                linkage_record,
                linkage_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
            } => {
                if body_entity_suffixes.len() != body_entity_suffix_offsets.len()
                    || body_entity_suffixes.len() != body_entity_fields.len()
                {
                    return Err("body_entity_suffixes, body_entity_suffix_offsets, and body_entity_fields must have equal lengths".into());
                }
                require_local_fields(&body_entity_fields)?;
                let bodies = body_entity_suffixes
                    .into_iter()
                    .zip(body_entity_suffix_offsets)
                    .zip(body_entity_fields)
                    .map(|((value, offset), field)| DesignBaseFeatureEntry {
                        value,
                        offset,
                        field: *field,
                    })
                    .collect();
                Self::BodySnapshot {
                    bodies,
                    related_guids,
                    related_guid_offsets,
                    linkage_record,
                    linkage_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                }
            }
            DesignBaseFeatureConstructionWire::TimelineOwnedBodies {
                parameter_body_records,
                parameter_body_record_offsets,
                parameter_body_fields,
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                owner_record,
                owner_reference_offset,
            } => {
                if parameter_body_records.len() != body_entity_suffixes.len() {
                    return Err(
                        "parameter_body_records and body_entity_suffixes must be parallel".into(),
                    );
                }
                Self::TimelineOwnedBodies {
                    parameter_bodies: entry_rows(
                        parameter_body_records,
                        parameter_body_record_offsets,
                        parameter_body_fields,
                    )?,
                    bodies: entry_rows(
                        body_entity_suffixes,
                        body_entity_suffix_offsets,
                        body_entity_fields,
                    )?,
                    owner_record,
                    owner_reference_offset,
                }
            }
        })
    }
}

#[cfg(test)]
impl From<DesignBaseFeatureConstruction> for DesignBaseFeatureConstructionWire {
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn from(value: DesignBaseFeatureConstruction) -> Self {
        match value {
            DesignBaseFeatureConstruction::ResultBodies {
                bodies,
                metadata_record,
                metadata_record_offset,
                metadata_field,
            } => {
                let body_entity_suffixes = bodies.iter().map(|body| body.entity.value).collect();
                let body_entity_suffix_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let body_entity_fields = bodies
                    .iter()
                    .map(|body| NativeBytes::from(body.entity.field))
                    .collect();
                let body_reference_records =
                    bodies.iter().map(|body| body.reference.value).collect();
                let body_reference_record_offsets =
                    bodies.iter().map(|body| body.reference.offset).collect();
                let body_reference_fields = bodies
                    .iter()
                    .map(|body| NativeBytes::from(body.reference.field))
                    .collect();
                let result_records = bodies.iter().map(|body| body.result.value).collect();
                let result_record_offsets = bodies.iter().map(|body| body.result.offset).collect();
                let result_fields = bodies
                    .iter()
                    .map(|body| NativeBytes::from(body.result.field))
                    .collect();
                let repeated_reference_fields = match bodies {
                    DesignBaseFeatureResults::WithoutRepeatedFields(_) => Vec::new(),
                    DesignBaseFeatureResults::WithRepeatedFields { first, rest } => {
                        std::iter::once(first.1)
                            .chain(rest.into_iter().map(|(_, field)| field))
                            .map(NativeBytes::from)
                            .collect()
                    }
                };
                Self::ResultBodies {
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    body_reference_records,
                    body_reference_record_offsets,
                    body_reference_fields,
                    repeated_reference_fields,
                    metadata_record,
                    metadata_record_offset,
                    metadata_field,
                    result_records,
                    result_record_offsets,
                    result_fields,
                }
            }
            DesignBaseFeatureConstruction::BodyBasedOnFaces {
                body,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => Self::BodyBasedOnFaces {
                body_entity_suffixes: vec![u64::from(body.value)],
                body_entity_suffix_offsets: vec![body.offset],
                body_reference_records: vec![body.value],
                body_reference_record_offsets: vec![body.offset],
                tag_body_based_on_faces: true,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            },
            DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
                form,
                scope_reference,
                scope_reference_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => {
                let bodies = form.bodies();
                let body_entity_suffixes = bodies
                    .iter()
                    .map(|body| u64::from(body.entity.value))
                    .collect();
                let body_entity_suffix_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let body_entity_fields = bodies
                    .iter()
                    .map(|body| NativeBytes::from(body.entity.field))
                    .collect();
                let body_reference_records = bodies.iter().map(|body| body.entity.value).collect();
                let body_reference_record_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let parameter_body_records = bodies
                    .iter()
                    .map(|body| body.parameter_body.value)
                    .collect();
                let parameter_body_record_offsets = bodies
                    .iter()
                    .map(|body| body.parameter_body.offset)
                    .collect();
                let auxiliary_records = bodies.iter().map(|body| body.auxiliary.value).collect();
                let auxiliary_record_offsets =
                    bodies.iter().map(|body| body.auxiliary.offset).collect();
                let (form, mode, mode_offset) = match form {
                    DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode, .. } => (
                        DesignBaseFeatureBodyReferenceFormWire::CompactOneBody,
                        Some(u8::from(mode.value)),
                        Some(mode.offset),
                    ),
                    DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { .. } => (
                        DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody,
                        None,
                        None,
                    ),
                };
                Self::LegacyBodyBasedOnFaces {
                    form,
                    mode,
                    mode_offset,
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    body_reference_records,
                    body_reference_record_offsets,
                    parameter_body_records,
                    parameter_body_record_offsets,
                    auxiliary_records,
                    auxiliary_record_offsets,
                    scope_reference,
                    scope_reference_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces: true,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstruction::BodySnapshot {
                bodies,
                related_guids,
                related_guid_offsets,
                linkage_record,
                linkage_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
            } => {
                let mut body_entity_suffixes = Vec::with_capacity(bodies.len());
                let mut body_entity_suffix_offsets = Vec::with_capacity(bodies.len());
                let mut body_entity_fields = Vec::with_capacity(bodies.len());
                for body in bodies {
                    body_entity_suffixes.push(body.value);
                    body_entity_suffix_offsets.push(body.offset);
                    body_entity_fields.push(body.field.into());
                }
                Self::BodySnapshot {
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    related_guids,
                    related_guid_offsets,
                    linkage_record,
                    linkage_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                }
            }
            DesignBaseFeatureConstruction::TimelineOwnedBodies {
                parameter_bodies,
                bodies,
                owner_record,
                owner_reference_offset,
            } => Self::TimelineOwnedBodies {
                parameter_body_records: parameter_bodies.iter().map(|row| row.value).collect(),
                parameter_body_record_offsets: parameter_bodies
                    .iter()
                    .map(|row| row.offset)
                    .collect(),
                parameter_body_fields: parameter_bodies
                    .iter()
                    .map(|row| row.field.into())
                    .collect(),
                body_entity_suffixes: bodies.iter().map(|row| row.value).collect(),
                body_entity_suffix_offsets: bodies.iter().map(|row| row.offset).collect(),
                body_entity_fields: bodies.iter().map(|row| row.field.into()).collect(),
                owner_record,
                owner_reference_offset,
            },
        }
    }
}

impl DesignBaseFeatureConstruction {
    /// Timeline object that gives this scope its authored position.
    pub(crate) fn timeline_owner(&self) -> Option<u32> {
        match self {
            Self::TimelineOwnedBodies { owner_record, .. } => Some(*owner_record),
            _ => None,
        }
    }

    /// Return the body suffixes in source order for any Base Feature form.
    pub(crate) fn body_entity_suffixes(&self) -> impl ExactSizeIterator<Item = u64> + '_ {
        let count = match self {
            Self::BodySnapshot { bodies, .. } | Self::TimelineOwnedBodies { bodies, .. } => {
                bodies.len()
            }
            Self::BodyBasedOnFaces { .. } => 1,
            Self::ResultBodies { bodies, .. } => bodies.iter().len(),
            Self::LegacyBodyBasedOnFaces { form, .. } => form.bodies().len(),
        };
        (0..count).map(move |index| match self {
            Self::BodySnapshot { bodies, .. } | Self::TimelineOwnedBodies { bodies, .. } => {
                bodies[index].value
            }
            Self::BodyBasedOnFaces { body, .. } => u64::from(body.value),
            Self::ResultBodies { bodies, .. } => match bodies {
                DesignBaseFeatureResults::WithoutRepeatedFields(bodies) => {
                    bodies[index].entity.value
                }
                DesignBaseFeatureResults::WithRepeatedFields { first, .. } if index == 0 => {
                    first.0.entity.value
                }
                DesignBaseFeatureResults::WithRepeatedFields { rest, .. } => {
                    rest[index - 1].0.entity.value
                }
            },
            Self::LegacyBodyBasedOnFaces { form, .. } => {
                u64::from(form.bodies()[index].entity.value)
            }
        })
    }

    /// Return passive body-reference records for forms that carry them.
    pub(crate) fn body_reference_records(&self) -> impl Iterator<Item = u32> + '_ {
        let (results, legacy, single) = match self {
            Self::ResultBodies { bodies, .. } => (Some(bodies), &[][..], None),
            Self::LegacyBodyBasedOnFaces { form, .. } => (None, form.bodies(), None),
            Self::BodyBasedOnFaces { body, .. } => (None, &[][..], Some(body.value)),
            Self::BodySnapshot { .. } | Self::TimelineOwnedBodies { .. } => (None, &[][..], None),
        };
        results
            .into_iter()
            .flat_map(DesignBaseFeatureResults::iter)
            .map(|body| body.reference.value)
            .chain(legacy.iter().map(|body| body.entity.value))
            .chain(single)
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
