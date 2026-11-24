//! Contracts and result types for `file.save_mesh` in `cmd/file.rs`.
//!
//! The handler reads its parameters from the raw JSON, so each key here reads as the handler reads
//! it and fails with the handler's message. Whether a body exists, a slicer is installed or the file
//! can be written stays the handler's to check. A call with several problems may get a different
//! one first in strict than with the check off (the check goes key by key, the handler in its own
//! order).
//!
//! `path` is the one key the handler refuses when it is left out: it is optional in the type, so the
//! handler keeps its message, and required in the schema.
//!
//! Two keys are read only in some cases, and the contract reads them in the same cases: `ascii`
//! only when `format` is `stl` (in any case), and `single_instance` only when `slicer` is text. A
//! value of the wrong type that the handler reads as if the key were left out (`ascii: "yes"` next
//! to `format: "stl"`, a number for `slicer`) is refused by the contract, as schemas.md has it: that
//! difference is what warn and strict report.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::check::{located, whole};
use super::types::{not_blank_pattern, whitespace_class};

/// A key that, once given (`null` included), must parse as `T`, as the handler reads it.
fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// The keys `file.save_mesh` reads. See [`SaveMeshParams`], which leaves `ascii` and
/// `single_instance` unread in the cases the handler does.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = conditional_keys)]
pub struct SaveMeshKeys {
    /// The file to write: not blank, fewer than 4096 UTF-8 bytes.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MeshPath")]
    pub path: Option<MeshPath>,
    /// The file format, in any case: `stl`, `3mf`, or `3mf-tagged` (3MF with a face tag on each
    /// triangle, docs/formats/3mf-face-tags.md). Default: from the extension of `path` (`.3mf`
    /// gives 3mf), else stl.
    #[serde(default)]
    pub format: Option<MeshFormat>,
    /// The bodies to save, by name. Default: every body.
    #[serde(default)]
    pub bodies: Option<MeshBodies>,
    /// STL only (read only when `format` is `stl`): true writes ASCII STL instead of binary.
    /// Default: false.
    #[serde(default)]
    pub ascii: Option<bool>,
    /// A slicer to send the saved file to, after it is written where `path` says, through the
    /// same steps as `file.send_to_slicer`: an id or a name from `file.slicers`, the program path
    /// it lists, or a program the user added under Custom…. A slicer that has to be started first
    /// is waited for. Kept for scripts; may be deprecated later in favour of `file.send_to_slicer`
    /// or a Send to App workflow. Default: none.
    #[serde(default)]
    pub slicer: Option<String>,
    /// Read only when `slicer` is given: true opens the file in the window the slicer already has
    /// open, for slicers that load files that way (see `file.send_to_slicer`). Default: true.
    #[serde(default)]
    pub single_instance: Option<bool>,
    /// How closely the mesh follows curved faces (STL, OBJ and 3MF), in any case: `standard`, `fine`
    /// or `finest`, a chord tolerance of 2e-4, 5e-5 or 1e-5 of each body's size. Default: standard.
    #[serde(default)]
    pub quality: Option<MeshQualityParam>,
    /// 3MF only: `body` is one object per body; `component` is one object for each placed
    /// component with two or more bodies, with those bodies as its parts, so a slicer loads them as
    /// one object with parts (other bodies stay objects of their own). `component` is refused for
    /// other formats. Default: body.
    #[serde(default)]
    pub objects: Option<MeshObjects>,
    /// True saves each body with a print face (`body.print_face`) laid on it, as a slicer export
    /// lays it: turned so the face points down at Z = 0, its centre of mass over the origin. Each
    /// body is laid on its own, so several land on the origin together; a part of a 3MF object
    /// (`objects: component`) stays as it lies. A file sent to `slicer` is laid on them whatever
    /// this says. Default: false.
    #[serde(default)]
    pub lay_on_print_faces: Option<bool>,
}

/// Leaves out the keys the handler never reads for this call: `ascii` unless `format` is `stl`,
/// and `single_instance` unless `slicer` is text.
fn leave_unread(value: &mut Value) {
    let Some(keys) = value.as_object_mut() else { return };
    let stl = keys.get("format").and_then(Value::as_str).is_some_and(|f| f.eq_ignore_ascii_case("stl"));
    if !stl {
        keys.remove("ascii");
    }
    if !keys.get("slicer").is_some_and(Value::is_string) {
        keys.remove("single_instance");
    }
}

fn read_keys<'de, D: Deserializer<'de>>(d: D) -> Result<SaveMeshKeys, D::Error> {
    let mut value = Value::deserialize(d)?;
    leave_unread(&mut value);
    serde_path_to_error::deserialize(&value).map_err(|e| D::Error::custom(located(&e)))
}

/// Save bodies as a mesh file: binary or ASCII STL, 3MF, or 3MF with a face tag on each triangle.
/// The format is `format`, else the extension of `path`, else STL. With `slicer`, the saved file
/// is then sent to that program. Nothing is added to undo.
#[derive(Debug)]
pub struct SaveMeshParams(pub SaveMeshKeys);

impl<'de> Deserialize<'de> for SaveMeshParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        read_keys(d).map(SaveMeshParams)
    }
}

impl JsonSchema for SaveMeshParams {
    fn schema_name() -> Cow<'static, str> {
        "SaveMeshParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = SaveMeshKeys::json_schema(generator);
        schema.insert("description".into(), SAVE_MESH.into());
        schema
    }
}

const SAVE_MESH: &str = "Save bodies as a mesh file: binary or ASCII STL, 3MF, or 3MF with a face tag on each triangle. The format is `format`, else the extension of `path`, else STL. With `slicer`, the saved file is then sent to that program. Nothing is added to undo.";

/// The pattern for `format`: the three words, in any case.
const FORMAT_PATTERN: &str = "^([Ss][Tt][Ll]|3[Mm][Ff]|3[Mm][Ff]-[Tt][Aa][Gg][Gg][Ee][Dd])$";

/// The pattern for `quality`: the three words, in any case, with whitespace around them allowed
/// (the handler trims).
fn quality_pattern() -> String {
    let ws = whitespace_class();
    format!("^[{ws}]*([Ss][Tt][Aa][Nn][Dd][Aa][Rr][Dd]|[Ff][Ii][Nn][Ee]|[Ff][Ii][Nn][Ee][Ss][Tt])[{ws}]*$")
}

/// `path` is required (the handler says so when it is left out); `ascii` and `single_instance`
/// have their shapes only where the handler reads them, as [`leave_unread`] has it.
fn conditional_keys(schema: &mut Schema) {
    super::motion::require(schema, "path");
    let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) else { return };
    let mut read_only_when = |key: &str| {
        let shape = props.get_mut(key).map(Value::take).unwrap_or_default();
        props.insert(key.into(), json!({"description": shape.get("description").cloned().unwrap_or_default()}));
        shape
    };
    let ascii = read_only_when("ascii");
    let single_instance = read_only_when("single_instance");
    // `format` is `stl` in any case: the eight spellings, which `enum` says and a pattern would only approximate.
    let spellings: Vec<String> =
        (0..8).map(|n| "stl".chars().enumerate().map(|(i, c)| if n >> i & 1 == 1 { c.to_ascii_uppercase() } else { c }).collect()).collect();
    let stl = json!({"required": ["format"], "properties": {"format": {"enum": spellings}}});
    let slicer = json!({"required": ["slicer"], "properties": {"slicer": {"type": "string"}}});
    schema.insert(
        "allOf".into(),
        json!([
            {"anyOf": [{"not": stl}, {"properties": {"ascii": ascii}}]},
            {"anyOf": [{"not": slicer}, {"properties": {"single_instance": single_instance}}]},
        ]),
    );
}

/// The file to write, as `path_arg` reads `path`: not blank and under 4096 bytes, or the
/// handler's "`path` is required" (anything but text reads as no `path`).
#[derive(Clone, Debug, PartialEq)]
pub struct MeshPath(pub String);

impl<'de> Deserialize<'de> for MeshPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if !s.trim().is_empty() && s.len() < 4096 => Ok(MeshPath(s)),
            _ => Err(D::Error::custom(whole("`path` is required"))),
        }
    }
}

impl JsonSchema for MeshPath {
    fn schema_name() -> Cow<'static, str> {
        "MeshPath".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // `maxLength` counts characters, so 4095 is as close as the schema gets to "under 4096 bytes".
        json_schema!({
            "description": "The file to write: not blank, fewer than 4096 UTF-8 bytes.",
            "type": "string",
            "maxLength": 4095,
            "pattern": not_blank_pattern()
        })
    }
}

/// The bodies to save, read by the handler's own `string_list`: a name or a list of names, with its
/// messages for anything else.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshBodies(pub Vec<String>);

impl<'de> Deserialize<'de> for MeshBodies {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let keys = json!({"bodies": Value::deserialize(d)?});
        match crate::params::string_list("file.save_mesh", &keys, "bodies") {
            Ok(names) => Ok(MeshBodies(names)),
            Err(crate::EngineError::BadParams { msg, .. }) => Err(D::Error::custom(whole(msg))),
            Err(e) => Err(D::Error::custom(whole(e))),
        }
    }
}

impl JsonSchema for MeshBodies {
    fn schema_name() -> Cow<'static, str> {
        "MeshBodies".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A body name, or a list of body names (at most 10000).",
            "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "maxItems": crate::params::MAX_STRING_LIST}]
        })
    }
}

/// A mesh format, as `file.save_mesh` reads `format` (the words in any case).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshFormat {
    /// STL, binary unless `ascii` is true.
    Stl,
    /// 3MF.
    ThreeMf,
    /// 3MF with a face tag on each triangle.
    ThreeMfTagged,
}

impl<'de> Deserialize<'de> for MeshFormat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match String::deserialize(d)?.to_ascii_lowercase().as_str() {
            "stl" => Ok(MeshFormat::Stl),
            "3mf" => Ok(MeshFormat::ThreeMf),
            "3mf-tagged" => Ok(MeshFormat::ThreeMfTagged),
            other => Err(D::Error::custom(whole(format!("unknown mesh format `{other}` (stl, 3mf, 3mf-tagged)")))),
        }
    }
}

impl JsonSchema for MeshFormat {
    fn schema_name() -> Cow<'static, str> {
        "MeshFormat".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "stl, 3mf or 3mf-tagged, in any case.",
            "type": "string",
            "pattern": FORMAT_PATTERN
        })
    }
}

/// How closely the mesh follows curved faces, as `quality` reads it (the words in any case,
/// whitespace around them ignored).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshQualityParam {
    /// A chord tolerance of 2e-4 of each body's size.
    Standard,
    /// 5e-5 of each body's size.
    Fine,
    /// 1e-5 of each body's size.
    Finest,
}

impl<'de> Deserialize<'de> for MeshQualityParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let named = v.as_str().filter(|s| solidcraft_io::MeshQuality::from_name(s).is_some()).map(|s| s.trim().to_ascii_lowercase());
        match named.as_deref() {
            Some("standard") => Ok(MeshQualityParam::Standard),
            Some("fine") => Ok(MeshQualityParam::Fine),
            Some("finest") => Ok(MeshQualityParam::Finest),
            _ => Err(D::Error::custom(whole(format!("`quality` must be standard, fine or finest, not {v}")))),
        }
    }
}

impl JsonSchema for MeshQualityParam {
    fn schema_name() -> Cow<'static, str> {
        "MeshQualityParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "standard (a chord tolerance of 2e-4 of each body's size), fine (5e-5) or finest (1e-5), in any case.",
            "type": "string",
            "pattern": quality_pattern()
        })
    }
}

/// What each 3MF object holds, as `objects` reads it (lower case only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshObjects {
    /// One object per body.
    Body,
    /// One object for each placed component with two or more bodies.
    Component,
}

impl<'de> Deserialize<'de> for MeshObjects {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match v.as_str() {
            Some("body") => Ok(MeshObjects::Body),
            Some("component") => Ok(MeshObjects::Component),
            _ => Err(D::Error::custom(whole(format!("`objects` must be body or component, not {v}")))),
        }
    }
}

impl JsonSchema for MeshObjects {
    fn schema_name() -> Cow<'static, str> {
        "MeshObjects".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "body (one object per body) or component (one object for each placed component with two or more bodies, with those bodies as its parts). Lower case only.",
            "type": "string",
            "enum": ["body", "component"]
        })
    }
}

/// What happened when the saved file was handed to `slicer`. Only the one that applies is there.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SlicerDelivery {
    /// The slicer's name: the file was opened there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slicer: Option<String>,
    /// Why the program would not open the file. The file is saved either way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slicer_error: Option<String>,
    /// The slicer's name: the slicer had to be started first and was not ready (never for
    /// `file.save_mesh`, which waits for it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starting: Option<String>,
    /// The slicer's name: it was started but was not ready within 10 seconds, so the file was
    /// not sent. The file is saved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slicer_timeout: Option<String>,
}

/// What `file.save_mesh` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SaveMeshResult {
    /// The file written, as given.
    pub path: String,
    /// Its size in bytes.
    pub bytes: usize,
    /// The format written: StlBinary, StlAscii, ThreeMf or ThreeMfTagged.
    pub format: String,
    /// With `slicer` or `lay_on_print_faces` only: the bodies that were saved laid on their print face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub print_faces: Vec<String>,
    /// With `slicer` or `lay_on_print_faces` only: a warning for each body that has a print face but
    /// was saved as it lies, and for each face the mesh path left short of its smallest angle or
    /// chord tolerance (led by its body's name).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// With `slicer` only: what happened when the file was handed over.
    #[serde(flatten)]
    pub delivery: SlicerDelivery,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "save_mesh_tests.rs"]
mod tests;
