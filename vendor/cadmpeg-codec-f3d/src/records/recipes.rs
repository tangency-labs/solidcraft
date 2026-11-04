// SPDX-License-Identifier: Apache-2.0
//! Construction recipes and the naming space and timestamp a component records.

use super::identity::RecordedValue;
use super::mesh::DesignRelaxedGuidText;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_design_id, String, "design_id");
cadmpeg_core::named_optional_field!(deserialize_design_id_offset, u64, "design_id_offset");
cadmpeg_core::named_optional_field!(
    deserialize_design_selector,
    ConstructionRecipeSelector,
    "design_selector"
);
cadmpeg_core::named_optional_field!(deserialize_record_index, i32, "record_index");
cadmpeg_core::named_optional_field!(deserialize_record_index_offset, u64, "record_index_offset");
cadmpeg_core::named_optional_field!(deserialize_component_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_component_name_offset, u64, "name_offset");
/// Component-local Design naming space bound to a context UUID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignComponentNamingSpace {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the binding marker in its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Component entity id named by the binding.
    pub(crate) component_record_index: u64,
    /// UUID used by persistent identities to select this component.
    pub(crate) context_uuid: DesignRelaxedGuidText,
    /// Byte offset of the UUID length prefix.
    pub(crate) context_uuid_offset: u64,
    /// The component's name, stored after the context UUID at the end of the
    /// component's definition record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component_name"
    )]
    pub(crate) name: Option<String>,
    /// Byte offset of the name's length prefix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component_name_offset"
    )]
    pub(crate) name_offset: Option<u64>,
}

/// Original authoring time attached to a solved ASM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CreationTimestamp {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Solved B-rep entity carrying the timestamp attribute.
    pub(crate) target: AttributeTarget,
    /// Source SAB record index of the timestamp attribute.
    pub(crate) record_index: u32,
    /// Creation time as microseconds since the Unix epoch.
    pub(crate) unix_microseconds: FiniteReal,
}

/// Design `BulkStream` regeneration-recipe family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConstructionRecipeKind {
    /// Recipe regenerates a whole body.
    Body,
    /// Recipe regenerates a single face.
    Face,
    /// Recipe regenerates a face bounded by an explicit region.
    BoundedFace,
    /// Recipe regenerates a single edge.
    Edge,
    /// Recipe regenerates a single vertex.
    Vertex,
}

/// A recipe Design id and its optional following selector.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ConstructionRecipeDesign<Id> {
    #[serde(rename = "design_id")]
    pub(crate) id: Id,
    #[serde(rename = "design_selector", skip_serializing_if = "Option::is_none")]
    pub(crate) selector: Option<ConstructionRecipeSelector>,
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ConstructionRecipeWire")]
pub(crate) struct ConstructionRecipe {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of this recipe's family marker in its Design `BulkStream`.
    pub(crate) byte_offset: u64,
    /// Topology kind this recipe regenerates on replay.
    pub(crate) kind: ConstructionRecipeKind,
    /// Design identity carried by the recipe; absent for recipes without a body key.
    pub(crate) design: Option<ConstructionRecipeDesign<RecordedValue<String>>>,
    /// Position of this recipe in the `BulkStream` recipe sequence, in source order.
    pub(crate) recipe_index: u32,
    /// Source `BulkStream` record index this recipe was decoded from, with the
    /// byte offset it was read at; `None` when the recipe's family marker opens
    /// the stream too early for the index word to precede it, so the stream
    /// states no record index for this recipe.
    pub(crate) record_index: Option<RecordedValue<i32>>,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_RECIPE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ConstructionRecipe {
    fn clone(&self) -> Self {
        CONSTRUCTION_RECIPE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            kind: self.kind,
            design: self.design.clone(),
            recipe_index: self.recipe_index,
            record_index: self.record_index,
        }
    }
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ConstructionRecipeWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Byte offset of this recipe's family marker in its Design `BulkStream`.
    byte_offset: u64,
    /// Byte offset of `record_index` in the Design `BulkStream`, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_record_index_offset"
    )]
    record_index_offset: Option<u64>,
    /// Topology kind this recipe regenerates on replay.
    kind: ConstructionRecipeKind,
    /// Design entity id of the body this recipe is keyed to, if the source record
    /// carried a `generic_tag_attrib_def` construction id; `None` for body-less recipes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_design_id"
    )]
    design_id: Option<String>,
    /// Byte offset of `design_id` in the Design `BulkStream`, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_design_id_offset"
    )]
    design_id_offset: Option<u64>,
    /// Selector following the Design entity id, when the recipe carries that id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_design_selector"
    )]
    design_selector: Option<ConstructionRecipeSelector>,
    /// Position of this recipe in the `BulkStream` recipe sequence, in source order.
    recipe_index: u32,
    /// Source `BulkStream` record index this recipe was decoded from, when the
    /// stream states one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_record_index"
    )]
    record_index: Option<i32>,
}

impl Serialize for ConstructionRecipe {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            byte_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            record_index_offset: Option<u64>,
            kind: ConstructionRecipeKind,
            #[serde(skip_serializing_if = "Option::is_none")]
            design_id: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            design_id_offset: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            design_selector: Option<ConstructionRecipeSelector>,
            recipe_index: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            record_index: Option<i32>,
        }
        let design = self.design.as_ref();
        WireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            record_index_offset: self.record_index.map(|record| record.offset),
            kind: self.kind,
            design_id: design.map(|design| design.id.value.as_str()),
            design_id_offset: design.map(|design| design.id.offset),
            design_selector: design.and_then(|design| design.selector),
            recipe_index: self.recipe_index,
            record_index: self.record_index.map(|record| record.value),
        }
        .serialize(serializer)
    }
}

impl TryFrom<ConstructionRecipeWire> for ConstructionRecipe {
    type Error = String;
    fn try_from(wire: ConstructionRecipeWire) -> Result<Self, Self::Error> {
        let id = RecordedValue::from_wire(wire.design_id, wire.design_id_offset, "design_id")?;
        let design = match (id, wire.design_selector) {
            (Some(id), selector) => Some(ConstructionRecipeDesign { id, selector }),
            (None, None) => None,
            (None, Some(_)) => {
                return Err("construction recipe design_selector requires design_id".into())
            }
        };
        let record_index =
            RecordedValue::from_wire(wire.record_index, wire.record_index_offset, "record_index")?;
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            kind: wire.kind,
            design,
            recipe_index: wire.recipe_index,
            record_index,
        })
    }
}

#[cfg(test)]
impl From<ConstructionRecipe> for ConstructionRecipeWire {
    fn from(value: ConstructionRecipe) -> Self {
        let (design_id, design_id_offset, design_selector) = match value.design {
            Some(design) => (
                Some(design.id.value),
                Some(design.id.offset),
                design.selector,
            ),
            None => (None, None, None),
        };
        let (record_index, record_index_offset) = match value.record_index {
            Some(record_index) => (Some(record_index.value), Some(record_index.offset)),
            None => (None, None),
        };
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            record_index_offset,
            kind: value.kind,
            design_id,
            design_id_offset,
            design_selector,
            recipe_index: value.recipe_index,
            record_index,
        }
    }
}

/// Serialized Design selector carried by a construction recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConstructionRecipeSelector {
    /// Selector value.
    pub(crate) value: u32,
    /// Byte offset of `value`.
    pub(crate) byte_offset: u64,
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
