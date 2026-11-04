//! Contracts and result types for the commands in `cmd/material_properties.rs`: checking an
//! engineering material card, and its elastic modulus in a direction (#11036).

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::physical_material::{DirectionalModulus, IsotropicModuli, MaterialAxes, Validation};

use super::check::whole;

/// Check an engineering material card: its numbers' consistency, the isotropic moduli they
/// imply, and the conditions and sources still to review. Read-only: nothing is assigned or
/// saved, and there is no analysis solver.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ValidateCardParams {
    #[schemars(schema_with = "card_schema")]
    pub card: Value,
}

/// The Young modulus of an engineering material card in one direction, in the card's own
/// material axes. Read-only: no load case or structural solver.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DirectionalModulusParams {
    #[schemars(schema_with = "card_schema")]
    pub card: Value,
    pub direction: CardDirection,
}

/// The card, checked by the handler (`admit_card`), with its messages.
fn card_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A schema_version 1 engineering material card in SI units (see docs/physical-material-cards.md).",
        "type": "object"
    })
}

/// A direction in the card's material axes: three numbers, not all zero, with the handler's
/// messages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardDirection(pub [f64; 3]);

impl<'de> Deserialize<'de> for CardDirection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let a =
            v.as_array().filter(|a| a.len() == 3).ok_or_else(|| D::Error::custom(whole("direction: expected exactly three numeric components")))?;
        let c = |i: usize| a.get(i).and_then(Value::as_f64).ok_or_else(|| D::Error::custom(whole("direction: expected numeric components")));
        Ok(CardDirection([c(0)?, c(1)?, c(2)?]))
    }
}

impl JsonSchema for CardDirection {
    fn schema_name() -> Cow<'static, str> {
        "CardDirection".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A direction in the card's material axes: three numbers, not all zero.",
            "type": "array",
            "items": {"type": "number"},
            "minItems": 3,
            "maxItems": 3
        })
    }
}

/// What `material.properties.validate` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CardValidated {
    /// Always true: a card that isn't valid is refused.
    pub valid: bool,
    pub validation: CardValidation,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `material.properties.directional` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CardModulus {
    /// The direction, made a unit vector.
    pub direction: [f64; 3],
    /// The card's material axes, when it has directional properties.
    pub axes: Option<CardAxes>,
    /// The Young modulus in that direction (Pa).
    pub young_modulus_pa: f64,
    pub validation: CardValidation,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

impl From<DirectionalModulus> for CardModulus {
    fn from(m: DirectionalModulus) -> Self {
        let DirectionalModulus { direction, axes, young_modulus_pa, validation } = m;
        CardModulus { direction, axes: axes.map(CardAxes::from), young_modulus_pa, validation: validation.into(), param_warnings: Vec::new() }
    }
}

/// A card's material axes.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CardAxes {
    /// The material's own axes 1, 2 and 3.
    Material123,
    /// Wood's longitudinal, radial and tangential axes.
    WoodLrt,
}

impl From<MaterialAxes> for CardAxes {
    fn from(a: MaterialAxes) -> Self {
        match a {
            MaterialAxes::Material123 => CardAxes::Material123,
            MaterialAxes::WoodLrt => CardAxes::WoodLrt,
        }
    }
}

/// What checking a card found.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CardValidation {
    pub schema_version: u32,
    /// The kinds of property the card gives.
    pub property_kinds: Vec<String>,
    /// The shear and bulk moduli an isotropic card's numbers imply.
    pub isotropic_moduli: Option<CardModuli>,
    /// The property kinds given without every condition they hold at (temperature, state, and
    /// moisture for wood or strain rate for polymers).
    pub unspecified_conditions: Vec<String>,
    /// Per property kind: the condition fields it lacks.
    pub missing_condition_fields: BTreeMap<String, Vec<String>>,
    /// The sources the card's properties cite, which still need review.
    pub source_review_required: Vec<String>,
    /// Always false: there is no analysis solver.
    pub analysis_solver_available: bool,
    /// Always false: a card holds no rendering properties.
    pub rendering_properties_available: bool,
}

impl From<Validation> for CardValidation {
    fn from(v: Validation) -> Self {
        // In full, so a field added to `Validation` fails here rather than going missing.
        let Validation {
            schema_version,
            property_kinds,
            isotropic_moduli,
            unspecified_conditions,
            missing_condition_fields,
            source_review_required,
            analysis_solver_available,
            rendering_properties_available,
        } = v;
        CardValidation {
            schema_version,
            property_kinds,
            isotropic_moduli: isotropic_moduli.map(|IsotropicModuli { shear_pa, bulk_pa }| CardModuli { shear_pa, bulk_pa }),
            unspecified_conditions,
            missing_condition_fields,
            source_review_required,
            analysis_solver_available,
            rendering_properties_available,
        }
    }
}

/// The isotropic moduli a card's numbers imply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CardModuli {
    pub shear_pa: f64,
    pub bulk_pa: f64,
}

#[cfg(test)]
#[path = "material_properties_tests.rs"]
mod tests;
