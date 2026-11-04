// SPDX-License-Identifier: Apache-2.0
//! Extrude selection groups, operand and face roles, and group members.

use cadmpeg_core::decode::u64_from_index;

use super::fillet::deserialize_historical_binding;
use super::fillet::HistoricalBinding;
use crate::layout::typed_reference_extrude_selection_group_prefix as typed_group;
use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::serde_column::SliceColumn;
use crate::records::sketch_relations::SketchRelationOperand;
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroU32;

/// Longest extended selection-member tail that a following record ends, in bytes.
const EXTENDED_TAIL_MAX: u64 = 1024;

/// Counted selection group owned by an Extrude or Emboss parameter scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignExtrudeSelectionGroupWire")]
pub(crate) struct DesignExtrudeSelectionGroup {
    /// Globally unique deterministic identifier for this native group.
    pub(crate) id: String,
    /// Owning feature parameter-scope record.
    pub(crate) scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub(crate) record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Ordered indexed selection-member records.
    members: Vec<Located<u32>>,
    /// Opaque nonzero u32 repeated around the f64 scalar.
    pub(crate) opaque_index: NonZeroU32,
    /// Opaque scalar between the repeated u32 copies.
    opaque_scalar: cadmpeg_ir::scalar::FiniteReal,
    /// Boolean byte between the two nested-record references; the
    /// typed-reference form stores two zero bytes there and no variant.
    pub(crate) variant: Option<bool>,
    /// Whether every reference uses the 51-byte typed spelling.
    pub(crate) typed_references: bool,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: DesignClassTag,
    offsets: [u64; 4],
}

#[cfg(test)]
thread_local! {
    static EXTRUDE_SELECTION_GROUP_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignExtrudeSelectionGroup {
    fn clone(&self) -> Self {
        #[cfg(test)]
        EXTRUDE_SELECTION_GROUP_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            members: self.members.clone(),
            opaque_index: self.opaque_index,
            opaque_scalar: self.opaque_scalar,
            variant: self.variant,
            typed_references: self.typed_references,
            paired_class_tag: self.paired_class_tag.clone(),
            offsets: self.offsets,
        }
    }
}

impl Serialize for DesignExtrudeSelectionGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            scope_record_index: u32,
            scope_reference_ordinal: u32,
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a str,
            member_count_offset: u64,
            members: SliceColumn<'a, Located<u32>, u32>,
            member_offsets: SliceColumn<'a, Located<u32>, u64>,
            opaque_index: u32,
            opaque_index_offset: u64,
            opaque_scalar: f64,
            opaque_scalar_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            variant: Option<bool>,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            typed_references: bool,
            paired_class_tag: &'a str,
            paired_byte_offset: u64,
        }
        WireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.as_str(),
            member_count_offset: self.member_count_offset(),
            members: SliceColumn::new(&self.members, |member| member.value),
            member_offsets: SliceColumn::new(&self.members, |member| member.offset),
            opaque_index: self.opaque_index.get(),
            opaque_index_offset: self.opaque_index_offset(),
            opaque_scalar: self.opaque_scalar().get(),
            opaque_scalar_offset: self.opaque_scalar_offset(),
            variant: self.variant,
            typed_references: self.typed_references,
            paired_class_tag: self.paired_class_tag.as_str(),
            paired_byte_offset: self.paired_byte_offset(),
        }
        .serialize(serializer)
    }
}

/// Counted selection group owned by an Extrude or Emboss parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignExtrudeSelectionGroupWire {
    /// Globally unique deterministic identifier for this native group.
    pub(crate) id: String,
    /// Owning feature parameter-scope record.
    pub(crate) scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub(crate) record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: String,
    /// Byte offset of the counted member-run length.
    pub(crate) member_count_offset: u64,
    /// Ordered indexed selection-member records.
    pub(crate) members: Vec<u32>,
    /// Byte offsets parallel to `members`.
    pub(crate) member_offsets: Vec<u64>,
    /// Opaque nonzero u32 repeated around the f64 scalar.
    pub(crate) opaque_index: u32,
    /// Byte offset of the first `opaque_index` copy.
    pub(crate) opaque_index_offset: u64,
    /// Opaque finite f64 between the repeated u32 copies.
    pub(crate) opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    pub(crate) opaque_scalar_offset: u64,
    /// Boolean byte between the two nested-record references; absent in the
    /// typed-reference form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_variant"
    )]
    pub(crate) variant: Option<bool>,
    /// Whether every reference uses the 51-byte typed spelling.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) typed_references: bool,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub(crate) paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    pub(crate) paired_byte_offset: u64,
}

impl DesignExtrudeSelectionGroup {
    fn from_wire_inner(wire: DesignExtrudeSelectionGroupWire) -> Result<Self, String> {
        if wire.members.len() != wire.member_offsets.len() {
            return Err("members and member_offsets must have equal lengths".into());
        }
        if wire.typed_references != wire.variant.is_none() {
            return Err("variant is present exactly in the plain-reference form".into());
        }
        let offsets = DesignExtrudeSelectionGroup::offsets(
            wire.byte_offset,
            wire.members.len(),
            wire.typed_references,
        )?;
        let stride = if wire.typed_references { 51 } else { 11 };
        let mut unique = std::collections::HashSet::new();
        for member in &wire.members {
            if unique.contains(member) {
                return Err("members must be distinct".into());
            }
            {}
            unique.insert(member);
        }
        if wire.member_count_offset != offsets[0]
            || wire.opaque_index_offset != offsets[1]
            || wire.opaque_scalar_offset != offsets[2]
            || wire.paired_byte_offset != offsets[3]
        {
            return Err("member_count_offset, opaque_index_offset, opaque_scalar_offset and paired_byte_offset must follow the member run".into());
        }
        if !wire
            .member_offsets
            .iter()
            .enumerate()
            .all(|(index, offset)| *offset == offsets[0] + 5 + u64_from_index(index) * stride)
        {
            return Err(
                "member_offsets must start after member_count_offset and have the reference stride"
                    .into(),
            );
        }
        let opaque_index =
            NonZeroU32::new(wire.opaque_index).ok_or("opaque_index must be nonzero")?;
        let opaque_scalar = cadmpeg_ir::scalar::FiniteReal::new(wire.opaque_scalar)
            .ok_or("opaque_scalar must be finite")?;
        let mut members = Vec::new();
        members.extend(
            wire.members
                .into_iter()
                .zip(wire.member_offsets)
                .map(|(value, offset)| Located { value, offset }),
        );
        Ok(Self {
            members,
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            opaque_index,
            opaque_scalar,
            variant: wire.variant,
            typed_references: wire.typed_references,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            offsets,
        })
    }
}

impl TryFrom<DesignExtrudeSelectionGroupWire> for DesignExtrudeSelectionGroup {
    type Error = String;

    fn try_from(wire: DesignExtrudeSelectionGroupWire) -> Result<Self, Self::Error> {
        Self::from_wire_inner(wire)
    }
}

#[cfg(test)]
impl From<DesignExtrudeSelectionGroup> for DesignExtrudeSelectionGroupWire {
    fn from(group: DesignExtrudeSelectionGroup) -> Self {
        let opaque_scalar = group.opaque_scalar().get();
        let member_count_offset = group.member_count_offset();
        let opaque_index_offset = group.opaque_index_offset();
        let opaque_scalar_offset = group.opaque_scalar_offset();
        let paired_byte_offset = group.paired_byte_offset();
        let (members, member_offsets) = group
            .members
            .into_iter()
            .map(|member| (member.value, member.offset))
            .unzip();
        Self {
            members,
            member_offsets,
            id: group.id,
            scope_record_index: group.scope_record_index,
            scope_reference_ordinal: group.scope_reference_ordinal,
            record_index: group.record_index,
            byte_offset: group.byte_offset,
            class_tag: group.class_tag.into(),
            member_count_offset,
            opaque_index: group.opaque_index.get(),
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset,
            variant: group.variant,
            typed_references: group.typed_references,
            paired_class_tag: group.paired_class_tag.into(),
            paired_byte_offset,
        }
    }
}

impl DesignExtrudeSelectionGroup {
    /// Field offsets of a group with `count` members. Each typed reference is
    /// forty bytes wider: the scope reference before the count, every member,
    /// and the three references after the opaque scalar lane.
    fn offsets(byte_offset: u64, count: usize, typed: bool) -> Result<[u64; 4], String> {
        if count == 0 {
            return Err("members must not be empty".into());
        }
        // The tail runs from the opaque index to the paired header: u32 +
        // f64 + u32, three references, and the separator bytes between them
        // (three typed, four plain with the variant byte).
        let (count_at, stride, separators) = if typed {
            (
                typed_group::MEMBER_COUNT,
                crate::bytes::TYPED_LOCAL_REFERENCE_LEN,
                3,
            )
        } else {
            (32, 11, 4)
        };
        let tail = u64_from_index(16 + 3 * stride + separators);
        let member_count_offset = byte_offset
            .checked_add(u64_from_index(count_at))
            .ok_or("byte_offset overflows member_count_offset")?;
        let run_size = u64::try_from(count)
            .ok()
            .and_then(|count| count.checked_mul(u64_from_index(stride)))
            .ok_or("members exceed offset range")?;
        let opaque_index_offset = member_count_offset
            .checked_add(4)
            .and_then(|offset| offset.checked_add(run_size))
            .ok_or("members overflow opaque_index_offset")?;
        let paired_byte_offset = opaque_index_offset
            .checked_add(tail)
            .ok_or("opaque_index_offset overflows paired_byte_offset")?;
        Ok([
            member_count_offset,
            opaque_index_offset,
            opaque_index_offset + 4,
            paired_byte_offset,
        ])
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn members(&self) -> &[Located<u32>] {
        &self.members
    }
    fn member_count_offset(&self) -> u64 {
        self.offsets[0]
    }
    fn opaque_index_offset(&self) -> u64 {
        self.offsets[1]
    }
    fn opaque_scalar_offset(&self) -> u64 {
        self.offsets[2]
    }
    pub(crate) fn paired_byte_offset(&self) -> u64 {
        self.offsets[3]
    }
    pub(crate) fn opaque_scalar(&self) -> cadmpeg_ir::scalar::FiniteReal {
        self.opaque_scalar
    }
    #[cfg(test)]
    pub(crate) fn try_set_members(&mut self, members: Vec<u32>) -> Result<(), String> {
        let offsets = Self::offsets(self.byte_offset, members.len(), self.typed_references)?;
        let stride = if self.typed_references { 51 } else { 11 };
        let mut wire = DesignExtrudeSelectionGroupWire::from(self.clone());
        wire.member_offsets = (0..members.len())
            .map(|index| offsets[0] + 5 + u64_from_index(index) * stride)
            .collect();
        wire.members = members;
        [
            wire.member_count_offset,
            wire.opaque_index_offset,
            wire.opaque_scalar_offset,
            wire.paired_byte_offset,
        ] = offsets;
        *self = Self::try_from(wire)?;
        Ok(())
    }
}

/// Semantic role of a counted Extrude operand group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignExtrudeOperandRole {
    /// Existing bodies consumed by the Boolean operation.
    Bodies,
    /// Sketch profile swept by the Extrude.
    Profile,
    /// Faces used by profile-start or termination construction. The ordered
    /// position inside the scope always resolves to one of the two uses, so
    /// there is no unresolved face role.
    Faces(DesignExtrudeFaceRole),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DesignExtrudeOperandRoleTag {
    Bodies,
    Profile,
    Faces,
}

/// Semantic use of an ordered Extrude face-operand group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignExtrudeFaceRole {
    /// Face supporting a selected-face start.
    Start,
    /// Face terminating a one-sided to-face extent.
    Termination,
}

/// Source u64 role code carried by a construction-operand group.
///
/// The admitted set is open: files carry codes outside the named list, and
/// `extrude_operand_role` returns `None` for them, so this is a newtype with
/// named codes rather than a closed enum. One spelling per value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct DesignOperandRole(u64);

impl DesignOperandRole {
    /// Extrude body operand run A.
    pub(crate) const BODIES_A: Self = Self(0x0000_0004_0000_0000);
    /// Extrude body operand run B.
    pub(crate) const BODIES_B: Self = Self(0x0000_0008_0000_0000);
    /// Extrude profile operand run.
    pub(crate) const PROFILE: Self = Self(0x0000_0041_0000_0000);
    /// Extrude face operand run.
    pub(crate) const FACES: Self = Self(0x0000_0011_0000_0000);
    /// Pattern seed carried by occurrence paths instead of members.
    pub(crate) const PATH_SEED: Self = Self(0x0000_0100_0000_0000);

    // These codes have scope-dependent meanings and no single semantic name.
    /// Scope-dependent role code 0x5.
    pub(crate) const ROLE_0X5: Self = Self(0x0000_0005_0000_0000);
    /// Scope-dependent role code 0x7.
    pub(crate) const ROLE_0X7: Self = Self(0x0000_0007_0000_0000);
    /// Scope-dependent role code 0x9.
    pub(crate) const ROLE_0X9: Self = Self(0x0000_0009_0000_0000);
    /// Scope-dependent role code 0x10.
    pub(crate) const ROLE_0X10: Self = Self(0x0000_0010_0000_0000);
    /// Scope-dependent role code 0x12.
    pub(crate) const ROLE_0X12: Self = Self(0x0000_0012_0000_0000);
    /// Scope-dependent role code 0x21.
    pub(crate) const ROLE_0X21: Self = Self(0x0000_0021_0000_0000);
    /// Scope-dependent role code 0x43.
    pub(crate) const ROLE_0X43: Self = Self(0x0000_0043_0000_0000);

    /// Wrap the stored u64 role code.
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    /// The stored u64 role code.
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

/// Source encoding of an Extrude face operand run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignExtrudeFaceEncoding {
    /// Standard face-group encoding.
    Faces,
    /// Selected-face start encoding.
    SelectedStart,
    /// Legacy termination-face encoding.
    LegacyTermination,
}

/// One fixed-width member named by an Extrude selection group.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignExtrudeSelectionMemberDraft")]
pub(crate) struct DesignExtrudeSelectionMember {
    frame: crate::records::frame_chain::RecordFrameChain,
    frame_length: u64,
    /// Globally unique deterministic identifier for this native member.
    pub(crate) id: String,
    /// Owning selection-group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Local persistent selection identity preceding the two UUID fields.
    pub(crate) local_id: u64,
    /// Asset UUID qualifying the local selection identity.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// UUID of the local selection-identity context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Whether the fixed tail's optional slot is present.
    #[serde(default)]
    pub(crate) tail_slot_present: bool,
    /// Byte offset of the optional-slot marker.
    #[serde(default)]
    pub(crate) tail_slot_offset: u64,
    /// Fifty-byte scope tail that follows the 190-byte frame, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) scope_tail: Option<DesignExtrudeSelectionScopeTail>,
    /// Sketch geometry carrying `local_id`, when it resolves uniquely in
    /// the selected Sketch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_geometry: Option<SketchRelationOperand>,
    /// Construction-operand identity chains that terminate at this member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) operand_identity_ids: Vec<String>,
    /// Stable ASM history family, entity slot, and states carrying `local_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(flatten, deserialize_with = "deserialize_historical_binding")]
    pub(crate) historical: Option<HistoricalBinding>,
    /// Identity of the indexed record immediately following this member.
    pub(crate) next_record_index: u32,
}

#[cfg(test)]
thread_local! {
    static EXTRUDE_SELECTION_MEMBER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignExtrudeSelectionMember {
    fn clone(&self) -> Self {
        #[cfg(test)]
        EXTRUDE_SELECTION_MEMBER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            frame: self.frame,
            frame_length: self.frame_length,
            id: self.id.clone(),
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            class_tag: self.class_tag.clone(),
            local_id: self.local_id,
            asset_id: self.asset_id.clone(),
            context_id: self.context_id.clone(),
            context_id_offset: self.context_id_offset,
            tail_slot_present: self.tail_slot_present,
            tail_slot_offset: self.tail_slot_offset,
            scope_tail: self.scope_tail,
            resolved_geometry: self.resolved_geometry.clone(),
            operand_identity_ids: self.operand_identity_ids.clone(),
            historical: self.historical.clone(),
            next_record_index: self.next_record_index,
        }
    }
}

impl Serialize for DesignExtrudeSelectionMember {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            group_record_index: u32,
            group_member_ordinal: u32,
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a str,
            local_id: u64,
            local_id_offset: u64,
            asset_id: &'a str,
            asset_id_offset: u64,
            context_id: &'a str,
            context_id_offset: u64,
            tail_slot_present: bool,
            tail_slot_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            scope_tail: Option<&'a DesignExtrudeSelectionScopeTail>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_geometry: Option<&'a SketchRelationOperand>,
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            operand_identity_ids: &'a [String],
            #[serde(flatten, skip_serializing_if = "Option::is_none")]
            historical: Option<&'a HistoricalBinding>,
            next_record_index: u32,
            next_byte_offset: u64,
        }
        WireRef {
            id: &self.id,
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            record_index: self.record_index(),
            byte_offset: self.byte_offset(),
            class_tag: self.class_tag.as_str(),
            local_id: self.local_id,
            local_id_offset: self.local_id_offset(),
            asset_id: self.asset_id.as_str(),
            asset_id_offset: self.asset_id_offset(),
            context_id: self.context_id.as_str(),
            context_id_offset: self.context_id_offset,
            tail_slot_present: self.tail_slot_present,
            tail_slot_offset: self.tail_slot_offset,
            scope_tail: self.scope_tail.as_ref(),
            resolved_geometry: self.resolved_geometry.as_ref(),
            operand_identity_ids: &self.operand_identity_ids,
            historical: self.historical.as_ref(),
            next_record_index: self.next_record_index,
            next_byte_offset: self.next_byte_offset(),
        }
        .serialize(serializer)
    }
}

impl DesignExtrudeSelectionMember {
    pub(crate) fn try_new(draft: DesignExtrudeSelectionMemberDraft) -> Result<Self, String> {
        if draft.context_id_offset <= draft.asset_id_offset {
            return Err("context_id_offset must follow asset_id_offset".into());
        }
        let frame_length = draft
            .next_byte_offset
            .checked_sub(draft.byte_offset)
            .ok_or("next_byte_offset precedes member")?;
        let frame_length_valid = match draft.scope_tail {
            Some(_) => frame_length == 240 && !draft.tail_slot_present,
            None if frame_length == 204 => draft.tail_slot_present,
            None if draft.next_record_index == 0 => frame_length >= 190 && frame_length != 240,
            None => (190..=190 + EXTENDED_TAIL_MAX).contains(&frame_length) && frame_length != 240,
        };
        if !frame_length_valid {
            return Err("selection member has an invalid frame length".into());
        }
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            0,
            frame_length,
        )?;
        let value = Self {
            frame,
            frame_length,
            id: draft.id,
            group_record_index: draft.group_record_index,
            group_member_ordinal: draft.group_member_ordinal,
            class_tag: draft.class_tag,
            local_id: draft.local_id,
            asset_id: draft.asset_id,
            context_id: draft.context_id,
            context_id_offset: draft.context_id_offset,
            tail_slot_present: draft.tail_slot_present,
            tail_slot_offset: draft.tail_slot_offset,
            scope_tail: draft.scope_tail,
            resolved_geometry: draft.resolved_geometry,
            operand_identity_ids: draft.operand_identity_ids,
            historical: draft.historical,
            next_record_index: draft.next_record_index,
        };
        if value.local_id_offset() != draft.local_id_offset {
            return Err("local_id_offset disagrees with frame layout".into());
        }
        if value.asset_id_offset() != draft.asset_id_offset {
            return Err("asset_id_offset disagrees with frame layout".into());
        }
        if value.next_byte_offset() != draft.next_byte_offset {
            return Err("next_byte_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignExtrudeSelectionMemberDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let local_id_offset = self.local_id_offset();
        let asset_id_offset = self.asset_id_offset();
        let next_byte_offset = self.next_byte_offset();
        DesignExtrudeSelectionMemberDraft {
            id: self.id,
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            local_id: self.local_id,
            local_id_offset,
            asset_id: self.asset_id,
            asset_id_offset,
            context_id: self.context_id,
            context_id_offset: self.context_id_offset,
            tail_slot_present: self.tail_slot_present,
            tail_slot_offset: self.tail_slot_offset,
            scope_tail: self.scope_tail,
            resolved_geometry: self.resolved_geometry,
            operand_identity_ids: self.operand_identity_ids,
            historical: self.historical,
            next_record_index: self.next_record_index,
            next_byte_offset,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    fn local_id_offset(&self) -> u64 {
        self.frame.offset(21)
    }
    fn asset_id_offset(&self) -> u64 {
        self.frame.offset(33)
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.frame.offset(self.frame_length)
    }
}

/// Fifty-byte tail of an Extrude selection member whose optional slot is
/// absent. The scalar, the word and the chain references are retained as
/// encoded; the closing reference names the owning Extrude scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignExtrudeSelectionScopeTail {
    /// Raw bits of the little-endian f64 at tail offset 1.
    pub(crate) opaque_scalar_bits: u64,
    /// The u32 at tail offset 9.
    pub(crate) opaque_word: u32,
    /// Record indices of the two marked chain references, in stored order.
    pub(crate) chain_references: [u32; 2],
    /// Record index named by the closing marked scope reference.
    pub(crate) scope_record_index: u32,
}

/// Unadmitted `DesignExtrudeSelectionMember` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignExtrudeSelectionMemberDraft {
    /// Globally unique deterministic identifier for this native member.
    pub(crate) id: String,
    /// Owning selection-group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
    /// Indexed-record identity named by the selection group.
    pub(crate) record_index: u32,
    /// Byte offset of the indexed-record header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Local persistent selection identity preceding the two UUID fields.
    pub(crate) local_id: u64,
    /// Byte offset of `local_id`.
    pub(crate) local_id_offset: u64,
    /// Asset UUID qualifying the local selection identity.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// UUID of the local selection-identity context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub(crate) context_id_offset: u64,
    /// Whether the fixed tail's optional slot is present.
    #[serde(default)]
    pub(crate) tail_slot_present: bool,
    /// Byte offset of the optional-slot marker.
    #[serde(default)]
    pub(crate) tail_slot_offset: u64,
    /// Fifty-byte scope tail that follows the 190-byte frame, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scope_tail"
    )]
    pub(crate) scope_tail: Option<DesignExtrudeSelectionScopeTail>,
    /// Sketch geometry carrying `local_id`, when it resolves uniquely in
    /// the selected Sketch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_geometry"
    )]
    pub(crate) resolved_geometry: Option<SketchRelationOperand>,
    /// Construction-operand identity chains that terminate at this member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) operand_identity_ids: Vec<String>,
    /// Stable ASM history family, entity slot, and states carrying `local_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(flatten, deserialize_with = "deserialize_historical_binding")]
    pub(crate) historical: Option<HistoricalBinding>,
    /// Identity of the indexed record immediately following this member.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record immediately following this member.
    pub(crate) next_byte_offset: u64,
}

impl TryFrom<DesignExtrudeSelectionMemberDraft> for DesignExtrudeSelectionMember {
    type Error = String;
    fn try_from(draft: DesignExtrudeSelectionMemberDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}

#[cfg(test)]
impl From<DesignExtrudeSelectionMember> for DesignExtrudeSelectionMemberDraft {
    fn from(value: DesignExtrudeSelectionMember) -> Self {
        let value = value.into_draft();
        Self {
            id: value.id,
            group_record_index: value.group_record_index,
            group_member_ordinal: value.group_member_ordinal,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag,
            local_id: value.local_id,
            local_id_offset: value.local_id_offset,
            asset_id: value.asset_id,
            asset_id_offset: value.asset_id_offset,
            context_id: value.context_id,
            context_id_offset: value.context_id_offset,
            tail_slot_present: value.tail_slot_present,
            tail_slot_offset: value.tail_slot_offset,
            scope_tail: value.scope_tail,
            resolved_geometry: value.resolved_geometry,
            operand_identity_ids: value.operand_identity_ids,
            historical: value.historical,
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
        }
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_scope_tail,
    DesignExtrudeSelectionScopeTail,
    "scope_tail"
);
cadmpeg_core::named_optional_field!(
    deserialize_resolved_geometry,
    SketchRelationOperand,
    "resolved_geometry"
);
cadmpeg_core::named_optional_field!(deserialize_variant, bool, "variant");

#[cfg(test)]
mod tests;
