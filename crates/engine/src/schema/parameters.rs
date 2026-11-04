//! Contracts and result types for the commands in `cmd/parameters.rs` that manage the design's
//! user parameters by name (#11941): `parameters.add`, `.rename`, `.delete`, `.comment`,
//! `.favorite` and `.move`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a parameter exists, and whether a new
//! name, unit or expression is valid, stays the document's to check, with its messages.

use schemars::{JsonSchema, Schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;

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

/// `parameters.add` needs `expression`, or `value` in its place.
fn expression_or_value(schema: &mut Schema) {
    schema.insert("anyOf".into(), serde_json::json!([{"required": ["expression"]}, {"required": ["value"]}]));
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
#[schemars(deny_unknown_fields)]
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

#[cfg(test)]
#[path = "parameters_tests.rs"]
mod tests;
