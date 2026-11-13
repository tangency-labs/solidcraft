//! Contracts and result types for the design's own physical materials, in `cmd/materials.rs`
//! (#11862): `material.create`, `material.edit`, `material.inspect` and `material.delete`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Two things the handlers rely on stay as they
//! are, and the contracts keep them:
//! - `null` means something: `color: null` clears the look, and `physical_card: null` clears
//!   the card in an edit. Left out, they keep what the material has.
//! - The look keys are the appearance commands' (`ColorParam`, `PbrParam`), and the card stays
//!   the handler's to check (`ValidatedCard::from_value`), with its schema from
//!   `schema/material_properties.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::appearance::{ColorParam, LookReply, PbrParam};
use super::check::whole;
use super::material_properties::CardValidation;

/// `material`: the material the command works on, by name. Anything but text gets the
/// handler's message (`str_` reads text only).
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialRef(pub String);

impl<'de> Deserialize<'de> for MaterialRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(name) => Ok(MaterialRef(name)),
            _ => Err(D::Error::custom(whole("`material` must be a material name"))),
        }
    }
}

impl JsonSchema for MaterialRef {
    fn schema_name() -> Cow<'static, str> {
        "MaterialRef".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A material, by name (letter case doesn't matter).", "type": "string"})
    }
}

/// `density_g_cm3`: a density in g/cm³ above 0 and at most 1000, as `materials::density` reads
/// it, with its message for anything else, `null` included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Density(pub f64);

impl<'de> Deserialize<'de> for Density {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_f64()
            .filter(|density| solidcraft_doc::valid_density(*density))
            .map(Density)
            .ok_or_else(|| D::Error::custom(whole("`density_g_cm3` must be a number above 0 and at most 1000")))
    }
}

impl JsonSchema for Density {
    fn schema_name() -> Cow<'static, str> {
        "Density".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The density in g/cm³: above 0 and at most 1000.",
            "type": "number",
            "exclusiveMinimum": 0,
            "maximum": 1000
        })
    }
}

/// An edit's `density_g_cm3`: given, it must be a density, `null` included, as the handler
/// checks it whenever the key is there; left out, the material keeps its density.
fn some_density<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Density>, D::Error> {
    Density::deserialize(d).map(Some)
}

/// `opacity`: 0 (clear) to 1 (opaque), as `materials::look` reads it whenever the key is there,
/// with its message for anything else, `null` included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opacity(pub f64);

impl<'de> Deserialize<'de> for Opacity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_f64()
            .filter(|o| o.is_finite() && (0.0..=1.0).contains(o))
            .map(Opacity)
            .ok_or_else(|| D::Error::custom(whole("`opacity` must be 0…1")))
    }
}

impl JsonSchema for Opacity {
    fn schema_name() -> Cow<'static, str> {
        "Opacity".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "0 (clear) to 1 (opaque).", "type": "number", "minimum": 0, "maximum": 1})
    }
}

fn some_opacity<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Opacity>, D::Error> {
    Opacity::deserialize(d).map(Some)
}

/// `color`: a colour, or `null` to clear the look; the handler tells the two apart, so `null`
/// isn't read as left out.
fn color_or_clear<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<ColorParam>>, D::Error> {
    Option::<ColorParam>::deserialize(d).map(Some)
}

fn color_schema(generator: &mut SchemaGenerator) -> Schema {
    let color = generator.subschema_for::<ColorParam>();
    json_schema!({
        "description": "The look's colour, `\"#rrggbb\"` or [r, g, b] (0–255), not with `appearance`. `null` clears the material's look, so its bodies show the default.",
        "anyOf": [color, {"type": "null"}]
    })
}

/// `pbr`: the look's opaque PBR response, or `null` for scalar shading; it needs `appearance` or
/// `color`. `null` and left out differ for a library appearance (left out keeps its own).
fn pbr_or_scalar<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<PbrParam>>, D::Error> {
    Option::<PbrParam>::deserialize(d).map(Some)
}

fn pbr_schema(generator: &mut SchemaGenerator) -> Schema {
    let pbr = generator.subschema_for::<PbrParam>();
    json_schema!({
        "description": "The look's opaque PBR response; it needs `appearance` or `color`, and an opaque look. `null` selects scalar shading. Left out, a library appearance keeps its own response.",
        "anyOf": [pbr, {"type": "null"}]
    })
}

/// `physical_card`: the handler checks the card itself (`ValidatedCard::from_value`), with its
/// messages, so the contract takes any value here and describes it.
fn card_or_clear<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

fn card_schema(generator: &mut SchemaGenerator) -> Schema {
    let card = super::material_properties::card_schema(generator);
    json_schema!({
        "description": "A validated physical material card in SI units (see docs/physical-material-cards.md). The card's density never changes the assigned density. `null`: no card (an edit clears it). Left out: none for a new material; an edit keeps the current one.",
        "anyOf": [card, {"type": "null"}]
    })
}

/// Define a physical material in this design: a name, a density and an optional default look
/// and physical card. Assign it with `material.assign`. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MaterialCreateParams {
    /// The new material's name: 1 to 128 characters, not a built-in material's or another of the
    /// design's (letter case doesn't matter). Spaces at either end are dropped.
    pub name: String,
    pub density_g_cm3: Density,
    /// The look, a library appearance by name (see `appearance.library`). Not with `color`.
    #[serde(default)]
    pub appearance: Option<String>,
    #[serde(default, deserialize_with = "color_or_clear")]
    #[schemars(schema_with = "color_schema")]
    pub color: Option<Option<ColorParam>>,
    /// The look's opacity: needs `appearance` or `color`. Default: 1, or the appearance's own.
    #[serde(default, deserialize_with = "some_opacity")]
    #[schemars(with = "Opacity")]
    pub opacity: Option<Opacity>,
    #[serde(default, deserialize_with = "pbr_or_scalar")]
    #[schemars(schema_with = "pbr_schema")]
    pub pbr: Option<Option<PbrParam>>,
    #[serde(default, deserialize_with = "card_or_clear")]
    #[schemars(schema_with = "card_schema")]
    pub physical_card: Option<Value>,
}

/// Change a physical material this design defines: its name (its bodies and configuration cells
/// follow), its density, its look or its card. A key left out keeps what the material has; with
/// no look key at all, the whole look is kept. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MaterialEditParams {
    /// The material, one this design defines; built-in materials can't be changed.
    pub material: MaterialRef,
    /// Its new name, as for `material.create`.
    #[serde(default)]
    pub name: Option<String>,
    /// Its new density.
    #[serde(default, deserialize_with = "some_density")]
    #[schemars(with = "Density")]
    pub density_g_cm3: Option<Density>,
    /// The new look, a library appearance by name. Not with `color`.
    #[serde(default)]
    pub appearance: Option<String>,
    #[serde(default, deserialize_with = "color_or_clear")]
    #[schemars(schema_with = "color_schema")]
    pub color: Option<Option<ColorParam>>,
    /// The new look's opacity: needs `appearance` or `color`.
    #[serde(default, deserialize_with = "some_opacity")]
    #[schemars(with = "Opacity")]
    pub opacity: Option<Opacity>,
    #[serde(default, deserialize_with = "pbr_or_scalar")]
    #[schemars(schema_with = "pbr_schema")]
    pub pbr: Option<Option<PbrParam>>,
    #[serde(default, deserialize_with = "card_or_clear")]
    #[schemars(schema_with = "card_schema")]
    pub physical_card: Option<Value>,
}

/// Read a material, built-in or this design's: its density, default look and physical card,
/// with the card's check and how its density compares. The design doesn't change.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MaterialInspectParams {
    /// A built-in material or one this design defines.
    pub material: MaterialRef,
}

/// Delete a physical material this design defines. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MaterialDeleteParams {
    /// The material, one this design defines.
    pub material: MaterialRef,
    /// Another material, built-in or this design's, for the bodies and configuration cells that
    /// use the deleted one. Without it, a material in use is refused.
    #[serde(default)]
    pub replace: Option<String>,
}

/// What `material.create` and `material.edit` return: the material as the design now defines it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterialReply {
    pub name: String,
    /// The assigned density (g/cm³), the one masses use.
    pub density_g_cm3: f64,
    /// The default look; `null` when it has none.
    pub appearance: Option<LookReply>,
    /// Always true: a material this design defines.
    pub user: bool,
    /// Its physical card, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_card: Option<Value>,
    /// The card's check, when it has a card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_validation: Option<CardValidation>,
    /// How the card's measured density compares with the assigned one, when the card gives one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density_comparison: Option<DensityComparison>,
    /// After an edit: the bodies the material is assigned to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bodies: Option<Vec<String>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `material.inspect` returns. Unlike [`MaterialReply`], `physical_card` is always there,
/// `null` without a card.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterialInspected {
    pub name: String,
    /// The assigned density (g/cm³), the one masses use.
    pub density_g_cm3: f64,
    /// The default look; `null` when it has none.
    pub appearance: Option<LookReply>,
    /// Whether this design defines it (`false`: a built-in material).
    pub user: bool,
    /// Its physical card; `null` when it has none.
    pub physical_card: Option<Value>,
    /// The card's check, when it has a card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_validation: Option<CardValidation>,
    /// How the card's measured density compares with the assigned one, when the card gives one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density_comparison: Option<DensityComparison>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A card's measured density against the material's assigned one.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DensityComparison {
    pub assigned_density_kg_m3: f64,
    pub card_density_kg_m3: f64,
    /// Their difference as a share of the larger.
    pub relative_difference: f64,
    /// Whether they agree within `relative_tolerance`.
    pub matches_within_relative_tolerance: bool,
    /// Always 1e-9.
    pub relative_tolerance: f64,
}

/// What `material.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterialDeleted {
    /// The deleted material's name.
    pub deleted: String,
    /// The material its bodies and configuration cells now use; `null` when it wasn't in use.
    pub replaced_by: Option<String>,
    /// The bodies that used it.
    pub bodies: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "materials_tests.rs"]
mod tests;
