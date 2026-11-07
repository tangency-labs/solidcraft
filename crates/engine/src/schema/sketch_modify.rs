//! Contracts and result types for the commands in `cmd/sketch_modify.rs`: the sketch pattern
//! limit (#7824).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A pattern limit as the `sketch.pattern_limit` handler reads one: a whole number from 2 to
/// `MAX_PATTERN_COPIES`, with its message for anything else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternLimitValue(pub usize);

impl<'de> Deserialize<'de> for PatternLimitValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        crate::cmd::pattern_limit_value(&v).map(PatternLimitValue).ok_or_else(|| D::Error::custom(crate::cmd::pattern_limit_problem()))
    }
}

impl JsonSchema for PatternLimitValue {
    fn schema_name() -> Cow<'static, str> {
        "PatternLimitValue".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The most instances, the original included, one sketch pattern may make: a whole number from 2 to 20000.",
            "type": "number",
            "multipleOf": 1,
            "minimum": 2,
            "maximum": solidcraft_sketch::MAX_PATTERN_COPIES
        })
    }
}

/// Set or report the most instances, the original included, a sketch pattern command makes
/// (default 500). The user's setting: kept across opened and new designs, not saved in a
/// design, not an undo step. A pattern made before under a higher limit keeps its own size
/// when it is edited. Never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PatternLimitParams {
    /// The new limit, a whole number from 2 to 20000. Default: leave it as it is and report it.
    #[serde(default)]
    pub value: Option<PatternLimitValue>,
}

/// What `sketch.pattern_limit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PatternLimitResult {
    /// The limit now in force.
    pub pattern_limit: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_modify_tests.rs"]
mod tests;
