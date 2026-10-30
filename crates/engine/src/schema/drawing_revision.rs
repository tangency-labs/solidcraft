//! Contracts and result types for the commands in `cmd/drawing_revision.rs` (#3874): the rows of a
//! drawing's revision record.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A revision row, by its id as `drawing.inspect` lists it.
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

/// What `drawing.revise` takes.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ReviseParams {
    /// The row to issue, as it reads. Left out, a new revision is added and issued.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "RowIdParam")]
    pub row: Option<RowIdParam>,
    /// A new revision's texts: not given with `row`, which changes nothing in it.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "RowSet")]
    pub set: Option<RowSet>,
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

/// The design a revision was issued from: both parts opaque text from the version history.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VersionResult {
    pub path: String,
    pub change: String,
}

/// What `drawing.revision.tag.add` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct TagAddParams {
    pub row: RowIdParam,
    /// The sheet, by id or name.
    #[serde(deserialize_with = "super::drawing_sheet::sheet_ref")]
    pub sheet: String,
    /// Its centre, in paper millimetres from the sheet's lower-left corner.
    pub at: [f64; 2],
    /// The length of its side, 1 to 50 mm (default 5).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub size_mm: Option<f64>,
    /// Shown in place of its row's revision.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub text: Option<String>,
}

/// A tag's place, size and text: each left out stays as it is.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct TagSet {
    /// The sheet, by id or name.
    #[serde(default, deserialize_with = "tag_sheet", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub sheet: Option<String>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "[f64; 2]")]
    pub at: Option<[f64; 2]>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub size_mm: Option<f64>,
    /// Shown in place of its row's revision, or `null` to show the revision again.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    pub text: Option<Option<String>>,
}

fn tag_sheet<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    super::drawing_sheet::sheet_ref(d).map(Some)
}

/// What `drawing.revision.tag.edit` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct TagEditParams {
    pub tag: RowIdParam,
    pub set: TagSet,
}

/// What `drawing.revision.tag.delete` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct TagDeleteParams {
    pub tag: RowIdParam,
}

/// One revision tag.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TagResult {
    pub tag: String,
    pub row: String,
    pub sheet: String,
    pub at: [f64; 2],
    pub size_mm: f64,
    /// What it shows: its row's revision, its own text, or, its row deleted, the revision it had.
    pub shows: String,
    /// Its row was deleted.
    pub lost: bool,
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
    /// An issued copy's: the design it was issued from, when there is a version to name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionResult>,
}

/// What the revision commands return.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct RevisionsResult {
    /// The row added, changed or deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    /// Every row, the oldest first: the newest is the drawing's revision.
    pub rows: Vec<RowResult>,
    /// The revision the next row gets, or none when nothing follows the highest.
    pub next: Option<String>,
    /// The issued history, the oldest first: each copy as it read when issued.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issued: Vec<RowResult>,
    /// What `drawing.revise` issued against, when there is a version to name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionResult>,
    /// What an edit changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<String>,
    /// The tag added, changed or deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Every revision tag on the drawing's sheets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagResult>,
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
