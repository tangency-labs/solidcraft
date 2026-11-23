//! Contracts and result types for the saved-design file commands in `cmd/file_library.rs`:
//! `file.copy`, `file.move` and `file.trash`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether the file exists, keeps a design
//! extension, or belongs to a design that is open stays the handler's to check. A key the handler
//! refuses when it is left out (`path`, `destination`) is optional in the type and required in
//! the schema, so that the handler's message, not the check's "is required", is what a caller
//! sees.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::component_link::require;
use super::types::not_blank_pattern;

/// A design file's path as `cmd::file_library::arg` reads it: text with something in it, under
/// 4096 bytes, and no NUL character. `message` is that reader's own, for anything else, `null`
/// included. Whether the path keeps a supported design extension is the command's to refuse.
fn design_path<'de, D: Deserializer<'de>>(d: D, message: &str) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) if !s.trim().is_empty() && s.len() < 4096 && !s.contains('\0') => Ok(Some(s)),
        _ => Err(D::Error::custom(whole(message))),
    }
}

fn path_required<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    design_path(d, "A valid path is required")
}

fn destination_required<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    design_path(d, "A valid destination is required")
}

/// The schema for those two keys: what a path looks like, not which files the command takes.
fn path_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A design file's path: text with something in it, under 4096 bytes (the schema's maxLength counts characters), and no NUL character. The command itself refuses a path that does not keep a supported design extension.",
        "type": "string",
        "minLength": 1,
        "maxLength": 4095,
        "pattern": not_blank_pattern()
    })
}

/// `serde(default)` makes schemars publish `"default": null` on a key; drop it from a key the
/// schema requires.
fn drop_default(schema: &mut Schema) {
    schema.remove("default");
}

/// Both paths are required in the schema, though the type leaves them to the handler, whose
/// message a caller then hears.
fn transfer_required(schema: &mut Schema) {
    require(schema, "path");
    require(schema, "destination");
}

fn path_only_required(schema: &mut Schema) {
    require(schema, "path");
}

/// Copy or move a saved design file, as `file.copy` and `file.move` read their parameters. The
/// design that is open is not changed, and neither command makes an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = transfer_required)]
pub struct FileTransferParams {
    // Optional in the type and required in the schema (`transfer_required`), so that the handler's
    // "A valid path is required" is what a call without one hears.
    /// The saved design file to copy or move.
    #[serde(default, deserialize_with = "path_required")]
    #[schemars(schema_with = "path_schema", transform = drop_default)]
    pub path: Option<String>,
    /// Where to put it, which also renames the file when the folder stays the same. Nothing is
    /// replaced: a call whose destination already exists is refused. Both ends must keep a
    /// supported design extension, and a design whose components or drawings link other files
    /// keeps those links pointing at the same originals in the new place.
    #[serde(default, deserialize_with = "destination_required")]
    #[schemars(schema_with = "path_schema", transform = drop_default)]
    pub destination: Option<String>,
}

/// Move a saved design file to the system Trash, as `file.trash` reads its parameters. What is
/// open, and what its components link, is never trashed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = path_only_required)]
pub struct FileTrashParams {
    // Optional in the type and required in the schema (`path_only_required`).
    /// The saved design file to move to the system Trash.
    #[serde(default, deserialize_with = "path_required")]
    #[schemars(schema_with = "path_schema", transform = drop_default)]
    pub path: Option<String>,
}

/// What `file.copy` and `file.move` return: the design file and where it is now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FileTransferResult {
    /// The design file that was copied or moved.
    pub path: String,
    /// Where that file is now. For a copy this is a second file with the same contents; for a
    /// move the original name is gone.
    pub destination: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `file.trash` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FileTrashResult {
    /// The design file that was moved to the system Trash.
    pub path: String,
    /// Always true: the file is in the system Trash, where the operating system can put it back.
    /// SolidCraft never deletes a design file for good.
    pub trashed: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "file_library_tests.rs"]
mod tests;
