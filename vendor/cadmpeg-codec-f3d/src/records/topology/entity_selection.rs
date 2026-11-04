// SPDX-License-Identifier: Apache-2.0
//! Entity-selection operands, their candidates and the loft legacy body carrier.

use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_core::decode::u64_from_index;

use super::body_recipe::AsmHistoricalEntityKind;
use super::edge_identity::deserialize_resolved_edge_slot;
use super::fillet::HistoricalBinding;
use crate::records::identity::DesignSecondaryIdentity;
use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use serde::Deserialize;
use serde::Serialize;

/// Persistent Design entity selected through a nested indexed-record frame.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignEntitySelectionOperandWire")]
pub(crate) struct DesignEntitySelectionOperand {
    selection: EntitySelectionFrame,
    frame: crate::records::frame_chain::RecordFrameChain,
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Owning construction-operand group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
    /// Source per-file dynamic primary class tag.
    class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Primary entity identity in the nested identity pair; for a Sketch
    /// curve selection, this is the owning Sketch entity suffix.
    pub(crate) primary_identity: u64,
    /// Input-state edge proofs derived from the two serialized identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) historical_edge_candidates: Vec<DesignEntitySelectionEdgeCandidate>,
    /// History-qualified face proofs derived from the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Unique input-state edge selected by every available identity proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_edge_slot: Option<i64>,
}

#[cfg(test)]
thread_local! {
    static ENTITY_SELECTION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignEntitySelectionOperand {
    fn clone(&self) -> Self {
        #[cfg(test)]
        ENTITY_SELECTION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            selection: self.selection,
            frame: self.frame,
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            class_tag: self.class_tag.clone(),
            asset_id: self.asset_id.clone(),
            asset_id_offset: self.asset_id_offset,
            context_id: self.context_id.clone(),
            context_id_offset: self.context_id_offset,
            identity_record_offset: self.identity_record_offset,
            primary_identity: self.primary_identity,
            historical_edge_candidates: self.historical_edge_candidates.clone(),
            historical_face_candidates: self.historical_face_candidates.clone(),
            resolved_edge_slot: self.resolved_edge_slot,
        }
    }
}

impl Serialize for DesignEntitySelectionOperand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            id: &'a str,
            scope_record_index: u32,
            group_record_index: u32,
            group_member_ordinal: u32,
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a str,
            asset_id: &'a str,
            asset_id_offset: u64,
            context_id: &'a str,
            context_id_offset: u64,
            identity_record_index: u32,
            identity_record_offset: u64,
            primary_identity: u64,
            primary_identity_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            secondary_identity: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            secondary_identity_offset: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            curve_secondary_identity: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            curve_secondary_identity_offset: Option<u64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            historical_edge_candidates: &'a Vec<DesignEntitySelectionEdgeCandidate>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            historical_face_candidates: &'a Vec<DesignEntitySelectionFaceCandidate>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_edge_slot: Option<i64>,
            next_record_index: u32,
            next_byte_offset: u64,
        }
        let secondary = self.secondary();
        BorrowedWire {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            record_index: self.record_index(),
            byte_offset: self.byte_offset(),
            class_tag: self.class_tag.as_str(),
            asset_id: self.asset_id.as_str(),
            asset_id_offset: self.asset_id_offset,
            context_id: self.context_id.as_str(),
            context_id_offset: self.context_id_offset,
            identity_record_index: self.identity_record_index(),
            identity_record_offset: self.identity_record_offset,
            primary_identity: self.primary_identity,
            primary_identity_offset: self.primary_identity_offset(),
            secondary_identity: secondary.map(|value| value.identity.value),
            secondary_identity_offset: secondary.map(|value| value.identity.offset),
            curve_secondary_identity: secondary
                .and_then(|value| value.curve_identity)
                .map(|value| value.value),
            curve_secondary_identity_offset: secondary
                .and_then(|value| value.curve_identity)
                .map(|value| value.offset),
            historical_edge_candidates: &self.historical_edge_candidates,
            historical_face_candidates: &self.historical_face_candidates,
            resolved_edge_slot: self.resolved_edge_slot,
            next_record_index: self.next_record_index(),
            next_byte_offset: self.next_byte_offset(),
        }
        .serialize(serializer)
    }
}

impl DesignEntitySelectionOperand {
    pub(crate) fn try_new(draft: DesignEntitySelectionOperandDraft) -> Result<Self, String> {
        let selection = match draft.secondary {
            None if draft.identity_record_offset.checked_add(21)
                == Some(draft.primary_identity_offset) =>
            {
                EntitySelectionFrame::Primary {
                    next_record_index: draft.next_record_index,
                }
            }
            Some(secondary)
                if draft.identity_record_offset.checked_add(29)
                    == Some(draft.primary_identity_offset)
                    && draft.identity_record_offset.checked_add(37)
                        == Some(secondary.identity.offset)
                    && secondary.curve_identity.is_none_or(|curve| {
                        draft.identity_record_offset.checked_add(21) == Some(curve.offset)
                    }) =>
            {
                EntitySelectionFrame::Pair {
                    secondary: secondary.identity.value,
                    curve: secondary.curve_identity.map(|identity| identity.value),
                    next_record_index: draft.next_record_index,
                }
            }
            Some(secondary)
                if draft.class_tag.as_str() == "338"
                    && draft.identity_record_offset.checked_add(u64_from_index(
                        crate::layout::class_338_sketch_curve_identity::OWNER_RECORD_INDEX,
                    )) == Some(draft.primary_identity_offset)
                    && draft.identity_record_offset.checked_add(u64_from_index(
                        crate::layout::class_338_sketch_curve_identity::CURVE_PERSISTENT_ID,
                    )) == Some(secondary.identity.offset)
                    && secondary.curve_identity.is_none() =>
            {
                EntitySelectionFrame::SketchCurve {
                    secondary: secondary.identity.value,
                }
            }
            _ => return Err(
                "primary_identity_offset/secondary_identity_offset disagree with selection frame"
                    .into(),
            ),
        };
        draft
            .identity_record_offset
            .checked_add(selection.length())
            .ok_or("next_byte_offset overflows")?;
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            if matches!(selection, EntitySelectionFrame::SketchCurve { .. }) {
                4
            } else {
                3
            },
            0,
        )?;
        let value = Self {
            selection,
            frame,
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            group_record_index: draft.group_record_index,
            group_member_ordinal: draft.group_member_ordinal,
            class_tag: draft.class_tag,
            asset_id: draft.asset_id,
            asset_id_offset: draft.asset_id_offset,
            context_id: draft.context_id,
            context_id_offset: draft.context_id_offset,
            identity_record_offset: draft.identity_record_offset,
            primary_identity: draft.primary_identity,
            historical_edge_candidates: draft.historical_edge_candidates,
            historical_face_candidates: draft.historical_face_candidates,
            resolved_edge_slot: draft.resolved_edge_slot,
        };
        if value.identity_record_index() != draft.identity_record_index {
            return Err("identity_record_index disagrees with frame layout".into());
        }
        if value.primary_identity_offset() != draft.primary_identity_offset {
            return Err("primary_identity_offset disagrees with frame layout".into());
        }
        if value.secondary() != draft.secondary {
            return Err("secondary disagrees with frame layout".into());
        }
        if value.next_record_index() != draft.next_record_index {
            return Err("next_record_index disagrees with frame layout".into());
        }
        if value.next_byte_offset() != draft.next_byte_offset {
            return Err("next_byte_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignEntitySelectionOperandDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let identity_record_index = self.identity_record_index();
        let primary_identity_offset = self.primary_identity_offset();
        let secondary = self.secondary();
        let next_record_index = self.next_record_index();
        let next_byte_offset = self.next_byte_offset();
        DesignEntitySelectionOperandDraft {
            id: self.id,
            scope_record_index: self.scope_record_index,
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            asset_id_offset: self.asset_id_offset,
            context_id: self.context_id,
            context_id_offset: self.context_id_offset,
            identity_record_index,
            identity_record_offset: self.identity_record_offset,
            primary_identity: self.primary_identity,
            primary_identity_offset,
            secondary,
            historical_edge_candidates: self.historical_edge_candidates,
            historical_face_candidates: self.historical_face_candidates,
            resolved_edge_slot: self.resolved_edge_slot,
            next_record_index,
            next_byte_offset,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    pub(crate) fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    fn identity_record_index(&self) -> u32 {
        self.frame.index(3)
    }
    pub(crate) fn primary_identity_offset(&self) -> u64 {
        self.identity_record_offset + self.selection.primary_delta()
    }
    /// The single u64 identity of a compact 29-byte identity record, or
    /// `None` for a frame that stores an identity pair.
    pub(crate) fn compact_identity(&self) -> Option<u64> {
        matches!(self.selection, EntitySelectionFrame::Primary { .. })
            .then_some(self.primary_identity)
    }
    pub(crate) fn secondary(&self) -> Option<DesignSecondaryIdentity<Located<u64>>> {
        self.selection.secondary(self.identity_record_offset)
    }
    pub(crate) fn next_record_index(&self) -> u32 {
        match self.selection {
            EntitySelectionFrame::Primary { next_record_index }
            | EntitySelectionFrame::Pair {
                next_record_index, ..
            } => next_record_index,
            EntitySelectionFrame::SketchCurve { .. } => self.frame.index(4),
        }
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.identity_record_offset + self.selection.length()
    }
}

/// Unadmitted `DesignEntitySelectionOperand` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignEntitySelectionOperandDraft {
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Owning construction-operand group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
    /// Primary indexed-record identity.
    pub(crate) record_index: u32,
    /// Primary indexed-header byte offset.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub(crate) context_id_offset: u64,
    /// Nested indexed record that carries the persistent entity identity.
    pub(crate) identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub(crate) identity_record_offset: u64,
    /// Primary entity identity in the nested identity pair; for a Sketch
    /// curve selection, this is the owning Sketch entity suffix.
    pub(crate) primary_identity: u64,
    /// Byte offset of `primary_identity`.
    pub(crate) primary_identity_offset: u64,
    /// Secondary identity and any dependent curve identity, with their source locations.
    pub(crate) secondary: Option<DesignSecondaryIdentity<Located<u64>>>,
    /// Input-state edge proofs derived from the two serialized identities.
    pub(crate) historical_edge_candidates: Vec<DesignEntitySelectionEdgeCandidate>,
    /// History-qualified face proofs derived from the primary identity.
    pub(crate) historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Unique input-state edge selected by every available identity proof.
    pub(crate) resolved_edge_slot: Option<i64>,
    /// Identity of the indexed record immediately following the identity record.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record immediately following the identity record.
    pub(crate) next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignEntitySelectionOperandWire {
    /// Globally unique deterministic identifier for this native operand.
    id: String,
    /// Owning feature scope record.
    scope_record_index: u32,
    /// Owning construction-operand group record.
    group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    group_member_ordinal: u32,
    /// Primary indexed-record identity.
    record_index: u32,
    /// Primary indexed-header byte offset.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    asset_id: String,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Nested indexed record that carries the persistent entity identity.
    identity_record_index: u32,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Primary entity identity in the nested identity pair; for a Sketch
    /// curve selection, this is the owning Sketch entity suffix.
    primary_identity: u64,
    /// Byte offset of `primary_identity`.
    primary_identity_offset: u64,
    /// Secondary identity in the nested pair; for a Sketch curve selection,
    /// this is the curve's primary persistent identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_identity"
    )]
    secondary_identity: Option<u64>,
    /// Byte offset of `secondary_identity`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_identity_offset"
    )]
    secondary_identity_offset: Option<u64>,
    /// Optional secondary identity of the selected Sketch curve.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curve_secondary_identity"
    )]
    curve_secondary_identity: Option<u64>,
    /// Byte offset of `curve_secondary_identity`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curve_secondary_identity_offset"
    )]
    curve_secondary_identity_offset: Option<u64>,
    /// Input-state edge proofs derived from the two serialized identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_edge_candidates: Vec<DesignEntitySelectionEdgeCandidate>,
    /// History-qualified face proofs derived from the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Unique input-state edge selected by every available identity proof.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_edge_slot"
    )]
    resolved_edge_slot: Option<i64>,
    /// Identity of the indexed record immediately following the identity record.
    next_record_index: u32,
    /// Byte offset of the indexed record immediately following the identity record.
    next_byte_offset: u64,
}

impl TryFrom<DesignEntitySelectionOperandWire> for DesignEntitySelectionOperand {
    type Error = String;
    fn try_from(wire: DesignEntitySelectionOperandWire) -> Result<Self, Self::Error> {
        Self::try_new(DesignEntitySelectionOperandDraft {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            group_record_index: wire.group_record_index,
            group_member_ordinal: wire.group_member_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id.try_into()?,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id.try_into()?,
            context_id_offset: wire.context_id_offset,
            identity_record_index: wire.identity_record_index,
            identity_record_offset: wire.identity_record_offset,
            primary_identity: wire.primary_identity,
            primary_identity_offset: wire.primary_identity_offset,
            secondary: DesignSecondaryIdentity::from_wire(
                Located::from_wire(
                    wire.secondary_identity,
                    wire.secondary_identity_offset,
                    "secondary_identity",
                )?,
                Located::from_wire(
                    wire.curve_secondary_identity,
                    wire.curve_secondary_identity_offset,
                    "curve_secondary_identity",
                )?,
            )?,
            historical_edge_candidates: wire.historical_edge_candidates,
            historical_face_candidates: wire.historical_face_candidates,
            resolved_edge_slot: wire.resolved_edge_slot,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

#[cfg(test)]
impl From<DesignEntitySelectionOperand> for DesignEntitySelectionOperandWire {
    fn from(record: DesignEntitySelectionOperand) -> Self {
        let record = record.into_draft();
        Self {
            id: record.id,
            scope_record_index: record.scope_record_index,
            group_record_index: record.group_record_index,
            group_member_ordinal: record.group_member_ordinal,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag.into(),
            asset_id: record.asset_id.into(),
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id.into(),
            context_id_offset: record.context_id_offset,
            identity_record_index: record.identity_record_index,
            identity_record_offset: record.identity_record_offset,
            primary_identity: record.primary_identity,
            primary_identity_offset: record.primary_identity_offset,
            secondary_identity: record.secondary.map(|secondary| secondary.identity.value),
            secondary_identity_offset: record.secondary.map(|secondary| secondary.identity.offset),
            curve_secondary_identity: record
                .secondary
                .and_then(|secondary| secondary.curve_identity)
                .map(|identity| identity.value),
            curve_secondary_identity_offset: record
                .secondary
                .and_then(|secondary| secondary.curve_identity)
                .map(|identity| identity.offset),
            historical_edge_candidates: record.historical_edge_candidates,
            historical_face_candidates: record.historical_face_candidates,
            resolved_edge_slot: record.resolved_edge_slot,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}

/// Face proof for one persistent identity in one ASM history namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignEntitySelectionFaceCandidate {
    /// Native ASM history containing the selected identity.
    pub(crate) history_id: String,
    /// Stable ASM family, entity slot, and states containing the selected identity.
    #[serde(flatten)]
    pub(crate) historical: HistoricalBinding,
    /// Face incident to the identity in every listed state.
    pub(crate) face_slot: i64,
}

/// Legacy Boolean-Loft body carrier paired with a role-`0x8` body group.
///
/// The carrier is a scope-owned, role-less frame. It is retained separately
/// from the ordinary construction-operand group because its member and
/// scalar lanes do not use the counted-group grammar.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignLoftLegacyBodyCarrierSerde")]
pub(crate) struct DesignLoftLegacyBodyCarrier {
    /// Globally unique deterministic identifier.
    pub(crate) id: String,
    /// Owning Loft feature scope record.
    pub(crate) scope_record_index: u32,
    /// Primary indexed-record identity.
    pub(crate) record_index: u32,
    /// Primary indexed-header byte offset.
    pub(crate) byte_offset: u64,
    /// Per-file dynamic primary class tag (`322` or `411`).
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of `owner_scope_record_index`.
    pub(crate) owner_scope_record_index_offset: u64,
    /// The one member reference carried by this fixed legacy frame.
    pub(crate) member: u32,
    /// Byte offset of `member`.
    pub(crate) member_offset: u64,
    /// Byte offset of the on-wire member count (always 1).
    pub(crate) member_count_offset: u64,
    /// Opaque nonzero ordinal in the legacy scalar lane.
    pub(crate) opaque_index: std::num::NonZeroU8,
    /// Byte offset of the first `opaque_index` copy.
    pub(crate) opaque_index_offset: u64,
    /// Opaque scalar in the legacy scalar lane.
    pub(crate) opaque_scalar: cadmpeg_ir::scalar::FiniteReal,
    /// Byte offset of `opaque_scalar`.
    pub(crate) opaque_scalar_offset: u64,
    /// Byte offset of the repeated `opaque_index` copy.
    pub(crate) repeated_opaque_index_offset: u64,
    /// Record named by the marked `N+2` reference.
    pub(crate) next_next_record_index: u32,
    /// Byte offset of the marked `N+2` reference.
    pub(crate) next_next_reference_offset: u64,
    /// Byte offset of `flags`.
    pub(crate) flags_offset: u64,
    /// Record named by the marked `N+1` reference.
    pub(crate) next_record_index: u32,
    /// Byte offset of the marked `N+1` reference.
    pub(crate) next_reference_offset: u64,
    /// Source location of the additional owning-scope reference, when present.
    pub(crate) trailing_scope_reference_offset: Option<u64>,
    /// Per-file dynamic paired class tag (`262` or `266`).
    pub(crate) paired_class_tag: DesignClassTag,
    /// Same-index paired-header byte offset.
    pub(crate) paired_byte_offset: u64,
}

#[cfg(test)]
thread_local! {
    pub(super) static LOFT_LEGACY_BODY_CARRIER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignLoftLegacyBodyCarrier {
    fn clone(&self) -> Self {
        #[cfg(test)]
        LOFT_LEGACY_BODY_CARRIER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            owner_scope_record_index_offset: self.owner_scope_record_index_offset,
            member: self.member,
            member_offset: self.member_offset,
            member_count_offset: self.member_count_offset,
            opaque_index: self.opaque_index,
            opaque_index_offset: self.opaque_index_offset,
            opaque_scalar: self.opaque_scalar,
            opaque_scalar_offset: self.opaque_scalar_offset,
            repeated_opaque_index_offset: self.repeated_opaque_index_offset,
            next_next_record_index: self.next_next_record_index,
            next_next_reference_offset: self.next_next_reference_offset,
            flags_offset: self.flags_offset,
            next_record_index: self.next_record_index,
            next_reference_offset: self.next_reference_offset,
            trailing_scope_reference_offset: self.trailing_scope_reference_offset,
            paired_class_tag: self.paired_class_tag.clone(),
            paired_byte_offset: self.paired_byte_offset,
        }
    }
}

#[derive(Serialize)]
struct DesignLoftLegacyBodyCarrierRef<'a> {
    id: &'a str,
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: &'a str,
    owner_scope_record_index: u32,
    owner_scope_record_index_offset: u64,
    members: [u32; 1],
    member_offsets: [u64; 1],
    member_count: u32,
    member_count_offset: u64,
    opaque_index: u32,
    opaque_index_offset: u64,
    opaque_scalar: f64,
    opaque_scalar_offset: u64,
    repeated_opaque_index: u32,
    repeated_opaque_index_offset: u64,
    next_next_record_index: u32,
    next_next_reference_offset: u64,
    flags: NativeBytes<[u8; 2]>,
    flags_offset: u64,
    next_record_index: u32,
    next_reference_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    trailing_scope_record_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trailing_scope_reference_offset: Option<u64>,
    paired_class_tag: &'a str,
    paired_byte_offset: u64,
}

impl Serialize for DesignLoftLegacyBodyCarrier {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignLoftLegacyBodyCarrierRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: 0,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.as_str(),
            owner_scope_record_index: self.scope_record_index,
            owner_scope_record_index_offset: self.owner_scope_record_index_offset,
            members: [self.member],
            member_offsets: [self.member_offset],
            member_count: 1,
            member_count_offset: self.member_count_offset,
            opaque_index: u32::from(self.opaque_index.get()),
            opaque_index_offset: self.opaque_index_offset,
            opaque_scalar: self.opaque_scalar.get(),
            opaque_scalar_offset: self.opaque_scalar_offset,
            repeated_opaque_index: u32::from(self.opaque_index.get()),
            repeated_opaque_index_offset: self.repeated_opaque_index_offset,
            next_next_record_index: self.next_next_record_index,
            next_next_reference_offset: self.next_next_reference_offset,
            flags: ([0, 0]).into(),
            flags_offset: self.flags_offset,
            next_record_index: self.next_record_index,
            next_reference_offset: self.next_reference_offset,
            trailing_scope_record_index: self
                .trailing_scope_reference_offset
                .map(|_| self.scope_record_index),
            trailing_scope_reference_offset: self.trailing_scope_reference_offset,
            paired_class_tag: self.paired_class_tag.as_str(),
            paired_byte_offset: self.paired_byte_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignLoftLegacyBodyCarrierSerde {
    id: String,
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: String,
    owner_scope_record_index: u32,
    owner_scope_record_index_offset: u64,
    members: Vec<u32>,
    member_offsets: Vec<u64>,
    member_count: u32,
    member_count_offset: u64,
    opaque_index: u32,
    opaque_index_offset: u64,
    opaque_scalar: f64,
    opaque_scalar_offset: u64,
    repeated_opaque_index: u32,
    repeated_opaque_index_offset: u64,
    next_next_record_index: u32,
    next_next_reference_offset: u64,
    flags: NativeBytes<[u8; 2]>,
    flags_offset: u64,
    next_record_index: u32,
    next_reference_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_trailing_scope_record_index"
    )]
    trailing_scope_record_index: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_trailing_scope_reference_offset"
    )]
    trailing_scope_reference_offset: Option<u64>,
    paired_class_tag: String,
    paired_byte_offset: u64,
}

impl TryFrom<DesignLoftLegacyBodyCarrierSerde> for DesignLoftLegacyBodyCarrier {
    type Error = String;

    fn try_from(wire: DesignLoftLegacyBodyCarrierSerde) -> Result<Self, Self::Error> {
        if wire.scope_reference_ordinal != 0
            || wire.member_count != 1
            || wire.members.len() != 1
            || wire.member_offsets.len() != 1
        {
            return Err(
                "legacy loft body carrier must have one member at scope-reference ordinal zero"
                    .into(),
            );
        }
        if wire.owner_scope_record_index != wire.scope_record_index {
            return Err("owner_scope_record_index disagrees with scope_record_index".into());
        }
        if wire.repeated_opaque_index != wire.opaque_index {
            return Err("repeated_opaque_index disagrees with opaque_index".into());
        }
        if wire.flags != [0, 0] {
            return Err("flags must be zero".into());
        }
        let trailing_scope_reference_offset = match (
            wire.trailing_scope_record_index,
            wire.trailing_scope_reference_offset,
        ) {
            (None, None) => None,
            (Some(index), Some(offset)) if index == wire.scope_record_index => Some(offset),
            (Some(_), Some(_)) => return Err("trailing_scope_record_index disagrees with scope_record_index".into()),
            _ => return Err("trailing_scope_record_index and trailing_scope_reference_offset must occur together".into()),
        };
        let opaque_index = u8::try_from(wire.opaque_index)
            .ok()
            .and_then(std::num::NonZeroU8::new)
            .ok_or("opaque_index must be in 1..=255")?;
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            owner_scope_record_index_offset: wire.owner_scope_record_index_offset,
            member: wire.members[0],
            member_offset: wire.member_offsets[0],
            member_count_offset: wire.member_count_offset,
            opaque_index,
            opaque_index_offset: wire.opaque_index_offset,
            opaque_scalar: cadmpeg_ir::scalar::FiniteReal::new(wire.opaque_scalar)
                .ok_or("opaque_scalar must be finite")?,
            opaque_scalar_offset: wire.opaque_scalar_offset,
            repeated_opaque_index_offset: wire.repeated_opaque_index_offset,
            next_next_record_index: wire.next_next_record_index,
            next_next_reference_offset: wire.next_next_reference_offset,
            flags_offset: wire.flags_offset,
            next_record_index: wire.next_record_index,
            next_reference_offset: wire.next_reference_offset,
            trailing_scope_reference_offset,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

#[cfg(test)]
impl From<DesignLoftLegacyBodyCarrier> for DesignLoftLegacyBodyCarrierSerde {
    fn from(carrier: DesignLoftLegacyBodyCarrier) -> Self {
        Self {
            id: carrier.id,
            scope_record_index: carrier.scope_record_index,
            scope_reference_ordinal: 0,
            record_index: carrier.record_index,
            byte_offset: carrier.byte_offset,
            class_tag: carrier.class_tag.into(),
            owner_scope_record_index: carrier.scope_record_index,
            owner_scope_record_index_offset: carrier.owner_scope_record_index_offset,
            members: vec![carrier.member],
            member_offsets: vec![carrier.member_offset],
            member_count: 1,
            member_count_offset: carrier.member_count_offset,
            opaque_index: u32::from(carrier.opaque_index.get()),
            opaque_index_offset: carrier.opaque_index_offset,
            opaque_scalar: carrier.opaque_scalar.get(),
            opaque_scalar_offset: carrier.opaque_scalar_offset,
            repeated_opaque_index: u32::from(carrier.opaque_index.get()),
            repeated_opaque_index_offset: carrier.repeated_opaque_index_offset,
            next_next_record_index: carrier.next_next_record_index,
            next_next_reference_offset: carrier.next_next_reference_offset,
            flags: ([0, 0]).into(),
            flags_offset: carrier.flags_offset,
            next_record_index: carrier.next_record_index,
            next_reference_offset: carrier.next_reference_offset,
            trailing_scope_record_index: carrier
                .trailing_scope_reference_offset
                .map(|_| carrier.scope_record_index),
            trailing_scope_reference_offset: carrier.trailing_scope_reference_offset,
            paired_class_tag: carrier.paired_class_tag.into(),
            paired_byte_offset: carrier.paired_byte_offset,
        }
    }
}

/// Historical edge proof carried by one nested entity-selection identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignEntitySelectionEdgeCandidate {
    /// Zero for the first identity and one for the second identity.
    pub(crate) identity_ordinal: u32,
    /// Serialized persistent identity.
    pub(crate) local_id: u64,
    /// Stable ASM history family containing the identity.
    pub(crate) historical_entity_kind: AsmHistoricalEntityKind,
    /// Stable ASM entity slot after record-revision normalization.
    pub(crate) historical_entity_ref: i64,
    /// Edges incident to the stable entity in the feature-input topology.
    pub(crate) edge_slots: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntitySelectionFrame {
    Primary {
        next_record_index: u32,
    },
    Pair {
        secondary: u64,
        curve: Option<u64>,
        next_record_index: u32,
    },
    SketchCurve {
        secondary: u64,
    },
}

impl EntitySelectionFrame {
    fn primary_delta(self) -> u64 {
        match self {
            Self::Primary { .. } => 21,
            Self::Pair { .. } => 29,
            Self::SketchCurve { .. } => {
                u64_from_index(crate::layout::class_338_sketch_curve_identity::OWNER_RECORD_INDEX)
            }
        }
    }
    fn length(self) -> u64 {
        match self {
            Self::Primary { .. } => 29,
            Self::Pair { .. } => 45,
            Self::SketchCurve { .. } => {
                u64_from_index(crate::layout::class_338_sketch_curve_identity::LEN)
            }
        }
    }
    fn secondary(self, offset: u64) -> Option<DesignSecondaryIdentity<Located<u64>>> {
        match self {
            Self::Primary { .. } => None,
            Self::Pair {
                secondary, curve, ..
            } => Some(DesignSecondaryIdentity {
                identity: Located {
                    value: secondary,
                    offset: offset + 37,
                },
                curve_identity: curve.map(|value| Located {
                    value,
                    offset: offset + 21,
                }),
            }),
            Self::SketchCurve { secondary } => Some(DesignSecondaryIdentity {
                identity: Located {
                    value: secondary,
                    offset: offset
                        + u64_from_index(
                            crate::layout::class_338_sketch_curve_identity::CURVE_PERSISTENT_ID,
                        ),
                },
                curve_identity: None,
            }),
        }
    }
}

cadmpeg_core::named_optional_field!(deserialize_secondary_identity, u64, "secondary_identity");

cadmpeg_core::named_optional_field!(
    deserialize_secondary_identity_offset,
    u64,
    "secondary_identity_offset"
);

cadmpeg_core::named_optional_field!(
    deserialize_curve_secondary_identity,
    u64,
    "curve_secondary_identity"
);

cadmpeg_core::named_optional_field!(
    deserialize_curve_secondary_identity_offset,
    u64,
    "curve_secondary_identity_offset"
);

cadmpeg_core::named_optional_field!(
    deserialize_trailing_scope_record_index,
    u32,
    "trailing_scope_record_index"
);

cadmpeg_core::named_optional_field!(
    deserialize_trailing_scope_reference_offset,
    u64,
    "trailing_scope_reference_offset"
);

#[cfg(test)]
mod tests;

mod identity_rewrite;

/// One curve entry of a typed-reference profile identity record.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignProfileCurveEntry {
    /// Sketch-curve primary identity named by the entry.
    pub(crate) primary_id: u64,
    /// Byte offset of `primary_id`.
    pub(crate) primary_id_offset: u64,
    /// Sketch-curve secondary identity; zero names no secondary identity.
    pub(crate) secondary_id: u64,
    /// The three u32 values after the secondary identity, retained as
    /// encoded.
    pub(crate) entry_fields: [u32; 3],
}

/// Profile selection whose identity record lists curve loops of one Sketch,
/// written by the container generation whose references carry inline type
/// GUIDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignProfileCurveSelection {
    /// Globally unique deterministic identifier for this native selection.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Owning construction-operand group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
    /// Primary selection record index.
    pub(crate) record_index: u32,
    /// Byte offset of the primary selection record header.
    pub(crate) byte_offset: u64,
    /// Decimal Sketch entity suffix stored in the selection prefix.
    pub(crate) owner_text: String,
    /// Byte offset of the owner text's UTF-16LE code units.
    pub(crate) owner_text_offset: u64,
    /// Sketch scope named by the chain record at the selection index plus one.
    pub(crate) sketch_scope_record_index: u32,
    /// Byte offset of the Sketch scope reference target.
    pub(crate) sketch_scope_offset: u64,
    /// Byte offset of the identity record at the selection index plus three.
    pub(crate) identity_record_offset: u64,
    pub(crate) identity_end_offset: u64,
    /// Ordered region lists with their boundary loops.
    pub(crate) loop_lists: [Vec<Vec<DesignProfileCurveLoop>>; 2],
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignProfileCurveLoop {
    pub(crate) entries: Vec<DesignProfileCurveEntry>,
    pub(crate) outer: bool,
}

impl DesignProfileCurveSelection {
    pub(crate) fn valid_layout(&self) -> bool {
        self.lists_end() == Some(self.identity_end_offset)
    }

    pub(crate) fn lists_end(&self) -> Option<u64> {
        let mut cursor =
            self.identity_record_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    crate::layout::typed_reference_profile_identity_record::FIRST_REGION_COUNT,
                ))?;
        if self.loop_lists[0].is_empty() {
            return None;
        }
        for regions in &self.loop_lists {
            cursor = cursor.checked_add(4)?;
            for loops in regions {
                if loops.is_empty() || loops.iter().filter(|loop_| loop_.outer).count() != 1 {
                    return None;
                }
                cursor = cursor.checked_add(4)?;
                for loop_ in loops {
                    if loop_.entries.is_empty() {
                        return None;
                    }
                    cursor = cursor.checked_add(4)?;
                    for entry in &loop_.entries {
                        if entry.primary_id_offset != cursor.checked_add(4)? {
                            return None;
                        }
                        cursor = cursor.checked_add(40)?;
                    }
                    cursor = cursor.checked_add(1)?;
                }
            }
        }
        Some(cursor)
    }
}
