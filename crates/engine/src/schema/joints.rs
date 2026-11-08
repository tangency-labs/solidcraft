//! Contracts and result types for the commands in `cmd/joints.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::joints::cam::MAX_PROFILE_EDGES;
use solidcraft_doc::joints::{Coupling, JointExprs, JointKind, Snap};
use solidcraft_geom::Vec3Geom;

use super::check::whole;
use super::motion::require;
use super::types::{AngleExpr, IdOrName, LengthExpr, present};
use crate::cmd::joints::{MAX_FACE_NAME, MAX_REACH_JOINTS};
use crate::params;

/// A joint origin as joint.create takes it: a joint origin's name, or an object with the snap
/// (`occurrence`, then `face`, `circle`, `edge` or `point`, or a `frame`). Kept as given, for the
/// handler, which reads it as it reads joint.create's sides.
#[derive(Clone, Debug, PartialEq)]
pub struct JointOriginParam(pub Value);

impl<'de> Deserialize<'de> for JointOriginParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            v @ (Value::String(_) | Value::Object(_)) => Ok(JointOriginParam(v)),
            _ => Err(D::Error::custom("must be a joint origin's name or an object")),
        }
    }
}

impl JsonSchema for JointOriginParam {
    fn schema_name() -> Cow<'static, str> {
        "JointOrigin".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A joint origin, as joint.create takes a side: a joint origin's name, or an object with occurrence and face, circle, edge or point (world coordinates, or a face's name), with z?, x?, end?, on?, corner?, or with a frame {origin, z, x?}.",
            "anyOf": [
                {"type": "string", "minLength": 1},
                {"type": "object"}
            ]
        })
    }
}

/// Edit a joint in place. The sides `a` and `b` take a joint origin as joint.create does; a side
/// left out keeps its place, and the two must stay on different occurrences. The joint re-solves
/// and keeps its limits, its drive and the expressions it follows; a relation's values start
/// again where its parts are when its sides, type, offset, angle or flip change.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EditJointParams {
    /// The joint, by id or name.
    pub joint: IdOrName,
    /// Side a: a joint origin, as joint.create takes it. Default: a keeps its place.
    #[serde(default, deserialize_with = "present")]
    pub a: Option<JointOriginParam>,
    /// Side b: a joint origin, as joint.create takes it. Default: b keeps its place.
    #[serde(default, deserialize_with = "present")]
    pub b: Option<JointOriginParam>,
    /// The joint's type, as joint.create takes it (rigid, revolute, slider, ...).
    #[serde(default, rename = "type", deserialize_with = "present")]
    pub kind: Option<String>,
    /// Whether b's z lies along a's instead of against it, as joint.create's `flip`.
    #[serde(default, deserialize_with = "present")]
    pub flip: Option<bool>,
    /// The distance from a's frame to b's along z: a length, or an expression the joint follows.
    #[serde(default, deserialize_with = "present")]
    pub offset: Option<LengthExpr>,
    /// The turn about that distance: an angle, or an expression the joint follows.
    #[serde(default, deserialize_with = "present")]
    pub angle: Option<AngleExpr>,
    /// Suppress the joint (true) or bring it back (false).
    #[serde(default, deserialize_with = "present")]
    pub suppressed: Option<bool>,
    /// The joint's new name.
    #[serde(default, deserialize_with = "present")]
    pub name: Option<String>,
    /// Leave out the degrees of freedom and the fixed occurrences in the reply.
    #[serde(default, deserialize_with = "present")]
    pub quiet: Option<bool>,
}

/// A joint the solve reports: its id, name and why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointProblem {
    /// The joint, by id.
    pub joint: u64,
    /// The joint's name.
    pub name: String,
    /// Why it conflicts or could not be applied.
    pub why: String,
}

/// The degrees of freedom one occurrence keeps (6 free, 0 fixed).
#[derive(Debug, Serialize, JsonSchema)]
pub struct OccurrenceDof {
    /// The occurrence, by id (0 is the root).
    pub occurrence: u64,
    /// Its remaining degrees of freedom.
    pub dof: usize,
}

/// What `joint.edit` returns: the joints that disagree with the solved placement, and those the
/// solve could not apply. Unless `quiet`, also each occurrence's degrees of freedom and the
/// occurrences fixed in place.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EditJointResult {
    pub conflicts: Vec<JointProblem>,
    pub errors: Vec<JointProblem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dof: Option<Vec<OccurrenceDof>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<Vec<u64>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Show only the joints that attach one placement, with every body outside it faded. This is a
/// view: it is not saved and is not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FocusJointsParams {
    /// The placement, as occurrence ids from the top of the design. Default: leave the view.
    #[serde(default)]
    pub path: Option<Vec<u64>>,
}

/// What `joint.focus` returns: the joints shown and the bodies left solid (both empty when the
/// view is left).
#[derive(Debug, Serialize, JsonSchema)]
pub struct FocusJointsResult {
    pub joints: Vec<u64>,
    pub bodies: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// joint.create, joint.as_built, joint.drive, joint.limits, joint.delete, joint.list,
// joint.inspect and joint.solve (#12503).
//
// The handlers read their parameters from the raw JSON, so each key here reads as its handler
// reads it and fails with the handler's message. Whether a joint, an occurrence, a joint origin
// or a face exists stays the handler's to check. A key the handler refuses when it is left out
// (`type`, `a`, `b`, `joint`) is optional in the type and required in the schema, so that the
// handler's message, not the check's "is required", is what a caller sees. A handler that reads
// a key only in some cases (`value` when `values` is a list, `joint` when `origin` or `link` is
// given) has its contract read it in the same cases, so a call that works today gets no warning.
// ---------------------------------------------------------------------------------------------

/// A joint by its id (a number) or its name, as `joint_id` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct JointKey(pub String);

impl JointKey {
    fn read(v: &Value) -> Result<Self, String> {
        match v {
            Value::Number(n) => Ok(JointKey(n.to_string())),
            Value::String(s) => Ok(JointKey(s.clone())),
            _ => Err("`joint` must be an id or a name".into()),
        }
    }
}

impl JsonSchema for JointKey {
    fn schema_name() -> Cow<'static, str> {
        "JointKey".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A joint's id (a number) or its name.", "type": ["number", "string"]})
    }
}

/// An occurrence by its id (a number) or its name, as `occurrence_id` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct OccurrenceKey(pub String);

impl OccurrenceKey {
    fn read(v: &Value) -> Result<Self, String> {
        match v {
            Value::Number(n) => Ok(OccurrenceKey(n.to_string())),
            Value::String(s) => Ok(OccurrenceKey(s.clone())),
            _ => Err("an occurrence must be an id or a name".into()),
        }
    }
}

impl JsonSchema for OccurrenceKey {
    fn schema_name() -> Cow<'static, str> {
        "OccurrenceKey".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An occurrence by id or name (0 or `root`: the root), or the name of a component: its first occurrence.",
            "type": ["number", "string"]
        })
    }
}

/// A joint's type, as `kind_param` reads it: ignoring case, spaces, `_` and `-`, with the aliases
/// the engine takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointTypeParam(pub JointKind);

const TYPE_MESSAGE: &str = "`type` must be rigid, revolute, slider, cylindrical, pin_slot, planar, ball, parallel, perpendicular, angle, point_on_plane, point_on_line or line_on_plane";

/// The spellings `JointKind::parse` takes, canonical ones first.
const JOINT_TYPES: &[&str] = &[
    "rigid",
    "revolute",
    "slider",
    "cylindrical",
    "pin_slot",
    "planar",
    "ball",
    "parallel",
    "perpendicular",
    "angle",
    "point_on_plane",
    "point_on_line",
    "line_on_plane",
    "rotate",
    "slide",
    "square",
    "point_on_face",
    "point_on_axis",
    "line_on_face",
];

impl JointTypeParam {
    fn read(v: &Value) -> Result<Self, String> {
        v.as_str().and_then(JointKind::parse).map(JointTypeParam).ok_or_else(|| TYPE_MESSAGE.to_string())
    }
}

impl JsonSchema for JointTypeParam {
    fn schema_name() -> Cow<'static, str> {
        "JointType".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A joint's type: rigid, revolute (turns), slider (slides), cylindrical (both), pin_slot, planar or ball; or a relation that ties only some directions or points, made where the parts are: parallel, perpendicular, angle, point_on_plane, point_on_line or line_on_plane. Case, spaces, `_` and `-` are ignored, and rotate, slide, square, point_on_face, point_on_axis and line_on_face are also taken.",
            "type": "string",
            "examples": JOINT_TYPES
        })
    }
}

/// A joint value: a number (degrees for an angle, mm for a distance), or an expression, as
/// `joints::value` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct JointValue(pub Value);

impl JointValue {
    fn check(v: &Value) -> Result<(), String> {
        match v {
            Value::Number(n) if n.as_f64().is_some_and(|x| x.is_finite() && x.abs() < 1e9) => Ok(()),
            Value::String(_) => Ok(()),
            _ => Err(format!("a joint value {}", params::expr_problem(Some(v)))),
        }
    }
    fn read(v: &Value) -> Result<Self, String> {
        Self::check(v).map(|()| JointValue(v.clone()))
    }
}

impl JsonSchema for JointValue {
    fn schema_name() -> Cow<'static, str> {
        "JointValue".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A number (degrees for an angle, mm for a distance; below 1e9 in size), or an expression (text) over the design's parameters, such as \"width / 2\" or \"30 deg\".",
            "anyOf": [
                {"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9},
                {"type": "string"}
            ]
        })
    }
}

/// The values of a joint, one per coordinate in the order `joint.list` names them.
#[derive(Clone, Debug, PartialEq)]
pub struct JointValues(pub Vec<Value>);

impl JointValues {
    fn read(v: &Value) -> Result<Self, String> {
        match v {
            Value::Array(items) => items.iter().try_for_each(JointValue::check).map(|()| JointValues(items.clone())),
            _ => Err("`values` must be a list of joint values".into()),
        }
    }
}

impl JsonSchema for JointValues {
    fn schema_name() -> Cow<'static, str> {
        "JointValues".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item = generator.subschema_for::<JointValue>();
        json_schema!({
            "description": "A list of values, one for each of the joint's coordinates in order (the angle first for a cylindrical joint). Angles in degrees, distances in mm, or expressions.",
            "type": "array",
            "items": item
        })
    }
}

/// A joint's limits: one `[min, max]` pair for each value, `null` for a value with no limit.
#[derive(Clone, Debug, PartialEq)]
pub struct JointLimits(pub Vec<Value>);

impl JointLimits {
    fn read(v: &Value) -> Result<Self, String> {
        let Value::Array(items) = v else { return Err("`limits` must be a list of [min, max] pairs, with null for a value without limits".into()) };
        for item in items {
            match item {
                Value::Null => {}
                // A list of fewer than two is left out by the handler; of two or more, the first two
                // are read.
                Value::Array(pair) if pair.len() >= 2 => pair.iter().take(2).try_for_each(JointValue::check)?,
                Value::Array(_) => {}
                _ => return Err("`limits` must be a list of [min, max] pairs, with null for a value without limits".into()),
            }
        }
        Ok(JointLimits(items.clone()))
    }
}

impl JsonSchema for JointLimits {
    fn schema_name() -> Cow<'static, str> {
        "JointLimits".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let value = generator.subschema_for::<JointValue>();
        json_schema!({
            "description": "The limits of each value in order: [min, max] (degrees or mm, or expressions), or null for a value without limits. Not more pairs than the joint has values.",
            "type": "array",
            "items": {
                "anyOf": [
                    {"type": "null"},
                    {"type": "array", "prefixItems": [value, value]}
                ]
            }
        })
    }
}

/// A joint origin as `joint.create` reads a side: a joint origin's name, or an object that says
/// where (`face`, `circle`, `edge` or `point`: a point in world coordinates, or for the first two
/// a face's name; or a `frame`). Kept as given, for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct JointOriginArg(pub Value);

impl JointOriginArg {
    fn read(v: &Value) -> Result<Self, String> {
        if v.is_string() {
            return Ok(JointOriginArg(v.clone()));
        }
        let named = ["face", "circle"].iter().any(|k| v.get(*k).and_then(Value::as_str).is_some());
        let picked = ["face", "circle", "edge", "point"].iter().any(|k| v.get(*k).and_then(params::vec3).is_some());
        if named || picked {
            return Ok(JointOriginArg(v.clone()));
        }
        match v.get("frame") {
            Some(f) if f.get("origin").and_then(params::vec3).is_some() => Ok(JointOriginArg(v.clone())),
            Some(_) => Err("`frame` needs origin and z".into()),
            None => Err("a joint origin needs face, circle, edge, point or frame [x, y, z], or a face's name".into()),
        }
    }
}

impl JsonSchema for JointOriginArg {
    fn schema_name() -> Cow<'static, str> {
        "JointOriginArg".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // `params::vec3` reads [x, y, z] and {x, y, z}.
        let forms = [
            json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}),
            json!({"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "z": {"type": "number"}}, "required": ["x", "y", "z"]}),
        ];
        let point = json!({"anyOf": forms});
        let at = |description: &str| json!({"description": description, "anyOf": forms});
        json_schema!({
            "description": "A joint origin: the name of a joint origin made with joint.origin, or an object that says where it is, in world coordinates. Give one of face, circle, edge or point (a point [x, y, z]; for face and circle also a face's name, as model.faces lists it), or a frame {origin, z, x?}.",
            "anyOf": [
                {"type": "string", "description": "The name of a joint origin."},
                {
                    "type": "object",
                    "properties": {
                        "occurrence": {"description": "The occurrence the origin is on, by id or name. Default: found from the point, or the named face's component.", "type": ["number", "string"]},
                        "face": {"description": "The centre of a planar face: a point on it [x, y, z], or its name.", "anyOf": [point, {"type": "string"}]},
                        "circle": {"description": "The centre of a round end: a point [x, y, z] on its wall, rim or centre, or the round face's name.", "anyOf": [point, {"type": "string"}]},
                        "edge": at("A point [x, y, z] on an edge of a planar face."),
                        "point": at("A point [x, y, z]."),
                        "frame": {"description": "An explicit frame {origin: [x, y, z], z: [x, y, z], x?: [x, y, z]}.", "type": "object"},
                        "z": at("The frame's z axis, a direction [x, y, z], where it is not the face's normal."),
                        "x": {"description": "The frame's turn about z: a direction [x, y, z], a face's name or {face: [x, y, z]}.", "anyOf": [point, {"type": "string"}, {"type": "object"}]},
                        "end": {"description": "A circle's end: the name of the face across its rim there.", "type": "string"},
                        "on": at("An edge origin: a point on the face to use."),
                        "corner": at("An edge origin: a point near one end of the edge, for the corner there."),
                        "body": {"description": "The body a face name is on.", "type": "string"}
                    }
                }
            ]
        })
    }
}

macro_rules! read_with {
    ($($t:ty),*) => {$(
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::read(&Value::deserialize(d)?).map_err(|m| D::Error::custom(whole(m)))
            }
        }
    )*};
}

read_with!(JointKey, OccurrenceKey, JointTypeParam, JointValue, JointValues, JointLimits, JointOriginArg);

/// The call's keys as an object. (`TypedContract::check` has refused anything else already.)
fn keys<'de, D: Deserializer<'de>>(d: D) -> Result<Map<String, Value>, D::Error> {
    Map::<String, Value>::deserialize(d)
}

/// `key`, read by `read` when it is given (`null` included).
fn given<T, E: serde::de::Error>(m: &Map<String, Value>, key: &str, read: fn(&Value) -> Result<T, String>) -> Result<Option<T>, E> {
    m.get(key).map(|v| read(v).map_err(|e| E::custom(whole(e)))).transpose()
}

/// `key` as the handlers' `bool_` reads it, but refusing a value that is not a boolean, which the
/// handlers read as if the key were left out. `null` is left out.
fn flag<E: serde::de::Error>(m: &Map<String, Value>, key: &str) -> Result<Option<bool>, E> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(E::custom(whole(format!("`{key}` must be true or false")))),
    }
}

/// `key` as the handlers' `str_` reads it, but refusing a value that is not text.
fn text<E: serde::de::Error>(m: &Map<String, Value>, key: &str) -> Result<Option<String>, E> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(E::custom(whole(format!("`{key}` must be text")))),
    }
}

/// `key` as the handlers' `num` reads it, but refusing a value that is not a number.
fn number<E: serde::de::Error>(m: &Map<String, Value>, key: &str) -> Result<Option<f64>, E> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_f64()),
        Some(_) => Err(E::custom(whole(format!("`{key}` must be a number")))),
    }
}

/// `key` when it is not `null`, read by `read` (a list the handlers ignore unless it is one).
fn listed<T, E: serde::de::Error>(m: &Map<String, Value>, key: &str, read: fn(&Value) -> Result<T, String>) -> Result<Option<T>, E> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => read(v).map(Some).map_err(|e| E::custom(whole(e))),
    }
}

/// Add a joint between two occurrences, mated at two joint origins, and re-solve. A relation type
/// (parallel, perpendicular, angle, point_on_plane, point_on_line, line_on_plane) ties only some
/// directions or points and is made where the parts are. An expression given for `offset`,
/// `angle` or a value is kept with the joint and worked out again whenever the design changes. The
/// joint starts at its values inside its limits, and a part moved to a limit is said so in
/// `warnings`. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = create_required)]
pub struct CreateJointParams {
    // `type`, `a` and `b` are optional in the type and required in the schema
    // (`create_required`): left out, the handler refuses them with its own message.
    /// The joint's type.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointTypeParam")]
    pub kind: Option<JointTypeParam>,
    /// Side a: a joint origin, as an object in world coordinates or a joint origin's name. The
    /// two sides must be on different occurrences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointOriginArg")]
    pub a: Option<JointOriginArg>,
    /// Side b: a joint origin, as `a`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointOriginArg")]
    pub b: Option<JointOriginArg>,
    /// The joint's values to start at: one per coordinate (angles in degrees, distances in mm, or
    /// expressions the joint then follows). Default: 0 for each, or for a relation or a joint that
    /// closes a loop, where the parts are. Wins over `value`.
    #[serde(default)]
    pub values: Option<JointValues>,
    /// One value, for the joint's first coordinate, when `values` is not a list. Ignored (and not
    /// checked) when `values` is a list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub value: Option<JointValue>,
    /// The limits of each value: [min, max] pairs, with null for a value without limits. Default:
    /// none.
    #[serde(default)]
    pub limits: Option<JointLimits>,
    /// Whether b's z lies along a's instead of against it. Default: false (the origins are mated,
    /// b's z against a's as two faces meet); true keeps two origins given at one point with the
    /// same z and x as they are. A point origin without x takes its x from z, so give x on both,
    /// or use joint.as_built, to keep a turned part where it is.
    #[serde(default)]
    pub flip: Option<bool>,
    /// The distance from a's frame to b's along z, in mm or an expression. Default: 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub offset: Option<JointValue>,
    /// The turn about that distance, in degrees or an expression; for an angle relation, its tilt
    /// from mated. Default: none, or a half turn where that turns b less, so faces that already
    /// meet stay as they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub angle: Option<JointValue>,
    /// The joint's name. Default: its type, `Joint` and the first number no joint's name has.
    /// Blank text gives the default.
    #[serde(default)]
    pub name: Option<String>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

fn create_required(schema: &mut Schema) {
    require(schema, "type");
    require(schema, "a");
    require(schema, "b");
}

impl<'de> Deserialize<'de> for CreateJointParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        // In the handler's order: the type, the sides, then the numbers.
        let kind = given(&m, "type", JointTypeParam::read)?;
        let a = given(&m, "a", JointOriginArg::read)?;
        let b = given(&m, "b", JointOriginArg::read)?;
        let offset = given(&m, "offset", JointValue::read)?;
        let angle = given(&m, "angle", JointValue::read)?;
        let values = listed(&m, "values", JointValues::read)?;
        // `value` is read only when `values` is not a list.
        let value = if values.is_some() { None } else { given(&m, "value", JointValue::read)? };
        let limits = listed(&m, "limits", JointLimits::read)?;
        Ok(CreateJointParams {
            kind,
            a,
            b,
            values,
            value,
            limits,
            flip: flag(&m, "flip")?,
            offset,
            angle,
            name: text(&m, "name")?,
            quiet: flag(&m, "quiet")?,
        })
    }
}

/// Add a joint between two occurrences that stay where they are: its frame is a joint origin
/// (`at`, default b's origin), and the joint starts with the origins coinciding as they are. An
/// origin on a or b is kept on its face or hole there, so the joint follows it as the design
/// changes. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = as_built_required)]
pub struct AsBuiltJointParams {
    // `type`, `a` and `b` are optional in the type and required in the schema
    // (`as_built_required`), as in `CreateJointParams`.
    /// The joint's type, as joint.create takes it.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointTypeParam")]
    pub kind: Option<JointTypeParam>,
    /// Occurrence a, by id or name; it stays where it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrenceKey")]
    pub a: Option<OccurrenceKey>,
    /// Occurrence b, by id or name; it stays where it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrenceKey")]
    pub b: Option<OccurrenceKey>,
    /// The joint's frame: a joint origin, as joint.create takes one. Default: b's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointOriginArg")]
    pub at: Option<JointOriginArg>,
    /// The joint's name. Default: its type, `Joint` and the first number no joint's name has.
    /// Blank text gives the default.
    #[serde(default)]
    pub name: Option<String>,
    /// The limits of each value: [min, max] pairs, with null for a value without limits. Default:
    /// none.
    #[serde(default)]
    pub limits: Option<JointLimits>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
    /// Not a value of this joint, which starts at zero: only whether `values` is given matters.
    /// Given, a relation type is not started from where its parts are.
    #[serde(default)]
    pub values: Option<Value>,
    /// Like `values`: only whether it is given matters.
    #[serde(default)]
    pub value: Option<Value>,
}

fn as_built_required(schema: &mut Schema) {
    require(schema, "type");
    require(schema, "a");
    require(schema, "b");
}

impl<'de> Deserialize<'de> for AsBuiltJointParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        Ok(AsBuiltJointParams {
            kind: given(&m, "type", JointTypeParam::read)?,
            a: given(&m, "a", OccurrenceKey::read)?,
            b: given(&m, "b", OccurrenceKey::read)?,
            at: given(&m, "at", JointOriginArg::read)?,
            limits: listed(&m, "limits", JointLimits::read)?,
            name: text(&m, "name")?,
            quiet: flag(&m, "quiet")?,
            values: m.get("values").cloned(),
            value: m.get("value").cloned(),
        })
    }
}

/// Set a joint's values by hand, as far as its limits, contact sets and closed loops allow; the
/// other joints of a closed loop follow, and the motion stops where the loop can no longer close.
/// A value driven drops the expression joint.create gave it. Give `value` or `values`, or `index`
/// with `value` to drive one coordinate while the others follow.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = drive_required)]
pub struct DriveJointParams {
    // `joint` is optional in the type and required in the schema (`drive_required`): left out,
    // the handler refuses it with its own message.
    /// The joint, by id or name. A rigid joint does not move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub joint: Option<JointKey>,
    /// The values to drive to: one per coordinate (angles in degrees, distances in mm, or
    /// expressions worked out once). A value not given keeps its own. Not with `index`.
    #[serde(default)]
    pub values: Option<JointValues>,
    /// One value: the joint's first coordinate, or with `index` the one that `index` names. Used
    /// when `values` is not a list; ignored (and not checked) when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub value: Option<JointValue>,
    /// Drive only this coordinate, counted from zero (0 for a revolute joint's angle); the others
    /// follow. Goes with `value`, not `values`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub index: Option<u64>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
    /// A frame of a drag: the reply says `continuous`, and the app's command line doesn't echo it.
    /// Default: false.
    #[serde(default)]
    pub continuous: Option<bool>,
}

fn drive_required(schema: &mut Schema) {
    require(schema, "joint");
    // The handler refuses a call with neither (`give value or values`), and one with `index` and
    // `values`.
    schema.insert("anyOf".into(), json!([{"required": ["value"]}, {"required": ["values"]}]));
    schema.insert("not".into(), json!({"required": ["index", "values"]}));
}

impl<'de> Deserialize<'de> for DriveJointParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        let joint = given(&m, "joint", JointKey::read)?;
        // `index`, once given (`null` included), must name a coordinate of the joint.
        let index = match m.get("index") {
            None => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| D::Error::custom(whole("index is outside this joint's coordinates")))?),
        };
        let (values, value) = if index.is_some() {
            // One coordinate: `values` is refused, whatever it holds, and `value` is read.
            if m.contains_key("values") {
                return Err(D::Error::custom(whole("index goes with value, not values")));
            }
            (None, given(&m, "value", JointValue::read)?)
        } else {
            let values = listed(&m, "values", JointValues::read)?;
            // `value` is read only when `values` is not a list.
            let value = if values.is_some() { None } else { given(&m, "value", JointValue::read)? };
            (values, value)
        };
        Ok(DriveJointParams { joint, values, value, index, quiet: flag(&m, "quiet")?, continuous: flag(&m, "continuous")? })
    }
}

/// Set one of a joint's value limits, or clear it. A value already past the new limit moves to
/// it. Give `min` and `max` together, or neither to clear.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = limits_required)]
pub struct JointLimitsParams {
    // `joint` is optional in the type and required in the schema, as in `DriveJointParams`.
    /// The joint, by id or name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub joint: Option<JointKey>,
    /// Which value, counted from zero (0 for a revolute joint's angle); a negative number is 0.
    /// Default: 0.
    #[serde(default)]
    pub index: Option<f64>,
    /// The lowest the value goes, in degrees or mm, or an expression. With `max`; leave both out
    /// to clear the limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub min: Option<JointValue>,
    /// The highest the value goes, as `min`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub max: Option<JointValue>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

fn limits_required(schema: &mut Schema) {
    require(schema, "joint");
    schema.insert("dependentRequired".into(), json!({"min": ["max"], "max": ["min"]}));
}

impl<'de> Deserialize<'de> for JointLimitsParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        let joint = given(&m, "joint", JointKey::read)?;
        let index = number(&m, "index")?;
        // Both or neither, `null` counted as given; the minimum is read first.
        let (min, max) = match (m.get("min"), m.get("max")) {
            (Some(lo), Some(hi)) => {
                let lo = JointValue::read(lo).map_err(|e| D::Error::custom(whole(e)))?;
                (Some(lo), Some(JointValue::read(hi).map_err(|e| D::Error::custom(whole(e)))?))
            }
            (None, None) => (None, None),
            _ => return Err(D::Error::custom(whole("give both `min` and `max`, or neither to clear"))),
        };
        Ok(JointLimitsParams { joint, index, min, max, quiet: flag(&m, "quiet")? })
    }
}

/// Delete a joint (and the motion links it is in), a joint origin or a motion link. Give
/// `origin` to delete a joint origin, or else `link` to delete a motion link, or else `joint`.
/// With `origin` or `link` the other keys are not read. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = delete_required)]
pub struct DeleteJointParams {
    /// The joint origin to delete, by name. Wins over `link` and `joint`.
    #[serde(default)]
    pub origin: Option<String>,
    /// The motion link to delete, by its index in `joint.list`'s `links` (a negative number is
    /// 0). Wins over `joint`; not read when `origin` is given.
    #[serde(default)]
    pub link: Option<f64>,
    /// The joint to delete, by id or name. Read only without `origin` and `link`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub joint: Option<JointKey>,
    /// Leave out `dof` and `fixed` in the reply. Read only when a joint is deleted. Default:
    /// false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

fn delete_required(schema: &mut Schema) {
    // The handler refuses a call with none of them (`joint must be an id or a name`).
    schema.insert("anyOf".into(), json!([{"required": ["origin"]}, {"required": ["link"]}, {"required": ["joint"]}]));
}

impl<'de> Deserialize<'de> for DeleteJointParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        let unread = |origin, link| DeleteJointParams { origin, link, joint: None, quiet: None };
        // The modes, in the handler's order: a joint origin's name, a link's number, a joint.
        match m.get("origin") {
            Some(Value::String(name)) => return Ok(unread(Some(name.clone()), None)),
            None | Some(Value::Null) => {}
            Some(_) => return Err(D::Error::custom(whole("`origin` must be the name of a joint origin"))),
        }
        match m.get("link") {
            Some(Value::Number(n)) if n.as_f64().and_then(params::finite).is_some() => return Ok(unread(None, n.as_f64())),
            None | Some(Value::Null) | Some(Value::Number(_)) => {}
            Some(_) => return Err(D::Error::custom(whole("`link` must be a motion link's index, a number"))),
        }
        Ok(DeleteJointParams { origin: None, link: None, joint: given(&m, "joint", JointKey::read)?, quiet: flag(&m, "quiet")? })
    }
}

/// What every joint command that moves parts reports: the joints that disagree with the solved
/// placement, and those it could not apply.
/// Unless `quiet`, also each occurrence's degrees of freedom and the occurrences fixed in place.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SolveReport {
    /// The joints that disagree with the placement found through others.
    pub conflicts: Vec<JointProblem>,
    /// The joints that could not be applied (a lost origin, an expression that no longer works).
    pub errors: Vec<JointProblem>,
    /// Each occurrence's remaining degrees of freedom (6 free, 0 fixed). Left out with `quiet`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dof: Option<Vec<OccurrenceDof>>,
    /// The occurrences fixed in place (grounded, or the root), by id. Left out with `quiet`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<Vec<u64>>,
}

/// What `joint.limits` and `joint.solve` return: the solve.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SolveResult {
    #[serde(flatten)]
    pub solved: SolveReport,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `joint.create` and `joint.as_built` return: the solve after the joint is added.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointAddedResult {
    #[serde(flatten)]
    pub solved: SolveReport,
    /// The new joint, by id.
    pub joint: u64,
    /// Its name.
    pub name: String,
    /// A value that starts inside its limits, not where it was given: the part moved there as it
    /// was joined. Left out when there is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The unit of a joint value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ValueUnit {
    /// Degrees: an angle.
    Deg,
    /// Millimetres: a distance.
    Mm,
}

/// One of a joint's values, as `joint.list` gives it (rounded for reading).
#[derive(Debug, Serialize, JsonSchema)]
pub struct NamedValue {
    /// The coordinate's name (angle, distance, slide, …).
    pub name: String,
    /// Its value, in `unit`.
    pub value: f64,
    /// `deg` for an angle, `mm` for a distance.
    pub unit: ValueUnit,
    /// Its limits [min, max] in `unit`, or `null` for none.
    pub limits: Option<[f64; 2]>,
}

/// A joint's type as the replies name it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JointKindName {
    Rigid,
    Revolute,
    Slider,
    Cylindrical,
    PinSlot,
    Planar,
    Ball,
    Parallel,
    Perpendicular,
    Angle,
    PointOnPlane,
    PointOnLine,
    LineOnPlane,
}

/// The expressions a joint's numbers follow, over the design's parameters.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointExpressions {
    /// The offset's expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    /// The angle's expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// The expression of each value by index, `null` for a value that is a number.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Option<String>>,
}

/// A joint origin's snap as saved, in the component's coordinates.
pub struct SnapShape;

impl JsonSchema for SnapShape {
    fn schema_name() -> Cow<'static, str> {
        "Snap".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let point = json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3});
        json_schema!({
            "description": "Where a joint origin sits on its component's geometry, as saved: `type` is frame, face_center, circle_center, edge or point, with the keys that type holds: origin, pick, point, z or x as [x, y, z] (x may also be a face reference), face, end, edge and corner as persistent names, and for an edge origin its axis.",
            "type": "object",
            "properties": {
                "type": {"enum": ["frame", "face_center", "circle_center", "edge", "point"]},
                "origin": point, "pick": point, "point": point, "z": point,
                "x": {"description": "A direction [x, y, z], or {face, pick?}: a face the turn is taken from."},
                "face": {"type": "string"}, "end": {"type": "string"}, "edge": {"type": "string"}, "corner": {"type": "string"},
                "axis": {"type": "object"}
            },
            "required": ["type"]
        })
    }
}

/// A motion link's coupling: the geometry its ratio comes from.
pub struct CouplingShape;

impl JsonSchema for CouplingShape {
    fn schema_name() -> Cow<'static, str> {
        "Coupling".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "What the link's ratio is worked out from: `type` is gear (ra, rb, internal?, reverse?), rack_pinion (radius, reverse?), screw (pitch, reverse?) or cam (profile, follower, tip?, reverse?), with the keys that type holds.",
            "type": "object",
            "properties": {"type": {"enum": ["gear", "rack_pinion", "screw", "cam"]}},
            "required": ["type"]
        })
    }
}

/// A cam's follower travel over one turn of the cam.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamLift {
    /// The follower's lowest value, in `unit`.
    pub min: f64,
    /// Its highest value.
    pub max: f64,
    /// `max` less `min`.
    pub rise: f64,
    /// `deg` for an oscillating follower, `mm` for a translating one.
    pub unit: ValueUnit,
}

/// A motion link, as `joint.list` and `joint.inspect` give it: b's value follows a's.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkInfo {
    /// The driving joint, by id.
    pub a: u64,
    /// The driving value's index.
    pub ia: usize,
    /// The driven joint, by id.
    pub b: u64,
    /// The driven value's index.
    pub ib: usize,
    /// b's units per a's, per degree: b = ratio · a + offset. A cam has a `lift` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
    /// In b's units (`unit`).
    pub offset: f64,
    /// b's unit.
    pub unit: ValueUnit,
    /// What the ratio is worked out from, for a gear, a rack and pinion, a screw or a cam.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CouplingShape")]
    pub coupling: Option<Coupling>,
    /// Why the coupling can't be worked out now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// A cam's follower travel: `null` when its table is not whole (a hand-edited file). Only for
    /// a cam.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lift: Option<Option<CamLift>>,
}

/// A joint origin's frame in the world.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointFrame {
    /// The origin [x, y, z], in mm.
    pub origin: [f64; 3],
    /// The z axis [x, y, z].
    pub z: [f64; 3],
}

/// A joint as `joint.list` gives it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListedJoint {
    /// The joint, by id.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// Its type.
    #[serde(rename = "type")]
    #[schemars(with = "JointKindName")]
    pub kind: JointKind,
    /// Side a's occurrence, by id.
    pub a: u64,
    /// Side b's occurrence, by id.
    pub b: u64,
    /// Its values as the solver places them (after motion links), with their limits.
    pub values: Vec<NamedValue>,
    /// Whether b's z lies along a's.
    pub flip: bool,
    /// The distance from a's frame to b's along z, in mm.
    pub offset: f64,
    /// Whether the joint is suppressed (it places nothing).
    pub suppressed: bool,
    /// Whether the joint is shown in the viewport.
    pub visible: bool,
    /// Side a's frame in the world, `null` when its origin is lost.
    pub frame_a: Option<JointFrame>,
    /// Side b's frame in the world, `null` when its origin is lost.
    pub frame_b: Option<JointFrame>,
    /// The expressions the joint follows. Left out when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointExpressions")]
    pub expressions: Option<JointExprs>,
}

/// A named joint origin.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListedOrigin {
    /// Its name.
    pub name: String,
    /// The occurrence it is on, by id.
    pub occurrence: u64,
}

/// Joints joined to nothing grounded, which the solver places none of.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FloatingChain {
    /// The chain's occurrences, by name.
    pub occurrences: Vec<String>,
    /// Its joints, by name. Driving them moves nothing.
    pub joints: Vec<String>,
}

/// What `joint.list` returns: the solve, then every joint, motion link and joint origin.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListJointsResult {
    #[serde(flatten)]
    pub solved: SolveReport,
    /// Every joint.
    pub joints: Vec<ListedJoint>,
    /// Every motion link, by its index.
    pub links: Vec<LinkInfo>,
    /// Every named joint origin.
    pub origins: Vec<ListedOrigin>,
    /// Joint chains joined to nothing grounded.
    pub floating: Vec<FloatingChain>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// List the joints, motion links and joint origins, with the solve.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ListJointsParams {
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

/// Re-place the occurrences from the joints, and report conflicts, errors and degrees of freedom.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SolveJointsParams {
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

/// Say what a joint is defined by and attached to: its two origins (the snap as saved, and the
/// feature, face, edge or sketch curve each name refers to), whether each is found today and why
/// not, its type, offset, angle, flip, values, expressions and motion links. Reads only: nothing
/// changes.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct InspectJointParams {
    /// The joint, by id or name. Default: every joint.
    #[serde(default)]
    pub joint: Option<JointKey>,
}

/// What a face reference says about the feature that made the face.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MadeBy {
    /// The feature.
    Feature {
        /// Its id.
        id: u64,
        /// Its name.
        name: String,
        /// Its type.
        #[serde(rename = "type")]
        kind: String,
    },
    /// The feature is gone.
    Gone {
        /// The id the name refers to.
        id: u64,
        /// Why there is none.
        error: String,
    },
}

/// The sketch curve a face was swept from.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SweptFrom {
    /// The curve, in a sketch the feature takes.
    Curve {
        /// The sketch feature's id.
        id: u64,
        /// Its name.
        name: String,
        /// The curve's id in the sketch.
        curve: String,
        /// The curve's type (line, arc, circle, …).
        curve_type: Option<String>,
    },
    /// No sketch of the feature has the curve.
    Missing {
        /// The curve's id.
        curve: String,
        /// Why there is no such curve.
        error: String,
    },
}

/// What a face's persistent name refers to.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FaceReference {
    /// The persistent name, as saved.
    pub name: String,
    /// A name saved before exact names, so it may not be found again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unverified: Option<bool>,
    /// What the face is in the feature that made it (top, side:c1, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_in_feature: Option<String>,
    /// The feature that made the face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<MadeBy>,
    /// The sketch curve a side face was swept from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<SweptFrom>,
}

/// What an edge's persistent name refers to: its two faces.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EdgeReference {
    /// The persistent name, as saved.
    pub name: String,
    /// What each of the edge's faces refers to.
    pub faces: Vec<FaceReference>,
    /// A name saved before exact names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unverified: Option<bool>,
}

/// What a reference names.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Referent {
    /// A face.
    Face(FaceReference),
    /// An edge.
    Edge(EdgeReference),
}

/// What part of the snap a reference is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReferenceRole {
    /// The face the origin is on.
    Face,
    /// A round origin's end face.
    End,
    /// The face its x axis is taken from.
    X,
    /// The edge an edge origin is on.
    Edge,
    /// The edge that gives an edge origin's corner.
    Corner,
}

/// A persistent name a joint origin holds, with what it refers to.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Reference {
    /// The part of the snap that holds the name.
    pub role: ReferenceRole,
    #[serde(flatten)]
    pub to: Referent,
}

/// A joint origin's frame in the world.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WorldFrame {
    /// The origin [x, y, z], in mm.
    pub origin: [f64; 3],
    /// The z axis [x, y, z].
    pub z: [f64; 3],
    /// The x axis [x, y, z].
    pub x: [f64; 3],
}

/// One of a joint's origins.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointSide {
    /// The occurrence the origin is on, by id (0 is the root).
    pub occurrence: u64,
    /// Its name (`root` for the root).
    pub occurrence_name: String,
    /// The component it places, by id.
    pub component: u64,
    /// The component's name (`root` for the root).
    pub component_name: String,
    /// The occurrence names from the top of the assembly down, with `/` between.
    pub path: String,
    /// The snap as saved, in the component's coordinates.
    #[schemars(with = "SnapShape")]
    pub snap: Snap,
    /// The persistent names the snap holds.
    pub references: Vec<Reference>,
    /// Whether the origin is found now.
    pub resolves: bool,
    /// Where it is in the world. Left out when it is not found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<WorldFrame>,
    /// Why the origin is lost. Left out when it is found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Which end of a motion link a joint is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LinkRole {
    /// The joint drives the link.
    Driver,
    /// The joint is driven by it.
    Driven,
}

/// A motion link a joint takes part in.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointLink {
    /// The link's index.
    pub index: usize,
    /// Whether the joint drives it or is driven by it.
    pub role: LinkRole,
    #[serde(flatten)]
    pub link: LinkInfo,
}

/// A joint as `joint.inspect` gives it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InspectedJoint {
    /// The joint, by id.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// Its type.
    #[serde(rename = "type")]
    #[schemars(with = "JointKindName")]
    pub kind: JointKind,
    /// Whether the joint is suppressed.
    pub suppressed: bool,
    /// Whether the joint is shown in the viewport.
    pub visible: bool,
    /// Whether b's z lies along a's.
    pub flip: bool,
    /// The distance from a's frame to b's along z, in mm.
    pub offset: f64,
    /// The turn about that distance, in degrees.
    pub angle: f64,
    /// Its values as the solver places them, as joint.list gives them.
    pub values: Vec<NamedValue>,
    /// The names of its coordinates.
    pub dofs: Vec<String>,
    /// Side a.
    pub a: JointSide,
    /// Side b.
    pub b: JointSide,
    /// The motion links it takes part in.
    pub links: Vec<JointLink>,
    /// Why the solve could not apply it.
    pub errors: Vec<String>,
    /// Where it disagrees with the placement found through other joints.
    pub conflicts: Vec<String>,
    /// The expressions it follows. Left out when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointExpressions")]
    pub expressions: Option<JointExprs>,
}

/// What `joint.inspect` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InspectJointResult {
    /// The joint asked for, or every joint.
    pub joints: Vec<InspectedJoint>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One driven value that ended up elsewhere than asked.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HeldValue {
    /// The coordinate's name.
    pub name: String,
    /// The value asked for, in `unit`.
    pub requested: f64,
    /// The value it has, in `unit`.
    pub value: f64,
    /// `deg` or `mm`.
    pub unit: ValueUnit,
    /// Why: a motion link sets it, a limit holds it, contact or a closed loop stopped it.
    pub why: String,
}

/// Where contact stopped a drive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContactStop {
    /// One of the bodies that touch.
    pub a: String,
    /// The other.
    pub b: String,
    /// How far from the start to the asked-for values the drive got (0 to 1).
    pub fraction: f64,
}

/// Where a closed loop stopped a drive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoopStop {
    /// The joint whose loop opens first, by name.
    pub joint: String,
    /// How far from the start to the asked-for values the drive got (0 to 1).
    pub fraction: f64,
}

/// What `joint.drive` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DriveJointResult {
    #[serde(flatten)]
    pub solved: SolveReport,
    /// The joint driven, by id.
    pub joint: u64,
    /// Its values now, as joint.list gives them.
    pub values: Option<Vec<NamedValue>>,
    /// True for a `continuous` call; left out otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuous: Option<bool>,
    /// The bodies that touched, when contact stopped the drive short; `null` otherwise.
    pub stopped_by_contact: Option<ContactStop>,
    /// The closed loop that stopped the drive short; `null` otherwise.
    pub stopped_by_loop: Option<LoopStop>,
    /// Whether any occurrence moved.
    pub moved: bool,
    /// The values asked for, in the same form as `values`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<Vec<NamedValue>>,
    /// Each driven value that ended up elsewhere than asked, and why. Left out when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<Vec<HeldValue>>,
    /// Why the joint can move nothing: it is suppressed, its origins are lost, its chain joins
    /// nothing grounded, or, when nothing moved, the first held value's reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The occurrence joined to nothing grounded, when that is the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unconnected: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `joint.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeleteJointResult {
    #[serde(flatten)]
    pub deleted: JointDeleted,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What was deleted.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum JointDeleted {
    /// A joint origin (`origin`).
    Origin {
        /// The origin's name.
        deleted_origin: String,
        /// How many joints there are (the joints are not deleted with it).
        joints: usize,
    },
    /// A motion link (`link`).
    Link {
        /// The link's index.
        deleted_link: usize,
    },
    /// A joint, with the motion links it was in; the rest is re-solved.
    Joint {
        #[serde(flatten)]
        solved: SolveReport,
        /// The joint's id.
        deleted: u64,
    },
}

// ---------------------------------------------------------------------------------------------
// joint.origin, joint.rigid_group, joint.reach, joint.motion_link, joint.gear, joint.rack_pinion,
// joint.screw and joint.cam (#13185).
//
// As above: each key reads as its handler reads it and fails with the handler's message, and a key
// the handler refuses when it is left out is optional in the type and required in the schema. A
// key the handler reads only in some cases (a joint origin's `end`, `on` or `z`, a screw's `a` and
// `b`, a roller's `radius`) is read here in the same cases. Where the handler reads a key
// leniently (a value of the wrong type taken as if the key were left out) the type is stricter,
// which is what warn and strict report.
// ---------------------------------------------------------------------------------------------

/// A point [x, y, z] in mm, or {x, y, z}, as `params::vec3` reads it. Only its schema: the
/// parameter types read their points themselves, with each handler's message.
pub struct WorldPoint;

impl JsonSchema for WorldPoint {
    fn schema_name() -> Cow<'static, str> {
        "WorldPoint".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A point [x, y, z] in mm, or {x, y, z}, each number below 1e9 in size.",
            "anyOf": [
                {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "z": {"type": "number"}}, "required": ["x", "y", "z"]}
            ]
        })
    }
}

/// A direction [x, y, z] or {x, y, z}, which may not be zero. Only its schema.
pub struct WorldDirection;

impl JsonSchema for WorldDirection {
    fn schema_name() -> Cow<'static, str> {
        "WorldDirection".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A direction [x, y, z] or {x, y, z} of numbers below 1e9 in size, not the zero vector.",
            "anyOf": [
                {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "z": {"type": "number"}}, "required": ["x", "y", "z"]}
            ]
        })
    }
}

fn vector(v: &Value) -> Option<[f64; 3]> {
    params::vec3(v).map(|p| [p.x, p.y, p.z])
}

/// `key` as `joint.reach` reads a point (`direction` false) or a direction: finite and under 1e9
/// in length; a direction not zero. `null` is refused, not read as absent.
fn reach_vector<E: serde::de::Error>(m: &Map<String, Value>, key: &str, direction: bool) -> Result<Option<[f64; 3]>, E> {
    let Some(v) = m.get(key) else { return Ok(None) };
    let found = params::vec3(v).filter(|v| v.is_finite() && v.len() < 1e9);
    let found = if direction { found.and_then(Vec3::normalized) } else { found };
    match found {
        Some(_) => Ok(vector(v)),
        None if direction => Err(E::custom(whole(format!("`{key}` must be a direction [x, y, z]")))),
        None => Err(E::custom(whole(format!("`{key}` must be [x, y, z]")))),
    }
}

/// `key` as the handlers' `link` read: the index of a motion link, a whole number; whether the
/// link exists is the handler's to check.
fn link_index<E: serde::de::Error>(m: &Map<String, Value>, key: &str) -> Result<Option<u64>, E> {
    match m.get(key) {
        None => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| E::custom(whole("no such motion link"))),
    }
}

/// A joint origin as an object or a name, as `origin_param` reads one before it looks at the
/// design (a coupling's round face, a cam's `tip`): the forms of [`JointOriginArg`], and a face
/// given by its name is 1…16384 characters.
fn origin_arg(v: &Value) -> Result<JointOriginArg, String> {
    let named = ["face", "circle"].iter().find_map(|k| v.get(*k).and_then(Value::as_str));
    if let Some(name) = named
        && (name.is_empty() || name.len() > MAX_FACE_NAME)
    {
        return Err(format!("a face name is 1…{MAX_FACE_NAME} characters"));
    }
    JointOriginArg::read(v)
}

/// A coupling's length: a number (mm), an expression (kept as typed, so it follows the
/// parameters), or `{circle: [x, y, z], occurrence?}`, the round face there, by its radius. Kept
/// as given, for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct CouplingSize(pub Value);

impl CouplingSize {
    /// `v` as `size_param` reads `key`.
    fn read(v: &Value, key: &str) -> Result<Self, String> {
        match v {
            Value::Number(_) => Ok(CouplingSize(v.clone())),
            Value::String(t) if t.len() <= 1000 => Ok(CouplingSize(v.clone())),
            Value::Object(o) if o.contains_key("circle") => origin_arg(v).map(|_| CouplingSize(v.clone())),
            _ => Err(format!("`{key}` must be a length (mm or an expression) or a round face ({{circle: [x, y, z]}})")),
        }
    }
}

impl JsonSchema for CouplingSize {
    fn schema_name() -> Cow<'static, str> {
        "CouplingSize".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<WorldPoint>();
        json_schema!({
            "description": "A length: a number (mm), an expression (text of at most 1000 bytes, kept as typed so it follows the design's parameters), or {circle: [x, y, z], occurrence?}, the round face there, by its radius, read again whenever the design changes. The circle is a point on the round face (not its centre), or the face's name as model.faces lists it.",
            "anyOf": [
                {"type": "number"},
                {"type": "string"},
                {
                    "type": "object",
                    "properties": {
                        "circle": {"description": "A point on the round face, in world coordinates, or its name.", "anyOf": [point, {"type": "string", "minLength": 1}]},
                        "occurrence": {"description": "The occurrence the face is on, by id or name. Default: found from the point, or the named face's component.", "type": ["number", "string"]}
                    },
                    "required": ["circle"]
                }
            ]
        })
    }
}

/// `key`, read by `CouplingSize::read` with the key named in its message.
fn size<E: serde::de::Error>(m: &Map<String, Value>, key: &str) -> Result<Option<CouplingSize>, E> {
    m.get(key).map(|v| CouplingSize::read(v, key).map_err(|e| E::custom(whole(e)))).transpose()
}

/// Two joints `a` and `b`, read in that order.
fn joint_pair<E: serde::de::Error>(m: &Map<String, Value>) -> Result<(Option<JointKey>, Option<JointKey>), E> {
    Ok((given(m, "a", JointKey::read)?, given(m, "b", JointKey::read)?))
}

/// A list of joints by id or name, as `joint.reach` reads `joints`.
#[derive(Clone, Debug, PartialEq)]
pub struct JointKeyList(pub Vec<JointKey>);

impl JointKeyList {
    fn read(v: &Value) -> Result<Self, String> {
        match v {
            Value::Array(items) if items.len() <= MAX_REACH_JOINTS => items.iter().map(JointKey::read).collect::<Result<_, _>>().map(JointKeyList),
            _ => Err(format!("`joints` must be a list of joints (ids or names), at most {MAX_REACH_JOINTS}")),
        }
    }
}

impl JsonSchema for JointKeyList {
    fn schema_name() -> Cow<'static, str> {
        "JointKeyList".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let joint = generator.subschema_for::<JointKey>();
        json_schema!({
            "description": "A list of joints, each by id or name.",
            "type": "array",
            "items": joint,
            "maxItems": MAX_REACH_JOINTS
        })
    }
}

/// Two or more occurrences, by id or name, as `joint.rigid_group` reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct OccurrenceList(pub Vec<OccurrenceKey>);

impl OccurrenceList {
    fn read(v: &Value) -> Result<Self, String> {
        match v {
            Value::Array(items) if (2..=1000).contains(&items.len()) => {
                items.iter().map(OccurrenceKey::read).collect::<Result<_, _>>().map(OccurrenceList)
            }
            _ => Err("`occurrences` must list 2…1000 occurrences".into()),
        }
    }
}

impl JsonSchema for OccurrenceList {
    fn schema_name() -> Cow<'static, str> {
        "OccurrenceList".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let occurrence = generator.subschema_for::<OccurrenceKey>();
        json_schema!({
            "description": "2 to 1000 occurrences, each by id or name.",
            "type": "array",
            "items": occurrence,
            "minItems": 2,
            "maxItems": 1000
        })
    }
}

read_with!(JointKeyList, OccurrenceList);

/// Which follower a cam drives.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CamFollower {
    /// A point on the cam's outline.
    Knife,
    /// A roller of the given `radius`.
    Roller,
    /// A flat face, square to a slider's line.
    Flat,
}

/// A cam's outline, as `cam_profile` reads it: a sketch's closed profile, or picked edges of the
/// cam's bodies. Whether the sketch, the profile or an occurrence exists, and whether the edges
/// close, is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub enum CamProfileParam {
    /// A closed profile of a sketch (its largest by default).
    Sketch { sketch: String, index: Option<u64> },
    /// Points on the edges of the cam's bodies, in world coordinates.
    Edges { edges: Vec<[f64; 3]>, occurrence: Option<OccurrenceKey> },
}

impl CamProfileParam {
    fn read(v: &Value) -> Result<Self, String> {
        // `sketch`, once given (`null` included), makes it a sketch's profile, whatever else it has.
        if let Some(sketch) = v.get("sketch") {
            let sketch = crate::cmd::sketch_key(Some(sketch)).ok_or("`profile.sketch` must be a sketch id or name")?;
            let index = match v.get("index") {
                None => None,
                Some(i) => Some(i.as_u64().ok_or("the sketch has no such closed profile (an open curve makes none)")?),
            };
            return Ok(CamProfileParam::Sketch { sketch, index });
        }
        let Some(Value::Array(points)) = v.get("edges") else {
            return Err("`profile` is {sketch: id|name} or {edges: [[x, y, z], …]}".into());
        };
        if points.is_empty() || points.len() > MAX_PROFILE_EDGES {
            return Err(format!("pick 1 to {MAX_PROFILE_EDGES} edges"));
        }
        let edges = points.iter().map(vector).collect::<Option<Vec<_>>>().ok_or("`edges` are finite [x, y, z] points")?;
        let occurrence = v.get("occurrence").map(OccurrenceKey::read).transpose()?;
        Ok(CamProfileParam::Edges { edges, occurrence })
    }
}

impl JsonSchema for CamProfileParam {
    fn schema_name() -> Cow<'static, str> {
        "CamProfile".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<WorldPoint>();
        let occurrence = generator.subschema_for::<OccurrenceKey>();
        json_schema!({
            "description": "The cam's outline, in a plane square to its axis, read again whenever the design changes: {sketch, index?}, a closed profile of a sketch; or {edges, occurrence?}, points on edges of the cam's bodies that form one closed loop. A call with `sketch` is the first form, whatever else it has.",
            "anyOf": [
                {
                    "type": "object",
                    "properties": {
                        "sketch": {"description": "The sketch, by id or name.", "type": ["number", "string"]},
                        "index": {"description": "Which closed profile of the sketch, counted from zero. Default: the largest.", "type": "integer", "minimum": 0}
                    },
                    "required": ["sketch"]
                },
                {
                    "type": "object",
                    "properties": {
                        "edges": {"description": "Points on edges of the cam's bodies (world coordinates), 1 to 64, which form one closed loop.", "type": "array", "items": point, "minItems": 1, "maxItems": MAX_PROFILE_EDGES},
                        "occurrence": occurrence
                    },
                    "required": ["edges"],
                    "not": {"required": ["sketch"]}
                }
            ]
        })
    }
}

read_with!(CamProfileParam);

/// A joint origin's turn about z: `x` of `joint.origin`.
pub struct OriginX;

impl JsonSchema for OriginX {
    fn schema_name() -> Cow<'static, str> {
        "OriginX".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let direction = generator.subschema_for::<WorldDirection>();
        let point = generator.subschema_for::<WorldPoint>();
        json_schema!({
            "description": "A direction [x, y, z] in world coordinates (for a point origin also {x, y, z}), a face's name, or {face: a point on a face of the origin's component, or its name}: a direction, the way to another hole's axis or a plane's normal.",
            "anyOf": [
                direction,
                {"type": "string", "minLength": 1},
                {"type": "object", "properties": {"face": {"anyOf": [point, {"type": "string", "minLength": 1}]}}, "required": ["face"]}
            ]
        })
    }
}

/// A joint origin's face or circle: a point on it, or its name. Only its schema.
pub struct OriginFace;

impl JsonSchema for OriginFace {
    fn schema_name() -> Cow<'static, str> {
        "OriginFace".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<WorldPoint>();
        json_schema!({
            "description": "A point in world coordinates, or a face's name as model.faces lists it (1 to 16384 characters).",
            "anyOf": [point, {"type": "string", "minLength": 1}]
        })
    }
}

/// An explicit frame: its origin and z, and optionally its x. Only its schema.
pub struct OriginFrame;

impl JsonSchema for OriginFrame {
    fn schema_name() -> Cow<'static, str> {
        "OriginFrame".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<WorldPoint>();
        let direction = generator.subschema_for::<WorldDirection>();
        json_schema!({
            "description": "An explicit frame in world coordinates.",
            "type": "object",
            "properties": {
                "origin": {"description": "The frame's origin.", "anyOf": [point]},
                "z": {"description": "The frame's z axis.", "anyOf": [direction]},
                "x": {"description": "The frame's x axis. Default: chosen from z.", "anyOf": [direction]}
            },
            "required": ["origin", "z"]
        })
    }
}

/// Make a named joint origin: a frame on a face, a round edge, an edge, a point, or given
/// outright, that joints can then take for a side by its name. Give one of `face`, `circle`,
/// `edge`, `point` or `frame`; a face or circle given by its name is found again by name, so the
/// origin follows its face as the design changes. Keys that apply only to some of these (`on` and
/// `corner` to an edge; `end` to a circle; `body` to a name) are read only then.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = origin_required)]
pub struct MakeJointOriginParams {
    /// The origin's name. Default: `Joint Origin` and the first number no origin's name has.
    /// Blank text gives the default. A name already taken is refused.
    #[serde(default)]
    pub name: Option<String>,
    /// The occurrence the origin is on, by id or name. Default: found from the point, or the
    /// named face's component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrenceKey")]
    pub occurrence: Option<OccurrenceKey>,
    /// The centre of a planar face: a point on it, or its name. The first of `face` and `circle`
    /// given by name wins; then the first given as a point of `face`, `circle`, `edge` and `point`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OriginFace")]
    pub face: Option<Value>,
    /// The centre of a round end: a point on its wall or rim, the end's centre, or the round
    /// face's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OriginFace")]
    pub circle: Option<Value>,
    /// A point on an edge of a planar face: the edge's middle, z out of the face, y into it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorldPoint")]
    pub edge: Option<Value>,
    /// A point: the frame's origin there, z along `z` (default: the world's z), x along `x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorldPoint")]
    pub point: Option<Value>,
    /// An explicit frame, read only when no point is given in `face`, `circle`, `edge` or `point`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OriginFrame")]
    pub frame: Option<Value>,
    /// With `edge`: a point on the face to use. Default: the larger planar face beside the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<WorldPoint>")]
    pub on: Option<Value>,
    /// With `edge`: a point near one end of the edge, for the corner there (x and y along its two
    /// edges into the face), found again by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<WorldPoint>")]
    pub corner: Option<Value>,
    /// The frame's z, a direction in world coordinates, instead of the face's normal. Read for an
    /// `edge` (then along the edge when it runs that way), a `circle` given as a point (the way
    /// out of its end) and a `point`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<WorldDirection>")]
    pub z: Option<Value>,
    /// The frame's x, its turn about z: a direction, a face's name or {face}. Read for a `face`,
    /// a `circle` and a `point` (a direction there). Default: chosen from z alone, or the x the
    /// face's frame has now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<OriginX>")]
    pub x: Option<Value>,
    /// With `circle`: the end, by the name of the face across its rim there. Default: the end
    /// nearer the pick. Refused for any other origin; for a circle given as a point with `z` it
    /// is not checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub end: Option<Value>,
    /// With a face or circle given by its name: the body the face is on.
    #[serde(default)]
    pub body: Option<String>,
}

fn origin_required(schema: &mut Schema) {
    // The handler refuses a call with none of them.
    schema.insert(
        "anyOf".into(),
        json!([{"required": ["face"]}, {"required": ["circle"]}, {"required": ["edge"]}, {"required": ["point"]}, {"required": ["frame"]}]),
    );
}

/// The wording of the handler's refusals of a joint origin's keys.
const ORIGIN_X: &str = "`x` is a direction [x, y, z], a face's name, or {face: [x, y, z]}";
const END_NAME: &str = "`end` is the name of a face a round end lies on";
const END_ROUND: &str = "`end` is for a round origin (`circle`)";

/// A face or end name the handlers take: 1…16384 characters.
fn face_name(v: &Value) -> bool {
    v.as_str().is_some_and(|n| !n.is_empty() && n.len() <= MAX_FACE_NAME)
}

/// `x`, as `x_param` reads it: `null` is left out; a direction, a face's name or `{face}`.
fn origin_x(m: &Map<String, Value>) -> Result<(), String> {
    let direction = |v: &Value| params::vec3(v).filter(|d| d.is_finite() && d.len() > 1e-9).map(|_| ()).ok_or_else(|| ORIGIN_X.to_string());
    match m.get("x") {
        None | Some(Value::Null) => Ok(()),
        Some(v) if face_name(v) => Ok(()),
        Some(Value::Object(o)) => match o.get("face") {
            Some(f) if face_name(f) => Ok(()),
            Some(p) => params::vec3(p).map(|_| ()).ok_or_else(|| ORIGIN_X.to_string()),
            None => Err(ORIGIN_X.to_string()),
        },
        Some(d) => direction(d),
    }
}

/// `z`, as `given_z` reads it: `null` is left out; else a direction that is not zero. Whether it
/// is given tells which of the handler's paths a call takes.
fn origin_z(m: &Map<String, Value>) -> Result<bool, String> {
    match m.get("z") {
        None | Some(Value::Null) => Ok(false),
        Some(z) => {
            params::vec3(z).filter(|z| z.is_finite() && z.len() > 1e-9).map(|_| true).ok_or_else(|| "`z` is a direction [x, y, z]".to_string())
        }
    }
}

/// `end`, as `settle` reads it: the name of a face, and only for a round origin. (`null` is not
/// left out: it is not a name.)
fn origin_end(m: &Map<String, Value>, round: bool) -> Result<(), String> {
    match m.get("end") {
        None => Ok(()),
        Some(v) if !face_name(v) => Err(END_NAME.into()),
        Some(_) if !round => Err(END_ROUND.into()),
        Some(_) => Ok(()),
    }
}

/// A point key the handler reads as a point or leaves out when it is not one (`on`, `corner`, a
/// point origin's `z`, a frame's `x`): a value that is not a point is refused here, `null`
/// is left out.
fn origin_point(v: Option<&Value>, message: &str) -> Result<(), String> {
    match v {
        None | Some(Value::Null) => Ok(()),
        Some(v) => vector(v).map(|_| ()).ok_or_else(|| message.to_string()),
    }
}

impl MakeJointOriginParams {
    /// The keys `origin_param` reads, in the cases it reads them, in the order it reads them.
    fn check(m: &Map<String, Value>) -> Result<(), String> {
        // A face or circle given by its name: the first of the two that is text.
        let named = ["face", "circle"].into_iter().find_map(|k| m.get(k).and_then(Value::as_str).map(|n| (k, n)));
        if let Some((what, name)) = named {
            if name.is_empty() || name.len() > MAX_FACE_NAME {
                return Err(format!("a face name is 1…{MAX_FACE_NAME} characters"));
            }
            if !matches!(m.get("body"), None | Some(Value::Null | Value::String(_))) {
                return Err("`body` must be text".into());
            }
            m.get("occurrence").map(OccurrenceKey::read).transpose()?;
            origin_x(m)?;
            return origin_end(m, what == "circle");
        }
        // Else a point on a face, circle, edge or point, else a frame.
        let picked = ["face", "circle", "edge", "point"].into_iter().find(|k| m.get(*k).and_then(params::vec3).is_some());
        let what = match (picked, m.get("frame")) {
            (Some(k), _) => k,
            (None, Some(f)) => {
                f.get("origin").and_then(params::vec3).ok_or("`frame` needs origin and z")?;
                "frame"
            }
            _ => return Err("a joint origin needs face, circle, edge, point or frame [x, y, z], or a face's name".into()),
        };
        m.get("occurrence").map(OccurrenceKey::read).transpose()?;
        match what {
            "face" => {
                origin_x(m)?;
                origin_end(m, false)
            }
            // With a `z`, the handler tries the end and then the point, so a bad `end` is not refused.
            "circle" => {
                origin_x(m)?;
                if origin_z(m)? { Ok(()) } else { origin_end(m, true) }
            }
            "edge" => {
                origin_point(m.get("on"), "`on` must be a point [x, y, z]")?;
                origin_point(m.get("corner"), "`corner` must be a point [x, y, z]")?;
                // With a `z` the origin is made at once, and `end` is not read.
                if origin_z(m)? { Ok(()) } else { origin_end(m, false) }
            }
            "point" => {
                origin_point(m.get("z"), "`z` is a direction [x, y, z]")?;
                match m.get("x") {
                    None | Some(Value::Null) => {}
                    Some(x) => {
                        vector(x).ok_or("a point origin's `x` is a direction [x, y, z]")?;
                    }
                }
                origin_end(m, false)
            }
            _ => {
                let frame = m.get("frame");
                frame.and_then(|f| f.get("z")).and_then(params::vec3).ok_or("`frame` needs a z axis")?;
                origin_point(frame.and_then(|f| f.get("x")), "`frame`'s x must be a direction [x, y, z]")?;
                origin_end(m, false)
            }
        }
    }
}

impl<'de> Deserialize<'de> for MakeJointOriginParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        Self::check(&m).map_err(|e| D::Error::custom(whole(e)))?;
        let raw = |k: &str| m.get(k).cloned();
        // The name is read last, once the origin is made.
        Ok(MakeJointOriginParams {
            name: text(&m, "name")?,
            occurrence: m.get("occurrence").and_then(|v| OccurrenceKey::read(v).ok()),
            face: raw("face"),
            circle: raw("circle"),
            edge: raw("edge"),
            point: raw("point"),
            frame: raw("frame"),
            on: raw("on"),
            corner: raw("corner"),
            z: raw("z"),
            x: raw("x"),
            end: raw("end"),
            body: m.get("body").and_then(Value::as_str).map(str::to_string),
        })
    }
}

/// What `joint.origin` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MakeJointOriginResult {
    /// The new joint origin's name, as joints take it for a side.
    pub origin: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Join occurrences so that they move together, as they are: a rigid joint, made as built, from
/// the first occurrence to each of the others. One undo step for each joint.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = rigid_group_required)]
pub struct RigidGroupParams {
    // `occurrences` is optional in the type and required in the schema: left out, the handler
    // refuses it with its own message.
    /// The occurrences, each by id or name: 2 to 1000. The first is the one the others are joined
    /// to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrenceList")]
    pub occurrences: Option<OccurrenceList>,
}

fn rigid_group_required(schema: &mut Schema) {
    require(schema, "occurrences");
}

impl<'de> Deserialize<'de> for RigidGroupParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        Ok(RigidGroupParams { occurrences: given(&m, "occurrences", OccurrenceList::read)? })
    }
}

/// What `joint.rigid_group` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RigidGroupResult {
    /// The rigid joints made, by id: one for each occurrence after the first.
    pub joints: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Move an occurrence's point to a place in the world by changing the values of the joints on its
/// chain, from where they are to the nearest pose within their limits, through motion links and
/// keeping closed loops closed as joint.drive does (contact sets are not checked). With `z` the
/// occurrence's axis is turned to point that way too. Reaches as far as it can: the reply says how
/// close it got and what stopped it.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = reach_required)]
pub struct ReachParams {
    // `occurrence` and `point` are optional in the type and required in the schema
    // (`reach_required`): left out, the handler refuses them with its own message.
    /// The occurrence to move, by id or name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrenceKey")]
    pub occurrence: Option<OccurrenceKey>,
    /// Where its point goes, in world coordinates (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorldPoint")]
    pub point: Option<[f64; 3]>,
    /// The occurrence's point, in the world where it is now (mm). Default: the centre of the box
    /// around its bodies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorldPoint")]
    pub at: Option<[f64; 3]>,
    /// The way the occurrence's axis then points, a direction in the world. Without `z` the
    /// occurrence is not turned, only moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorldDirection")]
    pub z: Option<[f64; 3]>,
    /// The occurrence's axis, a direction in the world now, that `z` is the way it points.
    /// Default: its own z. Goes with `z`: refused without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorldDirection")]
    pub axis: Option<[f64; 3]>,
    /// Only these joints move toward the point, each by id or name (at most 1000); the other joints
    /// of a closed loop they are in follow. Default: every joint on its chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKeyList")]
    pub joints: Option<JointKeyList>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
    /// A frame of a drag: the reply says `continuous`, and the app's command line doesn't echo it.
    /// Default: false.
    #[serde(default)]
    pub continuous: Option<bool>,
}

fn reach_required(schema: &mut Schema) {
    require(schema, "occurrence");
    require(schema, "point");
    // `axis` goes with `z`.
    schema.insert("dependentRequired".into(), json!({"axis": ["z"]}));
}

impl<'de> Deserialize<'de> for ReachParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        // In the handler's order.
        let occurrence = given(&m, "occurrence", OccurrenceKey::read)?;
        let point = reach_vector(&m, "point", false)?;
        let at = reach_vector(&m, "at", false)?;
        let z = reach_vector(&m, "z", true)?;
        let axis = reach_vector(&m, "axis", true)?;
        let joints = given(&m, "joints", JointKeyList::read)?;
        Ok(ReachParams { occurrence, point, at, z, axis, joints, quiet: flag(&m, "quiet")?, continuous: flag(&m, "continuous")? })
    }
}

/// Which limit of a value a reach rests on.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LimitSide {
    /// The lowest the value goes.
    Min,
    /// The highest.
    Max,
}

/// A limit a reach came to rest on, short of the point.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LimitStop {
    /// The joint, by name.
    pub joint: String,
    /// The coordinate that rests on its limit (angle, distance, …), by name.
    pub value: Option<String>,
    /// Which limit.
    pub limit: LimitSide,
}

/// The closed loop that stopped a reach.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReachLoopStop {
    /// The joint that closes the loop, by name.
    pub joint: String,
}

/// A joint a reach moved.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReachedJoint {
    /// The joint, by id.
    pub joint: u64,
    /// Its name.
    pub name: String,
    /// Its values now, as joint.list gives them.
    pub values: Vec<NamedValue>,
}

/// What `joint.reach` returns: the solve, then how close the occurrence's point got.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReachResult {
    #[serde(flatten)]
    pub solved: SolveReport,
    /// The occurrence moved, by name.
    pub occurrence: String,
    /// Where its point is now, in world coordinates (mm).
    pub point: [f64; 3],
    /// How far that is from the target, in mm.
    pub error_mm: f64,
    /// With `z`: how far the axis points from where it was asked to, in degrees. Left out without.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_deg: Option<f64>,
    /// Whether the point (and the axis, with `z`) is at the target, within tolerance.
    pub reached: bool,
    /// The joints that moved, with their values now.
    pub values: Vec<ReachedJoint>,
    /// Whether any occurrence moved.
    pub moved: bool,
    /// The limits it rests on short of the point; `null` when none.
    pub stopped_by_limit: Option<Vec<LimitStop>>,
    /// The closed loop that stopped it (its chain alone gets closer); `null` when none.
    pub stopped_by_loop: Option<ReachLoopStop>,
    /// Why nothing can move. Left out when the reach worked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// True for a `continuous` call; left out otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuous: Option<bool>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Tie one joint's value to another's: b = ratio · a + offset, in b's units, so that when a is
/// driven b follows. The link starts at b's value now (the offset is `offset`, or 0). A joint
/// follows one link at most, and two joints may not drive each other.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = motion_link_required)]
pub struct MotionLinkParams {
    // `a`, `b` and `ratio` are optional in the type and required in the schema: left out, the
    // handler refuses them with its own message.
    /// The driving joint, by id or name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub a: Option<JointKey>,
    /// The driven joint, by id or name. Not the same joint as `a`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub b: Option<JointKey>,
    /// b's units per a's, per degree for an angle (mm per degree for an angle driving a distance,
    /// degrees per mm the other way round).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FiniteNumber")]
    pub ratio: Option<f64>,
    /// The offset in b's units (degrees for an angle, mm for a distance), or an expression.
    /// Default: 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub offset: Option<JointValue>,
    /// Which of a's values drives, counted from zero (a negative number is 0). Default: 0.
    #[serde(default)]
    pub ia: Option<f64>,
    /// Which of b's values follows, counted from zero (a negative number is 0). Default: 0.
    #[serde(default)]
    pub ib: Option<f64>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

fn motion_link_required(schema: &mut Schema) {
    require(schema, "a");
    require(schema, "b");
    require(schema, "ratio");
}

/// A number below 1e9 in size. Only its schema.
pub struct FiniteNumber;

impl JsonSchema for FiniteNumber {
    fn schema_name() -> Cow<'static, str> {
        "FiniteNumber".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A number below 1e9 in size.", "type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9})
    }
}

impl<'de> Deserialize<'de> for MotionLinkParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        // In the handler's order: the joints, the ratio, the indices, then the offset.
        let (a, b) = joint_pair(&m)?;
        let ratio = given(&m, "ratio", |v| match v {
            Value::Number(n) => n.as_f64().and_then(params::finite).ok_or_else(|| "`ratio` must be a number".to_string()),
            _ => Err("`ratio` must be a number".to_string()),
        })?;
        let (ia, ib) = (number(&m, "ia")?, number(&m, "ib")?);
        let offset = given(&m, "offset", JointValue::read)?;
        Ok(MotionLinkParams { a, b, ratio, offset, ia, ib, quiet: flag(&m, "quiet")? })
    }
}

/// What the coupling commands return: the solve, then the motion link made and its ratio.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CouplingResult {
    #[serde(flatten)]
    pub solved: SolveReport,
    /// The motion link made (or replaced), by its index in `joint.list`'s `links`.
    pub link: usize,
    /// The ratio worked out: b's units per a's, per degree (mm per degree, or degrees per mm).
    pub ratio: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Gear one turning joint to another: b turns the other way from a by the ratio of the pitch
/// radii (the same way for an internal gear), and keeps where it is, so the teeth mesh at the pose
/// they are in. The ratio follows the radii (an expression or a round face) as the design changes.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = gear_required)]
pub struct GearParams {
    // `a`, `b`, `ra` and `rb` are optional in the type and required in the schema: left out, the
    // handler refuses them with its own message.
    /// The driving joint, a revolute one, by id or name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub a: Option<JointKey>,
    /// The driven joint, a revolute one, by id or name. Not the same joint as `a`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub b: Option<JointKey>,
    /// a's gear: its pitch radius (or tooth count), in mm, an expression, or the round face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CouplingSize")]
    pub ra: Option<CouplingSize>,
    /// b's gear, as `ra`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CouplingSize")]
    pub rb: Option<CouplingSize>,
    /// One is a ring gear: both turn the same way. Default: false (b turns the other way).
    #[serde(default)]
    pub internal: Option<bool>,
    /// Turn b the other way round. Default: false.
    #[serde(default)]
    pub reverse: Option<bool>,
    /// Replace that motion link (its index in `joint.list`'s `links`) instead of adding one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub link: Option<u64>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

fn gear_required(schema: &mut Schema) {
    for key in ["a", "b", "ra", "rb"] {
        require(schema, key);
    }
}

impl<'de> Deserialize<'de> for GearParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        let (a, b) = joint_pair(&m)?;
        let (ra, rb) = (size(&m, "ra")?, size(&m, "rb")?);
        let (internal, reverse) = (flag(&m, "internal")?, flag(&m, "reverse")?);
        Ok(GearParams { a, b, ra, rb, internal, reverse, link: link_index(&m, "link")?, quiet: flag(&m, "quiet")? })
    }
}

/// Couple a turning joint to a sliding one as a rack and pinion: the rack travels the pinion's
/// radius times the turn in radians. b is driven by a, in either order of turning and sliding, and
/// keeps where it is, so the teeth mesh at the pose they are in.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = rack_pinion_required)]
pub struct RackPinionParams {
    // `a`, `b` and `radius` are optional in the type and required in the schema: left out, the
    // handler refuses them with its own message.
    /// The driving joint by id or name: a revolute one (the pinion) or a slider (the rack).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub a: Option<JointKey>,
    /// The driven joint, the other kind from `a`, by id or name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub b: Option<JointKey>,
    /// The pinion's pitch radius: mm, an expression, or the round face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CouplingSize")]
    pub radius: Option<CouplingSize>,
    /// Move the rack the other way round. Default: false.
    #[serde(default)]
    pub reverse: Option<bool>,
    /// Replace that motion link (its index in `joint.list`'s `links`) instead of adding one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub link: Option<u64>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

fn rack_pinion_required(schema: &mut Schema) {
    for key in ["a", "b", "radius"] {
        require(schema, key);
    }
}

impl<'de> Deserialize<'de> for RackPinionParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        let (a, b) = joint_pair(&m)?;
        let radius = size(&m, "radius")?;
        Ok(RackPinionParams { a, b, radius, reverse: flag(&m, "reverse")?, link: link_index(&m, "link")?, quiet: flag(&m, "quiet")? })
    }
}

/// Couple a turn to a travel as a screw: one cylindrical joint whose turn drives its travel, or a
/// joint that turns and one that slides, b driven by a. The travel is the pitch for each turn, and
/// b keeps where it is.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = screw_required)]
pub struct ScrewParams {
    // `pitch`, and `joint` or `a` and `b`, are optional in the type and required in the schema:
    // left out, the handler refuses them with its own message.
    /// A cylindrical joint, by id or name, whose turn drives its travel. When given, `a` and `b` are
    /// not read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub joint: Option<JointKey>,
    /// The driving joint, by id or name: one that turns or one that slides. Read without `joint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub a: Option<JointKey>,
    /// The driven joint, the other kind from `a`. Read without `joint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub b: Option<JointKey>,
    /// The travel for each turn: mm, or an expression, or the round face (its radius).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CouplingSize")]
    pub pitch: Option<CouplingSize>,
    /// A left-hand thread. Default: false.
    #[serde(default)]
    pub reverse: Option<bool>,
    /// Replace that motion link (its index in `joint.list`'s `links`) instead of adding one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub link: Option<u64>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

fn screw_required(schema: &mut Schema) {
    require(schema, "pitch");
    // The handler refuses a call with neither form.
    schema.insert("anyOf".into(), json!([{"required": ["joint"]}, {"required": ["a", "b"]}]));
}

impl<'de> Deserialize<'de> for ScrewParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        // `joint`, once given (`null` included), is the form; `a` and `b` are not read then.
        let (joint, a, b) = match m.get("joint") {
            Some(v) => (Some(JointKey::read(v).map_err(|e| D::Error::custom(whole(e)))?), None, None),
            None => {
                let (a, b) = joint_pair(&m)?;
                (None, a, b)
            }
        };
        let pitch = size(&m, "pitch")?;
        Ok(ScrewParams { joint, a, b, pitch, reverse: flag(&m, "reverse")?, link: link_index(&m, "link")?, quiet: flag(&m, "quiet")? })
    }
}

/// Drive a follower from a cam: the cam's turn sets the follower's travel (a slider, a translating
/// follower) or turn (a revolute, an oscillating one) from the cam's outline, and the follower
/// keeps where it is. The link is read again whenever the design changes.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = cam_required)]
pub struct CamParams {
    // `a`, `b` and `profile` are optional in the type and required in the schema: left out, the
    // handler refuses them with its own message.
    /// The cam's joint, a revolute one, by id or name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub a: Option<JointKey>,
    /// The follower's joint, by id or name: a slider (a translating follower) or a revolute (an
    /// oscillating one). Driven by `a`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointKey")]
    pub b: Option<JointKey>,
    /// The cam's outline: {sketch, index?} or {edges, occurrence?}.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CamProfileParam")]
    pub profile: Option<CamProfileParam>,
    /// The follower's kind: knife, roller or flat (a face square to a slider's line). Default:
    /// knife.
    #[serde(default)]
    pub follower: Option<CamFollower>,
    /// The roller's radius: mm, an expression, or the roller's round face (which is its tip too).
    /// Read only for a roller, which needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CouplingSize")]
    pub radius: Option<CouplingSize>,
    /// A joint origin where the follower touches: its knife's point or its roller's centre, such as
    /// {circle: [x, y, z]} on the roller. An oscillating follower needs it (unless its roller's
    /// `radius` is a round face); a translating follower's line runs through it, by default along
    /// its slider's axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointOriginArg")]
    pub tip: Option<JointOriginArg>,
    /// Ride the cam from the other side than the one the follower is on now. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Replace that motion link (its index in `joint.list`'s `links`) instead of adding one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub link: Option<u64>,
    /// Leave out `dof` and `fixed` in the reply. Default: false.
    #[serde(default)]
    pub quiet: Option<bool>,
}

fn cam_required(schema: &mut Schema) {
    for key in ["a", "b", "profile"] {
        require(schema, key);
    }
    // A roller needs its radius.
    schema.insert("if".into(), json!({"properties": {"follower": {"const": "roller"}}, "required": ["follower"]}));
    schema.insert("then".into(), json!({"required": ["radius"]}));
}

impl<'de> Deserialize<'de> for CamParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = keys(d)?;
        // In the handler's order: the joints, the follower and its radius, the tip, the profile.
        let (a, b) = joint_pair(&m)?;
        let follower = match text(&m, "follower")?.as_deref() {
            None => None,
            Some("knife") => Some(CamFollower::Knife),
            Some("roller") => Some(CamFollower::Roller),
            Some("flat") => Some(CamFollower::Flat),
            Some(_) => return Err(D::Error::custom(whole("`follower` is knife, roller or flat"))),
        };
        // Only a roller reads its radius.
        let radius = if follower == Some(CamFollower::Roller) { size(&m, "radius")? } else { None };
        let tip = given(&m, "tip", origin_arg)?;
        let profile = given(&m, "profile", CamProfileParam::read)?;
        Ok(CamParams { a, b, profile, follower, radius, tip, flip: flag(&m, "flip")?, link: link_index(&m, "link")?, quiet: flag(&m, "quiet")? })
    }
}

/// What `joint.cam` returns: the solve, then the motion link made and the follower's travel.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamResult {
    #[serde(flatten)]
    pub solved: SolveReport,
    /// The motion link made (or replaced), by its index in `joint.list`'s `links`.
    pub link: usize,
    /// The follower's travel over one turn of the cam; `null` when the link's table is not whole.
    pub lift: Option<CamLift>,
    /// Whether the follower rides the cam from the side `flip` chose, which the cam's direction of
    /// turn decides.
    pub reverse: bool,
    /// For a translating follower: how far its line passes from the cam's axis, in mm. Left out for
    /// an oscillating one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follower_offset: Option<f64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "joints_tests.rs"]
mod tests;
