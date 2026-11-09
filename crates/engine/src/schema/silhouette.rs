//! Contracts and result types for `inspect.silhouette` (`cmd/silhouette.rs`) and
//! `file.export_view_dxf` (`cmd/view_dxf.rs`) (#12611).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a body exists, or anything is visible,
//! stays the handler's to check. A key the handler refuses when it is left out (the DXF
//! export's `camera`) is optional in the type and required in the schema, so that the handler's
//! message, not the check's "is required", is what a caller sees.
//!
//! `inspect.silhouette` reads `view` or `camera`, and refuses the two together before it looks
//! at either, so its `Deserialize` is written out to do the same.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcraft_doc::NamedView;

use super::check::{located, whole};
use super::types::{CameraParam, not_blank_pattern};
use super::view_navigation::{read_as, read_with};
use solidcraft_viewport::{Camera, StandardView};

/// The most body names `inspect.silhouette` reads from a list, as the handler does.
const MAX_BODIES: usize = 10_000;

fn require(schema: &mut Schema, key: &str) {
    super::motion::require(schema, key);
}

fn camera_required(schema: &mut Schema) {
    require(schema, "camera");
}

// ---- inspect.silhouette ----

/// A standard view's name, as `inspect.silhouette` reads `view`: text that names one of the
/// standard views, with the handler's messages for anything else.
fn view_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(name) if StandardView::parse(&name).is_some() => Ok(Some(name)),
        Value::String(name) => Err(D::Error::custom(whole(format!("unknown standard view `{name}`")))),
        _ => Err(D::Error::custom(whole("`view` must be a standard view name"))),
    }
}

fn view_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A standard view: front, back, left, right, top, bottom or iso (also home or isometric); upper or lower case. Default: front.",
        "type": "string"
    })
}

/// A camera as `inspect.silhouette` reads `camera`: an orbit camera `{target, yaw, pitch,
/// distance, fov?}` in mm and radians, finite and at a positive distance. Other keys (a full
/// camera's `roll` and `up`) are not read.
#[derive(Clone, Debug, PartialEq)]
pub struct SilhouetteCamera(pub NamedView);

impl<'de> Deserialize<'de> for SilhouetteCamera {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let named: NamedView = serde_json::from_value(Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(format!("bad camera: {e}"))))?;
        let camera =
            Camera { target: named.target, yaw: named.yaw, pitch: named.pitch, distance: named.distance, fov: named.fov, ..Default::default() };
        if !camera.is_valid() || !camera.fov.is_finite() || camera.fov < 0.0 {
            return Err(D::Error::custom(whole("the camera must be finite with a positive distance")));
        }
        Ok(SilhouetteCamera(named))
    }
}

impl JsonSchema for SilhouetteCamera {
    fn schema_name() -> Cow<'static, str> {
        "SilhouetteCamera".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An orbit camera, in mm and radians. Other keys are ignored.",
            "type": "object",
            "properties": {
                "target": {"description": "The point the camera looks at, in mm: [x, y, z].", "type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                "yaw": {"description": "Turn about the vertical axis; 0 looks along +Y, from the front.", "type": "number"},
                "pitch": {"description": "Elevation above the ground plane.", "type": "number"},
                "distance": {"description": "From the target to the eye, in mm.", "type": "number", "exclusiveMinimum": 0},
                "fov": {"description": "Vertical field of view; 0 is parallel. Default: 0.", "type": "number", "minimum": 0, "default": 0.0}
            },
            "required": ["target", "yaw", "pitch", "distance"]
        })
    }
}

fn silhouette_camera<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SilhouetteCamera>, D::Error> {
    SilhouetteCamera::deserialize(d).map(Some)
}

/// The bodies `inspect.silhouette` outlines: one name, or a list of up to 10000.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyNames(pub Vec<String>);

impl<'de> Deserialize<'de> for BodyNames {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(name) => Ok(BodyNames(vec![name])),
            Value::Array(names) if names.len() <= MAX_BODIES => names
                .into_iter()
                .map(|v| match v {
                    Value::String(name) => Ok(name),
                    _ => Err(D::Error::custom(whole("`bodies` must contain names"))),
                })
                .collect::<Result<_, _>>()
                .map(BodyNames),
            _ => Err(D::Error::custom(whole("`bodies` must be a list of names"))),
        }
    }
}

impl JsonSchema for BodyNames {
    fn schema_name() -> Cow<'static, str> {
        "BodyNames".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "One body name, or a list of up to 10000.",
            "anyOf": [
                {"type": "string"},
                {"type": "array", "items": {"type": "string"}, "maxItems": MAX_BODIES}
            ]
        })
    }
}

fn body_names<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BodyNames>, D::Error> {
    BodyNames::deserialize(d).map(Some)
}

/// `rows` as the handler reads it: any integer (it is clamped, not refused), one above `i64`'s
/// range read as the largest.
fn rows<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    read_with(d, "`rows` must be an integer", |v| v.as_i64().or_else(|| v.as_u64().map(|n| n.min(i64::MAX as u64) as i64)))
}

fn rows_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The mask's height in pixels, a whole number written without a decimal point (240, not 240.0); clamped to 16..1024. Default: 240.",
        "type": "integer",
        "default": 240
    })
}

/// The keys `inspect.silhouette` reads, once `view` and `camera` are known not to come together.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SilhouetteKeys {
    /// A standard view to look from. Not with `camera`. Default: front.
    #[serde(default, deserialize_with = "view_name", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "view_schema")]
    pub view: Option<String>,
    /// A camera to look from, in mm and radians; `fov` 0 means parallel. Not with `view`.
    #[serde(default, deserialize_with = "silhouette_camera", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SilhouetteCamera")]
    pub camera: Option<SilhouetteCamera>,
    /// The world bodies to outline, by name; a named hidden body is included. Default: the
    /// visible bodies.
    #[serde(default, deserialize_with = "body_names", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BodyNames")]
    pub bodies: Option<BodyNames>,
    /// The mask's height in pixels, clamped to 16..1024. Default: 240.
    #[serde(default, deserialize_with = "rows", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "rows_schema")]
    pub rows: Option<i64>,
}

/// A tightly framed, view-aligned union of the projected body meshes, as a run-length mask the
/// size of the model in the view: for an agent comparing the model with a picture. It changes
/// nothing in the design.
///
/// `inspect.silhouette`'s parameters: [`SilhouetteKeys`], with `view` and `camera` together
/// refused first, as the handler does.
#[derive(Debug)]
pub struct SilhouetteParams(pub SilhouetteKeys);

impl<'de> Deserialize<'de> for SilhouetteParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        if value.get("view").is_some() && value.get("camera").is_some() {
            return Err(D::Error::custom(whole("give `view` or `camera`, not both")));
        }
        serde_path_to_error::deserialize(&value).map(SilhouetteParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for SilhouetteParams {
    fn schema_name() -> Cow<'static, str> {
        "SilhouetteParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = SilhouetteKeys::json_schema(generator);
        schema.insert(
            "description".into(),
            "A tightly framed, view-aligned union of the projected body meshes, as a run-length mask. Changes nothing in the design.".into(),
        );
        schema.insert("not".into(), json!({"required": ["view", "camera"]}));
        schema
    }
}

/// How the silhouette is projected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SilhouetteProjection {
    Perspective,
    Parallel,
}

/// What `inspect.silhouette` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SilhouetteResult {
    /// The mask's width in pixels.
    pub width: usize,
    /// The mask's height in pixels.
    pub height: usize,
    /// The size of a pixel in the view plane, in mm.
    pub mm_per_px: f64,
    /// The view-plane box the mask frames, in mm: `[[u_min, v_min], [u_max, v_max]]`, u along
    /// `right` and v along `up`.
    pub box_mm: [[f64; 2]; 2],
    /// The world direction of the mask's x axis.
    pub right: [f64; 3],
    /// The world direction of the mask's rows going up.
    pub up: [f64; 3],
    pub projection: SilhouetteProjection,
    /// How many bodies are in the outline.
    pub bodies: usize,
    /// How many triangles they have.
    pub triangles: usize,
    /// One entry per pixel row, top first: the filled runs of that row as `[start, end)` pixel
    /// columns.
    pub mask: Vec<Vec<[usize; 2]>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---- file.export_view_dxf ----

/// A camera as `file.export_view_dxf` reads `camera`: as the view commands read one, and also
/// with yaw and roll within ±1e6 and pitch within ±π/2 (else "invalid camera").
fn dxf_camera<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CameraParam>, D::Error> {
    read_with(d, "invalid camera", |v| {
        crate::cmd::camera_form(v)
            .filter(|c| c.yaw.abs() <= 1e6 && c.roll.abs() <= 1e6 && c.pitch.abs() <= std::f64::consts::FRAC_PI_2)
            .map(CameraParam)
    })
}

/// Whether hidden lines are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HiddenLines {
    /// Leave them out.
    Omit,
    /// Draw them dashed on the HIDDEN layer.
    Dashed,
}

fn hidden_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "`omit` leaves hidden lines out; `dashed` draws them dashed on the HIDDEN layer. Default: omit.",
        "enum": ["omit", "dashed"],
        "default": "omit"
    })
}

fn hidden<'de, D: Deserializer<'de>>(d: D) -> Result<Option<HiddenLines>, D::Error> {
    read_as(d, "hidden must be omit or dashed")
}

fn tolerance<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    read_with(d, "tolerance must be 0.0001..1 mm", |v| v.as_f64().filter(|t| (0.0001..=1.0).contains(t)))
}

fn tolerance_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The chord tolerance for curves, in mm: 0.0001 to 1. Default: 0.01.",
        "type": "number",
        "minimum": 0.0001,
        "maximum": 1.0,
        "default": 0.01
    })
}

/// The `.dxf` file's path, as the handler reads `path`: text with something in it, under 4096
/// bytes; `null` is refused.
fn export_path<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    read_with(d, "invalid path", |v| v.as_str().filter(|p| !p.trim().is_empty() && p.len() < 4096).map(str::to_string))
}

fn path_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The `.dxf` file to write: not blank, under 4096 bytes (maxLength counts characters). Default: the reply carries the DXF text instead.",
        "type": "string",
        "pattern": not_blank_pattern(),
        "maxLength": 4095
    })
}

/// Draw the visible placed bodies and the shown sketches' profile curves from a camera as a
/// 2D DXF drawing in mm: LINE and LWPOLYLINE entities on the VISIBLE and HIDDEN layers, sketch
/// curves on SKETCH. No construction geometry, points, dimensions or constraints; finished
/// sketches are hidden behind bodies and the active sketch is drawn on top; a perspective
/// camera is projected onto the target plane; there is no viewport crop and no overlay. Writes
/// the file when `path` is given, else returns the text.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = camera_required)]
pub struct ExportViewDxfParams {
    // Optional in the type and required in the schema (`camera_required`).
    /// The camera to draw from, as `ui.inspect` gives it. Its yaw and roll must also be within
    /// ±1e6 and its pitch within ±π/2.
    #[serde(default, deserialize_with = "dxf_camera", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CameraParam")]
    pub camera: Option<CameraParam>,
    /// Whether to draw the hidden lines (dashed) or leave them out. Default: omit.
    #[serde(default, deserialize_with = "hidden", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "hidden_schema")]
    pub hidden: Option<HiddenLines>,
    /// The chord tolerance for curves, in mm: 0.0001 to 1. Default: 0.01.
    #[serde(default, deserialize_with = "tolerance", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "tolerance_schema")]
    pub tolerance: Option<f64>,
    /// The `.dxf` file to write. Default: the reply carries the DXF text instead.
    #[serde(default, deserialize_with = "export_path", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "path_schema")]
    pub path: Option<String>,
}

/// How the drawing is projected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DxfProjection {
    Perspective,
    Orthographic,
}

/// The drawing's unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub enum DxfUnits {
    #[serde(rename = "mm")]
    Millimetres,
}

/// What the export measured while it ran.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ViewDxfStats {
    /// The placed bodies drawn.
    pub bodies: usize,
    /// The distinct meshes among them (placements that share geometry share a mesh).
    pub shared_meshes: usize,
    /// The triangles of those meshes.
    pub triangles: usize,
    /// The memory the meshes take, in bytes.
    pub mesh_bytes: usize,
    /// The most worker threads the meshing used.
    pub worker_limit: usize,
    /// The projected edge segments.
    pub segments: usize,
    /// The work the hidden-line test took.
    pub occlusion_work: usize,
    /// The time to mesh the bodies, in ms.
    pub prepare_ms: f64,
    /// The time to project them, in ms.
    pub projection_ms: f64,
    /// The time for the hidden-line test, in ms.
    pub occlusion_ms: f64,
}

/// Where the drawing went.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DxfOutput {
    /// With `path`: the file written.
    File {
        /// The path the drawing was written to, as given.
        path: String,
    },
    /// Without `path`: the DXF text.
    Text {
        /// The drawing, as DXF text.
        dxf: String,
    },
}

/// What `file.export_view_dxf` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ViewDxfResult {
    /// The lines found visible, before neighbouring ones are joined into polylines.
    pub visible: usize,
    /// The hidden lines drawn: always 0 unless `hidden` is dashed.
    pub hidden: usize,
    /// The entities drawn on the SKETCH layer.
    pub sketch: usize,
    pub units: DxfUnits,
    /// The chord tolerance used, in mm.
    pub tolerance: f64,
    pub projection: DxfProjection,
    pub stats: ViewDxfStats,
    #[serde(flatten)]
    pub output: DxfOutput,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "silhouette_tests.rs"]
mod tests;
