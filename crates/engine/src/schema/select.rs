//! Contracts and result types for the selection commands in `cmd/edit.rs`: `select.set`,
//! `select.filter`, `select.clear`, `select.same_look`, `select.edge_loop`, `select.sharp_edges` and
//! `select.parallel_edges`.
//!
//! `select.set` reads `items` and `add` from the raw JSON. Each item reads as the handler reads
//! it ([`read_item`] is the handler's own first step, so a call the handler takes is a call the
//! contract takes): a face or edge with an `index` or a `point` (not both) is read as a partial
//! pick, which the handler completes against the design, and anything else is read as a
//! [`Sel`]. Whether a body, face or edge is there, and how many items are too many, stay the
//! handler's to check. A call with several problems may get a different one first in strict than
//! with the check off (the check goes key by key, the handler item by item against the design).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcore_record::Vec3;

use super::check::whole;
use crate::Sel;

/// A face or edge picked by an index or a point only, which the handler completes.
#[derive(Debug, Deserialize)]
pub struct PartialPick {
    pub body: String,
    pub index: Option<usize>,
    pub point: Option<Vec3>,
}

/// One item of `select.set`, as the handler first reads it.
#[derive(Debug)]
pub enum SelectItem {
    /// A complete selection item.
    Exact(Sel),
    /// A face (`face` true) or edge given by its body and one of an index or a point.
    Pick { face: bool, pick: PartialPick },
}

/// Read one item of `select.set`; the error is the handler's message. A face or edge that has
/// both `index` and `point` (even `null`) is read as a [`Sel`], so is any other type; a face or
/// edge with at most one of them is read as a [`PartialPick`], which reads only `body`, `index`
/// and `point`.
pub(crate) fn read_item(value: Value) -> Result<SelectItem, String> {
    let kind = value.get("type").and_then(Value::as_str);
    if !matches!(kind, Some("face" | "edge")) || (value.get("index").is_some() && value.get("point").is_some()) {
        return serde_json::from_value(value).map(SelectItem::Exact).map_err(|e| e.to_string());
    }
    let face = kind == Some("face");
    serde_json::from_value(value).map(|pick| SelectItem::Pick { face, pick }).map_err(|e| e.to_string())
}

impl<'de> Deserialize<'de> for SelectItem {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        read_item(Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))
    }
}

/// A point in world mm.
fn point() -> Value {
    serde_json::json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3})
}

impl JsonSchema for SelectItem {
    fn schema_name() -> Cow<'static, str> {
        "SelectItem".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let text = || serde_json::json!({"type": "string"});
        let count = || serde_json::json!({"type": "integer", "minimum": 0});
        let variant = |kind: &str, what: &str, properties: Value, required: &[&str]| {
            let required: Vec<&str> = std::iter::once("type").chain(required.iter().copied()).collect();
            let mut all = serde_json::json!({"type": {"const": kind}});
            if let (Some(all), Some(properties)) = (all.as_object_mut(), properties.as_object()) {
                all.extend(properties.clone());
            }
            serde_json::json!({"description": what, "type": "object", "properties": all, "required": required})
        };
        let pick = |kind: &str, what: &str| {
            let mut v = variant(kind, what, serde_json::json!({"body": text(), "index": count(), "point": point()}), &["body"]);
            v["anyOf"] = serde_json::json!([{"required": ["index"]}, {"required": ["point"]}]);
            v
        };
        json_schema!({
            "description": "What to select, by `type`. Keys a type doesn't read are ignored. A face or edge needs `body` and an `index` or a `point` (both are accepted); the engine finds the other.",
            "anyOf": [
                variant("body", "A body, by name.", serde_json::json!({"name": text()}), &["name"]),
                pick("face", "A face of a body, by index or by a point on it (world mm, within Measure's pick tolerance)."),
                pick("edge", "An edge of a body, by index or by a point on it (world mm, within Measure's pick tolerance)."),
                variant("vertex", "A vertex of a body, at a point.", serde_json::json!({"body": text(), "point": point()}), &["body", "point"]),
                variant("occurrence", "A placed component, by its path of occurrence ids.", serde_json::json!({"path": {"type": "array", "items": count()}}), &["path"]),
                variant("feature", "A timeline feature, by id.", serde_json::json!({"id": count()}), &["id"]),
                variant("sketch_curve", "A curve of the active sketch, by id; `sketch` names the sketch it is in.", serde_json::json!({"id": text(), "sketch": {"type": ["integer", "null"], "minimum": 0}}), &["id"]),
                variant("sketch_point", "A point of the active sketch, by id.", serde_json::json!({"id": text()}), &["id"]),
                variant("sketch_midpoint", "The midpoint of a line of the active sketch, by id.", serde_json::json!({"id": text()}), &["id"]),
                variant("external_sketch_curve", "A curve of another sketch.", serde_json::json!({"sketch": count(), "id": text()}), &["sketch", "id"]),
                variant("external_sketch_point", "A point of another sketch.", serde_json::json!({"sketch": count(), "id": text()}), &["sketch", "id"]),
                variant("sketch_constraint", "A constraint or dimension of the active sketch, by id.", serde_json::json!({"id": text()}), &["id"]),
                variant("profile", "A closed profile of a sketch, by sketch id and index.", serde_json::json!({"sketch": count(), "index": count()}), &["sketch", "index"]),
                variant("plane", "An origin plane (XY, XZ, YZ) or a construction plane, by name.", serde_json::json!({"name": text()}), &["name"]),
                variant("construction_point", "A construction point, by feature name.", serde_json::json!({"name": text()}), &["name"]),
                variant("axis", "An origin axis (X, Y, Z) or a construction axis, by name.", serde_json::json!({"name": text()}), &["name"])
            ]
        })
    }
}

/// Make what is selected the given items, or add them to it. Nothing changes when an item is
/// refused. A body, face or edge must exist in the design; a body or occurrence that isn't there
/// is refused. At most 10,000 items. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectSetParams {
    /// What to select. Default: none (the selection is cleared, or left as it is with `add`).
    /// A face or edge may give just an `index` or just a `point`; the engine fills in the other.
    #[serde(default)]
    pub items: Option<Vec<SelectItem>>,
    /// Add to the current selection instead of replacing it. Items already selected are not
    /// added twice. Default: false.
    #[serde(default)]
    pub add: Option<bool>,
}

/// What `select.set` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectSetResult {
    /// How many items are selected now.
    pub selected: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Select the objects that use the same appearance as a body, or as one face of it. `body` names a
/// body in the design and defaults to the body of the current selection; `face` says which face to
/// take the appearance from and defaults to the selected face when the call doesn't name another
/// body. A call with neither and no selection is refused, as is a body or face that isn't there and
/// more than 10,000 matches. What it selects is `select.set`'s own reply. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SameLookParams {
    // Unknown keys are refused by serde here, not only by the check: the handler read this type
    // through a `deny_unknown_fields` struct before, and a key it doesn't take stays refused with
    // the same message at every parameter-check level. The keys are `Option` without
    // `serde(default)`, as that struct's were: a call whose parameters aren't an object at all,
    // such as `[]`, keeps the same "missing field" refusal.
    /// The name of a body in the design. Default: the body of the current selection.
    pub body: Option<String>,
    /// Which face of that body to take the appearance from. Default: the face of the current
    /// selection, when the call doesn't name another body.
    pub face: Option<usize>,
}

/// Select every straight edge of a solid body that runs parallel to one straight edge of it (the
/// edge itself included): four on a box. Directions are the edges' exact lines, never the mesh,
/// and parallel means within a hundredth of a degree, either way along. `edge` is an edge index of
/// `body`; both default to the selected edge, so the command runs with no parameters. A curved
/// edge or a mesh body is refused. What it selects is `select.set`'s own reply. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ParallelEdgesParams {
    /// The name of a solid body in the design. Default: the body of the selected edge.
    #[serde(default)]
    pub body: Option<String>,
    /// The index of a straight edge of that body. Default: the selected edge.
    #[serde(default)]
    pub edge: Option<usize>,
}

/// Select the sharp edges of solid bodies: every edge between two faces whose outward normals
/// turn by more than `angle_deg` across it somewhere along it (0 for faces that continue each
/// other, 90 at a box's corner). The angle is measured on the faces' exact surfaces, never on the
/// display mesh, and tangent edges (within a degree) are never selected. `body` names one body;
/// without it every shown solid body is searched. Mesh bodies are left out. What it selects is
/// `select.set`'s own reply. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SharpEdgesParams {
    /// The name of a solid body in the design. Default: every shown solid body.
    #[serde(default)]
    pub body: Option<String>,
    /// The threshold in degrees, 1 to 179: an edge is selected where its faces' normals turn by
    /// more than this. Default: 30.
    #[serde(default)]
    #[schemars(range(min = 1.0, max = 179.0))]
    pub angle_deg: Option<f64>,
    /// Add to the current selection instead of replacing it. Default: false.
    #[serde(default)]
    pub add: Option<bool>,
}

/// Select the closed loop of edges around a face that holds one edge (#18094): the edges of that
/// face's boundary loop, read from the body's exact topology, not from its mesh. `body` and `edge`
/// name the edge together; without them, the last selected edge. `face` says which of the edge's
/// faces gives the loop. Without it, a flat face comes before a curved one (a fillet or a hole's
/// wall), then a face with more than one loop (the edge bounds a pocket, a hole or a boss on it),
/// then the face with the lowest index. The loop follows the face, so a vertex where three or more
/// edges meet does not make the choice ambiguous. A mesh body has no edge loops and is refused.
/// Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EdgeLoopParams {
    /// The name of a body in the design. Given with `edge`; default: the body of the last selected edge.
    pub body: Option<String>,
    /// The index of an edge of that body. Given with `body`; default: the last selected edge.
    pub edge: Option<usize>,
    /// The index of the face whose loop is selected: one of the edge's faces.
    pub face: Option<usize>,
    /// Add the loop to the current selection instead of replacing it. Default: false.
    pub add: Option<bool>,
}

/// What `select.edge_loop` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EdgeLoopResult {
    /// How many items are selected now.
    pub selected: usize,
    /// The face whose loop was selected.
    pub face: usize,
    /// The loop's edges, in the order the loop runs, each once.
    pub edges: Vec<usize>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `select.all`: in an open sketch it selects every curve of that sketch, construction curves
/// too, and every point of it that is on no curve; projected geometry and the sketch's origin are
/// left out. With a drawing open and the model not focused it selects the views, dimensions,
/// balloons and notes on one of its sheets. Otherwise it selects every shown body. It replaces the selection; refused when that
/// would be more than 10,000 items. Not an undo step. Ctrl+A (Cmd+A).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectAllParams {
    /// With a drawing open and the model not focused: the sheet, by its id or its one name.
    /// Default: the drawing's first sheet. Refused otherwise.
    #[serde(default)]
    pub sheet: Option<String>,
}

/// `select.clear` takes no parameters: it deselects everything. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectClearParams {}

/// What `select.clear` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectClearResult {
    /// How many items are selected now: always 0.
    pub selected: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `select.invert` takes no parameters. It selects, in place of the selected faces, the other
/// faces of their bodies; in place of the selected edges, the other edges of their bodies; and in
/// place of the selected bodies, the other bodies that are shown. A body selected together with
/// faces or edges of it is not swapped for the other bodies: the invert stays inside it. Other
/// selected items, such as features or sketch curves, stay selected. Refused when nothing of
/// those three kinds is selected, or when the result would exceed 10,000 items. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectInvertParams {}

/// How fine the window lets a click pick: the three words `select.filter` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SelectMode {
    /// A whole body.
    Body,
    /// One face of a body.
    Face,
    /// One placed component.
    Component,
}

impl SelectMode {
    /// The mode a call names, or none for a word that isn't one. The handler and the contract both
    /// read through this, so they refuse the same words with the same message.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "body" => Some(Self::Body),
            "face" => Some(Self::Face),
            "component" => Some(Self::Component),
            _ => None,
        }
    }
}

fn mode_required(schema: &mut Schema) {
    super::motion::require(schema, "mode");
}

/// Reads `mode` as the handler reads it: one of the three words, refused with its message.
fn select_mode<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SelectMode>, D::Error> {
    super::view_navigation::read_with(d, "mode must be body, face or component", |v| v.as_str().and_then(SelectMode::parse))
}

/// `select.filter`: the pick granularity the window uses. `mode` is optional in the type, so the
/// check leaves its refusal to the handler, and required in the schema, because the handler refuses a
/// call without it. The headless engine only echoes the mode; the window keeps it as its own state.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = mode_required)]
pub struct SelectFilterParams {
    /// How fine a click picks: a whole body, one of its faces, or one placed component.
    #[serde(default, deserialize_with = "select_mode", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SelectMode")]
    pub mode: Option<SelectMode>,
}

/// What `select.filter` returns: the mode it was given.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectFilterResult {
    /// The mode the window picks at now.
    pub mode: SelectMode,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
