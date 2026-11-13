//! Contracts and result types for the selection commands in `cmd/edit.rs`: `select.set` and
//! `select.clear`.
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

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
