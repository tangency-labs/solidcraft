//! Contracts and result types for the commands in `cmd/component.rs`.

use std::borrow::Cow;
use std::collections::BTreeSet;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::Occurrence;

use super::check::{located, whole};
use super::component_link::{ComponentRef, component_key, require};
use super::types::{IdOrName, Point3, PointExpr3, not_blank_pattern, present};
use crate::params;

/// Edit a linked component's design in place. In the app window: open the design it links in a
/// tab of its own, with the rest of the assembly drawn faded around it as the placement `path`
/// puts it; `component.finish_edit` goes back. It fails when the component is not linked, the
/// assembly has no file yet, something is open or running in it, or the linked file is missing,
/// on a network location not allowed yet or open in another SolidCraft. Asked while the
/// assembly's agent is in its turn, the part opens once the turn ends (`queued`). Outside the
/// window it only checks the component and the placement.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EditInPlaceParams {
    /// The linked component, by id or name.
    pub component: IdOrName,
    /// The placement to edit it at, as occurrence ids from the top of the assembly down
    /// (`component.list`). Default: the component's first placement.
    #[serde(default, deserialize_with = "present")]
    pub path: Option<Vec<u64>>,
}

/// What `component.edit_in_place` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct EditInPlaceResult {
    /// In the app window, asked while the design's agent is in its turn: the part opens once the
    /// turn ends (Stop cancels it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
    /// The component, by id.
    pub component: u64,
    /// The placement it is edited at.
    pub path: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Finish editing a linked component's design in place. In the app window: go back to the
/// assembly around it, which reads the link again, and close the part's tab when the edit opened
/// it. It fails while the part has unsaved changes (save it first) or something is open in it;
/// asked while the part's agent is in its turn, it finishes once the turn ends (`queued`). Outside the window
/// nothing is edited in place, so nothing changes.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FinishEditParams {}

/// What `component.finish_edit` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct FinishEditResult {
    /// In the app window, asked while the part's agent is in its turn: the edit finishes once the
    /// turn ends (Stop cancels it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One exact occurrence placement path (`1..=64` occurrence ids, root-first).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OccurrencePath(pub Vec<u64>);

impl<'de> Deserialize<'de> for OccurrencePath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        v.as_array()
            .filter(|a| !a.is_empty() && a.len() <= 64)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .map(OccurrencePath)
            .ok_or_else(|| D::Error::custom("must list 1..64 occurrence ids"))
    }
}

impl JsonSchema for OccurrencePath {
    fn schema_name() -> Cow<'static, str> {
        "OccurrencePath".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An exact placement path: 1 to 64 occurrence ids from the root down.",
            "type": "array",
            "items": {"type": "integer", "minimum": 0, "maximum": u64::MAX},
            "minItems": 1,
            "maxItems": 64
        })
    }
}

/// A list of `1..=64` exact occurrence placement paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OccurrencePaths(pub Vec<Vec<u64>>);

impl<'de> Deserialize<'de> for OccurrencePaths {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let items = v.as_array().filter(|a| !a.is_empty() && a.len() <= 64).ok_or_else(|| D::Error::custom("must list 1..64 occurrence paths"))?;
        items
            .iter()
            .map(|item| OccurrencePath::deserialize(item).map(|p| p.0).map_err(D::Error::custom))
            .collect::<Result<Vec<_>, _>>()
            .map(OccurrencePaths)
    }
}

impl JsonSchema for OccurrencePaths {
    fn schema_name() -> Cow<'static, str> {
        "OccurrencePaths".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let path = generator.subschema_for::<OccurrencePath>();
        json_schema!({
            "description": "1 to 64 exact occurrence placement paths.",
            "type": "array",
            "items": path,
            "minItems": 1,
            "maxItems": 64
        })
    }
}

/// One occurrence or a list of `1..=64` occurrences, each by id or name.
#[derive(Clone, Debug, PartialEq)]
pub struct OccurrenceList(pub Vec<String>);

impl<'de> Deserialize<'de> for OccurrenceList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom("must be an occurrence id/name or a list of 1..64 ids/names");
        match Value::deserialize(d)? {
            v @ (Value::Number(_) | Value::String(_)) => IdOrName::deserialize(v).map(|x| OccurrenceList(vec![x.0])).map_err(|_| fail()),
            Value::Array(a) if !a.is_empty() && a.len() <= 64 => {
                a.into_iter().map(|v| IdOrName::deserialize(v).ok().map(|x| x.0)).collect::<Option<Vec<_>>>().map(OccurrenceList).ok_or_else(fail)
            }
            _ => Err(fail()),
        }
    }
}

impl JsonSchema for OccurrenceList {
    fn schema_name() -> Cow<'static, str> {
        "OccurrenceList".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item = generator.subschema_for::<IdOrName>();
        json_schema!({
            "description": "An occurrence id/name, or 1 to 64 occurrence ids/names.",
            "anyOf": [
                item.clone(),
                {"type": "array", "items": item, "minItems": 1, "maxItems": 64}
            ]
        })
    }
}

const SNAPS: [&str; 4] = ["vertex", "edge_mid", "face_center", "circle_center"];

/// An alignment point: `[x, y, z]`, `"origin"`, or `{snap?: vertex|edge_mid|face_center|circle_center, at: [x, y, z]}`.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignPickParam(pub Value);

impl<'de> Deserialize<'de> for AlignPickParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        if v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("origin")) {
            return Ok(AlignPickParam(v));
        }
        if let Some(o) = v.as_object()
            && (o.contains_key("at") || o.contains_key("snap"))
        {
            if o.get("at").and_then(params::vec3).is_none() {
                return Err(D::Error::custom("`.at` must be [x, y, z]"));
            }
            if let Some(snap) = o.get("snap") {
                let Some(s) = snap.as_str() else {
                    return Err(D::Error::custom("`.snap` must be vertex, edge_mid, face_center or circle_center"));
                };
                if !SNAPS.contains(&s) {
                    return Err(D::Error::custom("`.snap` must be vertex, edge_mid, face_center or circle_center"));
                }
            }
            return Ok(AlignPickParam(v));
        }
        if params::vec3(&v).is_some() {
            return Ok(AlignPickParam(v));
        }
        Err(D::Error::custom("must be [x, y, z], \"origin\", or {snap, at: [x, y, z]}"))
    }
}

impl JsonSchema for AlignPickParam {
    fn schema_name() -> Cow<'static, str> {
        "AlignPick".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A world point [x, y, z], \"origin\", or a snap point {\"snap\": \"vertex\"|\"edge_mid\"|\"face_center\"|\"circle_center\", \"at\": [x, y, z]}.",
            "anyOf": [
                point.clone(),
                {"type": "string", "pattern": "^[Oo][Rr][Ii][Gg][Ii][Nn]$"},
                {
                    "type": "object",
                    "properties": {
                        "snap": {"type": "string", "enum": SNAPS},
                        "at": point
                    },
                    "required": ["at"]
                }
            ]
        })
    }
}

/// A plane parameter as `features::plane_param` reads it: `"XY" | "XZ" | "YZ"`, a construction
/// plane name or feature id, or `{origin?, normal}` / `{origin?, x_dir, y_dir}`.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignPlaneParam(pub Value);

impl<'de> Deserialize<'de> for AlignPlaneParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match &v {
            Value::String(s) if !s.trim().is_empty() && s.len() <= 4096 => Ok(AlignPlaneParam(v)),
            Value::Number(n) if n.as_u64().is_some() => Ok(AlignPlaneParam(v)),
            Value::Object(o) => {
                if let Some(origin) = o.get("origin") {
                    PointExpr3::deserialize(origin).map_err(|_| D::Error::custom("plane `origin` must be [x, y, z]"))?;
                }
                let has_xy = o.get("x_dir").and_then(params::vec3).is_some() && o.get("y_dir").and_then(params::vec3).is_some();
                let has_n = o.get("normal").and_then(params::vec3).is_some();
                if has_xy || has_n { Ok(AlignPlaneParam(v)) } else { Err(D::Error::custom("plane needs x_dir and y_dir, or normal")) }
            }
            _ => Err(D::Error::custom("must be XY, XZ, YZ, a construction plane name or id, or {origin, normal}")),
        }
    }
}

impl JsonSchema for AlignPlaneParam {
    fn schema_name() -> Cow<'static, str> {
        "AlignPlane".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let expr_point = generator.subschema_for::<PointExpr3>();
        json_schema!({
            "description": "XY, XZ, YZ, a construction plane name or feature id, or an explicit plane {origin?, normal} / {origin?, x_dir, y_dir}.",
            "anyOf": [
                {"type": "string", "minLength": 1, "maxLength": 4096, "pattern": not_blank_pattern()},
                {"type": "integer", "minimum": 0, "maximum": u64::MAX},
                json!({
                    "type": "object",
                    "properties": {"origin": expr_point.clone(), "normal": point.clone()},
                    "required": ["normal"]
                }),
                json!({
                    "type": "object",
                    "properties": {"origin": expr_point, "x_dir": point.clone(), "y_dir": point},
                    "required": ["x_dir", "y_dir"]
                })
            ]
        })
    }
}

/// Align one or more component occurrence placements in world coordinates without modifying
/// shared component geometry.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct OccurrenceAlignParams {
    /// A single occurrence to align, by id or name (optionally disambiguated by `path`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub occurrence: Option<IdOrName>,
    /// One or more occurrences to align together, by id or name.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "OccurrenceList")]
    pub occurrences: Option<OccurrenceList>,
    /// Exact placement path `[occ_id, ...]` of a single occurrence.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "OccurrencePath")]
    pub path: Option<OccurrencePath>,
    /// Exact placement paths `[[occ_id, ...], ...]` of one or more occurrences to align together.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "OccurrencePaths")]
    pub paths: Option<OccurrencePaths>,
    /// Source point (`[x, y, z]`, `"origin"`, or `{snap, at}`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "AlignPickParam")]
    pub from: Option<AlignPickParam>,
    /// Point on a planar source face in world coordinates (turns to meet `to_face` or `to_plane`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Point3")]
    pub from_face: Option<Point3>,
    /// Source plane (`XY`, `XZ`, `YZ`, a construction plane, or `{origin, normal}`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "AlignPlaneParam")]
    pub from_plane: Option<AlignPlaneParam>,
    /// Destination point (`[x, y, z]`, `"origin"`, or `{snap, at}`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "AlignPickParam")]
    pub to: Option<AlignPickParam>,
    /// Point on a planar destination face in world coordinates.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Point3")]
    pub to_face: Option<Point3>,
    /// Destination plane (`XY`, `XZ`, `YZ`, a construction plane, or `{origin, normal}`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "AlignPlaneParam")]
    pub to_plane: Option<AlignPlaneParam>,
    /// Flip face/plane normal alignment so normals point the same way instead of meeting opposite.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub flip: Option<bool>,
    /// Capture the aligned position immediately (default: `true`); `false` leaves it pending.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub capture: Option<bool>,
    /// Allow aligning a grounded occurrence while keeping it grounded (default: `false`).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub move_grounded: Option<bool>,
}

/// Result of `occurrence.align`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OccurrenceAlignResult {
    /// Occurrence ids whose placements were updated, in order.
    pub occurrences: Vec<u64>,
    /// Exact placement paths aligned, in order.
    pub paths: Vec<Vec<u64>>,
    /// World-space rigid transform applied (column-major 4×4 matrix).
    pub world_transform: [[f64; 4]; 4],
    /// True when `capture: false` kept the aligned positions as pending moves.
    pub pending: bool,
    /// Aligned paths that sit inside a component placed more than once: they moved in every
    /// placement of it. Left out when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<Vec<u64>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Move timeline features (sketches too) into a component. Their ids, names and timeline order
/// stay, the geometry stays where it is in the world, and the bodies they made move with them.
/// Nothing changes when the move would break a feature, while the timeline is rolled back, or
/// for a component placed more than once.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MoveFeaturesParams {
    /// The features to move, by id or name.
    pub features: Vec<IdOrName>,
    /// The component they go to, by id or name. Default: the active component.
    #[serde(default, deserialize_with = "present")]
    pub component: Option<IdOrName>,
}

/// Move sketches to the part whose face they are drawn on (`component.move_sketches.to_parts`),
/// in one undo step. A sketch whose plane is an exactly attached face of a part's body goes to
/// that part, staying where it is on the face; the others stay where they are.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MoveSketchesToPartsParams {
    /// The sketches to look at, by id or name. Default: every sketch in the design.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Vec<IdOrName>")]
    pub sketches: Option<Vec<IdOrName>>,
}

/// A sketch `component.move_sketches.to_parts` moved.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SketchMovedToPart {
    pub sketch: u64,
    pub name: String,
    /// The part it went to: its component id and name.
    pub component: u64,
    pub part: String,
}

/// A sketch `component.move_sketches.to_parts` left where it was, and why.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SketchKeptInPlace {
    pub sketch: u64,
    pub name: String,
    /// Why, as a code: `not_on_face`, `no_exact_attachment`, `design_body`, `part_missing` or
    /// `already_in_part`; and in words.
    pub code: String,
    pub reason: String,
}

/// What `component.move_sketches.to_parts` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct MoveSketchesToPartsResult {
    /// How many sketches moved, and how many stayed.
    pub moved: usize,
    pub stayed: usize,
    /// The sketches that moved, each with the part it went to.
    pub sketches: Vec<SketchMovedToPart>,
    /// The sketches that stayed, each with the reason.
    pub kept: Vec<SketchKeptInPlace>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `component.move_features` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct MoveFeaturesResult {
    /// The names of the features that moved.
    pub moved: Vec<String>,
    /// The component they went to.
    pub component: u64,
    /// The bodies that moved with them.
    pub bodies: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A placement: an occurrence id, or its path of occurrence ids from the top of the design.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PlacementRef {
    Id(u64),
    Path(Vec<u64>),
}

/// Move placements, timeline features and bodies into a component in one undo step, all or
/// nothing. Placements move first and take what they hold, then features, which take the bodies
/// they made, then the bodies still elsewhere (a body transfer, which leaves its features
/// behind). Anything already in the component is left alone.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ReparentItemsParams {
    /// The placements to move.
    #[serde(default, deserialize_with = "present")]
    pub occurrences: Option<Vec<PlacementRef>>,
    /// The features to move, by id or name.
    #[serde(default, deserialize_with = "present")]
    pub features: Option<Vec<IdOrName>>,
    /// The bodies to move, by name.
    #[serde(default, deserialize_with = "present")]
    pub bodies: Option<Vec<String>>,
    /// The component they go to, by id or name. Default: the active component.
    #[serde(default, deserialize_with = "present")]
    pub component: Option<IdOrName>,
    /// The placement of that component to place them by, as occurrence ids from the top.
    #[serde(default, deserialize_with = "present")]
    pub component_path: Option<Vec<u64>>,
}

/// What `browser.reparent` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ReparentItemsResult {
    /// The component they went to.
    pub component: u64,
    /// The occurrence ids that moved.
    pub occurrences: Vec<u64>,
    /// The names of the features that moved.
    pub features: Vec<String>,
    /// The bodies that were transferred on their own.
    pub bodies: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// component.create, from_bodies, activate, list, properties, rename, delete and unlock (#12504).
//
// The handlers read their parameters from the raw JSON, so each key here reads as its handler
// reads it and fails with the handler's message. Whether a component, an occurrence or a body
// exists stays the handler's to check. A key the handler refuses when it is left out is optional
// in the type and required in the schema, so that the handler's message, not the check's "is
// required", is what a caller sees.
// ---------------------------------------------------------------------------------------------

/// `message`, the handler's, for a value that is not a boolean (`null` included).
fn flag<'de, D: Deserializer<'de>>(d: D, message: &str) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole(message))),
    }
}

fn grounded_flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag(d, "grounded must be true or false")
}

fn descendants_flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    flag(d, "`descendants` must be a boolean")
}

/// A placement path as `component.create` reads `parent_path`: up to 63 occurrence ids (none
/// is the design itself).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentPath(pub Vec<u64>);

impl<'de> Deserialize<'de> for ParentPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_array()
            .filter(|a| a.len() < 64)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .map(ParentPath)
            .ok_or_else(|| D::Error::custom(whole("parent_path must list up to 63 occurrence ids")))
    }
}

impl JsonSchema for ParentPath {
    fn schema_name() -> Cow<'static, str> {
        "ParentPath".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An exact placement path: up to 63 occurrence ids from the root down; empty for the design itself.",
            "type": "array",
            "items": {"type": "integer", "minimum": 0, "maximum": u64::MAX},
            "maxItems": 63
        })
    }
}

fn parent<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ComponentRef>, D::Error> {
    component_key(d, "parent")
}

fn component<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ComponentRef>, D::Error> {
    component_key(d, "component")
}

fn component_required(schema: &mut Schema) {
    require(schema, "component");
}

/// Make a new component, in the active component unless `parent` says another, and make it the
/// active one unless `activate` is false. The design's first part is grounded; the others are
/// not. Bodies are not moved here (`component.move_bodies`, `component.from_bodies`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CreateParams {
    /// The new component's name. Default: a name made from the design's naming.
    #[serde(default)]
    pub name: Option<String>,
    /// The component to put it in, by id or name. Default: the active one.
    #[serde(default, deserialize_with = "parent", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub parent: Option<ComponentRef>,
    /// The exact placement of the parent to place it by, as occurrence ids from the top of the
    /// design (up to 63; none is the design itself). It must belong to the parent. Default: the
    /// active placement when the parent is the active component, else the parent's first.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ParentPath")]
    pub parent_path: Option<ParentPath>,
    /// Make it the active component. Default: `true`.
    #[serde(default)]
    pub activate: Option<bool>,
    /// Ground its placement. Default: `true` for the design's first part, `false` otherwise.
    #[serde(default, deserialize_with = "grounded_flag", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub grounded: Option<bool>,
}

/// What `component.create` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CreateResult {
    /// The new component, by id.
    pub component: u64,
    /// Its name.
    pub name: String,
    /// The component it is in, by id (0 is the design).
    pub parent: u64,
    /// Its placement, by id.
    pub occurrence: Option<u64>,
    /// Whether its placement is grounded.
    pub grounded: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `component.from_bodies`' `bodies`: a list of body names, each once, as `body_names` reads
/// it. The handler also looks each name up in the design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinitionBodies(pub Vec<String>);

impl<'de> Deserialize<'de> for DefinitionBodies {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let names = Value::deserialize(d)?
            .as_array()
            .and_then(|a| a.iter().map(|v| v.as_str().map(str::to_string)).collect::<Option<Vec<_>>>())
            .ok_or_else(|| D::Error::custom(whole("bodies must be an array of body names")))?;
        if names.is_empty() || names.len() > 1000 || names.iter().collect::<BTreeSet<_>>().len() != names.len() {
            return Err(D::Error::custom(whole("bodies must list 1..1000 distinct existing definition bodies")));
        }
        Ok(DefinitionBodies(names))
    }
}

impl JsonSchema for DefinitionBodies {
    fn schema_name() -> Cow<'static, str> {
        "DefinitionBodies".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "1 to 1000 different body names.",
            "type": "array",
            "items": {"type": "string"},
            "minItems": 1,
            "maxItems": 1000,
            "uniqueItems": true
        })
    }
}

fn bodies_required(schema: &mut Schema) {
    require(schema, "bodies");
}

/// Make one new component per body, in the active component and named after the body, and move
/// each body into its component, keeping it where it is in the world. All or nothing, one undo
/// step; refused while the timeline is rolled back.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = bodies_required)]
pub struct FromBodiesParams {
    // Optional in the type and required in the schema (`bodies_required`).
    /// The bodies, by name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "DefinitionBodies")]
    pub bodies: Option<DefinitionBodies>,
}

/// One component `component.from_bodies` made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyComponent {
    /// The new component, by id.
    pub component: u64,
    /// The body it holds, by name.
    pub body: String,
    /// The Body Transfer feature that moved the body, by id.
    pub feature: u64,
}

/// What `component.from_bodies` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FromBodiesResult {
    /// The new components, in the order of `bodies`.
    pub components: Vec<BodyComponent>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// An occurrence by its id (a number) or its name, as `occurrence` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct OccurrenceRef(pub String);

impl<'de> Deserialize<'de> for OccurrenceRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => Ok(OccurrenceRef(n.to_string())),
            Value::String(s) => Ok(OccurrenceRef(s)),
            _ => Err(D::Error::custom(whole("`occurrence` must be an id or name"))),
        }
    }
}

impl JsonSchema for OccurrenceRef {
    fn schema_name() -> Cow<'static, str> {
        "OccurrenceRef".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "An occurrence's id (a number) or its name.", "type": ["number", "string"]})
    }
}

fn activate_schema(schema: &mut Schema) {
    require(schema, "component");
    schema.insert("dependentRequired".into(), json!({"path": ["occurrence"]}));
}

/// Make a component the active one: new sketches and features go into it, authored in its
/// frame. With `occurrence` (and `path`), the placement that shared geometry is authored
/// through. A change of what is active, not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = activate_schema)]
pub struct ActivateKeys {
    // Optional in the type and required in the schema (`activate_schema`).
    /// The component to make active, by id or name (0 or `"root"` for the design).
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
    /// The placement the component is authored in, by id or name, for a component placed more
    /// than once. Needed when `path` is given.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrenceRef")]
    pub occurrence: Option<OccurrenceRef>,
    /// The occurrence's exact placement, as occurrence ids from the top of the design (1 to 64),
    /// ending in `occurrence`, and a placement of the component. Default: the occurrence's
    /// first placement.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrencePath")]
    pub path: Option<OccurrencePath>,
}

/// `component.activate`'s parameters: [`ActivateKeys`], with `path` asked to come with
/// `occurrence`, as the handler asks.
#[derive(Debug)]
pub struct ActivateParams(pub ActivateKeys);

impl<'de> Deserialize<'de> for ActivateParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        let keys: ActivateKeys = serde_path_to_error::deserialize(&value).map_err(|e| D::Error::custom(located(&e)))?;
        if keys.path.is_some() && keys.occurrence.is_none() {
            return Err(D::Error::custom(whole("`occurrence` must be an id or name")));
        }
        Ok(ActivateParams(keys))
    }
}

impl JsonSchema for ActivateParams {
    fn schema_name() -> Cow<'static, str> {
        "ActivateParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ActivateKeys::json_schema(generator)
    }
}

/// What `component.activate` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ActivateResult {
    /// The active component now, by id.
    pub active: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// List the components as a tree: each with its bodies, features, sketches, construction planes,
/// placements and children, the active component and the pending moves. The design is not
/// changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ListKeys {
    /// Counts of bodies, features, sketches and construction planes instead of their names;
    /// placements leave out identity transforms; children as occurrence ids. Default: `false`.
    #[serde(default)]
    pub compact: Option<bool>,
    /// With `compact`: body names, not their count. Read only with `compact: true`. Default:
    /// `false`.
    #[serde(default)]
    pub bodies: Option<bool>,
}

/// `component.list`'s parameters: [`ListKeys`], with `bodies` read only when `compact` is true,
/// as the handler reads it.
#[derive(Debug)]
pub struct ListParams(pub ListKeys);

impl<'de> Deserialize<'de> for ListParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Some(keys) = value.as_object_mut()
            && keys.get("compact").and_then(Value::as_bool) != Some(true)
        {
            keys.remove("bodies");
        }
        serde_path_to_error::deserialize(&value).map(ListParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for ListParams {
    fn schema_name() -> Cow<'static, str> {
        "ListParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ListKeys::json_schema(generator)
    }
}

/// A placement as `component.list` shows it, in full.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlacementInfo {
    /// The occurrence, by id.
    pub id: u64,
    /// The component it places, by id.
    pub component: u64,
    /// The component it sits in, by id (0 is the design).
    pub parent: u64,
    pub name: String,
    /// Its placement in its parent, as a 4×4 matrix.
    pub transform: [[f64; 4]; 4],
    /// Whether it is grounded.
    pub grounded: bool,
}

impl From<&Occurrence> for PlacementInfo {
    fn from(o: &Occurrence) -> Self {
        PlacementInfo { id: o.id, component: o.component, parent: o.parent, name: o.name.clone(), transform: o.transform, grounded: o.grounded }
    }
}

/// A placement as `compact` `component.list` shows it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CompactPlacement {
    /// The occurrence, by id.
    pub id: u64,
    pub name: String,
    /// The component it sits in, by id (0 is the design).
    pub parent: u64,
    /// Whether it is grounded.
    pub grounded: bool,
    /// Its placement in its parent, as a 4×4 matrix; left out when it is the identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[[f64; 4]; 4]>,
}

/// A placement in a component's `occurrences`: in full, or compact.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PlacementEntry {
    Full(PlacementInfo),
    Compact(CompactPlacement),
}

/// A placement in a component's `children`: in full, or with `compact` its id.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ChildEntry {
    Full(PlacementInfo),
    Id(u64),
}

/// Names, or with `compact` how many there are.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum NamesOrCount {
    Names(Vec<String>),
    Count(usize),
}

/// One component of `component.list`; the design itself is the first, with id 0.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ComponentNode {
    /// The component, by id (0 is the design).
    pub id: u64,
    pub name: String,
    /// The component it is in; null for the design.
    pub parent: Option<u64>,
    /// Its bodies by name; with `compact` their count unless `bodies` is true.
    pub bodies: NamesOrCount,
    /// Its features other than sketches and construction planes, by name; with `compact` their count.
    pub features: NamesOrCount,
    /// Its sketches, by name; with `compact` their count.
    pub sketches: NamesOrCount,
    /// Its construction planes, by name; with `compact` their count.
    pub construction: NamesOrCount,
    /// Where it is placed.
    pub occurrences: Vec<PlacementEntry>,
    /// The placements it holds.
    pub children: Vec<ChildEntry>,
}

/// What `component.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListResult {
    /// The active component, by id.
    pub active: u64,
    /// The design and its components.
    pub components: Vec<ComponentNode>,
    /// The occurrences with a move not captured yet, by id.
    pub pending_moves: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `about`, a point in mm or `null`, with the handler's message for anything else.
fn about<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Point3>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => params::vec3(&v).map(|p| Some(Point3(p))).ok_or_else(|| D::Error::custom(whole("`about` must be a point [x, y, z] in mm"))),
    }
}

/// Measure a component where its first placement puts it (every placement for the design):
/// material-aware mass and centre of mass in world coordinates, volume, density, and inertia
/// for closed, consistently oriented bodies, with totals and principal moments. Pending moves
/// are included. The design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PropertiesParams {
    /// The component, by id or name. Default: the active component.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
    /// Include the components and placements inside it. Default: `true`.
    #[serde(default, deserialize_with = "descendants_flag", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub descendants: Option<bool>,
    /// A point in mm, also given the total inertia about it (`total.inertia_about`).
    #[serde(default, deserialize_with = "about", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Point3>")]
    pub about: Option<Point3>,
}

/// A body's box, in world coordinates (mm).
#[derive(Debug, Serialize, JsonSchema)]
pub struct BoundingBox {
    #[schemars(with = "[f64; 3]")]
    pub min: Vec3,
    #[schemars(with = "[f64; 3]")]
    pub max: Vec3,
}

/// One body's measurements at one placement. Volume, density, mass, centre of mass and inertia
/// are null for a body that is not closed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyProperties {
    pub name: String,
    /// The component it belongs to, by id.
    pub component: u64,
    /// The placement it is measured at, as occurrence ids from the top.
    pub path: Vec<u64>,
    /// Its material's name.
    pub material: String,
    /// Whether it is closed and consistently oriented.
    pub closed: bool,
    pub density_g_cm3: Option<f64>,
    pub mass_g: Option<f64>,
    pub volume_mm3: Option<f64>,
    pub area_mm2: f64,
    /// Its centre of mass, in world coordinates (mm).
    #[schemars(with = "Option<[f64; 3]>")]
    pub center_of_mass: Option<Vec3>,
    /// The inertia tensor about its centre of mass, in world axes (g·mm²).
    pub inertia_g_mm2: Option<[[f64; 3]; 3]>,
    pub bbox: BoundingBox,
}

/// The inertia about the point `about` asked for.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InertiaAbout {
    /// The point, in mm.
    #[schemars(with = "[f64; 3]")]
    pub point: Vec3,
    /// The total inertia tensor about it (g·mm²); null when it is not known.
    pub inertia_g_mm2: Option<[[f64; 3]; 3]>,
}

/// The totals of a physical-properties measurement, over the closed bodies unless said.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TotalProperties {
    /// How many open bodies are left out of the volume and mass totals.
    pub open_bodies_skipped: usize,
    pub volume_mm3: f64,
    /// The area of all bodies, closed or not.
    pub area_mm2: f64,
    pub mass_g: f64,
    /// The centre of mass (mm); null when the mass is zero.
    #[schemars(with = "Option<[f64; 3]>")]
    pub center_of_mass: Option<Vec3>,
    /// Mass times centre of mass (g·mm).
    #[schemars(with = "[f64; 3]")]
    pub first_moment_g_mm: Vec3,
    /// The inertia tensor about the centre of mass, in world axes (g·mm²); null when it is not known.
    pub inertia_g_mm2: Option<[[f64; 3]; 3]>,
    /// The principal moments, smallest first (g·mm²).
    pub principal_moments_g_mm2: Option<[f64; 3]>,
    /// The principal axes: unit vectors, right-handed.
    #[schemars(with = "Option<[[f64; 3]; 3]>")]
    pub principal_axes: Option<[Vec3; 3]>,
    /// The box around all bodies; null when there are none.
    pub bbox: Option<BoundingBox>,
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    /// With `about`: the inertia about that point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inertia_about: Option<InertiaAbout>,
}

/// Physical properties of a placement and what it holds, in world coordinates.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PhysicalProperties {
    /// The placement measured, as occurrence ids from the top; empty for the design.
    pub path: Vec<u64>,
    /// Always `world`.
    pub coordinate_frame: String,
    /// Whether what is inside the placement is included.
    pub descendants: bool,
    pub placement_count: usize,
    pub body_count: usize,
    pub bodies: Vec<BodyProperties>,
    pub total: TotalProperties,
}

/// What `component.properties` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PropertiesResult {
    /// The component, by id.
    pub component: u64,
    pub name: String,
    #[serde(flatten)]
    pub properties: PhysicalProperties,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The new name `component.rename` reads: text of 1 to 128 bytes once trimmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentName(pub String);

impl<'de> Deserialize<'de> for ComponentName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if !s.trim().is_empty() && s.trim().len() <= 128 => Ok(ComponentName(s)),
            _ => Err(D::Error::custom(whole("`name` must be 1…128 characters"))),
        }
    }
}

impl JsonSchema for ComponentName {
    fn schema_name() -> Cow<'static, str> {
        "ComponentName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Text with something in it; once surrounding blanks are cut, at most 128 bytes.",
            "type": "string",
            "pattern": not_blank_pattern()
        })
    }
}

fn rename_schema(schema: &mut Schema) {
    require(schema, "component");
    require(schema, "name");
}

/// Rename a component, or the design itself (id 0). Placements still named after it follow.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = rename_schema)]
pub struct RenameParams {
    // Optional in the type and required in the schema (`rename_schema`).
    /// The component, by id or name (0 or `"root"` for the design).
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
    /// The new name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentName")]
    pub name: Option<ComponentName>,
}

/// What `component.rename` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RenameResult {
    /// The component, by id.
    pub component: u64,
    /// Its name now, trimmed.
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Delete a component with its subcomponents, placements and features. Refused for the design
/// itself, and for a tree that supplies a body transfer recorded outside it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = component_required)]
pub struct DeleteParams {
    // Optional in the type and required in the schema (`component_required`).
    /// The component, by id or name.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
}

/// What `component.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeleteResult {
    /// The components removed, by id: it and everything inside it.
    pub deleted: Vec<u64>,
    /// The features that were inside them, by id.
    pub features: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Remove the rigid joints that hold a component's contents together (an imported assembly's
/// rigid group), so its parts move and take joints one by one. Refused for the design and when
/// nothing holds the contents together.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = component_required)]
pub struct UnlockParams {
    // Optional in the type and required in the schema (`component_required`).
    /// The component, by id or name.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
}

/// What `component.unlock` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnlockResult {
    /// The component, by id.
    pub component: u64,
    /// The joints removed, by id.
    pub joints: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "component_tests.rs"]
mod tests;
