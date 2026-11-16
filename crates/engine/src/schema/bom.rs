//! Contracts and result types for the commands in `cmd/bom.rs`: `assembly.bom` (#11075), and
//! `component.metadata` and `component.metadata.set` (#12609).
//!
//! The part property handlers read their parameters from the raw JSON, so each key here reads as
//! its handler reads it and fails with the handler's message. A `component` the handlers refuse
//! when it is left out is optional in the type and required in the schema, so that the handler's
//! message, not the check's "is required", is what a caller sees. Whether a component exists
//! (and that the root is not one `component.metadata.set` changes) stays the handler's to check.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use solidcraft_doc::metadata::MAX_KEYS;

use super::check::whole;
use super::motion::require;

/// A component's id or name, as `component_id` in the handlers reads it: anything else, `null`
/// included, is refused with the handler's message.
fn component_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => Ok(Some(n.to_string())),
        Value::String(s) => Ok(Some(s)),
        _ => Err(D::Error::custom(whole("`component` must be a component id or name"))),
    }
}

fn component_required(schema: &mut Schema) {
    require(schema, "component");
}

fn component_id_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "The component, by id or name (0 or \"root\": the design itself).", "type": ["number", "string"]})
}

fn text_or_null_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["string", "null"]})
}

/// Read the part properties of a component, or of the design itself (`0` or `root`): part
/// number, description, revision, bill of materials type, the `metadata` object as saved and the
/// free properties.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = component_required)]
pub struct ComponentMetadataParams {
    #[serde(default, deserialize_with = "component_key", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "component_id_schema")]
    pub component: Option<String>,
}

/// What `component.metadata` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ComponentMetadataResult {
    /// The component, by id (0: the design).
    pub component: u64,
    /// Its name (the design's name for 0).
    pub name: String,
    /// The part number, or null when it has none.
    pub part_number: Option<String>,
    /// The description, or null when it has none.
    pub description: Option<String>,
    /// The revision, or null when it has none.
    pub revision: Option<String>,
    /// `normal`, `phantom`, `reference`, `purchased` or `inseparable`. Not given for the design.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bom_type: Option<String>,
    /// The metadata object as saved, other tools' keys included.
    pub metadata: Map<String, Value>,
    /// The free properties, `{name: text}`. Empty for the design.
    pub properties: BTreeMap<String, String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// An object, as `component.metadata.set` reads `values` and `properties`: an object of at
/// most 256 entries, with the handler's messages for anything else (`null` included). Each
/// entry's value is left to the handler, which has its own message for each.
#[derive(Clone, Debug, PartialEq)]
pub struct Entries(pub Map<String, Value>);

fn entries<'de, D: Deserializer<'de>>(d: D, key: &str, what: &str, shape: &str) -> Result<Option<Entries>, D::Error> {
    match Value::deserialize(d)? {
        Value::Object(v) if v.len() <= MAX_KEYS => Ok(Some(Entries(v))),
        Value::Object(_) => Err(D::Error::custom(whole(format!("`{key}` can hold at most {MAX_KEYS} {what}")))),
        _ => Err(D::Error::custom(whole(format!("`{key}` must be an object: {shape}")))),
    }
}

fn values<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Entries>, D::Error> {
    entries(d, "values", "keys", "{key: value, …}")
}

fn free_properties<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Entries>, D::Error> {
    entries(d, "properties", "names", "{name: text, …}")
}

fn values_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Several properties at once, {key: value, …}, at most 256 keys: SolidCraft's keys (the ones `document.metadata`'s `properties` lists) and `bom_type`, which take text, or a reverse-DNS key such as `com.example.tool`, which takes an object. Each key reads its value as the parameter of the same name does (`part_number`, `bom_type`, …): for a text key, empty text or null removes it; for `bom_type` it means `normal`.",
        "type": "object",
        "maxProperties": MAX_KEYS
    })
}

fn properties_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Free properties, {name: text, …}, at most 256 names (a name is 1 to 128 bytes of text without control characters; a number or boolean is kept as its text). Empty text or null removes a property.",
        "type": "object",
        "maxProperties": MAX_KEYS,
        "additionalProperties": {"type": ["string", "number", "boolean", "null"]}
    })
}

// Deserialize is written out below rather than derived: the handler reads the other keys only for
// a component that is not the design, so for `0` or `root` they are neither read nor checked.
/// Change a component's part properties in one undo step (all or nothing): `part_number`,
/// `description`, `revision` and `bom_type`, `values` for several keys at once (also another
/// tool's reverse-DNS keys), and `properties` for free properties. Not the design's own
/// (`document.metadata.set`). Give at least one of them. A change that sets what is already
/// there makes no undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = component_required)]
pub struct ComponentMetadataSetParams {
    #[serde(default, deserialize_with = "component_key", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "component_id_schema")]
    pub component: Option<String>,
    /// The part number: text of at most 4096 bytes. Empty text or null removes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_or_null_schema")]
    pub part_number: Option<Value>,
    /// The description: text of at most 4096 bytes. Empty text or null removes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_or_null_schema")]
    pub description: Option<Value>,
    /// The revision: text of at most 4096 bytes. Empty text or null removes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_or_null_schema")]
    pub revision: Option<Value>,
    /// How the component counts in the bill of materials: `normal`, `phantom`, `reference`,
    /// `purchased` or `inseparable`. Empty text or null: `normal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_or_null_schema")]
    pub bom_type: Option<Value>,
    #[serde(default, deserialize_with = "values", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "values_schema")]
    pub values: Option<Entries>,
    #[serde(default, deserialize_with = "free_properties", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "properties_schema")]
    pub properties: Option<Entries>,
}

impl<'de> Deserialize<'de> for ComponentMetadataSetParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let p = Map::<String, Value>::deserialize(d)?;
        set_form(&p).map_err(D::Error::custom)
    }
}

/// A key's value through its reader, or `None` when the key is left out.
fn key_with<'a, T>(
    p: &'a Map<String, Value>,
    key: &str,
    read: impl FnOnce(&'a Value) -> Result<Option<T>, serde_json::Error>,
) -> Result<Option<T>, serde_json::Error> {
    p.get(key).map_or(Ok(None), read)
}

/// The keys `component.metadata.set`'s handler reads, in its order: the component, and for any
/// but the design (`0`, `root` in any case), the text keys, `values` and then `properties`.
fn set_form(p: &Map<String, Value>) -> Result<ComponentMetadataSetParams, serde_json::Error> {
    let component = key_with(p, "component", component_key)?;
    let mut form = ComponentMetadataSetParams {
        component,
        part_number: None,
        description: None,
        revision: None,
        bom_type: None,
        values: None,
        properties: None,
    };
    if form.component.as_deref().is_some_and(|c| c == "0" || c.eq_ignore_ascii_case("root")) {
        return Ok(form);
    }
    form.values = key_with(p, "values", values)?;
    form.part_number = p.get("part_number").cloned();
    form.description = p.get("description").cloned();
    form.revision = p.get("revision").cloned();
    form.bom_type = p.get("bom_type").cloned();
    form.properties = key_with(p, "properties", free_properties)?;
    Ok(form)
}

/// What `component.metadata.set` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ComponentMetadataSetResult {
    /// The component, by id.
    pub component: u64,
    /// Its metadata object after the change.
    pub metadata: Map<String, Value>,
    /// Its free properties after the change.
    pub properties: BTreeMap<String, String>,
    /// What changed: the keys whose value changed, and `properties.<name>` for each free
    /// property. Empty when nothing did (and then nothing was added to undo).
    pub changed: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

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
