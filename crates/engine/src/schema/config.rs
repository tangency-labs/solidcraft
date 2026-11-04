//! Contracts and result types for the configuration commands in `cmd/config.rs` that read the
//! table and switch between its rows (#11059).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;

/// The configuration table: its columns, rows and rules, the active row, and whether the design
/// still matches it. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConfigTableParams {}

/// Switch to a configuration row: its parameter values, suppressions, materials and appearances
/// are set, and its rules run. Refused, with nothing changed, when a feature that builds now
/// would fail in it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConfigRowParams {
    /// The row, by name.
    #[serde(deserialize_with = "row")]
    pub row: String,
}

/// The row's name, with the handler's message for anything that isn't text.
fn row<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`row` is required"))),
    }
}

/// The configuration table, as `config.table` and `config.rules` return it and an activation
/// returns it after the switch.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigTable {
    /// Each column's key: `param:<name>`, `suppress:<feature>`, or a material or appearance
    /// column's.
    pub columns: Vec<String>,
    /// Each column: its key, a label to show, and what it drives.
    pub targets: Vec<ConfigTarget>,
    /// The rows, in order.
    pub rows: Vec<ConfigTableRow>,
    /// The rules, in the order they run: `{id, name, enabled, condition, effect}` each.
    pub rules: Vec<Value>,
    /// The active row's name, or `null`.
    pub active: Option<String>,
    /// Whether the design still has the active row's values.
    pub matches: bool,
    /// Each style column's state, when it could be worked out.
    pub style_status: Option<Vec<Value>>,
    /// Why the style columns' state couldn't be worked out.
    pub style_error: Option<String>,
}

/// One column of the table.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigTarget {
    pub key: String,
    /// What to show for it, such as `Component: Bracket`.
    pub label: String,
    /// What it drives, as the design file saves it.
    pub target: Value,
}

/// One row of the table.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigTableRow {
    pub name: String,
    /// Its cell for each column, by column key.
    pub values: BTreeMap<String, String>,
}

/// What `config.table` and `config.rules` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigTableReply {
    #[serde(flatten)]
    pub table: ConfigTable,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `config.activate` and `config.preview` return: the table after the switch, and what the
/// switch did.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigActivation {
    #[serde(flatten)]
    pub table: ConfigTable,
    /// The features that fail after the switch, `<name>: <error>` each, those that failed
    /// before included.
    pub errors: Vec<String>,
    /// What each rule did.
    pub trace: Vec<Value>,
    /// Each style column's state after the switch.
    pub styles: Vec<Value>,
    /// For `config.preview`: always true. The design and its history are unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<bool>,
    /// For `config.preview`: the parameters as the row would set them, as `parameters.list`
    /// gives them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<Value>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
