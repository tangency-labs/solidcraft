// SPDX-License-Identifier: Apache-2.0
//! Edge-identity operands, edge operands and the resolved axis they carry.

use cadmpeg_ir::native::bytes::NativeBytes;

use super::edge_recipe::DesignEdgeRecipeSelectorContext;
use super::edge_recipe::DesignEdgeRecipeStructure;
use super::edge_recipe::DesignSurfacePatchRecipeStructure;
use super::fillet::deserialize_historical_binding;
use super::fillet::HistoricalBinding;
use super::historical_context::DesignEdgeRecipeReferenceContext;
use super::historical_context::DesignHistoricalEdgeContext;
use crate::records::dimensions::DesignRecipeReference;
use crate::records::feature::patterns::DesignAxis;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::ids::FaceId;
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroU32;

/// Prologue framing of a persistent edge-selection identity.
///
/// The three source framings differ only in the length of the zero run before
/// the presence marker, which fixes where every following field sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignEdgeIdentityLayout {
    /// Twelve-zero prologue; the presence marker sits at byte 23.
    Full,
    /// Eleven-zero prologue; the presence marker sits at byte 22.
    Compact,
    /// Ten-zero prologue; the presence marker sits at byte 21.
    Shortest,
}

impl DesignEdgeIdentityLayout {
    /// Byte offset of the presence marker from the indexed-record header.
    pub(crate) fn marker_offset(self) -> u64 {
        match self {
            Self::Full => 23,
            Self::Compact => 22,
            Self::Shortest => 21,
        }
    }

    /// Byte offset of `local_id` from the indexed-record header.
    pub(crate) fn local_id_offset(self) -> u64 {
        self.marker_offset() + 1
    }

    /// The two shortened framings share the on-wire `compact_layout` flag.
    pub(crate) fn is_compact(self) -> bool {
        !matches!(self, Self::Full)
    }

    fn from_local_id_delta(delta: u64) -> Option<Self> {
        [Self::Full, Self::Compact, Self::Shortest]
            .into_iter()
            .find(|layout| layout.local_id_offset() == delta)
    }
}

/// Persistent selection identity owned by a Fillet or Chamfer operand group.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignEdgeIdentityOperandWire")]
pub(crate) struct DesignEdgeIdentityOperand {
    frame: crate::records::frame_chain::RecordFrameChain,
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning parameter-scope record.
    pub(crate) scope_record_index: u32,
    /// Owning construction-operand group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Prologue framing, which fixes `local_id_offset` relative to
    /// `byte_offset`.
    layout: DesignEdgeIdentityLayout,
    /// Local persistent selection identity preceding the two UUID fields.
    pub(crate) local_id: u64,
    /// Asset UUID qualifying the local selection identity.
    asset_id: DesignRelaxedGuidText,
    /// UUID of the local selection-identity context.
    context_id: DesignRelaxedGuidText,
    /// Stable ASM history family, entity slot, and states carrying `local_id`.
    pub(crate) historical: Option<HistoricalBinding>,
    /// Complete radius-qualified deleted source-edge set proved by the owning
    /// feature transition. The transition-scoped set repeats on each operand.
    pub(crate) treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    /// Complete deleted source-edge chain proved by the owning feature
    /// transition. The transition-scoped chain repeats on each operand.
    pub(crate) transition_edge_candidates: Vec<i64>,
    /// Ordered deleted treatment edges selected by an embedded bounded-face
    /// rule owned by this operand.
    pub(crate) resolved_edge_slots: Vec<i64>,
    /// Unique edge slot selected in the owning feature's preceding state.
    pub(crate) resolved_edge_slot: Option<i64>,
    /// Native identity or embedded bounded-face operand proving the resolved
    /// edge selection.
    pub(crate) resolution_identity_id: Option<String>,
}

#[cfg(test)]
thread_local! {
    static EDGE_IDENTITY_OPERAND_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignEdgeIdentityOperand {
    fn clone(&self) -> Self {
        #[cfg(test)]
        EDGE_IDENTITY_OPERAND_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            frame: self.frame,
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            class_tag: self.class_tag.clone(),
            layout: self.layout,
            local_id: self.local_id,
            asset_id: self.asset_id.clone(),
            context_id: self.context_id.clone(),
            historical: self.historical.clone(),
            treatment_radius_candidates: self.treatment_radius_candidates.clone(),
            transition_edge_candidates: self.transition_edge_candidates.clone(),
            resolved_edge_slots: self.resolved_edge_slots.clone(),
            resolved_edge_slot: self.resolved_edge_slot,
            resolution_identity_id: self.resolution_identity_id.clone(),
        }
    }
}

impl Serialize for DesignEdgeIdentityOperand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            scope_record_index: u32,
            group_record_index: u32,
            group_member_ordinal: u32,
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a str,
            compact_layout: bool,
            local_id: u64,
            local_id_offset: u64,
            asset_id: &'a str,
            asset_id_offset: u64,
            context_id: &'a str,
            context_id_offset: u64,
            #[serde(flatten, skip_serializing_if = "Option::is_none")]
            historical: Option<&'a HistoricalBinding>,
            #[serde(skip_serializing_if = "<[DesignEdgeTreatmentRadiusCandidate]>::is_empty")]
            treatment_radius_candidates: &'a [DesignEdgeTreatmentRadiusCandidate],
            #[serde(skip_serializing_if = "<[i64]>::is_empty")]
            transition_edge_candidates: &'a [i64],
            #[serde(skip_serializing_if = "<[i64]>::is_empty")]
            resolved_edge_slots: &'a [i64],
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_edge_slot: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolution_identity_id: Option<&'a str>,
        }
        WireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            record_index: self.record_index(),
            byte_offset: self.byte_offset(),
            class_tag: self.class_tag.as_str(),
            compact_layout: self.layout.is_compact(),
            local_id: self.local_id,
            local_id_offset: self.local_id_offset(),
            asset_id: self.asset_id.as_str(),
            asset_id_offset: self.asset_id_offset(),
            context_id: self.context_id.as_str(),
            context_id_offset: self.context_id_offset(),
            historical: self.historical.as_ref(),
            treatment_radius_candidates: &self.treatment_radius_candidates,
            transition_edge_candidates: &self.transition_edge_candidates,
            resolved_edge_slots: &self.resolved_edge_slots,
            resolved_edge_slot: self.resolved_edge_slot,
            resolution_identity_id: self.resolution_identity_id.as_deref(),
        }
        .serialize(serializer)
    }
}

impl DesignEdgeIdentityOperand {
    pub(crate) fn try_new(draft: DesignEdgeIdentityOperandDraft) -> Result<Self, String> {
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            0,
            draft.layout.local_id_offset() + 94,
        )?;
        let value = Self {
            frame,
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            group_record_index: draft.group_record_index,
            group_member_ordinal: draft.group_member_ordinal,
            class_tag: draft.class_tag,
            layout: draft.layout,
            local_id: draft.local_id,
            asset_id: draft.asset_id,
            context_id: draft.context_id,
            historical: draft.historical,
            treatment_radius_candidates: draft.treatment_radius_candidates,
            transition_edge_candidates: draft.transition_edge_candidates,
            resolved_edge_slots: draft.resolved_edge_slots,
            resolved_edge_slot: draft.resolved_edge_slot,
            resolution_identity_id: draft.resolution_identity_id,
        };
        if value.asset_id_offset() != draft.asset_id_offset {
            return Err("asset_id_offset disagrees with frame layout".into());
        }
        if value.context_id_offset() != draft.context_id_offset {
            return Err("context_id_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignEdgeIdentityOperandDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let asset_id_offset = self.asset_id_offset();
        let context_id_offset = self.context_id_offset();
        DesignEdgeIdentityOperandDraft {
            id: self.id,
            scope_record_index: self.scope_record_index,
            group_record_index: self.group_record_index,
            group_member_ordinal: self.group_member_ordinal,
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            layout: self.layout,
            local_id: self.local_id,
            asset_id: self.asset_id,
            asset_id_offset,
            context_id: self.context_id,
            context_id_offset,
            historical: self.historical,
            treatment_radius_candidates: self.treatment_radius_candidates,
            transition_edge_candidates: self.transition_edge_candidates,
            resolved_edge_slots: self.resolved_edge_slots,
            resolved_edge_slot: self.resolved_edge_slot,
            resolution_identity_id: self.resolution_identity_id,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    pub(crate) fn layout(&self) -> DesignEdgeIdentityLayout {
        self.layout
    }
    pub(crate) fn lineage_key(&self) -> (&str, &str) {
        (self.asset_id.as_str(), self.context_id.as_str())
    }
    fn asset_id_offset(&self) -> u64 {
        self.local_id_offset() + 18
    }
    fn context_id_offset(&self) -> u64 {
        self.local_id_offset() + 94
    }
}

/// Unadmitted `DesignEdgeIdentityOperand` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignEdgeIdentityOperandDraft {
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning parameter-scope record.
    pub(crate) scope_record_index: u32,
    /// Owning construction-operand group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
    /// Indexed-record identity named by the construction group.
    pub(crate) record_index: u32,
    /// Byte offset of the indexed-record header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Prologue framing, which fixes `local_id_offset` relative to
    /// `byte_offset`.
    pub(crate) layout: DesignEdgeIdentityLayout,
    /// Local persistent selection identity preceding the two UUID fields.
    pub(crate) local_id: u64,
    /// Asset UUID qualifying the local selection identity.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// UUID of the local selection-identity context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub(crate) context_id_offset: u64,
    /// Stable ASM history family, entity slot, and states carrying `local_id`.
    pub(crate) historical: Option<HistoricalBinding>,
    /// Complete radius-qualified deleted source-edge set proved by the owning
    /// feature transition. The transition-scoped set repeats on each operand.
    pub(crate) treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    /// Complete deleted source-edge chain proved by the owning feature
    /// transition. The transition-scoped chain repeats on each operand.
    pub(crate) transition_edge_candidates: Vec<i64>,
    /// Ordered deleted treatment edges selected by an embedded bounded-face
    /// rule owned by this operand.
    pub(crate) resolved_edge_slots: Vec<i64>,
    /// Unique edge slot selected in the owning feature's preceding state.
    pub(crate) resolved_edge_slot: Option<i64>,
    /// Native identity or embedded bounded-face operand proving the resolved
    /// edge selection.
    pub(crate) resolution_identity_id: Option<String>,
}

impl DesignEdgeIdentityOperand {
    /// Byte offset of `local_id`, fixed by the prologue framing.
    pub(super) fn local_id_offset(&self) -> u64 {
        self.byte_offset() + self.layout.local_id_offset()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignEdgeIdentityOperandWire {
    id: String,
    scope_record_index: u32,
    group_record_index: u32,
    group_member_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: String,
    #[serde(default)]
    compact_layout: bool,
    local_id: u64,
    local_id_offset: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(flatten, deserialize_with = "deserialize_historical_binding")]
    historical: Option<HistoricalBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transition_edge_candidates: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resolved_edge_slots: Vec<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_edge_slot"
    )]
    resolved_edge_slot: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolution_identity_id"
    )]
    resolution_identity_id: Option<String>,
}

impl TryFrom<DesignEdgeIdentityOperandWire> for DesignEdgeIdentityOperand {
    type Error = String;

    fn try_from(wire: DesignEdgeIdentityOperandWire) -> Result<Self, Self::Error> {
        let layout = wire
            .local_id_offset
            .checked_sub(wire.byte_offset)
            .and_then(DesignEdgeIdentityLayout::from_local_id_delta)
            .ok_or("local_id_offset does not name an edge-identity prologue framing")?;
        if layout.is_compact() != wire.compact_layout {
            return Err(
                "compact_layout disagrees with the framing local_id_offset names".to_owned(),
            );
        }
        Self::try_new(DesignEdgeIdentityOperandDraft {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            group_record_index: wire.group_record_index,
            group_member_ordinal: wire.group_member_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            layout,
            local_id: wire.local_id,
            asset_id: wire.asset_id.try_into()?,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id.try_into()?,
            context_id_offset: wire.context_id_offset,
            historical: wire.historical,
            treatment_radius_candidates: wire.treatment_radius_candidates,
            transition_edge_candidates: wire.transition_edge_candidates,
            resolved_edge_slots: wire.resolved_edge_slots,
            resolved_edge_slot: wire.resolved_edge_slot,
            resolution_identity_id: wire.resolution_identity_id,
        })
    }
}

#[cfg(test)]
impl From<DesignEdgeIdentityOperand> for DesignEdgeIdentityOperandWire {
    fn from(operand: DesignEdgeIdentityOperand) -> Self {
        let local_id_offset = operand.local_id_offset();
        let operand = operand.into_draft();
        Self {
            id: operand.id,
            scope_record_index: operand.scope_record_index,
            group_record_index: operand.group_record_index,
            group_member_ordinal: operand.group_member_ordinal,
            record_index: operand.record_index,
            byte_offset: operand.byte_offset,
            class_tag: operand.class_tag.into(),
            compact_layout: operand.layout.is_compact(),
            local_id: operand.local_id,
            local_id_offset,
            asset_id: operand.asset_id.into(),
            asset_id_offset: operand.asset_id_offset,
            context_id: operand.context_id.into(),
            context_id_offset: operand.context_id_offset,
            historical: operand.historical,
            treatment_radius_candidates: operand.treatment_radius_candidates,
            transition_edge_candidates: operand.transition_edge_candidates,
            resolved_edge_slots: operand.resolved_edge_slots,
            resolved_edge_slot: operand.resolved_edge_slot,
            resolution_identity_id: operand.resolution_identity_id,
        }
    }
}

/// Edge-selection operand owned by an edge-selecting parameter scope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignEdgeOperandDraft")]
pub(crate) struct DesignEdgeOperand {
    #[cfg(test)]
    clone_probe: EdgeOperandCloneProbe,
    frame: crate::records::frame_chain::RecordFrameChain,
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning parameter-scope record.
    pub(crate) scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: DesignClassTag,
    /// Byte offset of the recipe record's indexed header.
    recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub(crate) recipe_id: String,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    pub(crate) recipe_prefix_bytes: NativeBytes<Vec<u8>>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    pub(crate) recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub(crate) recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub(crate) recipe_program: Vec<i32>,
    /// Standard two-side structure decoded from the recipe program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) recipe_structure: Option<DesignEdgeRecipeStructure>,
    /// Alternate two-clause structure decoded from a `SurfacePatch` edge
    /// recipe program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) surface_patch_recipe_structure: Option<DesignSurfacePatchRecipeStructure>,
    /// Ordered local topology references when every nonzero root and side scalar
    /// is a valid prefix-reference ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) local_topology_references: Option<Vec<NonZeroU32>>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology produced by the owning
    /// edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) result_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the result candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) result_boundary_edge_slots: Vec<i64>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_candidate_faces: Vec<FaceId>,
    /// Candidate and effective prefix-reference faces in the terminal topology
    /// used by a suppressed feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) terminal_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the preceding candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_boundary_edge_slots: Vec<i64>,
    /// Stable edge slots on terminal candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) terminal_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted or updated by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) deleted_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots assigned a different record revision by
    /// the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) updated_boundary_edge_slots: Vec<i64>,
    /// Deleted predecessor edges associated with inserted treatment-carrier radii.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    /// Ordered incident-loop topology for every changed boundary edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_boundary_edge_contexts: Vec<DesignHistoricalEdgeContext>,
    /// Ordered incident-loop topology for terminal candidate-face boundaries
    /// used by a suppressed feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) terminal_boundary_edge_contexts: Vec<DesignHistoricalEdgeContext>,
    /// Boundary-edge sets of the prefix-reference faces in the terminal
    /// topology, indexed by zero-based prefix-reference ordinal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) terminal_reference_edge_slots: Vec<Vec<i64>>,
    /// Ordered historical topology context for each prefix reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recipe_reference_contexts: Vec<DesignEdgeRecipeReferenceContext>,
    /// Topology entries grouped by source selector with evaluation-state edge
    /// context matching the selector's incident-loop boundary counts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recipe_selectors: Vec<DesignEdgeRecipeSelectorContext>,
    /// Historical topology state against which the edge recipe was evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) recipe_state_id: Option<i64>,
    /// Stable historical edge slot proven by the selector/reference candidate
    /// intersection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_edge_slot: Option<i64>,
    /// Selected historical carrier axis, when exact.
    #[serde(
        flatten,
        serialize_with = "serialize_edge_resolved_axis",
        deserialize_with = "deserialize_edge_resolved_axis"
    )]
    pub(crate) resolved_axis: Option<DesignAxis>,
    /// Identity of the indexed record following the operand frame.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    next_byte_offset: u64,
}

#[cfg(test)]
thread_local! {
    static EDGE_OPERAND_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct EdgeOperandCloneProbe;

#[cfg(test)]
impl Clone for EdgeOperandCloneProbe {
    fn clone(&self) -> Self {
        EDGE_OPERAND_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self
    }
}

impl Serialize for DesignEdgeOperand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            id: &'a str,
            scope_record_index: u32,
            scope_reference_ordinal: u32,
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a DesignClassTag,
            paired_byte_offset: u64,
            paired_class_tag: &'a DesignClassTag,
            recipe_record_index: u32,
            recipe_record_byte_offset: u64,
            recipe_id: &'a str,
            recipe_prefix_offset: u64,
            recipe_prefix_bytes: NativeBytes<&'a [u8]>,
            recipe_references: &'a Vec<DesignRecipeReference>,
            recipe_program_offset: u64,
            recipe_program: &'a Vec<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            recipe_structure: Option<&'a DesignEdgeRecipeStructure>,
            #[serde(skip_serializing_if = "Option::is_none")]
            surface_patch_recipe_structure: Option<&'a DesignSurfacePatchRecipeStructure>,
            #[serde(skip_serializing_if = "Option::is_none")]
            local_topology_references: Option<&'a Vec<NonZeroU32>>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            candidate_faces: &'a Vec<FaceId>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            result_candidate_faces: &'a Vec<FaceId>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            result_boundary_edge_slots: &'a Vec<i64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            preceding_candidate_faces: &'a Vec<FaceId>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            terminal_candidate_faces: &'a Vec<FaceId>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            changed_candidate_faces: &'a Vec<FaceId>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            preceding_boundary_edge_slots: &'a Vec<i64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            terminal_boundary_edge_slots: &'a Vec<i64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            changed_boundary_edge_slots: &'a Vec<i64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            deleted_boundary_edge_slots: &'a Vec<i64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            updated_boundary_edge_slots: &'a Vec<i64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            treatment_radius_candidates: &'a Vec<DesignEdgeTreatmentRadiusCandidate>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            changed_boundary_edge_contexts: &'a Vec<DesignHistoricalEdgeContext>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            terminal_boundary_edge_contexts: &'a Vec<DesignHistoricalEdgeContext>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            terminal_reference_edge_slots: &'a Vec<Vec<i64>>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            recipe_reference_contexts: &'a Vec<DesignEdgeRecipeReferenceContext>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            recipe_selectors: &'a Vec<DesignEdgeRecipeSelectorContext>,
            #[serde(skip_serializing_if = "Option::is_none")]
            recipe_state_id: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_edge_slot: Option<i64>,
            #[serde(flatten)]
            resolved_axis: EdgeResolvedAxisWire,
            next_record_index: u32,
            next_byte_offset: u64,
        }
        BorrowedWire {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index(),
            byte_offset: self.byte_offset(),
            class_tag: &self.class_tag,
            paired_byte_offset: self.paired_byte_offset,
            paired_class_tag: &self.paired_class_tag,
            recipe_record_index: self.recipe_record_index(),
            recipe_record_byte_offset: self.recipe_record_byte_offset,
            recipe_id: &self.recipe_id,
            recipe_prefix_offset: self.recipe_prefix_offset(),
            recipe_prefix_bytes: (&self.recipe_prefix_bytes).into(),
            recipe_references: &self.recipe_references,
            recipe_program_offset: self.recipe_program_offset,
            recipe_program: &self.recipe_program,
            recipe_structure: self.recipe_structure.as_ref(),
            surface_patch_recipe_structure: self.surface_patch_recipe_structure.as_ref(),
            local_topology_references: self.local_topology_references.as_ref(),
            candidate_faces: &self.candidate_faces,
            result_candidate_faces: &self.result_candidate_faces,
            result_boundary_edge_slots: &self.result_boundary_edge_slots,
            preceding_candidate_faces: &self.preceding_candidate_faces,
            terminal_candidate_faces: &self.terminal_candidate_faces,
            changed_candidate_faces: &self.changed_candidate_faces,
            preceding_boundary_edge_slots: &self.preceding_boundary_edge_slots,
            terminal_boundary_edge_slots: &self.terminal_boundary_edge_slots,
            changed_boundary_edge_slots: &self.changed_boundary_edge_slots,
            deleted_boundary_edge_slots: &self.deleted_boundary_edge_slots,
            updated_boundary_edge_slots: &self.updated_boundary_edge_slots,
            treatment_radius_candidates: &self.treatment_radius_candidates,
            changed_boundary_edge_contexts: &self.changed_boundary_edge_contexts,
            terminal_boundary_edge_contexts: &self.terminal_boundary_edge_contexts,
            terminal_reference_edge_slots: &self.terminal_reference_edge_slots,
            recipe_reference_contexts: &self.recipe_reference_contexts,
            recipe_selectors: &self.recipe_selectors,
            recipe_state_id: self.recipe_state_id,
            resolved_edge_slot: self.resolved_edge_slot,
            resolved_axis: EdgeResolvedAxisWire::from_axis(self.resolved_axis.as_ref()),
            next_record_index: self.next_record_index,
            next_byte_offset: self.next_byte_offset,
        }
        .serialize(serializer)
    }
}

impl DesignEdgeOperand {
    pub(crate) fn try_new(draft: DesignEdgeOperandDraft) -> Result<Self, String> {
        if !(draft.byte_offset < draft.paired_byte_offset
            && draft.paired_byte_offset < draft.recipe_record_byte_offset
            && draft.recipe_record_byte_offset < draft.next_byte_offset)
        {
            return Err(
                "paired_byte_offset/recipe_record_byte_offset/next_byte_offset must increase"
                    .into(),
            );
        }
        draft
            .recipe_record_byte_offset
            .checked_add(11)
            .ok_or("recipe_prefix_offset overflows")?;
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            3,
            0,
        )?;
        let value = Self {
            #[cfg(test)]
            clone_probe: EdgeOperandCloneProbe,
            frame,
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            scope_reference_ordinal: draft.scope_reference_ordinal,
            class_tag: draft.class_tag,
            paired_byte_offset: draft.paired_byte_offset,
            paired_class_tag: draft.paired_class_tag,
            recipe_record_byte_offset: draft.recipe_record_byte_offset,
            recipe_id: draft.recipe_id,
            recipe_prefix_bytes: (draft.recipe_prefix_bytes),
            recipe_references: draft.recipe_references,
            recipe_program_offset: draft.recipe_program_offset,
            recipe_program: draft.recipe_program,
            recipe_structure: draft.recipe_structure,
            surface_patch_recipe_structure: draft.surface_patch_recipe_structure,
            local_topology_references: draft.local_topology_references,
            candidate_faces: draft.candidate_faces,
            result_candidate_faces: draft.result_candidate_faces,
            result_boundary_edge_slots: draft.result_boundary_edge_slots,
            preceding_candidate_faces: draft.preceding_candidate_faces,
            terminal_candidate_faces: draft.terminal_candidate_faces,
            changed_candidate_faces: draft.changed_candidate_faces,
            preceding_boundary_edge_slots: draft.preceding_boundary_edge_slots,
            terminal_boundary_edge_slots: draft.terminal_boundary_edge_slots,
            changed_boundary_edge_slots: draft.changed_boundary_edge_slots,
            deleted_boundary_edge_slots: draft.deleted_boundary_edge_slots,
            updated_boundary_edge_slots: draft.updated_boundary_edge_slots,
            treatment_radius_candidates: draft.treatment_radius_candidates,
            changed_boundary_edge_contexts: draft.changed_boundary_edge_contexts,
            terminal_boundary_edge_contexts: draft.terminal_boundary_edge_contexts,
            terminal_reference_edge_slots: draft.terminal_reference_edge_slots,
            recipe_reference_contexts: draft.recipe_reference_contexts,
            recipe_selectors: draft.recipe_selectors,
            recipe_state_id: draft.recipe_state_id,
            resolved_edge_slot: draft.resolved_edge_slot,
            resolved_axis: draft.resolved_axis,
            next_record_index: draft.next_record_index,
            next_byte_offset: draft.next_byte_offset,
        };
        if value.recipe_record_index() != draft.recipe_record_index {
            return Err("recipe_record_index disagrees with frame layout".into());
        }
        if value.recipe_prefix_offset() != draft.recipe_prefix_offset {
            return Err("recipe_prefix_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignEdgeOperandDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let recipe_record_index = self.recipe_record_index();
        let recipe_prefix_offset = self.recipe_prefix_offset();
        DesignEdgeOperandDraft {
            id: self.id,
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            paired_byte_offset: self.paired_byte_offset,
            paired_class_tag: self.paired_class_tag,
            recipe_record_index,
            recipe_record_byte_offset: self.recipe_record_byte_offset,
            recipe_id: self.recipe_id,
            recipe_prefix_offset,
            recipe_prefix_bytes: (self.recipe_prefix_bytes),
            recipe_references: self.recipe_references,
            recipe_program_offset: self.recipe_program_offset,
            recipe_program: self.recipe_program,
            recipe_structure: self.recipe_structure,
            surface_patch_recipe_structure: self.surface_patch_recipe_structure,
            local_topology_references: self.local_topology_references,
            candidate_faces: self.candidate_faces,
            result_candidate_faces: self.result_candidate_faces,
            result_boundary_edge_slots: self.result_boundary_edge_slots,
            preceding_candidate_faces: self.preceding_candidate_faces,
            terminal_candidate_faces: self.terminal_candidate_faces,
            changed_candidate_faces: self.changed_candidate_faces,
            preceding_boundary_edge_slots: self.preceding_boundary_edge_slots,
            terminal_boundary_edge_slots: self.terminal_boundary_edge_slots,
            changed_boundary_edge_slots: self.changed_boundary_edge_slots,
            deleted_boundary_edge_slots: self.deleted_boundary_edge_slots,
            updated_boundary_edge_slots: self.updated_boundary_edge_slots,
            treatment_radius_candidates: self.treatment_radius_candidates,
            changed_boundary_edge_contexts: self.changed_boundary_edge_contexts,
            terminal_boundary_edge_contexts: self.terminal_boundary_edge_contexts,
            terminal_reference_edge_slots: self.terminal_reference_edge_slots,
            recipe_reference_contexts: self.recipe_reference_contexts,
            recipe_selectors: self.recipe_selectors,
            recipe_state_id: self.recipe_state_id,
            resolved_edge_slot: self.resolved_edge_slot,
            resolved_axis: self.resolved_axis,
            next_record_index: self.next_record_index,
            next_byte_offset: self.next_byte_offset,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    pub(crate) fn recipe_record_index(&self) -> u32 {
        self.frame.index(3)
    }
    pub(crate) fn recipe_record_byte_offset(&self) -> u64 {
        self.recipe_record_byte_offset
    }
    pub(crate) fn recipe_prefix_offset(&self) -> u64 {
        self.recipe_record_byte_offset + 11
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.next_byte_offset
    }
}

/// Unadmitted `DesignEdgeOperand` fields.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(crate) struct DesignEdgeOperandDraft {
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning parameter-scope record.
    pub(crate) scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub(crate) record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub(crate) paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub(crate) paired_class_tag: DesignClassTag,
    /// Indexed record containing the edge regeneration recipe.
    pub(crate) recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub(crate) recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub(crate) recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub(crate) recipe_prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    pub(crate) recipe_prefix_bytes: NativeBytes<Vec<u8>>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    pub(crate) recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub(crate) recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub(crate) recipe_program: Vec<i32>,
    /// Standard two-side structure decoded from the recipe program.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_recipe_structure"
    )]
    pub(crate) recipe_structure: Option<DesignEdgeRecipeStructure>,
    /// Alternate two-clause structure decoded from a `SurfacePatch` edge
    /// recipe program.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_surface_patch_recipe_structure"
    )]
    pub(crate) surface_patch_recipe_structure: Option<DesignSurfacePatchRecipeStructure>,
    /// Ordered local topology references when every nonzero root and side scalar
    /// is a valid prefix-reference ordinal.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_local_topology_references"
    )]
    pub(crate) local_topology_references: Option<Vec<NonZeroU32>>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology produced by the owning
    /// edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) result_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the result candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) result_boundary_edge_slots: Vec<i64>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_candidate_faces: Vec<FaceId>,
    /// Candidate and effective prefix-reference faces in the terminal topology
    /// used by a suppressed feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) terminal_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the preceding candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_boundary_edge_slots: Vec<i64>,
    /// Stable edge slots on terminal candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) terminal_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted or updated by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) deleted_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots assigned a different record revision by
    /// the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) updated_boundary_edge_slots: Vec<i64>,
    /// Deleted predecessor edges associated with inserted treatment-carrier radii.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    /// Ordered incident-loop topology for every changed boundary edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_boundary_edge_contexts: Vec<DesignHistoricalEdgeContext>,
    /// Ordered incident-loop topology for terminal candidate-face boundaries
    /// used by a suppressed feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) terminal_boundary_edge_contexts: Vec<DesignHistoricalEdgeContext>,
    /// Boundary-edge sets of the prefix-reference faces in the terminal
    /// topology, indexed by zero-based prefix-reference ordinal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) terminal_reference_edge_slots: Vec<Vec<i64>>,
    /// Ordered historical topology context for each prefix reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recipe_reference_contexts: Vec<DesignEdgeRecipeReferenceContext>,
    /// Topology entries grouped by source selector with evaluation-state edge
    /// context matching the selector's incident-loop boundary counts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recipe_selectors: Vec<DesignEdgeRecipeSelectorContext>,
    /// Historical topology state against which the edge recipe was evaluated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_recipe_state_id"
    )]
    pub(crate) recipe_state_id: Option<i64>,
    /// Stable historical edge slot proven by the selector/reference candidate
    /// intersection.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_edge_slot"
    )]
    pub(crate) resolved_edge_slot: Option<i64>,
    /// Selected historical carrier axis, when exact.
    #[serde(
        flatten,
        serialize_with = "serialize_edge_resolved_axis",
        deserialize_with = "deserialize_edge_resolved_axis"
    )]
    pub(crate) resolved_axis: Option<DesignAxis>,
    /// Identity of the indexed record following the operand frame.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    pub(crate) next_byte_offset: u64,
}

impl TryFrom<DesignEdgeOperandDraft> for DesignEdgeOperand {
    type Error = String;
    fn try_from(draft: DesignEdgeOperandDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}

#[cfg(test)]
impl From<DesignEdgeOperand> for DesignEdgeOperandDraft {
    fn from(value: DesignEdgeOperand) -> Self {
        let value = value.into_draft();
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            scope_reference_ordinal: value.scope_reference_ordinal,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag,
            paired_byte_offset: value.paired_byte_offset,
            paired_class_tag: value.paired_class_tag,
            recipe_record_index: value.recipe_record_index,
            recipe_record_byte_offset: value.recipe_record_byte_offset,
            recipe_id: value.recipe_id,
            recipe_prefix_offset: value.recipe_prefix_offset,
            recipe_prefix_bytes: (value.recipe_prefix_bytes),
            recipe_references: value.recipe_references,
            recipe_program_offset: value.recipe_program_offset,
            recipe_program: value.recipe_program,
            recipe_structure: value.recipe_structure,
            surface_patch_recipe_structure: value.surface_patch_recipe_structure,
            local_topology_references: value.local_topology_references,
            candidate_faces: value.candidate_faces,
            result_candidate_faces: value.result_candidate_faces,
            result_boundary_edge_slots: value.result_boundary_edge_slots,
            preceding_candidate_faces: value.preceding_candidate_faces,
            terminal_candidate_faces: value.terminal_candidate_faces,
            changed_candidate_faces: value.changed_candidate_faces,
            preceding_boundary_edge_slots: value.preceding_boundary_edge_slots,
            terminal_boundary_edge_slots: value.terminal_boundary_edge_slots,
            changed_boundary_edge_slots: value.changed_boundary_edge_slots,
            deleted_boundary_edge_slots: value.deleted_boundary_edge_slots,
            updated_boundary_edge_slots: value.updated_boundary_edge_slots,
            treatment_radius_candidates: value.treatment_radius_candidates,
            changed_boundary_edge_contexts: value.changed_boundary_edge_contexts,
            terminal_boundary_edge_contexts: value.terminal_boundary_edge_contexts,
            terminal_reference_edge_slots: value.terminal_reference_edge_slots,
            recipe_reference_contexts: value.recipe_reference_contexts,
            recipe_selectors: value.recipe_selectors,
            recipe_state_id: value.recipe_state_id,
            resolved_edge_slot: value.resolved_edge_slot,
            resolved_axis: value.resolved_axis,
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct EdgeResolvedAxisWire {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_axis_origin"
    )]
    resolved_axis_origin: Option<FinitePoint3>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_axis_direction"
    )]
    resolved_axis_direction: Option<FiniteVector3>,
}

impl EdgeResolvedAxisWire {
    fn from_axis(axis: Option<&DesignAxis>) -> Self {
        Self {
            resolved_axis_origin: axis.map(|axis| axis.origin),
            resolved_axis_direction: axis.map(|axis| FiniteVector3::from(axis.direction)),
        }
    }
}

// The wire adapter receives the optional field by reference, including its absence.
#[cfg(test)]
#[allow(clippy::ref_option)]
fn serialize_edge_resolved_axis<S: serde::Serializer>(
    axis: &Option<DesignAxis>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    EdgeResolvedAxisWire {
        resolved_axis_origin: axis.map(|axis| axis.origin),
        resolved_axis_direction: axis.map(|axis| FiniteVector3::from(axis.direction)),
    }
    .serialize(serializer)
}

fn deserialize_edge_resolved_axis<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DesignAxis>, D::Error> {
    let wire = EdgeResolvedAxisWire::deserialize(deserializer)?;
    match (wire.resolved_axis_origin, wire.resolved_axis_direction) {
        (None, None) => Ok(None),
        (Some(origin), Some(direction)) => Ok(Some(
            DesignAxis::from_parts(origin, direction)
                .ok_or_else(|| serde::de::Error::custom("resolved axis direction is zero"))?,
        )),
        _ => Err(serde::de::Error::custom(
            "resolved_axis_origin and resolved_axis_direction must occur together",
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// One radius-qualified historical edge candidate recovered from an inserted
/// treatment face and its carrier-stable adjacent supports.
pub(crate) struct DesignEdgeTreatmentRadiusCandidate {
    /// Deleted stable edge slot shared by the preceding support faces.
    pub(crate) edge_slot: i64,
    /// Characteristic radius of the inserted treatment carrier.
    pub(crate) radius: cadmpeg_ir::scalar::PositiveReal,
}

cadmpeg_core::named_optional_field!(pub(super) deserialize_resolved_edge_slot, i64, "resolved_edge_slot");

cadmpeg_core::named_optional_field!(
    deserialize_resolution_identity_id,
    String,
    "resolution_identity_id"
);

cadmpeg_core::named_optional_field!(
    deserialize_recipe_structure,
    DesignEdgeRecipeStructure,
    "recipe_structure"
);

cadmpeg_core::named_optional_field!(
    deserialize_surface_patch_recipe_structure,
    DesignSurfacePatchRecipeStructure,
    "surface_patch_recipe_structure"
);

cadmpeg_core::named_optional_field!(
    deserialize_local_topology_references,
    Vec<NonZeroU32>,
    "local_topology_references"
);

cadmpeg_core::named_optional_field!(deserialize_recipe_state_id, i64, "recipe_state_id");

cadmpeg_core::named_optional_field!(
    deserialize_resolved_axis_origin,
    FinitePoint3,
    "resolved_axis_origin"
);

cadmpeg_core::named_optional_field!(
    deserialize_resolved_axis_direction,
    FiniteVector3,
    "resolved_axis_direction"
);

#[cfg(test)]
mod tests;

mod identity_rewrite;
