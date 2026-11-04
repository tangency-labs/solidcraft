// SPDX-License-Identifier: Apache-2.0
//! Body-recipe operands, operand groups, owners and persistent references.

use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_core::decode::u64_from_index;

use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use cadmpeg_ir::ids::FaceId;
use serde::Deserialize;
use serde::Serialize;

/// Whole-body construction operand carrying a persistent body-recipe reference.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignBodyRecipeOperandWire")]
pub(crate) struct DesignBodyRecipeOperand {
    frame: crate::records::frame_chain::RecordFrameChain,
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Exact feature-scope ownership form.
    #[serde(flatten)]
    pub(crate) owner: DesignOperandOwner,
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the persistent selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector_tail: Option<Located<NativeBytes<[u8; 4]>>>,
    /// Counted persistent Design references carried by this operand.
    references: Vec<DesignBodyRecipeReference>,
    /// Body construction recipe contained by this operand record.
    pub(crate) recipe_id: String,
    /// Unique input-state face selected by this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) resolved_body_face_slots: Vec<i64>,
    /// Identity of the indexed record immediately following this operand.
    next_record_index: u32,
    /// Byte offset of the indexed record immediately following this operand.
    next_byte_offset: u64,
}

#[cfg(test)]
thread_local! {
    static BODY_RECIPE_OPERAND_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignBodyRecipeOperand {
    fn clone(&self) -> Self {
        #[cfg(test)]
        BODY_RECIPE_OPERAND_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            frame: self.frame,
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            owner: self.owner,
            class_tag: self.class_tag.clone(),
            asset_id: self.asset_id.clone(),
            context_id: self.context_id.clone(),
            context_id_offset: self.context_id_offset,
            selector_tail: (self.selector_tail),
            references: self.references.clone(),
            recipe_id: self.recipe_id.clone(),
            resolved_face_slot: self.resolved_face_slot,
            resolved_body_state_id: self.resolved_body_state_id,
            resolved_body_slot: self.resolved_body_slot,
            resolved_body_face_slots: self.resolved_body_face_slots.clone(),
            next_record_index: self.next_record_index,
            next_byte_offset: self.next_byte_offset,
        }
    }
}

impl DesignBodyRecipeOperand {
    pub(crate) fn try_new(draft: DesignBodyRecipeOperandDraft) -> Result<Self, String> {
        if draft.context_id_offset <= draft.asset_id_offset
            || draft.context_id_offset >= draft.next_byte_offset
            || draft.next_byte_offset <= draft.nested_record_index_offset
            || draft.selector_tail.is_some_and(|tail| {
                tail.offset < draft.context_id_offset
                    || tail
                        .offset
                        .checked_add(4)
                        .is_none_or(|end| end > draft.next_byte_offset)
            })
        {
            return Err(
                "context_id_offset/selector_tail/next_byte_offset disagree with body frame".into(),
            );
        }
        for (ordinal, reference) in draft.references.iter().enumerate() {
            let offset = u64::try_from(ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_mul(12))
                .and_then(|delta| draft.byte_offset.checked_add(25)?.checked_add(delta));
            if reference.design_reference == 0
                || offset != Some(reference.design_reference_offset)
                || offset.and_then(|offset| offset.checked_add(8)) != Some(reference.form_offset)
            {
                return Err("references contain an invalid identity or offset".into());
            }
        }
        if draft
            .next_record_index
            .checked_sub(draft.record_index)
            .is_some_and(|delta| delta < 4)
        {
            return Err("next_record_index names a record of the operand prologue".into());
        }
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            3,
            u64::try_from(draft.references.len())
                .ok()
                .and_then(|count| count.checked_mul(12))
                .and_then(|bytes| bytes.checked_add(44))
                .ok_or("references length overflows")?,
        )?;
        let value = Self {
            frame,
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            owner: draft.owner,
            class_tag: draft.class_tag,
            asset_id: draft.asset_id,
            context_id: draft.context_id,
            context_id_offset: draft.context_id_offset,
            selector_tail: draft.selector_tail.map(|tail| Located {
                value: tail.value.into(),
                offset: tail.offset,
            }),
            references: draft.references,
            recipe_id: draft.recipe_id,
            resolved_face_slot: draft.resolved_face_slot,
            resolved_body_state_id: draft.resolved_body_state_id,
            resolved_body_slot: draft.resolved_body_slot,
            resolved_body_face_slots: draft.resolved_body_face_slots,
            next_record_index: draft.next_record_index,
            next_byte_offset: draft.next_byte_offset,
        };
        if value.nested_record_index() != draft.nested_record_index {
            return Err("nested_record_index disagrees with frame layout".into());
        }
        if value.nested_record_index_offset() != draft.nested_record_index_offset {
            return Err("nested_record_index_offset disagrees with frame layout".into());
        }
        if value.asset_id_offset() != draft.asset_id_offset {
            return Err("asset_id_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignBodyRecipeOperandDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let nested_record_index = self.nested_record_index();
        let next_record_index = self.next_record_index();
        let nested_record_index_offset = self.nested_record_index_offset();
        let asset_id_offset = self.asset_id_offset();
        DesignBodyRecipeOperandDraft {
            id: self.id,
            scope_record_index: self.scope_record_index,
            owner: self.owner,
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            asset_id_offset,
            context_id: self.context_id,
            context_id_offset: self.context_id_offset,
            selector_tail: self.selector_tail.map(|tail| Located {
                value: tail.value.into_inner(),
                offset: tail.offset,
            }),
            references: self.references,
            nested_record_index,
            nested_record_index_offset,
            recipe_id: self.recipe_id,
            resolved_face_slot: self.resolved_face_slot,
            resolved_body_state_id: self.resolved_body_state_id,
            resolved_body_slot: self.resolved_body_slot,
            resolved_body_face_slots: self.resolved_body_face_slots,
            next_record_index,
            next_byte_offset: self.next_byte_offset,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    fn asset_id_offset(&self) -> u64 {
        self.nested_record_index_offset() + 18
    }
    pub(crate) fn context_id_offset(&self) -> u64 {
        self.context_id_offset
    }
    pub(crate) fn references(&self) -> &Vec<DesignBodyRecipeReference> {
        &self.references
    }
    pub(crate) fn nested_record_index(&self) -> u64 {
        u64::from(self.frame.index(3))
    }
    fn nested_record_index_offset(&self) -> u64 {
        self.frame
            .offset(26 + u64_from_index(self.references.len()) * 12)
    }
    pub(crate) fn next_record_index(&self) -> u32 {
        self.next_record_index
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.next_byte_offset
    }
}

impl Serialize for DesignBodyRecipeOperand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            scope_record_index: u32,
            #[serde(flatten)]
            owner: DesignOperandOwner,
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a str,
            asset_id: &'a str,
            asset_id_offset: u64,
            context_id: &'a str,
            context_id_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            selector_tail: Option<NativeBytes<[u8; 4]>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            selector_tail_offset: Option<u64>,
            references: &'a [DesignBodyRecipeReference],
            nested_record_index: u64,
            nested_record_index_offset: u64,
            recipe_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_face_slot: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_body_state_id: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_body_slot: Option<i64>,
            #[serde(skip_serializing_if = "<[i64]>::is_empty")]
            resolved_body_face_slots: &'a [i64],
            next_record_index: u32,
            next_byte_offset: u64,
        }
        WireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            owner: self.owner,
            record_index: self.record_index(),
            byte_offset: self.byte_offset(),
            class_tag: self.class_tag.as_str(),
            asset_id: self.asset_id.as_str(),
            asset_id_offset: self.asset_id_offset(),
            context_id: self.context_id.as_str(),
            context_id_offset: self.context_id_offset,
            selector_tail: (self.selector_tail.map(|tail| tail.value)),
            selector_tail_offset: self.selector_tail.map(|tail| tail.offset),
            references: &self.references,
            nested_record_index: self.nested_record_index(),
            nested_record_index_offset: self.nested_record_index_offset(),
            recipe_id: &self.recipe_id,
            resolved_face_slot: self.resolved_face_slot,
            resolved_body_state_id: self.resolved_body_state_id,
            resolved_body_slot: self.resolved_body_slot,
            resolved_body_face_slots: &self.resolved_body_face_slots,
            next_record_index: self.next_record_index(),
            next_byte_offset: self.next_byte_offset,
        }
        .serialize(serializer)
    }
}

/// Unadmitted `DesignBodyRecipeOperand` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignBodyRecipeOperandDraft {
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Exact feature-scope ownership form.
    pub(crate) owner: DesignOperandOwner,
    /// Primary indexed-record identity.
    pub(crate) record_index: u32,
    /// Primary indexed-header byte offset.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the persistent selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub(crate) context_id_offset: u64,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    pub(crate) selector_tail: Option<Located<[u8; 4]>>,
    /// Counted persistent Design references carried by this operand.
    pub(crate) references: Vec<DesignBodyRecipeReference>,
    /// Tagged nested record reference following the Design reference.
    pub(crate) nested_record_index: u64,
    /// Byte offset of `nested_record_index`.
    pub(crate) nested_record_index_offset: u64,
    /// Body construction recipe contained by this operand record.
    pub(crate) recipe_id: String,
    /// Unique input-state face selected by this operand.
    pub(crate) resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    pub(crate) resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    pub(crate) resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    pub(crate) resolved_body_face_slots: Vec<i64>,
    /// Identity of the indexed record immediately following this operand.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record immediately following this operand.
    pub(crate) next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignBodyRecipeOperandWire {
    /// Globally unique deterministic identifier for this native operand.
    id: String,
    /// Owning feature scope record.
    scope_record_index: u32,
    /// Exact feature-scope ownership form.
    #[serde(flatten)]
    owner: DesignOperandOwner,
    /// Primary indexed-record identity.
    record_index: u32,
    /// Primary indexed-header byte offset.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the persistent selection namespace.
    asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_selector_tail"
    )]
    selector_tail: Option<NativeBytes<[u8; 4]>>,
    /// Byte offset of the raw selector-tail member.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_selector_tail_offset"
    )]
    selector_tail_offset: Option<u64>,
    /// Counted persistent Design references carried by this operand.
    references: Vec<DesignBodyRecipeReference>,
    /// Tagged nested record reference following the Design reference.
    nested_record_index: u64,
    /// Byte offset of `nested_record_index`.
    nested_record_index_offset: u64,
    /// Body construction recipe contained by this operand record.
    recipe_id: String,
    /// Unique input-state face selected by this operand.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_face_slot"
    )]
    resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_body_state_id"
    )]
    resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_body_slot"
    )]
    resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resolved_body_face_slots: Vec<i64>,
    /// Identity of the indexed record immediately following this operand.
    next_record_index: u32,
    /// Byte offset of the indexed record immediately following this operand.
    next_byte_offset: u64,
}

impl TryFrom<DesignBodyRecipeOperandWire> for DesignBodyRecipeOperand {
    type Error = String;
    fn try_from(wire: DesignBodyRecipeOperandWire) -> Result<Self, Self::Error> {
        Self::try_new(DesignBodyRecipeOperandDraft {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            owner: wire.owner,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id.try_into()?,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id.try_into()?,
            context_id_offset: wire.context_id_offset,
            selector_tail: Located::from_wire(
                wire.selector_tail.map(NativeBytes::into_inner),
                wire.selector_tail_offset,
                "selector_tail",
            )?,
            references: wire.references,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            recipe_id: wire.recipe_id,
            resolved_face_slot: wire.resolved_face_slot,
            resolved_body_state_id: wire.resolved_body_state_id,
            resolved_body_slot: wire.resolved_body_slot,
            resolved_body_face_slots: wire.resolved_body_face_slots,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

#[cfg(test)]
impl From<DesignBodyRecipeOperand> for DesignBodyRecipeOperandWire {
    fn from(record: DesignBodyRecipeOperand) -> Self {
        let record = record.into_draft();
        Self {
            id: record.id,
            scope_record_index: record.scope_record_index,
            owner: record.owner,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag.into(),
            asset_id: record.asset_id.into(),
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id.into(),
            context_id_offset: record.context_id_offset,
            selector_tail: (record.selector_tail.map(|tail| tail.value)).map(Into::into),
            selector_tail_offset: record.selector_tail.map(|tail| tail.offset),
            references: record.references,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            recipe_id: record.recipe_id,
            resolved_face_slot: record.resolved_face_slot,
            resolved_body_state_id: record.resolved_body_state_id,
            resolved_body_slot: record.resolved_body_slot,
            resolved_body_face_slots: record.resolved_body_face_slots,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}

/// Construction-operand group record and member position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct DesignOperandGroup {
    /// Owning construction-operand group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
}

/// Exact owner of a construction operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum DesignOperandOwner {
    /// Operand named by a counted construction-operand group.
    Group {
        /// Owning construction-operand group record.
        group_record_index: u32,
        /// Zero-based position in the group's ordered member run.
        group_member_ordinal: u32,
    },
    /// Standalone operand named directly by the feature scope reference table.
    ScopeReference {
        /// Zero-based position in the scope's ordered reference table.
        scope_reference_ordinal: u32,
    },
}

impl DesignOperandOwner {
    /// Return the construction-group record and member position, when grouped.
    pub(crate) const fn group(self) -> Option<(u32, u32)> {
        match self {
            Self::Group {
                group_record_index,
                group_member_ordinal,
            } => Some((group_record_index, group_member_ordinal)),
            Self::ScopeReference { .. } => None,
        }
    }
}

/// One counted persistent reference inside a whole-body recipe operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignBodyRecipeReference {
    /// Persistent Design reference.
    pub(crate) design_reference: u64,
    /// Byte offset of `design_reference`.
    pub(crate) design_reference_offset: u64,
    /// Reference-local serialized form discriminator.
    pub(crate) form: u32,
    /// Byte offset of `form`.
    pub(crate) form_offset: u64,
    /// Solved faces carrying this reference, ordered by face identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidate_faces: Vec<cadmpeg_ir::ids::FaceId>,
    /// Candidate faces present in the owning feature's input topology.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_candidate_faces: Vec<cadmpeg_ir::ids::FaceId>,
    /// Input-state bodies containing at least one candidate face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_body_slots: Vec<i64>,
}

/// Stable ASM entity family named by a Design persistent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AsmHistoricalEntityKind {
    /// Body topology slot.
    Body,
    /// Region topology slot.
    Region,
    /// Shell topology slot.
    Shell,
    /// Face topology slot.
    Face,
    /// Loop topology slot.
    Loop,
    /// Coedge topology slot.
    Coedge,
    /// Edge topology slot.
    Edge,
    /// Vertex topology slot.
    Vertex,
    /// Point carrier slot.
    Point,
    /// Surface carrier slot.
    Surface,
    /// Curve carrier slot.
    Curve,
    /// Parametric-curve carrier slot.
    Pcurve,
}

/// Mutable binding evidence for one fixed body-recipe reference.
pub(crate) struct DesignBodyRecipeReferenceBindings<'a> {
    pub(crate) design_reference: u64,
    pub(crate) form: u32,
    pub(crate) candidate_faces: &'a mut Vec<FaceId>,
    pub(crate) preceding_candidate_faces: &'a mut Vec<FaceId>,
    pub(crate) preceding_body_slots: &'a mut Vec<i64>,
}

impl DesignBodyRecipeOperand {
    pub(crate) fn reference_bindings_mut(
        &mut self,
    ) -> impl Iterator<Item = DesignBodyRecipeReferenceBindings<'_>> {
        self.references
            .iter_mut()
            .map(|reference| DesignBodyRecipeReferenceBindings {
                design_reference: reference.design_reference,
                form: reference.form,
                candidate_faces: &mut reference.candidate_faces,
                preceding_candidate_faces: &mut reference.preceding_candidate_faces,
                preceding_body_slots: &mut reference.preceding_body_slots,
            })
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_selector_tail,
    cadmpeg_ir::native::bytes::NativeBytes<[u8; 4]>,
    "selector_tail"
);

cadmpeg_core::named_optional_field!(
    deserialize_selector_tail_offset,
    u64,
    "selector_tail_offset"
);

cadmpeg_core::named_optional_field!(deserialize_resolved_face_slot, i64, "resolved_face_slot");

cadmpeg_core::named_optional_field!(
    deserialize_resolved_body_state_id,
    i64,
    "resolved_body_state_id"
);

cadmpeg_core::named_optional_field!(deserialize_resolved_body_slot, i64, "resolved_body_slot");

#[cfg(test)]
mod tests;

mod identity_rewrite;
