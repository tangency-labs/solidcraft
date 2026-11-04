// SPDX-License-Identifier: Apache-2.0
//! Coil extents, sections, placements, transforms and the coil scope.

use super::extrude::DesignExtrudeOperation;
use super::scope::DesignFaceRecipeKind;
use crate::records::identity::{
    DesignSecondaryIdentity, Located, MaybeRecordedValue, RecordedValue, IDENTITY_MATRIX,
};
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::recipes::{ConstructionRecipeDesign, ConstructionRecipeSelector};
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::{Deserialize, Deserializer, Serialize};

cadmpeg_core::named_optional_field!(deserialize_coil_clockwise, bool, "coil_clockwise");
cadmpeg_core::named_optional_field!(
    deserialize_coil_clockwise_offset,
    u64,
    "coil_clockwise_offset"
);
cadmpeg_core::named_optional_field!(deserialize_coil_extent, DesignCoilExtent, "coil_extent");
cadmpeg_core::named_optional_field!(deserialize_coil_extent_offset, u64, "coil_extent_offset");
cadmpeg_core::named_optional_field!(
    deserialize_coil_operation,
    DesignExtrudeOperation,
    "coil_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_coil_operation_offset,
    u64,
    "coil_operation_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_coil_placement,
    DesignCoilPlacement,
    "coil_placement"
);
cadmpeg_core::named_optional_field!(deserialize_coil_section, DesignCoilSection, "coil_section");
cadmpeg_core::named_optional_field!(deserialize_coil_section_offset, u64, "coil_section_offset");
cadmpeg_core::named_optional_field!(
    deserialize_coil_section_placement,
    DesignCoilSectionPlacement,
    "coil_section_placement"
);
cadmpeg_core::named_optional_field!(
    deserialize_coil_section_placement_offset,
    u64,
    "coil_section_placement_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_coil_transform,
    DesignCoilTransform,
    "coil_transform"
);
cadmpeg_core::named_optional_field!(
    deserialize_curve_secondary_identity,
    u64,
    "curve_secondary_identity"
);
cadmpeg_core::named_optional_field!(deserialize_design_id, String, "design_id");
cadmpeg_core::named_optional_field!(
    deserialize_design_selector,
    ConstructionRecipeSelector,
    "design_selector"
);
cadmpeg_core::named_optional_field!(deserialize_secondary_identity, u64, "secondary_identity");
cadmpeg_core::named_optional_field!(deserialize_transform_offset, u64, "transform_offset");
/// Driving-dimension mode stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCoilExtent {
    /// Revolution count and total height are independent.
    RevolutionsHeight,
    /// Revolution count and pitch are independent.
    RevolutionsPitch,
    /// Total height and pitch are independent.
    HeightPitch,
    /// Revolution count and radial pitch define a planar spiral.
    Spiral,
}

/// Generated section family stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCoilSection {
    /// Circular section.
    Circular,
    /// Square section.
    Square,
    /// Triangular section pointing away from the axis.
    ExternalTriangle,
    /// Triangular section pointing toward the axis.
    InternalTriangle,
}

/// Radial section placement stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCoilSectionPlacement {
    /// Section inside the reference trajectory.
    Inside,
    /// Section centered on the reference trajectory.
    Center,
    /// Section outside the reference trajectory.
    Outside,
}

fn deserialize_coil_secondary_identity<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DesignSecondaryIdentity<u64>>, D::Error> {
    #[derive(Deserialize)]
    struct Wire {
        #[serde(default, deserialize_with = "deserialize_secondary_identity")]
        secondary_identity: Option<u64>,
        #[serde(default, deserialize_with = "deserialize_curve_secondary_identity")]
        curve_secondary_identity: Option<u64>,
    }
    let wire = Wire::deserialize(deserializer)?;
    DesignSecondaryIdentity::from_wire(wire.secondary_identity, wire.curve_secondary_identity)
        .map_err(serde::de::Error::custom)
}

fn deserialize_coil_recipe_design<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ConstructionRecipeDesign<String>>, D::Error> {
    #[derive(Deserialize)]
    struct Wire {
        #[serde(default, deserialize_with = "deserialize_design_id")]
        design_id: Option<String>,
        #[serde(default, deserialize_with = "deserialize_design_selector")]
        design_selector: Option<ConstructionRecipeSelector>,
    }
    let wire = Wire::deserialize(deserializer)?;
    match (wire.design_id, wire.design_selector) {
        (Some(id), selector) => Ok(Some(ConstructionRecipeDesign { id, selector })),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(serde::de::Error::custom(
            "design_selector requires design_id",
        )),
    }
}

/// Selection carrier used by a compact Coil placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignCoilSelection {
    /// Nested entity-selection frame with one or two persistent identities.
    Persistent {
        /// Asset UUID qualifying the persistent selection namespace.
        asset_id: DesignRelaxedGuidText,
        /// Context UUID qualifying the persistent selection namespace.
        context_id: DesignRelaxedGuidText,
        /// Indexed nested record carrying the persistent identity pair.
        identity_record_index: u32,
        /// First persistent identity value.
        primary_identity: u64,
        /// Secondary identity and any dependent curve identity.
        #[serde(flatten, deserialize_with = "deserialize_coil_secondary_identity")]
        secondary: Option<DesignSecondaryIdentity<u64>>,
    },
    /// Face construction recipe carried by a placement selection frame.
    FaceRecipe {
        /// Asset UUID qualifying the recipe selection namespace.
        asset_id: DesignRelaxedGuidText,
        /// Context UUID qualifying the recipe selection namespace.
        context_id: DesignRelaxedGuidText,
        /// Indexed record containing the face recipe.
        recipe_record_index: u32,
        /// Byte offset of the face recipe record header.
        recipe_record_byte_offset: u64,
        /// Native construction-recipe arena identity.
        recipe_id: String,
        /// Exact face-recipe family.
        recipe_kind: DesignFaceRecipeKind,
        /// Recipe Design identity and its optional selector.
        #[serde(flatten, deserialize_with = "deserialize_coil_recipe_design")]
        design: Option<ConstructionRecipeDesign<String>>,
    },
}

/// Exact placement construction carried by a compact Coil scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignCoilPlacementWire")]
pub(crate) struct DesignCoilPlacement {
    /// First ordered placement-construction reference.
    pub(crate) selection_record_index: u32,
    /// Byte offset of the support selection frame header.
    pub(crate) selection_record_byte_offset: u64,
    /// Dynamic class tag of the support selection frame.
    pub(crate) selection_class_tag: DesignClassTag,
    /// Exact selection semantics carried by the first placement reference.
    pub(crate) selection: DesignCoilSelection,
    /// Second ordered placement-construction reference: the frame carrier.
    pub(crate) transform_record_index: u32,
    /// Byte offset of the frame carrier header.
    pub(crate) transform_record_byte_offset: u64,
    /// Dynamic class tag of the frame carrier.
    pub(crate) transform_class_tag: DesignClassTag,
    /// Explicit matrix and its byte offset; absent for the encoded identity form.
    pub(crate) explicit_transform: Option<Located<SketchPlacementMatrix>>,
}

#[cfg(test)]
thread_local! {
    pub(super) static COIL_PLACEMENT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCoilPlacement {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COIL_PLACEMENT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            selection_record_index: self.selection_record_index,
            selection_record_byte_offset: self.selection_record_byte_offset,
            selection_class_tag: self.selection_class_tag.clone(),
            selection: self.selection.clone(),
            transform_record_index: self.transform_record_index,
            transform_record_byte_offset: self.transform_record_byte_offset,
            transform_class_tag: self.transform_class_tag.clone(),
            explicit_transform: self.explicit_transform,
        }
    }
}

#[derive(Serialize)]
struct DesignCoilPlacementRef<'a> {
    selection_record_index: u32,
    selection_record_byte_offset: u64,
    selection_class_tag: &'a str,
    selection: &'a DesignCoilSelection,
    transform_record_index: u32,
    transform_record_byte_offset: u64,
    transform_class_tag: &'a str,
    transform: &'a SketchPlacementMatrix,
    #[serde(skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
}

impl Serialize for DesignCoilPlacement {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCoilPlacementRef {
            selection_record_index: self.selection_record_index,
            selection_record_byte_offset: self.selection_record_byte_offset,
            selection_class_tag: self.selection_class_tag.as_str(),
            selection: &self.selection,
            transform_record_index: self.transform_record_index,
            transform_record_byte_offset: self.transform_record_byte_offset,
            transform_class_tag: self.transform_class_tag.as_str(),
            transform: self.transform(),
            transform_offset: self.explicit_transform.map(|matrix| matrix.offset),
        }
        .serialize(serializer)
    }
}

impl DesignCoilPlacement {
    /// Row-major local-to-model matrix with translation in source centimetres.
    #[must_use]
    pub(crate) fn transform(&self) -> &SketchPlacementMatrix {
        self.explicit_transform
            .as_ref()
            .map_or(&SketchPlacementMatrix::IDENTITY, |matrix| &matrix.value)
    }
}

/// Exact placement construction carried by a compact Coil scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCoilPlacementWire {
    /// First ordered placement-construction reference.
    selection_record_index: u32,
    /// Byte offset of the support selection frame header.
    selection_record_byte_offset: u64,
    /// Dynamic class tag of the support selection frame.
    selection_class_tag: String,
    /// Exact selection semantics carried by the first placement reference.
    selection: DesignCoilSelection,
    /// Second ordered placement-construction reference: the frame carrier.
    transform_record_index: u32,
    /// Byte offset of the frame carrier header.
    transform_record_byte_offset: u64,
    /// Dynamic class tag of the frame carrier.
    transform_class_tag: String,
    /// Row-major local-to-model rigid transform. Matrix values are in source
    /// centimetres for the translation column.
    transform: SketchPlacementMatrix,
    /// Byte offset of the matrix, or absent for the encoded identity form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
}

impl TryFrom<DesignCoilPlacementWire> for DesignCoilPlacement {
    type Error = String;
    fn try_from(wire: DesignCoilPlacementWire) -> Result<Self, Self::Error> {
        let explicit_transform = match wire.transform_offset {
            Some(offset) => Some(Located {
                value: wire.transform,
                offset,
            }),
            None => {
                if wire
                    .transform
                    .iter()
                    .flatten()
                    .zip(IDENTITY_MATRIX.iter().flatten())
                    .any(|(value, identity)| value.to_bits() != identity.to_bits())
                {
                    return Err("transform must be identity when transform_offset is absent".into());
                }
                None
            }
        };
        Ok(Self {
            selection_record_index: wire.selection_record_index,
            selection_record_byte_offset: wire.selection_record_byte_offset,
            selection_class_tag: wire.selection_class_tag.try_into()?,
            selection: wire.selection,
            transform_record_index: wire.transform_record_index,
            transform_record_byte_offset: wire.transform_record_byte_offset,
            transform_class_tag: wire.transform_class_tag.try_into()?,
            explicit_transform,
        })
    }
}

#[cfg(test)]
impl From<DesignCoilPlacement> for DesignCoilPlacementWire {
    fn from(record: DesignCoilPlacement) -> Self {
        let transform = *record.transform();
        Self {
            selection_record_index: record.selection_record_index,
            selection_record_byte_offset: record.selection_record_byte_offset,
            selection_class_tag: record.selection_class_tag.into(),
            selection: record.selection,
            transform_record_index: record.transform_record_index,
            transform_record_byte_offset: record.transform_record_byte_offset,
            transform_class_tag: record.transform_class_tag.into(),
            transform,
            transform_offset: record.explicit_transform.map(|matrix| matrix.offset),
        }
    }
}

/// Direct rigid placement carried by the long ten-reference Coil form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignCoilTransform {
    /// Row-major local-to-model rigid transform. Translation is in source
    /// centimetres.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub(crate) transform_offset: u64,
}

/// Coil-specific records carried by a Coil parameter scope.
#[derive(Debug, PartialEq, Default, Deserialize)]
#[serde(try_from = "DesignCoilScopeWire")]
// Field names are the native record serialized keys.
pub(crate) struct DesignCoilScope {
    pub(crate) operation: Option<RecordedValue<DesignExtrudeOperation>>,
    pub(crate) extent: Option<MaybeRecordedValue<DesignCoilExtent>>,
    pub(crate) section: Option<MaybeRecordedValue<DesignCoilSection>>,
    pub(crate) section_placement: Option<MaybeRecordedValue<DesignCoilSectionPlacement>>,
    pub(crate) clockwise: Option<MaybeRecordedValue<bool>>,
    pub(crate) placement: Option<DesignCoilPlacement>,
    pub(crate) transform: Option<DesignCoilTransform>,
}

#[cfg(test)]
thread_local! {
    pub(super) static COIL_SCOPE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCoilScope {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COIL_SCOPE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            operation: self.operation,
            extent: self.extent,
            section: self.section,
            section_placement: self.section_placement,
            clockwise: self.clockwise,
            placement: self.placement.clone(),
            transform: self.transform.clone(),
        }
    }
}

#[derive(Serialize)]
struct DesignCoilScopeRef<'a> {
    #[serde(rename = "coil_operation")]
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<DesignExtrudeOperation>,
    #[serde(rename = "coil_operation_offset")]
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_offset: Option<u64>,
    #[serde(rename = "coil_extent")]
    #[serde(skip_serializing_if = "Option::is_none")]
    extent: Option<DesignCoilExtent>,
    #[serde(rename = "coil_extent_offset")]
    #[serde(skip_serializing_if = "Option::is_none")]
    extent_offset: Option<u64>,
    #[serde(rename = "coil_section")]
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<DesignCoilSection>,
    #[serde(rename = "coil_section_offset")]
    #[serde(skip_serializing_if = "Option::is_none")]
    section_offset: Option<u64>,
    #[serde(rename = "coil_section_placement")]
    #[serde(skip_serializing_if = "Option::is_none")]
    section_placement: Option<DesignCoilSectionPlacement>,
    #[serde(rename = "coil_section_placement_offset")]
    #[serde(skip_serializing_if = "Option::is_none")]
    section_placement_offset: Option<u64>,
    #[serde(rename = "coil_clockwise")]
    #[serde(skip_serializing_if = "Option::is_none")]
    clockwise: Option<bool>,
    #[serde(rename = "coil_clockwise_offset")]
    #[serde(skip_serializing_if = "Option::is_none")]
    clockwise_offset: Option<u64>,
    #[serde(rename = "coil_placement")]
    #[serde(skip_serializing_if = "Option::is_none")]
    placement: Option<&'a DesignCoilPlacement>,
    #[serde(rename = "coil_transform")]
    #[serde(skip_serializing_if = "Option::is_none")]
    transform: Option<&'a DesignCoilTransform>,
}

impl Serialize for DesignCoilScope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCoilScopeRef {
            operation: self.operation.map(|field| field.value),
            operation_offset: self.operation.map(|field| field.offset),
            extent: self.extent.map(|field| field.value()),
            extent_offset: self.extent.and_then(|field| field.offset()),
            section: self.section.map(|field| field.value()),
            section_offset: self.section.and_then(|field| field.offset()),
            section_placement: self.section_placement.map(|field| field.value()),
            section_placement_offset: self.section_placement.and_then(|field| field.offset()),
            clockwise: self.clockwise.map(|field| field.value()),
            clockwise_offset: self.clockwise.and_then(|field| field.offset()),
            placement: self.placement.as_ref(),
            transform: self.transform.as_ref(),
        }
        .serialize(serializer)
    }
}

/// Coil-specific records carried by a Coil parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
// Field names are the native record serialized keys.
struct DesignCoilScopeWire {
    /// Coil result operation from the fixed scope prologue.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_operation"
    )]
    #[serde(rename = "coil_operation")]
    operation: Option<DesignExtrudeOperation>,
    /// Byte offset of the Coil operation enum.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_operation_offset"
    )]
    #[serde(rename = "coil_operation_offset")]
    operation_offset: Option<u64>,
    /// Coil driving-dimension mode.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_extent"
    )]
    #[serde(rename = "coil_extent")]
    extent: Option<DesignCoilExtent>,
    /// Byte offset of the Coil mode enum, when the form stores one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_extent_offset"
    )]
    #[serde(rename = "coil_extent_offset")]
    extent_offset: Option<u64>,
    /// Generated Coil section family.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_section"
    )]
    #[serde(rename = "coil_section")]
    section: Option<DesignCoilSection>,
    /// Byte offset of the Coil section enum, when the form stores one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_section_offset"
    )]
    #[serde(rename = "coil_section_offset")]
    section_offset: Option<u64>,
    /// Radial placement of the generated Coil section.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_section_placement"
    )]
    #[serde(rename = "coil_section_placement")]
    section_placement: Option<DesignCoilSectionPlacement>,
    /// Byte offset of the Coil section-placement enum, when the form stores one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_section_placement_offset"
    )]
    #[serde(rename = "coil_section_placement_offset")]
    section_placement_offset: Option<u64>,
    /// Whether Coil angular travel is clockwise.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_clockwise"
    )]
    #[serde(rename = "coil_clockwise")]
    clockwise: Option<bool>,
    /// Byte offset of the Coil direction enum, when the form stores one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_clockwise_offset"
    )]
    #[serde(rename = "coil_clockwise_offset")]
    clockwise_offset: Option<u64>,
    /// Exact placement construction carried by a compact Coil scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_placement"
    )]
    #[serde(rename = "coil_placement")]
    placement: Option<DesignCoilPlacement>,
    /// Direct rigid placement carried by the long ten-reference Coil form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_transform"
    )]
    #[serde(rename = "coil_transform")]
    transform: Option<DesignCoilTransform>,
}

impl TryFrom<DesignCoilScopeWire> for DesignCoilScope {
    type Error = String;
    fn try_from(wire: DesignCoilScopeWire) -> Result<Self, Self::Error> {
        Ok(Self {
            operation: RecordedValue::from_wire(
                wire.operation,
                wire.operation_offset,
                "coil_operation",
            )?,
            extent: MaybeRecordedValue::from_wire(wire.extent, wire.extent_offset, "coil_extent")?,
            section: MaybeRecordedValue::from_wire(
                wire.section,
                wire.section_offset,
                "coil_section",
            )?,
            section_placement: MaybeRecordedValue::from_wire(
                wire.section_placement,
                wire.section_placement_offset,
                "coil_section_placement",
            )?,
            clockwise: MaybeRecordedValue::from_wire(
                wire.clockwise,
                wire.clockwise_offset,
                "coil_clockwise",
            )?,
            placement: wire.placement,
            transform: wire.transform,
        })
    }
}

#[cfg(test)]
impl From<DesignCoilScope> for DesignCoilScopeWire {
    fn from(value: DesignCoilScope) -> Self {
        Self {
            operation: value.operation.map(|field| field.value),
            operation_offset: value.operation.map(|field| field.offset),
            extent: value.extent.map(|field| field.value()),
            extent_offset: value.extent.and_then(|field| field.offset()),
            section: value.section.map(|field| field.value()),
            section_offset: value.section.and_then(|field| field.offset()),
            section_placement: value.section_placement.map(|field| field.value()),
            section_placement_offset: value.section_placement.and_then(|field| field.offset()),
            clockwise: value.clockwise.map(|field| field.value()),
            clockwise_offset: value.clockwise.and_then(|field| field.offset()),
            placement: value.placement,
            transform: value.transform,
        }
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
