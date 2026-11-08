//! Contract for `solid.align` in `cmd/features_more.rs` (#13314). Align adds a feature and returns
//! `add_feature`'s reply, a [`super::features::FeatureResult`].
//!
//! The handler reads its parameters from the raw JSON, so each key here reads as the handler reads
//! it and fails with the handler's message. Whether a body, a plane, a sketch or a joint origin
//! exists, whether a pick lies on a planar face, and which combinations of keys go together, stay
//! the handler's to check.
//!
//! Align reads some keys only in some cases, and the contract reads them in the same cases:
//!
//! - a side (`from` or `to`) that has a plane or a frame (`from_plane`, `from_frame`) is worked
//!   out from that, and its point and face keys are not read (a call that gives both is refused by
//!   the handler as a whole);
//! - otherwise a face that is a point (`from_face`) wins, and `from` is not read at all;
//! - otherwise `from` is read: `"origin"`, `{snap?, at}` or `[x, y, z]`.
//!
//! So the type is written by hand, in the handler's order, and a call that works today gets no
//! warning for a key the handler does not read. Where the handler reads a key of the wrong type as
//! if it were left out (`flip`, `angle`, a `from_face` that is not a point, a `snap` that is not
//! text), the contract refuses it: that is what warn and strict are for. `bodies` is optional in
//! the type and required in the schema, and so is one of `from`, `from_face`, `from_plane` and
//! `from_frame` (and the same for `to`), so that the handler's own message is what a caller sees.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};

use super::check::whole;
use super::config::NameList;
use super::joints::JointOriginArg;
use super::motion::require;
use super::types::{AngleExpr, FeatureCommon, PlaneParam, Point3, SketchRef, StringList, feature_common};
use crate::cmd::{MAX_SKETCH_ID, SidePoint, SketchFrameId, axes_mask, sketch_frame_id, sketch_key};
use crate::params::{self, MAX_STRING_LIST};

/// The handler's message for `bodies` left out, `null` or empty.
const BODIES_MESSAGE: &str = "`bodies` must list the bodies to move";

/// The kinds of snap point `from` and `to` take.
const SNAPS: [&str; 4] = ["vertex", "edge_mid", "face_center", "circle_center"];

/// The `bodies` schema: one name, or a list of 1 to 10 000.
fn bodies_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The bodies to move, by name: one, or a list of 1 to 10 000.",
        "anyOf": [
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": MAX_STRING_LIST}
        ]
    })
}

/// A point of an Align side: `"origin"`, a snap `{snap?, at}` or `[x, y, z]`. Only for the schema:
/// whether it is read at all depends on the side's other keys, so [`AlignParams`] reads it.
struct SidePointSchema;

impl JsonSchema for SidePointSchema {
    fn schema_name() -> Cow<'static, str> {
        "AlignPoint".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let c = json!({"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9});
        json_schema!({
            "description": "A point: [x, y, z] in mm, the text \"origin\" (any case), or {\"snap\"?, \"at\"}: the nearest vertex, edge midpoint, planar face centre or circular edge centre of the model to the point `at`. {\"x\", \"y\", \"z\"} is not a point here.",
            "anyOf": [
                {"type": "array", "items": c, "minItems": 3, "maxItems": 3},
                {"type": "string", "pattern": "^[Oo][Rr][Ii][Gg][Ii][Nn]$"},
                {
                    "type": "object",
                    "properties": {
                        "snap": {"description": "What to snap to. Default: vertex.", "type": "string", "enum": SNAPS},
                        "at": {"description": "The point to snap near: [x, y, z] or {\"x\", \"y\", \"z\"}, in mm.", "anyOf": [
                            {"type": "array", "items": c, "minItems": 3, "maxItems": 3},
                            {"type": "object", "properties": {"x": c, "y": c, "z": c}, "required": ["x", "y", "z"]}
                        ]}
                    },
                    "required": ["at"]
                }
            ]
        })
    }
}

/// A frame of an Align side: a joint origin as `joint.create` takes one, or `{sketch, point|curve}`.
/// Only for the schema; [`AlignParams`] reads it with [`frame_problem`].
struct FrameSchema;

impl JsonSchema for FrameSchema {
    fn schema_name() -> Cow<'static, str> {
        "AlignFrame".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let sketch = generator.subschema_for::<SketchRef>();
        let origin = generator.subschema_for::<JointOriginArg>();
        let id = json!({"type": "string", "minLength": 1, "maxLength": MAX_SKETCH_ID});
        json_schema!({
            "description": "A frame that is found again whenever the design changes, so the bodies follow it: a sketch point, or the centre of a sketch circle, arc or ellipse, as {\"sketch\", \"point\" | \"curve\"} (z along the sketch's normal); or a joint origin as joint.create takes one: {face | circle | edge | point: [x, y, z]}, a face's name, or the name of a joint origin. On the bodies' own component, which must hold all of them and be placed once.",
            "anyOf": [
                {
                    "type": "object",
                    "properties": {
                        "sketch": sketch,
                        "point": {"description": "A sketch point's id: 1 to 256 characters. Give this or `curve`.", "allOf": [id]},
                        "curve": {"description": "A sketch circle's, arc's or ellipse's id: 1 to 256 characters. Give this or `point`.", "allOf": [id]}
                    },
                    "required": ["sketch"],
                    "anyOf": [
                        {"required": ["point"], "not": {"required": ["curve"]}},
                        {"required": ["curve"], "not": {"required": ["point"]}}
                    ]
                },
                {"allOf": [origin], "not": {"type": "object", "required": ["sketch"]}}
            ]
        })
    }
}

/// `axes`: three booleans, at least one true. Only for the schema.
struct AxesSchema;

impl JsonSchema for AxesSchema {
    fn schema_name() -> Cow<'static, str> {
        "AlignAxes".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Which world axes the move follows: [x, y, z] booleans, at least one true.",
            "type": "array",
            "items": {"type": "boolean"},
            "minItems": 3,
            "maxItems": 3,
            "contains": {"const": true}
        })
    }
}

/// `bodies` is required, and so is one key for each side.
fn align_schema(schema: &mut Schema) {
    require(schema, "bodies");
    // Each side needs one of its four keys; the handler says which, in its own words.
    let side = |keys: [&str; 4]| json!({"anyOf": keys.iter().map(|k| json!({"required": [k]})).collect::<Vec<_>>()});
    let sides = [side(["from", "from_face", "from_plane", "from_frame"]), side(["to", "to_face", "to_plane", "to_frame"])];
    match schema.get_mut("allOf") {
        Some(Value::Array(all)) => all.extend(sides),
        _ => {
            schema.insert("allOf".into(), Value::Array(sides.into()));
        }
    }
}

/// Move bodies so that a point, face, plane or frame on them meets another one, as a feature in
/// the timeline (one undo step). Each side is one of: a point (`from`, `to`), a planar face
/// picked by a point on it (`from_face`, `to_face`), a plane (`from_plane`, `to_plane`) or a frame
/// (`from_frame`, `to_frame`); give one key for each side. A plane or a frame is worked out again
/// whenever the design changes, so the bodies follow it. Faces, planes and frames also turn the
/// bodies so that they meet (the first side's face against the second's, or with `flip` side by
/// side); a point onto a plane moves along the plane's normal. `axes` keeps the move to some
/// world axes and only goes with two points. Align makes no body, so `body_name` and `body_names`
/// do nothing here (as for Remove), and `participants` changes nothing in the result.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = align_schema)]
pub struct AlignParams {
    // Optional in the type and required in the schema (`align_schema`): left out, `null` or
    // empty, the handler refuses the call with its own message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "bodies_schema")]
    pub bodies: Option<NameList>,
    /// Where the move starts, on the bodies. Not read when `from_face` is a point; not with
    /// `from_plane` or `from_frame` (the handler refuses both).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SidePointSchema")]
    pub from: Option<Value>,
    /// A point on a planar face of the bodies to start from: the bodies turn so that the face
    /// meets the target's. When it is a point, `from` is not read. Not with `from_plane` or
    /// `from_frame`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point3")]
    pub from_face: Option<Value>,
    /// A plane to start from, kept as a plane so the bodies follow it when it moves; the bodies
    /// turn so that it meets the target. Not with `from`, `from_face` or `from_frame`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PlaneParam")]
    pub from_plane: Option<Value>,
    /// A frame to start from, in place of the point, face or plane; the bodies turn so that it
    /// meets the target. Not with `from`, `from_face` or `from_plane`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FrameSchema")]
    pub from_frame: Option<Value>,
    /// Where the bodies go. Not read when `to_face` is a point; not with `to_plane` or
    /// `to_frame`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SidePointSchema")]
    pub to: Option<Value>,
    /// A point on a planar face to go to. When it is a point, `to` is not read. Not with
    /// `to_plane` or `to_frame`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point3")]
    pub to_face: Option<Value>,
    /// A plane to go to, which may move later (the bodies follow); a point or a face goes onto
    /// it along its normal. Not with `to`, `to_face` or `to_frame`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PlaneParam")]
    pub to_plane: Option<Value>,
    /// A frame to go to, in place of the point, face or plane. It must not be on a body that
    /// moves. Not with `to`, `to_face` or `to_plane`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FrameSchema")]
    pub to_frame: Option<Value>,
    /// With faces, planes or frames: true turns the bodies so that the two normals point the same
    /// way, instead of facing each other. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Which world axes the offset moves along, with a point on each side: [x, y, z] booleans,
    /// at least one true. Default: all three. Refused with a face, plane or frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AxesSchema")]
    pub axes: Option<[bool; 3]>,
    /// A turn about the target's direction (a face's normal, a plane's normal or a frame's z)
    /// after the move, in degrees or an expression. Needs a face, a plane or a frame as the
    /// target. Default: none.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    #[serde(flatten)]
    pub common: FeatureCommon,
}

/// `bodies` as the handler reads it: names, at least one; `null` and a missing key are none.
fn bodies(m: &Map<String, Value>) -> Result<NameList, String> {
    let names = match m.get("bodies") {
        None | Some(Value::Null) => NameList::default(),
        // Each of its messages starts with "must": the key goes in front, as `string_list` has it.
        Some(v) => NameList::deserialize(v).map_err(|e| whole(format!("`bodies` {e}")))?,
    };
    if names.0.is_empty() { Err(whole(BODIES_MESSAGE)) } else { Ok(names) }
}

/// `key`, a plane, when it is given (`null` included: the handler reads it as a plane and
/// refuses it).
fn plane(m: &Map<String, Value>, key: &str) -> Result<Option<Value>, String> {
    // The reader's own message is a whole sentence, marked as such.
    m.get(key).map(|v| PlaneParam::deserialize(v).map(|p| p.0).map_err(|e| e.to_string())).transpose()
}

/// What is wrong with the frame `v` of `key` (`from_frame`), as the handler says it, marked
/// whole, or `None`.
fn frame_problem(v: &Value, key: &str) -> Option<String> {
    if let Some(sketch) = v.get("sketch") {
        if sketch_key(Some(sketch)).is_none() {
            return Some(whole(format!("`{key}` must be a sketch id or name")));
        }
        return match sketch_frame_id(v) {
            Some(SketchFrameId::Point(_) | SketchFrameId::Curve(_)) => None,
            None => Some(whole(format!("`{key}`: a sketch frame takes `point` or `curve` (an id of 1 to {MAX_SKETCH_ID} characters)"))),
        };
    }
    // A joint origin, which the handler reads as `joint.create` reads one.
    JointOriginArg::deserialize(v).err().map(|e| e.to_string())
}

/// `<key>_frame`, when it is given and not `null`.
fn frame(m: &Map<String, Value>, key: &str) -> Result<Option<Value>, String> {
    let frame_key = format!("{key}_frame");
    match m.get(&frame_key) {
        None | Some(Value::Null) => Ok(None),
        // The handler refuses a frame given beside the side's other keys before it reads the frame.
        Some(v) if [key.to_string(), format!("{key}_face"), format!("{key}_plane")].iter().any(|k| m.contains_key(k)) => Ok(Some(v.clone())),
        Some(v) => frame_problem(v, &frame_key).map_or_else(|| Ok(Some(v.clone())), Err),
    }
}

/// The point and face of the side `pk` (`from`), read as the handler reads them: not at all when
/// the side has a plane or a frame (`held`); the face if it is a point, and then `from` is not
/// read; else `from`.
fn side(m: &Map<String, Value>, pk: &str, held: bool) -> Result<(Option<Value>, Option<Value>), String> {
    if held {
        return Ok((None, None));
    }
    let fk = format!("{pk}_face");
    match m.get(&fk) {
        None | Some(Value::Null) => {}
        Some(v) if params::vec3(v).is_some() => return Ok((None, Some(v.clone()))),
        Some(_) => return Err(whole(format!("`{fk}` must be a point [x, y, z] of numbers"))),
    }
    let v = m.get(pk);
    match crate::cmd::side_point(v, pk, &fk).map_err(whole)? {
        SidePoint::Snap { .. } => {
            // The handler reads a `snap` that is not text as a vertex; the word is its to look up.
            if v.and_then(|o| o.get("snap")).is_some_and(|s| !s.is_null() && !s.is_string()) {
                return Err(whole(format!("`{pk}.snap` must be {}", SNAPS.join(", "))));
            }
        }
        SidePoint::Origin | SidePoint::Point(_) => {}
    }
    Ok((v.cloned(), None))
}

impl<'de> Deserialize<'de> for AlignParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = Map::<String, Value>::deserialize(d)?;
        let fail = D::Error::custom;
        // In the handler's order: the bodies, the planes, the frames, the two sides' points, the
        // angle and the axes; then `add_feature`'s keys.
        let bodies = bodies(&m).map_err(fail)?;
        let (from_plane, to_plane) = (plane(&m, "from_plane").map_err(fail)?, plane(&m, "to_plane").map_err(fail)?);
        let (from_frame, to_frame) = (frame(&m, "from").map_err(fail)?, frame(&m, "to").map_err(fail)?);
        let (from, from_face) = side(&m, "from", from_plane.is_some() || from_frame.is_some()).map_err(fail)?;
        let (to, to_face) = side(&m, "to", to_plane.is_some() || to_frame.is_some()).map_err(fail)?;
        let angle = match m.get("angle") {
            None | Some(Value::Null) => None,
            Some(v) => Some(AngleExpr::deserialize(v).map_err(|e| fail(whole(format!("`angle` {e}"))))?),
        };
        let axes = match m.get("axes") {
            None | Some(Value::Null) => None,
            Some(v) => Some(axes_mask(v).map_err(|why| fail(whole(why)))?),
        };
        let flip = match m.get("flip") {
            None | Some(Value::Null) => None,
            Some(Value::Bool(b)) => Some(*b),
            Some(_) => return Err(fail(whole("`flip` must be true or false"))),
        };
        // `add_feature` reads `body_names` and `participants` as `string_list` does, whose messages
        // are `NameList`'s, not `FeatureCommon`'s; they are read here, and the rest go to it.
        let mut m = m;
        let mut lists = Vec::new();
        for key in ["participants", "body_names"] {
            if let Some(v) = m.remove(key).filter(|v| !v.is_null()) {
                lists.push(StringList(NameList::deserialize(v).map_err(|e| fail(whole(format!("`{key}` {e}"))))?.0));
            } else {
                lists.push(StringList::default());
            }
        }
        let mut common = feature_common::<_, FeatureCommon>(Value::Object(m)).map_err(|e| fail(e.to_string()))?;
        let (body_names, participants) = (lists.pop().filter(|l| !l.0.is_empty()), lists.pop().filter(|l| !l.0.is_empty()));
        (common.body_names, common.participants) = (body_names, participants);
        Ok(AlignParams { bodies: Some(bodies), from, from_face, from_plane, from_frame, to, to_face, to_plane, to_frame, flip, axes, angle, common })
    }
}

#[cfg(test)]
#[path = "align_tests.rs"]
mod tests;
