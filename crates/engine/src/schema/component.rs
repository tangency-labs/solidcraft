//! Contracts and result types for the commands in `cmd/component.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::check::whole;
use super::types::{IdOrName, Point3, PointExpr3, StringList, not_blank_pattern, present};
use crate::cmd::{MirrorAs, mirror_as_form, mirror_symmetry_form};
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

/// Mirror components across a plane. Each occurrence is placed again as its mirror image, as
/// the same part (`as: instance`, for a part symmetric about one of its own mid-planes) or as a
/// new opposite-hand part (`as: opposite_hand`). A placement is a rigid move: the mirror image
/// is placed as the reflection across the plane followed by the part's own reflection in its
/// mid-plane, which shows the mirror image exactly. The mirrored occurrence follows its source
/// and the plane until either is deleted (unless `follow` is false). Joints of the source are
/// not copied. Never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MirrorComponentsParams {
    /// What to mirror: occurrence names (`Bracket:2`) or ids, or component names or ids (their
    /// first occurrence). Not the design's root.
    pub components: StringList,
    /// The mirror plane, in world coordinates: XY, XZ, YZ (or Top, Front, Right), a construction
    /// plane's name or id (followed when it changes), `{origin, normal}` or
    /// `{origin, x_dir, y_dir}` (origin coordinates may be length expressions, followed when
    /// their parameters change).
    pub plane: MirrorPlane,
    /// What the mirror makes. `instance`: a new occurrence of the same component, refused for a
    /// part that is not symmetric about one of its own X, Y or Z mid-planes unless `symmetry`
    /// names one. `opposite_hand`: a new component named `<name> (mirror)`, its bodies the
    /// source's own bodies mirrored in the part's own frame (a Mirror feature in it, rebuilt
    /// when the source's features change), placed so it is the exact mirror image; a component
    /// that holds other components is refused. Default: instance.
    #[serde(default, rename = "as", deserialize_with = "mirror_as", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MirrorAsSchema")]
    pub as_: Option<MirrorAs>,
    /// The part's own mid-plane (through the middle of its box, square to its own X, Y or Z
    /// axis; case ignored) that the placement reflects it in. `auto` (the default): for an
    /// instance, the one of these the part is symmetric about, the one most square to the
    /// mirror plane first; for an opposite hand, the one most square to the mirror plane.
    #[serde(default, deserialize_with = "mirror_symmetry", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MirrorSymmetrySchema")]
    pub symmetry: Option<Option<usize>>,
    /// Keep each mirrored occurrence at the mirror image of its source as the source moves and
    /// the plane changes; a following occurrence is not moved on its own. False places it once.
    /// Default: true.
    #[serde(default)]
    pub follow: Option<bool>,
}

fn mirror_as<'de, D: Deserializer<'de>>(d: D) -> Result<Option<MirrorAs>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => mirror_as_form(&v).map(Some).map_err(|e| D::Error::custom(whole(e))),
    }
}

fn mirror_symmetry<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<usize>>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => mirror_symmetry_form(&v).map(Some).map_err(|e| D::Error::custom(whole(e))),
    }
}

/// The schema of `as`.
pub struct MirrorAsSchema;

impl JsonSchema for MirrorAsSchema {
    fn schema_name() -> Cow<'static, str> {
        "MirrorAs".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "instance or opposite_hand.", "type": "string", "enum": ["instance", "opposite_hand"]})
    }
}

/// The schema of `symmetry`.
pub struct MirrorSymmetrySchema;

impl JsonSchema for MirrorSymmetrySchema {
    fn schema_name() -> Cow<'static, str> {
        "MirrorSymmetry".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "auto, X, Y or Z (case ignored).", "type": "string", "pattern": "^(?i)(auto|x|y|z)$"})
    }
}

/// A plane as `features::plane_param` takes it: a name, a construction plane's id, or an object.
/// Whether it names a plane of the design is the handler's to say.
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorPlane(pub Value);

impl<'de> Deserialize<'de> for MirrorPlane {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            v @ (Value::String(_) | Value::Number(_) | Value::Object(_)) => Ok(MirrorPlane(v)),
            _ => Err(D::Error::custom("must be XY, XZ, YZ, a construction plane name or id, or {origin, normal}")),
        }
    }
}

impl JsonSchema for MirrorPlane {
    fn schema_name() -> Cow<'static, str> {
        "MirrorPlane".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "XY, XZ, YZ, a construction plane's name or id, {origin, normal} or {origin, x_dir, y_dir}.",
            "anyOf": [{"type": "string"}, {"type": "integer", "minimum": 0}, {"type": "object"}]
        })
    }
}

/// One mirrored occurrence.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MirroredOccurrence {
    /// The occurrence it mirrors.
    pub source: u64,
    /// The new occurrence, placed at the mirror image.
    pub occurrence: u64,
    /// Its name (`Bracket:2`, `Bracket (mirror):1`).
    pub name: String,
    /// Its component: the source's for an instance, the new opposite-hand part's otherwise.
    pub component: u64,
    /// `instance` or `opposite_hand`.
    #[serde(rename = "as")]
    pub kind: String,
    /// The part's own axis square to the mid-plane it is reflected in: X, Y or Z.
    pub symmetry: String,
    /// Whether it follows its source and the plane.
    pub follows: bool,
    /// The Mirror feature that makes an opposite-hand part's bodies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<u64>,
}

/// What `component.mirror` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MirrorComponentsResult {
    /// The new occurrences, in the order of `components`.
    pub occurrences: Vec<u64>,
    /// Each one with what it mirrors and how.
    pub mirrored: Vec<MirroredOccurrence>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "component_tests.rs"]
mod tests;
