//! Contract and result types for `selection.get`, `cmd/selection.rs` (#15900).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// Items resolved in full; the rest of a huge selection is listed by identity only.
pub(crate) const MAX_DETAIL: usize = 500;

/// Whether to resolve each item's geometry, as the handler reads it: true or false, with its
/// message. `null` is left out (`Option`), as the handler reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detail(pub bool);

impl<'de> Deserialize<'de> for Detail {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Bool(b) => Ok(Detail(b)),
            _ => Err(D::Error::custom("must be true or false")),
        }
    }
}

impl JsonSchema for Detail {
    fn schema_name() -> Cow<'static, str> {
        "Detail".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "true or false.", "type": "boolean"})
    }
}

/// Read what is selected.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionGetParams {
    /// Resolve each item's geometry (default true). Only the first 500 items are resolved; the
    /// reply says so in `detail_limit`.
    #[serde(default)]
    pub detail: Option<Detail>,
}

/// One selected item: its own fields as `select.set` takes them, `kind` (the same as `type`),
/// and with `detail` what the handler resolves for it. The figures it copies in from
/// `inspect.measure` stay open.
#[derive(Debug, Serialize)]
pub struct SelectionItem(pub Map<String, Value>);

impl JsonSchema for SelectionItem {
    fn schema_name() -> Cow<'static, str> {
        "SelectionItem".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let kinds = [
            "occurrence",
            "body",
            "edge",
            "face",
            "feature",
            "sketch_curve",
            "sketch_point",
            "sketch_midpoint",
            "external_sketch_curve",
            "external_sketch_point",
            "sketch_constraint",
            "profile",
            "plane",
            "construction_point",
            "axis",
            "vertex",
        ];
        json_schema!({
            "description": "A selected item: its fields as `select.set` takes them, `kind` as `type`, and with `detail` what is resolved for it.",
            "type": "object",
            "required": ["kind", "type"],
            "properties": {
                "kind": {"description": "What the item is; the same as `type`.", "enum": kinds},
                "type": {"description": "What the item is, as `select.set` names it.", "enum": kinds},
                "surface": {"description": "A face's surface: its type and the figures that go with it.", "type": "object"},
                "feature": {"description": "A body's feature: {id, name}.", "type": "object"},
                "visible": {"description": "Whether a body is shown.", "type": "boolean"},
                "component": {"description": "The component, {id, name}, of an item in an assembly.", "type": "object"},
                "occurrence_path": {"description": "The occurrence ids from the top of an assembly.", "type": "array", "items": {"type": "integer"}},
                "sketch": {"description": "A sketch item's sketch, by feature id.", "type": "integer"},
                "sketch_name": {"description": "That sketch's name.", "type": "string"},
                "entity": {"description": "What a sketch item is: line, circle, arc, point, midpoint and so on.", "type": "string"},
                "position": {"description": "Where a sketch point or midpoint is."},
                "name": {"description": "A face's or edge's persistent name, a feature's or an occurrence's name.", "type": "string"},
                "name_exact": {"description": "Whether a face's name is exact."},
                "feature_type": {"description": "A feature's type.", "type": "string"},
                "suppressed": {"description": "Whether a feature is suppressed.", "type": "boolean"},
                "start": {"description": "An edge's start."},
                "end": {"description": "An edge's end."},
                "direction": {"description": "An edge's direction."},
                "grounded": {"description": "Whether an occurrence is grounded.", "type": "boolean"},
                "bodies": {"description": "An occurrence's bodies, by world name.", "type": "array", "items": {"type": "string"}},
                "driven": {"description": "Whether a constraint is a reference dimension.", "type": "boolean"},
                "parameter": {"description": "The parameter a dimension is named after.", "type": "string"},
                "construction": {"description": "Whether a sketch curve is construction geometry.", "type": "boolean"}
            },
            "additionalProperties": true
        })
    }
}

/// What `selection.get` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionGetResult {
    /// How many items are selected.
    pub count: usize,
    /// The selected items, in order.
    pub items: Vec<SelectionItem>,
    /// With `detail`, when more than this many items are selected: how many were resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_limit: Option<usize>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
