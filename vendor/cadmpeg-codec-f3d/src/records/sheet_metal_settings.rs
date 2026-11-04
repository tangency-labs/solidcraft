// SPDX-License-Identifier: Apache-2.0
//! Common settings carriers shared by sheet-metal feature families.

use super::identity::NativeRecordId;
use serde::{Deserialize, Serialize};

pub(crate) const SETTINGS_GUID: &str = "E75C86D6-76D8-4F1F-BC16-AB8CB74958B0";
pub(crate) const RULE_GUID: &str = "B42ED662-8999-4C1B-A744-759D1D446D55";
pub(crate) const PRESENTATION_GUID: &str = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";
pub(crate) const PAIRED_GUID: &str = "CB844AB6-240D-4FC9-9C9F-3679DC896D6F";

/// The version fixes the number of optional input references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "version", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DesignSheetMetalSettingsInputs {
    V2 {
        slots: [Option<u64>; 7],
    },
    V3 {
        slots: [Option<u64>; 9],
        rule_reference_tail: V3RuleReferenceTail,
    },
}

/// Exact suffix bytes; their control meaning is unassigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum V3RuleReferenceTail {
    #[serde(rename = "0000")]
    ZeroPair,
    #[serde(rename = "010000")]
    OneZeroPair,
}

impl DesignSheetMetalSettingsInputs {
    pub(crate) fn version(&self) -> u32 {
        match self {
            Self::V2 { .. } => 2,
            Self::V3 { .. } => 3,
        }
    }
    pub(crate) fn slots(&self) -> &[Option<u64>] {
        match self {
            Self::V2 { slots } => slots,
            Self::V3 { slots, .. } => slots,
        }
    }
    pub(crate) fn rule_reference_tail_width(&self) -> u64 {
        match self {
            Self::V2 { .. } => 4,
            Self::V3 {
                rule_reference_tail: V3RuleReferenceTail::ZeroPair,
                ..
            } => 2,
            Self::V3 {
                rule_reference_tail: V3RuleReferenceTail::OneZeroPair,
                ..
            } => 3,
        }
    }
}

/// An exact common settings frame. Unassigned slots and mask bits stay native.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignSheetMetalSettingsWire")]
pub(crate) struct DesignSheetMetalSettings {
    id: NativeRecordId,
    pub(crate) record_index: u64,
    pub(crate) byte_offset: u64,
    pub(crate) paired_byte_offset: u64,
    pub(crate) mask: u32,
    pub(crate) inputs: DesignSheetMetalSettingsInputs,
    pub(crate) rule_record_index: u64,
    pub(crate) presentation_record_index: u64,
    pub(crate) owner_record_index: u64,
    pub(crate) context_record_index: u64,
    /// The rule record's sheet thickness in centimetres, when its leading
    /// entry is the `Thickness` value.
    pub(crate) rule_thickness: Option<cadmpeg_ir::scalar::PositiveReal>,
    /// Byte offset of `rule_thickness`.
    pub(crate) rule_thickness_offset: Option<u64>,
}

cadmpeg_core::named_optional_field!(
    deserialize_rule_thickness,
    cadmpeg_ir::scalar::PositiveReal,
    "rule_thickness"
);
cadmpeg_core::named_optional_field!(
    deserialize_rule_thickness_offset,
    u64,
    "rule_thickness_offset"
);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSheetMetalSettingsWire {
    pub(crate) id: String,
    pub(crate) record_index: u64,
    pub(crate) byte_offset: u64,
    pub(crate) paired_byte_offset: u64,
    pub(crate) mask: u32,
    pub(crate) inputs: DesignSheetMetalSettingsInputs,
    pub(crate) rule_record_index: u64,
    pub(crate) presentation_record_index: u64,
    pub(crate) owner_record_index: u64,
    pub(crate) context_record_index: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rule_thickness"
    )]
    pub(crate) rule_thickness: Option<cadmpeg_ir::scalar::PositiveReal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rule_thickness_offset"
    )]
    pub(crate) rule_thickness_offset: Option<u64>,
}

impl DesignSheetMetalSettings {
    pub(crate) fn id(&self) -> &str {
        self.id.text()
    }
    pub(crate) fn record_references(&self) -> impl Iterator<Item = u64> + '_ {
        std::iter::once(self.record_index)
            .chain(self.inputs.slots().iter().flatten().copied())
            .chain([
                self.rule_record_index,
                self.presentation_record_index,
                self.owner_record_index,
                self.context_record_index,
            ])
    }
}

impl TryFrom<DesignSheetMetalSettingsWire> for DesignSheetMetalSettings {
    type Error = String;
    fn try_from(wire: DesignSheetMetalSettingsWire) -> Result<Self, Self::Error> {
        if wire.paired_byte_offset <= wire.byte_offset {
            return Err("settings paired header must follow the primary header".into());
        }
        let id =
            NativeRecordId::try_f3d_new(wire.id, "design-sheet-metal-settings", wire.byte_offset)?;
        if !id.stream().ends_with("/BulkStream.dat") {
            return Err("settings identity must name its containing BulkStream".into());
        }
        Ok(Self {
            id,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            paired_byte_offset: wire.paired_byte_offset,
            mask: wire.mask,
            inputs: wire.inputs,
            rule_record_index: wire.rule_record_index,
            presentation_record_index: wire.presentation_record_index,
            owner_record_index: wire.owner_record_index,
            context_record_index: wire.context_record_index,
            rule_thickness: wire.rule_thickness,
            rule_thickness_offset: wire.rule_thickness_offset,
        })
    }
}
#[derive(Serialize)]
struct DesignSheetMetalSettingsWireRef<'a> {
    id: &'a str,
    record_index: u64,
    byte_offset: u64,
    paired_byte_offset: u64,
    mask: u32,
    inputs: &'a DesignSheetMetalSettingsInputs,
    rule_record_index: u64,
    presentation_record_index: u64,
    owner_record_index: u64,
    context_record_index: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule_thickness: Option<cadmpeg_ir::scalar::PositiveReal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule_thickness_offset: Option<u64>,
}
impl Serialize for DesignSheetMetalSettings {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignSheetMetalSettingsWireRef {
            id: self.id(),
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            paired_byte_offset: self.paired_byte_offset,
            mask: self.mask,
            inputs: &self.inputs,
            rule_record_index: self.rule_record_index,
            presentation_record_index: self.presentation_record_index,
            owner_record_index: self.owner_record_index,
            context_record_index: self.context_record_index,
            rule_thickness: self.rule_thickness,
            rule_thickness_offset: self.rule_thickness_offset,
        }
        .serialize(serializer)
    }
}
rewrite_native_record!(DesignSheetMetalSettings, []; {id, record_index, byte_offset, paired_byte_offset, mask, inputs, rule_record_index, presentation_record_index, owner_record_index, context_record_index, rule_thickness, rule_thickness_offset});
rewrite_native_scalar!(DesignSheetMetalSettingsInputs);

#[cfg(test)]
mod tests;
