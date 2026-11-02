//! Contracts and result types for the commands in `cmd/file.rs`: so far the export command of
//! each format (`file.export_step`, `file.export_stl`, `file.export_3mf`, `file.export_obj`,
//! `file.export_urdf`), which take what `file.export` takes for that format, without `format`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::StringList;

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

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
