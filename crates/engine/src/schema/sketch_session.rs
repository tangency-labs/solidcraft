//! Contracts and result types for the sketch session commands in `cmd/sketch.rs`:
//! `sketch.create`, `sketch.construction`, `sketch.delete`, `sketch.move_point`,
//! `sketch.move_points`, `sketch.drag`, `sketch.drag_end`, `sketch.dimension_text`,
//! `sketch.options`, `sketch.chain` and `sketch.solve` (#13224).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a sketch, a curve, a point or a
//! dimension exists stays the handler's to check, and so does everything that needs the design.
//! A key the handler refuses when it is left out is optional in the type and required in the
//! schema, so that the handler's message, not the check's "is required", is what a caller sees.
//!
//! Some handlers read a key only in some cases, and the types read it only there too, so a call
//! that works today gets no warning:
//!
//! - `sketch.create` reads `project_edges` only when `plane` is a picked face, and `frame` is
//!   for an explicit plane only. It refuses `face`, `origin`, `x_dir` and `y_dir` next to
//!   `plane` with a message that says where they belong, so they are listed (and can never be
//!   used) to keep that message in strict;
//! - `sketch.dimension_text` reads `at` only without `reset: true`;
//! - `sketch.drag_end` reads a constraint's `command` and `params` only when `constraint` is not
//!   `null`.
//!
//! A call with `live: true` of `sketch.drag` goes to the live runner, which the check doesn't
//! wrap, so live frames are not checked yet.
//!
//! `sketch.options` refuses a key that is not one of its options with "unknown option"; the
//! check names it first ("unknown key", with a suggestion) in warn and strict.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use super::canvas::CanvasPlane;
use super::check::{located, whole};
use super::frame::SketchSolve;
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::types::{LengthExpr, Point2, present};
use crate::params;

/// The words of `sketch.create`'s refusals, as `plane_frame` and `flip_param` give them.
const FRAME_FORMS: &str = "`frame` must be \"world\" or \"component\"";
const FRAME_NOT_EXPLICIT: &str =
    "`frame` is for an explicit plane {origin, x_dir, y_dir}: a named plane is the component's own, and a face is where it is";
const FLIP_FORMS: &str = "`flip` must be true, false, \"x\" or \"y\"";
const FLIP_ON_FACE: &str = "`flip` is for origin, construction and explicit planes; a face already faces outward";
const ACTIVATE_FORMS: &str = "`activate_part` must be true or false";

/// The keys `sketch.create` refuses next to `plane`, and the form each belongs in.
const MISPLACED: [(&str, &str); 4] = [
    ("face", r#"{"plane": {"face": [x, y, z]}}"#),
    ("origin", r#"{"plane": {"origin": [x, y, z], "x_dir": [..], "y_dir": [..]}}"#),
    ("x_dir", r#"{"plane": {"origin": [x, y, z], "x_dir": [..], "y_dir": [..]}}"#),
    ("y_dir", r#"{"plane": {"origin": [x, y, z], "x_dir": [..], "y_dir": [..]}}"#),
];

/// The message of a handler's refusal.
fn message(e: crate::EngineError) -> String {
    match e {
        crate::EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    }
}

/// The handler's message for a value that is not what a key needs, kept whole.
fn refuse<E: serde::de::Error>(message: impl std::fmt::Display) -> E {
    E::custom(whole(message))
}

// ---------------------------------------------------------------------------------------------
// Shared readers

/// `key` as text, the handler's `str_`-and-refuse: a string, or `message`.
macro_rules! text_key {
    ($($name:ident: $message:literal),+ $(,)?) => {$(
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
            match Value::deserialize(d)? {
                Value::String(s) => Ok(Some(s)),
                _ => Err(refuse($message)),
            }
        }
    )+};
}

text_key!(
    point_id: "`point` must be a point reference",
    entity_id: "`entity` must be a sketch point or curve",
    curve_id: "`curve` must be a curve id",
    dimension_id: "`dimension` must be a dimension id or parameter name",
);

/// A required point `[x, y]` as `req_vec2` reads it (`null` is refused).
macro_rules! point_key {
    ($($name:ident: $key:literal),+ $(,)?) => {$(
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Point2>, D::Error> {
            params::vec2(&Value::deserialize(d)?)
                .map(|p| Some(Point2(p)))
                .ok_or_else(|| refuse(concat!("`", $key, "` must be a point [x, y]")))
        }
    )+};
}

point_key!(to_point: "to", at_point: "at");

/// `from` of `sketch.drag`: a point, where `null` is read as left out (the grab is at `to`).
fn from_point<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Point2>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => params::vec2(&v).map(|p| Some(Point2(p))).ok_or_else(|| refuse("`from` must be a point [x, y]")),
    }
}

/// Entity ids, as `params::string_list` reads a list of names: a list of text, or one text. An
/// empty list, `null` included, is the handler's own refusal (`empty`).
#[derive(Clone, Debug, PartialEq)]
pub struct EntityIds(pub Vec<String>);

fn entity_ids<'de, D: Deserializer<'de>>(d: D, key: &str, empty: &str) -> Result<Option<EntityIds>, D::Error> {
    let mut given = Map::new();
    given.insert(key.to_string(), Value::deserialize(d)?);
    let list = params::string_list("", &Value::Object(given), key).map_err(|e| refuse::<D::Error>(message(e)))?;
    if list.is_empty() { Err(refuse(empty)) } else { Ok(Some(EntityIds(list))) }
}

fn curves<'de, D: Deserializer<'de>>(d: D) -> Result<Option<EntityIds>, D::Error> {
    entity_ids(d, "curves", "`curves` must list curve ids")
}

fn entities<'de, D: Deserializer<'de>>(d: D) -> Result<Option<EntityIds>, D::Error> {
    entity_ids(d, "entities", "`entities` must list sketch entity ids")
}

impl JsonSchema for EntityIds {
    fn schema_name() -> Cow<'static, str> {
        "SketchEntityIds".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Ids, as a list of text (at least one, at most 10000) or a single id.",
            "anyOf": [
                {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": params::MAX_STRING_LIST},
                {"type": "string"}
            ]
        })
    }
}

/// The ids of what a drag takes along: a list of text, as the handler reads `with`.
#[derive(Clone, Debug, PartialEq)]
pub struct DragWith(pub Vec<String>);

fn with_ids<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DragWith>, D::Error> {
    const FORMS: &str = "`with` must be a list of sketch point or curve ids";
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Array(ids) => ids
            .into_iter()
            .map(|v| if let Value::String(id) = v { Some(id) } else { None })
            .collect::<Option<Vec<_>>>()
            .map(|ids| Some(DragWith(ids)))
            .ok_or_else(|| refuse(FORMS)),
        _ => Err(refuse(FORMS)),
    }
}

impl JsonSchema for DragWith {
    fn schema_name() -> Cow<'static, str> {
        "DragWith".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Point and curve ids, as a list. No more than the sketch has points and curves together.",
            "type": "array",
            "items": {"type": "string"}
        })
    }
}

/// An optional flag the handler reads with `bool_`, which takes a wrong type as absent: `null` is
/// absent, and any other value that is not true or false is refused here.
fn flag<'de, D: Deserializer<'de>>(d: D, key: &str) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(refuse(format!("`{key}` must be true or false"))),
    }
}

fn live<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag(d, "live")
}

fn reset<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag(d, "reset")
}

/// One flag each, as the handler reads them (see [`flag`]).
macro_rules! option_flags {
    ($($name:ident),+ $(,)?) => {$(
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
            flag(d, stringify!($name))
        }
    )+};
}

option_flags!(value, show_profile, show_points, show_dimensions, show_constraints, show_projected, show_construction, grid, snap, slice, sketch_3d);

// ---------------------------------------------------------------------------------------------
// sketch.create

/// A plane for a new sketch, as `sketch.create` reads `plane`: XY, XZ or YZ (or Top, Front or
/// Right), a construction plane by name or id, a picked planar face, or an explicit plane. Kept
/// as given, for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchPlane(pub Value);

impl<'de> Deserialize<'de> for SketchPlane {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        CanvasPlane::deserialize(d).map(|plane| SketchPlane(plane.0))
    }
}

impl JsonSchema for SketchPlane {
    fn schema_name() -> Cow<'static, str> {
        "SketchPlane".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        CanvasPlane::json_schema(generator)
    }
}

/// `flip`: `true`, `false`, or the axis (`"x"` or `"y"`, any case) to turn the plane about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flip {
    Flag(bool),
    AboutX,
    AboutY,
}

impl<'de> Deserialize<'de> for Flip {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Bool(b) => Ok(Flip::Flag(b)),
            Value::String(a) if a.eq_ignore_ascii_case("x") => Ok(Flip::AboutX),
            Value::String(a) if a.eq_ignore_ascii_case("y") => Ok(Flip::AboutY),
            _ => Err(refuse(FLIP_FORMS)),
        }
    }
}

impl JsonSchema for Flip {
    fn schema_name() -> Cow<'static, str> {
        "Flip".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "false keeps the plane's own side; true turns it about its y axis (about x when y does not point upward) so the sketch's y axis stays upward; \"x\" or \"y\" (any case) turns it about that axis of the plane.",
            "anyOf": [{"type": "boolean"}, {"type": "string", "pattern": "^[xXyY]$"}]
        })
    }
}

impl Flip {
    /// Whether the plane is turned at all.
    fn turns(self) -> bool {
        self != Flip::Flag(false)
    }
}

/// Which coordinates an explicit plane is in: `world` (the default) or the active component's
/// own, `component`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Frame {
    // World coordinates (the default); the component's own coordinates.
    World,
    Component,
}

impl Frame {
    /// The frame `plane_frame` names.
    pub fn named(name: &str) -> Option<Frame> {
        match name {
            "world" => Some(Frame::World),
            "component" => Some(Frame::Component),
            _ => None,
        }
    }
}

/// A key `sketch.create` refuses next to `plane`: it belongs inside `plane`.
#[derive(Debug)]
pub struct InsidePlane;

impl<'de> Deserialize<'de> for InsidePlane {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(refuse("this key belongs inside `plane`"))
    }
}

impl JsonSchema for InsidePlane {
    fn schema_name() -> Cow<'static, str> {
        "InsidePlane".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Not taken here, in any form: the call is refused, and the message says where it belongs. Put it inside `plane`.",
            "not": {}
        })
    }
}

/// The keys of a `sketch.create` call.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CreateKeys {
    /// The plane to sketch on: XY, XZ or YZ (also Top, Front, Right), a construction plane's
    /// name or id, a picked planar face `{face: [x, y, z], body?, index?, path?, fixed?}`, or an
    /// explicit plane `{origin?, x_dir, y_dir}` in mm. Default: XY. On XY, the sketch's x and y
    /// run along world +X and +Y; on XZ, +X and -Z; on YZ, +Y and +Z. A face of a sheet metal
    /// body attaches to its panel instead, in the flat pattern's coordinates; `fixed: true` keeps
    /// the plane where the face is now. A named plane is the active component's own.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchPlane")]
    pub plane: Option<SketchPlane>,
    /// Move the plane this far along its normal (mm or a length expression), before `flip`.
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    /// Not on a face: sketch the plane from its other side, so the normal and the default
    /// extrude direction are reversed. Default: false.
    #[serde(default)]
    pub flip: Option<Flip>,
    /// Which coordinates an explicit plane is in. Default: world. Only for an explicit plane
    /// `{origin, x_dir, y_dir}`; the reply's `frame` says which, with a `note` when the active
    /// component is moved or turned.
    #[serde(default)]
    pub frame: Option<Frame>,
    /// Only on a picked face: project the face's edges into the sketch. Default: the session's
    /// Auto Project setting.
    #[serde(default)]
    pub project_edges: Option<bool>,
    /// Only on a face of a body of another component than the active one: activate that
    /// component at the picked placement first, as `component.activate` would, and make the
    /// sketch there. It stays active afterwards and the reply's `activated` names it. Default:
    /// false.
    #[serde(default)]
    pub activate_part: Option<bool>,
    /// The sketch's name. Default: numbered.
    #[serde(default)]
    pub name: Option<String>,
    /// Refused: it belongs inside `plane`.
    #[serde(default)]
    #[schemars(with = "InsidePlane")]
    pub face: Option<InsidePlane>,
    /// Refused: it belongs inside `plane`.
    #[serde(default)]
    #[schemars(with = "InsidePlane")]
    pub origin: Option<InsidePlane>,
    /// Refused: it belongs inside `plane`.
    #[serde(default)]
    #[schemars(with = "InsidePlane")]
    pub x_dir: Option<InsidePlane>,
    /// Refused: it belongs inside `plane`.
    #[serde(default)]
    #[schemars(with = "InsidePlane")]
    pub y_dir: Option<InsidePlane>,
}

/// `sketch.create`'s parameters: [`CreateKeys`], read in the handler's order, and `project_edges`
/// only for a face, as the handler does.
#[derive(Debug)]
pub struct CreateParams(pub CreateKeys);

impl<'de> Deserialize<'de> for CreateParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        for (key, example) in MISPLACED {
            if value.get(key).is_some() {
                return Err(refuse(format!("`{key}` belongs inside `plane`: {example}")));
            }
        }
        if !matches!(value.get("activate_part"), None | Some(Value::Null | Value::Bool(_))) {
            return Err(refuse(ACTIVATE_FORMS));
        }
        let face = value.get("plane").and_then(|plane| plane.get("face")).is_some();
        let explicit = value.get("plane").is_some_and(Value::is_object) && !face;
        let frame = match value.get("frame") {
            None | Some(Value::Null) => false,
            Some(Value::String(f)) if Frame::named(f).is_some() => true,
            Some(_) => return Err(refuse(FRAME_FORMS)),
        };
        if frame && !explicit {
            return Err(refuse(FRAME_NOT_EXPLICIT));
        }
        if let Some(plane) = value.get("plane") {
            SketchPlane::deserialize(plane).map_err(|e| D::Error::custom(e.to_string()))?;
        }
        match value.get("flip") {
            None | Some(Value::Null) => {}
            Some(flip) => {
                let flip = Flip::deserialize(flip).map_err(|e| D::Error::custom(e.to_string()))?;
                if flip.turns() && face {
                    return Err(refuse(FLIP_ON_FACE));
                }
            }
        }
        // `project_edges` is read only for a picked face.
        if !face && let Some(keys) = value.as_object_mut() {
            keys.remove("project_edges");
        }
        serde_path_to_error::deserialize(&value).map(CreateParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for CreateParams {
    fn schema_name() -> Cow<'static, str> {
        "CreateParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        CreateKeys::json_schema(generator)
    }
}

/// How a new sketch is attached to its plane: `exact_face`, `sheet_face`, `internal_legacy_face`
/// or `fixed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Attachment {
    // `exact_face`: a planar face of a body, exactly, so the sketch follows the face;
    // `sheet_face`: the panel of a sheet metal body's flat pattern;
    // `internal_legacy_face`: a face picked by a command that starts a sketch on a face
    // (Extrude, Revolve, Press Pull);
    // `fixed`: where the plane is (an origin, construction or explicit plane, or a face kept
    // with `fixed`).
    ExactFace,
    SheetFace,
    InternalLegacyFace,
    Fixed,
}

/// The component `activate_part` made active.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Activated {
    /// The component, by id.
    pub component: u64,
    /// The placement's occurrence path (empty at the root).
    pub path: Vec<u64>,
}

/// What `sketch.create` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CreateResult {
    /// The new sketch's feature id.
    pub sketch: u64,
    /// Its name.
    pub name: String,
    /// On a face, the link id of the face's projected edges; null when none were projected.
    pub projected: Option<String>,
    /// How the sketch is attached to its plane.
    pub attachment: Option<Attachment>,
    /// For an explicit plane: which coordinates it was read in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<Frame>,
    /// With a world `frame`, when the active component is moved or turned: that the plane was
    /// stored at its world place in the component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// With `activate_part`, when it made another component active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated: Option<Activated>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// sketch.construction, sketch.delete

fn curves_required(schema: &mut Schema) {
    require(schema, "curves");
}

fn entities_required(schema: &mut Schema) {
    require(schema, "entities");
}

/// Switch curves between normal and construction. The selection switches together (a slot or
/// polygon as a whole): to construction unless all of it already is. Projected curves may
/// switch too. A centerline that becomes construction stops being a centerline, and a normal
/// line is a finite one. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = curves_required)]
pub struct ConstructionParams {
    // Optional in the type and required in the schema (`curves_required`).
    /// The curves, by id.
    #[serde(default, deserialize_with = "curves", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "EntityIds")]
    pub curves: Option<EntityIds>,
    /// True makes them construction, false makes them normal. Default: construction unless all of
    /// them already are.
    #[serde(default, deserialize_with = "value", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub value: Option<bool>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// What `sketch.construction` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConstructionResult {
    /// How many curves changed.
    pub changed: usize,
    /// Whether they are construction now.
    pub construction: bool,
    /// The sketch's solve; null when it was not solved again.
    pub sketch: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Delete curves, points and constraints (dimensions too) from a sketch, by id; a sketch's
/// wires and linked geometry too. The sketch origin cannot be deleted. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = entities_required)]
pub struct DeleteParams {
    // Optional in the type and required in the schema (`entities_required`).
    /// What to delete: curve, point, constraint, wire or link ids.
    #[serde(default, deserialize_with = "entities", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "EntityIds")]
    pub entities: Option<EntityIds>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// What `sketch.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeleteResult {
    /// How many ids were given (all of them were found).
    pub deleted: usize,
    /// The sketch's solve; null when it was not solved again.
    pub sketch: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// sketch.move_point, sketch.move_points, sketch.drag, sketch.drag_end

fn move_point_required(schema: &mut Schema) {
    require(schema, "point");
    require(schema, "to");
}

/// Move a sketch point to a position and solve the sketch within its constraints: the point
/// goes as far toward it as they allow. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = move_point_required)]
pub struct MovePointParams {
    // `point` and `to` are optional in the type and required in the schema
    // (`move_point_required`).
    /// The point, by id (`p3`, `l1.end`, `origin`).
    #[serde(default, deserialize_with = "point_id", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub point: Option<String>,
    /// Where to move it, in sketch coordinates (mm).
    #[serde(default, deserialize_with = "to_point", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub to: Option<Point2>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// What `sketch.move_point` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovePointResult {
    /// The sketch's solve; null when it was not solved again.
    pub sketch: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

fn move_points_required(schema: &mut Schema) {
    require(schema, "moves");
}

/// One point of a `sketch.move_points` call: which point is moved and where it goes. The handler
/// reads `near` whenever that key is there and only looks at `point` without it, so a `point`
/// next to a `near` is left alone.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveOne {
    /// The point, by id (`p3`, `l1.end`, `origin`).
    pub point: Option<String>,
    /// The sketch point nearest to this position, instead of naming one.
    pub near: Option<Point2>,
    /// Where the point is moved to, in sketch coordinates (mm).
    pub to: Point2,
}

impl MoveOne {
    /// One entry of `moves`, read as the handler reads it and in its order: `to` first, then
    /// `near` when the key is there (`near: null` is refused), and `point` only without it.
    /// `k` is the entry's position, which the handler's messages name.
    fn read(m: &Value, k: usize) -> Result<Self, String> {
        let to = m.get("to").and_then(params::vec2).map(Point2).ok_or_else(|| format!("moves[{k}]: `to` must be a point [x, y]"))?;
        let near = match m.get("near") {
            Some(v) => Some(params::vec2(v).map(Point2).ok_or_else(|| format!("moves[{k}]: `near` must be a point [x, y]"))?),
            None => None,
        };
        let point = match (near, m.get("point")) {
            (Some(_), _) => None,
            (None, Some(v)) => Some(v.as_str().ok_or_else(|| "`point` must be a point reference".to_string())?.to_string()),
            (None, None) => None,
        };
        Ok(MoveOne { point, near, to })
    }
}

impl JsonSchema for MoveOne {
    fn schema_name() -> Cow<'static, str> {
        "MoveOne".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point2>();
        json_schema!({
            "description": "One point of the move: where it goes, and either its id or a position \
                            whose nearest sketch point is meant.",
            "type": "object",
            "properties": {
                "point": {"description": "The point, by id (`p3`, `l1.end`, `origin`).", "type": "string"},
                "near": point,
                "to": point,
            },
            "required": ["to"],
            "anyOf": [{"required": ["point"]}, {"required": ["near"]}]
        })
    }
}

/// `moves`, entry by entry as the handler reads them, with its messages. An empty list is the
/// handler's own refusal, and so is anything that is not a list.
fn moves<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<MoveOne>, D::Error> {
    let list = match Value::deserialize(d)? {
        Value::Array(list) if !list.is_empty() => list,
        _ => return Err(refuse("`moves` must be a list of {point | near, to}")),
    };
    list.iter().enumerate().map(|(k, m)| MoveOne::read(m, k).map_err(refuse::<D::Error>)).collect()
}

fn moves_schema(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The points to move, one entry each; every entry needs `to`, and names its \
                        point by `point` or by `near`, the sketch point closest to it.",
        "type": "array",
        "items": generator.subschema_for::<MoveOne>(),
        "minItems": 1
    })
}

/// `tolerance`: how close a sketch point must be to `near` to be the one moved, as the handler
/// reads it; `null` is its default (0.1 mm).
fn tolerance<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v.as_f64().and_then(params::finite).filter(|t| *t > 0.0).map(Some).ok_or_else(|| refuse("`tolerance` must be a positive length (mm)")),
    }
}

fn tolerance_schema(_: &mut SchemaGenerator) -> Schema {
    // Above 1e9 mm the engine does not read a length any more (`params::finite`).
    json_schema!({"type": ["number", "null"], "exclusiveMinimum": 0.0, "exclusiveMaximum": 1e9})
}

/// Move several sketch points, each to its own target, in one edit (one undo step), and solve the
/// sketch within its constraints: every point goes as far toward its target as they allow. A
/// point that parameters place is refused, naming them.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = move_points_required)]
pub struct MovePointsParams {
    // `moves` is optional in the type and required in the schema (`move_points_required`), so a
    // call without it gets the handler's message.
    /// The points to move, each `{point | near, to}`.
    #[serde(default, deserialize_with = "moves", skip_serializing_if = "Vec::is_empty")]
    #[schemars(schema_with = "moves_schema")]
    pub moves: Vec<MoveOne>,
    /// How close a sketch point must be to `near` to be the one moved, mm (default 0.1).
    #[serde(default, deserialize_with = "tolerance", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "tolerance_schema")]
    pub tolerance: Option<f64>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// One point of a `sketch.move_points` reply: where it was asked to go and where it ended up.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedPoint {
    /// The point's id; null when the sketch no longer has it.
    pub point: Option<String>,
    /// Where it was asked to go: [x, y].
    pub to: [f64; 2],
    /// Where it is after the solve: [x, y]; null when the sketch no longer has the point.
    pub at: Option<[f64; 2]>,
    /// Whether it reached `to` (constraints may stop it short of it).
    pub reached: bool,
}

/// What `sketch.move_points` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovePointsResult {
    /// The points it moved, in the order they were given.
    pub moved: Vec<MovedPoint>,
    /// The sketch's solve; null when it was not solved again.
    pub sketch: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

fn drag_required(schema: &mut Schema) {
    require(schema, "entity");
    require(schema, "to");
}

/// Drag a sketch point or curve from where it was grabbed to a position: points and the
/// material point grabbed on a line move toward it within the design constraints, and when exact
/// placement is impossible, bounded feasible steps minimize the cursor distance. Splines slide
/// by `to` minus `from`; a circle or arc grabbed on its edge takes the radius to `to`. Long
/// moves keep the solution branch, and arcs cannot sweep through their starts. With
/// `live: true` (one frame of a drag; such calls are not checked against this schema yet), later
/// features stay stale until `sketch.drag_end`, and `dof` stays from before the drag.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = drag_required)]
pub struct DragParams {
    // `entity` and `to` are optional in the type and required in the schema (`drag_required`).
    /// The point or curve to drag, by id.
    #[serde(default, deserialize_with = "entity_id", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub entity: Option<String>,
    /// Where it was grabbed, in sketch coordinates (mm). Default: `to`.
    #[serde(default, deserialize_with = "from_point", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Point2>")]
    pub from: Option<Point2>,
    /// Where it is dragged to, in sketch coordinates (mm).
    #[serde(default, deserialize_with = "to_point", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub to: Option<Point2>,
    /// Other points and curves that move with the entity, as one translation (circles keep
    /// their radii).
    #[serde(default, deserialize_with = "with_ids", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<DragWith>")]
    pub with: Option<DragWith>,
    /// One frame of an interactive drag: the sketch is solved and shown, and the features after
    /// it stay as they were until `sketch.drag_end`. Default: false.
    #[serde(default, deserialize_with = "live", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub live: Option<bool>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// What `sketch.drag` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DragResult {
    /// The sketch's solve; null when it was not solved again.
    pub sketch: Option<SketchSolve>,
    /// How far the drag got: progress projected onto the requested move, 0 to 1.
    pub reached: f64,
    /// Where the grabbed position is now, for the next frame: [x, y].
    pub at: [f64; 2],
    /// Whether work stopped at its budget after making progress: another frame can continue.
    pub retry: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The constraint a drag let go on, as `sketch.drag_end` reads `constraint`.
#[derive(Clone, Debug, PartialEq)]
pub struct DragConstraint {
    /// A `sketch.constraint.*` command.
    pub command: String,
    /// Its parameters, as that command takes them. Default: none.
    pub params: Option<Value>,
}

impl<'de> Deserialize<'de> for DragConstraint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        let name = value.get("command").and_then(Value::as_str).unwrap_or_default();
        if !crate::cmd::find_command(name).is_some_and(|spec| spec.id.starts_with("sketch.constraint.")) {
            return Err(refuse("`constraint.command` must be a sketch.constraint.* command"));
        }
        Ok(DragConstraint { command: name.to_string(), params: value.get("params").cloned() })
    }
}

impl JsonSchema for DragConstraint {
    fn schema_name() -> Cow<'static, str> {
        "DragConstraint".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The constraint the drag let go on: a `sketch.constraint.*` command and its parameters. It is added when it does not over-constrain the sketch.",
            "type": "object",
            "properties": {
                "command": {"description": "A sketch.constraint.* command id.", "type": "string", "pattern": "^sketch\\.constraint\\."},
                "params": {"description": "The command's parameters."}
            },
            "required": ["command"]
        })
    }
}

/// End a drag made of `sketch.drag` frames with `live: true`: add the constraint it let go on
/// (not kept if it over-constrains the sketch), then evaluate the whole model once. Returns the
/// constraint's result and the sketch's solved state and degrees of freedom. It reads only the
/// design, not the evaluated model.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DragEndParams {
    /// The constraint to add, `{command, params}`. Default: none.
    #[serde(default)]
    pub constraint: Option<DragConstraint>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// What `sketch.drag_end` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DragEndResult {
    /// What the constraint command returned; `{"error": message}` when it could not be added (it
    /// is not kept); null when no constraint was given.
    pub constraint: Value,
    /// The sketch's solve; null when it was not solved again.
    pub sketch: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// sketch.dimension_text

/// `sketch.dimension_text`'s keys.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = dimension_text_required)]
pub struct DimensionTextKeys {
    // `dimension` is optional in the type and required in the schema, and so is `at` unless
    // `reset` is true (`dimension_text_required`).
    /// The dimension: a constraint id, or the name of the parameter it drives.
    #[serde(default, deserialize_with = "dimension_id", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub dimension: Option<String>,
    /// Where the text's centre goes, in sketch coordinates (mm). Not read with `reset: true`.
    #[serde(default, deserialize_with = "at_point", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub at: Option<Point2>,
    /// True puts the text back in its default place; `at` is then not read. Default: false.
    #[serde(default, deserialize_with = "reset", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub reset: Option<bool>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn dimension_text_required(schema: &mut Schema) {
    require(schema, "dimension");
    schema.insert("anyOf".into(), json!([{"required": ["at"]}, {"required": ["reset"], "properties": {"reset": {"enum": [true]}}}]));
}

/// Move the text of a dimension to a position in the sketch, or back to its default place. The
/// text is kept relative to the dimension, so it follows the dimension when the sketch changes.
/// One undo step.
#[derive(Debug)]
pub struct DimensionTextParams(pub DimensionTextKeys);

impl<'de> Deserialize<'de> for DimensionTextParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        // `at` is read only without `reset: true`.
        if value.get("reset").and_then(Value::as_bool) == Some(true)
            && let Some(keys) = value.as_object_mut()
        {
            keys.remove("at");
        }
        serde_path_to_error::deserialize(&value).map(DimensionTextParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for DimensionTextParams {
    fn schema_name() -> Cow<'static, str> {
        "DimensionTextParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = DimensionTextKeys::json_schema(generator);
        schema.insert(
            "description".into(),
            "Move the text of a dimension to a position in the sketch, or back to its default place. The text is kept relative to the dimension, so it follows the dimension when the sketch changes. One undo step."
                .into(),
        );
        schema
    }
}

/// What `sketch.dimension_text` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DimensionTextResult {
    /// The dimension's constraint id.
    pub dimension: String,
    /// The sketch's solve; null when it was not solved again.
    pub sketch: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// sketch.options

/// Show or hide what a sketch's palette shows, and read the options. Without any option, only
/// reads them. Changing an option is one undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct OptionsParams {
    /// Show the profiles (the filled regions).
    #[serde(default, deserialize_with = "show_profile", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub show_profile: Option<bool>,
    /// Show the points.
    #[serde(default, deserialize_with = "show_points", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub show_points: Option<bool>,
    /// Show the dimensions.
    #[serde(default, deserialize_with = "show_dimensions", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub show_dimensions: Option<bool>,
    /// Show the constraint marks.
    #[serde(default, deserialize_with = "show_constraints", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub show_constraints: Option<bool>,
    /// Show projected geometry.
    #[serde(default, deserialize_with = "show_projected", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub show_projected: Option<bool>,
    /// Show construction geometry.
    #[serde(default, deserialize_with = "show_construction", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub show_construction: Option<bool>,
    /// Show the grid.
    #[serde(default, deserialize_with = "grid", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub grid: Option<bool>,
    /// Clicks snap to the grid.
    #[serde(default, deserialize_with = "snap", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub snap: Option<bool>,
    /// Cut away the model in front of the sketch plane while editing.
    #[serde(default, deserialize_with = "slice", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub slice: Option<bool>,
    /// Make it a 3D sketch: drawing may leave the sketch plane.
    #[serde(default, deserialize_with = "sketch_3d", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub sketch_3d: Option<bool>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// A sketch's palette options: true is shown (or on).
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchOptions {
    /// The profiles are shown.
    pub show_profile: bool,
    /// The points are shown.
    pub show_points: bool,
    /// The dimensions are shown.
    pub show_dimensions: bool,
    /// The constraint marks are shown.
    pub show_constraints: bool,
    /// Projected geometry is shown.
    pub show_projected: bool,
    /// Construction geometry is shown.
    pub show_construction: bool,
    /// The grid is shown.
    pub grid: bool,
    /// Clicks snap to the grid.
    pub snap: bool,
    /// The model in front of the sketch plane is cut away while editing.
    pub slice: bool,
    /// It is a 3D sketch: drawing may leave the sketch plane.
    pub sketch_3d: bool,
}

/// What `sketch.options` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OptionsResult {
    /// The options as they are, or as they are after a change; with a change, also the sketch's
    /// solve.
    #[serde(flatten)]
    pub options: OptionsReply,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The options as they are, or as they are after a change.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum OptionsReply {
    /// No option was given: only read.
    Read {
        /// All the options.
        options: SketchOptions,
    },
    /// Options were given: set.
    Changed {
        /// All the options, after the change.
        options: SketchOptions,
        /// The sketch's solve; null when it was not solved again.
        sketch: Option<SketchSolve>,
    },
}

// ---------------------------------------------------------------------------------------------
// sketch.chain, sketch.solve

fn curve_required(schema: &mut Schema) {
    require(schema, "curve");
}

/// The curves joined end to end to a curve, as a double-click selects them. The design is not
/// changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = curve_required)]
pub struct ChainParams {
    // Optional in the type and required in the schema (`curve_required`).
    /// The curve, by id.
    #[serde(default, deserialize_with = "curve_id", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub curve: Option<String>,
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// What `sketch.chain` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChainResult {
    /// The chain's curves, by id, the given one included.
    pub curves: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Report a sketch's solver status and degrees of freedom, as last evaluated. The design is not
/// changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SolveParams {
    /// The sketch. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Whether the solver satisfied the constraints: `solved` or `failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SolveOutcome {
    // `solved`: all constraints are satisfied; `failed`: they conflict (or the solver could not
    // satisfy them) and the geometry stays as it was before solving.
    Solved,
    Failed,
}

impl From<solidcraft_sketch::SolveStatus> for SolveOutcome {
    fn from(status: solidcraft_sketch::SolveStatus) -> Self {
        match status {
            solidcraft_sketch::SolveStatus::Solved => SolveOutcome::Solved,
            solidcraft_sketch::SolveStatus::Failed => SolveOutcome::Failed,
        }
    }
}

/// What `sketch.solve` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SolveResult {
    /// The solver's status.
    pub status: SolveOutcome,
    /// The degrees of freedom left (0: fully constrained).
    pub dof: usize,
    /// The constraints still violated after a failed solve, by id.
    pub failing: Vec<String>,
    /// How many profiles the sketch has.
    pub profiles: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_session_tests.rs"]
mod tests;
