//! Contract and reply type for `file.insert_step` in `cmd/file.rs` (#14918).
//!
//! The handler reads its parameters from the raw JSON, so each key here reads as the handler
//! reads it and fails with the handler's message. `up` and `names` are the shared types `doc.open`
//! describes, and `matrix` and `tolerance` those of `occurrence.move`. Whether the file is one of
//! the kinds it takes, and whether a `matrix` is a rigid move, stay the handler's to refuse:
//! `matrix::rigid` names what it rejects, in its own words.
//!
//! `path` is optional in the type and required in the schema, so a call without it gets the
//! handler's "`path` is required" rather than the check's wording. The keys the handler does not
//! read for a call are left unread, in the order it looks at them:
//!
//! - anything but `path` for a file whose extension it refuses, which it answers before reading
//!   another key;
//! - `tolerance` without `matrix`, which is where the handler passes it to `matrix::rigid`.
//!
//! `tolerance` itself is read as given, as `occurrence.move` reads it: the schema gives it its
//! shape only where the handler reads it, and `matrix::tolerance` is the one that refuses a bad
//! value, in its own words.
//!
//! A value of the wrong type that the handler reads as if the key were left out (a `name` that is
//! not text) is refused by the contract, as schemas.md has it: that difference is what warn and
//! strict report. A call with several problems may get a different one first in strict than with
//! the check off (the check goes key by key, the handler in its own order), except that a bad
//! `path` is always the first.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::check::{located, whole};
use super::component::{MatrixParam, MatrixTolerance};
use super::diagnostics::ProfileReport;
use super::file::{HistoryParam, NamesParam, UpParam};
use super::types::not_blank_pattern;

/// What `folder` says when it is neither true nor false, as `step_options` has it.
const FOLDER_MESSAGE: &str =
    "`folder` must be true (an import that brings more than one item goes into a `Group #N` history folder) or false (no folder)";

/// `path` as `path_arg` reads it: text that is not blank and under 4096 bytes, refused with the
/// handler's own message.
fn path_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(path) if crate::cmd::usable_path(&path) => Ok(Some(path)),
        _ => Err(D::Error::custom(whole("`path` is required"))),
    }
}

/// `folder`: true or false, `null` as left out, with the handler's message for anything else.
fn folder_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    let value = Value::deserialize(d)?;
    if value.is_null() {
        return Ok(None);
    }
    value.as_bool().map(Some).ok_or_else(|| D::Error::custom(whole(FOLDER_MESSAGE)))
}

/// What `force` says when it is neither true nor false, as `step_options` has it.
const FORCE_MESSAGE: &str = "`force` must be true (read the file even if it may need more memory than this computer can spare) or false";

/// `force`: true or false, `null` as left out, with the handler's message for anything else.
fn force_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    let value = Value::deserialize(d)?;
    if value.is_null() {
        return Ok(None);
    }
    value.as_bool().map(Some).ok_or_else(|| D::Error::custom(whole(FORCE_MESSAGE)))
}

/// `matrix`: as given, `null` as left out, which is how the handler has it.
fn matrix_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<MatrixParam>, D::Error> {
    let value = Value::deserialize(d)?;
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(MatrixParam(value)))
}

/// The keys `file.insert_step` reads. See [`InsertStepParams`], which leaves out the keys the
/// handler does not read for the call.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = insert_step_schema)]
pub struct InsertStepKeys {
    // Optional in the type and required in the schema (`insert_step_schema`): left out, the
    // handler refuses the call with its own message.
    /// The file to insert: `.step`, `.stp`, `.igs`, `.iges`, `.fcstd` or `.3dm` (any case), not
    /// blank and fewer than 4096 UTF-8 bytes. Any other kind of file is refused.
    #[serde(default, deserialize_with = "path_key", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub path: Option<String>,
    /// The Import feature's name, and the new component's. Default (also when blank): the root
    /// product's name in the file, else the file's.
    #[serde(default)]
    pub name: Option<String>,
    /// Which way is up in the file: `z` (its axes are kept, the default) or `y` (the file has Y
    /// up, so the import's component is turned a quarter turn about X and its Y becomes Z).
    #[serde(default)]
    pub up: Option<UpParam>,
    /// What the file's bodies are named after: `auto` (the default), `solid`, `product` or
    /// `instance`.
    #[serde(default)]
    pub names: Option<NamesParam>,
    /// true puts an import that brings more than one item into the history (its Import feature
    /// and the rigid joints holding its parts) into a new history folder named `Group #N`.
    /// Default: false.
    #[serde(default, deserialize_with = "folder_key", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub folder: Option<bool>,
    /// For a `.step` or `.stp` file: one that may need more memory than half of this computer's
    /// (32 times its size) is refused before it is read, naming the memory; true reads it anyway.
    /// Default: false.
    #[serde(default, deserialize_with = "force_key", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub force: Option<bool>,
    /// For a FreeCAD file: its sketches and features rebuilt as SolidCraft features (the
    /// default), or only its solids as one Import feature.
    #[serde(default)]
    pub history: Option<HistoryParam>,
    /// Where the import goes in the active component, as a rigid move applied after `up`: a
    /// row-major 3×4 (12 numbers) or 4×4 (16 numbers) matrix, flat or as rows, as
    /// `occurrence.move` takes it. The reply gives the placement it made as `transform`.
    #[serde(default, deserialize_with = "matrix_key", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<MatrixParam>")]
    pub matrix: Option<MatrixParam>,
    /// With `matrix` only: the largest accepted |RᵀR − I| per entry, above 0 and at most 0.001.
    /// Default: 1e-9. Read nowhere else, and `matrix::rigid` is the one that refuses both.
    #[serde(default)]
    pub tolerance: Option<MatrixTolerance>,
}

/// Leaves out the keys the handler does not read for this call, in the order it looks at them.
fn leave_unread(value: &mut Value) {
    let Some(keys) = value.as_object_mut() else { return };
    let Some(path) = keys.get("path").and_then(Value::as_str).filter(|path| crate::cmd::usable_path(path)) else {
        return;
    };
    if !solidcraft_io::is_step_path(path) {
        // A file of another kind is refused before any other key is read.
        for key in ["name", "up", "names", "folder", "history", "matrix", "tolerance"] {
            keys.remove(key);
        }
        return;
    }
    if !keys.get("matrix").is_some_and(|matrix| !matrix.is_null()) {
        // `matrix::tolerance` is read only where `matrix` is.
        keys.remove("tolerance");
    }
}

/// Insert a STEP, IGES, FreeCAD or Rhino file into the design as an Import base feature, in a new
/// component named after the file and placed in the active component. An assembly's products
/// become nested components held as one rigid group until `component.unlock`, and a part the file
/// places more than once becomes one component with an occurrence per placement.
#[derive(Debug)]
pub struct InsertStepParams(pub InsertStepKeys);

impl<'de> Deserialize<'de> for InsertStepParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        // A missing or unusable `path` is the handler's first refusal, whatever else is wrong.
        if let Some(path) = value.as_object().map(|keys| keys.get("path"))
            && !path.and_then(Value::as_str).is_some_and(crate::cmd::usable_path)
        {
            let only = json!({"path": path.cloned().unwrap_or(Value::Null)});
            return serde_path_to_error::deserialize::<_, InsertStepKeys>(&only).map(InsertStepParams).map_err(|e| D::Error::custom(located(&e)));
        }
        leave_unread(&mut value);
        serde_path_to_error::deserialize(&value).map(InsertStepParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for InsertStepParams {
    fn schema_name() -> Cow<'static, str> {
        "InsertStepParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = InsertStepKeys::json_schema(generator);
        schema.insert("description".into(), INSERT_STEP.into());
        schema
    }
}

const INSERT_STEP: &str = "Insert a STEP, IGES, FreeCAD or Rhino file into the design as an Import base feature, in a new component named after the file and placed in the active component. An assembly's products become nested components held as one rigid group until `component.unlock`, and a part the file places more than once becomes one component with an occurrence per placement.";

/// `path` is required, as the handler has it (not blank, fewer than 4096 bytes; `maxLength`
/// counts characters, so 4095 is as close as the schema gets). `tolerance` has its shape only
/// where the handler reads it: with `matrix`, which is the handler's to refuse.
fn insert_step_schema(schema: &mut Schema) {
    super::motion::require(schema, "path");
    let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) else { return };
    props.insert(
        "path".into(),
        json!({
            "description": props.get("path").and_then(|path| path.get("description")).cloned().unwrap_or_default(),
            "type": "string",
            "maxLength": 4095,
            "pattern": not_blank_pattern(),
        }),
    );
    let shape = props.get_mut("tolerance").map(Value::take).unwrap_or_default();
    props.insert("tolerance".into(), json!({"description": shape.get("description").cloned().unwrap_or_default()}));
    schema.insert("anyOf".into(), json!([{"properties": {"matrix": {"type": "null"}}}, {"properties": {"tolerance": shape}}]));
}

/// What `file.insert_step` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StepInserted {
    /// The file, as given.
    pub path: String,
    /// The Import feature's id.
    pub feature: u64,
    /// The feature's name.
    pub name: String,
    /// The bodies the import made, by name.
    pub bodies: Vec<String>,
    /// The same names as `bodies`.
    pub created_bodies: Vec<String>,
    /// What the file reader noticed and put right.
    pub warnings: Vec<String>,
    /// A FreeCAD file only: `"features"` when its sketches and features were rebuilt; absent
    /// when its solids came in as one Import feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
    /// FreeCAD only, with `history: features`: the ids of the features the rebuild added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features_added: Option<Vec<u64>>,
    /// The program the file's header names as writing it.
    pub system: String,
    /// The SHA-256 of the file's exact bytes, as lower-case hex.
    pub content_sha256: String,
    /// The exporter the file names, and which way up its files are known to be (`y` or `z`). Only
    /// for a program whose files are known to need standing up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exporter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exporter_up: Option<String>,
    /// The new component's id.
    pub component: u64,
    /// The new component's name.
    pub component_name: String,
    /// The component's occurrence in the active component; null when it has none.
    pub occurrence: Option<u64>,
    /// How many components were made below it (an assembly's parts and sub-assemblies). Only when
    /// there are some.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcomponents: Option<usize>,
    /// How many rigid joints hold its contents together. Only when `subcomponents` is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rigid_joints: Option<usize>,
    /// With `up: y`: the occurrence that was turned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stand_up: Option<u64>,
    /// With `folder: true`: the history folder the import's feature and joints went into. Only
    /// when the import has joints to put in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u64>,
    /// With `matrix`: the import's placement in the active component after the move, as the design
    /// stores a placement: 4 rows of 4 numbers, its translation in the last row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[[f64; 4]; 4]>,
    /// While the profiler records: the import's share of what it recorded, as `diagnostics.profile`
    /// reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileReport>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "insert_step_tests.rs"]
mod tests;
