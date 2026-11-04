// SPDX-License-Identifier: Apache-2.0
//! Design parameters, their owners and the companion records that carry their expressions.

use super::identity::{Located, RecordedValue};
use super::references::DesignClassTag;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Deserializer, Serialize};
use std::num::NonZeroU64;

cadmpeg_core::named_optional_field!(
    deserialize_family_discriminator,
    u64,
    "family_discriminator"
);
cadmpeg_core::named_optional_field!(
    deserialize_family_discriminator_offset,
    u64,
    "family_discriminator_offset"
);
cadmpeg_core::named_optional_field!(deserialize_owner_record_index, u32, "owner_record_index");
cadmpeg_core::named_optional_field!(deserialize_payload_byte_length, u64, "payload_byte_length");
cadmpeg_core::named_optional_field!(deserialize_payload_byte_offset, u64, "payload_byte_offset");
cadmpeg_core::named_optional_field!(deserialize_unit, String, "unit");
cadmpeg_core::named_optional_field!(deserialize_unit_offset, u64, "unit_offset");
cadmpeg_core::named_optional_field!(deserialize_variant, u8, "variant");
/// Semantic family of one Design parameter record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignParameterKind {
    /// A document-level named user parameter.
    User,
    /// A dimensional constraint parameter.
    Dimension,
    /// A parameter consumed by a construction feature.
    Feature,
}

/// Admitted Design parameter family discriminator values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub(crate) enum DesignParameterDiscriminator {
    Code0 = 0,
    Code1 = 1,
    Code2 = 2,
    Code3 = 3,
    Code4 = 4,
    Code5 = 5,
    Code6 = 6,
}

impl DesignParameterDiscriminator {
    pub(crate) fn code(self) -> u64 {
        match self {
            Self::Code0 => 0,
            Self::Code1 => 1,
            Self::Code2 => 2,
            Self::Code3 => 3,
            Self::Code4 => 4,
            Self::Code5 => 5,
            Self::Code6 => 6,
        }
    }
}

impl TryFrom<u64> for DesignParameterDiscriminator {
    type Error = String;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Code0),
            1 => Ok(Self::Code1),
            2 => Ok(Self::Code2),
            3 => Ok(Self::Code3),
            4 => Ok(Self::Code4),
            5 => Ok(Self::Code5),
            6 => Ok(Self::Code6),
            _ => Err(format!(
                "invalid design parameter family_discriminator {value}"
            )),
        }
    }
}

/// Source family and ownership of one Design parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DesignParameterSource {
    /// A document parameter. The discriminated form stores a family
    /// discriminator; the typed compact form stores none.
    User {
        /// Localized document-parameter source kind.
        source_kind: NonBlankString,
        family_discriminator: Option<Located<DesignParameterDiscriminator>>,
    },
    Owned(OwnedDesignParameter),
}

/// Localized source kinds of a document parameter.
const DOCUMENT_PARAMETER_SOURCE_KINDS: [&str; 2] = ["User Parameter", "Paramètre utilisateur"];

/// Whether a parameter source kind names a document parameter.
pub(crate) fn is_document_source_kind(source_kind: &str) -> bool {
    DOCUMENT_PARAMETER_SOURCE_KINDS.contains(&source_kind)
}

/// An owned source family. Its nonempty name cannot identify a user parameter.
/// A frame with a null owner reference has no owner record index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedDesignParameter {
    source_kind: NonBlankString,
    owner_record_index: Option<u32>,
    family_discriminator: Option<Located<DesignParameterDiscriminator>>,
}

impl DesignParameterSource {
    pub(crate) fn new(
        source_kind: String,
        owner_record_index: Option<u32>,
        family_discriminator: Option<Located<DesignParameterDiscriminator>>,
    ) -> Result<Self, String> {
        let Some(source_kind) = NonBlankString::new(source_kind) else {
            return Err("design parameter source_kind is empty".into());
        };
        match (
            is_document_source_kind(source_kind.as_str()),
            owner_record_index,
        ) {
            (true, None) => Ok(Self::User {
                source_kind,
                family_discriminator,
            }),
            (true, Some(_)) => {
                Err("design parameter owner_record_index is present for user source_kind".into())
            }
            (false, owner_record_index) => Ok(Self::Owned(OwnedDesignParameter {
                source_kind,
                owner_record_index,
                family_discriminator,
            })),
        }
    }
}

/// Flags of a parameter's expression trailer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DesignParameterFlags {
    /// The parameter belongs to a driven dimension.
    pub(crate) driven: bool,
    /// The user marks the parameter as a favorite.
    pub(crate) favorite: bool,
}

/// One indexed Design parameter or expression record.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignParameterSerde")]
pub(crate) struct DesignParameter {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Source indexed-record identity.
    pub(crate) record_index: u32,
    /// Source ordering value stored by the parameter record.
    pub(crate) source_ordinal: u32,
    /// Indexed owner: user parameters have none; feature and dimension
    /// parameters name their owning record.
    source: DesignParameterSource,
    /// Literal or symbolic source expression.
    expression: NonBlankString,
    /// Byte offset of the expression's UTF-16LE code units.
    expression_offset: u64,
    /// Byte offset of the source-family UTF-16LE code units.
    source_kind_offset: u64,
    /// Declared unit token; absent for dimensionless and Boolean parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit: Option<Located<NonBlankString>>,
    /// Source parameter name or dimension identifier.
    name: NonBlankString,
    /// Byte offset of the name's UTF-16LE code units.
    name_offset: u64,
    /// Evaluated scalar in the record's native unit convention.
    evaluated_value: FiniteReal,
    /// Byte offset of `evaluated_value`.
    evaluated_value_offset: u64,
    flags: DesignParameterFlags,
}

#[cfg(test)]
thread_local! {
    static PARAMETER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignParameter {
    fn clone(&self) -> Self {
        PARAMETER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            source_ordinal: self.source_ordinal,
            source: self.source.clone(),
            expression: self.expression.clone(),
            expression_offset: self.expression_offset,
            source_kind_offset: self.source_kind_offset,
            unit: self.unit.clone(),
            name: self.name.clone(),
            name_offset: self.name_offset,
            evaluated_value: self.evaluated_value,
            evaluated_value_offset: self.evaluated_value_offset,
            flags: self.flags,
        }
    }
}

/// Unchecked Design parameter input.
pub(crate) struct DesignParameterDraft {
    pub(crate) id: String,
    pub(crate) byte_offset: u64,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) record_index: u32,
    pub(crate) source_ordinal: u32,
    pub(crate) source: DesignParameterSource,
    pub(crate) expression: String,
    pub(crate) expression_offset: u64,
    pub(crate) source_kind_offset: u64,
    pub(crate) unit: Option<RecordedValue<String>>,
    pub(crate) name: String,
    pub(crate) name_offset: u64,
    pub(crate) evaluated_value: f64,
    pub(crate) evaluated_value_offset: u64,
}

impl TryFrom<DesignParameterDraft> for DesignParameter {
    type Error = String;
    fn try_from(draft: DesignParameterDraft) -> Result<Self, Self::Error> {
        let evaluated_value =
            FiniteReal::new(draft.evaluated_value).ok_or("evaluated_value must be finite")?;
        let expression =
            NonBlankString::new(draft.expression).ok_or("expression must not be empty")?;
        let name = NonBlankString::new(draft.name).ok_or("name must not be empty")?;
        let unit = draft
            .unit
            .map(|unit| {
                Ok::<_, String>(Located {
                    value: NonBlankString::new(unit.value).ok_or("unit must not be empty")?,
                    offset: unit.offset,
                })
            })
            .transpose()?;
        check_parameter_discriminator_offset(
            &draft.source,
            draft.byte_offset,
            draft.expression_offset,
        )?;
        if !(draft.byte_offset < draft.expression_offset
            && draft.expression_offset < draft.source_kind_offset
            && unit
                .as_ref()
                .map_or(draft.source_kind_offset < draft.name_offset, |unit| {
                    draft.source_kind_offset < unit.offset && unit.offset < draft.name_offset
                })
            && draft.name_offset < draft.evaluated_value_offset)
        {
            return Err("byte_offset, expression_offset, source_kind_offset, unit_offset, name_offset, and evaluated_value_offset must be strictly ordered".into());
        }
        Ok(Self {
            id: draft.id,
            byte_offset: draft.byte_offset,
            class_tag: draft.class_tag,
            record_index: draft.record_index,
            source_ordinal: draft.source_ordinal,
            source: draft.source,
            expression,
            expression_offset: draft.expression_offset,
            source_kind_offset: draft.source_kind_offset,
            unit,
            name,
            name_offset: draft.name_offset,
            evaluated_value,
            evaluated_value_offset: draft.evaluated_value_offset,
            flags: DesignParameterFlags::default(),
        })
    }
}

fn check_parameter_discriminator_offset(
    source: &DesignParameterSource,
    byte_offset: u64,
    expression_offset: u64,
) -> Result<(), String> {
    let family_discriminator = match source {
        DesignParameterSource::User {
            family_discriminator,
            ..
        } => *family_discriminator,
        DesignParameterSource::Owned(source) => source.family_discriminator,
    };
    // The wide discriminated form stores four further zero bytes before it.
    if family_discriminator.is_some_and(|field| {
        (byte_offset.checked_add(22) != Some(field.offset)
            && byte_offset.checked_add(26) != Some(field.offset))
            || field.offset >= expression_offset
    }) {
        return Err("family_discriminator_offset must follow byte_offset by 22 or 26 bytes and precede expression_offset".into());
    }
    Ok(())
}

impl DesignParameter {
    /// Source family and ownership.
    pub(crate) fn source(&self) -> &DesignParameterSource {
        &self.source
    }

    /// Checked replacement of the expression-trailer flags.
    pub(crate) fn with_flags(mut self, flags: DesignParameterFlags) -> Result<Self, String> {
        if flags.driven && matches!(self.source, DesignParameterSource::User { .. }) {
            return Err("a document parameter has no driven flag".into());
        }
        self.flags = flags;
        Ok(self)
    }

    /// Expression-trailer flags.
    #[cfg(test)]
    pub(crate) fn flags(&self) -> DesignParameterFlags {
        self.flags
    }

    /// `Some(false)` for the parameter of a driven dimension.
    pub(crate) fn driving(&self) -> Option<bool> {
        self.flags.driven.then_some(false)
    }

    #[cfg(test)]
    /// Checked replacement of a present unit token.
    pub(crate) fn try_set_unit_value(&mut self, value: String) -> Result<(), String> {
        let value = NonBlankString::new(value).ok_or("unit must not be empty")?;
        let unit = self.unit.as_mut().ok_or("unit is absent")?;
        unit.value = value;
        Ok(())
    }

    /// Indexed record header offset.
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    /// Expression code-unit offset.
    #[cfg(test)]
    pub(crate) fn expression_offset(&self) -> u64 {
        self.expression_offset
    }
    /// Evaluated scalar offset.
    pub(crate) fn evaluated_value_offset(&self) -> u64 {
        self.evaluated_value_offset
    }
    /// Evaluated scalar.
    pub(crate) fn evaluated_value(&self) -> FiniteReal {
        self.evaluated_value
    }
    /// Nonempty parameter name.
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }
    /// Nonempty source expression.
    pub(crate) fn expression(&self) -> &str {
        self.expression.as_str()
    }
    /// Located nonempty unit token, when present.
    pub(crate) fn unit(&self) -> Option<&Located<NonBlankString>> {
        self.unit.as_ref()
    }

    #[cfg(test)]
    /// Checked replacement of the evaluated scalar.
    pub(crate) fn try_set_evaluated_value(&mut self, value: f64) -> Result<(), String> {
        self.evaluated_value = FiniteReal::new(value).ok_or("evaluated_value must be finite")?;
        Ok(())
    }
    #[cfg(test)]
    /// Checked replacement of source family and ownership.
    pub(crate) fn try_set_source(&mut self, source: DesignParameterSource) -> Result<(), String> {
        check_parameter_discriminator_offset(&source, self.byte_offset, self.expression_offset)?;
        self.source = source;
        Ok(())
    }

    pub(crate) fn family_discriminator(&self) -> Option<Located<DesignParameterDiscriminator>> {
        match &self.source {
            DesignParameterSource::User {
                family_discriminator,
                ..
            } => *family_discriminator,
            DesignParameterSource::Owned(source) => source.family_discriminator,
        }
    }

    pub(crate) fn source_kind(&self) -> &NonBlankString {
        match &self.source {
            DesignParameterSource::User { source_kind, .. } => source_kind,
            DesignParameterSource::Owned(source) => &source.source_kind,
        }
    }

    /// The source kind as text.
    pub(crate) fn source_kind_str(&self) -> &str {
        self.source_kind().as_str()
    }

    pub(crate) fn kind(&self) -> DesignParameterKind {
        design_parameter_kind_from_source(self.source_kind_str())
    }

    pub(crate) fn owner_record_index(&self) -> Option<u32> {
        match &self.source {
            DesignParameterSource::User { .. } => None,
            DesignParameterSource::Owned(source) => source.owner_record_index,
        }
    }
}

impl Serialize for DesignParameter {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            byte_offset: u64,
            class_tag: &'a str,
            record_index: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            family_discriminator: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            family_discriminator_offset: Option<u64>,
            source_ordinal: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            owner_record_index: Option<u32>,
            expression: &'a str,
            expression_offset: u64,
            source_kind: &'a str,
            source_kind_offset: u64,
            kind: DesignParameterKind,
            #[serde(skip_serializing_if = "Option::is_none")]
            unit: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            unit_offset: Option<u64>,
            name: &'a str,
            name_offset: u64,
            evaluated_value: f64,
            evaluated_value_offset: u64,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            driven: bool,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            favorite: bool,
        }
        let family_discriminator = self.family_discriminator();
        WireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            family_discriminator: family_discriminator.map(|value| value.value.code()),
            family_discriminator_offset: family_discriminator.map(|value| value.offset),
            source_ordinal: self.source_ordinal,
            owner_record_index: self.owner_record_index(),
            expression: self.expression.as_str(),
            expression_offset: self.expression_offset,
            source_kind: self.source_kind_str(),
            source_kind_offset: self.source_kind_offset,
            kind: self.kind(),
            unit: self.unit.as_ref().map(|field| field.value.as_str()),
            unit_offset: self.unit.as_ref().map(|field| field.offset),
            name: self.name.as_str(),
            name_offset: self.name_offset,
            evaluated_value: self.evaluated_value.get(),
            evaluated_value_offset: self.evaluated_value_offset,
            driven: self.flags.driven,
            favorite: self.flags.favorite,
        }
        .serialize(serializer)
    }
}

fn design_parameter_kind_from_source(source_kind: &str) -> DesignParameterKind {
    if is_document_source_kind(source_kind) {
        DesignParameterKind::User
    } else if source_kind.contains("Dimension") {
        DesignParameterKind::Dimension
    } else {
        DesignParameterKind::Feature
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignParameterSerde {
    id: String,
    byte_offset: u64,
    class_tag: String,
    record_index: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_family_discriminator"
    )]
    family_discriminator: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_family_discriminator_offset"
    )]
    family_discriminator_offset: Option<u64>,
    source_ordinal: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner_record_index"
    )]
    owner_record_index: Option<u32>,
    expression: String,
    expression_offset: u64,
    source_kind: String,
    source_kind_offset: u64,
    kind: DesignParameterKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unit"
    )]
    unit: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unit_offset"
    )]
    unit_offset: Option<u64>,
    name: String,
    name_offset: u64,
    evaluated_value: f64,
    evaluated_value_offset: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    driven: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    favorite: bool,
}

impl TryFrom<DesignParameterSerde> for DesignParameter {
    type Error = String;

    fn try_from(wire: DesignParameterSerde) -> Result<Self, Self::Error> {
        let derived = design_parameter_kind_from_source(&wire.source_kind);
        if wire.kind != derived {
            return Err("design parameter kind disagrees with source_kind".into());
        }
        let family_discriminator = Located::from_wire(
            wire.family_discriminator,
            wire.family_discriminator_offset,
            "DesignParameter.family_discriminator",
        )?
        .map(|field| {
            Ok::<_, String>(Located {
                value: DesignParameterDiscriminator::try_from(field.value)?,
                offset: field.offset,
            })
        })
        .transpose()?;
        let source = DesignParameterSource::new(
            wire.source_kind,
            wire.owner_record_index,
            family_discriminator,
        )?;
        Self::try_from(DesignParameterDraft {
            id: wire.id,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            record_index: wire.record_index,
            source_ordinal: wire.source_ordinal,
            source,
            expression: wire.expression,
            expression_offset: wire.expression_offset,
            source_kind_offset: wire.source_kind_offset,
            unit: RecordedValue::from_wire(wire.unit, wire.unit_offset, "unit")?,
            name: wire.name,
            name_offset: wire.name_offset,
            evaluated_value: wire.evaluated_value,
            evaluated_value_offset: wire.evaluated_value_offset,
        })?
        .with_flags(DesignParameterFlags {
            driven: wire.driven,
            favorite: wire.favorite,
        })
    }
}

#[cfg(test)]
impl From<DesignParameter> for DesignParameterSerde {
    fn from(parameter: DesignParameter) -> Self {
        let byte_offset = parameter.byte_offset();
        let expression_offset = parameter.expression_offset;
        let source_kind_offset = parameter.source_kind_offset;
        let name_offset = parameter.name_offset;
        let evaluated_value_offset = parameter.evaluated_value_offset();
        let evaluated_value = parameter.evaluated_value().get();
        let kind = parameter.kind();
        let owner_record_index = parameter.owner_record_index();
        let family_discriminator = parameter.family_discriminator();
        let source_kind = match parameter.source {
            DesignParameterSource::User { source_kind, .. } => source_kind.to_string(),
            DesignParameterSource::Owned(source) => source.source_kind.to_string(),
        };
        Self {
            id: parameter.id,
            byte_offset,
            class_tag: parameter.class_tag.into(),
            record_index: parameter.record_index,
            family_discriminator: family_discriminator.map(|value| value.value.code()),
            family_discriminator_offset: family_discriminator.map(|value| value.offset),
            source_ordinal: parameter.source_ordinal,
            owner_record_index,
            expression: parameter.expression.to_string(),
            expression_offset,
            source_kind,
            source_kind_offset,
            kind,
            unit_offset: parameter.unit.as_ref().map(|field| field.offset),
            unit: parameter.unit.map(|field| field.value.to_string()),
            name: parameter.name.to_string(),
            name_offset,
            evaluated_value,
            evaluated_value_offset,
            driven: parameter.flags.driven,
            favorite: parameter.flags.favorite,
        }
    }
}

/// Indexed record that owns one Design parameter.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignParameterOwnerWire")]
pub(crate) struct DesignParameterOwner {
    id: String,
    byte_offset: u64,
    frame_length: u64,
    class_tag: DesignClassTag,
    scope_record_index: u32,
    local_ordinal: u32,
    evaluated_value: FiniteReal,
    evaluated_value_offset: u64,
    owned_ordinal: u32,
    variant: Option<u8>,
    base_index: u32,
    order: ParameterFrameOrder,
}

#[cfg(test)]
thread_local! {
    static PARAMETER_OWNER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignParameterOwner {
    fn clone(&self) -> Self {
        PARAMETER_OWNER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            frame_length: self.frame_length,
            class_tag: self.class_tag.clone(),
            scope_record_index: self.scope_record_index,
            local_ordinal: self.local_ordinal,
            evaluated_value: self.evaluated_value,
            evaluated_value_offset: self.evaluated_value_offset,
            owned_ordinal: self.owned_ordinal,
            variant: self.variant,
            base_index: self.base_index,
            order: self.order,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParameterFrameOrder {
    OwnerParameterCompanion,
    ParameterOwnerCompanion,
    OwnerCompanionParameter,
}

impl DesignParameterOwner {
    /// The id value.
    pub(crate) fn id(&self) -> &String {
        &self.id
    }
    /// The byte offset value.
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    /// The frame length value.
    pub(crate) fn frame_length(&self) -> u64 {
        self.frame_length
    }
    /// The class tag value.
    pub(crate) fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    /// The record index value.
    pub(crate) fn record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerParameterCompanion => 0,
                ParameterFrameOrder::ParameterOwnerCompanion => 1,
                ParameterFrameOrder::OwnerCompanionParameter => 0,
            }
    }
    /// The scope record index value.
    pub(crate) fn scope_record_index(&self) -> u32 {
        self.scope_record_index
    }
    /// The local ordinal value.
    pub(crate) fn local_ordinal(&self) -> u32 {
        self.local_ordinal
    }
    /// The evaluated value value.
    pub(crate) fn evaluated_value(&self) -> FiniteReal {
        self.evaluated_value
    }
    /// The evaluated value offset value.
    pub(crate) fn evaluated_value_offset(&self) -> u64 {
        self.evaluated_value_offset
    }
    /// The parameter record index value.
    pub(crate) fn parameter_record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerParameterCompanion => 1,
                ParameterFrameOrder::ParameterOwnerCompanion => 0,
                ParameterFrameOrder::OwnerCompanionParameter => 2,
            }
    }
    /// The companion record index value.
    pub(crate) fn companion_record_index(&self) -> u32 {
        self.base_index
            + match self.order {
                ParameterFrameOrder::OwnerParameterCompanion => 2,
                ParameterFrameOrder::ParameterOwnerCompanion => 2,
                ParameterFrameOrder::OwnerCompanionParameter => 1,
            }
    }
}

impl Serialize for DesignParameterOwner {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            byte_offset: u64,
            frame_length: u64,
            class_tag: &'a str,
            record_index: u32,
            scope_record_index: u32,
            local_ordinal: u32,
            evaluated_value: f64,
            evaluated_value_offset: u64,
            parameter_record_index: u32,
            owned_ordinal: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            variant: Option<u8>,
            companion_record_index: u32,
        }
        WireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            frame_length: self.frame_length,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index(),
            scope_record_index: self.scope_record_index,
            local_ordinal: self.local_ordinal,
            evaluated_value: self.evaluated_value.get(),
            evaluated_value_offset: self.evaluated_value_offset,
            parameter_record_index: self.parameter_record_index(),
            owned_ordinal: self.owned_ordinal,
            variant: self.variant,
            companion_record_index: self.companion_record_index(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignParameterOwnerWire> for DesignParameterOwner {
    type Error = String;
    fn try_from(wire: DesignParameterOwnerWire) -> Result<Self, Self::Error> {
        let evaluated_value =
            FiniteReal::new(wire.evaluated_value).ok_or("evaluated_value must be finite")?;
        Self::from_parts(DesignParameterOwnerWire {
            id: wire.id,
            byte_offset: wire.byte_offset,
            frame_length: wire.frame_length,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            scope_record_index: wire.scope_record_index,
            local_ordinal: wire.local_ordinal,
            evaluated_value,
            evaluated_value_offset: wire.evaluated_value_offset,
            parameter_record_index: wire.parameter_record_index,
            owned_ordinal: wire.owned_ordinal,
            variant: wire.variant,
            companion_record_index: wire.companion_record_index,
        })
    }
}

impl DesignParameterOwner {
    pub(crate) fn from_parts(wire: DesignParameterOwnerWire<FiniteReal>) -> Result<Self, String> {
        let evaluated_value = wire.evaluated_value;
        let (base_index, order) = if wire.record_index.checked_add(1)
            == Some(wire.parameter_record_index)
            && wire.record_index.checked_add(2) == Some(wire.companion_record_index)
        {
            (
                wire.record_index,
                ParameterFrameOrder::OwnerParameterCompanion,
            )
        } else if wire.parameter_record_index.checked_add(1) == Some(wire.record_index)
            && wire.parameter_record_index.checked_add(2) == Some(wire.companion_record_index)
        {
            (
                wire.parameter_record_index,
                ParameterFrameOrder::ParameterOwnerCompanion,
            )
        } else if wire.record_index.checked_add(1) == Some(wire.companion_record_index)
            && wire.record_index.checked_add(2) == Some(wire.parameter_record_index)
        {
            (
                wire.record_index,
                ParameterFrameOrder::OwnerCompanionParameter,
            )
        } else {
            return Err("record_index, parameter_record_index, and companion_record_index must follow a parameter frame order".into());
        };
        // A variant byte either ends its scope lane, keeping the variant-less
        // length, or is followed by one flag byte.
        let modern = matches!(
            (
                wire.frame_length,
                wire.evaluated_value_offset.checked_sub(wire.byte_offset),
                wire.variant
            ),
            (99 | 103, Some(40), None)
                | (100, Some(41), None)
                | (107, Some(44), None)
                | (100 | 101, Some(41), Some(0..=1))
                | (103 | 104, Some(40), Some(0..=1))
                | (107 | 108, Some(44), Some(0..=1))
        );
        // The typed-reference spelling widens each of the five reference
        // fields by forty bytes; one of them precedes the scalar envelope.
        let typed = matches!(
            (
                wire.frame_length,
                wire.evaluated_value_offset.checked_sub(wire.byte_offset),
                wire.variant
            ),
            (299 | 303, Some(80), None)
                | (300, Some(81), None)
                | (307, Some(84), None)
                | (300 | 301, Some(81), Some(0..=1))
                | (303 | 304, Some(80), Some(0..=1))
                | (307 | 308, Some(84), Some(0..=1))
        );
        let legacy = wire.local_ordinal == 0
            && match wire.frame_length {
                // Class tags are dynamic; the decoder admits these forms by the
                // owner record's type, so the wire form is its length and lanes.
                68 => wire.scope_record_index == 0,
                88 => wire.scope_record_index != 0,
                _ => false,
            };
        if !modern && !typed && !legacy {
            return Err("frame_length, evaluated_value_offset, and variant must describe a parameter owner frame".into());
        }
        Ok(Self {
            base_index,
            order,
            id: wire.id,
            byte_offset: wire.byte_offset,
            frame_length: wire.frame_length,
            class_tag: wire.class_tag,
            scope_record_index: wire.scope_record_index,
            local_ordinal: wire.local_ordinal,
            evaluated_value,
            evaluated_value_offset: wire.evaluated_value_offset,
            owned_ordinal: wire.owned_ordinal,
            variant: wire.variant,
        })
    }
}

#[cfg(test)]
impl From<DesignParameterOwner> for DesignParameterOwnerWire {
    fn from(owner: DesignParameterOwner) -> Self {
        Self {
            id: owner.id.clone(),
            byte_offset: owner.byte_offset,
            frame_length: owner.frame_length,
            class_tag: owner.class_tag.clone(),
            record_index: owner.record_index(),
            scope_record_index: owner.scope_record_index,
            local_ordinal: owner.local_ordinal,
            evaluated_value: owner.evaluated_value.get(),
            evaluated_value_offset: owner.evaluated_value_offset,
            parameter_record_index: owner.parameter_record_index(),
            owned_ordinal: owner.owned_ordinal,
            variant: owner.variant,
            companion_record_index: owner.companion_record_index(),
        }
    }
}

/// Parameter owner fields before aggregate validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignParameterOwnerWire<T = f64> {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Byte length from the primary header to its same-index paired header.
    #[serde(default)]
    pub(crate) frame_length: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Source indexed-record identity.
    pub(crate) record_index: u32,
    /// Feature or sketch record that scopes this parameter.
    pub(crate) scope_record_index: u32,
    /// Position among parameters in the same scope.
    pub(crate) local_ordinal: u32,
    /// Evaluated scalar duplicated from the parameter record.
    pub(crate) evaluated_value: T,
    /// Byte offset of `evaluated_value`.
    pub(crate) evaluated_value_offset: u64,
    /// Indexed parameter record owned by this frame.
    pub(crate) parameter_record_index: u32,
    /// Native owner ordering value.
    pub(crate) owned_ordinal: u32,
    /// Source owner-frame variant flag when the frame carries one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_variant"
    )]
    pub(crate) variant: Option<u8>,
    /// Paired indexed record following the parameter record.
    pub(crate) companion_record_index: u32,
}

/// Owned payload bound to a Design parameter companion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignCompanionPayload {
    byte_offset: u64,
    byte_length: u64,
    owned_recipe_ids: Vec<String>,
}

impl DesignCompanionPayload {
    /// The byte interval a companion owns, with the recipes nested in it.
    #[must_use]
    pub(crate) fn new(byte_offset: u64, byte_length: u64, owned_recipe_ids: Vec<String>) -> Self {
        Self {
            byte_offset,
            byte_length,
            owned_recipe_ids,
        }
    }

    /// First byte owned after the fixed companion prefix.
    #[must_use]
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    /// Number of bytes owned before the next sibling Design record.
    #[must_use]
    pub(crate) fn byte_length(&self) -> u64 {
        self.byte_length
    }

    /// Construction recipes contained by the owned payload, in byte order.
    #[must_use]
    pub(crate) fn owned_recipe_ids(&self) -> &[String] {
        &self.owned_recipe_ids
    }
}

/// Fixed prefix of the indexed record paired with a Design parameter owner.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignParameterCompanionWire")]
pub(crate) struct DesignParameterCompanion {
    id: String,
    byte_offset: u64,
    class_tag: DesignClassTag,
    record_index: u32,
    owner_record_index: u32,
    timestamp_micros: NonZeroU64,
    timestamp_micros_offset: u64,
    payload: Option<DesignCompanionPayload>,
}

#[cfg(test)]
thread_local! {
    static PARAMETER_COMPANION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignParameterCompanion {
    fn clone(&self) -> Self {
        PARAMETER_COMPANION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            owner_record_index: self.owner_record_index,
            timestamp_micros: self.timestamp_micros,
            timestamp_micros_offset: self.timestamp_micros_offset,
            payload: self.payload.clone(),
        }
    }
}

impl DesignParameterCompanion {
    /// A companion prefix whose owned payload has not been bound.
    #[must_use]
    pub(crate) fn unbound(
        id: String,
        byte_offset: u64,
        class_tag: DesignClassTag,
        record_index: u32,
        owner_record_index: u32,
        timestamp_micros: NonZeroU64,
        timestamp_micros_offset: u64,
    ) -> Self {
        Self {
            id,
            byte_offset,
            class_tag,
            record_index,
            owner_record_index,
            timestamp_micros,
            timestamp_micros_offset,
            payload: None,
        }
    }

    /// The same companion with its owned payload bound.
    #[must_use]
    pub(crate) fn bound(self, payload: DesignCompanionPayload) -> Self {
        Self {
            payload: Some(payload),
            ..self
        }
    }

    /// Globally unique deterministic identifier for this native record.
    #[must_use]
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// Byte offset of the indexed record header in its Design `BulkStream`.
    #[must_use]
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    /// Source indexed-record identity.
    #[must_use]
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }

    /// Indexed parameter-owner record referenced by this prefix.
    #[must_use]
    pub(crate) fn owner_record_index(&self) -> u32 {
        self.owner_record_index
    }

    /// Byte offset of `timestamp_micros`.
    #[must_use]
    pub(crate) fn timestamp_micros_offset(&self) -> u64 {
        self.timestamp_micros_offset
    }

    /// Start of the owned payload: the timestamp is followed by eight zero
    /// bytes in both reference spellings of the prefix.
    pub(crate) fn payload_byte_offset(&self) -> Option<u64> {
        self.timestamp_micros_offset.checked_add(16)
    }

    /// Owned payload, when the binding pass reached this companion.
    #[must_use]
    pub(crate) fn payload(&self) -> Option<&DesignCompanionPayload> {
        self.payload.as_ref()
    }
}

impl Serialize for DesignParameterCompanion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            byte_offset: u64,
            class_tag: &'a str,
            record_index: u32,
            owner_record_index: u32,
            timestamp_micros: NonZeroU64,
            timestamp_micros_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            payload_byte_offset: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            payload_byte_length: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            owned_recipe_ids: Option<&'a [String]>,
        }
        let payload = self.payload.as_ref();
        WireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            owner_record_index: self.owner_record_index,
            timestamp_micros: self.timestamp_micros,
            timestamp_micros_offset: self.timestamp_micros_offset,
            payload_byte_offset: payload.map(|payload| payload.byte_offset),
            payload_byte_length: payload.map(|payload| payload.byte_length),
            owned_recipe_ids: payload
                .map(|payload| payload.owned_recipe_ids.as_slice())
                .filter(|recipes| !recipes.is_empty()),
        }
        .serialize(serializer)
    }
}

/// Serialized form of [`DesignParameterCompanion`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesignParameterCompanionWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    class_tag: DesignClassTag,
    /// Source indexed-record identity.
    record_index: u32,
    /// Indexed parameter-owner record referenced by this prefix.
    owner_record_index: u32,
    /// Nonzero Unix-epoch timestamp in microseconds.
    #[serde(
        alias = "opaque_value",
        deserialize_with = "deserialize_companion_timestamp"
    )]
    timestamp_micros: NonZeroU64,
    /// Byte offset of `timestamp_micros`.
    #[serde(alias = "opaque_value_offset")]
    timestamp_micros_offset: u64,
    /// First byte owned after the fixed companion prefix, when bound.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_payload_byte_offset"
    )]
    payload_byte_offset: Option<u64>,
    /// Number of bytes owned before the next sibling Design record, when bound.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_payload_byte_length"
    )]
    payload_byte_length: Option<u64>,
    /// Construction recipes contained by the owned payload, in byte order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    owned_recipe_ids: Vec<String>,
}

impl TryFrom<DesignParameterCompanionWire> for DesignParameterCompanion {
    type Error = String;
    fn try_from(wire: DesignParameterCompanionWire) -> Result<Self, Self::Error> {
        let payload = match (wire.payload_byte_offset, wire.payload_byte_length) {
            (Some(byte_offset), Some(byte_length)) => Some(DesignCompanionPayload::new(
                byte_offset,
                byte_length,
                wire.owned_recipe_ids,
            )),
            (None, None) if wire.owned_recipe_ids.is_empty() => None,
            (None, None) => {
                return Err(
                    "owned_recipe_ids requires payload_byte_offset and payload_byte_length".into(),
                )
            }
            (Some(_), None) => return Err("payload_byte_length is required".into()),
            (None, Some(_)) => return Err("payload_byte_offset is required".into()),
        };
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            owner_record_index: wire.owner_record_index,
            timestamp_micros: wire.timestamp_micros,
            timestamp_micros_offset: wire.timestamp_micros_offset,
            payload,
        })
    }
}

#[cfg(test)]
impl From<DesignParameterCompanion> for DesignParameterCompanionWire {
    fn from(record: DesignParameterCompanion) -> Self {
        let (payload_byte_offset, payload_byte_length, owned_recipe_ids) = match record.payload {
            Some(payload) => (
                Some(payload.byte_offset),
                Some(payload.byte_length),
                payload.owned_recipe_ids,
            ),
            None => (None, None, Vec::new()),
        };
        Self {
            id: record.id,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag,
            record_index: record.record_index,
            owner_record_index: record.owner_record_index,
            timestamp_micros: record.timestamp_micros,
            timestamp_micros_offset: record.timestamp_micros_offset,
            payload_byte_offset,
            payload_byte_length,
            owned_recipe_ids,
        }
    }
}

fn deserialize_companion_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<NonZeroU64, D::Error> {
    NonZeroU64::new(u64::deserialize(deserializer)?)
        .ok_or_else(|| serde::de::Error::custom("timestamp_micros must be nonzero"))
}

#[cfg(test)]
mod tests;
