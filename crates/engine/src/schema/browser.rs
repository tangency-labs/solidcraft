//! Contracts and result types for the commands in `cmd/browser.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use crate::browser_find::{Kind, MAX_PATTERN_CHARS, Pattern, Search};

/// Search the Browser by name, as its search field does. Give `text`, `kinds` or both. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FindParams {
    /// What the names hold. Default: every name of the `kinds` given.
    #[serde(default)]
    pub text: Option<FindText>,
    /// The kinds of thing to list. Default: every kind.
    #[serde(default)]
    pub kinds: Option<FindKinds>,
}

/// `text`, read by the handler's own reader: `None` when blank.
#[derive(Debug)]
pub struct FindText(pub Option<Pattern>);

impl<'de> Deserialize<'de> for FindText {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::find_text(Some(&Value::deserialize(d)?)).map(FindText).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for FindText {
    fn schema_name() -> Cow<'static, str> {
        "FindText".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Text anywhere in the name, ignoring case. With `*` (any text) or `?` (any one character) it must match the whole name instead: `*:1` is every name that ends in `:1`. Spaces around it don't count, and blank text counts as none.",
            "type": "string",
            "maxLength": MAX_PATTERN_CHARS
        })
    }
}

/// `kinds`, read by the handler's own reader.
#[derive(Debug)]
pub struct FindKinds(pub Vec<Kind>);

impl<'de> Deserialize<'de> for FindKinds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::find_kinds(Some(&Value::deserialize(d)?)).map(FindKinds).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for FindKinds {
    fn schema_name() -> Cow<'static, str> {
        "FindKinds".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let ids: Vec<&str> = Kind::ALL.iter().map(|k| k.id()).collect();
        json_schema!({
            "description": "A list of kinds: a placement of a component, a body, a sketch, a construction plane, a joint. An empty list means every kind.",
            "type": "array",
            "items": {"enum": ids},
            "maxItems": Kind::ALL.len()
        })
    }
}

/// What `browser.find` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FindResult {
    #[serde(flatten)]
    pub search: Search,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
