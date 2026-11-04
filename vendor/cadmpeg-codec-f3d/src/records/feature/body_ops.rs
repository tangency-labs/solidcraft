// SPDX-License-Identifier: Apache-2.0
//! Whole-body operations: scale and copy-paste bodies.

use crate::records::identity::Located;
use crate::records::references::DesignClassTag;
use crate::records::serde_column::SliceColumn;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_center_position,
    [FiniteReal; 3],
    "center_position"
);
cadmpeg_core::named_optional_field!(
    deserialize_center_position_offset,
    u64,
    "center_position_offset"
);
/// Fixed construction carried by a uniform body-scale scope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignScaleOperationWire",
    into = "DesignScaleOperationWire"
)]
pub(crate) struct DesignScaleOperation {
    /// Counted construction group selecting the transformed bodies.
    pub(crate) body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    pub(crate) center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    pub(crate) center_position: Option<Located<[FiniteReal; 3]>>,
    /// Uniform scale factor.
    pub(crate) uniform_factor: PositiveReal,
    /// Byte offset of `uniform_factor`.
    pub(crate) uniform_factor_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignScaleOperationWire {
    /// Counted construction group selecting the transformed bodies.
    body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_center_position"
    )]
    center_position: Option<[FiniteReal; 3]>,
    /// Byte offset of the explicit center position.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_center_position_offset"
    )]
    center_position_offset: Option<u64>,
    /// Positive uniform scale factor.
    uniform_factor: f64,
    /// Byte offset of `uniform_factor`.
    uniform_factor_offset: u64,
}

impl From<DesignScaleOperation> for DesignScaleOperationWire {
    fn from(value: DesignScaleOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: value.center_position.map(|center| center.value),
            center_position_offset: value.center_position.map(|center| center.offset),
            uniform_factor: value.uniform_factor.get(),
            uniform_factor_offset: value.uniform_factor_offset,
        }
    }
}

impl TryFrom<DesignScaleOperationWire> for DesignScaleOperation {
    type Error = String;
    fn try_from(value: DesignScaleOperationWire) -> Result<Self, Self::Error> {
        Ok(Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: Located::from_wire(
                value.center_position,
                value.center_position_offset,
                "center_position",
            )?,
            uniform_factor: PositiveReal::new(value.uniform_factor)
                .ok_or("uniform_factor must be positive and finite")?,
            uniform_factor_offset: value.uniform_factor_offset,
        })
    }
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignCopyPasteBodiesOperationWire")]
pub(crate) struct DesignCopyPasteBodiesOperation {
    bodies: Vec<DesignCopiedBody>,
    /// Counted body-selection group named by the scope prefix and reference table.
    pub(crate) body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    pub(crate) body_group_class_tag: DesignClassTag,
    /// Byte offset of the body group's primary header.
    body_group_byte_offset: u64,
    /// Indexed source-to-copy relation record named by the scope prefix.
    pub(crate) relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    pub(crate) relation_class_tag: DesignClassTag,
    /// Byte offset of the relation record's primary header.
    relation_byte_offset: u64,
}

#[cfg(test)]
thread_local! {
    static COPY_PASTE_BODIES_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCopyPasteBodiesOperation {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COPY_PASTE_BODIES_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            bodies: self.bodies.clone(),
            body_group_record_index: self.body_group_record_index,
            body_group_class_tag: self.body_group_class_tag.clone(),
            body_group_byte_offset: self.body_group_byte_offset,
            relation_record_index: self.relation_record_index,
            relation_class_tag: self.relation_class_tag.clone(),
            relation_byte_offset: self.relation_byte_offset,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignCopiedBody {
    pub(crate) operand: Located<u32>,
    pub(crate) source: Located<u32>,
    pub(crate) copied: Located<u32>,
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCopyPasteBodiesOperationWire {
    /// Counted body-selection group named by the scope prefix and reference table.
    body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    body_group_class_tag: String,
    /// Byte offset of the body group's primary header.
    body_group_byte_offset: u64,
    /// Ordered body-operand records carried by the counted group.
    body_operand_record_indices: Vec<u32>,
    /// Byte offsets parallel to `body_operand_record_indices`.
    body_operand_record_offsets: Vec<u64>,
    /// Indexed source-to-copy relation record named by the scope prefix.
    relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    relation_class_tag: String,
    /// Byte offset of the relation record's primary header.
    relation_byte_offset: u64,
    /// Source Design body entity suffixes in copy order.
    source_body_entity_suffixes: Vec<u32>,
    /// Byte offsets parallel to `source_body_entity_suffixes`.
    source_body_entity_suffix_offsets: Vec<u64>,
    /// Newly copied Design body entity suffixes parallel to the sources.
    copied_body_entity_suffixes: Vec<u32>,
    /// Byte offsets parallel to `copied_body_entity_suffixes`.
    copied_body_entity_suffix_offsets: Vec<u64>,
}

#[derive(Serialize)]
struct DesignCopyPasteBodiesOperationWireRef<'a> {
    body_group_record_index: u32,
    body_group_class_tag: &'a str,
    body_group_byte_offset: u64,
    body_operand_record_indices: SliceColumn<'a, DesignCopiedBody, u32>,
    body_operand_record_offsets: SliceColumn<'a, DesignCopiedBody, u64>,
    relation_record_index: u32,
    relation_class_tag: &'a str,
    relation_byte_offset: u64,
    source_body_entity_suffixes: SliceColumn<'a, DesignCopiedBody, u32>,
    source_body_entity_suffix_offsets: SliceColumn<'a, DesignCopiedBody, u64>,
    copied_body_entity_suffixes: SliceColumn<'a, DesignCopiedBody, u32>,
    copied_body_entity_suffix_offsets: SliceColumn<'a, DesignCopiedBody, u64>,
}

impl Serialize for DesignCopyPasteBodiesOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let bodies = self.bodies.as_slice();
        DesignCopyPasteBodiesOperationWireRef {
            body_group_record_index: self.body_group_record_index,
            body_group_class_tag: self.body_group_class_tag.as_str(),
            body_group_byte_offset: self.body_group_byte_offset,
            body_operand_record_indices: SliceColumn::new(bodies, |body| body.operand.value),
            body_operand_record_offsets: SliceColumn::new(bodies, |body| body.operand.offset),
            relation_record_index: self.relation_record_index,
            relation_class_tag: self.relation_class_tag.as_str(),
            relation_byte_offset: self.relation_byte_offset,
            source_body_entity_suffixes: SliceColumn::new(bodies, |body| body.source.value),
            source_body_entity_suffix_offsets: SliceColumn::new(bodies, |body| body.source.offset),
            copied_body_entity_suffixes: SliceColumn::new(bodies, |body| body.copied.value),
            copied_body_entity_suffix_offsets: SliceColumn::new(bodies, |body| body.copied.offset),
        }
        .serialize(serializer)
    }
}

pub(crate) struct CopyPasteRecordLocation {
    pub(crate) record_index: u32,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) byte_offset: u64,
}

impl DesignCopyPasteBodiesOperation {
    pub(crate) fn try_new(
        bodies: Vec<DesignCopiedBody>,
        body_group: CopyPasteRecordLocation,
        relation: CopyPasteRecordLocation,
    ) -> Result<Self, CodecError> {
        Self::try_new_inner(bodies, body_group, relation).map_err(CodecError::Malformed)
    }

    fn try_new_inner(
        bodies: Vec<DesignCopiedBody>,
        body_group: CopyPasteRecordLocation,
        relation: CopyPasteRecordLocation,
    ) -> Result<Self, String> {
        let CopyPasteRecordLocation {
            record_index: body_group_record_index,
            class_tag: body_group_class_tag,
            byte_offset: body_group_byte_offset,
        } = body_group;
        let CopyPasteRecordLocation {
            record_index: relation_record_index,
            class_tag: relation_class_tag,
            byte_offset: relation_byte_offset,
        } = relation;
        if bodies.is_empty() {
            return Err("bodies must not be empty".into());
        }
        let mut suffixes = std::collections::HashSet::new();
        for (ordinal, body) in bodies.iter().enumerate() {
            let ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
            let operand_offset = ordinal
                .checked_mul(11)
                .and_then(|delta| body_group_byte_offset.checked_add(delta))
                .and_then(|offset| offset.checked_add(26))
                .ok_or("body operand offsets overflow")?;
            let source_offset = ordinal
                .checked_mul(30)
                .and_then(|delta| relation_byte_offset.checked_add(delta))
                .and_then(|offset| offset.checked_add(25))
                .ok_or("body source offsets overflow")?;
            let copied_offset = source_offset
                .checked_add(15)
                .ok_or("body copied offsets overflow")?;
            for suffix in [body.source.value, body.copied.value] {
                if suffixes.contains(&suffix) {
                    return Err("source and copied body suffixes must be pairwise distinct".into());
                }
                {}
                suffixes.insert(suffix);
            }
            if body.operand.offset != operand_offset
                || body.source.offset != source_offset
                || body.copied.offset != copied_offset
            {
                return Err(
                    "bodies operand, source, and copied offsets must follow their record strides"
                        .into(),
                );
            }
        }
        Ok(Self {
            bodies,
            body_group_record_index,
            body_group_class_tag,
            body_group_byte_offset,
            relation_record_index,
            relation_class_tag,
            relation_byte_offset,
        })
    }
    pub(crate) fn bodies(&self) -> &[DesignCopiedBody] {
        &self.bodies
    }
    pub(crate) fn body_group_byte_offset(&self) -> u64 {
        self.body_group_byte_offset
    }
    pub(crate) fn relation_byte_offset(&self) -> u64 {
        self.relation_byte_offset
    }
}

impl TryFrom<DesignCopyPasteBodiesOperationWire> for DesignCopyPasteBodiesOperation {
    type Error = String;
    fn try_from(wire: DesignCopyPasteBodiesOperationWire) -> Result<Self, Self::Error> {
        let count = wire.body_operand_record_indices.len();
        if wire.body_operand_record_offsets.len() != count {
            return Err(
                "body_operand_record_offsets must match body_operand_record_indices".into(),
            );
        }
        if wire.source_body_entity_suffixes.len() != count {
            return Err(
                "source_body_entity_suffixes must match body_operand_record_indices".into(),
            );
        }
        if wire.source_body_entity_suffix_offsets.len() != count {
            return Err(
                "source_body_entity_suffix_offsets must match body_operand_record_indices".into(),
            );
        }
        if wire.copied_body_entity_suffixes.len() != count {
            return Err(
                "copied_body_entity_suffixes must match body_operand_record_indices".into(),
            );
        }
        if wire.copied_body_entity_suffix_offsets.len() != count {
            return Err(
                "copied_body_entity_suffix_offsets must match body_operand_record_indices".into(),
            );
        }
        let mut bodies = {
            let mut storage = Vec::new();
            storage.try_reserve_exact(count).map(|()| storage)
        }
        .map_err(|error| error.to_string())?;
        bodies.extend(
            wire.body_operand_record_indices
                .into_iter()
                .zip(wire.body_operand_record_offsets)
                .zip(
                    wire.source_body_entity_suffixes
                        .into_iter()
                        .zip(wire.source_body_entity_suffix_offsets),
                )
                .zip(
                    wire.copied_body_entity_suffixes
                        .into_iter()
                        .zip(wire.copied_body_entity_suffix_offsets),
                )
                .map(
                    |(((value, offset), (source, source_offset)), (copied, copied_offset))| {
                        DesignCopiedBody {
                            operand: Located { value, offset },
                            source: Located {
                                value: source,
                                offset: source_offset,
                            },
                            copied: Located {
                                value: copied,
                                offset: copied_offset,
                            },
                        }
                    },
                ),
        );
        Self::try_new_inner(
            bodies,
            CopyPasteRecordLocation {
                record_index: wire.body_group_record_index,
                class_tag: wire.body_group_class_tag.try_into()?,
                byte_offset: wire.body_group_byte_offset,
            },
            CopyPasteRecordLocation {
                record_index: wire.relation_record_index,
                class_tag: wire.relation_class_tag.try_into()?,
                byte_offset: wire.relation_byte_offset,
            },
        )
    }
}

#[cfg(test)]
impl From<DesignCopyPasteBodiesOperation> for DesignCopyPasteBodiesOperationWire {
    fn from(value: DesignCopyPasteBodiesOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            body_group_class_tag: value.body_group_class_tag.into(),
            body_group_byte_offset: value.body_group_byte_offset,
            relation_record_index: value.relation_record_index,
            relation_class_tag: value.relation_class_tag.into(),
            relation_byte_offset: value.relation_byte_offset,
            body_operand_record_indices: value
                .bodies
                .iter()
                .map(|body| body.operand.value)
                .collect(),
            body_operand_record_offsets: value
                .bodies
                .iter()
                .map(|body| body.operand.offset)
                .collect(),
            source_body_entity_suffixes: value
                .bodies
                .iter()
                .map(|body| body.source.value)
                .collect(),
            source_body_entity_suffix_offsets: value
                .bodies
                .iter()
                .map(|body| body.source.offset)
                .collect(),
            copied_body_entity_suffixes: value
                .bodies
                .iter()
                .map(|body| body.copied.value)
                .collect(),
            copied_body_entity_suffix_offsets: value
                .bodies
                .iter()
                .map(|body| body.copied.offset)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation() -> DesignCopyPasteBodiesOperation {
        serde_json::from_str(r#"{"body_group_record_index":501,"body_group_class_tag":"264","body_group_byte_offset":100,"body_operand_record_indices":[502,504],"body_operand_record_offsets":[126,137],"relation_record_index":503,"relation_class_tag":"264","relation_byte_offset":200,"source_body_entity_suffixes":[11,13],"source_body_entity_suffix_offsets":[225,255],"copied_body_entity_suffixes":[12,14],"copied_body_entity_suffix_offsets":[240,270]}"#).unwrap()
    }

    #[test]
    fn copy_paste_bodies_borrowed_wire_matches_owned_wire_bytes() {
        let operation = operation();
        let owned = DesignCopyPasteBodiesOperationWire::from(operation.clone());
        assert_eq!(
            serde_json::to_vec(&operation).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }

    #[test]
    fn copy_paste_bodies_native_retained_limit_refuses_before_clone() {
        #[derive(Serialize)]
        struct NestedRecord<'a> {
            id: &'static str,
            value: &'a DesignCopyPasteBodiesOperation,
        }
        let operation = operation();
        let record = NestedRecord {
            id: "f3d:native:copy-paste-bodies#0",
            value: &operation,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || COPY_PASTE_BODIES_CLONE_COUNT.with(|count| count.set(0)),
            || COPY_PASTE_BODIES_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }
}

mod identity_rewrite;
