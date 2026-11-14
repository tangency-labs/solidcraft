//! Contracts and result types for the commands in `cmd/parameters.rs`: those that manage the
//! design's user parameters by name (#11941), `parameters.add`, `.rename`, `.delete`, `.comment`,
//! `.favorite` and `.move`, and the rest (#12497): `parameters.change`, `.list`, `.users`,
//! `.graph`, `.export`, `.import`, `.complete` and `expr.evaluate`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a parameter exists, and whether a new
//! name, unit or expression is valid, stays the document's to check, with its messages.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{ParamRow, ParamUser};

use super::check::{located, whole};

/// A parameter's name, as `cmd/parameters.rs`'s `param_name` reads `name`: text that isn't blank,
/// with its message for anything else. Spaces at either end are dropped.
fn param_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(name) if !name.trim().is_empty() => Ok(name),
        _ => Err(D::Error::custom(whole("`name` is required"))),
    }
}

/// Add a user parameter: a named number with a unit, or a named text. Features, sketches and
/// other parameters can use it by name. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = expression_or_value)]
pub struct ParamAddParams {
    /// The new parameter's name: a letter or `_`, then letters, digits or `_`; not a unit's,
    /// function's or constant's name, nor another parameter's.
    #[serde(deserialize_with = "param_name")]
    #[schemars(length(min = 1))]
    pub name: String,
    #[serde(default, deserialize_with = "expression")]
    #[schemars(schema_with = "expression_schema")]
    pub expression: Option<Value>,
    /// Another name for `expression`, read when `expression` isn't a number or text.
    #[serde(default, deserialize_with = "expression")]
    #[schemars(schema_with = "expression_schema")]
    pub value: Option<Value>,
    /// The unit: `mm`, `cm`, `m`, `in`, `ft`, `deg`, `rad`, `""` (no unit) or `text` (a text
    /// parameter, whose expression is its text). Default: the expression's own unit, a length
    /// in the design's units.
    #[serde(default)]
    pub unit: Option<String>,
    /// A note to show with it.
    #[serde(default)]
    pub comment: Option<String>,
    /// List it with the favourites, first. Default: false.
    #[serde(default)]
    pub favorite: Option<bool>,
}

/// `expression` and `value`: a number or text, as `expr_arg` reads them. `null` is left out, as
/// the reader reads it; anything else gets the handler's message.
fn expression<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v @ (Value::String(_) | Value::Number(_)) => Ok(Some(v)),
        _ => Err(D::Error::custom(whole("`expression` must be a number or text"))),
    }
}

fn expression_schema(_: &mut schemars::SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "description": "The value: a number, or an expression as text (`25 mm`, `W / 2`). For a text parameter, its text; quotes round it are optional.",
        "type": ["number", "string", "null"]
    })
}

/// `parameters.add` needs `expression`, or `value` in its place, as a number or text. Given
/// isn't enough: `expr_arg` reads a `null` one as left out, so each alternative also needs its
/// key's type.
fn expression_or_value(schema: &mut Schema) {
    schema.insert(
        "anyOf".into(),
        serde_json::json!([
            {"required": ["expression"], "properties": {"expression": {"type": ["number", "string"]}}},
            {"required": ["value"], "properties": {"value": {"type": ["number", "string"]}}}
        ]),
    );
}

/// Rename a parameter, a user parameter or a feature's named input. The expressions, feature
/// inputs and dimensions that use it follow. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ParamRenameParams {
    /// The parameter, by name.
    #[serde(deserialize_with = "param_name")]
    #[schemars(length(min = 1))]
    pub name: String,
    /// Its new name, as for a new parameter. Spaces at either end are dropped.
    #[serde(deserialize_with = "new_name")]
    pub new_name: String,
}

/// `new_name`: text, with the handler's message for anything else.
fn new_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(name) => Ok(name),
        _ => Err(D::Error::custom(whole("`new_name` is required"))),
    }
}

/// Delete a user parameter. It's refused while an expression, a feature input or a dimension
/// uses it; the message lists them. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ParamNameParams {
    /// The parameter, by name.
    #[serde(deserialize_with = "param_name")]
    #[schemars(length(min = 1))]
    pub name: String,
}

/// Set a parameter's comment, a note shown with it. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = comment_required)]
pub struct ParamCommentParams {
    /// The parameter, by name.
    #[serde(deserialize_with = "param_name")]
    #[schemars(length(min = 1))]
    pub name: String,
    /// The comment; empty text clears it.
    // Optional in the type and required in the schema (`comment_required`): left out, the
    // handler refuses it with its own message ("`comment` must be text"), not the check's "is
    // required". `skip_serializing_if` keeps schemars from publishing `default: null`, which
    // the schema refuses.
    #[serde(default, deserialize_with = "comment_text", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub comment: Option<String>,
}

/// `parameters.comment` needs `comment`, though its type has it optional.
fn comment_required(schema: &mut Schema) {
    if let Some(Value::Array(required)) = schema.get_mut("required")
        && !required.contains(&Value::from("comment"))
    {
        required.push("comment".into());
    }
}

/// `comment`: text, with the handler's message for anything else, `null` included.
fn comment_text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(comment) => Ok(Some(comment)),
        _ => Err(D::Error::custom(whole("`comment` must be text"))),
    }
}

/// Mark a parameter as a favourite, listed first, or unmark it. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ParamFavoriteParams {
    /// The parameter, by name.
    #[serde(deserialize_with = "param_name")]
    #[schemars(length(min = 1))]
    pub name: String,
    /// Whether it's a favourite. Default: true.
    #[serde(default)]
    pub favorite: Option<bool>,
}

/// Move a user parameter just before or after another, in the order the user parameters are
/// listed. Give exactly one of `before` and `after`. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = before_or_after)]
pub struct ParamMoveParams {
    /// The user parameter, by name.
    #[serde(deserialize_with = "param_name")]
    #[schemars(length(min = 1))]
    pub name: String,
    /// Another user parameter, by name: the parameter is listed just before it.
    #[serde(default, deserialize_with = "neighbour")]
    pub before: Option<String>,
    /// Another user parameter, by name: the parameter is listed just after it.
    #[serde(default, deserialize_with = "neighbour")]
    pub after: Option<String>,
}

/// `before` and `after`: text; `null` is left out, as the handler reads it. Anything else gets
/// the message the handler gives when it finds neither.
fn neighbour<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(name) => Ok(Some(name)),
        _ => Err(D::Error::custom(whole("give one of `before` or `after`: another user parameter"))),
    }
}

/// `parameters.move` needs exactly one of `before` and `after` as text. Each alternative needs
/// its key to be text, so a `null` one, which the handler reads as left out, matches neither:
/// `oneOf` then counts the keys as the handler does, and both or neither is refused.
fn before_or_after(schema: &mut Schema) {
    schema.insert(
        "oneOf".into(),
        serde_json::json!([
            {"required": ["before"], "properties": {"before": {"type": "string"}}},
            {"required": ["after"], "properties": {"after": {"type": "string"}}}
        ]),
    );
}

/// What `parameters.add` returns: the new parameter.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamAdded {
    pub name: String,
    #[serde(flatten)]
    pub value: AddedValue,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A new parameter's value: a number's, or a text parameter's text.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum AddedValue {
    /// A text parameter.
    Text {
        /// Always `text`.
        unit: String,
        /// Its text.
        text: String,
        /// Always `null`: a text has no number.
        value: Option<f64>,
    },
    /// A number.
    Number {
        /// Its unit; `""` for none.
        unit: Option<String>,
        /// Its value in mm for a length and degrees for an angle, as in every reply; `null` if
        /// it can't be worked out.
        value: Option<f64>,
        /// Its value in its own unit; `null` if it can't be worked out.
        display_value: Option<f64>,
    },
}

/// What `parameters.rename` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamRenamed {
    /// The old name.
    pub from: String,
    /// The new name.
    pub to: String,
    /// How many expressions, feature inputs and dimensions were changed to use the new name.
    pub references_updated: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `parameters.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamDeleted {
    /// The deleted parameter's name.
    pub deleted: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `parameters.comment` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamCommented {
    pub name: String,
    /// The comment as given.
    pub comment: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `parameters.favorite` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamFavorited {
    pub name: String,
    /// Whether it's now a favourite.
    pub favorite: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `parameters.move` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamMoved {
    pub name: String,
    /// The user parameters' names, in their new order.
    pub order: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// #12497: `parameters.change`, `.list`, `.users`, `.graph`, `.export`, `.import`, `.complete`
// and `expr.evaluate`.
// ---------------------------------------------------------------------------------------------

/// Adds `key` to the schema's `required` list: for a key the type has optional so that the
/// handler, not the check, refuses it when it's left out.
fn require(schema: &mut Schema, key: &str) {
    match schema.get_mut("required") {
        Some(Value::Array(required)) => {
            if !required.iter().any(|k| k == key) {
                required.push(key.into());
            }
        }
        _ => {
            schema.insert("required".into(), Value::Array(vec![key.into()]));
        }
    }
}

/// `key` as text: `null` is left out, as `str_` reads it. Anything else is refused, though the
/// handler would read it as left out; `message` is the handler's for that case, if it has one.
fn text_or_null<'de, D: Deserializer<'de>>(d: D, message: &str) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        _ => Err(D::Error::custom(whole(message))),
    }
}

/// What `parameters.change` takes, in the form it's read in: see [`ParamChangeParams`].
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = change_forms)]
pub struct ParamChangeKeys {
    /// Several existing parameters at once, `{name: expression}`: they are checked together and
    /// the design is rebuilt once, all or nothing. Renaming isn't done this way. When given,
    /// no other key is read.
    #[serde(default, deserialize_with = "changes_map", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "changes_schema")]
    pub changes: Option<Map<String, Value>>,
    /// Delete this user parameter (refused while something uses it). When given as text, no
    /// other key but `changes` is read.
    #[serde(default, deserialize_with = "delete_text")]
    pub delete: Option<String>,
    /// The parameter to change, by name: a user parameter, a sketch dimension or a feature's
    /// named input. Spaces at either end are dropped.
    #[serde(default, deserialize_with = "change_name", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", length(min = 1))]
    pub name: Option<String>,
    /// The new value: a number, or an expression as text (`25 mm`, `W / 2`). A text
    /// parameter's expression is its text, quotes round it optional. `name = value` renames
    /// the parameter to `name` and sets the value (or sets `name` when the value is just
    /// `name`). Needed unless `new_name`, `favorite` or `comment` is given.
    #[serde(default, deserialize_with = "expression")]
    #[schemars(schema_with = "expression_schema")]
    pub expression: Option<Value>,
    /// Another name for `expression`, read when `expression` isn't a number or text.
    #[serde(default, deserialize_with = "expression")]
    #[schemars(schema_with = "expression_schema")]
    pub value: Option<Value>,
    /// The unit: `mm`, `cm`, `m`, `in`, `ft`, `deg`, `rad`, `""` (no unit) or `text`. Default:
    /// the parameter's own, or for a new one its value's unit, a length in the design's units.
    #[serde(default, deserialize_with = "unit_text")]
    pub unit: Option<String>,
    /// A note to show with the parameter.
    #[serde(default, deserialize_with = "comment_key")]
    pub comment: Option<String>,
    /// List it with the favourites, first, or not.
    #[serde(default, deserialize_with = "favorite_flag")]
    pub favorite: Option<bool>,
    /// Rename the parameter: the expressions, feature inputs and dimensions that use it follow.
    /// With an `expression` the new value is set too.
    #[serde(default, deserialize_with = "new_name_key")]
    pub new_name: Option<String>,
}

fn changes_map<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Map<String, Value>>, D::Error> {
    let Value::Object(map) = Value::deserialize(d)? else {
        return Err(D::Error::custom(whole("`changes` must be an object of name: expression")));
    };
    if map.is_empty() {
        return Err(D::Error::custom(whole("`changes` is empty")));
    }
    for (name, e) in &map {
        // `expr_arg`: text, or a number.
        if !matches!(e, Value::String(_) | Value::Number(_)) {
            return Err(D::Error::custom(whole(format!("`changes.{name}` must be a number or text"))));
        }
    }
    Ok(Some(map))
}

fn changes_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Several existing parameters at once, `{name: expression}`: they are checked together and the design is rebuilt once, all or nothing. Renaming isn't done this way (`name = value` is refused). When given, no other key is read.",
        "type": "object",
        "minProperties": 1,
        "additionalProperties": {"type": ["number", "string"]}
    })
}

fn delete_text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "`delete` must be text")
}

fn unit_text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "`unit` must be text")
}

fn comment_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "`comment` must be text")
}

fn new_name_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "`new_name` must be text")
}

fn favorite_flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole("`favorite` must be true or false"))),
    }
}

/// `name` of a single change, as `param_name` reads it: text that isn't blank, `null` included
/// in what is refused.
fn change_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    param_name(d).map(Some)
}

/// `parameters.change` takes one of three forms: `changes`, `delete` or `name` with what to
/// change about it.
fn change_forms(schema: &mut Schema) {
    let name_with = json!({
        "required": ["name"],
        "properties": {"name": {"type": "string"}},
        "anyOf": [
            {"required": ["expression"], "properties": {"expression": {"type": ["number", "string"]}}},
            {"required": ["value"], "properties": {"value": {"type": ["number", "string"]}}},
            {"required": ["new_name"]},
            {"required": ["favorite"]},
            {"required": ["comment"]}
        ]
    });
    schema.insert(
        "anyOf".into(),
        json!([
            {"required": ["changes"]},
            {"required": ["delete"], "properties": {"delete": {"type": "string"}}},
            name_with
        ]),
    );
}

/// Change a parameter: set its expression, unit, comment or favourite, rename it (references
/// follow), delete it, or set several existing parameters at once. The design is rebuilt once
/// and the features that changed are named in the reply. One undo step.
///
/// The handler reads `changes` alone when it is given, else `delete` alone when that is text,
/// else the keys of a single change; the keys of a form it doesn't read are left unchecked.
#[derive(Debug)]
pub struct ParamChangeParams(pub ParamChangeKeys);

impl<'de> Deserialize<'de> for ParamChangeParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Some(keys) = value.as_object_mut() {
            let only = if keys.contains_key("changes") {
                Some("changes")
            } else if matches!(keys.get("delete"), Some(Value::String(_))) {
                Some("delete")
            } else {
                None
            };
            // Otherwise a single change: `changes` isn't there, and `delete` isn't text.
            if let Some(only) = only {
                keys.retain(|k, _| k == only || !CHANGE_KEYS.contains(&k.as_str()));
            }
            // `value` is read only when `expression` isn't a number or text.
            else if matches!(keys.get("expression"), Some(Value::String(_) | Value::Number(_))) {
                keys.remove("value");
            }
        }
        serde_path_to_error::deserialize(&value).map(ParamChangeParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

const CHANGE_KEYS: [&str; 9] = ["changes", "delete", "name", "expression", "value", "unit", "comment", "favorite", "new_name"];

impl JsonSchema for ParamChangeParams {
    fn schema_name() -> Cow<'static, str> {
        "ParamChangeParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ParamChangeKeys::json_schema(generator)
    }
}

/// A feature that failed to rebuild: its name and error.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FailedFeature {
    /// The feature's name.
    pub feature: String,
    /// Why it failed.
    pub error: String,
}

impl FailedFeature {
    /// The features of `results` that failed.
    pub fn of(results: &[solidcraft_doc::FeatureResult]) -> Vec<FailedFeature> {
        results.iter().filter_map(|r| r.error.as_ref().map(|e| FailedFeature { feature: r.name.clone(), error: e.clone() })).collect()
    }
}

/// A feature a configuration rule switched on or off.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SwitchedFeature {
    /// The feature's id.
    pub feature: u64,
    /// Its name; `null` if it is gone.
    pub name: Option<String>,
    /// Whether it is suppressed now.
    pub suppressed: bool,
}

/// A parameter that was renamed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamRename {
    /// The old name.
    pub from: String,
    /// The new name.
    pub to: String,
    /// How many expressions, feature inputs and dimensions were changed to use the new name.
    pub references_updated: usize,
}

/// What `parameters.change` returns: one of three shapes, by what was asked.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamChanged {
    #[serde(flatten)]
    pub body: ParamChangedBody,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The shapes of a `parameters.change` reply.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ParamChangedBody {
    /// A parameter was deleted (`delete`).
    Deleted {
        /// The deleted parameter's name.
        deleted: String,
    },
    /// Several parameters were set (`changes`).
    Many {
        /// Each changed parameter's value, in mm for a length and degrees for an angle; `null`
        /// where it can't be worked out.
        changed: BTreeMap<String, Option<f64>>,
        /// How many features were rebuilt.
        recomputed: usize,
        /// How many features reused an earlier result for the same inputs (a value set back).
        restored: usize,
        /// The features that failed to rebuild.
        errors: Vec<FailedFeature>,
        /// The features the configuration rules switched, when they did.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        switched: Vec<SwitchedFeature>,
    },
    /// One parameter was changed.
    One(Box<ParamChangedOne>),
}

/// What changing one parameter returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamChangedOne {
    /// Set when the parameter was renamed (`new_name`, or `name = expression`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed: Option<ParamRename>,
    /// Set when `name = expression` set the parameter that the changed value's expression named:
    /// the name asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_through: Option<String>,
    /// The features the configuration rules switched, when they did.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub switched: Vec<SwitchedFeature>,
    /// The parameter's name now.
    pub name: String,
    /// The expression it has now; `null` when only its name, comment or favourite changed.
    pub expression: Option<String>,
    /// A text parameter's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Its value in mm for a length and degrees for an angle, as in every reply; `null` if it
    /// can't be worked out.
    pub value: Option<f64>,
    /// Its value in its own unit; `null` if it can't be worked out.
    pub display_value: Option<f64>,
    /// How many features were rebuilt.
    pub recomputed: usize,
    /// How many features reused an earlier result for the same inputs (a value set back).
    pub restored: usize,
    /// The features that failed to rebuild.
    pub errors: Vec<FailedFeature>,
}

/// List the parameters: the user's, the sketch dimensions' and the feature inputs', favourites
/// first, with each one's expression, value, uses and users, and any cycles. Changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ParamListKeys {
    /// Only the favourites. When true, `filter` isn't read. Default: false.
    #[serde(default, deserialize_with = "list_favorites")]
    pub favorites: Option<bool>,
    /// Which parameters, ignoring case: `all`, `user`, `sketch` (dimensions), `feature` (inputs),
    /// `model` (every one but the user's) or `favorites`. Anything else lists all. Default: all.
    #[serde(default, deserialize_with = "list_filter")]
    pub filter: Option<String>,
}

fn list_favorites<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole("`favorites` must be true or false"))),
    }
}

fn list_filter<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "`filter` must be text")
}

/// `parameters.list`'s parameters: [`ParamListKeys`], with `filter` left unread when
/// `favorites` is true, as the handler leaves it.
#[derive(Debug)]
pub struct ParamListParams(pub ParamListKeys);

impl<'de> Deserialize<'de> for ParamListParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Some(keys) = value.as_object_mut().filter(|o| o.get("favorites") == Some(&Value::Bool(true))) {
            keys.remove("filter");
        }
        serde_path_to_error::deserialize(&value).map(ParamListParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for ParamListParams {
    fn schema_name() -> Cow<'static, str> {
        "ParamListParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ParamListKeys::json_schema(generator)
    }
}

/// One row of the parameters table.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParameterRow {
    pub name: String,
    pub expression: String,
    /// Its unit; `""` for none, `text` for a text parameter.
    pub unit: String,
    /// Its value in mm for a length and degrees for an angle, whatever its own unit; `null` if
    /// it can't be worked out (a text parameter has none).
    pub value: Option<f64>,
    /// Its value in its own unit, as the Parameters window shows it.
    pub display_value: Option<f64>,
    /// Why it can't be worked out, when it can't.
    pub error: Option<String>,
    pub comment: String,
    /// `user`, `sketch` (a dimension) or `feature` (a feature's named input).
    pub source: String,
    /// For a feature input: the feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    /// For a feature input: the input's label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    pub favorite: bool,
    /// The names its expression uses.
    pub uses: Vec<String>,
    /// The names whose expressions use it.
    pub used_by: Vec<String>,
    /// A text parameter's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl From<ParamRow> for ParameterRow {
    fn from(r: ParamRow) -> Self {
        ParameterRow {
            name: r.name,
            expression: r.expression,
            unit: r.unit,
            value: r.value,
            display_value: r.display_value,
            error: r.error,
            comment: r.comment,
            source: r.source.to_string(),
            feature: r.feature,
            input: r.input.map(str::to_string),
            favorite: r.favorite,
            uses: r.uses,
            used_by: r.used_by,
            text: r.text,
        }
    }
}

/// What `parameters.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamListResult {
    /// The parameters listed, favourites first.
    pub parameters: Vec<ParameterRow>,
    /// How many there are.
    pub count: usize,
    /// The names whose expressions refer to themselves, directly or through others.
    pub cycles: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// List what uses a parameter: other parameters, feature inputs, sketch dimensions,
/// configurations, joints, rules and canvases, and the features it drives. Changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ParamUsersParams {
    /// The parameter, by name: a user parameter, a dimension or a feature's named input.
    #[serde(deserialize_with = "param_name")]
    #[schemars(length(min = 1))]
    pub name: String,
}

/// Something that uses a parameter.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParameterUser {
    /// Another parameter's expression (a stored one or a feature input).
    Parameter { name: String },
    /// A feature input (`input` is its label, e.g. `Distance`).
    Feature { feature: u64, name: String, input: String },
    /// A sketch dimension (`constraint` is its id).
    Dimension { feature: u64, name: String, constraint: String },
    /// A configured column or an expression saved in a configuration row.
    Configuration { row: Option<String>, column: String },
    /// A joint's offset, angle or value, or a motion link's gear, rack or screw length (`input`
    /// says which).
    Joint { joint: u64, name: String, input: String },
    /// A sheet metal or plastic rule's value (`kind` says which, `field` which value).
    Rule { kind: String, name: String, field: String },
    /// The plane a canvas lies on (`input` is its label, e.g. `Offset`).
    Canvas { canvas: u64, name: String, input: String },
    /// An occurrence moved by an expression (`input` names the axis, e.g. `translate y`).
    Occurrence { occurrence: u64, name: String, input: String },
}

impl From<ParamUser> for ParameterUser {
    fn from(u: ParamUser) -> Self {
        match u {
            ParamUser::Parameter { name } => ParameterUser::Parameter { name },
            ParamUser::Feature { feature, name, input } => ParameterUser::Feature { feature, name, input },
            ParamUser::Dimension { feature, name, constraint } => ParameterUser::Dimension { feature, name, constraint },
            ParamUser::Configuration { row, column } => ParameterUser::Configuration { row, column },
            ParamUser::Joint { joint, name, input } => ParameterUser::Joint { joint, name, input },
            ParamUser::Rule { kind, name, field } => ParameterUser::Rule { kind, name, field },
            ParamUser::Canvas { canvas, name, input } => ParameterUser::Canvas { canvas, name, input },
            ParamUser::Occurrence { occurrence, name, input } => ParameterUser::Occurrence { occurrence, name, input },
        }
    }
}

/// A feature a parameter drives.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DrivenFeature {
    /// The feature's id.
    pub id: u64,
    pub name: String,
}

/// What `parameters.users` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamUsersResult {
    /// The parameter's name, spaces at either end dropped.
    pub name: String,
    /// What uses it directly.
    pub users: Vec<ParameterUser>,
    /// The features it drives, directly or through other parameters.
    pub drives: Vec<DrivenFeature>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `parameters.graph` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ParamGraphParams {}

/// What `parameters.graph` returns: the dependency graph of the parameters.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamGraphResult {
    /// Every parameter name: the user's, the dimensions' and the feature inputs'.
    pub nodes: Vec<String>,
    /// `[user, used]`: the first's expression uses the second; a user may be `Feature:<name>`.
    pub edges: Vec<[String; 2]>,
    /// The names whose expressions refer to themselves, directly or through others.
    pub cycles: Vec<String>,
    /// The parameters that can't be worked out, with why, by name.
    pub errors: BTreeMap<String, String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Export the user parameters as CSV or JSON text, and write it to a file when a path is
/// given. Changes nothing in the design.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ParamExportParams {
    /// `csv` or `json`, ignoring case. Default: `json` when `path` ends in `.json`, else `csv`.
    #[serde(default)]
    pub format: Option<String>,
    /// A file to write the text to. Default: none is written.
    #[serde(default)]
    pub path: Option<String>,
}

/// What `parameters.export` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamExportResult {
    /// The format used, `csv` or `json`.
    pub format: String,
    /// The file written; `null` if none was.
    pub path: Option<String>,
    /// The exported text.
    pub text: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `parameters.import` takes, as the handler reads it: see [`ParamImportParams`].
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = import_forms)]
pub struct ParamImportKeys {
    /// The table as text: CSV with a header (`name`, `expression`, and optionally `unit` and
    /// `comment`), or JSON `[{name, expression, unit?, comment?}]`. When given, `path` isn't
    /// read.
    #[serde(default, deserialize_with = "import_text", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub text: Option<String>,
    /// A file to read the table from, in the same forms. Read when `text` isn't given.
    #[serde(default, deserialize_with = "import_text", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub path: Option<String>,
}

/// `text` and `path`: text; `null` is left out, as `str_` reads it. Anything else gets the
/// message the handler gives when it finds neither.
fn import_text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "give `text` or `path`")
}

/// `parameters.import` needs `text` or `path` as text.
fn import_forms(schema: &mut Schema) {
    schema.insert(
        "anyOf".into(),
        json!([
            {"required": ["text"], "properties": {"text": {"type": "string"}}},
            {"required": ["path"], "properties": {"path": {"type": "string"}}}
        ]),
    );
}

/// Add the parameters in a CSV or JSON table, all or nothing. A name that exists is refused.
/// One undo step.
///
/// `path` is not read when `text` is text, as the handler leaves it.
#[derive(Debug)]
pub struct ParamImportParams(pub ParamImportKeys);

impl<'de> Deserialize<'de> for ParamImportParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Some(keys) = value.as_object_mut() {
            match keys.get("text") {
                Some(Value::String(_)) => {
                    keys.remove("path");
                }
                // Not text, so the handler reads `path`: when that works, this is the one thing
                // wrong; when it doesn't, the handler's message says what is needed.
                Some(Value::Number(_) | Value::Bool(_) | Value::Array(_) | Value::Object(_))
                    if matches!(keys.get("path"), Some(Value::String(_))) =>
                {
                    return Err(D::Error::custom(whole("`text` must be text")));
                }
                _ => {}
            }
        }
        serde_path_to_error::deserialize(&value).map(ParamImportParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for ParamImportParams {
    fn schema_name() -> Cow<'static, str> {
        "ParamImportParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ParamImportKeys::json_schema(generator)
    }
}

/// What `parameters.import` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParamImportResult {
    /// The names of the parameters added, in order.
    pub imported: Vec<String>,
    /// The features that failed to rebuild with them.
    pub errors: Vec<FailedFeature>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `expr.evaluate` takes, as the handler reads it: see [`ExprEvaluateParams`].
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = expression_required)]
pub struct ExprEvaluateKeys {
    /// The expression: text (`25 mm`, `W / 2`, `name = 3 * W`) or a number. With unit `text`, the
    /// text a text parameter would hold.
    // Optional in the type and required in the schema (`expression_required`): left out, the
    // handler refuses it with its own message.
    #[serde(default, deserialize_with = "evaluate_expression", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ExpressionText")]
    pub expression: Option<ExpressionText>,
    /// The unit the answer is shown in, and what bare numbers mean: `mm`, `cm`, `m`, `in`, `ft`,
    /// `deg`, `rad`, `""` (none) or `text` (a text parameter's text). Default: none; bare numbers
    /// follow the design's length unit.
    #[serde(default, deserialize_with = "evaluate_unit")]
    pub unit: Option<String>,
    /// What it must evaluate to: `length`, `angle` or `unitless` (`number` too). Default: the
    /// unit's kind, or what the expression is. Not read when `unit` is `text`.
    #[serde(default, deserialize_with = "evaluate_kind")]
    pub kind: Option<String>,
}

/// An expression given as text or as a number, as `expr_arg` reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionText(pub String);

impl JsonSchema for ExpressionText {
    fn schema_name() -> Cow<'static, str> {
        "ExpressionText".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "An expression as text, or a number.", "type": ["number", "string"]})
    }
}

fn evaluate_expression<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ExpressionText>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(Some(ExpressionText(s))),
        Value::Number(n) => n
            .as_f64()
            .filter(|v| v.is_finite())
            .map(|v| Some(ExpressionText(format!("{v}"))))
            .ok_or_else(|| D::Error::custom(whole("`expression` must be text or a number"))),
        _ => Err(D::Error::custom(whole("`expression` must be text or a number"))),
    }
}

fn evaluate_unit<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "`unit` must be text")
}

fn evaluate_kind<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "`kind` must be text")
}

fn expression_required(schema: &mut Schema) {
    require(schema, "expression");
}

/// Evaluate an expression against the design's parameters, without changing anything: its
/// value (in mm for a length, radians for an angle), its value in the unit asked for, what it
/// names, or why it fails. `name = expression` is worth its expression and says what it would
/// define.
///
/// `kind` is not read when `unit` is `text`, as the handler leaves it.
#[derive(Debug)]
pub struct ExprEvaluateParams(pub ExprEvaluateKeys);

impl<'de> Deserialize<'de> for ExprEvaluateParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Some(keys) = value.as_object_mut()
            && keys.get("unit").and_then(Value::as_str).is_some_and(solidcraft_doc::text_params::is_text_unit)
        {
            keys.remove("kind");
        }
        serde_path_to_error::deserialize(&value).map(ExprEvaluateParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for ExprEvaluateParams {
    fn schema_name() -> Cow<'static, str> {
        "ExprEvaluateParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ExprEvaluateKeys::json_schema(generator)
    }
}

/// What an evaluated expression is.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvalKind {
    Length,
    Angle,
    Unitless,
}

impl From<Kind> for EvalKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::Length => EvalKind::Length,
            Kind::Angle => EvalKind::Angle,
            Kind::Unitless => EvalKind::Unitless,
        }
    }
}

impl EvalKind {
    /// The word for it, as it's serialized.
    pub fn word(self) -> &'static str {
        match self {
            EvalKind::Length => "length",
            EvalKind::Angle => "angle",
            EvalKind::Unitless => "unitless",
        }
    }
}

/// What `expr.evaluate` returns: one of three shapes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExprEvaluated {
    #[serde(flatten)]
    pub body: ExprEvaluatedBody,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The shapes of an `expr.evaluate` reply.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ExprEvaluatedBody {
    /// With unit `text`: the text a text parameter would hold.
    Text {
        /// Always true.
        ok: bool,
        /// Always `text`.
        kind: String,
        /// The text, without the quotes round it.
        text: String,
        /// Always empty.
        references: Vec<String>,
    },
    /// The expression evaluated.
    Value {
        /// Always true.
        ok: bool,
        /// Its value: mm for a length, radians for an angle.
        value: f64,
        /// What it is.
        kind: EvalKind,
        /// Its value in the unit asked for; with none, degrees for an angle and the value itself
        /// otherwise.
        display_value: f64,
        /// The parameters it names.
        references: Vec<String>,
        /// For `name = expression`: the name it would define.
        #[serde(skip_serializing_if = "Option::is_none")]
        defines: Option<String>,
    },
    /// The expression didn't evaluate.
    Failed {
        /// Always false.
        ok: bool,
        /// Why.
        error: String,
        /// The parameters it names.
        references: Vec<String>,
        /// For `name = expression`: the name it would define.
        #[serde(skip_serializing_if = "Option::is_none")]
        defines: Option<String>,
    },
}

/// Suggest completions for the word at the end of some expression text: parameters (by name,
/// anywhere in it), functions, constants and units. Changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExprCompleteParams {
    /// The text before the cursor; the word being typed is its trailing letters, digits and
    /// underscores. Default: empty, which lists every parameter, function and constant.
    #[serde(default, deserialize_with = "complete_prefix")]
    pub prefix: Option<String>,
}

fn complete_prefix<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    text_or_null(d, "`prefix` must be text")
}

/// What a completion is.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompletionKind {
    Parameter,
    Function,
    Unit,
    Constant,
}

/// One suggestion.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Completion {
    /// The text to insert: a parameter's name, a function's name and `(`, a constant or a unit.
    pub text: String,
    pub kind: CompletionKind,
    /// A parameter's expression and value, a function's signature, or a unit's kind.
    pub detail: String,
}

/// What `parameters.complete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExprCompletions {
    /// The word being completed; empty when the text ends in something else.
    pub word: String,
    /// Parameters first (names that start with the word, then the rest; favourites and user
    /// parameters first, at most 200), then functions, the constant `pi` and units.
    pub suggestions: Vec<Completion>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "parameters_tests.rs"]
mod tests;
