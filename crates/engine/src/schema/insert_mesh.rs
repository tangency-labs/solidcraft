//! Contract and reply type for `file.insert_mesh` in `cmd/file.rs` (#13309).
//!
//! The handler reads its parameters from the raw JSON, so each key here reads as the handler
//! reads it and fails with the handler's message. Whether the file can be read stays the
//! handler's to check. A key the handler refuses when it is left out (`path`) is optional in the
//! type and required in the schema, so the handler's message, not the check's "is required", is
//! what a caller sees.
//!
//! Some keys are read only in some cases, and the contract reads them in the same cases:
//!
//! - nothing but `path` is read when the file is not a mesh file (the handler refuses it first);
//! - `up` is read only for a glTF file (any other file is refused with the handler's own message,
//!   whatever `up` says, so that message is left to the handler);
//! - `names` and `folder` are read only when `up` is given for a glTF file: the handler checks
//!   them there, as it does for a STEP file, and they change nothing for a mesh;
//! - `units` is read whatever the file (a glTF or 3MF file is refused it afterwards, which the
//!   handler says).
//!
//! A value of the wrong type that the handler reads as if the key were left out (a `name` that is
//! not text) is refused by the contract, as schemas.md has it: that difference is what warn and
//! strict report. A call with several problems may get a different one first in strict than with
//! the check off (the check goes key by key, the handler in its own order), except that a bad
//! `path` is always the first.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::{located, whole};
use super::types::{not_blank_pattern, whitespace_class};

/// What the handler says of a `path` that is missing, not text, blank or too long.
const PATH_REQUIRED: &str = "`path` is required";
const UP_MESSAGE: &str = "`up` must be \"z\" (the file's axes are kept) or \"y\" (the file has Y up: its Y becomes Z)";
const NAMES_MESSAGE: &str = "`names` must be \"auto\", \"solid\", \"product\" or \"instance\"";
const FOLDER_MESSAGE: &str =
    "`folder` must be true (an import that brings more than one item goes into a `Group #N` history folder) or false (no folder)";
const ORIGIN_MESSAGE: &str =
    "`origin` must be true (the mesh is moved so its box is centred on X and Y with its base on Z = 0) or false (the file's coordinates are kept)";

/// A key that is absent or `null` (the handler reads both as left out), else the value read by
/// `read`, with the handler's `message` when `read` refuses it.
fn read_key<'de, D: Deserializer<'de>, T>(d: D, message: &str, read: impl FnOnce(&Value) -> Option<T>) -> Result<Option<T>, D::Error> {
    let value = Value::deserialize(d)?;
    if value.is_null() {
        return Ok(None);
    }
    read(&value).map(Some).ok_or_else(|| D::Error::custom(whole(message)))
}

fn path_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) if !s.trim().is_empty() && s.len() < 4096 => Ok(Some(s)),
        _ => Err(D::Error::custom(whole(PATH_REQUIRED))),
    }
}

fn up_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<UpAxis>, D::Error> {
    read_key(d, UP_MESSAGE, |v| match v.as_str().map(|u| u.trim().to_ascii_lowercase()).as_deref() {
        Some("z") => Some(UpAxis::Z),
        Some("y") => Some(UpAxis::Y),
        _ => None,
    })
}

fn names_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<StepNamesParam>, D::Error> {
    read_key(d, NAMES_MESSAGE, |v| {
        let word = v.as_str().and_then(solidcraft_io::StepNames::parse)?;
        Some(match word {
            solidcraft_io::StepNames::Auto => StepNamesParam::Auto,
            solidcraft_io::StepNames::Solid => StepNamesParam::Solid,
            solidcraft_io::StepNames::Product => StepNamesParam::Product,
            solidcraft_io::StepNames::Instance => StepNamesParam::Instance,
        })
    })
}

fn units_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<MeshUnits>, D::Error> {
    let value = Value::deserialize(d)?;
    match &value {
        Value::Null => Ok(None),
        Value::String(u) => match solidcraft_doc::point_cloud::UNITS.iter().find(|(name, _)| name.eq_ignore_ascii_case(u)) {
            Some((name, _)) => Ok(Some(MeshUnits((*name).to_string()))),
            None => Err(D::Error::custom(whole(format!("`units` must be one of {} (not `{u}`)", unit_names())))),
        },
        _ => Err(D::Error::custom(whole(format!("`units` must be one of {}", unit_names())))),
    }
}

fn flag_key<'de, D: Deserializer<'de>>(d: D, message: &str) -> Result<Option<bool>, D::Error> {
    read_key(d, message, Value::as_bool)
}

fn folder_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag_key(d, FOLDER_MESSAGE)
}

fn origin_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag_key(d, ORIGIN_MESSAGE)
}

/// The unit names, as the handler lists them in its messages.
fn unit_names() -> String {
    solidcraft_doc::point_cloud::UNITS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}

/// The keys `file.insert_mesh` reads. See [`InsertMeshParams`], which leaves out the keys the
/// handler does not read for the call.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = path_required)]
pub struct InsertMeshKeys {
    // Optional in the type and required in the schema (`path_required`): left out, the handler
    // refuses it with its own message.
    /// The file to insert: `.3mf`, `.stl`, `.obj`, `.ply` (with faces), `.glb` or `.gltf` (any
    /// case), not blank and fewer than 4096 UTF-8 bytes. Any other kind of file is refused. Its
    /// meshes join the design as mesh bodies in a new component, placed in the active component.
    #[serde(default, deserialize_with = "path_key", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub path: Option<String>,
    /// The new component's name, and the feature's. Default (also when blank): the file's name
    /// without its extension.
    #[serde(default)]
    pub name: Option<String>,
    /// For a glTF file only (any other file is refused it): `y` (the default: glTF has Y up)
    /// turns the import's component a quarter turn about X, so Y becomes Z, and the reply gives
    /// its occurrence as `stand_up`; `z` keeps the file's axes. Any case.
    #[serde(default, deserialize_with = "up_key", skip_serializing_if = "Option::is_none")]
    pub up: Option<UpAxis>,
    /// Read only when `up` is given for a glTF file, where it is checked and changes nothing:
    /// `auto`, `solid`, `product` or `instance`, in any case. It is how a STEP file's bodies are
    /// named (`file.insert_step`).
    #[serde(default, deserialize_with = "names_key", skip_serializing_if = "Option::is_none")]
    pub names: Option<StepNamesParam>,
    /// Read only when `up` is given for a glTF file, where it is checked and changes nothing:
    /// true or false. It is whether a STEP import goes into a history folder (`file.insert_step`).
    #[serde(default, deserialize_with = "folder_key", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub folder: Option<bool>,
    /// For an STL, OBJ or PLY file: the unit its coordinates are in, scaled to millimetres as the
    /// file is read: `mm`, `cm`, `m`, `in` or `ft`, in any case. Default: mm. A 3MF or glTF file
    /// states its own, and is refused `units`. The design keeps the millimetre mesh and not the
    /// unit, so insert the file again to try another.
    #[serde(default, deserialize_with = "units_key", skip_serializing_if = "Option::is_none")]
    pub units: Option<MeshUnits>,
    /// Whether, once the mesh is read, scaled and stood up, the import's occurrence is moved so
    /// the box around its bodies is centred on X and Y of the active component with its lowest
    /// point on Z = 0 (the reply gives the shift as `moved_to_origin`). Default: false, so the
    /// mesh keeps the coordinates its file gives it.
    #[serde(default, deserialize_with = "origin_key", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub origin: Option<bool>,
}

/// Leaves out the keys the handler does not read for this call, in the order it looks at them.
fn leave_unread(value: &mut Value) {
    let Some(keys) = value.as_object_mut() else { return };
    let Some(path) = keys.get("path").and_then(Value::as_str).filter(|p| !p.trim().is_empty() && p.len() < 4096).map(str::to_string) else {
        return;
    };
    if !solidcraft_io::is_mesh_path(&path) && !solidcraft_io::is_ply_path(&path) {
        // Refused for its kind before anything else is read.
        for key in ["up", "names", "folder", "units", "origin"] {
            keys.remove(key);
        }
        return;
    }
    let up_given = keys.get("up").is_some_and(|u| !u.is_null());
    if !(up_given && solidcraft_io::is_gltf_path(&path)) {
        keys.remove("names");
        keys.remove("folder");
    }
    if !solidcraft_io::is_gltf_path(&path) {
        keys.remove("up");
    }
}

/// Insert a 3MF, STL, OBJ, PLY or glTF mesh file into the design as mesh bodies, in a new
/// component named after the file and placed in the active component. A glTF file's node tree
/// becomes nested components held as one rigid group until `component.unlock`, a mesh placed
/// again becomes another occurrence, its materials become the bodies' appearances, and its
/// metres become millimetres.
#[derive(Debug)]
pub struct InsertMeshParams(pub InsertMeshKeys);

impl<'de> Deserialize<'de> for InsertMeshParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        // A bad `path` is the handler's first refusal, whatever else is wrong.
        if let Some(path) = value.as_object().map(|o| o.get("path")) {
            let ok = path.and_then(Value::as_str).is_some_and(|p| !p.trim().is_empty() && p.len() < 4096);
            if !ok {
                let only = serde_json::json!({"path": path.cloned().unwrap_or(Value::Null)});
                return serde_path_to_error::deserialize::<_, InsertMeshKeys>(&only).map(InsertMeshParams).map_err(|e| D::Error::custom(located(&e)));
            }
        }
        leave_unread(&mut value);
        serde_path_to_error::deserialize(&value).map(InsertMeshParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for InsertMeshParams {
    fn schema_name() -> Cow<'static, str> {
        "InsertMeshParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = InsertMeshKeys::json_schema(generator);
        schema.insert("description".into(), INSERT_MESH.into());
        schema
    }
}

const INSERT_MESH: &str = "Insert a 3MF, STL, OBJ, PLY or glTF mesh file into the design as mesh bodies, in a new component named after the file and placed in the active component. A glTF file's node tree becomes nested components held as one rigid group until `component.unlock`, a mesh placed again becomes another occurrence, its materials become the bodies' appearances, and its metres become millimetres.";

/// `path` is required, as the handler has it (not blank, fewer than 4096 bytes; `maxLength`
/// counts characters, so 4095 is as close as the schema gets). `names` and `folder` have their
/// shapes only where the handler reads them: when `up` is given (not null) for a glTF file.
fn path_required(schema: &mut Schema) {
    super::motion::require(schema, "path");
    let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) else { return };
    props.insert(
        "path".into(),
        serde_json::json!({
            "description": props.get("path").and_then(|p| p.get("description")).cloned().unwrap_or_default(),
            "type": "string",
            "maxLength": 4095,
            "pattern": not_blank_pattern(),
        }),
    );
    let mut read_only_with_up = |key: &str| {
        let shape = props.get_mut(key).map(Value::take).unwrap_or_default();
        props.insert(key.into(), serde_json::json!({"description": shape.get("description").cloned().unwrap_or_default()}));
        shape
    };
    let names = read_only_with_up("names");
    let folder = read_only_with_up("folder");
    schema.insert("anyOf".into(), serde_json::json!([{"properties": {"up": {"type": "null"}}}, {"properties": {"names": names, "folder": folder}}]));
}

/// Which way is up in a glTF file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpAxis {
    /// The file's axes are kept.
    Z,
    /// The file has Y up: its Y becomes Z.
    Y,
}

/// Case-insensitive words (`word`), each letter in both cases, as a pattern; the pattern allows
/// the whitespace Rust's `trim` removes around them when `padded`.
fn words_pattern(words: &[&str], padded: bool) -> String {
    let word = |w: &str| w.chars().map(|c| format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase())).collect::<String>();
    let alternatives = words.iter().map(|w| word(w)).collect::<Vec<_>>().join("|");
    if padded {
        let ws = whitespace_class();
        format!("^[{ws}]*({alternatives})[{ws}]*$")
    } else {
        format!("^({alternatives})$")
    }
}

impl JsonSchema for UpAxis {
    fn schema_name() -> Cow<'static, str> {
        "UpAxis".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "`z` (the file's axes are kept) or `y` (the file has Y up: its Y becomes Z), in any case.",
            "type": "string",
            "pattern": words_pattern(&["z", "y"], true)
        })
    }
}

/// How a STEP file's bodies are named; read, and checked, for a glTF file with `up` too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepNamesParam {
    Auto,
    Solid,
    Product,
    Instance,
}

impl JsonSchema for StepNamesParam {
    fn schema_name() -> Cow<'static, str> {
        "StepNamesParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "`auto`, `solid`, `product` or `instance`, in any case.",
            "type": "string",
            "pattern": words_pattern(&["auto", "solid", "product", "instance"], true)
        })
    }
}

/// A length unit of an STL, OBJ or PLY file: one of `solidcraft_doc::point_cloud::UNITS`, in the
/// case the table spells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshUnits(pub String);

impl JsonSchema for MeshUnits {
    fn schema_name() -> Cow<'static, str> {
        "MeshUnits".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let names: Vec<&str> = solidcraft_doc::point_cloud::UNITS.iter().map(|(n, _)| *n).collect();
        json_schema!({
            "description": "`mm`, `cm`, `m`, `in` or `ft`, in any case.",
            "type": "string",
            "pattern": words_pattern(&names, false)
        })
    }
}

/// What `file.insert_mesh` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshInserted {
    /// The file, as given.
    pub path: String,
    /// The mesh import feature's id.
    pub feature: u64,
    /// The feature's name.
    pub name: String,
    /// The bodies the import made, by name.
    pub bodies: Vec<String>,
    /// The same names as `bodies`.
    pub created_bodies: Vec<String>,
    /// What the file reader noticed and put right.
    pub warnings: Vec<String>,
    /// The new component's id.
    pub component: u64,
    /// The new component's name.
    pub component_name: String,
    /// The component's occurrence in the active component; null when it has none.
    pub occurrence: Option<u64>,
    /// How many components were made below it (a glTF file's nodes). Only when there are some.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcomponents: Option<usize>,
    /// How many rigid joints hold its contents together. Only when `subcomponents` is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rigid_joints: Option<usize>,
    /// With `origin`: the shift, [x, y, z] in mm, that moved the import to the origin; absent
    /// when it was not moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to_origin: Option<[f64; 3]>,
    /// The unit the file's coordinates were read in, when `units` was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// For a glTF file stood up from Y up to Z up: the occurrence that was turned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stand_up: Option<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "insert_mesh_tests.rs"]
mod tests;
