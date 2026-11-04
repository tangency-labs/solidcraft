//! Contracts and result types for the commands in `cmd/bom.rs` (#11075).

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::BomType;
use solidcraft_doc::bom::View;

use super::check::whole;
use super::types::{IdOrName, present};

/// Let one occurrence count as another bill of materials type in every bill of materials, in
/// place of its component's, or as its component again. An occurrence inside a subassembly
/// belongs to its definition, so the type holds wherever the subassembly is placed. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct OccurrenceBomTypeParams {
    /// The occurrence: its id, or its name.
    pub occurrence: IdOrName,
    /// Its placement, checked when given: the occurrence ids from the design down to it.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrencePath")]
    pub path: Option<OccurrencePath>,
    /// The type it counts as, or `same_as_component` (or null) for its component's.
    #[serde(deserialize_with = "bom_type")]
    #[schemars(schema_with = "bom_type_schema")]
    pub bom_type: Option<BomType>,
}

/// An occurrence path, read by the handlers' own reader.
#[derive(Debug)]
pub struct OccurrencePath(pub Vec<u64>);

impl<'de> Deserialize<'de> for OccurrencePath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::path_ids(&Value::deserialize(d)?).map(OccurrencePath).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for OccurrencePath {
    fn schema_name() -> Cow<'static, str> {
        "OccurrencePath".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Occurrence ids from the design down to the placement.",
            "type": "array",
            "items": {"type": "integer", "minimum": 0},
            "minItems": 1,
            "maxItems": solidcraft_doc::MAX_PLACEMENT_DEPTH
        })
    }
}

/// `bom_type`, read by the handler's own reader.
fn bom_type<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BomType>, D::Error> {
    crate::cmd::occurrence_bom_type(Some(&Value::deserialize(d)?)).map_err(|e| D::Error::custom(whole(e)))
}

/// The five types' words, `same_as_component`, or null.
fn bom_type_schema(_: &mut SchemaGenerator) -> Schema {
    let mut words: Vec<Value> = BomType::ALL.iter().map(|t| Value::from(t.as_str())).collect();
    words.push(Value::from(crate::cmd::SAME_AS_COMPONENT));
    json_schema!({
        "description": "normal, phantom, reference, purchased or inseparable; same_as_component or null: the component's.",
        "anyOf": [{"type": "string", "enum": words}, {"type": "null"}]
    })
}

/// What `occurrence.bom_type` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OccurrenceBomTypeResult {
    /// The occurrence's id.
    pub occurrence: u64,
    /// Its own type now, or `same_as_component`.
    pub bom_type: String,
    /// The type it counts as: its own, else its component's.
    pub effective: String,
    /// Whether this call changed it (an undo step).
    pub changed: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The bill of materials below a component: its parts, counted through subassemblies, by part
/// number. Each placement counts as its occurrence's bom type, else its component's: a phantom has
/// no row (what it holds counts where it is placed), a reference counts nowhere, and a purchased or
/// inseparable component is one row.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BomParams {
    /// The component, by id or name (default: the design).
    #[serde(default, deserialize_with = "component")]
    #[schemars(schema_with = "component_schema")]
    pub component: Option<String>,
    /// `flat` (the default): every part at any depth, counted through its subassemblies; `top`:
    /// the component's direct children, subassemblies as one row each; `all`: as top, each
    /// subassembly's own rows listed after it, to any depth.
    #[serde(default, deserialize_with = "levels")]
    #[schemars(schema_with = "levels_schema")]
    pub levels: Option<String>,
    /// In all levels, a purchased part's contents listed under it too (default false).
    #[serde(default, deserialize_with = "show_purchased_contents")]
    pub show_purchased_contents: Option<bool>,
    #[serde(default)]
    pub group_by: Option<BomGroupBy>,
    /// A component with a part number is one row, the parts placed inside it listed under it as
    /// `children` rather than as rows of their own (default false).
    #[serde(default, deserialize_with = "group_children")]
    pub group_children: Option<bool>,
    /// Measure each part: `mass_g` on its row, and `total_mass_g` (default false).
    #[serde(default, deserialize_with = "mass")]
    pub mass: Option<bool>,
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

/// A view's word, with the handler's message; `null` is the default view.
fn levels<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(l) if View::parse(&l).is_some() => Ok(Some(l)),
        _ => Err(D::Error::custom(whole(format!("`levels` is {}", View::ALL.map(View::as_str).join(" or "))))),
    }
}

fn levels_schema(_: &mut SchemaGenerator) -> Schema {
    let mut words: Vec<Value> = View::ALL.iter().map(|v| Value::from(v.as_str())).collect();
    words.push(Value::Null);
    json_schema!({"enum": words})
}

/// `true` or `false`, with the handler's message; `null` is `false`.
fn flag<'de, D: Deserializer<'de>>(d: D, key: &str) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole(format!("`{key}` is true or false")))),
    }
}

fn group_children<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag(d, "group_children")
}

fn mass<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag(d, "mass")
}

fn show_purchased_contents<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag(d, "show_purchased_contents")
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
    /// `flat`, `top` or `all`.
    pub levels: String,
    /// `["part_number"]` or `["definition"]`.
    pub group_by: Vec<String>,
    pub group_children: bool,
    pub show_purchased_contents: bool,
    pub mass: bool,
    /// With `path`: the file written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// With `path`: `csv` or `json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// The rows, by part number; rows without one after them, by name. In a numbered view each
    /// level goes by item number; in all levels each row is followed by the rows under it.
    pub rows: Vec<BomRow>,
    /// How many occurrences the rows count, children included.
    pub total_occurrences: u64,
    /// With `mass`: what the assembly weighs, in grams; null when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_mass_g: Option<Option<f64>>,
    /// Whether the view's item numbers are saved.
    pub numbered: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One row of a bill of materials.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BomRow {
    /// Its number: from 1 at the top level, and as text such as `2.1` under another row.
    #[schemars(schema_with = "item_schema")]
    pub item: Value,
    pub part_number: String,
    pub description: String,
    pub revision: String,
    /// `normal`, `phantom`, `purchased` or `inseparable`.
    pub bom_type: String,
    /// How many are placed.
    pub count: u64,
    /// The components' names, `; ` between them.
    pub definition: String,
    /// The physical materials of the definitions' own bodies, `; ` between them.
    pub material: String,
    /// Under another row: how many are in one of that row's component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub each: Option<u64>,
    /// At the top level: the components' ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<u64>>,
    /// At the top level: their free properties, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, String>>,
    /// With `mass`: what one of the row's parts weighs, in grams; null when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass_g: Option<Option<f64>>,
    /// When its definitions have a `bom_quantity`: what its placements add up to; null with
    /// `quantity_unknown`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<Option<f64>>,
    /// `pieces` or `mm`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity_unit: Option<String>,
    /// `mixed` (pieces and a length) or `invalid` (a quantity that no longer evaluates).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity_unknown: Option<String>,
    /// In a numbered view, its number isn't saved yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisional: Option<bool>,
    /// A renumber keeps its number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    /// Another row at its level has its number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate: Option<bool>,
    /// The values typed in for the component, shown in place of the worked-out ones.
    #[serde(rename = "static", skip_serializing_if = "Option::is_none")]
    pub statics: Option<Vec<String>>,
    /// The parts placed inside it, listed under it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<BomRow>>,
}

fn item_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["integer", "string"]})
}

#[cfg(test)]
#[path = "bom_tests.rs"]
mod tests;
