//! Contracts and result types for the commands in `cmd/file.rs`: so far `file.export` and the
//! export command of each format (`file.export_step`, `file.export_stl`, `file.export_3mf`,
//! `file.export_obj`, `file.export_urdf`), which take what `file.export` takes for that format,
//! without `format`.

use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use solidcraft_io::Format;

use super::check::whole;
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

/// Export bodies, or the whole design, to a file. `format` names the format; without it the
/// path's extension does. STL, OBJ and 3MF take `quality` (and 3MF `objects`); URDF takes
/// `mesh_prefix`, `effort` and `velocity` and exports the whole assembly; design JSON the
/// complete document.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = export_required)]
pub struct ExportParams {
    // `path` is optional in the type and required in the schema: left out, the handler refuses it
    // with its own message.
    /// The file to write.
    #[serde(default)]
    pub path: Option<String>,
    /// The format: stl, stla, obj, step, 3mf, 3mf-tagged, urdf or json, or a file name whose
    /// extension names it. Default: from the path's extension.
    #[serde(default)]
    pub format: Option<String>,
    /// The bodies to export, by name. Default: every body. Design JSON and URDF export the
    /// complete design, so they refuse a list.
    #[serde(default)]
    pub bodies: Option<StringList>,
    /// How closely the mesh follows curved faces (STL, OBJ and 3MF). Default: standard. Not read
    /// for URDF.
    #[serde(default)]
    pub quality: Option<MeshQualityParam>,
    /// What each object in a 3MF file holds. Default: body; `component` is refused for the other
    /// formats. Not read for URDF.
    #[serde(default)]
    pub objects: Option<ObjectsParam>,
    /// URDF only: put before each mesh file name, such as `package://robot/meshes/`. Default: none.
    #[serde(default)]
    pub mesh_prefix: Option<String>,
    /// URDF only: each joint's effort limit, a positive number. Default: 1000.
    #[serde(default)]
    pub effort: Option<f64>,
    /// URDF only: each joint's velocity limit, a positive number. Default: 10.
    #[serde(default)]
    pub velocity: Option<f64>,
}

fn export_required(schema: &mut Schema) {
    super::motion::require(schema, "path");
}

/// The handler's own message for a key, whole.
fn refuse<E: serde::de::Error>(message: impl std::fmt::Display) -> E {
    E::custom(whole(message))
}

/// A key the handler reads as text (`str_`): any other value but `null` is refused, where the
/// handler would read it as left out.
fn text<E: serde::de::Error>(m: &Map<String, Value>, key: &str) -> Result<Option<String>, E> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(E::custom(format!("`{key}` must be text"))),
    }
}

/// `bodies` as the handler reads it (`params::string_list`), with its messages: `null` is left out.
fn names<E: serde::de::Error>(m: &Map<String, Value>) -> Result<Option<StringList>, E> {
    match m.get("bodies") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let p = Value::Object(Map::from_iter([("bodies".to_string(), v.clone())]));
            match crate::params::string_list("file.export", &p, "bodies") {
                Ok(list) => Ok(Some(StringList(list))),
                Err(crate::EngineError::BadParams { msg, .. }) => Err(refuse(msg)),
                Err(e) => Err(refuse(e)),
            }
        }
    }
}

impl<'de> Deserialize<'de> for ExportParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = Map::<String, Value>::deserialize(d)?;
        let mut out =
            ExportParams { path: None, format: None, bodies: None, quality: None, objects: None, mesh_prefix: None, effort: None, velocity: None };
        // The handler reads `path` first and refuses anything but a name it can write to; the
        // other keys are read only once it has one, and the format is known.
        out.path = match m.get("path") {
            None => return Ok(out),
            Some(Value::String(s)) if !s.trim().is_empty() && s.len() < 4096 => Some(s.clone()),
            Some(_) => return Err(refuse::<D::Error>("`path` is required")),
        };
        out.format = text::<D::Error>(&m, "format")?;
        let Some(format) = out.format.as_deref().or(out.path.as_deref()).and_then(|n| Format::from_name(n).ok()) else {
            // The handler refuses a format it does not know before it reads anything else.
            return Ok(out);
        };
        out.bodies = names::<D::Error>(&m)?;
        if format == Format::Urdf {
            // URDF reads its own keys; `quality` and `objects` are not read.
            out.mesh_prefix = text::<D::Error>(&m, "mesh_prefix")?;
            let positive = |key: &str| -> Result<Option<f64>, D::Error> {
                match m.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(v) => v
                        .as_f64()
                        .filter(|x| x.is_finite() && x.abs() < 1e9 && *x > 0.0)
                        .map(Some)
                        .ok_or_else(|| refuse(format!("`{key}` must be a positive number"))),
                }
            };
            out.effort = positive("effort")?;
            out.velocity = positive("velocity")?;
            return Ok(out);
        }
        // `mesh_prefix`, `effort` and `velocity` are not read for the other formats.
        out.quality = match m.get("quality") {
            None | Some(Value::Null) => None,
            Some(v) => Some(match v.as_str().and_then(solidcraft_io::MeshQuality::from_name) {
                Some(solidcraft_io::MeshQuality::Standard) => MeshQualityParam::Standard,
                Some(solidcraft_io::MeshQuality::Fine) => MeshQualityParam::Fine,
                Some(solidcraft_io::MeshQuality::Finest) => MeshQualityParam::Finest,
                None => return Err(refuse(format!("`quality` must be standard, fine or finest, not {v}"))),
            }),
        };
        out.objects = match m.get("objects") {
            None | Some(Value::Null) => None,
            Some(v) => match v.as_str() {
                Some("body") => Some(ObjectsParam::Body),
                Some("component") if matches!(format, Format::ThreeMf | Format::ThreeMfTagged) => Some(ObjectsParam::Component),
                Some("component") => return Err(refuse("`objects: component` is for 3MF: other formats have no parts")),
                _ => return Err(refuse(format!("`objects` must be body or component, not {v}"))),
            },
        };
        Ok(out)
    }
}

/// What `file.export_step`, `file.export_stl`, `file.export_3mf` and `file.export_obj` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportResult {
    /// The file written.
    pub path: String,
    /// Its size in bytes.
    pub bytes: usize,
    /// The format written: Step, StlBinary, ThreeMf or Obj (and for `file.export` also StlAscii,
    /// ThreeMfTagged or Design).
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

/// What `file.export` returns: what the export command of the format returns.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FileExportResult {
    /// STL, OBJ, STEP, 3MF or design JSON.
    File(ExportResult),
    /// URDF.
    Urdf(UrdfExportResult),
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
