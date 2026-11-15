//! Contracts and result types for the command in `cmd/delete.rs`: `selection.delete`.
//!
//! The handler reads its parameters from the raw JSON, so each key here reads as the handler
//! reads it and fails with the handler's message: `items` and `joints` with their own words, the
//! four lists of ids or names (`components`, `occurrences`, `canvases`, `point_clouds`) are read
//! as the handler reads them: what it ignores is ignored, never refused. Whether an item, a joint, an occurrence or a component exists stays the handler's to
//! check. A call with several problems may get a different one first in strict than with the check
//! off (the check goes key by key, the handler in its own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::check::whole;
use crate::Sel;

/// The most items or ids a list takes (the handler's limit).
const MAX_LIST: usize = 10_000;

/// Delete what is selected, in one undo step: sketch entities (while sketching), timeline features
/// and sketches (with the features that depend on them), construction planes and axes, bodies (a
/// Remove feature from here on in the timeline), occurrences and components, canvases, point
/// clouds and joints. `dry_run` reports what would go and what would fail without changing
/// anything. Give something to delete in `items` or one of the lists; otherwise the call is
/// refused.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionDeleteParams {
    /// What to delete, as selection items: `{"type": "feature", "id": 3}`, `{"type": "body",
    /// "name": "Body1"}`, `{"type": "sketch_curve", "id": "c1"}` and so on. At most 10000. Items
    /// that can't be deleted (an origin plane, an edge or vertex, a profile outside the sketch
    /// being edited) are named in `skipped`.
    #[serde(default)]
    pub items: Option<SelectionItems>,
    /// Components to delete, by id or name (not the design itself). Only the first 10000 are
    /// read.
    #[serde(default)]
    pub components: Option<IdList>,
    /// Occurrences to delete, by id or name. One goes alone while its component has others; the
    /// last one takes the component. Only the first 10000 are read.
    #[serde(default)]
    pub occurrences: Option<IdList>,
    /// Canvases and decals to delete, by id or name. Only the first 10000 are read.
    #[serde(default)]
    pub canvases: Option<IdList>,
    /// Point clouds to delete, by id or name. Only the first 10000 are read.
    #[serde(default)]
    pub point_clouds: Option<IdList>,
    /// Joints to delete, by id or name (at most 10000).
    #[serde(default)]
    pub joints: Option<JointList>,
    /// Only report what would be deleted and what would fail, and change nothing. Default:
    /// false.
    #[serde(default)]
    pub dry_run: Option<bool>,
}

/// `items`, as the handler reads it: a list of at most 10000 selection items, each as [`Sel`]
/// reads it. Kept as parsed, for the handler to read again.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionItems(pub Vec<Sel>);

impl<'de> Deserialize<'de> for SelectionItems {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(a) if a.len() <= MAX_LIST => a
                .into_iter()
                .map(|v| serde_json::from_value(v.clone()).map_err(|e| D::Error::custom(whole(format!("bad item {v}: {e}")))))
                .collect::<Result<_, _>>()
                .map(SelectionItems),
            _ => Err(D::Error::custom(whole("`items` must be a list of selection items"))),
        }
    }
}

impl JsonSchema for SelectionItems {
    fn schema_name() -> Cow<'static, str> {
        "SelectionItems".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let point = json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3});
        let text = json!({"type": "string"});
        let count = json!({"type": "integer", "minimum": 0});
        // One object per kind of item: `type` and the keys it needs.
        let item = |kind: &str, keys: &[(&str, &Value)], optional: &[(&str, &Value)]| {
            let mut properties = serde_json::Map::new();
            properties.insert("type".into(), json!({"type": "string", "enum": [kind]}));
            let mut required = vec![json!("type")];
            for (key, schema) in keys {
                properties.insert((*key).into(), (*schema).clone());
                required.push(json!(key));
            }
            for (key, schema) in optional {
                properties.insert((*key).into(), (*schema).clone());
            }
            json!({"type": "object", "properties": properties, "required": required})
        };
        let path = json!({"type": "array", "items": {"type": "integer", "minimum": 0}});
        json_schema!({
            "description": "Selection items: objects with a `type` and the keys that type needs.",
            "type": "array",
            "maxItems": MAX_LIST,
            "items": {"anyOf": [
                item("occurrence", &[("path", &path)], &[]),
                item("body", &[("name", &text)], &[]),
                item("edge", &[("body", &text), ("index", &count), ("point", &point)], &[]),
                item("face", &[("body", &text), ("index", &count), ("point", &point)], &[]),
                item("feature", &[("id", &count)], &[]),
                item("sketch_curve", &[("id", &text)], &[("sketch", &count)]),
                item("sketch_point", &[("id", &text)], &[]),
                item("sketch_midpoint", &[("id", &text)], &[]),
                item("external_sketch_curve", &[("sketch", &count), ("id", &text)], &[]),
                item("external_sketch_point", &[("sketch", &count), ("id", &text)], &[]),
                item("sketch_constraint", &[("id", &text)], &[]),
                item("profile", &[("sketch", &count), ("index", &count)], &[]),
                item("plane", &[("name", &text)], &[]),
                item("construction_point", &[("name", &text)], &[]),
                item("axis", &[("name", &text)], &[]),
                item("vertex", &[("body", &text), ("point", &point)], &[])
            ]}
        })
    }
}

/// An id (a number from 0 up) or a name, as the handler's list reader takes one.
fn id_or_name<E: serde::de::Error>(v: Value, message: &str) -> Result<String, E> {
    match v {
        Value::Number(n) if n.is_u64() => Ok(n.to_string()),
        Value::String(s) => Ok(s),
        _ => Err(E::custom(message.to_string())),
    }
}

fn id_or_name_schema() -> Value {
    json!({"anyOf": [{"type": "string"}, {"type": "integer", "minimum": 0}]})
}

/// A list of ids or names, read as the handler's `keys()` reads `components`, `occurrences`,
/// `canvases` and `point_clouds`: anything but a list is left out, so are items that are neither a
/// name nor an id, and only the first 10000 count. Nothing is refused, as the handler refuses
/// nothing here.
#[derive(Clone, Debug, PartialEq)]
pub struct IdList(pub Vec<String>);

impl<'de> Deserialize<'de> for IdList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Value::Array(a) = Value::deserialize(d)? else {
            return Ok(IdList(Vec::new()));
        };
        Ok(IdList(a.into_iter().take(MAX_LIST).filter_map(|v| id_or_name::<D::Error>(v, "").ok()).collect()))
    }
}

impl JsonSchema for IdList {
    fn schema_name() -> Cow<'static, str> {
        "IdList".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "Ids (numbers) or names. Anything else is ignored.", "type": "array", "items": id_or_name_schema()})
    }
}

/// `joints`, as the handler reads it: a list of at most 10000 ids or names.
#[derive(Clone, Debug, PartialEq)]
pub struct JointList(pub Vec<String>);

impl<'de> Deserialize<'de> for JointList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Value::Array(a) = Value::deserialize(d)? else {
            return Err(D::Error::custom(whole("`joints` must be a list of ids or names")));
        };
        let message = whole("`joints` must contain at most 10000 ids or names");
        if a.len() > MAX_LIST {
            return Err(D::Error::custom(message));
        }
        a.into_iter().map(|v| id_or_name(v, &message)).collect::<Result<_, _>>().map(JointList)
    }
}

impl JsonSchema for JointList {
    fn schema_name() -> Cow<'static, str> {
        "JointList".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "Joint ids (numbers) or names.", "type": "array", "items": id_or_name_schema(), "maxItems": MAX_LIST})
    }
}

/// What `selection.delete` returns. With `dry_run` it says what would happen.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionDeleteResult {
    /// The names of the timeline features that went (or would go), the selected ones and those
    /// that depended on them.
    pub deleted: Vec<String>,
    /// The ids of the joints that went (or would go): the ones asked for, and those on an
    /// occurrence deleted.
    pub deleted_joints: Vec<u64>,
    /// The bodies still there that got a Remove feature, by name.
    pub bodies: Vec<String>,
    /// The names of the features that would fail (or now fail) to rebuild without what went,
    /// also those after a rolled-back marker.
    pub would_fail: Vec<String>,
    /// Why some items were not deleted (an origin plane, an edge or vertex, …), each reason once.
    pub skipped: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "delete_tests.rs"]
mod tests;
