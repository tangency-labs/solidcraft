//! Contracts and result types for the commands in `cmd/drawing_revision.rs` (#3874): the rows of a
//! drawing's revision record.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A revision row, by its id as `drawing.revision.list` gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowIdParam(pub u64);

impl<'de> Deserialize<'de> for RowIdParam {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => {
                solidcraft_doc::ids::decode(&s).map(RowIdParam).ok_or_else(|| serde::de::Error::custom("must be a revision row's id"))
            }
            _ => Err(serde::de::Error::custom("must be a revision row's id")),
        }
    }
}

impl JsonSchema for RowIdParam {
    fn schema_name() -> Cow<'static, str> {
        "RevisionRowId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A revision row's id: 13 characters of lowercase Crockford base32.", "type": "string", "pattern": "^[0-9a-hjkmnp-tv-z]{13}$"})
    }
}

/// A row's texts as typed, and whether the table hides it: each left out stays as it is.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct RowSet {
    /// One line: a number, or letters when the drawing letters its revisions.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub revision: Option<String>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub designer: Option<String>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub checker: Option<String>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub date: Option<String>,
    /// Hidden from the revision table; still the newest revision while it is the last row.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub hidden: Option<bool>,
}

/// What `drawing.revision.add` takes.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct RevisionAddParams {
    /// The new row's texts (when not given, only the next revision).
    #[serde(default)]
    pub set: RowSet,
}

/// What `drawing.revision.edit` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct RevisionEditParams {
    pub row: RowIdParam,
    pub set: RowSet,
}

/// What `drawing.revision.delete` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct RevisionDeleteParams {
    pub row: RowIdParam,
}

/// One row, as the revision table shows it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RowResult {
    pub row: String,
    pub revision: String,
    pub description: String,
    pub designer: String,
    pub checker: String,
    pub date: String,
    pub hidden: bool,
}

/// What `drawing.revision.list` returns, and the other revision commands with it.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct RevisionsResult {
    /// The row added, changed or deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    /// Every row, the oldest first: the newest is the drawing's revision.
    pub rows: Vec<RowResult>,
    /// The revision the next row gets, or none when nothing follows the highest.
    pub next: Option<String>,
    /// What an edit changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<String>,
    /// The tags a deleted row leaves showing the revision they had, marked lost.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags_lost: Vec<String>,
    /// A revision in letters with a letter the lettering skips, kept as typed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_revision_tests.rs"]
mod tests;
