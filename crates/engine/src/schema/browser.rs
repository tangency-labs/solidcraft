//! Contracts and result types for the commands in `cmd/browser.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// The units a design can show lengths in.
pub const DESIGN_UNITS: [&str; 5] = ["mm", "cm", "m", "in", "ft"];

/// One of [`DESIGN_UNITS`], with `document.units`' message for anything else.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignUnits(pub String);

impl<'de> Deserialize<'de> for DesignUnits {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(u) if DESIGN_UNITS.contains(&u.as_str()) => Ok(DesignUnits(u)),
            _ => Err(D::Error::custom("must be mm, cm, m, in or ft")),
        }
    }
}

impl JsonSchema for DesignUnits {
    fn schema_name() -> Cow<'static, str> {
        "DesignUnits".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "The units the design shows lengths in.", "type": "string", "enum": DESIGN_UNITS})
    }
}

/// Change the units the design shows lengths in, and reads a bare number in. Values keep their
/// meaning: a unit-less parameter holding a bare number that lengths read is written with the
/// old unit (`10` becomes `10 mm`) when that keeps those lengths where they were. One undo
/// step; the same units again changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DocumentUnitsParams {
    /// The design's new units: mm, cm, m, in or ft.
    pub units: DesignUnits,
}

/// What `document.units` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentUnitsResult {
    /// The design's units now.
    pub units: String,
    /// The unit-less parameters now written with the old unit, so the lengths that read them
    /// stay where they were. Empty when nothing needed it, or the units didn't change.
    pub kept: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
