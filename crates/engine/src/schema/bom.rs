//! Contract and result types for `assembly.bom` in `cmd/bom.rs` (#11075).

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;

/// The bill of materials below a component: its parts, counted through subassemblies, by part
/// number. Each component's bom type counts: a phantom has no row (what it holds counts where it
/// is placed), a reference counts nowhere, and a purchased or inseparable component is one row.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BomParams {
    /// The component, by id or name (default: the design).
    #[serde(default, deserialize_with = "component")]
    #[schemars(schema_with = "component_schema")]
    pub component: Option<String>,
    /// `flat` (the default): every part at any depth, counted through its subassemblies; `top`:
    /// the component's direct children, subassemblies as one row each.
    #[serde(default, deserialize_with = "levels")]
    #[schemars(schema_with = "levels_schema")]
    pub levels: Option<String>,
    #[serde(default)]
    pub group_by: Option<BomGroupBy>,
    /// A component with a part number is one row, the parts placed inside it listed under it as
    /// `children` rather than as rows of their own (default false).
    #[serde(default, deserialize_with = "group_children")]
    pub group_children: Option<bool>,
    /// Also write the bill of materials to this `.csv` or `.json` file (children as items 1.1,
    /// 1.2…).
    #[serde(default)]
    pub path: Option<String>,
    /// With `path`: `csv` or `json`, in any case (default: from the path's extension).
    #[serde(default)]
    pub format: Option<String>,
}

/// A component's id or name, as the handler reads it, with its message; `null` is none.
fn component<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Number(n) => Ok(Some(n.to_string())),
        Value::String(s) => Ok(Some(s)),
        _ => Err(D::Error::custom(whole("`component` must be a component id or name"))),
    }
}

fn component_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["number", "string", "null"]})
}

/// `flat` or `top`, with the handler's message; `null` is `flat`.
fn levels<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(l) if l == "flat" || l == "top" => Ok(Some(l)),
        _ => Err(D::Error::custom(whole("`levels` is flat or top"))),
    }
}

fn levels_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"enum": ["flat", "top", null]})
}

/// `true` or `false`, with the handler's message; `null` is `false`.
fn group_children<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole("`group_children` is true or false"))),
    }
}

/// What makes two components one row: `part_number` (the default; components without a part
/// number get a row per definition) or `definition`, as a word or a one-item list, with the
/// handler's messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BomGroupBy(pub String);

impl<'de> Deserialize<'de> for BomGroupBy {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let key = |g: String| match g.as_str() {
            "part_number" | "definition" => Ok(BomGroupBy(g)),
            o => Err(D::Error::custom(whole(format!("can't group by `{o}` (part_number or definition)")))),
        };
        let shape = || D::Error::custom(whole("`group_by` is [\"part_number\"] or [\"definition\"]"));
        match Value::deserialize(d)? {
            Value::String(g) => key(g),
            Value::Array(a) => match <[Value; 1]>::try_from(a) {
                Ok([Value::String(g)]) => key(g),
                _ => Err(shape()),
            },
            _ => Err(shape()),
        }
    }
}

impl JsonSchema for BomGroupBy {
    fn schema_name() -> Cow<'static, str> {
        "BomGroupBy".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let key = serde_json::json!({"enum": ["part_number", "definition"]});
        json_schema!({
            "description": "What makes two components one row: part_number (the default; components without a part number get a row per definition) or definition, as a word or a one-item list.",
            "anyOf": [key, {"type": "array", "items": key, "minItems": 1, "maxItems": 1}]
        })
    }
}

/// What `assembly.bom` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Bom {
    /// The component listed, by id (0: the design).
    pub component: u64,
    /// `flat` or `top`.
    pub levels: String,
    /// `["part_number"]` or `["definition"]`.
    pub group_by: Vec<String>,
    pub group_children: bool,
    /// With `path`: the file written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// With `path`: `csv` or `json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// The rows, by part number; rows without one after them, by name.
    pub rows: Vec<BomRow>,
    /// How many occurrences the rows count, children included.
    pub total_occurrences: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One row of a bill of materials.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BomRow {
    /// Its number, from 1.
    pub item: usize,
    pub part_number: String,
    pub description: String,
    pub revision: String,
    /// `normal`, `purchased` or `inseparable`.
    pub bom_type: String,
    /// How many are placed.
    pub count: u64,
    /// The components' names, `; ` between them.
    pub definition: String,
    /// The components' ids.
    pub components: Vec<u64>,
    /// Their free properties, by name.
    pub properties: BTreeMap<String, String>,
    /// With `group_children`: the parts placed inside it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<BomChild>>,
}

/// A part placed inside a row's component, with `group_children`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BomChild {
    /// Its number under its row, such as `2.1`.
    pub item: String,
    pub part_number: String,
    pub description: String,
    pub revision: String,
    pub bom_type: String,
    /// How many are in one of the row's component.
    pub each: u64,
    /// How many in all.
    pub count: u64,
    pub definition: String,
}

#[cfg(test)]
#[path = "bom_tests.rs"]
mod tests;
