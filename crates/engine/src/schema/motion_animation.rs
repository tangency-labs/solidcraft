//! Contract and reply type for Export Motion Animation (`motion.export_animation`, #13310): a
//! study played, one joint driven or an exploded view coming apart, drawn frame by frame and
//! written as an animated GIF or a folder of PNG frames. The design is not changed.
//!
//! The handler (`cmd/motion_animation.rs`) reads its parameters from the raw JSON one after the
//! other, so the contract reads the whole call in the same order and with the same messages:
//! `path`, `width` and `height`, `fps`, `grid` and `axes`, what moves (an exploded `view` with
//! `back`, a `study`, or a `joint` with `from` and `to`), the `camera`, and `samples`, whose
//! upper end follows from the frame size and the format. A key given as `null` is given, as the
//! handler reads it. Keys the handler doesn't read in a case aren't read here either: `back` only
//! goes with a `view`, and `index`, `from` and `to` with a `joint`.
//!
//! The camera's fields are tried on the default camera: a field the handler refuses is wrong
//! whatever camera it completes, so the message is the handler's. What only the session can tell
//! stays the handler's to check: whether the view, the study and the joint exist, whether the
//! joint moves, `index` (its message names the joint's value count), what `from` and `to`
//! evaluate to, whether a renderer is installed and where the file may be written. A call with
//! several problems may get a different one first in strict than with the check off.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::check::whole;
use super::motion_analysis::StudyStop;
use super::types::not_blank_pattern;
use solidcraft_viewport::Camera;

/// The most frames of one animation. The handler (`cmd/motion_animation.rs`) uses these limits
/// too, so the schema, the strict check and the handler cannot disagree.
pub(crate) const MAX_FRAMES: usize = 1000;
/// The largest frame side, in pixels.
pub(crate) const MAX_SIDE: u32 = 4096;
/// The most pixels of a GIF's frames together: they are held in memory (4 bytes each) until the
/// palette is chosen, so 256 MB at most.
pub(crate) const MAX_GIF_PIXELS: usize = 1 << 26;
/// The most pixels of a PNG folder's frames together (each frame is written as it is drawn):
/// about 500 frames of 1920 × 1080.
pub(crate) const MAX_PNG_PIXELS: usize = 1 << 30;
/// The camera fields `camera` may set: those of `ui.camera`'s reply.
pub(crate) const CAMERA_FIELDS: [&str; 7] = ["target", "yaw", "pitch", "roll", "distance", "fov", "up"];
/// The handler's message for a camera that isn't finite or is out of range.
const BAD_CAMERA: &str = "the camera must be finite, with a distance up to 1e15 mm, a pitch within ±π/2 and a fov of 0 (orthographic) to 3 radians";

/// Draw a motion as an animation, frame by frame with the headless renderer: a motion study
/// played, one joint driven from `from` to `to`, or an exploded view coming apart (and back
/// together with `back`). Written as an animated GIF, or as a folder of PNG frames. The design
/// is not changed, nor its placements.
#[derive(Debug)]
pub struct ExportAnimationParams {
    /// The `.gif` file, or the folder for the PNG frames.
    pub path: String,
    /// The frame size, in pixels.
    pub width: u32,
    pub height: u32,
    /// A GIF's frames per second.
    pub fps: f64,
    /// Whether the ground grid and the origin's axes are drawn.
    pub grid: bool,
    pub axes: bool,
    /// What moves.
    pub motion: AnimatedMotion,
    /// The camera fields given, if any.
    pub camera: Option<Map<String, Value>>,
    /// The frames asked for, if any.
    pub samples: Option<usize>,
}

/// What an animation shows.
#[derive(Debug)]
pub enum AnimatedMotion {
    /// An exploded view, by id or name, coming apart (and back together with `back`).
    View { view: Value, back: bool },
    /// A motion study, by id or name, played from start to end.
    Study(Value),
    /// A joint, by id or name, driven from `from` to `to`: its value `index` (as given; the
    /// handler checks it against the joint's value count).
    Joint { joint: Value, index: Option<Value>, from: Value, to: Value },
}

impl ExportAnimationParams {
    /// The keys the handler reads, in its order and in the cases it reads them.
    fn read(o: &Map<String, Value>) -> Result<ExportAnimationParams, String> {
        let path = o
            .get("path")
            .and_then(Value::as_str)
            .filter(|x| !x.trim().is_empty() && x.len() <= 4096 && !x.chars().any(char::is_control))
            .ok_or("`path` must name a .gif file, or a folder for PNG frames")?;
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".png") {
            return Err("`path` is a .gif file or a folder for the PNG frames (frame-0001.png, …), not one .png".into());
        }
        let gif = lower.ends_with(".gif");
        if !gif && cfg!(target_arch = "wasm32") {
            return Err("a folder of PNG frames is not available in the browser: give a .gif path".into());
        }
        let (width, height) = (side(o, "width", 800)?, side(o, "height", 500)?);
        let fps = match o.get("fps") {
            None => 20.0,
            Some(v) => v.as_f64().filter(|n| n.is_finite() && (1.0..=50.0).contains(n)).ok_or("`fps` must be 1…50")?,
        };
        let (grid, axes) = (flag(o, "grid", true)?, flag(o, "axes", true)?);
        let motion = motion(o)?;
        let camera = o.get("camera").map(camera_fields).transpose()?;
        let max = MAX_FRAMES.min(if gif { MAX_GIF_PIXELS } else { MAX_PNG_PIXELS } / (width as usize * height as usize));
        let samples = match o.get("samples") {
            None => None,
            Some(v) => Some(
                v.as_f64()
                    .filter(|n| n.is_finite() && *n >= 2.0 && *n <= max as f64)
                    .map(|n| n.round() as usize)
                    .ok_or_else(|| format!("`samples` must be 2…{max} for {width} × {height} frames"))?,
            ),
        };
        Ok(ExportAnimationParams { path: path.to_string(), width, height, fps, grid, axes, motion, camera, samples })
    }
}

/// `width` or `height`: 16 to 4096 pixels, rounded, or `default` left out.
fn side(o: &Map<String, Value>, key: &str, default: u32) -> Result<u32, String> {
    match o.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite() && (16.0..=f64::from(MAX_SIDE)).contains(n))
            .map(|n| n.round() as u32)
            .ok_or_else(|| format!("`{key}` must be 16…{MAX_SIDE} pixels")),
    }
}

/// True or false, or `default` left out.
fn flag(o: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match o.get(key) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("`{key}` must be true or false")),
    }
}

/// An id (a number) or a name, or `message`.
fn id_or_name(v: &Value, message: &str) -> Result<Value, String> {
    match v {
        Value::Number(_) | Value::String(_) => Ok(v.clone()),
        _ => Err(message.into()),
    }
}

/// What moves, as the handler picks it: an exploded `view` alone, else `study` or `joint`
/// (through `motion_plan` in `cmd/motion.rs`).
fn motion(o: &Map<String, Value>) -> Result<AnimatedMotion, String> {
    let picked = "give a `study`, a `joint` with `from` and `to`, or an exploded `view`";
    match (o.get("view"), o.get("study").or(o.get("joint"))) {
        (Some(_), Some(_)) => Err("give an exploded `view`, a `study` or a `joint`: one of them".into()),
        (Some(view), None) => {
            let back = flag(o, "back", false)?;
            Ok(AnimatedMotion::View { view: id_or_name(view, "`view` must be an id or a name")?, back })
        }
        (None, None) => Err(picked.into()),
        (None, Some(_)) => {
            if o.contains_key("back") {
                return Err("`back` goes with an exploded `view`".into());
            }
            match (o.get("study"), o.get("joint")) {
                (Some(_), Some(_)) => Err("give `study` or `joint`, not both".into()),
                (Some(study), None) => Ok(AnimatedMotion::Study(id_or_name(study, "`study` must be an id or a name")?)),
                (None, Some(joint)) => {
                    let joint = id_or_name(joint, "`joint` must be an id or a name")?;
                    // The handler looks the joint up and reads `index` between the two.
                    let end = |key: &str| match o.get(key) {
                        Some(v) => joint_value(v),
                        None => Err(format!("give `from` and `to`: the joint's values the motion runs between (no `{key}`)")),
                    };
                    let (from, to) = (end("from")?, end("to")?);
                    Ok(AnimatedMotion::Joint { joint, index: o.get("index").cloned(), from, to })
                }
                (None, None) => Err(picked.into()),
            }
        }
    }
}

/// `from` or `to`, as `joints::value` reads it: a number between -1e9 and 1e9, or text (an
/// expression, which the handler evaluates), with its message for anything else.
fn joint_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::Number(n) if n.as_f64().is_some_and(|x| x.is_finite() && x.abs() < 1e9) => Ok(v.clone()),
        Value::String(_) => Ok(v.clone()),
        _ => Err(format!("a joint value {}", crate::params::expr_problem(Some(v)))),
    }
}

/// `camera`: an object of `ui.camera`'s fields, read as the handler reads it, on the default
/// camera instead of the session's (each check is on the fields given).
fn camera_fields(given: &Value) -> Result<Map<String, Value>, String> {
    let fields = given.as_object().ok_or("`camera` must be an object, such as ui.camera's reply")?;
    let mut v = serde_json::to_value(Camera::default()).map_err(|e| e.to_string())?;
    for (k, x) in fields {
        if !CAMERA_FIELDS.contains(&k.as_str()) {
            return Err(format!("unknown camera field `{k}` (it takes {})", CAMERA_FIELDS.join(", ")));
        }
        if let Some(o) = v.as_object_mut() {
            o.insert(k.clone(), x.clone());
        }
    }
    let cam: Camera = serde_json::from_value(v).map_err(|e| format!("bad camera: {e}"))?;
    if !cam.is_valid() || cam.distance > 1e15 || cam.pitch.abs() > std::f64::consts::FRAC_PI_2 || !(0.0..3.0).contains(&cam.fov) {
        return Err(BAD_CAMERA.into());
    }
    Ok(fields.clone())
}

impl<'de> Deserialize<'de> for ExportAnimationParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = Map::<String, Value>::deserialize(d)?;
        ExportAnimationParams::read(&o).map_err(|why| D::Error::custom(whole(why)))
    }
}

impl JsonSchema for ExportAnimationParams {
    fn schema_name() -> Cow<'static, str> {
        "ExportAnimationParams".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // `index`, `from` and `to` are read only with a `joint`: their shapes are in the `anyOf` below,
        // and the properties only describe them.
        let end_description = |end: &str| {
            format!(
                "With `joint`: the value the motion {end} at, a number (degrees for an angle, mm for a distance, between -1e9 and 1e9) \
                 or an expression such as \"width / 2\" or \"30 deg\". Not read without a joint."
            )
        };
        let index = "With `joint`: which of its values to drive, below its value count. Default: 0. Not read without a joint.";
        let side_schema = |what: &str, default: u32| {
            json_schema!({
                "description": format!("The frames' {what} in pixels, 16…{MAX_SIDE} (rounded). Default: {default}."),
                "type": "number",
                "minimum": 16,
                "maximum": MAX_SIDE,
                "default": default
            })
        };
        json_schema!({
            "description": "Draw a motion as an animation, frame by frame with the headless renderer: a motion study played (`study`), \
                one joint driven from `from` to `to` (`joint`), or an exploded view coming apart (`view`, and back together with \
                `back`). Written as an animated GIF, or as a folder of PNG frames. The design is not changed, nor its placements.",
            "type": "object",
            "properties": {
                "path": {
                    "description": "A .gif file, or a folder for the PNG frames (frame-0001.png, frame-0002.png, …), made where \
                        missing; frames of an earlier export there are removed first. Not one .png file. At most 4096 bytes (the \
                        schema counts characters), with no control characters.",
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 4096,
                    "pattern": not_blank_pattern()
                },
                "width": side_schema("width", 800),
                "height": side_schema("height", 500),
                "fps": {
                    "description": "A GIF's frames per second, 1…50. A GIF counts in hundredths of a second, so the reply's `fps` \
                        is the rate it plays at. Default: 20.",
                    "type": "number",
                    "minimum": 1,
                    "maximum": 50,
                    "default": 20
                },
                "grid": {"description": "Draw the ground grid. Default: true.", "type": "boolean", "default": true},
                "axes": {"description": "Draw the origin's axes. Default: true.", "type": "boolean", "default": true},
                "view": {
                    "description": "An exploded view, by id (a number) or name: the parts coming apart from where they are \
                        assembled. Instead of `study` or `joint`.",
                    "type": ["number", "string"]
                },
                "back": {
                    "description": "With `view`: true comes back together too, exploded at the middle frame of an odd count. \
                        Default: false.",
                    "type": "boolean",
                    "default": false
                },
                "study": {
                    "description": "A motion study, by id (a number) or name, played from start to end. A number is looked up as \
                        an id first, text as a name first. Instead of `view` or `joint`.",
                    "type": ["number", "string"]
                },
                "joint": {
                    "description": "A joint, by id (a number) or name, driven from `from` to `to`; a rigid joint doesn't move. \
                        Instead of `view` or `study`.",
                    "type": ["number", "string"]
                },
                "index": {"description": index},
                "from": {"description": end_description("starts")},
                "to": {"description": end_description("ends")},
                "camera": {
                    "description": "The camera the frames are drawn from: fields of ui.camera's reply. Fields left out keep the \
                        default view: view.apply's camera, or the home view framing the model (for an exploded view, both \
                        assembled and exploded). Angles in radians.",
                    "type": "object",
                    "properties": {
                        "target": {
                            "description": "The point looked at, [x, y, z] in mm.",
                            "type": "array",
                            "items": {"type": "number"},
                            "minItems": 3,
                            "maxItems": 3
                        },
                        "yaw": {"description": "Turn about the up axis; 0 looks along +Y, from the front.", "type": "number"},
                        "pitch": {
                            "description": "Elevation of the eye above the ground plane.",
                            "type": "number",
                            "minimum": -std::f64::consts::FRAC_PI_2,
                            "maximum": std::f64::consts::FRAC_PI_2
                        },
                        "roll": {"description": "Turn about the viewing direction.", "type": "number"},
                        "distance": {
                            "description": "From the target to the eye, in mm.",
                            "type": "number",
                            "exclusiveMinimum": 0,
                            "maximum": 1e15
                        },
                        "fov": {
                            "description": "Vertical field of view; 0 is orthographic.",
                            "type": "number",
                            "minimum": 0,
                            "exclusiveMaximum": 3
                        },
                        "up": {"description": "The world's up axis.", "enum": ["z", "y"]}
                    },
                    "additionalProperties": false
                },
                "samples": {
                    "description": format!(
                        "How many frames, 2…{MAX_FRAMES}, with at most 2^26 pixels in all for a GIF and 2^30 for PNG frames, so \
                         fewer for big frames (4 at 4096 × 4096 for a GIF). Default: the study's steps + 1, 100 for a joint, 41 for \
                         an exploded view (81 with `back`)."
                    ),
                    "type": "number",
                    "minimum": 2,
                    "maximum": MAX_FRAMES
                }
            },
            "required": ["path"],
            "oneOf": [{"required": ["view"]}, {"required": ["study"]}, {"required": ["joint"]}],
            "anyOf": [
                {"not": {"required": ["joint"]}},
                {
                    "properties": {
                        "index": {"type": "integer", "minimum": 0, "default": 0},
                        "from": {"type": ["number", "string"]},
                        "to": {"type": ["number", "string"]}
                    }
                }
            ],
            "dependentRequired": {"back": ["view"], "joint": ["from", "to"]},
            "additionalProperties": false
        })
    }
}

/// What `motion.export_animation` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AnimationExported {
    /// The file or the folder written, as given.
    pub path: String,
    /// `gif` for an animated GIF, `png` for a folder of PNG frames.
    pub format: &'static str,
    /// How many frames were drawn.
    pub frames: usize,
    /// The frame size, in pixels.
    pub width: u32,
    pub height: u32,
    /// A GIF's frames per second: the rate it plays at, which a GIF counts in hundredths of a
    /// second. Absent for PNG frames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
    /// The bytes written: the GIF's, or all the PNG frames'.
    pub bytes: usize,
    /// What drew the frames: `cpu` for the CPU rasterizer.
    pub renderer: &'static str,
    /// What frames drawn by the CPU rasterizer lack. Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<&'static str>,
    #[serde(flatten)]
    pub stop: StudyStop,
    /// Where two parts in a contact set touched and stopped the motion, or `null`.
    pub stopped_by_contact: Option<Map<String, Value>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "motion_animation_tests.rs"]
mod tests;
