// SPDX-License-Identifier: Apache-2.0
//! Sketch constraints, relations and patterns.

use cadmpeg_ir::native::bytes::NativeBytes;

use super::{
    identity::{Located, ReferenceRun},
    references::DesignClassTag,
};
use crate::records::serde_column::SliceColumn;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

const EPS_SKETCH_PATTERN_DIRECTION_SQUARED: f64 = 1.0e-6;

/// A source sketch-pattern direction with squared length within the Design tolerance of one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub(crate) struct SketchPatternUnitDirection([f64; 3]);

impl TryFrom<[f64; 3]> for SketchPatternUnitDirection {
    type Error = &'static str;

    fn try_from(value: [f64; 3]) -> Result<Self, Self::Error> {
        let length = value.iter().map(|axis| axis * axis).sum::<f64>();
        ((length - 1.0).abs() <= EPS_SKETCH_PATTERN_DIRECTION_SQUARED)
            .then_some(Self(value))
            .ok_or("sketch pattern direction squared length must be within 1e-6 of one")
    }
}

impl From<SketchPatternUnitDirection> for [f64; 3] {
    fn from(value: SketchPatternUnitDirection) -> Self {
        value.0
    }
}

impl SketchPatternUnitDirection {
    pub(crate) fn get(self) -> [f64; 3] {
        self.0
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_rectangular_counted_reference_count,
    u32,
    "rectangular_counted_reference_count"
);
const SKETCH_CONSTRAINT_DEFINITIONS: [(u64, SketchConstraintKind); 20] = [
    (0x0000_0001, SketchConstraintKind::Coincident),
    (0x0000_0002, SketchConstraintKind::Colinear),
    (0x0000_0004, SketchConstraintKind::Concentric),
    (0x0000_0008, SketchConstraintKind::EqualLength),
    (0x0000_0010, SketchConstraintKind::Parallel),
    (0x0000_0020, SketchConstraintKind::Perpendicular),
    (0x0000_0040, SketchConstraintKind::Horizontal),
    (0x0000_0080, SketchConstraintKind::Vertical),
    (0x0000_0100, SketchConstraintKind::Tangent),
    (0x0000_0200, SketchConstraintKind::Curvature),
    (0x0000_0400, SketchConstraintKind::Symmetry),
    (0x0000_0800, SketchConstraintKind::Equal),
    (0x0000_1000, SketchConstraintKind::Midpoint),
    (0x0000_2000, SketchConstraintKind::Polygon),
    (0x1000_0000, SketchConstraintKind::CircularPattern),
    (0x2000_0000, SketchConstraintKind::RectangularPattern),
    (0x8000_0000, SketchConstraintKind::SplineGroup),
    (0x20_0000_0000, SketchConstraintKind::Offset),
    (0x100_0000_0000, SketchConstraintKind::TextFrame),
    (0x200_0000_0000, SketchConstraintKind::TextPath),
];

pub(super) const SKETCH_CONSTRAINT_MASK: u64 = {
    let mut mask = 0;
    let mut index = 0;
    while index < SKETCH_CONSTRAINT_DEFINITIONS.len() {
        mask |= SKETCH_CONSTRAINT_DEFINITIONS[index].0;
        index += 1;
    }
    mask
};

/// Decode the constraint kinds and unknown bits selected by a sketch-relation mask.
#[must_use]
pub(crate) fn constraint_kinds_from_state(state: u64) -> (Vec<SketchConstraintKind>, u64) {
    (
        constraint_kinds_iter(state).collect(),
        state & !SKETCH_CONSTRAINT_MASK,
    )
}

pub(crate) fn constraint_kinds_iter(state: u64) -> impl Iterator<Item = SketchConstraintKind> {
    std::iter::once(SketchConstraintKind::Coincident)
        .filter(move |_| state == 0)
        .chain(
            SKETCH_CONSTRAINT_DEFINITIONS
                .into_iter()
                .filter(move |(bit, _)| state & bit != 0)
                .map(|(_, kind)| kind),
        )
}

/// An indexed relation reference before or after sketch identity resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SketchRelationReference {
    /// Indexed Design record whose identity has not been resolved.
    Index(u32),
    /// Resolved identity, which contains its indexed Design record reference.
    Resolved(SketchRelationOperand),
}

impl SketchRelationReference {
    /// Indexed Design record referenced by this member.
    #[must_use]
    pub(crate) fn record_index(&self) -> u32 {
        match self {
            Self::Index(index) => *index,
            Self::Resolved(operand) => operand.record_index(),
        }
    }

    /// Identity after resolution, including a record with no sketch identity.
    #[must_use]
    pub(crate) fn resolved(&self) -> Option<&SketchRelationOperand> {
        match self {
            Self::Index(_) => None,
            Self::Resolved(operand) => Some(operand),
        }
    }

    fn from_wire(
        record_index: u32,
        resolved: Option<SketchRelationOperand>,
        field: &str,
    ) -> Result<Self, SketchRelationPayloadError> {
        match resolved {
            None => Ok(Self::Index(record_index)),
            Some(operand) if operand.record_index() == record_index => Ok(Self::Resolved(operand)),
            Some(_) => Err(SketchRelationPayloadError(format!(
                "sketch relation {field} record_index disagrees with its reference"
            ))),
        }
    }
}

/// One first-run sketch-relation member with its offset and ordinal.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchRelationMember {
    /// Indexed record or its resolved sketch identity.
    pub(crate) reference: SketchRelationReference,
    /// Payload offset of the member, relative to the record.
    pub(crate) offset: u32,
    /// Count of relations already recorded on this member, when retained by the wire.
    pub(crate) relation_ordinal: Option<u32>,
}

impl SketchRelationMember {
    /// An unresolved member with zero offset and no retained ordinal.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn from_index(record_index: u32) -> Self {
        Self {
            reference: SketchRelationReference::Index(record_index),
            offset: 0,
            relation_ordinal: None,
        }
    }
}

/// One return-run sketch-relation member with its offset.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchRelationReturnMember {
    /// Indexed record or its resolved sketch identity.
    pub(crate) reference: SketchRelationReference,
    /// Payload offset of the return member, relative to the record.
    pub(crate) offset: u32,
}

impl SketchRelationReturnMember {
    /// An unresolved return member with zero offset.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn from_index(record_index: u32) -> Self {
        Self {
            reference: SketchRelationReference::Index(record_index),
            offset: 0,
        }
    }
}

/// A complete first member run, either unresolved or resolved throughout.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SketchRelationMembers(pub(super) Vec<SketchRelationMember>);

impl SketchRelationMembers {
    /// Construct the unresolved source run with its member locations.
    pub(crate) fn from_indices(
        rows: impl IntoIterator<Item = (u32, usize, u32)>,
    ) -> Result<Self, CodecError> {
        let mut members = Vec::new();
        for (record_index, offset, relation_ordinal) in rows {
            let offset = u32::try_from(offset).map_err(|_| {
                CodecError::malformed("f3d sketch relation member offset exceeds u32")
            })?;

            members.push(SketchRelationMember {
                reference: SketchRelationReference::Index(record_index),
                offset,
                relation_ordinal: Some(relation_ordinal),
            });
        }
        Ok(Self(members))
    }

    /// Resolve every member while retaining its position metadata.
    pub(crate) fn resolve(&mut self, mut resolve: impl FnMut(u32) -> SketchRelationOperand) {
        // Resolve every row before writing any, so a panicking resolver leaves
        // the run unchanged.
        let resolved = self
            .0
            .iter()
            .map(|row| resolve(row.reference.record_index()))
            .collect::<Vec<_>>();
        for (row, operand) in self.0.iter_mut().zip(resolved) {
            row.reference = SketchRelationReference::Resolved(operand);
        }
    }
}

impl TryFrom<Vec<SketchRelationMember>> for SketchRelationMembers {
    type Error = SketchRelationPayloadError;

    fn try_from(rows: Vec<SketchRelationMember>) -> Result<Self, Self::Error> {
        if rows.windows(2).any(|pair| {
            pair[0].reference.resolved().is_some() != pair[1].reference.resolved().is_some()
        }) {
            return Err(SketchRelationPayloadError(
                "sketch relation members run mixes resolved and unresolved references".into(),
            ));
        }
        if rows
            .windows(2)
            .any(|pair| pair[0].relation_ordinal.is_some() != pair[1].relation_ordinal.is_some())
        {
            return Err(SketchRelationPayloadError(
                "sketch relation member ordinals are only partially present".into(),
            ));
        }
        Ok(Self(rows))
    }
}

impl std::ops::Deref for SketchRelationMembers {
    type Target = [SketchRelationMember];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A complete return member run, either unresolved or resolved throughout.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SketchRelationReturnMembers(Vec<SketchRelationReturnMember>);

impl SketchRelationReturnMembers {
    /// Construct the unresolved source run with its member locations.
    pub(crate) fn from_indices(
        rows: impl IntoIterator<Item = (u32, usize)>,
    ) -> Result<Self, CodecError> {
        let mut members = Vec::new();
        for (record_index, offset) in rows {
            let offset = u32::try_from(offset).map_err(|_| {
                CodecError::malformed("f3d sketch relation return offset exceeds u32")
            })?;

            members.push(SketchRelationReturnMember {
                reference: SketchRelationReference::Index(record_index),
                offset,
            });
        }
        Ok(Self(members))
    }

    /// Resolve every member while retaining its position metadata.
    pub(super) fn resolve(&mut self, mut resolve: impl FnMut(u32) -> SketchRelationOperand) {
        // Resolve every row before writing any, so a panicking resolver leaves
        // the run unchanged.
        let resolved = self
            .0
            .iter()
            .map(|row| resolve(row.reference.record_index()))
            .collect::<Vec<_>>();
        for (row, operand) in self.0.iter_mut().zip(resolved) {
            row.reference = SketchRelationReference::Resolved(operand);
        }
    }
}

impl TryFrom<Vec<SketchRelationReturnMember>> for SketchRelationReturnMembers {
    type Error = SketchRelationPayloadError;

    fn try_from(rows: Vec<SketchRelationReturnMember>) -> Result<Self, Self::Error> {
        if rows.windows(2).any(|pair| {
            pair[0].reference.resolved().is_some() != pair[1].reference.resolved().is_some()
        }) {
            return Err(SketchRelationPayloadError(
                "sketch relation return_members run mixes resolved and unresolved references"
                    .into(),
            ));
        }
        Ok(Self(rows))
    }
}

impl std::ops::Deref for SketchRelationReturnMembers {
    type Target = [SketchRelationReturnMember];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Finite row-major native glyph placement in centimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct SketchGlyphTransform([[f64; 4]; 4]);

impl SketchGlyphTransform {
    /// Native coefficients, without an affine or invertibility restriction.
    #[must_use]
    pub(crate) fn rows(self) -> [[f64; 4]; 4] {
        self.0
    }
}

impl TryFrom<[[f64; 4]; 4]> for SketchGlyphTransform {
    type Error = SketchRelationPayloadError;

    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        if rows.iter().flatten().any(|value| !value.is_finite()) {
            return Err(SketchRelationPayloadError(
                "sketch relation glyph_transforms contains a non-finite coefficient".into(),
            ));
        }
        Ok(Self(rows))
    }
}

impl From<SketchGlyphTransform> for [[f64; 4]; 4] {
    fn from(transform: SketchGlyphTransform) -> Self {
        transform.0
    }
}

/// Constraint mask and its matching pattern or text payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchRelationDefinition {
    state: u64,
    pattern: Option<SketchPatternDefinition>,
}

impl SketchRelationDefinition {
    /// Reject a payload that does not match the mask's first constraint kind.
    pub(crate) fn new(
        state: u64,
        pattern: Option<SketchPatternDefinition>,
    ) -> Result<Self, SketchRelationPayloadError> {
        let (kinds, _) = constraint_kinds_from_state(state);
        let first = kinds.first().copied();
        let agrees = match &pattern {
            None => !matches!(
                first,
                Some(
                    SketchConstraintKind::CircularPattern
                        | SketchConstraintKind::RectangularPattern
                        | SketchConstraintKind::TextFrame
                        | SketchConstraintKind::TextPath
                )
            ),
            Some(SketchPatternDefinition::Circular { .. }) => {
                first == Some(SketchConstraintKind::CircularPattern)
            }
            Some(SketchPatternDefinition::Rectangular { .. }) => {
                first == Some(SketchConstraintKind::RectangularPattern)
            }
            Some(SketchPatternDefinition::TextFrame { .. }) => {
                first == Some(SketchConstraintKind::TextFrame)
            }
            Some(SketchPatternDefinition::TextPath { .. }) => {
                first == Some(SketchConstraintKind::TextPath)
            }
        };
        if !agrees {
            return Err(SketchRelationPayloadError(
                "sketch relation pattern disagrees with the first constraint kind".into(),
            ));
        }
        Ok(Self { state, pattern })
    }

    /// Source sketch-constraint bitmask.
    #[must_use]
    pub(crate) fn state(&self) -> u64 {
        self.state
    }

    /// Pattern or text payload selected by the mask.
    #[must_use]
    pub(crate) fn pattern(&self) -> Option<&SketchPatternDefinition> {
        self.pattern.as_ref()
    }
}

/// Rejected sketch-relation payload or inconsistent native wire columns.
#[derive(Debug)]
pub(crate) struct SketchRelationPayloadError(String);

impl std::fmt::Display for SketchRelationPayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SketchRelationPayloadError {}

/// Counted constraint relation owned by a sketch container.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "SketchRelationSerde")]
pub(crate) struct SketchRelation {
    #[cfg(test)]
    clone_probe: SketchRelationCloneProbe,
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this relation record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this relation's type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the constraint mask relative to the record start.
    pub(crate) state_offset: u32,
    /// Numeric design-entity suffix of the sketch container that owns this relation.
    pub(crate) owner_reference: u32,
    /// Full Design entity id resolved from `owner_reference`.
    #[serde(default)]
    pub(crate) owner_entity_id: Option<cadmpeg_core::text::NonBlankString>,
    /// Nullable or role-specific references stored before the owner reference.
    auxiliary_references: ReferenceRun<u32, u32>,
    /// Serialized count of the rectangular class's ordered optional direction
    /// reference slots. `None` for other classes or data without a retained count.
    pub(crate) rectangular_counted_reference_count: Option<u32>,
    /// First reference run, interleaved with per-member relation ordinals.
    /// Its order does not define relation operand order.
    members: SketchRelationMembers,
    /// Payload offset of `owner_reference`, relative to the record.
    owner_reference_offset: u32,
    /// Constraint mask and the payload it selects.
    pub(crate) definition: SketchRelationDefinition,
    /// `EntityGenesis` origin bitfield stored by the relation record, when present.
    pub(crate) entity_genesis: Option<u64>,
    /// Second reference run in semantic member order.
    return_members: SketchRelationReturnMembers,
    /// Complete variable-width source record for native replay/write.
    raw_bytes: NativeBytes<Vec<u8>>,
}

#[cfg(test)]
thread_local! {
    static SKETCH_RELATION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct SketchRelationCloneProbe;

#[cfg(test)]
impl Clone for SketchRelationCloneProbe {
    fn clone(&self) -> Self {
        SKETCH_RELATION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self
    }
}

struct FilteredColumn<'a, T, U> {
    items: &'a [T],
    value: fn(&'a T) -> Option<U>,
}

impl<T, U: Serialize> Serialize for FilteredColumn<'_, T, U> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.items.iter().filter_map(self.value))
    }
}

struct ConstraintKinds(u64);

impl Serialize for ConstraintKinds {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(constraint_kinds_iter(self.0))
    }
}

impl Serialize for SketchRelation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            id: &'a str,
            record_index: u32,
            class_tag: &'a str,
            byte_offset: u64,
            state_offset: u32,
            owner_reference: u32,
            owner_entity_id: &'a str,
            auxiliary_references: SliceColumn<'a, Located<u32, u32>, u32>,
            auxiliary_reference_offsets: SliceColumn<'a, Located<u32, u32>, u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            rectangular_counted_reference_count: Option<u32>,
            members: SliceColumn<'a, SketchRelationMember, u32>,
            resolved_members: FilteredColumn<'a, SketchRelationMember, &'a SketchRelationOperand>,
            member_offsets: SliceColumn<'a, SketchRelationMember, u32>,
            owner_reference_offset: u32,
            state: u64,
            constraint_kinds: ConstraintKinds,
            unknown_constraint_bits: u64,
            member_relation_ordinals: FilteredColumn<'a, SketchRelationMember, u32>,
            entity_genesis: Option<u64>,
            pattern: Option<&'a SketchPatternDefinition>,
            return_members: SliceColumn<'a, SketchRelationReturnMember, u32>,
            resolved_return_members:
                FilteredColumn<'a, SketchRelationReturnMember, &'a SketchRelationOperand>,
            return_member_offsets: SliceColumn<'a, SketchRelationReturnMember, u32>,
            raw_bytes: NativeBytes<&'a [u8]>,
        }
        let auxiliary = self.auxiliary_references.located_rows().ok_or_else(|| {
            serde::ser::Error::custom("sketch relation auxiliary_references must be located")
        })?;
        let members = &self.members.0;
        let returns = &self.return_members.0;
        let state = self.definition.state();
        BorrowedWire {
            id: &self.id,
            record_index: self.record_index,
            class_tag: self.class_tag.as_str(),
            byte_offset: self.byte_offset,
            state_offset: self.state_offset,
            owner_reference: self.owner_reference,
            owner_entity_id: self
                .owner_entity_id
                .as_ref()
                .map_or("", |owner| owner.as_str()),
            auxiliary_references: SliceColumn::new(auxiliary, |row| row.value),
            auxiliary_reference_offsets: SliceColumn::new(auxiliary, |row| row.offset),
            rectangular_counted_reference_count: self.rectangular_counted_reference_count,
            members: SliceColumn::new(members, |row| row.reference.record_index()),
            resolved_members: FilteredColumn {
                items: members,
                value: |row| row.reference.resolved(),
            },
            member_offsets: SliceColumn::new(members, |row| row.offset),
            owner_reference_offset: self.owner_reference_offset,
            state,
            constraint_kinds: ConstraintKinds(state),
            unknown_constraint_bits: state & !SKETCH_CONSTRAINT_MASK,
            member_relation_ordinals: FilteredColumn {
                items: members,
                value: |row| row.relation_ordinal,
            },
            entity_genesis: self.entity_genesis,
            pattern: self.definition.pattern.as_ref(),
            return_members: SliceColumn::new(returns, |row| row.reference.record_index()),
            resolved_return_members: FilteredColumn {
                items: returns,
                value: |row| row.reference.resolved(),
            },
            return_member_offsets: SliceColumn::new(returns, |row| row.offset),
            raw_bytes: (&self.raw_bytes).into(),
        }
        .serialize(serializer)
    }
}

/// Unchecked sketch relation payload and byte frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchRelationDraft {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this relation record within the `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this relation's type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this record within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte offset of the constraint mask relative to the record start.
    pub(crate) state_offset: u32,
    /// Numeric design-entity suffix of the sketch container that owns this relation.
    pub(crate) owner_reference: u32,
    /// Full Design entity id resolved from `owner_reference`.
    pub(crate) owner_entity_id: Option<cadmpeg_core::text::NonBlankString>,
    /// Nullable or role-specific references stored before the owner reference.
    pub(crate) auxiliary_references: ReferenceRun<u32, u32>,
    /// Serialized count of the rectangular class's ordered optional direction
    /// reference slots. `None` for other classes or data without a retained count.
    pub(crate) rectangular_counted_reference_count: Option<u32>,
    /// First reference run, interleaved with per-member relation ordinals.
    /// Its order does not define relation operand order.
    pub(crate) members: SketchRelationMembers,
    /// Payload offset of `owner_reference`, relative to the record.
    pub(crate) owner_reference_offset: u32,
    /// Constraint mask and the payload it selects.
    pub(crate) definition: SketchRelationDefinition,
    /// `EntityGenesis` origin bitfield stored by the relation record, when present.
    pub(crate) entity_genesis: Option<u64>,
    /// Second reference run in semantic member order.
    pub(crate) return_members: SketchRelationReturnMembers,
    /// Complete variable-width source record for native replay/write.
    pub(crate) raw_bytes: Vec<u8>,
}

impl SketchRelation {
    /// Admit a relation whose reference offsets fit its retained bytes.
    pub(crate) fn try_new(draft: SketchRelationDraft) -> Result<Self, SketchRelationPayloadError> {
        if draft.raw_bytes.len() < 24 {
            return Err(SketchRelationPayloadError(
                "sketch relation raw_bytes is shorter than 24 bytes".into(),
            ));
        }
        if draft.auxiliary_references.located_rows().is_none() {
            return Err(SketchRelationPayloadError(
                "sketch relation auxiliary_references must be located".into(),
            ));
        }
        for (field, offset) in draft
            .members
            .iter()
            .map(|row| ("member_offsets", row.offset))
            .chain(
                draft
                    .auxiliary_references
                    .offsets()
                    .map(|offset| ("auxiliary_reference_offsets", *offset)),
            )
            .chain(std::iter::once((
                "owner_reference_offset",
                draft.owner_reference_offset,
            )))
            .chain(
                draft
                    .return_members
                    .iter()
                    .map(|row| ("return_member_offsets", row.offset)),
            )
        {
            if usize::try_from(offset)
                .ok()
                .and_then(|offset| offset.checked_add(4))
                .is_none_or(|end| end > draft.raw_bytes.len())
            {
                return Err(SketchRelationPayloadError(format!(
                    "sketch relation {field} exceeds raw_bytes"
                )));
            }
        }
        Ok(Self {
            #[cfg(test)]
            clone_probe: SketchRelationCloneProbe,
            id: draft.id,
            record_index: draft.record_index,
            class_tag: draft.class_tag,
            byte_offset: draft.byte_offset,
            state_offset: draft.state_offset,
            owner_reference: draft.owner_reference,
            owner_entity_id: draft.owner_entity_id,
            auxiliary_references: draft.auxiliary_references,
            rectangular_counted_reference_count: draft.rectangular_counted_reference_count,
            members: draft.members,
            owner_reference_offset: draft.owner_reference_offset,
            definition: draft.definition,
            entity_genesis: draft.entity_genesis,
            return_members: draft.return_members,
            raw_bytes: (draft.raw_bytes).into(),
        })
    }

    /// Return the unchecked payload for a checked edit.
    fn into_draft(self) -> SketchRelationDraft {
        SketchRelationDraft {
            id: self.id,
            record_index: self.record_index,
            class_tag: self.class_tag,
            byte_offset: self.byte_offset,
            state_offset: self.state_offset,
            owner_reference: self.owner_reference,
            owner_entity_id: self.owner_entity_id,
            auxiliary_references: self.auxiliary_references,
            rectangular_counted_reference_count: self.rectangular_counted_reference_count,
            members: self.members,
            owner_reference_offset: self.owner_reference_offset,
            definition: self.definition,
            entity_genesis: self.entity_genesis,
            return_members: self.return_members,
            raw_bytes: self.raw_bytes.into_inner(),
        }
    }

    /// Apply an edit only when the resulting byte frame is valid.
    pub(crate) fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut SketchRelationDraft),
    ) -> Result<(), SketchRelationPayloadError> {
        let mut draft = self.clone().into_draft();
        edit(&mut draft);
        *self = Self::try_new(draft)?;
        Ok(())
    }

    /// Resolve both member runs without changing their byte offsets.
    pub(crate) fn resolve_members(
        &mut self,
        mut resolve: impl FnMut(u32) -> SketchRelationOperand,
    ) {
        self.members.resolve(&mut resolve);
        self.return_members.resolve(resolve);
    }

    /// Retained auxiliary references.
    pub(crate) fn auxiliary_references(&self) -> &ReferenceRun<u32, u32> {
        &self.auxiliary_references
    }

    /// Retained members.
    pub(crate) fn members(&self) -> &SketchRelationMembers {
        &self.members
    }

    /// Retained return members.
    pub(crate) fn return_members(&self) -> &SketchRelationReturnMembers {
        &self.return_members
    }

    /// Retained raw bytes.
    pub(crate) fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }

    /// Owner reference offset within the retained bytes.
    pub(crate) fn owner_reference_offset(&self) -> u32 {
        self.owner_reference_offset
    }

    /// Constraint kinds selected by `state`.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn constraint_kinds(&self) -> Vec<SketchConstraintKind> {
        constraint_kinds_from_state(self.definition.state()).0
    }

    /// Bits in `state` outside the defined constraint mask.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn unknown_constraint_bits(&self) -> u64 {
        constraint_kinds_from_state(self.definition.state()).1
    }

    /// Record indices of the first reference run.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn member_indices(&self) -> Vec<u32> {
        self.members
            .iter()
            .map(|member| member.reference.record_index())
            .collect()
    }

    /// Record indices of the return reference run.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn return_member_indices(&self) -> Vec<u32> {
        self.return_members
            .iter()
            .map(|member| member.reference.record_index())
            .collect()
    }

    /// First-run then return-run record indices.
    pub(crate) fn all_member_indices(&self) -> impl Iterator<Item = u32> + '_ {
        self.members
            .iter()
            .map(|member| member.reference.record_index())
            .chain(
                self.return_members
                    .iter()
                    .map(|member| member.reference.record_index()),
            )
    }

    /// Resolved first-run members, empty for an unresolved run.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn resolved_members(&self) -> Vec<SketchRelationOperand> {
        self.members
            .iter()
            .filter_map(|member| member.reference.resolved().cloned())
            .collect()
    }

    /// Resolved return-run members, empty for an unresolved run.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn resolved_return_members(&self) -> Vec<SketchRelationOperand> {
        self.return_members
            .iter()
            .filter_map(|member| member.reference.resolved().cloned())
            .collect()
    }
}

fn zip_relation_members(
    members: Vec<u32>,
    offsets: Vec<u32>,
    ordinals: Vec<u32>,
    resolved: Vec<SketchRelationOperand>,
) -> Result<SketchRelationMembers, SketchRelationPayloadError> {
    let len = members.len();
    let offsets = pad_or_check("member_offsets", offsets, len)?;
    let resolved = pad_resolved("resolved_members", resolved, len)?;
    let ordinals = if ordinals.is_empty() {
        (0..len).map(|_| None).collect::<Vec<_>>()
    } else if ordinals.len() == len {
        ordinals.into_iter().map(Some).collect::<Vec<_>>()
    } else {
        return Err(SketchRelationPayloadError(
            "sketch relation member_relation_ordinals length does not match members".into(),
        ));
    };
    (members
        .into_iter()
        .zip(offsets)
        .zip(ordinals)
        .zip(resolved)
        .map(|(((record_index, offset), relation_ordinal), resolved)| {
            Ok(SketchRelationMember {
                reference: SketchRelationReference::from_wire(
                    record_index,
                    resolved,
                    "resolved_members",
                )?,
                offset,
                relation_ordinal,
            })
        }))
    .collect::<Result<Vec<_>, _>>()?
    .try_into()
}

fn zip_return_members(
    members: Vec<u32>,
    offsets: Vec<u32>,
    resolved: Vec<SketchRelationOperand>,
) -> Result<SketchRelationReturnMembers, SketchRelationPayloadError> {
    let len = members.len();
    let offsets = pad_or_check("return_member_offsets", offsets, len)?;
    let resolved = pad_resolved("resolved_return_members", resolved, len)?;
    (members
        .into_iter()
        .zip(offsets)
        .zip(resolved)
        .map(|((record_index, offset), resolved)| {
            Ok(SketchRelationReturnMember {
                reference: SketchRelationReference::from_wire(
                    record_index,
                    resolved,
                    "resolved_return_members",
                )?,
                offset,
            })
        }))
    .collect::<Result<Vec<_>, _>>()?
    .try_into()
}

fn pad_or_check(
    name: &str,
    values: Vec<u32>,
    len: usize,
) -> Result<Vec<u32>, SketchRelationPayloadError> {
    if values.is_empty() {
        Ok((0..len).map(|_| 0).collect())
    } else if values.len() == len {
        Ok(values)
    } else {
        Err(SketchRelationPayloadError(format!(
            "sketch relation {name} length {} does not match members {len}",
            values.len()
        )))
    }
}

fn pad_resolved(
    name: &str,
    values: Vec<SketchRelationOperand>,
    len: usize,
) -> Result<Vec<Option<SketchRelationOperand>>, SketchRelationPayloadError> {
    if values.is_empty() {
        Ok((0..len).map(|_| None).collect())
    } else if values.len() == len {
        Ok(values.into_iter().map(Some).collect::<Vec<_>>())
    } else {
        Err(SketchRelationPayloadError(format!(
            "sketch relation {name} length {} does not match members {len}",
            values.len()
        )))
    }
}

/// Wire form of [`SketchRelation`] with the historical flat field set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SketchRelationSerde {
    id: String,
    record_index: u32,
    class_tag: String,
    byte_offset: u64,
    state_offset: u32,
    owner_reference: u32,
    #[serde(default)]
    owner_entity_id: String,
    #[serde(default)]
    auxiliary_references: Vec<u32>,
    #[serde(default)]
    auxiliary_reference_offsets: Vec<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rectangular_counted_reference_count"
    )]
    rectangular_counted_reference_count: Option<u32>,
    members: Vec<u32>,
    #[serde(default)]
    resolved_members: Vec<SketchRelationOperand>,
    #[serde(default)]
    member_offsets: Vec<u32>,
    #[serde(default)]
    owner_reference_offset: u32,
    state: u64,
    #[serde(default)]
    constraint_kinds: Vec<SketchConstraintKind>,
    #[serde(default)]
    unknown_constraint_bits: u64,
    #[serde(default)]
    member_relation_ordinals: Vec<u32>,
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    entity_genesis: Option<u64>,
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    pattern: Option<SketchPatternDefinition>,
    return_members: Vec<u32>,
    #[serde(default)]
    resolved_return_members: Vec<SketchRelationOperand>,
    #[serde(default)]
    return_member_offsets: Vec<u32>,
    raw_bytes: NativeBytes<Vec<u8>>,
}

impl TryFrom<SketchRelationSerde> for SketchRelation {
    type Error = SketchRelationPayloadError;

    fn try_from(wire: SketchRelationSerde) -> Result<Self, Self::Error> {
        Self::from_wire_inner(wire)
    }
}

impl SketchRelation {
    fn from_wire_inner(wire: SketchRelationSerde) -> Result<Self, SketchRelationPayloadError> {
        let (derived_kinds, derived_unknown) = constraint_kinds_from_state(wire.state);
        if !wire.constraint_kinds.is_empty() && wire.constraint_kinds != derived_kinds {
            return Err(SketchRelationPayloadError(
                "sketch relation constraint_kinds disagrees with state".into(),
            ));
        }
        if wire.unknown_constraint_bits != 0 && wire.unknown_constraint_bits != derived_unknown {
            return Err(SketchRelationPayloadError(
                "sketch relation unknown_constraint_bits disagrees with state".into(),
            ));
        }
        if wire.auxiliary_references.len() != wire.auxiliary_reference_offsets.len() {
            return Err(SketchRelationPayloadError(
                "sketch relation auxiliary_reference_offsets must locate every reference".into(),
            ));
        }
        let definition = SketchRelationDefinition::new(wire.state, wire.pattern)?;
        Self::try_new(SketchRelationDraft {
            id: wire.id,
            record_index: wire.record_index,
            class_tag: DesignClassTag::try_from(wire.class_tag)
                .map_err(SketchRelationPayloadError)?,
            byte_offset: wire.byte_offset,
            state_offset: wire.state_offset,
            owner_reference: wire.owner_reference,
            owner_entity_id: cadmpeg_core::text::NonBlankString::new(wire.owner_entity_id),
            auxiliary_references: ReferenceRun::located(
                wire.auxiliary_references
                    .into_iter()
                    .zip(wire.auxiliary_reference_offsets)
                    .map(|(value, offset)| Located { value, offset })
                    .collect::<Vec<_>>(),
            ),
            rectangular_counted_reference_count: wire.rectangular_counted_reference_count,
            members: zip_relation_members(
                wire.members,
                wire.member_offsets,
                wire.member_relation_ordinals,
                wire.resolved_members,
            )?,
            owner_reference_offset: wire.owner_reference_offset,
            definition,
            entity_genesis: wire.entity_genesis,
            return_members: zip_return_members(
                wire.return_members,
                wire.return_member_offsets,
                wire.resolved_return_members,
            )?,
            raw_bytes: wire.raw_bytes.into_inner(),
        })
    }
}

#[cfg(test)]
impl From<SketchRelation> for SketchRelationSerde {
    fn from(relation: SketchRelation) -> Self {
        let (constraint_kinds, unknown_constraint_bits) =
            constraint_kinds_from_state(relation.definition.state());
        let (auxiliary_references, auxiliary_reference_offsets) =
            relation.auxiliary_references.into_wire();
        Self {
            id: relation.id,
            record_index: relation.record_index,
            class_tag: relation.class_tag.into(),
            byte_offset: relation.byte_offset,
            state_offset: relation.state_offset,
            owner_reference: relation.owner_reference,
            owner_entity_id: relation
                .owner_entity_id
                .map(|owner| owner.as_str().to_owned())
                .unwrap_or_default(),
            auxiliary_references,
            auxiliary_reference_offsets,
            rectangular_counted_reference_count: relation.rectangular_counted_reference_count,
            members: relation
                .members
                .iter()
                .map(|member| member.reference.record_index())
                .collect(),
            resolved_members: relation
                .members
                .iter()
                .filter_map(|member| member.reference.resolved().cloned())
                .collect(),
            member_offsets: relation
                .members
                .iter()
                .map(|member| member.offset)
                .collect(),
            owner_reference_offset: relation.owner_reference_offset,
            state: relation.definition.state(),
            constraint_kinds,
            unknown_constraint_bits,
            member_relation_ordinals: relation
                .members
                .iter()
                .filter_map(|member| member.relation_ordinal)
                .collect(),
            entity_genesis: relation.entity_genesis,
            pattern: relation.definition.pattern,
            return_members: relation
                .return_members
                .iter()
                .map(|member| member.reference.record_index())
                .collect(),
            resolved_return_members: relation
                .return_members
                .iter()
                .filter_map(|member| member.reference.resolved().cloned())
                .collect(),
            return_member_offsets: relation
                .return_members
                .iter()
                .map(|member| member.offset)
                .collect(),
            raw_bytes: (relation.raw_bytes),
        }
    }
}

/// One sketch-relation reference resolved against the indexed Design record graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SketchRelationOperand {
    /// A sketch point.
    Point {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
        /// Persistent point identity stored by that record, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        persistent_id: Option<u64>,
    },
    /// A persistent sketch curve.
    Curve {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
        /// Primary persistent curve identity.
        primary_id: u64,
        /// Nullable secondary persistent curve identity.
        secondary_id: u64,
    },
    /// A persistent sketch surface.
    Surface {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
        /// Persistent surface identity stored by that record.
        persistent_id: u64,
    },
    /// A referenced indexed record without point, curve, or surface identity fields.
    Record {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
    },
}

impl SketchRelationOperand {
    /// Indexed Design record that owns this identity.
    #[must_use]
    fn record_index(&self) -> u32 {
        match self {
            Self::Point { record_index, .. }
            | Self::Curve { record_index, .. }
            | Self::Surface { record_index, .. }
            | Self::Record { record_index } => *record_index,
        }
    }
}

/// One bit in a Fusion sketch-constraint state mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SketchConstraintKind {
    /// Points or endpoints occupy the same position.
    Coincident,
    /// Two line-bearing entities lie on one infinite line.
    Colinear,
    /// Circular entities share a center.
    Concentric,
    /// Line-bearing entities have equal length.
    EqualLength,
    /// Line-bearing entities have parallel directions.
    Parallel,
    /// Line-bearing entities meet at a right angle.
    Perpendicular,
    /// An entity is horizontal in sketch coordinates.
    Horizontal,
    /// An entity is vertical in sketch coordinates.
    Vertical,
    /// Two entities share a tangent direction at contact.
    Tangent,
    /// Two entities share curvature at contact.
    Curvature,
    /// Entities are symmetric about an axis.
    Symmetry,
    /// Entities have equal size.
    Equal,
    /// A point lies at an entity midpoint.
    Midpoint,
    /// Entities participate in a polygon relation.
    Polygon,
    /// Result entities are offset from oriented source entities by one magnitude.
    Offset,
    /// A spline's defining entities grouped under the owning sketch.
    SplineGroup,
    /// Entities participate in a circular pattern.
    CircularPattern,
    /// Entities participate in a rectangular pattern.
    RectangularPattern,
    /// Frame curves bound to a sketch-text entity.
    TextFrame,
    /// A sketch-text entity bound to a path curve.
    TextPath,
}

/// A sketch pattern instance count in 1..=100000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct SketchPatternCount(u32);

impl TryFrom<u32> for SketchPatternCount {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if !(1..=100_000).contains(&value) {
            return Err("evaluated_count must be in 1..=100000");
        }
        Ok(Self(value))
    }
}

impl From<SketchPatternCount> for u32 {
    fn from(value: SketchPatternCount) -> Self {
        value.0
    }
}

impl SketchPatternCount {
    pub(crate) fn get(self) -> u32 {
        self.0
    }
}

/// Class-specific auxiliary payload of a pattern or text sketch relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SketchPatternDefinition {
    /// A circular-pattern relation's auxiliary operands.
    Circular {
        /// Optional record index of the total-angle parameter value record.
        angle_parameter: Option<u32>,
        /// Optional record index of the instance-count parameter value record.
        count_parameter: Option<u32>,
        /// Evaluated total pattern angle in radians.
        evaluated_angle: FiniteReal,
        /// Evaluated instance count.
        evaluated_count: SketchPatternCount,
    },
    /// A rectangular-pattern relation's two direction clauses.
    Rectangular {
        /// The two pattern direction clauses in record order.
        directions: [SketchPatternDirection; 2],
    },
    /// A text-frame relation's auxiliary operand.
    TextFrame {
        /// Record index of the sketch-text entity the frame curves bind to.
        text_reference: u32,
    },
    /// A text-path relation's auxiliary operands.
    TextPath {
        /// Record index of the sketch-text entity placed along the path curve.
        text_reference: u32,
        /// Row-major 4×4 character placement transforms in character order,
        /// in centimetres.
        glyph_transforms: Vec<SketchGlyphTransform>,
    },
}

/// One direction clause of a rectangular-pattern sketch relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SketchPatternDirection {
    /// Evaluated instance count along this direction.
    pub(crate) evaluated_count: SketchPatternCount,
    /// Optional record index of the count parameter value record.
    pub(crate) count_parameter: Option<u32>,
    /// Direction in sketch coordinates; the decoder admits a squared length
    /// within `1e-6` of one.
    pub(crate) direction: SketchPatternUnitDirection,
    /// Evaluated source distance along this direction, in source units. The
    /// source record's first rectangular flag selects extent or spacing.
    pub(crate) evaluated_distance: FiniteReal,
    /// Optional record index of the distance parameter value record.
    pub(crate) distance_parameter: Option<u32>,
}

#[cfg(test)]
mod tests;
