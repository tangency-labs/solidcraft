//! The design's parameters.

use serde::{Deserialize, Serialize};

use super::geometry::Quantity;
use super::ids::ParameterName;

/// One parameter: a user parameter, a sketch dimension, a feature input or a text parameter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Parameter {
    pub name: ParameterName,
    /// The expression as the design stores it.
    pub expression: String,
    pub value: ParameterValue,
    /// The unit the design shows it in (`mm`, `in`, `deg`, empty when unit-less).
    pub display_unit: String,
    pub source: ParameterSource,
    pub comment: String,
}

/// A parameter's value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ParameterValue {
    /// A length, angle or number (lengths in mm, angles in radians).
    Quantity(Quantity),
    Text(String),
    /// The expression does not evaluate.
    #[default]
    Invalid,
}

/// Where a parameter comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ParameterSource {
    /// Made by the user in the parameters table.
    User,
    /// A sketch dimension.
    Sketch,
    /// A feature's input (an extrude's distance, a fillet's radius…).
    Feature,
    /// A source this version does not know.
    #[default]
    #[serde(other)]
    Other,
}
