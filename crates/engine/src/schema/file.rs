//! Contracts and result types for the commands in `cmd/file.rs`: so far the export command of
//! each format (`file.export_step`, `file.export_stl`, `file.export_3mf`, `file.export_obj`,
//! `file.export_urdf`), which take what `file.export` takes for that format, without `format`,
//! and `doc.open` and the recovery commands (`doc.recovery_list`, `doc.recover`,
//! `doc.recovery_discard`).

use std::borrow::Cow;
use std::path::PathBuf;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::point_cloud::{MAX_POINTS, UNITS};

use super::check::whole;
use super::types::{StringList, not_blank_pattern};

/// How closely an exported mesh follows curved faces.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MeshQualityParam {
    /// A chord tolerance of 2e-4 of each body's size.
    Standard,
    /// 5e-5 of each body's size.
    Fine,
    /// 1e-5 of each body's size.
    Finest,
}

/// What each 3MF object holds.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ObjectsParam {
    /// One object per body.
    Body,
    /// One object for each placed component with two or more bodies, with those bodies as its
    /// parts, so a slicer loads them as one object with parts; other bodies stay objects of their own.
    Component,
}

/// Export bodies as a STEP file. With no `bodies`, a design with components exports as a STEP
/// assembly, otherwise its bodies as placed. The file is STEP whatever the path's extension.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExportStepParams {
    /// The file to write.
    pub path: String,
    /// The bodies to export, by name. Default: every body.
    #[serde(default)]
    pub bodies: Option<StringList>,
}

/// Export bodies as a mesh file: binary STL for `file.export_stl`, OBJ for `file.export_obj`
/// (the colours of the bodies go in a material library, `.mtl`, written next to it). The file
/// has that format whatever the path's extension.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExportMeshParams {
    /// The file to write.
    pub path: String,
    /// The bodies to export, by name. Default: every body.
    #[serde(default)]
    pub bodies: Option<StringList>,
    /// How closely the mesh follows curved faces. Default: standard.
    #[serde(default)]
    pub quality: Option<MeshQualityParam>,
}

/// Export bodies as a 3MF file, whatever the path's extension.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Export3mfParams {
    /// The file to write.
    pub path: String,
    /// The bodies to export, by name. Default: every body.
    #[serde(default)]
    pub bodies: Option<StringList>,
    /// How closely the mesh follows curved faces. Default: standard.
    #[serde(default)]
    pub quality: Option<MeshQualityParam>,
    /// What each object in the file holds. Default: body.
    #[serde(default)]
    pub objects: Option<ObjectsParam>,
}

/// Export the whole assembly as a robot description (URDF), with an STL mesh for each link
/// written next to it (docs/urdf.md).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExportUrdfParams {
    /// The `.urdf` file to write; the meshes go in its folder.
    pub path: String,
    /// Put before each mesh file name in the URDF, such as `package://robot/meshes/`. Default: none.
    #[serde(default)]
    pub mesh_prefix: Option<String>,
    /// Each joint's effort limit, a positive number. Default: 1000.
    #[serde(default)]
    pub effort: Option<f64>,
    /// Each joint's velocity limit, a positive number. Default: 10.
    #[serde(default)]
    pub velocity: Option<f64>,
}

/// What `file.export_step`, `file.export_stl`, `file.export_3mf` and `file.export_obj` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportResult {
    /// The file written.
    pub path: String,
    /// Its size in bytes.
    pub bytes: usize,
    /// The format written: Step, StlBinary, ThreeMf or Obj.
    pub format: String,
    /// OBJ only: every file written, the material library first and the mesh file last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `file.export_urdf` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UrdfExportResult {
    /// The `.urdf` file written.
    pub path: String,
    /// Always Urdf.
    pub format: String,
    /// Every file written, the meshes first and the `.urdf` last.
    pub files: Vec<String>,
    /// How many links the robot has.
    pub links: usize,
    /// How many joints the robot has.
    pub joints: usize,
    /// What the export could not carry over as it is in the design.
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A file path as the open command reads it: text that is not blank and shorter than 4096 bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenPath(pub String);

impl<'de> Deserialize<'de> for OpenPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if crate::cmd::usable_path(&s) => Ok(OpenPath(s)),
            _ => Err(D::Error::custom(whole("`path` is required"))),
        }
    }
}

impl JsonSchema for OpenPath {
    fn schema_name() -> Cow<'static, str> {
        "OpenPath".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A file path: text with a character that is not white space, under 4096 bytes long (the schema's maxLength counts characters).",
            "type": "string",
            "minLength": 1,
            "maxLength": 4095,
            "pattern": not_blank_pattern()
        })
    }
}

/// Which way is up in a STEP, IGES, FreeCAD, Rhino or glTF file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UpParam(pub bool);

impl<'de> Deserialize<'de> for UpParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::up_form(&Value::deserialize(d)?).map(UpParam).ok_or_else(|| D::Error::custom(whole(crate::cmd::UP_MESSAGE)))
    }
}

impl JsonSchema for UpParam {
    fn schema_name() -> Cow<'static, str> {
        "UpParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "z keeps the file's axes (the default); y says the file has Y up, so what it brings is turned a quarter turn about X, its Y becoming Z. Matching ignores case and surrounding spaces.",
            "type": "string",
            "enum": ["z", "y"]
        })
    }
}

/// What the bodies of a STEP, IGES, FreeCAD or Rhino file are named after.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NamesParam(pub solidcraft_io::StepNames);

impl<'de> Deserialize<'de> for NamesParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::names_form(&Value::deserialize(d)?).map(NamesParam).ok_or_else(|| D::Error::custom(whole(crate::cmd::NAMES_MESSAGE)))
    }
}

impl JsonSchema for NamesParam {
    fn schema_name() -> Cow<'static, str> {
        "NamesParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "auto (the default) keeps meaningful solid names and replaces placeholders with the nearest named product, else the instance's; solid is the solid's own name; product the part's name; instance the occurrence's name, else the product's. Matching ignores case and surrounding spaces.",
            "type": "string",
            "enum": ["auto", "solid", "product", "instance"]
        })
    }
}

/// The length unit of a point cloud file.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudUnits(pub String);

impl<'de> Deserialize<'de> for CloudUnits {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let words = UNITS.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ");
        match Value::deserialize(d)? {
            Value::String(u) if UNITS.iter().any(|(name, _)| name.eq_ignore_ascii_case(&u)) => Ok(CloudUnits(u)),
            Value::String(u) => Err(D::Error::custom(format!("must be one of {words} (not `{u}`)"))),
            _ => Err(D::Error::custom(format!("must be one of {words}"))),
        }
    }
}

impl JsonSchema for CloudUnits {
    fn schema_name() -> Cow<'static, str> {
        "CloudUnits".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let words: Vec<&str> = UNITS.iter().map(|(name, _)| *name).collect();
        json_schema!({
            "description": "The file's length unit. Matching ignores case.",
            "type": "string",
            "enum": words
        })
    }
}

/// The most points of a point cloud file kept: a whole number from 1 to the limit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaxPoints(pub u64);

impl<'de> Deserialize<'de> for MaxPoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)?.as_u64() {
            Some(n) if (1..=MAX_POINTS as u64).contains(&n) => Ok(MaxPoints(n)),
            _ => Err(D::Error::custom(format!("must be a whole number from 1 to {MAX_POINTS}"))),
        }
    }
}

impl JsonSchema for MaxPoints {
    fn schema_name() -> Cow<'static, str> {
        "MaxPoints".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Most points kept; a larger file is thinned evenly.",
            "type": "integer",
            "minimum": 1,
            "maximum": MAX_POINTS
        })
    }
}

/// Open a file as the design: a SolidCraft design (`.sc`, or a `.solidcraft` design from an
/// earlier release), a `.dxf` or `.svg` drawing (a new, unsaved design with an XY sketch, with
/// the units and defaults of `sketch.insert_dxf` and `sketch.insert_svg`), a STEP, IGES,
/// FreeCAD, Rhino, 3MF, STL, OBJ or glTF file (a new design holding the file as a component
/// named after it, as `file.insert_step` and `file.insert_mesh` make), or a `.ply`, `.xyz` or
/// `.pts` point cloud (a new design holding it, as `pointcloud.insert` makes). The open design
/// is replaced.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct OpenParams {
    /// The file to open; the kind of file is read from its extension.
    pub path: OpenPath,
    /// For a STEP, IGES, FreeCAD, Rhino or glTF file only (refused for any other): which way is up
    /// in the file. Default: z for STEP, IGES, FreeCAD and Rhino, y for glTF.
    #[serde(default)]
    pub up: Option<UpParam>,
    /// For a STEP, IGES, FreeCAD or Rhino file only (refused for any other): what the bodies are
    /// named after. Default: auto.
    #[serde(default)]
    pub names: Option<NamesParam>,
    /// For a STEP, IGES, FreeCAD or Rhino file (ignored for other files): true puts an import that
    /// brings more than one item into the history (its Import feature and the rigid joints holding
    /// its parts) into a new history folder named `Group #N`. Default: false.
    #[serde(default)]
    pub folder: Option<bool>,
    /// For a design: saving to its own file is refused (`file.save_as` keeps the changes
    /// elsewhere). Default: false.
    #[serde(default)]
    pub read_only: Option<bool>,
    /// For a point cloud, STL, OBJ or PLY file: the unit its coordinates are in, scaled to
    /// millimetres as it is read (refused for a 3MF or glTF file, which states its own; ignored
    /// for other files). Default: mm.
    #[serde(default)]
    pub units: Option<CloudUnits>,
    /// For a point cloud file only (ignored for other files): the most points kept. Default:
    /// 5000000.
    #[serde(default)]
    pub max_points: Option<MaxPoints>,
}

/// Another SolidCraft that has the design open, from its lock file.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenElsewhere {
    /// The computer's name.
    pub host: String,
    /// The user's name.
    pub user: String,
    /// The SolidCraft version that holds the lock.
    pub app: String,
    /// When it opened the design (Unix seconds).
    pub opened: u64,
}

/// What `doc.open` returns for a design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenedDesign {
    /// The file opened.
    pub path: String,
    /// How many features the design has.
    pub features: usize,
    /// How many of them failed to evaluate.
    pub errors: usize,
    /// The design's name.
    pub name: String,
    /// Only with `read_only`: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    /// Only when another SolidCraft has the design open (its lock file is fresh).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_elsewhere: Option<OpenElsewhere>,
    /// Only when a newer SolidCraft saved the design: it opened all the same, and what this
    /// version can't read is named here and kept by saving.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newer_file: Option<NewerFileReport>,
    /// Only with `newer_file`: its notice, then a line for each part this version didn't read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `doc.open` says about a design a newer SolidCraft saved.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NewerFileReport {
    /// The design file format it was saved in.
    pub format: u32,
    /// The newest format this version reads.
    pub reads_up_to: u32,
    /// The format changes it names that this version doesn't know.
    pub unknown_changes: Vec<String>,
    /// What this version couldn't read, by name; saving keeps it.
    pub not_understood: Vec<String>,
    /// What to tell the user.
    pub notice: String,
}

/// What `doc.open` returns for a `.dxf` or `.svg` drawing: what `sketch.insert_dxf` or
/// `sketch.insert_svg` returned for it, and the new design's.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenedDrawing {
    /// The sketch that holds the drawing.
    pub sketch: u64,
    /// The sketch made for it; null when one was already being edited.
    pub created_sketch: Option<u64>,
    /// How many curves were added.
    pub curves: usize,
    /// How many texts were added.
    pub texts: usize,
    /// The sketch's solve report, `{solved, dof}`, or null when it has none.
    pub info: Value,
    /// The file opened.
    pub path: String,
    /// The design's name.
    pub name: String,
    /// How many features the design has.
    pub features: usize,
    /// How many of them failed to evaluate.
    pub errors: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `doc.open` returns for a point cloud file.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenedPointCloud {
    /// The file opened.
    pub path: String,
    /// Always 0: a point cloud is not a feature.
    pub features: usize,
    /// Always 0.
    pub errors: usize,
    /// The cloud's id.
    pub cloud: u64,
    /// How many points were kept.
    pub points: usize,
    /// How many points the file has.
    pub source_points: u64,
    /// The box round the cloud in the design, `[[x, y, z], [x, y, z]]` in mm; null for no points.
    pub bounds: Option<[[f64; 3]; 2]>,
    /// What the reading could not carry over.
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `doc.open` returns for a STEP, IGES, FreeCAD, Rhino, 3MF, STL, OBJ or glTF file.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenedFile {
    /// The file opened.
    pub path: String,
    /// How many features the design has.
    pub features: usize,
    /// How many of them failed to evaluate.
    pub errors: usize,
    /// The bodies the file made, by name.
    pub bodies: Vec<String>,
    /// What the reading could not carry over.
    pub warnings: Vec<String>,
    /// The component made for the file (its id); it is the one being worked in.
    pub component: u64,
    /// The component's name.
    pub component_name: String,
    /// The component's occurrence in the design; null when it has none.
    pub occurrence: Option<u64>,
    /// How many components below it an assembly's parts and sub-assemblies made; only when there
    /// are some.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcomponents: Option<usize>,
    /// How many rigid joints hold those together; only when there are subcomponents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rigid_joints: Option<usize>,
    /// Only when `up` turned the file Y up to Z up: the occurrence that was turned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stand_up: Option<u64>,
    /// STEP and IGES only: the system that wrote the file, as its header says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// STEP and IGES only, when the program that wrote the file is known to have a fixed up
    /// direction: its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exporter: Option<String>,
    /// With `exporter`: the direction that program's files have up, `z` or `y`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exporter_up: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `doc.open` returns: the shape depends on the kind of file opened.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum OpenResult {
    /// A SolidCraft design.
    Design(OpenedDesign),
    /// A `.dxf` or `.svg` drawing.
    Drawing(OpenedDrawing),
    /// A point cloud.
    PointCloud(OpenedPointCloud),
    /// A STEP, IGES, FreeCAD, Rhino or mesh file.
    File(OpenedFile),
}

/// List the designs that apps which crashed, or closed with unsaved changes, autosaved. Nothing is
/// changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RecoveryListParams {
    /// The recovery folder. Default: the app's own; a blank value, or one of 4096 bytes or more, counts as no value.
    #[serde(default)]
    pub dir: Option<String>,
}

/// One autosaved design that can be recovered.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RecoveredDesign {
    /// The entry's id, for `doc.recover` and `doc.recovery_discard`.
    pub id: String,
    /// The design's file, or null when it never had one.
    pub path: Option<String>,
    /// The design's name.
    pub name: String,
    /// When it was autosaved (Unix seconds).
    pub saved_at: u64,
    /// How many features the design had.
    pub features: usize,
}

impl From<crate::recovery::Entry> for RecoveredDesign {
    fn from(e: crate::recovery::Entry) -> Self {
        RecoveredDesign { id: e.id, path: e.path, name: e.name, saved_at: e.saved_at, features: e.features }
    }
}

/// What `doc.recovery_list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RecoveryListResult {
    /// The recovery folder that was read.
    #[schemars(with = "String")]
    pub dir: PathBuf,
    /// The designs, the newest first (at most 1000).
    pub designs: Vec<RecoveredDesign>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Open an autosaved design, replacing the open design, with its file path and as unsaved.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RecoverParams {
    /// The entry to open: an `id` from `doc.recovery_list`.
    #[serde(deserialize_with = "recovery_id")]
    pub id: String,
    /// The recovery folder. Default: the app's own; a blank value, or one of 4096 bytes or more, counts as no value.
    #[serde(default)]
    pub dir: Option<String>,
    /// Delete the entry once the design is open. Default: false.
    #[serde(default)]
    pub discard: Option<bool>,
}

/// `id` of `doc.recover`: text, which the handler gives its own message for when it is not.
fn recovery_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`id` is required (see doc.recovery_list)"))),
    }
}

/// What `doc.recover` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RecoverResult {
    /// The entry opened.
    pub id: String,
    /// The design's file, or null when it never had one.
    pub path: Option<String>,
    /// The name the entry was saved under.
    pub name: String,
    /// How many features the design has.
    pub features: usize,
    /// How many of them failed to evaluate.
    pub errors: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Delete one autosaved design, or all of them.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RecoveryDiscardParams {
    /// The entry to delete: an `id` from `doc.recovery_list`. Needed unless `all` is true, and
    /// ignored then.
    #[serde(default)]
    pub id: Option<String>,
    /// Delete every entry left by an app that is no longer running. Default: false.
    #[serde(default)]
    pub all: Option<bool>,
    /// The recovery folder. Default: the app's own; a blank value, or one of 4096 bytes or more, counts as no value.
    #[serde(default)]
    pub dir: Option<String>,
}

/// What `doc.recovery_discard` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RecoveryDiscardResult {
    /// The ids deleted.
    pub discarded: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
