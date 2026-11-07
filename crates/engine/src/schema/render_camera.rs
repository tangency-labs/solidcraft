//! Contracts and reply types for the render camera commands in `cmd/render.rs` (#12288): Set,
//! Inspect and Frame Render Camera, and Apply Render Camera. They change only the camera in the
//! view, never the design.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a saved setup exists, and whether the
//! product can be framed, stay the handlers' to check.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcraft_doc::render_settings::{RenderSettings, RenderSetup};

use super::check::whole;
use super::types::not_blank_pattern;

/// The largest size a camera's target coordinates and distance may have, in mm, as a saved
/// render setup keeps them (`RenderSetup::validate`).
const MAX_LENGTH_MM: f64 = 1e9;
/// The largest size a camera's yaw, pitch and roll may have, in radians, likewise.
const MAX_ANGLE: f64 = 1e6;
/// The picture's width over its height that Frame Render Subject takes: the range a preview
/// frame can show (`frame_rect` in the Render workspace).
const MIN_ASPECT: f64 = 1e-6;
const MAX_ASPECT: f64 = 1e6;

/// A render camera, as a saved render setup keeps it: strict about its keys, with finite
/// lengths and angles within [`MAX_LENGTH_MM`] and [`MAX_ANGLE`], a positive distance and a
/// field of view in [0, π).
///
/// It is not the view commands' `types::CameraParam`: those read a camera more loosely and
/// refuse an explicit `up`, while a render camera keeps both `roll` and `up`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RenderCamera(pub solidcraft_render::Camera);

impl JsonSchema for RenderCamera {
    fn schema_name() -> Cow<'static, str> {
        "RenderCamera".into()
    }
    /// The size bounds are for a camera given (the input schema) only. A reply gives back the
    /// camera in the view, which Frame Render Subject may have moved beyond them (a picture far
    /// taller than wide moves it back that much) and the view commands don't bound, so the
    /// output schema has the camera's shape alone.
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let given = generator.contract().is_deserialize();
        // `schema` with a largest size of `limit` either way, in the input schema only.
        let sized = |mut schema: Value, limit: f64| {
            if given {
                schema["minimum"] = json!(-limit);
                schema["maximum"] = json!(limit);
            }
            schema
        };
        let angle = |description: &str| sized(json!({"description": description, "type": "number"}), MAX_ANGLE);
        let coordinate = sized(json!({"type": "number"}), MAX_LENGTH_MM);
        let mut roll = angle("Turn about the viewing direction; 0 keeps the up axis upright. Default: 0.");
        roll["default"] = json!(0.0);
        let mut distance = json!({"description": "From the target to the eye, in mm.", "type": "number", "exclusiveMinimum": 0});
        if given {
            distance["maximum"] = json!(MAX_LENGTH_MM);
        }
        let bounds = if given { " Lengths are at most 1e9 mm in size, and angles at most 1e6 radians." } else { "" };
        json_schema!({
            "description": format!("A render camera orbiting a target point. Lengths are in mm and angles in radians, measured in the frame of the up axis.{bounds}"),
            "type": "object",
            "properties": {
                "target": {
                    "description": "The point the camera looks at, in mm: [x, y, z].",
                    "type": "array",
                    "items": coordinate,
                    "minItems": 3,
                    "maxItems": 3
                },
                "yaw": angle("Turn about the up axis; 0 looks along +Y, from the front."),
                "pitch": angle("Elevation above the ground plane: π/2 looks straight down."),
                "roll": roll,
                "distance": distance,
                "fov": {"description": "Vertical field of view; 0 is orthographic.", "type": "number", "minimum": 0, "exclusiveMaximum": std::f64::consts::PI},
                "up": {"description": "The world's up axis. Default: z.", "enum": ["z", "y"], "default": "z"}
            },
            "required": ["target", "yaw", "pitch", "distance", "fov"],
            "additionalProperties": false
        })
    }
}

/// `serde(default)` makes schemars publish `"default": null` on a key; drop it from a key the
/// schema requires.
fn drop_default(schema: &mut Schema) {
    schema.remove("default");
}

/// Adds `key` to the schema's `required` list: for a key the type has optional so that the
/// handler, not the check, refuses it with its own message when it's left out.
/// (`#[schemars(required)]` doesn't do this for a key with `serde(default)`.)
fn require(schema: &mut Schema, key: &str) {
    match schema.get_mut("required") {
        Some(Value::Array(required)) => {
            if !required.iter().any(|k| k == key) {
                required.push(key.into());
            }
        }
        _ => {
            schema.insert("required".into(), Value::Array(vec![key.into()]));
        }
    }
}

fn camera_required(schema: &mut Schema) {
    require(schema, "camera");
}

fn name_required(schema: &mut Schema) {
    require(schema, "name");
}

/// Set the camera in the view from a complete camera snapshot. Nothing in the design changes,
/// and there is no undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = camera_required)]
pub struct SetParams {
    /// The camera: `target`, `yaw`, `pitch`, `distance` and `fov`, and optionally `roll` and
    /// `up`.
    // Optional in the type, required in the schema: left out, the handler refuses the call
    // with its own message.
    #[serde(default, deserialize_with = "render_camera")]
    #[schemars(with = "RenderCamera", transform = drop_default)]
    pub camera: Option<RenderCamera>,
}

/// A camera as `render.camera.set` reads it: a render setup holding only that camera, decoded
/// and checked as a saved one is, with the same messages.
fn render_camera<'de, D: Deserializer<'de>>(d: D) -> Result<Option<RenderCamera>, D::Error> {
    let setup: RenderSetup = serde_json::from_value(json!({"camera": Value::deserialize(d)?})).map_err(|e| D::Error::custom(whole(e)))?;
    setup.validate().map_err(|e| D::Error::custom(whole(e)))?;
    Ok(Some(RenderCamera(setup.camera)))
}

/// The camera set in the view, if any. No parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GetParams {}

/// Fit the camera in the view to the visible product, keeping its direction: for a picture
/// taller than wide it moves back, so the picture's sides hold the product too.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FitParams {
    // `skip_serializing_if` keeps schemars from publishing `"default": null`, which the reader
    // refuses, over the schema's own default of 1.
    #[serde(default, deserialize_with = "aspect", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "aspect_schema")]
    pub aspect: Option<f64>,
}

/// `aspect`, with `render.camera.fit`'s message for anything outside its range, `null`
/// included.
fn aspect<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)?.as_f64() {
        Some(a) if (MIN_ASPECT..=MAX_ASPECT).contains(&a) => Ok(Some(a)),
        _ => Err(D::Error::custom(whole("aspect must be a number from 0.000001 to 1000000: the picture's width over its height"))),
    }
}

fn aspect_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The picture's width over its height: 0.000001 to 1000000. Default: 1.",
        "type": "number",
        "minimum": MIN_ASPECT,
        "maximum": MAX_ASPECT,
        "default": 1.0
    })
}

/// Put a saved render setup's camera, with its roll and up axis, in the view. Nothing in the
/// design changes, and there is no undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = name_required)]
pub struct ApplyParams {
    /// The saved render setup, by name: trimmed, at most 128 bytes, with no control characters.
    // Optional in the type, required in the schema: left out (or `null`), the handler refuses
    // the call with its own message.
    #[serde(default, deserialize_with = "setup_name")]
    #[schemars(schema_with = "setup_name_schema", transform = drop_default)]
    pub name: Option<String>,
}

/// A setup's name as `render.settings.apply` reads it: `null` is left out, other text than a
/// string gets serde's message, and a string must be a valid setup name.
pub(super) fn setup_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let Some(name) = Option::<String>::deserialize(Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))? else {
        return Ok(None);
    };
    RenderSettings::validate_name(&name).map_err(|e| D::Error::custom(whole(e)))?;
    Ok(Some(name))
}

/// The pattern says "not blank"; trimmed, at most 128 bytes and free of control characters,
/// which JSON Schema can't count or class portably, are in the field's description and
/// checked by [`setup_name`].
pub(super) fn setup_name_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 128,
        "pattern": not_blank_pattern()
    })
}

/// What `render.camera.set` returns: the camera now in the view.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CameraSet {
    pub camera: RenderCamera,
    /// Always false: the design doesn't change.
    pub document_changed: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.camera.get` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CameraInfo {
    /// The camera set in the view, or `null` when none is.
    pub camera: Option<RenderCamera>,
    /// Whether a camera is set in the view.
    pub explicit_view: bool,
    /// Always false: the design doesn't change.
    pub document_changed: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.camera.fit` returns: the fitted camera, now in the view.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CameraFitted {
    pub camera: RenderCamera,
    /// Whether nothing visible was there to frame. The camera then isn't moved: it stays the
    /// one set in the view, or the default camera when none was.
    pub empty: bool,
    /// Always false: the design doesn't change.
    pub document_changed: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.settings.apply` returns: the setup and its camera, now in the view.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CameraApplied {
    pub name: String,
    pub camera: RenderCamera,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_camera_tests.rs"]
mod tests;
