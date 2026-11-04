// SPDX-License-Identifier: Apache-2.0
//! Combine operations, their tool bodies and the external body identity they may name.

use cadmpeg_core::decode::u64_from_index;

use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use serde::{Deserialize, Deserializer, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_external_identity,
    DesignCombineExternalBodyIdentity,
    "external_identity"
);
cadmpeg_core::named_optional_field!(deserialize_body_node, DesignCombineBodyNode, "body_node");
cadmpeg_core::named_optional_field!(
    deserialize_external_property_key,
    DesignRelaxedGuidText,
    "external_property_key"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_property_key_offset,
    u64,
    "external_property_key_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_version_urn,
    String,
    "external_version_urn"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_version_urn_offset,
    u64,
    "external_version_urn_offset"
);
/// Serialized prologue form of a `Combine` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCombineForm {
    /// Nine zero bytes followed by the operation at offset 20.
    Standard,
    /// Class-387 form with the operation at offset 21.
    Compact,
    /// Eighteen-zero reference form with the operation at offset 31.
    ExtendedReference,
    /// Counted operand-list form with the zero-based operation at offset 21.
    OperandLists,
}

/// Version identity carried by a cross-document reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignExternalVersion {
    pub(crate) property_key: Located<DesignRelaxedGuidText>,
    pub(crate) version_urn: Located<String>,
}

/// Cross-document persistent body identity carried by a `Combine` tool selector.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignCombineExternalBodyIdentityWire")]
pub(crate) struct DesignCombineExternalBodyIdentity {
    /// Asset GUID of the enclosing body selector.
    selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    external_segment: u32,
    /// Byte offset of `external_segment`.
    external_segment_offset: u64,
    /// Byte offset of `external_asset_id`.
    external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    external_link_name: String,
    /// Byte offset of `external_link_name`.
    external_link_name_offset: u64,
    /// Located property key and referenced-document version identity.
    external_version: Option<DesignExternalVersion>,
    /// Raw selection stamp and binary64 evaluation-scalar bits, separated by the epoch.
    #[serde(default)]
    tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    tail_value_offsets: [u64; 2],
}

#[cfg(test)]
thread_local! {
    static COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCombineExternalBodyIdentity {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            selector_asset_id: self.selector_asset_id.clone(),
            selector_asset_id_offset: self.selector_asset_id_offset,
            selector_context_id: self.selector_context_id.clone(),
            selector_context_id_offset: self.selector_context_id_offset,
            occurrence_reference: self.occurrence_reference,
            occurrence_reference_offset: self.occurrence_reference_offset,
            external_body_reference: self.external_body_reference,
            external_body_reference_offset: self.external_body_reference_offset,
            external_segment: self.external_segment,
            external_segment_offset: self.external_segment_offset,
            external_asset_id_offset: self.external_asset_id_offset,
            external_link_name: self.external_link_name.clone(),
            external_link_name_offset: self.external_link_name_offset,
            external_version: self.external_version.clone(),
            tail_values: self.tail_values,
            tail_value_offsets: self.tail_value_offsets,
        }
    }
}

#[derive(Serialize)]
struct DesignCombineExternalBodyIdentityRef<'a> {
    selector_asset_id: &'a str,
    selector_asset_id_offset: u64,
    selector_context_id: &'a str,
    selector_context_id_offset: u64,
    occurrence_reference: u64,
    occurrence_reference_offset: u64,
    external_body_reference: u64,
    external_body_reference_offset: u64,
    external_segment: u32,
    external_segment_offset: u64,
    external_asset_id: &'a str,
    external_asset_id_offset: u64,
    external_link_name: &'a str,
    external_link_name_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_property_key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_property_key_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_version_urn: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_version_urn_offset: Option<u64>,
    tail_values: [u64; 2],
    tail_value_offsets: [u64; 2],
}

impl Serialize for DesignCombineExternalBodyIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCombineExternalBodyIdentityRef {
            selector_asset_id: self.selector_asset_id.as_str(),
            selector_asset_id_offset: self.selector_asset_id_offset,
            selector_context_id: self.selector_context_id.as_str(),
            selector_context_id_offset: self.selector_context_id_offset,
            occurrence_reference: self.occurrence_reference,
            occurrence_reference_offset: self.occurrence_reference_offset,
            external_body_reference: self.external_body_reference,
            external_body_reference_offset: self.external_body_reference_offset,
            external_segment: self.external_segment,
            external_segment_offset: self.external_segment_offset,
            external_asset_id: self.selector_asset_id.as_str(),
            external_asset_id_offset: self.external_asset_id_offset,
            external_link_name: &self.external_link_name,
            external_link_name_offset: self.external_link_name_offset,
            external_property_key: self
                .external_version
                .as_ref()
                .map(|version| version.property_key.value.as_str()),
            external_property_key_offset: self
                .external_version
                .as_ref()
                .map(|version| version.property_key.offset),
            external_version_urn: self
                .external_version
                .as_ref()
                .map(|version| version.version_urn.value.as_str()),
            external_version_urn_offset: self
                .external_version
                .as_ref()
                .map(|version| version.version_urn.offset),
            tail_values: self.tail_values,
            tail_value_offsets: self.tail_value_offsets,
        }
        .serialize(serializer)
    }
}

/// Wire fields for an external Combine body identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignCombineExternalBodyIdentityWire {
    /// Asset GUID of the enclosing body selector.
    pub(crate) selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    pub(crate) selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    pub(crate) selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    pub(crate) selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    pub(crate) occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    pub(crate) occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    pub(crate) external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    pub(crate) external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    pub(crate) external_segment: u32,
    /// Byte offset of `external_segment`.
    pub(crate) external_segment_offset: u64,
    /// Asset GUID carried by the cross-document body reference.
    pub(crate) external_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `external_asset_id`.
    pub(crate) external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    pub(crate) external_link_name: String,
    /// Byte offset of `external_link_name`.
    pub(crate) external_link_name_offset: u64,
    /// Optional property key preceding the version identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_property_key"
    )]
    pub(crate) external_property_key: Option<DesignRelaxedGuidText>,
    /// Byte offset of `external_property_key` when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_property_key_offset"
    )]
    pub(crate) external_property_key_offset: Option<u64>,
    /// Optional referenced-document version identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_version_urn"
    )]
    pub(crate) external_version_urn: Option<String>,
    /// Byte offset of `external_version_urn` when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_version_urn_offset"
    )]
    pub(crate) external_version_urn_offset: Option<u64>,
    /// Raw selection stamp and binary64 evaluation-scalar bits, separated by the epoch.
    #[serde(default)]
    pub(crate) tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    pub(crate) tail_value_offsets: [u64; 2],
}

impl DesignCombineExternalBodyIdentity {
    pub(crate) fn selector_asset_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_asset_id
    }
    pub(crate) fn selector_asset_id_offset(&self) -> u64 {
        self.selector_asset_id_offset
    }
    pub(crate) fn selector_context_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_context_id
    }
    pub(crate) fn occurrence_reference(&self) -> u64 {
        self.occurrence_reference
    }
    pub(crate) fn external_body_reference(&self) -> u64 {
        self.external_body_reference
    }
    pub(crate) fn external_segment(&self) -> u32 {
        self.external_segment
    }
    pub(crate) fn external_asset_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_asset_id
    }
    pub(crate) fn external_link_name(&self) -> &str {
        &self.external_link_name
    }
    pub(crate) fn external_version(&self) -> Option<&DesignExternalVersion> {
        self.external_version.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn tail_values(&self) -> [u64; 2] {
        self.tail_values
    }
    #[cfg(test)]
    pub(crate) fn tail_value_offsets(&self) -> [u64; 2] {
        self.tail_value_offsets
    }
    #[cfg(test)]
    pub(crate) fn external_asset_id_offset(&self) -> u64 {
        self.external_asset_id_offset
    }
}

impl TryFrom<DesignCombineExternalBodyIdentityWire> for DesignCombineExternalBodyIdentity {
    type Error = String;
    fn try_from(wire: DesignCombineExternalBodyIdentityWire) -> Result<Self, Self::Error> {
        let external_version = match (wire.external_property_key, wire.external_property_key_offset, wire.external_version_urn, wire.external_version_urn_offset) {
            (None, None, None, None) => None,
            (Some(key), Some(key_offset), Some(urn), Some(urn_offset)) => Some(DesignExternalVersion { property_key: Located { value: key, offset: key_offset }, version_urn: Located { value: urn, offset: urn_offset } }),
            _ => return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into()),
        };
        if wire.external_asset_id != wire.selector_asset_id {
            return Err("external_asset_id must match selector_asset_id".into());
        }
        if wire.occurrence_reference == 0 {
            return Err("occurrence_reference must be nonzero".into());
        }
        if wire.external_body_reference == 0 {
            return Err("external_body_reference must be nonzero".into());
        }
        if wire.external_link_name.is_empty() {
            return Err("external_link_name must not be empty".into());
        }
        let utf16_end = |offset: u64, text: &str| -> Option<u64> {
            offset.checked_add(
                u64::try_from(text.encode_utf16().count())
                    .ok()?
                    .checked_mul(2)?,
            )
        };
        let after_text = |offset: u64, text: &str, delta: u64| {
            utf16_end(offset, text).and_then(|end| end.checked_add(delta))
        };
        let prefix = u64_from_index(crate::layout::combine_external_selector_prefix::LEN + 4);
        if wire.selector_asset_id_offset < prefix {
            return Err("selector_asset_id_offset must follow the selector header".into());
        }
        for (field, actual, expected) in [
            (
                "selector_context_id_offset",
                wire.selector_context_id_offset,
                after_text(
                    wire.selector_asset_id_offset,
                    wire.selector_asset_id.as_str(),
                    4,
                ),
            ),
            (
                "occurrence_reference_offset",
                wire.occurrence_reference_offset,
                after_text(
                    wire.selector_context_id_offset,
                    wire.selector_context_id.as_str(),
                    13,
                ),
            ),
            (
                "external_body_reference_offset",
                wire.external_body_reference_offset,
                wire.occurrence_reference_offset.checked_add(15),
            ),
            (
                "external_segment_offset",
                wire.external_segment_offset,
                wire.external_body_reference_offset.checked_add(9),
            ),
            (
                "external_asset_id_offset",
                wire.external_asset_id_offset,
                wire.external_segment_offset.checked_add(8),
            ),
            (
                "external_link_name_offset",
                wire.external_link_name_offset,
                after_text(
                    wire.external_asset_id_offset,
                    wire.external_asset_id.as_str(),
                    5,
                ),
            ),
            (
                "tail_value_offsets[1]",
                wire.tail_value_offsets[1],
                wire.tail_value_offsets[0].checked_add(12),
            ),
        ] {
            if expected != Some(actual) {
                return Err(format!(
                    "{field} disagrees with the external identity offset chain"
                ));
            }
        }
        let tail_offset = match &external_version {
            None => after_text(wire.external_link_name_offset, &wire.external_link_name, 7),
            Some(version) => {
                if version.version_urn.value.is_empty() {
                    return Err("external_version_urn must not be empty".into());
                }
                if after_text(wire.external_link_name_offset, &wire.external_link_name, 5)
                    != Some(version.property_key.offset)
                {
                    return Err(
                        "external_property_key_offset disagrees with external_link_name".into(),
                    );
                }
                if after_text(
                    version.property_key.offset,
                    version.property_key.value.as_str(),
                    4,
                ) != Some(version.version_urn.offset)
                {
                    return Err(
                        "external_version_urn_offset disagrees with external_property_key".into(),
                    );
                }
                after_text(version.version_urn.offset, &version.version_urn.value, 6)
            }
        };
        if tail_offset != Some(wire.tail_value_offsets[0]) {
            return Err(
                "tail_value_offsets[0] disagrees with the external identity offset chain".into(),
            );
        }
        Ok(Self {
            selector_asset_id: wire.selector_asset_id,
            selector_asset_id_offset: wire.selector_asset_id_offset,
            selector_context_id: wire.selector_context_id,
            selector_context_id_offset: wire.selector_context_id_offset,
            occurrence_reference: wire.occurrence_reference,
            occurrence_reference_offset: wire.occurrence_reference_offset,
            external_body_reference: wire.external_body_reference,
            external_body_reference_offset: wire.external_body_reference_offset,
            external_segment: wire.external_segment,
            external_segment_offset: wire.external_segment_offset,
            external_asset_id_offset: wire.external_asset_id_offset,
            external_link_name: wire.external_link_name,
            external_link_name_offset: wire.external_link_name_offset,
            external_version,
            tail_values: wire.tail_values,
            tail_value_offsets: wire.tail_value_offsets,
        })
    }
}

#[cfg(test)]
impl From<DesignCombineExternalBodyIdentity> for DesignCombineExternalBodyIdentityWire {
    fn from(record: DesignCombineExternalBodyIdentity) -> Self {
        Self {
            selector_asset_id: record.selector_asset_id.clone(),
            selector_asset_id_offset: record.selector_asset_id_offset,
            selector_context_id: record.selector_context_id,
            selector_context_id_offset: record.selector_context_id_offset,
            occurrence_reference: record.occurrence_reference,
            occurrence_reference_offset: record.occurrence_reference_offset,
            external_body_reference: record.external_body_reference,
            external_body_reference_offset: record.external_body_reference_offset,
            external_segment: record.external_segment,
            external_segment_offset: record.external_segment_offset,
            external_asset_id: record.selector_asset_id,
            external_asset_id_offset: record.external_asset_id_offset,
            external_link_name: record.external_link_name,
            external_link_name_offset: record.external_link_name_offset,
            external_property_key: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.value.clone()),
            external_property_key_offset: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.offset),
            external_version_urn: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.value.clone()),
            external_version_urn_offset: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.offset),
            tail_values: record.tail_values,
            tail_value_offsets: record.tail_value_offsets,
        }
    }
}

/// One target or tool body selector owned by a `Combine` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignCombineBodySelection {
    /// Body-selection record index.
    pub(crate) record_index: u32,
    /// Complete external body identity when the selector crosses a document boundary.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_identity"
    )]
    pub(crate) external_identity: Option<DesignCombineExternalBodyIdentity>,
    /// Browser node that the selector names in the operand-list form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_body_node"
    )]
    pub(crate) body_node: Option<DesignCombineBodyNode>,
}

/// Browser node of one body selected by an operand-list `Combine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignCombineBodyNode {
    /// Browser-node record index.
    pub(crate) record_index: u32,
    /// Design entity suffix of the body.
    pub(crate) body_entity_suffix: u32,
    /// Record index of the feature scope that creates the body.
    pub(crate) creating_scope_record_index: u32,
}

/// Exact Boolean construction carried by a `Combine` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignCombineOperationWire")]
pub(crate) struct DesignCombineOperation {
    /// Serialized scope-prologue form.
    pub(crate) form: DesignCombineForm,
    /// Join, cut, or intersect operation.
    pub(crate) operation: cadmpeg_ir::features::BooleanKind,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
    /// Whether the source operation retains its tool bodies.
    pub(crate) keep_tools: bool,
    /// Byte offset of the keep-tools Boolean.
    pub(crate) keep_tools_offset: u64,
    /// Boolean target body selector.
    pub(crate) target_record_index: u32,
    /// Browser node that the target selector names in the operand-list form.
    pub(crate) target_body_node: Option<DesignCombineBodyNode>,
    /// Boolean tool body selectors in source order.
    pub(crate) tools: DesignCombineTools,
}

#[cfg(test)]
thread_local! {
    static COMBINE_OPERATION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCombineOperation {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COMBINE_OPERATION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            form: self.form,
            operation: self.operation,
            operation_offset: self.operation_offset,
            keep_tools: self.keep_tools,
            keep_tools_offset: self.keep_tools_offset,
            target_record_index: self.target_record_index,
            target_body_node: self.target_body_node,
            tools: self.tools.clone(),
        }
    }
}

struct DesignCombineToolsRef<'a>(&'a DesignCombineTools);

impl Serialize for DesignCombineToolsRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter())
    }
}

#[derive(Serialize)]
struct DesignCombineOperationRef<'a> {
    form: DesignCombineForm,
    operation: cadmpeg_ir::features::BooleanKind,
    operation_offset: u64,
    keep_tools: bool,
    keep_tools_offset: u64,
    target: DesignCombineBodySelection,
    tools: DesignCombineToolsRef<'a>,
}

impl Serialize for DesignCombineOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCombineOperationRef {
            form: self.form,
            operation: self.operation,
            operation_offset: self.operation_offset,
            keep_tools: self.keep_tools,
            keep_tools_offset: self.keep_tools_offset,
            target: DesignCombineBodySelection {
                record_index: self.target_record_index,
                external_identity: None,
                body_node: self.target_body_node,
            },
            tools: DesignCombineToolsRef(&self.tools),
        }
        .serialize(serializer)
    }
}

/// Ordered nonempty tools of a Combine operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignCombineTools {
    pub(crate) first: DesignCombineBodySelection,
    pub(crate) additional: Vec<DesignCombineBodySelection>,
}

impl DesignCombineTools {
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &DesignCombineBodySelection> {
        std::iter::once(&self.first).chain(&self.additional)
    }
}

/// Exact Boolean construction carried by a `Combine` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCombineOperationWire {
    /// Serialized scope-prologue form.
    form: DesignCombineForm,
    /// Join, cut, or intersect operation.
    #[serde(deserialize_with = "deserialize_combine_operation_kind")]
    operation: cadmpeg_ir::features::BooleanKind,
    /// Byte offset of the operation u32.
    operation_offset: u64,
    /// Whether the source operation retains its tool bodies.
    keep_tools: bool,
    /// Byte offset of the keep-tools Boolean.
    keep_tools_offset: u64,
    /// Boolean target body selector.
    target: DesignCombineBodySelection,
    /// Boolean tool body selectors in source order.
    tools: Vec<DesignCombineBodySelection>,
}

fn deserialize_combine_operation_kind<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<cadmpeg_ir::features::BooleanKind, D::Error> {
    cadmpeg_ir::features::BooleanKind::deserialize(deserializer)
        .map_err(|error| serde::de::Error::custom(format!("operation: {error}")))
}

impl TryFrom<DesignCombineOperationWire> for DesignCombineOperation {
    type Error = String;
    fn try_from(wire: DesignCombineOperationWire) -> Result<Self, Self::Error> {
        if wire.target.external_identity.is_some() {
            return Err("target.external_identity must be absent".into());
        }
        let mut tools = wire.tools.into_iter();
        let first = tools
            .next()
            .ok_or("tools must contain at least one body selection")?;
        Ok(Self {
            form: wire.form,
            operation: wire.operation,
            operation_offset: wire.operation_offset,
            keep_tools: wire.keep_tools,
            keep_tools_offset: wire.keep_tools_offset,
            target_record_index: wire.target.record_index,
            target_body_node: wire.target.body_node,
            tools: DesignCombineTools {
                first,
                additional: tools.collect(),
            },
        })
    }
}

#[cfg(test)]
impl From<DesignCombineOperation> for DesignCombineOperationWire {
    fn from(operation: DesignCombineOperation) -> Self {
        Self {
            form: operation.form,
            operation: operation.operation,
            operation_offset: operation.operation_offset,
            keep_tools: operation.keep_tools,
            keep_tools_offset: operation.keep_tools_offset,
            target: DesignCombineBodySelection {
                record_index: operation.target_record_index,
                external_identity: None,
                body_node: operation.target_body_node,
            },
            tools: std::iter::once(operation.tools.first)
                .chain(operation.tools.additional)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DesignCombineExternalBodyIdentity, DesignCombineExternalBodyIdentityWire,
        DesignCombineOperation, DesignCombineOperationWire, COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT,
        COMBINE_OPERATION_CLONE_COUNT,
    };

    #[derive(serde::Serialize)]
    struct NestedRecord<'a, T: serde::Serialize> {
        id: &'static str,
        value: &'a T,
    }

    fn external_identity(version: bool) -> DesignCombineExternalBodyIdentity {
        let mut wire = serde_json::json!({
            "selector_asset_id": "00000004-1111-4111-8111-111111111111",
            "selector_asset_id_offset": 44,
            "selector_context_id": "00000005-1111-4111-8111-111111111111",
            "selector_context_id_offset": 120,
            "occurrence_reference": 1,
            "occurrence_reference_offset": 205,
            "external_body_reference": 2,
            "external_body_reference_offset": 220,
            "external_segment": 1,
            "external_segment_offset": 229,
            "external_asset_id": "00000004-1111-4111-8111-111111111111",
            "external_asset_id_offset": 237,
            "external_link_name": "link",
            "external_link_name_offset": 314,
            "tail_values": [0, 0],
            "tail_value_offsets": [329, 341]
        });
        if version {
            wire["external_property_key"] =
                serde_json::json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
            wire["external_property_key_offset"] = serde_json::json!(327);
            wire["external_version_urn"] = serde_json::json!("urn");
            wire["external_version_urn_offset"] = serde_json::json!(403);
            wire["tail_value_offsets"] = serde_json::json!([415, 427]);
        }
        serde_json::from_value(wire).unwrap()
    }

    fn operation(version: bool) -> DesignCombineOperation {
        serde_json::from_value(serde_json::json!({
            "form": "standard",
            "operation": "join",
            "operation_offset": 20,
            "keep_tools": false,
            "keep_tools_offset": 25,
            "target": {"record_index": 1},
            "tools": [{"record_index": 2, "external_identity": external_identity(version)}]
        }))
        .unwrap()
    }

    #[test]
    fn combine_external_identity_borrowed_wire_matches_owned_wire_bytes() {
        for identity in [external_identity(false), external_identity(true)] {
            let owned = DesignCombineExternalBodyIdentityWire::from(identity.clone());
            assert_eq!(
                serde_json::to_vec(&identity).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn combine_external_identity_native_retained_limit_refuses_before_clone() {
        let identity = external_identity(true);
        let record = NestedRecord {
            id: "f3d:native:combine-external-identity#0",
            value: &identity,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT.with(|count| count.set(0)),
            || COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    #[test]
    fn a_body_node_is_present_or_its_key_is_absent() {
        use super::{DesignCombineBodyNode, DesignCombineBodySelection};
        let node = DesignCombineBodyNode {
            record_index: 358,
            body_entity_suffix: 354,
            creating_scope_record_index: 322,
        };
        let selection = DesignCombineBodySelection {
            record_index: 363,
            external_identity: None,
            body_node: Some(node),
        };
        let wire = serde_json::to_value(&selection).unwrap();
        assert_eq!(
            serde_json::from_value::<DesignCombineBodySelection>(wire).unwrap(),
            selection
        );

        let without = DesignCombineBodySelection {
            body_node: None,
            ..selection
        };
        let wire = serde_json::to_value(&without).unwrap();
        assert_eq!(wire, serde_json::json!({ "record_index": 363 }));
        assert_eq!(
            serde_json::from_value::<DesignCombineBodySelection>(wire).unwrap(),
            without
        );

        let error = serde_json::from_value::<DesignCombineBodySelection>(serde_json::json!({
            "record_index": 363,
            "body_node": null,
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("body_node"), "{error}");
    }

    #[test]
    fn an_operation_keeps_its_target_and_tool_body_nodes_on_the_wire() {
        let node = |record_index| super::DesignCombineBodyNode {
            record_index,
            body_entity_suffix: record_index + 1,
            creating_scope_record_index: record_index + 2,
        };
        let mut value = operation(false);
        value.form = super::DesignCombineForm::OperandLists;
        value.target_body_node = Some(node(10));
        value.tools.first.body_node = Some(node(20));
        let wire = serde_json::to_value(&value).unwrap();
        let read: DesignCombineOperation = serde_json::from_value(wire).unwrap();
        assert_eq!(read, value);
    }

    #[test]
    fn combine_operation_borrowed_wire_matches_owned_wire_bytes() {
        for record in [operation(false), operation(true)] {
            let owned = DesignCombineOperationWire::from(record.clone());
            assert_eq!(
                serde_json::to_vec(&record).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn combine_operation_native_retained_limit_refuses_before_clone() {
        let operation = operation(true);
        let record = NestedRecord {
            id: "f3d:native:combine-operation#0",
            value: &operation,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || COMBINE_OPERATION_CLONE_COUNT.with(|count| count.set(0)),
            || COMBINE_OPERATION_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }
}

mod identity_rewrite;
