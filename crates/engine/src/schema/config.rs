//! Contracts and result types for the configuration commands in `cmd/config.rs`: those that read
//! the table and switch between its rows (#11059), and those that change the table (#11895):
//! `config.configure`, `config.column`, `config.row` and `config.rule`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. What only the design can answer (does the
//! parameter, feature, body, component or row exist; does a rule hold for every row) stays the
//! handler's to check. A key the handler reads only with another (`index` without `delete`, a
//! row's `values` without `delete`) is checked whenever it is given.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use solidcraft_doc::config::{MAX_COLUMNS, MAX_RULES};

use super::check::whole;
use crate::params::{self, MAX_STRING_LIST, json_brief};

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

/// Add columns to the configuration table: parameters, feature suppressions, and bodies' and
/// components' materials and looks, each filled in every row with the design's current value. A
/// column the table has is left as it is. With no rows yet, a "Default" row takes the current
/// values and becomes the active row. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConfigConfigureParams {
    /// Parameters to configure, by name, as `parameters.list` gives them.
    #[serde(default)]
    pub params: Option<NameList>,
    /// Features whose suppression to configure, by name.
    #[serde(default)]
    pub features: Option<NameList>,
    /// Bodies whose material to configure, by name.
    #[serde(default)]
    pub materials: Option<NameList>,
    /// Bodies whose look to configure, by name.
    #[serde(default)]
    pub appearances: Option<NameList>,
    /// Components whose look to configure: each a component's name or id, or an occurrence's name
    /// (its component). At most 500.
    #[serde(default, deserialize_with = "component_keys", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "component_keys_schema")]
    pub component_appearances: Option<Vec<ComponentKey>>,
    /// Faces whose look to configure, each picked as `face_appearance` is for `config.column`
    /// (not by a column key). At most 500.
    #[serde(default, deserialize_with = "face_picks", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "face_picks_schema")]
    pub face_appearances: Option<Vec<Value>>,
}

/// Add a column to the configuration table, or delete one: give exactly one of `param`,
/// `feature`, `material`, `appearance`, `component_appearance` and `face_appearance`. An added
/// column is filled in every row with the design's current value; the rows are checked against
/// the table's rules afterwards. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = exactly_one_target)]
pub struct ConfigColumnParams {
    /// A parameter, by name: its value in each row.
    #[serde(default, deserialize_with = "target_name", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub param: Option<String>,
    /// A feature, by name: whether each row suppresses it.
    #[serde(default, deserialize_with = "target_name", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub feature: Option<String>,
    /// A body, by name: its material in each row.
    #[serde(default, deserialize_with = "target_name", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub material: Option<String>,
    /// A body, by name: its look in each row.
    #[serde(default, deserialize_with = "target_name", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub appearance: Option<String>,
    /// A component, by name or id, or an occurrence by name (its component): its look in each row.
    #[serde(default, deserialize_with = "some_component_key", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentKey")]
    pub component_appearance: Option<ComponentKey>,
    #[serde(default, deserialize_with = "face_column", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "face_column_schema")]
    pub face_appearance: Option<Value>,
    /// Delete the column instead of adding it. Default: false.
    #[serde(default)]
    pub delete: Option<bool>,
}

/// Add a row to the configuration table, change one, rename it or delete it. A new row starts
/// from `from`'s cells, or the design's current values; `values` then sets cells. Rows are checked
/// against the table's rules, and a parameter value against the parameters it drives, before
/// anything changes. The design keeps its active row's values until a row is activated. One undo
/// step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConfigRowEditParams {
    /// The row, by name: 1 to 128 bytes once spaces at either end are dropped. A name the table
    /// doesn't have adds a row (at most 2000).
    #[serde(deserialize_with = "row_name")]
    #[schemars(length(min = 1, max = 128))]
    pub name: String,
    /// A row to copy the cells of, by name. Default: the row's own cells, or the design's current
    /// values for a new row.
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default, deserialize_with = "row_values", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "row_values_schema")]
    pub values: Option<Map<String, Value>>,
    /// A new name for the row, which must exist: 1 to 128 bytes once spaces at either end are
    /// dropped, not another row's.
    #[serde(default, deserialize_with = "rename", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", length(min = 1, max = 128))]
    pub rename: Option<String>,
    /// Delete the row instead; `from`, `values` and `rename` are then not used. The last row can't
    /// be deleted while the table has rules. Default: false.
    #[serde(default)]
    pub delete: Option<bool>,
}

/// Add, replace, delete or move the configuration table's rules, which drive parameters and
/// suppressions from conditions, in the order they run. Give `rule`, `rules`, or `id` with
/// `delete` or `index`. Every row is then checked against the rules; if one fails, nothing
/// changes. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConfigRuleParams {
    /// A rule to add, or to replace the rule with its id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "rule_schema")]
    pub rule: Option<Value>,
    /// Without `rule`: rules to add or replace together, the rows then checked once with all of
    /// them in place (rules that only hold together can be added). At most 2048.
    #[serde(default, deserialize_with = "rule_list", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "rule_list_schema")]
    pub rules: Option<Vec<Value>>,
    /// Without `rule` or `rules`: the rule to delete or move, by id.
    #[serde(default, deserialize_with = "rule_id")]
    #[schemars(with = "Option<String>")]
    pub id: Option<String>,
    /// With `id`: delete the rule. Default: false (move it to `index`).
    #[serde(default)]
    pub delete: Option<bool>,
    /// With `id` and without `delete`: where to move the rule, from 0 (it runs first); less than
    /// the number of rules.
    #[serde(default, deserialize_with = "rule_index", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub index: Option<u64>,
}

/// A name or a list of names, as `params::string_list` reads it, with its messages: a list of
/// at most 10 000 names, or one name as text. `null` is left out (`Option`), as the reader reads
/// it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NameList(pub Vec<String>);

impl<'de> Deserialize<'de> for NameList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Each message starts with "must", so the check names the key in front, as the reader
        // does: "`params` must list names; the entry 2 is 5".
        match Value::deserialize(d)? {
            Value::String(name) => Ok(NameList(vec![name])),
            Value::Array(list) if list.len() > MAX_STRING_LIST => Err(D::Error::custom(format!("must list at most {MAX_STRING_LIST} names"))),
            Value::Array(list) => list
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| D::Error::custom(format!("must list names; the entry {} is {}", i + 1, json_brief(v))))
                })
                .collect::<Result<_, _>>()
                .map(NameList),
            v => Err(D::Error::custom(format!("must be a list of names; it is {}", json_brief(&v)))),
        }
    }
}

impl JsonSchema for NameList {
    fn schema_name() -> Cow<'static, str> {
        "NameList".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A name, or a list of names (at most 10000).",
            "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "maxItems": MAX_STRING_LIST}]
        })
    }
}

/// A component's name or id, or an occurrence's name, as `config.column` reads
/// `component_appearance`: text or a whole number of 0 or more, with its message.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentKey(pub String);

impl ComponentKey {
    fn read(v: &Value) -> Option<Self> {
        v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|id| id.to_string())).map(ComponentKey)
    }
}

impl<'de> Deserialize<'de> for ComponentKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ComponentKey::read(&Value::deserialize(d)?).ok_or_else(|| D::Error::custom(whole("component name or ID required")))
    }
}

impl JsonSchema for ComponentKey {
    fn schema_name() -> Cow<'static, str> {
        "ComponentKey".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A component's name or id, or an occurrence's name (its component).",
            "anyOf": [{"type": "string"}, {"type": "integer", "minimum": 0}]
        })
    }
}

/// `component_appearance`: given, `null` included, it must be a [`ComponentKey`], as the
/// handler reads it whenever the key is there.
fn some_component_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ComponentKey>, D::Error> {
    ComponentKey::deserialize(d).map(Some)
}

/// `component_appearances`: a list of at most 500 [`ComponentKey`]s; given, `null` included, it
/// must be a list, as the handler reads it.
fn component_keys<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<ComponentKey>>, D::Error> {
    let list = bounded_targets(Value::deserialize(d)?).map_err(D::Error::custom)?;
    list.iter()
        .map(|v| ComponentKey::read(v).ok_or_else(|| D::Error::custom(whole("component name or ID required"))))
        .collect::<Result<_, _>>()
        .map(Some)
}

fn component_keys_schema(generator: &mut SchemaGenerator) -> Schema {
    let key = generator.subschema_for::<ComponentKey>();
    json_schema!({"type": "array", "items": key, "maxItems": MAX_COLUMNS})
}

/// The list `config.configure` reads `component_appearances` and `face_appearances` as: at most
/// 500 targets, with its message for anything else.
fn bounded_targets(v: Value) -> Result<Vec<Value>, String> {
    match v {
        Value::Array(list) if list.len() <= MAX_COLUMNS => Ok(list),
        _ => Err(whole("configuration targets must be a bounded array")),
    }
}

/// Why `column_of` refuses `v` as a face to pick, before looking at the design: a face is
/// `{target}` (which the handler checks), or `{body, point}` with a body's name and a point
/// [x, y, z] on it.
fn face_pick_problem(v: &Value) -> Option<&'static str> {
    if v.get("target").is_some() {
        return None;
    }
    if v.get("body").and_then(Value::as_str).is_none() {
        return Some("face body required");
    }
    v.get("point").and_then(params::vec3).is_none().then_some("finite face point required")
}

/// `config.column`'s `face_appearance`: a face to pick ([`face_pick_problem`]), or the key of a
/// face column to delete. Given, `null` included, it must be one of them.
fn face_column<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    match face_pick_problem(&v) {
        Some(problem) if !v.is_string() => Err(D::Error::custom(whole(problem))),
        _ => Ok(Some(v)),
    }
}

/// `config.configure`'s `face_appearances`: a list of faces to pick, each as for `config.column`.
/// A column key is refused here, as the handler refuses it.
fn face_picks<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<Value>>, D::Error> {
    let list = bounded_targets(Value::deserialize(d)?).map_err(D::Error::custom)?;
    if let Some(problem) = list.iter().find_map(face_pick_problem) {
        return Err(D::Error::custom(whole(problem)));
    }
    Ok(Some(list))
}

/// A face to pick, as `config.column` and `config.configure` read it.
fn face_pick_schema() -> Value {
    serde_json::json!({
        "description": "A face: `{body, point}` picks the face of the body (by name) at the point [x, y, z] (world, mm), which must be on a solid's face; or `{target}` gives a face column's target as `config.table` lists it.",
        "type": "object",
        "anyOf": [{"required": ["target"]}, {"required": ["body", "point"]}],
        "properties": {
            "body": {"type": "string"},
            "point": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
            "target": {"type": "object"}
        }
    })
}

fn face_column_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A face, whose look each row sets; to delete its column (with `delete`), the column's key as `config.table` lists it.",
        "anyOf": [face_pick_schema(), {"type": "string"}]
    })
}

fn face_picks_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": face_pick_schema(), "maxItems": MAX_COLUMNS})
}

/// `param`, `feature`, `material` and `appearance` for `config.column`: a name. Given, `null`
/// included, anything but text gets the handler's message, as it reads the key as absent and
/// then finds no target.
fn target_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(name) => Ok(Some(name)),
        _ => Err(D::Error::custom(whole("give `param` or `feature`"))),
    }
}

/// `config.column` takes exactly one target, as the handler counts them. (A target given as
/// `null` counts, and its reader refuses it.)
fn exactly_one_target(schema: &mut Schema) {
    let one: Vec<Value> = ["param", "feature", "material", "appearance", "component_appearance", "face_appearance"]
        .iter()
        .map(|key| serde_json::json!({"required": [key]}))
        .collect();
    schema.insert("oneOf".into(), Value::Array(one));
}

/// A row's name as `config.row` reads `name`: text that, without spaces at either end, is 1 to
/// 128 bytes, with the handler's message for anything else.
fn row_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(name) if is_row_name(&name) => Ok(name),
        _ => Err(D::Error::custom(whole("`name` is required"))),
    }
}

fn is_row_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name.len() <= 128
}

/// `rename`: given, `null` included, it must be a row name ([`row_name`]), with the handler's
/// message.
fn rename<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(name) if is_row_name(&name) => Ok(Some(name)),
        _ => Err(D::Error::custom(whole("rename requires an existing row and a nonempty name up to 128 bytes"))),
    }
}

/// The column-key prefixes whose cells take a look given as an object.
const LOOK_COLUMNS: [&str; 3] = ["appearance:", "component_appearance:", "face_appearance:"];

/// `values`: an object of cells by column key, as `config.row` reads it. Each cell is text (an
/// expression, a material's or a look's name), true or false, or a number, and for a look
/// column also an object; at most 4096 bytes as the cell saves it. Given, `null` included, it
/// must be an object.
fn row_values<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Map<String, Value>>, D::Error> {
    let Value::Object(cells) = Value::deserialize(d)? else {
        return Err(D::Error::custom(whole("`values` must map columns to values")));
    };
    for (key, v) in &cells {
        let saved = match v {
            Value::String(text) => text.len(),
            Value::Bool(b) => b.to_string().len(),
            Value::Number(n) => n.to_string().len(),
            Value::Object(_) if LOOK_COLUMNS.iter().any(|prefix| key.starts_with(prefix)) => {
                serde_json::to_string(v).map_err(D::Error::custom)?.len()
            }
            _ => return Err(D::Error::custom(whole(format!("value for `{key}` must be an expression or true/false")))),
        };
        if saved > 4096 {
            return Err(D::Error::custom(whole("value too long")));
        }
    }
    Ok(Some(cells))
}

fn row_values_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Cells to set, by column key as `config.table` lists it (`param:W`, `suppress:Block`, `material:Body1`…), or by a parameter's or a suppressed feature's name. A cell is an expression for a parameter, true or false (suppressed) for a feature, a material's name for a material column, and a look's name or a look object for a look column; at most 4096 bytes. Each key must be a column the table has.",
        "type": "object",
        "additionalProperties": {"type": ["string", "boolean", "number", "object"]}
    })
}

/// A configuration rule, as `config.rule` takes it and `config.table` lists it. The handler
/// checks it (`validate_rules_json`, then the doc crate's own reading), so only the schema says
/// what it holds.
fn rule_value_schema() -> Value {
    let text = |description: &str| serde_json::json!({"description": description, "type": "string", "minLength": 1, "maxLength": 4096});
    let parameter = serde_json::json!({"description": "A parameter, by name.", "type": "string", "minLength": 1, "maxLength": 64});
    let feature = serde_json::json!({"description": "A feature, by id.", "type": "integer", "minimum": 0});
    let effect = |kind: &str, fields: Value, required: &[&str]| {
        let mut properties = serde_json::json!({"type": {"enum": [kind]}});
        if let (Some(all), Some(more)) = (properties.as_object_mut(), fields.as_object()) {
            all.extend(more.clone());
        }
        let mut required: Vec<&str> = required.to_vec();
        required.insert(0, "type");
        serde_json::json!({"type": "object", "required": required, "properties": properties, "additionalProperties": false})
    };
    serde_json::json!({
        "type": "object",
        "required": ["id", "name", "condition", "effect"],
        "additionalProperties": false,
        "properties": {
            "id": {"description": "The rule's id: 1 to 64 characters, not only spaces. A rule with an id the table has replaces it.", "type": "string", "minLength": 1, "maxLength": 64},
            "name": {"description": "A name to show: 1 to 128 characters, not only spaces.", "type": "string", "minLength": 1, "maxLength": 128},
            "enabled": {"description": "Whether the rule runs. Default: true.", "type": "boolean"},
            "length_unit": {"description": "The unit of plain numbers in the rule's length expressions. Default: the replaced rule's, or the design's unit.", "enum": ["mm", "cm", "m", "in", "ft"]},
            "condition": {
                "description": "When the rule applies, by `type`: `always`; `all` or `any` of `conditions` (1 to 64); `not` (`condition`); `parameter`, a parameter compared (`operator`) with an `expression`; or `suppression`, whether a feature (`feature`, by id) is suppressed (`value`). At most 64 conditions, 8 deep.",
                "type": "object",
                "required": ["type"],
                "properties": {
                    "type": {"enum": ["always", "all", "any", "not", "parameter", "suppression"]},
                    "conditions": {"type": "array", "items": {"type": "object"}, "minItems": 1, "maxItems": 64},
                    "condition": {"type": "object"},
                    "parameter": parameter,
                    "operator": {"enum": ["eq", "ne", "lt", "le", "gt", "ge"]},
                    "expression": text("An expression the parameter is compared with."),
                    "feature": feature,
                    "value": {"type": "boolean"}
                }
            },
            "effect": {
                "description": "What the rule does when its condition holds, by `type`: `drive_parameter` sets a parameter to an `expression`; `drive_suppression` suppresses a feature (`value` true) or not; `parameter_range` keeps a parameter from `minimum` to `maximum`; `allow_parameter_values` allows only the values of `expressions` (1 to 128).",
                "anyOf": [
                    effect("drive_parameter", serde_json::json!({"parameter": parameter, "expression": text("The parameter's value.")}), &["parameter", "expression"]),
                    effect("drive_suppression", serde_json::json!({"feature": feature, "value": {"type": "boolean"}}), &["feature", "value"]),
                    effect("parameter_range", serde_json::json!({"parameter": parameter, "minimum": text("The lowest value."), "maximum": text("The highest value.")}), &["parameter", "minimum", "maximum"]),
                    effect("allow_parameter_values", serde_json::json!({"parameter": parameter, "expressions": {"type": "array", "items": text("An allowed value."), "minItems": 1, "maxItems": 128}}), &["parameter", "expressions"])
                ]
            }
        }
    })
}

fn rule_schema(_: &mut SchemaGenerator) -> Schema {
    let mut rule = rule_value_schema();
    if let Some(rule) = rule.as_object_mut() {
        rule.insert("description".into(), "A rule to add, or to replace the rule with its id.".into());
    }
    Schema::try_from(rule).unwrap_or_default()
}

/// `rules`: given, `null` included, a list of at most 2048 rules, with the handler's messages.
/// The handler checks each rule.
fn rule_list<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<Value>>, D::Error> {
    match Value::deserialize(d)? {
        Value::Array(list) if list.len() > MAX_RULES => Err(D::Error::custom(whole("too many rules"))),
        Value::Array(list) => Ok(Some(list)),
        _ => Err(D::Error::custom(whole("`rules` is a list of rules"))),
    }
}

fn rule_list_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": rule_value_schema(), "maxItems": MAX_RULES})
}

/// `id`: text; `null` is left out, as the handler reads it. Anything else gets the message the
/// handler gives when it finds no `rule`, `rules` or `id`.
fn rule_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(id) => Ok(Some(id)),
        _ => Err(D::Error::custom(whole("give rule, rules or id"))),
    }
}

/// `index`: given, `null` included, a whole number of 0 or more, with the handler's message.
fn rule_index<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Value::deserialize(d)?.as_u64().map(Some).ok_or_else(|| D::Error::custom(whole("valid index required")))
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
