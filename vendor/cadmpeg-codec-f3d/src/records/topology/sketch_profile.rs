// SPDX-License-Identifier: Apache-2.0
//! Sketch-profile selection operands and their profile regions.

use crate::records::identity::DesignEntityId;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use serde::Deserialize;
use serde::Serialize;

/// Sketch-profile selection frame named by a profile-based feature scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignSketchProfileOperandWire")]
pub(crate) struct DesignSketchProfileOperand {
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub(crate) record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selected Sketch reference.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    asset_id_offset: u64,
    /// Full Design entity id of the selected Sketch.
    pub(crate) entity_id: DesignEntityId,
    /// Byte offset of the suffix's UTF-16LE code units.
    entity_reference_offset: u64,
    /// Exact nested profile-region selection, when its complete frame closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
}

#[cfg(test)]
thread_local! {
    static SKETCH_PROFILE_OPERAND_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignSketchProfileOperand {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SKETCH_PROFILE_OPERAND_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            asset_id: self.asset_id.clone(),
            asset_id_offset: self.asset_id_offset,
            entity_id: self.entity_id.clone(),
            entity_reference_offset: self.entity_reference_offset,
            region_selection: self.region_selection.clone(),
            paired_class_tag: self.paired_class_tag.clone(),
            paired_byte_offset: self.paired_byte_offset,
        }
    }
}

impl DesignSketchProfileOperand {
    pub(crate) fn try_new(draft: DesignSketchProfileOperandDraft) -> Result<Self, String> {
        if !(draft.byte_offset < draft.asset_id_offset
            && draft.asset_id_offset < draft.entity_reference_offset
            && draft.entity_reference_offset < draft.paired_byte_offset)
        {
            return Err(
                "asset_id_offset/entity_reference_offset/paired_byte_offset must increase".into(),
            );
        }
        let value = Self {
            scope_reference_ordinal: draft.scope_reference_ordinal,
            record_index: draft.record_index,
            byte_offset: draft.byte_offset,
            class_tag: draft.class_tag,
            asset_id: draft.asset_id,
            asset_id_offset: draft.asset_id_offset,
            entity_id: draft.entity_id,
            entity_reference_offset: draft.entity_reference_offset,
            region_selection: draft.region_selection,
            paired_class_tag: draft.paired_class_tag,
            paired_byte_offset: draft.paired_byte_offset,
        };
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignSketchProfileOperandDraft {
        DesignSketchProfileOperandDraft {
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            asset_id_offset: self.asset_id_offset,
            entity_id: self.entity_id,
            entity_reference_offset: self.entity_reference_offset,
            region_selection: self.region_selection,
            paired_class_tag: self.paired_class_tag,
            paired_byte_offset: self.paired_byte_offset,
        }
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn paired_byte_offset(&self) -> u64 {
        self.paired_byte_offset
    }
}

impl Serialize for DesignSketchProfileOperand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            scope_reference_ordinal: u32,
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a str,
            asset_id: &'a str,
            asset_id_offset: u64,
            entity_id: &'a str,
            entity_suffix: u64,
            entity_reference_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            region_selection: Option<&'a DesignSketchProfileRegionSelection>,
            paired_class_tag: &'a str,
            paired_byte_offset: u64,
        }
        WireRef {
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.as_str(),
            asset_id: self.asset_id.as_str(),
            asset_id_offset: self.asset_id_offset,
            entity_id: self.entity_id.as_str(),
            entity_suffix: self.entity_id.suffix(),
            entity_reference_offset: self.entity_reference_offset,
            region_selection: self.region_selection.as_ref(),
            paired_class_tag: self.paired_class_tag.as_str(),
            paired_byte_offset: self.paired_byte_offset,
        }
        .serialize(serializer)
    }
}

/// Unadmitted `DesignSketchProfileOperand` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignSketchProfileOperandDraft {
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub(crate) record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selected Sketch reference.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// Full Design entity id of the selected Sketch.
    pub(crate) entity_id: DesignEntityId,
    /// Byte offset of the suffix's UTF-16LE code units.
    pub(crate) entity_reference_offset: u64,
    /// Exact nested profile-region selection, when its complete frame closes.
    pub(crate) region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub(crate) paired_class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub(crate) paired_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignSketchProfileOperandWire {
    /// Zero-based position in the scope's ordered reference table.
    scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selected Sketch reference.
    asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    asset_id_offset: u64,
    /// Full Design entity id of the selected Sketch.
    entity_id: String,
    /// Numeric suffix stored by the profile frame.
    entity_suffix: u64,
    /// Byte offset of the suffix's UTF-16LE code units.
    entity_reference_offset: u64,
    /// Exact nested profile-region selection, when its complete frame closes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_region_selection"
    )]
    region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
}

impl TryFrom<DesignSketchProfileOperandWire> for DesignSketchProfileOperand {
    type Error = String;

    fn try_from(wire: DesignSketchProfileOperandWire) -> Result<Self, Self::Error> {
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        if entity_id.suffix() != wire.entity_suffix {
            return Err("entity_suffix disagrees with entity_id".into());
        }
        Self::try_new(DesignSketchProfileOperandDraft {
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id.try_into()?,
            asset_id_offset: wire.asset_id_offset,
            entity_id,
            entity_reference_offset: wire.entity_reference_offset,
            region_selection: wire.region_selection,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

#[cfg(test)]
impl From<DesignSketchProfileOperand> for DesignSketchProfileOperandWire {
    fn from(value: DesignSketchProfileOperand) -> Self {
        let value = value.into_draft();
        let entity_suffix = value.entity_id.suffix();
        Self {
            scope_reference_ordinal: value.scope_reference_ordinal,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag.into(),
            asset_id: value.asset_id.into(),
            asset_id_offset: value.asset_id_offset,
            entity_id: value.entity_id.into_string(),
            entity_suffix,
            entity_reference_offset: value.entity_reference_offset,
            region_selection: value.region_selection,
            paired_class_tag: value.paired_class_tag.into(),
            paired_byte_offset: value.paired_byte_offset,
        }
    }
}

/// Nested ordered region selection carried by a sketch-profile operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSketchProfileRegionSelection {
    /// Indexed identity of the region-selection record.
    pub(crate) record_index: u32,
    /// Byte offset of the region-selection indexed header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII region-selection class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of the selected-region count.
    pub(crate) region_count_offset: u64,
    /// Selected regions in source order.
    pub(crate) regions: Vec<DesignSketchProfileRegion>,
    /// Source per-file dynamic three-digit ASCII companion class tag.
    pub(crate) companion_class_tag: DesignClassTag,
    /// Byte offset of the same-index companion header.
    pub(crate) companion_byte_offset: u64,
}

/// One selected region in a nested sketch-profile selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSketchProfileRegion {
    /// Byte offset of this region's member count.
    pub(crate) member_count_offset: u64,
    /// Persistent curve members in source order.
    pub(crate) members: Vec<DesignSketchProfileRegionMember>,
}

impl DesignSketchProfileRegionSelection {
    pub(crate) fn has_extended_incidence(&self) -> bool {
        self.regions
            .iter()
            .flat_map(|region| &region.members)
            .any(|member| {
                member
                    .incidence_values
                    .iter()
                    .any(|value| u32::from(*value) > 2)
            })
    }
}

/// One fixed-width persistent curve member of a selected sketch region.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchProfileRegionMemberWire",
    into = "DesignSketchProfileRegionMemberWire"
)]
pub(crate) struct DesignSketchProfileRegionMember {
    /// Byte offset of the member-kind code.
    pub(crate) kind_offset: u64,
    /// Primary persistent identity of the selected Sketch curve.
    pub(crate) curve_primary_id: std::num::NonZeroU32,
    /// Byte offset of the persistent curve identity.
    pub(crate) curve_primary_id_offset: u64,
    /// First incidence value, encoded as zero or one.
    pub(crate) incidence_flag: bool,
    /// Second and third nonzero incidence words, in source order.
    pub(crate) incidence_values: [DesignRegionIncidence; 2],
    /// Byte offset of the first incidence word.
    pub(crate) incidence_words_offset: u64,
}

/// Nonzero region-incidence word retained from a boundary member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignRegionIncidence(std::num::NonZeroU32);

impl TryFrom<u32> for DesignRegionIncidence {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        std::num::NonZeroU32::new(value)
            .map(Self)
            .ok_or_else(|| "incidence_words second and third values must be nonzero".into())
    }
}

impl From<DesignRegionIncidence> for u32 {
    fn from(value: DesignRegionIncidence) -> Self {
        value.0.get()
    }
}

/// One fixed-width persistent curve member of a selected sketch region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignSketchProfileRegionMemberWire {
    /// Native member-kind code. Profile-region curve members use value three.
    kind: u32,
    /// Byte offset of the member-kind code.
    kind_offset: u64,
    /// Primary persistent identity of the selected Sketch curve.
    curve_primary_id: u64,
    /// Byte offset of the persistent curve identity.
    curve_primary_id_offset: u64,
    /// Structural region-incidence words retained in source order.
    incidence_words: [u32; 8],
    /// Byte offset of the first incidence word.
    incidence_words_offset: u64,
}

impl TryFrom<DesignSketchProfileRegionMemberWire> for DesignSketchProfileRegionMember {
    type Error = String;
    fn try_from(wire: DesignSketchProfileRegionMemberWire) -> Result<Self, Self::Error> {
        if wire.kind != 3 {
            return Err("kind must be 3 for a profile-region curve member".into());
        }
        let curve_primary_id = u32::try_from(wire.curve_primary_id)
            .ok()
            .and_then(std::num::NonZeroU32::new)
            .ok_or("curve_primary_id must be a nonzero u32")?;
        let [0, 0, 0, flag, first, second, 0, 0] = wire.incidence_words else {
            return Err("incidence_words must contain three leading and two trailing zeros".into());
        };
        let incidence_flag = match flag {
            0 => false,
            1 => true,
            _ => return Err("incidence_words first value must be 0 or 1".into()),
        };
        Ok(Self {
            kind_offset: wire.kind_offset,
            curve_primary_id,
            curve_primary_id_offset: wire.curve_primary_id_offset,
            incidence_flag,
            incidence_values: [
                DesignRegionIncidence::try_from(first)?,
                DesignRegionIncidence::try_from(second)?,
            ],
            incidence_words_offset: wire.incidence_words_offset,
        })
    }
}

impl From<DesignSketchProfileRegionMember> for DesignSketchProfileRegionMemberWire {
    fn from(member: DesignSketchProfileRegionMember) -> Self {
        Self {
            kind: 3,
            kind_offset: member.kind_offset,
            curve_primary_id: u64::from(member.curve_primary_id.get()),
            curve_primary_id_offset: member.curve_primary_id_offset,
            incidence_words: [
                0,
                0,
                0,
                u32::from(member.incidence_flag),
                member.incidence_values[0].into(),
                member.incidence_values[1].into(),
                0,
                0,
            ],
            incidence_words_offset: member.incidence_words_offset,
        }
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_region_selection,
    DesignSketchProfileRegionSelection,
    "region_selection"
);

#[cfg(test)]
mod tests;

mod identity_rewrite;
