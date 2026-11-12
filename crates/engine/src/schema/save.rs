//! Contracts and result types for the commands in `cmd/file.rs` that save the design:
//! `file.save` and `file.save_as` (#13307).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it. `file.save_as` refuses a missing, blank or over-long `path` with its own message,
//! so there `path` is optional in the type and required in the schema. `format` and
//! `compression` are text in the type: the handler refuses a word it doesn't know, whichever of
//! the two commands was called, with a message that names `file.save` (kept as it is), so the
//! schema lists the words and the handler stays the one to refuse them. Whether the path can be
//! written, or the design has moved or changed on disk, stays the handler's to check.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::motion::require;
use super::types::not_blank_pattern;

/// `file.save_as`' `path` as `path_arg` reads it: text with something in it, under 4096 bytes;
/// anything else, `null` included, is refused with the handler's message.
fn save_as_path<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) if !s.trim().is_empty() && s.len() < 4096 => Ok(Some(s)),
        _ => Err(D::Error::custom(whole("`path` is required"))),
    }
}

fn save_as_path_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": not_blank_pattern(), "maxLength": 4095})
}

fn path_required(schema: &mut Schema) {
    require(schema, "path");
}

fn format_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["container", "json"]})
}

fn compression_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["fast", "compact"]})
}

/// Write the design to a file and make that file the design's own: the next `file.save` writes
/// it again. With no `path`, the design's current file is written; a design that has none yet
/// needs a `path`. A name that opens as another kind of file (`.step`, `.stl`, `.dxf` and the
/// like) is refused: `file.export` writes those. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SaveParams {
    /// The design file to write. Default: the design's current file. A blank value counts as
    /// left out.
    #[serde(default)]
    pub path: Option<String>,
    /// container (`.sc`) or json. Default: json for a path ending in `.json`, otherwise
    /// container. The handler refuses any other word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "format_schema")]
    pub format: Option<String>,
    /// fast or compact. Default: fast. The handler refuses any other word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "compression_schema")]
    pub compression: Option<String>,
    /// Keep the evaluated model in the file, so the design opens without rebuilding. Default:
    /// true. Plain json has no room for it.
    #[serde(default)]
    pub cache: Option<bool>,
    /// Keep the display meshes in the file. Default: false.
    #[serde(default)]
    pub preview: Option<bool>,
    /// Replace the design's file even though another program or computer changed it since it was
    /// opened or saved; without it that save is refused. Only matters when the path is the
    /// design's own file. Default: false.
    #[serde(default)]
    pub overwrite: Option<bool>,
    /// Sign the saved design with GnuPG, appending one record to its provenance. Default: false.
    #[serde(default)]
    pub sign: Option<bool>,
    /// The GnuPG key to sign with: a fingerprint or user id. Default, or a blank value: GnuPG's
    /// default signing key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key: Option<String>,
}

/// Write the design to a new file (a design file name, `.sc` as a rule) and make that file the
/// design's own. A name that opens as another kind of file (`.step`, `.stl`, `.dxf` and the like)
/// is refused: `file.export` writes those. A design with no name of its own, or one borrowed from
/// the file it was saved as before, takes the new file's name. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = path_required)]
pub struct SaveAsParams {
    // Optional in the type and required in the schema (`path_required`).
    /// The design file to write: text, not blank, under 4096 bytes.
    #[serde(default, deserialize_with = "save_as_path", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "save_as_path_schema")]
    pub path: Option<String>,
    /// container (`.sc`) or json. Default: json for a path ending in `.json`, otherwise
    /// container. The handler refuses any other word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "format_schema")]
    pub format: Option<String>,
    /// fast or compact. Default: fast. The handler refuses any other word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "compression_schema")]
    pub compression: Option<String>,
    /// Keep the evaluated model in the file, so the design opens without rebuilding. Default:
    /// true. Plain json has no room for it.
    #[serde(default)]
    pub cache: Option<bool>,
    /// Keep the display meshes in the file. Default: false.
    #[serde(default)]
    pub preview: Option<bool>,
    /// As for `file.save`, when `path` is the design's own file: replace it even though another
    /// program or computer changed it. Default: false.
    #[serde(default)]
    pub overwrite: Option<bool>,
    /// Sign the saved design with GnuPG, appending one record to its provenance. Default: false.
    #[serde(default)]
    pub sign: Option<bool>,
    /// The GnuPG key to sign with: a fingerprint or user id. Default, or a blank value: GnuPG's
    /// default signing key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key: Option<String>,
}

/// What a signed save put in the file.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SignedSave {
    /// The signature scheme: `gpg` for an OpenPGP record made by GnuPG.
    pub method: String,
    /// The fingerprint of the (sub)key that signed, uppercase hex.
    pub fingerprint: String,
    /// The primary user id of the signing key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// What `file.save` and `file.save_as` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SaveResult {
    /// The file written.
    pub path: String,
    /// Its size in bytes.
    pub bytes: usize,
    /// The design's name once saved (a design with no name of its own takes the file's).
    pub name: String,
    /// A file a newer SolidCraft saved: the format number it keeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_format: Option<String>,
    /// A newer file: what could not be kept, in sentences, when something could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    /// A newer file: what could not be kept, one item each, when something could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_kept: Option<Vec<String>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
    /// With `sign: true`: who signed the file that was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignedSave>,
}

#[cfg(test)]
#[path = "save_tests.rs"]
mod tests;
