//! Contracts and result types for the commands in `cmd/sketch_canvas.rs`: `canvas.insert`,
//! `canvas.decal`, `canvas.edit`, `canvas.calibrate`, `canvas.straighten` and `canvas.delete`, and
//! for `canvas.to_sketch` in `cmd/canvas_sketch.rs`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether an image can be read, a canvas exists
//! or a face is there stays the handler's to check. A call with several problems may get a
//! different one first in strict than with the check off (the check goes key by key, the handlers
//! in their own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::check::whole;
use super::frame::SketchSolve;
use super::motion::require;
use super::types::{LengthExpr, Point2, Point3};
use crate::params;

/// Place an image on a plane as a reference to trace, kept in the design. Give the image as
/// `path` or as base64 `data`; only PNG and JPEG are taken, up to 16 MiB. The image goes into the
/// active component. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = image_required)]
pub struct CanvasInsertParams {
    /// A PNG or JPEG file to read (not blank, under 4096 characters). Used when `data` is not
    /// given.
    #[serde(default)]
    pub path: Option<String>,
    /// The image itself, a PNG or JPEG, as base64. Wins over `path`.
    #[serde(default)]
    pub data: Option<String>,
    /// The plane the image lies on. Default: XY. A picked face is read as `sketch.create` reads
    /// one.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CanvasPlane")]
    pub plane: Option<CanvasPlane>,
    /// Move the plane this far along its normal (mm or a length expression).
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    /// Where the image's centre is, on the plane, in mm. Default: `at`; else the picked point when
    /// `plane` is a picked face; else the plane's origin.
    #[serde(default)]
    pub center: Option<Point2>,
    /// A point [x, y, z] in mm, projected onto the plane, for the centre when `center` is not
    /// given. Default: the picked point when `plane` is a picked face, else the plane's origin.
    #[serde(default)]
    pub at: Option<Point3>,
    /// The image's width on the plane, in mm (above 0, below 1e7); its height follows the
    /// image. Default: 100.
    #[serde(default)]
    pub width: Option<CanvasWidth>,
    /// Its rotation on the plane, in degrees (a number of 1e9 or more is ignored). Default: 0.
    #[serde(default)]
    pub angle: Option<f64>,
    /// Mirror it left to right. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Its name. Default: "Canvas" and the first number no canvas's name has.
    #[serde(default)]
    pub name: Option<String>,
    /// Its opacity, 0 (clear) to 1 (opaque); a value outside is clamped (a number of 1e9 or more is ignored, as if left out). Default: 0.5.
    #[serde(default)]
    pub opacity: Option<f64>,
}

/// Put an image on a planar face of the model, like a sticker, kept in the design. Give the image
/// as `path` or as base64 `data`; only PNG and JPEG are taken, up to 16 MiB. The image goes into
/// the active component. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = decal_required)]
pub struct CanvasDecalParams {
    /// A PNG or JPEG file to read (not blank, under 4096 characters). Used when `data` is not
    /// given.
    #[serde(default)]
    pub path: Option<String>,
    /// The image itself, a PNG or JPEG, as base64. Wins over `path`.
    #[serde(default)]
    pub data: Option<String>,
    // Optional in the type and required in the schema (`decal_required`): left out, the handler
    // refuses it with its own message, not the check's "is required".
    /// The face: a point [x, y, z] in mm on a planar face, where the image is centred, or a
    /// picked face as `sketch.create`'s `plane` takes it: `{face: [x, y, z], body?, index?,
    /// path?, fixed?}` (`body` and `index` go together).
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "DecalFace")]
    pub face: Option<DecalFace>,
    /// Move the face's plane this far along its normal (mm or a length expression).
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    /// Where the image's centre is, on the face's plane, in mm. Default: `at`, or the picked
    /// point.
    #[serde(default)]
    pub center: Option<Point2>,
    /// A point [x, y, z] in mm, projected onto the plane, for the centre when `center` is not
    /// given. Default: the picked point.
    #[serde(default)]
    pub at: Option<Point3>,
    /// The image's width on the plane, in mm (above 0, below 1e7); its height follows the
    /// image. Default: 100.
    #[serde(default)]
    pub width: Option<CanvasWidth>,
    /// Its rotation on the plane, in degrees (a number of 1e9 or more is ignored). Default: 0.
    #[serde(default)]
    pub angle: Option<f64>,
    /// Mirror it left to right. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Its name. Default: "Decal" and the first number no canvas's name has.
    #[serde(default)]
    pub name: Option<String>,
    /// Its opacity, 0 (clear) to 1 (opaque); a value outside is clamped (a number of 1e9 or more is ignored, as if left out). Default: 1.
    #[serde(default)]
    pub opacity: Option<f64>,
}

/// Change a canvas or decal: its place, size, angle, opacity, mirroring, visibility or name. Keys
/// left out stay as they are. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = canvas_required)]
pub struct CanvasEditParams {
    /// The canvas, by id or name.
    // Optional in the type and required in the schema, as `face` above.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CanvasRef")]
    pub canvas: Option<CanvasRef>,
    /// The image's new centre on its plane, in mm.
    #[serde(default)]
    pub center: Option<Point2>,
    /// Its new width, in mm (above 0, below 1e7); its height follows the image.
    #[serde(default)]
    pub width: Option<CanvasWidth>,
    /// Its new rotation on the plane, in degrees (a number of 1e9 or more is ignored).
    #[serde(default)]
    pub angle: Option<f64>,
    /// Its new opacity, 0 (clear) to 1 (opaque); a value outside is clamped (a number of 1e9 or more is ignored, as if left out).
    #[serde(default)]
    pub opacity: Option<f64>,
    /// Mirror it left to right, or not.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Show it or hide it.
    #[serde(default)]
    pub visible: Option<bool>,
    /// Its new name; blank text keeps the name.
    #[serde(default)]
    pub name: Option<String>,
    /// Lock it or unlock it. A locked canvas refuses a new centre, width, angle or flip, Calibrate
    /// and Delete until it is unlocked; its opacity, visibility and name can still change.
    #[serde(default)]
    pub locked: Option<bool>,
}

/// Scale a canvas so that two points on it are a known distance apart: the image is scaled about
/// the first point. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = calibrate_required)]
pub struct CanvasCalibrateParams {
    /// The canvas, by id or name.
    // Optional in the types and required in the schema (`calibrate_required`), as `face` above.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CanvasRef")]
    pub canvas: Option<CanvasRef>,
    /// A point on the canvas's plane, in mm, [x, y]: the one the image is scaled about.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CalibrationPoint")]
    pub a: Option<CalibrationPoint>,
    /// Another point on the plane, in mm, [x, y], not the same as `a`.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CalibrationPoint")]
    pub b: Option<CalibrationPoint>,
    /// What the distance from `a` to `b` really is, in mm (above 0).
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Distance")]
    pub distance: Option<Distance>,
}

/// Turn a canvas so that a line drawn on its image, from `a` to `b`, gets the direction `angle`:
/// 0 makes it horizontal, 90 vertical. The image turns about `a`, the shorter way (a line has no
/// arrow, so the turn is at most 90 degrees either way and the image stays the right way up). Its
/// size does not change. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = straighten_required)]
pub struct CanvasStraightenParams {
    /// The canvas, by id or name.
    // Optional in the types and required in the schema, as `face` above.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CanvasRef")]
    pub canvas: Option<CanvasRef>,
    /// A point on the canvas's plane, in mm, [x, y], on the line: the one the image turns about.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CalibrationPoint")]
    pub a: Option<CalibrationPoint>,
    /// Another point on the line, in mm, [x, y], not the same as `a`.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CalibrationPoint")]
    pub b: Option<CalibrationPoint>,
    /// The direction the line should have on the plane, in degrees from the plane's x axis,
    /// counter-clockwise: 0 horizontal, 90 vertical. Default: 0.
    #[serde(default)]
    pub angle: Option<StraightenAngle>,
}

/// `canvas.straighten`'s `angle`: a finite number of degrees, below 1e9 in size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StraightenAngle(pub f64);

impl<'de> Deserialize<'de> for StraightenAngle {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let n = match Value::deserialize(d)? {
            Value::Number(n) => n.as_f64().and_then(params::finite),
            _ => None,
        };
        n.map(StraightenAngle).ok_or_else(|| D::Error::custom("must be a number of degrees"))
    }
}

impl JsonSchema for StraightenAngle {
    fn schema_name() -> Cow<'static, str> {
        "StraightenAngle".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A direction in degrees from the plane's x axis, counter-clockwise: 0 horizontal, 90 vertical (below 1 000 000 000 in size).",
            "type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9
        })
    }
}

/// Remove a canvas or decal from the design. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = canvas_required)]
pub struct CanvasDeleteParams {
    /// The canvas, by id or name.
    // Optional in the type and required in the schema, as `face` above.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CanvasRef")]
    pub canvas: Option<CanvasRef>,
}

/// Trace a canvas or decal image into a new sketch lying exactly on its plane: the drawing is
/// told from the background, its outlines become lines and cubic splines, and its holes stay
/// holes. The picture is read at up to 512 pixels on a side, at most 40 megapixels.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CanvasToSketchParams {
    /// The canvas, by id or name. Default: the last canvas in the design.
    // `null` is the same as leaving it out, as `pick` in `cmd/canvas_sketch.rs` reads it, so the
    // schema says so too.
    #[serde(default, deserialize_with = "absent_or", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "canvas_or_absent")]
    pub canvas: Option<CanvasRef>,
    /// The new sketch's name. Default: `<the canvas's name> sketch`.
    #[serde(default)]
    pub name: Option<String>,
}

/// A key that is left out, `null`, or parses as `T`: `null` is the same as leaving it out, as
/// `pick` in `cmd/canvas_sketch.rs` reads a canvas.
fn absent_or<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        given => T::deserialize(given).map(Some).map_err(D::Error::custom),
    }
}

/// A key that, once given (`null` included), must parse as `T`, as the handlers read it.
fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// A canvas by its id (a number) or its name, as `find` in `cmd/sketch_canvas.rs` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasRef(pub String);

impl<'de> Deserialize<'de> for CanvasRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => Ok(CanvasRef(n.to_string())),
            Value::String(s) => Ok(CanvasRef(s)),
            _ => Err(D::Error::custom("must be an id or a name")),
        }
    }
}

impl JsonSchema for CanvasRef {
    fn schema_name() -> Cow<'static, str> {
        "CanvasRef".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A canvas's id (a number) or its name.", "type": ["number", "string"]})
    }
}

/// `canvas.to_sketch`'s `canvas`: a canvas reference, or `null` for the last canvas of the design.
fn canvas_or_absent(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A canvas's id (a number) or its name; null is the last canvas in the design.",
        "type": ["number", "string", "null"]
    })
}

/// An image width in mm: above 0 and below 1e7, as `cmd/sketch_canvas.rs` requires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasWidth(pub f64);

impl<'de> Deserialize<'de> for CanvasWidth {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => match n.as_f64() {
                Some(w) if w > 1e-6 && w < 1e7 => Ok(CanvasWidth(w)),
                _ => Err(D::Error::custom("must be positive")),
            },
            _ => Err(D::Error::custom("must be a number of mm")),
        }
    }
}

impl JsonSchema for CanvasWidth {
    fn schema_name() -> Cow<'static, str> {
        "CanvasWidth".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A width in mm: more than 0.000001 and below 10 000 000.",
            "type": "number", "exclusiveMinimum": 1e-6, "exclusiveMaximum": 1e7
        })
    }
}

/// `canvas.calibrate`'s `distance`: a number above 0 (and below 1e9), whatever else it is read
/// as `distance` must be positive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance(pub f64);

impl<'de> Deserialize<'de> for Distance {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let n = match Value::deserialize(d)? {
            Value::Number(n) => n.as_f64().and_then(params::finite),
            _ => None,
        };
        n.filter(|d| *d > 1e-9).map(Distance).ok_or_else(|| D::Error::custom("must be positive"))
    }
}

impl JsonSchema for Distance {
    fn schema_name() -> Cow<'static, str> {
        "Distance".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A distance in mm: above 0 (more than 0.000000001) and below 1 000 000 000.",
            "type": "number", "exclusiveMinimum": 1e-9, "exclusiveMaximum": 1e9
        })
    }
}

/// A point of `canvas.calibrate`: `[x, y]` or `{x, y}` of numbers, with the handler's one message
/// for `a` and `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationPoint(pub solidcore_record::Vec2);

impl<'de> Deserialize<'de> for CalibrationPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec2(&Value::deserialize(d)?).map(CalibrationPoint).ok_or_else(|| D::Error::custom(whole("`a` and `b` must be points [x,y]")))
    }
}

impl JsonSchema for CalibrationPoint {
    fn schema_name() -> Cow<'static, str> {
        "CalibrationPoint".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point2>()
    }
}

/// The plane of `canvas.insert`, as `sketch::plane_ref` reads it: an origin plane's name or alias,
/// a construction plane's name or feature id, a picked face `{face, …}`, or `{origin?, x_dir,
/// y_dir}`. Kept as given, for the handler. Whether a name or id names a plane is checked by the
/// handler, which has the design.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasPlane(pub Value);

impl<'de> Deserialize<'de> for CanvasPlane {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let problem = match &v {
            Value::String(_) => None,
            Value::Number(n) if n.is_u64() => None,
            Value::Number(_) => Some(format!("`plane` must be {}", crate::cmd::PLANE_FORMS)),
            Value::Object(o) if o.contains_key("face") => crate::cmd::face_pick_form(&v).err().map(str::to_string),
            Value::Object(_) => crate::cmd::custom_plane_form(&v).err().map(str::to_string),
            _ => Some(crate::cmd::PLANE_REF_FORMS.to_string()),
        };
        match problem {
            Some(why) => Err(D::Error::custom(whole(why))),
            None => Ok(CanvasPlane(v)),
        }
    }
}

impl JsonSchema for CanvasPlane {
    fn schema_name() -> Cow<'static, str> {
        "CanvasPlane".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A plane: XY, XZ or YZ (or Top, Front or Right; any case), a construction plane's name or feature id, a picked planar face {\"face\": [x, y, z], \"body\"?, \"index\"?, \"path\"?, \"fixed\"?}, or an explicit plane {\"origin\"?, \"x_dir\", \"y_dir\"}: two directions [x, y, z] that aren't parallel and an origin in mm, numbers only (default: the origin).",
            "anyOf": [
                {"type": "string"},
                {"type": "integer", "minimum": 0, "maximum": u64::MAX},
                face_pick(&point),
                {
                    "type": "object",
                    "properties": {"origin": {"anyOf": [point, {"type": "null"}]}, "x_dir": point, "y_dir": point},
                    "required": ["x_dir", "y_dir"],
                    "not": {"required": ["face"]}
                }
            ]
        })
    }
}

/// The face of `canvas.decal`, as the handler reads it: a point on the face, or a picked face
/// `{face, body?, index?, path?, fixed?}`. Kept as given, for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct DecalFace(pub Value);

impl<'de> Deserialize<'de> for DecalFace {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        // A point alone is made a pick, as the handler does.
        let pick = if v.get("face").is_some() { v.clone() } else { json!({"face": v}) };
        match crate::cmd::face_pick_form(&pick) {
            Ok(_) => Ok(DecalFace(v)),
            Err(why) => Err(D::Error::custom(whole(why))),
        }
    }
}

impl JsonSchema for DecalFace {
    fn schema_name() -> Cow<'static, str> {
        "DecalFace".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A point [x, y, z] in mm on a planar face, or a picked face {\"face\": [x, y, z], \"body\"?, \"index\"?, \"path\"?, \"fixed\"?}: \"body\" (the picked body's world name) and \"index\" (the face's index in it) pick the face exactly and go together; \"path\" is the occurrence path of the body's placement (at most 64 nonzero ids); \"fixed\": true keeps the plane where the face is now.",
            "anyOf": [point, face_pick(&point)]
        })
    }
}

/// A picked face, as `sketch::face::face_pick_form` reads it.
fn face_pick(point: &Schema) -> Value {
    json!({
        "type": "object",
        "properties": {
            "face": point,
            "fixed": {"type": "boolean"},
            "body": {"type": "string", "minLength": 1, "description": "Goes with `index`: give both or neither."},
            "index": {"type": "integer", "minimum": 0, "description": "Goes with `body`: give both or neither."},
            "path": {"type": "array", "items": {"type": "integer", "minimum": 1}, "maxItems": 64}
        },
        "required": ["face"]
    })
}

/// `canvas.insert` and `canvas.decal` need `path` or `data`; the handler says so when both are
/// left out.
fn need_image(schema: &mut Schema) {
    schema.insert("anyOf".into(), json!([{"required": ["data"]}, {"required": ["path"]}]));
}

fn image_required(schema: &mut Schema) {
    need_image(schema);
}

fn decal_required(schema: &mut Schema) {
    need_image(schema);
    require(schema, "face");
}

fn canvas_required(schema: &mut Schema) {
    require(schema, "canvas");
}

fn straighten_required(schema: &mut Schema) {
    for key in ["canvas", "a", "b"] {
        require(schema, key);
    }
}

fn calibrate_required(schema: &mut Schema) {
    for key in ["canvas", "a", "b", "distance"] {
        require(schema, key);
    }
}

/// What `canvas.insert` and `canvas.decal` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CanvasPlaced {
    /// The new canvas's id.
    pub canvas: u64,
    /// Its name.
    pub name: String,
    /// The image's size in pixels, [width, height].
    pub pixels: [u32; 2],
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `canvas.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CanvasEdited {
    /// The canvas's id.
    pub canvas: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `canvas.calibrate` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CanvasCalibrated {
    /// The canvas's id.
    pub canvas: u64,
    /// Its width now, in mm.
    pub width: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `canvas.straighten` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CanvasStraightened {
    /// The canvas's id.
    pub canvas: u64,
    /// Its rotation on the plane now, in degrees: above -180, at most 180.
    pub angle: f64,
    /// How far it turned, in degrees, counter-clockwise; above -90, at most 90.
    pub turned: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `canvas.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CanvasDeleted {
    /// The id the canvas had.
    pub deleted: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `canvas.to_sketch` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CanvasTraced {
    /// The new sketch's feature id.
    pub sketch: u64,
    /// Its name.
    pub name: String,
    /// The canvas that was traced, by id.
    pub canvas: u64,
    /// How many curves the sketch was given.
    pub curves: usize,
    /// How many outlines the image held.
    pub outlines: usize,
    /// How many of them were holes.
    pub holes: usize,
    /// The sketch's solve: `{solved, dof}`; null when it was not solved again.
    pub info: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
